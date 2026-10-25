//! **Arabic letters joined as HarfBuzz joins them**, one letter a cell.
//!
//! `tests/shape/arabic.txt` is what hb-shape made of each word, in logical
//! order, with its script and direction guessed: HarfBuzz's Arabic shaper
//! picks each letter's form from its neighbours and forms the lam-alef
//! ligature. Here the word is joined by baud's own rules
//! ([`baud_domain::join`], `[[source: unicode-ch9]]` 9.2.2 and 9.2.3) and
//! each cell is shaped alone in the form they give it -- the lam-alef pair
//! together -- which is what a renderer draws, and the glyphs, their
//! clusters and their offsets must be hb-shape's. A face that joins through
//! `morx` (Geeza Pro) is drawn through the presentation forms its `cmap`
//! maps, and is held to HarfBuzz by outline rather than by glyph id: the
//! two name different glyphs that draw the same contours.

use baud_domain::{
    Classified, Codepoint, GlyphId, GraphemeClass, JoiningForm, JoiningType, Width, join, lam_alefs,
};
use baud_font::{
    Cluster, Face, FaceIndex, Features, Glyph, Ignorable, Script, Shaper, Sink, Unresolved, resolve,
};

struct HbGlyph {
    id: GlyphId,
    cluster: Cluster,
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
                cluster: Cluster(cluster.parse().unwrap()),
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

/// The cells a terminal makes of `cps`: a mark, a joiner or another
/// zero-width character joins the cell before it, as the core's cluster
/// machine and HarfBuzz's level 0 both do.
fn cells(cps: &[Codepoint]) -> Vec<core::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for i in 1..=cps.len() {
        let boundary = i == cps.len() || {
            let c = cps[i];
            let cl = Classified::of(c);
            let zero = cl.width(baud_domain::WidthPolicy::DEFAULT) == Width::ZERO;
            let extends = matches!(
                cl.class,
                GraphemeClass::Extend | GraphemeClass::Zwj | GraphemeClass::SpacingMark
            );
            !(zero || extends || Ignorable::of(c) == Ignorable::Yes)
        };
        if boundary {
            out.push(start..i);
            start = i;
        }
    }
    out
}

/// Renames the face's `GSUB` to `GSUZ`, as the generator did, so the
/// shaper meets a face with no `GSUB` forms.
fn rename_gsub(data: &mut [u8]) {
    let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let mut renamed = 0;
    for i in 0..n {
        let at = 12 + 16 * i;
        if &data[at..at + 4] == b"GSUB" {
            data[at..at + 4].copy_from_slice(b"GSUZ");
            renamed += 1;
        }
    }
    assert_eq!(renamed, 1, "no GSUB to rename");
}

/// A glyph's contours as drawn, for comparing two glyphs by what they
/// draw.
#[derive(Default, PartialEq, Debug)]
struct Contours(Vec<[i64; 7]>);

impl Sink for Contours {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push([0, x as i64, y as i64, 0, 0, 0, 0]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push([1, x as i64, y as i64, 0, 0, 0, 0]);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0
            .push([2, x1 as i64, y1 as i64, x as i64, y as i64, 0, 0]);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push([
            3, x1 as i64, y1 as i64, x2 as i64, y2 as i64, x as i64, y as i64,
        ]);
    }
    fn close(&mut self) {
        self.0.push([4, 0, 0, 0, 0, 0, 0]);
    }
}

fn contours(face: &Face<'_>, id: GlyphId) -> Contours {
    let mut c = Contours::default();
    face.outline(id, &mut c);
    c
}

