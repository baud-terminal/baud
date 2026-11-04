//! What a program's page-width request does to the window: the policy, pure,
//! so its tests run on every platform.
//!
//! **xterm's `?40` gate**: the core acts on `?3` only while `?40` allows it,
//! and asks for the width it selects -- 132 or 80 -- as it does for DECSCPP,
//! and `RIS` after a request asks back for the width the window had before
//! it (`Term::requested_width`). The window is the driver's, so the driver
//! decides: the macOS window takes the columns and keeps its rows; a grid
//! `BAUD_GRID` pinned for a benchmark stays, and so does the Linux driver's,
//! which has no window it resizes. Each refusal warns once, by its reason.
//!
//! **The width lands before the rest of the read**: the macOS driver sets
//! `WidthHold::On`, so a read stops at the request, the grid takes the new
//! width in `reads::feed`'s `after`, and the bytes after the request are
//! parsed at it. The window follows when the callback that asked returns
//! (`app.rs`, `widen_later`).

// The Linux driver reads the refusal half alone: it resizes no window.
#![cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]

use baud_core::{Rows, Term, WidthRequest, WidthRequests, WinSize};
use baud_render::Tally;

/// What the driver can do with its window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Host {
    /// The macOS window: its owner sizes it, and so does a program that asks.
    Window,
    /// `BAUD_GRID`: the grid is pinned for a benchmark.
    Pinned,
    /// A driver with no window it resizes: Linux, today.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    NoResize,
}

/// Why a request was not honoured.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    Pinned,
    NoResize,
}

/// A grid, in cells: a `WinSize` that compares and copies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
}

impl Grid {
    /// The grid `term` has.
    pub fn of(term: &Term<'_>) -> Grid {
        let (cols, rows) = term.geometry();
        Grid {
            cols: u16::try_from(cols.index()).unwrap_or(u16::MAX),
            rows: u16::try_from(rows.index()).unwrap_or(u16::MAX),
        }
    }

    pub fn ws(self) -> WinSize {
        WinSize {
            ws_col: self.cols,
            ws_row: self.rows,
        }
    }
}

/// What to do with one request.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// Resize the grid to this -- the columns asked for, the rows kept --
    /// and the window after it.
    Resize(Grid),
    /// The grid already has the columns asked for.
    Already,
    Refused(Refusal),
}

/// The policy: what a request for `asked` does to `grid` under `host`.
pub fn decide(host: Host, asked: WidthRequest, grid: Grid) -> Decision {
    if grid.cols == asked.ws_col() {
        return Decision::Already;
    }
    match host {
        Host::Window => Decision::Resize(Grid {
            cols: asked.ws_col(),
            rows: grid.rows,
        }),
        Host::Pinned => Decision::Refused(Refusal::Pinned),
        Host::NoResize => Decision::Refused(Refusal::NoResize),
    }
}

/// Whether a reason has been warned about yet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Said {
    No,
    Yes,
}

/// The driver's side: the last request it acted on, and the counts.
pub struct Widths {
    seen: WidthRequests,
    pub resized: Tally,
    pub refused: Tally,
    said: [Said; 2],
}

impl Widths {
    /// Nothing acted on: the core's count as it stands now.
    pub fn new(term: &Term<'_>) -> Widths {
        Widths {
            seen: term.width_requests(),
            resized: Tally::default(),
            refused: Tally::default(),
            said: [Said::No; 2],
        }
    }

    /// After a read: the decision for the request it made, or `None` when it
    /// made none, against the grid the core has now. Counts what it decides,
    /// and hands back a refusal's warning the first time its reason comes.
    pub fn poll(&mut self, term: &Term<'_>, host: Host) -> Option<(Decision, Option<String>)> {
        let now = term.width_requests();
        if now == self.seen {
            return None;
        }
        self.seen = now;
        let asked = term.requested_width()?;
        let d = decide(host, asked, Grid::of(term));
        let warn = match d {
            Decision::Resize(_) => {
                self.resized.bump();
                None
            }
            Decision::Already => None,
            Decision::Refused(why) => {
                self.refused.bump();
                self.first(why).then(|| describe(why, asked))
            }
        };
        Some((d, warn))
    }

