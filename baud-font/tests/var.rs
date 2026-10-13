//! Variation coordinates, against HarfBuzz.
//!
//! `tests/var/normalize.txt` is HarfBuzz's normalised coordinate for every
//! wght design value from 0 to 1000 in quarter units, over the three
//! committed variable faces; `scripts/var-oracle.py` writes it. The
//! specification fixes the arithmetic except the rounding of one 16.16
//! division, so agreeing with HarfBuzz at every quarter unit -- both clamps,
//! the default, every `avar` segment and its edges -- is agreeing with the
//! specification wherever it is precise.

use baud_font::bytes::Index;
use baud_font::var::{F2Dot14, Fixed, Variation};
use baud_font::{Face, Tag};

fn font(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fonts/{name}", env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

const WGHT: Tag = Tag::from_bytes(*b"wght");

#[test]
fn every_coordinate_is_harfbuzzs() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/normalize.txt"
    ))
    .unwrap();
    let mut checked = 0usize;
    let mut wrong = Vec::new();
    let mut current: Option<(String, Vec<u8>)> = None;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.split(' ');
        let (name, q, want) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        if current.as_ref().is_none_or(|(n, _)| n != name) {
            current = Some((name.to_string(), font(name)));
        }
        let data = &current.as_ref().unwrap().1;
        let mut face = Face::parse(data).unwrap();
        let quarters: i32 = q.parse().unwrap();
        face.set_variations(&[Variation {
            tag: WGHT,
            value: Fixed(quarters * 0x4000),
        }]);
        let want = F2Dot14(want.parse().unwrap());
        let got = face.coords().get(Index::ZERO);
        if got != want {
            wrong.push(format!(
                "{name} {}: got {} want {}",
                f64::from(quarters) / 4.0,
                got.0,
                want.0
            ));
        }
        checked += 1;
    }
    assert_eq!(checked, 3 * 4001, "the control: every line was read");
    assert!(
        wrong.is_empty(),
        "{} of {checked} differ, first: {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(8)]
    );
}

/// The axis each face offers, as `fvar` states it.
#[test]
fn the_axes_are_fvars() {
    for (name, min, def, max) in [
        ("JetBrainsMono-Variable.ttf", 100, 400, 800),
        ("JetBrainsMono-Italic-Variable.ttf", 100, 400, 800),
        ("FiraCode-VF.ttf", 300, 300, 700),
    ] {
        let data = font(name);
        let face = Face::parse(&data).unwrap();
        let a = face.axis(Index::ZERO).expect("an axis");
        assert_eq!(a.tag, WGHT, "{name}");
        assert_eq!(
            (a.min, a.default, a.max),
            (Fixed(min << 16), Fixed(def << 16), Fixed(max << 16)),
            "{name}"
        );
        assert!(face.axis(Index::of(1u32)).is_none(), "{name}: one axis");
        assert!(
            face.coords().is_default(),
            "{name}: a fresh face is at its default"
        );
    }
}

/// A static face has no axes and ignores a setting; a tag the font has no
/// axis for is ignored; the last setting of an axis wins.
#[test]
fn settings_that_name_no_axis_change_nothing() {
    let data = font("JetBrainsMono-Regular.ttf");
    let mut face = Face::parse(&data).unwrap();
    assert!(face.axis(Index::ZERO).is_none());
    face.set_variations(&[Variation {
        tag: WGHT,
        value: Fixed(700 << 16),
    }]);
    assert!(face.coords().is_default());
    let data = font("JetBrainsMono-Variable.ttf");
    let mut face = Face::parse(&data).unwrap();
    face.set_variations(&[Variation {
        tag: Tag::from_bytes(*b"wdth"),
        value: Fixed(50 << 16),
    }]);
    assert!(face.coords().is_default(), "no wdth axis");
    face.set_variations(&[
        Variation {
            tag: WGHT,
            value: Fixed(100 << 16),
        },
        Variation {
            tag: WGHT,
            value: Fixed(800 << 16),
        },
    ]);
    assert_eq!(
        face.coords().get(Index::ZERO),
        F2Dot14(16384),
        "the last wins"
    );
}

