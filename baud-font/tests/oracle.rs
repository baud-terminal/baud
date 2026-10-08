//! The differential: every face this machine has, field by field, against
//! `ttf-parser`.
//!
//! Two implementations of one specification over the same bytes must answer
//! the same, and where they do not the test names the face, the field and
//! both answers so the specification can decide. This is `baud-difftest`'s
//! discipline one crate over: an oracle that only prints cannot fail, so
//! every disagreement is an assertion, and every normalisation the
//! comparison makes is counted and pinned rather than absorbed.
//!
//! **Corpus.** The four OFL faces committed under `tests/fonts/` always; on
//! macOS, every `.ttf`, `.otf` and `.ttc` under `/System/Library/Fonts`,
//! which is 369 files and several hundred faces including the collections.
//! A positive control requires the corpus to be non-trivial before any
//! agreement counts.

use baud_domain::{Codepoint, GlyphId};
use baud_font::{Drawn, Face, FaceIndex, Variation, faces};
use std::path::{Path, PathBuf};

fn committed() -> Vec<PathBuf> {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts"));
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("tests/fonts exists")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ttf" || e == "otf"))
        .collect();
    v.sort();
    v
}

fn system() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if cfg!(target_os = "macos") {
        walk(Path::new("/System/Library/Fonts"), &mut v);
    }
    v.sort();
    v
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p
            .extension()
            .is_some_and(|e| e == "ttf" || e == "otf" || e == "ttc")
        {
            out.push(p);
        }
    }
}

fn corpus() -> Vec<PathBuf> {
    let mut v = committed();
    v.extend(system());
    v
}

/// One face from each implementation, or `None` when *both* refuse it. A
/// face one accepts and the other rejects is a disagreement and is reported
/// as such by the caller.
fn pair<'a>(data: &'a [u8], i: u32) -> (Option<Face<'a>>, Option<ttf_parser::Face<'a>>) {
    (
        Face::parse_index(data, FaceIndex::of(i)).ok(),
        ttf_parser::Face::parse(data, i).ok(),
    )
}

fn cp(v: u32) -> Option<Codepoint> {
    Codepoint::new(v)
}

/// Every codepoint the cmap comparison visits: the whole BMP outside the
/// surrogates, and the astral ranges a terminal actually meets -- emoji and
/// symbols, CJK Extension B, and the variation-selector supplement.
fn codepoints() -> Vec<u32> {
    let mut v: Vec<u32> = (0..0xD800).chain(0xE000..0x1_0000).collect();
    v.extend(0x1_F000..0x1_FB00);
    v.extend((0x2_0000..0x2_A6E0).step_by(7));
    v.extend(0xE_0100..0xE_01F0);
    v
}

/// Where the oracle is wrong and the specification decides, each entry a
/// face, a codepoint and the reason. **Every entry must fire**: an exemption
/// that no longer matches anything is a note about a defect that was fixed
/// upstream, and the test says so rather than carrying it.
///
/// `ttf-parser` 0.25.1, `cmap` format 4, the `idRangeOffset` path: it reads
/// the `glyphIdArray` value as `u16`, casts it to `i16` to add `idDelta`, and
/// converts back with `u16::try_from`, which fails for any value at or above
/// 32768. The specification's arithmetic is modulo 65536 with no sign
/// (`[[source: ms-opentype-cmap]]`), and DecoType Nastaleeq Urdu has 53,137
/// glyphs, so every character its `cmap` maps through that path to a glyph
/// above 32,767 -- the Arabic signs, honorifics and more -- is one the oracle
/// refuses to name.
///
/// Each entry is a face and the reason. The shape of the defect is checked,
/// not a codepoint list: this crate answers a glyph at or above 32,768 and
/// the oracle answers nothing, on that face only. Anywhere else the same
/// shape is a disagreement.
const ORACLE_DEFECTS: &[(&str, &str)] = &[
    (
        "DecoTypeNastaleeqUrdu.ttc#0",
        "format 4 glyphs above 32767 truncated through i16 by ttf-parser",
    ),
    (
        "DecoTypeNastaleeqUrdu.ttc#1",
        "format 4 glyphs above 32767 truncated through i16 by ttf-parser",
    ),
];

