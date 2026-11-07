//! **A kitty animation kept as records** ([`FrameLayout::Rects`]), the
//! layout a picture's frames take, when its frames do not fit whole.
//!
//! A kitty image's frames are whole ([`FrameLayout::Whole`]) while a gap
//! holds them so: there an advance is a pointer's move and a frame of any
//! number can be read. When the next frame finds no such gap, the
//! animation is made records in the largest gap there is, before any image
//! is deleted for the room: each frame the rectangle in which it differs
//! from the frame before, its pixels the frame's own -- composed as kitty
//! composes them when it is received ([`put`]), so a record replaces and
//! never blends -- over a clear ground, `Y` made pixels where it is not
//! clear. `kitten icat` sends README's logo film as a frame over the one
//! before for each of its 320, the rectangle the GIF's graphic covers: the
//! store held 22 of them whole in the app's 95 MiB and refused the rest.
//!
//! **A frame is made when it is received, on the span's second canvas**
//! ([`Canvas::Build`]), which holds the last frame made; the first holds the
//! frame shown and changes only at the tick ([`ImageStore::compose_frames`]).
//! A frame over the last one made, as icat sends each, composes nothing to
//! be made.
//!
//! **A frame stays what it was made** when the frame it was made over is
//! edited (`a=f,r=`), composed into (`a=c`) or deleted, as baud's whole
//! frames do (see the module above): first each frame whose record is
//! over that one is given a record over that one's base, the two records'
//! rectangles together, its own pixel where it set one and the other's
//! where that set one. The room it takes is the arena's like a new frame's;
//! where none is found the command is refused, ENOSPC, and nothing changes.
//! A delete is refused only where the arena holds too few with the dead
//! records taken out and every image the quota could delete deleted
//! ([`ImageStore::record_delete`]).
//!
//! A scroll under side margins moves the records' pixels through the row
//! map (`Shape::shift`): an OSC 1337 GIF's, whose pixels inside the margins
//! the scroll moves within it. A kitty animation's rows never move, a kitty
//! placement keeping the text's rule (`ImageStore::scroll_rect`), so a frame
//! command composes onto its records as they were made, which a debug build
//! asserts (`ImageStore::kitty_rows_unmoved`).

use super::{
    Build, Canvas, FrameCompose, FrameDeleted, FrameDepth, FrameLayout, FrameLoad, FrameNo,
    FrameRefusal, GAP_MASK, Head, ImageAlpha, ImageId, ImageRefusal, ImageSlot, ImageStore, KEEP,
    Palette, Pixel, REC_AT, REC_BASE, REC_BLEND, REC_COLOURS, REC_GROUND, REC_HEAD, REC_SIZE,
    REC_WORD, RGB_BIT, RowMap, Shape, UNKNOWN, data_len, gap_of, lend, put,
};
use crate::style::Chan;
use zerocopy::IntoBytes;

/// A count or an offset as a word.
fn word32(v: usize) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

/// A frame word's depth bit.
fn depth_bit(d: FrameDepth) -> u32 {
    match d {
        FrameDepth::Rgba => 0,
        FrameDepth::Rgb => RGB_BIT,
    }
}

/// **The rectangle an `a=f`'s data covers in image `s`**, `x0, y0, x1, y1`,
/// clipped to the image as kitty's `compose` clips it
/// ([`ImageStore::kitty_frame`]'s whole frames clip so too).
fn data_rect(s: &ImageSlot, d: &ImageSlot, load: &FrameLoad) -> (usize, usize, usize, usize) {
    let (w, h) = (s.width.index(), s.height.index());
    let (ox, oy) = ((load.x as usize).min(w), (load.y as usize).min(h));
    let cols = (w - ox).min(d.width.index());
    let rows = (h - oy).min(d.height.index());
    (ox, oy, ox + cols, oy + rows)
}

/// **The background `Y` of a frame over none**, "a 32-bit RGBA color",
/// opaque for RGB data, as a whole frame's canvas is filled.
fn background(load: &FrameLoad) -> Pixel {
    let [r, g, b, a] = load.background.to_be_bytes();
    let (r, g, b) = (Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b));
    match load.depth {
        FrameDepth::Rgba => Pixel::straight(Chan::from_u8(a), r, g, b),
        FrameDepth::Rgb => Pixel::opaque(r, g, b),
    }
}