/// Records what an outline draws, one command a string, as the oracle
/// script writes HarfBuzz's.
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
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.0.push(('C', vec![]));
    }
    fn close(&mut self) {
        self.0.push(('Z', vec![]));
    }
}

/// HarfBuzz draws a contour of one point as `M p L p Z`; baud's default
/// path draws nothing for it, which is ttf-parser's rule and the one
/// `tests/oracle.rs` holds bit for bit. A contour whose every coordinate is
/// its first point's draws nothing either way, so it is dropped here -- and
/// counted, so the normalisation cannot fire anywhere it was not meant to.
fn drop_degenerate(cmds: Vec<(char, Vec<f32>)>, dropped: &mut usize) -> Vec<(char, Vec<f32>)> {
    let mut out = Vec::new();
    let mut contour: Vec<(char, Vec<f32>)> = Vec::new();
    for c in cmds {
        let end = c.0 == 'Z';
        contour.push(c);
        if end {
            let first = contour[0].1.clone();
            let flat = contour
                .iter()
                .all(|(_, v)| v.chunks(2).all(|p| p == first.as_slice()));
            if flat {
                *dropped += 1;
            } else {
                out.append(&mut contour);
            }
            contour.clear();
        }
    }
    out.extend(contour);
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
            _ => 0,
        };
        let v = words[i + 1..=i + n]
            .iter()
            .map(|w| w.parse().unwrap())
            .collect();
        out.push((c, v));
        i += n + 1;
    }
    out
}

/// How far a coordinate may be from HarfBuzz's: the script prints four
/// places (5e-5 of rounding) and a coordinate near 1,000 units carries an
/// `f32` step of 6e-5; the two sum the same deltas in the same order. The
/// worst measured over the whole fixture is 1.2e-4.
const TOLERANCE: f32 = 0.0005;

/// Every sampled glyph of the three variable faces, drawn at the default
/// and at the weights `scripts/gvar-oracle.py` names, against HarfBuzz's
/// drawing. The default rows are the control that the two emit contours by
/// one rule; the varied rows are the deltas, the inference, the component
/// offsets and the flattening.
#[test]
fn every_sampled_outline_is_harfbuzzs() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/outlines.txt"
    ))
    .unwrap();
    let (mut checked, mut varied, mut drawn, mut moved) = (0usize, 0usize, 0usize, 0usize);
    let mut worst = 0f32;
    let mut dropped = 0usize;
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
            let settings: Vec<Variation> = w
                .split(',')
                .map(|kv| {
                    let (k, v) = kv.split_once('=').unwrap();
                    let v: i32 = v.parse().unwrap();
                    Variation {
                        tag: Tag::from_bytes(k.as_bytes().try_into().unwrap()),
                        value: Fixed(v << 16),
                    }
                })
                .collect();
            face.set_variations(&settings);
            varied += 1;
        }
        let g = baud_domain::GlyphId::from_shaper(gid.parse().unwrap());
        // Exactly the need, so an underestimate cannot hide behind a
        // buffer an earlier glyph grew.
        let need = usize::try_from(face.outline_need(g).value()).unwrap();
        let mut scratch = vec![baud_font::VarPoint::EMPTY; need];
        let mut got = Commands::default();
        let d = face.outline_at(g, &mut scratch, &mut got);
        if w != "-" {
            let plain = Face::parse(data).unwrap();
            let mut default = Commands::default();
            plain.outline(g, &mut default);
            moved += usize::from(default.0 != got.0);
        }
        for ((_, av), (_, bv)) in got
            .0
            .iter()
            .zip(&drop_degenerate(parse_commands(&words[3..]), &mut 0))
        {
            for (x, y) in av.iter().zip(bv) {
                worst = worst.max((x - y).abs());
            }
        }
        assert!(
            !matches!(d, baud_font::Drawn::NeedsScratch(_)),
            "{line:.60}"
        );
        let want = drop_degenerate(parse_commands(&words[3..]), &mut dropped);
        drawn += usize::from(!want.is_empty());
        checked += 1;
        let same = got.0.len() == want.len()
            && got.0.iter().zip(&want).all(|((a, av), (b, bv))| {
                a == b && av.iter().zip(bv).all(|(x, y)| (x - y).abs() <= TOLERANCE)
            });
        if !same {
            let first = got
                .0
                .iter()
                .zip(&want)
                .position(|((a, av), (b, bv))| {
                    a != b || av.iter().zip(bv).any(|(x, y)| (x - y).abs() > TOLERANCE)
                })
                .unwrap_or(got.0.len().min(want.len()));
            wrong.push(format!(
                "{name} {w} {gid}: {} commands against {}, first difference at {first}: {:?} against {:?}",
                got.0.len(),
                want.len(),
                got.0.get(first),
                want.get(first)
            ));
        }
    }
    eprintln!("checked {checked} varied {varied} drawn {drawn} moved {moved} worst {worst}");
    assert!(
        checked > 900 && varied > 700 && drawn > 800,
        "the control: {checked} {varied} {drawn}"
    );
    // The cases font's one-point glyph, at its default and 44 settings.
    assert_eq!(dropped, 45, "degenerate contours dropped");
    assert!(
        wrong.is_empty(),
        "{} of {checked} differ, first: {:#?}",
        wrong.len(),
        &wrong[..wrong.len().min(6)]
    );
}