    fn first(&mut self, why: Refusal) -> bool {
        let slot = &mut self.said[match why {
            Refusal::Pinned => 0,
            Refusal::NoResize => 1,
        }];
        let new = *slot == Said::No;
        *slot = Said::Yes;
        new
    }

    /// The five-second report's line, or `None` before any request moved
    /// or was refused.
    pub fn report(&self) -> Option<String> {
        let (r, x) = (self.resized.value(), self.refused.value());
        (r + x > 0).then(|| format!("page width: {r} resized, {x} refused"))
    }
}

/// The one warning for a refusal reason.
fn describe(why: Refusal, asked: WidthRequest) -> String {
    let n = asked.ws_col();
    match why {
        Refusal::Pinned => {
            format!("page width: a program asked for {n} columns; BAUD_GRID pins the grid")
        }
        Refusal::NoResize => {
            format!("page width: a program asked for {n} columns; this driver keeps its window")
        }
    }
}

/// The content width, in points, of a window whose grid is `cols` columns:
/// `relayout`'s arithmetic inverted -- the cells, a legacy scroller's
/// gutter, and the padding on each side -- and half a pixel more, so the
/// floor `grid_in` takes lands on `cols` and not one short of it.
pub fn content_width(grid: Grid, cell_px: f64, scale: f64, padding: f64, gutter_px: f64) -> f64 {
    (f64::from(grid.cols) * cell_px + gutter_px + 0.5) / scale + 2.0 * padding
}

/// The grid a window of `ws` cells holds with the status line's `status`
/// rows below it: the rows left, one at least.
pub fn less_status(ws: WinSize, status: Rows) -> WinSize {
    let line = u16::try_from(status.index()).unwrap_or(0);
    WinSize {
        ws_col: ws.ws_col,
        ws_row: ws.ws_row.saturating_sub(line).max(1),
    }
}

/// **A height in device pixels less the activity lights' own line**,
/// `line` pixels that are no row of the grid's (`Renderer::lights_height`):
/// what is left for the grid's rows, the padding and the cell's remainder.
#[cfg(any(target_os = "macos", test))]
pub fn less_line(px: f64, line: baud_font::Px) -> f64 {
    (px - f64::from(line.value())).max(0.0)
}

/// **The words of a whole frame `stride` pixels wide that a slide moves**:
/// the frame's own `rows` from `top` down (`Renderer::grid_top`), held to
/// the words `frame` has; the activity lights' line outside them holds
/// still, as the margins do.
pub fn slide_span(
    top: baud_font::Px,
    rows: baud_font::Px,
    stride: baud_font::Px,
    frame: &[u32],
) -> core::ops::Range<usize> {
    let n = |p: baud_font::Px| usize::try_from(p.value()).unwrap_or(usize::MAX);
    let s = n(stride);
    let a = n(top).saturating_mul(s).min(frame.len());
    a..n(top)
        .saturating_add(n(rows))
        .saturating_mul(s)
        .min(frame.len())
        .max(a)
}

