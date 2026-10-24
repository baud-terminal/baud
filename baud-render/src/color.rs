//! Renditions to pixels: the theme, the palette, and the attribute rules.

use baud_domain::themefile::{CellPaint, CursorText, Scheme, SelectionText};
use baud_domain::{Attrs, Chan, Color, ColorWord, Count, DynamicColor, LINEAR, StyleWord};

/// A pixel: `0xAARRGGBB`, straight alpha, which is what a `CALayer` and a
/// `wl_shm` buffer both take once premultiplied at the edge, by the one
/// function both presenters call, `baud_domain::premultiply`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[repr(transparent)]
pub struct Argb(pub u32);

impl Argb {
    pub fn rgb(r: Chan, g: Chan, b: Chan) -> Argb {
        Argb(u32::from_be_bytes([0xFF, r.get(), g.get(), b.get()]))
    }

    fn channels(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    pub fn with_alpha(self, a: Chan) -> Argb {
        let [_, r, g, b] = self.channels();
        Argb(u32::from_be_bytes([a.get(), r, g, b]))
    }

    pub fn a(self) -> Chan {
        Chan::from_u8(self.channels()[0])
    }
    pub fn r(self) -> Chan {
        Chan::from_u8(self.channels()[1])
    }
    pub fn g(self) -> Chan {
        Chan::from_u8(self.channels()[2])
    }
    pub fn b(self) -> Chan {
        Chan::from_u8(self.channels()[3])
    }

    /// `level` of the way from `other` to this colour, every channel and
    /// alpha alike, in encoded sRGB: `(x l + y (255 - l) + 127) / 255` in
    /// integers, which the cell shader's `mix_level` spells the same way,
    /// so a fading cursor is one colour on every path. 255 is this colour
    /// and 0 is `other`, exactly.
    pub fn mix(self, other: Argb, level: Chan) -> Argb {
        let l = u32::from(level.get());
        let (x, y) = (self.channels(), other.channels());
        let at = |i: usize| {
            let v = (u32::from(x[i]) * l + u32::from(y[i]) * (255 - l) + 127) / 255;
            u8::try_from(v).unwrap_or(u8::MAX)
        };
        Argb(u32::from_be_bytes([at(0), at(1), at(2), at(3)]))
    }

    /// This colour with its alpha scaled by `level`, the way [`Argb::mix`]
    /// rounds: a bar or underline cursor part way through a fade.
    pub fn scaled(self, level: Chan) -> Argb {
        let a = (u32::from(self.a().get()) * u32::from(level.get()) + 127) / 255;
        self.with_alpha(Chan::from_u8(u8::try_from(a).unwrap_or(u8::MAX)))
    }

    /// Half way to `other`, channel by channel in encoded sRGB, rounded half
    /// up; this colour's alpha kept. Faint text drawn on `other`
    /// (note Y, decision 10): ghostty's `faint-opacity` default of 0.5, which
    /// ghostty applies as opacity and blends in the native, encoded space.
    /// Mixed in linear light instead, faint default text on the light page
    /// reads at 1.86:1 where the dark page gets 7.19:1; encoded, 2.96:1 and
    /// 4.07:1.
    pub(crate) fn faint_on(self, other: Argb) -> Argb {
        let [a, r, g, b] = self.channels();
        let [_, or, og, ob] = other.channels();
        let half = |x: u8, y: u8| {
            u8::try_from((u16::from(x) + u16::from(y)).div_ceil(2)).unwrap_or(u8::MAX)
        };
        Argb(u32::from_be_bytes([
            a,
            half(r, or),
            half(g, og),
            half(b, ob),
        ]))
    }
}

/// **The contrast floor** (note Y, decision 6): text whose contrast with the
/// colour it is drawn on is below `trigger` is lifted to `target`, by a
/// linear-light mix toward black or white that keeps its hue. Only a pair the
/// application did not choose exactly: see [`Floor::covers`]. On by default at
/// 1.2 and 4.5.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Floor {
    Off,
    Lift { trigger: f64, target: f64 },
}

impl Floor {
    /// The owner's defaults: a trigger of about 1.2 -- "fixing the invisible
    /// pairs of any convention, the built-in light theme's included" -- and
    /// WCAG 2.2's 4.5:1 for text (SC 1.4.3).
    pub const DEFAULT: Floor = Floor::Lift {
        trigger: 1.2,
        target: 4.5,
    };

    /// Whether the floor may touch a pair drawn from these two colour words:
    /// they differ -- unless both are the default, since text and page are
    /// two defaults and not one choice -- and **the page is ours** (the
    /// default or an index 0-15) **or the text is the default**. An
    /// application that set the same colour on both sides, or set the page
    /// itself as a cube or truecolour colour and chose the text on it, chose
    /// the pair and keeps it.
    ///
    /// **Not "either side ours"**, which note Y proposed: a real chafa
    /// `--colors 240` preview (`tests/previews/chafa-240.ans`) draws palette
    /// red, `38;5;1`, on ramp grey `48;5;239` in nine cells, 1.04:1 under the
    /// light theme, and that rule lifted them into pale speckles inside the
    /// picture. Over every capture in `tests/`, those are the only cells the
    /// two rules treat differently (measured, 2026-09-29).
    pub fn covers(text: ColorWord, page: ColorWord) -> bool {
        let same = text == page && !text.is_default();
        let ours = |w: ColorWord| w.is_default() || w.as_index().is_some_and(|i| i < 16);
        !same && (ours(page) || text.is_default())
    }

    /// `text` as drawn on `page`, lifted when this floor covers the pair and
    /// its contrast is below the trigger.
    ///
    /// **Toward black or white, in linear light, just far enough**: the text's
    /// linear channels are scaled toward 0 or toward 1, which keeps their
    /// order and so the hue, until the pair reaches `target`. The direction
    /// keeps the text on its own side of the page -- lighter text gets
    /// lighter -- unless that side cannot reach the target, and then the
    /// other; on a mid-grey page where neither can, the extreme with the
    /// higher ratio. `#00ff00` on the light page becomes `#007f00` at 4.53,
    /// `#ffffff` becomes `#6c6c6c`.
    pub fn lift(self, text: Argb, page: Argb, (tw, pw): (ColorWord, ColorWord)) -> Argb {
        let Floor::Lift { trigger, target } = self else {
            return text;
        };
        if !Floor::covers(tw, pw) {
            return text;
        }
        let (lt, lp) = (Lum::of(text), Lum::of(page));
        if lt.ratio(lp) >= trigger {
            return text;
        }
        Floor::lift_to(text, lp, Behind::solid(lp), target)
    }

    /// The trigger for text the window's dim made unreadable, beside the
    /// floor's own: the owner's call of 2026-10-17, measured on every
    /// capture first (the record, "A trigger of 3 for text the dim broke,
    /// measured: the owner's call"). WCAG 2.2's 3:1 is SC 1.4.3's minimum
    /// for large-scale text and SC 1.4.11's for graphical objects
    /// (`[[source: wcag22]]`).
    pub const GLASS_TRIGGER: f64 = 3.0;

