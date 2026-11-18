//! **The cell path's scenes, drawn alike on every path**: screens fed as
//! bytes, drawn by the scene harness (`scenes::Arms`) on the cell path,
//! the instance path and the CPU path, each frame held to the cell path's
//! bars -- the cell frame the instance frame exactly, and both the CPU
//! path's premultiplied frame, exact where no record blends and within a
//! step where one does -- on lavapipe on both memory arms, and on RADV
//! staged and on the arm its heaps allow. Each scene carries its own
//! controls, so that frames which agree are not frames that all dropped
//! what the scene draws.
//!
//! - [`the_cell_arm_draws_what_the_instance_arm_draws`]: a screen of every
//!   rule the cell path resolves -- colours, attributes, decorations, the
//!   procedural box and block range, marks and ligatures, on six faces and a
//!   translucent page whose set backgrounds stay opaque -- under every
//!   cursor shape, an `OSC 12` cursor colour, and each blinking shape's off
//!   phase, its fades and the plasma's glow.
//! - [`each_decoration_alone_and_the_cursor_line_draw_alike_on_every_arm`]:
//!   each decoration alone on a table that held none, and the cursor's line
//!   alone under every shape and look, solid and fading.
//! - [`a_coloured_bell_draws_alike_on_every_arm`]: a bell's tint at its
//!   peak and on the way down, on the dark and the light theme, and the
//!   other video where no colour is set.
//! - [`a_cell_frame_a_pixel_apart_fails_its_scene`],
//!   [`an_unblended_pixel_a_step_off_fails_its_scene`],
//!   [`a_blended_pixel_three_steps_off_fails_its_scene`]: the controls that
//!   the harness's bars fail a frame one pixel wrong.
//!
//! The scenes of every path that no file here draws, each for its reason:
//!
//! - smooth scroll and screen slides, as the cell path draws no pan yet
//!   (design section 8): `the_slide_draws_alike_on_every_arm`,
//!   `a_switch_while_the_screen_slides_keeps_it_unslid`,
//!   `no_surface_keeps_a_slid_frame`,
//!   `a_slide_draws_none_of_the_shells_ink_under_the_logo`,
//!   `a_sliding_cursor_line_draws_as_the_whole_pass_does`,
//!   `a_slide_under_a_re_broken_paragraph_draws_the_display_line`,
//!   `a_right_to_left_line_slides_out_in_the_order_it_was_drawn`,
//!   `a_line_sliding_out_is_shaped_as_it_is_drawn_still`,
//!   `a_sliding_lines_letter_reads_its_word_as_drawn_still`;
//! - images, as the cell path does not draw them yet and counts them
//!   undrawn: `a_shared_sixel_recolours_alike_on_every_arm_unsaid_and_mid_body`,
//!   `mpvs_kitty_video_draws_alike_on_every_arm`,
//!   `under_a_two_frame_quota_mpvs_video_draws_alike_on_every_arm`,
//!   `placements_that_tie_on_z_stack_alike_on_every_arm`;
//! - effect trails and fireworks, whose records the cell path leaves
//!   undrawn and counts: `the_fireworks_draw_alike_on_every_arm`, and
//!   `every_effect_rides_rigidly`, the effects under a smooth scroll on the
//!   CPU path alone;
//! - the instance path's own frames by rows, as it draws only whole frames:
//!   `a_cursor_only_instance_frame_draws_its_rows_and_the_whole_frames_pixels`;
//! - a frame made to fail, as `Gpu` has no fault to inject:
//!   `both_arms_failing_is_not_a_pass` and
//!   `a_failed_frame_is_not_compared_against`, whose concern -- frames that
//!   agree by all failing -- the three controls above hold instead;
//! - letters in faces the repository does not carry (Courier New's Arabic,
//!   Noto Sans Syriac, SF Arabic):
//!   `a_composition_of_arabic_draws_joined_as_written_on_every_arm`,
//!   `arabic_decrlm_wrote_draws_joined_with_its_forms_on_every_arm`,
//!   `arabic_letters_draw_joined_on_every_arm`,
//!   `a_letters_word_reads_its_digits_as_drawn_on_every_arm`,
//!   `a_joined_letter_reads_its_word_on_every_arm`,
//!   `sapv_meets_a_letters_word_on_every_arm`,
//!   `a_slanted_lam_alef_is_read_in_its_word_on_every_arm`,
//!   `an_italic_letter_reads_its_word_on_every_arm`,
//!   `a_status_lines_letter_reads_its_word_on_every_arm`;
//! - the window copy's arm and the kept frame of a screen switch, in the
//!   scenes that draw them: they need an API only the macOS presenter has.
//!
//! The faces are JetBrains Mono and the Noto colour emoji subset from
//! `baud-font/tests/fonts`, and the styled JetBrains Mono faces and the
//! Symbols Nerd Font from `baud-app/fonts`, each under the licence beside it.

