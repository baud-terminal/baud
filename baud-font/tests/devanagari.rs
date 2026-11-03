//! **Devanagari shaped as HarfBuzz shapes it**, through baud's Indic engine
//! (`baud-font/src/indic.rs`).
//!
//! `tests/shape/devanagari.txt` is what hb-shape 14.5.1 made of each line:
//! the README's line, the examples of Microsoft's Devanagari specification
//! (`[[source: ms-devanagari-script-development]]`) and every word of two
//! pages of Premchand (`[[source: premchand-saptasaroj-11]]`,
//! `[[source: premchand-saptasaroj-12]]`), on Noto Sans Devanagari, on the
//! macOS faces Kohinoor Devanagari, ITF Devanagari, Shree Devanagari 714 and
//! Arial Unicode MS (the last two the old specification's `deva`), on the
//! built Regular and Bold, which carry Noto Sans Devanagari merged in
//! (`scripts/build-fonts.sh`), and on stock JetBrains Mono Regular, which has
//! no Devanagari, as the control. Noto Sans Devanagari's own file is not in
//! the repository: it is pinned by tag and sha256 and
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

mod sha256;

use baud_domain::{ClusterBreak, ClusterCells, Codepoint, GlyphId, GraphemeClass};
use baud_font::{
    Cluster, ClusterLevel, Digest, Face, FaceIndex, Features, Glyph, Ignorable, MAX_LOOKUPS, Pens,
    Script, Shaper, Unresolved, resolve,
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
fn shaped(face: &Face<'_>, cps: &[Codepoint], pens: Pens) -> Vec<(GlyphId, usize, i32, i32)> {
    let plain = shaped_with(face, cps, &[], pens);
    let mut digests = vec![Digest::EMPTY; MAX_LOOKUPS];
    if let Some(gsub) = face.gsub() {
        gsub.digests(Features::JOINING, &mut digests);
    }
    let accelerated = shaped_with(face, cps, &digests, pens);
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
    pens: Pens,
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
    if pens == Pens::Run {
        shaper.lay_as_run();
    }
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
/// starts -- or, laid as a run ([`Pens::Run`]), from the run's start, the
/// pen hb-shape's advances reach.
fn expected(cps: &[Codepoint], body: &str, pens: Pens) -> Vec<(GlyphId, usize, i32, i32)> {
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
        *l = match pens {
            Pens::Cell => (*l).min(pen),
            Pens::Run => 0,
        };
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

fn devanagari_lines(pens: Pens) -> Reached {
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
        let got = shaped(&face, &cps, pens);
        let want = expected(&cps, body, pens);
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
/// cache holds it, and the built faces' and the control's everywhere, so
/// every platform shapes Devanagari through the Indic engine end to end.
/// On macOS every face's lines run, the reordering and the dotted circles
/// among them.
#[test]
fn every_devanagari_fixture_shapes_as_harfbuzz_does() {
    let r = devanagari_lines(Pens::Cell);
    eprintln!("{r:?}");
    for face in [
        "baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf",
        "baud-app/fonts/built/Regular.ttf",
        "baud-app/fonts/built/Bold.ttf",
    ] {
        let n = r.lines_by_face.get(face).copied().unwrap_or(0);
        assert!(n >= 390, "{n} lines of {face}");
    }
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

/// **Laid as a run, every line of `devanagari.txt` is where hb-shape put
/// it**: each cluster's pen goes on from the one before ([`Pens::Run`]),
/// so every glyph's offset from the line's start is the pen hb-shape's
/// advances reach plus its own offset -- the natural advances a word
/// drawn whole at them is laid at, its headline as continuous as the
/// face's. The same lines and faces as the cell test, the built faces'
/// and the control's on every platform; with the pen started again at
/// each cell most lines differ (2401 of the 3192 on macOS), the premise
/// that the test reads the run's pens.
#[test]
fn every_devanagari_fixture_laid_as_a_run_is_where_harfbuzz_puts_it() {
    let r = devanagari_lines(Pens::Run);
    eprintln!("{r:?}");
    for face in [
        "baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf",
        "baud-app/fonts/built/Regular.ttf",
        "baud-app/fonts/built/Bold.ttf",
    ] {
        let n = r.lines_by_face.get(face).copied().unwrap_or(0);
        assert!(n >= 390, "{n} lines of {face}");
    }
    if cfg!(target_os = "macos") {
        assert!(r.lines_by_face.len() >= 5, "{r:?}");
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

/// One fixture line's hb-shape glyphs, by face and code points.
fn fixture_glyphs(face: &str) -> std::collections::BTreeMap<String, Vec<HbGlyph>> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/shape/devanagari.txt"
    ))
    .unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter(|f| f[0] == face)
        .map(|f| (f[4].to_string(), parse_positioned(f[5])))
        .collect()
}

/// The built Regular: its bytes, and the first glyph the Devanagari merge
/// appended -- every glyph before it is JetBrains Mono's or the Hebrew's
/// (the merge appends after every glyph it is given, and its report names
/// this id: 1878 in Regular and Bold, 1865 in the italics).
fn built_regular() -> (Vec<u8>, u16) {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    (
        std::fs::read(format!("{root}/baud-app/fonts/built/Regular.ttf")).unwrap(),
        1878,
    )
}

/// **The built Regular shapes Devanagari exactly as the pinned Noto Sans
/// Devanagari does**: every line `devanagari.txt` shaped on Noto's file is
/// there shaped on the built Regular too, by hb-shape, and the two agree
/// glyph for glyph -- the same clusters, offsets and advances -- under one
/// renaming of glyph ids, a function and one to one, that sends every Noto
/// glyph to a glyph the merge appended but Noto's space, which goes to the
/// built face's own space: the one glyph whose advance is the built face's
/// (600) rather than Noto's, but where it stands for a joiner hb-shape
/// hides (0 in both). Read from the fixture text alone, so it runs on every
/// platform, Noto's file in the cache or not.
#[test]
fn the_built_regular_shapes_devanagari_as_noto_does() {
    let noto = fixture_glyphs("noto-cache/NotoSansDevanagari-Regular.ttf");
    let built = fixture_glyphs("baud-app/fonts/built/Regular.ttf");
    let (data, first) = built_regular();
    let face = Face::parse(&data).unwrap();
    let space = face.glyph(Codepoint::new(0x20).unwrap()).unwrap();
    let mut map = std::collections::BTreeMap::new();
    let mut lines = 0;
    for (cps, n) in &noto {
        let b = built
            .get(cps)
            .unwrap_or_else(|| panic!("{cps}: no built line"));
        assert_eq!(n.len(), b.len(), "{cps}: glyph count");
        for (x, y) in n.iter().zip(b) {
            let to = *map.entry(x.id.index()).or_insert(y.id.index());
            assert_eq!(
                to,
                y.id.index(),
                "{cps}: Noto glyph {:?} renamed two ways",
                x.id
            );
            assert_eq!(
                (x.cluster.0, x.dx, x.dy),
                (y.cluster.0, y.dx, y.dy),
                "{cps}: glyph {:?}",
                x.id
            );
            if y.id == space {
                // A space takes the built face's 600; a default ignorable
                // hb-shape hides behind the space glyph (ZWJ, ZWNJ) takes
                // 0 in both.
                let hidden = x.ax == 0 && y.ax == 0;
                assert!(
                    y.ax == 600 || hidden,
                    "{cps}: the built space's advance {}",
                    y.ax
                );
            } else {
                assert_eq!(x.ax, y.ax, "{cps}: glyph {:?}'s advance", x.id);
            }
        }
        lines += 1;
    }
    assert!(lines >= 390, "{lines} lines");
    let mut images: Vec<usize> = map.values().copied().collect();
    images.sort();
    images.dedup();
    assert_eq!(
        images.len(),
        map.len(),
        "two Noto glyphs share a built glyph"
    );
    let below: Vec<_> = map
        .iter()
        .filter(|(_, b)| **b < usize::from(first))
        .collect();
    assert_eq!(
        below.len(),
        1,
        "Noto glyphs named by built glyphs: {below:?}"
    );
    assert_eq!(*below[0].1, space.index(), "the one is the space");
    eprintln!("{lines} lines, {} glyphs renamed", map.len());
}

/// An outline as the commands a sink receives, in font units.
#[derive(Default, PartialEq, Debug)]
struct Path(Vec<(u8, [i32; 6])>);

impl Path {
    fn push(&mut self, op: u8, p: [f32; 6]) {
        // Exact in font units at the default instance; a varied point is
        // a sum of scaled deltas, held to a sixty-fourth of a unit.
        self.0.push((op, p.map(|v| (v * 64.0).round() as i32)));
    }
}

impl baud_font::Sink for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.push(0, [x, y, 0.0, 0.0, 0.0, 0.0]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.push(1, [x, y, 0.0, 0.0, 0.0, 0.0]);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.push(2, [x1, y1, x, y, 0.0, 0.0]);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.push(3, [x1, y1, x2, y2, x, y]);
    }
    fn close(&mut self) {
        self.push(4, [0.0; 6]);
    }
}

/// `g`'s outline at `face`'s coordinates.
fn path(face: &Face<'_>, g: GlyphId) -> Path {
    let mut p = Path::default();
    let need = usize::try_from(face.outline_need(g).value()).unwrap();
    let mut scratch = vec![baud_font::VarPoint::EMPTY; need];
    let _ = face.outline_at(g, &mut scratch, &mut p);
    p
}

fn at_width(face: &mut Face<'_>, wdth: f32) {
    face.set_variations(&[baud_font::var::Variation {
        tag: baud_font::Tag::from_bytes(*b"wdth"),
        value: baud_font::var::Fixed::from_f32(wdth),
    }]);
}

/// **Along the width axis only the Devanagari moves.** The built faces
/// carry Noto's `wdth` axis, 62.5 to 100, for a program that narrows a
/// Devanagari word to fit it: at 62.5 and at 81.25 every glyph before the
/// first the merge appended -- all of JetBrains Mono's and the Hebrew --
/// draws the outline and takes the advance it takes at the default
/// instance, and the README line's Devanagari letters narrow.
#[test]
fn along_the_width_axis_only_the_devanagari_moves() {
    let (data, first) = built_regular();
    let plain = Face::parse(&data).unwrap();
    let axis = plain.axis(baud_font::bytes::Index::ZERO).unwrap();
    assert_eq!(axis.tag, baud_font::Tag::from_bytes(*b"wdth"));
    assert_eq!(
        (axis.min.to_f32(), axis.default.to_f32(), axis.max.to_f32()),
        (62.5, 100.0, 100.0)
    );
    let mut narrowed = 0;
    for wdth in [62.5, 81.25] {
        let mut narrow = Face::parse(&data).unwrap();
        at_width(&mut narrow, wdth);
        for i in 0..first {
            let g = GlyphId::from_shaper(i);
            assert_eq!(
                narrow.advance(g),
                plain.advance(g),
                "glyph {i} at wdth {wdth}"
            );
            assert_eq!(
                path(&narrow, g),
                path(&plain, g),
                "glyph {i} at wdth {wdth}"
            );
        }
        for c in "टर्मिनल हर अक्षर को ग्रिड में रखता है"
            .chars()
            .filter(|c| *c != ' ')
        {
            let g = plain.glyph(Codepoint::from_char(c)).unwrap();
            assert!(g.index() >= usize::from(first), "{c} is an appended glyph");
            let (a, b) = (narrow.advance(g).unwrap(), plain.advance(g).unwrap());
            assert!(a.value() <= b.value(), "{c} at wdth {wdth}: {a:?} > {b:?}");
            narrowed += usize::from(a.value() < b.value());
        }
    }
    assert!(narrowed >= 20, "{narrowed} letters narrowed");
}

/// **The built Regular draws Noto's own outlines and shapes as Noto does at
/// every width**, where the cache holds the variable Noto Sans Devanagari
/// `scripts/build-fonts.sh` merges from (it fetches it there): every glyph
/// the fixtures rename, at the default instance and at wdth 62.5 and
/// 81.25, has Noto's outline and advance, and the README line shapes to
/// the same glyphs at the same places through baud's shaper.
#[test]
fn the_built_regular_draws_noto_at_every_width() {
    const SHA: &str = "9f82923465ef82fb86c6b99cbdd4947ba832fe3335948de815b18adfd75947c7";
    let Some(vf) = noto_cache()
        .map(|d| d.join("NotoSansDevanagari[wdth,wght].ttf"))
        .and_then(|p| std::fs::read(p).ok())
    else {
        eprintln!("the variable Noto Sans Devanagari is not in the cache: skipped");
        return;
    };
    assert_eq!(
        sha256_of(&vf),
        SHA,
        "the cache's file is not the pinned one"
    );
    let noto = fixture_glyphs("noto-cache/NotoSansDevanagari-Regular.ttf");
    let built = fixture_glyphs("baud-app/fonts/built/Regular.ttf");
    let mut map = std::collections::BTreeMap::new();
    for (cps, n) in &noto {
        for (x, y) in n.iter().zip(&built[cps]) {
            map.insert(x.id.index(), y.id);
        }
    }
    let (data, _) = built_regular();
    let space = Face::parse(&data)
        .unwrap()
        .glyph(Codepoint::new(0x20).unwrap())
        .unwrap();
    let line: Vec<Codepoint> = "टर्मिनल हर अक्षर को ग्रिड में रखता है।"
        .chars()
        .map(Codepoint::from_char)
        .collect();
    for wdth in [None, Some(62.5), Some(81.25)] {
        let (mut a, mut b) = (Face::parse(&vf).unwrap(), Face::parse(&data).unwrap());
        if let Some(w) = wdth {
            at_width(&mut a, w);
            at_width(&mut b, w);
        }
        for (&n, &m) in map.iter().filter(|(_, m)| **m != space) {
            let n = GlyphId::from_shaper(u16::try_from(n).unwrap());
            assert_eq!(a.advance(n), b.advance(m), "{n:?} at {wdth:?}");
            assert_eq!(path(&a, n), path(&b, m), "{n:?} at {wdth:?}");
        }
        let noto_space = a.glyph(Codepoint::new(0x20).unwrap()).unwrap();
        let (sa, sb) = (shaped(&a, &line, Pens::Cell), shaped(&b, &line, Pens::Cell));
        assert_eq!(sa.len(), sb.len(), "at {wdth:?}");
        for (x, y) in sa.iter().zip(&sb) {
            let want = if x.0 == noto_space {
                space
            } else {
                map[&x.0.index()]
            };
            assert_eq!(want, y.0, "at {wdth:?}");
            assert_eq!((x.1, x.2, x.3), (y.1, y.2, y.3), "at {wdth:?}");
        }
    }
}

/// The file's SHA-256, hex.
fn sha256_of(data: &[u8]) -> String {
    sha256::sha256_hex(data)
}

/// Each line of `devanagari-wdth.txt` -- Noto Sans Devanagari's variable
/// file at a narrower `wdth`, as hb-shape shaped it -- shaped at those
/// coordinates and laid as `pens` says, against hb-shape: the lines
/// compared, or `None` where the README's cache does not hold the file.
fn narrowed_lines(pens: Pens) -> Option<usize> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/shape/devanagari-wdth.txt"
    ))
    .unwrap();
    let mut data: Option<Option<Vec<u8>>> = None;
    let mut mismatches = Vec::new();
    let mut compared = 0usize;
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        let (path, index, digest, variations, cps, body) = (f[0], f[1], f[2], f[4], f[5], f[6]);
        let bytes = data.get_or_insert_with(|| {
            let d = face_file(path)?;
            assert_eq!(
                fnv64(&d),
                digest,
                "{path} is not the file the fixtures were shaped with"
            );
            Some(d)
        });
        let Some(bytes) = bytes else {
            return None;
        };
        let mut face = Face::parse_index(bytes, FaceIndex::of(index.parse().unwrap())).unwrap();
        let settings: Vec<baud_font::var::Variation> = variations
            .split(',')
            .map(|v| {
                let (tag, value) = v.split_once('=').unwrap();
                baud_font::var::Variation {
                    tag: baud_font::Tag::from_bytes(tag.as_bytes().try_into().unwrap()),
                    value: baud_font::var::Fixed::from_f32(value.parse().unwrap()),
                }
            })
            .collect();
        face.set_variations(&settings);
        let cps: Vec<Codepoint> = cps
            .split_whitespace()
            .map(|h| Codepoint::new(u32::from_str_radix(h, 16).unwrap()).unwrap())
            .collect();
        let got = shaped(&face, &cps, pens);
        let want = expected(&cps, body, pens);
        if got != want {
            mismatches.push(format!(
                "{variations} {cps:04X?}:\n  baud    {got:?}\n  hb-shape {want:?}"
            ));
            continue;
        }
        compared += 1;
    }
    assert!(
        mismatches.is_empty(),
        "{} of devanagari-wdth.txt's lines differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    Some(compared)
}

