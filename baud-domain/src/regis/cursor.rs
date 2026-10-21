//! ReGIS's graphics input and its cursors: `R(I<n>)`, `R(P(I))` and
//! `S(C ...)`, the Open Extensions of 070-8 § 8.8.5.2, § 8.8.5.3 and
//! § 8.8.1.4.2, and the graphics input of 070-13 § 5 (`[[source:
//! dec-std-070]]`), with the VT330/VT340's practice where the text leaves
//! room (`[[source: vt3xx-report]]`, `[[source: vt3xx-locator]]`,
//! `[[source: vt3xx-screen-control]]`).
//!
//! **A cursor is drawn by its caller into the page it shows, never into the
//! plane of indices**, so a drawing, a complement or a recolour reads back
//! what the program drew and nothing of the cursor. [`RegisCursor`] says
//! where it is in page pixels: [`RegisCursor::paint`] the pixels to colour
//! and with which of four tones, [`RegisCursor::cover`] the rectangles to
//! show from the plane again to take it away.

use super::*;

/// `R(I<n>)`: how graphics input runs. One-shot is "the default mode and is
/// set by executing a valid S(A)" (§ 8.8.5.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegisInputMode {
    /// `R(I0)`: a report waits for a key or a button, the host's output
    /// held meanwhile.
    OneShot,
    /// `R(I1)`: reports come from the buttons and from `R(P(I))` at once,
    /// and the host's output goes on.
    Multiple,
}

/// **What graphics input is doing**, for the embedder that feeds the core
/// its keys and pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegisInput {
    /// None: the keys and the pointer are the terminal's as ever.
    Off,
    /// A one-shot `R(P(I))` waits: the arrow keys and the pointer move the
    /// input cursor, any other key or a button ends it with a report, and
    /// the host's output is held until it does (070-13 § 5.1, "no further
    /// data from the application (screen output) is processed until the
    /// locator input is complete").
    OneShot,
    /// Multiple mode inside a ReGIS envelope: the pointer moves the cursor
    /// and its buttons report; the keys are the terminal's.
    Multiple,
}

/// A cursor's shape. **The input cursor is numbered as the VT330/VT340
/// manual's table numbers it** (`[[source: vt3xx-screen-control]]`):
/// omitted and 0 the crosshair, 0 marked "(default)", 1 the diamond, 2 the
/// crosshair, 3 and 4 the rubber band line and rectangle, and the
/// crosshair is what power-up and a valid `S(A)` select; 070-13 § 5.1
/// says of one-shot input "a cross hair cursor appears", and a real VT340
/// (`[[source: vt340test-locator]]`) shows "fine crosshairs". Jev's lean, VT340, at
/// confidences 0.59 and 0.67 after a re-ask, below the bar, taken as the
/// lean. **An open departure from 070-8 § 8.8.1.4.2** (`[[source:
/// dec-std-070]]`), whose selection syntax "has the force of a standard":
/// "A value of zero or 1 for <n> selects a the diamond cursor ... This too
/// is the default cursor, and is selected after a valid S(A) has been
/// executed", so `I0`, an
/// omitted number and `S(A)` select the crosshair here where 070-8 selects
/// the diamond, and `S(A)` is not § 8.8.1.4.1 g's "S(C(H1)(I1)1)", `I1`
/// being the diamond. The output cursor's numbers, `H`, are the same in
/// both texts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegisCursorStyle {
    /// "A 21 x 21 pixel diamond" (`[[source: vt3xx-regis-intro]]`) "with a
    /// small crosshair inscribed" (070-8).
    Diamond,
    /// "a horizontal and a vertical line. The horizontal line is the width
    /// of the screen, and the vertical line is the height of the screen."
    Crosshair,
    /// Input only: a line "anchored ... at the output drawing position in
    /// effect at the start of the input operation", its other end on the
    /// input position.
    RubberLine,
    /// Input only: a rectangle anchored so, its opposite corner on the
    /// input position.
    RubberRect,
    /// Two characters overlaid, `S(C(I[+x,+y]"XO"))` (070-8 § 8.8.1.4.2 c).
    Pair,
}