    /// `text` as drawn on `page`, which the window draws at `alpha` over
    /// whatever its material shows, `backdrop`: lifted when the pair reads
    /// below a trigger against what can be behind it ([`Behind::over`]) and
    /// **not** below
    /// it on `page`. Two bands: the floor's own trigger, and
    /// [`Floor::GLASS_TRIGGER`] (or the floor's, when that is higher) for a
    /// pair asked at it or above, so a pair asked below 3 is never lifted by
    /// the second. `text` is what [`Floor::lift`] made of the pair, so a
    /// pair the floor covers has already been lifted on the page, and what
    /// is left below the trigger on it -- a program's own low-contrast pair
    /// on its own page -- the program chose and keeps. The colour drawn
    /// over a page the dim leaves solid is `text` itself.
    ///
    /// **Where no colour reaches the target against all of it**, the text
    /// is lifted as far as the glass trigger, and only where none reaches
    /// that either to the extreme that reads better: a pair the floor
    /// cannot bring to 4.5 ends where an untouched pair may already sit,
    /// keeping what hue it can (Jev's lean, `decisions.jsonl` 744-747).
    ///
    /// **Just far enough, in OKLab** (`Oklab::lift`): note Y decision 6
    /// lifts "just far enough to reach 4.5:1, keeping the hue", and over
    /// the glass a linear-light mix toward white washes the hue out. It drew
    /// btop's red `#7f3b3b` used segment `#c8b9b9` beside its `#404040`
    /// track at `#bcbcbc`, 0.017 apart in OKLab where btop drew them 0.118
    /// apart, so every meter read full. Moving the lightness with the hue
    /// and chroma kept draws them `#f7a8a5` and `#bcbcbc`, 0.095 apart, at
    /// the same contrast. Greys have no hue, so greys lifted to one target
    /// still land on one grey. Jev's lean, below the bar (`decisions.jsonl`
    /// 754-755), and open to the owner.
    pub fn lift_on_glass(self, text: Argb, page: Argb, alpha: Chan, backdrop: Backdrop) -> Argb {
        let Floor::Lift { trigger, target } = self else {
            return text;
        };
        let (lt, lp) = (Lum::of(text), Lum::of(page));
        let asked = lt.ratio(lp);
        let glass = trigger.max(Floor::GLASS_TRIGGER);
        let k = if asked >= glass { glass } else { trigger };
        let behind = Behind::over(page, alpha, backdrop);
        if behind.ratio(lt) >= k || asked < k {
            return text;
        }
        let to = if behind.reaches(target) || !behind.reaches(glass) {
            target
        } else {
            glass
        };
        let toward = Toward::of(lt, lp, behind, to);
        let (dark, light) = behind.bounds(to);
        let y = match toward {
            Toward::White => light.min(1.0),
            Toward::Black => dark.max(0.0),
        };
        Oklab::lift(text, toward, y)
    }

    /// `text` moved toward black or white until it reads at `target`
    /// against everything `behind` spans, or as far as it can:
    /// [`Floor::lift`]'s arithmetic, ungated. The side is the text's own
    /// against `page`, the page as asked.
    fn lift_to(text: Argb, page: Lum, behind: Behind, target: f64) -> Argb {
        let lt = Lum::of(text);
        let (dark, light) = behind.bounds(target);
        let (dark_ok, light_ok) = (dark >= 0.0, light <= 1.0);
        let toward = Toward::of(lt, page, behind, target);
        let [a, r, g, b] = text.channels();
        let out = |f: &dyn Fn(Linear) -> Chan| {
            let l = |v: u8| Linear::of(Chan::from_u8(v));
            [a, f(l(r)).get(), f(l(g)).get(), f(l(b)).get()]
        };
        let channels = if toward == Toward::White {
            let want = if light_ok { light } else { 1.0 };
            let t = if lt.0 >= 1.0 {
                0.0
            } else {
                ((want - lt.0) / (1.0 - lt.0)).clamp(0.0, 1.0)
            };
            out(&|c| Linear(c.0 + t * (1.0 - c.0)).encode_up())
        } else {
            let want = if dark_ok { dark } else { 0.0 };
            let s = if lt.0 <= 0.0 {
                0.0
            } else {
                (want / lt.0).clamp(0.0, 1.0)
            };
            out(&|c| Linear(c.0 * s).encode_down())
        };
        Argb(u32::from_be_bytes(channels))
    }
}

/// What the window's material can show under a page drawn below full
/// alpha: every channel of it lies between two greys, in encoded sRGB.
///
/// **[`Backdrop::any`], black to white, is Liquid Glass's**, bounded where
/// the material darkens nothing (note AS, section 3), and the bound for
/// any window whose material is not modelled, Linux's among them.
///
/// **The see-through standard material is far narrower**, because its
/// recipe lays an opaque grey over the blurred desktop (note AT, section
/// 4). Fed a black desktop and a white one, the recipe gives the two ends
/// of each half, and every desktop lands between them: the blurred,
/// saturated desktop is clipped to black..white, and every later step --
/// the grey over it, the lighten or darken, the tint -- rises with what it
/// is given. Rounded outward, they are
/// [`Backdrop::under_window_dark`] and [`Backdrop::under_window_light`].
/// **Modelled, not measured**: the recipe is the shipped Sidebar's, which
/// note AT takes as a stand-in for `.underWindowBackground`, whose own was
/// found in none of the system's catalogs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Backdrop {
    dark: Chan,
    light: Chan,
}

impl Backdrop {
    /// Every grey the backdrop spans, darkest first, as opaque colours.
    pub fn greys(self) -> impl Iterator<Item = Argb> {
        (self.dark.get()..=self.light.get()).map(|v| {
            let c = Chan::from_u8(v);
            Argb::rgb(c, c, c)
        })
    }

    /// Anything from black to white.
    pub fn any() -> Backdrop {
        Backdrop {
            dark: Chan::from_u8(0x00),
            light: Chan::from_u8(0xFF),
        }
    }

    /// The see-through standard material under the dark appearance: grey
    /// 33.9 over a black desktop and 91.6 over a white one (the Sidebar
    /// recipe: a 0.1569 grey at alpha 0.80 over the blurred desktop,
    /// lightened to 0.14, and 5% of the desktop's mean tint).
    pub fn under_window_dark() -> Backdrop {
        Backdrop {
            dark: Chan::from_u8(33),
            light: Chan::from_u8(92),
        }
    }

    /// The same under the light appearance: grey 196.3 over a black
    /// desktop and 234.4 over a white one (a 0.9647 grey at alpha 0.84,
    /// darkened to 0.915, and 5% of the tint).
    pub fn under_window_light() -> Backdrop {
        Backdrop {
            dark: Chan::from_u8(196),
            light: Chan::from_u8(235),
        }
    }
}

/// What can be behind text, as the darkest and the lightest luminance of
/// it: one colour for a page drawn solid, a range for a page the window
/// draws over its material.
#[derive(Clone, Copy)]
struct Behind {
    dark: Lum,
    light: Lum,
}

impl Behind {
    fn solid(page: Lum) -> Behind {
        Behind {
            dark: page,
            light: page,
        }
    }

