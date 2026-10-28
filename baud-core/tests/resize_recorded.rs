//! Real shell traffic, replayed at the sizes it was recorded at.
//!
//! Every resize rule baud has shipped was fixed against a hand-written
//! model of a shell (`resize_matrix.rs`), and two of them in a row
//! traded one defect for another: not reaching into the history stopped
//! a drag stacking prompts and made an `ls` disappear, and taking a
//! shrink's rows off the bottom fixed that and let the shell redraw its
//! prompt over content. The traffic that actually failed lived in a
//! bench with its input in a scratch directory, so nothing gated it.
//!
//! These fixtures are that traffic. `scripts/capture-resize.py` records
//! a real `zsh` through a pty while re-issuing `TIOCSWINSZ`, writing the
//! bytes as `tests/resize/<name>.raw` -- the shape `tests/recordings/`
//! already uses -- beside a `<name>.steps` of `COLSxROWS <bytelen>`
//! lines. `drag` is a 329-step gesture, the sizes `BAUD_DEBUG_RESIZE=1`
//! printed during a session that failed, replayed from
//! `tests/resize/drag.sizes`; `tests/resize/README.md` says how each
//! fixture was made.
//!
//! The invariant they check is the owner's:
//!
//! > A resize never changes the top line. Only feeding bytes may.
//!
//! See `docs/research/notes/m-resize-and-reflow.md`.
mod support;
use baud_core::{Owned, PackedCell, Resized, Term};
use baud_domain::{BytePos, ReadLen, Row, WinSize};

/// One recorded gesture: the bytes, and the size each run of them
/// arrived at.
struct Fixture {
    name: &'static str,
    raw: Vec<u8>,
    steps: Vec<(WinSize, usize)>,
}

fn load(name: &'static str) -> Fixture {
    let dir = format!("{}/../tests/resize", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read(format!("{dir}/{name}.raw"))
        .unwrap_or_else(|e| panic!("{dir}/{name}.raw: {e}"));
    let text = std::fs::read_to_string(format!("{dir}/{name}.steps"))
        .unwrap_or_else(|e| panic!("{dir}/{name}.steps: {e}"));
    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // **Parsed strictly.** `winch` reads its JSON with every field
        // falling back to zero, so a malformed capture replays as a
        // `0x0` resize with no bytes rather than failing -- which is
        // tolerable in a bench and not in a gate.
        let (size, len) = line
            .split_once(' ')
            .unwrap_or_else(|| panic!("{name}.steps:{}: expected `COLSxROWS <len>`", n + 1));
        let (c, r) = size
            .split_once('x')
            .unwrap_or_else(|| panic!("{name}.steps:{}: expected `COLSxROWS`", n + 1));
        let parse = |s: &str, what: &str| -> usize {
            s.parse()
                .unwrap_or_else(|_| panic!("{name}.steps:{}: {what} is not a number", n + 1))
        };
        steps.push((
            WinSize {
                ws_col: u16::try_from(parse(c, "cols")).expect("cols fits"),
                ws_row: u16::try_from(parse(r, "rows")).expect("rows fits"),
            },
            parse(len, "length"),
        ));
    }
    let owed: usize = steps.iter().map(|(_, n)| n).sum();
    assert_eq!(
        owed,
        raw.len(),
        "{name}: the steps account for {owed} bytes and the recording is {}",
        raw.len()
    );
    Fixture { name, raw, steps }
}

fn text_of(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect()
}

fn row_text(t: &Term<'_>, y: usize) -> String {
    text_of(t.row(Row::from_index(y)).expect("a row on the grid"))
        .trim_end()
        .to_string()
}

/// The first row of the screen that holds anything, and what it holds.
///
/// The *first line* rather than the first row: a window whose top rows
/// are blank has its first line further down, and a resize that only
/// changes how much blank sits above it has not moved anything.
fn first_line(t: &Term<'_>) -> Option<String> {
    (0..t.geometry().1.index())
        .map(|y| row_text(t, y))
        .find(|l| !l.is_empty())
}

/// The last row of the screen that holds anything, and what it holds.
fn last_line(t: &Term<'_>) -> Option<String> {
    (0..t.geometry().1.index())
        .rev()
        .map(|y| row_text(t, y))
        .find(|l| !l.is_empty())
}

/// The last line, to at most `n` columns.
///
/// **Comparing whole rows across a width change compares the wrong
/// thing.** A narrower window shows less of the same line, so the text
/// differs while the line has not moved -- which is what the first
/// version of this read as a failure. Bounded by the narrower of the two
/// widths, it asks what the invariant actually says: *which* line is at
/// the anchor, not how much of it fits.
fn last_line_within(t: &Term<'_>, n: usize) -> Option<String> {
    last_line(t).map(|l| l.chars().take(n).collect::<String>().trim_end().to_string())
}

