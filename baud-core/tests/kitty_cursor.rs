//! **The cursor after a kitty placement is where kitty leaves it**: right by
//! the placement's columns and down by its rows less one, so it ends on the
//! placement's last row (`handle_put_command`, `[[source: kitty-graphics-c]]`),
//! then to the start of the next row past the right edge, and scrolling
//! past the bottom (`[[source: kitty-screen-c]]`). The text says "down by
//! the number of rows" (`[[source: kitty-graphics]]`), a row more than kitty
//! moves; kitty's graphics protocol is defined by kitty's behaviour, and
//! kitten icat's output is written against it.
use baud_core::*;
use baud_domain::{ImageBudget, ImagePx};

/// `bytes` fed to a `cols` x `rows` terminal with `cell`-pixel cells, in
/// reads of `read`: the cursor and the placements made.
fn fed(
    (cols, rows): (u16, u16),
    cell: (u16, u16),
    bytes: &[u8],
    read: ReadLen,
) -> ((usize, usize), usize) {
    let ws = WinSize {
        ws_row: rows,
        ws_col: cols,
    };
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(1 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(cell.0), ImagePx::of(cell.1));
    for piece in bytes.chunks(read.index()) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        while t.decode_step(baud_domain::DecodeBudget::ALL) == baud_domain::DecodeProgress::More {}
        t.clear_reply();
    }
    let (r, c) = t.cursor();
    ((r.index(), c.index()), t.images().placements().count())
}

/// RFC 4648 base64, padded.
fn b64(data: &[u8]) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let n = q
            .iter()
            .enumerate()
            .fold(0u32, |w, (i, &b)| w | u32::from(b) << (16 - 8 * i));
        for k in 0..4 {
            out.push(match k <= q.len() {
                true => A[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()],
                false => b'=',
            });
        }
    }
    out
}

/// **kitty's cursor after an `a=T` of `w` by `h` pixels at `(row, col)`**
/// of an 80x24 screen with `cw` by `ch` cells, written out from its source:
/// `update_dest_rect`'s columns and rows when neither is given, the pixels
/// rounded up to whole cells; `handle_put_command`'s `x += cols` and
/// `y += rows - 1`; and screen_handle_graphics_command's wrap to the next
/// row's column 0 at the right edge and scroll at the bottom, which leaves
/// the cursor on the last row.
fn kitty(
    (row, col): (usize, usize),
    (cw, ch): (usize, usize),
    (w, h): (usize, usize),
) -> (usize, usize) {
    let (cols, rows) = (w.div_ceil(cw), h.div_ceil(ch));
    let (mut y, mut x) = (row + rows - 1, col + cols);
    if x >= 80 {
        x = 0;
        y += 1;
    }
    (y.min(23), x)
}

/// **L1: kitten icat's own stream**, `tests/recordings/kitty/icat-logo-dark-140.raw`:
/// `CR`, `CSI 55 C`, an `a=T` of 140x97 with no `C`, its frames, `CR LF`.
/// In 14x32 cells the placement is 10 columns by 4 rows at column 55, so
/// the cursor goes to row 3, column 65, and the `CR LF` to row 4, column 0,
/// kitty's own result for this stream; by the text's rows it went to row 4
/// and then row 5. Whole, and in the tty's 1,024-byte reads.
#[test]
fn kitten_icats_stream_leaves_the_cursor_where_kitty_does() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/recordings/kitty/icat-logo-dark-140.raw"
    ))
    .expect("the recording");
    let a_t = bytes
        .windows(6)
        .position(|w| w == b"\x1b_Ga=T")
        .expect("the premise: an a=T");
    assert!(
        bytes[..a_t].ends_with(b"\r\x1b[55C"),
        "the premise: the cursor put at column 55"
    );
    let st = a_t
        + bytes[a_t..]
            .windows(2)
            .position(|w| w == b"\x1b\\")
            .expect("its ST")
        + 2;
    for read in [bytes.len(), 1024] {
        assert_eq!(
            fed((120, 40), (14, 32), &bytes[..st], ReadLen::from_index(read)),
            ((3, 65), 1),
            "after the a=T, reads of {read}"
        );
        assert_eq!(
            fed((120, 40), (14, 32), &bytes, ReadLen::from_index(read)).0,
            (4, 0),
            "after the CR LF, reads of {read}"
        );
    }
}

