//! The Sixel decode-agreement gate: hand-authored Sixel byte strings decode to
//! the exact pixels their grammar prescribes. It is the first of the three
//! acceptance gates Sixel had before any parser code, and it is the `decode_agrees.rs` shape -- a decoder checked against a
//! fixture it cannot argue with -- for `[[source: vt3xx-sixel]]`.
//!
//! Every token is cited to that source rather than written from memory. The
//! data byte's bit order in particular is the source's own: `t` (`0x74`) is
//! the binary value `0x74 - 0x3F = 0b110101` with the least significant bit at
//! the top, so its six vertical pixels read `[1,0,1,0,1,1]` top to bottom.

use baud_domain::{
    InputEncoding, Pixel, Sixel, SixelAspect, SixelCanvas, SixelFit, SixelPx, SixelReport,
    StreamByte,
};

/// The aspect the bodies here are decoded at unless a test says otherwise:
/// what an envelope's `P1 = 9` selects, 1:1 (`[[source: vt3xx-sixel]]`;
/// `[[source: dec-std-070]]` 070-9 §7.1), so a band is six pixels tall.
const SQUARE: SixelAspect = SixelAspect::ONE_TO_ONE;

/// Read the pixel at column `x`, row `y` of a `w`-wide image.
fn px(image: &[Pixel], w: u16, x: u16, y: u16) -> Pixel {
    image[usize::from(y) * usize::from(w) + usize::from(x)]
}

fn rgb(c: Pixel) -> Option<(u8, u8, u8)> {
    let (r, g, b) = c.rgb();
    (c != Pixel::CLEAR).then_some((r.get(), g.get(), b.get()))
}

#[test]
fn a_data_byte_sets_six_vertical_pixels_least_significant_at_the_top() {
    // `#0;2;100;0;0` defines and selects register 0 as RGB red (100% -> 255);
    // `t` paints it. RGB `Pu = 2` per [[source: vt3xx-sixel]].
    let mut image = [Pixel::CLEAR; 6];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(1));
    let report = Sixel::decode(b"#0;2;100;0;0t", SQUARE, &mut canvas);

    let red = Some((255, 0, 0));
    // `t` = 0b110101, LSB at the top: rows 0,2,4,5 set, rows 1,3 clear.
    assert_eq!(rgb(px(&image, 1, 0, 0)), red, "row 0 (bit 0) is set");
    assert!(
        px(&image, 1, 0, 1) == Pixel::CLEAR,
        "row 1 (bit 1) is clear"
    );
    assert_eq!(rgb(px(&image, 1, 0, 2)), red, "row 2 (bit 2) is set");
    assert!(
        px(&image, 1, 0, 3) == Pixel::CLEAR,
        "row 3 (bit 3) is clear"
    );
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
    let mut image = [Pixel::CLEAR; 12];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(2));
    let report = Sixel::decode(b"#0;2;100;0;0~?", SQUARE, &mut canvas);

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
    let mut image = [Pixel::CLEAR; 12];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(2));
    // column 0: `?` sets nothing; column 1: `~` sets green (0;100;0).
    let report = Sixel::decode(b"#0;2;0;100;0?~", SQUARE, &mut canvas);

    let green = Some((0, 255, 0));
    for y in 0..6 {
        assert!(
            px(&image, 2, 0, y) == Pixel::CLEAR,
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
    let mut image = [Pixel::CLEAR; 18];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(3));
    let report = Sixel::decode(b"#0;2;0;0;100!3~", SQUARE, &mut canvas);

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
    // one column, as `!1~` and a bare `~` are, and not none. DEC's printer
    // reference says it of an omitted count too, so `!~` is one column:
    // "If the repeat count is zero or is omitted, it is treated as a repeat
    // count of one" (`[[source: dec-pplv2-pm]]` p. 5-26, PDF 80).
    for body in [
        b"#0;2;0;0;100!0~".as_slice(),
        b"#0;2;0;0;100!~",
        b"#0;2;0;0;100!1~",
        b"#0;2;0;0;100~",
    ] {
        let report = Sixel::extent(body, SQUARE);
        assert_eq!(report.width, SixelPx::of(1), "{body:?} is one column wide");
    }
}

