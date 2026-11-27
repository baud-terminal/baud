//! Each terminal's cursor and screen after a replay in `tools/scripts/vt-compare.sh`'s
//! results, against a headless baud-core fed the same bytes:
//! `baud_bench::host::screen_check` has what is compared and why.
//!
//! ```text
//! vt-screens <results-file> <recording>...
//! ```
//!
//! One line a result: terminal, recording, verdict. Exits 1 when any
//! result is unequal or could not be read, so no rate is quoted from it.

use baud_bench::host::{self, screen_check};
use std::path::Path;

fn main() {
    let args = host::py_args();
    let [results, recs @ ..] = args.as_slice() else {
        eprintln!("usage: vt-screens <results-file> <recording>...");
        std::process::exit(2);
    };
    let mut recordings = Vec::new();
    for r in recs {
        let name = Path::new(r)
            .file_name()
            .map_or_else(|| r.clone(), |n| n.to_string_lossy().into_owned());
        match std::fs::read(r) {
            Ok(bytes) => recordings.push((name, bytes)),
            Err(e) => {
                eprintln!("{r}: {e}");
                std::process::exit(2);
            }
        }
    }
    let text = match std::fs::read_to_string(results) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{results}: {e}");
            std::process::exit(2);
        }
    };
    let mut clean = true;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match screen_check::judge_line(line, &recordings) {
            Ok((name, rec, verdict)) => {
                clean &= verdict.rated();
                println!("{name} {rec} {}", verdict.says());
            }
            Err(e) => {
                clean = false;
                println!("{e}");
            }
        }
    }
    std::process::exit(if clean { 0 } else { 1 });
}
