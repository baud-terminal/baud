//! alacritty's theme files, TOML and the YAML before 0.13: the colour
//! forms `Rgb::from_str` and its struct form take, the TOML and YAML shapes
//! an alacritty config uses, and what is skipped and why.

use baud_domain::Color;
use baud_domain::themefile::*;

fn c(h: &str) -> Color {
    hex(h.as_bytes()).unwrap()
}

/// The lines a reader skipped, each with why.
type Skips = Vec<(u32, String, Skipped)>;

fn toml(text: &str) -> (Result<Scheme, SchemeError>, Skips) {
    let mut skipped = Vec::new();
    let r = alacritty(text.as_bytes(), |l, k, w| {
        skipped.push((l.0, String::from_utf8_lossy(k).into_owned(), w))
    });
    (r, skipped)
}

fn yaml(text: &str) -> (Result<Scheme, SchemeError>, Skips) {
    let mut skipped = Vec::new();
    let r = alacritty_yaml(text.as_bytes(), |l, k, w| {
        skipped.push((l.0, String::from_utf8_lossy(k).into_owned(), w))
    });
    (r, skipped)
}

/// `#rrggbb` and `0xrrggbb`, in either string kind, `+` inside the six
/// as `from_str_radix` takes it, and `{ r, g, b }`; a three-digit form and
/// an unquoted integer are not alacritty colours.
#[test]
fn alacritty_reads_its_colour_forms() {
    let (s, skipped) = toml(
        "[colors.normal]\nblack = \"#1d1f21\"\nred = '0xcc6666'\ngreen = { r = 1, g = 2, b = 3 }\n\
         yellow = \"#+12345\"\nblue = \"#abc\"\nmagenta = 0x112233\n",
    );
    let s = s.unwrap();
    assert_eq!(s.ansi[0], Some(c("#1d1f21")));
    assert_eq!(s.ansi[1], Some(c("#cc6666")));
    assert_eq!(s.ansi[2], Some(c("#010203")));
    assert_eq!(s.ansi[3], Some(c("#012345")));
    assert_eq!((s.ansi[4], s.ansi[5]), (None, None));
    assert_eq!(
        skipped,
        [
            (6, "blue".into(), Skipped::Refused),
            (7, "magenta".into(), Skipped::Refused)
        ]
    );
}

/// The ways TOML spells one table: a header, dotted keys, an inline
/// table; `cursor` and `selection` take `text` and `cursor`/`background`,
/// and a cell-relative name the affected cell's colour.
#[test]
fn toml_spellings_land_on_the_same_slots() {
    let (s, skipped) = toml(
        "colors.primary.background = \"#161719\"\n[colors]\nprimary.foreground = \"#c5c8c7\"\n\
         cursor = { text = \"#1d1f22\", cursor = \"#aeafad\" }\n\
         selection = { text = \"CellForeground\", background = \"#373b41\" }\n",
    );
    let s = s.unwrap();
    assert_eq!((s.page, s.text), (Some(c("#161719")), Some(c("#c5c8c7"))));
    assert_eq!(
        (s.cursor_text, s.cursor),
        (
            Some(CellPaint::Colour(c("#1d1f22"))),
            Some(CellPaint::Colour(c("#aeafad")))
        )
    );
    assert_eq!(
        (s.selection_text, s.selection),
        (Some(CellPaint::Text), Some(CellPaint::Colour(c("#373b41"))))
    );
    assert_eq!(skipped, []);
}

/// `indexed_colors`, as an array across lines and as an array of tables;
/// an index below 16 is refused, as alacritty's `ColorIndex` refuses it.
#[test]
fn indexed_colors_both_ways_and_below_sixteen_refused() {
    let (s, skipped) = toml(
        "[colors]\nindexed_colors = [\n  { index = 16, color = \"#100000\" },\n  { index = 3, color = \"#030000\" },\n]\n\
         [[colors.indexed_colors]]\nindex = 255\ncolor = \"#ff0000\"\n",
    );
    let s = s.unwrap();
    assert_eq!(s.extended[0], Some(c("#100000")));
    assert_eq!(s.extended[239], Some(c("#ff0000")));
    assert_eq!(s.ansi[3], None);
    assert_eq!(skipped, [(4, "index".into(), Skipped::Refused)]);
}

