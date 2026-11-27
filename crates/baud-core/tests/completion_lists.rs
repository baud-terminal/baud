//! **A completion list stays on the screen**, in every shell
//! `tools/tests/completion/` recorded: zsh, bash, fish and nushell, each a
//! command typed and Tab pressed, replayed read by read as the pty
//! returned them.
//!
//! bash's readline prints the list below the line and then the prompt
//! again below the list, and zsh does the same when the list does not
//! fit below its prompt or after its "do you wish to see all"
//! question. Both send the prompt's mark again with it -- the `A` of
//! `tools/tests/completion/bashrc`, the `P;k=i` WezTerm's integration puts in
//! zsh's `PS1` -- in a prompt zone no `C` or `D` has closed. The
//! terminal took that mark for the shell redrawing its prompt after a
//! resize, put the cursor back on the block's first row and erased from
//! there down: the list was drawn and gone in the same moment, and the
//! new prompt sat over the old one. A shell that redraws moves the
//! cursor back to its block, or onto its input's line as readline does,
//! before it marks, and a resize comes first; one that prints a list has
//! written below its input with no resize, and the mark there is the
//! prompt drawn again.
//!
//! The controls: the same gestures recorded with no marks, which the
//! marks must not change a cell of, and the shells that keep the list
//! below a prompt they move back up to -- zsh when it fits, zsh's menu
//! selection, a five-line prompt, fish's pager, nushell's menu.
mod support;
use baud_core::{Owned, PackedCell, Resized, Term};
use baud_domain::{Back, BytePos, PromptAtBottom, ReadLen, Row, Rows, WinSize};

/// A recording's bytes and the length of each read, as its `.steps`
/// file gives them; the `# sent` and `# answered` lines are skipped.
struct Recording {
    raw: Vec<u8>,
    ws: WinSize,
    reads: Vec<usize>,
}

fn load(name: &str) -> Recording {
    let dir = format!(
        "{}/../../tools/tests/completion",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read(format!("{dir}/{name}.raw")).expect("the recording");
    let text = std::fs::read_to_string(format!("{dir}/{name}.steps")).expect("its steps");
    let mut ws = None;
    let reads: Vec<usize> = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (size, len) = l.split_once(' ').expect("COLSxROWS <len>");
            let (c, r) = size.split_once('x').expect("COLSxROWS");
            ws = Some(WinSize {
                ws_col: c.parse().expect("cols"),
                ws_row: r.parse().expect("rows"),
            });
            len.parse().expect("length")
        })
        .collect();
    assert_eq!(reads.iter().sum::<usize>(), raw.len(), "{name}");
    Recording {
        raw,
        ws: ws.expect("a read"),
        reads,
    }
}

