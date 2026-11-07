//! **kitty's animation frames, in the image's own span**
//! (`[[source: kitty-graphics]]`, "Animation"): the frames after the first
//! are laid after it in the arena, each the whole image coalesced, with a
//! word for each one's gap and depth after the last frame there is room
//! for, so an animation takes one slot of the slot table however many
//! frames it has and is bounded by the arena. A span with no room for
//! another frame moves to a gap twice its size, or one frame larger where
//! that is all there is, older images with no placement deleted to make
//! the room as for a new image ("delete older images to make space"), the
//! animation's own never. The old span is live while it moves and the
//! incoming frame's data needs room too, so kept whole an animation holds
//! fewer than half the arena's frames: measured before frames were kept as
//! records, a 32x32 animation in an arena of 12 frames and 64 words took 5
//! frames and the sixth was ENOSPC, and so did a 1920x1080 one in the
//! app's 95 MiB. A frame no gap holds whole makes the animation records
//! instead (`records`); kitty keeps frames on disk under a quota of five
//! times its image quota ("Image persistence and storage quotas"), its
//! practice and not followed.
//!
//! **Every kitty frame is a snapshot coalesced when it is made**, stored
//! whole while a gap holds the animation's frames whole, and past that as
//! what each frame changed, the layout a picture's frames take (`records`,
//! the module below, and the paragraph after this one); a frame's pixels
//! whole are what a renderer draws, as every image's are, and a frame kept
//! so is drawn from its canvas. The text says only that frames are "created by composing
//! the transmitted data onto a background canvas". kitty keeps a frame
//! as its data over its base frame and coalesces when it shows it
//! (`[[source: kitty-graphics-c]]` `get_coalesced_frame_data()`, a long
//! chain made a whole frame when it is made), so after its base frame is
//! edited (`a=f,r=`), composed into (`a=c`) or deleted, kitty's
//! dependent frame changes and this one does not: kitty's practice, not
//! followed. A frame is made as kitty makes it, in its order: on a
//! canvas of the base frame `c`, or of the background `Y` -- for a frame
//! that covers only part of the image, the data as sent for one that
//! covers all of it -- the data composed at `x`, `y`, alpha blended
//! unless `X=1` or the data is RGB. The blend is kitty's straight-alpha
//! over (`[[source: kitty-simd-string-h]]`
//! `blend_pixel_over_straight()`), onto an RGB frame its opaque one; an
//! RGB frame keeps no alpha, as kitty's three-byte frames keep none.
//!
//! **A picture's frames are stored as what changed** ([`FrameLayout::Rects`]):
//! an animated GIF's, decoded once, each frame the rectangle in which it
//! differs from the frame before, its changed pixels as indexes into a
//! table of their colours when they have 255 or fewer and as ARGB words
//! when they have more, and one canvas, the frame shown, which is what a
//! renderer reads -- the logical screen (`[[source: gif89a]]` 18), each
//! record's rectangle within it as 20 asks of an image's. Stored whole,
//! README's own logo film of 320 frames at 840 by 580 needed 594.7 MiB
//! and was refused; stored so, 22.6 to 22.9 MiB. A frame is only ever
//! the decoder's coalesced canvas, so a frame
//! made from one before it is a snapshot as a kitty frame is, and an
//! erase that clears cells of the picture clears them in every frame
//! (`ImageStore::clear_page_cells`), and a scroll under side margins moves
//! them in every frame, in the picture's own span (`Shape::shift`). GIF89a's disposal 3, "restore the
//! area overwritten by the graphic with what was there prior"
//! (`[[source: gif89a]]` 23 iv), is kept so: the pixels it restores are
//! the next frame's changed pixels, the prior pixels themselves, not the
//! Background Color 23 e allows a decoder that cannot keep them.
//!
//! **A frame is composed into the canvas once, at the tick before a
//! draw**, never as bytes arrive: what changes the frame to show --
//! the clock, or a jump ([`ImageStore::show_frame`]) -- sets the target,
//! and [`ImageStore::compose_frames`], which every tick runs, replays the
//! records from the frame the canvas holds, or from the last one over the
//! ground, to it. So a flood of jumps between two draws costs one compose.
//!
//! **The clock is the caller's** ([`ImageStore::animate`]): a renderer's
//! display link, or a loop's wait, says what time it is, and a frame
//! changes only then. The steps are kitty's `scan_active_animations`: one
//! frame a tick once its gap has passed, past gapless frames, a loop
//! counted at the wrap, a loading animation waiting at its last frame.

use super::{
    ImageAlpha, ImageId, ImageOwner, ImagePx, ImageRefusal, ImageSlot, ImageState, ImageStore,
    Pixel,
};
use crate::style::Chan;
use crate::time::HostTime;
use core::ops::Range;
use core::time::Duration;
use records::frame_bound;
use zerocopy::IntoBytes;

mod records;

/// **Whether finding room for a frame may delete other images**: kitty's
/// "delete older images to make space", which a frame kept as records is
/// tried before.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Evict {
    No,
    Yes,
}

/// **A frame by its 1-based number**, as the keys name frames: `1` the
/// root frame, the image as transmitted.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct FrameNo(u32);

impl FrameNo {
    /// The root frame.
    pub const ROOT: FrameNo = FrameNo(1);

    /// Frame `n`, 1-based.
    pub fn of(n: u32) -> FrameNo {
        FrameNo(n)
    }

    /// The number, for a reply.
    pub fn raw(self) -> u32 {
        self.0
    }
}

/// **Whether a frame keeps alpha**: kitty stores a frame made from RGB data
/// as three bytes a pixel (`is_opaque`), so nothing composed onto it later
/// gives it any, and data replacing pixels there gives its colour alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameDepth {
    /// Four bytes a pixel.
    Rgba,
    /// Three: every pixel opaque.
    Rgb,
}

/// The animation's state, `s` of `a=a`: "`s=1` stops the animation. `s=2`
/// runs the animation, but in *loading* mode ... `s=3` runs the animation
/// normally".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnimState {
    Stopped,
    /// At the last frame it waits "for the arrival of more frames".
    Loading,
    Running,
}

/// **Whether terminal-driven animation runs**: the system's motion
/// preference, which the embedder reads. Held, no frame changes on the
/// clock and a client's `c=` still shows at once: Apple's "reducing
/// automatic and repetitive animations" under Reduce Motion
/// (`[[source: apple-hig-accessibility]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageMotion {
    Play,
    Hold,
}

/// Whether a tick changed a frame on show: a frame the renderer owes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Animated {
    Same,
    Changed,
}

/// Why a frame command was refused, for the kitty stream to answer in
/// kitty's codes (`[[source: kitty-graphics-c]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameRefusal {
    /// The data is wider than the image.
    Wide,
    /// Taller.
    Tall,
    /// The base frame `c` names none.
    NoBase,
    /// The arena holds no room for the frame.
    NoSpace,
    /// `a=c`'s source frame names none.
    NoSource,
    /// Its destination frame names none.
    NoDest,
    /// Its destination rectangle leaves the image.
    DestOut,
    /// Its source rectangle leaves the image.
    SourceOut,
    /// One frame, and the two rectangles overlap.
    Overlap,
}

/// **An `a=f` frame's keys**, as the "Keys for animation frame loading"
/// table reads them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameLoad {
    /// `r`: the frame edited; zero, or past the last, a new frame.
    pub edit: u32,
    /// `c`: the frame a new one starts from; zero, the background.
    pub base: u32,
    /// `x`, `y`: where the data goes.
    pub x: u32,
    pub y: u32,
    /// `X=1`: replace rather than blend.
    pub replace: FrameBlend,
    /// `Y`: the background, `0xRRGGBBAA`.
    pub background: u32,
    /// `z`: the gap.
    pub gap: i32,
    /// The data's depth: RGB from `f=24`.
    pub depth: FrameDepth,
}

/// How data meets the frame under it: "either a simple replacement
/// (`X=1`) key or a full alpha blend (the default)".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameBlend {
    Blend,
    Replace,
}

/// **An `a=c` composition's keys** ("Keys for animation frame
/// composition"): `r` the source frame, `c` the destination, `w` by `h`
/// pixels (zero, the image's), `X`, `Y` the source's top-left and `x`, `y`
/// the destination's, `C=1` a replacement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameCompose {
    pub source: u32,
    pub dest: u32,
    pub w: u32,
    pub h: u32,
    pub sx: u32,
    pub sy: u32,
    pub dx: u32,
    pub dy: u32,
    pub blend: FrameBlend,
}

/// **An `a=a` command's keys** ("Keys for animation control").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameControl {
    /// `r`: the frame whose gap `z` sets.
    pub frame: u32,
    /// `z`.
    pub gap: i32,
    /// `c`: the frame made current.
    pub current: u32,
    /// `s`: 1 stop, 2 loading, 3 run.
    pub state: u32,
    /// `v`: the loops, 1 forever and `n` `n - 1` times.
    pub loops: u32,
}

/// What a frame delete did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameDeleted {
    Nothing,
    Frame,
    /// `d=F` on an image of one frame: the image is the caller's to free.
    Image,
}

/// kitty's default gap: "If not specified, frames have a default gap of
/// `40ms`".
const DEFAULT_GAP: u32 = 40;

/// A kitty frame's word: its gap in the low 31 bits.
const GAP_MASK: u32 = 0x7FFF_FFFF;

/// A picture's frame word, which no kitty command reaches: its gap in the
/// low 30 bits, as a GIF's delay, at most 65535 hundredths, takes twenty.
const PICTURE_GAP_MASK: u32 = 0x3FFF_FFFF;

/// A picture's frame word: whether it waits for the user's input.
const INPUT_BIT: u32 = 0x4000_0000;

/// Every frame word: its depth, in the top bit.
const RGB_BIT: u32 = 0x8000_0000;

/// The bits of slot `s`'s frame words a gap takes: all 31 for a kitty
/// image, 30 for a picture's, whose next bit is the input bit.
fn gap_mask(s: &ImageSlot) -> u32 {
    match s.owner {
        ImageOwner::Kitty(..) => GAP_MASK,
        _ => PICTURE_GAP_MASK,
    }
}

/// **Whether a frame waits for the user**: GIF89a's User Input Flag,
/// "processing will continue when user input is entered", or when its
/// delay expires if it has one, "whichever occurs first"
/// (`[[source: gif89a]]` 23 v).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameWait {
    /// Its gap alone paces it.
    Gap,
    /// A key or a click moves it on, or its gap, if not zero.
    Input,
}

/// Whether the user's input has arrived since the last tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum InputSeen {
    No,
    Yes,
}

