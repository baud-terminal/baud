//! Other terminals' theme files, read by each terminal's own grammar: the
//! value forms each parser takes, the lines each skips, and what lands
//! where. Hand-made inputs, one rule a case.

use baud_domain::Color;
use baud_domain::themefile::*;

fn c(h: &str) -> Color {
    hex(h.as_bytes()).unwrap()
}

/// The lines a reader skipped, each with why.
type Skips = Vec<(Line, String, Skipped)>;

/// A reader's result, and the lines it skipped with why.
fn kitty_of(text: &str) -> (Result<Scheme, SchemeError>, Skips) {
    let mut skipped = Vec::new();
    let r = kitty(text.as_bytes(), |l, k, w| {
        skipped.push((l, String::from_utf8_lossy(k).into_owned(), w))
    });
    (r, skipped)
}

fn foot_of(text: &str) -> (Result<Scheme, SchemeError>, Skips) {
    let mut skipped = Vec::new();
    let r = foot(text.as_bytes(), |l, k, w| {
        skipped.push((l, String::from_utf8_lossy(k).into_owned(), w))
    });
    (r, skipped)
}

/// kitty's `#` forms: three digits doubled, six, and nine or twelve read
/// as each component's first two digits (kitty `colors.c` `parse_sharp`);
/// `rgb:` one digit doubled or the first two; a trailing comment cut.
#[test]
fn kitty_reads_its_own_colour_forms() {
    let (s, skipped) = kitty_of(
        "color0 #abc\ncolor1 #a1b2c3\ncolor2 #123456789\ncolor3 #1234abcd5678\n\
         color4 rgb:a/bb/ccc\ncolor5 #FFFFFF # white\ncolor6 RGB:1/2/3\n",
    );
    let s = s.unwrap();
    assert_eq!(s.ansi[0], Some(c("#aabbcc")));
    assert_eq!(s.ansi[1], Some(c("#a1b2c3")));
    assert_eq!(s.ansi[2], Some(c("#124578")));
    assert_eq!(s.ansi[3], Some(c("#12ab56")));
    assert_eq!(s.ansi[4], Some(c("#aabbcc")));
    assert_eq!(s.ansi[5], Some(c("#ffffff")));
    assert_eq!(s.ansi[6], Some(c("#112233")));
    assert!(skipped.is_empty(), "{skipped:?}");
}

/// What kitty itself refuses it skips, and so does baud: four or eight
/// digits, `rgbi:` (its branch looks for a second `i`), `color01`, and a
/// line with no space before its value. A later line wins.
#[test]
fn kitty_skips_what_kitty_refuses_and_a_later_line_wins() {
    let (s, skipped) = kitty_of(
        "color0 #1234\ncolor1 #11223344\ncolor2 rgbi:1/0/0\ncolor01 #ffffff\n\
         color3#ffffff\nbackground #000000\nbackground #101010\nfont_size 12\n",
    );
    let s = s.unwrap();
    assert_eq!(s.ansi[..4], [None, None, None, None]);
    assert_eq!(s.page, Some(c("#101010")));
    let lines: Vec<_> = skipped
        .iter()
        .map(|(l, k, w)| (l.0, k.as_str(), *w))
        .collect();
    assert_eq!(
        lines,
        [
            (1, "color0", Skipped::Refused),
            (2, "color1", Skipped::Refused),
            (3, "color2", Skipped::Refused),
            (4, "color01", Skipped::NotAColour),
            (5, "color3#ffffff", Skipped::Syntax),
            (8, "font_size", Skipped::NotAColour),
        ]
    );
}

/// `none` and `cursor_text_color background` leave the colour unset,
/// which baud draws as kitty describes: the cell's own colours reversed.
/// Entries past 15 land in `extended`.
#[test]
fn kitty_none_unsets_and_high_entries_are_extended() {
    let (s, _) = kitty_of(
        "cursor #ffffff\ncursor none\ncursor_text_color background\n\
         selection_background none\ncolor255 #010203\n",
    );
    let s = s.unwrap();
    assert_eq!((s.cursor, s.cursor_text, s.selection), (None, None, None));
    assert_eq!(s.extended[239], Some(c("#010203")));
    assert_eq!(s.format, Format::Kitty);
}

/// kitty's own parse_color, from `scripts/kitty-colour-oracle.sh`: each
/// value and what kitty 1e1486e reads it as, `-` for nothing; an `oklch(`
/// or `lab(` value kitty reads then its case, and for `out` what kitty's Go
/// code draws. A value may hold a tab, so a row is read from its right.
const KITTY: &str = include_str!("kitty-colours.txt");

