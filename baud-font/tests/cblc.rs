//! `CBLC` sizes against the rules the lookup follows
//! (`[[source: ms-opentype-eblc]]`, which `CBLC` extends): "Records must be
//! sorted by firstGlyphIndex, and records should not have overlapping glyph ID
//! ranges". A size whose records run past the table or whose firsts decrease
//! is refused whole; a glyph at or past the face's `numGlyphs` has no bitmap;
//! on a size whose records overlap -- legal, since the overlap rule is a
//! "should not" -- the first record that holds the glyph answers, as FreeType,
//! ttf-parser and HarfBuzz all do; and an index subtable of format 4 finds a
//! glyph among its real pairs only, the extra one marking where the last image
//! ends.
//!
//! The fonts are the Noto Color Emoji subset (`tests/fonts/`), byte surgeries
//! on it, and `tests/bitmap/cblc-cases.ttf`, which `scripts/gen-cblc-fixture.py`
//! writes. `spec` is a model of the lookup written from the specification's
//! text, not from baud's reader, and every answer is held to it.

use baud_domain::GlyphId;
use baud_font::bytes::Index;
use baud_font::{Face, Records, StrikeRefusal, StrikeState};
use proptest::prelude::*;

const NOTO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fonts/NotoColorEmoji-subset.ttf"
);
const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/bitmap/cblc-cases.ttf");

