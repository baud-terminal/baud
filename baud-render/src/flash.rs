//! **A trail's repeat passes, drawn under the flash thresholds** (note AL,
//! option O2; the census is the record's "The cursor trails' flashes,
//! counted on the frames they draw").
//!
//! Above a size gate the driver marks a pass over a stretch of screen that
//! a full pass swept less than a second before [`Strength::UnderThreshold`],
//! and the renderer draws it with no record carrying more alpha than
//! [`under_threshold`] of the cursor's colour over the ground. Every
//! fragment multiplies its alpha by its record's and draws the record's
//! colour, so where one record covers a pixel, no alpha it can draw there
//! makes a WCAG 2.3.1 transition from the ground. **One record a pixel is
//! what a repeat pass keeps** (Jev, decisions 720/721, 0.98 and 0.97): its
//! bands, one a cell row and disjoint, and not its particles, which stack
//! over the bands and over each other, and whose alphas compound under
//! source-over past any one record's cap. **The ground is every colour the
//! cells under the pass draw** -- their backgrounds, and over text every
//! mixture of the text over its background a glyph's edge can draw
//! ([`under_threshold_over`]) -- Jev's lean below the bar (722/723, 0.83 and
//! 0.77), taken on the owner's word of 2026-10-17; a frame scans two new
//! caps, and over more than 64 grounds, or one not yet scanned, the pass
//! draws nothing (Jev, 728/729, 0.98 and 1.00), and so does one whose
//! records meet an image placed under the text or below the backgrounds,
//! whose pixels the cells do not name. **A selection and the cursor under
//! the pass are grounds too**, by the colours they draw, and **a dimmed page
//! is read over the desktop**, a flat white and a flat black, note AS's
//! bound; a coloured desktop is not read.
//!
//! The thresholds are WCAG 2.2's definition of the general and red flash
//! thresholds ([[source: wcag22]]), as `baud_bench::flash` counts them: a
//! general transition is a change in relative luminance of 0.10 or more
//! where the darker state is below 0.80; a red transition is a u'v'
//! distance over 0.2 where either state is a saturated red, R/(R+G+B) >= 0.8
//! on the linear values (the Understanding page's working definition,
//! informative). On a ground that is itself a saturated red the bound is
//! halved to 0.1, so any two states a repeat pass draws are within 0.2 of
//! each other by the triangle inequality.

use crate::color::Argb;
use baud_domain::Chan;

/// **How a trail's frame is drawn**: as the trail draws it, or with each
/// record's alpha scaled under the renderer's [`under_threshold`] and only
/// the records that cannot stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Strength {
    Full,
    UnderThreshold,
}

/// **A length in points**: device pixels over the backing scale, the unit a
/// size gate is set in so it reads the same at 1x and 2x.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub struct Points(pub f32);

/// A general flash's change: 10% of the maximum relative luminance.
const CHANGE: f64 = 0.10;

/// The darker image of a general flash is below this.
const DARKER: f64 = 0.80;

/// R/(R+G+B) at or above this, on the linear values, is a saturated red.
const RED: f64 = 0.8;

/// A red transition's u'v' distance is more than this.
const STEP: f64 = 0.2;

/// Which of the two transitions a scan looks for.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Rule {
    /// A change in relative luminance of [`CHANGE`] or more, the darker
    /// state below [`DARKER`].
    General,
    /// A u'v' distance over the bound, either state a saturated red.
    Red(f64),
}

/// Whether a state is a saturated red.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Redness {
    Red,
    Not,
}

/// A colour as the definitions read it.
#[derive(Clone, Copy, Debug)]
struct Seen {
    lum: f64,
    u: f64,
    v: f64,
    red: Redness,
}

