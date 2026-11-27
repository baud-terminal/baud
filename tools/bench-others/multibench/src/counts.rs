//! **What each arm stored, counted the same way on both.**
//!
//! The screen check (`equal::check`) compares where a rep ends; these
//! count the work along the way, so a rate is printed only where both arms
//! stored the same images. Both arms are read on the active screen, at
//! every read boundary of the check's rep, through each arm's own API:
//!
//! * the images it holds: every client id the recording names with `i=`,
//!   and every image its placements name, each looked up and kept with
//!   its id and its size. An id an arm assigned itself (an image sent with
//!   `I=` or with no id) is reached through its placements; the two arms
//!   number those differently, so images are compared by count and size,
//!   never by id;
//! * images stored: those held at a boundary and not, by id and size, at
//!   the one before, and their pixel bytes;
//! * at the end: the images held, their pixel bytes and their animation
//!   frames.
//!
//! Pixel bytes are four a pixel on both arms, whatever each keeps them
//! as. The first five counters are compared ([`Counter::COMPARED`]). A
//! compared counter either arm reports as "n/a" proves nothing equal: the
//! recording is [`Verdict::Unknown`], printed UNEQUAL-UNKNOWN, its rate
//! unquotable. Evicted and refused ([`Counter::OWN`]) are baud-core's own:
//! none on baud-core leaves the five to decide, but an upload it refused or
//! an image it evicted is work the other arm may have done and the five do
//! not show, so a count above none there that the other arm does not
//! report is unknown in the same way, and one both report and that differs
//! differs.
//!
//! **Which images each arm evicted, and in what order.** Each arm's images
//! are tracked from the read boundary at which they are first seen
//! ([`Track`]), and an image that is gone at a later boundary is an
//! eviction only where nothing else in the recording can have removed it.
//! Three of the kitty quota's orders are checked on each arm over those
//! evictions ([`Rule`]); where one departs the recording is
//! [`Verdict::Departs`], and where they hold with a case the images held
//! and evicted stop deciding the verdict ([`judged`]).

// Without libghostty-vt nothing is counted on the second arm.
#![cfg_attr(not(have_ghostty), allow(dead_code))]

/// Bytes a pixel is counted at, on both arms.
const PIXEL_BYTES: usize = 4;

/// One image an arm holds: its client id, as that arm numbers it, and its
/// size in pixels. baud-core gives every image the client sent with no id
/// the client id 0, so it names each of those by when its store took it
/// ([`Held::unnamed`]): two of them of one size are two images, as they
/// are on ghostty, which numbers each itself.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Held {
    pub(crate) id: u32,
    /// baud-core's name for an image of client id 0: when its store took
    /// it, one for each image stored. `None` for an image with a client
    /// id, and for every image on ghostty.
    pub(crate) unnamed: Option<baud_domain::ImageAge>,
    pub(crate) width: usize,
    pub(crate) height: usize,
}

impl Held {
    /// Its pixels at [`PIXEL_BYTES`] each.
    pub(crate) fn bytes(self) -> usize {
        self.width * self.height * PIXEL_BYTES
    }
}

/// One arm's images over a rep, observed at each read boundary.
#[derive(Clone, Default, Debug)]
pub(crate) struct Tally {
    /// What the last boundary found, sorted.
    last: Vec<Held>,
    stored: usize,
    stored_bytes: usize,
}

impl Tally {
    /// The images held at this boundary: each not held at the one before
    /// counts as stored.
    pub(crate) fn observe(&mut self, mut now: Vec<Held>) {
        now.sort_unstable();
        now.dedup();
        for h in &now {
            if self.last.binary_search(h).is_err() {
                self.stored += 1;
                self.stored_bytes += h.bytes();
            }
        }
        self.last = now;
    }

    /// The images held at the last boundary.
    pub(crate) fn held(&self) -> &[Held] {
        &self.last
    }

    /// The counts this tally gives, with the arm's `frames` held at the
    /// end and its own `evicted` and `refused`.
    pub(crate) fn counts(
        &self,
        frames: Option<usize>,
        evicted: Option<usize>,
        refused: Option<usize>,
    ) -> Counts {
        Counts {
            stored: Some(self.stored),
            stored_bytes: Some(self.stored_bytes),
            held: Some(self.last.len()),
            held_bytes: Some(self.last.iter().map(|h| h.bytes()).sum()),
            frames,
            evicted,
            refused,
        }
    }
}

/// One counter, by name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Counter {
    Stored,
    StoredBytes,
    Held,
    HeldBytes,
    Frames,
    Evicted,
    Refused,
}

impl Counter {
    /// The counters both arms are compared on.
    pub(crate) const COMPARED: [Counter; 5] = [
        Counter::Stored,
        Counter::StoredBytes,
        Counter::Held,
        Counter::HeldBytes,
        Counter::Frames,
    ];

    /// The counters baud-core alone reports: compared only where an arm
    /// reports one above none.
    pub(crate) const OWN: [Counter; 2] = [Counter::Evicted, Counter::Refused];

    /// The counters that stop deciding the verdict where the eviction
    /// check applies ([`Eviction::applies`]): what each arm holds is what
    /// its quota left, and the order it evicted in is checked instead.
    pub(crate) const EXEMPT: [Counter; 3] = [Counter::Held, Counter::HeldBytes, Counter::Evicted];

    /// Every counter, in the order they are printed.
    pub(crate) const ALL: [Counter; 7] = [
        Counter::Stored,
        Counter::StoredBytes,
        Counter::Held,
        Counter::HeldBytes,
        Counter::Frames,
        Counter::Evicted,
        Counter::Refused,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Counter::Stored => "images stored",
            Counter::StoredBytes => "pixel bytes stored",
            Counter::Held => "images held",
            Counter::HeldBytes => "pixel bytes held",
            Counter::Frames => "animation frames held",
            Counter::Evicted => "evicted",
            Counter::Refused => "refused",
        }
    }
}

