//! **Devanagari shaped as HarfBuzz shapes it**, through baud's Indic engine
//! (`baud-font/src/indic.rs`).
//!
//! `tests/shape/devanagari.txt` is what hb-shape 14.5.1 made of each line:
//! the README's line, the examples of Microsoft's Devanagari specification
//! (`[[source: ms-devanagari-script-development]]`) and every word of two
//! pages of Premchand (`[[source: premchand-saptasaroj-11]]`,
//! `[[source: premchand-saptasaroj-12]]`), on Noto Sans Devanagari, on the
//! macOS faces Kohinoor Devanagari, ITF Devanagari, Shree Devanagari 714 and
//! Arial Unicode MS (the last two the old specification's `deva`), and on
//! the built Regular, which has no Devanagari, as the control. Noto Sans
//! Devanagari is not in the repository: it is pinned by tag and sha256 and
//! read from the cache `scripts/gen-shape-fixtures.sh` fetches it into,
//! the one `scripts/readme-capture.sh` keeps its Noto faces in
//! (`NOTO_CACHE`, else `$XDG_CACHE_HOME/baud-readme`, else
//! `~/.cache/baud-readme`), and its lines are left out where the cache
//! does not hold it, as the macOS faces' are off macOS. Each line is cut
//! into the cells the core's cluster machine makes ([`ClusterBreak`]: a
//! conjunct is one cell, by UAX #29's GB9c), shaped as one run, and every
//! glyph's id, cell and offset from its cell's left edge must be
//! hb-shape's, its cluster -- a code point index -- named by the cell
//! holding it.

use baud_domain::{ClusterBreak, Codepoint, GlyphId};
use baud_font::{
    Cluster, Digest, Face, FaceIndex, Features, Glyph, Ignorable, MAX_LOOKUPS, Script, Shaper,
    Unresolved, resolve,
};

/// A code point index, hb-shape's cluster.
#[derive(Clone, Copy)]
struct CodeIndex(usize);

struct HbGlyph {
    id: GlyphId,
    cluster: CodeIndex,
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
                id: GlyphId::from_shaper(id.parse().unwrap()),
                cluster: CodeIndex(cluster.parse().unwrap()),
                dx,
                dy,
                ax: ax.parse().unwrap(),
            }
        })
        .collect()
}

/// The file's FNV-1a digest, as the generator writes it.
fn fnv64(data: &[u8]) -> String {
    let h = data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{h:016x}")
}

/// The cells the core's cluster machine makes of `cps`.
fn cells(cps: &[Codepoint]) -> Vec<core::ops::Range<usize>> {
    let mut machine = ClusterBreak::new();
    let mut out: Vec<core::ops::Range<usize>> = Vec::new();
    for (i, &c) in cps.iter().enumerate() {
        let brk = machine.is_break(c);
        match out.last_mut() {
            Some(last) if !brk => last.end = i + 1,
            _ => out.push(i..i + 1),
        }
    }
    out
}

/// `cps` shaped as one run on `face`, cell by cell: each glyph's id, cell
/// and offset from its cell's origin. Shaped twice, with no lookup digests
/// and with the digests a renderer keeps for the face (`Gsub::digests` of
/// the joining features, as `baud-render` fills them), which must agree:
/// the Indic stages run lookups those features do not name.
fn shaped(face: &Face<'_>, cps: &[Codepoint]) -> Vec<(GlyphId, usize, i32, i32)> {
    let plain = shaped_with(face, cps, &[]);
    let mut digests = vec![Digest::EMPTY; MAX_LOOKUPS];
    if let Some(gsub) = face.gsub() {
        gsub.digests(Features::JOINING, &mut digests);
    }
    let accelerated = shaped_with(face, cps, &digests);
    assert_eq!(
        plain, accelerated,
        "{cps:04X?}: the lookup digests change the shaping"
    );
    plain
}