/// **How an image's frames lie in its span.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FrameLayout {
    /// Each frame whole, one after another, with a word for each one's
    /// gap and depth after the last there is room for: a still image's,
    /// and a kitty animation's while a gap holds its frames so.
    Whole,
    /// **A picture's frames as what changed** ([`Shape`]): the canvas,
    /// the frame it holds, where each frame's record starts, and the
    /// records, each a rectangle over the frame before it -- or, a kitty
    /// animation's, over the frame it was made on. Never fewer than two
    /// frames: a picture of one is [`FrameLayout::Whole`], and a kitty
    /// animation left one goes back to it.
    Rects,
}

/// **One image's animation**: how many frames and how many it has room
/// for, the one shown, the root frame's gap and depth -- the others' are
/// in the span -- and `a=a`'s state. [`Frames::ONE`] for every image
/// until a frame is added.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Frames {
    count: u32,
    /// [`FrameLayout::Whole`]: the frames there is room for.
    /// [`FrameLayout::Rects`]: the words of the span, which its records
    /// decide.
    cap: u32,
    /// The frame to show. For [`FrameLayout::Rects`] the canvas holds
    /// it once [`ImageStore::compose_frames`] has run, the frame it holds
    /// being a word of the span.
    shown: u32,
    gap0: u32,
    depth0: FrameDepth,
    state: AnimState,
    /// Loops to play, zero for ever.
    max_loops: u32,
    loops: u32,
    /// When the shown frame was shown; none until a tick stamps it.
    shown_at: Option<HostTime>,
    /// The gaps summed, milliseconds: zero, every frame gapless, and
    /// nothing to run but for frames that wait for input.
    duration: u64,
    /// How many frames wait for the user's input ([`FrameWait::Input`]).
    waits: u32,
    layout: FrameLayout,
    /// [`FrameLayout::Rects`]: the frames the offset words have room
    /// for, the count for a picture's, which never gains one.
    table: u32,
    /// [`FrameLayout::Rects`]: where the records end in the span, the
    /// next one's start.
    used: u32,
}

impl Frames {
    /// One frame and no animation.
    pub(crate) const ONE: Frames = Frames {
        count: 1,
        cap: 1,
        shown: 0,
        gap0: 0,
        depth0: FrameDepth::Rgba,
        state: AnimState::Stopped,
        max_loops: 0,
        loops: 0,
        shown_at: None,
        duration: 0,
        waits: 0,
        layout: FrameLayout::Whole,
        table: 0,
        used: 0,
    };

    /// The words a span of frames `area` pixels each takes.
    pub(crate) fn span(&self, area: usize) -> usize {
        let cap = self.cap as usize;
        match self.layout {
            FrameLayout::Whole => area * cap + (cap - 1),
            FrameLayout::Rects => cap,
        }
    }

    /// How its frames lie in its span.
    pub(crate) fn layout(&self) -> FrameLayout {
        self.layout
    }

    /// The shown frame's index from zero.
    pub(crate) fn shown_index(&self) -> usize {
        self.shown as usize
    }

    /// The shown frame's number.
    pub(crate) fn shown_frame(&self) -> FrameNo {
        FrameNo(self.shown + 1)
    }

    /// How many frames it has.
    pub(crate) fn frame_count(&self) -> u32 {
        self.count
    }

    /// Whether it has frames past the first.
    pub(crate) fn animated(&self) -> bool {
        self.count > 1
    }
}

/// `x / 255` rounded, the classic division by 255 with `128` added and
/// the high byte folded back in once: exact for `x` up to 65407. kitty's
/// `div255_round` computes the same.
fn div255(x: u32) -> u32 {
    let y = x + 128;
    (y + (y >> 8)) >> 8
}

/// `num / den` to the nearest integer, a tie to the even one: kitty's
/// `lrintf((float)num / (float)denom)` bit for bit, since `num` is below
/// 2^24 and exact as a float, and no quotient below 256 that is not a tie
/// rounds to one -- the nearest a fraction over a denominator up to 65025
/// comes to a half is 0.5 / 65025, more than half a float's step there.
fn rint(num: u32, den: u32) -> u32 {
    let (n, d) = (u64::from(num), u64::from(den));
    let q = (2 * n + d) / (2 * d);
    let tie = (2 * n + d) % (2 * d) == 0;
    u32::try_from(match tie && q % 2 == 1 {
        true => q - 1,
        false => q,
    })
    .unwrap_or(255)
}

/// **The over operator on straight alpha**, Porter and Duff's, the
/// colours not premultiplied: the alpha out is `a + d * (255 - a) / 255`
/// for source alpha `a` and destination alpha `d`, each colour weighted by
/// its alpha over that sum, both clear leaving the destination as it was,
/// in integers with the rounding division by 255 ([`div255`]) and the
/// quotient rounded as [`rint`] says. Checked bit for bit against kitty's
/// `blend_pixel_over_straight()` (`[[source: kitty-simd-string-h]]`), the
/// reference it is held to, and the arithmetic re-expressed, not its code.
fn over_straight(dst: Pixel, src: Pixel) -> Pixel {
    let (a, da) = (u32::from(src.alpha().get()), u32::from(dst.alpha().get()));
    let inv = 255 - a;
    let den = a * 255 + da * inv;
    if den == 0 {
        return dst;
    }
    let (sr, sg, sb) = src.rgb();
    let (dr, dg, db) = dst.rgb();
    let mix = |s: Chan, d: Chan| {
        let num = u32::from(s.get()) * a * 255 + u32::from(d.get()) * da * inv;
        Chan::from_u8(u8::try_from(rint(num, den)).unwrap_or(255))
    };
    let alpha = Chan::from_u8(u8::try_from(div255(den)).unwrap_or(255));
    Pixel::straight(alpha, mix(sr, dr), mix(sg, dg), mix(sb, db))
}

/// **The same over onto an opaque frame**, whose alpha stays full: each
/// colour `s * a + d * (255 - a)` over 255, rounded by [`div255`]. Checked
/// against kitty's `blend_pixel_over_opaque()` (`[[source:
/// kitty-simd-string-h]]`).
fn over_opaque(dst: Pixel, src: Pixel) -> Pixel {
    let a = u32::from(src.alpha().get());
    let (sr, sg, sb) = src.rgb();
    let (dr, dg, db) = dst.rgb();
    let mix = |s: Chan, d: Chan| {
        let x = u32::from(s.get()) * a + u32::from(d.get()) * (255 - a);
        Chan::from_u8(u8::try_from(div255(x)).unwrap_or(255))
    };
    Pixel::opaque(mix(sr, dr), mix(sg, dg), mix(sb, db))
}

/// **One pixel of data onto one of a frame**, kitty's `copy_or_blend_row()`
/// for one: blended when asked and the data has alpha, else copied, an RGB
/// frame keeping its own opacity and RGB data giving its.
fn put(under: FrameDepth, dst: Pixel, src: Pixel, data: FrameDepth, blend: FrameBlend) -> Pixel {
    match (blend, data, under) {
        (FrameBlend::Blend, FrameDepth::Rgba, FrameDepth::Rgba) => over_straight(dst, src),
        (FrameBlend::Blend, FrameDepth::Rgba, FrameDepth::Rgb) => over_opaque(dst, src),
        (_, FrameDepth::Rgba, FrameDepth::Rgba) => src,
        _ => {
            let (r, g, b) = src.rgb();
            Pixel::opaque(r, g, b)
        }
    }
}

/// `z` read as a gap: "Negative values create a *gapless* frame", zero
/// the default.
fn gap_of(z: i32) -> u32 {
    match z {
        0 => DEFAULT_GAP,
        z if z < 0 => 0,
        z => z.unsigned_abs(),
    }
}

impl<'b> ImageStore<'b> {
    /// The slot of a live, whole kitty image.
    fn kitty_slot_index(&self, id: ImageId) -> Option<usize> {
        let i = id.index()?;
        let s = self.slots.get(i)?;
        (!s.is_empty() && s.state == ImageState::Ready && matches!(s.owner, ImageOwner::Kitty(..)))
            .then_some(i)
    }

    /// Frame `k`'s word, from zero, as kept: its gap, input bit and depth.
    fn raw_word(&self, s: &ImageSlot, k: usize) -> u32 {
        match k {
            0 => {
                s.anim.gap0
                    | match s.anim.depth0 {
                        FrameDepth::Rgba => 0,
                        FrameDepth::Rgb => RGB_BIT,
                    }
            }
            _ => self.pixels[word_at(s, &self.pixels[s.start()..s.end()], k)].bits(),
        }
    }

    /// Frame `k`'s word, from zero: its gap and depth.
    fn frame_word(&self, s: &ImageSlot, k: usize) -> (u32, FrameDepth) {
        let w = self.raw_word(s, k);
        let depth = match w & RGB_BIT {
            0 => FrameDepth::Rgba,
            _ => FrameDepth::Rgb,
        };
        (w & gap_mask(s), depth)
    }

    /// Whether frame `k` waits for the user's input: never a kitty
    /// image's.
    fn frame_waits(&self, s: &ImageSlot, k: usize) -> FrameWait {
        match (gap_mask(s), self.raw_word(s, k) & INPUT_BIT) {
            (GAP_MASK, _) | (_, 0) => FrameWait::Gap,
            _ => FrameWait::Input,
        }
    }

    fn set_frame_word(&mut self, i: usize, k: usize, gap: u32, depth: FrameDepth) {
        self.set_word(i, k, gap, depth, FrameWait::Gap);
    }

    fn set_word(&mut self, i: usize, k: usize, gap: u32, depth: FrameDepth, wait: FrameWait) {
        let s = self.slots[i];
        let mask = gap_mask(&s);
        let input = match (wait, mask) {
            (FrameWait::Input, PICTURE_GAP_MASK) => INPUT_BIT,
            _ => 0,
        };
        let gap = gap.min(mask);
        if k == 0 {
            self.slots[i].anim.gap0 = gap | input;
            self.slots[i].anim.depth0 = depth;
            return;
        }
        let bit = match depth {
            FrameDepth::Rgba => 0,
            FrameDepth::Rgb => RGB_BIT,
        };
        let at = word_at(&s, &self.pixels[s.start()..s.end()], k);
        self.pixels[at] = Pixel(gap | input | bit);
    }

    /// **Frame `k`'s gap set**, kitty's `change_gap`: never below zero,
    /// the sum of the gaps kept.
    fn change_gap(&mut self, i: usize, k: usize, gap: u32) {
        let s = self.slots[i];
        let (old, depth) = self.frame_word(&s, k);
        let wait = self.frame_waits(&s, k);
        self.set_word(i, k, gap, depth, wait);
        let gap = gap.min(gap_mask(&s));
        let d = &mut self.slots[i].anim.duration;
        *d = d.saturating_sub(u64::from(old)) + u64::from(gap);
    }

