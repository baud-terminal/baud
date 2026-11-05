//! The stage source as values, and its line format.
//!
//! One line is one record; a record's fields are separated by single
//! spaces, and a line that starts with `#` is a comment. Strings are
//! written between double quotes with `\"`, `\\` and `\u{..}` (any
//! character outside printable ASCII) as their only escapes. The records,
//! in the order they are written:
//!
//! ```text
//! style Regular
//! head.modified 2398377600
//! os2.ulUnicodeRange1 0xa00228ff
//! name 1 3 1 0x0409 "Proto Hebrew Mono C"
//! glyph uni05D0 advance 600 class 1 unicode 05D0
//! contour 52,0 52,98,off ...
//! instructions 00b0
//! component uni05E9 18,0 flags 0x0204
//! script GPOS hebr DFLT
//! feature GPOS 0 mark 0 1 5
//! markset uni05B0 uni05B1 ...
//! lookup 5 markbase flag 0x0000
//! subtable
//! mark uni05B0 0 268,-80
//! base uni05D0 300,-60 - 320,560
//! lookup 7 chain flag 0x0010 markset 3
//! subtable
//! backtrack
//! input [uni05D0 uni05D1] [uni05B8]
//! lookahead
//! apply 1 11 2 13
//! ```
//!
//! A point is `x,y` on the curve and `x,y,off` off it. An anchor is
//! `x,y`, and `-` is a class a base has no anchor for. A coverage is a
//! bracketed glyph list.

use std::fmt::Write as _;

/// One appended glyph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub name: String,
    pub advance: u16,
    /// GDEF's glyph class; 0 is none.
    pub class: u16,
    pub unicodes: Vec<u32>,
    pub outline: Outline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outline {
    Empty,
    Simple {
        contours: Vec<Vec<(i16, i16, bool)>>,
        instructions: Vec<u8>,
        overlap: bool,
    },
    Composite(Vec<Component>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub glyph: String,
    pub dx: i16,
    pub dy: i16,
    /// The component flags that are not packing: ROUND_XY_TO_GRID,
    /// USE_MY_METRICS, OVERLAP_COMPOUND and the two offset-scaling bits.
    pub flags: u16,
}

pub type Anchor = (i16, i16);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subtable {
    /// MarkBasePosFormat1, or MarkMarkPosFormat1 for a `markmark` lookup:
    /// each mark with its class and anchor, each base (or mark2) with one
    /// anchor per class.
    Attach {
        marks: Vec<(String, u16, Anchor)>,
        bases: Vec<(String, Vec<Option<Anchor>>)>,
    },
    /// ChainedSequenceContextFormat3.
    Chain {
        backtrack: Vec<Vec<String>>,
        input: Vec<Vec<String>>,
        lookahead: Vec<Vec<String>>,
        apply: Vec<(u16, u16)>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    MarkBase,
    MarkMark,
    Chain,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::MarkBase => "markbase",
            Kind::MarkMark => "markmark",
            Kind::Chain => "chain",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lookup {
    pub index: u16,
    pub kind: Kind,
    pub flag: u16,
    pub mark_set: Option<u16>,
    pub subtables: Vec<Subtable>,
}

/// A name record set (replaced or added).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub id: u16,
    pub platform: u16,
    pub encoding: u16,
    pub language: u16,
    pub text: String,
}

/// Everything the stage adds to or changes in the stock face.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Source {
    pub style: String,
    pub head_modified: i64,
    /// OS/2 fields by name, each set to its value.
    pub os2: Vec<(String, u32)>,
    pub names: Vec<Name>,
    pub glyphs: Vec<Glyph>,
    /// Scripts added to GSUB or GPOS: the table, the new tag and the tag
    /// of the stock script whose Script table it names.
    pub scripts: Vec<(String, String, String)>,
    /// Features whose lookup list is replaced: the table, the feature
    /// record's index, its tag and its new lookup indices.
    pub features: Vec<(String, u16, String, Vec<u16>)>,
    /// Mark glyph sets appended to GDEF's.
    pub mark_sets: Vec<Vec<String>>,
    /// Lookups appended to GPOS's.
    pub lookups: Vec<Lookup>,
}

/// The OS/2 fields a source may set.
pub const OS2_FIELDS: &[&str] = &[
    "ulUnicodeRange1",
    "ulUnicodeRange2",
    "ulUnicodeRange3",
    "ulUnicodeRange4",
    "ulCodePageRange1",
    "ulCodePageRange2",
];

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            _ => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
        }
    }
    out.push('"');
    out
}