/// **The most words a record of `area` pixels takes**: its head and, its
/// colours at most its pixels, a table of them and an index a pixel, or
/// an ARGB word a pixel where they are more than a table holds.
fn rec_words(area: usize) -> usize {
    REC_HEAD + area + area.div_ceil(4)
}

/// **The most words a new frame's record takes** ([`rec_words`]) for what
/// it can change: the data's rectangle over a frame, the image over none,
/// whose ground is the frame's too.
pub(super) fn frame_bound(s: &ImageSlot, d: &ImageSlot, load: &FrameLoad) -> usize {
    let area = match load.base {
        0 => s.area(),
        _ => {
            let (x0, y0, x1, y1) = data_rect(s, d, load);
            (x1 - x0) * (y1 - y0)
        }
    };
    rec_words(area)
}

/// The rectangle two records cover together; nothing where neither covers
/// any.
fn union(a: Head, b: Head) -> (usize, usize, usize, usize) {
    let r = |h: Head| (h.x, h.y, h.x + h.w, h.y + h.h);
    match (a.w * a.h, b.w * b.h) {
        (0, 0) => (0, 0, 0, 0),
        (0, _) => r(b),
        (_, 0) => r(a),
        _ => (
            a.x.min(b.x),
            a.y.min(b.y),
            (a.x + a.w).max(b.x + b.w),
            (a.y + a.h).max(b.y + b.h),
        ),
    }
}

/// **A record written at the start of `out`**: frame word `word`, over
/// frame `base` from zero or none, its rectangle what `value`'s pixels
/// bound in `x0..x1` by `y0..y1` -- `None` the frame under it showing,
/// [`KEEP`] -- its table their colours, or, past [`MOST_COLOURS`], each
/// pixel `frame`'s, the frame's own. The words written, or `None` if `out`
/// holds too few.
///
/// [`MOST_COLOURS`]: super::MOST_COLOURS
fn write_rec(
    out: &mut [Pixel],
    word: u32,
    base: Option<usize>,
    rect: (usize, usize, usize, usize),
    value: impl Fn(usize, usize) -> Option<Pixel>,
    frame: impl Fn(usize, usize) -> Pixel,
) -> Option<usize> {
    let ((bx, by, bw, bh), pal) = extent(rect, &value);
    let colours = pal.colours();
    let len = REC_HEAD + data_len(bw * bh, colours);
    if len > out.len() {
        return None;
    }
    out[REC_WORD] = Pixel(word);
    out[REC_AT] = Pixel(word32(bx | by << 16));
    out[REC_SIZE] = Pixel(word32(bw | bh << 16));
    out[REC_COLOURS] = Pixel(word32(colours));
    out[REC_BASE] = Pixel(word32(base.map_or(0, |b| b + 1)));
    out[REC_GROUND] = Pixel::CLEAR;
    out[REC_BLEND] = Pixel(0);
    let d = REC_HEAD;
    match colours {
        0 => {
            let mut j = d;
            for y in by..by + bh {
                for x in bx..bx + bw {
                    out[j] = frame(x, y);
                    j += 1;
                }
            }
        }
        n => {
            out[d..d + n].copy_from_slice(&pal.table[..n]);
            let bytes = out[d + n..len].as_mut_bytes();
            let mut j = 0;
            for y in by..by + bh {
                for x in bx..bx + bw {
                    bytes[j] = match value(x, y) {
                        Some(p) => pal.index(p),
                        None => KEEP,
                    };
                    j += 1;
                }
            }
            bytes[j..].fill(KEEP);
        }
    }
    Some(len)
}

