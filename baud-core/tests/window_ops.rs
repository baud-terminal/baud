//! XTWINOPS's window operations, `CSI 1 t` to `CSI 10 t`, and DECSLPP,
//! `CSI Ps t` with `Ps` 24 and up (`[[source: ctlseqs]]`, "Window
//! manipulation (XTWINOPS)"): queued for the embedder, which has the
//! window, on by default (the owner's call of 2026-10-19, option 4), and
//! refused and counted while the embedder has them off.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

fn at(x: i32, y: i32) -> WindowRequest {
    WindowRequest::Move(ScreenPoint {
        x: ScreenCoord::of(x),
        y: ScreenCoord::of(y),
    })
}

/// **Every window operation reaches the embedder, as asked, and changes
/// nothing itself**: no reply, no count, and the grid as it was after `CSI
/// 8 ; 50 ; 132 t` and DECSLPP, which reach it only through the
/// embedder's resize.
#[test]
fn each_window_operation_is_queued_for_the_embedder() {
    use SizeAsk::*;
    support::with_term(&WS, |t| {
        let counted = t.unsupported_param;
        support::feed(
            t,
            b"\x1b[1t\x1b[2t\x1b[3;10;20t\x1b[4;400;600t\x1b[5t\x1b[6t\x1b[7t\
              \x1b[8;50;132t\x1b[9;1t\x1b[9;2t\x1b[9;3t\x1b[9;0t\x1b[10;1t\x1b[10;2t\
              \x1b[10;0t\x1b[48t",
        );
        assert_eq!(
            t.window_requests(),
            [
                WindowRequest::Deiconify,
                WindowRequest::Iconify,
                at(10, 20),
                WindowRequest::ResizePixels {
                    height: Exactly(400),
                    width: Exactly(600)
                },
                WindowRequest::Raise,
                WindowRequest::Lower,
                WindowRequest::Refresh,
                WindowRequest::ResizeCells {
                    rows: Exactly(50),
                    cols: Exactly(132)
                },
                WindowRequest::Maximize(Maximize::Both),
                WindowRequest::Maximize(Maximize::Vertically),
                WindowRequest::Maximize(Maximize::Horizontally),
                WindowRequest::Maximize(Maximize::Restore),
                WindowRequest::FullScreen(FullScreen::Enter),
                WindowRequest::FullScreen(FullScreen::Toggle),
                WindowRequest::FullScreen(FullScreen::Leave),
                WindowRequest::ResizeCells {
                    rows: Exactly(48),
                    cols: Keep
                },
            ]
        );
        assert_eq!(t.reply(), b"");
        assert_eq!(t.unsupported_param, counted, "a request was refused");
        assert_eq!(t.geometry(), (Cols::of_index(80), Rows::of_count(24)));
        t.clear_window_requests();
        assert_eq!(t.window_requests(), []);
    });
}

/// **A valid window operation is not an unsupported one.** Red on the
/// build before them, which counted every one of these in
/// `unsupported_param`; the API this reads is that build's.
#[test]
fn a_window_operation_is_not_counted_unsupported() {
    support::with_term(&WS, |t| {
        let counted = t.unsupported_param;
        support::feed(t, b"\x1b[8;50;132t\x1b[24t\x1b[3;0;0t\x1b[9;1t\x1b[2t");
        assert_eq!(t.unsupported_param, counted);
        assert_eq!(t.reply(), b"");
    });
}

/// **The forms ctlseqs does not give stay refused and counted**: 12 and
/// 17, a `Ps` past 9's and 10's lists, a parameter more than an operation
/// takes, and 23 and below, which are not DECSLPP.
#[test]
fn a_form_ctlseqs_does_not_give_is_counted_and_not_queued() {
    support::with_term(&WS, |t| {
        let counted = t.unsupported_param;
        support::feed(
            t,
            b"\x1b[12t\x1b[17t\x1b[9;4t\x1b[10;3t\x1b[1;0t\x1b[7;1t\x1b[24;80t\
              \x1b[3;1;2;3t\x1b[t",
        );
        assert_eq!(t.window_requests(), []);
        assert_eq!(t.unsupported_param, counted + 9);
    });
}

