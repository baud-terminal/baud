//! `compare` holds every committed font equal to itself, and names each
//! of seven perturbed copies of the built Regular: a point, an anchor, a
//! lookup flag, a cmap entry, a gvar delta, a head field and a name
//! record. Each copy is the committed bytes with one field edited in
//! place, found through read-fonts rather than at a fixed offset, and
//! with the table checksums and `checkSumAdjustment` recomputed, so the
//! copy is a well-formed font that differs in that field alone.

use std::path::{Path, PathBuf};
use std::process::Command;

use baud_fontbuild::{compare, iup_contour, reach};
use read_fonts::tables::cmap::CmapSubtable;
use read_fonts::tables::glyf::Glyph;
use read_fonts::tables::gpos::{AnchorTable, MarkBasePos, PositionSubtables};
use read_fonts::tables::gsub::SubstitutionLookup;
use read_fonts::tables::layout::FeatureParams;
use read_fonts::types::{GlyphId, NameId, Tag};
use read_fonts::{FontRef, Offset, TableProvider};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn regular() -> Vec<u8> {
    std::fs::read(root().join("baud-app/fonts/built/Regular.ttf")).unwrap()
}

fn fonts_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            fonts_under(&p, out);
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("ttf" | "otf")) {
            out.push(p);
        }
    }
}

/// Where a slice read through read-fonts sits in the font's bytes.
fn at(font: &[u8], part: &[u8]) -> usize {
    let off = part.as_ptr() as usize - font.as_ptr() as usize;
    assert!(
        off + part.len() <= font.len(),
        "the slice is not inside the font"
    );
    off
}

fn table_start(font: &[u8], tag: &[u8; 4]) -> usize {
    let f = FontRef::new(font).unwrap();
    at(font, f.table_data(Tag::new(tag)).unwrap().as_bytes())
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn sum(b: &[u8]) -> u32 {
    b.chunks(4).fold(0u32, |s, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        s.wrapping_add(u32::from_be_bytes(w))
    })
}

/// The OpenType checksums after an edit: each table record's checksum
/// over its table (head's with `checkSumAdjustment` zeroed), then
/// `checkSumAdjustment` = 0xB1B0AFBA minus the whole font's sum.
fn fix_checksums(b: &mut [u8]) {
    let n = be16(b, 4) as usize;
    let mut head = None;
    for i in 0..n {
        let rec = 12 + 16 * i;
        let (off, len) = (be32(b, rec + 8) as usize, be32(b, rec + 12) as usize);
        if &b[rec..rec + 4] == b"head" {
            b[off + 8..off + 12].fill(0);
            head = Some(off);
        }
        let s = sum(&b[off..off + len]);
        b[rec + 4..rec + 8].copy_from_slice(&s.to_be_bytes());
    }
    let adj = 0xB1B0_AFBAu32.wrapping_sub(sum(b));
    let head = head.expect("a head table");
    b[head + 8..head + 12].copy_from_slice(&adj.to_be_bytes());
}

fn add16(b: &mut [u8], at: usize, by: i16) {
    let v = (be16(b, at) as i16).wrapping_add(by);
    b[at..at + 2].copy_from_slice(&v.to_be_bytes());
}

fn label(font: &[u8], gid: u32) -> String {
    let f = FontRef::new(font).unwrap();
    let name = f
        .post()
        .unwrap()
        .glyph_name(read_fonts::types::GlyphId16::new(gid as u16))
        .unwrap();
    format!("glyph '{name}' (gid {gid})")
}

/// The report on a copy, which must name exactly one table.
fn report(base: &[u8], copy: &mut [u8]) -> String {
    fix_checksums(copy);
    let r = compare(base, copy).unwrap();
    assert!(!r.is_equal(), "the perturbation went unseen");
    assert_eq!(
        r.differences.len(),
        1,
        "more than one table: {:?}",
        r.differences
    );
    let text = r.differences.join("\n");
    eprintln!("{text}");
    text
}