/// One arm's counters from one rep; `None` is a counter it does not
/// report.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Counts {
    pub(crate) stored: Option<usize>,
    pub(crate) stored_bytes: Option<usize>,
    pub(crate) held: Option<usize>,
    pub(crate) held_bytes: Option<usize>,
    pub(crate) frames: Option<usize>,
    pub(crate) evicted: Option<usize>,
    pub(crate) refused: Option<usize>,
}

impl Counts {
    pub(crate) fn get(&self, c: Counter) -> Option<usize> {
        match c {
            Counter::Stored => self.stored,
            Counter::StoredBytes => self.stored_bytes,
            Counter::Held => self.held,
            Counter::HeldBytes => self.held_bytes,
            Counter::Frames => self.frames,
            Counter::Evicted => self.evicted,
            Counter::Refused => self.refused,
        }
    }
}

impl std::fmt::Display for Counts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, c) in Counter::ALL.into_iter().enumerate() {
            let sep = if i == 0 { "" } else { ", " };
            match self.get(c) {
                Some(n) => write!(f, "{sep}{} {n}", c.name())?,
                None => write!(f, "{sep}{} n/a", c.name())?,
            }
        }
        Ok(())
    }
}

/// The counters both arms report and that differ: each compared counter,
/// and each of baud-core's own that the other arm reports too.
pub(crate) fn differing(a: &Counts, b: &Counts) -> Vec<Counter> {
    Counter::COMPARED
        .into_iter()
        .chain(Counter::OWN)
        .filter(|&c| matches!((a.get(c), b.get(c)), (Some(x), Some(y)) if x != y))
        .collect()
}

/// The counters that leave equal work unshown: each compared counter at
/// least one arm reports as "n/a", and each of baud-core's own that one arm
/// reports above none and the other does not report.
pub(crate) fn unreported(a: &Counts, b: &Counts) -> Vec<Counter> {
    let compared = Counter::COMPARED
        .into_iter()
        .filter(|&c| a.get(c).is_none() || b.get(c).is_none());
    let own = Counter::OWN
        .into_iter()
        .filter(|&c| matches!((a.get(c), b.get(c)), (Some(1..), None) | (None, Some(1..))));
    compared.chain(own).collect()
}

/// What two arms' counts say about the work each did.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Verdict {
    /// Every compared counter reported by both arms, and equal.
    Equal,
    /// These compared counters differ: the arms did different work.
    Differ(Vec<Counter>),
    /// None differs, and these compared counters are "n/a" on an arm: equal
    /// work is not shown, so the rate is printed and is not quotable.
    Unknown(Vec<Counter>),
    /// An arm evicted out of these orders of the kitty quota: as
    /// [`Verdict::Differ`], not timed and no rate.
    Departs(Vec<Rule>),
}

/// The verdict on `a` against `b`. A counter that differs outranks one
/// that is "n/a".
pub(crate) fn verdict(a: &Counts, b: &Counts) -> Verdict {
    settle(differing(a, b), unreported(a, b))
}

/// [`Verdict::Differ`] on `differ` if any, else [`Verdict::Unknown`] on
/// `missing` if any, else [`Verdict::Equal`].
fn settle(differ: Vec<Counter>, missing: Vec<Counter>) -> Verdict {
    if !differ.is_empty() {
        Verdict::Differ(differ)
    } else if !missing.is_empty() {
        Verdict::Unknown(missing)
    } else {
        Verdict::Equal
    }
}

/// The verdict on `a` against `b` with the eviction check `e`:
///
/// * a recording that deletes images itself, or one with no check, is
///   judged by [`verdict`];
/// * an order departed on either arm is [`Verdict::Departs`], whatever
///   the counters say;
/// * where the check applies ([`Eviction::applies`]), the counters in
///   [`Counter::EXEMPT`] do not decide, and the animation frames are
///   compared image by image ([`FrameCheck`]) rather than as a sum;
/// * otherwise [`verdict`].
pub(crate) fn judged(a: &Counts, b: &Counts, e: Option<&Eviction>) -> Verdict {
    let Some(e) = e.filter(|e| !e.deletes) else {
        return verdict(a, b);
    };
    let departs = e.departs();
    if !departs.is_empty() {
        return Verdict::Departs(departs);
    }
    if !e.applies() {
        return verdict(a, b);
    }
    let decides = |c: &Counter| !Counter::EXEMPT.contains(c) && *c != Counter::Frames;
    let mut differ: Vec<Counter> = differing(a, b).into_iter().filter(decides).collect();
    let mut missing: Vec<Counter> = unreported(a, b).into_iter().filter(decides).collect();
    if e.frames.differ > 0 {
        differ.push(Counter::Frames);
    } else if e.frames.unread > 0 {
        missing.push(Counter::Frames);
    }
    let order = |c: &Counter| Counter::ALL.iter().position(|x| x == c);
    differ.sort_by_key(order);
    missing.sort_by_key(order);
    settle(differ, missing)
}

/// One of the kitty quota's orders, each checked on each arm over the
/// images it evicted: images with no placement go before those with one,
/// and of either kind the older go first, "to make space for the new one"
/// (`[[source: kitty-graphics]]`, "Image persistence and storage quotas").
/// Older is by when the image was stored, which both arms are read for as
/// the read boundary at which it was first seen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Rule {
    /// No `i=` image with a placement is evicted while an `i=` image with
    /// none stays.
    PlacedKept,
    /// No image is evicted while one first seen at an earlier boundary,
    /// placed as it was, stays.
    OlderFirst,
    /// The image first seen last is not evicted where nothing newer is
    /// held.
    NewestSurvives,
}

