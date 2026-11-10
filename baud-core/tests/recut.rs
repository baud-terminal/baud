//! **A re-cut tells each line of the primary where its text went**
//! (`Term::resize_with`, `LineRecut`): the lines a logical line held before
//! a change of width and the lines it holds after, oldest first, so an
//! embedder that keeps facts by line can carry them through the re-cut
//! instead of losing them. Read here through the core's public line
//! addressing alone -- the window's top line (`Term::history_mark`), the
//! rows under it and the history behind it -- before and after.

mod support;

use baud_core::*;
use baud_domain::{Back, LineIdx, Row, Rows, WinSize};
use proptest::prelude::*;

const ROWS: u16 = 8;
const WIDEST: u16 = 120;

fn ws(cols: u16) -> WinSize {
    sized(cols, ROWS)
}

fn sized(cols: u16, rows: u16) -> WinSize {
    WinSize {
        ws_col: cols,
        ws_row: rows,
    }
}

/// The window's rows.
fn rows(t: &Term<'_>) -> usize {
    t.geometry().1.index()
}

/// The cells of `line` of the primary, from the window or the history;
/// none for a line neither holds.
fn cells(t: &Term<'_>, line: LineIdx) -> Option<Vec<PackedCell>> {
    let top = t.history_mark().line();
    match line.since(top) {
        Some(y) if y.index() < rows(t) => t
            .row(Row::from_index(y.index()))
            .map(<[PackedCell]>::to_vec),
        Some(_) => None,
        None => {
            let back = top.since(line)?.index() - 1;
            t.scrollback().line(Back::of(back)).map(|(c, _)| c.to_vec())
        }
    }
}

/// The characters the cells hold, in order, blanks and wide tails left out:
/// what a line says whatever width it is cut at.
fn chars(c: &[PackedCell]) -> String {
    c.iter()
        .filter_map(|c| c.content.as_codepoint())
        .filter_map(|cp| char::from_u32(cp.value()))
        .collect()
}

/// The oldest line the primary holds.
fn oldest(t: &Term<'_>) -> LineIdx {
    let top = t.history_mark().line();
    top.minus(Rows::of_count(t.scrollback().len()))
        .expect("the history starts at or after the first line")
}

/// What the re-cut to `cols` told, with how it followed.
fn recut(t: &mut Term<'_>, cols: u16) -> (Vec<LineRecut>, Following) {
    recut_to(t, &ws(cols))
}

/// What the resize to `to` told, with how it followed.
fn recut_to(t: &mut Term<'_>, to: &WinSize) -> (Vec<LineRecut>, Following) {
    let mut told = Vec::new();
    let (resized, following) = t.resize_with(to, &mut |r| told.push(r));
    assert_eq!(resized, Resized::Ok);
    // In the copy's order; in the lines' order, sorted.
    told.sort_by_key(|r| r.old.first());
    (told, following)
}

