//! iTerm2's cursor guide on the CPU path: the band over the cursor's row,
//! its lines, the block it leaves out, the text over it, and the rows it
//! repaints (`[[source: iterm2-7ce037e-text-drawing-helper]]`).

mod support;

use baud_core::{CacheGeometry, Depth, Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{Argb, Blend, CURSOR_GUIDE, Damage, GuideLook, Renderer, Stack, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 30,
    ws_row: 10,
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

use baud_render::PxSize;

/// A frame on the CPU path, kept between calls so incremental damage
/// repaints over the last.
fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, pixels: &mut Vec<u32>, damage: Damage) {
    let (w, h) = r.pixel_size();
    pixels.resize(px(w) * px(h), 0);
    let mut target = Target::new(pixels, w, w, h).unwrap();
    r.render(t, &mut target, damage);
}

fn look(line: u32) -> Option<GuideLook> {
    Some(GuideLook {
        colour: CURSOR_GUIDE,
        line: Px::of(line),
    })
}

/// A colour filled over an opaque page at `a`, as the CPU's native blend
/// and the GPU's do.
fn over(colour: Argb, page: Argb, a: u8) -> u32 {
    colour
        .with_alpha(Chan::from_u8(0xFF))
        .mix(page, Chan::from_u8(a))
        .0
        & 0x00FF_FFFF
}

/// **The guide's colour is iTerm2's default**, calibrated RGB (0.650,
/// 0.910, 1.000) at 0.25, in sRGB through the calibrated space's
/// conversion, and 0.25 of 255 to the nearest byte.
#[test]
fn the_guide_colour_is_iterm2s_default_in_srgb() {
    use baud_domain::themefile::gamut::{Space, to_srgb};
    let ([r, g, b], _) = to_srgb(Space::Calibrated, [0.650, 0.910, 1.000]);
    assert_eq!(
        (
            CURSOR_GUIDE.r().get(),
            CURSOR_GUIDE.g().get(),
            CURSOR_GUIDE.b().get()
        ),
        (r, g, b)
    );
    assert_eq!(CURSOR_GUIDE.a().get(), 64, "0.25 of 255, rounded");
}

/// **The guide fills the cursor's row and only it**: its middle at the
/// colour's alpha over the page, a line of `line` pixels at its top and its
/// bottom at the alpha filled twice, the block cursor's cell left as it was,
/// and a full block's pixel left in the text's colour, since the text is
/// drawn over it. The controls: without the guide no pixel differs, and
/// every other row is the same with it.
#[test]
fn the_guide_fills_the_cursor_row_under_the_text() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let page = Theme::dark().bg.with_alpha(Chan::from_u8(0xFF));
    let text = Theme::dark().fg.with_alpha(Chan::from_u8(0xFF));
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // The cursor at row 2, column 5, after five full blocks there, which
    // fill their cells in the text's colour; a steady block cursor.
    feed(
        &mut t,
        "\x1b[2 q\x1b[3;1H\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}".as_bytes(),
    );
    let mut r = renderer(&faces);
    let m = r.metrics();
    let (cw, ch) = (px(m.cell_w), px(m.cell_h));
    let (w, _) = r.pixel_size();
    let w = px(w);
    let (mut off, mut on) = (Vec::new(), Vec::new());
    paint(&mut r, &mut t, &mut off, Damage::Full);
    r.set_cursor_guide(look(2));
    paint(&mut r, &mut t, &mut on, Damage::Full);
    let at = |f: &[u32], x: usize, y: usize| f[y * w + x] & 0x00FF_FFFF;
    let top = 2 * ch;
    for y in 0..off.len() / w {
        if (top..top + ch).contains(&y) {
            continue;
        }
        for x in 0..w {
            assert_eq!(at(&on, x, y), at(&off, x, y), "row pixel ({x}, {y})");
        }
    }
    let x = 20 * cw + cw / 2;
    assert_eq!(
        at(&off, x, top + ch / 2),
        page.0 & 0x00FF_FFFF,
        "the control"
    );
    assert_eq!(
        at(&on, x, top + ch / 2),
        over(CURSOR_GUIDE, page, 64),
        "middle"
    );
    let twice = 112; // 255 - (191 * 191 + 127) / 255
    for y in [top, top + 1, top + ch - 2, top + ch - 1] {
        assert_eq!(
            at(&on, x, y),
            over(CURSOR_GUIDE, page, twice),
            "line at {y}"
        );
    }
    assert_eq!(
        at(&on, x, top + 2),
        over(CURSOR_GUIDE, page, 64),
        "under the line"
    );
    assert_eq!(
        at(&on, 6 * cw, top + ch / 2),
        at(&on, w - 1, top + ch / 2),
        "full width"
    );
    for y in top..top + ch {
        for x in 5 * cw..6 * cw {
            assert_eq!(at(&on, x, y), at(&off, x, y), "the block's cell ({x}, {y})");
        }
    }
    let cell: Vec<(usize, usize)> = (top..top + ch)
        .flat_map(|y| (0..cw).map(move |x| (x, y)))
        .collect();
    let solid = *cell
        .iter()
        .find(|&&(x, y)| at(&off, x, y) == text.0 & 0x00FF_FFFF)
        .expect("the control: a block pixel in the text's colour");
    assert_eq!(
        at(&on, solid.0, solid.1),
        text.0 & 0x00FF_FFFF,
        "text over it"
    );
}

