//! iTerm2's inline images (`[[source: iterm2-images]]`): `File=args:base64`
//! and `MultipartFile`, `FilePart`, `FileEnd`, at every read size, each
//! picture's pixels, place and cursor against what iTerm2's arithmetic gives
//! (`[[source: iterm2-964a75d-inline-image-helper]]`). The PNGs are written
//! by the `png` crate, never by the decoder under test.

use baud_core::*;
use baud_domain::{ImageBudget, ImagePx, ItermKey, KittyCounts, Messages, Pixel};

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

/// The read sizes: a byte at a time, a prime that splits every key and
/// quantum, one past a 4096-byte read, and whole.
const SIZES: [usize; 4] = [1, 7, 4097, 1 << 20];

const ONE: Messages = Messages::of(1);

/// A terminal with `pixels` of arena, a 10x20 cell and a white foreground.
fn with<R>(pixels: usize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(pixels));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    feed(&mut t, b"\x1b]10;rgb:ff/ff/ff\x07", 4096);
    f(&mut t)
}

fn feed(t: &mut Term<'_>, bytes: &[u8], size: usize) {
    for read in bytes.chunks(size) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
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
            match k <= q.len() {
                true => out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => out.push(b'='),
            }
        }
    }
    out
}

/// An RGBA PNG of `w` by `h` with noisy pixels, and its pixels as the store
/// keeps them.
fn picture(w: u32, h: u32, seed: u32) -> (Vec<u8>, Vec<u32>) {
    let mut x = seed.max(1);
    let data: Vec<u8> = (0..w * h * 4)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            // Every fourth byte, the alpha, opaque, so the picture covers.
            match i % 4 {
                3 => 0xff,
                _ => x.to_le_bytes()[0],
            }
        })
        .collect();
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(&data).unwrap();
    }
    let c = Chan::from_u8;
    let px = data
        .chunks(4)
        .map(|p| Pixel::straight(c(p[3]), c(p[0]), c(p[1]), c(p[2])).bits())
        .collect();
    (out, px)
}

fn file(args: &str, data: &[u8]) -> Vec<u8> {
    let mut v = format!("\x1b]1337;File={args}:").into_bytes();
    v.extend(b64(data));
    v.push(0x07);
    v
}

/// The one placement: its column, its image's size and pixels.
fn placed(t: &Term<'_>) -> (usize, u16, u16, Vec<u32>) {
    let imgs = t.images();
    let mut ps = imgs.placements();
    let (_, p) = ps.next().expect("a placement");
    assert!(ps.next().is_none(), "one placement");
    let v = imgs.image(p.image).expect("shown");
    (
        p.col.index(),
        v.width.raw(),
        v.height.raw(),
        v.pixels.iter().map(|p| p.bits()).collect(),
    )
}

/// **"auto: The image's inherent size"**: a PNG placed at the cursor one
/// pixel a device pixel, its cells `ceil(30 / 10)` by `ceil(40 / 20)`, the
/// cursor on its last row right of it, at every read size -- a picture
/// whose base64 passes 4096 bytes, so the routed body crosses reads.
#[test]
fn an_inline_png_is_placed_at_its_own_size() {
    let (png, px) = picture(30, 40, 7);
    assert!(b64(&png).len() > 4096, "the body crosses a read");
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, b"\x1b[3;5H", size);
            feed(t, &file("inline=1", &png), size);
            let (col, w, h, got) = placed(t);
            assert_eq!((col, w, h), (4, 30, 40), "reads of {size}");
            assert!(got == px, "the pixels, reads of {size}");
            assert_eq!(
                t.cursor(),
                (Row::from_index(3), Col::at_index(7)),
                "reads of {size}"
            );
            assert_eq!(t.iterm_counts().of(ItermKey::File), Messages::NONE);
        });
    }
}

/// The IJG's baseline test image and its reference decode
/// (`[[source: ijg-jpeg-6b]]`), as `djpeg -dct int` writes it.
const TESTORIG: &[u8] = include_bytes!("../../baud-domain/tests/ijg/testorig.jpg");
const TESTIMG: &[u8] = include_bytes!("../../baud-domain/tests/ijg/testimg.ppm");

