//! The VT protocol as a **graph**, and the graph as a lookup.
//!
//! 23 nodes, one per parser state, each with 256 out-edges, one per input
//! byte. An edge says what the byte does and which node it leads to, so
//! feeding a stream is walking the graph: `graph[n0][b0]` gives `n1`,
//! `graph[n1][b1]` gives `n2`, and so on. `machine.rs` spells the same graph
//! as a `match` over twenty states with a chain of byte tests inside each arm;
//! every one of those tests is a pure function of `(encoding, node, byte)` --
//! three bounded inputs -- so the answer is computable at build time and the
//! walk is a lookup.
//!
//! That is the argument that made the UAX #29 transition a table, worth 8%
//! there: a rule walk is a chain of unpredictable branches, and a mispredict
//! costs three times an L1 hit.
//!
//! **A graph, not a tree.** It has cycles -- `CsiParams` has an edge to
//! itself, `Ground` is reachable from nearly every node, `CsiIgnore` has many
//! parents -- and a trie over the VT sequences would be unbounded, because a
//! parameter is an arbitrary-length digit string. The back-edges are what make
//! it finite. Nothing is hashed, so it is not a Merkle structure; it is a
//! deterministic finite automaton, and `docs/research/notes/b-parser-as-a-graph.md`
//! carries the literature on storing one.
//!
//! **The protocol and the encoding are more nodes, not more indices.** VT52's
//! escape table is a different protocol rather than a subset of ANSI's, and
//! `machine.rs` asks `terminal_mode` inside its `Escape` arm to tell them
//! apart. Here they are two nodes in two sub-graphs that share no edge, and
//! the one crossing in either direction is a mode change -- which is an
//! *action*, not an edge. Latin-1 is a second graph for the same reason.
//!
//! Two things the graph deliberately does *not* absorb:
//!
//! * **The actions.** Which node a byte leads to is a function of three
//!   bounded values; *what it does* is a call into the grid, and that is not a
//!   lookup. The edge names the action and a flat `match` performs it, which
//!   is one dispatch over 31 arms instead of nested tests over 23 x 256.
//! * **Node payloads.** A UTF-8 accumulator and a VT52 row are values, not
//!   nodes. They live in `Term` beside the current node, because a graph is
//!   indexed by a small integer and an accumulator is not one.

#![allow(dead_code)]

use crate::*;

/// A parser state, as the small integer a table can be indexed by.
///
/// `machine.rs`'s `State` carries payloads -- a UTF-8 accumulator, a VT52 row
/// -- which is why it cannot be one. Here the payload lives in `Term` and this
/// is the discriminant alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(crate) enum Node {
    Ground = 0,
    /// Ground, but in VT52.
    ///
    /// A separate variant rather than a flag consulted from `Ground`, and the
    /// reason is measured: the flag was read in the byte loop and cost **4% of
    /// the cells path** (baud/ghostty 0.970 -> 0.931 across three runs,
    /// normalised in-run because the box was thermally drifting). As a state,
    /// the existing `st == Ground` gates exclude VT52 from the ANSI fast paths
    /// for free, and the ANSI path pays nothing at all.
    ///
    /// The trade is that VT52 text takes the byte-at-a-time path. VT52 is a
    /// 1975 terminal reached only by an explicit mode switch; the protocol
    /// everyone actually uses is the one that should be fast.
    Vt52Ground,
    /// Inside a multi-byte sequence, indexed by how many continuation bytes
    /// are still owed. **The count is part of the state**, because where the
    /// next byte goes depends on it and a table is indexed by the state alone.
    /// `machine.rs` carries it as a payload; here it is three ids.
    Utf8One,
    Utf8Two,
    Utf8Three,
    Escape,
    /// `ESC` in VT52.
    ///
    /// A state of its own, and it has to be: VT52's escape table is a
    /// different protocol rather than a subset -- `ESC D` is cursor-left there
    /// and IND here -- so one `Escape` row cannot answer for both. `machine.rs`
    /// spells this as a `terminal_mode` test *inside* the `Escape` arm, which
    /// is a fourth axis the table is not indexed by. Making it a state is what
    /// removes the test.
    Vt52Escape,
    /// After `ESC` and an intermediate byte. `[[source: williams]]`'s
    /// `escape_intermediate`. Without it `ESC # 8` dropped the `#` and printed
    /// the `8`.
    EscInter,
    /// `[[source: williams]]`'s `csi_entry`: the position *immediately* after
    /// `CSI`, and the only position where a private marker (`3C-3F`) is legal.
    ///
    /// This was folded into [`Node::CsiParams`], and the fold is exactly what
    /// the two states disagree about: `csi_entry` collects `3C-3F`, while
    /// `csi_param` sends those same bytes to `csi_ignore`. With one state,
    /// `CSI 2 ? l` was accepted as DECRST and **switched the terminal into VT52
    /// mode** — a protocol change out of a sequence the spec says to discard —
    /// and `CSI 1 ? J` erased the screen as DECSED. Executing what should be
    /// ignored is the dangerous direction, so this is a separate state.
    CsiEntry,
    CsiParams,
    /// `[[source: williams]]`'s `csi_intermediate`. The intermediate is kept,
    /// so `CSI ! p` (DECSTR) is distinguishable from `CSI p`.
    CsiInter,
    /// `[[source: williams]]`'s `csi_ignore`: a malformed sequence is consumed
    /// to its final byte and discarded. Returning to Ground mid-sequence
    /// instead would print the remainder as text.
    CsiIgnore,
    OscString,
    /// An `ESC` arrived inside an OSC payload.
    ///
    /// Its own state because the two continuations differ and both matter:
    /// `ESC \\` is ST and must **dispatch** the payload, while
    /// `[[source: williams]]` says a bare `ESC` *cancels* the control string,
    /// so anything else must discard it and start a fresh sequence. Folding
    /// this into `Escape` loses the payload either way -- which is what the
    /// terminator would have been for.
    OscEsc,
    /// `[[source: williams]]`'s `dcs_entry`: after `ESC P`, collecting
    /// parameters and intermediates before the final byte selects a handler.
    DcsEntry,
    /// `dcs_param` — inside the parameter string.
    DcsParam,
    /// `dcs_intermediate`.
    DcsInter,
    /// `dcs_passthrough` — the final byte named something baud handles, so the
    /// data between it and ST is being collected.
    DcsPass,
    /// `dcs_ignore` — a malformed or unhandled DCS. The payload is consumed to
    /// the terminator and discarded, which is the part that keeps it *safe*:
    /// falling back to Ground mid-string would execute the payload as commands.
    DcsIgnore,
    /// An `ESC` arrived inside a DCS payload. Same two continuations as
    /// [`Node::OscEsc`].
    DcsEsc,
    /// The payload of SOS, PM and APC.
    ///
    /// Consumed to the string terminator and executed as nothing. This state
    /// not existing was a **correctness and security** bug, not a missing
    /// feature: those introducers fell through to Ground, so
    /// `ESC _ ... ESC \` had its payload interpreted as commands. A stream
    /// could hide `CSI 2 J` inside an APC string and clear the screen.
    StringIgnore,
    /// `ESC Y` in VT52 mode: waiting for the row byte.
    Vt52Row,
    /// `ESC Y <row>`: waiting for the column byte.
    Vt52Col,
}

