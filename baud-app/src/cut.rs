//! **A read is cut at the shell's `D`**, so the prompt after a remote
//! command is the shell's even when the read that carries it began with
//! the command's last bytes. A read is filed under the most cautious verdict
//! of what wrote it (`ran::Ran::or`), and rows keep the most cautious fact
//! filed over them, so a read holding `ssh`'s last line, the shell's `D`
//! and the shell's next prompt left that prompt dark for good.
//!
//! The bytes before the `D` keep the read's verdict; the `D` and what
//! follows are the shell's (`ran::Commands`, which reads them on their
//! own). The cut is taken only where the bytes before the `D` provably
//! write nothing but on the rows the cursor passes as it goes: the read
//! begins at ground ([`Stream`]), under UTF-8, and those bytes hold only
//! printable UTF-8, `CR`, `LF`, `HT` and `SGR` spelled exactly
//! `CSI [0-9;:]* m` ([`Forward`]). Anything else -- another escape, any
//! other `CSI`, a control string, a C1 code, `DEL`, a malformed or split
//! character, a read begun inside a sequence -- makes no cut, and the read
//! is filed whole, as before. The `D` must be the shell's, with its token,
//! and begin in this read; one begun in an earlier read makes no cut.
//!
//! **zsh's `PROMPT_SP` run comes before the `D`**: zsh prints its default
//! `%B%S%#%s%b` mark, the columns less one of blanks, `CR`, a blank and
//! `CR`, before its `precmd` hooks run, where the integration sends the
//! `D`. Its job is to put the prompt on a clean row: text left on the row
//! wraps above it. So when the bytes after the last `LF` are exactly that
//! run, as zsh writes it by default, the cut may move to the run's start
//! ([`Eol`]): only when the blanks are exactly the terminal's columns less
//! one and the cursor is at the first column as the run begins, both read
//! from the terminal there (`reads::paced`). Any byte before the run, on
//! its row, keeps the cut at the `D`, and a mark the user set fails closed.
//!
//! **This allowlist goes when the core records which command wrote each
//! row**; until then it is what the cut is judged by.

use baud_core::{BytePos, ReadLen, ScreenEpoch, StreamByte, Term};
use baud_domain::{Col, Cols, Row, RowRot, Rows};
use baud_render::AbsLine;

use crate::ran::Ran;
use crate::reach::Reach;

/// Whether the stream stands at ground between two reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grounded {
    Yes,
    No,
}

/// Where the stream is, as far as [`Stream`] follows it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum At {
    #[default]
    Ground,
    /// Inside a UTF-8 character, with continuation bytes to come.
    Utf8(Owed),
    Esc,
    /// After `ESC` and an intermediate.
    EscInter,
    /// After `ESC %`: the byte that selects an encoding.
    Percent,
    Csi(Param),
    /// Inside an `OSC`, which a `BEL` ends as well as `ST`.
    Osc,
    /// Inside a `DCS`, `SOS`, `PM` or `APC`.
    Str,
    /// After an `ESC` inside a control string.
    StrEsc,
    /// Under `ESC % @`, where 0x80 to 0x9F are controls: not followed but
    /// for the way back, `ESC % G`.
    Latin1(Back),
    /// In VT52, after `CSI ? 2 l`, until `ESC <`.
    Vt52(Vt52),
}

/// The continuation bytes a character still owes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owed {
    One,
    Two,
    Three,
}

impl Owed {
    fn less(self) -> Option<Owed> {
        match self {
            Owed::One => None,
            Owed::Two => Some(Owed::One),
            Owed::Three => Some(Owed::Two),
        }
    }
}

/// How far `ESC % G` has come under `ESC % @`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Back {
    Nothing,
    Esc,
    Percent,
}

/// Where VT52's parser is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Vt52 {
    Ground,
    Esc,
    /// After `ESC Y`, its row and then its column to come.
    Row,
    Col,
}

/// **A `CSI` so far, as far as entering VT52 goes**: whether it began with
/// `?`, whether a parameter so far was exactly `2`, and the parameter now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Param {
    private: Private,
    two: Two,
    now: Now,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Private {
    Unknown,
    Yes,
    No,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Two {
    No,
    Yes,
}

/// The parameter being read: nothing yet, exactly `2`, or anything else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Now {
    Empty,
    Two,
    Other,
}

impl Param {
    const START: Param = Param {
        private: Private::Unknown,
        two: Two::No,
        now: Now::Empty,
    };

