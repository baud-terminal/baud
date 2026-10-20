//! XTWINOPS past the text area (`[[source: ctlseqs]]`, "Window manipulation
//! (XTWINOPS)"): the window and screen reports, answered from what the
//! embedder hands over and silent where it has handed nothing; the title
//! reports, refused; and the title stack, `CSI 22 t` and `CSI 23 t`, as
//! xterm's `xtermPushTitle` and `xtermPopTitle` keep it (`[[source:
//! xterm-misc]]`).

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

fn facts() -> WindowFacts {
    WindowFacts {
        state: Some(WindowState::Open),
        position: Some(ScreenPoint {
            x: ScreenCoord::of(-8),
            y: ScreenCoord::of(50),
        }),
        text_area: Some(ScreenPoint {
            x: ScreenCoord::of(12),
            y: ScreenCoord::of(106),
        }),
        window: Some(PixelSize {
            width: ImagePx::of(1384),
            height: ImagePx::of(872),
        }),
        screen: Some(PixelSize {
            width: ImagePx::of(3024),
            height: ImagePx::of(1964),
        }),
    }
}

/// **Each report answers from the embedder's numbers**, in ctlseqs' shape:
/// 11 "CSI 1 t", 13 and 13;2 "CSI 3 ; x ; y t" with a negative x as its
/// sixteen-bit two's complement, 14;2 "CSI 4 ; height ; width t", 15 "CSI 5
/// ; height ; width t", 19 the screen over the cell, "CSI 9 ; height ;
/// width t".
#[test]
fn the_window_reports_answer_from_what_the_embedder_handed_over() {
    support::with_term(&WS, |t| {
        t.set_window(facts());
        t.set_cell_size(ImagePx::of(17), ImagePx::of(34));
        let counted = t.unsupported_param;
        support::feed(t, b"\x1b[11t\x1b[13t\x1b[13;2t\x1b[14;2t\x1b[15t\x1b[19t");
        assert_eq!(
            t.reply(),
            b"\x1b[1t\x1b[3;65528;50t\x1b[3;12;106t\x1b[4;872;1384t\x1b[5;1964;3024t\x1b[9;57;177t"
        );
        assert_eq!(t.unsupported_param, counted, "every report was answered");
    });
}

/// **Nothing handed over, nothing answered**: each report is silence and a
/// count, the way an unanswerable `DSR` is, never a made-up number -- and
/// the screen in characters needs the cell as well as the screen.
#[test]
fn a_report_the_embedder_cannot_answer_is_silent_and_counted() {
    support::with_term(&WS, |t| {
        let counted = t.unsupported_param;
        support::feed(t, b"\x1b[11t\x1b[13t\x1b[13;2t\x1b[14;2t\x1b[15t\x1b[19t");
        assert_eq!(t.reply(), b"", "an answer from nothing");
        assert_eq!(t.unsupported_param, counted + 6);
        t.set_window(WindowFacts {
            screen: facts().screen,
            ..WindowFacts::UNKNOWN
        });
        support::feed(t, b"\x1b[19t\x1b[15t");
        assert_eq!(t.reply(), b"\x1b[5;1964;3024t", "19 needs the cell");
        assert_eq!(t.unsupported_param, counted + 7);
    });
}

/// The last facts handed over are the ones answered: an iconified window
/// reports `CSI 2 t`, and one moved reports where it went.
#[test]
fn the_reports_follow_the_facts_as_they_change() {
    support::with_term(&WS, |t| {
        t.set_window(facts());
        support::feed(t, b"\x1b[11t");
        t.set_window(WindowFacts {
            state: Some(WindowState::Iconified),
            position: Some(ScreenPoint {
                x: ScreenCoord::of(200),
                y: ScreenCoord::of(0),
            }),
            ..facts()
        });
        support::feed(t, b"\x1b[11t\x1b[13t");
        assert_eq!(t.reply(), b"\x1b[1t\x1b[2t\x1b[3;200;0t");
    });
}

/// **`CSI 20 t` and `CSI 21 t` are refused and counted on their own**: the
/// reply would put the title, text a program chose, into the input stream.
/// Nothing is written and `unsupported_param` does not move, so an embedder
/// can tell the requests from the operations baud does not have.
#[test]
fn the_title_reports_are_refused_and_counted() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]2;rm -rf ~\r\x07\x1b]1;x\x07");
        let (param, refused) = (t.unsupported_param, t.title_reports_refused());
        support::feed(t, b"\x1b[21t\x1b[20t\x1b[21;0t");
        assert_eq!(t.reply(), b"", "the title went back to the host");
        assert_ne!(t.title_reports_refused(), refused, "not counted");
        assert_eq!(t.unsupported_param, param);
        let after = t.title_reports_refused();
        support::feed(t, b"\x1b[18t\x1b[22;2t");
        assert_eq!(t.title_reports_refused(), after, "counted what was not one");
    });
}

/// **vim's title save and restore**: `CSI 22 ; 2 t` and `CSI 22 ; 1 t` at
/// start, `CSI 23 ; 2 t` and `CSI 23 ; 1 t` at exit, the shell's title back
/// after vim set its own. The icon name is not: the two pushes are two
/// slots, the first pop takes the icon-only slot (its title filled from the
/// older one), and the second finds a title-only slot with nothing older,
/// so the icon name stays vim's. That is what xterm's stack does with the
/// same bytes, and what this follows.
#[test]
fn vims_title_push_and_pop_restore_the_shells_title() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]0;shell\x07");
        support::feed(
            t,
            b"\x1b[22;2t\x1b[22;1t\x1b]2;vim main.rs\x07\x1b]1;vim\x07",
        );
        assert_eq!(t.title(), b"vim main.rs");
        support::feed(t, b"\x1b[23;2t\x1b[23;1t");
        assert_eq!(t.title(), b"shell");
        assert_eq!(t.icon_name(), b"vim", "xterm's stack leaves it");
    });
}

