//! What the table named, performed. One flat `match` and nothing else.
//!
//! **Every arm is reached because the table said so**, so nothing here re-tests
//! which byte it has or which node it is in. That is the difference from the
//! hand-written walk this replaced, where four recognisers each asked the same
//! questions again and one of them did not know a VT52 has no CSI.
//!
//! Free functions over [`Paint`] rather than methods on `Term`, because the
//! grid is borrowed for the whole read and `Term` cannot be touched while it
//! is. That is the point of `Paint`: the `View` setup is paid once a read
//! instead of once a run.

use crate::graph::{Act, Node};
use crate::osc::StrKind;
use crate::paint::Paint;
use crate::storage::STRIDE_MAX;
#[cfg(feature = "sgr-table")]
use crate::term::NSGR;
use baud_domain::Csi as DomCsi;
use baud_domain::*;

/// How many parameters a `CSI` may carry.
///
/// **`ParamIdx::CAPACITY`, not eight.** A truecolour SGR with a foreground and
/// a background is eleven parameters -- `0;1;38;2;r;g;b;48;2;r;g;b` is twelve
/// -- and a `sep` that saturates by overwriting the last slot turns that into
/// the wrong colour rather than into an error. This repo already records a
/// live bug from exactly this: `fast.rs` and `machine.rs` disagreed past the
/// sixteenth parameter, and only widening the generator to emit `SgrMany`
/// found it.
const NPARAM: usize = ParamIdx::CAPACITY;

/// The parameters of the `CSI` being parsed.
///
/// **Beside the node, not inside it**, for the same reason `Utf8Acc` is: a
/// graph is indexed by a small integer and a parameter list is not one. Safe
/// because the only edge *into* `CsiEntry` resets it.
pub(crate) struct Csi {
    /// `Option`, because an omitted parameter is not a zero: `CSI 1;;4 m` has
    /// a hole in the middle and every operation defaults it differently --
    /// CUP to 1;1, SU to 1, SGR to 0, DECSTBM to the whole screen.
    /// **`NPARAM + 1`, and the extra slot is a bin.**
    ///
    /// `sep` used to saturate at the last real slot, so a sequence with more
    /// parameters than this **overwrote** it -- `CSI 0;...;1;4 m` applied the
    /// underline and lost the bold, which is the opposite of
    /// `[[source: ecma48]]`'s "the parameter string is truncated". The comment
    /// on `sep` called it a deliberate trade against "a bounds check on the hot
    /// path for input no application sends"; the trade was not needed, because
    /// one more array element buys the same thing with no branch at all.
    ///
    /// Everything past capacity folds into index `NPARAM` and nothing reads it.
    p: [Option<Param>; NPARAM + 1],
    n: ParamIdx,
    /// Which parameters arrived after a `:` rather than a `;`. The colon form
    /// is a *group*, and without this `38:2:r:g:b` reads as `38;2;r;g;b` --
    /// correct for the first attribute and wrong for every one after it.
    sub: SubParams,
    /// `?`, `>`, `<` or `=`. **Dropping this made `CSI ? 25 h` set the ANSI
    /// mode 25 instead of the private one**, which is two different
    /// operations behind one byte.
    marker: Marker,
    /// `20-2F`. `CSI SP q` (DECSCUSR) and `CSI " q` (DECSCA) are different
    /// operations from `CSI q`, so an intermediate cannot be consumed and
    /// forgotten either.
    inter: Option<StreamByte>,
    /// The **second** intermediate, for `ESC ( % 6` and friends.
    ///
    /// `[[source: williams]]`'s `escape_intermediate` collects intermediates
    /// rather than replacing them, and keeping one meant the `(` was gone
    /// before the final arrived -- so eleven two-byte designations resolved to
    /// nothing at all. Two slots is what the grammar this core implements can
    /// actually produce: a `CSI` takes at most one, and a designation takes a
    /// slot selector plus a set selector.
    inter2: Option<StreamByte>,
}

impl Csi {
    pub(crate) const NEW: Csi = Csi {
        p: [None; NPARAM + 1],
        n: ParamIdx::FIRST,
        sub: SubParams::NONE,
        marker: Marker::Standard,
        inter: None,
        inter2: None,
    };
    fn reset(&mut self) {
        *self = Csi::NEW;
    }
    /// Put an accumulated parameter into the slot the walk is on.
    ///
    /// **`Option<Param>` and not a `Param` beside a `bool`.** `CSI 1;;4 m` has
    /// a hole in the middle, and an omitted parameter is a *defaulted* one
    /// rather than a zero -- every operation defaults it differently. The
    /// option is the two states; a flag beside the value is the missing enum
    /// this crate's audit exists to catch.
    fn put(&mut self, cur: Option<Param>) {
        self.p[self.n.raw_index()] = cur;
    }

    /// Fold the step's digits in, as the table says.
    #[cfg(not(feature = "param3"))]
    fn fold(&mut self, e: ParamStep) {
        let i = self.n.raw_index();
        self.p[i] = Some(e.fold(self.p[i].unwrap_or(Param::ZERO)));
    }

    #[cfg(feature = "param3")]
    fn fold(&mut self, e: ParamStep3) {
        let i = self.n.raw_index();
        self.p[i] = Some(e.fold(self.p[i].unwrap_or(Param::ZERO)));
    }

    /// **Saturating into the bin, not over the last parameter.** Once `n`
    /// reaches `NPARAM` every later digit folds into a slot nothing reads, so
    /// the parameters past capacity are *dropped* -- which is what truncation
    /// means -- rather than merged into the last storable one.
    fn sep(&mut self) {
        if self.n.raw_index() < NPARAM {
            self.n = self.n.next();
        }
    }
    /// A `:` -- the same advance, and the new parameter joins the group.
    fn sub_sep(&mut self) {
        self.sep();
        self.sub = self.sub.mark(self.n);
    }
    fn set_marker(&mut self, m: Marker) {
        self.marker = m;
    }
    /// Collect an intermediate, rather than replace the last one.
    fn set_inter(&mut self, b: StreamByte) {
        match self.inter {
            None => self.inter = Some(b),
            Some(_) => self.inter2 = Some(b),
        }
    }

    /// The second intermediate, if the sequence carried two.
    fn intermediate2(&self) -> Option<StreamByte> {
        self.inter2
    }
    /// How many parameter slots the sequence carries.
    ///
    /// **At least one, always.** `CSI m` is `SGR 0` and `CSI H` is `CUP 1;1`:
    /// an absent parameter is a *defaulted* parameter, not the absence of one.
    /// Reporting zero here made `fold_sgr`'s loop run no iterations, so
    /// `git-log.raw`'s commonest sequence -- `ESC [ m`, 1,779 of them -- left
    /// the rendition exactly as it was instead of resetting it. `baud-core2`
    /// spells the same rule as `count_at_least_one`.
    fn count(&self) -> Count {
        Count::of((self.n.raw_index() + 1).min(NPARAM))
    }
    /// Is this the plain `SGR` form -- standard namespace, no intermediate,
    /// final `m`? The one shape worth answering without building anything.
    /// Is this the plain `SGR` form -- standard namespace, no intermediate,
    /// final `m`? The one shape worth answering without building anything.
    pub(crate) fn is_plain_sgr(&self, f: FinalByte) -> bool {
        self.marker == Marker::Standard && self.inter.is_none() && f == FinalByte::SGR
    }

    /// The intermediate byte, if the sequence carried one.
    pub(crate) fn intermediate(&self) -> Option<StreamByte> {
        self.inter
    }

    /// Fold every SGR attribute in this sequence into `raw`.
    ///
    /// **The single-parameter form first.** `CSI 32 m`, `CSI 0 m` and
    /// `CSI 1 m` are what a sixteen-colour program emits and 86% of
    /// `asciiquarium.raw`'s SGRs, and that form reaches the attribute without
    /// building a `Params` at all. `parse_one` returns `None` for `38`/`48`
    /// and anything it does not recognise, so an unhandled attribute falls
    /// through to the general loop rather than being dropped.
    pub(crate) fn fold_sgr(&self, raw: &mut StyleWord) {
        let n = self.count().index();
        // **One parameter, and the range chain beat both alternatives.**
        // Routing it through the general `parse` covers every attribute and is
        // **-12% on asciiquarium**; a 128-entry compile-time table is flat on
        // btop and -3% on asciiquarium. The chain wins because an application
        // repeats a handful of renditions, so every branch in it predicts --
        // and this repo's rule is to replace an *unpredictable* branch with a
        // load, which this is not. `SgrAttr::one` is kept, verified against the
        // grammar, for the next person who wonders.
        if n == 1 && SgrAttr::is_plain(self.sub) {
            if let Some(v) = self.p[0] {
                if let Some(attr) = SgrAttr::parse_one(v.value()) {
                    attr.fold_into(raw);
                    return;
                }
            }
        }
        // **A `ParamIdx`, not a bare counter.** It indexes the parameter array
        // *and* realigns the sub-parameter mask, and those two must step
        // together -- two integers here is how the colon form comes to be read
        // correctly for the first attribute and wrongly for the rest.
        let mut i = ParamIdx::FIRST;
        while i.raw_index() < n {
            match SgrAttr::parse(Params::with_subs(
                &self.p[i.raw_index()..n],
                self.sub.drop_first(i),
            )) {
                Some((attr, used)) => {
                    attr.fold_into(raw);
                    i = i.advance(used.at_least_one_count());
                }
                None => break,
            }
        }
    }

    /// Every parameter this sequence carried. **Not just the first**:
    /// `CSI ? 1 ; 3 ; 4 ; 6 l` sets four modes, and rxvt's own reset string is
    /// exactly that.
    pub(crate) fn for_each_param(&self, mut f: impl FnMut(Option<Param>)) {
        for p in self.p.iter().take(self.count().index()) {
            f(*p);
        }
    }

    /// The operation, parsed by the **domain's** grammar rather than a second
    /// one written here. `baud-core2` calls the same function, so the two
    /// cores cannot drift about what a sequence means -- which is what a
    /// hand-written recogniser beside the table did, and it leaked
    /// half-parsed parameters when it bailed.
    #[inline]
    fn op(&self, f: FinalByte, lrm: MarginMode) -> baud_domain::Csi {
        baud_domain::Csi::parse_with(
            self.marker,
            self.inter,
            Params::with_subs(&self.p[..self.count().index()], self.sub),
            f,
            lrm,
        )
    }
}

