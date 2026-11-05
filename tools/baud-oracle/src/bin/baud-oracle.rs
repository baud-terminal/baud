//! `baud-oracle xkb | raster | shape-helpers | cases`; see the crate's
//! docs.

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let utf8 = || -> Vec<String> {
        args[1..]
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    };
    match args.first().and_then(|a| a.to_str()) {
        Some("xkb") => baud_oracle::xkb::main(&utf8()),
        Some("raster") => baud_oracle::raster::main(&utf8()),
        Some("shape-helpers") => baud_oracle::shape::main(&args[1..]),
        Some("cases") => baud_oracle::cases::main(&utf8()),
        _ => {
            eprintln!("usage: baud-oracle xkb | raster | shape-helpers | cases ...");
            ExitCode::FAILURE
        }
    }
}
