//! Capture a real TUI's escape stream through a real PTY: the `capture-tui`
//! example.
//!
//! `script(1)` is not enough on its own. Backgrounded it has no stdin, so the
//! child sees EOF and exits immediately -- 152 bytes of setup and nothing
//! else -- and it has no window size to give, which a full-screen program
//! needs before it will draw anything. So the PTY is opened here and
//! `TIOCSWINSZ` is set on it explicitly.
//!
//! ```text
//! capture-tui --seconds 10 --out capture.raw -- btop
//! ```
//!
//! With `--steps`, each `read(2)` is also written as a `COLSxROWS <bytes>`
//! line, the format of `tools/tests/resize/`'s `.steps`, and each key sent as a
//! `# sent <key>` line before the first read after it, so a replay can feed
//! the bytes at the boundaries they arrived at and say which key each read
//! answered.
//!
//! With `--answer QUERY=REPLY`, as Python escape strings, each `QUERY` the
//! program writes is answered with `REPLY`, as a terminal answers it: fish
//! waits on its `DA1` before it draws a prompt. Give the replies baud's
//! core gives (`Term::reply`), so the capture is the session baud would
//! have had.
//!
//! `--keys` drives the program, an escape string split at `--key-sep`, or a
//! byte a key without one: a mouse report sent a byte a delay arrives in
//! pieces, and the program reads its ESC as the Escape key. Without keys a
//! capture of an editor is one static screen; with them the stream contains
//! the scrolling, and scrolling is where DECSTBM, IL/DL and the scroll region
//! actually get used. `--first-key` is the seconds before the first,
//! `--key-delay` when not given: a program that waits out its start-up
//! queries reads the keys sent before then in one burst.

use std::ffi::OsString;
use std::fs::File;
use std::io::Write;

use super::answers::Answers;
use super::args::{Conv, Need, Opt, Parser, Rest, Takes};
use super::pty::{Child, Environ, Lookup, Spawn, clock};
use super::py;

/// Python's `min(0.2, d)` and then `max(0.0, ...)`, so a NaN goes where it did.
fn wait_for(d: f64) -> f64 {
    let capped = if d < 0.2 { d } else { 0.2 };
    if capped > 0.0 { capped } else { 0.0 }
}

