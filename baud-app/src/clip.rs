//! What an `OSC 52` write the core published does to the clipboard: the
//! policy, pure, so its tests run on every platform (note V, V11).
//!
//! **On by default, with `[clipboard] osc52 = "off"` to turn it off**: the
//! owner's V11, 2026-09-25, as kitty and sfte do. No focus gate: one is to be
//! tested over a real slow ssh link before it is adopted, because a yank that
//! arrives a round trip after the user switched away would be refused and
//! the old clipboard pasted.
//!
//! **macOS has one pasteboard**, so a write that names the clipboard (`c`),
//! the primary (`p`) or xterm's select (`s`, which an empty `Pc` means) lands
//! on it. The secondary (`q`) and the cut buffers (`0`-`7`) have no
//! counterpart, and a write naming only those is refused rather than put
//! somewhere the user did not ask. The pasteboard holds text, so octets that
//! are not UTF-8 are refused too; a `Pd` that was not base64 clears it, which
//! is `[[source: ctlseqs]]`'s "the selection is cleared".
//!
//! **Wayland has two**, the clipboard and the primary selection, so there a
//! write lands on each it names (`boards`): `c` the clipboard, `p` the
//! primary, and `s` -- which an empty `Pc` means -- xterm's SELECT, the
//! primary while `?1041` is reset and the clipboard while it is set
//! (`[[source: ctlseqs]]`; Jev, XTERM, 0.96/0.94, over foot's, alacritty's
//! and kitty's reading of an empty `Pc` as the clipboard). The rest of the
//! policy is the same on both.
//!
//! **Every accepted write is counted** in the five-second report, and every
//! refusal warns once a reason: an accepted write is the risk -- anything
//! printed to the terminal can replace the clipboard -- and its count is an `info` line,
//! which the default `warn` level does not show; whether an accepted write
//! warns is for Jev.

use crate::config::Osc52;
use baud_core::{ClipboardContent, ClipboardWrite, ClipboardWrites, Selection, Term};
use baud_domain::{ClipboardBudget, ClipboardRefusals};
use baud_render::Tally;

/// Which system the writes land on, for what a refusal says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Host {
    /// macOS's one general pasteboard.
    Pasteboard,
    /// A Wayland seat's clipboard and primary selection.
    #[cfg(any(target_os = "linux", test))]
    Wayland,
}

/// A Wayland seat's two selections.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Board {
    Clipboard,
    Primary,
}

/// The selections one write names, on a host with both.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Boards {
    Clipboard,
    Primary,
    Both,
}

#[cfg(any(target_os = "linux", test))]
impl Boards {
    /// The selections, the clipboard first.
    pub fn each(self) -> impl Iterator<Item = Board> {
        let (c, p) = match self {
            Boards::Clipboard => (Some(Board::Clipboard), None),
            Boards::Primary => (None, Some(Board::Primary)),
            Boards::Both => (Some(Board::Clipboard), Some(Board::Primary)),
        };
        c.into_iter().chain(p)
    }
}

/// **What xterm's SELECT names now**: the clipboard while `?1041` is set,
/// else the primary (`[[source: ctlseqs]]`, "Use the CLIPBOARD selection"
/// and "Use the PRIMARY selection"; `[[source: xterm-man]]`,
/// selectToClipboard, "The default is false, which tells it to use
/// PRIMARY"). Where a finished selection goes, what the middle button and
/// Shift+Insert paste, and where an `OSC 52` `s` lands.
#[cfg(any(target_os = "linux", test))]
pub fn select_board(term: &Term<'_>) -> Board {
    match term.resource(baud_core::Resource::SelectToClipboard) {
        true => Board::Clipboard,
        false => Board::Primary,
    }
}