fn text_of(cells: &[PackedCell]) -> String {
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

/// The screen, a row a line.
fn screen(t: &Term<'_>) -> Vec<String> {
    let vp = t.view(Rows::ZERO);
    (0..vp.rows().index())
        .map(|y| text_of(vp.row(Row::from_index(y))))
        .collect()
}

/// The rows that hold anything, the screen's and the history's.
fn written(t: &Term<'_>) -> usize {
    let hist = t.scrollback();
    let back = (0..hist.len())
        .filter_map(|k| hist.line(Back::of(k)))
        .filter(|(cells, _)| !text_of(cells).is_empty())
        .count();
    back + screen(t).iter().filter(|l| !l.is_empty()).count()
}

/// Replays `name` a read at a time, calling `each` after every read, and
/// hands back the screen, the cursor and the count of redraw erases.
fn replay(name: &str, each: impl FnMut(&Term<'_>, usize)) -> (Vec<String>, (usize, usize), usize) {
    replay_pinned(name, PromptAtBottom::Off, each)
}

/// [`replay`] with `[screen] prompt_at_bottom` as `pin` says.
fn replay_pinned(
    name: &str,
    pin: PromptAtBottom,
    mut each: impl FnMut(&Term<'_>, usize),
) -> (Vec<String>, (usize, usize), usize) {
    let rec = load(name);
    let mut owned = Owned::new(&rec.ws);
    let mut t = Term::new(owned.storage(), &rec.ws);
    t.set_prompt_at_bottom(pin);
    let mut at = 0;
    for (i, n) in rec.reads.iter().enumerate() {
        t.write_at(BytePos::ZERO, &rec.raw[at..at + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(*n));
        at += n;
        each(&t, i);
    }
    let (r, c) = t.cursor();
    (screen(&t), (r.index(), c.index()), t.prompt_erases)
}

fn last(name: &str) -> (Vec<String>, (usize, usize), usize) {
    replay(name, |_, _| ())
}

/// bash's eight rows for `git log --s`, two columns of git's completion.
const BASH_LIST: [&str; 8] = [
    "--shortstat                --since=",
    "--show-linear-break        --single-worktree",
    "--show-notes-by-default    --sparse",
    "--show-pulls               --src-prefix=",
    "--show-signature           --stat",
    "--simplify-by-decoration   --submodule",
    "--simplify-merges          --submodule=",
    "--since-as-filter          --summary",
];

/// **bash's double Tab**: the list, then the prompt again below it, with
/// the line typed so far -- and the marks change nothing of that: the
/// screen is the one the same keys draw with no marks, cell for cell.
#[test]
fn bash_keeps_its_list_above_the_prompt_it_prints_again() {
    let (plain, plain_at, _) = last("bash-double-plain");
    for name in ["bash-double", "bash-show-all"] {
        let (got, at, erases) = last(name);
        assert_eq!(got[0], "demo$ git log --s", "{name}: {got:#?}");
        assert_eq!(got[1..9], BASH_LIST, "{name}: the list stays: {got:#?}");
        assert_eq!(got[9], "demo$ git log --s", "{name}: {got:#?}");
        assert_eq!(at, (9, 17), "{name}: the cursor where bash put it");
        assert_eq!(got, plain, "{name}: the marks change no cell");
        assert_eq!(at, plain_at, "{name}");
        assert_eq!(erases, 0, "{name}: no mark here is a redraw");
    }
}

/// **The same at the bottom of a full screen**: the list scrolls the
/// output up, and the prompt lands on the last row below it.
#[test]
fn bash_keeps_its_list_on_a_full_screen() {
    let (got, at, erases) = last("bash-full");
    assert_eq!(got[0], "17", "{got:#?}");
    assert_eq!(got[13], "30", "{got:#?}");
    assert_eq!(got[14], "demo$ git log --s", "{got:#?}");
    assert_eq!(got[15..23], BASH_LIST, "{got:#?}");
    assert_eq!(got[23], "demo$ git log --s", "{got:#?}");
    assert_eq!(at, (23, 17));
    assert_eq!(erases, 0);
}

/// **zsh, when the list does not fit below its prompt**, in a window of
/// twelve rows: the list scrolls up and the prompt is printed again on
/// the last row, its `P;k=i` mark with it. The screen is the one the
/// same keys draw with no marks.
#[test]
fn zsh_keeps_a_list_that_does_not_fit_below_its_prompt() {
    let (plain, plain_at, plain_erases) = last("zsh-tall-plain");
    assert_eq!(plain_erases, 0, "the control sends no mark");
    let (got, at, _) = last("zsh-tall");
    assert_eq!(
        got[0], "--skip-to                 -- skip the output to the specified path",
        "{got:#?}"
    );
    assert_eq!(
        got[10],
        "--summary                 -- generate condensed summary of extended header i"
    );
    assert_eq!(got[11], "demo% git log --s", "{got:#?}");
    assert_eq!(at, (11, 17));
    assert_eq!(got, plain, "the marks change no cell");
    assert_eq!(at, plain_at);
}

/// **zsh's "do you wish to see all" question, answered `y`**: the whole
/// list, and the prompt printed again below it on the last row.
#[test]
fn zsh_keeps_the_list_it_asked_to_show() {
    let (got, at, _) = last("zsh-ask");
    assert_eq!(got[23], "demo% git log --", "{got:#?}");
    assert_eq!(at, (23, 16));
    assert!(
        got[..23].iter().all(|l| l.starts_with("--")),
        "every row above the prompt is the list: {got:#?}"
    );
    assert_eq!(
        got[22],
        "--ws-error-highlight        -- specify where to highlight whitespace errors"
    );
}

/// **The shells that move back up to their prompt**, which kept the list
/// before as now: zsh when the list fits, zsh's menu selection, the
/// five-line prompt of `tools/tests/resize/`, fish's pager and nushell's menu.
#[test]
fn a_list_below_a_prompt_the_shell_returns_to_stays() {
    let (got, at, _) = last("zsh-fits");
    assert_eq!(got[0], "demo% git log --s", "{got:#?}");
    assert_eq!(
        got[1],
        "--shortstat               -- generate summary diffstat"
    );
    assert_eq!(got[19].split_whitespace().next(), Some("--summary"));
    assert_eq!(at, (0, 17));
    let (got, at, _) = last("zsh-menu");
    assert_eq!(got[0], "demo% git log --shortstat", "{got:#?}");
    assert_eq!(got[19].split_whitespace().next(), Some("--summary"));
    assert_eq!(at, (0, 25));
    let (got, at, _) = last("zsh-five");
    assert_eq!(got[3].split_whitespace().last(), Some("~/repo"), "{got:#?}");
    assert_eq!(
        got[5],
        "--shortstat               -- generate summary diffstat"
    );
    assert_eq!(got[23].split_whitespace().next(), Some("--summary"));
    assert_eq!(at, (4, 13));
    let (got, at, erases) = last("fish");
    assert_eq!(got[0], "demo> git log --s", "{got:#?}");
    assert_eq!(got[1], "--shortstat");
    assert_eq!(got[12], "…and 6 more rows");
    assert_eq!((at, erases), ((0, 17), 0));
    let (got, at, _) = last("nushell");
    assert_eq!(got[0], "demo| str", "{got:#?}");
    assert_eq!(
        got[1],
        "str camel-case            Convert a string to camelCase."
    );
    assert_eq!(
        got[23],
        "str trim                  Trim whitespace or specific character."
    );
    assert_eq!(at, (0, 10));
}

/// **Read by read, nothing a shell wrote is lost**: from the read that
/// answers Tab on, the rows holding text, the screen's and the
/// history's, never fall in number -- a list drawn in one read and
/// erased by the mark in the next is what this counts.
#[test]
fn no_read_after_tab_takes_away_a_written_row() {
    for name in [
        "bash-double",
        "bash-show-all",
        "bash-full",
        "bash-double-plain",
        "zsh-tall",
        "zsh-tall-plain",
        "zsh-ask",
        "zsh-fits",
        "zsh-menu",
        "zsh-five",
        "fish",
        "nushell",
    ] {
        let rec = load(name);
        let steps = std::fs::read_to_string(format!(
            "{}/../../tools/tests/completion/{name}.steps",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("its steps");
        // The index of the first read after the first Tab was sent.
        let mut reads = 0;
        let mut tab = None;
        for l in steps.lines() {
            if l.starts_with("# sent b'\\t'") && tab.is_none() {
                tab = Some(reads);
            } else if !l.is_empty() && !l.starts_with('#') {
                reads += 1;
            }
        }
        let tab = tab.unwrap_or_else(|| panic!("{name}: no Tab"));
        assert!(tab < rec.reads.len(), "{name}");
        let mut most = 0;
        replay(name, |t, i| {
            let n = written(t);
            if i >= tab {
                assert!(
                    n >= most,
                    "{name}: read {i} left {n} rows of text, {most} before"
                );
            }
            most = most.max(n);
        });
    }
}

/// **The rule in a few bytes**: below the input's line, an `A` or a
/// `P;k=i` is the prompt drawn again -- nothing erased, the cursor where
/// the shell put it -- and the prompt log keeps one entry for the
/// command, on the new line.
#[test]
fn a_mark_below_the_lines_under_the_input_erases_nothing() {
    for mark in [&b"\x1b]133;A\x07"[..], b"\x1b]133;P;k=i\x07"] {
        support::with_term(
            &WinSize {
                ws_col: 40,
                ws_row: 8,
            },
            |t| {
                support::feed(t, b"\x1b]133;A\x07$ \x1b]133;B\x07git l");
                support::feed(t, b"\r\nlog\r\nls-files\r\n");
                support::feed(t, mark);
                support::feed(t, b"$ \x1b]133;B\x07git l");
                let got = screen(t);
                assert_eq!(
                    got[..4],
                    ["$ git l", "log", "ls-files", "$ git l"],
                    "{got:#?}"
                );
                assert_eq!(t.cursor().0, Row::from_index(3));
                assert_eq!(t.prompt_erases, 0);
                assert_eq!(t.prompt_row(), Some(Row::from_index(3)));
                assert_eq!(t.prompts().count(), 1, "one command");
            },
        );
    }
}

/// **A redraw from the input's wrapped row is still a redraw**: the
/// input wraps onto a second row, and a mark there -- a shell whose
/// count fell short by that row -- replaces the block from its first
/// row, as before. The row the text wrapped onto is the input's line.
#[test]
fn a_redraw_from_the_row_the_input_wrapped_onto_replaces_the_block() {
    support::with_term(
        &WinSize {
            ws_col: 20,
            ws_row: 8,
        },
        |t| {
            support::feed(t, b"above\r\n\x1b]133;A\x07$ \x1b]133;B\x07");
            support::feed(t, b"echo abcdefghijklmnopqrstuvwxyz");
            assert_eq!(t.cursor().0, Row::from_index(2), "the input wrapped");
            support::feed(t, b"\r\x1b[J\x1b]133;A\x07$ \x1b]133;B\x07echo");
            assert_eq!(t.prompt_erases, 1, "a redraw");
            let got = screen(t);
            assert_eq!(got[..3], ["above", "$ echo", ""], "{got:#?}");
            assert_eq!(t.cursor().0, Row::from_index(1));
        },
    );
}

/// **An input taller than the window, redrawn after a shrink, still
/// replaces its block**: zsh's count clamps at the top row, so its `CUU`
/// stops on a row the input wrapped onto, with the input's first row in
/// the history. Each redraw erases the block and draws it again, and
/// nothing is left in the history once the window grows back.
#[test]
fn a_tall_input_redrawn_after_a_shrink_replaces_the_block() {
    let tall = WinSize {
        ws_col: 40,
        ws_row: 8,
    };
    support::with_term(&tall, |t| {
        let mut block = b"\x1b]133;P;k=i\x07$ \x1b]133;B\x07".to_vec();
        block.extend_from_slice("x".repeat(130).as_bytes());
        support::feed(t, &block);
        let want = screen(t);
        let short = WinSize {
            ws_col: 40,
            ws_row: 3,
        };
        assert_eq!(t.resize(&short), Resized::Ok);
        for n in 1..=3 {
            let mut redraw = b"\r\x1b[3A\x1b[J".to_vec();
            redraw.extend_from_slice(&block);
            support::feed(t, &redraw);
            assert_eq!(t.prompt_erases, n, "redraw {n} replaced the block");
        }
        assert_eq!(t.resize(&tall), Resized::Ok);
        assert_eq!(screen(t), want);
        assert_eq!(t.scrollback().len(), 0, "nothing left in the history");
    });
}

/// **readline's redraw of an input taller than the window**: a `CR` and
/// an `EL` on the input's last row, a row it wrapped onto, then the
/// prompt and the line again. The input's first row is in the history,
/// and the mark is still a redraw.
#[test]
fn a_redraw_on_the_last_row_of_a_tall_input_replaces_the_block() {
    support::with_term(
        &WinSize {
            ws_col: 40,
            ws_row: 3,
        },
        |t| {
            let mut block = b"\x1b]133;A\x07$ \x1b]133;B\x07".to_vec();
            block.extend_from_slice("y".repeat(130).as_bytes());
            support::feed(t, &block);
            let mut redraw = b"\r\x1b[K".to_vec();
            redraw.extend_from_slice(&block);
            support::feed(t, &redraw);
            assert_eq!(t.prompt_erases, 1, "a redraw");
            assert_eq!(t.scrollback().len(), 1, "the input's first row alone");
        },
    );
}

/// **A line break inside the input, redrawn after a resize**: readline
/// on `SIGWINCH` goes to the input's last line, below the break a
/// multi-line paste or a recalled loop puts there, and sends a `CR`, an
/// `EL` and the prompt and the line again. The window changed size since
/// the block's last initial mark, so the mark is the shell's redraw and
/// replaces the block. That redraw's mark is the one the resize asked
/// for: a list and the prompt printed again after it stay.
#[test]
fn a_line_break_in_the_input_redrawn_after_a_resize_replaces_the_block() {
    support::with_term(
        &WinSize {
            ws_col: 40,
            ws_row: 10,
        },
        |t| {
            let block = b"\x1b]133;A\x07$ \x1b]133;B\x07for i in 1\r\ndo echo; done";
            support::feed(t, b"out\r\n");
            support::feed(t, block);
            let shorter = WinSize {
                ws_col: 40,
                ws_row: 8,
            };
            assert_eq!(t.resize(&shorter), Resized::Ok);
            let mut redraw = b"\r\x1b[K".to_vec();
            redraw.extend_from_slice(block);
            support::feed(t, &redraw);
            assert_eq!(t.prompt_erases, 1, "a redraw");
            let got = screen(t);
            assert_eq!(
                got[..4],
                ["out", "$ for i in 1", "do echo; done", ""],
                "{got:#?}"
            );
            support::feed(t, b"\r\ndone\r\ndown\r\n");
            support::feed(t, block);
            assert_eq!(t.prompt_erases, 1, "the list kept");
            let got = screen(t);
            assert_eq!(
                got[..7],
                [
                    "out",
                    "$ for i in 1",
                    "do echo; done",
                    "done",
                    "down",
                    "$ for i in 1",
                    "do echo; done"
                ],
                "{got:#?}"
            );
        },
    );
}

/// **With `[screen] prompt_at_bottom` on**, which walks a prompt down to
/// the last row at its `B`: the prompts sit lower before Tab, and the
/// list stays all the same, every row of it, with no erase the setting
/// off does not make.
#[test]
fn the_list_stays_with_the_prompt_pinned_to_the_last_row() {
    for (name, first, rows) in [
        ("bash-double", BASH_LIST[0], 8),
        ("bash-show-all", BASH_LIST[0], 8),
        ("bash-full", BASH_LIST[0], 8),
        (
            "zsh-tall",
            "--skip-to                 -- skip the output to the specified path",
            11,
        ),
        (
            "zsh-ask",
            "--skip                      -- skip given number of commits before output",
            23,
        ),
    ] {
        let (off, _, off_erases) = last(name);
        let (on, at, on_erases) = replay_pinned(name, PromptAtBottom::On, |_, _| ());
        let from = on
            .iter()
            .position(|l| l == first)
            .unwrap_or_else(|| panic!("{name}: the list's first row is gone: {on:#?}"));
        let start = off.iter().position(|l| l == first).expect("off has it");
        assert_eq!(
            on[from..from + rows],
            off[start..start + rows],
            "{name}: every row of the list"
        );
        assert_eq!(at.0, from + rows, "{name}: the prompt right below the list");
        assert_eq!(on_erases, off_erases, "{name}");
    }
}
