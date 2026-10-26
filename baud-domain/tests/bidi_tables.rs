//! The bidirectional properties against the pinned UCD 17.0.0 files they
//! are generated from, read back for every code point: Bidi_Class
//! (`[[source: unicode-derivedbidiclass]]`), Bidi_Mirrored
//! (`[[source: unicode-data]]`), Bidi_Mirroring_Glyph
//! (`[[source: unicode-bidimirroring]]`) and the paired brackets
//! (`[[source: unicode-bidibrackets]]`).

use baud_domain::{
    BidiClass, BidiMirrored, BidiProps, BracketKind, mirroring_glyph, opens_rtl, paired_bracket,
};
use std::collections::HashMap;
use std::ops::RangeInclusive;

fn source(name: &str) -> String {
    let path = format!(
        "{}/../docs/research/sources/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A UCD range field, `0041..005A` or `0041`.
fn span(field: &str) -> RangeInclusive<u32> {
    let (lo, hi) = field
        .trim()
        .split_once("..")
        .unwrap_or((field.trim(), field.trim()));
    u32::from_str_radix(lo, 16).expect("hex")..=u32::from_str_radix(hi, 16).expect("hex")
}

/// Each data line's fields, comments and blank lines dropped.
fn data(text: &str) -> impl Iterator<Item = Vec<&str>> {
    text.lines().filter_map(|l| {
        let body = l.split('#').next().unwrap_or("").trim();
        (!body.is_empty()).then(|| body.split(';').map(str::trim).collect())
    })
}

fn every_char() -> impl Iterator<Item = char> {
    (0..=0x10FFFF_u32).filter_map(char::from_u32)
}

/// **The table's classes are the enum's, in order**, with a character of
/// each class the algorithm names, an unassigned code point in each of the
/// right-to-left and currency blocks the file gives a default, and a
/// noncharacter, which is BN.
#[test]
fn the_bidi_trie_agrees_with_the_class_enum() {
    use BidiClass::*;
    for (ch, want) in [
        ('a', L),
        ('\u{05D0}', R),
        ('\u{0627}', AL),
        ('0', EN),
        ('+', ES),
        ('$', ET),
        ('\u{0660}', AN),
        (',', CS),
        ('\u{0300}', NSM),
        ('\u{200B}', BN),
        ('\u{2029}', B),
        ('\t', S),
        (' ', WS),
        ('(', ON),
        ('\u{202A}', LRE),
        ('\u{202D}', LRO),
        ('\u{202B}', RLE),
        ('\u{202E}', RLO),
        ('\u{202C}', PDF),
        ('\u{2066}', LRI),
        ('\u{2067}', RLI),
        ('\u{2068}', FSI),
        ('\u{2069}', PDI),
        ('\u{05FF}', R),
        ('\u{07BF}', AL),
        ('\u{20CF}', ET),
        ('\u{FDD0}', BN),
        ('\u{0378}', L),
    ] {
        assert_eq!(BidiClass::of(ch), want, "U+{:04X}", u32::from(ch));
    }
    for (i, c) in BidiClass::ALL.iter().enumerate() {
        assert_eq!(BidiClass::ALL.iter().position(|d| d == c), Some(i));
    }
}

/// **Every code point has the Bidi_Class `DerivedBidiClass.txt` gives
/// it**: its listed value, else the last `@missing` line whose range holds
/// it. The positive control: the file lists over three hundred thousand
/// code points and gives 24 defaults, so a reader that stopped would fail.
#[test]
fn every_code_point_has_the_bidi_class_the_pinned_file_gives() {
    let text = source("unicode-derivedbidiclass.txt");
    let long = HashMap::from([
        ("Left_To_Right", BidiClass::L),
        ("Right_To_Left", BidiClass::R),
        ("Arabic_Letter", BidiClass::AL),
        ("European_Terminator", BidiClass::ET),
        ("Boundary_Neutral", BidiClass::BN),
    ]);
    let mut want = vec![None; 0x110000];
    let mut defaults = 0;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# @missing: ") {
            let (range, name) = rest.split_once(';').expect("a default");
            for cp in span(range) {
                want[usize::try_from(cp).expect("index")] = Some(long[name.trim()]);
            }
            defaults += 1;
        }
    }
    let mut listed = 0;
    for f in data(&text) {
        for cp in span(f[0]) {
            want[usize::try_from(cp).expect("index")] = BidiClass::from_name(f[1]);
            listed += 1;
        }
    }
    assert_eq!(defaults, 24);
    assert!(listed > 300_000, "only {listed} listed");
    let mut wrong = Vec::new();
    for c in every_char() {
        let w = want[usize::try_from(u32::from(c)).expect("index")].expect("a class");
        if BidiClass::of(c) != w {
            wrong.push(format!(
                "U+{:04X} {:?} want {w:?}",
                u32::from(c),
                BidiClass::of(c)
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} differ: {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(8)]
    );
}

/// **The renderer's fast path agrees with the class table at every code
/// point**: `opens_rtl` answers yes exactly for R, AL, AN, RLE, RLO and RLI,
/// whichever way its block bit and its threshold below U+0590 get there,
/// and no for every value past U+10FFFF, where a cell's reserved words and
/// cluster indices live. A block bit left clear, or a class left out of the
/// set, would draw a right-to-left line in its stored order.
#[test]
fn opens_rtl_is_the_class_table_at_every_code_point() {
    let mut wrong = Vec::new();
    let mut yes = 0;
    for c in every_char() {
        let want = matches!(
            BidiClass::of(c),
            BidiClass::R
                | BidiClass::AL
                | BidiClass::AN
                | BidiClass::RLE
                | BidiClass::RLO
                | BidiClass::RLI
        );
        yes += usize::from(want);
        if opens_rtl(u32::from(c)) != want {
            wrong.push(format!("U+{:04X}", u32::from(c)));
        }
    }
    for word in [
        0x11_0000,
        0x11_0001,
        0x11_0002,
        0x12_0001,
        0x100_0000,
        u32::MAX,
    ] {
        if opens_rtl(word) {
            wrong.push(format!("{word:#x}"));
        }
    }
    // The positive control: Hebrew alef, Arabic alef, an Arabic-Indic digit
    // and RLI are in; Latin A, a European digit and LRI are not.
    assert!(opens_rtl(0x05D0) && opens_rtl(0x0627) && opens_rtl(0x0661) && opens_rtl(0x2067));
    assert!(!opens_rtl(0x41) && !opens_rtl(0x31) && !opens_rtl(0x2066));
    assert!(yes > 5_000, "only {yes} right-to-left code points");
    assert!(
        wrong.is_empty(),
        "{} differ: {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(8)]
    );
}

/// **The mirrored characters with no Bidi_Mirroring_Glyph are the 126
/// BidiMirroring.txt lists as comments at its end**, under "no appropriate
/// mirroring character": those L4 leaves "up to the rendering system", 554
/// mirrored in all and 428 with a glyph.
#[test]
fn the_mirrored_characters_with_no_glyph_are_the_pinned_files_126() {
    let text = source("unicode-bidimirroring.txt");
    let (_, tail) = text
        .split_once("# The following characters have no appropriate mirroring character.")
        .expect("the section");
    let listed: Vec<char> = tail
        .lines()
        .filter_map(|l| l.strip_prefix("# ")?.split_once(';'))
        .filter_map(|(cp, _)| char::from_u32(u32::from_str_radix(cp.trim(), 16).ok()?))
        .collect();
    let mirrored: Vec<char> = every_char()
        .filter(|&c| BidiProps::of(c).mirrored == BidiMirrored::Yes)
        .collect();
    let glyphless: Vec<char> = mirrored
        .iter()
        .copied()
        .filter(|&c| mirroring_glyph(c).is_none())
        .collect();
    assert_eq!(glyphless, listed);
    assert_eq!(
        (mirrored.len(), glyphless.len()),
        (554, 126),
        "mirrored, and of them with no glyph"
    );
}

/// **Bidi_Mirrored is UnicodeData.txt's field 9 for every code point**, and
/// a different property from the glyph: U+2211 is mirrored with no glyph
/// named, and U+FD3E and U+FD3F, which L4 says "are not mirrored", are not.
#[test]
fn every_code_point_is_bidi_mirrored_where_the_pinned_file_says() {
    let text = source("unicode-unicodedata.txt");
    let mut want = vec![BidiMirrored::No; 0x110000];
    let mut first = None;
    let mut yes = 0;
    for line in text.lines() {
        let f: Vec<&str> = line.split(';').collect();
        let cp = u32::from_str_radix(f[0], 16).expect("hex");
        if f[1].ends_with(", First>") {
            first = Some(cp);
            continue;
        }
        let lo = if f[1].ends_with(", Last>") {
            first.take().expect("a First")
        } else {
            cp
        };
        if f[9] == "Y" {
            for c in lo..=cp {
                want[usize::try_from(c).expect("index")] = BidiMirrored::Yes;
                yes += 1;
            }
        }
    }
    assert!(yes > 500, "only {yes} mirrored");
    for c in every_char() {
        assert_eq!(
            BidiProps::of(c).mirrored,
            want[usize::try_from(u32::from(c)).expect("index")],
            "U+{:04X}",
            u32::from(c)
        );
    }
    assert_eq!(BidiProps::of('\u{2211}').mirrored, BidiMirrored::Yes);
    assert_eq!(mirroring_glyph('\u{2211}'), None);
    assert_eq!(BidiProps::of('\u{FD3E}').mirrored, BidiMirrored::No);
}

/// **Every Bidi_Mirroring_Glyph is the pinned file's**, and every other
/// code point has none: 428 mappings, `(` to `)` and back.
#[test]
fn every_mirroring_glyph_is_the_pinned_files() {
    let text = source("unicode-bidimirroring.txt");
    let want: HashMap<char, char> = data(&text)
        .map(|f| {
            let cp =
                |s: &str| char::from_u32(u32::from_str_radix(s, 16).expect("hex")).expect("a char");
            (cp(f[0]), cp(f[1]))
        })
        .collect();
    assert_eq!(want.len(), 428);
    assert_eq!(mirroring_glyph('('), Some(')'));
    assert_eq!(mirroring_glyph('\u{226E}'), Some('\u{226F}'));
    for c in every_char() {
        assert_eq!(
            mirroring_glyph(c),
            want.get(&c).copied(),
            "U+{:04X}",
            u32::from(c)
        );
    }
}

/// **Every paired bracket is the pinned file's, and pairs as BD16 pairs**:
/// a code point is open, close or neither as `BidiBrackets.txt` says; a
/// bracket and its Bidi_Paired_Bracket are one pair; the canonically
/// equivalent U+2329/U+3008 and U+232A/U+3009 are one pair; and no two
/// other brackets are.
#[test]
fn every_paired_bracket_is_the_pinned_files() {
    let text = source("unicode-bidibrackets.txt");
    let mut want = HashMap::new();
    for f in data(&text) {
        let cp =
            |s: &str| char::from_u32(u32::from_str_radix(s, 16).expect("hex")).expect("a char");
        let kind = match f[2] {
            "o" => BracketKind::Open,
            "c" => BracketKind::Close,
            _ => continue,
        };
        want.insert(cp(f[0]), (kind, cp(f[1])));
    }
    assert_eq!(want.len(), 128);
    let mut pairs = HashMap::new();
    for c in every_char() {
        let got = paired_bracket(c);
        assert_eq!(
            got.map(|b| b.kind),
            want.get(&c).map(|w| w.0),
            "U+{:04X}",
            u32::from(c)
        );
        assert_eq!(BidiProps::of(c).bracket, want.get(&c).map(|w| w.0));
        if let (Some(b), Some(&(_, other))) = (got, want.get(&c)) {
            assert_eq!(paired_bracket(other).map(|o| o.pair), Some(b.pair));
            assert_eq!(
                mirroring_glyph(c),
                Some(other),
                "Bidi_Paired_Bracket is the glyph"
            );
            pairs.entry(b.pair).or_insert_with(Vec::new).push(c);
        }
    }
    assert_eq!(pairs.len(), 63);
    for members in pairs.values() {
        let canonical = members.contains(&'\u{3008}') || members.contains(&'\u{3009}');
        assert_eq!(members.len(), if canonical { 4 } else { 2 }, "{members:?}");
    }
    assert_eq!(
        paired_bracket('\u{2329}').map(|b| b.pair),
        paired_bracket('\u{3009}').map(|b| b.pair)
    );
    assert_ne!(
        paired_bracket('(').map(|b| b.pair),
        paired_bracket('[').map(|b| b.pair)
    );
}

/// **The committed table is the pinned data's**: the generator, run again,
/// writes the same file.
#[test]
fn bidi_tables_match_the_pinned_data() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-bidi-tables.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "bidi_data.rs is stale -- rerun scripts/gen-bidi-tables.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// **Each Box Drawing character's counterpart is the name with left and
/// right exchanged**, for `?2500`: a corner and the other side's, a tee
/// and the other side's, the diagonals, the arcs, a half line and the
/// other half, and every character whose name has no side its own. Every
/// entry is its counterpart's counterpart; outside the block there is
/// none.
#[test]
fn every_box_drawing_counterpart_is_its_counterparts_counterpart() {
    use baud_domain::box_counterpart;
    for (a, b) in [
        ('\u{250C}', '\u{2510}'),
        ('\u{2514}', '\u{2518}'),
        ('\u{251C}', '\u{2524}'),
        ('\u{2552}', '\u{2555}'),
        ('\u{256D}', '\u{256E}'),
        ('\u{2571}', '\u{2572}'),
        ('\u{2574}', '\u{2576}'),
        ('\u{257C}', '\u{257E}'),
    ] {
        assert_eq!(box_counterpart(a), Some(b), "{a}");
        assert_eq!(box_counterpart(b), Some(a), "{b}");
    }
    for same in [
        '\u{2500}', '\u{2502}', '\u{253C}', '\u{2550}', '\u{2573}', '\u{2575}',
    ] {
        assert_eq!(box_counterpart(same), Some(same), "{same}");
    }
    let mut moved = 0;
    for c in '\u{2500}'..='\u{257F}' {
        let m = box_counterpart(c).expect("in the block");
        assert_eq!(box_counterpart(m), Some(c), "{c}");
        moved += usize::from(m != c);
    }
    assert_eq!(moved, 78, "the characters with a side");
    assert_eq!(box_counterpart('\u{24FF}'), None);
    assert_eq!(box_counterpart('\u{2580}'), None);
}