/// **What [`write_rec`] writes in `x0..x1` by `y0..y1`**: the rectangle
/// the pixels `value` sets bound, as `x`, `y`, `w`, `h`, and their colours.
fn extent(
    (x0, y0, x1, y1): (usize, usize, usize, usize),
    value: impl Fn(usize, usize) -> Option<Pixel>,
) -> ((usize, usize, usize, usize), Palette) {
    let mut pal = Palette::NEW;
    let (mut bx0, mut by0, mut bx1, mut by1) = (usize::MAX, usize::MAX, 0, 0);
    for y in y0..y1 {
        for x in x0..x1 {
            if let Some(p) = value(x, y) {
                bx0 = bx0.min(x);
                by0 = by0.min(y);
                bx1 = bx1.max(x + 1);
                by1 = by1.max(y + 1);
                pal.add(p);
            }
        }
    }
    let rect = match bx0 < bx1 {
        true => (bx0, by0, bx1 - bx0, by1 - by0),
        false => (0, 0, 0, 0),
    };
    (rect, pal)
}

impl Shape {
    /// **Record `r`'s pixel at `x`, `y`**: none outside its rectangle, nor
    /// where it keeps the pixel under it.
    fn pixel_of(self, px: &[Pixel], r: Head, x: usize, y: usize) -> Option<Pixel> {
        if x < r.x || y < r.y || x >= r.x + r.w || y >= r.y + r.h {
            return None;
        }
        let j = (y - r.y) * r.w + (x - r.x);
        match r.colours {
            0 => Some(px[r.at + j]),
            n => match px[r.at + n..r.at + r.data_len()].as_bytes()[j] {
                KEEP => None,
                b => Some(px[r.at + usize::from(b) - 1]),
            },
        }
    }

    /// **Frame `t` composed on the build canvas where its records put it**,
    /// the row map not read: each record of its chain from its root, the
    /// canvas then held as none, since every other compose onto it is
    /// through the map.
    fn compose_stored(self, px: &mut [Pixel], t: usize) {
        let mut len = 1;
        let mut f = t;
        while let Some(p) = self.parent(px, f) {
            len += 1;
            f = p;
        }
        for i in (0..len).rev() {
            let mut g = t;
            for _ in 0..i {
                g = self.parent(px, g).unwrap_or(g);
            }
            self.apply_stored(px, g, Canvas::Build);
        }
        px[self.held_at(Canvas::Build)] = Pixel(UNKNOWN);
    }

    /// **Frame `k`'s record made again at `at`**, its pixels in `x0..x1` by
    /// `y0..y1` `f`'s -- handed the span before `at`, the position and the
    /// frame's pixel there -- and the rest as they were, over the base it
    /// had: the build canvas composed to `k` first, and each canvas that
    /// holds `k` given the new pixels. Where the record ends, or `None` if
    /// the span holds no room for it.
    fn rewrite(
        self,
        px: &mut [Pixel],
        k: usize,
        at: usize,
        (x0, y0, x1, y1): (usize, usize, usize, usize),
        f: impl Fn(&[Pixel], usize, usize, Pixel) -> Pixel,
    ) -> Option<usize> {
        if x0 >= x1 || y0 >= y1 {
            return Some(at);
        }
        self.compose_on(px, k, Canvas::Build);
        let old = self.head(px, k);
        let new = Head {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
            ..old
        };
        let word = px[self.record(px, k) + REC_WORD].0;
        let (bc, w) = (self.canvas_at(Canvas::Build), self.w);
        let inside = |x: usize, y: usize| x >= x0 && x < x1 && y >= y0 && y < y1;
        let (lo, hi) = px.split_at_mut(at);
        let len = {
            let lo = &*lo;
            write_rec(
                hi,
                word,
                old.base.checked_sub(1),
                union(old, new),
                |x, y| match inside(x, y) {
                    true => Some(f(lo, x, y, lo[bc + y * w + x])),
                    false => self.pixel_of(lo, old, x, y),
                },
                |x, y| match inside(x, y) {
                    true => f(lo, x, y, lo[bc + y * w + x]),
                    false => lo[bc + y * w + x],
                },
            )?
        };
        let shown = self.holds(lo, Canvas::Shown) as usize == k;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * w + x;
                let v = f(lo, x, y, lo[bc + i]);
                lo[bc + i] = v;
                if shown {
                    lo[i] = v;
                }
            }
        }
        lo[self.table_at() + k] = Pixel(word32(at));
        Some(at + len)
    }
}

