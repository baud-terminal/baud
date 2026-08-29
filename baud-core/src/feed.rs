//! The ingest loop, sixteen bytes at a time.
//!
//! **This is the reason the crate exists.** Ceiling arms on the previous core
//! priced everything the parser *does* -- decode, classification, clustering,
//! marks, cell writes, scrolling -- at 45% of `cat-cjk.raw` between them.
//! Remove all of it and the recording still runs at ~660 MB/s. What was left
//! was the loop, and the corpus said its cost is per **iteration**:
//!
//! | payload | floor with every action stubbed | bytes an iteration |
//! |---|---|---|
//! | `cat`, long printable runs | 2830 | ~30 |
//! | `cat-cjk`, 2.23-byte characters | 670 | 2.23 |
//!
//! The same nothing, iterated sixteen bytes at a time, measured **14,100**.
//!
//! So the loop below advances by a *block*. Each iteration classifies sixteen
//! bytes with one `vqtbl1q_u8` -- `Route` is a function of a byte's high
//! nibble, which is why sixteen fit one shuffle -- and then spends that answer
//! on as much of the block as it can account for.
//!
//! **What it does not buy is a faster predicate.** The old scanner was already
//! free: pricing it by running it twice measured 0%. What it buys is one
//! iteration covering sixteen bytes.

use crate::paint::Paint;
use crate::storage::STRIDE_MAX;
use crate::term::Term;
use crate::write::{control, one, put_one, put_run_of};
use baud_domain::*;

impl Term<'_> {
    /// Ingest `n` bytes starting at `base`.
    ///
    /// **One borrow of the grid for the whole read.** The walk used to build a
    /// `View` per run, and on `cat-cjk.raw` a run is a handful of bytes -- six
    /// fields and a bounds re-derivation, paid thousands of times to guard
    /// stores of eight cells. `Paint` holds it once.
    #[inline(always)]
    pub fn feed(&mut self, base: BytePos, n: ByteLen) {
        self.bytes_in += n.index();
        let (from, len) = (base.index(), n.index());
        let mut p = Paint::new(
            &mut self.store,
            self.cy,
            self.cx,
            self.rows,
            self.cols,
            self.rot,
            self.style,
            self.flags,
            self.charset,
        );
        let (mut node, mut utf8, mut min) = (self.node, self.utf8, self.utf8_min);
        let mut i = 0usize;
        while i < len {
            // One classification for the whole block. A short tail at the end
            // of a read goes a byte at a time, which is at most fifteen bytes
            // per `read(2)` and so cannot be hot.
            if node.is_ground() && i + LANE_COUNT <= len {
                let routes = Vectorized::<Printable>::load(StreamByte::slice_from(
                    &p.arena[from + i..from + i + LANE_COUNT],
                ))
                .routes();
                let took = block(&mut p, base.advance(ByteLen::from_index(i)), routes);
                if !took.is_zero() {
                    i += took.index();
                    continue;
                }
            }
            i += one(
                &mut p,
                &mut node,
                &mut utf8,
                &mut min,
                base.advance(ByteLen::from_index(i)),
            )
            .index();
        }
        self.cy = p.cy;
        self.cx = p.cx;
        self.rot = p.view.rot();
        self.cells_written += p.cells.index();
        #[cfg(feature = "diag")]
        {
            for i in 0..8 {
                self.runs[i] += p.runs[i];
            }
            for i in 0..5 {
                self.run_len[i] += p.run_len[i];
            }
            self.blocks += p.blocks;
            for i in 0..10 {
                self.block_runs[i] += p.block_runs[i];
            }
        }
        self.node = node;
        self.utf8 = utf8;
        self.utf8_min = min;
    }
}

