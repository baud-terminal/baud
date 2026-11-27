//! **What a part's record vouches for**, read from what the core did as it
//! ran the part (`reads::Taken`) rather than from the bytes: whether the
//! line it ended on holds nothing it wrote ([`Below`]), and whether the
//! bytes before the shell's `D` wrote only forward, so the read may be cut
//! there ([`CutAt`]). Each is a type only its own constructor makes, and
//! every condition lives in that constructor: a call site holds a `Below`
//! or a `CutAt` only where the record vouched, and is told why where it
//! did not ([`Why`]).

use baud_core::{ActiveDisplay, Cleared, Feeds, Grounded, ReadEncoding, ReadRecord, Screen, Term};
use baud_domain::PlaceSerial;

/// **A verdict**: what the record vouched for, or why it did not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vouch<T> {
    Yes(T),
    No(Why),
}

/// **Why a record did not vouch.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Why {
    /// The part has no record of its own, or one the core says renumbered
    /// its lines, or the core recorded nothing.
    Unrecorded,
    /// A byte ran under ISO 8859-1, where `0x80`-`0x9F` are controls.
    Encoding,
    /// The part began, or ended, inside a sequence or a character, or under
    /// VT52.
    Ground(Grounded),
    /// A line feed did not move the cursor: on the last row below a region
    /// that ends higher, it has nothing to do (H3).
    FeedStuck,
    /// The part wrote off its forward path: a climb, an erase or an edit of
    /// other rows, or a switch of screens.
    OffPath,
    /// The part moved text between lines.
    Moved,
    /// The part cleared the alternate screen.
    Cleared,
    /// The part ended on the line it began on.
    SameLine,
    /// The row the part ended on holds cells.
    EndRow,
    /// The part ended with the cursor on the status line, whose row is no
    /// line of the main display's.
    Status,
    /// The part placed an image, which leaves the row's mark where it was.
    Image,
    /// The shell's marks the app read in the part are not the core's,
    /// offset for offset and letter for letter, or more than either keeps.
    Marks,
    /// The window's marks drifted in an earlier part and have not been
    /// re-synced since ([`Latch`]).
    Drifted,
}

/// **The cursor's row as a part ended** (`Term::row_extent`): empty, or
/// holding cells -- text, or blanks other than the default -- or the
/// status line's, read once per part, with its record (`reads::paced`).
/// A mark of 0 means nothing was written there: a fill stores 0 only when
/// it fills with nothing, and every glyph raises it (pinned in
/// `crates/baud-core/tests/extent_zero_is_empty.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EndRow {
    Empty,
    Holds,
    /// The cursor on the status line (`DECSASD 1`).
    Status,
}

impl EndRow {
    /// The cursor's row of `term` as it stands.
    pub fn of(term: &Term<'_>) -> EndRow {
        if term.active_display() == ActiveDisplay::Status {
            return EndRow::Status;
        }
        match term.row_extent(term.cursor().0).index() {
            0 => EndRow::Empty,
            _ => EndRow::Holds,
        }
    }
}

/// **The image store's placement clock as a part began**
/// (`ImageStore::last_serial`), read in `reads::paced` before the part is
/// fed, so [`Drew::of`] can say after it whether the part placed an image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImagesAt(PlaceSerial);

impl ImagesAt {
    /// The clock of `term` as it stands.
    pub fn of(term: &Term<'_>) -> ImagesAt {
        ImagesAt(term.images().last_serial())
    }
}

/// **Whether a part placed an image**: a Sixel, a kitty or iTerm2 image, a
/// ReGIS page. An image leaves the row's mark at 0, so the end row cannot
/// say it, and is its own condition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drew {
    Nothing,
    Image,
}

impl Drew {
    /// What the part fed since `began` drew, `term` as it ended.
    pub fn of(began: ImagesAt, term: &Term<'_>) -> Drew {
        match ImagesAt::of(term) == began {
            true => Drew::Nothing,
            false => Drew::Image,
        }
    }
}

