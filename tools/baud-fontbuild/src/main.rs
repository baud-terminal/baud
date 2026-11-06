//! `baud-fontbuild compare A.ttf B.ttf`: exit 0 when the fonts are
//! equal, 1 when they differ (each difference on its own line), 2 when
//! either cannot be read.
//!
//! `baud-fontbuild stage BASE.ttf SOURCE.txt OUT.ttf`: one style's Hebrew
//! stage, the source merged into the stock face (exit 1 on a source it
//! refuses, 2 on a file it cannot read or write).
//!
//! `baud-fontbuild extract BASE.ttf STAGE.ttf STYLE OUT.txt`: the source
//! of a stage font, written only when it builds that font again.
//!
//! `baud-fontbuild merge BUILT.ttf NOTO.ttf STYLE EPOCH OUT.ttf`: Noto
//! Sans Devanagari merged into one built face (exit 1 when the merge
//! refuses, BUILT's objects having changed among the reasons).
//!
//! `baud-fontbuild instance VF.ttf WGHT EPOCH OUT.ttf`: the variable font
//! with `wght` pinned and its other axes kept (exit 1 when the bake
//! refuses it).

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage: baud-fontbuild compare A.ttf B.ttf\n       \
         baud-fontbuild stage BASE.ttf SOURCE.txt OUT.ttf\n       \
         baud-fontbuild extract BASE.ttf STAGE.ttf STYLE OUT.txt\n       \
         baud-fontbuild instance VF.ttf WGHT EPOCH OUT.ttf\n       \
         baud-fontbuild merge BUILT.ttf NOTO.ttf STYLE EPOCH OUT.ttf"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let read = |p: &str| std::fs::read(p).map_err(|e| eprintln!("{p}: {e}"));
    let write = |p: &str, d: &[u8]| match std::fs::write(p, d) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{p}: {e}");
            ExitCode::from(2)
        }
    };
    match args.as_slice() {
        [cmd, base, source, out] if cmd == "stage" => {
            let (Ok(base), Ok(source)) = (read(base), read(source)) else {
                return ExitCode::from(2);
            };
            let Ok(source) = String::from_utf8(source) else {
                eprintln!("the source is not UTF-8");
                return ExitCode::from(2);
            };
            return match baud_fontbuild::stage::build(&base, &source) {
                Ok(font) => write(out, &font),
                Err(e) => {
                    eprintln!("stage: {e}");
                    ExitCode::from(1)
                }
            };
        }
        [cmd, built, noto, style, epoch, out] if cmd == "merge" => {
            let (Ok(built), Ok(noto)) = (read(built), read(noto)) else {
                return ExitCode::from(2);
            };
            let Ok(epoch) = epoch.parse::<i64>() else {
                eprintln!("merge: EPOCH is an integer");
                return ExitCode::from(2);
            };
            return match baud_fontbuild::merge::merge(&built, &noto, style, epoch) {
                Ok(m) => {
                    println!(
                        "merged {style} {} glyphs, {} code points",
                        m.kept, m.code_points
                    );
                    write(out, &m.font)
                }
                Err(e) => {
                    eprintln!("merge: {e}");
                    ExitCode::from(1)
                }
            };
        }
        [cmd, vf, wght, epoch, out] if cmd == "instance" => {
            let Ok(vf) = read(vf) else {
                return ExitCode::from(2);
            };
            let (Ok(wght), Ok(epoch)) = (wght.parse::<i32>(), epoch.parse::<i64>()) else {
                eprintln!("instance: WGHT and EPOCH are integers");
                return ExitCode::from(2);
            };
            return match baud_fontbuild::instance::instance(&vf, wght, epoch) {
                Ok(font) => write(out, &font),
                Err(e) => {
                    eprintln!("instance: refused: {e}");
                    ExitCode::from(1)
                }
            };
        }
        [cmd, base, stage, style, out] if cmd == "extract" => {
            let (Ok(base), Ok(stage)) = (read(base), read(stage)) else {
                return ExitCode::from(2);
            };
            return match baud_fontbuild::stage::extract(&base, &stage, style) {
                Ok(text) => write(out, text.as_bytes()),
                Err(e) => {
                    eprintln!("extract: {e}");
                    ExitCode::from(1)
                }
            };
        }
        _ => {}
    }
    let [cmd, a, b] = args.as_slice() else {
        return usage();
    };
    if cmd != "compare" {
        return usage();
    }
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
