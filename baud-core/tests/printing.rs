//! Printing: Media Copy, autoprint, printer controller mode, DECPFF and
//! DECPEX (`[[source: dec-std-070]]` 070-7; `[[source: vt420-rm]]` chapter
//! 11), the owner's call of 2026-10-20, in through the print panel. The core
//! writes 070-7's print stream into jobs and the embedder prints them.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_col: 10,
    ws_row: 4,
};

fn owned() -> Owned {
    Owned::new(&WS).with_printer(PrintBudget::of_bytes(4096))
}

fn ready(s: &mut Owned) -> Term<'_> {
    let mut t = Term::new(s.storage(), &WS);
    t.set_printer(PrinterPort::Ready);
    t
}

/// Every finished job, as text, and the jobs taken.
fn jobs(t: &mut Term<'_>) -> Vec<String> {
    let out = t
        .print_jobs()
        .map(|j| String::from_utf8_lossy(j.bytes).into_owned())
        .collect();
    t.clear_print_jobs();
    out
}

fn row(t: &Term<'_>, y: usize) -> String {
    let row: String = t
        .row(Row::from_index(y))
        .expect("a row")
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect();
    row.trim_end().to_string()
}

/// `n` empty lines of a print, single width.
fn empty(n: usize) -> String {
    "\x1b#5\r\n".repeat(n)
}

/// **MC is a print function, not an unsupported sequence.** Red before
/// printing was built: `CSI 0 i` was counted in `unsupported`.
#[test]
fn mc_is_a_print_function_not_an_unsupported_sequence() {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    support::feed(
        &mut t,
        b"\x1b[i\x1b[0i\x1b[?1i\x1b[?5i\x1b[?4i\x1b[?10i\x1b[?11i",
    );
    assert_eq!(t.unsupported, 0);
}

/// **With no printer every print function is ignored and counted**, 070-7
/// § 7.3.2: "all print commands which require the printer port are
/// ignored". Controller mode is never entered, so the screen keeps drawing.
#[test]
fn with_no_printer_every_print_function_is_ignored_and_counted() {
    let mut s = owned();
    let mut t = Term::new(s.storage(), &WS);
    assert_eq!(t.printer(), PrinterPort::Absent);
    support::feed(
        &mut t,
        b"\x1b[0i\x1b[?1i\x1b[?5i\x1b[?10i\x1b[?11i\x1b[5iab",
    );
    assert_eq!(t.print_refusals().no_printer, 6);
    assert_eq!(t.print_run(), PrintRun::Idle);
    assert_eq!(row(&t, 0), "ab", "no controller run: the text is drawn");
    assert_eq!(t.print_jobs().count(), 0);
}

/// **DSR `?15` answers what a print would do**: `?10n` ready with a
/// printer, `?13n` "not connected" without (070-7 p. 7-32).
#[test]
fn dsr_15_answers_ready_with_a_printer_and_not_connected_without() {
    let mut s = owned();
    let mut t = Term::new(s.storage(), &WS);
    support::feed(&mut t, b"\x1b[?15n");
    assert_eq!(t.reply(), b"\x1b[?13n");
    t.clear_reply();
    t.set_printer(PrinterPort::Ready);
    support::feed(&mut t, b"\x1b[?15n");
    assert_eq!(t.reply(), b"\x1b[?10n");
}

