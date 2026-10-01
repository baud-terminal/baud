//! Text a rendition hides: `SGR 5` in its off phase, and `SGR 8`.
//!
//! Hidden text draws its cell's background and nothing of its own -- no
//! glyph, no underline, no strike -- and the cursor and the selection over it
//! are drawn as over any other cell. Drawing the glyph "in the background's
//! colour" is not the same thing once backgrounds are translucent: the glyph
//! lands on top of the background it is meant to vanish into, at a different
//! alpha, and its silhouette shows. So every comparison here is against the
//! frame of spaces in the same background, under glass.
//!
//! And a frame has to know blinking text is on screen, for the whole screen:
//! the driver repaints on a phase flip only when the last frame saw a
//! blinking cell, so an idle screen with nothing blinking builds no frame.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows};
use baud_font::{Face, Px};
use baud_render::{
    Blend, Blink, Blinking, Damage, GlyphMap, MaySwap, Overlay, PxSize, Renderer, Resolved, Scene,
    Stack, StyleStamp, Theme,
};
use support::{Canvas, WS, font, paint};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Glass: every background at an alpha of 56, which is where a glyph drawn
/// in its background's colour stops vanishing.
fn glass() -> Theme {
    Theme {
        bg_alpha: Chan::from_u8(56),
        ..Theme::dark()
    }
}

fn renderer<'f>(faces: &'f [Face<'f>], theme: Theme) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// One full CPU frame of `bytes` in `phase`, as a digest.
fn frame(bytes: &[u8], theme: Theme, phase: Blink) -> u64 {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, theme);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    r.set_blink(phase);
    let mut c = Canvas::for_renderer(&r);
    paint(&mut r, &mut t, &mut c, Damage::Full);
    c.digest()
}

/// Blinking text in its off phase is the frame of spaces in its background,
/// over the default background and over one the application set, with an
/// underline and a strike on it; and in its on phase it is not.
#[test]
fn hidden_blinking_text_is_the_frame_of_its_spaces() {
    for (text, spaces) in [
        // A box-drawing character is baud's own mask, emitted apart from a
        // shaped run, and `e` with a combining acute is one cell of two
        // codepoints: both have to hide.
        (
            "\x1b[5;31mBLINK\u{2500}e\u{301}\x1b[0m\x1b[?25l".as_bytes(),
            &b"       \x1b[?25l"[..],
        ),
        (
            b"\x1b[5;4;9;31;44mBLINK\x1b[0m\x1b[?25l",
            b"\x1b[44m     \x1b[0m\x1b[?25l",
        ),
    ] {
        let off = frame(text, glass(), Blink::Hidden);
        assert_eq!(
            off,
            frame(spaces, glass(), Blink::Hidden),
            "blinking text's off phase left a mark: {}",
            String::from_utf8_lossy(text)
        );
        assert_ne!(
            off,
            frame(text, glass(), Blink::Shown),
            "the on phase draws nothing"
        );
    }
}

/// `SGR 8` is the frame of spaces in its background, the same way.
#[test]
fn concealed_text_is_the_frame_of_its_spaces() {
    for (text, spaces) in [
        (
            "\x1b[8;31mhello\u{2500}e\u{301}\x1b[0m\x1b[?25l".as_bytes(),
            &b"       \x1b[?25l"[..],
        ),
        (
            b"\x1b[8;4;9;31;44mhello\x1b[0m\x1b[?25l",
            b"\x1b[44m     \x1b[0m\x1b[?25l",
        ),
    ] {
        for theme in [Theme::dark(), glass()] {
            assert_eq!(
                frame(text, theme.clone(), Blink::Shown),
                frame(spaces, theme.clone(), Blink::Shown),
                "concealed text left a mark at alpha {}: {}",
                theme.bg_alpha.get(),
                String::from_utf8_lossy(text)
            );
        }
    }
}

/// A block cursor over concealed text is drawn: it is the cell's colours
/// swapped, and the concealed glyph's colour is not the background's.
#[test]
fn a_block_cursor_over_concealed_text_is_drawn() {
    let text = b"\x1b[8mhello\x1b[0m\x1b[3D\x1b[2 q";
    let mut none = text.to_vec();
    none.extend_from_slice(b"\x1b[?25l");
    for theme in [Theme::dark(), glass()] {
        assert_ne!(
            frame(text, theme.clone(), Blink::Shown),
            frame(&none, theme.clone(), Blink::Shown),
            "the block cursor vanished over concealed text at alpha {}",
            theme.bg_alpha.get()
        );
    }
}

