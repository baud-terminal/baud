//! iTerm2's fireworks on the CPU path, which the Linux driver draws with
//! (`baud_render::fireworks`): drawn at the cursor and nowhere in the hole
//! round it, every star where its record is and in its colour, gone by
//! itself at the show's end with the rows it was on repainted, the trail
//! held while it burns, and still under Reduce Motion.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::fireworks::{self, BAUD, FADE, Form, HOLD, HOLE, ORIGINS, Show};
use baud_render::{
    Blend, Damage, Kind, Pt, PxSize, Rect, Renderer, Segment, Shape, Smear, Stack, Strength,
    Target, Theme, TrailLook,
};

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn font(p: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
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

/// A frame painted into `px`, which holds the last: an incremental frame
/// repaints only the rows it finds changed.
fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut Vec<u32>, d: Damage) -> Vec<u32> {
    let (w, h) = r.pixel_size();
    px.resize(usize::try_from(w.value() * h.value()).unwrap(), 0);
    let mut target = Target::new(px, w, w, h).unwrap();
    let seal = t.seal_writes();
    r.render(t, seal, &mut target, d);
    px.clone()
}

/// The show `age` seconds in, every burst and the hole at `at`.
fn show(age: f32, at: Pt, form: Form) -> Show {
    Show {
        age,
        hold: HOLD,
        origins: [at; ORIGINS],
        hole: at,
        scale: 1.0,
        seed: Seed(41),
        look: BAUD,
        form,
    }
}

/// The centre of cell `(row, col)` in a renderer's frame.
fn centre(r: &Renderer<'_, '_>, row: usize, col: usize) -> Pt {
    let m = r.metrics();
    let f = |v: u32| f32::from(u16::try_from(v).unwrap());
    let k = |v: usize| f32::from(u16::try_from(v).unwrap());
    Pt {
        x: (k(col) + 0.5) * f(m.cell_w.value()),
        y: (k(row) + 0.5) * f(m.cell_h.value()),
    }
}

