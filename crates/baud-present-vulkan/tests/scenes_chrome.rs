//! **What baud draws around and over the cells, on every path**: screens
//! fed as bytes and drawn by the scene harness (`scenes::Arms`) on the cell
//! path, the instance path and the CPU path, each frame held to the cell
//! path's bars -- the cell frame the instance frame exactly, and both the
//! CPU path's premultiplied frame, exact where no record blends and within
//! a step where one does -- on lavapipe on both memory arms, and on RADV
//! staged and on the arm its heaps allow.
//!
//! **Frames drawn by rows.** A run of steps is drawn into the window's
//! targets in turn (`damage::TARGETS`), each frame into a target that
//! holds an earlier one drawing that target's stale rows and its own change
//! ([`by_rows`]); after each, the target read back is the same frame drawn
//! whole into a new target, pixel for pixel, and its change is the one the
//! step names. Each step's frame is then held to the bars, but for a step
//! whose records the cell path leaves undrawn (an effect trail), whose
//! frame is held to the cell path drawn whole alone, the premise checked
//! that it left them undrawn.
//!
//! - [`an_undecorated_cursor_line_draws_by_rows_as_the_whole_frame_does`]:
//!   an underline and a bar cursor on a table with no decoration, moved and
//!   blinked by cursor-only frames.
//! - [`a_bell_tint_redraws_whole_on_a_reused_surface`]: a `BEL`'s tint
//!   rising, peaking, falling and gone, each frame whole, then a move by
//!   rows.
//! - [`a_cursor_only_cell_frame_draws_its_rows_and_the_whole_frames_pixels`]:
//!   cursor moves, blink flips, plasma, fire and ufo phases, trails, a
//!   row's cells rewritten and text blinking; then the cursor guide, the
//!   window looking back, the hollow cursor, the badge and the config's
//!   notice row.
//! - [`a_light_change_on_its_line_draws_the_frame_whole_on_both_paths`],
//!   a light on its own line, below the grid and above it.
//! - [`an_exit_status_draws_the_frame_whole_on_both_paths`]: an `OSC 133 ;
//!   D` between two frames, with the strip on and, the control, off.
//! - [`a_wrap_mark_draws_the_frame_whole_on_both_paths`]: a word wrap's
//!   mark coming and going in the margin.
//! - [`a_steady_strip_and_margin_draw_by_rows_on_both_paths`]: a strip and
//!   a margin that hold marks and do not change.
//! - [`a_guide_across_the_gutter_is_drawn_whole_on_both_paths`]: the cursor
//!   guide across a scroller's gutter.
//!
//! **Frames drawn whole.**
//!
//! - [`the_cell_path_draws_the_notice_as_the_instance_path_does`], the
//!   config's notice on a row of its own, with and without the status
//!   indicator above it.
//! - [`a_reversed_default_page_is_raised_alike_on_every_arm`]: an `SGR 7`
//!   bar and the `?5` page on the light theme at its shipped dim, raised to
//!   the alpha its text reads at, and the cell path's letterbox margin the
//!   page's alpha.
//! - [`the_cell_path_draws_the_status_line_as_the_instance_path_does`], the
//!   indicator and the host-writable line, Latin and Hebrew, selected and
//!   not.
//! - [`the_message_pair_holds_on_every_arm`],
//!   [`the_address_and_the_notice_take_no_colour_from_a_program`],
//!   [`no_cursor_covers_an_address_on_any_arm`]: baud's message pair under
//!   every colour a program sets, and no cursor over an address.
//! - [`the_lines_and_the_looks_are_one_picture_on_every_arm`],
//!   [`an_unhovered_line_is_amber_and_a_hovered_one_unchanged_on_every_arm`]:
//!   the link lines at rest and the three looks under the pointer.
//! - [`the_splash_draws_alike_on_every_arm`],
//!   [`the_shell_is_drawn_in_front_of_the_splash_on_every_arm`]: the launch
//!   splash's held logo, its fade and its end, and the shell's cells drawn
//!   in front of it.
//!
//! Not drawn here, each for its reason:
//!
//! - `both_arms_failing_is_not_a_pass` and
//!   `a_failed_frame_is_not_compared_against`: they inject a failed frame,
//!   and `Gpu` has no fault to inject;
//! - the instance path's frames by rows: the instance path draws a frame
//!   whole (`Pass::Whole`), so it has no rows to hold;
//! - the cursor's steps over the core's grid read in place: the cell path
//!   reads the set's copy of the window, and no other grid;
//! - the instance path's letterbox margin: the harness draws the instance
//!   frame at the whole frame's size;
//! - the splash's plasma: its records are effect records, which the cell
//!   path leaves undrawn and counts, the premise checked at each age.
//!
//! The faces are JetBrains Mono and the Noto colour emoji subset from
//! `crates/baud-font/tests/fonts`, and the Symbols Nerd Font, the bold JetBrains
//! Mono and the built face from `crates/baud-app/fonts`, each under the licence
//! beside it.

#![cfg(target_os = "linux")]

mod common;
mod scenes;

use baud_core::{Owned, Term};
use baud_domain::{Chan, Col, Row, Rows, WinSize};
use baud_font::{Face, Px as FontPx};
use baud_present_vulkan::damage::TARGETS;
use baud_present_vulkan::frame::{Gpu, Target};
use baud_present_vulkan::lend::Arm;
use baud_present_wayland::Px;
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{
    Argb, BellTint, Blink, BlinkPhases, CursorLook, CursorPhase, FaceRole, FaceStyle, Kind,
    Leaving, LineEdge, Pt, Rect, Redraw, Renderer, Segment, Shape, Smear, SplashLook, SplashPhase,
    Stack, StatusMarks, Tally, Theme, Trail, TrailLook,
};
use common::{lavapipe, radv};
use scenes::{Arms, Frames, differing, feed, jetbrains_mono, repo_file};

/// Wide enough that a path which stops short of the right-hand side shows.
const WS: WinSize = WinSize {
    ws_col: 120,
    ws_row: 10,
};

/// **A screen with every rule the cell path resolves on it**: text, SGR,
/// `38;5`, `48;5` and truecolour cells, underline styles, a set background,
/// reverse, dim and hidden runs, palette entries 0, 7, 8 and 15, box
/// drawing and braille, the procedural range, combining marks, ligature
/// pairs, and a band to the very last column above one that stops a cell
/// short of it.
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

/// A terminal of [`WS`]'s size over `owned`.
fn term(owned: &mut Owned) -> Term<'_> {
    Term::new(owned.storage(), &WS)
}

/// The text a codepoint at a time.
fn codepoints(text: &str) -> Vec<baud_domain::Codepoint> {
    text.chars()
        .map(baud_domain::Codepoint::from_char)
        .collect()
}

/// [`WS`] with a row given up, for the config's notice.
const LESS: WinSize = WinSize {
    ws_col: WS.ws_col,
    ws_row: WS.ws_row - 1,
};

// ---------------------------------------------------------------------
// Frames drawn by rows.
// ---------------------------------------------------------------------

/// What a step does to the scene.
type Act = Box<dyn Fn(&mut Arms<'_, '_, '_>, &mut Term<'_>)>;

/// What a step's frame must have changed of the one before: the positive
/// control that it was drawn by rows, so a pixel it got wrong would show.
#[derive(Clone, Copy, Debug)]
enum Want {
    /// Anything, the whole included.
    Any,
    /// Rows, any of them.
    Rows,
    /// Rows, these among them, counted from zero.
    Has(&'static [usize]),
    /// The whole.
    Whole,
}

impl Want {
    /// Whether `changed` is what this wants.
    fn met(self, changed: Redraw) -> bool {
        match (self, changed) {
            (Want::Any, _) | (Want::Rows, Redraw::Rows(_)) | (Want::Whole, Redraw::Whole) => true,
            (Want::Has(rows), Redraw::Rows(set)) => {
                rows.iter().all(|&r| set.has(Row::from_index(r)))
            }
            _ => false,
        }
    }
}

/// How a step's frame is held beside the by-rows oracle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    /// To the bars: the cell frame the instance frame, both the CPU path's.
    Bars,
    /// To the CPU path's frame, the cell frame alone: the step draws a
    /// smear or an effect trail, which the instance path does not draw.
    Trail,
}

/// One step of a by-rows run: its name, the change to the scene, what its
/// frame must have changed, and how its frame is held.
struct Step {
    name: &'static str,
    act: Act,
    want: Want,
    held: Held,
}

/// A step held to the bars.
fn step(name: &'static str, act: Act, want: Want) -> Step {
    Step {
        name,
        act,
        want,
        held: Held::Bars,
    }
}

/// The cursor moved to row `r`, column `c`, counted from one.
fn at(r: usize, c: usize) -> Act {
    Box::new(move |_, t| feed(t, format!("\x1b[{r};{c}H").as_bytes()))
}

/// The cursor's blink set to `p`.
fn blink(p: CursorPhase) -> Act {
    Box::new(move |a, _| a.renderer.set_cursor_blink(p))
}

/// The cursor at `opacity` and the plasma's glow at `glow`.
fn phase(opacity: u8, glow: u8) -> CursorPhase {
    CursorPhase {
        opacity: Chan::from_u8(opacity),
        glow: Chan::from_u8(glow),
        ..CursorPhase::HIDDEN
    }
}

/// What a by-rows run gives its renderer before it sizes the frame: what
/// widens the whole frame, the strip and the margins, cannot be turned on
/// in a step, whose targets are already that size.
type Setup = fn(&mut Renderer<'_, '_>);

/// Nothing beyond the run's own.
fn bare(_: &mut Renderer<'_, '_>) {}

/// The strip's marks on.
fn strip_on(r: &mut Renderer<'_, '_>) {
    r.set_status_marks(StatusMarks::On);
}

/// A band lent each side for the word wrap's marks.
fn margins_lent(r: &mut Renderer<'_, '_>) {
    r.set_margins(FontPx::of(16));
}

/// The strip's marks on and the margins lent.
fn strip_and_margins(r: &mut Renderer<'_, '_>) {
    strip_on(r);
    margins_lent(r);
}

/// **The frame a by-rows run draws in**: a scroller's gutter, the activity
/// lights' own line, and what the renderer is given before it sizes the
/// frame.
#[derive(Clone, Copy)]
struct Framing {
    gutter: FontPx,
    line: Option<LineEdge>,
    setup: Setup,
}

/// The plain frame: no gutter, no line, nothing set up.
const PLAIN: Framing = Framing {
    gutter: FontPx::of(0),
    line: None,
    setup: bare,
};

/// How many of a run's first frames count toward the rows-drawn bound.
#[derive(Clone, Copy)]
struct Counting(usize);

