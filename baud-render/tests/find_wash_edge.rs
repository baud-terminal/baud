//! **A match a soft wrap splits across the window's last row washes
//! only that row** while the window looks back and no paragraph is
//! re-broken: the line below the window is drawn on none of its rows,
//! so no wash lands on the status line, a notice row or the padding.

mod support;
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Find, FindLook, PxSize, Renderer, Stack, Theme, WordWrap};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 10,
    ws_col: 30,
};

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn washes(wrap: WordWrap, status: bool) -> Vec<(i32, i32)> {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut s = String::new();
    for i in 0..20 {
        s.push_str(&format!("h{i}\r\n"));
    }
    // A row that wraps: "NEEDLE" crosses the wrap.
    s.push_str(&"x".repeat(26));
    s.push_str("NEEDLE tail\r\n");
    for i in 0..4 {
        s.push_str(&format!("l{i}\r\n"));
    }
    s.push_str("NEEDLE\r\n");
    if status {
        s.push_str("\x1b[2$~\x1b[1$}status\x1b[0$}");
    }
    feed(&mut t, &s);
    // Find the row: live rows; look back so the wrapped row is the window's last.
    let mut back = Rows::ZERO;
    for k in 0..15usize {
        let b = Rows::of_count(k);
        let v = t.view(b);
        let last = baud_domain::Row::from_index(v.rows().index() - 1);
        if v.wrapped(last) == baud_domain::Wrap::Continues {
            back = b;
            break;
        }
    }
    assert!(back.index() > 0, "found a look back");
    let mut f = Find::new();
    let q: Vec<baud_domain::Codepoint> = "NEEDLE"
        .chars()
        .map(baud_domain::Codepoint::from_char)
        .collect();
    f.set_query(&t, q, back);
    let marks: Vec<baud_render::Mark> = f
        .marks(&t, back)
        .into_iter()
        .map(|mut m| {
            m.current = baud_render::Current::No;
            m
        })
        .collect();
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(30),
        Rows::of_index(10),
    )
    .unwrap();
    r.set_word_wrap(wrap);
    r.set_find(
        &t,
        Some(FindLook {
            marks,
            field: f.field(Cols::of_index(30)),
        }),
    );
    r.lay_out_bidi(&t, back);
    let m = r.metrics();
    let (cw, ch) = (
        i32::try_from(m.cell_w.value()).unwrap(),
        i32::try_from(m.cell_h.value()).unwrap(),
    );
    let mut w: Vec<(i32, i32)> = r
        .find_solids()
        .iter()
        .flat_map(|i| {
            let (y, x, n) = (
                i32::from(i.y.0) / ch,
                i32::from(i.x.0) / cw,
                i32::from(i.w.0) / cw,
            );
            (x..x + n).map(move |c| (y, c))
        })
        .collect();
    w.sort();
    w
}

#[test]
fn a_match_split_across_the_last_row_washes_nothing_below_the_window() {
    for wrap in [WordWrap::Off, WordWrap::On] {
        for status in [false, true] {
            let w = washes(wrap, status);
            assert!(
                w.iter().all(|&(y, _)| y < 10),
                "wrap {wrap:?} status {status}: a wash below the window: {w:?}"
            );
            assert!(
                (26..30).all(|c| w.contains(&(9, c))),
                "wrap {wrap:?} status {status}: the last row's half is washed: {w:?}"
            );
        }
    }
}
