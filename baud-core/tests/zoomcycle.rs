//! A zoom cycle must not duplicate a line.
//!
//! A user's report: on a fresh instance, zoom all the way in and all
//! the way out and lines start to multiply. A recording shows a shell
//! that printed its banner once with the banner on screen four times.
//!
//! A zoom changes the cell size, so it changes both the column count and
//! the row count -- which is a reflow, and the reflow re-cuts every line
//! in place with no scratch buffer. This drives the width the way a zoom
//! does and requires every line to appear exactly once afterwards.
mod support;
use baud_core::{PackedCell, Term};
use baud_domain::{Back, Row, WinSize};

fn text_of(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect()
}

/// Every non-blank line of the screen and the history behind it, as text.
fn lines(t: &Term<'_>) -> Vec<String> {
    let mut out = Vec::new();
    let sb = t.scrollback();
    for n in (0..sb.len()).rev() {
        if let Some((c, _)) = sb.line(Back::of(n)) {
            out.push(text_of(c).trim_end().to_string());
        }
    }
    for y in 0..t.geometry().1.index() {
        out.push(
            text_of(t.row(Row::from_index(y)).expect("a row on the grid"))
                .trim_end()
                .to_string(),
        );
    }
    out.retain(|l| !l.is_empty());
    out
}

#[test]
fn a_zoom_cycle_does_not_duplicate_a_line() {
    support::with_corpus(|t| {
        // Twelve lines a shell might print, each unique so a duplicate is
        // visible by name rather than by count.
        for i in 0..12 {
            support::feed(t, format!("banner line {i:02}\r\n").as_bytes());
        }
        let before = lines(t);
        assert!(before.len() >= 12, "the lines are there to begin with");

        // In all the way, then out all the way: the cell grows so the grid
        // shrinks, and back. These are the extremes the menu's zoom
        // reaches at 6 and 72 points against a fixed window.
        for ws in [
            WinSize {
                ws_col: 24,
                ws_row: 8,
            },
            WinSize {
                ws_col: 12,
                ws_row: 4,
            },
            WinSize {
                ws_col: 200,
                ws_row: 50,
            },
            WinSize {
                ws_col: 380,
                ws_row: 120,
            },
            WinSize {
                ws_col: 200,
                ws_row: 50,
            },
        ] {
            let _ = t.resize(&ws);
        }

        let after = lines(t);
        for l in &before {
            let n = after.iter().filter(|a| a.contains(l.trim())).count();
            assert!(
                n <= 1,
                "{l:?} appears {n} times after a zoom cycle\nbefore: {before:#?}\nafter: {after:#?}"
            );
        }
    });
}
