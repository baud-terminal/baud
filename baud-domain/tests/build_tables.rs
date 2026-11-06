//! **Each check the build modules make on the pinned files fires**: each
//! module, run here on its files with one row changed, reports the failure
//! that change must cause, and reports nothing on the files as pinned.
//! `build/bidi.rs`: a dropped `@missing` line, a class outside
//! PropertyValueAliases.txt's `bc` set, an overlapping row, a known code
//! point with another class, a mirroring glyph on a character that is not
//! mirrored, a bracket that is not its pair's pair, a canonical
//! decomposition moved, a Box Drawing name with no counterpart.
//! `build/grapheme.rs`, `build/width.rs` and `build/linebreak.rs`: a value
//! outside its alias set, a GCB value `GraphemeClass` has no class for, an
//! emoji-data.txt of another version, a General_Category that names a
//! group or leaves a code point out, a header default the header does not
//! give, and the rules the tables are written by -- width's zero over wide
//! over ambiguous, and its two exclusions, U+00AD and
//! Prepended_Concatenation_Mark. `build/caseless.rs`, `build/search.rs`,
//! `build/sequence.rs`, `build/joining.rs` and `build/ignorable.rs`: each
//! positive control the Python generators asserted, each count made exact,
//! a status, type or group outside its set, a duplicate, a Hangul
//! syllable given a decomposition, the derived joining file disagreeing,
//! and the perfect hash's seeds tried from 0 as the table was found.
//! `build/colour_names.rs` and `build/placeholder.rs`: a channel past 255,
//! a name that is not ASCII or is missing, a spelling given twice, two
//! spellings in two cases with two colours, `debianred` already in
//! rgb.txt, a name past a slot's length, a pool past its offset, the 753
//! and 782 counts, the two rgb.txt pins swapped, and the diacritics'
//! count, first three, header, duplicates and blocks. `build/cluster.rs`:
//! a rule dropped from UAX #29's, the reachable state count, a state id
//! past seven bits, a ready state missing or twice. `build/merged.rs`: a
//! trie it reads missing, a class past `GraphemeClass`'s, a palette or a
//! block id past a byte, and the code points it is checked at.
//! `build/dec_graphics.rs`, `build/dec_sets.rs`, `build/nrcs.rs` and
//! `build/iso96.rs`: every refusal of the xterm macro readers -- a line
//! that is no entry, an entry they cannot read, a byte twice, a macro with
//! no body or two, a keysym with no `#define` or past Latin-1, a `/* U+ */`
//! comment that is not its keysym, a skip or an override the source no
//! longer bears out, an `UNDEF` line neither emitted nor skipped -- the
//! headers, the counts, Figure A-2's mask, JIS0201.TXT's lines, and the
//! rule that the cells where xterm and DEC's text differ are exactly the
//! cells named, over DEC's Table 2-3 as `vt220_tables.tsv` transcribes it.
//! `build/tcap.rs`, `build/termcap_keys.rs` and `build/iterm_keys.rs`:
//! every refusal of the xtermcap.c table reader -- the table not found, a
//! row or a group not read, a state neither 0 nor SHIFT, a group with no
//! BASE, the 110 and 17 counts -- a keysym with no key, a name past seven
//! bytes, the 319 names, the multiplier search, the entry's line 2, its
//! kcuu1 and kf13 controls, an escape terminfo(5) does not define, a
//! capability twice, the 107 keys, and in the iTerm2 names a key that is
//! not an identifier, the grammar's placeholder, a key twice or out of
//! order, a page that is none of the four, each page's and the source's
//! controls, the 39 and 54 counts and both headers.
//! The modules only compute: they read
//! through the function they are given and write nothing, so running them
//! here touches no `OUT_DIR`.

#[path = "../build/bidi.rs"]
mod bidi;
#[path = "../build/caseless.rs"]
mod caseless;
#[path = "../build/cluster.rs"]
mod cluster;
#[path = "../build/colour_names.rs"]
mod colour_names;
#[path = "../build/dec_graphics.rs"]
mod dec_graphics;
#[path = "../build/dec_sets.rs"]
mod dec_sets;
#[path = "../build/grapheme.rs"]
mod grapheme;
#[path = "../build/ignorable.rs"]
mod ignorable;
#[path = "../build/iso96.rs"]
mod iso96;
#[path = "../build/iterm_keys.rs"]
mod iterm_keys;
#[path = "../build/joining.rs"]
mod joining;
#[path = "../build/linebreak.rs"]
mod linebreak;
#[path = "../build/merged.rs"]
mod merged;
#[path = "../build/nrcs.rs"]
mod nrcs;
#[path = "../build/placeholder.rs"]
mod placeholder;
#[path = "../build/py_random.rs"]
mod py_random;
#[path = "../build/search.rs"]
mod search;
#[path = "../build/sequence.rs"]
mod sequence;
#[path = "../build/tcap.rs"]
mod tcap;
#[path = "../build/termcap_keys.rs"]
mod termcap_keys;
#[path = "../build/ucd.rs"]
mod ucd;
#[path = "../build/width.rs"]
mod width;
#[path = "../build/xterm.rs"]
mod xterm;
#[path = "../build/xtermcap.rs"]
mod xtermcap;

use baud_tablegen::{Emit, Errors, Kind};

fn pinned(file: &str) -> std::io::Result<String> {
    // Through the crate's link where it has one, since a link's name need
    // not be its file's: `terminfo-xterm-256color.ti` is
    // `terminfo/xterm-256color.ti`.
    let data = format!("{}/data/{file}", env!("CARGO_MANIFEST_DIR"));
    if std::path::Path::new(&data).exists() {
        return std::fs::read_to_string(data);
    }
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources");
    std::fs::read_to_string(format!("{dir}/{file}"))
}

