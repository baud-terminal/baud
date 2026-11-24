//! **A rate only where baud-core and ghostty end on the same screen.**
//!
//! Before a recording is timed, it is fed once through both, from memory
//! in the reads the rates use and to the same total, and both final
//! screens are read back:
//!
//! * every cell: blank, a wide character's tail, or its base character,
//!   read as `ghostty-oracle` reads them;
//! * every kitty placement anchored on the screen: its cell, its `z`, and
//!   its image's size and pixels;
//! * every virtual placement (`U=1`) either arm names: by its client id,
//!   from ghostty's placements and from the image ids baud-core's
//!   placeholder cells carry, each looked up on both arms.
//!
//! Pixels are read as `0xAARRGGBB` words and hashed. An image in a format
//! this does not read is compared by its size, and the report says how
//! many were. A recording whose screens differ is not timed, prints no
//! rate, and fails the run.
//!
//! The same rep counts what each arm stored ([`crate::counts`]): a
//! recording whose arms stored different images, frames or pixel bytes
//! is not timed either. One whose compared count is "n/a" on an arm is
//! UNEQUAL-UNKNOWN: timed, its rate printed unquotable, and it fails the
//! run. The counters are printed beside the rates.
//!
//! The same rep tracks each arm's images from the read at which each was
//! first seen, and checks the order each arm evicted them in
//! ([`crate::counts::Track`]). An arm that evicts out of the kitty quota's
//! order is not timed. Where the orders hold with a case, what each arm
//! holds and evicted stops deciding, the frames are compared image by
//! image, and a placement one arm shows may be missing on the other only
//! where the other evicted its image.

// Without libghostty-vt there is no second arm to compare, so `check`
// returns nothing and what it would build goes unused.
#![cfg_attr(not(have_ghostty), allow(dead_code))]

use crate::counts::{Counter, Counts, Eviction, Left, Rule, Verdict, judged};
#[cfg(have_ghostty)]
use crate::transport::through_memory;
use crate::{COLS, ROWS};

/// How many differing cells the report names.
const SHOWN: usize = 5;

/// One cell's character, as both arms are read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Glyph {
    Blank,
    WideTail,
    Char(u32),
}

impl std::fmt::Display for Glyph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Glyph::Blank => write!(f, "blank"),
            Glyph::WideTail => write!(f, "wide tail"),
            Glyph::Char(cp) => match char::from_u32(*cp).filter(|c| !c.is_control()) {
                Some(c) => write!(f, "U+{cp:04X} {c:?}"),
                None => write!(f, "U+{cp:04X}"),
            },
        }
    }
}

/// An image as an arm reports it: its size, and a hash of its pixels as
/// `0xAARRGGBB` words, `None` where the arm's format is not one read here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Picture {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) pixels: Option<u64>,
}

impl Picture {
    /// The same size, and the same pixels where both were read.
    fn same(self, other: Picture) -> bool {
        self.width == other.width
            && self.height == other.height
            && match (self.pixels, other.pixels) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
    }

    fn unread(self) -> bool {
        self.pixels.is_none()
    }
}

impl std::fmt::Display for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.pixels {
            Some(h) => write!(f, "{}x{} #{h:016x}", self.width, self.height),
            None => write!(f, "{}x{} (pixels not read)", self.width, self.height),
        }
    }
}

/// A placement anchored on the screen: its anchor cell, its `z` and its
/// image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Placed {
    pub(crate) row: usize,
    pub(crate) col: usize,
    pub(crate) z: i32,
    pub(crate) picture: Picture,
    /// The read at which the arm first saw its image; `None` for an image
    /// it does not track.
    pub(crate) first: Option<usize>,
}

impl Placed {
    /// The same cell, `z`, picture and first read.
    fn matches(&self, other: &Placed) -> bool {
        (self.row, self.col, self.z, self.first) == (other.row, other.col, other.z, other.first)
            && self.picture.same(other.picture)
    }
}

/// Whether every placement of `ours` and `theirs` is matched on the other
/// arm ([`Placed::matches`]), or is of an image first seen at a read the
/// other arm evicted an image from: `ours_evicted` and `theirs_evicted`
/// are the reads of each arm's evictions.
fn match_apart_from_evicted(
    ours: &[Placed],
    theirs: &[Placed],
    ours_evicted: &[usize],
    theirs_evicted: &[usize],
) -> bool {
    let mut rest: Vec<Option<&Placed>> = theirs.iter().map(Some).collect();
    for a in ours {
        match rest.iter_mut().find(|b| b.is_some_and(|b| a.matches(b))) {
            Some(b) => *b = None,
            None if a.first.is_some_and(|f| theirs_evicted.contains(&f)) => {}
            None => return false,
        }
    }
    rest.into_iter()
        .flatten()
        .all(|b| b.first.is_some_and(|f| ours_evicted.contains(&f)))
}

impl std::fmt::Display for Placed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "row {} col {} z {}: {}",
            self.row, self.col, self.z, self.picture
        )
    }
}

/// A cell the arms disagree on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CellDiff {
    pub(crate) row: usize,
    pub(crate) col: usize,
    pub(crate) baud: Glyph,
    pub(crate) ghostty: Glyph,
}

/// A virtual placement the arms disagree on, by its image's client id:
/// the image each arm shows for it, `None` where it has none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct VirtualDiff {
    pub(crate) image: u32,
    pub(crate) baud: Option<Picture>,
    pub(crate) ghostty: Option<Picture>,
}

/// What one arm shows: the cells row by row, the placements anchored on
/// the screen sorted by cell and `z`, and the image ids of its virtual
/// placements as it names them.
struct Shown {
    cells: Vec<Glyph>,
    placed: Vec<Placed>,
    virtual_ids: Vec<u32>,
}

impl Shown {
    fn sort(mut self) -> Shown {
        self.placed
            .sort_by_key(|p| (p.row, p.col, p.z, p.picture.width, p.picture.height));
        self
    }
}

/// The two arms' final screens, compared.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Compared {
    /// The first [`SHOWN`] differing cells, top to bottom.
    pub(crate) cells: Vec<CellDiff>,
    pub(crate) cells_differing: usize,
    /// baud-core's and ghostty's placements anchored on the screen.
    pub(crate) placed: [Vec<Placed>; 2],
    /// The virtual placements that differ.
    pub(crate) virtuals: Vec<VirtualDiff>,
    /// Virtual placements either arm has, each compared.
    pub(crate) virtuals_compared: usize,
    /// Images compared by size alone, their pixels in a format not read.
    pub(crate) pixels_unread: usize,
    /// baud-core's and ghostty's counters from the same rep.
    pub(crate) counts: [Counts; 2],
    /// The eviction check from the same rep.
    pub(crate) eviction: Option<Eviction>,
}

impl Compared {
    /// The eviction check, where it applies ([`Eviction::applies`]).
    fn applied(&self) -> Option<&Eviction> {
        self.eviction.as_ref().filter(|e| e.applies())
    }

    /// Whether the placements anchored on the screen differ. Where the
    /// eviction check applies, a placement one arm shows and the other
    /// does not is no difference if the other arm evicted an image first
    /// seen at the same read as its image, and every other placement
    /// matches in its cell, `z`, picture and first read.
    pub(crate) fn placements_differ(&self) -> bool {
        let [ours, theirs] = &self.placed;
        let differ = ours.len() != theirs.len()
            || ours.iter().zip(theirs).any(|(a, b)| {
                (a.row, a.col, a.z) != (b.row, b.col, b.z) || !a.picture.same(b.picture)
            });
        differ
            && !self.applied().is_some_and(|e| {
                let [o, t] = &e.arms;
                match_apart_from_evicted(ours, theirs, &o.evicted, &t.evicted)
            })
    }

    /// Whether both arms show the same screen.
    pub(crate) fn equal(&self) -> bool {
        self.cells_differing == 0 && !self.placements_differ() && self.virtuals.is_empty()
    }

    /// What the two arms' counts say, with the eviction check ([`judged`]).
    pub(crate) fn counted(&self) -> Verdict {
        let [ours, theirs] = &self.counts;
        judged(ours, theirs, self.eviction.as_ref())
    }

    /// Whether the recording is timed: the same screen, no compared count
    /// that differs, and no order of the quota departed.
    pub(crate) fn timed(&self) -> bool {
        self.equal() && !matches!(self.counted(), Verdict::Differ(_) | Verdict::Departs(_))
    }

    /// Whether its rate is quotable and the run passes on it: the same
    /// screen, and every compared count reported by both arms and equal.
    pub(crate) fn rated(&self) -> bool {
        self.equal() && self.counted() == Verdict::Equal
    }