/// **Under `PrinterHold::On` a DSR `?15` stops the read and waits for the
/// embedder**: no answer until `Term::set_printer`, which answers from the
/// printer it is given, and the rest of the read -- here a second `?15`
/// and text -- is parsed after the answer, so the replies stay in order.
/// Off, the answer is at once, from what the embedder last said.
#[test]
fn a_held_dsr_15_is_answered_from_the_printer_the_embedder_finds() {
    let mut s = owned();
    let mut t = Term::new(s.storage(), &WS);
    t.set_printer_hold(PrinterHold::On);
    let bytes = b"\x1b[?15nab\x1b[?15n";
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    assert_eq!(t.reply(), b"", "nothing until the embedder looks");
    assert_eq!(t.printer_asked(), PrinterAsked::Owed);
    let rest = t.take_held().expect("the read stopped at the question");
    assert_eq!((rest.at.index(), rest.len.index()), (6, 8));
    assert_eq!(row(&t, 0), "", "nothing after it parsed");
    t.set_printer(PrinterPort::Ready);
    assert_eq!(t.printer_asked(), PrinterAsked::No);
    assert_eq!(t.reply(), b"\x1b[?10n");
    t.clear_reply();
    t.feed(rest.at, rest.len);
    assert_eq!(t.reply(), b"", "the second waits too");
    assert_eq!(t.printer_asked(), PrinterAsked::Owed, "stopped again");
    assert_eq!(t.take_held().map_or(0, |r| r.len.index()), 0);
    assert_eq!(row(&t, 0), "ab");
    t.set_printer(PrinterPort::Absent);
    assert_eq!(t.reply(), b"\x1b[?11n", "lost since it was seen");
    t.clear_reply();
    // Answered, a printer change answers nothing more.
    t.set_printer(PrinterPort::Ready);
    assert_eq!(t.reply(), b"");
    t.set_printer_hold(PrinterHold::Off);
    support::feed(&mut t, b"\x1b[?15n");
    assert_eq!(t.reply(), b"\x1b[?10n");
    assert!(t.take_held().is_none());
}

/// **Print page is the scrolling region while DECPEX is reset**, its
/// default, and the whole page when set (070-7 p. 7-38); `CSI i` is
/// `CSI 0 i`.
#[test]
fn print_page_prints_the_scrolling_region_unless_decpex_is_set() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"one\r\ntwo\r\nthree\r\nfour\x1b[2;3r\x1b[i");
    assert_eq!(jobs(&mut t), ["\x1b[0m\x1b#5two\r\n\x1b#5three\r\n\x1b[0m"]);
    support::feed(&mut t, b"\x1b[?19h\x1b[0i");
    assert_eq!(
        jobs(&mut t),
        ["\x1b[0m\x1b#5one\r\n\x1b#5two\r\n\x1b#5three\r\n\x1b#5four\r\n\x1b[0m"]
    );
}

/// **Print screen ignores DECPEX; DECPFF ends a print with a form feed**,
/// "at the conclusion of each Print Page or Print Screen operation"
/// (070-7 p. 7-37), before the closing `ESC [ 0 m`; print all pages is the
/// one page baud has.
#[test]
fn print_screen_ignores_decpex_and_decpff_adds_a_form_feed() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"a\x1b[2;3r\x1b[?18h\x1b[?10i");
    assert_eq!(
        jobs(&mut t),
        [format!("\x1b[0m\x1b#5a\r\n{}\x0c\x1b[0m", empty(3))]
    );
    support::feed(&mut t, b"\x1b[?11i");
    assert_eq!(jobs(&mut t), [format!("\x1b[0m{}\x0c\x1b[0m", empty(2))]);
}

/// **Print line is the cursor's line**, within DECSLRM's margins while
/// DECPEX is reset (070-7, PRINT LINE), and DECPFF does not touch it
/// ("DECPFF does not affect the print cursor line function",
/// `[[source: vt420-rm]]` p. 211). The cursor does not move.
#[test]
fn print_line_prints_the_cursor_line_and_decpex_picks_the_margins() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(
        &mut t,
        b"\r\nabcdefgh\x1b[?18h\x1b[?69h\x1b[3;5s\x1b[2;8H\x1b[?1i",
    );
    assert_eq!(jobs(&mut t), ["\x1b[0m\x1b#5cde\r\n\x1b[0m"]);
    assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(7)));
    support::feed(&mut t, b"\x1b[?19h\x1b[?1i");
    assert_eq!(jobs(&mut t), ["\x1b[0m\x1b#5abcdefgh\r\n\x1b[0m"]);
}

