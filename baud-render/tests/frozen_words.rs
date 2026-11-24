//! **A window over history the core froze is drawn at its words, as one
//! whose table never fills is** (`wrap_words`, on by default): a display
//! line that joins two stored rows reads each cell through its own row's
//! table, the live one or a generation's (`baud_render::Words::Runs`).
//!
//! `[[source: dec-std-070]]`'s SGR, page 5-104, note 3: "In no way does the
//! selection of current rendition affect the rendition of characters already
//! entered into the display." Re-breaking a finished paragraph at its words
//! is baud's display of the stored rows, so it may not draw them otherwise
//! for the table having filled.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, DrawnRows, PxSize, Renderer, Reorder, Stack, Theme, WordWrap};
use support::{Canvas, font};

const WS: WinSize = WinSize {
    ws_row: 5,
    ws_col: 12,
};

/// The app's provisioning, whose table holds 65,536 renditions.
const BIG: WinSize = WinSize {
    ws_row: 150,
    ws_col: 400,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for c in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, c);
        t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
    }
}

fn text(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content {
            x if x.is_empty() => '.',
            x => x
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or('?'),
        })
        .collect()
}

/// The window `back` lines up, each line as the layout draws it.
fn drawn(t: &Term<'_>, back: Rows) -> Vec<String> {
    let mut o = Reorder::new();
    o.set_word_wrap(WordWrap::On);
    o.lay_out(t, back);
    let mut fresh = DrawnRows::default();
    let vp = o.orders().window_kept(t, back, &mut fresh);
    (0..vp.rows().index())
        .map(|y| text(vp.row(Row::from_index(y))))
        .collect()
}

/// `lines` lines of 12 cells, each cell in a background of its own, from
/// `first`.
fn flood(first: usize, lines: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for l in 0..lines {
        for x in 0..12usize {
            let b = (first + l * 12 + x).to_le_bytes();
            out.extend(format!("\x1b[0;48;2;{};{};77mq", b[0], b[1]).as_bytes());
        }
        out.extend(b"\x1b[0m\r\n");
    }
    out
}

/// **A coloured paragraph in history, re-broken at its words before the
/// table passes 7/8 and after**: a freeze freezes every history row that
/// names a rendition, and the paragraph is drawn at its words still.
#[test]
fn a_coloured_paragraph_in_history_wraps_at_its_words_after_seven_eighths() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    assert_eq!(t.styles().one().len(), 1024);
    let mut bytes = Vec::new();
    for i in 0..6 {
        bytes.extend(format!("h{i}\r\n").as_bytes());
    }
    bytes.extend(b"\x1b[31mthe quick brown fox jumps\x1b[0m\r\n");
    for i in 0..6 {
        bytes.extend(format!("g{i}\r\n").as_bytes());
    }
    feed(&mut t, &bytes);
    let back = Rows::of_count(t.looking_back(Rows::of_count(10_000)).index() - 5);
    let before = drawn(&t, back);
    // 80 lines of 12 cells each in a rendition of its own: 960 named.
    feed(&mut t, &flood(0, 80));
    feed(&mut t, b"\x1b]133;A\x07$ ");
    assert!(t.style_reclaims().rewrites > 0, "the control: it froze");
    let back = Rows::of_count(t.looking_back(Rows::of_count(10_000)).index() - 5);
    let after = drawn(&t, back);
    assert!(
        before.iter().any(|l| l == "the quick .."),
        "the control: re-broken before"
    );
    assert_eq!(before, after, "the paragraph drawn otherwise");
}

/// A paragraph of words, every letter its own foreground and background,
/// numbered from `first`.
fn paragraph(first: usize) -> Vec<u8> {
    let words = "aa bbbb cc ddddd ee ffff gg hhh iiii jj kkk llll mm nnnn oo pp qqq";
    let mut out = Vec::new();
    for (k, ch) in words.chars().enumerate() {
        match ch {
            ' ' => out.push(b' '),
            _ => {
                let b = (first + k).to_le_bytes();
                out.extend(
                    format!(
                        "\x1b[0;38;2;{};{};5;48;2;5;{};{}m{ch}",
                        b[0], b[1], b[1], b[0]
                    )
                    .as_bytes(),
                );
            }
        }
    }
    out.extend(b"\x1b[0m\r\n");
    out
}

fn renderer<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    let mut r = Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    r.set_word_wrap(WordWrap::On);
    r
}