/// **The map tiles what the re-cut kept and what it made of it, every line
/// that holds a cell is in it, and each line says the same before and
/// after**: lines of letters and wide characters, some longer than either
/// width, some begun under the shell's prompt mark (`OSC 133 ; A`, whose
/// rows the re-cut keeps whole, `Recut::Above`) and some double-width
/// (`DECDWL`), fed at one width on a window of one height; the height
/// changed alone first -- rows hidden or taken back from the history -- and
/// the window held some lines back; then re-cut to another width and
/// height, over a history short enough that a re-cut to a narrower width
/// overflows it and drops lines from the oldest end. The first line told
/// is the first the re-cut kept: the old oldest plus what it dropped
/// (`Term::reflow_dropped`), before and after alike; the last reaches the
/// cursor's line; and every line told is compared, none skipped. Named
/// break: each line's new lines one later than it says (`LineRecut::new`
/// shifted in the re-cut's hook) -- the first line told starts a line
/// after the first the re-cut kept.
#[test]
fn a_re_cut_tells_each_line_where_its_text_went() {
    let line = (
        prop::collection::vec(prop_oneof![4 => b'a'..=b'z', 1 => Just(0u8)], 0..140),
        prop_oneof![6 => Just(0u8), 1 => Just(1u8), 1 => Just(2u8)],
    );
    let lines = prop::collection::vec(line, 1..40);
    let config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    let geometry = (
        5u16..=WIDEST,
        5u16..=WIDEST,
        2u16..=12,
        2u16..=12,
        2u16..=12,
        0usize..6,
    );
    proptest!(config, |(lines in lines, (from, to, tall, between, after, back) in geometry, depth in 0usize..24)| {
        prop_assume!(from != to);
        let mut owned = Owned::with_history(&sized(WIDEST, 12), &CacheGeometry::UNKNOWN, Depth::of(depth));
        let mut t = Term::new(owned.storage(), &sized(from, tall));
        let mut bytes = Vec::new();
        for (i, (l, kind)) in lines.iter().enumerate() {
            if i > 0 {
                bytes.extend_from_slice(b"\r\n");
            }
            match kind {
                1 => bytes.extend_from_slice(b"\x1b]133;A\x07"),
                // A double-width line is a line of one row, kept whole by a
                // re-cut: in capitals, as many as fit its row.
                2 => {
                    bytes.extend_from_slice(b"\x1b#6");
                    let fit = l.iter().filter(|&&b| b != 0).take(usize::from(from / 2));
                    bytes.extend(fit.map(u8::to_ascii_uppercase));
                    continue;
                }
                _ => {}
            }
            for &b in l {
                match b {
                    0 => bytes.extend_from_slice("\u{3042}".as_bytes()),
                    b => bytes.push(b),
                }
            }
        }
        support::feed(&mut t, &bytes);
        // Every line held, by its number, read while the window shows them
        // all: a change of height renumbers nothing, and the rows a shorter
        // window hides below it are lines the re-cut reads too.
        let top = t.history_mark().line();
        let mut held = Vec::new();
        let mut l = oldest(&t);
        while l < top.plus(Rows::of_count(rows(&t))) {
            held.push((l, cells(&t, l).map(|c| chars(&c))));
            l = l.next();
        }
        let below = l;
        // The height alone, which re-cuts nothing, and the window held back.
        let (_, following) = recut_to(&mut t, &sized(from, between));
        prop_assert_eq!(following, Following::Followed);
        t.set_window_back(Rows::of_count(back));
        let before = oldest(&t);
        let was_dropped = t.reflow_dropped();
        // A line past the last held is a row a taller window added below
        // them, blank.
        let was_line = |l: LineIdx| -> Option<String> {
            match l < below {
                true => held.iter().find(|(h, _)| *h == l)?.1.clone(),
                false => Some(String::new()),
            }
        };
        // The rows the last prompt mark began are kept whole, row for row
        // (`Recut::Above`), as a double-width line is, each showing what
        // fits of it at the new width.
        let top = t.history_mark().line();
        let kept = t.prompts().last().map(|p| match p.line {
            PromptLine::Back(n) => top.minus(n).expect("a prompt the history holds"),
            PromptLine::Screen(r) => top.plus(Rows::of_count(r.index())),
        });
        let (told, following) = recut_to(&mut t, &sized(to, after));
        prop_assert_eq!(following, Following::Followed);
        let dropped = t.reflow_dropped().index() - was_dropped.index();
        let first = before.plus(Rows::of_count(dropped));
        // Nothing told is one of two things. A line longer than the whole
        // ring at the new width is dropped with the rest: what the screen
        // then holds is no line's, which the coverage below holds to no
        // cell. Or the last prompt mark's line has left the history, so
        // every line kept is of its block and nothing is re-cut
        // (`Term::primary_cut`): each line keeps its number and shows what
        // fits of it, held below.
        let renumbered = !told.is_empty() || dropped > 0;
        if let (Some(r), Some(z)) = (told.first(), told.last()) {
            prop_assert_eq!(r.old.first(), first, "the first kept, before");
            prop_assert_eq!(r.new.first(), first, "the first kept, after");
            prop_assert_eq!(oldest(&t), first, "the oldest line now");
            let cursor = t.history_mark().line().plus(Rows::of_count(t.cursor().0.index()));
            prop_assert!(z.new.last() >= cursor, "the last told, {:?}, reaches the cursor's line, {:?}", z.new, cursor);
        }
        for w in told.windows(2) {
            prop_assert_eq!(w[1].old.first(), w[0].old.last().next(), "old tiles: {:?}", w);
            prop_assert_eq!(w[1].new.first(), w[0].new.last().next(), "new tiles: {:?}", w);
        }
        // Every line the primary now holds with a cell on it is told.
        let top = t.history_mark().line();
        let mut l = oldest(&t);
        while renumbered && l < top.plus(Rows::of_count(rows(&t))) {
            if cells(&t, l).is_some_and(|c| !chars(&c).is_empty()) {
                prop_assert!(
                    told.iter().any(|r| r.new.holds(l)),
                    "line {:?} holds a cell and is told by no line",
                    l
                );
            }
            l = l.next();
        }
        // Each line told says the same before and after, every one read. A
        // line in the rows a shorter window hides below it reads as blank,
        // which it is: the text ends at the cursor, with only blank rows
        // below it, and a re-cut puts nothing under the cursor's line.
        let hidden = top.plus(Rows::of_count(rows(&t)));
        let hidden = (hidden, hidden.plus(t.hidden()));
        let now_line = |l: LineIdx| -> Option<String> {
            match l >= hidden.0 && l < hidden.1 {
                true => Some(String::new()),
                false => cells(&t, l).map(|c| chars(&c)),
            }
        };
        let joined = |span: Span, read: &dyn Fn(LineIdx) -> Option<String>| -> Option<Vec<String>> {
            let mut out = Vec::new();
            let mut l = span.first();
            loop {
                out.push(read(l)?);
                if l == span.last() {
                    return Some(out);
                }
                l = l.next();
            }
        };
        let mut compared = 0;
        for r in &told {
            let was = joined(r.old, &was_line);
            let now = joined(r.new, &now_line);
            prop_assert!(was.is_some() && now.is_some(), "{:?} read before {:?} and after {:?}", r, was, now);
            let (was, now) = (was.unwrap_or_default(), now.unwrap_or_default());
            let double = was.concat().bytes().any(|b| b.is_ascii_uppercase());
            if double {
                prop_assert!(r.old.first() == r.old.last() && r.new.first() == r.new.last(), "one row: {:?}", r);
            }
            match double || kept.is_some_and(|k| r.old.first() >= k) {
                true => {
                    prop_assert_eq!(was.len(), now.len(), "kept row for row: {:?}", r);
                    for (w, n) in was.iter().zip(&now) {
                        prop_assert!(w.starts_with(n.as_str()), "{:?} kept whole: {:?} shows {:?}", r, w, n);
                    }
                }
                false => prop_assert_eq!(was.concat(), now.concat(), "{:?}", r),
            }
            compared += 1;
        }
        prop_assert_eq!(compared, told.len());
        if !renumbered {
            prop_assert_eq!(oldest(&t), before, "nothing re-cut keeps the oldest");
            let mut l = before;
            let mut shown = 0;
            while l < top.plus(Rows::of_count(rows(&t))) {
                let (w, n) = (was_line(l), now_line(l));
                prop_assert!(w.is_some() && n.is_some(), "{:?} read before {:?} and after {:?}", l, w, n);
                let (w, n) = (w.unwrap_or_default(), n.unwrap_or_default());
                prop_assert!(w.starts_with(n.as_str()), "{:?} kept: {:?} shows {:?}", l, w, n);
                shown += 1;
                l = l.next();
            }
            prop_assert!(shown >= rows(&t), "every row of the window read");
        }
    });
}

