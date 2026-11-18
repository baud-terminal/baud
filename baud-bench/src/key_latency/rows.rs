//! Posts paired to records, and each key's legs.

use super::parse::{Count, Ns, Post, Record, Seq, Stamps};
use super::py;

/// A point a leg runs from or to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Point {
    /// The helper's stamp.
    Post,
    /// The event's own stamp.
    Event,
    /// See [`Stamps`].
    Handled,
    /// See [`Stamps`].
    Written,
    /// See [`Stamps`].
    Echo,
    /// See [`Stamps`].
    Fed,
    /// See [`Stamps`].
    Commit,
    /// See [`Stamps`].
    Landed,
    /// See [`Stamps`].
    Shown,
    /// See [`Stamps`].
    Vsync,
    /// See [`Stamps`].
    VsyncHi,
}

/// How many legs a key has.
pub const LEG_COUNT: usize = 14;

/// The legs, in report order, as (name, start, end).
pub const LEGS: [(&str, Point, Point); LEG_COUNT] = [
    ("stamp", Point::Post, Point::Event),
    ("deliver", Point::Post, Point::Handled),
    ("write", Point::Handled, Point::Written),
    ("child", Point::Written, Point::Echo),
    ("wake", Point::Echo, Point::Fed),
    ("build", Point::Fed, Point::Commit),
    ("gpu", Point::Commit, Point::Landed),
    ("show", Point::Landed, Point::Shown),
    // The app's own share: the responder's stamp to the surface on the
    // layer -- to_shown less the HID tap's delivery, which is the system's
    // and the noisiest leg under load.
    ("app", Point::Handled, Point::Shown),
    ("vsync", Point::Shown, Point::Vsync),
    ("vsync_hi", Point::Shown, Point::VsyncHi),
    ("to_shown", Point::Post, Point::Shown),
    ("total", Point::Post, Point::Vsync),
    ("total_hi", Point::Post, Point::VsyncHi),
];

/// One value per leg, in report order.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PerLeg<T>(pub [T; LEG_COUNT]);

impl<T> PerLeg<T> {
    /// The value of the leg `name`.
    pub fn get(&self, name: &str) -> &T {
        let at = LEGS
            .iter()
            .position(|(n, _, _)| *n == name)
            .expect("a leg's name");
        &self.0[at]
    }

    /// Each leg's name with its value.
    pub fn named(&self) -> impl Iterator<Item = (&'static str, &T)> {
        LEGS.iter().map(|(n, _, _)| *n).zip(self.0.iter())
    }
}

impl<T: Copy> PerLeg<Option<T>> {
    /// No leg has a value.
    pub fn none() -> Self {
        PerLeg([None; LEG_COUNT])
    }

    /// Only the leg `name` has a value.
    pub fn only(name: &str, value: T) -> Self {
        let mut out = Self::none();
        let at = LEGS
            .iter()
            .position(|(n, _, _)| *n == name)
            .expect("a leg's name");
        out.0[at] = Some(value);
        out
    }
}

/// Counts by name, in the order the names first came.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Tally(pub Vec<(String, Count)>);

impl Tally {
    /// Add `n` to `key`.
    pub fn add(&mut self, key: &str, n: Count) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some((_, have)) => *have += n,
            None => self.0.push((key.to_string(), n)),
        }
    }

    /// The count of `key`, 0 when it never came.
    pub fn get(&self, key: &str) -> Count {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map_or(Count(0), |(_, n)| *n)
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether it holds these counts, in any order.
    pub fn equals(&self, want: &[(&str, Count)]) -> bool {
        self.0.len() == want.len() && want.iter().all(|(k, n)| self.get(k) == *n)
    }

    /// `k=v, k=v`, by key.
    pub fn sorted_line(&self) -> String {
        let mut items: Vec<_> = self.0.iter().collect();
        items.sort_by(|a, b| a.0.cmp(&b.0));
        let items: Vec<String> = items.iter().map(|(k, v)| format!("{k}={v}")).collect();
        items.join(", ")
    }

    /// `repr` of it as a dict.
    pub fn repr_dict(&self) -> String {
        let items: Vec<String> = self
            .0
            .iter()
            .map(|(k, v)| format!("{}: {v}", py::repr_str(k)))
            .collect();
        format!("{{{}}}", items.join(", "))
    }

    /// `repr` of it as a `Counter`: by count, descending, ties as they came.
    pub fn repr_counter(&self) -> String {
        if self.0.is_empty() {
            return "Counter()".to_string();
        }
        let mut items = self.0.clone();
        items.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        format!("Counter({})", Tally(items).repr_dict())
    }
}