/// **L2: one 1x1 image at the origin** in 10x20 cells: one column, one row,
/// so the cursor goes right one and down none, to (0, 1), as kitty leaves
/// it. By the text's rows it went to (1, 1). `C=1` moves it neither.
#[test]
fn a_one_cell_image_moves_the_cursor_right_and_not_down() {
    let mut s = b"\x1b_Ga=T,f=24,s=1,v=1;".to_vec();
    s.extend(b64(&[9, 9, 9]));
    s.extend_from_slice(b"\x1b\\");
    assert_eq!(fed((80, 24), (10, 20), &s, ReadLen::of(&s)), ((0, 1), 1));
    let mut c = b"\x1b_Ga=T,f=24,s=1,v=1,C=1;".to_vec();
    c.extend(b64(&[9, 9, 9]));
    c.extend_from_slice(b"\x1b\\");
    assert_eq!(
        fed((80, 24), (10, 20), &c, ReadLen::of(&c)),
        ((0, 0), 1),
        "C=1"
    );
}

proptest::proptest! {
    /// **L3: any raw `a=T`, any cell size, anywhere on an 80x24 screen,
    /// leaves the cursor where kitty's source puts it** ([`kitty`]),
    /// wrapping past the right edge and scrolling past the bottom included.
    #[test]
    fn any_placement_leaves_the_cursor_where_kittys_source_puts_it(
        cw in 1u16..=20,
        ch in 1u16..=40,
        w in 1usize..=100,
        h in 1usize..=100,
        row in 0usize..24,
        col in 0usize..80,
    ) {
        let mut s = format!("\x1b[{};{}H\x1b_Ga=T,f=24,s={w},v={h};", row + 1, col + 1).into_bytes();
        s.extend(b64(&vec![7u8; w * h * 3]));
        s.extend_from_slice(b"\x1b\\");
        let (at, placed) = fed((80, 24), (cw, ch), &s, ReadLen::of(&s));
        proptest::prop_assert_eq!(placed, 1);
        proptest::prop_assert_eq!(
            at,
            kitty((row, col), (usize::from(cw), usize::from(ch)), (w, h))
        );
    }
}

/// **kitty's cursor after an `a=T` of `w` by `h` pixels at `(row, col)`
/// with one count given**, `c` (and `r` 0) or `r` (and `c` 0), and the
/// cell offset `X`, `Y`, written out from its source: `handle_put_command`
/// clamps each offset to the cell less one, and `update_dest_rect` takes
/// the given count's pixels plus that axis's offset, scales them by the
/// source's aspect in doubles and rounds up to whole cells, the other
/// axis's offset not counted; then the move of [`kitty`].
fn kitty_one_count(
    (row, col): (usize, usize),
    (cw, ch): (usize, usize),
    (w, h): (usize, usize),
    (c, r): (usize, usize),
    (x_off, y_off): (usize, usize),
) -> (usize, usize) {
    let (x_off, y_off) = (x_off.min(cw - 1), y_off.min(ch - 1));
    let (cols, rows) = match (c, r) {
        // The count is the box's size over the cell's, rounded up: exact
        // here, since every value is far below 2^53.
        (c, 0) => (c, ((cw * c + x_off) * h).div_ceil(w * ch)),
        (0, r) => (((ch * r + y_off) * w).div_ceil(h * cw), r),
        _ => unreachable!("one count"),
    };
    let (mut y, mut x) = (row + rows - 1, col + cols);
    if x >= 80 {
        x = 0;
        y += 1;
    }
    (y.min(23), x)
}