#[derive(Default, Debug)]
struct Tally {
    files: usize,
    faces: usize,
    parse_disagreements: Vec<String>,
    field_disagreements: Vec<String>,
    glyphs_agreed: usize,
    cmap_agreed: usize,
    cmap_hits: usize,
    variations_agreed: usize,
    variation_hits: usize,
    notdef_normalised: usize,
    oracle_defects_hit: [usize; ORACLE_DEFECTS.len()],
    outlines_agreed: usize,
    outline_hits: usize,
    outline_presence_normalised: usize,
    gvar_refusals: usize,
    cff2_unimplemented: usize,
}

impl Tally {
    /// Adds another file's tally to this one. Destructured whole, so a field
    /// added to `Tally` and not summed here does not compile -- a count left
    /// out would read zero, and a positive control on it would fail for the
    /// merge rather than for the fonts.
    fn absorb(&mut self, other: Tally) {
        let Tally {
            files,
            faces,
            parse_disagreements,
            field_disagreements,
            glyphs_agreed,
            cmap_agreed,
            cmap_hits,
            variations_agreed,
            variation_hits,
            notdef_normalised,
            oracle_defects_hit,
            outlines_agreed,
            outline_hits,
            outline_presence_normalised,
            gvar_refusals,
            cff2_unimplemented,
        } = other;
        self.files += files;
        self.faces += faces;
        self.parse_disagreements.extend(parse_disagreements);
        self.field_disagreements.extend(field_disagreements);
        self.glyphs_agreed += glyphs_agreed;
        self.cmap_agreed += cmap_agreed;
        self.cmap_hits += cmap_hits;
        self.variations_agreed += variations_agreed;
        self.variation_hits += variation_hits;
        self.notdef_normalised += notdef_normalised;
        for (mine, theirs) in self.oracle_defects_hit.iter_mut().zip(oracle_defects_hit) {
            *mine += theirs;
        }
        self.outlines_agreed += outlines_agreed;
        self.outline_hits += outline_hits;
        self.outline_presence_normalised += outline_presence_normalised;
        self.gvar_refusals += gvar_refusals;
        self.cff2_unimplemented += cff2_unimplemented;
    }
}

/// One outline operation with its coordinates as `f32` bits, so equality is
/// bit equality and `-0.0` is not `0.0`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seg {
    M(u32, u32),
    L(u32, u32),
    Q(u32, u32, u32, u32),
    C(u32, u32, u32, u32, u32, u32),
    Z,
}

struct Segs(Vec<Seg>);

impl ttf_parser::OutlineBuilder for Segs {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(Seg::M(x.to_bits(), y.to_bits()));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(Seg::L(x.to_bits(), y.to_bits()));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0
            .push(Seg::Q(x1.to_bits(), y1.to_bits(), x.to_bits(), y.to_bits()));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push(Seg::C(
            x1.to_bits(),
            y1.to_bits(),
            x2.to_bits(),
            y2.to_bits(),
            x.to_bits(),
            y.to_bits(),
        ));
    }
    fn close(&mut self) {
        self.0.push(Seg::Z);
    }
}

impl baud_font::Sink for Segs {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(Seg::M(x.to_bits(), y.to_bits()));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(Seg::L(x.to_bits(), y.to_bits()));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0
            .push(Seg::Q(x1.to_bits(), y1.to_bits(), x.to_bits(), y.to_bits()));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push(Seg::C(
            x1.to_bits(),
            y1.to_bits(),
            x2.to_bits(),
            y2.to_bits(),
            x.to_bits(),
            y.to_bits(),
        ));
    }
    fn close(&mut self) {
        self.0.push(Seg::Z);
    }
}

