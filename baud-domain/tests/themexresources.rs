//! X resources, read as xrdb loads them: the C preprocessor subset, Xrm's
//! names and XParseColor's colours, each case one rule.

use baud_domain::Color;
use baud_domain::themefile::*;

fn c(h: &str) -> Color {
    hex(h.as_bytes()).unwrap()
}

/// The lines passed over: the file, the line, the key and why.
type Skips = Vec<(String, Line, String, Skipped)>;

/// `text` read as the file `main`, its includes from `files`.
fn read_with(
    text: &str,
    files: &[(&str, &str)],
) -> (Result<Scheme, XresourcesError<String>>, Skips) {
    let mut skipped = Vec::new();
    let r = xresources(
        text.as_bytes(),
        &"main".to_string(),
        |_: &String, name: &[u8]| {
            let name = String::from_utf8_lossy(name).into_owned();
            files
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(n, t)| (n.to_string(), t.as_bytes()))
        },
        |file: &String, l, k: &[u8], w| {
            skipped.push((file.clone(), l, String::from_utf8_lossy(k).into_owned(), w))
        },
    );
    (r, skipped)
}

fn read(text: &str) -> (Result<Scheme, XresourcesError<String>>, Skips) {
    read_with(text, &[])
}

/// tinted-xresources b97e7eca's base16-0x96f, verbatim, and what clang's
/// `cpp -P -DCOLOR` prints for it.
const TINTED: &str = include_str!("../../tests/themes/base16-0x96f.Xresources");
const TINTED_CPP: &str = include_str!("../../tests/themes/base16-0x96f.cpp");

/// **A tinted file reads as cpp makes it**: the slots its `#define`s name,
/// the `#else` branch where `background_opacity` is not defined, and the
/// same scheme as cpp's own output read with no directive left in it.
#[test]
fn a_tinted_file_reads_what_cpp_p_dcolor_gives() {
    let (s, skipped) = read(TINTED);
    let s = s.unwrap();
    let (cpp, _) = read(TINTED_CPP);
    assert_eq!(s, cpp.unwrap());
    assert_eq!(s.format, Format::Xresources);
    assert_eq!(s.page, Some(c("#262427")));
    assert_eq!(s.text, Some(c("#fcfcfc")));
    assert_eq!(s.cursor, Some(c("#fcfcfc")));
    assert_eq!(s.ansi[1], Some(c("#ff7272")));
    assert_eq!(s.ansi[15], Some(c("#fcfcfc")));
    assert_eq!(s.extended[5], Some(c("#eae9eb")));
    assert!(s.ansi.iter().all(Option::is_some));
    assert!(skipped.is_empty(), "{skipped:?}");
    assert!(TINTED_CPP.contains("*background: #262427\n"));
}

/// **`#if`, `#elif`, a macro with arguments and `#include <f>` refuse the
/// file by name**, at their line; in a group cpp skips, `#if` only nests.
#[test]
fn what_the_subset_does_not_run_is_refused_by_name() {
    for (text, line, why) in [
        ("*color0: #000000\n#if COLOR\n#endif\n", 2, Cpp::If),
        ("#ifdef COLOR\n#elif 1\n#endif\n", 2, Cpp::Elif),
        ("#define RGB(r) r\n", 1, Cpp::MacroArguments),
        ("#include <colors>\n", 1, Cpp::AngleInclude),
        ("#endif\n", 1, Cpp::Unmatched),
        ("#ifdef COLOR\n*color0: #000000\n", 2, Cpp::Unterminated),
        ("/* open\n*color0: #000000\n", 1, Cpp::Unterminated),
    ] {
        let (r, _) = read(text);
        assert_eq!(
            r,
            Err((
                None,
                SchemeError {
                    line: Line(line),
                    kind: SchemeErrorKind::Cpp(why)
                }
            )),
            "{text:?}"
        );
    }
    let (s, _) = read("#ifndef COLOR\n#if 0\n#elif 1\n#endif\n#else\n*color0: #010203\n#endif\n");
    assert_eq!(s.unwrap().ansi[0], Some(c("#010203")));
}

