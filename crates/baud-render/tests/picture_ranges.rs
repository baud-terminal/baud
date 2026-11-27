//! **What a picture under the text spans is built once a placement, never
//! a frame** (`src/picture_ranges.rs`): the table of each placement under
//! the text is built on the frame it arrives, and a frame whose placements,
//! pixels and theme are the last frame's builds none
//! (`Counter::PictureRangesBuilt`). Each control moves one input and reads
//! a build: a second placement of an image, a placement drawn at another
//! size, an image's pixels edited, and the theme changed; a placement
//! moved by whole cells builds none. On the CPU path's frames and on
//! the placements a GPU path sets alike.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{Col, ImageBudget, ImagePx, Row, Rows};
use baud_font::Face;
use baud_render::counters::{self, Counter, Site};
use baud_render::{Damage, Renderer, Theme};
use support::{Canvas, WS, feed, font, kitty, renderer};

const RED: [u8; 4] = [200, 40, 40, 255];
const GREEN: [u8; 4] = [40, 200, 40, 255];

/// How a frame is drawn: by the CPU path, or the placements set as a GPU
/// path sets them before its convert.
#[derive(Clone, Copy, Debug)]
enum Path {
    Cpu,
    Gpu,
}

/// One frame of `t` on `path`, and how many range tables it built.
fn built(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, c: &mut Canvas, path: Path) -> u64 {
    let _ = counters::take();
    match path {
        Path::Cpu => {
            let mut target = c.target();
            let seal = t.seal_writes();
            r.render(t, seal, &mut target, Damage::Incremental);
        }
        Path::Gpu => {
            let _ = t.seal_writes();
            r.set_images(t, Rows::ZERO);
            r.convert(t, Rows::ZERO, Damage::Incremental);
        }
    }
    counters::take()
        .at(Site::Images, Counter::PictureRangesBuilt)
        .value()
}

/// One move of a control: what it does to the renderer and the terminal.
type Step = dyn Fn(&mut Renderer<'_, '_>, &mut Term<'_>);

/// A renderer, its canvas and a term with room for images, the cursor
/// hidden and the term told the renderer's cell size, for `f`.
fn with_images(f: impl FnOnce(&mut Renderer<'_, '_>, &mut Canvas, &mut Term<'_>)) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    let m = r.metrics();
    t.set_cell_size(
        ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
        ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
    );
    feed(&mut t, b"\x1b[?25l");
    f(&mut r, &mut c, &mut t);
}

/// **A table is built when its key or its pixels move, and on no other
/// frame**: two images under the text, one at row 3 and one at row 10, on
/// each path. The first frame builds both, and the frames after it, nothing
/// moved and then text written beside the pictures, none. Then one at a
/// time, each followed by a frame that builds none: the first placement
/// moved by whole cells, which draws the same pixels into the same cells
/// and builds none; a second placement of the first image; the first
/// placement drawn at another size; the first image's pixels edited in
/// place (`a=f` on its frame), which builds its two placements' tables and
/// keeps the other image's; and the theme changed, which builds all three.
/// The tables are what a cell reads: a cell the first placement covers has
/// a range behind it, and one beside the pictures none.
#[test]
fn a_range_table_is_built_once_a_placement_and_not_a_frame() {
    for path in [Path::Cpu, Path::Gpu] {
        with_images(|r, c, t| {
            feed(t, b"\x1b[4;3H");
            feed(
                t,
                &kitty(
                    "a=T,f=32,s=16,v=16,i=1,p=1,c=4,r=2,z=-1,C=1,q=2",
                    &[RED; 256].concat(),
                ),
            );
            feed(t, b"\x1b[11;3H");
            feed(
                t,
                &kitty(
                    "a=T,f=32,s=16,v=16,i=2,p=1,c=4,r=2,z=-1,C=1,q=2",
                    &[GREEN; 256].concat(),
                ),
            );
            let first = built(r, t, c, path);
            assert_eq!(
                first, 2,
                "{path:?}: the first frame builds a table a placement"
            );
            assert!(
                r.picture_behind(Row::from_index(3), Col::at_index(3))
                    .is_some(),
                "{path:?}: the control: a cell under the first picture has a range"
            );
            assert!(
                r.picture_behind(Row::from_index(3), Col::at_index(20))
                    .is_none(),
                "{path:?}: the control: a cell beside it has none"
            );
            assert_eq!(
                built(r, t, c, path),
                0,
                "{path:?}: a frame with nothing moved"
            );
            feed(t, b"\x1b[6;20Hwritten beside the pictures");
            assert_eq!(
                built(r, t, c, path),
                0,
                "{path:?}: a frame with text written"
            );
            let moves: [(&str, Box<Step>, u64); 5] = [
                (
                    "the placement moved by whole cells",
                    Box::new(|_, t| {
                        feed(t, b"\x1b[5;7H");
                        feed(t, &kitty("a=p,i=1,p=1,c=4,r=2,z=-1,C=1,q=2", &[]));
                    }),
                    0,
                ),
                (
                    "a second placement of the image",
                    Box::new(|_, t| {
                        feed(t, b"\x1b[15;30H");
                        feed(t, &kitty("a=p,i=1,p=2,c=4,r=2,z=-1,C=1,q=2", &[]));
                    }),
                    1,
                ),
                (
                    "drawn at another size",
                    Box::new(|_, t| {
                        feed(t, b"\x1b[5;7H");
                        feed(t, &kitty("a=p,i=1,p=1,c=6,r=3,z=-1,C=1,q=2", &[]));
                    }),
                    1,
                ),
                (
                    "its pixels edited, under both its placements",
                    Box::new(|_, t| {
                        feed(
                            t,
                            &kitty(
                                "a=f,f=32,s=8,v=8,x=0,y=0,r=1,i=1,q=2",
                                &[GREEN; 64].concat(),
                            ),
                        );
                    }),
                    2,
                ),
                (
                    "the theme changed, under all three",
                    Box::new(|r, _| r.set_theme(Theme::light())),
                    3,
                ),
            ];
            for (what, step, want) in &moves {
                step(r, t);
                assert_eq!(built(r, t, c, path), *want, "{path:?}: {what}");
                assert_eq!(built(r, t, c, path), 0, "{path:?}: the frame after {what}");
            }
        });
    }
}
