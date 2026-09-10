//! The protocol as a table. One lookup a character, and the loop is one line.
//!
//! **What the table holds is the whole decision.** An edge says what the byte
//! does and where the parser stands afterwards; the *action* says how many
//! bytes it eats. So a three-byte character is one lookup, not three -- the
//! edge for its lead byte names `Char3`, and `Char3` consumes three.
//!
//! That is the difference from a byte-at-a-time state machine, which needs a
//! node per continuation and three trips round the loop for one character.
//! Here the continuation nodes exist only for a character split across two
//! `read(2)`s, which is at most one per read.
//!
//! ```text
//! while i < n {
//!     i += self.step(&mut node, base, i);
//! }
//! ```
//!
//! Everything else is a `const fn` that runs at build time and a flat `match`
//! that performs what the table named.

use baud_domain::*;

/// Where the parser stands.
///
/// **No payloads.** A UTF-8 accumulator is a value, not a state, so the
/// continuation count *is* the node and the bits live beside it on `Term`.
/// That is what lets a node be an index; a payload cannot be one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum Node {
    /// Between characters, where anything may start.
    Ground = 0,
    /// A character split across reads, with this many continuations owed.
    Utf8One,
    Utf8Two,
    Utf8Three,
    /// After `ESC`. **A protocol is more nodes, not a runtime flag** -- and
    /// until these existed `ESC` was a C0 control that did nothing and
    /// `[?1049h` was printable ASCII, so every escape sequence in the corpus
    /// was rendered to the screen as its own parameter bytes.
    Escape,
    /// Just after `CSI`, where a private marker is legal.
    ///
    /// `[[source: williams]]` collects `3C-3F` in `csi_entry` and **only**
    /// there: the same bytes in `csi_param` are a grammar violation. Folding
    /// the two into one node made `CSI 1 ? m` parse as a private sequence,
    /// which is the "one write path knowing a rule the other did not" shape
    /// this repo already records four times.
    CsiEntry,
    /// Inside `CSI`, collecting parameters.
    CsiParam,
    /// Inside `CSI`, past the first intermediate.
    CsiInter,
    /// A `CSI` that broke its own grammar: swallow to the final byte.
    CsiIgnore,
    /// The body of a control string -- `OSC`, `DCS`, `SOS`, `PM`, `APC`.
    Str,
    /// An `ESC` inside a control string: `\` ends it, anything else cancels.
    StrEsc,
}

pub(crate) const NNODE: usize = 11;

/// How many actions there are. Only `diag` reads it, to size the histogram
/// that says which of them a wider key would collapse.
#[cfg(feature = "diag")]
pub(crate) const NACT: usize = 27;

impl Node {
    const FROM_INDEX: [Node; NNODE] = [
        Node::Ground,
        Node::Utf8One,
        Node::Utf8Two,
        Node::Utf8Three,
        Node::Escape,
        Node::CsiEntry,
        Node::CsiParam,
        Node::CsiInter,
        Node::CsiIgnore,
        Node::Str,
        Node::StrEsc,
    ];
}

/// What a byte does.
///
/// **A flat set, deliberately.** The table has already decided which node and
/// which byte range this is, so nothing here re-tests either -- and the
/// dispatch is one `match` over eight arms rather than a cascade of byte tests
/// inside twenty.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum Act {
    /// Printable ASCII: a run starts here. The action scans it and blits.
    Text = 0,
    /// **The C0 controls, one action each.** They used to share a single
    /// `Act::Control` and an if-chain over five byte identities in `execute` --
    /// on a path where the control action is 15% of `cat-cjk.raw`'s dispatched
    /// actions and **50-64% of `asciiquarium.raw`'s and `git-log.raw`'s**. The
    /// table already holds a 256-entry row per node and `Act` spends 20 of 256
    /// encodings, so naming the operation costs nothing and removes the only
    /// place in this crate that re-tested which byte it had.
    Index,
    Return,
    Backspace,
    /// HT. **The tab machinery was already here and only the character was
    /// unrouted**: `tab_forward`, `tab_back` and `reset_tabs` exist, `tabs[x]`
    /// is read in three places, and `HTS`/`TBC` write it -- all reachable only
    /// through `CSI`'s `CHT` and `CBT`. `0x09` fell to `Act::Consume`, so the
    /// stops were maintained and the tab was dropped. No recording in the
    /// corpus contains one, which is why nothing caught it.
    Tab,
    /// A complete multi-byte character whose bytes are all in this read.
    /// The action decodes and writes it, and eats its length.
    Char2,
    Char3,
    Char4,
    /// A lead whose continuations have not arrived: start the accumulator and
    /// wait. One per read at most.
    Begin2,
    Begin3,
    Begin4,
    /// A continuation byte, expected, with more still owed.
    Cont,
    /// The continuation that completes a split character: decode and write.
    ContEnd,
    /// A single-byte graphic in a byte protocol: the codepoint is the byte.
    Latin1,
    /// A byte that cannot begin or continue a character: one replacement.
    Bad,
    /// Eat the byte and do nothing: a private marker, an intermediate, the
    /// body of a control string, the `ESC` that opened one.
    Consume,
    /// `ESC [`: start a fresh parameter list.
    CsiStart,
    /// `ESC`: start a fresh sequence. **The parameter state has to be reset
    /// here and not only at `CSI`**, or an `ESC ( B` after a `CSI 1 m` reads
    /// the earlier sequence's intermediate and designates a character set
    /// nobody asked for.
    EscStart,
    /// A parameter digit.
    Digit,
    /// A private marker, `3C-3F`, legal only immediately after `CSI`.
    Marker,
    /// An intermediate byte, `20-2F`. `CSI SP q` and `CSI " q` are different
    /// operations from `CSI q`, so it cannot be consumed and forgotten.
    Inter,
    /// `:` -- a *sub*-parameter separator. It ends a parameter like `;` and
    /// also marks the next one as belonging to the same group, which is what
    /// tells `38:2:r:g:b` from `38;2;r;g;b`.
    SubSep,
    /// `;` or `:`: the parameter ends here.
    Sep,
    /// A `CSI` final byte: perform the operation.
    CsiFinal,
    /// A two-byte `ESC` sequence's final byte.
    EscFinal,
    /// Re-dispatch this byte from the node the edge names, consuming nothing.
    /// `[[source: williams]]`: a bare `ESC` **cancels** a control string, and
    /// the byte after it belongs to the new sequence rather than the old one.
    Again,
    /// A byte that arrived mid-character and cannot continue it. The partial
    /// character becomes a replacement and the byte is **not** consumed --
    /// `[[source: unicode-ch3]]` 3.9.5 requires a converter not to swallow a
    /// successor that is itself well-formed. The node is now `Ground`, where
    /// this byte cannot produce `Resync` again, so the loop still advances.
    Resync,
}

