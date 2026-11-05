//! A table expanded to lines, and the first difference between two.
//!
//! A line is a glyph (when it is about one), a locus naming the field, and
//! a value of space-separated tokens, each of which names itself (`p12=..`,
//! `U+0041>36`). Two expansions are equal when their lines are; where they
//! differ the report gives the glyph, the locus and the first token that
//! differs, so a point or a mapping is named and not a whole line.

use crate::glyphs::GlyphNames;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Dump {
    lines: Vec<Line>,
}

#[derive(Debug, PartialEq, Eq)]
struct Line {
    glyph: Option<u32>,
    locus: String,
    value: String,
}

impl Dump {
    /// A line about the table as a whole.
    pub fn put(&mut self, locus: impl Into<String>, value: impl Into<String>) {
        self.lines.push(Line {
            glyph: None,
            locus: locus.into(),
            value: value.into(),
        });
    }

    /// A line about one glyph.
    pub fn glyph(&mut self, gid: u32, locus: impl Into<String>, value: impl Into<String>) {
        self.lines.push(Line {
            glyph: Some(gid),
            locus: locus.into(),
            value: value.into(),
        });
    }

    /// The lines that compare a part by its bytes.
    pub fn unexpanded(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter(|l| l.value.contains("unmodelled:") || l.value.contains("not-expanded:"))
            .map(|l| format!("{} = {}", l.locus, l.value))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Space-separated tokens, built up one at a time.
#[derive(Default)]
pub struct Tokens(String);

impl Tokens {
    pub fn push(&mut self, token: impl AsRef<str>) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        self.0.push_str(token.as_ref());
    }

    pub fn done(self) -> String {
        self.0
    }
}

fn where_(line: &Line, names: &GlyphNames) -> String {
    match line.glyph {
        Some(g) => format!("{} {}", names.label(g), line.locus),
        None => line.locus.clone(),
    }
}

fn short(s: &str) -> String {
    const MAX: usize = 120;
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let cut: String = s.chars().take(MAX).collect();
    format!("{cut}...")
}

/// The first difference between two expansions of one table, or `None`.
pub fn diff(a: &Dump, b: &Dump, names: &GlyphNames) -> Option<String> {
    for (i, (la, lb)) in a.lines.iter().zip(&b.lines).enumerate() {
        if la == lb {
            continue;
        }
        if la.glyph == lb.glyph && la.locus == lb.locus {
            let (ta, tb): (Vec<&str>, Vec<&str>) =
                (la.value.split(' ').collect(), lb.value.split(' ').collect());
            let at = ta
                .iter()
                .zip(&tb)
                .position(|(x, y)| x != y)
                .unwrap_or(ta.len().min(tb.len()));
            let tok = |t: &[&str]| {
                t.get(at)
                    .map_or("(nothing)".to_string(), |s| format!("`{}`", short(s)))
            };
            return Some(format!(
                "{}: {} vs {}",
                where_(la, names),
                tok(&ta),
                tok(&tb)
            ));
        }
        return Some(format!(
            "line {i}: A has {} = `{}`, B has {} = `{}`",
            where_(la, names),
            short(&la.value),
            where_(lb, names),
            short(&lb.value)
        ));
    }
    let (n, m) = (a.lines.len(), b.lines.len());
    if n == m {
        return None;
    }
    let (side, extra) = if n > m {
        ("A", &a.lines[m])
    } else {
        ("B", &b.lines[n])
    };
    Some(format!(
        "{n} lines vs {m}; {side} goes on with {} = `{}`",
        where_(extra, names),
        short(&extra.value)
    ))
}