#![cfg(target_os = "linux")]

mod common;
mod scenes;

use baud_core::{Owned, Term};
use baud_domain::{Chan, Codepoint, WinSize};
use baud_font::Face;
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::lend::Arm;
use baud_render::{
    BellFlash, BellKind, BellShown, Blink, CursorLook, CursorPhase, Decorations, FaceRole,
    FaceStyle, Rang, Renderer, Stack, Theme, VideoFlash,
};
use common::{lavapipe, radv};
use core::time::Duration;
use scenes::{Arms, differing, feed, jetbrains_mono, repo_file};

/// Wide enough that a path which stops short of the right-hand side shows.
const WS: WinSize = WinSize {
    ws_col: 120,
    ws_row: 10,
};

/// **A screen with every rule the cell path resolves on it**: text, SGR,
/// `38;5`, `48;5` and truecolour cells, underline styles and an underline
/// colour, a set background, reverse, dim and hidden runs, palette entries
/// 0, 7, 8 and 15, box drawing and braille, the procedural range -- heavy,
/// double, arc, diagonal, dashed, block, shade and braille, and a red heavy
/// corner -- combining marks, ligature pairs, and a band to the very last
/// column above one that stops a cell short of it.
fn scene(t: &mut Term<'_>) {
    feed(
        t,
        b"Hello, world. 0123456789 abc XYZ (x) [y] {z}\r\n\
          \x1b[31mred \x1b[1;32mbold green\x1b[0m \x1b[4munderline\x1b[0m \x1b[9mstrike\x1b[0m\r\n\
          \x1b[4:3mcurly\x1b[0m \x1b[4:4mdotted\x1b[0m \x1b[4:5mdashed\x1b[0m \x1b[58;5;196;4:3mred curl\x1b[0m\r\n\
          \x1b[44m blue bg \x1b[7m reverse \x1b[0m \x1b[2mdim\x1b[0m \x1b[8mhidden\x1b[0m done \x1b[30;47m ink \x1b[90;107m pale \x1b[0m\r\n\
          \x1b[38;2;200;100;50mtrue colour\x1b[0m \x1b[48;5;27;38;5;226mindexed\x1b[0m \x1b[58;5;196;4mul red\x1b[0m\r\n\
          box \xe2\x94\x8c\xe2\x94\x80\xe2\x94\x90 braille \xe2\xa3\xbf end \x1b[38;2;200;0;0;48;2;0;0;200m\xe2\x96\x80\xe2\x96\x80 \xe2\x94\x80\x1b[0;7;32m\xe2\x96\x84\x1b[0m image\r\n\
          \xe2\x94\x8f\xe2\x94\x81\xe2\x94\xb3\xe2\x94\x93 \xe2\x95\x94\xe2\x95\x90\xe2\x95\xa6\xe2\x95\x97 \xe2\x95\xad\xe2\x94\x80\xe2\x95\xae \xe2\x95\xb1\xe2\x95\xb2\xe2\x95\xb3 \xe2\x94\x84\xe2\x94\x8c\xe2\x95\x8c \xe2\x96\x80\xe2\x96\x84\xe2\x96\x88\xe2\x96\x8c\xe2\x96\x90\xe2\x96\x91\xe2\x96\x92\xe2\x96\x93\xe2\x96\x9a\xe2\x96\x9f \xe2\xa0\x81\xe2\xa0\xbf\xe2\xa3\x80\xe2\xa2\xb2 \x1b[31m\xe2\x94\x8f\xe2\x94\x81\xe2\x94\x93\x1b[0m\r\n\
          marks e\xcc\x81 a\xcc\x88 o\xcc\x82 q\xcc\x96\xcc\x81 composed\r\n\
          arrows a -> b => c != d <= e // f :: g <- end\r\n",
    );
    let full = "~".repeat(usize::from(WS.ws_col));
    let short = "^".repeat(usize::from(WS.ws_col) - 1);
    feed(t, format!("\x1b[36m{full}\x1b[0m").as_bytes());
    feed(t, format!("\r\n\x1b[36m{short}\x1b[0m").as_bytes());
}