/// Which histogram slot a node is, as an **exhaustive match rather than a
/// cast**. Casting a discriminant to an index is the one thing this crate's
/// audit counts, the budget is a floor to drive down, and diagnostic counters
/// are the last thing that should spend it. The compiler checks this match is
/// total; a cast does not.
#[cfg(feature = "diag")]
pub(crate) fn count_node(h: &mut Hist, n: Node) {
    h.node[match n {
        Node::Ground => 0,
        Node::Utf8One => 1,
        Node::Utf8Two => 2,
        Node::Utf8Three => 3,
        Node::Escape => 4,
        Node::CsiEntry => 5,
        Node::CsiParam => 6,
        Node::CsiInter => 7,
        Node::CsiIgnore => 8,
        Node::Str => 9,
        Node::StrEsc => 10,
    }] += 1;
}

/// The two histograms, so the counting functions return nothing and take no
/// primitive: an index that escapes into a signature is exactly what this
/// crate's audit is for.
#[cfg(feature = "diag")]
#[derive(Clone, Copy)]
pub struct Hist {
    pub node: [usize; NNODE],
    pub act: [usize; NACT],
}

/// A node's name, from the same file the node is declared in.
#[cfg(feature = "diag")]
const fn node_name(n: Node) -> &'static str {
    match n {
        Node::Ground => "Ground",
        Node::Utf8One => "Utf8One",
        Node::Utf8Two => "Utf8Two",
        Node::Utf8Three => "Utf8Three",
        Node::Escape => "Escape",
        Node::CsiEntry => "CsiEntry",
        Node::CsiParam => "CsiParam",
        Node::CsiInter => "CsiInter",
        Node::CsiIgnore => "CsiIgnore",
        Node::Str => "Str",
        Node::StrEsc => "StrEsc",
    }
}

/// An action's name, likewise.
#[cfg(feature = "diag")]
const fn act_name(a: Act) -> &'static str {
    match a {
        Act::Text => "Text",
        Act::Index => "Index",
        Act::Return => "Return",
        Act::Backspace => "Backspace",
        Act::Tab => "Tab",
        Act::Char2 => "Char2",
        Act::Char3 => "Char3",
        Act::Char4 => "Char4",
        Act::Begin2 => "Begin2",
        Act::Begin3 => "Begin3",
        Act::Begin4 => "Begin4",
        Act::Cont => "Cont",
        Act::ContEnd => "ContEnd",
        Act::Latin1 => "Latin1",
        Act::Bad => "Bad",
        Act::Consume => "Consume",
        Act::CsiStart => "CsiStart",
        Act::EscStart => "EscStart",
        Act::Digit => "Digit",
        Act::Marker => "Marker",
        Act::Inter => "Inter",
        Act::SubSep => "SubSep",
        Act::Sep => "Sep",
        Act::CsiFinal => "CsiFinal",
        Act::EscFinal => "EscFinal",
        Act::Again => "Again",
        Act::Resync => "Resync",
    }
}

/// Every action, so a reader can walk them without a parallel list.
#[cfg(feature = "diag")]
const ALL_ACTS: [Act; NACT] = [
    Act::Text,
    Act::Index,
    Act::Return,
    Act::Backspace,
    Act::Tab,
    Act::Char2,
    Act::Char3,
    Act::Char4,
    Act::Begin2,
    Act::Begin3,
    Act::Begin4,
    Act::Cont,
    Act::ContEnd,
    Act::Latin1,
    Act::Bad,
    Act::Consume,
    Act::CsiStart,
    Act::Digit,
    Act::Marker,
    Act::Inter,
    Act::SubSep,
    Act::Sep,
    Act::CsiFinal,
    Act::EscFinal,
    Act::Again,
    Act::Resync,
    Act::EscStart,
];