#[test]
fn every_committed_font_equals_itself() {
    let mut fonts = vec![];
    fonts_under(&root().join("baud-app/fonts"), &mut fonts);
    fonts_under(&root().join("baud-font/tests"), &mut fonts);
    fonts.sort();
    // Positive control: the walk reached the built fonts and the cases.
    assert!(
        fonts
            .iter()
            .any(|p| p.ends_with("baud-app/fonts/built/Regular.ttf"))
    );
    assert!(
        fonts
            .iter()
            .any(|p| p.ends_with("baud-font/tests/var/hvar/bad-map.ttf"))
    );
    assert!(fonts.len() >= 57, "{} fonts", fonts.len());
    for p in &fonts {
        let b = std::fs::read(p).unwrap();
        let r = compare(&b, &b).unwrap();
        assert!(r.is_equal(), "{}: {:?}", p.display(), r.differences);
    }
}

#[test]
fn the_built_fonts_are_expanded_with_nothing_left_as_bytes() {
    for style in ["Regular", "Bold", "Italic", "BoldItalic"] {
        let b = std::fs::read(root().join(format!("baud-app/fonts/built/{style}.ttf"))).unwrap();
        let baud_fontbuild::Reach { tables, bytes_only } = reach(&b).unwrap();
        assert!(bytes_only.is_empty(), "{style}: {bytes_only:?}");
        let names: Vec<&str> = tables.iter().map(|(t, _)| t.as_str()).collect();
        for t in [
            "glyph order",
            "cmap",
            "glyf",
            "gvar",
            "hmtx",
            "HVAR",
            "head",
            "hhea",
            "maxp",
            "OS/2",
            "post",
            "name",
            "gasp",
            "cvt ",
            "fpgm",
            "prep",
            "fvar",
            "avar",
            "STAT",
            "GDEF",
            "GSUB",
            "GPOS",
        ] {
            assert!(names.contains(&t), "{style}: {t} not expanded: {names:?}");
        }
        assert!(tables.iter().all(|(_, n)| *n > 0), "{style}: {tables:?}");
    }
}

/// The routine the edits rest on: it leaves every built font's bytes as
/// they are, so their checksums, written by fontTools, are the ones it
/// computes.
#[test]
fn the_checksums_are_recomputed_as_the_built_fonts_carry_them() {
    for style in ["Regular", "Bold", "Italic", "BoldItalic"] {
        let base = std::fs::read(root().join(format!("baud-app/fonts/built/{style}.ttf"))).unwrap();
        let mut copy = base.clone();
        fix_checksums(&mut copy);
        assert!(copy == base, "{style}: the recomputed checksums differ");
    }
}

/// A stylistic set's UI name is a feature parameter: a copy that points
/// ss01 at another name is a different font.
#[test]
fn a_changed_stylistic_set_name_is_named() {
    let base = regular();
    let f = FontRef::new(&base).unwrap();
    let list = f.gsub().unwrap().feature_list().unwrap();
    let (name_at, id) = list
        .feature_records()
        .iter()
        .find_map(|r| {
            let feature = r.feature(list.offset_data()).ok()?;
            let Some(Ok(FeatureParams::StylisticSet(p))) = feature.feature_params() else {
                return None;
            };
            Some((
                at(&base, p.offset_data().as_bytes()) + p.ui_name_id_byte_range().start,
                p.ui_name_id().to_u16(),
            ))
        })
        .expect("a stylistic set with a UI name");
    let mut copy = base.clone();
    add16(&mut copy, name_at, 1);
    let r = report(&base, &mut copy);
    assert!(
        r.starts_with("GSUB: script ") && r.contains(&format!("uiName={id})")),
        "{r}"
    );
    assert!(r.contains(&format!("uiName={})", id + 1)), "{r}");
}

#[test]
fn a_changed_checksum_adjustment_alone_is_equal() {
    let base = regular();
    let mut copy = base.clone();
    let head = table_start(&copy, b"head");
    copy[head + 8] ^= 0x5a;
    assert_ne!(base, copy);
    let r = compare(&base, &copy).unwrap();
    assert!(r.is_equal(), "{:?}", r.differences);
}

