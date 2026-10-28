//! Ingest: one lookup a character, and the loop is one line.
//!
//! **The whole decision is in the table.** `graph` says what a byte does and
//! where that leaves the parser; the action says how many bytes it eats. So a
//! three-byte character is one lookup and one step, not three -- which is what
//! makes a table-driven loop competitive with a hand-written one instead of
//! several times slower.
//!
//! This replaced a block classifier and about two hundred lines of walk: six
//! mask extractions a block, run detection, run-length extension past the
//! block boundary, whole-block fast paths and three specialised writers.
//! Everything they did is either in the table now or in one of nine action
//! arms. `CLAUDE.md` records what each of those pieces measured, because
//! several were worth real percentages and the next person should know what
//! was given up for the simplicity.

use crate::graph::{Edge, Node, split};
// `Act` names the ASCII-only ladder's arms and nothing else in this file, so
// the import carries the same gate the block does.
#[cfg(feature = "ascii-only")]
use crate::graph::Act;
use crate::paint::Paint;
use crate::term::Term;
use crate::write::{Csi, act};
use baud_domain::*;

/// Where a read's parse starts: [`Fresh`] at its first byte, every read's;
/// [`AfterStop`] after playing on a macrograph its reports stopped
/// (`Term::feed_playing_on`).
trait ReadStart {
    fn start(p: &mut Paint<'_, '_>, base: BytePos, n: ReadLen) -> ReadLen;
}

/// A read parsed from its first byte.
struct Fresh;

impl ReadStart for Fresh {
    #[inline(always)]
    fn start(_: &mut Paint<'_, '_>, _: BytePos, _: ReadLen) -> ReadLen {
        ReadLen::ZERO
    }
}

/// A read that may follow a macrograph its reports stopped
/// (`Paint::regis_play_on`): how much of it the playing on took, the whole
/// read if it stopped again.
struct AfterStop;

impl ReadStart for AfterStop {
    #[inline(always)]
    fn start(p: &mut Paint<'_, '_>, base: BytePos, n: ReadLen) -> ReadLen {
        match p.strs.regis.waits_on_replies() {
            false => ReadLen::ZERO,
            true => p.regis_play_on(base, n),
        }
    }
}

