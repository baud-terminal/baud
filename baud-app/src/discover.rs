//! **Fonts by family name**: the installed faces, read once at launch, and
//! the face a family name and a style pick among them.
//!
//! **What is installed is what the font directories hold** (Jev's lean,
//! D_SCAN, 0.72/0.85 then 0.89/0.89): fontconfig's own default directories
//! for each system (`[[source: fontconfig-configure-2-15-0]]`: on Darwin
//! "/System/Library/Fonts,/Library/Fonts,~/Library/Fonts" and the system's
//! downloadable-font assets, elsewhere "/usr/share/fonts";
//! `[[source: fontconfig-fonts-conf-2-15-0]]` adds `<dir prefix="xdg">fonts</dir>`
//! and `~/.fonts`), each XDG data directory's `fonts`
//! (`[[source: xdg-basedir-0-8]]`), and the X11 font directories fontconfig
//! adds where they exist. Apple's Font Book puts a font a user installs in
//! `~/Library/Fonts` or `/Library/Fonts` (`[[source: apple-font-book-settings]]`).
//! Every face is read with baud-font's own reader ([`Described`]), nothing
//! past its directory and five small tables, from a mapping released as
//! soon as it is read; the files are split across the cores. A font Font
//! Book has disabled is still found -- the state is CoreText's, not the
//! file's -- and so is nothing CoreText reaches only through its private
//! frameworks.
//!
//! **A family is matched as CSS Fonts matches one** (`[[source: css-fonts-3]]`
//! 5.1 and 5.2): the name against every family record of every face by
//! default caseless match, then the family's faces narrowed by width, then
//! slope, then weight, each by the text's search order. A variable face
//! covers the range its axes reach and is drawn at the value the search
//! lands on (Jev, V_AXIS, 0.94/0.96), the reading CSS Fonts 4
//! (`[[source: css-fonts-4-wd-2026-10-16]]` 5.2) gives: "the algorithm
//! proceeds as if each supported combination of values is a unique font in
//! the set". Nothing is synthesised (Jev, R_REGULAR, 0.98/0.98): a family
//! with no face nearer a style than its regular one draws that style
//! regular.

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

/// An installed face: where it is, its family names and its style.
#[derive(Clone, Debug)]
pub struct Known {
    path: String,
    index: FaceIndex,
    names: Vec<String>,
    width: Span,
    weight: Span,
    slopes: Vec<(Scale, Span)>,
    axes: Vec<Axis>,
}

impl Known {
    /// The face `index` of `data`, read from `path`.
    fn read(path: &str, data: &[u8], index: FaceIndex) -> Option<Known> {
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
            index,
            names,
            width,
            weight,
            slopes,
            axes,
        })
    }

    /// Whether `name` is one of its family names, by default caseless
    /// match (`[[source: css-fonts-3]]` 5.1).
    fn answers_to(&self, name: &str) -> bool {
        self.names
            .iter()
            .any(|n| baud_domain::caseless_eq(n.chars(), name.chars()))
    }
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "only tests ask for a styled face yet")
    )]
    pub const BOLD: Want = Want {
        italic: Italic::No,
        weight: baud_font::Weight::BOLD,
    };
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "only tests ask for a styled face yet")
    )]
    pub const ITALIC: Want = Want {
        italic: Italic::Yes,
        weight: baud_font::Weight::REGULAR,
    };
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "only tests ask for a styled face yet")
    )]
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
        index: k.index,
        variations,
    })
}

/// The installed faces, in the order their directories were walked.
pub struct Catalogue {
    faces: Vec<Known>,
}

impl Catalogue {
    /// The faces under `dirs`, each walked in name order, earlier
    /// directories first; a directory that does not exist adds nothing.
    pub fn scan(dirs: &[PathBuf]) -> Catalogue {
        let mut files = Vec::new();
        for d in dirs {
            walk(d, Depth(0), &mut files);
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
                            .map(|i| (i, read_file(&files[i])))
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
    /// scanned once per process.
    pub fn system() -> &'static Catalogue {
        static SYSTEM: std::sync::OnceLock<Catalogue> = std::sync::OnceLock::new();
        SYSTEM.get_or_init(|| Catalogue::scan(&system_dirs()))
    }

    /// The faces whose family names include `name`.
    pub fn family(&self, name: &str) -> Vec<&Known> {
        self.faces.iter().filter(|k| k.answers_to(name)).collect()
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

/// The font files under `dir`, in name order, following symbolic links
/// (Home Manager links its fonts) to [`DEPTH_MAX`].
fn walk(dir: &Path, depth: Depth, out: &mut Vec<PathBuf>) {
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
        match kind {
            Some(k) if k.is_dir() && depth < DEPTH_MAX => walk(&p, Depth(depth.0 + 1), out),
            Some(k) if k.is_file() && is_font(&p) => out.push(p),
            _ => {}
        }
    }
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
        .filter_map(|i| Known::read(path, data, FaceIndex::of(i)))
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
/// face is decided**: the user's own first, then the machine's, then the
/// system's.
pub fn system_dirs() -> Vec<PathBuf> {
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
        mac_dirs(home, assets)
    } else {
        xdg_dirs(
            home,
            std::env::var_os("XDG_DATA_HOME"),
            std::env::var_os("XDG_DATA_DIRS"),
        )
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
            index: FaceIndex::FIRST,
            names: vec!["Test Family".into()],
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

    /// **A variable face covers its axis and is drawn at the value the
    /// search lands on** (Jev, V_AXIS): FiraCode's wght runs 300 to 700, so
    /// regular is 400 and bold 700, from the one file.
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
    /// the system's font directories and the picks, as `Config::load` runs
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
        let t = std::time::Instant::now();
        let mut files = Vec::new();
        for d in &dirs {
            walk(d, Depth(0), &mut files);
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
}
