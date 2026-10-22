//! A face's family from its `name` table (`[[source: ms-opentype-name]]`):
//! nameID 16 over 1, Windows English (United States) over the other
//! records, UTF-16BE, and Mac Roman where it is ASCII. The tables here are
//! built to the specification's layout and spliced into JetBrains Mono in
//! place of its own, so the face around them is a real one.

use baud_font::Face;

const JETBRAINS: &[u8] = include_bytes!("fonts/JetBrainsMono-Regular.ttf");

/// A `name` table holding `records` (platform, encoding, language, id,
/// string), laid out as the specification gives it.
fn table(records: &[(u16, u16, u16, u16, &[u8])]) -> Vec<u8> {
    let n = u16::try_from(records.len()).unwrap();
    let mut head = Vec::new();
    let mut store = Vec::new();
    for v in [0, n, 6 + 12 * n] {
        head.extend_from_slice(&v.to_be_bytes());
    }
    for &(p, e, l, id, s) in records {
        let len = u16::try_from(s.len()).unwrap();
        let off = u16::try_from(store.len()).unwrap();
        for v in [p, e, l, id, len, off] {
            head.extend_from_slice(&v.to_be_bytes());
        }
        store.extend_from_slice(s);
    }
    head.extend_from_slice(&store);
    head
}

/// JetBrains Mono with its `name` record pointed at `name`, appended.
fn spliced(name: &[u8]) -> Vec<u8> {
    let mut f = JETBRAINS.to_vec();
    let tables = usize::from(u16::from_be_bytes([f[4], f[5]]));
    let rec = (0..tables)
        .map(|i| 12 + 16 * i)
        .find(|&r| &f[r..r + 4] == b"name")
        .expect("JetBrains Mono has a name table");
    while !f.len().is_multiple_of(4) {
        f.push(0);
    }
    let at = u32::try_from(f.len()).unwrap();
    f.extend_from_slice(name);
    f[rec + 8..rec + 12].copy_from_slice(&at.to_be_bytes());
    f[rec + 12..rec + 16].copy_from_slice(&u32::try_from(name.len()).unwrap().to_be_bytes());
    f
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

fn family_of(name: &[u8]) -> Option<String> {
    let data = spliced(name);
    let face = Face::parse(&data).ok()?;
    let mut out = [0u8; 64];
    let f = face.family()?;
    f.utf8(&mut out)
        .map(|b| String::from_utf8(b.to_vec()).unwrap())
}

/// **The real face's family**, the positive control the spliced cases rest
/// on.
#[test]
fn jetbrains_mono_names_its_family() {
    let face = Face::parse(JETBRAINS).unwrap();
    let mut out = [0u8; 64];
    let got = face
        .family()
        .and_then(|f| f.utf8(&mut out).map(<[u8]>::to_vec));
    assert_eq!(got.as_deref(), Some(&b"JetBrains Mono"[..]));
}

/// **nameID 16 over 1, and Windows English over the other records.**
#[test]
fn the_typographic_family_and_the_preferred_record_win() {
    let mac = b"Mac Name".as_slice();
    let (fr, us, one) = (utf16("Nom"), utf16("Typo"), utf16("Family"));
    let t = table(&[
        (1, 0, 0, 16, mac),
        (3, 1, 0x040c, 16, &fr),
        (3, 1, 0x0409, 1, &one),
        (3, 1, 0x0409, 16, &us),
    ]);
    assert_eq!(family_of(&t).as_deref(), Some("Typo"));
    let t = table(&[(3, 1, 0x040c, 1, &fr), (0, 3, 0, 1, &one)]);
    assert_eq!(family_of(&t).as_deref(), Some("Nom"), "any Windows record");
    let t = table(&[(1, 0, 0, 1, mac), (3, 1, 0x0409, 1, &one)]);
    assert_eq!(family_of(&t).as_deref(), Some("Family"), "no 16");
    let t = table(&[(1, 0, 0, 1, mac)]);
    assert_eq!(
        family_of(&t).as_deref(),
        Some("Mac Name"),
        "Mac Roman alone"
    );
    let t = table(&[(3, 1, 0x0409, 2, &one)]);
    assert_eq!(family_of(&t), None, "no family record");
    let astral = utf16("A\u{1F600}");
    let t = table(&[(3, 10, 0x0409, 1, &astral)]);
    assert_eq!(
        family_of(&t).as_deref(),
        Some("A\u{1F600}"),
        "a surrogate pair"
    );
}

/// **A record past the table, an unpaired surrogate and a Mac byte past
/// ASCII read as no name**, never as a cut one.
#[test]
fn a_name_that_does_not_read_is_none() {
    let mut t = table(&[(3, 1, 0x0409, 1, &utf16("Name"))]);
    t.truncate(t.len() - 2);
    assert_eq!(family_of(&t), None, "past the table");
    let t = table(&[(3, 1, 0x0409, 1, &[0xd8, 0x00, 0x00, 0x41])]);
    assert_eq!(family_of(&t), None, "unpaired surrogate");
    let t = table(&[(1, 0, 0, 1, &[0x41, 0xa5])]);
    assert_eq!(family_of(&t), None, "Mac Roman past ASCII");
}

/// **A name matches without its ASCII case, and only whole.**
#[test]
fn a_name_matches_whole_without_case() {
    let face = Face::parse(JETBRAINS).unwrap();
    let f = face.family().unwrap();
    assert!(f.matches(b"JetBrains Mono"));
    assert!(f.matches(b"jetbrains MONO"));
    assert!(!f.matches(b"JetBrains"));
    assert!(!f.matches(b"JetBrains Mono NL"));
    assert!(!f.matches(b""));
    assert!(!f.matches(b"\xff"));
}

/// **A family answers by default caseless match, under any of its
/// family records** (`[[source: css-fonts-3]]` 5.1: "user agents must match
/// any of these names independent of the underlying system locale"):
/// nameID 16 and 1, every language, `ß` matching `ss`, and nothing
/// normalised, so a precomposed letter does not match its decomposition.
#[test]
fn a_family_answers_to_every_record_caselessly() {
    let (fr, us, one, sub) = (
        utf16("Nom Fran\u{E7}ais"),
        utf16("Stra\u{DF}e Mono"),
        utf16("Stra\u{DF}e Mono Bold"),
        utf16("Bold"),
    );
    let t = table(&[
        (3, 1, 0x040c, 16, &fr),
        (3, 1, 0x0409, 16, &us),
        (3, 1, 0x0409, 1, &one),
        (3, 1, 0x0409, 2, &sub),
    ]);
    let data = spliced(&t);
    let face = Face::parse(&data).unwrap();
    assert!(face.family().unwrap().matches("STRASSE MONO".as_bytes()));
    for name in ["STRASSE MONO", "nom fran\u{C7}ais", "strasse mono bold"] {
        assert!(face.answers_to(name.as_bytes()), "{name}");
        let d = baud_font::Described::of(&data, baud_font::FaceIndex::FIRST).unwrap();
        assert!(d.answers_to(name.as_bytes()), "{name}");
    }
    for name in ["Bold", "Strasse", "nom franc\u{327}ais", ""] {
        assert!(!face.answers_to(name.as_bytes()), "{name:?}");
    }
}
