//! **The status line**: DECSSDT (`CSI Ps $ ~`) and DECSASD (`CSI Ps $ }`),
//! to `[[source: vt420-rm]]` pp. 219-221 and `[[source: dec-std-070]]`
//! 070-14.
//!
//! A row below the main display, of one of three types: none, the
//! indicator (the terminal's own state in reverse video) or host-writable
//! (a second display of one row, written while DECSASD selects it). The
//! main display keeps its rows; the line is beside it
//! (`Term::status_rows`). What the host-writable line keeps of its own is
//! 070-14's list, §14.2.2: the active position, the rendition, origin
//! mode, the character sets and shifts, the pending wrap and the `DECSC`
//! slot (MODE_SHARED_FLAG_SEPARATE, 0.94 and 0.96).

use crate::support;
use baud_core::*;
use baud_domain::{CharacterPath, ImageBudget, ImagePx};

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 20,
};

fn text(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or(' ')
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn status(t: &Term<'_>) -> String {
    text(t.status_line().expect("a status line is shown").cells)
}

fn main_row(t: &Term<'_>, r: Row) -> String {
    text(t.row(r).expect("a row on the screen"))
}

fn show(b: &[u8]) -> String {
    b.escape_ascii().to_string()
}

fn after(setup: &[u8], query: &[u8]) -> String {
    support::with_term(&WS, |t| {
        support::feed(t, setup);
        t.clear_reply();
        support::feed(t, query);
        show(t.reply())
    })
}

const HOST: &[u8] = b"\x1b[2$~";
const ON: &[u8] = b"\x1b[1$}";
const OFF: &[u8] = b"\x1b[0$}";

// ---- the types ---------------------------------------------------------

/// **No status line at power-on**, 070-14's factory default, and none
/// takes a row of the window until a program asks; DECSSDT 1 and 2 each
/// show one, beside the main display's rows, and 0 and an omitted `Ps`
/// take it away (`[[source: dec-std-070]]` p. 14-12, "If Ps is omitted, it
/// is assumed to be 0").
#[test]
fn a_program_opens_and_closes_the_line_beside_the_main_display() {
    support::with_term(&WS, |t| {
        assert_eq!(t.status_type(), StatusType::None);
        assert_eq!(t.status_rows(), Rows::ZERO);
        assert!(t.status_line().is_none());
        for (open, ty) in [
            (b"\x1b[1$~".as_slice(), StatusType::Indicator),
            (HOST, StatusType::HostWritable),
        ] {
            support::feed(t, open);
            assert_eq!(t.status_type(), ty);
            assert_eq!(t.status_rows(), Rows::of_count(1));
            assert_eq!(t.geometry(), (Cols::of_index(20), Rows::of_count(6)));
            assert_eq!(t.status_line().map(|s| s.cells.len()), Some(20));
        }
        support::feed(t, b"\x1b[$~");
        assert_eq!(t.status_type(), StatusType::None);
        assert_eq!(t.status_rows(), Rows::ZERO);
        support::feed(t, b"\x1b[2$~\x1b[0$~");
        assert_eq!(t.status_rows(), Rows::ZERO);
    });
}

/// A `Ps` neither text gives is no control, and counted.
#[test]
fn a_type_or_display_no_text_gives_is_counted() {
    support::with_term(&WS, |t| {
        let before = t.unsupported;
        support::feed(t, b"\x1b[3$~\x1b[2$}");
        assert_eq!(t.unsupported, before + 2);
        assert_eq!(t.status_type(), StatusType::None);
    });
}

// ---- DECRQSS --------------------------------------------------------------

/// "The Status Display Type (DECSSDT) and Active Status Display (DECSASD)
/// may be queried through DECRQSS" (070-14 p. 14-9), each answered as the
/// control that sets it.
#[test]
fn decrqss_answers_the_type_and_the_display() {
    let ask_type = b"\x1bP$q$~\x1b\\";
    let ask_display = b"\x1bP$q$}\x1b\\";
    assert_eq!(after(b"", ask_type), show(b"\x1bP1$r0$~\x1b\\"));
    assert_eq!(after(b"", ask_display), show(b"\x1bP1$r0$}\x1b\\"));
    assert_eq!(after(b"\x1b[1$~", ask_type), show(b"\x1bP1$r1$~\x1b\\"));
    assert_eq!(after(HOST, ask_type), show(b"\x1bP1$r2$~\x1b\\"));
    let on = [HOST, ON].concat();
    assert_eq!(after(&on, ask_display), show(b"\x1bP1$r1$}\x1b\\"));
    // Asked from the status line itself, as from the main display.
    assert_eq!(after(&on, ask_type), show(b"\x1bP1$r2$~\x1b\\"));
}

/// "Note that the Host Writable Status Line must be selected through
/// DECSSDT for DECSASD to be able to select the Host Writable Status Line"
/// (070-14 p. 14-11): under no line or the indicator, DECSASD 1 selects
/// nothing and the text stays on the main display.
#[test]
fn decsasd_selects_the_line_only_when_it_is_host_writable() {
    for setup in [b"".as_slice(), b"\x1b[1$~"] {
        support::with_term(&WS, |t| {
            support::feed(t, setup);
            support::feed(t, b"\x1b[1$}ab");
            assert_eq!(t.active_display(), ActiveDisplay::Main);
            assert_eq!(main_row(t, Row::from_index(0)), "ab");
        });
    }
}

// ---- the host-writable line ----------------------------------------------

/// "A typical operation by the host would be to send a DECSASD command to
/// switch to the Host Writable Status Line, send a line of text to the
/// line, and then send another DECSASD command to return to the Main
/// Display" (070-14 p. 14-7): the text lands on the line, and the main
/// display's text and cursor are where they were.
#[test]
fn text_between_two_decsasds_lands_on_the_line() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[3;4Hmain");
        support::feed(t, &[HOST, ON, b"mail: 3", OFF, b"!"].concat());
        assert_eq!(status(t), "mail: 3");
        assert_eq!(main_row(t, Row::from_index(2)), "   main!");
        assert_eq!(t.cursor(), (Row::from_index(2), Col::at_index(8)));
        for r in [0, 1, 3, 4, 5] {
            assert_eq!(main_row(t, Row::from_index(r)), "", "row {r}");
        }
    });
}

/// **Each display keeps its own active position across a read**, and the
/// cursor is drawn where the active display has it: on the line while it
/// is selected (`StatusLine::cursor`), and on the main display after.
#[test]
fn each_display_keeps_its_position_across_reads() {
    support::with_term(&WS, |t| {
        support::feed(t, &[HOST, ON, b"ab"].concat());
        assert_eq!(t.active_display(), ActiveDisplay::Status);
        assert_eq!(
            t.status_line().and_then(|s| s.cursor),
            Some(Col::at_index(2))
        );
        // The main display's cursor, between reads, is the main display's.
        assert_eq!(t.cursor(), (Row::ZERO, Col::ZERO));
        support::feed(t, b"cd");
        assert_eq!(status(t), "abcd");
        support::feed(t, &[OFF, b"x", ON, b"e"].concat());
        assert_eq!(status(t), "abcde");
        assert_eq!(main_row(t, Row::from_index(0)), "x");
        support::feed(t, OFF);
        assert_eq!(t.status_line().and_then(|s| s.cursor), None);
    });
}