type Build = fn(&dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors>;

/// A build whose `file` has its first `find` replaced.
fn perturbed(build: Build, file: &str, find: &str, replace: &str) -> Result<Emit, Errors> {
    let read = |f: &str| {
        let text = pinned(f)?;
        if f != file {
            return Ok(text);
        }
        assert!(text.contains(find), "{file} has no {find:?}");
        Ok(text.replacen(find, replace, 1))
    };
    build(&read)
}

/// The failures of `build` when `file` has its first `find` replaced.
fn failures_of(build: Build, file: &str, find: &str, replace: &str) -> Vec<(Kind, String)> {
    match perturbed(build, file, find, replace) {
        Ok(_) => Vec::new(),
        Err(errs) => errs
            .list()
            .iter()
            .map(|e| (e.kind, e.to_string()))
            .collect(),
    }
}

/// The failures of the bidi build whose `file` has its first `find`
/// replaced.
fn failures(file: &str, find: &str, replace: &str) -> Vec<(Kind, String)> {
    failures_of(bidi::build, file, find, replace)
}

/// Each case is `(build, file, find, replace, kind, says)`: the build with
/// `file`'s first `find` replaced must report a `kind` failure saying `says`.
type Case<'a> = (Build, &'a str, &'a str, &'a str, Kind, &'a str);

fn each_fires(cases: &[Case<'_>]) {
    let mut missed = Vec::new();
    for case in cases {
        let (build, file, find, replace, kind, says) = *case;
        let got = failures_of(build, file, find, replace);
        if !got.iter().any(|(k, m)| *k == kind && m.contains(says)) {
            missed.push(format!(
                "{file}: {find:?} -> {replace:?}: no {kind:?} saying {says:?} in {got:#?}"
            ));
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}

/// The bytes of `emit`'s `<name>.bin`.
fn bin(emit: &Emit, name: &str) -> Vec<u8> {
    let file = format!("{name}.bin");
    emit.files()
        .into_iter()
        .find(|(f, _)| *f == file)
        .unwrap_or_else(|| panic!("no {file}"))
        .1
        .to_vec()
}

const CLASSES: &str = "unicode-derivedbidiclass.txt";
const DATA: &str = "unicode-unicodedata.txt";
const MIRRORING: &str = "unicode-bidimirroring.txt";
const BRACKETS: &str = "unicode-bidibrackets.txt";
const ALIASES: &str = "unicode-propertyvaluealiases.txt";

#[test]
fn the_pinned_files_build_the_bidi_tables() {
    let emit = bidi::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    let files: Vec<String> = emit.files().into_iter().map(|(f, _)| f).collect();
    assert_eq!(files, ["bidi_data.rs", "bidi_index.bin", "bidi_blocks.bin"]);
    assert!(emit.rust().contains("one of 122 distinct blocks"));
    assert_eq!(
        bidi::INPUTS.map(|i| i.file),
        [ALIASES, CLASSES, DATA, MIRRORING, BRACKETS]
    );
}

#[test]
fn each_check_the_bidi_build_makes_fires() {
    let cases: &[(&str, &str, &str, Kind, &str)] = &[
        // The positive control on the version wired in.
        (
            CLASSES,
            "# DerivedBidiClass-17.0.0.txt",
            "# DerivedBidiClass-16.0.0.txt",
            Kind::Header,
            "data/unicode-derivedbidiclass.txt:1:1",
        ),
        // 24 `@missing` lines.
        (
            CLASSES,
            "# @missing: 20A0..20CF; European_Terminator",
            "#",
            Kind::Count,
            "23 @missing lines, 24 expected",
        ),
        (
            CLASSES,
            "# @missing: 20A0..20CF; European_Terminator",
            "#",
            Kind::Control,
            "U+20CF is Bidi_Class L, ET expected",
        ),
        // A class the alias file does not list, in a row and in a default.
        (
            CLASSES,
            "05D0..05EA    ; R #",
            "05D0..05EA    ; RTL #",
            Kind::UnknownValue,
            "data/unicode-derivedbidiclass.txt:1243:17: bc `RTL` is not one of the 23 values of data/unicode-propertyvaluealiases.txt:106-128",
        ),
        (
            CLASSES,
            "# @missing: 0590..05FF; Right_To_Left",
            "# @missing: 0590..05FF; Right_To_Lft",
            Kind::UnknownValue,
            "`Right_To_Lft`",
        ),
        // Two rows over one code point.
        (
            CLASSES,
            "05EF..05F2    ; R #",
            "05EF..05F2    ; R\n05F0..05F2    ; AL #",
            Kind::Overlap,
            "U+05F0 is already set by line 1244",
        ),
        // A known code point's class.
        (
            CLASSES,
            "# @missing: 0590..05FF; Right_To_Left",
            "# @missing: 0590..05FF; Left_To_Right",
            Kind::Control,
            "U+05FF is Bidi_Class L, R expected",
        ),
        // The alias file's `bc` set is the table's order.
        (
            ALIASES,
            "bc ; WS                               ; White_Space\n",
            "",
            Kind::Invariant,
            "[\"WS\"] are not values",
        ),
        // UnicodeData.txt: its count, its Y/N, and the mirrored controls.
        (
            DATA,
            "0041;LATIN CAPITAL LETTER A;Lu;0;L;;;;;N;;;;0061;\n",
            "",
            Kind::Count,
            "40554 UnicodeData.txt entries, 40555 expected",
        ),
        (
            DATA,
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;Y;",
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;X;",
            Kind::UnknownValue,
            "Bidi_Mirrored `X`",
        ),
        (
            DATA,
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;Y;",
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;N;",
            Kind::Control,
            "U+0028 is Bidi_Mirrored N",
        ),
        (
            DATA,
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;Y;",
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;N;",
            Kind::Invariant,
            "U+0028 has a mirroring glyph and is not Bidi_Mirrored",
        ),
        (
            DATA,
            "4E00;<CJK Ideograph, First>",
            "4E00;<CJK Ideograph>",
            Kind::FirstWithoutLast,
            "does not follow its First",
        ),
        // BidiMirroring.txt: 428 glyphs, each once, and none for U+2211.
        (
            MIRRORING,
            "0028; 0029 # LEFT PARENTHESIS\n",
            "",
            Kind::Count,
            "427 mirroring glyphs, 428 expected",
        ),
        (
            MIRRORING,
            "0029; 0028 #",
            "0028; 0029 #",
            Kind::Duplicate,
            "U+0028 is already set by line 76",
        ),
        (
            MIRRORING,
            "0028; 0029 #",
            "2211; 2211\n0028; 0029 #",
            Kind::Control,
            "U+2211's mirroring glyph is Some(2211), None expected",
        ),
        // BidiBrackets.txt: 128 brackets, 64 open, each its pair's pair.
        (
            BRACKETS,
            "0028; 0029; o",
            "0028; 0029; n",
            Kind::Count,
            "127 paired brackets, 128 expected",
        ),
        (
            BRACKETS,
            "0028; 0029; o",
            "0028; 0029; n",
            Kind::Count,
            "63 opening brackets, 64 expected",
        ),
        (
            BRACKETS,
            "0028; 0029; o",
            "0028; 0029; x",
            Kind::UnknownValue,
            "Bidi_Paired_Bracket_Type `x`",
        ),
        (
            BRACKETS,
            "0029; 0028; c",
            "0029; 005B; c",
            Kind::Invariant,
            "bracket U+0029 is not its pair's pair",
        ),
        (
            BRACKETS,
            "0029; 0028; c",
            "0029; 005B; c",
            Kind::Invariant,
            "bracket U+0029 is not its pair's mirroring glyph",
        ),
        // BD16's two canonical equivalents.
        (
            DATA,
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;3008;",
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;3009;",
            Kind::Invariant,
            "the brackets' canonical decompositions are",
        ),
        // Box Drawing: each name draws something of its own, and has a
        // counterpart.
        (
            DATA,
            "2501;BOX DRAWINGS HEAVY HORIZONTAL;",
            "2501;BOX DRAWINGS LIGHT HORIZONTAL;",
            Kind::Duplicate,
            "U+2501 draws what U+2500 draws",
        ),
        (
            DATA,
            "250C;BOX DRAWINGS LIGHT DOWN AND RIGHT;",
            "250C;BOX DRAWINGS LIGHT DOWN AND RIGHTWARDS;",
            Kind::Invariant,
            "U+2510 has no counterpart",
        ),
        (
            DATA,
            "2501;BOX DRAWINGS HEAVY HORIZONTAL;",
            "2501;BOX DRAWINGS LIGHT HORIZONTAL;",
            Kind::Invariant,
            "U+2500's counterpart's counterpart is not itself",
        ),
        (
            DATA,
            "2501;BOX DRAWINGS HEAVY HORIZONTAL;",
            "2501;HEAVY HORIZONTAL;",
            Kind::Invariant,
            "a Box Drawing name without BOX DRAWINGS",
        ),
        (
            DATA,
            "2501;BOX DRAWINGS HEAVY HORIZONTAL;So;0;ON;;;;;N;FORMS HEAVY HORIZONTAL;;;;\n",
            "",
            Kind::Count,
            "127 Box Drawing names, 128 expected",
        ),
        // A bracket is bc=ON and Bidi_Mirrored.
        (
            DATA,
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;Y;",
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;N;",
            Kind::Invariant,
            "bracket U+0028 is not bc=ON and Bidi_M=Y",
        ),
        // U+2329 without its decomposition takes a pair of its own: 64
        // pairs, and BD16's equivalence lost.
        (
            DATA,
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;3008;",
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;;",
            Kind::Invariant,
            "64 bracket pairs, up to Some(64): 63 below 64 expected",
        ),
        (
            DATA,
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;3008;",
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;;",
            Kind::Control,
            "U+0028/U+0029 and U+2329/U+3009 are not paired as BD16 says",
        ),
    ];
    let mut missed = Vec::new();
    for case in cases {
        let (file, find, replace, kind, says) = *case;
        let got = failures(file, find, replace);
        if !got.iter().any(|(k, m)| *k == kind && m.contains(says)) {
            missed.push(format!(
                "{file}: {find:?} -> {replace:?}: no {kind:?} saying {says:?} in {got:#?}"
            ));
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}

/// **A mirroring glyph past nine bits is refused**: 83 more distinct
/// glyphs make 512 with the 0 for none, which MIRROR's low nine bits
/// cannot index.
#[test]
fn a_512th_mirroring_glyph_does_not_fit() {
    let more: String = (0..83)
        .map(|i| format!("{:04X}; {:04X}\n", 0xE000 + i, 0xF000 + i))
        .collect();
    let got = failures(MIRRORING, "0028; 0029 #", &format!("{more}0028; 0029 #"));
    assert!(
        got.iter()
            .any(|(k, m)| *k == Kind::Invariant && m.contains("512 glyphs will not fit nine bits")),
        "{got:#?}"
    );
}

const EAW: &str = "unicode-eastasianwidth.txt";
const EAW8: &str = "unicode-eastasianwidth-8.0.0.txt";
const GC: &str = "unicode-derivedgeneralcategory.txt";
const PROPS: &str = "unicode-proplist.txt";
const EMOJI: &str = "unicode-emoji-data.txt";
const GBP: &str = "unicode-graphemebreakproperty.txt";
const DCP: &str = "unicode-derivedcoreproperties.txt";
const LB: &str = "unicode-linebreak.txt";

/// The width `emit`'s `<table>` gives `cp`, read as `Width::of` reads it.
fn width_of(emit: &Emit, table: &str, cp: u32) -> u8 {
    let (index, blocks) = (
        bin(emit, &format!("{table}_index")),
        bin(emit, &format!("{table}_blocks")),
    );
    width::packed_get(&index, &blocks, usize::try_from(cp).expect("a code point"))
}

const NARROW: u8 = 0;
const ZERO: u8 = 1;
const WIDE: u8 = 2;
const AMBIGUOUS: u8 = 3;

#[test]
fn the_pinned_files_build_the_unicode_tables() {
    let names = |e: Emit| -> Vec<String> { e.files().into_iter().map(|(f, _)| f).collect() };
    let g = grapheme::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        g.rust().contains("one of 111 distinct blocks"),
        "{}",
        g.rust()
    );
    assert_eq!(
        names(g),
        ["grapheme_data.rs", "break_index.bin", "break_blocks.bin"]
    );
    let w = width::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        names(w),
        [
            "width_data.rs",
            "width_index.bin",
            "width_blocks.bin",
            "width8_index.bin",
            "width8_blocks.bin"
        ]
    );
    let l = linebreak::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        l.rust().contains("one of 144 distinct blocks"),
        "{}",
        l.rust()
    );
    assert_eq!(
        names(l),
        ["linebreak_data.rs", "line_index.bin", "line_blocks.bin"]
    );
}

/// **Zero over wide over ambiguous**, the rule `width` is, at every
/// combination of the three.
#[test]
fn width_takes_zero_over_wide_over_ambiguous() {
    for (zero, wide, ambiguous, want) in [
        (true, true, true, ZERO),
        (true, true, false, ZERO),
        (true, false, true, ZERO),
        (true, false, false, ZERO),
        (false, true, true, WIDE),
        (false, true, false, WIDE),
        (false, false, true, AMBIGUOUS),
        (false, false, false, NARROW),
    ] {
        assert_eq!(
            width::width(zero, wide, ambiguous),
            want,
            "{zero} {wide} {ambiguous}"
        );
    }
}

/// The rule in the tables: U+0300 is Mn and East_Asian_Width A, U+302A Mn
/// and W, and both are zero. No code point of 17.0.0 is both wide and
/// ambiguous -- East_Asian_Width gives one value, and no Emoji_Presentation
/// character is A -- so U+00A7 SECTION SIGN, A, is made Emoji_Presentation
/// here, and is wide under the current tables and still ambiguous under
/// Unicode 8's, which read no Emoji_Presentation.
#[test]
fn the_width_tables_take_zero_over_wide_over_ambiguous() {
    let w = width::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    for table in ["width", "width8"] {
        assert_eq!(width_of(&w, table, 0x0300), ZERO, "{table} U+0300");
        assert_eq!(width_of(&w, table, 0x302A), ZERO, "{table} U+302A");
        assert_eq!(width_of(&w, table, 0x00A7), AMBIGUOUS, "{table} U+00A7");
    }
    let row = "231A..231B    ; Emoji_Presentation";
    let w = perturbed(
        width::build,
        EMOJI,
        row,
        &format!("00A7 ; Emoji_Presentation\n{row}"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(width_of(&w, "width", 0x00A7), WIDE);
    assert_eq!(width_of(&w, "width8", 0x00A7), AMBIGUOUS);
}

/// **The two exclusions from zero.** U+00AD SOFT HYPHEN is Cf and takes a
/// column in both tables, its East_Asian_Width's A; each
/// Prepended_Concatenation_Mark, Cf too, takes a column under the current
/// tables and none under Unicode 8's, whose zero set it does not narrow.
#[test]
fn soft_hyphen_and_the_concatenation_marks_are_not_zero() {
    let w = width::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(width_of(&w, "width", 0x00AD), AMBIGUOUS);
    assert_eq!(width_of(&w, "width8", 0x00AD), AMBIGUOUS);
    let marks = [
        0x0600, 0x0601, 0x0602, 0x0603, 0x0604, 0x0605, 0x06DD, 0x070F, 0x0890, 0x0891, 0x08E2,
        0x110BD, 0x110CD,
    ];
    for cp in marks {
        assert_eq!(width_of(&w, "width", cp), NARROW, "U+{cp:04X}");
        assert_eq!(width_of(&w, "width8", cp), ZERO, "U+{cp:04X}");
    }
    // A Cf beside them is zero in both: U+200B ZERO WIDTH SPACE.
    assert_eq!(width_of(&w, "width", 0x200B), ZERO);
    // And without the PropList row the mark is zero again, which the
    // build's own control refuses.
    let got = failures_of(
        width::build,
        PROPS,
        "0600..0605    ; Prepended_Concatenation_Mark",
        "0600..0605    ; Other_Math",
    );
    assert!(
        got.iter().any(|(k, m)| *k == Kind::Control
            && m.contains("U+0600 (ARABIC NUMBER SIGN, a PCM) is (1, 1)")),
        "{got:#?}"
    );
}

#[test]
fn each_check_the_unicode_builds_make_fires() {
    let g: Build = grapheme::build;
    let w: Build = width::build;
    let l: Build = linebreak::build;
    each_fires(&[
        // The versions wired in.
        (
            w,
            EAW,
            "# EastAsianWidth-17.0.0.txt",
            "# EastAsianWidth-16.0.0.txt",
            Kind::Header,
            "data/unicode-eastasianwidth.txt:1:1",
        ),
        (
            g,
            GBP,
            "# GraphemeBreakProperty-17.0.0.txt",
            "# GraphemeBreakProperty-16.0.0.txt",
            Kind::Header,
            "data/unicode-graphemebreakproperty.txt:1:1",
        ),
        (
            l,
            LB,
            "# LineBreak-17.0.0.txt",
            "# LineBreak-16.0.0.txt",
            Kind::Header,
            "data/unicode-linebreak.txt:1:1",
        ),
        (
            w,
            EMOJI,
            "# Version: 17.0",
            "# Version: 18.0",
            Kind::Header,
            "no \"# Version: 17.0\" header line",
        ),
        // East_Asian_Width: its value set, its default, its rows, the
        // header's W defaults.
        (
            w,
            EAW,
            "00A7           ; A ",
            "00A7           ; Amb ",
            Kind::UnknownValue,
            "data/unicode-eastasianwidth.txt:78:18: ea `Amb` is not one of the 6 values",
        ),
        (w, EAW8, "00A7;A ", "00A7;X ", Kind::UnknownValue, "ea `X`"),
        (
            w,
            EAW,
            "# @missing: 0000..10FFFF; N",
            "#",
            Kind::Count,
            "0 @missing lines, 1 expected",
        ),
        (
            w,
            EAW,
            "00A7           ; A ",
            "00A7           ; A\n00A6..00A7 ; N ",
            Kind::Overlap,
            "data/unicode-eastasianwidth.txt:79:1: U+00A6 is already set by line 77",
        ),
        (
            w,
            EAW8,
            "U+3400..U+4DBF",
            "U+3400..U+4DB5",
            Kind::Control,
            "the header does not give U+3400..U+4DBF as defaulting to \"W\"",
        ),
        (
            w,
            EAW,
            "0041..005A     ; Na",
            "0041..005A     ; W",
            Kind::Control,
            "U+0041 (LATIN CAPITAL LETTER A) is (2, 0)",
        ),
        // General_Category: a group is no code point's, and every code
        // point is listed.
        (
            w,
            GC,
            "0300..036F    ; Mn",
            "0300..036F    ; M",
            Kind::UnknownValue,
            "gc `M` is a group of categories",
        ),
        (
            l,
            GC,
            "0300..036F    ; Mn",
            "0300..036F    ; Nonspacing",
            Kind::UnknownValue,
            "gc `Nonspacing` is not one of the 38 values",
        ),
        (
            g,
            GC,
            "D800..DFFF    ; Cs",
            "#",
            Kind::Invariant,
            "U+D800 has no value",
        ),
        // PropList: the concatenation marks' rows.
        (
            w,
            PROPS,
            "110CD         ; Prepended_Concatenation_Mark",
            "110CD         ; Other_Math",
            Kind::Count,
            "6 Prepended_Concatenation_Mark rows, 7 expected",
        ),
        // emoji-data.txt: its properties and its rows.
        (
            w,
            EMOJI,
            "231A..231B    ; Emoji_Presentation",
            "231A..231B    ; Emoji_Presentaton",
            Kind::UnknownValue,
            "emoji-data.txt property `Emoji_Presentaton`",
        ),
        (
            w,
            EMOJI,
            "231A..231B    ; Emoji_Presentation",
            "231A..231B    ; Emoji_Presentation\n231B ; Emoji_Presentation",
            Kind::Duplicate,
            "U+231B is already set by line 449",
        ),
        // Grapheme_Cluster_Break: long names, the default, a class for
        // each value.
        (
            g,
            GBP,
            "; Regional_Indicator #",
            "; RI #",
            Kind::UnknownValue,
            "GCB `RI` is not one of the 18 values",
        ),
        (
            g,
            GBP,
            "; Regional_Indicator #",
            "; E_Base #",
            Kind::UnknownValue,
            "GCB `E_Base` has no GraphemeClass",
        ),
        (
            g,
            GBP,
            "# @missing: 0000..10FFFF; Other",
            "#",
            Kind::Invariant,
            "has no value, and no default covers it",
        ),
        (
            g,
            GBP,
            "0903          ; SpacingMark",
            "0903          ; Extend",
            Kind::Control,
            "U+0903 is 0x04, SpacingMark with lookback 0 expected",
        ),
        (
            g,
            GBP,
            "09BE          ; Extend",
            "09BE          ; SpacingMark",
            Kind::Control,
            "U+09BE BENGALI VOWEL SIGN AA is not the first spacing Extend",
        ),
        (
            g,
            GBP,
            "0BBE          ; Extend",
            "0BBE          ; SpacingMark",
            Kind::Count,
            "45 spacing Extend ranges, 46 expected",
        ),
        // InCB: its four values, three fields, 505 rows.
        (
            g,
            DCP,
            "0915..0939    ; InCB; Consonant",
            "0915..0939    ; InCB; Consonnant",
            Kind::UnknownValue,
            "InCB `Consonnant` is not one of the 4 values",
        ),
        (
            g,
            DCP,
            "0915..0939    ; InCB; Consonant",
            "0915..0939    ; InCB",
            Kind::FieldCount,
            "2 fields, 3 expected",
        ),
        (
            g,
            DCP,
            "0915..0939    ; InCB; Consonant",
            "0915..0939    ; Alphabetic",
            Kind::Count,
            "504 InCB rows, 505 expected",
        ),
        (
            g,
            DCP,
            "094D          ; InCB; Linker",
            "094D          ; InCB; Extend",
            Kind::Control,
            "U+094D is 0x84, Extend with lookback 2 expected",
        ),
        // Line_Break: its value set, its default, LB1.
        (
            l,
            LB,
            "0E31           ; SA ",
            "0E31           ; Sa ",
            Kind::UnknownValue,
            "lb `Sa` is not one of the 49 values",
        ),
        (
            l,
            LB,
            "# @missing: 0000..10FFFF; XX",
            "#",
            Kind::Count,
            "0 @missing lines, 1 expected",
        ),
        (
            l,
            LB,
            "0041..005A     ; AL",
            "0041..005A     ; AL\n005A ; AL",
            Kind::Duplicate,
            "U+005A is already set by line 81",
        ),
        (
            l,
            LB,
            "0E31           ; SA ",
            "0E31           ; AL ",
            Kind::Control,
            "U+0E31 is 0x00, CM with flags 0x00 expected",
        ),
    ]);
    // An emoji-data.txt of another version: 18.0's, beside 17.0's in the
    // sources, is refused by its version line.
    let read = |f: &str| {
        pinned(if f == EMOJI {
            "ucd18-emoji-data.txt"
        } else {
            f
        })
    };
    for build in [g, w, l] {
        let errs = build(&read).expect_err("refused");
        assert!(errs.list().iter().any(|e| e.kind == Kind::Header), "{errs}");
    }
}

const CASEFOLDING: &str = "unicode-casefolding.txt";
const SHAPING: &str = "unicode-arabicshaping.txt";
const DJT: &str = "unicode-derivedjoiningtype.txt";
const SEQUENCES: &str = "emoji17-sequences.txt";
const ZWJ: &str = "emoji17-zwj-sequences.txt";
const VARIATION: &str = "unicode-emoji-variation-sequences.txt";

#[test]
fn the_pinned_files_build_the_five_tables() {
    let names: Vec<Vec<String>> = [
        caseless::build as Build,
        search::build,
        sequence::build,
        joining::build,
        ignorable::build,
    ]
    .iter()
    .map(|b| {
        b(&pinned)
            .unwrap_or_else(|e| panic!("{e}"))
            .files()
            .into_iter()
            .map(|(f, _)| f)
            .collect()
    })
    .collect();
    assert_eq!(
        names,
        [
            vec!["caseless_data.rs"],
            vec!["search_data.rs"],
            vec!["sequence_data.rs"],
            vec!["joining_data.rs", "joining_index.bin", "joining_blocks.bin"],
            vec!["ignorable_data.rs"],
        ]
    );
    let inputs: Vec<Vec<&str>> = [
        &caseless::INPUTS[..],
        &search::INPUTS,
        &sequence::INPUTS,
        &joining::INPUTS,
        &ignorable::INPUTS,
    ]
    .iter()
    .map(|i| i.iter().map(|i| i.file).collect())
    .collect();
    assert_eq!(
        inputs,
        [
            vec![CASEFOLDING],
            vec![DATA, CASEFOLDING],
            vec![SEQUENCES, ZWJ, VARIATION],
            vec![ALIASES, GC, SHAPING, DJT],
            vec![DCP],
        ]
    );
}

#[test]
fn each_check_the_five_builds_make_fires() {
    let (c, s, q, j, i): (Build, Build, Build, Build, Build) = (
        caseless::build,
        search::build,
        sequence::build,
        joining::build,
        ignorable::build,
    );
    each_fires(&[
        // CaseFolding.txt: the version, a status outside the four, a second
        // full folding, a mapping that is not code points, and each
        // control.
        (
            c,
            CASEFOLDING,
            "# CaseFolding-17.0.0.txt",
            "# CaseFolding-16.0.0.txt",
            Kind::Header,
            "is this the pinned file [unicode-casefolding]?",
        ),
        (
            c,
            CASEFOLDING,
            "0041; C; 0061;",
            "0041; X; 0061;",
            Kind::UnknownValue,
            "data/unicode-casefolding.txt:63:7: CaseFolding.txt status `X`",
        ),
        (
            s,
            CASEFOLDING,
            "0041; C; 0061;",
            "0041; C; 0061; #\n0041; F; 0061;",
            Kind::Duplicate,
            "U+0041 already has a full folding on line 63",
        ),
        (
            c,
            CASEFOLDING,
            "00DF; F; 0073 0073;",
            "00DF; F; 0073 73;",
            Kind::Malformed,
            "data/unicode-casefolding.txt:121:10: \"73\" is not a code point",
        ),
        (
            c,
            CASEFOLDING,
            "00DF; F; 0073 0073;",
            "00DF; F; 0073;",
            Kind::Control,
            "U+00DF folds to Some([73]), [73, 73] expected",
        ),
        (
            c,
            CASEFOLDING,
            "0130; F; 0069 0307;",
            "0130; S; 0069 0307;",
            Kind::Control,
            "U+0130 folds to None",
        ),
        (
            c,
            CASEFOLDING,
            "212A; C; 006B;",
            "212A; C; 004B;",
            Kind::Control,
            "U+212A folds to Some([4B])",
        ),
        (
            c,
            CASEFOLDING,
            "00C5; C; 00E5;",
            "00C5; F; 0061 030A;",
            Kind::Control,
            "U+00C5 folds to Some([61, 30A])",
        ),
        (
            c,
            CASEFOLDING,
            "0049; T; 0131;",
            "0049; F; 0131;",
            Kind::Duplicate,
            "U+0049 already has a full folding",
        ),
        (
            s,
            CASEFOLDING,
            "1E9E; F; 0073 0073;",
            "1E9E; F; 0073;",
            Kind::Control,
            "U+1E9E folds to [73], [73, 73] expected",
        ),
        (
            s,
            CASEFOLDING,
            "0041; C; 0061;",
            "0041; C; AC00;",
            Kind::Invariant,
            "U+0041 folds to a Hangul syllable",
        ),
        // UnicodeData.txt for the search keys: a decomposition moved, a
        // combining class that is not one, a Hangul syllable listed.
        (
            s,
            DATA,
            "212B;ANGSTROM SIGN;Lu;0;L;00C5;",
            "212B;ANGSTROM SIGN;Lu;0;L;0041;",
            Kind::Control,
            "U+212B decomposes to [41], [41, 30A] expected",
        ),
        (
            s,
            DATA,
            "0300;COMBINING GRAVE ACCENT;Mn;230;",
            "0300;COMBINING GRAVE ACCENT;Mn;X;",
            Kind::Malformed,
            "a combining class is a number 0 to 254",
        ),
        (
            s,
            DATA,
            "D7A3;<Hangul Syllable, Last>;Lo;0;L;;",
            "D7A3;<Hangul Syllable, Last>;Lo;0;L;1100 1161;",
            Kind::Invariant,
            "U+AC00..U+D7A3 is a Hangul syllable with a listed decomposition",
        ),
        // The sequence files: each version line, a type outside the file's
        // set, a field missing, a sequence listed twice, each count.
        (
            q,
            SEQUENCES,
            "# Version: 17.0",
            "# Version: 16.0",
            Kind::Header,
            "no \"# Version: 17.0\" header line",
        ),
        (
            q,
            ZWJ,
            "; RGI_Emoji_ZWJ_Sequence  ; family: man, woman, girl ",
            "; RGI_Emoji_ZWJ_Sequenc  ; family: man, woman, girl ",
            Kind::UnknownValue,
            "data/emoji17-zwj-sequences.txt type_field `RGI_Emoji_ZWJ_Sequenc`",
        ),
        (
            q,
            VARIATION,
            "0023 FE0E  ; text style;",
            "0023 FE0E  ; text style",
            Kind::FieldCount,
            "data/unicode-emoji-variation-sequences.txt:12:",
        ),
        (
            q,
            SEQUENCES,
            "1F1E6 1F1E8   ; RGI_Emoji_Flag_Sequence      ; flag: Ascension Island",
            "0023 FE0F 20E3; RGI_Emoji_Flag_Sequence      ; flag: Ascension Island",
            Kind::Duplicate,
            "listed already on line 550",
        ),
        (
            q,
            SEQUENCES,
            "1F1E6 1F1E8   ; RGI_Emoji_Flag_Sequence      ; flag: Ascension Island",
            "1F1E6         ; Basic_Emoji                  ; flag: Ascension Island",
            Kind::Count,
            "1145 sequences of two codepoints or more, 1146 expected",
        ),
        (
            q,
            VARIATION,
            "0023 FE0E  ; text style;",
            "0023 FE0D  ; text style; #\n0023 FE0E  ; text style;",
            Kind::Count,
            "3296 distinct sequences in the three files, 3295 expected",
        ),
        (
            q,
            SEQUENCES,
            "231A..231B    ; Basic_Emoji",
            "231B..231A    ; Basic_Emoji",
            Kind::Malformed,
            "U+231B..U+231A runs backwards",
        ),
        // ArabicShaping.txt and DerivedJoiningType.txt: the version, a type
        // or group outside its set, a row given twice, a row dropped, each
        // control, and the derived file disagreeing.
        (
            j,
            SHAPING,
            "0628; BEH; D; BEH",
            "0628; BEH; X; BEH",
            Kind::UnknownValue,
            "jt `X` is not one of the 6 values",
        ),
        (
            j,
            SHAPING,
            "0628; BEH; D; BEH",
            "0628; BEH; D; BAH",
            Kind::UnknownValue,
            "data/unicode-arabicshaping.txt:139:15: jg `BAH`",
        ),
        (
            j,
            SHAPING,
            "0628; BEH; D; BEH",
            "0627; BEH; D; BEH",
            Kind::Duplicate,
            "U+0627 is already set by line 138",
        ),
        (
            j,
            SHAPING,
            "0628; BEH; D; BEH",
            "#",
            Kind::Count,
            "833 rows, 834 expected",
        ),
        (
            j,
            SHAPING,
            "0627; ALEF; R; ALEF",
            "0627; ALEF; D; ALEF",
            Kind::Control,
            "U+0627 is jt D, R expected",
        ),
        (
            j,
            SHAPING,
            "0644; LAM; D; LAM",
            "0644; LAM; D; BEH",
            Kind::Control,
            "U+0644 is in group 0, 1 expected",
        ),
        (
            j,
            SHAPING,
            "0710; ALAPH; R; ALAPH",
            "0710; ALAPH; R; BETH",
            Kind::Control,
            "group 3 is [], [710] expected",
        ),
        (
            j,
            SHAPING,
            "0715; DALATH; R; DALATH RISH",
            "0715; DALATH; R; DAL",
            Kind::Control,
            "group 4 is [716, 72A, 72F]",
        ),
        (
            j,
            DJT,
            "0640          ; C",
            "0640          ; D",
            Kind::Control,
            "U+0640 is jt D in this file and C from ArabicShaping.txt",
        ),
        (
            j,
            DJT,
            "# @missing: 0000..10FFFF; Non_Joining",
            "# @missing: 0000..10FFFF; Transparent",
            Kind::Control,
            "the default is not 0000..10FFFF Non_Joining",
        ),
        (
            j,
            DJT,
            "# @missing: 0000..10FFFF; Non_Joining",
            "#",
            Kind::Count,
            "0 @missing lines, 1 expected",
        ),
        // DerivedCoreProperties.txt for the ignorables: a row dropped, two
        // rows over one code point, a field too many, each control.
        (
            i,
            DCP,
            "3164          ; Default_Ignorable_Code_Point",
            "#",
            Kind::Count,
            "16 Default_Ignorable_Code_Point ranges, 17 expected",
        ),
        (
            i,
            DCP,
            "061C          ; Default_Ignorable_Code_Point",
            "034F          ; Default_Ignorable_Code_Point",
            Kind::Overlap,
            "U+034F is already given Default_Ignorable_Code_Point",
        ),
        (
            i,
            DCP,
            "061C          ; Default_Ignorable_Code_Point",
            "061C          ; Default_Ignorable_Code_Point ; Y",
            Kind::FieldCount,
            "3 fields, 2 expected",
        ),
        (
            i,
            DCP,
            "200B..200F    ; Default_Ignorable_Code_Point",
            "200B..200C    ; Default_Ignorable_Code_Point",
            Kind::Control,
            "U+200D is default-ignorable: false, true expected",
        ),
        (
            i,
            DCP,
            "3164          ; Default_Ignorable_Code_Point",
            "3164          ; Default_Ignorable_Code_Point\n1F600 ; Default_Ignorable_Code_Point",
            Kind::Control,
            "U+1F600 is default-ignorable: true, false expected",
        ),
    ]);
}

/// The sequence table's hash is the one found from seed 0 up: from 1 up,
/// as `perfect_hash` tries for the colour names, every edge is still
/// placed, and the seeds are not the table's.
#[test]
fn the_sequence_seeds_are_tried_from_zero() {
    let emit = sequence::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    let rs = emit.rust();
    let seeds: Vec<u16> = rs
        .split_once("pub static SEEDS: [u16; 2048] = [")
        .and_then(|(_, r)| r.split_once("];"))
        .expect("SEEDS")
        .0
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().expect("a seed"))
        .collect();
    assert!(seeds.contains(&0), "a bucket placed at seed 0");
    // The keys as the build makes them, read back from EDGES.
    let edges: Vec<u64> = rs
        .split_once("pub static EDGES: [u64; 8192] = [")
        .and_then(|(_, r)| r.split_once("];"))
        .expect("EDGES")
        .0
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).expect("an edge"))
        .filter(|&e| e != 0)
        .collect();
    assert_eq!(edges.len(), 4688);
    let keys: Vec<u64> = edges.iter().map(|e| e >> 16).collect();
    let buckets: Vec<usize> = keys.iter().map(|&k| sequence::bucket(k)).collect();
    let from = |first: u16| {
        let (b, s) = (baud_tablegen::Buckets(2048), baud_tablegen::Slots(8192));
        baud_tablegen::place("edges", &buckets, b, s, first..=u16::MAX, |k, s| {
            sequence::slot(keys[k], s)
        })
        .expect("placed")
        .seeds
    };
    let zero = from(0);
    let one = from(1);
    for (b, (&z, &s)) in zero.iter().zip(&seeds).enumerate() {
        if buckets.contains(&b) {
            assert_eq!(z, s, "bucket {b}");
        }
    }
    assert_ne!(one, zero, "seeds from 1 are another table");
}

const RGB_2008: &str = "xorg-rgb-749182c.txt";
const RGB_2023: &str = "xorg-rgb-0d2caec.txt";
const DIACRITICS: &str = "kitty-placeholder-diacritics.txt";

#[test]
fn the_pinned_files_build_the_name_tables() {
    let names: Vec<Vec<String>> = [
        colour_names::kitty as Build,
        colour_names::x,
        placeholder::build,
    ]
    .iter()
    .map(|b| {
        b(&pinned)
            .unwrap_or_else(|e| panic!("{e}"))
            .files()
            .into_iter()
            .map(|(f, _)| f)
            .collect()
    })
    .collect();
    assert_eq!(
        names,
        [
            vec!["kitty_colour_names.rs"],
            vec!["x_colour_names.rs"],
            vec!["placeholder_data.rs"],
        ]
    );
    let inputs: Vec<Vec<&str>> = [&colour_names::INPUTS[..], &placeholder::INPUTS]
        .iter()
        .map(|i| i.iter().map(|i| i.file).collect())
        .collect();
    assert_eq!(inputs, [vec![RGB_2008, RGB_2023], vec![DIACRITICS]]);
}

#[test]
fn each_check_the_name_builds_make_fires() {
    let (k, x, p): (Build, Build, Build) =
        (colour_names::kitty, colour_names::x, placeholder::build);
    let long = format!("248 248 255\t\tGhostWhite\n1 2 3\t\t{}", "a".repeat(256));
    let wide: String = (0..260u32)
        .map(|n| format!("1 2 3\t\t{n:03}{}\n", "a".repeat(252)))
        .collect();
    let wide = format!("248 248 255\t\tGhostWhite\n{wide}");
    let many: String = (0..260u32)
        .map(|n| format!("{:05X}\n", 0x2_0000 + n * 64))
        .collect();
    let many = format!("0305\n{many}");
    each_fires(&[
        // rgb.txt: another first line, a channel, ASCII, a missing name.
        (
            k,
            RGB_2008,
            "255 250 250\t\tsnow",
            "255 250 251\t\tsnow",
            Kind::Header,
            "is this the pinned file [xorg-rgb-749182c]?",
        ),
        (
            x,
            RGB_2023,
            "255 250 250\t\tsnow",
            "255 250 251\t\tsnow",
            Kind::Header,
            "is this the pinned file [xorg-rgb-0d2caec]?",
        ),
        (
            k,
            RGB_2008,
            "248 248 255\t\tghost white",
            "248 248 256\t\tghost white",
            Kind::Malformed,
            "data/xorg-rgb-749182c.txt:2:9: \"256\" is not a channel, 0 to 255",
        ),
        (
            x,
            RGB_2023,
            "248 248 255\t\tghost white",
            "248 248 255\t\tghost whit\u{e9}",
            Kind::Malformed,
            "data/xorg-rgb-0d2caec.txt:2:24: not ASCII",
        ),
        (
            k,
            RGB_2008,
            "248 248 255\t\tghost white",
            "248 248 255\t\t",
            Kind::FieldCount,
            "data/xorg-rgb-749182c.txt:2:14: a colour is three channels and a name",
        ),
        (
            x,
            RGB_2023,
            "248 248 255\t\tghost white",
            "248 248",
            Kind::FieldCount,
            "data/xorg-rgb-0d2caec.txt:2:8: a colour is three channels and a name",
        ),
        // The duplicate rule: a spelling given twice, and two spellings
        // that differ only in case and carry two colours.
        (
            k,
            RGB_2008,
            "248 248 255\t\tGhostWhite",
            "248 248 255\t\tghost white",
            Kind::Duplicate,
            "data/xorg-rgb-749182c.txt:3:14: \"ghost white\" is given already on line 2",
        ),
        (
            x,
            RGB_2023,
            "248 248 255\t\tGhostWhite",
            "248 248 254\t\tGhost White",
            Kind::Duplicate,
            "data/xorg-rgb-0d2caec.txt:3:14: \"Ghost White\" is \"ghost white\" on line 2 \
             in another case, with another colour",
        ),
        (
            k,
            RGB_2008,
            "248 248 255\t\tGhostWhite",
            "248 248 255\t\tGhostWhite\n215 7 81\t\tDebianRed",
            Kind::Duplicate,
            "debianred is in rgb.txt already, and kitty's table adds it",
        ),
        // A name past a slot's u8 length.
        (
            x,
            RGB_2023,
            "248 248 255\t\tGhostWhite",
            &long,
            Kind::Invariant,
            "is longer than a slot's u8 length",
        ),
        // A pool past a slot's u16 offset: 260 more names of 255 bytes.
        (
            k,
            RGB_2008,
            "248 248 255\t\tGhostWhite\n",
            &wide,
            Kind::Invariant,
            "past a slot's u16 offset",
        ),
        // The counts: a line dropped from each.
        (
            k,
            RGB_2008,
            "248 248 255\t\tGhostWhite\n",
            "",
            Kind::Count,
            "data/xorg-rgb-749182c.txt: 752 colour names and debianred, 753 expected",
        ),
        (
            x,
            RGB_2023,
            "248 248 255\t\tGhostWhite\n",
            "",
            Kind::Count,
            "data/xorg-rgb-0d2caec.txt: 781 colour names, 782 expected",
        ),
        // The diacritics: the parent's hash, a row, the count, the first
        // three, a duplicate, and more blocks than a u8 names.
        (
            p,
            DIACRITICS,
            "sha256 a80368b3",
            "sha256 a80368b4",
            Kind::Header,
            "is this the pinned file [kitty-placeholder-diacritics]?",
        ),
        (
            p,
            DIACRITICS,
            "0310\n",
            "031G\n",
            Kind::Malformed,
            "data/kitty-placeholder-diacritics.txt:9:1: \"031G\" is not a code point",
        ),
        (
            p,
            DIACRITICS,
            "0310\n",
            "0310; 1\n",
            Kind::FieldCount,
            "data/kitty-placeholder-diacritics.txt:9:7: 2 fields, 1 expected",
        ),
        (
            p,
            DIACRITICS,
            "0305\n",
            "",
            Kind::Count,
            "296 diacritics, 297 expected",
        ),
        (
            p,
            DIACRITICS,
            "0305\n030D\n",
            "030D\n0305\n",
            Kind::Control,
            "diacritic 0 is Some(30D), U+0305 expected",
        ),
        (
            p,
            DIACRITICS,
            "030E\n",
            "030D\n",
            Kind::Duplicate,
            "data/kitty-placeholder-diacritics.txt:8:1: U+030D is listed already on line 7",
        ),
        (
            p,
            DIACRITICS,
            "0305\n",
            &many,
            Kind::Invariant,
            "blocks, more than a u8 names",
        ),
    ]);
}

/// **A case-only second spelling with the line's colour is one name**: no
/// failure but the count, which the name it no longer adds misses.
#[test]
fn a_spelling_in_another_case_with_one_colour_is_one_name() {
    for (build, file, want) in [
        (
            colour_names::kitty as Build,
            RGB_2008,
            "752 colour names and debianred, 753 expected",
        ),
        (colour_names::x, RGB_2023, "781 colour names, 782 expected"),
    ] {
        let got = failures_of(
            build,
            file,
            "248 248 255\t\tGhostWhite",
            "248 248 255\t\tGhost White",
        );
        assert_eq!(got.len(), 1, "{got:#?}");
        assert_eq!(got[0].0, Kind::Count);
        assert!(got[0].1.contains(want), "{got:#?}");
    }
}

/// **The two rgb.txt pins open with one line**, so the header cannot tell
/// them apart: the counts do. Each build given the other's file fails.
#[test]
fn the_two_rgb_pins_swapped_fail_their_counts() {
    let swapped = |f: &str| match f {
        RGB_2008 => pinned(RGB_2023),
        RGB_2023 => pinned(RGB_2008),
        _ => pinned(f),
    };
    for (build, want) in [
        (
            colour_names::kitty as Build,
            "783 colour names and debianred, 753 expected",
        ),
        (colour_names::x, "752 colour names, 782 expected"),
    ] {
        let errs = build(&swapped).expect_err("the other pin");
        assert_eq!(errs.kinds(), [Kind::Count], "{errs}");
        assert!(errs.to_string().contains(want), "{errs}");
    }
}

/// The files of `e`, by name.
fn file_names(e: &Emit) -> Vec<String> {
    e.files().into_iter().map(|(f, _)| f).collect()
}

#[test]
fn the_rules_build_the_cluster_tables() {
    let e = cluster::build().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        file_names(&e),
        [
            "cluster_data.rs",
            "cluster_next.bin",
            "cluster_parts_legacy.bin",
            "cluster_parts_marks_joined.bin"
        ]
    );
    let rs = e.rust();
    assert!(
        rs.starts_with(
            "// Generated by baud-domain/build.rs (build/cluster.rs) from UAX #29's rules [uax29].\n"
        ),
        "{rs}"
    );
    assert!(rs.contains("pub const CLUSTER_READY: u8 = 1;"), "{rs}");
    assert!(rs.contains("pub const CLUSTER_STATES: usize = 94;"), "{rs}");
    assert_eq!(bin(&e, "cluster_next").len(), 94 * 256);
    assert_eq!(bin(&e, "cluster_parts_legacy").len(), 94 * 32);
}

/// **Each rule is seen**: the table built with one of UAX #29's rules
/// dropped is refused, by the control written for that rule. GB999 is
/// not among them: without it nothing matches, and that breaks, as GB999
/// does, so the table is the same by construction. None moves
/// the reachable state count, which the rules' decisions reach only
/// through a regional indicator's parity.
#[test]
fn a_rule_dropped_from_the_cluster_rules_is_refused() {
    use cluster::Rule;
    for (drop, says) in [
        (Rule::Gb1, "GB1, an Extend at the start"),
        (Rule::Gb3, "GB3, CR LF"),
        (Rule::Gb4, "GB4, Control Extend"),
        (Rule::Gb5, "GB5, Prepend LF"),
        (Rule::Gb6, "GB6 and GB7, L V T"),
        (Rule::Gb7, "GB6 and GB7, L V T"),
        (Rule::Gb8, "GB8, LVT T"),
        (Rule::Gb9, "GB9, Other Extend"),
        (Rule::Gb9a, "GB9a, Other SpacingMark"),
        (Rule::Gb9b, "GB9b, Prepend Other"),
        (Rule::Gb9c, "GB9c, a consonant, a virama, a consonant"),
        (Rule::Gb11, "GB11, a pictograph, ZWJ, a pictograph"),
        (Rule::Gb12And13, "GB12 and GB13, RI RI RI"),
    ] {
        let rules: Vec<Rule> = cluster::EXTENDED
            .iter()
            .copied()
            .filter(|&r| r != drop)
            .collect();
        let errs = cluster::build_with(&rules).expect_err("a rule dropped");
        assert!(
            errs.list()
                .iter()
                .any(|e| e.kind == Kind::Control && e.to_string().contains(says)),
            "{drop:?}: {errs}"
        );
    }
}

#[test]
fn the_cluster_checks_refuse_what_does_not_fit() {
    let states = |n: usize| {
        let mut errs = Errors::default();
        let ids = cluster::check_states(n, &mut errs);
        (ids, errs)
    };
    let (ids, errs) = states(94);
    assert!(ids == Some(94) && errs.is_empty(), "{errs}");
    let (ids, errs) = states(128);
    assert_eq!(ids, Some(128));
    assert_eq!(errs.kinds(), [Kind::Count]);
    assert!(
        errs.to_string()
            .contains("128 reachable states, 94 expected"),
        "{errs}"
    );
    let (ids, errs) = states(129);
    assert_eq!(ids, None);
    assert_eq!(errs.kinds(), [Kind::Count, Kind::Invariant]);
    assert!(
        errs.to_string()
            .contains("129 states will not fit a seven-bit state id"),
        "{errs}"
    );
    assert_eq!(
        cluster::the_ready_state(&[false, true, false]).ok(),
        Some(1)
    );
    for ready in [&[false, false][..], &[true, false, true]] {
        let e = cluster::the_ready_state(ready).expect_err("not one ready state");
        assert_eq!(e.kind, Kind::Invariant);
        assert!(e.to_string().contains("exactly one expected"), "{e}");
    }
}

/// A grapheme build's two tries, `index` and `blocks`, as `grapheme::build`
/// names them.
fn grapheme_emit(index: &[u8], blocks: &[u8]) -> Emit {
    let mut e = Emit::new("grapheme_data", "a test", &[]);
    e.bytes("BREAK_INDEX", &[], index);
    e.bytes("BREAK_BLOCKS", &[], blocks);
    e
}

/// A width build whose both tries are `index` and the blocks of `values`,
/// 256 two-bit values each.
fn width_emit(index: &[u8], values: &[[u8; 256]]) -> Emit {
    let packed: Vec<u8> = values
        .iter()
        .flat_map(|b| {
            b.chunks(4)
                .map(|q| {
                    q.iter()
                        .enumerate()
                        .fold(0, |p, (j, &v)| p | (v << (2 * j)))
                })
                .collect::<Vec<u8>>()
        })
        .collect();
    let mut e = Emit::new("width_data", "a test", &[]);
    for t in ["WIDTH", "WIDTH8"] {
        e.bytes(&format!("{t}_INDEX"), &[], index);
        e.bytes(&format!("{t}_BLOCKS"), &[], &packed);
    }
    e
}

#[test]
fn the_pinned_files_build_the_merged_table() {
    let g = grapheme::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    let w = width::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    let m = merged::build(&g, &w).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        file_names(&m),
        [
            "merged_data.rs",
            "merged_palette.bin",
            "merged_index.bin",
            "merged_blocks.bin",
            "bmp_direct.bin"
        ]
    );
    assert!(
        m.rust().contains("per palette id: 33 of them."),
        "{}",
        m.rust()
    );
    assert_eq!(bin(&m, "merged_palette").len(), 99);
    assert_eq!(bin(&m, "merged_blocks").len(), 256 * 256);
    assert_eq!(bin(&m, "bmp_direct").len(), 0x10000 * 2);
}