    /// **Mark a kitty image's root frame RGB**: transmitted as `f=24`, it
    /// keeps no alpha, as kitty's three-byte frame keeps none.
    pub fn mark_rgb(&mut self, id: ImageId) {
        if let Some(s) = id.index().and_then(|i| self.slots.get_mut(i)) {
            if !s.is_empty() {
                s.anim.depth0 = FrameDepth::Rgb;
            }
        }
    }

    /// **Room in slot `i` for `need` frames**: the span moved, its frames
    /// and their words with it, to a gap twice its frames or `need` where
    /// that is all there is, kitty images but `spare` deleted until one is
    /// free, the oldest with no placement first -- with [`Evict::No`],
    /// none deleted.
    fn frame_room(
        &mut self,
        i: usize,
        need: u32,
        spare: ImageId,
        evict: Evict,
    ) -> Result<(), FrameRefusal> {
        let s = self.slots[i];
        if need <= s.anim.cap {
            return Ok(());
        }
        let area = s.area();
        let len = |cap: u32| Frames { cap, ..s.anim }.span(area);
        let me = ImageId::from_index(i);
        let at = [need.saturating_mul(2).max(need), need]
            .into_iter()
            .find_map(|cap| {
                match evict {
                    Evict::No => self.lowest_gap(len(cap)),
                    Evict::Yes => self.gap_evicting(len(cap), [me, spare]),
                }
                .map(|at| (at, cap))
            });
        let Some((to, cap)) = at else {
            return Err(FrameRefusal::NoSpace);
        };
        let s = self.slots[i];
        let (from, frames, words) = (s.start(), s.anim.count as usize, s.anim.count as usize - 1);
        let old_meta = from + area * s.anim.cap as usize;
        self.pixels.copy_within(from..from + frames * area, to);
        if words > 0 {
            self.pixels
                .copy_within(old_meta..old_meta + words, to + area * cap as usize);
        }
        let (lo, hi) = (from, s.end());
        let t = &mut self.slots[i];
        t.offset = u32::try_from(to).map_err(|_| FrameRefusal::NoSpace)?;
        t.anim.cap = cap;
        self.release_free(lo, hi);
        Ok(())
    }

    /// **A gap of `len` words**, deleting kitty images but those in
    /// `spare` and the images their placements hang under until one opens,
    /// the oldest with no placement first, as
    /// [`ImageStore::reserve_keeping`] deletes for a new image; none
    /// deleted unless deleting them all would make room.
    fn gap_evicting(&mut self, len: usize, spare: [ImageId; 2]) -> Option<usize> {
        if let Some(at) = self.lowest_gap(len) {
            return Some(at);
        }
        // The image and the data spared, and every image a placement of
        // either hangs under, which a delete of a parent would take too.
        let kept = self.quota_kept(spare)?;
        let spared = |s: &ImageSlot| kept.has(s.start());
        let evictable = |s: &ImageSlot| s.quota_evictable() && !spared(s);
        self.lowest_gap_among(len, |s| !s.is_empty() && !evictable(s))?;
        loop {
            if let Some(at) = self.lowest_gap(len) {
                return Some(at);
            }
            let old = self.quota_victim(spared)?;
            self.free(old);
            self.images_evicted += 1;
        }
    }

