//! **A glyph map walk over the screen it last walked, under the same
//! [`WalkKey`], changes nothing**: the cell path skips it on a frame where
//! only the cursor, its blink animation or its trail moved (`baud-app`'s
//! `Screens`), so the walk it skips must be one that would have left the
//! map, its generation and the candidates as they were.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, GlyphMap, PxSize, Renderer, Resolved, Stack, StyleStamp, Theme, WalkKey};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 40,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

#[test]
fn a_walk_over_the_last_walked_screen_changes_nothing() {
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
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // Ligature pairs, so the walk lists candidates; colours and a bold
    // run, so the table is more than the default.
    feed(
        &mut t,
        b"a -> b => c != d\r\n\x1b[1;31mbold red\x1b[0m e\xcc\x81 \x1b[38;5;200mpink\x1b[0m",
    );
    let mut backing = GlyphMap::backing();
    let mut map = GlyphMap::over(&mut backing);
    let mut styles = vec![Resolved::default(); t.styles().len()];
    let mut stamp = StyleStamp::default();
    let mut candidates = Vec::new();
    let key = |map: &GlyphMap<'_>, r: &Renderer<'_, '_>, t: &Term<'_>, stamp| WalkKey {
        table: stamp,
        map: map.generation(),
        grown: r.atlas_stats().grown,
        clusters: t.cluster_epoch(),
        back: Rows::ZERO,
        cols: t.viewport().cols(),
        rows: t.viewport().rows(),
    };
    r.sync_styles(&t, &mut styles, &mut stamp);
    map.ensure(&t, t.viewport(), &styles, &mut r, &mut candidates);
    let walked = key(&map, &r, &t, stamp);
    let (entries, listed) = (map.entries().to_vec(), candidates.clone());
    assert!(
        !listed.is_empty(),
        "the premise: the screen lists candidates"
    );
    // The cursor moves, which the walk does not read: the key is the same.
    feed(&mut t, b"\x1b[2;3H");
    r.sync_styles(&t, &mut styles, &mut stamp);
    assert_eq!(
        key(&map, &r, &t, stamp),
        walked,
        "nothing the walk reads moved"
    );
    map.ensure(&t, t.viewport(), &styles, &mut r, &mut candidates);
    assert_eq!(map.generation(), walked.map, "the walk changed no entry");
    assert!(map.entries() == entries.as_slice(), "nor any slot");
    assert_eq!(candidates, listed, "and listed the same candidates");
    // A rendition written moves the table's stamp, so the key differs and
    // the frame walks.
    feed(&mut t, b"\x1b[38;2;1;2;3mX");
    r.sync_styles(&t, &mut styles, &mut stamp);
    assert_ne!(
        key(&map, &r, &t, stamp),
        walked,
        "a new rendition is a new key"
    );
}
