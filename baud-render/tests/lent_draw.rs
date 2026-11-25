//! **A frame drawn into a lent buffer** (`Renderer::draw_lent`): every
//! row the damage names and every row the buffer is missing is painted at
//! the surface's whole width -- the margins, the strip, the gutter and the
//! columns beside the whole frame included -- and what it painted is
//! returned. A buffer is filled with poison before each draw, and no
//! poison may survive in a band the draw says it painted.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, DirtyRows, FrameDamage, ReadLen, Row, Rows, WholeWhy, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Age, Argb, Band, Blend, ChromeBands, Damage, LineEdge, Painted, PxSize, Renderer, Stack,
    StatusMarks, SurfaceRows, Target, Theme, VideoFlash, WrapMarker,
};
use support::{Canvas, RECORDINGS, paint, recording, replay_final};

/// A word no draw writes: straight alpha 0x12 over a colour no theme
/// here holds.
const POISON: u32 = 0x12_C0_FF_EE;

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 8,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn font() -> Vec<u8> {
    support::font("JetBrainsMono-Regular.ttf")
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

/// **A renderer with every band of chrome**: the margins and their marks,
/// the exit-status strip, the scroller's gutter and the lights' line,
/// over a terminal with a status line, placed `(15, 9)` into a surface 30
/// pixels wider and 23 taller than the whole frame.
fn chromed(r: &mut Renderer<'_, '_>, t: &Term<'_>) -> (Px, Px) {
    r.set_wrap_marker(WrapMarker::On);
    r.set_margins(Px::of(6));
    r.set_status_marks(StatusMarks::On);
    r.set_gutter(Px::of(5));
    r.set_lights_line(Some(LineEdge::Bottom), 2.0);
    r.fit(t);
    let (w, h) = r.whole_size();
    let size = (Px::of(w.value() + 30), Px::of(h.value() + 23));
    r.set_surface(size, (Px::of(15), Px::of(9)));
    size
}

/// A pixel count as an index.
macro_rules! at {
    ($p:expr) => {
        usize::try_from($p.value()).unwrap()
    };
}

/// A lent buffer the surface's size.
struct Buffer {
    px: Vec<u32>,
    w: Px,
    h: Px,
}

impl Buffer {
    fn new((w, h): (Px, Px)) -> Buffer {
        Buffer {
            px: vec![POISON; at!(w) * at!(h)],
            w,
            h,
        }
    }

    fn target(&mut self) -> Target<'_> {
        Target::new(&mut self.px, self.w, self.w, self.h).unwrap()
    }

    /// Poisons the pixel rows of each band.
    fn poison(&mut self, bands: impl Iterator<Item = Band>) {
        let w = at!(self.w);
        for b in bands {
            self.px[at!(b.top) * w..at!(b.bottom) * w].fill(POISON);
        }
    }

    /// The pixel rows poison is left on, in the bands given.
    fn poisoned_in(&self, bands: &[Band]) -> Vec<usize> {
        let w = at!(self.w);
        bands
            .iter()
            .flat_map(|b| at!(b.top)..at!(b.bottom))
            .filter(|&y| self.px[y * w..(y + 1) * w].contains(&POISON))
            .collect()
    }
}

/// The surface's every row.
fn everything(r: &Renderer<'_, '_>) -> Band {
    let all = SurfaceRows {
        grid: DirtyRows::NONE
            .span(Row::ZERO, Row::from_index(baud_domain::DIRTY_ROWS))
            .unwrap(),
        chrome: ChromeBands::TOP
            .union(ChromeBands::LIGHTS)
            .union(ChromeBands::STATUS)
            .union(ChromeBands::BOTTOM),
    };
    let bands: Vec<Band> = r.bands(&all).collect();
    assert_eq!(bands.len(), 1, "the premise: every row is one band");
    bands[0]
}

/// **One lent draw**: the frame sealed and its damage read as a driver
/// does, `age` what the buffer is missing.
fn draw(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    buf: &mut Buffer,
    age: Age,
) -> (FrameDamage, Painted) {
    let seal = t.seal_writes();
    let damage = r.damage(t, Rows::ZERO);
    let painted = r.draw_lent(t, seal, Rows::ZERO, &damage, &mut buf.target(), age);
    (damage, painted)
}

