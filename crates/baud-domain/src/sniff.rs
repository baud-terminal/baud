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
//!
//! **The same pass finds the shell's `D`**, where a read is cut: an `OSC
//! 133 ; D` whose first `baud_token=` option with a value is the window's
//! token ([`ShellTokenHex`]), in a field that ends inside the first 512
//! bytes of the body when the body is longer. A `D` is found from its
//! `ESC`, which the pass stops at anyway, and its body is walked a byte at
//! a time, once, as the rest of the read is; where it stands is carried to
//! the next read. The read is cut at the first such `D` it ends, when that
//! one began in it; one begun in an earlier read places no cut there. The
//! token is matched as a sixteen-byte pattern at its last digit, so the
//! carried bytes hold the rest of one split across reads.

use crate::{
    BytePos, LANE_COUNT, Printable, ReadLen, ReadLinked, ShellTokenHex, StreamByte, Vectorized,
    WalkSteps,
};

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

/// **The longest pattern the sniff looks for**: the window's token,
/// sixteen digits.
pub const SNIFF_LONGEST: usize = longest(&[
    INTRODUCER.len(),
    C1_INTRODUCER.len(),
    FILE_URI.len(),
    ShellTokenHex::LEN,
]);

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
/// let mut s = LinkSniff::new(None);
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
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Sniffed {
    /// Whether an `OSC 8` introducer or a `file://` URI ends in the read or
    /// in the bytes carried into it.
    pub linked: ReadLinked,
    /// Where the read is cut: at the `ESC` of the first `D` with the
    /// window's token that ends in the read, when that `D` also began in
    /// it.
    pub token_at: Option<BytePos>,
}

/// The cut shown as its offset.
impl core::fmt::Debug for Sniffed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Sniffed")
            .field("linked", &self.linked)
            .field("token_at", &self.token_at.map(BytePos::index))
            .finish()
    }
}

/// **The sniff of a stream's reads**, carrying `CARRY` bytes from each read
/// to the next and the introducer's state where they begin, and the
/// window's token, if baud made one. `CARRY` is one fewer than
/// [`SNIFF_LONGEST`], checked when the sniff is made, so it cannot be set by
/// hand to a length the patterns outgrow: a mismatch fails the build where
/// `new` is called.
///
/// ```compile_fail,E0080
/// let _ = baud_domain::ReadSniff::<6>::new(None);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{BytePos, LinkSniff, ReadLinked, ShellTokenHex, WalkSteps};
/// let token = ShellTokenHex::parse(b"0123456789abcdef");
/// let mut s = LinkSniff::new(token);
/// let mut steps = WalkSteps::ZERO;
/// assert_eq!(s.read(b"x \x1b]8;;", &mut steps).linked, ReadLinked::Yes);
/// assert_eq!(steps.get(), 7);
/// let d = b"$ \x1b]133;D;0;baud_token=0123456789abcdef\x07% ";
/// assert_eq!(s.read(d, &mut steps).token_at.map(BytePos::index), Some(2));
/// ```
#[derive(Clone)]
pub struct ReadSniff<const CARRY: usize> {
    /// The stream's last bytes, at most `CARRY`, the latest last.
    tail: [Option<StreamByte>; CARRY],
    /// The introducer's state before the first of `tail`.
    intro: LinkIntroducerAt,
    /// The window's token, which the shell's `D` carries.
    token: Option<ShellTokenHex>,
    /// Where the scan for the shell's `D` stands at the stream's end.
    mark: MarkAt,
}

/// The carried bytes escaped, the introducer's state, and whether a token
/// is held, not its digits.
impl<const CARRY: usize> core::fmt::Debug for ReadSniff<CARRY> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ReadSniff { tail: \"")?;
        for b in self.tail.iter().flatten() {
            write!(f, "{}", core::ascii::escape_default(b.raw_byte()))?;
        }
        write!(f, "\", intro: {:?}, token: {:?} }}", self.intro, self.token)
    }
}

impl<const CARRY: usize> Default for ReadSniff<CARRY> {
    fn default() -> Self {
        Self::new(None)
    }
}

impl<const CARRY: usize> ReadSniff<CARRY> {
    /// **A sniff at the stream's start** for a window whose shell baud
    /// handed `token`: nothing carried, outside any introducer.
    pub const fn new(token: Option<ShellTokenHex>) -> Self {
        const {
            assert!(
                CARRY + 1 == SNIFF_LONGEST,
                "the carry is the longest pattern less one"
            )
        };
        ReadSniff {
            tail: [None; CARRY],
            intro: LinkIntroducerAt::Outside,
            token,
            mark: MarkAt::Idle,
        }
    }

