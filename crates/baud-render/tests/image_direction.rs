//! **A picture after a Hebrew caption sits after it**: on a row drawn right
//! to left a picture's box is placed mirrored, `cols - (col + w)` from the
//! left, by the direction of the row it is anchored on, and its pixels are
//! not flipped -- terminal-wg's "images must not be flipped by default in
//! RTL mode ... In paragraphs of RTL direction, the overall layout should
//! be the mirrored version of the LTR one by default"
//! (`[[source: terminal-wg-bidi-misc]]`), the anchor being where the
//! picture is in the text, UAX #9 3.2's object (`[[source: uax9-bidi]]`).
//! kitty's placeholders are characters, so a strip is placed over its run's
//! cells where the order draws them.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, ImageBudget, ImagePx, ReadLen, Rows};
use baud_font::Face;
use baud_render::Renderer;
use support::{WS, font, renderer};

/// A sixel `w` by `h` pixels: the left half red, the right half blue.
fn sixel(w: usize, h: usize) -> Vec<u8> {
    let mut s = format!("\x1bP0;1;0q\"1;1;{w};{h}#1;2;90;10;10#2;2;10;20;90").into_bytes();
    for _ in 0..h.div_ceil(6) {
        s.extend_from_slice(format!("#1!{}~#2!{}~-", w / 2, w - w / 2).as_bytes());
    }
    s.extend_from_slice(b"\x1b\\");
    s
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

const HEBREW: &str = "\u{5D0}\u{5D1}\u{5D2} ";

/// Each placement's `(x, y, w)` in pixels, after `bytes`, for a window
/// `back` lines up; and the cell's width.
fn placed(bytes: &[u8], back: usize) -> (Vec<(i64, i64, i64)>, i64) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r: Renderer<'_, '_> = renderer(&faces);
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 20));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(
        ImagePx::of(u16::try_from(cw).unwrap()),
        ImagePx::of(u16::try_from(ch).unwrap()),
    );
    feed(&mut t, bytes);
    r.set_images(&t, Rows::of_count(back));
    let out = r
        .image_placements()
        .iter()
        .map(|p| (i64::from(p.x), i64::from(p.y), i64::from(p.w)))
        .collect();
    (out, i64::from(cw))
}

/// The grid's width in pixels.
fn grid_w(cw: i64) -> i64 {
    i64::from(WS.ws_col) * cw
}

/// **On a Hebrew line the picture is placed mirrored**: anchored at column
/// 4 after a four-cell caption, its box is drawn `cols - (4 + w)` from the
/// left, after the caption in reading order. The control: after a Latin
/// caption it is at column 4.
#[test]
fn a_picture_after_a_hebrew_caption_is_placed_mirrored() {
    let img = sixel(40, 24);
    let (p, cw) = placed(&[HEBREW.as_bytes(), &img].concat(), 0);
    assert_eq!(p.len(), 1, "one placement");
    let (x, _, w) = p[0];
    assert_eq!(w, 40);
    assert_eq!(x, grid_w(cw) - 4 * cw - w, "mirrored by its anchor row");
    let (p, cw) = placed(&[b"abc ".as_slice(), &img].concat(), 0);
    assert_eq!(p[0].0, 4 * cw, "the control: a Latin caption");
}

/// **The pixels are not flipped**: the texel columns the placement samples
/// run left to right, its first drawn column the picture's first.
#[test]
fn a_mirrored_picture_is_not_flipped() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r: Renderer<'_, '_> = renderer(&faces);
    let m = r.metrics();
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 20));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(
        ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
        ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
    );
    feed(&mut t, &[HEBREW.as_bytes(), &sixel(40, 24)].concat());
    r.set_images(&t, Rows::ZERO);
    let p = r.image_placements()[0];
    let (xs, _) = r.image_map(&p);
    assert!(!xs.is_empty());
    assert!(xs.windows(2).all(|w| w[0] <= w[1]), "left to right: {xs:?}");
}

