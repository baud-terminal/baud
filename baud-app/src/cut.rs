//! **A read is cut at the shell's `D`**, so the prompt after a remote
//! command is the shell's even when the read that carries it began with
//! the command's last bytes. A read is filed under the most cautious verdict
//! of what wrote it (`ran::Ran::or`), and rows keep the most cautious fact
//! filed over them, so a read holding `ssh`'s last line, the shell's `D`
//! and the shell's next prompt left that prompt dark for good.
//!
//! The bytes before the `D` keep their own verdict; the `D` and what
//! follows are the shell's, as the core's record of them says
//! (`ran::Fold`, which reads them as a part of their own). The sniff
//! places the `D` before the read is fed, and the cut is taken only where
//! the core's record of the part from the cut places that same `D` there,
//! the shell's with its token; anywhere else, or nowhere, there is no cut.
//! It is taken only where the core's record of every part of
//! the read before it, and of the cut's own point, vouches that those
//! bytes wrote only on the rows the cursor passed as it went
//! (`vouch::CutAt`): at ANSI ground from the read's start to the `D`,
//! under UTF-8, every line feed moved, nothing written off the forward
//! path, no text moved, no screen cleared, no line renumbered. Where it
//! does not, there is no cut, and the read is filed whole. The
//! `D` must be the shell's, with its token, and begin in this read; one
//! begun in an earlier read makes no cut.
//!
//! **zsh's `PROMPT_SP` run comes before the `D`**: zsh prints its default
//! `%B%S%#%s%b` mark, the columns less one of blanks, `CR`, a blank and
//! `CR`, before its `precmd` hooks run, where the integration sends the
//! `D`. Its job is to put the prompt on a clean row: text left on the row
//! wraps above it. So when the bytes after the last `LF` are exactly that
//! run, as zsh writes it by default, the cut may move to the run's start
//! ([`Eol`]): only when the blanks are exactly the terminal's columns less
//! one, the cursor is at the first column, the side margins are the
//! screen's and the row is single-width as the run begins, all read from
//! the terminal there (`reads::paced`, [`eol_holds`]). Any byte before the
//! run, on its row, keeps the cut at the `D`, and a mark the user set fails
//! closed.

use baud_core::{BytePos, ReadLen, ScreenEpoch, Term};
use baud_domain::{Col, Cols, LineRendition, Row, RowRot, Rows};
use baud_render::AbsLine;

use crate::ran::Ran;
use crate::vouch::{CutAt, Vouch};

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
/// the shell's `D`, zsh's run before it, if any, and what the records of the parts fed before the cut so far vouch
/// for (`vouch::CutAt`), none before the first.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    d: BytePos,
    eol: Option<Eol>,
    before: Option<Vouch<CutAt>>,
}

/// Each offset shown as a number.
impl core::fmt::Debug for Plan {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Plan")
            .field("d", &self.d.index())
            .field("eol", &self.eol)
            .field("before", &self.before)
            .finish()
    }
}

impl Plan {
    /// **The plan for a read of `bytes`**, `d` where its shell's `D`
    /// begins, with zsh's run if the bytes after the last `LF` before the
    /// `D` are exactly it; none for a `D` past the read. Whether the cut is
    /// taken is the records' to vouch for as the read is fed, and what the
    /// bytes from the `D` say is the core's record of them, which must
    /// place that `D` at `d` too (`reads::paced`).
    pub fn of(bytes: &[u8], d: BytePos) -> Option<Plan> {
        let before = bytes.get(..d.index())?;
        let line = before
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        Some(Plan {
            d,
            eol: eol(&before[line..]).map(|blanks| Eol {
                at: BytePos::from_index(line),
                blanks,
            }),
            before: None,
        })
    }

    /// What the parts fed before the cut so far vouch for, none before the
    /// first.
    pub fn before(&self) -> Option<Vouch<CutAt>> {
        self.before
    }

    /// The plan after one more part before the cut, which vouched `v`.
    pub fn vouched(self, v: Vouch<CutAt>) -> Plan {
        Plan {
            before: Some(self.before.map_or(v, |b| b.and(v))),
            ..self
        }
    }

    /// Where the shell's `D` begins in the read.
    pub fn d(&self) -> BytePos {
        self.d
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

/// **Whether zsh's run may take the cut**, all read from `term` as the run
/// begins: the cursor at the first column, the blanks the terminal's
/// columns less one, the side margins the screen's own (`DECLRMM`), and the
/// row single-width -- under a right margin, or on a double-width row, the
/// blanks wrap and leave the row's tail where the run was to clear it.
pub fn eol_holds(term: &Term<'_>, eol: Eol) -> bool {
    let (cols, _) = term.geometry();
    let last = Col::at_index(cols.index().saturating_sub(1));
    let (row, col) = term.cursor();
    col == Col::at_index(0)
        && cols.index().checked_sub(1) == Some(eol.blanks.index())
        && term.side_margins() == (Col::at_index(0), last)
        && term.row_extent(row).rendition() == LineRendition::Single
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
        let eol_of = |bytes: &[u8]| {
            let d = [bytes, b"\x1b]133;D\x07"].concat();
            Plan::of(&d, BytePos::from_index(bytes.len())).and_then(|p| p.eol)
        };
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

    /// **zsh's run takes the cut only on a full-width, single-width row**
    /// (R7): the cursor at the first column and the blanks the columns less
    /// one hold; under a right margin short of the screen's edge, or on a
    /// double-width row, the blanks wrap and leave the row's tail, and they
    /// do not. Named breaks: drop the margins from `eol_holds` -- the run
    /// under a right margin holds; drop the rendition -- the run on a
    /// double-width row holds.
    #[test]
    fn zshs_run_holds_only_on_a_full_width_single_width_row() {
        use baud_core::Owned;
        use baud_domain::WinSize;
        let ws = WinSize {
            ws_col: 20,
            ws_row: 4,
        };
        let holds = |set: &[u8]| {
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            let bytes = [set, b"\r"].concat();
            t.write_at(BytePos::ZERO, &bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let eol = Eol {
                at: BytePos::ZERO,
                blanks: ReadLen::from_index(19),
            };
            eol_holds(&t, eol)
        };
        assert!(holds(b""), "the control");
        assert!(!holds(b"\x1b[?69h\x1b[1;10s"), "a right margin");
        assert!(
            holds(b"\x1b[?69h\x1b[1;20s"),
            "margins at the screen's edges"
        );
        assert!(!holds(b"\x1b#6"), "a double-width row");
    }
}
