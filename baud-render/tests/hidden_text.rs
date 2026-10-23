//! Text a rendition hides: `SGR 5` and `SGR 6` each in its own rate's off
//! phase, and `SGR 8`.
//!
//! Hidden text draws its cell's background and nothing of its own -- no
//! glyph, no underline, no strike -- and the cursor and the selection over it
//! are drawn as over any other cell. Drawing the glyph "in the background's
//! colour" is not the same thing once backgrounds are translucent: the glyph
//! lands on top of the background it is meant to vanish into, at a different
//! alpha, and its silhouette shows. So every comparison here is against the
//! frame of spaces in the same background, under glass.
//!
//! And a frame has to know blinking text is on screen, for the whole screen
//! and for each rate: the driver repaints on a rate's phase flip only when
//! the last frame saw a cell blinking at that rate, so an idle screen with
//! nothing blinking builds no frame and a slow-only one none at the rapid
//! rate.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows};
use baud_font::{Face, Px};
use baud_render::{
    Blend, Blink, BlinkPhases, Blinking, Damage, GlyphMap, Overlay, PxSize, Renderer, Resolved,
    Scene, Stack, StyleStamp, Theme,
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

/// The phases with the slow rate at `b` and the rapid one shown.
fn slow(b: Blink) -> BlinkPhases {
    BlinkPhases {
        slow: b,
        rapid: Blink::Shown,
    }
}

/// The phases with the rapid rate at `b` and the slow one shown.
fn rapid(b: Blink) -> BlinkPhases {
    BlinkPhases {
        slow: Blink::Shown,
        rapid: b,
    }
}

/// One full CPU frame of `bytes` in `phase`, as a digest.
fn frame(bytes: &[u8], theme: Theme, phase: BlinkPhases) -> u64 {
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
        let off = frame(text, glass(), slow(Blink::Hidden));
        assert_eq!(
            off,
            frame(spaces, glass(), slow(Blink::Hidden)),
            "blinking text's off phase left a mark: {}",
            String::from_utf8_lossy(text)
        );
        assert_ne!(
            off,
            frame(text, glass(), BlinkPhases::SHOWN),
            "the on phase draws nothing"
        );
    }
}

/// **Each rate hides its text only in its own off phase.** `SGR 6` is the
/// frame of its spaces in the rapid off phase and drawn in the slow one;
/// `SGR 5` the other way round. A path that read one phase for both rates
/// would hide the other rate's text at the wrong flips.
#[test]
fn each_blink_rate_hides_only_in_its_own_off_phase() {
    let spaces = &b"\x1b[44m     \x1b[0m\x1b[?25l"[..];
    let rapid_text = &b"\x1b[6;4;9;31;44mBLINK\x1b[0m\x1b[?25l"[..];
    let slow_text = &b"\x1b[5;4;9;31;44mBLINK\x1b[0m\x1b[?25l"[..];
    let both = BlinkPhases {
        slow: Blink::Hidden,
        rapid: Blink::Hidden,
    };
    for phase in [rapid(Blink::Hidden), both] {
        assert_eq!(
            frame(rapid_text, glass(), phase),
            frame(spaces, glass(), phase),
            "rapid text in the rapid off phase left a mark ({phase:?})"
        );
    }
    let drawn = frame(rapid_text, glass(), BlinkPhases::SHOWN);
    assert_ne!(
        drawn,
        frame(spaces, glass(), BlinkPhases::SHOWN),
        "the control: shown, it draws"
    );
    assert_eq!(
        frame(rapid_text, glass(), slow(Blink::Hidden)),
        drawn,
        "rapid text hid in the slow off phase"
    );
    assert_eq!(
        frame(slow_text, glass(), rapid(Blink::Hidden)),
        frame(slow_text, glass(), BlinkPhases::SHOWN),
        "slow text hid in the rapid off phase"
    );
}

