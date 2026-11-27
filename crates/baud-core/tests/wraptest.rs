//! A prompt padded to exactly the terminal's width must occupy one row.
//!
//! This is load-bearing for every shell and it was a suspect rather than
//! a defect: a user reported a resize leaving an orphaned prompt line
//! behind, and a shell redraws by counting the rows its prompt occupies,
//! moving the cursor up by that many and erasing from there. One row too
//! many and the erase starts below the prompt's first line, which then
//! survives -- exactly the orphan. So the arithmetic a shell depends on
//! is pinned here.
//!
//! It passes, and the orphan's cause turned out to be the reflow
//! re-wrapping the *old* prompt on a width change. The test stays because
//! the property is one a shell cannot work without, and nothing else
//! asserted it at the exact width.
mod support;
use baud_domain::WinSize;

#[test]
fn a_line_that_exactly_fills_the_row_does_not_advance_until_something_follows() {
    support::with_term(
        &WinSize {
            ws_col: 20,
            ws_row: 6,
        },
        |t| {
            // Exactly the width, then CR LF -- which is what a prompt padded
            // to the terminal's width emits. It must occupy one row, not two:
            // a shell counts its prompt's rows and then moves the cursor back
            // up by that many, so one row too many leaves an orphan behind.
            support::feed(t, &b"X".repeat(20));
            let (r, c) = t.cursor();
            eprintln!(
                "after exactly 20 columns: cursor row {} col {}",
                r.index(),
                c.index()
            );
            assert_eq!(
                r.index(),
                0,
                "the cursor left the row before anything followed"
            );
            support::feed(t, b"\r\n");
            let (r2, _) = t.cursor();
            eprintln!("after CR LF: cursor row {}", r2.index());
            assert_eq!(r2.index(), 1, "a full row plus CR LF is one row, not two");
        },
    );
}