#[test]
fn graphics_new_line_drops_one_six_pixel_band() {
    // `~` (red band), `-` (new line), `~` (green band): a 1x12 image with the
    // top band red and the bottom band green. `$` would return to the left of
    // the same band; `-` returns and drops six rows.
    let mut image = [Pixel::CLEAR; 12];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(1));
    let report = Sixel::decode(b"#0;2;100;0;0~-#0;2;0;100;0~", SQUARE, &mut canvas);

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
    let mut image = [Pixel::CLEAR; 6];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(1));
    Sixel::decode(b"#0;2;100;0;0~$?", SQUARE, &mut canvas);

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
    let mut image = [Pixel::CLEAR; 12];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(2));
    let report = Sixel::decode(b"#0;2;100;0;0!5~", SQUARE, &mut canvas);

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

/// The colour one all-set column of `body` paints, and the decode's report.
fn one_column(body: &[u8]) -> (Pixel, SixelReport) {
    let mut image = [Pixel::CLEAR; 6];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(1));
    let report = Sixel::decode(body, SQUARE, &mut canvas);
    for c in image {
        assert_eq!(c, image[0], "{body:?}: one colour down the column");
    }
    (image[0], report)
}

#[test]
fn the_hls_hue_origin_is_blue_at_zero_red_at_120_green_at_240() {
    // `[[source: vt3xx-sixel]]` gives the hue as "0 to 360 degrees" and
    // defers to chapter 2's "Output Mapping", which places it:
    // `[[source: vt3xx-screen-control]]`, "The hue angles for the three
    // primary colors are as follows. Blue: 0 degrees Red: 120 degrees Green:
    // 240 degrees". At lightness 50 and saturation 100 each is the pure
    // primary, and 360 is 0 again.
    for (body, want, name) in [
        (b"#0;1;0;50;100~".as_slice(), (0, 0, 255), "blue at 0"),
        (b"#0;1;120;50;100~", (255, 0, 0), "red at 120"),
        (b"#0;1;240;50;100~", (0, 255, 0), "green at 240"),
        (b"#0;1;360;50;100~", (0, 0, 255), "blue at 360"),
    ] {
        let (c, report) = one_column(body);
        assert_eq!(rgb(c), Some(want), "{name}");
        assert_eq!(report.unapplied_registers, 0, "{name}: applied");
    }
}

#[test]
fn hls_defines_reproduce_the_vt340_default_colour_map() {
    // `[[source: vt3xx-screen-control]]` Table 2-3, "VT340 Default Color
    // Map": each row's HLS triple beside its RGB percentages. The table is in
    // whole percents and on six rows sits above this conversion by up to 0.88
    // of a point, more than rounding explains, so each channel must be within
    // one percent of the table: |c / 255 * 100 - p| <= 1, kept in integers as
    // |100 c - 255 p| <= 255.
    let table_2_3 = [
        ("Black", [0, 0, 0], [0, 0, 0]),
        ("Blue", [0, 50, 60], [20, 20, 80]),
        ("Red", [120, 46, 72], [80, 13, 13]),
        ("Green", [240, 50, 60], [20, 80, 20]),
        ("Magenta", [60, 50, 60], [80, 20, 80]),
        ("Cyan", [300, 50, 60], [20, 80, 80]),
        ("Yellow", [180, 50, 60], [80, 80, 20]),
        ("Gray 50%", [0, 53, 0], [53, 53, 53]),
        ("Gray 25%", [0, 26, 0], [26, 26, 26]),
        ("Blue*", [0, 46, 29], [33, 33, 60]),
        ("Red*", [120, 43, 39], [60, 26, 26]),
        ("Green*", [240, 46, 29], [33, 60, 33]),
        ("Magenta*", [60, 46, 29], [60, 33, 60]),
        ("Cyan*", [300, 46, 29], [33, 60, 60]),
        ("Yellow*", [180, 46, 29], [60, 60, 33]),
        ("Gray 75%", [0, 80, 0], [80, 80, 80]),
    ];
    for (name, [h, l, s], want) in table_2_3 {
        let body = format!("#0;1;{h};{l};{s}~");
        let (c, report) = one_column(body.as_bytes());
        assert_eq!(report.unapplied_registers, 0, "{name}: applied");
        let Some((r, g, b)) = rgb(c) else {
            panic!("{name}: an HLS define paints an RGB pixel, got {c:?}");
        };
        for (got, p, chan) in [(r, want[0], "R"), (g, want[1], "G"), (b, want[2], "B")] {
            let off = (100 * i32::from(got) - 255 * p).abs();
            assert!(
                off <= 255,
                "{name} {chan}: {got}/255 against the table's {p}%"
            );
        }
    }
}

