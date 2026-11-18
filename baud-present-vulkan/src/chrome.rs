//! **The frame around the grid, for both paths**: the whole frame centred
//! in its target, the grid's viewport and pad inside it, and the exit-status
//! strip, the lights' line and the word wrap's marks in viewports of their
//! own -- the cell path's and the instance path's from one [`Chrome`], so
//! the two cannot place them apart.

use crate::frame::Premultiplied;
use crate::lend::Elems;
use ash::vk;
use baud_font::Px as FontPx;
use baud_present_wayland::Px;
use baud_render::Renderer;

/// A rectangle of the target in pixels, as a viewport places one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct View {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) w: f64,
    pub(crate) h: f64,
}

impl View {
    pub(crate) fn vk(self) -> vk::Viewport {
        vk::Viewport {
            x: self.x as f32,
            y: self.y as f32,
            width: self.w as f32,
            height: self.h as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }
    }

    /// Whether a viewport can be made of it: Vulkan takes no empty one.
    pub(crate) fn drawable(self) -> bool {
        self.w > 0.0 && self.h > 0.0
    }
}

/// **The whole frame's parts**, in its pixels: the margin, the exit-status
/// strip, the grid's frame's top and height, and the lights' line's.
struct Parts {
    margin: f64,
    strip: f64,
    top: f64,
    rows: f64,
    line_top: f64,
    line: f64,
}

/// [`letterbox_whole`]'s viewports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Boxed {
    pub(crate) grid: View,
    pub(crate) pad: [f32; 2],
    pub(crate) strip: View,
    pub(crate) line: View,
    pub(crate) rows: View,
}

/// **The whole frame centred in the target**, on whole pixels, the odd one
/// right and bottom as the drivers place it; then the grid's viewport and
/// pad the margin and the strip in from the whole's left and the grid's
/// top down, the strip's viewport past the left margin over the grid's
/// rows, the line's across the whole width at its rows, and the grid's rows
/// across the whole width.
fn letterbox_whole(target: (f64, f64), whole: (f64, f64), parts: &Parts) -> Boxed {
    let vp = View {
        x: ((target.0 - whole.0).max(0.0) / 2.0).floor(),
        y: ((target.1 - whole.1).max(0.0) / 2.0).floor(),
        w: whole.0,
        h: whole.1,
    };
    let rows = View {
        y: vp.y + parts.top,
        h: parts.rows,
        ..vp
    };
    let grid = View {
        x: rows.x + parts.margin + parts.strip,
        w: (rows.w - 2.0 * parts.margin - parts.strip).max(0.0),
        ..rows
    };
    let strip = View {
        x: rows.x + parts.margin,
        w: parts.strip,
        ..rows
    };
    let line = View {
        y: vp.y + parts.line_top,
        h: parts.line,
        ..vp
    };
    Boxed {
        grid,
        pad: [grid.x as f32, grid.y as f32],
        strip,
        line,
        rows,
    }
}

/// The rectangle of the target from `pad` that is `size` big, clamped to
/// the target: the first pixel whose centre the pad covers is its floor.
pub(crate) fn scissor(target: (Px, Px), pad: [f32; 2], size: (Px, Px)) -> vk::Rect2D {
    let floor = |p: f32| u32::try_from(p.max(0.0).floor() as i64).unwrap_or(0);
    let (x, y) = (floor(pad[0]).min(target.0.0), floor(pad[1]).min(target.1.0));
    vk::Rect2D {
        offset: vk::Offset2D {
            x: i32::try_from(x).unwrap_or(i32::MAX),
            y: i32::try_from(y).unwrap_or(i32::MAX),
        },
        extent: vk::Extent2D {
            width: size.0.0.min(target.0.0 - x),
            height: size.1.0.min(target.1.0 - y),
        },
    }
}

/// A strip's records, its viewport and the height its uniform is given.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Strip {
    pub(crate) from: Elems,
    pub(crate) to: Elems,
    pub(crate) view: View,
    pub(crate) height: FontPx,
}