/// "If there is only one row in the Status Display, only the column
/// parameters in cursor positioning control functions (e.g. CUP, etc.)
/// ... are significant. Horizontal cursor positioning commands (e.g. CUF,
/// CUB) are recognized, but vertical cursor positioning commands (e.g.
/// CUD, CUU) are ignored. The line number shall be reported as 1, when
/// necessary" (070-14 p. 14-8).
#[test]
fn only_the_columns_position_the_cursor_on_the_line() {
    support::with_term(&WS, |t| {
        support::feed(
            t,
            &[HOST, ON, b"\x1b[5;7Ha\x1b[3Bb\x1b[2Ac\x1b[3Cd\x1b[6n"].concat(),
        );
        assert_eq!(status(t), "      abc   d");
        assert_eq!(show(t.reply()), show(b"\x1b[1;14R"));
    });
}

/// "If scrolling occurs as a result of a Format Effector, a single line
/// Host Writable Status Line is erased" (070-14 p. 14-9): a line feed, an
/// IND, a NEL and an RI on it blank it, and the history and the main
/// display gain nothing.
#[test]
fn scrolling_on_the_line_erases_it_and_keeps_no_history() {
    for fe in [
        b"\n".as_slice(),
        b"\x1bD",
        b"\x1bE",
        b"\x1bM",
        b"\x1b[S",
        b"\x1b[T",
    ] {
        support::with_term(&WS, |t| {
            // History behind the screen first, which a scroll that
            // retired a line, or that numbered the line's ring as the
            // primary's, would change.
            support::feed(t, b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n9\x1b[Htop");
            let hist = t.scrollback().len();
            assert!(hist > 0, "the premise: lines behind the screen");
            let mark = t.history_mark();
            support::feed(t, &[HOST, ON, b"abc", fe, b"d"].concat());
            let want = match fe {
                b"\x1bE" => "d",
                _ => "   d",
            };
            assert_eq!(status(t), want, "{}", show(fe));
            assert_eq!(t.scrollback().len(), hist, "{}", show(fe));
            assert_eq!(
                t.history_moved_since(mark),
                Moved::Retired(Rows::ZERO),
                "{}",
                show(fe)
            );
            assert_eq!(main_row(t, Row::from_index(0)), "top", "{}", show(fe));
        });
    }
}

/// ED, EL and ECH are significant on the line (070-14 p. 14-8, "commands
/// which affect areas dependent on character positions (e.g. ED, DCH,
/// etc.)"), and erase it alone.
#[test]
fn an_erase_on_the_line_erases_the_line_alone() {
    support::with_term(&WS, |t| {
        support::feed(t, b"keep");
        support::feed(t, &[HOST, ON, b"abcdef\x1b[3G\x1b[K"].concat());
        assert_eq!(status(t), "ab");
        support::feed(t, b"\x1b[2J");
        assert_eq!(status(t), "");
        assert_eq!(main_row(t, Row::from_index(0)), "keep");
    });
}

/// **What the line keeps of its own**, 070-14 §14.2.2: the rendition, the
/// character sets ("the Status Line could have the USASCII invoked into
/// G0, while the Main Display has the Line Drawing character set invoked
/// into G0") and the `DECSC` slot, which VT420 p. 271 keeps "separate ...
/// for the main display and the status line".
#[test]
fn the_line_keeps_its_own_rendition_charsets_and_saved_cursor() {
    support::with_term(&WS, |t| {
        // The main display: bold, line drawing in G0, a save at 3;3.
        support::feed(t, b"\x1b[1m\x1b(0\x1b[3;3H\x1b7\x1b[3;1H");
        support::feed(t, &[HOST, ON, b"q\x1b[5G\x1b7x"].concat());
        let line = t.status_line().expect("shown").cells;
        assert_eq!(
            line[0].content.as_codepoint().map(|c| c.value()),
            Some(u32::from(b'q'))
        );
        assert!(!t.style_of(line[0]).attrs().has(Attrs::BOLD));
        support::feed(t, &[OFF, b"q\x1b8y"].concat());
        // Line drawing's `q` on the main display, bold, and its own save.
        let main = t.row(Row::from_index(2)).expect("row");
        assert_eq!(
            main[0].content.as_codepoint().map(|c| c.value()),
            Some(0x2500)
        );
        assert!(t.style_of(main[0]).attrs().has(Attrs::BOLD));
        assert_eq!(t.cursor(), (Row::from_index(2), Col::at_index(3)));
        // The line's save is its own.
        support::feed(t, &[ON, b"\x1b8z"].concat());
        assert_eq!(status(t), "q   z");
    });
}

/// **Autowrap is one mode and two pending wraps**: DECAWM set on the
/// main display wraps on the line, and a wrap pending at the main
/// display's last column waits there while the line is written.
#[test]
fn autowrap_is_shared_and_the_pending_wrap_is_each_displays() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?7l");
        support::feed(t, &[HOST, ON, b"abcdefghijklmnopqrstuvwxyz"].concat());
        assert_eq!(status(t), "abcdefghijklmnopqrsz");
        support::feed(t, &[OFF, b"\x1b[?7h\x1b[1;1H01234567890123456789"].concat());
        support::feed(t, &[ON, b"\x1b[1GABC", OFF, b"!"].concat());
        assert_eq!(main_row(t, Row::from_index(0)), "01234567890123456789");
        assert_eq!(main_row(t, Row::from_index(1)), "!");
    });
}

/// **Exits and resets**: DECSTR "The Status Display will be exited, but
/// not erased"; RIS "The Status Display will be erased and exited"
/// (070-14 pp. 14-9 and 14-10), and power-on has no line; DECSCL "performs
/// a Soft Terminal Reset". DECSSDT to another type "will be exited" (p.
/// 14-9), and a type changed is initialized, the host-writable line
/// cleared (p. 14-12).
#[test]
fn the_resets_exit_the_line_as_070_says() {
    let written = [HOST, ON, b"abc"].concat();
    let cases: [(&[u8], StatusType, &str); 3] = [
        (b"\x1b[!p", StatusType::HostWritable, "abc"),
        (b"\x1b[64;1\"p", StatusType::HostWritable, "abc"),
        (b"\x1b[1$~", StatusType::Indicator, ""),
    ];
    for (reset, ty, left) in cases {
        support::with_term(&WS, |t| {
            support::feed(t, &written);
            support::feed(t, reset);
            assert_eq!(t.active_display(), ActiveDisplay::Main, "{}", show(reset));
            assert_eq!(t.status_type(), ty, "{}", show(reset));
            if ty == StatusType::HostWritable {
                assert_eq!(status(t), left, "{}", show(reset));
            }
            support::feed(t, b"m");
            assert_eq!(main_row(t, Row::from_index(0)), "m", "{}", show(reset));
        });
    }
    // The type it has is no change, and leaves the line selected.
    support::with_term(&WS, |t| {
        support::feed(t, &[written.as_slice(), HOST, b"d"].concat());
        assert_eq!(t.active_display(), ActiveDisplay::Status);
        assert_eq!(status(t), "abcd");
    });
    support::with_term(&WS, |t| {
        support::feed(t, &written);
        support::feed(t, b"\x1bc");
        assert_eq!(t.active_display(), ActiveDisplay::Main);
        assert_eq!(t.status_type(), StatusType::None);
        support::feed(t, HOST);
        assert_eq!(status(t), "");
    });
}

/// "DECCOLM clears data from the status line if the status line is set to
/// host-writable" (`[[source: vt510-deccolm]]`; 070-14 p. 14-9, "Receipt
/// of this sequence will clear both the Main Display and the Host Writable
/// Status Line. The Status Display cursor is moved to the home position").
#[test]
fn deccolm_clears_the_line_and_homes_its_cursor() {
    support::with_term(&WS, |t| {
        support::feed(t, &[HOST, ON, b"abc", OFF, b"\x1b[?40h\x1b[?3h"].concat());
        assert_eq!(status(t), "");
        support::feed(t, &[ON, b"x"].concat());
        assert_eq!(status(t), "x");
    });
}