fn compare_face(
    name: &str,
    ours: &Face<'_>,
    theirs: &ttf_parser::Face<'_>,
    t: &mut Tally,
    cps: &[u32],
) {
    let mut bad = |what: &str, a: String, b: String| {
        t.field_disagreements
            .push(format!("{name}: {what}: baud {a} / ttf-parser {b}"));
    };

    if ours.glyph_count().value() != theirs.number_of_glyphs() {
        bad(
            "numGlyphs",
            ours.glyph_count().value().to_string(),
            theirs.number_of_glyphs().to_string(),
        );
    }
    if ours.units_per_em().value() != theirs.units_per_em() {
        bad(
            "unitsPerEm",
            ours.units_per_em().value().to_string(),
            theirs.units_per_em().to_string(),
        );
    }
    // ttf-parser's ascender/descender/line_gap prefer OS/2 typo metrics when
    // USE_TYPO_METRICS is set; hhea's raw values are `hhea` accessors there
    // are none of, so compare through the same rule it applies: when the
    // face has no OS/2 table the two must equal hhea.
    if theirs.tables().os2.is_none() {
        let l = ours.line_metrics();
        if l.ascender.value() != theirs.ascender()
            || l.descender.value() != theirs.descender()
            || l.line_gap.value() != theirs.line_gap()
        {
            bad(
                "hhea metrics",
                format!("{:?}", l),
                format!(
                    "{} {} {}",
                    theirs.ascender(),
                    theirs.descender(),
                    theirs.line_gap()
                ),
            );
        }
    }

    // Every glyph's advance and side bearing.
    let n = theirs.number_of_glyphs();
    for id in 0..n {
        let g = GlyphId::from_shaper(id);
        let tg = ttf_parser::GlyphId(id);
        let (a, b) = (
            ours.advance(g).map(|a| a.value()),
            theirs.glyph_hor_advance(tg),
        );
        if a != b {
            bad(
                &format!("advance of glyph {id}"),
                format!("{a:?}"),
                format!("{b:?}"),
            );
            break;
        }
        let (a, b) = (
            ours.side_bearing(g).map(|a| a.value()),
            theirs.glyph_hor_side_bearing(tg),
        );
        if a != b {
            bad(
                &format!("lsb of glyph {id}"),
                format!("{a:?}"),
                format!("{b:?}"),
            );
            break;
        }
        t.glyphs_agreed += 1;
    }

    // Every glyph's outline, segment for segment, bit for bit. The two
    // readers emit the same operations in the same order from the same
    // specification, so equality is exact rather than within a tolerance;
    // a tolerance would hide a wrong control point behind a right endpoint.
    //
    // **One normalisation, counted.** The oracle reports "nothing drawn" for
    // a glyph whose points are all at the origin (its bounding box never
    // grows), while this crate reports what it emitted; when the segments
    // are identical and only that verdict differs, it is counted here.
    let mut first_bad = true;
    for id in 0..n {
        let g = GlyphId::from_shaper(id);
        let tg = ttf_parser::GlyphId(id);
        let mut a = Segs(Vec::new());
        let mut b = Segs(Vec::new());
        let drawn = ours.outline(g, &mut a);
        let theirs_drew = theirs.outline_glyph(tg, &mut b).is_some();
        // **Two more normalisations, each tied to a table, each verified
        // against a second reading rather than assumed, and counted.**
        //
        // A variable face: the oracle routes every outline through `gvar`
        // even at the default instance, and where it cannot apply the
        // variation data it stops -- emitting nothing, or a prefix. baud
        // reads the default instance from `glyf` directly (register D11),
        // which is what the specification says `glyf` holds. The check is
        // not "the face is variable, so excuse it": baud's answer must equal
        // what the oracle's *own* `glyf` reader emits when asked without
        // `gvar`, bit for bit. Only then is the difference the oracle's
        // variation path and not the outline.
        if a.0 != b.0 && theirs.tables().gvar.is_some() {
            let mut c = Segs(Vec::new());
            let _ = theirs.tables().glyf.map(|glyf| glyf.outline(tg, &mut c));
            if a.0 == c.0 {
                t.gvar_refusals += 1;
                continue;
            }
        }
        // CFF2 is not read yet (register D11 defers it with variations):
        // a face whose only outlines are CFF2 draws for the oracle and not
        // for baud, and that gap is counted rather than hidden.
        if a.0.is_empty() && !b.0.is_empty() && theirs.tables().cff2.is_some() {
            t.cff2_unimplemented += 1;
            continue;
        }
        if a.0 != b.0 {
            if first_bad {
                let at =
                    a.0.iter()
                        .zip(&b.0)
                        .position(|(x, y)| x != y)
                        .unwrap_or(a.0.len().min(b.0.len()));
                bad(
                    &format!(
                        "outline of glyph {id} (segment {at} of {}/{})",
                        a.0.len(),
                        b.0.len()
                    ),
                    format!("{:?}", a.0.get(at)),
                    format!("{:?}", b.0.get(at)),
                );
                first_bad = false;
            }
            continue;
        }
        if (drawn == Drawn::Outline) != theirs_drew {
            t.outline_presence_normalised += 1;
        }
        t.outlines_agreed += 1;
        if drawn == Drawn::Outline {
            t.outline_hits += 1;
        }
    }

    // cmap over the corpus of codepoints.
    //
    // **One normalisation, counted.** `ttf-parser` answers glyph 0 where a
    // format 4 delta segment lands on it -- U+FFFF's sentinel segment in
    // nearly every face -- and this crate never does, because glyph 0 is
    // `.notdef` and a caller that gets `None` can fall back to the next face
    // where one that got 0 would draw a box. So `Some(0)` from the oracle is
    // read as `None`, and how often that happens is pinned below rather than
    // absorbed.
    let mut first_bad = true;
    for &c in cps {
        let Some(code) = cp(c) else { continue };
        let a = ours.glyph(code).map(|g| u16::try_from(g.index()).unwrap());
        let raw = char::from_u32(c)
            .and_then(|ch| theirs.glyph_index(ch))
            .map(|g| g.0);
        if raw == Some(0) {
            t.notdef_normalised += 1;
        }
        let b = raw.filter(|&g| g != 0);
        if a != b {
            let truncated = a.is_some_and(|g| g >= 32768) && b.is_none();
            if let Some(k) = ORACLE_DEFECTS
                .iter()
                .position(|(face, _)| *face == name && truncated)
            {
                t.oracle_defects_hit[k] += 1;
                continue;
            }
            if first_bad {
                bad(
                    &format!("cmap U+{c:04X}"),
                    format!("{a:?}"),
                    format!("{b:?}"),
                );
                first_bad = false;
            }
            continue;
        }
        t.cmap_agreed += 1;
        if a.is_some() {
            t.cmap_hits += 1;
        }
    }

    // Variation sequences: text and emoji presentation selectors over the
    // BMP and the emoji block, and the supplement selectors over a sample.
    let mut first_bad = true;
    let selectors = [0xFE0E_u32, 0xFE0F, 0xE0100, 0xE0101];
    for &vs in &selectors {
        let Some(vsel) = cp(vs) else { continue };
        let Some(vch) = char::from_u32(vs) else {
            continue;
        };
        for &c in cps.iter().step_by(3) {
            let (Some(code), Some(ch)) = (cp(c), char::from_u32(c)) else {
                continue;
            };
            let a = match ours.variation(code, vsel) {
                Variation::Default => ours.glyph(code).map(|g| u16::try_from(g.index()).unwrap()),
                Variation::Glyph(g) => Some(u16::try_from(g.index()).unwrap()),
                Variation::Absent => None,
            };
            let raw = theirs.glyph_variation_index(ch, vch).map(|g| g.0);
            if raw == Some(0) {
                t.notdef_normalised += 1;
            }
            let b = raw.filter(|&g| g != 0);
            if a != b {
                if first_bad {
                    bad(
                        &format!("variation U+{c:04X} U+{vs:04X}"),
                        format!("{a:?}"),
                        format!("{b:?}"),
                    );
                    first_bad = false;
                }
                continue;
            }
            t.variations_agreed += 1;
            if a.is_some() {
                t.variation_hits += 1;
            }
        }
    }
}

