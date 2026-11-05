//! Note Z step 5: the build-time bake against fontTools 4.63.0's instancer.
//!
//! `tests/var/bake/*.ttf` are fontTools' instances of the committed variable
//! fonts, kept as observations of that engine as `bake/VERSION` names it,
//! "fontTools 4.63.0": JetBrains Mono upright and italic at wght 400 and
//! 700, made by `fontTools.varLib.instancer.instantiateVariableFont(font,
//! {"wght": w})` with its defaults (overlap flags set, names left as they
//! are) on each `TTFont(path, recalcTimestamp=False)`, saved with
//! `recalcTimestamp` off so the head timestamp stays the variable file's.
//! The script that made them is retired, since a Rust port could not be
//! fontTools; observing them again needs fontTools 4.63.0 outside the
//! repository, as re-reading an observed source does. Every comparison
//! reads both files with this file's own small `glyf` reader, which shares
//! no code with the bake: the fixture was encoded by fontTools, so one
//! reader agreeing over both is an independent check, and a disagreement
//! names the glyph and the field.

use std::collections::BTreeMap;

use baud_font::var::Fixed;
use baud_font::{BakePoint, Cluster, Face, Features, Glyph, Ignorable, Refused, Shaper, bake};

const FONTS: [&str; 2] = ["JetBrainsMono-Variable", "JetBrainsMono-Italic-Variable"];
const WEIGHTS: [Weight; 2] = [Weight(400), Weight(700)];
/// `tools/baud-oracle cases bake`'s font, which reaches what JetBrains Mono
/// never makes the bake do, and the weights it is instanced at: both sides
/// of the default, through its `avar`.
const CASES: &str = "cases";
const CASE_WEIGHTS: [Weight; 4] = [Weight(250), Weight(550), Weight(700), Weight(875)];

fn source(name: &str) -> String {
    if name == CASES {
        "var/bake/cases.ttf".to_string()
    } else {
        format!("fonts/{name}.ttf")
    }
}

fn read(path: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn baked(name: &str, wght: Weight) -> Vec<u8> {
    let font = read(&source(name));
    let mut scratch = vec![BakePoint::ZERO; 1 << 16];
    let mut out = vec![0u8; 1 << 10];
    let len = match bake(&font, wght.fixed(), &mut scratch, &mut out) {
        Err(Refused::Out(need)) => {
            out = vec![0u8; usize::try_from(need.value()).unwrap()];
            bake(&font, wght.fixed(), &mut scratch, &mut out).unwrap()
        }
        other => panic!("{name} {wght}: {other:?}"),
    };
    out.truncate(usize::try_from(len.value()).unwrap());
    out
}

fn fixture(name: &str, wght: Weight) -> Vec<u8> {
    read(&format!("var/bake/{name}-{wght}.ttf"))
}

// The file's fields, read big-endian and widened to the `usize` they index
// with; the signed one stays `i16`.
macro_rules! be16 {
    ($b:expr, $at:expr) => {{
        let (b, at) = (&$b, $at);
        usize::from(u16::from_be_bytes([b[at], b[at + 1]]))
    }};
}
macro_rules! bei16 {
    ($b:expr, $at:expr) => {{
        let (b, at) = (&$b, $at);
        i16::from_be_bytes([b[at], b[at + 1]])
    }};
}
macro_rules! be32 {
    ($b:expr, $at:expr) => {{
        let (b, at) = (&$b, $at);
        usize::try_from(u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])).unwrap()
    }};
}

/// A weight in `wght`'s user units.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Weight(i32);

impl Weight {
    fn fixed(self) -> Fixed {
        Fixed(self.0 << 16)
    }
}

impl std::fmt::Display for Weight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The directory: tag to (offset, length).
fn directory(f: &[u8]) -> BTreeMap<[u8; 4], (usize, usize)> {
    let n = be16!(f, 4);
    (0..n)
        .map(|k| {
            let r = 12 + 16 * k;
            let tag: [u8; 4] = f[r..r + 4].try_into().unwrap();
            (tag, (be32!(f, r + 8), be32!(f, r + 12)))
        })
        .collect()
}