/// How many states the table is indexed over.
pub(crate) const NNODE: usize = 23;

impl Node {
    /// The node for a lead byte owing `n` continuations.
    ///
    /// **The count is the node**, which is what a graph forces you to make
    /// explicit: `machine.rs` used to carry it as a payload, and a payload
    /// cannot be an index.
    pub(crate) fn utf8_owing(n: Pending) -> Node {
        match n.owed() {
            3 => Node::Utf8Three,
            2 => Node::Utf8Two,
            _ => Node::Utf8One,
        }
    }
    /// Is the next continuation byte the one that completes the character?
    pub(crate) fn is_last_continuation(self) -> bool {
        matches!(self, Node::Utf8One)
    }
    /// One continuation nearer the end.
    pub(crate) fn one_fewer_continuation(self) -> Node {
        match self {
            Node::Utf8Three => Node::Utf8Two,
            _ => Node::Utf8One,
        }
    }

    /// Equality in a `const fn`; see [`Edge::same`].
    const fn same(self, other: Node) -> bool {
        self as u8 == other as u8
    }

    const FROM_INDEX: [Node; NNODE] = [
        Node::Ground,
        Node::Vt52Ground,
        Node::Utf8One,
        Node::Utf8Two,
        Node::Utf8Three,
        Node::Escape,
        Node::Vt52Escape,
        Node::EscInter,
        Node::CsiEntry,
        Node::CsiParams,
        Node::CsiInter,
        Node::CsiIgnore,
        Node::OscString,
        Node::OscEsc,
        Node::DcsEntry,
        Node::DcsParam,
        Node::DcsInter,
        Node::DcsPass,
        Node::DcsIgnore,
        Node::DcsEsc,
        Node::StringIgnore,
        Node::Vt52Row,
        Node::Vt52Col,
    ];
}

/// What a byte does once the table has said where it goes.
///
/// Deliberately a *flat* set: the table has already resolved which state and
/// which byte range we are in, so nothing here re-tests either.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(crate) enum Act {
    /// Consume it and do nothing. The largest arm by far.
    Nothing = 0,
    /// A C0 control that executes where it stands.
    Execute,
    /// Latin-1: one byte, one character.
    PrintByte,
    /// An 8-bit C1 outside UTF-8, re-entered through the `Escape` state.
    C1Escape,
    /// A UTF-8 lead byte: start the accumulator.
    Utf8Start,
    /// A UTF-8 continuation byte.
    Utf8Cont,
    /// Not a lead, not a continuation, not printable: the cold arm.
    GroundUncommon,
    /// A byte that cannot continue a UTF-8 sequence: emit the replacement and
    /// re-dispatch it from `Ground`. **Re-dispatching is part of the action**,
    /// so the table names the intermediate state and the action runs the table
    /// again -- which is what `machine.rs`'s recursive `control` call does.
    Utf8Bad,
    /// `CSI` seen: clear the parameter block.
    CsiClear,
    /// `DCS` seen: clear the parameter block and the string buffer.
    DcsClear,
    /// A two-byte escape, `ESC` plus a final.
    EscFe,
    /// An escape intermediate, collected.
    EscCollect,
    /// The final byte of an `ESC <inter> <final>` form.
    EscInterFinal,
    /// A private marker in `csi_entry`.
    CsiPrivate,
    /// A parameter or intermediate byte of a control sequence.
    CsiParamByte,
    /// The final byte of a control sequence.
    CsiFinal,
    /// A control sequence intermediate.
    CsiCollect,
    /// A private marker in `dcs_entry`.
    DcsPrivate,
    /// A parameter byte of a device control string.
    DcsParamByte,
    /// A device control string intermediate.
    DcsCollect,
    /// The final byte of a device control string: select the handler.
    ///
    /// **This action owns its own transition**, and it is the only one that
    /// does. Where it goes depends on `Dcs::parse(csi_inter, final)` -- whether
    /// baud has a handler for this string -- and `csi_inter` is *collected*
    /// state, not one of the three bounded inputs the table is indexed by. So
    /// the table names the action and the action decides between passthrough
    /// and ignore.
    DcsHook,
    /// A payload byte of a string.
    StringPut,
    /// A device control string terminated: dispatch it.
    DcsDispatch,
    /// A bare `ESC` inside a device control string: cancel it.
    DcsCancel,
    /// An operating system command terminated by `ST`.
    OscDispatchSt,
    /// The same, terminated by `BEL`.
    OscDispatchBel,
    /// A bare `ESC` inside an operating system command: cancel it.
    OscCancel,
    /// `CAN` or `SUB` mid-sequence: execute and abandon.
    Cancel,
    /// The first coordinate byte of `ESC Y`.
    Vt52SetRow,
    /// The second, which completes the address.
    Vt52Address,
    /// VT52's escape table, which is a different protocol rather than a subset.
    Vt52Dispatch,
}