/// A renderer over `faces` in `theme`, sized to [`WS`].
fn fresh<'f, 'a>(faces: &'f [Face<'a>], theme: Theme) -> Renderer<'f, 'a> {
    scenes::renderer(Stack::new(faces).expect("a stack"), theme, WS)
}

/// A fade part way through: the cursor at `opacity`, no glow.
fn fade(opacity: u8) -> CursorPhase {
    phase(opacity, 0)
}

/// The cursor at `opacity` and the plasma's glow at `glow`.
fn phase(opacity: u8, glow: u8) -> CursorPhase {
    CursorPhase {
        opacity: Chan::from_u8(opacity),
        glow: Chan::from_u8(glow),
        ..CursorPhase::HIDDEN
    }
}

/// **The blink animations' phases, part way through each**: a fade at
/// half and near its end, the plasma glow, and the fire's and the ufo's
/// hand-over, burning or lifting, and end.
fn blink_phases() -> Vec<(CursorPhase, &'static str)> {
    let fire = |at: u8| CursorPhase::fire(Chan::from_u8(at));
    let ufo = |at: u8| CursorPhase::ufo(Chan::from_u8(at));
    vec![
        (fade(128), "half way through a fade"),
        (fade(40), "near the end of a fade out"),
        (phase(0, 200), "glowing"),
        (fire(10), "handing over to the fire"),
        (fire(70), "burning down"),
        (fire(135), "in embers"),
        (fire(205), "relighting"),
        (ufo(10), "handing over to the ufo"),
        (ufo(70), "lifting"),
        (ufo(128), "held up"),
        (ufo(190), "setting down"),
    ]
}