/// Perform one action and report how many bytes of the read it consumed.
///
/// **An action eats a run, not a byte.** The table found *a* printable byte or
/// *a* three-byte lead; how many more follow it is a question about the bytes
/// ahead, which one vector instruction answers for sixteen at a time. That is
/// what keeps the vectored writers -- `put_ascii_run` and `put_wide_run` both
/// write eight cells to one store -- on a loop that dispatches per character.
///
/// Zero only for [`Act::Resync`], which leaves the node at `Ground` where the
/// same byte cannot produce it again.
///
/// Nine parameters because nine things vary per step and every one of them is
/// either read or written by the dispatch. `act` is `#[inline(always)]` into
/// `feed`'s loop, so a struct holding them would be a struct the loop carries --
/// the trade this file prices at 5-10% for the span field and the row map, and
/// at 1.5-4% for `Cold` by value. The audit caps how many may be raw integers,
/// which is the constraint that is actually about safety here: they are
/// `BytePos`, `ReadLen`, `ByteLen` and `Count`, not nine `usize`.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) fn act(
    p: &mut Paint<'_, '_>,
    a: Act,
    node: &mut Node,
    enc: &mut InputEncoding,
    utf8: &mut Utf8Acc,
    min: &mut Utf8Min,
    csi: &mut Csi,
    from: BytePos,
    left: ReadLen,
) -> ReadLen {
    // **Ceiling arm: the loop with every action free.** The table lookup, the
    // dispatch and both run scans still run -- they are the loop -- and
    // nothing is decoded, classified or written. `cells_written` goes to zero
    // and says so. This is the only number that answers "is the target
    // reachable without changing the loop", which no component probe can.
    #[cfg(feature = "ceiling-no-act")]
    {
        let _ = (&node, &enc, &utf8, &min, &csi);
        return match a {
            Act::Text => ReadLen::of_bytes(run_of_text(p, from, left)),
            Act::Char2 => ReadLen::from_index(2 * run_of(p, from, left, ByteLen::TWO).index()),
            Act::Char3 => ReadLen::from_index(3 * run_of(p, from, left, ByteLen::THREE).index()),
            Act::Char4 => ReadLen::from_index(4 * run_of(p, from, left, ByteLen::FOUR).index()),
            Act::Resync => ReadLen::ZERO,
            _ => ReadLen::of_bytes(a.width()),
        };
    }
    // **A jump table, and flattening it to compares is a trade rather than a
    // win.** The `match` compiles to `ldrh` an offset and `br x9` -- an
    // *indirect* branch taken once an iteration on an action that alternates
    // every few bytes, which looked like an obvious cost. Four hot arms plus a
    // `#[cold]` `rare` removes it (`br x` goes to zero in the disassembly) and
    // measures, cells byte-identical: cat-cjk **+4.3%**, less-cjk **+5.8%**,
    // and git-log **-12.6%**, vim -4.5%, asciiquarium -5%.
    //
    // The reason is the frequency table, which is why it is now counted under
    // `diag` rather than guessed at. Ordering a compare chain is a frequency
    // question and the payloads disagree about the answer: `Control` and
    // `Text` are 50/50 on asciiquarium and 64/36 on git-log, while cat-cjk
    // splits Text 33 / Char4 25 / Char3 18 / Control 15 / Char2 7. Any fixed
    // order pays the ASCII payloads to speed up the CJK ones. The jump table
    // is the shape that does not have an order.
    #[allow(unreachable_code)]
    match a {
        // **Three call sites, one body, and the length stays a constant.**
        // Folding these into one `Act::Char2 | Act::Char3 | Act::Char4` arm is
        // less code still and measures **-5% on cat-cjk and -8% on vim**,
        // cells byte-identical: `len` becomes a runtime value, so `put_one`'s
        // decode stops being unrolled and its shortest-form floor stops being
        // a constant. That is this repo's "a runtime flag in the hot loop"
        // rule, arriving through a refactor rather than through a feature.
        Act::Char2 => chars(p, from, left, ByteLen::TWO),
        Act::Char3 => chars(p, from, left, ByteLen::THREE),
        Act::Char4 => chars(p, from, left, ByteLen::FOUR),
        // **The line ending was folded in here too, and it bought nothing.**
        // `cat-scroll.raw` was 48.8% `Text` and 50.6% `Return` -- perfectly
        // alternating -- so taking the `CR LF` in the same step took it from
        // 19.5 to **37.2 bytes an iteration**, halving the loop. Throughput did
        // not move: flat on cat-scroll, **-3% on less-pager**, -1% on btop.
        //
        // Because cat-scroll is *store*-bound, not loop-bound: 70 cells an
        // iteration, and this file measures its write at the machine's ceiling.
        // A fold pays while the loop dominates -- `ESC [` was 81% of btop's
        // iterations and worth 20% -- and stops paying the moment it does not.
        // The same shape, twice, with opposite results.
        Act::Text => {
            let n = run_of_text(p, from, left);
            #[cfg(feature = "probe-double-text-scan")]
            core::hint::black_box(run_of_text(p, from, left));
            p.text(from, n);
            ReadLen::of_bytes(n)
        }
        // The edge named the operation; there is nothing left to test.
        Act::Index => {
            p.line_feed();
            ReadLen::ONE
        }
        // **`CR LF` is one step, not two.** `Return` and `Index` were 22.5%
        // and 21.5% of `less-pager.raw`'s iterations -- 44% of the loop on the
        // two bytes that end every line -- and 15% of `cat-cjk.raw`'s. The
        // pair is the commonest two-byte sequence in any line-oriented stream,
        // and it costs one lookahead to fold, exactly as `ESC [` did.
        Act::Return => {
            p.cx = Col::ZERO;
            match left.index() >= 2 && p.arena[from.index() + 1] == 0x0a {
                true => {
                    p.index();
                    ReadLen::from_index(2)
                }
                false => ReadLen::ONE,
            }
        }
        // **`BS`, and `?45` is what it does at the left margin.**
        // `[[source: ctlseqs]]`'s XTREVWRAP: "Reverse-wraparound mode" -- a
        // backspace at the left margin moves to the **right margin of the
        // previous line** instead of stopping. Off, it stops, which is the
        // default and was the only behaviour.
        //
        // `?45` was *recorded nowhere*, so `DECRQM` answered `0` and
        // `docs/vt-coverage.md` called it a gap rather than an accessor --
        // correctly: recording a mode nothing acts on is the failure that file
        // rates worst, so the bit and the behaviour had to land together.
        //
        // Bounded by the scrolling region's top, not by row 0: reverse-wrapping
        // out of a region would put the cursor somewhere the application has
        // told the terminal it is not drawing.
        Act::Backspace => {
            if p.cx.index() > p.cold.left.index() {
                p.cx = Col::at_index(p.cx.index() - 1);
            } else if p.cold.modes.is_set(ModeBit::ReverseWrap) && p.cy.index() > p.cold.top.index()
            {
                p.cy = Row::from_index(p.cy.index() - 1);
                p.cx = p.cold.right;
            }
            ReadLen::ONE
        }
        // HT. One stop forward, through the same `tab_forward` that `CHT`
        // reaches -- so the character and the control sequence cannot disagree
        // about where the stops are.
        Act::Tab => {
            p.tab_forward(Count::ONE);
            ReadLen::ONE
        }

        // **The locking shifts: which slot `GL` reads through.** `SO` invokes
        // G1 and `SI` invokes G0, and both go through the same `invoke` that
        // designation does -- so "which set is in force" has one definition
        // and the resolved byte the write path reads cannot go stale behind
        // the slots.
        Act::ShiftOut => {
            p.cold.gl = GSlot::G1;
            p.invoke();
            ReadLen::ONE
        }
        Act::ShiftIn => {
            p.cold.gl = GSlot::G0;
            p.invoke();
            ReadLen::ONE
        }

        // A character the read cut in half: park the lead's bits and wait for
        // the rest, which arrives at the front of the next `read(2)`.
        //
        // **The act already says how long the character is**, so this does not
        // ask `utf8_lead` -- which would reload `UTF8_LEAD` to recover a length
        // the edge named, and hand back an `Option` whose `None` the graph has
        // made unreachable.
        Act::Begin2 => {
            let b = StreamByte::from_stream(p.arena[from.index()]);
            *utf8 = Utf8Acc::lead(b.lead_bits(2));
            *min = Utf8Min::Two;
            ReadLen::ONE
        }
        Act::Begin3 => {
            let b = StreamByte::from_stream(p.arena[from.index()]);
            *utf8 = Utf8Acc::lead(b.lead_bits(3));
            *min = Utf8Min::Three;
            ReadLen::ONE
        }
        Act::Begin4 => {
            let b = StreamByte::from_stream(p.arena[from.index()]);
            *utf8 = Utf8Acc::lead(b.lead_bits(4));
            *min = Utf8Min::Four;
            ReadLen::ONE
        }
        Act::Cont => {
            *utf8 = utf8.push(StreamByte::from_stream(p.arena[from.index()]).continuation_bits());
            ReadLen::ONE
        }
        Act::ContEnd => {
            *utf8 = utf8.push(StreamByte::from_stream(p.arena[from.index()]).continuation_bits());
            // `finish` applies the shortest-form floor the lead byte set: a
            // non-shortest form is not a character, and `E0 80 AF` renders `/`
            // without it -- the path-traversal overlong.
            let cp = utf8.finish(*min);
            glyph(p, cp);
            ReadLen::ONE
        }
        Act::Latin1 => {
            let b = StreamByte::from_stream(p.arena[from.index()]);
            if let Some(cp) = InputEncoding::Latin1.single_byte(b) {
                glyph(p, cp);
            }
            ReadLen::ONE
        }
        // `[[source: unicode-ch3]]` 3.9.5: a converter "must not consume the
        // successor bytes as part of the ill-formed subsequence whenever those
        // successor bytes themselves constitute part of a well-formed" one. So
        // the partial character becomes a replacement and this byte is
        // re-dispatched from `Ground` rather than swallowed with it.
        Act::Resync => {
            p.glyph(Codepoint::REPLACEMENT, Width::NARROW);
            ReadLen::ZERO
        }
        // **The whole escape surface, and every arm is one byte.** Before
        // these existed `ESC` was a C0 that did nothing and `[?1049h` was
        // printable ASCII, so every sequence in the corpus was rendered to the
        // screen as its own parameter bytes.
        Act::Consume => ReadLen::ONE,
        // **The accelerated path starts at `ESC`, not at `[`.**
        //
        // `Act::EscStart` and `Act::CsiStart` were **40.5% of `btop.raw`'s
        // iterations each** -- exactly equal, because every sequence paid one
        // trip round the loop for `ESC` and another for `[` before the fast
        // path could take the rest in one. That is 81% of the loop spent on a
        // two-byte prefix. `asciiquarium.raw` was worse: 74% on the prefix,
        // plus `ESC ( B` costing *three* iterations, one per byte, 10,487
        // times a rep.
        //
        // Looking one byte ahead folds the prefix in. Everything it does not
        // recognise -- a split read, a string introducer, a private form --
        // returns `None` and the graph walks it, so there is still one
        // definition of the protocol.
        Act::EscStart => {
            csi.reset();
            match fast_esc(p, csi, from, left, node, enc) {
                Some(n) => n,
                None => ReadLen::ONE,
            }
        }
        // **The accelerated arm.** `[[source: hyperscan]]`'s technique in one
        // sentence: when an automaton cannot leave a state without seeing an
        // easily detected event, scan for the event with vectors instead of
        // stepping to it. `CsiParam` is that state -- digits and separators
        // keep the parser there, and only `40-7E` gets it out -- so the whole
        // interior of a control sequence is skippable.
        //
        // This is the shape the text path has had all along: `Act::Text` scans
        // for the run's end and hands a bulk writer the whole thing. An escape
        // sequence had no such path and was walked a byte at a time, which on
        // `btop.raw` is 66.6% of the bytes.
        //
        // **It may only decline, never disagree.** Anything it does not
        // recognise -- a C0 inside the sequence, a marker out of place, a
        // parameter after an intermediate, a final byte past the end of the
        // read -- returns `None` and the graph walks it. That is what keeps one
        // definition of the protocol: this repo already paid for a second one,
        // when a hand-written CSI grammar beside the table leaked half-parsed
        // parameters on the way out and broke chunk invariance.
        Act::CsiStart => {
            csi.reset();
            match fast_csi(p, csi, from, left, node) {
                Some(n) => n,
                None => ReadLen::ONE,
            }
        }
        // The edge is only named for `0x30..=0x39`; testing it again here gave
        // an arm that cannot be taken.
        // **Two bytes, one lookup.** The edge is named only for
        // `0x30..=0x39`, so the first byte is known to be a digit and the
        // table answers what the *pair* means -- two digits, a digit and a
        // separator, or a digit and something that is not ours. On `btop.raw`
        // `Digit` and `Sep` are **69.8% of every iteration the loop takes**,
        // and the mean digit run is 2.01 bytes.
        Act::Digit => {
            let at = from.index();
            let b0 = StreamByte::from_stream(p.arena[at]);
            // **Only bytes the read actually carries.** A read that ends on a
            // digit must not look past its end -- that is the bug this repo
            // records, where a forward scan bounded by the arena rather than
            // by the read made `cells_written` depend on the chunk size. `NUL`
            // is neither a digit nor a separator, so a substituted byte makes
            // the table answer "stop here" and the next read takes it
            // properly.
            let nul = StreamByte::from_stream(0);
            #[cfg(not(feature = "param3"))]
            let e = {
                let b1 = match left.index() >= 2 {
                    true => StreamByte::from_stream(p.arena[at + 1]),
                    false => nul,
                };
                ParamStep::of(b0, b1)
            };
            // **One test for the whole window, not one per byte.** Two guards
            // -- `left >= 2` then `left >= 3` -- cost 4% of `git-log.raw`,
            // whose parameters are one and two digits so the second guard is
            // paid on every step and never earns anything. A read carries all
            // three bytes except at most once per `read(2)`, so the rare arm is
            // the one worth branching to.
            #[cfg(feature = "param3")]
            let e = match left.index() >= 3 {
                true => ParamStep3::of(
                    b0,
                    StreamByte::from_stream(p.arena[at + 1]),
                    StreamByte::from_stream(p.arena[at + 2]),
                ),
                false => {
                    let b1 = match left.index() >= 2 {
                        true => StreamByte::from_stream(p.arena[at + 1]),
                        false => nul,
                    };
                    ParamStep3::of(b0, b1, nul)
                }
            };
            csi.fold(e);
            match e.after() {
                After::Nothing => {}
                After::Sep => csi.sep(),
                After::SubSep => csi.sub_sep(),
            }
            ReadLen::of_bytes(e.width())
        }

        Act::Sep => {
            csi.sep();
            ReadLen::ONE
        }
        Act::SubSep => {
            csi.sub_sep();
            ReadLen::ONE
        }
        // The graph names these only where the grammar allows them, so
        // neither arm re-tests the byte range it was found by.
        Act::Marker => {
            let b = StreamByte::from_stream(p.arena[from.index()]);
            if let Some(m) = Marker::from_private(b) {
                csi.set_marker(m);
            }
            ReadLen::ONE
        }
        Act::Inter => {
            csi.set_inter(StreamByte::from_stream(p.arena[from.index()]));
            ReadLen::ONE
        }
        // The three control-string arms. Each body is `#[cold]
        // #[inline(never)]` in `osc.rs`, so what lands in the per-byte loop is
        // a call and nothing else -- the rule this file learned when
        // `for w in [WIDE, NARROW]` cost 3.5% of a payload that could not
        // execute it.
        Act::StrStart => {
            // Fresh parameters for this string. A DCS's `Pa;Pb;Ph` feed `csi`
            // (the `Node::Dcs` digit edges), so it must start empty rather than
            // holding the last CSI's parameters; harmless for an OSC, which
            // reads none.
            csi.reset();
            p.str_start(StreamByte::from_stream(p.arena[from.index()]));
            ReadLen::ONE
        }
        Act::StrPut => {
            p.str_put(StreamByte::from_stream(p.arena[from.index()]));
            ReadLen::ONE
        }
        // The three VT52 arms. Each body is `#[cold] #[inline(never)]`, so
        // what lands in the per-byte loop is a call.
        Act::Bell => {
            p.bells += 1;
            ReadLen::ONE
        }
        Act::Vt52Dispatch => {
            let b = StreamByte::from_stream(p.arena[from.index()]);
            p.vt52(Vt52::parse(b), node);
            ReadLen::ONE
        }
        Act::Vt52SetRow => {
            p.cold.vt52_row = Vt52Coord::from_wire(StreamByte::from_stream(p.arena[from.index()]));
            ReadLen::ONE
        }
        Act::Vt52Address => {
            let col = Vt52Coord::from_wire(StreamByte::from_stream(p.arena[from.index()]));
            p.vt52_address(col);
            ReadLen::ONE
        }
        Act::DcsQuery => {
            p.cold.str_kind = StrKind::Decrqss;
            ReadLen::ONE
        }
        // A bare-`q` DCS is a Sixel: rename the string kind so the body the
        // `StrPut`/`StrEnd` path accumulates goes to `sixel_body` and decodes,
        // and reset the length in case a previous body did not terminate.
        Act::SixelStart => {
            p.cold.str_kind = StrKind::Sixel;
            p.cold.sixel_len = ReadLen::ZERO;
            // `Pb`/`P2` is the second parameter: 0 or 2 asks for an opaque
            // background where baud renders unpainted pixels transparent. Record
            // which it is for `sixel_end` to count *after* the image stores --
            // a refused Sixel drew nothing, so it is not a request the fill
            // would have helped. An absent or empty second parameter is
            // transparent: `for_each_param` yields only the parameters present,
            // so fewer than two leaves `p2` `None`.
            let mut i = 0u8;
            let mut p2 = None;
            csi.for_each_param(|param| {
                if i == 1 {
                    p2 = param;
                }
                i = i.saturating_add(1);
            });
            p.cold.sixel_bg = match p2.map(|v| v.value()) {
                Some(0) | Some(2) => crate::term::SixelBg::Opaque,
                _ => crate::term::SixelBg::Transparent,
            };
            csi.reset();
            ReadLen::ONE
        }
        Act::StrEnd => {
            p.str_end(StreamByte::from_stream(p.arena[from.index()]));
            ReadLen::ONE
        }
        Act::CsiFinal => {
            // The graph only names `CsiFinal` for `0x40..=0x7e`, and
            // `FinalByte` is the type that carries that proof.
            let b = StreamByte::from_stream(p.arena[from.index()]);
            csi_final(p, csi, FinalByte::of_final(b), node, from);
            ReadLen::ONE
        }
        // **A Latin-1 C1 byte is its 7-bit final, and starts a sequence of its
        // own.** The graph routes `84` as `escape(04/04)`, but this arm read
        // the byte back from the arena, so `esc_final` saw `0x84` and counted
        // IND, NEL, HTS, RI, SS2 and SS3 as unsupported. `c1_as_escape` maps
        // it, as `[[source: ctlseqs]]` pairs the two forms, and is `None` for
        // every byte an `ESC` row leads here and under UTF-8. And a C1 byte is
        // reached from Ground, or from a control string it cancels, without
        // `EscStart`, where `csi` is cleared, so it inherited the last
        // escape's intermediate: `ESC ( 0 q 84` was read as `ESC ( D` and
        // replaced G0 (`a_c1_control_does_not_inherit_the_last_escapes_intermediate`).
        Act::EscFinal => {
            let raw = StreamByte::from_stream(p.arena[from.index()]);
            let b = match enc.c1_as_escape(raw) {
                Some(b) => {
                    csi.reset();
                    b
                }
                None => raw,
            };
            esc_final(p, csi, b, enc);
            ReadLen::ONE
        }
        Act::Again => ReadLen::ZERO,
        Act::Bad => {
            p.glyph(Codepoint::REPLACEMENT, Width::NARROW);
            ReadLen::ONE
        }
    }
}