#[test]
fn a_point_moved_by_one_is_named() {
    let base = regular();
    let f = FontRef::new(&base).unwrap();
    let (loca, glyf) = (f.loca(None).unwrap(), f.glyf().unwrap());
    // The first simple glyph whose last point's x is stored as a word:
    // adding 1 to that word moves that point alone, by 1.
    let (gid, last, x_at) = (0..f.maxp().unwrap().num_glyphs() as u32)
        .find_map(|g| {
            let Ok(Some(Glyph::Simple(s))) = loca.get_glyf(GlyphId::new(g), &glyf) else {
                return None;
            };
            let n = s.num_points();
            let data = s.glyph_data();
            let (mut flags, mut i) = (Vec::with_capacity(n), 0);
            while flags.len() < n {
                let fl = data[i];
                i += 1;
                flags.push(fl);
                if fl & 0x08 != 0 {
                    for _ in 0..data[i] {
                        flags.push(fl);
                    }
                    i += 1;
                }
            }
            let mut x_at = None;
            for &fl in &flags[..n] {
                match (fl & 0x02 != 0, fl & 0x10 != 0) {
                    (true, _) => {
                        x_at = None;
                        i += 1;
                    }
                    (false, true) => x_at = None,
                    (false, false) => {
                        x_at = Some(i);
                        i += 2;
                    }
                }
            }
            x_at.map(|x| (g, n - 1, at(&base, data) + x))
        })
        .expect("a glyph whose last x is a word");
    let mut copy = base.clone();
    add16(&mut copy, x_at, 1);
    let r = report(&base, &mut copy);
    assert!(
        r.contains(&format!("glyf: {} points: `p{last}=", label(&base, gid))),
        "{r}"
    );
}

#[test]
fn a_moved_anchor_is_named() {
    let base = regular();
    let f = FontRef::new(&base).unwrap();
    let lookups = f.gpos().unwrap().lookup_list().unwrap().lookups();
    let (index, x_at, x) = lookups
        .iter()
        .enumerate()
        .find_map(|(i, l)| {
            let PositionSubtables::MarkToBase(subs) = l.ok()?.subtables().ok()? else {
                return None;
            };
            let MarkBasePos::Format1(s) = subs.iter().next()?.ok()? else {
                return None;
            };
            let marks = s.mark_array().ok()?;
            let rec = marks.mark_records().first()?;
            let a = rec.mark_anchor(marks.offset_data()).ok()?;
            let (data, range, x) = match &a {
                AnchorTable::Format1(a) => (
                    a.offset_data(),
                    a.x_coordinate_byte_range(),
                    a.x_coordinate(),
                ),
                AnchorTable::Format2(a) => (
                    a.offset_data(),
                    a.x_coordinate_byte_range(),
                    a.x_coordinate(),
                ),
                AnchorTable::Format3(a) => (
                    a.offset_data(),
                    a.x_coordinate_byte_range(),
                    a.x_coordinate(),
                ),
            };
            Some((i, at(&base, data.as_bytes()) + range.start, x))
        })
        .expect("a mark-to-base lookup");
    let mut copy = base.clone();
    add16(&mut copy, x_at, 1);
    let r = report(&base, &mut copy);
    assert!(r.contains(&format!("lookup {index} attachments: ")), "{r}");
    assert!(
        r.contains(&format!("=({x},")) && r.contains(&format!("=({},", x + 1)),
        "{r}"
    );
    assert!(r.starts_with("GPOS: ") && !r.contains('\n'), "{r}");
}

#[test]
fn a_flipped_lookup_flag_is_named() {
    let base = regular();
    let f = FontRef::new(&base).unwrap();
    let gsub = f.gsub().unwrap();
    let list = gsub.lookup_list().unwrap();
    let lookup = list.lookups().get(0).unwrap();
    let flag = lookup.lookup_flag().to_bits();
    // A lookup table opens with lookupType, then lookupFlag. IgnoreMarks
    // is flipped: it changes no table's layout, as UseMarkFilteringSet
    // would.
    let data = match &lookup {
        SubstitutionLookup::Single(l) => l.offset_data(),
        SubstitutionLookup::Multiple(l) => l.offset_data(),
        SubstitutionLookup::Alternate(l) => l.offset_data(),
        SubstitutionLookup::Ligature(l) => l.offset_data(),
        SubstitutionLookup::Contextual(l) => l.offset_data(),
        SubstitutionLookup::ChainContextual(l) => l.offset_data(),
        SubstitutionLookup::Extension(l) => l.offset_data(),
        SubstitutionLookup::Reverse(l) => l.offset_data(),
    };
    let flag_at = at(&base, data.as_bytes()) + 2;
    assert_eq!(be16(&base, flag_at), flag);
    let mut copy = base.clone();
    copy[flag_at + 1] ^= 0x08;
    let r = report(&base, &mut copy);
    assert!(
        r.contains(&format!(
            "GSUB: lookup 0: `flag={flag:#06x}` vs `flag={:#06x}`",
            flag ^ 0x08
        )),
        "{r}"
    );
}

