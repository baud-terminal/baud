//! iTerm2's badge on the CPU path: where it sits, what it draws under the
//! text, the rows it repaints, and a trail over it (`[[source:
//! iterm2-badges]]`).

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Argb, BADGE, BOX, BadgeLook, Blend, Damage, FaceStyle, Held, PxSize, Renderer, Shape, Smear,
    Stack, Strength, Target, Theme, TrailLook,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 60,
    ws_row: 20,
};

fn px(p: Px) -> usize {
    usize::try_from(p.value()).unwrap()
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn renderer<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

fn look(text: &str) -> BadgeLook {
    BadgeLook {
        text: text.into(),
        colour: BADGE,
        fraction: BOX,
        top: Px::of(30),
        right: Px::of(10),
        scale: 1.0,
        style: FaceStyle::Regular,
    }
}

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, pixels: &mut Vec<u32>, damage: Damage) {
    let (w, h) = r.pixel_size();
    pixels.resize(px(w) * px(h), 0);
    let mut target = Target::new(pixels, w, w, h).unwrap();
    r.render(t, &mut target, damage);
}

/// The rectangle a frame's records cover, `[x0, y0, x1, y1)`.
fn bounds(recs: &[baud_render::Instance]) -> [i32; 4] {
    recs.iter()
        .fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |b, i| {
            let (x, y) = (i32::from(i.x.0), i32::from(i.y.0));
            [
                b[0].min(x),
                b[1].min(y),
                b[2].max(x + i32::from(i.w.0)),
                b[3].max(y + i32::from(i.h.0)),
            ]
        })
}

/// **The badge's colour is iTerm2's default**, calibrated red in sRGB, at
/// the quarter iTerm2 draws its half at.
#[test]
fn the_badge_colour_is_iterm2s_default_in_srgb() {
    use baud_domain::themefile::gamut::{Space, to_srgb};
    let ([r, g, b], _) = to_srgb(Space::Calibrated, [1.0, 0.0, 0.0]);
    assert_eq!(
        (BADGE.r().get(), BADGE.g().get(), BADGE.b().get()),
        (r, g, b)
    );
    assert_eq!(BADGE.a().get(), 64, "0.5 drawn at 0.5, of 255");
}

/// **The badge sits in the top right, inside iTerm2's box**: its glyphs
/// below the top margin and left of the right one, in half the width and
/// a fifth of the height, each line ending at the same right edge; and the
/// text is drawn over it: a full block's pixel under it keeps the text's
/// colour, while the page under it takes the badge's colour. The controls:
/// with no badge the page there is the page's.
#[test]
fn the_badge_sits_in_the_top_right_under_the_text() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // A row of full blocks across the top rows, under the badge.
    for row in 3..5 {
        feed(
            &mut t,
            format!("\x1b[{row};1H{}", "\u{2588}".repeat(60)).as_bytes(),
        );
    }
    feed(&mut t, b"\x1b[?25l");
    let mut r = renderer(&faces);
    let (w, h) = r.pixel_size();
    let (fw, fh) = (px(w), px(h));
    let (mut bare, mut drawn) = (Vec::new(), Vec::new());
    paint(&mut r, &mut t, &mut bare, Damage::Full);
    r.set_badge(Some(look("prod-db-01\nmain")));
    paint(&mut r, &mut t, &mut drawn, Damage::Full);
    let b = bounds(r.badge());
    assert!(!r.badge().is_empty(), "the badge has records");
    let (fwi, fhi) = (i32::try_from(fw).unwrap(), i32::try_from(fh).unwrap());
    assert!(b[1] >= 30, "below the top margin: {b:?}");
    assert!(b[2] <= fwi - 10, "left of the right margin: {b:?}");
    assert!(b[2] >= fwi - 10 - 4, "against it: {b:?}");
    assert!(b[2] - b[0] <= fwi / 2, "half the width: {b:?}");
    assert!(b[3] - 30 <= fhi / 5 + 4, "a fifth of the height: {b:?}");
    let text = Theme::dark().fg.0 & 0x00FF_FFFF;
    let page = Theme::dark().bg.0 & 0x00FF_FFFF;
    let mut under_text = 0;
    let mut on_page = 0;
    for y in usize::try_from(b[1]).unwrap()..usize::try_from(b[3]).unwrap() {
        for x in usize::try_from(b[0]).unwrap()..usize::try_from(b[2]).unwrap() {
            let (was, now) = (
                bare[y * fw + x] & 0x00FF_FFFF,
                drawn[y * fw + x] & 0x00FF_FFFF,
            );
            if was == text {
                assert_eq!(now, text, "the text over the badge at ({x}, {y})");
                under_text += 1;
            } else if was == page && now != page {
                on_page += 1;
            }
        }
    }
    assert!(under_text > 0, "the control: text lies under the badge");
    assert!(on_page > 0, "the badge draws on the page");
    for (i, (a, c)) in bare.iter().zip(&drawn).enumerate() {
        let (x, y) = (
            i32::try_from(i % fw).unwrap(),
            i32::try_from(i / fw).unwrap(),
        );
        if !(b[0]..b[2]).contains(&x) || !(b[1]..b[3]).contains(&y) {
            assert_eq!(a, c, "a pixel outside the badge at ({x}, {y})");
        }
    }
}