/// How far the printable-ASCII run starting here reaches, within the read.
///
/// One subtract and one unsigned compare over sixteen bytes, then a scalar
/// tail. **Never zero**: the table only names `Text` for a byte that is one.
#[inline(always)]
fn run_of_text(p: &Paint<'_, '_>, from: BytePos, left: ReadLen) -> ByteLen {
    let left = left.index();
    let s = StreamByte::slice_from(&p.arena[from.index()..from.index() + left]);
    let mut n = 0usize;
    // **Scalar first, and that is not a fallback.** A vector scan costs a load
    // and a mask extraction to discover a run of one, and `cat-cjk.raw`
    // alternates scripts often enough that discovering it is most of what the
    // scan does.
    while n < LANE_COUNT / 4 && n < left && s[n].is_printable_ascii() {
        n += 1;
    }
    if n < LANE_COUNT / 4 {
        return ByteLen::from_index(n);
    }
    // **Sixteen bytes a step, and sixty-four was tried.** The scan is 10-17%
    // of every text payload, priced by running it twice, and most of a step on
    // aarch64 is the mask extraction rather than the compare -- so four
    // vectors accumulating with `umax` and paying one reduction should win.
    // It does, on runs a screen wide: **+8% on a synthetic that never breaks a
    // line**. On the recordings it is **-2 to -3% on `cat-scroll`,
    // `less-pager` and an 80-column synthetic**, because a run that ends
    // inside the first sixty-four bytes pays for machinery it cannot use, and
    // real text breaks every eighty characters. `Vectorized::printable_run`
    // keeps the wide form for whoever finds a payload with long enough runs.
    // **The `neon-scan` arm replaces both loops with one call**, because
    // `Scan::run` is specified as "length of the leading printable-ASCII run"
    // and its own scalar tail closes the same gap this function's does. The two
    // predicates are the identical range -- `StreamByte::is_printable` and
    // `is_printable_ascii` are both `(0x20..0x7f)` -- so the swap cannot change
    // an answer, only which instructions produce it.
    #[cfg(all(feature = "neon-scan", target_arch = "aarch64"))]
    {
        // No `unsafe` here. `Neon`'s inherent methods are the safe surface over
        // a trait whose `unsafe fn` exists for `Avx2`, where the target feature
        // is real; on aarch64 NEON is baseline and the contract is vacuous, so
        // the raw load stays inside `baud-isa-aarch64`.
        let l = baud_isa_aarch64::Neon::run(&s[n..]);
        ByteLen::from_index(n + l.index())
    }
    // The x86-64 twin: 32 bytes a step, and only where the build targets AVX2.
    #[cfg(all(feature = "avx2-scan", target_arch = "x86_64", target_feature = "avx2"))]
    {
        let l = baud_isa_x86_64::Avx2::run(&s[n..]);
        ByteLen::from_index(n + l.index())
    }
    #[cfg(not(any(
        all(feature = "neon-scan", target_arch = "aarch64"),
        all(feature = "avx2-scan", target_arch = "x86_64", target_feature = "avx2")
    )))]
    {
        while n + LANE_COUNT <= left {
            if let Some(l) = Vectorized::<Printable>::load(&s[n..])
                .not_printable()
                .first()
            {
                return ByteLen::from_index(n + l.index());
            }
            n += LANE_COUNT;
        }
        while n < left && s[n].is_printable_ascii() {
            n += 1;
        }
        ByteLen::from_index(n)
    }
}

/// A run of characters of one encoded length: find it, then write it.
///
/// `#[inline(always)]` at three call sites with a constant `len`, so each
/// specialises exactly as three separate arms did -- **Char2 and Char4 had no
/// run scan at all before this**, one character each by omission, and they are
/// 7% and 25% of `cat-cjk.raw`'s dispatched actions.
#[inline(always)]
fn chars(p: &mut Paint<'_, '_>, from: BytePos, left: ReadLen, len: ByteLen) -> ReadLen {
    let k = run_of(p, from, left, len);
    // **The run scan, priced by doing it twice.** Same bytes, same answer,
    // every counter unchanged. It is **7% of cat-cjk** -- the largest single
    // item in the loop -- because aarch64 has no `movemask`: `to_bitmask`
    // lowers to a shift-narrow and a move across register files, and
    // `stride_three` takes two of them.
    #[cfg(feature = "probe-double-scan")]
    core::hint::black_box(run_of(p, from, left, len));
    // **A lead byte whose continuations are malformed, which used to hang.**
    // `run_of` counts whole characters, so `E2 41` -- a three-byte lead
    // followed by an `A` -- counts **zero**, and `len * 0` is zero. `step` has
    // already moved the node to `Ground`, so the same byte was looked up
    // again, answered `Char3` again, and scanned to zero again: an unbounded
    // loop on input a terminal must survive, since anything can arrive on a
    // pty. `ansi`'s `acsc` capability is a hundred raw high bytes and it hung
    // the conformance suite for as long as it was given.
    //
    // `[[source: unicode-ch3]]` 3.9.5 says what to emit: the **maximal
    // subpart** of the ill-formed sequence is one unit here, because `41`
    // cannot continue `E2`. So one `U+FFFD` for the lead and the byte after it
    // is re-dispatched -- which is `Act::Bad`'s behaviour, reached by a
    // different route.
    if k.index() == 0 {
        let s = StreamByte::slice_from(&p.arena[from.index()..from.index() + left.index()]);
        let n = maximal_subpart(s, len);
        p.glyph(Codepoint::REPLACEMENT, Width::NARROW);
        return n;
    }
    match k.index() {
        1 => put_one(p, from, len),
        _ => put_run(p, from, len, k, left),
    }
    let taken = len.index() * k.index();
    // **The text after the characters, in the same step.** `Char2` is 31% of
    // `vim-unicode.raw`'s iterations and `Text` 26.5%, and in mixed-script
    // prose a run of non-ASCII is followed by ASCII far more often than not --
    // a Cyrillic word, then a space. Those were two trips round the loop for
    // one stretch of text. Same fold as `ESC [` and `CR LF`: look at the next
    // byte, and if it starts a printable run, take it here.
    let rest = left.less(ReadLen::from_index(taken));
    if rest.index() > 0 {
        let at = from.advance_read(ReadLen::from_index(taken));
        if StreamByte::from_stream(p.arena[at.index()]).is_printable_ascii() {
            let n = run_of_text(p, at, rest);
            p.text(at, n);
            return ReadLen::from_index(taken + n.index());
        }
    }
    ReadLen::from_index(taken)
}