/// Replays `f`, calling `each` after every step with the terminal and
/// the step's index.
fn replay(f: &Fixture, mut each: impl FnMut(&Term<'_>, usize)) {
    let widest = f.steps.iter().map(|(w, _)| w.ws_col).max().expect("steps");
    let tallest = f.steps.iter().map(|(w, _)| w.ws_row).max().expect("steps");
    let max = WinSize {
        ws_col: widest,
        ws_row: tallest,
    };
    // Provisioned at the largest step, because `Owned` provisions exactly
    // what it is given and a growth past it is `Truncated`. `support`'s
    // helpers cannot do this: `with_room` pins the width at `CORPUS`.
    let owned: &'static mut Owned = Box::leak(Box::new(Owned::new(&max)));
    let mut t = Term::new(owned.storage(), &f.steps[0].0);
    let mut at = 0;
    for (i, (ws, len)) in f.steps.iter().enumerate() {
        // Asserted rather than discarded: `winch` drops the `Resized`,
        // and a step silently truncated would make every assertion after
        // it a statement about a grid nobody asked for.
        // A zero size is not a resize, it is a broken capture: the
        // `rows >= 1 / cols >= 2` floor lives in the app's `relayout`,
        // not in `Term::resize`, whose arithmetic underflows on it.
        assert!(
            ws.ws_col >= 2 && ws.ws_row >= 1,
            "{}: step {i} is {}x{}, which no resize can be",
            f.name,
            ws.ws_col,
            ws.ws_row
        );
        assert_eq!(
            t.resize(ws),
            Resized::Ok,
            "{}: step {i} at {}x{} was truncated",
            f.name,
            ws.ws_col,
            ws.ws_row
        );
        // **Fed whole.** A step is one chunk the core was handed --
        // `Pty::drain` returns one `Vec` per `read(2)` and the app feeds
        // each on its own -- so re-chunking here at 4096 was a guess at
        // boundaries the trace now records. `BAUD_TRACE` never writes a
        // step longer than a read, and a pty-captured fixture's steps
        // are one drain each, which is the same thing.
        let bytes = &f.raw[at..at + len];
        at += len;
        if !bytes.is_empty() {
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        }
        each(&t, i);
    }
}

/// A `CSI`'s final byte: `@` through `~`, which is what ends one.
///
/// Named rather than spelled as a range in place, because the audit
/// counts a raw byte range wherever it sits and a budget is a floor.
fn is_final(b: u8) -> bool {
    matches!(b, b'@'..=b'~')
}

/// The text the shell writes at the start of its prompt, taken from the
/// recording rather than written here.
///
/// **Not a literal.** Matching `"zsh "` would be matching one user's
/// prompt on one machine, and a fixture recorded from a different shell
/// would then silently check nothing. The recording says where its own
/// prompts begin: `OSC 133 ; P` opens the prompt, so the first printable
/// run after one is the prompt's own first text.
fn prompt_signature(f: &Fixture) -> String {
    let at = f
        .raw
        .windows(7)
        .position(|w| w == b"\x1b]133;P")
        .unwrap_or_else(|| panic!("{}: no prompt mark to take a signature from", f.name));
    let mut out = String::new();
    // **Past the mark's own terminator first.** `OSC 133 ; P` carries a
    // payload -- the recorded shell sends `;k=i` -- so starting seven
    // bytes in lands inside it and the signature came out as `";k=izs"`,
    // which matched no row. The control caught it; the fix is to find
    // the `BEL` or `ST` that ends the string.
    let mut i = at + 7;
    while i < f.raw.len() && f.raw[i] != 0x07 && f.raw[i] != 0x1b {
        i += 1;
    }
    i += if i < f.raw.len() && f.raw[i] == 0x1b {
        2
    } else {
        1
    };
    // Then over the escapes and the line breaks the shell puts before
    // the first glyph.
    while i < f.raw.len() && out.len() < 6 {
        match f.raw[i] {
            0x1b => {
                // Skip a CSI or an OSC whole: to the final byte of a CSI,
                // or to the BEL or ST of a string.
                i += 1;
                if i < f.raw.len() && f.raw[i] == b'[' {
                    i += 1;
                    while i < f.raw.len() && !is_final(f.raw[i]) {
                        i += 1;
                    }
                } else {
                    while i < f.raw.len() && f.raw[i] != 0x07 && f.raw[i] != b'\\' {
                        i += 1;
                    }
                }
                i += 1;
            }
            b'\r' | b'\n' | 0x07 => i += 1,
            b if b.is_ascii_graphic() || b == b' ' => {
                out.push(char::from(b));
                i += 1;
            }
            // A multi-byte glyph: the signature stops here rather than
            // splitting one, which is enough to identify a row.
            _ => break,
        }
    }
    assert!(
        out.len() >= 3,
        "{}: the prompt signature {out:?} is too short to identify a row",
        f.name
    );
    out
}

/// The controls every test here leans on, asserted once so no assertion
/// below can pass because the fixture was empty.
fn controls(f: &Fixture) {
    assert!(
        f.steps.len() > 20,
        "{}: {} steps is not a gesture",
        f.name,
        f.steps.len()
    );
    // **Transitions, not distinct sizes.** A zoom cycle walks the same
    // handful of sizes in and out repeatedly, so counting distinct ones
    // called a twenty-six step gesture "barely resizing" -- the control
    // measuring the wrong quantity, which is the failure this file is
    // full of guards against.
    let moves = f
        .steps
        .windows(2)
        .filter(|w| (w[0].0.ws_col, w[0].0.ws_row) != (w[1].0.ws_col, w[1].0.ws_row))
        .count();
    assert!(
        moves > 10,
        "{}: the size changes {moves} times, so it barely resizes",
        f.name
    );
    assert!(
        f.raw.windows(6).any(|w| w == b"\x1b]133;"),
        "{}: no OSC 133 in the recording, so the zone rules are untested by it",
        f.name
    );
}

/// The recorded gestures. `drag`, `zoom` and `short` begin with a full
/// screen -- the shell's start-up banner -- so on them a gesture
/// legitimately pushes content into the history; `zoom` and `short`
/// take their first narrowing as a burst, two sizes with nothing read
/// between them, so two of the shell's redraws land in their step 3.
/// `fresh-drag` and
/// `fresh-zoom` are the same size sequences with the shell started in
/// `/tmp`, where one line prints before the prompt: a fresh terminal,
/// the case the first three cannot show. `fresh-burst` is Enter twice,
/// a drag in and out delivered as the app delivers it -- the sizes a
/// display-link tick apart with nothing read between them, so the shell
/// redraws once per size and every redraw but the last is computed for
/// a width the screen has left -- then Enter again, twice over.
/// `live-drag` replays the size transitions of a session the app
/// recorded (`BAUD_TRACE`), 512 of them: a fresh terminal, Enter four
/// times, a drag in and out three times to as small as 21x3.
const FIXTURES: [&str; 7] = [
    "drag",
    "zoom",
    "short",
    "fresh-drag",
    "fresh-zoom",
    "fresh-burst",
    "live-drag",
];
const FRESH: [&str; 2] = ["fresh-drag", "fresh-zoom"];

/// The steps at least `rows` tall, counted off the step list rather than
/// off the replay: the control every fresh-terminal test holds its
/// `checked` count to, so a replay that skipped a step fails rather than
/// checking less than it appears to. At least three, or the fixture never
/// returned to a height worth checking.
fn tall_steps(f: &Fixture, rows: usize) -> usize {
    // From the second step: the first is the fixture's startup, which the
    // tests read their expectations from rather than check.
    let n = f
        .steps
        .iter()
        .skip(1)
        .filter(|(ws, _)| usize::from(ws.ws_row) >= rows)
        .count();
    assert!(
        n >= 3,
        "{}: only {n} steps are {rows} rows or taller",
        f.name
    );
    n
}

/// Every row of the screen, trimmed, for comparing whole screens.
fn screen_text(t: &Term<'_>) -> Vec<String> {
    (0..t.geometry().1.index())
        .map(|y| row_text(t, y))
        .collect()
}

/// The row of the first line that holds anything.
fn first_row(t: &Term<'_>) -> Option<usize> {
    (0..t.geometry().1.index()).find(|y| !row_text(t, *y).is_empty())
}

#[test]
fn every_fixture_is_a_gesture_with_marks_in_it() {
    for name in FIXTURES {
        controls(&load(name));
    }
}

/// The shell's redraws in each step: its `OSC 133;P` marks, one a prompt
/// drawn.
fn redraws_per_step(f: &Fixture) -> Vec<usize> {
    let mut at = 0;
    f.steps
        .iter()
        .map(|(_, len)| {
            let bytes = &f.raw[at..at + len];
            at += len;
            bytes.windows(7).filter(|w| w == b"\x1b]133;P").count()
        })
        .collect()
}

/// `zoom` and `short` are kept for one case: two of the shell's redraws
/// arriving in one step, the first computed for a width the screen has
/// already left. Their `.sizes` deliver the first narrowing as a burst so
/// a capture reaches it whatever the shell's speed; a capture that does
/// not would leave `a_gesture_leaves_one_prompt_on_screen` holding them to
/// nothing.
#[test]
fn zoom_and_short_each_carry_two_redraws_in_one_step() {
    for name in ["zoom", "short"] {
        let f = load(name);
        let per = redraws_per_step(&f);
        assert!(
            per.iter().sum::<usize>() > 10,
            "{name}: {} redraw marks in the whole recording, so this counts nothing",
            per.iter().sum::<usize>()
        );
        let at = per.iter().position(|n| *n >= 2);
        assert_eq!(
            at,
            Some(3),
            "{name}: the first step carrying two redraws is {at:?}, not the burst's step 3: {per:?}"
        );
    }
}

#[test]
fn a_resize_holds_the_bottom_line() {
    // **The anchor is the bottom**, which is the owner's call after a
    // shrink-then-grow was found deleting their `ls`: holding the *top*
    // still means rows come off the bottom, the cursor lands on a row it
    // was not on, and the shell's next redraw overwrites output that no
    // growth can restore. Holding the bottom means rows come off the top
    // into the history, which is where content that no longer fits
    // belongs -- the owner's own refinement, "what the first line is
    // changes when the content starts to push up and exceed the visible
    // side at the current window size".
    //
    // So the line that must not move is the last one. Checked around the
    // `resize` call alone -- nothing fed between the two reads -- so
    // output scrolling, which may and must move things, cannot make it
    // pass or fail.
    for name in FIXTURES {
        let f = load(name);
        controls(&f);
        let widest = f.steps.iter().map(|(w, _)| w.ws_col).max().unwrap();
        let tallest = f.steps.iter().map(|(w, _)| w.ws_row).max().unwrap();
        let owned: &'static mut Owned = Box::leak(Box::new(Owned::new(&WinSize {
            ws_col: widest,
            ws_row: tallest,
        })));
        let mut t = Term::new(owned.storage(), &f.steps[0].0);
        let mut at = 0;
        let mut checked = 0;
        for (i, (ws, len)) in f.steps.iter().enumerate() {
            let narrower = t.geometry().0.index().min(usize::from(ws.ws_col));
            let before = last_line_within(&t, narrower);
            assert_eq!(t.resize(ws), Resized::Ok, "{name}: step {i} truncated");
            let after = last_line_within(&t, narrower);
            if before.is_some() {
                assert_eq!(
                    before, after,
                    "{name}: step {i} to {}x{} moved the bottom line with nothing fed",
                    ws.ws_col, ws.ws_row
                );
                checked += 1;
            }
            let bytes = &f.raw[at..at + len];
            at += len;
            for chunk in bytes.chunks(4096) {
                t.write_at(BytePos::ZERO, chunk);
                t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
            }
        }
        assert!(
            checked > 20,
            "{name}: only {checked} steps had a top line to compare, so this checked nothing"
        );
    }
}

