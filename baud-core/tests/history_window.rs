//! **The terminal keeps the window over the history** (note V, V2, the
//! owner's answer of 2026-09-25: "B -- the core keeps the window over the
//! history, as a line number or live"). Before it, baud-app kept the window as
//! a count of lines up from the live screen and moved it after every read by
//! the read's net movement, which is not composable: a window a scroll down
//! brought to the live screen stays there, so `SD 5` and five lines landed
//! live when the two were reads of their own and five lines up when one read
//! held both. The core holds an absolute line, which output retiring lines
//! never moves, and sends it live at the byte that takes its line back.
//!
//! The oracle is the app's anchor as it was, ported here and fed a byte a
//! read, where a read's net movement is one event's: the answer every other
//! split of the same bytes must give.

mod support;

use baud_core::*;
use proptest::prelude::*;

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 10,
};

/// The grid every test here provisions: room for the resizes the property
/// takes. A depth of thirty is thirty lines past this height, so 44 behind a
/// ten-row screen, and the ring recycles a window's line within a few dozen.
const ROOM: WinSize = WinSize {
    ws_col: 40,
    ws_row: 24,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// `bytes` in reads of `size`.
fn feed_in(t: &mut Term<'_>, bytes: &[u8], size: usize) {
    for read in bytes.chunks(size) {
        feed(t, read);
    }
}

fn with_depth<R>(depth: usize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::with_history(&ROOM, &CacheGeometry::UNKNOWN, Depth::of(depth));
    let mut t = Term::new(owned.storage(), &WS);
    f(&mut t)
}

fn lines(n: usize) -> Vec<u8> {
    (0..n)
        .flat_map(|i| format!("line {i}\r\n").into_bytes())
        .collect()
}

/// Forty lines on a ten-row screen, thirty-one behind it, and the cursor on
/// the last row, where a newline retires one more.
fn with_history<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    with_depth(100, |t| {
        feed(t, &lines(40));
        assert_eq!(t.scrollback().len(), 31, "the control: a history");
        f(t)
    })
}

