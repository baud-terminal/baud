//! What kitty's Unicode placeholders draw, on the CPU compositor
//! (`[[source: kitty-graphics]]`, "Unicode placeholders"): a grid of
//! U+10EEEE cells shows the image its virtual placement fits into its
//! `c` x `r` cells -- "The image will eventually be fit to the specified
//! rectangle, its aspect ratio preserved" -- each cell its own slice.
//!
//! **The oracle is a placement of the same box.** A virtual placement's
//! whole image in `c` by `r` cells is what a placement on screen with `c`
//! and `r` draws (`kitty_draw.rs` holds that one to the text pixel by
//! pixel), so a full grid of placeholders at the anchor must equal it pixel
//! for pixel, and a partial grid must equal it inside its cells and the
//! empty frame outside them. The image is 6 by 4 with every texel distinct
//! and the boxes are not square, so a row and a column swapped, a run that
//! inherited nothing, an id's top byte or a placement id ignored each draw
//! other pixels.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, ImageBudget, ImagePx, ReadLen};
use baud_font::Face;
use baud_render::Damage;
use support::{Canvas, WS, font, renderer};

const IW: u32 = 6;
const IH: u32 = 4;

fn texel(x: u32, y: u32) -> u32 {
    0xFF00_0000 | ((37 * x + 20) << 16) | ((53 * y + 10) << 8) | 200
}

fn other(x: u32, y: u32) -> u32 {
    0xFF00_0000 | (200 << 16) | ((29 * x + 5) << 8) | (41 * y + 30)
}

fn rgba(px: impl Fn(u32, u32) -> u32) -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..IH {
        for x in 0..IW {
            let [_, r, g, b] = px(x, y).to_be_bytes();
            v.extend_from_slice(&[r, g, b, 0xFF]);
        }
    }
    v
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

fn feed(t: &mut Term, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn transmit(t: &mut Term, id: u32, px: impl Fn(u32, u32) -> u32) {
    let ctrl = format!("a=t,f=32,s={IW},v={IH},i={id},q=2");
    feed(t, &apc(&ctrl, &b64(&rgba(px))));
}

/// The pinned list's diacritic for `n`.
fn d(n: usize) -> char {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/kitty-rowcolumn-diacritics.txt"
    );
    let src = std::fs::read_to_string(path).unwrap();
    let line = src
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .nth(n)
        .unwrap();
    char::from_u32(u32::from_str_radix(line.split(';').next().unwrap(), 16).unwrap()).unwrap()
}

/// The anchor, zero-based: row 2, column 4.
const ROW: usize = 2;
const COL: usize = 4;

/// One full frame of a terminal with image memory at the renderer's cell
/// size, cursor hidden, after `setup`; and the cell size.
struct Frame {
    c: Canvas,
    cell: (usize, usize),
}

fn draw(setup: impl FnOnce(&mut Term)) -> Frame {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(
        ImagePx::of(u16::try_from(cw).unwrap()),
        ImagePx::of(u16::try_from(ch).unwrap()),
    );
    feed(&mut t, b"\x1b[?25l");
    setup(&mut t);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let cell = (usize::try_from(cw).unwrap(), usize::try_from(ch).unwrap());
    Frame { c, cell }
}

/// The pixels two frames differ in, inside `cells` -- `(row, col)` cells
/// -- or everywhere when `cells` is `None`.
fn differ(a: &Frame, b: &Frame, cells: Option<&[(usize, usize)]>) -> usize {
    let (cw, ch) = a.cell;
    let w = a.c.width;
    (0..a.c.pixels.len())
        .filter(|&i| {
            let (x, y) = (i % w, i / w);
            cells.is_none_or(|cs| cs.contains(&(y / ch, x / cw)))
        })
        .filter(|&i| a.c.pixels[i] != b.c.pixels[i])
        .count()
}

/// The placement of image `id`'s whole box, `c` by `r` cells, at the anchor.
fn placed(id: u32, px: fn(u32, u32) -> u32, c: usize, r: usize) -> Frame {
    draw(|t| {
        transmit(t, id, px);
        feed(t, format!("\x1b[{};{}H", ROW + 1, COL + 1).as_bytes());
        feed(t, &apc(&format!("a=p,i={id},c={c},r={r},q=2"), b""));
    })
}

/// How a grid writes its diacritics.
#[derive(Clone, Copy, Debug)]
enum Form {
    /// Every cell its row and column, and the byte when there is one: what
    /// `kitten icat` writes.
    Full,
    /// The first cell of each row its row and column, and the byte; the
    /// rest inherit.
    First,
    /// The first cell of each row its row only; the rest inherit -- the
    /// text's 2 by 3 example.
    RowOnly,
}

