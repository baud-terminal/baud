//! `CFF2` outlines, against HarfBuzz.
//!
//! `tests/var/cff2-outlines.txt` is HarfBuzz 12.3.0's drawing of every
//! seventh glyph of Source Code VF and every glyph of the cases font
//! (`tools/baud-oracle cases cff2`), at the default instance and at the
//! settings `tools/baud-oracle`'s `cff2` names, with every coordinate as
//! the exact `f32` HarfBuzz handed its draw functions. baud computes a
//! CFF2 charstring in `f64`, as HarfBuzz does, and rounds to `f32` only at
//! the sink, so the comparison is exact: a tolerance would hide a wrong
//! blend behind a right default.
//!
//! **Two normalisations, each counted and pinned.** HarfBuzz's draw session
//! (`[[source: harfbuzz-12-3-0-draw]]`) defers a moveto until something is
//! drawn, so a contour of a moveto alone is not emitted, where baud emits
//! `M p Z`; and it closes a contour whose current point is not its start
//! with a `line_to` to the start, which is the specification's "implicit
//! lineto operation", where baud's sink receives the close alone. Both are
//! applied to baud's commands, so HarfBuzz's are compared as written.

use baud_font::var::{Fixed, Variation};
use baud_font::{Drawn, Error, Face, Tag};

const CFF2: Tag = Tag::from_bytes(*b"CFF2");

#[derive(Default)]
struct Commands(Vec<(char, Vec<f32>)>);

impl baud_font::Sink for Commands {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(('M', vec![x, y]));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(('L', vec![x, y]));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.push(('Q', vec![x1, y1, x, y]));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push(('C', vec![x1, y1, x2, y2, x, y]));
    }
    fn close(&mut self) {
        self.0.push(('Z', vec![]));
    }
}

/// What HarfBuzz's draw session makes of baud's commands: a contour that
/// is a moveto alone is dropped, and one whose last point is not its start
/// gains a line to the start before its close. Each is counted.
fn as_harfbuzz_draws(
    cmds: &[(char, Vec<f32>)],
    lone_moves: &mut usize,
    closing_lines: &mut usize,
) -> Vec<(char, Vec<f32>)> {
    let mut out = Vec::new();
    let mut contour: Vec<(char, Vec<f32>)> = Vec::new();
    for c in cmds {
        if c.0 != 'Z' {
            contour.push(c.clone());
            continue;
        }
        if contour.len() == 1 {
            *lone_moves += 1;
            contour.clear();
            continue;
        }
        let start = contour[0].1.clone();
        let last = contour.last().unwrap().1.clone();
        let end = last[last.len() - 2..].to_vec();
        out.append(&mut contour);
        if end
            .iter()
            .zip(&start)
            .any(|(a, b)| a.to_bits() != b.to_bits())
        {
            out.push(('L', start));
            *closing_lines += 1;
        }
        out.push(('Z', vec![]));
    }
    assert!(
        contour.is_empty(),
        "a contour baud did not close: {contour:?}"
    );
    out
}

fn parse_commands(words: &[&str]) -> Vec<(char, Vec<f32>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let c = words[i].chars().next().unwrap();
        let n = match c {
            'M' | 'L' => 2,
            'Q' => 4,
            'C' => 6,
            _ => 0,
        };
        let v = words[i + 1..=i + n]
            .iter()
            .map(|w| {
                // The script prints the C float's exact value: an `f64`
                // that converts to the `f32` it came from without rounding.
                let d: f64 = w.parse().unwrap();
                let f = d as f32;
                assert_eq!(f64::from(f), d, "{w} is not an f32");
                f
            })
            .collect();
        out.push((c, v));
        i += n + 1;
    }
    out
}

fn settings(w: &str) -> Vec<Variation> {
    w.split(',')
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap();
            let v: i32 = v.parse().unwrap();
            Variation {
                tag: Tag::from_bytes(k.as_bytes().try_into().unwrap()),
                value: Fixed(v << 16),
            }
        })
        .collect()
}

fn same(a: &[(char, Vec<f32>)], b: &[(char, Vec<f32>)]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|((x, xv), (y, yv))| {
            x == y
                && xv.len() == yv.len()
                && xv.iter().zip(yv).all(|(p, q)| p.to_bits() == q.to_bits())
        })
}