    /// `page` at `alpha` over anything the material can show, `backdrop`:
    /// source-over in encoded sRGB, rounded as [`Argb::mix`] rounds, over
    /// its darkest grey for the darkest and its lightest for the lightest.
    /// A channel of the result rises with the backdrop's, so every colour
    /// whose channels lie between the two greys lands between the two.
    /// **The text reads against the part most like it**: WCAG 2.2's
    /// contrast is "with respect to the specified background over which
    /// the text is rendered" (`[[source: wcag22]]`, contrast ratio, note
    /// 3), and over an image, against "those parts of the image that are
    /// most like the text and behind the text"
    /// (`[[source: wcag22-technique-f83]]`).
    fn over(page: Argb, alpha: Chan, backdrop: Backdrop) -> Behind {
        let over = |v: Chan| page.mix(grey(v.get()), alpha);
        Behind {
            dark: Lum::of(over(backdrop.dark)),
            light: Lum::of(over(backdrop.light)),
        }
    }

    /// The text's luminance at most (darker than all of this) and at least
    /// (lighter than all of it) for it to read at `target`.
    fn bounds(self, target: f64) -> (f64, f64) {
        (
            (self.dark.0 + 0.05) / target - 0.05,
            target * (self.light.0 + 0.05) - 0.05,
        )
    }

    /// Whether some colour reads at `target` against all of this.
    fn reaches(self, target: f64) -> bool {
        let (dark, light) = self.bounds(target);
        dark >= 0.0 || light <= 1.0
    }

    /// The contrast of text of luminance `text` against the part of this
    /// most like it: 1 when the text falls inside the range.
    fn ratio(self, text: Lum) -> f64 {
        if text.0 >= self.light.0 {
            text.ratio(self.light)
        } else if text.0 <= self.dark.0 {
            text.ratio(self.dark)
        } else {
            1.0
        }
    }
}

/// Which way the floor moves text: toward white or toward black.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Toward {
    White,
    Black,
}

impl Toward {
    /// The text's own side of `page` -- lighter text gets lighter -- unless
    /// that side cannot reach `target` against `behind`, and then the other;
    /// where neither can, the extreme with the higher ratio.
    fn of(text: Lum, page: Lum, behind: Behind, target: f64) -> Toward {
        let (dark, light) = behind.bounds(target);
        let (dark_ok, light_ok) = (dark >= 0.0, light <= 1.0);
        match (text.0 >= page.0, dark_ok, light_ok) {
            (true, _, true) | (false, false, true) => Toward::White,
            (false, true, _) | (true, true, false) => Toward::Black,
            _ if behind.ratio(Lum(1.0)) >= behind.ratio(Lum(0.0)) => Toward::White,
            _ => Toward::Black,
        }
    }
}

/// A colour in OKLab (`[[source: ottosson-oklab]]`'s matrices): lightness
/// `l` and the opponent axes `a` and `b`, whose angle is the hue and length
/// the chroma.
#[derive(Clone, Copy)]
struct Oklab {
    l: f64,
    a: f64,
    b: f64,
}

/// The rows of OKLab's last matrix, which give each linear sRGB channel
/// from the cubed LMS: red, green, blue.
const TO_RGB: [[f64; 3]; 3] = [
    [4.076_741_662_1, -3.307_711_591_3, 0.230_969_929_2],
    [-1.268_438_004_6, 2.609_757_401_1, -0.341_319_396_5],
    [-0.004_196_086_3, -0.703_418_614_7, 1.707_614_701],
];

/// WCAG's luminance as weights on the cubed LMS: its three weights times
/// [`TO_RGB`].
const LUM_OF_LMS: [f64; 3] = {
    let (w, m) = ([0.2126, 0.7152, 0.0722], TO_RGB);
    [
        w[0] * m[0][0] + w[1] * m[1][0] + w[2] * m[2][0],
        w[0] * m[0][1] + w[1] * m[1][1] + w[2] * m[2][1],
        w[0] * m[0][2] + w[1] * m[1][2] + w[2] * m[2][2],
    ]
};

/// How a unit step of `(a, b)` moves the cube roots of L, M and S.
fn lms_step(a: f64, b: f64) -> [f64; 3] {
    [
        0.396_337_777_4 * a + 0.215_803_757_3 * b,
        -0.105_561_345_8 * a - 0.063_854_172_8 * b,
        -0.089_484_177_5 * a - 1.291_485_548 * b,
    ]
}

fn dot(w: [f64; 3], x: [f64; 3]) -> f64 {
    w[0] * x[0] + w[1] * x[1] + w[2] * x[2]
}

impl Oklab {
    fn of(r: f64, g: f64, b: f64) -> Oklab {
        let l = cbrt(0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b);
        let m = cbrt(0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b);
        let s = cbrt(0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b);
        Oklab {
            l: 0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
            a: 1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
            b: 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
        }
    }

    /// Linear sRGB, unclamped.
    fn rgb(self) -> [f64; 3] {
        let lms = lms_step(self.a, self.b).map(|k| {
            let v = self.l + k;
            v * v * v
        });
        TO_RGB.map(|w| dot(w, lms))
    }

