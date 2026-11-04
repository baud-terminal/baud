//! **Every background carries the window's dim, not only the default
//! one**, and a cursor or selection hands back an *opaque* foreground. The
//! one page raised above it is a reversed default one, `SGR 7` in the
//! default colours, to the lowest alpha at which its text reads
//! (`Theme::reversed_alpha`; `reverse_raise.rs` is its gate).
//!
//! Nothing else in the workspace renders at a translucent `bg_alpha`:
//! every other gate uses `Theme::dark()`, whose alpha is `0xFF`, where
//! `with_alpha(0xFF)` is a no-op and this whole path is invisible. So
//! these are the only tests that can fail when the rule changes.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{AppBg, Argb, Blend, Damage, Kind, PictureBg, PxSize, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 12,
};

/// The old default dim, 0.22 of 255 (alpha 56; the default is 0.70 since
/// 2026-10-17).
const DIM: u8 = 56;

/// What a reversed default page is raised to on the dark theme over the
/// glass: the lowest alpha at which its text reads at 4.5:1 over black to
/// white, above [`DIM`].
const RAISED: u8 = 146;

fn alpha(c: Argb) -> u8 {
    c.0.to_be_bytes()[0]
}

fn dim_theme() -> Theme {
    let mut t = Theme::dark();
    t.bg_alpha = Chan::from_u8(DIM);
    t
}

fn render(bytes: &[u8], f: impl FnOnce(&Renderer<'_, '_>)) {
    render_with(dim_theme(), bytes, f);
}

fn render_with(theme: Theme, bytes: &[u8], f: impl FnOnce(&Renderer<'_, '_>)) {
    render_ws(theme, &WS, bytes, f);
}

fn render_ws(theme: Theme, ws: &WinSize, bytes: &[u8], f: impl FnOnce(&Renderer<'_, '_>)) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    f(&r);
}

/// A default background, a background the application set, and a reverse
/// rendition with a colour of its own -- all three at the window's alpha --
/// and a reversed default one, raised until its text reads. Under the rule
/// this replaced only the first was translucent, which is why btop and
/// asciiquarium were opaque where a shell was not.
#[test]
fn every_background_carries_the_windows_dim() {
    // `A` default, `B` on red, `C` reversed green, `D` reversed, then the
    // cursor parked off them so it cannot opaque one of them.
    render(b"A\x1b[41mB\x1b[0;7;32mC\x1b[0;7mD\x1b[0m\x1b[2;12H", |r| {
        let bg = r.background();
        assert_eq!(alpha(bg[3]), RAISED, "a reversed default page is raised");
        for (i, name) in ["default", "SGR 41", "SGR 7 on green"].iter().enumerate() {
            assert_eq!(
                alpha(bg[i]),
                DIM,
                "cell {i} ({name}) is {:#04x}, not the window's dim",
                alpha(bg[i])
            );
        }
        // And they are three *different* colours, so the assertion above
        // is about three renditions rather than one repeated.
        assert_ne!(bg[0].0 & 0x00FF_FFFF, bg[1].0 & 0x00FF_FFFF);
        assert_ne!(bg[1].0 & 0x00FF_FFFF, bg[2].0 & 0x00FF_FFFF);
    });
}

/// The block cursor is opaque on **both** sides of its swap. `bg` becomes
/// the old foreground and is the block a reader sees; `fg` becomes the old
/// background, which now carries the dim -- and a glyph drawn at 0.22 over
/// its own cursor block is barely there.
#[test]
fn a_cursor_is_opaque_on_both_sides_of_its_swap() {
    // One glyph, with the cursor homed back onto it, so every mask
    // instance in the frame is the cursor's own glyph.
    render(b"A\x1b[H", |r| {
        assert_eq!(alpha(r.background()[0]), 0xFF, "the cursor block is dimmed");
        let masks: Vec<_> = r
            .instances()
            .filter(|i| matches!(i.kind, Kind::Mask))
            .collect();
        assert!(!masks.is_empty(), "no glyph drawn: the test proves nothing");
        for m in masks {
            assert_eq!(
                alpha(m.color),
                0xFF,
                "a glyph under the cursor is drawn at the background's alpha"
            );
        }
    });
}

/// The margin keeps the same alpha, so the gutter and the cells are one
/// surface rather than a see-through edge around an opaque grid.
#[test]
fn the_margin_is_the_same_alpha_as_a_cell() {
    render(b"A", |r| {
        assert_eq!(alpha(r.clear_color()), DIM);
    });
}