#[test]
fn an_hls_hue_between_the_table_rows_follows_the_double_hexcone() {
    // Chapter 2 names H150 L50 S60 "gold" and H60 L80 S60 "plum"
    // (`[[source: vt3xx-screen-control]]`) and gives neither an RGB value.
    // The conversion is `[[source: css-color-4-hsl]]`'s, with DEC's hue
    // turned 120 degrees: gold is CSS hue 30, R 80% G 50% B 20%, and plum
    // CSS hue 300, R 92% G 68% B 92%, each channel rounded half up from
    // 255 times its fraction.
    let (gold, _) = one_column(b"#0;1;150;50;60~");
    assert_eq!(rgb(gold), Some((204, 128, 51)), "gold");
    let (plum, _) = one_column(b"#0;1;60;80;60~");
    assert_eq!(rgb(plum), Some((235, 173, 235)), "plum");
}

#[test]
fn an_hls_define_out_of_range_is_ignored_whole() {
    // `[[source: dec-std-070]]` 070-9 §8.1.3: "Values for Pu, Px, Py, and Pz
    // other than those specified above cause the entire color specifier
    // command to be ignored, including the color select parameter, Pc" (PDF
    // p. 921), the ranges being hue 0-360 and lightness and saturation 0-100.
    // So after register 1 is defined green and selected, a define of
    // register 5 with any value past its range neither defines 5 nor selects
    // it, and the data draws green; it is counted as unapplied.
    for bad in [
        b"#5;1;361;50;100".as_slice(),
        b"#5;1;0;101;100",
        b"#5;1;0;50;101",
    ] {
        let mut body = b"#1;2;0;100;0".to_vec();
        body.extend_from_slice(bad);
        body.push(b'~');
        let (c, report) = one_column(&body);
        assert_eq!(
            rgb(c),
            Some((0, 255, 0)),
            "{bad:?}: register 1 still selected"
        );
        assert_eq!(report.unapplied_registers, 1, "{bad:?}: counted");
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
    let mut image = [Pixel::CLEAR; 24]; // 4 wide, one six-pixel band
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(4));
    let report = Sixel::decode(
        b"\"1;1;4;4#0;2;100;0;0#1;2;0;100;0#2;2;0;0;100#0NLJN$#1?A#2C-",
        // ImageMagick's envelope is `ESC P 0;1;0 q`, `P1 = 0`, 2:1, which
        // the raster command's `1;1` overrides.
        SixelAspect::from_macro(Some(0)),
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
    assert!(px(&image, 4, 0, 4) == Pixel::CLEAR, "row 4 is untouched");

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
        let mut image = [Pixel::CLEAR; 64];
        let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(8));
        let painted = Sixel::decode(data, SQUARE, &mut canvas);
        let measured = Sixel::extent(data, SQUARE);
        assert_eq!(painted.width, measured.width, "width disagrees on {data:?}");
        assert_eq!(
            painted.height, measured.height,
            "height disagrees on {data:?}"
        );
    }
}

#[test]
fn an_extent_past_65535_is_reported_too_large_not_stopped() {
    // A coordinate is a `u16`. 65535 columns, and a band of 65532 rows at a
    // ratio of 10922, fit; one more column, or a ratio whose band passes
    // 65535, does not, and the report says so rather than stating 65535 as
    // the image's extent.
    let cases: [(&[u8], SixelFit); 4] = [
        (b"#0;2;100;0;0!65535~", SixelFit::Fits),
        (b"#0;2;100;0;0!65535~!1~", SixelFit::TooLarge),
        (b"\"10922;1#0;2;100;0;0~", SixelFit::Fits),
        (b"\"10923;1#0;2;100;0;0~", SixelFit::TooLarge),
    ];
    for (body, fit) in cases {
        assert_eq!(Sixel::extent(body, SQUARE).fit, fit, "{body:?}");
    }
}