    /// `text` with its hue and as much of its chroma as the sRGB gamut
    /// holds, at the lightness whose WCAG luminance is `y`: at least `y`
    /// toward white, at most toward black, the encoded channels rounded the
    /// same way so the contrast reached is never undershot.
    ///
    /// **The lightness is solved, not stepped.** A grey has no hue and its
    /// luminance is its linear channel, so it is `y` itself. Where the
    /// whole chroma fits, the luminance is a cubic in the lightness and
    /// four Newton steps settle it. Otherwise a secant, kept inside a
    /// bracket, runs on the luminance of the colour as drawn, clip and
    /// clamp included, with [`Hue`]'s closed-form clip, so the end of the
    /// bracket returned reads at `y` whatever the clip's own error. The
    /// nested bisection this replaces took 8.3 us for btop's `#7f3b3b` and
    /// this 55 ns, about 200-240 ns where the gamut clips (205 for
    /// `#dbbfef` under the light glass, 240 for `#cd0000` on black at alpha
    /// 179), against 17-30 ns for the linear-light mix. IEEE arithmetic
    /// only, `sqrt` included, so the colour does not depend on a platform
    /// maths library; not yet run on Linux.
    fn lift(text: Argb, toward: Toward, y: f64) -> Argb {
        let [alpha, r, g, b] = text.channels();
        let enc = match toward {
            Toward::White => Linear::encode_up,
            Toward::Black => Linear::encode_down,
        };
        if r == g && g == b {
            return Oklab::grey(Chan::from_u8(alpha), enc(Linear(y)), toward, y);
        }
        let lin = |v: u8| Linear::of(Chan::from_u8(v)).0;
        let lab = Oklab::of(lin(r), lin(g), lin(b));
        let lum = |c: [f64; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let draw = |c: [f64; 3]| {
            let [r, g, b] = c.map(|v| enc(Linear(v)).get());
            Argb(u32::from_be_bytes([alpha, r, g, b]))
        };
        // The bracket's ends are known without drawing them: the text at
        // its own lightness, and white or black, which no clip moves.
        let own = Lum::of(text).0 - y;
        let ((mut lo, mut flo), (mut hi, mut fhi)) = match toward {
            Toward::White if own >= 0.0 => return text,
            Toward::Black if own < 0.0 => return text,
            Toward::White => ((lab.l, own), (1.0, 1.0 - y)),
            Toward::Black => ((0.0, -y), (lab.l, own)),
        };
        // Aimed a hair past `y`, on the side that reads, so the lightness
        // found is one that may be drawn.
        let aim = match toward {
            Toward::White => 5e-8,
            Toward::Black => -5e-8,
        };
        // **Where the whole chroma fits**, the luminance is a cubic in the
        // lightness alone, and Newton's method from the grey's lightness,
        // the cube root of `y`, takes four steps to settle; the colour is
        // drawn if it is inside sRGB and reads, and otherwise the clip
        // decides it below.
        let d = lms_step(lab.a, lab.b);
        let newton = (0..4).fold(cbrt(y), |l, _| {
            let v = d.map(|d| l + d);
            let err = dot(LUM_OF_LMS, v.map(|v| v * v * v)) - y - aim;
            l - err / (3.0 * dot(LUM_OF_LMS, v.map(|v| v * v)))
        });
        let whole = Oklab { l: newton, ..lab }.rgb();
        let fits = whole.iter().all(|v| (0.0..=1.0).contains(v)) && newton > lo && newton < hi;
        let reads = match toward {
            Toward::White => lum(whole) >= y,
            Toward::Black => lum(whole) < y,
        };
        if fits && reads {
            return draw(whole);
        }
        let chroma = (lab.a * lab.a + lab.b * lab.b).sqrt();
        let hue = Hue::of(lab.a / chroma, lab.b / chroma);
        let at = |l: f64| hue.at(l, chroma.min(hue.max_chroma(l)));
        let f = |l: f64| lum(at(l)) - y;
        let seed = if newton > lo && newton < hi {
            newton
        } else {
            cbrt(y)
        };
        let (mut p, mut q) = ((lo, flo), (seed, f64::NAN));
        for _ in 0..24 {
            let short = match toward {
                Toward::White => fhi,
                Toward::Black => -flo,
            };
            if short <= 1e-7 || hi - lo <= 1e-12 {
                break;
            }
            let x = if q.1.is_nan() {
                q.0
            } else {
                q.0 - (q.1 - aim) * (q.0 - p.0) / (q.1 - p.1)
            };
            let x = if x > lo && x < hi { x } else { (lo + hi) / 2.0 };
            let fx = f(x);
            if fx >= 0.0 {
                (hi, fhi) = (x, fx);
            } else {
                (lo, flo) = (x, fx);
            }
            (p, q) = (if q.1.is_nan() { p } else { q }, (x, fx));
        }
        draw(at(match toward {
            Toward::White => hi,
            Toward::Black => lo,
        }))
    }

    /// The grey `v`, stepped a code at a time until it reads at `y`: a
    /// grey's luminance is its linear channel times weights that sum to 1
    /// only to within rounding.
    fn grey(alpha: Chan, v: Chan, toward: Toward, y: f64) -> Argb {
        let grey = |v: u8| Argb(u32::from_be_bytes([alpha.get(), v, v, v]));
        let mut v = v.get();
        match toward {
            Toward::White => {
                while Lum::of(grey(v)).0 < y && v < u8::MAX {
                    v += 1;
                }
            }
            Toward::Black => {
                while Lum::of(grey(v)).0 > y && v > 0 {
                    v -= 1;
                }
            }
        }
        grey(v)
    }
}

/// One OKLab hue, the unit direction `(a, b)`, with the sRGB gamut's cusp
/// at it: the lightness and chroma of its most saturated colour
/// (`[[source: ottosson-gamutclipping]]`, `find_cusp`). At one hue the
/// gamut is bounded below by the line from black to the cusp, exactly --
/// scaling linear sRGB keeps the hue and keeps the zero channel at zero --
/// and above by a curve from the cusp to white, reached from its chord by
/// one Halley step (`find_gamut_intersection`). Re-expressed from that
/// post's code, MIT, (c) 2021 Björn Ottosson: LICENSES/ottosson-MIT.txt.
struct Hue {
    a: f64,
    b: f64,
    k: [f64; 3],
    cusp_l: f64,
    cusp_c: f64,
}

impl Hue {
    fn of(a: f64, b: f64) -> Hue {
        let k = lms_step(a, b);
        let s = Hue::max_saturation(a, b, k);
        let [r, g, bl] = Oklab {
            l: 1.0,
            a: s * a,
            b: s * b,
        }
        .rgb();
        let cusp_l = cbrt(1.0 / r.max(g).max(bl));
        Hue {
            a,
            b,
            k,
            cusp_l,
            cusp_c: cusp_l * s,
        }
    }

    /// The largest chroma over lightness this hue has inside sRGB, where
    /// the first channel to fall to zero does: Ottosson's fitted polynomial,
    /// then one Halley step on that channel (`compute_max_saturation`).
    fn max_saturation(a: f64, b: f64, k: [f64; 3]) -> f64 {
        let (c, w) = if -1.881_703_28 * a - 0.809_364_93 * b > 1.0 {
            (
                [
                    1.190_862_77,
                    1.765_767_28,
                    0.596_626_41,
                    0.755_151_97,
                    0.567_712_45,
                ],
                TO_RGB[0],
            )
        } else if 1.814_441_04 * a - 1.194_452_76 * b > 1.0 {
            (
                [
                    0.739_565_15,
                    -0.459_544_04,
                    0.082_854_27,
                    0.125_410_7,
                    0.145_032_04,
                ],
                TO_RGB[1],
            )
        } else {
            (
                [
                    1.357_336_52,
                    -0.009_157_99,
                    -1.151_302_1,
                    -0.505_596_06,
                    0.006_921_67,
                ],
                TO_RGB[2],
            )
        };
        let s = c[0] + c[1] * a + c[2] * b + c[3] * a * a + c[4] * a * b;
        let v = k.map(|k| 1.0 + s * k);
        let f = dot(w, v.map(|v| v * v * v));
        let f1 = dot(w, [0, 1, 2].map(|i| 3.0 * k[i] * v[i] * v[i]));
        let f2 = dot(w, [0, 1, 2].map(|i| 6.0 * k[i] * k[i] * v[i]));
        s - f * f1 / (f1 * f1 - 0.5 * f * f2)
    }

    /// The largest chroma this hue has inside sRGB at lightness `l`.
    fn max_chroma(&self, l: f64) -> f64 {
        if l <= self.cusp_l {
            return self.cusp_c * l / self.cusp_l;
        }
        let c = self.cusp_c * (1.0 - l) / (1.0 - self.cusp_l);
        let v = self.k.map(|k| l + c * k);
        let cube = v.map(|v| v * v * v);
        let d1 = [0, 1, 2].map(|i| 3.0 * self.k[i] * v[i] * v[i]);
        let d2 = [0, 1, 2].map(|i| 6.0 * self.k[i] * self.k[i] * v[i]);
        let step = TO_RGB
            .map(|w| {
                let (f, f1, f2) = (dot(w, cube) - 1.0, dot(w, d1), dot(w, d2));
                let u = f1 / (f1 * f1 - 0.5 * f * f2);
                if u >= 0.0 { -f * u } else { f64::MAX }
            })
            .into_iter()
            .fold(f64::MAX, f64::min);
        (c + step).max(0.0)
    }

