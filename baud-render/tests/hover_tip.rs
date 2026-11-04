//! **The hover tip baud draws in the frame**: a word's meaning on DEC's
//! indicator line, laid out above the line (`tip::tip_look`) and drawn on
//! the CPU path, which the Linux driver draws with, as the macOS paths draw
//! it (`baud-present-macos/tests/cells.rs`, the arms); and on Linux a
//! light's on the lights' own line, above a line at the bottom and below
//! one at the top (`tip::tip_look_opening`).

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Blend, Damage, Glow, Led, Light, LineEdge, Opens, PxSize, Renderer, Shown, Stack, TIP_COLS,
    Target, Theme, TipLook, tip_look, tip_look_opening,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32], damage: Damage) {
    let (w, h) = r.pixel_size();
    let mut target = Target::new(px, w, w, h).unwrap();
    r.render(t, &mut target, damage);
}

/// The tip's lines as text, each trimmed, joined by a space.
fn words(look: &TipLook) -> String {
    look.lines
        .iter()
        .map(|f| {
            f.text
                .iter()
                .filter_map(|c| char::from_u32(c.value()))
                .collect::<String>()
                .trim()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// **A tip is its text wrapped at its words, right-aligned above the
/// line with a row between**: at 80 columns no line is wider than
/// `TIP_COLS` and its two spaces, every line the same width and ending at
/// the last column, the last on the row above the main display's last,
/// and the area the pointer may cross runs from its top down to the line.
/// The words read back whole and in order. At 30 columns it wraps at 28;
/// with fewer rows than lines it shows the first that fit and drops the
/// gap; with no room across it is none.
#[test]
fn a_tip_is_wrapped_right_aligned_and_above_the_line() {
    for led in Led::ALL {
        let look = tip_look(led.tip(), Cols::of_index(80), Rows::of_index(24)).unwrap();
        assert_eq!(words(&look), led.tip(), "{led:?}");
        let width = look.lines[0].text.len();
        assert!(width <= TIP_COLS + 2, "{led:?}: {width}");
        for (i, f) in look.lines.iter().enumerate() {
            assert_eq!(f.text.len(), width, "{led:?}: every line as wide");
            assert_eq!(f.col.index() + width, 80, "{led:?}: right-aligned");
            assert_eq!(f.row.index(), 23 - look.lines.len() + i, "{led:?}");
            assert_eq!(f.caret.index(), 80);
        }
        let last = look.lines.last().unwrap();
        assert_eq!(
            last.row,
            Row::from_index(22),
            "{led:?}: a row above the last"
        );
        assert_eq!(look.area.bottom, Row::from_index(24), "down to the line");
        assert_eq!(look.area.right, Col::at_index(80));
        assert!(
            look.area.holds(Row::from_index(23), Col::at_index(79)),
            "the gap"
        );
        assert!(!look.area.holds(
            Row::from_index(23),
            Col::at_index(look.area.left.index() - 1)
        ));
    }
    let narrow = tip_look(Led::Cs.tip(), Cols::of_index(30), Rows::of_index(24)).unwrap();
    assert_eq!(words(&narrow), Led::Cs.tip());
    assert!(narrow.lines.iter().all(|f| f.text.len() <= 30));
    let all = tip_look(Led::Cs.tip(), Cols::of_index(80), Rows::of_index(24)).unwrap();
    let n = all.lines.len();
    assert!(n >= 3, "{n}");
    let short = tip_look(Led::Cs.tip(), Cols::of_index(80), Rows::of_index(n - 1)).unwrap();
    assert_eq!(short.lines.len(), n - 1, "the first that fit");
    assert_eq!(short.lines[0].text, all.lines[0].text, "from the first");
    assert_eq!(short.lines[0].row, Row::from_index(0), "no gap");
    assert!(tip_look(Led::Cs.tip(), Cols::of_index(2), Rows::of_index(24)).is_none());
}

/// FNV-1a over a frame's pixel words.
fn digest(frame: &[u32]) -> u64 {
    frame.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &p| {
        p.to_le_bytes()
            .iter()
            .fold(h, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
    })
}

/// **The tip is drawn, moved and cleared without a trace, in the
/// selection's colours, the same pixels on every OS**: over the indicator
/// line with CD's word on it, CD's tip covers its cells and nothing past
/// its left edge, the gap row and the line are left as they were, an
/// incremental frame is the full one, OH's tip in its place leaves none of
/// CD's, and with none the frame is the plain one. A blank cell of the tip
/// is the selection's ground, the default foreground in reverse. The frame
/// with CD's tip is pinned, taken on macOS and required of every target:
/// the Linux driver draws with this path.
///
/// Perturbation: drop `tipped` from the convert's damage, and the
/// incremental frames keep what the last one drew.
#[test]
fn the_tip_is_drawn_moved_and_cleared_without_a_trace() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l\x1b[1$~some text\r\nand more");
    t.set_indicator_lamps(Some(baud_core::IndicatorLamps {
        cd: baud_core::ReadLamp::Unknown,
        ..baud_core::IndicatorLamps::DARK
    }));
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(25),
    )
    .unwrap();
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let mut plain = vec![0u32; n];
    paint(&mut r, &mut t, &mut plain, Damage::Full);
    let m = r.metrics();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let stride = usize::try_from(w.value()).unwrap();
    let cell = move |row: usize, col: usize| {
        (row * ch..(row + 1) * ch)
            .flat_map(move |y| (col * cw..(col + 1) * cw).map(move |x| y * stride + x))
    };
    let (cols, rows) = t.geometry();
    let cd = tip_look(Led::Cd.tip(), cols, rows).unwrap();
    let top = cd.area.top.index();
    let left = cd.area.left.index();
    let mut shown = plain.clone();
    r.set_tip(Some(cd.clone()));
    paint(&mut r, &mut t, &mut shown, Damage::Incremental);
    let differs = |a: &[u32], row: usize, col: usize| cell(row, col).any(|i| a[i] != plain[i]);
    assert!(
        differs(&shown, top, left) && differs(&shown, 22, 79),
        "the tip covers its cells"
    );
    assert!(
        !differs(&shown, top, left - 1),
        "and stops at its left edge"
    );
    assert!(
        (0..80).all(|c| !differs(&shown, 23, c) && !differs(&shown, 24, c)),
        "the gap row and the line are as they were"
    );
    // The last line's last cell is a blank of the box: the selection's
    // ground, the default foreground under reverse video.
    let fg = Theme::dark().fg.0 & 0x00FF_FFFF;
    assert!(
        cell(22, 79).all(|i| shown[i] & 0x00FF_FFFF == fg),
        "a blank of the tip is the selection's ground"
    );
    let mut full = vec![0u32; n];
    paint(&mut r, &mut t, &mut full, Damage::Full);
    assert!(full == shown, "an incremental frame is the full one");
    const PINNED: u64 = 0xd6c5_66e0_884e_214f;
    let got = digest(&full);
    assert_eq!(got, PINNED, "the tip's frame moved: 0x{got:016x}");
    // OH's tip in its place: nothing of CD's left.
    let oh = tip_look(Led::Oh.tip(), cols, rows).unwrap();
    r.set_tip(Some(oh));
    paint(&mut r, &mut t, &mut shown, Damage::Incremental);
    let mut full = vec![0u32; n];
    paint(&mut r, &mut t, &mut full, Damage::Full);
    assert!(full == shown, "moved without a trace");
    // Gone: the frame is the plain one again.
    r.set_tip(None);
    paint(&mut r, &mut t, &mut shown, Damage::Incremental);
    assert!(shown == plain, "nothing of the tip is left behind");
}

/// **Over a line above the main display a tip opens down**: the same
/// lines as opening up, right-aligned the same, its first on the main
/// display's second row, the first left as the gap, and the area from the
/// top down to its last line. With fewer rows than lines it shows the first
/// that fit from the top and drops the gap.
#[test]
fn a_tip_over_a_top_line_opens_down() {
    for led in Led::ALL {
        let (cols, rows) = (Cols::of_index(80), Rows::of_index(24));
        let up = tip_look(led.tip(), cols, rows).unwrap();
        let down = tip_look_opening(led.tip(), cols, rows, Opens::Down).unwrap();
        assert_eq!(words(&down), led.tip(), "{led:?}");
        assert_eq!(down.lines.len(), up.lines.len(), "{led:?}");
        for (i, (d, u)) in down.lines.iter().zip(&up.lines).enumerate() {
            assert_eq!((&d.text, d.col), (&u.text, u.col), "{led:?}: the same line");
            assert_eq!(d.row, Row::from_index(1 + i), "{led:?}: below the gap");
        }
        assert_eq!(down.area.top, Row::from_index(0), "{led:?}: from the line");
        assert_eq!(down.area.bottom.index(), 1 + down.lines.len(), "{led:?}");
        assert_eq!(
            (down.area.left, down.area.right),
            (up.area.left, up.area.right)
        );
        assert_eq!(tip_look_opening(led.tip(), cols, rows, Opens::Up), Some(up));
    }
    let all = tip_look(Led::Cs.tip(), Cols::of_index(80), Rows::of_index(24)).unwrap();
    let n = all.lines.len();
    let short = tip_look_opening(
        Led::Cs.tip(),
        Cols::of_index(80),
        Rows::of_index(n - 1),
        Opens::Down,
    )
    .unwrap();
    assert_eq!(short.lines.len(), n - 1, "the first that fit");
    assert_eq!(short.lines[0].row, Row::from_index(0), "no gap");
    assert_eq!(short.lines[0].text, all.lines[0].text);
}

/// **A light's tip is drawn above its own line and leaves the line as it
/// was, the same pixels on every OS**: on a 2x CPU frame with the lights
/// on a line at the bottom, RD lit, RD's tip covers its cells in the
/// main display's last rows but one, the gap row, and the line's own
/// pixels are those of the frame without it; an incremental frame is the
/// full one, and with the tip gone the frame is the plain one. The frame
/// with RD's tip is pinned, taken on macOS and required of every target:
/// the Linux driver draws with this path. On a line at the top the tip
/// opens down and the line is left the same.
#[test]
fn a_lights_tip_is_drawn_by_its_line_and_leaves_the_line_as_it_was() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    for edge in [LineEdge::Bottom, LineEdge::Top] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?25l$ make\r\nbuilding");
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(26.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(80),
            Rows::of_index(24),
        )
        .unwrap();
        r.set_lights_line(Some(edge), 2.0);
        r.set_lights(
            Shown {
                rd: Light::Lit,
                ..Shown::DARK
            },
            Glow::Full,
        );
        r.fit(&t);
        let (w, h) = r.whole_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let frame = |r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32], d| {
            let mut target = Target::new(px, w, w, h).unwrap();
            r.render(t, &mut target, d);
        };
        let mut plain = vec![0u32; n];
        frame(&mut r, &mut t, &mut plain, Damage::Full);
        assert!(
            r.lights_places().iter().all(|(_, p)| p.is_some()),
            "the control: every light on the line"
        );
        let stride = usize::try_from(w.value()).unwrap();
        let rows_of = |top: Px, height: Px| {
            let a = usize::try_from(top.value()).unwrap() * stride;
            a..a + usize::try_from(height.value()).unwrap() * stride
        };
        let line = rows_of(r.lights_top(), r.lights_height());
        let m = r.metrics();
        let (cw, ch) = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let grid_top = usize::try_from(r.grid_top().value()).unwrap();
        let cell = move |row: usize, col: usize| {
            (grid_top + row * ch..grid_top + (row + 1) * ch)
                .flat_map(move |y| (col * cw..(col + 1) * cw).map(move |x| y * stride + x))
        };
        let (cols, rows) = t.geometry();
        let opens = match edge {
            LineEdge::Bottom => Opens::Up,
            LineEdge::Top => Opens::Down,
        };
        let rd = tip_look_opening(Led::Rd.tip(), cols, rows, opens).unwrap();
        let first = rd.lines[0].row.index();
        let last = rd.lines.last().unwrap().row.index();
        let left = rd.area.left.index();
        let gap = match edge {
            LineEdge::Bottom => 23,
            LineEdge::Top => 0,
        };
        assert_eq!(
            (first, last),
            match edge {
                LineEdge::Bottom => (22 - (rd.lines.len() - 1), 22),
                LineEdge::Top => (1, rd.lines.len()),
            },
            "{edge:?}"
        );
        r.set_tip(Some(rd));
        let mut shown = plain.clone();
        frame(&mut r, &mut t, &mut shown, Damage::Incremental);
        let differs = |a: &[u32], row: usize, col: usize| cell(row, col).any(|i| a[i] != plain[i]);
        assert!(
            differs(&shown, first, left) && differs(&shown, last, 79),
            "{edge:?}: the tip covers its cells"
        );
        assert!(
            !differs(&shown, first, left - 1),
            "{edge:?}: and stops at its left"
        );
        assert!(
            (0..80).all(|c| !differs(&shown, gap, c)),
            "{edge:?}: the gap row is as it was"
        );
        assert!(
            shown[line.clone()] == plain[line.clone()],
            "{edge:?}: the line is as it was"
        );
        let mut full = vec![0u32; n];
        frame(&mut r, &mut t, &mut full, Damage::Full);
        assert!(
            full == shown,
            "{edge:?}: an incremental frame is the full one"
        );
        if edge == LineEdge::Bottom {
            const PINNED: u64 = 0xec7f_272b_9e7b_6fdd;
            let got = digest(&full);
            assert_eq!(got, PINNED, "the light's tip's frame moved: 0x{got:016x}");
        }
        r.set_tip(None);
        frame(&mut r, &mut t, &mut shown, Damage::Incremental);
        assert!(
            shown == plain,
            "{edge:?}: nothing of the tip is left behind"
        );
    }
}