/// Every face in one file, compared and tallied on its own.
fn compare_file(path: &Path, cps: &[u32]) -> Tally {
    let mut t = Tally::default();
    let data = std::fs::read(path).unwrap();
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    t.files += 1;
    let ours_n = faces(&data).map(|n| n.value()).ok();
    let theirs_n = ttf_parser::fonts_in_collection(&data).or(Some(1));
    if ours_n != theirs_n {
        t.parse_disagreements.push(format!(
            "{name}: faces: baud {ours_n:?} / ttf-parser {theirs_n:?}"
        ));
        return t;
    }
    for i in 0..ours_n.unwrap_or(0) {
        let face = format!("{name}#{i}");
        match pair(&data, i) {
            (Some(a), Some(b)) => {
                t.faces += 1;
                compare_face(&face, &a, &b, &mut t, cps);
            }
            (None, None) => {}
            (a, b) => t.parse_disagreements.push(format!(
                "{face}: parse: baud {} / ttf-parser {}",
                a.is_some(),
                b.is_some()
            )),
        }
    }
    t
}

#[test]
fn every_face_on_this_machine_agrees_with_ttf_parser() {
    let cps = codepoints();
    let files = corpus();
    // **One file a thread at a time, summed in corpus order.** Each file is
    // compared on its own, so the corpus is split across every core. Each
    // worker takes the next file, largest first, so Apple Color Emoji and the
    // CJK collections start at once rather than last and alone. The tallies
    // are then added in the corpus's own order, so every count and every
    // disagreement message is the same as one thread's. Peak memory is the
    // files in flight: on this Mac's eighteen cores the eighteen largest are
    // ~626 of the 813 MB, against Apple Color Emoji's 192 MB alone before.
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by_key(|&i| core::cmp::Reverse(std::fs::metadata(&files[i]).map_or(0, |m| m.len())));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(files.len().max(1));
    let mut slots: Vec<Option<Tally>> = (0..files.len()).map(|_| None).collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&i) = order.get(k) else { break };
                        done.push((i, compare_file(&files[i], &cps)));
                    }
                    done
                })
            })
            .collect();
        for h in handles {
            let done = h.join().unwrap_or_else(|e| std::panic::resume_unwind(e));
            for (i, t) in done {
                slots[i] = Some(t);
            }
        }
    });
    let mut t = Tally::default();
    for slot in slots {
        t.absorb(slot.expect("every file was compared"));
    }

    // Positive controls: the corpus is real and the comparison reached the
    // tables, or an empty walk would report perfect agreement.
    assert!(t.files >= 2, "corpus: {} files", t.files);
    assert!(t.faces >= 2, "faces compared: {}", t.faces);
    assert!(
        t.glyphs_agreed > 1_000,
        "glyph metrics compared: {}",
        t.glyphs_agreed
    );
    assert!(t.cmap_hits > 10_000, "cmap hits: {}", t.cmap_hits);
    assert!(t.variation_hits > 0, "variation hits: {}", t.variation_hits);
    // The normalisation must have fired: a corpus where it never did would
    // mean the oracle's semantics changed under this test, and the note above
    // would be describing something that no longer happens.
    assert!(
        t.notdef_normalised > 0,
        "glyph-0 normalisations: {}",
        t.notdef_normalised
    );
    assert!(
        t.outline_hits > 100_000,
        "outlines drawn: {}",
        t.outline_hits
    );
    eprintln!(
        "oracle: {} outlines compared ({} drawn, {} presence verdicts normalised, {} outlines the oracle's gvar path changed while its glyf reader agrees with baud, {} CFF2 glyphs baud does not read)",
        t.outlines_agreed,
        t.outline_hits,
        t.outline_presence_normalised,
        t.gvar_refusals,
        t.cff2_unimplemented
    );
    // Both normalisations must fire on this Mac -- ADTNumeric and SFNS carry
    // gvar, SFIndia is CFF2-only -- or the notes above describe nothing.
    if cfg!(target_os = "macos") {
        assert!(t.gvar_refusals > 0, "the gvar normalisation never fired");
        assert!(
            t.cff2_unimplemented > 0,
            "the CFF2 normalisation never fired"
        );
    }
    // Each recorded oracle defect fired -- on macOS, where the face lives;
    // elsewhere the corpus cannot reach it and the list is documentation.
    if cfg!(target_os = "macos") {
        for (k, (face, why)) in ORACLE_DEFECTS.iter().enumerate() {
            assert!(
                t.oracle_defects_hit[k] > 0,
                "{face} ({why}) never fired: the entry is stale"
            );
        }
    }

    eprintln!(
        "oracle: {} files, {} faces, {} glyph metrics, {} cmap lookups ({} hits), {} variation lookups ({} hits), {} glyph-0 answers normalised",
        t.files,
        t.faces,
        t.glyphs_agreed,
        t.cmap_agreed,
        t.cmap_hits,
        t.variations_agreed,
        t.variation_hits,
        t.notdef_normalised
    );
    assert!(
        t.parse_disagreements.is_empty(),
        "{} face(s) one side refuses:\n  {}",
        t.parse_disagreements.len(),
        t.parse_disagreements.join("\n  ")
    );
    assert!(
        t.field_disagreements.is_empty(),
        "{} disagreement(s):\n  {}",
        t.field_disagreements.len(),
        t.field_disagreements.join("\n  ")
    );
}