impl Act {
    /// Equality in a `const fn`; see [`Edge::same`].
    const fn same(self, other: Act) -> bool {
        self as u8 == other as u8
    }
}

/// One transition: the action, and where the parser goes next.
///
/// Two `repr(u8)` enums side by side, so the entry is **two bytes** and the
/// lookup is one load -- the same size as packing them into a `u16`, and
/// without the shift, the mask and the two `as` casts each way. A packed word
/// was the first spelling and it cost four conversions per read to say what
/// the field names say for free.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(C)]
pub(crate) struct Edge {
    pub(crate) act: Act,
    pub(crate) next: Node,
}

const fn edge(act: Act, next: Node) -> Edge {
    Edge { act, next }
}

// ── byte classification ─────────────────────────────────────────────────────
// Everything below asks `StreamByte`. The predicates were duplicated here as
// raw-byte tests while the table was being written, which the type audit
// refused -- correctly: two spellings of one rule is how `0x7f` went missing
// from the AVX2 stop mask while the scalar tail had it. They are `const fn` on
// `StreamByte` now, so the table is built from the same definitions the rest of
// the parser uses and there is nothing to keep in sync.

/// Which ground a state returns to when its sequence ends.
///
/// **VT52 is not a fourth index; it is more states.** `machine.rs` asks
/// `terminal_mode` inside the `Escape` arm and again in `self.ground()`, which
/// is a runtime test of a value the *state* already implies: nothing in the
/// VT52 sub-graph is reachable from ANSI and nothing in the ANSI sub-graph is
/// reachable from VT52, so the table knows which ground a cell belongs to at
/// build time. The one crossing in either direction is a mode change, and that
/// is an *action* -- `set_terminal_mode` puts the parser in the other ground
/// itself, after the table has already named this one.
const fn ground_of(st: Node) -> Node {
    match st {
        Node::Vt52Ground | Node::Vt52Escape | Node::Vt52Row | Node::Vt52Col => Node::Vt52Ground,
        _ => Node::Ground,
    }
}

/// How many continuations a lead byte owes, as a state.
const fn utf8_state(b: StreamByte) -> Node {
    match b.utf8_lead() {
        Some((3, _, _)) => Node::Utf8Three,
        Some((2, _, _)) => Node::Utf8Two,
        _ => Node::Utf8One,
    }
}

/// One byte of a control sequence's parameter string.
///
/// `csi_entry` and `csi_param` differ in exactly one transition -- the private
/// marker -- so they share this and the caller handles the difference. That is
/// how `machine.rs` spells it too, and the alternative was two arms that had
/// to be kept identical.
const fn csi_param(b: StreamByte) -> Edge {
    if b.is_c0() {
        // Executed where it stands and the sequence continues: `CSI 1 <BEL> m`
        // is still an SGR.
        edge(Act::Execute, Node::CsiParams)
    } else if b.is_csi_param_byte() {
        edge(Act::CsiParamByte, Node::CsiParams)
    } else if FinalByte::from_stream(b).is_some() {
        edge(Act::CsiFinal, Node::Ground)
    } else if b.is_csi_intermediate() {
        edge(Act::CsiCollect, Node::CsiInter)
    } else if b.is_del() {
        edge(Act::Nothing, Node::CsiParams)
    } else {
        // `3C-3F` past the first position. Consumed to the final byte rather
        // than returned to Ground, which printed the rest of the sequence.
        edge(Act::Nothing, Node::CsiIgnore)
    }
}

/// One byte of a device control string's parameter string.
///
/// Shared by `dcs_entry` and `dcs_param` for the same reason, and differing
/// from `csi_param` in two rules: a C0 here is *ignored* rather than executed,
/// and there is no sub-parameter colon.
const fn dcs_param(b: StreamByte) -> Edge {
    if b.is_c0() || b.is_del() {
        edge(Act::Nothing, Node::DcsParam)
    } else if b.is_dcs_param_byte() {
        edge(Act::DcsParamByte, Node::DcsParam)
    } else if b.is_csi_intermediate() {
        edge(Act::DcsCollect, Node::DcsInter)
    } else if FinalByte::from_stream(b).is_some() {
        // `DcsHook` owns its own transition; see its documentation.
        edge(Act::DcsHook, Node::DcsParam)
    } else {
        edge(Act::Nothing, Node::DcsIgnore)
    }
}

