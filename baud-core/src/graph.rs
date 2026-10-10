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
    /// `ESC P`: a DCS, before its prefix is known.
    Dcs,
    /// `ESC P $`: one byte from being a DECRQSS.
    DcsDollar,
    /// The body of an `OSC`: `[[source: williams]]`'s `osc string`, which a
    /// `BEL` ends as well as `ST`, because `[[source: ctlseqs]]` says so
    /// (2023-2024) where williams ignores it.
    ///
    /// **Three body nodes, for williams' `osc string`, `dcs passthrough` and
    /// `sos/pm/apc string`, and the only byte their rows differ in is
    /// `BEL`.** They were one node, and so a `BEL` ended all five strings,
    /// where every text that speaks of it ends only the `OSC`:
    /// `[[source: ecma48]]` 5.6 closes a control string with `ST` alone, and
    /// williams puts the byte in a DCS and ignores it in the other three.
    OscStr,
    /// The body of a `DCS`, past its prefix: williams' `dcs passthrough`,
    /// whose row puts a `BEL` (388), so it is payload and the string goes on.
    ///
    /// **It is williams' `dcs ignore` as well**, which this graph has no node
    /// for: a prefix williams sends there, `ESC P 1 ?` or `ESC P $ 1`, leads
    /// here too, and its body is put -- `20-7F` before this node split,
    /// `BEL` as well now -- then discarded as `StrKind::Other`, where
    /// williams ignores every byte of it. Only `string_truncations` can see
    /// the difference, on such a string longer than the accumulator.
    DcsStr,
    /// The body of an `SOS`, `PM` or `APC`: williams' `sos/pm/apc string`,
    /// whose row ignores a `BEL` (426), so it goes nowhere and the string
    /// goes on.
    SosStr,
    /// An `ESC` inside a control string: `\` ends it, anything else cancels.
    StrEsc,
    /// Between characters in VT52. **A protocol is more nodes, not a runtime
    /// flag** -- the rule this graph already applied to `ESC` and to the UTF-8
    /// continuations, and the reason VT52 is a disjoint sub-graph rather than
    /// a mode test inside the `Escape` arm. In VT52 `ESC [` is not a CSI
    /// introducer: it is a command VT52 does not define. A flag would have to
    /// be read on the path every byte takes; a node costs one row of a table.
    Vt52Ground,
    /// After `ESC` in VT52. One uniform row: the byte goes to `Vt52::parse` at
    /// run time rather than being split into an action per command, because a
    /// stride-k table is `CLASSES^k` and fifteen new byte classes would grow
    /// stride 4 far past what two ever did.
    Vt52Escape,
    /// After `ESC Y`, owed the row byte of a direct address.
    Vt52Row,
    /// After `ESC Y <row>`, owed the column byte.
    Vt52Col,
    /// A character split across reads **in VT52**, with this many continuations
    /// owed. Separate from the ANSI three because completing a character has to
    /// return to the ground it started in -- otherwise a multi-byte character
    /// straddling a `read(2)` silently leaves VT52, which is a chunk-invariance
    /// failure rather than a rendering one.
    Vt52Utf8One,
    Vt52Utf8Two,
    Vt52Utf8Three,
}

pub(crate) const NNODE: usize = 22;