impl Term<'_> {
    /// Ingest `n` bytes starting at `base`.
    ///
    /// **`ReadLen`, not `ByteLen`.** `ByteLen` is a `u16` that saturates, and
    /// taking one here silently discarded everything past 65,535 bytes of a
    /// read -- which read as a chunk-invariance bug in the parser and was a
    /// length lost before it arrived.
    #[inline(always)]
    pub fn feed(&mut self, base: BytePos, n: ReadLen) {
        // **Level 1 under ISO 8859-1 strips the eighth bit** "before any
        // processing occurs" (`Cold::strips`), and a control in a read can
        // start or end the strip (`Paint::restrip_from`): both out of line,
        // so the read is parsed here as it always was, one compare before it
        // and one after.
        if self.cold.strips() == crate::term::Strip::Seven {
            return self.feed_stripping(base, n);
        }
        self.feed_read::<Fresh>(base, n);
        if self.cold.restrip == crate::term::Restrip::Held {
            self.feed_restripped();
        }
    }

    /// The rest a change of the strip held, parsed as the strip now says.
    #[cold]
    #[inline(never)]
    fn feed_restripped(&mut self) {
        self.cold.restrip = crate::term::Restrip::Clear;
        if let Some(r) = self.cold.width.held.take() {
            self.feed_stripping(r.at, r.len);
        }
    }

    /// A read parsed while a strip is in force or may change: a byte at a
    /// time while it strips, each stripped as it is reached, and whole once
    /// it does not; a change held the rest, which goes on here, and an
    /// embedder's hold hands the rest of the read to the embedder.
    #[cold]
    #[inline(never)]
    fn feed_stripping(&mut self, base: BytePos, n: ReadLen) {
        let (mut at, mut left) = (base, n);
        loop {
            let step = match (self.cold.strips(), left.index()) {
                (crate::term::Strip::Seven, 1..) => {
                    self.store.arena[at.index()] &= 0x7f;
                    ReadLen::ONE
                }
                _ => left,
            };
            self.feed_read_outlined(at, step);
            at = at.advance_read(step);
            left = left.less(step);
            let held = self.cold.width.held.take();
            match (self.cold.restrip, held) {
                (crate::term::Restrip::Held, Some(r)) => {
                    self.cold.restrip = crate::term::Restrip::Clear;
                    at = r.at;
                    left = r.len.plus(left);
                }
                (crate::term::Restrip::Held, None) => {
                    self.cold.restrip = crate::term::Restrip::Clear;
                }
                (crate::term::Restrip::Clear, Some(r)) => {
                    self.cold.width.held = Some(crate::term::Rest {
                        at: r.at,
                        len: r.len.plus(left),
                    });
                    return;
                }
                (crate::term::Restrip::Clear, None) => {}
            }
            if left.index() == 0 {
                return;
            }
        }
    }

    #[inline(never)]
    fn feed_read_outlined(&mut self, base: BytePos, n: ReadLen) {
        self.feed_read::<Fresh>(base, n);
    }

    /// **A macrograph its reports stopped, played on** before the rest of
    /// the read it stopped, once the embedder has sent them
    /// (`Term::take_held`): a read of no bytes that starts by playing on
    /// (`Paint::regis_play_on`), and holds no bytes if it stops again.
    #[cold]
    #[inline(never)]
    pub(crate) fn feed_playing_on(&mut self, at: BytePos) {
        self.feed_read::<AfterStop>(at, ReadLen::ZERO);
    }

    #[inline(always)]
    fn feed_read<S: ReadStart>(&mut self, base: BytePos, n: ReadLen) {
        self.bytes_in += n.index();
        // Where this read starts, for a hide looking back past it
        // (`placed.rs`).
        self.cold.placement.begin(base);
        // A rest no one took is the previous read's.
        self.cold.width.held = None;
        let mut p = Paint::new(
            &mut self.store,
            self.cy,
            self.cx,
            self.rows,
            self.cols,
            self.cell_height,
            self.cell_width,
            self.rot,
            self.style,
            self.style_raw,
            self.flags,
            self.charset,
            self.cluster,
            self.last_cp,
            &mut self.cold,
            self.reply_len,
            self.title_len,
            self.icon_len,
            self.link,
        );
        // **A read that goes on writing the status line** takes the line's
        // state back: the terminal between reads is the main display's
        // (`status.rs`). One compare a read, never a byte.
        if p.strs.status.active == baud_domain::ActiveDisplay::Status {
            p.swap_display();
        }
        // The ASCII-only ladder never leaves `Ground`, so it advances none of
        // these; the graph loop below does.
        #[cfg_attr(feature = "ascii-only", allow(unused_mut, unused_variables))]
        // **`enc` is `mut` because `ESC % @` can change it mid-read**, and a
        // stream that switches encoding between two bytes of one `read(2)`
        // must have the second byte parsed by the table the first byte
        // selected. Bound once and copied into `step`, the switch would take
        // effect on the *next* read -- which is a screen that depends on where
        // the reads fell, the exact class chunk invariance exists to catch.
        let (mut enc, mut node, mut utf8, mut min) =
            (p.cold.encoding, self.node, self.utf8, self.utf8_min);
        let csi = &mut self.csi;
        // **One induction variable, as the untyped draft had.** Carrying a
        // position *and* a remaining count, each updated per step, cost 4% of
        // `cat-cjk.raw` and **13% of `asciiquarium.raw`** -- the payload with
        // the most steps per byte. Two counters where the loop needs one is
        // not a typing cost, and the newtypes are free the moment the shape
        // matches: `advance_read` and `less` are an add and a subtract on the
        // same value the untyped loop computed.
        // **The ASCII-only arm: one table, one lookup, one cell, per byte.**
        //
        // No decode, no run scan, no multi-byte nodes -- every byte is a single
        // index into a 256-entry table and the action it names. This is the
        // O(1)-per-byte shape in its purest form, and it exists to answer what
        // that shape actually delivers before any encoding is layered on.
        //
        // Incorrect for any non-ASCII byte by construction, so `cells_written`
        // moves on a payload that has one; `ascii/cat-scroll.raw` is 403
        // non-ASCII bytes in 448,460 and `ascii/git-log.raw` 444 in 501,185.
        #[cfg(feature = "ascii-only")]
        {
            let _ = &csi;
            let mut i = 0usize;
            while i < n.index() {
                let b = StreamByte::from_stream(p.arena[base.index() + i]);
                #[cfg(feature = "diag")]
                {
                    self.iters += 1;
                }
                // **The same ceiling arm, on the ASCII loop.** The lookup and
                // the dispatch still run and nothing reaches the grid, so the
                // pair brackets what a one-byte-an-iteration loop costs
                // against what its per-cell write costs -- which is the only
                // way to tell "the table is slow" from "the write is not
                // vectored".
                #[cfg(feature = "ceiling-no-act")]
                {
                    core::hint::black_box(crate::graph::ascii(b));
                    i += 1;
                    continue;
                }
                #[allow(unreachable_code)]
                match crate::graph::ascii(b) {
                    Act::Text => p.glyph(Codepoint::from_ascii(b), Width::NARROW),
                    Act::Index => {
                        p.index();
                    }
                    Act::LineTab => p.vertical_tab(),
                    Act::Return => p.set_col(CursorCol::moved(Col::ZERO)),
                    Act::Backspace if p.cx.raw() > Col::ZERO => p.set_col(p.cx.before()),
                    _ => {}
                }
                i += 1;
            }
            // The status line's end of a read: one compare while none is shown.
            if p.strs.status.kind != baud_domain::StatusType::None {
                p.read_ends();
            }
            self.cy = p.cy;
            self.cx = p.cx;
            let rots = (p.view.rot(), p.other.rot);
            self.cells_written += p.cells.index();
            self.set_rots(rots);
            return;
        }
        // The segment arm below consumes the whole read itself and leaves
        // nothing for the main loop, so the initialiser is dead under it.
        // **A macrograph its reports stopped plays on before the read's
        // first byte** (`Paint::regis_play_on`), and holds the whole read if
        // it stops again: only in the empty read `Term::take_held` makes for
        // it (`AfterStop`), so every other read starts at zero as it always
        // did (`Fresh`).
        #[cfg_attr(feature = "ceiling-segment-cache", allow(unused_assignments))]
        #[allow(unreachable_code)]
        let mut done = S::start(&mut p, base, n);
        #[cfg(feature = "diag")]
        let mut iters = 0usize;

        // **Ceiling arm: a repeated redraw costs a hash instead of the loop.**
        //
        // A TUI redraws the same regions over and over. Split `btop.raw` at
        // every absolute cursor move -- the unit it repaints -- and **41.3% of
        // its bytes lie in segments whose exact bytes have been seen before**
        // (vim 61.7%, and asciiquarium only 5.7%). Its loop is 93% of its
        // clock, so this arm asks what is left if those bytes are hashed and
        // skipped rather than parsed.
        //
        // Incorrect by construction -- a skipped segment is never drawn, so
        // the screen is wrong and `cells_written` falls, which is the arm
        // announcing itself. The hash is *inside* the timing, so this is a
        // realistic ceiling rather than an infinite one.
        //
        // **Valid only at whole-file reads.** A segment split across two
        // `read(2)`s would be hashed as two different segments, which prices a
        // different program. Run it at `CHUNK=400000`; the same precondition
        // `probe-pinned-node` carries.
        #[cfg(feature = "ceiling-segment-cache")]
        {
            let bytes = StreamByte::slice_from(&p.arena[base.index()..base.index() + n.index()]);
            let mut at = 0usize;
            while at < bytes.len() {
                // The segment runs to the next absolute cursor move.
                let mut end = at + 1;
                while end < bytes.len() {
                    if bytes[end] == StreamByte::ESC
                        && end + 1 < bytes.len()
                        && bytes[end + 1] == StreamByte::CSI_BRACKET
                        && is_redraw_start(&bytes[end..])
                    {
                        break;
                    }
                    end += 1;
                }
                let h = SegHash::of(&bytes[at..end]);
                let slot = h.slot(SEGS);
                match self.seen[slot] == h {
                    // Seen: the whole segment is free, and the screen is wrong.
                    true => {}
                    false => {
                        self.seen[slot] = h;
                        let mut d = ReadLen::ZERO;
                        let seg = ReadLen::from_index(end - at);
                        while d < seg {
                            d = d.plus(step(
                                &mut p,
                                &mut enc,
                                &mut node,
                                &mut utf8,
                                &mut min,
                                csi,
                                base.advance_read(ReadLen::from_index(at)).advance_read(d),
                                seg.less(d),
                            ));
                        }
                    }
                }
                at = end;
            }
            done = n;
        }

        while done < n {
            #[cfg(feature = "diag")]
            {
                iters += 1;
            }
            done = done.plus(step(
                &mut p,
                &mut enc,
                &mut node,
                &mut utf8,
                &mut min,
                csi,
                base.advance_read(done),
                n.less(done),
            ));
        }

        // **`last_cp` survives the read, and it did not.** `Paint` is built
        // once per `feed` with `last_cp: None`, and nothing wrote it back --
        // so "the preceding graphic character" meant *preceding within this
        // `read(2)`*, and `ab` then `CSI 3 b` in two reads repeated nothing
        // where the same bytes in one read repeat correctly. A screen that
        // depends on where the reads fell.
        //
        // Written back here rather than kept in `Cold`, which is where it used
        // to live and cost **16% of `cat-scroll`**: this is once a read, and
        // that was once a character.
        // The status line's end of a read: one compare while none is shown.
        if p.strs.status.kind != baud_domain::StatusType::None {
            p.read_ends();
        }
        self.last_cp = p.last_cp;
        self.cy = p.cy;
        self.cx = p.cx;
        let rots = (p.view.rot(), p.other.rot);
        self.style = p.style;
        self.style_raw = p.style_raw;
        self.flags = p.flags;
        self.charset = p.charset;
        self.cluster = p.cluster;
        // Replies queued during the read, carried back like the cursor.
        self.reply_len = p.reply_len;
        self.reply_overflows += p.reply_overflows;
        // Control-string results, carried back like the cursor. The three
        // states are assignments and the four counters accumulate, which is
        // the same split every counter here has: a length is where the string
        // got to, a count is how many times something was refused.
        self.title_len = p.title_len;
        self.icon_len = p.icon_len;
        self.link = p.link;
        self.vt52_unsupported += p.vt52_unsupported;
        if p.bells > 0 {
            p.bells_read_out();
        }
        self.bells += p.bells;
        self.margin_bells += p.margin_bells;
        self.charset_unsupported += p.charset_unsupported;
        self.unsupported += p.unsupported_total;
        self.unsupported_param += p.unsupported_param;
        self.replacements += p.replacements;
        self.string_truncations += p.string_truncations;
        self.links_refused += p.links_refused;
        self.links_clipped += p.links_clipped;
        self.strings_ignored += p.strings_ignored;
        self.clipboard_reads_refused += p.clipboard_reads_refused;
        self.semantic_marks += p.semantic_marks;
        self.prompt_erases += p.prompt_erases;
        self.clipboard_writes_refused += p.clipboard_writes_refused;
        self.checksums_refused += p.checksums_refused;
        self.marks_dropped = p.clusters.refused;
        self.cells_written += p.cells.index();
        #[cfg(feature = "diag")]
        {
            self.iters += iters;
            self.table_hits += p.table_hits;
            self.hist.add(&p.hist);
            for (a, b) in self
                .unsupported_by_final
                .iter_mut()
                .zip(p.unsupported_by_final.iter())
            {
                *a += *b;
            }
            self.scroll_rot += p.scroll_rot;
            self.scroll_copy += p.scroll_copy;
            self.fast_try += p.fast_try;
            self.fast_hit += p.fast_hit;
            #[cfg(feature = "sgr-table")]
            {
                self.sgr_hit += p.sgr_hit;
            }
            for (a, b) in self.runs.iter_mut().zip(p.runs.iter()) {
                *a += *b;
            }
            for (a, b) in self.stride_scan.iter_mut().zip(p.stride_scan.iter()) {
                *a += b.get();
            }
            for (a, b) in self.run_len.iter_mut().zip(p.run_len.iter()) {
                *a += *b;
            }
        }
        // **A request that was the read's last byte holds an empty rest**:
        // the parser is still `Held`, with no byte left to take the action
        // that would have said so, and the next read goes on from the node
        // the request's sequence left (`WidthAsk::resume`), VT52's after a
        // DECANM beside it.
        if node == Node::Held {
            node = self.cold.width.resume;
            self.cold.width.held = Some(crate::term::Rest {
                at: base.advance_read(n),
                len: ReadLen::ZERO,
            });
        }
        self.node = node;
        self.utf8 = utf8;
        self.utf8_min = min;
        self.set_rots(rots);
        // **What a hold kept back was not parsed**: it is
        // neither in this read's count nor in its tail, and the feed that
        // takes it counts it (`Term::take_held`).
        let held = self.cold.width.held.map_or(ReadLen::ZERO, |r| r.len);
        self.bytes_in -= held.index();
        // The read's last bytes, for a hide in the next one to look back into:
        // one short copy a read (`placed.rs`).
        let read = &self.store.arena[base.index()..base.index() + n.less(held).index()];
        self.cold.placement.end(StreamByte::slice_from(read));
        // **`?1010`, scroll to the bottom on output**: a read sends the
        // primary's window over the history live, as xterm's scrollTtyOutput
        // does on every read from the pty (`[[source: xterm-reset]]`'s file,
        // the read loop). Once a read, never a byte; reset at power-on.
        // **Here, after the read, and not before it**: the same test at the
        // read's start moved the inlined write loop's code, +0.9% to +1.1%
        // of `ingest2`'s instructions on cat, vim and btop, out of line or
        // not; here it reads within 0.1% of none. A read that ends on the
        // alternate screen leaves the primary's window.
        if n.index() != 0
            && self.cold.screen == crate::term::Screen::Normal
            && self
                .cold
                .resources
                .is_set(baud_domain::Resource::ScrollOnOutput)
        {
            self.store.book.window = crate::storage::HistoryWindow::Live;
        }
        // **A ReGIS envelope this read cancelled takes its cursors with
        // it**, and displays the first surface again: `CAN`, `SUB` and a
        // stray `ESC` leave the string with no terminator to say so. Once a
        // read, never a byte.
        let regis = &self.store.strs.regis;
        if regis.is_drawn() || regis.surfaces().1 == baud_domain::RegisSurface::Second {
            self.regis_read_ends();
        }
        // **The fence may lag a line the cursor left, never one it is on**:
        // a fence above the line's own would let a write past a double
        // line's half (`Paint::enter_row`), and a sized line exists only
        // with `DECLRMM` reset (`[[source: dec-std-070]]` p. 5-29).
        debug_assert!(
            self.cold.fence <= self.line_fence(),
            "the fence {:?} is past the cursor line's {:?}",
            self.cold.fence,
            self.line_fence()
        );
        debug_assert!(
            self.cold.fence == self.cold.right
                || !self.cold.modes.is_set(ModeBit::SideMargins)
                || self.cold.modes.is_set(ModeBit::LineBounded),
            "a sized line's fence under side margins"
        );
    }
}