#[test]
fn each_check_the_merged_build_makes_fires() {
    let g = grapheme::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    let w = width::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    let fails = |g: &Emit, w: &Emit, kind: Kind, says: &str| {
        let errs = merged::build(g, w).expect_err(says);
        assert!(
            errs.list()
                .iter()
                .any(|e| e.kind == kind && e.to_string().contains(says)),
            "no {kind:?} saying {says:?} in {errs}"
        );
    };
    // The two builds swapped: neither has the other's tries.
    fails(&w, &g, Kind::Invariant, "no break_index.bin to read");
    fails(&g, &g, Kind::Invariant, "no width_index.bin to read");

    let one_block = vec![0u8; 0x110000 >> 8];
    let narrow = width_emit(&one_block, &[[0; 256]]);
    // A class nibble past the fourteen.
    fails(
        &grapheme_emit(&one_block, &[0x0E; 256]),
        &narrow,
        Kind::Invariant,
        "U+0080's class is past the 14 GraphemeClass has",
    );

    // 224 classes and lookback sets, each narrow and zero: 448 entries
    // and more, past a byte.
    let mut many = [0u8; 256];
    for (i, b) in many.iter_mut().enumerate().take(224) {
        *b = u8::try_from((i % 14) | ((i / 14) << 4)).expect("a byte");
    }
    let halves: Vec<u8> = (0..0x110000usize >> 8)
        .map(|hi| u8::try_from(hi & 1).expect("0 or 1"))
        .collect();
    let g2 = grapheme_emit(&one_block, &many);
    let w2 = width_emit(&halves, &[[0; 256], [1; 256]]);
    fails(
        &g2,
        &w2,
        Kind::Invariant,
        "palette entries will not fit a byte",
    );
    fails(&g2, &w2, Kind::Count, "palette entries, 33 expected");

    // Every block of 256 code points distinct, from 256 class blocks and
    // 17 width blocks: 4,352 blocks, past a byte.
    let class_blocks: Vec<u8> = (0..256usize)
        .flat_map(|b| {
            (0..256usize)
                .map(move |j| u8::from((0x80..0x88).contains(&j) && (b >> (j - 0x80)) & 1 == 1))
        })
        .collect();
    let class_index: Vec<u8> = (0..0x110000usize >> 8)
        .map(|hi| u8::try_from(hi & 0xFF).expect("a byte"))
        .collect();
    let width_blocks: Vec<[u8; 256]> = (0..17usize)
        .map(|k| {
            let mut b = [0u8; 256];
            for (j, v) in b.iter_mut().enumerate().skip(0xC8).take(5) {
                *v = u8::from((k >> (j - 0xC8)) & 1 == 1);
            }
            b
        })
        .collect();
    let width_index: Vec<u8> = (0..0x110000usize >> 8)
        .map(|hi| u8::try_from(hi >> 8).expect("a byte"))
        .collect();
    fails(
        &grapheme_emit(&class_index, &class_blocks),
        &width_emit(&width_index, &width_blocks),
        Kind::Invariant,
        "4345 distinct blocks will not fit a byte",
    );

    // Unicode 8's widths taken for the current ones: a regional indicator
    // is narrow in both, which its control refuses.
    let mut w8 = Emit::new("width_data", "a test", &[]);
    for t in ["WIDTH", "WIDTH8"] {
        w8.bytes(&format!("{t}_INDEX"), &[], &bin(&w, "width8_index"));
        w8.bytes(&format!("{t}_BLOCKS"), &[], &bin(&w, "width8_blocks"));
    }
    fails(&g, &w8, Kind::Control, "U+1F1E6 (a regional indicator)");
}

