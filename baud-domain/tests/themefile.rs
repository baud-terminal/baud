//! The scheme file readers (note Y, section 4). Fixtures are hand-made in
//! the shapes `[[source: tinted-builder]]` 0.11.2 specifies, each slot a
//! distinct colour so a slot landing in the wrong entry cannot pass.

use baud_domain::themefile::*;
use baud_domain::{Chan, Color, ColorScheme};

fn c(v: u32) -> Color {
    let [_, r, g, b] = v.to_be_bytes();
    Color::Rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

#[test]
fn hex_reads_six_three_and_bare_digits() {
    assert_eq!(hex(b"#1a2b3c"), Ok(c(0x1a2b3c)));
    assert_eq!(hex(b"1A2B3C"), Ok(c(0x1a2b3c)));
    assert_eq!(hex(b"#abc"), Ok(c(0xaabbcc)));
    assert_eq!(hex(b"abc"), Ok(c(0xaabbcc)));
}

/// One refused form a line: a name, a keyword, `rgb:`, the alpha form, a
/// wrong width, a stray letter, nothing.
#[test]
fn hex_refuses_every_other_form() {
    for (text, want) in [
        (&b""[..], HexError::Empty),
        (b"#", HexError::Empty),
        (b"red", HexError::NotHex),
        (b"none", HexError::NotHex),
        (b"rgb:12/34/56", HexError::NotHex),
        (b"#12345g", HexError::NotHex),
        (b"#1a2b3c4d", HexError::Width),
        (b"#12", HexError::Width),
        (b"12345", HexError::Width),
    ] {
        assert_eq!(hex(text), Err(want), "{:?}", core::str::from_utf8(text));
    }
}

/// The slot lines of [`BASE16_LIGHT`]: those after `palette:`, so the
/// description's decoy is not one.
fn slot_lines() -> impl Iterator<Item = &'static str> {
    BASE16_LIGHT
        .lines()
        .skip_while(|l| *l != "palette:")
        .skip(1)
}

/// A light base16 scheme in the current nested form. Slot `base0N` holds
/// `0x0N0N0N`-ish values made distinct per slot.
const BASE16_LIGHT: &str = "\
# A hand-made light scheme; not from the tinted corpus.
system: \"base16\"
name: \"Fixture Light\"
author: \"baud tests\"
description: >
  A folded description whose lines must not be read as keys:
  base00: \"000000\"
variant: \"light\"
palette:
  base00: \"#f0f0f0\"  # the page
  base01: \"#e0e0e1\"
  base02: \"#d0d0d2\"
  base03: \"#909093\"
  base04: \"#707074\"
  base05: \"#303035\"
  base06: \"#202026\"
  base07: \"#101017\"
  base08: \"#a00008\"
  base09: \"#a05009\"
  base0A: \"#80700a\"
  base0B: \"#00700b\"
  base0C: \"#00707c\"
  base0D: \"#00307d\"
  base0E: \"#70007e\"
  base0F: \"#70300f\"
";

#[test]
fn base16_lands_on_tinted_themings_table() {
    let s = base16(BASE16_LIGHT.as_bytes()).expect("reads");
    assert_eq!(s.format, Format::Base16);
    let want = [
        0xf0f0f0, 0xa00008, 0x00700b, 0x80700a, 0x00307d, 0x70007e, 0x00707c, 0x303035, 0x909093,
        0xa00008, 0x00700b, 0x80700a, 0x00307d, 0x70007e, 0x00707c, 0x101017,
    ];
    for (i, w) in want.iter().enumerate() {
        assert_eq!(s.ansi[i], Some(c(*w)), "entry {i}");
    }
    // base09, 0F, 01, 02, 04, 06 are 16-21.
    let ext = [0xa05009, 0x70300f, 0xe0e0e1, 0xd0d0d2, 0x707074, 0x202026];
    for (i, w) in ext.iter().enumerate() {
        assert_eq!(s.extended[i], Some(c(*w)), "entry {}", 16 + i);
    }
    assert_eq!((s.page, s.text), (Some(c(0xf0f0f0)), Some(c(0x303035))));
    assert_eq!(s.declared, Some(ColorScheme::Light));
    assert_eq!(s.polarity(), Some(ColorScheme::Light));
}

/// The legacy flat form: `scheme` for the name, every other key a slot at
/// the top level, `system` inferred, values without `#`.
#[test]
fn the_legacy_flat_form_reads_the_same() {
    let flat: String = slot_lines()
        .map(|l| l.trim_start().replace('#', "").replace("  the page", "") + "\n")
        .collect();
    let text = format!("scheme: \"Fixture Light\"\nauthor: \"baud tests\"\n{flat}");
    let legacy = base16(text.as_bytes()).expect("reads");
    let current = base16(BASE16_LIGHT.as_bytes()).unwrap();
    assert_eq!(legacy.ansi, current.ansi);
    assert_eq!(legacy.extended, current.extended);
    assert_eq!(legacy.format, Format::Base16);
    assert_eq!(legacy.declared, None, "the legacy form has no variant");
}

