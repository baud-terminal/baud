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
use crate::term::Term;
use crate::write::{control, one, put_run_of};
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
    let text = routes.is(Route::Text).bits();
    let ctrl = routes.is(Route::Control).bits();
    let cont = routes.is(Route::Cont).bits();
    let leads = [
        routes.is(Route::Lead2).bits(),
        routes.is(Route::Lead3).bits(),
        routes.is(Route::Lead4).bits(),
    ];
    let mut off = 0usize;
    while off < LANE_COUNT {
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
        put_run_of(p, from.advance(ByteLen::from_index(off)), len, k);
        off += len.index() * k.index();
    }
    ByteLen::from_index(off)
}

/// The encoded length starting at lane `off`, and how many characters of that
/// length follow back to back within the block.
///
/// Pure bit work: no lane is read individually, so the classification stays in
/// a register for the whole walk.
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