const CHARSETS: &str = "xterm-charsets.h";
const KEYSYMS: &str = "xorg-keysymdef.h";
const JIS: &str = "JIS0201.TXT";

#[test]
fn the_pinned_files_build_the_dec_tables() {
    for (name, build) in [
        ("dec graphics", dec_graphics::build as Build),
        ("dec sets", dec_sets::build),
        ("nrcs", nrcs::build),
        ("iso96", iso96::build),
    ] {
        if let Err(errs) = build(&pinned) {
            panic!("{name}: {errs}");
        }
    }
    // What each reads, and every one of them under `data/`, where the
    // published crate carries it.
    let inputs: Vec<Vec<&str>> = [&dec_sets::INPUTS[..], &nrcs::INPUTS, &iso96::INPUTS]
        .iter()
        .map(|i| i.iter().map(|i| i.file).collect())
        .collect();
    assert_eq!(
        inputs,
        [
            vec![CHARSETS, JIS],
            vec![CHARSETS, KEYSYMS],
            vec![
                "iso8859-1.txt",
                "iso8859-2.txt",
                "iso8859-7.txt",
                "iso8859-8.txt",
                "iso8859-5.txt",
                "iso8859-9.txt"
            ],
        ]
    );
    let data = concat!(env!("CARGO_MANIFEST_DIR"), "/data");
    for file in inputs.concat() {
        assert!(
            std::path::Path::new(&format!("{data}/{file}")).is_file(),
            "data/{file}"
        );
    }
}