fn list(glyphs: &[String]) -> String {
    format!("[{}]", glyphs.join(" "))
}

fn anchor(a: &Option<Anchor>) -> String {
    match a {
        Some((x, y)) => format!("{x},{y}"),
        None => "-".to_string(),
    }
}

impl Source {
    /// The source as text, `header` (comment lines without their `#`)
    /// first.
    pub fn write(&self, header: &str) -> String {
        let mut o = String::new();
        for l in header.lines() {
            if l.is_empty() {
                o.push_str("#\n");
            } else {
                let _ = writeln!(o, "# {l}");
            }
        }
        let _ = writeln!(o, "style {}", self.style);
        let _ = writeln!(o, "head.modified {}", self.head_modified);
        for (f, v) in &self.os2 {
            let _ = writeln!(o, "os2.{f} {v:#010x}");
        }
        for n in &self.names {
            let _ = writeln!(
                o,
                "name {} {} {} {:#06x} {}",
                n.id,
                n.platform,
                n.encoding,
                n.language,
                quote(&n.text)
            );
        }
        for g in &self.glyphs {
            let _ = write!(
                o,
                "glyph {} advance {} class {}",
                g.name, g.advance, g.class
            );
            if !g.unicodes.is_empty() {
                o.push_str(" unicode");
                for u in &g.unicodes {
                    let _ = write!(o, " {u:04X}");
                }
            }
            if let Outline::Simple { overlap: true, .. } = g.outline {
                o.push_str(" overlap");
            }
            o.push('\n');
            match &g.outline {
                Outline::Empty => {}
                Outline::Simple {
                    contours,
                    instructions,
                    ..
                } => {
                    for c in contours {
                        o.push_str("contour");
                        for (x, y, on) in c {
                            let _ = write!(o, " {x},{y}{}", if *on { "" } else { ",off" });
                        }
                        o.push('\n');
                    }
                    if !instructions.is_empty() {
                        o.push_str("instructions ");
                        for b in instructions {
                            let _ = write!(o, "{b:02x}");
                        }
                        o.push('\n');
                    }
                }
                Outline::Composite(cs) => {
                    for c in cs {
                        let _ = writeln!(
                            o,
                            "component {} {},{} flags {:#06x}",
                            c.glyph, c.dx, c.dy, c.flags
                        );
                    }
                }
            }
        }
        for (t, tag, from) in &self.scripts {
            let _ = writeln!(o, "script {t} {tag} {from}");
        }
        for (t, i, tag, lookups) in &self.features {
            let _ = write!(o, "feature {t} {i} {tag}");
            for l in lookups {
                let _ = write!(o, " {l}");
            }
            o.push('\n');
        }
        for s in &self.mark_sets {
            let _ = writeln!(o, "markset {}", s.join(" "));
        }
        for l in &self.lookups {
            let _ = write!(
                o,
                "lookup {} {} flag {:#06x}",
                l.index,
                l.kind.word(),
                l.flag
            );
            if let Some(s) = l.mark_set {
                let _ = write!(o, " markset {s}");
            }
            o.push('\n');
            for s in &l.subtables {
                o.push_str("subtable\n");
                match s {
                    Subtable::Attach { marks, bases } => {
                        for (g, c, (x, y)) in marks {
                            let _ = writeln!(o, "mark {g} {c} {x},{y}");
                        }
                        let word = if l.kind == Kind::MarkMark {
                            "mark2"
                        } else {
                            "base"
                        };
                        for (g, anchors) in bases {
                            let _ = write!(o, "{word} {g}");
                            for a in anchors {
                                let _ = write!(o, " {}", anchor(a));
                            }
                            o.push('\n');
                        }
                    }
                    Subtable::Chain {
                        backtrack,
                        input,
                        lookahead,
                        apply,
                    } => {
                        for (word, seq) in [
                            ("backtrack", backtrack),
                            ("input", input),
                            ("lookahead", lookahead),
                        ] {
                            o.push_str(word);
                            for c in seq {
                                let _ = write!(o, " {}", list(c));
                            }
                            o.push('\n');
                        }
                        o.push_str("apply");
                        for (s, l) in apply {
                            let _ = write!(o, " {s} {l}");
                        }
                        o.push('\n');
                    }
                }
            }
        }
        o
    }