/// How many actions there are. Only `diag` reads it, to size the histogram
/// that says which of them a wider key would collapse.
#[cfg(feature = "diag")]
pub(crate) const NACT: usize = 38;

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
        Node::Dcs,
        Node::DcsDollar,
        Node::OscStr,
        Node::DcsStr,
        Node::SosStr,
        Node::StrEsc,
        Node::Vt52Ground,
        Node::Vt52Escape,
        Node::Vt52Row,
        Node::Vt52Col,
        Node::Vt52Utf8One,
        Node::Vt52Utf8Two,
        Node::Vt52Utf8Three,
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
    /// `SO` and `SI`. **Locking shifts: which G slot `GL` invokes.**
    ///
    /// `SO` (`0x0E`) invokes G1 and `SI` (`0x0F`) invokes G0, and they are the
    /// other half of `ESC ( 0`: designation loads a slot, invocation decides
    /// which slot the bytes are read through. `baud-core` had one `charset`
    /// field, so designating into G1 changed what G0 rendered and neither
    /// shift did anything -- the designation *was* the invocation.
    ///
    /// Two actions and not one carrying the byte: an arm that re-tested which
    /// byte it had is the shape this file measured at +9% to remove, three
    /// times in one change.
    ShiftOut,
    ShiftIn,
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
    /// `ESC` that opened a control string.
    Consume,
    /// The introducer of a control string: `OSC`, `DCS`, `SOS`, `PM`, `APC`.
    ///
    /// Reads the byte, so one action serves all five, and `SOS`, `PM` and
    /// `APC` share a body node. **It resets the accumulator**, which is the
    /// whole reason it is distinct from `Consume`: a string that a `CAN`
    /// cancels never reaches a terminator, so resetting where the string
    /// starts is what makes the cancelled case right without a second rule
    /// for it.
    StrStart,
    /// One byte of a control string's body.
    StrPut,
    /// `BEL`. Counted, because the core cannot make a sound and an embedder
    /// can -- a `BEL` consumed silently is a protocol event the application
    /// has no way to learn was received.
    ///
    /// Distinct from the `BEL` inside a control string, which rings nothing:
    /// `StrEnd` in an `OSC`, which it terminates, `StrPut` in a `DCS` body
    /// and `Consume` in the `DCS` prefix and in an `SOS`, `PM` or `APC`. The
    /// node is what tells them apart.
    Bell,
    /// A VT52 command byte, handed to `Vt52::parse` at run time.
    ///
    /// **One action for fifteen commands**, because a stride-k table is
    /// `CLASSES^k`: an action per command would split fifteen byte values into
    /// their own classes, and this file's own ladder prices two new actions at
    /// +54% of stride 4.
    Vt52Dispatch,
    /// The row byte of `ESC Y`, biased by `0x20`.
    Vt52SetRow,
    /// The column byte of `ESC Y`, which completes the address.
    Vt52Address,
    /// The `q` of a `DCS $ q`: this control string is a DECRQSS.
    ///
    /// One action, because the body and the accumulator are already `StrPut`
    /// and `StrEnd` -- a DCS payload and an OSC payload are the same bytes
    /// with a different prefix, and the one byte their bodies treat apart,
    /// `BEL`, is told apart by the body node, `Node::DcsStr`.
    DcsQuery,
    /// The `q` of a `DCS Pa;Pb;Ph q`: this control string is a Sixel image,
    /// reached from `Node::Dcs` **without** the `$` intermediate that marks a
    /// DECRQSS. Like `DcsQuery`, it renames the string kind so the body the
    /// existing `StrPut`/`StrEnd` machinery accumulates is decoded as Sixel
    /// raster data rather than dropped.
    SixelStart,
    /// The `\` of an `ST`, its one-byte form `9C` under ISO 8859-1, or a `BEL`
    /// in an `OSC`: the string is complete, act on it.
    ///
    /// Distinct from the `Consume` that the `anywhere` rule uses for `CAN` and
    /// `SUB`, and that is the difference that stops a cancelled title being
    /// set: both return to `Ground`, and only one dispatches.
    StrEnd,
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
        Node::Dcs => 9,
        Node::DcsDollar => 10,
        Node::OscStr => 11,
        Node::DcsStr => 12,
        Node::SosStr => 13,
        Node::StrEsc => 14,
        Node::Vt52Ground => 15,
        Node::Vt52Escape => 16,
        Node::Vt52Row => 17,
        Node::Vt52Col => 18,
        Node::Vt52Utf8One => 19,
        Node::Vt52Utf8Two => 20,
        Node::Vt52Utf8Three => 21,
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
        Node::Dcs => "Dcs",
        Node::DcsDollar => "DcsDollar",
        Node::OscStr => "OscStr",
        Node::DcsStr => "DcsStr",
        Node::SosStr => "SosStr",
        Node::StrEsc => "StrEsc",
        Node::Vt52Ground => "Vt52Ground",
        Node::Vt52Escape => "Vt52Escape",
        Node::Vt52Row => "Vt52Row",
        Node::Vt52Col => "Vt52Col",
        Node::Vt52Utf8One => "Vt52Utf8One",
        Node::Vt52Utf8Two => "Vt52Utf8Two",
        Node::Vt52Utf8Three => "Vt52Utf8Three",
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
        Act::ShiftOut => "ShiftOut",
        Act::ShiftIn => "ShiftIn",
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
        Act::StrStart => "StrStart",
        Act::StrPut => "StrPut",
        Act::Bell => "Bell",
        Act::Vt52Dispatch => "Vt52Dispatch",
        Act::Vt52SetRow => "Vt52SetRow",
        Act::Vt52Address => "Vt52Address",
        Act::DcsQuery => "DcsQuery",
        Act::SixelStart => "SixelStart",
        Act::StrEnd => "StrEnd",
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
    Act::ShiftOut,
    Act::ShiftIn,
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
    Act::StrStart,
    Act::StrPut,
    Act::Bell,
    Act::Vt52Dispatch,
    Act::Vt52SetRow,
    Act::Vt52Address,
    Act::DcsQuery,
    Act::SixelStart,
    Act::StrEnd,
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
        Act::ShiftOut => 5,
        Act::ShiftIn => 6,
        Act::Char2 => 7,
        Act::Char3 => 8,
        Act::Char4 => 9,
        Act::Begin2 => 10,
        Act::Begin3 => 11,
        Act::Begin4 => 12,
        Act::Cont => 13,
        Act::ContEnd => 14,
        Act::Latin1 => 15,
        Act::Bad => 16,
        Act::Consume => 17,
        Act::CsiStart => 18,
        Act::Digit => 19,
        Act::Marker => 20,
        Act::Inter => 21,
        Act::SubSep => 22,
        Act::Sep => 23,
        Act::CsiFinal => 24,
        Act::EscFinal => 25,
        Act::Again => 26,
        Act::Resync => 27,
        Act::EscStart => 28,
        // **Eight arms the protocol work added while `diag` did not compile.**
        // The match is exhaustive on purpose, so a new `Act` cannot be added
        // without landing here -- and that gate is only a gate while something
        // builds the feature. It did not, so the drift accumulated silently and
        // was found by reaching for the instrument rather than by a red test.
        Act::StrStart => 29,
        Act::StrPut => 30,
        Act::StrEnd => 31,
        Act::Bell => 32,
        Act::Vt52Dispatch => 33,
        Act::Vt52SetRow => 34,
        Act::Vt52Address => 35,
        Act::DcsQuery => 36,
        Act::SixelStart => 37,
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
/// 22 x 256 x 2 bytes = **11 KB of rodata per encoding, 22 KB for both**, no
/// initialiser and nothing to load. (Was 2 KB when there were 4 nodes, and 10
/// KB at 20, before the string body split three ways on `BEL`; a row is read
/// only while the parser stands in its node.)
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
        0x0a..=0x0c => Act::Index,
        0x0d => Act::Return,
        0x08 => Act::Backspace,
        0x09 => Act::Tab,
        0x0e => Act::ShiftOut,
        0x0f => Act::ShiftIn,
        0x07 => Act::Bell,
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
    // which was core2's rule too, spelled the same way: in `Ground` there is
    // nothing to cancel and `c0` already answers `Consume`, and mid-character
    // `Act::Resync` makes the partial character a replacement and re-dispatches
    // the byte from `Ground` -- `[[source: unicode-ch3]]` 3.9.5 requires the
    // successor not be swallowed, and the outcome is the same `Consume`.
    let ground_like = matches!(
        node,
        Node::Ground
            | Node::Utf8One
            | Node::Utf8Two
            | Node::Utf8Three
            | Node::Vt52Ground
            | Node::Vt52Utf8One
            | Node::Vt52Utf8Two
            | Node::Vt52Utf8Three
    );
    // **Back to the ground it was in, not to ANSI ground.** A `CAN` inside a
    // VT52 escape cancels the escape; it does not switch protocols, and
    // `ground_of` is what says so in one place rather than at each return.
    if !ground_like && (b.is(StreamByte::CAN) || b.is(StreamByte::SUB)) {
        return edge(Act::Consume, ground_of(node));
    }
    // **The same block's `1B`: an `ESC` abandons the sequence in progress
    // and starts a new one**, whose entry *clears* -- williams' reason is
    // "so that erroneous sequences like CSI 3 ; 1 CSI 2 J are handled
    // correctly". The five nodes below sent it to `c0()`, which consumed it
    // and stayed put, so `CSI 1 ESC [ 2 m` dispatched `CSI 1 [` and printed
    // `2m`, and `ESC ( ESC 0` kept the `(` and designated.
    //
    // **`Act::EscStart`, not a `Consume` into `Escape`**: it resets `csi`,
    // intermediates included, so a `CSI`'s `SP` cannot become the escape
    // sequence's intermediate. A list and not `!ground_like`, because the
    // other nodes keep their own `ESC` -- in a control string it is the first
    // half of `ST`, and VT52 has no `CSI`
    // (`an_esc_abandons_every_sequence_and_keeps_its_meaning_elsewhere`).
    let in_sequence = matches!(
        node,
        Node::Escape | Node::CsiEntry | Node::CsiParam | Node::CsiInter | Node::CsiIgnore
    );
    if in_sequence && b.is(StreamByte::ESC) {
        return edge(Act::EscStart, Node::Escape);
    }
    // **The same block's C1 controls, from a control string, under ISO
    // 8859-1.** `[[source: williams]]` (612-615): "All C1 controls cancel any
    // escape sequence, control sequence or control string in progress and are
    // executed. Control functions special to this parser, i.e. DCS, SOS, CSI,
    // OSC, PM and APC, cause a transition to their appropriate states." So a
    // C1 inside a string takes the edge it takes from ground -- `85` is NEL,
    // `9B` opens a `CSI`, `9D` a fresh `OSC` -- and `9C` is the `ST` that ends
    // it. The string node ended only on `9C` and stored the other thirty-one,
    // so `85` landed in a window title and the NEL never ran; the DCS prefix
    // nodes sent all thirty-two, `9C` included, into the body, so `90 9C X`
    // swallowed the `X`.
    //
    // **Cancelled, as a bare `ESC` cancels**: williams uses the one verb for
    // C1, `CAN`, `SUB` and `ESC`, and of a control sequence (534-537) gives
    // the reason, that "the 7-bit (ESC Fe) and 8-bit representations of C1
    // controls should act in the same way". `ESC E` inside an `OSC` cancels
    // it, so `85` must. xterm's `sos_table` agrees, taking `84` to `CASE_IND`
    // and every other C1 to its case or to `CASE_GROUND_STATE`
    // (`[[source: xterm-vtprstbl-c]]` 7781-7820). Its `CASE_OSC` keeps the
    // old string's bytes, where williams' `osc string` begins anew on entry;
    // the text is followed. A cancelled string is not dispatched here, by any
    // of the four; whether it should keep what it has done, as williams'
    // `unhook` and `osc_end` suggest, is open (`docs/record.md`).
    //
    // Under UTF-8 `latin1_c1` answers `None` for every byte, so these nodes'
    // rows there are what they were: `80-9F` are continuation bytes of the
    // payload's characters (`a_latin1_c1_leaves_every_control_string_and_a_utf8_one_stays`).
    let in_string = matches!(
        node,
        Node::Dcs | Node::DcsDollar | Node::OscStr | Node::DcsStr | Node::SosStr
    );
    if in_string {
        if let Some(e) = latin1_c1(enc, b) {
            return match b.raw_byte() {
                0x9c => edge(Act::StrEnd, Node::Ground),
                _ => e,
            };
        }
    }
    // **And from an escape or control sequence**, the rule's other half: the
    // same five nodes the `1B` edge above cancels from, `Escape` standing for
    // williams' `escape` and `escape intermediate` both, whose intermediates
    // `csi` holds. Of `csi entry` and the states after it williams says it
    // again (534-537): "C1 controls will cancel the sequence and then be
    // executed", so that the 8-bit form acts as the 7-bit one, whose `ESC`
    // cancels it. `escape()` sent every C1 to `Consume` and `Ground`, so
    // `ESC 85` ended the escape and lost the NEL, and the `csi_*` rows
    // consumed it, `csi param` staying put, so `CSI 2 ; 85 H` moved the
    // cursor. `9C` takes its ground edge, not `StrEnd`: there is no string to
    // end, and the diagram draws it to ground (268), where `esc_final` takes
    // the bare ST it maps to as no function and counts nothing. The actions
    // need nothing new: `EscFinal` maps a C1 through `c1_as_escape` and clears
    // `csi`, which drops the sequence's parameters and intermediates, and
    // `CsiStart` and `StrStart` clear it on entry, as williams' `clear` does.
    //
    // So every ANSI node a Latin-1 C1 can arrive in now acts on it: `Ground`,
    // the five string nodes, these five, and `StrEsc`, whose `Again` hands
    // the byte to `Escape`. The UTF-8 continuation nodes cannot be reached
    // under Latin-1, whose every character is one byte wide, and VT52 is note
    // AE's D3, waiting on the DEC text for that mode
    // (`a_latin1_c1_cancels_every_escape_and_control_sequence_and_a_utf8_one_does_not`).
    if in_sequence {
        if let Some(e) = latin1_c1(enc, b) {
            return e;
        }
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
        // **The same three rows in VT52, and they exist because of `read(2)`.**
        // A character cut between two reads completes on the next one and has
        // to complete into the ground it started in -- otherwise a multi-byte
        // character straddling a read boundary silently leaves VT52, so the
        // same bytes render differently depending on where the reads fell.
        Node::Vt52Utf8Three => {
            return match b.utf8_continuation() {
                Some(_) => edge(Act::Cont, Node::Vt52Utf8Two),
                None => edge(Act::Resync, Node::Vt52Ground),
            };
        }
        Node::Vt52Utf8Two => {
            return match b.utf8_continuation() {
                Some(_) => edge(Act::Cont, Node::Vt52Utf8One),
                None => edge(Act::Resync, Node::Vt52Ground),
            };
        }
        Node::Vt52Utf8One => {
            return match b.utf8_continuation() {
                Some(_) => edge(Act::ContEnd, Node::Vt52Ground),
                None => edge(Act::Resync, Node::Vt52Ground),
            };
        }
        Node::Escape => return escape(b),
        Node::CsiEntry => return csi_entry(b),
        Node::CsiParam => return csi_param(b),
        Node::CsiInter => return csi_inter(b),
        Node::CsiIgnore => return csi_ignore(b),
        // **Both rows are `Act::Consume` with a different next node**, which
        // is the cheapest kind of edge this graph has: it costs table content
        // and nothing in the dispatch, the way `CAN`/`SUB` did. Anything that
        // is not the DECRQSS prefix falls to `DcsStr`, where the body is still
        // consumed to its terminator and then ignored -- consumed is the half
        // that matters, because a DCS body reaching the screen would render an
        // application's private protocol as glyphs.
        // **A C0 or `DEL` in the prefix is ignored, and the prefix survives
        // it.** Both of these sent anything unrecognised to the body, so a
        // `DEL` between `ESC P` and the `$` turned a `DECRQSS` into a generic
        // control string -- the request was dropped and the application got no
        // reply, which for a *query* is the worst of the three outcomes.
        //
        // `[[source: williams]]`'s `dcs_entry`, `dcs_param` and
        // `dcs_intermediate` all give `00-17`, `19`, `1C-1F` and `7F` the
        // *ignore* action (354, 361, 368): the byte goes nowhere and the state
        // does not move. `18` and `1A` are absent because the `anywhere` rule
        // takes them first -- they cancel.
        //
        // **`BEL` is one of them.** Both rows ended the string on it, an
        // `OSC`'s terminator (`[[source: ctlseqs]]` 2023-2024) where no text
        // puts one, so `ESC P BEL $ q m ST` lost its reply and drew `$qm`.
        Node::Dcs => {
            return match b.raw_byte() {
                b'$' => edge(Act::Consume, Node::DcsDollar),
                // A bare `q` (no `$` before it) is a Sixel: rename the string
                // kind and route the body through the `StrPut`/`StrEnd` path,
                // which now decodes it rather than dropping it.
                b'q' => edge(Act::SixelStart, Node::DcsStr),
                // **A Sixel is `DCS Pa;Pb;Ph q` -- a `q` after *numeric
                // parameters* and no intermediate.** So the parameters `30-3F`
                // (`0-9 : ; < = > ?`) collect and stay in `Node::Dcs`, and a
                // `q` reached from here is a Sixel. The earlier catch-all sent
                // the first digit to the passthrough body, so `q` never arrived
                // and the image was never recognised.
                //
                // **Only the numeric parameters `30-3B` (`0-9 : ;`) collect,
                // not the private markers `3C-3F` (`< = > ?`) or the
                // intermediates `20-2F`.** A Sixel's parameters are
                // `Pa;Pb;Ph` -- digits and `;` -- so that is all it needs; and
                // routing the rest to `Node::DcsStr` keeps two things right that
                // williams' separate `dcs_param`/`dcs_intermediate`/`dcs_ignore`
                // states enforce: `DCS + q` (XTGETTCAP, an intermediate then
                // `q`) is not mistaken for a Sixel, and `DCS 1 ? $ q` (a marker
                // in a non-first position, which "cause[s] the entire control
                // sequence to be ignored") produces no DECRQSS reply. `$` is
                // the intermediate `DcsDollar` catches for DECRQSS, above.
                // The numeric parameters feed `csi` -- the same buffer a CSI
                // uses, fresh here because `Act::StrStart` reset it at the
                // `ESC P` entry -- so `sixel_end` can read `Pb`/`P2` (the
                // background-transparency selector) and count a request it does
                // not honour, rather than dropping the parameters unseen. A `q`
                // is still a Sixel and `$` still a DECRQSS: neither is a digit
                // or `;`, so both leave this edge untouched.
                0x30..=0x39 => edge(Act::Digit, Node::Dcs),
                0x3a..=0x3b => edge(Act::Sep, Node::Dcs),
                0x1b => edge(Act::Consume, Node::StrEsc),
                0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f => edge(Act::Consume, Node::Dcs),
                _ => edge(Act::Consume, Node::DcsStr),
            };
        }
        Node::DcsDollar => {
            return match b.raw_byte() {
                b'q' => edge(Act::DcsQuery, Node::DcsStr),
                0x1b => edge(Act::Consume, Node::StrEsc),
                0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f => edge(Act::Consume, Node::DcsDollar),
                _ => edge(Act::Consume, Node::DcsStr),
            };
        }
        Node::Vt52Escape | Node::Vt52Row | Node::Vt52Col => return vt52(node, b),
        Node::OscStr | Node::DcsStr | Node::SosStr => return string(node, b),
        // `[[source: williams]]`: `ESC \` is the *only* string terminator, and
        // a bare `ESC` cancels the string rather than continuing it. Code that
        // assumed the byte after `ESC` was the backslash swallowed a `NEL`.
        Node::StrEsc => {
            return match b.is(StreamByte::from_stream(0x5c)) {
                true => edge(Act::StrEnd, Node::Ground),
                false => edge(Act::Again, Node::Escape),
            };
        }
        Node::Ground | Node::Vt52Ground => {}
    }

    // **The ground the byte came from decides everything below.** One local,
    // resolved once per table entry at build time, in place of the literal
    // `Node::Ground` this body used to name in eight places.
    let g = ground_of(node);
    if b.is(StreamByte::ESC) {
        // **Not `Act::EscStart`.** That arm resets the parameter list and is
        // the entry to the accelerated CSI path, and VT52 has no CSI -- `ESC [`
        // there is a command VT52 does not define, not an introducer. Entering
        // `fast_csi` from VT52 would parse a sequence that does not exist in
        // this protocol.
        return match g {
            Node::Vt52Ground => edge(Act::Consume, Node::Vt52Escape),
            _ => edge(Act::EscStart, Node::Escape),
        };
    }

    // **`Route` is this row of the graph, and it is the same definition the
    // block scanner answers to.** Spelling the ranges again here would be a
    // second source of truth for "which route does this byte take", and the
    // run scanner would then be agreeing with a different function from the
    // one that dispatched to it.
    if matches!(Route::of(b), Route::Control) {
        return edge(c0(b), g);
    }
    if matches!(Route::of(b), Route::Text) {
        return edge(Act::Text, g);
    }
    // **DEL is ignored, in every ground.** `[[source: dec-std-070]]` 070-3
    // §3.4.2 says it "shall be ignored" with a 94-character set in GL, as do
    // `[[source: vt510-ch7]]` § 7.2.5 and `[[source: williams]]`; it fell to
    // `Act::Bad` below and drew U+FFFD, a decoding error for a byte that is
    // well-formed in UTF-8 (`del_in_ground_is_ignored`). VT52 ground has only
    // 94-character sets and takes the same rule. A 96-character set in GL
    // makes 0x7F a character of the set (ECMA-35 8.3.1), which waits on note
    // AE's 96-set work.
    if matches!(Route::of(b), Route::Del) {
        return edge(Act::Consume, g);
    }
    if matches!(enc, InputEncoding::Latin1) {
        // ISO 8859-1: one byte, one character, no decoder state. **Not
        // `Text`**, which names a *run* of printable ASCII -- the run scanner
        // would refuse the first byte of it and the loop would not advance.
        //
        // `80..9F` are the 8-bit C1 controls, the same edge a control string
        // gives them above (`latin1_c1`).
        //
        // This answered `Act::Bad` -- one `U+FFFD` -- under a comment saying
        // it was "wrong here and knowingly so ... because the new core parses
        // no control sequences at all yet". That reason expired the day the VT
        // sub-graph landed, and the row outlived it because **nothing could
        // reach this table**: no recording is Latin-1 and no sequence switched
        // the encoding, so the wrong answer cost nothing and announced
        // nothing. A row that cannot be reached is a row nobody re-reads.
        if let Some(e) = latin1_c1(enc, b) {
            return e;
        }
        return match b.is_latin1_graphic() {
            true => edge(Act::Latin1, g),
            false => edge(Act::Bad, g),
        };
    }
    // **The action carries the length, so the character is one lookup.** A
    // byte-at-a-time machine needs a node per continuation and three trips
    // round the loop for one three-byte character; here the edge for the lead
    // says `Char3` and `Char3` eats three. The `Begin` forms exist only for a
    // character the read cut in half, which is at most one per `read(2)`.
    match b.utf8_lead() {
        Some((1, _, _)) => edge(Act::Char2, g),
        Some((2, _, _)) => edge(Act::Char3, g),
        Some((3, _, _)) => edge(Act::Char4, g),
        // A stray continuation, or a lead UTF-8 does not allow.
        _ => edge(Act::Bad, g),
    }
}