#[test]
fn each_check_the_dec_builds_make_fires() {
    let (g, d, n, i): (Build, Build, Build, Build) = (
        dec_graphics::build,
        dec_sets::build,
        nrcs::build,
        iso96::build,
    );
    let id = "charsets.h,v 1.36";
    each_fires(&[
        // The headers: charsets.h's line 1 and its revision on line 2,
        // keysymdef.h's, JIS0201.TXT's and an ISO 8859 file's.
        (
            g,
            CHARSETS,
            id,
            "charsets.h,v 1.37",
            Kind::Header,
            "line 2 is not",
        ),
        (
            d,
            CHARSETS,
            id,
            "charsets.h,v 1.37",
            Kind::Header,
            "line 2 is not",
        ),
        (
            n,
            CHARSETS,
            id,
            "charsets.h,v 1.37",
            Kind::Header,
            "line 2 is not",
        ),
        (
            d,
            CHARSETS,
            "/*",
            "//",
            Kind::Header,
            "is this the pinned file [xterm-charsets]?",
        ),
        (
            n,
            KEYSYMS,
            "/****",
            "/*  *",
            Kind::Header,
            "is this the pinned file [xorg-keysymdef]?",
        ),
        (
            d,
            JIS,
            "# JIS0201.TXT",
            "# JIS0208.TXT",
            Kind::Header,
            "is this the pinned file [unicode-jis0201]?",
        ),
        (
            i,
            "iso8859-1.txt",
            "# 8859-1.TXT",
            "# 8859-2.TXT",
            Kind::Header,
            "is this the pinned file [iso8859-1]?",
        ),
        // The code-page reader: upper-case digits only in the sets'
        // macros, a line that is no entry, a byte twice, no body.
        (
            d,
            CHARSETS,
            "UNI(0x28, 0x00A4)",
            "UNI(0x28, 0x00a4)",
            Kind::Malformed,
            "data/xterm-charsets.h:443:6: map_DEC_Supp_Graphic: an entry the build cannot read",
        ),
        (
            d,
            CHARSETS,
            "XXX(0x24, UNDEF) \\",
            "XXX(0x24, UNDEF) \\\n\t    junk \\",
            Kind::Malformed,
            "map_DEC_Supp_Graphic: an entry the build cannot read",
        ),
        (
            d,
            CHARSETS,
            "UNI(0x57, 0x0152)",
            "UNI(0x28, 0x0152)",
            Kind::Duplicate,
            "map_DEC_Supp_Graphic: 0x28 twice",
        ),
        (
            d,
            CHARSETS,
            "#define map_JIS_Roman(code) \\",
            "#define map_JIS_Rome(code) \\",
            Kind::Malformed,
            "no map_JIS_Roman with a body",
        ),
        (
            g,
            CHARSETS,
            "UNI(0x61, 0x2592)",
            "UNI(0x61,0x2592)",
            Kind::Malformed,
            "map_DEC_Spec_Graphic: an entry the build cannot read",
        ),
        // DEC Supplemental: Figure A-2's mask, the 81, CONFORM's cell
        // agreeing with xterm.
        (
            d,
            CHARSETS,
            "XXX(0x24, UNDEF) \\",
            "UNI(0x24, 0x0024) \\",
            Kind::Control,
            "are not Figure A-2's reserved ones",
        ),
        (
            d,
            CHARSETS,
            "XXX(0x24, UNDEF) \\",
            "UNI(0x24, 0x0024) \\",
            Kind::Count,
            "82 DEC Supplemental positions defined, 81 expected",
        ),
        (
            d,
            CHARSETS,
            "UNI(0x5F, 0x005F)",
            "UNI(0x5F, 0x00DF)",
            Kind::Control,
            "Supplemental 0x5F: CONFORM gives U+00DF, DEC's text U+00DF, xterm U+00DF",
        ),
        // JIS0201.TXT: a line it cannot read, a katakana it does not map,
        // the 63, the Yen sign.
        (
            d,
            JIS,
            "0xA1\t0xFF61",
            "0xA1 0xFF61",
            Kind::Malformed,
            "data/JIS0201.TXT:142:1: a line the build cannot read",
        ),
        (
            d,
            JIS,
            "0xA1\t0xFF61\t# HALFWIDTH IDEOGRAPHIC FULL STOP\n",
            "",
            Kind::Control,
            "JIS-Katakana 0x21: JIS0201.TXT maps no 0xA1",
        ),
        (
            d,
            JIS,
            "0xA1\t0xFF61\t# HALFWIDTH IDEOGRAPHIC FULL STOP\n",
            "",
            Kind::Count,
            "62 JIS-Katakana positions defined, 63 expected",
        ),
        (
            d,
            JIS,
            "0x5C\t0x00A5",
            "0x5C\t0x005C",
            Kind::Control,
            "does not map 0x5C to U+00A5",
        ),
        // xterm drifting from the JIS tables and DEC Special Graphics, and
        // a departure recorded that is no longer xterm's.
        (
            d,
            CHARSETS,
            "UNI(0x31, 0xFF71)",
            "UNI(0x31, 0xFF72)",
            Kind::Control,
            "JisKatakana 0x31: xterm U+FF72, DEC's text U+FF71, and neither",
        ),
        (
            d,
            CHARSETS,
            "UNI(0x7E, 0x203E)",
            "UNI(0x7E, 0x00AF)",
            Kind::Control,
            "JisRoman 0x7E: XTERM_DEPARTS records U+203E, xterm U+00AF",
        ),
        (
            g,
            CHARSETS,
            "UNI(0x60, 0x25c6)",
            "XXX(0x60, UNDEF)",
            Kind::Control,
            "map_DEC_Spec_Graphic gives 0x60 no codepoint",
        ),
        (
            g,
            CHARSETS,
            "UNI(0x60, 0x25c6)",
            "UNI(0x41, 0x0391) \\\n\t    UNI(0x60, 0x25c6)",
            Kind::Control,
            "Graphics 0x41: xterm U+0391, DEC's text U+0041, and neither",
        ),
        (
            g,
            CHARSETS,
            "XXX(0x5F, UNDEF) \\\n\t    UNI(0x60, 0x25c6)",
            "UNI(0x5F, 0x0020) \\\n\t    UNI(0x60, 0x25c6)",
            Kind::Control,
            "Graphics 0x5F: XTERM_DEPARTS records U+2426, xterm U+0020",
        ),
        // The national macros: a line that is no entry, an entry it cannot
        // read, a keysym with no `#define`, past Latin-1, a comment that
        // is not its keysym, a byte twice.
        (
            n,
            CHARSETS,
            "MAP(0x23, XK_sterling)",
            "junk \\\n\t    MAP(0x23, XK_sterling)",
            Kind::Malformed,
            "data/xterm-charsets.h:260:6: neither an entry nor the switch around them",
        ),
        (
            n,
            CHARSETS,
            "MAP(0x40, XK_Eacute)",
            "MAP(0x40 XK_Eacute)",
            Kind::Malformed,
            "an entry the build cannot read",
        ),
        (
            n,
            CHARSETS,
            "MAP(0x40, XK_Eacute)",
            "MAP(0x40, XK_Eacutx)",
            Kind::UnknownValue,
            "XK_Eacutx has no #define in xorg-keysymdef.h",
        ),
        (
            n,
            KEYSYMS,
            "#define XK_Eacute                        0x00c9",
            "#define XK_Eacute                        0x01c9",
            Kind::Invariant,
            "XK_Eacute is 0x1c9, outside Latin-1",
        ),
        (
            n,
            KEYSYMS,
            "#define XK_Eacute                        0x00c9",
            "#define XK_Eacute                        0x00c8",
            Kind::Control,
            "XK_Eacute does not resolve to 0xC9",
        ),
        (
            n,
            CHARSETS,
            "MAP(0x23, XK_sterling)\t/* U+00A3",
            "MAP(0x23, XK_sterling)\t/* U+00A4",
            Kind::Control,
            "XK_sterling is U+00A3, its comment says U+00A4",
        ),
        (
            n,
            CHARSETS,
            "MAP(0x40, XK_threequarters)",
            "MAP(0x23, XK_threequarters)",
            Kind::Duplicate,
            "Dutch: two entries for 0x23",
        ),
        // A skip, an override and a body the source no longer bears out,
        // an `UNDEF` line neither emitted nor skipped, two bodies.
        (
            n,
            CHARSETS,
            "XXX(0x79, UNDEF)\t/* unused */",
            "UNI(0x79, 0x0391)\t/* unused */",
            Kind::Invariant,
            "skip Greek 0x79: no `XXX(..., UNDEF)` line there any more",
        ),
        (
            n,
            CHARSETS,
            "UNI(0x26, 0x011F)",
            "UNI(0x21, 0x0131) \\\n\t    UNI(0x26, 0x011F)",
            Kind::Invariant,
            "Turkish 0x21: xterm's macro has it now; retire the DEC override",
        ),
        (
            n,
            CHARSETS,
            "#define map_NRCS_Turkish(code) \\",
            "#define map_NRCS_Turkey(code) \\",
            Kind::Invariant,
            "DEC override Turkish 0x21: not applied",
        ),
        (
            n,
            CHARSETS,
            "#define map_NRCS_Swiss(code) \\",
            "#define map_NRCS_Swizz(code) \\",
            Kind::Malformed,
            "map_NRCS_Swiss: no definition with a body",
        ),
        (
            n,
            CHARSETS,
            "MAP(0x23, XK_sterling)\t/* U+00A3 POUND SIGN */",
            "XXX(0x23, UNDEF)",
            Kind::Count,
            "Dutch: 8 emitted for 9 lines in map_NRCS_Dutch, 0 skipped and 0 added; not emitted: charsets.h:260 XXX(0x23, UNDEF)",
        ),
        (
            n,
            CHARSETS,
            "#define map_NRCS_Greek(code)\t/* nothing */",
            "#define map_NRCS_Greek(code) \\\n\tswitch (code) { \\\n\t}",
            Kind::Duplicate,
            "map_NRCS_Greek: two definitions with a body",
        ),
        // The ISO 8859 files: a line it cannot read, a byte twice, a
        // later edition's position the file does not map, the counts.
        (
            i,
            "iso8859-1.txt",
            "0xA1\t0x00A1",
            "0xA1 0x00A1",
            Kind::Malformed,
            "data/iso8859-1.txt:198:1: a line the build cannot read",
        ),
        (
            i,
            "iso8859-1.txt",
            "0xA1\t0x00A1",
            "0xA0\t0x00A1",
            Kind::Duplicate,
            "0xA0 twice",
        ),
        (
            i,
            "iso8859-7.txt",
            "0xA4\t0x20AC\t#\tEURO SIGN\n",
            "",
            Kind::Control,
            "0xA4 is listed as added later but the file does not map it",
        ),
        (
            i,
            "iso8859-1.txt",
            "0xA0\t0x00A0\t#\tNO-BREAK SPACE\n",
            "",
            Kind::Count,
            "95 positions assigned, 96 expected",
        ),
    ]);
}