const PARSER: Parser = Parser {
    prog: "capture-tui",
    usage: "usage: capture-tui [-h] [--seconds SECONDS] [--rows ROWS] [--cols COLS]
                   --out OUT [--keys KEYS] [--key-delay KEY_DELAY]
                   [--key-sep KEY_SEP] [--first-key FIRST_KEY] [--steps STEPS]
                   [--answer ANSWER]
                   ...
",
    help: "
positional arguments:
  cmd

options:
  -h, --help            show this help message and exit
  --seconds SECONDS
  --rows ROWS
  --cols COLS
  --out OUT
  --keys KEYS
  --key-delay KEY_DELAY
  --key-sep KEY_SEP
  --first-key FIRST_KEY
  --steps STEPS
  --answer ANSWER
",
    opts: &[
        Opt {
            flag: "--seconds",
            takes: Takes::One(Conv::Float),
            need: Need::Optional,
        },
        Opt {
            flag: "--rows",
            takes: Takes::One(Conv::Int),
            need: Need::Optional,
        },
        Opt {
            flag: "--cols",
            takes: Takes::One(Conv::Int),
            need: Need::Optional,
        },
        Opt {
            flag: "--out",
            takes: Takes::One(Conv::Text),
            need: Need::Required,
        },
        Opt {
            flag: "--keys",
            takes: Takes::One(Conv::Text),
            need: Need::Optional,
        },
        Opt {
            flag: "--key-delay",
            takes: Takes::One(Conv::Float),
            need: Need::Optional,
        },
        Opt {
            flag: "--key-sep",
            takes: Takes::One(Conv::Text),
            need: Need::Optional,
        },
        Opt {
            flag: "--first-key",
            takes: Takes::One(Conv::Float),
            need: Need::Optional,
        },
        Opt {
            flag: "--steps",
            takes: Takes::One(Conv::Text),
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

/// `bytes.split(sep)`, the empty pieces dropped.
fn split_keys(raw: &[u8], sep: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = py::find(&raw[at..], sep) {
        out.push(raw[at..at + i].to_vec());
        at += i + sep.len();
    }
    out.push(raw[at..].to_vec());
    out.retain(|k| !k.is_empty());
    out
}

/// The keys `--keys` and `--key-sep` name.
pub fn keys_from(keys: &str, sep: &str) -> Result<Vec<Vec<u8>>, String> {
    let raw = py::unescape(keys)?;
    Ok(if sep.is_empty() {
        raw.iter().map(|b| vec![*b]).collect()
    } else {
        split_keys(&raw, sep.as_bytes())
    })
}

/// The example.
pub fn run(args: &[OsString]) -> i32 {
    let parsed = match PARSER.parse(args) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let cmd = parsed.command();
    if cmd.is_empty() {
        eprintln!("no command given");
        return 2;
    }
    let mut env = Environ::current();
    let term = env.get("TERM").map_or_else(
        || "xterm-256color".to_string(),
        |t| t.to_string_lossy().into_owned(),
    );
    env.set("TERM", &term);
    match capture(&parsed, &cmd, &env) {
        Ok(code) => code,
        Err(e) => super::died(&e),
    }
}

/// Everything past the command line. `Err` is the exception the script died
/// of, at the step it died at.
fn capture(parsed: &super::args::Parsed, cmd: &[OsString], env: &Environ) -> Result<i32, String> {
    let cols = parsed.int("--cols", 100);
    let rows = parsed.int("--rows", 30);
    let delay = parsed.float("--key-delay", 0.35);
    let out_path = parsed.one("--out").expect("a required option");
    let spawn = Spawn::new(Lookup::Path, cmd, None, env, 127)?;
    let child = Child::fork(&spawn)?;
    child.set_size(cols, rows, 0, 0)?;

    let keys_text = args_text(parsed.one("--keys"))?;
    let keys = keys_from(&keys_text, &parsed.text("--key-sep", ""))?;
    let first = parsed.float("--first-key", delay);
    let mut next_key = clock() + first;
    let mut ki = 0;

    let mut answers = Answers::parse(&parsed.all("--answer"))?;

    let mut end = clock() + parsed.float("--seconds", 10.0);
    let mut total = 0usize;
    let mut steps: Vec<String> = Vec::new();
    let mut f = File::create(out_path).map_err(|e| format!("open: {e}"))?;
    while clock() < end {
        if ki < keys.len() && clock() >= next_key {
            child.write(&keys[ki])?;
            steps.push(format!("# sent {}", py::bytes_repr(&keys[ki])));
            ki += 1;
            next_key = clock() + delay;
        }
        // Wake for the next key as well as for output, so a key is not sent
        // up to a whole timeout late.
        let wait = if ki >= keys.len() {
            0.2
        } else {
            wait_for(next_key - clock())
        };
        if !child.readable(wait)? {
            continue;
        }
        let chunk = child.read(65536).unwrap_or_default();
        if chunk.is_empty() {
            // The child closed the pty.
            end = f64::NEG_INFINITY;
            continue;
        }
        f.write_all(&chunk).map_err(|e| format!("write: {e}"))?;
        total += chunk.len();
        steps.push(format!("{cols}x{rows} {}", chunk.len()));
        for (q, r) in answers.owed(&chunk)? {
            child.write(&r)?;
            steps.push(format!(
                "# answered {} with {}",
                py::bytes_repr(&q),
                py::bytes_repr(&r)
            ));
        }
    }
    drop(f);

    child
        .kill(libc::SIGTERM)
        .map_err(|e| format!("kill: {e}"))?;
    py::sleep(0.3)?;
    match child.kill(libc::SIGKILL) {
        Err(e) if e.raw_os_error() != Some(libc::ESRCH) => return Err(format!("kill: {e}")),
        _ => {}
    }
    child.reap();
    child.close();

    if let Some(path) = parsed.one("--steps") {
        let mut text = String::from("# COLSxROWS <bytes of the .raw that one read returned>\n");
        for line in &steps {
            text.push_str(line);
            text.push('\n');
        }
        std::fs::write(path, text).map_err(|e| format!("open: {e}"))?;
    }
    super::say(
        &[
            format!("{total} bytes -> ").as_bytes(),
            super::raw(out_path),
        ]
        .concat(),
    );
    Ok(i32::from(total == 0))
}

/// An escape-string argument, which the script read through `str.encode`:
/// bytes that are not UTF-8 are its `UnicodeEncodeError`.
fn args_text(arg: Option<&OsString>) -> Result<String, String> {
    match arg {
        None => Ok(String::new()),
        Some(a) => a
            .to_str()
            .map(str::to_string)
            .ok_or_else(|| "'utf-8' codec can't encode: surrogates not allowed".to_string()),
    }
}
