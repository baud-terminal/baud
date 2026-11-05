//! `baud-pictures brand [--check] [--root DIR]`: see the crate's docs.

use baud_pictures::{brand, zlib};
use std::path::PathBuf;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("baud-pictures: usage: baud-pictures brand [--check] [--root DIR]");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let (mut check, mut it) = (false, args.iter());
    if it.next().map(String::as_str) != Some("brand") {
        return usage();
    }
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
    let have = zlib::version();
    if have != zlib::ZLIB {
        eprintln!(
            "baud-pictures brand: the zlib linked is {have}, not the pinned {}",
            zlib::ZLIB
        );
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
