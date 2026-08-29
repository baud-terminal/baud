//! What the route mask names: the runs, and the byte path behind them.
//!
//! Free functions over [`Paint`] rather than methods on `Term`, because the
//! grid is borrowed for the whole read and `Term` cannot be touched while it
//! is. That is the point of `Paint`: the `View` setup is paid once a read
//! instead of once a run.

use crate::graph::Node;
use crate::paint::Paint;
use crate::storage::STRIDE_MAX;
use baud_domain::*;

/// A run of `k` characters of one encoded length, decoded and written in one
/// pass.
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
pub(crate) fn put_run_of(p: &mut Paint<'_>, from: BytePos, len: ByteLen, count: Count) {
    let (len, k) = (len.index(), count.index());
    #[cfg(feature = "diag")]
    {
        p.runs[k.min(7)] += 1;
        p.run_len[len.min(4)] += 1;
    }
    // Eight is all sixteen lanes can hold at the shortest length, so this
    // never spills.
    let mut cps = [Codepoint::REPLACEMENT; STRIDE_MAX];
    let mut all_wide = true;

    // **Four characters a step, in vectors, when the stride is three.** The
    // route map has already fixed every byte's position, so the swizzle
    // indices are compile-time constants and there is no boundary search --
    // which is the part of a UTF-8 decoder that resists vectorising, and the
    // reason two previous attempts at a block decoder measured *slower* than
    // one character at a time. Three-byte characters are 81% of `cat-cjk.raw`.
    let mut done = 0usize;
    if len == 3 {
        while done + 4 <= k && from.index() + 3 * done + LANE_COUNT <= p.arena.len() {
            let at = from.index() + 3 * done;
            let quad = Vectorized::<Printable>::load(StreamByte::slice_from(
                &p.arena[at..at + LANE_COUNT],
            ))
            .stride_three_quad();
            for (j, cp) in quad.iter().enumerate() {
                all_wide &= Classified::of(*cp).width(AmbiguousWidth::Narrow) == Width::WIDE;
                cps[done + j] = *cp;
            }
            done += 4;
        }
    }

    for (i, slot) in cps.iter_mut().enumerate().take(k).skip(done) {
        let at = from.index() + len * i;
        let s = StreamByte::slice_from(&p.arena[at..at + len]);
        // **Unchecked on purpose.** The route map already established that
        // `s[0]` leads a `len`-byte character and that every byte after it is
        // a continuation -- one shuffle answered that for all sixteen lanes --
        // so the checking forms would re-ask a settled question once a byte.
        let mut raw = s[0].lead_bits(len);
        for b in &s[1..] {
            raw = (raw << 6) | b.continuation_bits();
        }
        // A non-shortest form is not a character: `E0 80 AF` renders `/`
        // without this, which is the path-traversal overlong.
        // `Codepoint::decoded` is the surrogate and out-of-range test, so
        // those two rules are spelled once in the domain rather than again
        // here; the floor depends on the encoded length and so cannot live on
        // the codepoint.
        let floor = [0x80u32, 0x800, 0x10000][len - 2];
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
            all_wide &= Classified::of(cp).width(AmbiguousWidth::Narrow) == Width::WIDE;
        }
        // **Priced by adding one, not by removing it.** Removing the lookup
        // changes `all_wide` and so changes which writer takes the run --
        // the arm that tried it measured *slower* for that reason. A second
        // identical lookup leaves every counter alone and costs exactly one.
        #[cfg(feature = "probe-double-classify")]
        core::hint::black_box(Classified::of(cp));
        #[cfg(feature = "ceiling-no-classify")]
        {
            all_wide = false;
        }
        *slot = cp;
    }

    #[cfg(feature = "ceiling-no-write")]
    {
        let _ = &cps;
        p.cells = p.cells.plus(k);
        return;
    }
    #[allow(unreachable_code)]
    // **A run of one does not want the vectored writer.** `put_wide_run`
    // amortises its setup -- bounds, `phys`, the orphan repair, the extent --
    // over eight cells to a store; for a single character it pays all of that
    // for two cells. The run-length histogram says **69% of runs on
    // `cat-cjk.raw` are one character**, which is the recording alternating
    // scripts and not the block size: extending runs past the block boundary
    // moved that figure by four points.
    if k == 1 {
        let w = match all_wide {
            true => Width::WIDE,
            false => Classified::of(cps[0]).width(AmbiguousWidth::Narrow),
        };
        p.glyph(cps[0], w);
        return;
    }
    if all_wide && k > 0 && p.fits_wide(count) {
        p.wide(&cps[..k]);
        return;
    }
    // Mixed widths, or the row is about to end: one at a time, which also
    // handles the wrap.
    for cp in cps.iter().take(k) {
        let w = Classified::of(*cp).width(AmbiguousWidth::Narrow);
        p.glyph(*cp, w);
    }
}

/// A C0 control at a known offset, for the block walk.
#[inline(always)]
pub(crate) fn control(p: &mut Paint<'_>, from: BytePos) {
    execute(p, StreamByte::from_stream(p.arena[from.index()]));
}

/// One byte, off the block path. Returns how many bytes it consumed, which is
/// **never zero** -- the block loop relies on that to make progress.
#[inline(always)]
pub(crate) fn one(
    p: &mut Paint<'_>,
    node: &mut Node,
    utf8: &mut Utf8Acc,
    min: &mut Utf8Min,
    from: BytePos,
) -> ByteLen {
    let b = StreamByte::from_stream(p.arena[from.index()]);

    // Mid-character: this byte continues one, or it does not and the partial
    // character becomes a replacement.
    if !node.is_ground() {
        return match b.utf8_continuation() {
            Some(bits) => {
                *utf8 = utf8.push(bits);
                *node = node.one_fewer();
                if node.is_ground() {
                    let cp = utf8.finish(*min);
                    let w = Classified::of(cp).width(AmbiguousWidth::Narrow);
                    p.glyph(cp, w);
                }
                ByteLen::from_index(1)
            }
            None => {
                // `[[source: unicode-ch3]]` 3.9.5: a converter "must not
                // consume the successor bytes as part of the ill-formed
                // subsequence whenever those successor bytes themselves
                // constitute part of a well-formed" one. So the partial
                // character becomes a replacement and this byte is then
                // re-dispatched from ground rather than swallowed.
                *node = Node::Ground;
                p.glyph(Codepoint::REPLACEMENT, Width::NARROW);
                one(p, node, utf8, min, from)
            }
        };
    }

    if b.is_c0() {
        execute(p, b);
        return ByteLen::from_index(1);
    }
    if b.is_printable_ascii() {
        let cp = PackedCell::translate(b, p.charset);
        p.glyph(cp, Width::NARROW);
        return ByteLen::from_index(1);
    }
    match b.utf8_lead() {
        Some((rem, bits, m)) => {
            *utf8 = Utf8Acc::lead(bits);
            *min = m;
            *node = Node::owing(Pending::new(rem));
            ByteLen::from_index(1)
        }
        // A stray continuation or an illegal lead: one replacement, and the
        // next byte is not consumed with it.
        None => {
            p.glyph(Codepoint::REPLACEMENT, Width::NARROW);
            ByteLen::from_index(1)
        }
    }
}

/// A C0 control.
#[inline(always)]
fn execute(p: &mut Paint<'_>, b: StreamByte) {
    if b == StreamByte::LF || b == StreamByte::VT || b == StreamByte::FF {
        p.index();
    } else if b == StreamByte::CR {
        p.cx = Col::ZERO;
    } else if b == StreamByte::BS && p.cx.index() > 0 {
        p.cx = Col::at_index(p.cx.index() - 1);
    }
}
