//! The six 96-character sets `ESC - . /` designate, each the right half
//! of an ISO 8859 part, `OUT_DIR/iso96_data.rs`.
//!
//! `[[source: ctlseqs]]` lists the finals: `A` ISO Latin-1 Supplemental,
//! `B` Latin-2, `F` Greek, `H` Hebrew, `L` Latin-Cyrillic and `M` Latin-5.
//! Each final is the one ISO-IR registers for the set (100, 101, 126, 138,
//! 144 and 148), and each set is read here from Unicode's mapping file for
//! its ISO 8859 part:
//!
//! * `A` `[[source: iso8859-1]]`, `B` `[[source: iso8859-2]]`, `L`
//!   `[[source: iso8859-5]]` and `M` `[[source: iso8859-9]]`, whose version
//!   histories change no character mapping.
//! * `F` `[[source: iso8859-7]]`, which maps ISO 8859-7:2003 and says its
//!   2.0 version added "the three newly added characters 0xA4, 0xA5,
//!   0xAA". ISO-IR 126, the set `F` names, is ISO 8859-7:1987 ("ISO
//!   8859-7:1987 is equivalent to ISO-IR-126, ELOT 928, and ECMA 118"), so
//!   those three are unassigned here.
//! * `H` `[[source: iso8859-8]]`, which maps ISO/IEC 8859-8:1999 and says
//!   its 1.1 version added "mappings for LRM and RLM", at 0xFD and 0xFE.
//!   ISO-IR 138 is the 1988 set, so those two are unassigned here; 0xAF
//!   keeps the file's U+00AF, the 1.1 version's correction.
//!
//! Each table is 96 codepoints indexed by `code - 0x20`, where `code` is
//! the 7-bit code: 2/0 through 7/15 in GL, and the same positions less
//! 0x80 in GR. An unassigned position holds DEC's error character:
//! `[[source: dec-std-070]]` 070-3 §3.4.3 calls 10/0 "an unassigned
//! graphic character code" that "will cause the error character (usually
//! imaged as a REVERSE QUESTION MARK) to be displayed on conforming
//! devices".
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use crate::xterm::{Cur, Digits, Line};
use baud_tablegen::{Emit, Errors, Input, Kind, Source};
use std::collections::HashMap;
use std::fmt::Write as _;

/// One set: its Rust name, its final, its file, its ISO-IR number, the
/// positions the file maps that the registered set does not have, how
/// many positions it assigns, and its title.
pub struct Iso96 {
    pub ident: &'static str,
    pub final_byte: char,
    pub input: Input,
    pub ir: u16,
    pub drop: &'static [u8],
    pub assigned: usize,
    pub title: &'static str,
}

const fn input(file: &'static str, key: &'static str, header: &'static str) -> Input {
    Input { file, key, header }
}

pub const SETS: [Iso96; 6] = [
    Iso96 {
        ident: "LATIN_1",
        final_byte: 'A',
        input: input("iso8859-1.txt", "iso8859-1", "# 8859-1.TXT"),
        ir: 100,
        drop: &[],
        assigned: 96,
        title: "ISO Latin-1 Supplemental",
    },
    Iso96 {
        ident: "LATIN_2",
        final_byte: 'B',
        input: input("iso8859-2.txt", "iso8859-2", "# 8859-2.TXT"),
        ir: 101,
        drop: &[],
        assigned: 96,
        title: "ISO Latin-2 Supplemental",
    },
    Iso96 {
        ident: "GREEK",
        final_byte: 'F',
        input: input("iso8859-7.txt", "iso8859-7", "# 8859-7.TXT"),
        ir: 126,
        drop: &[0xA4, 0xA5, 0xAA],
        assigned: 90,
        title: "ISO Greek Supplemental",
    },
    Iso96 {
        ident: "HEBREW",
        final_byte: 'H',
        input: input("iso8859-8.txt", "iso8859-8", "# 8859-8.TXT"),
        ir: 138,
        drop: &[0xFD, 0xFE],
        assigned: 58,
        title: "ISO Hebrew Supplemental",
    },
    Iso96 {
        ident: "LATIN_CYRILLIC",
        final_byte: 'L',
        input: input("iso8859-5.txt", "iso8859-5", "# 8859-5.TXT"),
        ir: 144,
        drop: &[],
        assigned: 96,
        title: "ISO Latin-Cyrillic",
    },
    Iso96 {
        ident: "LATIN_5",
        final_byte: 'M',
        input: input("iso8859-9.txt", "iso8859-9", "# 8859-9.TXT"),
        ir: 148,
        drop: &[],
        assigned: 96,
        title: "ISO Latin-5 Supplemental",
    },
];

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 6] = [
    SETS[0].input,
    SETS[1].input,
    SETS[2].input,
    SETS[3].input,
    SETS[4].input,
    SETS[5].input,
];

