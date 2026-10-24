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
    /// After `ESC`, before any intermediate: `[[source: williams]]`'s
    /// `escape`, the one state that opens a `CSI` or a control string.
    /// **A protocol is more nodes, not a runtime flag** -- and
    /// until these existed `ESC` was a C0 control that did nothing and
    /// `[?1049h` was printable ASCII, so every escape sequence in the corpus
    /// was rendered to the screen as its own parameter bytes. Past an
    /// intermediate the parser is in `EscInter`.
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
    /// `sos/pm/apc string`, and their rows differ in eight bytes: `BEL`,
    /// the six format effectors `08-0D` and `DEL`.** They were one node, and
    /// so a `BEL` ended all five strings, where every text that speaks of it
    /// ends only the `OSC`: `[[source: ecma48]]` 5.6 closes a control string
    /// with `ST` alone, and williams puts the byte in a DCS and ignores it in
    /// the other three. `08-0D` and `7F` are the DCS body's own, below.
    OscStr,
    /// The body of a `DCS`, past its prefix: williams' `dcs passthrough`,
    /// whose row puts a `BEL` (388), so it is payload and the string goes on.
    /// It puts the format effectors `08-0D` and ignores `7F` too, the two
    /// classes every text agrees on (`string`).
    ///
    /// **It is williams' `dcs ignore` as well**, which this graph has no node
    /// for: a prefix williams sends there, `ESC P 1 ?` or `ESC P $ 1`, leads
    /// here at its final byte, through `DcsIntro`, and its body is put --
    /// `20-7E`, `BEL` and `08-0D` -- then discarded as `StrKind::Other`,
    /// where williams ignores every byte of it. Only `string_truncations` can
    /// see the difference, on such a string longer than the accumulator.
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
    /// `Ground`, after a control function: **the parser's record that a
    /// control went past, which ends the grapheme cluster before it**
    /// (`[[source: uax29]]` GB4 and GB5). Every edge that performs a control
    /// or ends a sequence or a string leads here rather than to `Ground`, and
    /// its row is `Ground`'s but for the bytes that would write a character
    /// through the cluster machine: those take `Act::EndCluster`, which tells
    /// the machine a control went past and hands the byte back to `Ground`.
    ///
    /// **A node, not a store in every control's arm.** `step` writes the node
    /// on every byte already, so which node a control leaves costs nothing
    /// per control; what it costs is one more trip round the loop where a
    /// control is followed by a character that is not printable ASCII, 16% of
    /// `vim`'s iterations, a `CUP` before each run of its two-byte text. A
    /// store of the machine's state in each control's arm, seventeen sites,
    /// measured `cat` 2.9% and `vim` 1.9% below the parent on the medians and
    /// this `cat` 1.5% above and `vim` 1.4% below, `vim`'s range clear of the
    /// parent's and `cat`'s overlapping (a second set read `vim` 1.9% below,
    /// its range clear, and `cat` between 2.9% below and 0.8% above, ranges
    /// overlapping, as `main`, unchanged, moved 64 bytes); doing the
    /// character's write in `EndCluster`'s own step, inlined or out of line,
    /// cost `cat` more (`docs/record.md`, "A control ends the grapheme
    /// cluster before it"). Printable ASCII leaves for
    /// `Ground` without the reset, because `Paint::piece` tells the machine
    /// what a run of it leaves; `CR` leads to `Ground` too, because the cursor
    /// is at column 0, where `Paint::restart_cluster` starts the machine again,
    /// and sending it here measured `cat` and `vim` lower.
    AfterControl,
    /// The same in VT52, whose commands and C0 controls end a cluster too.
    Vt52AfterControl,
    /// After `ESC` and one or more intermediates: `[[source: williams]]`'s
    /// `escape intermediate`, which collects `20-2F` and dispatches `30-7E`
    /// (224) and opens nothing.
    ///
    /// **Its own node, because `Escape` was both states and could not tell
    /// them apart.** `Escape` opens a `CSI` on `[`, a DCS on `P`, an `OSC`
    /// on `]` and the other strings on `X ^ _`, williams' edges out of
    /// `escape` alone (222, 226, 232, 233); after an intermediate the same
    /// bytes are finals. One node took them as introducers there too, so
    /// `ESC ( P` opened a DCS where williams designates a set, and where
    /// `fast_esc`, which reads `ESC <intermediate> <final>` whole, designated
    /// it: the same bytes did one thing in one read and the other split
    /// before the `P`. Last in the enum, so no other node's index moves.
    EscInter,
    /// **A split character owing the second byte of one of Table 3-7's four
    /// narrowed rows**, which only that row's second bytes continue:
    /// `[[source: unicode-ch3]]` lets `E0` go on with `A0..BF`, `ED` with
    /// `80..9F`, `F0` with `90..BF` and `F4` with `80..8F`, where every other
    /// lead takes any of `80..BF`. `Utf8Two` and `Utf8Three` took any of them
    /// after every lead, so `E0 | 80 80` cut between reads waited for the
    /// rest of an overlong and wrote one U+FFFD for three bytes, where 3.9.6
    /// replaces each maximal subpart and the `E0` is a subpart of its own.
    /// A row each, because which second bytes continue is the *lead's*
    /// property and the node is the only place the lead is remembered.
    Utf8TwoAfterE0,
    Utf8TwoAfterEd,
    Utf8ThreeAfterF0,
    Utf8ThreeAfterF4,
    /// The same four in VT52, which a split character has to resume in.
    Vt52Utf8TwoAfterE0,
    Vt52Utf8TwoAfterEd,
    Vt52Utf8ThreeAfterF0,
    Vt52Utf8ThreeAfterF4,
    /// **After a page-width request, under `WidthHold::On`, or a change of
    /// `?46` while logging is on: the read stops here.** Every byte is
    /// `Act::Hold`, which consumes the rest of the read and leaves it for the
    /// embedder to feed once it has resized or started or stopped its log
    /// (`Term::take_held`), and sets the node the request's sequence left
    /// -- the edge's `AfterControl` is overwritten, as a DECANM beside the
    /// request leaves VT52's. The request's dispatch puts the parser here --
    /// `?3` under `?40`, DECSCPP, `?46` -- so the loop's step and the `CSI` paths
    /// are the ones every sequence takes; a node costs one row of a table,
    /// where a stop tested after each dispatch costs the dispatch. Last in
    /// the enum, so no other node's index moves.
    Held,
    /// **A single shift pending**: after `ESC N`, `ESC O`, or `8E`/`8F`
    /// under ISO 8859-1, the next graphic character is drawn from G2 or G3
    /// (`Cold::single`). `[[source: dec-std-070]]` p. 3-81: the character,
    /// "which must be in the range 2/0 through 7/15 or 10/0 through 15/15, [is]
    /// rendered according to the current contents of the G3 set. If the next
    /// character in the received data stream is not a graphic character in
    /// this range, the single shift function is ignored and the received
    /// character is processed as if the single shift had not been received",
    /// SO and SI excepted, which act and keep it (Jev's lean STD070_CANCEL,
    /// and ANY_NUMBER of SO and SI, decided).
    ///
    /// **A node, so the text path tests nothing**: the shift lives in the
    /// parser's position, as VT52 and the UTF-8 continuations do, and a
    /// byte that cancels it is handed back to the ground with `Act::Again`.
    /// Last in the enum, so no other node's index moves.
    SingleShift,
    /// **A Sixel's body**, `DcsStr`'s row but for the bytes DEC's Sixel
    /// chapter gives their own meaning (`[[source: dec-std-070]]` 070-9
    /// §9.1 and §9.3): `SUB` is a blank sixel and the image goes on, where
    /// it ends every other string; `CAN` and, under ISO 8859-1, a C1 end it
    /// after placing what it received, and the C1 then runs. Its own node
    /// because `DcsStr` is every DCS's, and DECRQSS and the rest end on
    /// `SUB`. Last, with the next, so no other node's index moves.
    SixelStr,
    /// An `ESC` inside a Sixel's body: `\` ends it, as `StrEsc`; any other
    /// byte places what it received and is dispatched again from `Escape`
    /// (070-9 §9.1, "exit DCS mode processing, exit Sixel mode, begin
    /// processing escape sequence").
    SixelEsc,
    /// **A printer controller run**, `CSI 5 i` to `CSI 4 i` (`[[source:
    /// dec-std-070]]` 070-7 § 7.3.6): "all data received at the host port
    /// is transferred directly to the printer port without interpretation".
    /// Every byte is `Act::Printer`, which takes the rest of the read to the
    /// print job and watches for the sequence that ends the run
    /// (`Paint::printer_bytes`), so no byte here is parsed, the anywhere
    /// block's included. Last, so no other node's index moves.
    Printing,
    /// **A DCS introducer past an intermediate or a private marker**, up
    /// to its final byte: `[[source: williams]]`' `dcs intermediate`, and
    /// its `dcs ignore` for an introducer that broke the grammar. The byte
    /// that leads here from `Dcs` or `DcsDollar` is consumed, or read by
    /// `DcsQuery` for a `+`, as it was when it led to `DcsStr`. The `20-3F`
    /// bytes after it, and the final, are put to the body the way `DcsStr`
    /// puts them; a C0 is executed and the introducer goes on, `[[source:
    /// dec-std-070]]` 3.5.5 ("All other C0 controls are immediately
    /// recognized within escape, control, or control string introducer
    /// sequences"), where `DcsStr` would put it. Last, so no other node's
    /// index moves.
    DcsIntro,
}

pub(crate) const NNODE: usize = 39;

/// How many actions there are. Only `diag` reads it, to size the histogram
/// that says which of them a wider key would collapse.
#[cfg(feature = "diag")]
pub(crate) const NACT: usize = 47;

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
        Node::AfterControl,
        Node::Vt52AfterControl,
        Node::EscInter,
        Node::Utf8TwoAfterE0,
        Node::Utf8TwoAfterEd,
        Node::Utf8ThreeAfterF0,
        Node::Utf8ThreeAfterF4,
        Node::Vt52Utf8TwoAfterE0,
        Node::Vt52Utf8TwoAfterEd,
        Node::Vt52Utf8ThreeAfterF0,
        Node::Vt52Utf8ThreeAfterF4,
        Node::Held,
        Node::SingleShift,
        Node::SixelStr,
        Node::SixelEsc,
        Node::Printing,
        Node::DcsIntro,
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
    ///
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
    /// **`BEL`, and `SUB` in ANSI mode: the two C0 controls whose effect the
    /// embedder decides**, told apart by the byte in their out-of-line step.
    /// A `BEL` is counted, because the core cannot make a sound and an
    /// embedder can -- a `BEL` consumed silently is a protocol event the
    /// application has no way to learn was received.
    ///
    /// Distinct from the `BEL` inside a control string, which rings nothing:
    /// `StrEnd` in an `OSC`, which it terminates, `StrPut` in a `DCS` body
    /// and `Consume` in the `DCS` prefix and in an `SOS`, `PM` or `APC`. The
    /// node is what tells them apart.
    ///
    /// **A `SUB` writes the error character when the embedder turns the
    /// mark on** (`Term::set_substitute_mark`), and nothing otherwise, in the
    /// ground and as the `anywhere` edge that cancels a sequence or a
    /// control string other than a Sixel's body: `[[source: dec-std-070]]`
    /// p. 5-132, "placing a reversed question mark in the display at the
    /// Active Position", and note 2, the sequence "aborted and the
    /// Substitute control function to be executed instead". VT52 mode keeps
    /// `Consume` (Jev's lean, ANSI_MODE_ONLY, 0.76 and 0.84, which a re-ask
    /// split). **One action and not two**:
    /// a 48th, its own arm in the dispatch, retired 0.7% more instructions on
    /// `btop` and `cat`, which hold no `SUB`, and sharing this one, whose
    /// step was already as rare, retired none more.
    Signal,
    /// A control went past since the machine was last told anything: tell it
    /// so, and dispatch the byte again from the ground, consuming nothing.
    ///
    /// **Only `Node::AfterControl`'s row names it, and only for the bytes that
    /// write through the cluster machine**: a UTF-8 lead, and a graphic byte
    /// under ISO 8859-1. `[[source: uax29]]` GB4 and GB5 break after and before
    /// Control, CR and LF, and the terminal performs a control rather than
    /// writing it, so the machine never sees one; the node remembers it and
    /// this tells the machine, once, where it would be read. The other zero-
    /// width action, beside `Again` and `Resync`.
    EndCluster,
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
    /// The `q` of a `DCS $ q`: this control string is a DECRQSS; the `t`
    /// of a `DCS $ t`, a DECRSPS; the bare `p` of a `DCS Pn p`, ReGIS. The
    /// arm reads which byte it was.
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
    /// `Node::Held`'s every byte: the rest of the read is left for the
    /// embedder, and the step consumes all of it, so the loop ends.
    Hold,
    /// The character after a single shift, from G2 or G3 (`Node::SingleShift`).
    /// **May consume nothing**: `DEL` with a 94-character set in the slot is
    /// no character of it, so the shift is cancelled and the byte goes back to
    /// the ground, as `Again` would send it.
    Shifted,
    /// `DEL` in the ground: ignored with a 94-character set in GL, a
    /// character of the set with a 96-character one (`[[source: ecma35]]`
    /// 8.3.1).
    Del,
    /// VT, `0x0B`: the following line tabulation stop, or `LF` where none
    /// follows.
    LineTab,
    /// A Sixel cut off by `CAN`, an `ESC` that is not `ST` or a C1: what it
    /// received is placed, as at `ST`, and the byte is read again from the
    /// edge's node, which performs it. Consumes nothing. Last, so no other
    /// action's encoding moves.
    SixelCut,
    /// `Node::Printing`'s every byte: the rest of the read is a printer
    /// controller run's, up to the `CSI 4 i` that ends it.
    Printer,
    /// **A lead byte while iTerm2's `UnicodeVersion=8` is chosen**: the
    /// `Char2`, `Char3` and `Char4` of the graph [`Edge::of_graph`] reads
    /// then, whose step measures the characters by Unicode 8's widths. Its
    /// own width is one byte, so the per-step test that a character fits is
    /// the step's to make, and `width` reads as it did.
    Lead8,
    /// `ContEnd` under Unicode 8: the last byte of a character a read split.
    ContEnd8,
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
        Node::AfterControl => 22,
        Node::Vt52AfterControl => 23,
        Node::EscInter => 24,
        Node::Utf8TwoAfterE0 => 25,
        Node::Utf8TwoAfterEd => 26,
        Node::Utf8ThreeAfterF0 => 27,
        Node::Utf8ThreeAfterF4 => 28,
        Node::Vt52Utf8TwoAfterE0 => 29,
        Node::Vt52Utf8TwoAfterEd => 30,
        Node::Vt52Utf8ThreeAfterF0 => 31,
        Node::Vt52Utf8ThreeAfterF4 => 32,
        Node::Held => 33,
        Node::SingleShift => 34,
        Node::SixelStr => 35,
        Node::SixelEsc => 36,
        Node::Printing => 37,
        Node::DcsIntro => 38,
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
        Node::AfterControl => "AfterControl",
        Node::Vt52AfterControl => "Vt52AfterControl",
        Node::EscInter => "EscInter",
        Node::Utf8TwoAfterE0 => "Utf8TwoAfterE0",
        Node::Utf8TwoAfterEd => "Utf8TwoAfterEd",
        Node::Utf8ThreeAfterF0 => "Utf8ThreeAfterF0",
        Node::Utf8ThreeAfterF4 => "Utf8ThreeAfterF4",
        Node::Vt52Utf8TwoAfterE0 => "Vt52Utf8TwoAfterE0",
        Node::Vt52Utf8TwoAfterEd => "Vt52Utf8TwoAfterEd",
        Node::Vt52Utf8ThreeAfterF0 => "Vt52Utf8ThreeAfterF0",
        Node::Vt52Utf8ThreeAfterF4 => "Vt52Utf8ThreeAfterF4",
        Node::Held => "Held",
        Node::SingleShift => "SingleShift",
        Node::SixelStr => "SixelStr",
        Node::SixelEsc => "SixelEsc",
        Node::Printing => "Printing",
        Node::DcsIntro => "DcsIntro",
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
        Act::Signal => "Signal",
        Act::EndCluster => "EndCluster",
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
        Act::Hold => "Hold",
        Act::Shifted => "Shifted",
        Act::Del => "Del",
        Act::LineTab => "LineTab",
        Act::SixelCut => "SixelCut",
        Act::Printer => "Printer",
        Act::Lead8 => "Lead8",
        Act::ContEnd8 => "ContEnd8",
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
    Act::Signal,
    Act::EndCluster,
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
    Act::Hold,
    Act::Shifted,
    Act::Del,
    Act::LineTab,
    Act::SixelCut,
    Act::Printer,
    Act::Lead8,
    Act::ContEnd8,
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
        Act::Signal => 32,
        Act::Vt52Dispatch => 33,
        Act::Vt52SetRow => 34,
        Act::Vt52Address => 35,
        Act::DcsQuery => 36,
        Act::SixelStart => 37,
        Act::EndCluster => 38,
        Act::Hold => 39,
        Act::Shifted => 40,
        Act::Del => 41,
        Act::LineTab => 42,
        Act::SixelCut => 43,
        Act::Printer => 44,
        Act::Lead8 => 45,
        Act::ContEnd8 => 46,
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
/// 39 x 256 x 2 bytes = **19.5 KB of rodata per graph**, three graphs, no
/// initialiser and nothing to load. (Was 16.5 KB at 33 nodes, 2 KB when
/// there were 4 nodes, 10 KB at 20, before the string body split three
/// ways on `BEL`, 11 KB at 22,
/// before the two grounds after a control, 12 KB at 24, before `escape
/// intermediate` had a node, and 12.5 KB at 25, before Table 3-7's four
/// narrowed second bytes had a node each in both grounds; a row is read only
/// while the parser stands in its node, so `Node::AfterControl`'s 512 bytes
/// are read once per control, for the byte after it, `Node::EscInter`'s only
/// inside an escape sequence with an intermediate that `fast_esc` did not
/// take whole, and the narrowed nodes' only when a read ends straight after
/// an `E0`, `ED`, `F0` or `F4`.)
static UTF8: Nodes<Bytes<Edge>> = build(InputEncoding::Utf8);
static LATIN1: Nodes<Bytes<Edge>> = build(InputEncoding::Latin1);
static UTF8_EIGHT: Nodes<Bytes<Edge>> = eight(build(InputEncoding::Utf8));