/// A face read at a variation never draws the default instance in the
/// varied glyph's place: `outline` answers what it needs, `outline_at`
/// refuses one point short of it and draws with it. At the default
/// instance neither needs scratch.
#[test]
fn a_varied_glyph_is_never_drawn_as_the_default() {
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/gvar-cases.ttf"
    ))
    .unwrap();
    let mut face = Face::parse(&data).unwrap();
    let nest = baud_domain::GlyphId::from_shaper(5);
    assert_eq!(
        face.outline_need(nest).value(),
        0,
        "the default instance needs none"
    );
    assert_eq!(
        face.outline(nest, &mut Commands::default()),
        baud_font::Drawn::Outline
    );
    face.set_variations(&[Variation {
        tag: WGHT,
        value: Fixed(700 << 16),
    }]);
    let need = face.outline_need(nest);
    // nest: comp's 7 + 4 flattened points, two's 7, sq's 5, and the stack
    // of offsets and phantoms two composites deep.
    assert!(need.value() > 20, "{need:?}");
    let mut drawn = Commands::default();
    assert_eq!(
        face.outline(nest, &mut drawn),
        baud_font::Drawn::NeedsScratch(need)
    );
    assert!(drawn.0.is_empty(), "nothing drawn in its place");
    let n = usize::try_from(need.value()).unwrap();
    let mut short = vec![baud_font::VarPoint::EMPTY; n - 1];
    assert_eq!(
        face.outline_at(nest, &mut short, &mut drawn),
        baud_font::Drawn::NeedsScratch(need)
    );
    assert!(drawn.0.is_empty());
    let mut enough = vec![baud_font::VarPoint::EMPTY; n];
    assert_eq!(
        face.outline_at(nest, &mut enough, &mut drawn),
        baud_font::Drawn::Outline
    );
}

/// HarfBuzz's graph-edge bound, on both paths: `edge2049` visits 2,049
/// glyphs and draws its square, `edge2050` visits one more and draws
/// nothing -- at the default instance and at a variation. The bound is
/// checked before a visit is counted, as HarfBuzz checks it; counting
/// first would refuse `edge2049` too.
#[test]
fn a_walk_past_harfbuzzs_edge_bound_draws_nothing_on_either_path() {
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/gvar-cases.ttf"
    ))
    .unwrap();
    let (at, past) = (
        baud_domain::GlyphId::from_shaper(8),
        baud_domain::GlyphId::from_shaper(9),
    );
    let mut face = Face::parse(&data).unwrap();
    let mut drawn = Commands::default();
    assert_eq!(face.outline(at, &mut drawn), baud_font::Drawn::Outline);
    assert_eq!(drawn.0.len(), 6, "the square: M L Q L L Z");
    let mut none = Commands::default();
    assert_eq!(face.outline(past, &mut none), baud_font::Drawn::Nothing);
    assert!(none.0.is_empty(), "nothing drawn before the refusal");

    face.set_variations(&[Variation {
        tag: WGHT,
        value: Fixed(700 << 16),
    }]);
    let need = face.outline_need(at);
    assert!(need.value() > 0);
    let mut scratch = vec![baud_font::VarPoint::EMPTY; usize::try_from(need.value()).unwrap()];
    let mut varied = Commands::default();
    assert_eq!(
        face.outline_at(at, &mut scratch, &mut varied),
        baud_font::Drawn::Outline
    );
    assert_eq!(varied.0.len(), 6);
    assert_eq!(face.outline_need(past).value(), 0);
    let mut none = Commands::default();
    assert_eq!(
        face.outline_at(past, &mut scratch, &mut none),
        baud_font::Drawn::Nothing
    );
    assert!(none.0.is_empty());
}