#[test]
fn a_gesture_leaves_one_prompt_on_screen() {
    // After **every** step, not only the last: the owner sees the
    // duplicate mid-drag, so a test that only looks at the end would
    // miss the moment it appears.
    //
    // The prompt is found by what the shell marks rather than by its
    // text: `OSC 133 ; P` opens the zone, and the row the shell was on
    // when it did is the prompt's first. Counting a literal would be
    // counting this user's prompt on this machine.
    for name in FIXTURES {
        let f = load(name);
        controls(&f);
        let sig = prompt_signature(&f);
        // **The control.** A signature that matches nothing counts zero
        // on every step and can never exceed one, which is a test that
        // passes by being blind. It has to find the prompt at least
        // once, on at least one step, or it is not counting prompts.
        let mut ever = 0;
        // One block, plus one for every command that finished: a fixture
        // that presses Enter has that many prompts on screen by right,
        // and only a copy beyond that is a duplicate. Counted off the
        // bytes fed so far, `D` marks being what Enter produces.
        let mut fed = 0;
        let mut done = 0;
        // **One.** The mechanism is in `zoom`'s and `short`'s own bytes,
        // at step 3: two resizes with nothing fed between them, then two
        // of the shell's redraws landing together -- the first padded to
        // the width the shell had at the first `SIGWINCH`, so on the
        // narrower screen its lines wrap and the block grows, and the
        // second's `CUU n` lands below where the first began. The erase
        // on the redraw mark is what takes this to one, and a pin above
        // one is the bug kept. `drag`, `fresh-drag`, `fresh-burst` and
        // `live-drag` need the same erase, each by another way.
        replay(&f, |t, i| {
            let len = f.steps[i].1;
            done += f.raw[fed..fed + len]
                .windows(7)
                .filter(|w| w == b"\x1b]133;D")
                .count();
            fed += len;
            let allowed = 1 + done;
            let rows = t.geometry().1.index();
            let starts = (0..rows)
                .filter(|y| row_text(t, *y).trim_start().starts_with(sig.trim()))
                .count();
            ever = ever.max(starts);
            // **Held at `allowed` on every fixture, `zoom` included.**
            // This was a pin at what each fixture did, with `zoom`
            // stranding a prompt as the open bug; `a6d6c68` fixed it with
            // the erase on the redraw mark (the paragraph above) and put
            // this bound in the pin's place, so a duplicate on any fixture
            // fails here.
            assert!(
                starts <= allowed,
                "{name}: {starts} prompts ({sig:?}) on screen after step {i}, {allowed} allowed:\n{}",
                (0..rows)
                    .map(|y| format!("{y:3}| {}\n", row_text(t, y)))
                    .collect::<String>()
            );
        });
        assert!(
            ever >= 1,
            "{name}: the signature {sig:?} never matched a row, so nothing was counted"
        );
    }
}