fn text_of(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .filter_map(|c| c.content.as_codepoint())
        .filter_map(|cp| char::from_u32(cp.value()))
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// The first row of the window the terminal draws.
fn shown(t: &Term<'_>) -> String {
    text_of(t.window().row(Row::ZERO))
}

/// The window five lines up, where each take-back row starts.
fn five_up() -> Rows {
    Rows::of_count(5)
}

/// A prompt block the shell opened on the last row and printed sixteen
/// lines into, so its first row is seven lines up the history; and the redraw
/// from row 3 that takes those seven back, its rows below cleared.
const BLOCK_OPEN: &[u8] = b"\x1b]133;A\x07";
const REDRAW: &[u8] = b"\x1b[3;1H\x1b[J\x1b]133;A\x07$ \x1b[10;1H";

fn prompt_block() -> Vec<u8> {
    [BLOCK_OPEN, &b"p\r\n".repeat(16)].concat()
}

/// The window after `bytes` from `back` lines up, after `setup`, with the
/// bytes in the reads `cut` makes of them.
fn window_after(
    back: Rows,
    setup: &[u8],
    bytes: &[u8],
    cut: impl FnOnce(&[u8]) -> Vec<&[u8]>,
) -> Rows {
    with_history(|t| {
        feed(t, setup);
        t.set_window_back(back);
        assert_eq!(t.window_back(), back, "the control: set");
        for read in cut(bytes) {
            feed(t, read);
        }
        t.primary_window_back()
    })
}

/// **A take-back that reaches the window sends it live at that byte**, at
/// every read size and every cut into two reads: a full-screen `SD`, `RI` at
/// the top and a prompt redraw each take lines back from the history, and the
/// five lines after them retire five again. The rows a window keeps are the
/// controls: five lines alone, and `SD 3`, which leaves it two up for the
/// lines to carry back. Each expected answer is the one a byte a read gives,
/// and the byte-a-read answer is the oracle's.
#[test]
fn a_take_back_that_reaches_the_window_sends_it_live_at_that_byte() {
    let five = b"\n".repeat(5);
    let block = prompt_block();
    let ri = b"\x1b[H\x1bM\x1bM\x1bM\x1bM\x1bM\x1b[10;1H";
    let rows: [(&str, &[u8], Vec<u8>, usize); 6] = [
        ("five lines", b"", five.clone(), 10),
        (
            "SD 3, five lines",
            b"",
            [&b"\x1b[3T"[..], &five].concat(),
            7,
        ),
        (
            "SD 5, five lines",
            b"",
            [&b"\x1b[5T"[..], &five].concat(),
            0,
        ),
        (
            "SD 9, five lines",
            b"",
            [&b"\x1b[9T"[..], &five].concat(),
            0,
        ),
        (
            "five RI at the top, five lines",
            b"",
            [&ri[..], &five].concat(),
            0,
        ),
        (
            "a prompt redraw, five lines",
            &block,
            [REDRAW, &five].concat(),
            0,
        ),
    ];
    for (what, setup, bytes, want) in &rows {
        let canon = window_after(five_up(), setup, bytes, |b| b.chunks(1).collect());
        assert_eq!(canon.index(), *want, "{what}, a byte a read");
        assert_eq!(
            canon,
            oracle_after(5, setup, bytes),
            "{what}: the oracle, a byte a read"
        );
        for size in 2..=bytes.len() + 1 {
            assert_eq!(
                window_after(five_up(), setup, bytes, |b| b.chunks(size).collect()),
                canon,
                "{what}, reads of {size}"
            );
        }
        for at in 1..bytes.len() {
            assert_eq!(
                window_after(five_up(), setup, bytes, |b| {
                    let (x, y) = b.split_at(at);
                    vec![x, y]
                }),
                canon,
                "{what}, cut at {at}"
            );
        }
    }
}

/// The app's anchor, as `baud-app` kept it until the core held the window:
/// a live window stays live; one looking back moves up by the rows the
/// primary retired and down by those it took back, bounded by what is
/// retained.
fn anchored(back: Rows, moved: Moved, retained: Rows) -> Rows {
    if back.index() == 0 {
        return back;
    }
    let at = match moved {
        Moved::Retired(n) => back.index().saturating_add(n.index()),
        Moved::Restored(n) => back.index().saturating_sub(n.index()),
    };
    Rows::of_count(at.min(retained.index()))
}

/// The app's window after one read, as `after_read` gave it: live after an
/// erasure, else anchored by the primary's movement.
fn after_read(t: &Term<'_>, back: Rows, mark: HistoryMark, erased: HistoryErasures) -> Rows {
    if t.history_erasures() != erased {
        return Rows::ZERO;
    }
    let retained = Rows::of_count(t.scrollback().len());
    anchored(back, t.history_moved_since(mark), retained)
}

/// `bytes` a byte a read through the oracle, from `back`: where its window is.
fn oracle_feed(t: &mut Term<'_>, mut back: Rows, bytes: &[u8]) -> Rows {
    for b in bytes {
        let (mark, erased) = (t.history_mark(), t.history_erasures());
        feed(t, core::slice::from_ref(b));
        back = after_read(t, back, mark, erased);
    }
    back
}

fn oracle_after(back: usize, setup: &[u8], bytes: &[u8]) -> Rows {
    with_history(|t| {
        feed(t, setup);
        oracle_feed(t, Rows::of_count(back), bytes)
    })
}

/// **The window holds its line as lines retire**, on the primary, at every
/// read size: the line it shows is the line it showed.
#[test]
fn the_window_holds_its_line_as_lines_retire() {
    for size in [1, 2, 3, 7, 4096] {
        with_history(|t| {
            t.set_window_back(Rows::of_count(5));
            let line = shown(t);
            assert_eq!(line, "line 26", "the control: five lines up");
            feed_in(t, &lines(12), size);
            assert_eq!(t.window_back().index(), 17, "reads of {size}");
            assert_eq!(shown(t), line, "reads of {size}");
        });
    }
}

/// **A window whose line the ring recycles reads as the oldest line kept**,
/// as an anchor clamped at every retirement would, and a scroll down that
/// then takes the whole history back sends it live -- one older than the
/// oldest kept, left unclamped, would read as the line it named and come
/// back off the live screen as lines retired again.
#[test]
fn the_window_reads_the_oldest_line_kept_once_the_ring_recycles_its_line() {
    for size in [1, 2, 3, 4096] {
        with_depth(30, |t| {
            feed(t, &lines(80));
            let full = t.scrollback().len();
            assert_eq!(full, 44, "the control: the ring is full");
            t.set_window_back(Rows::of_count(full - 2));
            feed_in(t, &lines(5), size);
            assert_eq!(
                t.window_back().index(),
                full,
                "reads of {size}: the oldest kept"
            );
            assert_eq!(
                shown(t),
                text_of(t.view(Rows::of_count(full)).row(Row::ZERO)),
                "reads of {size}"
            );
            // Five screens down takes all of it back.
            feed_in(t, &b"\x1b[10T".repeat(5), size);
            assert_eq!(t.scrollback().len(), 0, "the control: none left");
            assert_eq!(t.window_back(), Rows::ZERO, "reads of {size}: live");
            feed_in(t, b"\x1b[10;1H", size);
            feed_in(t, &lines(8), size);
            assert_eq!(
                t.window_back(),
                Rows::ZERO,
                "reads of {size}: and live stays live"
            );
        });
    }
}

/// **An erasure sends the window live, from either screen**: `CSI 3 J` and
/// `RIS` take the lines it showed. `CSI ? 3 J` erases characters and keeps
/// the lines, and so keeps the window -- the control.
#[test]
fn an_erasure_sends_the_window_live_from_either_screen() {
    for (setup, bytes, want, what) in [
        (&b""[..], &b"\x1b[3J"[..], 0, "ED 3"),
        (
            b"\x1b[?1049h",
            b"\x1b[3J",
            0,
            "ED 3 on the alternate screen",
        ),
        (b"", b"\x1bc", 0, "RIS"),
        (b"\x1b[?1049h", b"\x1bc", 0, "RIS on the alternate screen"),
        (b"", b"\x1b[3J\x1bc", 0, "ED 3, RIS"),
        (b"", b"\x1b[?3J", 5, "DECSED 3, the control"),
        (b"", b"\x1b[2J", 5, "ED 2, the control"),
    ] {
        for size in [1, 2, 4096] {
            with_history(|t| {
                feed(t, setup);
                t.set_window_back(Rows::of_count(5));
                feed_in(t, bytes, size);
                assert_eq!(
                    t.primary_window_back().index(),
                    want,
                    "{what}, reads of {size}"
                );
                feed_in(t, &lines(3), size);
                let want = if want == 0 { 0 } else { want + 3 };
                if t.screen() == Screen::Normal {
                    assert_eq!(t.window_back().index(), want, "{what}, and lines after");
                }
            });
        }
    }
}

/// **The alternate screen neither moves nor shows the primary's window**:
/// a program's output retires nothing into the primary's history, the
/// window drawn there is the live screen, and leaving finds the primary's
/// window where it was.
#[test]
fn the_alternate_screen_neither_moves_nor_shows_the_primarys_window() {
    with_history(|t| {
        t.set_window_back(Rows::of_count(3));
        assert_eq!(t.window_back().index(), 3, "the control: on the primary");
        feed(t, b"\x1b[?1049h\x1b[10;1H");
        assert_eq!(t.window_back(), Rows::ZERO, "drawn: the live screen");
        assert_eq!(t.primary_window_back().index(), 3);
        feed(t, &lines(20));
        assert_eq!(
            t.primary_window_back().index(),
            3,
            "its output moved nothing"
        );
        feed(t, b"\x1b[5T");
        assert_eq!(t.primary_window_back().index(), 3, "nor its scroll down");
        feed(t, b"\x1b[?1049l");
        assert_eq!(t.window_back().index(), 3, "and leaving finds it");
        feed(t, &lines(2));
        assert_eq!(
            t.window_back().index(),
            5,
            "the control: the primary carries it"
        );
    });
}

/// **The window is set within the history**: zero, or no history, is the
/// live screen, and a count past the history is its oldest line.
#[test]
fn the_window_is_set_within_the_history() {
    support::with_term(&WS, |t| {
        t.set_window_back(Rows::of_count(4));
        assert_eq!(t.window_back(), Rows::ZERO, "no history, the live screen");
        feed(t, &lines(15));
        assert_eq!(t.scrollback().len(), 6, "the control: six behind it");
        t.set_window_back(Rows::of_count(40));
        assert_eq!(t.window_back().index(), 6, "bounded by the history");
        t.set_window_back(Rows::ZERO);
        assert_eq!(t.window_back(), Rows::ZERO);
        t.set_window_back(Rows::of_count(2));
        t.window_live();
        assert_eq!(t.window_back(), Rows::ZERO, "sent live");
        feed(t, &lines(3));
        assert_eq!(t.window_back(), Rows::ZERO, "and live stays live");
    });
}

/// A reset row: the window's lines up, the setup, the bytes, the first
/// erasure they leave, and what the row is.
type ResetRow<'a> = (usize, &'a [u8], &'a [u8], Option<Erasure>, &'a str);