/// How many back-to-back characters of `len` bytes start here, in this read.
///
/// **A run of one *stride*, which is far commoner than a run of one character
/// class.** Every BMP codepoint above `U+07FF` is three bytes, so CJK, Hangul,
/// Devanagari and box drawing all qualify together, and a constant stride is
/// what makes the decode's swizzle indices compile-time constants.
///
/// `Route::of` is the same definition the block scanner answers to, and
/// `Route::width` already says how many bytes a lead claims -- so one test
/// serves every stride and there is no per-length table here.
///
/// **Never zero**: the graph only names a character action for a byte that
/// leads one, and the split test has already established its bytes are here.
/// Are the `len - 1` bytes after the lead all continuations?
///
/// The graph answers "how long does this lead **claim** to be"; only the bytes
/// after it say whether the claim holds. Splitting the two is what `Act::Bad`
/// already does for a byte that can begin nothing, and this is the same
/// question one byte later.
#[inline(always)]
fn well_formed(s: &[StreamByte], len: ByteLen) -> bool {
    if s.len() < len.index() {
        return false;
    }
    let mut i = 1;
    while i < len.index() {
        if lead_width(s[i]) != 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// How many bytes the ill-formed sequence at `s` occupies.
///
/// `[[source: unicode-ch3]]` 3.9.5: one `U+FFFD` per **maximal subpart**,
/// which is the lead plus every continuation that really follows it. `E2 41`
/// is one byte and `E2 80 41` is two, so the `A` is re-dispatched in both --
/// against `E2 80` being silently eaten, which is the shape that loses data
/// rather than the shape that renders it oddly.
#[inline(always)]
fn maximal_subpart(s: &[StreamByte], len: ByteLen) -> ReadLen {
    let mut n = 1;
    while n < len.index() && n < s.len() && lead_width(s[n]) == 0 {
        n += 1;
    }
    ReadLen::from_index(n)
}

#[inline(always)]
fn run_of(p: &Paint<'_, '_>, from: BytePos, left: ReadLen, len: ByteLen) -> Count {
    let (left, len) = (left.index(), len.index());
    let s = StreamByte::slice_from(&p.arena[from.index()..from.index() + left]);
    // **The median run here is one character, and one byte test says so
    // without a load.** Ungated, most of what a vector scan does is discover a
    // run of one; the gate was worth 6.7% when it was written for stride three
    // and it generalises to any stride unchanged.
    // **The first character has to be well-formed, and this assumed it was.**
    // The table names `Char3` from the *lead byte alone* -- it cannot see the
    // continuations, which is the whole reason a scan exists -- so answering
    // `1` here handed `put_one` an ill-formed sequence and consumed all three
    // bytes of it. `E2 41 61` swallowed the `A` and the `a`, which is exactly
    // what `[[source: unicode-ch3]]` 3.9.5 forbids: "must not consume the
    // successor bytes".
    if !well_formed(s, ByteLen::from_index(len)) {
        return Count::ZERO;
    }
    if left < 2 * len || lead_width(s[len]) != len {
        return Count::of(1);
    }
    let mut k = 0usize;
    // **There is deliberately no stride-four scan, and two things say so.**
    //
    // `Act::Char4` is 37.8% of `cat-cjk.raw`'s iterations and 50% of its runs
    // by encoded length, so this looked like the obvious gap -- and
    // `baud-core/tests/bulk_paths.rs` proves it was one: the vector scan was
    // stride-three-only while `CLAUDE.md` recorded the fix as "generalising
    // `run_of_three` to `run_of` over any stride". Only the *scalar* loop below
    // was generalised.
    //
    // It was built, and it is entered **zero times on every recording**. A
    // vector step needs four consecutive four-byte characters, and the corpus
    // has **none**: 13,722 four-byte runs in `cat-cjk.raw`, 3,267 in
    // `less-cjk.raw`, 1,122 in `vim-unicode.raw`, and not one of length four.
    // Four-byte characters in real text are emoji, and emoji arrive as ZWJ
    // families -- a pictograph is followed by a three-byte joiner, never by
    // another pictograph -- which is the same reason this file already gives
    // for the wide collector refusing 92% of its gate tests.
    //
    // So the scan measured flat on all nine, and `audit_all.sh` refused it as a
    // sixth scan loop against a floor of five. Measurement and gate agreed, and
    // the budget does not move to fit a change. `Vectorized::stride_four_run`
    // and `Scan::stride_four` are kept and tested for whoever finds a payload
    // that reaches them -- CJK Extension B is four bytes a character.
    // **Five characters a step where the stride is three**, which is 81% of
    // `cat-cjk.raw` and the only length a sixteen-byte block can hold five of.
    // Dropping this for a purely scalar scan is **-8% on vim** and -5% on
    // git-log, cells byte-identical -- the scan is not overhead, it is what
    // lets the vectored writer engage.
    if len == 3 {
        while 3 * k + LANE_COUNT <= left && k + 5 <= STRIDE_MAX {
            #[cfg(feature = "diag")]
            {
                p.stride_scan[3].set(p.stride_scan[3].get() + 1);
            }
            // **Direct, not through the route map.** `routes()` costs a `tbl`
            // and the `7F` correction, and reading two lane masks off it costs
            // two shift-narrow-and-move pairs on aarch64. A three-byte lead
            // and a continuation are each one compare on the raw bytes, and
            // selecting between them by lane position needs **one** mask
            // extraction rather than two.
            // **The `neon-scan` arm changes the extraction here**, which is
            // where this recording's scan cost actually is: `chars()` prices
            // the run scan at 7% of cat-cjk and names `stride_three` as taking
            // two mask extractions. `Neon::stride_three` combines the two
            // compares with `vbslq_u8` and extracts with `shrn`, where the
            // portable form ends in `to_bitmask()` and a horizontal `addv.8h`.
            #[cfg(all(feature = "neon-scan", target_arch = "aarch64"))]
            let n = baud_isa_aarch64::Neon::stride_three(&s[3 * k..]).index();
            #[cfg(all(
                not(feature = "route-stride"),
                not(all(feature = "neon-scan", target_arch = "aarch64"))
            ))]
            let n = Vectorized::<Printable>::load(&s[3 * k..]).stride_three_run();
            #[cfg(all(
                feature = "route-stride",
                not(all(feature = "neon-scan", target_arch = "aarch64"))
            ))]
            let n = Vectorized::<Printable>::load(&s[3 * k..])
                .routes()
                .stride_three();
            k += n;
            if n < 5 {
                break;
            }
        }
    }
    // **One byte-indexed load each, where this was a seven-way range match.**
    // A continuation is the byte that claims no length of its own, so both
    // questions the scan asks come off the same table.
    while k < STRIDE_MAX
        && len * (k + 1) <= left
        && lead_width(s[len * k]) == len
        && s[len * k + 1..len * (k + 1)]
            .iter()
            .all(|b| lead_width(*b) == 0)
    {
        k += 1;
    }
    Count::of(k)
}

/// One character, decoded and written with no run machinery at all.
///
/// **`put_run`'s decode buffer is a 128-byte stack array**, and the median
/// wide run on `cat-cjk.raw` is a single character -- so the common case was
/// paying for a buffer to hold one codepoint, plus the `all_wide` bookkeeping
/// and the writer choice that a run of one has already settled.
#[inline(always)]
fn put_one(p: &mut Paint<'_, '_>, from: BytePos, len: ByteLen) {
    #[cfg(feature = "diag")]
    {
        p.runs[1] += 1;
        p.run_len[len.index().min(4)] += 1;
    }
    let (at, len) = (from.index(), len.index());
    // **Index the cell instead of computing it.** `len` is a constant at each
    // call site, so this test folds; the three-byte plane is 81% of
    // `cat-cjk.raw`. The table is built once at construction and verified
    // against what `baud-core2` writes for all 65,536 sequences by
    // `the_cell_table_is_what_the_write_path_writes`.
    #[cfg(feature = "cell-table")]
    if len == 3 && !p.cells3.is_empty() {
        let s = StreamByte::slice_from(&p.arena[at..at + 3]);
        let row = (s[0].index() - 0xE0) * 64 * 256 + (s[1].index() - 0x80) * 256;
        let e = p.cells3[row + s[2].index()];
        #[cfg(feature = "diag")]
        {
            p.table_hits += 1;
        }
        p.indexed(e);
        return;
    }
    // **Ceiling arm: the per-character decode.** `put_one` takes 84% of this
    // recording's multi-byte runs, so an arm that covers only `put_run` --
    // which is what these features did before the loop was rewritten --
    // prices almost nothing. Incorrect by construction: the cell gets a
    // replacement instead of its character, and `cells_written` says so.
    #[cfg(feature = "ceiling-no-decode")]
    {
        // **A stand-in of the same width, not a replacement character.** An
        // earlier version of this arm wrote U+FFFD, which is narrow where the
        // characters it stands for are wide -- `cells_written` fell 8,325,429
        // to 3,060,872 and the arm had removed most of the wrapping and
        // scrolling along with the decode. Keyed on the encoded length, the
        // screen keeps its shape and only the bits differ.
        glyph(
            p,
            match len {
                2 => Codepoint::decoded(0x00e9),
                3 => Codepoint::decoded(0x4e00),
                _ => Codepoint::decoded(0x1f600),
            },
        );
        let _ = at;
        return;
    }
    #[allow(unreachable_code)]
    let s = StreamByte::slice_from(&p.arena[at..at + len]);
    // **Unchecked on purpose.** The table established that `s[0]` leads a
    // `len`-byte character and the split test that all of its bytes are here,
    // so the checking forms would re-ask a settled question once a byte.
    let mut raw = s[0].lead_bits(len);
    for b in &s[1..] {
        raw = (raw << 6) | b.continuation_bits();
    }
    // The decode, priced by doing it twice: same bytes, same result, every
    // counter unchanged.
    #[cfg(feature = "probe-double-decode")]
    {
        let mut again = s[0].lead_bits(len);
        for b in &s[1..] {
            again = (again << 6) | b.continuation_bits();
        }
        core::hint::black_box(again);
    }
    let floor = match len {
        2 => 0x80u32,
        3 => 0x800,
        _ => 0x1_0000,
    };
    glyph(
        p,
        match raw < floor {
            true => Codepoint::REPLACEMENT,
            false => Codepoint::decoded(raw),
        },
    );
}