impl RegisCursorStyle {
    /// `S(C(H<n>))`: 0, 1 and omitted the diamond, 2 the crosshair.
    fn output(n: Option<i64>) -> Option<RegisCursorStyle> {
        match n {
            None | Some(0) | Some(1) => Some(RegisCursorStyle::Diamond),
            Some(2) => Some(RegisCursorStyle::Crosshair),
            _ => None,
        }
    }

    /// `S(C(I<n>))`, the VT340's numbers: 0, 2 and omitted the crosshair,
    /// 1 the diamond, 3 and 4 the rubber bands.
    fn input(n: Option<i64>) -> Option<RegisCursorStyle> {
        match n {
            Some(1) => Some(RegisCursorStyle::Diamond),
            None | Some(0) | Some(2) => Some(RegisCursorStyle::Crosshair),
            Some(3) => Some(RegisCursorStyle::RubberLine),
            Some(4) => Some(RegisCursorStyle::RubberRect),
            _ => None,
        }
    }
}

/// The largest character a [`RegisCursorStyle::Pair`] keeps, device pixels: "The
/// cursor size is limited to 16 x 24 pixels. If you define a larger cursor,
/// the terminal clips the cursor to fit into the 16 x 24 cell"
/// (`[[source: vt3xx-screen-control]]`).
pub const REGIS_CURSOR_CELL: (i32, i32) = (16, 24);

/// The two characters of a user-defined cursor, as device-pixel masks, a
/// row of up to 16 bits each, the leftmost the high bit, and the offset
/// from the cell's upper-left to the point it tracks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisCursorPair {
    /// The first character, "displayed in the foreground color".
    pub fg: [u16; 24],
    /// The second, "displayed in the background color".
    pub bg: [u16; 24],
    pub w: i32,
    pub h: i32,
    /// "the distance, in character cell pixels, from the origin of the
    /// character cell to the point that will be tracked".
    pub off: (i32, i32),
}

impl RegisCursorPair {
    const NONE: RegisCursorPair = RegisCursorPair {
        fg: [0; 24],
        bg: [0; 24],
        w: 0,
        h: 0,
        off: (0, 0),
    };

    fn bit(mask: &[u16; 24], x: i32, y: i32) -> bool {
        if !(0..16).contains(&x) || !(0..24).contains(&y) {
            return false;
        }
        mask[y as usize] & (0x8000 >> x) != 0
    }
}

/// The cursors' settings: `S(C<n>)`, `S(C(H...))`, `S(C(I...))`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Cursors {
    pub(crate) output: RegisCursorStyle,
    pub(crate) input: RegisCursorStyle,
    pub(crate) output_pair: RegisCursorPair,
    pub(crate) input_pair: RegisCursorPair,
    /// `S(C1)` and `S(C0)`: "0 turns the output cursor off. 1 turns the
    /// output cursor on" (`[[source: vt3xx-screen-control]]`).
    pub(crate) shown: Shown,
}

/// Whether the output cursor is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shown {
    No,
    Yes,
}

impl Cursors {
    /// What a valid `S(A)` and power-up select: "Cursor visibility is set
    /// to ON, and the default cursors are selected" (070-8 § 8.8.1.4.1 g).
    pub(crate) const DEFAULT: Cursors = Cursors {
        output: RegisCursorStyle::Diamond,
        input: RegisCursorStyle::Crosshair,
        output_pair: RegisCursorPair::NONE,
        input_pair: RegisCursorPair::NONE,
        shown: Shown::Yes,
    };

    pub(crate) fn set_output(&mut self, n: Option<i64>) -> Option<()> {
        self.output = RegisCursorStyle::output(n)?;
        Some(())
    }

    pub(crate) fn set_input(&mut self, n: Option<i64>) -> Option<()> {
        self.input = RegisCursorStyle::input(n)?;
        Some(())
    }
}

