//! Record yazi walking down a directory of pictures and back, as a terminal
//! with Sixel graphics sees it: the `capture-yazi-walk` example.
//!
//! ```text
//! capture-yazi-walk --out yazi-walk.raw -- yazi
//! ```
//!
//! `tests/recordings/sixel/yazi-walk.raw` was recorded this way. yazi asks
//! what the terminal is before it draws a preview, and draws none for a
//! terminal that does not answer, so `capture-tui`, which answers nothing,
//! cannot record one. This answers yazi's two bursts of queries as a VT102
//! with Sixel graphics and a 17x34-pixel cell would, the answers being the
//! ones the recording it replaces was given:
//!
//! ```text
//! after the first DA1:  DCS > | baud ST, CSI 6 ; 34 ; 17 t,
//!                       OSC 11 ; rgb:1010/1010/1010 BEL, CSI ? 6 ; 4 c
//! after the second:     DCS 1 $ r 1 SP q ST, CSI ? 12 ; 2 $ y,
//!                       CSI ? 0 u, CSI ? 6 ; 4 c
//! ```
//!
//! Then it presses `j` six times and `k` six times, each key once the preview
//! before it has arrived whole (its Sixel's ST) and yazi has been quiet for
//! `--quiet` seconds, so every key draws its own preview: thirteen in all.
//! The window is `--rows` by `--cols`. The bytes are written exactly as the
//! pty returned them; nothing yazi sent is answered but those two bursts.

use std::ffi::OsString;

use super::args::{Conv, Need, Opt, Parser, Rest, Takes};
use super::pty::{Child, Environ, Lookup, Spawn, clock};
use super::py;

const DA1: &[u8] = b"\x1b[0c";
const REPLIES: [&[u8]; 2] = [
    b"\x1bP>|baud\x1b\\\x1b[6;34;17t\x1b]11;rgb:1010/1010/1010\x07\x1b[?6;4c",
    b"\x1bP1$r1 q\x1b\\\x1b[?12;2$y\x1b[?0u\x1b[?6;4c",
];
const KEYS: &[u8] = b"jjjjjjkkkkkk";
const SIXEL: &[u8] = b"\x1bP9;1q";
const ST: &[u8] = b"\x1b\\";

const PARSER: Parser = Parser {
    prog: "capture-yazi-walk",
    usage: "usage: capture-yazi-walk [-h] --out OUT [--rows ROWS] [--cols COLS]
                         [--quiet QUIET] [--limit LIMIT]
                         ...
",
    help: "
positional arguments:
  cmd

options:
  -h, --help     show this help message and exit
  --out OUT
  --rows ROWS
  --cols COLS
  --quiet QUIET
  --limit LIMIT  seconds before giving up
",
    opts: &[
        Opt {
            flag: "--out",
            takes: Takes::One(Conv::Text),
            need: Need::Required,
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
            flag: "--quiet",
            takes: Takes::One(Conv::Float),
            need: Need::Optional,
        },
        Opt {
            flag: "--limit",
            takes: Takes::One(Conv::Float),
            need: Need::Optional,
        },
    ],
    rest: Rest::Command,
};

/// How many Sixel images in `out` have arrived up to their ST.
pub fn finished_previews(out: &[u8]) -> usize {
    let (mut n, mut at) = (0, 0);
    while let Some(i) = py::find(&out[at..], SIXEL) {
        let begin = at + i;
        let Some(j) = py::find(&out[begin..], ST) else {
            break;
        };
        n += 1;
        at = begin + j + ST.len();
    }
    n
}

/// The example.
pub fn run(args: &[OsString]) -> i32 {
    let parsed = match PARSER.parse(args) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let cmd = parsed.command();
    if cmd.is_empty() {
        return super::died("give a command after --, e.g. -- yazi");
    }
    match capture(&parsed, &cmd) {
        Ok(code) => code,
        Err(e) => super::died(&e),
    }
}

/// Everything past the command line.
fn capture(parsed: &super::args::Parsed, cmd: &[OsString]) -> Result<i32, String> {
    let spawn = Spawn::new(Lookup::Path, cmd, None, &Environ::current(), 1)?;
    let child = Child::fork(&spawn)?;
    child.set_size(parsed.int("--cols", 117), parsed.int("--rows", 40), 0, 0)?;
    let quiet = parsed.float("--quiet", 0.5);
    let limit = parsed.float("--limit", 120.0);

    let mut out: Vec<u8> = Vec::new();
    let mut answered = 0;
    let mut keys = 0;
    let mut last = clock();
    let start = last;
    let mut log: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut gave_up = true;
    while clock() - start < limit {
        let ready = child.readable(0.02)?;
        if ready {
            match child.read(65536) {
                Ok(chunk) if !chunk.is_empty() => {
                    out.extend_from_slice(&chunk);
                    last = clock();
                }
                _ => {
                    gave_up = false;
                    break;
                }
            }
        }
        // A burst is answered once its DA1 has arrived: the queries before
        // it are the ones the answer covers.
        if answered < REPLIES.len() && py::count(&out, DA1) > answered {
            child.write(REPLIES[answered])?;
            log.push((out.len(), REPLIES[answered].to_vec()));
            answered += 1;
            continue;
        }
        if ready || clock() - last < quiet {
            continue;
        }
        let done = finished_previews(&out);
        if keys < KEYS.len() && done == keys + 1 {
            child.write(&KEYS[keys..=keys])?;
            log.push((out.len(), KEYS[keys..=keys].to_vec()));
            keys += 1;
        } else if keys == KEYS.len() && done == KEYS.len() + 1 {
            gave_up = false;
            break;
        }
    }
    if gave_up {
        eprintln!(
            "gave up after {} s: {} previews",
            py::repr_float(limit),
            finished_previews(&out)
        );
    }
    child.stop()?;
    child.close();
    let path = parsed.one("--out").expect("a required option");
    std::fs::write(path, &out).map_err(|e| format!("open: {e}"))?;
    for (at, sent) in &log {
        println!("{at}\t{}", py::bytes_repr(sent));
    }
    let previews = finished_previews(&out);
    let line = format!("{} bytes, {previews} previews -> ", out.len());
    super::say(&[line.as_bytes(), super::raw(path)].concat());
    Ok(i32::from(previews != KEYS.len() + 1))
}
