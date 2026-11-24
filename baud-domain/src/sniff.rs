//! **Whether a read carries a link**, found in one pass over the bytes the
//! reads before it ended on and the read's own, before the read is fed: an
//! `OSC 8` introducer the parser opens a link at, or a plain `file://` URI
//! in the text, whose origin is the program that printed it.
//!
//! **The introducer is a state, not bytes.** `OSC` is `ESC ]`, or the C1
//! `9D` (`[[source: ctlseqs]]`), and the parser drops or executes some bytes
//! inside the introducer and goes on (`[[source: williams]]`'s `escape` and
//! `osc string` rows): after `ESC`, every C0 but `ESC`, `CAN` and `SUB`, and
//! `DEL`; in the string before its `;`, `00-06`, `08-17`, `19` and `1C-1F`.
//! [`LinkIntroducerAt`] steps over them as the parser does, from read to
//! read, so a run of them of any length adds nothing to what is carried.
//! A GR byte is read as its 7-bit byte there, as the parser reads it under
//! ISO 8859-1; under UTF-8 that and a `9D` can only find an introducer the
//! parser does not open, which asks once more and misses none.
//!
//! **The carried bytes are the longest pattern's less one**
//! ([`SNIFF_CARRY`]), so a pattern split across reads is whole in the read
//! that completes it. The window -- the carried bytes, then the read -- is
//! walked once, the introducer's state taken from where the carried bytes
//! begin, and the read is linked when a pattern ends anywhere in it.

use crate::{LANE_COUNT, Printable, ReadLinked, StreamByte, Vectorized, WalkSteps};

/// `ESC ] 8 ;`, the `OSC 8` introducer with nothing the parser drops inside
/// it: the shortest window [`LinkIntroducerAt`] finds one in.
const INTRODUCER: &[u8] = b"\x1b]8;";

/// `9D 8 ;`, its C1 form.
const C1_INTRODUCER: &[u8] = b"\x9d8;";

/// `file://`, in any case, whatever byte comes before it: a control
/// string's parameter, as `OSC 7`'s folder report is, asks as printed text
/// does.
const FILE_URI: &[u8] = b"file://";

/// The longest of `lens`.
const fn longest(lens: &[usize]) -> usize {
    let mut most = 0;
    let mut i = 0;
    while i < lens.len() {
        if lens[i] > most {
            most = lens[i];
        }
        i += 1;
    }
    most
}

/// **The longest pattern the sniff looks for**: `file://`.
pub const SNIFF_LONGEST: usize = longest(&[INTRODUCER.len(), C1_INTRODUCER.len(), FILE_URI.len()]);

/// **The bytes a read's sniff carries to the next**: one fewer than
/// [`SNIFF_LONGEST`], so a pattern that ends in a read began no earlier than
/// the bytes carried into it.
pub const SNIFF_CARRY: usize = SNIFF_LONGEST - 1;

/// The sniff a reader keeps: carrying [`SNIFF_CARRY`] bytes.
pub type LinkSniff = ReadSniff<SNIFF_CARRY>;

/// **How far into an `OSC 8` introducer the stream stands.** Each byte
/// steps it as the parser steps through the same bytes: `ESC` goes to
/// [`LinkIntroducerAt::AfterEsc`] from any state and `9D` to
/// [`LinkIntroducerAt::InOsc`]; `CAN` and `SUB` cancel to
/// [`LinkIntroducerAt::Outside`]; after `ESC` a C0 or `DEL` leaves it where
/// it is; in the string a `BEL` ends it, a C0 the parser drops leaves it
/// where it is, and `;` after the `8` is a link. Made only by
/// [`ReadSniff::read`], whose field is private.
///
/// ```compile_fail,E0616
/// fn f(s: &mut baud_domain::LinkSniff) {
///     s.intro = baud_domain::LinkIntroducerAt::AfterEight;
/// }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{LinkSniff, ReadLinked, WalkSteps};
/// let mut s = LinkSniff::new();
/// let mut steps = WalkSteps::ZERO;
/// assert_eq!(s.read(b"\x1b]8\x01", &mut steps).linked, ReadLinked::No);
/// assert_eq!(s.read(b";;file:///x\x07", &mut steps).linked, ReadLinked::Yes);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkIntroducerAt {
    Outside,
    AfterEsc,
    /// After `ESC ]` or `9D`.
    InOsc,
    /// After the `8`.
    AfterEight,
}

