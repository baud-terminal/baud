//! Shaping against HarfBuzz's golden fixtures.
//!
//! `tests/shape/gsub.txt` is what `hb-shape` produced for each (face,
//! text) with the grid feature set -- `ccmp` and `calt` on, the rest off --
//! written once by `scripts/gen-shape-fixtures.sh` and committed. Each line
//! is compared glyph for glyph: the ids and the cluster each glyph carries.
//!
//! **Clusters follow HarfBuzz's level 0**: a combining mark or a
//! default-ignorable joins the cluster before it, and a ligature keeps its
//! first cluster. That is also the terminal's rule, because a mark lives in
//! its base's cell, so the fixture and the shaper agree on what a cluster
//! is without translation.

use baud_domain::{Classified, Codepoint, GlyphId, GraphemeClass, Width};
use baud_font::{
    Buffer, Cluster, Digest, Face, FaceIndex, Features, Glyph, Ignorable, MAX_LOOKUPS, Script,
    Shaper, Sink, Unresolved, resolve,
};

struct Fixture {
    face: String,
    text: Vec<u32>,
    expected: Vec<(u16, u16)>,
}

fn fixtures() -> Vec<Fixture> {
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/shape/gsub.txt"))
            .unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let text = f[1]
                .split_whitespace()
                .map(|h| u32::from_str_radix(h, 16).unwrap())
                .collect();
            let body = f[2].trim_start_matches('[').trim_end_matches(']');
            let expected = body
                .split('|')
                .map(|g| {
                    let (id, cl) = g.split_once('=').unwrap();
                    (id.parse().unwrap(), cl.parse().unwrap())
                })
                .collect();
            Fixture {
                face: f[0].to_string(),
                text,
                expected,
            }
        })
        .collect()
}

/// Shapes `text` through the shaper, clustering marks and ignorables with
/// the codepoint before them, the way HarfBuzz's level 0 and the terminal
/// both do.
fn shape<'b>(
    face: &Face<'_>,
    text: &[u32],
    storage: &'b mut [Glyph],
) -> (Buffer<'b>, baud_font::Shaped) {
    shape_with(face, text, storage, &[])
}

/// [`shape`] with the lookup digests handed to the shaper.
fn shape_with<'b>(
    face: &Face<'_>,
    text: &[u32],
    storage: &'b mut [Glyph],
    digests: &[Digest],
) -> (Buffer<'b>, baud_font::Shaped) {
    let cps: Vec<Codepoint> = text.iter().map(|&v| Codepoint::new(v).unwrap()).collect();
    let mut shaper = Shaper::new(face, storage);
    shaper.accelerate(digests);
    let mut start = 0usize;
    let mut cells: Vec<Unresolved> = Vec::new();
    let is_ri = |c: Codepoint| (0x1F1E6..=0x1F1FF).contains(&c.value());
    for i in 1..=cps.len() {
        let boundary = i == cps.len() || {
            let c = cps[i];
            let cl = Classified::of(c);
            let zero = cl.width(baud_domain::WidthPolicy::DEFAULT) == Width::ZERO;
            // GB9: extenders, joiners and spacing marks continue the
            // cluster; GB11: a pictograph after a joiner does too; GB12:
            // regional indicators pair up. That is the cell the core's
            // cluster machine hands the shaper, and HarfBuzz's level 0.
            let extends = matches!(
                cl.class,
                GraphemeClass::Extend | GraphemeClass::Zwj | GraphemeClass::SpacingMark
            );
            let joined = cps[i - 1].value() == 0x200D && cl.lookback.pictographic();
            let paired = is_ri(c) && {
                let run = cps[start..i]
                    .iter()
                    .rev()
                    .take_while(|&&p| is_ri(p))
                    .count();
                run % 2 == 1
            };
            !(zero || extends || Ignorable::of(c) == Ignorable::Yes || joined || paired)
        };
        if boundary {
            shaper
                .push_cluster(&cps[start..i], Cluster(u16::try_from(start).unwrap()))
                .unwrap();
            cells.push(Unresolved::of(&cps[start..i]));
            start = i;
        }
    }
    shaper.finish(Features::GRID, one_script(&cells))
}

/// The script a fixture's text settles to, which must be one: `hb-shape`
/// shapes a whole line in the script of its first character that names one
/// (`[[source: harfbuzz-hb-buffer]]`, `guess_segment_properties`), and a
/// fixture is written as one run. A line that settled to two would compare
/// one run against two.
fn one_script(cells: &[Unresolved]) -> Script {
    let mut settled = vec![Script::COMMON; cells.len()];
    resolve(cells, &mut settled);
    let first = settled.first().copied().unwrap_or(Script::COMMON);
    assert!(
        settled.iter().all(|&s| s == first),
        "a fixture settles to more than one script: {settled:?}"
    );
    first
}

#[test]
fn every_gsub_fixture_shapes_as_harfbuzz_shaped_it() {
    let fixtures = fixtures();
    assert!(fixtures.len() >= 150, "{}", fixtures.len());
    let mut faces = std::collections::HashMap::new();
    let mut ligatures = 0usize;
    let mut glyphs = 0usize;
    for fx in &fixtures {
        let data = faces.entry(fx.face.clone()).or_insert_with(|| {
            std::fs::read(format!(
                "{}/tests/fonts/{}",
                env!("CARGO_MANIFEST_DIR"),
                fx.face
            ))
            .unwrap()
        });
        let face = Face::parse(data).unwrap();
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 128];
        let before = fx.text.len();
        let (buf, shaped) = shape(&face, &fx.text, &mut storage);
        assert!(shaped.declined.is_zero(), "{}: lookups declined", fx.face);
        let got: Vec<(u16, u16)> = buf
            .as_slice()
            .iter()
            .map(|g| (u16::try_from(g.id.index()).unwrap(), g.cluster.0))
            .collect();
        assert_eq!(
            got, fx.expected,
            "{} {:04X?}: baud {:?} against hb-shape {:?}",
            fx.face, fx.text, got, fx.expected
        );
        ligatures += before.saturating_sub(got.len());
        glyphs += got.len();
    }
    eprintln!(
        "{} fixtures, {glyphs} glyphs, {ligatures} glyphs consumed by ligatures",
        fixtures.len()
    );
    // Positive controls: Fira Code's `->` and `!=` are chained singles that
    // keep the glyph count, Bungee Tint has no GSUB and its lines must pass
    // untouched, and something must have formed a ligature somewhere.
    assert!(glyphs > 800, "{glyphs}");
    assert!(ligatures > 0, "{ligatures}");
}