/// Every advance `scripts/advance-oracle.py` recorded -- every glyph of the
/// cases fonts, every 11th of the real ones -- against HarfBuzz's, exactly:
/// both round to whole units. The default rows are the control that the
/// oracle's scale is units per em: each must be the `hmtx` advance.
#[test]
fn every_advance_is_harfbuzzs() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/advances.txt"
    ))
    .unwrap();
    let (mut checked, mut varied, mut moved) = (0usize, 0usize, 0usize);
    let mut wrong = Vec::new();
    let mut current: Option<(String, Vec<u8>)> = None;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let words: Vec<&str> = line.split(' ').collect();
        let (name, w, gid, want) = (words[0], words[1], words[2], words[3]);
        if current.as_ref().is_none_or(|(n, _)| n != name) {
            let path = format!("{}/tests/{name}", env!("CARGO_MANIFEST_DIR"));
            current = Some((name.to_string(), std::fs::read(path).unwrap()));
        }
        let data = &current.as_ref().unwrap().1;
        let mut face = Face::parse(data).unwrap();
        let g = baud_domain::GlyphId::from_shaper(gid.parse().unwrap());
        let want: u16 = want.parse().unwrap();
        let default = face.default_advance(g).map(|a| a.value());
        if w == "-" {
            assert_eq!(default, Some(want), "the control: {line}");
        } else {
            let settings: Vec<Variation> = w
                .split(',')
                .map(|kv| {
                    let (k, v) = kv.split_once('=').unwrap();
                    Variation {
                        tag: Tag::from_bytes(k.as_bytes().try_into().unwrap()),
                        value: Fixed(v.parse::<i32>().unwrap() << 16),
                    }
                })
                .collect();
            face.set_variations(&settings);
            varied += 1;
            moved += usize::from(default != Some(want));
        }
        let got = face.advance(g).map(|a| a.value());
        if got != Some(want) {
            wrong.push(format!("{line}: got {got:?}"));
        }
        checked += 1;
    }
    eprintln!("checked {checked} varied {varied} moved {moved}");
    assert!(
        checked > 9000 && varied > 8000 && moved > 2000,
        "the control: {checked} {varied} {moved}"
    );
    assert!(
        wrong.is_empty(),
        "{} of {checked} differ, first: {:#?}",
        wrong.len(),
        &wrong[..wrong.len().min(200)]
    );
}