    /// **`a=f`: the frame `data`'s pixels make, of kitty image `root`**,
    /// its number on success, in kitty's order
    /// (`handle_animation_frame_load_command()`): the data no wider or
    /// taller than the image; a new frame -- `r` zero or past the last --
    /// on its canvas, its gap `z`, 40 when not given; an existing frame
    /// edited in place, its gap changed only by a `z` given. `data` is the
    /// caller's to free.
    pub fn kitty_frame(
        &mut self,
        root: ImageId,
        data: ImageId,
        load: FrameLoad,
    ) -> Result<FrameNo, FrameRefusal> {
        let Some(i) = self.kitty_slot_index(root) else {
            return Err(FrameRefusal::NoSpace);
        };
        let Some(j) = data
            .index()
            .filter(|&j| j != i && !self.slots[j].is_empty())
        else {
            return Err(FrameRefusal::NoSpace);
        };
        let (s, d) = (self.slots[i], self.slots[j]);
        if d.width > s.width {
            return Err(FrameRefusal::Wide);
        }
        if d.height > s.height {
            return Err(FrameRefusal::Tall);
        }
        let count = s.anim.count;
        let fresh = load.edit == 0 || load.edit > count;
        if fresh && load.base > count {
            return Err(FrameRefusal::NoBase);
        }
        if s.anim.layout == FrameLayout::Rects {
            return match fresh {
                true => self.record_frame(i, j, load),
                false => self.record_edit(i, load.edit as usize - 1, j, load),
            };
        }
        let k = match fresh {
            true => {
                // Whole while a gap holds the frames whole; as records
                // where one does not, before any image is deleted for the
                // room; whole again, deleting, where records do not fit.
                if self.frame_room(i, count + 1, data, Evict::No).is_err() {
                    let extra = frame_bound(&s, &d, &load);
                    if self.whole_to_records(i, extra).is_some() {
                        return self.record_frame(i, j, load);
                    }
                    self.frame_room(i, count + 1, data, Evict::Yes)?;
                }
                count as usize
            }
            false => load.edit as usize - 1,
        };
        let s = self.slots[i];
        let area = s.area();
        let canvas = s.start() + k * area;
        let full = d.width == s.width && d.height == s.height && load.x == 0 && load.y == 0;
        let under = match fresh {
            false => self.frame_word(&s, k).1,
            true if load.base != 0 => {
                let b = s.start() + (load.base as usize - 1) * area;
                self.pixels.copy_within(b..b + area, canvas);
                self.frame_word(&s, load.base as usize - 1).1
            }
            true => {
                let (r, g, b, a) = (
                    (load.background >> 24) as u8,
                    (load.background >> 16) as u8,
                    (load.background >> 8) as u8,
                    load.background as u8,
                );
                let fill = match load.depth {
                    FrameDepth::Rgba => Pixel::straight(
                        Chan::from_u8(a),
                        Chan::from_u8(r),
                        Chan::from_u8(g),
                        Chan::from_u8(b),
                    ),
                    FrameDepth::Rgb => {
                        Pixel::opaque(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
                    }
                };
                self.pixels[canvas..canvas + area].fill(fill);
                load.depth
            }
        };
        // A whole-image frame of its own is the data as sent; anything
        // else is the data composed onto the canvas.
        match fresh && load.base == 0 && full {
            true => {
                let at = self.slots[j].start();
                self.pixels.copy_within(at..at + area, canvas);
            }
            false => self.compose_data(i, k, j, load, under),
        }
        let gap = gap_of(load.gap);
        match fresh {
            true => {
                self.set_frame_word(i, k, gap, under);
                let a = &mut self.slots[i].anim;
                a.count += 1;
                a.duration += u64::from(gap);
                if a.count == 2 {
                    self.framed += 1;
                }
            }
            false => {
                if load.gap != 0 {
                    self.change_gap(i, k, gap);
                }
                if self.slots[i].anim.count == 1 {
                    let s = self.slots[i];
                    self.slots[i].alpha = ImageAlpha::of(&self.pixels[s.start()..s.start() + area]);
                }
            }
        }
        self.epoch = self.epoch.bumped();
        Ok(FrameNo(k as u32 + 1))
    }

    /// Slot `j`'s pixels composed onto frame `k` of slot `i` at the load's
    /// `x`, `y`: kitty's `compose`, rows and columns clipped to the image.
    fn compose_data(&mut self, i: usize, k: usize, j: usize, load: FrameLoad, under: FrameDepth) {
        let (s, d) = (self.slots[i], self.slots[j]);
        let (w, h) = (s.width.index(), s.height.index());
        let (dw, dh) = (d.width.index(), d.height.index());
        let (ox, oy) = (load.x as usize, load.y as usize);
        let row = w.saturating_sub(ox).min(dw);
        let canvas = s.start() + k * s.area();
        let blend = load.replace;
        for y in 0..dh {
            if y + oy >= h {
                break;
            }
            for x in 0..row {
                let src = self.pixels[d.start() + y * dw + x];
                let at = canvas + (y + oy) * w + ox + x;
                self.pixels[at] = put(under, self.pixels[at], src, load.depth, blend);
            }
        }
    }

    /// **`a=c`: a rectangle of one frame composed onto another**, kitty's
    /// `handle_compose_command`: ENOENT for a frame neither names, EINVAL
    /// for a rectangle past the image and for one frame's two rectangles
    /// overlapping; blended unless `C=1` or the source is RGB.
    pub fn kitty_compose(&mut self, root: ImageId, c: FrameCompose) -> Result<(), FrameRefusal> {
        let Some(i) = self.kitty_slot_index(root) else {
            return Err(FrameRefusal::NoSource);
        };
        let s = self.slots[i];
        let count = s.anim.count;
        if c.source == 0 || c.source > count {
            return Err(FrameRefusal::NoSource);
        }
        if c.dest == 0 || c.dest > count {
            return Err(FrameRefusal::NoDest);
        }
        let (iw, ih) = (u64::from(s.width.raw()), u64::from(s.height.raw()));
        let w = match c.w {
            0 => iw,
            n => u64::from(n),
        };
        let h = match c.h {
            0 => ih,
            n => u64::from(n),
        };
        let (dx, dy, sx, sy) = (
            u64::from(c.dx),
            u64::from(c.dy),
            u64::from(c.sx),
            u64::from(c.sy),
        );
        if dx + w > iw || dy + h > ih {
            return Err(FrameRefusal::DestOut);
        }
        if sx + w > iw || sy + h > ih {
            return Err(FrameRefusal::SourceOut);
        }
        if c.source == c.dest && sx.max(dx) < sx.min(dx) + w && sy.max(dy) < sy.min(dy) + h {
            return Err(FrameRefusal::Overlap);
        }
        if s.anim.layout == FrameLayout::Rects {
            let (w, h) = (w as usize, h as usize);
            let (sx, sy, dx, dy) = (sx as usize, sy as usize, dx as usize, dy as usize);
            return self.record_compose(i, c, (sx, sy), (dx, dy, w, h));
        }
        let (src_k, dst_k) = (c.source as usize - 1, c.dest as usize - 1);
        let (_, sd) = self.frame_word(&s, src_k);
        let (_, dd) = self.frame_word(&s, dst_k);
        let stride = s.width.index();
        let (from, to) = (s.start() + src_k * s.area(), s.start() + dst_k * s.area());
        let (w, h) = (w as usize, h as usize);
        let (sx, sy, dx, dy) = (sx as usize, sy as usize, dx as usize, dy as usize);
        for y in 0..h {
            for x in 0..w {
                let src = self.pixels[from + (sy + y) * stride + sx + x];
                let at = to + (dy + y) * stride + dx + x;
                self.pixels[at] = put(dd, self.pixels[at], src, sd, c.blend);
            }
        }
        if count == 1 {
            let a = s.area();
            self.slots[i].alpha = ImageAlpha::of(&self.pixels[s.start()..s.start() + a]);
        }
        self.epoch = self.epoch.bumped();
        Ok(())
    }

    /// **`a=d,d=f` or `d=F`: frame `number` deleted**, kitty's
    /// `handle_delete_frame_command()`: the number past the last is the
    /// last and zero the root, whose place frame 2 takes; an image of one
    /// frame keeps it, and `d=F` frees that image ([`FrameDeleted::Image`],
    /// the caller's to free). The frame shown stays shown, or, deleted, the
    /// one after it takes its place, the text silent; kitty moves its index
    /// back by one and shows the one before it, its practice and not
    /// followed.
    pub fn kitty_delete_frame(
        &mut self,
        root: ImageId,
        number: u32,
        upper: FrameDeleted,
    ) -> FrameDeleted {
        let Some(i) = root.index().filter(|&i| !self.slots[i].is_empty()) else {
            return FrameDeleted::Nothing;
        };
        let s = self.slots[i];
        let count = s.anim.count;
        if s.anim.layout == FrameLayout::Rects && !matches!(s.owner, ImageOwner::Kitty(..)) {
            return FrameDeleted::Nothing;
        }
        if count == 1 {
            return upper;
        }
        if s.anim.layout == FrameLayout::Rects {
            return self.record_delete(i, number.clamp(1, count) as usize - 1);
        }
        let k = number.clamp(1, count) as usize - 1;
        let area = s.area();
        let (removed, _) = self.frame_word(&s, k);
        if k == 0 {
            let (g, d) = self.frame_word(&s, 1);
            self.slots[i].anim.gap0 = g;
            self.slots[i].anim.depth0 = d;
        }
        let n = count as usize;
        // The pixels after it down a frame, and the words after its.
        let start = s.start();
        self.pixels
            .copy_within(start + (k + 1) * area..start + n * area, start + k * area);
        let meta = start + area * s.anim.cap as usize;
        let first = k.max(1);
        if first < n - 1 {
            self.pixels
                .copy_within(meta + first..meta + n - 1, meta + first - 1);
        }
        let a = &mut self.slots[i].anim;
        a.count -= 1;
        a.duration = a.duration.saturating_sub(u64::from(removed));
        let last = a.count - 1;
        let k = k as u32;
        if a.shown > last {
            a.shown = last;
        } else if k < a.shown {
            a.shown -= 1;
        }
        if a.count == 1 {
            self.framed = self.framed.saturating_sub(1);
            // The frame left is the image now, its mark its own.
            let s = self.slots[i];
            self.slots[i].alpha = ImageAlpha::of(&self.pixels[s.start()..s.start() + area]);
        }
        self.epoch = self.epoch.bumped();
        FrameDeleted::Frame
    }

    /// **`a=a`: the animation controlled**, kitty's
    /// `handle_animation_control_command()`: frame `r`'s gap set by a `z`
    /// given, frame `c` made current, the state `s` -- a stop counting
    /// loops from none again, a start from stopped stamping the frame
    /// shown -- and `v` the loops, "`v=1` is loop infinitely, and any other
    /// positive number is loop `number - 1` times".
    pub fn kitty_animation(&mut self, root: ImageId, c: FrameControl) {
        let Some(i) = self.kitty_slot_index(root) else {
            return;
        };
        let count = self.slots[i].anim.count;
        if c.frame != 0 && c.frame <= count && c.gap != 0 {
            self.change_gap(i, c.frame as usize - 1, c.gap.max(0).unsigned_abs());
        }
        let a = &mut self.slots[i].anim;
        if c.current != 0 && c.current <= count && c.current - 1 != a.shown {
            a.shown = c.current - 1;
            a.shown_at = None;
        }
        if c.state != 0 {
            let old = a.state;
            a.state = match c.state {
                1 => AnimState::Stopped,
                2 => AnimState::Loading,
                3 => AnimState::Running,
                _ => a.state,
            };
            if old == AnimState::Stopped && a.state != AnimState::Stopped {
                a.shown_at = None;
            }
            a.loops = 0;
        }
        if c.loops != 0 {
            a.max_loops = c.loops - 1;
        }
    }

    /// Whether slot `s` runs on the clock now: kitty's
    /// `image_is_animatable`, with a placement for its "drawn".
    fn runs(s: &ImageSlot) -> bool {
        !s.is_empty()
            && s.state == ImageState::Ready
            && s.anim.count > 1
            && s.anim.state != AnimState::Stopped
            && (s.anim.duration > 0 || s.anim.waits > 0)
            && s.placements.index() > 0
            && (s.anim.max_loops == 0 || s.anim.loops < s.anim.max_loops)
    }

    /// **A tick of the clock at `now`**: each running animation whose
    /// shown frame's gap has passed moves on a frame, past gapless ones,
    /// a loop counted at the wrap and the last loop or a loading
    /// animation's last frame where it waits -- a picture's last loop on
    /// its last frame, gapless or not; a frame shown first is
    /// stamped `now`. Held, nothing moves, and the frames shown are
    /// stamped afresh when it runs again, so each gets its whole gap.
    /// One compare when no image has a second frame. A frame that waits
    /// for input ([`FrameWait::Input`]) moves on when input arrived since
    /// the last tick or its gap, if not zero, has passed, "whichever occurs
    /// first"; input that arrives while held is dropped.
    ///
    /// **Then the canvases composed** ([`ImageStore::compose_frames`]):
    /// the tick is the draw's, so a picture's frame is composed once a
    /// drawn frame, held or not, and only where its target moved; any
    /// compose, a jump's ([`ImageStore::show_frame`]) included, answers
    /// [`Animated::Changed`].
    pub fn animate(&mut self, now: HostTime, motion: ImageMotion) -> Animated {
        let was = self.held;
        self.held = motion;
        let input = core::mem::replace(&mut self.input, InputSeen::No);
        if self.framed == 0 {
            return Animated::Same;
        }
        let changed = match motion {
            ImageMotion::Hold => Animated::Same,
            ImageMotion::Play => self.advance(now, was, input),
        };
        // A jump since the last tick is composed here too, and owes a
        // frame as the clock's move does.
        match self.compose_frames() {
            0 => changed,
            _ => Animated::Changed,
        }
    }

    /// The clock's step of [`ImageStore::animate`]: each running
    /// animation's target moved on, nothing composed.
    fn advance(&mut self, now: HostTime, was: ImageMotion, input: InputSeen) -> Animated {
        let mut changed = Animated::Same;
        for i in 0..self.slots.len() {
            let s = self.slots[i];
            if !ImageStore::runs(&s) {
                continue;
            }
            let shown_at = match (was, s.anim.shown_at) {
                (ImageMotion::Play, Some(t)) => t,
                _ => {
                    self.slots[i].anim.shown_at = Some(now);
                    continue;
                }
            };
            let gap = self.frame_word(&s, s.anim.shown_index()).0;
            let due = shown_at.plus(Duration::from_millis(u64::from(gap)));
            let ready = match self.frame_waits(&s, s.anim.shown_index()) {
                FrameWait::Gap => now >= due,
                FrameWait::Input => input == InputSeen::Yes || (gap != 0 && now >= due),
            };
            if !ready {
                continue;
            }
            let mut a = s.anim;
            let mut at = a.shown;
            let advanced = loop {
                let next = (at + 1) % a.count;
                if next == 0 {
                    if a.state == AnimState::Loading {
                        break false;
                    }
                    a.loops += 1;
                    // The last loop played ends where it is -- a kitty
                    // image on the last frame with a gap, a picture on its
                    // last frame whatever its gap, the composite a stream
                    // ends on (`Gif::decode`).
                    if a.max_loops != 0 && a.loops >= a.max_loops {
                        break gap_mask(&s) != GAP_MASK && at != a.shown;
                    }
                }
                at = next;
                if self.frame_word(&s, at as usize).0 != 0
                    || self.frame_waits(&s, at as usize) == FrameWait::Input
                {
                    break true;
                }
            };
            if advanced {
                a.shown = at;
                a.shown_at = Some(now);
                changed = Animated::Changed;
            }
            self.slots[i].anim.loops = a.loops;
            self.slots[i].anim.shown = a.shown;
            self.slots[i].anim.shown_at = a.shown_at;
        }
        changed
    }

    /// **When the next frame is due after `now`**: the earliest a running
    /// animation's shown frame's gap ends, as kitty's
    /// `scan_active_animations` keeps the least gap still ahead. One not
    /// yet stamped is due at `now`. A tick at `now` comes first: what it
    /// left past due waits -- a loading animation at its last frame, the
    /// last loop played -- and is not due, so a loop that waits on this
    /// never spins. A frame waiting for input is due at `now` once input
    /// has arrived, else at its gap's end, or never with no gap. None while
    /// held or nothing runs -- but for a canvas owed its compose, a jump's
    /// or a frame command's, which is due at `now`, held or not, stopped or
    /// running, as the jump shows at once on frames kept whole.
    pub fn next_frame(&self, now: HostTime) -> Option<HostTime> {
        if self.framed == 0 {
            return None;
        }
        if self.compose_owed() {
            return Some(now);
        }
        if self.held == ImageMotion::Hold {
            return None;
        }
        self.slots
            .iter()
            .filter(|s| ImageStore::runs(s))
            .filter_map(|s| {
                let k = s.anim.shown_index();
                let gap = self.frame_word(s, k).0;
                let waits = self.frame_waits(s, k);
                match (s.anim.shown_at, waits, self.input) {
                    (None, ..) => Some(now),
                    (Some(_), FrameWait::Input, InputSeen::Yes) => Some(now),
                    (Some(_), FrameWait::Input, InputSeen::No) if gap == 0 => None,
                    (Some(t), ..) => {
                        Some(t.plus(Duration::from_millis(u64::from(gap)))).filter(|&d| d > now)
                    }
                }
            })
            .min()
    }

    /// **The user pressed a key or a button**: frames waiting for input
    /// move on at the next tick ([`ImageStore::animate`]). One compare
    /// when no image has a second frame.
    pub fn input_arrived(&mut self) {
        if self.framed > 0 {
            self.input = InputSeen::Yes;
        }
    }

    /// **Play an animation from its first frame**, `loops` times, zero for
    /// ever, as kitty's `s=3` runs one.
    pub fn play_frames(&mut self, id: ImageId, loops: u32) {
        if let Some(s) = id.index().and_then(|i| self.slots.get_mut(i)) {
            if !s.is_empty() {
                s.anim.state = AnimState::Running;
                s.anim.max_loops = loops;
                s.anim.loops = 0;
                s.anim.shown = 0;
                s.anim.shown_at = None;
            }
        }
    }

    /// **A kitty image's frames**: how many, which is shown, and the
    /// state, for a reader and a test.
    pub fn kitty_frames(&self, id: ImageId) -> Option<(u32, FrameNo, AnimState)> {
        let s = self.slots.get(id.index()?)?;
        (!s.is_empty()).then_some((s.anim.count, s.anim.shown_frame(), s.anim.state))
    }

    /// Frame `n`'s gap in milliseconds and its depth.
    pub fn frame_gap(&self, id: ImageId, n: FrameNo) -> Option<(u32, FrameDepth)> {
        let s = *self.slots.get(id.index()?)?;
        (!s.is_empty() && n.0 >= 1 && n.0 <= s.anim.count)
            .then(|| self.frame_word(&s, n.0 as usize - 1))
    }

    /// **Frame `n`'s pixels, where they are kept whole**: any frame of a
    /// kitty animation kept whole, and of one kept as rectangles, a
    /// picture's or a kitty animation's, the one its canvas holds alone.
    pub fn frame_pixels(&self, id: ImageId, n: FrameNo) -> Option<&[Pixel]> {
        let s = *self.slots.get(id.index()?)?;
        if s.is_empty() || n.0 == 0 || n.0 > s.anim.count {
            return None;
        }
        let k = match s.anim.layout {
            FrameLayout::Whole => n.0 as usize - 1,
            FrameLayout::Rects => {
                let span = &self.pixels[s.start()..s.end()];
                match Shape::of(&s).composed(span) + 1 == n.0 as usize {
                    true => 0,
                    false => return None,
                }
            }
        };
        let at = s.start() + k * s.area();
        self.pixels.get(at..at + s.area())
    }

    /// The loops counted and the loops to play, zero for ever.
    pub fn frame_loops(&self, id: ImageId) -> Option<(u32, u32)> {
        let s = self.slots.get(id.index()?)?;
        (!s.is_empty()).then_some((s.anim.loops, s.anim.max_loops))
    }
}

/// **Where frame `k`'s word is**, `k` from one, in the arena: after the
/// last whole frame there is room for, or the first word of its record.
fn word_at(s: &ImageSlot, span: &[Pixel], k: usize) -> usize {
    match s.anim.layout {
        FrameLayout::Whole => s.start() + s.area() * s.anim.cap as usize + k - 1,
        FrameLayout::Rects => s.start() + Shape::of(s).record(span, k) + REC_WORD,
    }
}

/// A record's words before its data: the frame's word (its gap, input bit
/// and depth, as a whole frame's), where its rectangle is, its size, its
/// colours, the frame it is over, the ground and the blend.
const REC_WORD: usize = 0;
/// `x | y << 16`.
const REC_AT: usize = 1;
/// `w | h << 16`.
const REC_SIZE: usize = 2;
/// How many colours its table has, 1 to [`MOST_COLOURS`]; zero, its
/// data is ARGB words.
const REC_COLOURS: usize = 3;
/// The frame it is composed over, from one; zero, over [`REC_GROUND`].
const REC_BASE: usize = 4;
/// **The ground under a record over no frame**: the canvas a frame is
/// composed onto when it has no base, as kitty's `Y` is
/// (`[[source: kitty-graphics]]`, "Keys for animation frame loading").
/// Clear for a picture's first frame.
const REC_GROUND: usize = 5;
/// Zero, its data replaces what is under it; one, alpha blended onto it.
const REC_BLEND: usize = 6;
/// The words before the data.
const REC_HEAD: usize = 7;

/// **The index that keeps the pixel under it**: a pixel of the rectangle
/// its frame did not change, or one an erase cleared in every frame.
const KEEP: u8 = 0;

/// The most colours an indexed record's table has, its indexes from one,
/// [`KEEP`] taking zero. Measured on the README logo's films, whose
/// changed pixels have at most 251 a frame, so every frame is indexed.
const MOST_COLOURS: usize = 255;

/// **The shape of a span of rectangles** ([`FrameLayout::Rects`]): the
/// canvas, `w` by `h` words; one word, the frame the canvas holds from
/// zero; one word, whether the rows are moved ([`RowMap`]); the row map,
/// two bytes a pixel, the row of the canvas each stored pixel is shown
/// on; `frames` words, each the offset of a frame's record from the
/// span's start; then the records, a [`REC_HEAD`] of words and the data
/// -- `w * h` ARGB words of the rectangle, or a table of its colours and
/// a byte an index of each of its pixels, four to a word.
///
/// **The row map is what a scroll under side margins moves**
/// ([`Shape::shift`]): the records stay as they were made, every frame's
/// pixels are drawn through the map, and the canvas is moved as base
/// moved each whole frame, in place, so a picture moves within its own
/// span whatever room the arena has. A scroll moves pixels up or down a
/// column only, so the map keeps a pixel's column and holds its row.
///
/// **A kitty image's span has a second canvas** ([`Build`]) after the row
/// map, and room in its offset words for more frames than it has
/// (`table`): its frames arrive one at a time, each made over the frame
/// before as it is sent, and that canvas holds the last one made, the base
/// of the next, while the first holds the frame shown. One canvas for both
/// replays the records from the first frame twice a frame received while
/// the frame shown lags the frames received, as it does while icat sends
/// a film: `compose` walks a frame's bases back, never forward.
#[derive(Clone, Copy)]
pub(crate) struct Shape {
    w: usize,
    h: usize,
    /// The frames stored.
    frames: usize,
    /// The frames the offset words have room for.
    table: usize,
    build: Build,
}

/// **Whether a span has a canvas of its own for making frames**: a kitty
/// image's, whose frames arrive one at a time.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Build {
    None,
    Canvas,
}