/// Where a record began and ended at ANSI ground, under UTF-8, every line
/// feed moved, and nothing renumbered: what both verdicts ask first.
fn grounded(rec: &ReadRecord) -> Option<Why> {
    if rec.renumbered().is_some() {
        return Some(Why::Unrecorded);
    }
    if rec.encoding() != ReadEncoding::Utf8 {
        return Some(Why::Encoding);
    }
    match rec.grounded() {
        (Grounded::Ansi, Grounded::Ansi) => {}
        (Grounded::Ansi, g) | (g, _) => return Some(Why::Ground(g)),
    }
    match rec.feeds() {
        Feeds::Moved => None,
        Feeds::Stuck => Some(Why::FeedStuck),
    }
}

/// **The line a part ended on holds nothing it wrote**: it ran at ANSI
/// ground from start to end, under UTF-8, every line feed it ran moved the
/// cursor, it wrote nothing off its forward path on that line, it ended on
/// a line past the one it began on, the row there is empty as it ended and
/// on the main display, and the part placed no image. So the next writer's
/// text there is the next writer's -- where the shell draws its prompt
/// after a command's last line feed -- and a title, a mark or a colour
/// after that line feed leaves it so, where text, an image or a pending
/// wrap does not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Below {
    screen: Screen,
    line: baud_domain::LineIdx,
}

impl Below {
    /// **What `rec`, with the row it ended on `end`, what it `drew` and
    /// whether its marks were the core's (`matched`), vouches for.**
    pub fn of(rec: &ReadRecord, end: EndRow, drew: Drew, matched: Vouch<Matched>) -> Vouch<Below> {
        if let Some(why) = grounded(rec) {
            return Vouch::No(why);
        }
        if let Vouch::No(why) = matched {
            return Vouch::No(why);
        }
        // On the status line the cursor is on no line of the main
        // display's, whatever line the record names.
        if end == EndRow::Status {
            return Vouch::No(Why::Status);
        }
        let (start, at) = (rec.start(), rec.end());
        if rec.off_path(at.screen).is_some_and(|s| s.holds(at.line)) {
            return Vouch::No(Why::OffPath);
        }
        if start.screen == at.screen && at.line <= start.line {
            return Vouch::No(Why::SameLine);
        }
        if drew == Drew::Image {
            return Vouch::No(Why::Image);
        }
        match end {
            EndRow::Holds | EndRow::Status => Vouch::No(Why::EndRow),
            EndRow::Empty => Vouch::Yes(Below {
                screen: at.screen,
                line: at.line,
            }),
        }
    }

    /// The screen and the line the part ended on, which holds nothing it
    /// wrote.
    pub fn line(self) -> (Screen, baud_domain::LineIdx) {
        (self.screen, self.line)
    }
}

/// **The bytes before the shell's `D` wrote only forward**: every part of
/// the read before the cut, and the cut's own point, ran at ANSI ground,
/// under UTF-8, every line feed moved, and none wrote off its forward path,
/// moved text, cleared the alternate screen or renumbered a line. So those
/// bytes wrote only on the rows the cursor passed as it went, and what the
/// `D` and after it write is the shell's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CutAt(());

impl CutAt {
    /// **What one part before the cut, or the cut's point, vouches for**
    /// (`Term::take_record` there: a record of nothing, which says where
    /// the parser stands and under which encoding), with whether the
    /// shell's marks read in it were the core's (`matched`).
    pub fn of(rec: &ReadRecord, matched: Vouch<Matched>) -> Vouch<CutAt> {
        if let Some(why) = grounded(rec) {
            return Vouch::No(why);
        }
        if let Vouch::No(why) = matched {
            return Vouch::No(why);
        }
        if rec.cleared() == Cleared::Alternate {
            return Vouch::No(Why::Cleared);
        }
        if rec.moves().iter().next().is_some() {
            return Vouch::No(Why::Moved);
        }
        match rec
            .off_path(Screen::Normal)
            .or(rec.off_path(Screen::Alternate))
        {
            None => Vouch::Yes(CutAt(())),
            Some(_) => Vouch::No(Why::OffPath),
        }
    }
}

impl Vouch<CutAt> {
    /// **This verdict and then `next`'s**, for the parts of one read before
    /// its cut: yes only while both are, the first refusal kept.
    pub fn and(self, next: Vouch<CutAt>) -> Vouch<CutAt> {
        match self {
            Vouch::Yes(_) => next,
            Vouch::No(why) => Vouch::No(why),
        }
    }
}