impl<'b> ImageStore<'b> {
    /// **Whether a canvas is owed its compose**: the frame to show moved
    /// since its canvas was composed -- by a jump, or by a frame command
    /// on records -- and no tick has run since.
    pub(super) fn compose_owed(&self) -> bool {
        self.slots.iter().any(|s| {
            !s.is_empty()
                && s.anim.layout == FrameLayout::Rects
                && Shape::of(s).composed(&self.pixels[s.start()..s.end()]) != s.anim.shown as usize
        })
    }

    /// **Room in kitty image `i`'s records for `frames` frames and `words`
    /// words more**: the span moved, as a whole animation's is, to a gap
    /// with room for twice the frames and an eighth more records -- so the
    /// moves are few and the room spare at most an eighth -- kitty
    /// images but `spare` deleted for it as for a new frame, the oldest
    /// with no placement first.
    fn record_room(
        &mut self,
        i: usize,
        frames: usize,
        words: usize,
        spare: ImageId,
    ) -> Result<(), FrameRefusal> {
        let s = self.slots[i];
        let shape = Shape::of(&s);
        let (table, used, cap) = (
            s.anim.table as usize,
            s.anim.used as usize,
            s.anim.cap as usize,
        );
        if frames <= table && used + words <= cap {
            return Ok(());
        }
        let table = match frames > table {
            true => frames.max(table * 2),
            false => table,
        };
        let grown = Shape { table, ..shape };
        let (first, first_new) = (shape.first_record(), grown.first_record());
        let recs = used - first;
        let len = first_new + recs + recs / 8 + words;
        let me = ImageId::from_index(i);
        let to = self
            .gap_evicting(len, [me, spare])
            .ok_or(FrameRefusal::NoSpace)?;
        let offset = u32::try_from(to).map_err(|_| FrameRefusal::NoSpace)?;
        let s = self.slots[i];
        let (from, t) = (s.start(), shape.table_at());
        self.pixels.copy_within(from..from + t, to);
        for k in 0..s.anim.count as usize {
            let o = self.pixels[from + t + k].0 as usize;
            self.pixels[to + t + k] = Pixel(word32(o - first + first_new));
        }
        self.pixels
            .copy_within(from + first..from + used, to + first_new);
        let (lo, hi) = (s.start(), s.end());
        let t = &mut self.slots[i];
        t.offset = offset;
        t.anim.cap = word32(len);
        t.anim.table = word32(table);
        t.anim.used = word32(first_new + recs);
        self.release_free(lo, hi);
        Ok(())
    }

    /// **Kitty image `i`'s whole frames made records**, in the largest gap,
    /// with `extra` words to spare after them, each frame over the one
    /// before; the canvas shown its frame shown, the build canvas its last
    /// frame. `None`, nothing changed, if the gap holds too few.
    pub(super) fn whole_to_records(&mut self, i: usize, extra: usize) -> Option<()> {
        let s = self.slots[i];
        if s.anim.layout != FrameLayout::Whole {
            return None;
        }
        let n = s.anim.count as usize;
        let (w, h) = (s.width.index(), s.height.index());
        let area = w * h;
        let table = (2 * (n + 1)).max(8);
        let shape = Shape {
            w,
            h,
            frames: n,
            table,
            build: Build::Canvas,
        };
        let first = shape.first_record();
        let (at, len) = self.largest_gap();
        let len = len.min(u32::MAX as usize);
        if len < first + extra {
            return None;
        }
        let offset = u32::try_from(at).ok()?;
        let word0 = self.raw_word(&s, 0);
        let meta = area * s.anim.cap as usize;
        let shown = s.anim.shown as usize;
        let (from, span) = lend(self.pixels, s.start()..s.end(), at..at + len);
        let mut end = first;
        for f in 0..n {
            let word = match f {
                0 => word0,
                _ => from[meta + f - 1].0,
            };
            let frame = &from[f * area..(f + 1) * area];
            end = shape.push(
                span,
                f,
                end,
                word,
                f.checked_sub(1),
                Canvas::Build,
                (0, 0, w, h),
                |x, y, _| frame[y * w + x],
            )?;
        }
        if end + extra > len {
            return None;
        }
        span[..area].copy_from_slice(&from[shown * area..(shown + 1) * area]);
        span[area] = Pixel(word32(shown));
        let keep = (end + (end - first) / 8 + extra).min(len);
        let (lo, hi) = (s.start(), s.end());
        let t = &mut self.slots[i];
        t.offset = offset;
        t.anim.layout = FrameLayout::Rects;
        t.anim.cap = word32(keep);
        t.anim.table = word32(table);
        t.anim.used = word32(end);
        self.release_free(lo, hi);
        self.release_free(at + keep, at + len);
        self.epoch = self.epoch.bumped();
        Some(())
    }