fn read(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A table's offset and length, from the font's directory.
fn table(data: &[u8], tag: &[u8; 4]) -> (usize, usize) {
    let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let r = (0..n)
        .map(|i| 12 + 16 * i)
        .find(|&r| &data[r..r + 4] == tag)
        .unwrap_or_else(|| panic!("no {tag:?}"));
    let word = |at: usize| {
        usize::try_from(u32::from_be_bytes(data[at..at + 4].try_into().unwrap())).unwrap()
    };
    (word(r + 8), word(r + 12))
}

fn put16(data: &mut [u8], at: usize, v: u16) {
    data[at..at + 2].copy_from_slice(&v.to_be_bytes());
}

fn put32(data: &mut [u8], at: usize, v: u32) {
    data[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

/// `data` with its `CBLC` replaced by `cblc`: appended, four-aligned, and
/// its directory record repointed. Checksums are not read.
fn with_cblc(data: &[u8], cblc: &[u8]) -> Vec<u8> {
    let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let r = (0..n)
        .map(|i| 12 + 16 * i)
        .find(|&r| &data[r..r + 4] == b"CBLC")
        .unwrap();
    let mut out = data.to_vec();
    out.resize(out.len().next_multiple_of(4), 0);
    let at = u32::try_from(out.len()).unwrap();
    out.extend_from_slice(cblc);
    put32(&mut out, r + 8, at);
    put32(&mut out, r + 12, u32::try_from(cblc.len()).unwrap());
    out
}

/// The Noto subset's `CBLC` as two sizes: size 0 at 109 ppem with records
/// 0-3, and size 1 at 54 ppem with all five and records 0 and 1 swapped,
/// each size with its own copy of the 232-byte record and subtable block.
fn two_sizes(noto: &[u8]) -> Vec<u8> {
    let (at, len) = table(noto, b"CBLC");
    let c = &noto[at..at + len];
    let (size, block) = (&c[8..56], &c[56..288]);
    let (list0, list1) = (8 + 48 * 2, 8 + 48 * 2 + block.len());
    let mut out = Vec::new();
    out.extend_from_slice(&0x0003_0000u32.to_be_bytes());
    out.extend_from_slice(&2u32.to_be_bytes());
    let mut s0 = size.to_vec();
    put32(&mut s0, 0, u32::try_from(list0).unwrap());
    put32(&mut s0, 8, 4);
    let mut s1 = size.to_vec();
    put32(&mut s1, 0, u32::try_from(list1).unwrap());
    s1[44] = 54;
    s1[45] = 54;
    out.extend_from_slice(&s0);
    out.extend_from_slice(&s1);
    out.extend_from_slice(block);
    let mut b1 = block.to_vec();
    b1[0..8].copy_from_slice(&block[8..16]);
    b1[8..16].copy_from_slice(&block[0..8]);
    out.extend_from_slice(&b1);
    with_cblc(noto, &out)
}

/// The Noto subset's `CBLC` as two sizes sharing its record list, the
/// second claiming 2^20 records, which run past the table.
fn past_table_size(noto: &[u8]) -> Vec<u8> {
    let (at, len) = table(noto, b"CBLC");
    let c = &noto[at..at + len];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0003_0000u32.to_be_bytes());
    out.extend_from_slice(&2u32.to_be_bytes());
    let mut s0 = c[8..56].to_vec();
    put32(&mut s0, 0, 56 + 48);
    let mut s1 = s0.clone();
    put32(&mut s1, 8, 1 << 20);
    out.extend_from_slice(&s0);
    out.extend_from_slice(&s1);
    out.extend_from_slice(&c[56..288]);
    with_cblc(noto, &out)
}

/// The subset with one surgery inside its `CBLC`, at offsets relative to the
/// table: the five records sit at 56, 64, 72, 80 and 88 (first, last, offset),
/// and record 2's format-4 subtable keeps its count at 200 and its pairs from 204.
fn noto_with(edit: impl FnOnce(&mut [u8])) -> Vec<u8> {
    let mut data = read(NOTO);
    let (at, len) = table(&data, b"CBLC");
    edit(&mut data[at..at + len]);
    data
}

fn with_glyphs(data: &[u8], n: u16) -> Vec<u8> {
    let mut data = data.to_vec();
    let (maxp, _) = table(&data, b"maxp");
    put16(&mut data, maxp + 4, n);
    data
}

fn png(face: &Face<'_>, i: u32, g: u16) -> Option<Vec<u8>> {
    face.bitmap(Index::of(i), GlyphId::from_shaper(g))
        .map(|b| b.png.as_slice().to_vec())
}

/// The clean subset's PNG of glyph `g`, which every surgery that leaves the
/// glyph's record alone must answer exactly.
fn own(g: u16) -> Vec<u8> {
    let noto = read(NOTO);
    png(&Face::parse(&noto).unwrap(), 0, g)
        .unwrap_or_else(|| panic!("the clean subset draws glyph {g}"))
}

// ------------------------------------------------------------------ the model

/// What a size is refused for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Refusal {
    PastTable,
    Unsorted(Index),
}

/// An image as the specification lays it out: the PNG and the bearings.
#[derive(PartialEq, Eq, Debug)]
struct Image {
    png: Vec<u8>,
    x: i8,
    y: i8,
}

fn be16(d: &[u8], at: u64) -> Option<u16> {
    let at = usize::try_from(at).ok()?;
    Some(u16::from_be_bytes(
        d.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn be32(d: &[u8], at: u64) -> Option<u32> {
    let at = usize::try_from(at).ok()?;
    Some(u32::from_be_bytes(
        d.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn i8_at(d: &[u8], at: u64) -> Option<i8> {
    d.get(usize::try_from(at).ok()?)
        .map(|b| i8::from_ne_bytes([*b]))
}

/// The first of `n` sorted u16 keys, read by `key`, at or above `g`: the
/// position a sorted list gives `g`.
fn lower_bound(n: u32, g: u16, key: impl Fn(u32) -> Option<u16>) -> Option<u32> {
    let mut j = 0;
    while j < n {
        if key(j)? >= g {
            return Some(j);
        }
        j += 1;
    }
    Some(n)
}

/// Size `i`'s answer for glyph `g`, from EBLC's text (which CBLC extends) and
/// the choices above: the table holds at most 255 sizes whose records fit; a
/// size is refused when its index subtable records run past the table or a
/// record's first glyph is below the one before it; a glyph answers only
/// below `glyphs` and inside the size's start..end range, from the first
/// record whose range holds it; format 4's pairs are sorted by glyph id and
/// the extra pair names no glyph; then the subtable's and the image's formats
/// as the specification lays them out.
fn spec(cblc: &[u8], cbdt: &[u8], glyphs: u32, i: u32, g: u32) -> Result<Option<Image>, Refusal> {
    let Some(sizes) = be32(cblc, 4) else {
        return Ok(None);
    };
    if sizes > 255 || 8 + 48 * u64::from(sizes) > cblc.len() as u64 || i >= sizes {
        return Ok(None);
    }
    let size = 8 + 48 * u64::from(i);
    let (Some(list), Some(count)) = (be32(cblc, size), be32(cblc, size + 8)) else {
        return Ok(None);
    };
    if u64::from(list) + 8 * u64::from(count) > cblc.len() as u64 {
        return Err(Refusal::PastTable);
    }
    let rec = |j: u32| {
        let at = u64::from(list) + 8 * u64::from(j);
        (
            be16(cblc, at).unwrap(),
            be16(cblc, at + 2).unwrap(),
            be32(cblc, at + 4).unwrap(),
        )
    };
    for j in 1..count {
        if rec(j).0 < rec(j - 1).0 {
            return Err(Refusal::Unsorted(Index::of(j)));
        }
    }
    let answer = || {
        let start = u32::from(be16(cblc, size + 40)?);
        let end = u32::from(be16(cblc, size + 42)?);
        if g >= glyphs || g < start || g > end {
            return None;
        }
        let g = u16::try_from(g).ok()?;
        let (first, _, off) = (0..count).map(rec).find(|&(f, l, _)| f <= g && g <= l)?;
        let header = u64::from(list) + u64::from(off);
        let index_format = be16(cblc, header)?;
        let image_format = be16(cblc, header + 2)?;
        let data = u64::from(be32(cblc, header + 4)?);
        let body = header + 8;
        let diff = u64::from(g - first);
        // A subtable that states a size shares one bigGlyphMetrics: height,
        // width, then the horizontal bearings.
        let shared = |at: u64| Some((i8_at(cblc, at + 2)?, i8_at(cblc, at + 3)?));
        let (image, metrics) = match index_format {
            1 => (data + u64::from(be32(cblc, body + 4 * diff)?), None),
            2 => (
                data + u64::from(be32(cblc, body)?) * diff,
                Some(shared(body + 4)?),
            ),
            3 => (data + u64::from(be16(cblc, body + 2 * diff)?), None),
            4 => {
                let n = be32(cblc, body)?;
                // The n + 1 pairs must lie in the table; only the first n name glyphs.
                if body + 4 + 4 * (u64::from(n) + 1) > cblc.len() as u64 {
                    return None;
                }
                let pair = |k: u32| be16(cblc, body + 4 + 4 * u64::from(k));
                let k = lower_bound(n, g, pair)?;
                if k == n || pair(k)? != g {
                    return None;
                }
                (
                    data + u64::from(be16(cblc, body + 4 + 4 * u64::from(k) + 2)?),
                    None,
                )
            }
            5 => {
                let n = be32(cblc, body + 12)?;
                let id = |k: u32| be16(cblc, body + 16 + 2 * u64::from(k));
                if body + 16 + 2 * u64::from(n) > cblc.len() as u64 {
                    return None;
                }
                let k = lower_bound(n, g, id)?;
                if k == n || id(k)? != g {
                    return None;
                }
                (
                    data + u64::from(be32(cblc, body)?) * u64::from(k),
                    Some(shared(body + 4)?),
                )
            }
            _ => return None,
        };
        // Image formats 17 (smallGlyphMetrics, 5 bytes), 18 (bigGlyphMetrics,
        // 8 bytes) and 19 (none: the subtable's), each then a u32 length and the PNG.
        let (len_at, x, y) = match image_format {
            17 => (image + 5, i8_at(cbdt, image + 2)?, i8_at(cbdt, image + 3)?),
            18 => (image + 8, i8_at(cbdt, image + 2)?, i8_at(cbdt, image + 3)?),
            19 => {
                let (x, y) = metrics?;
                (image, x, y)
            }
            _ => return None,
        };
        let len = u64::from(be32(cbdt, len_at)?);
        let from = usize::try_from(len_at + 4).ok()?;
        let png = cbdt
            .get(from..from.checked_add(usize::try_from(len).ok()?)?)?
            .to_vec();
        Some(Image { png, x, y })
    };
    Ok(answer())
}

/// Size `i`'s state by the specification: `None` where there is no size `i`
/// (or the table is refused whole); else refused, or read with the search the
/// records allow -- disjoint when every record starts after the last glyph of
/// the one before, else a staircase when every record also ends no earlier
/// than the one before, else nested.
fn spec_state(cblc: &[u8], i: u32) -> Option<Result<Records, Refusal>> {
    let sizes = be32(cblc, 4)?;
    if sizes > 255 || 8 + 48 * u64::from(sizes) > cblc.len() as u64 || i >= sizes {
        return None;
    }
    let size = 8 + 48 * u64::from(i);
    let (list, count) = (be32(cblc, size)?, be32(cblc, size + 8)?);
    if u64::from(list) + 8 * u64::from(count) > cblc.len() as u64 {
        return Some(Err(Refusal::PastTable));
    }
    let rec = |j: u32| {
        let at = u64::from(list) + 8 * u64::from(j);
        (be16(cblc, at).unwrap(), be16(cblc, at + 2).unwrap())
    };
    let (mut disjoint, mut staircase) = (true, true);
    for j in 1..count {
        let ((f0, l0), (f1, l1)) = (rec(j - 1), rec(j));
        if f1 < f0 {
            return Some(Err(Refusal::Unsorted(Index::of(j))));
        }
        disjoint &= f1 > l0;
        staircase &= l1 >= l0;
    }
    Some(Ok(if disjoint {
        Records::Disjoint
    } else if staircase {
        Records::Staircase
    } else {
        Records::Nested
    }))
}

/// Every surgery the tests make, by name, for the model sweep.
fn surgeries() -> Vec<(&'static str, Vec<u8>)> {
    let noto = read(NOTO);
    vec![
        ("the clean subset", noto.clone()),
        ("cblc-cases.ttf", read(CASES)),
        (
            "the subset's records past the table",
            noto_with(|c| put32(c, 16, 100)),
        ),
        ("two sizes, the second unsorted", two_sizes(&noto)),
        (
            "two sizes, the second claiming 2^20 records",
            past_table_size(&noto),
        ),
        ("numGlyphs 40", with_glyphs(&noto, 40)),
        ("record 0's last 15 -> 20", noto_with(|c| put16(c, 58, 20))),
        (
            "record 1 = [10..12]",
            noto_with(|c| {
                put16(c, 64, 10);
                put16(c, 66, 12);
            }),
        ),
        (
            "record 1 = [4..15]",
            noto_with(|c| {
                put16(c, 64, 4);
                put16(c, 66, 15);
            }),
        ),
        (
            "record 2 = [33..28]",
            noto_with(|c| {
                put16(c, 72, 33);
                put16(c, 74, 28);
            }),
        ),
        ("format 4's count 6 -> 5", noto_with(|c| put32(c, 200, 5))),
        (
            "format 4's count 6 -> 21: 21 pairs end at the table's end, 22 do not",
            noto_with(|c| put32(c, 200, 21)),
        ),
        (
            "format 4's pairs 1 and 2 swapped",
            noto_with(|c| {
                let (p1, p2) = (c[208..212].to_vec(), c[212..216].to_vec());
                c[208..212].copy_from_slice(&p2);
                c[212..216].copy_from_slice(&p1);
            }),
        ),
    ]
}

/// Every size of `data` at every glyph below `numGlyphs + 3` and at 0xFFFF:
/// what `Face::bitmap` answers against what the model says.
fn sweep(name: &str, data: &[u8]) {
    let face = Face::parse(data).unwrap();
    let (cl, cll) = table(data, b"CBLC");
    let (cd, cdl) = table(data, b"CBDT");
    let (maxp, _) = table(data, b"maxp");
    let glyphs = u32::from(u16::from_be_bytes([data[maxp + 4], data[maxp + 5]]));
    let (cblc, cbdt) = (&data[cl..cl + cll], &data[cd..cd + cdl]);
    for i in 0..face.strikes().value() {
        let state = face.strike(Index::of(i));
        let read = match (spec_state(cblc, i), state) {
            (Some(Ok(mode)), Some(StrikeState::Read(s))) => {
                assert_eq!(
                    (s.index(), s.records()),
                    (Index::of(i), mode),
                    "{name}: size {i}"
                );
                Some(s)
            }
            (
                Some(Err(Refusal::PastTable)),
                Some(StrikeState::Refused(StrikeRefusal::PastTable)),
            ) => None,
            (
                Some(Err(Refusal::Unsorted(j))),
                Some(StrikeState::Refused(StrikeRefusal::Unsorted { at })),
            ) => {
                assert_eq!(
                    at,
                    j,
                    "{name}: size {i} is unsorted at record {}",
                    j.value()
                );
                None
            }
            (want, got) => panic!("{name}: size {i} is {got:?}; the model says {want:?}"),
        };
        for g in (0..glyphs + 3).chain([0xFFFF]) {
            let want = spec(cblc, cbdt, glyphs, i, g);
            let glyph = GlyphId::from_shaper(u16::try_from(g).unwrap());
            let image = |b: baud_font::Bitmap<'_>| Image {
                png: b.png.as_slice().to_vec(),
                x: i8::try_from(b.placement.x).unwrap(),
                y: i8::try_from(b.placement.y).unwrap(),
            };
            let got = face.bitmap(Index::of(i), glyph).map(image);
            match &want {
                Ok(want) => assert!(
                    &got == want,
                    "{name}: size {i}, glyph {g}: {} against the model's {}",
                    show(&got),
                    show(want)
                ),
                Err(r) => assert!(
                    got.is_none(),
                    "{name}: size {i} is refused ({r:?}), yet glyph {g} answers"
                ),
            }
            if let Some(s) = read {
                assert!(
                    face.bitmap_in(s, glyph).map(image) == got,
                    "{name}: size {i}, glyph {g}: bitmap_in disagrees"
                );
            }
        }
    }
}

fn show(i: &Option<Image>) -> String {
    match i {
        Some(i) => format!("a {}-byte image at ({}, {})", i.png.len(), i.x, i.y),
        None => "nothing".to_string(),
    }
}

// ------------------------------------------------------------------ tests

#[test]
fn an_unsorted_size_is_refused_and_the_rest_of_the_table_draws() {
    let data = two_sizes(&read(NOTO));
    let face = Face::parse(&data).unwrap();
    assert_eq!(face.strikes().value(), 2);
    assert_eq!(face.strike_ppem(Index::of(1u32)).unwrap().value(), 54);
    for g in 0..50 {
        assert!(
            png(&face, 1, g).is_none(),
            "the unsorted size answers glyph {g}"
        );
    }
    for g in 4..=39 {
        assert_eq!(png(&face, 0, g), Some(own(g)), "size 0, glyph {g}");
    }
    for g in 40..=45 {
        assert!(
            !face.has_bitmap(GlyphId::from_shaper(g)),
            "glyph {g} is only in the refused size"
        );
    }
    assert!(matches!(
        face.strike(Index::ZERO),
        Some(StrikeState::Read(_))
    ));
    assert_eq!(
        face.strike(Index::of(1u32)),
        Some(StrikeState::Refused(StrikeRefusal::Unsorted {
            at: Index::of(1u32)
        }))
    );
}

#[test]
fn a_size_whose_records_run_past_the_table_is_refused() {
    let face_data = noto_with(|c| put32(c, 16, 100));
    let face = Face::parse(&face_data).unwrap();
    for g in 0..50 {
        assert!(
            png(&face, 0, g).is_none(),
            "100 records past a 288-byte table answer glyph {g}"
        );
    }
    assert_eq!(
        face.strike(Index::ZERO),
        Some(StrikeState::Refused(StrikeRefusal::PastTable))
    );
    let data = past_table_size(&read(NOTO));
    let face = Face::parse(&data).unwrap();
    for g in 0..50 {
        assert!(png(&face, 1, g).is_none(), "2^20 records answer glyph {g}");
    }
    assert_eq!(
        face.strike(Index::of(1u32)),
        Some(StrikeState::Refused(StrikeRefusal::PastTable))
    );
    for g in 4..=45 {
        assert_eq!(png(&face, 0, g), Some(own(g)), "size 0, glyph {g}");
    }
}

#[test]
fn a_glyph_past_the_face_has_no_bitmap() {
    let data = with_glyphs(&read(NOTO), 40);
    let face = Face::parse(&data).unwrap();
    for g in 40..=45 {
        assert!(png(&face, 0, g).is_none(), "glyph {g} is past numGlyphs 40");
    }
    for g in 4..=39 {
        assert_eq!(png(&face, 0, g), Some(own(g)), "glyph {g}");
    }
    let cases = read(CASES);
    let face = Face::parse(&cases).unwrap();
    assert!(
        png(&face, 4, 17).is_none(),
        "glyph 17 is past the fixture's 16 glyphs"
    );
    for g in 13..=15 {
        assert!(
            png(&face, 4, g).is_some(),
            "the control: size 4 draws glyph {g}"
        );
    }
}

/// The overlap rule is a "should not", so these sizes are read, and each
/// glyph answers from the first record that holds it, exactly as before.
/// The four surgeries reach the lookup's three modes: a staircase (each
/// record also ends later), a nested record, equal firsts, and an inverted
/// record, which holds no glyph at all.
#[test]
fn an_overlapping_size_answers_as_today() {
    let cases: [(&str, Vec<u8>, &[u16]); 4] = [
        (
            "record 0's last 15 -> 20",
            noto_with(|c| put16(c, 58, 20)),
            &[16, 17, 18, 19, 20],
        ),
        (
            "record 1 = [10..12]",
            noto_with(|c| {
                put16(c, 64, 10);
                put16(c, 66, 12);
            }),
            &[16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27],
        ),
        (
            "record 1 = [4..15]",
            noto_with(|c| {
                put16(c, 64, 4);
                put16(c, 66, 15);
            }),
            &[16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27],
        ),
        (
            "record 2 = [33..28]",
            noto_with(|c| {
                put16(c, 72, 33);
                put16(c, 74, 28);
            }),
            &[28, 29, 30, 31, 32, 33],
        ),
    ];
    for (name, data, nothing) in cases {
        let face = Face::parse(&data).unwrap();
        for g in 0..50u16 {
            let want = if (4..=45).contains(&g) && !nothing.contains(&g) {
                Some(own(g))
            } else {
                None
            };
            assert_eq!(
                png(&face, 0, g).map(|p| p.len()),
                want.as_ref().map(|p| p.len()),
                "{name}: glyph {g}"
            );
            assert!(
                png(&face, 0, g) == want,
                "{name}: glyph {g} is not its own image"
            );
        }
    }
}

#[test]
fn a_format_4_glyph_is_found_among_its_real_pairs() {
    let data = noto_with(|c| put32(c, 200, 5));
    let face = Face::parse(&data).unwrap();
    assert!(
        png(&face, 0, 33).is_none(),
        "glyph 33 is named only by the extra pair"
    );
    for g in 28..=32 {
        assert_eq!(png(&face, 0, g), Some(own(g)), "the control: glyph {g}");
    }
    let data = noto_with(|c| {
        let (p1, p2) = (c[208..212].to_vec(), c[212..216].to_vec());
        c[208..212].copy_from_slice(&p2);
        c[212..216].copy_from_slice(&p1);
    });
    let face = Face::parse(&data).unwrap();
    assert!(
        png(&face, 0, 29).is_none(),
        "glyph 29's pair is out of glyph order"
    );
    assert_eq!(
        png(&face, 0, 30),
        Some(own(30)),
        "glyph 30 is where a sorted list puts it"
    );
    // A count of 21: the 21 pairs from CBLC+204 end exactly at the 288-byte
    // table's end, so the 22 the format stores run past it.
    let data = noto_with(|c| put32(c, 200, 21));
    let face = Face::parse(&data).unwrap();
    for g in 28..=33 {
        assert!(
            png(&face, 0, g).is_none(),
            "glyph {g}: 22 pairs run past the table"
        );
    }
    assert_eq!(
        png(&face, 0, 27),
        Some(own(27)),
        "the control: glyph 27, in record 1"
    );
}

/// The model's own positive control: it refuses what the specification
/// refuses, at the record it names, and reads the rest -- or the sweep below
/// would hold baud to a model that never refuses anything.
#[test]
fn the_model_refuses_what_the_specification_refuses() {
    let refusal = |data: &[u8], i: u32| {
        let (cl, cll) = table(data, b"CBLC");
        let (cd, cdl) = table(data, b"CBDT");
        spec(&data[cl..cl + cll], &data[cd..cd + cdl], 46, i, 4).err()
    };
    let noto = read(NOTO);
    assert_eq!(refusal(&noto, 0), None, "the clean subset reads");
    let two = two_sizes(&noto);
    assert_eq!(refusal(&two, 0), None);
    match refusal(&two, 1) {
        Some(Refusal::Unsorted(at)) => {
            assert_eq!(at, Index::of(1u32), "record 1 starts before record 0")
        }
        other => panic!("the swapped size is refused as unsorted, not {other:?}"),
    }
    assert_eq!(
        refusal(&noto_with(|c| put32(c, 16, 100)), 0),
        Some(Refusal::PastTable)
    );
    assert_eq!(
        refusal(&past_table_size(&noto), 1),
        Some(Refusal::PastTable)
    );
}

#[test]
fn every_cblc_glyph_is_the_specifications_record_walk() {
    for (name, data) in surgeries() {
        sweep(name, &data);
    }
}

#[test]
fn the_cblc_fixture_is_what_its_generator_writes() {
    // As for `sbix-strikes.ttf`: the generator's table is the fixture's
    // documentation, so the committed bytes must be what it writes.
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::env::temp_dir().join(format!("baud-cblc-{}.ttf", std::process::id()));
    let ran = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-cblc-fixture.py"))
        .arg(&out)
        .status()
        .expect("python3 to run the generator");
    assert!(ran.success(), "gen-cblc-fixture.py failed");
    let fresh = std::fs::read(&out).unwrap();
    std::fs::remove_file(&out).unwrap();
    assert!(
        fresh == read(CASES),
        "cblc-cases.ttf is not what scripts/gen-cblc-fixture.py writes: regenerate it"
    );
}

// ------------------------------------------------------------------ fonts written here

/// How an index subtable locates its images: EBLC's index formats 1 to 5.
#[derive(Clone, Copy, Debug)]
enum IndexFormat {
    Offsets32,
    Constant,
    Offsets16,
    Sparse,
    SparseConstant,
}

impl IndexFormat {
    const ALL: [IndexFormat; 5] = [
        IndexFormat::Offsets32,
        IndexFormat::Constant,
        IndexFormat::Offsets16,
        IndexFormat::Sparse,
        IndexFormat::SparseConstant,
    ];

    fn number(self) -> u16 {
        match self {
            IndexFormat::Offsets32 => 1,
            IndexFormat::Constant => 2,
            IndexFormat::Offsets16 => 3,
            IndexFormat::Sparse => 4,
            IndexFormat::SparseConstant => 5,
        }
    }

    /// Formats 4 and 5 list the glyphs they carry; 1 to 3 carry their range.
    fn lists(self) -> Lists {
        match self {
            IndexFormat::Sparse | IndexFormat::SparseConstant => Lists::Glyphs,
            _ => Lists::Range,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lists {
    Range,
    Glyphs,
}

/// How an image is laid out: CBDT's three PNG formats.
#[derive(Clone, Copy, Debug)]
enum ImageFormat {
    Small,
    Big,
    Shared,
}

impl ImageFormat {
    const ALL: [ImageFormat; 3] = [ImageFormat::Small, ImageFormat::Big, ImageFormat::Shared];

    fn number(self) -> u16 {
        match self {
            ImageFormat::Small => 17,
            ImageFormat::Big => 18,
            ImageFormat::Shared => 19,
        }
    }

    /// An image's bytes: its metrics, the length and the 8-byte payload.
    fn size(self) -> u32 {
        match self {
            ImageFormat::Small => 17,
            ImageFormat::Big => 20,
            ImageFormat::Shared => 12,
        }
    }
}

/// Whether a record's first image states its own length or one that runs
/// past the table.
#[derive(Clone, Copy, Debug)]
enum Length {
    Own,
    PastTable,
}

/// An index subtable record the test writes: its glyph range, its subtable's
/// formats, the glyphs a format 4 or 5 lists (all of the range unless given),
/// the glyph format 4's extra pair names (an image is written for it, so a
/// reader that counts the pair as a glyph would draw it), and its first
/// image's length.
#[derive(Clone, Debug)]
struct Rec {
    first: GlyphId,
    last: GlyphId,
    index: IndexFormat,
    image: ImageFormat,
    ids: Option<Vec<GlyphId>>,
    sentinel: Option<GlyphId>,
    length: Length,
}

fn gid(g: u16) -> GlyphId {
    GlyphId::from_shaper(g)
}

fn id(g: GlyphId) -> u16 {
    u16::try_from(g.index()).unwrap()
}

fn rec(first: u16, last: u16) -> Rec {
    Rec {
        first: gid(first),
        last: gid(last),
        index: IndexFormat::Offsets32,
        image: ImageFormat::Small,
        ids: None,
        sentinel: None,
        length: Length::Own,
    }
}

/// A size the test writes: its start..end range, its records, and the record
/// count it claims when that is not how many it lists.
#[derive(Clone, Debug)]
struct Size {
    start: GlyphId,
    end: GlyphId,
    recs: Vec<Rec>,
    count: Option<baud_font::bytes::Count>,
}

fn size(start: u16, end: u16, recs: Vec<Rec>) -> Size {
    Size {
        start: gid(start),
        end: gid(end),
        recs,
        count: None,
    }
}

/// A minimal sfnt -- `head`, `hhea`, `hmtx`, `maxp`, `CBLC` and `CBDT`, no
/// checksums -- with `glyphs` glyphs and these sizes at 10, 20, 30 ... ppem.
///
/// Each image's payload names its size, record and glyph, so an answer from
/// the wrong record is a different image.
fn font(glyphs: u16, sizes: &[Size]) -> Vec<u8> {
    let mut cbdt = 0x0003_0000u32.to_be_bytes().to_vec();
    let mut lists = Vec::new();
    let header = 8 + 48 * sizes.len();
    let mut records = Vec::new();
    for (si, s) in sizes.iter().enumerate() {
        let list_at = header + lists.len();
        let mut list = vec![0u8; 8 * s.recs.len()];
        for (ri, r) in s.recs.iter().enumerate() {
            while !list.len().is_multiple_of(4) {
                list.push(0);
            }
            let sub_at = list.len();
            put16(&mut list, 8 * ri, id(r.first));
            put16(&mut list, 8 * ri + 2, id(r.last));
            put32(&mut list, 8 * ri + 4, u32::try_from(sub_at).unwrap());
            let range: Vec<u16> = (id(r.first)..=id(r.last)).collect();
            let ids: Vec<u16> = r
                .ids
                .as_ref()
                .map_or_else(|| range.clone(), |v| v.iter().map(|&g| id(g)).collect());
            let (bx, by) = (
                i8::try_from(ri % 7).unwrap(),
                i8::try_from(ri % 5).unwrap() - 2,
            );
            let big = [4u8, 4, bx.to_ne_bytes()[0], by.to_ne_bytes()[0], 4, 0, 0, 4];
            let image = |cbdt: &mut Vec<u8>, g: u16, first: bool| {
                let [hi, lo] = g.to_be_bytes();
                let payload = [
                    b'S',
                    u8::try_from(si).unwrap(),
                    b'R',
                    u8::try_from(ri).unwrap(),
                    b'G',
                    hi,
                    lo,
                    b'.',
                ];
                let len: u32 = match (first, r.length) {
                    (true, Length::PastTable) => 0xFFFF_0000,
                    _ => 8,
                };
                match r.image {
                    ImageFormat::Small => cbdt.extend_from_slice(&[4, 4, big[2], big[3], 4]),
                    ImageFormat::Big => cbdt.extend_from_slice(&big),
                    ImageFormat::Shared => {}
                }
                cbdt.extend_from_slice(&len.to_be_bytes());
                cbdt.extend_from_slice(&payload);
            };
            let base = u32::try_from(cbdt.len()).unwrap();
            let carried: &[u16] = if r.index.lists() == Lists::Glyphs {
                &ids
            } else {
                &range
            };
            let mut offs = Vec::new();
            for (k, &g) in carried.iter().enumerate() {
                offs.push(u32::try_from(cbdt.len()).unwrap() - base);
                image(&mut cbdt, g, k == 0);
            }
            let end = u32::try_from(cbdt.len()).unwrap() - base;
            if let Some(s) = r.sentinel {
                image(&mut cbdt, id(s), false);
            }
            let image_size = r.image.size();
            list.extend_from_slice(&r.index.number().to_be_bytes());
            list.extend_from_slice(&r.image.number().to_be_bytes());
            list.extend_from_slice(&base.to_be_bytes());
            match r.index {
                IndexFormat::Offsets32 => offs
                    .iter()
                    .chain([&end])
                    .for_each(|o| list.extend_from_slice(&o.to_be_bytes())),
                IndexFormat::Offsets16 => offs.iter().chain([&end]).for_each(|o| {
                    list.extend_from_slice(&u16::try_from(*o).unwrap().to_be_bytes())
                }),
                IndexFormat::Constant => {
                    list.extend_from_slice(&image_size.to_be_bytes());
                    list.extend_from_slice(&big);
                }
                IndexFormat::Sparse => {
                    list.extend_from_slice(&u32::try_from(ids.len()).unwrap().to_be_bytes());
                    for (k, &g) in ids.iter().enumerate() {
                        list.extend_from_slice(&g.to_be_bytes());
                        list.extend_from_slice(&u16::try_from(offs[k]).unwrap().to_be_bytes());
                    }
                    list.extend_from_slice(&r.sentinel.map_or(0xFFFF, id).to_be_bytes());
                    list.extend_from_slice(&u16::try_from(end).unwrap().to_be_bytes());
                }
                IndexFormat::SparseConstant => {
                    list.extend_from_slice(&image_size.to_be_bytes());
                    list.extend_from_slice(&big);
                    list.extend_from_slice(&u32::try_from(ids.len()).unwrap().to_be_bytes());
                    ids.iter()
                        .for_each(|g| list.extend_from_slice(&g.to_be_bytes()));
                }
            }
        }
        while !list.len().is_multiple_of(4) {
            list.push(0);
        }
        let mut record = vec![0u8; 48];
        put32(&mut record, 0, u32::try_from(list_at).unwrap());
        put32(&mut record, 4, u32::try_from(list.len()).unwrap());
        put32(
            &mut record,
            8,
            s.count
                .map_or(u32::try_from(s.recs.len()).unwrap(), |c| c.value()),
        );
        put16(&mut record, 40, id(s.start));
        put16(&mut record, 42, id(s.end));
        let ppem = u8::try_from(10 * (si + 1)).unwrap();
        record[44] = ppem;
        record[45] = ppem;
        record[46] = 32;
        record[47] = 1;
        records.extend_from_slice(&record);
        lists.extend_from_slice(&list);
    }
    let mut cblc = 0x0003_0000u32.to_be_bytes().to_vec();
    cblc.extend_from_slice(&u32::try_from(sizes.len()).unwrap().to_be_bytes());
    cblc.extend_from_slice(&records);
    cblc.extend_from_slice(&lists);
    let metrics = glyphs.max(1);
    let mut head = vec![0u8; 54];
    put32(&mut head, 0, 0x0001_0000);
    put32(&mut head, 12, 0x5F0F_3CF5);
    put16(&mut head, 18, 800);
    let mut hhea = vec![0u8; 36];
    put32(&mut hhea, 0, 0x0001_0000);
    put16(&mut hhea, 4, 800);
    put16(&mut hhea, 6, 0xFF38);
    put16(&mut hhea, 34, metrics);
    let hmtx: Vec<u8> = (0..metrics).flat_map(|_| [3u8, 32, 0, 0]).collect();
    let mut maxp = 0x0000_5000u32.to_be_bytes().to_vec();
    maxp.extend_from_slice(&glyphs.to_be_bytes());
    let tables: [(&[u8; 4], Vec<u8>); 6] = [
        (b"CBDT", cbdt),
        (b"CBLC", cblc),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"maxp", maxp),
    ];
    let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
    out.extend_from_slice(&6u16.to_be_bytes());
    out.extend_from_slice(&[0, 64, 0, 2, 0, 32]);
    let mut at = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(at).unwrap().to_be_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        body.extend_from_slice(data);
        while !body.len().is_multiple_of(4) {
            body.push(0);
        }
        at = 12 + 16 * tables.len() + body.len();
    }
    out.extend_from_slice(&body);
    out
}

// ------------------------------------------------------------------ the named shapes

fn disjoint() -> Size {
    size(0, 30, vec![rec(1, 3), rec(4, 6), rec(7, 9)])
}
fn unsorted_at(j: usize) -> Size {
    let mut s = size(0, 30, vec![rec(1, 2), rec(3, 4), rec(5, 6), rec(7, 8)]);
    s.recs.swap(j - 1, j);
    s
}
fn past_table() -> Size {
    Size {
        count: Some(baud_font::bytes::Count::of(100_000u32)),
        ..disjoint()
    }
}
fn staircase() -> Size {
    size(0, 30, vec![rec(1, 5), rec(3, 8), rec(6, 10)])
}
fn nested() -> Size {
    size(0, 30, vec![rec(1, 9), rec(3, 5), rec(7, 10)])
}
fn equal_firsts() -> Size {
    size(0, 30, vec![rec(1, 5), rec(1, 8)])
}
fn inverted() -> Size {
    size(0, 30, vec![rec(1, 4), rec(6, 2), rec(7, 9)])
}
fn gaps() -> Size {
    size(0, 30, vec![rec(1, 2), rec(5, 6), rec(9, 10)])
}
fn outside_start_end() -> Size {
    size(3, 6, vec![rec(0, 10)])
}
fn sentinel_inside() -> Size {
    size(
        0,
        30,
        vec![Rec {
            index: IndexFormat::Sparse,
            ids: Some(vec![gid(1), gid(2), gid(4)]),
            sentinel: Some(gid(5)),
            ..rec(1, 6)
        }],
    )
}
fn past_glyphs(glyphs: u16) -> Size {
    size(
        0,
        glyphs + 4,
        vec![rec(glyphs.saturating_sub(2), glyphs + 3)],
    )
}
fn truncated() -> Size {
    size(
        0,
        30,
        vec![
            Rec {
                length: Length::PastTable,
                ..rec(1, 3)
            },
            rec(4, 6),
        ],
    )
}

/// H1's positive control: each named shape, built once, takes the arm it is
/// named for, and answers or refuses the glyph it is there for -- without it,
/// 20,000 random fonts could pass while never reaching a refusal.
#[test]
fn every_named_cblc_shape_takes_its_arm() {
    let state = |s: Size, glyphs: u16| {
        let data = font(glyphs, &[s]);
        let face = Face::parse(&data).unwrap();
        (face.strike(Index::ZERO), data)
    };
    let answers = |data: &[u8], g: u16| {
        Face::parse(data)
            .unwrap()
            .bitmap(Index::ZERO, GlyphId::from_shaper(g))
            .is_some()
    };
    let read = |st: Option<StrikeState>| match st {
        Some(StrikeState::Read(s)) => s.records(),
        other => panic!("{other:?} is not read"),
    };
    let (st, d) = state(disjoint(), 16);
    assert_eq!(read(st), Records::Disjoint);
    assert!(answers(&d, 5), "the control: a disjoint size answers");
    for j in 1..4 {
        let (st, _) = state(unsorted_at(j), 16);
        assert_eq!(
            st,
            Some(StrikeState::Refused(StrikeRefusal::Unsorted {
                at: Index::of(u32::try_from(j).unwrap())
            }))
        );
    }
    assert_eq!(
        state(past_table(), 16).0,
        Some(StrikeState::Refused(StrikeRefusal::PastTable))
    );
    let (st, d) = state(staircase(), 16);
    assert_eq!(read(st), Records::Staircase);
    assert!(answers(&d, 7));
    let (st, d) = state(nested(), 16);
    assert_eq!(read(st), Records::Nested);
    assert!(answers(&d, 10));
    assert_eq!(read(state(equal_firsts(), 16).0), Records::Staircase);
    let (st, d) = state(inverted(), 16);
    assert_eq!(read(st), Records::Disjoint);
    assert!(
        !answers(&d, 6) && answers(&d, 8),
        "the inverted record holds nothing, its own first glyph included"
    );
    let (_, d) = state(gaps(), 16);
    assert!(
        !answers(&d, 3) && answers(&d, 5),
        "a gap glyph answers nothing"
    );
    let (_, d) = state(outside_start_end(), 16);
    assert!(
        !answers(&d, 2) && answers(&d, 4),
        "a glyph outside start..end answers nothing"
    );
    let (_, d) = state(sentinel_inside(), 16);
    assert!(
        !answers(&d, 5) && answers(&d, 4),
        "the extra pair's glyph answers nothing"
    );
    let (_, d) = state(past_glyphs(16), 16);
    assert!(
        !answers(&d, 16) && answers(&d, 15),
        "a glyph at numGlyphs answers nothing"
    );
    let (_, d) = state(truncated(), 16);
    assert!(
        !answers(&d, 1) && answers(&d, 2),
        "a truncated image answers nothing"
    );
    let (_, d) = state(disjoint(), 0);
    assert!(
        (0..12).all(|g| !answers(&d, g)),
        "a face of no glyphs answers nothing"
    );
}

fn formats() -> impl Strategy<Value = (IndexFormat, ImageFormat)> {
    (
        prop::sample::select(IndexFormat::ALL.to_vec()),
        prop::sample::select(ImageFormat::ALL.to_vec()),
    )
}

/// One size: a named shape, or free-form records, with random formats on its
/// records (format 4 kept where the shape is about format 4).
fn shape(glyphs: u16) -> impl Strategy<Value = Size> {
    let named = prop_oneof![
        Just(disjoint()),
        (1usize..4).prop_map(unsorted_at),
        Just(past_table()),
        Just(staircase()),
        Just(nested()),
        Just(equal_firsts()),
        Just(inverted()),
        Just(gaps()),
        Just(outside_start_end()),
        Just(sentinel_inside()),
        Just(past_glyphs(glyphs)),
        Just(truncated()),
        prop::collection::vec((0..=glyphs + 4, 0..=glyphs + 4), 0..=8).prop_map(|rs| size(
            0,
            30,
            rs.into_iter().map(|(f, l)| rec(f, l)).collect()
        )),
    ];
    (named, prop::collection::vec(formats(), 8)).prop_map(|(mut s, fs)| {
        for (r, (index, image)) in s.recs.iter_mut().zip(fs) {
            if r.sentinel.is_none() {
                r.index = index;
            }
            r.image = image;
        }
        s
    })
}

proptest! {
    /// Fonts written here, of 0 to 24 glyphs and one to three sizes, each size
    /// a named shape or free-form: every size's state and every glyph's answer
    /// -- through `bitmap` and through `bitmap_in` -- is the model's. Run it
    /// once at `PROPTEST_CASES=20000` after a change to the lookup.
    #[test]
    fn cblc_lookups_are_the_specification_on_good_sizes_and_nothing_on_refused_ones(
        (glyphs, sizes) in (0u16..=24).prop_flat_map(|g| (Just(g), prop::collection::vec(shape(g), 1..=3)))
    ) {
        sweep("a written font", &font(glyphs, &sizes));
    }
}