/// **The anchor row places the whole box**: a picture taller than its row,
/// the rows under it empty and left to right, is one box, mirrored; and it
/// stays mirrored once its anchor line has scrolled into the history while
/// its lower rows are still on screen.
#[test]
fn the_anchor_row_places_the_whole_box_after_it_scrolls_off() {
    let img = sixel(40, 120);
    let (p, cw) = placed(&[HEBREW.as_bytes(), &img].concat(), 0);
    let (x, _, w) = p[0];
    assert_eq!(x, grid_w(cw) - 4 * cw - w);
    let rows = usize::from(WS.ws_row);
    let scrolled = [
        HEBREW.as_bytes(),
        &img,
        format!("\x1b[{rows};1H\n").as_bytes(),
    ]
    .concat();
    let (p, cw) = placed(&scrolled, 0);
    assert_eq!(p.len(), 1);
    let (x, y, w) = p[0];
    assert!(
        y < 0,
        "the premise: the anchor row is above the window, {y}"
    );
    assert_eq!(x, grid_w(cw) - 4 * cw - w, "still mirrored");
}

/// **Looking back, the row read in the window shown**: the picture's row
/// one line into the history, the window one line up, is mirrored.
#[test]
fn a_window_looking_back_mirrors_by_the_row_it_shows() {
    let rows = usize::from(WS.ws_row);
    let bytes = [
        HEBREW.as_bytes(),
        &sixel(40, 12),
        format!("\x1b[{rows};1H\n").as_bytes(),
    ]
    .concat();
    let (p, cw) = placed(&bytes, 1);
    let (x, y, w) = p[0];
    assert_eq!(y, 0, "the premise: the anchor row is the window's first");
    assert_eq!(x, grid_w(cw) - 4 * cw - w);
}

/// **With BDSM reset a right-to-left path mirrors it**, the row drawn in
/// reverse; without the path the row is drawn as stored, and so is the
/// picture.
#[test]
fn with_bdsm_reset_a_right_to_left_path_mirrors_the_picture() {
    let img = sixel(40, 12);
    let (p, cw) = placed(&[b"\x1b[8l\x1b[2 kabc ".as_slice(), &img].concat(), 0);
    assert_eq!(p[0].0, grid_w(cw) - 4 * cw - p[0].2);
    let (p, cw) = placed(
        &[b"\x1b[8l".as_slice(), HEBREW.as_bytes(), &img].concat(),
        0,
    );
    assert_eq!(p[0].0, 4 * cw, "BDSM reset, no path: stored");
}

/// **A table row keeps its columns, and its picture its place**: a row
/// holding a Box Drawing character is laid out left to right whatever its
/// cells hold, so a picture on it is not mirrored.
#[test]
fn a_picture_on_a_table_row_keeps_its_place() {
    // A Hebrew cell first, so the row's first column is in a cell that
    // runs right to left.
    let (p, cw) = placed(
        &["\u{5D0}\u{5D1}\u{2502}cd".as_bytes(), &sixel(40, 12)].concat(),
        0,
    );
    assert_eq!(p[0].0, 5 * cw);
}

/// A 4 by 4 red picture's RGBA, base64, for kitty's `a=t`.
fn kitty_pixels() -> Vec<u8> {
    let px: Vec<u8> = (0..4 * 4).flat_map(|_| [200u8, 30, 30, 255]).collect();
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = Vec::new();
    for c in px.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, &b)| a | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            o.push(match i <= c.len() {
                true => T[usize::try_from(n >> (18 - 6 * i) & 63).unwrap_or(0)],
                false => b'=',
            });
        }
    }
    o
}

/// **A kitty picture placed at the cursor, `a=p`, is mirrored as a Sixel
/// is**: after a Hebrew caption its box is drawn `cols - (4 + w)` from the
/// left; after a Latin one at column 4.
#[test]
fn a_kitty_picture_after_a_hebrew_caption_is_placed_mirrored() {
    let setup = |caption: &str| {
        let mut v = caption.as_bytes().to_vec();
        v.extend_from_slice(b"\x1b_Ga=t,f=32,s=4,v=4,i=9,q=2;");
        v.extend_from_slice(&kitty_pixels());
        v.extend_from_slice(b"\x1b\\\x1b_Ga=p,i=9,c=2,r=1,q=2\x1b\\");
        v
    };
    let (latin, cw) = placed(&setup("abc "), 0);
    assert_eq!(latin.len(), 1, "one placement");
    let dx = latin[0].0 - 4 * cw;
    assert!((0..2 * cw).contains(&dx), "the control: column 4, {dx} in");
    let (p, cw) = placed(&setup(HEBREW), 0);
    assert_eq!(p.len(), 1, "one placement");
    let (x, _, w) = p[0];
    // The box as placed, `dx` into its cells, mirrored whole.
    assert_eq!(x, grid_w(cw) - (4 * cw + dx) - w, "mirrored");
}