/// One word joined and shaped cell by cell: each glyph's id, cell and
/// offset from its cell's origin, in logical order, and how many
/// lam-alef pairs were shaped together.
fn joined(face: &Face<'_>, cps: &[Codepoint]) -> (Vec<(GlyphId, Cluster, i32, i32)>, usize) {
    let types: Vec<JoiningType> = cps.iter().map(|c| JoiningType::of(c.value())).collect();
    let mut forms = vec![None; cps.len()];
    join(&types, &mut forms);
    let values: Vec<u32> = cps.iter().map(|c| c.value()).collect();
    let mut pairs = Vec::new();
    lam_alefs(&values, &forms, |lam, alef, _| pairs.push((lam, alef)));
    let cells = cells(cps);
    let unresolved: Vec<Unresolved> = cells
        .iter()
        .map(|r| Unresolved::of(&cps[r.clone()]))
        .collect();
    let mut settled = vec![Script::COMMON; cells.len()];
    resolve(&unresolved, &mut settled);
    let cell_of = |i: usize| cells.iter().position(|r| r.contains(&i)).unwrap();
    let mut out = Vec::new();
    let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 64];
    let push = |shaper: &mut Shaper<'_, '_, '_>, k: usize, r: &core::ops::Range<usize>| {
        let at = Cluster(u16::try_from(k).unwrap());
        match forms[r.start] {
            Some(f) => shaper.push_joined(&cps[r.clone()], at, f),
            None => shaper.push_cluster(&cps[r.clone()], at),
        }
        .unwrap();
    };
    let mut k = 0usize;
    let mut ligated = 0usize;
    while k < cells.len() {
        let pair = pairs
            .iter()
            .find(|&&(lam, alef)| cell_of(lam) == k && cell_of(alef) == k + 1);
        let mut shaper = Shaper::new(face, &mut storage);
        push(&mut shaper, k, &cells[k]);
        let take = match pair {
            Some(_) => {
                push(&mut shaper, k + 1, &cells[k + 1]);
                ligated += 1;
                2
            }
            None => 1,
        };
        let (buf, done) = shaper.finish(Features::JOINING, settled[k]);
        // A `morx` face's chains may hold subtables baud declines, which
        // `morx.txt`'s fixtures cover; every other face runs whole.
        assert!(
            done.declined.is_zero() || (face.morx().is_some() && face.gsub().is_none()),
            "{cps:04X?}: lookups declined"
        );
        for g in buf.as_slice() {
            out.push((g.id, g.cluster, i32::from(g.dx), i32::from(g.dy)));
        }
        k += take;
    }
    (out, ligated)
}

/// **Where baud departs from what hb-shape drew, and why**: each a face, a
/// word and the reason, held here so every other line is exact and these
/// fail if they ever agree. None is a departure from the Unicode Standard.
const DEPARTURES: [(&str, bool, &[u32], &str); 4] = [
    // HarfBuzz's fallback forms every ligature the face's `cmap` maps a
    // presentation form for; the standard makes lam-alef the only obligatory
    // one ("Many other Arabic ligatures are discretionary", 9.2.3), and the
    // grid draws no discretionary ligature (D8: `liga` off).
    (
        "Courier New",
        true,
        &[0x0645, 0x062D, 0x0645, 0x062F],
        "discretionary meem-hah",
    ),
    // Geeza Pro's `morx` forms the word ligature Allah from alef, lam, lam,
    // heh, which 9.2.11 says should not form "in the absence of the shadda
    // and the superscript alef".
    (
        "GeezaPro",
        false,
        &[0x0627, 0x0644, 0x0644, 0x0647],
        "the Allah ligature without shadda",
    ),
    // Geeza Pro's `morx` gives a final farsi yeh a contextual alternate; its
    // presentation form, U+FBFD, draws another outline.
    (
        "GeezaPro",
        false,
        &[0x0641, 0x0627, 0x0631, 0x0633, 0x06CC],
        "a contextual alternate of final farsi yeh",
    ),
    (
        "GeezaPro",
        false,
        &[
            0x0645, 0x06CC, 0x200C, 0x062E, 0x0648, 0x0627, 0x0647, 0x0645,
        ],
        "a contextual alternate of final farsi yeh, before ZWNJ",
    ),
];

/// Which way hb-shape laid a fixture file's words out: right to left,
/// its glyphs in visual order, or left to right, in logical order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Laid {
    RightToLeft,
    LeftToRight,
}

/// A count of lines, letters or ligatures a fixture file reached.
#[derive(Clone, Copy, Default, PartialEq, PartialOrd, Debug)]
struct Count(usize);

/// What one fixture file's lines reached.
#[derive(Default)]
struct Reached {
    compared: Count,
    by_outline: Count,
    fallback: Count,
    joined_forms: Count,
    ligatures: Count,
    departed: Count,
}