    /// **A kitty image's rows are where its records put them**: a scroll
    /// under side margins moves a kitty placement by the text's rule and
    /// never moves its pixels within it -- only a Sixel's, an embedder's or
    /// an OSC 1337 image is split or moved within
    /// (`ImageStore::scroll_rect`) -- so a frame command composes onto the
    /// records as they were made. Were a kitty image ever moved within, its
    /// records would have to be made again from the frames as shown before
    /// the command, and this says so in a debug build.
    fn kitty_rows_unmoved(&self, i: usize) {
        let s = self.slots[i];
        debug_assert!(
            Shape::of(&s).row_map(&self.pixels[s.start()..s.end()]) == RowMap::Same,
            "kitty image {i}'s rows were moved under its records"
        );
    }

    /// **`a=f`'s new frame, kitty image `i`'s kept as records**, from data
    /// slot `j`: made as a whole frame is made -- the data as sent for a
    /// whole-image frame over none, else onto the base frame `c` or the
    /// background `Y`, blended unless `X=1` or the data is RGB -- on the
    /// build canvas, and stored as what differs from its base.
    pub(super) fn record_frame(
        &mut self,
        i: usize,
        j: usize,
        load: FrameLoad,
    ) -> Result<FrameNo, FrameRefusal> {
        let data = ImageId::from_index(j);
        self.kitty_rows_unmoved(i);
        let (s, d) = (self.slots[i], self.slots[j]);
        let k = s.anim.count as usize;
        self.record_room(i, k + 1, frame_bound(&s, &d, &load), data)?;
        let (s, d) = (self.slots[i], self.slots[j]);
        let shape = Shape::of(&s);
        let (w, h) = (shape.w, shape.h);
        let base = match load.base {
            0 => None,
            b => Some(b as usize - 1),
        };
        let under = base.map_or(load.depth, |b| self.frame_word(&s, b).1);
        let fill = background(&load);
        let full = d.width == s.width && d.height == s.height && load.x == 0 && load.y == 0;
        let as_sent = base.is_none() && full;
        let (x0, y0, x1, y1) = data_rect(&s, &d, &load);
        let within = match base {
            None if as_sent || fill != Pixel::CLEAR => (0, 0, w, h),
            _ => (x0, y0, x1, y1),
        };
        let gap = gap_of(load.gap);
        let word = gap.min(GAP_MASK) | depth_bit(under);
        if let Some(b) = base {
            shape.compose_on(&mut self.pixels[s.start()..s.end()], b, Canvas::Build);
        }
        let dw = d.width.index();
        let at = s.anim.used as usize;
        let (from, span) = lend(
            self.pixels,
            d.start()..d.start() + d.area(),
            s.start()..s.end(),
        );
        let end = shape
            .push(
                span,
                k,
                at,
                word,
                base,
                Canvas::Build,
                within,
                |x, y, below| {
                    if as_sent {
                        return from[y * dw + x];
                    }
                    let ground = match base {
                        Some(_) => below,
                        None => fill,
                    };
                    match x >= x0 && x < x1 && y >= y0 && y < y1 {
                        true => put(
                            under,
                            ground,
                            from[(y - y0) * dw + (x - x0)],
                            load.depth,
                            load.replace,
                        ),
                        false => ground,
                    }
                },
            )
            .ok_or(FrameRefusal::NoSpace)?;
        let a = &mut self.slots[i].anim;
        a.used = word32(end);
        a.count += 1;
        a.duration += u64::from(gap.min(GAP_MASK));
        if a.count == 2 {
            self.framed += 1;
        }
        self.epoch = self.epoch.bumped();
        Ok(FrameNo(word32(k + 1)))
    }