/// An `a=T` of a `w` by `h` raw image with `keys` at the origin of an
/// 80x24 screen with `cell`-pixel cells: the cursor and the placements.
fn one_count(cell: (u16, u16), (w, h): (usize, usize), keys: &str) -> ((usize, usize), usize) {
    let mut s = format!("\x1b_Ga=T,f=24,s={w},v={h},{keys};").into_bytes();
    s.extend(b64(&vec![7u8; w * h * 3]));
    s.extend_from_slice(b"\x1b\\");
    fed((80, 24), cell, &s, ReadLen::of(&s))
}

/// **L4: `c` alone counts its rows as kitty does**, in 10x20 cells: the
/// columns' pixels plus `X` at the source's aspect, rounded up, `Y` not
/// counted. A 4x40 image at `c=1,X=9` is (10 + 9) * 10 = 190 pixels tall
/// to kitty, ten rows, where the 1-pixel box less `X` draws one row; a
/// 10x10 image at `c=1,Y=15` is one row, where the image drawn 15 pixels
/// down reaches a second; a 25x17 image at `c=3` is 20.4 pixels, two rows,
/// where the drawn height rounds to 20.
#[test]
fn c_alone_counts_its_rows_from_the_columns_and_x_as_kitty_does() {
    assert_eq!(one_count((10, 20), (4, 40), "c=1,X=9"), ((9, 1), 1), "X");
    assert_eq!(one_count((10, 20), (10, 10), "c=1,Y=15"), ((0, 1), 1), "Y");
    assert_eq!(
        one_count((10, 20), (25, 17), "c=3"),
        ((1, 3), 1),
        "rounded up"
    );
}

/// **L5: `r` alone counts its columns as kitty does**, in 10x20 cells: the
/// rows' pixels plus `Y` at the source's aspect, rounded up, `X` not
/// counted. A 10x10 image at `r=1,Y=19` is 39 pixels wide to kitty, four
/// columns, where the 1-pixel box less `Y` draws one; at `r=1,X=9` it is
/// two, where the image drawn 9 pixels right reaches a third.
#[test]
fn r_alone_counts_its_columns_from_the_rows_and_y_as_kitty_does() {
    assert_eq!(one_count((10, 20), (10, 10), "r=1,Y=19"), ((0, 4), 1), "Y");
    assert_eq!(one_count((10, 20), (10, 10), "r=1,X=9"), ((0, 2), 1), "X");
}

proptest::proptest! {
    /// **L6: any raw `a=T` with one count**, any cell size, any offset
    /// (past the cell included, which kitty clamps), anywhere on an 80x24
    /// screen, leaves the cursor where kitty's source puts it
    /// ([`kitty_one_count`]).
    #[test]
    fn a_placement_with_one_count_leaves_the_cursor_where_kittys_source_puts_it(
        cw in 1u16..=20,
        ch in 1u16..=40,
        w in 1usize..=100,
        h in 1usize..=100,
        n in 1usize..=12,
        by_cols in proptest::bool::ANY,
        x_off in 0usize..=24,
        y_off in 0usize..=44,
        row in 0usize..24,
        col in 0usize..80,
    ) {
        let (c, r) = match by_cols {
            true => (n, 0),
            false => (0, n),
        };
        let key = match by_cols {
            true => format!("c={n}"),
            false => format!("r={n}"),
        };
        let mut s = format!(
            "\x1b[{};{}H\x1b_Ga=T,f=24,s={w},v={h},{key},X={x_off},Y={y_off};",
            row + 1,
            col + 1
        )
        .into_bytes();
        s.extend(b64(&vec![7u8; w * h * 3]));
        s.extend_from_slice(b"\x1b\\");
        let (at, placed) = fed((80, 24), (cw, ch), &s, ReadLen::of(&s));
        proptest::prop_assert_eq!(placed, 1);
        proptest::prop_assert_eq!(
            at,
            kitty_one_count(
                (row, col),
                (usize::from(cw), usize::from(ch)),
                (w, h),
                (c, r),
                (x_off, y_off),
            )
        );
    }
}