/// **Which of a span's canvases**: the frame shown, which a renderer
/// reads, or a kitty image's canvas for making frames ([`Build`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Canvas {
    Shown,
    Build,
}

/// The frame a canvas holds when it holds none known: a canvas just laid,
/// or one an erase or a scroll changed under its records. No frame is
/// this, so a compose to any replays from its first record.
const UNKNOWN: u32 = u32::MAX;

/// **Whether a picture's rows have been moved** by a scroll under side
/// margins: until then the row map is not read, nor written, and every
/// pixel shows where it was stored ([`ImageStore::row_map`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowMap {
    /// Every pixel shows where it was stored.
    Same,
    /// A scroll moved some of its rows: every frame is drawn through the map.
    Moved,
}

/// The row map's entry for a stored pixel no longer shown: a scroll took
/// it past its band, or an erase cleared it.
const GONE: u16 = u16::MAX;

/// One record's head, read.
#[derive(Clone, Copy)]
struct Head {
    /// Where its data starts in the span.
    at: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    colours: usize,
    base: usize,
    ground: Pixel,
    blend: FrameBlend,
}

impl Head {
    /// The words of its data.
    fn data_len(self) -> usize {
        data_len(self.w * self.h, self.colours)
    }

    /// The part of `x0..x1` by `y0..y1` its rectangle covers, if any.
    fn within(
        self,
        (x0, y0, x1, y1): (usize, usize, usize, usize),
    ) -> Option<(usize, usize, usize, usize)> {
        let (a, b) = (x0.max(self.x), y0.max(self.y));
        let (c, d) = (x1.min(self.x + self.w), y1.min(self.y + self.h));
        (a < c && b < d).then_some((a, b, c, d))
    }
}

/// The words of a record's data: `area` ARGB words, or a table of
/// `colours` and the indexes four to a word.
fn data_len(area: usize, colours: usize) -> usize {
    match colours {
        0 => area,
        n => n + area.div_ceil(4),
    }
}

impl Shape {
    /// Slot `s`'s, its frames all stored.
    pub(crate) fn of(s: &ImageSlot) -> Shape {
        Shape {
            w: s.width.index(),
            h: s.height.index(),
            frames: s.anim.count as usize,
            table: s.anim.table as usize,
            build: match s.owner {
                ImageOwner::Kitty(..) => Build::Canvas,
                _ => Build::None,
            },
        }
    }

    /// Where canvas `c` starts.
    fn canvas_at(self, c: Canvas) -> usize {
        match c {
            Canvas::Shown => 0,
            Canvas::Build => self.rows().end,
        }
    }

    /// Where the word saying which frame canvas `c` holds is.
    fn held_at(self, c: Canvas) -> usize {
        self.canvas_at(c) + self.area()
    }

    /// The frame canvas `c` holds, from zero, or [`UNKNOWN`].
    fn holds(self, px: &[Pixel], c: Canvas) -> u32 {
        px[self.held_at(c)].0
    }

    /// Where the offset words start.
    fn table_at(self) -> usize {
        match self.build {
            Build::None => self.rows().end,
            Build::Canvas => self.rows().end + self.area() + 1,
        }
    }

    /// The build canvas marked as holding no frame known, where it has
    /// one: its pixels no longer what its records make.
    fn forget_build(self, px: &mut [Pixel]) {
        if self.build == Build::Canvas {
            px[self.held_at(Canvas::Build)] = Pixel(UNKNOWN);
        }
    }

    fn area(self) -> usize {
        self.w * self.h
    }

    /// The frame the canvas holds, from zero.
    fn composed(self, px: &[Pixel]) -> usize {
        px[self.area()].0 as usize
    }

    /// Where the word saying whether the rows are moved is.
    fn map_word(self) -> usize {
        self.area() + 1
    }

    fn row_map(self, px: &[Pixel]) -> RowMap {
        match px[self.map_word()].0 {
            0 => RowMap::Same,
            _ => RowMap::Moved,
        }
    }

    /// Where the row map starts, and its words.
    fn rows(self) -> Range<usize> {
        let at = self.area() + 2;
        at..at + self.area().div_ceil(2)
    }

    /// The canvas row stored pixel `q` is shown on, or [`GONE`].
    fn row_of(self, px: &[Pixel], q: usize) -> u16 {
        let b = px[self.rows()].as_bytes();
        u16::from_ne_bytes([b[2 * q], b[2 * q + 1]])
    }

    fn set_row(self, px: &mut [Pixel], q: usize, row: u16) {
        let r = self.rows();
        px[r].as_mut_bytes()[2 * q..2 * q + 2].copy_from_slice(&row.to_ne_bytes());
    }

    /// Where frame `k`'s record starts.
    fn record(self, px: &[Pixel], k: usize) -> usize {
        px[self.table_at() + k].0 as usize
    }

    /// Where the first record starts.
    fn first_record(self) -> usize {
        self.table_at() + self.table
    }

    fn head(self, px: &[Pixel], k: usize) -> Head {
        let r = self.record(px, k);
        let word = |n: usize| px[r + n].0;
        let (at, size) = (word(REC_AT), word(REC_SIZE));
        Head {
            at: r + REC_HEAD,
            x: (at & 0xFFFF) as usize,
            y: (at >> 16) as usize,
            w: (size & 0xFFFF) as usize,
            h: (size >> 16) as usize,
            colours: word(REC_COLOURS) as usize,
            base: word(REC_BASE) as usize,
            ground: Pixel(word(REC_GROUND)),
            blend: match word(REC_BLEND) {
                0 => FrameBlend::Replace,
                _ => FrameBlend::Blend,
            },
        }
    }

