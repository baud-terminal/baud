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

/// **Only the forms the specification gives are named.** 12 and 17 are
/// [`WindowOp::Refused`], and so is a report with a parameter ctlseqs does
/// not give it: `CSI 13 ; 1 t` is neither the window's position nor the
/// text area's, and answering it as either answers a question that was not
/// asked. So is a window operation past ctlseqs' list, `CSI 9 ; 4 t` and
/// `CSI 10 ; 3 t`, where xterm's `RequestMaximize` falls through to a
/// restore, and one with more parameters than ctlseqs gives it. `t` in the `?`, `>` and
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
        (&[param(9), param(4)][..], "9 takes 0 to 3"),
        (&[param(10), param(3)][..], "10 takes 0 to 2"),
        (&[param(1), param(0)][..], "1 takes no parameter"),
        (&[param(2), param(1)][..], "2 takes no parameter"),
        (&[param(5), param(5)][..], "5 takes no parameter"),
        (&[param(6), param(0)][..], "6 takes no parameter"),
        (&[param(7), param(7)][..], "7 takes no parameter"),
        (&[param(9), param(1), param(1)][..], "9 takes one"),
        (&[param(10), param(2), param(0)][..], "10 takes one"),
        (&[param(3), param(1), param(2), param(3)][..], "3 takes two"),
        (
            &[param(8), param(24), param(80), param(1)][..],
            "8 takes two",
        ),
        (&[param(24), param(80)][..], "DECSLPP takes one"),
    ] {
        assert!(refused(parse(Marker::Standard, None, ps, b't')), "{why}");
    }
    for ps in [0, 12, 17] {
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

/// **Each window operation parses to its own name**, with the parameters
/// ctlseqs gives it: 1, 2, 5, 6 and 7 alone, 3, 4 and 8 with up to two, 9
/// and 10 with one, and DECSLPP, "Ps >= 2 4", alone, 24 and every value
/// above it.
#[test]
fn the_window_operations_parse_to_their_names() {
    for (ps, want) in [
        (&[param(1)][..], WindowOp::Deiconify),
        (&[param(2)][..], WindowOp::Iconify),
        (&[param(3), param(10), param(20)][..], WindowOp::Move),
        (&[param(3)][..], WindowOp::Move),
        (
            &[param(4), param(400), param(600)][..],
            WindowOp::ResizePixels,
        ),
        (&[param(5)][..], WindowOp::Raise),
        (&[param(6)][..], WindowOp::Lower),
        (&[param(7)][..], WindowOp::Refresh),
        (
            &[param(8), param(50), param(132)][..],
            WindowOp::ResizeCells,
        ),
        (&[param(8), None, param(132)][..], WindowOp::ResizeCells),
        (&[param(9)][..], WindowOp::Maximize),
        (&[param(9), param(3)][..], WindowOp::Maximize),
        (&[param(10), param(2)][..], WindowOp::FullScreen),
        (&[param(24)][..], WindowOp::Lines),
        (&[param(25)][..], WindowOp::Lines),
        (&[param(144)][..], WindowOp::Lines),
        (&[param(65535)][..], WindowOp::Lines),
    ] {
        match parse(Marker::Standard, None, ps, b't') {
            Csi::Window(got) => assert_eq!(got, want, "{ps:?}"),
            _ => panic!("{ps:?} is not a window operation"),
        }
    }
    assert!(
        matches!(
            parse(Marker::Standard, None, &[param(23)], b't'),
            Csi::Window(WindowOp::PopTitle)
        ),
        "23 is the title pop, not DECSLPP"
    );
}

/// The request one sequence makes, as [`WindowRequest::of`] reads it.
fn request(ps: &[Option<Param>]) -> Option<WindowRequest> {
    match parse(Marker::Standard, None, ps, b't') {
        Csi::Window(op) => WindowRequest::of(op, &Params::new(ps)),
        _ => None,
    }
}

/// **Omitted and 0 are two different sizes**: ctlseqs, for 4 and 8,
/// "Omitted parameters reuse the current height or width. Zero parameters
/// use the display's height or width." xterm reads an empty parameter as
/// omitted (`optional_param` gives `DEFAULT`), so `CSI 8 ; ; 132 t` keeps
/// the rows. Height, or rows, first.
#[test]
fn a_resize_tells_omitted_from_zero() {
    use SizeAsk::*;
    assert_eq!(
        request(&[param(4), param(400), param(600)]),
        Some(WindowRequest::ResizePixels {
            height: Exactly(400),
            width: Exactly(600)
        })
    );
    assert_eq!(
        request(&[param(4)]),
        Some(WindowRequest::ResizePixels {
            height: Keep,
            width: Keep
        })
    );
    assert_eq!(
        request(&[param(4), param(0), param(0)]),
        Some(WindowRequest::ResizePixels {
            height: Screen,
            width: Screen
        })
    );
    assert_eq!(
        request(&[param(8), None, param(132)]),
        Some(WindowRequest::ResizeCells {
            rows: Keep,
            cols: Exactly(132)
        })
    );
    assert_eq!(
        request(&[param(8), param(50)]),
        Some(WindowRequest::ResizeCells {
            rows: Exactly(50),
            cols: Keep
        })
    );
    assert_eq!(
        request(&[param(8), param(0), param(80)]),
        Some(WindowRequest::ResizeCells {
            rows: Screen,
            cols: Exactly(80)
        })
    );
}

/// DECSLPP asks for `Ps` rows and keeps the columns, as xterm's
/// `RequestResize(xw, code, -1, True)` does; the window operations without
/// parameters, and 9 and 10 by their `Ps`, an omitted one 0.
#[test]
fn the_other_requests_carry_what_ctlseqs_gives_them() {
    use WindowRequest::*;
    for (ps, want) in [
        (
            &[param(48)][..],
            ResizeCells {
                rows: SizeAsk::Exactly(48),
                cols: SizeAsk::Keep,
            },
        ),
        (&[param(1)][..], Deiconify),
        (&[param(2)][..], Iconify),
        (&[param(5)][..], Raise),
        (&[param(6)][..], Lower),
        (&[param(7)][..], Refresh),
        (
            &[param(9)][..],
            WindowRequest::Maximize(baud_domain::Maximize::Restore),
        ),
        (
            &[param(9), param(0)][..],
            WindowRequest::Maximize(baud_domain::Maximize::Restore),
        ),
        (
            &[param(9), param(1)][..],
            WindowRequest::Maximize(baud_domain::Maximize::Both),
        ),
        (
            &[param(9), param(2)][..],
            WindowRequest::Maximize(baud_domain::Maximize::Vertically),
        ),
        (
            &[param(9), param(3)][..],
            WindowRequest::Maximize(baud_domain::Maximize::Horizontally),
        ),
        (
            &[param(10)][..],
            WindowRequest::FullScreen(baud_domain::FullScreen::Leave),
        ),
        (
            &[param(10), param(1)][..],
            WindowRequest::FullScreen(baud_domain::FullScreen::Enter),
        ),
        (
            &[param(10), param(2)][..],
            WindowRequest::FullScreen(baud_domain::FullScreen::Toggle),
        ),
    ] {
        assert_eq!(request(ps), Some(want), "{ps:?}");
    }
    for ps in [&[param(11)][..], &[param(22)][..], &[param(12)][..]] {
        assert_eq!(request(ps), None, "{ps:?} is no window operation");
    }
}

/// **A move reads its coordinates as the report writes them**: 32768-65535
/// are negative, the sixteen-bit two's complement `CSI 13 t` reports a
/// window left of or above the primary screen in, so what is reported, sent
/// back, is where the window goes. Omitted or 0 is 0, as xterm's
/// `zero_if_default` reads it.
#[test]
fn a_move_reads_its_coordinates_as_the_report_writes_them() {
    let at = |x: i32, y: i32| {
        Some(WindowRequest::Move(ScreenPoint {
            x: ScreenCoord::of(x),
            y: ScreenCoord::of(y),
        }))
    };
    assert_eq!(request(&[param(3), param(65528), param(50)]), at(-8, 50));
    assert_eq!(
        request(&[param(3), param(32768), param(32767)]),
        at(-32768, 32767)
    );
    assert_eq!(request(&[param(3)]), at(0, 0));
    assert_eq!(request(&[param(3), None, param(7)]), at(0, 7));
    for v in [-32768, -1, 0, 1, 1440, 32767] {
        let c = ScreenCoord::of(v);
        assert_eq!(ScreenCoord::from_wire(c.wire()), c, "{v}");
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
    // Neither a title operation nor a window operation is a report.
    for op in [
        WindowOp::TitleReport,
        WindowOp::PushTitle,
        WindowOp::PopTitle,
        WindowOp::Deiconify,
        WindowOp::Iconify,
        WindowOp::Move,
        WindowOp::ResizePixels,
        WindowOp::Raise,
        WindowOp::Lower,
        WindowOp::Refresh,
        WindowOp::ResizeCells,
        WindowOp::Maximize,
        WindowOp::FullScreen,
        WindowOp::Lines,
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
