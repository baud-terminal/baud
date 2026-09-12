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
//! **Corpus.** The two OFL faces committed under `tests/fonts/` always; on
//! macOS, every `.ttf`, `.otf` and `.ttc` under `/System/Library/Fonts`,
//! which is 369 files and several hundred faces including the collections.
//! A positive control requires the corpus to be non-trivial before any
//! agreement counts.

use baud_domain::{Codepoint, GlyphId};
use baud_font::{Face, FaceIndex, Variation, faces};
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

#[test]
fn every_face_on_this_machine_agrees_with_ttf_parser() {
    let cps = codepoints();
    let mut t = Tally::default();
    for path in corpus() {
        let data = std::fs::read(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        t.files += 1;
        let ours_n = faces(&data).map(|n| n.value()).ok();
        let theirs_n = ttf_parser::fonts_in_collection(&data).or(Some(1));
        if ours_n != theirs_n {
            t.parse_disagreements.push(format!(
                "{name}: faces: baud {ours_n:?} / ttf-parser {theirs_n:?}"
            ));
            continue;
        }
        for i in 0..ours_n.unwrap_or(0) {
            let face = format!("{name}#{i}");
            match pair(&data, i) {
                (Some(a), Some(b)) => {
                    t.faces += 1;
                    compare_face(&face, &a, &b, &mut t, &cps);
                }
                (None, None) => {}
                (a, b) => t.parse_disagreements.push(format!(
                    "{face}: parse: baud {} / ttf-parser {}",
                    a.is_some(),
                    b.is_some()
                )),
            }
        }
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
    // Two OFL faces, each with its licence beside it and each hash-pinned in
    // the research manifest; a test font that is not pinned is a byte string
    // nobody can re-fetch, and one whose bytes drift from the pin is a
    // different font wearing the name.
    let names: Vec<String> = committed()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, ["FiraCode-Regular.ttf", "JetBrainsMono-Regular.ttf"]);
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
    for (p, key) in committed()
        .iter()
        .zip(["font-fira-code-regular", "font-jetbrains-mono-regular"])
    {
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
