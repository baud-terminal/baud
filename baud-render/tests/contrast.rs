//! Text contrast on the CPU and instance paths: every outline is
//! rasterised once, as exact coverage, and its coverage is bent by
//! DirectWrite's grayscale curve for the colour it is finally drawn in
//! (`baud_domain::contrast`). The curve itself is held to its formula by
//! `baud-domain/tests/contrast.rs`; what is held here is the renderer's
//! half: which row a glyph takes, from which colour, and that the
//! compositor applies it.

mod support;

use baud_core::{Owned, Term};
use baud_domain::contrast::{self, CurveRow};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Argb, Blend, BlinkPhases, BlockCursor, Curves, Damage, Ink, Instance, Kind, Pictured, PxSize,
    Renderer, Resolved, Selected, Shapes, Stack, Swaps, Theme,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 24,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn renderer<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(16.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// One full frame of `bytes`: its pixels and the glyph instances.
fn frame(bytes: &[u8]) -> (Vec<u32>, usize, Vec<Instance>) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    let glyphs = r
        .instances()
        .filter(|i| i.kind == Kind::Mask)
        .copied()
        .collect();
    (canvas.pixels, canvas.width, glyphs)
}

fn rgb(c: Argb) -> (u8, u8, u8) {
    (c.r().get(), c.g().get(), c.b().get())
}

/// White, then black on white, then a mid green: three colours, three
/// rows, the same letter in each, the cursor hidden.
const THREE: &[u8] = b"\x1b[?25l\x1b[38;2;255;255;255mH\x1b[38;2;0;0;0;48;2;255;255;255mH\
\x1b[0m\x1b[38;2;0;205;0mH";

/// **One raster a glyph, one row a colour.** The three letters sample one
/// atlas slot whatever their colour -- there is no per-colour raster to
/// keep -- and each carries its own colour's curve row.
#[test]
fn each_colour_takes_its_own_row_over_one_raster() {
    let (_, _, glyphs) = frame(THREE);
    assert_eq!(glyphs.len(), 3);
    let rect = |i: &Instance| (i.u.0, i.v.0, i.w.0, i.h.0);
    assert!(
        glyphs.iter().all(|i| rect(i) == rect(&glyphs[0])),
        "one slot"
    );
    for g in &glyphs {
        let (r, gr, b) = rgb(g.color);
        assert_eq!(g.curve, CurveRow::of(r, gr, b), "{:08x}", g.color.0);
    }
    assert_ne!(
        glyphs[0].curve, glyphs[1].curve,
        "white and black, two rows"
    );
    assert_ne!(
        glyphs[1].curve, glyphs[2].curve,
        "black and green, two rows"
    );
}

/// **The row is the colour finally drawn's**: reverse video draws the
/// letter in the background's colour, so reversed white on black takes
/// black's row.
#[test]
fn the_row_follows_the_drawn_colour() {
    let (_, _, normal) = frame(b"\x1b[?25l\x1b[38;2;255;255;255;48;2;0;0;0mH");
    let (_, _, reversed) = frame(b"\x1b[?25l\x1b[7;38;2;255;255;255;48;2;0;0;0mH");
    assert_eq!(rgb(reversed[0].color), (0, 0, 0));
    assert_eq!(reversed[0].curve, CurveRow::of(0, 0, 0));
    assert_ne!(reversed[0].curve, normal[0].curve);
}