#[test]
fn a_ligature_spans_the_cells_of_its_components() {
    // `Cluster` and `last` are what the cell mapper reads; a ligature must
    // report the span of every cell it consumed, and a chained single
    // substitution -- Fira's whole scheme -- must not widen anything.
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fonts/JetBrainsMono-Regular.ttf"
    ))
    .unwrap();
    let face = Face::parse(&data).unwrap();
    let text: Vec<u32> = "a->b".chars().map(u32::from).collect();
    let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 16];
    let (buf, _) = shape(&face, &text, &mut storage);
    let spans: Vec<(u16, u16)> = buf
        .as_slice()
        .iter()
        .map(|g| (g.cluster.0, g.last.0))
        .collect();
    // Every cell 0..4 is covered exactly once, whatever the glyph count.
    let mut covered = [0u8; 4];
    for (a, b) in &spans {
        for c in *a..=*b {
            covered[usize::from(c)] += 1;
        }
    }
    assert_eq!(covered, [1, 1, 1, 1], "{spans:?}");
}

/// One glyph of an `hb-shape` line with positions on:
/// `gid=cluster[@dx,dy]+ax`.
struct HbGlyph {
    id: u16,
    cluster: u16,
    dx: i32,
    dy: i32,
    ax: i32,
}

fn parse_positioned(body: &str) -> Vec<HbGlyph> {
    body.trim_start_matches('[')
        .trim_end_matches(']')
        .split('|')
        .map(|g| {
            let (id, rest) = g.split_once('=').unwrap();
            let (cluster_and_off, ax) = rest.split_once('+').unwrap();
            let (cluster, off) = match cluster_and_off.split_once('@') {
                Some((c, o)) => (c, Some(o)),
                None => (cluster_and_off, None),
            };
            let (dx, dy) = match off {
                Some(o) => {
                    let (x, y) = o.split_once(',').unwrap();
                    (x.parse().unwrap(), y.parse().unwrap())
                }
                None => (0, 0),
            };
            HbGlyph {
                id: id.parse().unwrap(),
                cluster: cluster.parse().unwrap(),
                dx,
                dy,
                ax: ax.parse().unwrap(),
            }
        })
        .collect()
}

/// Whether to rename the face's `GPOS` table away before parsing, so
/// the shaper takes the same fallback path HarfBuzz took for the fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GposTable {
    Kept,
    Stripped,
    /// Kept, with every Script table renamed past any script's tag.
    Unselected,
}

/// Renames every Script table of the face's `GPOS` to `zzz0`, `zzz1` and
/// on, in their order, so the ScriptList stays sorted and no script selects
/// any of them: the table and its lookups stay, and a Hebrew run finds
/// neither `hebr` nor `DFLT` nor `latn`, as on Arial Unicode MS.
fn unselect_gpos(data: &mut [u8]) {
    let be16 = |d: &[u8], at: usize| usize::from(u16::from_be_bytes([d[at], d[at + 1]]));
    let n = be16(data, 4);
    let gpos = (0..n)
        .map(|i| 12 + 16 * i)
        .find(|&at| &data[at..at + 4] == b"GPOS")
        .map(|at| {
            usize::try_from(u32::from_be_bytes([
                data[at + 8],
                data[at + 9],
                data[at + 10],
                data[at + 11],
            ]))
            .unwrap()
        })
        .expect("a GPOS to unselect");
    let list = gpos + be16(data, gpos + 4);
    for (k, i) in (0..be16(data, list)).enumerate() {
        let at = list + 2 + 6 * i;
        data[at..at + 4].copy_from_slice(&[b'z', b'z', b'z', b'0' + u8::try_from(k).unwrap()]);
    }
}

/// Renames the `GPOS` directory entry to `GPOZ` -- a tag that keeps the
/// directory sorted for a binary search -- the same edit
/// `scripts/gen-shape-fixtures.sh` makes for the fallback fixtures.
fn strip_gpos(data: &mut [u8]) -> usize {
    let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let mut renamed = 0;
    for i in 0..n {
        let at = 12 + 16 * i;
        if &data[at..at + 4] == b"GPOS" {
            data[at..at + 4].copy_from_slice(b"GPOZ");
            renamed += 1;
        }
    }
    renamed
}

struct Agreement {
    fixtures: usize,
    positioned: usize,
    raised: usize,
}

/// Compares every line of a positioning fixture file against the shaper.
/// HarfBuzz offsets a glyph from its own pen position, which for a mark is
/// one base advance past the cell origin baud offsets from -- so baud's dx
/// is HarfBuzz's plus the advances of the glyphs before it in the same
/// cluster. dy is the same frame in both.
fn positions_agree(file: &str, gpos: GposTable) -> Agreement {
    let text =
        std::fs::read_to_string(format!("{}/tests/shape/{file}", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let mut faces = std::collections::HashMap::new();
    let mut agreement = Agreement {
        fixtures: 0,
        positioned: 0,
        raised: 0,
    };
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        let cps: Vec<u32> = f[1]
            .split_whitespace()
            .map(|h| u32::from_str_radix(h, 16).unwrap())
            .collect();
        let expected = parse_positioned(f[2]);
        let data = faces.entry(f[0].to_string()).or_insert_with(|| {
            let mut d = std::fs::read(format!(
                "{}/tests/fonts/{}",
                env!("CARGO_MANIFEST_DIR"),
                f[0]
            ))
            .unwrap();
            if gpos == GposTable::Stripped {
                assert_eq!(strip_gpos(&mut d), 1, "{}: no GPOS to strip", f[0]);
            }
            d
        });
        let face = Face::parse(data).unwrap();
        assert_eq!(
            face.gpos().is_none(),
            gpos == GposTable::Stripped,
            "{}",
            f[0]
        );
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 128];
        let (buf, shaped) = shape(&face, &cps, &mut storage);
        assert!(shaped.declined.is_zero());
        let got = buf.as_slice();
        assert_eq!(got.len(), expected.len(), "{} {:04X?}", f[0], cps);
        let mut pen_in_cluster = 0i32;
        let mut cluster_start: Option<u16> = None;
        for (g, e) in got.iter().zip(&expected) {
            assert_eq!(
                u16::try_from(g.id.index()).unwrap(),
                e.id,
                "{} {:04X?}",
                f[0],
                cps
            );
            assert_eq!(g.cluster.0, e.cluster, "{} {:04X?}", f[0], cps);
            if cluster_start != Some(e.cluster) {
                cluster_start = Some(e.cluster);
                pen_in_cluster = 0;
            }
            let want_dx = e.dx + pen_in_cluster;
            assert_eq!(
                (i32::from(g.dx), i32::from(g.dy)),
                (want_dx, e.dy),
                "{} {:04X?} glyph {}: baud ({}, {}) against hb-shape ({}, {}) + pen {}",
                f[0],
                cps,
                e.id,
                g.dx,
                g.dy,
                e.dx,
                e.dy,
                pen_in_cluster
            );
            if e.dx != 0 || e.dy != 0 {
                agreement.positioned += 1;
            }
            if e.dy != 0 {
                agreement.raised += 1;
            }
            pen_in_cluster += e.ax;
        }
        agreement.fixtures += 1;
    }
    agreement
}