/// **And with `?95` set, it homes the line's cursor still**: "The Status
/// Display cursor is moved to the home position" (070-14 item 5, p. 14-9),
/// and `?95` withholds the clear and nothing else ("When enabled, a column
/// mode change ... does not clear the screen", `[[source:
/// vt510-decncsm]]`), as the main display's cursor goes home under it.
/// Received on either display, the line's cursor stayed where it was.
#[test]
fn deccolm_homes_the_lines_cursor_with_the_screen_kept() {
    let deccolm = b"\x1b[?95h\x1b[?40h\x1b[?3l";
    for from in [deccolm.to_vec(), [ON, deccolm.as_slice(), OFF].concat()] {
        support::with_term(&WS, |t| {
            support::feed(t, &[HOST, ON, b"abc", OFF].concat());
            support::feed(t, &from);
            t.clear_reply();
            support::feed(t, &[ON, b"\x1b[6n", OFF].concat());
            assert_eq!(show(t.reply()), "\\x1b[1;1R", "{}", show(&from));
            assert_eq!(status(t), "abc", "`?95` keeps the text: {}", show(&from));
        });
    }
}

/// **DECCOLM's home ends a wrap waiting on the line**, as every move ends
/// the Last Column Flag (`[[source: dec-std-070]]` D.6.1) and as the main
/// display's home ends its own: under DECRLM a character written at the
/// line's left margin leaves a wrap waiting there, and after the home the
/// next character went to the next line, erasing the line, rather than
/// home.
#[test]
fn deccolm_ends_a_wrap_waiting_on_the_line() {
    let line = |keep: &[u8]| {
        support::with_term(&WS, |t| {
            support::feed(t, &[HOST, ON, b"\x1b[?34ha", OFF].concat());
            support::feed(t, &[keep, b"\x1b[?40h\x1b[?3l"].concat());
            support::feed(t, &[ON, b"b", OFF].concat());
            status(t)
        })
    };
    // Cleared, or kept with `?95` set, where the `b` overwrites the `a`:
    // home either way.
    assert_eq!([line(b""), line(b"\x1b[?95h")], ["b", "b"]);
}

/// "DECANM - ANSI/VT52 Mode: This sequence is ignored if received in the
/// Status Display" (070-14 p. 14-8).
#[test]
fn decanm_is_ignored_on_the_line() {
    support::with_term(&WS, |t| {
        support::feed(t, &[HOST, ON, b"\x1b[?2l\x1b[3CA"].concat());
        assert_eq!(status(t), "   A");
    });
}

/// "Status Display is an extension to Level 2. DECSSDT and DECSASD do not
/// work in Level 1 (VT100 mode)" (070-14 p. 14-13): at Level 1 both are
/// counted and change nothing.
#[test]
fn level_1_does_not_have_them() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[61\"p");
        let before = t.unsupported;
        support::feed(t, &[HOST, ON].concat());
        assert_eq!(t.unsupported, before + 2);
        assert_eq!(t.status_type(), StatusType::None);
    });
}

/// "Tabulation stops affect both the Main and Status displays" and IRM
/// "affects both" (070-14 pp. 14-9 and 14-10).
#[test]
fn tabs_and_irm_are_shared() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[3g\x1b[1;6H\x1bH\x1b[4h");
        support::feed(t, &[HOST, ON, b"bc\x1b[1Ga\tx"].concat());
        assert_eq!(status(t), "abc  x");
    });
}

// ---- the indicator --------------------------------------------------------

fn indicator(t: &Term<'_>) -> String {
    let line = t.status_line().expect("an indicator");
    for c in line.cells {
        assert!(
            t.style_of(*c).attrs().has(Attrs::REVERSE),
            "the indicator is in reverse video"
        );
    }
    text(line.cells)
}

/// "The indicator status display provides information about the current
/// state of the terminal. Page number, Text cursor position (line, column)
/// ... It appears in reverse video" (`[[source: vt420-rm]]` p. 219): the
/// page, baud's one, and the cursor, as the VT340's "1(24,008)" (070-14
/// p. 14-6). It follows the cursor.
#[test]
fn the_indicator_shows_the_page_and_the_cursor_in_reverse_video() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[1$~");
        assert_eq!(indicator(t), "    1(01,001)");
        support::feed(t, b"\x1b[5;17H");
        assert_eq!(indicator(t), "    1(05,017)");
    });
}

/// The indicator's width, the VT420's 80 columns: room for the printer
/// field at column 42, where the VT340's line has it (070-14 p. 14-6).
const WIDE: WinSize = WinSize {
    ws_row: 6,
    ws_col: 80,
};

/// The line's text past column 40: the printer field, and the modem field
/// after it, which baud leaves blank.
fn printer_field(t: &Term<'_>) -> String {
    indicator(t).get(41..).unwrap_or("").to_string()
}

/// **The printer field shows the printer's state and the print mode**, in
/// the VT420's words (`[[source: vt420-uu-ch4]]`, Table 4-2, field 3):
/// None with no printer since power-up, Ready while the embedder has one,
/// Not Ready once it is lost, and Auto Print or Controller while a run is
/// on, whatever the port's state. The modem field is blank, "unless you
/// have a modem connected", and a pty has none. `Term::set_printer`
/// redraws it between reads.
#[test]
fn the_indicator_shows_the_printer_and_the_print_mode() {
    let mut owned = Owned::new(&WIDE).with_printer(PrintBudget::of_bytes(4096));
    let mut t = Term::new(owned.storage(), &WIDE);
    support::feed(&mut t, b"\x1b[1$~");
    assert_eq!(printer_field(&t), "Printer: None");
    t.set_printer(PrinterPort::Ready);
    assert_eq!(printer_field(&t), "Printer: Ready");
    support::feed(&mut t, b"\x1b[?5i");
    assert_eq!(printer_field(&t), "Printer: Auto Print");
    support::feed(&mut t, b"\x1b[?4i");
    assert_eq!(printer_field(&t), "Printer: Ready");
    support::feed(&mut t, b"\x1b[5i");
    assert_eq!(printer_field(&t), "Printer: Controller");
    support::feed(&mut t, b"\x1b[4i");
    assert_eq!(printer_field(&t), "Printer: Ready");
    t.set_printer(PrinterPort::Absent);
    assert_eq!(printer_field(&t), "Printer: Not Ready");
    // A run's mode shows over the port's state: its prints are queued.
    support::feed(&mut t, b"\x1b[?5i");
    assert_eq!(printer_field(&t), "Printer: Auto Print");
    support::feed(&mut t, b"\x1b[?4i");
    assert_eq!(
        indicator(&t),
        format!("    1(01,001){}Printer: Not Ready", " ".repeat(28))
    );
}

/// The indicator of a terminal `cols` wide and 120 rows tall, a printer
/// attached at `port` and `bytes` fed after the line is shown.
fn indicator_at(cols: u16, port: Option<PrinterPort>, bytes: &[u8]) -> String {
    let ws = WinSize {
        ws_row: 120,
        ws_col: cols,
    };
    let mut owned = Owned::new(&ws).with_printer(PrintBudget::of_bytes(4096));
    let mut t = Term::new(owned.storage(), &ws);
    if let Some(p) = port {
        t.set_printer(p);
    }
    support::feed(&mut t, b"\x1b[1$~");
    support::feed(&mut t, bytes);
    indicator(&t)
}