/// The reference decode's pixels as the store keeps them, opaque.
fn testimg() -> Vec<u32> {
    // "P6\n227 149\n255\n", fifteen bytes.
    assert_eq!(&TESTIMG[..15], b"P6\n227 149\n255\n");
    TESTIMG[15..]
        .chunks(3)
        .map(|p| u32::from_be_bytes([0xff, p[0], p[1], p[2]]))
        .collect()
}

/// **A JPEG draws, not the placeholder**: `imgcat`'s JPEG is "Any image
/// format that macOS supports", placed at its own size at the cursor, its
/// pixels the reference decode's, the cursor right of it on its last row,
/// at every read size; with an Exif orientation of 6 the picture is shown
/// turned a quarter clockwise, its sides swapped.
#[test]
fn an_inline_jpeg_is_placed_as_it_decodes() {
    let want = testimg();
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, b"\x1b[3;5H", size);
            feed(t, &file("inline=1", TESTORIG), size);
            let (col, w, h, got) = placed(t);
            assert_eq!((col, w, h), (4, 227, 149), "reads of {size}");
            assert!(got == want, "the pixels, reads of {size}");
            // 149 pixels are eight rows of 20; 227 are 23 columns of 10.
            assert_eq!(
                t.cursor(),
                (Row::from_index(9), Col::at_index(27)),
                "reads of {size}"
            );
            assert_eq!(t.iterm_counts().of(ItermKey::File), Messages::NONE);
        });
    }
    // Exif's tag 274 at 6, big-endian, after the JFIF APP0's 18 bytes.
    let mut turned = TESTORIG[..20].to_vec();
    let tiff = b"MM\x00\x2a\x00\x00\x00\x08\x00\x01\x01\x12\x00\x03\x00\x00\x00\x01\x00\x06\x00\x00\x00\x00\x00\x00";
    let len = u16::try_from(2 + 6 + tiff.len()).unwrap();
    turned.extend([0xff, 0xe1]);
    turned.extend(len.to_be_bytes());
    turned.extend(b"Exif\0\0");
    turned.extend(tiff);
    turned.extend_from_slice(&TESTORIG[20..]);
    with(1 << 16, |t| {
        feed(t, &file("inline=1", &turned), 4097);
        let (_, w, h, got) = placed(t);
        assert_eq!((w, h), (149, 227), "the sides swapped");
        // Shown (x, y) is coded (y, 148 - x).
        for y in 0..227 {
            for x in 0..149 {
                assert_eq!(got[y * 149 + x], want[(148 - x) * 227 + y], "({x}, {y})");
            }
        }
    });
}

/// **A JPEG this decoder does not implement draws the placeholder**, and is
/// counted: an arithmetic-coded frame, `SOF9`.
#[test]
fn a_jpeg_process_not_built_draws_the_placeholder() {
    let sof = TESTORIG.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    let mut f = TESTORIG.to_vec();
    f[sof + 1] = 0xc9;
    with(1 << 16, |t| {
        feed(t, &file("inline=1", &f), 4097);
        let (_, w, h, _) = placed(t);
        assert_eq!((w, h), (35, 36), "the placeholder");
        assert_eq!(t.iterm_counts().of(ItermKey::File), ONE);
    });
}

/// **"N: N character cells"**, the height from the aspect: six cells wide
/// is 60 pixels, 80 tall, the picture resampled to them as every drawing
/// path samples, the pixel under each drawn pixel's centre.
#[test]
fn a_width_in_cells_resamples_with_the_aspect() {
    let (png, px) = picture(30, 40, 9);
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, &file("inline=1;width=6", &png), size);
            let (_, w, h, got) = placed(t);
            assert_eq!((w, h), (60, 80), "reads of {size}");
            // Twice the size: each source pixel drawn two by two.
            for (i, &p) in got.iter().enumerate() {
                let (x, y) = (i % 60, i / 60);
                assert_eq!(p, px[(y / 2) * 30 + x / 2], "({x}, {y}), reads of {size}");
            }
            assert_eq!(t.cursor(), (Row::from_index(3), Col::at_index(6)));
        });
    }
}