#[test]
fn every_sampled_cff2_outline_is_harfbuzzs() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/cff2-outlines.txt"
    ))
    .unwrap();
    let (mut checked, mut varied, mut drawn, mut moved, mut cubics) = (0, 0, 0, 0, 0);
    let (mut lone_moves, mut closing_lines) = (0, 0);
    let mut wrong = Vec::new();
    let mut current: Option<(String, Vec<u8>)> = None;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let words: Vec<&str> = line.split(' ').filter(|w| !w.is_empty()).collect();
        let (name, w, gid) = (words[0], words[1], words[2]);
        if current.as_ref().is_none_or(|(n, _)| n != name) {
            let path = format!("{}/tests/{name}", env!("CARGO_MANIFEST_DIR"));
            current = Some((name.to_string(), std::fs::read(path).unwrap()));
        }
        let data = &current.as_ref().unwrap().1;
        let mut face = Face::parse(data).unwrap();
        if w != "-" {
            face.set_variations(&settings(w));
            assert!(!face.coords().is_default(), "{w} moved no axis");
            varied += 1;
        }
        let g = baud_domain::GlyphId::from_shaper(gid.parse().unwrap());
        let mut got = Commands::default();
        let d = face.outline(g, &mut got);
        assert!(!matches!(d, Drawn::NeedsScratch(_)), "{line:.60}");
        if w != "-" {
            let mut default = Commands::default();
            Face::parse(data).unwrap().outline(g, &mut default);
            moved += usize::from(!same(&default.0, &got.0));
        }
        let want = parse_commands(&words[3..]);
        drawn += usize::from(!want.is_empty());
        cubics += want.iter().filter(|c| c.0 == 'C').count();
        checked += 1;
        let got = as_harfbuzz_draws(&got.0, &mut lone_moves, &mut closing_lines);
        if !same(&got, &want) {
            let first = got
                .iter()
                .zip(&want)
                .position(|(a, b)| !same(std::slice::from_ref(a), std::slice::from_ref(b)))
                .unwrap_or(got.len().min(want.len()));
            wrong.push(format!(
                "{name} {w} {gid}: {} commands against {}, first difference at {first}: {:?} against {:?}",
                got.len(),
                want.len(),
                got.get(first),
                want.get(first)
            ));
        }
    }
    eprintln!(
        "checked {checked} varied {varied} drawn {drawn} moved {moved} cubics {cubics} lone moves {} closing lines {}",
        lone_moves, closing_lines
    );
    // The control: every line was read, most draw, and the varied rows
    // reach the blend -- a reader that ignored `blend` would draw every
    // varied row as its default and fail here before it failed below.
    assert_eq!(checked, 1360, "lines read");
    assert!(varied > 1100 && drawn > 1250, "{varied} {drawn}");
    assert!(
        moved > 950,
        "varied rows that differ from the default: {moved}"
    );
    assert!(cubics > 8_000, "cubics compared: {cubics}");
    assert!(
        wrong.is_empty(),
        "{} of {checked} differ, first: {:#?}",
        wrong.len(),
        &wrong[..wrong.len().min(6)]
    );
    // The cases font's `moves` glyph, at its default and 23 settings.
    assert_eq!(lone_moves, 24, "lone moves dropped");
    assert!(closing_lines > 0, "the closing-line rule never fired");
}

fn fixture() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fonts/SourceCodeVF-Upright.otf"
    ))
    .unwrap()
}

/// The directory record for `tag`: the table's offset and length.
fn table(data: &[u8], tag: &[u8; 4]) -> (usize, usize) {
    let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
    (0..n)
        .map(|i| 12 + 16 * i)
        .find(|&r| &data[r..r + 4] == tag)
        .map(|r| {
            let at = u32::from_be_bytes(data[r + 8..r + 12].try_into().unwrap());
            let len = u32::from_be_bytes(data[r + 12..r + 16].try_into().unwrap());
            (usize::try_from(at).unwrap(), usize::try_from(len).unwrap())
        })
        .unwrap_or_else(|| panic!("no {tag:?}"))
}

/// A face whose only outlines are a `CFF2` table that does not read is
/// refused at load, naming the table, rather than parsed into a face that
/// draws every glyph blank. The control: the same bytes unbroken parse and
/// draw.
#[test]
fn a_face_whose_cff2_does_not_read_is_refused() {
    let data = fixture();
    let face = Face::parse(&data).expect("the unbroken face");
    let mut c = Commands::default();
    let a = face
        .glyph(baud_domain::Codepoint::new(u32::from('a')).unwrap())
        .unwrap();
    assert_eq!(face.outline(a, &mut c), Drawn::Outline, "the control draws");

    let (cff2, _) = table(&data, b"CFF2");
    // A major version CFF2 is not.
    let mut broken = data.clone();
    broken[cff2] = 3;
    assert_eq!(
        Face::parse(&broken).err(),
        Some(Error::Malformed(CFF2)),
        "major version 3"
    );
    // A CharStringINDEX whose count is not maxp's: maxp says one fewer.
    let (maxp, _) = table(&data, b"maxp");
    let mut broken = data.clone();
    let n = u16::from_be_bytes([broken[maxp + 4], broken[maxp + 5]]) - 1;
    broken[maxp + 4..maxp + 6].copy_from_slice(&n.to_be_bytes());
    assert_eq!(
        Face::parse(&broken).err(),
        Some(Error::Malformed(CFF2)),
        "numGlyphs one short"
    );
    // A GlobalSubrINDEX whose offset size is 0, which no INDEX has.
    let mut broken = data.clone();
    let top = usize::from(u16::from_be_bytes([broken[cff2 + 3], broken[cff2 + 4]]));
    let gsubrs = cff2 + usize::from(broken[cff2 + 2]) + top;
    broken[gsubrs..gsubrs + 4].copy_from_slice(&1u32.to_be_bytes());
    broken[gsubrs + 4] = 0;
    assert_eq!(
        Face::parse(&broken).err(),
        Some(Error::Malformed(CFF2)),
        "offSize 0"
    );
}

/// Read at a variation, a CFF2 glyph draws its blended outline through
/// [`Face::outline`] with no scratch, and that outline is not the default.
#[test]
fn a_varied_cff2_glyph_draws_without_scratch() {
    let data = fixture();
    let mut face = Face::parse(&data).unwrap();
    let a = face
        .glyph(baud_domain::Codepoint::new(u32::from('a')).unwrap())
        .unwrap();
    let mut default = Commands::default();
    face.outline(a, &mut default);
    face.set_variations(&settings("wght=900"));
    assert_eq!(face.outline_need(a).value(), 0);
    let mut heavy = Commands::default();
    assert_eq!(face.outline(a, &mut heavy), Drawn::Outline);
    let mut at = Commands::default();
    assert_eq!(face.outline_at(a, &mut [], &mut at), Drawn::Outline);
    assert!(same(&heavy.0, &at.0), "outline_at is outline");
    assert!(!same(&default.0, &heavy.0), "wght 900 is not the default");
}
