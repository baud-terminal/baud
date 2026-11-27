//! Capture a shell's output across a sequence of window sizes: the
//! `capture-resize` example.
//!
//! `capture-tui` sets `TIOCSWINSZ` once and records raw bytes with no frame
//! boundaries, which is right for a recording of a program drawing and wrong
//! for a recording of a *resize*: what a resize test needs is the bytes
//! **and** the size in force when each of them arrived.
//!
//! So this re-issues `TIOCSWINSZ` mid-loop and cuts a step at each change,
//! writing two files:
//!
//! ```text
//! <name>.raw     the shell's bytes, exactly as `tools/tests/recordings/`
//!                stores them -- no framing, no header, no timestamps
//! <name>.steps   one `COLSxROWS <bytelen>` per line: the size in force
//!                and how many bytes of the `.raw` belong to it
//! ```
//!
//! Two files rather than one JSON blob on purpose. `baud-bench`'s `winch`
//! parses JSON with a hand-rolled reader whose fields all fall back to zero,
//! so a malformed capture replays as a `0x0` resize with no bytes instead of
//! failing; a fixture that gates the suite has to fail loudly. The pair is
//! also diffable, and it keeps the bytes in the shape the rest of the corpus
//! is already in.
//!
//! The sizes come from a file of `COLSxROWS` lines, which is what
//! `BAUD_DEBUG_RESIZE=1` already prints -- so a real session's gesture can be
//! replayed as it was. Several sizes on one line are a burst -- applied with
//! nothing drained between them, the shape the app produces -- and a line
//! reading `enter` presses Enter:
//!
//! ```text
//! grep '^resize asked' baud-resize.log | awk '{print $5}' > drag.sizes
//! capture-resize --sizes drag.sizes --out tests/resize/drag -- /bin/zsh -i
//! ```
//!
//! **A burst is the shape the app produces and a per-size drain cannot.**
//! AppKit delivers several `setFrameSize:` callbacks between two display-link
//! ticks; each is a `TIOCSWINSZ` with nothing fed, and the shell's redraws
//! for all of them land together afterwards -- the first computed for a width
//! the screen has already left. `COLSxROWS COLSxROWS ...` on one line is
//! that: every size set back to back, then one drain, recorded as zero-byte
//! steps and one step with the bytes. `enter` writes a carriage return and
//! drains, so a fixture can carry what a user does between resizes.
//!
//! `--answer QUERY=REPLY` answers a query the shell writes, as
//! `capture-tui`'s does (`super::answers`): fish draws no prompt until its
//! `DA1` is answered. The answers are written to the shell, not the `.raw`,
//! which holds only what the shell wrote.

use std::ffi::OsString;

use super::answers::Answers;
use super::args::{Conv, Need, Opt, Parser, Rest, Takes};
use super::pty::{Child, Environ, Lookup, Spawn, clock};
use super::py;

const PARSER: Parser = Parser {
    prog: "capture-resize",
    usage: "usage: capture-resize [-h] --sizes SIZES --out OUT [--settle SETTLE]
                      [--step STEP] [--tail TAIL] [--gap GAP]
                      [--answer ANSWER]
                      ...
",
    help: "
positional arguments:
  cmd

options:
  -h, --help       show this help message and exit
  --sizes SIZES    file of COLSxROWS lines
  --out OUT        output prefix: writes .raw and .steps
  --settle SETTLE  seconds before the first resize
  --step STEP      seconds a step gives the shell
  --tail TAIL      seconds after the last resize
  --gap GAP        seconds between the sizes of a burst, with nothing drained:
                   a display-link tick apart, so the shell redraws once per
                   size and every redraw but the last is computed for a width
                   the screen has left. Zero coalesces them into one redraw.
  --answer ANSWER  QUERY=REPLY: answer QUERY with REPLY each time the shell
                   writes it (repeatable)
",
    opts: &[
        Opt {
            flag: "--sizes",
            takes: Takes::One(Conv::Text),
            need: Need::Required,
        },
        Opt {
            flag: "--out",
            takes: Takes::One(Conv::Text),
            need: Need::Required,
        },
        Opt {
            flag: "--settle",
            takes: Takes::One(Conv::Float),
            need: Need::Optional,
        },
        Opt {
            flag: "--step",
            takes: Takes::One(Conv::Float),
            need: Need::Optional,
        },
        Opt {
            flag: "--tail",
            takes: Takes::One(Conv::Float),
            need: Need::Optional,
        },
        Opt {
            flag: "--gap",
            takes: Takes::One(Conv::Float),
            need: Need::Optional,
        },
        Opt {
            flag: "--answer",
            takes: Takes::Append,
            need: Need::Optional,
        },
    ],
    rest: Rest::Command,
};

/// A window size: columns and rows.
pub type Size = (i64, i64);

/// One line of the sizes file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Line {
    /// A burst of sizes applied with nothing drained between them.
    Sizes(Vec<Size>),
    /// The key the child is sent.
    Enter,
}