/// The pinned list's diacritic for `n`.
fn d(n: usize) -> char {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/research/sources/kitty-placeholder-diacritics.txt"
    );
    let src = std::fs::read_to_string(path).unwrap();
    let line = src
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .nth(n)
        .unwrap();
    char::from_u32(u32::from_str_radix(line.split(';').next().unwrap(), 16).unwrap()).unwrap()
}

/// **A kitty placeholder strip is placed over its cells where they are
/// drawn**: four placeholder cells after a Hebrew word, logical columns 4
/// to 7, are a left-to-right run the right-to-left paragraph draws at
/// visual columns `cols - 8` to `cols - 5`, and the strip is drawn there,
/// and the control: after a Latin word, at column 4.
#[test]
fn a_placeholder_strip_is_drawn_over_its_cells_where_they_are_drawn() {
    let b64 = kitty_pixels();
    let setup = |caption: &str| {
        let mut v = b"\x1b_Ga=t,f=32,s=4,v=4,i=7,q=2;".to_vec();
        v.extend_from_slice(&b64);
        v.extend_from_slice(b"\x1b\\\x1b_Ga=p,U=1,i=7,c=4,r=1,q=2\x1b\\");
        v.extend_from_slice(caption.as_bytes());
        v.extend_from_slice(b"\x1b[38;5;7m");
        for c in 0..4 {
            v.extend_from_slice(format!("\u{10EEEE}{}{}", d(0), d(c)).as_bytes());
        }
        v.extend_from_slice(b"\x1b[0m");
        v
    };
    // The picture is fitted into the run's box, so it starts `dx` into it.
    let (latin, cw) = placed(&setup("abc "), 0);
    assert_eq!(latin.len(), 1, "one strip");
    let dx = latin[0].0 - 4 * cw;
    assert!((0..4 * cw).contains(&dx), "the control: column 4, {dx} in");
    let (p, cw) = placed(&setup(HEBREW), 0);
    assert_eq!(p.len(), 1, "one strip");
    let cols = i64::from(WS.ws_col);
    assert_eq!(p[0].0, (cols - 8) * cw + dx, "over its cells as drawn");
}

/// **On the leaving row too**: the same strip after a Hebrew word, its
/// row scrolled just into the history, is the line a smooth scroll slides
/// out above the window, and is placed over its cells as that line was
/// drawn on screen, one row above the top; after a Latin word at column
/// 4, the control.
#[test]
fn a_placeholder_strip_on_the_leaving_row_is_drawn_where_its_cells_are() {
    let b64 = kitty_pixels();
    let setup = |caption: &str| {
        let mut v = b"\x1b_Ga=t,f=32,s=4,v=4,i=7,q=2;".to_vec();
        v.extend_from_slice(&b64);
        v.extend_from_slice(b"\x1b\\\x1b_Ga=p,U=1,i=7,c=4,r=1,q=2\x1b\\");
        v.extend_from_slice(caption.as_bytes());
        v.extend_from_slice(b"\x1b[38;5;7m");
        for c in 0..4 {
            v.extend_from_slice(format!("\u{10EEEE}{}{}", d(0), d(c)).as_bytes());
        }
        v.extend_from_slice(format!("\x1b[0m\x1b[{};1H\n", WS.ws_row).as_bytes());
        v
    };
    let (latin, cw) = placed(&setup("abc "), 0);
    assert_eq!(latin.len(), 1, "one strip");
    assert!(latin[0].1 < 0, "the premise: on the leaving row, {latin:?}");
    let dx = latin[0].0 - 4 * cw;
    assert!((0..4 * cw).contains(&dx), "the control: column 4, {dx} in");
    let (p, cw) = placed(&setup(HEBREW), 0);
    assert_eq!(p.len(), 1, "one strip");
    let cols = i64::from(WS.ws_col);
    assert_eq!(p[0].0, (cols - 8) * cw + dx, "over its cells as drawn");
}
