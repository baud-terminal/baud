//! **The CPU path copies a surface only the rows it lacks**, and every
//! surface it shows is the whole frame's copy. The compositor repaints the
//! rows a frame changed into a frame it keeps (`Renderer::painted`); three
//! surfaces rotate, and the one a frame goes to holds the frame three back,
//! so it is brought up to date in every row changed since, as the Wayland
//! presenter brings a released buffer up to date (`baud-present-wayland`'s
//! `damage.rs`). Held here against a presenter that copies every frame
//! whole, surface for surface, through cursor moves, blink flips, text
//! written and a change of the margin's colour; and each surface against
//! the copy of a frame a second renderer draws whole, through the same
//! with iTerm2's cursor guide on, the badge set and the config's notice
//! row shown.
//!
//! macOS only: the presenter it drives is.
#![cfg(target_os = "macos")]

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_present_macos::redraw::SurfaceIx;
use baud_present_macos::{Bands, Changed, Drew, Presented, Presenter};
use baud_render::{
    BADGE, BOX, BadgeLook, Blend, CURSOR_GUIDE, CursorPhase, Damage, FaceStyle, GuideLook, PxSize,
    Renderer, Stack, Target, Theme,
};
use macos_liquid_glass::drawable::ColorSpace;

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 8,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// What both renderers are given beside the terminal: the blink's phase,
/// the cursor guide, the badge, the notice and how far back the window is.
#[derive(Clone)]
struct Look {
    blink: CursorPhase,
    guide: Option<GuideLook>,
    badge: Option<BadgeLook>,
    notice: Option<Vec<Codepoint>>,
    back: Rows,
}

impl Look {
    fn apply(&self, r: &mut Renderer<'_, '_>, t: &Term<'_>) {
        r.set_cursor_blink(self.blink);
        r.set_cursor_guide(self.guide);
        r.set_badge(self.badge.clone());
        r.set_notice(self.notice.clone());
        r.fit(t);
    }
}