/// **Every line of `arabic.txt` joins as hb-shape joined it.** The macOS
/// faces' lines run on macOS only, as `script.txt`'s do; the built
/// Regular's run everywhere and are the control, a face with no Arabic.
#[test]
fn every_arabic_fixture_joins_as_harfbuzz_does() {
    let on_macos = cfg!(target_os = "macos");
    let r = joins_as_harfbuzz_does("arabic.txt", Laid::RightToLeft);
    let [
        compared,
        by_outline,
        fallback,
        joined_forms,
        ligatures,
        departed,
    ] = [
        r.compared,
        r.by_outline,
        r.fallback,
        r.joined_forms,
        r.ligatures,
        r.departed,
    ]
    .map(|c| c.0);
    // The fixtures reach what they are for: on macOS every face and the
    // fallback, joined letters and ligatures among them.
    match on_macos {
        true => {
            assert_eq!(departed, DEPARTURES.len(), "every departure is met");
            assert!(compared >= 190 && by_outline >= 30 && fallback >= 30);
            assert!(
                joined_forms >= 300 && ligatures >= 40,
                "{joined_forms} {ligatures}"
            );
        }
        false => assert!(compared >= 30),
    }
}

/// **Every line of `mongolian.txt` joins as hb-shape joined it**: Mongolian
/// and Phags-pa words on the macOS system faces Noto Sans Mongolian and
/// Noto Sans PhagsPa, which hb-shape lays out left to right, the first
/// letter initial and the last final. The built Regular's lines are the
/// control, a face with neither script.
#[test]
fn every_mongolian_and_phags_pa_fixture_joins_as_harfbuzz_does() {
    let r = joins_as_harfbuzz_does("mongolian.txt", Laid::LeftToRight);
    match cfg!(target_os = "macos") {
        true => assert!(
            r.compared.0 >= 25 && r.joined_forms.0 >= 30,
            "{:?} {:?}",
            r.compared,
            r.joined_forms
        ),
        false => assert!(r.compared.0 >= 10),
    }
    assert_eq!(r.departed, Count(0));
}