/// The content height, in points, of a window whose grid is `grid.rows`
/// rows with the status line's `status` below them: `relayout`'s
/// arithmetic inverted -- the cells and the padding top and bottom -- and
/// half a pixel more, so the floor `grid_in` takes lands on the rows.
pub fn content_height(grid: Grid, status: Rows, cell_px: f64, scale: f64, padding: f64) -> f64 {
    let line = f64::from(u16::try_from(status.index()).unwrap_or(0));
    ((f64::from(grid.rows) + line) * cell_px + 0.5) / scale + 2.0 * padding
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{BytePos, Owned, PageWidth, ReadLen};

    const GRID: Grid = Grid {
        rows: 30,
        cols: 100,
    };

    /// **A program is told the grid and its text area without the lights'
    /// line, and the whole window with it**: a window of 24 rows of 34
    /// pixels, the padding, a 30-pixel line and 10 pixels the grid does not
    /// fill lays out a 24-row grid, and
    /// the terminal laid out so answers `CSI 18 t` with 24 rows, `CSI 14 t`
    /// with 24 rows' pixels, the line left out, and `CSI 14 ; 2 t` with the
    /// window's, the line in it -- what each driver tells the core from the
    /// window it has. Perturbation: lay the grid out from the window's
    /// height with the line still in it -- the grid is told 25 rows, one
    /// the line covers -- and this goes red.
    #[test]
    fn the_size_reports_leave_the_line_out_of_the_text_area() {
        use baud_core::{ImagePx, Owned, PixelSize, WindowFacts};
        use baud_domain::{BytePos, ReadLen};
        let (cell_w, cell_h, pad, line) = (17u32, 34u32, 16u32, 30u32);
        let (cols, rows) = (80u32, 24u32);
        let (win_w, win_h) = (cols * cell_w + 2 * pad, rows * cell_h + 2 * pad + line + 10);
        let left = less_line(f64::from(win_h - 2 * pad), baud_font::Px::of(line));
        let laid = (left / f64::from(cell_h)).floor();
        assert_eq!(laid, f64::from(rows), "the grid's rows");
        let ws = WinSize {
            ws_col: u16::try_from(cols).unwrap(),
            ws_row: u16::try_from(rows).unwrap(),
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(
            ImagePx::of(u16::try_from(cell_w).unwrap()),
            ImagePx::of(u16::try_from(cell_h).unwrap()),
        );
        t.set_window(WindowFacts {
            window: Some(PixelSize {
                width: ImagePx::of(u16::try_from(win_w).unwrap()),
                height: ImagePx::of(u16::try_from(win_h).unwrap()),
            }),
            ..WindowFacts::UNKNOWN
        });
        let ask = b"\x1b[18t\x1b[14t\x1b[14;2t";
        t.write_at(BytePos::ZERO, ask);
        t.feed(BytePos::ZERO, ReadLen::from_index(ask.len()));
        let want = format!(
            "\x1b[8;{rows};{cols}t\x1b[4;{};{}t\x1b[4;{win_h};{win_w}t",
            rows * cell_h,
            cols * cell_w
        );
        assert_eq!(String::from_utf8_lossy(t.reply()), want);
        assert_eq!(
            win_h - rows * cell_h - 2 * pad - 10,
            line,
            "the line is in the window"
        );
    }

    /// **The activity lights' line is pixels, not a row**: a window
    /// `rows` cells tall and the line's pixels more holds `rows` rows once
    /// the line is taken off, at cell heights from 12 to 40 and lines from
    /// 0 to 39 pixels -- shorter or taller than a cell -- and a line taller
    /// than the window leaves nothing. With the frame's words, a slide moves
    /// only the frame's own rows: below a line at the top, above one at the
    /// bottom. Perturbation: return `px` from `less_line` -- the grid would
    /// keep a row the line covers once the line reaches a cell's height --
    /// and this goes red.
    #[test]
    fn the_lights_line_is_taken_off_in_pixels() {
        use baud_font::Px;
        for cell in 12..=40u32 {
            for line in 0..40u32 {
                for rows in [1u32, 24, 50] {
                    let px = f64::from(rows * cell + line);
                    let left = less_line(px, Px::of(line));
                    assert_eq!(
                        (left / f64::from(cell)).floor(),
                        f64::from(rows),
                        "{cell} {line}"
                    );
                }
            }
        }
        assert_eq!(less_line(10.0, Px::of(30)), 0.0);
        let frame = vec![0u32; 10 * 8];
        assert_eq!(
            slide_span(Px::of(0), Px::of(6), Px::of(10), &frame),
            0..60,
            "line below"
        );
        assert_eq!(
            slide_span(Px::of(2), Px::of(6), Px::of(10), &frame),
            20..80,
            "line above"
        );
        assert_eq!(
            slide_span(Px::of(0), Px::of(8), Px::of(10), &frame),
            0..80,
            "no line"
        );
        assert_eq!(
            slide_span(Px::of(5), Px::of(9), Px::of(10), &frame),
            50..80,
            "held to it"
        );
    }

    /// **Every host and every width**, the two pages and a window's own
    /// that `RIS` asks back for: the window takes the columns and keeps the
    /// rows; a pinned grid and a driver without a window refuse, each by its
    /// reason; a grid that already has the columns does nothing under any
    /// host.
    #[test]
    fn decide_over_every_host_and_width() {
        for (asked, cols) in [
            (WidthRequest::Page(PageWidth::Eighty), 80),
            (WidthRequest::Page(PageWidth::OneThirtyTwo), 132),
            (WidthRequest::Window(baud_core::Cols::of_index(90)), 90),
        ] {
            let want = Grid { cols, rows: 30 };
            assert_eq!(decide(Host::Window, asked, GRID), Decision::Resize(want));
            assert_eq!(
                decide(Host::Pinned, asked, GRID),
                Decision::Refused(Refusal::Pinned)
            );
            assert_eq!(
                decide(Host::NoResize, asked, GRID),
                Decision::Refused(Refusal::NoResize)
            );
            for host in [Host::Window, Host::Pinned, Host::NoResize] {
                assert_eq!(decide(host, asked, want), Decision::Already, "{host:?}");
            }
        }
    }

    fn feed(t: &mut Term<'_>, b: &[u8]) {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }

    /// **A request is acted on once**, the next poll sees nothing, a `?3`
    /// `?40` refused asks nothing, and a refusal warns once a reason.
    #[test]
    fn a_request_is_decided_once_and_a_refusal_warns_once() {
        let mut s = Owned::new(&GRID.ws());
        let mut t = Term::new(s.storage(), &GRID.ws());
        let mut w = Widths::new(&t);
        feed(&mut t, b"\x1b[?3h");
        assert_eq!(w.poll(&t, Host::Window), None, "?40 is reset: no request");
        feed(&mut t, b"\x1b[132$|");
        let (d, warn) = w.poll(&t, Host::NoResize).expect("a request");
        assert_eq!(d, Decision::Refused(Refusal::NoResize));
        assert_eq!(
            warn.as_deref(),
            Some("page width: a program asked for 132 columns; this driver keeps its window")
        );
        assert_eq!(w.poll(&t, Host::NoResize), None, "acted on once");
        feed(&mut t, b"\x1b[80$|");
        assert_eq!(
            w.poll(&t, Host::NoResize),
            Some((Decision::Refused(Refusal::NoResize), None)),
            "said once"
        );
        feed(&mut t, b"\x1b[?40h\x1b[?3h");
        let (d, _) = w.poll(&t, Host::Window).expect("a request");
        assert_eq!(
            d,
            Decision::Resize(Grid {
                cols: 132,
                rows: 30
            })
        );
        assert_eq!(
            w.report().as_deref(),
            Some("page width: 1 resized, 2 refused")
        );
    }

    /// **The content width is the cells, the gutter and half a pixel at the
    /// scale, and the padding each side**: 80 columns of 10 px at 2x with a
    /// 34 px gutter and 4 pt of padding is (800 + 34 + 0.5) / 2 + 8 points.
    /// `app.rs` holds it against `grid_in` on macOS.
    #[test]
    fn the_content_width_is_the_cells_the_gutter_and_the_padding() {
        let grid = Grid { cols: 80, rows: 24 };
        assert_eq!(content_width(grid, 10.0, 2.0, 4.0, 34.0), 425.25);
    }

    /// A pinned grid's warning names `BAUD_GRID`.
    #[test]
    fn a_pinned_grid_says_so() {
        assert_eq!(
            describe(Refusal::Pinned, WidthRequest::Page(PageWidth::Eighty)),
            "page width: a program asked for 80 columns; BAUD_GRID pins the grid"
        );
    }
}