/// The rest of a config is read past and said: other tables, multi-line
/// strings and arrays; alacritty's own colours with no place in a scheme.
#[test]
fn the_rest_of_a_config_is_read_past() {
    let (s, skipped) = toml(
        "[general]\nimport = [\n  \"a.toml\",\n  'b.toml',\n]\n[terminal.shell]\nargs = \"\"\"\nline = \"#000000\"\n\"\"\"\n\
         [colors.search.matches]\nbackground = \"#ffff00\"\n[colors.normal]\nblack = \"#000001\"\n",
    );
    let s = s.unwrap();
    assert_eq!((s.page, s.ansi[0]), (None, Some(c("#000001"))));
    assert_eq!(
        skipped,
        [
            (2, "import".into(), Skipped::NotAColour),
            (7, "args".into(), Skipped::NotAColour),
            (11, "background".into(), Skipped::Unheld),
        ]
    );
}

/// What is not TOML is refused at its line, as alacritty refuses it.
#[test]
fn what_is_not_toml_is_refused_at_its_line() {
    for (text, line) in [
        ("[colors.primary]\nbackground = \n", 2),
        ("[colors.primary]\nbackground = \"#161719\n", 2),
        ("[colors.primary\n", 1),
        ("[colors]\nprimary = { background = \"#000000\"\n", 2),
        ("[colors]\nnormal.black = \"#000000\" extra\n", 2),
    ] {
        assert_eq!(
            toml(text).0,
            Err(SchemeError {
                line: Line(line),
                kind: SchemeErrorKind::Syntax
            }),
            "{text:?}"
        );
    }
}

/// The YAML config: block mappings, a flow mapping, a sequence at its
/// key's own indentation, and a plain `0x` number, which YAML reads as an
/// integer and alacritty refuses.
#[test]
fn the_yaml_config_reads_its_colours() {
    let (s, skipped) = yaml(
        "font:\n  size: 12\ncolors:\n  primary:\n    background: '0x161719'\n    foreground: \"#c5c8c7\"\n\
         \x20 cursor: { text: CellBackground, cursor: '0xaeafad' }\n  normal:\n    black: 0x1d1f21\n\
         \x20   red: '0xcc6666' # red\n  indexed_colors:\n  - { index: 17, color: '0x110000' }\n",
    );
    let s = s.unwrap();
    assert_eq!((s.page, s.text), (Some(c("#161719")), Some(c("#c5c8c7"))));
    assert_eq!(
        (s.cursor_text, s.cursor),
        (Some(CellPaint::Page), Some(CellPaint::Colour(c("#aeafad"))))
    );
    assert_eq!((s.ansi[0], s.ansi[1]), (None, Some(c("#cc6666"))));
    assert_eq!(s.extended[1], Some(c("#110000")));
    assert_eq!(
        skipped,
        [
            (1, "font".into(), Skipped::NotAColour),
            (9, "black".into(), Skipped::Refused),
        ]
    );
}

/// **A TOML file must be UTF-8** (`[[source: toml-1-0-0]]` 44), so a byte
/// that is not refuses it, naming its line; the YAML reader, which no such
/// text binds here, reads it.
#[test]
fn a_toml_file_that_is_not_utf_8_is_refused_at_its_line() {
    let text = b"[colors.primary]\n# Th\xe8me\nbackground = '#1d1f21'\n";
    assert_eq!(
        alacritty(text, |_, _, _| {}),
        Err(SchemeError {
            line: Line(2),
            kind: SchemeErrorKind::NotUtf8
        })
    );
    let yml = b"# Th\xe8me\ncolors:\n  primary:\n    background: '#1d1f21'\n";
    let s = alacritty_yaml(yml, |_, _, _| {}).unwrap();
    assert_eq!(s.page, Some(c("#1d1f21")));
}