/// A hex SHA-256, from the same FNV-free primitive the rest of the repo's
/// gates use: `shasum` semantics, implemented here so the test needs no
/// dependency.
fn sha256_hex(data: &[u8]) -> String {
    // FIPS 180-4, straight from the standard; small enough to carry.
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes(word.try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[test]
fn the_committed_faces_are_the_ones_the_manifest_pins() {
    // Seven OFL faces and one generated sbix fixture, each with its licence
    // beside it and each hash-pinned in
    // the research manifest; a test font that is not pinned is a byte string
    // nobody can re-fetch, and one whose bytes drift from the pin is a
    // different font wearing the name.
    let names: Vec<String> = committed()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "BungeeTint-Regular.ttf",
            "FiraCode-Regular.ttf",
            "FiraCode-VF.ttf",
            "JetBrainsMono-Italic-Variable.ttf",
            "JetBrainsMono-Regular.ttf",
            "JetBrainsMono-Variable.ttf",
            "NotoColorEmoji-subset.ttf",
            "sbix-strikes.ttf",
        ]
    );
    let manifest = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/MANIFEST.toml"
    ))
    .unwrap();
    let pinned = |key: &str| -> String {
        let block = manifest
            .split("\n[sources.")
            .find(|b| b.starts_with(&format!("{key}]")))
            .unwrap_or_else(|| panic!("{key} is not in the manifest"));
        block
            .lines()
            .find_map(|l| l.strip_prefix("sha256 = \""))
            .map(|s| s.trim_end_matches('"').to_string())
            .unwrap()
    };
    for (p, key) in committed().iter().zip([
        "font-bungee-tint-regular",
        "font-fira-code-regular",
        "font-fira-code-variable",
        "font-jetbrains-mono-italic-variable",
        "font-jetbrains-mono-regular",
        "font-jetbrains-mono-variable",
        "font-noto-color-emoji-subset",
        "font-sbix-strikes",
    ]) {
        let lic = p.with_file_name(format!(
            "{}-LICENSE.txt",
            p.file_stem()
                .unwrap()
                .to_string_lossy()
                .split('-')
                .next()
                .unwrap()
        ));
        assert!(lic.exists(), "{} has no licence beside it", p.display());
        let data = std::fs::read(p).unwrap();
        assert_eq!(
            sha256_hex(&data),
            pinned(key),
            "{} drifted from its pin",
            p.display()
        );
    }
    // The hash primitive itself, against a known vector, so a wrong
    // implementation cannot agree with a wrong pin.
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