/// **A field the line cannot hold whole is left out whole**, so no field
/// shows a cut, wrong value: at 52 columns `Printer: None`, which needs
/// 54, is not shown as `Printer: No`, and at 12 columns with the cursor
/// on row 100, column 12, the position, which needs 14, is not shown as
/// `1(100,01`, which reads as column 1. The Active Position Indicator
/// "Displays the current cursor position, row and column" and the
/// Printer Status Indicator "Indicates the status of the attached
/// printer" (`[[source: dec-std-070]]` p. 14-6), and a field cut at the
/// window's edge does neither. Each field appears at the width its last
/// character needs, and not one column before.
#[test]
fn a_field_the_line_cannot_hold_whole_is_left_out_whole() {
    let none = |cols| indicator_at(cols, None, b"");
    assert_eq!(
        (none(52), indicator_at(12, None, b"\x1b[100;12H")),
        ("    1(01,001)".to_string(), String::new()),
        "Printer: None needs 54 columns and 1(100,012) needs 14"
    );
    let printer = format!("    1(01,001){}Printer: None", " ".repeat(28));
    assert_eq!(none(53), "    1(01,001)");
    assert_eq!(none(54), printer);
    let controller = |cols| indicator_at(cols, Some(PrinterPort::Ready), b"\x1b[5i");
    assert_eq!(
        controller(59),
        "    1(01,001)",
        "Printer: Controller needs 60"
    );
    assert_eq!(
        controller(60),
        format!("    1(01,001){}Printer: Controller", " ".repeat(28))
    );
    assert_eq!(none(12), "", "1(01,001) needs 13");
    assert_eq!(none(13), "    1(01,001)");
    assert_eq!(indicator_at(13, None, b"\x1b[100;12H"), "");
    assert_eq!(indicator_at(14, None, b"\x1b[100;12H"), "    1(100,012)");
}

/// **Every width from 1 to 80 shows each field whole or not at all**,
/// with all four on: a three-digit row, Wait, the locator and the
/// longest printer value. Their fixed columns leave the printer out
/// first, then the locator, then Wait, and the cursor position last, as
/// each field's last column is before the next field's first.
#[test]
fn every_width_shows_each_field_whole_or_not_at_all() {
    const FIELDS: [(usize, &str); 4] = [
        (4, "1(100,001)"),
        (16, "Wait"),
        (23, "[1,1]"),
        (41, "Printer: Controller"),
    ];
    let mut seen = [0usize; 4];
    for cols in 1..=80u16 {
        let ws = WinSize {
            ws_row: 120,
            ws_col: cols,
        };
        let mut owned = Owned::new(&ws).with_printer(PrintBudget::of_bytes(4096));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_locator_device(LocatorDevice::Present);
        t.set_printer(PrinterPort::Ready);
        support::feed(&mut t, b"\x1b[1$~\x1b[100;1H\x1b[1;2'z\x1b[1'{");
        let moved = LocatorInput {
            kind: LocatorKind::Moved,
            at: Some(LocatorAt {
                col: Col::ZERO,
                row: Row::ZERO,
                px: None,
            }),
            held: LocatorButtons::NONE,
        };
        assert!(!t.locator(moved), "{cols} columns: no report on a move");
        support::feed(&mut t, b"\x1b[2h\x1b[5i");
        let mut want = String::new();
        for (k, (at, field)) in FIELDS.into_iter().enumerate() {
            if at + field.len() <= usize::from(cols) {
                want = format!("{want:<at$}{field}");
                seen[k] += 1;
            }
        }
        assert_eq!(indicator(&t), want, "{cols} columns");
    }
    // Each field is shown from the width its last column needs.
    assert_eq!(seen, [80 - 13, 80 - 19, 80 - 27, 80 - 59]);
}

// ---- the embedder's state lamps -----------------------------------------

/// Every lamp lit, CD and OH as read.
const LIT: IndicatorLamps = IndicatorLamps {
    cd: ReadLamp::Read(LampState::Lit),
    oh: ReadLamp::Read(LampState::Lit),
    cs: LampState::Lit,
    fax: LampState::Lit,
    syn: LampState::Lit,
};

/// The indicator's text from column 62, one-based, where the lamps begin.
fn lamps_text(t: &Term<'_>) -> String {
    indicator(t).get(61..).unwrap_or("").to_string()
}

/// **The five state lamps are words in their own columns after the
/// printer field**: CD at 62, OH at 66, CS at 70, FAX at 73 and SYN at
/// 77, one-based, a space at least between each and the printer field's
/// longest value, `Printer: Controller`, which ends at 60 (070-14 p.
/// 14-5, "fixed positions ... at least one space"). A lamp is present
/// while its condition holds and its columns are blank while it is dark,
/// as the Edit indicator is "only visible when DECEDM is active" (p.
/// 14-7); CD and OH carry the unknown mark while baud cannot read them.
#[test]
fn the_state_lamps_are_words_in_their_columns_after_the_printer() {
    let mut owned = Owned::new(&WIDE).with_printer(PrintBudget::of_bytes(4096));
    let mut t = Term::new(owned.storage(), &WIDE);
    t.set_printer(PrinterPort::Ready);
    support::feed(&mut t, b"\x1b[1$~\x1b[5i");
    assert_eq!(t.indicator_lamps(), None);
    assert_eq!(lamps_text(&t), "", "no lamps until the embedder gives them");
    t.set_indicator_lamps(Some(LIT));
    assert_eq!(t.indicator_lamps(), Some(LIT));
    assert_eq!(
        indicator(&t),
        format!(
            "    1(01,001){}Printer: Controller CD  OH  CS FAX SYN",
            " ".repeat(28)
        )
    );
    assert_eq!(lamps_text(&t), "CD  OH  CS FAX SYN");
    let unknown = IndicatorLamps {
        cd: ReadLamp::Unknown,
        oh: ReadLamp::Unknown,
        ..LIT
    };
    t.set_indicator_lamps(Some(unknown));
    assert_eq!(lamps_text(&t), "CD? OH? CS FAX SYN");
    // Each alone, in its own columns.
    let alone = [
        (IndicatorLamp::CarrierDetect, "CD"),
        (IndicatorLamp::OffHook, "    OH"),
        (IndicatorLamp::ClearToSend, "        CS"),
        (IndicatorLamp::Fax, "           FAX"),
        (IndicatorLamp::Synchronous, "               SYN"),
    ];
    for (lamp, want) in alone {
        let lit = |l: IndicatorLamp| match l == lamp {
            true => LampState::Lit,
            false => LampState::Dark,
        };
        let one = IndicatorLamps {
            cd: ReadLamp::Read(lit(IndicatorLamp::CarrierDetect)),
            oh: ReadLamp::Read(lit(IndicatorLamp::OffHook)),
            cs: lit(IndicatorLamp::ClearToSend),
            fax: lit(IndicatorLamp::Fax),
            syn: lit(IndicatorLamp::Synchronous),
        };
        t.set_indicator_lamps(Some(one));
        assert_eq!(lamps_text(&t), want, "{lamp:?}");
    }
    // Every lamp dark is the line with no lamps.
    t.set_indicator_lamps(Some(IndicatorLamps::DARK));
    let dark = indicator(&t);
    t.set_indicator_lamps(None);
    assert_eq!(dark, indicator(&t));
    assert_eq!(lamps_text(&t), "");
}

