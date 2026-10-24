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
//! incoming frame's data needs room too, so an animation holds fewer than
//! half the arena's frames: measured, a 32x32 animation in an arena of 12
//! frames and 64 words takes 5 frames and the sixth is ENOSPC, and so
//! does a 1920x1080 one in the app's 95 MiB; kitty keeps frames on disk under a quota of five
//! times its image quota ("Image persistence and storage quotas"), its
//! practice and not followed.
//!
//! **Every frame is stored whole**, a snapshot coalesced when it is made,
//! and a frame's pixels here are what a renderer draws, as every image's
//! are. The text says only that frames are "created by composing the
//! transmitted data onto a background canvas". kitty keeps a frame as its
//! data over its base frame and coalesces when it shows it (`[[source:
//! kitty-graphics-c]]` `get_coalesced_frame_data()`, a long chain made a
//! whole frame when it is made), so after its base frame is edited
//! (`a=f,r=`), composed into (`a=c`) or deleted, kitty's dependent frame
//! changes and this one does not: kitty's practice, not followed. A frame is made as kitty makes it, in its order: on a
//! canvas of the base frame `c`, or of the background `Y` -- for a frame
//! that covers only part of the image, the data as sent for one that
//! covers all of it (Jev, KITTY, 0.98 and 1.00) -- the data composed at
//! `x`, `y`, alpha blended unless `X=1` or the data is RGB. The blend is
//! kitty's straight-alpha over (`[[source: kitty-simd-string-h]]`
//! `blend_pixel_over_straight()`), onto an RGB frame its opaque one; an RGB
//! frame keeps no alpha, as kitty's three-byte frames keep none.
//!
//! **The clock is the caller's** ([`ImageStore::animate`]): a renderer's
//! display link, or a loop's wait, says what time it is, and a frame
//! changes only then. The steps are kitty's `scan_active_animations`: one
//! frame a tick once its gap has passed, past gapless frames, a loop
//! counted at the wrap, a loading animation waiting at its last frame.

use super::{ImageAlpha, ImageId, ImageOwner, ImagePx, ImageSlot, ImageState, ImageStore, Pixel};
use crate::style::Chan;
use crate::time::HostTime;
use core::time::Duration;

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
/// clock and a client's `c=` still shows at once (Jev's lean,
/// HOLD_CURRENT, 0.85 and 0.88 after research): Apple's "reducing
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

