//! **A `U+FE0F` makes its base two cells wide** (owner, 2026-10-01).
//!
//! `DECRQM ?2027` answered 3 -- permanently set -- while a text-default emoji
//! and its `U+FE0F` stayed one cell, which is a wrong answer to a query. The
//! owner chose two cells, the width tmux 3.6+, fish, macOS bash, helix and
//! lipgloss assume: the base widens when `[[source:
//! emoji-variation-sequences]]` lists it with an emoji-style sequence, keycap
//! bases excepted, in place when the row has room and moved whole to the next
//! row at the right margin, as ghostty does. A configuration keeps one cell,
//! and then `?2027` answers 4.
//!
//! Every test feeds at read sizes 1, 2, 3 and whole, because the widen happens
//! on the codepoint *after* the base, which a read boundary can separate.
mod support;
use baud_core::{Owned, Term};
use baud_domain::{Col, Row, Vs16Width, WinSize};

const WS: WinSize = WinSize {
    ws_row: 3,
    ws_col: 8,
};

const DECRQM_2027: &[u8] = b"\x1b[?2027$p";

/// Feed `bytes` at every read size and hand each terminal to `f`.
fn each_read_size(w: Vs16Width, bytes: &[u8], f: impl Fn(&mut Term<'_>)) {
    for chunk in [1, 2, 3, bytes.len().max(1)] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_vs16_width(w);
        for piece in bytes.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        f(&mut t);
    }
}

/// Row `y` as one token a cell: the character, `>` for a wide tail, `.` for
/// a cell nothing was written to.
fn row(t: &Term<'_>, y: usize) -> String {
    t.row(Row::from_index(y))
        .expect("a row on the grid")
        .iter()
        .map(|c| {
            if c.content.is_wide_tail() {
                '>'
            } else {
                t.base_of(*c)
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.')
            }
        })
        .collect()
}