/// DEC's Table 2-3, its right half, as `vt220_tables.tsv` transcribes it:
/// the codepoint each GR cell draws, or the error character for a cell
/// that draws nothing. Read here, with no code of the tests' own readers.
fn table_2_3(tsv: &str) -> [u32; 94] {
    let mut out = [0u32; 94];
    let mut seen = 0;
    for line in tsv.lines().filter(|l| l.starts_with("cell\tT2-3b\t")) {
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 5, "{line:?}");
        let code = usize::from_str_radix(f[2], 16).expect("hex");
        let mut cs = f[3].chars();
        out[code - 0x21] = match (cs.next(), cs.next()) {
            (None, _) => dec_sets::ERROR,
            (Some(c), None) => u32::from(c),
            _ => panic!("{line:?}: more than one character"),
        };
        seen += 1;
    }
    assert_eq!(seen, 94, "Table 2-3's 94 GR cells");
    out
}

const VT220_TABLES: &str = include_str!("vt220_tables.tsv");

/// xterm's DEC Supplemental, every cell, as the build reads it.
fn xterm_supplemental(charsets: &str) -> [u32; 94] {
    let src = baud_tablegen::Source::new(&xterm::CHARSETS, charsets.to_string()).expect("header");
    let mut errs = Errors::default();
    let page = dec_sets::code_page(
        &src,
        "map_DEC_Supp_Graphic",
        xterm::Digits::Upper,
        &mut errs,
    );
    assert!(errs.is_empty(), "{errs}");
    dec_sets::cells(&page)
}

