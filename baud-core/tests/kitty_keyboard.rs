#![deny(dead_code)]
//! `[[source: kitty-keyboard]]`'s progressive enhancement: the negotiation is
//! live and the query is **answered**.
//!
//! The protocol's support detection makes it all-or-nothing. From the
//! specification: an application queries the enhancement status and then asks
//! for primary device attributes, and "if an answer for the device attributes is
//! received without getting back an answer for the progressive enhancement the
//! terminal does not support this protocol". **So a reply is the claim** -- and
//! it is made only now that `Term::key` honours all five enhancements
//! (disambiguate, event types, alternate keys, all-keys, associated text) for
//! every key it can represent: text keys, the functional keys, the keypad, and
//! the Private Use Area keys the individual modifiers live in. The
//! specification's own words are "It does not make sense to implement only a
//! subset", and none is skipped.
//!
//! `CSI ? u` now answers `CSI ? flags u` with the active screen's flags. The
//! query stayed silent through every earlier increment -- answering while the
//! encoding was incomplete would have been the "wrong-shaped reply" this
//! project refuses -- which is why turning it on is one deliberate change here
//! rather than a side effect of building the encoder.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

fn feed(t: &mut Term, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn drawn(t: &Term) -> bool {
    let (_, rows) = support::CORPUS.geometry();
    (0..rows.index()).any(|y| {
        t.row(Row::from_index(y))
            .iter()
            .any(|c| c.content != Content::EMPTY)
    })
}

/// **The query is answered with the current flags, `CSI ? flags u`.** This is
/// the support claim; the reply reflects whatever the active screen's stack
/// holds, so an application reads back exactly what it set.
#[test]
fn the_query_is_answered_with_the_current_flags() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);

    // No enhancements set yet: the honest answer is zero, but it is answered.
    feed(&mut t, b"\x1b[?u");
    assert!(!drawn(&t), "the query is consumed, not drawn");
    assert_eq!(t.reply(), b"\x1b[?0u", "answered, with no flags set");
    assert_eq!(t.unsupported, 0, "answered, not refused");

    // After setting disambiguate + alternate keys (5), the query reflects them.
    t.clear_reply();
    feed(&mut t, b"\x1b[=5u\x1b[?u");
    assert_eq!(t.reply(), b"\x1b[?5u");
}

/// Set, push and pop are consumed and **not** counted as unsupported: they do
/// real work now (maintain the stack) rather than being refused.
#[test]
fn set_push_pop_are_consumed_not_counted() {
    for bytes in [b"\x1b[=1;1u".as_slice(), b"\x1b[>1u", b"\x1b[<1u"] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        feed(&mut t, bytes);

        assert!(!drawn(&t), "the request must be consumed, not drawn");
        assert_eq!(t.unsupported, 0, "a stack op is acted on, not refused");
        assert!(t.reply().is_empty(), "no reply: support is not claimed");
    }
}

/// `CSI = flags ; mode u` sets the active flags, and the undefined bits are
/// dropped on the way in.
#[test]
fn set_moves_the_active_flags() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);

    feed(&mut t, b"\x1b[=5u"); // disambiguate + alternate keys
    assert_eq!(t.kitty_flags().bits(), 0b101);

    // Mode 2 ORs in another bit; mode 3 clears one.
    feed(&mut t, b"\x1b[=2;2u");
    assert_eq!(t.kitty_flags().bits(), 0b111);
    feed(&mut t, b"\x1b[=1;3u");
    assert_eq!(t.kitty_flags().bits(), 0b110);
}

/// Push sets the flags and pop restores the level below it; popping the last
/// entry resets, which the spec makes a special case.
#[test]
fn push_and_pop_move_the_active_flags() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);

    feed(&mut t, b"\x1b[=1u"); // base: disambiguate
    feed(&mut t, b"\x1b[>31u"); // push, now all five
    assert_eq!(t.kitty_flags(), KittyFlags::ALL);

    feed(&mut t, b"\x1b[>2u"); // push again, now event-types only
    assert_eq!(t.kitty_flags().bits(), 0b10);

    feed(&mut t, b"\x1b[<1u"); // pop one, back to ALL
    assert_eq!(t.kitty_flags(), KittyFlags::ALL);

    feed(&mut t, b"\x1b[<1u"); // pop the last, empties -> reset
    assert!(t.kitty_flags().is_none());
}

/// **The main and alternate screens keep independent stacks.** `[[source:
/// kitty-keyboard]]` requires it so an editor changes the mode in the alternate
/// screen without disturbing the shell's. Entering the alternate screen gives a
/// fresh stack; leaving restores the primary's.
#[test]
fn the_alternate_screen_has_its_own_stack() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);

    feed(&mut t, b"\x1b[=5u"); // main screen: flags 5
    assert_eq!(t.kitty_flags().bits(), 0b101);

    feed(&mut t, b"\x1b[?1049h"); // enter alternate screen
    assert!(
        t.kitty_flags().is_none(),
        "the alternate screen starts with its own empty stack"
    );

    feed(&mut t, b"\x1b[=2u"); // alternate screen: flags 2
    assert_eq!(t.kitty_flags().bits(), 0b10);

    feed(&mut t, b"\x1b[?1049l"); // leave
    assert_eq!(
        t.kitty_flags().bits(),
        0b101,
        "the primary's flags are restored, untouched by the alternate screen"
    );
}

/// The forms carry markers, so none of them collides with SCORC.
///
/// **This is the collision `[[source: ctlseqs]]` warns about, resolved by the
/// other protocol.** Its note on fixterms' format says "applications may confuse
/// it with `CSI u` (restore-cursor)", and that is a real hazard for a bare
/// `CSI u`. Every kitty form carries `=`, `?`, `>` or `<` before the parameters,
/// so the parser separates them on the marker alone and SCORC keeps working.
#[test]
fn the_stack_operations_do_not_disturb_restore_cursor() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);

    // Put the cursor somewhere, save it, move, then run every kitty form
    // before restoring. SCORC must still return to the saved position.
    feed(&mut t, b"\x1b[5;10H\x1b[s\x1b[20;40H");
    for bytes in [
        b"\x1b[=1;1u".as_slice(),
        b"\x1b[?u",
        b"\x1b[>1u",
        b"\x1b[<1u",
    ] {
        feed(&mut t, bytes);
    }
    feed(&mut t, b"\x1b[u");

    let (row, col) = t.cursor();
    assert_eq!(
        (row.index(), col.index()),
        (4, 9),
        "SCORC must restore the saved position with the kitty forms in between"
    );
}