    /// Reads a source; every malformed or unknown line is an error that
    /// names its line.
    pub fn parse(text: &str) -> Result<Source, String> {
        let mut src = Source::default();
        let mut style = false;
        let mut modified = false;
        for (n, line) in text.lines().enumerate() {
            let at = |e: String| format!("line {}: {e}: {line}", n + 1);
            if line.starts_with('#') {
                continue;
            }
            let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
            let f: Vec<&str> = rest.split(' ').filter(|s| !s.is_empty()).collect();
            match word {
                "style" => {
                    src.style = one(&f).map_err(at)?.to_string();
                    style = true;
                }
                "head.modified" => {
                    src.head_modified = num(one(&f).map_err(at)?).map_err(at)?;
                    modified = true;
                }
                w if w.starts_with("os2.") => {
                    let field = &w[4..];
                    if !OS2_FIELDS.contains(&field) {
                        return Err(at(format!("OS/2 field {field} is not one a source sets")));
                    }
                    src.os2
                        .push((field.to_string(), num(one(&f).map_err(at)?).map_err(at)?));
                }
                "name" => {
                    let (head, text) = rest
                        .split_once(" \"")
                        .ok_or_else(|| at("no quoted string".into()))?;
                    let h: Vec<&str> = head.split(' ').collect();
                    let [id, p, e, l] = h.as_slice() else {
                        return Err(at("want id platform encoding language".into()));
                    };
                    src.names.push(Name {
                        id: num(id).map_err(at)?,
                        platform: num(p).map_err(at)?,
                        encoding: num(e).map_err(at)?,
                        language: num(l).map_err(at)?,
                        text: unquote(text).map_err(at)?,
                    });
                }
                "glyph" => src.glyphs.push(glyph(&f).map_err(at)?),
                "contour" => {
                    let g = src
                        .glyphs
                        .last_mut()
                        .ok_or_else(|| at("contour before any glyph".into()))?;
                    let pts = f.iter().map(|p| point(p)).collect::<Result<Vec<_>, _>>();
                    let pts = pts.map_err(at)?;
                    match &mut g.outline {
                        Outline::Empty => {
                            g.outline = Outline::Simple {
                                contours: vec![pts],
                                instructions: vec![],
                                overlap: false,
                            }
                        }
                        Outline::Simple { contours, .. } => contours.push(pts),
                        Outline::Composite(_) => {
                            return Err(at("contour in a composite".into()));
                        }
                    }
                }
                "instructions" => {
                    let g = src
                        .glyphs
                        .last_mut()
                        .ok_or_else(|| at("instructions before any glyph".into()))?;
                    let Outline::Simple { instructions, .. } = &mut g.outline else {
                        return Err(at("instructions outside a simple glyph".into()));
                    };
                    *instructions = hex(one(&f).map_err(at)?).map_err(at)?;
                }
                "component" => {
                    let g = src
                        .glyphs
                        .last_mut()
                        .ok_or_else(|| at("component before any glyph".into()))?;
                    let [name, off, "flags", flags] = f.as_slice() else {
                        return Err(at("want glyph dx,dy flags F".into()));
                    };
                    let (dx, dy) = pair(off).map_err(at)?;
                    let c = Component {
                        glyph: name.to_string(),
                        dx,
                        dy,
                        flags: num(flags).map_err(at)?,
                    };
                    match &mut g.outline {
                        Outline::Empty => g.outline = Outline::Composite(vec![c]),
                        Outline::Composite(cs) => cs.push(c),
                        Outline::Simple { .. } => {
                            return Err(at("component in a simple glyph".into()));
                        }
                    }
                }
                "script" => {
                    let [t, tag, from] = f.as_slice() else {
                        return Err(at("want table tag from".into()));
                    };
                    src.scripts
                        .push((table(t).map_err(at)?, tag.to_string(), from.to_string()));
                }
                "feature" => {
                    let [t, i, tag, lookups @ ..] = f.as_slice() else {
                        return Err(at("want table index tag lookups".into()));
                    };
                    let lookups = lookups
                        .iter()
                        .map(|l| num(l))
                        .collect::<Result<Vec<u16>, _>>()
                        .map_err(at)?;
                    src.features.push((
                        table(t).map_err(at)?,
                        num(i).map_err(at)?,
                        tag.to_string(),
                        lookups,
                    ));
                }
                "markset" => src
                    .mark_sets
                    .push(f.iter().map(|s| s.to_string()).collect()),
                "lookup" => {
                    let (index, kind, flag, mark_set) = match f.as_slice() {
                        [i, k, "flag", fl] => (i, k, fl, None),
                        [i, k, "flag", fl, "markset", s] => (i, k, fl, Some(num(s).map_err(at)?)),
                        _ => return Err(at("want index kind flag F [markset N]".into())),
                    };
                    let kind = match *kind {
                        "markbase" => Kind::MarkBase,
                        "markmark" => Kind::MarkMark,
                        "chain" => Kind::Chain,
                        k => return Err(at(format!("lookup kind {k}"))),
                    };
                    src.lookups.push(Lookup {
                        index: num(index).map_err(at)?,
                        kind,
                        flag: num(flag).map_err(at)?,
                        mark_set,
                        subtables: vec![],
                    });
                }
                "subtable" => {
                    let l = src
                        .lookups
                        .last_mut()
                        .ok_or_else(|| at("subtable before any lookup".into()))?;
                    l.subtables.push(match l.kind {
                        Kind::Chain => Subtable::Chain {
                            backtrack: vec![],
                            input: vec![],
                            lookahead: vec![],
                            apply: vec![],
                        },
                        _ => Subtable::Attach {
                            marks: vec![],
                            bases: vec![],
                        },
                    });
                }
                "mark" | "base" | "mark2" => {
                    let kind = src.lookups.last().map(|l| l.kind);
                    let want = match word {
                        "mark" => kind != Some(Kind::Chain),
                        "base" => kind == Some(Kind::MarkBase),
                        _ => kind == Some(Kind::MarkMark),
                    };
                    let sub = src.lookups.last_mut().and_then(|l| l.subtables.last_mut());
                    let (true, Some(Subtable::Attach { marks, bases })) = (want, sub) else {
                        return Err(at(format!("{word} outside its lookup's subtable")));
                    };
                    if word == "mark" {
                        let [g, c, a] = f.as_slice() else {
                            return Err(at("want glyph class x,y".into()));
                        };
                        marks.push((g.to_string(), num(c).map_err(at)?, pair(a).map_err(at)?));
                    } else {
                        let [g, anchors @ ..] = f.as_slice() else {
                            return Err(at("want glyph anchors".into()));
                        };
                        let anchors = anchors
                            .iter()
                            .map(|a| {
                                if *a == "-" {
                                    Ok(None)
                                } else {
                                    pair(a).map(Some)
                                }
                            })
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(at)?;
                        bases.push((g.to_string(), anchors));
                    }
                }
                "backtrack" | "input" | "lookahead" | "apply" => {
                    let sub = src.lookups.last_mut().and_then(|l| l.subtables.last_mut());
                    let Some(Subtable::Chain {
                        backtrack,
                        input,
                        lookahead,
                        apply,
                    }) = sub
                    else {
                        return Err(at(format!("{word} outside a chain subtable")));
                    };
                    if word == "apply" {
                        if !f.len().is_multiple_of(2) {
                            return Err(at("want sequence-index lookup pairs".into()));
                        }
                        for p in f.chunks(2) {
                            apply.push((num(p[0]).map_err(at)?, num(p[1]).map_err(at)?));
                        }
                    } else {
                        let seq = coverages(rest).map_err(at)?;
                        *match word {
                            "backtrack" => backtrack,
                            "input" => input,
                            _ => lookahead,
                        } = seq;
                    }
                }
                w => return Err(at(format!("unknown record {w:?}"))),
            }
        }
        if !style || !modified {
            return Err("a source needs its style and head.modified lines".into());
        }
        Ok(src)
    }
}