    /// Linear sRGB of lightness `l` and chroma `c` at this hue, clamped
    /// into the gamut.
    fn at(&self, l: f64, c: f64) -> [f64; 3] {
        Oklab {
            l,
            a: self.a * c,
            b: self.b * c,
        }
        .rgb()
        .map(|v| v.clamp(0.0, 1.0))
    }
}

/// The cube root of a positive `x`, by IEEE arithmetic alone: scaled by
/// eights into [1/8, 1], seeded there by the cubic through cube roots at
/// four Chebyshev nodes (1.3% at worst), then two Halley steps, which
/// cube that error each.
fn cbrt(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let (mut m, mut scale) = (x, 1.0);
    while m < 0.125 {
        m *= 8.0;
        scale /= 2.0;
    }
    while m > 1.0 {
        m /= 8.0;
        scale *= 2.0;
    }
    let seed = 0.358_367_320_930_220_9
        + m * (1.315_345_817_852_591
            + m * (-1.088_833_716_892_957_4 + m * 0.416_638_806_536_472_3));
    let y = (0..2).fold(seed, |y: f64, _| {
        let c = y * y * y;
        y * (c + 2.0 * m) / (2.0 * c + m)
    });
    y * scale
}

/// WCAG 2.2 relative luminance (`[[source: wcag22]]`), 0 to 1, through
/// `baud_domain::LINEAR`, the one sRGB decode the 996 classifier uses.
#[derive(Clone, Copy)]
struct Lum(f64);

impl Lum {
    fn of(c: Argb) -> Lum {
        let [_, r, g, b] = c.channels();
        let l = |v: u8| Linear::of(Chan::from_u8(v)).0;
        Lum(0.2126 * l(r) + 0.7152 * l(g) + 0.0722 * l(b))
    }

    /// The contrast ratio of the two, the lighter over the darker.
    fn ratio(self, other: Lum) -> f64 {
        (self.0.max(other.0) + 0.05) / (self.0.min(other.0) + 0.05)
    }
}

/// One channel in linear light, 0 to 1.
#[derive(Clone, Copy)]
struct Linear(f64);

impl Linear {
    fn of(c: Chan) -> Linear {
        Linear(f64::from(LINEAR[usize::from(c.get())]) / 65535.0)
    }

    /// The largest channel value whose linear light is at most this: rounding
    /// toward black never undershoots the contrast the lift reached.
    fn encode_down(self) -> Chan {
        let n = LINEAR.partition_point(|&v| f64::from(v) / 65535.0 <= self.0);
        Chan::from_u8(u8::try_from(n.saturating_sub(1)).unwrap_or(u8::MAX))
    }

    /// The smallest channel value whose linear light is at least this.
    fn encode_up(self) -> Chan {
        let n = LINEAR.partition_point(|&v| f64::from(v) / 65535.0 < self.0);
        Chan::from_u8(u8::try_from(n).unwrap_or(u8::MAX))
    }
}

/// The colours a rendition resolves against. The 256-entry palette is
/// xterm's by default and an application may repaint it through `OSC 4`,
/// which the core records and [`Theme::resolve`] reads back.
#[derive(Clone)]
pub struct Theme {
    pub fg: Argb,
    pub bg: Argb,
    /// The alpha **every** background is painted with -- the default one
    /// and any an application sets -- so a window's translucency is not a
    /// property only the unstyled cells have. Opaque unless a window wants
    /// the glass through; `baud-render/tests/translucent.rs` is the gate,
    /// and it is the only one in the workspace that renders below `0xFF`.
    pub bg_alpha: Chan,
    /// Whether a background the application set is drawn under the dim
    /// or above it. The default is [`AppBg::Dimmed`]: every background is
    /// translucent, which is the owner's 2026-09-20 call. [`AppBg::Opaque`]
    /// keeps every background an application set at full alpha and leaves
    /// only the default one glass; see [`Theme::opaque_bg`].
    pub app_bg: AppBg,
    /// Whether a picture drawn in text keeps its backgrounds solid under
    /// [`AppBg::Dimmed`]. See [`PictureBg`].
    pub picture_bg: PictureBg,
    /// How a Sixel's background fill draws on the glass. See [`SixelBg`].
    pub sixel_bg: SixelBg,
    /// The contrast floor, [`Floor::DEFAULT`] on both built-in themes.
    pub floor: Floor,
    /// What the window's material can show under a page drawn below full
    /// alpha, which the floor reads text against ([`Floor::lift_on_glass`]).
    /// [`Backdrop::any`] on both built-in themes.
    pub backdrop: Backdrop,
    pub palette: [Argb; 256],
    /// **The selection's colours, the `Theme` selection setting**: the
    /// page under selected text and the selected text, each `None` for no
    /// colour of its own -- both `None` is reverse video, the default (the
    /// owner's V13 call; Jev, 1.00 both orders, with renders beside
    /// Terminal.app's and iTerm2's). A page alone draws each selected
    /// cell's own text on it. A program's `OSC 17`/`19` wins over each
    /// while it is set ([`Renderer::hilite`](crate::Renderer::hilite)). A
    /// theme file may name the cell's own text or page instead of a colour
    /// ([`CellPaint`]).
    pub selection: Option<CellPaint<Argb>>,
    pub selection_text: Option<CellPaint<Argb>>,
    /// **The cursor's colours**: its fill, and the text under a block,
    /// each a colour, the cell's own text or page, or `None` for the cell's
    /// colours swapped. A program's `OSC 12` wins over the fill while set,
    /// and leaves the text colour as it is
    /// ([`Renderer::block_paint`](crate::Renderer::block_paint)).
    pub cursor: Option<CellPaint<Argb>>,
    pub cursor_text: Option<CellPaint<Argb>>,
    /// Under which cursor [`Theme::cursor_text`] is drawn: every one, or
    /// after a kitty theme's `cursor none` only one with a colour
    /// ([`Theme::block_text`]).
    pub cursor_text_under: CursorText,
    /// What the selection's text answers where [`Theme::selection_text`]
    /// is unset: reverse video's, or a kitty theme's
    /// `selection_foreground none`. Drawn the same either way.
    pub unset_selection_text: SelectionText,
}

/// What the window's dim does to a background the application set.
///
/// **Why the rule is every set background and not something narrower**, and
/// the one exception [`PictureBg`] carves out of it by looking at a block of
/// cells rather than at one,
/// measured 2026-09-22 on the owner's own yazi preview: a file manager
/// with no image protocol draws a photo with chafa, whose output is a
/// grid of cells each carrying its own foreground *and* background --
/// block elements, box-drawing strokes, and plain spaces for the flat
/// areas. On that .heic 68% of the preview's area was cell background, and
/// on a photo with flat regions most cells are spaces. A space on a set
/// background is also every TUI panel there is, so no rule that looks at
/// one cell can keep the image's spaces opaque and btop's glass; the
/// block-elements-only rule this replaced covered 66% of that photo's
/// cells and 19% of another's. So the choice is the owner's, per window,
/// and it is this key -- and under `Dimmed`, [`PictureBg`] finds the
/// pictures from the blocks they make and draws those solid.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AppBg {
    /// The same alpha as the default background.
    #[default]
    Dimmed,
    /// Opaque: a colour an application chose is drawn as it chose it.
    Opaque,
}