/// The placeholder cells `cells` of a grid, in `sgr`, each row's from the
/// anchor's column on, with the id's top byte `msb` when it is not zero.
fn grid(sgr: &str, cells: &[(usize, usize)], msb: usize, form: Form) -> Vec<u8> {
    let mut s = String::new();
    let mut rows: Vec<usize> = cells.iter().map(|c| c.0).collect();
    rows.dedup();
    for r in rows {
        let cols: Vec<usize> = cells.iter().filter(|c| c.0 == r).map(|c| c.1).collect();
        let first = cols[0];
        s += &format!("\x1b[{};{}H\x1b[0;{sgr}m", ROW + 1 + r, COL + 1 + first);
        for c in cols {
            s.push('\u{10EEEE}');
            let lead = c == first;
            match form {
                Form::Full => {
                    s.push(d(r));
                    s.push(d(c));
                    if msb != 0 {
                        s.push(d(msb));
                    }
                }
                Form::First if lead => {
                    s.push(d(r));
                    s.push(d(c));
                    if msb != 0 {
                        s.push(d(msb));
                    }
                }
                Form::RowOnly if lead => s.push(d(r)),
                _ => {}
            }
        }
    }
    s += "\x1b[0m";
    s.into_bytes()
}

/// Every cell of a `c` by `r` box.
fn all(c: usize, r: usize) -> Vec<(usize, usize)> {
    (0..r).flat_map(|y| (0..c).map(move |x| (y, x))).collect()
}

/// The frame of image `id` with virtual placement `virt` and `grid` written.
fn shown(id: u32, px: fn(u32, u32) -> u32, virt: &str, grid: &[u8]) -> Frame {
    draw(|t| {
        transmit(t, id, px);
        feed(t, &apc(&format!("a=p,U=1,i={id},q=2,{virt}"), b""));
        feed(t, grid);
    })
}

/// **A full grid draws what a placement of its box draws**, pixel for
/// pixel, however its diacritics are written: on three boxes, wider and
/// narrower than the image's aspect.
#[test]
fn a_full_grid_draws_what_a_placement_of_its_box_draws() {
    let empty = draw(|_| {});
    for (c, r) in [(4, 3), (2, 3), (5, 1)] {
        let want = placed(42, texel, c, r);
        assert!(
            differ(&want, &empty, None) > 0,
            "the premise: {c}x{r} draws"
        );
        for form in [Form::Full, Form::First, Form::RowOnly] {
            let got = shown(
                42,
                texel,
                &format!("c={c},r={r}"),
                &grid("38;5;42", &all(c, r), 0, form),
            );
            assert_eq!(differ(&got, &want, None), 0, "{c}x{r}, {form:?}");
        }
    }
}

/// **A partial grid draws its cells' slices and nothing else**: "in case
/// of a mismatch only part of the image will be displayed".
#[test]
fn a_partial_grid_draws_only_its_cells() {
    let empty = draw(|_| {});
    let want = placed(42, texel, 4, 3);
    let cells = [(1, 1), (1, 2), (2, 3)];
    let got = shown(
        42,
        texel,
        "c=4,r=3",
        &grid("38;5;42", &cells, 0, Form::Full),
    );
    let on: Vec<(usize, usize)> = cells.iter().map(|(r, c)| (ROW + r, COL + c)).collect();
    assert_eq!(differ(&got, &want, Some(&on)), 0, "inside its cells");
    assert_eq!(
        differ(&got, &empty, None),
        differ(&got, &empty, Some(&on)),
        "outside"
    );
    assert!(
        differ(&got, &empty, Some(&on)) > 0,
        "the premise: something drew"
    );
}

/// **The underline colour names the placement**, and with none "the
/// terminal may choose any virtual placement of the given image": here the
/// first made.
#[test]
fn the_underline_colour_names_the_placement() {
    let two = [
        "a=p,U=1,i=42,p=1,c=4,r=3,q=2",
        "a=p,U=1,i=42,p=2,c=2,r=3,q=2",
    ];
    let frame = |sgr: &str, cells: &[(usize, usize)]| {
        let g = grid(sgr, cells, 0, Form::First);
        draw(|t| {
            transmit(t, 42, texel);
            for v in two {
                feed(t, &apc(v, b""));
            }
            feed(t, &g);
        })
    };
    let got = frame("38;5;42;58;5;2", &all(2, 3));
    assert_eq!(
        differ(&got, &placed(42, texel, 2, 3), None),
        0,
        "placement 2"
    );
    let got = frame("38;5;42", &all(4, 3));
    assert_eq!(
        differ(&got, &placed(42, texel, 4, 3), None),
        0,
        "no placement id"
    );
    let got = frame("38;5;42;58;5;1", &all(4, 3));
    assert_eq!(
        differ(&got, &placed(42, texel, 4, 3), None),
        0,
        "placement 1"
    );
}