/// WCAG's curve: an encoded channel to its linear value, a byte at a time
/// from a table built once.
fn lin(c: Chan) -> f64 {
    static TABLE: std::sync::OnceLock<[f64; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        core::array::from_fn(|i| {
            let c = f64::from(u8::try_from(i).unwrap_or(u8::MAX)) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
    });
    table[usize::from(c.get())]
}

/// u'v' from linear R, G and B by the Understanding page's matrix; black,
/// which has none, takes the white point's, the u'v' of R = G = B.
fn chromaticity(r: f64, g: f64, b: f64) -> (f64, f64) {
    let x = 0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = 0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b;
    let d = x + 15.0 * y + 3.0 * z;
    if d > 0.0 {
        (4.0 * x / d, 9.0 * y / d)
    } else {
        chromaticity(1.0, 1.0, 1.0)
    }
}

fn seen(rgb: [Chan; 3]) -> Seen {
    let [r, g, b] = rgb.map(lin);
    let (u, v) = chromaticity(r, g, b);
    let sum = r + g + b;
    Seen {
        lum: 0.2126 * r + 0.7152 * g + 0.0722 * b,
        u,
        v,
        red: if sum > 0.0 && r / sum >= RED {
            Redness::Red
        } else {
            Redness::Not
        },
    }
}

/// Whether a state `c` is a transition of `rule` from the ground `g`.
fn transition(c: &Seen, g: &Seen, rule: Rule) -> bool {
    match rule {
        Rule::General => (c.lum - g.lum).abs() >= CHANGE && c.lum.min(g.lum) < DARKER,
        Rule::Red(bound) => {
            (c.red == Redness::Red || g.red == Redness::Red) && (c.u - g.u).hypot(c.v - g.v) > bound
        }
    }
}

/// **The frame's blend**: the ink premultiplied at alpha `a` over the opaque
/// ground, in encoded space, rounded to a byte -- a `BGRA8Unorm` target's,
/// and `over` in `baud-present-macos/tests/cells.rs`.
fn over(ink: [Chan; 3], ground: [Chan; 3], a: f32) -> [Chan; 3] {
    let mix = |c: Chan, d: Chan| {
        let s = f32::from(c.get()) / 255.0 * a;
        let d = f32::from(d.get()) / 255.0;
        let v = ((s + d * (1.0 - a)) * 255.0).round().clamp(0.0, 255.0);
        Chan::from_u8(u8::try_from(v as i64).unwrap_or(u8::MAX))
    };
    [
        mix(ink[0], ground[0]),
        mix(ink[1], ground[1]),
        mix(ink[2], ground[2]),
    ]
}

fn rgb(c: Argb) -> [Chan; 3] {
    [c.r(), c.g(), c.b()]
}

/// The alphas at which some channel of `ink` over `ground`, rounded to a
/// byte, steps: between two of them the composite does not change.
fn cuts(ink: [Chan; 3], ground: [Chan; 3]) -> Vec<f64> {
    let mut cuts: Vec<f64> = vec![0.0, 1.0];
    for (&c, &d) in ink.iter().zip(&ground) {
        let (c, d) = (c.get(), d.get());
        let (lo, hi) = (c.min(d), c.max(d));
        let span = f64::from(c) - f64::from(d);
        for n in lo..hi {
            let a = (f64::from(n) + 0.5 - f64::from(d)) / span;
            if a > 0.0 && a < 1.0 {
                cuts.push(a);
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    cuts
}

/// **The least alpha at which `ink` over `ground` makes a transition of
/// `rule`**, exactly over the continuous alphas a fragment can produce: as
/// the alpha rises each channel crosses each half-integer once, and between
/// those crossings the rounded composite does not change, so one composite
/// an interval is every composite there is. The answer is the start of the
/// first interval whose composite is a transition; `None` if none is.
fn first_flash(ink: [Chan; 3], ground: [Chan; 3], rule: Rule) -> Option<f64> {
    let g = seen(ground);
    cuts(ink, ground).windows(2).find_map(|w| {
        let mid = (w[0] + w[1]) * 0.5;
        let c = seen(over(ink, ground, mid as f32));
        transition(&c, &g, rule).then_some(w[0])
    })
}

/// **The most alpha a repeat pass's record may carry**: the largest byte
/// strictly below the least alpha at which `ink` over `ground` makes a
/// general or a red transition, less one step of 255; every alpha under it
/// makes neither. 255 when no alpha does.
///
/// It scans every interval rather than bisecting, because neither predicate
/// is monotone in the alpha -- the redness test flips, and the byte rounding
/// moves the u'v' distance in steps -- and the record's alpha is only a
/// cap: a fragment draws every alpha under it, so all of them must be safe,
/// not only the cap. The ground's redness halves the red bound. Computed
/// when the ink or the ground changes, never a frame.
pub fn under_threshold(ink: Argb, ground: Argb) -> Chan {
    let (i, g) = (rgb(ink), rgb(ground));
    let bound = match seen(g).red {
        Redness::Red => STEP / 2.0,
        Redness::Not => STEP,
    };
    let first = [
        first_flash(i, g, Rule::General),
        first_flash(i, g, Rule::Red(bound)),
    ]
    .into_iter()
    .flatten()
    .reduce(f64::min);
    match first {
        None => Chan::from_u8(0xFF),
        Some(a) => {
            let below = ((a * 255.0).ceil() - 2.0).max(0.0);
            Chan::from_u8(u8::try_from(below as i64).unwrap_or(0))
        }
    }
}

/// **[`under_threshold`] over every colour a glyph of `text` over `page`
/// draws**: an anti-aliased edge is the text at some coverage over the
/// page, rounded to a byte, and those mixtures are the grounds a trail
/// passes over -- not only their two ends, since the luminance curve can
/// move a mid-tone across the 0.10 threshold at an alpha where neither end
/// moves. The least cap over each distinct mixture, found by the same exact
/// intervals.
pub fn under_threshold_over(ink: Argb, text: Argb, page: Argb) -> Chan {
    let (t, p) = (rgb(text), rgb(page));
    let mut k = Chan::from_u8(0xFF);
    let mut last = None;
    for w in cuts(t, p).windows(2) {
        let mix = over(t, p, ((w[0] + w[1]) * 0.5) as f32);
        if last == Some(mix) {
            continue;
        }
        last = Some(mix);
        let c = under_threshold(ink, Argb::rgb(mix[0], mix[1], mix[2]));
        if c.get() < k.get() {
            k = c;
        }
    }
    k
}

/// A record's alpha `a` scaled by `k` of 255, rounded: never more than `k`.
pub fn capped(a: Chan, k: Chan) -> Chan {
    let v = (u16::from(a.get()) * u16::from(k.get()) + 127) / 255;
    Chan::from_u8(u8::try_from(v).unwrap_or(u8::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The census's grounds and inks (`baud-bench/benches/flash.rs`): the
    /// theme's default foreground is `#272727` on the light page and
    /// `#D8D8D8` on every other ground.
    fn pair(ground: &str, ink: &str) -> ([Chan; 3], [Chan; 3]) {
        let g = match ground {
            "dark" => [0x10, 0x10, 0x10],
            "light" => [0xEF, 0xEF, 0xEF],
            "red" => [0xCD, 0, 0],
            _ => [0x1A, 0, 0],
        };
        let i = match (ink, ground) {
            ("default", "light") => [0x27, 0x27, 0x27],
            ("default", _) => [0xD8, 0xD8, 0xD8],
            ("white", _) => [0xFF, 0xFF, 0xFF],
            ("black", _) => [0, 0, 0],
            _ => [0xFF, 0, 0],
        };
        (i.map(Chan::from_u8), g.map(Chan::from_u8))
    }

    fn argb(c: [Chan; 3]) -> Argb {
        Argb::rgb(c[0], c[1], c[2])
    }

    fn bound(ground: [Chan; 3]) -> f64 {
        match seen(ground).red {
            Redness::Red => STEP / 2.0,
            Redness::Not => STEP,
        }
    }

    /// Note AL's threshold table ([[source: al-obs-thresholds]]): the least
    /// alpha, to 1/2000, for a general and for a red transition (bound 0.2
    /// on every ground), per ground and ink; `None` is "never".
    const TABLE: [(&str, &str, Option<f64>, Option<f64>); 16] = [
        ("dark", "default", Some(0.3775), None),
        ("dark", "white", Some(0.3160), None),
        ("dark", "black", None, None),
        ("dark", "red", Some(0.7095), Some(0.2450)),
        ("light", "default", Some(0.0625), None),
        ("light", "white", None, None),
        ("light", "black", Some(0.0525), None),
        ("light", "red", Some(0.0695), Some(0.7345)),
        ("red", "default", Some(0.4470), Some(0.6460)),
        ("red", "white", Some(0.3395), Some(0.6060)),
        ("red", "black", Some(0.4905), Some(0.9980)),
        ("red", "red", None, None),
        ("darkred", "default", Some(0.4005), Some(0.1505)),
        ("darkred", "white", Some(0.3385), Some(0.1275)),
        ("darkred", "black", None, Some(0.9810)),
        ("darkred", "red", Some(0.6925), None),
    ];

    /// **The scan reproduces the threshold table within one step** for all
    /// sixteen ink and ground pairs, both transitions, at the table's bound:
    /// the table's least alphas, sampled at 1/2000 over a float alpha, are
    /// where this scan's exact intervals put them.
    #[test]
    fn the_scan_reproduces_the_threshold_table() {
        for (ground, ink, general, red) in TABLE {
            let (i, g) = pair(ground, ink);
            let found = [
                first_flash(i, g, Rule::General),
                first_flash(i, g, Rule::Red(STEP)),
            ];
            for ((want, got), what) in [general, red]
                .into_iter()
                .zip(found)
                .zip(["general", "red"])
            {
                match (want, got) {
                    (None, None) => {}
                    (Some(w), Some(a)) => assert!(
                        (w - a).abs() <= 1.0 / 255.0,
                        "{ink} on {ground}, {what}: {a} against the table's {w}"
                    ),
                    _ => panic!("{ink} on {ground}, {what}: {got:?} against the table's {want:?}"),
                }
            }
        }
    }

    /// **`under_threshold` per pair, pinned**: note AL's caps (section 4.2,
    /// [[source: al-obs-o2-astar-tool]]), the red bound halved on the red
    /// and dark-red grounds -- which is why those rows sit under the table's
    /// red column.
    #[test]
    fn under_threshold_is_pinned_for_every_pair() {
        let want: [(&str, &str, u8); 16] = [
            ("dark", "default", 95),
            ("dark", "white", 79),
            ("dark", "black", 255),
            ("dark", "red", 61),
            ("light", "default", 14),
            ("light", "white", 255),
            ("light", "black", 12),
            ("light", "red", 16),
            ("red", "default", 85),
            ("red", "white", 77),
            ("red", "black", 124),
            ("red", "red", 255),
            ("darkred", "default", 4),
            ("darkred", "white", 3),
            ("darkred", "black", 249),
            ("darkred", "red", 175),
        ];
        for (ground, ink, k) in want {
            let (i, g) = pair(ground, ink);
            assert_eq!(
                under_threshold(argb(i), argb(g)).get(),
                k,
                "{ink} on {ground}"
            );
        }
    }

    /// **No alpha at or under the cap makes a transition**, at a sixteenth
    /// of a byte, for every pair: the property the cap is for, by brute force
    /// rather than by the intervals that derive it. And the cap is tight:
    /// two steps past it reaches the first flashing alpha, where there is
    /// one.
    #[test]
    fn nothing_under_the_cap_flashes() {
        for ground in ["dark", "light", "red", "darkred"] {
            for ink in ["default", "white", "black", "red"] {
                let (i, g) = pair(ground, ink);
                let k = under_threshold(argb(i), argb(g));
                let (gs, red) = (seen(g), Rule::Red(bound(g)));
                for j in 0..=(u16::from(k.get()) * 16) {
                    let a = f32::from(j) / (255.0 * 16.0);
                    let c = seen(over(i, g, a));
                    assert!(
                        !transition(&c, &gs, Rule::General) && !transition(&c, &gs, red),
                        "{ink} on {ground}: alpha {a} under the cap {} flashes",
                        k.get()
                    );
                }
                let first = [first_flash(i, g, Rule::General), first_flash(i, g, red)]
                    .into_iter()
                    .flatten()
                    .reduce(f64::min);
                if let Some(a) = first {
                    assert!(
                        (f64::from(k.get()) + 2.0) / 255.0 >= a,
                        "{ink} on {ground}: the cap {} is more than two steps under {a}",
                        k.get()
                    );
                }
            }
        }
    }

    /// **A glyph's edge flashes where neither end does**: the theme's text
    /// `#D8D8D8` over its own colour never changes and over the dark page
    /// takes 95, but an edge pixel between them -- the text at a partial
    /// coverage -- crosses the 0.10 threshold at a lower alpha, because the
    /// curve is steeper there. So the cap over text is taken over every
    /// mixture, and no alpha under it flashes over any of them, at a
    /// sixteenth of a byte.
    #[test]
    fn a_glyph_edge_flashes_where_neither_end_does() {
        let (ink, page) = pair("dark", "default");
        let (i, p) = (argb(ink), argb(page));
        let ends = under_threshold(i, p).get().min(under_threshold(i, i).get());
        assert_eq!(ends, 95, "the control: the ends' least cap");
        let k = under_threshold_over(i, i, p);
        assert!(
            k.get() < ends,
            "a mixture's cap {} is under the ends' {ends}",
            k.get()
        );
        for w in cuts(ink, page).windows(2) {
            let mix = over(ink, page, ((w[0] + w[1]) * 0.5) as f32);
            let (gs, red) = (seen(mix), Rule::Red(bound(mix)));
            for j in 0..=(u16::from(k.get()) * 16) {
                let c = seen(over(ink, mix, f32::from(j) / (255.0 * 16.0)));
                assert!(
                    !transition(&c, &gs, Rule::General) && !transition(&c, &gs, red),
                    "over {mix:?}: alpha {j}/4080 under the cap {} flashes",
                    k.get()
                );
            }
        }
    }

    /// **A capped record never carries more alpha than the cap**, for every
    /// alpha and every cap; and a cap of 255 changes nothing.
    #[test]
    fn a_capped_alpha_never_exceeds_the_cap() {
        for k in 0..=255u8 {
            for a in 0..=255u8 {
                let c = capped(Chan::from_u8(a), Chan::from_u8(k)).get();
                assert!(c <= k, "{a} capped by {k} is {c}");
            }
        }
        for a in 0..=255u8 {
            assert_eq!(capped(Chan::from_u8(a), Chan::from_u8(255)).get(), a);
        }
    }
}