/// One block's worth of work: **as much of the sixteen bytes as the route map
/// can account for**, and zero when none of it can.
///
/// The walk is over *masks*, not over lanes. One shuffle has already answered
/// "what is each of these sixteen"; `is()` turns that into a sixteen-bit
/// integer per route, and everything below is a shift and a test in
/// general-purpose registers. Reading a lane by a runtime index instead forces
/// the vector to memory on every step, and measured **430 against 503**.
///
/// **Every route is handled here, not just the convenient ones.** A walk that
/// breaks out on a two- or four-byte lead sends that character to the byte
/// path, which then re-classifies the whole block from one byte further along
/// -- so a recording that interleaves scripts pays the classification several
/// times per character and the block buys nothing. The first version did
/// exactly that and measured identical to the core it replaces.
#[inline(always)]
fn block(p: &mut Paint<'_>, from: BytePos, routes: RouteBlock) -> ByteLen {
    // **Six mask extractions a block, and cutting them to three lost.**
    // `to_bitmask` has no single instruction on aarch64 -- a shift-narrow and
    // a move across register files -- so this looked like the walk's cost.
    // Replacing the three per-length lead masks with one `route >= Lead2`
    // compare plus a scalar nibble test to find the length measured **584-611
    // against 638-646**: the masks are paid once a *block* and amortised over
    // its five characters, and the byte test is paid once a *run* -- of which
    // 69% are a single character. Fewer vector operations, more scalar ones,
    // and the scalar ones ran five times as often.
    let text = routes.is(Route::Text).bits();
    let ctrl = routes.is(Route::Control).bits();
    let cont = routes.is(Route::Cont).bits();
    let leads = [
        routes.is(Route::Lead2).bits(),
        routes.is(Route::Lead3).bits(),
        routes.is(Route::Lead4).bits(),
    ];
    // **The shapes that dominate, each taken in one step.** The walk is
    // general and costs a `run_at` and a dispatch per run; a block that is
    // *entirely* one thing needs neither, and the `diag` histogram says those
    // blocks are most of the recording -- 63% of the multi-character runs the
    // walk finds are a full block of five CJK characters, and `cat`'s blocks
    // are all text.
    //
    // Each is one compare against a constant. The masks are already in
    // registers, so a shape that does not match costs a test and nothing else.
    // **Order is by how often each fires, and it was measured.** Five CJK
    // characters first, all-text second: the reverse cost `cat-cjk` 4% for
    // the 14% it gave `cat`, because a test that rarely matches is a test the
    // common payload pays on every block. A third arm for four four-byte
    // characters -- the emoji shape -- was tried and dropped: emoji arrive as
    // ZWJ sequences with joiners between them, so the exact shape almost never
    // occurs and the test was pure cost.
    //
    // Five three-byte characters fill fifteen lanes; the sixteenth begins a
    // run the next block continues.
    if leads[1] & 0x7fff == 0x1249 && cont & 0x7fff == 0x6db6 {
        put_run_of(p, from, ByteLen::from_index(3), Count::of(5));
        return ByteLen::from_index(15);
    }
    if text == 0xffff {
        p.text(from, ByteLen::from_index(LANE_COUNT));
        return ByteLen::from_index(LANE_COUNT);
    }

    let mut off = 0usize;
    #[cfg(feature = "diag")]
    let mut iters = 0usize;
    while off < LANE_COUNT {
        #[cfg(feature = "diag")]
        {
            iters += 1;
        }
        let here = 1u32 << off;
        if text & here != 0 {
            // The first non-text lane at or after `off`. A whole block of text
            // leaves `off` at sixteen and the run continues next iteration.
            let n = ((!text >> off).trailing_zeros() as usize).min(LANE_COUNT - off);
            p.text(
                from.advance(ByteLen::from_index(off)),
                ByteLen::from_index(n),
            );
            off += n;
            continue;
        }
        if ctrl & here != 0 {
            control(p, from.advance(ByteLen::from_index(off)));
            off += 1;
            continue;
        }
        // **How many back-to-back characters of one encoded length start
        // here.** Not a homogeneity gate on character class: every BMP
        // character above `U+07FF` is three bytes, so CJK, Hangul and box
        // drawing all qualify together -- which is the condition a real
        // recording meets and one character class is not.
        let (len, k) = run_at(&leads, cont, off);
        if k.index() == 0 {
            break;
        }
        // **A run that reaches the end of the block is probably not the end of
        // the run.** Sixteen bytes hold at most five three-byte characters, and
        // the histogram says 25% of runs are exactly five -- which is the lane
        // count talking, not the text. Extending past the boundary is what
        // makes a run's length a property of the recording, and it is what the
        // vectored decode and the vectored write both need to engage.
        let mut n = k.index();
        #[cfg(not(feature = "no-extend"))]
        if off + len.index() * n == LANE_COUNT {
            // Bounded by what the decode buffer has left, not by `STRIDE_MAX`
            // alone -- the block already contributed `n` and the first version
            // added another 31 to it, which overran the buffer. The five short
            // tests passed and the recording panicked.
            n += extend(
                p.arena,
                from.advance(ByteLen::from_index(off + len.index() * n)),
                len,
                Count::of(STRIDE_MAX - n),
            )
            .index();
        }
        let at = from.advance(ByteLen::from_index(off));
        match n {
            1 => put_one(p, at, len),
            _ => put_run_of(p, at, len, Count::of(n)),
        }
        off += len.index() * n;
    }
    #[cfg(feature = "diag")]
    {
        p.blocks += 1;
        p.block_runs[iters.min(9)] += 1;
    }
    ByteLen::from_index(off)
}

