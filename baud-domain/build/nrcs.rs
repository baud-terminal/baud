//! The National Replacement Character Sets, `OUT_DIR/nrcs_data.rs`.
//!
//! The sets replace a handful of ASCII positions per language. None of the
//! DEC texts read gives a Unicode codepoint for them, and xterm's
//! `charsets.h` does (`[[source: xterm-charsets]]`), as `MAP`/`UNI`
//! macros: a `UNI` line carries the codepoint, and a `MAP` line names an X
//! keysym, resolved here by its `#define` in `[[source: xorg-keysymdef]]`.
//! By the value, not by the line's `/* U+ */` comment, because a `MAP`
//! line may carry none: Swedish 4/0, `MAP(0x40, XK_Eacute)`, has no
//! comment and is resolved all the same. Those entries are xterm's judgement
//! and are cited as xterm's.
//!
//! Two typed lists stand between xterm's macros and baud's tables:
//!
//! * [`SKIP`], lines the source has and baud does not emit yet, each with
//!   the question that decides it;
//! * [`DEC_OVERRIDES`], positions xterm's table lacks, typed from the DEC
//!   text that draws them -- that text is `external`, so nothing can
//!   extract it here -- each with its citation, which goes into the
//!   table's doc.
//!
//! Every check runs before anything is emitted: a line of a wanted macro
//! that does not parse, a keysym with no `#define` or outside Latin-1, a
//! `/* U+ */` comment that is not its keysym's value, two entries for one
//! byte, a skip or an override the source no longer bears out, and a set
//! whose emitted count is not its macro's line count adjusted by the two
//! lists. Each set is held to DEC's own tables by `tests/nrcs_dec.rs`.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use crate::xterm::{self, Cur, Digits};
use baud_tablegen::{Check, Emit, Errors, Input, Kind, Source};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

pub const KEYSYMS: Input = Input {
    file: "xorg-keysymdef.h",
    key: "xorg-keysymdef",
    header: "/***********************************************************",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 2] = [xterm::CHARSETS, KEYSYMS];

/// xterm's macro name, after `map_NRCS_`, and baud's `Nrcs` variant, in
/// the order the tables are written.
pub const WANTED: [(&str, &str); 14] = [
    ("Dutch", "Dutch"),
    ("Finnish", "Finnish"),
    ("French", "French"),
    ("French_Canadian", "FrenchCanadian"),
    ("German", "German"),
    ("Italian", "Italian"),
    ("Norwegian_Danish", "NorwegianDanish"),
    ("Portuguese", "Portuguese"),
    ("Spanish", "Spanish"),
    ("Swedish", "Swedish"),
    ("Swiss", "Swiss"),
    ("Greek", "Greek"),
    ("Hebrew", "Hebrew"),
    ("Turkish", "Turkish"),
];

/// A line of a wanted macro that baud does not emit: it must be an
/// `XXX(byte, UNDEF)` line, so a skip cannot swallow a position xterm
/// maps.
pub struct Skip {
    pub set: &'static str,
    pub byte: u8,
    pub why: &'static str,
}

/// `charsets.h:2380-2381`, marked "unused". xterm draws them as U+2426.
pub const SKIP: [Skip; 2] = [
    Skip {
        set: "Greek",
        byte: 0x79,
        why: "awaits Q-UNDEF",
    },
    Skip {
        set: "Greek",
        byte: 0x7A,
        why: "awaits Q-UNDEF",
    },
];

/// A position xterm's macro lacks: its codepoint, and the citation, as the
/// doc lines it is emitted as. The byte must be absent from the macro, so
/// a source that gains it fails here.
pub struct Override {
    pub set: &'static str,
    pub byte: u8,
    pub cp: u32,
    pub cite: &'static [&'static str],
}

pub const DEC_OVERRIDES: [Override; 1] = [Override {
    set: "Turkish",
    byte: 0x21,
    cp: 0x0131,
    cite: &[
        "`[[source: dec-pplv2-pm]]` Figure A-26, which draws a",
        "dotless small i at 2/1 (note AF §2.4)",
    ],
}];