    /// Whether the parameter now, ended, was exactly `2`.
    fn ended(self) -> Two {
        match (self.two, self.now) {
            (Two::Yes, _) | (_, Now::Two) => Two::Yes,
            _ => Two::No,
        }
    }
}

/// **The stream's state between reads, followed on the thread taking
/// them**: whether a read begins at ground, which the cut needs and no byte
/// of the read can show. It follows the stream as `[[source: williams]]`'s
/// parser does, a `BEL` ending an `OSC` as `[[source: ctlseqs]]` says, and
/// where it does not follow -- under `ESC % @` -- it says not at ground
/// until `ESC % G`, which fails closed. `CSI ? 2 l` enters VT52, which it
/// follows to `ESC <`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stream {
    at: At,
}

impl Stream {
    /// Whether the stream stands at ground now, between reads.
    pub fn grounded(&self) -> Grounded {
        match self.at {
            At::Ground => Grounded::Yes,
            _ => Grounded::No,
        }
    }

    /// The stream after `bytes`.
    pub fn read(&mut self, bytes: &[u8]) {
        for &b in StreamByte::slice_from(bytes) {
            self.at = step(self.at, b);
        }
    }
}

/// The state after `b` from `at`.
fn step(at: At, s: StreamByte) -> At {
    let b = s.raw_byte();
    match at {
        // Only a direct `@` or `G` is read as the core reads it; after
        // anything else the stream is taken as under `ESC % @` until
        // `ESC % G`, which fails closed.
        At::Percent => {
            return match b {
                b'@' => At::Latin1(Back::Nothing),
                b'G' => At::Ground,
                0x1b => At::Latin1(Back::Esc),
                _ => At::Latin1(Back::Nothing),
            };
        }
        At::Latin1(back) => {
            return match (back, b) {
                (_, 0x1b) => At::Latin1(Back::Esc),
                (Back::Esc, b'%') => At::Latin1(Back::Percent),
                (Back::Percent, b'G') => At::Ground,
                _ => At::Latin1(Back::Nothing),
            };
        }
        At::Vt52(v) => {
            return match (v, b) {
                (Vt52::Row, _) => At::Vt52(Vt52::Col),
                (Vt52::Col, _) => At::Vt52(Vt52::Ground),
                (_, 0x1b) => At::Vt52(Vt52::Esc),
                (Vt52::Esc, b'<') => At::Ground,
                (Vt52::Esc, b'Y') => At::Vt52(Vt52::Row),
                _ => At::Vt52(Vt52::Ground),
            };
        }
        _ => {}
    }
    match (at, b) {
        (_, 0x18 | 0x1a) => return At::Ground,
        (At::Osc | At::Str, 0x1b) => return At::StrEsc,
        (At::StrEsc, b'\\') => return At::Ground,
        (At::StrEsc, _) => return step(At::Esc, s),
        (_, 0x1b) => return At::Esc,
        _ => {}
    }
    match at {
        At::Ground => match b {
            0xc2..=0xdf => At::Utf8(Owed::One),
            0xe0..=0xef => At::Utf8(Owed::Two),
            0xf0..=0xf4 => At::Utf8(Owed::Three),
            _ => At::Ground,
        },
        At::Utf8(owed) => match b {
            0x80..=0xbf => owed.less().map_or(At::Ground, At::Utf8),
            // A character cut short: the byte is read afresh.
            _ => step(At::Ground, s),
        },
        At::Esc => match b {
            b'[' => At::Csi(Param::START),
            b']' => At::Osc,
            b'P' | b'X' | b'^' | b'_' => At::Str,
            b'%' => At::Percent,
            0x20..=0x2f => At::EscInter,
            // A control is carried out inside the sequence and `DEL`
            // ignored, as the core's `escape` row does.
            0x00..=0x1f | 0x7f => At::Esc,
            // Under UTF-8 a byte from 0x80 ends the sequence and is the
            // ground's.
            0x80..=0xff => step(At::Ground, s),
            _ => At::Ground,
        },
        At::EscInter => match b {
            0x30..=0x7e => At::Ground,
            // A byte from 0x80 ends the sequence and is the ground's.
            0x80..=0xff => step(At::Ground, s),
            _ => At::EscInter,
        },
        At::Csi(p) => csi(p, s),
        At::Osc => match b {
            0x07 => At::Ground,
            _ => At::Osc,
        },
        At::Str | At::StrEsc => At::Str,
        At::Percent | At::Latin1(_) | At::Vt52(_) => at,
    }
}

