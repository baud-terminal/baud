//! **A change of width at a marked prompt re-cuts the finished output above
//! it and leaves the shell's block as it was** (note M,
//! `docs/research/notes/m-resize-and-reflow.md`): "Do not re-cut what the
//! application is about to redraw; re-cut everything above it."
//!
//! The screen is a long listing written at eighty columns -- an underlined
//! header padded past the width, coloured columns, a long name, a symbolic
//! link longer than the line -- under the command that printed it, and a
//! three-row prompt whose first row carries a mark at the last column, as a
//! right-aligned prompt segment does. The window is narrowed to forty
//! columns in one step and in a drag of small steps, and widened back.
//!
//! What is checked, every step: each line of finished output reads whole and
//! in order, joined across its rows as the wrap flag joins them, from the
//! cells the screen shows -- so a line clipped at the edge, or a soft-wrapped
//! line whose middle stayed past the edge, fails; the shell's block, from
//! its first row to the cursor, reads as it did, cut at the new edge and not
//! re-wrapped, and the cursor keeps its distance below the block's first
//! row, which is the count the shell redraws by. Widened back, the screen
//! is the one the listing was printed on.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, ReadLen, Row, Rows, WinSize, Wrap};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    t.clear_reply();
}

const WIDE: WinSize = WinSize {
    ws_col: 80,
    ws_row: 30,
};

const NARROW: u16 = 40;

/// The prompt: its mark, a first row with a segment at the last column, a
/// directory row, and `$ `, then the input's mark.
fn prompt() -> String {
    "\x1b]133;A\x07zsh\x1b[80G\u{2588}\r\n/tmp/listing\r\n\x1b[32m$\x1b[0m \x1b]133;B\x07".into()
}

/// A listing as a long-format `ls` prints it with colour and an
/// underlined header, written at eighty columns.
fn listing() -> String {
    let u = |s: &str| format!("\x1b[4m{s}\x1b[24m");
    let mut out = String::new();
    // The header: underlined names, the last padded to the widest name,
    // which is wider than the screen.
    out.push_str(&format!(
        "{} {} {} {} {} {}\r\n",
        u("Permissions"),
        u("User  "),
        u("Group"),
        u("Size"),
        u("Date Modified    "),
        u(&format!("Name{}", " ".repeat(60)))
    ));
    let row = |kind: &str, name: &str| {
        format!(
            "\x1b[1;34m{kind}\x1b[0m jordan staff  64 B \x1b[32m26-11-04 21:23:07\x1b[0m {name}\r\n"
        )
    };
    out.push_str(&row("drwxr-xr-x", "\x1b[1;34mApplications\x1b[0m/"));
    out.push_str(&row(
        "drwxr-xr-x",
        "\x1b[1;34mpre-nix-darwin-20260803-201530-backup\x1b[0m/",
    ));
    out.push_str(&row(
        ".rw-r--r--",
        "a-rather-long-file-name-that-keeps-going-for-a-while.txt",
    ));
    out.push_str(&row(
        "lrwxr-xr-x",
        "\x1b[1;36m'Shared - Example Team Space Projects'\x1b[0m -> \
         /home/jordan/Library/CloudStorage/Shared-ExampleTeamSpace-Projects",
    ));
    out
}

/// The whole session: a prompt, the command typed, the listing, the next
/// prompt.
fn session() -> String {
    format!(
        "{p}cd /home/jordan/Library/CloudStorage/listing && ll\r\n\x1b]133;C\x07{l}\x1b]133;D;0\x07{p}",
        p = prompt(),
        l = listing()
    )
}

/// The text the screen shows on row `y`, blanks trimmed.
fn shown(t: &Term<'_>, y: usize) -> String {
    t.view(Rows::ZERO)
        .row(Row::from_index(y))
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|p| char::from_u32(p.value()))
                .unwrap_or(' ')
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// The lines the screen shows above row `end`, each joined across the rows
/// its wrap flag joins, from the cells the screen shows; blanks trimmed.
fn lines_above(t: &Term<'_>, end: usize) -> Vec<String> {
    let vp = t.view(Rows::ZERO);
    let mut out = Vec::new();
    let mut line = String::new();
    for y in 0..end {
        let cells = vp.row(Row::from_index(y));
        let text: String = cells
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|p| char::from_u32(p.value()))
                    .unwrap_or(' ')
            })
            .collect();
        line.push_str(&text);
        if vp.wrapped(Row::from_index(y)) == Wrap::Ended {
            out.push(line.trim_end().to_string());
            line.clear();
        }
    }
    if !line.is_empty() {
        out.push(line.trim_end().to_string());
    }
    out
}

/// What the shell's block shows: the rows from its first to the cursor's.
fn block(t: &Term<'_>) -> Vec<String> {
    let first = t.prompt_row().expect("the block is on the screen").index();
    (first..=t.cursor().0.index())
        .map(|y| shown(t, y))
        .collect()
}

/// The terminal at eighty columns after the session, with its finished
/// output's lines, its block and the cursor's distance below the block.
struct Before {
    output: Vec<String>,
    block: Vec<String>,
    distance: Rows,
    screen: Vec<String>,
}