/// **The lamps are drawn the moment the embedder gives them, and every
/// read's end draws them again**: "The interval between the change in the
/// state of a feature and the update of the appropriate indicator shall
/// not exceed 80 milliseconds. Indicator changes are not queued" (070-14
/// p. 14-6). A read redraws the line from the terminal's state, so lamps
/// held anywhere but there would be wiped by the next byte.
#[test]
fn the_lamps_are_drawn_at_once_and_kept_across_reads() {
    support::with_term(&WIDE, |t| {
        support::feed(t, b"\x1b[1$~");
        t.set_indicator_lamps(Some(LIT));
        assert_eq!(lamps_text(t), "CD  OH  CS FAX SYN", "drawn between reads");
        support::feed(t, b"x");
        assert_eq!(lamps_text(t), "CD  OH  CS FAX SYN", "a read keeps them");
        assert_eq!(&indicator(t)[..13], "    1(01,002)", "and moves the cursor");
        let syn_off = IndicatorLamps {
            syn: LampState::Dark,
            ..LIT
        };
        t.set_indicator_lamps(Some(syn_off));
        assert_eq!(lamps_text(t), "CD  OH  CS FAX");
    });
}

/// **The lamps are the embedder's facts, shown only with the indicator**:
/// not on the host-writable line, which is the program's, nor while no
/// line is shown; kept across `RIS` and a change of type, as the printer
/// port is, and back the moment the indicator is.
#[test]
fn the_lamps_show_only_with_the_indicator_and_outlive_ris() {
    support::with_term(&WIDE, |t| {
        t.set_indicator_lamps(Some(LIT));
        assert!(t.status_line().is_none());
        support::feed(t, &[HOST, ON, b"hello", OFF].concat());
        assert_eq!(status(t), "hello", "the program's line carries none");
        assert_eq!(t.indicator_lamp_at(Col::at_index(61)), None);
        support::feed(t, b"\x1b[1$~");
        assert_eq!(lamps_text(t), "CD  OH  CS FAX SYN");
        support::feed(t, b"\x1bc");
        assert!(t.status_line().is_none(), "RIS exits the line");
        assert_eq!(t.indicator_lamps(), Some(LIT), "and keeps the lamps");
        assert_eq!(t.indicator_lamp_at(Col::at_index(61)), None);
        support::feed(t, b"\x1b[1$~");
        assert_eq!(lamps_text(t), "CD  OH  CS FAX SYN");
    });
}