#[test]
fn every_gpos_fixture_positions_marks_as_harfbuzz_does() {
    let a = positions_agree("gpos.txt", GposTable::Kept);
    eprintln!(
        "{} positioning fixtures, {} marks moved by a lookup",
        a.fixtures, a.positioned
    );
    assert!(a.fixtures >= 40, "{}", a.fixtures);
    // Fira stacks a macron on a diaeresis and JetBrains Mono anchors an
    // acute over a dotted a; a lookup must have moved something.
    assert!(a.positioned >= 5, "{}", a.positioned);
}

#[test]
fn every_fallback_fixture_places_marks_as_harfbuzz_does() {
    // No committed face lacks GPOS, so the fixtures were shaped on copies
    // with the table renamed away, and the same rename is made here: what
    // is compared is the geometric fallback, on both sides.
    let a = positions_agree("fallback.txt", GposTable::Stripped);
    eprintln!(
        "{} fallback fixtures, {} marks placed, {} of them vertically",
        a.fixtures, a.positioned, a.raised
    );
    assert!(a.fixtures >= 40, "{}", a.fixtures);
    // A stacked acute rides a diaeresis and a capital lifts its mark, so
    // the vertical half of the placer must have moved something.
    assert!(a.positioned >= 20, "{}", a.positioned);
    assert!(a.raised >= 10, "{}", a.raised);
}

/// FNV-1a over a file, the digest `scripts/gen-shape-fixtures.sh` records
/// beside each system face so a replaced font is refused, not compared.
fn fnv64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[test]
fn every_morx_fixture_shapes_as_harfbuzz_does() {
    // The AAT faces are the ones macOS ships, so this runs there and is
    // absent elsewhere -- said here so the absence is not mistaken for a
    // pass. A face the OS has replaced since the fixture was written fails
    // loudly with the regeneration step, rather than comparing two fonts.
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: the morx fixtures name macOS system faces");
        return;
    }
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/shape/morx.txt"))
            .unwrap();
    let mut files = std::collections::HashMap::new();
    let mut fixtures = 0usize;
    let mut ligated = 0usize;
    let mut rtl = 0usize;
    let mut faces = std::collections::HashSet::new();
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        let (path, index, digest, cps, shaped) = (f[0], f[1], f[2], f[4], f[5]);
        let data = files.entry(path.to_string()).or_insert_with(|| {
            let d = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert_eq!(
                format!("{:016x}", fnv64(&d)),
                digest,
                "{path} is not the file the fixtures were shaped with; rerun \
                 scripts/gen-shape-fixtures.sh under nix-shell -p harfbuzz"
            );
            d
        });
        let face = Face::parse_index(data, FaceIndex::of(index.parse().unwrap())).unwrap();
        assert!(face.morx().is_some() && face.gsub().is_none(), "{path}");
        faces.insert((path.to_string(), index.to_string()));
        let cps: Vec<u32> = cps
            .split_whitespace()
            .map(|h| u32::from_str_radix(h, 16).unwrap())
            .collect();
        // hb-shape writes a right-to-left run in visual order, its last
        // cluster first; baud's buffer is in logical order. Each glyph is
        // placed where hb-shape's pen puts it, in visual order, and taken
        // from its cluster's left edge, as `script.txt`'s are: right to
        // left, a glyph after its cluster's base sits left of it.
        let expected = parse_positioned(shaped);
        let mut placed: Vec<(&HbGlyph, i32)> = Vec::new();
        let mut pen = 0i32;
        for e in &expected {
            placed.push((e, pen));
            pen += e.ax;
        }
        if expected.first().map(|e| e.cluster) > expected.last().map(|e| e.cluster) {
            placed.reverse();
            rtl += 1;
        }
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 128];
        let (buf, shaped) = shape(&face, &cps, &mut storage);
        assert!(
            shaped.declined.is_zero(),
            "{path} {cps:04X?}: subtables declined"
        );
        let got: Vec<(u16, u16, i32, i32)> = buf
            .as_slice()
            .iter()
            .map(|g| {
                (
                    u16::try_from(g.id.index()).unwrap(),
                    g.cluster.0,
                    i32::from(g.dx),
                    i32::from(g.dy),
                )
            })
            .collect();
        let mut left = std::collections::HashMap::new();
        for &(e, pen) in &placed {
            let l = left.entry(e.cluster).or_insert(pen);
            *l = (*l).min(pen);
        }
        let want: Vec<(u16, u16, i32, i32)> = placed
            .iter()
            .map(|&(e, pen)| (e.id, e.cluster, pen + e.dx - left[&e.cluster], e.dy))
            .collect();
        assert_eq!(
            got, want,
            "{path} {cps:04X?}: baud {got:?} against hb-shape {want:?}"
        );
        if got.len() < cps.len() {
            ligated += 1;
        }
        fixtures += 1;
    }
    eprintln!(
        "{fixtures} morx fixtures over {} faces, {ligated} formed a ligature, {rtl} right to left",
        faces.len()
    );
    // Positive controls: every face the generator names was present and
    // matched, and Apple Color Emoji's families and flags ligated.
    assert!(faces.len() >= 7, "{}", faces.len());
    assert!(fixtures >= 150, "{fixtures}");
    assert!(ligated >= 30, "{ligated}");
    assert!(rtl == 8, "{rtl}");
}

