//! `baud-pictures brand [--check] [--root DIR]`,
//! `baud-pictures logo [--ufo] [--check | --out DIR [--mp4]] [--keep DIR]`,
//! `baud-pictures geometry <walls.png> <out dir>` and
//! `baud-pictures plasma <geometry dir> <dark|light> <on_ms> <off_ms> <out>`:
//! see the crate's docs.

use baud_pictures::{brand, geometry, logo, plasma, ufo, zlib};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "baud-pictures: usage: baud-pictures brand [--check] [--root DIR]\n\
         \x20      baud-pictures logo [--ufo] [--check | --out DIR [--mp4]] [--keep DIR]\n\
         \x20          (--mp4 needs --out: the MP4s are not committed, and in the tree they\n\
         \x20           would make the demo's --record refuse it as dirty)\n\
         \x20      baud-pictures geometry <walls.png> <out dir>\n\
         \x20      baud-pictures plasma <geometry dir> <dark|light> <on_ms> <off_ms> <out>"
    );
    ExitCode::FAILURE
}

/// The zlib linked is the pinned one.
fn zlib_pinned(cmd: &str) -> bool {
    let have = zlib::version();
    if have != zlib::ZLIB {
        eprintln!(
            "baud-pictures {cmd}: the zlib linked is {have}, not the pinned {}",
            zlib::ZLIB
        );
        return false;
    }
    true
}

fn brand(args: &[String]) -> ExitCode {
    let mut root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let (mut check, mut it) = (false, args.iter());
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check" => check = true,
            "--root" => match it.next() {
                Some(d) => root = PathBuf::from(d),
                None => return usage(),
            },
            _ => return usage(),
        }
    }
    if !zlib_pinned("brand") {
        return ExitCode::FAILURE;
    }
    let bad = match brand::run(&root, check) {
        Ok(bad) => bad,
        Err(msg) => {
            eprintln!("baud-pictures brand: {msg}");
            return ExitCode::FAILURE;
        }
    };
    for line in &bad {
        println!("FAIL {line}");
    }
    if !bad.is_empty() {
        return ExitCode::FAILURE;
    }
    if check {
        println!("ok: every logo file is the generator's, and both pictures meet GitHub's limits");
    } else {
        println!("ok");
    }
    ExitCode::SUCCESS
}

fn logo(args: &[String]) -> ExitCode {
    let (which, args) = ufo::asked(args);
    let (movie, args) = logo::mp4_asked(&args);
    let (files, keep) = match logo::args(&args) {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("baud-pictures logo: {msg}");
            return ExitCode::FAILURE;
        }
    };
    let made = match which {
        ufo::Which::Plasma => logo::main(&logo::root(), &files, keep.as_deref(), movie),
        ufo::Which::Ufo => ufo::main(&logo::root(), &files, keep.as_deref(), movie),
    };
    let bad = match made {
        Ok(bad) => bad,
        Err(msg) => {
            eprintln!("baud-pictures logo: {msg}");
            return ExitCode::FAILURE;
        }
    };
    for line in &bad {
        println!("FAIL {line}");
    }
    if !bad.is_empty() {
        return ExitCode::FAILURE;
    }
    if files == logo::Files::Check {
        println!("ok: the logos are the film's, byte for byte");
    } else {
        println!("ok");
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("brand") => brand(&args[1..]),
        Some("logo") => logo(&args[1..]),
        Some("geometry") => {
            if args.len() != 3 {
                return usage();
            }
            if !zlib_pinned("geometry") {
                return ExitCode::FAILURE;
            }
            match geometry::run(Path::new(&args[1]), Path::new(&args[2])) {
                Ok(()) => ExitCode::SUCCESS,
                Err(msg) => {
                    eprintln!("{msg}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("plasma") => match plasma::run(&args[1..]) {
            Ok(report) => {
                print!("{report}");
                ExitCode::SUCCESS
            }
            Err(plasma::Stop::Exit(msg)) => {
                eprintln!("{msg}");
                ExitCode::FAILURE
            }
            Err(plasma::Stop::Assert(msg)) => {
                eprintln!("AssertionError: {msg}");
                ExitCode::FAILURE
            }
        },
        _ => usage(),
    }
}