    /// **`a=f,r=`: frame `k` of kitty image `i`, kept as records, edited**
    /// by data slot `j`, composed onto it as onto a whole frame, its gap
    /// changed only by a `z` given; the frames over it first given records
    /// over its base, so they stay as they were.
    pub(super) fn record_edit(
        &mut self,
        i: usize,
        k: usize,
        j: usize,
        load: FrameLoad,
    ) -> Result<FrameNo, FrameRefusal> {
        let data = ImageId::from_index(j);
        self.kitty_rows_unmoved(i);
        self.rebase_children(i, k, data)?;
        let s = self.slots[i];
        self.record_room(i, s.anim.count as usize, rec_words(s.area()), data)?;
        let (s, d) = (self.slots[i], self.slots[j]);
        let under = self.frame_word(&s, k).1;
        let rect = data_rect(&s, &d, &load);
        let (x0, y0, dw) = (rect.0, rect.1, d.width.index());
        let at = s.anim.used as usize;
        let (from, span) = lend(
            self.pixels,
            d.start()..d.start() + d.area(),
            s.start()..s.end(),
        );
        let end = Shape::of(&s)
            .rewrite(span, k, at, rect, |_, x, y, old| {
                put(
                    under,
                    old,
                    from[(y - y0) * dw + (x - x0)],
                    load.depth,
                    load.replace,
                )
            })
            .ok_or(FrameRefusal::NoSpace)?;
        self.slots[i].anim.used = word32(end);
        if load.gap != 0 {
            self.change_gap(i, k, gap_of(load.gap));
        }
        self.epoch = self.epoch.bumped();
        Ok(FrameNo(word32(k + 1)))
    }

    /// **`a=c` onto kitty image `i` kept as records**: frame `source`'s
    /// `w` by `h` at `sx`, `sy` composed onto frame `dest` at `dx`, `dy`, as
    /// onto whole frames ([`ImageStore::kitty_compose`], which has checked
    /// the keys); the frames over the destination first given records over
    /// its base. The source's rectangle is kept after the records while the
    /// destination's record is made, then the record moved down over it.
    pub(super) fn record_compose(
        &mut self,
        i: usize,
        c: FrameCompose,
        (sx, sy): (usize, usize),
        (dx, dy, w, h): (usize, usize, usize, usize),
    ) -> Result<(), FrameRefusal> {
        let me = ImageId::from_index(i);
        self.kitty_rows_unmoved(i);
        let (src_k, dst_k) = (c.source as usize - 1, c.dest as usize - 1);
        self.rebase_children(i, dst_k, me)?;
        let s = self.slots[i];
        let wh = w * h;
        self.record_room(i, s.anim.count as usize, wh + rec_words(s.area()), me)?;
        let s = self.slots[i];
        let (_, sd) = self.frame_word(&s, src_k);
        let (_, dd) = self.frame_word(&s, dst_k);
        let shape = Shape::of(&s);
        let px = &mut self.pixels[s.start()..s.end()];
        let used = s.anim.used as usize;
        shape.compose_on(px, src_k, Canvas::Build);
        let (bc, iw) = (shape.canvas_at(Canvas::Build), shape.w);
        for y in 0..h {
            let row = bc + (sy + y) * iw + sx;
            px.copy_within(row..row + w, used + y * w);
        }
        let blend = c.blend;
        let end = shape
            .rewrite(
                px,
                dst_k,
                used + wh,
                (dx, dy, dx + w, dy + h),
                |lo, x, y, old| put(dd, old, lo[used + (y - dy) * w + (x - dx)], sd, blend),
            )
            .ok_or(FrameRefusal::NoSpace)?;
        let used = match end == used + wh {
            true => used,
            false => {
                px.copy_within(used + wh..end, used);
                px[shape.table_at() + dst_k] = Pixel(word32(used));
                end - wh
            }
        };
        self.slots[i].anim.used = word32(used);
        self.epoch = self.epoch.bumped();
        Ok(())
    }

