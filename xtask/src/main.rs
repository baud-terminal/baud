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
use xtask::page_text::{self, Options};
use xtask::publish::{self, Flags};
use xtask::scan_audit::{self, Limit};
use xtask::sources;
use xtask::third_party::{self, Mode, Outcome};
use xtask::{
    changelog, citations, credits, features, features_since, fnv, forbidden, loop_head, no_python,
    pdf_search, pmc, test_scope, text, vt_table,
};

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

fn outcome(o: Outcome) -> ExitCode {
    match o {
        Outcome::Passed => ExitCode::SUCCESS,
        Outcome::Failed => ExitCode::FAILURE,
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
        ["sources"] => {
            let o = sources::run(root, &mut stdout).map_err(|e| e.to_string())?;
            Ok(outcome(o))
        }
        ["citations", rest @ ..] => {
            let o = match rest {
                [] => citations::run(root, citations::Mode::Write, &mut stdout),
                ["--check"] => citations::run(root, citations::Mode::Check, &mut stdout),
                ["--merge", fragments @ ..] if !fragments.is_empty() => {
                    citations::merge(root, fragments, &mut stdout)
                }
                _ => return Err(format!("not an option: {}", rest.join(" "))),
            }
            .map_err(|e| e.to_string())?;
            Ok(outcome(o))
        }
        ["no-python"] => {
            let o = no_python::run(root, &mut stdout).map_err(|e| e.to_string())?;
            Ok(outcome(o))
        }
        ["features", rest @ ..] if rest.len() <= 1 => {
            let mode = match rest {
                [] => features::Mode::Write,
                ["--check"] => features::Mode::Check,
                _ => return Err(format!("not an option: {}", rest.join(" "))),
            };
            let o =
                features::run(root, mode, &mut io::stderr().lock()).map_err(|e| e.to_string())?;
            Ok(outcome(o))
        }
        ["features-since", rest @ ..] => {
            let exit = features_since::run(rest, &mut stdout).map_err(|e| e.to_string())?;
            Ok(ExitCode::from(exit.0))
        }
        ["test-scope", rest @ ..] => {
            let exit = test_scope::run(rest, &mut stdout).map_err(|e| e.to_string())?;
            Ok(ExitCode::from(exit))
        }
        ["third-party", rest @ ..] if rest.len() <= 1 => {
            let mode = match rest {
                [] => Mode::Write,
                ["--check"] => Mode::Check,
                _ => return Err(format!("not an option: {}", rest.join(" "))),
            };
            let o = third_party::run(root, mode, &mut stdout).map_err(|e| e.to_string())?;
            Ok(outcome(o))
        }
        ["credits", rest @ ..] if rest.len() <= 1 => {
            let mode = match rest {
                [] => credits::Mode::Write,
                ["--check"] => credits::Mode::Check,
                ["--list"] => credits::Mode::List,
                ["--crates"] => credits::Mode::Crates,
                _ => return Err(format!("not an option: {}", rest.join(" "))),
            };
            let o = credits::run(root, mode, &mut stdout).map_err(|e| e.to_string())?;
            Ok(outcome(o))
        }
        // Every argument but the five flags is ignored, as the script
        // ignored it, and what stopped the run is printed as the script's
        // `SystemExit` printed it: the message alone.
        ["publish", rest @ ..] => {
            let root = fs::canonicalize(root).map_err(|e| format!("the repository: {e}"))?;
            let cargo = publish::Cargo::from_env(&root);
            match publish::run(&root, Flags::parse(rest), &cargo, &mut stdout) {
                Ok(exit) => Ok(exit.code()),
                Err(e) => {
                    eprintln!("{e}");
                    Ok(ExitCode::FAILURE)
                }
            }
        }
        ["page-text", rest @ ..] => {
            let o = Options::parse(rest)?;
            let worst = page_text::run(root, &o, &mut stdout)?;
            Ok(ExitCode::from(worst))
        }
        ["pdf-search", rest @ ..] => {
            let exit = pdf_search::run(root, rest, &mut stdout, &mut io::stderr().lock())
                .map_err(|e| e.to_string())?;
            Ok(ExitCode::from(exit.0))
        }
        ["loop-head", rest @ ..] => {
            let exit = loop_head::run(rest, "objdump", &mut stdout, &mut io::stderr().lock())
                .map_err(|e| e.to_string())?;
            Ok(ExitCode::from(exit.0))
        }
        ["vt-table", rest @ ..] => {
            let exit = vt_table::run(rest, &mut stdout, &mut io::stderr().lock())
                .map_err(|e| e.to_string())?;
            Ok(ExitCode::from(exit.0))
        }
        ["pmc", rest @ ..] => {
            let exit =
                pmc::run(rest, &mut stdout, &mut io::stderr().lock()).map_err(|e| e.to_string())?;
            Ok(ExitCode::from(exit.0))
        }
        ["forbidden", rest @ ..] => {
            let exit = forbidden::run(rest, &mut stdout, &mut io::stderr().lock())
                .map_err(|e| e.to_string())?;
            Ok(ExitCode::from(exit.0))
        }
        _ => {
            eprintln!(
                "usage: cargo xtask audit | impl-audit <file> | scan-audit [limit] \
                 | changelog <context> | features [--check] | features-since <rev>|--explain \
                 | fnv1a <src> <lib> <sdig> <ldig> \
                 | sources | no-python | citations [--check|--merge <tsv>...] \
                 | third-party [--check] | credits [--check|--list|--crates] | publish [--size|--verify|--release\
                 |--write-notices|--write-licences] | page-text [--key K] [--from DIR] [--save DIR] \
                 [--print] [--self-test] | pdf-search <key> [<term>...] \
                 | loop-head [--fn NAME] [--no-pageoff] A [B] | vt-table <results> [<baseline>] \
                 | pmc <export.xml> | forbidden <patterns> <controls> <file>..."
            );
            Ok(ExitCode::from(2))
        }
    }
}