/// A run's counters.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Diag {
    /// The helper's count.
    pub posted: Count,
    /// Records the app printed.
    pub keylats: Count,
    /// Pairs made.
    pub paired: Count,
    /// Posts with no record.
    pub unpaired_posts: Count,
    /// Records with no post.
    pub unpaired_keylats: Count,
    /// Pairs whose `seq`s differ.
    pub seq_mismatch: Count,
    /// Pairs whose event precedes their post.
    pub event_before_post: Count,
    /// Pairs at or past the window.
    pub window: Count,
    /// Pairs with a negative leg.
    pub negative: Count,
    /// Records that did not parse, once the run has counted them.
    pub parse_failures: Option<Count>,
    /// `landing refused` lines, once the run has counted them.
    pub landing_refused: Option<Count>,
    /// Rows excluded for a refused landing, once they have been.
    pub refused_landing_rows: Option<Count>,
    /// The `refused` lines, by why.
    pub predict_refused: Tally,
}

impl Diag {
    /// `repr` of it as the dict the self-test prints.
    pub fn repr(&self) -> String {
        let mut items = vec![
            ("posted", self.posted),
            ("keylats", self.keylats),
            ("paired", self.paired),
            ("unpaired_posts", self.unpaired_posts),
            ("unpaired_keylats", self.unpaired_keylats),
            ("seq_mismatch", self.seq_mismatch),
            ("event_before_post", self.event_before_post),
            ("window", self.window),
            ("negative", self.negative),
        ];
        for (k, v) in [
            ("parse_failures", self.parse_failures),
            ("landing_refused", self.landing_refused),
            ("refused_landing_rows", self.refused_landing_rows),
        ] {
            if let Some(v) = v {
                items.push((k, v));
            }
        }
        let items: Vec<String> = items.iter().map(|(k, v)| format!("'{k}': {v}")).collect();
        format!("{{{}}}", items.join(", "))
    }
}

/// One paired key.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    /// The helper's sequence number.
    pub seq: Seq,
    /// The app's.
    pub app_seq: Seq,
    /// The letter.
    pub ch: String,
    /// The helper's stamp.
    pub post: Ns,
    /// The event's own stamp.
    pub event: Ns,
    /// Every later stage.
    pub stamps: Stamps,
    /// Why it is left out of every median, if it is.
    pub exclude_reason: Option<String>,
}

impl Row {
    /// Whether it is left out of every median.
    pub fn excluded(&self) -> bool {
        self.exclude_reason.is_some()
    }

    fn at(&self, p: Point) -> Option<Ns> {
        let s = &self.stamps;
        match p {
            Point::Post => Some(self.post),
            Point::Event => Some(self.event),
            Point::Handled => s.handled,
            Point::Written => s.written,
            Point::Echo => s.echo,
            Point::Fed => s.fed,
            Point::Commit => s.commit,
            Point::Landed => s.landed,
            Point::Shown => s.shown,
            Point::Vsync => s.vsync,
            Point::VsyncHi => s.vsync_hi,
        }
    }

    fn legs(&self) -> PerLeg<Option<f64>> {
        PerLeg(LEGS.map(|(_, a, b)| {
            let (a, b) = (self.at(a)?, self.at(b)?);
            Some((b.0 - a.0) as f64 / 1000.0)
        }))
    }
}

/// The legs after `stamp` that came out negative: a stamp that is not on
/// the clock the others are (the GPU end after the wake that reported it, a
/// re-stamped event). Such a row is excluded, never pooled.
pub fn negative_legs(row: &Row) -> Vec<&'static str> {
    row.legs()
        .named()
        .filter(|(name, v)| *name != "stamp" && v.is_some_and(|v| v < 0.0))
        .map(|(name, _)| name)
        .collect()
}

