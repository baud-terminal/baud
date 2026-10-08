//! The embedded font's Hebrew, through the renderer.
//!
//! `baud-app/fonts/built/Regular.ttf` and `Bold.ttf` are JetBrains Mono's
//! Regular and Bold with baud's Hebrew added (`scripts/build-fonts.sh`), and
//! nothing else: so a frame painted with the built Regular is the frame stock
//! JetBrains Mono paints, except in a cell that holds Hebrew and the cell to
//! its right, which a glyph overhanging its cell can reach. Every recording is
//! held to that, with `vim-unicode` and `cat-cjk` -- the two whose final
//! screens carry Hebrew -- as the control that the built font reached the
//! pixels at all.
//!
//! And through the stack the app builds: bold Hebrew paints what the built
//! Bold paints as the only text face, and differs from the Regular's. Italic
//! and bold-italic Hebrew paint the built Regular's frame, because the italic
//! slots are stock JetBrains Mono for now (STOCK_ITALIC, interim) and the
//! chain answers for what they lack.
//!
//! These are properties of the committed files, which baud-render reads by
//! path; which bytes the app compiles in is `baud-app/src/embedded.rs`'s
//! business.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, FaceRole, FaceStyle, PxSize, Renderer, Stack, Theme};
use support::*;

fn app_font(path: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../baud-app/fonts/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Whether `c` is in the Hebrew block, U+0590-05FF.
fn hebrew(c: Codepoint) -> bool {
    matches!(c.value(), 0x0590..=0x05FF)
}

/// For each cell of the screen, row by row, whether a frame may differ
/// there: the cell holds Hebrew, as its base or as a mark riding on it, or
/// the cell to its left does. And how many cells hold Hebrew.
fn may_differ(t: &Term<'_>) -> (Vec<bool>, usize) {
    let (rows, cols) = (usize::from(WS.ws_row), usize::from(WS.ws_col));
    let mut mask = vec![false; rows * cols];
    let mut held = 0;
    for y in 0..rows {
        let row = t.row(Row::from_index(y));
        for x in 0..cols {
            let base = row[x].content.as_codepoint().is_some_and(hebrew);
            let marks = t
                .marks(Row::from_index(y), Col::at_index(x))
                .filter_map(Codepoint::new)
                .any(hebrew);
            if base || marks {
                held += 1;
                mask[y * cols + x] = true;
                if x + 1 < cols {
                    mask[y * cols + x + 1] = true;
                }
            }
        }
    }
    (mask, held)
}

/// **The built Regular paints every recording as stock JetBrains Mono
/// does, but its Hebrew.** Each recording's final screen, painted at 14 px
/// with each file as the only face, compared cell by cell: a cell that
/// differs is a cell holding Hebrew or the one right of it, and a recording
/// with no Hebrew paints the same frame byte for byte. The control:
/// `vim-unicode`'s Hebrew, `.notdef` boxes in the stock file, draws -- 55
/// cells, and `cat-cjk`'s 5, every one of them changed (measured, and
/// pinned, so a recording or a mask that moves says so).
#[test]
fn the_built_regular_paints_every_recording_as_stock_does_but_its_hebrew() {
    let built = app_font("built/Regular.ttf");
    let stock = app_font("jetbrains-mono/JetBrainsMono-Regular.ttf");
    let (built, stock) = (
        [Face::parse(&built).unwrap()],
        [Face::parse(&stock).unwrap()],
    );
    let cols = usize::from(WS.ws_col);
    let mut hebrew = Vec::new();
    for name in RECORDINGS {
        let bytes = recording(name);
        let paint_with = |faces: &[Face<'_>]| {
            let mut r = renderer(faces);
            let mut c = Canvas::for_renderer(&r);
            let mut mask = (Vec::new(), 0);
            replay_final(&bytes, |t| {
                mask = may_differ(t);
                paint(&mut r, t, &mut c, Damage::Full);
            });
            let m = r.metrics();
            (c, mask, (m.cell_w.value(), m.cell_h.value()))
        };
        let (b, (mask, held), cell) = paint_with(&built);
        let (s, _, stock_cell) = paint_with(&stock);
        assert_eq!(cell, stock_cell, "{name}: the premise -- one cell size");
        assert_eq!((b.width, b.height), (s.width, s.height), "{name}");
        let (cw, ch) = (
            usize::try_from(cell.0).unwrap(),
            usize::try_from(cell.1).unwrap(),
        );
        let differs = |y: usize, x: usize| {
            (y * ch..(y + 1) * ch).any(|py| {
                let at = py * b.width + x * cw;
                b.pixels[at..at + cw] != s.pixels[at..at + cw]
            })
        };
        let mut changed = 0;
        let mut outside = Vec::new();
        for y in 0..usize::from(WS.ws_row) {
            for x in 0..cols {
                if differs(y, x) {
                    changed += 1;
                    if !mask[y * cols + x] {
                        outside.push((y, x));
                    }
                }
            }
        }
        eprintln!("{name}: {held} cells hold Hebrew, {changed} cells changed");
        assert!(
            outside.is_empty(),
            "{name}: {} cells that neither hold Hebrew nor follow one changed: (row, col) {:?}",
            outside.len(),
            &outside[..outside.len().min(8)]
        );
        if held == 0 {
            assert!(
                b.pixels == s.pixels,
                "{name}: no Hebrew, and the frame moved"
            );
        } else {
            hebrew.push((name, held, changed));
        }
    }
    assert_eq!(
        hebrew,
        [("utf8/cat-cjk.raw", 5, 5), ("utf8/vim-unicode.raw", 55, 55)],
        "the control: the recordings that carry Hebrew, its cells and the cells that changed"
    );
}

/// **A Hebrew point riding a Latin letter is Hebrew too**, which no
/// recording carries: every point in them rides a Hebrew letter. `a` with
/// qamats draws the point from the built Regular and drops it in the stock
/// file, so the cell differs, and `may_differ` must count it by its mark.
#[test]
fn a_hebrew_point_on_a_latin_letter_counts_as_hebrew() {
    let built = app_font("built/Regular.ttf");
    let stock = app_font("jetbrains-mono/JetBrainsMono-Regular.ttf");
    let (built, stock) = (
        [Face::parse(&built).unwrap()],
        [Face::parse(&stock).unwrap()],
    );
    let text = "a\u{5B8}".as_bytes();
    let paint_with = |faces: &[Face<'_>]| {
        let mut r = renderer(faces);
        let mut c = Canvas::for_renderer(&r);
        let mut mask = (Vec::new(), 0);
        replay_final(text, |t| {
            mask = may_differ(t);
            paint(&mut r, t, &mut c, Damage::Full);
        });
        (c.pixels, mask)
    };
    let (b, (mask, held)) = paint_with(&built);
    let (s, _) = paint_with(&stock);
    assert!(b != s, "the premise: the point reaches the pixels");
    assert_eq!(held, 1, "the cell holding the point");
    assert!(mask[0] && mask[1], "the cell and the one right of it");
}

const WS2: WinSize = WinSize {
    ws_row: 2,
    ws_col: 16,
};

/// The pixels `bytes` paints through a stack of `faces` with `roles`.
fn painted(faces: &[Face<'_>], roles: &[FaceRole], bytes: &[u8]) -> Vec<u32> {
    let mut r = Renderer::new(
        Stack::with_roles(faces, roles).unwrap(),
        PxSize::of(16.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS2.ws_col)),
        Rows::of_index(usize::from(WS2.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS2);
    let mut t = Term::new(owned.storage(), &WS2);
    let mut c = Canvas::for_renderer(&r);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    paint(&mut r, &mut t, &mut c, Damage::Full);
    c.pixels
}

/// The rendition, in a truecolour grey so bold's brightening of palette
/// 0-7 cannot be what differs.
fn sgr(style: FaceStyle) -> &'static [u8] {
    match style {
        FaceStyle::Regular => b"\x1b[0;38;2;200;200;200m",
        FaceStyle::Bold => b"\x1b[0;1;38;2;200;200;200m",
        FaceStyle::Italic => b"\x1b[0;3;38;2;200;200;200m",
        FaceStyle::BoldItalic => b"\x1b[0;1;3;38;2;200;200;200m",
    }
}

/// Shalom, pointed: shin with qamats and the shin dot, lamed, vav with
/// holam, final mem.
const SHALOM: &str = "\u{5E9}\u{5B8}\u{5C1}\u{5DC}\u{5D5}\u{5B9}\u{5DD}";

const STYLED: [FaceRole; 5] = [
    FaceRole::Fallback,
    FaceRole::Fallback,
    FaceRole::Style(FaceStyle::Bold),
    FaceRole::Style(FaceStyle::Italic),
    FaceRole::Style(FaceStyle::BoldItalic),
];

/// **Bold Hebrew draws from the built Bold, and italic Hebrew from the
/// chain.** On the stack the app builds by default -- the built Regular, the
/// icon face, then the built Bold and the stock Italic and Bold Italic as the
/// styled faces -- `SGR 1` shalom paints exactly what the built Bold paints
/// as the only text face, and not what the Regular paints. `SGR 3` and
/// `SGR 1;3` shalom paint the built Regular's frame: the stock italics lack
/// Hebrew, so the chain draws it upright and regular weight (STOCK_ITALIC,
/// interim), while italic Latin on the same stack is the italic face's.
#[test]
fn bold_hebrew_draws_from_the_built_bold_and_italic_from_the_chain() {
    let files = [
        app_font("built/Regular.ttf"),
        app_font("symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
        app_font("built/Bold.ttf"),
        app_font("jetbrains-mono/JetBrainsMono-Italic.ttf"),
        app_font("jetbrains-mono/JetBrainsMono-BoldItalic.ttf"),
    ];
    let faces: Vec<Face<'_>> = files.iter().map(|d| Face::parse(d).unwrap()).collect();
    let plain = [FaceRole::Fallback, FaceRole::Fallback];
    let alone = |i: usize| {
        [
            Face::parse(&files[i]).unwrap(),
            Face::parse(&files[1]).unwrap(),
        ]
    };
    let line = |style: FaceStyle, text: &str| [sgr(style), text.as_bytes()].concat();
    let regular = painted(&alone(0), &plain, &line(FaceStyle::Regular, SHALOM));
    let stock = app_font("jetbrains-mono/JetBrainsMono-Regular.ttf");
    let boxes = painted(
        &[
            Face::parse(&stock).unwrap(),
            Face::parse(&files[1]).unwrap(),
        ],
        &plain,
        &line(FaceStyle::Regular, SHALOM),
    );
    assert!(
        regular != boxes,
        "the control: the built Regular draws Hebrew"
    );

    let bold = painted(&faces, &STYLED, &line(FaceStyle::Bold, SHALOM));
    let want = painted(&alone(2), &plain, &line(FaceStyle::Regular, SHALOM));
    assert!(bold == want, "bold Hebrew is not the built Bold's");
    assert!(bold != regular, "the control: bold Hebrew is the Regular's");

    let shin = Codepoint::new(0x5E9).unwrap();
    for (style, ix) in [(FaceStyle::Italic, 3), (FaceStyle::BoldItalic, 4)] {
        assert!(
            faces[ix].glyph(shin).is_none(),
            "{style:?}: the premise of the interim -- its face is stock JetBrains Mono"
        );
        let got = painted(&faces, &STYLED, &line(style, SHALOM));
        assert!(
            got == regular,
            "{style:?}: the italic slots are stock until the owner answers the lean, so \
             their Hebrew is the chain's -- the built Regular, upright and regular weight"
        );
        let latin = painted(&faces, &STYLED, &line(style, "ab"));
        let upright = painted(&alone(0), &plain, &line(FaceStyle::Regular, "ab"));
        assert!(
            latin != upright,
            "{style:?}: the control -- italic Latin on this stack is the italic face's"
        );
    }
}