fn shaped_with(
    face: &Face<'_>,
    cps: &[Codepoint],
    digests: &[Digest],
) -> Vec<(GlyphId, usize, i32, i32)> {
    let cells = cells(cps);
    let unresolved: Vec<Unresolved> = cells
        .iter()
        .map(|r| Unresolved::of(&cps[r.clone()]))
        .collect();
    let mut settled = vec![Script::COMMON; cells.len()];
    resolve(&unresolved, &mut settled);
    let script = settled[0];
    assert!(
        settled.iter().all(|&s| s == script),
        "{cps:04X?} settles to more than one script: {settled:?}"
    );
    let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 256];
    let mut shaper = Shaper::new(face, &mut storage);
    shaper.accelerate(digests);
    for (k, r) in cells.iter().enumerate() {
        shaper
            .push_cluster(&cps[r.clone()], Cluster(u16::try_from(k).unwrap()))
            .unwrap();
    }
    let (buf, done) = shaper.finish(Features::GRID, script);
    assert!(done.declined.is_zero(), "{cps:04X?}: lookups declined");
    buf.as_slice()
        .iter()
        .map(|g| {
            (
                g.id,
                usize::from(g.cluster.0),
                i32::from(g.dx),
                i32::from(g.dy),
            )
        })
        .collect()
}

/// hb-shape's glyphs for `cps`, each named by the cell its cluster falls
/// in and offset from that cell's left edge, the pen where its first glyph
/// starts.
fn expected(cps: &[Codepoint], body: &str) -> Vec<(GlyphId, usize, i32, i32)> {
    let starts: Vec<usize> = cells(cps).iter().map(|r| r.start).collect();
    let cell_of = |c: CodeIndex| starts.iter().rposition(|&s| s <= c.0).unwrap();
    let hb = parse_positioned(body);
    let mut placed = Vec::new();
    let mut pen = 0i32;
    for e in &hb {
        placed.push((e, cell_of(e.cluster), pen));
        pen += e.ax;
    }
    let mut left = std::collections::BTreeMap::new();
    for &(_, cell, pen) in &placed {
        let l = left.entry(cell).or_insert(pen);
        *l = (*l).min(pen);
    }
    placed
        .iter()
        .map(|&(e, cell, pen)| (e.id, cell, pen + e.dx - left[&cell], e.dy))
        .collect()
}

/// What the fixture lines reached: the lines each face compared, and
/// [`Counts`].
#[derive(Default, Debug)]
struct Reached {
    lines_by_face: std::collections::BTreeMap<String, usize>,
    counts: Counts,
}

/// Lines compared, lines that reorder and lines that draw a dotted circle.
#[derive(Default, Debug)]
struct Counts(usize, usize, usize);

/// Where `noto-cache/` in the path column points: the cache the README
/// capture fetches its pinned Noto faces into, if there is one.
fn noto_cache() -> Option<std::path::PathBuf> {
    if let Some(d) = std::env::var_os("NOTO_CACHE") {
        return Some(d.into());
    }
    if let Some(d) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(std::path::PathBuf::from(d).join("baud-readme"));
    }
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/baud-readme"))
}

/// The bytes of the face a path column names, or `None` for a face this
/// machine does not have: a macOS face off macOS, or a cached face not in
/// the cache.
fn face_file(path: &str) -> Option<Vec<u8>> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    if let Some(name) = path.strip_prefix("noto-cache/") {
        return std::fs::read(noto_cache()?.join(name)).ok();
    }
    if path.starts_with('/') {
        return cfg!(target_os = "macos")
            .then(|| std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}")));
    }
    Some(std::fs::read(format!("{root}/{path}")).unwrap_or_else(|e| panic!("{path}: {e}")))
}