fn before(t: &Term<'_>) -> Before {
    let first = t.prompt_row().expect("at the prompt").index();
    Before {
        output: lines_above(t, first),
        block: block(t),
        distance: Rows::of_count(t.cursor().0.index() - first),
        screen: (0..WIDE.ws_row.into()).map(|y| shown(t, y)).collect(),
    }
}

/// The checks every step makes at `cols`.
fn holds(t: &Term<'_>, b: &Before, cols: usize, step: &str) {
    let first = t
        .prompt_row()
        .unwrap_or_else(|| panic!("{step}: the block's first row is on the screen"))
        .index();
    let screen: Vec<String> = (0..t.geometry().1.index()).map(|y| shown(t, y)).collect();
    assert_eq!(
        lines_above(t, first),
        b.output,
        "{step}: every line of finished output whole, in order, from what \
         the screen shows:\n{}",
        screen.join("\n")
    );
    let cut: Vec<String> = b
        .block
        .iter()
        .map(|l| {
            l.chars()
                .take(cols)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    assert_eq!(block(t), cut, "{step}: the block's rows as they were");
    assert_eq!(
        Rows::of_count(t.cursor().0.index() - first),
        b.distance,
        "{step}: the cursor's distance below the block's first row"
    );
}

fn at(cols: u16) -> WinSize {
    WinSize {
        ws_col: cols,
        ws_row: WIDE.ws_row,
    }
}

/// **One step to forty columns and back.**
#[test]
fn a_narrowing_at_a_prompt_re_cuts_the_output_above_it_in_one_step() {
    let mut owned = Owned::new(&WIDE);
    let mut t = Term::new(owned.storage(), &WIDE);
    feed(&mut t, session().as_bytes());
    let b = before(&t);
    // The controls: the listing's lines are what was printed, and two of
    // them are longer than forty columns -- the header one row wraps.
    assert!(
        b.output
            .iter()
            .any(|l| l.contains("-> /home/jordan/Library")),
        "{:?}",
        b.output
    );
    assert!(b.output.iter().filter(|l| l.chars().count() > 40).count() >= 4);
    assert_eq!(b.block.len(), 3, "a three-row block: {:?}", b.block);
    let _ = t.resize(&at(NARROW));
    holds(&t, &b, usize::from(NARROW), "at forty");
    let _ = t.resize(&WIDE);
    holds(&t, &b, 80, "back at eighty");
    let screen: Vec<String> = (0..WIDE.ws_row.into()).map(|y| shown(&t, y)).collect();
    assert_eq!(screen, b.screen, "the screen the listing was printed on");
}

/// **A drag to forty columns in steps of three, and back.**
#[test]
fn a_narrowing_at_a_prompt_re_cuts_the_output_above_it_through_a_drag() {
    let mut owned = Owned::new(&WIDE);
    let mut t = Term::new(owned.storage(), &WIDE);
    feed(&mut t, session().as_bytes());
    let b = before(&t);
    let widths: Vec<u16> = (NARROW..80).rev().step_by(3).chain([NARROW]).collect();
    for &c in &widths {
        let _ = t.resize(&at(c));
        holds(&t, &b, usize::from(c), &format!("in at {c}"));
    }
    for c in (NARROW..=80).step_by(3).chain([80]) {
        let _ = t.resize(&at(c));
        holds(&t, &b, usize::from(c), &format!("out at {c}"));
    }
    let screen: Vec<String> = (0..WIDE.ws_row.into()).map(|y| shown(&t, y)).collect();
    assert_eq!(screen, b.screen, "the screen the listing was printed on");
}

/// **The shell's redraw after the narrowing lands on the block**: zsh's
/// redraw on `SIGWINCH` counts the rows its prompt drew, moves up that many
/// from the cursor, erases to the end of the screen and draws the prompt
/// again (note M). Its `CUU 2` from the cursor reaches the block's first
/// row, so the redraw replaces the block whole: one prompt on the screen,
/// and the listing above it untouched.
#[test]
fn the_shells_redraw_after_a_narrowing_replaces_its_block_whole() {
    let mut owned = Owned::new(&WIDE);
    let mut t = Term::new(owned.storage(), &WIDE);
    feed(&mut t, session().as_bytes());
    let b = before(&t);
    let _ = t.resize(&at(NARROW));
    let redraw = format!(
        "\r\x1b[2A\x1b[J\x1b]133;P;k=i\x07zsh\x1b[{NARROW}G\u{2588}\r\n/tmp/listing\r\n\x1b[32m$\x1b[0m \x1b]133;B\x07"
    );
    feed(&mut t, redraw.as_bytes());
    let first = t.prompt_row().expect("at the prompt").index();
    assert_eq!(lines_above(&t, first), b.output, "the listing untouched");
    let screen: Vec<String> = (0..WIDE.ws_row.into()).map(|y| shown(&t, y)).collect();
    assert_eq!(
        screen.iter().filter(|l| l.starts_with("zsh")).count(),
        2,
        "two prompts, the listing's and the redrawn one: {screen:#?}"
    );
    assert_eq!(
        block(&t),
        [
            format!("zsh{}\u{2588}", " ".repeat(36)),
            "/tmp/listing".into(),
            "$".into()
        ],
        "the block redrawn at forty"
    );
}