/// **The shell's marks the app folded in a part are the core's**: under
/// UTF-8, neither side past what it keeps, and the same marks at the same
/// offsets with the same letters, in order (`ran::Timeline`,
/// `ReadRecord::marks`). Where they are not -- more marks than the record
/// keeps, so the fold missed some -- what the app read of the shell's
/// commands is not what the core ran, and the part
/// vouches nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Matched(());

impl Matched {
    /// Whether the marks `ran` read in the part `rec` records match.
    pub fn of(rec: &ReadRecord, ran: &crate::ran::Timeline) -> Vouch<Matched> {
        if rec.encoding() != ReadEncoding::Utf8 {
            return Vouch::No(Why::Encoding);
        }
        if rec.marks().fill() != baud_core::Fill::Room || ran.overflowed() {
            return Vouch::No(Why::Marks);
        }
        let core = rec.marks().iter().map(|m| (m.at(), m.letter().raw_byte()));
        match core.eq(ran.marks().map(|m| (m.at(), m.letter().raw_byte()))) {
            true => Vouch::Yes(Matched(())),
            false => Vouch::No(Why::Marks),
        }
    }
}

/// **A part re-syncs the window's marks**: they match the core's
/// ([`Matched`]); among them is the shell's `D` with the window's token,
/// which ends whatever was open whatever the fold held before it; and
/// after the last such `D` no mark opens a command and nothing is open as
/// the part ends. What the fold reads from there on depends on nothing it
/// read before, so a window whose marks drifted is trusted again from the
/// next part -- or from this one, when it begins at that `D`, the shell's
/// part of a read cut there ([`Resync::trusts_itself`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Resync {
    start: Start,
}

/// Whether the part's first mark is the shell's `D` that re-syncs it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Start {
    AtD,
    Later,
}

impl Resync {
    /// Whether the part `rec` records, whose marks `ran` read, re-syncs.
    pub fn of(rec: &ReadRecord, ran: &crate::ran::Timeline) -> Option<Resync> {
        use crate::ran::{Left, ShellsD};
        if let Vouch::No(_) = Matched::of(rec, ran) {
            return None;
        }
        let shells = |m: &crate::ran::Mark| m.shells_d() == ShellsD::Yes;
        let d = ran
            .marks()
            .enumerate()
            .filter(|(_, m)| shells(m))
            .map(|(k, _)| k)
            .last()?;
        let opens = ran
            .marks()
            .skip(d + 1)
            .any(|m| m.letter().raw_byte() == b'C');
        let left = ran.marks().last().map(|m| m.left());
        if opens || left != Some(Left::Closed) {
            return None;
        }
        let start = match ran.marks().next().is_some_and(|m| shells(&m)) {
            true => Start::AtD,
            false => Start::Later,
        };
        Some(Resync { start })
    }

    /// Whether the part itself is trusted: its first mark is the shell's
    /// `D`, so nothing in it was read under what drifted.
    pub fn trusts_itself(self) -> bool {
        self.start == Start::AtD
    }
}

/// **Whether a window's marks are the core's**: synced until a part's are
/// not ([`Matched`]), and then drifted until a part re-syncs them
/// ([`Resync`]). While drifted, no part's marks are trusted: what the fold
/// holds open may be wrong in either direction, as it missed the marks
/// the record could not keep and its state carries from part to part.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Latch {
    #[default]
    Synced,
    Drifted,
}

/// **The window's own shell has ended its command and left nothing open**,
/// in a part the core read alike and the latch trusts: a re-sync
/// ([`Resync`]) through a trusted step ([`Latch::step`]). The shell's `D`,
/// "End of current command" (`[[source: semantic-prompts]]`), with the
/// token baud handed this window's shell, which no program on another
/// machine and no nested shell holds. The shell prints it once it has
/// waited for its foreground job, so every byte that job wrote is before
/// it in the stream. Carries no span and no verdict: it says when, never
/// who, and only the printed paths' pace takes it (`probe::Pace::ended`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ended(());