/// **The badge repaints the rows it leaves and reaches**: a new text, a
/// look change and the badge going, each on incremental damage, equal a
/// full repaint of the same frame.
#[test]
fn the_badge_changes_on_incremental_damage() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"hello\r\nworld");
    let mut r = renderer(&faces);
    let mut kept = Vec::new();
    r.set_badge(Some(look("first")));
    paint(&mut r, &mut t, &mut kept, Damage::Full);
    let red = Argb::rgb(Chan::from_u8(255), Chan::from_u8(0), Chan::from_u8(0));
    let steps: [Option<BadgeLook>; 4] = [
        Some(look("a much longer badge text")),
        Some(BadgeLook {
            colour: red,
            ..look("a much longer badge text")
        }),
        Some(look("x\ny\nz")),
        None,
    ];
    for (i, badge) in steps.into_iter().enumerate() {
        r.set_badge(badge.clone());
        paint(&mut r, &mut t, &mut kept, Damage::Incremental);
        let mut whole = Vec::new();
        let mut fresh = renderer(&faces);
        fresh.set_badge(badge);
        paint(&mut fresh, &mut t, &mut whole, Damage::Full);
        assert!(kept == whole, "step {i}: incremental differs from full");
    }
}

/// **No badge for white space**, and the same look on the same frame size
/// builds nothing again.
#[test]
fn a_badge_of_spaces_draws_nothing() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut r = renderer(&faces);
    r.set_badge(Some(look("   ")));
    assert!(r.badge().is_empty());
    r.set_badge(Some(look("x")));
    assert!(!r.badge().is_empty(), "the control: a letter draws");
}

/// **A repeat pass over the badge draws nothing**: the badge is a ground no
/// cell names, as an image under the text is. The control: the same pass
/// with no badge is capped at the page's.
#[test]
fn a_repeat_pass_over_the_badge_draws_nothing() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut held = Vec::new();
    for badge in [false, true] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?25l\x1b]12;rgb:00/00/00\x07");
        let mut r = renderer(&faces);
        r.set_badge(badge.then(|| look("prod-db-01")));
        let mut pixels = Vec::new();
        paint(&mut r, &mut t, &mut pixels, Damage::Full);
        let style = baud_core::CursorStyle::SteadyBlock;
        let (a, b) = (
            r.cursor_rect(Row::from_index(1), Col::at_index(0), style),
            r.cursor_rect(Row::from_index(8), Col::at_index(59), style),
        );
        r.set_trail(Some(TrailLook::Smear(Smear {
            corners: baud_render::Rect {
                left: a.left,
                top: a.top,
                right: b.right,
                bottom: b.bottom,
            }
            .corners(),
            hole: r.cursor_rect(Row::from_index(19), Col::at_index(0), style),
            shape: Shape::Quad,
            strength: Strength::UnderThreshold,
        })));
        for _ in 0..3 {
            paint(&mut r, &mut t, &mut pixels, Damage::Full);
        }
        held.push((r.flash_held(), r.flash_cap().get()));
    }
    assert_eq!(held[0], (Held::Capped, 0xFF), "the control: the page's cap");
    assert_eq!(held[1], (Held::Imaged, 0), "over the badge");
}