/// `k` characters of one encoded length, decoded and written in one pass.
///
/// **Decoded first, then written by whichever writer fits.** A stride run is a
/// run of one *encoded length*, which is a far weaker condition than a run of
/// one character class -- but the vectored writer wants one *width*, and that
/// is a question about the decoded codepoints rather than about parser state.
/// Asking it after the decode costs a handful of compares and lets
/// `put_wide_run` write eight cells to one `Simd<u64,8>` store; asking it
/// before, as a gate on the cluster machine, is what refused 92% of candidates
/// in the previous core.
#[inline(always)]
fn put_run(p: &mut Paint<'_, '_>, from: BytePos, len: ByteLen, count: Count, left: ReadLen) {
    #[cfg(feature = "diag")]
    {
        p.runs[count.index().min(7)] += 1;
        p.run_len[len.index().min(4)] += 1;
    }
    let (len, k, left) = (len.index(), count.index(), left.index());
    #[cfg(feature = "ceiling-no-decode")]
    {
        let _ = (&p, from, len, k, left);
        return;
    }
    #[allow(unreachable_code)]
    let mut cps = [Codepoint::REPLACEMENT; STRIDE_MAX];
    let (mut all_wide, mut all_narrow) = (true, true);
    let mut all_ordinary = true;

    // **Four characters a step, in vectors, when the stride is three.** The
    // route map has already fixed every byte's position, so the swizzle
    // indices are compile-time constants and there is no boundary search --
    // which is the part of a UTF-8 decoder that resists vectorising, and the
    // reason two previous attempts at a block decoder measured *slower* than
    // one character at a time. Three-byte characters are 81% of `cat-cjk.raw`.
    //
    // **Bounded by the read, not by the arena.** Bounding it by
    // `p.arena.len()` was a real bug: the load reached bytes that had not
    // arrived, and `cells_written` then depended on the read size --
    // 8,325,429 at 4 KB chunks against 3,322,229 at 256 KB, the same bytes and
    // two answers. A forward scan on this path is bounded by what was fed.
    let mut done = 0usize;
    if len == 3 {
        while done + 4 <= k && 3 * done + LANE_COUNT <= left {
            let at = from.index() + 3 * done;
            let quad = Vectorized::<Printable>::load(StreamByte::slice_from(
                &p.arena[at..at + LANE_COUNT],
            ))
            .stride_three_quad();
            for (j, cp) in quad.iter().enumerate() {
                // Zero-width is neither, exactly as in the scalar tail below.
                // **This is the second of two accumulators and the first edit
                // missed it**, which chunk invariance caught immediately: a
                // Devanagari run of four or more three-byte characters takes
                // the vector path, kept `all_narrow` true through a mark, and
                // sent the run to `put_narrow_run` -- so U+0947 got a cell at
                // 4096-byte reads and none at one-byte reads, the same bytes
                // and two screens.
                let c = Classified::of(*cp);
                let cw = c.width(p.cold.ambiguous);
                all_wide &= cw == Width::WIDE;
                all_narrow &= cw == Width::NARROW;
                all_ordinary &= c.class == GraphemeClass::Other && c.lookback.is_none();
                cps[done + j] = *cp;
            }
            done += 4;
        }
    }

    for (i, slot) in cps.iter_mut().enumerate().take(k).skip(done) {
        let at = from.index() + len * i;
        let s = StreamByte::slice_from(&p.arena[at..at + len]);
        // **Unchecked on purpose.** The table already established that `s[0]`
        // leads a `len`-byte character and the run scan that every byte after
        // it is a continuation -- one shuffle answered that for all sixteen
        // lanes -- so the checking forms would re-ask a settled question once
        // a byte.
        let mut raw = s[0].lead_bits(len);
        for b in &s[1..] {
            raw = (raw << 6) | b.continuation_bits();
        }
        // A `match`, not an index into a constant array: the array form is a
        // bounds-checked load once a character for three values the compiler
        // already knows. `Codepoint::decoded` is the surrogate and range half,
        // spelled once in the domain; the floor depends on the encoded length
        // and so cannot live on the codepoint.
        let floor = match len {
            2 => 0x80u32,
            3 => 0x800,
            _ => 0x1_0000,
        };
        let cp = match raw < floor {
            true => Codepoint::REPLACEMENT,
            false => Codepoint::decoded(raw),
        };
        // **The table, not four range compares.** `is_wide_ordinary` answers
        // without touching memory and was tried here; it measured **414
        // against 492**, because it covers CJK, kana and the fullwidth forms
        // and refuses Hangul and everything else the table calls wide. Every
        // character it refuses drops the whole run to the per-character path,
        // and losing the vectored write costs far more than the load it
        // saves. A cheaper predicate that answers a narrower question is not
        // cheaper.
        #[cfg(not(feature = "ceiling-no-classify"))]
        {
            // **~~Zero-width counts as narrow~~ -- it did, and it stopped
            // being true the moment `Paint::glyph` started suppressing it.**
            // The old comment's reason was that `glyph` gave a combining mark
            // `need = 1`, so a run of marks already occupied one cell each and
            // calling them narrow kept `cells_written` byte-identical against
            // the per-character path. `glyph` now writes them no cell, so a
            // run called narrow here would go to `put_narrow_run` at eight
            // cells a store while the single path suppresses them -- the two
            // paths would disagree, which is exactly what that check exists to
            // catch.
            //
            // Neither uniform, so the run falls to the per-character path and
            // every character gets the same answer. Rare by construction: a
            // run has to *contain* a zero-width character to take it.
            let c = Classified::of(cp);
            let cw = c.width(p.cold.ambiguous);
            all_wide &= cw == Width::WIDE;
            all_narrow &= cw == Width::NARROW;
            all_ordinary &= c.class == GraphemeClass::Other && c.lookback.is_none();
        }
        // **Priced by adding one, not by removing it.** Removing the lookup
        // changes `all_wide` and so changes which writer takes the run -- the
        // arm that tried it measured *slower* for that reason. A second
        // identical lookup leaves every counter alone and costs exactly one.
        #[cfg(feature = "probe-double-classify")]
        core::hint::black_box(Classified::of(cp));
        #[cfg(feature = "ceiling-no-classify")]
        {
            all_wide = false;
            all_narrow = false;
            all_ordinary = false;
        }
        *slot = cp;
    }

    #[cfg(feature = "ceiling-no-write")]
    {
        let _ = (&cps, all_wide, all_narrow, all_ordinary);
        p.cells = p.cells.plus(k);
        return;
    }
    #[allow(unreachable_code)]
    // **A run of one does not want the vectored writer.** `put_wide_run`
    // amortises its setup -- bounds, `phys`, the orphan repair, the extent --
    // over eight cells to a store; for a single character it pays all of that
    // for two cells. The run-length histogram says **84% of runs on
    // `cat-cjk.raw` are one character**, which is the recording alternating
    // scripts rather than anything about the loop -- the block walk reported
    // 69% for the same bytes because a block capped what it could see.
    if k == 1 {
        glyph(p, cps[0]);
        return;
    }
    // **The narrow writer existed and nothing called it.** `put_narrow_run`
    // writes eight cells to one store and only `baud-core2` referenced it, so
    // every narrow non-ASCII character went one at a time through the ~38
    // operations of `Paint::glyph`: box drawing, braille, Devanagari, kana,
    // narrow CJK punctuation, Greek, Cyrillic. This file already prices that
    // inversion at 266 MB/s against a wide character's 1155.
    // **The bulk writers ask the machine nothing, so a run may only reach one
    // if the machine can be advanced across it without walking it.**
    //
    // That is possible exactly when the run leaves the machine somewhere that
    // does not depend on where it started, and for class `Other` with no
    // lookback it does: an ordinary character ends any regional-indicator run,
    // clears the `Extended_Pictographic` lookback GB11 needs and clears the
    // Indic lookback GB9c needs. `one_ordinary_character_lands_in_one_state_
    // from_every_state` checks that against `CLUSTER_NEXT` for all 94 reachable
    // states rather than arguing it from the rules.
    //
    // **So the gate asks about the first character, not about the state.** The
    // first version required `ordinary_run_ready()` -- the machine already in
    // that state -- and refused the whole run otherwise, which
    // `ceiling-no-run-gate` then priced at **33.7% of cat-cjk**: on text that
    // alternates scripts the machine is mid-cluster at most run boundaries, so
    // most runs were refused and every character took the per-character path.
    // One `is_break_classified` on a `Copy` answers instead whether the run's
    // first character starts a cluster, and characters two onward cannot
    // disagree -- they begin from the converged state, where an ordinary
    // character always breaks.
    //
    // The `Copy` matters: the real machine must advance exactly once per
    // character, and `set_after_ordinary` below is that advance for the whole
    // run at once. Asking on the live machine would advance it twice for the
    // first character.
    #[cfg(not(feature = "ceiling-no-run-gate"))]
    let admit = all_ordinary && k > 0 && {
        let mut probe = p.cluster;
        probe.is_break_classified(GraphemeClass::Other, Lookback::from_bits(0))
    };
    // **The ceiling arm: the gate always admits.** Every run reaches the bulk
    // writers, so a run holding a ZWJ or a regional indicator is written as
    // separate characters -- `a_flag_is_one_cluster` renders four regional
    // indicators where two flags belong, which is what makes this an upper
    // bound on the gate's fallback rather than a measurement of it. On the
    // corpus no recording changes a cell, because everything that clusters is
    // separated by a codepoint of a different encoded length and so never
    // reaches a stride run at all; the fixture is the only thing that can see
    // the difference, which is why it exists.
    #[cfg(feature = "ceiling-no-run-gate")]
    let admit = {
        // `all_ordinary` is the gate's own input and the arm is what ignores
        // it; consumed so the stub does not trip `-D warnings`.
        let _ = all_ordinary;
        k > 0
    };
    if !admit {
        for cp in cps.iter().take(k) {
            glyph(p, *cp);
        }
        return;
    }
    let uniform = match (all_wide, all_narrow) {
        (true, _) => Some(Width::WIDE),
        (_, true) => Some(Width::NARROW),
        _ => None,
    };
    if let Some(w) = uniform.filter(|w| k > 0 && p.fits(count, *w)) {
        p.run(&cps[..k], w);
        // **The advance for the whole run, in one store.** Every character in
        // it was ordinary and the first one broke, so the machine ends where
        // one ordinary character leaves it -- which the domain test above pins
        // as the same state from all 94.
        p.cluster = ClusterBreak::after_ascii_run();
        return;
    }
    // Mixed widths, or the row is about to end: one at a time, which also
    // handles the wrap.
    //
    // **The second `Classified::of` here is not a duplicate worth removing.**
    // Carrying the width alongside the codepoint, so this loop reads what the
    // loop above computed, measured **632-638 against 675-680**: a
    // `[bool; 32]` beside the decode buffer is a store a character in the hot
    // loop, and the lookup it saves is on a table the probe above prices at
    // **0%** -- fully hidden behind the work around it. Removing a free load
    // by adding a store is a loss, and this is the second time that shape has
    // measured one here.
    for cp in cps.iter().take(k) {
        glyph(p, *cp);
    }
}

/// One codepoint at its own width.
///
/// **Two arms meet here**, which is why every writer goes through it rather
/// than calling `Paint::glyph` directly. `ceiling-no-classify` assumes narrow,
/// pricing the property-table load; `ceiling-no-write` keeps the classification
/// and drops the cell, pricing the store and the addressing. Both are
/// incorrect by construction and both move `cells_written`, so a pair that
/// disagrees there is not measuring what it says.
#[inline(always)]
fn glyph(p: &mut Paint<'_, '_>, cp: Codepoint) {
    #[cfg(feature = "ceiling-no-classify")]
    let w = Width::NARROW;
    #[cfg(not(feature = "ceiling-no-classify"))]
    let w = {
        // **One lookup, and the break decision rides on it.** `Classified::of`
        // answers width, class and lookback together from one load, and this
        // core read the width and threw the other two away -- which are
        // exactly the operands `is_break_classified` takes. So GB11 costs a
        // table index on a value already in a register, not a second lookup.
        let cl = Classified::of(cp);
        let w = cl.width(p.cold.ambiguous);
        // **Does this codepoint continue the cluster before it?** If so it
        // belongs to the previous cell rather than to one of its own, which is
        // what makes `U+1F468 ZWJ U+1F469` one family instead of two people.
        //
        // The `cx` test is not belt and braces: `[[source: uax29]]` GB1 puts a
        // boundary at start of text, so a continuation at column zero has no
        // base and is written as its own character rather than attached.
        // **Priced by adding one, on a `Copy` so the machine advances once.**
        // Calling it twice on the real machine advances the cluster state
        // twice, which stops marks attaching and moves `cells_written` -- the
        // contamination this file records from the same probe in the other
        // core, where four of eighteen points were the extra writes rather
        // than the call.
        #[cfg(feature = "probe-double-break")]
        {
            let mut c = p.cluster;
            core::hint::black_box(c.is_break_classified(cl.class, cl.lookback));
        }
        if !p.cluster.is_break_classified(cl.class, cl.lookback) && p.cx != Col::ZERO {
            p.attach(cp);
            return;
        }
        w
    };
    // **Phase 0: is there a loop-carried dependency through the cursor?**
    // `cx += need` waits on `need`, which waits on `w`, which waits on a load
    // from `BMP_DIRECT` -- so character N+1's store address cannot be computed
    // until character N's table load returns. This arm keeps the load, and
    // discards it, and takes the width from a compare on the codepoint the
    // decode already put in a register. Same lookup, no dependence on it.
    //
    // **Incorrect by construction and `cells_written` says so** -- the width
    // is wrong for everything between U+1100 and the real wide ranges -- so it
    // is an upper bound on breaking the chain, not a measurement of it.
    #[cfg(feature = "probe-free-width")]
    let w = {
        core::hint::black_box(w);
        match cp.value() >= 0x1100 {
            true => Width::WIDE,
            false => Width::NARROW,
        }
    };
    // **Priced by adding one, not by removing it.** Removing the lookup makes
    // every character narrow, which changes how the screen wraps and scrolls
    // -- that arm's `cells_written` falls 8,325,429 to 6,332,978, so it is not
    // measuring the lookup. A second identical lookup leaves every counter
    // alone and costs exactly one.
    #[cfg(feature = "probe-double-classify")]
    core::hint::black_box(Classified::of(cp));
    #[cfg(feature = "ceiling-no-write")]
    {
        core::hint::black_box((cp, w));
        return;
    }
    #[allow(unreachable_code)]
    p.glyph(cp, w);
}

/// What a `CSI` final byte names.
///
/// **A table, because a final byte is a bounded input.** The map was nine
/// literal arms with a silent `_ => {}`, which is exactly what
/// `baud_domain::Csi::Unsupported { final_byte }` exists to prevent -- its doc
/// says it is a variant "rather than a `_ => {}` arm on purpose: the set of
/// operations not yet implemented becomes enumerable."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
#[cfg(test)]
pub(crate) enum CsiOp {
    /// Recognised, consumed, and not implemented here. Enumerable rather than
    /// silent: `the_unimplemented_finals_are_enumerable` counts them.
    Unsupported,
    Goto,
    Up,
    Down,
    Right,
    Left,
    Column,
    Row,
    EraseDisplay,
    EraseLine,
}

/// The same map, materialised over the whole final-byte space.
///
/// **Not on the dispatch path, and that is a measurement.** Indexing it instead
/// of calling `csi_op_of` measures **-3.4% on btop** (576-581 against 596-602),
/// cells byte-identical: the dispatch is `#[cold]`, runs once per sequence, and
/// branches on finals a TUI repeats, so the compare chain is predicted and the
/// load is not. This repo's rule is to replace an *unpredictable* branch with a
/// load, and this one is predictable.
///
/// It stays because it is what makes the gap enumerable --
/// `the_unimplemented_finals_are_enumerable` counts the 53 finals this core
/// recognises and does not implement, which a `_ => {}` could not.
#[cfg(test)]
pub(crate) static CSI_OP: [CsiOp; FinalByte::COUNT] = csi_ops();

#[cfg(test)]
const fn csi_ops() -> [CsiOp; FinalByte::COUNT] {
    let mut t = [CsiOp::Unsupported; FinalByte::COUNT];
    let mut i = 0usize;
    while i < FinalByte::COUNT {
        t[i] = csi_op_of(FinalByte::at(i));
        i += 1;
    }
    t
}

#[cfg(test)]
pub(crate) const fn csi_op_of(f: FinalByte) -> CsiOp {
    match f.raw() {
        b'H' | b'f' => CsiOp::Goto,
        b'A' => CsiOp::Up,
        b'B' => CsiOp::Down,
        b'C' => CsiOp::Right,
        b'D' => CsiOp::Left,
        b'G' => CsiOp::Column,
        b'd' => CsiOp::Row,
        b'J' => CsiOp::EraseDisplay,
        b'K' => CsiOp::EraseLine,
        _ => CsiOp::Unsupported,
    }
}