    /// **Whether `bytes`, read after the stream so far, carry a link, and
    /// where the shell's `D` begins in them**: one pass over the carried
    /// bytes and `bytes`, each counted once in `steps`, so a read costs at
    /// most its length and `CARRY`. Outside an introducer and a `D`'s body,
    /// sixteen bytes a step: one compare finds the lanes holding `ESC`, `9D`
    /// or `:`, and a block holding none is passed whole.
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
        let mut mark = self.mark.carried();
        let mut first = FirstD::Unseen;
        let mut i = 0;
        while i < end {
            let passable = state == LinkIntroducerAt::Outside && mark == MarkAt::Idle;
            if passable && i >= held && i + LANE_COUNT <= end {
                let lanes = Vectorized::<Printable>::load(&stream[i - held..]).any_of_three(
                    StreamByte(0x1b),
                    StreamByte(0x9d),
                    StreamByte(b':'),
                );
                let skip = lanes.first().map_or(LANE_COUNT, |l| l.index());
                // No byte passed moves the state from `Outside` or opens a
                // mark.
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
            if let (Some(token), Some(j)) = (&self.token, i.checked_sub(held)) {
                if let Some(esc) = mark.step(&window, i, j, token) {
                    first = first.met(esc);
                }
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
        self.mark = mark;
        Sniffed {
            linked,
            token_at: first.cut(),
        }
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

/// `ESC ] 133 ; D ;`: the start of a `D` mark carrying options, which the
/// window's token is one of.
const D_MARK: &[u8] = b"\x1b]133;D;";

/// `;baud_token=`: the option the window's shell's integration sends its
/// token in, a field of its own.
const TOKEN_OPTION: &[u8] = b";baud_token=";

/// The most of a mark's body the marks are read from: a token in a field
/// that does not end inside it is not read.
const MARK_BODY: usize = 512;

/// **Where the scan for the shell's `D` stands**, carried from read to
/// read: outside one; part of the way through `ESC ] 133 ; D ;`; in the
/// body of such a mark; or after an `ESC` in that body, where `\` ends it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MarkAt {
    Idle,
    /// The `ESC` and how many bytes of [`D_MARK`] are matched.
    Opening(EscAt, ReadLen),
    Body(OpenMark),
    BodyEsc(OpenMark),
}

/// **Where a mark's `ESC` is**: in this read, or in one before it, which
/// places no cut.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EscAt {
    Here(BytePos),
    Earlier,
}

/// The offset only.
impl core::fmt::Debug for EscAt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EscAt::Here(p) => write!(f, "Here({})", p.index()),
            EscAt::Earlier => f.write_str("Earlier"),
        }
    }
}

/// A `D` mark open: its `ESC`, the bytes of its body so far, and its first
/// `baud_token=` option with a value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct OpenMark {
    esc: EscAt,
    body: ReadLen,
    option: TokenOption,
}

/// **The first `baud_token=` option with a value in a `D`'s body.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TokenOption {
    /// How many bytes of [`TOKEN_OPTION`] are matched.
    Unseen(ReadLen),
    /// How many bytes of its value are read, fewer than the token's.
    Value(ReadLen),
    /// The window's token, its sixteen digits, not yet ended.
    Token,
    /// The window's token, ended by the `;` at this byte of the body.
    Shells(ReadLen),
    /// Any other value.
    Other,
}

impl TokenOption {
    /// After the body's next byte `b`, the window's byte `i`.
    fn step(
        self,
        b: u8,
        i: usize,
        window: &Window<'_>,
        token: &ShellTokenHex,
        at: ReadLen,
    ) -> Self {
        match self {
            TokenOption::Unseen(k) => match (b == TOKEN_OPTION[k.index()], b) {
                (true, _) if k.index() + 1 == TOKEN_OPTION.len() => {
                    TokenOption::Value(ReadLen::ZERO)
                }
                (true, _) => TokenOption::Unseen(k.plus(ReadLen::ONE)),
                (false, b';') => TokenOption::Unseen(ReadLen::ONE),
                (false, _) => TokenOption::Unseen(ReadLen::ZERO),
            },
            // An empty value is passed over, as no value; its `;` begins
            // the next field.
            TokenOption::Value(n) if b == b';' && n.is_zero() => TokenOption::Unseen(ReadLen::ONE),
            TokenOption::Value(_) if b == b';' => TokenOption::Other,
            TokenOption::Value(n) if n.index() + 1 < ShellTokenHex::LEN => {
                TokenOption::Value(n.plus(ReadLen::ONE))
            }
            // The value's sixteenth byte: the token, matched as a pattern
            // over the window, the carried bytes holding the fifteen before
            // it when the value began in an earlier read.
            TokenOption::Value(_) => match token_ends_at(token, window, i) {
                true => TokenOption::Token,
                false => TokenOption::Other,
            },
            TokenOption::Token if b == b';' => TokenOption::Shells(at),
            TokenOption::Token => TokenOption::Other,
            TokenOption::Shells(_) | TokenOption::Other => self,
        }
    }
}