#[test]
fn a_dropped_cmap_entry_is_named() {
    let base = regular();
    let f = FontRef::new(&base).unwrap();
    let cmap = f.cmap().unwrap();
    // In the first format 4 subtable, a segment of more than one
    // character mapped by idDelta: lowering its endCode by one unmaps its
    // last character and nothing else.
    let (end_at, cp) = cmap
        .encoding_records()
        .iter()
        .find_map(|r| {
            let CmapSubtable::Format4(s) = r.subtable(cmap.offset_data()).ok()? else {
                return None;
            };
            let (ends, starts, deltas, ranges) = (
                s.end_code(),
                s.start_code(),
                s.id_delta(),
                s.id_range_offsets(),
            );
            let i = (0..ends.len() - 1).find(|&i| {
                let (e, st) = (ends[i].get(), starts[i].get());
                e > st
                    && ranges[i].get() == 0
                    && (e as i32 + deltas[i].get() as i32).rem_euclid(65536) != 0
            })?;
            Some((
                at(&base, s.offset_data().as_bytes()) + s.end_code_byte_range().start + 2 * i,
                ends[i].get(),
            ))
        })
        .expect("a format 4 segment");
    let mut copy = base.clone();
    add16(&mut copy, end_at, -1);
    let r = report(&base, &mut copy);
    assert!(
        r.contains("cmap: record ") && r.contains(&format!("map: `U+{cp:04X}>")),
        "{r}"
    );
}

/// A gvar tuple that names only some points: its glyph, the points it
/// names, and where each named point's x delta is stored when it is
/// stored as a byte.
struct Sparse {
    gid: u32,
    points: Vec<usize>,
    x_byte: Vec<Option<usize>>,
}

/// Every tuple of the font with explicit point numbers, the private or
/// shared list that leaves the other points to inference, read from the
/// gvar bytes as the common formats chapter lays them out.
fn sparse_tuples(font: &[u8]) -> Vec<Sparse> {
    let g = table_start(font, b"gvar");
    let axes = be16(font, g + 4) as usize;
    let count = be16(font, g + 12) as u32;
    let long = be16(font, g + 14) & 1 != 0;
    let array = g + be32(font, g + 16) as usize;
    let offset = |i: u32| -> usize {
        if long {
            be32(font, g + 20 + 4 * i as usize) as usize
        } else {
            2 * be16(font, g + 20 + 2 * i as usize) as usize
        }
    };
    // Packed point numbers: a count (0 means all points), then runs of
    // byte or word increments. `None` is "all points".
    let points = |mut p: usize| -> (Option<Vec<usize>>, usize) {
        let c = font[p] as usize;
        p += 1;
        if c == 0 {
            return (None, p);
        }
        let mut n = c;
        if c & 0x80 != 0 {
            n = ((c & 0x7f) << 8) | font[p] as usize;
            p += 1;
        }
        let (mut out, mut last) = (vec![], 0usize);
        while out.len() < n {
            let ctl = font[p];
            p += 1;
            for _ in 0..(ctl & 0x7f) as usize + 1 {
                let step = if ctl & 0x80 != 0 {
                    p += 2;
                    be16(font, p - 2) as usize
                } else {
                    p += 1;
                    font[p - 1] as usize
                };
                last += step;
                out.push(last);
            }
        }
        (Some(out), p)
    };
    let mut found = vec![];
    for gid in 0..count {
        let (start, end) = (offset(gid), offset(gid + 1));
        if start == end {
            continue;
        }
        let v = array + start;
        let tuples = be16(font, v);
        let (shared, n) = (tuples & 0x8000 != 0, (tuples & 0x0fff) as usize);
        let mut header = v + 4;
        let mut data = v + be16(font, v + 2) as usize;
        let mut shared_points = None;
        if shared {
            let (pts, after) = points(data);
            shared_points = pts;
            data = after;
        }
        for _ in 0..n {
            let size = be16(font, header) as usize;
            let index = be16(font, header + 2);
            header += 4
                + if index & 0x8000 != 0 { 2 * axes } else { 0 }
                + if index & 0x4000 != 0 { 4 * axes } else { 0 };
            let (pts, mut d) = if index & 0x2000 != 0 {
                points(data)
            } else {
                (shared_points.clone(), data)
            };
            if let Some(pts) = pts {
                // Packed deltas: runs of zeros (no bytes), bytes or words.
                let mut x_byte = vec![];
                while x_byte.len() < pts.len() {
                    let ctl = font[d];
                    d += 1;
                    for _ in 0..(ctl & 0x3f) as usize + 1 {
                        match ctl & 0xc0 {
                            0x00 => {
                                x_byte.push(Some(d));
                                d += 1;
                            }
                            0x40 => {
                                x_byte.push(None);
                                d += 2;
                            }
                            0x80 => x_byte.push(None),
                            _ => panic!("32-bit deltas are not in these fonts"),
                        }
                    }
                }
                x_byte.truncate(pts.len());
                found.push(Sparse {
                    gid,
                    points: pts,
                    x_byte,
                });
            }
            data += size;
        }
    }
    found
}