/// **The multipart form is the same picture**: `MultipartFile`, the base64
/// in `FilePart`s, `FileEnd`, with other output between the parts -- parts
/// of 1024 characters, and parts of 1023 and 1001 that cut the base64's
/// quanta, which iTerm2 joins before it decodes them; a `FilePart` and a
/// `FileEnd` with none open are counted.
#[test]
fn a_multipart_file_is_the_same_picture() {
    let (png, px) = picture(30, 40, 11);
    let coded = b64(&png);
    let parts = |cuts: &[usize]| {
        let mut bytes = b"\x1b]1337;MultipartFile=inline=1\x07".to_vec();
        let mut at = 0;
        let mut k = 0;
        while at < coded.len() {
            let n = cuts[k % cuts.len()].min(coded.len() - at);
            bytes.extend(b"\x1b]1337;FilePart=");
            bytes.extend(&coded[at..at + n]);
            bytes.extend(b"\x07\x1b[m");
            at += n;
            k += 1;
        }
        bytes.extend(b"\x1b]1337;FileEnd\x07");
        bytes
    };
    for bytes in [parts(&[1024]), parts(&[1023, 1001])] {
        for size in SIZES {
            with(1 << 16, |t| {
                feed(t, &bytes, size);
                let (_, w, h, got) = placed(t);
                assert_eq!((w, h), (30, 40), "reads of {size}");
                assert!(got == px, "the pixels, reads of {size}");
            });
        }
    }
    let bytes = parts(&[1024]);
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, &bytes, size);
            let (_, w, h, got) = placed(t);
            assert_eq!((w, h), (30, 40), "reads of {size}");
            assert!(got == px, "the pixels, reads of {size}");
            feed(t, b"\x1b]1337;FilePart=AAAA\x07\x1b]1337;FileEnd\x07", size);
            assert_eq!(t.iterm_counts().of(ItermKey::FilePart), ONE);
            assert_eq!(t.iterm_counts().of(ItermKey::FileEnd), ONE);
        });
    }
}

/// **"Otherwise, it will be downloaded"**: refused, counted, nothing placed
/// and the cursor where it was; so is a file past the scratch, whole.
#[test]
fn a_download_and_a_file_past_the_scratch_are_refused() {
    let (png, _) = picture(30, 40, 13);
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, &file("name=eA==", &png), size);
            assert_eq!(t.images().placements().count(), 0, "reads of {size}");
            assert_eq!(t.cursor(), (Row::from_index(0), Col::at_index(0)));
            assert_eq!(t.iterm_counts().of(ItermKey::File), ONE, "reads of {size}");
        });
        // An arena of 512 pixels lends 4096 bytes of scratch.
        with(512, |t| {
            feed(t, &file("inline=1", &png), size);
            assert_eq!(t.images().placements().count(), 0, "reads of {size}");
            assert_eq!(t.iterm_counts().of(ItermKey::File), ONE, "reads of {size}");
        });
    }
}

/// **What does not decode draws the broken-image placeholder**, iTerm2's
/// 35 by 36 pixels for `auto`, a frame and its diagonals in the foreground,
/// clear inside, and the cursor past it; counted.
#[test]
fn what_does_not_decode_draws_the_placeholder() {
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, &file("inline=1", b"not a picture at all"), size);
            let (_, w, h, got) = placed(t);
            assert_eq!((w, h), (35, 36), "reads of {size}");
            let white = 0xffff_ffff;
            assert_eq!(got[0], white, "the frame's corner");
            assert_eq!(got[35 * 36 - 1], white, "the opposite corner");
            assert_eq!(got[18 * 35 + 17], white, "the diagonals cross");
            assert_eq!(got[5 * 35 + 20], 0, "clear inside");
            assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(4)));
            assert_eq!(t.iterm_counts().of(ItermKey::File), ONE, "reads of {size}");
        });
    }
}

/// **The picture's cells take no text, and text written over one cell
/// clears that cell alone**, as iTerm2's image characters are written and
/// replaced (Jev's leans, ERASED and PER_CELL).
#[test]
fn the_cells_are_the_pictures_and_text_clears_one() {
    let (png, px) = picture(30, 40, 17);
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, b"abcdefgh\r", size);
            feed(t, &file("inline=1", &png), size);
            let row0: Vec<Content> = t.viewport().row(Row::from_index(0))[..4]
                .iter()
                .map(|c| c.content)
                .collect();
            assert_eq!(row0[..3], [Content::EMPTY; 3], "erased, reads of {size}");
            assert_eq!(
                row0[3],
                Content::codepoint(Codepoint::from_char('d')),
                "past the picture"
            );
            // `X` over the second cell of the first row.
            feed(t, b"\x1b[1;2HX", size);
            let (_, _, _, got) = placed(t);
            for y in 0..40 {
                for x in 0..30 {
                    let want = match (10..20).contains(&x) && y < 20 {
                        true => 0,
                        false => px[y * 30 + x],
                    };
                    assert_eq!(got[y * 30 + x], want, "({x}, {y}), reads of {size}");
                }
            }
        });
    }
}