/// Xorg's own spelling of a keysym the national sets use, the control
/// that the file read is a keysymdef.h: Swedish 4/0's `XK_Eacute`.
pub const EACUTE: (&str, u32) = ("XK_Eacute", 0x00C9);

/// The lines of a macro besides its entries.
const STRUCTURE: [&str; 3] = ["", "switch (code) {", "}"];

/// `#define XK_name value`, where the value is a number or, for an alias,
/// another keysym's name; a later `#define` of a name stands.
fn keysyms(src: &Source) -> HashMap<&str, &str> {
    let mut out = HashMap::new();
    for line in src.text().lines() {
        let mut c = Cur::new(line);
        if !c.lit("#define ") {
            continue;
        }
        let Some(name) = c.word("XK_") else {
            continue;
        };
        let before = c.rest.len();
        c.rest = c.rest.trim_start_matches([' ', '\t']);
        if c.rest.len() == before {
            continue;
        }
        let value = if let Some(hex) = c.rest.strip_prefix("0x") {
            let n = hex.chars().take_while(char::is_ascii_hexdigit).count();
            let next = hex[n..].chars().next();
            if n == 0 || next.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                continue;
            }
            &c.rest[..2 + n]
        } else if let Some(alias) = c.word("XK_") {
            alias
        } else {
            continue;
        };
        out.insert(name, value);
    }
    out
}

/// A keysym's value, through its aliases.
fn keysym(table: &HashMap<&str, &str>, name: &str) -> Option<u32> {
    let mut seen: Vec<&str> = Vec::new();
    let mut name = name;
    while let Some(&value) = table.get(name) {
        if seen.contains(&name) {
            return None;
        }
        seen.push(name);
        match value.strip_prefix("0x") {
            Some(hex) => return u32::from_str_radix(hex, 16).ok(),
            None => name = value,
        }
    }
    None
}

/// One entry of a national macro.
enum Entry<'a> {
    /// `UNI(byte, codepoint)`.
    Uni(u8, u32),
    /// `MAP(byte, keysym)` and the comment after it, if any.
    Map(u8, &'a str, Option<&'a str>),
    /// `XXX(byte, UNDEF)`.
    Undef(u8),
}

fn entry(text: &str) -> Option<Entry<'_>> {
    let mut c = Cur::new(text);
    let byte = |c: &mut Cur<'_>| {
        c.hex(2, 2, Digits::Either)
            .and_then(|b| u8::try_from(b).ok())
    };
    if c.lit("UNI(") {
        let b = byte(&mut c)?;
        c.lit(",").then_some(())?;
        c.ws();
        let cp = c.hex(4, 6, Digits::Either)?;
        c.lit(")").then_some(())?;
        c.end()?;
        Some(Entry::Uni(b, cp))
    } else if c.lit("MAP(") {
        let b = byte(&mut c)?;
        c.lit(",").then_some(())?;
        c.ws();
        let name = c.word("XK_")?;
        c.lit(")").then_some(())?;
        Some(Entry::Map(b, name, c.end()?))
    } else if c.lit("XXX(") {
        let b = byte(&mut c)?;
        c.lit(",").then_some(())?;
        c.ws();
        c.lit("UNDEF)").then_some(())?;
        c.end()?;
        Some(Entry::Undef(b))
    } else {
        None
    }
}