#[test]
fn raster_attributes_are_consumed_and_do_not_corrupt_the_data() {
    // `"1;1;10;6` is the raster attributes (aspect 1:1, size 10x6); a decoder
    // consumes them and paints the data that follows unchanged.
    let mut image = [Pixel::CLEAR; 6];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(1));
    let report = Sixel::decode(b"\"1;1;10;6#0;2;100;0;0~", SQUARE, &mut canvas);

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

#[test]
fn the_macro_parameter_sets_the_aspect_where_both_dec_texts_make_it_whole() {
    // `[[source: vt3xx-sixel]]`: omitted "2:1 (default)", 0 and 1 2:1, 3 and
    // 4 3:1, 5 and 6 2:1, 7 to 9 1:1. `[[source: dec-std-070]]` 070-9 §7.1:
    // 0, 1 and 5 200:100, 2 450:100, 3 300:100, 4 250:100, 6 150:100, 7
    // 130:100, 8 112:100, 9 100:100 (PDF p. 918). Where both are whole they
    // agree. **Where 070-9's is not whole it is preserved**, a band six times
    // the ratio tall rounded once, 27, 15, 9, 8 and 7 rows, where the VT300
    // rounds each row to 5, 3, 2, 1 and 1; all five were 6, drawn 1:1. **A
    // `P1` past 9 is omitted**, 12 (070-3 §3.5.1.3); it was 6.
    let cases = [
        (None, 12),
        (Some(0), 12),
        (Some(1), 12),
        (Some(5), 12),
        (Some(3), 18),
        (Some(9), 6),
        (Some(2), 27),
        (Some(4), 15),
        (Some(6), 9),
        (Some(7), 8),
        (Some(8), 7),
        (Some(10), 12),
        (Some(65535), 12),
    ];
    for (p1, rows) in cases {
        let report = Sixel::extent(b"#0;2;100;0;0~", SixelAspect::from_macro(p1));
        assert_eq!(report.height, SixelPx::of(rows), "P1 {p1:?}");
    }
}

#[test]
fn a_raster_ratio_before_the_data_overrides_the_macro() {
    // `[[source: vt3xx-sixel]]`: "The " command overrides any raster
    // attributes set by the macro parameter"; `[[source: dec-std-070]]` 070-9
    // §8.1.2: it "must precede any Sixel, or any sixel positioning command
    // ... or the command will be ignored", and omitted or zero `Pan`, `Pad`
    // "default to '1' (one)" (PDF p. 921). `"3;2` is 1.5:1, a band of 9 rows;
    // `"1;2`, below one, is 1:1 (070-9 §12's smallest imageable unit); both
    // were 6 until 2026-10-23.
    let two = SixelAspect::from_macro(None);
    let cases: [(&[u8], u16); 11] = [
        (b"\"1;1#0;2;100;0;0~", 6),
        (b"\"3;1#0;2;100;0;0~", 18),
        (b"\"6;3#0;2;100;0;0~", 12),
        (b"\";#0;2;100;0;0~", 6),
        (b"\"0;0#0;2;100;0;0~", 6),
        (b"\"2#0;2;100;0;0~", 12),
        (b"\"3;2#0;2;100;0;0~", 9),
        (b"\"1;2#0;2;100;0;0~", 6),
        (b"\"31;30#0;2;100;0;0~", 6),
        (b"#0;2;100;0;0~\"3;1~", 12),
        (b"#0;2;100;0;0$\"3;1~", 12),
    ];
    for (body, rows) in cases {
        let report = Sixel::extent(body, two);
        assert_eq!(report.height, SixelPx::of(rows), "{body:?}");
    }
}

