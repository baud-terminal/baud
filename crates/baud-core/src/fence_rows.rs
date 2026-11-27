//! **The write fence after every operation, against the margins**: a
//! property over programs that put the cursor above, inside and below a
//! scroll area with side margins narrower than the page, and the
//! operations that move it, scroll or shift the area, erase or fill rows,
//! set the margins and switch screens.
//!
//! `Cold::fence` is what the per-character fit test compares against, and
//! on a single-width line it must be the line's write fence:
//! `[[source: dec-std-070]]`'s INSERT OR REPLACE GRAPHIC CHARACTER (p.
//! 5-139), "If the Active Position is within the Scroll Area, it will
//! advance to the Right Margin. If the Active Position is outside the
//! Scroll Area, it will advance to the last column of the Active Line".
//! Too high, a write inside the area runs past the right margin; too low,
//! one outside it wraps at the margin. Neither can be seen from the fence
//! alone, and `feed.rs`'s end-of-read assert sees only the first, so this
//! computes the fence from the margins and the cursor's row, by that
//! sentence and not by `paint::row_fence`, and compares after each
//! operation, each its own read.
//!
//! No operation here sizes a line or sets SLL's limit: a sized line's fence
//! may lag the line the cursor left (`Paint::enter_row`), by design, and
//! SLL lowers it. With DECLRMM set no line is sized (p. 5-29), so every
//! line is single-width there. The status line is not selected.

extern crate std;

use std::string::String;
use std::vec::Vec;

use baud_domain::{Col, ModeBit};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::{Config, TestRunner};

use crate::{BytePos, Owned, ReadLen, Resized, Term, WinSize};

/// The screen a program starts at, and the room it may resize into.
const START: WinSize = WinSize {
    ws_row: 12,
    ws_col: 20,
};
const ROOM: WinSize = WinSize {
    ws_row: 14,
    ws_col: 24,
};

/// The operations, by index; [`OPS`] is how many.
const OPS: u8 = 50;

/// One operation: its kind and four small numbers it reads as it needs.
type Op = (u8, u16, u16, u16, u16);

/// A program: the margins first, `?69h`, DECSTBM `t;b` and DECSLRM `l;r`
/// with rows above and below the area and the right margin left of the
/// last column, then the operations.
type Program = (Margins, Vec<Op>);

/// DECSTBM's `t;b` and DECSLRM's `l;r`, 1-based.
type Margins = (u16, u16, u16, u16);

/// What an operation does to the terminal: bytes fed as one read, or a
/// resize.
enum Step {
    Bytes(String),
    Resize(WinSize),
}

/// The step operation `op` is, under the program's margins `m`.
fn step(op: Op, m: Margins) -> Step {
    let (k, a, b, c, d) = op;
    let above = m.0 - 1;
    let bytes = match k {
        0 => std::format!("\x1b[{a};{b}H"),
        1 => std::format!("\x1b[{a}d"),
        2 => String::from("\n"),
        3 => String::from("\x1bM"),
        4 => String::from("\x1bD"),
        5 => String::from("\x1bE"),
        6 => "x".repeat(usize::from(a)),
        7 => std::format!("\x1b[{}S", a % 4),
        8 => std::format!("\x1b[{}T", a % 4),
        9 => std::format!("\x1b[{} @", a % 4),
        10 => std::format!("\x1b[{} A", a % 4),
        11 => std::format!("\x1b[{}L", a % 4),
        12 => std::format!("\x1b[{}M", a % 4),
        13 => std::format!("\x1b[{}@", a % 4),
        14 => std::format!("\x1b[{}P", a % 4),
        15 => std::format!("\x1b[{}'}}", a % 4),
        16 => std::format!("\x1b[{}'~", a % 4),
        17 => std::format!("\x1b[{}J", a % 4),
        18 => std::format!("\x1b[{}K", a % 3),
        19 => std::format!("\x1b[{}O", a % 3),
        20 => std::format!("\x1b[?{}J", a % 3),
        21 => std::format!("\x1b[?{}K", a % 3),
        22 => std::format!("\x1b[46;{a};{b};{c};{d}$x"),
        23 => std::format!("\x1b[{a};{b};{c};{d}$z"),
        24 => {
            let to = d % 12 + 1;
            std::format!("\x1b[{a};{b};{c};{d};1;{to};{a};1$v")
        }
        25 => std::format!("\x1b[{a};{b};{c};{d}${{"),
        26 => std::format!("\x1b[{};{}r", a % 13, b % 13),
        27 => std::format!("\x1b[{};{}s", a % 21, b % 21),
        28 => String::from("\x1b[?69h"),
        29 => String::from("\x1b[?69l"),
        30 => String::from("\x1b[?1049h"),
        31 => String::from("\x1b[?1049l"),
        32 => String::from("\x1b[?1047h"),
        33 => String::from("\x1b[?1047l"),
        34 => String::from("\x1b7"),
        35 => String::from("\x1b8"),
        36 => std::format!("\x1b[{}A", a % 13),
        37 => std::format!("\x1b[{}B", a % 13),
        38 => std::format!("\x1b[{}C", a % 21),
        39 => std::format!("\x1b[{}D", a % 21),
        40 => String::from("\t\r\x08"),
        41 => std::format!("\x1b[?7{}", ["h", "l"][usize::from(a % 2)]),
        42 => std::format!("\x1b[?6{}", ["h", "l"][usize::from(a % 2)]),
        43 => String::from("\x1b[!p"),
        44 => String::from("\x1b9\x1b6"),
        45 => std::format!("\x1b[{a}G\x1b[{b}`"),
        46 => String::from("\x1b#8"),
        // The row above the area, and there an operation and a line feed
        // onto the area's top: where a fill or a scroll that rebuilds the
        // top's word and the line feed that reads it meet, in one read.
        47 => {
            let inner = u8::try_from(c % 47).unwrap_or(0);
            let then = match step((inner, d, a, b, c), m) {
                Step::Bytes(s) => s,
                Step::Resize(_) => String::new(),
            };
            std::format!("\x1b[{above};{b}H{then}\n")
        }
        48 => std::format!("\x1b[{above};{b}H"),
        _ => {
            return Step::Resize(WinSize {
                ws_row: 8 + a % 7,
                ws_col: 12 + b % 13,
            });
        }
    };
    Step::Bytes(bytes)
}