/// A class of bytes the graph cannot tell apart.
///
/// Measured, not guessed: `the_graph_is_the_size_the_representation_assumes`
/// asserts there are **20** of them under UTF-8 and 19 under ISO 8859-1, so
/// 236 of the 256 columns in a dense row say the same thing as some other
/// column. `[[source: regex-automata-dense]]` is the statement of the general
/// technique -- an equivalence class is a set of bytes the automaton cannot
/// use to distinguish anything -- and of the saving: a row shrinks from
/// `256 * sizeof(edge)` to `k * sizeof(edge)`.
///
/// One map covers both encodings, at the cost of one class Latin-1 does not
/// need. Two maps would be two loads to choose between; the class lookup is
/// on the critical path's *input* side and should stay a single load.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub(crate) struct Class(u8);

/// How many classes the graph has.
///
/// **The joint refinement of both encodings, which is coarser than either
/// alone.** UTF-8 alone distinguishes 20 and ISO 8859-1 19, but a single map
/// has to serve both, so two bytes share a class only when *every* node agrees
/// about them under *both* -- and that partition is finer than either. The
/// number is asserted by `the_graph_is_the_size_the_representation_assumes`
/// rather than reasoned about, because getting it wrong is a build failure
/// wearing an index-out-of-bounds costume, which is what the first version of
/// this constant was.
pub(crate) const NCLASS: usize = 22;

/// An array with one entry per node, addressed by the node itself.
///
/// **The single point at which a node enters index space**, which is the same
/// role `LaneMask::from_movemask` plays for an intrinsic's return value: a
/// cast belongs where a value crosses into the type system, and nowhere else.
/// Inherent rather than `core::ops::Index`, because the graph is built by
/// `const fn` and a const fn cannot call a trait method.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(crate) struct Nodes<T>([T; NNODE]);

impl<T> Nodes<T> {
    #[inline(always)]
    const fn at(&self, n: Node) -> &T {
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

/// An array with one entry per byte class.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub(crate) struct Classes<T>([T; NCLASS]);

impl<T> Classes<T> {
    #[inline(always)]
    const fn at(&self, c: Class) -> &T {
        &self.0[c.0 as usize]
    }
}

/// How the edges are stored.
///
/// **A representation, not a protocol.** Every implementation answers the same
/// question and they are checked against each other exhaustively; what differs
/// is the table's shape and what lands on the loop-carried dependency chain.
/// They are compiled variants rather than a runtime switch, because this repo
/// has measured a runtime `if` in the hot loop manufacturing a fake 13-17%
/// cost for a change that was free.
pub(crate) trait Edges {
    fn edge(enc: InputEncoding, from: Node, b: StreamByte) -> Edge;
}

/// `graph[node][byte]`: the dense form, one row per node.
///
/// The address is `base + 256 * node + byte`, so the multiply by 256 is on the
/// **node** and therefore on the loop-carried chain --
/// `[[source: shift-dfa]]` measures that as an extra cycle of latency a byte.
/// Kept as the reference the others are checked against.
pub(crate) struct ByNode;

impl Edges for ByNode {
    #[inline(always)]
    fn edge(enc: InputEncoding, from: Node, b: StreamByte) -> Edge {
        *half_of(enc).at(from).at(b)
    }
}

/// `graph[byte][node]`: the same table transposed.
///
/// `[[source: shift-dfa]]`: "the multiplication by 256 (shift by 8) is also
/// added to the loop-carried dependency chain for the state, resulting in an
/// extra cycle of latency. You can fix that by just flipping the table."
/// Flipped, the multiply is on the *byte*, which the loop does not carry. The
/// row is padded to 32 so the index is a shift rather than a multiply by 23.
pub(crate) struct ByByte;

impl Edges for ByByte {
    #[inline(always)]
    fn edge(enc: InputEncoding, from: Node, b: StreamByte) -> Edge {
        *flipped_of(enc).at(b).at(from)
    }
}

/// `graph[node][class]`: 20 columns instead of 256.
///
/// 920 bytes an encoding against 11,776, which is the rule-#2 form of the
/// same graph. The class lookup is an extra load and it is indexed by the
/// **byte alone**, so it does not lengthen the chain -- it issues as soon as
/// the byte is available, in parallel with everything the previous byte is
/// still doing.
pub(crate) struct ByClass;

impl Edges for ByClass {
    #[inline(always)]
    fn edge(enc: InputEncoding, from: Node, b: StreamByte) -> Edge {
        *classed_of(enc).at(from).at(*CLASS_OF.at(b))
    }
}

/// The representation in force, chosen at compile time.
#[cfg(feature = "graph-by-byte")]
pub(crate) type Repr = ByByte;
#[cfg(feature = "graph-by-class")]
pub(crate) type Repr = ByClass;
#[cfg(not(any(feature = "graph-by-byte", feature = "graph-by-class")))]
pub(crate) type Repr = ByNode;

/// The graph itself, built at compile time -- one sub-graph per encoding.
///
/// Rodata with no initialiser and nothing to load. `Nodes<Bytes<Edge>>` is
/// the shape written out: an array per node, of an array per byte value, of
/// edges, which is `graph[node][byte]` said in types.
static UTF8_HALF: Nodes<Bytes<Edge>> = build(Half::Utf8);
static LATIN1_HALF: Nodes<Bytes<Edge>> = build(Half::Latin1);

#[inline(always)]
fn half_of(enc: InputEncoding) -> &'static Nodes<Bytes<Edge>> {
    match enc {
        InputEncoding::Utf8 => &UTF8_HALF,
        InputEncoding::Latin1 => &LATIN1_HALF,
    }
}

/// The same edges, transposed: `graph[byte][node]`.
static UTF8_FLIP: Bytes<Nodes<Edge>> = flip(&UTF8_HALF);
static LATIN1_FLIP: Bytes<Nodes<Edge>> = flip(&LATIN1_HALF);

#[inline(always)]
fn flipped_of(enc: InputEncoding) -> &'static Bytes<Nodes<Edge>> {
    match enc {
        InputEncoding::Utf8 => &UTF8_FLIP,
        InputEncoding::Latin1 => &LATIN1_FLIP,
    }
}