/// **070-7 § 7.4.2's attributes, § 7.4.3's trailing spaces and invisible
/// text as spaces**: `ESC [ 0 ; Ps m` at each change, bold 1, underline 4
/// (any style), blink 5 (either rate), reverse 7, nothing else; a space in
/// reverse is printable and kept, a plain one at the end is not; a wide
/// character is one character, its marks after it; a double-width line is
/// `ESC # 6` and its half.
#[test]
fn the_stream_carries_renditions_line_sizes_and_hidden_text_as_spaces() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(
        &mut t,
        "\x1b[1mB\x1b[0;4:3mU\x1b[0;6;7mR\x1b[0;8mH\x1b[0;3mi\x1b[0m\u{4e2d}e\u{301}\x1b[7m \x1b[0m \r\n\x1b#6wide"
            .as_bytes(),
    );
    support::feed(&mut t, b"\x1b[?19h\x1b[0i");
    assert_eq!(
        jobs(&mut t),
        [format!(
            "\x1b[0m\x1b#5\x1b[0;1mB\x1b[0;4mU\x1b[0;5;7mR\x1b[0m i\u{4e2d}e\u{301}\x1b[0;7m \r\n\
             \x1b#6\x1b[0mwide\r\n{}\x1b[0m",
            empty(2)
        )]
    );
}

/// **Autoprint prints each line the cursor leaves by `LF`, `VT`, `FF` or an
/// autowrap**, ended by `CR` and that character ("or LF if Auto Wrap",
/// 070-7 § 7.3.7), each with the whole initialisation and closing; `CR LF`
/// in one step prints as `LF` does; `IND` and `NEL` print nothing. The run
/// is one job, from `CSI ? 5 i` to `CSI ? 4 i`.
#[test]
fn autoprint_prints_each_line_the_cursor_leaves() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b[?5ia\nb\x0bc\x0cd\r\n0123456789W\x1bDx\x1bEy");
    assert_eq!(t.print_run(), PrintRun::AutoPrint);
    assert_eq!(t.print_jobs().count(), 0, "the run is still collecting");
    support::feed(&mut t, b"\x1b[?4i");
    let line = |s: &str, end: &str| format!("\x1b[0m\x1b#5{s}\r{end}\x1b[0m");
    assert_eq!(
        jobs(&mut t),
        [[
            line("a", "\n"),
            line(" b", "\x0b"),
            line("  c", "\x0c"),
            line("   d", "\n"),
            line("0123456789", "\n"),
        ]
        .concat()]
    );
    assert_eq!(t.print_run(), PrintRun::Idle);
}