/// Whether `a` holds every row and band of `b`.
fn holds(a: SurfaceRows, b: SurfaceRows) -> bool {
    a.union(b) == a
}

/// **No poison survives a painted band.** A buffer poisoned whole and
/// drawn at `Age::Whole` holds none anywhere; one poisoned whole and
/// drawn at `Age::Stale(s)` holds none in the bands of what the draw
/// says it painted, which holds the damage and `s`; and a frame that
/// only moves the cursor along its row paints that row alone.
#[test]
fn no_poison_survives_a_painted_band() {
    let data = font();
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[2$~\x1b[1;1Hthe first row\x1b[4;3Hand the fourth",
    );
    let mut r = renderer(&faces);
    let size = chromed(&mut r, &t);
    let mut buf = Buffer::new(size);

    let (damage, painted) = draw(&mut r, &mut t, &mut buf, Age::Whole);
    assert!(matches!(damage, FrameDamage::Whole(_)), "{damage:?}");
    let all = everything(&r);
    assert_eq!(
        buf.poisoned_in(&[all]),
        Vec::<usize>::new(),
        "Age::Whole: poison left on these pixel rows"
    );
    assert_eq!(
        r.bands(&painted.rows()).collect::<Vec<_>>(),
        [all],
        "a whole draw says it painted the surface"
    );

    // One row written, and a stale row, the band above and the lights'
    // line the buffer is missing.
    feed(&mut t, b"\x1b[6;1Hsixth");
    let stale = SurfaceRows {
        grid: DirtyRows::NONE.with(Row::from_index(1)).unwrap(),
        chrome: ChromeBands::TOP.union(ChromeBands::LIGHTS),
    };
    buf.poison(core::iter::once(all));
    let (damage, painted) = draw(&mut r, &mut t, &mut buf, Age::Stale(stale));
    let FrameDamage::Rows { rows, .. } = damage else {
        panic!("one row written: {damage:?}");
    };
    assert!(rows.contains(Row::from_index(5)), "{rows:?}");
    let painted = painted.rows();
    assert!(
        holds(
            painted,
            stale.union(SurfaceRows {
                grid: rows,
                chrome: ChromeBands::NONE
            })
        ),
        "painted {painted:?} holds the damage {rows:?} and the age {stale:?}"
    );
    let bands: Vec<Band> = r.bands(&painted).collect();
    assert_eq!(
        buf.poisoned_in(&bands),
        Vec::<usize>::new(),
        "Age::Stale: poison left in a painted band"
    );
    assert!(
        bands.iter().any(|b| b.top.value() == 0),
        "the band above is painted from the surface's top"
    );

    // The cursor moved along its row, and nothing else.
    feed(&mut t, b"\x1b[6;12H");
    let (_, painted) = draw(&mut r, &mut t, &mut buf, Age::Stale(SurfaceRows::NONE));
    feed(&mut t, b"\x1b[6;20H");
    buf.poison(core::iter::once(all));
    let (damage, painted_now) = draw(&mut r, &mut t, &mut buf, Age::Stale(SurfaceRows::NONE));
    assert!(
        matches!(damage, FrameDamage::Rows { rows, .. } if rows == DirtyRows::NONE),
        "the premise: no row written: {damage:?} (the frame before painted {painted:?})"
    );
    assert_eq!(
        painted_now.rows(),
        SurfaceRows {
            grid: DirtyRows::NONE.with(Row::from_index(5)).unwrap(),
            chrome: ChromeBands::NONE,
        },
        "a cursor-only frame paints the cursor's row alone"
    );
    let bands: Vec<Band> = r.bands(&painted_now.rows()).collect();
    assert_eq!(buf.poisoned_in(&bands), Vec::<usize>::new());
}

/// **A lent draw at `Age::Whole` is the kept frame's full render, pixel
/// for pixel**, on every pinned recording's last screen; and placed into
/// a larger surface it is that frame where the whole frame lies, and the
/// margin's colour, `Renderer::clear_color_of`, everywhere else.
#[test]
fn draw_lent_matches_render() {
    let data = font();
    let faces = [Face::parse(&data).unwrap()];
    for name in RECORDINGS {
        let bytes = recording(name);
        let mut kept = support::renderer(&faces);
        let mut canvas = Canvas::for_renderer(&kept);
        let mut lent = support::renderer(&faces);
        let mut boxed = support::renderer(&faces);
        let mut into = None;
        let mut letterboxed = None;
        let mut margin = Argb(0);
        replay_final(&bytes, |t| {
            paint(&mut kept, t, &mut canvas, Damage::Full);
            lent.fit(t);
            assert_eq!(
                lent.whole_size(),
                lent.pixel_size(),
                "{name}: the premise, a frame of cells alone"
            );
            let mut buf = Buffer::new(lent.whole_size());
            draw(&mut lent, t, &mut buf, Age::Whole);
            into = Some(buf);
            boxed.fit(t);
            let (w, h) = boxed.whole_size();
            boxed.set_surface(
                (Px::of(w.value() + 11), Px::of(h.value() + 7)),
                (Px::of(4), Px::of(3)),
            );
            let mut buf = Buffer::new((Px::of(w.value() + 11), Px::of(h.value() + 7)));
            draw(&mut boxed, t, &mut buf, Age::Whole);
            margin = boxed.clear_color_of(t);
            letterboxed = Some(buf);
        });
        let into = into.unwrap();
        assert!(
            into.px == canvas.pixels,
            "{name}: the lent draw differs from the full render at {} pixels",
            into.px
                .iter()
                .zip(&canvas.pixels)
                .filter(|(a, b)| a != b)
                .count()
        );
        let boxed = letterboxed.unwrap();
        let (bw, cw) = (at!(boxed.w), canvas.width);
        let mut wrong = 0;
        for y in 0..at!(boxed.h) {
            for x in 0..bw {
                let inside = (3..3 + canvas.height).contains(&y) && (4..4 + cw).contains(&x);
                let want = match inside {
                    true => canvas.pixels[(y - 3) * cw + x - 4],
                    false => margin.0,
                };
                wrong += usize::from(boxed.px[y * bw + x] != want);
            }
        }
        assert_eq!(wrong, 0, "{name}: pixels wrong in the letterboxed surface");
    }
}

/// **A changed margin leaves no buffer its old margin.** Three buffers
/// drawn in turn, each aged as a presenter keeps them: by what the draws
/// since its own painted, and whole for every buffer after a whole
/// damage. Each holds the same quiet frame to begin with, so each is
/// missing nothing. The visual bell's flash turns the margin's colour
/// with no cell written, and after one draw into each, its stale rows
/// poisoned first, every pixel beside the whole frame is the new colour
/// in all three.
#[test]
fn a_changed_margin_leaves_no_buffer_its_old_margin() {
    let data = font();
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l\x1b[2;2Hthree buffers");
    let mut r = renderer(&faces);
    let size = chromed(&mut r, &t);
    let mut bufs = [Buffer::new(size), Buffer::new(size), Buffer::new(size)];
    // What each buffer is missing: all of it until it is first drawn.
    let mut ages = [Age::Whole; 3];
    // One draw into each buffer in turn, and the damage each read.
    let round = |r: &mut Renderer<'_, '_>,
                 t: &mut Term<'_>,
                 bufs: &mut [Buffer; 3],
                 ages: &mut [Age; 3]| {
        let mut damages = Vec::new();
        for i in 0..bufs.len() {
            let all = everything(r);
            match ages[i] {
                Age::Whole => bufs[i].poison(core::iter::once(all)),
                Age::Stale(s) => {
                    let bands: Vec<Band> = r.bands(&s).collect();
                    bufs[i].poison(bands.into_iter());
                }
            }
            let (damage, painted) = draw(r, t, &mut bufs[i], ages[i]);
            for (j, a) in ages.iter_mut().enumerate() {
                *a = match (j == i, *a, damage) {
                    (true, _, _) => Age::Stale(SurfaceRows::NONE),
                    (false, _, FrameDamage::Whole(_)) | (false, Age::Whole, _) => Age::Whole,
                    (false, Age::Stale(s), FrameDamage::Rows { .. }) => {
                        Age::Stale(s.union(painted.rows()))
                    }
                };
            }
            damages.push(damage);
        }
        damages
    };
    round(&mut r, &mut t, &mut bufs, &mut ages);
    // Each buffer holds the one frame drawn three times over.
    assert!(bufs[0].px == bufs[1].px && bufs[1].px == bufs[2].px);
    ages = [Age::Stale(SurfaceRows::NONE); 3];
    let old = r.clear_color_of(&t);
    r.set_flash(VideoFlash::On);
    let new = r.clear_color_of(&t);
    assert_ne!(old, new, "the premise: the flash turns the margin");
    let damages = round(&mut r, &mut t, &mut bufs, &mut ages);
    assert_eq!(
        damages[0],
        FrameDamage::Whole(WholeWhy::Surface),
        "a margin of another colour"
    );
    let (sw, sh) = (at!(size.0), at!(size.1));
    for (i, b) in bufs.iter().enumerate() {
        assert_eq!(
            b.poisoned_in(&[everything(&r)]),
            Vec::<usize>::new(),
            "buffer {i}"
        );
        let edge: Vec<usize> = (0..sh)
            .flat_map(|y| [y * sw, y * sw + sw - 1])
            .chain(0..sw)
            .filter(|&at| b.px[at] != new.0)
            .collect();
        assert_eq!(
            edge.len(),
            0,
            "buffer {i}: {} pixels beside the whole frame not the new margin {new:?} (old {old:?})",
            edge.len()
        );
    }
}

/// **A new surface, or a margin of another colour, draws every pixel**:
/// the next damage is `Whole(Surface)`, and the frame after it is drawn
/// by rows again. The same surface set again changes nothing.
#[test]
fn a_new_surface_or_margin_is_whole() {
    let data = font();
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25lsurface");
    let mut r = renderer(&faces);
    let size = chromed(&mut r, &t);
    let mut buf = Buffer::new((Px::of(size.0.value() + 8), size.1));
    draw(&mut r, &mut t, &mut buf, Age::Whole);
    let still = |r: &Renderer<'_, '_>, t: &Term<'_>| matches!(r.damage(t, Rows::ZERO), FrameDamage::Rows { rows, .. } if rows == DirtyRows::NONE);
    assert!(still(&r, &t), "{:?}", r.damage(&t, Rows::ZERO));
    r.set_surface(size, (Px::of(15), Px::of(9)));
    assert!(still(&r, &t), "the same surface again");
    r.set_surface(size, (Px::of(16), Px::of(9)));
    assert_eq!(
        r.damage(&t, Rows::ZERO),
        FrameDamage::Whole(WholeWhy::Surface),
        "the whole frame moved in the surface"
    );
    draw(&mut r, &mut t, &mut buf, Age::Stale(SurfaceRows::NONE));
    assert!(still(&r, &t), "{:?}", r.damage(&t, Rows::ZERO));
    r.set_surface(
        (Px::of(size.0.value() + 8), size.1),
        (Px::of(16), Px::of(9)),
    );
    assert_eq!(
        r.damage(&t, Rows::ZERO),
        FrameDamage::Whole(WholeWhy::Surface),
        "a surface of another size"
    );
    draw(&mut r, &mut t, &mut buf, Age::Stale(SurfaceRows::NONE));
    r.set_flash(VideoFlash::On);
    assert_eq!(
        r.damage(&t, Rows::ZERO),
        FrameDamage::Whole(WholeWhy::Surface),
        "the margin's colour turned"
    );
    draw(&mut r, &mut t, &mut buf, Age::Stale(SurfaceRows::NONE));
    assert!(still(&r, &t), "{:?}", r.damage(&t, Rows::ZERO));
}
