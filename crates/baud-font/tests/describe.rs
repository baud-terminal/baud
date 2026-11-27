//! **A face described for discovery**, read from its directory, `name`,
//! `OS/2`, `head`, `post` and `fvar` and nothing else: every family record
//! (`[[source: ms-opentype-name]]` nameIDs 16 and 1, in every language),
//! the weight from `usWeightClass` and the width from `usWidthClass`
//! (`[[source: ms-opentype-os2-2026-10-21]]`), the slope from `fsSelection`
//! bits 0 and 9, and the variation axes a face offers.

use baud_font::{Described, FaceIndex, Slope, Tag};

const JETBRAINS: &[u8] = include_bytes!("fonts/JetBrainsMono-Regular.ttf");
const JB_VAR: &[u8] = include_bytes!("fonts/JetBrainsMono-Variable.ttf");
const FIRA_VF: &[u8] = include_bytes!("fonts/FiraCode-VF.ttf");

fn embedded(name: &str) -> Vec<u8> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    std::fs::read(format!(
        "{root}/crates/baud-app/fonts/jetbrains-mono/{name}"
    ))
    .unwrap()
}

fn names(d: &Described<'_>) -> Vec<String> {
    d.families()
        .filter_map(|f| {
            let mut out = [0u8; 128];
            f.utf8(&mut out)
                .map(|b| String::from_utf8(b.to_vec()).unwrap())
        })
        .collect()
}

/// **The four JetBrains Mono styles read as the specification gives
/// them**: 400 and 700, upright and italic, width 100 %, one family.
#[test]
fn the_four_styles_of_a_family() {
    for (file, weight, italic) in [
        ("JetBrainsMono-Regular.ttf", 400.0, false),
        ("JetBrainsMono-Bold.ttf", 700.0, false),
        ("JetBrainsMono-Italic.ttf", 400.0, true),
        ("JetBrainsMono-BoldItalic.ttf", 700.0, true),
    ] {
        let data = embedded(file);
        let d = Described::of(&data, FaceIndex::FIRST).expect(file);
        assert_eq!(d.weight().get(), weight, "{file}");
        assert_eq!(d.width().get(), 100.0, "{file}");
        assert_eq!(
            d.slope() == Slope::Italic,
            italic,
            "{file}: {:?}",
            d.slope()
        );
        assert!(names(&d).iter().any(|n| n == "JetBrains Mono"), "{file}");
        assert!(d.family().unwrap().matches(b"jetbrains mono"));
    }
}

/// JetBrains Mono with its `OS/2` patched: `usWeightClass` at 4,
/// `fsSelection` at 62.
fn patched(weight: u16, selection: u16) -> Vec<u8> {
    let mut f = JETBRAINS.to_vec();
    let tables = usize::from(u16::from_be_bytes([f[4], f[5]]));
    let rec = (0..tables)
        .map(|i| 12 + 16 * i)
        .find(|&r| &f[r..r + 4] == b"OS/2")
        .unwrap();
    let at = usize::try_from(u32::from_be_bytes(f[rec + 8..rec + 12].try_into().unwrap())).unwrap();
    f[at + 4..at + 6].copy_from_slice(&weight.to_be_bytes());
    f[at + 62..at + 64].copy_from_slice(&selection.to_be_bytes());
    f
}

/// **The weight is `usWeightClass`, not the BOLD bit**: a 700 face whose
/// `fsSelection` names neither bold nor regular -- Apple SD Gothic Neo's
/// Bold is one -- is 700, and a 600 face with the bit set is 600.
/// **Italic is bit 0, oblique bit 9**, which the OS/2 text keeps apart.
#[test]
fn the_weight_is_the_weight_class_and_the_slope_two_bits() {
    let f = patched(700, 0);
    let d = Described::of(&f, FaceIndex::FIRST).unwrap();
    assert_eq!(d.weight().get(), 700.0);
    assert_eq!(d.slope(), Slope::Upright);
    let f = patched(600, 0x20);
    assert_eq!(
        Described::of(&f, FaceIndex::FIRST).unwrap().weight().get(),
        600.0
    );
    let f = patched(400, 0x01);
    assert_eq!(
        Described::of(&f, FaceIndex::FIRST).unwrap().slope(),
        Slope::Italic
    );
    let f = patched(400, 0x201);
    match Described::of(&f, FaceIndex::FIRST).unwrap().slope() {
        Slope::Oblique(a) => assert_eq!(a.get(), 14.0, "no italicAngle: CSS's 14 degrees"),
        other => panic!("{other:?}"),
    }
}

/// **A variable face offers its axes**: JetBrains Mono's wght runs 100 to
/// 800 around 400, FiraCode's 300 to 700 around 300.
#[test]
fn a_variable_face_offers_its_axes() {
    let wght = Tag::from_bytes(*b"wght");
    let d = Described::of(JB_VAR, FaceIndex::FIRST).unwrap();
    let a = d.axis(wght).expect("wght");
    assert_eq!(
        (a.min.0, a.default.0, a.max.0),
        (100 << 16, 400 << 16, 800 << 16)
    );
    let d = Described::of(FIRA_VF, FaceIndex::FIRST).unwrap();
    let a = d.axis(wght).expect("wght");
    assert_eq!(
        (a.min.0, a.default.0, a.max.0),
        (300 << 16, 300 << 16, 700 << 16)
    );
    assert!(d.axis(Tag::from_bytes(*b"ital")).is_none());
    let d = Described::of(JETBRAINS, FaceIndex::FIRST).unwrap();
    assert!(d.axis(wght).is_none(), "a static face");
}

/// **A collection's faces are described one by one**, and a face past its
/// end, or bytes that are not a font, describe nothing.
#[test]
fn a_collection_and_what_is_not_a_font() {
    assert!(Described::of(JETBRAINS, FaceIndex::of(1)).is_none());
    assert!(Described::of(b"not a font at all", FaceIndex::FIRST).is_none());
    assert!(Described::of(&[], FaceIndex::FIRST).is_none());
}