/// The same edges again, one column per byte *class*: `graph[node][class]`.
static UTF8_CLASSED: Nodes<Classes<Edge>> = classed(&UTF8_HALF);
static LATIN1_CLASSED: Nodes<Classes<Edge>> = classed(&LATIN1_HALF);

#[inline(always)]
fn classed_of(enc: InputEncoding) -> &'static Nodes<Classes<Edge>> {
    match enc {
        InputEncoding::Utf8 => &UTF8_CLASSED,
        InputEncoding::Latin1 => &LATIN1_CLASSED,
    }
}

/// Which class each byte falls in, **derived from the graph** rather than
/// written down: two bytes share a class when every node sends them to the
/// same edge, under either encoding. Deriving it is what makes it impossible
/// for the map and the graph to disagree -- the failure this repo records as
/// two spellings of one rule drifting apart.
pub(crate) static CLASS_OF: Bytes<Class> = classify();

const fn classify() -> Bytes<Class> {
    let mut map = [Class(0); 256];
    let mut reps = [0usize; NCLASS];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < 256 {
        let mut c = n;
        let mut r = 0usize;
        while r < n {
            let j = reps[r];
            let mut same = true;
            let mut k = 0usize;
            while k < NNODE {
                if !UTF8_HALF.0[k].0[i].same(UTF8_HALF.0[k].0[j])
                    || !LATIN1_HALF.0[k].0[i].same(LATIN1_HALF.0[k].0[j])
                {
                    same = false;
                    break;
                }
                k += 1;
            }
            if same {
                c = r;
                break;
            }
            r += 1;
        }
        if c == n {
            reps[n] = i;
            n += 1;
        }
        map[i] = Class(c as u8);
        i += 1;
    }
    Bytes(map)
}

const fn flip(src: &Nodes<Bytes<Edge>>) -> Bytes<Nodes<Edge>> {
    let mut t = [Nodes([edge(Act::Nothing, Node::Ground); NNODE]); 256];
    let mut n = 0usize;
    while n < NNODE {
        let mut b = 0usize;
        while b < 256 {
            t[b].0[n] = src.0[n].0[b];
            b += 1;
        }
        n += 1;
    }
    Bytes(t)
}

const fn classed(src: &Nodes<Bytes<Edge>>) -> Nodes<Classes<Edge>> {
    let mut t = [Classes([edge(Act::Nothing, Node::Ground); NCLASS]); NNODE];
    let mut n = 0usize;
    while n < NNODE {
        let mut b = 0usize;
        while b < 256 {
            t[n].0[CLASS_OF.0[b].0 as usize] = src.0[n].0[b];
            b += 1;
        }
        n += 1;
    }
    Nodes(t)
}

impl Edge {
    /// The edge one byte takes out of one node: the whole parser, as a lookup.
    ///
    /// Typed at the boundary on purpose. The first spelling took
    /// `(bool, Node, u8)` and returned a `usize` index the caller then had to
    /// use correctly -- three raw values in one signature, and the audit was
    /// right to refuse it: an encoding is not a `bool` and a stream byte is
    /// not a number.
    #[inline(always)]
    pub(crate) fn of(enc: InputEncoding, from: Node, b: StreamByte) -> Edge {
        <Repr as Edges>::edge(enc, from, b)
    }
    /// Equality, callable from a `const fn`, for the class derivation.
    ///
    /// `PartialEq` is derived and used everywhere else; a const fn cannot call
    /// it, and the class map has to be derived at build time or it is a second
    /// spelling of the graph.
    const fn same(self, other: Edge) -> bool {
        self.act.same(other.act) && self.next.same(other.next)
    }
}

/// Which half of the table an encoding selects.
///
/// The table is built for two encodings and `InputEncoding` has exactly two
/// variants, so this is a rename -- but it is the *table's* axis rather than
/// the terminal's mode, and naming it stops `latin1: bool` from reappearing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(crate) enum Half {
    Utf8 = 0,
    Latin1 = 1,
}

impl Half {
    /// Latin-1 gives `80..9F` to the C1 controls and `A0..FF` to graphics;
    /// UTF-8 gives both to the decoder. That is the only rule the halves
    /// differ by, and asking it by name keeps the builder from testing a flag.
    const fn is_latin1(self) -> bool {
        matches!(self, Half::Latin1)
    }
}

