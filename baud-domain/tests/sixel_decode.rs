//! The Sixel decode-agreement gate: hand-authored Sixel byte strings decode to
//! the exact pixels their grammar prescribes. It is the first of the three
//! acceptance gates `docs/research/notes/s-sixel.md` fixes before any parser
//! code, and it is the `decode_agrees.rs` shape -- a decoder checked against a
//! fixture it cannot argue with -- for `[[source: vt3xx-sixel]]`.
//!
//! Every token is cited to that source rather than written from memory. The
//! data byte's bit order in particular is the source's own: `t` (`0x74`) is
//! the binary value `0x74 - 0x3F = 0b110101` with the least significant bit at
//! the top, so its six vertical pixels read `[1,0,1,0,1,1]` top to bottom.

use baud_domain::{ColorWord, Sixel, SixelCanvas, SixelPx};

/// Read the pixel at column `x`, row `y` of a `w`-wide image.
fn px(image: &[ColorWord], w: u16, x: u16, y: u16) -> ColorWord {
    image[usize::from(y) * usize::from(w) + usize::from(x)]
}

fn rgb(c: ColorWord) -> Option<(u8, u8, u8)> {
    c.as_rgb()
}

#[test]
fn a_data_byte_sets_six_vertical_pixels_least_significant_at_the_top() {
    // `#0;2;100;0;0` defines and selects register 0 as RGB red (100% -> 255);
    // `t` paints it. RGB `Pu = 2` per [[source: vt3xx-sixel]].
    let mut image = [ColorWord::DEFAULT; 6];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(1));
    let report = Sixel::decode(b"#0;2;100;0;0t", &mut canvas);

    let red = Some((255, 0, 0));
    // `t` = 0b110101, LSB at the top: rows 0,2,4,5 set, rows 1,3 clear.
    assert_eq!(rgb(px(&image, 1, 0, 0)), red, "row 0 (bit 0) is set");
    assert!(px(&image, 1, 0, 1).is_default(), "row 1 (bit 1) is clear");
    assert_eq!(rgb(px(&image, 1, 0, 2)), red, "row 2 (bit 2) is set");
    assert!(px(&image, 1, 0, 3).is_default(), "row 3 (bit 3) is clear");
    assert_eq!(rgb(px(&image, 1, 0, 4)), red, "row 4 (bit 4) is set");
    assert_eq!(rgb(px(&image, 1, 0, 5)), red, "row 5 (bit 5) is set");

    assert_eq!(report.width, SixelPx::of(1));
    assert_eq!(report.height, SixelPx::of(6));
    assert_eq!(report.refused, 0);
    assert_eq!(report.unapplied_registers, 0);
}

#[test]
fn the_extent_is_where_the_data_reached_not_where_pixels_were_set() {
    // `~` (all six) then `?` (none): the second byte paints nothing but still
    // advances the cursor a column, so the width is 2, not the painted 1. This
    // is the extent mode 8452 places the next text past.
    let mut image = [ColorWord::DEFAULT; 12];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(2));
    let report = Sixel::decode(b"#0;2;100;0;0~?", &mut canvas);

    assert_eq!(
        report.width,
        SixelPx::of(2),
        "a trailing clear column still counts"
    );
    assert_eq!(report.height, SixelPx::of(6));
}

#[test]
fn the_extremes_are_none_and_all_six() {
    // `?` (0x3F) is 0b000000 -- no pixels; `~` (0x7E) is 0b111111 -- all six.
    let mut image = [ColorWord::DEFAULT; 12];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(2));
    // column 0: `?` sets nothing; column 1: `~` sets green (0;100;0).
    let report = Sixel::decode(b"#0;2;0;100;0?~", &mut canvas);

    let green = Some((0, 255, 0));
    for y in 0..6 {
        assert!(
            px(&image, 2, 0, y).is_default(),
            "column 0 stays clear at row {y}"
        );
        assert_eq!(
            rgb(px(&image, 2, 1, y)),
            green,
            "column 1 is green at row {y}"
        );
    }
    assert_eq!(report.width, SixelPx::of(2));
    assert_eq!(report.height, SixelPx::of(6));
}