impl LinkIntroducerAt {
    /// **The state after `b`**, and whether `b` completes an introducer.
    const fn step(self, b: u8) -> (LinkIntroducerAt, ReadLinked) {
        // A GR byte, as the parser reads one under ISO 8859-1.
        let g = match b {
            0xa0..=0xff => b - 0x80,
            _ => b,
        };
        match b {
            0x1b => return (LinkIntroducerAt::AfterEsc, ReadLinked::No),
            0x9d => return (LinkIntroducerAt::InOsc, ReadLinked::No),
            0x18 | 0x1a => return (LinkIntroducerAt::Outside, ReadLinked::No),
            _ => {}
        }
        let next = match self {
            LinkIntroducerAt::Outside => LinkIntroducerAt::Outside,
            LinkIntroducerAt::AfterEsc => match g {
                b']' => LinkIntroducerAt::InOsc,
                0x00..=0x1f | 0x7f => LinkIntroducerAt::AfterEsc,
                _ => LinkIntroducerAt::Outside,
            },
            LinkIntroducerAt::InOsc => match (b, g) {
                (0x07, _) => LinkIntroducerAt::Outside,
                (_, b'8') => LinkIntroducerAt::AfterEight,
                (0x00..=0x1f, _) => LinkIntroducerAt::InOsc,
                _ => LinkIntroducerAt::Outside,
            },
            LinkIntroducerAt::AfterEight => match (b, g) {
                (0x07, _) => LinkIntroducerAt::Outside,
                (_, b';') => return (LinkIntroducerAt::Outside, ReadLinked::Yes),
                (0x00..=0x1f, _) => LinkIntroducerAt::AfterEight,
                _ => LinkIntroducerAt::Outside,
            },
        };
        (next, ReadLinked::No)
    }
}

/// **What one read's sniff found.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sniffed {
    /// Whether an `OSC 8` introducer or a `file://` URI ends in the read or
    /// in the bytes carried into it.
    pub linked: ReadLinked,
}

/// **The sniff of a stream's reads**, carrying `CARRY` bytes from each read
/// to the next and the introducer's state where they begin. `CARRY` is one
/// fewer than [`SNIFF_LONGEST`], checked when the sniff is made, so it
/// cannot be set by hand to a length the patterns outgrow: a mismatch fails
/// the build where `new` is called.
///
/// ```compile_fail,E0080
/// let _ = baud_domain::ReadSniff::<7>::new();
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{LinkSniff, ReadLinked, WalkSteps};
/// let mut s = LinkSniff::new();
/// let mut steps = WalkSteps::ZERO;
/// assert_eq!(s.read(b"x \x1b]8;;", &mut steps).linked, ReadLinked::Yes);
/// assert_eq!(steps.get(), 7);
/// ```
#[derive(Clone)]
pub struct ReadSniff<const CARRY: usize> {
    /// The stream's last bytes, at most `CARRY`, the latest last.
    tail: [Option<StreamByte>; CARRY],
    /// The introducer's state before the first of `tail`.
    intro: LinkIntroducerAt,
}

/// The carried bytes escaped, and the introducer's state.
impl<const CARRY: usize> core::fmt::Debug for ReadSniff<CARRY> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ReadSniff { tail: \"")?;
        for b in self.tail.iter().flatten() {
            write!(f, "{}", core::ascii::escape_default(b.raw_byte()))?;
        }
        write!(f, "\", intro: {:?} }}", self.intro)
    }
}