/// **Comments, splices, `#undef` and rescanning are cpp's**: a `/* */` is
/// a space and may span lines, `//` runs to the line's end, a name is
/// replaced again inside what replaced it but never inside itself, and
/// nothing is replaced inside quotes or a number.
#[test]
fn comments_splices_and_rescanning_are_cpps() {
    let (s, skipped) = read(
        "#define red #ff0000 /* a comment */\n#define warm red\n\
         *color1: warm // trailing\n*color2: /* two\nlines */ #00ff00\n\
         *col\\\nor3: #0000ff\n#undef red\n#ifdef red\n*color4: #ffffff\n#endif\n\
         #define loop loop\n*color5: #loop\n*color6: #a0b0c0\n",
    );
    let s = s.unwrap();
    assert_eq!(s.ansi[1], Some(c("#ff0000")));
    assert_eq!(s.ansi[2], Some(c("#00ff00")));
    assert_eq!(s.ansi[3], Some(c("#0000ff")));
    assert_eq!(s.ansi[4], None);
    assert_eq!(s.ansi[6], Some(c("#a0b0c0")));
    // `#loop` is no colour X reads, so its line is skipped.
    assert_eq!(
        skipped
            .iter()
            .map(|(_, l, k, w)| (l.0, k.as_str(), *w))
            .collect::<Vec<_>>(),
        [(13, "*color5", Skipped::Refused)]
    );
}

/// An X colour name refuses the file, as a ghostty file's does: xterm
/// resolves it and baud does not yet -- `crimson` too, which the X server
/// holds and X.Org's rgb.txt at 749182c does not. So do `rgbi:` and the CIE
/// forms.
#[test]
fn a_colour_name_rgbi_and_cie_refuse_the_file() {
    for (v, kind) in [
        ("red", SchemeErrorKind::ColourName),
        ("crimson", SchemeErrorKind::ColourName),
        ("light blue", SchemeErrorKind::ColourName),
        ("rgbi:1/0/0", SchemeErrorKind::XcmsColour),
        ("CIEXYZ:0.3227/0.28133/0.2493", SchemeErrorKind::XcmsColour),
        ("TekHVC:0/50/0", SchemeErrorKind::XcmsColour),
    ] {
        let (r, _) = read(&format!("*color0: {v}\n"));
        assert_eq!(
            r,
            Err((
                None,
                SchemeError {
                    line: Line(1),
                    kind
                }
            )),
            "{v}"
        );
    }
}

/// **A 16-bit component is drawn as its high byte**, as xterm and the X
/// server reduce it: `#ab0000` is 0xab, `#fff` is `#f0f0f0` (the most
/// significant bits), `rgb:f/f/f` is 0xff (scaled), and the 9- and 12-digit
/// forms keep their first two digits.
#[test]
fn a_component_is_its_high_byte() {
    let (s, skipped) = read(
        "*color0: #ab0000\n*color1: #fff\n*color2: rgb:f/f/f\n*color3: #123456789\n\
         *color4: #1234abcd5678\n*color5: RGB:8/80/800\n*color6: rgb:1234/5/6/7\n\
         *color7: #12345\n*color8: rgb:12345/0/0\n*color9: [90]#000000\n",
    );
    let s = s.unwrap();
    assert_eq!(s.ansi[0], Some(c("#ab0000")));
    assert_eq!(s.ansi[1], Some(c("#f0f0f0")));
    assert_eq!(s.ansi[2], Some(c("#ffffff")));
    assert_eq!(s.ansi[3], Some(c("#124578")));
    assert_eq!(s.ansi[4], Some(c("#12ab56")));
    assert_eq!(s.ansi[5], Some(c("#888080")));
    assert_eq!(s.ansi[6], Some(c("#125566")));
    let refused: Vec<_> = skipped.iter().map(|(_, l, _, w)| (l.0, *w)).collect();
    assert_eq!(
        refused,
        [
            (8, Skipped::Refused),
            (9, Skipped::Refused),
            (10, Skipped::Refused)
        ]
    );
}

