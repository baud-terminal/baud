//! **A prompt pinned to the window's last row** (`[screen]
//! prompt_at_bottom`), driven by recorded shells.
//!
//! `tests/prompt/` holds real bash 5.3 traffic captured through a pty with
//! the `capture-tui` and `capture-resize` examples, the shell's
//! `OSC 133` marks set by the three lines in `tests/prompt/bashrc`:
//! `bash-session` types `clear; ls`, `echo one`, Control-L and `echo two`
//! in a 60x12 window, and `bash-grow` grows a 60x12 window to 60x20 and
//! then 80x20 at a prompt. `tests/resize/fresh-zoom` is zsh with a
//! five-line prompt through a zoom gesture.
//!
//! What the mode does: once the shell has drawn a prompt, at the mark
//! where its input begins (`B` or `I`), the prompt walks down over the
//! blank rows directly below it, so its last line lands on the last row
//! and output stays where it was written; a shrink at the prompt sends
//! the top rows into the history and a growth brings them back. Nothing
//! happens in the output zone, on the alternate screen, or under a
//! scrolling region.
mod support;
use baud_core::{Owned, PackedCell, PromptLine, Resized, SurfaceLine, Term};
use baud_domain::{BytePos, PromptAtBottom, ReadLen, Row, WinSize};

struct Fixture {
    raw: Vec<u8>,
    steps: Vec<(WinSize, usize)>,
}

fn load(name: &str) -> Fixture {
    let dir = format!("{}/../tests", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read(format!("{dir}/{name}.raw")).expect("the recording");
    let text = std::fs::read_to_string(format!("{dir}/{name}.steps")).expect("its steps");
    let steps: Vec<(WinSize, usize)> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (size, len) = l.split_once(' ').expect("COLSxROWS <len>");
            let (c, r) = size.split_once('x').expect("COLSxROWS");
            (
                WinSize {
                    ws_col: c.parse().expect("cols"),
                    ws_row: r.parse().expect("rows"),
                },
                len.parse().expect("length"),
            )
        })
        .collect();
    assert_eq!(
        steps.iter().map(|s| s.1).sum::<usize>(),
        raw.len(),
        "{name}"
    );
    Fixture { raw, steps }
}

