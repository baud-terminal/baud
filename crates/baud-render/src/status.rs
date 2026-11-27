//! **Each command's exit status, beside its prompt**: a mark in a strip
//! one cell wide at the frame's left, on the row a shell's prompt began
//! (`OSC 133 ; A`), for the status the shell's `D` sent
//! (`[[source: semantic-prompts]]`: "0 means the command succeeded, and
//! other values indicate failure"; `err=`'s value "should be displayed as
//! an error, even 0"). A prompt whose command has not ended, or whose `D`
//! said nothing, has no mark: nothing is guessed.
//!
//! **The look** (over iTerm2's one triangle, Warp's tinted block and a line
//! across the row, each rendered): a filled disc for success, a cross for
//! failure and a ring for a cancelled command, VS Code's scheme
//! (`[[source: vscode-terminal-icons-ec01053]]`), so the shape carries the
//! state and the colour repeats it -- "Avoid relying solely on color to
//! differentiate between objects ... you can use text labels or glyph
//! shapes to identify objects or states" (`[[source: apple-hig-color]]`,
//! Inclusive color). The colours are the palette's, so a theme or `OSC 4`
//! moves them: entry 4 for success and 1 for failure, iTerm2's and VS
//! Code's blue and red, and 8 for a cancelled command.
//!
//! **Where**: a strip of its own, which the grid gives a column to while
//! the setting is on, so no mark lies on a program's text; the setting is
//! off unless set. Each frame reads the prompts on the window it shows
//! ([`TermRead::prompts_from`]), so a mark moves with its line through
//! scrolling, a reflow and the history. The alternate screen has no
//! prompts: a full-screen program's rows have no marks.

use baud_core::{ExitCode, ExitStatus, PromptLine, Screen};
use baud_domain::TermRead;
use baud_domain::{Chan, Row, Rows};
use baud_font::{Canvas, Extent, Point, Px};

use crate::units::slots;

/// **Whether exit-status marks are drawn**: `[screen] exit_status`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StatusMarks {
    On,
    #[default]
    Off,
}

/// What a prompt's mark says of the command it began.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Success,
    Failure,
    Cancelled,
}

impl Mark {
    /// The mark for `s`, or none for a command not reported ended.
    pub fn of(s: ExitStatus) -> Option<Mark> {
        match s {
            ExitStatus::Unreported => None,
            ExitStatus::Exited(c) if c == ExitCode::of(0) => Some(Mark::Success),
            ExitStatus::Succeeded => Some(Mark::Success),
            ExitStatus::Exited(_) | ExitStatus::Failed(_) => Some(Mark::Failure),
            ExitStatus::Cancelled => Some(Mark::Cancelled),
        }
    }

    /// The palette entry it is drawn in.
    pub(crate) fn slot(self) -> Chan {
        Chan::from_u8(match self {
            Mark::Success => 4,
            Mark::Failure => 1,
            Mark::Cancelled => 8,
        })
    }

    /// Its mask's key among the atlas's procedural masks, past the
    /// encircled run's (`Ring::KEY` and the three after it).
    pub(crate) fn key(self) -> crate::CellKey {
        crate::CellKey(
            STATUS_KEY
                + match self {
                    Mark::Success => 0,
                    Mark::Failure => 1,
                    Mark::Cancelled => 2,
                },
        )
    }
}

/// The first key of the marks' masks.
const STATUS_KEY: u32 = (1 << 27) + 8;

/// A mark on the window: the row it is on, `-1` for the line a smooth
/// scroll is drawing above the window, and what it says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placed {
    pub row: ViewRow,
    pub mark: Mark,
}

/// A row of the window, or the leaving line above it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewRow {
    Leaving,
    On(Row),
}

/// **The marks on the window `back` lines up the history**, of its `rows`
/// main rows, top first; with `leaving`, the line above the window too, as
/// the cell path's smooth scroll draws it. None on the alternate screen.
pub fn placed(
    term: &impl TermRead,
    back: Rows,
    rows: Rows,
    leaving: Leaving,
    out: &mut Vec<Placed>,
) {
    out.clear();
    if term.screen() == Screen::Alternate {
        return;
    }
    // The leaving line is the one above the window: a window one deeper
    // starts on it.
    let (from, shift) = match leaving {
        Leaving::Drawn => (Rows::of_index(back.index() + 1), 1),
        Leaving::None => (back, 0),
    };
    for p in term.prompts_from(from) {
        let at = match p.line {
            PromptLine::Back(n) => from.index().checked_sub(n.index()),
            PromptLine::Screen(r) => Some(from.index() + r.index()),
        };
        let Some(at) = at else { continue };
        if at >= rows.index() + shift {
            break;
        }
        let row = match (shift, at) {
            (1, 0) => ViewRow::Leaving,
            _ => ViewRow::On(Row::from_index(at - shift)),
        };
        if let Some(mark) = Mark::of(p.status) {
            out.push(Placed { row, mark });
        }
    }
}

