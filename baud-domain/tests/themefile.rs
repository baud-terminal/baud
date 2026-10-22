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

/// **Any one of base10-base17 makes an unlabelled file base24** (O2,
/// Jev decided any_one 1.00 / 1.00, decisions.jsonl 710/711), as both
/// tinted builders that infer the system class it: base16-builder-go by
/// its count, tinted-builder-python on the first `base1x` it finds. A
/// base24 file without all 24 is then refused at its first missing slot
/// (O3), however many it has.
#[test]
fn one_base1x_slot_makes_an_unlabelled_file_base24() {
    let sixteen: String = slot_lines().map(|l| format!("{l}\n")).collect();
    for (k, v) in BASE24_SLOTS {
        let text = format!("palette:\n{sixteen}  {k}: \"{v:06x}\"\n");
        let err = base16(text.as_bytes()).unwrap_err();
        let first = if k == "base10" { 0x11 } else { 0x10 };
        assert_eq!(err.kind, SchemeErrorKind::Missing(Slot(first)), "{k}");
    }
    let s = base16(format!("palette:\n{sixteen}").as_bytes()).unwrap();
    assert_eq!(s.format, Format::Base16);
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

/// A hand-made ghostty theme: every palette index form, every colour form
/// ghostty reads but a name, a `#` inside a value, an indented comment, a
/// quoted value, a CRLF line, and two keys baud does not read.
const GHOSTTY: &str = "\
# A hand-made ghostty theme; not from the bundled set.
palette = 0=#000001
palette = 1=a00001
palette = 2 = #0a0\r
palette = 3=0a0b03
palette = 4=rgb:00/30/7d
palette = 5=#7000007e0
palette = 0x6=#00707c
palette = 0b111=#c0c0c7
  # an indented comment
palette = 0o10=#808088
palette = 9=#ff0009
palette = 10=#00ff0a
palette = 11=#ffff0b
palette = 12=#0000ff
palette = 13=#ff00ff
palette = 14=#00ffff
palette = 15=#ffffff
palette = 200=#c8c8c8
background = #102030
foreground = \"#e0e0e1\"
cursor-color = #fff000
cursor-text = #000fff
selection-background = #333333
selection-foreground = #cccccc
font-family = Fixture Mono
window-theme = dark
";

fn read_ghostty(text: &str) -> (Result<Scheme, SchemeError>, Vec<(u32, String)>) {
    let mut ignored = Vec::new();
    let s = ghostty(text.as_bytes(), |Line(n), key| {
        ignored.push((n, String::from_utf8_lossy(key).into_owned()))
    });
    (s, ignored)
}

#[test]
fn ghostty_reads_its_seven_keys_and_every_index_form() {
    let (s, ignored) = read_ghostty(GHOSTTY);
    let s = s.expect("reads");
    assert_eq!(s.format, Format::Ghostty);
    let want = [
        0x000001, 0xa00001, 0x00aa00, 0x0a0b03, 0x00307d, 0x6f007d, 0x00707c, 0xc0c0c7, 0x808088,
        0xff0009, 0x00ff0a, 0xffff0b, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff,
    ];
    for (i, w) in want.iter().enumerate() {
        assert_eq!(s.ansi[i], Some(c(*w)), "entry {i}");
    }
    assert_eq!(s.extended[200 - 16], Some(c(0xc8c8c8)), "entry 200");
    assert_eq!(s.extended.iter().filter(|e| e.is_some()).count(), 1);
    // A `#` in a value is data, not a comment.
    assert_eq!(s.page, Some(c(0x102030)));
    assert_eq!(s.text, Some(c(0xe0e0e1)), "the quotes come off");
    let paint = |v| Some(CellPaint::Colour(c(v)));
    assert_eq!(s.cursor, paint(0xfff000));
    assert_eq!(s.cursor_text, paint(0x000fff));
    assert_eq!(s.selection, paint(0x333333));
    assert_eq!(s.selection_text, paint(0xcccccc));
    assert_eq!(s.declared, None, "a ghostty file declares no variant");
    assert_eq!(s.polarity(), Some(ColorScheme::Dark));
    assert_eq!(
        ignored,
        [
            (26, "font-family".to_string()),
            (27, "window-theme".to_string())
        ]
    );
}

/// A file that sets only some entries leaves the rest unset; the config
/// layer fills them from baud's own.
/// **ghostty 1.2's cell names read as the cell's own colours**: the
/// cursor's and the selection's four take `cell-foreground` and
/// `cell-background` (`[[source: ghostty-config-8f0dd37]]` 707-716,
/// 845-859, 905-911), where the file was refused whole as no colour;
/// the page and the text take no cell name, so there it is still refused.
#[test]
fn ghostty_cell_names_name_the_cells_colours() {
    let (s, _) = read_ghostty(
        "selection-foreground = cell-background\nselection-background = cell-foreground\n\
         cursor-color = cell-foreground\ncursor-text = #102030\n",
    );
    let s = s.expect("reads");
    assert_eq!(
        (s.selection_text, s.selection),
        (Some(CellPaint::Page), Some(CellPaint::Text))
    );
    assert_eq!(
        (s.cursor, s.cursor_text),
        (Some(CellPaint::Text), Some(CellPaint::Colour(c(0x102030))))
    );
    let (page, _) = read_ghostty("background = cell-background\n");
    assert!(page.is_err(), "{page:?}");
}

#[test]
fn a_partial_ghostty_file_leaves_the_rest_unset() {
    let (s, _) = read_ghostty("palette = 1=#a00001\nbackground = #ffffff\n");
    let s = s.expect("reads");
    assert_eq!(s.ansi[1], Some(c(0xa00001)));
    assert_eq!(s.ansi.iter().filter(|e| e.is_some()).count(), 1);
    assert_eq!((s.page, s.text), (Some(c(0xffffff)), None));
    assert_eq!(s.polarity(), None, "no text, no polarity");
}

/// ghostty applies lines in order: a repeat takes its last value, and an
/// empty value resets -- one entry, or with `palette =` all of them.
#[test]
fn a_repeat_takes_the_last_value_and_an_empty_one_unsets() {
    let (s, _) = read_ghostty(
        "palette = 1=#111111\npalette = 1=#a00001\nbackground = #000000\n\
         background = #102030\nforeground = #eeeeee\nforeground =\n",
    );
    let s = s.expect("reads");
    assert_eq!(s.ansi[1], Some(c(0xa00001)));
    assert_eq!(s.page, Some(c(0x102030)));
    assert_eq!(s.text, None);
    let (s, _) = read_ghostty(
        "palette = 1=#a00001\npalette = 200=#c8c8c8\npalette =\npalette = 2=#00aa00\n",
    );
    let s = s.expect("reads");
    assert_eq!(s.ansi[1], None, "`palette =` unset entry 1");
    assert_eq!(s.extended[200 - 16], None, "and entry 200");
    assert_eq!(s.ansi[2], Some(c(0x00aa00)), "a later entry stands");
}

#[test]
fn ghostty_refusals_name_the_line_and_the_reason() {
    for (text, line, kind) in [
        ("palette = 256=#000000\n", 1, SchemeErrorKind::PaletteIndex),
        (
            "\npalette = red=#000000\n",
            2,
            SchemeErrorKind::PaletteIndex,
        ),
        ("palette = 5\n", 1, SchemeErrorKind::Syntax),
        ("background\n", 1, SchemeErrorKind::Syntax),
        ("background = black\n", 1, SchemeErrorKind::ColourName),
        (
            "palette = 1=DarkSlateGray4\n",
            1,
            SchemeErrorKind::ColourName,
        ),
        (
            "background = #12\n",
            1,
            SchemeErrorKind::Hex(HexError::Width),
        ),
        (
            "background = #12345g\n",
            1,
            SchemeErrorKind::Hex(HexError::NotHex),
        ),
        (
            "background = rgbi:2/0/0\n",
            1,
            SchemeErrorKind::Hex(HexError::NotHex),
        ),
        (
            "background = rgb:1/2\n",
            1,
            SchemeErrorKind::Hex(HexError::NotHex),
        ),
    ] {
        let (s, _) = read_ghostty(text);
        assert_eq!(
            s.unwrap_err(),
            SchemeError {
                line: Line(line),
                kind
            },
            "{text:?}"
        );
    }
}

/// The 602 themes ghostty bundles, when the pinned tarball is unpacked at
/// `BAUD_GHOSTTY_THEMES` (see `[[source: ghostty-themes-tgz]]`): every file
/// reads, sets all sixteen and the six others, and ignores nothing.
#[test]
fn every_bundled_ghostty_theme_reads_whole() {
    let Some(dir) = std::env::var_os("BAUD_GHOSTTY_THEMES") else {
        return;
    };
    let mut files = 0;
    for entry in std::fs::read_dir(&dir).expect("a directory") {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let (s, ignored) = read_ghostty(&text);
        let s = s.unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
        assert!(s.ansi.iter().all(Option::is_some), "{}", path.display());
        assert!(
            [s.page, s.text].iter().all(Option::is_some)
                && [s.cursor, s.cursor_text, s.selection, s.selection_text]
                    .iter()
                    .all(Option::is_some),
            "{}",
            path.display()
        );
        assert!(ignored.is_empty(), "{}: {ignored:?}", path.display());
        files += 1;
    }
    assert_eq!(files, 602, "the pinned set");
}

/// Review, 2026-09-29: the colour forms exactly as ghostty's `RGB.parse`
/// reads them. `fromHex` truncates `v * 255 / max` and reads each channel
/// with Zig's `parseUnsigned(u16, _, 16)`, `_` separators included;
/// `rgbi:` goes through `fraction.parse` and `@intFromFloat(i * 255)`; a
/// quoted value is trimmed again inside its quotes.
#[test]
fn ghostty_colours_read_exactly_as_ghostty_reads_them() {
    let page = |v: &str| {
        read_ghostty(&format!("background = {v}\n"))
            .0
            .map(|s| s.page)
    };
    let grey = |n: u32| Ok(Some(c(n * 0x010101)));
    assert_eq!(
        page("#800800800"),
        grey(0x7f),
        "2048 * 255 / 4095 truncates to 127"
    );
    assert_eq!(
        page("rgb:8000/8000/8000"),
        grey(0x7f),
        "32768 * 255 / 65535 = 127.5 truncates"
    );
    assert_eq!(
        page("#1_21_21_2"),
        grey(1),
        "0x12 over twelve bits: Zig skips the `_`"
    );
    assert_eq!(page("#fff"), grey(0xff));
    assert_eq!(page("rgb:f/f/f"), grey(0xff));
    assert_eq!(
        page("\" #800800800 \""),
        grey(0x7f),
        "trimmed inside its quotes"
    );
    assert_eq!(page("\" rgb:f/f/f \""), grey(0xff));
    assert_eq!(
        page("rgbi:1/0/0.5"),
        Ok(Some(c(0xff007f))),
        "0.5 * 255 truncates to 127"
    );
    assert_eq!(page("rgbi:-0/+1/.5"), Ok(Some(c(0x00ff7f))));
    assert_eq!(page("rgbi:1./0.0/00.00"), Ok(Some(c(0xff0000))));
    for bad in [
        "rgbi:1.5/0/0",
        "rgbi:-0.5/0/0",
        "rgbi:1e-1/0/0",
        "rgbi:./0/0",
        "rgbi: 1/0/0",
        "RGB:f/f/f",
        "rgb:12345/0/0",
        "rgb:f/f/f/f",
        "#_12",
        "#12_",
    ] {
        assert!(page(bad).is_err(), "{bad} is refused");
    }
}

/// ghostty's line trim is space, tab and CR (`args.zig` 19, 1453), not
/// every ASCII whitespace: a form feed stays part of the key.
#[test]
fn a_form_feed_is_not_whitespace_to_ghostty() {
    let (s, ignored) = read_ghostty("\x0cbackground = #ffffff\n");
    assert_eq!(s.unwrap().page, None);
    assert_eq!(ignored, [(1, "\x0cbackground".to_string())]);
}

/// The palette index as Zig 0.16's `parseInt(u8, _, 0)` reads it: a sign,
/// `_` between digits, a prefix in either case when more than two bytes
/// follow.
#[test]
fn a_palette_index_reads_as_zigs_parse_int() {
    let entry = |n: &str| {
        read_ghostty(&format!("palette = {n}=#123456\n"))
            .0
            .map(|s| s.ansi.iter().position(Option::is_some))
    };
    for (n, want) in [
        ("+5", 5),
        ("-0", 0),
        ("1_0", 10),
        ("0x1_0", 16),
        ("0X6", 6),
        ("0B1", 1),
        ("0O7", 7),
        ("00012", 12),
        ("\t3 ", 3),
    ] {
        let slot = if want < 16 { Some(want) } else { None };
        match slot {
            Some(i) => assert_eq!(entry(n), Ok(Some(i)), "{n}"),
            None => assert_eq!(entry(n), Ok(None), "{n} lands past 15"),
        }
    }
    for bad in [
        "-5", "_1", "1_", "0x", "0b", "+", "-", "0x_10", "0x10_", "256", "1 0",
    ] {
        assert_eq!(
            entry(bad).unwrap_err().kind,
            SchemeErrorKind::PaletteIndex,
            "{bad}"
        );
    }
}

/// A multi-word X11 name is a name.
#[test]
fn a_name_with_spaces_is_a_name() {
    let (s, _) = read_ghostty("background = dark slate gray\n");
    assert_eq!(s.unwrap_err().kind, SchemeErrorKind::ColourName);
}

/// The YAML subset's own rules, each with a case that only it decides.
#[test]
fn the_yaml_subset_rules_each_decide_a_case() {
    let slots = || slot_lines().collect::<Vec<_>>().join("\n") + "\n";
    // A block scalar inside `palette:` is skipped whole; the nested-map rule
    // cannot skip it, because the palette is open.
    let text = format!("palette:\n  note: |\n    base00: \"#000000\"\n{}", slots());
    assert_eq!(base16(text.as_bytes()).unwrap().page, Some(c(0xf0f0f0)));
    // A top-level key closes `palette:`; a map under it is not the palette.
    let text = format!("palette:\n{}extra:\n  base00: \"#000000\"\n", slots());
    assert_eq!(base16(text.as_bytes()).unwrap().page, Some(c(0xf0f0f0)));
    // `---` opens the document and is not a line of it.
    let text = format!("---\npalette:\n{}", slots());
    assert!(base16(text.as_bytes()).is_ok());
    // base18 is no slot.
    let text = format!("palette:\n{}  base18: \"#000000\"\n", slots());
    assert_eq!(base16(text.as_bytes()).unwrap().format, Format::Base16);
    // Single quotes read; text after a closing quote, or a quote left open,
    // does not.
    let single = slots().replace('"', "'");
    assert_eq!(
        base16(format!("palette:\n{single}").as_bytes())
            .unwrap()
            .page,
        Some(c(0xf0f0f0))
    );
    let after = slots().replacen("\"#f0f0f0\"", "\"#f0f0f0\" x", 1);
    assert_eq!(
        base16(format!("palette:\n{after}").as_bytes()).unwrap_err(),
        SchemeError {
            line: Line(2),
            kind: SchemeErrorKind::Syntax
        }
    );
    let open = slots().replacen("\"#f0f0f0\"", "\"#f0f0f0", 1);
    assert_eq!(
        base16(format!("palette:\n{open}").as_bytes())
            .unwrap_err()
            .kind,
        SchemeErrorKind::Syntax
    );
    // A tab in the indentation is refused.
    let tab = slots().replacen("  base00", "\tbase00", 1);
    assert_eq!(
        base16(format!("palette:\n{tab}").as_bytes()).unwrap_err(),
        SchemeError {
            line: Line(2),
            kind: SchemeErrorKind::Syntax
        }
    );
}

/// An explicit `system` decides the format over the slots present; `system`
/// and `variant` given twice are refused like a slot given twice.
#[test]
fn an_explicit_system_decides_and_a_repeat_is_refused() {
    let s = base16(
        base24_text(false)
            .replace("name:", "system: \"base16\"\nname:")
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(s.format, Format::Base16, "declared base16, base1x present");
    assert_eq!(s.ansi[9], Some(c(0xa00008)), "so the brights are base16's");
    let short = format!(
        "system: \"base24\"\npalette:\n{}",
        slot_lines().collect::<Vec<_>>().join("\n") + "\n"
    );
    assert_eq!(
        base16(short.as_bytes()).unwrap_err().kind,
        SchemeErrorKind::Missing(Slot(0x10))
    );
    for key in ["system: \"base16\"", "variant: \"light\""] {
        let twice = BASE16_LIGHT.replace("palette:", &format!("{key}\npalette:"));
        assert_eq!(
            base16(twice.as_bytes()).unwrap_err().kind,
            SchemeErrorKind::Duplicate,
            "{key}"
        );
    }
}