/// The frames a by-rows run drew by rows, and the rows they drew.
struct Turned {
    partial: Tally,
    rows: Tally,
}

/// The faces a by-rows run draws with: JetBrains Mono, the Noto subset and
/// the Nerd Font symbols behind it.
fn partial_faces() -> [Vec<u8>; 3] {
    [
        jetbrains_mono(),
        repo_file("crates/baud-font/tests/fonts/NotoColorEmoji-subset.ttf"),
        repo_file("crates/baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
    ]
}

/// A renderer over `faces` on a page configured at alpha 56 and drawn at
/// the alpha the dark theme's text reads at from there
/// ([`Theme::glass_reach`]).
fn partial_renderer<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    let theme = Theme {
        bg_alpha: Chan::from_u8(56),
        ..Theme::dark()
    };
    let theme = Theme {
        bg_alpha: theme.glass_reach(),
        ..theme
    };
    fresh(faces, theme)
}

/// **`steps` on `arm`, each frame drawn by rows into the target its turn
/// names** and read back against the same frame drawn whole, its change
/// held to its [`Want`], and the frame held as the step says ([`Held`]);
/// the first `counted` frames drawn by rows counted.
fn by_rows(gpu: &Gpu, arm: Arm, framing: Framing, steps: &[Step], counted: Counting) -> Turned {
    let files = partial_faces();
    let faces = files.each_ref().map(|f| Face::parse(f).expect("a face"));
    let mut r = partial_renderer(&faces);
    r.set_gutter(framing.gutter);
    r.set_lights_line(framing.line, 1.0);
    (framing.setup)(&mut r);
    let mut owned = Owned::new(&WS);
    let mut t = term(&mut owned);
    let mut arms = Arms::new(gpu, arm, r, &t);
    // The trails the steps draw, the quad and the fire, built before any
    // frame is drawn by rows.
    for fragment in [
        baud_present_vulkan::pipeline::Fragment::Quad,
        baud_present_vulkan::pipeline::Fragment::Effect(baud_present_vulkan::pipeline::Fx::Fire),
    ] {
        arms.build_trail(fragment, baud_present_vulkan::pipeline::Blend::Normal);
    }
    let (w, h) = arms.renderer.whole_size();
    let targets: Vec<Target> = (0..TARGETS)
        .map(|_| gpu.target(Px(w.value()), Px(h.value())).expect("a target"))
        .collect();
    let mut turned = Turned {
        partial: Tally::ZERO,
        rows: Tally::ZERO,
    };
    let mut wrong = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        (s.act)(&mut arms, &mut t);
        arms.build(&mut t);
        let into = &targets[i % TARGETS];
        let (apart, drawn) = arms.with_frame(&t, |d, input| {
            let drawn = d.draw(input, into);
            let got = d.gpu.read(into).expect("the target read back");
            let whole = d.whole(input);
            (differing(&got, &whole), drawn)
        });
        if apart != 0 {
            wrong.push(format!(
                "{i} {}: {apart} pixels apart from the whole frame, drawn {:?}",
                s.name, drawn.drawn
            ));
        }
        if !s.want.met(drawn.changed) {
            wrong.push(format!(
                "{i} {}: changed {:?}, wanted {:?}",
                s.name, drawn.changed, s.want
            ));
        }
        if let (Redraw::Rows(set), true) = (drawn.drawn, i < counted.0) {
            turned.partial.bump();
            turned.rows.add(Tally::of(set.len().index()));
        }
        let what = format!("{arm:?}: step {i}, {}", s.name);
        match s.held {
            Held::Bars => {
                arms.check(&mut t, &what);
            }
            Held::Trail => {
                arms.check_trail(&mut t, &what);
            }
        }
    }
    arms.with_frame(&t, |d, _| {
        for target in targets {
            d.mem.forget(d.gpu, &target);
            d.gpu.drop_target(target);
        }
    });
    assert!(wrong.is_empty(), "{arm:?}: {wrong:#?}");
    turned
}

/// The cursor's frames were drawn by rows, and each by few.
fn rows_were_few(turned: &Turned) {
    assert!(
        turned.partial.value() >= 20,
        "the cursor-only frames were drawn by rows: {} of them",
        turned.partial.value()
    );
    assert!(
        turned.rows.value() < turned.partial.value() * u64::from(WS.ws_row) / 2,
        "and each by few: {} rows over {} frames",
        turned.rows.value(),
        turned.partial.value()
    );
}

/// **The cursor's steps.** The first is a content change; most of the
/// rest change only the cursor, its blink animation or its trail, so each
/// of those frames is drawn by rows.
fn cursor_steps() -> Vec<Step> {
    let ghost = |p: f32| {
        TrailLook::Ghost(Trail {
            from: (Row::from_index(1), Col::at_index(2)),
            to: (Row::from_index(6), Col::at_index(40)),
            progress: p,
            origin_dy: 0.0,
            lift: FontPx::of(0),
            strength: baud_render::Strength::Full,
        })
    };
    let mut steps = vec![step(
        "the scene, the cursor a blinking block on row 2",
        Box::new(|_, t| {
            scene(t);
            // Rows of powerline arrows above and below the cursor's row:
            // the Nerd Font's U+E0B0 reaches two pixels past its row at
            // either end at 14 px, so a band between them is drawn with
            // both of their rows.
            for row in [2, 4] {
                feed(t, format!("\x1b[{row};1H\x1b[2K").as_bytes());
                feed(t, "\u{e0b0} ".repeat(60).as_bytes());
            }
            feed(t, b"\x1b[1 q\x1b[3;5H");
        }),
        Want::Any,
    )];
    for (r, c) in [(3, 9), (8, 30), (4, 1), (2, 3), (1, 1), (9, 119), (5, 60)] {
        steps.push(step("the cursor moved", at(r, c), Want::Any));
    }
    // A row's cells rewritten away from the cursor, in glyphs and a
    // rendition the screen already shows, the cursor saved and put back.
    steps.push(step(
        "a row's cells rewritten, the cursor where it was",
        Box::new(|_, t| feed(t, b"\x1b7\x1b[8;1Hworld\x1b8")),
        Want::Any,
    ));
    for p in [
        CursorPhase::HIDDEN,
        CursorPhase::SHOWN,
        CursorPhase::HIDDEN,
        CursorPhase::SHOWN,
    ] {
        steps.push(step("the blink flipped", blink(p), Want::Any));
    }
    for g in [128, 192, 64, 255] {
        steps.push(step("a plasma phase", blink(phase(0, g)), Want::Any));
    }
    for k in [70, 80, 135, 205] {
        steps.push(step(
            "a fire step",
            blink(CursorPhase::fire(Chan::from_u8(k))),
            Want::Any,
        ));
    }
    for k in [10, 70, 128, 190] {
        steps.push(step(
            "a ufo step",
            blink(CursorPhase::ufo(Chan::from_u8(k))),
            Want::Any,
        ));
    }
    steps.push(step(
        "the blink shown again",
        blink(CursorPhase::SHOWN),
        Want::Any,
    ));
    for p in [0.1, 0.3, 0.6, 0.9] {
        steps.push(step(
            "a trail step",
            Box::new(move |a, _| a.renderer.set_trail(Some(ghost(p)))),
            Want::Any,
        ));
    }
    steps.push(step(
        "the trail gone",
        Box::new(|a, _| a.renderer.set_trail(None)),
        Want::Any,
    ));
    // A quad trail arriving at the cursor from row 7, and the fire trail's
    // particles twice: from the top row, where they rise past the grid, and
    // across the middle of the screen.
    for p in [0.05f32, 0.2, 0.4, 0.6] {
        steps.push(Step {
            name: "a quad trail step",
            act: Box::new(move |a, t| {
                let (cr, cc) = t.cursor();
                let hole = a.renderer.cursor_rect(cr, cc, t.cursor_style());
                let from = a
                    .renderer
                    .cursor_rect(Row::from_index(7), Col::at_index(60), t.cursor_style())
                    .corners();
                a.renderer.set_trail(Some(TrailLook::Smear(Smear {
                    corners: Segment::new(from, hole).at(p),
                    hole,
                    shape: Shape::Quad,
                    strength: baud_render::Strength::Full,
                    opacity: Smear::OPAQUE,
                })));
            }),
            want: Want::Any,
            held: Held::Trail,
        });
    }
    for ((c0, r0), (c1, r1), ages) in [
        (
            (98.0f32, 0.0f32),
            (116.0f32, 1.0f32),
            &[0.04f32, 0.12, 0.25, 0.4][..],
        ),
        (
            (60.0, 4.0),
            (100.0, 6.0),
            &[0.04, 0.12, 0.25, 0.4, 0.55, 0.7, 0.85],
        ),
    ] {
        for &age in ages {
            steps.push(Step {
                name: "a fire trail step",
                act: Box::new(move |a, _| {
                    a.renderer
                        .set_trail(Some(fire_trail(a, ((c0, r0), (c1, r1)), age)));
                }),
                want: Want::Any,
                held: Held::Trail,
            });
        }
    }
    steps.push(step(
        "the effect gone",
        Box::new(|a, _| a.renderer.set_trail(None)),
        Want::Any,
    ));
    steps.push(step(
        "text written, some of it blinking",
        Box::new(|_, t| feed(t, b"\x1b[7;1Hmore \x1b[5mtext\x1b[25m\x1b[5;60H")),
        Want::Any,
    ));
    // Not the cursor's: the blinking text's phase, which the cell shader
    // hides by a uniform.
    for slow in [Blink::Hidden, Blink::Shown] {
        steps.push(step(
            "the text's blink flipped",
            Box::new(move |a, _| {
                a.renderer.set_blink(BlinkPhases {
                    slow,
                    rapid: Blink::Shown,
                })
            }),
            Want::Any,
        ));
    }
    for (r, c) in [(5, 61), (5, 62), (6, 1), (2, 2)] {
        steps.push(step("the cursor moved after text", at(r, c), Want::Any));
    }
    steps
}

