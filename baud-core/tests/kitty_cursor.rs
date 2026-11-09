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