/// Perform the operation a `CSI` final byte names.
///
/// **`#[cold]` and out of line.** It runs once a sequence where the loop above
/// runs once a byte, and this repo has measured that a big `match` inlined into
/// a per-byte body costs every payload whether or not it executes.
#[cold]
#[inline(never)]
fn csi_final(p: &mut Paint<'_, '_>, csi: &Csi, f: FinalByte, node: &mut Node, at: BytePos) {
    #[cfg(feature = "probe-no-csi-op")]
    {
        let _ = (&p, &csi, f, &node, at);
        return;
    }
    #[allow(unreachable_code)]
    let lrm = match p.cold.modes.is_set(ModeBit::SideMargins) {
        true => MarginMode::On,
        false => MarginMode::Off,
    };
    // **SGR before anything is built, because it needs none of it.**
    // `fold_sgr` reads the parameter array directly, so the `Params` below and
    // the whole `baud_domain::Csi` this function would parse are dead for the
    // one final byte that dominates every recording in the corpus: SGR is
    // **86% of asciiquarium's** dispatched finals, 63% of btop's and 100% of
    // git-log's. The condition is exactly the general path's -- standard
    // namespace, no intermediate, final `m` -- so anything else falls through
    // unchanged.
    if csi.is_plain_sgr(f) {
        p.apply_sgr(csi);
        return;
    }
    #[cfg(feature = "probe-double-parse")]
    core::hint::black_box(csi.op(f, lrm));
    match csi.op(f, lrm) {
        DomCsi::CursorPosition { row, col } => {
            let base = p.origin_row().index();
            let want = Row::from_index(base + row.to_row(p.rows).index());
            p.goto(p.confine(want), col.to_col(p.cols));
        }
        DomCsi::CursorUp(c) => p.cy = p.confine(p.cy.up(c)),
        DomCsi::CursorDown(c) => p.cy = p.confine(p.cy.down(c, p.rows)),
        DomCsi::CursorForward(c) => p.cx = p.cx.forward(c, p.cols),
        DomCsi::CursorBack(c) => p.cx = p.cx.back(c),
        DomCsi::CursorNextLine(c) => {
            p.cy = p.confine(p.cy.down(c, p.rows));
            p.cx = Col::ZERO;
        }
        DomCsi::CursorPrevLine(c) => {
            p.cy = p.confine(p.cy.up(c));
            p.cx = Col::ZERO;
        }
        DomCsi::ColumnAbsolute(c) => p.cx = c.to_col(p.cols),
        DomCsi::LineAbsolute(r) => {
            let base = p.origin_row().index();
            let want = Row::from_index(base + r.to_row(p.rows).index());
            p.cy = p.confine(want);
        }
        DomCsi::ColumnRelative(c) => p.cx = p.cx.forward(c, p.cols),
        DomCsi::LineRelative(c) => p.cy = p.confine(p.cy.down(c, p.rows)),
        DomCsi::EraseInLine(e) => p.erase_line(e),
        DomCsi::EraseInDisplay(e) => p.erase_display(e),
        DomCsi::EraseChars(c) => p.erase_chars(c),
        DomCsi::DeleteChars(c) => p.delete_chars(c),
        DomCsi::InsertBlanks(c) => p.insert_blanks(c),
        DomCsi::InsertLines(c) => p.insert_lines(c),
        DomCsi::DeleteLines(c) => p.delete_lines(c),
        DomCsi::ScrollUp(c) => p.scroll_region_up(c),
        DomCsi::ScrollDown(c) => p.scroll_region_down(c),
        DomCsi::SetScrollRegion { top, bottom } => p.set_scroll_region(top, bottom),
        DomCsi::Sgr => p.apply_sgr(csi),
        DomCsi::SaveCursor => p.cold.saved = Some(p.save()),
        DomCsi::RestoreCursor => {
            let s = p.cold.saved;
            p.restore(s);
        }
        DomCsi::CursorForwardTab(c) => p.tab_forward(c),
        DomCsi::CursorBackTab(c) => p.tab_back(c),
        DomCsi::TabClear(w) => p.tab_clear(w),
        DomCsi::PrivateMode { change, .. } => p.apply_private_modes(csi, change, node, at),
        DomCsi::SaveModes => p.save_private_modes(csi),
        DomCsi::RestoreModes => p.restore_private_modes(csi, node, at),
        DomCsi::Mode { change, mode } => p.apply_ansi_mode(change, mode),
        DomCsi::SelectProtection { protect } => {
            p.cold.protect = CellFlags::protection(protect.is_protected());
            p.refresh_flags();
        }
        DomCsi::SelectiveEraseLine(e) => p.selective_erase_line(e),
        DomCsi::SelectiveEraseDisplay(e) => p.selective_erase_display(e),
        DomCsi::EraseSavedLines => p.erase_saved_lines(),
        DomCsi::SelectiveEraseSavedLines => p.selective_erase_saved_lines(),
        DomCsi::Repeat(c) => p.repeat(c),
        DomCsi::SoftReset => p.soft_reset(),
        DomCsi::InsertColumns(c) => p.insert_columns(c),
        DomCsi::DeleteColumns(c) => p.delete_columns(c),
        DomCsi::SetSideMargins { left, right } => p.set_side_margins(left, right),
        // **Answered, now that there is a channel.** Each goes through a named
        // `Report`, so the shape is chosen by the variant rather than by the
        // call site -- `docs/vt-coverage.md`'s rule is that answering in the
        // wrong shape is worse than silence, because an application parses
        // whatever arrives as the report it asked for.
        DomCsi::DeviceAttributes => {
            // Advertise Sixel (attribute `4`) only when the embedder opted in
            // (`Term::set_sixel_claim`) and the store is provisioned, so the
            // claim is honest: an application that reads it
            // then draws with Sixel rather than a text fallback, and an
            // unprovisioned baud that refuses every Sixel must not claim it.
            let sixel = match (p.cold.sixel_claim, p.images.provisioned()) {
                (SixelClaim::Yes, true) => SixelClaim::Yes,
                _ => SixelClaim::No,
            };
            p.want_reply(Report::PrimaryDeviceAttributes(
                DeviceAttribute::Vt102,
                sixel,
            ))
        }
        // **`DSR 6` is the cursor, `DSR 5` is health, and nothing else is
        // answered.** An unimplemented `DSR` gets no reply at all rather than
        // a plausible one.
        DomCsi::DeviceStatus(k) => match k {
            // **Relative to the origin when DECOM is set**, which is what
            // `[[source: vt510-decom]]` requires: an application inside a
            // margin asks where it is and must be told in its own coordinates.
            Some(StatusKind::CursorPosition) => {
                let base = p.origin_row().index();
                let row = Row::from_index(p.cy.index().saturating_sub(base));
                // **The last writable column, not the sentinel one past
                // it.** With `DECAWM` on, the cursor parks one past the right
                // margin until the next character wraps it, and `CPR`
                // reported that -- a column the application cannot address,
                // off the end of its own row. The same clamp `Term::cursor`
                // carries, which is where this was found the first time.
                let col = Col::at_index(p.cx.index().min(p.cold.right.index()));
                p.want_reply(Report::CursorPosition {
                    row: row.one_based(),
                    col: col.one_based(),
                });
            }
            Some(StatusKind::Operating) => p.want_reply(Report::OperatingStatus),
            // **A `DSR` this core cannot answer gets silence, and silence is
            // the right reply** -- an application parses whatever arrives as
            // the report it asked for, so a wrong shape is worse than none.
            // What was missing is the *count*: refusing is not the same as not
            // knowing, and the conformance inventory asks exactly that.
            _ => p.unsupported_param += 1,
        },
        // **Recorded, not acted on, and those are different things.** The
        // region is what an application asks the terminal to highlight; baud
        // has no selection model to highlight *with*, which is a reason not to
        // draw it and not a reason to discard the request. An embedder that
        // has one reads it from [`Term::highlight_region`].
        //
        // It is also the arity trap. `CSI Ps T` is SD and `CSI Ps ; Ps ; Ps ;
        // Ps ; Ps T` is this, and reading the five-parameter form as SD makes
        // a highlight request **scroll the screen** -- silently, because SD is
        // implemented and the sequence reaches a real handler.
        // `baud_domain::Csi` splits them on `p.len() >= 5`, so the two cannot
        // be confused here; the arity is the discriminator and it is one line
        // above in a crate both cores share.
        DomCsi::HighlightMouseTracking(r) => p.cold.highlight_region = r,
        DomCsi::CursorStyle(CursorPick::Style(c)) => p.cold.cursor_style = c,
        DomCsi::CursorStyle(CursorPick::Initial) => p.cold.cursor_style = p.cold.initial_cursor,
        // **DECRQM, answered from the modes this core actually stores.**
        // `Paint::mode_state` returns `NotRecognized` for everything else,
        // which is the rule `docs/vt-coverage.md` states: answering `Reset`
        // for a mode that does not exist tells an application the mode is
        // available, and it then sets it and believes it took effect.
        DomCsi::RequestMode { marker, mode } => {
            let state = p.mode_state(marker, mode);
            p.want_reply(Report::ModeStatus {
                marker,
                mode,
                state,
            });
        }
        // **XTVERSION, and the refusal it replaces was scoped wrongly.** The
        // reason on record was that answering "would name a version this core
        // does not have" -- but `Report::Version` is `baud_domain`'s and
        // already encodes `DCS > | baud ST`, which is the answer core2 sends.
        // The name is the *program*'s rather than the core's, and both cores
        // are baud. Read a refusal against its scope, which this file says to
        // do and which has closed four others.
        DomCsi::RequestVersion => p.want_reply(Report::Version),
        // **XTWINOPS 14, 16 and 18: the size of the text area, which an image
        // client sizes its pictures from.** `[[source: yazi-emulator]]` sends
        // `CSI 16 t` in its DA1 burst and takes the cell size from the
        // `CSI 6 ; h ; w t` it gets back; without it, and without pixels in
        // the pty's `winsize`, it draws every image at a fixed 600x900.
        // The pixel answers need the cell size the embedder gives
        // [`Term::set_cell_size`]; until then they are counted and unanswered,
        // the same silence an unanswerable `DSR` gets -- and so is every other
        // form of the family, which acts on a window this core is not handed.
        DomCsi::Window(op) => {
            match Report::window(op, p.rows, p.cols, p.cell_width, p.cell_height) {
                Some(r) => p.want_reply(r),
                None => p.unsupported_param += 1,
            }
        }
        // **XTMODKEYS and XTFMTKEYS: recorded here and read by `Term::key`.**
        // They land in `Cold` rather than on `Paint` because no byte of the
        // ingest loop reads them -- they are written once per control sequence
        // and consulted once per key press, which is exactly the split `Cold`
        // exists for.
        //
        // Parsing them without the encoder acting on them would be a mode
        // recorded and never read, which `docs/vt-coverage.md` rates worse than
        // not implementing the sequence at all -- so the two halves landed in
        // one change.
        DomCsi::SetOtherKeys(k) => p.cold.other_keys = k,
        DomCsi::SetOtherKeysFormat(k) => p.cold.other_keys_format = k,
        // **`[[source: kitty-keyboard]]`: the negotiation is live and the query
        // is answered.** Set, push and pop maintain the active screen's stack,
        // and `CSI ? u` replies with the current flags -- which is the
        // protocol's support claim, made only now that `Term::key` honours all
        // five enhancements for every key it can represent. See the kitty
        // section of `CLAUDE.md`.
        DomCsi::KittySet { flags, mode } => p.cold.kitty.set(flags, mode),
        DomCsi::KittyPush { flags } => p.cold.kitty.push(flags),
        DomCsi::KittyPop { count } => p.cold.kitty.pop(count),
        DomCsi::KittyQuery => p.want_reply(Report::KittyFlags(p.cold.kitty.current())),
        // The class of what `OSC 10`/`11 ; ?` would answer, and silence where
        // either would be silent -- the `OSC 4` refusal, for the same reason:
        // a terminal told no colours has nothing true to say.
        DomCsi::ColorSchemeQuery => {
            if let Some(s) = p.cold.color_scheme() {
                p.want_reply(Report::ColorScheme(s));
            }
        }
        DomCsi::Unsupported { .. } => {
            p.unsupported_total += 1;
            #[cfg(feature = "diag")]
            {
                p.unsupported_by_final[f.slot()] += 1;
            }
        }
    }
}