#[test]
fn the_lookup_digests_change_nothing_on_any_gsub_fixture() {
    // The accelerator may only skip lookups that cannot match, so every
    // GSUB fixture must shape identically with and without it -- and the
    // digests must actually narrow something, or they accelerate nothing.
    let fixtures = fixtures();
    let mut faces = std::collections::HashMap::new();
    let mut checked = 0usize;
    for fx in &fixtures {
        let data = faces.entry(fx.face.clone()).or_insert_with(|| {
            std::fs::read(format!(
                "{}/tests/fonts/{}",
                env!("CARGO_MANIFEST_DIR"),
                fx.face
            ))
            .unwrap()
        });
        let face = Face::parse(data).unwrap();
        let Some(gsub) = face.gsub() else {
            continue;
        };
        let mut digests = vec![Digest::EMPTY; MAX_LOOKUPS];
        gsub.digests(Features::GRID, &mut digests);
        let mut a = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 128];
        let mut b = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 128];
        let (plain, _) = shape(&face, &fx.text, &mut a);
        let (fast, _) = shape_with(&face, &fx.text, &mut b, &digests);
        assert_eq!(
            plain.as_slice(),
            fast.as_slice(),
            "{} {:04X?}",
            fx.face,
            fx.text
        );
        checked += 1;
    }
    assert!(checked >= 150, "{checked}");
    // Positive control: most named lookups have a digest narrower than
    // everything, or nothing is ever skipped.
    let data = faces.get("FiraCode-Regular.ttf").unwrap();
    let face = Face::parse(data).unwrap();
    let mut digests = vec![Digest::EMPTY; MAX_LOOKUPS];
    face.gsub().unwrap().digests(Features::GRID, &mut digests);
    let named = digests.iter().filter(|d| **d != Digest::EMPTY).count();
    let narrow = digests
        .iter()
        .filter(|d| **d != Digest::EMPTY && **d != Digest::ALL)
        .count();
    assert!(
        named > 50 && narrow * 2 > named,
        "{named} named, {narrow} narrow"
    );
}

/// **Text is shaped with its own script's table, else `DFLT`.** The
/// specification: "An application should use a DFLT script table if there
/// is not a script table associated with the specific script of the text
/// being formatted, or if the text does not have a specific script"
/// (`[[source: ms-opentype-chapter2]]`, ScriptList table). Every line of
/// `tests/shape/script.txt` is what hb-shape made of it with the script and
/// direction guessed from the text; a Hebrew line comes back right to left
/// in visual order, and is reversed here into the logical order baud draws
/// it in, each glyph's offset taken from its cluster's left edge.
///
/// Courier New is what macOS falls back to for Hebrew and the face the
/// fault was found on (note AA, section 6): its `latn` table names no
/// features at all and its `hebr` one names `ccmp` and `mark`, so a point
/// shaped `latn` sat one cell right of its letter. It is a system face, so
/// its lines run on macOS only, and a different file fails loudly rather
/// than comparing two fonts. The built JetBrains Mono carries a `hebr`
/// record (note AC) and runs everywhere, so its lines carry the controls
/// that hold on every platform, and Courier New's have their own on macOS.
#[test]
fn every_script_fixture_shapes_as_harfbuzz_does() {
    // One binding for the skip and for Courier New's control, so a run with
    // the skip forced checks what a platform without the face checks.
    let on_macos = cfg!(target_os = "macos");
    let Tally {
        fixtures: Reached(fixtures),
        hebrew: Reached(hebrew),
        moved: Reached(moved),
        system_fixtures: Reached(system_fixtures),
        system_hebrew: Reached(system_hebrew),
        skipped: Reached(skipped),
        ..
    } = shape_script_fixtures("script.txt", on_macos, GposTable::Kept);
    eprintln!(
        "built faces: {fixtures} script fixtures, {hebrew} of them Hebrew, {moved} glyphs \
         offset; system faces: {system_fixtures} shaped, {system_hebrew} of them Hebrew, \
         {skipped} skipped"
    );
    // Positive controls on the built Regular's lines, which run everywhere:
    // the Hebrew lines were reached, and some point was placed by an anchor
    // rather than left at the pen.
    assert!(fixtures >= 20, "{fixtures}");
    assert!(hebrew >= 13, "{hebrew}");
    assert!(moved >= 8, "{moved}");
    // And Courier New's lines, where the face is: shaped on macOS, every
    // one of them counted as skipped elsewhere.
    if on_macos {
        assert!(system_fixtures >= 20, "{system_fixtures}");
        assert!(system_hebrew >= 13, "{system_hebrew}");
    } else {
        assert!(skipped >= 20 && system_fixtures == 0, "{skipped}");
    }
}

/// **A reverse chain runs from the end of the run back to its start**
/// (`GSUB` type 8, "processed in reverse order", `[[source:
/// ms-opentype-gsub]]`). Every line of `tests/shape/reverse.txt` is what
/// hb-shape made of a Coptic word with a combining overline on each
/// letter, compared as `script.txt`'s are: on Noto Sans Coptic, which
/// macOS ships, an overline before a capital's takes the capital's height,
/// and so does each before it, back to the word's start, which a walk from
/// the start could not carry. The built Regular's lines, with no Coptic,
/// run everywhere and are the control.
#[test]
fn every_reverse_chain_fixture_shapes_as_harfbuzz_does() {
    let on_macos = cfg!(target_os = "macos");
    let Tally {
        fixtures: Reached(fixtures),
        system_fixtures: Reached(system_fixtures),
        skipped: Reached(skipped),
        ..
    } = shape_script_fixtures("reverse.txt", on_macos, GposTable::Kept);
    assert_eq!(fixtures, 10, "the control's lines");
    match on_macos {
        true => assert_eq!(system_fixtures, 10, "Noto Sans Coptic's lines"),
        false => assert!(skipped == 10 && system_fixtures == 0, "{skipped}"),
    }
}

/// **Two or more Hebrew points in one cell reach the face in HarfBuzz's
/// order**, its modified combining classes (`[[source:
/// harfbuzz-hb-unicode]]`; the owner's pick, note AB 11.4), so a font's
/// `ccmp` sees what HarfBuzz hands it. Every line of
/// `tests/shape/order.txt` is what hb-shape made of a cluster typed in
/// canonical order and in HarfBuzz's, compared as `script.txt`'s are. On
/// Courier New canonical order put the shin dot after the qamats, its
/// `ccmp` did not form the shin-dot ligature, and the dot drew a cell to
/// the right. The built JetBrains Mono draws the same glyphs at the same
/// offsets in either order, but hands them on in the order the points were
/// sorted, so its lines tell the orders apart too, on every platform, and
/// Courier New's on macOS.
#[test]
fn every_mark_order_fixture_shapes_as_harfbuzz_does() {
    let on_macos = cfg!(target_os = "macos");
    let Tally {
        fixtures: Reached(fixtures),
        hebrew: Reached(hebrew),
        system_fixtures: Reached(system_fixtures),
        system_hebrew: Reached(system_hebrew),
        skipped: Reached(skipped),
        ..
    } = shape_script_fixtures("order.txt", on_macos, GposTable::Kept);
    eprintln!(
        "built faces: {fixtures} order fixtures; system faces: {system_fixtures} shaped, \
         {skipped} skipped"
    );
    // Fourteen clusters in two spellings on each face, every one Hebrew.
    assert!(fixtures == 28 && hebrew == 28, "{fixtures} {hebrew}");
    if on_macos {
        assert!(
            system_fixtures == 28 && system_hebrew == 28,
            "{system_fixtures}"
        );
    } else {
        assert!(skipped == 28 && system_fixtures == 0, "{skipped}");
    }
}

