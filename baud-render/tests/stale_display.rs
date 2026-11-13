//! **A display the re-cut keeps is the display a fresh layout draws**
//! (FS-C3): the re-cut keeps its last layout of the window's finished
//! paragraphs while no row it read was written since the seal it was read
//! under, and its key -- the window's physical rows, how far it looks back,
//! its size, its finished rows, the cluster slots, whether a string was
//! begun, and whether its top is the history's first line -- is the same
//! (`src/rebreak.rs`, `kept`). Every other change has to reach it through a
//! row's stamp or the key.
//!
//! Each test here is a staleness control for one input the kept display
//! depends on: a screen whose display is held, one operation, then two
//! incremental frames on the CPU path, each drawing, pixel for pixel and
//! ground for ground, what a renderer of its own draws in one full frame.
//! Each case states, against a seal taken before the operation, which
//! rows it wrote and which it did not, so the input it isolates is the one
//! a broken key would miss. That a kept display is kept, and not laid out
//! again every frame, is counted by `rebreak`'s own tests and by
//! `baud-present-macos`'s `caret_frame.rs`.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Row, Rows, Seal, WinSize, WindowAt};
use baud_font::Face;
use baud_render::{Blend, Damage, Ground, PxSize, Renderer, Stack, Target, Theme, WordWrap};
use support::font;

const COLS: usize = 40;
const ROWS: usize = 10;
const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 10,
};

/// The cursor hidden, so no frame here draws one.
const HIDE: &str = "\x1b[?25l";

/// A paragraph the terminal's wrap cuts inside a word, "do|g" at the
/// fortieth column, which the display draws at its words on the same two
/// lines, "dog" moved whole to the second.
const PARAGRAPH: &str = "a quick brown fox jumps over the lazy dog and then runs away";

/// The shell's prompt mark and its prompt: what finishes the rows above.
const PROMPT: &str = "\x1b]133;A\x07$ ";

/// `bytes` fed to the terminal, in reads no longer than the app's.
fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for read in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
    }
}

/// A renderer over `faces` for the grid here.
fn made<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(COLS),
        Rows::of_index(ROWS),
    )
    .unwrap()
}

/// **What a CPU frame drew**: its pixels, and each cell's ground as the
/// convert drew it.
#[derive(Clone, PartialEq)]
struct Drawn {
    pixels: Vec<u32>,
    grounds: Vec<Ground>,
}