    /// **`d=f`: frame `k` of kitty image `i`, kept as records, deleted**,
    /// as a whole frame is ([`ImageStore::kitty_delete_frame`]): the frames
    /// over it given records over its base first, then its offset word
    /// taken out and every later frame's base numbered down, the frame
    /// shown kept or the next taking its place. An animation left one frame
    /// is kept whole, its canvas. A delete brings no pixels, so the records
    /// are not made again where a scroll moved them: the frames over it are
    /// made where their records put them, as the map does not move.
    /// The frames over it take their room from
    /// the span with its dead records taken out, then from the arena with
    /// kitty images deleted for it; refused, counted as the arena's and
    /// nothing changed, only where the frame's live records and the frames
    /// over it made records over its base take more than the arena holds
    /// with every kitty image the quota could delete deleted.
    pub(super) fn record_delete(&mut self, i: usize, k: usize) -> FrameDeleted {
        let me = ImageId::from_index(i);
        if self.rebase_children(i, k, me).is_err() {
            let s = self.slots[i];
            self.refuse(ImageRefusal::Arena(s.width, s.height));
            return FrameDeleted::Nothing;
        }
        let s = self.slots[i];
        let shape = Shape::of(&s);
        let n = s.anim.count as usize;
        let (removed, _) = self.frame_word(&s, k);
        if k == 0 {
            let (g, d) = self.frame_word(&s, 1);
            self.slots[i].anim.gap0 = g;
            self.slots[i].anim.depth0 = d;
        }
        let px = &mut self.pixels[s.start()..s.end()];
        let t = shape.table_at();
        px.copy_within(t + k + 1..t + n, t + k);
        for m in 0..n - 1 {
            let r = px[t + m].0 as usize + REC_BASE;
            let b = px[r].0 as usize;
            if b > k + 1 {
                px[r] = Pixel(word32(b - 1));
            }
        }
        for c in [Canvas::Shown, Canvas::Build] {
            let at = shape.held_at(c);
            let held = px[at].0;
            px[at] = Pixel(match held {
                UNKNOWN => UNKNOWN,
                h if h as usize == k => UNKNOWN,
                h if h as usize > k => h - 1,
                h => h,
            });
        }
        let a = &mut self.slots[i].anim;
        a.count -= 1;
        a.duration = a.duration.saturating_sub(u64::from(removed));
        let last = a.count - 1;
        if a.shown > last {
            a.shown = last;
        } else if word32(k) < a.shown {
            a.shown -= 1;
        }
        let s = self.slots[i];
        let shape = Shape::of(&s);
        let px = &mut self.pixels[s.start()..s.end()];
        if shape.holds(px, Canvas::Shown) == UNKNOWN {
            shape.compose(px, s.anim.shown as usize);
        }
        if s.anim.count == 1 {
            self.framed = self.framed.saturating_sub(1);
            self.records_to_whole(i);
        }
        self.epoch = self.epoch.bumped();
        FrameDeleted::Frame
    }