/// **On a face with no `GPOS`, a Hebrew point is placed by the class
/// HarfBuzz's fallback gives it** (`recategorize_combining_class`,
/// `[[source: harfbuzz-hb-ot-shape-fallback]]`; the owner's pick of
/// 2026-10-15): the vowels and meteg below, rafe attached above, shin dot
/// above right, sin dot and holam above left, varika above. Every line of
/// `tests/shape/fallback-hebrew.txt` is what hb-shape made of a cluster on
/// such a face, compared as `script.txt`'s are: the built Regular with its
/// `GPOS` renamed away, on every platform, and on macOS Raanana, New
/// Peninim MT, and Lucida Grande and Corsiva Hebrew regular and bold,
/// which carry none and shape through `morx`. Placed by Unicode's class,
/// every point fell to a centred default with no vertical move, and
/// Raanana's points sat at dy 0 where HarfBuzz puts them at -4.
#[test]
fn every_hebrew_fallback_fixture_places_points_as_harfbuzz_does() {
    let on_macos = cfg!(target_os = "macos");
    let Tally {
        fixtures: Reached(fixtures),
        hebrew: Reached(hebrew),
        raised: Reached(raised),
        system_fixtures: Reached(system_fixtures),
        system_hebrew: Reached(system_hebrew),
        skipped: Reached(skipped),
        ligated: Reached(ligated),
        ..
    } = shape_script_fixtures("fallback-hebrew.txt", on_macos, GposTable::Stripped);
    eprintln!(
        "built faces: {fixtures} fallback fixtures, {raised} glyphs moved vertically; \
         system faces: {system_fixtures} shaped, {ligated} ligated, {skipped} skipped"
    );
    // Thirty-eight clusters on the built Regular, every one Hebrew, and the
    // vertical half of the placer moved some point: the positive control
    // that the classes reached the placer.
    assert!(fixtures == 38 && hebrew == 38, "{fixtures} {hebrew}");
    assert!(raised >= 20, "{raised}");
    // The six `morx` faces: thirty-eight each, and a presentation-form
    // ligature on eighteen of them on every face -- the positive control
    // that the ligature subtables ran. On all but New Peninim MT, which
    // carries the ligatures in both orders, those subtables set coverage
    // bit 30 and not bit 28, "reverse layout order", and form them only
    // when a right-to-left run is walked in logical order
    // (`[[source: apple-ttrm-morx]]`).
    // And on the built Regular, which maps the presentation forms, nineteen
    // letters with a point compose to their form, as HarfBuzz's Hebrew
    // shaper composes them for a face with no mark feature
    // (`[[source: harfbuzz-hb-ot-shaper-hebrew]]`), its GPOS stripped here.
    if on_macos {
        assert!(
            system_fixtures == 228 && system_hebrew == 228,
            "{system_fixtures} {system_hebrew}"
        );
        assert!(ligated == 108 + 19, "{ligated}");
    } else {
        assert!(skipped == 228 && system_fixtures == 0, "{skipped}");
        assert!(ligated == 19, "{ligated}");
    }
}

/// **Hebrew points sit as HarfBuzz sets them on three more macOS faces.**
/// Every line of `tests/shape/hebrew-faces.txt` is a distinct cluster of
/// the pointed Genesis 1:1-5 and Pango corpora (note AB, section 11),
/// shaped by hb-shape and compared as `script.txt`'s are. On Arial Unicode
/// MS, whose `GPOS` has no Script table a Hebrew run selects, baud kept the
/// points' advances in the pen and drew them a cell or two right; on
/// Courier New it ran only the anchors of the `hebr` `mark` feature, which
/// also carries single, pair and chained-context positioning, so two points
/// below one letter stacked where the face spreads them; and Arial Hebrew,
/// which carries both `GSUB` and `morx`, it shaped through `GSUB`, whose
/// `ccmp` draws meteg and every cantillation mark as one empty glyph. All
/// three are system faces, so their lines run on macOS only. Four lines on
/// the built Regular run everywhere: a Latin mark on a Hebrew letter, which
/// no lookup places, sits where the pen leaves it, and right to left that
/// is its letter's left edge -- baud's pen ran left to right inside a
/// cluster and put it a cell right. And on each of the four built faces,
/// everywhere, all 215 clusters: the upright, bold and italic Hebrew, its
/// points placed as HarfBuzz places them. 99 of them carry a cantillation
/// mark, which the built faces draw: above or below its letter, and beside
/// a point below it by the mark-to-mark lookups. And on each, the characters baud draws itself
/// (`baud-app/fonts/glyphs/languages/hebrew/build/cover.py`), alone and
/// carrying a point.
#[test]
fn every_hebrew_face_fixture_shapes_as_harfbuzz_does() {
    let on_macos = cfg!(target_os = "macos");
    let Tally {
        fixtures: Reached(fixtures),
        hebrew: Reached(hebrew),
        system_fixtures: Reached(system_fixtures),
        system_hebrew: Reached(system_hebrew),
        system_moved: Reached(system_moved),
        skipped: Reached(skipped),
        ..
    } = shape_script_fixtures("hebrew-faces.txt", on_macos, GposTable::Kept);
    eprintln!(
        "built faces: {fixtures} shaped; system faces: {system_fixtures} shaped, \
         {system_moved} glyphs offset, {skipped} skipped"
    );
    // 215 clusters and the 140 lines of what baud draws itself on each of
    // the four, the 4 Latin-mark lines on the Regular; every one Hebrew but
    // the sheqel sign's 4.
    assert!(fixtures == 1424 && hebrew == 1420, "{fixtures} {hebrew}");
    // The cantillated clusters, 99 lines on each of the four built faces:
    // since the faces draw the marks, no line's hb-shape output holds a
    // `.notdef`, and the positive control that they were compared is the
    // count of lines whose input carries a cantillation mark.
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/shape/hebrew-faces.txt"
    ))
    .unwrap();
    let notdef = text
        .lines()
        .filter(|l| l.starts_with("baud-app/"))
        .filter(|l| {
            let shaped = l.rsplit('\t').next().unwrap();
            shaped.starts_with("[0=") || shaped.contains("|0=")
        })
        .count();
    assert!(notdef == 0, "{notdef}");
    let cantillated = text
        .lines()
        .filter(|l| l.starts_with("baud-app/"))
        .filter(|l| {
            l.split('\t').nth(3).unwrap().split_whitespace().any(|h| {
                let c = u32::from_str_radix(h, 16).unwrap();
                (0x0591..=0x05AF).contains(&c)
            })
        })
        .count();
    assert!(cantillated == 4 * (99 + 46), "{cantillated}");
    // 215 clusters on each of the three faces, every one Hebrew, and the
    // positive control that some point was moved off its pen.
    if on_macos {
        assert!(
            system_fixtures == 645 && system_hebrew == 645,
            "{system_fixtures} {system_hebrew}"
        );
        assert!(system_moved >= 300, "{system_moved}");
    } else {
        assert!(skipped == 645 && system_fixtures == 0, "{skipped}");
    }
}