fn table<'f>(f: &'f [u8], tag: &[u8; 4]) -> &'f [u8] {
    let (at, len) = directory(f)[tag];
    &f[at..at + len]
}

/// One glyph as the file stores it, decoded.
#[derive(Debug, PartialEq)]
enum G {
    Empty,
    Simple {
        contours: i16,
        bbox: [i16; 4],
        ends: Vec<usize>,
        program: Vec<u8>,
        /// Absolute position and the on-curve and overlap bits, one a point.
        points: Vec<(i32, i32, u8)>,
    },
    Composite {
        contours: i16,
        bbox: [i16; 4],
        /// Flags less the bits a writer recomputes, glyph, offset.
        comps: Vec<(usize, usize, i32, i32)>,
        program: Vec<u8>,
    },
}

/// The component flags that say something about the glyph: ROUND_XY_TO_GRID,
/// USE_MY_METRICS, OVERLAP_COMPOUND and the two offset scalings. ARGS_ARE_XY
/// and the scale bits are checked separately; the word, more and
/// instruction bits are an encoding.
const MEANING: usize = 0x0004 | 0x0200 | 0x0400 | 0x0800 | 0x1000;

fn glyphs(f: &[u8]) -> Vec<G> {
    let head = table(f, b"head");
    let long = bei16!(head, 50) == 1;
    let n = be16!(table(f, b"maxp"), 4);
    let loca = table(f, b"loca");
    let glyf = table(f, b"glyf");
    let off = |k: usize| {
        if long {
            be32!(loca, 4 * k)
        } else {
            2 * be16!(loca, 2 * k)
        }
    };
    (0..n)
        .map(|k| {
            let (a, b) = (off(k), off(k + 1));
            if a == b {
                return G::Empty;
            }
            let g = &glyf[a..b];
            let contours = bei16!(g, 0);
            let bbox = [bei16!(g, 2), bei16!(g, 4), bei16!(g, 6), bei16!(g, 8)];
            if contours == 0 {
                return G::Empty;
            }
            if contours > 0 {
                let nc = usize::try_from(contours).unwrap();
                let ends: Vec<usize> = (0..nc).map(|c| be16!(g, 10 + 2 * c)).collect();
                let at = 10 + 2 * nc;
                let il = be16!(g, at);
                let program = g[at + 2..at + 2 + il].to_vec();
                let np = *ends.last().unwrap() + 1;
                let mut at = at + 2 + il;
                let mut flags = Vec::new();
                while flags.len() < np {
                    let fl = g[at];
                    at += 1;
                    flags.push(fl);
                    if fl & 8 != 0 {
                        for _ in 0..g[at] {
                            flags.push(fl);
                        }
                        at += 1;
                    }
                }
                let mut coord = |short: u8, same: u8| -> Vec<i32> {
                    let mut v = 0i32;
                    flags
                        .iter()
                        .map(|&fl| {
                            if fl & short != 0 {
                                let d = i32::from(g[at]);
                                at += 1;
                                v += if fl & same != 0 { d } else { -d };
                            } else if fl & same == 0 {
                                v += i32::from(bei16!(g, at));
                                at += 2;
                            }
                            v
                        })
                        .collect()
                };
                let xs = coord(0x02, 0x10);
                let ys = coord(0x04, 0x20);
                let points = (0..np).map(|p| (xs[p], ys[p], flags[p] & 0x41)).collect();
                return G::Simple {
                    contours,
                    bbox,
                    ends,
                    program,
                    points,
                };
            }
            let mut at = 10;
            let mut comps = Vec::new();
            let mut instructions = false;
            loop {
                let fl = be16!(g, at);
                let gid = be16!(g, at + 2);
                assert!(fl & 0x0002 != 0, "an anchored component");
                assert!(fl & (0x0008 | 0x0040 | 0x0080) == 0, "a scaled component");
                let (x, y, size) = if fl & 1 != 0 {
                    (
                        i32::from(bei16!(g, at + 4)),
                        i32::from(bei16!(g, at + 6)),
                        8,
                    )
                } else {
                    (i32::from(g[at + 4] as i8), i32::from(g[at + 5] as i8), 6)
                };
                comps.push((fl & MEANING, gid, x, y));
                instructions |= fl & 0x0100 != 0;
                at += size;
                if fl & 0x0020 == 0 {
                    break;
                }
            }
            let program = if instructions {
                let il = be16!(g, at);
                g[at + 2..at + 2 + il].to_vec()
            } else {
                Vec::new()
            };
            G::Composite {
                contours,
                bbox,
                comps,
                program,
            }
        })
        .collect()
}