impl Strip {
    /// Whether it has records and a viewport to draw them in.
    pub(crate) fn drawn(&self) -> bool {
        self.to.0 > self.from.0 && self.view.drawable()
    }

    /// Its scissor in a target of `target`.
    pub(crate) fn scissor(&self, target: (Px, Px)) -> vk::Rect2D {
        let width = Px(u32::try_from(self.view.w as i64).unwrap_or(0));
        let origin = [self.view.x as f32, self.view.y as f32];
        scissor(target, origin, (width, Px(self.height.value())))
    }

    /// **Its records' uniform `U`**: the strip's width and height as the
    /// viewport, the cell and the sheets' sides as the grid's, and its
    /// viewport's origin as the pad.
    pub(crate) fn u(&self, cell: (f32, f32), sides: (f32, f32)) -> [f32; 12] {
        [
            (self.view.w.max(0.0) as f32).trunc(),
            self.height.value() as f32,
            cell.0,
            cell.1,
            0.0,
            0.0,
            sides.0,
            sides.0,
            sides.1,
            sides.1,
            self.view.x as f32,
            self.view.y as f32,
        ]
    }
}

/// **The frame's chrome in a target**: the page it is cleared to, its
/// viewports, and the heights the strips' uniforms are given.
#[derive(Clone, Copy, Debug)]
pub struct Chrome {
    pub(crate) clear: Premultiplied,
    pub(crate) boxed: Boxed,
    frame_height: FontPx,
    lights_height: FontPx,
}

impl Chrome {
    /// **The chrome `renderer` lays out**, its whole frame centred in a
    /// target `target` big.
    pub fn of(renderer: &Renderer<'_, '_>, target: (Px, Px)) -> Chrome {
        let fpx = |p: FontPx| f64::from(p.value());
        let whole = renderer.whole_size();
        let frame_height = renderer.frame_size().1;
        let lights_height = renderer.lights_height();
        let boxed = letterbox_whole(
            (f64::from(target.0.0), f64::from(target.1.0)),
            (fpx(whole.0), fpx(whole.1)),
            &Parts {
                margin: fpx(renderer.margins()),
                strip: fpx(renderer.strip()),
                top: fpx(renderer.grid_top()),
                rows: fpx(frame_height),
                line_top: fpx(renderer.lights_top()),
                line: fpx(lights_height),
            },
        );
        Chrome {
            clear: Premultiplied(baud_domain::premultiply(renderer.clear_color().0)),
            boxed,
            frame_height,
            lights_height,
        }
    }

    /// The grid's pad: where its first cell's corner sits in the target.
    pub fn pad(&self) -> [f32; 2] {
        self.boxed.pad
    }

    /// **Where each run of records puts its origin in the target**: the
    /// grid's, then the exit-status strip's, the lights' line's and the
    /// word wrap's marks'.
    pub fn origins(&self) -> [[f32; 2]; 4] {
        let at = |v: View| [v.x as f32, v.y as f32];
        [
            self.boxed.pad,
            at(self.boxed.strip),
            at(self.boxed.line),
            at(self.boxed.rows),
        ]
    }

    /// **The three strips over records from `first`**, the exit-status
    /// strip's `lens[0]`, the lights' line's `lens[1]` and the word wrap's
    /// marks' `lens[2]`, in that order.
    pub(crate) fn strips(&self, first: Elems, lens: [usize; 3]) -> [Strip; 3] {
        let a = first.0;
        let (b, c, e) = (
            a + lens[0],
            a + lens[0] + lens[1],
            a + lens[0] + lens[1] + lens[2],
        );
        [
            Strip {
                from: Elems(a),
                to: Elems(b),
                view: self.boxed.strip,
                height: self.frame_height,
            },
            Strip {
                from: Elems(b),
                to: Elems(c),
                view: self.boxed.line,
                height: self.lights_height,
            },
            Strip {
                from: Elems(c),
                to: Elems(e),
                view: self.boxed.rows,
                height: self.frame_height,
            },
        ]
    }
}
