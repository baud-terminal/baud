//! **Find in the scrollback**, the list a driver keeps and what a frame
//! draws of it: the list brought up to date after a read is the list read
//! afresh, the current match steps from the newest to older ones and wraps,
//! and the CPU path draws every mark and the field, and leaves nothing of
//! them behind when they go.

mod support;

use baud_core::{CacheGeometry, Depth, Owned, Term};
use baud_domain::{BytePos, Codepoint, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Current, Damage, Find, FindLook, PxSize, Renderer, Stack, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 30,
    ws_row: 6,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn query(q: &str) -> Vec<Codepoint> {
    q.chars().map(Codepoint::from_char).collect()
}

fn with_term<R>(depth: usize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let room = WinSize {
        ws_col: 40,
        ws_row: 10,
    };
    let mut owned = Owned::with_history(&room, &CacheGeometry::UNKNOWN, Depth::of(depth));
    let mut t = Term::new(owned.storage(), &WS);
    f(&mut t)
}

/// The list a fresh query reads now, to hold a kept one to.
fn afresh(t: &Term<'_>, q: &str) -> Vec<baud_core::Found> {
    let mut f = Find::new();
    f.set_query(t, query(q), Rows::ZERO);
    f.matches().to_vec()
}

#[test]
fn the_list_kept_across_reads_is_the_list_read_afresh() {
    with_term(40, |t| {
        feed(t, b"a needle\r\nhay\r\n");
        let mut f = Find::new();
        f.set_query(t, query("needle"), Rows::ZERO);
        assert_eq!(f.matches().len(), 1);
        let steps: &[&[u8]] = &[
            // Lines scrolled into the history, one with a match.
            b"x\r\ny\r\nneedle two\r\nz\r\nw\r\nv\r\n",
            // A match on the screen overwritten in place.
            b"\x1b[1;1Hnothing here\x1b[6;1H",
            // A match written across a wrap.
            b"----------------------------needle\r\n",
            // A line taken back over the screen's top.
            b"\x1b[1;1H\x1bMneedle back\x1b[6;1H",
            // The screen erased, the history kept.
            b"\x1b[2Jneedle after\r\n",
            // The alternate screen, and back.
            b"\x1b[?1049hneedle alt\x1b[?1049l",
            // The history erased.
            b"\x1b[3Jneedle last\r\n",
        ];
        for (i, step) in steps.iter().enumerate() {
            feed(t, step);
            f.refresh(t);
            assert_eq!(f.matches(), &afresh(t, "needle")[..], "after step {i}");
        }
        // A new width reflows every line.
        let _ = t.resize(&WinSize {
            ws_col: 20,
            ws_row: 6,
        });
        f.refresh(t);
        assert_eq!(f.matches(), &afresh(t, "needle")[..], "after the resize");
    });
}

#[test]
fn a_match_whose_line_retired_is_dropped() {
    with_term(4, |t| {
        feed(t, b"needle\r\n");
        let mut f = Find::new();
        f.set_query(t, query("needle"), Rows::ZERO);
        assert!(f.current().is_some());
        for i in 0..20 {
            feed(t, format!("x{i}\r\n").as_bytes());
        }
        f.refresh(t);
        assert!(f.matches().is_empty());
        assert_eq!(f.current(), None);
    });
}

#[test]
fn find_next_goes_older_and_find_previous_newer_both_wrapping() {
    with_term(40, |t| {
        feed(t, b"m1\r\nm2\r\nm3\r\n");
        let mut f = Find::new();
        f.set_query(t, query("m"), Rows::ZERO);
        let text = |f: &Find| f.current().unwrap().first.line.value();
        let lines: Vec<u64> = f.matches().iter().map(|m| m.first.line.value()).collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(text(&f), lines[2], "the newest is current first");
        assert_eq!(f.position().unwrap().nth.value(), 1);
        f.next();
        assert_eq!(text(&f), lines[1]);
        f.next();
        f.next();
        assert_eq!(text(&f), lines[2], "past the oldest, the newest");
        f.previous();
        assert_eq!(text(&f), lines[0], "past the newest, the oldest");
        assert_eq!(f.position().unwrap().nth.value(), 3);
        assert_eq!(f.position().unwrap().of.value(), 3);
    });
}

#[test]
fn the_window_moves_to_a_match_in_the_history_and_stays_for_one_it_shows() {
    with_term(40, |t| {
        feed(t, b"needle\r\n");
        for i in 0..20 {
            feed(t, format!("x{i}\r\n").as_bytes());
        }
        let mut f = Find::new();
        f.set_query(t, query("needle"), Rows::ZERO);
        let back = f.window_for(t, Rows::ZERO).unwrap();
        assert!(back.index() > 0);
        t.set_window_back(back);
        let marks = f.marks(t, t.window_back());
        assert_eq!(marks.len(), 1, "the match is in the window");
        assert_eq!(marks[0].current, Current::Yes);
        assert_eq!(f.window_for(t, t.window_back()), Some(t.window_back()));
    });
}

#[test]
fn a_wrapped_match_is_marked_on_each_of_its_rows() {
    with_term(40, |t| {
        feed(t, b"----------------------------needle");
        let mut f = Find::new();
        f.set_query(t, query("needle"), Rows::ZERO);
        let marks = f.marks(t, Rows::ZERO);
        assert_eq!(marks.len(), 2);
        assert_eq!(
            (marks[0].row, marks[0].from, marks[0].to),
            (Row::from_index(0), Col::at_index(28), Col::at_index(29))
        );
        assert_eq!(
            (marks[1].row, marks[1].from, marks[1].to),
            (Row::from_index(1), Col::ZERO, Col::at_index(3))
        );
    });
}

#[test]
fn the_field_shows_the_querys_tail_and_what_was_found() {
    with_term(40, |t| {
        feed(t, b"abc abc");
        let text = |f: &Find| -> String {
            let field = f.field(Cols::of_index(30));
            field
                .text
                .iter()
                .filter_map(|c| char::from_u32(c.value()))
                .collect()
        };
        let mut f = Find::new();
        f.set_query(t, query("abc"), Rows::ZERO);
        let shown = text(&f);
        assert!(shown.starts_with(" Find abc"), "{shown:?}");
        assert!(shown.ends_with(" 1 of 2 "), "{shown:?}");
        let field = f.field(Cols::of_index(30));
        assert_eq!(field.row, Row::from_index(0));
        assert_eq!(
            field.col,
            Col::ZERO,
            "a third of thirty is under the least width"
        );
        assert_eq!(field.caret, Col::at_index(9));
        assert_eq!(field.text.len(), 30);
        f.set_query(t, query("zzz"), Rows::ZERO);
        assert!(text(&f).ends_with(" none "));
        // A query longer than the field shows its end.
        f.set_query(t, query("0123456789abcdefghijklmnopqrstuvwxyz"), Rows::ZERO);
        let shown = text(&f);
        assert!(shown.contains("xyz"), "{shown:?}");
        assert!(!shown.contains("0123"), "{shown:?}");
        assert_eq!(f.field(Cols::of_index(30)).text.len(), 30);
    });
}

struct Painter<'f> {
    faces: &'f [Face<'f>],
}