/// Every text `scripts/marks-oracle.py` shaped -- the Latin letters under
/// sixteen combining marks, and stacks of two -- against HarfBuzz's glyphs,
/// clusters and mark offsets, at the default and at each weight, exactly.
/// HarfBuzz reports a mark's offset from its own pen position, one base
/// advance along; baud's is from the cell origin, so the pen is added back,
/// as `tests/shape.rs` does. The default rows are the control; the varied
/// rows are the anchors' variation devices, and for `var/ink-cases.ttf`,
/// which has no GPOS, the fallback placement against varied glyph boxes.
#[test]
fn every_mark_position_is_harfbuzzs() {
    use baud_font::{Cluster, Features, Glyph, Ignorable, Shaper};
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/var/marks.txt"))
        .unwrap();
    let (mut checked, mut varied, mut offset, mut asked) = (0usize, 0usize, 0usize, 0usize);
    let mut default_rows = std::collections::HashMap::new();
    let mut moved = 0usize;
    let mut wrong = Vec::new();
    let mut current: Option<(String, Vec<u8>)> = None;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let words: Vec<&str> = line.split(' ').collect();
        let (name, w, cps, want) = (words[0], words[1], words[2], words[3]);
        if current.as_ref().is_none_or(|(n, _)| n != name) {
            let path = format!("{}/tests/{name}", env!("CARGO_MANIFEST_DIR"));
            current = Some((name.to_string(), std::fs::read(path).unwrap()));
        }
        let mut face = Face::parse(&current.as_ref().unwrap().1).unwrap();
        if w == "-" {
            default_rows.insert((name.to_string(), cps.to_string()), want.to_string());
        } else {
            let settings: Vec<Variation> = w
                .split(',')
                .map(|kv| {
                    let (k, v) = kv.split_once('=').unwrap();
                    Variation {
                        tag: Tag::from_bytes(k.as_bytes().try_into().unwrap()),
                        value: Fixed(v.parse::<i32>().unwrap() << 16),
                    }
                })
                .collect();
            face.set_variations(&settings);
            varied += 1;
            moved += usize::from(default_rows[&(name.to_string(), cps.to_string())] != want);
        }
        let cps: Vec<baud_domain::Codepoint> = cps
            .split(',')
            .map(|h| baud_domain::Codepoint::new(u32::from_str_radix(h, 16).unwrap()).unwrap())
            .collect();
        let mut storage = [Glyph::new(baud_domain::GlyphId::NOTDEF, Cluster(0), Ignorable::No); 16];
        // The renderer's protocol: shape with what scratch there is, and on
        // a refused box grow it to the need the refusal names and shape
        // again. A face with GPOS reads no box and never asks.
        let mut scratch: Vec<baud_font::VarPoint> = Vec::new();
        let mut shaper = Shaper::new(&face, &mut storage);
        shaper.push_cluster(&cps, Cluster(0)).unwrap();
        let (_, first) = shaper.finish(Features::GRID, baud_font::Script::of_cluster(&cps));
        if !first.ink_refused.is_zero() {
            asked += 1;
            scratch.resize(
                usize::try_from(first.ink_need.value()).unwrap(),
                baud_font::VarPoint::EMPTY,
            );
        }
        let mut shaper = Shaper::new(&face, &mut storage);
        shaper.scratch(&mut scratch);
        shaper.push_cluster(&cps, Cluster(0)).unwrap();
        let (buf, shaped) = shaper.finish(Features::GRID, baud_font::Script::of_cluster(&cps));
        assert!(shaped.declined.is_zero(), "{line}");
        assert!(shaped.ink_refused.is_zero(), "{line}: a box refused");
        let mut got = Vec::new();
        let mut pen = 0i32;
        let expected: Vec<[i32; 5]> = want
            .split(';')
            .map(|g| {
                let v: Vec<i32> = g.split(',').map(|x| x.parse().unwrap()).collect();
                [v[0], v[1], v[2], v[3], v[4]]
            })
            .collect();
        for (g, e) in buf.as_slice().iter().zip(&expected) {
            got.push([
                i32::try_from(g.id.index()).unwrap(),
                i32::from(g.cluster.0),
                i32::from(g.dx) - pen,
                i32::from(g.dy),
            ]);
            pen += e[4];
            offset += usize::from(e[2] != 0 || e[3] != 0);
        }
        let want: Vec<[i32; 4]> = expected.iter().map(|e| [e[0], e[1], e[2], e[3]]).collect();
        if got != want {
            wrong.push(format!("{line}: got {got:?}"));
        }
        checked += 1;
    }
    eprintln!("checked {checked} varied {varied} moved {moved} offsets {offset} asked {asked}");
    assert!(
        checked > 11_000 && varied > 8_000 && moved > 1_500 && offset > 4_500,
        "the control: {checked} {varied} {moved} {offset}"
    );
    // Exactly the varied rows of the one face with no GPOS -- four weights,
    // twenty texts -- read boxes at a variation and asked for scratch.
    assert_eq!(asked, 80, "rows that asked for scratch");
    assert!(
        wrong.is_empty(),
        "{} of {checked} differ, first: {:#?}",
        wrong.len(),
        &wrong[..wrong.len().min(8)]
    );
}