/// **One image's animation**: how many frames and how many it has room
/// for, the one shown, the root frame's gap and depth -- the others' are
/// in the span -- and `a=a`'s state. [`Frames::ONE`] for every image
/// until a frame is added.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Frames {
    count: u32,
    cap: u32,
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
    };

    /// The words a span of frames `area` pixels each takes.
    pub(crate) fn span(&self, area: usize) -> usize {
        let cap = self.cap as usize;
        area * cap + (cap - 1)
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
            _ => self.pixels[s.start() + s.area() * s.anim.cap as usize + k - 1].bits(),
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
        let at = s.start() + s.area() * s.anim.cap as usize + k - 1;
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
    /// that is all there is, the oldest kitty images with no placement but
    /// `spare` deleted until one is free.
    fn frame_room(&mut self, i: usize, need: u32, spare: ImageId) -> Result<(), FrameRefusal> {
        let s = self.slots[i];
        if need <= s.anim.cap {
            return Ok(());
        }
        let area = s.area();
        let len = |cap: u32| Frames { cap, ..s.anim }.span(area);
        let me = ImageId::from_index(i);
        let at = [need.saturating_mul(2).max(need), need]
            .into_iter()
            .find_map(|cap| self.gap_evicting(len(cap), [me, spare]).map(|at| (at, cap)));
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

    /// **A gap of `len` words**, deleting the oldest kitty images with no
    /// placement but those in `spare` until one opens, as
    /// [`ImageStore::reserve_evicting`] deletes for a new image; none
    /// deleted unless deleting them all would make room.
    fn gap_evicting(&mut self, len: usize, spare: [ImageId; 2]) -> Option<usize> {
        if let Some(at) = self.lowest_gap(len) {
            return Some(at);
        }
        // Two live spans never share a start, so a spared one is known by
        // its own.
        let starts = spare.map(|id| {
            id.index()
                .and_then(|j| self.slots.get(j))
                .filter(|s| !s.is_empty())
                .map(|s| s.start())
        });
        let evictable = |s: &ImageSlot| {
            !s.is_empty()
                && !starts.contains(&Some(s.start()))
                && s.placements.index() == 0
                && s.state == ImageState::Ready
                && matches!(s.owner, ImageOwner::Kitty(..))
        };
        self.lowest_gap_among(len, |s| !s.is_empty() && !evictable(s))?;
        loop {
            if let Some(at) = self.lowest_gap(len) {
                return Some(at);
            }
            let old = self
                .slots
                .iter()
                .enumerate()
                .filter(|(_, s)| evictable(s))
                .min_by_key(|(_, s)| s.born.raw())
                .map(|(j, _)| ImageId::from_index(j))?;
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
        let k = match fresh {
            true => {
                self.frame_room(i, count + 1, data)?;
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
        if count == 1 {
            return upper;
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
    pub fn animate(&mut self, now: HostTime, motion: ImageMotion) -> Animated {
        let was = self.held;
        self.held = motion;
        let input = core::mem::replace(&mut self.input, InputSeen::No);
        if self.framed == 0 || motion == ImageMotion::Hold {
            return Animated::Same;
        }
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
    /// held or nothing runs.
    pub fn next_frame(&self, now: HostTime) -> Option<HostTime> {
        if self.framed == 0 || self.held == ImageMotion::Hold {
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

    /// **Store a `width` x `height` animation with room for `frames`
    /// frames**, each stored whole, as [`ImageStore::store`] stores one
    /// frame and refuses as it refuses: the span reserved at once, so its
    /// frames are written in place by [`ImageStore::frame_mut`] and never
    /// move. A picture's frames, a decoded GIF's.
    pub fn store_frames(
        &mut self,
        width: ImagePx,
        height: ImagePx,
        frames: u32,
    ) -> Option<ImageId> {
        let area = width.index().checked_mul(height.index())?;
        let span = Frames {
            cap: frames.max(1),
            ..Frames::ONE
        }
        .span(area);
        let at = (area > 0).then(|| self.lowest_gap(span)).flatten();
        let id = self.store_at(at, width, height, ImageAlpha::OpaqueOrClear)?;
        let i = id.index()?;
        self.slots[i].anim.cap = frames.max(1);
        Some(id)
    }

    /// **Frame `k`'s pixels, from zero, to write**, its gap in milliseconds
    /// and whether it waits for input set: the root, or the next frame
    /// after the last, while there is room ([`ImageStore::store_frames`]).
    pub fn frame_mut(
        &mut self,
        id: ImageId,
        k: u32,
        gap: u32,
        wait: FrameWait,
    ) -> Option<&mut [Pixel]> {
        let i = id.index()?;
        let s = *self.slots.get(i)?;
        let count = s.anim.count;
        if s.is_empty() || k >= s.anim.cap || (k != 0 && k != count) {
            return None;
        }
        let gap = gap.min(gap_mask(&s));
        self.set_word(i, k as usize, gap, FrameDepth::Rgba, wait);
        let a = &mut self.slots[i].anim;
        a.duration += u64::from(gap);
        if wait == FrameWait::Input {
            a.waits += 1;
        }
        if k > 0 {
            a.count += 1;
            if a.count == 2 {
                self.framed += 1;
            }
        }
        self.epoch = self.epoch.bumped();
        let at = s.start() + k as usize * s.area();
        self.pixels.get_mut(at..at + s.area())
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

    /// **Slot `to`'s frames timed as slot `from`'s**, its frames already
    /// written: the words, the count, the gaps' sum and the state, as the
    /// copy of an animation resampled takes them.
    pub(crate) fn copy_timing(&mut self, from: usize, to: usize) {
        let (f, t) = (self.slots[from], self.slots[to]);
        let n = f.anim.count.min(t.anim.cap);
        for k in 1..n as usize {
            let w = self.raw_word(&f, k);
            let at = t.start() + t.area() * t.anim.cap as usize + k - 1;
            self.pixels[at] = Pixel(w);
        }
        let a = &mut self.slots[to].anim;
        let cap = a.cap;
        *a = Frames {
            cap,
            count: n,
            ..f.anim
        };
        a.shown = 0;
        a.shown_at = None;
        if n > 1 {
            self.framed += 1;
        }
        self.epoch = self.epoch.bumped();
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

    /// Frame `n`'s pixels.
    pub fn frame_pixels(&self, id: ImageId, n: FrameNo) -> Option<&[Pixel]> {
        let s = *self.slots.get(id.index()?)?;
        if s.is_empty() || n.0 == 0 || n.0 > s.anim.count {
            return None;
        }
        let at = s.start() + (n.0 as usize - 1) * s.area();
        self.pixels.get(at..at + s.area())
    }

    /// The loops counted and the loops to play, zero for ever.
    pub fn frame_loops(&self, id: ImageId) -> Option<(u32, u32)> {
        let s = self.slots.get(id.index()?)?;
        (!s.is_empty()).then_some((s.anim.loops, s.anim.max_loops))
    }
}