/// **A reset keeps where the window stood at its byte**, for an embedder's
/// reveal: back when nothing before it in the read moved the window, live
/// when a scroll down, `RI` at the top or a prompt redraw earlier in the same
/// read took the window's line back first, or when the window was live -- at
/// every read size and every cut into two reads. The case the reveal used to
/// read at the read's edges, where the same bytes revealed or not by where
/// the reads fell (TODO.md, the residue of 385ee1b).
#[test]
fn a_reset_keeps_where_the_window_stood_at_its_byte() {
    let back = |s| Some(Erasure::Reset(s, Looking::Back));
    let live = |s| Some(Erasure::Reset(s, Looking::Live));
    let block = prompt_block();
    let redraw_reset = [&b"\x1b[3;1H\x1b[J\x1b]133;A\x07"[..], b"\x1bc"].concat();
    let rows: [ResetRow<'_>; 8] = [
        (
            5,
            b"",
            b"\x1bc",
            back(Screen::Normal),
            "a reset from five up",
        ),
        (
            0,
            b"",
            b"\x1bc",
            live(Screen::Normal),
            "a reset from the live screen",
        ),
        (
            5,
            b"",
            b"\x1b[3T\x1bc",
            back(Screen::Normal),
            "SD 3, a reset",
        ),
        (
            5,
            b"",
            b"\x1b[5T\x1bc",
            live(Screen::Normal),
            "SD 5, a reset",
        ),
        (
            5,
            b"",
            b"\x1b[H\x1bM\x1bM\x1bM\x1bM\x1bM\x1bc",
            live(Screen::Normal),
            "five RI at the top, a reset",
        ),
        (
            5,
            &block,
            &redraw_reset,
            live(Screen::Normal),
            "a prompt redraw, a reset",
        ),
        (
            5,
            b"",
            b"\x1b[?1049h\x1bc",
            back(Screen::Alternate),
            "?1049h, a reset: the primary's window was back",
        ),
        (
            5,
            b"",
            b"\x1b[3J\x1bc",
            Some(Erasure::SavedLines),
            "ED 3, a reset",
        ),
    ];
    let first = |from: usize, setup: &[u8], reads: &[&[u8]]| {
        with_history(|t| {
            feed(t, setup);
            t.set_window_back(Rows::of_count(from));
            assert_eq!(t.window_back().index(), from, "the control: set");
            t.watch_erasures();
            let since = t.history_erasures();
            for read in reads {
                feed(t, read);
            }
            assert_eq!(t.window_back(), Rows::ZERO, "live after it");
            t.first_erasure(since)
        })
    };
    for (from, setup, bytes, want, what) in rows {
        for size in 1..=bytes.len() {
            let reads: Vec<&[u8]> = bytes.chunks(size).collect();
            assert_eq!(first(from, setup, &reads), want, "{what}, reads of {size}");
        }
        for at in 1..bytes.len() {
            let (x, y) = bytes.split_at(at);
            assert_eq!(first(from, setup, &[x, y]), want, "{what}, cut at {at}");
        }
    }
}