fn devanagari_lines() -> Reached {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/shape/devanagari.txt"
    ))
    .unwrap();
    let mut files: std::collections::HashMap<String, Option<Vec<u8>>> = Default::default();
    let mut mismatches = Vec::new();
    let mut reached = Reached::default();
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        let (path, index, digest, cps, body) = (f[0], f[1], f[2], f[4], f[5]);
        let data = files.entry(path.to_string()).or_insert_with(|| {
            let d = face_file(path)?;
            assert_eq!(
                fnv64(&d),
                digest,
                "{path} is not the file the fixtures were shaped with"
            );
            Some(d)
        });
        let Some(data) = data else {
            continue;
        };
        let face = Face::parse_index(data, FaceIndex::of(index.parse().unwrap())).unwrap();
        let cps: Vec<Codepoint> = cps
            .split_whitespace()
            .map(|h| Codepoint::new(u32::from_str_radix(h, 16).unwrap()).unwrap())
            .collect();
        let got = shaped(&face, &cps);
        let want = expected(&cps, body);
        if got != want {
            mismatches.push(format!(
                "{path} {cps:04X?}:\n  baud    {got:?}\n  hb-shape {want:?}"
            ));
            continue;
        }
        reached.counts.0 += 1;
        *reached.lines_by_face.entry(path.to_string()).or_default() += 1;
        // A glyph that sits before a glyph of an earlier code point: the
        // reordering is what the line exercises.
        let hb = parse_positioned(body);
        reached.counts.1 += usize::from(
            hb.windows(2).any(|w| w[1].cluster.0 < w[0].cluster.0)
                || cps.windows(2).any(|w| w[1].value() == 0x093F),
        );
        let dotted = face.glyph(Codepoint::new(0x25CC).unwrap());
        reached.counts.2 += usize::from(dotted.is_some_and(|d| got.iter().any(|g| g.0 == d)));
    }
    assert!(
        mismatches.is_empty(),
        "{} of devanagari.txt's lines differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    reached
}

/// **Every line of `devanagari.txt` shapes as hb-shape shaped it.** The
/// macOS faces' lines run on macOS only, Noto Sans Devanagari's where the
/// cache holds it, and the control's everywhere. On macOS every face's
/// lines run, the reordering and the dotted circles among them.
#[test]
fn every_devanagari_fixture_shapes_as_harfbuzz_does() {
    let r = devanagari_lines();
    eprintln!("{r:?}");
    let control = r
        .lines_by_face
        .get("baud-app/fonts/built/Regular.ttf")
        .copied()
        .unwrap_or(0);
    assert!(control >= 390, "{control} control lines");
    if cfg!(target_os = "macos") {
        assert!(r.lines_by_face.len() >= 5, "{r:?}");
        assert!(r.counts.1 >= 300 && r.counts.2 >= 40, "{r:?}");
    }
    if let Some(n) = r
        .lines_by_face
        .get("noto-cache/NotoSansDevanagari-Regular.ttf")
    {
        assert!(*n >= 390, "{n} Noto lines");
    }
}

/// **A nukta letter comes apart for `nukt`, which forms it again**: "The
/// input context for the nukt feature always consists of the full form of
/// the consonant" (`[[source: ms-devanagari-script-development]]`, Nukta),
/// and HarfBuzz's Indic normalisation takes U+0958 QA apart into KA and
/// NUKTA though the face maps it (`[[source: harfbuzz-14-5-1-shape-normalize]]`,
/// never the short path). U+0929 NNNA composes again, as Unicode composes
/// it; U+0931 RRA stays whole; a Latin letter and its mark keep the
/// face's precomposed letter. Every Devanagari face read draws the nukta
/// letter `nukt` forms with the glyph its `cmap` gives the precomposed one,
/// so the fixtures cannot tell the two paths apart: this holds the path,
/// on Kohinoor Devanagari, macOS's own face.
#[cfg(target_os = "macos")]
#[test]
fn a_nukta_letter_comes_apart_for_nukt() {
    let data = std::fs::read("/System/Library/Fonts/Kohinoor.ttc").unwrap();
    let face = Face::parse_index(&data, FaceIndex::of(0u32)).unwrap();
    let cp = |v: u32| Codepoint::new(v).unwrap();
    let norm = |v: &[u32]| -> Vec<u32> {
        let cps: Vec<Codepoint> = v.iter().map(|&c| cp(c)).collect();
        baud_font::Normalised::of(&face, &cps)
            .as_slice()
            .iter()
            .map(|c| c.value())
            .collect()
    };
    assert!(face.glyph(cp(0x0958)).is_some(), "the face maps QA");
    assert_eq!(norm(&[0x0958]), [0x0915, 0x093C]);
    assert_eq!(
        norm(&[0x0958, 0x094D, 0x0937]),
        [0x0915, 0x093C, 0x094D, 0x0937]
    );
    assert_eq!(norm(&[0x0928, 0x093C]), [0x0929]);
    assert_eq!(norm(&[0x0929]), [0x0929]);
    assert_eq!(norm(&[0x0931]), [0x0931]);
}