fn cursor(t: &Term<'_>) -> (usize, usize) {
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

fn ask(t: &mut Term<'_>, q: &[u8]) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, q);
    t.reply().to_vec()
}

#[test]
fn a_listed_base_takes_two_cells_and_keeps_its_selector() {
    each_read_size(Vs16Width::Two, "\u{2764}\u{fe0f}x".as_bytes(), |t| {
        assert_eq!(row(t, 0), "\u{2764}>x.....");
        assert_eq!(cursor(t), (0, 3));
        // The selector is still the cell's, for the renderer's emoji face.
        let marks: Vec<u32> = t.marks(Row::from_index(0), Col::at_index(0)).collect();
        assert_eq!(marks, [0xfe0f]);
    });
}

#[test]
fn an_unlisted_base_a_keycap_base_and_a_wide_base_do_not_widen() {
    // `A` has no emoji-style sequence; `1` is a keycap base, deferred; `⌚`
    // is wide already, and its selector changes nothing.
    each_read_size(
        Vs16Width::Two,
        "A\u{fe0f}1\u{fe0f}\u{231a}\u{fe0f}x".as_bytes(),
        |t| {
            assert_eq!(row(t, 0), "A1\u{231a}>x...");
            assert_eq!(cursor(t), (0, 5));
        },
    );
}

#[test]
fn a_mark_between_the_base_and_the_selector_widens_nothing() {
    each_read_size(Vs16Width::Two, "\u{2764}\u{301}\u{fe0f}x".as_bytes(), |t| {
        assert_eq!(row(t, 0), "\u{2764}x......");
    });
}

#[test]
fn a_selector_inside_a_zwj_family_widens_nothing() {
    // Pirate flag: black flag, ZWJ, skull and crossbones, VS16. `☠` is a
    // listed base, but the cluster is the flag's, which is wide already.
    each_read_size(
        Vs16Width::Two,
        "\u{1f3f4}\u{200d}\u{2620}\u{fe0f}x".as_bytes(),
        |t| {
            assert_eq!(row(t, 0), "\u{1f3f4}>x.....");
            assert_eq!(cursor(t), (0, 3));
        },
    );
}

#[test]
fn a_base_widened_into_the_last_column_leaves_the_wrap_pending() {
    each_read_size(Vs16Width::Two, "abcdef\u{2764}\u{fe0f}x".as_bytes(), |t| {
        assert_eq!(row(t, 0), "abcdef\u{2764}>");
        assert_eq!(row(t, 1), "x.......");
        assert_eq!(cursor(t), (1, 1));
    });
}

#[test]
fn a_base_in_the_last_column_moves_whole_to_the_next_row() {
    each_read_size(Vs16Width::Two, "abcdefg\u{2764}\u{fe0f}x".as_bytes(), |t| {
        assert_eq!(row(t, 0), "abcdefg.");
        assert_eq!(row(t, 1), "\u{2764}>x.....");
        assert_eq!(cursor(t), (1, 3));
        let marks: Vec<u32> = t.marks(Row::from_index(1), Col::at_index(0)).collect();
        assert_eq!(marks, [0xfe0f], "the selector moves with its base");
        assert!(
            t.marks(Row::from_index(0), Col::at_index(7))
                .next()
                .is_none(),
            "the cell left behind keeps nothing"
        );
    });
}

/// **With `DECAWM` off nothing moves to the next row**, and the selector
/// joins its base. The cursor pins *on* the last column, so the character
/// just written is under it rather than left of it; the attach read the cell
/// left of the cursor and put the selector on the `g`, which this test pinned
/// as an older defect until `Paint::attach_continuing` (`tests/margin_pin.rs`).
/// The base keeps one cell: there is no room to widen into and no wrap to
/// move it by.
#[test]
fn without_autowrap_nothing_moves_and_the_selector_joins_its_base() {
    each_read_size(
        Vs16Width::Two,
        "\x1b[?7labcdefg\u{2764}\u{fe0f}".as_bytes(),
        |t| {
            assert_eq!(row(t, 0), "abcdefg\u{2764}");
            assert_eq!(row(t, 1), "........");
            let on_base: Vec<u32> = t.marks(Row::from_index(0), Col::at_index(7)).collect();
            assert_eq!(on_base, [0xfe0f], "the selector is the base's");
            assert!(
                t.marks(Row::from_index(0), Col::at_index(6))
                    .next()
                    .is_none(),
                "the cell before keeps nothing"
            );
        },
    );
}

/// A cursor moved onto the tail of a wide listed base has that base directly
/// left of it; its selector widens nothing and moves nothing.
#[test]
fn a_selector_with_the_cursor_on_a_wide_tail_moves_nothing() {
    each_read_size(
        Vs16Width::Two,
        "\u{231a}\x1b[1;2H\u{fe0f}".as_bytes(),
        |t| {
            assert_eq!(row(t, 0), "\u{231a}>......");
            assert_eq!(cursor(t), (0, 1));
        },
    );
}

#[test]
fn widening_overwrites_the_cell_after_and_repairs_a_wide_neighbour() {
    // `❤` then a move back over the wide `水` written after it: the selector
    // arrives with the cursor just past `❤`, so the tail lands on `水`'s lead
    // and the orphaned `水` tail is blanked.
    each_read_size(
        Vs16Width::Two,
        "\u{2764}\u{6c34}\x1b[1;2H\u{fe0f}".as_bytes(),
        |t| {
            assert_eq!(row(t, 0), "\u{2764}>......");
        },
    );
}

#[test]
fn one_cell_is_a_configuration_and_decrqm_reports_it() {
    each_read_size(Vs16Width::One, "\u{2764}\u{fe0f}x".as_bytes(), |t| {
        assert_eq!(t.vs16_width(), Vs16Width::One);
        assert_eq!(row(t, 0), "\u{2764}x......");
        assert_eq!(ask(t, DECRQM_2027), b"\x1b[?2027;4$y");
    });
    each_read_size(Vs16Width::Two, b"", |t| {
        assert_eq!(ask(t, DECRQM_2027), b"\x1b[?2027;3$y");
    });
}

#[test]
fn no_sequence_changes_the_width() {
    // `?2027` is permanent either way: an application's reset or set, and
    // both resets, leave the configured width.
    for w in [Vs16Width::Two, Vs16Width::One] {
        each_read_size(
            w,
            "\x1b[?2027l\x1b[?2027h\x1b[!p\x1bc\u{2764}\u{fe0f}".as_bytes(),
            |t| {
                let want = match w {
                    Vs16Width::Two => "\u{2764}>......",
                    Vs16Width::One => "\u{2764}.......",
                };
                assert_eq!(row(t, 0), want);
            },
        );
    }
}

/// **The recorded level.** `cat-cjk.raw` ends with seven rainbow flags on
/// screen, each `U+1F3F3 U+FE0F U+200D U+1F308`, and each white flag now
/// takes two cells with its selector on the lead -- at a one-byte read, where
/// every sequence is cut, and at the harness's 4096.
#[test]
fn the_recordings_rainbow_flags_take_two_cells_at_every_read_size() {
    let bytes = std::fs::read(format!(
        "{}/../tests/recordings/utf8/cat-cjk.raw",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    for chunk in [1usize, 4096] {
        support::with_corpus(|t| {
            for piece in bytes.chunks(chunk) {
                support::feed(t, piece);
            }
            let mut widened = Vec::new();
            for y in 0..50 {
                let cells = t
                    .row(Row::from_index(y))
                    .expect("a row on the grid")
                    .to_vec();
                for x in 0..cells.len() - 1 {
                    let Some(cp) = t.base_of(cells[x]).as_codepoint() else {
                        continue;
                    };
                    let first = t.marks(Row::from_index(y), Col::at_index(x)).next();
                    if baud_domain::vs16::widens(cp)
                        && first == Some(0xfe0f)
                        && cells[x + 1].content.is_wide_tail()
                    {
                        widened.push(cp.value());
                    }
                }
            }
            assert_eq!(widened, [0x1f3f3; 7], "at {chunk}-byte reads");
        });
    }
}

/// **The hand model: tmux's full redraw.** tmux 3.6+ counts a VS16 cluster as
/// two cells and redraws a line by writing it sequentially, with no cursor
/// move after the emoji (measured in the session's research). One cell here
/// put every character after it one column left of where tmux believes it
/// is; two cells puts `X` where tmux writes it next.
#[test]
fn a_tmux_style_redraw_lands_where_tmux_counts() {
    each_read_size(
        Vs16Width::Two,
        "\x1b[2;1Ha\u{2764}\u{fe0f}XY\x1b[2;5Hz".as_bytes(),
        |t| {
            // tmux's columns: a 0, the heart 1-2, X 3, Y 4; its CUP to
            // column 5 overwrites Y and lands where it counts.
            assert_eq!(row(t, 1), "a\u{2764}>Xz...");
        },
    );
}

/// **A base in the bottom-right cell scrolls the screen as it moves**, and in
/// a scrolling region it scrolls only the region.
#[test]
fn a_base_moved_off_the_last_row_scrolls() {
    each_read_size(
        Vs16Width::Two,
        "top\r\nmid\r\nabcdefg\u{2764}\u{fe0f}x".as_bytes(),
        |t| {
            assert_eq!(row(t, 0), "mid.....");
            assert_eq!(row(t, 1), "abcdefg.");
            assert_eq!(row(t, 2), "\u{2764}>x.....");
            assert_eq!(cursor(t), (2, 3));
        },
    );
    // The region is rows 1-2; row 3 is a status line that must not move.
    each_read_size(
        Vs16Width::Two,
        "\x1b[3;1Hstatus\x1b[1;2r\x1b[1;1Htop\r\nabcdefg\u{2764}\u{fe0f}x".as_bytes(),
        |t| {
            assert_eq!(row(t, 0), "abcdefg.");
            assert_eq!(row(t, 1), "\u{2764}>x.....");
            assert_eq!(row(t, 2), "status..");
            assert_eq!(cursor(t), (1, 3));
        },
    );
}
