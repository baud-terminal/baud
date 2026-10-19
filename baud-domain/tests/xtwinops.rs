//! XTWINOPS: the reports about the text area, the window and the screen,
//! the title stack and the refused forms, parsed to named operations and
//! answered in `[[source: ctlseqs]]`'s shapes.
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

/// **Only the forms the specification gives are named.** The window-changing
/// operations 1-10 and DECSLPP (24 and up) are [`WindowOp::Refused`], and so
/// is a report with a parameter ctlseqs does not give it: `CSI 13 ; 1 t` is
/// neither the window's position nor the text area's, and answering it as
/// either answers a question that was not asked. `t` in the `?`, `>` and
/// intermediate namespaces is a different operation altogether (`CSI > Pm
/// t` is XTSMTITLE, `CSI Ps SP t` is DECSWBV) and is not XTWINOPS at all.
#[test]
fn every_other_t_is_refused_or_not_xtwinops() {
    let refused = |c: Csi| matches!(c, Csi::Window(WindowOp::Refused));
    let not_xtwinops = |c: Csi| !matches!(c, Csi::Window(_));
    for (ps, why) in [
        (&[param(16), param(0)][..], "16 takes one parameter"),
        (&[param(18), param(1)][..], "18 takes one parameter"),
        (&[param(13), param(1)][..], "13 takes 2 or nothing"),
        (&[param(14), param(3)][..], "14 takes 2 or nothing"),
        (&[param(11), param(0)][..], "11 takes one parameter"),
        (&[param(15), param(2)][..], "15 takes one parameter"),
        (&[param(19), param(2)][..], "19 takes one parameter"),
        (&[param(22), param(3)][..], "22 takes 0, 1 or 2"),
        (&[param(23), param(9)][..], "23 takes 0, 1 or 2"),
        (
            &[param(22), param(0), param(1), param(1)][..],
            "22 takes three at most",
        ),
        (&[][..], "a bare CSI t"),
    ] {
        assert!(refused(parse(Marker::Standard, None, ps, b't')), "{why}");
    }
    for ps in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 17, 24, 48, 144] {
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

/// The window and screen reports, the two title reports and the title stack
/// each parse to their own operation. A bare `CSI 22 t` is the push of
/// both titles, as xterm reads the omitted `Ps` as 0 (Jev's lean, AS_ZERO).
#[test]
fn the_window_and_title_forms_parse_to_their_operations() {
    for (ps, want) in [
        (&[param(11)][..], WindowOp::WindowState),
        (&[param(13)][..], WindowOp::WindowPosition),
        (&[param(13), param(2)][..], WindowOp::TextAreaPosition),
        (&[param(14), param(2)][..], WindowOp::WindowPixels),
        (&[param(15)][..], WindowOp::ScreenPixels),
        (&[param(19)][..], WindowOp::ScreenChars),
        (&[param(20)][..], WindowOp::TitleReport),
        (&[param(21)][..], WindowOp::TitleReport),
        (&[param(22)][..], WindowOp::PushTitle),
        (&[param(22), param(2)][..], WindowOp::PushTitle),
        (&[param(22), param(1), param(10)][..], WindowOp::PushTitle),
        (&[param(23), param(0)][..], WindowOp::PopTitle),
        (&[param(23), param(2), param(3)][..], WindowOp::PopTitle),
    ] {
        match parse(Marker::Standard, None, ps, b't') {
            Csi::Window(got) => assert_eq!(got, want, "{ps:?}"),
            _ => panic!("{ps:?} is not a window operation"),
        }
    }
}

fn facts() -> WindowFacts {
    WindowFacts {
        state: Some(WindowState::Iconified),
        position: Some(ScreenPoint {
            x: ScreenCoord::of(-1),
            y: ScreenCoord::of(52),
        }),
        text_area: Some(ScreenPoint {
            x: ScreenCoord::of(40),
            y: ScreenCoord::of(-32768),
        }),
        window: Some(PixelSize {
            width: ImagePx::of(1440),
            height: ImagePx::of(900),
        }),
        screen: Some(PixelSize {
            width: ImagePx::of(3456),
            height: ImagePx::of(2234),
        }),
    }
}

/// `[[source: ctlseqs]]`: 11 "returns CSI 1 t" or "CSI 2 t"; 13 and 13;2
/// "Result is CSI 3 ; x ; y t", **x first**, with a negative position "in
/// the range 0-65535. Negative values correspond to 32768-65535"; 14;2
/// "Result is CSI 4 ; height ; width t"; 15 "CSI 5 ; height ; width t";
/// 19 "CSI 9 ; height ; width t", the screen divided by the cell.
#[test]
fn the_window_and_screen_answers_have_ctlseqs_shapes() {
    let (rows, cols) = (Rows::of_count(24), Cols::of_index(80));
    let (cw, ch) = (ImagePx::of(17), ImagePx::of(34));
    let f = facts();
    let q = |w| Report::window(w, rows, cols, cw, ch, &f).map(enc);
    assert_eq!(q(WindowOp::WindowState), Some(b"\x1b[2t".to_vec()));
    assert_eq!(
        q(WindowOp::WindowPosition),
        Some(b"\x1b[3;65535;52t".to_vec())
    );
    assert_eq!(
        q(WindowOp::TextAreaPosition),
        Some(b"\x1b[3;40;32768t".to_vec())
    );
    assert_eq!(
        q(WindowOp::WindowPixels),
        Some(b"\x1b[4;900;1440t".to_vec())
    );
    assert_eq!(
        q(WindowOp::ScreenPixels),
        Some(b"\x1b[5;2234;3456t".to_vec())
    );
    assert_eq!(q(WindowOp::ScreenChars), Some(b"\x1b[9;65;203t".to_vec()));
    let open = WindowFacts {
        state: Some(WindowState::Open),
        ..f
    };
    assert_eq!(
        Report::window(WindowOp::WindowState, rows, cols, cw, ch, &open).map(enc),
        Some(b"\x1b[1t".to_vec())
    );
}

/// **What the embedder did not say is not answered**: no fact, no reply,
/// and the screen in characters needs the cell as well as the screen.
#[test]
fn a_window_answer_needs_the_fact_it_reports() {
    let (rows, cols) = (Rows::of_count(24), Cols::of_index(80));
    let (cw, ch) = (ImagePx::of(17), ImagePx::of(34));
    for op in [
        WindowOp::WindowState,
        WindowOp::WindowPosition,
        WindowOp::TextAreaPosition,
        WindowOp::WindowPixels,
        WindowOp::ScreenPixels,
        WindowOp::ScreenChars,
    ] {
        assert_eq!(
            Report::window(op, rows, cols, cw, ch, &WindowFacts::UNKNOWN),
            None,
            "{op:?}"
        );
    }
    assert_eq!(
        Report::window(
            WindowOp::ScreenChars,
            rows,
            cols,
            ImagePx::ZERO,
            ch,
            &facts()
        ),
        None,
        "no cell width"
    );
    for op in [
        WindowOp::TitleReport,
        WindowOp::PushTitle,
        WindowOp::PopTitle,
    ] {
        assert_eq!(Report::window(op, rows, cols, cw, ch, &facts()), None);
    }
}

/// `[[source: ctlseqs]]`: "Result is CSI 4 ; height ; width t", "Result is
/// CSI 6 ; height ; width t", "Result is CSI 8 ; height ; width t". Height
/// first in all three.
#[test]
fn the_answers_have_the_shapes_ctlseqs_specifies() {
    let (rows, cols) = (Rows::of_count(50), Cols::of_index(200));
    let (cw, ch) = (ImagePx::of(17), ImagePx::of(34));
    let q = |w| Report::window(w, rows, cols, cw, ch, &WindowFacts::UNKNOWN).map(enc);
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
            Report::window(
                WindowOp::TextAreaPixels,
                rows,
                cols,
                cw,
                ch,
                &WindowFacts::UNKNOWN
            ),
            None
        );
        assert_eq!(
            Report::window(
                WindowOp::CellPixels,
                rows,
                cols,
                cw,
                ch,
                &WindowFacts::UNKNOWN
            ),
            None
        );
        assert_eq!(
            Report::window(
                WindowOp::TextAreaChars,
                rows,
                cols,
                cw,
                ch,
                &WindowFacts::UNKNOWN
            )
            .map(enc),
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
        &facts(),
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
        &WindowFacts::UNKNOWN,
    );
    assert_eq!(r.map(enc), Some(b"\x1b[4;65535;65535t".to_vec()));
}