/// What the window's dim does to a picture drawn in text, under
/// [`AppBg::Dimmed`] -- a block of set backgrounds holding at least
/// [`crate::PICTURE_COLOURS`] distinct colours, found by
/// [`crate::Pictures`]. The owner's call, 2026-09-22: panels stay glass and
/// pictures are drawn solid, because under the dim a picture splits into
/// solid glyphs beside see-through backgrounds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PictureBg {
    /// A picture's backgrounds are drawn solid.
    #[default]
    Opaque,
    /// A picture is dimmed like every other background.
    Dimmed,
}

/// **How a Sixel's background fill draws on the see-through window**:
/// `[glass] sixel_backgrounds`, the owner's call of 2026-10-22, "i would
/// say b is default with config option to change it". A Sixel with `P2` 0
/// or 2 sets the positions its data leaves at zero to the background
/// (`[[source: vt3xx-sixel]]`); no DEC text knows a see-through window,
/// so how that background meets the glass is this setting. On an opaque
/// window, or under Reduce Transparency, every choice draws as
/// [`SixelBg::Opaque`] does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SixelBg {
    /// The fill solid, as on an opaque window: foot's practice, which
    /// fills with register 0 at full alpha and draws the image with
    /// `PIXMAN_OP_SRC` (`[[source: foot-sixel-c]]`,
    /// `[[source: foot-render]]`).
    #[default]
    Opaque,
    /// The fill at the window's dim, over what is under it: the text and
    /// any earlier image show through it, dimmed.
    Dimmed,
    /// The fill replaces what is under it with the background at the
    /// window's dim: the text and any earlier image under it are gone, as
    /// erased cells look.
    Cleared,
}

/// Whether the screen is drawn in reverse video: `?5`, DECSCNM -- baud-domain's
/// [`baud_domain::Video`], the one spelling of the per-cell exchange the core
/// reads too, so the colour `OSC 10`/`11` set under `?5` is the one drawn in
/// the role it named. Here each cell's pair is resolved as stored and then
/// exchanged, once for `?5` and once for the cell's own `SGR 7`, so reverse
/// inside reverse video draws a normal cell.
///
/// **Every background keeps the window's alpha under it**, the reversed
/// default one included: the owner's call, 2026-09-25, "same alpha for all".
pub use baud_domain::Video;

impl Default for Theme {
    fn default() -> Self {
        Theme::dark()
    }
}

impl Theme {
    /// Light text on near-black, with [`dark_palette`]: xterm's 256 with
    /// four of the sixteen made readable on this page.
    pub fn dark() -> Theme {
        Theme {
            fg: grey(0xD8),
            bg: grey(0x10),
            bg_alpha: Chan::from_u8(0xFF),
            app_bg: AppBg::Dimmed,
            picture_bg: PictureBg::Opaque,
            sixel_bg: SixelBg::Opaque,
            floor: Floor::DEFAULT,
            backdrop: Backdrop::any(),
            palette: dark_palette(),
            selection: None,
            selection_text: None,
            cursor: None,
            cursor_text: None,
            cursor_text_under: CursorText::UnderEveryCursor,
            unset_selection_text: SelectionText::Reverse,
        }
    }

    /// Dark text on near-white, with [`light_palette`]: [`MODUS_OPERANDI`]
    /// as the sixteen, entry 7 made readable on this page.
    ///
    /// The two greys are [`Theme::dark`]'s mirrored channel for channel --
    /// `0xFF - 0xD8` is `0x27` and `0xFF - 0x10` is `0xEF` -- so the pair
    /// carries the same contrast rather than a second opinion about what
    /// "near-black" should be. Neither end is pure: `dark` does not use
    /// `0xFF` because white on black glares, and the mirror inherits that.
    ///
    /// **The sixteen are not xterm's**, which it had been: xterm keeps one
    /// palette under either background, so on this page its bright green
    /// read at 1.19:1 and its bright cyan at 1.09:1. The owner chose Modus
    /// Operandi's (note Y, decision 3), whose colours are made for a light
    /// page; the cube and the grey ramp stay xterm's.
    pub fn light() -> Theme {
        Theme {
            fg: grey(0x27),
            bg: grey(0xEF),
            bg_alpha: Chan::from_u8(0xFF),
            app_bg: AppBg::Dimmed,
            picture_bg: PictureBg::Opaque,
            sixel_bg: SixelBg::Opaque,
            floor: Floor::DEFAULT,
            backdrop: Backdrop::any(),
            palette: light_palette(),
            selection: None,
            selection_text: None,
            cursor: None,
            cursor_text: None,
            cursor_text_under: CursorText::UnderEveryCursor,
            unset_selection_text: SelectionText::Reverse,
        }
    }

    /// Lays a theme file's scheme over this half: each of the sixteen, the
    /// page, the text and the selection's two colours that the file sets
    /// replaces the half's, and the
    /// count of those it leaves unset -- which stay the half's -- comes back
    /// for the caller to report. Entries 16-255 are not written whatever the
    /// file says (note Y, decision 2): a program addresses them by number and
    /// expects xterm's cube. The dim, the floor and the exemptions stay the
    /// window's.
    pub fn take_scheme(&mut self, scheme: &Scheme) -> Count {
        let mut unset = 0;
        let pairs = self.palette[..16].iter_mut().zip(&scheme.ansi);
        let named = [(&mut self.bg, &scheme.page), (&mut self.fg, &scheme.text)];
        for (slot, c) in pairs.chain(named) {
            match (*c).and_then(argb_of) {
                Some(a) => *slot = a,
                None => unset += 1,
            }
        }
        // The selection's and the cursor's colours are not counted: a theme
        // that leaves them unset leaves the cell's colours swapped, which is
        // no colour of baud's.
        let painted = [
            (&mut self.selection, &scheme.selection),
            (&mut self.selection_text, &scheme.selection_text),
            (&mut self.cursor, &scheme.cursor),
            (&mut self.cursor_text, &scheme.cursor_text),
        ];
        for (slot, p) in painted {
            let landed = match *p {
                Some(CellPaint::Colour(c)) => argb_of(c).map(CellPaint::Colour),
                Some(CellPaint::Text) => Some(CellPaint::Text),
                Some(CellPaint::Page) => Some(CellPaint::Page),
                None => None,
            };
            if landed.is_some() {
                *slot = landed;
            }
        }
        // What the file's none leaves to resolve where it is drawn.
        self.cursor_text_under = scheme.cursor_text_under;
        self.unset_selection_text = scheme.unset_selection_text;
        Count::of(unset)
    }

    /// **The text under a block cursor filled with `fill`**: the theme's,
    /// except under a cursor of no colour of its own -- the cell's colours
    /// swapped, `None` -- where a kitty theme's `cursor none` sets it aside
    /// ([`CursorText::UnderACursorColour`]). `fill` is the one in force
    /// when the frame is drawn, a program's or the config's included, so
    /// a cursor colour set over the file's none brings the text back.
    pub fn block_text(&self, fill: Option<CellPaint<Argb>>) -> Option<CellPaint<Argb>> {
        match (self.cursor_text_under, fill) {
            (CursorText::UnderACursorColour, None) => None,
            _ => self.cursor_text,
        }
    }