/// **The rules scene on `arm`**: JetBrains Mono with its bold, italic and
/// bold-italic faces, the Noto subset for U+231A and the icon face behind
/// them, so styled cells come from their own faces on every path; a page
/// at alpha 56 whose set backgrounds stay opaque, so the swap a cursor or
/// a selection makes over a translucent background is drawn.
fn drawn_alike(gpu: &Gpu, arm: Arm) {
    let data = jetbrains_mono();
    let noto = repo_file("baud-font/tests/fonts/NotoColorEmoji-subset.ttf");
    let symbols = repo_file("baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf");
    let bold = repo_file("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf");
    let italic = repo_file("baud-app/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf");
    let bold_italic = repo_file("baud-app/fonts/jetbrains-mono/JetBrainsMono-BoldItalic.ttf");
    let faces = [
        Face::parse(&data).expect("JetBrains Mono"),
        Face::parse(&noto).expect("the Noto subset"),
        Face::parse(&symbols).expect("the icon face"),
        Face::parse(&bold).expect("the bold face"),
        Face::parse(&italic).expect("the italic face"),
        Face::parse(&bold_italic).expect("the bold-italic face"),
    ];
    // `q` + U+0316 + U+0301 is the cluster no face draws whole: every path
    // keeps it on JetBrains Mono and drops the mark before shaping.
    for (i, f) in faces.iter().enumerate() {
        assert!(
            f.glyph(Codepoint::new(0x316).expect("a codepoint"))
                .is_none(),
            "the premise: face {i} lacks U+0316"
        );
    }
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Style(FaceStyle::Bold),
        FaceRole::Style(FaceStyle::Italic),
        FaceRole::Style(FaceStyle::BoldItalic),
    ];
    let theme = Theme {
        bg_alpha: Chan::from_u8(56),
        app_bg: baud_render::AppBg::Opaque,
        ..Theme::dark()
    };
    let r = scenes::renderer(
        Stack::with_roles(&faces, &roles).expect("a stack"),
        theme,
        WS,
    );
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, r, &t);
    scene(&mut t);
    // Every cursor shape, block first.
    for (shape, name) in [
        (b"\x1b[2 q", "block"),
        (b"\x1b[4 q", "underline"),
        (b"\x1b[6 q", "bar"),
    ] {
        feed(&mut t, shape);
        let f = arms.check(&mut t, &format!("{arm:?}: the scene, a {name} cursor"));
        // Four marked cells and seven ligature pairs, each drawn by a
        // record, the pairs shaped in windows.
        let stats = arms.exceptions().stats();
        assert!(
            stats.cells.value() >= 4 + 7,
            "the marked and ligature cells went to the records: {stats:?}"
        );
        assert!(
            stats.windows.value() >= 1,
            "the admitted pairs were shaped in windows: {stats:?}"
        );
        assert!(
            f.instance.iter().filter(|&&p| p != 0).count() > f.instance.len() / 2,
            "the instance path drew the scene, {name}"
        );
        assert!(
            arms.renderer.marks_unmapped().value() > 0,
            "the dropped-mark cluster reached the drop, {name}"
        );
    }
    // **`OSC 12`, a cursor colour the application set**: it moves the
    // block's pixels off the reverse-video default, and every shape takes
    // it on every path.
    feed(&mut t, b"\x1b[2 q\x1b[1;1H");
    let plain = arms.check(&mut t, &format!("{arm:?}: a block cursor at the origin"));
    feed(&mut t, b"\x1b]12;rgb:ff/00/00\x07");
    let block = arms.check(&mut t, &format!("{arm:?}: an OSC 12 block cursor"));
    assert_ne!(
        differing(&plain.instance, &block.instance),
        0,
        "the OSC 12 colour never reached the block cursor's pixels"
    );
    for (shape, name) in [
        (b"\x1b[4 q".as_slice(), "underline"),
        (b"\x1b[6 q".as_slice(), "bar"),
    ] {
        feed(&mut t, shape);
        arms.check(&mut t, &format!("{arm:?}: an OSC 12 {name} cursor"));
    }
    feed(&mut t, b"\x1b]112\x07");
    // **A blinking cursor's off phase** is the frame `?25l` draws, and not
    // the on phase; a fade part way is neither.
    for (shape, name) in [
        (b"\x1b[1 q", "block"),
        (b"\x1b[3 q", "underline"),
        (b"\x1b[5 q", "bar"),
    ] {
        feed(&mut t, shape);
        let on = arms.check(&mut t, &format!("{arm:?}: a blinking {name} cursor, on"));
        arms.renderer.set_cursor_blink(Blink::Hidden);
        let off = arms.check(&mut t, &format!("{arm:?}: a blinking {name} cursor, off"));
        assert_ne!(
            differing(&on.instance, &off.instance),
            0,
            "a {name} cursor's off phase reached no pixel"
        );
        arms.renderer.set_cursor_blink(Blink::Shown);
        feed(&mut t, b"\x1b[?25l");
        let none = arms.check(&mut t, &format!("{arm:?}: no cursor, after a {name}"));
        assert_eq!(
            differing(&off.instance, &none.instance),
            0,
            "a {name} cursor's off phase is not the frame without a cursor"
        );
        feed(&mut t, b"\x1b[?25h");
        for (p, what) in blink_phases() {
            arms.renderer.set_cursor_blink(p);
            let f = arms.check(&mut t, &format!("{arm:?}: a {name} cursor {what}"));
            assert_ne!(
                differing(&f.instance, &on.instance),
                0,
                "a {name} cursor {what} is the on phase"
            );
            assert_ne!(
                differing(&f.instance, &off.instance),
                0,
                "a {name} cursor {what} is the off phase"
            );
        }
        arms.renderer.set_cursor_blink(Blink::Shown);
    }
}