#[test]
fn the_repeat_introducer_draws_the_next_byte_that_many_columns() {
    // `!3~` is three columns of all-six-set, in blue (0;0;100).
    let mut image = [ColorWord::DEFAULT; 18];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(3));
    let report = Sixel::decode(b"#0;2;0;0;100!3~", &mut canvas);

    let blue = Some((0, 0, 255));
    for x in 0..3 {
        for y in 0..6 {
            assert_eq!(rgb(px(&image, 3, x, y)), blue, "({x},{y}) is blue");
        }
    }
    assert_eq!(report.width, SixelPx::of(3));
    assert_eq!(report.refused, 0);
}

#[test]
fn a_zero_repeat_draws_one_column() {
    // `[[source: vt3xx-sixel]]` says only that the count "can be any decimal
    // value"; `[[source: dec-std-070]]` 070-9 §8.1.1 settles zero: "A repeat
    // count of zero implies a repeat count of 1" (PDF p. 920). So `!0~` is
    // one column, as `!1~` and a bare `~` are, and not none.
    for body in [
        b"#0;2;0;0;100!0~".as_slice(),
        b"#0;2;0;0;100!1~",
        b"#0;2;0;0;100~",
    ] {
        let report = Sixel::extent(body);
        assert_eq!(report.width, SixelPx::of(1), "{body:?} is one column wide");
    }
}

#[test]
fn graphics_new_line_drops_one_six_pixel_band() {
    // `~` (red band), `-` (new line), `~` (green band): a 1x12 image with the
    // top band red and the bottom band green. `$` would return to the left of
    // the same band; `-` returns and drops six rows.
    let mut image = [ColorWord::DEFAULT; 12];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(1));
    let report = Sixel::decode(b"#0;2;100;0;0~-#0;2;0;100;0~", &mut canvas);

    let red = Some((255, 0, 0));
    let green = Some((0, 255, 0));
    for y in 0..6 {
        assert_eq!(rgb(px(&image, 1, 0, y)), red, "top band red at row {y}");
    }
    for y in 6..12 {
        assert_eq!(
            rgb(px(&image, 1, 0, y)),
            green,
            "bottom band green at row {y}"
        );
    }
    assert_eq!(report.height, SixelPx::of(12));
}

#[test]
fn graphics_carriage_return_overlays_the_same_band() {
    // `~` fills column 0 red, `$` returns to the left, `?` leaves it -- so the
    // overlay does not erase, it composites, and the red survives.
    let mut image = [ColorWord::DEFAULT; 6];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(1));
    Sixel::decode(b"#0;2;100;0;0~$?", &mut canvas);

    let red = Some((255, 0, 0));
    for y in 0..6 {
        assert_eq!(
            rgb(px(&image, 1, 0, y)),
            red,
            "the returned band keeps its red at row {y}"
        );
    }
}

#[test]
fn pixels_past_the_canvas_are_refused_and_counted_never_wrapped() {
    // Five all-set columns into a two-wide canvas: columns 0 and 1 land (12
    // pixels), columns 2, 3 and 4 are refused (18 pixels). Nothing wraps.
    let mut image = [ColorWord::DEFAULT; 12];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(2));
    let report = Sixel::decode(b"#0;2;100;0;0!5~", &mut canvas);

    let red = Some((255, 0, 0));
    for x in 0..2 {
        for y in 0..6 {
            assert_eq!(rgb(px(&image, 2, x, y)), red, "({x},{y}) landed");
        }
    }
    assert_eq!(
        report.width,
        SixelPx::of(5),
        "the extent is what the data described, independent of the canvas"
    );
    assert_eq!(
        report.refused, 18,
        "three columns of six pixels fell outside the two-wide canvas"
    );
}

#[test]
fn an_hls_register_is_counted_and_left_unapplied() {
    // `#0;1;...` defines register 0 in HLS (`Pu = 1`), whose hue origin this
    // source does not pin, so the register is not painted and the pixel stays
    // transparent. The count is how a caller learns the image is incomplete.
    let mut image = [ColorWord::DEFAULT; 6];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(1));
    let report = Sixel::decode(b"#0;1;120;50;100~", &mut canvas);

    assert_eq!(report.unapplied_registers, 1, "the HLS define is counted");
    for y in 0..6 {
        assert!(
            px(&image, 1, 0, y).is_default(),
            "an unapplied register paints nothing at row {y}"
        );
    }
}