/// **A rate flips a frame only while text at that rate is on screen**, which
/// is how a driver asks: `Blinking::flips` from the last phases to the new.
#[test]
fn a_rate_flips_a_frame_only_while_its_text_is_on_screen() {
    let (shown, s_off, r_off) = (
        BlinkPhases::SHOWN,
        slow(Blink::Hidden),
        rapid(Blink::Hidden),
    );
    assert!(
        Blinking::SLOW.flips(shown, s_off),
        "the control: slow text, slow flip"
    );
    assert!(
        Blinking::RAPID.flips(shown, r_off),
        "rapid text, rapid flip"
    );
    assert!(
        !Blinking::SLOW.flips(shown, r_off),
        "slow text owes no rapid flip"
    );
    assert!(
        !Blinking::RAPID.flips(shown, s_off),
        "rapid text owes no slow flip"
    );
    assert!(
        Blinking::BOTH.flips(s_off, r_off),
        "both on screen, both flipped"
    );
    assert!(!Blinking::UNSEEN.flips(shown, r_off), "nothing blinks");
    assert!(!Blinking::BOTH.flips(r_off, r_off), "no flip");
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
                frame(text, theme.clone(), BlinkPhases::SHOWN),
                frame(spaces, theme.clone(), BlinkPhases::SHOWN),
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
            frame(text, theme.clone(), BlinkPhases::SHOWN),
            frame(&none, theme.clone(), BlinkPhases::SHOWN),
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
    feed(&mut t, b"\x1b[5mBLINK\x1b[0m\r\n\x1b[6mRAPID\x1b[0m\r\n");
    paint(&mut r, &mut t, &mut c, Damage::Full);
    assert_eq!(
        r.blink_seen(),
        Blinking::BOTH,
        "the blinking rows were drawn"
    );
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
        Blinking::BOTH,
        "an incremental frame forgot the blinking text still on screen"
    );
    // Erasing the slow row leaves the rapid one, at its rate alone.
    feed(&mut t, b"\x1b[1;1H\x1b[K");
    paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(
        r.blink_seen(),
        Blinking::RAPID,
        "the rapid row is still on screen"
    );
    // The control: once both are erased, nothing blinks.
    feed(&mut t, b"\x1b[2;1H\x1b[K");
    paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(r.blink_seen(), Blinking::UNSEEN, "nothing on screen blinks");
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
        map.ensure(t, t.view(Rows::ZERO), &styles, r, &mut candidates);
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
                composition: None,
            },
            r,
            &mut mask,
            &mut records,
        );
        r.blink_seen()
    };
    feed(&mut t, b"plain");
    assert_eq!(seen(&t, &mut r), Blinking::UNSEEN, "nothing blinks yet");
    feed(&mut t, b"\x1b[5mBLINK\x1b[0m");
    assert_eq!(
        seen(&t, &mut r),
        Blinking::SLOW,
        "the blinking cells are on screen, at the slow rate alone"
    );
    feed(&mut t, b"\x1b[6mRAPID\x1b[0m");
    assert_eq!(
        seen(&t, &mut r),
        Blinking::BOTH,
        "and the rapid ones beside them"
    );
    feed(&mut t, b"\x1b[2J");
    assert_eq!(
        seen(&t, &mut r),
        Blinking::UNSEEN,
        "no cell on screen blinks, and the table still holds the renditions"
    );
    feed(&mut t, b"\x1b[6mRAPID\x1b[0m");
    assert_eq!(seen(&t, &mut r), Blinking::RAPID, "the rapid rate alone");
}

/// **A table whose only blinking slot is rapid still opens the walk.** The
/// cell path tests cells for blinking only while the table holds a blinking
/// slot, and that count is of either rate: counted by `SGR 5` alone, a screen
/// of rapid text would never be seen and would never flip.
#[test]
fn a_rapid_slot_alone_is_seen_on_the_cell_path() {
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
    let mut stamp = StyleStamp::default();
    feed(&mut t, b"\x1b[6mRAPID\x1b[0m");
    r.sync_styles(&t, &mut styles, &mut stamp);
    map.ensure(&t, t.view(Rows::ZERO), &styles, &mut r, &mut candidates);
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
            composition: None,
        },
        &mut r,
        &mut mask,
        &mut records,
    );
    assert_eq!(
        r.blink_seen(),
        Blinking::RAPID,
        "the rapid cells are on screen"
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
    let mut drawn = |bytes: &[u8], phase: BlinkPhases, r: &mut Renderer<'_, '_>| {
        feed(&mut t, b"\x1b[H\x1b[2J");
        feed(&mut t, bytes);
        r.set_blink(phase);
        r.resolve_styles_into(&t, &mut styles);
        map.ensure(&t, t.view(Rows::ZERO), &styles, r, &mut candidates);
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
                composition: None,
            },
            r,
            &mut mask,
            &mut records,
        );
        records.len()
    };
    let text = "e\u{301} a -> b";
    // The control: shown, the mark and the ligature are drawn by records.
    let shown = BlinkPhases::SHOWN;
    assert!(
        drawn(format!("\x1b[5m{text}").as_bytes(), shown, &mut r) > 0,
        "the mark and the ligature went to the records"
    );
    assert_eq!(
        drawn(
            format!("\x1b[5m{text}").as_bytes(),
            slow(Blink::Hidden),
            &mut r
        ),
        0,
        "blinking text's records were drawn in its off phase"
    );
    let rapid_text = format!("\x1b[6m{text}");
    assert!(
        drawn(rapid_text.as_bytes(), slow(Blink::Hidden), &mut r) > 0,
        "rapid text's records were dropped in the slow off phase"
    );
    assert_eq!(
        drawn(rapid_text.as_bytes(), rapid(Blink::Hidden), &mut r),
        0,
        "rapid text's records were drawn in its off phase"
    );
    assert_eq!(
        drawn(format!("\x1b[8m{text}").as_bytes(), shown, &mut r),
        0,
        "concealed text's records were drawn"
    );
}