#[test]
fn the_cell_arm_draws_what_the_instance_arm_draws() {
    let gpu = lavapipe();
    drawn_alike(&gpu, Arm::InPlace);
    drawn_alike(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn the_cell_arm_draws_what_the_instance_arm_draws_on_radv() {
    let gpu = radv();
    drawn_alike(&gpu, Arm::Staging);
    drawn_alike(&gpu, gpu.arm(1));
}

/// **Each decoration alone, and the cursor's line alone, on `arm`**: each
/// decoration minted on a screen whose table held none, and an undecorated
/// screen under each cursor shape, shaped and hollow, solid and part way
/// through a fade -- each on a terminal of its own, since a reset keeps the
/// table's slots -- with the premise checked each time: the table held no
/// decoration before, and the decoration or the cursor reached the pixels.
fn decorations_alone(gpu: &Gpu, arm: Arm) {
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("JetBrains Mono")];
    // The screen before the case, drawn: the table it was synced from
    // holds no decoration.
    let plain = |arms: &mut Arms<'_, '_, '_>, t: &mut Term<'_>, what: &str| {
        feed(
            t,
            b"\x1b[?25l\x1b[2;3HThe quick brown fox \x1b[31mjumps over\x1b[0m 0123456789",
        );
        arms.check(t, &format!("{what}, before"));
        assert_eq!(
            arms.stamp().decorations(),
            Decorations::Absent,
            "{what}: the table held no decoration before"
        );
    };
    let tail = |sgr: &str| format!(" \x1b[{sgr}mmarked, a.b\x1b[0m end");
    for sgr in [
        "4", "9", "21", "4:2", "4:3", "4:4", "4:5", "53", "62", "63", "51", "52", "64",
    ] {
        let what = format!("{arm:?}: SGR {sgr} alone");
        let control = {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut arms = Arms::new(gpu, arm, fresh(&faces, Theme::dark()), &t);
            plain(&mut arms, &mut t, &what);
            feed(&mut t, tail("0").as_bytes());
            arms.check(&mut t, &format!("{what}, its control")).instance
        };
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces, Theme::dark()), &t);
        plain(&mut arms, &mut t, &what);
        feed(&mut t, tail(sgr).as_bytes());
        let f = arms.check(&mut t, &what);
        assert_eq!(
            arms.stamp().decorations(),
            Decorations::Present,
            "{what}: counted a decoration"
        );
        assert!(
            differing(&control, &f.instance) > 0,
            "{what}: never reached the instance path's pixels"
        );
    }
    // The cursor on a table with no decoration, its own line the shape
    // pass's one instance: under every shape and look, solid and fading.
    for (shape, style) in [("block", 2), ("underline", 4), ("bar", 6)] {
        for look in [CursorLook::Shaped, CursorLook::Hollow] {
            for phase in [None, Some(fade(128))] {
                let what = format!(
                    "{arm:?}: a {shape} cursor, {look:?}, fading {}",
                    phase.is_some()
                );
                let mut owned = Owned::new(&WS);
                let mut t = Term::new(owned.storage(), &WS);
                let mut arms = Arms::new(gpu, arm, fresh(&faces, Theme::dark()), &t);
                arms.renderer.set_cursor_look(look);
                plain(&mut arms, &mut t, &what);
                feed(&mut t, tail("0").as_bytes());
                let none = arms.check(&mut t, &format!("{what}, no cursor"));
                feed(
                    &mut t,
                    format!("\x1b[{style} q\x1b[2;5H\x1b[?25h").as_bytes(),
                );
                if let Some(p) = phase {
                    arms.renderer.set_cursor_blink(p);
                }
                let f = arms.check(&mut t, &what);
                assert_eq!(
                    arms.stamp().decorations(),
                    Decorations::Absent,
                    "{what}: the table holds no decoration"
                );
                assert!(
                    differing(&none.instance, &f.instance) > 0,
                    "{what}: the cursor reached no pixel"
                );
            }
        }
    }
}