impl Latch {
    /// **The latch after a part**, whether that part's marks are trusted,
    /// and whether the window's shell ended its command in it ([`Ended`]):
    /// `rec` its record, `ran` the marks folded from it. A part that
    /// re-syncs is trusted itself only if it begins at the shell's `D`
    /// ([`Resync::trusts_itself`]); a drifted window re-synced by a later
    /// mark of the part is trusted from the next part, and the part ends
    /// nothing.
    pub fn step(
        self,
        rec: &ReadRecord,
        ran: &crate::ran::Timeline,
    ) -> (Latch, Vouch<Matched>, Option<Ended>) {
        let matched = Matched::of(rec, ran);
        let resync = Resync::of(rec, ran);
        let (latch, trusted) = match (self, matched, resync) {
            (_, Vouch::No(why), _) => (Latch::Drifted, Vouch::No(why)),
            (Latch::Synced, yes, _) => (Latch::Synced, yes),
            (Latch::Drifted, yes, Some(r)) if r.trusts_itself() => (Latch::Synced, yes),
            (Latch::Drifted, Vouch::Yes(_), Some(_)) => (Latch::Synced, Vouch::No(Why::Drifted)),
            (Latch::Drifted, Vouch::Yes(_), None) => (Latch::Drifted, Vouch::No(Why::Drifted)),
        };
        let ended = match (trusted, resync) {
            (Vouch::Yes(_), Some(_)) => Some(Ended(())),
            (Vouch::Yes(_), None) | (Vouch::No(_), _) => None,
        };
        (latch, trusted, ended)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 6,
    };

    /// The record of `bytes`, fed after `before`, the row it ended on and
    /// what it drew, on a terminal with image memory.
    fn taken(before: &[u8], bytes: &[u8]) -> (ReadRecord, EndRow, Drew) {
        taken_under(baud_domain::ControlStringEnd::Standard, before, bytes)
    }