impl Rule {
    /// Every rule, in the order they are printed.
    pub(crate) const ALL: [Rule; 3] = [Rule::PlacedKept, Rule::OlderFirst, Rule::NewestSurvives];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Rule::PlacedKept => "placed kept over unplaced",
            Rule::OlderFirst => "older stored goes first",
            Rule::NewestSurvives => "newest survives",
        }
    }

    fn index(self) -> usize {
        match self {
            Rule::PlacedKept => 0,
            Rule::OlderFirst => 1,
            Rule::NewestSurvives => 2,
        }
    }
}

/// Why a tracked image left the observed set without counting as an
/// eviction: something other than the quota may have removed it, or the
/// handle may now name another image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Left {
    /// Its generation moved in a read that transmits: its id may name a
    /// new store (ghostty).
    Moved,
    /// The read transmits its `i=` again: a new store under the same id.
    Resent,
    /// It has no id, and goes with its placements.
    NoId,
    /// The read resets the terminal (RIS) or changes the screen
    /// (`?47`, `?1047`, `?1049`).
    Reset,
    /// The recording deletes images itself (`a=d`).
    Deleted,
}

impl Left {
    /// Every reason, in the order they are printed.
    pub(crate) const ALL: [Left; 5] = [
        Left::Moved,
        Left::Resent,
        Left::NoId,
        Left::Reset,
        Left::Deleted,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Left::Moved => "generation moved under a transmission",
            Left::Resent => "re-transmitted id",
            Left::NoId => "no id",
            Left::Reset => "reset or change of screen",
            Left::Deleted => "the recording deletes",
        }
    }

    fn index(self) -> usize {
        match self {
            Left::Moved => 0,
            Left::Resent => 1,
            Left::NoId => 2,
            Left::Reset => 3,
            Left::Deleted => 4,
        }
    }
}

/// The first departure from a rule on one arm: the image it evicted and
/// the one that stays against the rule, each by the read at which it was
/// first seen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Departure {
    pub(crate) rule: Rule,
    /// The read the evicted image was first seen at.
    pub(crate) gone: usize,
    /// The read the image that stays was first seen at; `None` for
    /// [`Rule::NewestSurvives`], where nothing newer is held.
    pub(crate) kept: Option<usize>,
}

impl Departure {
    /// What the arm did, after its name.
    pub(crate) fn says(&self) -> String {
        match (self.rule, self.kept) {
            (Rule::PlacedKept, Some(k)) => format!(
                "evicted the placed image stored at read {} while the unplaced one stored at \
                 read {k} stays",
                self.gone
            ),
            (Rule::OlderFirst, Some(k)) => format!(
                "evicted the image stored at read {} while the one stored at read {k} stays",
                self.gone
            ),
            (_, _) => format!(
                "evicted the newest image, stored at read {}, and holds none stored after it",
                self.gone
            ),
        }
    }
}

/// What one arm's evictions say about the rules.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(crate) struct Order {
    /// Images tracked over the rep.
    pub(crate) seen: usize,
    /// Cases each rule was checked on, by [`Rule::ALL`]: an evicted image
    /// with at least one image held at the boundary before that the rule
    /// compares it with.
    pub(crate) cases: [usize; 3],
    /// The first departure from each rule that departed.
    pub(crate) departures: Vec<Departure>,
    /// The read each evicted image was first seen at, one per eviction.
    pub(crate) evicted: Vec<usize>,
    /// Images that left the observed set, by [`Left::ALL`].
    pub(crate) left: [usize; 5],
    /// Placements lost from an image still held: never an eviction.
    pub(crate) lost: usize,
}

impl Order {
    /// The cases `rule` was checked on.
    pub(crate) fn cases(&self, rule: Rule) -> usize {
        self.cases[rule.index()]
    }

    /// The images that left the observed set for `why`.
    pub(crate) fn left(&self, why: Left) -> usize {
        self.left[why.index()]
    }

    /// Whether `rule` departed.
    pub(crate) fn departs(&self, rule: Rule) -> bool {
        self.departures.iter().any(|d| d.rule == rule)
    }

    /// Records a departure from `rule`, the first only.
    fn depart(&mut self, rule: Rule, gone: usize, kept: Option<usize>) {
        if !self.departs(rule) {
            self.departures.push(Departure { rule, gone, kept });
        }
    }
}

/// One image an arm can see at a read boundary: its placements name it,
/// or its client id does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Seen<H> {
    /// How the arm finds it again: baud-core's slot and when its store
    /// took it, ghostty's id and generation.
    pub(crate) handle: H,
    /// Whether it has a placement.
    pub(crate) placed: bool,
    /// Its `i=`, where the client named it so.
    pub(crate) named: Option<u32>,
    /// Whether it has an id at all, `i=` or one given for an `I=` number.
    pub(crate) has_id: bool,
}

/// What an arm's store says of a tracked image through its handle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Lookup<H> {
    /// The handle names no image.
    Gone,
    /// The handle names an image, now read as `handle`.
    Held { handle: H, placed: bool },
}

/// A tracked image: its handle, the read it was first seen at, and
/// whether it was placed at the last boundary.
#[derive(Clone, Copy, Debug)]
struct Entry<H> {
    handle: H,
    first: usize,
    placed: bool,
    named: Option<u32>,
    has_id: bool,
}

/// One arm's images, each from the read boundary at which it was first
/// seen, and the rules checked over those it evicted.
///
/// At each boundary every tracked image is read again through its handle.
/// One still held stays. One whose handle names another image now (a
/// ghostty generation that moved) stays only where the read transmits
/// nothing. One gone is an eviction only where the recording deletes no
/// image, the read neither resets nor changes the screen nor transmits its
/// `i=` again, and it has an id; otherwise it leaves the observed set
/// ([`Left`]). An image held but no longer placed has lost its placement,
/// which is never an eviction. Images first seen at one boundary have no
/// order between them.
#[derive(Clone, Debug)]
pub(crate) struct Track<H> {
    deletes: bool,
    reads: usize,
    images: Vec<Entry<H>>,
    pub(crate) order: Order,
}