/// What a program step does: bytes, a wheel, a key's snap or a resize.
#[derive(Clone, Debug)]
enum Op {
    Bytes(Vec<u8>),
    /// The wheel or the thumb: the window set this many lines up or down
    /// from where it is, bounded by the history.
    Wheel(i64),
    /// A key on the primary: the window to the live screen.
    Snap,
    Resize(u16, u16),
}

fn bytes_op() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        (1usize..40).prop_map(|n| [&b"\x1b[10;1H"[..], &b"\n".repeat(n)].concat()),
        (1usize..30).prop_map(lines),
        (0u16..14).prop_map(|n| format!("\x1b[{n}T").into_bytes()),
        (0u16..14).prop_map(|n| format!("\x1b[{n}S").into_bytes()),
        (1usize..12).prop_map(|n| [&b"\x1b[H"[..], &b"\x1bM".repeat(n)].concat()),
        Just(b"\x1b[3;7r\x1b[4H\x1b[2L\x1b[r".to_vec()),
        Just(b"\x1b[3;7r\x1b[4H\x1b[2M\x1b[r".to_vec()),
        Just(b"\x1b[1;5r\x1b[5;1H\n\n\n\x1b[r".to_vec()),
        Just(b"\x1b[3;8r\x1b[3;1H\x1bM\x1bM\x1b[r".to_vec()),
        Just(BLOCK_OPEN.to_vec()),
        Just(REDRAW.to_vec()),
        Just(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07".to_vec()),
        Just(b"\x1b[?1049h".to_vec()),
        Just(b"\x1b[?1049l".to_vec()),
        Just(b"\x1b[?47h".to_vec()),
        Just(b"\x1b[?47l".to_vec()),
        Just(b"\x1bc".to_vec()),
        Just(b"\x1b[3J".to_vec()),
        Just(b"\x1b[?3J".to_vec()),
        Just(b"\x1b[2J".to_vec()),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => bytes_op().prop_map(Op::Bytes),
        2 => (-12i64..12).prop_map(Op::Wheel),
        1 => Just(Op::Snap),
        1 => prop_oneof![
            Just((20u16, 10u16)),
            Just((12, 10)),
            Just((40, 8)),
            Just((20, 24)),
            Just((30, 6)),
        ]
        .prop_map(|(c, r)| Op::Resize(c, r)),
    ]
}

