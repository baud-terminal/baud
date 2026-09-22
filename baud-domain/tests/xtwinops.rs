//! XTWINOPS `CSI 14 t`, `CSI 16 t` and `CSI 18 t`: the three questions about
//! the text area's size, parsed to named queries and answered in
//! `[[source: ctlseqs]]`'s shapes.
//!
//! The grammar and the encoding only. Which of them the core answers, and
//! when, is `baud-core/tests/image_geometry.rs`.

use baud_domain::*;

/// A parameter pushed digit by digit, the way the stream spells it.
fn param(n: u16) -> Option<Param> {
    let mut p = Param::ZERO;
    for d in n.to_string().bytes() {
        p = p.push(StreamByte::from_stream(d).as_ascii_digit().unwrap());
    }
    Some(p)
}

fn parse(marker: Marker, inter: Option<u8>, params: &[Option<Param>], final_byte: u8) -> Csi {
    Csi::parse(
        marker,
        inter.map(StreamByte::from_stream),
        Params::new(params),
        FinalByte::from_stream(StreamByte::from_stream(final_byte)).unwrap(),
    )
}

fn enc(r: Report) -> Vec<u8> {
    r.encode(C1Transmit::SevenBit).as_bytes().to_vec()
}

#[test]
fn the_three_reports_parse_to_their_queries() {
    for (ps, want) in [
        (14, WindowOp::TextAreaPixels),
        (16, WindowOp::CellPixels),
        (18, WindowOp::TextAreaChars),
    ] {
        match parse(Marker::Standard, None, &[param(ps)], b't') {
            Csi::Window(got) => assert_eq!(got, want, "CSI {ps} t"),
            _ => panic!("CSI {ps} t is not a window operation"),
        }
    }
}

/// **Only the forms the specification gives are answered.** `CSI 14 ; 2 t`
/// is the window, frame and all, which this core is never handed --
/// answering it with the text area would be an answer to a question that
/// was not asked. The other `Ps` move, resize, iconify or report the window,
/// or push the title. Every one is XTWINOPS and [`WindowOp::Refused`]: known
/// and not done, which is a counted parameter rather than an unknown
/// operation. `t` in the `?`, `>` and intermediate namespaces is a different
/// operation altogether (`CSI > Pm t` is XTSMTITLE, `CSI Ps SP t` is DECSWBV)
/// and is not XTWINOPS at all.
#[test]
fn every_other_t_is_refused_or_not_xtwinops() {
    let refused = |c: Csi| matches!(c, Csi::Window(WindowOp::Refused));
    let not_xtwinops = |c: Csi| !matches!(c, Csi::Window(_));
    assert!(
        refused(parse(Marker::Standard, None, &[param(14), param(2)], b't')),
        "14;2 is the window"
    );
    assert!(
        refused(parse(Marker::Standard, None, &[param(16), param(0)], b't')),
        "16 takes one parameter"
    );
    assert!(
        refused(parse(Marker::Standard, None, &[param(18), param(1)], b't')),
        "18 takes one parameter"
    );
    assert!(
        refused(parse(Marker::Standard, None, &[], b't')),
        "a bare CSI t"
    );
    for ps in [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 19, 20, 21, 22, 23, 24, 48,
    ] {
        assert!(
            refused(parse(Marker::Standard, None, &[param(ps)], b't')),
            "CSI {ps} t"
        );
    }
    assert!(
        not_xtwinops(parse(Marker::Private, None, &[param(16)], b't')),
        "CSI ? 16 t"
    );
    assert!(
        not_xtwinops(parse(Marker::Gt, None, &[param(16)], b't')),
        "CSI > 16 t"
    );
    assert!(
        not_xtwinops(parse(Marker::Standard, Some(b' '), &[param(16)], b't')),
        "CSI 16 SP t"
    );
}

/// `[[source: ctlseqs]]`: "Result is CSI 4 ; height ; width t", "Result is
/// CSI 6 ; height ; width t", "Result is CSI 8 ; height ; width t". Height
/// first in all three.
#[test]
fn the_answers_have_the_shapes_ctlseqs_specifies() {
    let (rows, cols) = (Rows::of_count(50), Cols::of_index(200));
    let (cw, ch) = (ImagePx::of(17), ImagePx::of(34));
    let q = |w| Report::window(w, rows, cols, cw, ch).map(enc);
    assert_eq!(
        q(WindowOp::TextAreaPixels),
        Some(b"\x1b[4;1700;3400t".to_vec())
    );
    assert_eq!(q(WindowOp::CellPixels), Some(b"\x1b[6;34;17t".to_vec()));
    assert_eq!(q(WindowOp::TextAreaChars), Some(b"\x1b[8;50;200t".to_vec()));
}

/// With no cell size there is no pixel answer at all, rather than a zero a
/// client would divide by; the cell count needs none and is always known.
#[test]
fn a_pixel_answer_needs_both_dimensions_of_the_cell() {
    let (rows, cols) = (Rows::of_count(24), Cols::of_index(80));
    for (cw, ch) in [(0, 0), (8, 0), (0, 16)] {
        let (cw, ch) = (ImagePx::of(cw), ImagePx::of(ch));
        assert_eq!(
            Report::window(WindowOp::TextAreaPixels, rows, cols, cw, ch),
            None
        );
        assert_eq!(
            Report::window(WindowOp::CellPixels, rows, cols, cw, ch),
            None
        );
        assert_eq!(
            Report::window(WindowOp::TextAreaChars, rows, cols, cw, ch).map(enc),
            Some(b"\x1b[8;24;80t".to_vec())
        );
    }
}

/// A refused form has no answer whatever the terminal knows.
#[test]
fn a_refused_form_is_never_answered() {
    let r = Report::window(
        WindowOp::Refused,
        Rows::of_count(24),
        Cols::of_index(80),
        ImagePx::of(8),
        ImagePx::of(16),
    );
    assert_eq!(r, None);
}

/// `[[source: inband-resize]]`: the pixel fields are the text area, and zero
/// when the terminal cannot report them -- which here means either dimension
/// of the cell is unknown.
#[test]
fn the_inband_report_carries_the_text_area_in_pixels() {
    let (rows, cols) = (Rows::of_count(24), Cols::of_index(80));
    assert_eq!(
        enc(Report::inband(rows, cols, ImagePx::of(8), ImagePx::of(16))),
        b"\x1b[48;24;80;384;640t"
    );
    for (cw, ch) in [(0, 0), (8, 0), (0, 16)] {
        assert_eq!(
            enc(Report::inband(rows, cols, ImagePx::of(cw), ImagePx::of(ch))),
            b"\x1b[48;24;80;0;0t",
            "cell {cw}x{ch}"
        );
    }
}

/// A text area wider than a `u16` saturates rather than wrapping to a small
/// area that does not exist.
#[test]
fn a_text_area_past_sixty_five_thousand_pixels_saturates() {
    let r = Report::window(
        WindowOp::TextAreaPixels,
        Rows::of_count(640),
        Cols::of_index(2048),
        ImagePx::of(72),
        ImagePx::of(144),
    );
    assert_eq!(r.map(enc), Some(b"\x1b[4;65535;65535t".to_vec()));
}