/// **Narrowed on its `wdth` axis, Noto Sans Devanagari shapes as hb-shape
/// shapes it**, cell by cell and laid as a run: every Noto line of
/// `devanagari.txt` at `wdth` 81.25 and at 62.5, the narrowest a word too
/// wide for its columns is drawn at, its advances read through `HVAR` and
/// its anchors through `GDEF`'s variation store. Where the cache does not
/// hold the variable file, skipped.
#[test]
fn narrowed_noto_shapes_as_harfbuzz_does() {
    for pens in [Pens::Cell, Pens::Run] {
        match narrowed_lines(pens) {
            Some(n) => assert!(n >= 790, "{pens:?}: {n} lines"),
            None => eprintln!("the variable file is not in the cache: skipped"),
        }
    }
}

/// The cells the core gives `cps` while `?2027` is reset: each legacy
/// grapheme cluster a cell ([`ClusterCells::Legacy`]), so a consonant after
/// a virama and a spacing vowel sign each start one, as the core's writer
/// parts the extended machine where it joins by GB9a, GB9b or GB9c --
/// र्मि is the three cells र्, म and ि the renderer joins into a syllable.
fn legacy_cells(cps: &[Codepoint]) -> Vec<core::ops::Range<usize>> {
    let mut machine = ClusterBreak::new();
    let mut out: Vec<core::ops::Range<usize>> = Vec::new();
    for (i, &c) in cps.iter().enumerate() {
        let (class, lb) = GraphemeClass::of_with_lookback(c);
        let before = machine;
        let joined = !machine.is_break_classified(class, lb);
        let parted = joined && before.parts_at(ClusterCells::Legacy, class, lb, c);
        if parted {
            machine = ClusterBreak::restart(class, lb);
        }
        match out.last_mut() {
            Some(last) if joined && !parted => last.end = i + 1,
            _ => out.push(i..i + 1),
        }
    }
    out
}