#[test]
fn each_decoration_alone_and_the_cursor_line_draw_alike_on_every_arm() {
    let gpu = lavapipe();
    decorations_alone(&gpu, Arm::InPlace);
    decorations_alone(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn each_decoration_alone_and_the_cursor_line_draw_alike_on_every_arm_on_radv() {
    let gpu = radv();
    decorations_alone(&gpu, Arm::Staging);
    decorations_alone(&gpu, gpu.arm(1));
}

/// **A `BEL` in a colour, on `arm`**: the bell's solid over the cells, at
/// the peak and on the way down, on the dark and the light theme, each
/// frame not the one without it; with the tint gone the frame is the plain
/// one again; with no colour set the frame is the other video's.
fn coloured_bell(gpu: &Gpu, arm: Arm) {
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("JetBrains Mono")];
    for (theme, page) in [(Theme::dark(), "dark"), (Theme::light(), "light")] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces, theme), &t);
        feed(
            &mut t,
            b"\x1b[?25l\x1b[2;3HThe quick brown fox \x1b[31mjumps over\x1b[0m 0123456789\x1b[7m reversed \x1b[0m",
        );
        let plain = arms.check(&mut t, &format!("{arm:?}, {page}: no bell"));
        for (colour, at) in [(&b"#ff0000"[..], 50), (b"#3f638b", 50), (b"#ffffff", 80)] {
            let what = format!(
                "{arm:?}, {page}: {} at {at} ms",
                String::from_utf8_lossy(colour)
            );
            feed(
                &mut t,
                &[&b"\x1b]21;visual_bell="[..], colour, b"\x07"].concat(),
            );
            let kind = BellKind::of(&t);
            let (b, _) =
                BellFlash::new(Duration::from_millis(100)).at(Duration::ZERO, Rang::Yes, kind);
            let shown = b.at(Duration::from_millis(at), Rang::No, kind).1;
            assert!(shown.tint().is_some(), "{what}: a tint");
            arms.renderer.set_bell_tint(shown.tint());
            let f = arms.check(&mut t, &what);
            assert!(
                differing(&plain.instance, &f.instance) > 0,
                "{what}: the tint reached the pixels"
            );
        }
        arms.renderer.set_bell_tint(None);
        let back = arms.check(&mut t, &format!("{arm:?}, {page}: the tint gone"));
        assert_eq!(
            differing(&back.instance, &plain.instance),
            0,
            "nothing left of the tint"
        );
        feed(&mut t, b"\x1b]21;visual_bell\x07");
        let (b, shown) = BellFlash::new(Duration::from_millis(100)).at(
            Duration::ZERO,
            Rang::Yes,
            BellKind::of(&t),
        );
        assert_eq!(shown, BellShown::Video, "no colour: the other video");
        assert!(b.lit().is_some());
        arms.renderer.set_flash(shown.video());
        arms.renderer.set_bell_tint(shown.tint());
        let f = arms.check(&mut t, &format!("{arm:?}, {page}: the other video"));
        assert!(
            differing(&f.instance, &plain.instance) > 0,
            "the other video reached the pixels"
        );
        arms.renderer.set_flash(VideoFlash::Off);
    }
}

#[test]
fn a_coloured_bell_draws_alike_on_every_arm() {
    let gpu = lavapipe();
    coloured_bell(&gpu, Arm::InPlace);
    coloured_bell(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_coloured_bell_draws_alike_on_every_arm_on_radv() {
    let gpu = radv();
    coloured_bell(&gpu, Arm::Staging);
    coloured_bell(&gpu, gpu.arm(1));
}

/// **The frames of a screen of text on lavapipe**, drawn and not held to
/// anything, and the bar they would be held to: what the controls below
/// spoil.
fn control_frames() -> (scenes::Frames, common::Bar) {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("JetBrains Mono")];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(&gpu, Arm::InPlace, fresh(&faces, Theme::dark()), &t);
    feed(
        &mut t,
        b"The quick brown fox \x1b[31mjumps over\x1b[0m the lazy dog",
    );
    let frames = arms.draw(&mut t);
    let bar = arms.bar;
    (frames, bar)
}

/// **A cell frame a pixel apart from the instance frame fails its
/// scene**: the control that the exact bar can fail.
#[test]
#[should_panic(expected = "pixels apart from the instance frame")]
fn a_cell_frame_a_pixel_apart_fails_its_scene() {
    let (mut f, _) = control_frames();
    f.cell[0] ^= 0x0001_0000;
    f.cell_is_instance("the control");
}

/// **An unblended pixel a step off the CPU path's fails its scene**: no
/// record blends it, so the bar holds it exact.
#[test]
#[should_panic(expected = "pixels wrong")]
fn an_unblended_pixel_a_step_off_fails_its_scene() {
    let (mut f, bar) = control_frames();
    let at = f
        .blended
        .iter()
        .position(|&b| !b)
        .expect("an unblended pixel");
    f.cell[at] = f.cpu[at] ^ 0x0000_0001;
    f.instance[at] = f.cell[at];
    f.against_cpu("the control", bar);
}

/// **A blended pixel three steps off the CPU path's fails its scene**,
/// past the most any device is allowed.
#[test]
#[should_panic(expected = "pixels wrong")]
fn a_blended_pixel_three_steps_off_fails_its_scene() {
    let (mut f, bar) = control_frames();
    let at = f.blended.iter().position(|&b| b).expect("a blended pixel");
    let green = (f.cpu[at] >> 8) & 0xff;
    let off = if green >= 3 { green - 3 } else { green + 3 };
    f.cell[at] = (f.cpu[at] & !0x0000_ff00) | off << 8;
    f.instance[at] = f.cell[at];
    f.against_cpu("the control", bar);
}