/// One CPU frame of `t`'s window `back` lines into its history, onto `px`,
/// as `damage` says. A canvas of another size starts blank.
fn draw(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    back: Rows,
    px: &mut Vec<u32>,
    damage: Damage,
) -> Drawn {
    r.fit(t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    if px.len() != n {
        *px = vec![0; n];
    }
    {
        let mut target = Target::new(&mut px[..], w, w, h).unwrap();
        r.render_at(t, back, &mut target, damage);
    }
    let mut grounds = Vec::new();
    r.grounds(&mut grounds);
    Drawn {
        pixels: px.clone(),
        grounds,
    }
}

/// **What a renderer of its own draws**, in one full frame: it keeps no
/// layout.
fn fresh(faces: &[Face<'_>], t: &mut Term<'_>, back: Rows) -> Drawn {
    let mut r = made(faces);
    let mut px = Vec::new();
    draw(&mut r, t, back, &mut px, Damage::Full)
}

/// What `now` gets wrong against `full`, by name.
fn differs(now: &Drawn, full: &Drawn, frame: &str, wrong: &mut Vec<String>) {
    if now.pixels != full.pixels {
        wrong.push(format!("{frame}: pixels"));
    }
    if now.grounds != full.grounds {
        wrong.push(format!("{frame}: grounds"));
    }
}

/// `n` cells, each in a background colour of its own from the `first`th
/// on, the blue channel `blue`, each the next character of `text` (cycled).
fn coloured_text(first: usize, n: usize, blue: u8, text: &str) -> String {
    let mut out = String::new();
    for (k, c) in text.chars().cycle().take(n).enumerate() {
        let i = first + k;
        out.push_str(&format!(
            "\x1b[0;48;2;{};{};{blue}m{c}",
            i % 41,
            (i / 41) % 41
        ));
    }
    out.push_str("\x1b[0m");
    out
}

/// One step of a case.
enum Step {
    /// Bytes fed to the terminal.
    Bytes(String),
    /// The renderer's `wrap_words` set.
    Wrap(WordWrap),
}

impl Step {
    fn apply(&self, t: &mut Term<'_>, r: &mut Renderer<'_, '_>) {
        match self {
            Step::Bytes(b) => feed(t, b.as_bytes()),
            Step::Wrap(w) => r.set_word_wrap(*w),
        }
    }
}

/// How a case's control knows the operation did what it is there for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shows {
    /// The picture changed.
    Drawn,
    /// The core froze renditions, which draws the same picture through
    /// other ids.
    Froze,
    /// Nothing: a round trip ends on the picture it began on.
    Same,
}

/// What a case's isolation reads: the seal taken before the operation, the
/// window's physical rows then, and how far it looks back.
struct Was {
    seal: Seal,
    at: WindowAt,
    back: Rows,
}

/// **One input the kept display depends on**: a screen whose display is
/// held, the window `back` lines into its history, the steps -- an
/// incremental frame between each two -- and what they wrote and did not,
/// against the seal taken before them.
struct Case {
    what: &'static str,
    setup: String,
    back: Rows,
    steps: Vec<Step>,
    shows: Shows,
    alone: fn(&Term<'_>, &Was) -> Result<(), String>,
}

/// The window's rows written after `w`'s seal.
fn written(t: &Term<'_>, w: &Was) -> Vec<usize> {
    let g = t.row_gens(w.back);
    (0..ROWS)
        .filter(|&y| g.after(Row::from_index(y), &w.seal))
        .collect()
}

/// `Ok` where `holds`, else the isolation that failed.
fn isolated(holds: bool, what: &str) -> Result<(), String> {
    match holds {
        true => Ok(()),
        false => Err(what.to_string()),
    }
}

/// Whether the window shows the physical rows it showed.
fn same_rows(t: &Term<'_>, w: &Was) -> bool {
    t.row_gens(w.back).window() == w.at
}

/// **The screen most cases start from**: the paragraph on rows 3 and 4,
/// the prompt on row 10, after `first`, with no history.
fn held_live(first: &str) -> String {
    format!("{HIDE}{first}\x1b[3;1H{PARAGRAPH}\x1b[10;1H{PROMPT}")
}

/// **One case on the CPU path**: a full frame and two incremental ones,
/// the display held, the steps, then two incremental frames, each the
/// frame a fresh renderer draws.
fn on_the_cpu_path(case: &Case) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, case.setup.as_bytes());
    let back = case.back;
    let mut r = made(&faces);
    let mut px = Vec::new();
    draw(&mut r, &mut t, back, &mut px, Damage::Full);
    for _ in 0..2 {
        draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
    }
    assert!(
        r.bidi().display().is_some(),
        "{}: the control, the display is held",
        case.what
    );
    let before = fresh(&faces, &mut t, back);
    let rewrites = t.style_reclaims().rewrites;
    let was = Was {
        seal: t.sealed(),
        at: t.row_gens(back).window(),
        back,
    };
    for (k, step) in case.steps.iter().enumerate() {
        if k > 0 {
            draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
        }
        step.apply(&mut t, &mut r);
    }
    let after = fresh(&faces, &mut t, back);
    match case.shows {
        Shows::Drawn => assert!(
            after.pixels != before.pixels,
            "{}: the control, the operation is drawn",
            case.what
        ),
        Shows::Froze => assert!(
            t.style_reclaims().rewrites > rewrites,
            "{}: the control, the renditions froze",
            case.what
        ),
        Shows::Same => assert!(
            after.pixels == before.pixels,
            "{}: the control, the round trip ends where it began",
            case.what
        ),
    }
    if let Err(e) = (case.alone)(&t, &was) {
        panic!("{}: the isolation, {e}", case.what);
    }
    let mut wrong = Vec::new();
    for frame in ["the frame after", "the frame after that"] {
        let now = draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
        let full = fresh(&faces, &mut t, back);
        differs(&now, &full, frame, &mut wrong);
    }
    assert!(wrong.is_empty(), "{} drawn stale: {wrong:?}", case.what);
}

/// **S1: a write in a re-broken row is drawn** (a window row's
/// generation): a word of the paragraph's first row written over. The
/// isolation: that row written, the window on the rows it showed. Named
/// break: an answer from the key alone, which keeps the display the write
/// left stale.
#[test]
fn a_write_in_a_re_broken_row_is_drawn() {
    on_the_cpu_path(&Case {
        what: "a write in a re-broken row",
        setup: held_live(""),
        back: Rows::ZERO,
        steps: vec![Step::Bytes("\x1b[3;10HQQ".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                written(t, w).contains(&2) && same_rows(t, w),
                "row 3 written, the window on its rows",
            )
        },
    });
}

/// **S2: a line rendition set on a re-broken row is drawn** (a window
/// row's generation): DECDWL on the paragraph's second row, which keeps a
/// paragraph at the terminal's cut. DECDWL clears the cells past the half
/// and sets the row's mark, both stamped. Named break: an answer from the
/// key alone.
#[test]
fn a_line_rendition_set_on_a_re_broken_row_is_drawn() {
    on_the_cpu_path(&Case {
        what: "DECDWL on a re-broken row",
        setup: held_live(""),
        back: Rows::ZERO,
        steps: vec![Step::Bytes("\x1b[4;1H\x1b#6".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                written(t, w).contains(&3) && same_rows(t, w),
                "row 4 written, the window on its rows",
            )
        },
    });
}