/// A row: the value, what kitty's terminal reads it as, and the case.
fn kitty_row(row: &str) -> (String, &str, Option<&str>, Option<&str>) {
    let f: Vec<&str> = row.split('\t').collect();
    let n = f.len();
    let (cols, case, go) = match f[n - 1] {
        "edge" | "grey" | "in" => (2, Some(f[n - 1]), None),
        _ if n >= 4 && f[n - 2] == "out" => (3, Some("out"), Some(f[n - 1])),
        _ => (1, None, None),
    };
    (f[..n - cols].join("\t"), f[n - cols], case, go)
}

/// **Every value reads as kitty reads it**, on the 2300 the oracle ran:
/// all 753 names in three cases, every `#` and `rgb:` form, and `oklch(` and
/// `lab(` with each number form C's `strtod` takes or refuses. A value kitty
/// refuses skips its line; a name resolves; an `oklch(` or `lab(` kitty
/// reads is drawn by the owner's call on note AR's O14: black or white at an
/// edge, a grey and a colour inside sRGB as kitty's terminal draws them, a
/// colour outside sRGB as kitty's Go code does -- byte for byte.
#[test]
fn every_value_reads_as_kittys_own_parser_reads_it() {
    let mut n = 0;
    let mut cases = std::collections::BTreeMap::new();
    let mut apart = 0;
    for row in KITTY.lines() {
        let (value, want, case, go) = kitty_row(row);
        let text = format!("color0 #000000\ncolor1 {value}\n");
        let (got, skipped) = kitty_of(&text);
        let got = got.unwrap_or_else(|e| panic!("{value:?}: {e:?}"));
        if want == "-" {
            assert_eq!(got.ansi[1], None, "{value:?}");
            assert_eq!(
                skipped,
                [(Line(2), "color1".into(), Skipped::Refused)],
                "{value:?}"
            );
        } else {
            let drawn = go.unwrap_or(want);
            assert_eq!(got.ansi[1], Some(c(drawn)), "{value:?} ({case:?})");
            assert!(skipped.is_empty(), "{value:?}: {skipped:?}");
        }
        *cases.entry(case.unwrap_or("")).or_insert(0) += 1;
        apart += usize::from(go.is_some_and(|g| g != want));
        n += 1;
    }
    assert_eq!(n, 2300);
    assert_eq!(
        cases.into_iter().collect::<Vec<_>>(),
        [
            ("", 870),
            ("edge", 35),
            ("grey", 135),
            ("in", 415),
            ("out", 845)
        ]
    );
    // Rows where kitty's terminal and its Go code draw different bytes, so
    // the case decides.
    assert_eq!(apart, 744);
}

/// **kitty's names are X.Org's rgb.txt and `debianred`**, each found in
/// any case, with spaces where rgb.txt has them; a word that is not one is
/// skipped, as kitty skips it.
#[test]
fn kitty_names_are_rgb_txt_and_debianred() {
    let rgb = include_str!("../../docs/research/sources/xorg-rgb-749182c.txt");
    let mut n = 0;
    for line in rgb.lines().filter(|l| !l.is_empty() && !l.starts_with('!')) {
        let mut f = line.split_whitespace();
        let v: Vec<u8> = f.by_ref().take(3).map(|x| x.parse().unwrap()).collect();
        let name = f.collect::<Vec<_>>().join(" ");
        let (s, _) = kitty_of(&format!("color1 {name}\n"));
        assert_eq!(
            s.unwrap().ansi[1],
            Some(c(&format!("#{:02x}{:02x}{:02x}", v[0], v[1], v[2]))),
            "{name}"
        );
        n += 1;
    }
    assert_eq!(n, 752);
    let (s, _) = kitty_of("color1 DebianRed\n");
    assert_eq!(s.unwrap().ansi[1], Some(c("#d70751")));
    let (s, skipped) = kitty_of("color1 lime\ncolor2 abcdef\n");
    assert_eq!(s.unwrap().ansi[1..3], [None, None]);
    assert_eq!(skipped.len(), 2, "{skipped:?}");
}

