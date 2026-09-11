#![deny(dead_code)]
//! `[[source: kitty-keyboard]]`'s progressive enhancement: **refused, consumed
//! and counted**, and this file is why that is the right answer rather than a
//! gap nobody got to.
//!
//! The protocol's own support detection makes it all-or-nothing. From the
//! specification: an application queries the enhancement status and then asks
//! for primary device attributes, and "if an answer for the device attributes is
//! received **without getting back an answer for the progressive enhancement the
//! terminal does not support this protocol**".
//!
//! **So a reply is the claim.** Answering `CSI ? u` with `CSI ? 0 u` would be
//! truthful about the flags and a lie about the protocol: the application would
//! conclude the enhancements are available and stop sending input the legacy way.
//! That is the failure `docs/vt-coverage.md` rates worst -- answering a query
//! with a shape the application did not ask for -- and it is why `DA`
//! under-claims as a VT102 and why `DECRQM` answers `0` rather than `2`.
//!
//! The specification says the same thing from its own side: implementations are
//! "strongly encouraged to implement all progressive enhancements. It does not
//! make sense to implement only a subset."
//!
//! So silence is correct until all five enhancements exist, and the useful thing
//! a test can pin is that the silence is **deliberate**: the bytes are consumed
//! rather than drawn, and the requests are counted rather than dropped, so an
//! embedder can see them arriving. Same contract as `clipboard_reads_refused`.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

/// The four stack operations, spelled as the specification spells them.
const OPS: [(&str, &[u8]); 4] = [
    ("CSI = flags ; mode u -- set", b"\x1b[=1;1u"),
    ("CSI ? u -- query", b"\x1b[?u"),
    ("CSI > flags u -- push", b"\x1b[>1u"),
    ("CSI < number u -- pop", b"\x1b[<1u"),
];

/// **Each request is consumed, counted, and left unanswered.**
///
/// Three properties, and the third is the one that makes the refusal safe:
/// answering would claim support for enhancements baud does not implement.
#[test]
fn the_progressive_enhancements_are_refused_and_counted() {
    for (name, bytes) in OPS {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));

        let row = t.row(Row::from_index(0));
        assert!(
            row.iter().all(|c| c.content == Content::EMPTY),
            "{name}: the request must be consumed, not drawn -- a sequence \
             rendered as its own parameter bytes is the withdrawal this \
             project already had to make once, when 17.7M cells of btop \
             turned out to be escape sequences blitted to the screen"
        );
        assert_eq!(
            t.unsupported, 1,
            "{name}: counted, so an embedder can see the request arrive"
        );
        assert!(
            t.reply().is_empty(),
            "{name}: answering claims support for the whole protocol -- the \
             specification's detection mechanism is the presence of a reply"
        );
    }
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
    let setup = b"\x1b[5;10H\x1b[s\x1b[20;40H";
    t.write_at(BytePos::ZERO, setup);
    t.feed(BytePos::ZERO, ReadLen::from_index(setup.len()));
    for (_, bytes) in OPS {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }
    let restore = b"\x1b[u";
    t.write_at(BytePos::ZERO, restore);
    t.feed(BytePos::ZERO, ReadLen::from_index(restore.len()));

    let (row, col) = t.cursor();
    assert_eq!(
        (row.index(), col.index()),
        (4, 9),
        "SCORC must restore the saved position with the kitty forms in between"
    );
}