/// **The third diacritic is the id's top byte**: two images whose ids
/// share their low 24 bits, told apart by it alone.
#[test]
fn the_third_diacritic_is_the_ids_top_byte() {
    let hi = 42 + (2 << 24);
    let frame = |msb: usize, form: Form| {
        let g = grid("38;5;42", &all(4, 3), msb, form);
        draw(|t| {
            transmit(t, 42, texel);
            transmit(t, hi, other);
            feed(t, &apc("a=p,U=1,i=42,c=4,r=3,q=2", b""));
            feed(t, &apc(&format!("a=p,U=1,i={hi},c=4,r=3,q=2"), b""));
            feed(t, &g);
        })
    };
    for form in [Form::Full, Form::First] {
        let got = frame(2, form);
        assert_eq!(
            differ(&got, &placed(hi, other, 4, 3), None),
            0,
            "byte 2, {form:?}"
        );
        let got = frame(0, form);
        assert_eq!(
            differ(&got, &placed(42, texel, 4, 3), None),
            0,
            "no byte, {form:?}"
        );
    }
}

/// **A placeholder with nothing to show draws its background**: the
/// character and its diacritics are never drawn (kitty's blank font,
/// `[[source: kitty-fonts-c]]`; Jev's interim BLANK), with no virtual
/// placement, no image, or the default foreground's id 0.
#[test]
fn a_placeholder_with_nothing_to_show_draws_its_background() {
    let empty = draw(|_| {});
    let cells = all(4, 3);
    type Setup = fn(&mut Term);
    let cases: [(&str, Setup); 3] = [
        ("no virtual placement", |t| transmit(t, 42, texel)),
        ("no image", |_| {}),
        ("id 0", |t| {
            transmit(t, 42, texel);
            feed(t, &apc("a=p,U=1,i=42,c=4,r=3,q=2", b""));
        }),
    ];
    for (k, (name, setup)) in cases.into_iter().enumerate() {
        let sgr = if k == 2 { "39" } else { "38;5;42" };
        let g = grid(sgr, &cells, 0, Form::Full);
        let got = draw(|t| {
            setup(t);
            feed(t, &g);
            feed(t, "\x1b[1;1H\u{10EEEE}".as_bytes());
        });
        assert_eq!(differ(&got, &empty, None), 0, "{name}");
    }
}

/// **A delete that names a cell leaves a virtual placement; one that
/// names its image takes it**, and the grid with it.
#[test]
fn a_cell_delete_leaves_the_grid_and_an_id_delete_takes_it() {
    let empty = draw(|_| {});
    let want = placed(42, texel, 4, 3);
    for (del, gone) in [
        ("a=d", false),
        ("a=d,d=p,x=6,y=4", false),
        ("a=d,d=i,i=42", true),
    ] {
        let got = draw(|t| {
            transmit(t, 42, texel);
            feed(t, &apc("a=p,U=1,i=42,c=4,r=3,q=2", b""));
            feed(t, &grid("38;5;42", &all(4, 3), 0, Form::Full));
            feed(t, &apc(del, b""));
        });
        let base = if gone { &empty } else { &want };
        assert_eq!(differ(&got, base, None), 0, "{del}");
    }
}

/// `kitten icat --unicode-placeholder` 0.49.2's output for a 60x40
/// picture in an 80x24 window at 17x34 a cell, as the pty received it
/// (`scripts/record-kitten-icat.py`'s `placeholder` mode;
/// `tests/recordings/README.md`): `a=T,U=1,c=4,r=2,X=4` with the PNG, then
/// two rows of four U+10EEEE cells, each with its row, column and id-byte
/// diacritics, centred with spaces.
const KITTEN: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/kitty/grad-80x24-placeholder.raw"
));