impl<const CARRY: usize> Default for ReadSniff<CARRY> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CARRY: usize> ReadSniff<CARRY> {
    /// **A sniff at the stream's start**: nothing carried, outside any
    /// introducer.
    pub const fn new() -> Self {
        const {
            assert!(
                CARRY + 1 == SNIFF_LONGEST,
                "the carry is the longest pattern less one"
            )
        };
        ReadSniff {
            tail: [None; CARRY],
            intro: LinkIntroducerAt::Outside,
        }
    }

    /// **Whether `bytes`, read after the stream so far, carry a link**: one
    /// pass over the carried bytes and `bytes`, each counted once in
    /// `steps`, so a read costs at most its length and `CARRY`. Outside an
    /// introducer, sixteen bytes a step: one compare finds the lanes holding
    /// `ESC`, `9D` or `:`, and a block holding none is passed whole.
    pub fn read(&mut self, bytes: &[u8], steps: &mut WalkSteps) -> Sniffed {
        let mut carried = [0u8; CARRY];
        let mut held = 0;
        for b in self.tail.iter().flatten() {
            carried[held] = b.raw_byte();
            held += 1;
        }
        let carried = &carried[..held];
        let window = Window { carried, bytes };
        let end = held + bytes.len();
        // The first byte the next read's window carries.
        let keep = end.saturating_sub(CARRY);
        let stream = StreamByte::slice_from(bytes);
        let mut state = self.intro;
        let mut kept = state;
        let mut linked = ReadLinked::No;
        let mut i = 0;
        while i < end {
            if state == LinkIntroducerAt::Outside && i >= held && i + LANE_COUNT <= end {
                let lanes = Vectorized::<Printable>::load(&stream[i - held..]).any_of_three(
                    StreamByte(0x1b),
                    StreamByte(0x9d),
                    StreamByte(b':'),
                );
                let skip = lanes.first().map_or(LANE_COUNT, |l| l.index());
                // No byte passed moves the state from `Outside`.
                if (i..i + skip).contains(&keep) {
                    kept = state;
                }
                steps.over(skip);
                i += skip;
                if skip == LANE_COUNT {
                    continue;
                }
            }
            if i == keep {
                kept = state;
            }
            let b = window.at(i);
            let (next, closed) = state.step(b);
            state = next;
            if closed == ReadLinked::Yes || (b == b':' && window.file_uri_at(i)) {
                linked = ReadLinked::Yes;
            }
            steps.bump();
            i += 1;
        }
        self.tail = [None; CARRY];
        let kept_len = end - keep;
        for (k, slot) in self.tail[CARRY - kept_len..].iter_mut().enumerate() {
            *slot = Some(StreamByte(window.at(keep + k)));
        }
        self.intro = kept;
        Sniffed { linked }
    }
}

/// The carried bytes, then the read: one run of indices over both.
struct Window<'a> {
    carried: &'a [u8],
    bytes: &'a [u8],
}

impl Window<'_> {
    fn len(&self) -> usize {
        self.carried.len() + self.bytes.len()
    }

    fn at(&self, i: usize) -> u8 {
        match i.checked_sub(self.carried.len()) {
            Some(r) => self.bytes[r],
            None => self.carried[i],
        }
    }

    /// **Whether the `:` at `i` is a `file://` URI's**: `file` in any case
    /// before it and `//` after it, whatever byte comes before the `file`.
    /// So `Makefile:12:` and `profile:` are not one, and `OSC 7`'s folder
    /// is.
    fn file_uri_at(&self, i: usize) -> bool {
        let Some(name) = i.checked_sub(4) else {
            return false;
        };
        if i + 2 >= self.len() || self.at(i + 1) != b'/' || self.at(i + 2) != b'/' {
            return false;
        }
        (0..4).all(|k| self.at(name + k).eq_ignore_ascii_case(&b"file"[k]))
    }
}