/// **Omitted keeps, 0 is the screen's** (ctlseqs for 4 and 8), and a move's
/// omitted or 0 coordinate is 0; a 9 or 10 without its `Ps` is 0, the
/// restore and the leave.
#[test]
fn omitted_and_zero_parameters_reach_the_embedder_as_sent() {
    use SizeAsk::*;
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[4;;640t\x1b[4;0;0t\x1b[8t\x1b[3t\x1b[9t\x1b[10t");
        assert_eq!(
            t.window_requests(),
            [
                WindowRequest::ResizePixels {
                    height: Keep,
                    width: Exactly(640)
                },
                WindowRequest::ResizePixels {
                    height: Screen,
                    width: Screen
                },
                WindowRequest::ResizeCells {
                    rows: Keep,
                    cols: Keep
                },
                at(0, 0),
                WindowRequest::Maximize(Maximize::Restore),
                WindowRequest::FullScreen(FullScreen::Leave),
            ]
        );
    });
}

/// **The position `CSI 13 t` reports is where `CSI 3 t` moves the window**:
/// a window left of the primary screen reports its x as the sixteen-bit two's
/// complement, and the same bytes sent back as a move are that position.
#[test]
fn the_reported_position_sent_back_is_the_same_move() {
    support::with_term(&WS, |t| {
        t.set_window(WindowFacts {
            position: Some(ScreenPoint {
                x: ScreenCoord::of(-1440),
                y: ScreenCoord::of(25),
            }),
            ..WindowFacts::UNKNOWN
        });
        support::feed(t, b"\x1b[13t");
        let report = t.reply().to_vec();
        assert_eq!(report, b"\x1b[3;64096;25t");
        t.clear_reply();
        support::feed(t, &report);
        assert_eq!(t.window_requests(), [at(-1440, 25)]);
    });
}

/// **Off, nothing is queued and each is counted**, as xterm performs no
/// operation it disallows; the reports and the title stack answer as
/// before, since they are not window operations.
#[test]
fn with_the_operations_off_nothing_is_queued_and_each_is_counted() {
    support::with_term(&WS, |t| {
        assert_eq!(t.window_ops(), WindowOps::On, "on by default");
        t.set_window_ops(WindowOps::Off);
        let (refused, param) = (t.window_ops_refused(), t.unsupported_param);
        support::feed(t, b"\x1b[2t\x1b[3;0;0t\x1b[8;50;132t\x1b[48t\x1b[10;1t");
        assert_eq!(t.window_requests(), []);
        assert_ne!(t.window_ops_refused(), refused, "not counted");
        assert_eq!(t.unsupported_param, param, "counted as unsupported");
        let after = t.window_ops_refused();
        support::feed(
            t,
            b"\x1b]2;shell\x07\x1b[22;2t\x1b]2;x\x07\x1b[23;2t\x1b[18t",
        );
        assert_eq!(t.reply(), b"\x1b[8;24;80t", "the report");
        assert_eq!(t.title(), b"shell", "the title stack");
        assert_eq!(t.window_ops_refused(), after, "counted what was not one");
        t.set_window_ops(WindowOps::On);
        support::feed(t, b"\x1b[2t");
        assert_eq!(t.window_requests(), [WindowRequest::Iconify]);
    });
}

/// **No sequence turns the operations back on**: `RIS` and `DECSTR` leave
/// the embedder's setting, so a program cannot undo a refusal.
#[test]
fn no_reset_turns_the_window_operations_back_on() {
    support::with_term(&WS, |t| {
        t.set_window_ops(WindowOps::Off);
        support::feed(t, b"\x1bc\x1b[!p\x1b[2t");
        assert_eq!(t.window_ops(), WindowOps::Off);
        assert_eq!(t.window_requests(), []);
    });
}

/// **A queue left undrained drops what does not fit, and counts it**: the
/// first `WINDOW_QUEUE` stay, in order, and the next is dropped whole.
#[test]
fn a_full_queue_drops_and_counts_the_rest() {
    support::with_term(&WS, |t| {
        let dropped = t.window_requests_dropped();
        let mut bytes = Vec::new();
        for _ in 0..WINDOW_QUEUE {
            bytes.extend_from_slice(b"\x1b[5t");
        }
        bytes.extend_from_slice(b"\x1b[6t");
        support::feed(t, &bytes);
        assert_eq!(t.window_requests(), [WindowRequest::Raise; WINDOW_QUEUE]);
        assert_ne!(t.window_requests_dropped(), dropped, "not counted");
        t.clear_window_requests();
        support::feed(t, b"\x1b[6t");
        assert_eq!(t.window_requests(), [WindowRequest::Lower]);
    });
}