/// **The selections a write naming `targets` lands on**, with SELECT
/// naming `select`; `None` when it names neither the clipboard, the primary
/// nor SELECT, which `decide` has refused already.
#[cfg(any(target_os = "linux", test))]
pub fn boards(targets: baud_core::Selections, select: Board) -> Option<Boards> {
    let s = targets.contains(Selection::Select);
    let c = targets.contains(Selection::Clipboard) || (s && select == Board::Clipboard);
    let p = targets.contains(Selection::Primary) || (s && select == Board::Primary);
    match (c, p) {
        (true, true) => Some(Boards::Both),
        (true, false) => Some(Boards::Clipboard),
        (false, true) => Some(Boards::Primary),
        (false, false) => None,
    }
}

/// Why a write the core published was not put on the pasteboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// `[clipboard] osc52 = "off"`.
    Off,
    /// It named only the secondary selection or the cut buffers, which the
    /// pasteboard is not.
    NotThePasteboard,
    /// Its octets are not UTF-8, and the pasteboard holds text.
    NotText,
}

/// What to do with one write.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision<'a> {
    /// Replace the pasteboard's contents with this.
    Set(&'a str),
    /// Empty the pasteboard.
    Clear,
    Refused(Refusal),
}

/// The policy: what `w` does under `policy`.
pub fn decide<'a>(policy: Osc52, w: &ClipboardWrite<'a>) -> Decision<'a> {
    if policy == Osc52::Off {
        return Decision::Refused(Refusal::Off);
    }
    let t = w.targets;
    if !(t.contains(Selection::Clipboard)
        || t.contains(Selection::Primary)
        || t.contains(Selection::Select))
    {
        return Decision::Refused(Refusal::NotThePasteboard);
    }
    match w.content {
        ClipboardContent::Set(bytes) => match core::str::from_utf8(bytes) {
            Ok(s) => Decision::Set(s),
            Err(_) => Decision::Refused(Refusal::NotText),
        },
        ClipboardContent::Cleared => Decision::Clear,
    }
}

/// Whether a reason has been warned about yet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Said {
    No,
    Yes,
}

/// The driver's side: the last write it acted on, and the counts.
pub struct Clips {
    seen: ClipboardWrites,
    pub written: Tally,
    pub cleared: Tally,
    pub refused: Tally,
    said: [Said; 3],
    /// The core's refusals as of the last report, for the warning that says
    /// which limit refused a write.
    core: ClipboardRefusals,
    host: Host,
}

impl Clips {
    /// Nothing acted on: the core's count as it stands now, so a write the
    /// core published before this driver existed is not replayed.
    pub fn new(term: &Term<'_>) -> Clips {
        Clips {
            seen: term.clipboard_writes(),
            written: Tally::default(),
            cleared: Tally::default(),
            refused: Tally::default(),
            said: [Said::No; 3],
            core: term.clipboard_refusals(),
            host: Host::Pasteboard,
        }
    }

    /// The same for a Wayland seat, whose refusals say Wayland.
    #[cfg(any(target_os = "linux", test))]
    pub fn for_wayland(term: &Term<'_>) -> Clips {
        Clips {
            host: Host::Wayland,
            ..Clips::new(term)
        }
    }

