//! **Fonts by family name**: the installed faces, read once at launch, and
//! the face a family name and a style pick among them.
//!
//! **What is installed is what the font directories hold**: fontconfig's own
//! default directories for each system (`[[source: fontconfig-configure-2-15-0]]`:
//! on Darwin "/System/Library/Fonts,/Library/Fonts,~/Library/Fonts" and the
//! system's downloadable-font assets, elsewhere "/usr/share/fonts";
//! `[[source: fontconfig-fonts-conf-2-15-0]]` adds `<dir prefix="xdg">fonts</dir>`
//! and `~/.fonts`), each XDG data directory's `fonts`
//! (`[[source: xdg-basedir-0-8]]`), and the X11 font directories fontconfig adds
//! where they exist. Apple's Font Book puts a font a user installs in
//! `~/Library/Fonts` or `/Library/Fonts` (`[[source: apple-font-book-settings]]`).
//! Every face is read with baud-font's own reader ([`Described`]), nothing past
//! its directory and five small tables, from a mapping released as soon as it is
//! read; the files are split across the cores. A font Font Book has disabled is
//! still found -- the state is CoreText's, not the file's -- and so is nothing
//! CoreText reaches only through its private frameworks.
//!
//! **A family is matched as CSS Fonts matches one** (`[[source: css-fonts-3]]`
//! 5.1 and 5.2): the name against every family record of every face by default
//! caseless match, then the family's faces narrowed by width, then slope, then
//! weight, each by the text's search order. A variable face covers the range
//! its axes reach and is drawn at the value the search lands on, the reading
//! CSS Fonts 4 (`[[source: css-fonts-4-wd-2026-10-16]]` 5.2) gives: "the
//! algorithm proceeds as if each supported combination of values is a unique
//! font in the set". Nothing is synthesised: a family with no face nearer a
//! style than its regular one draws that style regular.
//!
//! **baud's own embedded faces answer to their names too, ahead of every
//! installed face**: the embedded font's four faces by its family's name
//! -- one family, the built italics carrying the built family's name
//! records -- and the icon and maths faces by theirs.

use crate::fontconf::{FaceView, FcIndex, Kept, Select};
use baud_font::var::{Fixed, Variation};
use baud_font::{Described, FaceIndex, Slope, Tag};
use std::path::{Path, PathBuf};

/// The deepest a directory walk goes below a font directory: fonts are
/// filed a few levels down (`/Library/Fonts/Nix Fonts/<store path>/share/
/// fonts/truetype/<family>`), and a symbolic link that loops back stops
/// here.
const DEPTH_MAX: Depth = Depth(10);

/// How far below a font directory a walk is.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Depth(usize);

/// A range of values a face covers on one scale: one value for a static
/// face, an axis's minimum to maximum for a variable one.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Span {
    lo: f32,
    hi: f32,
}

impl Span {
    const fn at(v: f32) -> Span {
        Span { lo: v, hi: v }
    }

    fn holds(self, v: f32) -> bool {
        self.lo <= v && v <= self.hi
    }
}

/// How a slope is measured: CSS Fonts 4 keeps an italic scale and an
/// oblique angle apart, and a normal face is oblique 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scale {
    Italic,
    Oblique,
}

/// One axis of a variable face and the range it covers.
#[derive(Clone, Copy, Debug)]
struct Axis {
    tag: Tag,
    span: Span,
}

/// Where a face's bytes are: a file on disk, or compiled into the binary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    File,
    Embedded(&'static [u8]),
}

/// An installed or embedded face: where it is, its family names and its
/// style. An embedded face's `path` is only its name for the log.
#[derive(Clone, Debug)]
pub struct Known {
    path: String,
    origin: Origin,
    index: FaceIndex,
    /// The family the module's order in baud-font's `name.rs` prefers,
    /// nameID 16 else 1: what a face named by its path is a face of.
    family: Option<String>,
    names: Vec<String>,
    /// Its full name (nameID 4, US English else the first) and PostScript
    /// name (nameID 6), the names that pick it alone.
    full: Option<String>,
    postscript: Option<String>,
    width: Span,
    weight: Span,
    slopes: Vec<(Scale, Span)>,
    axes: Vec<Axis>,
}

impl Known {
    /// The face `index` of `data`, read from `path` or compiled in.
    fn read(path: &str, origin: Origin, data: &[u8], index: FaceIndex) -> Option<Known> {
        let d = Described::of(data, index)?;
        let names: Vec<String> = d
            .families()
            .filter_map(|f| {
                let mut out = [0u8; 256];
                f.utf8(&mut out)
                    .map(|b| String::from_utf8_lossy(b).into_owned())
            })
            .collect();
        if names.is_empty() {
            return None;
        }
        let text = |f: baud_font::Family<'_>| {
            let mut out = [0u8; 256];
            f.utf8(&mut out)
                .map(|b| String::from_utf8_lossy(b).into_owned())
        };
        let family = d.family().and_then(text);
        let full = d.full_name().and_then(text);
        let postscript = d.postscript_name().and_then(text);
        let axis = |tag: &[u8; 4]| {
            d.axis(Tag::from_bytes(*tag)).map(|a| Axis {
                tag: a.tag,
                span: Span {
                    lo: a.min.to_f32(),
                    hi: a.max.to_f32(),
                },
            })
        };
        let axes: Vec<Axis> = [b"wght", b"wdth", b"ital", b"slnt"]
            .into_iter()
            .filter_map(axis)
            .collect();
        let on = |tag: &[u8; 4]| {
            axes.iter()
                .find(|a| a.tag == Tag::from_bytes(*tag))
                .map(|a| a.span)
        };
        let width = on(b"wdth").unwrap_or(Span::at(d.width().get()));
        let weight = on(b"wght").unwrap_or(Span::at(d.weight().get()));
        let mut slopes = Vec::new();
        match (on(b"ital"), on(b"slnt")) {
            (None, None) => slopes.push(match d.slope() {
                Slope::Upright => (Scale::Oblique, Span::at(0.0)),
                Slope::Italic => (Scale::Italic, Span::at(1.0)),
                Slope::Oblique(a) => (Scale::Oblique, Span::at(a.get())),
            }),
            (ital, slnt) => {
                if let Some(s) = ital {
                    slopes.push((Scale::Italic, s));
                    if s.lo <= 0.0 && slnt.is_none() {
                        slopes.push((Scale::Oblique, Span::at(0.0)));
                    }
                }
                // `slnt` is counter-clockwise degrees; CSS's oblique angle
                // leans clockwise, so the range turns over.
                if let Some(s) = slnt {
                    slopes.push((
                        Scale::Oblique,
                        Span {
                            lo: -s.hi,
                            hi: -s.lo,
                        },
                    ));
                }
            }
        }
        Some(Known {
            path: path.to_string(),
            origin,
            index,
            family,
            names,
            full,
            postscript,
            width,
            weight,
            slopes,
            axes,
        })
    }

    /// Whether `name` is one of its family names, by default caseless
    /// match (`[[source: css-fonts-3]]` 5.1).
    fn answers_to(&self, name: &str) -> bool {
        self.names.iter().any(|n| same(n, name))
    }

    /// Whether `name` is its PostScript name.
    fn postscript_is(&self, name: &str) -> bool {
        self.postscript.as_deref().is_some_and(|p| same(p, name))
    }

    /// Whether `name` is its full name and that is none of its own family
    /// names (nameID 16 or 1), which a name finds as the family: kitty
    /// passes a full name over that way ("IBM Plex Mono has fullname of
    /// regular face == family_name", `[[source: kitty-fonts-fontconfig-fa9f8da]]`).
    /// nameID 1 counts too: a weight-split family's "Seravek Light" is
    /// its Light face's full name and its nameID 1, and as a family it
    /// names the Light faces only, where the face would bring Seravek's.
    fn full_name_is(&self, name: &str) -> bool {
        self.full
            .as_deref()
            .is_some_and(|f| same(f, name) && !self.answers_to(f))
    }
}