/// One step: look the byte up, move where it says, do what it says, and report
/// how much of the read it consumed.
///
/// **Never zero.** Every action eats at least the byte it was found by, so the
/// loop cannot stall on malformed input.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn step(
    p: &mut Paint<'_, '_>,
    enc: &mut InputEncoding,
    node: &mut Node,
    utf8: &mut Utf8Acc,
    min: &mut Utf8Min,
    csi: &mut Csi,
    from: BytePos,
    left: ReadLen,
) -> ReadLen {
    let b = StreamByte::from_stream(p.arena[from.index()]);
    // **Phase 0b: is the node a loop-carried chain?** `graph[node][byte]` is
    // one load whose *address* needs `node`, which the previous iteration's
    // load produced -- a load-to-load dependence once an iteration that
    // nothing can overlap. This arm indexes the Ground row unconditionally.
    //
    // **The claim that this is exact on a whole-file read is false, and the
    // counters say so.** It read: "the non-Ground nodes exist only for a
    // character a `read(2)` cut in half, so with one read there are none".
    // That is true of the *UTF-8* nodes and of nothing else -- `Escape`,
    // `CsiParam`, `CsiInter`, `CsiIgnore`, the three string bodies and
    // `StrEsc` are entered by every escape sequence in the stream, whatever
    // the read size. Pinned at
    // `Ground` an escape is never parsed and its parameter bytes are drawn as
    // text: at `CHUNK=400000`, `asciiquarium.raw` goes **3,389,132 cells to
    // 29,325,329** and `btop.raw` **7,482,240 to 53,034,740**.
    //
    // So this arm prices the node chain only on a payload with **no escapes**
    // -- `utf8/cat-cjk.raw` and `ascii/cat-scroll.raw` are the two -- and on
    // anything else it is timing a different program that happens to compile.
    // The stated precondition was checked against the UTF-8 half of the graph
    // and never against the half that produced it.
    #[cfg(feature = "probe-pinned-node")]
    let mut e = Edge::of_graph(p.graph, Node::Ground, b);
    #[cfg(not(feature = "probe-pinned-node"))]
    let mut e = Edge::of_graph(p.graph, *node, b);
    // **The graph follows the encoding, the version and a SAPV form's
    // state**: a writer of any that did not `reselect` is caught here, in
    // every debug run.
    debug_assert!(core::ptr::eq(
        p.graph,
        crate::graph::graph_of(
            *enc,
            p.cold.widths.widths(),
            crate::variants::form_read(p.cold.modes)
        )
    ));
    // **Whether a character fits is a property of the read, not of the byte**,
    // so the table cannot answer it and the loop does. A lead whose
    // continuations have not arrived becomes a `Begin`, which parks the
    // accumulator and waits -- at most once per `read(2)`.
    //
    // Bounded by the *read*, not by the arena. Bounding a forward scan by
    // `arena.len()` was a real bug here: it consumed a character before it had
    // arrived, and `cells_written` then depended on the read size -- 8,325,429
    // at 4 KB chunks against 3,322,229 at 256 KB, the same bytes and two
    // answers.
    if !left.holds(e.act.width()) {
        e = split(e.act, *node);
    }
    #[cfg(feature = "diag")]
    {
        crate::graph::count_node(&mut p.hist, *node);
        crate::graph::count_act(&mut p.hist, e.act);
    }
    *node = e.next;
    act(p, e.act, node, enc, utf8, min, csi, from, left)
}