impl<H: Copy + PartialEq> Track<H> {
    /// An empty track for a recording that deletes images itself or not.
    pub(crate) fn new(deletes: bool) -> Track<H> {
        Track {
            deletes,
            reads: 0,
            images: Vec::new(),
            order: Order::default(),
        }
    }

    /// The next read boundary: `read` is what the read just fed did, `now`
    /// the images the arm can see, and `lookup` reads a tracked image
    /// through its handle.
    pub(crate) fn step(
        &mut self,
        read: &ReadFacts,
        now: &[Seen<H>],
        lookup: impl Fn(H) -> Lookup<H>,
    ) {
        self.reads += 1;
        let b = self.reads;
        let before = std::mem::take(&mut self.images);
        // Each image held before, with whether it is placed now if it stays.
        let mut stays: Vec<Option<bool>> = vec![None; before.len()];
        let mut gone: Vec<usize> = Vec::new();
        for (i, e) in before.iter().enumerate() {
            let left = match lookup(e.handle) {
                Lookup::Held { handle, placed } => {
                    let resent = e.named.is_some_and(|id| read.sends(id));
                    if handle != e.handle && resent {
                        Some(Left::Resent)
                    } else if handle != e.handle && read.resets {
                        Some(Left::Reset)
                    } else if handle != e.handle && read.transmits {
                        Some(Left::Moved)
                    } else {
                        if e.placed && !placed {
                            self.order.lost += 1;
                        }
                        stays[i] = Some(placed);
                        self.images.push(Entry {
                            handle,
                            placed,
                            ..*e
                        });
                        None
                    }
                }
                Lookup::Gone => {
                    if self.deletes {
                        Some(Left::Deleted)
                    } else if read.resets {
                        Some(Left::Reset)
                    } else if e.named.is_some_and(|id| read.sends(id)) {
                        Some(Left::Resent)
                    } else if !e.has_id {
                        Some(Left::NoId)
                    } else {
                        gone.push(i);
                        None
                    }
                }
            };
            if let Some(why) = left {
                self.order.left[why.index()] += 1;
            }
        }
        let mut fresh = false;
        for s in now {
            if self.images.iter().any(|e| e.handle == s.handle) {
                continue;
            }
            fresh = true;
            self.order.seen += 1;
            self.images.push(Entry {
                handle: s.handle,
                first: b,
                placed: s.placed,
                named: s.named,
                has_id: s.has_id,
            });
        }
        self.rules(&before, &stays, &gone, fresh);
    }

    /// The rules over `gone`, the images of `before` evicted at this
    /// boundary; `stays` holds, for each image of `before`, whether it is
    /// placed now if it stays, and `fresh` whether an image first seen at
    /// this boundary is held.
    fn rules(&mut self, before: &[Entry<H>], stays: &[Option<bool>], gone: &[usize], fresh: bool) {
        let newest = before.iter().map(|e| e.first).max();
        let newest = newest
            .filter(|&top| before.iter().filter(|e| e.first == top).count() == 1)
            .and_then(|top| before.iter().position(|e| e.first == top));
        for &x in gone {
            let xe = before[x];
            self.order.evicted.push(xe.first);
            // The placed-over-unplaced order compares x with the `i=`
            // images unplaced before, the store order with those placed as
            // x was and first seen at another read; each departs where one
            // of them stays, placed as it was.
            let (mut unplaced, mut unplaced_kept) = (false, None);
            let (mut alike, mut older_kept) = (false, None);
            for (y, ye) in before.iter().enumerate() {
                if y == x {
                    continue;
                }
                let kept = stays[y] == Some(ye.placed);
                if ye.named.is_some() && !ye.placed {
                    unplaced = true;
                    if kept && unplaced_kept.is_none() {
                        unplaced_kept = Some(ye.first);
                    }
                }
                if ye.first != xe.first && ye.placed == xe.placed {
                    alike = true;
                    if kept && ye.first < xe.first && older_kept.is_none() {
                        older_kept = Some(ye.first);
                    }
                }
            }

            if xe.placed && xe.named.is_some() && unplaced {
                self.order.cases[Rule::PlacedKept.index()] += 1;
                if let Some(k) = unplaced_kept {
                    self.order.depart(Rule::PlacedKept, xe.first, Some(k));
                }
            }

            if alike {
                self.order.cases[Rule::OlderFirst.index()] += 1;
                if let Some(k) = older_kept {
                    self.order.depart(Rule::OlderFirst, xe.first, Some(k));
                }
            }

            if let Some(n) = newest {
                self.order.cases[Rule::NewestSurvives.index()] += 1;
                if n == x && !fresh {
                    self.order.depart(Rule::NewestSurvives, xe.first, None);
                }
            }
        }
    }

    /// The read at which the tracked image `is` picks was first seen.
    pub(crate) fn first(&self, is: impl Fn(&H) -> bool) -> Option<usize> {
        self.images.iter().find(|e| is(&e.handle)).map(|e| e.first)
    }

    /// Every image held at the last boundary: its handle, the read it was
    /// first seen at, and its `i=`.
    pub(crate) fn held(&self) -> impl Iterator<Item = (H, usize, Option<u32>)> + '_ {
        self.images.iter().map(|e| (e.handle, e.first, e.named))
    }
}