/// The graph a step reads: the encoding's, and under iTerm2's
/// `UnicodeVersion=8` the UTF-8 graph whose character acts measure by
/// Unicode 8's widths (`[[source: iterm2-escape-codes]]`). **Chosen when
/// the encoding or the version changes, never by a step**, so the default
/// text path asks no version: the record, "Unicode 8's widths cost the
/// default nothing".
pub(crate) type Graph = Nodes<Bytes<Edge>>;

pub(crate) fn graph_of(enc: InputEncoding, widths: UnicodeWidths) -> &'static Graph {
    match (enc, widths) {
        (InputEncoding::Latin1, _) => &LATIN1,
        (InputEncoding::Utf8, UnicodeWidths::Current) => &UTF8,
        (InputEncoding::Utf8, UnicodeWidths::Eight) => &UTF8_EIGHT,
    }
}

/// `g` with every character act taken by its Unicode 8 twin: a lead's
/// `Char2`, `Char3` or `Char4` by `Lead8`, a split character's `ContEnd`
/// by `ContEnd8`. Every node and every other act is `g`'s.
const fn eight(mut g: Graph) -> Graph {
    let mut n = 0usize;
    while n < NNODE {
        let mut i = 0usize;
        while i < 256 {
            let e = g.0[n].0[i];
            g.0[n].0[i].act = match e.act {
                Act::Char2 | Act::Char3 | Act::Char4 => Act::Lead8,
                Act::ContEnd => Act::ContEnd8,
                a => a,
            };
            i += 1;
        }
        n += 1;
    }
    g
}

impl Edge {
    /// The edge one byte takes out of one node, in the encoding's graph
    /// under the current Unicode tables: what the tests ask. A step asks
    /// [`Edge::of_graph`], **the lookup**, in the graph [`graph_of`] chose.
    #[cfg(test)]
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