/// **Each word whole or not at all**, as every field is: at each width
/// from 61 to 80 a word appears at the width its last character needs and
/// not one column before. At 132 columns the words keep the columns they
/// have at 80, and the 52 past them stay blank, "This will leave 52 blank
/// spaces on the right hand side of the Indicator Status Line in 132
/// Column Mode" (070-14 p. 14-5).
#[test]
fn each_lamp_is_whole_or_not_at_all_and_within_the_first_80() {
    let unknown = IndicatorLamps {
        cd: ReadLamp::Unknown,
        oh: ReadLamp::Unknown,
        ..LIT
    };
    const WORDS: [(usize, &str); 5] = [
        (61, "CD?"),
        (65, "OH?"),
        (69, "CS"),
        (72, "FAX"),
        (76, "SYN"),
    ];
    for cols in 61..=80u16 {
        let ws = WinSize {
            ws_row: 6,
            ws_col: cols,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        support::feed(&mut t, b"\x1b[1$~");
        let mut want = indicator(&t);
        t.set_indicator_lamps(Some(unknown));
        for (at, word) in WORDS {
            if at + word.len() <= usize::from(cols) {
                want = format!("{want:<at$}{word}");
            }
        }
        assert_eq!(indicator(&t), want, "{cols} columns");
    }
    let ws = WinSize {
        ws_row: 6,
        ws_col: 132,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    t.set_indicator_lamps(Some(unknown));
    support::feed(&mut t, b"\x1b[1$~");
    let line = indicator(&t);
    assert_eq!(line.get(61..), Some("CD? OH? CS FAX SYN"));
    assert_eq!(line.len(), 79, "nothing past column 79");
}

/// **The lamp under a column is the word drawn there**, for an
/// embedder's hover text: each word's columns name its lamp, the mark's
/// column too, and a space, a dark lamp's blank columns, a word the line
/// cannot hold whole and every column of a line that is not the
/// indicator name none.
#[test]
fn the_lamp_at_a_column_is_the_word_drawn_there() {
    let at = |t: &Term<'_>| -> Vec<Option<IndicatorLamp>> {
        (0..80)
            .map(|x| t.indicator_lamp_at(Col::at_index(x)))
            .collect()
    };
    support::with_term(&WIDE, |t| {
        t.set_indicator_lamps(Some(IndicatorLamps {
            cd: ReadLamp::Unknown,
            oh: ReadLamp::Read(LampState::Dark),
            ..LIT
        }));
        assert_eq!(at(t), vec![None; 80], "no indicator shown");
        support::feed(t, b"\x1b[1$~");
        let mut want = vec![None; 80];
        for (cols, lamp) in [
            (61..64, IndicatorLamp::CarrierDetect),
            (69..71, IndicatorLamp::ClearToSend),
            (72..75, IndicatorLamp::Fax),
            (76..79, IndicatorLamp::Synchronous),
        ] {
            want[cols].fill(Some(lamp));
        }
        assert_eq!(at(t), want);
        assert_eq!(lamps_text(t), "CD?     CS FAX SYN");
    });
    // SYN needs 79 columns: at 78 it is not drawn, and names nothing.
    let ws = WinSize {
        ws_row: 6,
        ws_col: 78,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    t.set_indicator_lamps(Some(LIT));
    support::feed(&mut t, b"\x1b[1$~");
    assert_eq!(lamps_text(&t), "CD  OH  CS FAX");
    assert_eq!(t.indicator_lamp_at(Col::at_index(76)), None);
    assert_eq!(
        t.indicator_lamp_at(Col::at_index(74)),
        Some(IndicatorLamp::Fax)
    );
}

/// **The locator's field shows only while the locator reports**: "The
/// Graphics Input Position Indicator is only visible when graphics input
/// is enabled" (070-14 p. 14-7), so a one-shot DECELR's report, which
/// disables it, takes the field away with the pointer event that sent it,
/// as DECELR 0 does at the end of its read.
#[test]
fn the_locator_field_goes_when_a_one_shot_report_disables_the_locator() {
    support::with_term(&WIDE, |t| {
        t.set_locator_device(LocatorDevice::Present);
        support::feed(t, b"\x1b[1$~\x1b[2;2'z\x1b[1'{");
        let at = Some(LocatorAt {
            col: Col::at_index(6),
            row: Row::from_index(2),
            px: None,
        });
        let moved = LocatorInput {
            kind: LocatorKind::Moved,
            at,
            held: LocatorButtons::NONE,
        };
        assert!(!t.locator(moved));
        assert_eq!(&indicator(t)[..28], "    1(01,001)          [3,7]");
        let press = LocatorInput {
            kind: LocatorKind::Press(LocatorButton::Left),
            at,
            held: LocatorButtons::NONE.pressed(LocatorButton::Left),
        };
        assert!(t.locator(press), "the one-shot report");
        assert_eq!(t.mouse_tracking(), MouseTracking::Off);
        assert_eq!(indicator(t)[23..28].trim(), "");
        // DECELR 0 from the byte stream, at the read's end.
        support::feed(t, b"\x1b[1;2'z");
        assert!(!t.locator(moved));
        assert_eq!(&indicator(t)[23..28], "[3,7]");
        support::feed(t, b"\x1b[0'z");
        assert_eq!(indicator(t)[23..28].trim(), "");
    });
}

/// "When the keyboard is locked ... the Wait indicator appears on the
/// keyboard indicator line at the bottom of the screen" (`[[source:
/// vt420-rm]]`, Unlocking the Keyboard; `[[source: vt510-kam]]`).
#[test]
fn the_indicator_shows_wait_while_the_keyboard_is_locked() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[1$~\x1b[2h");
        assert_eq!(indicator(t), "    1(01,001)   Wait");
        support::feed(t, b"\x1b[2l");
        assert_eq!(indicator(t), "    1(01,001)");
    });
}

/// The line is the main display's width, and a resize carries it.
#[test]
fn the_line_is_the_main_displays_width() {
    support::with_term(&WS, |t| {
        support::feed(t, &[HOST, ON, b"0123456789abcdefghij", OFF].concat());
        let narrow = WinSize {
            ws_row: 6,
            ws_col: 10,
        };
        assert_eq!(t.resize(&narrow), Resized::Ok);
        assert_eq!(t.status_line().map(|s| s.cells.len()), Some(10));
        assert_eq!(status(t), "0123456789");
        assert_eq!(t.resize(&WS), Resized::Ok);
        assert_eq!(status(t), "0123456789");
    });
}

/// **The indicator after a resize shows the cursor the resize left**: a
/// narrower screen keeps the cursor inside its double-width line's half,
/// 20 columns of 40, and the line is drawn again after that clamp, not
/// before it.
#[test]
fn the_indicator_after_a_resize_shows_the_cursor_it_left() {
    support::with_term(&WIDE, |t| {
        support::feed(t, b"\x1b[1$~\x1b[5;1H\x1b#6\x1b[5;30H");
        assert_eq!(&indicator(t)[..13], "    1(05,030)");
        let narrow = WinSize {
            ws_row: 6,
            ws_col: 40,
        };
        assert_eq!(t.resize(&narrow), Resized::Ok);
        assert_eq!(&indicator(t)[..13], "    1(05,020)");
    });
}

// ---- the row ---------------------------------------------------------------

/// **The line's row is the embedder's**, as every buffer the core writes
/// is: lent through `Storage::with_status_line`, the line's text lands in
/// it. `Owned` lends one of its own.
#[test]
fn the_line_writes_the_row_the_embedder_lends() {
    let mut owned = Owned::new(&WS);
    let mut row = vec![PackedCell::EMPTY; owned.stride().index()];
    let mut used = vec![RowMeta::ZERO; 1];
    {
        let store = owned
            .storage()
            .with_status_line(StatusBuf::new(&mut row, &mut used));
        let mut t = Term::new(store, &WS);
        support::feed(&mut t, &[HOST, ON, b"hi", OFF].concat());
        assert_eq!(status(&t), "hi");
    }
    assert_eq!(text(&row[..2]), "hi");
}

/// **An empty row declines the line**: DECSSDT 1 and 2 are counted and
/// show nothing, and DECRQSS answers that there is no line.
#[test]
fn a_storage_that_declines_the_row_has_no_line() {
    let mut owned = Owned::new(&WS);
    let store = owned
        .storage()
        .with_status_line(StatusBuf::new(&mut [], &mut []));
    let mut t = Term::new(store, &WS);
    let before = t.unsupported;
    support::feed(&mut t, b"\x1b[1$~\x1b[2$~\x1b[1$}x");
    assert_eq!(t.unsupported, before + 2);
    assert_eq!(t.status_type(), StatusType::None);
    assert_eq!(t.status_rows(), Rows::ZERO);
    assert!(t.status_line().is_none());
    assert_eq!(main_row(&t, Row::ZERO), "x");
    t.clear_reply();
    support::feed(&mut t, b"\x1bP$q$~\x1b\\");
    assert_eq!(show(t.reply()), show(b"\x1bP1$r0$~\x1b\\"));
}

/// **`OSC 62` names `StatusLine` and `OSC 61` never does**: the line is
/// one of `allowWindowOps`' subcategories in xterm's table, and baud keeps
/// it with the operations off -- the window keeps its size and the grid
/// gives the line its row -- so nothing about it is refused.
#[test]
fn the_feature_queries_name_the_status_line_truthfully() {
    support::with_term(&WS, |t| {
        t.set_window_ops(WindowOps::Off);
        support::feed(t, b"\x1b]62;allowWindowOps\x07");
        assert!(t.reply().ends_with(b",SetXprop,StatusLine\x07"));
        t.clear_reply();
        support::feed(t, b"\x1b]61;allowWindowOps\x07");
        let refused = String::from_utf8_lossy(t.reply()).to_string();
        assert!(refused.contains("SetXprop"), "{refused}");
        assert!(!refused.contains("StatusLine"), "{refused}");
        support::feed(t, &[HOST, ON, b"up"].concat());
        assert_eq!(status(t), "up");
    });
}

// ---- what the line does not reach --------------------------------------------

const TINY: &[u8] = b"\x1bP0;1;0q#0;2;100;0;0~\x1b\\";
const KITTY: &[u8] = b"\x1b_Ga=T,f=24,s=1,v=1;AAAA\x1b\\";

/// **The main display's images stay where they are**: text and ED 2 on the
/// line free none of them -- on the alternate screen too, the screen the
/// line reads as while it is written -- and a Sixel, a kitty image or a
/// ReGIS page sent to the line is kept and discarded, and counted: an image
/// names a line of the main display, which the line is not.
#[test]
fn the_line_leaves_the_main_displays_images_alone() {
    for screen in [b"".as_slice(), b"\x1b[?1049h"] {
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4 * (64 * 48 + 64)));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
        support::feed(&mut t, screen);
        support::feed(&mut t, b"\x1b[H");
        // Both on the first row, the one the line's row would name if it
        // were numbered as the main display's.
        support::feed(&mut t, TINY);
        support::feed(&mut t, b"\x1b[1;10H");
        support::feed(&mut t, KITTY);
        let n = t.images().placements().count();
        assert_eq!(n, 2, "the premise, {}", show(screen));
        support::feed(&mut t, &[HOST, ON, b"over the images\x1b[2J", OFF].concat());
        assert_eq!(t.images().placements().count(), n, "ED, {}", show(screen));
        // The selective erases too, over text on the line.
        support::feed(
            &mut t,
            &[HOST, ON, b"\x1b[1Gover\x1b[?2J\x1b[1Gagain\x1b[?2K", OFF].concat(),
        );
        assert_eq!(t.images().placements().count(), n, "{}", show(screen));
        // A scroll on the line moves none of them up and off.
        support::feed(&mut t, &[ON, b"\n\x1bD\x1b[S", OFF].concat());
        assert_eq!(
            t.images().placements().count(),
            n,
            "scrolled, {}",
            show(screen)
        );
        let ignored = t.strings_ignored;
        t.clear_reply();
        support::feed(
            &mut t,
            &[ON, TINY, KITTY, b"\x1bPpP[10,20]R(P)\x1b\\", OFF].concat(),
        );
        assert_eq!(t.strings_ignored, ignored + 3, "{}", show(screen));
        assert_eq!(t.images().placements().count(), n, "nothing placed");
        assert!(
            t.reply().is_empty(),
            "ReGIS was never entered: {:?}",
            t.reply()
        );
    }
}

/// Four thousand distinct truecolour renditions written over one cell of
/// the main display's first row, each dying as the next replaces it.
fn churn() -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..4000u16 {
        let (a, b) = (i & 255, i >> 8);
        out.extend(format!("\x1b[1;1H\x1b[0;38;2;{a};{b};7mC").as_bytes());
    }
    out
}