/// A C1 control under ISO 8859-1: the `Escape` row read at `b - 0x40`, and
/// `None` for every other byte and for all of them under UTF-8.
///
/// **`80..9F` are the 8-bit C1 controls, and each is `ESC` plus the same byte
/// less `0x40`.** `9B` is `CSI`, `9D` is `OSC`, `90` is `DCS`, `9C` is `ST`,
/// `84` is `IND` -- so the row is not a special case at all, it is the
/// `Escape` row read at a different offset, and delegating is what keeps one
/// definition of what each final means. Ground, the control strings and the
/// escape and control sequences all ask here, so they cannot disagree on
/// which byte is which control.
///
/// They are controls *under ISO 8859-1 and not under UTF-8*, which is not an
/// inconsistency: in UTF-8 `9B` is a continuation byte and cannot begin
/// anything, and in ISO 8859-1 there is no multi-byte sequence for it to be
/// part of.
const fn latin1_c1(enc: InputEncoding, b: StreamByte) -> Option<Edge> {
    match enc {
        InputEncoding::Utf8 => None,
        InputEncoding::Latin1 => match b.raw_byte() {
            v @ 0x80..=0x9f => Some(escape(StreamByte::from_stream(v - 0x40))),
            _ => None,
        },
    }
}

/// Out of `ESC`: an introducer, an intermediate, or a two-byte final.
const fn escape(b: StreamByte) -> Edge {
    let v = b.raw_byte();
    if b.is_c0() {
        // A C0 inside a sequence executes where it stands and the sequence
        // carries on around it -- every C0 but `ESC`, `CAN` and `SUB`, which
        // `one()` takes first because they end the sequence.
        return edge(c0(b), Node::Escape);
    }
    match v {
        0x5b => edge(Act::CsiStart, Node::CsiEntry),
        // `OSC`, `DCS`, `SOS`, `PM`, `APC` -- all bodies, all ended by `ST`,
        // and **which one it was has to be recorded here** because `SOS`, `PM`
        // and `APC` share a body node and `str_end` dispatches on the kind.
        // `Act::StrStart` reads the introducer byte and resets the
        // accumulator; a string that is cancelled never reaches a terminator,
        // so resetting at the *start* is what makes both the completed and the
        // cancelled case right.
        //
        // **`OSC` has a body node of its own because a `BEL` ends it**, which
        // `[[source: ctlseqs]]` allows "for terminating OSC sequences"
        // (2023-2024) and for no other string; `[[source: williams]]`'s
        // `sos/pm/apc string` ignores the byte (426). One shared node ended all
        // four on it.
        0x5d => edge(Act::StrStart, Node::OscStr),
        0x58 | 0x5e | 0x5f => edge(Act::StrStart, Node::SosStr),
        // `DCS` alone, because it has a **prefix**: `[[source: ctlseqs]]`
        // gives DECRQSS as `DCS $ q Pt ST`, so two bytes stand between the
        // introducer and the payload. `OSC` has none -- its parameters are in
        // the payload -- and neither have `SOS`, `PM` and `APC`.
        0x50 => edge(Act::StrStart, Node::Dcs),
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

/// Which ground a node returns to when its sequence or character completes.
///
/// **This is the whole of what makes VT52 a sub-graph rather than a mode.**
/// Every place that used to name `Node::Ground` as a next node now names this,
/// so a character, a C0 or a cancelled sequence returns to the protocol it was
/// in. The alternative is a `terminal_mode` test on the path every byte takes:
/// a runtime flag in the hot loop, which the sub-graph replaces.
pub(crate) const fn ground_of(node: Node) -> Node {
    match node {
        Node::Vt52Ground
        | Node::Vt52Escape
        | Node::Vt52Row
        | Node::Vt52Col
        | Node::Vt52Utf8One
        | Node::Vt52Utf8Two
        | Node::Vt52Utf8Three => Node::Vt52Ground,
        _ => Node::Ground,
    }
}

/// `ESC` in VT52, and the two bytes of a direct address.
const fn vt52(node: Node, b: StreamByte) -> Edge {
    match node {
        // **A second `ESC` restarts the sequence rather than dispatching.**
        // `ESC ESC A` moves the cursor up; a uniform row would hand `ESC` to
        // `Vt52::parse`, which does not name it, and the `A` would then be
        // drawn as text. One compare in a `const fn` that runs at build time.
        Node::Vt52Escape => match b.raw_byte() {
            0x1b => edge(Act::Consume, Node::Vt52Escape),
            b'Y' => edge(Act::Consume, Node::Vt52Row),
            _ => edge(Act::Vt52Dispatch, Node::Vt52Ground),
        },
        // `ESC Y <row> <col>`, each biased by `0x20`. Two nodes because the
        // pair is a parser sub-state nothing else in either protocol reaches,
        // and because a read can fall between them.
        Node::Vt52Row => edge(Act::Vt52SetRow, Node::Vt52Col),
        _ => edge(Act::Vt52Address, Node::Vt52Ground),
    }
}

/// The body of a control string, in the node it is: `OscStr`, `DcsStr` or
/// `SosStr`. `ESC` may begin `ST` in all three; `BEL` ends only the `OSC`.
///
/// `9C`, the one-byte `ST`, and the other C1 controls are `one()`'s, under
/// ISO 8859-1 only: in UTF-8 they are continuation bytes and belong to
/// whatever character opened them, so they reach here and are payload.
const fn string(node: Node, b: StreamByte) -> Edge {
    match b.raw_byte() {
        // **`BEL` ends an `OSC` and no other string.** `[[source: ctlseqs]]`
        // allows it beside `ST` "for terminating OSC sequences" (2023-2024),
        // and applications overwhelmingly send it, so a core that took only
        // `ST` would drop nearly every title it was sent. It names no other
        // string, and `[[source: ecma48]]` 5.6 closes every control string
        // with `ST` alone. So the other two rows follow
        // `[[source: williams]]`: `dcs passthrough` puts it (388), and it is
        // payload, as the `Pt` of a DECRQSS that then names nothing;
        // `sos/pm/apc string` ignores it (426). One shared row ended all five,
        // so a DCS answered at the `BEL` and drew what followed it.
        //
        // **Neither rings**: put and ignore do not execute the byte. xterm
        // does, which is its departure and not followed: its `sos_table`
        // holds every string, `BEL` there is `CASE_BELL`
        // (`[[source: xterm-vtprstbl-c]]` 7630), and `CASE_BELL` ends an
        // `OSC` and otherwise calls `Bell` and leaves the string open
        // (`[[source: xterm-reset]]`'s file, 3691-3705). Not a terminator, it
        // agrees: the byte is appended to a DCS's buffer (3454) before that
        // case is reached, as put does here, so a DECRQSS with one fails its
        // `strcmp` (`[[source: xterm-misc]]`'s file, 4935), and its Sixel
        // parser is handed only `CASE_IGNORE` bytes (3408-3412), so never the
        // `BEL`. Here the Sixel handler drops it, with every C0 and `DEL`,
        // before the decoder (`Paint::str_put`): the decoder reads a number
        // up to its first non-digit, so a `BEL` put inside one split it.
        0x07 => match node {
            Node::OscStr => edge(Act::StrEnd, Node::Ground),
            Node::DcsStr => edge(Act::StrPut, Node::DcsStr),
            _ => edge(Act::Consume, node),
        },
        0x1b => edge(Act::Consume, Node::StrEsc),
        // **A C0 in the payload is ignored, not stored.**
        // `[[source: williams]]`'s `osc_string` makes only `20-7F` `osc_put`;
        // `00-06`, `08-17`, `19` and `1C-1F` take the *ignore* action, so the
        // byte is dropped and the string continues rather than ending or
        // being recorded. `18` and `1A` are absent here because the `anywhere`
        // rule takes them first -- they cancel.
        //
        // An OSC payload is **attacker-supplied**, so a raw control byte
        // landing in a window title is in the threat model rather than only in
        // the conformance table. This core stored them until
        // core2's control-string differential grew a row for it:
        // `ESC ] 0 ; t i <01> t l e BEL` set the title to `ti\x01tle` where
        // `baud-core2` set it to `title`.
        //
        // **In a DCS body this row departs from williams, and it is open.**
        // `dcs passthrough` puts every C0 here and ignores `7F` (388); this
        // row, the `osc string` one, ignores the C0 and stores `7F`, so a
        // DECRQSS reads a `CR` in its `Pt` as absent where williams' handler,
        // and xterm's buffer, would see it, and keeps a `7F` williams drops.
        // A stored `7F` reached the Sixel decoder too and split a number in
        // it, `!1 DEL 0~` drawing one column for ten, until the Sixel handler
        // dropped C0 and `DEL` itself (`Paint::str_put`); DECRQSS is the
        // consumer the gap is left in. Whether to follow the passthrough row
        // whole is the owner's call; only `BEL` moved here.
        0x00..=0x06 | 0x08..=0x17 | 0x19 | 0x1c..=0x1f => edge(Act::Consume, node),
        _ => edge(Act::StrPut, node),
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
pub(crate) fn split(act: Act, node: Node) -> Edge {
    // **Which family of continuation nodes, not just how many are owed.** A
    // character cut by a `read(2)` in VT52 has to resume in VT52; returning the
    // ANSI nodes here would leave the protocol at a read boundary, so the same
    // bytes would render differently depending on where the reads fell.
    match (ground_of(node), act) {
        (Node::Vt52Ground, Act::Char2) => edge(Act::Begin2, Node::Vt52Utf8One),
        (Node::Vt52Ground, Act::Char3) => edge(Act::Begin3, Node::Vt52Utf8Two),
        (Node::Vt52Ground, _) => edge(Act::Begin4, Node::Vt52Utf8Three),
        (_, Act::Char2) => edge(Act::Begin2, Node::Utf8One),
        (_, Act::Char3) => edge(Act::Begin3, Node::Utf8Two),
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
        // **The ladder has moved three times and none was a regression.**
        // Every move is the grammar starting to tell a byte apart that it used
        // to treat as any other unrouted C0:
        //
        // | | stride 1-4 | stride 4 grows |
        // |---|---|---|
        // | before | 22/402/6,441/96,812 | -- |
        // | `Act::Tab` -- HT routed | 23/444/7,619/123,202 | +27% |
        // | `CAN`/`SUB` -- the `anywhere` edge | 24/477/8,352/137,338 | +11.5% |
        // | `Act::ShiftOut`/`ShiftIn` -- SO and SI | 26/571/11,294/211,410 | **+54%** |
        // | `StrStart`/`StrPut`/`StrEnd` -- OSC | 26/574/11,478/217,172 | +2.7% |
        // | C0 in a payload ignored, not stored | 26/574/11,455/216,403 | **-0.4%** |
        // | `Act::DcsQuery` -- DECRQSS's `$ q` prefix | 29/641/12,596/235,526 | +8.8% |
        // | VT52 -- seven nodes, three actions | 30/662/13,143/247,683 | +5.2% |
        // | `Act::Bell` -- BEL counted, not consumed | 30/921/13,339/255,588 | +3.2% |
        // | **the 8-bit C1 controls, routed** | **34/921/22,619/522,818** | **+105%** |
        // | C0 and `DEL` ignored in a DCS prefix | **34/958/24,113/566,827** | +8.4% |
        // | DCS `Pa;Pb;Ph` routed to `csi` (Digit/Sep) | **34/968/24,645/584,842** | +0.6% at stride 4 |
        // | `ESC` abandons a sequence (the anywhere `1B` edge) | **34/964/24,373/572,642** | **-2.1%** |
        // | `DEL` ignored in ground, not drawn as U+FFFD (D4) | **34/963/24,272/568,022** | **-0.8%** |
        // | a Latin-1 C1 cancels a control string and acts | **34/955/23,735/549,546** | **-3.3%** |
        // | ... and an escape or control sequence | **34/955/23,743/555,038** | +1.0% |
        // | `BEL` ends only an `OSC`: three string bodies | **36/1,075/27,628/663,126** | **+19.5%** |
        //
        // **The `BEL` row is a prefix's cost, the shape DECRQSS paid.** Eight
        // edges changed what they do, taken against a dump of every edge
        // before: `07` in `Dcs`, `DcsDollar` and the two new bodies, under
        // both encodings, each from `StrEnd` into ground to `Consume` or
        // `StrPut` in place. Every other edge is what it was but for the node
        // it names: `OscStr` is the old `Str` row exactly, `DcsStr` and
        // `SosStr` are that row but for `07`, and the edges into the old body
        // now name one of the three. What moves stride 1, by 2, is the last
        // part: `5D` led to the body `58`, `5E` and `5F` led to, and now leads
        // to `OscStr` where they lead to `SosStr`, so `]` parts from `X ^ _`,
        // and under ISO 8859-1 `9D` from `98 9E 9F`. No class merges. At
        // stride 2, 112 old classes split into 232 and none merge, against a
        // dump of the old partition. By cause, 106 are that split seen
        // through a pair: 104 a byte and then an introducer or the reverse,
        // and 2 a `BEL` that rings and then an introducer, `07 5D` parting
        // from `07 58 5E 5F` and `07 9D` from `07 98 9E 9F`. 6 hold a `BEL`
        // the string no longer ends on -- `BEL` then `$`, `q` or `9C`, which
        // the prefix or body it stayed in now reads; `$` then `BEL`; and an
        // introducer then `BEL`, now three ways where it was one, for `P`,
        // `]` and `X ^ _` and for `90`, `9D` and `98 9E 9F`. At stride 3,
        // 3,472 old classes split into 7,357 and none merge. Stride 4
        // compounds the same and was not decomposed.
        //
        // **The sequence half grows it a little, and again only the Latin-1
        // table moved**: the UTF-8 edges hash as they did, and exactly 160
        // Latin-1 edges changed, the thirty-two C1 bytes in each of `Escape`
        // and the four `CSI` nodes, each now the ground edge. Stride 1 stays
        // at 34, for the reason the string row gave. Stride 2 does not move
        // at all, class for class, taken against a dump of the old partition:
        // a C1 after `ESC` or `9B` was already told apart by start nodes that
        // never enter the sequence. The signature reads `Resync` out of a
        // UTF-8 continuation node as landing in ground, so `1B 9B` from
        // `Utf8One` reached the C1's ground row; and from `Vt52Col` the `ESC`
        // is a coordinate and lands in VT52 ground, where note AE's D3 leaves
        // a Latin-1 C1 its ANSI edge. At stride 3 two old classes split five
        // ways and none merge: `CAN` or `SUB`, then `ESC` or `9B`, then a
        // byte the sequence consumed, which is now one of an executing C1,
        // `90`, the other string introducers, `9B`, or still consumed (`7F`
        // and `A0-FF`). The `CAN` or `SUB` first is what makes them new: it
        // sends every start node to its ground, `Vt52Row` and the
        // continuation nodes included, so the second byte reaches a sequence
        // node by no path that told the C1s apart before. After any other
        // first byte `Vt52Row` still did. Stride 4 compounds the same and was
        // not decomposed.
        //
        // **The C1 row shrinks it too, and it moved only the Latin-1 table**:
        // a hash of all 5,120 UTF-8 edges is the same before and after. A C1
        // inside `Str`, `Dcs` or `DcsDollar` now takes the edge it takes from
        // ground (`9C` ends the string), where it was stored or consumed.
        // Stride 1 stays at 34: every C1 byte was already told from the
        // others by its ground row, and the string rows now repeat that row.
        // At stride 2, eight pairs of classes merge and none splits, taken
        // against a copy of the old edges: `90 c` with `98 c` for each of five
        // C1 behaviours `c`, because the DCS prefix and a string body now
        // treat a C1 alike; and `80 80` with `80 9C`, `9B 80` with `9B 9C`,
        // `9C 80` with `9C 9C`, because a C1 no longer leaves the parser
        // inside a string, where `9C` would end it and `80` be stored. At
        // stride 3, 532 new classes each absorb old ones and six old classes
        // split: `CAN`, `SUB`, `BEL` or `ESC`, then a DCS or string
        // introducer, then a C1. The first byte narrows where the word can
        // start, so the introducer reaches a string node only by paths where
        // the old graph consumed every C1 alike; now each takes its own edge.
        // Stride 4 compounds the same and was not decomposed.
        //
        // **The `DEL` row shrinks it again, and the same way.** `7F` in
        // ground took `Act::Bad` and draws nothing now: it takes
        // `Act::Consume`, the edge it already took in the sequence nodes, so
        // words that differed only by what a `DEL` in ground did stop being
        // distinct. It adds no action and no class; stride 1 stays at 34.
        //
        // **The `ESC` row is the first to shrink the ladder.** It adds no
        // action and no class: `ESC` inside a sequence now takes the edge it
        // takes from ground, so words that differed only by landing an `ESC`
        // in a `CSI` node stop being distinct.
        //
        // The last row added **no class at stride 1** -- `Act::Consume` with a
        // different next node, the cheapest kind of edge this graph has, the
        // same shape `CAN`/`SUB` took. Stride 1 is unchanged at 34 and every
        // wider stride grows, because what changed is which *node* a byte
        // leaves the prefix in, and that is only visible to a word.
        //
        // **That last row is the largest move the ladder has recorded, and it
        // added no `Act` at all.** `80..9F` under Latin-1 stopped being one
        // undifferentiated `Act::Bad` and became the `Escape` row read at an
        // offset -- so `9B` is a `CSI` introducer, `9D` an `OSC` one, `9C` a
        // terminator, and the grammar now tells thirty-two bytes apart that it
        // used to treat as one. Stride 4 **doubles**.
        //
        // Which is the trade stated exactly as this table exists to state it:
        // the cost of an encoding is table *content*, paid in a table nobody
        // has built, against a per-byte loop that is unchanged. The four
        // numbers moved and the two pins caught it on the first run, which is
        // the fourth time they have.
        //
        // **The cheapest move added no action at all.** `CAN`/`SUB` is
        // `Act::Consume` with a different next node, so it cost table content
        // and nothing in the dispatch. The dearest added two at once, and two
        // is not twice one: a stride-k table is `CLASSES^k`, so actions
        // compound -- 24 -> 26 is +8% at stride 1 and +54% at stride 4.
        //
        // **Three new actions and stride 1 did not move, which is the
        // interesting row.** The ladder counts *behaviours*, not variants, and
        // every byte these three renamed was already distinguished by the node
        // it led to: a body byte stayed in `Str` where `BEL` went to `Ground`,
        // and the five introducers went to `Str` where every other escape
        // final went to `Ground`. So the split existed and only its name is
        // new -- which is why 26 holds and the wider strides grow by 2.7%
        // rather than by the 54% two genuinely new splits cost.
        //
        // **And the row after it is the first one that goes down.** Ignoring
        // C0 inside a payload adds a rule and *removes* 769 stride-4 classes,
        // because those bytes used to be `StrPut` -- indistinguishable from
        // the printable payload around them -- and are now `Consume` back into
        // `Str`, which is what an unrouted byte already does in three other
        // nodes. Telling two bytes apart can make the alphabet smaller when
        // the class it moves them into already exists.
        //
        // **DECRQSS is the first move to shift stride 1**, 26 -> 29, and one
        // new action does not explain three. Two of the three are the *nodes*:
        // `$` and `q` now do something no other byte does from `Dcs` and
        // `DcsDollar`, so each earns a class of its own, and `Act::DcsQuery`
        // is the third. That is what a **prefix** costs as against an
        // operation -- `OSC` added three actions and moved stride 1 by nothing
        // at all, because its parameters live in the payload, where the graph
        // never looks at them.
        //
        // **Seven nodes and three actions moved stride 1 by one.** Not by
        // three: `Vt52Escape` is deliberately *one uniform row* handing the
        // byte to `Vt52::parse` at run time, so the fifteen VT52 commands share
        // a class instead of earning fifteen. What did earn one is `Y`, which
        // alone leads to the two address nodes. An action per command is the
        // honest-looking spelling and this file's own arithmetic says what it
        // costs: `CLASSES^k`, so fifteen new classes at stride 4 is not fifteen
        // times anything.
        //
        // Which is the argument for this representation stated as arithmetic.
        // Telling bytes apart in a *table* costs entries; telling them apart
        // in the `match` costs the per-byte loop. The first is a number you
        // can look at before you pay it.
        //
        // **Pinned, because these numbers size a table.** Each is the count
        // of distinct behaviours a k-byte word can have across every node and
        // both encodings -- the alphabet a stride-k transition table is
        // indexed by, once the characters that label the same transitions
        // everywhere are grouped.
        // **The Sixel hook moved strides 2, 3 and 4, not stride 1.** Routing a
        // `DCS` bare-`q` to `Act::SixelStart` and collecting the numeric
        // parameters `30-3F` in `Node::Dcs` (rather than dumping the first one
        // to the body) tells apart the `q` final and the param bytes there.
        // It nets zero new stride-1 classes, but the param bytes `30-3B` now
        // differ from the markers `3C-3F` and the intermediates `20-2F` at that
        // node, so 958 -> 967 at stride 2, 24,113 -> 24,566 at stride 3 and
        // 566,827 -> 581,143 at stride 4 -- a distinction invisible in one byte
        // compounding in two, three and four. That is the cost of recognising
        // Sixel stated as arithmetic, the same shape every action this file
        // adds pays.
        assert_eq!(one.len(), CLASSES, "stride 1");
        assert_eq!(two.len(), 1_075, "stride 2");
        assert_eq!(three.len(), 27_628, "stride 3");
        assert_eq!(four.len(), 663_126, "stride 4");
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
        for (i, sig) in sigs.iter_mut().enumerate() {
            let b = StreamByte::ALL[i];
            for (n, node) in Node::FROM_INDEX.iter().enumerate() {
                sig[n] = Edge::of(InputEncoding::Utf8, *node, b);
                sig[NNODE + n] = Edge::of(InputEncoding::Latin1, *node, b);
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
    /// `Escape` and `DcsStr`, the DECRQSS body (`Str` when there was one body
    /// node); **`CsiEntry`, `CsiInter`, `CsiIgnore` and `StrEsc`
    /// are reached by no payload in the repository**, so a refactor that
    /// pushed the test back into the node functions would leave four nodes
    /// broken and every test green.
    ///
    /// This is exhaustive over both encodings and every node, twenty-two
    /// since the string body split, so the claim is a gate rather than a
    /// comment.
    #[test]
    fn can_and_sub_cancel_from_every_node_that_is_not_ground_like() {
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for node in Node::FROM_INDEX {
                // **Written against `ground_of`, not against a second copy of
                // the list.** This test kept its own and expected the literal
                // `Node::Ground`, so VT52 failed it on every VT52 node --
                // correctly, because a cancel there must return to VT52 ground.
                // Two spellings of one rule is the shape this repo records four
                // times; the fix is to ask the function.
                let g = ground_of(node);
                let ground_like = matches!(
                    node,
                    Node::Ground
                        | Node::Utf8One
                        | Node::Utf8Two
                        | Node::Utf8Three
                        | Node::Vt52Ground
                        | Node::Vt52Utf8One
                        | Node::Vt52Utf8Two
                        | Node::Vt52Utf8Three
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
                                Node::Ground | Node::Vt52Ground => edge(Act::Consume, g),
                                _ => edge(Act::Resync, g),
                            },
                            "{node:?} {enc:?} byte {:#04x}",
                            b.raw_byte()
                        ),
                        false => assert_eq!(
                            got,
                            edge(Act::Consume, g),
                            "{node:?} {enc:?} byte {:#04x} does not cancel",
                            b.raw_byte()
                        ),
                    }
                }
            }
        }
    }

    /// **`ESC` abandons an escape or control sequence from every node that
    /// holds one, and keeps its own meaning everywhere else.** Written out
    /// node by node rather than as a rule, because the rule is the thing under
    /// test: `[[source: williams]]`'s anywhere edge to `escape` on `1B`, whose
    /// entry action *clears*.
    ///
    /// The four `CSI` nodes and `Escape` sent `ESC` to `c0()`, which consumed
    /// it and stayed in the sequence, so `CSI 1 ESC [ 2 m` dispatched `CSI 1 [`
    /// and printed `2m`. `docs/vt-coverage.md`'s `1B` row said `done` the
    /// whole time: its test is the mnemonic `ESC`, which every suite mentions,
    /// so this is the row's real gate. The other nodes are excluded on
    /// purpose, and each exclusion is an edge below, so a refactor that folds
    /// them into the rule fails here:
    ///
    /// * a control string's `ESC` is the first half of `ST` -- sent to
    ///   `Escape`, every OSC, DECRQSS and Sixel ended by `ST` would be lost;
    /// * `StrEsc` already re-dispatches any byte but `\` into `Escape`;
    /// * mid-character, the partial character becomes a replacement and the
    ///   `ESC` is re-dispatched from ground, which is the same outcome;
    /// * VT52 has no `CSI`: its escape restarts on a second `ESC`, and a
    ///   direct address takes one as a coordinate, as xterm's does.
    #[test]
    fn an_esc_abandons_every_sequence_and_keeps_its_meaning_elsewhere() {
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for node in Node::FROM_INDEX {
                let want = match node {
                    Node::Ground
                    | Node::Escape
                    | Node::CsiEntry
                    | Node::CsiParam
                    | Node::CsiInter
                    | Node::CsiIgnore => edge(Act::EscStart, Node::Escape),
                    Node::Utf8One | Node::Utf8Two | Node::Utf8Three => {
                        edge(Act::Resync, Node::Ground)
                    }
                    Node::Vt52Utf8One | Node::Vt52Utf8Two | Node::Vt52Utf8Three => {
                        edge(Act::Resync, Node::Vt52Ground)
                    }
                    Node::Dcs | Node::DcsDollar | Node::OscStr | Node::DcsStr | Node::SosStr => {
                        edge(Act::Consume, Node::StrEsc)
                    }
                    Node::StrEsc => edge(Act::Again, Node::Escape),
                    Node::Vt52Ground | Node::Vt52Escape => edge(Act::Consume, Node::Vt52Escape),
                    Node::Vt52Row => edge(Act::Vt52SetRow, Node::Vt52Col),
                    Node::Vt52Col => edge(Act::Vt52Address, Node::Vt52Ground),
                };
                assert_eq!(
                    Edge::of(enc, node, StreamByte::ESC),
                    want,
                    "{node:?} {enc:?}: ESC"
                );
            }
        }
    }

    /// **Under ISO 8859-1 a C1 byte leaves a control string as it leaves
    /// ground; under UTF-8 it stays in the string, the continuation byte it
    /// is.** Exhaustive over `80-9F` and the five string nodes, and checked
    /// against the shipped table.
    ///
    /// `[[source: williams]]` (612-615): "All C1 controls cancel any escape
    /// sequence, control sequence or control string in progress and are
    /// executed." The string node ended only on `9C` and stored the other
    /// thirty-one, and the DCS prefix nodes sent all thirty-two, `9C`
    /// included, into the body. The behaviour tests are in
    /// `tests/encoding/latin1.rs`; this is the gate for the edges they cannot
    /// see, such as an `SOS` that a `98` replaces with another.
    #[test]
    fn a_latin1_c1_leaves_every_control_string_and_a_utf8_one_stays() {
        // Positive control: the Ground row compared against is the C1 row,
        // not a row of `Bad` that every node could agree with by accident.
        let ground = |v: usize| Edge::of(InputEncoding::Latin1, Node::Ground, StreamByte::ALL[v]);
        assert_eq!(ground(0x85), edge(Act::EscFinal, Node::Ground), "85 NEL");
        assert_eq!(ground(0x9b), edge(Act::CsiStart, Node::CsiEntry), "9B CSI");
        assert_eq!(ground(0x90), edge(Act::StrStart, Node::Dcs), "90 DCS");
        assert_eq!(ground(0x9d), edge(Act::StrStart, Node::OscStr), "9D OSC");
        for node in [
            Node::Dcs,
            Node::DcsDollar,
            Node::OscStr,
            Node::DcsStr,
            Node::SosStr,
        ] {
            for v in 0x80..=0x9fusize {
                let b = StreamByte::ALL[v];
                let want = match v {
                    0x9c => edge(Act::StrEnd, Node::Ground),
                    _ => ground(v),
                };
                assert_eq!(
                    Edge::of(InputEncoding::Latin1, node, b),
                    want,
                    "{node:?} Latin1 byte {v:#04x}"
                );
                let kept = match node {
                    Node::Dcs | Node::DcsDollar => edge(Act::Consume, Node::DcsStr),
                    _ => edge(Act::StrPut, node),
                };
                assert_eq!(
                    Edge::of(InputEncoding::Utf8, node, b),
                    kept,
                    "{node:?} Utf8 byte {v:#04x}"
                );
            }
        }
    }

    /// **A BEL ends an OSC and no other control string**, checked against the
    /// shipped table in both encodings, at the node each introducer reaches.
    ///
    /// `[[source: ctlseqs]]` gives BEL as a terminator "for terminating OSC
    /// sequences" (2023-2024) and for nothing else; `[[source: ecma48]]` 5.6
    /// ends a control string with ST alone. `[[source: williams]]` puts BEL in
    /// `dcs passthrough` (388), ignores it in `dcs entry`, `dcs param` and `dcs
    /// intermediate` (354, 361, 368) and in `sos/pm/apc string` (426). The
    /// body node was shared, so all five ended on BEL, and so did the DCS
    /// prefix. The nodes are found by walking the table from ground rather
    /// than named, so the gate holds whatever they are called, and a node two
    /// introducers share must answer for both.
    #[test]
    fn a_bel_ends_an_osc_and_no_other_control_string() {
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            let walk = |bytes: &[u8]| {
                let mut n = Node::Ground;
                for b in bytes {
                    n = Edge::of(enc, n, StreamByte::from_stream(*b)).next;
                }
                n
            };
            let bel = |n: Node| Edge::of(enc, n, StreamByte::BEL);
            // Positive control: each walk ends inside a string, where an
            // `ESC` is the first half of `ST`, and not back in ground.
            for intro in [
                b"\x1b]".as_slice(),
                b"\x1bP",
                b"\x1bP$",
                b"\x1bP$q",
                b"\x1bPq",
                b"\x1bP|",
                b"\x1bX",
                b"\x1b^",
                b"\x1b_",
            ] {
                assert_eq!(
                    Edge::of(enc, walk(intro), StreamByte::ESC),
                    edge(Act::Consume, Node::StrEsc),
                    "{enc:?} {intro:?}: not inside a string"
                );
            }
            assert_eq!(
                bel(walk(b"\x1b]")),
                edge(Act::StrEnd, Node::Ground),
                "{enc:?}: OSC"
            );
            for prefix in [b"\x1bP".as_slice(), b"\x1bP$", b"\x1bP1;2"] {
                let n = walk(prefix);
                assert_eq!(bel(n), edge(Act::Consume, n), "{enc:?} {prefix:?}");
            }
            for body in [b"\x1bP$q".as_slice(), b"\x1bPq", b"\x1bP|"] {
                let n = walk(body);
                assert_eq!(bel(n), edge(Act::StrPut, n), "{enc:?} {body:?}");
            }
            for body in [b"\x1bX".as_slice(), b"\x1b^", b"\x1b_"] {
                let n = walk(body);
                assert_eq!(bel(n), edge(Act::Consume, n), "{enc:?} {body:?}");
            }
        }
    }

    /// **Under ISO 8859-1 a C1 byte cancels an escape or control sequence and
    /// takes the edge it takes from ground; under UTF-8 each of these nodes
    /// keeps the row it had.** Exhaustive over `80-9F`, the five sequence
    /// nodes and both encodings, and checked against the shipped table.
    ///
    /// `[[source: williams]]` (612-615): "All C1 controls cancel any escape
    /// sequence, control sequence or control string in progress and are
    /// executed." `escape()` sent all thirty-two to `Consume` and `Ground`,
    /// so `ESC 85` lost its NEL, and the `csi_*` rows consumed them, `csi
    /// param` staying where it was. `9C` takes the ground edge here rather
    /// than `StrEnd`: there is no string to end, and williams draws it to
    /// ground with no action (268). `StrEsc`, the `ESC` inside a string,
    /// sends every byte but `\` back through `Escape` with `Again`, so it
    /// takes this rule through that row and its own edge does not move.
    #[test]
    fn a_latin1_c1_cancels_every_escape_and_control_sequence_and_a_utf8_one_does_not() {
        // Positive control: the Ground row compared against is the C1 row.
        let ground = |v: usize| Edge::of(InputEncoding::Latin1, Node::Ground, StreamByte::ALL[v]);
        assert_eq!(ground(0x84), edge(Act::EscFinal, Node::Ground), "84 IND");
        assert_eq!(ground(0x9b), edge(Act::CsiStart, Node::CsiEntry), "9B CSI");
        assert_eq!(ground(0x98), edge(Act::StrStart, Node::SosStr), "98 SOS");
        assert_eq!(ground(0x9c), edge(Act::EscFinal, Node::Ground), "9C ST");
        for (node, utf8) in [
            (Node::Escape, edge(Act::Consume, Node::Ground)),
            (Node::CsiEntry, edge(Act::Consume, Node::CsiIgnore)),
            (Node::CsiParam, edge(Act::Consume, Node::CsiParam)),
            (Node::CsiInter, edge(Act::Consume, Node::CsiIgnore)),
            (Node::CsiIgnore, edge(Act::Consume, Node::CsiIgnore)),
        ] {
            for v in 0x80..=0x9fusize {
                let b = StreamByte::ALL[v];
                assert_eq!(
                    Edge::of(InputEncoding::Latin1, node, b),
                    ground(v),
                    "{node:?} Latin1 byte {v:#04x}"
                );
                assert_eq!(
                    Edge::of(InputEncoding::Utf8, node, b),
                    utf8,
                    "{node:?} Utf8 byte {v:#04x}"
                );
            }
        }
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for v in 0x80..=0x9fusize {
                assert_eq!(
                    Edge::of(enc, Node::StrEsc, StreamByte::ALL[v]),
                    edge(Act::Again, Node::Escape),
                    "StrEsc {enc:?} byte {v:#04x}"
                );
            }
        }
    }

    /// The measured count, pinned. `CLASSES^4` is what a four-byte window
    /// costs as a perfect index.
    ///
    /// **Twenty-six**, comparing whole edges. A four-byte window is
    /// `26^4 = 456,976` entries and a three-byte window `17,576` -- both
    /// perfect indexes with no hash, where the same windows over raw bytes are
    /// 4.3 billion and 16.7 million. Comparing actions alone gives fewer,
    /// which is the wrong equivalence: two bytes are interchangeable only if
    /// they also leave the parser in the same node.
    ///
    /// **It has only ever gone up, and each step is a byte the grammar started
    /// telling apart**: 21 before `:` was distinguished from `;` and the
    /// private markers and intermediates stopped being consumed, 22 after, 23
    /// when HT was routed to the tab machinery, 24 when `CAN` and `SUB` gained
    /// the `anywhere` cancel, 26 when `SO` and `SI` became the locking shifts.
    /// A number that moves this way is the table growing to fit the protocol,
    /// not a regression -- and the assertion is here so that growth is a
    /// decision rather than a discovery.
    // 36, not the 26 the paragraph above narrates: HT, CAN/SUB, SO/SI and the
    // 8-bit C1 routing each split more bytes apart after that prose was
    // written, which made 34, and the `OSC` body parting from the `SOS`, `PM`
    // and `APC` one on `BEL` split `5D` from `58 5E 5F` and `9D` from
    // `98 9E 9F`, which made 36.
    const CLASSES: usize = 36;
    /// **The loop always advances**, and it is a table property rather than a
    /// hope.
    ///
    /// `feed` is `while done < n { done += step(..) }`, so a step that consumes
    /// nothing must at least *move*, to a node where the same byte does consume.
    /// `Act::Resync` is the one zero-width action -- it makes a partial
    /// character a replacement and re-dispatches the byte, which
    /// `[[source: unicode-ch3]]` 3.9.5 requires -- and its whole safety
    /// argument is that the node it lands in cannot produce it again.
    ///
    /// That argument was a comment. `acsc` in the `ansi` terminfo entry is a
    /// hundred bytes of raw high bytes and it hung the parser, so the comment
    /// was wrong somewhere; this walks every node against every byte under both
    /// encodings and says where.
    /// What `write::act` consumes, which is **not** `Act::width()`.
    ///
    /// `width` is the byte length of the *character* an act writes; two acts
    /// consume nothing at all and re-dispatch the byte. A first version of the
    /// gate below asked `width` and passed against a live hang -- the
    /// probe-that-cannot-fire shape this file records eight times, and the
    /// eighth was mine.
    fn consumed(a: Act) -> ByteLen {
        match a {
            Act::Resync | Act::Again => ByteLen::ZERO,
            _ => a.width(),
        }
    }

    /// **`split`'s wildcard means "Char4", and only `feed` makes that true.**
    ///
    /// `split` ends in `_ => edge(Act::Begin4, ...)`, so any act reaching it
    /// that is not `Char2` or `Char3` is taken to owe four bytes. Nothing in
    /// `split` establishes that. What does is one line in `feed`:
    ///
    /// ```text
    /// if !left.holds(e.act.width()) { e = split(e.act, *node); }
    /// ```
    ///
    /// `width` is `ONE` for every act but the three character ones, and the
    /// loop runs only while a byte remains, so `left.holds(ONE)` cannot fail
    /// and `split` is unreachable for them. The invariant is real and it lives
    /// in a different file from the wildcard that depends on it.
    ///
    /// Give `width` a value above one for some other act -- a five-byte
    /// encoding, a fused action -- and that act starts arriving at `split`,
    /// which will quietly tell the parser to wait for four bytes. It would not
    /// fail to compile and no existing test would notice: the graph still
    /// terminates, chunk invariance still holds at every read size that does
    /// not land inside such a character, and the cells only differ on input the
    /// corpus does not contain.
    ///
    /// So the assumption is checked rather than commented. This is the cheap
    /// half of the plan's "`split` as a table" step; the table itself is
    /// dropped, and the reason is in `CLAUDE.md`.
    #[test]
    fn only_the_character_acts_are_wider_than_one_byte() {
        let mut wide = Vec::new();
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for node in Node::FROM_INDEX {
                for i in 0..256usize {
                    let a = Edge::of(enc, node, StreamByte::ALL[i]).act;
                    if a.width().index() > 1 {
                        wide.push(a);
                    }
                }
            }
        }
        // Positive control: the walk must find the three character acts, or it
        // is asserting over an empty set and cannot fail.
        assert!(
            wide.iter().any(|a| matches!(a, Act::Char2))
                && wide.iter().any(|a| matches!(a, Act::Char3))
                && wide.iter().any(|a| matches!(a, Act::Char4)),
            "the walk did not reach all three character acts -- it is checking \
             nothing"
        );
        for a in wide {
            assert!(
                matches!(a, Act::Char2 | Act::Char3 | Act::Char4),
                "{a:?} is wider than one byte, so it can reach `split`, whose \
                 wildcard would treat it as Char4 and wait for four bytes"
            );
        }
    }

    /// And `split` maps each of the three to the length it actually owes.
    ///
    /// The wildcard is right for `Char4` only if `Char2` and `Char3` are taken
    /// by the arms above it -- a transposition here would park the accumulator
    /// waiting for the wrong number of continuations, which is a screen that
    /// depends on where the reads fell.
    #[test]
    fn split_owes_the_bytes_the_character_actually_has() {
        for ground in [Node::Ground, Node::Vt52Ground] {
            for (act, want) in [
                (Act::Char2, Act::Begin2),
                (Act::Char3, Act::Begin3),
                (Act::Char4, Act::Begin4),
            ] {
                assert_eq!(
                    split(act, ground).act,
                    want,
                    "{act:?} from {ground:?} must owe its own continuations"
                );
            }
        }
    }

    #[test]
    fn every_step_of_the_loop_advances() {
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for node in Node::FROM_INDEX {
                for i in 0..256usize {
                    let byte = StreamByte::ALL[i];
                    // Follow the zero-consuming edges. Each one must reach a
                    // node that consumes, in fewer hops than there are nodes --
                    // anything longer is a cycle by the pigeonhole principle.
                    let mut at = node;
                    let mut path = [node; NNODE];
                    let mut hops = 0usize;
                    loop {
                        let e = Edge::of(enc, at, byte);
                        if consumed(e.act).index() > 0 {
                            break;
                        }
                        assert!(
                            hops < NNODE,
                            "{enc:?} byte {i:#04x} cycles without consuming: \
                             {path:?} -- the loop cannot advance"
                        );
                        path[hops] = e.next;
                        hops += 1;
                        at = e.next;
                    }
                }
            }
        }
    }
}