/// Two names compared by default caseless match, no normalisation
/// (`[[source: css-fonts-3]]` 5.1). CSS states it for family names; for a
/// full or PostScript name, which `local()` matches with no case rule
/// stated, it is a reading.
fn same(a: &str, b: &str) -> bool {
    baud_domain::caseless_eq(a.chars(), b.chars())
}

/// The style a cell asks its face for: upright or italic, regular or
/// bold. CSS's `font-style: normal | italic` and `font-weight: 400 | 700`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Want {
    pub italic: Italic,
    pub weight: baud_font::Weight,
}

/// Whether a style asks for italic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Italic {
    No,
    Yes,
}

impl Want {
    pub const REGULAR: Want = Want {
        italic: Italic::No,
        weight: baud_font::Weight::REGULAR,
    };
    pub const BOLD: Want = Want {
        italic: Italic::No,
        weight: baud_font::Weight::BOLD,
    };
    pub const ITALIC: Want = Want {
        italic: Italic::Yes,
        weight: baud_font::Weight::REGULAR,
    };
    pub const BOLD_ITALIC: Want = Want {
        italic: Italic::Yes,
        weight: baud_font::Weight::BOLD,
    };
}

/// The face a family name and a style picked, and the axis values it is
/// drawn at.
#[derive(Clone, PartialEq, Debug)]
pub struct Picked {
    pub path: String,
    pub origin: Origin,
    pub index: FaceIndex,
    pub variations: Vec<Variation>,
}

/// One step of a search along a scale: from `from`, upward or downward,
/// as far as `to` (excluded downward, included upward).
#[derive(Clone, Copy)]
enum Step {
    Up { from: f32, to: f32 },
    Down { from: f32, past: f32 },
}

/// The first value `step` meets that one of `spans` covers.
fn first(spans: impl Iterator<Item = Span>, step: Step) -> Option<f32> {
    let mut best: Option<f32> = None;
    for s in spans {
        match step {
            Step::Up { from, to } => {
                let v = s.lo.max(from);
                if s.hi >= from && v <= to && best.is_none_or(|b| v < b) {
                    best = Some(v);
                }
            }
            Step::Down { from, past } => {
                let v = s.hi.min(from);
                if s.lo <= from && v > past && best.is_none_or(|b| v > b) {
                    best = Some(v);
                }
            }
        }
    }
    best
}

/// The value `steps` land on first, in order.
fn search(spans: &[Span], steps: &[Step]) -> Option<f32> {
    steps.iter().find_map(|&s| first(spans.iter().copied(), s))
}

const INF: f32 = f32::INFINITY;

/// `[[source: css-fonts-4-wd-2026-10-16]]` 5.2's width search for 100 %:
/// "If the desired width value is less than or equal to 100%, width values
/// below the desired width value are checked in descending order followed
/// by width values above the desired width value in ascending order".
const WIDTH_STEPS: [Step; 2] = [
    Step::Down {
        from: 100.0,
        past: -INF,
    },
    Step::Up {
        from: 100.0,
        to: INF,
    },
];

/// Its weight search: "If the desired weight is inclusively between 400
/// and 500, weights greater than or equal to the target weight are checked
/// in ascending order until 500 is hit and checked, followed by weights
/// less than the target weight in descending order, followed by weights
/// greater than 500 ... If the desired weight is greater than 500, weights
/// greater than or equal to the desired weight are checked in ascending
/// order followed by weights below the desired weight in descending order".
/// CSS Fonts 3 gives the same faces for static weights.
fn weight_steps(w: f32) -> Vec<Step> {
    match w {
        w if (400.0..=500.0).contains(&w) => vec![
            Step::Up { from: w, to: 500.0 },
            Step::Down {
                from: w,
                past: -INF,
            },
            Step::Up {
                from: 500.0,
                to: INF,
            },
        ],
        w if w > 500.0 => vec![
            Step::Up { from: w, to: INF },
            Step::Down {
                from: w,
                past: -INF,
            },
        ],
        w => vec![
            Step::Down {
                from: w,
                past: -INF,
            },
            Step::Up { from: w, to: INF },
        ],
    }
}

/// Its style search, in its order, each stage on one scale. Italic:
/// italic values from 1 up then down to 0 (excluded), then "oblique values
/// greater than or equal to 11deg ... in ascending order followed by
/// oblique values below 11deg in descending order, until 0 is hit", then
/// italic and oblique values at or below 0. Normal: "Oblique values greater
/// than or equal to 0 are checked in ascending order. If no match is found,
/// italic values greater than or equal to 0 are checked in ascending",
/// then the negative values of each.
fn slope_steps(italic: Italic) -> Vec<(Scale, Step)> {
    match italic {
        Italic::Yes => vec![
            (Scale::Italic, Step::Up { from: 1.0, to: INF }),
            (
                Scale::Italic,
                Step::Down {
                    from: 1.0,
                    past: 0.0,
                },
            ),
            (
                Scale::Oblique,
                Step::Up {
                    from: 11.0,
                    to: INF,
                },
            ),
            (
                Scale::Oblique,
                Step::Down {
                    from: 11.0,
                    past: 0.0,
                },
            ),
            (
                Scale::Italic,
                Step::Down {
                    from: 0.0,
                    past: -INF,
                },
            ),
            (
                Scale::Oblique,
                Step::Down {
                    from: 0.0,
                    past: -INF,
                },
            ),
        ],
        Italic::No => vec![
            (Scale::Oblique, Step::Up { from: 0.0, to: INF }),
            (Scale::Italic, Step::Up { from: 0.0, to: INF }),
            (
                Scale::Oblique,
                Step::Down {
                    from: 0.0,
                    past: -INF,
                },
            ),
            (
                Scale::Italic,
                Step::Down {
                    from: 0.0,
                    past: -INF,
                },
            ),
        ],
    }
}

/// **The face of `family` CSS 5.2 picks for `want`**, and its axis values;
/// `None` when `family` has no face. Ties keep the earliest face, so the
/// catalogue's order decides between two copies of one face.
pub fn pick(family: &[&Known], want: Want) -> Option<Picked> {
    let mut set: Vec<&Known> = family.to_vec();
    let widths: Vec<Span> = set.iter().map(|k| k.width).collect();
    let width = search(&widths, &WIDTH_STEPS)?;
    set.retain(|k| k.width.holds(width));
    let mut slope = None;
    for (scale, step) in slope_steps(want.italic) {
        let spans = set
            .iter()
            .flat_map(|k| k.slopes.iter().filter(|s| s.0 == scale).map(|s| s.1));
        if let Some(v) = first(spans, step) {
            slope = Some((scale, v));
            break;
        }
    }
    let (scale, value) = slope?;
    set.retain(|k| k.slopes.iter().any(|s| s.0 == scale && s.1.holds(value)));
    let weights: Vec<Span> = set.iter().map(|k| k.weight).collect();
    let weight = search(&weights, &weight_steps(want.weight.get()))?;
    let k = set.into_iter().find(|k| k.weight.holds(weight))?;
    let variations = k
        .axes
        .iter()
        .map(|a| {
            let is = |t: &[u8; 4]| a.tag == Tag::from_bytes(*t);
            let v = match () {
                _ if is(b"wght") => weight,
                _ if is(b"wdth") => width,
                _ if is(b"ital") && scale == Scale::Italic => value,
                _ if is(b"slnt") && scale == Scale::Oblique => -value,
                _ => 0.0,
            };
            Variation {
                tag: a.tag,
                value: Fixed::from_f32(v.clamp(a.span.lo, a.span.hi)),
            }
        })
        .collect();
    Some(Picked {
        path: k.path.clone(),
        origin: k.origin,
        index: k.index,
        variations,
    })
}

/// The installed faces, in the order their directories were walked.
pub struct Catalogue {
    faces: Vec<Known>,
}