/// A `VT` that reaches a line tabulation stop leaves its line too, and
/// prints it ended by `VT`.
#[test]
fn a_vt_to_a_line_stop_prints_the_line_it_leaves() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b[3H\x1bJ\x1b[H\x1b[?5ivt\x0b\x1b[?4i");
    assert_eq!(jobs(&mut t), ["\x1b[0m\x1b#5vt\r\x0b\x1b[0m"]);
    assert_eq!(t.cursor().0, Row::from_index(2));
}

/// **An autoprint run that printed no line is no job**: no panel for an
/// empty print.
#[test]
fn an_autoprint_run_that_printed_nothing_is_no_job() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b[?5iabc\x1b[?4i");
    assert_eq!(t.print_jobs().count(), 0);
}

/// **A print in the middle of an autoprint run ends its job there**, the
/// print is a job of its own and the run goes on into another (Jev's lean
/// OWN_JOB).
#[test]
fn a_print_inside_an_autoprint_run_ends_its_job_and_starts_another() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b[?5ia\n\x1b[?1ib\n\x1b[?4i");
    assert_eq!(
        jobs(&mut t),
        [
            "\x1b[0m\x1b#5a\r\n\x1b[0m",
            "\x1b[0m\x1b#5\r\n\x1b[0m",
            "\x1b[0m\x1b#5 b\r\n\x1b[0m",
        ]
    );
}

/// **Printer controller mode sends the host's bytes to the printer and not
/// the screen** (070-7 § 7.3.6), but NUL, XON and XOFF and the sequences
/// that turn it on and off; `CSI 4 i` ends the run and autoprint with it;
/// any other sequence, `CSI 0 i` and `CSI ? 4 i` among them, is the
/// printer's. The screen is drawn again after.
#[test]
fn printer_controller_mode_hides_output_and_collects_it() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(
        &mut t,
        b"\x1b[?5i\x1b[5ihi\x00\x11\x13 \x1b[5i\x1b[0i\x1b[?4i\x1b[1mthere\r\n",
    );
    assert_eq!(t.print_run(), PrintRun::Controller);
    assert_eq!(row(&t, 0), "", "nothing reached the screen");
    support::feed(&mut t, b"\x1b[4iok");
    assert_eq!(t.print_run(), PrintRun::Idle);
    assert_eq!(row(&t, 0), "ok");
    assert_eq!(jobs(&mut t), ["hi \x1b[0i\x1b[?4i\x1b[1mthere\r\n"]);
    support::feed(&mut t, b"\n");
    assert_eq!(t.print_jobs().count(), 0, "autoprint ended with the run");
}

/// **Where the reads fall changes nothing**: a run fed a byte at a time,
/// its `ESC [ 4 i` cut between reads, collects and ends as it does whole,
/// and a sequence held back at a read's end that turns out not to be
/// `CSI 4 i` reaches the printer whole.
#[test]
fn a_controller_run_is_the_same_at_every_split() {
    let prog: &[u8] = b"x\x1b[5iab\x1b[4\x1b[40i\x1b[44ic\x1b[4iyz";
    let whole = {
        let mut s = owned();
        let mut t = ready(&mut s);
        support::feed(&mut t, prog);
        (jobs(&mut t), row(&t, 0))
    };
    assert_eq!(whole.0, ["ab\x1b[4\x1b[40i\x1b[44ic"]);
    assert_eq!(whole.1, "xyz");
    for cut in 1..prog.len() {
        let mut s = owned();
        let mut t = ready(&mut s);
        support::feed(&mut t, &prog[..cut]);
        support::feed(&mut t, &prog[cut..]);
        assert_eq!((jobs(&mut t), row(&t, 0)), whole, "cut at {cut}");
    }
}

/// **Under ISO 8859-1 a run's bytes reach the job as UTF-8**, so a job is
/// always UTF-8, and the 8-bit `CSI 4 i` ends it; under UTF-8 `9B` is a
/// continuation byte and goes to the printer.
#[test]
fn latin1_controller_bytes_become_utf8_and_the_8_bit_csi_ends_the_run() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b%@\x1b[5i\xe9t\xe9\x9b4ix");
    assert_eq!(jobs(&mut t), ["\u{e9}t\u{e9}"]);
    assert_eq!(row(&t, 0), "x");
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b[5ia\x9b4i\x1b[4i");
    assert_eq!(
        jobs(&mut t),
        [String::from_utf8_lossy(b"a\x9b4i").into_owned()]
    );
}

/// **VT52's print commands are MC's** (`[[source: vt420-rm]]`'s VT52
/// table): `ESC ]` prints the screen (070-7: print page "was previously
/// called Print Screen"), `ESC V` the cursor's line, `ESC ^` and `ESC _`
/// autoprint, `ESC W` and `ESC X` a controller run, which `ESC X` ends.
#[test]
fn vt52_print_commands_print() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b[?2lab\x1bV\x1b^\n\x1b_\x1bWcd\x1b[4i\x1bXe");
    assert_eq!(
        jobs(&mut t),
        [
            "\x1b[0m\x1b#5ab\r\n\x1b[0m",
            "\x1b[0m\x1b#5ab\r\n\x1b[0m",
            "cd\x1b[4i",
        ]
    );
    assert_eq!(row(&t, 1), "  e", "drawn again after ESC X, still VT52");
    assert_eq!(t.terminal_mode(), TerminalMode::Vt52);
    support::feed(&mut t, b"\x1b]");
    assert_eq!(jobs(&mut t).len(), 1);
}

/// **DECPFF and DECPEX are modes**: DECRQM answers them as xterm's
/// `MdBool` does, XTSAVE and XTRESTORE carry them, `RIS` returns them to
/// their defaults and `DECSTR` keeps them; they are set with no printer.
#[test]
fn decpff_and_decpex_answer_decrqm_and_reset_with_ris() {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    let ask = |t: &mut Term<'_>| {
        t.clear_reply();
        support::feed(t, b"\x1b[?18$p\x1b[?19$p");
        String::from_utf8_lossy(t.reply()).into_owned()
    };
    assert_eq!(ask(&mut t), "\x1b[?18;2$y\x1b[?19;2$y");
    support::feed(&mut t, b"\x1b[?18h\x1b[?19h\x1b[!p");
    assert_eq!(ask(&mut t), "\x1b[?18;1$y\x1b[?19;1$y", "DECSTR keeps them");
    support::feed(&mut t, b"\x1b[?18;19s\x1b[?18l\x1b[?19l\x1b[?18;19r");
    assert_eq!(ask(&mut t), "\x1b[?18;1$y\x1b[?19;1$y", "XTRESTORE");
    support::feed(&mut t, b"\x1bc");
    assert_eq!(ask(&mut t), "\x1b[?18;2$y\x1b[?19;2$y", "RIS");
}

/// **`RIS` ends an autoprint run, its job kept.**
#[test]
fn ris_ends_an_autoprint_run_and_keeps_its_job() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b[?5ia\n\x1bcb\n");
    assert_eq!(t.print_run(), PrintRun::Idle);
    assert_eq!(jobs(&mut t), ["\x1b[0m\x1b#5a\r\n\x1b[0m"]);
}

/// **The kinds refused because answering is unsafe are counted apart**:
/// `CSI 2 i` would send the screen to the host, `CSI ? 2 i` a hard copy's
/// image of it, `CSI ? 8 i` and `CSI ? 9 i` are a printer-to-host session,
/// `CSI 10 i` and `CSI 11 i` xterm's file dumps. A `Ps` no text gives is
/// unsupported.
#[test]
fn the_refused_kinds_are_counted_apart_from_the_unknown() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b[2i\x1b[?2i\x1b[?8i\x1b[?9i\x1b[10i\x1b[11i");
    assert_eq!(t.print_refusals().unsafe_kinds, 6);
    assert_eq!(t.reply(), b"", "nothing is sent to the host");
    assert_eq!(t.unsupported, 0);
    support::feed(&mut t, b"\x1b[3i\x1b[?18i\x1b[?3i");
    assert_eq!(t.unsupported, 3);
    assert_eq!(t.print_jobs().count(), 0);
}

/// **The area is the bound**: bytes past it are dropped from the job,
/// counted, and the job marked cut; the run goes on to its `CSI 4 i`.
#[test]
fn the_area_bound_cuts_a_job_and_counts_the_bytes() {
    let mut s = Owned::new(&WS).with_printer(PrintBudget::of_bytes(8));
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b[5i0123456789ab\x1b[4ix");
    let got: Vec<_> = t.print_jobs().map(|j| (j.bytes.to_vec(), j.cut)).collect();
    assert_eq!(got, [(b"01234567".to_vec(), PrintCut::Cut)]);
    assert_eq!(t.print_refusals().dropped, 4);
    assert_eq!(row(&t, 0), "x");
}

/// **A read that finishes more than `PRINT_QUEUE` jobs joins the rest to
/// the last, after a form feed**, so a slow embedder loses nothing.
#[test]
fn a_full_queue_joins_the_next_jobs_to_the_last_after_a_form_feed() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"z");
    for _ in 0..PRINT_QUEUE + 2 {
        support::feed(&mut t, b"\x1b[?1i");
    }
    let got = jobs(&mut t);
    assert_eq!(got.len(), PRINT_QUEUE);
    let one = "\x1b[0m\x1b#5z\r\n\x1b[0m";
    assert_eq!(got[0], one);
    assert_eq!(got[PRINT_QUEUE - 1], format!("{one}\x0c{one}\x0c{one}"));
    assert_eq!(t.print_refusals().merged, 2);
}

/// **A job being collected outlives the embedder taking the finished
/// ones**: it moves to the front of the area and finishes whole.
#[test]
fn a_job_being_collected_survives_the_finished_ones_being_taken() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"z\x1b[?1i\x1b[5iabc");
    assert_eq!(jobs(&mut t).len(), 1);
    support::feed(&mut t, b"def\x1b[4i");
    assert_eq!(jobs(&mut t), ["abcdef"]);
}

/// **A printer lost after it was there is "not ready", not "not
/// connected"**: 070-7 § 7.3.2 gives a port that "has seen DSR since
/// power-up, but it is no longer asserted" the answer `?11n` and has
/// "print operations ... queued". The run keeps collecting, so the
/// program's printer data never reaches the screen, and a new print is a
/// job, not a refusal.
#[test]
fn losing_the_printer_leaves_a_run_collecting() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b[5iab");
    t.set_printer(PrinterPort::Absent);
    assert_eq!(t.print_run(), PrintRun::Controller);
    support::feed(&mut t, b"\x1b[?15ncd\x1b[4ie\x1b[?15n\x1b[0i");
    assert_eq!(row(&t, 0), "e", "only what followed the run is drawn");
    assert_eq!(t.reply(), b"\x1b[?11n", "the run's ?15 was the printer's");
    let got = jobs(&mut t);
    assert_eq!(got.len(), 2, "the run and the queued print screen");
    assert_eq!(got[0], "ab\x1b[?15ncd");
    assert_eq!(t.print_refusals().no_printer, 0);
}

/// **RIS forgets a printer that is gone**: "not available" is a port that
/// "has not been on since the last power-up or reset (RIS)" (070-7 p.
/// 7-32), so after RIS a lost printer answers `?13n` and its prints are
/// ignored and counted, while one still there answers `?10n`.
#[test]
fn ris_forgets_a_lost_printer_and_keeps_a_present_one() {
    let mut s = owned();
    let mut t = ready(&mut s);
    t.set_printer(PrinterPort::Absent);
    support::feed(&mut t, b"\x1bc\x1b[?15n\x1b[0i");
    assert_eq!(t.reply(), b"\x1b[?13n");
    assert_eq!(t.print_refusals().no_printer, 1);
    assert_eq!(t.print_jobs().count(), 0);
    t.clear_reply();
    t.set_printer(PrinterPort::Ready);
    support::feed(&mut t, b"\x1bc\x1b[?15n");
    assert_eq!(t.reply(), b"\x1b[?10n");
    t.clear_reply();
    t.set_printer(PrinterPort::Absent);
    support::feed(&mut t, b"\x1b[?15n");
    assert_eq!(t.reply(), b"\x1b[?11n", "seen since the RIS");
}

/// **A text run that crosses rows prints each row it leaves**, the bulk
/// writer's wrap included, and a wide character that wraps prints the line
/// it left.
#[test]
fn autoprint_prints_each_row_a_long_run_and_a_wide_wrap_leave() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, b"\x1b[?5i0123456789abcdefghijABCD\r\n");
    support::feed(&mut t, "123456789\u{4e2d}x\x1b[?4i".as_bytes());
    let line = |s: &str| format!("\x1b[0m\x1b#5{s}\r\n\x1b[0m");
    assert_eq!(
        jobs(&mut t),
        [[
            line("0123456789"),
            line("abcdefghij"),
            line("ABCD"),
            line("123456789"),
        ]
        .concat()]
    );
}

/// `abcdef` on row 0 with `cd` guarded: SPA at `c`, EPA on `d` (the
/// position "before the receipt of EPA is the last", `[[source: ecma48]]`
/// 8.3.46), then `ef`.
const GUARDED: &[u8] = b"ab\x1bVcd\x1b[D\x1bW\x1b[Cef";

/// **Under GATM's GUARD, its reset state, a print leaves out a guarded
/// position's contents, printing it as a space**: "Only the contents of
/// unguarded areas in an eligible area are transmitted or transferred"
/// (`[[source: ecma48]]` 7.2.7), excluded "from transfer to an auxiliary
/// input/output device" (6.5.2.2), which MC initiates (6.6); as a space,
/// as an invisible character prints, so `ef` keeps its columns (Jev's
/// lean, AS_SPACES). `CSI 1 h`, ALL, prints them; `CSI 1 l` excludes
/// them again.
#[test]
fn under_gatm_guard_a_print_leaves_out_the_guarded_contents() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, GUARDED);
    assert_eq!(row(&t, 0), "abcdef", "the screen keeps them");
    support::feed(&mut t, b"\x1b[i");
    assert_eq!(
        jobs(&mut t),
        [format!("\x1b[0m\x1b#5ab  ef\r\n{}\x1b[0m", empty(3))]
    );
    support::feed(&mut t, b"\x1b[1h\x1b[i");
    assert_eq!(
        jobs(&mut t),
        [format!("\x1b[0m\x1b#5abcdef\r\n{}\x1b[0m", empty(3))]
    );
    support::feed(&mut t, b"\x1b[1l\x1b[i");
    assert_eq!(
        jobs(&mut t),
        [format!("\x1b[0m\x1b#5ab  ef\r\n{}\x1b[0m", empty(3))]
    );
    assert_eq!(t.unsupported_param, 0, "GATM is no longer counted");
}

/// **Every print of screen contents is a transfer GATM governs** (Jev,
/// EVERY_SCREEN_PRINT, 0.90/0.98): print screen, all pages, print line and
/// autoprint, as well as print page. A guarded position at a line's end is
/// left off as a trailing space is (070-7 § 7.4.3).
#[test]
fn every_screen_print_leaves_out_the_guarded_contents() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(&mut t, GUARDED);
    support::feed(&mut t, b"\x1b[?10i\x1b[?11i\x1b[?1i");
    assert_eq!(
        jobs(&mut t),
        [
            format!("\x1b[0m\x1b#5ab  ef\r\n{}\x1b[0m", empty(3)),
            format!("\x1b[0m\x1b#5ab  ef\r\n{}\x1b[0m", empty(3)),
            "\x1b[0m\x1b#5ab  ef\r\n\x1b[0m".into(),
        ]
    );
    support::feed(&mut t, b"\x1b[2;1Hxy\x1bVzw\x1b[D\x1bW\x1b[?5i\n\x1b[?4i");
    assert_eq!(jobs(&mut t), ["\x1b[0m\x1b#5xy\r\n\x1b[0m"]);
}

/// **An area only partly inside a print is left out where it is inside**
/// (Jev's lean, EXCLUDE_PART; 7.3.1 g leaves it to the implementation):
/// guarded from row 0's `e` through row 1's `c`, print line on row 1.
#[test]
fn a_guarded_area_partly_inside_a_print_is_left_out_there() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(
        &mut t,
        b"abcdefgh\x1b[2;1Habcdef\x1b[1;5H\x1bV\x1b[2;3H\x1bW\x1b[?1i",
    );
    assert_eq!(jobs(&mut t), ["\x1b[0m\x1b#5   def\r\n\x1b[0m"]);
}

/// **DECRQM answers GATM**, reset at power-on; `RIS` resets it and `DECSTR`
/// keeps it, as ERM (Jev's lean, KEEP, 0.90/0.88).
#[test]
fn decrqm_answers_gatm_and_ris_resets_it_and_decstr_keeps_it() {
    let mut s = owned();
    let mut t = ready(&mut s);
    support::feed(
        &mut t,
        b"\x1b[1$p\x1b[1h\x1b[1$p\x1b[!p\x1b[1$p\x1bc\x1b[1$p",
    );
    assert_eq!(t.reply(), b"\x1b[1;2$y\x1b[1;1$y\x1b[1;1$y\x1b[1;2$y");
}