/// Whether a smooth scroll draws the line above the window this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Leaving {
    Drawn,
    None,
}

/// **The mark's mask**, `w` by `h`, the cell's box, centred: a disc of
/// radius a third of the smaller side for success, a ring of that radius
/// with a stroke of a quarter of it for a cancelled command, a cross of
/// that half-width and the ring's stroke for failure.
pub(crate) fn draw(
    mark: Mark,
    w: Px,
    h: Px,
    scratch: &mut Vec<f32>,
    coverage: &mut Vec<u8>,
) -> Option<Extent> {
    let extent = Extent::new(w, h);
    let cells = extent.cells()?;
    if coverage.len() < cells {
        coverage.resize(cells, 0);
    }
    coverage[..cells].fill(0);
    let need = extent.scratch()?;
    if scratch.len() < need {
        scratch.resize(need, 0.0);
    }
    let (wf, hf) = (fx(w), fx(h));
    let c = Point::new(wf / 2.0, hf / 2.0);
    let r = (wf.min(hf) / 3.0).max(2.0);
    let t = (r / 4.0).max(1.0);
    let mut canvas = Canvas::new(extent, scratch)?;
    match mark {
        Mark::Success => circle(&mut canvas, c, r, Turn::Clockwise),
        Mark::Cancelled => {
            circle(&mut canvas, c, r, Turn::Clockwise);
            circle(&mut canvas, c, r - t, Turn::Counter);
        }
        Mark::Failure => {
            let a = r * core::f32::consts::FRAC_1_SQRT_2;
            bar(
                &mut canvas,
                Point::new(c.x - a, c.y - a),
                Point::new(c.x + a, c.y + a),
                t,
            );
            bar(
                &mut canvas,
                Point::new(c.x - a, c.y + a),
                Point::new(c.x + a, c.y - a),
                t,
            );
        }
    }
    canvas.coverage(&mut coverage[..cells])?;
    Some(extent)
}

/// Which way a contour turns: an inner one the other way round leaves its
/// inside at zero.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Turn {
    Clockwise,
    Counter,
}

/// A circle of four cubics about `c`.
fn circle(canvas: &mut Canvas<'_>, c: Point, r: f32, turn: Turn) {
    const KAPPA: f32 = 0.552_284_8;
    let k = KAPPA * r;
    let s = match turn {
        Turn::Clockwise => 1.0,
        Turn::Counter => -1.0,
    };
    let at = |dx: f32, dy: f32| Point::new(c.x + dx, c.y + s * dy);
    canvas.cubic(at(r, 0.0), at(r, k), at(k, r), at(0.0, r));
    canvas.cubic(at(0.0, r), at(-k, r), at(-r, k), at(-r, 0.0));
    canvas.cubic(at(-r, 0.0), at(-r, -k), at(-k, -r), at(0.0, -r));
    canvas.cubic(at(0.0, -r), at(k, -r), at(r, -k), at(r, 0.0));
}

/// A stroke `t` wide from `a` to `b`, as the quadrilateral round it.
fn bar(canvas: &mut Canvas<'_>, a: Point, b: Point, t: f32) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
    let (nx, ny) = (-dy / len * t / 2.0, dx / len * t / 2.0);
    let p = [
        Point::new(a.x + nx, a.y + ny),
        Point::new(b.x + nx, b.y + ny),
        Point::new(b.x - nx, b.y - ny),
        Point::new(a.x - nx, a.y - ny),
    ];
    for i in 0..4 {
        canvas.line(p[i], p[(i + 1) % 4]);
    }
}

fn fx(p: Px) -> f32 {
    // A cell is a few dozen pixels: exact in an `f32`.
    #[allow(clippy::cast_precision_loss)]
    let v = slots(p) as f32;
    v
}