/// The state after `b` inside a `CSI`.
fn csi(p: Param, s: StreamByte) -> At {
    let b = s.raw_byte();
    let private = match (p.private, b) {
        (Private::Unknown, b'?') => {
            return At::Csi(Param {
                private: Private::Yes,
                ..p
            });
        }
        (Private::Unknown, _) => Private::No,
        (k, _) => k,
    };
    let p = Param { private, ..p };
    match b {
        b'0'..=b'9' => {
            let now = match (p.now, b) {
                (Now::Empty, b'2') => Now::Two,
                _ => Now::Other,
            };
            At::Csi(Param { now, ..p })
        }
        b';' | b':' => At::Csi(Param {
            two: p.ended(),
            now: Now::Empty,
            ..p
        }),
        b'l' if p.private == Private::Yes && p.ended() == Two::Yes => At::Vt52(Vt52::Ground),
        0x40..=0x7e => At::Ground,
        _ => At::Csi(Param {
            now: Now::Other,
            ..p
        }),
    }
}

/// **The bytes before the shell's `D`, judged writing only forward**:
/// printable UTF-8, `CR`, `LF`, `HT` and `SGR` as exactly `CSI [0-9;:]* m`,
/// nothing else. What they did to the row the `D` is on, and zsh's
/// `PROMPT_SP` run after their last `LF`, if they end with one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Forward {
    /// What the bytes did to the row they ended on.
    reach: Reach,
    /// zsh's run, if the bytes after the last `LF` are exactly it.
    eol: Option<Eol>,
}

/// **zsh's default `PROMPT_SP` run**: where it begins in the read, and its
/// blanks, which must be the terminal's columns less one for the cut to
/// move there.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Eol {
    at: BytePos,
    blanks: ReadLen,
}

impl Eol {
    /// Where the run begins in the read.
    pub fn at(self) -> BytePos {
        self.at
    }
}

/// Each offset shown as a number.
impl core::fmt::Debug for Eol {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Eol")
            .field("at", &self.at.index())
            .field("blanks", &self.blanks.index())
            .finish()
    }
}

impl Forward {
    /// **`bytes` judged**, none if any of them is not allowed.
    fn of(bytes: &[u8]) -> Option<Forward> {
        let text = core::str::from_utf8(bytes).ok()?;
        let mut rest = text.as_bytes();
        while let Some((&b, after)) = rest.split_first() {
            rest = match b {
                0x1b => sgr(after)?,
                b'\r' | b'\n' | b'\t' => after,
                0x00..=0x1f | 0x7f => return None,
                _ => {
                    let c = text[bytes.len() - rest.len()..].chars().next()?;
                    if ('\u{80}'..='\u{9f}').contains(&c) {
                        return None;
                    }
                    &rest[c.len_utf8()..]
                }
            };
        }
        let line = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        let reach = match (bytes.last(), line) {
            (Some(b'\n'), _) => Reach::Below,
            _ => Reach::Forward,
        };
        Some(Forward {
            reach,
            eol: eol(&bytes[line..]).map(|blanks| Eol {
                at: BytePos::from_index(line),
                blanks,
            }),
        })
    }
}

/// The bytes after an `ESC` past one `SGR`, `[` then `[0-9;:]*` then `m`;
/// none if they are not one.
fn sgr(after: &[u8]) -> Option<&[u8]> {
    let body = after.strip_prefix(b"[")?;
    let n = body
        .iter()
        .position(|b| !matches!(b, b'0'..=b'9' | b';' | b':'))?;
    match body[n] {
        b'm' => Some(&body[n + 1..]),
        _ => None,
    }
}

/// Past any `SGR`s at the front of `bytes`.
fn sgrs(mut bytes: &[u8]) -> &[u8] {
    while let Some(after) = bytes.strip_prefix(b"\x1b").and_then(sgr) {
        bytes = after;
    }
    bytes
}

/// **The blanks of zsh's default `PROMPT_SP` run, if `bytes` are exactly
/// it**: `SGR* (%|#) SGR* SP+ CR SP? CR`.
fn eol(bytes: &[u8]) -> Option<ReadLen> {
    let rest = sgrs(bytes);
    let rest = rest
        .strip_prefix(b"%")
        .or_else(|| rest.strip_prefix(b"#"))?;
    let rest = sgrs(rest);
    let blanks = rest.iter().position(|&b| b != b' ')?;
    let rest = rest[blanks..].strip_prefix(b"\r")?;
    let rest = rest.strip_prefix(b" ").unwrap_or(rest);
    match (blanks, rest) {
        (1.., b"\r") => Some(ReadLen::from_index(blanks)),
        _ => None,
    }
}