/// **A re-cut that overflows the history drops lines from the oldest end
/// and tells none of them**: forty lines of forty columns on a twenty-row
/// history, narrowed to twelve. The first line told is the old oldest plus
/// what was dropped, which is more than none.
#[test]
fn a_re_cut_that_drops_lines_tells_none_of_them() {
    let mut owned = Owned::with_history(&ws(40), &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &ws(40));
    let s: String = (0..40)
        .map(|i| format!("{i:02}{}\r\n", "x".repeat(36)))
        .collect();
    support::feed(&mut t, s.as_bytes());
    let before = oldest(&t);
    let (told, _) = recut(&mut t, 12);
    let dropped = t.reflow_dropped();
    assert!(dropped.index() > 0, "the control: the re-cut dropped lines");
    assert_eq!(told[0].old.first(), before.plus(dropped));
}

/// **The shell's rows kept whole are told one for one**: a line of output
/// longer than the new width, then a prompt and its typed text past it,
/// narrowed. The output's line, the first, takes two lines; the prompt's,
/// the second, keeps its one. The control: the same with no prompt mark --
/// the typed line is re-cut too.
#[test]
fn the_shells_rows_kept_whole_are_told_one_for_one() {
    let lens = |marked: bool| {
        support::with_term(&ws(40), |t| {
            let mark = match marked {
                true => "\x1b]133;A\x07",
                false => "",
            };
            let s = format!("{}\r\n{mark}$ {}", "o".repeat(35), "p".repeat(30));
            support::feed(t, s.as_bytes());
            let first = oldest(t);
            let (told, following) = recut(t, 20);
            assert_eq!(following, Following::Followed);
            let len = |s: Span| s.last().since(s.first()).map_or(0, |r| r.index()) + 1;
            let of = |l: LineIdx| {
                told.iter()
                    .find(|r| r.old.first() == l)
                    .map(|r| (len(r.old), len(r.new)))
            };
            (of(first), of(first.next()))
        })
    };
    assert_eq!(
        lens(true),
        (Some((1, 2)), Some((1, 1))),
        "the prompt's row kept whole"
    );
    assert_eq!(
        lens(false),
        (Some((1, 2)), Some((1, 2))),
        "the control: no mark"
    );
}