/// **The fireworks are drawn at the cursor, and end by themselves**: a
/// frame a sixth of a second into a show differs from the bare page, every
/// changed pixel within the fast stars' reach of the cursor's centre and
/// none inside the hole round it; every star record is drawn where it
/// is, a pixel at its centre in its own colour; the frames hold the
/// effect's slot until the end; and the frame after the end -- painted
/// incrementally, so only the rows the stars were on are repainted -- is
/// the bare page again, as is every pixel once the show is handed no more.
#[test]
fn fireworks_are_drawn_at_the_cursor_and_end_by_themselves() {
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&regular).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l\x1b[2;1Hsome text on a row\x1b[12;40H");
    let mut px = Vec::new();
    let bare = paint(&mut r, &mut t, &mut px, Damage::Full);
    let at = centre(&r, 11, 39);
    let (w, _) = r.pixel_size();
    let w = usize::try_from(w.value()).unwrap();
    let pt = |i: usize| Pt {
        x: (i % w) as f32 + 0.5,
        y: (i / w) as f32 + 0.5,
    };
    let s = show(1.0 / 6.0, at, Form::Moving);
    r.set_fireworks(&[s]);
    let lit = paint(&mut r, &mut t, &mut px, Damage::Incremental);
    let changed: Vec<usize> = (0..lit.len()).filter(|&i| lit[i] != bare[i]).collect();
    assert!(changed.len() > 50, "drawn: {} pixels", changed.len());
    let reach = 1100.0 / 6.0 + 30.0;
    for &i in &changed {
        let p = pt(i);
        let d = ((p.x - at.x).powi(2) + (p.y - at.y).powi(2)).sqrt();
        assert!(d <= reach, "a pixel {d} from the cursor");
        assert!(d >= HOLE - 0.5, "a pixel in the hole, {d} from the cursor");
    }
    // Each star where its record is, in its colour: its centre pixel, if
    // the star covers it whole at four fifths opacity or more and no later
    // star or the hole is over it, is the colour's over the dark page --
    // each channel the colour has near full, each it lacks near none.
    let records = r.trail().to_vec();
    assert!(!records.is_empty() && records.iter().all(|q| q.kind == Kind::Particle));
    let mut checked = 0;
    for (n, q) in records.iter().enumerate() {
        let c = fireworks::centre(q);
        let (x, y) = (c.x.floor() as i64, c.y.floor() as i64);
        let later = records[n + 1..].iter().any(|o| {
            let (ox, oy) = (i64::from(o.x.0), i64::from(o.y.0));
            (ox..ox + i64::from(o.w.0)).contains(&x) && (oy..oy + i64::from(o.h.0)).contains(&y)
        });
        let whole = fireworks::coverage(
            Pt { x: 0.0, y: 0.0 },
            fireworks::image_side(q.v),
            fireworks::turned(q.u),
        ) == 1.0;
        let free = fireworks::hole(c, at, HOLE) == 1.0;
        if later || !whole || !free || q.color.a().get() < 204 || x < 0 || y < 0 {
            continue;
        }
        let i = usize::try_from(y).unwrap() * w + usize::try_from(x).unwrap();
        if i < lit.len() {
            for shift in [0, 8, 16] {
                let (got, want) = ((lit[i] >> shift) & 0xFF, (q.color.0 >> shift) & 0xFF);
                match want {
                    0xFF => assert!(got >= 190, "star {n} at {c:?}: {:08x}", lit[i]),
                    _ => assert!(got <= 40, "star {n} at {c:?}: {:08x}", lit[i]),
                }
            }
            checked += 1;
        }
    }
    assert!(checked > 3, "only {checked} stars' centres checked");
    // Through the show the slot is the fireworks'; at the end, nothing.
    let end = HOLD + FADE;
    let mut age = 1.0 / 6.0;
    while age < end {
        r.set_fireworks(&[show(age, at, Form::Moving)]);
        assert!(r.fireworks_uniform([0.0, 0.0]).is_some(), "{age}");
        paint(&mut r, &mut t, &mut px, Damage::Incremental);
        age += 1.0 / 30.0;
    }
    r.set_fireworks(&[show(end, at, Form::Moving)]);
    assert!(r.trail().is_empty(), "no star at the end");
    let gone = paint(&mut r, &mut t, &mut px, Damage::Incremental);
    assert_eq!(gone, bare, "the end repaints the rows the stars were on");
    r.set_fireworks(&[]);
    assert!(r.fireworks_uniform([0.0, 0.0]).is_none());
    let after = paint(&mut r, &mut t, &mut px, Damage::Incremental);
    assert_eq!(after, bare);
}

/// **A trail lit while the fireworks burn is held until it lands**, as
/// under the splash: the frame's slot is the fireworks', and once they are
/// gone the trail asked for is drawn again only after the driver has handed
/// none.
#[test]
fn a_trail_waits_for_the_fireworks() {
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&regular).unwrap()];
    let mut r = renderer(&faces);
    let at = centre(&r, 5, 5);
    let cell = |c: f32| Rect {
        left: c * 8.0,
        top: 0.0,
        right: c * 8.0 + 8.0,
        bottom: 17.0,
    };
    let (from, to) = (cell(2.0), cell(30.0));
    let fire = TrailLook::Vfx(Vfx {
        effect: Effect::Fire,
        emitter: Smear {
            corners: Segment::new(from.corners(), to).at(0.05),
            hole: to,
            shape: Shape::Quad,
            strength: Strength::Full,
            opacity: Smear::OPAQUE,
        },
        ignition: Ignition::ignite(Pt { x: 20.0, y: 8.5 }, to, Seed(3), 17.0).at(0.05),
        axis: Pt { x: 1.0, y: 0.0 },
    });
    r.set_fireworks(&[show(0.2, at, Form::Moving)]);
    r.set_trail(Some(fire));
    assert!(r.vfx().is_none(), "the fireworks hold the slot");
    assert!(r.fireworks_uniform([0.0, 0.0]).is_some());
    r.set_fireworks(&[]);
    assert!(r.vfx().is_none(), "the trail lit under them is held");
    r.set_trail(None);
    r.set_trail(Some(fire));
    assert!(r.vfx().is_some(), "a trail lit after them is drawn");
}

/// **Under Reduce Motion the show is still**: every frame of a still show
/// draws the same records at the same places, sizes and turns, fainter,
/// and none at the end.
#[test]
fn a_still_show_draws_the_same_records_fainter() {
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&regular).unwrap()];
    let mut r = renderer(&faces);
    let at = centre(&r, 11, 39);
    let records = |r: &mut Renderer<'_, '_>, age: f32| {
        r.set_fireworks(&[show(age, at, Form::Still)]);
        r.trail().to_vec()
    };
    let (a, b) = (records(&mut r, 0.1), records(&mut r, 0.9));
    assert!(!a.is_empty());
    assert_eq!(a.len(), b.len());
    for (p, q) in a.iter().zip(&b) {
        assert_eq!((p.x, p.y, p.w, p.u, p.v), (q.x, q.y, q.w, q.u, q.v));
        assert!(q.color.a().get() < p.color.a().get());
    }
    assert!(records(&mut r, HOLD + FADE).is_empty());
}