/// The lines of a sizes file named `path`, or what the script exited with.
pub fn parse_sizes(path: &str, text: &str) -> Result<Vec<Line>, String> {
    let mut lines = Vec::new();
    for (i, raw) in crate::key_latency::py::universal_lines(text)
        .into_iter()
        .enumerate()
    {
        let n = i + 1;
        let line = py::strip(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "enter" {
            lines.push(Line::Enter);
            continue;
        }
        let mut burst = Vec::new();
        for word in line.split(py::is_space).filter(|w| !w.is_empty()) {
            let Some((c, r)) = word.split_once('x') else {
                return Err(format!(
                    "{path}:{n}: expected COLSxROWS or `enter`, got {}",
                    py::str_repr(line)
                ));
            };
            match (py::int(c), py::int(r)) {
                (Some(c), Some(r)) => burst.push((c, r)),
                _ => {
                    return Err(format!(
                        "{path}:{n}: expected COLSxROWS, got {}",
                        py::str_repr(word)
                    ));
                }
            }
        }
        lines.push(Line::Sizes(burst));
    }
    if !lines.iter().any(|l| matches!(l, Line::Sizes(_))) {
        return Err(format!("{path}: no sizes, so there is nothing to capture"));
    }
    Ok(lines)
}

/// Everything the child writes within `seconds`.
///
/// Not "until it goes quiet": the point of a resize capture is what the shell
/// managed to emit in the time a gesture gave it, so the window is fixed and
/// a shell that is still redrawing when it closes is the case under test
/// rather than a capture to retry.
fn drain(child: &Child, seconds: f64, answers: &mut Answers) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let end = clock() + seconds;
    while clock() < end {
        if !child.readable(0.004)? {
            continue;
        }
        match child.read(65536) {
            Ok(chunk) if !chunk.is_empty() => {
                for (_, reply) in answers.owed(&chunk)? {
                    child.write(&reply)?;
                }
                out.extend_from_slice(&chunk);
            }
            _ => break,
        }
    }
    Ok(out)
}

/// The example.
pub fn run(args: &[OsString]) -> i32 {
    let parsed = match PARSER.parse(args) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let cmd = parsed.command();
    if cmd.is_empty() {
        return super::died("give a command after --, e.g. -- /bin/zsh -i");
    }
    match capture(&parsed, &cmd) {
        Ok(()) => 0,
        Err(e) => super::died(&e),
    }
}

/// Everything past the command line.
fn capture(parsed: &super::args::Parsed, cmd: &[OsString]) -> Result<(), String> {
    let sizes = parsed.one("--sizes").expect("a required option");
    let bytes = std::fs::read(sizes).map_err(|e| format!("open: {e}"))?;
    let text = String::from_utf8(bytes).map_err(|e| format!("'utf-8' codec: {e}"))?;
    let lines = parse_sizes(&sizes.to_string_lossy(), &text)?;
    let first = lines
        .iter()
        .find_map(|l| match l {
            Line::Sizes(b) => b.first().copied(),
            Line::Enter => None,
        })
        .expect("parse_sizes found a size");

    let mut answers = Answers::parse(&parsed.all("--answer"))?;
    let mut env = Environ::current();
    env.set_default("TERM", "xterm-256color");
    let spawn = Spawn::new(Lookup::Path, cmd, None, &env, 1)?;
    let child = Child::fork(&spawn)?;

    let mut steps: Vec<(Size, Vec<u8>)> = Vec::new();
    child.set_size(first.0, first.1, 0, 0)?;
    // The shell's own startup, at the first size: a fixture that began
    // mid-prompt would be a recording of a terminal nobody has.
    steps.push((
        first,
        drain(&child, parsed.float("--settle", 2.5), &mut answers)?,
    ));
    let step = parsed.float("--step", 0.05);
    let gap = parsed.float("--gap", 0.02);
    let mut at = first;
    for line in &lines {
        let burst = match line {
            Line::Enter => {
                child.write(b"\r")?;
                steps.push((at, drain(&child, step, &mut answers)?));
                continue;
            }
            Line::Sizes(burst) => burst,
        };
        // Every size of the burst before any drain: the sizes but the last
        // are steps with no bytes, which is what the app's trace records.
        let (last, before) = burst.split_last().expect("a burst of at least one");
        for size in before {
            child.set_size(size.0, size.1, 0, 0)?;
            steps.push((*size, Vec::new()));
            py::sleep(gap)?;
        }
        at = *last;
        child.set_size(at.0, at.1, 0, 0)?;
        steps.push((at, drain(&child, step, &mut answers)?));
    }
    steps.push((
        at,
        drain(&child, parsed.float("--tail", 2.0), &mut answers)?,
    ));
    child.stop()?;
    child.close();

    let prefix = parsed.one("--out").expect("a required option");
    let raw = super::with_suffix(prefix, ".raw");
    let stp = super::with_suffix(prefix, ".steps");
    let all: Vec<u8> = steps.iter().flat_map(|(_, b)| b.iter().copied()).collect();
    std::fs::write(&raw, &all).map_err(|e| format!("open: {e}"))?;
    let mut text = String::from("# COLSxROWS <bytes of the .raw that arrived at that size>\n");
    for ((cols, rows), b) in &steps {
        text.push_str(&format!("{cols}x{rows} {}\n", b.len()));
    }
    std::fs::write(&stp, text).map_err(|e| format!("open: {e}"))?;
    let (total, n) = (all.len(), steps.len());
    super::say(
        &[
            super::raw(&raw),
            format!(": {total} bytes over {n} steps").as_bytes(),
        ]
        .concat(),
    );
    super::say(&[super::raw(&stp), format!(": {n} steps").as_bytes()].concat());
    Ok(())
}