/// What a recording does that the eviction check reads, by where in it
/// each happens. A command counts at the byte that ends it.
#[derive(Clone, Default, Debug)]
pub(crate) struct Scan {
    /// Each transmission (`a=t`, `a=T`, or no `a=`, a continuation chunk
    /// among them), with the `i=` it stores under.
    sends: Vec<(usize, Option<u32>)>,
    /// Each RIS and each change of screen (`?47`, `?1047`, `?1049`).
    resets: Vec<usize>,
    /// Whether any kitty command deletes (`a=d`).
    pub(crate) deletes: bool,
}

/// What one read of a recording does.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(crate) struct ReadFacts {
    /// Whether it ends a transmission.
    pub(crate) transmits: bool,
    /// The `i=` of each transmission it ends, sorted.
    pub(crate) ids: Vec<u32>,
    /// Whether it resets the terminal or changes the screen.
    pub(crate) resets: bool,
}

impl ReadFacts {
    /// Whether it ends a transmission under `i=id`.
    fn sends(&self, id: u32) -> bool {
        self.ids.binary_search(&id).is_ok()
    }
}

impl Scan {
    /// The recording `p`, scanned.
    pub(crate) fn of(p: &[u8]) -> Scan {
        let mut s = Scan::default();
        // The `i=` of a chunked upload still open: its continuation
        // chunks carry none of their own.
        let mut open: Option<u32> = None;
        let mut at = 0;
        while at + 1 < p.len() {
            if p[at] != 0x1b {
                at += 1;
                continue;
            }
            match p[at + 1] {
                b'_' | b'P' | b']' | b'X' | b'^' => {
                    let body = at + 2;
                    let stop = p[body..]
                        .iter()
                        .position(|&b| matches!(b, 0x1b | 0x07))
                        .map_or(p.len(), |e| body + e);
                    let st = p.get(stop) == Some(&0x1b) && p.get(stop + 1) == Some(&b'\\');
                    let end = if st { stop + 1 } else { stop.min(p.len() - 1) };
                    if p[at + 1] == b'_' && p.get(body) == Some(&b'G') {
                        s.command(&p[body + 1..stop], end, &mut open);
                    }
                    // A bare ESC ends the string and starts what follows.
                    at = if st { stop + 2 } else { stop };
                }
                b'[' => {
                    let mut i = at + 2;
                    while i < p.len() && p[i] != 0x1b && !(0x40..=0x7e).contains(&p[i]) {
                        i += 1;
                    }
                    if i < p.len() && matches!(p[i], b'h' | b'l') {
                        let screen = p[at + 2..i].strip_prefix(b"?").is_some_and(|params| {
                            params
                                .split(|&b| b == b';')
                                .any(|v| matches!(v, b"47" | b"1047" | b"1049"))
                        });
                        if screen {
                            s.resets.push(i);
                        }
                    }
                    at = if i < p.len() && p[i] == 0x1b {
                        i
                    } else {
                        i + 1
                    };
                }
                b'c' => {
                    s.resets.push(at + 1);
                    at += 2;
                }
                _ => at += 2,
            }
        }
        s
    }

    /// One kitty command's keys, ended at `end`.
    fn command(&mut self, keys: &[u8], end: usize, open: &mut Option<u32>) {
        let keys = keys.split(|&b| b == b';').next().unwrap_or(&[]);
        let (mut action, mut id, mut more) = (None, None, false);
        for kv in keys.split(|&b| b == b',') {
            match kv {
                [b'a', b'=', a] => action = Some(*a),
                [b'm', b'=', m] => more = *m == b'1',
                _ => {
                    if let Some(v) = kv.strip_prefix(b"i=") {
                        id = std::str::from_utf8(v)
                            .ok()
                            .and_then(|v| v.parse::<u32>().ok());
                    }
                }
            }
        }
        match action {
            Some(b'd') => self.deletes = true,
            None | Some(b't' | b'T') => {
                let id = id.or(*open);
                self.sends.push((end, id));
                *open = if more { id } else { None };
            }
            Some(_) => {}
        }
    }

    /// What the read of `len` bytes at `at` in the recording does.
    pub(crate) fn read(&self, at: usize, len: usize) -> ReadFacts {
        let within = |o: &usize| (at..at + len).contains(o);
        let lo = self.sends.partition_point(|&(o, _)| o < at);
        let sends: Vec<_> = self.sends[lo..]
            .iter()
            .take_while(|(o, _)| within(o))
            .collect();
        let mut ids: Vec<u32> = sends.iter().filter_map(|(_, id)| *id).collect();
        ids.sort_unstable();
        ids.dedup();
        ReadFacts {
            transmits: !sends.is_empty(),
            ids,
            resets: self.resets.iter().any(within),
        }
    }
}

/// One image held at the end, for the frame check: the read it was first
/// seen at, its `i=`, and its animation frames, `None` where they could
/// not be read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Framed {
    pub(crate) first: usize,
    pub(crate) named: Option<u32>,
    pub(crate) frames: Option<usize>,
}

/// The animation frames of the images both arms hold, compared image by
/// image.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct FrameCheck {
    /// Images compared.
    pub(crate) compared: usize,
    /// Of those, the ones whose frames differ.
    pub(crate) differ: usize,
    /// Images whose frames an arm could not read.
    pub(crate) unread: usize,
    /// Images held on one arm alone, or first seen at a boundary where the
    /// arms hold different numbers of unnamed images, so never paired.
    pub(crate) unmatched: usize,
}

