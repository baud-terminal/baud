//! **baud's cursor is the brand's amber**, on the CPU path, chosen against
//! the page it is drawn on: the dark theme's is `#F4A32C`, read from the
//! brand sheet's generator, and on a page it does not read on, that amber
//! with its OKLCH lightness alone stepped away from the page until it reads
//! at 4.5:1 -- WCAG 2.2's for text (1.4.3, `[[source: wcag22]]`), past
//! 1.4.11's 3:1 for a component, since the prompt's `❯` is drawn in it --
//! derived here step by step from the standard's definitions. The text
//! under the block is the brand's ground where it reads 4.5:1 on the fill,
//! else the page.
//!
//! **The block draws them**, a reversed video's and a flash's included,
//! and the trail takes the block's colour without the core being told it:
//! a theme file's `cursor none` (kitty's reverse video) takes the amber
//! away, and a file's own cursor takes the amber's text with it.

mod support;

use baud_core::{Owned, Term};
use baud_domain::themefile::{CellPaint, CursorText, kitty};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Argb, BaudCursor, Blend, CURSOR_CONTRAST, Damage, PxSize, Renderer, Stack, Target, Theme,
    VideoFlash,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 6,
    ws_row: 2,
};

/// The brand sheet's colour named `name` in `scripts/make-brand-pictures.py`
/// (`AMBER = "#F4A32C"`), as an opaque pixel.
fn brand(name: &str) -> Argb {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scripts/make-brand-pictures.py"
    );
    let text = std::fs::read_to_string(path).expect("the brand generator");
    let at = format!("\n{name} = \"#");
    let start = text.find(&at).unwrap_or_else(|| panic!("no {name} in it")) + at.len();
    let hex = &text[start..start + 6];
    Argb(0xFF00_0000 | u32::from_str_radix(hex, 16).expect("six hex digits"))
}

fn channels(c: Argb) -> [f64; 3] {
    [c.r(), c.g(), c.b()].map(|v| f64::from(v.get()) / 255.0)
}

fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(v: f64) -> f64 {
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// WCAG 2.2's relative luminance and contrast ratio, from its definitions.
fn ratio(a: Argb, b: Argb) -> f64 {
    let lum = |c: Argb| {
        let [r, g, b] = channels(c).map(decode);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Björn Ottosson's OKLab, as CSS Color 4 gives its matrices: L, C, h in
/// degrees.
fn oklch(c: Argb) -> (f64, f64, f64) {
    let [r, g, b] = channels(c).map(decode);
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    let ll = 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s;
    let a = 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s;
    let bb = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s;
    (ll, a.hypot(bb), bb.atan2(a).to_degrees())
}

/// `oklch(l c h)` in linear sRGB, unclipped.
fn linear_of(l: f64, c: f64, h: f64) -> [f64; 3] {
    let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
    let l_ = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m_ = (l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s_ = (l - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    [
        4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_,
        -1.268_438_004_6 * l_ + 2.609_757_401_1 * m_ - 0.341_319_396_5 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_614_7 * m_ + 1.707_614_701_0 * s_,
    ]
}

fn inside(rgb: [f64; 3]) -> bool {
    rgb.iter().all(|v| (-1e-9..=1.0 + 1e-9).contains(v))
}

/// **The amber at lightness `l`**: its hue, and the most chroma up to its
/// own that sRGB holds there, bisected; rounded to bytes.
fn amber_at(l: f64, c: f64, h: f64) -> Argb {
    let chroma = if inside(linear_of(l, c, h)) {
        c
    } else {
        let (mut lo, mut hi) = (0.0, c);
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            match inside(linear_of(l, mid, h)) {
                true => lo = mid,
                false => hi = mid,
            }
        }
        lo
    };
    let [r, g, b] = linear_of(l, chroma, h).map(|v| {
        let e = encode(v.clamp(0.0, 1.0));
        Chan::from_u8(u8::try_from((e * 255.0).round() as i64).expect("a byte"))
    });
    Argb::rgb(r, g, b)
}

/// **The reference derivation**, from the standard's definitions in
/// `f64` and the platform's `powf`: the brand's amber stepped by 0.0001 in
/// OKLCH lightness, darker then lighter, to the first step that reads 4.5:1
/// against `page`; the brand's ground under it where that reads 4.5:1, else
/// the page. Also the step before the one found, in its direction.
fn derived(page: Argb) -> (BaudCursor, Argb) {
    let (l0, c0, h0) = oklch(brand("AMBER"));
    for n in 0..=10_000u32 {
        for by in [-1e-4, 1e-4] {
            let l = l0 + by * f64::from(n);
            if !(0.0..=1.0).contains(&l) {
                continue;
            }
            let fill = amber_at(l, c0, h0);
            if ratio(fill, page) >= 4.5 {
                let ground = brand("GROUND");
                let text = if ratio(ground, fill) >= 4.5 {
                    ground
                } else {
                    page
                };
                let before = amber_at(l - by, c0, h0);
                return (BaudCursor { fill, text }, before);
            }
        }
    }
    panic!("no lightness reads 4.5:1 on {:08X}", page.0)
}

/// **The dark theme's cursor is the brand's amber, its text the brand's
/// ground**, each read from the generator the logo's files come from, and
/// the lockup's own SVG fills with the same amber. The amber reads past
/// 4.5:1 against the page and the ground past 4.5:1 on the amber.
#[test]
fn the_dark_cursor_is_the_brands_amber_on_its_ground() {
    let (amber, ground) = (brand("AMBER"), brand("GROUND"));
    assert_eq!(CURSOR_CONTRAST, 4.5);
    assert_eq!(
        BaudCursor::dark(),
        BaudCursor {
            fill: amber,
            text: ground
        }
    );
    let svg = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../assets/logo/baud-lockup-dark.svg"
    ))
    .expect("the lockup");
    assert!(svg.contains(&format!("fill=\"#{:06X}\"", amber.0 & 0xFF_FFFF)));
    let t = Theme::dark();
    assert_eq!(derived(t.bg).0, BaudCursor::dark(), "the reference");
    assert_eq!(t.cursor, Some(CellPaint::Colour(amber)));
    assert_eq!(t.cursor_text, Some(CellPaint::Colour(ground)));
    assert_eq!(t.cursor_text_under, CursorText::UnderACursorColour);
    let page = ratio(amber, t.bg);
    assert!(page >= 4.5, "the amber on the dark page: {page:.2}");
    let text = ratio(ground, amber);
    assert!(text >= 4.5, "the ground on the amber: {text:.2}");
    // The swap's text, a reverse-video cell's, is why the text is fixed.
    let swapped = ratio(t.fg, amber);
    assert!(
        swapped < 4.5,
        "the reversed text on the amber: {swapped:.2}"
    );
}

/// **The light theme's cursor is the amber darkened in OKLCH lightness
/// alone, the first step to read 4.5:1 against the page**, `#986100`: from
/// the brand's amber down by 0.0001 at a time, its hue held and its chroma
/// as much of its own as sRGB holds, each step rounded to bytes. The step
/// before it reads under 4.5:1 and the hue moves under a degree. The
/// brand's ground reads under 4.5:1 on it, so the text under the block is
/// the page, which reads on it as it reads on the page.
#[test]
fn the_light_cursor_is_the_amber_darkened_to_four_and_a_half_to_one() {
    let light = Theme::light();
    let (got, before) = derived(light.bg);
    assert_eq!(BaudCursor::light(), got, "{:08X}", got.fill.0);
    assert_eq!(got.fill, Argb(0xFF_98_61_00));
    assert!(ratio(before, light.bg) < 4.5, "{:08X}", before.0);
    let reads = ratio(got.fill, light.bg);
    assert!((4.5..4.53).contains(&reads), "{reads:.4}");
    let (_, _, h0) = oklch(brand("AMBER"));
    let (_, _, h) = oklch(got.fill);
    assert!((h - h0).abs() < 1.0, "the hue moved to {h:.2} from {h0:.2}");
    assert!(ratio(brand("AMBER"), light.bg) < 3.0, "the brand's own");
    assert!(ratio(brand("AMBER_LIGHT"), light.bg) < 3.0, "the lockup's");
    assert!(ratio(brand("GROUND"), got.fill) < 4.5, "the ground on it");
    assert_eq!(got.text, Argb(light.bg.0 | 0xFF00_0000), "the page");
    assert_eq!(light.cursor, Some(CellPaint::Colour(got.fill)));
    assert_eq!(light.cursor_text, Some(CellPaint::Colour(got.text)));
    let text = ratio(got.text, got.fill);
    assert!(text >= 4.5, "the page on the amber: {text:.2}");
    assert!(ratio(light.fg, got.fill) < 4.5, "the light text");
}

/// **Every page gets the reference's colour**: the two themes' pages and
/// their reversed ones, black, white, a mid grey neither end of the amber
/// wins on at once, and pages it must lighten on -- navy and a dark red
/// -- each the colour the reference derives, reading 4.5:1 against the
/// page with a text reading 4.5:1 on it.
#[test]
fn every_page_gets_the_reference_derivation() {
    let pages = [
        0x10_10_10, 0xEF_EF_EF, 0xD8_D8_D8, 0x27_27_27, 0x00_00_00, 0xFF_FF_FF, 0x77_77_77,
        0x00_00_80, 0x60_10_10, 0x1D_1F_22, 0xFD_F6_E3,
    ];
    for p in pages {
        let page = Argb(0xFF00_0000 | p);
        let got = BaudCursor::on(page);
        assert_eq!(got, derived(page).0, "{p:06X}");
        let (fill, text) = (ratio(got.fill, page), ratio(got.text, got.fill));
        assert!(
            fill >= 4.5 && text >= 4.5,
            "{p:06X}: {fill:.2} and {text:.2}"
        );
    }
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// A full block at column 0 in the default colours, so its middle pixel is
/// the colour its glyph is drawn in, painted under `theme` after `bytes`:
/// the block's cell's page and that middle pixel.
fn painted(theme: Theme, bytes: &[u8]) -> (Argb, Argb) {
    painted_in(theme, bytes, VideoFlash::Off)
}

/// [`painted`] under `flash`, the bell's other video.
fn painted_in(theme: Theme, bytes: &[u8], flash: VideoFlash) -> (Argb, Argb) {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(6),
        Rows::of_index(2),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\u{2588} \x1b[2 q\x1b[1;1H".as_bytes());
    feed(&mut t, bytes);
    r.set_flash(flash);
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    {
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let middle = px[usize::try_from((ch / 2) * w.value() + cw / 2).unwrap()];
    (Argb(r.background()[0].0 | 0xFF00_0000), Argb(middle))
}

/// **The block is drawn in the amber with the ground's glyph**, on both
/// themes; a program's `OSC 12` takes the fill and keeps the text, as a
/// theme's text is kept, and `OSC 112` gives the amber back. A program's
/// automatic colour is the swap.
#[test]
fn the_block_is_the_amber_and_osc_112_returns_to_it() {
    for (theme, b) in [
        (Theme::dark(), BaudCursor::dark()),
        (Theme::light(), BaudCursor::light()),
    ] {
        assert_eq!(painted(theme.clone(), b""), (b.fill, b.text));
        let red = Argb(0xFF_FF_00_00);
        let set = b"\x1b]12;#ff0000\x07";
        assert_eq!(painted(theme.clone(), set), (red, b.text), "OSC 12");
        let mut reset = set.to_vec();
        reset.extend_from_slice(b"\x1b]112\x07");
        assert_eq!(painted(theme.clone(), &reset), (b.fill, b.text), "OSC 112");
        let (fill, glyph) = painted(theme.clone(), b"\x1b]21;cursor=\x1b\\");
        assert_eq!(
            (fill, glyph),
            (theme.fg, Argb(theme.bg.0 | 0xFF00_0000)),
            "automatic"
        );
    }
}

/// **Under reverse video baud's cursor is chosen against the page drawn**,
/// on both themes, `?5`'s and the bell's flash alike: the block reads 4.5:1
/// against the reversed page, past 1.4.11's 3:1, and its glyph 4.5:1 on
/// the block (1.4.3), where the dark theme's amber on its reversed page
/// read 1.46:1. Back in normal video it is the theme's own.
#[test]
fn reverse_video_chooses_the_amber_against_the_page_drawn() {
    for theme in [Theme::dark(), Theme::light()] {
        let reversed = Argb(theme.fg.0 | 0xFF00_0000);
        let want = BaudCursor::on(reversed);
        for (how, bytes, flash) in [
            ("?5", &b"\x1b[?5h"[..], VideoFlash::Off),
            ("the flash", &b""[..], VideoFlash::On),
        ] {
            let (fill, glyph) = painted_in(theme.clone(), bytes, flash);
            assert_eq!((fill, glyph), (want.fill, want.text), "{how}");
            let (block, text) = (ratio(fill, reversed), ratio(glyph, fill));
            assert!(block >= 3.0, "{how}: the block on the page, {block:.2}");
            assert!(text >= 4.5, "{how}: the glyph on the block, {text:.2}");
        }
        let normal = painted_in(theme.clone(), b"\x1b[?5h\x1b[?5l", VideoFlash::Off);
        let own = BaudCursor::on(theme.bg);
        assert_eq!(normal, (own.fill, own.text), "back to normal video");
        let (fill, _) = painted_in(
            theme.clone(),
            b"\x1b[?5h\x1b]12;#ff0000\x07",
            VideoFlash::Off,
        );
        assert_eq!(fill, Argb(0xFF_FF_00_00), "a program's colour is its own");
    }
}

/// **A kitty theme's `cursor none` is the swap, and a file's own cursor
/// takes the amber's text with it**, where `[palette] cursor = "theme"`
/// asks for the file's: kitty draws `cursor none` as reverse video, and a
/// file that names a cursor colour and no text leaves the cell's colours
/// swapped under it, as it did before the amber. A file that names
/// neither keeps baud's, chosen against the file's page.
#[test]
fn a_theme_files_cursor_takes_the_amber_and_its_text_away() {
    let themed = |text: &[u8]| {
        let mut t = Theme::dark();
        t.take_scheme(&kitty(text, |_, _, _| {}).expect("the file reads"));
        t
    };
    let none = themed(b"cursor none\n");
    assert_eq!((none.cursor, none.cursor_text), (None, None));
    let (fill, glyph) = painted(none.clone(), b"");
    assert_eq!((fill, glyph), (none.fg, Argb(none.bg.0 | 0xFF00_0000)));
    let own = themed(b"cursor #ff8800\n");
    let orange = Argb(0xFF_FF_88_00);
    assert_eq!(
        (own.cursor, own.cursor_text),
        (Some(CellPaint::Colour(orange)), None)
    );
    let plain = themed(b"background #000000\n");
    assert_eq!(
        plain.cursor,
        Some(CellPaint::Colour(BaudCursor::dark().fill))
    );
    assert_eq!(
        plain.cursor_text,
        Some(CellPaint::Colour(BaudCursor::dark().text))
    );
}

/// **The trail is the block's colour without the core being told it**:
/// the ghost drawn over empty cells, where the core has no configured
/// cursor colour, is the amber, on both themes, and under reverse video
/// the amber chosen against the reversed page, as the block's is.
#[test]
fn the_trail_takes_the_amber_without_the_core() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let ws = WinSize {
        ws_col: 30,
        ws_row: 6,
    };
    let reversed = |t: &Theme| Argb(t.fg.0 | 0xFF00_0000);
    for (theme, video, b, page) in [
        (
            Theme::dark(),
            &b""[..],
            BaudCursor::dark(),
            Theme::dark().bg,
        ),
        (
            Theme::light(),
            &b""[..],
            BaudCursor::light(),
            Theme::light().bg,
        ),
        (
            Theme::dark(),
            &b"\x1b[?5h"[..],
            BaudCursor::on(reversed(&Theme::dark())),
            reversed(&Theme::dark()),
        ),
        (
            Theme::light(),
            &b"\x1b[?5h"[..],
            BaudCursor::on(reversed(&Theme::light())),
            reversed(&Theme::light()),
        ),
    ] {
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            theme.clone(),
            Blend::Native,
            Cols::of_index(30),
            Rows::of_index(6),
        )
        .unwrap();
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, video);
        feed(&mut t, b"\x1b[6;1H");
        let (w, h) = r.pixel_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let mut bare = vec![0u32; n];
        {
            let mut target = Target::new(&mut bare, w, w, h).unwrap();
            r.render(&mut t, &mut target, Damage::Full);
        }
        r.set_trail(Some(baud_render::TrailLook::Ghost(baud_render::Trail {
            from: (
                baud_domain::Row::from_index(2),
                baud_domain::Col::at_index(2),
            ),
            to: (
                baud_domain::Row::from_index(2),
                baud_domain::Col::at_index(22),
            ),
            progress: 0.5,
            origin_dy: 0.0,
            lift: baud_font::Px::of(0),
            strength: baud_render::Strength::Full,
        })));
        let mut ghosted = vec![0u32; n];
        {
            let mut target = Target::new(&mut ghosted, w, w, h).unwrap();
            r.render(&mut t, &mut target, Damage::Full);
        }
        // The pixel the ghost moved furthest from the page, which a ghost
        // over the page at its own alpha moves toward its colour.
        let page = channels(page);
        let far = bare
            .iter()
            .zip(&ghosted)
            .filter(|(a, g)| a != g)
            .map(|(_, &g)| Argb(g | 0xFF00_0000))
            .max_by(|x, y| {
                let d = |c: Argb| {
                    channels(c)
                        .iter()
                        .zip(page)
                        .map(|(v, p)| (v - p).abs())
                        .sum::<f64>()
                };
                d(*x).total_cmp(&d(*y))
            })
            .expect("the ghost drew nothing");
        // It is the amber over the page: each channel the same share of
        // the way from the page to the amber, which the text, a grey, is
        // not.
        let [ra, ga, ba] = channels(b.fill);
        let share = channels(far)
            .iter()
            .zip(page)
            .zip([ra, ga, ba])
            .map(|((v, p), a)| (v - p) / (a - p))
            .collect::<Vec<f64>>();
        let (lo, hi) = share
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), &k| (l.min(k), h.max(k)));
        assert!(
            lo > 0.2 && hi - lo < 0.06,
            "{:08X} is not {:08X} over the page: {share:?}",
            far.0,
            b.fill.0
        );
    }
}
