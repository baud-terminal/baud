//! What the faces baud embeds map of Hebrew, against fontTools 4.63.0's
//! `getBestCmap` on the same files: the counts below are that reader's, and
//! baud-font's cmap reader has to find them too. Stock JetBrains Mono has no
//! Hebrew; the built faces add it, and that row is the positive control, so
//! a reader that found nothing would fail it as surely as one that found too
//! much would fail the rest.

use baud_domain::Codepoint;
use baud_font::{Face, Tag};

/// The Hebrew letters.
const LETTERS: core::ops::RangeInclusive<char> = '\u{05D0}'..='\u{05EA}';
/// The Hebrew points.
const POINTS: core::ops::RangeInclusive<char> = '\u{05B0}'..='\u{05C7}';

/// How many of a range a face maps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Mapped(usize);

/// Whether a face maps U+FFFD.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Replacement {
    Mapped,
    Absent,
}

/// What fontTools read of an embedded face.
struct Row {
    file: &'static str,
    letters: Mapped,
    points: Mapped,
    /// A character the face must map, so a face read as empty fails.
    control: char,
    replacement: Replacement,
}

const ROWS: [Row; 5] = [
    Row {
        file: "built/Regular.ttf",
        letters: Mapped(27),
        points: Mapped(24),
        control: 'A',
        replacement: Replacement::Mapped,
    },
    Row {
        file: "built/Bold.ttf",
        letters: Mapped(27),
        points: Mapped(24),
        control: 'A',
        replacement: Replacement::Mapped,
    },
    Row {
        file: "jetbrains-mono/JetBrainsMono-Regular.ttf",
        letters: Mapped(0),
        points: Mapped(0),
        control: 'A',
        replacement: Replacement::Mapped,
    },
    Row {
        file: "stix-two-math/STIXTwoMath-Regular.otf",
        letters: Mapped(0),
        points: Mapped(0),
        control: '\u{23F5}',
        replacement: Replacement::Mapped,
    },
    Row {
        file: "symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf",
        letters: Mapped(0),
        points: Mapped(0),
        control: '\u{F07B}',
        replacement: Replacement::Absent,
    },
];

fn font(file: &str) -> Vec<u8> {
    let path = format!("{}/../baud-app/fonts/{file}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn maps(face: &Face<'_>, c: char) -> bool {
    face.glyph(Codepoint::from_char(c)).is_some()
}

fn count(face: &Face<'_>, range: core::ops::RangeInclusive<char>) -> Mapped {
    Mapped(range.filter(|c| maps(face, *c)).count())
}

/// `OS/2.fsType`: the field past `version`, `xAvgCharWidth`, `usWeightClass`
/// and `usWidthClass`.
fn embedding_bits(face: &Face<'_>) -> Option<u16> {
    let os2 = face.table(Tag::from_bytes(*b"OS/2"))?;
    os2.read(baud_font::bytes::Offset::of(8u32))
}

/// **Each embedded face maps what fontTools says**, its control included,
/// U+05FF (unassigned in the Hebrew block) nowhere, and each may be embedded
/// in a program: `fsType` 0 is installable.
#[test]
fn each_embedded_face_maps_what_fonttools_read() {
    for r in &ROWS {
        let data = font(r.file);
        let face = Face::parse(&data).unwrap_or_else(|e| panic!("{}: {e:?}", r.file));
        assert_eq!(count(&face, LETTERS), r.letters, "{}: letters", r.file);
        assert_eq!(count(&face, POINTS), r.points, "{}: points", r.file);
        assert!(maps(&face, r.control), "{}: {:?}", r.file, r.control);
        assert!(!maps(&face, '\u{05FF}'), "{}: U+05FF is unassigned", r.file);
        let replacement = if maps(&face, '\u{FFFD}') {
            Replacement::Mapped
        } else {
            Replacement::Absent
        };
        assert_eq!(replacement, r.replacement, "{}: U+FFFD", r.file);
        assert_eq!(embedding_bits(&face), Some(0), "{}: fsType", r.file);
    }
}

/// **The check can fire**: the same face with its `cmap` renamed away maps
/// nothing, and the control is what says so.
#[test]
fn a_face_with_no_cmap_fails_the_control() {
    let mut data = font("built/Regular.ttf");
    let at = data
        .windows(4)
        .take(12 + 16 * 40)
        .position(|w| w == b"cmap")
        .expect("the directory names cmap");
    data[at..at + 4].copy_from_slice(b"cmaq");
    let face = Face::parse(&data).expect("cmap is not required");
    assert_eq!(count(&face, LETTERS), Mapped(0));
    assert!(!maps(&face, 'A'));
}

/// The ranges are the Hebrew block's whole letters and points: 27 and 24.
#[test]
fn the_ranges_hold_what_the_rows_count() {
    assert_eq!(LETTERS.count(), 27);
    assert_eq!(POINTS.count(), 24);
}
