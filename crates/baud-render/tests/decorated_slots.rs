//! **A resolved copy of the rendition table says whether a slot of it can
//! draw a decoration** ([`StyleStamp::decorations`]), and the cell path
//! leaves its shape pass undrawn on that word, so the count may run over
//! the slots cells name and never under them.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Decorations, PxSize, Renderer, Resolved, Stack, StyleStamp, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 40,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn slot_at(t: &Term<'_>, row: usize, col: usize) -> usize {
    t.viewport().row(Row::from_index(row))[Col::at_index(col).index()]
        .style
        .index()
}

/// The count over one copy synced from two tables in turn. The second
/// table's first sync resolves only the slots its log names and leaves
/// the first table's underline where it was, in a slot no cell of the
/// second names; when the second table later writes an undecorated
/// rendition into that slot, the copy must still say a decoration may be
/// drawn while the second table's own underline is on screen.
#[test]
fn a_copy_counts_the_decorated_slots_it_holds() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned_a = Owned::new(&WS);
    let mut a = Term::new(owned_a.storage(), &WS);
    let mut out = vec![Resolved::default(); a.styles().one().len()];
    let mut stamp = StyleStamp::default();
    assert_eq!(
        stamp.decorations(),
        Decorations::Present,
        "a copy never synced may hold any slot"
    );
    feed(&mut a, b"plain \x1b[1;31mbold red\x1b[0m");
    r.sync_styles(&a, &mut out, &mut stamp);
    assert_eq!(
        stamp.decorations(),
        Decorations::Absent,
        "no slot is decorated"
    );
    // Each decoration, minted after a sync, is counted by the replay.
    for sgr in ["4", "9", "21", "4:3", "53", "63", "51", "52", "64"] {
        feed(&mut a, format!("\x1b[2;1H\x1b[{sgr}mx\x1b[0m").as_bytes());
        r.sync_styles(&a, &mut out, &mut stamp);
        assert_eq!(
            stamp.decorations(),
            Decorations::Present,
            "SGR {sgr} counted"
        );
    }
    let underlined = {
        feed(&mut a, b"\x1b[3;1H\x1b[4mu\x1b[0m");
        r.sync_styles(&a, &mut out, &mut stamp);
        slot_at(&a, 2, 0)
    };
    // A new theme resolves every slot again, and counts them again.
    r.set_theme(Theme::dark());
    r.sync_styles(&a, &mut out, &mut stamp);
    assert_eq!(
        stamp.decorations(),
        Decorations::Present,
        "after a full pass"
    );

    // The second table, through the same copy.
    let mut owned_b = Owned::new(&WS);
    let mut b = Term::new(owned_b.storage(), &WS);
    assert_eq!(
        b.styles().one().len(),
        out.len(),
        "the premise: one table size"
    );
    feed(&mut b, b"\x1b[4;32mown\x1b[0m");
    let own = slot_at(&b, 0, 0);
    assert_ne!(own, underlined, "the premise: two slots");
    r.sync_styles(&b, &mut out, &mut stamp);
    assert_eq!(
        stamp.decorations(),
        Decorations::Present,
        "its own underline"
    );
    // Truecolour renditions, one a write, each synced, until one lands in
    // the slot the first table's underline left.
    let mut landed = false;
    for i in 0..20_000u32 {
        let (red, green, blue) = (i % 256, (i / 256) % 256, 7);
        feed(
            &mut b,
            format!("\x1b[2;1H\x1b[38;2;{red};{green};{blue}my\x1b[0m").as_bytes(),
        );
        r.sync_styles(&b, &mut out, &mut stamp);
        assert_eq!(
            slot_at(&b, 0, 0),
            own,
            "the premise: the underline stays on screen"
        );
        if slot_at(&b, 1, 0) == underlined {
            landed = true;
            break;
        }
    }
    assert!(landed, "the premise: a write reached the stale slot");
    assert!(
        !out[underlined].shapes.any(baud_render::Shapes::DECORATIONS),
        "the premise: the slot holds an undecorated rendition now"
    );
    assert_eq!(
        stamp.decorations(),
        Decorations::Present,
        "the second table's underline is still counted"
    );
}