/// Which of a cursor's colours a pixel takes. The VT340's: "The graphics
/// cursor is a composite character in two shades or colors ... The VT340
/// uses location 0 for the foreground, and location 15 for the
/// background", and for a user-defined cursor "color map entry 14 for the
/// foreground and entry 1 for the background" (`[[source:
/// vt3xx-screen-control]]`). A standard cursor's stroke takes the first and
/// a one-pixel border round it the second, so it shows on black and on a
/// light drawing alike: decided by Jev, TWO_TONE, 1.00 and 1.00.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegisCursorTone {
    /// A standard cursor's line: map entry 0.
    Stroke,
    /// The border round it: map entry 15.
    Border,
    /// A user-defined cursor's first character: entry 14.
    Fore,
    /// Its second character: entry 1.
    Back,
}

impl RegisCursorTone {
    /// The output map entry the tone is drawn in.
    pub fn index(self) -> RegisIndex {
        match self {
            RegisCursorTone::Stroke => RegisIndex::wrap(0),
            RegisCursorTone::Border => RegisIndex::wrap(15),
            RegisCursorTone::Fore => RegisIndex::wrap(14),
            RegisCursorTone::Back => RegisIndex::wrap(1),
        }
    }
}

/// A cursor resolved to page pixels: where to draw it and what to take
/// away again. Copied by the caller that drew it, so taking it away needs
/// nothing the interpreter may since have changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisCursor {
    shape: Shape,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    /// Lines across the device's area through `at`; `end` is the area's
    /// lower-right pixel.
    Crosshair {
        at: RegisPoint,
        end: RegisPoint,
    },
    /// The diamond round `at`, `r` page pixels to each vertex, its
    /// diagonals drawn.
    Diamond {
        at: RegisPoint,
        r: i32,
    },
    Line {
        a: RegisPoint,
        b: RegisPoint,
    },
    Rect {
        a: RegisPoint,
        b: RegisPoint,
    },
    /// A pair's cell from `at`, `size` page pixels for the pair's `w` by
    /// `h` device pixels.
    Pair {
        at: RegisPoint,
        size: (i32, i32),
        pair: RegisCursorPair,
    },
}

/// Lines are cut into pieces this many steps long for [`RegisCursor::cover`],
/// so a long diagonal is taken away a few small rectangles at a time and not
/// by its bounding box.
const COVER_STEP: i64 = 16;

impl RegisCursor {
    pub(crate) fn crosshair(at: RegisPoint, end: RegisPoint) -> RegisCursor {
        RegisCursor {
            shape: Shape::Crosshair { at, end },
        }
    }

    pub(crate) fn diamond(at: RegisPoint, r: i32) -> RegisCursor {
        RegisCursor {
            shape: Shape::Diamond { at, r },
        }
    }

    pub(crate) fn line(a: RegisPoint, b: RegisPoint) -> RegisCursor {
        RegisCursor {
            shape: Shape::Line { a, b },
        }
    }

    pub(crate) fn rect(a: RegisPoint, b: RegisPoint) -> RegisCursor {
        RegisCursor {
            shape: Shape::Rect { a, b },
        }
    }

    pub(crate) fn pair(at: RegisPoint, size: (i32, i32), pair: RegisCursorPair) -> RegisCursor {
        RegisCursor {
            shape: Shape::Pair { at, size, pair },
        }
    }