/// How many segment hashes the ceiling arm remembers. The btop capture of
/// 2026-08-27 had 1,126 distinct redraw segments, so 4096 held them all; the
/// capture of 2026-10-28 has not been counted.
#[cfg(feature = "ceiling-segment-cache")]
pub(crate) const SEGS: usize = 4096;

/// Whether a redraw unit begins at the front of `b`: a `CSI` whose final byte
/// moves the cursor somewhere absolute (`CUP`, `VPA`/`HVP`), which is what a
/// TUI emits before repainting a region.
///
/// **It takes the slice, not an index.** The first version took a `Count`,
/// which is a `u16`, so every position past 65,535 saturated and the scan
/// inspected the wrong byte -- the arm then found 8% of `btop.raw`'s repeats
/// instead of 39% and read as a much worse ceiling than it is. Same failure as
/// `feed` taking a `ByteLen` for a read length, which this file already
/// records; a bounded type on an unbounded quantity does not fail loudly.
#[cfg(feature = "ceiling-segment-cache")]
#[inline(always)]
fn is_redraw_start(b: &[StreamByte]) -> bool {
    let mut j = 2;
    while j < b.len() && FinalByte::from_stream(b[j]).is_none() {
        j += 1;
    }
    match j < b.len() {
        false => false,
        true => matches!(
            FinalByte::from_stream(b[j]),
            Some(FinalByte::CUP) | Some(FinalByte::VPA) | Some(FinalByte::HVP)
        ),
    }
}