/// **The fire trail from cell `(c0, r0)` to cell `(c1, r1)` at `age`** of
/// its life, the emitter a quad between the two.
fn fire_trail(
    a: &Arms<'_, '_, '_>,
    ((c0, r0), (c1, r1)): ((f32, f32), (f32, f32)),
    age: f32,
) -> TrailLook {
    let m = a.renderer.metrics();
    let (cw, ch) = (m.cell_w.value() as f32, m.cell_h.value() as f32);
    let cell = |c: f32, r: f32| Rect {
        left: c * cw,
        top: r * ch,
        right: (c + 1.0) * cw,
        bottom: (r + 1.0) * ch,
    };
    let (from, to) = (cell(c0, r0), cell(c1, r1));
    let centre = |r: Rect| Pt {
        x: (r.left + r.right) * 0.5,
        y: (r.top + r.bottom) * 0.5,
    };
    let (fa, ta) = (centre(from), centre(to));
    let l = (ta.x - fa.x).hypot(ta.y - fa.y);
    let ignite = Ignition::ignite(fa, to, Seed(7), ch);
    let life = (Effect::Fire.end(&ignite) / Effect::Fire.life()).min(1.0);
    let age = age * life;
    TrailLook::Vfx(Vfx {
        effect: Effect::Fire,
        emitter: Smear {
            corners: Segment::new(from.corners(), to).at(age),
            hole: to,
            shape: Shape::Quad,
            strength: baud_render::Strength::Full,
            opacity: Smear::OPAQUE,
        },
        ignition: ignite.at(age),
        axis: Pt {
            x: (ta.x - fa.x) / l,
            y: (ta.y - fa.y) / l,
        },
    })
}

/// The cursor guide's look.
fn guide() -> Option<baud_render::GuideLook> {
    Some(baud_render::GuideLook {
        colour: baud_render::CURSOR_GUIDE,
        line: FontPx::of(2),
    })
}

/// A badge of `text`.
fn badge(text: &str) -> baud_render::BadgeLook {
    baud_render::BadgeLook {
        text: text.into(),
        colour: baud_render::BADGE,
        outline: Theme::dark().bg.with_alpha(baud_render::DRAWN_AT),
        drawn_at: baud_render::DRAWN_AT,
        fraction: baud_render::BOX,
        top: FontPx::of(4),
        right: FontPx::of(10),
        scale: 1.0,
        style: FaceStyle::Bold,
    }
}

/// **The steps where what moves beside the cursor is drawn**: the cursor
/// guide on the cursor's row, moved with a hidden cursor and on the
/// cursor's line while the window looks back, and under a hollow cursor;
/// the badge, with a block cursor moved into it and blinking there; and
/// the config's notice row below the grid, with the cursor on the row
/// above it among glyphs that reach into it. Each step that changes only
/// those names the rows its frame must change.
fn ground_steps() -> Vec<Step> {
    vec![
        step(
            "the cursor guide on",
            Box::new(|a, _| a.renderer.set_cursor_guide(guide())),
            Want::Has(&[1]),
        ),
        step(
            "the cursor moved under the guide",
            at(6, 10),
            Want::Has(&[1, 5]),
        ),
        step(
            "the blink hidden under the guide",
            blink(CursorPhase::HIDDEN),
            Want::Has(&[5]),
        ),
        // No cursor word changes: only the guide's rows.
        step(
            "the hidden cursor moved under the guide",
            at(8, 3),
            Want::Has(&[5, 7]),
        ),
        step(
            "the blink shown under the guide",
            blink(CursorPhase::SHOWN),
            Want::Has(&[7]),
        ),
        step(
            "lines scrolled into the history",
            Box::new(|_, t| feed(t, b"\x1b[10;1H\n\n\n\n\x1b[8;3H")),
            Want::Any,
        ),
        step(
            "looking back with the guide",
            Box::new(|a, _| a.back = Rows::of_count(3)),
            Want::Any,
        ),
        // The live cursor's line, three rows down the window, takes the
        // guide; no cursor is drawn.
        step(
            "the live cursor moved, looking back",
            at(4, 6),
            Want::Has(&[6]),
        ),
        step("and again, looking back", at(2, 6), Want::Has(&[4, 6])),
        step("a third time, looking back", at(5, 8), Want::Has(&[4, 7])),
        step("a fourth time, looking back", at(3, 2), Want::Has(&[5, 7])),
        step(
            "the live screen again",
            Box::new(|a, _| a.back = Rows::ZERO),
            Want::Any,
        ),
        step(
            "the hollow cursor under the guide",
            Box::new(|a, _| a.renderer.set_cursor_look(CursorLook::Hollow)),
            Want::Has(&[2]),
        ),
        step("the hollow cursor moved", at(4, 7), Want::Has(&[2, 3])),
        step(
            "the shaped cursor again",
            Box::new(|a, _| a.renderer.set_cursor_look(CursorLook::Shaped)),
            Want::Has(&[3]),
        ),
        step(
            "the cursor guide off",
            Box::new(|a, _| a.renderer.set_cursor_guide(None)),
            Want::Has(&[3]),
        ),
        step(
            "the badge set",
            Box::new(|a, _| a.renderer.set_badge(Some(badge("prod-db-01\nmain")))),
            Want::Any,
        ),
        step(
            "a block cursor moved into the badge",
            Box::new(|a, t| {
                let rect = a.renderer.badge().iter().fold(
                    [i32::MAX, i32::MAX, i32::MIN, i32::MIN],
                    |r, i| {
                        let (x, y) = (i32::from(i.x.0), i32::from(i.y.0));
                        [
                            r[0].min(x),
                            r[1].min(y),
                            r[2].max(x + i32::from(i.w.0)),
                            r[3].max(y + i32::from(i.h.0)),
                        ]
                    },
                );
                assert!(rect[0] < rect[2], "the badge was drawn");
                let m = a.renderer.metrics();
                let (cw, ch) = (
                    i32::try_from(m.cell_w.value()).expect("a width"),
                    i32::try_from(m.cell_h.value()).expect("a height"),
                );
                let (row, col) = ((rect[1] + rect[3]) / 2 / ch, (rect[0] + rect[2]) / 2 / cw);
                feed(t, format!("\x1b[{};{}H", row + 1, col + 1).as_bytes());
            }),
            Want::Rows,
        ),
        step(
            "the blink hidden in the badge",
            blink(CursorPhase::HIDDEN),
            Want::Rows,
        ),
        step(
            "the blink shown in the badge",
            blink(CursorPhase::SHOWN),
            Want::Rows,
        ),
        step("the cursor moved out of the badge", at(9, 5), Want::Rows),
        step(
            "the badge changed",
            Box::new(|a, _| a.renderer.set_badge(Some(badge("staging\nmain")))),
            Want::Any,
        ),
        // Two frames that leave the badge as it is, so the target its
        // clearing goes to holds it.
        step(
            "the cursor moved under the new badge",
            at(7, 5),
            Want::Has(&[6, 8]),
        ),
        step("and moved again", at(6, 5), Want::Has(&[5, 6])),
        step(
            "the badge cleared",
            Box::new(|a, _| a.renderer.set_badge(None)),
            Want::Any,
        ),
        step(
            "the notice shown, arrows on the row above it",
            Box::new(|a, t| {
                let _ = t.resize(&LESS);
                a.renderer.set_notice(Some(codepoints(
                    "~/.config/baud/config.toml line 12: \"font_size 15\" is not key = value",
                )));
                a.renderer.fit(t);
                feed(t, b"\x1b[9;1H\x1b[2K");
                feed(t, "\u{e0b0} ".repeat(60).as_bytes());
                feed(t, b"\x1b[5;5H");
            }),
            Want::Any,
        ),
        // The arrows reach into the notice's row: its band is drawn.
        step(
            "the cursor moved onto the arrows above the notice",
            at(9, 4),
            Want::Has(&[8, 9]),
        ),
        step(
            "the blink hidden above the notice",
            blink(CursorPhase::HIDDEN),
            Want::Has(&[9]),
        ),
        step(
            "the blink shown above the notice",
            blink(CursorPhase::SHOWN),
            Want::Has(&[9]),
        ),
        // Away from the notice, so the frames before its change hold no
        // row of it.
        step(
            "the cursor moved away from the notice",
            at(3, 3),
            Want::Has(&[8, 9]),
        ),
        step("the cursor moved again", at(4, 4), Want::Has(&[2, 3])),
        step("and again", at(5, 5), Want::Has(&[3, 4])),
        step(
            "the notice changed",
            Box::new(|a, _| {
                a.renderer.set_notice(Some(codepoints(
                    "~/.config/baud/config.toml line 3: a list never closed",
                )))
            }),
            Want::Any,
        ),
        step(
            "the cursor moved above the changed notice",
            at(9, 30),
            Want::Has(&[9]),
        ),
        step(
            "the notice gone",
            Box::new(|a, t| {
                a.renderer.set_notice(None);
                let _ = t.resize(&WS);
                a.renderer.fit(t);
            }),
            Want::Any,
        ),
        step("the cursor moved after the notice", at(3, 3), Want::Rows),
    ]
}

/// **The cursor's line alone, drawn by rows, on `arm`**: an underline and
/// a bar cursor on a table with no decoration, moved and blinked by
/// cursor-only frames -- the one instance of the shape pass held to each
/// band's scissor as the whole pass would be.
fn undecorated_cursor_line(gpu: &Gpu, arm: Arm) {
    for shape in ["\x1b[4 q", "\x1b[6 q"] {
        let mut steps = vec![step(
            "plain text, the cursor a line",
            Box::new(move |_, t| {
                feed(
                    t,
                    b"\x1b[?25h\x1b[2;1HThe quick brown fox jumps over 0123456789",
                );
                feed(
                    t,
                    b"\x1b[5;1H\x1b[32mthe lazy dog\x1b[0m, again: ABCDEFGHIJ",
                );
                feed(t, format!("{shape}\x1b[3;5H").as_bytes());
            }),
            Want::Any,
        )];
        let cells = [
            (3, 9),
            (8, 30),
            (4, 1),
            (2, 3),
            (1, 1),
            (9, 119),
            (5, 60),
            (10, 2),
        ];
        for (r, c) in cells.iter().copied().chain(cells.iter().copied().rev()) {
            steps.push(step("the cursor moved", at(r, c), Want::Rows));
        }
        for p in [CursorPhase::HIDDEN, CursorPhase::SHOWN].repeat(3) {
            steps.push(step("the blink flipped", blink(p), Want::Rows));
        }
        steps.push(step(
            "the table still holds no decoration",
            Box::new(|a, t| {
                assert_eq!(
                    a.stamp().decorations(),
                    baud_render::Decorations::Absent,
                    "the premise: the shape pass draws the cursor's line alone"
                );
                feed(t, b"\x1b[6;7H");
            }),
            Want::Rows,
        ));
        let n = steps.len();
        rows_were_few(&by_rows(gpu, arm, PLAIN, &steps, Counting(n)));
    }
}

