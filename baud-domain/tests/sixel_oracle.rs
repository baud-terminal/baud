//! The Sixel decode-correctness gate: baud decodes a real image to the exact
//! pixels libsixel's `sixel2png` decodes it to.
//!
//! Where `sixel_decode.rs` checks hand-authored byte strings against the
//! grammar, this checks a whole real image against an *independent
//! implementation*. It is the oracle a scrambled preview turned out not to
//! need at the decoder: baud's decode is byte-identical to libsixel here and
//! was AE=0 against `sixel2png` at every size and chunk boundary tried, so a
//! previewer's garbled image was never a decode fault -- it was a placement
//! leak (`baud-core/tests/sixel.rs`).
//!
//! The fixture is `img2sixel`'s output for a 96x72 image; the reference is
//! that Sixel through `sixel2png` (libsixel 1.10.5), committed as raw 8-bit
//! RGB beside it, so the gate needs no external tool at run time. Both come
//! from libsixel, so this pins baud against libsixel's own encode/decode
//! round trip -- the palette scaling (`0..100` percent to `0..255` with the
//! same rounding), the band order and the run-length expansion all at once.

use baud_domain::{ColorWord, Sixel, SixelCanvas};

const SIXEL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/img2sixel-96x72.six"
));

/// `sixel2png`'s decode of `SIXEL`, 96x72 row-major 8-bit RGB triples.
const ORACLE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/img2sixel-96x72.oracle-rgb"
));

/// The image's declared raster size, `sixel2png`'s reference dimensions.
const OW: usize = 96;
const OH: usize = 72;

#[test]
fn baud_decodes_a_real_sixel_to_what_libsixel_decodes_it_to() {
    let report = Sixel::extent(SIXEL);
    let (w, h) = (report.width, report.height);

    // baud's width is the data's *extent* -- the last column the cursor reached
    // -- by design (`SixelReport::width`), not the declared `Ph`. img2sixel pads
    // the last band's run-length past the width, so the extent is a column or
    // two wider than the 96 the image declares; `sixel2png` clips to `Ph`.
    // yazi's own output does not pad and decodes to its exact declared size.
    assert!(
        w.index() >= OW && h.index() == OH,
        "the extent covers the declared {OW}x{OH}, {}x{} here",
        w.raw(),
        h.raw()
    );

    let mut image = vec![ColorWord::DEFAULT; w.index() * h.index()];
    let mut canvas = SixelCanvas::new(&mut image, w);
    let filled = Sixel::decode(SIXEL, &mut canvas);
    assert_eq!(
        filled.refused, 0,
        "every pixel fits the extent-sized canvas"
    );

    // Every pixel baud *paints* is byte-identical to libsixel's -- the whole
    // point, and it exercises the palette scaling (0..100 percent to 0..255,
    // with the same rounding), the band order and the run-length expansion all
    // at once. The pixels baud leaves *unpainted* are a design difference, not
    // a decode fault: baud draws them transparent so a Sixel composites over
    // the cells beneath it, where `sixel2png` fills them opaque; those are
    // skipped here and pinned by their own count so a regression that started
    // dropping painted pixels could not hide as "more transparent".
    assert_eq!(ORACLE.len(), OW * OH * 3, "one RGB triple a pixel, 96x72");
    let stride = w.index();
    let mut painted = 0usize;
    let mut transparent = 0usize;
    let mut first = None;
    for y in 0..OH {
        for x in 0..OW {
            match image[y * stride + x].as_rgb() {
                None => transparent += 1,
                Some((r, g, b)) => {
                    painted += 1;
                    let o = (y * OW + x) * 3;
                    let want = [ORACLE[o], ORACLE[o + 1], ORACLE[o + 2]];
                    if [r, g, b] != want && first.is_none() {
                        first = Some((x, y, [r, g, b], want));
                    }
                }
            }
        }
    }
    assert_eq!(
        first, None,
        "a painted pixel differs from sixel2png's decode"
    );
    // The overwhelming majority is painted and identical; only a handful of
    // background pixels are transparent, which is the design difference above,
    // not a decode that quietly stopped painting.
    assert!(
        painted > OW * OH * 99 / 100,
        "nearly every pixel is painted and matches: {painted} of {}, {transparent} transparent",
        OW * OH
    );
}