/// Each glyph's advance and left side bearing.
fn metrics(f: &[u8]) -> Vec<(usize, i16)> {
    let n = be16!(table(f, b"maxp"), 4);
    let long = be16!(table(f, b"hhea"), 34);
    let hmtx = table(f, b"hmtx");
    (0..n)
        .map(|k| {
            if k < long {
                (be16!(hmtx, 4 * k), bei16!(hmtx, 4 * k + 2))
            } else {
                (
                    be16!(hmtx, 4 * (long - 1)),
                    bei16!(hmtx, 4 * long + 2 * (k - long)),
                )
            }
        })
        .collect()
}

fn each_instance(mut f: impl FnMut(&str, Weight, &[u8], &[u8])) {
    for name in FONTS {
        for w in WEIGHTS {
            f(name, w, &baked(name, w), &fixture(name, w));
        }
    }
    for w in CASE_WEIGHTS {
        f(CASES, w, &baked(CASES, w), &fixture(CASES, w));
    }
}

fn report(what: &str, wrong: &[String]) {
    assert!(
        wrong.is_empty(),
        "{what}: {} differ, first:\n{}",
        wrong.len(),
        wrong
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn every_glyph_is_fonttools() {
    let mut composites = 0;
    let mut simple = 0;
    let mut wrong = Vec::new();
    each_instance(|name, w, ours, theirs| {
        let (a, b) = (glyphs(ours), glyphs(theirs));
        assert_eq!(a.len(), b.len());
        for (k, (a, b)) in a.iter().zip(&b).enumerate() {
            match a {
                G::Simple { .. } => simple += 1,
                G::Composite { .. } => composites += 1,
                G::Empty => {}
            }
            if a != b {
                wrong.push(format!("{name}-{w} glyph {k}: ours {a:?}\n  theirs {b:?}"));
            }
        }
    });
    report("glyphs", &wrong);
    // The control: the comparison saw both kinds of glyph, in number.
    assert!(
        simple > 4_000 && composites > 2_000,
        "{simple} {composites}"
    );
}

#[test]
fn every_advance_and_side_bearing_is_fonttools() {
    let mut wrong = Vec::new();
    let mut seen = 0;
    each_instance(|name, w, ours, theirs| {
        for (k, (a, b)) in metrics(ours).iter().zip(metrics(theirs)).enumerate() {
            seen += 1;
            if *a != b {
                wrong.push(format!("{name}-{w} glyph {k}: ours {a:?} theirs {b:?}"));
            }
        }
    });
    report("hmtx", &wrong);
    assert!(seen > 6_000);
}

/// The table set is fontTools' less `STAT`, which the bake drops where
/// fontTools instances it.
#[test]
fn the_tables_are_fonttools_less_stat() {
    each_instance(|name, w, ours, theirs| {
        let mut want: Vec<[u8; 4]> = directory(theirs).into_keys().collect();
        want.retain(|t| t != b"STAT");
        let got: Vec<[u8; 4]> = directory(ours).into_keys().collect();
        assert_eq!(got, want, "{name}-{w}");
    });
}

#[test]
fn the_metric_tables_are_fonttools() {
    each_instance(|name, w, ours, theirs| {
        let at = |t: &[u8; 4]| (table(ours, t), table(theirs, t));
        // head: all but checkSumAdjustment; hhea whole.
        let (a, b) = at(b"head");
        assert_eq!(a.len(), b.len(), "{name}-{w} head");
        assert_eq!((&a[..8], &a[12..]), (&b[..8], &b[12..]), "{name}-{w} head");
        let (a, b) = at(b"hhea");
        assert!(a == b, "{name}-{w} hhea");
        for t in [b"OS/2", b"maxp", b"post", b"cmap", b"prep", b"gasp"] {
            // The table set is its own test; here, a table either has.
            if !directory(theirs).contains_key(t) {
                continue;
            }
            let (a, b) = at(t);
            assert!(a == b, "{name}-{w} {}", String::from_utf8_lossy(t));
        }
        let (a, b) = at(b"GDEF");
        assert_eq!(be32!(a, 0), be32!(b, 0), "{name}-{w} GDEF version");
        // Every anchor a mark lookup reaches is format 1 in both: a format
        // 3 left behind draws the same on a static face, so only its
        // format says so.
        let (a, b) = at(b"GPOS");
        let (fa, fb) = (anchor_formats(a), anchor_formats(b));
        assert!(!fb.is_empty() && fb.iter().all(|&f| f == 1), "{name}-{w}");
        assert_eq!(fa, fb, "{name}-{w} anchor formats");
    });
}

/// Every table's checksum, the directory's search fields and the whole
/// file's adjustment are what the format asks, and `ttf-parser` reads it.
#[test]
fn the_file_is_well_formed() {
    let sum = |b: &[u8]| {
        b.chunks(4).fold(0u32, |s, c| {
            let mut w = [0u8; 4];
            w[..c.len()].copy_from_slice(c);
            s.wrapping_add(u32::from_be_bytes(w))
        })
    };
    each_instance(|name, w, ours, _| {
        let n = be16!(ours, 4);
        let pow = 1usize << (usize::BITS - 1 - n.leading_zeros());
        assert_eq!(be16!(ours, 6), pow * 16, "{name}-{w}");
        assert_eq!(
            be16!(ours, 8),
            usize::try_from(pow.trailing_zeros()).unwrap()
        );
        assert_eq!(be16!(ours, 10), n * 16 - pow * 16);
        let mut prev = [0u8; 4];
        for k in 0..n {
            let r = 12 + 16 * k;
            let tag: [u8; 4] = ours[r..r + 4].try_into().unwrap();
            assert!(tag > prev, "{name}-{w} directory order");
            prev = tag;
            let (at, len) = (be32!(ours, r + 8), be32!(ours, r + 12));
            assert_eq!(at % 4, 0);
            let padded = &ours[at..(at + len).next_multiple_of(4)];
            // The padding is zero, as the format recommends and fontTools
            // writes it: nothing the bake moved is left behind in it.
            assert!(
                padded[len..].iter().all(|&b| b == 0),
                "{name}-{w} {} padding",
                String::from_utf8_lossy(&tag)
            );
            let mut body = padded.to_vec();
            if &tag == b"head" {
                body[8..12].fill(0);
            }
            assert_eq!(
                usize::try_from(sum(&body)).unwrap(),
                be32!(ours, r + 4),
                "{name}-{w} {}",
                String::from_utf8_lossy(&tag)
            );
        }
        assert_eq!(ours.len() % 4, 0);
        assert_eq!(sum(ours), 0xB1B0_AFBA, "{name}-{w} whole file");
        let face = ttf_parser::Face::parse(ours, 0).unwrap();
        assert_eq!(
            usize::from(face.number_of_glyphs()),
            be16!(table(ours, b"maxp"), 4)
        );
        assert!(!face.is_variable());
    });
}

/// Every mark sequence the step-4 oracle shapes, through the bake and
/// through fontTools' instance: glyphs and offsets equal, the offsets
/// carrying the pen, so the advances within a cluster too. This is
/// the one test that reads GPOS and GDEF as the shaper reads them, which a
/// byte comparison cannot, since fontTools recompiles both.
#[test]
fn every_mark_is_placed_as_in_fonttools() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/var/marks.txt"))
        .unwrap();
    let (mut checked, mut moved, mut wrong) = (0usize, 0usize, Vec::new());
    for name in FONTS {
        let seqs: std::collections::BTreeSet<&str> = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter(|l| l.split(' ').next() == Some(&format!("fonts/{name}.ttf")))
            .map(|l| l.split(' ').nth(2).unwrap())
            .collect();
        let shape = |font: &[u8], cps: &str| -> Vec<[i32; 4]> {
            let face = Face::parse(font).unwrap();
            let cps: Vec<baud_domain::Codepoint> = cps
                .split(',')
                .map(|h| baud_domain::Codepoint::new(u32::from_str_radix(h, 16).unwrap()).unwrap())
                .collect();
            let mut storage =
                [Glyph::new(baud_domain::GlyphId::NOTDEF, Cluster(0), Ignorable::No); 16];
            let mut shaper = Shaper::new(&face, &mut storage);
            shaper.push_cluster(&cps, Cluster(0)).unwrap();
            let (buf, _) = shaper.finish(Features::GRID, baud_font::Script::of_cluster(&cps));
            buf.as_slice()
                .iter()
                .map(|g| {
                    [
                        i32::try_from(g.id.index()).unwrap(),
                        i32::from(g.cluster.0),
                        i32::from(g.dx),
                        i32::from(g.dy),
                    ]
                })
                .collect()
        };
        let regular = baked(name, Weight(400));
        for w in WEIGHTS {
            let (ours, theirs) = (baked(name, w), fixture(name, w));
            for cps in &seqs {
                let (a, b) = (shape(&ours, cps), shape(&theirs, cps));
                checked += 1;
                moved += usize::from(w != Weight(400) && a != shape(&regular, cps));
                if a != b {
                    wrong.push(format!("{name}-{w} {cps}: ours {a:?} theirs {b:?}"));
                }
            }
        }
    }
    report("marks", &wrong);
    // The control: the sequences were shaped, and at 700 the marks moved.
    assert!(checked > 3_500 && moved > 250, "{checked} {moved}");
}

/// The case font's own marks: `acc` on `sq` through the varied anchors, and
/// `dot` through the plain one.
#[test]
fn the_case_fonts_marks_are_placed_as_in_fonttools() {
    let mut moved = 0;
    for w in CASE_WEIGHTS {
        let (ours, theirs) = (baked(CASES, w), fixture(CASES, w));
        for text in ["AH", "AB"] {
            let (a, b) = (shape(&ours, text), shape(&theirs, text));
            assert_eq!(a, b, "{w} {text}");
            moved += usize::from(text == "AH" && a != shape(&baked(CASES, Weight(400)), text));
        }
    }
    assert_eq!(moved, 4);
}

fn shape(font: &[u8], text: &str) -> Vec<[i32; 4]> {
    let face = Face::parse(font).unwrap();
    let cps: Vec<baud_domain::Codepoint> = text
        .chars()
        .map(|c| baud_domain::Codepoint::new(u32::from(c)).unwrap())
        .collect();
    let mut storage = [Glyph::new(baud_domain::GlyphId::NOTDEF, Cluster(0), Ignorable::No); 16];
    let mut shaper = Shaper::new(&face, &mut storage);
    shaper.push_cluster(&cps, Cluster(0)).unwrap();
    let (buf, _) = shaper.finish(Features::GRID, baud_font::Script::of_cluster(&cps));
    buf.as_slice()
        .iter()
        .map(|g| {
            [
                i32::try_from(g.id.index()).unwrap(),
                i32::from(g.cluster.0),
                i32::from(g.dx),
                i32::from(g.dy),
            ]
        })
        .collect()
}

/// The same bytes twice, and into a buffer that held something else: no
/// byte of the output comes from the caller's buffer.
#[test]
fn the_bake_is_deterministic_whatever_the_buffer_held() {
    let font = read(&source(CASES));
    let clean = baked(CASES, Weight(700));
    let mut scratch = vec![BakePoint::ZERO; 1 << 16];
    let mut dirty = vec![0xAAu8; clean.len() + 100];
    let len = bake(&font, Weight(700).fixed(), &mut scratch, &mut dirty).unwrap();
    assert_eq!(&dirty[..usize::try_from(len.value()).unwrap()], &clean[..]);
    assert_eq!(
        baked("JetBrainsMono-Variable", Weight(700)),
        baked("JetBrainsMono-Variable", Weight(700))
    );
}

/// A buffer too small is told the size it needs; that size bakes, one byte
/// fewer is told the same size again, and the font fits inside it.
#[test]
fn a_short_buffer_is_told_the_size_it_needs() {
    let font = read(&source(CASES));
    let mut scratch = vec![BakePoint::ZERO; 1 << 16];
    let mut empty = [0u8; 0];
    let Err(Refused::Out(need)) = bake(&font, Weight(550).fixed(), &mut scratch, &mut empty) else {
        panic!("an empty buffer is not refused");
    };
    let need = usize::try_from(need.value()).unwrap();
    let mut out = vec![0u8; need];
    let len = bake(&font, Weight(550).fixed(), &mut scratch, &mut out).unwrap();
    let len = usize::try_from(len.value()).unwrap();
    assert!(len <= need);
    assert_eq!(&out[..len], &baked(CASES, Weight(550))[..]);
    let mut short = vec![0u8; need - 1];
    assert_eq!(
        bake(&font, Weight(550).fixed(), &mut scratch, &mut short),
        Err(Refused::Out(baud_font::bytes::Len::of(
            u32::try_from(need).unwrap()
        )))
    );
}

#[test]
fn a_short_scratch_is_refused() {
    let font = read(&source(CASES));
    let mut out = vec![0u8; 1 << 16];
    // sq has five points and four phantoms.
    let mut scratch = vec![BakePoint::ZERO; 8];
    assert_eq!(
        bake(&font, Weight(700).fixed(), &mut scratch, &mut out),
        Err(Refused::Scratch)
    );
    let mut scratch = vec![BakePoint::ZERO; 64];
    assert!(bake(&font, Weight(700).fixed(), &mut scratch, &mut out).is_ok());
}

/// What the bake does not instance, it refuses, and says which.
#[test]
fn what_the_bake_cannot_instance_is_refused() {
    let mut scratch = vec![BakePoint::ZERO; 1 << 16];
    let mut out = vec![0u8; 1 << 20];
    let mut go = |font: &[u8]| bake(font, Weight(700).fixed(), &mut scratch, &mut out);
    assert_eq!(
        go(&read("fonts/JetBrainsMono-Regular.ttf")),
        Err(Refused::Axes)
    );
    // Two axes, wght and wdth.
    assert_eq!(go(&read("var/gvar-cases.ttf")), Err(Refused::Axes));
    assert_eq!(go(b"not a font"), Err(Refused::NotAFont));
    let mut cff = read(&source(CASES));
    cff[..4].copy_from_slice(b"OTTO");
    assert_eq!(go(&cff), Err(Refused::NotTrueType));
    // A table the bake does not instance, by renaming one in the directory.
    let mut mvar = read(&source(CASES));
    let n = be16!(&mvar, 4);
    let post = (0..n)
        .map(|k| 12 + 16 * k)
        .find(|&r| &mvar[r..r + 4] == b"post")
        .unwrap();
    mvar[post..post + 4].copy_from_slice(b"MVAR");
    assert_eq!(
        go(&mvar),
        Err(Refused::Table(baud_font::Tag::from_bytes(*b"MVAR")))
    );
}

/// The bake is fontTools' instance and the runtime draws HarfBuzz's
/// variation, and the two agree: every simple glyph's points within half a
/// unit -- the rounding -- and a composite's within one, its components'
/// offsets rounded before their points are. The case font's `sq` and
/// `instc` move their left phantom, and fontTools keeps their points and
/// writes the moved bearing as `lsb`, no longer xMin, with `head` bit 1
/// cleared: the static face agrees with the varied one only if it is drawn
/// from its left side bearing point, `xMin - lsb`
/// (`[[source: ms-opentype-hmtx]]`), as the varied one is drawn from the
/// moved phantom -- drawn from x 0 the static one was 7.1 units off at
/// 875. Its points are rounded to the unit and so is that bearing, so the
/// case font's glyphs agree within one unit, simple ones included.
#[test]
fn the_baked_outline_is_the_runtimes_within_a_unit() {
    use baud_font::var::Variation;
    use baud_font::{Sink, VarPoint};
    #[derive(Default)]
    struct Points(Vec<(f32, f32)>);
    impl Sink for Points {
        fn move_to(&mut self, x: f32, y: f32) {
            self.0.push((x, y));
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.0.push((x, y));
        }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            self.0.push((x1, y1));
            self.0.push((x, y));
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            self.0.extend([(x1, y1), (x2, y2), (x, y)]);
        }
        fn close(&mut self) {}
    }
    let (mut simple, mut composite, mut cases) = (0f32, 0f32, 0f32);
    let mut glyphs = 0;
    let runs = FONTS
        .iter()
        .map(|&name| (name, Weight(700)))
        .chain(CASE_WEIGHTS.iter().map(|&w| (CASES, w)));
    for (name, wght) in runs {
        let var = read(&source(name));
        let ours = baked(name, wght);
        let mut face = Face::parse(&var).unwrap();
        face.set_variations(&[Variation {
            tag: baud_font::Tag::from_bytes(*b"wght"),
            value: wght.fixed(),
        }]);
        let stat = Face::parse(&ours).unwrap();
        let kinds = self::glyphs(&ours);
        let mut scratch = vec![VarPoint::EMPTY; 1 << 12];
        for k in 0..stat.glyph_count().value() {
            let g = baud_domain::GlyphId::from_shaper(k);
            let (mut a, mut b) = (Points::default(), Points::default());
            face.outline_at(g, &mut scratch, &mut a);
            stat.outline(g, &mut b);
            assert_eq!(a.0.len(), b.0.len(), "{name} {wght} glyph {k}");
            let worst = match kinds[usize::from(k)] {
                _ if name == CASES => &mut cases,
                G::Composite { .. } => &mut composite,
                _ => &mut simple,
            };
            for (p, q) in a.0.iter().zip(&b.0) {
                *worst = worst.max((p.0 - q.0).abs()).max((p.1 - q.1).abs());
            }
            glyphs += 1;
        }
    }
    assert!(glyphs > 3_000);
    eprintln!("worst: simple {simple}, composite {composite}, cases {cases}");
    assert!(simple <= 0.5, "{simple}");
    assert!(composite <= 1.0, "{composite}");
    assert!(cases <= 1.0, "{cases}");
}