#[test]
fn at_two_to_one_each_sixel_row_is_two_pixels_and_a_band_twelve() {
    // `t` is `0b110101`, top to bottom 1,0,1,0,1,1 (`[[source: vt3xx-sixel]]`)
    // and at 2:1 each row is two pixels; `-` moves down a band, "six actual
    // grid units" (`[[source: dec-std-070]]` 070-9 §8.1.5), twelve pixels
    // here, so the second band's `~` fills rows 12 to 23.
    let mut image = [Pixel::CLEAR; 24];
    let mut canvas = SixelCanvas::new(&mut image, SixelPx::of(1));
    let report = Sixel::decode(
        b"#0;2;100;0;0t-~",
        SixelAspect::from_macro(None),
        &mut canvas,
    );
    let lit: Vec<bool> = image.iter().map(|c| *c != Pixel::CLEAR).collect();
    let mut want = vec![
        true, true, false, false, true, true, false, false, true, true, true, true,
    ];
    want.extend([true; 12]);
    assert_eq!(lit, want);
    assert_eq!(report.height, SixelPx::of(24));
    assert_eq!(report.refused, 0);
    let measured = Sixel::extent(b"#0;2;100;0;0t-~", SixelAspect::from_macro(None));
    assert_eq!(
        (measured.width, measured.height),
        (report.width, report.height)
    );
}

/// **What a Sixel's handler keeps of each body byte, all 512 (encoding,
/// byte) pairs, against ranges written out here rather than computed
/// through `gr_as_gl`.** `[[source: dec-std-070]]` 070-9 §9.1 drops the C0,
/// §9.2 "2/0 (SPACE) Ignored ... 7/15 (DELETE) Ignored", and §9.4 makes a GR
/// code its GL code by clearing the eighth bit, so under ISO 8859-1 `A0` and
/// `FF` are dropped and `A1-FE` kept as `21-7E`. Under UTF-8 `80-FF` are a
/// character's bytes and are kept as they came, as are `80-9F` under ISO
/// 8859-1, which the graph takes as C1 controls before a body sees them.
#[test]
fn a_sixel_body_byte_is_kept_folded_or_dropped_as_070_9_section_9_says() {
    let mut kept = [0usize; 2];
    for (e, enc) in [InputEncoding::Utf8, InputEncoding::Latin1]
        .into_iter()
        .enumerate()
    {
        for v in 0..=255u8 {
            let want = match (enc, v) {
                (_, 0x00..=0x20) | (_, 0x7f) => None,
                (_, 0x21..=0x7e) | (_, 0x80..=0x9f) => Some(v),
                (InputEncoding::Utf8, 0xa0..=0xff) => Some(v),
                (InputEncoding::Latin1, 0xa0) | (InputEncoding::Latin1, 0xff) => None,
                (InputEncoding::Latin1, 0xa1..=0xfe) => Some(v - 0x80),
            };
            let got = Sixel::body_byte(enc, StreamByte::from_stream(v)).map(StreamByte::raw_byte);
            assert_eq!(got, want, "{enc:?}, {v:#04x}");
            kept[e] += usize::from(got.is_some());
        }
    }
    // The positive control: each encoding keeps what the ranges say, so the
    // loop ran over bytes that are kept as well as dropped.
    assert_eq!(kept, [94 + 32 + 96, 94 + 32 + 94], "kept per encoding");
}

/// **The per-byte test a handler stores on is `21-7F` exactly**, and every
/// byte in it but `DEL`, which the graph ignores before a body handler sees
/// it, is one `body_byte` keeps as it is under both encodings. A store that
/// trusted the test past that range would keep a byte `body_byte` drops or
/// folds.
#[test]
fn the_inline_sixel_body_test_is_21_to_7f_and_keeps_what_body_byte_keeps() {
    let mut inside = 0;
    for v in 0..=255u8 {
        let b = StreamByte::from_stream(v);
        let fast = Sixel::body_byte_is_gl_past_space(b);
        assert_eq!(fast, (0x21..=0x7f).contains(&v), "{v:#04x}");
        if fast && v != 0x7f {
            inside += 1;
            for enc in [InputEncoding::Utf8, InputEncoding::Latin1] {
                let kept = Sixel::body_byte(enc, b).map(StreamByte::raw_byte);
                assert_eq!(kept, Some(v), "{enc:?}, {v:#04x}");
            }
        }
    }
    assert_eq!(inside, 94, "control: 21-7E");
}