/// **One paragraph whose stored rows are in two tables, drawn at its
/// words**: 840 renditions name the history, a paragraph of 50 letters each
/// in its own follows, and a line of 12 more takes the table past 7/8 while
/// the paragraph's first rows are off the screen and its last are on it.
/// The freeze freezes the first and not the last, so its display lines join
/// cells of a frozen row and a live one, and each is drawn in its own
/// rendition: the frame is the reference's, pixel for pixel, at every
/// window over it.
#[test]
fn a_display_line_joining_a_frozen_row_and_a_live_one_draws_each_in_its_own() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut small_owned = Owned::new(&WS);
    let mut small = Term::new(small_owned.storage(), &WS);
    let mut big_owned = Owned::new(&BIG);
    let mut big = Term::new(big_owned.storage(), &WS);
    let mut bytes = flood(0, 70);
    bytes.extend(paragraph(20_000));
    bytes.extend(flood(840, 1));
    for i in 0..6 {
        bytes.extend(format!("g{i}\r\n").as_bytes());
    }
    bytes.extend(b"\x1b]133;A\x07$ ");
    feed(&mut small, &bytes);
    feed(&mut big, &bytes);
    assert!(small.style_reclaims().rewrites > 0, "the control: it froze");
    assert_eq!(big.style_reclaims().rewrites, 0, "the reference froze");
    let deepest = small.looking_back(Rows::of_count(10_000)).index();
    let big_deepest = big.looking_back(Rows::of_count(10_000)).index();
    assert_eq!(deepest, big_deepest, "the two hold the same history");
    // The control: the paragraph's stored rows are in two tables, and the
    // window over them is drawn at its words.
    let (mut joined, mut rebroken) = (false, false);
    let (mut r, mut rr) = (renderer(&faces), renderer(&faces));
    let (mut c, mut rc) = (Canvas::for_renderer(&r), Canvas::for_renderer(&rr));
    for back in 1..=14usize {
        let back = Rows::of_count(back);
        let vp = small.view(back);
        let tables: Vec<bool> = (0..vp.rows().index())
            .map(|y| vp.styles(Row::from_index(y)).is_live())
            .collect();
        joined |= tables.contains(&true) && tables.contains(&false);
        let stored: Vec<String> = (0..vp.rows().index())
            .map(|y| text(vp.row(Row::from_index(y))))
            .collect();
        rebroken |= drawn(&small, back) != stored;
        assert_eq!(drawn(&small, back), drawn(&big, back), "laid out otherwise");
        let got = {
            let mut target = c.target();
            let seal = small.seal_writes();
            let _ = r.render_at(&small, seal, back, &mut target, Damage::Full);
            c.digest()
        };
        let want = {
            let mut target = rc.target();
            let seal = big.seal_writes();
            let _ = rr.render_at(&big, seal, back, &mut target, Damage::Full);
            rc.digest()
        };
        assert_eq!(
            got,
            want,
            "the window {} back drawn otherwise",
            back.index()
        );
    }
    assert!(
        joined,
        "the control: no window held a frozen row and a live one"
    );
    assert!(
        rebroken,
        "the control: no window was re-broken at its words"
    );
}

/// **Two SDS strings on a line the core froze are drawn apart**, each an
/// isolate of its direction as its rendition names it (`[[source: ecma48]]`
/// 8.3.114): the layout reads a cell's string through its own row's table.
/// Read through the live table, the cells name no string and the Hebrew is
/// one run, reversed whole.
#[test]
fn directed_strings_on_a_frozen_line_are_drawn_apart() {
    let mut small_owned = Owned::new(&WS);
    let mut small = Term::new(small_owned.storage(), &WS);
    let mut big_owned = Owned::new(&BIG);
    let mut big = Term::new(big_owned.storage(), &WS);
    let mut bytes = "go \x1b[2]\u{5D0}\u{5D1}\x1b[0]\x1b[2]\u{5D2}\u{5D3}\x1b[0] now\r\n"
        .as_bytes()
        .to_vec();
    bytes.extend(flood(0, 80));
    feed(&mut small, &bytes);
    feed(&mut big, &bytes);
    assert!(small.style_reclaims().rewrites > 0, "the control: it froze");
    let back = Rows::of_count(small.looking_back(Rows::of_count(10_000)).index());
    assert!(
        !small.view(back).styles(Row::ZERO).is_live(),
        "the control: the line is frozen"
    );
    let want = drawn(&big, back);
    assert_eq!(
        want[0], "go \u{5D1}\u{5D0}\u{5D3}\u{5D2} now.",
        "the control: the reference draws the strings apart"
    );
    assert_eq!(drawn(&small, back), want);
}