/// Perform a two-byte `ESC` sequence.
///
/// **`#[cold]`, like `csi_final`**: one call a sequence against the loop's one
/// call a byte.
#[cold]
#[inline(never)]
fn esc_final(p: &mut Paint<'_, '_>, csi: &Csi, b: StreamByte, enc: &mut InputEncoding) {
    // A designator is an *intermediate plus* a final: `ESC ( B` selects
    // USASCII into G0 and `ESC B` is a different sequence entirely.
    if let Some(i) = csi.intermediate() {
        match i.raw_byte() {
            // **Designation loads a slot; it does not invoke one.** `ESC ) 0`
            // puts DEC graphics in G1 and changes nothing on screen until an
            // `SO` reads through it. Writing the resolved set here is what
            // made `baud-core` render `ESC ) 0` as though it were `ESC ( 0`.
            b'(' | b')' | b'*' | b'+' => {
                if let Some(slot) = GSlot::from_intermediate(i) {
                    // **A second intermediate names the set, not the slot.**
                    // `ESC ( % 6` is G0 <- Portuguese: `(` selects the slot and
                    // `% 6` the set, so a resolver reading only the final sees
                    // `6` and answers `Other`.
                    let set = match csi.intermediate2() {
                        Some(i2) => Charset::from_multi(i2, b),
                        None => Charset::from_final(b),
                    };
                    if set == Charset::Other {
                        p.charset_unsupported += 1;
                    }
                    p.cold.g[slot.index()] = set;
                    p.invoke();
                }
            }
            // `ESC # 8` -- DECALN fills the screen with `E`, and it is how
            // vttest checks alignment.
            b'#' if b.raw_byte() == b'8' => p.screen_alignment(),
            // **`S7C1T` and `S8C1T`, `ESC SP F` and `ESC SP G`.**
            // `docs/vt-coverage.md` marked both **done** while this core
            // implemented neither and answered every query seven-bit, and
            // nothing caught it until an unsupported *count* made a dropped
            // `ESC` observable -- before that, "implemented" and "recognised
            // and silently discarded" produced the same snapshot.
            //
            // It changes only what baud *writes*. What it reads is settled by
            // the encoding, because a `9B` can be a control only where it
            // cannot be a UTF-8 continuation byte.
            b' ' if b.raw_byte() == b'F' => p.cold.c1 = C1Transmit::SevenBit,
            b' ' if b.raw_byte() == b'G' => p.cold.c1 = C1Transmit::EightBit,
            // **`ESC SP L/M/N` and `ESC # 3/4/5`, counted rather than
            // dropped.** The first three are ANSI conformance levels and the
            // last three double-height and double-width lines -- none
            // implemented, and all three previously indistinguishable from an
            // implemented form because the wildcard below did nothing at all.
            // **`ESC % @` and `ESC % G`, and this arm was the whole gap.**
            // Both tables are built at compile time and `Edge::of` selects
            // between them, so the Latin-1 half of the graph has existed and
            // been unreachable -- `Act::Latin1` appeared in no recording's
            // histogram because nothing could put the parser in the table that
            // names it. `[[source: ctlseqs]]`: `ESC % @` selects ISO 8859-1,
            // `ESC % G` UTF-8.
            //
            // It writes `p.cold` *and* the loop's live copy, because a stream
            // can switch between two bytes of one `read(2)` and the next byte
            // has to be parsed by the table this one selected.
            b'%' => {
                let want = match b.raw_byte() {
                    b'@' => InputEncoding::Latin1,
                    b'G' => InputEncoding::Utf8,
                    _ => {
                        p.unsupported_total += 1;
                        return;
                    }
                };
                p.cold.encoding = want;
                *enc = want;
            }
            _ => p.unsupported_total += 1,
        }
        return;
    }
    match Esc::parse(b) {
        Esc::SaveCursor => p.cold.saved = Some(p.save()),
        Esc::RestoreCursor => {
            let s = p.cold.saved;
            p.restore(s);
        }
        Esc::ReverseIndex => p.reverse_index(),
        Esc::Index => {
            p.index();
        }
        Esc::NextLine => {
            p.index();
            p.cx = Col::ZERO;
        }
        Esc::TabSet => {
            let x = p.cx.index();
            if x < p.tabs.len() {
                p.tabs[x] = true;
            }
        }
        Esc::Reset => p.hard_reset(),
        // **DECKPAM and DECKPNM, recorded and exposed.** They decide what the
        // keypad sends, so the terminal's whole part is to know which mode is
        // live and let the embedder ask -- the same argument
        // `ModeBit::BracketedPaste` makes. Dropping them was the `parsed`
        // failure `docs/vt-coverage.md` names: accepted, did nothing, and no
        // way for an application to find out.
        Esc::KeypadApplication => p.cold.keypad = KeypadMode::Application,
        Esc::KeypadNumeric => p.cold.keypad = KeypadMode::Numeric,
        // **Counted, and it was a bare `{}`.** `spec_inventory` asks one
        // question per form the specification lists -- "was this acted on, or
        // *counted*" -- and it asks it by snapshotting the terminal either
        // side. An `ESC` this core does not implement changed nothing
        // observable, so it read as **recognised and dropped**, which
        // `docs/vt-coverage.md` rates worse than not implementing it: the
        // application gets no way to find out.
        //
        // `baud-core2` answers with one `unsupported` counter every dispatch
        // arm bumps; this core had one for CSI *finals* only, which is why
        // `every_spec_csi_final_is_handled_or_counted` passed while the ESC,
        // OSC and DCS inventories did not. The mechanism was right and its
        // coverage was three quarters short.
        Esc::Unsupported => p.unsupported_total += 1,
    }
}

/// Where the control sequence's final byte is, measured from `from`.
///
/// One vector compare over sixteen bytes at a time. **`FinalByte::from_stream`
/// is the range test**, so the `40-7E` bound has one definition and the scalar
/// tail cannot drift from the vector body.
#[inline(always)]
#[cfg(feature = "two-pass-csi")]
fn final_byte_at(p: &Paint<'_, '_>, from: BytePos, left: ReadLen) -> Option<ByteLen> {
    let s = StreamByte::slice_from(&p.arena[from.index()..from.index() + left.index()]);
    let (lo, hi) = (StreamByte::from_stream(0x40), StreamByte::from_stream(0x7e));
    let mut n = 0usize;
    // **Scalar first, and that is not a fallback.** `ESC [ K` and `ESC [ m`
    // put the final byte at offset zero, and a sixteen-byte load with a mask
    // extraction to discover that costs more than looking. `less-pager.raw`
    // sends 850 of them a rep and went **3% slower** with the scan ungated,
    // while declining none of them -- so it was not wasted scans, it was
    // scanning for something already in front of it. Same shape, same fix, as
    // `run_of_text`'s scalar prefix, which is worth 6.7% there.
    //
    // **Four, measured against two.** A two-byte prefix is 5% worse on
    // asciiquarium (379-389 against 407-411) and no better anywhere: its
    // sequences put the final at about offset two, so a shorter prefix stops
    // just before the answer and pays for the vector anyway.
    while n < LANE_COUNT / 4 && n < left.index() {
        if FinalByte::from_stream(s[n]).is_some() {
            return Some(ByteLen::from_index(n));
        }
        n += 1;
    }
    while n + LANE_COUNT <= left.index() {
        if let Some(l) = Vectorized::<Printable>::load(&s[n..])
            .in_range(lo, hi)
            .first()
        {
            return Some(ByteLen::from_index(n + l.index()));
        }
        n += LANE_COUNT;
    }
    while n < left.index() {
        if FinalByte::from_stream(s[n]).is_some() {
            return Some(ByteLen::from_index(n));
        }
        n += 1;
    }
    None
}