/// **kitten's own placeholders place the picture**: replayed, the frame
/// is what a placement of the same picture in the same four by two cells
/// draws at the cell kitten centred it on -- with `X` unused, as a virtual
/// placement's geometry is the whole image in its cells.
#[test]
fn kittens_placeholders_place_the_picture() {
    let start = KITTEN.windows(3).position(|w| w == b"\x1b_G").unwrap();
    let semi = start + KITTEN[start..].iter().position(|b| *b == b';').unwrap();
    let end = semi
        + KITTEN[semi..]
            .windows(2)
            .position(|w| w == b"\x1b\\")
            .unwrap();
    let keys = std::str::from_utf8(&KITTEN[start + 3..semi]).unwrap();
    assert!(
        keys.contains("U=1") && keys.contains("c=4,r=2"),
        "the premise: {keys}"
    );
    let id = keys.split(',').find_map(|k| k.strip_prefix("i=")).unwrap();
    let settle = |t: &mut Term| {
        while t.decode_step(baud_domain::DecodeBudget::ALL) == baud_domain::DecodeProgress::More {}
    };
    let got = draw(|t| {
        feed(t, KITTEN);
        settle(t);
    });
    let mut put = format!("\x1b[1;39H\x1b_Ga=T,q=2,f=100,C=1,c=4,r=2,i={id};").into_bytes();
    put.extend_from_slice(&KITTEN[semi + 1..end]);
    put.extend_from_slice(b"\x1b\\");
    let want = draw(|t| {
        feed(t, &put);
        settle(t);
    });
    let empty = draw(|_| {});
    assert!(
        differ(&want, &empty, None) > 0,
        "the premise: the placement draws"
    );
    assert_eq!(differ(&got, &want, None), 0, "kitten's placeholders");
}

/// **Incremental frames show what a full frame shows**, the app's default
/// path, where a placeholder's cells can be on screen before its picture:
/// kitten's PNG decodes after the read that wrote the cells, and a virtual
/// placement can arrive after the text. No cell changes when the picture
/// becomes drawable; only the frame's image list does.
#[test]
fn incremental_frames_show_the_picture_when_it_becomes_drawable() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let full = |t: &mut Term| {
        let mut r = renderer(&faces);
        let mut c = Canvas::for_renderer(&r);
        {
            let mut target = c.target();
            r.render(t, &mut target, Damage::Full);
        }
        c.pixels
    };
    let settle = |t: &mut Term| {
        while t.decode_step(baud_domain::DecodeBudget::ALL) == baud_domain::DecodeProgress::More {}
    };
    type Step = fn(&mut Term);
    let cases: [(&str, Step, Step); 2] = [
        (
            "kitten's decode after its cells",
            |t| feed(t, KITTEN),
            |t| {
                while t.decode_step(baud_domain::DecodeBudget::ALL)
                    == baud_domain::DecodeProgress::More
                {}
            },
        ),
        (
            "the virtual placement after the text",
            |t| {
                transmit(t, 42, texel);
                feed(t, &grid("38;5;42", &all(4, 3), 0, Form::Full));
            },
            |t| feed(t, &apc("a=p,U=1,i=42,c=4,r=3,q=2", b"")),
        ),
    ];
    for (name, before, after) in cases {
        let mut r = renderer(&faces);
        let mut c = Canvas::for_renderer(&r);
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
        let mut t = Term::new(owned.storage(), &WS);
        let m = r.metrics();
        t.set_cell_size(
            ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
            ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
        );
        feed(&mut t, b"\x1b[?25l");
        {
            let mut target = c.target();
            r.render(&mut t, &mut target, Damage::Full);
        }
        before(&mut t);
        {
            let mut target = c.target();
            r.render(&mut t, &mut target, Damage::Incremental);
        }
        assert_eq!(c.pixels, full(&mut t), "{name}: before");
        let empty = c.pixels.clone();
        after(&mut t);
        settle(&mut t);
        {
            let mut target = c.target();
            r.render(&mut t, &mut target, Damage::Incremental);
        }
        assert_ne!(c.pixels, empty, "{name}: the premise, the picture drew");
        assert_eq!(c.pixels, full(&mut t), "{name}: after");
    }
}

/// **A placeholder's image is never below its own cells' backgrounds**:
/// "The background color is interpreted as the background color, visible
/// if the image is transparent" ("Unicode placeholders"), so the image is
/// over it, whatever the virtual placement's `z` -- below `INT32_MIN/2`
/// included, which would put a placement's image under a red background.
#[test]
fn a_placeholder_image_is_over_its_cells_background_at_any_z() {
    let cells = all(4, 3);
    let sgr = "38;5;42;41";
    let bare = draw(|t| feed(t, &grid(sgr, &cells, 0, Form::Full)));
    let under = shown(42, texel, "c=4,r=3,z=-1", &grid(sgr, &cells, 0, Form::Full));
    assert!(differ(&under, &bare, None) > 0, "the premise: it draws");
    let deep = shown(
        42,
        texel,
        "c=4,r=3,z=-1073741825",
        &grid(sgr, &cells, 0, Form::Full),
    );
    assert_eq!(differ(&deep, &under, None), 0, "over the red, as at -1");
}