    /// The frame record `k` is over, from zero; none over the ground, and
    /// none for a base that is not before it, which no record is written
    /// with.
    fn parent(self, px: &[Pixel], k: usize) -> Option<usize> {
        match self.head(px, k).base {
            0 => None,
            b => (b - 1 < k).then_some(b - 1),
        }
    }

    /// **Record `k` onto the canvas**: the ground first if it is over
    /// none, then its rectangle, each pixel its data's -- replaced, or
    /// blended with kitty's straight-alpha over -- and an index of
    /// [`KEEP`] leaving the pixel as it is.
    fn apply(self, px: &mut [Pixel], k: usize, c: Canvas) {
        if self.row_map(px) == RowMap::Moved {
            return self.apply_moved(px, k, c);
        }
        self.apply_stored(px, k, c)
    }

    /// [`Shape::apply`] where the record puts its pixels, the row map not
    /// read.
    fn apply_stored(self, px: &mut [Pixel], k: usize, c: Canvas) {
        let r = self.head(px, k);
        let at = self.canvas_at(c);
        let cut = at + self.area();
        let (lo, rest) = px.split_at_mut(cut);
        let canvas = &mut lo[at..];
        if r.base == 0 {
            canvas.fill(r.ground);
        }
        let from = r.at - cut;
        let data = &rest[from..from + r.data_len()];
        let put = |dst: Pixel, src: Pixel| match r.blend {
            FrameBlend::Replace => src,
            FrameBlend::Blend => over_straight(dst, src),
        };
        match r.colours {
            0 => {
                for row in 0..r.h {
                    let at = (r.y + row) * self.w + r.x;
                    let (to, src) = (&mut canvas[at..at + r.w], &data[row * r.w..(row + 1) * r.w]);
                    match r.blend {
                        FrameBlend::Replace => to.copy_from_slice(src),
                        FrameBlend::Blend => {
                            for (d, &s) in to.iter_mut().zip(src) {
                                *d = put(*d, s);
                            }
                        }
                    }
                }
            }
            n => {
                let (table, indexes) = data.split_at(n);
                let bytes = indexes.as_bytes();
                for row in 0..r.h {
                    let at = (r.y + row) * self.w + r.x;
                    let to = &mut canvas[at..at + r.w];
                    for (d, &b) in to.iter_mut().zip(&bytes[row * r.w..(row + 1) * r.w]) {
                        if b != KEEP {
                            *d = put(*d, table[usize::from(b) - 1]);
                        }
                    }
                }
            }
        }
    }

    /// **Record `k` onto the canvas through the row map**, its rows moved:
    /// each pixel it sets drawn on the row the map holds for it, in its
    /// column, and none whose row is [`GONE`]. The ground fills what no
    /// stored pixel is shown on, a row a scroll left behind or an erase
    /// cleared, as base cleared it in every frame.
    #[cold]
    #[inline(never)]
    fn apply_moved(self, px: &mut [Pixel], k: usize, c: Canvas) {
        let r = self.head(px, k);
        let w = self.w;
        let at = self.canvas_at(c);
        let cut = at + self.area();
        let (lo, rest) = px.split_at_mut(cut);
        let (before, canvas) = lo.split_at_mut(at);
        if r.base == 0 {
            canvas.fill(r.ground);
        }
        let rows = self.rows();
        // The map is after the shown canvas and before the build canvas.
        let map = match c {
            Canvas::Shown => rest[rows.start - cut..rows.end - cut].as_bytes(),
            Canvas::Build => before[rows].as_bytes(),
        };
        let from = r.at - cut;
        let data = &rest[from..from + r.data_len()];
        let (table, indexes) = data.split_at(r.colours);
        let bytes = indexes.as_bytes();
        for row in 0..r.h {
            for col in 0..r.w {
                let j = row * r.w + col;
                let src = match r.colours {
                    0 => data[j],
                    _ => match bytes[j] {
                        KEEP => continue,
                        b => table[usize::from(b) - 1],
                    },
                };
                let (x, q) = (r.x + col, (r.y + row) * w + r.x + col);
                let to = u16::from_ne_bytes([map[2 * q], map[2 * q + 1]]);
                if to == GONE {
                    continue;
                }
                let at = usize::from(to) * w + x;
                canvas[at] = match r.blend {
                    FrameBlend::Replace => src,
                    FrameBlend::Blend => over_straight(canvas[at], src),
                };
            }
        }
    }

    /// **Columns `x0..x1` of rows `y0..y1` moved `k` rows `way`, in every
    /// frame**, as a scroll under side margins moves them: the canvas's
    /// pixels moved and the rows left behind cleared, as base moved each
    /// whole frame's, and the row map moved with them -- each stored pixel
    /// shown in the band moved `k` rows, or [`GONE`] past the band's edge --
    /// so every frame composed after it is drawn moved. In place: the map
    /// is laid in the span when the frames are, and the first move fills
    /// it, each pixel on its own row.
    fn shift(
        self,
        px: &mut [Pixel],
        (x0, x1): (usize, usize),
        (y0, y1): (usize, usize),
        k: usize,
        way: super::BandMove,
    ) {
        let (w, h) = (self.w, self.h);
        let (x1, y1) = (x1.min(w), y1.min(h));
        self.forget_build(px);
        if self.row_map(px) == RowMap::Same {
            for q in 0..self.area() {
                self.set_row(px, q, u16::try_from(q / w).unwrap_or(GONE));
            }
            px[self.map_word()] = Pixel(1);
        }
        for x in x0..x1 {
            for q in (0..h).map(|y| y * w + x) {
                let r = usize::from(self.row_of(px, q));
                if r == usize::from(GONE) || r < y0 || r >= y1 {
                    continue;
                }
                let to = match way {
                    super::BandMove::Up => (r >= y0 + k).then(|| r - k),
                    super::BandMove::Down => (r + k < y1).then_some(r + k),
                };
                let to = to.and_then(|t| u16::try_from(t).ok()).unwrap_or(GONE);
                self.set_row(px, q, to);
            }
        }
        let canvas = &mut px[..self.area()];
        match way {
            super::BandMove::Up => {
                for y in y0..y1 {
                    let row = y * w + x0..y * w + x1;
                    match y + k < y1 {
                        true => canvas.copy_within((y + k) * w + x0..(y + k) * w + x1, row.start),
                        false => canvas[row].fill(Pixel::CLEAR),
                    }
                }
            }
            super::BandMove::Down => {
                for y in (y0..y1).rev() {
                    let row = y * w + x0..y * w + x1;
                    match y >= y0 + k {
                        true => canvas.copy_within((y - k) * w + x0..(y - k) * w + x1, row.start),
                        false => canvas[row].fill(Pixel::CLEAR),
                    }
                }
            }
        }
    }

    /// **The canvas made frame `t`**: the records from the one after the
    /// frame it holds, or from the last over the ground, to `t`, each
    /// over the one before. How many were applied; none if it held `t`.
    fn compose(self, px: &mut [Pixel], t: usize) -> usize {
        self.compose_on(px, t, Canvas::Shown)
    }

    /// [`Shape::compose`] on canvas `c`.
    fn compose_on(self, px: &mut [Pixel], t: usize, c: Canvas) -> usize {
        let held = self.holds(px, c) as usize;
        if held == t {
            return 0;
        }
        // The chain back from `t`: to the frame held, or to its root.
        let mut len = 1;
        let mut f = t;
        while let Some(p) = self.parent(px, f) {
            if p == held {
                break;
            }
            len += 1;
            f = p;
        }
        for i in (0..len).rev() {
            let mut g = t;
            for _ in 0..i {
                g = self.parent(px, g).unwrap_or(g);
            }
            self.apply(px, g, c);
        }
        px[self.held_at(c)] = Pixel(u32::try_from(t).unwrap_or(u32::MAX));
        len
    }

    /// **Frame `k`'s record written at `at`**, its pixels `sample`'s, its
    /// word `word`, over frame `base` from zero -- none, a clear ground, as
    /// the first frame always is -- which canvas `c` holds, and the canvas
    /// made it: the rectangle the pixels that differ from the canvas
    /// bound, looked for in `within` alone, outside which none differs;
    /// its table the colours of those pixels, or ARGB words if they have
    /// more than [`MOST_COLOURS`]. `sample` is handed each position and the
    /// canvas's pixel there, the base's. Where the record ends, or `None`
    /// if the span holds no room for it.
    #[allow(clippy::too_many_arguments)]
    fn push(
        self,
        px: &mut [Pixel],
        k: usize,
        at: usize,
        word: u32,
        base: Option<usize>,
        c: Canvas,
        (x0, y0, x1, y1): (usize, usize, usize, usize),
        sample: impl Fn(usize, usize, Pixel) -> Pixel,
    ) -> Option<usize> {
        let (area, w) = (self.area(), self.w);
        let len = px.len();
        if k >= self.table || at < self.first_record() {
            return None;
        }
        if k == 0 {
            px[self.map_word()] = Pixel(0);
        }
        let cut = self.canvas_at(c) + area;
        let (lo, rest) = px.split_at_mut(cut);
        let canvas = &mut lo[cut - area..];
        if base.is_none() {
            canvas.fill(Pixel::CLEAR);
        }
        let (x1, y1) = (x1.min(w), y1.min(self.h));
        let mut pal = Palette::NEW;
        let (mut bx0, mut by0, mut bx1, mut by1) = (usize::MAX, usize::MAX, 0, 0);
        for y in y0..y1 {
            for x in x0..x1 {
                let under = canvas[y * w + x];
                let p = sample(x, y, under);
                if p != under {
                    bx0 = bx0.min(x);
                    by0 = by0.min(y);
                    bx1 = bx1.max(x + 1);
                    by1 = by1.max(y + 1);
                    pal.add(p);
                }
            }
        }
        let (bx, by, bw, bh) = match bx0 < bx1 {
            true => (bx0, by0, bx1 - bx0, by1 - by0),
            false => (0, 0, 0, 0),
        };
        let colours = pal.colours();
        let end = at.checked_add(REC_HEAD + data_len(bw * bh, colours))?;
        if end > len {
            return None;
        }
        let r = at - cut;
        let word32 = |v: usize| Pixel(u32::try_from(v).unwrap_or(u32::MAX));
        rest[r + REC_WORD] = Pixel(word);
        rest[r + REC_AT] = word32(bx | by << 16);
        rest[r + REC_SIZE] = word32(bw | bh << 16);
        rest[r + REC_COLOURS] = word32(colours);
        rest[r + REC_BASE] = word32(base.map_or(0, |b| b + 1));
        rest[r + REC_GROUND] = Pixel::CLEAR;
        rest[r + REC_BLEND] = Pixel(0);
        let d = r + REC_HEAD;
        match colours {
            0 => {
                let mut j = d;
                for y in by..by + bh {
                    for x in bx..bx + bw {
                        let i = y * w + x;
                        let p = sample(x, y, canvas[i]);
                        rest[j] = p;
                        canvas[i] = p;
                        j += 1;
                    }
                }
            }
            n => {
                rest[d..d + n].copy_from_slice(&pal.table[..n]);
                let words = d + n;
                let mut quad = [KEEP; 4];
                let mut j = 0;
                for y in by..by + bh {
                    for x in bx..bx + bw {
                        let i = y * w + x;
                        let p = sample(x, y, canvas[i]);
                        quad[j % 4] = match p == canvas[i] {
                            true => KEEP,
                            false => pal.index(p),
                        };
                        canvas[i] = p;
                        j += 1;
                        if j % 4 == 0 {
                            rest[words + j / 4 - 1] = Pixel(u32::from_ne_bytes(quad));
                            quad = [KEEP; 4];
                        }
                    }
                }
                if j % 4 != 0 {
                    rest[words + j / 4] = Pixel(u32::from_ne_bytes(quad));
                }
            }
        }
        rest[self.table_at() + k - cut] = word32(at);
        rest[0] = word32(k);
        Some(end)
    }