/// **The badge stays out from under the block cursor and an input
/// method's composition**, which iTerm2 draws over it: the pixels of a
/// block's cell and of a composition's cells are the frame's without the
/// badge. The controls: each lies inside the badge's rectangle, and the
/// badge draws elsewhere in it.
#[test]
fn the_badge_leaves_the_cursor_and_a_composition_alone() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = renderer(&faces);
    let (w, _) = r.pixel_size();
    let fw = px(w);
    let m = r.metrics();
    let (cw, ch) = (px(m.cell_w), px(m.cell_h));
    // The badge's rectangle, with no cursor to leave out.
    feed(&mut t, b"\x1b[?25l");
    r.set_badge(Some(look("MMMMMMMMMMMMMMMM")));
    let mut probe = Vec::new();
    paint(&mut r, &mut t, &mut probe, Damage::Full);
    let b = bounds(r.badge());
    let cell = |v: i32| usize::try_from(v).unwrap();
    let (row, col) = (cell(b[1] + b[3]) / 2 / ch, cell(b[0] + b[2]) / 2 / cw);
    // A steady block there, and a composition two cells to its left.
    feed(
        &mut t,
        format!("\x1b[2 q\x1b[?25h\x1b[{};{}H", row + 1, col + 1).as_bytes(),
    );
    let composition = baud_render::Composition {
        row: Row::from_index(row),
        col: Col::at_index(col - 4),
        text: "ab"
            .chars()
            .map(baud_domain::Codepoint::from_char)
            .collect(),
        ambiguous: baud_domain::AmbiguousWidth::Narrow,
        hints: Vec::new(),
        caret: baud_render::Caret::Hidden,
    };
    // The block alone, then the composition alone: while one is shown the
    // cursor is drawn in it, not at the block.
    for (comp, x0, x1) in [
        (None, col * cw, (col + 1) * cw),
        (Some(composition), (col - 4) * cw, (col - 2) * cw),
    ] {
        let mut frames = Vec::new();
        for badge in [None, Some(look("MMMMMMMMMMMMMMMM"))] {
            let mut r = renderer(&faces);
            r.set_badge(badge);
            r.set_composition(comp.clone());
            let mut pixels = Vec::new();
            paint(&mut r, &mut t, &mut pixels, Damage::Full);
            frames.push(pixels);
        }
        let (bare, drawn) = (&frames[0], &frames[1]);
        assert_ne!(bare, drawn, "the control: the badge draws");
        for y in row * ch..(row + 1) * ch {
            for x in x0..x1 {
                assert_eq!(bare[y * fw + x], drawn[y * fw + x], "({x}, {y})");
            }
        }
    }
}

