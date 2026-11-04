//! **The plasma's handedness** (`baud_render::plasma::Hand`): a leftward
//! move is the rightward one mirrored, pixel for pixel, on the CPU path,
//! which the Linux driver draws with and which the pixel oracle holds both
//! GPU paths to. The braid's offset across the move and the ions' launch
//! side, turn and spin were fixed to the axis, so a move to the left was
//! the move to the right turned over rather than mirrored: its filaments
//! crossed the other way and its ions curled to the other side.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::plasma;
use baud_render::{
    Blend, Damage, Pt, PxSize, Rect, Renderer, Segment, Shape, Smear, Stack, Strength, Target,
    Theme, TrailLook,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 64,
    ws_row: 9,
};

fn px(p: Px) -> usize {
    usize::try_from(p.value()).unwrap()
}

fn renderer<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

fn centre(r: Rect) -> Pt {
    Pt {
        x: (r.left + r.right) * 0.5,
        y: (r.top + r.bottom) * 0.5,
    }
}

/// **The plasma of a move from `from` to `to` on `row`, `age` seconds in**,
/// as the app's driver hands it: lit from the centre of the cell it left
/// towards the cell it reached, on kitty's quad, its braid along the move.
fn plasma(r: &Renderer<'_, '_>, row: Row, (from, to): (Col, Col), seed: Seed, age: f32) -> Vfx {
    let style = baud_core::CursorStyle::SteadyBlock;
    let rect = |col: Col| r.cursor_rect(row, col, style);
    let (a, b) = (rect(from), rect(to));
    let unit = f32::from(u16::try_from(r.metrics().cell_h.value()).unwrap());
    let ignition = Ignition::ignite(centre(a), b, seed, unit);
    Vfx {
        effect: Effect::Plasma,
        emitter: Smear {
            corners: Segment::new(a.corners(), b).at(age),
            hole: b,
            shape: Shape::Quad,
            strength: Strength::Full,
            opacity: Smear::OPAQUE,
        },
        ignition: ignition.at(age),
        axis: ignition.dir,
    }
}

/// The frame with the cursor at `(row, col)` and `look`, on a blank screen.
fn frame(r: &mut Renderer<'_, '_>, (row, col): (usize, usize), look: Option<Vfx>) -> Vec<u32> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let cup = format!("\x1b[{};{}H", row + 1, col + 1).into_bytes();
    t.write_at(BytePos::ZERO, &cup);
    t.feed(BytePos::ZERO, ReadLen::from_index(cup.len()));
    r.set_trail(look.map(TrailLook::Vfx));
    let (w, h) = r.pixel_size();
    let mut pixels = vec![0u32; px(w) * px(h)];
    let mut target = Target::new(&mut pixels, w, w, h).unwrap();
    r.render(&mut t, &mut target, Damage::Full);
    pixels
}

/// `pixels`, `width` wide, mirrored left to right.
fn mirrored(pixels: &[u32], width: usize) -> Vec<u32> {
    pixels
        .chunks(width)
        .flat_map(|row| row.iter().rev().copied())
        .collect()
}

/// **A leftward plasma is the rightward one mirrored**: on one seed, a move
/// from column 10 to 40 and the mirrored move, from 53 to 23 on a 64-column
/// screen, draw frames that are each other's mirror image at every age
/// from the first frame to past the last ion, on two seeds and two rows,
/// with the cursor at the cell each reached. The controls: each frame
/// lights hundreds of pixels the blank frame does not, and the rightward
/// frame is not its own mirror image, so the comparison is not between two
/// symmetric frames. Red when the hand is fixed to the axis, which turned
/// the leftward move over.
#[test]
fn a_leftward_plasma_is_the_rightward_one_mirrored() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut r = renderer(&faces);
    let width = px(r.pixel_size().0);
    let cols = usize::from(WS.ws_col);
    let (from, to) = (10usize, 40usize);
    for row in [4usize, 2] {
        let bare = frame(&mut r, (row, to), None);
        for seed in [Seed(5), Seed(0x00C0_FFEE)] {
            for age in [0.016f32, 0.04, 0.08, 0.15, 0.25, 0.4, 0.55, 0.62] {
                let back = (cols - 1 - from, cols - 1 - to);
                let on = Row::from_index(row);
                let (forth, returned) = (
                    plasma(&r, on, (Col::at_index(from), Col::at_index(to)), seed, age),
                    plasma(
                        &r,
                        on,
                        (Col::at_index(back.0), Col::at_index(back.1)),
                        seed,
                        age,
                    ),
                );
                let right = frame(&mut r, (row, to), Some(forth));
                let left = frame(&mut r, (row, back.1), Some(returned));
                let want = mirrored(&right, width);
                let off = left.iter().zip(&want).filter(|(a, b)| a != b).count();
                assert_eq!(
                    off, 0,
                    "row {row}, {seed:?} at {age}: {off} pixels of the leftward move are not \
                     the rightward one's mirrored"
                );
                let lit = right.iter().zip(&bare).filter(|(a, b)| a != b).count();
                if age < 0.5 {
                    assert!(
                        lit > 300,
                        "row {row}, {seed:?} at {age}: only {lit} pixels lit"
                    );
                    let own = right.iter().zip(&want).filter(|(a, b)| a != b).count();
                    assert!(
                        own > 100,
                        "row {row}, {seed:?} at {age}: the rightward frame is nearly its own \
                         mirror image ({own} pixels differ), so the comparison shows nothing"
                    );
                }
            }
        }
    }
}

/// **The hand is the ignition's, so a retarget keeps it**: a rightward
/// ignition whose quad is retargeted back to the left draws its braid in
/// the right hand along the leftward axis, where the same quad under the
/// ignition reversed draws it in the left -- the light differs at the
/// braid's pixels, and nothing else of the light reads the ignition's
/// direction. Red when the hand is read off the axis, which a retarget in
/// flight turns, or not at all. The control: the light reaches hundreds of
/// pixels.
#[test]
fn the_hand_is_the_ignitions_and_a_retarget_keeps_it() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let r = renderer(&faces);
    let row = 4usize;
    // A rightward ignition from 10 to 40 whose quad heads back left to 20.
    let lit = plasma(
        &r,
        Row::from_index(row),
        (Col::at_index(10), Col::at_index(40)),
        Seed(5),
        0.12,
    );
    let style = baud_core::CursorStyle::SteadyBlock;
    let rect = |col: usize| r.cursor_rect(Row::from_index(row), Col::at_index(col), style);
    let back = Vfx {
        emitter: Smear {
            corners: Segment::new(rect(40).corners(), rect(20)).at(0.05),
            hole: rect(20),
            ..lit.emitter
        },
        axis: Pt { x: -1.0, y: 0.0 },
        ..lit
    };
    let reversed = Vfx {
        ignition: Ignition {
            dir: Pt {
                x: -back.ignition.dir.x,
                y: back.ignition.dir.y,
            },
            ..back.ignition
        },
        ..back
    };
    let (w, h) = r.pixel_size();
    let (mut shown, mut differ) = (0usize, 0usize);
    for y in 0..px(h) {
        for x in 0..px(w) {
            let p = Pt {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
            };
            let (a, b) = (plasma::light(&back, p), plasma::light(&reversed, p));
            shown += usize::from(a > 0.01);
            differ += usize::from(a != b);
        }
    }
    assert!(shown > 300, "the control: only {shown} pixels lit");
    assert!(
        differ > 50,
        "a retarget drew the same braid whichever hand its ignition has ({differ} pixels differ)"
    );
}
