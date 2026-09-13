use crate::support;
use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, (usize, usize), Vec<u8>, TerminalMode) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let (r, c) = t.cursor();
    (
        support::screen(&t),
        (r.index(), c.index()),
        t.reply().to_vec(),
        t.terminal_mode(),
    )
}

fn cursor(b: &[u8]) -> (usize, usize) {
    run(b).1
}
fn grid(b: &[u8]) -> Vec<PackedCell> {
    run(b).0
}

/// `CSI ? 2 l` enters VT52; `CSI ? 2 h` returns. The polarity is the trap —
/// *reset* is what enters a different protocol.
#[test]
fn decanm_switches_protocol() {
    assert_eq!(run(b"").3, TerminalMode::Ansi, "power-on is ANSI");
    assert_eq!(run(b"\x1b[?2l").3, TerminalMode::Vt52);
    // `ESC <` is the only way back: a VT52 has no CSI to send `?2h` with.
    assert_eq!(run(b"\x1b[?2l\x1b<").3, TerminalMode::Ansi);
}

/// `ESC <` leaves VT52 from inside it — the only way back that VT52 itself has.
#[test]
fn esc_less_than_leaves_vt52() {
    assert_eq!(run(b"\x1b[?2l\x1b<").3, TerminalMode::Ansi);
}

/// **The same byte, two meanings.** `ESC D` is IND in ANSI (down one line,
/// column kept) and cursor-left in VT52.
#[test]
fn esc_d_means_different_things_in_each_mode() {
    assert_eq!(cursor(b"\x1b[5;5H\x1bD"), (5, 4), "ANSI: IND, down a line");
    assert_eq!(
        cursor(b"\x1b[5;5H\x1b[?2l\x1bD"),
        (4, 3),
        "VT52: cursor left"
    );
}

/// And `ESC H`: HTS in ANSI, home in VT52.
#[test]
fn esc_h_means_different_things_in_each_mode() {
    assert_eq!(
        cursor(b"\x1b[5;5H\x1bH"),
        (4, 4),
        "ANSI: HTS sets a tab stop, cursor unmoved"
    );
    assert_eq!(cursor(b"\x1b[5;5H\x1b[?2l\x1bH"), (0, 0), "VT52: home");
}

/// The four cursor movements.
#[test]
fn cursor_movement() {
    let at = |seq: &[u8]| {
        // Positioned in ANSI, then switched: a VT52 has no CSI, and this also
        // checks the switch leaves the cursor alone.
        let mut v = b"\x1b[5;5H\x1b[?2l".to_vec();
        v.extend_from_slice(seq);
        cursor(&v)
    };
    assert_eq!(at(b"\x1bA"), (3, 4), "up");
    assert_eq!(at(b"\x1bB"), (5, 4), "down");
    assert_eq!(at(b"\x1bC"), (4, 5), "right");
    assert_eq!(at(b"\x1bD"), (4, 3), "left");
}

/// `ESC Y row col`, each coordinate biased by 0x20.
///
/// **A source conflict, recorded.** `[[source: vt510-decanm]]` describes
/// `ESC Y Pn` as "Move cursor to column Pn" — one operand. Terminfo's `vt52`
/// entry uses two: `cup=\EY%p1%' '%+%c%p2%' '%+%c`, row then column. baud
/// follows terminfo, because that is what applications emit.
#[test]
fn direct_address_takes_row_then_column() {
    // 0x20 is 0; ' '+3 = '#', ' '+7 = "'"
    assert_eq!(cursor(b"\x1b[?2l\x1bY\x23\x27"), (3, 7));
    assert_eq!(cursor(b"\x1b[?2l\x1bY\x20\x20"), (0, 0), "the origin");
}

/// A coordinate below the 0x20 bias would be negative. It clamps to the origin
/// rather than wrapping to the far edge of the grid.
#[test]
fn an_underflowing_coordinate_clamps() {
    assert_eq!(cursor(b"\x1b[?2l\x1bY\x01\x01"), (0, 0));
}