fn one<'a>(f: &[&'a str]) -> Result<&'a str, String> {
    match f {
        [x] => Ok(x),
        _ => Err(format!("want one value, have {}", f.len())),
    }
}

fn num<T: TryFrom<i64>>(s: &str) -> Result<T, String> {
    let v = if let Some(h) = s.strip_prefix("0x") {
        i64::from_str_radix(h, 16)
    } else {
        s.parse::<i64>()
    }
    .map_err(|e| format!("{s:?}: {e}"))?;
    T::try_from(v).map_err(|_| format!("{s} is out of range"))
}

fn pair(s: &str) -> Result<(i16, i16), String> {
    let (x, y) = s
        .split_once(',')
        .ok_or_else(|| format!("{s:?} is not x,y"))?;
    Ok((num(x)?, num(y)?))
}

fn point(s: &str) -> Result<(i16, i16, bool), String> {
    match s.strip_suffix(",off") {
        Some(p) => pair(p).map(|(x, y)| (x, y, false)),
        None => pair(s).map(|(x, y)| (x, y, true)),
    }
}

fn hex(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("odd hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

fn table(t: &str) -> Result<String, String> {
    match t {
        "GSUB" | "GPOS" => Ok(t.to_string()),
        _ => Err(format!("table {t}")),
    }
}

fn glyph(f: &[&str]) -> Result<Glyph, String> {
    let [name, "advance", adv, "class", class, rest @ ..] = f else {
        return Err("want name advance N class N".into());
    };
    let mut unicodes = vec![];
    let mut overlap = false;
    let mut it = rest.iter();
    let mut in_unicode = false;
    for w in it.by_ref() {
        match *w {
            "unicode" => in_unicode = true,
            "overlap" => overlap = true,
            u if in_unicode => {
                unicodes.push(u32::from_str_radix(u, 16).map_err(|e| format!("{u}: {e}"))?)
            }
            w => return Err(format!("unknown glyph field {w}")),
        }
    }
    Ok(Glyph {
        name: name.to_string(),
        advance: num(adv)?,
        class: num(class)?,
        unicodes,
        outline: if overlap {
            Outline::Simple {
                contours: vec![],
                instructions: vec![],
                overlap,
            }
        } else {
            Outline::Empty
        },
    })
}

fn coverages(rest: &str) -> Result<Vec<Vec<String>>, String> {
    let mut out = vec![];
    let mut s = rest.trim();
    while !s.is_empty() {
        let body = s
            .strip_prefix('[')
            .ok_or_else(|| format!("{s:?} is not a bracketed list"))?;
        let (inner, after) = body
            .split_once(']')
            .ok_or_else(|| "an unclosed list".to_string())?;
        out.push(
            inner
                .split(' ')
                .filter(|g| !g.is_empty())
                .map(str::to_string)
                .collect(),
        );
        s = after.trim_start();
    }
    Ok(out)
}

fn unquote(s: &str) -> Result<String, String> {
    let body = s
        .strip_suffix('"')
        .ok_or_else(|| "an unclosed string".to_string())?;
    let mut out = String::new();
    let mut cs = body.chars();
    while let Some(c) = cs.next() {
        if c != '\\' {
            if c == '"' {
                return Err("a bare quote inside a string".into());
            }
            out.push(c);
            continue;
        }
        match cs.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('u') => {
                let rest: String = cs.by_ref().take_while(|c| *c != '}').collect();
                let h = rest
                    .strip_prefix('{')
                    .ok_or_else(|| "a \\u without braces".to_string())?;
                let v = u32::from_str_radix(h, 16).map_err(|e| e.to_string())?;
                out.push(char::from_u32(v).ok_or_else(|| format!("U+{v:X} is not a char"))?);
            }
            e => return Err(format!("escape {e:?}")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_with_every_escape_reads_back() {
        let s = "a \"b\" \\ \u{5d0} \u{1F600}";
        assert_eq!(unquote(&quote(s)[1..]).unwrap(), s);
    }

    #[test]
    fn an_unknown_record_names_its_line() {
        let e = Source::parse("style Regular\nhead.modified 1\nglif x\n").unwrap_err();
        assert!(e.starts_with("line 3: "), "{e}");
    }
}