#[test]
fn output_written_before_a_gesture_survives_it() {
    // The owner: "did ls, then made window smaller then bigger and the
    // ls disappear". The fixture's own startup is the output -- a
    // shell's banner, written before the first resize -- so this asks
    // whether a real gesture loses real output: at every step back at
    // the starting height, the first line is either on the screen or
    // retained in the history, and never gone.
    //
    // The first version asserted the history was *non-empty* after the
    // gesture, which is true of a screen that started full and false of
    // a fresh one -- a fresh terminal has nothing to retain, and a test
    // that demanded it retain something was demanding the bug.
    let mut with_output = 0;
    for name in FIXTURES {
        let f = load(name);
        controls(&f);
        let sig = prompt_signature(&f);
        let start_rows = usize::from(f.steps[0].0.ws_row);
        let mut first = None;
        let mut checked = 0;
        // The first line is taken from the first step that draws one: a
        // trace the app records begins before the shell has said
        // anything, where a pty capture's first step is the settle.
        replay(&f, |t, i| {
            if first.is_none() {
                first = first_line(t);
                return;
            }
            if t.geometry().1.index() >= start_rows {
                // A fixture whose first line is the prompt itself has no
                // output to lose: the prompt is redrawn at every width,
                // with its right prompt wherever that width puts it, and
                // this test is about output. Left uncounted rather than
                // passed: the sum below says how many fixtures it reached.
                let want = first.as_deref().expect("a step drew something");
                if want.trim_start().starts_with(sig.trim()) {
                    return;
                }
                // Compared with runs of spaces collapsed: a line's padding
                // is the width it was drawn at.
                let squash = |l: &str| l.split_whitespace().collect::<Vec<_>>().join(" ");
                let want = squash(want);
                let on_screen = screen_text(t).iter().any(|l| squash(l) == want);
                let sb = t.scrollback();
                let retained = (0..sb.len()).any(|n| {
                    sb.line(baud_domain::Back::of(n))
                        .is_some_and(|(c, _)| squash(&text_of(c)) == want)
                });
                assert!(
                    on_screen || retained,
                    "{name}: step {i} at {}x{}: the first line {want:?} is neither on screen nor retained",
                    t.geometry().0.index(),
                    t.geometry().1.index()
                );
                checked += 1;
            }
        });
        with_output += usize::from(checked > 0);
    }
    // The fresh fixtures begin with a banner or with the prompt itself; the
    // three full-screen ones begin with output, and those are the ones
    // this test is for.
    assert!(
        with_output >= 3,
        "only {with_output} fixtures had output before the gesture"
    );
}