impl Catalogue {
    /// baud's embedded faces, then the faces under `dirs`, each walked in
    /// name order, earlier directories first; a directory that does not
    /// exist adds nothing.
    #[cfg(test)]
    pub fn scan(dirs: &[PathBuf]) -> Catalogue {
        Catalogue::scan_selected(dirs, &Select::default())
    }

    /// [`Catalogue::scan`], keeping of what `dirs` hold what `select`
    /// keeps: fontconfig's `<selectfont>` rules ([`crate::fontconf::Select`]).
    pub fn scan_selected(dirs: &[PathBuf], select: &Select) -> Catalogue {
        let mut faces = embedded();
        faces.extend(Catalogue::read(dirs, DEPTH_MAX, select).faces);
        Catalogue { faces }
    }

    /// The faces under `dirs` that `select` keeps, walked to `max` levels
    /// below each.
    fn read(dirs: &[PathBuf], max: Depth, select: &Select) -> Catalogue {
        let mut files = Vec::new();
        let mut seen = Seen::default();
        for d in dirs {
            walk(d, Depth(0), max, &mut seen, &mut files, select);
        }
        let threads = std::thread::available_parallelism()
            .map_or(1, std::num::NonZero::get)
            .clamp(1, 8);
        // Dealt out a file at a time rather than cut into runs, so a run of
        // large collections does not land on one thread; put back in the
        // walk's order after.
        let files = &files;
        let mut read: Vec<(usize, Vec<Known>)> = std::thread::scope(|s| {
            let parts: Vec<_> = (0..threads)
                .map(|t| {
                    s.spawn(move || {
                        (t..files.len())
                            .step_by(threads)
                            .map(|i| (i, read_selected(&files[i], select)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            parts
                .into_iter()
                .flat_map(|h| h.join().unwrap_or_default())
                .collect()
        });
        read.sort_by_key(|r| r.0);
        Catalogue {
            faces: read.into_iter().flat_map(|r| r.1).collect(),
        }
    }

    /// The faces of the system's font directories ([`system_dirs`]),
    /// scanned once per process; on Linux, with fontconfig's
    /// `<selectfont>` rules applied to what they hold.
    pub fn system() -> &'static Catalogue {
        static SYSTEM: std::sync::OnceLock<Catalogue> = std::sync::OnceLock::new();
        SYSTEM.get_or_init(|| {
            let (dirs, select) = system_config();
            Catalogue::scan_selected(&dirs, &select)
        })
    }

    /// The faces whose family names include `name`.
    pub fn family(&self, name: &str) -> Vec<&Known> {
        self.faces.iter().filter(|k| k.answers_to(name)).collect()
    }

    /// **What a `fonts` name names** (kitty's order,
    /// `[[source: kitty-fonts-core-text-fa9f8da]]` `find_best_match`):
    /// the first face whose PostScript name it is, else the first whose
    /// full name it is (but for a full name that is one of the face's
    /// own family names), else the faces of the family it is. A unique
    /// name "specifies a single font, not an entire font family"
    /// (`[[source: css-fonts-3]]` 4.3).
    pub fn lookup(&self, name: &str) -> Named<'_> {
        match self.lookup_at(name) {
            NamedAt::Face(i) => Named::Face(&self.faces[i]),
            NamedAt::Family(f) => Named::Family(f.into_iter().map(|i| &self.faces[i]).collect()),
            NamedAt::Nothing => Named::Nothing,
        }
    }

    /// [`Catalogue::lookup`] by each face's place in the catalogue, for a
    /// caller that keeps its own record of each face: a program's
    /// `OSC 50`, which names a face among those loaded (`fonts.rs`).
    pub fn lookup_at(&self, name: &str) -> NamedAt {
        if let Some(i) = self.faces.iter().position(|k| k.postscript_is(name)) {
            return NamedAt::Face(i);
        }
        if let Some(i) = self.faces.iter().position(|k| k.full_name_is(name)) {
            return NamedAt::Face(i);
        }
        let f: Vec<usize> = (0..self.faces.len())
            .filter(|&i| self.faces[i].answers_to(name))
            .collect();
        match f.is_empty() {
            true => NamedAt::Nothing,
            false => NamedAt::Family(f),
        }
    }

    /// **The faces already in memory, in their order**, each `(bytes,
    /// index)`, as the config's lookup reads them, and each kept face's
    /// place in `faces`: a face whose `name` table names no family is left
    /// out, as a scan leaves it out.
    pub fn of_loaded(faces: &[(&'static [u8], FaceIndex)]) -> (Catalogue, Vec<usize>) {
        let mut kept = Vec::new();
        let mut at = Vec::new();
        for (i, &(data, index)) in faces.iter().enumerate() {
            if let Some(k) = Known::read("loaded", Origin::Embedded(data), data, index) {
                kept.push(k);
                at.push(i);
            }
        }
        (Catalogue { faces: kept }, at)
    }

    /// **The faces of `k`'s family**: of its preferred family name
    /// (nameID 16 else 1), so a face named alone brings its family's
    /// other styles.
    pub fn family_of(&self, k: &Known) -> Vec<&Known> {
        k.family
            .as_deref()
            .map_or_else(Vec::new, |f| self.family(f))
    }

    /// **The faces of the font a config names by path**: its file and the
    /// font files beside it in its own directory, not below it, read on
    /// their own -- a font named by path need not be in any font directory,
    /// and the directory may be the home folder -- and the faces here, kept
    /// where they are of its family.
    pub fn beside(&self, path: &str, index: FaceIndex) -> Beside {
        let p = Path::new(path);
        // A font named by path is opened as named, as fontconfig's
        // `<selectfont>` does not reach a file an application opens.
        let near = Catalogue::read(
            p.parent().map(Path::to_path_buf).as_slice(),
            Depth(0),
            &Select::default(),
        );
        let own = read_file(p);
        let named = own.iter().find(|k| k.index == index);
        let family = named.and_then(|k| k.family.clone());
        let regular = regular_in(&own, index);
        let faces = match &family {
            Some(f) => near
                .faces
                .into_iter()
                .chain(own)
                .chain(self.faces.iter().cloned())
                .filter(|k| k.answers_to(f))
                .collect(),
            None => Vec::new(),
        };
        Beside { faces, regular }
    }
}

/// **The face of the file at `path` whose PostScript name is `postscript`,
/// at the axis values CSS 5.2's search gives the regular style**: the face
/// the system answered for a character ([`crate::fallback`]), as baud's own
/// reader reads it; `None` when the file has no such face.
#[cfg(target_os = "macos")]
pub fn named_in(path: &str, postscript: &str) -> Option<Picked> {
    let faces = read_file(Path::new(path));
    let k = faces.iter().find(|k| k.postscript_is(postscript))?;
    regular_in(&faces, k.index)
}

impl Catalogue {
    /// **fontconfig's charset match over the installed faces**: for each of
    /// `wanted`, the faces whose `cmap` maps it -- fontconfig scores the
    /// characters of a pattern's charset a font lacks, `FcCompareCharSet`,
    /// ahead of every tier such a pattern sets
    /// (`[[source: fontconfig-fcmatch-2-15-0]]`, `PRI_CHARSET`) -- then,
    /// among those, one of `family` (the primary's, fontconfig's strong
    /// family tier), then the face nearest the regular style (its slant,
    /// weight and width tiers, in that order), which a fallback face is
    /// drawn as, rather than the primary's own style, then the one the walk
    /// met first. fontconfig's lang tier and its aliases' weak family tier
    /// are not taken. Each face that maps a wanted character no earlier
    /// pick maps is returned once, in the order of the first character it
    /// is picked for; baud's embedded faces are not the system's and are
    /// passed over.
    #[cfg(any(not(target_os = "macos"), test))]
    pub fn covering(&self, family: Option<&str>, wanted: &[baud_domain::Codepoint]) -> Vec<Picked> {
        let mut ranked: Vec<(usize, Vec<bool>)> = Vec::new();
        for (i, k) in self.faces.iter().enumerate() {
            if k.origin != Origin::File {
                continue;
            }
            let Some(m) = Mapped::open(Path::new(&k.path)) else {
                continue;
            };
            let Ok(face) = baud_font::Face::parse_index(m.bytes(), k.index) else {
                continue;
            };
            let maps: Vec<bool> = wanted.iter().map(|&c| face.glyph(c).is_some()).collect();
            if maps.contains(&true) {
                ranked.push((i, maps));
            }
        }
        let regular = |k: &Known| {
            let slant = k
                .slopes
                .iter()
                .any(|(scale, span)| *scale == Scale::Oblique && span.holds(0.0));
            let off = |s: Span, v: f32| match s.holds(v) {
                true => 0.0,
                false => (s.lo - v).abs().min((s.hi - v).abs()),
            };
            (!slant, off(k.weight, 400.0), off(k.width, 100.0))
        };
        ranked.sort_by(|(a, _), (b, _)| {
            let (ka, kb) = (&self.faces[*a], &self.faces[*b]);
            let fam = |k: &Known| !family.is_some_and(|f| k.answers_to(f));
            let (ra, rb) = (regular(ka), regular(kb));
            fam(ka)
                .cmp(&fam(kb))
                .then(ra.0.cmp(&rb.0))
                .then(ra.1.total_cmp(&rb.1))
                .then(ra.2.total_cmp(&rb.2))
                .then(a.cmp(b))
        });
        let mut picked: Vec<usize> = Vec::new();
        for w in 0..wanted.len() {
            let covered = picked
                .iter()
                .any(|p| ranked.iter().any(|(i, maps)| i == p && maps[w]));
            if covered {
                continue;
            }
            if let Some((i, _)) = ranked.iter().find(|(_, maps)| maps[w]) {
                picked.push(*i);
            }
        }
        picked
            .into_iter()
            .filter_map(|i| regular_in(core::slice::from_ref(&self.faces[i]), self.faces[i].index))
            .collect()
    }
}

/// **Face `index` of the file at `path` at the axis values CSS 5.2's search
/// gives the regular style**: for a variable face, `wght` 400 where its
/// axis reaches it, as the face named by its family is drawn; `None` when
/// the file has no such face. Reads that one file and nothing beside it.
pub fn regular_of(path: &str, index: FaceIndex) -> Option<Picked> {
    regular_in(&read_file(Path::new(path)), index)
}

/// Face `index` of `faces`, one file's, picked for the regular style.
fn regular_in(faces: &[Known], index: FaceIndex) -> Option<Picked> {
    let k = faces.iter().find(|k| k.index == index)?;
    pick(&[k], Want::REGULAR)
}

/// What a `fonts` name found ([`Catalogue::lookup`]).
pub enum Named<'c> {
    /// One face, by its PostScript or full name.
    Face(&'c Known),
    /// A family's faces, by one of its family names.
    Family(Vec<&'c Known>),
    Nothing,
}

/// What a name found ([`Catalogue::lookup_at`]), by place in the
/// catalogue.
pub enum NamedAt {
    Face(usize),
    Family(Vec<usize>),
    Nothing,
}

/// The faces of a font named by path's family ([`Catalogue::beside`]).
pub struct Beside {
    faces: Vec<Known>,
    regular: Option<Picked>,
}

impl Beside {
    pub fn faces(&self) -> Vec<&Known> {
        self.faces.iter().collect()
    }

    /// **The named face at the axis values CSS 5.2's search gives the
    /// regular style**: a variable face at `wght` 400 where its axis
    /// reaches it, as the same face named by its family is drawn, and not
    /// at its `fvar` default; `None` when the file has no such face.
    pub fn regular(&self) -> Option<&Picked> {
        self.regular.as_ref()
    }
}

/// Whether `p` names a font file by its extension.
fn is_font(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["ttf", "otf", "ttc", "otc"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}

/// The directories a scan has read, by device and inode: a directory
/// reached twice -- through a symbolic link, or named twice under two
/// spellings -- is read the first time only, so a tie between its copies
/// keeps the earlier directory's.
#[derive(Default)]
struct Seen(std::collections::HashSet<(u64, u64)>);

impl Seen {
    /// Whether `dir` is read now: it exists and has not been read before.
    fn first(&mut self, dir: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(dir).is_ok_and(|m| self.0.insert((m.dev(), m.ino())))
    }
}

/// The font files under `dir`, in name order, following symbolic links
/// (Home Manager links its fonts), to `max` levels below, each directory
/// once ([`Seen`]); a file or a directory below `dir` that `select`'s globs
/// refuse is left out, by the path the walk spells, `dir`, `/` and the
/// name, as `FcConfigAddCache` tests each font file and subdirectory
/// (`[[source: fontconfig-fccfg-2-15-0]]`). `dir` itself is not tested:
/// fontconfig tests no configured directory.
fn walk(
    dir: &Path,
    depth: Depth,
    max: Depth,
    seen: &mut Seen,
    out: &mut Vec<PathBuf>,
    select: &Select,
) {
    if !seen.first(dir) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    // The directory's own entry types, which need no `stat`; only a
    // symbolic link is followed to what it names.
    let mut entries: Vec<(PathBuf, Option<std::fs::FileType>)> = rd
        .flatten()
        .map(|e| (e.path(), e.file_type().ok()))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (p, kind) in entries {
        let kind = match kind {
            Some(k) if k.is_symlink() => std::fs::metadata(&p).ok().map(|m| m.file_type()),
            k => k,
        };
        if select.path(&p) == Kept::No {
            continue;
        }
        match kind {
            Some(k) if k.is_dir() && depth < max => {
                walk(&p, Depth(depth.0 + 1), max, seen, out, select);
            }
            Some(k) if k.is_file() && is_font(&p) => out.push(p),
            _ => {}
        }
    }
}

/// **baud's embedded faces**, in the order a tie between two faces is
/// decided, so ahead of any installed copy: the embedded font's Regular,
/// Bold, Italic and Bold Italic, then the icon face and STIX Two Math.
fn embedded() -> Vec<Known> {
    use crate::embedded::{BOLD, BOLD_ITALIC, ITALIC, REGULAR, STIX, SYMBOLS};
    [REGULAR, BOLD, ITALIC, BOLD_ITALIC, SYMBOLS, STIX]
        .into_iter()
        .filter_map(|e| Known::read(e.name, Origin::Embedded(e.bytes), e.bytes, FaceIndex::FIRST))
        .collect()
}

/// Every face of the file at `p` that `select`'s patterns keep
/// ([`Select::face`] over [`FaceView::of`]), read only when a pattern asks.
fn read_selected(p: &Path, select: &Select) -> Vec<Known> {
    if !select.tests_faces() {
        return read_file(p);
    }
    let Some(path) = p.to_str() else {
        return Vec::new();
    };
    let Some(map) = Mapped::open(p) else {
        return Vec::new();
    };
    let data = map.bytes();
    let n = baud_font::faces(data).map_or(0, |n| n.value());
    (0..n)
        .filter(|&i| {
            Described::of(data, FaceIndex::of(i)).is_some_and(|d| {
                select.face(&FaceView::of(&d, path, FcIndex(i64::from(i)))) == Kept::Yes
            })
        })
        .filter_map(|i| Known::read(path, Origin::File, data, FaceIndex::of(i)))
        .collect()
}

/// Every face of the file at `p`, or none when it does not map or is not
/// a font.
fn read_file(p: &Path) -> Vec<Known> {
    let Some(path) = p.to_str() else {
        return Vec::new();
    };
    let Some(map) = Mapped::open(p) else {
        return Vec::new();
    };
    let data = map.bytes();
    let n = baud_font::faces(data).map_or(0, |n| n.value());
    (0..n)
        .filter_map(|i| Known::read(path, Origin::File, data, FaceIndex::of(i)))
        .collect()
}

/// A read-only mapping of a whole file, released when dropped: a scan maps
/// hundreds of files and keeps none.
struct Mapped {
    ptr: *mut core::ffi::c_void,
    len: crate::sys::MapLen,
}

impl Mapped {
    fn open(p: &Path) -> Option<Mapped> {
        use std::os::fd::AsRawFd;
        let file = std::fs::File::open(p).ok()?;
        let len = usize::try_from(file.metadata().ok()?.len()).ok()?;
        if len == 0 {
            return None;
        }
        // SAFETY: a private read-only mapping of the whole file at offset
        // zero; the kernel checks the length against the file, and the
        // mapping keeps its own reference after `file` closes.
        let ptr = unsafe {
            crate::sys::mmap(
                std::ptr::null_mut(),
                len,
                crate::sys::PROT_READ,
                crate::sys::MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };
        (ptr != crate::sys::MAP_FAILED).then_some(Mapped {
            ptr,
            len: crate::sys::MapLen(len),
        })
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: `ptr` is a live read-only mapping of exactly `len` bytes,
        // released only when `self` drops, after every borrow of it.
        unsafe { std::slice::from_raw_parts(self.ptr.cast::<u8>(), self.len.0) }
    }
}

impl Drop for Mapped {
    fn drop(&mut self) {
        // SAFETY: the mapping `open` made, unmapped once.
        unsafe { crate::sys::munmap(self.ptr, self.len) };
    }
}

/// **The font directories, in the order a tie between two copies of one
/// face is decided**. On macOS the user's own first, then the machine's,
/// then the system's. Elsewhere, the directories fontconfig's
/// configuration names, in its order ([`crate::fontconf`]), and where no
/// configuration is found, [`xdg_dirs`]'s list.
#[cfg(test)]
pub fn system_dirs() -> Vec<PathBuf> {
    system_config().0
}

/// [`system_dirs`] and, on Linux where fontconfig's configuration is
/// found, its `<selectfont>` rules; elsewhere rules that keep everything.
fn system_config() -> (Vec<PathBuf>, Select) {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .filter(|h| h.is_absolute());
    if cfg!(target_os = "macos") {
        let mut assets = Vec::new();
        for root in ["/System/Library/AssetsV2", "/System/Library/Assets"] {
            if let Ok(rd) = std::fs::read_dir(root) {
                assets.extend(rd.flatten().map(|e| e.path()));
            }
        }
        (mac_dirs(home, assets), Select::default())
    } else {
        // fontconfig's own list where its configuration is found; the
        // fixed list where there is none.
        match crate::fontconf::load(&crate::fontconf::Env::from_process()) {
            Some(c) => (c.dirs, c.select),
            None => (
                xdg_dirs(
                    home,
                    std::env::var_os("XDG_DATA_HOME"),
                    std::env::var_os("XDG_DATA_DIRS"),
                ),
                Select::default(),
            ),
        }
    }
}

/// The macOS directories: Font Book's two install locations, the system's
/// fonts, and of `assets` (what the system's two asset roots hold) the
/// downloadable-font directories, whose name carries a version
/// (`com_apple_MobileAsset_Font8` on this release; fontconfig's list names
/// the `Font3` and `Font4` of older ones).
fn mac_dirs(home: Option<PathBuf>, mut assets: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = home.iter().map(|h| h.join("Library/Fonts")).collect();
    out.push("/Library/Fonts".into());
    out.push("/System/Library/Fonts".into());
    assets.retain(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("com_apple_MobileAsset_Font"))
    });
    assets.sort();
    out.extend(assets);
    out
}

/// The Linux directories: `$XDG_DATA_HOME/fonts` (default
/// `~/.local/share/fonts`) and `~/.fonts`, then each `$XDG_DATA_DIRS`
/// entry's `fonts` and `X11/fonts` (default `/usr/local/share:/usr/share`;
/// NixOS files its system fonts under `X11/fonts`), then fontconfig's own
/// `/usr/share/fonts` and `/usr/local/share/fonts` and the X11 font
/// directories it adds where they exist, and NixOS's system font
/// directory, which the Linux default stack named by path before. An
/// empty or relative variable is ignored, as the XDG specification says.
fn xdg_dirs(
    home: Option<PathBuf>,
    data_home: Option<std::ffi::OsString>,
    data_dirs: Option<std::ffi::OsString>,
) -> Vec<PathBuf> {
    let absolute = |p: PathBuf| p.is_absolute().then_some(p);
    let data_home = data_home
        .map(PathBuf::from)
        .and_then(absolute)
        .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let mut out: Vec<PathBuf> = data_home.iter().map(|d| d.join("fonts")).collect();
    out.extend(home.iter().map(|h| h.join(".fonts")));
    let dirs: Vec<PathBuf> = match data_dirs.filter(|d| !d.is_empty()) {
        Some(d) => std::env::split_paths(&d).filter_map(absolute).collect(),
        None => vec!["/usr/local/share".into(), "/usr/share".into()],
    };
    for d in dirs {
        out.push(d.join("fonts"));
        out.push(d.join("X11/fonts"));
    }
    for x in [
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/usr/X11R6/lib/X11/fonts",
        "/usr/X11/lib/X11/fonts",
        "/usr/lib/X11/fonts",
        "/run/current-system/sw/share/X11/fonts",
    ] {
        out.push(x.into());
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

    /// A face of family `name` covering `weight` and `slope`, at `path`.
    fn face(path: &str, weight: Span, slopes: Vec<(Scale, Span)>) -> Known {
        Known {
            path: path.to_string(),
            origin: Origin::File,
            index: FaceIndex::FIRST,
            family: Some("Test Family".into()),
            names: vec!["Test Family".into()],
            full: None,
            postscript: None,
            width: Span::at(100.0),
            weight,
            slopes,
            axes: Vec::new(),
        }
    }

    fn upright() -> Vec<(Scale, Span)> {
        vec![(Scale::Oblique, Span::at(0.0))]
    }

    fn italic() -> Vec<(Scale, Span)> {
        vec![(Scale::Italic, Span::at(1.0))]
    }

    fn picked(family: &[Known], want: Want) -> Option<String> {
        let refs: Vec<&Known> = family.iter().collect();
        pick(&refs, want).map(|p| p.path)
    }

    /// **A name finds its family's faces by default caseless match, and
    /// the regular style picks the Regular**: the four static JetBrains
    /// Mono files baud carries, read from disk as a font directory is.
    #[test]
    fn a_family_name_picks_its_regular_face() {
        let cat = Catalogue::scan(&[PathBuf::from(format!(
            "{ROOT}/baud-app/fonts/jetbrains-mono"
        ))]);
        let family = cat.family("jetbrains MONO");
        assert_eq!(family.len(), 4, "the four styles");
        let p = pick(&family, Want::REGULAR).expect("a face");
        assert!(p.path.ends_with("/JetBrainsMono-Regular.ttf"), "{}", p.path);
        assert!(p.variations.is_empty(), "a static face");
        assert!(cat.family("JetBrains").is_empty(), "only whole");
        assert!(cat.family("No Such Family").is_empty());
    }

    /// **fontconfig's charset match over a font directory**, the Linux
    /// system tier, on every OS from a fixture directory: FiraCode, then
    /// JetBrains Mono's italic, then its Regular. A character only
    /// JetBrains Mono maps picks the Regular (the charset tier, and the
    /// regular style over the italic); `a`, which all three map, picks
    /// FiraCode, the first upright one the walk met -- and not baud's
    /// embedded faces, which the scan lists first and which are not the
    /// system's -- unless JetBrains Mono is the primary's family, which
    /// picks its Regular. Asked together, each picked face is returned once,
    /// in the order of the first character it is picked for.
    #[test]
    fn the_charset_match_picks_a_face_per_character() {
        let dir = std::env::temp_dir().join(format!("baud-covering-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (to, from) in [
            ("a.ttf", "baud-font/tests/fonts/FiraCode-Regular.ttf"),
            (
                "b.ttf",
                "baud-font/tests/fonts/JetBrainsMono-Italic-Variable.ttf",
            ),
            ("c.ttf", "baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        ] {
            std::fs::copy(format!("{ROOT}/{from}"), dir.join(to)).unwrap();
        }
        let cat = Catalogue::scan(core::slice::from_ref(&dir));
        let bytes = |n: &str| std::fs::read(dir.join(n)).unwrap();
        let (fira, italic, jbm) = (bytes("a.ttf"), bytes("b.ttf"), bytes("c.ttf"));
        let maps = |b: &[u8], c: baud_domain::Codepoint| {
            baud_font::Face::parse(b).unwrap().glyph(c).is_some()
        };
        let only = (0x80..0x3000)
            .filter_map(baud_domain::Codepoint::new)
            .find(|&c| maps(&jbm, c) && maps(&italic, c) && !maps(&fira, c))
            .expect("a character JetBrains Mono maps and FiraCode does not");
        let a = baud_domain::Codepoint::new(0x61).unwrap();
        let file = |p: &[Picked]| -> Vec<String> {
            p.iter()
                .map(|p| p.path.rsplit('/').next().unwrap().to_string())
                .collect()
        };
        assert_eq!(file(&cat.covering(None, &[only])), ["c.ttf"]);
        assert_eq!(file(&cat.covering(None, &[a])), ["a.ttf"]);
        assert_eq!(file(&cat.covering(Some("jetbrains mono"), &[a])), ["c.ttf"]);
        assert_eq!(file(&cat.covering(None, &[a, only, a])), ["a.ttf", "c.ttf"]);
        let none = baud_domain::Codepoint::new(0x0378).unwrap();
        assert!(
            cat.covering(None, &[none]).is_empty(),
            "nothing maps U+0378"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The charset match finds Arabic and Syriac faces in a directory
    /// that holds them**, macOS's own files copied into a fixture directory
    /// so the Linux path runs headless here: beh from Courier New, alaph and
    /// beth from Noto Sans Syriac, each face returned once.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_charset_match_finds_arabic_and_syriac() {
        let dir = std::env::temp_dir().join(format!("baud-covering-rtl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["Courier New.ttf", "NotoSansSyriac-Regular.ttf"] {
            std::fs::copy(
                format!("/System/Library/Fonts/Supplemental/{f}"),
                dir.join(f),
            )
            .expect("a face macOS ships; this test cannot skip itself");
        }
        let cat = Catalogue::scan(core::slice::from_ref(&dir));
        let cps: Vec<baud_domain::Codepoint> = [0x0628, 0x0710, 0x0712]
            .into_iter()
            .filter_map(baud_domain::Codepoint::new)
            .collect();
        let found: Vec<String> = cat
            .covering(Some("JetBrains Mono"), &cps)
            .iter()
            .map(|p| p.path.rsplit('/').next().unwrap().to_string())
            .collect();
        assert_eq!(found, ["Courier New.ttf", "NotoSansSyriac-Regular.ttf"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **CSS 5.2's weight search**, on the families it gives as examples
    /// and the two it is easy to get wrong: 400 checks 500 first and then
    /// goes down before it goes up (Hiragino Sans GB's W3 at 300, not its
    /// W6 at 600); a weight above 500 goes up first, then down.
    #[test]
    fn the_weight_search_is_the_texts() {
        let fam = [
            face("w300", Span::at(300.0), upright()),
            face("w600", Span::at(600.0), upright()),
        ];
        assert_eq!(picked(&fam, Want::REGULAR).as_deref(), Some("w300"));
        assert_eq!(picked(&fam, Want::BOLD).as_deref(), Some("w600"));
        let fam = [
            face("w400", Span::at(400.0), upright()),
            face("w700", Span::at(700.0), upright()),
            face("w900", Span::at(900.0), upright()),
        ];
        assert_eq!(picked(&fam, Want::REGULAR).as_deref(), Some("w400"));
        assert_eq!(picked(&fam, Want::BOLD).as_deref(), Some("w700"));
        let fam = [
            face("w300", Span::at(300.0), upright()),
            face("w500", Span::at(500.0), upright()),
        ];
        assert_eq!(
            picked(&fam, Want::REGULAR).as_deref(),
            Some("w500"),
            "500 first"
        );
        let fam = [
            face("w800", Span::at(800.0), upright()),
            face("w900", Span::at(900.0), upright()),
        ];
        assert_eq!(
            picked(&fam, Want::BOLD).as_deref(),
            Some("w800"),
            "up first"
        );
        assert_eq!(
            picked(&fam, Want::REGULAR).as_deref(),
            Some("w800"),
            "then up past 500"
        );
    }

    /// **Width first, then slope, then weight**: a condensed face never
    /// wins over a normal one, whatever its weight; an italic face is
    /// passed over for an upright request while an upright one exists,
    /// and taken when none does.
    #[test]
    fn width_then_slope_then_weight() {
        let mut narrow = face("narrow-400", Span::at(400.0), upright());
        narrow.width = Span::at(75.0);
        let fam = [
            narrow.clone(),
            face("normal-700", Span::at(700.0), upright()),
        ];
        assert_eq!(picked(&fam, Want::REGULAR).as_deref(), Some("normal-700"));
        assert_eq!(
            picked(&[narrow], Want::REGULAR).as_deref(),
            Some("narrow-400")
        );
        let fam = [
            face("italic-400", Span::at(400.0), italic()),
            face("upright-700", Span::at(700.0), upright()),
        ];
        assert_eq!(picked(&fam, Want::REGULAR).as_deref(), Some("upright-700"));
        assert_eq!(picked(&fam, Want::ITALIC).as_deref(), Some("italic-400"));
        assert_eq!(
            picked(&fam, Want::BOLD_ITALIC).as_deref(),
            Some("italic-400"),
            "style before weight"
        );
        let only = [face("italic", Span::at(400.0), italic())];
        assert_eq!(picked(&only, Want::REGULAR).as_deref(), Some("italic"));
        let obl = [
            face("upright", Span::at(400.0), upright()),
            face(
                "oblique",
                Span::at(400.0),
                vec![(Scale::Oblique, Span::at(12.0))],
            ),
        ];
        assert_eq!(
            picked(&obl, Want::ITALIC).as_deref(),
            Some("oblique"),
            "oblique before normal"
        );
        assert_eq!(picked(&obl, Want::REGULAR).as_deref(), Some("upright"));
    }

    /// **What the Linux scan costs, headless, on a fixture tree**: a
    /// `fonts.conf` with a `conf.d` of 20 files (each a `<dir>` and a
    /// `<selectfont>` rule, a glob and a pattern), and 360 font files, the
    /// nine under `baud-font/tests/fonts` linked into 40 directories. Each
    /// round reads the configuration (`fontconf::load`), walks the
    /// directories and reads every face on the scan's threads, once with
    /// the rules and once without; 11 rounds, the first not counted. Run by
    /// hand (`--ignored --nocapture`) for the record; it asserts the counts.
    #[test]
    #[ignore]
    fn measure_the_linux_scan_on_a_fixture_tree() {
        let root = std::env::temp_dir().join(format!("baud-b155-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("etc/conf.d")).unwrap();
        let root = root.canonicalize().unwrap();
        let src = PathBuf::from(format!("{ROOT}/baud-font/tests/fonts"));
        let mut fonts: Vec<PathBuf> = std::fs::read_dir(&src)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_font(p))
            .collect();
        fonts.sort();
        assert_eq!(fonts.len(), 9);
        for d in 0..40 {
            let dir = root.join(format!("fonts/d{d:02}"));
            std::fs::create_dir_all(&dir).unwrap();
            for f in &fonts {
                std::os::unix::fs::symlink(f, dir.join(f.file_name().unwrap())).unwrap();
            }
        }
        let r = root.display();
        std::fs::write(
            root.join("etc/fonts.conf"),
            format!("<fontconfig><dir>{r}/fonts</dir><include>conf.d</include></fontconfig>"),
        )
        .unwrap();
        for i in 0..20 {
            std::fs::write(
                root.join(format!("etc/conf.d/{i:02}-rule.conf")),
                format!(
                    "<fontconfig><dir>{r}/fonts/d{i:02}</dir><selectfont><rejectfont>\
                     <glob>{r}/fonts/none-{i}/*</glob><pattern><patelt name=\"family\">\
                     <string>No Such Family {i}</string></patelt></pattern></rejectfont>\
                     </selectfont></fontconfig>"
                ),
            )
            .unwrap();
        }
        let env = crate::fontconf::Env {
            home: None,
            xdg_data_home: None,
            xdg_data_dirs: None,
            xdg_config_home: None,
            file: None,
            path: None,
            config_dir: root.join("etc"),
            cwd: Some(root.clone()),
        };
        let mut rows: Vec<[f64; 5]> = Vec::new();
        for round in 0..11 {
            let t = std::time::Instant::now();
            let c = crate::fontconf::load(&env).unwrap();
            let conf = t.elapsed();
            let t = std::time::Instant::now();
            let mut files = Vec::new();
            let mut seen = Seen::default();
            for d in &c.dirs {
                walk(d, Depth(0), DEPTH_MAX, &mut seen, &mut files, &c.select);
            }
            let walked = t.elapsed();
            let t = std::time::Instant::now();
            let ruled = Catalogue::read(&c.dirs, DEPTH_MAX, &c.select).faces.len();
            let with_rules = t.elapsed();
            let t = std::time::Instant::now();
            let plain = Catalogue::read(&c.dirs, DEPTH_MAX, &Select::default())
                .faces
                .len();
            let without = t.elapsed();
            assert_eq!(
                c.dirs.len(),
                21,
                "the tree and twenty of its directories, each walked once"
            );
            assert_eq!(files.len(), 360);
            assert_eq!((ruled, plain), (ruled, ruled));
            let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
            println!(
                "round {round}: config {:.3} ms, walk {:.3} ms ({} files), \
                 walk+read with rules {:.2} ms, without {:.2} ms ({ruled} faces)",
                ms(conf),
                ms(walked),
                files.len(),
                ms(with_rules),
                ms(without)
            );
            if round > 0 {
                rows.push([ms(conf), ms(walked), ms(with_rules), ms(without), 0.0]);
            }
        }
        for (i, name) in ["config", "walk", "walk+read, rules", "walk+read, none"]
            .iter()
            .enumerate()
        {
            let mut v: Vec<f64> = rows.iter().map(|r| r[i]).collect();
            v.sort_by(f64::total_cmp);
            println!(
                "{name}: median {:.3} ms, range {:.3}-{:.3}",
                v[v.len() / 2],
                v[0],
                v[v.len() - 1]
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **fontconfig's `<selectfont>` keeps a face out of the catalogue**,
    /// on a tree of three fonts read through `fontconf::load`: a
    /// `<rejectfont>` glob on a subdirectory drops what is below it, and a
    /// pattern on a family drops that family's face, its name matched
    /// without spaces or case. A glob equal to the configured
    /// directory drops nothing, as fontconfig tests no configured
    /// directory; one that ends in `*` below it drops all of it.
    #[test]
    fn the_scan_keeps_what_selectfont_keeps() {
        let root = std::env::temp_dir().join(format!("baud-b155-sel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fonts = root.join("fonts");
        std::fs::create_dir_all(fonts.join("keep")).unwrap();
        std::fs::create_dir_all(fonts.join("drop")).unwrap();
        let fonts = fonts.canonicalize().unwrap();
        let src = |n: &str| PathBuf::from(format!("{ROOT}/baud-font/tests/fonts/{n}"));
        let link = |n: &str, at: PathBuf| std::os::unix::fs::symlink(src(n), at).unwrap();
        link(
            "JetBrainsMono-Regular.ttf",
            fonts.join("keep/JetBrainsMono-Regular.ttf"),
        );
        link(
            "JetBrainsMono-Variable.ttf",
            fonts.join("drop/JetBrainsMono-Variable.ttf"),
        );
        link("FiraCode-Regular.ttf", fonts.join("FiraCode-Regular.ttf"));
        let f = fonts.display();
        let files = |rules: &str| -> Vec<String> {
            let conf = root.join("fonts.conf");
            std::fs::write(
                &conf,
                format!("<fontconfig><dir>{f}</dir>{rules}</fontconfig>"),
            )
            .unwrap();
            let env = crate::fontconf::Env {
                home: None,
                xdg_data_home: None,
                xdg_data_dirs: None,
                xdg_config_home: None,
                file: Some(conf.into()),
                path: None,
                config_dir: root.join("etc"),
                cwd: Some(root.clone()),
            };
            let c = crate::fontconf::load(&env).unwrap();
            let cat = Catalogue::scan_selected(&c.dirs, &c.select);
            let mut v: Vec<String> = cat
                .faces
                .iter()
                .filter(|k| k.origin == Origin::File)
                .map(|k| k.path.replace(&f.to_string(), "F"))
                .collect();
            v.sort();
            v
        };
        let all = [
            "F/FiraCode-Regular.ttf",
            "F/drop/JetBrainsMono-Variable.ttf",
            "F/keep/JetBrainsMono-Regular.ttf",
        ];
        // Positive control: with no rules every face is in.
        assert_eq!(files(""), all);
        let rules = format!(
            "<selectfont><rejectfont><glob>{f}/drop</glob><pattern>\
             <patelt name=\"family\"><string>firacode</string></patelt></pattern></rejectfont></selectfont>"
        );
        assert_eq!(files(&rules), ["F/keep/JetBrainsMono-Regular.ttf"]);
        let own = format!("<selectfont><rejectfont><glob>{f}</glob></rejectfont></selectfont>");
        assert_eq!(files(&own), all, "the configured directory is not tested");
        let below = format!("<selectfont><rejectfont><glob>{f}/*</glob></rejectfont></selectfont>");
        assert!(files(&below).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **A variable face covers its axis and is drawn at the value the
    /// search lands on**: FiraCode's wght runs 300 to 700, so regular is
    /// 400 and bold 700, from the one file.
    #[test]
    fn a_variable_face_is_drawn_at_the_searched_weight() {
        let cat = Catalogue::scan(&[PathBuf::from(format!("{ROOT}/baud-font/tests/fonts"))]);
        let fira = cat.family("Fira Code");
        assert!(!fira.is_empty());
        let wght = Tag::from_bytes(*b"wght");
        for (want, at) in [(Want::REGULAR, 400.0), (Want::BOLD, 700.0)] {
            let p = pick(&fira, want).expect("a face");
            let vf: Vec<_> = p.variations.iter().filter(|v| v.tag == wght).collect();
            if p.path.ends_with("FiraCode-VF.ttf") {
                assert_eq!(vf.len(), 1, "{p:?}");
                assert_eq!(vf[0].value, Fixed::from_f32(at), "{p:?}");
            } else {
                assert!(
                    p.path.ends_with("FiraCode-Regular.ttf") && at == 400.0,
                    "{p:?}"
                );
            }
        }
        let vf_only: Vec<&Known> = fira
            .iter()
            .copied()
            .filter(|k| k.path.ends_with("FiraCode-VF.ttf"))
            .collect();
        let bold = pick(&vf_only, Want::BOLD).unwrap();
        assert_eq!(
            bold.variations,
            vec![Variation {
                tag: wght,
                value: Fixed::from_f32(700.0)
            }]
        );
    }

    /// **A folder reached twice is read once**: through a symbolic link
    /// beside it, or named twice in the list, JetBrains Mono's Regular is
    /// one face, and the copy kept is the first walked.
    #[test]
    fn a_folder_reached_twice_is_read_once() {
        let dir = std::env::temp_dir().join(format!("baud-b145-seen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("b-fonts")).unwrap();
        std::fs::copy(
            format!("{ROOT}/baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"),
            dir.join("b-fonts/JetBrainsMono-Regular.ttf"),
        )
        .unwrap();
        std::os::unix::fs::symlink(dir.join("b-fonts"), dir.join("a-link")).unwrap();
        let none = Catalogue::scan(&[]).faces.len();
        let cat = Catalogue::scan(&[dir.clone(), dir.join("b-fonts")]);
        assert_eq!(cat.faces.len() - none, 1, "one face");
        let jb = cat.family("JetBrains Mono");
        assert_eq!(jb.len(), 1);
        assert!(jb[0].path.contains("/a-link/"), "{}", jb[0].path);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// **The directories, in order**: on macOS the user's, the machine's,
    /// the system's and the downloadable-font assets; elsewhere the XDG
    /// data directories, with the specification's defaults for a variable
    /// that is unset, empty or relative, and fontconfig's own after them.
    #[test]
    fn the_font_directories_in_order() {
        let home = Some(PathBuf::from("/home/u"));
        let mac = mac_dirs(
            home.clone(),
            vec![
                "/System/Library/AssetsV2/com_apple_MobileAsset_Font8".into(),
                "/System/Library/AssetsV2/com_apple_MobileAsset_Other".into(),
            ],
        );
        let s = |v: &[PathBuf]| {
            v.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            s(&mac),
            [
                "/home/u/Library/Fonts",
                "/Library/Fonts",
                "/System/Library/Fonts",
                "/System/Library/AssetsV2/com_apple_MobileAsset_Font8"
            ]
        );
        let xdg = xdg_dirs(home.clone(), None, None);
        assert_eq!(
            s(&xdg)[..6],
            [
                "/home/u/.local/share/fonts",
                "/home/u/.fonts",
                "/usr/local/share/fonts",
                "/usr/local/share/X11/fonts",
                "/usr/share/fonts",
                "/usr/share/X11/fonts",
            ]
        );
        let xdg = xdg_dirs(
            home,
            Some("rel".into()),
            Some("/run/sw/share:rel:/usr/share".into()),
        );
        assert_eq!(
            s(&xdg)[0],
            "/home/u/.local/share/fonts",
            "a relative home is ignored"
        );
        assert_eq!(
            s(&xdg)[2..4],
            ["/run/sw/share/fonts", "/run/sw/share/X11/fonts"]
        );
        assert!(
            s(&xdg).contains(&"/usr/local/share/fonts".to_string()),
            "fontconfig's own"
        );
        assert_eq!(
            xdg.len(),
            xdg.iter().collect::<std::collections::HashSet<_>>().len()
        );
    }

    /// **What resolving the default stack costs at launch**: the scan of
    /// the system's font directories and the picks, as `Config::launch` runs
    /// them, eleven times in one process, each scan made afresh. Run by
    /// hand (`--ignored --nocapture`) for the record; it asserts only that
    /// every default name was found.
    #[test]
    #[ignore]
    fn measure_the_launch_resolution() {
        let dirs = system_dirs();
        for round in 0..11 {
            let t = std::time::Instant::now();
            let cat = Catalogue::scan(&dirs);
            let scanned = t.elapsed();
            let mut c = crate::config::Config::defaults();
            let said = c.resolve(&cat);
            let all = t.elapsed();
            assert!(said.is_empty(), "{said:?}");
            assert!(
                c.faces
                    .iter()
                    .all(|f| f.from != crate::config::FaceFrom::Family)
            );
            println!(
                "round {round}: {} faces, scan {:.2} ms, scan and picks {:.2} ms",
                cat.faces.len(),
                scanned.as_secs_f64() * 1e3,
                all.as_secs_f64() * 1e3
            );
        }
        let cat = Catalogue::scan(&dirs);
        for path in [
            "/System/Library/Fonts/Menlo.ttc@0",
            "/System/Library/Fonts/SFNSMono.ttf",
        ] {
            for _ in 0..3 {
                let t = std::time::Instant::now();
                let mut c = crate::config::Config::defaults();
                assert!(c.apply(&format!("fonts = [\"{path}\"]\n")).is_empty());
                let said = c.resolve(&cat);
                println!(
                    "{path} named first: its family's styles in {:.2} ms ({said:?})",
                    t.elapsed().as_secs_f64() * 1e3
                );
            }
        }
        let t = std::time::Instant::now();
        let mut files = Vec::new();
        let mut seen = Seen::default();
        for d in &dirs {
            walk(
                d,
                Depth(0),
                DEPTH_MAX,
                &mut seen,
                &mut files,
                &Select::default(),
            );
        }
        let walked = t.elapsed();
        let n: usize = files.iter().map(|p| read_file(p).len()).sum();
        println!(
            "of which: the walk {:.2} ms ({} files); the reads, one thread, {:.2} ms ({n} faces)",
            walked.as_secs_f64() * 1e3,
            files.len(),
            (t.elapsed() - walked).as_secs_f64() * 1e3
        );
    }

    /// **The default stack's names pick the faces its paths named**, on
    /// the system's own fonts (macOS only, as the shaping fixtures run its
    /// faces): Menlo's Regular, not the hidden `.Apple Color Emoji UI` of
    /// the same collection, and Hiragino Sans GB's W3 at 300 rather than
    /// its W6.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_default_names_pick_the_default_paths() {
        let cat = Catalogue::system();
        for (name, path, index) in [
            ("Menlo", "/System/Library/Fonts/Menlo.ttc", 0),
            (
                "Apple Color Emoji",
                "/System/Library/Fonts/Apple Color Emoji.ttc",
                0,
            ),
            (
                "Apple Symbols",
                "/System/Library/Fonts/Apple Symbols.ttf",
                0,
            ),
            (
                "Hiragino Sans GB",
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
                0,
            ),
            (
                "Apple SD Gothic Neo",
                "/System/Library/Fonts/AppleSDGothicNeo.ttc",
                0,
            ),
            (
                "Kohinoor Devanagari",
                "/System/Library/Fonts/Kohinoor.ttc",
                0,
            ),
        ] {
            let p = pick(&cat.family(name), Want::REGULAR).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(
                (p.path.as_str(), p.index),
                (path, FaceIndex::of(index)),
                "{name}"
            );
        }
        assert!(
            cat.family(".Apple Color Emoji UI").len() == 1,
            "named only by its own name"
        );
        let localized =
            pick(&cat.family("冬青黑体简体中文"), Want::REGULAR).expect("a localized name");
        assert!(
            localized.path.ends_with("/Hiragino Sans GB.ttc"),
            "{localized:?}"
        );
    }

    /// **The system's faces by PostScript and full name** (macOS only):
    /// Menlo's Bold by `Menlo-Bold`, its Bold Italic by "menlo bold
    /// italic", each one face of the collection; "Apple Color Emoji" is
    /// also that face's full name, which is its family's own, so it is
    /// found as the family and picks what it picked before.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_faces_by_postscript_and_full_name() {
        let cat = Catalogue::system();
        let face = |name: &str| match cat.lookup(name) {
            Named::Face(k) => Some((k.path.clone(), k.index)),
            _ => None,
        };
        let menlo = "/System/Library/Fonts/Menlo.ttc".to_string();
        assert_eq!(face("Menlo-Bold"), Some((menlo.clone(), FaceIndex::of(1))));
        assert_eq!(face("menlo bold italic"), Some((menlo, FaceIndex::of(3))));
        assert!(face("Apple Color Emoji").is_none(), "a family");
        assert!(matches!(cat.lookup("Apple Color Emoji"), Named::Family(_)));
        assert!(matches!(cat.lookup("No Such Face-Bold"), Named::Nothing));
    }
}