#[cfg(feature = "diag")]
impl Hist {
    pub const ZERO: Hist = Hist {
        node: [0; NNODE],
        act: [0; NACT],
    };
    /// The counts with their names, **derived rather than paired with a list
    /// the reader keeps.** A parallel name array in the benchmark drifted
    /// twice in one session: `CsiEntry` was added to the graph and not to the
    /// names, so btop reported 57.9% of its iterations in `CsiInter` -- a node
    /// it never enters -- and three actions were reported as `Again` and
    /// `Resync`, which cannot fire on it. The counters were right both times
    /// and the labels were not, which is the harder failure to notice.
    pub fn named_nodes(&self) -> [(&'static str, usize); NNODE] {
        let mut out = [("", 0usize); NNODE];
        let mut i = 0;
        while i < NNODE {
            let n = Node::FROM_INDEX[i];
            let mut h = Hist::ZERO;
            count_node(&mut h, n);
            let mut slot = 0;
            while h.node[slot] == 0 {
                slot += 1;
            }
            out[i] = (node_name(n), self.node[slot]);
            i += 1;
        }
        out
    }

    /// The same for actions.
    pub fn named_acts(&self) -> [(&'static str, usize); NACT] {
        let mut out = [("", 0usize); NACT];
        let mut i = 0;
        while i < NACT {
            let a = ALL_ACTS[i];
            let mut h = Hist::ZERO;
            count_act(&mut h, a);
            let mut slot = 0;
            while h.act[slot] == 0 {
                slot += 1;
            }
            out[i] = (act_name(a), self.act[slot]);
            i += 1;
        }
        out
    }

    pub fn add(&mut self, o: &Hist) {
        for (a, b) in self.node.iter_mut().zip(o.node.iter()) {
            *a += *b;
        }
        for (a, b) in self.act.iter_mut().zip(o.act.iter()) {
            *a += *b;
        }
    }
}

/// The same for an action.
#[cfg(feature = "diag")]
pub(crate) fn count_act(h: &mut Hist, a: Act) {
    h.act[match a {
        Act::Text => 0,
        Act::Index => 1,
        Act::Return => 2,
        Act::Backspace => 3,
        Act::Tab => 4,
        Act::Char2 => 5,
        Act::Char3 => 6,
        Act::Char4 => 7,
        Act::Begin2 => 8,
        Act::Begin3 => 9,
        Act::Begin4 => 10,
        Act::Cont => 11,
        Act::ContEnd => 12,
        Act::Latin1 => 13,
        Act::Bad => 14,
        Act::Consume => 15,
        Act::CsiStart => 16,
        Act::EscStart => 26,
        Act::Digit => 17,
        Act::Marker => 18,
        Act::Inter => 19,
        Act::SubSep => 20,
        Act::Sep => 21,
        Act::CsiFinal => 22,
        Act::EscFinal => 23,
        Act::Again => 24,
        Act::Resync => 25,
    }] += 1;
}

/// One edge: what to do, and where that leaves the parser.
///
/// Two `repr(u8)` enums side by side -- the same two bytes as packing them
/// into a `u16`, without the shift, the mask and the casts back out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(C)]
pub struct Edge {
    pub act: Act,
    pub next: Node,
}

const fn edge(act: Act, next: Node) -> Edge {
    Edge { act, next }
}

/// An array with one entry per node, addressed by the node itself.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(crate) struct Nodes<T>([T; NNODE]);

impl<T> Nodes<T> {
    #[inline(always)]
    const fn at(&self, n: Node) -> &T {
        // The one cast in this file, and the one the language forces: an array
        // index is a `usize` and no newtype can be one.
        &self.0[n as usize]
    }
}

/// An array with one entry per byte value.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(crate) struct Bytes<T>([T; 256]);

impl<T> Bytes<T> {
    #[inline(always)]
    const fn at(&self, b: StreamByte) -> &T {
        &self.0[b.index()]
    }
}

/// The graph, built at compile time: `[node][byte] -> edge`.
///
/// 4 x 256 x 2 bytes = **2 KB of rodata**, no initialiser and nothing to load.
static UTF8: Nodes<Bytes<Edge>> = build(InputEncoding::Utf8);
static LATIN1: Nodes<Bytes<Edge>> = build(InputEncoding::Latin1);

impl Edge {
    /// The edge one byte takes out of one node. **This is the lookup.**
    #[inline(always)]
    pub(crate) fn of(enc: InputEncoding, from: Node, b: StreamByte) -> Edge {
        // The encoding selects a *table* rather than an index: a byte protocol
        // is a different graph, not a fourth dimension of this one.
        let g = match enc {
            InputEncoding::Utf8 => &UTF8,
            InputEncoding::Latin1 => &LATIN1,
        };
        *g.at(from).at(b)
    }
}

const fn build(enc: InputEncoding) -> Nodes<Bytes<Edge>> {
    let mut t = [Bytes([edge(Act::Bad, Node::Ground); 256]); NNODE];
    let mut n = 0usize;
    while n < NNODE {
        let node = Node::FROM_INDEX[n];
        let mut i = 0usize;
        while i < 256 {
            t[n].0[i] = one(enc, node, StreamByte::ALL[i]);
            i += 1;
        }
        n += 1;
    }
    Nodes(t)
}

/// Which operation a C0 byte performs. **Decided once, at build time, for all
/// thirty-two of them** -- the ones that do nothing share `Consume` with every
/// other byte this parser eats and ignores.
const fn c0(b: StreamByte) -> Act {
    match b.raw_byte() {
        0x0a | 0x0b | 0x0c => Act::Index,
        0x0d => Act::Return,
        0x08 => Act::Backspace,
        0x09 => Act::Tab,
        _ => Act::Consume,
    }
}

/// The whole protocol, as one function of three bounded values, run at build
/// time and never at run time.
const fn one(enc: InputEncoding, node: Node, b: StreamByte) -> Edge {
    // `[[source: williams]]`'s **anywhere** block: "CAN and SUB cancel any
    // escape sequence", from every state, with the same action. Here rather
    // than in each node function because six of them would have to remember
    // it and `escape`, `csi_*` and `string` all forgot -- `is_c0()` is
    // `b < 0x20`, which *includes* both, so every arm spelling
    // `if b.is_c0() { return edge(c0(b), <same node>) }` consumed the byte and
    // stayed exactly where it was.
    //
    // **A cancelled sequence that still performs its operation is the worse
    // half.** Inside a `CSI` the parser stayed in `CsiParam` and took the next
    // byte in `40-7E` as a final, so `ESC [ 1 ; 5 CAN I` dispatched `CHT` and
    // moved the cursor eight columns for a sequence that had been cancelled.
    // Inside an `OSC` the byte was body text and the string ate what followed.
    //
    // The ground-like states fall through and handle both bytes themselves,
    // which is core2's rule spelled the same way: in `Ground` there is nothing
    // to cancel and `c0` already answers `Consume`, and mid-character
    // `Act::Resync` makes the partial character a replacement and re-dispatches
    // the byte from `Ground` -- `[[source: unicode-ch3]]` 3.9.5 requires the
    // successor not be swallowed, and the outcome is the same `Consume`.
    let ground_like = matches!(
        node,
        Node::Ground | Node::Utf8One | Node::Utf8Two | Node::Utf8Three
    );
    if !ground_like && (b.is(StreamByte::CAN) || b.is(StreamByte::SUB)) {
        return edge(Act::Consume, Node::Ground);
    }

    // Mid-character: only a continuation can follow, and it moves one node
    // nearer the end.
    match node {
        Node::Utf8Three => {
            return match b.utf8_continuation() {
                Some(_) => edge(Act::Cont, Node::Utf8Two),
                None => edge(Act::Resync, Node::Ground),
            };
        }
        Node::Utf8Two => {
            return match b.utf8_continuation() {
                Some(_) => edge(Act::Cont, Node::Utf8One),
                None => edge(Act::Resync, Node::Ground),
            };
        }
        // The last one owed: the character is complete, so the edge says so
        // and the action writes it. **The table knows, so the dispatch does
        // not have to ask the node** -- which is what keeps the action a flat
        // match over a value rather than a match over a pair.
        Node::Utf8One => {
            return match b.utf8_continuation() {
                Some(_) => edge(Act::ContEnd, Node::Ground),
                None => edge(Act::Resync, Node::Ground),
            };
        }
        Node::Escape => return escape(b),
        Node::CsiEntry => return csi_entry(b),
        Node::CsiParam => return csi_param(b),
        Node::CsiInter => return csi_inter(b),
        Node::CsiIgnore => return csi_ignore(b),
        Node::Str => return string(b),
        // `[[source: williams]]`: `ESC \` is the *only* string terminator, and
        // a bare `ESC` cancels the string rather than continuing it. Code that
        // assumed the byte after `ESC` was the backslash swallowed a `NEL`.
        Node::StrEsc => {
            return match b.is(StreamByte::from_stream(0x5c)) {
                true => edge(Act::Consume, Node::Ground),
                false => edge(Act::Again, Node::Escape),
            };
        }
        Node::Ground => {}
    }

    if b.is(StreamByte::ESC) {
        return edge(Act::EscStart, Node::Escape);
    }

    // **`Route` is this row of the graph, and it is the same definition the
    // block scanner answers to.** Spelling the ranges again here would be a
    // second source of truth for "which route does this byte take", and the
    // run scanner would then be agreeing with a different function from the
    // one that dispatched to it.
    if matches!(Route::of(b), Route::Control) {
        return edge(c0(b), Node::Ground);
    }
    if matches!(Route::of(b), Route::Text) {
        return edge(Act::Text, Node::Ground);
    }
    if matches!(enc, InputEncoding::Latin1) {
        // ISO 8859-1: one byte, one character, no decoder state. **Not
        // `Text`**, which names a *run* of printable ASCII -- the run scanner
        // would refuse the first byte of it and the loop would not advance.
        //
        // **`80..9F` is wrong here and knowingly so.** Those are 8-bit C1
        // controls, which `CLAUDE.md` records as working under Latin-1 --
        // there is no multi-byte sequence for them to be part of outside
        // UTF-8, which is why the refusal was scoped to the encoding rather
        // than to the bytes. This row answers `Bad`, one U+FFFD, because the
        // new core parses no control sequences at all yet and a C1 that
        // introduced one would have nowhere to go. It is unreachable today --
        // nothing switches the encoding -- and it is a wrong row in a table
        // whose whole claim is to be the source of truth, so it is written
        // down rather than left to be discovered.
        return match b.is_latin1_graphic() {
            true => edge(Act::Latin1, Node::Ground),
            false => edge(Act::Bad, Node::Ground),
        };
    }
    // **The action carries the length, so the character is one lookup.** A
    // byte-at-a-time machine needs a node per continuation and three trips
    // round the loop for one three-byte character; here the edge for the lead
    // says `Char3` and `Char3` eats three. The `Begin` forms exist only for a
    // character the read cut in half, which is at most one per `read(2)`.
    match b.utf8_lead() {
        Some((1, _, _)) => edge(Act::Char2, Node::Ground),
        Some((2, _, _)) => edge(Act::Char3, Node::Ground),
        Some((3, _, _)) => edge(Act::Char4, Node::Ground),
        // A stray continuation, or a lead UTF-8 does not allow.
        _ => edge(Act::Bad, Node::Ground),
    }
}

/// Out of `ESC`: an introducer, an intermediate, or a two-byte final.
const fn escape(b: StreamByte) -> Edge {
    let v = b.raw_byte();
    if b.is_c0() {
        // A C0 inside a sequence executes where it stands and the sequence
        // carries on around it.
        return edge(c0(b), Node::Escape);
    }
    match v {
        0x5b => edge(Act::CsiStart, Node::CsiEntry),
        // `OSC`, `DCS`, `SOS`, `PM`, `APC` -- all bodies, all ended the same way.
        0x5d | 0x50 | 0x58 | 0x5e | 0x5f => edge(Act::Consume, Node::Str),
        // **Recorded, not consumed.** `ESC ( B` designates a character set
        // and `ESC B` does not exist; dropping the `(` made every charset
        // designation land on the same arm as its final byte alone.
        0x20..=0x2f => edge(Act::Inter, Node::Escape),
        0x30..=0x7e => edge(Act::EscFinal, Node::Ground),
        _ => edge(Act::Consume, Node::Ground),
    }
}

/// Inside `CSI`, before any intermediate.
/// The byte straight after `CSI`, where a marker is still legal.
const fn csi_entry(b: StreamByte) -> Edge {
    if b.is_c0() {
        return edge(c0(b), Node::CsiEntry);
    }
    match b.raw_byte() {
        0x30..=0x39 => edge(Act::Digit, Node::CsiParam),
        0x3a => edge(Act::SubSep, Node::CsiParam),
        0x3b => edge(Act::Sep, Node::CsiParam),
        0x3c..=0x3f => edge(Act::Marker, Node::CsiParam),
        0x20..=0x2f => edge(Act::Inter, Node::CsiInter),
        0x40..=0x7e => edge(Act::CsiFinal, Node::Ground),
        _ => edge(Act::Consume, Node::CsiIgnore),
    }
}

const fn csi_param(b: StreamByte) -> Edge {
    if b.is_c0() {
        return edge(c0(b), Node::CsiParam);
    }
    match b.raw_byte() {
        0x30..=0x39 => edge(Act::Digit, Node::CsiParam),
        0x3a => edge(Act::SubSep, Node::CsiParam),
        0x3b => edge(Act::Sep, Node::CsiParam),
        // A marker here is a grammar violation: `[[source: williams]]` sends
        // `3C-3F` in `csi_param` to `csi_ignore`.
        0x3c..=0x3f => edge(Act::Consume, Node::CsiIgnore),
        0x20..=0x2f => edge(Act::Inter, Node::CsiInter),
        0x40..=0x7e => edge(Act::CsiFinal, Node::Ground),
        _ => edge(Act::Consume, Node::CsiParam),
    }
}

/// Inside `CSI`, past the first intermediate: no more parameters may start.
const fn csi_inter(b: StreamByte) -> Edge {
    if b.is_c0() {
        return edge(c0(b), Node::CsiInter);
    }
    match b.raw_byte() {
        0x20..=0x2f => edge(Act::Inter, Node::CsiInter),
        0x40..=0x7e => edge(Act::CsiFinal, Node::Ground),
        _ => edge(Act::Consume, Node::CsiIgnore),
    }
}

/// A `CSI` that broke its grammar: swallow everything up to the final.
const fn csi_ignore(b: StreamByte) -> Edge {
    if b.is_c0() {
        return edge(c0(b), Node::CsiIgnore);
    }
    match b.raw_byte() {
        0x40..=0x7e => edge(Act::Consume, Node::Ground),
        _ => edge(Act::Consume, Node::CsiIgnore),
    }
}

/// The body of a control string. `BEL` ends an `OSC`; `ESC` may begin `ST`.
const fn string(b: StreamByte) -> Edge {
    match b.raw_byte() {
        0x07 => edge(Act::Consume, Node::Ground),
        0x1b => edge(Act::Consume, Node::StrEsc),
        _ => edge(Act::Consume, Node::Str),
    }
}

/// **The whole of ASCII as one 256-entry table.**
///
/// One index per byte, and the entry names the operation outright -- no
/// decode, no length, no state. This is what "O(1) for anything ASCII" means
/// concretely, and the table is 256 bytes.
#[cfg(feature = "ascii-only")]
pub(crate) fn ascii(b: StreamByte) -> Act {
    static ASCII: Bytes<Act> = build_ascii();
    *ASCII.at(b)
}

#[cfg(feature = "ascii-only")]
const fn build_ascii() -> Bytes<Act> {
    let mut t = [Act::Bad; 256];
    let mut i = 0usize;
    while i < 256 {
        let b = StreamByte::ALL[i];
        t[i] = match Route::of(b) {
            Route::Text => Act::Text,
            Route::Control => c0(b),
            _ => Act::Bad,
        };
        i += 1;
    }
    Bytes(t)
}

/// The `Begin` edge for a lead whose bytes the read does not all carry.
///
/// Chosen by the loop rather than by the table, because whether a character
/// fits is a property of the *read*, not of the byte.
#[inline(always)]
pub(crate) fn split(act: Act) -> Edge {
    match act {
        Act::Char2 => edge(Act::Begin2, Node::Utf8One),
        Act::Char3 => edge(Act::Begin3, Node::Utf8Two),
        _ => edge(Act::Begin4, Node::Utf8Three),
    }
}

impl Act {
    /// How many bytes this action eats, when that is fixed. `Text` scans and
    /// says for itself.
    #[inline(always)]
    pub(crate) const fn width(self) -> ByteLen {
        match self {
            Act::Char2 => ByteLen::TWO,
            Act::Char3 => ByteLen::THREE,
            Act::Char4 => ByteLen::FOUR,
            _ => ByteLen::ONE,
        }
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod classes {
    use super::*;
    use std::collections::HashMap;
    use std::vec::Vec;

    /// What a `k`-byte word does, seen from one starting node: the actions it
    /// performs and the node it leaves the parser in.
    ///
    /// **This is the alphabet a stride-`k` table is indexed by.** Two words are
    /// the same symbol when they agree here from *every* node, which is the
    /// standard alphabet reduction -- group the characters that label the same
    /// transitions everywhere.
    fn word_signature(enc: InputEncoding, from: Node, word: &[StreamByte]) -> (Vec<Act>, Node) {
        let mut acts = Vec::with_capacity(word.len());
        let mut n = from;
        for b in word {
            let e = Edge::of(enc, n, *b);
            acts.push(e.act);
            n = e.next;
        }
        // **The actions and where it ends, not the nodes it passed through.**
        // A stride-k table maps `(start, k bytes)` to what to do and where to
        // stand; the intermediate nodes are internal to the edge and no reader
        // can observe them. Including them makes the alphabet finer than it
        // needs to be -- 403 classes at stride 2 instead of 402 -- which
        // understates the reduction the table actually gets.
        (acts, n)
    }

    /// The signature of a word across every node and both encodings.
    ///
    /// **Typed, not cast to bytes.** A discriminant cast would be one more
    /// raw-integer site in a crate whose budget is a floor, and the action is
    /// already the value being compared.
    fn full_signature(word: &[StreamByte]) -> Vec<(Vec<Act>, Node)> {
        let mut sig = Vec::new();
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for n in Node::FROM_INDEX {
                sig.push(word_signature(enc, n, word));
            }
        }
        sig
    }

    /// **How wide a stride this graph can afford, measured rather than
    /// assumed.**
    ///
    /// The literature's construction for a stride-`k` DFA alternates stride
    /// doubling with alphabet reduction, because bytes that are distinct at
    /// stride 1 become equivalent as *pairs* at stride 2. Without the
    /// reduction a stride-2 table is `256^2` transitions a state and a stride-4
    /// table is `256^4`; with it the width is whatever this test reports.
    ///
    /// Printed and asserted: the numbers decide whether a wider parameter edge
    /// is 380 KB or unaffordable, and a number nothing checks drifts.
    #[test]
    fn the_alphabet_reduces_as_the_stride_doubles() {
        // Stride 1: every byte, by the edge it takes from every node.
        let mut one: HashMap<Vec<(Vec<Act>, Node)>, usize> = HashMap::new();
        for b in 0..256usize {
            let k = full_signature(&[StreamByte::ALL[b]]);
            let n = one.len();
            one.entry(k).or_insert(n);
        }

        // Stride 2: every byte *pair*, by what the two of them do.
        let mut two: HashMap<Vec<(Vec<Act>, Node)>, usize> = HashMap::new();
        for b0 in 0..256usize {
            for b1 in 0..256usize {
                let k = full_signature(&[StreamByte::ALL[b0], StreamByte::ALL[b1]]);
                let n = two.len();
                two.entry(k).or_insert(n);
            }
        }

        // Stride 3, for the edge that actually shipped.
        let mut three: HashMap<Vec<(Vec<Act>, Node)>, usize> = HashMap::new();
        for b0 in 0..256usize {
            for b1 in 0..256usize {
                for b2 in 0..256usize {
                    let k = full_signature(&[
                        StreamByte::ALL[b0],
                        StreamByte::ALL[b1],
                        StreamByte::ALL[b2],
                    ]);
                    let n = three.len();
                    three.entry(k).or_insert(n);
                }
            }
        }

        // Stride 4, composed from stride-2 representatives rather than
        // enumerated: 4.3 billion four-byte words is not walkable, but every
        // one of them is a pair of two-byte words, so `C2^2` compositions
        // cover the space exactly.
        let mut reps: Vec<[StreamByte; 2]> = Vec::new();
        {
            let mut seen: HashMap<Vec<(Vec<Act>, Node)>, ()> = HashMap::new();
            for b0 in 0..256usize {
                for b1 in 0..256usize {
                    let w = [StreamByte::ALL[b0], StreamByte::ALL[b1]];
                    if seen.insert(full_signature(&w), ()).is_none() {
                        reps.push(w);
                    }
                }
            }
        }
        let mut four: HashMap<Vec<(Vec<Act>, Node)>, usize> = HashMap::new();
        for a in &reps {
            for b in &reps {
                let w = [a[0], a[1], b[0], b[1]];
                let k = full_signature(&w);
                let n = four.len();
                four.entry(k).or_insert(n);
            }
        }
        std::eprintln!(
            "stride 4: {} classes of 4,294,967,296; table {} x {} = {} entries",
            four.len(),
            NNODE,
            four.len(),
            NNODE * four.len(),
        );

        std::eprintln!(
            "stride 1: {} classes of 256\nstride 2: {} classes of 65,536\nstride 3: {} classes of 16,777,216",
            one.len(),
            two.len(),
            three.len(),
        );
        std::eprintln!(
            "a stride-2 table is {} x {} = {} entries; stride-3 is {} x {} = {}",
            NNODE,
            two.len(),
            NNODE * two.len(),
            NNODE,
            three.len(),
            NNODE * three.len(),
        );
        // **The ladder has moved twice, and neither move was a regression.**
        // 22/402/6,441/96,812 -> 23/444/7,619/123,202 when `Act::Tab` landed,
        // because HT had shared a class with every other unrouted C0 while
        // they all fell to `Act::Consume`; then -> 24/477/8,352/137,338 when
        // `CAN`/`SUB` gained the `anywhere` rule, for the same reason one step
        // further out. Both times the pin caught it on the first run.
        //
        // **The second move cost less than the first and no new action.**
        // `Act::Tab` grew stride 4 by 27%; the cancel edge grows it 11.5% and
        // adds nothing to the dispatch, because "consume and return to Ground"
        // is `Act::Consume` with a different next node. A stride-k table is
        // `CLASSES^k`, so telling two bytes apart is never free -- but telling
        // them apart in the *table* is much cheaper than doing it in the
        // `match`, which is the whole argument for this representation.
        //
        // **Pinned, because these numbers size a table.** Each is the count
        // of distinct behaviours a k-byte word can have across every node and
        // both encodings -- the alphabet a stride-k transition table is
        // indexed by, once the characters that label the same transitions
        // everywhere are grouped.
        assert_eq!(one.len(), CLASSES, "stride 1");
        assert_eq!(two.len(), 477, "stride 2");
        assert_eq!(three.len(), 8_352, "stride 3");
        assert_eq!(four.len(), 137_338, "stride 4");
        // The *buildable* index is a k-tuple of stride-1 classes, because that
        // is what four independent byte lookups can produce: `CLASSES^k`
        // columns, larger than the true class count and reachable with a
        // dependency depth of two.
        std::eprintln!(
            "buildable columns: k=2 {} k=3 {} k=4 {}",
            CLASSES.pow(2),
            CLASSES.pow(3),
            CLASSES.pow(4),
        );
    }

    /// **How many byte values the graph can actually tell apart.**
    ///
    /// Two bytes are the same as far as this parser is concerned when they
    /// take the same edge out of *every* node under *both* encodings. That
    /// count is what decides whether a k-byte lookup can stay a perfect index:
    /// a window of `k` raw bytes is `256^k` slots and unaddressable past two,
    /// where a window of `k` classes is `C^k`.
    ///
    /// Asserted rather than printed, because a number nothing checks drifts --
    /// and this one bounds a table size.
    #[test]
    fn the_graph_tells_apart_far_fewer_than_256_bytes() {
        // **The whole edge, not just the action.** Two bytes are
        // interchangeable to a wider key only if they agree on where the
        // parser stands afterwards as well as on what to do -- and comparing
        // `Edge` values directly costs no cast, which the action's
        // discriminant would have.
        let mut sigs = [[edge(Act::Bad, Node::Ground); 2 * NNODE]; 256];
        for i in 0..256usize {
            let b = StreamByte::ALL[i];
            for (n, node) in Node::FROM_INDEX.iter().enumerate() {
                sigs[i][n] = Edge::of(InputEncoding::Utf8, *node, b);
                sigs[i][NNODE + n] = Edge::of(InputEncoding::Latin1, *node, b);
            }
        }
        let (mut count, mut seen) = (0usize, [false; 256]);
        for i in 0..256usize {
            if seen[i] {
                continue;
            }
            count += 1;
            for j in i..256usize {
                if sigs[i] == sigs[j] {
                    seen[j] = true;
                }
            }
        }
        assert_eq!(
            count, CLASSES,
            "byte equivalence classes changed; a k-stride table is CLASSES^k entries"
        );
    }

    /// **Every row of the table must be the row its node names**, and nothing
    /// checked that it was.
    ///
    /// `build` fills row `n` from `FROM_INDEX[n]` while `Nodes::at` addresses
    /// by the discriminant, so inserting a variant in one order in the enum
    /// and another in the table permutes the whole graph -- every byte then
    /// gets some other node's edge. It happened: adding `CsiEntry` before
    /// `Escape` in the enum and after it in the table made `ESC [ 1 ; 1 H`
    /// render as the text `1;1H`. The differential caught it only because a
    /// hand-written case addresses the cursor; a recording that never moves it
    /// would have passed, which is this repo's "payloads that don't exercise
    /// the case" rule wearing a correctness costume.
    ///
    /// Checked against `one` -- the function the table is built from -- so it
    /// fails on a permutation rather than on a value.
    #[test]
    fn every_row_of_the_graph_is_the_row_its_node_names() {
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for node in Node::FROM_INDEX {
                for i in 0..256usize {
                    let b = StreamByte::ALL[i];
                    assert_eq!(
                        Edge::of(enc, node, b),
                        one(enc, node, b),
                        "{node:?} byte {i:#04x}"
                    );
                }
            }
        }
    }

    /// **Every node that must cancel does, checked against the shipped table
    /// rather than against the function that built it.**
    ///
    /// The rule lives in one place in [`one`] and therefore cannot be
    /// half-applied -- but nothing enforced that it stays in one place, and
    /// the shape it had before was six node functions each remembering it,
    /// which all six got wrong. The two differential fixtures drive `CsiParam`,
    /// `Escape` and `Str`; **`CsiEntry`, `CsiInter`, `CsiIgnore` and `StrEsc`
    /// are reached by no payload in the repository**, so a refactor that
    /// pushed the test back into the node functions would leave four nodes
    /// broken and every test green.
    ///
    /// This is exhaustive over both encodings and all eleven nodes, so the
    /// claim is a gate rather than a comment.
    #[test]
    fn can_and_sub_cancel_from_every_node_that_is_not_ground_like() {
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for node in Node::FROM_INDEX {
                let ground_like = matches!(
                    node,
                    Node::Ground | Node::Utf8One | Node::Utf8Two | Node::Utf8Three
                );
                for b in [StreamByte::CAN, StreamByte::SUB] {
                    let got = Edge::of(enc, node, b);
                    match ground_like {
                        // Nothing to cancel: `Ground` consumes it as an
                        // unrouted C0, and mid-character it is a byte that
                        // cannot continue the character, so the partial one
                        // becomes a replacement and the byte is re-dispatched
                        // from `Ground` without being swallowed.
                        true => assert_eq!(
                            got,
                            match node {
                                Node::Ground => edge(Act::Consume, Node::Ground),
                                _ => edge(Act::Resync, Node::Ground),
                            },
                            "{node:?} {enc:?} byte {:#04x}",
                            b.raw_byte()
                        ),
                        false => assert_eq!(
                            got,
                            edge(Act::Consume, Node::Ground),
                            "{node:?} {enc:?} byte {:#04x} does not cancel",
                            b.raw_byte()
                        ),
                    }
                }
            }
        }
    }

    /// The measured count, pinned. `CLASSES^4` is what a four-byte window
    /// costs as a perfect index.
    ///
    /// **Twenty-four**, comparing whole edges. A four-byte window is
    /// `24^4 = 331,776` entries and a three-byte window `13,824` -- both
    /// perfect indexes with no hash, where the same windows over raw bytes are
    /// 4.3 billion and 16.7 million. Comparing actions alone gives fewer,
    /// which is the wrong equivalence: two bytes are interchangeable only if
    /// they also leave the parser in the same node.
    ///
    /// **It has only ever gone up, and each step is a byte the grammar started
    /// telling apart**: 21 before `:` was distinguished from `;` and the
    /// private markers and intermediates stopped being consumed, 22 after, 23
    /// when HT was routed to the tab machinery, 24 when `CAN` and `SUB` gained
    /// the `anywhere` cancel. A number that moves this way is the table
    /// growing to fit the protocol, not a regression -- and the assertion is
    /// here so that growth is a decision rather than a discovery.
    const CLASSES: usize = 24;
}