/// The window a wheel step of `step` from `now` asks for, bounded by
/// `retained` -- the app's wheel arithmetic.
fn stepped(now: Rows, step: i64, retained: usize) -> Rows {
    let now = i64::try_from(now.index()).unwrap_or(0);
    let r = i64::try_from(retained).unwrap_or(i64::MAX);
    Rows::of_count(usize::try_from((now + step).clamp(0, r)).unwrap_or(0))
}

proptest! {
    /// **The core's window is the oracle's at a byte a read, and the same
    /// at every other read size.** The oracle is the app's anchor fed a
    /// byte a read, where a read's net movement is one event's; fed in
    /// longer reads it gives the net movement's answer, which is the
    /// batching dependence the core removes, so the other sizes are held to
    /// the core's own byte-a-read answer. Programs mix lines, full-screen
    /// and region scrolls both ways, `RI` at the top, `IL` and `DL` in a
    /// region, prompt marks and redraws, both alternate screens, `RIS`,
    /// `ED 3`, `DECSED 3`, the wheel, a key's snap, and resizes of width
    /// and height in the output zone and the prompt zone, over a history
    /// of 44 lines behind ten rows, so the ring recycles a window's line.
    /// A resize is
    /// the core's on both sides: at one the oracle's window is the core's
    /// own after it, and the property holds every read size to that.
    #[test]
    fn the_window_is_the_byte_a_read_anchor_at_every_read_size(
        prog in prop::collection::vec(op(), 1..40),
        size in prop_oneof![Just(2usize), Just(3), Just(7), Just(64), Just(4096)],
    ) {
        with_depth(30, |o| {
            with_depth(30, |one| {
                with_depth(30, |t| {
                    let mut back = Rows::ZERO;
                    for (i, op) in prog.iter().enumerate() {
                        match op {
                            Op::Bytes(b) => {
                                back = oracle_feed(o, back, b);
                                feed_in(one, b, 1);
                                feed_in(t, b, size);
                            }
                            Op::Wheel(step) => {
                                back = stepped(back, *step, o.scrollback().len());
                                o.set_window_back(back);
                                one.set_window_back(back);
                                t.set_window_back(back);
                            }
                            Op::Snap => {
                                if o.screen() == Screen::Normal {
                                    back = Rows::ZERO;
                                    one.window_live();
                                    t.window_live();
                                }
                            }
                            Op::Resize(c, r) => {
                                let ws = WinSize { ws_col: *c, ws_row: *r };
                                o.set_window_back(back);
                                let _ = o.resize(&ws);
                                back = o.primary_window_back();
                                let _ = one.resize(&ws);
                                let _ = t.resize(&ws);
                            }
                        }
                        prop_assert_eq!(
                            one.primary_window_back(), back,
                            "step {}, {:?}: a byte a read against the oracle", i, op
                        );
                        prop_assert_eq!(
                            t.primary_window_back(), one.primary_window_back(),
                            "step {}, {:?}: reads of {} against a byte a read", i, op, size
                        );
                        prop_assert_eq!(t.window_back(), t.looking_back(t.primary_window_back()));
                    }
                    Ok(())
                })
            })
        })?;
    }
}