const fn build(half: Half) -> Nodes<Bytes<Edge>> {
    let mut t = [Bytes([edge(Act::Nothing, Node::Ground); 256]); NNODE];
    let mut s = 0usize;
    while s < NNODE {
        let st = Node::FROM_INDEX[s];
        let mut i = 0usize;
        while i < 256 {
            t[s].0[i] = one(half, st, StreamByte::ALL[i]);
            i += 1;
        }
        s += 1;
    }
    Nodes(t)
}

/// The whole protocol, as one function of three bounded values.
const fn one(half: Half, st: Node, b: StreamByte) -> Edge {
    // `anywhere`: CAN and SUB abandon a sequence in progress; ESC restarts one.
    // Ground-like states handle both themselves, so they fall through.
    // **Two different lists, and conflating them was four wrong cells.**
    // `CAN`/`SUB` abandon anything except the ground-like states, and the VT52
    // coordinate states count as in-progress. `ESC` restarts a sequence from
    // fewer states than that: the string forms keep it for themselves, because
    // there it is a terminator candidate rather than a fresh introducer, and
    // the VT52 coordinate bytes take it as data.
    let ground_like = matches!(
        st,
        Node::Ground | Node::Vt52Ground | Node::Utf8One | Node::Utf8Two | Node::Utf8Three
    );
    if (b.is(StreamByte::CAN) || b.is(StreamByte::SUB)) && !ground_like {
        return edge(Act::Cancel, ground_of(st));
    }

    let in_sequence = !matches!(
        st,
        Node::Ground
            | Node::Vt52Ground
            | Node::Utf8One
            | Node::Utf8Two
            | Node::Utf8Three
            | Node::Vt52Row
            | Node::Vt52Col
    );
    if b.is(StreamByte::ESC) {
        // The string states keep ESC for themselves: it is a terminator
        // candidate there, not a fresh sequence.
        let keeps_esc = matches!(
            st,
            Node::OscString
                | Node::OscEsc
                | Node::DcsPass
                | Node::DcsEsc
                | Node::DcsIgnore
                | Node::StringIgnore
        );
        if in_sequence && !keeps_esc {
            let esc = match st {
                Node::Vt52Escape => Node::Vt52Escape,
                _ => Node::Escape,
            };
            return edge(Act::Nothing, esc);
        }
    }
    match st {
        Node::Ground | Node::Vt52Ground => {
            if b.is(StreamByte::ESC) {
                let esc = match st {
                    Node::Vt52Ground => Node::Vt52Escape,
                    _ => Node::Escape,
                };
                edge(Act::Nothing, esc)
            } else if b.is_c0() || b.is(StreamByte::BEL) {
                edge(Act::Execute, st)
            } else if half.is_latin1() && b.is_c1() {
                edge(Act::C1Escape, Node::Escape)
            } else if half.is_latin1() && b.is_latin1_graphic() {
                edge(Act::PrintByte, st)
            } else if !half.is_latin1() && b.utf8_lead().is_some() {
                edge(Act::Utf8Start, utf8_state(b))
            } else {
                edge(Act::GroundUncommon, st)
            }
        }
        Node::Utf8One | Node::Utf8Two | Node::Utf8Three => {
            if b.utf8_continuation().is_some() {
                let next = match st {
                    Node::Utf8Three => Node::Utf8Two,
                    Node::Utf8Two => Node::Utf8One,
                    // The last one completes the character.
                    _ => Node::Ground,
                };
                edge(Act::Utf8Cont, next)
            } else {
                // Flush a replacement and re-dispatch the byte from Ground.
                edge(Act::Utf8Bad, Node::Ground)
            }
        }
        Node::Escape => {
            if b.is(StreamByte::CSI_INTRO) {
                edge(Act::CsiClear, Node::CsiEntry)
            } else if b.is(StreamByte::OSC_INTRO) {
                edge(Act::Nothing, Node::OscString)
            } else if b.is(StreamByte::DCS_INTRO) {
                edge(Act::DcsClear, Node::DcsEntry)
            } else if b.is(StreamByte::SOS_INTRO)
                || b.is(StreamByte::PM_INTRO)
                || b.is(StreamByte::APC_INTRO)
            {
                edge(Act::Nothing, Node::StringIgnore)
            } else if b.is_esc_intermediate() {
                edge(Act::EscCollect, Node::EscInter)
            } else {
                edge(Act::EscFe, Node::Ground)
            }
        }
        Node::EscInter => {
            if b.is_c0() {
                edge(Act::Execute, st)
            } else if b.is_esc_intermediate() {
                edge(Act::EscCollect, st)
            } else if b.is_del() {
                edge(Act::Nothing, st)
            } else {
                edge(Act::EscInterFinal, Node::Ground)
            }
        }
        Node::CsiEntry => {
            if b.is_c0() {
                edge(Act::Execute, st)
            } else if b.is_del() {
                edge(Act::Nothing, st)
            } else if Marker::from_private(b).is_some() {
                edge(Act::CsiPrivate, Node::CsiParams)
            } else {
                csi_param(b)
            }
        }
        Node::CsiParams => csi_param(b),
        Node::CsiInter => {
            if b.is_c0() {
                edge(Act::Execute, st)
            } else if FinalByte::from_stream(b).is_some() {
                edge(Act::CsiFinal, Node::Ground)
            } else if b.is_del() {
                edge(Act::Nothing, st)
            } else if !b.is_csi_intermediate() {
                edge(Act::Nothing, Node::CsiIgnore)
            } else {
                edge(Act::CsiCollect, st)
            }
        }
        Node::CsiIgnore => {
            if b.is_c0() {
                edge(Act::Execute, st)
            } else if FinalByte::from_stream(b).is_some() {
                edge(Act::Nothing, Node::Ground)
            } else {
                edge(Act::Nothing, st)
            }
        }
        Node::OscString => {
            if half.is_latin1() && b.is(StreamByte::ST_8BIT) {
                edge(Act::OscDispatchSt, Node::Ground)
            } else if b.is(StreamByte::BEL) {
                edge(Act::OscDispatchBel, Node::Ground)
            } else if b.is(StreamByte::ESC) {
                edge(Act::Nothing, Node::OscEsc)
            } else if b.is_c0() {
                edge(Act::Nothing, st)
            } else {
                edge(Act::StringPut, st)
            }
        }
        Node::OscEsc => {
            if b.is(StreamByte::ST_FINAL) {
                edge(Act::OscDispatchSt, Node::Ground)
            } else {
                edge(Act::OscCancel, Node::Escape)
            }
        }
        Node::DcsEntry => {
            if b.is_c0() || b.is_del() {
                edge(Act::Nothing, st)
            } else if Marker::from_private(b).is_some() {
                edge(Act::DcsPrivate, Node::DcsParam)
            } else {
                dcs_param(b)
            }
        }
        Node::DcsParam => dcs_param(b),
        Node::DcsInter => {
            if b.is_c0() || b.is_del() {
                edge(Act::Nothing, st)
            } else if b.is_csi_intermediate() {
                edge(Act::DcsCollect, st)
            } else if FinalByte::from_stream(b).is_some() {
                edge(Act::DcsHook, st)
            } else {
                edge(Act::Nothing, Node::DcsIgnore)
            }
        }
        Node::DcsPass => {
            if half.is_latin1() && b.is(StreamByte::ST_8BIT) {
                edge(Act::DcsDispatch, Node::Ground)
            } else if b.is(StreamByte::ESC) {
                edge(Act::Nothing, Node::DcsEsc)
            } else if b.is(StreamByte::BEL) {
                edge(Act::DcsDispatch, Node::Ground)
            } else if b.is_c0() {
                edge(Act::Nothing, st)
            } else {
                edge(Act::StringPut, st)
            }
        }
        Node::DcsIgnore => {
            if half.is_latin1() && b.is(StreamByte::ST_8BIT) {
                edge(Act::Nothing, Node::Ground)
            } else if b.is(StreamByte::ESC) {
                edge(Act::Nothing, Node::Escape)
            } else if b.is(StreamByte::BEL) {
                edge(Act::Nothing, Node::Ground)
            } else {
                edge(Act::Nothing, st)
            }
        }
        Node::DcsEsc => {
            if b.is(StreamByte::ST_FINAL) {
                edge(Act::DcsDispatch, Node::Ground)
            } else {
                edge(Act::DcsCancel, Node::Escape)
            }
        }
        Node::StringIgnore => {
            // `9C` terminates only outside UTF-8, where it is not a
            // continuation byte; `BEL` terminates in either encoding. Two
            // rules, one outcome.
            if (half.is_latin1() && b.is(StreamByte::ST_8BIT)) || b.is(StreamByte::BEL) {
                edge(Act::Nothing, Node::Ground)
            } else if b.is(StreamByte::ESC) {
                edge(Act::Nothing, Node::Escape)
            } else {
                edge(Act::Nothing, st)
            }
        }
        // VT52's whole escape table, and `ESC Y` is the only form with more
        // to collect -- `vt52_dispatch` sends the parser to `Vt52Row` itself,
        // because that is the one transition the *action* decides.
        Node::Vt52Escape => edge(Act::Vt52Dispatch, Node::Vt52Ground),
        Node::Vt52Row => edge(Act::Vt52SetRow, Node::Vt52Col),
        Node::Vt52Col => edge(Act::Vt52Address, Node::Vt52Ground),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InputEncoding, Storage, Term, TerminalMode};

    /// **The table must send every byte where the machine sends it.**
    ///
    /// Exhaustive over `(encoding, state, byte)` -- 10,240 transitions -- and
    /// exhaustive on purpose: a transition table is exactly the shape where a
    /// sampled check passes and one cell is wrong. Compares the state the
    /// machine lands in against the state the table names.
    #[test]
    fn the_table_sends_every_byte_where_the_machine_does() {
        // `no_std`: count them and keep the first, rather than collecting.
        let mut wrong = 0usize;
        let mut first: Option<(InputEncoding, Node, u8, Node, Node)> = None;
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for mode in [TerminalMode::Ansi, TerminalMode::Vt52] {
                for si in 0..NNODE {
                    let from = Node::FROM_INDEX[si];
                    // Each sub-graph is only reachable under its own protocol.
                    if (ground_of(from) == Node::Vt52Ground) != (mode == TerminalMode::Vt52) {
                        continue;
                    }
                    for raw in 0u8..=255 {
                        let b = crate::StreamByte::from_stream(raw);
                        let mut st = Storage::boxed();
                        let mut t = Term::new(&mut st, crate::OpMode::Computed);
                        t.terminal_mode = mode;
                        t.encoding = enc;
                        t.st = from;
                        t.control(b);
                        // A byte that *changes protocol* -- `ESC <` leaving VT52,
                        // `CSI ? 2 l` entering it -- crosses from one sub-graph to
                        // the other, and the crossing is the action's, not the
                        // cell's. `set_terminal_mode` puts the parser in the other
                        // ground after the table has named this one, which is the
                        // whole reason the table needs no `terminal_mode` index.
                        if t.terminal_mode != mode {
                            continue;
                        }
                        let want = t.st;
                        let entry = Edge::of(enc, from, b);
                        // Three actions re-enter the table with the same byte,
                        // mirroring `control`'s recursive call: a byte that cannot
                        // continue UTF-8, and a bare ESC cancelling either string
                        // form. `C1Escape` re-enters with a *different* byte and is
                        // checked separately.
                        let mut e = entry;
                        let mut hops = 0;
                        while hops < 2 {
                            if !matches!(e.act, Act::Utf8Bad | Act::OscCancel | Act::DcsCancel) {
                                break;
                            }
                            e = Edge::of(enc, e.next, b);
                            hops += 1;
                        }
                        let got = e.next;
                        // Three actions own their own transition, because where
                        // they go depends on something that is not the indexed
                        // byte: `C1Escape` re-enters with a *different* byte,
                        // `DcsHook` on the collected intermediate, `Vt52Dispatch`
                        // on which VT52 command it decoded.
                        // `Vt52Dispatch` joins them: `ESC Y` takes two coordinate
                        // bytes, and whether it does depends on which VT52 command
                        // arrived rather than on where the byte can go.
                        if matches!(entry.act, Act::C1Escape | Act::DcsHook | Act::Vt52Dispatch) {
                            continue;
                        }
                        if got != want {
                            wrong += 1;
                            if first.is_none() {
                                first = Some((enc, from, raw, want, got));
                            }
                        }
                    }
                }
            }
        }
        assert!(
            wrong == 0,
            "{wrong} transitions disagree; first \
             (encoding, from, byte, machine says, table says): {first:?}"
        );
    }
}