/// **A faint cell's records are drawn, faint**: the cell path's records --
/// a mark, a ligature window, and a double-width line's glyphs and its
/// underline in its own `SGR 58` colour -- are all there in the faint off
/// phase, one for one with the shown phase's, and each in a colour of its
/// own: the faint one. A record skipped as hidden, or drawn in full, fails.
#[test]
fn a_faint_cells_records_are_drawn_faint() {
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
    let mut candidates = Vec::new();
    let mut drawn = |bytes: &[u8], phase: BlinkPhases, r: &mut Renderer<'_, '_>| {
        let mut records = Vec::new();
        feed(&mut t, b"\x1b[H\x1b[2J");
        feed(&mut t, bytes);
        r.set_blink(phase);
        r.resolve_styles_into(&t, &mut styles);
        map.ensure(&t, t.view(Rows::ZERO), &styles, r, &mut candidates);
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
                composition: None,
            },
            r,
            &mut mask,
            &mut records,
        );
        records
    };
    for (text, phase) in [
        ("\x1b[5;31me\u{301} a -> b", slow(Blink::Faint)),
        ("\x1b[6;31me\u{301} a -> b", rapid(Blink::Faint)),
        ("\x1b#6\x1b[5;4;31;58;5;3mWIDE", slow(Blink::Faint)),
    ] {
        let shown = drawn(text.as_bytes(), BlinkPhases::SHOWN, &mut r);
        let faint = drawn(text.as_bytes(), phase, &mut r);
        assert!(
            !shown.is_empty(),
            "the control: {text:?} goes to the records"
        );
        assert_eq!(
            faint.len(),
            shown.len(),
            "{text:?}: a faint record was dropped"
        );
        for (s, f) in shown.iter().zip(&faint) {
            assert_ne!(s.color, f.color, "{text:?}: a record drawn in full");
        }
    }
}

/// One full CPU frame of `bytes` in `phase`, its pixels.
fn pixels(bytes: &[u8], theme: Theme, phase: BlinkPhases) -> Vec<u32> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, theme);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    r.set_blink(phase);
    let mut c = Canvas::for_renderer(&r);
    paint(&mut r, &mut t, &mut c, Damage::Full);
    c.pixels
}

/// **A faint off phase draws the text half way to its page**: every pixel
/// of the glyphs, the underline in its own `SGR 58` colour and the strike
/// is half as far from the page as in the on phase -- each channel within
/// rounding of half the on phase's distance, as `SGR 2` mixes -- in each
/// rate's off phase. Hidden would be the page and shown the full distance,
/// so the control is that both differ from it.
#[test]
fn a_faint_off_phase_draws_the_text_half_way_to_its_page() {
    let theme = Theme::dark;
    for (text, spaces, phase) in [
        (
            &b"\x1b[5;4;9;31;58;5;3mBLINK\x1b[0m\x1b[?25l"[..],
            &b"     \x1b[?25l"[..],
            slow(Blink::Faint),
        ),
        (
            &b"\x1b[6;4;9;32;58;5;5mBLINK\x1b[0m\x1b[?25l"[..],
            &b"     \x1b[?25l"[..],
            rapid(Blink::Faint),
        ),
    ] {
        let page = pixels(spaces, theme(), BlinkPhases::SHOWN);
        let on = pixels(text, theme(), BlinkPhases::SHOWN);
        let faint = pixels(text, theme(), phase);
        assert_ne!(faint, on, "the faint phase drew the text in full");
        assert_ne!(faint, page, "the faint phase hid the text");
        let mut inked = 0;
        for ((&p, &s), &f) in page.iter().zip(&on).zip(&faint) {
            for shift in [0, 8, 16] {
                let ch = |w: u32| i32::try_from((w >> shift) & 0xff).unwrap();
                let (full, half) = (ch(s) - ch(p), ch(f) - ch(p));
                assert!(
                    (2 * half - full).abs() <= 3,
                    "{phase:?}: page {p:08x}, on {s:08x}, faint {f:08x}"
                );
                inked += usize::from(full.abs() > 32);
            }
        }
        assert!(inked > 100, "the control: the text inked {inked} channels");
    }
}

/// **The faint phase owes frames as the hidden one does**: a rate going
/// faint, coming back, or going from hidden to faint -- the setting
/// switched mid-phase -- changes what its text draws, and staying faint
/// changes nothing. `BlinkPhases::dimmed` turns only hidden phases faint.
#[test]
fn a_faint_phase_flips_frames_and_dims_only_hidden_phases() {
    let (shown, faint, hidden) = (BlinkPhases::SHOWN, slow(Blink::Faint), slow(Blink::Hidden));
    assert!(Blinking::SLOW.flips(shown, faint), "going faint");
    assert!(Blinking::SLOW.flips(faint, shown), "coming back");
    assert!(Blinking::SLOW.flips(hidden, faint), "hidden to faint");
    assert!(
        !Blinking::SLOW.flips(faint, faint),
        "the control: no change"
    );
    assert!(
        !Blinking::RAPID.flips(shown, faint),
        "rapid text owes no slow change"
    );
    assert_eq!(hidden.dimmed(), faint);
    assert_eq!(shown.dimmed(), shown);
    assert_eq!(rapid(Blink::Hidden).dimmed(), rapid(Blink::Faint));
    assert_eq!(faint.hidden(), Default::default());
    assert_eq!(hidden.faint(), Default::default());
}