/// **kitty and foot read bytes**: a byte that is not UTF-8 in a comment
/// leaves the file read, as kitty reads its config with `errors='replace'`
/// (`[[source: kitty-conf-utils-1e1486e]]` 468) and foot reads bytes.
#[test]
fn kitty_and_foot_read_a_latin_1_comment() {
    let k = kitty(b"# Th\xe8me\nbackground #1d1f21\n", |_, _, _| {}).unwrap();
    assert_eq!(k.page, Some(c("#1d1f21")));
    let f = foot(
        b"# Th\xe8me\n[colors-dark]\nbackground=1d1f21\n",
        |_, _, _| {},
    )
    .unwrap();
    assert_eq!(f.page, Some(c("#1d1f21")));
}

/// foot's values are six hex digits, "without prefix"; C's `strtoul`
/// also takes a `+` or `0x` inside the six. Other widths are skipped, as
/// foot skips them. A blank then `#` starts a trailing comment.
#[test]
fn foot_reads_six_digits_and_skips_the_rest() {
    let (s, skipped) = foot_of(
        "[colors-dark]\nregular0=1d1f21\nregular1=+12345\nregular2=0x1234\n\
         regular3=fff\nregular4=1d1f21ff\nbackground=161719 # the page\n",
    );
    let s = s.unwrap();
    assert_eq!(s.ansi[0], Some(c("#1d1f21")));
    assert_eq!(s.ansi[1], Some(c("#012345")));
    assert_eq!(s.ansi[2], Some(c("#001234")));
    assert_eq!((s.ansi[3], s.ansi[4]), (None, None));
    assert_eq!(s.page, Some(c("#161719")));
    let lines: Vec<_> = skipped
        .iter()
        .map(|(l, k, w)| (l.0, k.as_str(), *w))
        .collect();
    assert_eq!(
        lines,
        [
            (5, "regular3", Skipped::Refused),
            (6, "regular4", Skipped::Refused)
        ]
    );
}

/// `cursor` is two colours, text first; a palette key is a C base-0
/// number, so `010` is entry 8 and `0x10` entry 16; another section and
/// its keys are passed over, reported once at the header.
#[test]
fn foot_cursor_palette_numbers_and_other_sections() {
    let (s, skipped) = foot_of(
        "[main]\nfont=Mono\n[colors-dark]\ncursor=111111 222222\n010=0a0a0a\n0x10=101010\n\
         bright7=eaeaea\nalpha=0.9\n",
    );
    let s = s.unwrap();
    assert_eq!(
        (s.cursor_text, s.cursor),
        (Some(c("#111111")), Some(c("#222222")))
    );
    assert_eq!(s.ansi[8], Some(c("#0a0a0a")));
    assert_eq!(s.extended[0], Some(c("#101010")));
    assert_eq!(s.ansi[15], Some(c("#eaeaea")));
    let lines: Vec<_> = skipped
        .iter()
        .map(|(l, k, w)| (l.0, k.as_str(), *w))
        .collect();
    assert_eq!(
        lines,
        [
            (1, "[main]", Skipped::Section),
            (8, "alpha", Skipped::NotAColour)
        ]
    );
}

/// **A dark and a light theme in one file are refused**, at the second's
/// first colour: which one a baud half reads is undecided (note AR, O8).
/// Either section alone is read.
#[test]
fn foot_with_both_halves_is_refused_and_either_alone_is_read() {
    let both = "[colors-dark]\nbackground=000000\n[colors-light]\nbackground=ffffff\n";
    assert_eq!(
        foot_of(both).0,
        Err(SchemeError {
            line: Line(4),
            kind: SchemeErrorKind::Pair
        })
    );
    let light = "[colors-light]\nbackground=ffffff\n";
    assert_eq!(foot_of(light).0.unwrap().page, Some(c("#ffffff")));
    let empty_light = "[colors-dark]\nbackground=000000\n[colors-light]\n";
    assert_eq!(foot_of(empty_light).0.unwrap().page, Some(c("#000000")));
}

/// A list of sixteen colours lands on ANSI 0 to 15 in order, either
/// case; the page and the text stay unset; any other count is refused.
#[test]
fn a_plain_list_is_ansi_0_to_15() {
    let lines: Vec<String> = (0..16).map(|i| format!("#0{i:x}0{i:X}0A")).collect();
    let s = plain_list(lines.join("\n").as_bytes()).unwrap();
    assert_eq!(s.ansi[0], Some(c("#00000a")));
    assert_eq!(s.ansi[15], Some(c("#0f0f0a")));
    assert_eq!((s.page, s.text, s.format), (None, None, Format::PlainList));
    assert_eq!(
        plain_list(lines[..15].join("\n").as_bytes()).map(|_| ()),
        Err(SchemeError {
            line: Line(15),
            kind: SchemeErrorKind::Count
        })
    );
}
