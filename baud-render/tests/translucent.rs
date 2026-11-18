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
use baud_domain::{BytePos, Chan, Col, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    AppBg, Argb, Backdrop, Blend, Damage, Floor, GlassAlpha, Kind, PictureBg, PxSize, Renderer,
    Stack, Theme, contrast_over,
};
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

/// A colour's channels, its alpha aside.
fn rgb(c: Argb) -> u32 {
    c.0 & 0x00FF_FFFF
}

/// The glass's floor: WCAG 2.2 SC 1.4.3's ratio for text that is not
/// large-scale (`[[source: wcag22]]`).
const FLOOR: f64 = 4.5;

/// The colour of the glyph drawn in cell `col` of the first row, found
/// by the middle of its quad, so a bearing past the cell's edge does not
/// move it into the next: a mask, or for a block element drawn as a
/// rectangle, a solid. `None` when the cell drew none.
fn glyph_in(r: &Renderer<'_, '_>, col: Col) -> Option<Argb> {
    let m = r.metrics();
    let side = |p: baud_font::Px| i32::try_from(p.value()).unwrap();
    let (cw, ch) = (side(m.cell_w), side(m.cell_h));
    let c = i32::try_from(col.index()).unwrap();
    r.instances()
        .filter(|i| matches!(i.kind, Kind::Mask | Kind::Solid))
        .find(|i| {
            let x = i32::from(i.x.0) + i32::from(i.w.0) / 2;
            let y = i32::from(i.y.0) + i32::from(i.h.0) / 2;
            (0..ch).contains(&y) && (c * cw..(c + 1) * cw).contains(&x)
        })
        .map(|i| i.color)
}

/// [`glyph_in`], for a cell that must have drawn one.
fn text_in(r: &Renderer<'_, '_>, col: Col) -> Argb {
    glyph_in(r, col).unwrap_or_else(|| panic!("no glyph in cell {}", col.index()))
}

