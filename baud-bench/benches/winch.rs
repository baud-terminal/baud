//! Replay a real shell's resize traffic through the core and say, step by
//! step, what each resize did to the prompt.
//!
//! The owner reports that a resize leaves another copy of their prompt
//! behind. Captured from their own `zsh`, one resize looks like
//! `\r` `CUU n` `ED 0` and then the prompt redrawn -- so the shell relies
//! on `CUU n` landing on the first line of the prompt it last drew. If it
//! does not, the erase misses the old prompt and the redraw adds a second.
//!
//! The shell's contract is one number: the rows from its prompt's first
//! line to the cursor (note M). So around every bare `resize` this prints
//! that distance before and after and flags the step that changed it;
//! then it feeds the step's bytes **whole** -- a step is one chunk the
//! core was handed -- and counts the prompts on screen by the recording's
//! own signature, dumping the screen at the step the count rises. No
//! window, no GPU, no shell: the stream is a recording.
//!
//! Usage: `winch <prefix>` for `<prefix>.raw` + `<prefix>.steps` -- the
//! `tests/resize/` and `BAUD_TRACE` format -- or `winch <captured.json>`
//! for the older flat-array capture.
//!
//! `winch <prefix> cost [reps]` answers a different question, the one row
//! 10 of note N says is owed before anything coalesces the shell's
//! stale-width redraws: **what a drag step costs the core**. It replays
//! the same steps with nothing printed per step and reports, per step and
//! over the gesture, the resize and the feed apart, with a null arm
//! beside them so the smaller of the two is read against what a clock
//! pair costs. No window and no GPU: this is the core's half, and a
//! frame is the renderer's.

use baud_core::{Owned, PackedCell, Resized, Term, Zone};
use baud_domain::{Back, BytePos, ReadLen, Row, WinSize};
use std::time::{Duration, Instant};

/// One step: the size in force, and the bytes that arrived at it.
type Step = (WinSize, Vec<u8>);

fn load(path: &str) -> Vec<Step> {
    if path.ends_with(".json") {
        return load_json(path);
    }
    let raw = std::fs::read(format!("{path}.raw")).unwrap_or_else(|e| panic!("{path}.raw: {e}"));
    let text = std::fs::read_to_string(format!("{path}.steps"))
        .unwrap_or_else(|e| panic!("{path}.steps: {e}"));
    let mut steps = Vec::new();
    let mut at = 0;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Generic so one message serves the two splits and the three parses,
        // whatever each is expected to produce.
        fn bad<T>(path: &str, n: usize, line: &str) -> T {
            panic!(
                "{path}.steps:{}: expected `COLSxROWS <len>`, got {line:?}",
                n + 1
            )
        }
        let (size, len) = line.split_once(' ').unwrap_or_else(|| bad(path, n, line));
        let (c, r) = size.split_once('x').unwrap_or_else(|| bad(path, n, line));
        let ws = WinSize {
            ws_col: c.parse().unwrap_or_else(|_| bad(path, n, line)),
            ws_row: r.parse().unwrap_or_else(|_| bad(path, n, line)),
        };
        let len: usize = len.parse().unwrap_or_else(|_| bad(path, n, line));
        assert!(
            at + len <= raw.len(),
            "{path}.steps:{}: the steps claim more bytes than the recording holds",
            n + 1
        );
        steps.push((ws, raw[at..at + len].to_vec()));
        at += len;
    }
    assert_eq!(
        at,
        raw.len(),
        "{path}: the steps account for {at} bytes and the recording is {}",
        raw.len()
    );
    steps
}

/// The older capture: a flat array of `{cols, rows, bytes:[..]}`, read
/// with a hand parser rather than a dependency, since this crate has none.
fn load_json(path: &str) -> Vec<Step> {
    let raw = std::fs::read_to_string(path).expect("capture");
    let mut steps = Vec::new();
    for chunk in raw.split("{\"cols\":").skip(1) {
        let num = |key: &str| -> u16 {
            chunk
                .split(key)
                .nth(1)
                .and_then(|s| s.trim_start_matches(['"', ':']).split(',').next())
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0)
        };
        let cols: u16 = chunk
            .split(',')
            .next()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let rows = num("\"rows\"");
        let bytes: Vec<u8> = chunk
            .split("\"bytes\": [")
            .nth(1)
            .or_else(|| chunk.split("\"bytes\":[").nth(1))
            .map(|s| {
                s.split(']')
                    .next()
                    .unwrap_or("")
                    .split(',')
                    .filter_map(|v| v.trim().parse::<u8>().ok())
                    .collect()
            })
            .unwrap_or_default();
        steps.push((
            WinSize {
                ws_col: cols,
                ws_row: rows,
            },
            bytes,
        ));
    }
    steps
}