#[test]
fn the_prompt_to_cursor_distance_holds_across_every_resize() {
    // **The shell's own contract.** zsh keeps one number about the screen:
    // how many rows above the cursor its prompt starts, computed from its
    // own text and `COLUMNS`. On `SIGWINCH` it moves up that many, erases
    // to the end of the screen and redraws; "the rest of the screen
    // contents are unknown" (Schaefer, zsh-workers). So a terminal that
    // changes that distance behind its back -- by moving the cursor and
    // not the prompt, or the prompt and not the cursor -- leaves the top
    // of the old prompt for the erase to miss, and that is the orphan.
    //
    // Checked around the `resize` call alone, with nothing fed, on every
    // step where the shell is at a prompt whose block still fits the new
    // height. A block taller than the screen cannot keep its first row and
    // is not the contract's case: the shell cannot redraw it either.
    for name in FIXTURES {
        let f = load(name);
        controls(&f);
        let widest = f.steps.iter().map(|(w, _)| w.ws_col).max().unwrap();
        let tallest = f.steps.iter().map(|(w, _)| w.ws_row).max().unwrap();
        let owned: &'static mut Owned = Box::leak(Box::new(Owned::new(&WinSize {
            ws_col: widest,
            ws_row: tallest,
        })));
        let mut t = Term::new(owned.storage(), &f.steps[0].0);
        let mut at = 0;
        let mut checked = 0;
        for (i, (ws, len)) in f.steps.iter().enumerate() {
            let before = t.prompt_row().map(|p| (p.index(), t.cursor().0.index()));
            assert_eq!(t.resize(ws), Resized::Ok, "{name}: step {i} truncated");
            if let Some((p, cy)) = before {
                assert!(
                    cy >= p,
                    "{name}: step {i}: the cursor at row {cy} sits above its prompt at row {p}"
                );
                let rows = t.geometry().1.index();
                if cy - p < rows {
                    let dump = || {
                        (0..rows)
                            .map(|y| format!("{y:3}| {}\n", row_text(&t, y)))
                            .collect::<String>()
                    };
                    let cy2 = t.cursor().0.index();
                    let p2 = t.prompt_row().unwrap_or_else(|| {
                        panic!(
                            "{name}: step {i} to {}x{} lost the prompt's first row with nothing fed \
                             (it was {p}, the cursor {cy}, now the cursor is {cy2}):\n{}",
                            ws.ws_col,
                            ws.ws_row,
                            dump()
                        )
                    });
                    let p2 = p2.index();
                    assert!(
                        cy2 >= p2 && cy2 - p2 == cy - p,
                        "{name}: step {i} to {}x{} changed the prompt-to-cursor distance \
                         from {} to {} with nothing fed (prompt {p}->{p2}, cursor {cy}->{cy2}):\n{}",
                        ws.ws_col,
                        ws.ws_row,
                        cy - p,
                        cy2.saturating_sub(p2),
                        dump()
                    );
                    checked += 1;
                }
            }
            let bytes = &f.raw[at..at + len];
            at += len;
            if !bytes.is_empty() {
                t.write_at(BytePos::ZERO, bytes);
                t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            }
        }
        // **The control is against a `prompt_row` that never answers.** Ten
        // mirrors the fixture control's own floor on size transitions;
        // `short` has 19 prompt-time resizes and a threshold copied from
        // `drag` called that "nothing", which is a control measuring the
        // fixture's length rather than the accessor's silence.
        assert!(
            checked > 10,
            "{name}: only {checked} resizes happened at a prompt, so this checked nothing"
        );
    }
}