/// **The cells where xterm and DEC's text differ are exactly the cells
/// named.** The build reads xterm's `map_DEC_Supp_Graphic` and never DEC's
/// pages, so this holds what it takes against `[[source:
/// dec-special-graphics]]` Table 2-3: a cell where they differ that
/// `CONFORM` does not name is red, a named cell where they agree is red,
/// and a transcription changed under the table is red.
#[test]
fn the_cells_where_xterm_and_dec_differ_are_named() {
    use dec_sets::{CONFORM, Conform, Departs, Set, XTERM_DEPARTS, named_exactly};
    let charsets = pinned(CHARSETS).expect("charsets.h");
    let xterm = xterm_supplemental(&charsets);
    let dec = table_2_3(VT220_TABLES);
    let s = Set::Supplemental;
    assert_eq!(
        named_exactly(s, &xterm, &dec, &CONFORM, &XTERM_DEPARTS),
        Vec::<String>::new()
    );
    // 5/15 not named.
    let unnamed = named_exactly(s, &xterm, &dec, &[], &XTERM_DEPARTS);
    assert_eq!(
        unnamed,
        [
            "Supplemental 0x5F: xterm U+005F, DEC's text U+00DF, and neither CONFORM nor XTERM_DEPARTS names it"
        ]
    );
    // A cell named where the two agree, either way.
    let agree = [
        Conform {
            set: s,
            byte: 0x21,
            dec: 0x00A1,
            why: &[],
        },
        CONFORM[0],
    ];
    assert_eq!(
        named_exactly(s, &xterm, &dec, &agree, &XTERM_DEPARTS),
        ["Supplemental 0x21: CONFORM gives U+00A1, DEC's text U+00A1, xterm U+00A1"]
    );
    let departs = [Departs {
        set: s,
        byte: 0x21,
        xterm: 0x00A1,
        why: &[],
    }];
    assert_eq!(
        named_exactly(s, &xterm, &dec, &CONFORM, &departs),
        ["Supplemental 0x21: XTERM_DEPARTS records U+00A1, xterm U+00A1, DEC's text U+00A1"]
    );
    // DEC's text changed under the table, and xterm's.
    let moved = VT220_TABLES.replacen("cell\tT2-3b\t21\t¡", "cell\tT2-3b\t21\t!", 1);
    assert_eq!(
        named_exactly(s, &xterm, &table_2_3(&moved), &CONFORM, &XTERM_DEPARTS),
        [
            "Supplemental 0x21: xterm U+00A1, DEC's text U+0021, and neither CONFORM nor XTERM_DEPARTS names it"
        ]
    );
    let drifted =
        xterm_supplemental(&charsets.replacen("UNI(0x57, 0x0152)", "UNI(0x57, 0x0153)", 1));
    assert_eq!(
        named_exactly(s, &drifted, &dec, &CONFORM, &XTERM_DEPARTS),
        [
            "Supplemental 0x57: xterm U+0153, DEC's text U+0152, and neither CONFORM nor XTERM_DEPARTS names it"
        ]
    );
}

const XTERMCAP: &str = "xterm-xtermcap.c";
const ENTRY: &str = "terminfo-xterm-256color.ti";
const DOC_KEYS: &str = "iterm2-osc1337-doc-keys.txt";
const SOURCE_KEYS: &str = "iterm2-osc1337-source-keys.txt";

#[test]
fn the_pinned_files_build_the_key_tables() {
    for (name, build) in [
        ("tcap", tcap::build as Build),
        ("termcap keys", termcap_keys::build),
        ("iterm2 keys", iterm_keys::build),
    ] {
        if let Err(errs) = build(&pinned) {
            panic!("{name}: {errs}");
        }
    }
    let inputs: Vec<Vec<&str>> = [
        &tcap::INPUTS[..],
        &termcap_keys::INPUTS,
        &iterm_keys::INPUTS,
    ]
    .iter()
    .map(|i| i.iter().map(|i| i.file).collect())
    .collect();
    assert_eq!(
        inputs,
        [
            vec![XTERMCAP],
            vec![XTERMCAP, ENTRY],
            vec![DOC_KEYS, SOURCE_KEYS]
        ]
    );
    let data = concat!(env!("CARGO_MANIFEST_DIR"), "/data");
    for file in inputs.concat() {
        assert!(
            std::path::Path::new(&format!("{data}/{file}")).is_file(),
            "data/{file}"
        );
    }
}

/// **Each names file's line 1 names its parents as the manifest pins
/// them**, the entry's `derived_from` listing them, and every row holds an
/// identifier and nothing else. The manifest is read here, not by a build
/// module.
#[test]
fn the_names_files_name_their_parents_and_hold_only_identifiers() {
    let manifest = include_str!("../../docs/research/sources/MANIFEST.toml");
    let field = |key: &str, name: &str| -> String {
        let at = manifest
            .find(&format!("[sources.{key}]\n"))
            .unwrap_or_else(|| panic!("{key} in the manifest"));
        manifest[at..]
            .lines()
            .take_while(|l| !l.is_empty())
            .find_map(|l| {
                let (n, v) = l.split_once('=')?;
                (n.trim() == name).then(|| v.trim().to_string())
            })
            .unwrap_or_else(|| panic!("{key} has no {name}"))
    };
    for (key, file, parents, fields) in [
        (
            "iterm2-osc1337-doc-keys",
            DOC_KEYS,
            &[
                "iterm2-escape-codes",
                "iterm2-images",
                "iterm2-badges",
                "iterm2-feature-reporting",
            ][..],
            2,
        ),
        (
            "iterm2-osc1337-source-keys",
            SOURCE_KEYS,
            &["iterm2-964a75d-vt100terminal"][..],
            1,
        ),
    ] {
        let text = pinned(file).expect("the names file");
        let first = text.lines().next().expect("line 1");
        let derived = field(key, "derived_from");
        for parent in parents {
            let sha = field(parent, "sha256");
            let sha = sha.trim_matches('"');
            assert_eq!(sha.len(), 64, "{parent}");
            assert!(
                first.contains(&format!("[[source: {parent}]] at sha256 {sha}")),
                "{file}: line 1 does not name {parent} at {sha}"
            );
            assert!(
                derived.contains(&format!("\"{parent}\"")),
                "{key}: {parent}"
            );
            assert_eq!(field(parent, "status"), "\"external\"", "{parent}");
            assert_eq!(field(parent, "file"), "\"\"", "{parent} is not in the tree");
        }
        for row in text.lines().filter(|l| !l.starts_with('#')) {
            let cells: Vec<&str> = row.split("; ").collect();
            assert_eq!(cells.len(), fields, "{row}");
            assert!(cells[0].bytes().all(|b| b.is_ascii_alphanumeric()), "{row}");
            if let Some(page) = cells.get(1) {
                assert!(parents.contains(page), "{row}");
            }
        }
    }
}