/// The programs: margins with rows on both sides of the area, then up to
/// forty operations, each number small enough to land on the screen or
/// just past it.
fn programs() -> impl Strategy<Value = Program> {
    let small = 1u16..=26;
    let op = (0..OPS, small.clone(), small.clone(), small.clone(), small);
    (
        (2u16..=5, 6u16..=10, 2u16..=8, 9u16..=18),
        proptest::collection::vec(op, 1..40),
    )
}

/// The write fence of the cursor's line, from the margins and its row: the
/// right margin inside the scroll area under DECLRMM, and the line's last
/// column everywhere else, outside the area or with the side margins at
/// the page's edges (p. 5-139).
fn expected(t: &Term<'_>) -> Col {
    let outside = t.cy < t.cold.top || t.cy > t.cold.bot;
    match (t.cold.modes.is_set(ModeBit::SideMargins), outside) {
        (true, false) => t.cold.right,
        _ => Col::at_index(t.cols.index() - 1),
    }
}

/// Where the cursor's row is against the scroll area.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Band {
    Above,
    Inside,
    Below,
}

/// Where the cursor's row is against a scroll area that leaves rows on
/// both sides, with side margins narrower than the page: the state the
/// property is about. `None` in any other.
fn band(t: &Term<'_>) -> Option<Band> {
    let split = t.cold.modes.is_set(ModeBit::SideMargins)
        && t.cold.right.index() + 1 < t.cols.index()
        && t.cold.top.index() > 0
        && t.cold.bot.index() + 1 < t.rows.index();
    match (split, t.cy < t.cold.top, t.cy > t.cold.bot) {
        (false, _, _) => None,
        (true, true, _) => Some(Band::Above),
        (true, false, false) => Some(Band::Inside),
        (true, false, true) => Some(Band::Below),
    }
}

/// Run program `p` on a fresh terminal, calling `seen` after the margins
/// and after each operation with the terminal and the bytes so far.
fn drive(p: &Program, mut seen: impl FnMut(&Term<'_>, &str)) {
    let mut owned = Owned::new(&ROOM);
    let mut t = Term::new(owned.storage(), &START);
    let (top, bot, left, right) = p.0;
    let mut so_far = std::format!("\x1b[?69h\x1b[{top};{bot}r\x1b[{left};{right}s");
    read(&mut t, &so_far);
    seen(&t, &so_far);
    for &op in &p.1 {
        match step(op, p.0) {
            Step::Bytes(b) => {
                read(&mut t, &b);
                so_far.push_str(&b);
            }
            Step::Resize(ws) => {
                assert_eq!(
                    t.resize(&ws),
                    Resized::Ok,
                    "{}x{} fits the room",
                    ws.ws_row,
                    ws.ws_col
                );
                so_far.push_str(&std::format!("<resize {}x{}>", ws.ws_row, ws.ws_col));
            }
        }
        seen(&t, &so_far);
    }
}

/// `bytes` as one read.
fn read(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// **After every operation the fence is the cursor line's write fence**:
/// the right margin on a row inside the scroll area under DECLRMM, the
/// line's last column on every other. Cases at `PROPTEST_CASES`, 256 by
/// default; no regression file, since this is a unit test in `src/` and a
/// failing case prints its bytes, which a deterministic test then pins.
#[test]
fn the_fence_is_the_cursor_lines_write_fence_after_every_operation() {
    let config = Config {
        failure_persistence: None,
        ..Config::default()
    };
    let mut runner = TestRunner::new(config);
    let result = runner.run(&programs(), |p| {
        drive(&p, |t, so_far| {
            assert_eq!(
                t.cold.fence,
                expected(t),
                "row {} of {}..={}, right margin {}, after {so_far:?}",
                t.cy.index(),
                t.cold.top.index(),
                t.cold.bot.index(),
                t.cold.right.index()
            );
        });
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}

/// **The programs reach every band**: over 64 of them, drawn from a fixed
/// seed, the cursor is above, inside and below an area that leaves rows on
/// both sides, with the side margins narrower than the page, after some
/// operation, and each operation is drawn. A property that never stood on
/// a row outside the area would hold of a fence that ignored it.
#[test]
fn the_fence_programs_reach_every_band() {
    let mut runner = TestRunner::deterministic();
    let strategy = programs();
    let mut seen: Vec<Band> = Vec::new();
    let mut kinds: Vec<u8> = Vec::new();
    for _ in 0..64 {
        let p = strategy.new_tree(&mut runner).expect("a program").current();
        kinds.extend(p.1.iter().map(|op| op.0));
        drive(&p, |t, _| {
            if let Some(b) = band(t) {
                seen.push(b);
            }
        });
    }
    for b in [Band::Above, Band::Inside, Band::Below] {
        assert!(seen.contains(&b), "never {b:?}");
    }
    let missing: Vec<u8> = (0..OPS).filter(|k| !kinds.contains(k)).collect();
    assert!(missing.is_empty(), "never drawn: {missing:?}");
}