    /// **`x0..x1` by `y0..y1` cleared in every frame**: the canvas there
    /// cleared, every ARGB record's pixels there clear and every indexed
    /// one's [`KEEP`], so the first frame shows its clear ground there and
    /// each after it keeps what the one before shows. How many pixels of
    /// the records were lit there before ([`Shape::lit`]).
    fn clear(self, px: &mut [Pixel], region: (usize, usize, usize, usize)) -> u32 {
        self.forget_build(px);
        if self.row_map(px) == RowMap::Moved {
            return self.clear_moved(px, region);
        }
        let (x0, y0, x1, y1) = region;
        let (x1, y1) = (x1.min(self.w), y1.min(self.h));
        for y in y0..y1 {
            if x0 < x1 {
                px[y * self.w + x0..y * self.w + x1].fill(Pixel::CLEAR);
            }
        }
        let mut lit = 0u32;
        for k in 0..self.frames {
            let r = self.head(px, k);
            let Some((a, b, c, d)) = r.within((x0, y0, x1, y1)) else {
                continue;
            };
            match r.colours {
                0 => {
                    for y in b..d {
                        let row = r.at + (y - r.y) * r.w;
                        for p in &mut px[row + a - r.x..row + c - r.x] {
                            lit += u32::from(p.alpha().get() != 0);
                            *p = Pixel::CLEAR;
                        }
                    }
                }
                n => {
                    let (head, data) = px.split_at_mut(r.at + n);
                    let table = &head[r.at..];
                    let bytes = data[..(r.w * r.h).div_ceil(4)].as_mut_bytes();
                    for y in b..d {
                        let row = (y - r.y) * r.w;
                        for ix in &mut bytes[row + a - r.x..row + c - r.x] {
                            if *ix != KEEP && table[usize::from(*ix) - 1].alpha().get() != 0 {
                                lit += 1;
                            }
                            *ix = KEEP;
                        }
                    }
                }
            }
        }
        lit
    }

    /// **How many pixels of the records in `x0..x1` by `y0..y1` are lit**,
    /// summed over the frames: each pixel a record sets there that is not
    /// clear. Zero exactly when every frame is clear there, since the
    /// first frame is over a clear ground and every pixel any frame shows
    /// is one some record set; so an erase that brings it to zero has left
    /// nothing of the picture, as one over whole frames does.
    fn lit(self, px: &[Pixel], (x0, y0, x1, y1): (usize, usize, usize, usize)) -> u32 {
        if self.row_map(px) == RowMap::Moved {
            return self.lit_moved(px, (x0, y0, x1, y1));
        }
        let mut lit = 0usize;
        for k in 0..self.frames {
            let r = self.head(px, k);
            let Some((a, b, c, d)) = r.within((x0, y0, x1, y1)) else {
                continue;
            };
            match r.colours {
                0 => {
                    for y in b..d {
                        let row = r.at + (y - r.y) * r.w;
                        lit += px[row + a - r.x..row + c - r.x]
                            .iter()
                            .filter(|p| p.alpha().get() != 0)
                            .count();
                    }
                }
                n => {
                    let table = &px[r.at..r.at + n];
                    let bytes = px[r.at + n..r.at + r.data_len()].as_bytes();
                    for y in b..d {
                        let row = (y - r.y) * r.w;
                        lit += bytes[row + a - r.x..row + c - r.x]
                            .iter()
                            .filter(|&&ix| {
                                ix != KEEP && table[usize::from(ix) - 1].alpha().get() != 0
                            })
                            .count();
                    }
                }
            }
        }
        u32::try_from(lit).unwrap_or(u32::MAX)
    }
}

impl Shape {
    /// [`Shape::lit`] through the row map: the pixels the records set that
    /// are shown in the region, wherever they are stored.
    #[cold]
    #[inline(never)]
    fn lit_moved(self, px: &[Pixel], (x0, y0, x1, y1): (usize, usize, usize, usize)) -> u32 {
        let w = self.w;
        let mut lit = 0usize;
        for k in 0..self.frames {
            let r = self.head(px, k);
            let (a, c) = (x0.max(r.x), x1.min(r.x + r.w));
            let table = &px[r.at..r.at + r.colours];
            let data = &px[r.at + r.colours..r.at + r.data_len()];
            let bytes = data.as_bytes();
            for x in a..c {
                for y in r.y..r.y + r.h {
                    let to = usize::from(self.row_of(px, y * w + x));
                    if to == usize::from(GONE) || to < y0 || to >= y1 {
                        continue;
                    }
                    let j = (y - r.y) * r.w + x - r.x;
                    let p = match r.colours {
                        0 => data[j],
                        _ => match bytes[j] {
                            KEEP => continue,
                            b => table[usize::from(b) - 1],
                        },
                    };
                    lit += usize::from(p.alpha().get() != 0);
                }
            }
        }
        u32::try_from(lit).unwrap_or(u32::MAX)
    }

    /// [`Shape::clear`] through the row map: every stored pixel shown in
    /// the region taken off it, [`GONE`], so no frame draws it again and
    /// the ground shows there, and the canvas cleared there. How many of
    /// the records' pixels shown there were lit.
    #[cold]
    #[inline(never)]
    fn clear_moved(self, px: &mut [Pixel], region: (usize, usize, usize, usize)) -> u32 {
        let lit = self.lit_moved(px, region);
        let (x0, y0, x1, y1) = region;
        let (w, h) = (self.w, self.h);
        let (x1, y1) = (x1.min(w), y1.min(h));
        for x in x0..x1 {
            for q in (0..h).map(|y| y * w + x) {
                let to = usize::from(self.row_of(px, q));
                if to != usize::from(GONE) && to >= y0 && to < y1 {
                    self.set_row(px, q, GONE);
                }
            }
        }
        for y in y0..y1 {
            if x0 < x1 {
                px[y * w + x0..y * w + x1].fill(Pixel::CLEAR);
            }
        }
        lit
    }
}

/// **The colours of one frame's changed pixels**, up to [`MOST_COLOURS`],
/// each given the next index from one: open addressing over twice their
/// most, linear probing. One more colour and it holds none
/// ([`Palette::colours`] zero), the record ARGB.
struct Palette {
    keys: [u32; PALETTE_SLOTS],
    /// Each slot's index, [`KEEP`] for an empty one.
    index: [u8; PALETTE_SLOTS],
    table: [Pixel; MOST_COLOURS],
    n: usize,
}

const PALETTE_SLOTS: usize = 512;

impl Palette {
    const NEW: Palette = Palette {
        keys: [0; PALETTE_SLOTS],
        index: [KEEP; PALETTE_SLOTS],
        table: [Pixel::CLEAR; MOST_COLOURS],
        n: 0,
    };

    /// The slot `p` hashes to: the multiplicative hash's top nine bits.
    fn slot(p: Pixel) -> usize {
        (p.0.wrapping_mul(0x9E37_79B1) >> 23) as usize
    }

    fn add(&mut self, p: Pixel) {
        if self.n > MOST_COLOURS {
            return;
        }
        let mut s = Palette::slot(p);
        loop {
            if self.index[s] == KEEP {
                if self.n == MOST_COLOURS {
                    self.n += 1;
                    return;
                }
                self.keys[s] = p.0;
                self.table[self.n] = p;
                self.n += 1;
                self.index[s] = u8::try_from(self.n).unwrap_or(KEEP);
                return;
            }
            if self.keys[s] == p.0 {
                return;
            }
            s = (s + 1) % PALETTE_SLOTS;
        }
    }

    /// The index of `p`, one it was given.
    fn index(&self, p: Pixel) -> u8 {
        let mut s = Palette::slot(p);
        for _ in 0..PALETTE_SLOTS {
            if self.index[s] == KEEP || self.keys[s] == p.0 {
                return self.index[s];
            }
            s = (s + 1) % PALETTE_SLOTS;
        }
        KEEP
    }

    /// How many colours its table has; zero once there were too many.
    fn colours(&self) -> usize {
        match self.n > MOST_COLOURS {
            true => 0,
            false => self.n,
        }
    }
}

/// **Words `src` of the arena to read and `dst` to write**, the two
/// apart: a picture's frames read from one image's canvas into another's
/// span. No words to read lends none.
fn lend(px: &mut [Pixel], src: Range<usize>, dst: Range<usize>) -> (&[Pixel], &mut [Pixel]) {
    if src.is_empty() {
        return (&[], &mut px[dst]);
    }
    match src.end <= dst.start {
        true => {
            let (lo, hi) = px.split_at_mut(dst.start);
            (&lo[src], &mut hi[..dst.len()])
        }
        false => {
            let (lo, hi) = px.split_at_mut(src.start);
            (&hi[..src.len()], &mut lo[dst])
        }
    }
}

/// **A picture's frames being stored as rectangles**
/// ([`ImageStore::begin_rects`]): the image, how many frames it will have,
/// how many are stored, and where the next record goes in its span.
pub(crate) struct RectFrames {
    id: ImageId,
    frames: u32,
    made: u32,
    at: usize,
}

impl RectFrames {
    /// How many frames are stored.
    pub(crate) fn made(&self) -> u32 {
        self.made
    }
}