/// Posts paired to records BY ORDER: one key is one echo is one frame under
/// a cadence slower than a frame, with no per-frame identity to carry. A
/// pair is excluded, with the reason, when the app's `seq` is not the
/// helper's (`seq-mismatch`: a shifted or foreign pairing), `event < post`
/// (a stamp that cannot be this key's), `event - post >= window` (a lost or
/// mispaired line), or a leg after `stamp` is negative.
pub fn pair_by_order(posts: &[Post], keylats: &[Record], window: Ns) -> (Vec<Row>, Diag) {
    let mut rows = Vec::new();
    let mut diag = Diag {
        posted: Count(posts.len()),
        keylats: Count(keylats.len()),
        ..Diag::default()
    };
    for (post, rec) in posts.iter().zip(keylats) {
        let mut row = Row {
            seq: post.seq,
            app_seq: rec.seq,
            ch: post.ch.clone(),
            post: post.ns,
            event: rec.event,
            stamps: rec.stamps,
            exclude_reason: None,
        };
        let reason = if rec.seq != post.seq {
            diag.seq_mismatch += Count(1);
            Some("seq-mismatch".to_string())
        } else if rec.event < post.ns {
            diag.event_before_post += Count(1);
            Some("event<post".to_string())
        } else if rec.event.0 - post.ns.0 >= window.0 {
            diag.window += Count(1);
            Some("event-post>=window".to_string())
        } else {
            let neg = negative_legs(&row);
            if neg.is_empty() {
                None
            } else {
                diag.negative += Count(1);
                Some(format!("negative {}", neg.join(",")))
            }
        };
        row.exclude_reason = reason;
        rows.push(row);
    }
    let n = rows.len();
    diag.paired = Count(n);
    diag.unpaired_posts = Count(posts.len() - n);
    diag.unpaired_keylats = Count(keylats.len() - n);
    (rows, diag)
}

/// When the app printed `keylat: landing refused` in this run, exclude
/// every row that reached the GPU (`commit` set) and has no landing
/// (`landed=-`) as `refused-landing`: a refused present can stamp `shown` on
/// a record whose real frame has not presented, so its show, vsync and
/// to_shown are not this key's. Rows already excluded keep their reason; a
/// row with no `commit` never reached the GPU and is merely incomplete. A
/// run with no refusal line -- the CPU path, where every row has `landed=-`
/// -- is untouched. Returns how many rows it excluded.
pub fn exclude_refused_landings(rows: &mut [Row], refusals: Count) -> Count {
    if refusals == Count(0) {
        return Count(0);
    }
    let mut n = Count(0);
    for r in rows {
        if r.excluded() || r.stamps.commit.is_none() || r.stamps.landed.is_some() {
            continue;
        }
        r.exclude_reason = Some("refused-landing".to_string());
        n += Count(1);
    }
    n
}

/// The per-key legs in microseconds, `None` where an endpoint is missing or
/// the row is excluded.
pub fn legs_of(row: &Row) -> PerLeg<Option<f64>> {
    if row.excluded() {
        return PerLeg::none();
    }
    row.legs()
}

/// Paired (not excluded) and reached vsync.
pub fn is_completed(row: &Row) -> bool {
    !row.excluded() && row.stamps.vsync.is_some()
}

/// Two keys share a frame when their reconstructed absolute `commit`s lie
/// within this of each other. The offsets print at 0.1 us and every key
/// rebuilds its absolute from its own `event`, so two keys on ONE commit can
/// differ by up to 100 ns after the round trip; exact equality would miss
/// them. Distinct frames are milliseconds apart.
pub const SHARED_FRAME: Ns = Ns(200);

/// How many keys landed in a frame they share with another key: absolute
/// `commit`s within `tolerance` of the group's first. Under typeahead several
/// keys ride one frame; this is that count, so the reader knows how much the
/// medians pooled.
pub fn shared_frame_count(rows: &[Row], tolerance: Ns) -> Count {
    let mut commits: Vec<Ns> = rows
        .iter()
        .filter(|r| !r.excluded())
        .filter_map(|r| r.stamps.commit)
        .collect();
    commits.sort();
    let mut shared = 0;
    let mut group: Vec<Ns> = Vec::new();
    for c in commits {
        if group.first().is_some_and(|g| c.0 - g.0 > tolerance.0) {
            if group.len() > 1 {
                shared += group.len();
            }
            group.clear();
        }
        group.push(c);
    }
    if group.len() > 1 {
        shared += group.len();
    }
    Count(shared)
}
