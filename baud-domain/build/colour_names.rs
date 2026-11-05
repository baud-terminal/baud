//! The two colour-name tables, `OUT_DIR/kitty_colour_names.rs` and
//! `OUT_DIR/x_colour_names.rs`, each a name and its colour.
//!
//! kitty looks a colour name up before any other form
//! (`[[source: kitty-colors-1e1486e]]` 1525-1526), in a table of 753
//! lower-cased names (`[[source: kitty-color-names-1e1486e]]`): X.Org's
//! rgb.txt at 749182c (`[[source: xorg-rgb-749182c]]`), each name
//! lower-cased, and the one name it lacks, `debianred` (215, 7, 81;
//! kitty's line 161), Debian's addition to its rgb.txt
//! (`[[source: debian-bug-243288]]`, `[[source: debian-xorg-rgb-7-7-26]]`
//! 478). kitty is GPL-3.0 and is not in this tree, so the table is built
//! from rgb.txt and the one name; `kitty_names_are_its_own_file`, run by
//! hand, holds it to kitty's own file.
//!
//! The X server's table is the names an `OSC 4`, `10`, `11` or `12`
//! resolves, as xterm's XParseColor sends a name to the server
//! (`[[source: xserver-oscolor-98aed9d]]`): X.Org's rgb.txt at 0d2caec
//! (`[[source: xorg-rgb-0d2caec]]`), 782 names, lower-cased. The server
//! ignores case and nothing else, and both tables are keyed the same way.
//!
//! Each is a two-level perfect hash, `baud_tablegen::perfect_hash`: FNV-1a
//! over the lower-cased name picks one of 256 buckets, the bucket's seed
//! picks one of 1024 slots, and the slot's name is compared. A name is
//! found by two hashes and one comparison, never a scan.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`kitty`] and [`x`] return.

use baud_tablegen::{Buckets, Emit, Error, Errors, Input, Kind, Pos, Slots, Source, perfect_hash};
use std::collections::HashMap;
use std::fmt::Write as _;

/// rgb.txt's first line, the same in both pins: the counts below, not the
/// header, tell the two apart.
const FIRST: &str = "255 250 250\t\tsnow";

pub const RGB_2008: Input = Input {
    file: "xorg-rgb-749182c.txt",
    key: "xorg-rgb-749182c",
    header: FIRST,
};

pub const RGB_2023: Input = Input {
    file: "xorg-rgb-0d2caec.txt",
    key: "xorg-rgb-0d2caec",
    header: FIRST,
};

/// The files, under `data/`, that [`kitty`] and [`x`] read.
pub const INPUTS: [Input; 2] = [RGB_2008, RGB_2023];

/// The one name kitty's table holds that rgb.txt at 749182c does not:
/// Debian's DebianRed, its debian/local/rgb.txt line 478.
const DEBIANRED: (&str, [u8; 3]) = ("debianred", [215, 7, 81]);

/// The names each table holds: rgb.txt at 749182c's 752 and `debianred`,
/// and rgb.txt at 0d2caec's 782.
const KITTY_NAMES: usize = 753;
const X_NAMES: usize = 782;

pub const BUCKETS: usize = 256;
pub const SLOTS: usize = 1024;

/// A name, lower-cased, and its colour.
pub struct Named {
    pub name: String,
    pub rgb: [u8; 3],
}

/// rgb.txt's names, lower-cased, each where it first occurs: a line is
/// three decimal channels and the name, separated by blanks, and a line
/// starting `!` is a comment. Two spellings of one name that differ only
/// in case, with one colour, are one name; a spelling given twice, or two
/// that differ only in case with two colours, are refused.
pub fn rgb_txt(src: &Source, errs: &mut Errors) -> Vec<Named> {
    let mut out: Vec<Named> = Vec::new();
    // Lower-cased name -> its index in `out`, its line and its spelling.
    let mut seen: HashMap<String, (usize, usize, String)> = HashMap::new();
    for (i, line) in src.text().lines().enumerate() {
        let n = i + 1;
        if line.is_empty() || line.starts_with('!') {
            continue;
        }
        let at = |col: usize, kind: Kind, message: String| {
            Error::new(kind, src.path(), message).at(Pos { line: n, col }, line)
        };
        if let Some(col) = line.chars().position(|c| !c.is_ascii()) {
            errs.push(at(col + 1, Kind::Malformed, "not ASCII".to_string()));
            continue;
        }
        // Three channels, then the rest of the line, its blanks around it
        // dropped and the blanks inside kept: `ghost white`.
        let mut rest = line;
        let mut rgb = [0u8; 3];
        let mut ok = true;
        for c in &mut rgb {
            let start = rest.len() - rest.trim_start().len();
            let field = rest.trim_start();
            let end = field.find(char::is_whitespace).unwrap_or(field.len());
            let col = line.len() - rest.len() + start + 1;
            if field.is_empty() {
                errs.push(at(
                    col,
                    Kind::FieldCount,
                    "a colour is three channels and a name".to_string(),
                ));
                ok = false;
                break;
            }
            match field[..end].parse::<u8>() {
                Ok(v) => *c = v,
                Err(_) => {
                    errs.push(at(
                        col,
                        Kind::Malformed,
                        format!("{:?} is not a channel, 0 to 255", &field[..end]),
                    ));
                    ok = false;
                    break;
                }
            }
            rest = &field[end..];
        }
        if !ok {
            continue;
        }
        let spelled = rest.trim();
        let col = line.len() - rest.trim_start().len() + 1;
        if spelled.is_empty() {
            errs.push(at(
                col,
                Kind::FieldCount,
                "a colour is three channels and a name".to_string(),
            ));
            continue;
        }
        let name = spelled.to_ascii_lowercase();
        match seen.get(&name) {
            Some((k, first, as_written)) => {
                if as_written == spelled {
                    errs.push(at(
                        col,
                        Kind::Duplicate,
                        format!("{spelled:?} is given already on line {first}"),
                    ));
                } else if out[*k].rgb != rgb {
                    errs.push(at(
                        col,
                        Kind::Duplicate,
                        format!(
                            "{spelled:?} is {as_written:?} on line {first} in another case, with another colour"
                        ),
                    ));
                }
            }
            None => {
                seen.insert(name.clone(), (out.len(), n, spelled.to_string()));
                out.push(Named { name, rgb });
            }
        }
    }
    out
}