/// **Only a name every client matches is read**: `*color0` and
/// `*.color0`; a program's entry is passed over naming it, and a later
/// line wins.
#[test]
fn only_names_every_client_reads_are_read() {
    let (s, skipped) = read(
        "*.color0: #000001\n*color0: #000002\nURxvt*color1: #111111\n\
         XTerm*vt100.color2: #222222\n?.color3: #333333\n*font: fixed\n\
         *colorBD: #444444\nfoo: bar\nno colon here\n",
    );
    let s = s.unwrap();
    assert_eq!(s.ansi[0], Some(c("#000002")));
    assert_eq!(s.ansi[1..4], [None, None, None]);
    let got: Vec<_> = skipped
        .iter()
        .map(|(_, l, k, w)| (l.0, k.as_str(), *w))
        .collect();
    assert_eq!(
        got,
        [
            (3, "URxvt*color1", Skipped::Program),
            (4, "XTerm*vt100.color2", Skipped::Program),
            (5, "?.color3", Skipped::Program),
            (6, "*font", Skipped::NotAColour),
            (7, "*colorBD", Skipped::Unheld),
            (8, "foo", Skipped::NotAColour),
            (9, "no colon here", Skipped::Syntax),
        ]
    );
}

/// **`#include "f"` is read where it stands**, the macros carried across
/// both ways; a file that cannot be read is passed over and named, as Xrm
/// skips it; and an unknown directive is passed over, as cpp prints nothing
/// for it.
#[test]
fn an_include_is_read_where_it_stands() {
    let files = [("colours", "#define page #161719\n*.color1: #cc6666\n")];
    let (s, skipped) = read_with(
        "#define text #c5c8c7\n#include \"colours\"\n*.background: page\n\
         #include \"gone\"\n#pragma once\n*.foreground: text\n",
        &files,
    );
    let s = s.unwrap();
    assert_eq!(s.page, Some(c("#161719")));
    assert_eq!(s.text, Some(c("#c5c8c7")));
    assert_eq!(s.ansi[1], Some(c("#cc6666")));
    let got: Vec<_> = skipped
        .iter()
        .map(|(f, l, k, w)| (f.as_str(), l.0, k.as_str(), *w))
        .collect();
    assert_eq!(
        got,
        [
            ("main", 4, "gone", Skipped::NoFile),
            ("main", 5, "pragma", Skipped::Directive)
        ]
    );
    // A refusal inside an included file names that file.
    let (r, _) = read_with("#include \"bad\"\n", &[("bad", "\n#if 1\n#endif\n")]);
    assert_eq!(
        r,
        Err((
            Some("bad".to_string()),
            SchemeError {
                line: Line(2),
                kind: SchemeErrorKind::Cpp(Cpp::If)
            }
        ))
    );
}

/// **A file that includes itself stops at Xrm's depth, 100**: read 101
/// times, the first and 100 nested, and the include past that named once.
#[test]
fn a_self_include_stops_at_one_hundred() {
    let me = "*.color0: #000001\n#include \"main\"\n";
    let mut reads = 0;
    let mut skipped = Vec::new();
    let s = xresources(
        me.as_bytes(),
        &(),
        |_: &(), _: &[u8]| {
            reads += 1;
            Some(((), me.as_bytes()))
        },
        |_: &(), l, k: &[u8], w| skipped.push((l, k.to_vec(), w)),
    )
    .unwrap();
    assert_eq!(reads, 100);
    assert_eq!(XRM_DEPTH, 100);
    assert_eq!(skipped, [(Line(2), b"main".to_vec(), Skipped::Nested)]);
    assert_eq!(s.ansi[0], Some(c("#000001")));
}

/// Entries 16-255 land in `extended`, and `cursorColor`, `highlightColor`
/// and `highlightTextColor` are the cursor and the selection.
#[test]
fn every_colour_resource_lands_where_it_names() {
    let (s, _) = read(
        "*color16: #101010\n*color255: #ffffff\n*color256: #000000\n\
         *cursorColor: #010101\n*highlightColor: #020202\n*highlightTextColor: #030303\n",
    );
    let s = s.unwrap();
    assert_eq!(s.extended[0], Some(c("#101010")));
    assert_eq!(s.extended[239], Some(c("#ffffff")));
    assert_eq!(s.cursor, Some(c("#010101")));
    assert_eq!(s.selection, Some(c("#020202")));
    assert_eq!(s.selection_text, Some(c("#030303")));
}