/// **A sweep keeps what the line names**: a cell's rendition, and the
/// rendition the line was left in, which no cell names, while the main
/// display mints renditions past the table's floor and every one dies.
#[test]
fn a_sweep_keeps_the_lines_renditions() {
    support::with_term(&WS, |t| {
        // `S` in one rendition, `U` in a second, and `U` erased, so only
        // the line's parked state names the second.
        support::feed(
            t,
            &[HOST, ON, b"\x1b[38;2;1;2;3mS\x1b[38;2;4;5;6mU"].concat(),
        );
        let cells = t.status_line().expect("shown").cells;
        let (cell, left) = (t.style_of(cells[0]), t.style_of(cells[1]));
        support::feed(t, &[b"\x1b[D\x1b[X".as_slice(), OFF].concat());
        let sweeps = t.style_reclaims().sweeps;
        support::feed(t, &churn());
        assert!(t.style_reclaims().sweeps > sweeps, "the premise: it swept");
        let line = t.status_line().expect("shown").cells[0];
        assert_eq!(t.style_of(line), cell, "the line's cell");
        support::feed(t, &[ON, b"T", OFF].concat());
        let next = t.status_line().expect("shown").cells[1];
        assert_eq!(t.style_of(next), left, "the rendition the line was left in");
    });
}

/// **A cluster sweep keeps the line's clusters** while the main display
/// mints more than the store holds.
#[test]
fn a_cluster_sweep_keeps_the_lines_clusters() {
    support::with_term(&WS, |t| {
        support::feed(t, &[HOST, ON, "e\u{301}".as_bytes(), OFF].concat());
        let line = t.status_line().expect("shown").cells[0];
        assert_eq!(t.cluster_marks(line).collect::<Vec<_>>(), [0x301]);
        // A letter and one combining mark, every pair of the 52 letters
        // and U+0300-036F, each written over the first cell.
        let mut many = String::new();
        let letters = ('a'..='z').chain('A'..='Z');
        for m in 0x0300..=0x036Fu32 {
            for b in letters.clone() {
                let mark = char::from_u32(m).expect("a mark");
                many.push_str(&format!("\x1b[1;1H{b}{mark}"));
            }
        }
        support::feed(t, many.as_bytes());
        assert!(t.cluster_sweeps().freed > 0, "the premise: a sweep freed");
        let line = t.status_line().expect("shown").cells[0];
        assert_eq!(t.cluster_marks(line).collect::<Vec<_>>(), [0x301]);
    });
}

/// **DECLRMM's single-width pages are the main display's too**: set from
/// the line, a double-width line of the main display goes single-width.
#[test]
fn declrmm_from_the_line_resets_the_main_displays_lines() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b#6wide");
        assert!(t.line_rendition(Row::ZERO).is_some_and(|r| r.is_double()));
        support::feed(t, &[HOST, ON, b"\x1b[?69h", OFF].concat());
        assert_eq!(t.line_rendition(Row::ZERO), Some(LineRendition::Single));
    });
}

/// What each display answers for DECLRMM and its margins, the line's
/// read with it selected.
fn margins(t: &mut Term<'_>, on_line: bool) -> String {
    let ask = b"\x1b[?69$p\x1bP$qs\x1b\\";
    t.clear_reply();
    match on_line {
        true => support::feed(t, &[ON, ask.as_slice(), OFF].concat()),
        false => support::feed(t, ask),
    }
    show(t.reply())
}

/// **DECLRMM is the terminal's, and its reset puts back both displays'
/// margins**: each display keeps its own margins, and "In the reset state,
/// the left and right scrolling margins in every page ... are set to be at
/// the extremes of the page" (`[[source: dec-std-070]]` p. 5-29). Reset on
/// the line, the main display's margins stayed where DECSLRM set them, so
/// it wrapped at column 16 with the mode reset and DECRQSS named a margin
/// DECRQM said was off; a debug build asserted at the first move. Reset on
/// the main display, the line kept its own and wrapped there.
#[test]
fn declrmm_reset_on_either_display_puts_back_both_displays_margins() {
    let off: [(&[u8], &str); 2] = [
        (&[ON, b"\x1b[?69l", OFF].concat(), "on the line"),
        (b"\x1b[?69l", "on the main display"),
    ];
    for (reset, at) in off {
        support::with_term(&WS, |t| {
            support::feed(t, b"\x1b[?69h\x1b[5;16s");
            support::feed(t, &[HOST, ON, b"\x1b[3;12s", OFF].concat());
            support::feed(t, reset);
            let edges = "\\x1b[?69;2$y\\x1bP1$r1;20s\\x1b\\\\";
            assert_eq!(margins(t, false), edges, "the main display, {at}");
            assert_eq!(margins(t, true), edges, "the line, {at}");
            support::feed(t, b"\x1b[Habcdefghijklmnopqrst");
            assert_eq!(main_row(t, Row::ZERO), "abcdefghijklmnopqrst", "{at}");
            support::feed(t, &[ON, b"\rABCDEFGHIJKLMNOPQRST", OFF].concat());
            assert_eq!(status(t), "ABCDEFGHIJKLMNOPQRST", "{at}");
        });
    }
}

/// **DECCOLM puts back the line's margins as the main display's**, with
/// the screen cleared or kept (`?95`): it "affects both the Main Display
/// and the Status Display simultaneously" (070-14 p. 14-9), eliminates
/// the scrolling region (p. 5-71) and resets DECLRMM, the terminal's
/// (`[[source: vt510-deccolm]]`). The line kept its margins under the mode
/// reset and wrapped at them.
#[test]
fn deccolm_puts_back_the_lines_margins() {
    for keep in [b"".as_slice(), b"\x1b[?95h"] {
        support::with_term(&WS, |t| {
            support::feed(t, &[HOST, ON, b"\x1b[?69h\x1b[5;16s", OFF].concat());
            support::feed(t, &[keep, b"\x1b[?40h\x1b[?3l"].concat());
            let edges = "\\x1b[?69;2$y\\x1bP1$r1;20s\\x1b\\\\";
            assert_eq!(margins(t, true), edges, "{}", show(keep));
            support::feed(t, &[ON, b"\rABCDEFGHIJKLMNOPQRST", OFF].concat());
            assert_eq!(status(t), "ABCDEFGHIJKLMNOPQRST", "{}", show(keep));
        });
    }
}

/// **DECLRMM set on either display makes the line's row single-width**,
/// as it makes the main display's: "In the set state ... no line
/// attributes other than Single Wide may be used" (`[[source:
/// dec-std-070]]` p. 5-29), and the mode is the terminal's. The line kept
/// a double-width row under DECLRMM, so a line start taken in one read
/// from its margins and in the next from its half put the cursor in two
/// places (`declrmm_on_a_double_status_line_agrees_atom_by_atom`).
#[test]
fn declrmm_set_makes_the_lines_row_single_width() {
    for set in [[ON, b"\x1b[?69h", OFF].concat(), b"\x1b[?69h".to_vec()] {
        support::with_term(&WS, |t| {
            support::feed(t, &[HOST, ON, b"\x1b#6ab", OFF].concat());
            let row = |t: &Term<'_>| t.status_view().map(|v| v.rendition(Row::ZERO));
            assert_eq!(row(t), Some(LineRendition::DoubleWidth));
            support::feed(t, &set);
            assert_eq!(row(t), Some(LineRendition::Single), "{}", show(&set));
            assert_eq!(status(t), "ab", "the cells stay, as DECSWL keeps them");
            // A read that begins on the line takes its fence again, from
            // the row, and DECSLRM in the read before took it from the
            // margin.
            support::feed(t, &[ON, b"\x1b[5;16s", OFF].concat());
            support::feed(t, &[ON, b"\x1b[?34h\x1b[E"].concat());
            assert_eq!(
                t.status_line().and_then(|l| l.cursor),
                Some(Col::at_index(15))
            );
        });
    }
}