#[test]
fn a_real_imagemagick_sixel_decodes_with_nothing_refused_or_unapplied() {
    // The bytes after the `q` of a real Sixel emitted by ImageMagick
    // (`magick -size 4x4 xc:red -fill lime -draw 'point 1,1' -fill blue
    // -draw 'point 2,2' sixel:-`): a red 4x4 with a green pixel at (1,1) and
    // a blue one at (2,2). A mainstream encoder's output, not a hand-authored
    // payload -- the "use real recordings" discipline for the decode gate --
    // and it exercises only tokens the fixtures above cover: `"` raster
    // attributes, `#Pc;2;...` RGB defines, `#Pc` selects, data bytes, `$`, `-`.
    let mut image = [ColorWord::DEFAULT; 24]; // 4 wide, one six-pixel band
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(4));
    let report = Sixel::decode(
        b"\"1;1;4;4#0;2;100;0;0#1;2;0;100;0#2;2;0;0;100#0NLJN$#1?A#2C-",
        &mut canvas,
    );

    let red = Some((255, 0, 0));
    let green = Some((0, 255, 0));
    let blue = Some((0, 0, 255));
    // The corners and edges are red; the two drawn points overlay it.
    assert_eq!(rgb(px(&image, 4, 0, 0)), red, "top-left is red");
    assert_eq!(rgb(px(&image, 4, 3, 3)), red, "bottom-right is red");
    assert_eq!(rgb(px(&image, 4, 2, 1)), red, "an interior cell stays red");
    assert_eq!(rgb(px(&image, 4, 1, 1)), green, "the lime point at (1,1)");
    assert_eq!(rgb(px(&image, 4, 2, 2)), blue, "the blue point at (2,2)");
    // Rows 4 and 5 of the band are past the 4-pixel image and never painted.
    assert!(px(&image, 4, 0, 4).is_default(), "row 4 is untouched");

    assert_eq!(
        report.width,
        SixelPx::of(4),
        "the image is four pixels wide"
    );
    assert_eq!(report.height, SixelPx::of(6), "one six-pixel band");
    assert_eq!(report.refused, 0, "a real encoder's output fits its canvas");
    assert_eq!(
        report.unapplied_registers, 0,
        "ImageMagick emits RGB, not HLS"
    );
}

#[test]
fn the_measure_pass_agrees_with_the_paint_pass_on_the_extent() {
    // `Sixel::extent` is the same walk as `decode` with no canvas, so a size
    // reserved from it is exactly what `decode` fills. Every payload the
    // fixtures above exercise -- a trailing clear column, a repeat, a new line,
    // an over-wide run past a canvas -- must give the same width and height
    // measured as painted.
    let cases: &[&[u8]] = &[
        b"#0;2;100;0;0t",
        b"#0;2;100;0;0~?",
        b"#0;2;0;0;100!3~",
        b"#0;2;100;0;0~-#0;2;0;100;0~",
        b"#0;2;100;0;0!5~",
        b"\"1;1;10;6#0;2;100;0;0~",
    ];
    for data in cases {
        // Paint into a canvas large enough that the extent is the data's.
        let mut image = [ColorWord::DEFAULT; 64];
        let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(8));
        let painted = Sixel::decode(data, &mut canvas);
        let measured = Sixel::extent(data);
        assert_eq!(painted.width, measured.width, "width disagrees on {data:?}");
        assert_eq!(
            painted.height, measured.height,
            "height disagrees on {data:?}"
        );
    }
}

#[test]
fn raster_attributes_are_consumed_and_do_not_corrupt_the_data() {
    // `"1;1;10;6` is the raster attributes (aspect 1:1, size 10x6); a decoder
    // consumes them and paints the data that follows unchanged.
    let mut image = [ColorWord::DEFAULT; 6];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(1));
    let report = Sixel::decode(b"\"1;1;10;6#0;2;100;0;0~", &mut canvas);

    let red = Some((255, 0, 0));
    for y in 0..6 {
        assert_eq!(
            rgb(px(&image, 1, 0, y)),
            red,
            "the data after the raster attributes drew at row {y}"
        );
    }
    assert_eq!(report.width, SixelPx::of(1));
}