    /// The report printed under the table for the recording `label`.
    pub(crate) fn report(&self, label: &str) -> Vec<String> {
        let mut out = Vec::new();
        let [ours, theirs] = &self.placed;
        let unread = match self.pixels_unread {
            0 => String::new(),
            n => format!("; {n} images compared by size, their pixels not read"),
        };
        if self.equal() && ours.len() != theirs.len() {
            out.push(format!(
                "{label}: baud-core and ghostty show the same screen: {ROWS}x{COLS} cells, \
                 placements on the screen baud-core {} and ghostty {}, those on one arm \
                 alone of images the other evicted, {} virtual placements{unread}",
                ours.len(),
                theirs.len(),
                self.virtuals_compared
            ));
        } else if self.equal() {
            out.push(format!(
                "{label}: baud-core and ghostty show the same screen: {ROWS}x{COLS} cells, \
                 {} placements on the screen, {} virtual placements{unread}",
                ours.len(),
                self.virtuals_compared
            ));
        } else {
            out.push(format!(
                "{label}: SCREENS DIFFER, no rate: baud-core against ghostty{unread}"
            ));
            if self.cells_differing > 0 {
                out.push(format!(
                    "  {} of {} cells differ; the first {}:",
                    self.cells_differing,
                    ROWS * COLS,
                    self.cells.len()
                ));
                for d in &self.cells {
                    out.push(format!(
                        "    row {} col {}: baud-core {}, ghostty {}",
                        d.row, d.col, d.baud, d.ghostty
                    ));
                }
            }
            if self.placements_differ() {
                for (name, list) in [("baud-core", ours), ("ghostty", theirs)] {
                    out.push(format!("  {name}: {} placements on the screen", list.len()));
                    for p in list.iter().take(SHOWN) {
                        out.push(format!("    {p}"));
                    }
                }
            }
            let none = |p: Option<Picture>| p.map_or("none".to_string(), |p| p.to_string());
            for v in &self.virtuals {
                out.push(format!(
                    "  virtual placement of image {}: baud-core {}, ghostty {}",
                    v.image,
                    none(v.baud),
                    none(v.ghostty)
                ));
            }
        }
        let [ours, theirs] = &self.counts;
        let names = |list: &[Counter]| list.iter().map(|c| c.name()).collect::<Vec<_>>().join(", ");
        let own = "; evicted and refused are baud-core's own, and none";
        let n = |x: &Counts, c: Counter| x.get(c).map_or("n/a".to_string(), |n| n.to_string());
        let each = |list: &[Counter], out: &mut Vec<String>| {
            for &c in list {
                out.push(format!(
                    "  {}: baud-core {}, ghostty {}",
                    c.name(),
                    n(ours, c),
                    n(theirs, c)
                ));
            }
        };
        match self.counted() {
            Verdict::Equal if self.applied().is_some() => out.push(format!(
                "{label}: baud-core and ghostty counted the same: {}, the frames image by \
                 image; refused is baud-core's own, and none; {} do not decide, as the \
                 eviction check below says",
                names(&[Counter::Stored, Counter::StoredBytes, Counter::Frames]),
                names(&Counter::EXEMPT)
            )),
            Verdict::Equal => out.push(format!(
                "{label}: baud-core and ghostty counted the same: {}{own}",
                names(&Counter::COMPARED)
            )),
            Verdict::Differ(differ) => {
                out.push(format!(
                    "{label}: COUNTS DIFFER, no rate: baud-core against ghostty"
                ));
                each(&differ, &mut out);
            }
            Verdict::Unknown(missing) => {
                out.push(format!(
                    "{label}: UNEQUAL-UNKNOWN, rate unquotable: n/a on an arm, or baud-core's \
                     own above none where the other arm does not report it, so equal work is \
                     not shown: {}",
                    names(&missing)
                ));
                each(&missing, &mut out);
            }
            Verdict::Departs(_) => {
                let departures = self.eviction.iter().flat_map(|e| {
                    ["baud-core", "ghostty"]
                        .into_iter()
                        .zip(&e.arms)
                        .flat_map(|(arm, o)| o.departures.iter().map(move |d| (arm, d)))
                });
                for (arm, d) in departures {
                    out.push(format!(
                        "{label}: EVICTION ORDER DEPARTS, no rate: {}: {arm} {}",
                        d.rule.name(),
                        d.says()
                    ));
                }
            }
        }
        if let Some(e) = &self.eviction {
            self.eviction_lines(label, e, &mut out);
        }
        out
    }

    /// The eviction check's lines: whether it ran, each rule's cases on
    /// each arm, the images that left the observed set and why, the
    /// evictions against baud-core's own count, and what stopped deciding
    /// where it applies. Nothing where neither arm tracked an image.
    fn eviction_lines(&self, label: &str, e: &Eviction, out: &mut Vec<String>) {
        let [o, t] = &e.arms;
        if o.seen == 0 && t.seen == 0 {
            return;
        }
        if e.deletes {
            out.push(format!(
                "{label}: eviction order not checked: the recording sends a=d"
            ));
            return;
        }
        let [ours, theirs] = &self.counts;
        let n = |x: Option<usize>| x.map_or("n/a".to_string(), |n| n.to_string());
        let sign = if ours.stored == theirs.stored {
            "="
        } else {
            "!="
        };
        out.push(format!(
            "{label}: eviction order checked, the recording sends no a=d: stored {} {sign} {}",
            n(ours.stored),
            n(theirs.stored)
        ));
        let arm = |name: &str, cases: usize| match cases {
            0 => format!("{name} not checked"),
            1 => format!("{name} 1 case"),
            n => format!("{name} {n} cases"),
        };
        for rule in Rule::ALL {
            let (a, b) = (o.cases(rule), t.cases(rule));
            if a == 0 && b == 0 {
                out.push(format!("  {}: not checked", rule.name()));
                continue;
            }
            let status = if o.departs(rule) || t.departs(rule) {
                "DEPARTS"
            } else {
                "passed"
            };
            out.push(format!(
                "  {}: {}, {}: {status}",
                rule.name(),
                arm("baud-core", a),
                arm("ghostty", b)
            ));
        }
        let left = |x: &crate::counts::Order| {
            let why: Vec<String> = Left::ALL
                .into_iter()
                .map(|w| format!("{} {}", w.name(), x.left(w)))
                .collect();
            format!("{} ({})", x.left.iter().sum::<usize>(), why.join(", "))
        };
        out.push(format!(
            "  left the observed set: baud-core {}, ghostty {}",
            left(o),
            left(t)
        ));
        out.push(format!(
            "  evicted: baud-core {}, ghostty {}, of images tracked baud-core {}, ghostty {}; \
             placements lost from an image still held: baud-core {}, ghostty {}; baud-core's \
             store counted {} evicted",
            o.evicted.len(),
            t.evicted.len(),
            o.seen,
            t.seen,
            o.lost,
            t.lost,
            e.store_evicted
        ));
        let checked = e.checked();
        let rules = checked
            .iter()
            .map(|r| r.name())
            .collect::<Vec<_>>()
            .join(", ");
        if e.applies() {
            let exempt = Counter::EXEMPT
                .iter()
                .map(|c| c.name())
                .collect::<Vec<_>>()
                .join(", ");
            let f = e.frames;
            out.push(format!(
                "  exempted: {exempt} do not decide, the rules with cases being {rules}; \
                 animation frames held compared per image: {} images both arms hold, {} \
                 differ, {} unread, {} not paired",
                f.compared, f.differ, f.unread, f.unmatched
            ));
        } else if !e.departs().is_empty() {
            out.push("  not applied: an order departs".to_string());
        } else if checked.is_empty() {
            out.push("  not applied: no rule had a case".to_string());
        } else if !e.control() {
            out.push(format!(
                "  not applied: baud-core's evictions here are {} and its store counted {}",
                o.evicted.len(),
                e.store_evicted
            ));
        }
    }

    /// `arm`'s counters from the check's rep, as printed beside its rates:
    /// "n/a" for each one the arm does not report. libghostty-vt's image
    /// storage reports no evictions or refusals.
    pub(crate) fn counters(&self, arm: Arm) -> String {
        let i = match arm {
            Arm::Baud => 0,
            Arm::Ghostty => 1,
        };
        self.counts[i].to_string()
    }
}

/// The two arms the screen check compares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Arm {
    Baud,
    Ghostty,
}