/// **DECSTR keeps a wrap pending at the line's narrower right margin**, as
/// it keeps the main display's: the margins it puts back assign no
/// position (`[[source: dec-std-070]]` 070-4 p. 32, note c), so the line's
/// cursor still reports the margin's column and the next character wraps
/// first. The line dropped the wait, reported the column after the margin
/// and wrote there.
#[test]
fn decstr_keeps_a_wrap_pending_at_the_lines_margin() {
    support::with_term(&WS, |t| {
        support::feed(
            t,
            &[HOST, ON, b"\x1b[?69h\x1b[1;10sabcdefghij", OFF].concat(),
        );
        support::feed(t, b"\x1b[!p");
        t.clear_reply();
        support::feed(t, &[ON, b"\x1b[6nX", OFF].concat());
        assert_eq!(show(t.reply()), "\\x1b[1;10R");
        assert_eq!(status(t), "X", "the wrap scrolled the line, erasing it");
    });
}

/// **A resize keeps a wrap pending on the line**, as it keeps the main
/// display's: on the column it waited on, a right margin's or the last
/// column's, clamped to the line's new end, where the line put the
/// cursor one past it, or on its last column with no wrap pending, and
/// the next character went there.
#[test]
fn a_resize_keeps_a_wrap_pending_on_the_line() {
    let narrow = WinSize {
        ws_row: 6,
        ws_col: 15,
    };
    let cases: [(&[u8], &str); 2] = [
        (b"\x1b[?69h\x1b[1;10sabcdefghij", "\\x1b[1;10R"),
        (b"abcdefghijklmnopqrst", "\\x1b[1;15R"),
    ];
    for (fill, cpr) in cases {
        support::with_term(&WS, |t| {
            support::feed(t, &[HOST, ON, fill, OFF].concat());
            assert_eq!(t.resize(&narrow), Resized::Ok);
            t.clear_reply();
            support::feed(t, &[ON, b"\x1b[6nX", OFF].concat());
            assert_eq!(show(t.reply()), cpr, "{}", show(fill));
            assert_eq!(status(t), "X", "the wrap scrolled the line: {}", show(fill));
        });
    }
}

/// **DECLRMM set keeps a wrap pending at the half of the line's
/// double-width row**, as the row goes single-width:
/// `set_left_right_margins_mode()` (`[[source: dec-std-070]]` p. 5-30)
/// sets each line's rendition and leaves the Last Column Flag alone, so
/// the cursor still reports the half's last column and the next character
/// wraps first. Untagged, the sentinel one past the half reads as an
/// ordinary column of the single-width row, CPR naming the column after
/// the half and the next character going there.
#[test]
fn declrmm_set_keeps_a_wrap_pending_at_the_lines_half() {
    for set in [[ON, b"\x1b[?69h", OFF].concat(), b"\x1b[?69h".to_vec()] {
        support::with_term(&WS, |t| {
            support::feed(t, &[HOST, ON, b"\x1b#6abcdefghij", OFF].concat());
            t.clear_reply();
            support::feed(t, &[ON, b"\x1b[6n", OFF].concat());
            assert_eq!(show(t.reply()), "\\x1b[1;10R", "the wait, before");
            support::feed(t, &set);
            t.clear_reply();
            support::feed(t, &[ON, b"\x1b[6nX", OFF].concat());
            assert_eq!(show(t.reply()), "\\x1b[1;10R", "{}", show(&set));
            assert_eq!(status(t), "X", "the wrap scrolled the line: {}", show(&set));
        });
    }
}

/// **And at the half of the main display's double-width line**, by the
/// same rule, the mode set on either display: the cursor reported the
/// column after the half and the next character went there, the wrap
/// lost.
#[test]
fn declrmm_set_keeps_a_wrap_pending_at_a_double_lines_half() {
    for set in [[ON, b"\x1b[?69h", OFF].concat(), b"\x1b[?69h".to_vec()] {
        support::with_term(&WS, |t| {
            support::feed(t, &[HOST, b"\x1b#6abcdefghij"].concat());
            t.clear_reply();
            support::feed(t, b"\x1b[6n");
            assert_eq!(show(t.reply()), "\\x1b[1;10R", "the wait, before");
            support::feed(t, &set);
            t.clear_reply();
            support::feed(t, b"\x1b[6nX");
            assert_eq!(show(t.reply()), "\\x1b[1;10R", "{}", show(&set));
            assert_eq!(main_row(t, Row::ZERO), "abcdefghij", "{}", show(&set));
            assert_eq!(main_row(t, Row::from_index(1)), "X", "{}", show(&set));
        });
    }
}

/// **The line's cursor is drawn where CPR reports it** while a wrap waits
/// at its double-width row's half: the column the drawn cursor is read
/// against is the half's last, as the report's is. It was read against
/// the right margin, and named the column after the one CPR reports.
#[test]
fn the_lines_cursor_is_drawn_where_cpr_reports_it_on_a_double_row() {
    support::with_term(&WS, |t| {
        support::feed(t, &[HOST, ON, b"\x1b#6abcdefghij"].concat());
        let cursor = t.status_line().and_then(|l| l.cursor);
        assert_eq!(cursor, Some(Col::at_index(9)));
        t.clear_reply();
        support::feed(t, b"\x1b[6n");
        assert_eq!(show(t.reply()), "\\x1b[1;10R");
    });
}

/// **DECSTR resets the line's own state where it waits**: the rendition
/// and the character sets the line was left in are the soft reset's on its
/// next write, as the main display's are, and its text stays.
#[test]
fn decstr_resets_the_lines_own_state() {
    support::with_term(&WS, |t| {
        support::feed(
            t,
            &[HOST, ON, b"x\x1b[1m\x1b(0", OFF, b"\x1b[!p", ON, b"q"].concat(),
        );
        let cells = t.status_line().expect("shown").cells;
        assert_eq!(text(cells), "xq");
        assert!(!t.style_of(cells[1]).attrs().has(Attrs::BOLD));
    });
}

/// **The line's own direction, as a window of one row** (`Term::status_view`,
/// what a renderer lays the line out from): SCP sent while DECSASD selects
/// the line sets its path and leaves the main display's lines as they were,
/// the line holding "its own ANSI text state and settings"
/// (`[[source: dec-std-070]]` 070-14 § 14.2).
#[test]
fn the_line_keeps_the_direction_set_on_it() {
    support::with_term(&WS, |t| {
        assert!(t.status_view().is_none(), "no line, no view");
        support::feed(t, b"\x1b[2$~");
        let v = t.status_view().expect("the host-writable line's view");
        assert_eq!((v.rows().index(), v.cols().index()), (1, 20));
        assert_eq!(v.path(Row::ZERO), CharacterPath::Default, "power-on");
        support::feed(t, b"\x1b[1$}\x1b[2 kabc\x1b[0$}");
        let v = t.status_view().expect("shown");
        assert_eq!(v.path(Row::ZERO), CharacterPath::RightToLeft);
        assert_eq!(text(v.row(Row::ZERO)), "abc");
        assert_eq!(
            t.view(Rows::ZERO).path(Row::ZERO),
            CharacterPath::Default,
            "the main display's first line keeps its own"
        );
    });
}