/// kitty's table: rgb.txt at 749182c and `debianred`.
pub fn kitty(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let src = Source::load(&RGB_2008, read)?;
    let mut names = rgb_txt(&src, &mut errs);
    let (extra, rgb) = DEBIANRED;
    if names.iter().any(|n| n.name == extra) {
        errs.push(src.error(
            Kind::Duplicate,
            format!("{extra} is in rgb.txt already, and kitty's table adds it"),
        ));
    } else {
        names.push(Named {
            name: extra.to_string(),
            rgb,
        });
    }
    errs.count(
        src.path(),
        "colour names and debianred",
        names.len(),
        KITTY_NAMES,
    );
    let doc = [
        "kitty's colour names: X.Org's rgb.txt at 749182c, lower-cased, and",
        "`debianred`, Debian's addition to its rgb.txt, which kitty's table",
        "carries. A name is looked up by [`SEEDS`] then [`SLOTS`]: two hashes",
        "and one comparison.",
    ];
    finish(
        table("kitty_colour_names", &src, &names, &doc, &mut errs),
        errs,
    )
}

/// The X server's table: rgb.txt at 0d2caec.
pub fn x(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let src = Source::load(&RGB_2023, read)?;
    let names = rgb_txt(&src, &mut errs);
    errs.count(src.path(), "colour names", names.len(), X_NAMES);
    let doc = [
        "The X server's colour names: X.Org's rgb.txt at 0d2caec, lower-cased,",
        "the table the X.Org server answers XParseColor's LookupColor from. A",
        "name is looked up by [`SEEDS`] then [`SLOTS`]: two hashes and one",
        "comparison.",
    ];
    finish(table("x_colour_names", &src, &names, &doc, &mut errs), errs)
}

/// The table, unless anything failed: every failure, the hash's among
/// them.
fn finish(table: Result<Emit, Errors>, mut errs: Errors) -> Result<Emit, Errors> {
    match table {
        Ok(emit) => errs.or(emit),
        Err(e) => {
            errs.extend(e);
            Err(errs)
        }
    }
}

/// `SEEDS`, `SLOTS` and `POOL` for `names`: the pool holds the names in
/// slot order, and a slot is a name's offset and length in it and its
/// colour, or zeros where no name is.
fn table(
    module: &str,
    src: &Source,
    names: &[Named],
    doc: &[&str],
    errs: &mut Errors,
) -> Result<Emit, Errors> {
    // A slot holds a name's length in a u8 and its offset in a u16, so
    // both are checked before the hash, which more names than slots fail.
    let long = names.iter().find(|n| n.name.len() > usize::from(u8::MAX));
    if let Some(n) = long {
        errs.push(src.error(
            Kind::Invariant,
            format!("{:?} is longer than a slot's u8 length", n.name),
        ));
    }
    let bytes: usize = names.iter().map(|n| n.name.len()).sum();
    if bytes > usize::from(u16::MAX) {
        errs.push(src.error(
            Kind::Invariant,
            format!("the pool is {bytes} bytes, past a slot's u16 offset"),
        ));
    }
    let keys: Vec<&[u8]> = names.iter().map(|n| n.name.as_bytes()).collect();
    let hash = perfect_hash(src.path(), &keys, Buckets(BUCKETS), Slots(SLOTS))?;
    let mut pool = String::new();
    let mut slots: Vec<(usize, usize, [u8; 3])> = Vec::with_capacity(SLOTS);
    for slot in &hash.slots {
        match slot {
            None => slots.push((0, 0, [0; 3])),
            Some(k) => {
                let n = &names[*k];
                slots.push((pool.len(), n.name.len(), n.rgb));
                pool.push_str(&n.name);
            }
        }
    }
    let by = "baud-domain/build.rs (build/colour_names.rs)";
    let mut out = Emit::new(module, by, &[src]);
    out.code(&doc.iter().map(|l| format!("// {l}\n")).collect::<String>());
    out.array(
        "SEEDS",
        "u16",
        &[&format!(
            "The seed of each of the {BUCKETS} buckets the first hash picks."
        )],
        &hash.seeds,
        16,
        u16::to_string,
    );
    out.array(
        "SLOTS",
        "crate::colour_name::ColourSlot",
        &[
            "Each slot's name, as an offset and a length in [`POOL`], and its",
            "colour; a length of 0 is an empty slot.",
        ],
        &slots,
        1,
        |(at, len, [r, g, b])| {
            format!(
                "crate::colour_name::ColourSlot {{ at: {at}, len: {len}, rgb: [{r}, {g}, {b}] }}"
            )
        },
    );
    let mut rs = String::from("/// Every name, end to end.\npub static POOL: &[u8] = concat!(\n");
    for chunk in pool.as_bytes().chunks(64) {
        let _ = writeln!(rs, "    {:?},", String::from_utf8_lossy(chunk));
    }
    rs.push_str(")\n.as_bytes();\n");
    out.code(&rs);
    Ok(out)
}