/// **Meteg beside its vowel shapes as HarfBuzz shapes it, on every built
/// face.** Every line of `tests/shape/meteg.txt` is a letter, a vowel meteg
/// may follow, and meteg, spelled for each side the Unicode Standard
/// (`[[source: unicode-ch9]]`, 9.1) gives: <vowel, meteg>, left of the
/// vowel, and <meteg, CGJ, vowel>, right of it; and on the Regular and the
/// Bold, on four letters, with a dagesh after the vowel and before it, which
/// the lookup's mark filtering set steps over. Where the vowel's own place
/// leaves meteg out of the cell, the faces' chained context places the vowel
/// and meteg again by further sets of anchors, after meteg's mark-to-mark:
/// it places meteg itself, from its letter, because baud places an attached
/// mark once, from its target's place at that moment, where HarfBuzz adds
/// the target's offset at the end, so a vowel moved under a meteg already
/// attached to it would carry meteg in HarfBuzz alone.
#[test]
fn every_meteg_fixture_shapes_as_harfbuzz_does() {
    let Tally {
        fixtures: Reached(fixtures),
        hebrew: Reached(hebrew),
        moved: Reached(moved),
        ..
    } = shape_script_fixtures("meteg.txt", cfg!(target_os = "macos"), GposTable::Kept);
    // 27 letters by 11 vowels by 2 spellings on four faces, and 4 letters
    // by 11 vowels by 2 more spellings on two.
    assert!(fixtures == 2552 && hebrew == 2552, "{fixtures} {hebrew}");
    // Positive control: every line places its points, so each moves at
    // least its vowel and its meteg off the base.
    assert!(moved >= 2 * fixtures, "{moved}");
}

/// A glyph's ink box: the extremes of the points its outline draws through,
/// control points included -- exact for meteg, a bar, and enough to say
/// which side of a vowel its centre lies.
#[derive(Default)]
struct InkBox {
    b: Option<(f32, f32, f32, f32)>,
}

impl InkBox {
    fn put(&mut self, x: f32, y: f32) {
        let (x0, y0, x1, y1) = self.b.unwrap_or((x, y, x, y));
        self.b = Some((x0.min(x), y0.min(y), x1.max(x), y1.max(y)));
    }
}