/// A named point's x delta is raised by 1 where the point before it is
/// unnamed and lies strictly between it and the previous named point in
/// x. That point's inferred delta then moves too, and it comes first, so
/// the report names a point no tuple names: the inference is what found
/// it. Without the inference the report would name the edited point.
#[test]
fn a_changed_delta_is_named_at_the_point_it_is_inferred_for() {
    let base = regular();
    let f = FontRef::new(&base).unwrap();
    let (loca, glyf) = (f.loca(None).unwrap(), f.glyf().unwrap());
    let mut pick = None;
    'search: for t in sparse_tuples(&base) {
        let Ok(Some(Glyph::Simple(s))) = loca.get_glyf(GlyphId::new(t.gid), &glyf) else {
            continue;
        };
        let xs: Vec<i32> = s.points().map(|p| p.x as i32).collect();
        let mut contour_start = 0;
        for end in s.end_pts_of_contours() {
            let end = end.get() as usize;
            for j in 1..t.points.len() {
                let (prev, r) = (t.points[j - 1], t.points[j]);
                let target = r.wrapping_sub(1);
                let in_contour = |p: usize| p >= contour_start && p <= end;
                if !(in_contour(prev) && in_contour(r) && r > contour_start && target > prev) {
                    continue;
                }
                let (lo, hi) = (xs[prev].min(xs[r]), xs[prev].max(xs[r]));
                let Some(byte) = t.x_byte[j] else { continue };
                if lo < xs[target] && xs[target] < hi && (base[byte] as i8) < 127 {
                    pick = Some((t.gid, byte, t.points.clone(), r));
                    break 'search;
                }
            }
            contour_start = end + 1;
        }
    }
    let (gid, byte, named, edited) =
        pick.expect("a named point after an unnamed one between references");
    let mut copy = base.clone();
    copy[byte] = copy[byte].wrapping_add(1);
    let r = report(&base, &mut copy);
    let want = format!("gvar: {} tuple ", label(&base, gid));
    assert!(r.starts_with(&want) && r.contains(" deltas: `p"), "{r}");
    let p: usize = r
        .split(" deltas: `p")
        .nth(1)
        .unwrap()
        .split('=')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        p < edited && !named.contains(&p),
        "{r}: p{p} is named, or not before p{edited}"
    );
}

#[test]
fn a_changed_head_field_is_named() {
    let base = regular();
    let f = FontRef::new(&base).unwrap();
    let head = f.head().unwrap();
    let rev_at = at(&base, head.offset_data().as_bytes()) + head.font_revision_byte_range().start;
    let mut copy = base.clone();
    copy[rev_at + 3] ^= 0x01;
    let r = report(&base, &mut copy);
    assert!(r.starts_with("head: fontRevision: `"), "{r}");
}

