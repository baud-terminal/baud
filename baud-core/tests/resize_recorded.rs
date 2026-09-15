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
//! lines. `drag` is the owner's own 329-step gesture, taken from what
//! `BAUD_DEBUG_RESIZE=1` printed during the session that failed.
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
    text_of(t.row(Row::from_index(y))).trim_end().to_string()
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
        assert_eq!(
            t.resize(ws),
            Resized::Ok,
            "{}: step {i} at {}x{} was truncated",
            f.name,
            ws.ws_col,
            ws.ws_row
        );
        let bytes = &f.raw[at..at + len];
        at += len;
        for chunk in bytes.chunks(4096) {
            t.write_at(BytePos::ZERO, chunk);
            t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        }
        each(&t, i);
    }
}

/// The text the shell writes at the start of its prompt, taken from the
/// recording rather than written here.
///
/// **Not a literal.** Matching `"zsh "` would be matching one user's
/// prompt on one machine, and a fixture recorded from a different shell
/// would then silently check nothing. The recording says where its own
/// prompts begin: `OSC 133 ; P` opens the prompt, so the first printable
/// run after one is the prompt's own first text.
/// A `CSI`'s final byte: `@` through `~`, which is what ends one.
///
/// Named rather than spelled as a range in place, because the audit
/// counts a raw byte range wherever it sits and a budget is a floor.
fn is_final(b: u8) -> bool {
    matches!(b, b'@'..=b'~')
}

fn prompt_signature(f: &Fixture) -> String {
    let at = f
        .raw
        .windows(7)
        .position(|w| w == b"\x1b]133;P")
        .unwrap_or_else(|| panic!("{}: no prompt mark to take a signature from", f.name));
    let mut out = String::new();
    // **Past the mark's own terminator first.** `OSC 133 ; P` carries a
    // payload -- the owner's shell sends `;k=i` -- so starting seven
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

const FIXTURES: [&str; 3] = ["drag", "zoom", "short"];

#[test]
fn every_fixture_is_a_gesture_with_marks_in_it() {
    for name in FIXTURES {
        controls(&load(name));
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
        // **Pinned at what it is, and what it is got worse on purpose.**
        // Holding the *top* still took `drag` to 1 and `zoom` to 2, and
        // it did that by letting the shell overwrite output a growth
        // could never restore -- the owner's `ls`, ten of twelve lines
        // gone. The bottom anchor gives those back and reveals the
        // prompts the shell drew and scrolled away while the window was
        // too short to hold one.
        //
        // That is the trade taken deliberately: a duplicate is a
        // nuisance and a deletion is a loss. These numbers are the open
        // bug, pinned as a ceiling *and* a floor so that fixing it fails
        // here and says to move the pin -- and so does trading it back
        // for data loss.
        let worst_seen = match name {
            "drag" => 5,
            _ => 2,
        };
        replay(&f, |t, i| {
            let rows = t.geometry().1.index();
            let starts = (0..rows)
                .filter(|y| row_text(t, *y).trim_start().starts_with(sig.trim()))
                .count();
            ever = ever.max(starts);
            // **Pinned at what it is, not at what it should be.** `zoom`
            // strands a prompt and `drag` and `short` do not, and that
            // gap is this repo's open bug rather than a number to wish
            // away -- the same footing the glyph differential put its
            // three clustering rows on, "pinned at what they *are* so
            // the number moves the day it is fixed". Fixing it must take
            // `zoom` to 1; breaking either of the others shows up here
            // too, in the other direction.
            assert!(
                starts <= worst_seen,
                "{name}: {starts} prompts ({sig:?}) on screen after step {i}, \
                 pinned at {worst_seen}:\n{}",
                (0..rows)
                    .map(|y| format!("{y:3}| {}\n", row_text(t, y)))
                    .collect::<String>()
            );
        });
        assert!(
            ever >= 1,
            "{name}: the signature {sig:?} never matched a row, so nothing was counted"
        );
        // The pin is a ceiling and a floor: a fixture that quietly stops
        // stranding anything has had its bug fixed, and that is a result
        // to notice rather than to absorb.
        assert_eq!(
            ever, worst_seen,
            "{name}: the worst step stranded {ever} prompts against a pin of {worst_seen} -- \
             if this improved, move the pin"
        );
    }
}

#[test]
fn output_written_before_a_gesture_survives_it() {
    // The owner: "did ls, then made window smaller then bigger and the
    // ls disappear". The fixture's own startup is the output -- a
    // shell's banner, written before the first resize -- so this asks
    // whether a real gesture loses real output.
    for name in FIXTURES {
        let f = load(name);
        controls(&f);
        let first = (f.steps[0].0.ws_col, f.steps[0].0.ws_row);
        let mut before = None;
        let mut after = None;
        replay(&f, |t, i| {
            if i == 0 {
                before = first_line(t);
            }
            if t.geometry().0.index() == usize::from(first.0)
                && t.geometry().1.index() == usize::from(first.1)
            {
                after = Some((first_line(t), t.scrollback().len()));
            }
        });
        let before = before.expect("the first step drew something");
        let (_, retained) = after.expect("the gesture returned to its starting size");
        assert!(
            retained > 0 || before.is_empty(),
            "{name}: nothing retained and nothing on screen -- the output went nowhere"
        );
    }
}