#[test]
fn a_blinking_row_keeps_blinking_through_incremental_frames() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, Theme::dark());
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut c = Canvas::for_renderer(&r);
    feed(&mut t, b"\x1b[5mBLINK\x1b[0m\r\n");
    paint(&mut r, &mut t, &mut c, Damage::Full);
    assert_eq!(r.blink_seen(), Blinking::Seen, "the blinking row was drawn");
    // Another row changes; the blinking one does not, so it is not
    // converted again, and it is still on screen.
    feed(&mut t, b"\x1b[4;1Hother");
    let f = paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert!(
        f.rows_dirty.value() < f.rows.value(),
        "the frame is incremental: {} of {} rows",
        f.rows_dirty.value(),
        f.rows.value()
    );
    assert_eq!(
        r.blink_seen(),
        Blinking::Seen,
        "an incremental frame forgot the blinking text still on screen"
    );
    // The control: once it is erased, nothing blinks.
    feed(&mut t, b"\x1b[1;1H\x1b[K");
    paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(r.blink_seen(), Blinking::Unseen, "nothing on screen blinks");
}

/// The cell path's answer: the rendition table resolved and the overlay
/// walked, as the app builds a frame. A rendition that blinks and names no
/// cell on screen is not blinking text on screen -- the table never frees a
/// slot, so answering from the table kept a screen repainting at every
/// phase flip for the rest of the session after one `SGR 5`.
#[test]
fn a_blinking_slot_off_screen_is_not_seen() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, Theme::dark());
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut backing = GlyphMap::backing();
    let mut map = GlyphMap::over(&mut backing);
    let mut styles = vec![Resolved::default(); t.styles().len()];
    let mut overlay = Overlay::new();
    let mut mask = vec![0u8; usize::from(WS.ws_col) * usize::from(WS.ws_row)];
    let mut records = Vec::new();
    let mut candidates = Vec::new();
    // The table as the app keeps a set's: synced from the core's log, so a
    // rendition minted after the first frame reaches the blink count by the
    // replay rather than by a full resolve.
    let mut stamp = StyleStamp::default();
    let mut seen = |t: &Term<'_>, r: &mut Renderer<'_, '_>| {
        r.sync_styles(t, &mut styles, &mut stamp);
        map.ensure(
            t.view(Rows::ZERO),
            &styles,
            MaySwap::NONE,
            r,
            &mut candidates,
        );
        overlay.ensure(
            &Scene {
                term: t,
                view: t.view(Rows::ZERO),
                styles: &styles,
                cursor: None,
                highlight: None,
                map: &map,
                candidates: &candidates,
                pan: Px::of(0),
            },
            r,
            &mut mask,
            &mut records,
        );
        r.blink_seen()
    };
    feed(&mut t, b"plain");
    assert_eq!(seen(&t, &mut r), Blinking::Unseen, "nothing blinks yet");
    feed(&mut t, b"\x1b[5mBLINK\x1b[0m");
    assert_eq!(
        seen(&t, &mut r),
        Blinking::Seen,
        "the blinking cells are on screen"
    );
    feed(&mut t, b"\x1b[2J");
    assert_eq!(
        seen(&t, &mut r),
        Blinking::Unseen,
        "no cell on screen blinks, and the table still holds the rendition"
    );
}

/// The cell path's records -- a mark, a glyph inside a ligature window --
/// are the cell's text too, and a hidden cell's are not drawn. The walk
/// still masks the cell, so the shader draws nothing there either.
#[test]
fn a_hidden_cells_records_are_not_drawn() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, Theme::dark());
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut backing = GlyphMap::backing();
    let mut map = GlyphMap::over(&mut backing);
    let mut styles = vec![Resolved::default(); t.styles().len()];
    let mut overlay = Overlay::new();
    let mut mask = vec![0u8; usize::from(WS.ws_col) * usize::from(WS.ws_row)];
    let mut records = Vec::new();
    let mut candidates = Vec::new();
    let mut drawn = |bytes: &[u8], phase: Blink, r: &mut Renderer<'_, '_>| {
        feed(&mut t, b"\x1b[H\x1b[2J");
        feed(&mut t, bytes);
        r.set_blink(phase);
        r.resolve_styles_into(&t, &mut styles);
        map.ensure(
            t.view(Rows::ZERO),
            &styles,
            MaySwap::NONE,
            r,
            &mut candidates,
        );
        overlay.ensure(
            &Scene {
                term: &t,
                view: t.view(Rows::ZERO),
                styles: &styles,
                cursor: None,
                highlight: None,
                map: &map,
                candidates: &candidates,
                pan: Px::of(0),
            },
            r,
            &mut mask,
            &mut records,
        );
        records.len()
    };
    let text = "e\u{301} a -> b";
    // The control: shown, the mark and the ligature are drawn by records.
    assert!(
        drawn(format!("\x1b[5m{text}").as_bytes(), Blink::Shown, &mut r) > 0,
        "the mark and the ligature went to the records"
    );
    assert_eq!(
        drawn(format!("\x1b[5m{text}").as_bytes(), Blink::Hidden, &mut r),
        0,
        "blinking text's records were drawn in its off phase"
    );
    assert_eq!(
        drawn(format!("\x1b[8m{text}").as_bytes(), Blink::Shown, &mut r),
        0,
        "concealed text's records were drawn"
    );
}