#[test]
fn a_fresh_terminal_keeps_its_first_line_wherever_the_window_is_tall_enough() {
    // The owner's invariant, stated as they stated it: the size of the
    // window does not change what the first line at the top is; what
    // changes it is content pushing up into the history when it exceeds
    // the visible size. A fresh terminal has a banner line and a prompt,
    // and nothing else ever pushes -- so at every step where the window
    // is at least as tall as it started, the first line is the one it
    // started with, on the row it started on. The recording of
    // 2026-09-18 02:18 shows the prompt at the bottom of a same-sized
    // window with blank rows above it after one resize.
    for name in FRESH {
        let f = load(name);
        controls(&f);
        let start_rows = usize::from(f.steps[0].0.ws_row);
        let mut first = None;
        let mut checked = 0;
        replay(&f, |t, i| {
            if i == 0 {
                first = Some((first_row(t), first_line(t)));
                assert_eq!(
                    first_row(t),
                    Some(0),
                    "{name}: the fresh screen starts at row 0"
                );
                return;
            }
            if t.geometry().1.index() >= start_rows {
                let want = first.clone().expect("step 0 recorded the first line");
                let got = (first_row(t), first_line(t));
                assert_eq!(
                    got,
                    want,
                    "{name}: step {i} at {}x{} moved the first line with nothing pushed up:\n{}",
                    t.geometry().0.index(),
                    t.geometry().1.index(),
                    screen_text(t)
                        .iter()
                        .enumerate()
                        .map(|(y, l)| format!("{y:3}| {l}\n"))
                        .collect::<String>()
                );
                checked += 1;
            }
        });
        assert_eq!(
            checked,
            tall_steps(&f, start_rows),
            "{name}: not every tall step was checked"
        );
    }
}

#[test]
fn a_gesture_on_a_fresh_terminal_creates_no_history() {
    // Nothing in a fresh terminal is content that exceeds the screen once
    // the screen is back at its size, so nothing belongs in the history:
    // a retained line here is one the terminal manufactured -- a row of
    // the shell's own prompt that a redraw at a too-short window scrolled
    // off, or the blank the erase left in its place. Those are what a
    // growth pulled back above the prompt in the recording.
    for name in FRESH {
        let f = load(name);
        controls(&f);
        let start_rows = usize::from(f.steps[0].0.ws_row);
        let mut checked = 0;
        replay(&f, |t, i| {
            if t.geometry().1.index() >= start_rows {
                assert_eq!(
                    t.scrollback().len(),
                    0,
                    "{name}: step {i} at {}x{} holds {} manufactured lines in the history",
                    t.geometry().0.index(),
                    t.geometry().1.index(),
                    t.scrollback().len()
                );
                checked += usize::from(i > 0);
            }
        });
        assert_eq!(
            checked,
            tall_steps(&f, start_rows),
            "{name}: not every tall step was checked"
        );
    }
}