/// Each image both arms hold, its frames on one against the other.
/// Images are paired by the read they were first seen at, and within one
/// read by `i=` where named; the unnamed images of one read are compared
/// as a multiset of frame counts, sorted, and never paired by a guessed
/// order, which only holds where both arms hold as many of them.
pub(crate) fn frames_per_image(a: &[Framed], b: &[Framed]) -> FrameCheck {
    let mut c = FrameCheck::default();
    let mut firsts: Vec<usize> = a.iter().chain(b).map(|f| f.first).collect();
    firsts.sort_unstable();
    firsts.dedup();
    let pair = |c: &mut FrameCheck, x: Option<usize>, y: Option<usize>| match (x, y) {
        (Some(x), Some(y)) => {
            c.compared += 1;
            c.differ += usize::from(x != y);
        }
        _ => c.unread += 1,
    };
    for f in firsts {
        let at = |side: &[Framed], named: bool| -> Vec<Framed> {
            side.iter()
                .filter(|x| x.first == f && x.named.is_some() == named)
                .copied()
                .collect()
        };
        let (an, bn) = (at(a, true), at(b, true));
        for x in &an {
            match bn.iter().find(|y| y.named == x.named) {
                Some(y) => pair(&mut c, x.frames, y.frames),
                None => c.unmatched += 1,
            }
        }
        c.unmatched += bn
            .iter()
            .filter(|y| !an.iter().any(|x| x.named == y.named))
            .count();
        let (au, bu) = (at(a, false), at(b, false));
        if au.len() != bu.len() {
            c.unmatched += au.len() + bu.len();
            continue;
        }
        let read = |side: &[Framed]| side.iter().map(|x| x.frames).collect::<Option<Vec<_>>>();
        match (read(&au), read(&bu)) {
            (Some(mut x), Some(mut y)) => {
                x.sort_unstable();
                y.sort_unstable();
                c.compared += x.len();
                c.differ += x.iter().zip(&y).filter(|(p, q)| p != q).count();
            }
            _ => c.unread += au.len(),
        }
    }
    c
}

/// The eviction check over one rep: what each arm evicted, the frame
/// check, and baud-core's store's own count of the images it evicted.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Eviction {
    /// Whether the recording deletes images itself (`a=d`): then nothing
    /// is classified as evicted and no rule is checked.
    pub(crate) deletes: bool,
    /// baud-core's and ghostty's.
    pub(crate) arms: [Order; 2],
    pub(crate) frames: FrameCheck,
    /// `images_evicted` from baud-core's store: what its classified
    /// evictions are checked against.
    pub(crate) store_evicted: usize,
}

impl Eviction {
    /// The rules that departed on either arm.
    pub(crate) fn departs(&self) -> Vec<Rule> {
        Rule::ALL
            .into_iter()
            .filter(|&r| self.arms.iter().any(|o| o.departs(r)))
            .collect()
    }

    /// The rules with a case on either arm.
    pub(crate) fn checked(&self) -> Vec<Rule> {
        Rule::ALL
            .into_iter()
            .filter(|&r| self.arms.iter().any(|o| o.cases(r) > 0))
            .collect()
    }

    /// The positive control: the evictions classified on baud-core are
    /// the ones its store counted.
    pub(crate) fn control(&self) -> bool {
        self.arms[0].evicted.len() == self.store_evicted
    }

    /// Whether the check decides in place of [`Counter::EXEMPT`]: the
    /// recording deletes nothing itself, a rule has a case, none departed,
    /// and the positive control holds.
    pub(crate) fn applies(&self) -> bool {
        !self.deletes && !self.checked().is_empty() && self.departs().is_empty() && self.control()
    }
}