    /// After a read: the decision for the write that read published, or
    /// `None` when it published none. **Called after every read, not after
    /// the batch**: a write is an event, and the core keeps only the last
    /// one finished, so two reads of a batch each finishing a write would
    /// lose the first if this ran once after both. Counts what it decides,
    /// and hands back a refusal's warning the first time its reason comes.
    pub fn poll<'t>(
        &mut self,
        term: &'t Term<'_>,
        policy: Osc52,
    ) -> Option<(Decision<'t>, Option<String>)> {
        let now = term.clipboard_writes();
        if now == self.seen {
            return None;
        }
        self.seen = now;
        let w = term.clipboard()?;
        let d = decide(policy, &w);
        let warn = match d {
            Decision::Set(_) => {
                self.written.bump();
                None
            }
            Decision::Clear => {
                self.cleared.bump();
                None
            }
            Decision::Refused(why) => {
                self.refused.bump();
                self.first(why).then(|| describe(why, self.host))
            }
        };
        Some((d, warn))
    }

    /// Whether `why` is new, marking it said.
    fn first(&mut self, why: Refusal) -> bool {
        let slot = &mut self.said[match why {
            Refusal::Off => 0,
            Refusal::NotThePasteboard => 1,
            Refusal::NotText => 2,
        }];
        let new = *slot == Said::No;
        *slot = Said::Yes;
        new
    }

    /// One warning for the writes the core refused since the last call --
    /// over the budget, no selection, no area -- naming the limit and the
    /// `[clipboard]` key that raises it; `None` when it refused none.
    pub fn core_refusals(&mut self, term: &Term<'_>, budget: ClipboardBudget) -> Option<String> {
        let (before, after) = (self.core, term.clipboard_refusals());
        self.core = after;
        let mut said = Vec::new();
        if after.body > before.body {
            let len = after.body_len.index();
            said.push(format!(
                "a copy of {len} bytes refused against a {}-byte clipboard area; \
                 [clipboard] body_kb raises it",
                budget.text()
            ));
        }
        if after.no_selection > before.no_selection {
            said.push("a copy naming no selection refused".to_string());
        }
        if after.unprovisioned > before.unprovisioned {
            said.push("a copy refused: this driver keeps no clipboard area".to_string());
        }
        (!said.is_empty()).then(|| format!("clipboard: {}", said.join("; ")))
    }

    /// The five-second report's line, or `None` before any write. The
    /// Linux driver has no five-second report.
    #[cfg(any(target_os = "macos", test))]
    pub fn report(&self) -> Option<String> {
        let (w, c, r) = (
            self.written.value(),
            self.cleared.value(),
            self.refused.value(),
        );
        (w + c + r > 0).then(|| format!("clipboard: {w} written, {c} cleared, {r} refused"))
    }
}