/// **Where a read is to be cut**, as its tag carries it (`reads::Part`):
/// the shell's `D`, what the bytes after it are, what those before it did
/// to the row it is on, and zsh's run before it, if any.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    d: BytePos,
    after: Ran,
    reach: Reach,
    eol: Option<Eol>,
}

/// Each offset shown as a number.
impl core::fmt::Debug for Plan {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Plan")
            .field("d", &self.d.index())
            .field("after", &self.after)
            .field("reach", &self.reach)
            .field("eol", &self.eol)
            .finish()
    }
}

impl Plan {
    /// **The plan for a read**, `d` where its shell's `D` begins and
    /// `after` what the bytes from there are: none unless the read began at
    /// ground, under UTF-8, and the bytes before the `D` are [`Forward`].
    pub fn of(bytes: &[u8], d: BytePos, after: Ran, grounded: Grounded) -> Option<Plan> {
        if grounded == Grounded::No {
            return None;
        }
        let f = Forward::of(bytes.get(..d.index())?)?;
        Some(Plan {
            d,
            after,
            reach: f.reach,
            eol: f.eol,
        })
    }

    /// Where the shell's `D` begins in the read.
    pub fn d(&self) -> BytePos {
        self.d
    }

    /// What the bytes from the `D` are.
    pub fn after(&self) -> Ran {
        self.after
    }

    /// zsh's run before the `D`, if the bytes after the last `LF` are it.
    pub fn eol(&self) -> Option<Eol> {
        self.eol
    }

    /// The plan for the bytes from `from` on, counted from there: zsh's
    /// run dropped once it is behind them, none once the `D` is.
    pub fn rest(self, from: BytePos) -> Option<Plan> {
        let d = self.d.index().checked_sub(from.index())?;
        Some(Plan {
            d: BytePos::from_index(d),
            eol: self.eol.and_then(|e| {
                Some(Eol {
                    at: BytePos::from_index(e.at.index().checked_sub(from.index())?),
                    ..e
                })
            }),
            ..self
        })
    }
}

/// Whether zsh's run may take the cut: the cursor at the first column and
/// the blanks the terminal's columns less one, both read from `term`.
pub fn eol_holds(term: &Term<'_>, eol: Eol) -> bool {
    let (cols, _) = term.geometry();
    term.cursor().1 == Col::at_index(0) && cols.index().checked_sub(1) == Some(eol.blanks.index())
}

/// **Where the cut was taken**, read from the terminal as the bytes after
/// it began: the cursor's row, the ring's rotation, which surface was up and
/// the grid's size.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spot {
    row: Row,
    rot: RowRot,
    epoch: ScreenEpoch,
    grid: (Cols, Rows),
}

impl Spot {
    /// The cut as `term` stands now.
    pub fn of(term: &Term<'_>) -> Spot {
        Spot {
            row: term.cursor().0,
            rot: term.rotation(),
            epoch: term.screen_epoch(),
            grid: term.geometry(),
        }
    }

    /// **The line the cut was on**, the batch having ended on `ended` with
    /// `term` as it stands: none if the surface switched, the grid changed
    /// size, or the screen took lines back since, which no count can
    /// number.
    pub fn line(&self, term: &Term<'_>, ended: AbsLine) -> Option<AbsLine> {
        if term.screen_epoch() != self.epoch || term.geometry() != self.grid {
            return None;
        }
        let retired = match term.moved_since(self.rot) {
            baud_core::Moved::Retired(n) => n.index(),
            baud_core::Moved::Restored(_) => return None,
        };
        let back = (retired + term.cursor().0.index()).checked_sub(self.row.index())?;
        // Counted back a line at a time: the lines after the cut in one
        // batch, which the ring bounds.
        (0..back).try_fold(ended, |line, _| line.above())
    }
}

/// **A part after the cut**: where it was taken, and what the part would
/// be filed as without it, which it is filed as when the cut's line cannot
/// be numbered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mark {
    pub spot: Spot,
    pub uncut: Uncut,
}

/// What a part after the cut would be filed as without it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Uncut {
    pub marked: Ran,
    pub before: Option<crate::sys::Pid>,
    pub watched: crate::probe::Watched,
}