    /// [`taken`] on a terminal that ends an `APC G` where `end` says:
    /// under `Kitty` only at `BEL` or `ESC \\`, so a picture's body can
    /// hold a mark the core never reads.
    fn taken_under(
        end: baud_domain::ControlStringEnd,
        before: &[u8],
        bytes: &[u8],
    ) -> (ReadRecord, EndRow, Drew) {
        let mut owned = Owned::new(&WS).with_images(baud_domain::ImageBudget::of(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_control_string_end(end);
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        feed(&mut t, before);
        let _ = t.take_record();
        let images = ImagesAt::of(&t);
        feed(&mut t, bytes);
        let (end, drew) = (EndRow::of(&t), Drew::of(images, &t));
        (t.take_record(), end, drew)
    }

    /// The marks the core recorded of `bytes` after `before`, on a window
    /// whose shell holds the token `t`, folded.
    fn scanned(before: &[u8], bytes: &[u8]) -> crate::ran::Timeline {
        scanned_under(baud_domain::ControlStringEnd::default(), before, bytes)
    }

    /// [`scanned`] on a terminal whose control strings end as `end` says,
    /// the same terminal [`taken_under`] records.
    fn scanned_under(
        end: baud_domain::ControlStringEnd,
        before: &[u8],
        bytes: &[u8],
    ) -> crate::ran::Timeline {
        let cuts = crate::ran::fed::Cuts::Unchecked;
        crate::ran::fed::with_shell_under(end, Some("t"), cuts, |s| {
            let _ = s.read(before);
            let _ = s.read(bytes);
            s.marks()
        })
    }

    fn below(before: &[u8], bytes: &[u8]) -> Result<(), Why> {
        let (rec, end, drew) = taken(before, bytes);
        let matched = Matched::of(&rec, &scanned(before, bytes));
        match Below::of(&rec, end, drew, matched) {
            Vouch::Yes(_) => Ok(()),
            Vouch::No(why) => Err(why),
        }
    }

    fn cut(before: &[u8], bytes: &[u8]) -> Result<(), Why> {
        let rec = taken(before, bytes).0;
        match CutAt::of(&rec, Matched::of(&rec, &scanned(before, bytes))) {
            Vouch::Yes(_) => Ok(()),
            Vouch::No(why) => Err(why),
        }
    }

    /// **The line a part ended on is vouched clean only where the record
    /// says so, each condition refusing on its own**: text and a line feed
    /// leave it clean, and so does a title or a colour after the feed; text
    /// after the feed, a picture-free erase above, a climb back onto it, a
    /// feed stuck below a region, a read ending mid-sequence, one under ISO
    /// 8859-1, one that never left its line, a pending wrap, a row of one
    /// character, the cursor on the status line and a Sixel after the line
    /// feed do not. Named breaks, one per condition: drop it from
    /// `Below::of` -- its row vouches; for the status line, `EndRow::of`
    /// blind to it -- its row vouches; for the image, `Drew::of` always
    /// `Nothing` -- the Sixel's row vouches.
    #[test]
    fn below_vouches_only_where_the_record_does() {
        assert_eq!(below(b"", b"logout\r\n"), Ok(()));
        assert_eq!(below(b"", b"logout\r\n\x1b]0;t\x07\x1b[m"), Ok(()));
        assert_eq!(below(b"", b"foo\r\nbar"), Err(Why::EndRow));
        assert_eq!(below(b"", b"x\r\ny\r\n\x1b[Ax\r\n"), Err(Why::OffPath));
        assert_eq!(
            below(b"\x1b[1;3r\x1b[6;1H", b"x\r\n"),
            Err(Why::FeedStuck),
            "stuck below a region"
        );
        assert_eq!(
            below(b"", b"x\r\n\x1b["),
            Err(Why::Ground(Grounded::Inside))
        );
        assert_eq!(below(b"\x1b%@", b"x\r\n"), Err(Why::Encoding));
        assert_eq!(below(b"", b"x"), Err(Why::SameLine));
        assert_eq!(
            below(b"", b"foo\r\nxxxxxxxxxxxxxxxxxxxx"),
            Err(Why::EndRow),
            "a pending wrap: a full row, the cursor still on it"
        );
        assert_eq!(
            below(b"", &[b"foo\r\n".as_slice(), &[b'x'; 80]].concat()),
            Err(Why::EndRow),
            "a row of one character"
        );
        assert_eq!(
            below(b"\x1b[2$~", b"logout\r\n\x1b[1$}"),
            Err(Why::Status),
            "the cursor on the status line"
        );
        assert_eq!(
            below(b"", b"logout\r\n\x1bPq#0;2;100;0;0#0~~~~\x1b\\"),
            Err(Why::Image),
            "a Sixel after the line feed"
        );
        assert_eq!(
            below(b"\x1bPq#0;2;100;0;0#0~~~~\x1b\\", b"logout\r\n"),
            Ok(()),
            "the control: an image before the part"
        );
    }

    /// **A part re-syncs only at a mark the core ran** (`Resync::of`): the
    /// shell's `D` with its token and an `A`, re-syncing; the same bytes
    /// inside a kitty graphics command's body (`ESC _ G a=d;`, left open,
    /// on a terminal that ends one only at `BEL` or `ESC \\`), where the
    /// core runs no mark, carry no mark the fold reads, and do not. The
    /// token is the core's to read, in a mark it ran, so the bytes were a
    /// control string and not a picture's payload. Named break: fold marks
    /// read from the bytes rather than from the record -- the payload's
    /// `D` re-syncs.
    #[test]
    fn a_tokened_d_in_a_pictures_body_does_not_re_sync() {
        let d = b"\x1b]133;D;0;baud_token=t\x07\x1b]133;A\x07";
        let resync = |bytes: &[u8]| {
            let (rec, _, _) = taken_under(baud_domain::ControlStringEnd::Kitty, b"", bytes);
            let ran = scanned_under(baud_domain::ControlStringEnd::Kitty, b"", bytes);
            let read = ran
                .marks()
                .any(|m| m.shells_d() == crate::ran::ShellsD::Yes);
            let resyncs = Resync::of(&rec, &ran).is_some();
            assert_eq!(read, resyncs, "the shell's D read, and only then a re-sync");
            resyncs
        };
        assert!(resync(d), "the control");
        assert!(!resync(&[b"\x1b_Ga=d;".as_slice(), d].concat()));
    }

    /// **The shell's `D` ends its command for the pace only in a part the
    /// latch trusts** (`Latch::step`'s proof, `Ended`): its `C`, output and
    /// `D` in one part, synced, ends it; drifted, a part that begins at
    /// the `D` ends it and re-syncs, and one whose `D` comes after its
    /// first mark re-syncs the window from the next part and ends nothing;
    /// a `D` inside a picture's body, where the core reads no mark (on a
    /// terminal that ends an `APC G` only at `BEL` or `ESC \\`), ends
    /// nothing. Named break: the proof minted from the re-sync alone,
    /// without the latch's verdict -- the drifted part whose `D` comes late
    /// ends the command.
    #[test]
    fn a_part_that_re_syncs_after_its_first_mark_ends_nothing() {
        let ends = |latch: Latch, bytes: &[u8]| {
            let (rec, _, _) = taken_under(baud_domain::ControlStringEnd::Kitty, b"", bytes);
            latch
                .step(
                    &rec,
                    &scanned_under(baud_domain::ControlStringEnd::Kitty, b"", bytes),
                )
                .2
                .is_some()
        };
        let d = b"\x1b]133;D;0;baud_token=t\x07\x1b]133;A\x07";
        let later = [b"\x1b]133;C;baud_token=t\x07x\r\n".as_slice(), d].concat();
        assert!(ends(Latch::Synced, &later), "synced");
        assert!(ends(Latch::Drifted, d), "drifted, the part begins at the D");
        assert!(!ends(Latch::Drifted, &later), "drifted, the D after a mark");
        let inside = [b"\x1b_Ga=d;".as_slice(), d].concat();
        assert!(!ends(Latch::Synced, &inside), "inside a picture's body");
    }

    /// **The bytes before a `D` vouch for a cut only where the record says
    /// they wrote forward, each condition refusing on its own**: text,
    /// colour, a title and a saved cursor do; a climb, an erase of other
    /// rows, a line inserted, a switch of screens, the rows under a region
    /// moved, a feed stuck below a region, a part ending mid-sequence, one
    /// under ISO 8859-1, a cleared alternate screen and a part with more
    /// marks than its record keeps do not. Named breaks, one per
    /// condition: drop it from `CutAt::of` -- its part vouches.
    #[test]
    fn a_cut_is_vouched_only_where_the_record_says_forward() {
        assert_eq!(cut(b"", b"logout\r\n"), Ok(()));
        assert_eq!(cut(b"", b"\x1b[1mx\x1b[m\x1b]0;t\x07\x1b7"), Ok(()));
        assert_eq!(cut(b"", b"x\r\ny\x1b[A"), Err(Why::OffPath));
        assert_eq!(cut(b"x\r\ny\r\n", b"\x1b[1J"), Err(Why::OffPath));
        assert_eq!(
            cut(b"x\r\ny\r\n", b"\x1b[2;1H\x1b[L\x1b[3;1H"),
            Err(Why::Moved)
        );
        assert_eq!(cut(b"\x1b[1;3r\x1b[6;1H", b"x\r\n"), Err(Why::FeedStuck));
        assert_eq!(cut(b"", b"x\x1b["), Err(Why::Ground(Grounded::Inside)));
        assert_eq!(cut(b"\x1b%@", b"x"), Err(Why::Encoding));
        assert_eq!(
            cut(b"", b"\x1b[?47h"),
            Err(Why::OffPath),
            "a switch writes its lines"
        );
        assert_eq!(
            cut(b"\x1b[1;5r\x1b[5;1H", b"x\n"),
            Err(Why::Moved),
            "the rows under a region"
        );
        assert_eq!(cut(b"\x1b[?1049h", b"\x1b[?1049h"), Err(Why::Cleared));
        assert_eq!(
            cut(b"", &b"\x1b]133;A\x07".repeat(9)),
            Err(Why::Marks),
            "more marks than the record keeps"
        );
        assert_eq!(
            cut(b"\x1b[2$~\x1b[1$}", b"\x1b]133;A\x07"),
            Ok(()),
            "a mark the core does not run, on the status line, read by neither"
        );
    }
}