const BASE24_SLOTS: [(&str, u32); 8] = [
    ("base10", 0x100010),
    ("base11", 0x110011),
    ("base12", 0xff0012),
    ("base13", 0xff1300),
    ("base14", 0x14ff00),
    ("base15", 0x00ff15),
    ("base16", 0x1600ff),
    ("base17", 0xff17ff),
];

fn base24_text(system: bool) -> String {
    let mut t = String::new();
    if system {
        t.push_str("system: \"base24\"\n");
    }
    t.push_str("name: \"Fixture 24\"\npalette:\n");
    for line in slot_lines() {
        t.push_str(line);
        t.push('\n');
    }
    for (k, v) in BASE24_SLOTS {
        t.push_str(&format!("  {k}: \"{v:06x}\"\n"));
    }
    t
}

/// base24's brights are its own; base10 and base11 have no ANSI number; and
/// with `system` missing, the extra slots say it is base24.
#[test]
fn base24_takes_its_own_brights_and_is_inferred() {
    for system in [true, false] {
        let s = base16(base24_text(system).as_bytes()).expect("reads");
        assert_eq!(s.format, Format::Base24, "system given: {system}");
        let brights = [0xff0012, 0x14ff00, 0xff1300, 0x1600ff, 0xff17ff, 0x00ff15];
        for (i, w) in brights.iter().enumerate() {
            assert_eq!(s.ansi[9 + i], Some(c(*w)), "entry {}", 9 + i);
        }
        assert_eq!(s.ansi[1], Some(c(0xa00008)), "the normal red is base08");
        for (_, v) in &BASE24_SLOTS[..2] {
            assert!(!s.ansi.contains(&Some(c(*v))), "base10/11 land nowhere");
        }
    }
}

/// Each refused form is one error, at its line.
#[test]
fn refused_files_name_the_line_and_the_reason() {
    let missing = BASE16_LIGHT.replace("  base0F: \"#70300f\"\n", "");
    let err = base16(missing.as_bytes()).unwrap_err();
    assert_eq!(err.kind, SchemeErrorKind::Missing(Slot(0x0f)));
    let last = u32::try_from(missing.lines().count()).unwrap();
    assert_eq!(err.line, Line(last), "the file's last line, not past it");

    let dup = BASE16_LIGHT.replace(
        "  base01: \"#e0e0e1\"\n",
        "  base01: \"#e0e0e1\"\n  base01: \"#e0e0e2\"\n",
    );
    let err = base16(dup.as_bytes()).unwrap_err();
    assert_eq!((err.kind, err.line), (SchemeErrorKind::Duplicate, Line(12)));

    let bad = BASE16_LIGHT.replace("\"#a00008\"", "\"red\"");
    let err = base16(bad.as_bytes()).unwrap_err();
    assert_eq!(
        (err.kind, err.line),
        (SchemeErrorKind::Hex(HexError::NotHex), Line(18))
    );

    let other = BASE16_LIGHT.replace("\"base16\"", "\"tinted8\"");
    assert_eq!(
        base16(other.as_bytes()).unwrap_err().kind,
        SchemeErrorKind::UnknownSystem
    );
}

/// A declared `variant` that the page and text contradict is kept as
/// declared, so the embedder can report the mismatch; the polarity used is
/// derived.
#[test]
fn a_declared_variant_is_checked_not_trusted() {
    let lying = BASE16_LIGHT.replace("variant: \"light\"", "variant: \"dark\"");
    let s = base16(lying.as_bytes()).unwrap();
    assert_eq!(s.declared, Some(ColorScheme::Dark));
    assert_eq!(s.polarity(), Some(ColorScheme::Light));
    let any = BASE16_LIGHT.replace("variant: \"light\"", "variant: \"sepia\"");
    assert_eq!(
        base16(any.as_bytes()).unwrap().declared,
        None,
        "tinted: 'could be anything'"
    );
}

/// A bare value ends at a comment, and a comment begins at a `#` that
/// starts the value or follows a space -- so YAML reads a bare `#f0f0f0` as
/// no value at all, and so does baud.
#[test]
fn a_bare_value_ends_at_its_comment() {
    let bare: String = slot_lines()
        .map(|l| l.replace("\"#", "").replace('"', "") + " # a comment\n")
        .collect();
    let s = base16(format!("palette:\n{bare}").as_bytes()).expect("reads");
    assert_eq!(s.ansi, base16(BASE16_LIGHT.as_bytes()).unwrap().ansi);
    let hash = BASE16_LIGHT.replace("\"#f0f0f0\"", "#f0f0f0");
    let err = base16(hash.as_bytes()).unwrap_err();
    assert_eq!(err.kind, SchemeErrorKind::Hex(HexError::Empty));
}