/// Every client image id the recording's kitty commands name with `i=`,
/// sorted, each once.
pub(crate) fn kitty_ids(p: &[u8]) -> Vec<u32> {
    const START: &[u8] = b"\x1b_G";
    let mut ids = Vec::new();
    let mut at = 0;
    while let Some(k) = p[at..].windows(START.len()).position(|w| w == START) {
        let keys = at + k + START.len();
        let end = p[keys..]
            .iter()
            .position(|&b| matches!(b, b';' | 0x1b | 0x07))
            .map_or(p.len(), |e| keys + e);
        for kv in p[keys..end].split(|&b| b == b',') {
            let id = kv
                .strip_prefix(b"i=")
                .and_then(|v| std::str::from_utf8(v).ok())
                .and_then(|v| v.parse::<u32>().ok());
            ids.extend(id);
        }
        at = end;
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(id: u32, width: usize, height: usize) -> Held {
        Held {
            id,
            unnamed: None,
            width,
            height,
        }
    }

    /// An image counts as stored when it appears, again when it comes
    /// back after a boundary without it, and not while it stays.
    #[test]
    fn an_image_is_stored_each_time_it_appears() {
        let mut t = Tally::default();
        t.observe(vec![]);
        t.observe(vec![held(1, 2, 3)]);
        t.observe(vec![held(1, 2, 3), held(7, 1, 1)]);
        t.observe(vec![]);
        t.observe(vec![held(1, 2, 3)]);
        let c = t.counts(Some(1), None, None);
        assert_eq!(c.stored, Some(3));
        assert_eq!(c.stored_bytes, Some((6 + 1 + 6) * 4));
        assert_eq!(c.held, Some(1));
        assert_eq!(c.held_bytes, Some(24));
    }

    /// Evicted and refused are baud-core's own: none on baud-core, and the
    /// other arm not reporting them, leaves the verdict to the five; an
    /// upload refused or an image evicted that the other arm does not
    /// report proves no equal work, and a compared counter that differs
    /// outranks it; both arms reporting one, it is compared.
    #[test]
    fn what_baud_core_refused_or_evicted_and_the_other_arm_does_not_report_is_unknown() {
        let mut a = Tally::default();
        a.observe(vec![held(1, 1, 1)]);
        let theirs = a.counts(Some(1), None, None);
        assert_eq!(
            verdict(&a.counts(Some(1), Some(0), Some(0)), &theirs),
            Verdict::Equal
        );
        let ours = a.counts(Some(1), Some(3), Some(4));
        assert_eq!(
            verdict(&ours, &theirs),
            Verdict::Unknown(vec![Counter::Evicted, Counter::Refused])
        );
        assert_eq!(
            verdict(&theirs, &ours),
            Verdict::Unknown(vec![Counter::Evicted, Counter::Refused])
        );
        assert_eq!(
            verdict(&a.counts(Some(1), Some(0), Some(4)), &theirs),
            Verdict::Unknown(vec![Counter::Refused])
        );
        assert_eq!(
            verdict(&ours, &a.counts(Some(2), None, None)),
            Verdict::Differ(vec![Counter::Frames])
        );
        assert_eq!(
            verdict(&ours, &a.counts(Some(1), Some(3), Some(5))),
            Verdict::Differ(vec![Counter::Refused])
        );
        assert_eq!(verdict(&ours, &ours), Verdict::Equal);
        assert_eq!(
            a.counts(Some(2), None, None).to_string(),
            "images stored 1, pixel bytes stored 4, images held 1, pixel bytes held 4, \
             animation frames held 2, evicted n/a, refused n/a"
        );
    }

    /// A compared counter that is "n/a" on one arm, either arm, proves no
    /// equal work: the verdict is unknown, never equal. A counter that
    /// differs outranks it.
    #[test]
    fn one_arm_n_a_is_unequal_unknown() {
        let mut a = Tally::default();
        a.observe(vec![held(1, 2, 2)]);
        let full = a.counts(Some(1), None, None);
        let blind = a.counts(None, None, None);
        assert_eq!(verdict(&full, &full), Verdict::Equal);
        assert_eq!(
            verdict(&full, &blind),
            Verdict::Unknown(vec![Counter::Frames])
        );
        assert_eq!(
            verdict(&blind, &full),
            Verdict::Unknown(vec![Counter::Frames])
        );
        let mut b = Tally::default();
        b.observe(vec![held(1, 2, 3)]);
        assert_eq!(
            verdict(&b.counts(None, None, None), &full),
            Verdict::Differ(vec![Counter::StoredBytes, Counter::HeldBytes])
        );
    }

    /// The ids are read from the keys alone, never from the payload.
    #[test]
    fn the_ids_are_the_i_keys() {
        let p =
            b"\x1b_Ga=T,i=31,f=100;aT0=\x1b\\x\x1b_Gi=5\x07\x1b_Ga=t,I=9;i=4\x1b\\\x1b_Gi=31\x1b\\";
        assert_eq!(kitty_ids(p), vec![5, 31]);
    }

    /// A transmission counts in the read its terminator lands in, a
    /// continuation chunk under the `i=` of the chunk that opened it; a
    /// placement and a frame are no transmission; RIS and a change of
    /// screen are resets; and a recording that sends `a=d` deletes.
    #[test]
    fn a_transmission_counts_in_the_read_its_end_lands_in() {
        let open = b"\x1b_Ga=t,i=4,f=32,s=1,v=1,m=1;AAAA\x1b\\";
        let mut p = open.to_vec();
        p.extend_from_slice(b"\x1b_Gm=0;/w==\x1b\\");
        let cont = p.len();
        p.extend_from_slice(b"\x1b_Ga=p,i=4\x1b\\\x1b_Ga=f,i=4;AAAA\x1b\\");
        let quiet = p.len();
        p.extend_from_slice(b"\x1b[?1049h");
        let screen = p.len();
        p.extend_from_slice(b"\x1bc");
        let s = Scan::of(&p);
        assert!(!s.deletes);
        // The opening chunk ends one byte before its read does.
        let first = s.read(0, open.len());
        assert!(first.transmits && first.ids == [4], "{first:?}");
        assert!(!s.read(0, open.len() - 1).transmits);
        let second = s.read(open.len(), cont - open.len());
        assert!(second.transmits && second.ids == [4], "{second:?}");
        assert_eq!(s.read(cont, quiet - cont), ReadFacts::default());
        assert!(s.read(quiet, screen - quiet).resets);
        assert!(s.read(screen, p.len() - screen).resets);
        p.extend_from_slice(b"\x1b_Ga=d,d=I,i=4\x1b\\");
        assert!(Scan::of(&p).deletes);
    }

    /// ghostty's store as a list of (id, generation, placed).
    type Store = [(u32, u64, bool)];

    /// Each image of `store`, as ghostty's reads give it.
    fn seen(store: &Store) -> Vec<Seen<(u32, u64)>> {
        store
            .iter()
            .map(|&(id, generation, placed)| Seen {
                handle: (id, generation),
                placed,
                named: Some(id),
                has_id: true,
            })
            .collect()
    }

    /// One read boundary over `store`.
    fn feed(t: &mut Track<(u32, u64)>, read: &ReadFacts, store: &Store) {
        t.step(read, &seen(store), |(id, _)| {
            store
                .iter()
                .find(|s| s.0 == id)
                .map_or(Lookup::Gone, |&(id, generation, placed)| Lookup::Held {
                    handle: (id, generation),
                    placed,
                })
        });
    }

    /// **An id whose generation moved where the read transmits may name a
    /// new store**, and it leaves the observed set rather than keep its
    /// age; one that moved where nothing is transmitted keeps it. Two
    /// images first seen at one read have no order, so evicting either is
    /// no case for any rule. The rules' positive control: the newest image
    /// evicted while older ones stay departs from both orders.
    #[test]
    fn ambiguous_ghostty_generation_leaves_the_observed_set() {
        let quiet = ReadFacts::default();
        let sends = |ids: Vec<u32>| ReadFacts {
            transmits: true,
            ids,
            resets: false,
        };
        let mut t = Track::new(false);
        feed(&mut t, &quiet, &[(1, 10, false), (2, 11, false)]);
        feed(&mut t, &quiet, &[(2, 11, false)]);
        assert_eq!(t.order.evicted, [1]);
        assert_eq!(t.order.cases, [0, 0, 0], "no order within one read");

        feed(&mut t, &sends(vec![3]), &[(2, 11, false), (3, 12, false)]);
        assert_eq!(t.first(|h| h.0 == 3), Some(3));
        feed(&mut t, &sends(vec![]), &[(2, 20, false), (3, 12, false)]);
        assert_eq!(t.order.left(Left::Moved), 1);
        assert_eq!(t.first(|h| h.0 == 2), Some(4), "seen again, as new");
        feed(&mut t, &quiet, &[(2, 20, false), (3, 21, false)]);
        assert_eq!(t.first(|h| h.0 == 3), Some(3), "moved with nothing sent");
        feed(&mut t, &sends(vec![3]), &[(2, 20, false), (3, 30, false)]);
        assert_eq!(t.order.left(Left::Resent), 1);
        assert_eq!(t.first(|h| h.0 == 3), Some(6));
        assert!(t.order.departures.is_empty(), "{:?}", t.order);

        feed(
            &mut t,
            &quiet,
            &[(2, 20, false), (3, 30, false), (4, 40, false)],
        );
        feed(&mut t, &quiet, &[(2, 20, false), (3, 30, false)]);
        assert_eq!(t.order.evicted, [1, 7]);
        assert_eq!(t.order.cases(Rule::OlderFirst), 1);
        assert_eq!(t.order.cases(Rule::NewestSurvives), 1);
        let rules: Vec<Rule> = t.order.departures.iter().map(|d| d.rule).collect();
        assert_eq!(rules, [Rule::OlderFirst, Rule::NewestSurvives]);
        assert_eq!(t.order.departures[0].gone, 7);
    }

    /// Two arms' counters that differ in what each holds and evicted, and
    /// whose frames differ as a sum.
    fn quota_parted() -> (Counts, Counts) {
        let ours = Counts {
            stored: Some(3),
            stored_bytes: Some(12),
            held: Some(2),
            held_bytes: Some(8),
            frames: Some(2),
            evicted: Some(1),
            refused: Some(0),
        };
        let theirs = Counts {
            held: Some(1),
            held_bytes: Some(4),
            frames: Some(1),
            evicted: None,
            refused: None,
            ..ours
        };
        (ours, theirs)
    }

    /// An eviction check with cases and no departure, the recording
    /// deleting images itself or not.
    fn checked(deletes: bool) -> Eviction {
        let order = Order {
            seen: 3,
            cases: [0, 1, 1],
            evicted: vec![1],
            ..Order::default()
        };
        Eviction {
            deletes,
            arms: [order.clone(), order],
            frames: FrameCheck {
                compared: 1,
                ..FrameCheck::default()
            },
            store_evicted: 1,
        }
    }

    /// **A recording that sends `a=d` keeps today's verdict**: it deletes
    /// images itself, so no drop is an eviction and the counters decide as
    /// they do with no check. The same check on a recording that deletes
    /// nothing exempts what each arm holds and evicted, and compares the
    /// frames image by image.
    #[test]
    fn a_recording_with_a_d_keeps_todays_verdict() {
        let (ours, theirs) = quota_parted();
        let today = verdict(&ours, &theirs);
        assert_eq!(
            today,
            Verdict::Differ(vec![Counter::Held, Counter::HeldBytes, Counter::Frames])
        );
        let deletes = checked(true);
        assert!(!deletes.applies());
        assert_eq!(judged(&ours, &theirs, Some(&deletes)), today);
        assert_eq!(judged(&ours, &theirs, None), today);

        let applied = checked(false);
        assert!(applied.applies());
        assert_eq!(judged(&ours, &theirs, Some(&applied)), Verdict::Equal);
        let frames_differ = Eviction {
            frames: FrameCheck {
                compared: 1,
                differ: 1,
                ..FrameCheck::default()
            },
            ..checked(false)
        };
        assert_eq!(
            judged(&ours, &theirs, Some(&frames_differ)),
            Verdict::Differ(vec![Counter::Frames])
        );
        let control_fails = Eviction {
            store_evicted: 2,
            ..checked(false)
        };
        assert!(!control_fails.applies());
        assert_eq!(judged(&ours, &theirs, Some(&control_fails)), today);
        let mut departs = checked(false);
        departs.arms[1].departures.push(Departure {
            rule: Rule::OlderFirst,
            gone: 9,
            kept: Some(2),
        });
        assert_eq!(
            judged(&ours, &theirs, Some(&departs)),
            Verdict::Departs(vec![Rule::OlderFirst])
        );
    }

    /// Frames are compared image by image: by `i=` within one read, and
    /// the unnamed images of one read as a sorted multiset, which is not
    /// paired at all where the arms hold different numbers of them.
    #[test]
    fn frames_are_compared_per_image_and_never_paired_by_a_guess() {
        let f = |first: usize, named: Option<u32>, frames: usize| Framed {
            first,
            named,
            frames: Some(frames),
        };
        let ours = [
            f(1, Some(7), 3),
            f(2, None, 5),
            f(2, None, 1),
            f(4, None, 2),
        ];
        let theirs = [
            f(1, Some(7), 3),
            f(2, None, 1),
            f(2, None, 5),
            f(3, None, 9),
        ];
        let c = frames_per_image(&ours, &theirs);
        assert_eq!(
            c,
            FrameCheck {
                compared: 3,
                differ: 0,
                unread: 0,
                unmatched: 2,
            }
        );
        let other = [f(1, Some(7), 4), f(2, None, 1), f(2, None, 6)];
        let c = frames_per_image(&ours, &other);
        assert_eq!((c.compared, c.differ), (3, 2));
    }
}
