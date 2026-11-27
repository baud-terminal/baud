//! Write the file `tools/tests/recordings/utf8/vim-unicode.raw` shows vim paging:
//! the `gen-vim-unicode` example.
//!
//! ```text
//! gen-vim-unicode unicode.txt
//! ```
//!
//! 1600 lines, four kinds repeated for each of 400 line numbers: wide CJK,
//! emoji with regional-indicator flags and a ZWJ family, Hebrew with its points
//! beside Arabic and Devanagari (combining marks and right-to-left text), and a
//! 148-column line of ASCII with Cyrillic and Latin letters of ambiguous width.
//! No final newline, so vim reports `[noeol]`. Nothing is random: the same
//! command writes the same bytes.

use std::ffi::OsString;

const DOC: &str = "Write the file `tools/tests/recordings/utf8/vim-unicode.raw` shows vim paging.

    gen-vim-unicode unicode.txt

1600 lines, four kinds repeated for each of 400 line numbers: wide CJK,
emoji with regional-indicator flags and a ZWJ family, Hebrew with its points
beside Arabic and Devanagari (combining marks and right-to-left text), and a
148-column line of ASCII with Cyrillic and Latin letters of ambiguous width.
No final newline, so vim reports `[noeol]`. Nothing is random: the same
command writes the same bytes.
";

/// The file's bytes.
pub fn contents() -> Vec<u8> {
    let mixed = ["mixed ascii ЁЖЗ ünïcödé"; 6].join(" ");
    let mut lines = Vec::with_capacity(1600);
    for i in 0..400 {
        lines.push(format!(
            "{i:04} 世界へようこそ 안녕하세요 你好世界 — wide CJK at column {i}"
        ));
        lines.push(format!(
            "{i:04} 🚀🇯🇵👨\u{200d}👩\u{200d}👧 emoji, flags and a ZWJ family"
        ));
        lines.push(format!(
            "{i:04} \u{5e2}\u{5b4}\u{5d1}\u{5b0}\u{5e8}\u{5b4}\u{5d9}\u{5ea} \u{648}\u{627}\u{644}\u{639}\u{631}\u{628}\u{64a}\u{629} \u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947} combining marks and RTL"
        ));
        lines.push(format!("{i:04} {mixed}"));
    }
    lines.join("\n").into_bytes()
}

/// The example.
pub fn run(args: &[OsString]) -> i32 {
    let [out] = args else {
        eprintln!("{DOC}");
        return 2;
    };
    match std::fs::write(out, contents()) {
        Ok(()) => 0,
        Err(e) => super::died(&format!("open: {e}")),
    }
}