impl OpenMark {
    /// **Whether the mark, ended with `body` bytes in its body, is the
    /// shell's own `D`**: its first `baud_token=` option with a value is
    /// the window's token, in a field that ends inside the most of the body
    /// the marks are read from.
    fn is_the_shells(self) -> bool {
        let whole = self.body.index() < MARK_BODY;
        match self.option {
            TokenOption::Shells(semi) => whole || semi.index() < MARK_BODY,
            TokenOption::Token => whole,
            TokenOption::Unseen(_) | TokenOption::Value(_) | TokenOption::Other => false,
        }
    }
}

/// **Which `D` of the shell's a read met first**: none yet, one begun in
/// the read, or one begun in a read before it, which places no cut.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstD {
    Unseen,
    At(BytePos),
    Earlier,
}

impl FirstD {
    /// After the shell's `D` begun at `esc` ends.
    fn met(self, esc: EscAt) -> FirstD {
        match (self, esc) {
            (FirstD::Unseen, EscAt::Here(p)) => FirstD::At(p),
            (FirstD::Unseen, EscAt::Earlier) => FirstD::Earlier,
            (met, _) => met,
        }
    }

    /// Where the read is cut.
    fn cut(self) -> Option<BytePos> {
        match self {
            FirstD::At(p) => Some(p),
            FirstD::Unseen | FirstD::Earlier => None,
        }
    }
}

impl MarkAt {
    /// **The state a new read starts in**: a mark's `ESC` is in a read
    /// before it.
    fn carried(self) -> MarkAt {
        let earlier = |m: OpenMark| OpenMark {
            esc: EscAt::Earlier,
            ..m
        };
        match self {
            MarkAt::Idle => MarkAt::Idle,
            MarkAt::Opening(_, k) => MarkAt::Opening(EscAt::Earlier, k),
            MarkAt::Body(m) => MarkAt::Body(earlier(m)),
            MarkAt::BodyEsc(m) => MarkAt::BodyEsc(earlier(m)),
        }
    }

    /// **After the read's byte `j`, the window's byte `i`**, and the `ESC`
    /// of the shell's `D` it ends, if it ends one. The bytes are the ones
    /// the scan of the marks reads them as: `ESC ]` begins a body wherever
    /// it stands, `BEL` or `ESC \` ends one, `CAN` or `SUB` or an `ESC`
    /// before any other byte abandons it, and every other byte is the
    /// body's.
    fn step(
        &mut self,
        window: &Window<'_>,
        i: usize,
        j: usize,
        token: &ShellTokenHex,
    ) -> Option<EscAt> {
        let b = window.at(i);
        let esc_before = match j.checked_sub(1) {
            Some(p) => EscAt::Here(BytePos::from_index(p)),
            None => EscAt::Earlier,
        };
        let (at, k) = match *self {
            MarkAt::Idle => (EscAt::Earlier, ReadLen::ZERO),
            MarkAt::Opening(at, k) => (at, k),
            MarkAt::Body(mut m) => {
                *self = match b {
                    0x07 => {
                        *self = MarkAt::Idle;
                        return m.is_the_shells().then_some(m.esc);
                    }
                    0x1b => MarkAt::BodyEsc(m),
                    0x18 | 0x1a => MarkAt::Idle,
                    _ => {
                        m.option = m.option.step(b, i, window, token, m.body);
                        m.body = m.body.plus(ReadLen::ONE);
                        MarkAt::Body(m)
                    }
                };
                return None;
            }
            MarkAt::BodyEsc(m) => {
                if b == b'\\' {
                    *self = MarkAt::Idle;
                    return m.is_the_shells().then_some(m.esc);
                }
                // The `ESC` ended the body and begins a sequence of its own.
                (esc_before, ReadLen::ONE)
            }
        };
        *self = match (k.is_zero(), b == D_MARK[k.index()]) {
            (false, true) if k.index() + 1 == D_MARK.len() => MarkAt::Body(OpenMark {
                esc: at,
                body: ReadLen::from_index(D_MARK.len() - 2),
                // The `;` after the letter begins the first field.
                option: TokenOption::Unseen(ReadLen::ONE),
            }),
            (false, true) => MarkAt::Opening(at, k.plus(ReadLen::ONE)),
            _ if b == 0x1b => MarkAt::Opening(EscAt::Here(BytePos::from_index(j)), ReadLen::ONE),
            _ => MarkAt::Idle,
        };
        None
    }
}

/// **Whether the window's sixteen bytes ending at `i` are `token`'s
/// digits**: the token as a pattern, whose fifteen bytes before its last
/// the carried bytes hold when it began in an earlier read.
fn token_ends_at(token: &ShellTokenHex, window: &Window<'_>, i: usize) -> bool {
    let Some(first) = (i + 1).checked_sub(ShellTokenHex::LEN) else {
        return false;
    };
    let mut digits = [0u8; ShellTokenHex::LEN];
    for (k, d) in digits.iter_mut().enumerate() {
        *d = window.at(first + k);
    }
    token.is(&digits)
}