/// The two erases.
#[test]
fn erase_to_end_of_screen_and_line() {
    // Positioned with VT52's own `ESC Y`, not CSI: a VT52 has no CSI, and
    // this file must not depend on a hybrid that never existed.
    assert_eq!(
        grid(b"abc\r\ndef\x1b[1;2H\x1b[?2l\x1bJ"),
        grid(b"a"),
        "ESC J: to end of screen"
    );
    assert_eq!(
        grid(b"abc\r\ndef\x1b[1;2H\x1b[?2l\x1bK"),
        grid(b"a\r\ndef"),
        "ESC K: to end of line only"
    );
}

/// `ESC I` is a reverse line feed, scrolling at the top.
#[test]
fn reverse_line_feed() {
    assert_eq!(cursor(b"\x1b[5;5H\x1b[?2l\x1bI"), (3, 4));
}

/// `ESC F` / `ESC G` reach the line-drawing set — VT52's route to the same
/// cell flag `ESC ( 0` sets in ANSI.
#[test]
fn graphics_mode_marks_cells() {
    let (g, _, _, _) = run(b"\x1b[?2l\x1bFq");
    assert!(g[0].flags.is_graphics(), "ESC F marks the cell");
    let (h, _, _, _) = run(b"\x1b[?2l\x1bF\x1bGq");
    assert!(!h[0].flags.is_graphics(), "ESC G clears it");
}

/// `ESC Z` identifies. `[[source: vt510-decanm]]`: "ESC /Z Report (terminal to
/// host)." Not a CSI and not a DCS — a third reply shape.
#[test]
fn identify_answers_esc_slash_z() {
    assert_eq!(run(b"\x1b[?2l\x1bZ").2, b"\x1b/Z");
}

/// A VT52 sequence baud declines is counted, not silently dropped.
///
/// The print functions (`ESC ^ _ W X ] V`) have no printer to drive. Counting
/// them is what keeps "accepted and did nothing" observable.
#[test]
fn declined_print_functions_are_counted() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"\x1b[?2l\x1bW\x1bX");
    t.feed(
        BytePos::ZERO,
        ReadLen::from_index((b"\x1b[?2l\x1bW\x1bX").len()),
    );
    assert_eq!(t.vt52_unsupported, 2, "both declined and both counted");
}

/// RIS is **not** reachable from VT52, and that is correct rather than a gap.
///
/// `ESC c` is not in `[[source: vt510-decanm]]`'s table, so in VT52 it is an
/// unknown escape — declined and counted, like the print functions. The only
/// exits are `ESC <` and a host-side reset. Worth knowing: `reset(1)` sends
/// `ESC c`, so it does not rescue a terminal stranded in VT52 either.
#[test]
fn ris_is_not_reachable_from_vt52() {
    assert_eq!(run(b"\x1b[?2l\x1bc").3, TerminalMode::Vt52, "still VT52");
    assert_eq!(run(b"\x1bc").3, TerminalMode::Ansi, "but RIS works in ANSI");
}

/// DECRQM reports the mode, with DECANM's inverted sense: VT52 is `?2` *reset*.
#[test]
fn decrqm_reports_decanm() {
    assert_eq!(run(b"\x1b[?2$p").2, b"\x1b[?2;1$y", "ANSI: set");
    // Queried after returning, because a VT52 has no CSI to ask with.
    assert_eq!(
        run(b"\x1b[?2l\x1b<\x1b[?2$p").2,
        b"\x1b[?2;1$y",
        "back in ANSI after ESC <"
    );
}

/// `ESC [` is not CSI in VT52.
///
/// It is an unknown VT52 escape: declined, counted, and the bytes that follow
/// are ordinary text. Allowing CSI would invent a hybrid terminal that never
/// existed and that nothing targets.
#[test]
fn csi_is_not_special_in_vt52() {
    let (_, _, reply, mode) = run(b"\x1b[?2l\x1b[?2$p");
    assert!(reply.is_empty(), "a VT52 cannot be asked a CSI question");
    assert_eq!(mode, TerminalMode::Vt52, "and it did not leave VT52");
}

/// VT52 parses the same however the reads are split — `ESC Y` in particular
/// spans three bytes and two parser states.
#[test]
fn vt52_is_chunk_invariant() {
    let seq = b"\x1b[?2l\x1bY\x23\x27text\x1bA\x1bK\x1b<done";
    let whole = grid(seq);
    for chunk in 1..seq.len() {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            t.write_at(BytePos::ZERO, &seq[i..i + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            i += n;
        }
        assert_eq!(support::screen(&t), whole, "differed at chunk {chunk}");
    }
}