    /// The edge out of `from` on `b` in the graph `g`, which [`graph_of`]
    /// chose for the encoding and the Unicode version.
    #[inline(always)]
    pub(crate) fn of_graph(g: &Graph, from: Node, b: StreamByte) -> Edge {
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
/// other byte this parser eats and ignores. In a ground each one leads to
/// `Node::AfterControl` but `CR` (`one`), since a control ends a cluster
/// whether or not it moves anything.
const fn c0(b: StreamByte) -> Act {
    match b.raw_byte() {
        // VT has its own action: it reaches a line tabulation stop
        // (`Paint::vertical_tab`), and is `LF` only where none follows.
        0x0a | 0x0c => Act::Index,
        0x0b => Act::LineTab,
        0x0d => Act::Return,
        0x08 => Act::Backspace,
        0x09 => Act::Tab,
        0x0e => Act::ShiftOut,
        0x0f => Act::ShiftIn,
        0x07 | 0x1a => Act::Signal,
        _ => Act::Consume,
    }
}

/// The whole protocol, as one function of three bounded values, run at build
/// time and never at run time.
const fn one(enc: InputEncoding, node: Node, b: StreamByte) -> Edge {
    // A held read: every byte is the rest, the anywhere block's included.
    if matches!(node, Node::Held) {
        return edge(Act::Hold, Node::AfterControl);
    }
    // A printer controller run: every byte the printer's, `CAN`, `SUB` and
    // `ESC` included, which only its own step reads.
    if matches!(node, Node::Printing) {
        return edge(Act::Printer, Node::Printing);
    }
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
        Node::Ground | Node::AfterControl | Node::Vt52AfterControl | Node::Vt52Ground
    ) || mid_character(node);
    // **Back to the ground it was in, not to ANSI ground.** A `CAN` inside a
    // VT52 escape cancels the escape; it does not switch protocols, and
    // `ground_of` is what says so in one place rather than at each return.
    // **A Sixel's body is the exception, 070-9 §9.1's own rule**: `SUB` is
    // processed "as if a 3/15 code (blank Sixel) was received" and the image
    // goes on, and `CAN` exits Sixel mode, the sixels it received placed, as
    // williams' "Cancelling a control string does not undo these effects"
    // (501-503) has them. Jev decided BLANK_SIXEL (0.92/0.94) and leaned to
    // KEEP for `CAN` (0.72/0.68), 2026-10-19. After its `ESC`, either places
    // the image and cancels the escape sequence from `Escape`.
    if matches!(node, Node::SixelStr) {
        if b.is(StreamByte::SUB) {
            return edge(Act::StrPut, Node::SixelStr);
        }
        if b.is(StreamByte::CAN) {
            return edge(Act::SixelCut, Node::AfterControl);
        }
    }
    if matches!(node, Node::SixelEsc) && (b.is(StreamByte::CAN) || b.is(StreamByte::SUB)) {
        return edge(Act::SixelCut, Node::Escape);
    }
    if !ground_like && (b.is(StreamByte::CAN) || b.is(StreamByte::SUB)) {
        return edge(cancel(node, b), after_control_of(node));
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
        Node::Escape
            | Node::EscInter
            | Node::CsiEntry
            | Node::CsiParam
            | Node::CsiInter
            | Node::CsiIgnore
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
    // of the four, but a Sixel, whose body places what it received
    // (`Node::SixelStr`); whether the others should keep what they have
    // done, as williams' `unhook` and `osc_end` suggest, is open
    // (`docs/record.md`).
    //
    // Under UTF-8 `latin1_c1` answers `None` for every byte, so these nodes'
    // rows there are what they were: `80-9F` are continuation bytes of the
    // payload's characters (`a_latin1_c1_leaves_every_control_string_and_a_utf8_one_stays`).
    let in_string = matches!(
        node,
        Node::Dcs | Node::DcsDollar | Node::DcsIntro | Node::OscStr | Node::DcsStr | Node::SosStr
    );
    if in_string {
        if let Some(e) = latin1_c1(enc, b) {
            return match b.raw_byte() {
                0x9c => edge(Act::StrEnd, Node::AfterControl),
                _ => e,
            };
        }
    }
    // A C1 in a Sixel's body places what it received and is then read again
    // from the ground after a control, where it runs: 070-9 §9.3, "exit DCS
    // mode processing, exit Sixel mode, and the C1 control code is processed
    // outside of the Sixel mode" (Jev, KEEP, 0.94/0.94).
    if matches!(node, Node::SixelStr) && latin1_c1(enc, b).is_some() {
        return match b.raw_byte() {
            0x9c => edge(Act::StrEnd, Node::AfterControl),
            _ => edge(Act::SixelCut, Node::AfterControl),
        };
    }
    // **And from an escape or control sequence**, the rule's other half: the
    // same six nodes the `1B` edge above cancels from, `Escape` and
    // `EscInter` standing for williams' `escape` and `escape intermediate`,
    // whose intermediates `csi` holds. Of `csi entry` and the states after it williams says it
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
        // **Table 3-7's four narrowed second bytes**, asked of the domain's
        // table for the lead each node stands for (`narrowed_lead`): a byte
        // the lead admits continues the character into the node that owes
        // what is left, and any other byte, a continuation among them, ends
        // the maximal subpart at the lead -- one U+FFFD -- and is dispatched
        // again from the ground, where a continuation is a subpart of its own.
        Node::Utf8TwoAfterE0
        | Node::Utf8TwoAfterEd
        | Node::Utf8ThreeAfterF0
        | Node::Utf8ThreeAfterF4
        | Node::Vt52Utf8TwoAfterE0
        | Node::Vt52Utf8TwoAfterEd
        | Node::Vt52Utf8ThreeAfterF0
        | Node::Vt52Utf8ThreeAfterF4 => {
            return match narrowed_lead(node).admits_second(b) {
                true => edge(Act::Cont, after_second(node)),
                false => edge(Act::Resync, ground_of(node)),
            };
        }
        // **Under ISO 8859-1 a GR byte in an escape or control sequence is
        // read as its 7-bit byte**, in all six rows. `[[source: williams]]`
        // (462): "In all states, these codes are treated identically to GL
        // codes 20-7F", after X3.64's Appendix H, which lets GR represent
        // "parameters, intermediates, and finals of a control sequence";
        // `[[source: ecma48]]` §9 names the "Parameter Bytes, Intermediate
        // Bytes, and Final Bytes of a control sequence", "the same meanings"
        // as `02/00` to `07/14`; and `[[source: dec-std-070]]` 3.5.4.5 calls
        // its DCS rule "the same way 8-bit graphic characters are handled
        // within control sequences". xterm's `csi_table` gives `A0-FF` the
        // cases of `20-7F` (`[[source: xterm-vtprstbl-c]]` 679-798). `escape`
        // ended at a GR byte and dropped it, `csi_entry` and `csi_intermediate`
        // sent the sequence to `csi_ignore`, `csi_param` dropped the byte, and
        // `csi_ignore` waited for a 7-bit final. The DCS prefix took the rule
        // first and these rows delegate the same way, so `C0-FE` are whatever
        // `40-7E` are, `A0` an intermediate as `20` is and `FF` ignored as
        // `7F` is. Under UTF-8 `gr_as_gl` maps nothing and the rows are what
        // they were. The rule reaches each byte here after the `CAN`, `SUB`,
        // `ESC` and C1 edges above, none of which is a GR byte, and maps none
        // to a C0. The actions that read their byte, `Marker`, `Inter`,
        // `CsiFinal`, `EscFinal`, `StrStart` and `Digit`, read it through the
        // same rule (`write::act`). Under UTF-8 `escape` and `esc_inter` do
        // not drop a byte `80-FF` with the sequence: they cancel it and hand
        // the byte to the ground after a control (`escape`).
        Node::SingleShift => return single_shift(enc, b),
        Node::Escape => return escape(enc.gr_as_gl(b)),
        Node::EscInter => return esc_inter(enc.gr_as_gl(b)),
        Node::CsiEntry => return csi_entry(enc.gr_as_gl(b)),
        Node::CsiParam => return csi_param(enc.gr_as_gl(b)),
        Node::CsiInter => return csi_inter(enc.gr_as_gl(b)),
        Node::CsiIgnore => return csi_ignore(enc.gr_as_gl(b)),
        // **Both rows are `Act::Consume` with a different next node**, which
        // is the cheapest kind of edge this graph has: it costs table content
        // and nothing in the dispatch, the way `CAN`/`SUB` did. Anything that
        // is not the DECRQSS prefix falls to `DcsStr`, where the body is still
        // consumed to its terminator and then ignored -- consumed is the half
        // that matters, because a DCS body reaching the screen would render an
        // application's private protocol as glyphs.
        // **A C0 in the prefix is executed, a `DEL` is ignored, and the
        // prefix survives both.** Both of these sent anything unrecognised to
        // the body, so a `DEL` between `ESC P` and the `$` turned a `DECRQSS`
        // into a generic control string -- the request was dropped and the
        // application got no reply, which for a *query* is the worst of the
        // three outcomes.
        //
        // `[[source: williams]]`'s `dcs_entry`, `dcs_param` and
        // `dcs_intermediate` all give `00-17`, `19`, `1C-1F` and `7F` the
        // *ignore* action (354, 361, 368): the byte goes nowhere and the state
        // does not move. `[[source: dec-std-070]]` 3.5.5 executes the C0
        // and the rows follow it (below); `DEL` stays ignored. `18` and `1A`
        // are absent because the `anywhere` rule takes them first -- they
        // cancel.
        //
        // **`BEL` is one of them.** Both rows ended the string on it, an
        // `OSC`'s terminator (`[[source: ctlseqs]]` 2023-2024) where no text
        // puts one, so `ESC P BEL $ q m ST` lost its reply and drew `$qm`.
        //
        // **Under ISO 8859-1 a GR byte is read as its 7-bit byte, in both
        // rows.** `[[source: dec-std-070]]` 3.5.4.5: GR graphic characters
        // "in the introducer sequence to a Device Control String, the eighth
        // bit will be ignored, and they will be treated as their 7-bit
        // equivalent"; `[[source: williams]]` (462): "In all states, these
        // codes are treated identically to GL codes 20-7F"; and
        // `[[source: ecma48]]` §9 gives `10/00` to `15/14` in a control
        // string "the same meanings" as `02/00` to `07/14`. xterm agrees in
        // practice, keeping `AsciiOf` of each string byte (`[[source:
        // xterm-reset]]`'s file, 3454-3456) and finding the `$ q` there.
        // Both rows sent every GR byte to the body, so `ESC P A4 F1 m ST`
        // was an ignored string and not a DECRQSS. `FF`, which §9 stops
        // short of and williams reads as `7F`, is ignored where `7F` is, and
        // `A0` leads to the body as `20` does. Under UTF-8 `gr_as_gl` maps
        // nothing: the bytes are a character's, and these rows are what they
        // were. The body keeps a GR byte as it is, the owner's call
        // (`dcs_passthrough`, `docs/vt-coverage.md`).
        //
        // **The rows delegate rather than respell the ranges**, as
        // `latin1_c1` does, so `B0-B9` are whatever `30-39` are. The one
        // action that reads its byte is `Digit`, and the parameter table
        // gives a GR digit's lead the row its GL digit's has.
        Node::Dcs => {
            return match enc.gr_as_gl(b).raw_byte() {
                // **`$` is kept as the intermediate**, so a `p` after it is
                // DECRSTS's and not ReGIS's (`Paint::dcs_start`).
                b'$' => edge(Act::Inter, Node::DcsDollar),
                // A bare `q` (no `$` before it) is a Sixel: rename the string
                // kind and route the body through the `StrPut`/`StrEnd` path,
                // which now decodes it rather than dropping it.
                b'q' => edge(Act::SixelStart, Node::SixelStr),
                // **A bare `p` is ReGIS**, `DCS Pn p` (`[[source: dec-std-070]]`
                // 070-8 § 8.9.1), after the same numeric parameters, so
                // `DCS $ p` and `DCS + p` keep their rows. The action is
                // DECRQSS's, which reads its byte: no new action, so no other
                // action's encoding moves. The body is a DCS's.
                b'p' => edge(Act::DcsQuery, Node::DcsStr),
                // **`+` is XTGETTCAP's and XTSETTCAP's**, `DCS + q Pt ST`
                // and `DCS + p Pt ST` (`[[source: ctlseqs]]` 535-582). The
                // action is DECRQSS's, which reads its byte, so no action's
                // encoding moves; the `q` or `p` after it is the body's
                // first byte, read at the terminator (`Paint::tcap_end`).
                // After a parameter it is no XTGETTCAP, as xterm's `do_dcs`
                // switches on the first byte (`[[source: xterm-misc]]`).
                b'+' => edge(Act::DcsQuery, Node::DcsIntro),
                // **A Sixel is `DCS Pa;Pb;Ph q` -- a `q` after *numeric
                // parameters* and no intermediate.** So the parameters `30-3F`
                // (`0-9 : ; < = > ?`) collect and stay in `Node::Dcs`, and a
                // `q` reached from here is a Sixel. The earlier catch-all sent
                // the first digit to the passthrough body, so `q` never arrived
                // and the image was never recognised.
                //
                // **Only the numeric parameters `30-39` and `3B` (`0-9 ;`) collect,
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
                //
                // **`3A` ignores the string**, as `[[source: williams]]`'s
                // `dcs ignore` does, "the character 3A appears anywhere", and
                // `[[source: dec-std-070]]` 3.5.4.1 with 3.5.3.1, "If 3/10 is
                // received within a parameter string, the entire sequence ...
                // shall be ignored": it goes to the body unread, discarded at
                // `ST` and counted, as a marker after a parameter does. It
                // separated as `;` did until 2026-10-19; Jev leaned to keeping
                // that at 0.85/0.71 once a CSI ignored a colon, below the bar
                // for a departure.
                0x30..=0x39 => edge(Act::Digit, Node::Dcs),
                0x3b => edge(Act::Sep, Node::Dcs),
                0x1b => edge(Act::Consume, Node::StrEsc),
                // **A C0 in the introducer is executed and the introducer
                // goes on**, as in a `CSI`: `[[source: dec-std-070]]` 3.5.5,
                // "All other C0 controls are immediately recognized within
                // escape, control, or control string introducer sequences.
                // The escape, control, or control string introducer sequence
                // continues where it left off" (PDF p. 144), and "DCS
                // introducer sequences are parsed according to the rules for
                // control sequences" (p. 145). `[[source: williams]]` ignores
                // them (565), describing the VT500, and xterm too, practice.
                // Jev leaned to executing, 0.74/0.70 (2026-10-23), below the
                // bar, taken as the lean: it is the standard's reading. They
                // were ignored until then, so `DCS 1 LF $ q m` moved nothing.
                0x00..=0x17 | 0x19 | 0x1c..=0x1f => edge(c0(b), Node::Dcs),
                0x7f => edge(Act::Consume, Node::Dcs),
                // **An intermediate, a private marker or a `:` leads to
                // `DcsIntro`**, where the introducer goes on to its final
                // byte and a C0 is still executed; a final leads to the body.
                0x20..=0x3f => edge(Act::Consume, Node::DcsIntro),
                _ => edge(Act::Consume, Node::DcsStr),
            };
        }
        // **`$ t` is DECRSPS**, `DCS Ps $ t D...D ST` (`[[source:
        // dec-std-070]]` p. 5-205), its `Ps` collected by `Dcs`'s digit
        // edges, **`$ w` DECLKD**, `DCS Pc $ w ... ST` (070-13 § 5.4), and
        // **`$ p` DECRSTS**, `DCS Ps $ p D...D ST` (p. 5-208); `DcsQuery`
        // reads which of the four the byte names.
        Node::DcsDollar => {
            return match enc.gr_as_gl(b).raw_byte() {
                b'q' | b't' | b'w' | b'p' => edge(Act::DcsQuery, Node::DcsStr),
                0x1b => edge(Act::Consume, Node::StrEsc),
                // Executed, as in `Node::Dcs`.
                0x00..=0x17 | 0x19 | 0x1c..=0x1f => edge(c0(b), Node::DcsDollar),
                0x7f => edge(Act::Consume, Node::DcsDollar),
                0x20..=0x3f => edge(Act::Consume, Node::DcsIntro),
                _ => edge(Act::Consume, Node::DcsStr),
            };
        }
        // **`DcsStr`'s row, but a C0 is executed and `20-3F` keep the
        // introducer going**: each byte is put as the body would put it, so
        // what a handler reads is what it read when these led straight to
        // the body, and only the C0 differs. `DEL` is ignored in place, as
        // in `Dcs`. Under ISO 8859-1 a GR byte is read as its 7-bit byte to
        // find the final, as in `Dcs`, and so `FF` is ignored as `DEL` is.
        Node::DcsIntro => {
            let e = string(Node::DcsStr, b);
            return match enc.gr_as_gl(b).raw_byte() {
                0x00..=0x17 | 0x19 | 0x1c..=0x1f => edge(c0(b), Node::DcsIntro),
                0x20..=0x3f => edge(e.act, Node::DcsIntro),
                0x7f => edge(Act::Consume, Node::DcsIntro),
                _ => e,
            };
        }
        Node::Vt52Escape | Node::Vt52Row | Node::Vt52Col => return vt52(enc, node, b),
        Node::OscStr | Node::DcsStr | Node::SosStr => return string(node, b),
        // `[[source: williams]]`: `ESC \` is the *only* string terminator, and
        // a bare `ESC` cancels the string rather than continuing it. Code that
        // assumed the byte after `ESC` was the backslash swallowed a `NEL`.
        //
        // **A `DEL` after that `ESC` is ignored and the node stays**, so
        // `ESC DEL \` is the `ST` it would be without it: this node is
        // williams' `escape` reached from a string, which ignores `7F` (286).
        // It went to `Escape` with `Again`, which ended the string there and
        // took the `\` for a bare `ST`.
        //
        // **And under ISO 8859-1 `DC` is the `\`, `FF` the `DEL`**, the
        // escape state's GR rule (above), so `ESC DC` ends the string as
        // `ESC \` does. It led to `Escape`, which cancelled the string.
        Node::StrEsc => {
            return match enc.gr_as_gl(b).raw_byte() {
                0x5c => edge(Act::StrEnd, Node::AfterControl),
                0x7f => edge(Act::Consume, Node::StrEsc),
                _ => edge(Act::Again, Node::Escape),
            };
        }
        // As `StrEsc`, but a byte that is not `\` places the Sixel first
        // (`Node::SixelEsc`; Jev, KEEP, 0.94/0.94).
        Node::SixelEsc => {
            return match enc.gr_as_gl(b).raw_byte() {
                0x5c => edge(Act::StrEnd, Node::AfterControl),
                0x7f => edge(Act::Consume, Node::SixelEsc),
                _ => edge(Act::SixelCut, Node::Escape),
            };
        }
        // `DcsStr`'s row, its `ESC` to `SixelEsc` and its own node kept.
        Node::SixelStr => {
            let e = string(Node::DcsStr, b);
            return match (e.act, e.next) {
                (_, Node::StrEsc) => edge(Act::Consume, Node::SixelEsc),
                (a, Node::DcsStr) => edge(a, Node::SixelStr),
                _ => e,
            };
        }
        Node::AfterControl | Node::Vt52AfterControl => return after_control(enc, node, b),
        // Taken at the top, before the anywhere block.
        Node::Held => return edge(Act::Hold, Node::AfterControl),
        Node::Printing => return edge(Act::Printer, Node::Printing),
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
    //
    // **A C0 control leads to `Node::AfterControl`, and `CR` to the ground.**
    // Every control ends the grapheme cluster before it, and the node is what
    // says so (`Node::AfterControl`). `CR` leaves the cursor at column 0, where
    // `Paint::glyph` writes the next character in a cell of its own whatever the
    // machine says and starts the machine again if it said otherwise
    // (`Paint::restart_cluster`); a `CR LF`, which the `Return` arm folds,
    // leaves it there too. So the line ending every line-oriented stream sends
    // never visits the twin row. Under DECOM a `CR` goes to the left margin
    // instead, and there its arm tells the machine itself.
    if matches!(Route::of(b), Route::Control) {
        return match (c0(b), g) {
            (Act::Return, _) => edge(Act::Return, g),
            (Act::Signal, Node::Vt52Ground) if b.is(StreamByte::SUB) => {
                edge(Act::Consume, after_control_of(g))
            }
            (a, _) => edge(a, after_control_of(g)),
        };
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
    // makes 0x7F a character of the set (`[[source: ecma35]]` 8.3.1), which
    // is GL's to say at run time, so the ANSI grounds name `Act::Del`.
    if matches!(Route::of(b), Route::Del) {
        return match g {
            Node::Vt52Ground => edge(Act::Consume, after_control_of(g)),
            _ => edge(Act::Del, after_control_of(g)),
        };
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
        // `8E` and `8F` leave a single shift pending in the ANSI grounds; a
        // VT52 ground has no G2 or G3 to shift to, and takes them as every
        // other C1 there (note AE's D3).
        if let Some(e) = latin1_c1(enc, b) {
            return match (g, e.next) {
                (Node::Vt52Ground, Node::SingleShift) => edge(e.act, Node::AfterControl),
                _ => e,
            };
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
        // designation land on the same arm as its final byte alone. And into
        // `EscInter`, williams' `escape intermediate`, where the introducers
        // above are finals (`esc_inter`).
        0x20..=0x2f => edge(Act::Inter, Node::EscInter),
        // **SS2 and SS3 leave a single shift pending**, from this row only:
        // after an intermediate `N` and `O` are finals (`esc_inter`), so
        // `ESC SP N` stays what it is at every read split. The C1 `8E` and
        // `8F` read this row too (`latin1_c1`).
        0x4e | 0x4f => edge(Act::EscFinal, Node::SingleShift),
        0x30..=0x7e => edge(Act::EscFinal, Node::AfterControl),
        // **`DEL` is ignored and the sequence goes on.** `[[source:
        // williams]]` gives `7F` the *ignore* action in `escape` (286). It
        // fell to the arm below, which ended the sequence, so `ESC DEL 7`
        // drew the `7` and lost the DECSC.
        0x7f => edge(Act::Consume, Node::Escape),
        // **Under UTF-8 a byte `80-FF` cancels the sequence and is the
        // ground's**, and they are the only bytes that reach this arm: under
        // ISO 8859-1 `one()` takes `80-9F` as C1 controls first and `gr_as_gl`
        // reads `A0-FF` as `20-7F`. The edge consumes nothing and leads to the
        // ground after a control, whose decoder takes the byte as any byte
        // there: a well-formed character is written as the ground writes one
        // after any control, so U+0903 takes a cell of its own
        // (`[[source: uax29]]` GB4) and a zero-width mark attaches where it
        // does after any control (note AB's Q24), and a stray continuation, a
        // disallowed lead byte (0xC0, 0xC1, 0xF5-0xFF) or a lead cut off writes
        // U+FFFD. It was `Consume`, which dropped the lead with the sequence,
        // so each continuation of a well-formed U+0903 in `a ESC E0 A4 83`
        // wrote a U+FFFD. Read as one UTF-8 conversion over the whole stream,
        // `[[source: unicode-ch3]]` 3.9.5 rules that out: a converter "must
        // not consume the successor bytes as part of the ill-formed
        // subsequence whenever those successor bytes themselves constitute
        // part of a well-formed UTF-8 code unit subsequence". Read with the
        // bytes inside a sequence belonging to the sequence and not to the
        // text, it says nothing of them. That
        // the character is then written, and not dropped with the sequence, is
        // Jev's CANCEL_AND_PRINT (decisions.jsonl lines 451-452, 0.96 and 0.92;
        // the owner can veto): no text read says what a character after `ESC`
        // does. `StrEsc` hands its byte here with `Again`, so after the `ESC`
        // inside a string the same byte cancels the string and is the ground's.
        _ => edge(Act::Again, Node::AfterControl),
    }
}

/// After a single shift: the character it applies to, or a byte that
/// cancels it (`Node::SingleShift`).
///
/// `20-7F` are 070's range, and under ISO 8859-1 `A0-FF` too, stripped to
/// seven bits by the act (`[[source: dec-std-070]]` p. 3-81 and 070-5 p.
/// 5-140, `code = parv[0] & 0x7F`; `[[source: ecma48]]` §9; Jev's lean
/// STRIP_BIT8). SO and SI act and the shift stays pending. Every other
/// byte -- a C0, `ESC`, `CAN`, `SUB`, a C1, a UTF-8 lead -- cancels it and is
/// dispatched again from the ground after a control.
const fn single_shift(enc: InputEncoding, b: StreamByte) -> Edge {
    match (enc, b.raw_byte()) {
        (_, 0x0e) => edge(Act::ShiftOut, Node::SingleShift),
        (_, 0x0f) => edge(Act::ShiftIn, Node::SingleShift),
        (_, 0x20..=0x7f) | (InputEncoding::Latin1, 0xa0..=0xff) => edge(Act::Shifted, Node::Ground),
        _ => edge(Act::Again, Node::AfterControl),
    }
}

/// Past `ESC` and an intermediate: `[[source: williams]]`'s `escape
/// intermediate` (293), which executes `00-17,19,1C-1F`, collects `20-2F`,
/// ignores `7F`, and dispatches `30-7E` (224) -- **all of them, `P X [ ] ^ _`
/// among them**, which open a string or a `CSI` only from `escape`. `Escape`
/// was this row too, so `ESC ( P` opened a DCS split before the `P` and
/// designated read whole, through `fast_esc`; and `ESC ( %` then `P` opened
/// one in any read. Under UTF-8 a byte `80-FF` ends the sequence and is the
/// ground's after a control, as in `escape`.
const fn esc_inter(b: StreamByte) -> Edge {
    if b.is_c0() {
        // As in `escape`: every C0 but `ESC`, `CAN` and `SUB`, which `one()`
        // takes first because they end the sequence.
        return edge(c0(b), Node::EscInter);
    }
    match b.raw_byte() {
        0x20..=0x2f => edge(Act::Inter, Node::EscInter),
        0x30..=0x7e => edge(Act::EscFinal, Node::AfterControl),
        // Ignored, and the sequence goes on (293): `ESC ( DEL 0` designates.
        0x7f => edge(Act::Consume, Node::EscInter),
        // Under UTF-8, `80-FF`: cancelled, and the byte is the ground's after
        // a control, as in `escape` (Jev's CANCEL_AND_PRINT). It was
        // `Consume`, so `a ESC ( E0 A4 83` wrote two U+FFFD.
        _ => edge(Act::Again, Node::AfterControl),
    }
}

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
        0x40..=0x7e => edge(Act::CsiFinal, Node::AfterControl),
        // **`DEL` is ignored here, and a marker is still legal after it**:
        // `[[source: williams]]`'s `csi entry` ignores `7F` and stays (306).
        // It sent the sequence to `csi_ignore`, so `CSI DEL 5 C` moved
        // nothing.
        0x7f => edge(Act::Consume, Node::CsiEntry),
        // **Under UTF-8 a byte `80-FF` cancels the sequence and is the
        // ground's after a control**, in this row and the three after it, as
        // after `ESC` (`escape`): the owner's decision of 2026-10-13, "cancel
        // and print it". Only those bytes reach here, as in `escape`: under
        // ISO 8859-1 `one()` takes `80-9F` as C1 controls and `gr_as_gl`
        // reads `A0-FF` as `20-7F`. Each row dropped the byte: this one and
        // `csi_inter` sent the sequence to `csi_ignore`, `csi_param` stayed
        // and `csi_ignore` stayed, so the continuations of a character whose
        // lead was dropped each wrote U+FFFD, or were taken as the sequence
        // went on: `a CSI 1 E0 A4 83 b` ran REP and wrote the `a` twice, and
        // `a CSI E0 A4 83 b` took the `b` as an ignored final
        // (`a_character_inside_a_csi_is_written_and_the_csi_is_cancelled`).
        0x80..=0xff => edge(Act::Again, Node::AfterControl),
        _ => edge(Act::Consume, Node::CsiIgnore),
    }
}

/// Inside `CSI`, before any intermediate.
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
        0x40..=0x7e => edge(Act::CsiFinal, Node::AfterControl),
        // Under UTF-8, cancelled, and the byte is the ground's (`csi_entry`).
        0x80..=0xff => edge(Act::Again, Node::AfterControl),
        // `DEL`, which `[[source: williams]]`'s `csi param` ignores (315).
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
        0x40..=0x7e => edge(Act::CsiFinal, Node::AfterControl),
        // `[[source: williams]]`'s `csi intermediate` ignores `7F` and stays
        // (328). It sent the sequence to `csi_ignore`, so `CSI ! DEL p` was
        // no DECSTR.
        0x7f => edge(Act::Consume, Node::CsiInter),
        // Under UTF-8, cancelled, and the byte is the ground's (`csi_entry`).
        0x80..=0xff => edge(Act::Again, Node::AfterControl),
        _ => edge(Act::Consume, Node::CsiIgnore),
    }
}

