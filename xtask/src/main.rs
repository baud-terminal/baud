//! `cargo xtask <command>`: see the library's documentation for the list.
//! Paths are read from the directory it is run in, which the shells that
//! call it set to the repository's root.

use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::ExitCode;

use xtask::audit::{self, Audit, Failed};
use xtask::impl_audit::Counts;
use xtask::scan_audit::{self, Limit};
use xtask::{changelog, fnv, text};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&words) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

/// 0 on a pass, 1 on a failure, as the scripts exited.
fn verdict(failed: Failed) -> ExitCode {
    match failed {
        Failed::No => ExitCode::SUCCESS,
        Failed::Yes => ExitCode::FAILURE,
    }
}

fn run(args: &[&str]) -> Result<ExitCode, String> {
    let root = Path::new(".");
    let mut stdout = io::stdout().lock();
    match args {
        ["audit"] => {
            let cross = audit::Cargo {
                root: root.to_path_buf(),
            };
            let failed = Audit::new(root, &mut stdout)
                .run(&cross)
                .map_err(|e| e.to_string())?;
            Ok(verdict(failed))
        }
        ["impl-audit", file] => {
            let src = text::read_universal(Path::new(file)).map_err(|e| format!("{file}: {e}"))?;
            print!("{}", Counts::of(&src).table());
            Ok(ExitCode::SUCCESS)
        }
        ["scan-audit", rest @ ..] if rest.len() <= 1 => {
            let limit = match rest.first() {
                Some(n) => Limit(n.parse().map_err(|_| format!("not a limit: {n}"))?),
                None => audit::SCAN_LIMIT,
            };
            let src = text::read_universal(Path::new(scan_audit::PATH))
                .map_err(|e| format!("{}: {e}", scan_audit::PATH))?;
            let (report, ok) = scan_audit::report(&scan_audit::scans(&src), limit);
            print!("{report}");
            Ok(ExitCode::from(u8::from(!ok)))
        }
        ["changelog", context] => {
            let mut log = String::new();
            io::stdin()
                .read_to_string(&mut log)
                .map_err(|e| format!("the log: {e}"))?;
            let plz = fs::read_to_string("release-plz.toml")
                .map_err(|e| format!("release-plz.toml: {e}"))?;
            let cliff = fs::read_to_string(context).map_err(|e| format!("{context}: {e}"))?;
            let (report, ok) = changelog::check(&log, &plz, &cliff)?.report();
            print!("{report}");
            Ok(ExitCode::from(u8::from(!ok)))
        }
        ["fnv1a", src, lib, sdig, ldig] => {
            let digest = |p: &str| {
                fs::read(p)
                    .map(|b| fnv::fnv1a(&b))
                    .map_err(|e| format!("{p}: {e}"))
            };
            let (s, l) = (digest(src)?, digest(lib)?);
            fs::write(sdig, format!("{s}\n")).map_err(|e| format!("{sdig}: {e}"))?;
            fs::write(ldig, format!("{l}\n")).map_err(|e| format!("{ldig}: {e}"))?;
            println!("shaders.metal fnv-1a {s}, baud.metallib {l}");
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            eprintln!(
                "usage: cargo xtask audit | impl-audit <file> | scan-audit [limit] \
                 | changelog <context> | fnv1a <src> <lib> <sdig> <ldig>"
            );
            Ok(ExitCode::from(2))
        }
    }
}