/// A change to the scene.
type Step = dyn Fn(&mut Look, &mut Term<'_>);

fn notice(text: &str) -> Vec<Codepoint> {
    text.chars().map(Codepoint::from_char).collect()
}

fn badge(text: &str) -> BadgeLook {
    BadgeLook {
        text: text.into(),
        colour: BADGE,
        outline: Theme::dark().bg.with_alpha(baud_render::DRAWN_AT),
        drawn_at: baud_render::DRAWN_AT,
        fraction: BOX,
        top: Px::of(4),
        right: Px::of(6),
        scale: 1.0,
        style: FaceStyle::Bold,
    }
}

const LESS: WinSize = WinSize {
    ws_col: WS.ws_col,
    ws_row: WS.ws_row - 1,
};

#[test]
fn a_cpu_frame_copies_only_its_rows_and_each_surface_is_the_whole_copy() {
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [Face::parse(&data).unwrap()];
    let theme = Theme {
        bg_alpha: baud_domain::Chan::from_u8(56),
        ..Theme::dark()
    };
    let renderer = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            theme.clone(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let mut r = renderer();
    // The reference: a renderer of its own, every frame drawn whole.
    let mut fresh = renderer();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let (w, h) = r.frame_size();
    // A margin round the frame, which only a whole copy fills.
    let (sw, sh) = (Px::of(w.value() + 6), Px::of(h.value() + 4));
    let mut rows = Presenter::new(sw, sh, ColorSpace::Srgb).expect("surfaces");
    let mut whole = Presenter::new(sw, sh, ColorSpace::Srgb).expect("surfaces");
    let mut reference = Presenter::new(sw, sh, ColorSpace::Srgb).expect("surfaces");
    let mut frame = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let mut fresh_frame = frame.clone();
    let mut clear = r.clear_color();
    let mut look = Look {
        blink: CursorPhase::SHOWN,
        guide: None,
        badge: None,
        notice: None,
        back: Rows::ZERO,
    };
    let guide = Some(GuideLook {
        colour: CURSOR_GUIDE,
        line: Px::of(2),
    });
    let steps: Vec<(&str, Box<Step>)> = vec![
        (
            "text and a blinking block",
            Box::new(|_, t| {
                feed(
                    t,
                    b"hello, world\r\n\x1b[31mred\x1b[0m line\x1b[1 q\x1b[2;5H",
                )
            }),
        ),
        ("the cursor moved", Box::new(|_, t| feed(t, b"\x1b[5;20H"))),
        ("the cursor moved", Box::new(|_, t| feed(t, b"\x1b[1;1H"))),
        (
            "the blink hidden",
            Box::new(|l, _| l.blink = CursorPhase::HIDDEN),
        ),
        (
            "the blink shown",
            Box::new(|l, _| l.blink = CursorPhase::SHOWN),
        ),
        (
            "text written",
            Box::new(|_, t| feed(t, b"\x1b[7;1Hmore text")),
        ),
        ("the cursor moved", Box::new(|_, t| feed(t, b"\x1b[3;3H"))),
        ("the cursor moved", Box::new(|_, t| feed(t, b"\x1b[8;40H"))),
        (
            "the margin's colour",
            Box::new(|_, t| feed(t, b"\x1b]11;#203040\x07")),
        ),
        ("the cursor moved", Box::new(|_, t| feed(t, b"\x1b[4;4H"))),
        ("the cursor moved", Box::new(|_, t| feed(t, b"\x1b[4;5H"))),
        ("the cursor guide on", Box::new(move |l, _| l.guide = guide)),
        (
            "the cursor moved under the guide",
            Box::new(|_, t| feed(t, b"\x1b[6;9H")),
        ),
        (
            "the blink hidden under the guide",
            Box::new(|l, _| l.blink = CursorPhase::HIDDEN),
        ),
        (
            "the hidden cursor moved under the guide",
            Box::new(|_, t| feed(t, b"\x1b[2;9H")),
        ),
        (
            "the blink shown under the guide",
            Box::new(|l, _| l.blink = CursorPhase::SHOWN),
        ),
        (
            "lines scrolled into the history",
            Box::new(|_, t| feed(t, b"\x1b[8;1H\n\n\nold\x1b[3;9H")),
        ),
        (
            "looking back with the guide",
            Box::new(|l, _| l.back = Rows::of_count(2)),
        ),
        (
            "the live cursor moved, looking back",
            Box::new(|_, t| feed(t, b"\x1b[4;2H")),
        ),
        (
            "the live screen again",
            Box::new(|l, _| l.back = Rows::ZERO),
        ),
        ("the cursor guide off", Box::new(|l, _| l.guide = None)),
        (
            "the badge set",
            Box::new(|l, _| l.badge = Some(badge("prod\nmain"))),
        ),
        (
            "a block cursor moved into the badge",
            Box::new(|_, t| feed(t, b"\x1b[2;36H")),
        ),
        (
            "the blink hidden in the badge",
            Box::new(|l, _| l.blink = CursorPhase::HIDDEN),
        ),
        (
            "the blink shown in the badge",
            Box::new(|l, _| l.blink = CursorPhase::SHOWN),
        ),
        (
            "the cursor moved out of the badge",
            Box::new(|_, t| feed(t, b"\x1b[6;3H")),
        ),
        (
            "the badge changed",
            Box::new(|l, _| l.badge = Some(badge("staging"))),
        ),
        ("the badge cleared", Box::new(|l, _| l.badge = None)),
        (
            "the notice shown",
            Box::new(|l, t| {
                let _ = t.resize(&LESS);
                l.notice = Some(notice("config.toml line 12: not key = value"));
            }),
        ),
        (
            "the cursor moved above the notice",
            Box::new(|_, t| feed(t, b"\x1b[7;3H")),
        ),
        (
            "the blink hidden above the notice",
            Box::new(|l, _| l.blink = CursorPhase::HIDDEN),
        ),
        (
            "the cursor moved away from the notice",
            Box::new(|_, t| feed(t, b"\x1b[2;3H")),
        ),
        (
            "the cursor moved again",
            Box::new(|_, t| feed(t, b"\x1b[3;3H")),
        ),
        (
            "the notice changed",
            Box::new(|l, _| l.notice = Some(notice("config.toml line 3: a list never closed"))),
        ),
        (
            "the blink shown above the notice",
            Box::new(|l, _| l.blink = CursorPhase::SHOWN),
        ),
        (
            "the notice gone",
            Box::new(|l, t| {
                l.notice = None;
                let _ = t.resize(&WS);
            }),
        ),
        ("the cursor moved", Box::new(|_, t| feed(t, b"\x1b[5;5H"))),
    ];
    let mut by_rows = 0;
    let mut wrong = Vec::new();
    let mut at = SurfaceIx::FIRST;
    for (i, (name, step)) in steps.iter().enumerate() {
        step(&mut look, &mut t);
        look.apply(&mut r, &t);
        let mut target = Target::new(&mut frame, w, w, h).unwrap();
        let seal = t.seal_writes();
        r.render_at(&t, seal, look.back, &mut target, Damage::Incremental);
        let painted: Vec<_> = r.painted().collect();
        look.apply(&mut fresh, &t);
        let mut fresh_target = Target::new(&mut fresh_frame, w, w, h).unwrap();
        let seal = t.seal_writes();
        fresh.render_at(&t, seal, look.back, &mut fresh_target, Damage::Full);
        if painted
            .iter()
            .map(|b| b.bottom.value() - b.top.value())
            .sum::<u32>()
            < h.value()
        {
            by_rows += 1;
        }
        clear = r.clear_color();
        assert_eq!(
            rows.copy_changed(&frame, w, clear, Changed::Bands(&painted)),
            Presented::Yes
        );
        assert_eq!(
            whole.copy_changed(&frame, w, clear, Changed::Whole),
            Presented::Yes
        );
        assert_eq!(
            reference.copy_changed(&fresh_frame, w, fresh.clear_color(), Changed::Whole),
            Presented::Yes
        );
        if rows.surface_pixels(at) != whole.surface_pixels(at) {
            wrong.push(format!("{i} {name}: unlike the whole copy"));
        }
        if rows.surface_pixels(at) != reference.surface_pixels(at) {
            wrong.push(format!("{i} {name}: unlike the frame drawn whole"));
        }
        at = at.next();
    }
    let _ = clear;
    assert!(wrong.is_empty(), "surfaces unlike a whole copy: {wrong:#?}");
    assert!(
        by_rows >= 20,
        "the cursor's frames repainted rows: {by_rows}"
    );
}

/// **A CPU frame drawn for a surface is copied into it**, and
/// `Presenter::copied` counts the bytes: each of the three surfaces is owed
/// every row the first time a frame goes to it, and after that the rows
/// changed since it was last drawn. Each surface is the frame a second
/// renderer draws whole.
#[test]
fn a_cpu_frame_drawn_for_a_surface_is_copied_into_it() {
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [Face::parse(&data).unwrap()];
    let theme = Theme {
        bg_alpha: baud_domain::Chan::from_u8(56),
        ..Theme::dark()
    };
    let renderer = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            theme.clone(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    };
    let mut r = renderer();
    let mut fresh = renderer();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let (w, h) = r.frame_size();
    let (sw, sh) = (Px::of(w.value() + 6), Px::of(h.value() + 4));
    let mut drawn = Presenter::new(sw, sh, ColorSpace::Srgb).expect("surfaces");
    let mut reference = Presenter::new(sw, sh, ColorSpace::Srgb).expect("surfaces");
    let mut fresh_frame = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let frame_bytes = u64::from(w.value()) * u64::from(h.value()) * 4;
    // Three frames to give each surface its first, whole copy; then text
    // and cursor moves, so the last surface lacks only the cursor's rows.
    let steps: [&[u8]; 7] = [
        b"hello, world\r\n\x1b[31mred\x1b[0m line",
        b"\x1b[5;20H",
        b"\x1b[1;1H",
        b"\x1b[7;1Hmore text",
        b"\x1b[3;3H",
        b"\x1b[3;4H",
        b"\x1b[3;5H",
    ];
    let mut at = SurfaceIx::FIRST;
    let mut wrong = Vec::new();
    for (i, bytes) in steps.iter().enumerate() {
        feed(&mut t, bytes);
        let before = drawn.copied();
        // The lent frame holds nothing before the first draw.
        let damage = match i {
            0 => Damage::Full,
            _ => Damage::Incremental,
        };
        let shown = drawn.draw_cpu(w, h, |target| {
            let seal = t.seal_writes();
            r.render(&t, seal, target, damage);
            Drew {
                painted: Bands::These(r.painted().collect()),
                clear: r.clear_color(),
            }
        });
        assert_eq!(shown, Presented::Yes, "{i}: drawn");
        let after = drawn.copied();
        let (whole, rows) = (after.whole - before.whole, after.rows - before.rows);
        assert_eq!(after.presents, before.presents + 1, "{i}: one copy a frame");
        match i {
            // A surface no frame has gone to lacks every row.
            0..=2 => {
                assert_eq!(
                    (whole, rows),
                    (frame_bytes, 0),
                    "{i}: the whole frame copied"
                );
            }
            // A surface drawn before: the rows the frames since changed.
            _ => {
                assert_eq!(whole, 0, "{i}: no whole copy");
                assert!(rows > 0, "{i}: rows copied");
            }
        }
        // Past the whole repaints, only the cursor's rows.
        if i == steps.len() - 1 {
            assert!(
                rows < frame_bytes,
                "{i}: the rows copied, {rows} of {frame_bytes} bytes"
            );
        }
        let mut fresh_target = Target::new(&mut fresh_frame, w, w, h).unwrap();
        let seal = t.seal_writes();
        fresh.render(&t, seal, &mut fresh_target, Damage::Full);
        assert_eq!(
            reference.copy_changed(&fresh_frame, w, fresh.clear_color(), Changed::Whole),
            Presented::Yes
        );
        if drawn.surface_pixels(at) != reference.surface_pixels(at) {
            wrong.push(format!("{i}: unlike the frame drawn whole"));
        }
        at = at.next();
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