/// Every line of `file`, laid out by hb-shape the way `laid` says, joined
/// and shaped cell by cell and held to it.
fn joins_as_harfbuzz_does(file: &str, laid: Laid) -> Reached {
    let text =
        std::fs::read_to_string(format!("{}/tests/shape/{file}", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let on_macos = cfg!(target_os = "macos");
    let mut files: std::collections::HashMap<(String, bool), Vec<u8>> = Default::default();
    let mut mismatches = Vec::new();
    let (mut compared, mut by_outline, mut joined_forms, mut ligatures, mut fallback) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut departed = 0usize;
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        let (path, index, digest, features, cps, shaped) = (f[0], f[1], f[2], f[3], f[4], f[5]);
        let system = path.starts_with('/');
        if system && !on_macos {
            continue;
        }
        let renamed = features.contains("gsub=renamed");
        let data = files.entry((path.to_string(), renamed)).or_insert_with(|| {
            let full = if system {
                path.to_string()
            } else {
                format!("{root}/{path}")
            };
            let mut d = std::fs::read(&full).unwrap_or_else(|e| panic!("{full}: {e}"));
            assert_eq!(
                fnv64(&d),
                digest,
                "{path} is not the file the fixtures were shaped with"
            );
            if renamed {
                rename_gsub(&mut d);
            }
            d
        });
        let face = Face::parse_index(data, FaceIndex::of(index.parse().unwrap())).unwrap();
        let cps: Vec<Codepoint> = cps
            .split_whitespace()
            .map(|h| Codepoint::new(u32::from_str_radix(h, 16).unwrap()).unwrap())
            .collect();
        let (got, ligated) = joined(&face, &cps);
        // hb-shape's glyphs in visual order, put back in logical order, each
        // offset from its cluster's left edge, its cluster -- a code point
        // index -- named by the cell holding it: HarfBuzz keeps a ZWNJ a
        // cluster of its own where the terminal puts it in the cell before.
        let cell_starts: Vec<usize> = cells(&cps).iter().map(|r| r.start).collect();
        let cell_of = |c: Cluster| {
            let k = cell_starts
                .iter()
                .rposition(|&s| s <= usize::from(c.0))
                .unwrap();
            Cluster(u16::try_from(k).unwrap())
        };
        let mut expected = parse_positioned(shaped);
        for e in &mut expected {
            e.cluster = cell_of(e.cluster);
        }
        let mut placed: Vec<(&HbGlyph, i32)> = Vec::new();
        let mut pen = 0i32;
        for e in &expected {
            placed.push((e, pen));
            pen += e.ax;
        }
        if laid == Laid::RightToLeft {
            placed.reverse();
        }
        let mut left = std::collections::BTreeMap::new();
        for &(e, pen) in &placed {
            let l = left.entry(e.cluster).or_insert(pen);
            *l = (*l).min(pen);
        }
        let want: Vec<(GlyphId, Cluster, i32, i32)> = placed
            .iter()
            .map(|&(e, pen)| (e.id, e.cluster, pen + e.dx - left[&e.cluster], e.dy))
            .collect();
        let morx = face.morx().is_some() && face.gsub().is_none();
        // The control, a face without the word's script: every glyph a
        // `.notdef`, whose offsets say nothing -- a renderer leaves out the
        // marks a face cannot draw -- so its glyphs and cells are compared.
        let control = face.glyph(cps[0]).is_none();
        let same = match morx {
            _ if control => got
                .iter()
                .map(|g| (g.0, g.1))
                .eq(want.iter().map(|w| (w.0, w.1))),
            // By outline and cluster: the presentation form's glyph and the
            // one `morx` chose draw the same contours.
            true => {
                got.len() == want.len()
                    && got
                        .iter()
                        .zip(&want)
                        .all(|(g, w)| g.1 == w.1 && contours(&face, g.0) == contours(&face, w.0))
            }
            false => got == want,
        };
        let departs = DEPARTURES.iter().find(|&&(name, gsuz, word, _)| {
            path.contains(name)
                && gsuz == renamed
                && word.iter().copied().eq(cps.iter().map(|c| c.value()))
        });
        if let Some(&(_, _, _, why)) = departs {
            assert!(
                !same,
                "{path} {cps:04X?} now agrees; {why} is no longer a departure"
            );
            departed += 1;
            continue;
        }
        if !same {
            mismatches.push(format!(
                "{path}{} {cps:04X?}: baud {got:?} against hb-shape {want:?}",
                if renamed { " (GSUB renamed)" } else { "" }
            ));
        }
        compared += 1;
        by_outline += usize::from(morx);
        fallback += usize::from(renamed);
        ligatures += ligated;
        let types: Vec<JoiningType> = cps.iter().map(|c| JoiningType::of(c.value())).collect();
        let mut forms = vec![None; cps.len()];
        join(&types, &mut forms);
        joined_forms += forms
            .iter()
            .filter(|f| {
                matches!(
                    f,
                    Some(JoiningForm::Final | JoiningForm::Initial | JoiningForm::Medial)
                )
            })
            .count();
    }
    assert!(
        mismatches.is_empty(),
        "{} of {file}'s lines differ:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    Reached {
        compared: Count(compared),
        by_outline: Count(by_outline),
        fallback: Count(fallback),
        joined_forms: Count(joined_forms),
        ligatures: Count(ligatures),
        departed: Count(departed),
    }
}

/// **On Courier New a letter's `GSUB` form is the glyph its `cmap` maps the
/// presentation form to**, for every letter and form Presentation Forms-B
/// encodes that the face maps, for each letter that takes a form: the premise of the pixel oracle in
/// baud-present-macos, which draws the joined letters against the
/// presentation forms written out. macOS only, as the face is.
#[cfg(target_os = "macos")]
#[test]
fn courier_new_s_forms_are_its_presentation_form_glyphs() {
    use baud_domain::JoiningForm::*;
    let data = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf").unwrap();
    let face = Face::parse(&data).unwrap();
    let mut checked = 0usize;
    for letter in 0x0621u32..=0x064A {
        let c = Codepoint::new(letter).unwrap();
        // Hamza is non-joining: it takes no form, though U+FE80 names one.
        if !baud_domain::takes_form(letter) {
            continue;
        }
        for form in [Isolated, Final, Initial, Medial] {
            let Some(p) = baud_font::presentation_form(c, form)
                .filter(|p| (0xFE70..=0xFEFF).contains(&p.value()))
            else {
                continue;
            };
            let Some(want) = face.glyph(p) else {
                continue;
            };
            let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 4];
            let mut shaper = Shaper::new(&face, &mut storage);
            shaper.push_joined(&[c], Cluster(0), form).unwrap();
            let (buf, _) = shaper.finish(Features::JOINING, Script::of(c));
            assert_eq!(buf.as_slice()[0].id, want, "U+{letter:04X} {form:?}");
            checked += 1;
        }
    }
    assert!(checked >= 100, "{checked}");
}