/// The format of every anchor the mark lookups (4, 5 and 6, through
/// extensions) reach, in the order they are reached.
fn anchor_formats(gpos: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let list = be16!(gpos, 8);
    let anchor = |at: usize, field: usize, out: &mut Vec<usize>| {
        let o = be16!(gpos, field);
        if o != 0 {
            out.push(be16!(gpos, at + o));
        }
    };
    for k in 0..be16!(gpos, list) {
        let lookup = list + be16!(gpos, list + 2 + 2 * k);
        let mut kind = be16!(gpos, lookup);
        for j in 0..be16!(gpos, lookup + 4) {
            let mut sub = lookup + be16!(gpos, lookup + 6 + 2 * j);
            if kind == 9 {
                kind = be16!(gpos, sub + 2);
                sub += be32!(gpos, sub + 4);
            }
            if !(4..=6).contains(&kind) {
                continue;
            }
            let classes = be16!(gpos, sub + 6);
            let marks = sub + be16!(gpos, sub + 8);
            for r in 0..be16!(gpos, marks) {
                anchor(marks, marks + 2 + 4 * r + 2, &mut out);
            }
            let arr = sub + be16!(gpos, sub + 10);
            let n = be16!(gpos, arr);
            if kind == 5 {
                for l in 0..n {
                    let at = arr + be16!(gpos, arr + 2 + 2 * l);
                    for r in 0..be16!(gpos, at) * classes {
                        anchor(at, at + 2 + 2 * r, &mut out);
                    }
                }
            } else {
                for r in 0..n * classes {
                    anchor(arr, arr + 2 + 2 * r, &mut out);
                }
            }
        }
    }
    out.sort_unstable();
    out
}