    /// The lines a stroked shape is made of: up to four.
    fn lines(&self) -> ([(RegisPoint, RegisPoint); 6], usize) {
        let p = |x: i32, y: i32| RegisPoint { x, y };
        let mut out = [(p(0, 0), p(0, 0)); 6];
        let n = match self.shape {
            Shape::Crosshair { at, end } => {
                out[0] = (p(0, at.y), p(end.x, at.y));
                out[1] = (p(at.x, 0), p(at.x, end.y));
                2
            }
            Shape::Diamond { at, r } => {
                let (l, t, rt, b) = (
                    p(at.x - r, at.y),
                    p(at.x, at.y - r),
                    p(at.x + r, at.y),
                    p(at.x, at.y + r),
                );
                out[0] = (l, t);
                out[1] = (t, rt);
                out[2] = (rt, b);
                out[3] = (b, l);
                out[4] = (l, rt);
                out[5] = (t, b);
                6
            }
            Shape::Line { a, b } => {
                out[0] = (a, b);
                1
            }
            Shape::Rect { a, b } => {
                out[0] = (p(a.x, a.y), p(b.x, a.y));
                out[1] = (p(b.x, a.y), p(b.x, b.y));
                out[2] = (p(b.x, b.y), p(a.x, b.y));
                out[3] = (p(a.x, b.y), p(a.x, a.y));
                4
            }
            Shape::Pair { .. } => 0,
        };
        (out, n)
    }

    /// **Every pixel of the cursor and its tone**, borders before strokes
    /// and a pair's second character before its first, so the later tone
    /// wins where two meet. Pixels off the page are the caller's to clip.
    pub fn paint(&self, f: &mut dyn FnMut(i32, i32, RegisCursorTone)) {
        if let Shape::Pair { at, size, pair } = self.shape {
            let (pw, ph) = (size.0.max(1), size.1.max(1));
            for (mask, tone) in [
                (&pair.bg, RegisCursorTone::Back),
                (&pair.fg, RegisCursorTone::Fore),
            ] {
                for y in 0..ph {
                    for x in 0..pw {
                        let (u, v) = (x * pair.w / pw, y * pair.h / ph);
                        if RegisCursorPair::bit(mask, u, v) {
                            f(at.x + x, at.y + y, tone);
                        }
                    }
                }
            }
            return;
        }
        let (lines, n) = self.lines();
        for &(a, b) in &lines[..n] {
            each_pixel(a, b, &mut |x, y| {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if (dx, dy) != (0, 0) {
                            f(x + dx, y + dy, RegisCursorTone::Border);
                        }
                    }
                }
            });
        }
        for &(a, b) in &lines[..n] {
            each_pixel(a, b, &mut |x, y| f(x, y, RegisCursorTone::Stroke));
        }
    }

    /// **The rectangles that hold every pixel [`RegisCursor::paint`]
    /// draws**, to show again from the plane: a line's border a piece at a
    /// time, a pair's cell whole.
    pub fn cover(&self, f: &mut dyn FnMut(RegisRect)) {
        if let Shape::Pair { at, size, .. } = self.shape {
            f(RegisRect {
                x0: at.x,
                y0: at.y,
                x1: at.x + size.0.max(1),
                y1: at.y + size.1.max(1),
            });
            return;
        }
        let (lines, n) = self.lines();
        for &(a, b) in &lines[..n] {
            let (dx, dy) = (
                i64::from(b.x) - i64::from(a.x),
                i64::from(b.y) - i64::from(a.y),
            );
            let steps = dx.abs().max(dy.abs());
            let mut k = 0;
            loop {
                let k1 = (k + COVER_STEP).min(steps);
                let (x0, y0) = step_at(a, dx, dy, steps, k);
                let (x1, y1) = step_at(a, dx, dy, steps, k1);
                f(RegisRect {
                    x0: x0.min(x1) - 1,
                    y0: y0.min(y1) - 1,
                    x1: x0.max(x1) + 2,
                    y1: y0.max(y1) + 2,
                });
                if k1 >= steps {
                    break;
                }
                k = k1;
            }
        }
    }
}

/// The pixel of step `k` of `steps` along `(dx, dy)` from `a`, rounded, as
/// the page's own lines step ([`RegisCanvas::line`]).
fn step_at(a: RegisPoint, dx: i64, dy: i64, steps: i64, k: i64) -> (i32, i32) {
    if steps == 0 {
        return (a.x, a.y);
    }
    let r = |d: i64| -> i32 {
        let n = d * k * 2;
        let q = if n >= 0 {
            (n + steps) / (2 * steps)
        } else {
            -((-n + steps) / (2 * steps))
        };
        q as i32
    };
    (a.x + r(dx), a.y + r(dy))
}