/// The six cells `[glass] app_backgrounds` decides between: a space, a
/// box-drawing stroke and a block element on a set background -- the three
/// kinds of cell chafa draws an image with -- the same block under `SGR 7`
/// with a set foreground, so the drawn background is the set one; then a
/// letter on the default background, and a reversed letter whose drawn
/// background is the default foreground. Cursor parked off them.
const RULE_ROW: &[u8] = "\x1b[44m \x1b[31;44m\u{2500}\x1b[31;44m\u{2580}\x1b[0;7;32m\u{2584}\
     \x1b[0mA\x1b[7mB\x1b[0m\x1b[2;12H"
    .as_bytes();

const RULE_NAMES: [&str; 6] = [
    "space on blue",
    "box-drawing on blue",
    "half-block on blue",
    "reversed half-block, green foreground",
    "letter on default",
    "reversed letter, default foreground",
];

/// **The default draws every background under the dim** -- which the
/// key must not change unless it is asked for -- and the reversed default
/// page raised above it.
#[test]
fn app_backgrounds_are_dimmed_by_default() {
    render(RULE_ROW, |r| {
        let bg = r.background();
        let want = [DIM, DIM, DIM, DIM, DIM, RAISED];
        for (i, c) in bg[..6].iter().enumerate() {
            assert_eq!(alpha(*c), want[i], "cell {i} ({})", RULE_NAMES[i]);
        }
    });
}

/// `app_backgrounds = "opaque"`: every background the application set is
/// opaque, whatever glyph is on it, and under reverse too; a cell on the
/// default background stays glass -- including a reversed one, whose drawn
/// background is the default foreground, raised but not opaque.
#[test]
fn opaque_keeps_every_set_background_and_only_those() {
    let theme = Theme {
        app_bg: AppBg::Opaque,
        ..dim_theme()
    };
    render_with(theme, RULE_ROW, |r| {
        let bg = r.background();
        let want = [0xFF, 0xFF, 0xFF, 0xFF, DIM, RAISED];
        for (i, c) in bg[..6].iter().enumerate() {
            assert_eq!(alpha(*c), want[i], "cell {i} ({})", RULE_NAMES[i]);
        }
    });
}

/// **The case the keys exist for, on a real capture**: chafa's truecolour
/// output (`tests/previews/`), fed the way yazi feeds it -- each line at the
/// pane's origin, a row further down each time -- is opaque in every one of
/// its 1148 cells unless *both* keys say dim: `app_backgrounds = "opaque"`
/// keeps it because it keeps every set background, and `pictures =
/// "opaque"`, the default, keeps it because it is a picture. A cell of the
/// default background beside it is glass under all four.
#[test]
fn a_chafa_preview_is_dimmed_only_when_both_keys_ask() {
    let path = format!(
        "{}/../tests/previews/chafa-truecolour.ans",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).expect("the capture");
    let ws = WinSize {
        ws_row: 30,
        ws_col: 44,
    };
    let (row0, col0) = (1usize, 2usize);
    let mut bytes = Vec::new();
    let mut rows = 0usize;
    for (i, line) in text.split('\n').filter(|l| !l.is_empty()).enumerate() {
        bytes.extend(format!("\x1b[{};{}H", row0 + i + 1, col0 + 1).into_bytes());
        bytes.extend(line.as_bytes());
        rows += 1;
    }
    bytes.extend(b"\x1b[30;44H");
    assert_eq!(rows, 28, "the capture is not the 41x28 it was");
    let cols = usize::from(ws.ws_col);
    for (app_bg, picture_bg, want) in [
        (AppBg::Dimmed, PictureBg::Dimmed, DIM),
        (AppBg::Dimmed, PictureBg::Opaque, 0xFF),
        (AppBg::Opaque, PictureBg::Dimmed, 0xFF),
        (AppBg::Opaque, PictureBg::Opaque, 0xFF),
    ] {
        let theme = Theme {
            app_bg,
            picture_bg,
            ..dim_theme()
        };
        render_ws(theme, &ws, &bytes, |r| {
            let bg = r.background();
            let mut off = 0usize;
            for y in row0..row0 + rows {
                for x in col0..col0 + 41 {
                    if alpha(bg[y * cols + x]) != want {
                        off += 1;
                    }
                }
            }
            assert_eq!(
                off, 0,
                "{app_bg:?}/{picture_bg:?}: {off} of 1148 preview cells are not {want:#04x}"
            );
            assert_eq!(
                alpha(bg[0]),
                DIM,
                "{app_bg:?}/{picture_bg:?}: the default background left the glass"
            );
        });
    }
}