/// A mapping file's right half, byte to codepoint and name: each line
/// starting `0x` is the byte, a tab, the codepoint, a tab and `#` and the
/// name.
fn right_half(src: &Source, errs: &mut Errors) -> HashMap<u8, (u32, String)> {
    let mut out = HashMap::new();
    for (i, line) in src.text().lines().enumerate() {
        if !line.starts_with("0x") {
            continue;
        }
        let row = Line {
            n: i + 1,
            raw: line,
        };
        let mut c = Cur::new(line);
        let read = (|| {
            let b = c.hex(2, 2, Digits::Either)?;
            c.lit("\t").then_some(())?;
            let cp = c.hex(4, 4, Digits::Either)?;
            c.lit("\t").then_some(())?;
            let name = c.rest.split('\t').next_back()?;
            Some((
                u8::try_from(b).ok()?,
                cp,
                name.trim_start_matches('#').trim(),
            ))
        })();
        let Some((b, cp, name)) = read else {
            errs.push(row.error(src.path(), Kind::Malformed, "a line the build cannot read"));
            continue;
        };
        if b < 0xA0 {
            continue;
        }
        if out.insert(b, (cp, name.to_string())).is_some() {
            errs.push(row.error(src.path(), Kind::Duplicate, format!("0x{b:02X} twice")));
        }
    }
    out
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let mut srcs = Vec::new();
    for set in &SETS {
        if let Some(s) = errs.take(Source::load(&set.input, read)) {
            srcs.push(s);
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    let mut rs = String::from(
        "use crate::{Codepoint, DEC_ERROR_CHARACTER};\n\n\
         /// DEC's error character, at an unassigned position.\n\
         const E: Codepoint = DEC_ERROR_CHARACTER;\n",
    );
    for (set, src) in SETS.iter().zip(&srcs) {
        let m = right_half(src, &mut errs);
        for b in set.drop {
            if !m.contains_key(b) {
                errs.push(src.error(
                    Kind::Control,
                    format!("0x{b:02X} is listed as added later but the file does not map it"),
                ));
            }
        }
        let (fin, ir, title, key) = (set.final_byte, set.ir, set.title, set.input.key);
        let _ = writeln!(
            rs,
            "\n/// {title}, final `{fin}`, ISO-IR {ir}: `[[source: {key}]]`'s right half."
        );
        if !set.drop.is_empty() {
            let list: Vec<String> = set.drop.iter().map(|b| format!("`0x{b:02x}`")).collect();
            let _ = writeln!(
                rs,
                "///\n/// Unassigned here and mapped by the file, which maps a later edition: {}.",
                list.join(", ")
            );
        }
        let _ = writeln!(rs, "pub(crate) static {}: [Codepoint; 96] = [", set.ident);
        let mut assigned = 0;
        for b in 0xA0..=0xFFu8 {
            let code = b - 0x80;
            match m.get(&b) {
                Some((cp, name)) if !set.drop.contains(&b) => {
                    let _ = writeln!(rs, "    Codepoint(0x{cp:04x}), // 0x{code:02x} {name}");
                    assigned += 1;
                }
                _ => {
                    let _ = writeln!(rs, "    E, // 0x{code:02x} unassigned");
                }
            }
        }
        rs.push_str("];\n");
        errs.count(src.path(), "positions assigned", assigned, set.assigned);
    }
    let mut out = Emit::new(
        "iso96_data",
        "crates/baud-domain/build.rs (build/iso96.rs)",
        &srcs.iter().collect::<Vec<_>>(),
    );
    out.code(&rs);
    errs.or(out)
}