/// **How `text` reads on `page`** drawn at the page's own alpha over the
/// glass: its contrast against the worst grey from black to white
/// (`contrast_over` over `Backdrop::any`), and beside it against the page
/// over white and over black. Printed, so a run says what it measured.
fn reads(what: &str, text: Argb, page: Argb) -> f64 {
    let a = baud_domain::Chan::from_u8(alpha(page));
    let worst = contrast_over(text, page, a, Backdrop::any());
    let grey = |v: u8| {
        let c = baud_domain::Chan::from_u8(v);
        Argb::rgb(c, c, c)
    };
    // At full alpha nothing behind counts: WCAG's ratio of two colours.
    let solid = Chan::from_u8(0xFF);
    let (white, black) = (
        contrast_over(text, page.mix(grey(0xFF), a), solid, Backdrop::any()),
        contrast_over(text, page.mix(grey(0), a), solid, Backdrop::any()),
    );
    eprintln!(
        "{what}: {text:?} on {page:?} at {}: worst {worst:.3}, over white {white:.3}, over black {black:.3}",
        alpha(page)
    );
    worst
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

/// **Every palette colour as text on the default page, under the glass**,
/// on both built-in themes at the dim these tests draw at and at the
/// default glass opacity, each raised as the app raises it to the theme's
/// reach (`Theme::glass_reach`): each reads at the floor over every grey
/// from black to white. What it measures is printed, pair by pair.
#[test]
fn every_palette_text_reads_on_the_default_page_under_the_glass() {
    let ws = WinSize {
        ws_row: 2,
        ws_col: 20,
    };
    // `X` in the default foreground, then in each of the sixteen colours.
    let mut bytes = b"X".to_vec();
    for n in (30..38).chain(90..98) {
        bytes.extend(format!("\x1b[{n}mX").into_bytes());
    }
    bytes.extend(b"\x1b[0m\x1b[2;20H");
    let mut low = Vec::new();
    for (name, base) in [("dark", Theme::dark()), ("light", Theme::light())] {
        // The dim these tests use and the shipped default, 0.70 of 255.
        for dim in [DIM, 179] {
            let mut theme = Theme {
                bg_alpha: Chan::from_u8(dim),
                ..base.clone()
            };
            // The lowest alpha at which each asked colour reads at the
            // floor on the default page over every grey, and the highest
            // of those: what the page must be drawn at for all of them.
            let asked = core::iter::once(theme.fg).chain(theme.palette[..16].iter().copied());
            let reach: Vec<u8> = asked
                .map(|text| {
                    theme
                        .floor
                        .page_alpha(text, theme.bg, theme.bg_alpha, Backdrop::any())
                        .get()
                })
                .collect();
            eprintln!(
                "{name} at {dim}: Floor::page_alpha on the default page: default fg {}, palette {:?}, highest {}",
                reach[0],
                &reach[1..],
                reach.iter().max().copied().unwrap_or(dim)
            );
            // The window draws the theme at its reach, as the app does.
            theme.bg_alpha = theme.glass_reach();
            let drawn = theme.bg_alpha.get();
            eprintln!("{name} at {dim}: drawn at {drawn}");
            render_ws(theme, &ws, &bytes, |r| {
                let bg = r.background();
                for (col, &page) in bg.iter().enumerate().take(17) {
                    let what = match col {
                        0 => format!("{name} at {dim}: default foreground"),
                        c => format!("{name} at {dim}: palette {}", c - 1),
                    };
                    assert_eq!(alpha(page), drawn, "{what}: the default page's alpha");
                    let Some(text) = glyph_in(r, Col::at_index(col)) else {
                        low.push(format!("{what}: no glyph found"));
                        continue;
                    };
                    let ratio = reads(&what, text, page);
                    if ratio < FLOOR {
                        low.push(format!("{what}: {ratio:.3}"));
                    }
                }
            });
        }
    }
    assert!(low.is_empty(), "under the floor: {low:#?}");
}

/// **Where each built-in theme's default page reaches 4.5:1 under the
/// glass** (`Theme::glass_reach`): the dark theme at 146 and the light at
/// 125 from a dim of 0.22, below opaque on both even from a clear page;
/// and at the default opacity, 0.70, the theme's own alpha, where every
/// colour already reaches it.
#[test]
fn glass_reach_is_pinned_on_both_built_in_themes() {
    let at = |base: Theme, a: Chan| {
        Theme {
            bg_alpha: a,
            ..base
        }
        .glass_reach()
    };
    let (dim, default) = (Chan::from_u8(DIM), Chan::from_u8(179));
    assert_eq!(at(Theme::dark(), dim), Chan::from_u8(146), "dark at 0.22");
    assert_eq!(at(Theme::light(), dim), Chan::from_u8(125), "light at 0.22");
    for (name, base) in [("dark", Theme::dark()), ("light", Theme::light())] {
        assert_eq!(at(base.clone(), default), default, "{name} at 0.70");
        for a in [Chan::from_u8(0), dim] {
            let reach = at(base.clone(), a);
            assert!(reach.get() < 0xFF, "{name} from {a:?} goes opaque");
        }
    }
}

/// **A configured alpha below the theme's reach is raised to it, and one
/// at or above it is drawn as set** (`GlassAlpha`): the dark theme set at
/// 0.22 is drawn at 146, whatever alpha the theme carries; set at the
/// default opacity, 0.70, at 179. Perturbation: give `AsSet` always -- the
/// first assertion fails.
#[test]
fn glass_alpha_raises_a_dim_below_the_reach_and_keeps_one_above_it() {
    let (dim, default) = (Chan::from_u8(DIM), Chan::from_u8(179));
    let raised = GlassAlpha::of(&Theme::dark(), dim);
    assert_eq!(
        raised,
        GlassAlpha::Raised {
            configured: dim,
            effective: Chan::from_u8(146),
        }
    );
    assert_eq!(raised.effective(), Chan::from_u8(146));
    let kept = GlassAlpha::of(&Theme::dark(), default);
    assert_eq!(kept, GlassAlpha::AsSet(default));
    assert_eq!(kept.effective(), default);
}

/// **The reach stays below opaque on every page**: with white or black
/// text and every one of the sixteen in it, a default page of any grey
/// from black to white, from a clear window, reaches 4.5:1 below opaque,
/// since a page drawn opaque lets black or white read at 4.5:1 or more on
/// it. Every pair is counted (`Theme::glass_reach`), and none takes the
/// window opaque. Perturbation: end `Floor::page_alpha`'s bisection at
/// opaque -- every page fails but the two whose text is the page's own.
#[test]
fn glass_reach_stays_below_opaque_on_every_grey_page() {
    let grey = |v: u8| {
        let c = Chan::from_u8(v);
        Argb::rgb(c, c, c)
    };
    let mut checked = 0;
    for v in (0..=0xFFu8).step_by(17) {
        for text in [grey(0xFF), grey(0)] {
            let mut theme = Theme {
                fg: text,
                bg: grey(v),
                bg_alpha: Chan::from_u8(0),
                ..Theme::dark()
            };
            theme.palette[..16].fill(text);
            let reach = theme.glass_reach();
            eprintln!("{text:?} on grey {v:#04x}: reach {}", reach.get());
            assert!(reach.get() < 0xFF, "{text:?} on grey {v:#04x} goes opaque");
            checked += 1;
        }
    }
    assert_eq!(checked, 32, "the pages checked");
}

/// **Text on the default page is lifted under the glass**
/// (`Floor::at_target`, `Floor::lift_on_glass`): palette red on the dark
/// theme's default page at the default opacity, 0.70, reads at 4.5:1 or above on
/// the page drawn opaque, so the floor leaves it there, and against that
/// page over every grey it reads 1.7:1. It is drawn as the glass floor
/// lifts it, at 4.5:1 over every grey. Perturbation: leave the default
/// page out of the glass floor in `resolve_slot` -- red is drawn as asked.
#[test]
fn default_page_text_is_lifted_under_the_glass() {
    let theme = Theme {
        bg_alpha: Chan::from_u8(179),
        ..Theme::dark()
    };
    let red = theme.palette[1];
    let want =
        theme
            .floor
            .at_target()
            .lift_on_glass(red, theme.bg, theme.bg_alpha, Backdrop::any());
    assert_ne!(rgb(want), rgb(red), "the premise: the floor lifts red here");
    render_with(theme, b"\x1b[31mX\x1b[0m\x1b[2;12H", |r| {
        let bg = r.background();
        assert_eq!(
            alpha(bg[0]),
            179,
            "the default page keeps the theme's alpha"
        );
        let text = text_in(r, Col::at_index(0));
        assert_eq!(rgb(text), rgb(want), "drawn as the glass floor lifts it");
        let ratio = reads("palette 1 on the default page", text, bg[0]);
        assert!(ratio >= FLOOR, "reads {ratio:.3}:1");
    });
}

/// **The default page's text is lifted to the floor's target, not to the
/// glass band**: on the dark theme drawn at its raised alpha
/// (`Theme::glass_reach`), its default foreground lifted only below the
/// glass trigger reads under 4.5:1 against some grey from black to white,
/// and lifted with `Floor::at_target` reads at 4.5:1 or more -- and the
/// renderer draws the second. Perturbation: lift the default page with the
/// theme's own floor in `resolve_slot` and the drawn glyph reads under the
/// floor.
#[test]
fn the_default_page_text_reaches_the_target_not_the_glass_band() {
    let mut theme = dim_theme();
    theme.bg_alpha = theme.glass_reach();
    let any = Backdrop::any();
    let a = theme.bg_alpha;
    let banded = theme.floor.lift_on_glass(theme.fg, theme.bg, a, any);
    let ratio = contrast_over(banded, theme.bg, a, any);
    assert!(
        ratio < FLOOR,
        "the premise: the band alone keeps it at {ratio:.3}:1"
    );
    let lifted = theme
        .floor
        .at_target()
        .lift_on_glass(theme.fg, theme.bg, a, any);
    let ratio = contrast_over(lifted, theme.bg, a, any);
    assert!(ratio >= FLOOR, "at the target it reads {ratio:.3}:1");
    render_with(theme, b"A\x1b[2;12H", |r| {
        let drawn = reads(
            "dark default fg at the reach",
            text_in(r, Col::at_index(0)),
            r.background()[0],
        );
        assert!(
            drawn >= FLOOR,
            "the default page's text is drawn at {drawn:.3}:1"
        );
    });
}

/// **Palette colours asked below the target are lifted to it under the
/// glass**: a red, blue and magenta of `#cd0000`, `#0000ee` and `#cd00cd`
/// in palette entries 1, 4 and 5 read under 4.5:1
/// on the dark theme's page drawn opaque, and are drawn as asked there.
/// Under the glass each is drawn at 4.5:1 or above against the page over
/// every grey -- at the theme's reach from a dim of 0.22, and at 250, where
/// the floor's own bands keep all three below the target. Perturbation:
/// draw the default page by the theme's floor in `resolve_slot`, or keep a
/// pair asked below the target in `Floor::glass_band` -- at 250 the three
/// read under 4.5:1.
#[test]
fn palette_colours_asked_below_the_target_are_lifted_under_the_glass() {
    let c = Chan::from_u8;
    let low = [
        (1, Argb::rgb(c(0xCD), c(0), c(0))),
        (4, Argb::rgb(c(0), c(0), c(0xEE))),
        (5, Argb::rgb(c(0xCD), c(0), c(0xCD))),
    ];
    let mut base = dim_theme();
    for &(i, colour) in &low {
        base.palette[i] = colour;
    }
    let any = Backdrop::any();
    for &(i, colour) in &low {
        let asked = contrast_over(colour, base.bg, c(0xFF), any);
        assert!(
            asked < FLOOR,
            "the premise: palette {i} asked at {asked:.3}:1 drawn opaque"
        );
        let banded = base.floor.lift_on_glass(colour, base.bg, c(250), any);
        let banded = contrast_over(banded, base.bg, c(250), any);
        assert!(
            banded < FLOOR,
            "the premise: at 250 the bands keep palette {i} at {banded:.3}:1"
        );
    }
    let bytes = b"\x1b[31mX\x1b[34mX\x1b[35mX\x1b[0m\x1b[2;12H";
    let reach = base.glass_reach();
    for a in [reach, c(250)] {
        let theme = Theme {
            bg_alpha: a,
            ..base.clone()
        };
        assert_eq!(theme.glass_reach(), a, "drawn at its own reach");
        render_with(theme, bytes, |r| {
            let bg = r.background();
            for (col, &(i, _)) in low.iter().enumerate() {
                let what = format!("palette {i} at {}", a.get());
                let drawn = reads(&what, text_in(r, Col::at_index(col)), bg[col]);
                assert!(drawn >= FLOOR, "{what}: drawn at {drawn:.3}:1");
            }
        });
    }
    render_with(
        Theme {
            bg_alpha: c(0xFF),
            ..base.clone()
        },
        bytes,
        |r| {
            for (col, &(i, colour)) in low.iter().enumerate() {
                let text = text_in(r, Col::at_index(col));
                assert_eq!(rgb(text), rgb(colour), "palette {i} opaque: as asked");
            }
        },
    );
}

/// **A colour hidden on its page stays hidden under the glass**: text in
/// its page's own colour -- `SGR 31;41`, the one colour word on both sides,
/// which the floor leaves on the page drawn opaque -- is drawn in the
/// page's colour under the glass too, at the theme's reach and opaque, on
/// both built-in themes (`Floor::lift_on_glass`, `Floor::page_alpha`). One
/// step off the page in one channel, the text is lifted to 4.5:1 or above
/// under the glass. On the default page, text in the page's colour is
/// lifted by the floor's own trigger on the page drawn opaque
/// (`Floor::lift`), and reads at 4.5:1 or above there and under the glass.
/// Perturbation: drop the hidden-pair test in `Floor::lift_on_glass` and
/// `Floor::page_alpha` -- the page's own colour is lifted to near-white on the
/// dark theme and near-black on the light, and its page raised from 0.22.
#[test]
fn a_colour_hidden_on_its_page_stays_hidden_under_the_glass() {
    let any = Backdrop::any();
    for (name, base) in [("dark", Theme::dark()), ("light", Theme::light())] {
        let theme = Theme {
            bg_alpha: Chan::from_u8(DIM),
            ..base
        };
        let reach = theme.glass_reach();
        let at = theme.floor.at_target();
        let page = theme.bg;
        assert_eq!(
            rgb(at.lift_on_glass(page, page, reach, any)),
            rgb(page),
            "{name}: the page's own colour is kept"
        );
        let dim = theme.bg_alpha;
        assert_eq!(
            at.page_alpha(page, page, dim, any),
            dim,
            "{name}: its page is not raised"
        );
        let [_, r, g, b] = page.0.to_be_bytes();
        let off = Argb::rgb(
            Chan::from_u8(r),
            Chan::from_u8(g),
            Chan::from_u8(if b < 0xFF { b + 1 } else { b - 1 }),
        );
        let lifted = at.lift_on_glass(off, page, reach, any);
        let ratio = contrast_over(lifted, page, reach, any);
        assert!(
            ratio >= FLOOR,
            "{name}: one step off, {off:?} is lifted to {ratio:.3}:1"
        );
        let truecolour = |c: Argb| {
            let [_, r, g, b] = c.0.to_be_bytes();
            format!("\x1b[38;2;{r};{g};{b}mX\x1b[0m")
        };
        let bytes = format!(
            "\x1b[31;41mX\x1b[0m{}{}\x1b[2;12H",
            truecolour(off),
            truecolour(page)
        )
        .into_bytes();
        for a in [reach, Chan::from_u8(0xFF)] {
            let drawn = Theme {
                bg_alpha: a,
                ..theme.clone()
            };
            render_with(drawn, &bytes, |r| {
                let bg = r.background();
                let at_a = format!("{name} at {}", a.get());
                let text = text_in(r, Col::at_index(0));
                assert_eq!(rgb(text), rgb(bg[0]), "{at_a}: hidden text is the page's");
                let ratio = reads(
                    &format!("{at_a}: one step off the page"),
                    text_in(r, Col::at_index(1)),
                    bg[1],
                );
                assert!(ratio >= FLOOR, "{at_a}: one step off reads {ratio:.3}:1");
                let ratio = reads(
                    &format!("{at_a}: the page's colour on the default page"),
                    text_in(r, Col::at_index(2)),
                    bg[2],
                );
                assert!(ratio >= FLOOR, "{at_a}: on the default page {ratio:.3}:1");
            });
        }
    }
}

/// **Faint text reads at the target under the glass and stays faint
/// opaque**: `SGR 2` mixes the default foreground half way to the page,
/// which on the page drawn opaque reads under 4.5:1 on both built-in themes
/// and is drawn so. Under the glass, at the default opacity, 0.70, and at
/// 250, the faint glyph reads at 4.5:1 or above against the page over
/// every grey, and below the normal text beside it wherever that reads
/// above 4.5:1. Perturbation: keep a pair asked below the target in
/// `Floor::glass_band` -- the light theme's faint text at 0.70 reads under
/// 4.5:1.
#[test]
fn faint_text_reads_at_the_target_under_the_glass_and_stays_faint_opaque() {
    let half = |x: u8, y: u8| u8::try_from((u16::from(x) + u16::from(y)).div_ceil(2)).unwrap();
    let bytes = b"X\x1b[2mX\x1b[0m\x1b[2;12H";
    for (name, base) in [("dark", Theme::dark()), ("light", Theme::light())] {
        let [_, fr, fg, fb] = base.fg.0.to_be_bytes();
        let [_, br, bgc, bb] = base.bg.0.to_be_bytes();
        let faint = Argb::rgb(
            Chan::from_u8(half(fr, br)),
            Chan::from_u8(half(fg, bgc)),
            Chan::from_u8(half(fb, bb)),
        );
        let asked = contrast_over(faint, base.bg, Chan::from_u8(0xFF), Backdrop::any());
        assert!(
            asked < FLOOR,
            "the premise: {name} faint asked at {asked:.3}:1"
        );
        render_with(
            Theme {
                bg_alpha: Chan::from_u8(0xFF),
                ..base.clone()
            },
            bytes,
            |r| {
                let text = text_in(r, Col::at_index(1));
                assert_eq!(rgb(text), rgb(faint), "{name} opaque: drawn faint");
            },
        );
        for a in [179, 250] {
            let theme = Theme {
                bg_alpha: Chan::from_u8(a),
                ..base.clone()
            };
            assert_eq!(theme.glass_reach().get(), a, "{name} at {a}: at its reach");
            render_with(theme, bytes, |r| {
                let bg = r.background();
                let normal = reads(
                    &format!("{name} at {a}: normal"),
                    text_in(r, Col::at_index(0)),
                    bg[0],
                );
                let drawn = reads(
                    &format!("{name} at {a}: faint"),
                    text_in(r, Col::at_index(1)),
                    bg[1],
                );
                assert!(drawn >= FLOOR, "{name} at {a}: faint drawn at {drawn:.3}:1");
                if normal > FLOOR {
                    assert!(
                        drawn < normal,
                        "{name} at {a}: faint {drawn:.3}:1 against normal {normal:.3}:1"
                    );
                }
            });
        }
    }
}

/// **The reach raises the page as far as the text is drawn by**: a theme
/// whose every colour is white on black, configured at the lowest alpha at
/// which white reads 3:1 over every grey, reads under 4.5:1 there, and no
/// colour reaches 4.5:1 at that alpha -- so `Theme::glass_reach` raises it,
/// by the same rule `Floor::at_target` draws the text by, until white reads
/// at the target. Perturbation: take each pair's reach by the theme's own
/// floor, whose glass band keeps a pair reading 3:1 or more, and the reach
/// stays at the configured alpha, where white reads under 4.5:1.
#[test]
fn the_reach_raises_a_pair_the_glass_band_would_keep() {
    let any = Backdrop::any();
    let grey = |v: u8| {
        let c = Chan::from_u8(v);
        Argb::rgb(c, c, c)
    };
    let mut theme = Theme::dark();
    theme.bg = grey(0);
    theme.fg = grey(0xFF);
    for p in theme.palette[..16].iter_mut() {
        *p = grey(0xFF);
    }
    let white = grey(0xFF);
    let reads = |a: u8| contrast_over(white, theme.bg, Chan::from_u8(a), any);
    let configured = (0..=0xFFu8)
        .find(|&a| reads(a) >= Floor::GLASS_TRIGGER)
        .expect("white reads 3:1 at some alpha");
    assert!(
        reads(configured) < FLOOR,
        "the premise: at {configured} white reads {:.3}:1",
        reads(configured)
    );
    theme.bg_alpha = Chan::from_u8(configured);
    let reach = theme.glass_reach();
    assert!(
        reach.get() > configured,
        "the reach raises the page: {}",
        reach.get()
    );
    let drawn = theme
        .floor
        .at_target()
        .lift_on_glass(white, theme.bg, reach, any);
    let ratio = contrast_over(drawn, theme.bg, reach, any);
    assert!(ratio >= FLOOR, "at the reach the text reads {ratio:.3}:1");
}

/// **`Floor::at_target` draws a pair at least as well as the floor's own
/// bands**: over greys and colours of text and page and every alpha in
/// steps, the text it draws reads, over every grey, no lower than the
/// theme's own floor draws it.
#[test]
fn at_target_reads_at_least_as_well_as_the_floor() {
    let any = Backdrop::any();
    let c = Chan::from_u8;
    let floor = Theme::dark().floor;
    let colours: Vec<Argb> = (0..=0xFFu8)
        .step_by(17)
        .flat_map(|v| {
            [
                Argb::rgb(c(v), c(v), c(v)),
                Argb::rgb(c(v), c(0x40), c(0x10)),
                Argb::rgb(c(0x20), c(v), c(0xE0)),
            ]
        })
        .collect();
    let mut checked = 0usize;
    for &text in &colours {
        for &page in &colours {
            for a in (0..=0xFFu8).step_by(15) {
                let a = c(a);
                let read =
                    |f: Floor| contrast_over(f.lift_on_glass(text, page, a, any), page, a, any);
                let (drawn, own) = (read(floor.at_target()), read(floor));
                assert!(
                    drawn >= own,
                    "{text:?} on {page:?} at {}: {drawn:.3} against {own:.3}",
                    a.get()
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 10_000, "the grid checked {checked} pairs");
}