#[cfg(test)]
mod shape {
    use super::*;
    use crate::InputEncoding;

    /// **The graph's dimensions, pinned, because the representation follows
    /// from them.**
    ///
    /// `[[source: regex-automata-dense]]` names the two standard compressions
    /// of a transition table, and both are sized by numbers this asserts
    /// rather than guesses: how many nodes there are, and how many *classes*
    /// of byte the graph cannot tell apart. 256 columns of which only 20 are
    /// distinguishable is 92% of the table saying the same thing twice.
    ///
    /// It is a test rather than a comment because a new node or a new byte
    /// rule moves both numbers, and a representation chosen against the old
    /// ones is the "instrumentation outlived what fed it" failure.
    #[test]
    fn the_graph_is_the_size_the_representation_assumes() {
        for (enc, want) in [(InputEncoding::Utf8, 20), (InputEncoding::Latin1, 19)] {
            let mut reps = [0u8; 256];
            let mut n = 0usize;
            for b in 0u8..=255 {
                let bb = crate::StreamByte::from_stream(b);
                let mut seen = false;
                for r in reps.iter().take(n) {
                    let rb = crate::StreamByte::from_stream(*r);
                    if (0..NNODE).all(|si| {
                        let st = Node::FROM_INDEX[si];
                        Edge::of(enc, st, bb) == Edge::of(enc, st, rb)
                    }) {
                        seen = true;
                        break;
                    }
                }
                if !seen {
                    reps[n] = b;
                    n += 1;
                }
            }
            assert_eq!(n, want, "{enc:?} byte classes");
        }
        assert_eq!(NNODE, 23, "nodes");

        // The joint partition the single class map actually uses, and the one
        // `NCLASS` has to be: a byte shares a class only when every node
        // agrees about it under *both* encodings.
        let joint = CLASS_OF.0.iter().map(|c| c.0).max().unwrap() as usize + 1;
        assert_eq!(joint, 22, "joint byte classes");
        assert_eq!(NCLASS, joint, "NCLASS must equal the count, not exceed it");
    }

