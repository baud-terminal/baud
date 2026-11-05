//! `baud-fontbuild compare A.ttf B.ttf`: exit 0 when the fonts are
//! equal, 1 when they differ (each difference on its own line), 2 when
//! either cannot be read.

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("usage: baud-fontbuild compare A.ttf B.ttf");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [cmd, a, b] = args.as_slice() else {
        return usage();
    };
    if cmd != "compare" {
        return usage();
    }
    let read = |p: &str| std::fs::read(p).map_err(|e| eprintln!("{p}: {e}"));
    let (Ok(da), Ok(db)) = (read(a), read(b)) else {
        return ExitCode::from(2);
    };
    match baud_fontbuild::compare(&da, &db) {
        Err(baud_fontbuild::NotAFont(e)) => {
            eprintln!("not a font: {e}");
            ExitCode::from(2)
        }
        Ok(report) if report.is_equal() => {
            println!("equal");
            ExitCode::SUCCESS
        }
        Ok(report) => {
            for d in &report.differences {
                println!("{d}");
            }
            ExitCode::from(1)
        }
    }
}