/// **What a read's tag says of the cut** (`reads::Part`): nothing, where
/// it is to be taken, or, for a part after it, where it was.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Cut {
    #[default]
    None,
    Plan(Plan),
    After(Mark),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `bytes` before a `D` would let the cut be taken.
    fn admits(bytes: &[u8]) -> bool {
        Forward::of(bytes).is_some()
    }

    /// **Text, `CR`, `LF`, `HT` and colour write only forward**: plain and
    /// multi-byte text, tabs, a colour in every spelling `SGR` takes, and
    /// nothing at all; what they did to the row they ended on, below it
    /// after a line feed.
    #[test]
    fn text_and_colour_write_only_forward() {
        let f = Forward::of(b"logout\r\n").expect("text");
        assert_eq!((f.reach, f.eol), (Reach::Below, None));
        assert!(admits("a\tb \u{2713} \u{e9}\r\n".as_bytes()), "UTF-8, HT");
        assert!(admits(b"\x1b[1;38:2::1:2:3m x\x1b[m\x1b[0;48;5;10m"));
        assert_eq!(Forward::of(b"x").map(|f| f.reach), Some(Reach::Forward));
        assert!(admits(b""), "nothing");
    }

    /// **Another escape makes no cut**: `ESC 7`, a designation, `RIS`.
    /// Named break: pass over any escape's next byte.
    #[test]
    fn another_escape_makes_no_cut() {
        for b in [&b"x\x1b7"[..], b"\x1b(0q", b"\x1bc"] {
            assert!(!admits(b), "{:?}", b.escape_ascii().to_string());
        }
    }

    /// **A `CSI` other than `SGR` makes no cut**: an erase, a cursor move,
    /// `CSI ? m`, an intermediate, a `>` form. Named break: pass any `CSI`
    /// to its final byte.
    #[test]
    fn a_csi_not_sgr_makes_no_cut() {
        for b in [
            &b"\x1b[2J"[..],
            b"\x1b[A",
            b"\x1b[?m",
            b"\x1b[1 m",
            b"\x1b[>4m",
        ] {
            assert!(!admits(b), "{:?}", b.escape_ascii().to_string());
        }
    }

    /// **A control string makes no cut**: an `OSC` ended by `BEL` or `ST`,
    /// a title among them, the shell's marks of a remote shell, a `DCS`.
    /// Named break: pass a control string to its end.
    #[test]
    fn a_control_string_makes_no_cut() {
        for b in [
            &b"\x1b]0;title\x07"[..],
            b"\x1b]133;A\x1b\\",
            b"\x1bPq#0\x1b\\",
        ] {
            assert!(!admits(b), "{:?}", b.escape_ascii().to_string());
        }
    }

    /// **A backspace makes no cut**: under `?45` one at the first column
    /// moves to the row above. Named break: let `BS` through with `HT`.
    #[test]
    fn a_backspace_makes_no_cut() {
        assert!(!admits(b"ab\x08"));
    }

    /// **A C1 code, `DEL` or a malformed character makes no cut**: `CSI`
    /// as `C2 9B`, `NEL` as `C2 85`, `DEL`, a lone continuation byte, a
    /// character cut short. Named break: let U+0080 to U+009F through.
    #[test]
    fn a_c1_code_del_or_bad_utf8_makes_no_cut() {
        for b in [
            &b"\xc2\x9b2J"[..],
            b"x\xc2\x85",
            b"x\x7f",
            b"\x9b",
            b"\xe2\x86",
        ] {
            assert!(!admits(b), "{:?}", b.escape_ascii().to_string());
        }
    }

    /// **zsh's default `PROMPT_SP` run is read exactly**: its mark in bold
    /// and standout, or `#`, the blanks, `CR`, a blank and `CR`, or no
    /// blank; and nothing else after the last `LF` -- text before the mark,
    /// a mark of the user's, no blanks, a `LF` after the blanks. Named
    /// break: find the run from its mark, whatever comes before it on its
    /// row -- the text before the mark is read as the run.
    #[test]
    fn zshs_run_is_read_exactly() {
        let run = |mark: &str, blanks: usize, tail: &str| {
            format!(
                "\x1b[1m\x1b[7m{mark}\x1b[27m\x1b[1m\x1b[0m{}\r{tail}",
                " ".repeat(blanks)
            )
        };
        let eol_of = |bytes: &[u8]| Forward::of(bytes).and_then(|f| f.eol);
        let seen = eol_of(format!("logout\r\n{}", run("%", 79, " \r")).as_bytes());
        assert_eq!(
            seen.map(|e| (e.at.index(), e.blanks.index())),
            Some((8, 79)),
            "after the last LF"
        );
        assert!(
            eol_of(run("#", 3, "\r").as_bytes()).is_some(),
            "# and no blank"
        );
        for bytes in [
            format!("remote$ {}", run("%", 79, " \r")),
            run("\u{bb}", 79, " \r"),
            run("%", 0, " \r"),
            format!("{}\n", run("%", 79, " \r")),
        ] {
            assert_eq!(eol_of(bytes.as_bytes()), None, "{bytes:?}");
        }
    }

    /// The stream after `bytes`, from ground.
    fn after(bytes: &[u8]) -> Grounded {
        let mut s = Stream::default();
        s.read(bytes);
        s.grounded()
    }

    /// **A read begun inside a sequence makes no cut**: the stream is not
    /// at ground inside a `CSI`, an `OSC` until `BEL` or `ST`, a `DCS`
    /// whatever a `BEL` in it, a character cut short, VT52 until `ESC <`
    /// and `ESC % @` until `ESC % G`; and it is after each. A plan made from
    /// a read begun off ground is none. Named break: the stream always at
    /// ground -- a read begun inside a `CSI` is planned.
    #[test]
    fn a_read_begun_inside_a_sequence_makes_no_cut() {
        for (bytes, want) in [
            (&b"\x1b["[..], Grounded::No),
            (b"\x1b[31m", Grounded::Yes),
            (b"\x1b]0;t", Grounded::No),
            (b"\x1b]0;t\x07", Grounded::Yes),
            (b"\x1b]0;t\x1b\\", Grounded::Yes),
            (b"\x1bPq\x07", Grounded::No),
            (b"\x1bPq\x1b\\", Grounded::Yes),
            (b"\xe2\x86", Grounded::No),
            (b"\xe2\x86\x92", Grounded::Yes),
            (b"\x1b[?2l", Grounded::No),
            (b"\x1b[?2lx\x1bY  \x1b<", Grounded::Yes),
            (b"\x1b[?25l", Grounded::Yes),
            (b"\x1b%@", Grounded::No),
            (b"\x1b%@\x9b\x1b%G", Grounded::Yes),
            (b"\x1b[\x18", Grounded::Yes),
        ] {
            assert_eq!(after(bytes), want, "{:?}", bytes.escape_ascii().to_string());
        }
        let mut s = Stream::default();
        s.read(b"\x1b[");
        let plan = |g| Plan::of(b"31mx\x1b]133;D\x07", BytePos::from_index(4), Ran::Shell, g);
        assert_eq!(plan(s.grounded()), None, "begun inside a CSI");
        assert!(plan(Grounded::Yes).is_some(), "the control");
    }

    /// **`ESC DEL` is still an escape**, as the core reads it: `DEL` is
    /// ignored there and the sequence goes on, so a read ending on it does
    /// not end at ground, and the `[5A` the next read begins with is the
    /// core's `CUU`. A control there is carried out and the escape goes on
    /// too. Named break: `DEL` after `ESC` back to ground -- a read ending
    /// `ESC DEL` is at ground.
    #[test]
    fn an_escape_goes_on_past_del() {
        assert_eq!(after(b"x\x1b\x7f"), Grounded::No, "ESC DEL");
        assert_eq!(after(b"x\x1b\x7f[5A"), Grounded::Yes, "ended by its CSI");
        assert_eq!(after(b"x\x1b\x07"), Grounded::No, "ESC BEL");
    }

    /// **`ESC %` with anything before its final but a direct `@` or `G` is
    /// taken as `ESC % @` until `ESC % G`**: the core keeps the `%` as an
    /// intermediate, carries out a control or ignores `DEL` there, and
    /// switches on the `@` after them, so the stream is not at ground, and
    /// stays not at ground over text, until `ESC % G`. Named break: `ESC %`
    /// and any byte but `@` back to ground -- `ESC % BEL @` is at ground.
    #[test]
    fn escape_percent_with_anything_before_its_final_fails_closed() {
        for shape in [&b"\x1b%\x07@"[..], b"\x1b%\x7f@", b"\x1b%\x01"] {
            let at = |tail: &[u8]| after(&[shape, tail].concat());
            let shown = shape.escape_ascii().to_string();
            assert_eq!(at(b""), Grounded::No, "{shown}");
            assert_eq!(at(b"text \x9b"), Grounded::No, "{shown}, then text");
            assert_eq!(at(b"text\x1b%G"), Grounded::Yes, "{shown}, then ESC % G");
        }
        assert_eq!(after(b"\x1b%G"), Grounded::Yes, "the control: a direct G");
    }
}