/// The first `U+` and four to six hexadecimal digits in `text`.
fn said(text: &str) -> Option<u32> {
    text.match_indices("U+").find_map(|(i, _)| {
        let hex = &text[i + 2..];
        let n = hex
            .chars()
            .take(6)
            .take_while(char::is_ascii_hexdigit)
            .count();
        (n >= 4).then(|| u32::from_str_radix(&hex[..n], 16).ok())?
    })
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let charsets = errs.take(xterm::load(read));
    let keys_src = errs.take(Source::load(&KEYSYMS, read));
    let (Some(charsets), Some(keys_src)) = (charsets, keys_src) else {
        return Err(errs);
    };
    let path = charsets.path();
    let keys = keysyms(&keys_src);
    let (xk, want) = EACUTE;
    errs.require(Check::from(keysym(&keys, xk) == Some(want)), || {
        keys_src.error(
            Kind::Control,
            format!(
                "{xk} does not resolve to 0x{want:X}: is this the pinned file [xorg-keysymdef]?"
            ),
        )
    });

    let mut sets: Vec<(&str, Vec<(u8, u32)>)> = Vec::new();
    let mut used_skips: HashSet<(&str, u8)> = HashSet::new();
    let mut used_overrides: HashSet<(&str, u8)> = HashSet::new();
    for (name, _) in WANTED {
        let bodies = xterm::bodies(&charsets, &format!("map_NRCS_{name}"));
        let body = match bodies.as_slice() {
            [] => {
                errs.push(charsets.error(
                    Kind::Malformed,
                    format!("map_NRCS_{name}: no definition with a body"),
                ));
                continue;
            }
            [(_, body)] => body,
            [_, (n, _), ..] => {
                errs.push(charsets.error(
                    Kind::Duplicate,
                    format!("map_NRCS_{name}: two definitions with a body, the second at line {n}"),
                ));
                continue;
            }
        };
        let mut pairs: Vec<(u8, u32)> = Vec::new();
        let mut unemitted: Vec<String> = Vec::new();
        let mut source = 0usize;
        for line in body {
            let text = line.text();
            if STRUCTURE.contains(&text) {
                continue;
            }
            if !xterm::has_entry(text) {
                errs.push(line.error(
                    path,
                    Kind::Malformed,
                    "neither an entry nor the switch around them",
                ));
                continue;
            }
            source += 1;
            let at = |b: u8| pairs.iter().any(|(k, _)| *k == b);
            match entry(text) {
                Some(Entry::Uni(b, cp)) => {
                    if at(b) {
                        errs.push(line.error(
                            path,
                            Kind::Duplicate,
                            format!("{name}: two entries for 0x{b:02x}"),
                        ));
                    }
                    pairs.push((b, cp));
                }
                Some(Entry::Map(b, xk, comment)) => {
                    let Some(value) = keysym(&keys, xk) else {
                        errs.push(line.error(
                            path,
                            Kind::UnknownValue,
                            format!("{xk} has no #define in xorg-keysymdef.h"),
                        ));
                        continue;
                    };
                    // A keysym is its codepoint only in Latin-1; past it the
                    // two numberings part, and this would need a mapping it
                    // does not have.
                    if !((0x20..=0x7E).contains(&value) || (0xA0..=0xFF).contains(&value)) {
                        errs.push(line.error(
                            path,
                            Kind::Invariant,
                            format!("{xk} is {value:#x}, outside Latin-1"),
                        ));
                        continue;
                    }
                    if let Some(s) = comment.and_then(said) {
                        if s != value {
                            errs.push(line.error(
                                path,
                                Kind::Control,
                                format!("{xk} is U+{value:04X}, its comment says U+{s:04X}"),
                            ));
                            continue;
                        }
                    }
                    if at(b) {
                        errs.push(line.error(
                            path,
                            Kind::Duplicate,
                            format!("{name}: two entries for 0x{b:02x}"),
                        ));
                    }
                    pairs.push((b, value));
                }
                Some(Entry::Undef(b)) => {
                    // `UNDEF` is a choice nobody has made for baud: the line
                    // is emitted only once an answer gives it a codepoint,
                    // and until then it is skipped by name or the count
                    // below fails.
                    if SKIP.iter().any(|s| s.set == name && s.byte == b) {
                        used_skips.insert((name, b));
                    } else {
                        unemitted.push(format!("charsets.h:{} XXX(0x{b:02x}, UNDEF)", line.n));
                    }
                }
                None => {
                    errs.push(line.error(path, Kind::Malformed, "an entry the build cannot read"))
                }
            }
        }
        for o in DEC_OVERRIDES.iter().filter(|o| o.set == name) {
            if pairs.iter().any(|(b, _)| *b == o.byte) {
                errs.push(charsets.error(
                    Kind::Invariant,
                    format!(
                        "{name} 0x{:02x}: xterm's macro has it now; retire the DEC override",
                        o.byte
                    ),
                ));
                continue;
            }
            used_overrides.insert((name, o.byte));
            pairs.push((o.byte, o.cp));
        }
        let skipped = used_skips.iter().filter(|k| k.0 == name).count();
        let added = used_overrides.iter().filter(|k| k.0 == name).count();
        if pairs.len() + skipped != source + added {
            let not = if unemitted.is_empty() {
                "none".to_string()
            } else {
                unemitted.join(", ")
            };
            errs.push(charsets.error(
                Kind::Count,
                format!(
                    "{name}: {} emitted for {source} lines in map_NRCS_{name}, {skipped} skipped and {added} added; not emitted: {not}",
                    pairs.len()
                ),
            ));
        }
        pairs.sort_unstable();
        sets.push((name, pairs));
    }
    for s in SKIP
        .iter()
        .filter(|s| !used_skips.contains(&(s.set, s.byte)))
    {
        errs.push(charsets.error(
            Kind::Invariant,
            format!(
                "skip {} 0x{:02x}: no `XXX(..., UNDEF)` line there any more",
                s.set, s.byte
            ),
        ));
    }
    for o in DEC_OVERRIDES
        .iter()
        .filter(|o| !used_overrides.contains(&(o.set, o.byte)))
    {
        errs.push(charsets.error(
            Kind::Invariant,
            format!("DEC override {} 0x{:02x}: not applied", o.set, o.byte),
        ));
    }
    if !errs.is_empty() {
        return Err(errs);
    }

    let mut out = Emit::new(
        "nrcs_data",
        "baud-domain/build.rs (build/nrcs.rs)",
        &[&charsets, &keys_src],
    );
    let mut rs = String::from("use crate::Nrcs;\n");
    for ((name, variant), (_, pairs)) in WANTED.iter().zip(&sets) {
        rs.push('\n');
        let overrides: Vec<&Override> = DEC_OVERRIDES.iter().filter(|o| o.set == *name).collect();
        if overrides.is_empty() {
            let _ = writeln!(rs, "/// `[[source: xterm-charsets]]`'s `map_NRCS_{name}`.");
        } else {
            let _ = writeln!(
                rs,
                "/// `[[source: xterm-charsets]]`'s `map_NRCS_{name}`, and what it lacks,"
            );
            let _ = writeln!(rs, "/// typed from the DEC text that draws it:");
            for o in overrides {
                let _ = writeln!(rs, "///");
                for (i, c) in o.cite.iter().enumerate() {
                    let lead = if i == 0 {
                        format!("* `0x{:02x}`, U+{:04X}: ", o.byte, o.cp)
                    } else {
                        "  ".to_string()
                    };
                    let end = if i + 1 == o.cite.len() { "." } else { "" };
                    let _ = writeln!(rs, "/// {lead}{c}{end}");
                }
            }
        }
        let skips: Vec<&Skip> = SKIP.iter().filter(|s| s.set == *name).collect();
        if !skips.is_empty() {
            rs.push_str("///\n/// Not emitted, each until its question is answered:\n///\n");
            for s in skips {
                let _ = writeln!(
                    rs,
                    "/// * `0x{:02x}`, `XXX(..., UNDEF)` in the macro: {} (note AF).",
                    s.byte, s.why
                );
            }
        }
        let ident = variant.to_ascii_uppercase();
        let _ = writeln!(rs, "static {ident}: [(u8, u32); {}] = [", pairs.len());
        for (b, cp) in pairs {
            let _ = writeln!(rs, "    (0x{b:02x}, 0x{cp:04x}),");
        }
        rs.push_str("];\n");
    }
    rs.push_str(
        "\n/// The replacement table for a set: the positions it replaces, by byte.\n\
         pub const fn nrcs_table(set: Nrcs) -> &'static [(u8, u32)] {\n    match set {\n",
    );
    for (_, variant) in WANTED {
        let _ = writeln!(
            rs,
            "        Nrcs::{variant} => &{},",
            variant.to_ascii_uppercase()
        );
    }
    rs.push_str("    }\n}\n");
    out.code(&rs);
    Ok(out)
}