    /// Dark or light by the rule `CSI ? 996 n` answers with: the page's
    /// relative luminance against the text's.
    pub fn polarity(&self) -> Option<baud_domain::ColorScheme> {
        let word = |a: Argb| {
            let [_, r, g, b] = a.channels();
            ColorWord::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
        };
        baud_domain::ColorScheme::of(word(self.fg), word(self.bg))
    }

    /// Whether a cell drawn under `style` keeps its background opaque: the
    /// exemption is on and the background it resolves to under `video` is
    /// one the application set. A pure function of the rendition and the
    /// video, so the cell path bakes it into the style table, which a change
    /// of video resolves again, and the shader reads one bit.
    pub fn opaque_bg(&self, style: StyleWord, video: Video) -> bool {
        self.app_bg == AppBg::Opaque && Self::set_bg(style, video)
    }

    /// Whether pictures drawn in text are looked for at all: only under the
    /// dim, only while the pictures are asked for solid, and only when the
    /// dim is below opaque -- otherwise every background is already what a
    /// picture would be drawn as, and finding them would change nothing.
    pub fn finds_pictures(&self) -> bool {
        self.app_bg == AppBg::Dimmed
            && self.picture_bg == PictureBg::Opaque
            && self.bg_alpha.get() < 0xFF
    }

    /// Whether the background `style` resolves to under `video` is one the
    /// application set rather than the default -- under exactly one of
    /// `SGR 7` and `?5` that is the foreground, because each exchanges them
    /// ([`Video::drawn`]).
    pub fn set_bg(style: StyleWord, video: Video) -> bool {
        let (_, page) = video.drawn(style.attrs(), style.fg(), style.bg());
        !page.is_default()
    }

    /// Whether a background drawn in `drawn` -- the colour [`Theme::resolve`]
    /// gives a cell's background, after `SGR 7` and `?5` exchange the pair --
    /// is the default background, whose colour is `default`, the page's as
    /// stored ([`Theme::defaults`] under [`Video::Normal`]: the theme's, or
    /// `OSC 11`'s). By the colour, not the word: an explicit colour equal to
    /// the default's is default, as kitty's `cell_has_default_bg` compares
    /// the drawn colour (`[[source: kitty-background-slang]]`), the owner's
    /// pick of 2026-10-16. Alpha is the window's and not the colour's, so
    /// it is not compared. What a kitty image below `INT32_MIN/2` shows
    /// through (`[[source: kitty-graphics]]`).
    pub fn default_bg(drawn: Argb, default: Argb) -> bool {
        let opaque = Chan::from_u8(0xFF);
        drawn.with_alpha(opaque) == default.with_alpha(opaque)
    }

    /// The foreground and background a rendition draws with, after the
    /// attribute rules: bold brightens the first eight palette entries,
    /// `?5` and `SGR 7` each exchange the pair ([`Video::drawn`]), and faint
    /// mixes the drawn text half way to the colour it is drawn on.
    ///
    /// **Under `?5` every cell is exchanged, an explicit colour included**
    /// (DEC STD 070 § 5.6.3.2, `[[source: dec-std-070]]` page 5-107, PDF
    /// 362: the mode "will affect which attribute bits from display memory
    /// are used to render the text foreground and background color"; the
    /// owner's decision of 2026-10-13). The pair is resolved from the stored
    /// defaults and the stored palette and only then exchanged, so no entry
    /// moves and a default cell comes out as the defaults swapped. Bold
    /// brightens before the exchange, as the VT525 order j4james reports
    /// does (`[[source: gh-ckwin-547-comments]]`, not a DEC text).
    /// Hiding is not a colour and is not here: `Resolved::text`.
    pub fn resolve(
        &self,
        style: StyleWord,
        video: Video,
        palette: impl Fn(Chan) -> Option<(u8, u8, u8)>,
        dynamic: impl Fn(DynamicColor) -> Option<(u8, u8, u8)>,
    ) -> (Argb, Argb) {
        let attrs = style.attrs();
        // The stored pair: the exchange below is the only thing `?5` does
        // to a cell.
        let (fg_default, bg_default) = self.defaults(Video::Normal, &dynamic);
        let pick = |w: ColorWord, default: Argb, bright: bool| -> Argb {
            if let Some((r, g, b)) = w.as_rgb() {
                return triple(r, g, b);
            }
            if let Some(i) = w.as_index() {
                let i = if bright && i < 8 { i + 8 } else { i };
                return self.entry(Chan::from_u8(i), &palette);
            }
            default
        };
        let bold = attrs.has(Attrs::BOLD);
        let (mut fg, bg) = video.drawn(
            attrs,
            pick(style.fg(), fg_default, bold),
            pick(style.bg(), bg_default, false),
        );
        // **After the exchange**: faint dims the text that is drawn, which
        // under `SGR 7` or `?5` is the background's colour, toward the band
        // it is drawn on. Before it, it would dim the band instead.
        if attrs.has(Attrs::DIM) {
            fg = fg.faint_on(bg);
        }
        // `SGR 8` is not a colour: concealed text keeps its foreground, so a
        // block cursor over it is drawn in it, and draws nothing of its own
        // (`Resolved::text`). Drawing it in the background's colour, as this
        // did, left an opaque silhouette on every translucent background.
        (fg, bg)
    }

    /// The default foreground and background the screen draws under `video`:
    /// the theme's unless the application moved them with `OSC 10`/`11` --
    /// consulted first, the way `palette` is consulted before the theme's own
    /// table -- and swapped under reverse video, so the default a program
    /// moved is the one `?5` puts on the other side. What a margin, a
    /// scrollbar or a fade drawn "in the foreground" or "in the background"
    /// takes.
    pub fn defaults(
        &self,
        video: Video,
        dynamic: impl Fn(DynamicColor) -> Option<(u8, u8, u8)>,
    ) -> (Argb, Argb) {
        // The stored slot each role names under `video`, through the one
        // mapping the core's `OSC 10`/`11` use as well.
        let stored = |w: DynamicColor| {
            let theme = match w {
                DynamicColor::Background => self.bg,
                DynamicColor::Foreground
                | DynamicColor::Cursor
                | DynamicColor::HighlightBg
                | DynamicColor::HighlightFg => self.fg,
            };
            dynamic(w).map_or(theme, |(r, g, b)| triple(r, g, b))
        };
        (
            stored(video.role(DynamicColor::Foreground)),
            stored(video.role(DynamicColor::Background)),
        )
    }

    /// Palette entry `i`: the application's `OSC 4` override, else the
    /// theme's table.
    pub(crate) fn entry(&self, i: Chan, palette: impl Fn(Chan) -> Option<(u8, u8, u8)>) -> Argb {
        match palette(i) {
            Some((r, g, b)) => triple(r, g, b),
            None => self.palette[usize::from(i.get())],
        }
    }
}

