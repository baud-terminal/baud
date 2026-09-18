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

use baud_core::{Owned, PackedCell, Resized, Term};
use baud_domain::{Back, BytePos, ReadLen, Row, WinSize};

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

fn main() {
    let path = baud_bench::args()
        .next()
        .expect("usage: winch <prefix> | winch <captured.json>");
    let steps = load(&path);
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