/// A bare `CSI 22 t` pushes both names and a bare `CSI 23 t` pops both, as
/// xterm reads the omitted `Ps` as 0.
#[test]
fn a_bare_push_and_pop_take_both_names() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]2;one\x07\x1b]1;ico\x07\x1b[22t");
        support::feed(t, b"\x1b]0;two\x07\x1b[23t");
        assert_eq!((t.title(), t.icon_name()), (&b"one"[..], &b"ico"[..]));
    });
}

/// **A pop of an empty stack does nothing**, and a pop takes the slot:
/// the second pop of one push finds nothing and leaves the title.
#[test]
fn a_pop_of_an_empty_stack_leaves_the_title() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]2;now\x07\x1b[23;2t");
        assert_eq!(t.title(), b"now");
        support::feed(t, b"\x1b[22;2t\x1b]2;later\x07\x1b[23;2t");
        assert_eq!(t.title(), b"now");
        support::feed(t, b"\x1b]2;last\x07\x1b[23;2t");
        assert_eq!(t.title(), b"last", "the slot was popped once");
    });
}

/// **A half the push did not take comes from an older slot** (xterm's
/// `TryHigher`): an icon-only push popped as both restores its icon and the
/// title an earlier push kept.
#[test]
fn a_half_the_push_did_not_take_comes_from_an_older_slot() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]0;A\x07\x1b[22;0t");
        support::feed(t, b"\x1b]0;B\x07\x1b[22;1t");
        support::feed(t, b"\x1b]0;C\x07\x1b[23;0t");
        assert_eq!((t.icon_name(), t.title()), (&b"B"[..], &b"A"[..]));
    });
}

/// With nothing older to fill a half from, that name is left as it is.
#[test]
fn a_half_nothing_kept_leaves_the_name() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]0;A\x07\x1b[22;1t\x1b]0;C\x07\x1b[23;0t");
        assert_eq!((t.icon_name(), t.title()), (&b"A"[..], &b"C"[..]));
    });
}

/// **The direct-access slot, 1 to 10**, stores and reads without pushing or
/// popping, so it reads back as often as it is asked and the stack under it
/// is untouched; a slot past 10 is outside ctlseqs' range and counted.
#[test]
fn a_slot_from_one_to_ten_reads_and_writes_without_pushing() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]2;kept\x07\x1b[22;2;7t\x1b]2;x\x07\x1b[23;2;7t");
        assert_eq!(t.title(), b"kept");
        support::feed(t, b"\x1b]2;y\x07\x1b[23;2;7t");
        assert_eq!(t.title(), b"kept", "a direct read does not pop");
        support::feed(t, b"\x1b]2;z\x07\x1b[23;2t");
        assert_eq!(t.title(), b"z", "nothing was pushed");
        let counted = t.unsupported_param;
        support::feed(t, b"\x1b[22;2;11t\x1b[23;2;11t");
        assert_eq!(t.unsupported_param, counted + 2);
        assert_eq!(t.title(), b"z");
    });
}

/// Ten slots: the eleventh push writes over the oldest, so ten pops give
/// back the last ten titles and the eleventh lands on the slot t10 took,
/// already popped, and changes nothing.
#[test]
fn the_eleventh_push_overwrites_the_oldest() {
    support::with_term(&WS, |t| {
        for n in 0..11 {
            support::feed(t, format!("\x1b]2;t{n}\x07\x1b[22;2t").as_bytes());
        }
        support::feed(t, b"\x1b]2;end\x07");
        for n in (1..11).rev() {
            support::feed(t, b"\x1b[23;2t");
            assert_eq!(t.title(), format!("t{n}").as_bytes());
        }
        support::feed(t, b"\x1b[23;2t");
        assert_eq!(t.title(), b"t1", "an emptied slot changed the title");
    });
}

/// **`RIS` and `DECSTR` keep the stack**, as both keep the title itself
/// and as xterm's resets do (Jev, RIS_KEEPS, 0.94/0.98).
#[test]
fn the_resets_keep_the_title_stack() {
    support::with_term(&WS, |t| {
        support::feed(
            t,
            b"\x1b]2;saved\x07\x1b[22;2t\x1b]2;x\x07\x1bc\x1b[!p\x1b[23;2t",
        );
        assert_eq!(t.title(), b"saved");
    });
}

/// The same bytes split at every boundary give the same titles.
#[test]
fn the_title_stack_is_chunk_invariant() {
    let bytes = b"\x1b]0;A\x07\x1b[22;0t\x1b]0;B\x07\x1b[22;1;3t\x1b]2;C\x07\x1b[23;1;3t\x1b[23t";
    let whole = support::with_term(&WS, |t| {
        support::feed(t, bytes);
        (t.title().to_vec(), t.icon_name().to_vec())
    });
    assert_eq!(whole, (b"A".to_vec(), b"A".to_vec()));
    for cut in 1..bytes.len() {
        let split = support::with_term(&WS, |t| {
            support::feed(t, &bytes[..cut]);
            support::feed(t, &bytes[cut..]);
            (t.title().to_vec(), t.icon_name().to_vec())
        });
        assert_eq!(split, whole, "cut at {cut}");
    }
}