impl Theme {
    /// The colour an underline draws in: `SGR 58`'s when set, else the
    /// resolved foreground -- the text as drawn, so under `?5` an underline
    /// that follows the text follows the exchange. An `SGR 58` colour is
    /// neither the foreground nor the background DEC's `DECSCNM` names, and
    /// is drawn as set under either video.
    pub fn underline(
        &self,
        style: StyleWord,
        fg: Argb,
        palette: impl Fn(Chan) -> Option<(u8, u8, u8)>,
    ) -> Argb {
        let w = style.underline();
        if let Some((r, g, b)) = w.as_rgb() {
            return triple(r, g, b);
        }
        if let Some(i) = w.as_index() {
            return self.entry(Chan::from_u8(i), palette);
        }
        fg
    }
}

fn triple(r: u8, g: u8, b: u8) -> Argb {
    Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

/// A scheme colour as a palette entry; a scheme holds only `Rgb`.
fn argb_of(c: Color) -> Option<Argb> {
    match c {
        Color::Rgb(r, g, b) => Some(Argb::rgb(r, g, b)),
        Color::Default | Color::Indexed(_) => None,
    }
}

fn grey(v: u8) -> Argb {
    triple(v, v, v)
}

/// The sixteen of the light theme: **Modus Operandi by Protesilaos Stavrou,
/// (C) Free Software Foundation, GPL-3.0-or-later** -- its Emacs `ansi-color`
/// mapping, the sixteen hex values and no file text (note Y, decision 3;
/// `[[source: modus-operandi-theme-el]]`, pinned external). Designed as text
/// on a near-white page: every colour entry from 1 to 6 and 9 to 14 is at
/// least 6.09:1 on this theme's `0xEF`. Entries 7 and 15 are the light greys
/// that sit near the page, 2.12 and 1.15; [`light_palette`] darkens 7, and
/// 15 is the page's own extreme. These are Modus Operandi's own values, and
/// baud's one change is made over them, not in them.
pub const MODUS_OPERANDI: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0xA6, 0x00, 0x00),
    (0x00, 0x68, 0x00),
    (0x6F, 0x55, 0x00),
    (0x00, 0x31, 0xA9),
    (0x72, 0x10, 0x45),
    (0x00, 0x5E, 0x8B),
    (0xA6, 0xA6, 0xA6),
    (0x59, 0x59, 0x59),
    (0x97, 0x25, 0x00),
    (0x00, 0x66, 0x3F),
    (0x88, 0x49, 0x00),
    (0x35, 0x48, 0xCF),
    (0x53, 0x1A, 0xB6),
    (0x00, 0x5F, 0x5F),
    (0xFF, 0xFF, 0xFF),
];

/// xterm's 256 with [`MODUS_OPERANDI`] as the sixteen: the cube and the grey
/// ramp stay xterm's, because an application addresses them by number and
/// expects their fixed values.
///
/// **Entry 7 is `#6c6c6c`, not Operandi's `#a6a6a6`**: 4.57:1 on the page
/// where Operandi's grey reads at 2.12, the one change to Operandi, its OKLCH
/// lightness only. The owner's answer of 2026-10-18 to note Y's decision 3.
pub fn light_palette() -> [Argb; 256] {
    let mut p = xterm_palette();
    for (slot, &(r, g, b)) in p.iter_mut().zip(&MODUS_OPERANDI) {
        *slot = triple(r, g, b);
    }
    p[7] = triple(0x6C, 0x6C, 0x6C);
    p
}

/// The four of xterm's sixteen that read below 4.5:1 on the dark page,
/// `0x10`, and what each becomes: the colour with its OKLCH lightness raised
/// until it reads at 4.5, hue kept (note Y, decision 8; the owner's answer of
/// 2026-10-18). Red 3.26 to 4.50, blue 2.02 to 4.50, magenta 4.06 to 4.51,
/// bright blue 4.01 to 4.52. The two blues lose chroma too, because sRGB
/// holds no blue that light at xterm's chroma.
const DARK_FIXES: [(usize, (u8, u8, u8)); 4] = [
    (1, (0xE9, 0x33, 0x27)),
    (4, (0x37, 0x71, 0xFF)),
    (5, (0xD6, 0x1B, 0xD5)),
    (12, (0x64, 0x69, 0xFE)),
];

/// xterm's 256 with `DARK_FIXES` over four of the sixteen: every other
/// entry is xterm's.
pub fn dark_palette() -> [Argb; 256] {
    let mut p = xterm_palette();
    for &(i, (r, g, b)) in &DARK_FIXES {
        p[i] = triple(r, g, b);
    }
    p
}

/// xterm's 256-colour palette: the sixteen ANSI colours, a 6x6x6 cube and
/// a 24-step grey ramp.
pub fn xterm_palette() -> [Argb; 256] {
    let mut p = [Argb(0); 256];
    const ANSI: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xCD, 0x00, 0x00),
        (0x00, 0xCD, 0x00),
        (0xCD, 0xCD, 0x00),
        (0x00, 0x00, 0xEE),
        (0xCD, 0x00, 0xCD),
        (0x00, 0xCD, 0xCD),
        (0xE5, 0xE5, 0xE5),
        (0x7F, 0x7F, 0x7F),
        (0xFF, 0x00, 0x00),
        (0x00, 0xFF, 0x00),
        (0xFF, 0xFF, 0x00),
        (0x5C, 0x5C, 0xFF),
        (0xFF, 0x00, 0xFF),
        (0x00, 0xFF, 0xFF),
        (0xFF, 0xFF, 0xFF),
    ];
    for (i, &(r, g, b)) in ANSI.iter().enumerate() {
        p[i] = triple(r, g, b);
    }
    const STEPS: [u8; 6] = [0, 0x5F, 0x87, 0xAF, 0xD7, 0xFF];
    for (i, slot) in p.iter_mut().enumerate().skip(16).take(216) {
        let n = i - 16;
        *slot = triple(STEPS[n / 36], STEPS[(n / 6) % 6], STEPS[n % 6]);
    }
    for (i, slot) in p.iter_mut().enumerate().skip(232) {
        let v = u8::try_from(8 + (i - 232) * 10).unwrap_or(0xFF);
        *slot = grey(v);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::Argb;
    use baud_domain::{Chan, Pixel};

    /// **One faint mix on both sides of the crate line**: what a ReGIS
    /// page's faint phase paints in the core (`Pixel::faint_on`) is what
    /// faint text is drawn with here, for every pair of channel bytes, an
    /// opacity kept.
    #[test]
    fn a_regis_page_mixes_faint_as_text_does() {
        let pixel = |[a, r, g, b]: [u8; 4]| {
            Pixel::straight(
                Chan::from_u8(a),
                Chan::from_u8(r),
                Chan::from_u8(g),
                Chan::from_u8(b),
            )
        };
        for x in 0..=255u8 {
            for y in 0..=255u8 {
                let a = [0x80, x, y, x ^ y];
                let b = [0xFF, y, 255 - x, y];
                let text = Argb(u32::from_be_bytes(a))
                    .faint_on(Argb(u32::from_be_bytes(b)))
                    .0;
                let regis = pixel(a).faint_on(pixel(b)).bits();
                assert_eq!(text, regis, "{a:02x?} on {b:02x?}");
            }
        }
    }
}
