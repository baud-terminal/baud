//! kitty's Unicode placeholders through the byte stream
//! (`[[source: kitty-graphics]]`, "Unicode placeholders"; note V, V19):
//! a virtual placement (`U=1`) made, answered and moving nothing; the
//! diacritics after U+10EEEE kept whole, at any read size; what a row of
//! placeholder cells resolves to, inheritance included; and which deletes
//! reach a virtual placement.
//!
//! The diacritics are read from the pinned list's codepoints
//! (`[[source: kitty-placeholder-diacritics]]`, from `[[source:
//! kitty-rowcolumn-diacritics]]`), never from the table compiled from
//! them, so a build that drifted is caught here too.

use baud_core::*;
use baud_domain::{
    Codepoint, DecodeBudget, DecodeProgress, ImageBudget, ImagePx, KittyId, KittyNumber,
    KittyPlacement, PlaceholderRun, Row, is_placeholder,
};

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

/// The pinned list's diacritics, number `n` at index `n`.
fn diacritics() -> Vec<char> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/kitty-placeholder-diacritics.txt"
    );
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let hex = l.split(';').next().unwrap();
            char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap()
        })
        .collect()
}

/// The diacritic for `n`.
fn d(n: usize) -> char {
    diacritics()[n]
}

/// A terminal with image memory and a 10x20 cell.
fn with<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    f(&mut t)
}

/// Feed `bytes` as one read and finish any deferred decode, keeping the
/// replies.
fn feed(t: &mut Term<'_>, bytes: &[u8], replies: &mut Vec<u8>) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
    replies.extend_from_slice(t.reply());
    t.clear_reply();
}

/// Feed `bytes` in reads of `n`.
fn feed_by(t: &mut Term<'_>, bytes: &[u8], n: usize) {
    for chunk in bytes.chunks(n) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        t.clear_reply();
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            if k <= q.len() {
                out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]);
            } else {
                out.push(b'=');
            }
        }
    }
    out
}