/// **The atlas holds the badge at two sizes at most**: a window resized
/// through every size the badge fits at leaves the sheet no larger than
/// two sizes need, a third size starting the atlas over, and the text on
/// the screen draws throughout, the same as a renderer that never drew a
/// badge. The control: the badge's size does change with the box.
#[test]
fn the_badge_keeps_two_sizes_in_the_atlas() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // The text on the bottom row, below every box the badge takes.
    feed(&mut t, b"\x1b[?25l\x1b[20;1Hthe text on the screen");
    let mut r = renderer(&faces);
    let mut pixels = Vec::new();
    let resets = r.atlas_stats().grown;
    let mut heights = Vec::new();
    for n in 1..=40u8 {
        r.set_badge(Some(BadgeLook {
            fraction: (1.0, f32::from(n) / 80.0),
            ..look("ABCDEFGHIJKLMNOPQRSTUVWXYZ")
        }));
        paint(&mut r, &mut t, &mut pixels, Damage::Incremental);
        let b = bounds(r.badge());
        heights.push(b[3] - b[1]);
        assert!(r.atlas().side().value() <= 2048, "step {n}: the sheet grew");
    }
    heights.dedup();
    assert!(
        heights.len() > 3,
        "the control: the badge took sizes {heights:?}"
    );
    // Every glyph record the frame holds names ink in the atlas as it is
    // now, not a rectangle a start over emptied.
    let side = usize::try_from(r.atlas().side().value()).unwrap();
    let sheet = r.atlas().pixels();
    for i in r.instances().filter(|i| i.kind == baud_render::Kind::Mask) {
        let (u, v) = (usize::from(i.u.0), usize::from(i.v.0));
        let inked = (v..v + usize::from(i.h.0))
            .any(|y| (u..u + usize::from(i.w.0)).any(|x| sheet[y * side + x] != 0));
        assert!(inked, "a record names an empty rectangle: {i:?}");
    }
    assert!(
        r.atlas_stats().grown.value() > resets.value(),
        "the atlas started over"
    );
    r.set_badge(None);
    paint(&mut r, &mut t, &mut pixels, Damage::Incremental);
    let mut fresh = renderer(&faces);
    let mut whole = Vec::new();
    paint(&mut fresh, &mut t, &mut whole, Damage::Full);
    assert!(pixels == whole, "the text draws as it did");
}

/// **The badge is drawn in its own face, whatever face the text is**:
/// with Fira Code leading the stack and baud's embedded JetBrains Mono
/// Bold given the badge's role, its records -- each glyph's place
/// against the frame's right edge and its size -- are those of a stack
/// whose only face is JetBrains Mono Bold, grid and glyphs both, as
/// iTerm2's badge has a face of its own (`badgeFont:`, `[[source:
/// iterm2-964a75d-profile-preferences]]`; Jev, EMBEDDED_BOLD). The screen
/// is tall enough that both reach iTerm2's largest size, 100 points, so
/// the frames' sizes do not decide it. The control: the stack without the
/// badge's role draws it in Fira Code, and differs.
#[test]
fn the_badge_is_drawn_in_its_own_face() {
    use baud_render::FaceRole;
    const TALL: WinSize = WinSize {
        ws_col: 60,
        ws_row: 50,
    };
    let fira = font("FiraCode-Regular.ttf");
    let bold = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"
    ))
    .unwrap();
    let records = |faces: &[Face<'_>], roles: &[FaceRole]| {
        let mut owned = Owned::new(&TALL);
        let mut t = Term::new(owned.storage(), &TALL);
        feed(&mut t, b"\x1b[?25l");
        let mut r = Renderer::new(
            Stack::with_roles(faces, roles).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(TALL.ws_col)),
            Rows::of_index(usize::from(TALL.ws_row)),
        )
        .unwrap();
        r.set_badge(Some(BadgeLook {
            style: FaceStyle::Bold,
            ..look("ab")
        }));
        let mut pixels = Vec::new();
        paint(&mut r, &mut t, &mut pixels, Damage::Full);
        let fw = i32::try_from(r.pixel_size().0.value()).unwrap();
        r.badge()
            .iter()
            .map(|i| (i32::from(i.x.0) - fw, i.y.0, i.w.0, i.h.0))
            .collect::<Vec<_>>()
    };
    let fira_faces = [Face::parse(&fira).unwrap(), Face::parse(&bold).unwrap()];
    let bold_faces = [Face::parse(&bold).unwrap()];
    let own = records(&fira_faces, &[FaceRole::Fallback, FaceRole::Badge]);
    let want = records(&bold_faces, &[FaceRole::Fallback]);
    assert_eq!(want.len(), 2, "the control: two glyphs");
    assert_eq!(own, want, "the badge in its own face");
    let text = records(&fira_faces, &[FaceRole::Fallback, FaceRole::Fallback]);
    assert_ne!(text, want, "the control: in the text's face it differs");
}