impl<'b> ImageStore<'b> {
    /// **Every picture's canvas made the frame it shows**, a picture's frames
    /// being kept as rectangles: each whose target moved since it was last
    /// composed has its records replayed to it (`Shape::compose`), and
    /// the rest are left alone. What [`ImageStore::animate`] ends with, so
    /// a frame is composed once a tick, never as bytes arrive. How many
    /// canvases were composed, which [`ImageStore::frames_composed`] sums.
    pub fn compose_frames(&mut self) -> usize {
        if self.framed == 0 {
            return 0;
        }
        let mut n = 0;
        for i in 0..self.slots.len() {
            let s = self.slots[i];
            if s.is_empty() || s.anim.layout != FrameLayout::Rects {
                continue;
            }
            let span = &mut self.pixels[s.start()..s.end()];
            if Shape::of(&s).compose(span, s.anim.shown as usize) > 0 {
                n += 1;
            }
        }
        self.frames_composed += n;
        n
    }

    /// **Frame `n` made the one to show**, as kitty's `a=a,c=` makes it,
    /// its gap counted from the next tick: the target alone, the canvas
    /// of a picture's frames composed to it at the next
    /// [`ImageStore::compose_frames`], however many jumps come first.
    /// Whether `n` names a frame of a live image.
    pub fn show_frame(&mut self, id: ImageId, n: FrameNo) -> bool {
        let Some(s) = id.index().and_then(|i| self.slots.get_mut(i)) else {
            return false;
        };
        if s.is_empty() || n.0 == 0 || n.0 > s.anim.count {
            return false;
        }
        s.anim.shown = n.0 - 1;
        s.anim.shown_at = None;
        true
    }

    /// **How many words image `id`'s span takes in the arena**: its pixels,
    /// every frame as it is stored, and a picture's register numbers.
    pub fn stored_words(&self, id: ImageId) -> Option<usize> {
        let s = self.slots.get(id.index()?)?;
        (!s.is_empty()).then(|| s.span_len())
    }

    /// **Whether image `id`'s rows have been moved within it** by a scroll
    /// under side margins, where its frames are kept as records; `None`
    /// for an image kept whole, or none.
    pub fn row_map(&self, id: ImageId) -> Option<RowMap> {
        let s = self.slots.get(id.index()?)?;
        (!s.is_empty() && s.anim.layout == FrameLayout::Rects)
            .then(|| Shape::of(s).row_map(&self.pixels[s.start()..s.end()]))
    }

    /// **Begin storing a `width` x `height` picture of `frames` frames as
    /// rectangles**, in the largest gap the arena has, since the room its
    /// records take is known only once they are made; what they leave of
    /// it goes back at [`ImageStore::end_rects`]. Refused and counted as
    /// [`ImageStore::store`] refuses, and for a gap that holds less than
    /// the canvas and the frames' words.
    pub(crate) fn begin_rects(
        &mut self,
        width: ImagePx,
        height: ImagePx,
        frames: u32,
    ) -> Option<RectFrames> {
        let area = width.index().checked_mul(height.index())?;
        let shape = Shape {
            w: width.index(),
            h: height.index(),
            frames: frames as usize,
            table: frames as usize,
            build: Build::None,
        };
        let least = shape.first_record().checked_add(REC_HEAD)?;
        let (at, len) = self.largest_gap();
        let len = len.min(u32::MAX as usize);
        let fits = area > 0 && frames > 0 && len >= least;
        let id = self.store_at(fits.then_some(at), width, height, ImageAlpha::OpaqueOrClear)?;
        let a = &mut self.slots[id.index()?].anim;
        a.layout = FrameLayout::Rects;
        a.cap = u32::try_from(len).unwrap_or(u32::MAX);
        a.count = 0;
        a.table = frames;
        Some(RectFrames {
            id,
            frames,
            made: 0,
            at: shape.first_record(),
        })
    }

    /// **The next frame stored**, its gap in milliseconds and whether it
    /// waits for input: its pixels `sample`'s, read from words `src` of
    /// the arena -- none for pixels from elsewhere -- and differing from
    /// the frame before only inside `within`. `None`, the arena refusal
    /// counted, when the span holds no room for it.
    pub(crate) fn push_rect(
        &mut self,
        b: &mut RectFrames,
        gap: u32,
        wait: FrameWait,
        within: (usize, usize, usize, usize),
        src: Range<usize>,
        sample: impl Fn(&[Pixel], usize, usize) -> Pixel,
    ) -> Option<()> {
        let i = b.id.index()?;
        let s = self.slots[i];
        if s.is_empty() || s.anim.layout != FrameLayout::Rects || b.made >= b.frames {
            return None;
        }
        let shape = Shape {
            frames: b.frames as usize,
            table: b.frames as usize,
            ..Shape::of(&s)
        };
        let gap = gap.min(PICTURE_GAP_MASK);
        let word = gap
            | match wait {
                FrameWait::Gap => 0,
                FrameWait::Input => INPUT_BIT,
            };
        let (from, span) = lend(self.pixels, src, s.start()..s.end());
        let k = b.made as usize;
        let base = k.checked_sub(1);
        let Some(end) = shape.push(
            span,
            k,
            b.at,
            word,
            base,
            Canvas::Shown,
            within,
            |x, y, _| sample(from, x, y),
        ) else {
            self.refuse(ImageRefusal::Arena(s.width, s.height));
            return None;
        };
        b.at = end;
        b.made += 1;
        let a = &mut self.slots[i].anim;
        if k == 0 {
            a.gap0 = word;
            a.depth0 = FrameDepth::Rgba;
        }
        a.count = b.made;
        a.duration += u64::from(gap);
        if wait == FrameWait::Input {
            a.waits += 1;
        }
        Some(())
    }

    /// **A picture's frames all stored**: its span cut to its records, the
    /// rest of the gap given back, and the canvas made its first frame. A
    /// picture of one frame keeps its canvas alone, a still image. `None`,
    /// the image gone, if fewer frames were stored than it was begun with.
    pub(crate) fn end_rects(&mut self, b: RectFrames) -> Option<ImageId> {
        let i = b.id.index()?;
        let s = self.slots[i];
        if s.is_empty() || b.made != b.frames || b.made == 0 {
            self.abandon_rects(b);
            return None;
        }
        let (lo, hi) = (s.start(), s.end());
        let words = match b.made {
            1 => {
                let a = &mut self.slots[i].anim;
                a.layout = FrameLayout::Whole;
                a.cap = 1;
                s.area()
            }
            _ => {
                self.slots[i].anim.cap = u32::try_from(b.at).unwrap_or(u32::MAX);
                let t = self.slots[i];
                Shape::of(&t).compose(&mut self.pixels[t.start()..t.end()], 0);
                self.slots[i].anim.shown = 0;
                self.framed += 1;
                b.at
            }
        };
        self.release_free(lo + words, hi);
        self.epoch = self.epoch.bumped();
        Some(b.id)
    }

    /// **A picture begun as rectangles given up**, its slot and gap freed
    /// with nothing counted but the refusal already counted: a decode that
    /// failed, or one the span ran out for.
    pub(crate) fn abandon_rects(&mut self, b: RectFrames) {
        let Some(i) = b.id.index() else {
            return;
        };
        if self.slots[i].is_empty() {
            return;
        }
        // Never counted among the images with frames.
        self.slots[i].anim.count = 0;
        self.forget(i);
        self.epoch = self.epoch.bumped();
    }

    /// **A `width` x `height` picture of slot `si`'s frames, each
    /// `sample`'s pixels of that frame**: a frame of slot `si`, whole or
    /// composed in its canvas, handed to `sample` with the position, and
    /// stored as rectangles ([`ImageStore::begin_rects`]) with its gap and
    /// wait; slot `si`'s canvas left holding the frame it held. The new
    /// picture shows its first frame, stopped. Refused and counted as
    /// [`ImageStore::begin_rects`] refuses.
    pub(crate) fn rects_from(
        &mut self,
        si: usize,
        width: ImagePx,
        height: ImagePx,
        sample: impl Fn(&[Pixel], usize, usize) -> Pixel,
    ) -> Option<ImageId> {
        let s = self.slots[si];
        let n = s.anim.count;
        let mut b = self.begin_rects(width, height, n)?;
        let held = match s.anim.layout {
            FrameLayout::Rects => Some(Shape::of(&s).composed(&self.pixels[s.start()..s.end()])),
            FrameLayout::Whole => None,
        };
        let whole = (0, 0, width.index(), height.index());
        for k in 0..n as usize {
            let at = match s.anim.layout {
                FrameLayout::Whole => s.start() + k * s.area(),
                FrameLayout::Rects => {
                    Shape::of(&s).compose(&mut self.pixels[s.start()..s.end()], k);
                    s.start()
                }
            };
            let gap = self.frame_word(&s, k).0;
            let wait = self.frame_waits(&s, k);
            if self
                .push_rect(&mut b, gap, wait, whole, at..at + s.area(), &sample)
                .is_none()
            {
                break;
            }
        }
        if let Some(c) = held {
            Shape::of(&s).compose(&mut self.pixels[s.start()..s.end()], c);
        }
        self.end_rects(b)
    }

    /// **Slot `to`'s frames timed as slot `from`'s**, a copy of its
    /// animation made by [`ImageStore::rects_from`]: its state and loops,
    /// shown from its first frame.
    pub(crate) fn time_as(&mut self, from: usize, to: usize) {
        let f = self.slots[from].anim;
        let a = &mut self.slots[to].anim;
        a.state = f.state;
        a.max_loops = f.max_loops;
        a.loops = 0;
        a.shown = 0;
        a.shown_at = None;
    }

    /// **Slot `s`'s frames' columns `x0..x1` of rows `y0..y1` moved `k`
    /// rows `way`** in its own span ([`Shape::shift`]).
    pub(crate) fn move_rects(
        &mut self,
        s: &ImageSlot,
        cols: (usize, usize),
        rows: (usize, usize),
        k: usize,
        way: super::BandMove,
    ) {
        Shape::of(s).shift(&mut self.pixels[s.start()..s.end()], cols, rows, k, way);
        self.epoch = self.epoch.bumped();
    }

    /// The frame slot `s`'s canvas holds, a picture's of rectangles.
    pub(crate) fn canvas_frame(&self, s: &ImageSlot) -> FrameNo {
        let k = Shape::of(s).composed(&self.pixels[s.start()..s.end()]);
        FrameNo(u32::try_from(k).unwrap_or(u32::MAX).saturating_add(1))
    }

    /// Slot `s`'s records cleared in `region` ([`Shape::clear`]): how many
    /// of their pixels were lit there.
    pub(crate) fn clear_rects(
        &mut self,
        s: &ImageSlot,
        region: (usize, usize, usize, usize),
    ) -> u32 {
        Shape::of(s).clear(&mut self.pixels[s.start()..s.end()], region)
    }

    /// Slot `s`'s records' lit pixels in `region` ([`Shape::lit`]).
    pub(crate) fn lit_rects(&self, s: &ImageSlot, region: (usize, usize, usize, usize)) -> u32 {
        Shape::of(s).lit(&self.pixels[s.start()..s.end()], region)
    }
}