/// **The CPU compositor bends every mask by its row**, held to the curve's
/// formula -- not to the table the renderer reads -- composited here over
/// the frame of the same cells with spaces: every glyph pixel within two
/// steps. The control is the same composite with no curve, which must
/// miss, so a compositor that skipped the curve cannot pass.
#[test]
fn the_cpu_path_bends_every_mask_by_the_curve() {
    let lines = [
        "\x1b[38;2;0;0;0;48;2;255;255;255m",
        "\x1b[38;2;255;255;255;48;2;0;0;0m",
        "\x1b[38;2;128;128;128;48;2;20;24;28m",
        "\x1b[38;2;30;90;200;48;2;250;240;220m",
    ];
    let text = "Hamburgefonstiv 0123 @#";
    let paint = |body: &str| {
        let mut bytes = String::from("\x1b[?25l");
        for (i, sgr) in lines.iter().enumerate() {
            bytes.push_str(&format!("\x1b[{};1H{sgr}{body}\x1b[0m", i + 1));
        }
        bytes
    };
    let (plain, _, none) = frame(paint(&" ".repeat(text.len())).as_bytes());
    assert!(none.is_empty(), "the plain frame draws no glyph");
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, paint(text).as_bytes());
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    let side = usize::try_from(r.atlas().side().value()).unwrap();
    let mask = r.atlas().pixels();
    let w = canvas.width;
    let glyphs: Vec<Instance> = r
        .instances()
        .filter(|i| i.kind == Kind::Mask)
        .copied()
        .collect();
    assert!(glyphs.len() > 60, "the scene drew its glyphs");
    let reference = |curved: bool| {
        let mut out = plain.clone();
        for g in &glyphs {
            let (cr, cg, cb) = rgb(g.color);
            for y in 0..usize::from(g.h.0) {
                for x in 0..usize::from(g.w.0) {
                    let cov = mask[(usize::from(g.v.0) + y) * side + usize::from(g.u.0) + x];
                    let a = f64::from(cov) / 255.0;
                    let a = if curved {
                        contrast::exact_for(a, cr, cg, cb)
                    } else {
                        a
                    };
                    let a = a * f64::from(g.color.a().get()) / 255.0;
                    let at = (usize::try_from(i32::from(g.y.0)).unwrap() + y) * w
                        + usize::try_from(i32::from(g.x.0)).unwrap()
                        + x;
                    let d = out[at];
                    let mix = |s: u8, k: u32| {
                        let d = f64::from((d >> k) & 0xff);
                        let v = (f64::from(s) * a + d * (1.0 - a)).round();
                        u32::try_from(v as i64).unwrap() << k
                    };
                    out[at] = (d & 0xff00_0000) | mix(cr, 16) | mix(cg, 8) | mix(cb, 0);
                }
            }
        }
        out
    };
    let off_by = |want: &[u32]| {
        canvas
            .pixels
            .iter()
            .zip(want)
            .filter(|&(&g, &r)| {
                (0..3).any(|k| ((g >> (8 * k)) & 0xff).abs_diff((r >> (8 * k)) & 0xff) > 2)
            })
            .count()
    };
    assert_eq!(
        off_by(&reference(true)),
        0,
        "the CPU path follows the curve"
    );
    assert!(
        off_by(&reference(false)) > 200,
        "the curve reaches the pixels: an uncurved reference must miss"
    );
}

/// **The half of the slot the cell path reads is the drawn foreground's
/// row.** The shader picks a slot's own row or its swapped one by the
/// swaps' parity (`Swaps::ink`); the CPU path takes the row of the colour
/// `Resolved::drawn` hands back. Over every swap, an `OSC 12` fill and a
/// picture, the two must name one row.
#[test]
fn the_ink_is_the_drawn_foregrounds_row() {
    let (fg, bg) = (Argb(0xFF_FF_FF_FF), Argb(0x38_10_10_10));
    let row = |c: Argb| {
        let (r, g, b) = rgb(c);
        CurveRow::of(r, g, b)
    };
    let slot = Resolved {
        fg,
        bg,
        underline: fg,
        shapes: Shapes::default(),
        curves: Curves::of(row(fg), row(bg)),
    };
    assert_ne!(row(fg), row(bg), "the control: two rows");
    let mut swapped = 0;
    for selected in [Selected::No, Selected::Yes] {
        for block in [
            BlockCursor::No,
            BlockCursor::Yes(None),
            BlockCursor::Yes(Some(Argb(0xFF_FF_00_00))),
        ] {
            for pictured in [Pictured::No, Pictured::Yes] {
                let s = Swaps {
                    pictured,
                    selected,
                    block,
                    blink: BlinkPhases::SHOWN,
                    cursor: Chan::from_u8(0xFF),
                };
                let drawn = slot.drawn(s);
                assert_eq!(slot.curves.row(s.ink()), row(drawn.fg), "{s:?}");
                assert_eq!(drawn.curve, row(drawn.fg), "{s:?}");
                swapped += usize::from(s.ink() == Ink::Swapped);
                // **A block part way through a fade takes the row of the side
                // it is nearer**: the whole block's from half way, the cell's
                // without it below -- the shader's `swapped()` at 128.
                for opacity in [254u8, 200, 128, 127, 40, 0] {
                    let f = Swaps {
                        cursor: Chan::from_u8(opacity),
                        ..s
                    };
                    let side = if opacity >= 128 {
                        s
                    } else {
                        Swaps {
                            block: BlockCursor::No,
                            ..s
                        }
                    };
                    let want = row(slot.drawn(side).fg);
                    assert_eq!(slot.drawn(f).curve, want, "{f:?}");
                    assert_eq!(slot.curves.row(f.ink()), want, "{f:?}");
                }
            }
        }
    }
    assert_eq!(swapped, 6, "the odd swaps: selected or block, not both");
}