impl<'f> Painter<'f> {
    fn renderer(&self) -> Renderer<'f, 'f> {
        Renderer::new(
            Stack::new(self.faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap()
    }
}

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32], damage: Damage) {
    let (w, h) = r.pixel_size();
    let mut target = Target::new(px, w, w, h).unwrap();
    r.render(t, &mut target, damage);
}

#[test]
fn every_mark_and_the_field_are_drawn_and_leave_nothing_when_they_go() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let p = Painter { faces: &faces };
    with_term(40, |t| {
        feed(t, b"\x1b[?25l\r\n\r\nab needle cd needle");
        let mut f = Find::new();
        f.set_query(t, query("needle"), Rows::ZERO);
        let look = FindLook {
            marks: f.marks(t, Rows::ZERO),
            field: f.field(t.geometry().0),
        };
        let mut r = p.renderer();
        let (w, h) = r.pixel_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let mut plain = vec![0u32; n];
        paint(&mut r, t, &mut plain, Damage::Full);
        // Drawn with the find, incrementally over the plain frame.
        let mut found = plain.clone();
        r.set_find(t, Some(look));
        paint(&mut r, t, &mut found, Damage::Incremental);
        let m = r.metrics();
        let cell = |row: usize, col: usize| {
            let (cw, ch) = (
                usize::try_from(m.cell_w.value()).unwrap(),
                usize::try_from(m.cell_h.value()).unwrap(),
            );
            let stride = usize::try_from(w.value()).unwrap();
            (row * ch..(row + 1) * ch)
                .flat_map(move |y| (col * cw..(col + 1) * cw).map(move |x| y * stride + x))
        };
        let differs = |row: usize, col: usize| cell(row, col).any(|i| plain[i] != found[i]);
        // The older match is washed; the newest is the current one, left
        // to the selection, which this frame does not set.
        assert!(differs(2, 3), "the older match is marked");
        assert!(!differs(2, 13), "the current match is not washed");
        assert!(!differs(2, 1), "a cell outside every match is untouched");
        // The field covers the top row from its first column.
        assert!(differs(0, 0) || differs(0, 5), "the field is drawn");
        // Gone, incrementally: the frame is the plain one again.
        r.set_find(t, None);
        paint(&mut r, t, &mut found, Damage::Incremental);
        assert!(found == plain, "nothing of the find is left behind");
    });
}

/// **The wash is the palette's yellow as a program sets it**: `OSC 4`
/// moving palette entry 3 moves the wash, as it moves `SGR 33`'s text.
#[test]
fn a_program_palette_moves_the_wash() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let p = Painter { faces: &faces };
    with_term(40, |t| {
        feed(t, b"\x1b[?25l\r\n\r\nab cd ab cd");
        let mut f = Find::new();
        f.set_query(t, query("ab"), Rows::ZERO);
        let mut r = p.renderer();
        let (w, h) = r.pixel_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let look = FindLook {
            marks: f.marks(t, Rows::ZERO),
            field: f.field(t.geometry().0),
        };
        let mut yellow = vec![0u32; n];
        r.set_find(t, Some(look));
        paint(&mut r, t, &mut yellow, Damage::Full);
        feed(t, b"\x1b]4;3;rgb:00/00/ff\x1b\\");
        let look = FindLook {
            marks: f.marks(t, Rows::ZERO),
            field: f.field(t.geometry().0),
        };
        let mut blue = vec![0u32; n];
        r.set_find(t, Some(look));
        paint(&mut r, t, &mut blue, Damage::Full);
        // The older match's first cell, on the third row.
        let m = r.metrics();
        let (cw, ch) = (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        );
        let stride = usize::try_from(w.value()).unwrap();
        let px = |v: &[u32]| v[(2 * ch + ch / 2) * stride + cw / 2];
        let (y, b) = (px(&yellow), px(&blue));
        assert_ne!(y, b, "the wash follows the program's palette");
        assert!(b & 0xff > (b >> 16) & 0xff, "blue now: {b:08x}");
    });
}
