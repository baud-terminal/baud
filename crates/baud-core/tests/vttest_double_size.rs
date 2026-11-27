//! vttest's double-size screens, replayed from its own bytes.
//!
//! `tools/tests/vttest/` holds what vttest sent through a real pty for its
//! "Test of double-sized characters" and three of its known-bug tests (the
//! README there has the commit and the keys). Each is fed through a terminal
//! that honours `?3`'s width request, as an embedder resizing on it does, and
//! the screen is read at every "Push <RETURN>" -- the points vttest asks a
//! person to look. What is checked is what `[[source: dec-std-070]]` says the
//! screen holds, which departs from vttest's own prose once: at Bug L.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

/// 24 rows at 80 columns, provisioned for vttest's 132.
const VT: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};
const ROOM: WinSize = WinSize {
    ws_row: 24,
    ws_col: 132,
};

/// One screen at a hold: each line's text, its rendition, and the cursor.
struct Seen {
    text: Vec<String>,
    size: Vec<LineRendition>,
    cols: Cols,
}

impl Seen {
    fn line(&self, n: usize) -> &str {
        &self.text[n - 1]
    }
    fn size(&self, n: usize) -> LineRendition {
        self.size[n - 1]
    }
}

fn recording(name: &str) -> Vec<u8> {
    let p = format!(
        "{}/../../tools/tests/vttest/{name}.raw",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn seen(t: &Term<'_>) -> Seen {
    let (cols, rows) = t.geometry();
    let text = (0..rows.index())
        .map(|y| {
            t.row(Row::from_index(y))
                .expect("a row")
                .iter()
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|c| char::from_u32(c.value()))
                        .unwrap_or(' ')
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    let size = (0..rows.index())
        .map(|y| t.line_rendition(Row::from_index(y)).expect("a row"))
        .collect();
    Seen { text, size, cols }
}

/// The recording fed in reads of `read` bytes, the width request honoured,
/// and the screen at each "Push <RETURN>", as vttest waits there: before
/// the pty's echo of the key that answered it, the `CR LF` after it.
fn replay(name: &str, read: usize) -> Vec<Seen> {
    let bytes = recording(name);
    let hold = b"Push <RETURN>";
    let mut stops: Vec<usize> = bytes
        .windows(hold.len())
        .enumerate()
        .filter(|(_, w)| *w == hold)
        .map(|(i, _)| i + hold.len())
        .collect();
    assert!(!stops.is_empty(), "{name}: no hold in the recording");
    stops.dedup();
    let mut s = Owned::new(&ROOM);
    let mut t = Term::new(s.storage(), &VT);
    t.set_width_hold(WidthHold::On);
    let mut out = Vec::new();
    let mut from = 0;
    for stop in stops {
        while from < stop {
            let n = read.min(stop - from);
            t.write_at(BytePos::ZERO, &bytes[from..from + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            from += n;
            while let Some(rest) = t.take_held() {
                let cols = t.requested_width().expect("a held read asked").ws_col();
                let ws = WinSize {
                    ws_row: VT.ws_row,
                    ws_col: cols,
                };
                assert_eq!(t.resize(&ws), Resized::Ok, "the room is provisioned");
                t.feed(rest.at, rest.len);
            }
        }
        out.push(seen(&t));
    }
    out
}

fn sp(n: usize) -> String {
    " ".repeat(n)
}

/// "Test of double-sized characters", at 80 columns and at 132: each line's
/// rendition is the last of the controls sent on it, the text written before
/// or after them stays where it was addressed, `EL 2` keeps the rendition,
/// and DECDWL then DECSWL of "This is not a double-width line" changes the
/// line's size and not its text.
#[test]
fn the_double_size_test_draws_each_rendition_at_both_widths() {
    let s = replay("doublesize", 4096);
    assert_eq!(s.len(), 6, "six holds");
    for (k, (cols, w1)) in [(80, 0), (132, 13)].into_iter().enumerate() {
        for i in 0..2 {
            let h = &s[2 * k + i];
            assert_eq!(h.cols.index(), cols, "hold {}", 2 * k + i + 1);
            assert_eq!(h.line(1), format!("{cols:3} column mode"));
            let wide = format!("{}This is a Double-width line", sp(1 + w1));
            let tall = format!("{}This is a Double-width-and-height line", sp(1 + w1));
            let another = format!("{}This is another such line", sp(1 + w1));
            assert_eq!(h.line(5), format!("{}v------- left margin", sp(2 + 2 * w1)));
            assert_eq!(
                h.line(7),
                format!("{}This is a normal-sized line", sp(2 + 2 * w1))
            );
            assert_eq!(h.size(7), LineRendition::Single, "DECSWL last");
            assert_eq!(
                (h.line(9), h.size(9)),
                (wide.as_str(), LineRendition::DoubleWidth)
            );
            assert_eq!(
                (h.line(11), h.size(11)),
                (tall.as_str(), LineRendition::DoubleTop)
            );
            assert_eq!(
                (h.line(12), h.size(12)),
                (tall.as_str(), LineRendition::DoubleBottom)
            );
            assert_eq!(
                (h.line(14), h.size(14)),
                (another.as_str(), LineRendition::DoubleTop),
                "EL 2 kept the rendition"
            );
            assert_eq!(
                (h.line(15), h.size(15)),
                (another.as_str(), LineRendition::DoubleBottom)
            );
            assert_eq!(
                h.line(17),
                format!("{}^------- left margin", sp(2 + 2 * w1))
            );
            let (text, size) = match i {
                0 => ("This is not a double-width line", LineRendition::Single),
                _ => (
                    "This **is** a double-width line",
                    LineRendition::DoubleWidth,
                ),
            };
            assert_eq!(
                (h.line(21), h.size(21)),
                (text, size),
                "hold {}",
                2 * k + i + 1
            );
            for n in [2, 3, 4, 6, 8, 10, 13, 16, 18, 19, 20, 22] {
                assert_eq!(h.size(n), LineRendition::Single, "line {n}");
            }
        }
    }
}

/// The frame of DEC line drawing on double-height lines: its corners and
/// edges on both halves of every pair, the tabs on a forty-column line
/// stopping on its last column where the right edge is drawn, and the
/// bold-blinking text over it. Then twelve RIs at the top of a region from
/// line 8: the box's first five lines come down to lines 20-24 with their
/// renditions, and the lines brought in are single-width.
#[test]
fn the_double_height_frame_and_the_half_that_remains() {
    let s = replay("doublesize", 4096);
    let h = &s[4];
    assert_eq!(h.cols.index(), 80);
    let top = format!("┌{}┐", "─".repeat(38));
    let bottom = format!("└{}┘", "─".repeat(38));
    let side = format!("│{}│", sp(38));
    let mad = "│ * The mad programmer strikes again * │";
    let want = [
        (8, top.as_str()),
        (9, top.as_str()),
        (10, side.as_str()),
        (11, side.as_str()),
        (12, mad),
        (13, mad),
        (14, side.as_str()),
        (15, side.as_str()),
        (16, bottom.as_str()),
        (17, bottom.as_str()),
    ];
    for (n, text) in want {
        let size = match n % 2 {
            0 => LineRendition::DoubleTop,
            _ => LineRendition::DoubleBottom,
        };
        assert_eq!((h.line(n), h.size(n)), (text, size), "line {n}");
    }
    let f = &s[5];
    assert_eq!(
        f.line(1),
        "Exactly half of the box should remain. Push <RETURN>"
    );
    for n in 8..=19 {
        assert_eq!(
            (f.line(n), f.size(n)),
            ("", LineRendition::Single),
            "line {n}"
        );
    }
    for (k, n) in (20..=24).enumerate() {
        let (orig, text) = want[k];
        let size = match orig % 2 {
            0 => LineRendition::DoubleTop,
            _ => LineRendition::DoubleBottom,
        };
        assert_eq!(
            (f.line(n), f.size(n)),
            (text, size),
            "line {n} is line {}",
            8 + k
        );
    }
}

/// Bug B: line 11 made double-width over DECALN's `E`s keeps the forty
/// inside its half; a region from line 12 takes an RI and sixteen lines
/// under it, none of which is sized, and line 11 above it does not move.
#[test]
fn bug_b_scrolls_a_region_under_a_double_width_line() {
    let s = replay("bug-b", 4096);
    let h = &s[0];
    assert_eq!(
        (h.line(11), h.size(11)),
        ("E".repeat(40).as_str(), LineRendition::DoubleWidth)
    );
    assert_eq!(h.line(12), "Here we go... Push <RETURN>");
    let h = &s[1];
    assert_eq!(
        (h.line(11), h.size(11)),
        ("E".repeat(40).as_str(), LineRendition::DoubleWidth)
    );
    for (c, n) in ('E'..='P').zip(12..=23) {
        assert!(
            h.line(n).starts_with(c),
            "line {n} begins {c}: {:?}",
            h.line(n)
        );
        assert_eq!(h.size(n), LineRendition::Single, "line {n}");
    }
    assert_eq!(h.line(24), "Push <RETURN>");
}

/// Bug E: from a double-width line at 132 columns, a CUP to line 3 column
/// 100 lands on column 100 of that single-width line.
#[test]
fn bug_e_moves_from_a_double_width_line_to_column_100() {
    let s = replay("bug-e", 4096);
    let h = &s[0];
    assert_eq!(h.cols.index(), 132);
    assert_eq!(
        (h.line(1), h.size(1)),
        (
            "This test should put an 'X' at line 3 column 100.",
            LineRendition::DoubleWidth
        )
    );
    assert_eq!(h.line(2), "1234567890".repeat(12));
    assert_eq!(h.line(3), format!("{}X", sp(99)));
    assert_eq!(h.line(4), format!("{}!{}!", sp(65), sp(33)));
    assert_eq!(h.line(5).find('+'), Some(65));
    assert_eq!(
        (h.line(10), h.size(10)),
        ("Push <RETURN>", LineRendition::DoubleWidth)
    );
}

/// Bug L: an 80-character line made double-width keeps its first forty,
/// and made single-width again the right half does not come back: "data
/// which is beyond the end of the Active Line ... [is] lost" (DECDWL note 2,
/// p. 5-97), as the VT100 User Guide and the VT510 manual say. vttest's
/// prose calls this a misfeature; the text is followed. At 132 columns, 66.
#[test]
fn bug_l_loses_the_right_half_as_the_texts_say() {
    let s = replay("bug-l", 4096);
    assert_eq!(s.len(), 6);
    let long80 = "This-is-a-long-line-".repeat(4);
    let long132 = format!("{}ending-here-", "This-is-a-long-line-".repeat(6));
    for (k, (cols, long)) in [(80, long80), (132, long132)].into_iter().enumerate() {
        let half = &long[..cols / 2];
        let at = |i: usize| &s[3 * k + i];
        assert_eq!(at(0).cols.index(), cols);
        assert_eq!(
            (at(0).line(15), at(0).size(15)),
            (long.as_str(), LineRendition::Single)
        );
        assert_eq!(
            (at(1).line(15), at(1).size(15)),
            (half, LineRendition::DoubleWidth)
        );
        assert_eq!(
            (at(2).line(15), at(2).size(15)),
            (half, LineRendition::Single),
            "lost"
        );
    }
}

/// Every hold reads the same at every read size: the recordings split
/// inside each control.
#[test]
fn the_replays_do_not_depend_on_where_the_reads_fell() {
    for name in ["doublesize", "bug-b", "bug-e", "bug-l"] {
        let whole: Vec<(Vec<String>, Vec<LineRendition>)> = replay(name, 4096)
            .into_iter()
            .map(|s| (s.text, s.size))
            .collect();
        for read in [1, 2, 3, 5, 7, 64] {
            let parts: Vec<(Vec<String>, Vec<LineRendition>)> = replay(name, read)
                .into_iter()
                .map(|s| (s.text, s.size))
                .collect();
            assert!(parts == whole, "{name} at reads of {read}");
        }
    }
}