/// `cps` cut into `cut`, a cell a cluster, shaped as one run laid end to
/// end at cluster level `level`, with the lookup digests a renderer keeps:
/// each glyph's id, cell, pen from the run's start and rise.
fn shaped_at(
    face: &Face<'_>,
    cps: &[Codepoint],
    cut: &[core::ops::Range<usize>],
    level: ClusterLevel,
) -> Vec<(GlyphId, usize, i32, i32)> {
    let mut digests = vec![Digest::EMPTY; MAX_LOOKUPS];
    if let Some(gsub) = face.gsub() {
        gsub.digests(Features::JOINING, &mut digests);
    }
    let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 256];
    let mut shaper = Shaper::new(face, &mut storage);
    shaper.accelerate(&digests);
    shaper.lay_as_run();
    if level == ClusterLevel::Characters {
        shaper.keep_cells();
    }
    for (k, r) in cut.iter().enumerate() {
        shaper
            .push_cluster(&cps[r.clone()], Cluster(u16::try_from(k).unwrap()))
            .unwrap();
    }
    let script = Script::of_cluster(cps);
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

/// **At HarfBuzz's CHARACTERS cluster level every line of
/// `devanagari.txt` keeps its cells as hb-shape keeps them**
/// (`[[source: harfbuzz-14-5-1-buffer-h]]`: the cluster values "are not
/// merged into monotone order"): `tests/shape/devanagari-characters.txt`
/// is each line shaped by hb-shape at `--cluster-level=2`, on
/// `devanagari.txt`'s faces and the built Italic and Bold Italic. Cut into
/// the cells the core gives it under each count -- each legacy cluster a
/// cell while `?2027` is reset, each extended one while it is set -- and
/// shaped as a run with each cell's glyphs keeping it
/// ([`ClusterLevel::Characters`]), every glyph's id, pen and rise is
/// hb-shape's and so is the cell it names, the cell holding its code point
/// index. The glyphs and pens are the default level's on every line, and
/// only the cells differ: on the built Regular, the cells of 96 lines
/// under the legacy count and of one under the extended count.
#[test]
fn the_characters_level_keeps_cells_as_harfbuzz_does() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/shape/devanagari-characters.txt"
    ))
    .unwrap();
    let mut files: std::collections::HashMap<String, Option<Vec<u8>>> = Default::default();
    let mut mismatches = Vec::new();
    let mut lines_by_face: std::collections::BTreeMap<String, usize> = Default::default();
    // Lines of the built Regular whose cells the level changes, by count.
    let (mut kept_legacy, mut kept_extended) = (0usize, 0usize);
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
        let hb = parse_positioned(body);
        let mut agreed = true;
        for (count, cut) in [("legacy", legacy_cells(&cps)), ("extended", cells(&cps))] {
            let cell_of = |c: CodeIndex| cut.iter().rposition(|r| r.start <= c.0).unwrap();
            let mut pen = 0i32;
            let want: Vec<(GlyphId, usize, i32, i32)> = hb
                .iter()
                .map(|e| {
                    let g = (e.id, cell_of(e.cluster), pen + e.dx, e.dy);
                    pen += e.ax;
                    g
                })
                .collect();
            let got = shaped_at(&face, &cps, &cut, ClusterLevel::Characters);
            if got != want {
                mismatches.push(format!(
                    "{path} {count} {cps:04X?}:\n  baud     {got:?}\n  hb-shape {want:?}"
                ));
                agreed = false;
                continue;
            }
            let merged = shaped_at(&face, &cps, &cut, ClusterLevel::Monotone);
            let drawn = |v: &[(GlyphId, usize, i32, i32)]| -> Vec<(GlyphId, i32, i32)> {
                v.iter().map(|&(id, _, x, y)| (id, x, y)).collect()
            };
            assert_eq!(
                drawn(&got),
                drawn(&merged),
                "{path} {count} {cps:04X?}: the level changes a glyph or a pen"
            );
            if path == "baud-app/fonts/built/Regular.ttf" && got != merged {
                match count {
                    "legacy" => kept_legacy += 1,
                    _ => kept_extended += 1,
                }
            }
        }
        if agreed {
            *lines_by_face.entry(path.to_string()).or_default() += 1;
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of devanagari-characters.txt's cuts differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    eprintln!("{lines_by_face:?}, cells kept {kept_legacy} legacy, {kept_extended} extended");
    for face in [
        "baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf",
        "baud-app/fonts/built/Regular.ttf",
        "baud-app/fonts/built/Bold.ttf",
        "baud-app/fonts/built/Italic.ttf",
        "baud-app/fonts/built/BoldItalic.ttf",
    ] {
        let n = lines_by_face.get(face).copied().unwrap_or(0);
        assert!(n >= 390, "{n} lines of {face}");
    }
    if cfg!(target_os = "macos") {
        assert!(lines_by_face.len() >= 9, "{lines_by_face:?}");
    }
    assert!(
        kept_legacy >= 90 && kept_extended >= 1 && kept_extended < kept_legacy,
        "the premise: the level keeps cells the default merges, {kept_legacy} legacy, {kept_extended} extended"
    );
}