/// **`indic_data.rs` is what its generator makes of the pinned files**:
/// IndicSyllabicCategory.txt, IndicPositionalCategory.txt and Blocks.txt,
/// through HarfBuzz's mapping (`scripts/gen-indic-tables.py`).
#[test]
fn indic_tables_match_the_pinned_data() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-indic-tables.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "indic_data.rs is stale -- rerun scripts/gen-indic-tables.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// **Each Devanagari character takes the class and position the pinned
/// files give it**, read here from the files themselves for the block
/// U+0900..U+097F, so a table that indexed wrongly could not agree with
/// the generator that built it: a consonant is a consonant at the base,
/// RA is RA, the virama a halant below, VOWEL SIGN I a matra before the
/// half forms ("093F BeforeHalf", the specification's Reorder Class), the
/// other matras after the subjoined consonants ("AfterSubscript"), and
/// ZWJ and ZWNJ themselves.
#[test]
fn devanagari_characters_take_their_indic_classes() {
    use baud_font::{Category, Position, Syllabic};
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources");
    let syllabic =
        std::fs::read_to_string(format!("{dir}/unicode-indicsyllabiccategory.txt")).unwrap();
    let mut consonants = 0usize;
    for line in syllabic.lines() {
        let body = line.split('#').next().unwrap();
        let f: Vec<&str> = body.split(';').map(str::trim).collect();
        if f.len() < 2 || f[1] != "Consonant" {
            continue;
        }
        let (lo, hi) = match f[0].split_once("..") {
            Some((a, b)) => (
                u32::from_str_radix(a, 16).unwrap(),
                u32::from_str_radix(b, 16).unwrap(),
            ),
            None => {
                let v = u32::from_str_radix(f[0], 16).unwrap();
                (v, v)
            }
        };
        for v in (lo..=hi).filter(|v| matches!(v, 0x0900..=0x097F)) {
            let s = Syllabic::of(Codepoint::new(v).unwrap());
            let want = if v == 0x0930 {
                Category::RA
            } else {
                Category::C
            };
            assert_eq!(s.category, want, "U+{v:04X}");
            assert_eq!(s.position, Position::BASE_C, "U+{v:04X}");
            consonants += 1;
        }
    }
    assert!(consonants >= 40, "{consonants} consonants");
    let of = |v: u32| Syllabic::of(Codepoint::new(v).unwrap());
    assert_eq!(
        (of(0x094D).category, of(0x094D).position),
        (Category::H, Position::BELOW_C)
    );
    assert_eq!(
        (of(0x093F).category, of(0x093F).position),
        (Category::M, Position::PRE_M)
    );
    for m in [0x093E, 0x0940, 0x0941, 0x0947, 0x0948, 0x094B] {
        assert_eq!(
            (of(m).category, of(m).position),
            (Category::M, Position::AFTER_SUB),
            "U+{m:04X}"
        );
    }
    assert_eq!(of(0x200D).category, Category::ZWJ);
    assert_eq!(of(0x200C).category, Category::ZWNJ);
    assert_eq!(of(0x25CC).category, Category::DOTTED_CIRCLE);
    assert_eq!(of(0x0041).category, Category::X);
}