#[test]
fn a_changed_name_record_is_named() {
    let base = regular();
    let f = FontRef::new(&base).unwrap();
    let name = f.name().unwrap();
    let rec = name
        .name_record()
        .iter()
        .find(|r| r.platform_id() == 3 && r.name_id() == NameId::FAMILY_NAME)
        .expect("a Windows family name");
    let string = rec.string(name.string_data()).unwrap();
    let first: String = string.chars().collect();
    let char_at = at(&base, name.string_data().as_bytes()) + rec.string_offset().to_usize() + 1;
    let mut copy = base.clone();
    copy[char_at] = copy[char_at].wrapping_add(1);
    let r = report(&base, &mut copy);
    let locus = format!(
        "name: platform=3 encoding={} language={:#06x} nameID=1: `{}",
        rec.encoding_id(),
        rec.language_id(),
        first.split(' ').next().unwrap()
    );
    assert!(r.starts_with(&locus), "{r}\nwant {locus}");
}

/// The OpenType gvar chapter's own example ("Inferred deltas for
/// un-referenced point numbers"): P1 and P3 referenced, P2 between them
/// in x and outside them in y, nearer P3. Its x delta is
/// (1 - 0.25) * 28 + 0.25 * -42 = 10.5 and its y delta is P3's, -57.
#[test]
fn inference_matches_the_specifications_example() {
    let coords = [(245.0, 100.0), (260.0, 200.0), (305.0, 150.0)];
    let out = iup_contour(&coords, &[Some((28.0, -62.0)), None, Some((-42.0, -57.0))]);
    assert_eq!(out, vec![(28.0, -62.0), (10.5, -57.0), (-42.0, -57.0)]);
}

#[test]
fn inference_covers_each_case_the_chapter_names() {
    // Equal reference coordinates: their common delta, or zero.
    let same = iup_contour(
        &[(10.0, 0.0), (20.0, 5.0), (10.0, 0.0)],
        &[Some((3.0, 4.0)), None, Some((3.0, 6.0))],
    );
    assert_eq!(same[1], (3.0, 0.0));
    // A target outside the references takes the nearer one's delta.
    let out = iup_contour(
        &[(0.0, 0.0), (-5.0, 50.0), (10.0, 10.0)],
        &[Some((1.0, 1.0)), None, Some((2.0, 2.0))],
    );
    assert_eq!(out[1], (1.0, 2.0));
    // One referenced point: the contour moves with it.
    let one = iup_contour(
        &[(0.0, 0.0), (5.0, 5.0), (9.0, 1.0)],
        &[None, Some((7.0, -3.0)), None],
    );
    assert_eq!(one, vec![(7.0, -3.0); 3]);
    // None referenced: unaffected.
    let none = iup_contour(&[(0.0, 0.0), (5.0, 5.0)], &[None, None]);
    assert_eq!(none, vec![(0.0, 0.0); 2]);
    // The neighbours wrap within the contour: point 0's are 2 and 1.
    let wrap = iup_contour(
        &[(0.0, 0.0), (10.0, 0.0), (-10.0, 0.0)],
        &[None, Some((10.0, 0.0)), Some((20.0, 0.0))],
    );
    assert_eq!(wrap[0], (15.0, 0.0));
}

#[test]
fn the_binary_exits_0_equal_1_different_2_unreadable() {
    let bin = env!("CARGO_BIN_EXE_baud-fontbuild");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("compare-exit");
    std::fs::create_dir_all(&dir).unwrap();
    let base = regular();
    let mut copy = base.clone();
    let head = table_start(&copy, b"head");
    copy[head + 47] ^= 0x01; // lowestRecPPEM's low byte
    fix_checksums(&mut copy);
    let (a, b, junk) = (dir.join("a.ttf"), dir.join("b.ttf"), dir.join("junk.ttf"));
    std::fs::write(&a, &base).unwrap();
    std::fs::write(&b, &copy).unwrap();
    std::fs::write(&junk, b"not a font").unwrap();
    let run = |x: &Path, y: &Path| {
        Command::new(bin)
            .arg("compare")
            .arg(x)
            .arg(y)
            .output()
            .unwrap()
    };
    let same = run(&a, &a);
    assert_eq!(same.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&same.stdout), "equal\n");
    let differ = run(&a, &b);
    assert_eq!(differ.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&differ.stdout).starts_with("head: lowestRecPPEM: `"));
    assert_eq!(run(&a, &junk).status.code(), Some(2));
}