#[test]
fn a_fresh_terminal_returns_to_its_first_screen() {
    // The strongest form: at every step at least as tall as the start,
    // the whole starting screen reads as it did at step 0. Not only the
    // first line and not only the count of prompts -- every row.
    for name in FRESH {
        let f = load(name);
        controls(&f);
        let start = (f.steps[0].0.ws_col, f.steps[0].0.ws_row);
        let mut want = None;
        let mut checked = 0;
        replay(&f, |t, i| {
            if i == 0 {
                want = Some(screen_text(t));
                return;
            }
            // At least as tall as it started: the starting screen's rows
            // must read the same, and whatever the taller window adds
            // below them is blank, because nothing was ever written there.
            // Rows are compared trimmed, so a wider or narrower window is
            // the same rows -- the recorded prompt's lines are short.
            // And at the starting width: the recorded prompt has a right
            // prompt, whose row is padded to the width it was drawn at.
            let (cols, rows) = (t.geometry().0.index(), t.geometry().1.index());
            if cols == usize::from(start.0) && rows >= usize::from(start.1) {
                let got = screen_text(t);
                let want = want.as_ref().expect("step 0");
                assert_eq!(
                    &got[..want.len()],
                    &want[..],
                    "{name}: step {i} at {}x{} does not read as the starting screen",
                    t.geometry().0.index(),
                    rows
                );
                assert!(
                    got[want.len()..].iter().all(String::is_empty),
                    "{name}: step {i} has content below the starting screen's rows:\n{}",
                    got.iter()
                        .enumerate()
                        .map(|(y, l)| format!("{y:3}| {l}\n"))
                        .collect::<String>()
                );
                checked += 1;
            }
        });
        // Its own control: eligible is the starting width *and* at least
        // the starting height, counted off the step list.
        let eligible = f
            .steps
            .iter()
            .skip(1)
            .filter(|(ws, _)| ws.ws_col == start.0 && ws.ws_row >= start.1)
            .count();
        assert!(
            eligible >= 3,
            "{name}: only {eligible} steps are at the starting width and tall"
        );
        assert_eq!(
            checked, eligible,
            "{name}: not every eligible step was checked"
        );
    }
}

#[test]
fn a_block_never_moves_once_it_is_drawn() {
    // A screen recording of 2026-09-18 09:09, frames f12 to f14: the
    // current prompt block sits right under the one before it, the
    // window is resized, and the block is four rows lower with blank
    // rows above it. Each stale redraw's `CUU n` lands below the block's
    // first row -- the block it is counting grew by a wrapped row when
    // its padded line met a narrower screen -- and the new block is
    // drawn where the cursor landed. So: at every step at the starting
    // size, every row up to the cursor of the previous such step reads
    // exactly as it did then. Enter adds a block below; nothing moves
    // what is above.
    for name in ["fresh-drag", "fresh-zoom", "fresh-burst"] {
        let f = load(name);
        controls(&f);
        let start = (f.steps[0].0.ws_col, f.steps[0].0.ws_row);
        let mut last: Option<(usize, Vec<String>)> = None;
        let mut checked = 0;
        // At the starting width and at least the starting height: rows
        // are what a block's position is, and the recorded prompt's first
        // line is padded to the width it was drawn at, so only the same
        // width reads the same.
        replay(&f, |t, i| {
            let (c, r) = t.geometry();
            if c.index() != usize::from(start.0) || r.index() < usize::from(start.1) {
                return;
            }
            let now = screen_text(t);
            let cy = t.cursor().0.index();
            if let Some((was_cy, was)) = &last {
                assert_eq!(
                    &now[..=*was_cy],
                    &was[..=*was_cy],
                    "{name}: step {i}: a block drawn by an earlier step moved:\n{}",
                    now.iter()
                        .enumerate()
                        .map(|(y, l)| format!("{y:3}| {l}\n"))
                        .collect::<String>()
                );
                checked += 1;
            }
            last = Some((cy, now));
        });
        let eligible = f
            .steps
            .iter()
            .filter(|(ws, _)| ws.ws_col == start.0 && ws.ws_row >= start.1)
            .count();
        assert!(
            eligible >= 2,
            "{name}: only {eligible} steps are at the starting width and tall"
        );
        assert_eq!(
            checked,
            eligible - 1,
            "{name}: not every eligible step was checked"
        );
    }
}

/// The rows holding the prompt's first text, top to bottom.
fn prompt_rows(t: &Term<'_>, sig: &str) -> Vec<usize> {
    (0..t.geometry().1.index())
        .filter(|y| row_text(t, *y).trim_start().starts_with(sig))
        .collect()
}