/// The one warning for a refusal reason.
fn describe(why: Refusal, host: Host) -> String {
    match why {
        Refusal::Off => {
            "clipboard: a program's copy (OSC 52) was refused; [clipboard] osc52 = \"off\""
        }
        Refusal::NotThePasteboard => match host {
            Host::Pasteboard => {
                "clipboard: a program's copy named only the secondary selection or a cut \
                 buffer, which macOS does not have; refused"
            }
            #[cfg(any(target_os = "linux", test))]
            Host::Wayland => {
                "clipboard: a program's copy named only the secondary selection or a cut \
                 buffer, which Wayland does not have; refused"
            }
        },
        Refusal::NotText => "clipboard: a program's copy was not UTF-8 text; refused",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{BytePos, Owned, ReadLen, Selections, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 24,
        ws_col: 80,
    };

    fn write<'a>(pc: &[u8], content: ClipboardContent<'a>) -> ClipboardWrite<'a> {
        ClipboardWrite {
            targets: pc
                .iter()
                .fold(Selections::NONE, |s, &b| s.push(b))
                .resolve(),
            content,
        }
    }

    #[test]
    fn a_copy_to_the_clipboard_primary_or_select_is_written() {
        for pc in [&b"c"[..], b"p", b"s", b"", b"cq", b"s0"] {
            let w = write(pc, ClipboardContent::Set(b"hello"));
            assert_eq!(
                decide(Osc52::Write, &w),
                Decision::Set("hello"),
                "{pc:?} {:?}",
                w.targets
            );
        }
    }

    #[test]
    fn off_refuses_every_copy() {
        let w = write(b"c", ClipboardContent::Set(b"hello"));
        assert_eq!(decide(Osc52::Off, &w), Decision::Refused(Refusal::Off));
        let w = write(b"c", ClipboardContent::Cleared);
        assert_eq!(decide(Osc52::Off, &w), Decision::Refused(Refusal::Off));
    }

    #[test]
    fn the_secondary_and_the_cut_buffers_are_not_the_pasteboard() {
        for pc in [&b"q"[..], b"0", b"7", b"q01234567"] {
            let w = write(pc, ClipboardContent::Set(b"x"));
            assert_eq!(
                decide(Osc52::Write, &w),
                Decision::Refused(Refusal::NotThePasteboard),
                "{pc:?}"
            );
        }
    }

    #[test]
    fn octets_that_are_not_utf8_are_refused() {
        let w = write(b"c", ClipboardContent::Set(b"\xff\xfe"));
        assert_eq!(
            decide(Osc52::Write, &w),
            Decision::Refused(Refusal::NotText)
        );
    }

    #[test]
    fn a_clear_empties_the_pasteboard() {
        let w = write(b"c", ClipboardContent::Cleared);
        assert_eq!(decide(Osc52::Write, &w), Decision::Clear);
    }

    /// **The driver acts on every write, read by read**, which is why the
    /// poll runs inside the feed loop: two reads, each finishing a copy, fed
    /// through `reads::feed` with the poll after each, put both on the
    /// pasteboard in order -- polled once after the batch, the first would
    /// be lost, the core keeping only the last finished.
    #[test]
    fn two_reads_each_finishing_a_copy_are_both_acted_on() {
        let mut s = Owned::new(&WS).with_clipboard(ClipboardBudget::of_text(64));
        let mut t = Term::new(s.storage(), &WS);
        let mut clips = Clips::new(&t);
        let mut board = Vec::new();
        let reads: [&[u8]; 2] = [b"\x1b]52;c;Zmlyc3Q=\x07", b"\x1b]52;c;c2Vjb25k\x07"];
        crate::reads::feed(
            &mut t,
            reads,
            |_| {},
            |t| {
                if let Some((Decision::Set(s), _)) = clips.poll(t, Osc52::Write) {
                    board.push(s.to_string());
                }
            },
        );
        assert_eq!(board, ["first", "second"]);
        assert_eq!(clips.written.value(), 2);
        assert_eq!(
            clips.report().as_deref(),
            Some("clipboard: 2 written, 0 cleared, 0 refused")
        );
    }

    #[test]
    fn a_refusal_warns_once_a_reason() {
        let mut s = Owned::new(&WS).with_clipboard(ClipboardBudget::of_text(64));
        let mut t = Term::new(s.storage(), &WS);
        let mut clips = Clips::new(&t);
        let mut warned = Vec::new();
        for _ in 0..3 {
            let b = b"\x1b]52;c;aGk=\x07";
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
            if let Some((d, w)) = clips.poll(&t, Osc52::Off) {
                assert_eq!(d, Decision::Refused(Refusal::Off));
                warned.extend(w);
            }
        }
        assert_eq!(warned.len(), 1, "one warning for three refusals");
        assert_eq!(clips.refused.value(), 3, "and all three counted");
    }

    #[test]
    fn a_copy_published_before_the_driver_is_not_replayed() {
        let mut s = Owned::new(&WS).with_clipboard(ClipboardBudget::of_text(64));
        let mut t = Term::new(s.storage(), &WS);
        let b = b"\x1b]52;c;aGk=\x07";
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        let mut clips = Clips::new(&t);
        assert_eq!(clips.poll(&t, Osc52::Write), None);
    }

    /// **On Wayland each selection named is its own**: `c` the clipboard,
    /// `p` the primary, and `s` -- an empty `Pc` too, ctlseqs' `s 0` --
    /// SELECT, the primary while `?1041` is reset and the clipboard while
    /// it is set, as xterm's selectToClipboard (Jev, XTERM, 0.96/0.94).
    #[test]
    fn on_wayland_each_selection_named_is_its_own() {
        use Boards::{Both, Clipboard as C, Primary as P};
        for (pc, reset, set) in [
            (&b"c"[..], C, C),
            (b"p", P, P),
            (b"s", P, C),
            (b"", P, C),
            (b"cp", Both, Both),
            (b"ps", P, Both),
            (b"cs", Both, C),
            (b"q0s", P, C),
        ] {
            let w = write(pc, ClipboardContent::Set(b"hi"));
            assert_eq!(boards(w.targets, Board::Primary), Some(reset), "{pc:?}");
            assert_eq!(
                boards(w.targets, Board::Clipboard),
                Some(set),
                "{pc:?} ?1041"
            );
        }
        let w = write(b"q7", ClipboardContent::Set(b"hi"));
        assert_eq!(boards(w.targets, Board::Primary), None);
        assert_eq!(
            Both.each().collect::<Vec<_>>(),
            [Board::Clipboard, Board::Primary]
        );
        assert_eq!(P.each().collect::<Vec<_>>(), [Board::Primary]);
    }

    /// **`?1041` is what SELECT names**: the primary at power-on, the
    /// clipboard once set, the primary again once reset.
    #[test]
    fn select_is_the_primary_until_1041() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        assert_eq!(select_board(&t), Board::Primary);
        for (b, want) in [
            (&b"\x1b[?1041h"[..], Board::Clipboard),
            (b"\x1b[?1041l", Board::Primary),
        ] {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
            assert_eq!(select_board(&t), want);
        }
    }

    /// **A refusal says which host refused**: Wayland has no secondary
    /// selection or cut buffers, and its warning says so, not macOS's.
    #[test]
    fn a_wayland_refusal_names_wayland() {
        let mut s = Owned::new(&WS).with_clipboard(ClipboardBudget::of_text(64));
        let mut t = Term::new(s.storage(), &WS);
        let mut clips = Clips::for_wayland(&t);
        let b = b"\x1b]52;q;aGk=\x07";
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        let (d, warn) = clips.poll(&t, Osc52::Write).expect("a write");
        assert_eq!(d, Decision::Refused(Refusal::NotThePasteboard));
        let warn = warn.expect("said once");
        assert!(
            warn.contains("Wayland") && !warn.contains("macOS"),
            "{warn}"
        );
    }

    /// **The Linux driver polls as the macOS pump does**, after every read
    /// and inside the feed's per-read callback, over a core given an area.
    /// The driver is Wayland-bound and no test drives it, so this holds its
    /// source, as `the_clipboard_is_polled_after_every_read` holds the
    /// macOS pump's.
    #[test]
    fn the_linux_driver_polls_after_every_read() {
        const SRC: &str = include_str!("linux.rs");
        // Spelled apart, so this test's own text is not what it finds.
        const FEED_FN: &str = concat!("    fn feed(&mut self, ", "bytes: &[u8]) {");
        const FEED: &str = concat!("crate::reads::", "feed(");
        const POLL: &str = concat!("clips.poll(", "t, osc52)");
        const AREA: &str = concat!(".with_clipboard(", "cfg.clipboard.budget)");
        assert_eq!(SRC.matches(FEED_FN).count(), 1, "one feed");
        let body = SRC.split(FEED_FN).nth(1).expect("the feed");
        let body = &body[..body.find("\n    }\n").expect("the feed ends")];
        assert_eq!(body.matches(FEED).count(), 1, "it feeds once");
        let call = body.split(FEED).nth(1).expect("the feed call");
        let call = &call[..call.find("\n        );").expect("the call ends")];
        assert!(call.contains(POLL), "the poll is not the per-read callback");
        assert_eq!(SRC.matches(POLL).count(), 1, "and nowhere else");
        assert_eq!(SRC.matches(AREA).count(), 1, "the core has an area");
    }

    #[test]
    fn the_core_refusals_name_the_limit() {
        let budget = ClipboardBudget::of_text(4);
        let mut s = Owned::new(&WS).with_clipboard(budget);
        let mut t = Term::new(s.storage(), &WS);
        let mut clips = Clips::new(&t);
        let b = b"\x1b]52;c;aGVsbG8=\x07";
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        let line = clips.core_refusals(&t, budget).expect("one refusal");
        assert!(
            line.contains("5 bytes") && line.contains("4-byte") && line.contains("body_kb"),
            "{line}"
        );
        assert_eq!(clips.core_refusals(&t, budget), None, "said once");
    }
}