#[test]
fn an_undecorated_cursor_line_draws_by_rows_as_the_whole_frame_does() {
    let gpu = lavapipe();
    undecorated_cursor_line(&gpu, Arm::InPlace);
    undecorated_cursor_line(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn an_undecorated_cursor_line_draws_by_rows_as_the_whole_frame_does_on_radv() {
    let gpu = radv();
    undecorated_cursor_line(&gpu, Arm::Staging);
    undecorated_cursor_line(&gpu, gpu.arm(1));
}

/// **A `BEL`'s tint redraws the whole frame on a surface drawn before, on
/// `arm`**: its solid changes with no cell, so each frame of its fade, and
/// the one it leaves, is drawn whole into the target its turn names; a
/// cursor move after it is drawn by rows again.
fn bell_tint_whole(gpu: &Gpu, arm: Arm) {
    let tint = |alpha: Option<u8>| -> Act {
        Box::new(move |a, _| {
            a.renderer.set_bell_tint(alpha.map(|v| BellTint {
                colour: Argb(0xFFFF_0000),
                alpha: Chan::from_u8(v),
            }))
        })
    };
    let steps = [
        step(
            "plain text",
            Box::new(|_, t| {
                feed(
                    t,
                    b"\x1b[?25h\x1b[2;1HThe quick brown fox jumps over 0123456789\x1b[3;5H",
                );
            }),
            Want::Any,
        ),
        step("the tint rises", tint(Some(36)), Want::Whole),
        step("the tint peaks", tint(Some(153)), Want::Whole),
        step("the tint falls", tint(Some(36)), Want::Whole),
        step("the tint goes", tint(None), Want::Whole),
        step("the cursor moved", at(4, 9), Want::Rows),
    ];
    by_rows(gpu, arm, PLAIN, &steps, Counting(0));
}

#[test]
fn a_bell_tint_redraws_whole_on_a_reused_surface() {
    let gpu = lavapipe();
    bell_tint_whole(&gpu, Arm::InPlace);
    bell_tint_whole(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_bell_tint_redraws_whole_on_a_reused_surface_on_radv() {
    let gpu = radv();
    bell_tint_whole(&gpu, Arm::Staging);
    bell_tint_whole(&gpu, gpu.arm(1));
}

/// **A cursor-only frame draws only its rows, and every frame is the
/// whole frame's pixels, on `arm`**: over the glass, through cursor
/// moves, blink flips, plasma, fire and ufo phases and trail steps, and
/// after a content change; and with the cursor guide, the badge and the
/// config's notice row ([`ground_steps`]).
fn cursor_only(gpu: &Gpu, arm: Arm) {
    let cursor = cursor_steps();
    let counted = cursor.len();
    let steps: Vec<Step> = cursor.into_iter().chain(ground_steps()).collect();
    rows_were_few(&by_rows(gpu, arm, PLAIN, &steps, Counting(counted)));
}

#[test]
fn a_cursor_only_cell_frame_draws_its_rows_and_the_whole_frames_pixels() {
    let gpu = lavapipe();
    cursor_only(&gpu, Arm::InPlace);
    cursor_only(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_cursor_only_cell_frame_draws_its_rows_and_the_whole_frames_pixels_on_radv() {
    let gpu = radv();
    cursor_only(&gpu, Arm::Staging);
    cursor_only(&gpu, gpu.arm(1));
}

/// **A light that changes on its own line draws the frame whole, and a
/// cursor move beside it only its rows, on `arm`**, the line below the
/// grid and above it: the line is outside every row's band, so a frame
/// that kept the target's pixels there would show the light as it was.
fn light_change(gpu: &Gpu, arm: Arm) {
    use baud_render::{ByLed, Glow, Light, Shown};
    let lit = |rd: Light| -> Act {
        Box::new(move |a, _| {
            a.renderer
                .set_lights(ByLed { rd, ..Shown::DARK }, Glow::Full)
        })
    };
    for edge in [LineEdge::Bottom, LineEdge::Top] {
        let mut steps: Vec<Step> = cursor_steps().into_iter().take(1).collect();
        steps.extend([
            step("the cursor moved", at(3, 4), Want::Rows),
            step("RD lit", lit(Light::Lit), Want::Whole),
            step("the cursor moved beside it", at(5, 6), Want::Rows),
            step("RD dark", lit(Light::Dark), Want::Whole),
            step("and the cursor again", at(2, 9), Want::Rows),
        ]);
        let framing = Framing {
            line: Some(edge),
            ..PLAIN
        };
        by_rows(gpu, arm, framing, &steps, Counting(0));
    }
}

#[test]
fn a_light_change_on_its_line_draws_the_frame_whole_on_both_paths() {
    let gpu = lavapipe();
    light_change(&gpu, Arm::InPlace);
    light_change(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_light_change_on_its_line_draws_the_frame_whole_on_both_paths_on_radv() {
    let gpu = radv();
    light_change(&gpu, Arm::Staging);
    light_change(&gpu, gpu.arm(1));
}

/// How many marks the window holds now, the strip's and the margins',
/// placed as every frame places them: what a step's premise reads.
fn marks_now(a: &mut Arms<'_, '_, '_>, t: &Term<'_>) -> (usize, usize) {
    a.renderer
        .place_marks(t, a.back, Leaving::None, FontPx::of(0));
    (
        a.renderer.strip_records().len(),
        a.renderer.margin_records().len(),
    )
}

/// Two commands at marked prompts, the first ended in failure and the
/// second still running: one mark in the strip, a cross on the first
/// prompt's row.
const TWO_COMMANDS: &[u8] = b"\x1b[H\x1b[2J\
    \x1b]133;A\x07$ \x1b]133;B\x07bad\x1b]133;C\x07\r\nerr\r\n\x1b]133;D;1\x07\
    \x1b]133;A\x07$ \x1b]133;B\x07bad\x1b]133;C\x07\r\nerr\r\n";

/// **An exit status that arrives between two frames draws the frame
/// whole, and a cursor move beside the strip only its rows, on `arm`**.
/// The status (`OSC 133 ; D`) writes no cell and moves no cursor, so only
/// the strip's records change, and the strip is outside every row's band.
/// The control: the same frames with the strip off draw the status's by
/// rows. The second command fails as the first did, so its cross's mask is
/// already in the atlas, and the premise holds the atlas's generation
/// still across the status.
fn exit_status(gpu: &Gpu, arm: Arm) {
    for (marks, setup) in [
        (StatusMarks::Off, bare as Setup),
        (StatusMarks::On, strip_on as Setup),
    ] {
        let (ended, premise) = match marks {
            StatusMarks::On => (Want::Whole, (1, 2)),
            StatusMarks::Off => (Want::Rows, (0, 0)),
        };
        let steps = [
            step(
                "two commands, the first ended, the second running",
                Box::new(|_, t| feed(t, TWO_COMMANDS)),
                Want::Any,
            ),
            step("the cursor moved", at(6, 4), Want::Rows),
            step(
                "the second command failed",
                Box::new(move |a, t| {
                    let was = marks_now(a, t).0;
                    let atlas = a.renderer.atlas().generation();
                    feed(t, b"\x1b]133;D;2\x07");
                    let now = marks_now(a, t).0;
                    assert_eq!((was, now), premise, "the premise: the strip's marks");
                    assert_eq!(
                        a.renderer.atlas().generation(),
                        atlas,
                        "the premise: the mark's mask was held, so the atlas did not move"
                    );
                }),
                ended,
            ),
            step("the cursor moved beside the strip", at(7, 9), Want::Rows),
            step("and again", at(5, 2), Want::Rows),
        ];
        let framing = Framing { setup, ..PLAIN };
        by_rows(gpu, arm, framing, &steps, Counting(0));
    }
}

#[test]
fn an_exit_status_draws_the_frame_whole_on_both_paths() {
    let gpu = lavapipe();
    exit_status(&gpu, Arm::InPlace);
    exit_status(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn an_exit_status_draws_the_frame_whole_on_both_paths_on_radv() {
    let gpu = radv();
    exit_status(&gpu, Arm::Staging);
    exit_status(&gpu, gpu.arm(1));
}

/// A paragraph the terminal cuts inside a word, its 120th column the
/// third letter of the fourteenth: drawn as the terminal cut it until a
/// prompt marked after it finishes it.
const CUT_PARAGRAPH: &str = "\
    alphabet alphabet alphabet alphabet alphabet alphabet alphabet \
    alphabet alphabet alphabet alphabet alphabet alphabet alphabet \
    alphabet alphabet alphabet alphabet alphabet alphabet \r\n";

/// **A word wrap's mark that comes or goes between two frames draws the
/// frame whole, and a cursor move beside the margins only its rows, on
/// `arm`**: a paragraph cut inside a word is finished once the shell marks
/// its prompt, its second line marked in the left margin; the output zone
/// that follows makes it live again, and the mark goes.
fn wrap_mark(gpu: &Gpu, arm: Arm) {
    let steps = [
        step(
            "a paragraph cut inside a word, no prompt marked",
            Box::new(|_, t| {
                feed(t, b"\x1b[H\x1b[2J");
                feed(t, CUT_PARAGRAPH.as_bytes());
            }),
            Want::Any,
        ),
        step("the cursor moved", at(6, 4), Want::Rows),
        step(
            "the prompt marked, the paragraph above it finished",
            Box::new(|a, t| {
                let was = marks_now(a, t).1;
                feed(t, b"\x1b]133;A\x07");
                let now = marks_now(a, t).1;
                assert!(
                    was == 0 && now > 0,
                    "the premise: the margin marked, {was} marks then {now}"
                );
            }),
            Want::Whole,
        ),
        step("the cursor moved beside the margin", at(7, 9), Want::Rows),
        step(
            "the output zone, the paragraph live again",
            Box::new(|a, t| {
                let was = marks_now(a, t).1;
                feed(t, b"\x1b]133;C\x07");
                let now = marks_now(a, t).1;
                assert!(
                    was > 0 && now == 0,
                    "the premise: the margin's mark gone, {was} marks then {now}"
                );
            }),
            Want::Whole,
        ),
        step("and the cursor again", at(5, 2), Want::Rows),
    ];
    let framing = Framing {
        setup: margins_lent,
        ..PLAIN
    };
    by_rows(gpu, arm, framing, &steps, Counting(0));
}

#[test]
fn a_wrap_mark_draws_the_frame_whole_on_both_paths() {
    let gpu = lavapipe();
    wrap_mark(&gpu, Arm::InPlace);
    wrap_mark(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_wrap_mark_draws_the_frame_whole_on_both_paths_on_radv() {
    let gpu = radv();
    wrap_mark(&gpu, Arm::Staging);
    wrap_mark(&gpu, gpu.arm(1));
}

/// **A strip and a margin that hold marks and do not change draw a cursor
/// move by its rows, on `arm`**: a command ended at a marked prompt, a
/// paragraph cut inside a word after it and finished by the next prompt,
/// so the strip holds a mark and the margin another.
fn steady_strip_and_margin(gpu: &Gpu, arm: Arm) {
    let steps = [
        step(
            "a command ended, a finished paragraph, the next prompt",
            Box::new(|_, t| {
                feed(
                    t,
                    b"\x1b[H\x1b[2J\x1b]133;A\x07$ \x1b]133;B\x07cat\x1b]133;C\x07\r\n",
                );
                feed(t, CUT_PARAGRAPH.as_bytes());
                feed(t, b"\x1b]133;D;0\x07\x1b]133;A\x07$ ");
            }),
            Want::Any,
        ),
        step(
            "the cursor moved",
            Box::new(|a, t| {
                feed(t, b"\x1b[6;4H");
                let (strip, margin) = marks_now(a, t);
                assert!(
                    strip > 0 && margin > 0,
                    "the premise: both hold marks, {strip} and {margin}"
                );
            }),
            Want::Rows,
        ),
        step("and again", at(7, 9), Want::Rows),
        step("and again", at(5, 2), Want::Rows),
        step("and again", at(8, 30), Want::Rows),
    ];
    let framing = Framing {
        setup: strip_and_margins,
        ..PLAIN
    };
    by_rows(gpu, arm, framing, &steps, Counting(0));
}

#[test]
fn a_steady_strip_and_margin_draw_by_rows_on_both_paths() {
    let gpu = lavapipe();
    steady_strip_and_margin(&gpu, Arm::InPlace);
    steady_strip_and_margin(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_steady_strip_and_margin_draw_by_rows_on_both_paths_on_radv() {
    let gpu = radv();
    steady_strip_and_margin(&gpu, Arm::Staging);
    steady_strip_and_margin(&gpu, gpu.arm(1));
}

/// **A guide across a scroller's gutter is drawn whole, on `arm`**: the
/// gutter is outside every band, so a frame that moves the guide there
/// draws every pixel; with the guide off, the cursor moves by rows.
fn guide_across_gutter(gpu: &Gpu, arm: Arm) {
    let mut steps: Vec<Step> = cursor_steps().into_iter().take(3).collect();
    steps.extend([
        step(
            "the cursor guide on",
            Box::new(|a, _| a.renderer.set_cursor_guide(guide())),
            Want::Whole,
        ),
        step("the cursor moved under the guide", at(6, 10), Want::Whole),
        step("and again", at(3, 4), Want::Whole),
        step(
            "the blink hidden under the guide",
            blink(CursorPhase::HIDDEN),
            Want::Whole,
        ),
        step(
            "the hidden cursor moved under the guide",
            at(8, 3),
            Want::Whole,
        ),
        step("and again", at(2, 7), Want::Whole),
        step(
            "the guide off, the blink shown",
            Box::new(|a, _| {
                a.renderer.set_cursor_guide(None);
                a.renderer.set_cursor_blink(CursorPhase::SHOWN);
            }),
            Want::Whole,
        ),
        step("the cursor moved", at(5, 5), Want::Rows),
        step("and again", at(6, 6), Want::Rows),
    ]);
    let framing = Framing {
        gutter: FontPx::of(16),
        ..PLAIN
    };
    by_rows(gpu, arm, framing, &steps, Counting(0));
}

#[test]
fn a_guide_across_the_gutter_is_drawn_whole_on_both_paths() {
    let gpu = lavapipe();
    guide_across_gutter(&gpu, Arm::InPlace);
    guide_across_gutter(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_guide_across_the_gutter_is_drawn_whole_on_both_paths_on_radv() {
    let gpu = radv();
    guide_across_gutter(&gpu, Arm::Staging);
    guide_across_gutter(&gpu, gpu.arm(1));
}

// ---------------------------------------------------------------------
// Frames drawn whole.
// ---------------------------------------------------------------------

/// **The config's notice, on `arm`**: a row of its own after the screen's,
/// drawn over by the notice's field with baud's own overlays, with and
/// without the status indicator above it; each scene the first frame of a
/// renderer of its own. The control: the notice reaches the pixels; and
/// with it gone, the frame is its old size.
fn notice(gpu: &Gpu, arm: Arm) {
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("JetBrains Mono")];
    let bare = {
        let mut owned = Owned::new(&WS);
        let mut t = term(&mut owned);
        let mut arms = Arms::new(gpu, arm, fresh(&faces, Theme::dark()), &t);
        scene(&mut t);
        arms.check(&mut t, &format!("{arm:?}: no notice")).instance
    };
    let text =
        codepoints("~/.config/baud/config.toml line 12: \"font_size 15\" is not key = value");
    let opens = [
        (b"".as_slice(), "the notice"),
        (b"\x1b[1$~".as_slice(), "the notice under the indicator"),
    ];
    for k in 0..opens.len() {
        let name = format!("{arm:?}: {}", opens[k].1);
        let mut owned = Owned::new(&WS);
        let mut t = term(&mut owned);
        let mut arms = Arms::new(gpu, arm, fresh(&faces, Theme::dark()), &t);
        scene(&mut t);
        let _ = t.resize(&LESS);
        arms.renderer.set_notice(Some(text.clone()));
        for (open, _) in &opens[..=k] {
            feed(&mut t, open);
        }
        let f = arms.check(&mut t, &name);
        assert_ne!(f.instance, bare, "{name}: the notice reached the pixels");
        if k + 1 == opens.len() {
            feed(&mut t, b"\x1b[0$~");
            arms.renderer.set_notice(None);
            let _ = t.resize(&WS);
            let after = arms.check(&mut t, &format!("{name}, gone"));
            assert_eq!(after.instance.len(), bare.len(), "the notice's row is gone");
        }
    }
}

#[test]
fn the_cell_path_draws_the_notice_as_the_instance_path_does() {
    let gpu = lavapipe();
    notice(&gpu, Arm::InPlace);
    notice(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn the_cell_path_draws_the_notice_as_the_instance_path_does_on_radv() {
    let gpu = radv();
    notice(&gpu, Arm::Staging);
    notice(&gpu, gpu.arm(1));
}

/// **A reversed default page is raised alike on every path, on `arm`**:
/// the bar `SGR 7` draws in the default colours and the `?5` screen, on
/// the light theme at its shipped dim, 153, where each is drawn at the
/// lowest alpha its text reads at 4.5:1 over black to white, 173
/// (`Theme::reversed_alpha`). The default page beside the bar is the
/// control: it keeps the dim. Letterboxed, the cell path's margin is the
/// default page's alpha.
fn reversed_page(gpu: &Gpu, arm: Arm) {
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("JetBrains Mono")];
    let theme = Theme {
        bg_alpha: Chan::from_u8(153),
        ..Theme::light()
    };
    let raised = u32::from(theme.reversed_alpha(theme.fg, theme.bg).get());
    assert_eq!(raised, 173, "the premise: the light theme's raise");
    let mut owned = Owned::new(&WS);
    let mut t = term(&mut owned);
    let mut arms = Arms::new(gpu, arm, fresh(&faces, theme), &t);
    let m = arms.renderer.metrics();
    let side = |p: FontPx| usize::try_from(p.value()).expect("a size");
    let (cw, ch) = (side(m.cell_w), side(m.cell_h));
    let fw = side(arms.renderer.whole_size().0);
    // The middle of cell `(row, col)`, a space.
    let page =
        |px: &[u32], row: usize, col: usize| px[(row * ch + ch / 2) * fw + col * cw + cw / 2];
    let scenes: [(&str, &[u8], u32); 2] = [
        (
            "an SGR 7 bar",
            b"\x1b[7m status \x1b[0m\x1b[6;1HThe quick brown fox jumps over 0123456789\x1b[10;120H",
            153,
        ),
        (
            "the ?5 page",
            b"\x1bc\x1b[?5h page \x1b[6;1HThe quick brown fox jumps over 0123456789\x1b[10;120H",
            raised,
        ),
    ];
    for (what, bytes, row_two) in scenes {
        let what = format!("{arm:?}: {what}");
        feed(&mut t, bytes);
        let f = arms.check(&mut t, &what);
        assert_eq!(page(&f.instance, 0, 0) >> 24, raised, "{what}: raised");
        assert_eq!(
            page(&f.instance, 2, 4) >> 24,
            row_two,
            "{what}: the page below"
        );
        // Letterboxed: the margin around the frame is the default page.
        let boxed = arms.with_frame(&t, |d, input| {
            let (w, h) = d.renderer.whole_size();
            let target = d
                .gpu
                .target(Px(w.value() + 8), Px(h.value() + 6))
                .expect("a target");
            d.draw(input, &target);
            let got = d.gpu.read(&target).expect("the target read back");
            d.mem.forget(d.gpu, &target);
            d.gpu.drop_target(target);
            got
        });
        assert_eq!(boxed[0] >> 24, row_two, "{what}: the margin is the page");
    }
}

#[test]
fn a_reversed_default_page_is_raised_alike_on_every_arm() {
    let gpu = lavapipe();
    reversed_page(&gpu, Arm::InPlace);
    reversed_page(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn a_reversed_default_page_is_raised_alike_on_every_arm_on_radv() {
    let gpu = radv();
    reversed_page(&gpu, Arm::Staging);
    reversed_page(&gpu, gpu.arm(1));
}

/// **The status line, on `arm`**: its row after the screen's and its
/// glyphs drawn by records, the indicator in reverse video and the
/// host-writable line with its cursor on it, Latin and Hebrew laid out
/// from the right edge, selected and not; each scene the first frame of a
/// renderer of its own. The control: the frame grew a row.
fn status_line(gpu: &Gpu, arm: Arm) {
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("JetBrains Mono")];
    let bare = {
        let mut owned = Owned::new(&WS);
        let mut t = term(&mut owned);
        let mut arms = Arms::new(gpu, arm, fresh(&faces, Theme::dark()), &t);
        scene(&mut t);
        feed(&mut t, b"\x1b[2 q");
        arms.check(&mut t, &format!("{arm:?}: no status line"))
            .instance
    };
    let opens = [
        (b"\x1b[1$~".as_slice(), "the indicator"),
        (
            b"\x1b[2$~\x1b[1$}mail: \x1b[1;31m3\x1b[0m".as_slice(),
            "the host-writable line, selected",
        ),
        (b"\x1b[0$}".as_slice(), "the host-writable line"),
        // Hebrew on a red ground, laid out from the right edge: its
        // backgrounds from the copy and its glyphs from the records agree
        // only if both are in the line's drawn order.
        (
            "\x1b[1$}\x1b[2K\x1b[41m\u{5E9}\u{5DC}\u{5D5}\u{5DD}\x1b[0m 42\x1b[0$}".as_bytes(),
            "a Hebrew host-writable line",
        ),
        (
            "\x1b[1$}\x1b[2K\x1b[41m\u{5E9}\u{5DC}\u{5D5}\u{5DD}\x1b[0m 42".as_bytes(),
            "a Hebrew host-writable line, selected",
        ),
    ];
    for k in 0..opens.len() {
        let name = format!("{arm:?}: {}", opens[k].1);
        let mut owned = Owned::new(&WS);
        let mut t = term(&mut owned);
        let mut arms = Arms::new(gpu, arm, fresh(&faces, Theme::dark()), &t);
        scene(&mut t);
        feed(&mut t, b"\x1b[2 q");
        for (open, _) in &opens[..=k] {
            feed(&mut t, open);
        }
        let f = arms.check(&mut t, &name);
        assert!(
            f.instance.len() > bare.len(),
            "{name}: the frame grew a row"
        );
    }
}

#[test]
fn the_cell_path_draws_the_status_line_as_the_instance_path_does() {
    let gpu = lavapipe();
    status_line(&gpu, Arm::InPlace);
    status_line(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn the_cell_path_draws_the_status_line_as_the_instance_path_does_on_radv() {
    let gpu = radv();
    status_line(&gpu, Arm::Staging);
    status_line(&gpu, gpu.arm(1));
}

/// The face the message pair's scenes draw with: the built face the app
/// ships.
fn built_face() -> Vec<u8> {
    repo_file("crates/baud-app/fonts/built/Regular.ttf")
}

/// **Text enough that the glyph map holds the scene's**: a line on row 7,
/// which no message, address or line check reads, written with the cursor
/// saved and put back.
const ENOUGH_GLYPHS: &[u8] = b"\x1b7\x1b[7;3HThe quick brown fox jumps 0123456789\x1b8";

/// The pixels of the cell drawn at logical column `col` of row `row` of
/// `frame` (both from zero), wherever the layout draws it.
fn crop_cell(r: &Renderer<'_, '_>, frame: &[u32], (row, col): (usize, usize)) -> Vec<u32> {
    let w = usize::try_from(r.whole_size().0.value()).expect("a width");
    let cw = usize::try_from(r.metrics().cell_w.value()).expect("a width");
    let ch = usize::try_from(r.metrics().cell_h.value()).expect("a height");
    let v = r
        .bidi()
        .visual_col(Row::from_index(row), Col::at_index(col))
        .index();
    (row * ch..(row + 1) * ch)
        .flat_map(|y| frame[y * w + v * cw..y * w + (v + 1) * cw].to_vec())
        .collect()
}

/// **`t`'s frame on every path**, held to the bars: the instance path's.
fn three(arms: &mut Arms<'_, '_, '_>, t: &mut Term<'_>, what: &str) -> Vec<u32> {
    let Frames { instance, .. } = arms.check(t, what);
    instance
}

/// **baud's message pair, on `arm`**: under each of a program's `OSC 17`,
/// `19`, `10`, `11`, `4` and `?5`, on dark and light, a tip's padding is
/// `#F4A32C` in every pixel and its letters reach `#15171B`; with both
/// colours `"none"` and a program's unreadable pair, the same pair.
fn message_pair(gpu: &Gpu, arm: Arm) {
    use baud_render::{MessageAt, MessageColours, MessagePaint, Pair, TipAnchor, message_look};
    let data = built_face();
    let faces = [Face::parse(&data).expect("the built face")];
    let (cols, rows) = (
        baud_domain::Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    );
    let cell = (Row::from_index(3), Col::at_index(5));
    let look = message_look(
        "Copied 42 characters",
        cols,
        rows,
        MessageAt::Tip(TipAnchor {
            first: cell,
            last: cell,
        }),
    )
    .expect("room");
    let (row, left) = (look.lines[0].row.index(), look.lines[0].col.index());
    let width = look.lines[0].text.len();
    let none = MessageColours {
        page: MessagePaint::Selection,
        text: MessagePaint::Selection,
    };
    let programs: [(&str, &[u8], MessageColours); 8] = [
        ("nothing", b"", MessageColours::default()),
        (
            "OSC 17",
            b"\x1b]17;#102030\x1b\\",
            MessageColours::default(),
        ),
        (
            "OSC 19",
            b"\x1b]19;#a0a0a0\x1b\\",
            MessageColours::default(),
        ),
        (
            "OSC 10",
            b"\x1b]10;#00ff00\x1b\\",
            MessageColours::default(),
        ),
        (
            "OSC 11",
            b"\x1b]11;#ff00ff\x1b\\",
            MessageColours::default(),
        ),
        (
            "OSC 4",
            b"\x1b]4;0;#ff0000;7;#0000ff;15;#ffff00\x1b\\",
            MessageColours::default(),
        ),
        ("?5", b"\x1b[?5h", MessageColours::default()),
        (
            "\"none\" under 4.5:1",
            b"\x1b]17;#808080\x1b\\\x1b]19;#909090\x1b\\",
            none,
        ),
    ];
    for theme in [Theme::dark(), Theme::light()] {
        for (name, bytes, colours) in programs {
            let name = format!("{arm:?}: {name}");
            let mut owned = Owned::new(&WS);
            let mut t = term(&mut owned);
            let mut arms = Arms::new(gpu, arm, fresh(&faces, theme.clone()), &t);
            feed(&mut t, b"\x1b[?25lsome text under it\r\nand more");
            feed(&mut t, ENOUGH_GLYPHS);
            feed(&mut t, bytes);
            arms.renderer.set_message_colours(colours);
            arms.renderer.set_tip(Some(look.clone()));
            let frame = three(&mut arms, &mut t, &name);
            for p in crop_cell(&arms.renderer, &frame, (row, left)) {
                assert_eq!(p, Pair::BAUD.page.0, "{name}: the box, whole");
            }
            let reached = (left + 1..left + width - 1)
                .flat_map(|col| crop_cell(&arms.renderer, &frame, (row, col)))
                .any(|p| p == Pair::BAUD.text.0);
            assert!(reached, "{name}: the text's own colour was drawn");
        }
    }
}

#[test]
fn the_message_pair_holds_on_every_arm() {
    let gpu = lavapipe();
    message_pair(&gpu, Arm::InPlace);
    message_pair(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn the_message_pair_holds_on_every_arm_on_radv() {
    let gpu = radv();
    message_pair(&gpu, Arm::Staging);
    message_pair(&gpu, gpu.arm(1));
}

/// A program's colours in every form they take, for the address and the
/// notice: what each of these moves, the selection's colours among them.
const COLOURS_A_PROGRAM_SETS: [(&str, &[u8]); 7] = [
    ("nothing", b""),
    ("OSC 17", b"\x1b]17;#102030\x1b\\"),
    ("OSC 19", b"\x1b]19;#102030\x1b\\"),
    ("OSC 10", b"\x1b]10;#00ff00\x1b\\"),
    ("OSC 11", b"\x1b]11;#ff00ff\x1b\\"),
    ("OSC 4", b"\x1b]4;0;#ff0000;7;#0000ff;15;#ffff00\x1b\\"),
    ("?5", b"\x1b[?5h"),
];

/// The address bar's look on the bottom row, in the message pair.
fn address_look(address: &str) -> baud_render::LinkLook {
    let text = codepoints(&format!(" {address} "));
    let n = text.len();
    baud_render::LinkLook {
        spans: Vec::new(),
        bar: baud_render::Field {
            row: Row::from_index(usize::from(WS.ws_row) - 1),
            col: Col::ZERO,
            text,
            hints: vec![baud_render::Hints::MESSAGE; n],
            caret: Col::at_index(n),
        },
    }
}

/// **The address and the config's notice, whatever a program sets, on
/// `arm`**: under each of `OSC 17`, `19`, `10`, `11`, `4` and `?5`, the
/// bar's and the notice's padding cells are `#F4A32C` in every pixel and
/// their letters reach `#15171B`.
fn address_and_notice(gpu: &Gpu, arm: Arm) {
    use baud_render::Pair;
    let data = built_face();
    let faces = [Face::parse(&data).expect("the built face")];
    let look = address_look("https://example.org/docs");
    let width = look.bar.text.len();
    let last = usize::from(WS.ws_row) - 1;
    let text = codepoints("baud: a setting was not read");
    for theme in [Theme::dark(), Theme::light()] {
        for (name, bytes) in COLOURS_A_PROGRAM_SETS {
            let name = format!("{arm:?}: {name}");
            let mut owned = Owned::new(&WS);
            let mut t = term(&mut owned);
            let mut arms = Arms::new(gpu, arm, fresh(&faces, theme.clone()), &t);
            feed(&mut t, b"\x1b[?25lsome text under it\r\nand more");
            feed(&mut t, ENOUGH_GLYPHS);
            feed(&mut t, bytes);
            arms.renderer.set_link(Some(look.clone()));
            let frame = three(&mut arms, &mut t, &format!("{name}, the address"));
            for p in crop_cell(&arms.renderer, &frame, (last, 0)) {
                assert_eq!(p, Pair::BAUD.page.0, "{name}: the bar's box, whole");
            }
            assert!(
                (1..width - 1)
                    .flat_map(|col| crop_cell(&arms.renderer, &frame, (last, col)))
                    .any(|p| p == Pair::BAUD.text.0),
                "{name}: the address's own colour was drawn"
            );
            // The notice, the first frame of a renderer of its own, its row
            // the one the terminal gave up.
            let mut owned = Owned::new(&WS);
            let mut t = term(&mut owned);
            let mut arms = Arms::new(gpu, arm, fresh(&faces, theme.clone()), &t);
            feed(&mut t, b"\x1b[?25lsome text under it\r\nand more");
            feed(&mut t, ENOUGH_GLYPHS);
            feed(&mut t, bytes);
            let _ = t.resize(&LESS);
            arms.renderer.set_notice(Some(text.clone()));
            let frame = three(&mut arms, &mut t, &format!("{name}, the notice"));
            let blank = text.len() + 2;
            for p in crop_cell(&arms.renderer, &frame, (last, blank)) {
                assert_eq!(p, Pair::BAUD.page.0, "{name}: the notice's box, whole");
            }
            assert!(
                (0..text.len())
                    .flat_map(|col| crop_cell(&arms.renderer, &frame, (last, col)))
                    .any(|p| p == Pair::BAUD.text.0),
                "{name}: the notice's own colour was drawn"
            );
        }
    }
}

#[test]
fn the_address_and_the_notice_take_no_colour_from_a_program() {
    let gpu = lavapipe();
    address_and_notice(&gpu, Arm::InPlace);
    address_and_notice(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn the_address_and_the_notice_take_no_colour_from_a_program_on_radv() {
    let gpu = radv();
    address_and_notice(&gpu, Arm::Staging);
    address_and_notice(&gpu, gpu.arm(1));
}

/// **No cursor covers an address, on `arm`**: a visible cursor moved onto
/// a letter of the bar, in `#F4A32C`'s own colour by `OSC 12` or any other,
/// draws nothing over it. The control: the same cursor with no bar reaches
/// the pixels.
fn no_cursor_on_address(gpu: &Gpu, arm: Arm) {
    use baud_render::Pair;
    let data = built_face();
    let faces = [Face::parse(&data).expect("the built face")];
    let look = address_look("https://example.org/docs");
    let last = usize::from(WS.ws_row) - 1;
    let put = format!("\x1b[{};4H", last + 1);
    let programs: [(&str, &[u8]); 4] = [
        ("a block", b"\x1b[2 q"),
        ("a bar", b"\x1b[6 q"),
        ("an underline", b"\x1b[4 q"),
        (
            "a block in the box's colour",
            b"\x1b[2 q\x1b]12;#F4A32C\x1b\\",
        ),
    ];
    for (name, bytes) in programs {
        let name = format!("{arm:?}: {name}");
        let mut owned = Owned::new(&WS);
        let mut t = term(&mut owned);
        let mut arms = Arms::new(gpu, arm, fresh(&faces, Theme::dark()), &t);
        feed(&mut t, b"some text under it");
        feed(&mut t, ENOUGH_GLYPHS);
        feed(&mut t, bytes);
        feed(&mut t, put.as_bytes());
        arms.renderer.set_cursor_blink(Blink::Shown);
        let bare = three(&mut arms, &mut t, &format!("{name}, no bar"));
        arms.renderer.set_link(Some(look.clone()));
        let hidden = three(&mut arms, &mut t, &format!("{name}, the bar over it"));
        arms.renderer.set_link(None);
        feed(&mut t, b"\x1b[?25l");
        arms.renderer.set_link(Some(look.clone()));
        let none = three(&mut arms, &mut t, &format!("{name}, the bar, no cursor"));
        arms.renderer.set_link(None);
        assert_eq!(
            differing(&hidden, &none),
            0,
            "{name}: the bar with the cursor on it is the bar with no cursor"
        );
        for p in crop_cell(&arms.renderer, &hidden, (last, 0)) {
            assert_eq!(p, Pair::BAUD.page.0, "{name}: the box, whole");
        }
        feed(&mut t, b"\x1b[?25h");
        let plain = three(&mut arms, &mut t, &format!("{name}, no bar again"));
        assert_eq!(differing(&bare, &plain), 0, "the control's own frame again");
        feed(&mut t, b"\x1b[?25l");
        let hid = three(&mut arms, &mut t, &format!("{name}, no bar, no cursor"));
        assert!(
            differing(&plain, &hid) > 0,
            "{name}, the control: with no bar the cursor reaches the pixels"
        );
    }
}

#[test]
fn no_cursor_covers_an_address_on_any_arm() {
    let gpu = lavapipe();
    no_cursor_on_address(&gpu, Arm::InPlace);
    no_cursor_on_address(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn no_cursor_covers_an_address_on_any_arm_on_radv() {
    let gpu = radv();
    no_cursor_on_address(&gpu, Arm::Staging);
    no_cursor_on_address(&gpu, gpu.arm(1));
}

/// The link lines under row 0's two links, from `first` to `second`
/// columns, each in its first cell's rendition.
fn two_lines(t: &Term<'_>, spans: [(usize, usize); 2]) -> Vec<baud_render::LinkLine> {
    let word = |x: usize| {
        let c = t.view(Rows::ZERO).row(Row::ZERO)[x];
        t.style_in(t.row_styles(Row::ZERO), c)
    };
    spans
        .map(|(from, to)| baud_render::LinkLine {
            row: Row::ZERO,
            from: Col::at_index(from),
            to: Col::at_index(to),
            word: word(from),
        })
        .to_vec()
}

/// The pointer over row 0's columns `from` to `to`, drawn as `paint`.
fn hovered(paint: baud_render::HoverPaint, (from, to): (usize, usize)) -> baud_render::LinkHover {
    baud_render::LinkHover {
        spans: vec![baud_render::LinkSpan {
            row: Row::ZERO,
            from: Col::at_index(from),
            to: Col::at_index(to),
        }],
        paint,
    }
}

/// **The lines at rest and all three looks under the pointer, on `arm`**:
/// a line in the text's colour on two links, with and without a program's
/// colours, solid and dotted; the box, the line and the text. The
/// controls: each reaches the pixels, and each leaves nothing.
fn lines_and_looks(gpu: &Gpu, arm: Arm) {
    use baud_render::{HoverPaint, LineColour, LineStyle};
    let data = built_face();
    let faces = [Face::parse(&data).expect("the built face")];
    let mut owned = Owned::new(&WS);
    let mut t = term(&mut owned);
    let mut arms = Arms::new(gpu, arm, fresh(&faces, Theme::dark()), &t);
    feed(
        &mut t,
        b"\x1b[?25lsee \x1b[31mhttps://example.org/x\x1b[m and \x1b[44;33mhttps://b.example\x1b[m",
    );
    feed(&mut t, ENOUGH_GLYPHS);
    let at = |what: &str| format!("{arm:?}: {what}");
    let bare = three(&mut arms, &mut t, &at("no lines"));
    let lines = two_lines(&t, [(4, 24), (30, 46)]);
    arms.renderer.set_link_lines(&lines, LineStyle::Solid);
    let solid = three(&mut arms, &mut t, &at("solid"));
    assert!(differing(&bare, &solid) > 0, "the line reached the pixels");
    arms.renderer.set_link_lines(&lines, LineStyle::Dotted);
    let dotted = three(&mut arms, &mut t, &at("dotted"));
    assert!(differing(&solid, &dotted) > 0, "dotted is not solid");
    arms.renderer.set_link_lines(&lines, LineStyle::Solid);
    for paint in [HoverPaint::Box, HoverPaint::Line, HoverPaint::Text] {
        // The lines at rest as the driver sets them under this look.
        arms.renderer.set_line_colour(match paint {
            HoverPaint::Line => LineColour::Text,
            HoverPaint::Box | HoverPaint::Text => LineColour::Box,
        });
        let rest = three(&mut arms, &mut t, &at(&format!("{paint:?}, at rest")));
        arms.renderer.set_link_hover(Some(&hovered(paint, (4, 24))));
        let over = three(&mut arms, &mut t, &at(&format!("{paint:?}")));
        assert!(differing(&rest, &over) > 0, "{paint:?} reached the pixels");
        arms.renderer.set_link_hover(None);
        let gone = three(&mut arms, &mut t, &at(&format!("{paint:?}, gone")));
        assert_eq!(differing(&rest, &gone), 0, "{paint:?} leaves nothing");
    }
    arms.renderer.set_line_colour(LineColour::Box);
}

#[test]
fn the_lines_and_the_looks_are_one_picture_on_every_arm() {
    let gpu = lavapipe();
    lines_and_looks(&gpu, Arm::InPlace);
    lines_and_looks(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn the_lines_and_the_looks_are_one_picture_on_every_arm_on_radv() {
    let gpu = radv();
    lines_and_looks(&gpu, Arm::Staging);
    lines_and_looks(&gpu, gpu.arm(1));
}

/// **An unhovered link's line is the box's colour, and a hovered link
/// looks as it did, on `arm`**: every pixel of the line under two links of
/// two text colours is `#F4A32C`; with the pointer over the first, `"text"`
/// keeps its line in the text's own red and `"box"` covers it, while the
/// second stays amber; under `"line"` the lines at rest keep the text's
/// colour and the hovered one is the box's. The control: the same lines
/// under no look are not the text's colour.
fn unhovered_amber(gpu: &Gpu, arm: Arm) {
    use baud_render::{HoverPaint, LineColour, LineStyle, Pair};
    let data = built_face();
    let faces = [Face::parse(&data).expect("the built face")];
    let mut owned = Owned::new(&WS);
    let mut t = term(&mut owned);
    let mut arms = Arms::new(gpu, arm, fresh(&faces, Theme::dark()), &t);
    feed(
        &mut t,
        b"\x1b[?25lsee \x1b[38;2;255;0;0mhttps://example.org/x\x1b[m and \
          \x1b[38;2;0;255;0;44mhttps://b.example\x1b[m",
    );
    feed(&mut t, ENOUGH_GLYPHS);
    let at = |what: &str| format!("{arm:?}: {what}");
    let plain = three(&mut arms, &mut t, &at("no lines"));
    let (first, second) = ((4, 24), (30, 46));
    let lines = two_lines(&t, [first, second]);
    arms.renderer.set_link_lines(&lines, LineStyle::Solid);
    let m = arms.renderer.metrics();
    let w = usize::try_from(arms.renderer.whole_size().0.value()).expect("a width");
    let cw = usize::try_from(m.cell_w.value()).expect("a width");
    let y = usize::try_from(m.underline.value()).expect("a row");
    // The line's row of pixels over a span, where no glyph's ink reaches it
    // in the frame with no lines: the pixels its cells' page fills there.
    let rgb = |f: &[u32], (from, to): (usize, usize)| -> Vec<u32> {
        let xs = from * cw..(to + 1) * cw;
        let page = |x: usize| plain[y * w + x];
        let mut seen: Vec<(u32, usize)> = Vec::new();
        for x in xs.clone() {
            match seen.iter_mut().find(|(p, _)| *p == page(x)) {
                Some((_, n)) => *n += 1,
                None => seen.push((page(x), 1)),
            }
        }
        let most = seen.iter().max_by_key(|(_, n)| *n).map(|(p, _)| *p);
        let clear: Vec<usize> = xs.filter(|&x| Some(page(x)) == most).collect();
        assert!(
            clear.len() * 2 > (to + 1 - from) * cw,
            "most of the row is clear"
        );
        clear.iter().map(|&x| f[y * w + x] & 0x00FF_FFFF).collect()
    };
    let all = |v: &[u32], c: u32| v.iter().all(|&p| p == c);
    let amber = Pair::BAUD.page.0 & 0x00FF_FFFF;
    let rest = three(&mut arms, &mut t, &at("at rest"));
    for (span, what) in [(first, "red text"), (second, "green text")] {
        assert!(
            all(&rgb(&rest, span), amber),
            "{what}: the line at rest is #F4A32C, found {:06x?}",
            &rgb(&rest, span)[..4]
        );
    }
    assert!(!all(&rgb(&rest, first), 0xFF_0000), "the control");
    arms.renderer
        .set_link_hover(Some(&hovered(HoverPaint::Text, first)));
    let text = three(&mut arms, &mut t, &at("text"));
    assert!(
        all(&rgb(&text, first), 0xFF_0000),
        "\"text\": the hovered line is the text's red, found {:06x?}",
        &rgb(&text, first)[..4]
    );
    assert!(
        all(&rgb(&text, second), amber),
        "\"text\": the other at rest"
    );
    arms.renderer.set_line_colour(LineColour::Text);
    arms.renderer
        .set_link_hover(Some(&hovered(HoverPaint::Line, first)));
    let line = three(&mut arms, &mut t, &at("line"));
    assert!(all(&rgb(&line, first), amber), "\"line\": the box's colour");
    assert!(
        all(&rgb(&line, second), 0x00_FF00),
        "\"line\": the other at rest in its text's green"
    );
    arms.renderer.set_line_colour(LineColour::Box);
    arms.renderer
        .set_link_hover(Some(&hovered(HoverPaint::Box, first)));
    let boxed = three(&mut arms, &mut t, &at("box"));
    arms.renderer.set_link_lines(&lines[1..], LineStyle::Solid);
    let bare = three(&mut arms, &mut t, &at("box without the first line"));
    assert_eq!(
        rgb(&boxed, first),
        rgb(&bare, first),
        "\"box\": the box covers the line under it"
    );
    assert!(
        all(&rgb(&boxed, second), amber),
        "\"box\": the other at rest"
    );
}

#[test]
fn an_unhovered_line_is_amber_and_a_hovered_one_unchanged_on_every_arm() {
    let gpu = lavapipe();
    unhovered_amber(&gpu, Arm::InPlace);
    unhovered_amber(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn an_unhovered_line_is_amber_and_a_hovered_one_unchanged_on_every_arm_on_radv() {
    let gpu = radv();
    unhovered_amber(&gpu, Arm::Staging);
    unhovered_amber(&gpu, gpu.arm(1));
}

/// The faces the splash's scenes draw with: JetBrains Mono and its bold,
/// which the lockup's wordmark takes.
fn splash_faces() -> [Vec<u8>; 2] {
    [
        jetbrains_mono(),
        repo_file("crates/baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    ]
}

/// A renderer over the splash's faces, the second the bold.
fn splash_renderer<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    const ROLES: [FaceRole; 2] = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)];
    scenes::renderer(
        Stack::with_roles(faces, &ROLES).expect("a stack"),
        Theme::dark(),
        WS,
    )
}

/// **The plasma's ages, each drawn by the cell path as the CPU path draws
/// it** (`Arms::check_trail`).
fn plasma_is_drawn(arms: &mut Arms<'_, '_, '_>, t: &mut Term<'_>, ages: &[f32], what: &str) {
    for &age in ages {
        arms.renderer.set_splash(Some(SplashLook {
            phase: SplashPhase::Plasma(age),
        }));
        arms.check_trail(t, &format!("{what}, the plasma at {age}"));
    }
}

/// **The launch splash, on `arm`**: the lockup held, the fade at half and
/// its end, over a prompt and a line of text under the logo, on the dark
/// and the light half. The controls: the held logo changes the frame and
/// its block is the half's amber, exactly, at the block's centre; at its
/// end the frame is the plain one, and no record is left.
fn splash(gpu: &Gpu, arm: Arm) {
    let files = splash_faces();
    let faces = files.each_ref().map(|f| Face::parse(f).expect("a face"));
    let mut owned = Owned::new(&WS);
    let mut t = term(&mut owned);
    let mut arms = Arms::new(gpu, arm, splash_renderer(&faces), &t);
    feed(
        &mut t,
        b"\x1b[1;34m~/code\x1b[0m on \x1b[35mmain\x1b[0m\r\n\x1b[5;30Hunder the logo, text\x1b[2;1H$ ",
    );
    let (out, gone) = (baud_render::splash::OUT, baud_render::splash::GONE);
    for (half, theme, colours) in [
        ("dark", Theme::dark(), baud_render::splash::DARK),
        ("light", Theme::light(), baud_render::splash::light()),
    ] {
        let amber = colours.amber;
        arms.renderer.set_theme(theme);
        arms.renderer.set_splash(None);
        let plain = three(&mut arms, &mut t, &format!("{arm:?}, {half}: no splash"));
        for phase in [
            SplashPhase::Hold,
            SplashPhase::Fade(0.5),
            SplashPhase::Plasma(gone),
        ] {
            let what = format!("{arm:?}, {half}, {phase:?}");
            arms.renderer.set_splash(Some(SplashLook { phase }));
            let a = three(&mut arms, &mut t, &what);
            assert_eq!(
                arms.renderer.flash_held(),
                baud_render::Held::Full,
                "{what}: no repeat pass"
            );
            match phase {
                SplashPhase::Hold => {
                    assert!(differing(&a, &plain) > 500, "{what}: the control, drawn");
                    let records = arms.renderer.splash();
                    let block = records.last().expect("the block's record");
                    assert_eq!(block.kind, Kind::Solid);
                    let fw =
                        usize::try_from(arms.renderer.whole_size().0.value()).expect("a width");
                    let (cx, cy) = (
                        usize::try_from(i32::from(block.x.0) + i32::from(block.w.0) / 2)
                            .expect("a column"),
                        usize::try_from(i32::from(block.y.0) + i32::from(block.h.0) / 2)
                            .expect("a row"),
                    );
                    assert_eq!(
                        a[cy * fw + cx],
                        baud_domain::premultiply(amber.0),
                        "{what}: the block is the half's amber"
                    );
                }
                SplashPhase::Plasma(_) => {
                    assert_eq!(differing(&a, &plain), 0, "{what}: gone");
                    assert!(arms.renderer.splash().is_empty(), "{what}: no record left");
                }
                SplashPhase::Fade(_) => {
                    assert!(differing(&a, &plain) > 0, "{what}: the fade is drawn");
                }
            }
        }
        plasma_is_drawn(
            &mut arms,
            &mut t,
            &[0.0, 0.03, out, out + 0.1],
            &format!("{arm:?}, {half}"),
        );
    }
    arms.renderer.set_splash(None);
}

#[test]
fn the_splash_draws_alike_on_every_arm() {
    let gpu = lavapipe();
    splash(&gpu, Arm::InPlace);
    splash(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn the_splash_draws_alike_on_every_arm_on_radv() {
    let gpu = radv();
    splash(&gpu, Arm::Staging);
    splash(&gpu, gpu.arm(1));
}

/// **The shell is drawn in front of the splash, on `arm`**
/// (`Renderer::splash_yields_to`): with four lines of output reaching into
/// the logo's box, at the held logo and the fade, every cell the output
/// inked is drawn on the cell and the instance paths, pixel for pixel, as
/// each draws it with no splash. The controls: the logo is drawn, the
/// frame differing from the plain one outside the shell's cells; and the
/// output reaches the logo's box.
fn shell_in_front(gpu: &Gpu, arm: Arm) {
    let files = splash_faces();
    let faces = files.each_ref().map(|f| Face::parse(f).expect("a face"));
    let mut owned = Owned::new(&WS);
    let mut t = term(&mut owned);
    let mut arms = Arms::new(gpu, arm, splash_renderer(&faces), &t);
    for i in 0..4 {
        feed(
            &mut t,
            format!(
                "\x1b[{};1Hline {i}: the shell's output, drawn in front of the logo it meets",
                i + 4
            )
            .as_bytes(),
        );
    }
    feed(&mut t, b"\x1b[8;1H\x1b[1;34m~/code\x1b[0m $ ");
    let m = arms.renderer.metrics();
    let n = |p: FontPx| usize::try_from(p.value()).expect("a size");
    let (cw, ch, fw) = (n(m.cell_w), n(m.cell_h), n(arms.renderer.whole_size().0));
    let view = t.view(Rows::ZERO);
    let mut inked = Vec::new();
    for y in 0..view.rows().index() {
        for (x, c) in view.row(Row::from_index(y)).iter().enumerate() {
            if !c.content.is_empty() && c.content.as_codepoint().is_none_or(|p| p.value() != 0x20) {
                inked.push((y, x));
            }
        }
    }
    let cell = |px: &[u32], (y, x): (usize, usize)| -> Vec<u32> {
        (0..ch)
            .flat_map(|dy| px[(y * ch + dy) * fw + x * cw..][..cw].to_vec())
            .collect()
    };
    let shell = |i: usize| inked.contains(&((i / fw) / ch, (i % fw) / cw));
    arms.renderer.set_splash(None);
    let plain = arms.check(&mut t, &format!("{arm:?}: no splash"));
    let mut reached = false;
    for phase in [SplashPhase::Hold, SplashPhase::Fade(0.5)] {
        let what = format!("{arm:?}, {phase:?}");
        arms.renderer.set_splash(Some(SplashLook { phase }));
        let f = arms.check(&mut t, &what);
        for &c in &inked {
            assert!(
                cell(&f.instance, c) == cell(&plain.instance, c),
                "{what}: the instance path draws cell {c:?} under the logo"
            );
            assert!(
                cell(&f.cell, c) == cell(&plain.cell, c),
                "{what}: the cell path draws cell {c:?} under the logo"
            );
        }
        let logo = (0..fw * ch * usize::from(WS.ws_row))
            .filter(|&i| !shell(i) && f.instance[i] != plain.instance[i])
            .count();
        assert!(logo > 100, "{what}: the control, the logo drawn ({logo})");
        reached |= arms.renderer.splash().iter().any(|r| {
            let right = usize::try_from(i32::from(r.x.0) + i32::from(r.w.0)).unwrap_or(0);
            right > 0
                && inked.iter().any(|&(y, x)| {
                    let (top, bottom) = (i32::from(r.y.0), i32::from(r.y.0) + i32::from(r.h.0));
                    let row = i32::try_from(y * ch).expect("a row");
                    x * cw < right
                        && row < bottom
                        && row + i32::try_from(ch).expect("a height") > top
                })
        });
    }
    assert!(
        reached,
        "the control: the output reaches the logo's rows and columns"
    );
    let out = baud_render::splash::OUT;
    plasma_is_drawn(
        &mut arms,
        &mut t,
        &[0.03, out, out + 0.1],
        &format!("{arm:?}"),
    );
    arms.renderer.set_splash(None);
}

#[test]
fn the_shell_is_drawn_in_front_of_the_splash_on_every_arm() {
    let gpu = lavapipe();
    shell_in_front(&gpu, Arm::InPlace);
    shell_in_front(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn the_shell_is_drawn_in_front_of_the_splash_on_every_arm_on_radv() {
    let gpu = radv();
    shell_in_front(&gpu, Arm::Staging);
    shell_in_front(&gpu, gpu.arm(1));
}