/// Every pixel of the line from `a` to `b`, both ends.
fn each_pixel(a: RegisPoint, b: RegisPoint, f: &mut dyn FnMut(i32, i32)) {
    let (dx, dy) = (
        i64::from(b.x) - i64::from(a.x),
        i64::from(b.y) - i64::from(a.y),
    );
    let steps = dx.abs().max(dy.abs());
    for k in 0..=steps {
        let (x, y) = step_at(a, dx, dy, steps, k);
        f(x, y);
    }
}

/// **The cursors drawn into a page's shown image**, as they were resolved
/// when drawn, kept so they can be taken away again from the plane under
/// them: the page's two images, its size and the output and input cursors.
/// Kept here, behind the reference the terminal keeps to ReGIS, so the
/// terminal's own storage does not grow for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisDrawn {
    pub shown: crate::image::ImageId,
    pub plane: crate::image::ImageId,
    pub w: crate::image::ImagePx,
    pub h: crate::image::ImagePx,
    pub marks: [Option<RegisCursor>; 2],
}

impl RegisDrawn {
    /// Nothing drawn.
    pub const NONE: RegisDrawn = RegisDrawn {
        shown: crate::image::ImageId::NONE,
        plane: crate::image::ImageId::NONE,
        w: crate::image::ImagePx::ZERO,
        h: crate::image::ImagePx::ZERO,
        marks: [None, None],
    };
}

/// The input cursor's state: the mode, whether a one-shot report waits,
/// where the cursor is on the device and where a rubber band is anchored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Input {
    pub(crate) mode: RegisInputMode,
    pub(crate) pending: Pending,
    /// On the 800 by 480 device, in 1/65536 pixel.
    pub(crate) at: (i64, i64),
    pub(crate) anchor: (i64, i64),
}

/// Whether a one-shot `R(P(I))` waits for the user.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Pending {
    No,
    Yes,
}

impl Input {
    pub(crate) const NEW: Input = Input {
        mode: RegisInputMode::OneShot,
        pending: Pending::No,
        at: (0, 0),
        anchor: (0, 0),
    };

    /// A step of whole device pixels, **wrapping** at the screen's edge:
    /// "If you move the cursor past a screen boundary, the cursor wraps to
    /// the other side of the screen" (`[[source: vt3xx-report]]`).
    pub(crate) fn step(&mut self, dx: i32, dy: i32) {
        let (w, h) = (i64::from(REGIS_WIDTH) << 16, i64::from(REGIS_HEIGHT) << 16);
        self.at = (
            (self.at.0 + (i64::from(dx) << 16)).rem_euclid(w),
            (self.at.1 + (i64::from(dy) << 16)).rem_euclid(h),
        );
    }
}

/// A value of 1/65536 user units to a tenth, half away from zero, written
/// as `-12.5` or `200`: Jev's decision, TENTHS (0.96 and 0.98), the
/// VT340's report example `[102.5,200]` (`[[source: vt3xx-locator]]`).
pub(crate) fn tenths(v: i64, out: &mut [u8; 24]) -> usize {
    let neg = v < 0;
    let a = i128::from(v).abs();
    let t = (a * 10 + (1 << 15)) >> 16;
    let (whole, frac) = (t / 10, t % 10);
    let mut digits = [0u8; 24];
    let mut nd = 0;
    let mut w = whole;
    loop {
        digits[nd] = b'0' + (w % 10) as u8;
        nd += 1;
        w /= 10;
        if w == 0 || nd == 20 {
            break;
        }
    }
    let mut n = 0;
    if neg && t != 0 {
        out[n] = b'-';
        n += 1;
    }
    for i in (0..nd).rev() {
        out[n] = digits[i];
        n += 1;
    }
    if frac != 0 {
        out[n] = b'.';
        out[n + 1] = b'0' + frac as u8;
        n += 2;
    }
    n
}