/// How many more characters of `len` bytes follow at `at`, past the block the
/// classifier covered.
///
/// **Three compares a character, not a decode.** This is a scan and this repo
/// records that "a path that decides by scanning either commits or wastes the
/// scan" -- here it commits: every character counted is decoded and written by
/// the caller immediately, so the scan is never thrown away.
///
/// Bounded by `limit` -- what the caller's decode buffer has left after the
/// block's own contribution -- and by the arena, so a run at the end of a read
/// stops rather than reading a character the next `read(2)` still owes.
#[inline(always)]
fn extend(arena: &[u8], at: BytePos, len: ByteLen, limit: Count) -> Count {
    let (mut at, len, limit) = (at.index(), len.index(), limit.index());
    let mut k = 0usize;
    while k < limit && at + len <= arena.len() {
        let s = StreamByte::slice_from(&arena[at..at + len]);
        let owed = match s[0].utf8_lead() {
            Some((rem, _, _)) => usize::from(rem) + 1,
            None => break,
        };
        if owed != len || s[1..].iter().any(|b| b.utf8_continuation().is_none()) {
            break;
        }
        k += 1;
        at += len;
    }
    Count::of(k)
}

/// The encoded length starting at lane `off`, and how many characters of that
/// length follow back to back within the block.
///
/// Pure bit work: no lane is read individually, so the classification stays in
/// a register for the whole walk.
///
/// **A branchless form of this was written and is flat.** A run of one encoded
/// length has a constant bit pattern -- at stride three the leads sit on lanes
/// 0, 3, 6, 9, 12, which is `0x1249` -- so "where does the pattern break" is
/// one exclusive-or and a `trailing_zeros`, against a loop that goes round
/// once a character. It measured 628-636 against this loop's 623-640, ranges
/// overlapping, so there is no result and the simpler form stays.
///
/// The first version of it measured **580-591**, a clear loss, for a reason
/// worth carrying: `first / len` with the length from a runtime match is an
/// *integer divide*, twenty-odd cycles on this core. Choosing the pattern and
/// the division together, so each arm sees its own constant, recovered all of
/// it. A bit trick that ends in a division by a variable is not a bit trick.
#[inline(always)]
fn run_at(leads: &[u32; 3], cont: u32, off: usize) -> (ByteLen, Count) {
    let here = 1u32 << off;
    let len = match () {
        _ if leads[0] & here != 0 => 2,
        _ if leads[1] & here != 0 => 3,
        _ if leads[2] & here != 0 => 4,
        // A stray continuation, a `DEL`, or a lead the nibble allows and UTF-8
        // does not. The byte path answers for those.
        _ => return (ByteLen::ZERO, Count::ZERO),
    };
    let lead = leads[len - 2];
    let mut k = 0usize;
    let mut o = off;
    while o + len <= LANE_COUNT {
        if lead & (1 << o) == 0 {
            break;
        }
        // Every byte after the lead must be a continuation, or the character
        // is malformed and the byte path owes it a replacement.
        let tail = ((1u32 << (len - 1)) - 1) << (o + 1);
        if cont & tail != tail {
            break;
        }
        k += 1;
        o += len;
    }
    (ByteLen::from_index(len), Count::of(k))
}