#[test]
fn no_more_than_two_blank_rows_come_between_consecutive_prompts() {
    // A screenshot of 2026-09-18 09:31, from a live trace: four blocks
    // at rows 1-19, then twelve blank rows, then the fifth.
    // Between one prompt block and the next there is the `$` row, the
    // mark's blank row and the next block's first text: one blank row,
    // two when a padded line wraps into a row of spaces. More than two
    // is rows the terminal left behind -- a block's own rows scrolled
    // off by a stale redraw and never taken back. Checked at every
    // step at least as tall as the start, whatever the width, so a
    // drag that never returns to its exact starting size is checked
    // too; `live-drag` returns to none of its sizes.
    for name in FIXTURES {
        let f = load(name);
        controls(&f);
        let sig = prompt_signature(&f);
        let start_rows = usize::from(f.steps[0].0.ws_row);
        let mut checked = 0;
        replay(&f, |t, i| {
            if i == 0 || t.geometry().1.index() < start_rows {
                return;
            }
            let rows = prompt_rows(t, sig.trim());
            for pair in rows.windows(2) {
                let blank = (pair[0] + 1..pair[1])
                    .filter(|y| row_text(t, *y).is_empty())
                    .count();
                assert!(
                    blank <= 2,
                    "{name}: step {i} at {}x{}: {blank} blank rows between the prompts at rows {} and {}:\n{}",
                    t.geometry().0.index(),
                    t.geometry().1.index(),
                    pair[0],
                    pair[1],
                    screen_text(t)
                        .iter()
                        .enumerate()
                        .map(|(y, l)| format!("{y:3}| {l}\n"))
                        .collect::<String>()
                );
            }
            checked += 1;
        });
        assert_eq!(
            checked,
            tall_steps(&f, start_rows),
            "{name}: not every tall step was checked"
        );
    }
}

/// The oldest numbered line the history holds whole, and the rows above
/// it: the tail of the line before, whose first rows a scroll recycled.
/// The recording's `awk` writes `00000` to `02599`, each followed by twenty
/// words, 145 characters, before its clock lines.
fn oldest_numbered(t: &Term<'_>) -> (usize, usize) {
    let sb = t.scrollback();
    (0..sb.len())
        .rev()
        .enumerate()
        .find_map(|(tail, back)| {
            let (cells, _) = sb
                .line(baud_domain::Back::of(back))
                .expect("a retained line");
            let s = text_of(cells);
            let n = s.get(..5).and_then(|n| n.parse().ok())?;
            s[5..].starts_with(" word00").then_some((n, tail))
        })
        .expect("a numbered line in the history")
}

/// **A narrow drag through a full ring counts the rows it drops** (note V,
/// V5). `full-ring` is `/bin/sh` running `awk` over 2,600 lines of 145
/// characters and then `date` every 50 ms, captured by
/// `scripts/capture-resize.py` through a drag from 200 columns to 40 and
/// back while the clock still prints, so every width change is in the
/// output zone and re-cuts the 2,000-line history. A narrower width makes
/// each line more rows, and the ring keeps the newest: each step's count
/// must move by exactly the rows the lost lines held at the width they
/// were dropped from, `ceil(145 / cols)` a line, and a widening drops
/// nothing and brings nothing back.
#[test]
fn a_narrow_drag_through_a_full_ring_counts_the_rows_it_drops() {
    let f = load("full-ring");
    let max = WinSize {
        ws_col: 200,
        ws_row: 50,
    };
    let owned: &'static mut Owned = Box::leak(Box::new(Owned::new(&max)));
    let mut t = Term::new(owned.storage(), &f.steps[0].0);
    let (mut at, mut narrowings, mut lost) = (0, 0, 0);
    for (i, (ws, len)) in f.steps.iter().enumerate() {
        let (cols, _) = t.geometry();
        let fed = t.amplification().0 > 0;
        let (before, was) = (t.reflow_dropped(), fed.then(|| oldest_numbered(&t)));
        assert_eq!(t.resize(ws), Resized::Ok);
        if let Some((n0, tail)) = was {
            assert_eq!(t.primary_zone(), baud_core::Zone::Output);
            let (n1, tail1) = oldest_numbered(&t);
            // The tail goes first, a line of its own to the re-cut.
            let rows = match (n1 > n0, tail1) {
                (true, _) | (false, 0) => tail + (n1 - n0) * 145usize.div_ceil(cols.index()),
                (false, _) => 0,
            };
            assert_eq!(
                t.reflow_dropped().index() - before.index(),
                rows,
                "step {i}, {} to {} columns: lines {n0} to {n1} went",
                cols.index(),
                ws.ws_col
            );
            if usize::from(ws.ws_col) > cols.index() {
                assert_eq!(n1, n0, "step {i}: a widening drops nothing");
            }
            narrowings += usize::from(usize::from(ws.ws_col) < cols.index());
            lost += n1 - n0;
        }
        let bytes = &f.raw[at..at + len];
        at += len;
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }
    assert_eq!(narrowings, 40, "the drag narrowed forty times");
    assert!(lost > 1000, "most of the history went: {lost} lines");
    assert!(
        t.reflow_dropped().index() >= lost,
        "a dropped line is one row or more"
    );
    let total = t.reflow_dropped();
    t.write_at(BytePos::ZERO, b"\x1bc");
    t.feed(BytePos::ZERO, ReadLen::from_index(2));
    assert_eq!(t.reflow_dropped(), total, "a reset keeps the count");
}