    /// **The frames over frame `k` of kitty image `i` given records over
    /// its base**, so a change to `k` leaves them as they were: each one's
    /// rectangle and `k`'s together, its own pixel where it set one, else
    /// `k`'s where that set one, else the base's, each where the records put
    /// it, unmoved by the row map, which moves the new records as it moved
    /// the old. The room is the words they take, measured first
    /// ([`extent`]), and it is the span's
    /// first, its records the dead taken out ([`ImageStore::compact_records`])
    /// where it holds too few, and then the arena's ([`ImageStore::record_room`]),
    /// kitty images deleted for it. ENOSPC, nothing changed, if they find
    /// no room.
    fn rebase_children(&mut self, i: usize, k: usize, spare: ImageId) -> Result<(), FrameRefusal> {
        let s = self.slots[i];
        let shape = Shape::of(&s);
        let n = s.anim.count as usize;
        let span = &self.pixels[s.start()..s.end()];
        let kh = shape.head(span, k);
        let need: usize = (k + 1..n)
            .map(|m| shape.head(span, m))
            .filter(|m| m.base == k + 1)
            .map(|mh| {
                let value = |x, y| {
                    shape
                        .pixel_of(span, mh, x, y)
                        .or_else(|| shape.pixel_of(span, kh, x, y))
                };
                let ((_, _, w, h), pal) = extent(union(kh, mh), value);
                REC_HEAD + data_len(w * h, pal.colours())
            })
            .sum();
        if need == 0 {
            return Ok(());
        }
        if s.anim.used as usize + need > s.anim.cap as usize {
            self.compact_records(i);
        }
        self.record_room(i, n, need, spare)?;
        for m in k + 1..n {
            let s = self.slots[i];
            let shape = Shape::of(&s);
            let px = &mut self.pixels[s.start()..s.end()];
            let mh = shape.head(px, m);
            if mh.base != k + 1 {
                continue;
            }
            let kh = shape.head(px, k);
            match shape.row_map(px) {
                RowMap::Same => _ = shape.compose_on(px, m, Canvas::Build),
                RowMap::Moved => shape.compose_stored(px, m),
            }
            let at = s.anim.used as usize;
            let word = px[shape.record(px, m) + REC_WORD].0;
            let (bc, w) = (shape.canvas_at(Canvas::Build), shape.w);
            let (lo, hi) = px.split_at_mut(at);
            let len = {
                let lo = &*lo;
                write_rec(
                    hi,
                    word,
                    kh.base.checked_sub(1),
                    union(kh, mh),
                    |x, y| {
                        shape
                            .pixel_of(lo, mh, x, y)
                            .or_else(|| shape.pixel_of(lo, kh, x, y))
                    },
                    |x, y| lo[bc + y * w + x],
                )
                .ok_or(FrameRefusal::NoSpace)?
            };
            lo[shape.table_at() + m] = Pixel(word32(at));
            self.slots[i].anim.used = word32(at + len);
        }
        Ok(())
    }

    /// **Kitty image `i`'s records with the dead taken out**: each frame's
    /// record moved down over those no frame names -- an edit, a compose or
    /// a rebase writes a frame's record anew after the last and leaves the
    /// one before it named by none -- in the order they lie, and the table
    /// given where each went. A record lies after the last one written, so
    /// the records from the first to the last written are each the length
    /// its head says; a kitty record blends none ([`write_rec`], `push`), so
    /// its blend word holds which frame names it while they move.
    fn compact_records(&mut self, i: usize) {
        const NAMED: u32 = 1 << 31;
        let s = self.slots[i];
        let shape = Shape::of(&s);
        let px = &mut self.pixels[s.start()..s.end()];
        let t = shape.table_at();
        for m in 0..s.anim.count as usize {
            let r = px[t + m].0 as usize;
            px[r + REC_BLEND] = Pixel(NAMED | word32(m));
        }
        let (mut from, mut to) = (shape.first_record(), shape.first_record());
        let used = s.anim.used as usize;
        while from < used {
            let size = px[from + REC_SIZE].0 as usize;
            let area = (size & 0xFFFF) * (size >> 16);
            let len = REC_HEAD + data_len(area, px[from + REC_COLOURS].0 as usize);
            let named = px[from + REC_BLEND].0;
            if named & NAMED != 0 {
                px[from + REC_BLEND] = Pixel(0);
                px.copy_within(from..from + len, to);
                px[t + (named & !NAMED) as usize] = Pixel(word32(to));
                to += len;
            }
            from += len;
        }
        self.slots[i].anim.used = word32(to);
    }

    /// **Kitty image `i`, records of one frame, kept whole**: its canvas
    /// made that frame and the rest of its span given back, as a picture of
    /// one frame keeps its canvas alone.
    fn records_to_whole(&mut self, i: usize) {
        let s = self.slots[i];
        let shape = Shape::of(&s);
        shape.compose(&mut self.pixels[s.start()..s.end()], 0);
        let (lo, hi, area) = (s.start(), s.end(), s.area());
        let t = &mut self.slots[i];
        t.anim.layout = FrameLayout::Whole;
        t.anim.cap = 1;
        t.anim.table = 0;
        t.anim.used = 0;
        t.anim.shown = 0;
        t.alpha = ImageAlpha::of(&self.pixels[lo..lo + area]);
        self.release_free(lo + area, hi);
    }
}