/// A hash of pixels as `0xAARRGGBB` words, FNV-1a over each word's bytes
/// little end first.
fn digest(words: impl Iterator<Item = u32>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for w in words {
        for b in w.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Both arms' screens after the whole replays of `p` that fit in `total`
/// bytes (one at least), compared, and what each stored; `None` when libghostty-vt was not linked
/// and there is nothing to compare against. The replay ends at the
/// recording's end and never inside it: a stream cut inside a control
/// string leaves it open, and what each arm shows of an open string is
/// not a screen either has settled on.
pub(crate) fn check(p: &[u8], total: usize) -> Option<Compared> {
    check_with(p, total, crate::cores::GhosttyImages::Budget)
}

/// [`check`], ghostty's image storage given `images`.
#[cfg(have_ghostty)]
pub(crate) fn check_with(
    p: &[u8],
    total: usize,
    images: crate::cores::GhosttyImages,
) -> Option<Compared> {
    let whole = (total / p.len().max(1)).max(1) * p.len();
    let reads = |read: &mut dyn FnMut(usize, &[u8])| {
        // Each replay is read from its start, so a read's offset in the
        // recording is the bytes read so far, modulo its length.
        let mut at = 0;
        through_memory(p, whole, |chunk| {
            read(at, chunk);
            at = (at + chunk.len()) % p.len();
        });
    };
    Some(compare(p, reads, images, |_, _| {}))
}

/// The check over recording `p` fed as `reads` gives it, each read with
/// its offset in `p`: `before` is handed each read's number, from one,
/// and ghostty's terminal before the read is fed to either arm.
#[cfg(have_ghostty)]
fn compare(
    p: &[u8],
    reads: impl FnOnce(&mut dyn FnMut(usize, &[u8])),
    images: crate::cores::GhosttyImages,
    mut before: impl FnMut(usize, crate::ffi::ghostty::Terminal),
) -> Compared {
    use crate::counts::{Framed, Scan, Tally, Track, frames_per_image, kitty_ids};
    use crate::ffi::ghostty as g;
    let ws = crate::cores::baud_ws();
    let mut owned = crate::cores::baud_owned_with(&ws, images.baud());
    let mut t = crate::cores::baud_new(&mut owned, &ws);
    let term = crate::cores::ghostty_with(images);
    let ids = kitty_ids(p);
    let scan = Scan::of(p);
    let mut tally = [Tally::default(), Tally::default()];
    let mut ours_track = Track::new(scan.deletes);
    let mut theirs_track = Track::new(scan.deletes);
    let mut n = 0;
    reads(&mut |at: usize, chunk: &[u8]| {
        n += 1;
        before(n, term);
        crate::cores::baud_feed(&mut t, chunk);
        // SAFETY: `term` is a live terminal and `chunk` a live slice.
        unsafe { g::ghostty_terminal_vt_write(term, chunk.as_ptr(), chunk.len()) };
        let read = scan.read(at, chunk.len());
        tally[0].observe(baud::held(&t, &ids).into_iter().map(|(h, _)| h).collect());
        // SAFETY: `term` is live and nothing writes to it while it is read.
        tally[1].observe(unsafe { ghostty::held(term, &ids) });
        ours_track.step(&read, &baud::seen(&t, &ids), |h| baud::lookup(&t, h));
        // SAFETY: `term` is live and nothing writes to it while it is read.
        let placed = unsafe { ghostty::placed_ids(term) };
        // SAFETY: as above.
        let now = unsafe { ghostty::seen(term, &ids, &placed) };
        // SAFETY: as above; each lookup reads before the next write.
        theirs_track.step(&read, &now, |h| unsafe {
            ghostty::lookup(term, h, &placed)
        });
    });
    let ours_first = |id: baud_domain::ImageId| ours_track.first(|h| h.0 == id);
    let ours = baud::shown(&t, &ours_first).sort();
    let theirs_first = |id: u32| theirs_track.first(|h| h.0 == id);
    // SAFETY: `term` is live and nothing writes to it while it is read.
    let theirs = unsafe { ghostty::shown(term, &theirs_first) }.sort();

    let mut cells = Vec::new();
    let mut cells_differing = 0;
    for (i, (&a, &b)) in ours.cells.iter().zip(&theirs.cells).enumerate() {
        if a != b {
            cells_differing += 1;
            if cells.len() < SHOWN {
                cells.push(CellDiff {
                    row: i / COLS,
                    col: i % COLS,
                    baud: a,
                    ghostty: b,
                });
            }
        }
    }

    let mut ids: Vec<u32> = ours
        .virtual_ids
        .iter()
        .chain(&theirs.virtual_ids)
        .copied()
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let mut virtuals = Vec::new();
    let mut virtuals_compared = 0;
    let mut pixels_unread = 0;
    for &image in &ids {
        let a = baud::virtual_picture(&t, image);
        // SAFETY: as above.
        let b = unsafe { ghostty::virtual_picture(term, &theirs.virtual_ids, image) };
        if a.is_none() && b.is_none() {
            continue;
        }
        virtuals_compared += 1;
        match (a, b) {
            (Some(x), Some(y)) if x.same(y) => {
                pixels_unread += usize::from(x.unread() || y.unread());
            }
            _ => virtuals.push(VirtualDiff {
                image,
                baud: a,
                ghostty: b,
            }),
        }
    }
    pixels_unread += ours
        .placed
        .iter()
        .chain(&theirs.placed)
        .filter(|p| p.picture.unread())
        .count();

    let store = t.images();
    // An image whose frames cannot be read makes the count "n/a".
    let baud_frames = baud::held(&t, &ids)
        .into_iter()
        .map(|(_, id)| {
            let (n, _, _) = store.kitty_frames(id)?;
            usize::try_from(n).ok()
        })
        .sum();
    let baud_counts = tally[0].counts(
        baud_frames,
        Some(store.images_evicted),
        Some(store.images_refused),
    );
    // The frame probe writes to ghostty, so it runs once the screen is
    // read.
    // SAFETY: `term` is live; each probe reads it before the next write.
    let ghostty_frames = tally[1]
        .held()
        .iter()
        .map(|h| unsafe { ghostty::frames(term, h.id) })
        .sum();
    let ghostty_counts = tally[1].counts(ghostty_frames, None, None);
    let ours_framed: Vec<Framed> = ours_track
        .held()
        .map(|(h, first, named)| Framed {
            first,
            named,
            frames: store
                .kitty_frames(h.0)
                .and_then(|(n, _, _)| usize::try_from(n).ok()),
        })
        .collect();
    let theirs_framed: Vec<Framed> = theirs_track
        .held()
        .map(|(h, first, named)| Framed {
            first,
            named,
            // SAFETY: `term` is live; each probe reads it before the next
            // write.
            frames: unsafe { ghostty::frames(term, h.0) },
        })
        .collect();
    // SAFETY: `term` came from `ghostty_with` and is not used after this.
    unsafe { g::ghostty_terminal_free(term) };
    let eviction = Eviction {
        deletes: scan.deletes,
        frames: frames_per_image(&ours_framed, &theirs_framed),
        store_evicted: store.images_evicted,
        arms: [ours_track.order, theirs_track.order],
    };
    Compared {
        cells,
        cells_differing,
        placed: [ours.placed, theirs.placed],
        virtuals,
        virtuals_compared,
        pixels_unread,
        counts: [baud_counts, ghostty_counts],
        eviction: Some(eviction),
    }
}

/// [`compare`] over `reads`, each one read, ghostty's image storage given
/// `images`, and each of `plants` fed to ghostty alone before the read it
/// names: a delete outside the recording, a departure planted on one arm.
#[cfg(all(test, have_ghostty))]
pub(crate) fn check_reads(
    reads: &[Vec<u8>],
    images: crate::cores::GhosttyImages,
    plants: &[crate::cores::Plant],
) -> Compared {
    let p = reads.concat();
    let feed = |read: &mut dyn FnMut(usize, &[u8])| {
        let mut at = 0;
        for r in reads {
            read(at, &p[at..at + r.len()]);
            at += r.len();
        }
    };
    let plant = |n: usize, term: crate::ffi::ghostty::Terminal| {
        for x in plants.iter().filter(|x| x.read == n) {
            let cmd = format!("\x1b_Ga=d,d=I,i={},q=2\x1b\\", x.id);
            // SAFETY: `term` is a live terminal and `cmd` a live string.
            unsafe {
                crate::ffi::ghostty::ghostty_terminal_vt_write(term, cmd.as_ptr(), cmd.len())
            };
        }
    };
    compare(&p, feed, images, plant)
}

/// With no libghostty-vt there is no second screen.
#[cfg(not(have_ghostty))]
pub(crate) fn check_with(_: &[u8], _: usize, _: crate::cores::GhosttyImages) -> Option<Compared> {
    None
}

/// baud-core's screen, read through its public API.
#[cfg(have_ghostty)]
mod baud {
    use super::{Glyph, Picture, Placed, Shown, digest};
    use crate::counts::{Held, Lookup, Seen};
    use crate::{COLS, ROWS};
    use baud_core::Term;
    use baud_domain::{ImageAge, ImageId, ImageView, KittyId, KittyPlacement, Row, Rows};

    pub(super) fn picture(v: &ImageView<'_>) -> Picture {
        Picture {
            width: v.width.index(),
            height: v.height.index(),
            pixels: Some(digest(v.pixels.iter().map(|p| p.bits()))),
        }
    }

    /// The cells, the kitty placements anchored on the live screen, each
    /// with the read `first` gives for its image, and the image ids the
    /// placeholder cells on it name.
    pub(super) fn shown(t: &Term<'_>, first: &dyn Fn(ImageId) -> Option<usize>) -> Shown {
        let mut cells = Vec::with_capacity(ROWS * COLS);
        let mut virtual_ids = Vec::new();
        let view = t.viewport();
        for y in 0..ROWS {
            let r = Row::from_index(y);
            let row = &t.row(r).expect("a row on the grid")[..COLS];
            cells.extend(row.iter().map(|c| match c.content.is_wide_tail() {
                true => Glyph::WideTail,
                false => match t.base_of(*c).as_codepoint() {
                    Some(cp) => Glyph::Char(cp.value()),
                    None => Glyph::Blank,
                },
            }));
            virtual_ids.extend(
                t.placeholder_runs(row, view.styles(r))
                    .map(|run| run.image.raw()),
            );
        }
        let store = t.images();
        let origin = t.image_origin(Rows::ZERO);
        let placed = store
            .placements()
            .filter(|(_, s)| s.screen == origin.screen && store.kitty_names(s.image).is_some())
            .filter_map(|(_, s)| {
                let row = s.line.since(origin.top)?.index();
                let col = s.col.index();
                let image = store.image(s.image)?;
                (row < ROWS && col < COLS).then(|| Placed {
                    row,
                    col,
                    z: s.z.raw(),
                    picture: picture(&image),
                    first: first(s.image),
                })
            })
            .collect();
        Shown {
            cells,
            placed,
            virtual_ids,
        }
    }

    /// The image a virtual placement of client image `id` shows, if any.
    pub(super) fn virtual_picture(t: &Term<'_>, id: u32) -> Option<Picture> {
        let v = t.kitty_virtual(KittyId::of(id), KittyPlacement::NONE)?;
        t.images().image(v.image).map(|i| picture(&i))
    }

    /// The kitty images held on the live screen that client ids `ids` or
    /// a placement there name, each with its handle in the store.
    pub(super) fn held(t: &Term<'_>, ids: &[u32]) -> Vec<(Held, ImageId)> {
        let store = t.images();
        let screen = t.image_origin(Rows::ZERO).screen;
        let placed = store
            .placements()
            .filter(|(_, s)| s.screen == screen)
            .map(|(_, s)| s.image);
        let named = ids
            .iter()
            .filter_map(|&i| store.kitty_image_on(screen, KittyId::of(i)));
        let mut out: Vec<(Held, ImageId)> = placed
            .chain(named)
            .filter_map(|id| {
                let (kitty, _) = store.kitty_names(id)?;
                let (w, h) = store.size_of(id)?;
                let image = Held {
                    id: kitty.raw(),
                    unnamed: if kitty.is_none() {
                        store.born(id)
                    } else {
                        None
                    },
                    width: w.index(),
                    height: h.index(),
                };
                Some((image, id))
            })
            .collect();
        out.sort_unstable_by_key(|&(h, _)| h);
        out.dedup_by_key(|&mut (h, _)| h);
        out
    }

    /// The kitty images [`held`] finds, each with the slot and age that
    /// find it again, whether it is placed, and its `i=`.
    pub(super) fn seen(t: &Term<'_>, ids: &[u32]) -> Vec<Seen<(ImageId, ImageAge)>> {
        let store = t.images();
        held(t, ids)
            .into_iter()
            .filter_map(|(_, id)| {
                let born = store.born(id)?;
                let (kitty, number) = store.kitty_names(id)?;
                Some(Seen {
                    handle: (id, born),
                    placed: store.is_placed(id),
                    named: (number.is_none() && !kitty.is_none()).then_some(kitty.raw()),
                    has_id: !kitty.is_none(),
                })
            })
            .collect()
    }

    /// The image `h` names, held while its slot holds the image stored
    /// when `h` was taken.
    pub(super) fn lookup(t: &Term<'_>, h: (ImageId, ImageAge)) -> Lookup<(ImageId, ImageAge)> {
        let store = t.images();
        match store.born(h.0) {
            Some(born) if born == h.1 => Lookup::Held {
                handle: h,
                placed: store.is_placed(h.0),
            },
            _ => Lookup::Gone,
        }
    }
}

/// ghostty's screen, read through libghostty-vt's C API.
#[cfg(have_ghostty)]
mod ghostty {
    use super::{Glyph, Picture, Placed, Shown, digest};
    use crate::counts::{Held, Lookup, Seen};
    use crate::ffi::ghostty::*;
    use crate::{COLS, ROWS};
    use std::ffi::c_void;

    /// The active screen's image storage, borrowed until the next write.
    ///
    /// # Safety
    /// `t` is a live terminal.
    unsafe fn graphics(t: Terminal) -> Graphics {
        let mut g: Graphics = std::ptr::null_mut();
        // SAFETY: the out-pointer is a `GhosttyKittyGraphics`, as the header
        // names for this datum.
        let rc =
            unsafe { ghostty_terminal_get(t, TERMINAL_DATA_KITTY_GRAPHICS, (&raw mut g).cast()) };
        assert_eq!(rc, 0, "reading ghostty's image storage failed with {rc}");
        g
    }

    /// One datum of `image` into `out`, of the type the header names.
    ///
    /// # Safety
    /// `image` is live and `out` points at that type.
    unsafe fn image_get<T>(image: Image, data: i32, out: &mut T) -> bool {
        // SAFETY: the caller's.
        unsafe {
            ghostty_kitty_graphics_image_get(image, data, (out as *mut T).cast::<c_void>()) == 0
        }
    }

    /// `image`'s size and pixels, each pixel widened to `0xAARRGGBB`.
    ///
    /// # Safety
    /// `image` is live.
    unsafe fn picture(image: Image) -> Picture {
        let (mut w, mut h, mut format) = (0u32, 0u32, -1i32);
        let (mut ptr, mut len): (*const u8, usize) = (std::ptr::null(), 0);
        // SAFETY: each out-pointer is of the type the header names.
        let read = unsafe {
            image_get(image, IMAGE_DATA_WIDTH, &mut w)
                && image_get(image, IMAGE_DATA_HEIGHT, &mut h)
                && image_get(image, IMAGE_DATA_FORMAT, &mut format)
        };
        assert!(read, "reading a ghostty image's size and format failed");
        // SAFETY: as above.
        let data = unsafe {
            image_get(image, IMAGE_DATA_DATA_PTR, &mut ptr)
                && image_get(image, IMAGE_DATA_DATA_LEN, &mut len)
        };
        let (width, height) = (w as usize, h as usize);
        let per = match format {
            IMAGE_FORMAT_RGB => 3,
            IMAGE_FORMAT_RGBA => 4,
            IMAGE_FORMAT_GRAY_ALPHA => 2,
            IMAGE_FORMAT_GRAY => 1,
            _ => 0,
        };
        let pixels =
            (data && !ptr.is_null() && per > 0 && len == width * height * per).then(|| {
                // SAFETY: the library holds `len` bytes at `ptr` until the
                // terminal is next written.
                let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
                digest(bytes.chunks_exact(per).map(|p| {
                    let (r, g, b, a) = match *p {
                        [r, g, b] => (r, g, b, 0xff),
                        [r, g, b, a] => (r, g, b, a),
                        [v, a] => (v, v, v, a),
                        [v] => (v, v, v, 0xff),
                        _ => unreachable!("a pixel is one to four bytes"),
                    };
                    u32::from_be_bytes([a, r, g, b])
                }))
            });
        Picture {
            width,
            height,
            pixels,
        }
    }

    /// The cells, the placements anchored on the screen, each with the
    /// read `first` gives for its image id, and the client image ids of
    /// the virtual placements.
    ///
    /// # Safety
    /// `t` is a live terminal, not written while this runs.
    pub(super) unsafe fn shown(t: Terminal, first: &dyn Fn(u32) -> Option<usize>) -> Shown {
        let mut cells = Vec::with_capacity(ROWS * COLS);
        for y in 0..ROWS as u32 {
            for x in 0..COLS as u16 {
                let mut gref = GridRef {
                    size: std::mem::size_of::<GridRef>(),
                    node: std::ptr::null_mut(),
                    x: 0,
                    y: 0,
                };
                let point = Point {
                    tag: POINT_TAG_ACTIVE,
                    _gap: 0,
                    x,
                    _x_pad: 0,
                    y,
                    _rest: 0,
                };
                let (mut cell, mut cp, mut wide) = (0u64, 0u32, 0i32);
                // SAFETY: every out-pointer is a live local of the type the
                // header names; the ref is used before the next write.
                unsafe {
                    assert_eq!(ghostty_terminal_grid_ref(t, point, &mut gref), 0);
                    assert_eq!(ghostty_grid_ref_cell(&gref, &mut cell), 0);
                    assert_eq!(
                        ghostty_cell_get(cell, CELL_DATA_CODEPOINT, (&raw mut cp).cast()),
                        0
                    );
                    assert_eq!(
                        ghostty_cell_get(cell, CELL_DATA_WIDE, (&raw mut wide).cast()),
                        0
                    );
                }
                cells.push(match (wide, cp) {
                    (CELL_WIDE_SPACER_TAIL, _) => Glyph::WideTail,
                    (_, 0) => Glyph::Blank,
                    (_, c) => Glyph::Char(c),
                });
            }
        }

        let mut placed = Vec::new();
        let mut virtual_ids = Vec::new();
        // SAFETY: `t` is live; the storage, iterator and images are read
        // before the next write, and the iterator is freed here.
        unsafe {
            let g = graphics(t);
            let mut it: Placements = std::ptr::null_mut();
            assert_eq!(
                ghostty_kitty_graphics_placement_iterator_new(std::ptr::null(), &mut it),
                0
            );
            assert_eq!(
                ghostty_kitty_graphics_get(
                    g,
                    GRAPHICS_DATA_PLACEMENT_ITERATOR,
                    (&raw mut it).cast()
                ),
                0
            );
            while ghostty_kitty_graphics_placement_next(it) {
                let (mut id, mut is_virtual, mut z) = (0u32, false, 0i32);
                assert_eq!(
                    ghostty_kitty_graphics_placement_get(
                        it,
                        PLACEMENT_DATA_IMAGE_ID,
                        (&raw mut id).cast()
                    ),
                    0
                );
                assert_eq!(
                    ghostty_kitty_graphics_placement_get(
                        it,
                        PLACEMENT_DATA_IS_VIRTUAL,
                        (&raw mut is_virtual).cast()
                    ),
                    0
                );
                assert_eq!(
                    ghostty_kitty_graphics_placement_get(it, PLACEMENT_DATA_Z, (&raw mut z).cast()),
                    0
                );
                if is_virtual {
                    virtual_ids.push(id);
                    continue;
                }
                let image = ghostty_kitty_graphics_image(g, id);
                if image.is_null() {
                    continue;
                }
                let (mut col, mut row) = (0i32, 0i32);
                if ghostty_kitty_graphics_placement_viewport_pos(it, image, t, &mut col, &mut row)
                    != 0
                {
                    continue;
                }
                let (Ok(row), Ok(col)) = (usize::try_from(row), usize::try_from(col)) else {
                    continue;
                };
                if row < ROWS && col < COLS {
                    placed.push(Placed {
                        row,
                        col,
                        z,
                        picture: picture(image),
                        first: first(id),
                    });
                }
            }
            ghostty_kitty_graphics_placement_iterator_free(it);
        }
        Shown {
            cells,
            placed,
            virtual_ids,
        }
    }

    /// The image id of every placement in the active screen's storage,
    /// sorted, each once. The storage lists a placement whose row left the
    /// retained history until its next placement is added.
    ///
    /// # Safety
    /// `t` is a live terminal, not written while this runs.
    pub(super) unsafe fn placed_ids(t: Terminal) -> Vec<u32> {
        let mut ids = Vec::new();
        // SAFETY: `t` is live; the storage and iterator are read before
        // the next write, and the iterator is freed here.
        unsafe {
            let g = graphics(t);
            let mut it: Placements = std::ptr::null_mut();
            assert_eq!(
                ghostty_kitty_graphics_placement_iterator_new(std::ptr::null(), &mut it),
                0
            );
            assert_eq!(
                ghostty_kitty_graphics_get(
                    g,
                    GRAPHICS_DATA_PLACEMENT_ITERATOR,
                    (&raw mut it).cast()
                ),
                0
            );
            while ghostty_kitty_graphics_placement_next(it) {
                let mut id = 0u32;
                assert_eq!(
                    ghostty_kitty_graphics_placement_get(
                        it,
                        PLACEMENT_DATA_IMAGE_ID,
                        (&raw mut id).cast()
                    ),
                    0
                );
                ids.push(id);
            }
            ghostty_kitty_graphics_placement_iterator_free(it);
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// The images the active screen's storage holds that client ids `ids`
    /// or a placement there name, each with its id and generation, whether
    /// `placed`, the ids of the placements, names it, and its `i=`. An
    /// image with an image number has an id the storage gave it; one with
    /// neither a number nor an id the recording names has none.
    ///
    /// # Safety
    /// `t` is a live terminal, not written while this runs.
    pub(super) unsafe fn seen(t: Terminal, ids: &[u32], placed: &[u32]) -> Vec<Seen<(u32, u64)>> {
        let mut all: Vec<u32> = placed.iter().chain(ids).copied().collect();
        all.sort_unstable();
        all.dedup();
        // SAFETY: `t` is live; the storage and images are read before the
        // next write.
        unsafe {
            let g = graphics(t);
            all.into_iter()
                .filter_map(|id| {
                    let image = ghostty_kitty_graphics_image(g, id);
                    if image.is_null() {
                        return None;
                    }
                    let (mut generation, mut number) = (0u64, 0u32);
                    let read = image_get(image, IMAGE_DATA_GENERATION, &mut generation)
                        && image_get(image, IMAGE_DATA_NUMBER, &mut number);
                    assert!(
                        read,
                        "reading a ghostty image's generation and number failed"
                    );
                    let named = (number == 0 && ids.binary_search(&id).is_ok()).then_some(id);
                    Some(Seen {
                        handle: (id, generation),
                        placed: placed.binary_search(&id).is_ok(),
                        named,
                        has_id: number != 0 || named.is_some(),
                    })
                })
                .collect()
        }
    }

    /// The image `h`'s id names now, read with its generation, and whether
    /// `placed`, the ids of the placements, names it.
    ///
    /// # Safety
    /// `t` is a live terminal, not written while this runs.
    pub(super) unsafe fn lookup(t: Terminal, h: (u32, u64), placed: &[u32]) -> Lookup<(u32, u64)> {
        // SAFETY: the caller's.
        match unsafe { generation(t, h.0) } {
            None => Lookup::Gone,
            Some(g) => Lookup::Held {
                handle: (h.0, g),
                placed: placed.binary_search(&h.0).is_ok(),
            },
        }
    }

    /// The images the active screen's storage holds that client ids `ids`
    /// or a placement there name.
    ///
    /// # Safety
    /// `t` is a live terminal, not written while this runs.
    pub(super) unsafe fn held(t: Terminal, ids: &[u32]) -> Vec<Held> {
        let mut named = ids.to_vec();
        // SAFETY: `t` is live; the storage and images are read before the
        // next write.
        unsafe {
            named.extend(placed_ids(t));
            let g = graphics(t);
            named.sort_unstable();
            named.dedup();
            named
                .into_iter()
                .filter_map(|id| {
                    let image = ghostty_kitty_graphics_image(g, id);
                    if image.is_null() {
                        return None;
                    }
                    let (mut w, mut h) = (0u32, 0u32);
                    let read = image_get(image, IMAGE_DATA_WIDTH, &mut w)
                        && image_get(image, IMAGE_DATA_HEIGHT, &mut h);
                    assert!(read, "reading a ghostty image's size failed");
                    Some(Held {
                        id,
                        unnamed: None,
                        width: w as usize,
                        height: h as usize,
                    })
                })
                .collect()
        }
    }

    /// Image `id`'s generation, `None` once the storage no longer holds it.
    ///
    /// # Safety
    /// `t` is a live terminal, not written while this runs.
    unsafe fn generation(t: Terminal, id: u32) -> Option<u64> {
        // SAFETY: the caller's; the image is read before the next write.
        unsafe {
            let image = ghostty_kitty_graphics_image(graphics(t), id);
            let mut g = 0u64;
            (!image.is_null() && image_get(image, IMAGE_DATA_GENERATION, &mut g)).then_some(g)
        }
    }

    /// How many animation frames image `id` holds, the image itself the
    /// first. The C API reports none, so each is asked for: the animation
    /// is stopped on frame 1, then frame `n + 1` is made current while
    /// the image's generation moves. A frame past the last is ignored and
    /// leaves the generation where it was, so the last that moved it is
    /// the count; `None` when the image or its generation cannot be read.
    /// This writes to `t` and leaves it on its last frame.
    ///
    /// # Safety
    /// `t` is a live terminal.
    pub(super) unsafe fn frames(t: Terminal, id: u32) -> Option<usize> {
        let send = |cmd: String| {
            // SAFETY: the caller's, and `cmd` is a live string.
            unsafe { ghostty_terminal_vt_write(t, cmd.as_ptr(), cmd.len()) }
        };
        send(format!("\x1b_Ga=a,i={id},s=1,c=1,q=2\x1b\\"));
        let mut n = 1;
        loop {
            // SAFETY: the caller's.
            let before = unsafe { generation(t, id) }?;
            send(format!("\x1b_Ga=a,i={id},c={},q=2\x1b\\", n + 1));
            // SAFETY: as above.
            if unsafe { generation(t, id) } == Some(before) {
                return Some(n);
            }
            n += 1;
        }
    }

    /// The image a virtual placement of client image `id` shows: none
    /// unless `virtual_ids`, ghostty's own, holds it.
    ///
    /// # Safety
    /// `t` is a live terminal, not written while this runs.
    pub(super) unsafe fn virtual_picture(
        t: Terminal,
        virtual_ids: &[u32],
        id: u32,
    ) -> Option<Picture> {
        if !virtual_ids.contains(&id) {
            return None;
        }
        // SAFETY: the caller's; the image is read before the next write.
        unsafe {
            let image = ghostty_kitty_graphics_image(graphics(t), id);
            (!image.is_null()).then(|| picture(image))
        }
    }
}

/// **Recordings the two cores are known to leave apart**, in what they
/// show or in what they store, by file name, and why. Such a recording gets
/// no rate and the run says so, with the reason, instead of failing; one
/// listed here whose screens and counts agree fails the run, so the list
/// cannot outlive its reason.
pub(crate) const UNEQUAL: [(&str, &str); 2] = [
    (
        "mpv-logo-40x6.raw",
        "uploads a bare ESC cuts, dropped by both arms at different stages: mpv's status \
         line cuts two uploads a replay inside a chunk; baud-core cancels the chunk at the \
         ESC and refuses the next for having no size, decoding nothing after it, while \
         ghostty takes the ESC as the chunk's end, decodes the rest of the upload and drops \
         it when its length falls short at the last chunk; neither stores it, and the work \
         is not equal",
    ),
    (
        "mpv-status-in-chunk.raw",
        "uploads refused (a bare ESC in an upload): the recording is one continuation \
         chunk that mpv's status line cuts, a bare ESC cancels it, and baud-core refuses \
         the upload whole; ghostty reports no refusals, so equal work is not shown",
    ),
];

/// The reason `label` is listed in [`UNEQUAL`], if it is.
pub(crate) fn known_unequal(label: &str) -> Option<&'static str> {
    UNEQUAL
        .iter()
        .find(|(name, _)| *name == label)
        .map(|(_, why)| *why)
}

#[cfg(test)]
mod tests {
    /// Each recording [`super::UNEQUAL`] lists is one the run can be given,
    /// and only a listed name has a reason.
    #[test]
    fn every_known_unequal_recording_exists() {
        for (name, why) in super::UNEQUAL {
            let found = ["ascii", "kitty", "sixel", "utf8"].iter().any(|dir| {
                std::path::Path::new(&format!(
                    "{}/../../tests/recordings/{dir}/{name}",
                    env!("CARGO_MANIFEST_DIR")
                ))
                .exists()
            });
            assert!(found, "{name} is not a recording");
            assert!(!why.is_empty(), "{name} has no reason");
            assert_eq!(super::known_unequal(name), Some(why));
        }
        assert_eq!(super::known_unequal("grad-80x24-placeholder.raw"), None);
    }

    /// A recording from `tests/recordings/`.
    fn recording(name: &str) -> Vec<u8> {
        let p = format!(
            "{}/../../tests/recordings/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
    }

    /// The decoder hook is the process's: each test holds this while it
    /// sets the hook and runs its check.
    #[cfg(have_ghostty)]
    static HOOK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// One pass of the placeholder recording, its PNG decoded by ghostty
    /// or not, compared.
    #[cfg(have_ghostty)]
    fn placeholder(decoder: crate::ffi::ghostty::PngDecoder) -> super::Compared {
        placeholder_with(decoder, crate::cores::GhosttyImages::Budget)
    }

    /// [`placeholder`], ghostty's image storage given `images`.
    #[cfg(have_ghostty)]
    fn placeholder_with(
        decoder: crate::ffi::ghostty::PngDecoder,
        images: crate::cores::GhosttyImages,
    ) -> super::Compared {
        let _hook = HOOK.lock().unwrap_or_else(|e| e.into_inner());
        crate::ffi::ghostty::set_png_decoder(decoder);
        let p = recording("kitty/grad-80x24-placeholder.raw");
        super::check_with(&p, p.len(), images).expect("libghostty-vt is linked")
    }

    /// With no PNG decoder ghostty refuses the `f=100` transmit, so its
    /// placeholder cells name an image it does not have while baud-core's
    /// show the one it decoded. The text is the same on both; the check
    /// fails on the virtual placement, and so the recording gets no rate.
    #[cfg(have_ghostty)]
    #[test]
    fn a_png_ghostty_does_not_decode_fails_the_screen_check() {
        let c = placeholder(crate::ffi::ghostty::PngDecoder::Cleared);
        let report = c.report("grad-80x24-placeholder.raw").join("\n");
        assert!(!c.equal(), "{report}");
        assert_eq!(
            c.cells_differing, 0,
            "the placeholders are text on both arms\n{report}"
        );
        assert!(!c.placements_differ(), "{report}");
        assert_eq!(c.virtuals.len(), 1, "{report}");
        let v = c.virtuals[0];
        assert!(v.baud.is_some() && v.ghostty.is_none(), "{report}");
        assert!(report.contains("SCREENS DIFFER, no rate"), "{report}");
    }

    /// With the decoder both arms store the same image, pixel for pixel,
    /// under the same virtual placement, and the check passes having
    /// compared it.
    #[cfg(have_ghostty)]
    #[test]
    fn a_png_both_arms_decode_passes_the_screen_check() {
        let c = placeholder(crate::ffi::ghostty::PngDecoder::Registered);
        let report = c.report("grad-80x24-placeholder.raw").join("\n");
        assert!(c.equal(), "{report}");
        assert_eq!(c.virtuals_compared, 1, "{report}");
        assert_eq!(c.pixels_unread, 0, "{report}");
    }

    /// Three one-cell pictures placed at rows 1, 11 and 21 of the 50-row
    /// screen, then, in the second stream, the screen scrolled up by 15
    /// lines from its bottom row.
    #[cfg(have_ghostty)]
    fn three_placed(scrolled: bool) -> super::Compared {
        let mut p = Vec::new();
        for (row, id) in [(1, 7), (11, 8), (21, 9)] {
            p.extend_from_slice(format!("\x1b[{row};2H").as_bytes());
            p.extend_from_slice(
                format!("\x1b_Ga=T,f=32,s=1,v=1,i={id},c=1,r=1,q=2;AAAA/w==\x1b\\").as_bytes(),
            );
        }
        if scrolled {
            p.extend_from_slice(b"\x1b[50;1H");
            p.extend_from_slice(&[b'\n'; 15]);
        }
        super::check(&p, p.len()).expect("libghostty-vt is linked")
    }

    /// **The placement counter reads a known count on both arms**, its
    /// positive control: three pictures placed read as three on each arm,
    /// at rows 0, 10 and 20; scrolled up by 15 lines, a picture moves with
    /// its text, so the two that leave the top are off the screen and the
    /// third reads at row 5, on each arm.
    #[cfg(have_ghostty)]
    #[test]
    fn three_placements_read_three_and_scroll_with_the_text_on_both_arms() {
        let c = three_placed(false);
        let report = c.report("three placed").join("\n");
        let rows = |i: usize| c.placed[i].iter().map(|p| p.row).collect::<Vec<_>>();
        assert_eq!(rows(0), [0, 10, 20], "baud-core\n{report}");
        assert_eq!(rows(1), [0, 10, 20], "ghostty\n{report}");
        assert!(!c.placements_differ(), "{report}");
        let c = three_placed(true);
        let report = c.report("three placed, scrolled").join("\n");
        let rows = |i: usize| c.placed[i].iter().map(|p| p.row).collect::<Vec<_>>();
        assert_eq!(rows(0), [5], "baud-core\n{report}");
        assert_eq!(rows(1), [5], "ghostty\n{report}");
    }

    /// With storage on, both arms count the one image the recording
    /// stores, its pixels and its one frame, and the recording is rated.
    /// With ghostty's storage off its count falls to none while
    /// baud-core's stays, and the count check, not only the screen check,
    /// refuses the rate: a counter that read nothing on either arm could
    /// not pass both.
    #[cfg(have_ghostty)]
    #[test]
    fn ghostty_storing_nothing_fails_the_count_check() {
        use crate::cores::GhosttyImages;
        use crate::counts::{Counter, Verdict};
        use crate::ffi::ghostty::PngDecoder;
        let on = placeholder_with(PngDecoder::Registered, GhosttyImages::Budget);
        let report = on.report("grad-80x24-placeholder.raw").join("\n");
        let [ours, theirs] = on.counts;
        assert_eq!(ours.held, Some(1), "{report}");
        assert_eq!(theirs.held, Some(1), "{report}");
        assert_eq!(theirs.stored, Some(1), "{report}");
        assert_eq!(theirs.frames, Some(1), "{report}");
        assert!(theirs.held_bytes.is_some_and(|b| b > 0), "{report}");
        assert_eq!(ours.held_bytes, theirs.held_bytes, "{report}");
        assert_eq!(on.counted(), Verdict::Equal, "{report}");
        assert!(on.rated(), "{report}");
        assert!(report.contains("counted the same"), "{report}");
        assert!(report.contains("evicted and refused are baud-core's own"));

        let off = placeholder_with(PngDecoder::Registered, GhosttyImages::Off);
        let report = off.report("grad-80x24-placeholder.raw").join("\n");
        let [ours, theirs] = off.counts;
        assert_eq!(ours.held, Some(1), "{report}");
        assert_eq!(theirs.held, Some(0), "{report}");
        assert_eq!(theirs.stored, Some(0), "{report}");
        assert_eq!(
            off.counted(),
            Verdict::Differ(Counter::COMPARED.to_vec()),
            "{report}"
        );
        assert!(!off.timed(), "{report}");
        assert!(!off.rated(), "{report}");
        assert!(report.contains("COUNTS DIFFER, no rate"), "{report}");
        assert!(
            report.contains("images held: baud-core 1, ghostty 0"),
            "{report}"
        );
        assert_eq!(
            off.counters(super::Arm::Ghostty),
            "images stored 0, pixel bytes stored 0, images held 0, pixel bytes held 0, \
             animation frames held 0, evicted n/a, refused n/a"
        );
    }

    /// **The placement reader's positive control**: three images sent with
    /// no client id and two with one, each placed on its own row, read as
    /// five placements on the screen on both arms, at the same cells. A
    /// placement count either arm reports for a recording is believed only
    /// because this known case reads right on the same build.
    #[cfg(have_ghostty)]
    #[test]
    fn five_images_placed_read_as_five_placements_on_both_arms() {
        const PIXELS: &str = "f=24,s=2,v=2;////////////////\x1b\\\r\n";
        let mut p = Vec::new();
        for keys in ["a=T,", "a=T,", "a=T,", "a=T,i=7,", "a=T,i=8,"] {
            p.extend_from_slice(b"\x1b_G");
            p.extend_from_slice(keys.as_bytes());
            p.extend_from_slice(PIXELS.as_bytes());
        }
        let _hook = HOOK.lock().unwrap_or_else(|e| e.into_inner());
        crate::ffi::ghostty::set_png_decoder(crate::ffi::ghostty::PngDecoder::Registered);
        let c = super::check(&p, p.len()).expect("libghostty-vt is linked");
        let report = c.report("five-placed").join("\n");
        let [ours, theirs] = &c.placed;
        assert_eq!(ours.len(), 5, "baud-core's placements\n{report}");
        assert_eq!(theirs.len(), 5, "ghostty's placements\n{report}");
        assert!(!c.placements_differ(), "{report}");
    }

    /// **icat comes off [`super::UNEQUAL`] under the eviction check**: each
    /// replay transmits a new image under one image number and places it,
    /// and both arms evict the oldest of them as their quotas fill. The
    /// check finds cases for the store order and for the newest surviving
    /// on both arms, and none for the placed-over-unplaced order, since no
    /// image is named by `i=`; baud-core's evictions as classified are the
    /// ones its store counted; and the recording is rated.
    #[cfg(have_ghostty)]
    #[test]
    fn icat_comes_off_unequal_under_the_eviction_check() {
        use crate::counts::Rule;
        let p = recording("kitty/icat-logo-dark-140.raw");
        let _hook = HOOK.lock().unwrap_or_else(|e| e.into_inner());
        crate::ffi::ghostty::set_png_decoder(crate::ffi::ghostty::PngDecoder::Registered);
        let c = super::check(&p, 32 << 20).expect("libghostty-vt is linked");
        let report = c.report("icat").join("\n");
        assert_eq!(super::known_unequal("icat-logo-dark-140.raw"), None);
        let e = c.eviction.as_ref().expect("the check ran");
        for (arm, o) in ["baud-core", "ghostty"].into_iter().zip(&e.arms) {
            assert_eq!(o.cases(Rule::PlacedKept), 0, "{arm}\n{report}");
            assert!(o.cases(Rule::OlderFirst) > 0, "{arm}\n{report}");
            assert!(o.cases(Rule::NewestSurvives) > 0, "{arm}\n{report}");
        }
        assert!(
            report.contains("placed kept over unplaced: not checked"),
            "{report}"
        );
        assert!(e.store_evicted > 0, "{report}");
        assert_eq!(e.arms[0].evicted.len(), e.store_evicted, "{report}");
        assert!(c.rated(), "{report}");
    }

    /// A 1x1 picture's keys and pixels, after a command's own keys.
    #[cfg(have_ghostty)]
    const PIXEL: &str = "f=32,s=1,v=1,c=1,r=1,q=2;AAAA/w==\x1b\\";

    /// A kitty transmission of a 1x1 picture with the keys `keys`.
    #[cfg(have_ghostty)]
    fn kitty(keys: &str) -> Vec<u8> {
        format!("\x1b_G{keys},{PIXEL}").into_bytes()
    }

    /// A placement of image `id` at the cursor.
    #[cfg(have_ghostty)]
    fn place(id: u32) -> Vec<u8> {
        format!("\x1b_Ga=p,i={id},c=1,r=1,q=2\x1b\\").into_bytes()
    }

    /// The cursor to column 1 of `row`.
    #[cfg(have_ghostty)]
    fn cup(row: usize) -> Vec<u8> {
        format!("\x1b[{row};1H").into_bytes()
    }

    /// A read that changes nothing either arm stores or shows.
    #[cfg(have_ghostty)]
    fn quiet() -> Vec<u8> {
        b"\x1b[m".to_vec()
    }

    /// The check over `reads`, one read each, with `plants` fed to ghostty.
    #[cfg(have_ghostty)]
    fn by_reads(
        reads: &[Vec<u8>],
        images: crate::cores::GhosttyImages,
        plants: &[crate::cores::Plant],
    ) -> super::Compared {
        let _hook = HOOK.lock().unwrap_or_else(|e| e.into_inner());
        crate::ffi::ghostty::set_png_decoder(crate::ffi::ghostty::PngDecoder::Registered);
        super::check_reads(reads, images, plants)
    }

    /// **The quota orders by when an image was stored, not by when it was
    /// placed**: images 1 and 2 stored in that order and placed in the
    /// other, then a third over a two-image budget. Both arms evict image
    /// 1, the first stored, and the store order passes with one case on
    /// each; a check by place order would have wanted image 2 gone.
    #[cfg(have_ghostty)]
    #[test]
    fn store_early_place_late_evicts_the_first_stored() {
        use crate::counts::Rule;
        let reads = [
            kitty("a=t,i=1"),
            kitty("a=t,i=2"),
            [cup(1), place(2)].concat(),
            [cup(3), place(1)].concat(),
            [cup(5), kitty("a=T,i=3")].concat(),
        ];
        let c = by_reads(&reads, crate::cores::GhosttyImages::Small(2), &[]);
        let report = c.report("store-early").join("\n");
        let e = c.eviction.as_ref().expect("the check ran");
        for (arm, o) in ["baud-core", "ghostty"].into_iter().zip(&e.arms) {
            assert_eq!(o.evicted, [1], "{arm} evicts the first stored\n{report}");
            assert_eq!(o.cases(Rule::OlderFirst), 1, "{arm}\n{report}");
            assert!(o.departures.is_empty(), "{arm}\n{report}");
        }
        assert_eq!(e.store_evicted, 1, "{report}");
        assert!(e.applies(), "{report}");
        assert!(c.rated(), "{report}");
        assert!(
            report.contains("older stored goes first: baud-core 1 case, ghostty 1 case: passed"),
            "{report}"
        );
    }

    /// **A re-transmission under one id is a younger store**: image 1 sent
    /// again after image 2, then a third over a two-image budget. Image 2
    /// goes on both arms, the first image 1 left the observed set as
    /// re-transmitted, and the store order passes.
    #[cfg(have_ghostty)]
    #[test]
    fn a_retransmission_under_one_id_is_a_younger_store() {
        use crate::counts::{Left, Rule};
        let reads = [
            kitty("a=t,i=1"),
            kitty("a=t,i=2"),
            kitty("a=t,i=1"),
            kitty("a=t,i=3"),
        ];
        let c = by_reads(&reads, crate::cores::GhosttyImages::Small(2), &[]);
        let report = c.report("re-sent").join("\n");
        let e = c.eviction.as_ref().expect("the check ran");
        for (arm, o) in ["baud-core", "ghostty"].into_iter().zip(&e.arms) {
            assert_eq!(o.evicted, [2], "{arm} evicts image 2\n{report}");
            assert_eq!(o.left(Left::Resent), 1, "{arm}\n{report}");
            assert_eq!(o.cases(Rule::OlderFirst), 1, "{arm}\n{report}");
            assert!(o.departures.is_empty(), "{arm}\n{report}");
        }
        assert!(e.applies(), "{report}");
        assert!(c.rated(), "{report}");
    }

    /// **A lost placement is not an eviction**: an `i=` image placed, then
    /// lines past the 10,000 kept, so its row leaves the history on both
    /// arms, then a second image placed, which has ghostty drop the
    /// placements whose rows left. The first image's placement is gone on
    /// both arms and the image is held, unplaced: nothing is evicted, no
    /// rule has a case, and the check does not apply.
    #[cfg(have_ghostty)]
    #[test]
    fn a_lost_placement_is_not_an_eviction() {
        let mut p = [cup(1), kitty("a=T,i=1")].concat();
        for _ in 0..13_000 {
            p.extend_from_slice(b"x\r\n");
        }
        p.extend_from_slice(&[cup(1), kitty("a=T,i=2")].concat());
        let _hook = HOOK.lock().unwrap_or_else(|e| e.into_inner());
        crate::ffi::ghostty::set_png_decoder(crate::ffi::ghostty::PngDecoder::Registered);
        let c = super::check(&p, p.len()).expect("libghostty-vt is linked");
        let report = c.report("lost").join("\n");
        let e = c.eviction.as_ref().expect("the check ran");
        for (arm, o) in ["baud-core", "ghostty"].into_iter().zip(&e.arms) {
            assert_eq!(o.lost, 1, "{arm} lost the first placement\n{report}");
            assert_eq!(o.seen, 2, "{arm}\n{report}");
            assert!(o.evicted.is_empty(), "{arm}\n{report}");
            assert_eq!(o.cases, [0, 0, 0], "{arm}\n{report}");
        }
        assert!(!e.applies(), "{report}");
        for rule in crate::counts::Rule::ALL {
            assert!(
                report.contains(&format!("  {}: not checked", rule.name())),
                "{report}"
            );
        }
        assert!(
            report.contains("not applied: no rule had a case"),
            "{report}"
        );
        let [ours, theirs] = &c.counts;
        assert_eq!(
            c.counted(),
            crate::counts::verdict(ours, theirs),
            "{report}"
        );
    }

    /// Each planted departure: the check over `reads` with `plant` fed to
    /// ghostty, its screen the same as baud-core's, its verdict a
    /// departure from `rule` alone, no rate, and the report naming it.
    #[cfg(have_ghostty)]
    fn planted(reads: &[Vec<u8>], plant: crate::cores::Plant, rule: crate::counts::Rule) {
        use crate::counts::Verdict;
        let c = by_reads(reads, crate::cores::GhosttyImages::Budget, &[plant]);
        let report = c.report("planted").join("\n");
        assert!(c.equal(), "{report}");
        assert_eq!(c.counted(), Verdict::Departs(vec![rule]), "{report}");
        assert!(!c.timed(), "{report}");
        assert!(!c.rated(), "{report}");
        assert!(
            report.contains(&format!(
                "planted: EVICTION ORDER DEPARTS, no rate: {}: ghostty",
                rule.name()
            )),
            "{report}"
        );
    }

    /// **A placed `i=` image evicted while an unplaced one stays departs
    /// from the placed-over-unplaced order**: image 1 placed and scrolled
    /// into the history, so the screens stay alike, image 2 stored
    /// unplaced, and image 1 deleted on ghostty alone.
    #[cfg(have_ghostty)]
    #[test]
    fn planted_r2_departure_is_unequal() {
        let reads = [
            [cup(1), kitty("a=T,i=1"), cup(50), vec![b'\n'; 60]].concat(),
            kitty("a=t,i=2"),
            quiet(),
        ];
        let plant = crate::cores::Plant { read: 3, id: 1 };
        planted(&reads, plant, crate::counts::Rule::PlacedKept);
    }

    /// **The newer of two unplaced images evicted while the older stays
    /// departs from the store order**: images 1 and 2 stored unplaced, a
    /// third placed, and image 2 deleted on ghostty alone.
    #[cfg(have_ghostty)]
    #[test]
    fn planted_r3_departure_is_unequal() {
        let reads = [
            kitty("a=t,i=1"),
            kitty("a=t,i=2"),
            [cup(1), kitty("a=T,i=3")].concat(),
            quiet(),
        ];
        let plant = crate::cores::Plant { read: 4, id: 2 };
        planted(&reads, plant, crate::counts::Rule::OlderFirst);
    }

    /// **The newest image evicted with nothing newer held departs from
    /// the newest surviving**: image 1 placed, image 2 stored unplaced,
    /// and image 2 deleted on ghostty alone in a read that stores nothing.
    #[cfg(have_ghostty)]
    #[test]
    fn planted_r4_departure_is_unequal() {
        let reads = [
            [cup(1), kitty("a=T,i=1")].concat(),
            kitty("a=t,i=2"),
            quiet(),
        ];
        let plant = crate::cores::Plant { read: 3, id: 2 };
        planted(&reads, plant, crate::counts::Rule::NewestSurvives);
    }

    /// **A store under a reused id is neither an eviction nor a
    /// departure**: three images sent by image number, ghostty's first
    /// deleted on ghostty alone just before the third is sent, which
    /// ghostty gives the lowest free id, the deleted one's. The id's
    /// generation moved in a read that transmits, so the image it named
    /// leaves the observed set; nothing is evicted on either arm.
    #[cfg(have_ghostty)]
    #[test]
    fn a_store_under_a_reused_id_is_no_eviction() {
        use crate::counts::{Left, Verdict};
        let reads = [
            [cup(1), kitty("a=T,I=7")].concat(),
            [cup(3), kitty("a=T,I=8")].concat(),
            [cup(5), kitty("a=T,I=9")].concat(),
        ];
        let plant = crate::cores::Plant { read: 3, id: 1 };
        let c = by_reads(&reads, crate::cores::GhosttyImages::Budget, &[plant]);
        let report = c.report("reused").join("\n");
        let e = c.eviction.as_ref().expect("the check ran");
        let [ours, theirs] = &e.arms;
        assert!(ours.evicted.is_empty(), "{report}");
        assert!(theirs.evicted.is_empty(), "{report}");
        assert_eq!(ours.left, [0; 5], "{report}");
        assert_eq!(theirs.left(Left::Moved), 1, "{report}");
        assert_eq!(theirs.seen, 3, "the store under the reused id\n{report}");
        assert!(!matches!(c.counted(), Verdict::Departs(_)), "{report}");
        assert!(
            report.contains("ghostty 1 (generation moved under a transmission 1,"),
            "{report}"
        );
    }

    /// One arm's eviction outcome with `cases` and `evicted`.
    fn order(cases: [usize; 3], evicted: Vec<usize>) -> crate::counts::Order {
        crate::counts::Order {
            seen: 2,
            cases,
            evicted,
            ..crate::counts::Order::default()
        }
    }

    /// A same-screen comparison with these placements and eviction check,
    /// and equal counts.
    fn compared(
        placed: [Vec<super::Placed>; 2],
        eviction: Option<crate::counts::Eviction>,
    ) -> super::Compared {
        let full = crate::counts::Counts {
            stored: Some(1),
            stored_bytes: Some(4),
            held: Some(1),
            held_bytes: Some(4),
            frames: Some(1),
            evicted: Some(0),
            refused: Some(0),
        };
        super::Compared {
            cells: Vec::new(),
            cells_differing: 0,
            placed,
            virtuals: Vec::new(),
            virtuals_compared: 0,
            pixels_unread: 0,
            counts: [full, full],
            eviction,
        }
    }

    /// **A rule with no case prints "not checked"**, never "equal" or
    /// "passed", on each arm and for the rule.
    #[test]
    fn a_rule_with_no_case_prints_not_checked() {
        use crate::counts::{Eviction, FrameCheck, Rule};
        let e = Eviction {
            deletes: false,
            arms: [order([0, 0, 0], vec![]), order([0, 2, 0], vec![1])],
            frames: FrameCheck::default(),
            store_evicted: 0,
        };
        let c = compared([Vec::new(), Vec::new()], Some(e));
        let report = c.report("r").join("\n");
        assert!(
            report.contains("  placed kept over unplaced: not checked"),
            "{report}"
        );
        assert!(
            report.contains(
                "  older stored goes first: baud-core not checked, ghostty 2 cases: passed"
            ),
            "{report}"
        );
        assert!(
            report.contains("  newest survives: not checked"),
            "{report}"
        );
        for rule in [Rule::PlacedKept, Rule::NewestSurvives] {
            let line = report
                .lines()
                .find(|l| l.starts_with(&format!("  {}:", rule.name())))
                .expect("a line for each rule");
            assert!(
                !line.contains("equal") && !line.contains("passed"),
                "{line}"
            );
        }
        let none = Eviction {
            arms: [order([0, 0, 0], vec![]), order([0, 0, 0], vec![])],
            ..c.eviction.clone().expect("given")
        };
        let report = compared([Vec::new(), Vec::new()], Some(none))
            .report("r")
            .join("\n");
        assert!(!report.contains("passed"), "{report}");
        assert!(
            report.contains("not applied: no rule had a case"),
            "{report}"
        );
    }

    /// **A placement one arm shows and the other does not passes only for
    /// an image the other evicted**, and only where the check applies:
    /// baud-core shows images first seen at reads 1 and 3, ghostty only
    /// the one from read 3, having evicted one from read 1.
    #[test]
    fn a_missing_placement_passes_only_for_an_evicted_image() {
        use crate::counts::{Eviction, FrameCheck};
        let at = |row: usize, first: usize| super::Placed {
            row,
            col: 0,
            z: 0,
            picture: super::Picture {
                width: 1,
                height: 1,
                pixels: Some(7),
            },
            first: Some(first),
        };
        let check = |ghostty_evicted: Vec<usize>| Eviction {
            deletes: false,
            arms: [order([0, 0, 0], vec![]), order([0, 1, 1], ghostty_evicted)],
            frames: FrameCheck::default(),
            store_evicted: 0,
        };
        let placed = [vec![at(0, 1), at(2, 3)], vec![at(2, 3)]];
        let c = compared(placed.clone(), Some(check(vec![1])));
        assert!(!c.placements_differ());
        assert!(c.equal());
        assert!(c.rated());
        let report = c.report("r").join("\n");
        assert!(
            report.contains("placements on the screen baud-core 2 and ghostty 1"),
            "{report}"
        );
        assert!(compared(placed.clone(), Some(check(vec![2]))).placements_differ());
        assert!(compared(placed, None).placements_differ());
        let first_differs = [vec![at(0, 1), at(2, 3)], vec![at(2, 4)]];
        assert!(compared(first_differs, Some(check(vec![1]))).placements_differ());
        let not_applied = Eviction {
            store_evicted: 5,
            ..check(vec![1])
        };
        let placed = [vec![at(0, 1), at(2, 3)], vec![at(2, 3)]];
        assert!(compared(placed, Some(not_applied)).placements_differ());
    }

    /// **An upload a bare ESC cuts is dropped by both arms, at different
    /// stages**: a 3x2 RGB upload whose first chunk is cut after half its
    /// data, the rest of it printed as text, then a last chunk. baud-core
    /// cancels at the ESC and refuses the last chunk for having no size;
    /// ghostty takes the ESC as the chunk's end, keeps its half and the last
    /// chunk, and drops the upload when 12 bytes arrive for 18. Neither
    /// stores it, the screens agree, and only baud-core counts a refusal:
    /// mpv-logo is listed for that reason.
    #[cfg(have_ghostty)]
    #[test]
    fn an_upload_a_bare_esc_cuts_is_dropped_by_both_arms_at_different_stages() {
        let p =
            b"\x1b_Ga=T,f=24,s=3,v=2,q=2,m=1;AAAAAAAA\x1b[0mAAAAAAAA\x1b\\\x1b_Gm=0;AAAAAAAA\x1b\\";
        let _hook = HOOK.lock().unwrap_or_else(|e| e.into_inner());
        crate::ffi::ghostty::set_png_decoder(crate::ffi::ghostty::PngDecoder::Registered);
        let c = super::check(p, p.len()).expect("libghostty-vt is linked");
        let report = c.report("cut-upload").join("\n");
        let [ours, theirs] = &c.counts;
        assert_eq!(ours.stored, Some(0), "baud-core stores nothing\n{report}");
        assert_eq!(theirs.stored, Some(0), "ghostty stores nothing\n{report}");
        assert_eq!(
            ours.refused,
            Some(1),
            "baud-core refuses the last chunk\n{report}"
        );
        assert_eq!(theirs.refused, None, "ghostty counts no refusal\n{report}");
        assert_eq!(c.cells_differing, 0, "the screens agree\n{report}");
        assert!(
            super::known_unequal("mpv-logo-40x6.raw")
                .is_some_and(|w| w.contains("different stages")),
            "mpv-logo listed for that reason"
        );
    }

    /// The same screen with a compared count "n/a" on one arm is timed,
    /// marked UNEQUAL-UNKNOWN, its rate printed unquotable, and never
    /// passed as equal.
    #[test]
    fn an_n_a_count_is_timed_unquotable_and_fails() {
        use crate::counts::{Counts, Verdict};
        use crate::report::Rate;
        let full = Counts {
            stored: Some(1),
            stored_bytes: Some(4),
            held: Some(1),
            held_bytes: Some(4),
            frames: Some(1),
            evicted: Some(0),
            refused: Some(0),
        };
        let blind = Counts {
            frames: None,
            evicted: None,
            refused: None,
            ..full
        };
        let c = super::Compared {
            cells: Vec::new(),
            cells_differing: 0,
            placed: [Vec::new(), Vec::new()],
            virtuals: Vec::new(),
            virtuals_compared: 0,
            pixels_unread: 0,
            counts: [full, blind],
            eviction: None,
        };
        let report = c.report("r").join("\n");
        assert!(c.equal(), "{report}");
        assert!(matches!(c.counted(), Verdict::Unknown(_)), "{report}");
        assert!(c.timed(), "{report}");
        assert!(!c.rated(), "{report}");
        assert!(
            report.contains("r: UNEQUAL-UNKNOWN, rate unquotable"),
            "{report}"
        );
        assert!(
            report.contains("animation frames held: baud-core 1, ghostty n/a"),
            "{report}"
        );
        assert_eq!(
            Rate::Unquotable((9.5, 9.0, 10.0)).cell(),
            "       unquotable 9.5"
        );
        let both = super::Compared {
            counts: [full, full],
            ..c
        };
        assert!(both.rated());
    }

    /// Images sent with no client id each count: baud-core gives all of
    /// them the client id 0, and three of one size placed one after
    /// another are three held and three stored, not one.
    #[cfg(have_ghostty)]
    #[test]
    fn images_with_no_client_id_count_one_each() {
        let ws = crate::cores::baud_ws();
        let mut owned = crate::cores::baud_owned(&ws);
        let mut t = crate::cores::baud_new(&mut owned, &ws);
        let mut tally = crate::counts::Tally::default();
        for _ in 0..3 {
            crate::cores::baud_feed(&mut t, b"\x1b_Ga=T,f=24,s=2,v=2;////////////////\x1b\\");
            tally.observe(
                super::baud::held(&t, &[])
                    .into_iter()
                    .map(|(h, _)| h)
                    .collect(),
            );
        }
        let held = super::baud::held(&t, &[]);
        assert_eq!(held.len(), 3, "{held:?}");
        assert!(
            held.iter().all(|(h, _)| h.id == 0),
            "sent with no id: {held:?}"
        );
        let c = tally.counts(None, None, None);
        assert_eq!(c.stored, Some(3));
        assert_eq!(c.held, Some(3));
    }

    /// mpv sends its frames with no client id: one pass of its recording
    /// stores as many images on baud-core as on ghostty, and as many pixel
    /// bytes.
    #[cfg(have_ghostty)]
    #[test]
    fn mpv_s_frames_with_no_client_id_are_counted_alike_on_both_arms() {
        let _hook = HOOK.lock().unwrap_or_else(|e| e.into_inner());
        crate::ffi::ghostty::set_png_decoder(crate::ffi::ghostty::PngDecoder::Registered);
        let p = recording("kitty/mpv-logo-40x6.raw");
        let c = super::check(&p, p.len()).expect("libghostty-vt is linked");
        let report = c.report("mpv-logo-40x6.raw").join("\n");
        let [ours, theirs] = c.counts;
        assert!(c.equal(), "{report}");
        assert!(ours.stored.is_some_and(|n| n > 1), "{report}");
        assert_eq!(ours.stored, theirs.stored, "{report}");
        assert_eq!(ours.stored_bytes, theirs.stored_bytes, "{report}");
    }

    /// Without the library there is no second arm: the check compares
    /// nothing and says so (`None`), which the run reports as no ghostty
    /// figure, never as screens that agree. A build asked for the library
    /// and not finding it stops in build.rs.
    #[cfg(not(have_ghostty))]
    #[test]
    fn without_libghostty_vt_the_check_compares_nothing() {
        let p = recording("kitty/grad-80x24-placeholder.raw");
        assert!(super::check(&p, p.len()).is_none());
    }
}