fn text(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect()
}

fn is_final(b: u8) -> bool {
    matches!(b, b'@'..=b'~')
}

/// The text the shell writes at the start of its prompt, from the
/// recording rather than from a literal: the first printable run after
/// the first `OSC 133 ; P` or `; A`, past the mark's own terminator and
/// the escapes the shell puts before its first glyph. `None` when the
/// recording carries no mark, so a count of zero is reported as
/// "uncounted" rather than as a clean screen.
fn signature(raw: &[u8]) -> Option<String> {
    let at = raw
        .windows(7)
        .position(|w| w == b"\x1b]133;P" || w == b"\x1b]133;A")?;
    let mut i = at + 7;
    while i < raw.len() && raw[i] != 0x07 && raw[i] != 0x1b {
        i += 1;
    }
    i += if i < raw.len() && raw[i] == 0x1b {
        2
    } else {
        1
    };
    let mut out = String::new();
    while i < raw.len() && out.len() < 6 {
        match raw[i] {
            0x1b => {
                i += 1;
                if i < raw.len() && raw[i] == b'[' {
                    i += 1;
                    while i < raw.len() && !is_final(raw[i]) {
                        i += 1;
                    }
                } else {
                    while i < raw.len() && raw[i] != 0x07 && raw[i] != b'\\' {
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
            _ => break,
        }
    }
    (out.trim().len() >= 3).then(|| out.trim().to_string())
}

fn copies(t: &Term<'_>, sig: Option<&str>) -> Option<usize> {
    let sig = sig?;
    let (_, rows) = t.geometry();
    Some(
        (0..rows.index())
            .filter(|y| {
                text(t.row(Row::from_index(*y)))
                    .trim_start()
                    .starts_with(sig)
            })
            .count(),
    )
}

fn dump(t: &Term<'_>) {
    let (c, r) = t.geometry();
    for y in 0..r.index() {
        let line = text(&t.row(Row::from_index(y))[..c.index()]);
        let line = line.trim_end();
        if !line.is_empty() {
            println!("    {y:3}| {}", line.chars().take(72).collect::<String>());
        }
    }
}

/// Row 10's number: what one step of a resize gesture costs the core.
/// Each step is a size and the bytes that arrived at it, so the step's
/// work is a `resize` -- which re-cuts the lines when the width changes
/// in `Zone::Output` -- and a `feed` of whatever the shell sent for the
/// size it thought it had. Both are timed apart.
///
/// `reps` whole replays, the first discarded as warm-up, and the
/// per-step figures are the median round divided by the step count --
/// an average over the gesture's steps, not a per-step median, which
/// this arm does not collect.
///
/// Three things here exist because a number that looks like a
/// measurement is not one:
///
/// * **A fresh [`Owned`] per round.** `Term::new` resets the cursor and
///   the counters and not the grid, the marks or the book, which live in
///   `Storage` -- so a round on a reused one starts on the last round's
///   screen and does different work. Measured: `short` writes 12,612
///   parser cells from clean and 15,219 from dirty, +21%.
/// * **A null arm.** An empty timed region per step, accumulated beside
///   the resize, so the resize column is read against what a clock pair
///   costs rather than against zero.
/// * **The zone counts.** A width change re-cuts the primary only in
///   `Zone::Output`, so the claim that a drag at a prompt does not
///   reflow is counted at the call rather than asserted in prose.
fn cost(path: &str, steps: &[Step], reps: usize) {
    let geom = baud_bench::host_geometry();
    let max = WinSize {
        ws_col: steps
            .iter()
            .map(|(w, _)| w.ws_col)
            .max()
            .unwrap_or(0)
            .max(400),
        ws_row: steps
            .iter()
            .map(|(w, _)| w.ws_row)
            .max()
            .unwrap_or(0)
            .max(150),
    };
    let first = &steps.first().expect("a first step").0;

    /// A count taken over one replay: cells, steps or lines. `usize`
    /// rather than [`baud_domain::Count`]'s `u16`, because a gesture
    /// writes up to 200,000 parser cells and a bounded type on an
    /// unbounded quantity does not fail loudly -- it returns a
    /// plausible number, which is the `ByteLen`-as-a-read-length
    /// failure this repo already records.
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
    struct Tally(usize);
    impl Tally {
        fn of(n: usize) -> Tally {
            Tally(n)
        }
        fn index(self) -> usize {
            self.0
        }
        fn plus(self, n: usize) -> Tally {
            Tally(self.0 + n)
        }
    }
    struct Round {
        resize: Duration,
        feed: Duration,
        null: Duration,
        cells: Tally,
        wide: Tally,
        wide_in_output: Tally,
        lines: Tally,
    }
    let mut rounds: Vec<Round> = Vec::new();
    for _ in 0..reps.max(2) {
        let mut owned = Owned::with_geometry(&max, &geom);
        let mut t = Term::new(owned.storage(), first);
        let (mut resize, mut feed, mut null) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
        let (mut cells, mut wide, mut wide_in_output) =
            (Tally::default(), Tally::default(), Tally::default());
        for (ws, bytes) in steps {
            let changes_width = t.geometry().0.index() != usize::from(ws.ws_col);
            if changes_width {
                wide = wide.plus(1);
                if t.zone() == Zone::Output {
                    wide_in_output = wide_in_output.plus(1);
                }
            }
            let z = Instant::now();
            null += z.elapsed();
            let a = Instant::now();
            let r = t.resize(ws);
            resize += a.elapsed();
            assert_eq!(r, Resized::Ok, "{path}: a step was clamped by the storage");
            let before = t.amplification().1;
            let b = Instant::now();
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            feed += b.elapsed();
            cells = cells.plus(t.amplification().1 - before);
        }
        let lines = Tally::of(t.scrollback().len());
        rounds.push(Round {
            resize,
            feed,
            null,
            cells,
            wide,
            wide_in_output,
            lines,
        });
    }
    rounds.remove(0);
    let med = |mut v: Vec<u128>| {
        v.sort_unstable();
        v[v.len() / 2]
    };
    let span = |v: &[Tally]| {
        let lo = v.iter().min().copied().unwrap_or_default();
        let hi = v.iter().max().copied().unwrap_or_default();
        (lo.index(), hi.index())
    };
    let resize = med(rounds.iter().map(|r| r.resize.as_nanos()).collect());
    let feed = med(rounds.iter().map(|r| r.feed.as_nanos()).collect());
    let null = med(rounds.iter().map(|r| r.null.as_nanos()).collect());
    let together = med(rounds
        .iter()
        .map(|r| (r.resize + r.feed).as_nanos())
        .collect());
    let n = steps.len().max(1) as u128;
    let bytes: usize = steps.iter().map(|(_, b)| b.len()).sum();
    let (cmin, cmax) = span(&rounds.iter().map(|r| r.cells).collect::<Vec<_>>());
    let (lmin, lmax) = span(&rounds.iter().map(|r| r.lines).collect::<Vec<_>>());
    let wide = rounds[0].wide.index();
    let in_output = rounds
        .iter()
        .map(|r| r.wide_in_output)
        .max()
        .unwrap_or_default()
        .index();
    println!(
        "{path}: {} steps, {bytes} bytes, {wide} width changes, {} settled rounds",
        steps.len(),
        rounds.len()
    );
    println!(
        "  a gesture: resize {:.2} ms, feed {:.2} ms, together {:.2} ms (null {:.2} ms)",
        resize as f64 / 1e6,
        feed as f64 / 1e6,
        together as f64 / 1e6,
        null as f64 / 1e6
    );
    println!(
        "  a step:    resize {:6.2} us, feed {:6.2} us, together {:6.2} us (null {:5.2} us)",
        resize as f64 / n as f64 / 1e3,
        feed as f64 / n as f64 / 1e3,
        together as f64 / n as f64 / 1e3,
        null as f64 / n as f64 / 1e3
    );
    // The parser's cells, and only those. `cells_written` is bumped by
    // `feed`, the write paths, and `Term::resize`'s own blank-fill of the
    // rows a *height* growth reveals -- and by nothing in `View::reflow`,
    // which lives in `baud-domain` and cannot see the counter. So a
    // column around `resize` would not separate a re-cut from a no-op: it
    // would report the blanking instead. What separates them is the
    // control below. The span is across the settled rounds, so a round
    // that did different work from its neighbours is visible rather than
    // averaged away.
    println!("  cells:     parser {cmin}-{cmax} over the gesture, {lmin}-{lmax} lines retained");
    println!(
        "  zone:      {} of the {wide} width changes are in Zone::Output, where a re-cut runs",
        in_output
    );

    // **The control, because a resize that costs 20 ns looks exactly like
    // a resize that did not happen.** A width change re-cuts the primary
    // only in `Zone::Output`; between a prompt mark and the output that
    // follows it the shell will redraw those rows itself, so the core
    // leaves them (see `Zone` in `baud-core`). Here the same call is
    // timed in each zone, on the screen the gesture left -- the replay
    // resizes as well as feeds, so the terminal ends at the geometry the
    // gesture ends at -- and looped the way the gesture is, first round
    // discarded, so the figure is a median rather than one cold sample.
    let mut owned = Owned::with_geometry(&max, &geom);
    let mut t = Term::new(owned.storage(), first);
    for (ws, bytes) in steps {
        let r = t.resize(ws);
        assert_eq!(r, Resized::Ok, "{path}: the control clamped a step");
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }
    let (c, r) = t.geometry();
    let narrow = WinSize {
        ws_col: u16::try_from(c.index().saturating_sub(1)).unwrap_or(1),
        ws_row: u16::try_from(r.index()).unwrap_or(1),
    };
    let wide_ws = WinSize {
        ws_col: u16::try_from(c.index()).unwrap_or(1),
        ws_row: u16::try_from(r.index()).unwrap_or(1),
    };
    // The zone each arm is timed in, asserted rather than assumed: the
    // terminal is already in `Zone::Prompt` when the gesture ends, so an
    // `A` mark that did nothing at all would leave the prompt arm
    // reading the same number.
    let mut one = |mark: &[u8], to: &WinSize, want: Zone| -> u128 {
        t.write_at(BytePos::ZERO, mark);
        t.feed(BytePos::ZERO, ReadLen::from_index(mark.len()));
        assert_eq!(t.zone(), want, "{path}: the control's zone");
        let a = Instant::now();
        let got = t.resize(to);
        let took = a.elapsed().as_nanos();
        assert_eq!(got, Resized::Ok, "{path}: the control clamped");
        took
    };
    let (mut prompt, mut output) = (Vec::new(), Vec::new());
    for _ in 0..reps.max(2) {
        prompt.push(one(b"\x1b]133;C\x07\x1b]133;A\x07", &narrow, Zone::Prompt));
        let _ = one(b"", &wide_ws, Zone::Prompt);
        output.push(one(b"\x1b]133;C\x07", &narrow, Zone::Output));
        let _ = one(b"", &wide_ws, Zone::Output);
    }
    prompt.remove(0);
    output.remove(0);
    let bounds = |mut v: Vec<u128>| {
        v.sort_unstable();
        (v[v.len() / 2], v[0], v[v.len() - 1])
    };
    let (pm, plo, phi) = bounds(prompt);
    let settled = output.len();
    let (om, olo, ohi) = bounds(output);
    println!(
        "  control:   one width change costs {pm} [{plo}-{phi}] ns in Zone::Prompt (no re-cut) \
         and {om} [{olo}-{ohi}] ns in Zone::Output (the re-cut), on the screen the gesture \
         left, median of {settled} settled rounds"
    );
}

fn main() {
    let mut argv = baud_bench::args();
    let path = argv
        .next()
        .expect("usage: winch <prefix> [cost [reps]] | winch <captured.json>");
    let mode = argv.next().unwrap_or_default();
    let reps: usize = argv.next().and_then(|s| s.parse().ok()).unwrap_or(20);
    let steps = load(&path);
    if mode == "cost" {
        cost(&path, &steps, reps);
        return;
    }
    let all: Vec<u8> = steps.iter().flat_map(|(_, b)| b.iter().copied()).collect();
    let sig = signature(&all);
    match &sig {
        Some(s) => println!("prompt signature from the recording: {s:?}"),
        None => println!("no OSC 133 mark in the recording: prompts are not counted"),
    }
    let geom = baud_bench::host_geometry();
    let max = WinSize {
        ws_col: steps
            .iter()
            .map(|(w, _)| w.ws_col)
            .max()
            .unwrap_or(0)
            .max(400),
        ws_row: steps
            .iter()
            .map(|(w, _)| w.ws_row)
            .max()
            .unwrap_or(0)
            .max(150),
    };
    let owned: &'static mut Owned = Box::leak(Box::new(Owned::with_geometry(&max, &geom)));
    let first = &steps.first().expect("a first step").0;
    let mut t = Term::new(owned.storage(), first);
    let mut last = 0usize;
    let mut broken = 0usize;
    for (i, (ws, bytes)) in steps.iter().enumerate() {
        let before = t.prompt_row().map(|p| (p.index(), t.cursor().0.index()));
        let res = t.resize(ws);
        let after = t.prompt_row().map(|p| (p.index(), t.cursor().0.index()));
        let (c, r) = t.geometry();
        if let Resized::Truncated { .. } = res {
            println!(
                "  !! step {i}: {}x{} was truncated to {}x{}",
                ws.ws_col,
                ws.ws_row,
                c.index(),
                r.index()
            );
        }
        match (before, after) {
            (Some((p, cy)), Some((p2, cy2))) if cy.wrapping_sub(p) != cy2.wrapping_sub(p2) => {
                broken += 1;
                println!(
                    "  !! step {i} to {}x{}: prompt-to-cursor distance {} -> {} with nothing fed \
                     (prompt {p}->{p2}, cursor {cy}->{cy2})",
                    ws.ws_col,
                    ws.ws_row,
                    cy.wrapping_sub(p),
                    cy2.wrapping_sub(p2)
                );
                dump(&t);
            }
            (Some((p, cy)), None) if cy - p < r.index() => {
                broken += 1;
                println!(
                    "  !! step {i} to {}x{}: the prompt's first row left the screen with nothing fed \
                     (prompt {p}, cursor {cy}, now cursor {:?})",
                    ws.ws_col,
                    ws.ws_row,
                    t.cursor()
                );
                dump(&t);
            }
            _ => {}
        }
        // A step holding a redraw mark is fed in two: up to the end of
        // the mark's string, then the rest, with the terminal's state
        // printed between -- where the mark left the cursor and the block,
        // and what it did to the history and the hidden rows. The core is
        // chunk-invariant, so the split changes nothing it renders.
        let mark = bytes
            .windows(7)
            .position(|w| w == b"\x1b]133;P")
            .and_then(|at| {
                bytes[at..]
                    .iter()
                    .position(|b| *b == 0x07)
                    .map(|end| at + end + 1)
            });
        let (head, tail) = match mark {
            Some(cut) => (&bytes[..cut], &bytes[cut..]),
            None => (&bytes[..], &bytes[bytes.len()..]),
        };
        if !head.is_empty() {
            let (h0, s0) = (t.scrollback().len(), t.hidden().index());
            t.write_at(BytePos::ZERO, head);
            t.feed(BytePos::ZERO, ReadLen::from_index(head.len()));
            if mark.is_some() {
                println!(
                    "  mark: prompt {:>3}  cursor {:?}  hist {h0}->{}  hidden {s0}->{}  erases {}",
                    t.prompt_row()
                        .map_or("-".to_string(), |p| p.index().to_string()),
                    t.cursor(),
                    t.scrollback().len(),
                    t.hidden().index(),
                    t.prompt_erases
                );
            }
        }
        if !tail.is_empty() {
            t.write_at(BytePos::ZERO, tail);
            t.feed(BytePos::ZERO, ReadLen::from_index(tail.len()));
        }
        let on = copies(&t, sig.as_deref());
        println!(
            "step {i:4} {:3}x{:<3} fed {:5}  prompt {:>8}  cursor {:?}  copies {}  hist {} hidden {} erases {}",
            c.index(),
            r.index(),
            bytes.len(),
            t.prompt_row()
                .map_or("-".to_string(), |p| p.index().to_string()),
            t.cursor(),
            on.map_or("-".to_string(), |n| n.to_string()),
            t.scrollback().len(),
            t.hidden().index(),
            t.prompt_erases
        );
        if let Some(on) = on {
            if on > last {
                println!("  === screen after the step that added a copy ===");
                dump(&t);
                println!(
                    "  === bytes: {:?}",
                    String::from_utf8_lossy(&bytes[..bytes.len().min(200)])
                        .replace('\u{1b}', "<E>")
                );
            }
            last = on;
        }
    }
    {
        let sb = t.scrollback();
        println!("--- history, newest first ({} lines) ---", sb.len());
        for n in 0..sb.len().min(6) {
            if let Some((c, _)) = sb.line(Back::of(n)) {
                println!(
                    "  -{n}| {}",
                    text(c).trim_end().chars().take(72).collect::<String>()
                );
            }
        }
    }
    println!("--- final screen ---");
    dump(&t);
    let sb = t.scrollback();
    let retained = sig.as_deref().map(|s| {
        (0..sb.len())
            .filter(|n| {
                sb.line(Back::of(*n))
                    .is_some_and(|(c, _)| text(c).trim_start().starts_with(s))
            })
            .count()
    });
    println!(
        "prompt copies: {} on screen, {} in history; {broken} bare resizes broke the contract",
        copies(&t, sig.as_deref()).map_or("-".to_string(), |n| n.to_string()),
        retained.map_or("-".to_string(), |n| n.to_string())
    );
}