/// A whole `CSI` in one step, or `None` and the graph walks it.
///
/// `from` is the `[`. The parameters run from `from + 1` to the final byte.
///
/// **Inlined, and it was `#[cold]`.** Out of line it has a call's setup before
/// it can decide anything, and that setup is most of what a short sequence
/// costs: on a payload of one `ESC [ 3 2 m` per eighty bytes the accelerated
/// path measured **3% slower than the graph walking it**, while being +25% on
/// `btop.raw` whose sequences are eleven bytes at one per seventeen. The scan
/// and the parameter walk are small; only `csi_final`'s match is big, and that
/// is `#[cold]` on its own.
#[inline(always)]
fn fast_csi(
    p: &mut Paint<'_, '_>,
    csi: &mut Csi,
    from: BytePos,
    left: ReadLen,
    node: &mut Node,
) -> Option<ReadLen> {
    #[cfg(feature = "probe-no-fast-csi")]
    {
        let _ = (&p, &csi, from, left, &node);
        return None;
    }
    #[cfg(feature = "diag")]
    {
        p.fast_try += 1;
    }
    #[allow(unreachable_code)]
    let body = from.advance_read(ReadLen::ONE);
    let rest = left.less(ReadLen::ONE);
    if rest.index() == 0 {
        return None;
    }
    // **Additive: the terminator scan run twice, the second discarded.** Every
    // counter and every stored parameter unchanged, so the delta is the scan's
    // own marginal cost -- the question being whether a structural index over
    // the whole read could be worth building.
    #[cfg(feature = "probe-double-final-scan")]
    core::hint::black_box(final_byte_at(p, body, rest));
    // **One pass, not two.** `final_byte_at` classified every byte to find the
    // terminator and the walk below then classified the same bytes again to
    // accumulate -- two passes over the eight to twelve bytes of a `CSI`. The
    // walk already has a fallthrough arm for "not a digit, separator, marker or
    // intermediate", and that arm *is* the terminator, so one pass can do both.
    //
    // It is speculative: nothing checks that a terminator is present before the
    // accumulation starts. That is the right trade because the check almost
    // never fails -- `fast_csi` declines **0.1-0.4%** of the time -- and the
    // failure path is cheap. But it means the walk can mutate `csi` and *then*
    // discover the sequence is not complete in this read, so every decline has
    // to `reset` on the way out: `Act::CsiStart` resets before calling this,
    // and the graph then rebuilds from the same bytes. Leaving half-parsed
    // parameters behind is the `csi_whole` bug this repo already paid for.
    #[cfg(not(feature = "two-pass-csi"))]
    let (off, s) = {
        let all = StreamByte::slice_from(&p.arena[body.index()..body.index() + rest.index()]);
        let mut seen = false;
        let mut acc: Option<Param> = None;
        let mut k = 0usize;
        // **Test for the final byte before testing for a digit.** `ESC [ K`
        // and `ESC [ m` put it first, which is **99.4% of `less-pager.raw`'s**
        // sequences and 50% of `git-log.raw`'s -- and asking "is it a digit"
        // first made those pay a test that can never succeed. That ordering
        // cost 2.6% on less-pager, the one payload this fusion otherwise made
        // slower. `final_byte_at`'s scalar prefix existed for the same
        // recordings and for the same reason.
        let end = loop {
            let Some(b) = all.get(k).copied() else {
                csi.reset();
                return None;
            };
            if matches!(b.raw_byte(), 0x40..=0x7e) {
                break k;
            }
            match b.as_ascii_digit() {
                Some(d) if !seen => {
                    acc = Some(match acc {
                        Some(v) => v.push(d),
                        None => Param::ZERO.push(d),
                    });
                }
                Some(_) => {
                    csi.reset();
                    return None;
                }
                None => match b.raw_byte() {
                    b';' | b':' if !seen => {
                        csi.put(acc);
                        match b.raw_byte() {
                            b':' => csi.sub_sep(),
                            _ => csi.sep(),
                        }
                        acc = None;
                    }
                    0x3c..=0x3f if k == 0 => match Marker::from_private(b) {
                        Some(m) => csi.set_marker(m),
                        None => {
                            csi.reset();
                            return None;
                        }
                    },
                    0x20..=0x2f => {
                        seen = true;
                        csi.set_inter(b);
                    }
                    _ => {
                        csi.reset();
                        return None;
                    }
                },
            }
            k += 1;
        };
        csi.put(acc);
        (end, &all[..=end])
    };
    #[cfg(feature = "two-pass-csi")]
    let off = final_byte_at(p, body, rest)?.index();
    #[cfg(feature = "two-pass-csi")]
    let s = StreamByte::slice_from(&p.arena[body.index()..body.index() + off + 1]);
    // **An intermediate ends the parameters.** After one, the graph moves to
    // `CsiInter`, where a digit is a grammar violation that lands the whole
    // sequence in `CsiIgnore`. Declining is how this path stays in step with
    // that rather than inventing a more permissive grammar.
    #[cfg(all(feature = "probe-no-csi-params", feature = "two-pass-csi"))]
    let params: &[StreamByte] = &[];
    #[cfg(all(not(feature = "probe-no-csi-params"), feature = "two-pass-csi"))]
    let params = &s[..off];
    // **Accumulate in a register; store when the parameter ends.**
    //
    // The walk was a read-modify-write of `Option<Param>` per digit: a 4-byte
    // load, a match on the `Option`, the multiply-add, a 4-byte store -- and
    // `btop.raw`'s sequences are eleven bytes of which about nine are digits.
    // Priced by stubbing it, the walk read 32% of btop -- an upper bound, and
    // the stub also removes the truecolor `SGR` chain; two instruments bracket
    // it at **19.5-33.5%**, see `CLAUDE.md`. A parameter only
    // has to reach the array when it *ends*, at a separator or at the final
    // byte, so the digits fold in a register and the array is touched once a
    // parameter instead of once a digit. `baud-core2` has always done this.
    //
    // **One loop, and splitting it into phases lost.** The marker can only be
    // at position zero and `seen_inter` only ever goes from false to true, so
    // hoisting both out looked like removing two branches a byte from a walk
    // that is 32% of `btop.raw`. It measures **-5% on asciiquarium and -7% on
    // git-log**: the branches are perfectly predictable, and the loop setup a
    // second phase needs costs more than they do.
    //
    // A three-byte lookup table was tried here too and is **-14% on btop**.
    // This loop is short and local, so a table removes arithmetic rather than
    // iterations, which is not where a table wins.
    // **The honest price of the walk, and the reason it needed a second
    // instrument.** `probe-no-csi-params` feeds the walk an empty slice, so
    // nothing is stored and `ESC[38;2;204;204;204m` -- **47% of btop's
    // sequences** -- never parses a colour, never folds a `StyleWord` and never
    // interns one. That arm removes the walk *and* the whole truecolor SGR
    // chain, and `cells_written` cannot see it because the same glyphs land in
    // the same cells with a different `StyleId`. Its 33% is an upper bound.
    //
    // This arm is additive instead: the identical walk, run once into a
    // scratch and discarded, so every counter and every stored parameter is
    // unchanged and the delta is the walk's own marginal cost.
    // **The scratch alone, so it can be subtracted.** `Csi::NEW` is
    // `[None; NPARAM]` -- about 128 bytes zeroed -- and the walk being priced
    // never pays it, so the additive arm above over-prices the walk by exactly
    // this. Second contamination found in one instrument today, and the same
    // cause both times: an arm that does more than the thing it names.
    // **A repeated `SGR` is answered from its bytes.**
    //
    // Two compares and a call, and everything else is out of line. `fast_csi`
    // is `#[inline(always)]` into the per-byte loop, and this repo has
    // measured three times that whatever lands in that body is paid by every
    // payload whether or not it runs -- the first version of this probe cost
    // **5% on asciiquarium**, whose sequences are all too short to reach it.
    #[cfg(feature = "sgr-table")]
    if s[off].raw_byte() == b'm' && off + 1 >= SGR_MIN_BYTES && sgr_from_table(p, &s[..=off]) {
        #[cfg(feature = "diag")]
        {
            p.sgr_hit += 1;
        }
        return Some(ReadLen::from_index(off + 2));
    }
    // **Ceiling arm: every plain `SGR` free.** Incorrect by construction --
    // the rendition never changes, so the screen is one colour -- and it
    // prices the whole `SGR` route: the parameter walk, `SgrAttr::parse`, the
    // fold and the intern. It is the bound on any scheme that answers a
    // repeated `SGR` byte-string from a table, and it must be multiplied by
    // the achievable hit rate before it means anything.
    //
    // `cells_written` is unchanged by it, which is exactly why that counter
    // cannot be the check here: the same glyphs land in the same cells and
    // only the `StyleId` differs.
    #[cfg(feature = "ceiling-free-sgr")]
    if s[off].raw_byte() == b'm' && off + 1 <= 32 {
        return Some(ReadLen::from_index(off + 2));
    }
    #[cfg(feature = "probe-csi-scratch")]
    {
        let scratch = Csi::NEW;
        core::hint::black_box(&scratch);
    }
    #[cfg(feature = "probe-double-params")]
    {
        let mut scratch = Csi::NEW;
        let mut si = false;
        let mut sc: Option<Param> = None;
        for (k, b) in s.iter().enumerate() {
            match b.as_ascii_digit() {
                Some(d) if !si => {
                    sc = Some(match sc {
                        Some(v) => v.push(d),
                        None => Param::ZERO.push(d),
                    });
                }
                Some(_) => break,
                None => match b.raw_byte() {
                    b';' | b':' if !si => {
                        scratch.put(sc);
                        match b.raw_byte() {
                            b':' => scratch.sub_sep(),
                            _ => scratch.sep(),
                        }
                        sc = None;
                    }
                    0x3c..=0x3f if k == 0 => match Marker::from_private(*b) {
                        Some(m) => scratch.set_marker(m),
                        None => break,
                    },
                    0x20..=0x2f => {
                        si = true;
                        scratch.set_inter(*b);
                    }
                    _ => break,
                },
            }
        }
        scratch.put(sc);
        core::hint::black_box(&scratch);
    }
    #[cfg(feature = "two-pass-csi")]
    let mut seen_inter = false;
    #[cfg(feature = "two-pass-csi")]
    let mut cur: Option<Param> = None;
    #[cfg(feature = "two-pass-csi")]
    for (k, b) in params.iter().enumerate() {
        match b.as_ascii_digit() {
            Some(d) if !seen_inter => {
                cur = Some(match cur {
                    Some(v) => v.push(d),
                    None => Param::ZERO.push(d),
                });
            }
            Some(_) => return None,
            None => match b.raw_byte() {
                b';' | b':' if !seen_inter => {
                    csi.put(cur);
                    match b.raw_byte() {
                        b':' => csi.sub_sep(),
                        _ => csi.sep(),
                    }
                    cur = None;
                }
                0x3c..=0x3f if k == 0 => match Marker::from_private(*b) {
                    Some(m) => csi.set_marker(m),
                    None => return None,
                },
                0x20..=0x2f => {
                    seen_inter = true;
                    csi.set_inter(*b);
                }
                _ => return None,
            },
        }
    }
    #[cfg(feature = "two-pass-csi")]
    csi.put(cur);

    // **The commonest sequence must not enter a `#[cold]` function.** `SGR`
    // is 86% of `asciiquarium.raw`'s dispatches, 63% of btop's and 100% of
    // git-log's, and the short-circuit for it lived *inside* `csi_final` --
    // which is `#[cold]` and out of line, so every one of them paid a call
    // into a cold frame to be told it did not need the grammar. Inlining
    // `fast_csi` itself was worth 3-5% for the same reason.
    let f = FinalByte::of_final(s[off]);
    // **Commit the node here, before the dispatch, so the dispatch can
    // override it.** `DECANM` leaves ANSI, and leaving is a parser transition:
    // the next byte has to be read by the VT52 graph. Setting `Node::Ground`
    // *after* `csi_final` -- which is where it used to live, in the
    // `Act::CsiStart` arm -- would undo the switch and the very next byte
    // would be parsed by the protocol the application just left.
    *node = Node::Ground;
    match csi.is_plain_sgr(f) {
        true => {
            // **Insert on the miss, from the parse that just happened.**
            // `StyleFold::derive` runs the fold on two witnesses, so this
            // costs two extra folds -- paid once per distinct sequence, 168
            // times in a `btop.raw` run, against 19,984 lookups.
            #[cfg(feature = "sgr-table")]
            if off + 1 >= SGR_MIN_BYTES {
                sgr_into_table(p, csi, &s[..=off]);
            }
            p.apply_sgr(csi)
        }
        false => csi_final(p, csi, f, node, BytePos::from_index(body.index() + off)),
    }
    // The `[`, the parameters, and the final byte.
    // **The one place this path commits.** `fast_hit` was declared, summed and
    // printed and incremented *nowhere*, so `ingest2` reported "100% declined"
    // for every recording while the node histogram showed the opposite -- the
    // `Escape` node at 0.1% against `EscStart` at 77% is a sequence being taken
    // whole. Broken instrumentation is worse than none; this repo has the rule
    // written down from `inf x amplification` and it happened again.
    #[cfg(feature = "diag")]
    {
        p.fast_hit += 1;
    }
    Some(ReadLen::from_index(off + 2))
}

/// A whole `ESC` sequence in one step, or `None` and the graph walks it.
///
/// `from` is the `ESC`. Three shapes are worth folding, and they are the three
/// the corpus sends: `ESC [ ... final`, `ESC <intermediate> <final>` -- the
/// character-set designators -- and the bare two-byte `ESC <final>`.
#[inline(always)]
fn fast_esc(
    p: &mut Paint<'_, '_>,
    csi: &mut Csi,
    from: BytePos,
    left: ReadLen,
    node: &mut Node,
    enc: &mut InputEncoding,
) -> Option<ReadLen> {
    if left.index() < 2 {
        return None;
    }
    let b1 = StreamByte::from_stream(p.arena[from.index() + 1]);
    match b1.raw_byte() {
        // `ESC [` -- hand the rest to the CSI path, which scans for the final
        // byte and parses the parameters in straight-line code.
        0x5b => {
            let n = fast_csi(
                p,
                csi,
                from.advance_read(ReadLen::ONE),
                left.less(ReadLen::ONE),
                node,
            )?;
            Some(n.plus(ReadLen::ONE))
        }
        // `ESC ( B`, `ESC ) 0` and friends: an intermediate then a final.
        0x20..=0x2f => {
            if left.index() < 3 {
                return None;
            }
            let b2 = StreamByte::from_stream(p.arena[from.index() + 2]);
            match b2.raw_byte() {
                0x30..=0x7e => {
                    csi.set_inter(b1);
                    // The same commit `fast_csi` makes, and for the same
                    // reason: this path used to leave the node to the caller,
                    // and moving that reset into the fast paths so `DECANM`
                    // could override it left these two branches setting
                    // nothing at all. The node stayed `Escape` and the next
                    // byte was parsed as an escape continuation -- charset
                    // designation and both CJK recordings went red at once.
                    *node = Node::Ground;
                    esc_final(p, csi, b2, enc);
                    Some(ReadLen::from_index(3))
                }
                _ => None,
            }
        }
        // A bare two-byte `ESC M`, `ESC 7`, `ESC c`. **Not the string
        // introducers**: `P`, `X`, `]`, `^`, `_` open a control string whose
        // body the graph has to walk.
        0x30..=0x4f | 0x51..=0x57 | 0x59 | 0x5a | 0x5c | 0x60..=0x7e => {
            *node = Node::Ground;
            esc_final(p, csi, b1, enc);
            Some(ReadLen::from_index(2))
        }
        _ => None,
    }
}

/// Answer a plain `SGR` from the byte table, or say it is not there.
///
/// Behind `sgr-table`, **off**: see the note in `CLAUDE.md`. It is correct and
/// it hits, and it is worth about 1.3% on `btop.raw` against an A/A floor of
/// 4.8% -- kept compiled so the next person re-measures rather than rebuilds.
///
/// **Out of line on purpose.** The saving is a parameter walk over eleven to
/// seventeen bytes plus `SgrAttr::parse` and the fold; a call costs a few
/// cycles against that, and keeping it out of `fast_csi`'s inlined body is
/// what stops every other payload paying for it.
#[cfg(feature = "sgr-table")]
#[inline(never)]
fn sgr_from_table(p: &mut Paint<'_, '_>, seq: &[StreamByte]) -> bool {
    if seq.len() > SGR_KEY_BYTES {
        return false;
    }
    let slot = &p.cold.sgr[SgrKey::bucket_of(seq, NSGR)];
    match slot.key.matches(seq) {
        false => false,
        true => {
            let fold = slot.fold;
            p.apply_fold(fold);
            true
        }
    }
}

/// Record what a plain `SGR` sequence means, on the miss that just parsed it.
///
/// `StyleFold::derive` runs the fold twice, on the two witnesses -- paid once
/// per distinct sequence, 168 times in a `btop.raw` run against 19,984
/// lookups.
#[cfg(feature = "sgr-table")]
#[inline(never)]
fn sgr_into_table(p: &mut Paint<'_, '_>, csi: &Csi, seq: &[StreamByte]) {
    if let Some(key) = SgrKey::of(seq) {
        let fold = StyleFold::derive(|w| csi.fold_sgr(w));
        p.cold.sgr[SgrKey::bucket_of(seq, NSGR)] = SgrSlot { key, fold };
    }
}

// **The test module sits at the end of the file**, because clippy's
// items-after-test-module lint -- named without backticks, because the docs
// gate reads a backticked snake_case name as a test and cannot tell one from
// a lint -- is about a reader's expectation as much as the compiler's: a
// `#[cfg(test)] mod` in the middle of a file reads as the end of it, and the
// six functions that followed this one were easy to miss.

#[cfg(test)]
mod tables {
    use super::*;

    /// **All 63 finals, against the rules the table replaced.** A table may
    /// only stand in for a function if it answers the same way everywhere, and
    /// for a byte-indexed table "everywhere" is small enough to enumerate --
    /// which is the whole reason it is a table.
    #[test]
    fn the_csi_table_agrees_with_the_rules() {
        for i in 0..FinalByte::COUNT {
            let f = FinalByte::at(i);
            assert_eq!(CSI_OP[f.slot()], csi_op_of(f), "final slot {i}");
        }
    }

    /// **The finals this core recognises and does not implement are
    /// enumerable**, which is the point of an `Unsupported` variant rather than
    /// a silent `_ => {}`: the gap is a number someone can look at. Ten finals
    /// are implemented -- `H` and `f` are both CUP.
    #[test]
    fn the_unimplemented_finals_are_enumerable() {
        let n = CSI_OP.iter().filter(|o| **o == CsiOp::Unsupported).count();
        assert_eq!(n, 53, "63 finals, 10 implemented");
    }
}