/// **S3: a write in the row below the window its last paragraph runs onto
/// is drawn** (a row the context read below the window): the window two
/// lines back, its last row the paragraph's first, the paragraph's second
/// on the live row below it; that row's first cell written blank, which
/// ends "do" at the cut and draws it on the window's last row. The
/// isolation: no window row written, the row below it written, the window
/// on its rows. Named break: the rows below the window not read for their
/// generation.
#[test]
fn a_write_in_the_row_below_the_window_its_paragraph_runs_onto_is_drawn() {
    let lines: String = (0..10).map(|i| format!("h{i}\r\n")).collect();
    on_the_cpu_path(&Case {
        what: "a write below the window",
        setup: format!("{HIDE}{lines}{PARAGRAPH}\r\n{PROMPT}"),
        back: Rows::of_index(2),
        steps: vec![Step::Bytes("\x1b[9;1H ".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            let below = t
                .row_gens(Rows::of_count(w.back.index() - 1))
                .after(Row::from_index(ROWS - 1), &w.seal);
            isolated(
                written(t, w).is_empty() && below && same_rows(t, w),
                "no window row written, the row below it written",
            )
        },
    });
}

/// **S4: history erased above the window is drawn** (a row the context
/// read above the window): the window's first row the paragraph's second,
/// its first the history's last line; DECSED 3 erases the history in
/// place, no line dropped, and the window's first line loses the "do" it
/// drew from above. The isolation: no window row written, the row above
/// written and still held, the window on its rows. Named break: the rows
/// above the window not read for their generation.
#[test]
fn history_erased_above_the_window_is_drawn() {
    let lines: String = (1..=8).map(|i| format!("x{i}\r\n")).collect();
    on_the_cpu_path(&Case {
        what: "the history above erased",
        setup: format!("{HIDE}{PARAGRAPH}\r\n{lines}{PROMPT}"),
        back: Rows::ZERO,
        steps: vec![Step::Bytes("\x1b[?3J".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            let one = Rows::of_index(1);
            isolated(
                written(t, w).is_empty()
                    && t.looking_back(one) == one
                    && t.row_gens(one).after(Row::ZERO, &w.seal)
                    && same_rows(t, w),
                "no window row written, the row above it written and held",
            )
        },
    });
}

/// **S5: a freeze under a re-broken paragraph is drawn** (a window row's
/// generation, through the ids a freeze rewrites): thirteen lines of
/// history, each cell a rendition of its own, two of them the paragraph,
/// under a screen of plain text, and the window over the paragraph; then
/// the screen written three times over, each cell a rendition of its own,
/// which freezes the history. The isolation: the paragraph's rows written,
/// the window on its rows. Named break: an answer from the key alone.
#[test]
fn a_freeze_under_a_re_broken_paragraph_is_drawn() {
    let mut setup = HIDE.to_string();
    for line in 0..5 {
        setup.push_str(&coloured_text(line * COLS, COLS, 20, "q"));
        setup.push_str("\r\n");
    }
    setup.push_str(&coloured_text(0, PARAGRAPH.len(), 22, PARAGRAPH));
    setup.push_str("\r\n");
    for line in 7..13 {
        setup.push_str(&coloured_text(line * COLS, COLS, 20, "q"));
        setup.push_str("\r\n");
    }
    setup.push_str("plain 0");
    for k in 1..ROWS {
        setup.push_str(&format!("\r\nplain {k}"));
    }
    let mut freezing = String::new();
    for pass in 0..3 {
        freezing.push_str("\x1b[H");
        freezing.push_str(&coloured_text(pass * COLS * ROWS, COLS * ROWS, 21, "q"));
    }
    on_the_cpu_path(&Case {
        what: "a freeze under the paragraph",
        setup,
        back: Rows::of_index(ROWS),
        steps: vec![Step::Bytes(freezing)],
        shows: Shows::Froze,
        alone: |t, w| {
            let rows = written(t, w);
            isolated(
                rows.contains(&2) && rows.contains(&3) && same_rows(t, w),
                "the paragraph's rows written, the window on its rows",
            )
        },
    });
}

/// **S6: a scroll under a window held back is drawn** (the window's
/// physical rows): the window five lines back over a paragraph in the
/// history, no prompt marks; one line feed at the live screen's bottom
/// scrolls a line in below the window, which then shows the rows one
/// further on, none of them written. The isolation: no window row written,
/// the window on other rows. Named break: the window's rows not in the
/// key.
#[test]
fn a_scroll_under_a_window_held_back_is_drawn() {
    let above: String = (0..5).map(|i| format!("n{i}\r\n")).collect();
    let below: String = (0..11).map(|i| format!("m{i}\r\n")).collect();
    on_the_cpu_path(&Case {
        what: "a scroll under a held window",
        setup: format!("{HIDE}{above}{PARAGRAPH}\r\n{below}m11"),
        back: Rows::of_index(5),
        steps: vec![Step::Bytes("\r\n".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                written(t, w).is_empty() && !same_rows(t, w),
                "no window row written, the window on other rows",
            )
        },
    });
}

/// **S7: a command that starts its output draws the rows below the prompt
/// as stored** (the finished rows): the window three lines back, a
/// paragraph in the history at its top and one on the live screen below
/// it, both above the prompt and so both re-broken; `OSC 133 ; C` opens
/// the command's output, and the live screen is no longer finished, so
/// the second paragraph is drawn as the terminal cut it while the first is
/// still re-broken. The isolation: no window row written, the window on
/// its rows. Named break: the finished rows not in the key.
#[test]
fn a_command_that_starts_its_output_draws_its_rows_as_stored() {
    let below: String = (3..=8).map(|i| format!("y{i}\r\n")).collect();
    on_the_cpu_path(&Case {
        what: "output begun",
        setup: format!("{HIDE}{PARAGRAPH}\r\nh2\r\nx0\r\n{PARAGRAPH}\r\n{below}{PROMPT}"),
        back: Rows::of_index(3),
        steps: vec![Step::Bytes("\x1b]133;C\x07".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                written(t, w).is_empty() && same_rows(t, w),
                "no window row written, the window on its rows",
            )
        },
    });
}

/// **S8: a cell written into a string keeps its paragraph at the cut**
/// (the strings, where a string was already begun): on a screen that began
/// one before its first layout, a cell of the paragraph written inside a
/// new SDS; a paragraph holding a string is drawn as stored. The
/// isolation: that row written. Named break: an answer from the key
/// alone, which, with the bypass for strings lifted, is what a string
/// bound moved without a stamp would draw.
#[test]
fn a_cell_written_into_a_string_keeps_its_paragraph_at_the_cut() {
    on_the_cpu_path(&Case {
        what: "a cell written into a string",
        setup: held_live("\x1b[1;1H\x1b[1]abc\x1b[0]"),
        back: Rows::ZERO,
        steps: vec![Step::Bytes("\x1b[3;10H\x1b[1]Q\x1b[0]".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                written(t, w).contains(&2) && same_rows(t, w),
                "row 3 written, the window on its rows",
            )
        },
    });
}

/// **S8, its control: a string opened and closed outside the display
/// leaves it as drawn**: on a screen that began a string before its first
/// layout, with a status line, an SDS opened and closed on the status
/// line. The kept display is kept (`rebreak`'s
/// `a_string_opened_outside_what_the_display_read_keeps_it`) and is the
/// one a fresh layout draws. The isolation: the status line written, no
/// window row.
#[test]
fn a_string_opened_outside_the_display_leaves_it_as_drawn() {
    on_the_cpu_path(&Case {
        what: "a string opened on the status line",
        setup: held_live("\x1b[2$~\x1b[1;1H\x1b[1]abc\x1b[0]"),
        back: Rows::ZERO,
        steps: vec![Step::Bytes("\x1b[1$}\x1b[1]abc\x1b[0]\x1b[0$}".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                written(t, w).is_empty()
                    && t.status_gen().is_some_and(|s| s.after(&w.seal))
                    && same_rows(t, w),
                "the status line written, no window row",
            )
        },
    });
}

/// **S9: across each gate and back, the display is drawn as it was**:
/// the alternate screen entered and left, BDSM reset and set, and the
/// renderer's `wrap_words` off and on, a frame drawn between, during which
/// the re-cut answers nothing and keeps its layout. Controls, not reds: the
/// round trip ends on the picture it began on, and no window row was
/// written.
#[test]
fn each_gate_round_trip_leaves_the_display_as_drawn() {
    let round = |what: &'static str, steps: Vec<Step>| Case {
        what,
        setup: held_live(""),
        back: Rows::ZERO,
        steps,
        shows: Shows::Same,
        alone: |t, w| {
            isolated(
                written(t, w).is_empty() && same_rows(t, w),
                "no window row written, the window on its rows",
            )
        },
    };
    for case in [
        round(
            "?1049 set and reset",
            vec![
                Step::Bytes("\x1b[?1049h\x1b[2;2Halt".into()),
                Step::Bytes("\x1b[?1049l".into()),
            ],
        ),
        round(
            "BDSM reset and set",
            vec![Step::Bytes("\x1b[8l".into()), Step::Bytes("\x1b[8h".into())],
        ),
        round(
            "wrap_words off and on",
            vec![Step::Wrap(WordWrap::Off), Step::Wrap(WordWrap::On)],
        ),
    ] {
        on_the_cpu_path(&case);
    }
}