/// `PyRandom` draws what CPython's `random.Random(seed).getrandbits(64)`
/// draws: the first three of each seed, as Python 3.13.12 printed them.
#[test]
fn each_seed_draws_what_python_draws() {
    for (seed, want) in [
        (
            1,
            [
                0x91b7_584a_2265_b1f5u64,
                0xcd61_3e30_d8f1_6adf,
                0x1027_c4d1_c386_bbc4,
            ],
        ),
        (
            7,
            [
                0xf2a7_4de4_52e6_b438,
                0x6513_270e_269e_0d37,
                0x0c5c_7fd0_a6a3_a450,
            ],
        ),
        (
            0,
            [
                0x629f_6fbe_d82c_07cd,
                0xe3e7_0682_c209_4cac,
                0x0a5d_2f34_6baa_9455,
            ],
        ),
    ] {
        let mut r = py_random::PyRandom::new(seed);
        assert_eq!(want.map(|_| r.bits64()), want, "seed {seed}");
    }
}

/// **The multiplier is the generator's search**: of the odd draws of
/// `PyRandom` seeded with 1, the first under which no name walks two slots
/// is the 1009th, the multiplier the Python generator found.
#[test]
fn the_tcap_multiplier_is_the_seeded_search_and_its_bound_is_held() {
    let mut r = py_random::PyRandom::new(tcap::SEED);
    let draws: Vec<u64> = (0..tcap::TRIES).map(|_| r.bits64() | 1).collect();
    let emit = tcap::build(&pinned).expect("the tcap table");
    let names: Vec<&[u8]> = emit
        .rust()
        .lines()
        .filter_map(|l| l.strip_prefix("    (b\"")?.split('"').next())
        .map(str::as_bytes)
        .collect();
    assert_eq!(names.len(), 319);
    let first = draws.iter().position(|&m| tcap::walk(&names, m) <= 1);
    assert_eq!(first, Some(1008));
    assert_eq!(tcap::multiplier(&names), Some(0xf224_2639_261b_5841));
    assert!(emit.rust().contains("const MUL: u64 = 0xf2242639261b5841;"));
    // Three names in one bucket under any multiplier: one of them walks
    // two slots, so no multiplier holds the bound.
    assert_eq!(tcap::multiplier(&[b"ab", b"ab", b"ab"]), None);
    assert_eq!(tcap::walk(&[b"ab", b"ab"], 1), 1);
}

#[test]
fn each_check_the_key_builds_make_fires() {
    let (t, k, i): (Build, Build, Build) = (tcap::build, termcap_keys::build, iterm_keys::build);
    let help = "\"khlp\",\t\tXK_Help,\t0\t)";
    each_fires(&[
        // xtermcap.c: its header, the table's bounds, a row, a group and
        // a BASE not read, a state, a group with no BASE, the counts.
        (
            t,
            XTERMCAP,
            "xtermcap.c,v 1.70",
            "xtermcap.c,v 1.71",
            Kind::Header,
            "is this the pinned file [xterm-xtermcap-c]?",
        ),
        (
            k,
            XTERMCAP,
            "xtermcap.c,v 1.70",
            "xtermcap.c,v 1.71",
            Kind::Header,
            "is this the pinned file [xterm-xtermcap-c]?",
        ),
        (
            t,
            XTERMCAP,
            "TCAPINFO table[]",
            "TCAPINFO tabel[]",
            Kind::Control,
            "the table was not found",
        ),
        (
            k,
            XTERMCAP,
            "#undef DATA",
            "#undef DAT A",
            Kind::Control,
            "after the table",
        ),
        (
            t,
            XTERMCAP,
            help,
            "\"khlp\",\t\tXK_Help,\tCTRL\t)",
            Kind::UnknownValue,
            "state CTRL, neither 0 nor SHIFT",
        ),
        (
            t,
            XTERMCAP,
            "DATA(\t\"%1\"",
            "DATA(\t%1\"",
            Kind::Malformed,
            "a DATA row not read",
        ),
        (
            k,
            XTERMCAP,
            "DGRP(DN,   XK_Down)",
            "DGRP(DN XK_Down)",
            Kind::Malformed,
            "a DGRP group not read",
        ),
        (
            t,
            XTERMCAP,
            "#define BASE \"kp\"",
            "#define BASE kp",
            Kind::Malformed,
            "a BASE not read",
        ),
        (
            t,
            XTERMCAP,
            "#define BASE \"k\"",
            "/* no BASE */",
            Kind::Malformed,
            "a DGRP with no BASE",
        ),
        (
            t,
            XTERMCAP,
            "\tDATA(\t\"%1\",\t\"khlp\",\t\tXK_Help,\t0\t),\n",
            "",
            Kind::Count,
            "109 DATA rows, 110 expected",
        ),
        (
            k,
            XTERMCAP,
            "\tDGRP(DN,   XK_Down),\n",
            "",
            Kind::Count,
            "16 DGRP groups, 17 expected",
        ),
        // A keysym neither table has a key for, a name past seven bytes,
        // one name fewer.
        (
            t,
            XTERMCAP,
            help,
            "\"khlp\",\t\tXK_Hlep,\t0\t)",
            Kind::UnknownValue,
            "no key for XK_Hlep",
        ),
        (
            k,
            XTERMCAP,
            help,
            "\"khlp\",\t\tXK_Hlep,\t0\t)",
            Kind::UnknownValue,
            "no key for XK_Hlep",
        ),
        (
            t,
            XTERMCAP,
            "\"khlp\"",
            "\"khlpkhlp\"",
            Kind::Invariant,
            "\"khlpkhlp\" is outside 2..=7 bytes",
        ),
        (
            t,
            XTERMCAP,
            "\"kHLP\"",
            "\"khlp\"",
            Kind::Count,
            "318 distinct names, 319 expected",
        ),
        // The entry: its two header lines, the two controls, an escape
        // terminfo(5) does not define, a capability twice, one key fewer.
        (
            k,
            ENTRY,
            "Reconstructed via infocmp",
            "Reconstructed by infocmp",
            Kind::Header,
            "is this the pinned file [terminfo-xterm-256color]?",
        ),
        (
            k,
            ENTRY,
            "with 256 colors,",
            "with 88 colors,",
            Kind::Header,
            "line 2 is not",
        ),
        (
            k,
            ENTRY,
            "kcuu1=\\EOA,",
            "kcuu1=\\E[A,",
            Kind::Control,
            "kcuu1 is",
        ),
        (
            k,
            ENTRY,
            "kf13=\\E[1;2P,",
            "kf13=\\E[1;2X,",
            Kind::Control,
            "kf13 is",
        ),
        (
            k,
            ENTRY,
            "kcud1=\\EOB,",
            "kcud1=\\qOB,",
            Kind::Malformed,
            "an escape terminfo(5) does not define in kcud1",
        ),
        (
            k,
            ENTRY,
            "\tkcud1=\\EOB,\n",
            "\tkcud1=\\EOB,\n\tkcud1=\\EOB,\n",
            Kind::Duplicate,
            "kcud1 is given already on line 74",
        ),
        (
            k,
            ENTRY,
            "\tkDC=\\E[3;2~,\n",
            "",
            Kind::Count,
            "106 keys and parameters given a string, 107 expected",
        ),
        // The iTerm2 names: the headers, a key that is no identifier, the
        // placeholder, a key twice or out of order, a page, a field
        // missing, every control and both counts.
        (
            i,
            DOC_KEYS,
            "at sha256 7ee11f94",
            "at sha256 7ee11f95",
            Kind::Header,
            "is this the pinned file [iterm2-osc1337-doc-keys]?",
        ),
        (
            i,
            SOURCE_KEYS,
            "at sha256 37fd1636",
            "at sha256 37fd1637",
            Kind::Header,
            "is this the pinned file [iterm2-osc1337-source-keys]?",
        ),
        (
            i,
            DOC_KEYS,
            "\nAddAnnotation;",
            "\nAdd-Annotation;",
            Kind::Malformed,
            "\"Add-Annotation\" is not a key",
        ),
        (
            i,
            SOURCE_KEYS,
            "\nCopy\n",
            "\nkey\n",
            Kind::Control,
            "the grammar's placeholder is not a key",
        ),
        (
            i,
            DOC_KEYS,
            "\nAddHiddenAnnotation;",
            "\nAddAnnotation;",
            Kind::Duplicate,
            "AddAnnotation is listed already on line 6",
        ),
        (
            i,
            DOC_KEYS,
            "\nBlock;",
            "\nZlock;",
            Kind::Invariant,
            "Button is not after Zlock",
        ),
        (
            i,
            DOC_KEYS,
            "Block; iterm2-escape-codes",
            "Block; iterm2-website",
            Kind::UnknownValue,
            "\"iterm2-website\" is none of the four pages",
        ),
        (
            i,
            DOC_KEYS,
            "Block; iterm2-escape-codes",
            "Block",
            Kind::FieldCount,
            "1 fields, 2 expected",
        ),
        (
            i,
            DOC_KEYS,
            "SetProfile; iterm2-escape-codes",
            "SetProfile; iterm2-images",
            Kind::Control,
            "control: SetProfile is not found in iterm2-escape-codes",
        ),
        (
            i,
            DOC_KEYS,
            "FileEnd; iterm2-images",
            "FileEnd; iterm2-escape-codes",
            Kind::Control,
            "control: FileEnd is not found in iterm2-images",
        ),
        (
            i,
            DOC_KEYS,
            "SetBadgeFormat; iterm2-badges",
            "SetBadgeFormat; iterm2-escape-codes",
            Kind::Control,
            "control: SetBadgeFormat is not found in iterm2-badges",
        ),
        (
            i,
            DOC_KEYS,
            "Capabilities; iterm2-feature-reporting",
            "Capabilities; iterm2-escape-codes",
            Kind::Control,
            "control: Capabilities is not found in iterm2-feature-reporting",
        ),
        (
            i,
            SOURCE_KEYS,
            "\nCursorShape\n",
            "\nCursorShapes\n",
            Kind::Control,
            "control: CursorShape is missing",
        ),
        (
            i,
            SOURCE_KEYS,
            "\nExecFailed\n",
            "\nExecFail\n",
            Kind::Control,
            "control: ExecFailed is missing",
        ),
        (
            i,
            DOC_KEYS,
            "Button; iterm2-escape-codes\n",
            "",
            Kind::Count,
            "38 keys, 39 expected",
        ),
        (
            i,
            SOURCE_KEYS,
            "\nCurrentDir\n",
            "\n",
            Kind::Count,
            "53 keys, 54 expected",
        ),
    ]);
}