/// A `CSI` that broke its grammar: swallow everything up to the final.
const fn csi_ignore(b: StreamByte) -> Edge {
    if b.is_c0() {
        return edge(c0(b), Node::CsiIgnore);
    }
    match b.raw_byte() {
        // **Still a control sequence, and it ends the cluster before it**:
        // nothing is performed, and the sequence is a control all the same.
        0x40..=0x7e => edge(Act::Consume, Node::AfterControl),
        // Under UTF-8, cancelled, and the byte is the ground's (`csi_entry`).
        0x80..=0xff => edge(Act::Again, Node::AfterControl),
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
        | Node::Vt52Utf8Three
        | Node::Vt52Utf8TwoAfterE0
        | Node::Vt52Utf8TwoAfterEd
        | Node::Vt52Utf8ThreeAfterF0
        | Node::Vt52Utf8ThreeAfterF4
        | Node::Vt52AfterControl => Node::Vt52Ground,
        _ => Node::Ground,
    }
}

/// The nodes a character split across reads waits in, owing continuations:
/// the ones a byte that cannot continue it leaves with `Act::Resync`.
pub(crate) const fn mid_character(node: Node) -> bool {
    matches!(
        node,
        Node::Utf8One
            | Node::Utf8Two
            | Node::Utf8Three
            | Node::Utf8TwoAfterE0
            | Node::Utf8TwoAfterEd
            | Node::Utf8ThreeAfterF0
            | Node::Utf8ThreeAfterF4
            | Node::Vt52Utf8One
            | Node::Vt52Utf8Two
            | Node::Vt52Utf8Three
            | Node::Vt52Utf8TwoAfterE0
            | Node::Vt52Utf8TwoAfterEd
            | Node::Vt52Utf8ThreeAfterF0
            | Node::Vt52Utf8ThreeAfterF4
    )
}

/// The lead a node owing a narrowed second byte waits after: Table 3-7's
/// four rows, `E0`, `ED`, `F0` and `F4`, and nothing for any other node.
/// `waits_for_second` (through `waits_after`) is the other direction, and
/// `a_split_lead_waits_for_the_second_bytes_it_admits` holds the two together
/// against the domain's table for every lead and every byte after it.
const fn narrowed_lead(node: Node) -> StreamByte {
    StreamByte::from_stream(match node {
        Node::Utf8TwoAfterE0 | Node::Vt52Utf8TwoAfterE0 => 0xe0,
        Node::Utf8TwoAfterEd | Node::Vt52Utf8TwoAfterEd => 0xed,
        Node::Utf8ThreeAfterF0 | Node::Vt52Utf8ThreeAfterF0 => 0xf0,
        Node::Utf8ThreeAfterF4 | Node::Vt52Utf8ThreeAfterF4 => 0xf4,
        // Leads nothing, so admits nothing: unreachable from the rows above.
        _ => 0x00,
    })
}

/// Where a narrowed node goes once its second byte has come: owing what any
/// lead of its length owes after two bytes.
const fn after_second(node: Node) -> Node {
    match node {
        Node::Utf8TwoAfterE0 | Node::Utf8TwoAfterEd => Node::Utf8One,
        Node::Utf8ThreeAfterF0 | Node::Utf8ThreeAfterF4 => Node::Utf8Two,
        Node::Vt52Utf8TwoAfterE0 | Node::Vt52Utf8TwoAfterEd => Node::Vt52Utf8One,
        _ => Node::Vt52Utf8Two,
    }
}

/// The node a lead whose bytes the read does not all carry waits in, after
/// the lead: the narrowed row for `E0`, `ED`, `F0` and `F4`, and otherwise the
/// node owing its length's continuations, in the ground `g` it began in.
const fn waits_for_second(g: Node, lead: StreamByte) -> Node {
    let vt52 = matches!(g, Node::Vt52Ground);
    match (vt52, lead.raw_byte()) {
        (false, 0xe0) => Node::Utf8TwoAfterE0,
        (false, 0xed) => Node::Utf8TwoAfterEd,
        (false, 0xf0) => Node::Utf8ThreeAfterF0,
        (false, 0xf4) => Node::Utf8ThreeAfterF4,
        (true, 0xe0) => Node::Vt52Utf8TwoAfterE0,
        (true, 0xed) => Node::Vt52Utf8TwoAfterEd,
        (true, 0xf0) => Node::Vt52Utf8ThreeAfterF0,
        (true, 0xf4) => Node::Vt52Utf8ThreeAfterF4,
        (false, 0xc2..=0xdf) => Node::Utf8One,
        (false, 0xe1..=0xef) => Node::Utf8Two,
        (false, _) => Node::Utf8Three,
        (true, 0xc2..=0xdf) => Node::Vt52Utf8One,
        (true, 0xe1..=0xef) => Node::Vt52Utf8Two,
        (true, _) => Node::Vt52Utf8Three,
    }
}

/// **What `CAN` or `SUB` does to the sequence or string it cancels**:
/// `CAN` consumes itself and nothing more ("No error character is
/// displayed", `[[source: vt220-rm-ch4]]`); `SUB` in ANSI mode performs
/// [`Act::Signal`], DEC STD 070's "the Substitute control function to
/// be executed instead" (p. 5-132, note 2); in VT52 mode it only cancels.
const fn cancel(node: Node, b: StreamByte) -> Act {
    match (b.raw_byte(), ground_of(node)) {
        (0x1a, Node::Ground) => Act::Signal,
        _ => Act::Consume,
    }
}

/// The ground a node returns to when a control ends what it was in, and so
/// the cluster with it: `Node::AfterControl`, or its VT52 twin.
pub(crate) const fn after_control_of(node: Node) -> Node {
    match ground_of(node) {
        Node::Vt52Ground => Node::Vt52AfterControl,
        _ => Node::AfterControl,
    }
}

/// `Node::AfterControl`'s row, and its VT52 twin's: **the ground's row, but
/// for the bytes that write through the cluster machine.**
///
/// A UTF-8 lead and an ISO 8859-1 graphic take `Act::EndCluster` back to the
/// ground, which tells the machine a control went past and dispatches the
/// byte again from there, so the character is decoded and written by the
/// ground's own edge and the machine breaks before it (GB4, GB5). A byte
/// that is no character, `Act::Bad`, does the same: the replacement it
/// writes, `U+FFFD`, goes through the machine as a literal one does (the
/// owner's SAME_AS_LITERAL, 2026-10-13), so the machine must know a control
/// went past, or after `a U+0600 BEL` the U+FFFD joins the Prepend
/// (`a_control_ends_the_cluster_before_a_replacement_for_ill_formed_bytes`).
/// Printable ASCII takes the ground's edge as it is: `Paint::piece` tells the
/// machine what a run of it leaves, which does not depend on what came
/// before. Every other byte is the ground's edge, and a control there leads
/// back here.
const fn after_control(enc: InputEncoding, node: Node, b: StreamByte) -> Edge {
    let g = ground_of(node);
    let e = one(enc, g, b);
    match e.act {
        Act::Char2 | Act::Char3 | Act::Char4 | Act::Latin1 | Act::Bad => edge(Act::EndCluster, g),
        _ => e,
    }
}