fn row_text(t: &Term<'_>, y: usize) -> String {
    let cells: &[PackedCell] = t.row(Row::from_index(y)).expect("a row on the grid");
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// A row's words, one space apart: `ls` writes its names a tab apart.
fn words(t: &Term<'_>, y: usize) -> String {
    row_text(t, y)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn rows(t: &Term<'_>) -> usize {
    t.geometry().1.index()
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    if !bytes.is_empty() {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }
}

/// Where the `n`th input mark (`OSC 133 ; B`, counted from 1) of `raw`
/// ends: the bytes through it leave the shell at its `n`th prompt. bash
/// ends the mark with BEL, fish with ST.
fn through_input(raw: &[u8], n: usize) -> usize {
    const B: &[u8] = b"\x1b]133;B";
    raw.windows(B.len())
        .enumerate()
        .filter(|(_, w)| *w == B)
        .map(|(i, _)| match raw[i + B.len()..] {
            [0x07, ..] => i + B.len() + 1,
            [0x1b, b'\\', ..] => i + B.len() + 2,
            _ => panic!("an input mark at {i} that neither BEL nor ST ends"),
        })
        .nth(n - 1)
        .unwrap_or_else(|| panic!("the recording has no input mark {n}"))
}

/// `bash-session` up to its `n`th prompt, in one window, with the setting
/// as `pin` says.
fn session_at<R>(pin: PromptAtBottom, n: usize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    recording_at("prompt/bash-session", pin, n, f)
}

/// The recording `name` up to its `n`th prompt, in one window, with the
/// setting as `pin` says.
fn recording_at<R, F>(name: &str, pin: PromptAtBottom, n: usize, f: F) -> R
where
    F: FnOnce(&mut Term<'_>) -> R,
{
    let fx = load(name);
    let ws = WinSize { ..fx.steps[0].0 };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    t.set_prompt_at_bottom(pin);
    feed(&mut t, &fx.raw[..through_input(&fx.raw, n)]);
    f(&mut t)
}

/// Replays a stepped recording, resizing at each step, and hands back what
/// `each` says after every step.
fn replay(name: &str, pin: PromptAtBottom, mut each: impl FnMut(&Term<'_>, usize)) {
    let fx = load(name);
    let max = WinSize {
        ws_col: fx.steps.iter().map(|s| s.0.ws_col).max().expect("steps"),
        ws_row: fx.steps.iter().map(|s| s.0.ws_row).max().expect("steps"),
    };
    let owned: &'static mut Owned = Box::leak(Box::new(Owned::new(&max)));
    let mut t = Term::new(owned.storage(), &fx.steps[0].0);
    t.set_prompt_at_bottom(pin);
    let mut at = 0;
    for (i, (ws, len)) in fx.steps.iter().enumerate() {
        assert_eq!(t.resize(ws), Resized::Ok, "{name}: step {i}");
        feed(&mut t, &fx.raw[at..at + len]);
        at += len;
        each(&t, i);
    }
}

/// **The control: off, a prompt follows the output down**, as it always
/// has. With the setting on, the same bytes put the first prompt of a
/// fresh session, the prompt after `clear; ls` and the prompt Control-L
/// redraws on the last row, and leave `ls`'s output on the row it was
/// written to.
#[test]
fn a_prompt_lands_on_the_last_row_only_with_the_setting_on() {
    let last = 11;
    // Off: where the cursor was.
    session_at(PromptAtBottom::Off, 1, |t| {
        assert_eq!(t.cursor().0.index(), 0)
    });
    session_at(PromptAtBottom::Off, 2, |t| {
        assert_eq!(words(t, 0), "alpha beta gamma");
        assert_eq!(row_text(t, 1), "demo$");
    });
    session_at(PromptAtBottom::Off, 4, |t| {
        assert_eq!(row_text(t, 0), "demo$")
    });
    // On.
    session_at(PromptAtBottom::On, 1, |t| {
        assert_eq!(rows(t), last + 1);
        assert_eq!(t.cursor().0.index(), last, "a fresh session's prompt");
        assert_eq!(row_text(t, last), "demo$");
        assert!(t.scrollback().is_empty(), "walking down retires nothing");
    });
    session_at(PromptAtBottom::On, 2, |t| {
        assert_eq!(words(t, 0), "alpha beta gamma", "the output stays put");
        assert!((1..last).all(|y| row_text(t, y).is_empty()), "a gap");
        assert_eq!(row_text(t, last), "demo$", "the prompt after clear; ls");
    });
    session_at(PromptAtBottom::On, 3, |t| {
        assert_eq!(row_text(t, last - 2), "demo$ echo one");
        assert_eq!(row_text(t, last - 1), "one");
        assert_eq!(row_text(t, last), "demo$");
    });
    session_at(PromptAtBottom::On, 4, |t| {
        assert!((0..last).all(|y| row_text(t, y).is_empty()));
        assert_eq!(row_text(t, last), "demo$", "Control-L's redraw");
        assert_eq!(t.cursor().0.index(), last);
    });
}

/// **The prompt's records follow it**: the block start a resize and a
/// redraw count from, and the prompt log a jump reads, both name the row
/// the prompt landed on.
#[test]
fn the_block_and_the_prompt_log_name_the_row_the_prompt_landed_on() {
    for n in [1, 2, 4] {
        session_at(PromptAtBottom::On, n, |t| {
            assert_eq!(t.prompt_row(), Some(Row::from_index(11)), "prompt {n}");
            let newest = t.prompts().last().expect("a prompt");
            assert_eq!(
                newest.line,
                PromptLine::Screen(Row::from_index(11)),
                "prompt {n}"
            );
        });
    }
}

/// **A window that grows at a prompt keeps the prompt on its last row**:
/// bash redraws its prompt with its `A` after each `SIGWINCH`, and the
/// input mark that ends the redraw pins it. Off, the session's first
/// line stays at the top, the rule for a growth.
#[test]
fn a_growth_at_a_prompt_keeps_it_on_the_last_row() {
    let mut on = Vec::new();
    replay("prompt/bash-grow", PromptAtBottom::On, |t, _| {
        on.push((rows(t), t.cursor().0.index(), row_text(t, rows(t) - 1)));
    });
    for (r, cy, text) in &on {
        assert_eq!(*cy, r - 1, "{on:?}");
        assert_eq!(text, "demo$", "{on:?}");
    }
    replay("prompt/bash-grow", PromptAtBottom::Off, |t, _| {
        assert_eq!(row_text(t, 0), "demo$");
    });
}

/// **fish's own marks pin its prompts as bash's do.** fish 4.9.3 sends
/// `OSC 133` itself, each mark ended by ST where bash's end with BEL, its
/// prompt mark carrying `click_events=1` and its `C` the command line. On,
/// the first prompt, the prompt after `clear; ls`, both prompts Control-L
/// draws and the prompts after `echo one` and `echo two` land on the last
/// row, and `ls`'s output stays on the row it was written to. Off, each
/// prompt follows the output down.
#[test]
fn fish_s_prompts_land_on_the_last_row_only_with_the_setting_on() {
    let fish = "prompt/fish-session";
    let last = 11;
    // Off: where the cursor was.
    recording_at(fish, PromptAtBottom::Off, 1, |t| {
        assert_eq!(t.cursor().0.index(), 0)
    });
    recording_at(fish, PromptAtBottom::Off, 2, |t| {
        assert_eq!(words(t, 0), "alpha beta gamma");
        assert_eq!(row_text(t, 1), "demo>");
    });
    recording_at(fish, PromptAtBottom::Off, 4, |t| {
        assert_eq!(row_text(t, 0), "demo>")
    });
    // On.
    recording_at(fish, PromptAtBottom::On, 1, |t| {
        assert_eq!(rows(t), last + 1);
        assert_eq!(t.cursor().0.index(), last, "a fresh session's prompt");
        assert_eq!(row_text(t, last), "demo>");
        assert!(t.scrollback().is_empty(), "walking down retires nothing");
    });
    recording_at(fish, PromptAtBottom::On, 2, |t| {
        assert_eq!(words(t, 0), "alpha beta gamma", "the output stays put");
        assert!((1..last).all(|y| row_text(t, y).is_empty()), "a gap");
        assert_eq!(row_text(t, last), "demo>", "the prompt after clear; ls");
    });
    for (n, said) in [(3, "one"), (6, "two")] {
        recording_at(fish, PromptAtBottom::On, n, |t| {
            assert_eq!(row_text(t, last - 2), format!("demo> echo {said}"));
            assert_eq!(row_text(t, last - 1), said);
            assert_eq!(row_text(t, last), "demo>");
        });
    }
    // Control-L: fish clears and draws its prompt twice, each with its marks.
    for n in [4, 5] {
        recording_at(fish, PromptAtBottom::On, n, |t| {
            assert!((0..last).all(|y| row_text(t, y).is_empty()), "prompt {n}");
            assert_eq!(row_text(t, last), "demo>", "Control-L's redraw {n}");
            assert_eq!(t.cursor().0.index(), last);
            assert_eq!(t.prompt_row(), Some(Row::from_index(last)));
        });
    }
}

/// **A window that grows at a fish prompt keeps it on the last row**: fish
/// redraws its prompt with its `A` and its `B` after each `SIGWINCH`, through
/// 60x12, 60x20 and 80x20, and the input mark that ends the redraw pins it.
/// Off, the session's first line stays at the top.
#[test]
fn a_growth_at_a_fish_prompt_keeps_it_on_the_last_row() {
    let mut on = Vec::new();
    replay("prompt/fish-grow", PromptAtBottom::On, |t, _| {
        on.push((rows(t), t.cursor().0.index(), row_text(t, rows(t) - 1)));
    });
    assert_eq!(on.len(), 5, "{on:?}");
    assert_eq!(on.last().map(|s| s.0), Some(20), "{on:?}");
    for (r, cy, text) in &on {
        assert_eq!(*cy, r - 1, "{on:?}");
        assert_eq!(text, "demo>", "{on:?}");
    }
    replay("prompt/fish-grow", PromptAtBottom::Off, |t, _| {
        assert_eq!(row_text(t, 0), "demo>");
    });
}

/// **A recorded zsh through a zoom gesture**: with the setting on, at the
/// end of every step the shell is at a prompt, the cursor is on the last
/// row and one prompt is on the screen.
#[test]
fn a_zoom_gesture_leaves_one_prompt_on_the_last_row() {
    let mut steps = 0;
    replay("resize/fresh-zoom", PromptAtBottom::On, |t, i| {
        if t.zone() != baud_core::Zone::Prompt {
            return;
        }
        steps += 1;
        let r = rows(t);
        assert_eq!(t.cursor().0.index(), r - 1, "step {i} at {r} rows");
        let prompts = (0..r)
            .filter(|&y| row_text(t, y).starts_with("zsh"))
            .count();
        assert_eq!(prompts, 1, "step {i}: one prompt on the screen");
    });
    assert!(
        steps > 10,
        "the gesture reached the prompt at {steps} steps"
    );
}

/// **Only the marks move it**, once the prompt's input mark says it is
/// drawn: text with no marks is output, a prompt mark with no input mark
/// yet or an input mark with no prompt mark moves nothing, and marks on
/// the alternate screen or under a scrolling region leave the cursor where
/// it is.
#[test]
fn nothing_but_a_prompt_mark_on_the_primary_moves_the_cursor() {
    let ws = WinSize {
        ws_col: 40,
        ws_row: 10,
    };
    let case = |bytes: &[u8]| {
        support::with_term(&ws, |t| {
            t.set_prompt_at_bottom(PromptAtBottom::On);
            support::feed(t, bytes);
            t.cursor().0.index()
        })
    };
    assert_eq!(case(b"hello\r\n$ "), 1, "no marks");
    assert_eq!(
        case(b"\x1b]133;A\x07$ "),
        0,
        "a prompt not drawn to its input"
    );
    assert_eq!(
        case(b"$ \x1b]133;B\x07"),
        0,
        "an input mark with no prompt mark"
    );
    assert_eq!(
        case(b"\x1b[?1049h\x1b]133;A\x07$ \x1b]133;B\x07"),
        0,
        "the alternate screen"
    );
    assert_eq!(
        case(b"\x1b[1;5r\x1b]133;A\x07$ \x1b]133;B\x07"),
        0,
        "a scrolling region"
    );
    assert_eq!(
        case(b"\x1b[3;10r\x1b]133;A\x07$ \x1b]133;B\x07"),
        0,
        "a region to the last row"
    );
    assert_eq!(
        case(b"\x1b]133;A\x07$ \x1b]133;B\x07"),
        9,
        "the positive control"
    );
    assert_eq!(
        case(b"\x1bc\x1b]133;A\x07$ \x1b]133;B\x07"),
        9,
        "RIS leaves the setting on"
    );
}

/// **It walks over blank rows only**: a row holding text below the cursor
/// stops it, and the text stays where it was written.
#[test]
fn text_below_the_cursor_stops_the_walk() {
    let ws = WinSize {
        ws_col: 40,
        ws_row: 10,
    };
    support::with_term(&ws, |t| {
        t.set_prompt_at_bottom(PromptAtBottom::On);
        support::feed(t, b"\x1b[5;1Hstale\x1b[2;1H\x1b]133;A\x07$ \x1b]133;B\x07");
        assert_eq!(row_text(t, 4), "stale");
        assert_eq!(t.cursor().0.index(), 3, "two blank rows walked");
        assert_eq!(row_text(t, 3), "$");
    });
}

/// **Turned on at a prompt, it pins that prompt at once**, the typed
/// text and a selection on the prompt's row going with it and the output
/// above left alone; turned off, nothing moves back.
#[test]
fn turning_it_on_at_a_prompt_pins_that_prompt_at_once() {
    session_at(PromptAtBottom::Off, 2, |t| {
        feed(t, b"ls");
        assert_eq!(row_text(t, 1), "demo$ ls");
        t.watch_rows(Some((Row::from_index(1), Row::from_index(1))));
        t.set_prompt_at_bottom(PromptAtBottom::On);
        assert_eq!(words(t, 0), "alpha beta gamma");
        assert_eq!(row_text(t, 11), "demo$ ls");
        assert_eq!((t.cursor().0.index(), t.cursor().1.index()), (11, 8));
        assert_eq!(t.prompt_row(), Some(Row::from_index(11)));
        let newest = t.prompts().last().expect("a prompt");
        assert_eq!(
            newest.line,
            PromptLine::Screen(Row::from_index(11)),
            "the log"
        );
        let w = t.watched_lines().expect("the selection is kept");
        assert_eq!(w.first, SurfaceLine::At(Row::from_index(11)));
        t.set_prompt_at_bottom(PromptAtBottom::Off);
        assert_eq!(row_text(t, 11), "demo$ ls");
        assert_eq!(t.cursor().0.index(), 11);
    });
}

/// **A five-line prompt through a zoom gesture leaves the session's
/// output where it was written, at the window's largest size**: the
/// recorded zsh draws its prompt in five lines, and with the setting on
/// every step at 111x32, the last among them, has no history, as off has
/// none, and the session's first output line on the screen. A smaller
/// window holds fewer rows above a prompt on its last row: the resize
/// sends the top ones into the history, as it does with the setting off
/// for a screen whose cursor is on its last row (the control below), and
/// a growth brings them back.
#[test]
fn a_tall_prompt_keeps_the_output_through_a_zoom_at_its_largest() {
    let fx = load("resize/fresh-zoom");
    let run = |pin| {
        let mut at_largest = Vec::new();
        replay("resize/fresh-zoom", pin, |t, i| {
            if (fx.steps[i].0.ws_col, fx.steps[i].0.ws_row) == (111, 32) {
                let kept = (0..rows(t)).any(|y| row_text(t, y) == "baud: a fresh shell in /tmp");
                at_largest.push((i, t.scrollback().len(), kept));
            }
        });
        at_largest
    };
    let off = run(PromptAtBottom::Off);
    assert_eq!(
        off.len(),
        4,
        "the gesture reaches its largest size four times"
    );
    assert!(
        off.iter().all(|&(_, h, k)| h == 0 && k),
        "the control: {off:?}"
    );
    let on = run(PromptAtBottom::On);
    assert_eq!(on, off, "the history and the output at the largest size");
    assert_eq!(on.last().map(|s| s.0), Some(fx.steps.len() - 1), "the end");
    // Off, a cursor on the last row: a shrink by six retires six rows, and
    // the growth back brings them back.
    let big = WinSize {
        ws_col: 40,
        ws_row: 26,
    };
    let owned: &'static mut Owned = Box::leak(Box::new(Owned::new(&big)));
    let mut t = Term::new(owned.storage(), &big);
    let mut bytes = Vec::new();
    for n in 0..25 {
        bytes.extend_from_slice(format!("line {n}\r\n").as_bytes());
    }
    bytes.extend_from_slice(b"$ ");
    feed(&mut t, &bytes);
    assert_eq!(t.cursor().0.index(), 25);
    let small = WinSize {
        ws_col: 40,
        ws_row: 20,
    };
    assert_eq!(t.resize(&small), Resized::Ok);
    assert_eq!(t.scrollback().len(), 6, "a shrink retires the top rows");
    assert_eq!(t.resize(&big), Resized::Ok);
    assert_eq!(t.scrollback().len(), 0, "a growth brings them back");
    assert_eq!(row_text(&t, 0), "line 0");
}

/// The screen as text, a row a string.
fn screen_text(t: &Term<'_>) -> Vec<String> {
    (0..rows(t)).map(|y| row_text(t, y)).collect()
}

/// `clear; ls`'s output, then a prompt of two lines, starship's shape: an
/// information line and the input line, the input mark on the second.
const TWO_LINE: &[u8] = b"\x1b]133;A\x07~/dir\r\n$ \x1b]133;B\x07";

/// **A two-line prompt lands with its last line on the last row**, and
/// the output above it stays on the row it was written to.
#[test]
fn a_two_line_prompt_lands_whole_and_moves_no_output() {
    let ws = WinSize {
        ws_col: 20,
        ws_row: 8,
    };
    let mut bytes = b"\x1b[H\x1b[2Jalpha beta\r\n".to_vec();
    bytes.extend_from_slice(TWO_LINE);
    let run = |pin| {
        support::with_term(&ws, |t| {
            t.set_prompt_at_bottom(pin);
            support::feed(t, &bytes);
            (screen_text(t), t.scrollback().len(), t.cursor().0.index())
        })
    };
    let (off, off_hist, _) = run(PromptAtBottom::Off);
    assert_eq!(off[..3], ["alpha beta", "~/dir", "$"], "the control");
    assert_eq!(off_hist, 0);
    let (on, hist, cy) = run(PromptAtBottom::On);
    assert_eq!(on[0], "alpha beta", "the output stays put: {on:?}");
    assert!(on[1..6].iter().all(String::is_empty), "a gap: {on:?}");
    assert_eq!(on[6..], ["~/dir", "$"], "the prompt, whole: {on:?}");
    assert_eq!(
        (hist, cy),
        (0, 7),
        "nothing retired, the input on the last row"
    );
}

/// **`I`, the input mark that ends at the end of the line, pins as `B`
/// does**: a one-line prompt and a two-line one whose input mark is `I`
/// land with their last line on the last row, the output above stays on
/// the row it was written to, and nothing is retired.
#[test]
fn an_input_mark_at_the_end_of_the_line_pins_as_b_does() {
    let ws = WinSize {
        ws_col: 20,
        ws_row: 8,
    };
    let run = |pin, bytes: &[u8]| {
        support::with_term(&ws, |t| {
            t.set_prompt_at_bottom(pin);
            support::feed(t, bytes);
            (screen_text(t), t.scrollback().len(), t.cursor().0.index())
        })
    };
    let one: &[u8] = b"\x1b]133;A\x07$ \x1b]133;I\x07";
    let (off, _, off_cy) = run(PromptAtBottom::Off, one);
    assert_eq!((off[0].as_str(), off_cy), ("$", 0), "the control: {off:?}");
    let (on, hist, cy) = run(PromptAtBottom::On, one);
    assert_eq!(on[7], "$", "one line: {on:?}");
    assert_eq!((hist, cy), (0, 7), "one line: nothing retired");
    let two = b"\x1b[H\x1b[2Jalpha beta\r\n\x1b]133;A\x07~/dir\r\n$ \x1b]133;I\x07";
    let (off, _, _) = run(PromptAtBottom::Off, two);
    assert_eq!(off[..3], ["alpha beta", "~/dir", "$"], "the control");
    let (on, hist, cy) = run(PromptAtBottom::On, two);
    assert_eq!(on[0], "alpha beta", "the output stays put: {on:?}");
    assert_eq!(on[6..], ["~/dir", "$"], "the prompt, whole: {on:?}");
    assert_eq!(
        (hist, cy),
        (0, 7),
        "nothing retired, the input on the last row"
    );
}

/// **A two-line prompt redrawn again and again moves no output row into
/// the history**: each redraw (`CR`, `CUU 1`, `ED`, the mark, the prompt)
/// is drawn back where the last one was.
#[test]
fn a_two_line_prompt_redrawn_moves_no_output() {
    let ws = WinSize {
        ws_col: 20,
        ws_row: 8,
    };
    let mut bytes = b"one\r\ntwo\r\nthree\r\n\x1b]133;P;k=i\x07~/dir\r\n$ \x1b]133;B\x07".to_vec();
    for _ in 0..3 {
        bytes.extend_from_slice(b"\r\x1b[1A\x1b[J\x1b]133;P;k=i\x07~/dir\r\n$ \x1b]133;B\x07");
    }
    let run = |pin| {
        support::with_term(&ws, |t| {
            t.set_prompt_at_bottom(pin);
            support::feed(t, &bytes);
            (screen_text(t), t.scrollback().len())
        })
    };
    let (off, off_hist) = run(PromptAtBottom::Off);
    assert_eq!(
        off[..5],
        ["one", "two", "three", "~/dir", "$"],
        "the control"
    );
    assert_eq!(off_hist, 0);
    let (on, hist) = run(PromptAtBottom::On);
    assert_eq!(on[..3], ["one", "two", "three"], "the output stays: {on:?}");
    assert_eq!(
        on[6..],
        ["~/dir", "$"],
        "the prompt on the last rows: {on:?}"
    );
    assert_eq!(hist, 0, "no redraw retires a row");
}
