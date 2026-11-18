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
//! as. The first five counters are compared ([`Counter::COMPARED`]);
//! evicted and refused are baud-core's own and only printed. A compared
//! counter either arm reports as "n/a" proves nothing equal: the recording
//! is [`Verdict::Unknown`], printed UNEQUAL-UNKNOWN, its rate unquotable.

// Without libghostty-vt nothing is counted on the second arm.
#![cfg_attr(not(have_ghostty), allow(dead_code))]

/// Bytes a pixel is counted at, on both arms.
const PIXEL_BYTES: usize = 4;

/// One image an arm holds: its client id, as that arm numbers it, and its
/// size in pixels.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct Held {
    pub(crate) id: u32,
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

/// The compared counters both arms report and that differ.
pub(crate) fn differing(a: &Counts, b: &Counts) -> Vec<Counter> {
    Counter::COMPARED
        .into_iter()
        .filter(|&c| matches!((a.get(c), b.get(c)), (Some(x), Some(y)) if x != y))
        .collect()
}

/// The compared counters at least one arm reports as "n/a".
pub(crate) fn unreported(a: &Counts, b: &Counts) -> Vec<Counter> {
    Counter::COMPARED
        .into_iter()
        .filter(|&c| a.get(c).is_none() || b.get(c).is_none())
        .collect()
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
}

/// The verdict on `a` against `b`. A counter that differs outranks one
/// that is "n/a".
pub(crate) fn verdict(a: &Counts, b: &Counts) -> Verdict {
    let differ = differing(a, b);
    let missing = unreported(a, b);
    if !differ.is_empty() {
        Verdict::Differ(differ)
    } else if !missing.is_empty() {
        Verdict::Unknown(missing)
    } else {
        Verdict::Equal
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
        Held { id, width, height }
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

    /// Evicted and refused are baud-core's own: one arm reporting them
    /// and the other not leaves the verdict to the five compared counters.
    #[test]
    fn evicted_and_refused_are_not_compared() {
        let mut a = Tally::default();
        a.observe(vec![held(1, 1, 1)]);
        let ours = a.counts(Some(1), Some(3), Some(4));
        let theirs = a.counts(Some(1), None, None);
        assert_eq!(verdict(&ours, &theirs), Verdict::Equal);
        let theirs = a.counts(Some(2), None, None);
        assert_eq!(
            verdict(&ours, &theirs),
            Verdict::Differ(vec![Counter::Frames])
        );
        assert_eq!(
            theirs.to_string(),
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
}