/// **No guide while the cursor is hidden, and while the window looks back
/// into the history the guide stays on the cursor's line for as long as
/// that line is in view**: iTerm2's CPU path hides it with the cursor
/// (DECTCEM; Jev's lean, WITH_CURSOR), and both its paths draw it on the
/// cursor's line, shifted with the scroll, while that line is inside the
/// window (`drawCursrGuideIfNeededInRect`, iTerm2's spelling, `[[source:
/// iterm2-7ce037e-text-drawing-helper]]`). The controls: shown again, it
/// draws, and at the live screen it draws on the cursor's row.
#[test]
fn the_guide_hides_with_the_cursor() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[3;1Hx\x1b[?25l");
    let mut r = renderer(&faces);
    let (mut off, mut on) = (Vec::new(), Vec::new());
    paint(&mut r, &mut t, &mut off, Damage::Full);
    r.set_cursor_guide(look(1));
    paint(&mut r, &mut t, &mut on, Damage::Full);
    assert_eq!(on, off, "hidden with the cursor");
    feed(&mut t, b"\x1b[?25h");
    paint(&mut r, &mut t, &mut on, Damage::Full);
    assert_ne!(on, off, "the control: shown with it");
    // Fifteen lines into a ten-row screen, six of them into the history,
    // then the cursor on row 5: `n` lines back its line is window row
    // 4 + n, in view for n up to 5 and gone at 6.
    let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &WS);
    for i in 0..15 {
        feed(&mut t, format!("line {i}\r\n").as_bytes());
    }
    feed(&mut t, b"\x1b[5;4H");
    let back = |r: &mut Renderer<'_, '_>, t: &mut Term<'_>, n: usize| {
        r.fit(t);
        let (w, h) = r.pixel_size();
        let mut pixels = vec![0u32; px(w) * px(h)];
        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
        r.render_at(t, Rows::of_count(n), &mut target, Damage::Full);
        pixels
    };
    let ch = px(renderer(&faces).metrics().cell_h);
    let width = px(renderer(&faces).pixel_size().0);
    for (n, row, why) in [
        (0, Some(4), "the control: at the live screen"),
        (2, Some(6), "two lines back, the cursor's line in view"),
        (5, Some(9), "five lines back, on the window's last row"),
        (6, None, "six lines back, the cursor's line out of view"),
    ] {
        let mut r = renderer(&faces);
        let bare = back(&mut r, &mut t, n);
        r.set_cursor_guide(look(1));
        let guided = back(&mut r, &mut t, n);
        let changed: Vec<usize> = (0..bare.len() / width)
            .filter(|&y| bare[y * width..(y + 1) * width] != guided[y * width..(y + 1) * width])
            .collect();
        let want: Vec<usize> = row.map_or(Vec::new(), |row| (row * ch..(row + 1) * ch).collect());
        assert_eq!(changed, want, "{why}");
    }
    // DECSASD puts the cursor on the status line, below the main display
    // and no line of the history: in view whatever the window looks back
    // on, so the guide is on its row three lines back as at the live screen.
    feed(&mut t, b"\x1b[2$~\x1b[1$}AB");
    for n in [0, 3] {
        let mut r = renderer(&faces);
        let bare = back(&mut r, &mut t, n);
        r.set_cursor_guide(look(1));
        let guided = back(&mut r, &mut t, n);
        let changed: Vec<usize> = (0..bare.len() / width)
            .filter(|&y| bare[y * width..(y + 1) * width] != guided[y * width..(y + 1) * width])
            .collect();
        assert_eq!(
            changed,
            (10 * ch..11 * ch).collect::<Vec<_>>(),
            "the status line, {n} back"
        );
    }
}

/// **The guide repaints the rows it leaves and reaches**: a cursor moved to
/// another row, the guide turned off, and its look changed, each drawn on
/// incremental damage, equal a full repaint of the same frame.
#[test]
fn the_guide_moves_on_incremental_damage() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[3;1Hx");
    let mut r = renderer(&faces);
    let mut kept = Vec::new();
    r.set_cursor_guide(look(1));
    paint(&mut r, &mut t, &mut kept, Damage::Full);
    let steps: [(&[u8], Option<GuideLook>); 4] = [
        (b"\x1b[7;9H", look(1)),
        (b"", look(2)),
        (b"\x1b[2;4H", look(2)),
        (b"", None),
    ];
    for (i, (bytes, guide)) in steps.into_iter().enumerate() {
        feed(&mut t, bytes);
        r.set_cursor_guide(guide);
        paint(&mut r, &mut t, &mut kept, Damage::Incremental);
        let mut whole = Vec::new();
        let mut fresh = renderer(&faces);
        fresh.set_cursor_guide(guide);
        paint(&mut fresh, &mut t, &mut whole, Damage::Full);
        assert!(kept == whole, "step {i}: incremental differs from full");
    }
}