    /// **Every representation must answer identically, on every edge.**
    ///
    /// 11,776 edges each, checked against the dense form rather than against
    /// each other, so a shared mistake in two of them cannot pass. This is
    /// what makes the representation a compiled *choice*: a variant that only
    /// existed behind a feature nobody built would be the cfg'd-out test this
    /// repo already records going silently absent.
    #[test]
    fn the_representations_agree_on_every_edge() {
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for ni in 0..NNODE {
                let from = Node::FROM_INDEX[ni];
                for raw in 0u8..=255 {
                    let b = crate::StreamByte::from_stream(raw);
                    let want = ByNode::edge(enc, from, b);
                    assert_eq!(
                        ByByte::edge(enc, from, b),
                        want,
                        "flip {enc:?} {from:?} {raw:#04x}"
                    );
                    assert_eq!(
                        ByClass::edge(enc, from, b),
                        want,
                        "class {enc:?} {from:?} {raw:#04x}"
                    );
                }
            }
        }
    }

    /// Every action the table can name must be one some byte actually
    /// produces. An unreachable arm is the dead gate this repo keeps finding.
    #[test]
    fn every_action_occurs() {
        let mut seen: [Option<Act>; 32] = [None; 32];
        let mut n = 0usize;
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for si in 0..NNODE {
                for b in 0u8..=255 {
                    let e = Edge::of(enc, Node::FROM_INDEX[si], crate::StreamByte::from_stream(b));
                    if !seen.iter().flatten().any(|a| *a == e.act) {
                        seen[n] = Some(e.act);
                        n += 1;
                    }
                }
            }
        }
        assert_eq!(n, 31, "actions reached");
    }
}