thread_local! {
    /// The bytes the page-release double was handed on this thread.
    static HANDED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn count(span: &mut [u8]) -> baud_domain::PagesBack {
    HANDED.with(|h| h.set(h.get() + span.len()));
    baud_domain::PagesBack::Released
}

/// The bytes handed to the release while `f` runs.
fn handed(t: &mut Term<'_>, f: impl FnOnce(&mut Term<'_>)) -> usize {
    HANDED.with(|h| h.set(0));
    t.set_page_release(baud_domain::PageRelease::new(64, count).expect("a power of two"));
    f(t);
    HANDED.with(std::cell::Cell::get)
}

/// **The scratch a file wrote goes back, however it ends**, as kitty's
/// PNGs give it back: a picture shown hands back its bytes and the
/// scanlines decoded after them, a file past the scratch the whole of it,
/// and a file abandoned by a kitty command what it had written.
#[test]
fn the_scratch_a_file_wrote_goes_back() {
    let (png, _) = picture(30, 40, 23);
    let coded = b64(&png);
    with(1 << 16, |t| {
        let n = handed(t, |t| feed(t, &file("inline=1", &png), 4096));
        // The PNG, and 40 rows of 1 + 120 bytes of scanlines after it.
        assert!(
            n + 128 >= png.len() + 40 * 121,
            "{n} of {}",
            png.len() + 40 * 121
        );
    });
    // An arena of 512 pixels lends 4096 bytes of scratch, filled but for
    // the last decoded run that did not fit, at most 768 octets.
    with(512, |t| {
        let n = handed(t, |t| feed(t, &file("inline=1", &png), 4096));
        assert!(n + 768 + 128 >= 4096, "{n} of 4096");
    });
    with(1 << 16, |t| {
        let n = handed(t, |t| {
            feed(
                t,
                b"\x1b]1337;MultipartFile=inline=1\x07\x1b]1337;FilePart=",
                4096,
            );
            feed(t, &coded[..2000], 4096);
            feed(t, b"\x07\x1b_Ga=q,i=9,s=1,v=1,f=24;AAAA\x1b\\", 4096);
        });
        assert!(n + 128 >= 1500, "{n} of the 1500 octets written");
    });
}

/// **One scratch for two streams**: a file abandons a kitty PNG still
/// arriving in chunks, counted cancelled, and a kitty command abandons a
/// file in parts, counted under its key.
#[test]
fn a_file_and_a_kitty_png_take_the_scratch_in_turn() {
    let (png, _) = picture(30, 40, 19);
    let coded = b64(&png);
    for size in SIZES {
        with(1 << 16, |t| {
            let mut first = b"\x1b_Gf=100,m=1;".to_vec();
            first.extend(&coded[..400]);
            first.extend(b"\x1b\\");
            feed(t, &first, size);
            let before: KittyCounts = t.kitty();
            feed(t, &file("inline=1", &png), size);
            assert_eq!(t.kitty().cancelled, before.cancelled + 1, "reads of {size}");
            assert_eq!(t.images().placements().count(), 1, "the file's picture");
            feed(
                t,
                b"\x1b]1337;MultipartFile=inline=1\x07\x1b]1337;FilePart=",
                size,
            );
            feed(t, &coded[..400], size);
            feed(t, b"\x07\x1b_Ga=q,i=9,s=1,v=1,f=24;AAAA\x1b\\", size);
            assert_eq!(
                t.iterm_counts().of(ItermKey::MultipartFile),
                ONE,
                "reads of {size}"
            );
            feed(t, b"\x1b]1337;FileEnd\x07", size);
            assert_eq!(t.iterm_counts().of(ItermKey::FileEnd), ONE, "none open");
        });
    }
}