/// `ESC` in VT52, and the two bytes of a direct address.
const fn vt52(enc: InputEncoding, node: Node, b: StreamByte) -> Edge {
    match node {
        // **A second `ESC` restarts the sequence rather than dispatching.**
        // `ESC ESC A` moves the cursor up; a uniform row would hand `ESC` to
        // `Vt52::parse`, which does not name it, and the `A` would then be
        // drawn as text. One compare in a `const fn` that runs at build time.
        //
        // **Under UTF-8 a byte `80-FF` cancels the escape and is the
        // ground's after a control**, as after the ANSI `ESC` (`escape`):
        // the owner's decision of 2026-10-13 that VT52 follow the `ESC`
        // decision. It went to `Vt52::parse`, which names no such byte, so
        // the lead of `a ESC E0 A4 83 b` was dropped with the escape and its
        // two continuations each wrote U+FFFD. Under ISO 8859-1 the row is
        // what it was: what a C1 or GR byte does after a VT52 `ESC` is note
        // AE's D3, which waits on the DEC text for the mode.
        Node::Vt52Escape => match (enc, b.raw_byte()) {
            (_, 0x1b) => edge(Act::Consume, Node::Vt52Escape),
            (_, b'Y') => edge(Act::Consume, Node::Vt52Row),
            (InputEncoding::Utf8, 0x80..=0xff) => edge(Act::Again, Node::Vt52AfterControl),
            _ => edge(Act::Vt52Dispatch, Node::Vt52AfterControl),
        },
        // `ESC Y <row> <col>`, each biased by `0x20`. Two nodes because the
        // pair is a parser sub-state nothing else in either protocol reaches,
        // and because a read can fall between them.
        Node::Vt52Row => edge(Act::Vt52SetRow, Node::Vt52Col),
        _ => edge(Act::Vt52Address, Node::Vt52AfterControl),
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
        // **The parser executes it in neither**: put and ignore do not ring.
        // In a DCS the handler decides, as williams' `put` says, "C0
        // controls are also passed to the handler" (726): a Sixel rings it,
        // as `[[source: dec-std-070]]` 070-9 §9.1 gives `BEL` the "same
        // action as in text mode", and every other handler ignores it
        // (`Paint::str_put`). xterm rings it in every string but an `OSC`
        // (`[[source: xterm-reset]]`'s file, 3691-3705), practice; its
        // DECRQSS keeps the byte and fails its `strcmp`
        // (`[[source: xterm-misc]]`'s file, 4935), where baud's ignores it as
        // STD 070 3.5.4.4 recommends.
        0x07 => match node {
            Node::OscStr => edge(Act::StrEnd, Node::AfterControl),
            Node::DcsStr => edge(Act::StrPut, Node::DcsStr),
            _ => edge(Act::Consume, node),
        },
        0x1b => edge(Act::Consume, Node::StrEsc),
        // **The six format effectors, `BS HT LF VT FF CR`, are payload in a
        // DCS body, where every text puts them or leaves them to the
        // handler.** `[[source: ecma48]]` 8.3.27: DCS's command string "may
        // consist of bit combinations in the range 00/08 to 00/13 and 02/00
        // to 07/14", and 5.6: its interpretation "requires prior agreement
        // between the sender and the recipient".
        // `[[source: williams]]`'s `dcs passthrough` puts them (388), and "C0
        // controls are also passed to the handler" (726). `[[source: dec-std-070]]`
        // 3.5.4.4: "The receipt of the control characters 0/8 through 0/13
        // is valid within control strings", and 3.5.4 recommends "that these
        // characters not affect the interpretation" -- the interpretation is
        // the handler's, and of DEC's practice williams says a C0 with "no
        // meaning for the device function" is "silently ignored" by it, and
        // one that has, as in ReGIS, "acted upon" (818-822). The X3.64 text
        // williams quotes calls them "error conditions whose recovery is not
        // specified" (804-811), and a handler that ignores them is such a
        // recovery. This row ignored them, the `osc string` row, so no
        // handler could have seen one. Both handlers here give them no
        // meaning and drop them (`Paint::str_put`), so DECRQSS answers and a
        // Sixel draws as before; only a string longer than the accumulator
        // can tell, by `string_truncations`.
        //
        // In an `OSC` and an `SOS`, `PM` or `APC` they stay ignored: williams'
        // `osc string` and `sos/pm/apc string` rows (407, 426).
        0x08..=0x0d => match node {
            Node::DcsStr => edge(Act::StrPut, node),
            _ => edge(Act::Consume, node),
        },
        // **The other C0 in the payload are ignored, not stored.**
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
        // **In a DCS body they are put, williams' row whole, and each
        // handler decides** (`dcs passthrough`, 388; "C0 controls are also
        // passed to the handler", 726). Of DEC's practice williams says a C0
        // with "no meaning for the device function" is "silently ignored"
        // by it (818-822), which is `[[source: dec-std-070]]` 3.5.4.4's
        // recommendation "that all other control codes not used within a
        // command string be ignored by hardware", and 3.5.5 leaves a control
        // string's handling "at a higher level than the parser". So DECRQSS
        // and DECRSPS ignore every C0 before they read their data, a Sixel
        // acts on `BEL` and `ENQ` as 070-9 §9.1 says and ignores the rest,
        // and any other DCS is stored and discarded whole (`Paint::str_put`).
        // `[[source: ecma48]]` leaves error recovery to the implementation
        // (Annex D, item 10). Decided by Jev, B_PUT_ALL, 0.99/0.99
        // (2026-10-19). The graph ignored the 22 and put `BEL` alone, so
        // `DCS $ q m BEL ST` answered invalid and a Sixel `BEL` rang nothing.
        //
        // **`SO` and `SI` shift, and the string reads as if they were not
        // there**, in every string: `[[source: ecma48]]` §9, "SO and SI have
        // no effect on the interpretation of a control sequence, a control
        // string or the operand of a single-shift control function, but they
        // may, indeed, affect the meanings of bit combinations following in
        // the data stream" (PDF p. 89), and `[[source: dec-std-070]]` 3.5.1.4
        // the same (PDF p. 126). So neither reaches a handler, and GL is G1
        // or G0 for the text after the terminator, as in the ground. Jev
        // leaned to it, 0.86/0.88 (2026-10-23), below the bar, taken as the
        // lean; `[[source: williams]]` puts them in a DCS and ignores them
        // elsewhere, and xterm ignores them, practice. Until then every
        // string dropped them and the shift was lost.
        0x0e | 0x0f => edge(c0(b), node),
        0x00..=0x06 | 0x10..=0x17 | 0x19 | 0x1c..=0x1f => match node {
            Node::DcsStr => edge(Act::StrPut, node),
            _ => edge(Act::Consume, node),
        },
        // **`DEL` is ignored in a DCS body and kept in the other two.** For a
        // DCS every text that speaks of it drops it or lets it be dropped:
        // `[[source: williams]]`'s `dcs passthrough` ignores `7F` with no
        // condition (388); `[[source: dec-std-070]]` 3.5.4.4 recommends that
        // control codes not used in a command string be ignored, and 3.3.4.1
        // counts `DEL` among them, "also a control character, although it is
        // not a member of the C0 or C1 sets"; its 3.4.2 and `[[source: vt510-rm-b]]`
        // Table 4-1 ignore it on receipt with a 94-character set in GL, and
        // `[[source: ecma48]]` F.8.1 says it "may be inserted into, or removed
        // from, a CC-data-element without affecting its information content".
        // Only xterm stores it: `CASE_IGNORE` in `sos_table`
        // (`[[source: xterm-vtprstbl-c]]` 7780), after the byte is appended
        // (`[[source: xterm-reset]]`'s file, 3454), practice and not followed. The
        // 96-set condition cannot arise here: a command string is bit
        // combinations, not characters of a set GL invokes, `[[source: ecma48]]`
        // §9 saying SO and SI "have no effect on the interpretation" of one,
        // whatever set GL holds. This row stored it, the
        // `osc string` row, so `DCS $ q m DEL ST` answered invalid.
        //
        // `osc string` puts `7F` (`20-7F`, 407), so an `OSC` keeps it; an
        // `SOS`, `PM` or `APC` stores it and discards the string whole.
        0x7f => match node {
            Node::DcsStr => edge(Act::Consume, node),
            _ => edge(Act::StrPut, node),
        },
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
    //
    // **Which second bytes may follow is the lead's, and the `Begin` arm
    // asks it** (`waits_after`), since `E0`, `ED`, `F0` and `F4` admit fewer
    // than `80..BF`. Not here: this runs in the loop, and a `split` that read
    // the lead kept it live on every step, which cost the loop 1.2-2.2% more
    // instructions on payloads that never split a character (the record, "One
    // U+FFFD per maximal subpart").
    match (ground_of(node), act) {
        (Node::Vt52Ground, Act::Char2) => edge(Act::Begin2, Node::Vt52Utf8One),
        (Node::Vt52Ground, Act::Char3) => edge(Act::Begin3, Node::Vt52Utf8Two),
        (Node::Vt52Ground, _) => edge(Act::Begin4, Node::Vt52Utf8Three),
        (_, Act::Char2) => edge(Act::Begin2, Node::Utf8One),
        (_, Act::Char3) => edge(Act::Begin3, Node::Utf8Two),
        _ => edge(Act::Begin4, Node::Utf8Three),
    }
}

/// The node a lead `split` began waits in, once the `Begin` arm has the lead:
/// Table 3-7's narrowed row for `E0`, `ED`, `F0` and `F4`, and the node
/// `split` named for every other lead. A table built at compile time, one row
/// a ground, read at most once a `read(2)`.
#[inline(always)]
pub(crate) fn waits_after(node: Node, lead: StreamByte) -> Node {
    static WAITS: [Bytes<Node>; 2] = [build_waits(Node::Ground), build_waits(Node::Vt52Ground)];
    let row = match ground_of(node) {
        Node::Vt52Ground => &WAITS[1],
        _ => &WAITS[0],
    };
    *row.at(lead)
}

/// One ground's `waits_after` row. A byte that leads nothing never reaches
/// it, since the graph names no character act for it, and keeps the ground.
const fn build_waits(g: Node) -> Bytes<Node> {
    let mut t = [g; 256];
    let mut i = 0usize;
    while i < 256 {
        let b = StreamByte::ALL[i];
        if b.utf8_lead().is_some() {
            t[i] = waits_for_second(g, b);
        }
        i += 1;
    }
    Bytes(t)
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
    use std::format;
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
        // | a DCS body puts `08-0D` and ignores `7F` | **36/1,075/27,634/663,397** | +0.04% |
        // | a GR byte in a DCS prefix is its 7-bit byte (Latin-1) | **41/1,313/35,570/893,413** | **+34.7%** |
        // | the grounds after a control, `Node::AfterControl` | **41/1,313/34,972/852,113** | **-4.6%** |
        // | ... and `Bad` there takes the ground's edge | **41/1,313/34,972/856,965** | +0.6% |
        // | `DEL` ignored in `escape`, `csi entry`, `csi intermediate` and a string's `ESC` | **41/1,314/35,265/869,631** | +1.5% |
        // | `escape intermediate` its own node, `EscInter` | **41/1,314/35,053/860,128** | **-1.1%** |
        // | a GR byte in an escape or control sequence is its 7-bit byte (Latin-1) | **48/1,841/54,641/1,450,886** | **+68.7%** |
        // | a byte `80-FF` after `ESC` is the ground's (UTF-8) | **48/1,841/54,641/1,450,888** | +2 classes |
        //
        // **The UTF-8 `ESC` row moves only stride 4, by two.** Against a dump
        // of every edge before and after, exactly 256 changed: `80-FF` in
        // `Escape` and `EscInter` under UTF-8, each now `Again` into
        // `AfterControl` where it was `Consume` into it. The Latin-1 table is
        // the same edge for edge. Stride 1 keeps its 48 classes, since both
        // rows change alike over all of `80-FF` and no other byte takes either
        // edge there; strides 2 and 3 keep their counts and stride 4 rises by
        // two, and neither was decomposed. The table the parser runs, 256
        // edges a node, keeps its size.
        //
        // **The GR sequence row is the prefix row's cost again, six nodes
        // wide.** Against a dump of every edge before and after, 528 changed,
        // all in the Latin-1 table: 96 in each of `Escape`, `EscInter` and
        // `CsiEntry`, 95 in `CsiParam`, 80 in `CsiInter`, 63 in `CsiIgnore`
        // and 2 in `StrEsc` (`DC` and `FF`), each GR byte now its GL twin's
        // edge. The UTF-8 table is the same edge for edge. At stride 1 three
        // old classes split into ten and none merge: `BC-BF`, the markers in
        // GR, leave `A0-AF`; `BA` leaves `BB`, because a `CSI` tells `:` from
        // `;`; and `D0`, `D8 DE DF`, `DB`, `DC` and `DD`, the introducers and
        // `\` in GR, leave the two-byte leads. At stride 2, 202 old classes
        // split into 729 and none merge. Strides 3 and 4 compound the same and
        // were not decomposed. Like the prefix row, this is the analysis's
        // alphabet; the shipped tables keep their 25 rows of 256.
        //
        // **The `EscInter` row adds a node and shrinks the ladder.** Against a
        // dump of every edge before and after, 32 edges changed, `20-2F` out
        // of `Escape` under both encodings, each now `Inter` into `EscInter`
        // where it stayed in `Escape`, and the new row's 512 are new. Strides
        // 1 and 2 are the same class for class. Stride 3 falls by 212, net,
        // and was not decomposed: the words that can now coincide are those
        // in which an intermediate is followed by `P X [ ] ^ _`, which take the
        // edge any other final takes rather than opening a `CSI` or a string.
        // Stride 4 compounds the same. The table is a row longer, 25 nodes.
        //
        // **The `DEL` row moves eight edges and splits one pair.** Against a
        // dump of every edge before and after, exactly eight changed: `7F` in
        // `Escape`, `CsiEntry`, `CsiInter` and `StrEsc`, under both encodings,
        // each now `Consume` in place, where the first ended the sequence, the
        // `CSI` two led to `CsiIgnore` and `StrEsc` went to `Escape` with
        // `Again`. Stride 1 stays at 41, class for class: `7F` was a class of
        // its own already. At stride 2 one old class splits in two and none
        // merge: `7F` then `5C` leaves `7F` then the other finals, because
        // from `StrEsc` the `DEL` now stays and the `\` ends the string. Strides
        // 3 and 4 compound the same and were not decomposed.
        //
        // **The GR prefix row is a prefix's cost again, and stride 1 moves for
        // the first time since the `BEL` split, for DECRQSS's reason**, 36 ->
        // 41. Seventeen edges changed, fifteen bytes in two nodes, all
        // in the Latin-1 table, against a dump of every edge before and after:
        // in `Dcs`, `A4` now leads to `DcsDollar`, `B0-B9` are `Digit`, `BA BB`
        // are `Sep`, `F1` is `SixelStart` and `FF` is ignored in place, where
        // all fifteen led to the body; in `DcsDollar`, `F1` is `DcsQuery` and
        // `FF` is ignored in place. The UTF-8 table is the same edge for edge.
        // `A0` moved nothing, `20` and `A0` both leading to the body. At
        // stride 1 the class `A0-BF` splits four ways, `A4`, `B0-B9`, `BA BB`
        // and the rest; `FF` leaves `C0 C1 F5-FF`, the bytes that cannot lead
        // a UTF-8 character; and `F1` leaves `F0-F4`, the four-byte leads: +5,
        // and no class merges. `BA` and `BB` stay one class where `3A` and
        // `3B` are two, because a `CSI` tells `:` from `;` and a GR byte
        // never reaches one. At stride 2, against a dump of the old
        // partition, 117 old classes split into 355 and none merge, each
        // holding one of the fifteen bytes: first in 79, second in 31 and
        // both in 7. Strides 3 and 4 compound the same and were not
        // decomposed. A prefix's bytes are told apart by the edges they take
        // there, and fifteen more of them now are under ISO 8859-1.
        //
        // **The grounds after a control move only stride 3 and 4, and down.**
        // Two nodes and one action, `EndCluster`, and every edge that left a
        // control for a ground now leaves it for a twin. Stride 1 cannot move:
        // the twin's row tells bytes apart exactly where its ground's does,
        // and the edges that changed their next node changed it for whole
        // classes at once. Stride 2 is the old count. Strides 3 and 4 fall by
        // 598 and 41,300, and neither fall was decomposed. The likely reason,
        // not checked against a dump: the twin's row sends every UTF-8 lead to
        // the one `EndCluster`, where the ground's tells two-, three- and
        // four-byte leads apart, so words that perform a control and then
        // meet a lead, and differed only in the lead's length, can merge.
        //
        // **`Bad` on the twin moves only stride 4, and up**, 852,113 ->
        // 856,965. 154 edges changed, the 77 bytes UTF-8 calls no character
        // in each twin, `Bad` back to the twin becoming `Bad` to its ground
        // (the table test's count with the old edge put back); the Latin-1
        // table has no `Bad` there. Strides 1 to 3 keep their counts, and the
        // table the parser runs, 256 edges a node, keeps its size. Not
        // decomposed.
        //
        // **The DCS body's two classes move only stride 3 and 4.** Fourteen
        // edges changed, `08-0D` from `Consume` to `StrPut` and `7F` from
        // `StrPut` to `Consume`, in `DcsStr` under both encodings, and no
        // other. Stride 1 cannot move: each of the seven bytes was already
        // told apart by its ground row, and `LF`, `VT` and `FF`, one class,
        // moved together. Stride 2 is the old partition word for word,
        // against a dump of it that reproduced 1,075 and 27,628. At stride 3,
        // 8 old classes split in two and 4 merge in pairs, +6: a byte that
        // leaves `Dcs` for the body (`20`, an intermediate other than `$`)
        // and `$`, which leaves it for `DcsDollar`, then a format effector,
        // put in the body and ignored in `DcsDollar`, now differ where they
        // were alike, and the same with `7F` agree where they differed --
        // seen only when a third byte, `CAN`, `SUB` or `ESC`, sends both
        // nodes to one, since any other already tells `DcsStr` from
        // `DcsDollar`. Stride 4 compounds the same and was not decomposed.
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
        // **Table 3-7's narrowed second bytes moved all four.** A character
        // split after `E0`, `ED`, `F0` or `F4` waits in a node of its own
        // that `80-8F`, `90-9F` and `A0-BF` leave differently, so under UTF-8
        // those three ranges part. Under ISO 8859-1 `A0-BF` had parted from
        // the C1 bytes already, but `80-8F` and the C1 finals `91-97 99 9A`
        // were, by reading the rows, one class -- each a C1 read as `ESC` and
        // a final, the same edge -- and are two now: 48 -> 49, and 1,841 -> 1,878, 54,641 ->
        // 55,701 and 1,450,888 -> 1,478,109 at strides 2, 3 and 4.
        // **A byte that is no character after a control now goes through
        // `EndCluster`, as a character's lead does** (the owner's
        // SAME_AS_LITERAL, 2026-10-13): 154 edges, the 77 such bytes in
        // `AfterControl` and `Vt52AfterControl` under UTF-8. Strides 1 and 2
        // do not move; 55,701 -> 55,354 and 1,478,109 -> 1,453,919 at strides 3
        // and 4; the fall was not decomposed.
        // **A byte `80-FF` inside a `CSI` and after a VT52 `ESC` is the
        // ground's after a control, under UTF-8** (the owner's decision of
        // 2026-10-13): 640 edges, `80-FF` in the four `CSI` nodes and
        // `Vt52Escape`. Stride 1 does not move; 1,878 -> 1,816, 55,354 ->
        // 50,852 and 1,453,919 -> 1,263,705 at strides 2, 3 and 4; the fall
        // was not decomposed.
        // **Code extension moved all four** (2026-10-19): a node for a
        // pending single shift, entered on `N` and `O` from `Escape` (and so
        // on `8E` and `8F` under ISO 8859-1), whose row takes `20-7F` and,
        // under ISO 8859-1, `A0-FF` as `Act::Shifted`; `Act::Del` for `7F` in
        // the ANSI grounds; and `t` after `DCS $` for DECRSPS. 49 -> 54,
        // 1,816 -> 2,151, 50,852 -> 62,208 and 1,263,705 -> 1,589,720 at
        // strides 1 to 4, +25.8% at stride 4; which bytes parted at stride 1
        // was not decomposed.
        // **VT parts from LF and FF**: it reaches a line tabulation stop
        // (`Act::LineTab`), so `0B` is a class of its own: 49 -> 50, and
        // 1,816 -> 1,911, 50,852 -> 55,856 and 1,263,705 -> 1,452,629 at
        // strides 2, 3 and 4, each lane on the base without the other.
        // **Both together**: 55, 2,256, 68,083 and 1,818,823.
        // **`3A` in a DCS prefix ignores the string** (2026-10-19): `3A`
        // and, under ISO 8859-1, `BA` in `Dcs` lead to the ignored body,
        // where both were `Sep`. Stride 1 does not move; 2,256 -> 2,248,
        // 68,083 -> 67,636 and 1,818,823 -> 1,801,914 at strides 2 to 4.
        // **Every C0 of a DCS body is put** (Jev's B_PUT_ALL, 2026-10-19): 44
        // edges, `00-06`, `0E-17`, `19` and `1C-1F` in `DcsStr` under each
        // encoding, from `Consume` to `StrPut`. Strides 1 and 2 do not move;
        // 67,636 -> 67,669 and 1,801,914 -> 1,805,035 at strides 3 and 4.
        // **A Sixel's body is a node of its own, and its `ESC` another**
        // (2026-10-19): `SUB` there is a blank sixel, and `CAN`, a C1 and a
        // byte after its `ESC` that is not `\` place the image first
        // (`Act::SixelCut`). 55 -> 56, 2,248 -> 2,342, 67,669 -> 71,712 and
        // 1,805,035 -> 1,945,172 at strides 1 to 4, +7.8% at stride 4.
        // **ReGIS's `p` after a `DCS`** (2026-10-20): `70` and, under ISO
        // 8859-1, `F0` in `Dcs` lead to the body as `DcsQuery`, where both
        // led there as `Consume`. 56 -> 58, 2,342 -> 2,454, 71,712 -> 75,689
        // and 1,945,172 -> 2,066,297 at strides 1 to 4, +6.2% at stride 4.
        // **DECLKD's `w` after `DCS $`** (2026-10-21): `77` and, under ISO
        // 8859-1, `F7` in `DcsDollar` lead to the body as `DcsQuery`, where
        // both led there as `Consume`. 58 -> 59, 2,454 -> 2,505, 75,689 ->
        // 77,196 and 2,066,297 -> 2,106,468 at strides 1 to 4, +1.9% at
        // stride 4; which bytes parted was not decomposed.
        // **XTGETTCAP's `+` after a `DCS`** (2026-10-21): `2B` and, under
        // ISO 8859-1, `AB` in `Dcs` lead to the body as `DcsQuery`, where
        // both led there as `Consume`. 59 -> 61, 2,505 -> 2,621, 77,196 ->
        // 81,385 and 2,106,468 -> 2,232,041 at strides 1 to 4, +6.0% at
        // stride 4.
        // **DECRSTS's `p` after `DCS $`, and the `$` kept as an
        // intermediate** (2026-10-22): `70` and, under ISO 8859-1, `F0` in
        // `DcsDollar` lead to the body as `DcsQuery`, where both led there
        // as `Consume`, and `24` and `A4` in `Dcs` take `Inter` where they
        // took `Consume`. Stride 1 does not move; 2,621 -> 2,634, 81,385 ->
        // 82,006 and 2,232,041 -> 2,257,008 at strides 2 to 4, +1.1% at
        // stride 4.
        // **A C0 in a DCS introducer is executed, and `SO` and `SI` shift in
        // every string** (2026-10-23): `00-17`, `19` and `1C-1F` in `Dcs`
        // and `DcsDollar` take the ground's action where they took
        // `Consume`, `20-3F` other than the parameters, `$` and `+` lead
        // to the new `DcsIntro` where they led to the body, and `0E`/`0F`
        // in the five string bodies take `ShiftOut`/`ShiftIn`. Stride 1 does
        // not move; 2,634 -> 2,639, 82,006 -> 82,870 and 2,257,008 ->
        // 2,307,363 at strides 2 to 4, +2.2% at stride 4.
        // **`SUB` performs `Act::Signal` in ANSI mode** (2026-10-23): in
        // the ground and in every cancel edge but VT52's and a Sixel body's
        // it takes `BEL`'s action where it took `Consume`. Stride 1
        // does not move, `SUB` being a class of its own already; 2,639 ->
        // 2,641, 82,870 -> 83,141 and 2,307,363 -> 2,324,725 at strides 2
        // to 4, +0.8% at stride 4.
        // The ingest loop's table is one byte wide and does not grow.
        assert_eq!(one.len(), CLASSES, "stride 1");
        assert_eq!(two.len(), 2_641, "stride 2");
        assert_eq!(three.len(), 83_141, "stride 3");
        assert_eq!(four.len(), 2_324_725, "stride 4");
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
    /// This is exhaustive over both encodings and every node, thirty-three
    /// since Table 3-7's narrowed second bytes had nodes, so the claim is a
    /// gate rather than a comment.
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
                    Node::Ground | Node::AfterControl | Node::Vt52AfterControl | Node::Vt52Ground
                ) || mid_character(node);
                for b in [StreamByte::CAN, StreamByte::SUB] {
                    let got = Edge::of(enc, node, b);
                    // A held read cancels nothing: the byte is the rest's.
                    if node == Node::Held {
                        assert_eq!(got, edge(Act::Hold, Node::AfterControl), "{enc:?}");
                        continue;
                    }
                    // Nor does a printer controller run: it is the printer's.
                    if node == Node::Printing {
                        assert_eq!(got, edge(Act::Printer, Node::Printing), "{enc:?}");
                        continue;
                    }
                    // `SUB` performs the Substitute function in ANSI mode,
                    // whether or not there was a sequence to cancel
                    // (`[[source: dec-std-070]]` p. 5-132), and only cancels
                    // in VT52 mode; `CAN` only ever cancels.
                    let act = match (b.raw_byte(), g) {
                        (0x1a, Node::Ground) => Act::Signal,
                        _ => Act::Consume,
                    };
                    match ground_like {
                        // Nothing to cancel: `Ground` takes it as a control
                        // all the same, and mid-character
                        // it is a byte that cannot continue the character, so
                        // the partial one becomes a replacement and the byte is
                        // re-dispatched from `Ground` without being swallowed.
                        true => assert_eq!(
                            got,
                            match node {
                                Node::Ground
                                | Node::Vt52Ground
                                | Node::AfterControl
                                | Node::Vt52AfterControl => edge(act, after_control_of(node)),
                                _ => edge(Act::Resync, g),
                            },
                            "{node:?} {enc:?} byte {:#04x}",
                            b.raw_byte()
                        ),
                        // A Sixel's body and its `ESC` keep 070-9 §9.1's
                        // rule: `SUB` a blank sixel, `CAN` the image placed.
                        false => assert_eq!(
                            got,
                            match (node, b.raw_byte()) {
                                (Node::SixelStr, 0x1a) => edge(Act::StrPut, Node::SixelStr),
                                (Node::SixelStr, _) => edge(Act::SixelCut, Node::AfterControl),
                                (Node::SixelEsc, _) => edge(Act::SixelCut, Node::Escape),
                                _ => edge(act, after_control_of(node)),
                            },
                            "{node:?} {enc:?} byte {:#04x} does not cancel",
                            b.raw_byte()
                        ),
                    }
                }
            }
        }
    }

    /// **Every control ends the grapheme cluster before it, and the table is
    /// what says so**: an edge that leaves a control function, sequence or
    /// string leads to `Node::AfterControl` or its VT52 twin, never to a
    /// ground, so `Act::EndCluster` tells the cluster machine before the next
    /// character it would be asked about (`[[source: uax29]]` GB4, GB5).
    ///
    /// Exhaustive over both encodings, all thirty-three nodes and all 256
    /// bytes, three ways:
    ///
    /// * **an edge into a ground is text**: printable ASCII, whose run tells
    ///   the machine what it leaves; a character's lead and an ISO 8859-1
    ///   graphic, which go through the machine; a byte that is no character,
    ///   whose replacement goes through the machine as U+FFFD, and only from
    ///   a ground; and the end or the abandoning of a character split across
    ///   reads. Anything else into a
    ///   ground skipped the reset, as `csi_ignore`'s exit did and `ESC DEL`'s;
    /// * **a control performed in a ground leaves for the twin**, and one
    ///   performed inside a sequence stays in it, whose own end then does;
    /// * **the twin's row is its ground's**, but for the bytes that would
    ///   write through the machine, which take `EndCluster` back to the
    ///   ground.
    #[test]
    fn every_way_out_of_a_control_leads_to_the_ground_after_a_control() {
        let grounds = [Node::Ground, Node::Vt52Ground];
        let twins = [Node::AfterControl, Node::Vt52AfterControl];
        let performs = |a: Act| {
            matches!(
                a,
                Act::Index
                    | Act::LineTab
                    | Act::Backspace
                    | Act::Tab
                    | Act::ShiftOut
                    | Act::ShiftIn
                    | Act::Signal
                    | Act::CsiFinal
                    | Act::EscFinal
                    | Act::StrEnd
                    | Act::Vt52Dispatch
                    | Act::Vt52Address
            )
        };
        let mut wrong = Vec::new();
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for node in Node::FROM_INDEX {
                for b in StreamByte::ALL {
                    let e = Edge::of(enc, node, b);
                    let utf8 = mid_character(node);
                    let into_ground = grounds.contains(&e.next);
                    let text = match e.act {
                        Act::Text => grounds.contains(&node) || twins.contains(&node),
                        Act::Char2 | Act::Char3 | Act::Char4 | Act::Latin1 => {
                            grounds.contains(&node)
                        }
                        Act::Bad => grounds.contains(&node),
                        // The character a single shift applies to, which
                        // starts a cluster of its own (`Paint::shifted`).
                        Act::Shifted => node == Node::SingleShift,
                        Act::EndCluster => twins.contains(&node),
                        Act::ContEnd | Act::Resync => utf8,
                        // `CR`: the cursor is at column 0, where `glyph`
                        // starts the machine again (`Paint::restart_cluster`),
                        // or under DECOM at the left margin, where the
                        // `Return` arm does.
                        Act::Return => grounds.contains(&node) || twins.contains(&node),
                        _ => false,
                    };
                    if into_ground && !text {
                        wrong.push(format!(
                            "{enc:?} {node:?} {:#04x}: {e:?} into a ground",
                            b.raw_byte()
                        ));
                    }
                    let in_ground = grounds.contains(&node) || twins.contains(&node);
                    // A single shift leaves its shift pending in a node of
                    // its own, whose every way out is checked above.
                    let pending = e.next == Node::SingleShift;
                    if performs(e.act) && in_ground && !twins.contains(&e.next) && !pending {
                        wrong.push(format!(
                            "{enc:?} {node:?} {:#04x}: {e:?} performs a control and leaves no twin",
                            b.raw_byte()
                        ));
                    }
                    if twins.contains(&node) {
                        let g = ground_of(node);
                        let plain = Edge::of(enc, g, b);
                        let want = match plain.act {
                            Act::Char2 | Act::Char3 | Act::Char4 | Act::Latin1 | Act::Bad => {
                                edge(Act::EndCluster, g)
                            }
                            _ => plain,
                        };
                        if e != want {
                            wrong.push(format!(
                                "{enc:?} {node:?} {:#04x}: {e:?}, its ground's row says {want:?}",
                                b.raw_byte()
                            ));
                        }
                    }
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} edges:\n  {}",
            wrong.len(),
            wrong.join("\n  ")
        );
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
                    | Node::AfterControl
                    | Node::Escape
                    | Node::EscInter
                    | Node::CsiEntry
                    | Node::CsiParam
                    | Node::CsiInter
                    | Node::CsiIgnore => edge(Act::EscStart, Node::Escape),
                    Node::Utf8One
                    | Node::Utf8Two
                    | Node::Utf8Three
                    | Node::Utf8TwoAfterE0
                    | Node::Utf8TwoAfterEd
                    | Node::Utf8ThreeAfterF0
                    | Node::Utf8ThreeAfterF4 => edge(Act::Resync, Node::Ground),
                    Node::Vt52Utf8One
                    | Node::Vt52Utf8Two
                    | Node::Vt52Utf8Three
                    | Node::Vt52Utf8TwoAfterE0
                    | Node::Vt52Utf8TwoAfterEd
                    | Node::Vt52Utf8ThreeAfterF0
                    | Node::Vt52Utf8ThreeAfterF4 => edge(Act::Resync, Node::Vt52Ground),
                    Node::Dcs
                    | Node::DcsDollar
                    | Node::DcsIntro
                    | Node::OscStr
                    | Node::DcsStr
                    | Node::SosStr => edge(Act::Consume, Node::StrEsc),
                    Node::StrEsc => edge(Act::Again, Node::Escape),
                    // A Sixel's: its `ESC` may begin `ST`, and after it the
                    // byte places the image first.
                    Node::SixelStr => edge(Act::Consume, Node::SixelEsc),
                    Node::SixelEsc => edge(Act::SixelCut, Node::Escape),
                    Node::Vt52Ground | Node::Vt52AfterControl | Node::Vt52Escape => {
                        edge(Act::Consume, Node::Vt52Escape)
                    }
                    Node::Vt52Row => edge(Act::Vt52SetRow, Node::Vt52Col),
                    Node::Vt52Col => edge(Act::Vt52Address, Node::Vt52AfterControl),
                    // A held read: the `ESC` is the rest's, like every byte.
                    Node::Held => edge(Act::Hold, Node::AfterControl),
                    // A printer controller run's `ESC` is the printer's
                    // until its step reads `ESC [ 4 i`.
                    Node::Printing => edge(Act::Printer, Node::Printing),
                    // A pending single shift is cancelled and the `ESC` is
                    // the ground's after a control, where it opens a sequence.
                    Node::SingleShift => edge(Act::Again, Node::AfterControl),
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
        assert_eq!(
            ground(0x85),
            edge(Act::EscFinal, Node::AfterControl),
            "85 NEL"
        );
        assert_eq!(ground(0x9b), edge(Act::CsiStart, Node::CsiEntry), "9B CSI");
        assert_eq!(ground(0x90), edge(Act::StrStart, Node::Dcs), "90 DCS");
        assert_eq!(ground(0x9d), edge(Act::StrStart, Node::OscStr), "9D OSC");
        for node in [
            Node::Dcs,
            Node::DcsDollar,
            Node::DcsIntro,
            Node::OscStr,
            Node::DcsStr,
            Node::SosStr,
        ] {
            for v in 0x80..=0x9fusize {
                let b = StreamByte::ALL[v];
                let want = match v {
                    0x9c => edge(Act::StrEnd, Node::AfterControl),
                    _ => ground(v),
                };
                assert_eq!(
                    Edge::of(InputEncoding::Latin1, node, b),
                    want,
                    "{node:?} Latin1 byte {v:#04x}"
                );
                // Under UTF-8 `80-9F` lead out of the introducer to the body,
                // put by `DcsIntro`, as `DcsStr` puts them.
                let kept = match node {
                    Node::Dcs | Node::DcsDollar => edge(Act::Consume, Node::DcsStr),
                    Node::DcsIntro => edge(Act::StrPut, Node::DcsStr),
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
                let n = walk(intro);
                let esc = match n {
                    Node::SixelStr => Node::SixelEsc,
                    _ => Node::StrEsc,
                };
                assert_eq!(
                    Edge::of(enc, n, StreamByte::ESC),
                    edge(Act::Consume, esc),
                    "{enc:?} {intro:?}: not inside a string"
                );
            }
            assert_eq!(
                bel(walk(b"\x1b]")),
                edge(Act::StrEnd, Node::AfterControl),
                "{enc:?}: OSC"
            );
            // In the prefix it rings and the prefix goes on (STD 070 3.5.5),
            // after an intermediate or a marker too.
            for prefix in [
                b"\x1bP".as_slice(),
                b"\x1bP$",
                b"\x1bP1;2",
                b"\x1bP+",
                b"\x1bP=",
                b"\x1bP0!",
            ] {
                let n = walk(prefix);
                assert_eq!(bel(n), edge(Act::Signal, n), "{enc:?} {prefix:?}");
            }
            for body in [b"\x1bP$q".as_slice(), b"\x1bPq", b"\x1bP|", b"\x1bP+q"] {
                let n = walk(body);
                assert_eq!(bel(n), edge(Act::StrPut, n), "{enc:?} {body:?}");
            }
            for body in [b"\x1bX".as_slice(), b"\x1b^", b"\x1b_"] {
                let n = walk(body);
                assert_eq!(bel(n), edge(Act::Consume, n), "{enc:?} {body:?}");
            }
        }
    }

    /// **A DCS body puts every C0 but `CAN`, `SUB` and `ESC`, and ignores
    /// `DEL`; an `OSC`, an `SOS`, `PM` or `APC` and the DCS prefix keep their
    /// rows**, checked against the shipped table in both encodings, at the
    /// node each introducer reaches -- the 8-bit introducers too, under ISO
    /// 8859-1.
    ///
    /// `[[source: williams]]`'s `dcs passthrough` puts `00-17,19,1C-1F,20-7E`
    /// and ignores `7F` (388); `[[source: ecma48]]` 8.3.27 lets DCS's command
    /// string hold `00/08` to `00/13`, and F.8.1 lets a `DEL` be removed
    /// without changing its content. The DCS body took the `osc string` row
    /// until 2026-10-10, ignoring the format effectors and putting the
    /// `DEL`, and until 2026-10-19 it ignored `00-06`, `0E-17`, `19` and
    /// `1C-1F` and put `BEL` alone (Jev, B_PUT_ALL). In the prefix, past an
    /// intermediate or a marker too, a C0 is executed and `DEL` ignored
    /// (STD 070 3.5.5), where williams ignores both (354-368).
    #[test]
    fn a_dcs_body_puts_the_format_effectors_and_ignores_del() {
        // `SO` and `SI` shift in every string (`string`), so they are
        // checked on their own below.
        let other_c0 = || {
            (0x00..=0x06usize)
                .chain(0x10..=0x17)
                .chain([0x19])
                .chain(0x1c..=0x1f)
        };
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            let walk = |bytes: &[u8]| {
                let mut n = Node::Ground;
                for b in bytes {
                    n = Edge::of(enc, n, StreamByte::from_stream(*b)).next;
                }
                n
            };
            let at = |n: Node, v: usize| Edge::of(enc, n, StreamByte::ALL[v]);
            let mut bodies: Vec<&[u8]> = std::vec![
                b"\x1bP$q",
                b"\x1bPq",
                b"\x1bP1;2q",
                b"\x1bP0;1|",
                b"\x1bP1?$q",
                b"\x1bP+q",
                b"\x1bP=1s"
            ];
            let mut others: Vec<&[u8]> = std::vec![b"\x1b]", b"\x1bX", b"\x1b^", b"\x1b_"];
            let mut prefixes: Vec<&[u8]> = std::vec![
                b"\x1bP",
                b"\x1bP$",
                b"\x1bP1;2",
                b"\x1bP1?",
                b"\x1bP1:",
                b"\x1bP+",
                b"\x1bP+!",
                b"\x1bP=",
                b"\x1bP0!",
                b"\x1bP$1"
            ];
            if matches!(enc, InputEncoding::Latin1) {
                bodies.extend([b"\x90$q".as_slice(), b"\x90q", b"\x90+q"]);
                others.extend([b"\x9d".as_slice(), b"\x98", b"\x9e", b"\x9f"]);
                prefixes.extend([b"\x90".as_slice(), b"\x90$", b"\x90+", b"\x1bP\xab"]);
            }
            // Positive control: each walk ends inside a string, where an
            // `ESC` is the first half of `ST`, and not back in ground.
            for intro in bodies.iter().chain(&others).chain(&prefixes) {
                let n = walk(intro);
                let esc = match n {
                    Node::SixelStr => Node::SixelEsc,
                    _ => Node::StrEsc,
                };
                assert_eq!(
                    at(n, 0x1b),
                    edge(Act::Consume, esc),
                    "{enc:?} {intro:?}: not inside a string"
                );
            }
            for body in &bodies {
                let n = walk(body);
                for v in 0x08..=0x0d {
                    assert_eq!(at(n, v), edge(Act::StrPut, n), "{enc:?} {body:?} {v:#04x}");
                }
                assert_eq!(at(n, 0x7f), edge(Act::Consume, n), "{enc:?} {body:?} DEL");
                assert_eq!(at(n, 0x07), edge(Act::StrPut, n), "{enc:?} {body:?} BEL");
                for v in other_c0() {
                    assert_eq!(at(n, v), edge(Act::StrPut, n), "{enc:?} {body:?} {v:#04x}");
                }
                assert_eq!(at(n, 0x0e), edge(Act::ShiftOut, n), "{enc:?} {body:?} SO");
                assert_eq!(at(n, 0x0f), edge(Act::ShiftIn, n), "{enc:?} {body:?} SI");
            }
            for body in &others {
                let n = walk(body);
                for v in (0x08..=0x0d).chain(other_c0()) {
                    assert_eq!(at(n, v), edge(Act::Consume, n), "{enc:?} {body:?} {v:#04x}");
                }
                assert_eq!(at(n, 0x7f), edge(Act::StrPut, n), "{enc:?} {body:?} DEL");
                assert_eq!(at(n, 0x0e), edge(Act::ShiftOut, n), "{enc:?} {body:?} SO");
                assert_eq!(at(n, 0x0f), edge(Act::ShiftIn, n), "{enc:?} {body:?} SI");
            }
            // A C0 in the prefix is executed as in ground and the prefix
            // goes on (STD 070 3.5.5); `DEL` is ignored.
            for prefix in &prefixes {
                let n = walk(prefix);
                for v in (0x00..=0x17).chain([0x19]).chain(0x1c..=0x1f) {
                    assert_eq!(
                        at(n, v),
                        edge(c0(StreamByte::ALL[v]), n),
                        "{enc:?} {prefix:?} {v:#04x}"
                    );
                }
                assert_eq!(at(n, 0x7f), edge(Act::Consume, n), "{enc:?} {prefix:?} DEL");
            }
        }
    }

    /// **Under ISO 8859-1 a GR byte in the DCS prefix takes the edge its 7-bit
    /// byte takes; under UTF-8 the prefix rows are what they were**, and the
    /// body keeps every GR byte in both. Exhaustive over `A0-FF` and both
    /// prefix nodes, checked against the shipped table, with each node
    /// reached by its introducers first.
    ///
    /// `[[source: dec-std-070]]` 3.5.4.5: in "the introducer sequence to a
    /// Device Control String, the eighth bit will be ignored"; `[[source:
    /// williams]]` (462) and `[[source: ecma48]]` §9 read GR as GL there too.
    /// Both nodes sent every GR byte to the body. The edges are named as well
    /// as compared, so the comparison cannot pass with both sides moved: `A4`
    /// leads to `DcsDollar`, `B0-B9` are `Digit`, `BB` is `Sep` and `BA` leads
    /// to the ignored body as `3A` does, `F1` is
    /// the Sixel or DECRQSS `q`, and `FF`, `DEL`'s twin, is ignored in place.
    #[test]
    fn a_gr_byte_in_a_dcs_prefix_takes_its_seven_bit_edge_under_latin1() {
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            let walk = |bytes: &[u8]| {
                let mut n = Node::Ground;
                for b in bytes {
                    n = Edge::of(enc, n, StreamByte::from_stream(*b)).next;
                }
                n
            };
            let at = |n: Node, v: usize| Edge::of(enc, n, StreamByte::ALL[v]);
            let mut prefixes: Vec<(&[u8], Node)> = std::vec![
                (b"\x1bP", Node::Dcs),
                (b"\x1bP1;2", Node::Dcs),
                (b"\x1bP$", Node::DcsDollar),
                (b"\x1bP1$", Node::DcsDollar),
                (b"\x1bP+", Node::DcsIntro),
                (b"\x1bP=", Node::DcsIntro),
            ];
            let latin1 = matches!(enc, InputEncoding::Latin1);
            if latin1 {
                prefixes.extend([
                    (b"\x90".as_slice(), Node::Dcs),
                    (b"\x90$", Node::DcsDollar),
                    (b"\x1bP\xb1\xbb\xb2", Node::Dcs),
                    (b"\x90\xa4", Node::DcsDollar),
                    (b"\x90\xab", Node::DcsIntro),
                    (b"\x90\xbd", Node::DcsIntro),
                ]);
            }
            // Positive control: each introducer leaves the parser in the node
            // whose row is checked below.
            for (intro, node) in &prefixes {
                assert_eq!(walk(intro), *node, "{enc:?} {intro:?}");
            }
            for node in [Node::Dcs, Node::DcsDollar] {
                for v in 0xa0..=0xffusize {
                    let want = match latin1 {
                        true => at(node, v & 0x7f),
                        false => edge(Act::Consume, Node::DcsStr),
                    };
                    assert_eq!(at(node, v), want, "{enc:?} {node:?} {v:#04x}");
                }
            }
            // Past an intermediate or a marker the introducer puts its bytes
            // as the body does; under UTF-8 a GR byte leads to the body.
            for v in 0xa0..=0xffusize {
                let want = match latin1 {
                    true => at(Node::DcsIntro, v & 0x7f),
                    false => edge(Act::StrPut, Node::DcsStr),
                };
                assert_eq!(at(Node::DcsIntro, v), want, "{enc:?} DcsIntro {v:#04x}");
            }
            if latin1 {
                assert_eq!(at(Node::DcsIntro, 0xa1), edge(Act::StrPut, Node::DcsIntro));
                assert_eq!(at(Node::DcsIntro, 0xf1), edge(Act::StrPut, Node::DcsStr));
                assert_eq!(at(Node::DcsIntro, 0xff), edge(Act::Consume, Node::DcsIntro));
                assert_eq!(at(Node::Dcs, 0xa4), edge(Act::Inter, Node::DcsDollar));
                for v in 0xb0..=0xb9 {
                    assert_eq!(at(Node::Dcs, v), edge(Act::Digit, Node::Dcs), "{v:#04x}");
                }
                assert_eq!(at(Node::Dcs, 0xbb), edge(Act::Sep, Node::Dcs));
                assert_eq!(at(Node::Dcs, 0xba), edge(Act::Consume, Node::DcsIntro));
                assert_eq!(at(Node::Dcs, 0xf1), edge(Act::SixelStart, Node::SixelStr));
                assert_eq!(at(Node::DcsDollar, 0xf1), edge(Act::DcsQuery, Node::DcsStr));
                assert_eq!(at(Node::DcsDollar, 0xf0), edge(Act::DcsQuery, Node::DcsStr));
                assert_eq!(at(Node::Dcs, 0xff), edge(Act::Consume, Node::Dcs));
                assert_eq!(
                    at(Node::DcsDollar, 0xff),
                    edge(Act::Consume, Node::DcsDollar)
                );
            }
            for v in 0xa0..=0xffusize {
                assert_eq!(
                    at(Node::DcsStr, v),
                    edge(Act::StrPut, Node::DcsStr),
                    "{enc:?} body {v:#04x}"
                );
            }
        }
    }

    /// **Under ISO 8859-1 a C1 byte cancels an escape or control sequence and
    /// takes the edge it takes from ground; under UTF-8 none is a control:
    /// all six nodes hand the byte to the ground after a control
    /// (`under_utf8_a_byte_80_ff_after_esc_is_the_grounds_after_a_control`,
    /// `under_utf8_a_byte_80_ff_inside_a_csi_or_after_a_vt52_esc_is_the_grounds_after_a_control`).**
    /// Exhaustive over `80-9F`, the six sequence nodes and both encodings,
    /// and checked against the shipped table.
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
        assert_eq!(
            ground(0x84),
            edge(Act::EscFinal, Node::AfterControl),
            "84 IND"
        );
        assert_eq!(ground(0x9b), edge(Act::CsiStart, Node::CsiEntry), "9B CSI");
        assert_eq!(ground(0x98), edge(Act::StrStart, Node::SosStr), "98 SOS");
        assert_eq!(
            ground(0x9c),
            edge(Act::EscFinal, Node::AfterControl),
            "9C ST"
        );
        for (node, utf8) in [
            (Node::Escape, edge(Act::Again, Node::AfterControl)),
            (Node::EscInter, edge(Act::Again, Node::AfterControl)),
            (Node::CsiEntry, edge(Act::Again, Node::AfterControl)),
            (Node::CsiParam, edge(Act::Again, Node::AfterControl)),
            (Node::CsiInter, edge(Act::Again, Node::AfterControl)),
            (Node::CsiIgnore, edge(Act::Again, Node::AfterControl)),
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

    /// **A `DEL` in every node of an escape or control sequence is ignored and
    /// the node stays, under both encodings.** `[[source: williams]]` gives
    /// `7F` the *ignore* action in `escape` (286), `escape intermediate`
    /// (293), `csi entry` (306), `csi param` (315), `csi intermediate` (328)
    /// and `csi ignore`. `Escape` and `EscInter` are the two escape states,
    /// and `StrEsc` is williams' `escape` reached from a string. `Escape`
    /// (then both escape states) and `StrEsc` led out of the sequence on it, and `CsiEntry` and `CsiInter` to
    /// `CsiIgnore`. Named edges, so a comparison cannot pass with both sides
    /// moved, and checked against the shipped table.
    #[test]
    fn a_del_in_an_escape_or_control_sequence_is_ignored_and_the_node_stays() {
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for node in [
                Node::Escape,
                Node::EscInter,
                Node::CsiEntry,
                Node::CsiParam,
                Node::CsiInter,
                Node::CsiIgnore,
                Node::StrEsc,
            ] {
                assert_eq!(
                    Edge::of(enc, node, StreamByte::DEL),
                    edge(Act::Consume, node),
                    "{enc:?} {node:?}"
                );
            }
        }
    }

    /// **`EscInter` dispatches every final and opens nothing; `Escape` alone
    /// opens a `CSI` or a string.** `[[source: williams]]`: `escape
    /// intermediate` sends `30-7E` to `esc_dispatch` (224), and the edges to
    /// `csi entry`, `osc string`, `dcs entry` and `sos/pm/apc string` leave
    /// `escape` (222, 226, 232, 233). Both encodings, the whole row, against
    /// the shipped table.
    #[test]
    fn escape_intermediate_dispatches_every_final_and_opens_nothing() {
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            let at = |n: Node, v: u8| Edge::of(enc, n, StreamByte::from_stream(v));
            for v in 0x20..=0x2fu8 {
                assert_eq!(
                    at(Node::Escape, v),
                    edge(Act::Inter, Node::EscInter),
                    "{enc:?} {v:#04x}"
                );
                assert_eq!(
                    at(Node::EscInter, v),
                    edge(Act::Inter, Node::EscInter),
                    "{enc:?} {v:#04x}"
                );
            }
            for v in 0x30..=0x7eu8 {
                assert_eq!(
                    at(Node::EscInter, v),
                    edge(Act::EscFinal, Node::AfterControl),
                    "{enc:?} {v:#04x}"
                );
            }
            for v in 0..=0xffu8 {
                let e = at(Node::EscInter, v);
                assert!(
                    !matches!(e.act, Act::CsiStart | Act::StrStart)
                        || matches!(enc, InputEncoding::Latin1) && (0x80..=0x9f).contains(&v),
                    "{enc:?} {v:#04x}: {e:?} opens from EscInter"
                );
            }
            // Positive control: the same bytes open from `Escape`.
            assert_eq!(at(Node::Escape, b'['), edge(Act::CsiStart, Node::CsiEntry));
            assert_eq!(at(Node::Escape, b']'), edge(Act::StrStart, Node::OscStr));
            assert_eq!(at(Node::Escape, b'P'), edge(Act::StrStart, Node::Dcs));
            assert_eq!(at(Node::Escape, b'_'), edge(Act::StrStart, Node::SosStr));
        }
    }

    /// **Under ISO 8859-1 a GR byte in an escape or control sequence takes
    /// the edge its 7-bit byte takes; under UTF-8 none is read as GL:
    /// `StrEsc` keeps the row it had, `Escape`, `EscInter` and the four `CSI`
    /// nodes hand the byte to the ground after a control, and the string
    /// bodies keep theirs in both encodings.**
    /// Exhaustive over `A0-FF`, the six sequence nodes and `StrEsc`, and
    /// both encodings, against the shipped table.
    ///
    /// `[[source: williams]]` (462): "In all states, these codes are treated
    /// identically to GL codes 20-7F"; `[[source: ecma48]]` §9 names the
    /// "Parameter Bytes, Intermediate Bytes, and Final Bytes of a control
    /// sequence". `escape` dropped a GR byte and ended the sequence,
    /// `csi_entry` and `csi_intermediate` sent the sequence to `csi_ignore`,
    /// `csi_param` dropped the byte, `csi_ignore` waited for a 7-bit final,
    /// and `StrEsc` took `DC` for no `\`. The edges are named as well as
    /// compared, so the comparison cannot pass with both sides moved.
    #[test]
    fn a_gr_byte_in_an_escape_or_control_sequence_takes_its_seven_bit_edge_under_latin1() {
        let l1 = |n: Node, v: usize| Edge::of(InputEncoding::Latin1, n, StreamByte::ALL[v]);
        let u8_ = |n: Node, v: usize| Edge::of(InputEncoding::Utf8, n, StreamByte::ALL[v]);
        for (node, utf8) in [
            (Node::Escape, edge(Act::Again, Node::AfterControl)),
            (Node::EscInter, edge(Act::Again, Node::AfterControl)),
            (Node::CsiEntry, edge(Act::Again, Node::AfterControl)),
            (Node::CsiParam, edge(Act::Again, Node::AfterControl)),
            (Node::CsiInter, edge(Act::Again, Node::AfterControl)),
            (Node::CsiIgnore, edge(Act::Again, Node::AfterControl)),
            (Node::StrEsc, edge(Act::Again, Node::Escape)),
        ] {
            for v in 0xa0..=0xffusize {
                assert_eq!(l1(node, v), l1(node, v & 0x7f), "{node:?} Latin1 {v:#04x}");
                assert_eq!(u8_(node, v), utf8, "{node:?} Utf8 {v:#04x}");
            }
        }
        // Named: what each mechanism's GR byte now does under ISO 8859-1.
        let named = [
            (Node::Escape, 0xa8, edge(Act::Inter, Node::EscInter)),
            (Node::EscInter, 0xa5, edge(Act::Inter, Node::EscInter)),
            (
                Node::EscInter,
                0xd0,
                edge(Act::EscFinal, Node::AfterControl),
            ),
            (Node::EscInter, 0xff, edge(Act::Consume, Node::EscInter)),
            (Node::Escape, 0xb7, edge(Act::EscFinal, Node::AfterControl)),
            (Node::Escape, 0xdb, edge(Act::CsiStart, Node::CsiEntry)),
            (Node::Escape, 0xdd, edge(Act::StrStart, Node::OscStr)),
            (Node::Escape, 0xd0, edge(Act::StrStart, Node::Dcs)),
            (Node::Escape, 0xff, edge(Act::Consume, Node::Escape)),
            (Node::CsiEntry, 0xbf, edge(Act::Marker, Node::CsiParam)),
            (Node::CsiEntry, 0xb5, edge(Act::Digit, Node::CsiParam)),
            (Node::CsiEntry, 0xa1, edge(Act::Inter, Node::CsiInter)),
            (Node::CsiEntry, 0xff, edge(Act::Consume, Node::CsiEntry)),
            (Node::CsiParam, 0xbb, edge(Act::Sep, Node::CsiParam)),
            (Node::CsiParam, 0xba, edge(Act::SubSep, Node::CsiParam)),
            (
                Node::CsiParam,
                0xed,
                edge(Act::CsiFinal, Node::AfterControl),
            ),
            (
                Node::CsiInter,
                0xf0,
                edge(Act::CsiFinal, Node::AfterControl),
            ),
            (
                Node::CsiIgnore,
                0xed,
                edge(Act::Consume, Node::AfterControl),
            ),
            (Node::StrEsc, 0xdc, edge(Act::StrEnd, Node::AfterControl)),
            (Node::StrEsc, 0xff, edge(Act::Consume, Node::StrEsc)),
        ];
        for (node, v, want) in named {
            assert_eq!(l1(node, v), want, "{node:?} Latin1 {v:#04x}");
        }
        // The bodies: every GR byte is put, under both encodings, as before;
        // what an `OSC` keeps of it is `Paint::str_put`'s.
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for node in [Node::OscStr, Node::SosStr] {
                for v in 0xa0..=0xffusize {
                    assert_eq!(
                        Edge::of(enc, node, StreamByte::ALL[v]),
                        edge(Act::StrPut, node),
                        "{enc:?} {node:?} {v:#04x}"
                    );
                }
            }
        }
    }

    /// **Under UTF-8 every byte `80-FF` in `Escape` and `EscInter` cancels
    /// the sequence and is dispatched again from the ground after a control,
    /// and under ISO 8859-1 none of them takes that edge.** Exhaustive over
    /// both nodes, `80-FF` and both encodings, against the shipped table.
    ///
    /// Jev's CANCEL_AND_PRINT (decisions.jsonl lines 451-452, 0.96 and 0.92;
    /// the owner can veto). Both rows ended the sequence with `Consume`,
    /// dropping a lead byte, so the continuations of a well-formed character
    /// wrote U+FFFD, which `[[source: unicode-ch3]]` 3.9.5 rules out when the
    /// stream is read as one UTF-8 conversion. The
    /// edge it hands to is `AfterControl`'s, `EndCluster`, as after any
    /// control (`[[source: uax29]]` GB4), and then the ground's: a character
    /// is written through `Char2-4` and a byte that is no character through
    /// `Bad`; and the chain consumes the byte, so it cannot come back to the
    /// escape. `StrEsc` keeps its own
    /// edge and reaches this one through `Escape`.
    #[test]
    fn under_utf8_a_byte_80_ff_after_esc_is_the_grounds_after_a_control() {
        let u8_ = |n: Node, v: usize| Edge::of(InputEncoding::Utf8, n, StreamByte::ALL[v]);
        let l1 = |n: Node, v: usize| Edge::of(InputEncoding::Latin1, n, StreamByte::ALL[v]);
        let (mut chars, mut bad) = (0usize, 0usize);
        for node in [Node::Escape, Node::EscInter] {
            for v in 0x80..=0xffusize {
                assert_eq!(
                    u8_(node, v),
                    edge(Act::Again, Node::AfterControl),
                    "{node:?} Utf8 {v:#04x}"
                );
                assert_ne!(
                    l1(node, v),
                    edge(Act::Again, Node::AfterControl),
                    "{node:?} Latin1 {v:#04x}"
                );
                // Where it lands: the machine is told a control went past,
                // then a lead UTF-8 allows is written as a character and every
                // other byte as a replacement, both through the cluster
                // machine, and both consume it in the ground.
                assert_eq!(
                    u8_(Node::AfterControl, v),
                    edge(Act::EndCluster, Node::Ground),
                    "{v:#04x}"
                );
                let e = u8_(Node::Ground, v);
                assert_eq!(e.next, Node::Ground, "{v:#04x}");
                match e.act {
                    Act::Char2 | Act::Char3 | Act::Char4 => chars += 1,
                    Act::Bad => bad += 1,
                    a => panic!("{v:#04x}: the ground takes {a:?}"),
                }
            }
        }
        // Positive control: both kinds were reached, in the counts Table 3-7
        // gives, `C2-F4` as leads and the other 77 bytes as no character.
        assert_eq!((chars, bad), (2 * 51, 2 * 77));
        // The `ESC` inside a string hands every byte but `\` and `DEL` to
        // `Escape`, so it reaches the edge above through its own.
        for v in 0x80..=0xffusize {
            assert_eq!(
                u8_(Node::StrEsc, v),
                edge(Act::Again, Node::Escape),
                "StrEsc {v:#04x}"
            );
        }
    }

    /// **Under UTF-8 every byte `80-FF` in the four `CSI` states and after a
    /// VT52 `ESC` cancels the sequence and is dispatched again from the
    /// ground after a control, and under ISO 8859-1 none of them takes that
    /// edge.** Exhaustive over the five nodes, `80-FF` and both encodings,
    /// against the shipped table; `DEL` keeps its row in each `CSI` state.
    ///
    /// The owner's decision of 2026-10-13, "cancel and print it": the `CSI`
    /// rows as `Escape`, and `Vt52Escape` following the `ESC` decision. Each
    /// dropped the byte (`csi_entry` and `csi_inter` into `csi_ignore`,
    /// `csi_param` and `csi_ignore` staying, `Vt52Escape` to `Vt52::parse`).
    #[test]
    fn under_utf8_a_byte_80_ff_inside_a_csi_or_after_a_vt52_esc_is_the_grounds_after_a_control() {
        let u8_ = |n: Node, v: usize| Edge::of(InputEncoding::Utf8, n, StreamByte::ALL[v]);
        let l1 = |n: Node, v: usize| Edge::of(InputEncoding::Latin1, n, StreamByte::ALL[v]);
        let rows = [
            (Node::CsiEntry, Node::AfterControl),
            (Node::CsiParam, Node::AfterControl),
            (Node::CsiInter, Node::AfterControl),
            (Node::CsiIgnore, Node::AfterControl),
            (Node::Vt52Escape, Node::Vt52AfterControl),
        ];
        let mut n = 0usize;
        for (node, twin) in rows {
            for v in 0x80..=0xffusize {
                assert_eq!(
                    u8_(node, v),
                    edge(Act::Again, twin),
                    "{node:?} Utf8 {v:#04x}"
                );
                assert_ne!(
                    l1(node, v),
                    edge(Act::Again, twin),
                    "{node:?} Latin1 {v:#04x}"
                );
                n += 1;
            }
        }
        assert_eq!(n, 5 * 128);
        // `DEL` is ignored and the sequence goes on in every `CSI` state
        // (`[[source: williams]]` 306, 315, 328), and `csi ignore` ignores it.
        for node in [
            Node::CsiEntry,
            Node::CsiParam,
            Node::CsiInter,
            Node::CsiIgnore,
        ] {
            assert_eq!(u8_(node, 0x7f), edge(Act::Consume, node), "{node:?} DEL");
        }
        // Under ISO 8859-1 the VT52 row is what it was, note AE's D3.
        for v in 0x80..=0xffusize {
            assert_eq!(
                l1(Node::Vt52Escape, v),
                edge(Act::Vt52Dispatch, Node::Vt52AfterControl),
                "Vt52Escape Latin1 {v:#04x}"
            );
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
    // 48, not the 26 the paragraph above narrates: HT, CAN/SUB, SO/SI and the
    // 8-bit C1 routing each split more bytes apart after that prose was
    // written, which made 34, and the `OSC` body parting from the `SOS`, `PM`
    // and `APC` one on `BEL` split `5D` from `58 5E 5F` and `9D` from
    // `98 9E 9F`, which made 36. A GR byte in a DCS prefix read as its 7-bit
    // byte under ISO 8859-1 split `A4`, `B0-B9`, `BA BB`, `F1` and `FF` from
    // the classes they shared, which made 41. A GR byte in an escape or
    // control sequence read so split `BC-BF` from `A0-AF`, `BA` from `BB`,
    // and `D0`, `D8 DE DF`, `DB`, `DC` and `DD` from the other two-byte
    // leads, which made 48. Table 3-7's narrowed second bytes, read where a
    // split character waits after `E0`, `ED`, `F0` or `F4`, made 49 -- by
    // reading the rows, `80-8F` parting from the C1 finals `91-97 99 9A`.
    // The single shifts' node, `Act::Del` and DECRSPS's `t` made 54, and VT's own action, the line
    // tabulation stop it reaches, parting `0B` from `0A` and `0C`, made 55.
    // A Sixel's body, where `SUB` is a blank sixel and `CAN` places the
    // image, parting `1A` from `18`, made 56. ReGIS's `p` after a `DCS`, and
    // under ISO 8859-1 its GR `F0`, made 58. DECLKD's `w` after `DCS $`,
    // and its GR `F7`, made 59. XTGETTCAP's `+` after a `DCS`, and its GR
    // `AB`, made 61.
    const CLASSES: usize = 61;
    /// What `write::act` consumes, which is **not** `Act::width()`.
    ///
    /// `width` is the byte length of the *character* an act writes; two acts
    /// consume nothing at all and re-dispatch the byte. A first version of the
    /// gate below asked `width` and passed against a live hang -- the
    /// probe-that-cannot-fire shape this file records eight times, and the
    /// eighth was mine.
    fn consumed(a: Act) -> ByteLen {
        match a {
            Act::Resync | Act::Again | Act::Shifted => ByteLen::ZERO,
            _ => a.width(),
        }
    }

    /// **Only a character act reaches `split`, and only `feed` makes that
    /// true.** `feed` asks for a `Begin` edge whenever the act's width does
    /// not fit in what is left of the read:
    ///
    /// ```text
    /// if !left.holds(e.act.width()) { e = split(e.act, *node); }
    /// ```
    ///
    /// `width` is `ONE` for every act but the three character ones, and the
    /// loop runs only while a byte remains, so `left.holds(ONE)` cannot fail
    /// and `split` is unreachable for them. The invariant is real and it lives
    /// in a different file from the table that depends on it.
    ///
    /// `split` ends in `_ => edge(Act::Begin4, ...)`, so any act reaching it
    /// that is not `Char2` or `Char3` is taken to owe four bytes. Give `width`
    /// a value above one for some other act -- a five-byte encoding, a fused
    /// action -- and that act starts arriving at `split`, which will quietly
    /// tell the parser to wait for four bytes. It would not fail to compile
    /// and no existing test would notice: the graph still terminates, chunk
    /// invariance still holds at every read size that does not land inside
    /// such a character, and the cells only differ on input the corpus does
    /// not contain.
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

    /// And `split` maps each length to the bytes it actually owes.
    ///
    /// The wildcard is right for `Char4` only if `Char2` and `Char3` are taken
    /// by the arms above it -- a transposition here would park the accumulator
    /// waiting for the wrong number of continuations, which is a screen that
    /// depends on where the reads fell.
    #[test]
    fn split_owes_the_bytes_the_character_actually_has() {
        for ground in [Node::Ground, Node::Vt52Ground] {
            for (lead, act, want) in [
                (0xc3u8, Act::Char2, Act::Begin2),
                (0xe4, Act::Char3, Act::Begin3),
                (0xf0, Act::Char4, Act::Begin4),
            ] {
                let lead = StreamByte::from_stream(lead);
                assert_eq!(Edge::of(InputEncoding::Utf8, ground, lead).act, act);
                assert_eq!(
                    split(act, ground).act,
                    want,
                    "{act:?} from {ground:?} must owe its own continuations"
                );
            }
        }
    }

    /// **A character split under Unicode 8 resumes where it would have under
    /// the current tables.** `step` splits a `Char` act from the node before
    /// the step; the `Lead8` step splits from the node the edge left it in.
    /// The two agree for every node and byte, so the split's family is the
    /// same; and the Unicode 8 graph is the UTF-8 graph with its `Char` acts
    /// taken by `Lead8` and its `ContEnd` by `ContEnd8`, every node the same.
    #[test]
    fn a_split_from_the_edge_s_node_is_the_split_from_its_source() {
        let eight = graph_of(InputEncoding::Utf8, UnicodeWidths::Eight);
        for node in Node::FROM_INDEX {
            for b in StreamByte::ALL {
                let e = Edge::of(InputEncoding::Utf8, node, b);
                let e8 = Edge::of_graph(eight, node, b);
                if e.act == Act::ContEnd {
                    assert_eq!(
                        e8,
                        edge(Act::ContEnd8, e.next),
                        "{node:?} {:#04x}",
                        b.index()
                    );
                    continue;
                }
                if !matches!(e.act, Act::Char2 | Act::Char3 | Act::Char4) {
                    assert_eq!(e8, e, "{node:?} {:#04x}", b.index());
                    continue;
                }
                assert_eq!(e8, edge(Act::Lead8, e.next), "{node:?} {:#04x}", b.index());
                assert_eq!(
                    split(e.act, e.next),
                    split(e.act, node),
                    "{node:?} {:#04x}",
                    b.index()
                );
            }
        }
    }

    /// **A lead the read cut off waits for exactly the bytes Table 3-7 lets
    /// follow it.** For every lead the ground names a character act for, in
    /// both grounds: `split` begins the length the act owes, in the ground's
    /// own family of nodes, `waits_after` picks the node the `Begin` arm
    /// leaves it in, and that node continues on a second
    /// byte if and only if the domain's table says the lead admits it --
    /// `[[source: unicode-ch3]]`'s `A0..BF` after `E0`, `80..9F` after `ED`,
    /// `90..BF` after `F0`, `80..8F` after `F4`, `80..BF` after the rest --
    /// then owes what any lead of its length owes after two bytes. Every other
    /// second byte ends the maximal subpart at the lead (3.9.6): `Resync`,
    /// one U+FFFD, and the byte dispatched again from the ground.
    ///
    /// `Utf8Two` and `Utf8Three` took any continuation after any lead, so
    /// `E0 | 80` cut by a read waited for the rest of an overlong; a
    /// transposition of two narrowed nodes, or a lead sent to the wide node,
    /// fails here on its first byte.
    #[test]
    fn a_split_lead_waits_for_the_second_bytes_it_admits() {
        let mut checked = 0usize;
        for ground in [Node::Ground, Node::Vt52Ground] {
            for lead in StreamByte::ALL {
                let e = Edge::of(InputEncoding::Utf8, ground, lead);
                let (begin, owed) = match e.act {
                    Act::Char2 => (Act::Begin2, 1),
                    Act::Char3 => (Act::Begin3, 2),
                    Act::Char4 => (Act::Begin4, 3),
                    _ => continue,
                };
                // What the loop and the `Begin` arm do between them.
                let mut s = split(e.act, ground);
                assert_eq!(ground_of(s.next), ground);
                s.next = waits_after(s.next, lead);
                assert_eq!(s.act, begin, "{:#04x} from {ground:?}", lead.raw_byte());
                assert_eq!(
                    ground_of(s.next),
                    ground,
                    "{:#04x} from {ground:?}",
                    lead.raw_byte()
                );
                assert!(
                    mid_character(s.next),
                    "{:#04x} from {ground:?}",
                    lead.raw_byte()
                );
                for b in StreamByte::ALL {
                    let c = Edge::of(InputEncoding::Utf8, s.next, b);
                    let want = match (lead.admits_second(b), owed) {
                        (true, 1) => edge(Act::ContEnd, ground),
                        (true, _) => c,
                        (false, _) => edge(Act::Resync, ground),
                    };
                    assert_eq!(
                        c,
                        want,
                        "{:#04x} {:#04x} from {ground:?}",
                        lead.raw_byte(),
                        b.raw_byte()
                    );
                    if lead.admits_second(b) && owed > 1 {
                        assert_eq!(
                            c.act,
                            Act::Cont,
                            "{:#04x} {:#04x}",
                            lead.raw_byte(),
                            b.raw_byte()
                        );
                        // What is left is any continuation, after any lead.
                        let rest = c.next;
                        for b3 in StreamByte::ALL {
                            let r = Edge::of(InputEncoding::Utf8, rest, b3);
                            assert_eq!(
                                r.act == Act::Resync,
                                b3.utf8_width() != 0,
                                "{:#04x} {:#04x} {:#04x}",
                                lead.raw_byte(),
                                b.raw_byte(),
                                b3.raw_byte()
                            );
                        }
                        let generic = match (owed, ground) {
                            (2, Node::Ground) => Node::Utf8One,
                            (2, _) => Node::Vt52Utf8One,
                            (_, Node::Ground) => Node::Utf8Two,
                            _ => Node::Vt52Utf8Two,
                        };
                        assert_eq!(
                            rest,
                            generic,
                            "{:#04x} {:#04x}",
                            lead.raw_byte(),
                            b.raw_byte()
                        );
                    }
                    checked += 1;
                }
            }
        }
        // Positive control: the 51 leads `C2-F4`, all 256 bytes after each, in
        // both grounds.
        assert_eq!(checked, 2 * 51 * 256);
    }

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

    /// **The bytes `Paint::str_run` hands a kitty stream as one run are
    /// bytes the body puts, edge for edge**: a run is `run_of_text`'s,
    /// printable ASCII, and each such byte in an `APC` body is `StrPut`
    /// back to `SosStr` under both encodings, so taking a run of them in
    /// one step leaves the parser where taking them one at a time would.
    /// Every byte the run stops at is checked to be one the scan refuses.
    #[test]
    fn a_kitty_run_is_bytes_the_string_body_puts() {
        for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
            for &b in StreamByte::ALL.iter() {
                if b.is_printable_ascii() {
                    assert_eq!(
                        Edge::of(enc, Node::SosStr, b),
                        edge(Act::StrPut, Node::SosStr),
                        "{enc:?} {:#04x}",
                        b.raw_byte()
                    );
                }
            }
        }
        for v in [
            0x00, 0x07, 0x0a, 0x18, 0x1a, 0x1b, 0x1f, 0x7f, 0x80, 0x9c, 0xa0, 0xff,
        ] {
            assert!(!StreamByte::ALL[v].is_printable_ascii(), "{v:#04x}");
        }
    }
}