fn apc(ctrl: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    if !payload.is_empty() {
        v.push(b';');
        v.extend_from_slice(payload);
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

/// A 6x4 RGBA image, transmitted under `ctrl`.
fn image(ctrl: &str) -> Vec<u8> {
    let px: Vec<u8> = (0..24u8).flat_map(|i| [i, 2 * i, 3 * i, 255]).collect();
    apc(&format!("f=32,s=6,v=4,{ctrl}"), &b64(&px))
}

/// The cursor as `(row, col)` indices.
fn at(t: &Term<'_>) -> (usize, usize) {
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

/// **A virtual placement is answered OK and moves the cursor nowhere**,
/// `C` or no `C`: it has "no physical location on the screen". On the
/// commit before, `U=1` was answered EINVAL.
#[test]
fn a_virtual_placement_is_answered_ok_and_moves_nothing() {
    with(|t| {
        let mut r = Vec::new();
        feed(t, &image("a=t,i=7,q=2"), &mut r);
        feed(t, b"\x1b[5;9H", &mut r);
        feed(t, &apc("a=p,U=1,i=7,c=4,r=3", b""), &mut r);
        assert_eq!(r, b"\x1b_Gi=7;OK\x1b\\", "a=p,U=1");
        assert_eq!(at(t), (4, 8), "a=p,U=1 moved the cursor");
        r.clear();
        feed(t, &image("a=T,U=1,i=8,c=2,r=1"), &mut r);
        assert_eq!(r, b"\x1b_Gi=8;OK\x1b\\", "a=T,U=1");
        assert_eq!(at(t), (4, 8), "a=T,U=1 moved the cursor");
        assert_eq!(
            t.images().placements().count(),
            0,
            "nothing placed on screen"
        );
        assert!(
            t.kitty_virtual(KittyId::of(7), KittyPlacement::NONE)
                .is_some()
        );
        assert!(
            t.kitty_virtual(KittyId::of(8), KittyPlacement::NONE)
                .is_some()
        );
        // A virtual placement made relative: "the terminal must respond
        // with the EINVAL error code".
        r.clear();
        feed(t, &apc("a=p,U=1,i=7,P=8,c=1,r=1", b""), &mut r);
        assert!(
            r.starts_with(b"\x1b_Gi=7;EINVAL"),
            "{}",
            String::from_utf8_lossy(&r)
        );
    });
}

/// `kitten icat --unicode-placeholder`'s bytes for a `w` x `h` picture of
/// image `id` (`[[source: kitty-icat-transmit]]`, `write_unicode_placeholder`).
fn kitten(id: u32, w: usize, h: usize) -> String {
    let [msb, r, g, b] = id.to_be_bytes();
    let mut s = format!("\x1b[38:2:{r}:{g}:{b}m");
    for row in 0..h {
        for col in 0..w {
            s.push('\u{10EEEE}');
            s.push(d(row));
            s.push(d(col));
            s.push(d(usize::from(msb)));
        }
        if row + 1 < h {
            s.push_str("\n\r");
        }
    }
    s.push_str("\x1b[39m");
    s
}

/// **Every diacritic of a full-screen kitten picture is kept, and reads
/// back as written.** Kept as combining marks in the 1024-slot cluster
/// store, the commit before dropped 2366 of this picture's 5760.
#[test]
fn every_diacritic_of_a_kitten_picture_is_kept() {
    with(|t| {
        let id = 0x0201_0203;
        feed(t, kitten(id, 80, 24).as_bytes(), &mut Vec::new());
        assert_eq!(t.marks_dropped, 0, "marks dropped");
        for row in [0usize, 11, 23] {
            for col in [0usize, 40, 79] {
                let cell = t.row(Row::from_index(row)).unwrap()[col];
                let base = t.base_of(cell).as_codepoint().map(Codepoint::value);
                assert_eq!(base, Some(0x10EEEE), "({row}, {col})");
                let marks: Vec<u32> = t.cluster_marks(cell).collect();
                let want: Vec<u32> = [d(row), d(col), d(2)]
                    .iter()
                    .map(|c| u32::from(*c))
                    .collect();
                assert_eq!(marks, want, "({row}, {col})");
            }
        }
    });
}

/// **A placeholder with a mark that is not a diacritic, or a fourth, keeps
/// every mark**, through the cluster store as any base's would be: what
/// the cell holds is the text that was written.
#[test]
fn a_placeholder_with_other_marks_keeps_them_all() {
    with(|t| {
        let text = format!(
            "\u{10EEEE}{}{}{}{}\u{10EEEE}{}\u{0301}",
            d(1),
            d(2),
            d(3),
            d(4),
            d(5)
        );
        feed(t, text.as_bytes(), &mut Vec::new());
        let row = t.row(Row::from_index(0)).unwrap();
        let marks = |c: usize| -> Vec<u32> { t.cluster_marks(row[c]).collect() };
        let u = |c: char| u32::from(c);
        assert_eq!(marks(0), vec![u(d(1)), u(d(2)), u(d(3)), u(d(4))]);
        assert_eq!(marks(1), vec![u(d(5)), 0x301]);
        assert_eq!(t.marks_dropped, 0);
    });
}

/// The runs row `y` resolves to.
fn runs(t: &Term<'_>, y: usize) -> Vec<PlaceholderRun> {
    t.placeholder_runs(
        t.row(Row::from_index(y)).unwrap(),
        t.viewport().styles(Row::from_index(y)),
    )
    .collect()
}

/// `(at, cells, image, placement, row, col)` of each run.
fn shape(runs: &[PlaceholderRun]) -> Vec<(usize, usize, u32, u32, usize, usize)> {
    runs.iter()
        .map(|r| {
            (
                r.at.index(),
                r.cells.index(),
                r.image.raw(),
                r.placement.raw(),
                r.row.index(),
                r.col.index(),
            )
        })
        .collect()
}

/// **The text's examples resolve as the text says**: the 2x2 grid with
/// every row and column written, the same with the top byte in a third
/// diacritic, and the 2x3 grid with only the first column's rows written.
#[test]
fn the_texts_examples_resolve_as_it_says() {
    with(|t| {
        let (z, o, two) = (d(0), d(1), d(2));
        let grid = format!(
            "\x1b[38;5;42m\u{10EEEE}{z}{z}\u{10EEEE}{z}{o}\x1b[39m\r\n\
             \x1b[38;5;42m\u{10EEEE}{o}{z}\u{10EEEE}{o}{o}\x1b[39m\r\n\
             \x1b[38;5;42m\u{10EEEE}{z}{z}{two}\u{10EEEE}{z}{o}{two}\r\n\
             \x1b[38;5;42m\u{10EEEE}{z}\u{10EEEE}\u{10EEEE}\r\n\
             \x1b[38;5;42m\u{10EEEE}{o}\u{10EEEE}\u{10EEEE}\x1b[39m"
        );
        feed(t, grid.as_bytes(), &mut Vec::new());
        assert_eq!(shape(&runs(t, 0)), vec![(0, 2, 42, 0, 0, 0)]);
        assert_eq!(shape(&runs(t, 1)), vec![(0, 2, 42, 0, 1, 0)]);
        assert_eq!(shape(&runs(t, 2)), vec![(0, 2, 42 + (2 << 24), 0, 0, 0)]);
        assert_eq!(shape(&runs(t, 3)), vec![(0, 3, 42, 0, 0, 0)]);
        assert_eq!(shape(&runs(t, 4)), vec![(0, 3, 42, 0, 1, 0)]);
    });
}

/// **What a cell inherits, and when it does not.** The placement id is the
/// underline colour's; the top byte is inherited with the row; a column
/// that does not follow, a change of colour, a cell between, a row that
/// differs each start a run of their own, which takes 0 for what it lacks.
#[test]
fn inheritance_follows_the_cell_to_the_left_and_stops_where_the_text_says() {
    with(|t| {
        let p = '\u{10EEEE}';
        let lines = [
            // The underline names placement 5, and a byte of 3 is inherited.
            format!("\x1b[38;2;0;1;2;58;5;5m{p}{}{}{}{p}{p}", d(4), d(6), d(3)),
            // A column that does not follow: a new run at column 9.
            format!("\x1b[38;5;9m{p}{}{}{p}{}{}", d(1), d(0), d(1), d(9)),
            // A change of colour, then a cell between.
            format!(
                "\x1b[38;5;9m{p}{}\x1b[38;5;10m{p}{}\x1b[38;5;9m{p} {p}",
                d(2),
                d(0)
            ),
            // A row that differs, with its column absent.
            format!("\x1b[38;5;9m{p}{}{}{p}{}", d(3), d(7), d(4)),
            // The same id spelled two ways compares equal.
            format!("\x1b[38;5;9m{p}{}{}\x1b[38;2;0;0;9m{p}", d(5), d(2)),
        ];
        for (k, l) in lines.iter().enumerate() {
            feed(
                t,
                format!("\x1b[0m\x1b[{};1H{l}", k + 1).as_bytes(),
                &mut Vec::new(),
            );
        }
        let hi = |b: u32, id: u32| id + (b << 24);
        assert_eq!(shape(&runs(t, 0)), vec![(0, 3, hi(3, 0x0102), 5, 4, 6)]);
        assert_eq!(
            shape(&runs(t, 1)),
            vec![(0, 1, 9, 0, 1, 0), (1, 1, 9, 0, 1, 9)]
        );
        assert_eq!(
            shape(&runs(t, 2)),
            vec![
                (0, 1, 9, 0, 2, 0),
                (1, 1, 10, 0, 0, 0),
                (2, 1, 9, 0, 0, 0),
                (4, 1, 9, 0, 0, 0)
            ]
        );
        assert_eq!(
            shape(&runs(t, 3)),
            vec![(0, 1, 9, 0, 3, 7), (1, 1, 9, 0, 4, 0)]
        );
        assert_eq!(shape(&runs(t, 4)), vec![(0, 2, 9, 0, 5, 2)]);
    });
}

/// What a terminal's screen holds, cell for cell: content, flags, the
/// marks, and the runs of every row.
fn screen(t: &Term<'_>) -> String {
    let mut s = String::new();
    for y in 0..24 {
        let row = t.row(Row::from_index(y)).unwrap();
        for c in row {
            let marks: Vec<u32> = t.cluster_marks(*c).collect();
            s += &format!("{:x}/{:?}/{marks:x?} ", c.content.bits(), c.flags);
        }
        s += &format!("{:?}\n", shape(&runs(t, y)));
    }
    s
}

/// **A placeholder line is the same however its bytes were cut**: at
/// every read size, and in two reads split at every byte, so a diacritic
/// arriving in the read after its placeholder still joins it.
#[test]
fn a_placeholder_line_is_the_same_however_it_is_cut() {
    let p = '\u{10EEEE}';
    let text = format!(
        "{}\r\n\x1b[38;5;7;58;5;3m{p}{}{p}{p}{}{}{}{p}\u{0301}{p}",
        kitten(0x0300_0102, 4, 2),
        d(2),
        d(1),
        d(1),
        d(5)
    );
    let bytes = text.as_bytes();
    let whole = with(|t| {
        feed_by(t, bytes, bytes.len());
        screen(t)
    });
    assert!(whole.contains("(0, 4, 50331906, 0, 1, 0)"), "the premise");
    for n in 1..=8 {
        let got = with(|t| {
            feed_by(t, bytes, n);
            screen(t)
        });
        assert_eq!(got, whole, "reads of {n}");
    }
    for cut in 1..bytes.len() {
        let got = with(|t| {
            for part in [&bytes[..cut], &bytes[cut..]] {
                t.write_at(BytePos::ZERO, part);
                t.feed(BytePos::ZERO, ReadLen::from_index(part.len()));
            }
            screen(t)
        });
        assert_eq!(got, whole, "two reads cut at {cut}");
    }
}

/// Whether image `id` has a virtual placement `p`, and whether it is stored.
fn state(t: &Term<'_>, id: u32, p: u32) -> (bool, bool) {
    let v = t
        .kitty_virtual(KittyId::of(id), KittyPlacement::of(p))
        .is_some();
    let stored = t.images().kitty_image(KittyId::of(id)).is_some();
    (v, stored)
}

/// The id the terminal gave image number 9.
fn nine(t: &Term<'_>) -> u32 {
    let img = t.images().kitty_newest(KittyNumber::of(9)).unwrap();
    t.images().kitty_names(img).unwrap().0.raw()
}

/// Images 3 and 4 transmitted, 3 with virtual placements 1 and 2 and 4
/// with one, and image number 9 with one.
fn setup(t: &mut Term<'_>) {
    let mut r = Vec::new();
    feed(t, &image("a=t,i=3,q=2"), &mut r);
    feed(t, &image("a=t,i=4,q=2"), &mut r);
    feed(t, &image("a=t,I=9,q=2"), &mut r);
    for c in [
        "a=p,U=1,i=3,p=1,c=2,r=2,q=2",
        "a=p,U=1,i=3,p=2,c=3,r=1,q=2",
        "a=p,U=1,i=4,c=1,r=1,q=2",
        "a=p,U=1,I=9,c=1,r=1,q=2",
    ] {
        feed(t, &apc(c, b""), &mut r);
    }
    assert!(r.is_empty());
}

/// **Only `d=i`, `I`, `n`, `N`, `r` and `R` delete a virtual placement**:
/// "The key values a, c, p, q, x, y, z and their capital variants never
/// affect virtual placements because they do not have a physical location
/// on the screen". The uppercase letters free an image its virtual
/// placements alone held; a lowercase one leaves it stored.
#[test]
fn only_the_id_number_and_range_deletes_reach_a_virtual_placement() {
    for spec in [
        "a=d",
        "a=d,d=A",
        "a=d,d=c",
        "a=d,d=C",
        "a=d,d=p,x=1,y=1",
        "a=d,d=P,x=1,y=1",
        "a=d,d=q,x=1,y=1",
        "a=d,d=Q,x=1,y=1",
        "a=d,d=x,x=1",
        "a=d,d=X,x=1",
        "a=d,d=y,y=1",
        "a=d,d=Y,y=1",
        "a=d,d=z",
        "a=d,d=Z",
    ] {
        with(|t| {
            setup(t);
            feed(t, &apc(spec, b""), &mut Vec::new());
            assert_eq!(state(t, 3, 1), (true, true), "{spec}");
            assert_eq!(state(t, 4, 0), (true, true), "{spec}");
        });
    }
    with(|t| {
        setup(t);
        feed(t, &apc("a=d,d=i,i=3,p=2", b""), &mut Vec::new());
        assert_eq!(state(t, 3, 2), (false, true), "d=i with p takes that one");
        assert_eq!(state(t, 3, 1), (true, true), "and leaves the other");
        feed(t, &apc("a=d,d=i,i=3", b""), &mut Vec::new());
        assert_eq!(state(t, 3, 1), (false, true), "d=i keeps the image");
        feed(t, &apc("a=d,d=I,i=4", b""), &mut Vec::new());
        assert_eq!(state(t, 4, 0), (false, false), "d=I frees it");
    });
    with(|t| {
        setup(t);
        feed(t, &apc("a=d,d=I,i=3,p=2", b""), &mut Vec::new());
        assert_eq!(
            state(t, 3, 1),
            (true, true),
            "d=I with p keeps an image held elsewhere"
        );
        feed(t, &apc("a=d,d=I,i=3,p=1", b""), &mut Vec::new());
        assert_eq!(state(t, 3, 1), (false, false), "and frees it with its last");
    });
    with(|t| {
        setup(t);
        let id = nine(t);
        assert_eq!(state(t, id, 0), (true, true), "the premise");
        feed(t, &apc("a=d,d=n,I=9", b""), &mut Vec::new());
        assert_eq!(state(t, id, 0), (false, true), "d=n");
    });
    with(|t| {
        setup(t);
        let id = nine(t);
        feed(t, &apc("a=d,d=N,I=9", b""), &mut Vec::new());
        assert_eq!(state(t, id, 0), (false, false), "d=N");
    });
    with(|t| {
        setup(t);
        feed(t, &apc("a=d,d=r,x=3,y=3", b""), &mut Vec::new());
        assert_eq!(state(t, 3, 1), (false, true), "d=r");
        assert_eq!(state(t, 4, 0), (true, true), "d=r outside the range");
        feed(t, &apc("a=d,d=R,x=4,y=9", b""), &mut Vec::new());
        assert_eq!(state(t, 4, 0), (false, false), "d=R");
    });
}

/// **What else a virtual placement outlives, and what it does not**: `ED
/// 2`, the alternate screen and its leaving, and the quota leave it; the
/// image re-transmitted under its id takes it, "the existing image and all
/// its placements must be deleted"; `RIS` takes everything; the same image
/// and placement id replaces it.
#[test]
fn a_virtual_placement_outlives_the_screen_and_not_its_image() {
    with(|t| {
        setup(t);
        feed(t, b"\x1b[2J\x1b[?1049h\x1b[2J\x1b[?1049l", &mut Vec::new());
        assert_eq!(
            state(t, 3, 1),
            (true, true),
            "ED 2 and the alternate screen"
        );
        feed(t, &apc("a=p,U=1,i=3,p=1,c=5,r=4,q=2", b""), &mut Vec::new());
        let v = t
            .kitty_virtual(KittyId::of(3), KittyPlacement::of(1))
            .unwrap();
        assert_eq!(
            (v.place.cols.index(), v.place.rows.index()),
            (5, 4),
            "replaced"
        );
        let n = t.images().placements().count();
        assert_eq!(n, 0, "no placement on screen");
        feed(t, &image("a=t,i=3,q=2"), &mut Vec::new());
        assert_eq!(state(t, 3, 1), (false, true), "re-transmitted");
        assert_eq!(state(t, 3, 2), (false, true), "re-transmitted");
        assert!(t.kitty_virtuals(), "image 4's is held");
        feed(t, b"\x1bc", &mut Vec::new());
        assert!(!t.kitty_virtuals(), "RIS");
    });
}

/// **The geometry is the whole image in the box**: `c` and `r` as given,
/// or the image's own size in cells rounded up, at the cell size set now.
#[test]
fn a_count_not_given_is_the_images_size_in_cells() {
    with(|t| {
        feed(t, &image("a=t,i=3,q=2"), &mut Vec::new());
        feed(t, &apc("a=p,U=1,i=3,q=2", b""), &mut Vec::new());
        let v = t
            .kitty_virtual(KittyId::of(3), KittyPlacement::NONE)
            .unwrap();
        assert_eq!((v.place.cols.index(), v.place.rows.index()), (1, 1));
        t.set_cell_size(ImagePx::of(4), ImagePx::of(3));
        let v = t
            .kitty_virtual(KittyId::of(3), KittyPlacement::NONE)
            .unwrap();
        assert_eq!((v.place.cols.index(), v.place.rows.index()), (2, 2));
        assert_eq!(v.place.fit, baud_domain::KittyFit::Box);
    });
}

/// **U+10EEEE is one cell wide**: a private-use character, narrow
/// whatever the ambiguous width, as kitty draws it.
#[test]
fn a_placeholder_is_one_cell_wide() {
    with(|t| {
        feed(t, "\u{10EEEE}\u{10EEEE}x".as_bytes(), &mut Vec::new());
        assert_eq!(at(t), (0, 3));
        let c = t.row(Row::from_index(0)).unwrap()[0];
        assert!(t.base_of(c).as_codepoint().is_some_and(is_placeholder));
    });
}

/// **The quota spares an image a virtual placement holds**: "existing
/// images without placements will be preferentially deleted", and a
/// virtual placement is one. An arena of two 6x4 images: the third
/// transmitted deletes the image with no placement, not the held one.
#[test]
fn the_quota_spares_an_image_a_virtual_placement_holds() {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(2 * 24 + 8));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    let mut r = Vec::new();
    feed(&mut t, &image("a=t,i=3,q=2"), &mut r);
    feed(&mut t, &apc("a=p,U=1,i=3,c=1,r=1,q=2", b""), &mut r);
    feed(&mut t, &image("a=t,i=4,q=2"), &mut r);
    feed(&mut t, &image("a=t,i=5,q=2"), &mut r);
    assert!(r.is_empty(), "{}", String::from_utf8_lossy(&r));
    let stored = |id| t.images().kitty_image(KittyId::of(id)).is_some();
    assert_eq!((stored(3), stored(4), stored(5)), (true, false, true));
    assert_eq!(t.images().images_evicted, 1);
    assert!(
        t.kitty_virtual(KittyId::of(3), KittyPlacement::NONE)
            .is_some()
    );
}

/// **Once no image without a placement is left, the quota takes a held
/// one, and its virtual placement goes with it**, with every relative
/// placement under that: "the terminal emulator should delete older images
/// to make space for the new one". An arena of two 6x4 images: image 3 held
/// by virtual placement 7, image 4 put on screen under it. Image 5 finds
/// none unplaced and deletes 3, the older; 4's placement under 3's virtual
/// one goes, and 4 with it, as after a delete of 3: "If the image that the
/// relative placement is a placement of, has no more placements, the image
/// is deleted as well" (`[[source: kitty-graphics]]`, "Relative
/// placements"). Deleted for its parent, not for space, so one counted.
#[test]
fn the_quota_takes_a_held_image_last_and_its_virtual_placement_with_it() {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(2 * 24 + 8));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    let mut r = Vec::new();
    feed(&mut t, &image("a=t,i=3,q=2"), &mut r);
    feed(&mut t, &apc("a=p,U=1,i=3,p=7,c=1,r=1,q=2", b""), &mut r);
    feed(&mut t, &image("a=t,i=4,q=2"), &mut r);
    feed(&mut t, &apc("a=p,i=4,p=1,P=3,Q=7,q=2", b""), &mut r);
    assert_eq!(t.images().placements().count(), 1, "the premise: 4 under 3");
    feed(&mut t, &image("a=t,i=5,q=2"), &mut r);
    assert!(r.is_empty(), "{}", String::from_utf8_lossy(&r));
    let stored = |id| t.images().kitty_image(KittyId::of(id)).is_some();
    assert_eq!((stored(3), stored(4), stored(5)), (false, false, true));
    assert_eq!(t.images().images_evicted, 1);
    assert_eq!(
        t.images().placements().count(),
        0,
        "the placement under 3 went"
    );
    assert!(
        t.kitty_virtual(KittyId::of(3), KittyPlacement::NONE)
            .is_none()
    );
}

/// **What the table refuses it counts**: the 257th virtual placement, and
/// one of an image no id names, which no placeholder could show and which
/// is not held.
#[test]
fn a_full_table_and_a_nameless_image_are_refused_and_counted() {
    with(|t| {
        feed(t, &image("a=t,i=3,q=2"), &mut Vec::new());
        for p in 1..=257 {
            feed(
                t,
                &apc(&format!("a=p,U=1,i=3,p={p},c=1,r=1,q=2"), b""),
                &mut Vec::new(),
            );
        }
        assert_eq!(t.kitty().virtuals_refused, 1, "the 257th");
        for (p, held) in [(1, true), (256, true), (257, false)] {
            let v = t.kitty_virtual(KittyId::of(3), KittyPlacement::of(p));
            assert_eq!(v.is_some(), held, "placement {p}");
        }
    });
    with(|t| {
        feed(t, &image("a=T,U=1,c=1,r=1"), &mut Vec::new());
        assert_eq!(t.kitty().virtuals_refused, 1, "a nameless image");
        assert!(!t.kitty_virtuals(), "nothing held");
        assert_eq!(t.images().images_dropped, 0, "the premise");
        feed(t, &image("a=t,i=9,q=2"), &mut Vec::new());
        assert_eq!(
            t.images().images_dropped,
            1,
            "unheld, the nameless image goes at the next transmission"
        );
    });
}