impl Sink for InkBox {
    fn move_to(&mut self, x: f32, y: f32) {
        self.put(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.put(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.put(x1, y1);
        self.put(x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.put(x1, y1);
        self.put(x2, y2);
        self.put(x, y);
    }
    fn close(&mut self) {}
}

/// **Meteg stays in its letter's cell, on the side its text order gives.**
/// The Unicode Standard (`[[source: unicode-ch9]]`, 9.1): "Typically, meteg
/// is placed to the left of the vowel", <vowel, meteg>, and "to display
/// meteg to the right of the sheva ... <meteg, CGJ, sheva>". On each built
/// face, every letter with every vowel meteg may follow, in both spellings,
/// shaped by baud: meteg's centre must lie on its side of the vowel's, and
/// its ink inside x 0..600 -- except where the faces' rule
/// (`baud-app/fonts/glyphs/languages/hebrew/build/meteg.py`) finds no place
/// in the cell on that side: left of the vowel on a final letter, whose
/// vowel stands beside its descender, and right of it on qof, whose vowel
/// stands right of its leg. Before the faces' chained context, hataf segol
/// and qubuts put meteg past the left edge on every letter, and <meteg,
/// CGJ, vowel> drew meteg on the vowel.
///
/// It holds meteg's side and its box inside x 0..600 only. The 20-unit
/// clearance to the vowel and the letter, part of the round 3 check whose
/// counts FONTLOG quotes, is asserted by no test; the counts this one pins
/// equal the check's because no shipped cluster passes the box and fails
/// the clearance.
#[test]
fn meteg_stays_in_its_cell_on_the_side_its_text_order_gives() {
    const VOWELS: [u32; 11] = [
        0x05B0, 0x05B1, 0x05B2, 0x05B3, 0x05B4, 0x05B5, 0x05B6, 0x05B7, 0x05B8, 0x05BB, 0x05C7,
    ];
    const FINALS: [u32; 4] = [0x05DA, 0x05DF, 0x05E3, 0x05E5];
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../baud-app/fonts/built");
    let mut wrong = Vec::new();
    let mut inside = Vec::new();
    for style in ["Regular", "Bold", "Italic", "BoldItalic"] {
        let data = std::fs::read(format!("{root}/{style}.ttf")).unwrap();
        let face = Face::parse(&data).unwrap();
        let ink = |g: GlyphId, dx: i16, dy: i16| {
            let mut b = InkBox::default();
            face.outline(g, &mut b);
            let (x0, y0, x1, y1) = b.b.unwrap();
            let (dx, dy) = (f32::from(dx), f32::from(dy));
            (x0 + dx, y0 + dy, x1 + dx, y1 + dy)
        };
        let gid = |c: u32| face.glyph(Codepoint::new(c).unwrap()).unwrap();
        let meteg = gid(0x05BD);
        let (mut left_in, mut right_in) = (0usize, 0usize);
        for letter in 0x05D0..=0x05EAu32 {
            for v in VOWELS {
                for right in [false, true] {
                    let text: &[u32] = if right {
                        &[letter, 0x05BD, 0x034F, v]
                    } else {
                        &[letter, v, 0x05BD]
                    };
                    let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
                    let (buf, _) = shape(&face, text, &mut storage);
                    let find = |id: GlyphId| buf.as_slice().iter().find(|g| g.id == id).unwrap();
                    let (m, vg) = (find(meteg), find(gid(v)));
                    let mb = ink(meteg, m.dx, m.dy);
                    let vb = ink(vg.id, vg.dx, vg.dy);
                    let (mc, vc) = ((mb.0 + mb.2) / 2.0, (vb.0 + vb.2) / 2.0);
                    if (right && mc <= vc) || (!right && mc >= vc) {
                        wrong.push(format!("{style} {text:04X?}: meteg {mb:?} vowel {vb:?}"));
                    }
                    let fits = mb.0 >= 0.0 && mb.2 <= 600.0;
                    let no_place = if right {
                        letter == 0x05E7
                    } else {
                        FINALS.contains(&letter)
                    };
                    if !fits && !no_place {
                        wrong.push(format!("{style} {text:04X?}: meteg {mb:?} out of the cell"));
                    }
                    if fits {
                        *if right { &mut right_in } else { &mut left_in } += 1;
                    }
                }
            }
        }
        inside.push((style, left_in, right_in));
    }
    assert!(
        wrong.is_empty(),
        "{} clusters:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    // Of 297 on each side: the clusters whose meteg is inside the cell,
    // measured; the rest are the cases above.
    assert_eq!(
        inside,
        [
            ("Regular", 274, 291),
            ("Bold", 265, 291),
            ("Italic", 262, 293),
            ("BoldItalic", 259, 291)
        ]
    );
}

/// **A Hebrew run whose `GPOS` selects no Script table is placed as on a
/// face with no `GPOS`** -- HarfBuzz's Hebrew shaper takes `GPOS` only
/// from `hebr` (`gpos_tag`, `[[source: harfbuzz-hb-ot-shaper-hebrew]]`), and
/// baud now does too where nothing else is selected (Jev, 0.97 and 1.00).
/// The built Regular with every `GPOS` Script table renamed away keeps its
/// table and its lookups, and must shape each line of
/// `tests/shape/fallback-hebrew.txt` exactly as the face with no `GPOS` at
/// all does: the points zeroed and placed by the fallback, on every
/// platform. Kept in the pen, they sat a cell right of their letters.
#[test]
fn a_gpos_selecting_no_table_places_hebrew_as_no_gpos_does() {
    let Tally {
        fixtures: Reached(fixtures),
        hebrew: Reached(hebrew),
        raised: Reached(raised),
        ..
    } = shape_script_fixtures("fallback-hebrew.txt", false, GposTable::Unselected);
    assert!(fixtures == 38 && hebrew == 38, "{fixtures} {hebrew}");
    assert!(raised >= 20, "{raised}");
}

/// What [`shape_script_fixtures`] compared: lines on the built faces, how
/// many of them Hebrew, how many glyphs they offset and how many of those
/// vertically; lines on system faces, how many of them Hebrew and how many
/// glyphs they offset, and how many were skipped where the
/// face is not; and how many lines on either came out in fewer glyphs than
/// codepoints.
struct Tally {
    fixtures: Reached,
    hebrew: Reached,
    moved: Reached,
    raised: Reached,
    system_fixtures: Reached,
    system_hebrew: Reached,
    system_moved: Reached,
    skipped: Reached,
    ligated: Reached,
}

/// How many fixture lines, or glyphs, one comparison reached.
struct Reached(usize);

/// Shapes every line of `tests/shape/<name>` and requires hb-shape's
/// glyphs and offsets, in the columns `scripts/gen-shape-fixtures.sh`
/// writes for `script.txt`; a system face's lines are skipped unless
/// `on_macos`. With [`GposTable::Stripped`] a repository face has its
/// `GPOS` renamed away after its digest is checked, as the generator did,
/// and every face compared must then have none. Every line is compared
/// before any mismatch fails, and the failure lists them all.
fn shape_script_fixtures(name: &str, on_macos: bool, gpos: GposTable) -> Tally {
    let text =
        std::fs::read_to_string(format!("{}/tests/shape/{name}", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let mut files = std::collections::HashMap::new();
    let (mut fixtures, mut hebrew, mut moved, mut raised) = (0usize, 0usize, 0usize, 0usize);
    let mut mismatches: Vec<String> = Vec::new();
    let (mut system_fixtures, mut system_hebrew, mut skipped) = (0usize, 0usize, 0usize);
    let mut system_moved = 0usize;
    let mut ligated = 0usize;
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        let (path, index, digest, cps, shaped) = (f[0], f[1], f[2], f[3], f[4]);
        let system = path.starts_with('/');
        if system && !on_macos {
            skipped += 1;
            continue;
        }
        let data = files.entry(path.to_string()).or_insert_with(|| {
            let full = if system {
                path.to_string()
            } else {
                format!("{root}/{path}")
            };
            let mut d = std::fs::read(&full).unwrap_or_else(|e| panic!("{full}: {e}"));
            assert_eq!(
                format!("{:016x}", fnv64(&d)),
                digest,
                "{path} is not the file the fixtures were shaped with; rerun \
                 scripts/gen-shape-fixtures.sh under nix-shell -p harfbuzz"
            );
            if gpos == GposTable::Stripped && !system {
                assert_eq!(strip_gpos(&mut d), 1, "{path}: no GPOS to strip");
            }
            if gpos == GposTable::Unselected && !system {
                unselect_gpos(&mut d);
            }
            d
        });
        let face = Face::parse_index(data, FaceIndex::of(index.parse().unwrap())).unwrap();
        if gpos == GposTable::Stripped {
            assert!(face.gpos().is_none(), "{path} {index} has a GPOS");
        }
        if gpos == GposTable::Unselected {
            let hebrew = Script::of(Codepoint::from_char('א'));
            let g = face.gpos().expect("the GPOS is kept");
            assert!(
                g.chosen(hebrew).is_none(),
                "{path}: Hebrew still selects a table"
            );
            assert!(
                !g.names_mark(hebrew),
                "{path}: a mark feature is still named"
            );
        }
        let cps: Vec<u32> = cps
            .split_whitespace()
            .map(|h| u32::from_str_radix(h, 16).unwrap())
            .collect();
        let rtl = cps
            .iter()
            .any(|c| (0x0590..=0x05FF).contains(c) || (0xFB1D..=0xFB4F).contains(c));
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 128];
        let (buf, out) = shape(&face, &cps, &mut storage);
        assert!(out.declined.is_zero(), "{path} {cps:04X?}");
        // hb-shape's glyphs with their pen positions, in visual order, then
        // in logical order; each glyph's offset from its cluster's first
        // glyph's pen, the base, which baud puts at its cell's origin. A
        // cluster holding a glyph that advances besides its base -- the
        // `.notdef` of a mark the face does not map -- has its left edge
        // there in a right-to-left run, a cell left of the base, and the
        // cell keeps the base; every other glyph's offset from the base is
        // the same as from the left edge, and baud's are not shifted to
        // match, so a whole cluster a cell off is still a mismatch.
        let expected = parse_positioned(shaped);
        let mut placed: Vec<(&HbGlyph, i32)> = Vec::new();
        let mut pen = 0i32;
        for e in &expected {
            placed.push((e, pen));
            pen += e.ax;
        }
        if rtl {
            placed.reverse();
        }
        let mut base = std::collections::HashMap::new();
        for &(e, pen) in &placed {
            base.entry(e.cluster).or_insert(pen);
        }
        let want: Vec<(u16, u16, i32, i32)> = placed
            .iter()
            .map(|&(e, pen)| (e.id, e.cluster, pen + e.dx - base[&e.cluster], e.dy))
            .collect();
        let got: Vec<(u16, u16, i32, i32)> = buf
            .as_slice()
            .iter()
            .map(|g| {
                (
                    u16::try_from(g.id.index()).unwrap(),
                    g.cluster.0,
                    i32::from(g.dx),
                    i32::from(g.dy),
                )
            })
            .collect();
        if got != want {
            mismatches.push(format!(
                "{path} {cps:04X?}: baud {got:?} against hb-shape {want:?}"
            ));
        }
        ligated += usize::from(got.len() < cps.len());
        if system {
            system_fixtures += 1;
            system_hebrew += usize::from(rtl);
            system_moved += want.iter().filter(|w| w.2 != 0 || w.3 != 0).count();
        } else {
            fixtures += 1;
            hebrew += usize::from(rtl);
            moved += want.iter().filter(|w| w.2 != 0 || w.3 != 0).count();
            raised += want.iter().filter(|w| w.3 != 0).count();
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {name}'s lines differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    Tally {
        fixtures: Reached(fixtures),
        hebrew: Reached(hebrew),
        moved: Reached(moved),
        raised: Reached(raised),
        system_fixtures: Reached(system_fixtures),
        system_hebrew: Reached(system_hebrew),
        system_moved: Reached(system_moved),
        skipped: Reached(skipped),
        ligated: Reached(ligated),
    }
}

/// **A bracket pair is in the run of the text enclosing it, and each run
/// shapes as hb-shape shapes it** (`[[source: uax24]]` 5.1, "paired
/// punctuation ... should therefore match the script of the enclosing
/// text"). Every line of `tests/shape/brackets.txt` is written as the runs
/// UAX #24 gives it, each with its script, and hb-shape shaped one run at a
/// time with that script given; hb-shape itemises nothing, so the runs are
/// the file's. baud's resolver must settle every cell of a run to the run's
/// script, which it did not for a closer before pairs were matched -- it
/// took the script inside the pair -- and each run, shaped in that script,
/// must be hb-shape's glyph for glyph. On STIX Two Math, whose `latn`,
/// `grek` and `cyrl` select different lookups, every cut is a run cut.
#[test]
fn every_bracket_run_fixture_settles_and_shapes_as_harfbuzz_does() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/shape/brackets.txt"
    ))
    .unwrap();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let by_code = |c: &str| {
        Script::all()
            .find(|s| s.code() == c.as_bytes())
            .unwrap_or_else(|| panic!("{c}: no such script"))
    };
    let mut mismatches: Vec<String> = Vec::new();
    let (mut lines, mut runs, mut brackets) = (0usize, 0usize, 0usize);
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        let (path, digest, cps, scripts, shaped) = (f[0], f[2], f[3], f[4], f[5]);
        let data = std::fs::read(format!("{root}/{path}")).unwrap();
        assert_eq!(format!("{:016x}", fnv64(&data)), digest, "{path}");
        let face = Face::parse(&data).unwrap();
        let run_cps: Vec<Vec<u32>> = cps
            .split('|')
            .map(|r| {
                r.split_whitespace()
                    .map(|h| u32::from_str_radix(h, 16).unwrap())
                    .collect()
            })
            .collect();
        let run_scripts: Vec<Script> = scripts.split(' ').map(by_code).collect();
        let outputs: Vec<&str> = shaped.split(' ').collect();
        assert!(run_cps.len() == run_scripts.len() && run_cps.len() == outputs.len());
        // The itemisation: one cluster a code point, settled as a row is.
        let all: Vec<Codepoint> = run_cps
            .iter()
            .flatten()
            .map(|&v| Codepoint::new(v).unwrap())
            .collect();
        let cells: Vec<Unresolved> = all.iter().map(|c| Unresolved::of(&[*c])).collect();
        let mut settled = vec![Script::COMMON; cells.len()];
        resolve(&cells, &mut settled);
        let want: Vec<Script> = run_cps
            .iter()
            .zip(&run_scripts)
            .flat_map(|(r, &s)| std::iter::repeat_n(s, r.len()))
            .collect();
        if settled != want {
            mismatches.push(format!("{cps}: settled {settled:?}, runs {want:?}"));
        }
        brackets += all
            .iter()
            .filter(|c| !matches!(baud_font::Bracket::of(**c), baud_font::Bracket::None))
            .count();
        // Each run, shaped in its script, against hb-shape's.
        for ((r, &script), out) in run_cps.iter().zip(&run_scripts).zip(&outputs) {
            let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 64];
            let mut shaper = Shaper::new(&face, &mut storage);
            for (i, &v) in r.iter().enumerate() {
                shaper
                    .push_cluster(
                        &[Codepoint::new(v).unwrap()],
                        Cluster(u16::try_from(i).unwrap()),
                    )
                    .unwrap();
            }
            let (buf, done) = shaper.finish(Features::GRID, script);
            assert!(done.declined.is_zero(), "{cps}");
            let mut pen = 0i32;
            let mut left = std::collections::HashMap::new();
            let placed: Vec<(HbGlyph, i32)> = parse_positioned(out)
                .into_iter()
                .map(|e| {
                    let at = pen;
                    pen += e.ax;
                    let l = left.entry(e.cluster).or_insert(at);
                    *l = (*l).min(at);
                    (e, at)
                })
                .collect();
            let hb: Vec<(u16, u16, i32, i32)> = placed
                .iter()
                .map(|(e, at)| (e.id, e.cluster, at + e.dx - left[&e.cluster], e.dy))
                .collect();
            let got: Vec<(u16, u16, i32, i32)> = buf
                .as_slice()
                .iter()
                .map(|g| {
                    (
                        u16::try_from(g.id.index()).unwrap(),
                        g.cluster.0,
                        i32::from(g.dx),
                        i32::from(g.dy),
                    )
                })
                .collect();
            if got != hb {
                mismatches.push(format!("{cps} run {r:04X?}: baud {got:?}, hb-shape {hb:?}"));
            }
            runs += 1;
        }
        lines += 1;
    }
    // Positive controls: every line and run was reached, and the lines
    // hold brackets.
    assert!(
        lines == 8 && runs == 25 && brackets == 19,
        "{lines} {runs} {brackets}"
    );
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
