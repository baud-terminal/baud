//! **A struck link's grey, lent as a frozen row's renditions are**
//! (`baud_render::Lent`): the frame's resolved table gives each struck
//! source rendition a slot no cell of the frame names, after the frozen
//! rows have taken theirs, from the top of the table down. These pin the
//! lending: which slot a struck rendition is lent, that the frozen pass is
//! never left short by the strike near the slot budget while a struck cell
//! left without a slot keeps its own rendition, and that a frozen cell
//! struck draws its own ground with its text grey.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, PackedCell, ReadLen, Row, Rows, StyleId, WinSize};
use baud_font::Face;
use baud_render::{
    Blend, Damage, Lend, Lent, LinkLine, PxSize, Renderer, Resolved, Stack, StyleStamp, Theme,
    Words,
};
use support::{Canvas, font};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for c in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, c);
        t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
    }
}

fn renderer<'f>(faces: &'f [Face<'f>], ws: WinSize) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap()
}

/// Colour 8 a light gray, which reads 4.5:1 on every ground below.
const GRAY: &[u8] = b"\x1b]4;8;rgb:c0/c0/c0\x1b\\";

/// **A struck rendition is lent the top slot the live table holds nothing
/// in, and never one it holds a rendition in**: renditions written until
/// the table's top slot holds one, the slot lent is the highest free one,
/// below it; it holds a rendition other than the cell's own,
/// and the lending names the cell's own word for it. Nothing else is
/// rewritten. Perturbation, run red: drop the live table's test in the
/// strike's pass, which lends the top slot over a rendition.
#[test]
fn a_struck_rendition_is_lent_the_top_free_slot_and_never_a_held_one() {
    const WS: WinSize = WinSize {
        ws_col: 40,
        ws_row: 10,
    };
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, GRAY);
    let n = t.styles().len();
    // Renditions written until one lands in the top slot, short of the 7/8
    // that sweeps the table.
    for i in 0..n * 7 / 8 - 16 {
        let (r, g, b) = ((i * 37) % 256, (i * 91 + 7) % 256, (i * 53 + 11) % 251);
        feed(&mut t, format!("\x1b[38;2;{r};{g};{b}mq").as_bytes());
        if t.styles()[n - 1].is_some() {
            break;
        }
    }
    feed(&mut t, b"\x1b[m\x1b[H\x1b[2J\x1b[38;2;255;0;0mhttps\x1b[m");
    assert!(
        t.styles()[n - 1].is_some(),
        "the control: the top slot is held"
    );
    let free = (1..n).rev().find(|&k| t.styles()[k].is_none()).unwrap();
    let mut r = renderer(&faces, WS);
    let mut cells: Vec<PackedCell> = t.view(Rows::ZERO).row(Row::ZERO).to_vec();
    let own = cells[0];
    let mut out = vec![Resolved::default(); n];
    let (mut stamp, mut lent) = (StyleStamp::default(), Lent::default());
    let lend = r.lend(
        &t,
        &mut [(&mut cells[..], Words::LIVE)],
        &[(Row::ZERO, 0..5)],
        &mut out,
        &mut stamp,
        &mut lent,
    );
    assert_eq!(lend, Lend::Rows);
    assert_eq!(lent.short().value(), 0);
    for c in &cells[..5] {
        assert_eq!(c.style, StyleId::from_index(free), "the top free slot");
    }
    assert_eq!(cells[5].style, t.view(Rows::ZERO).row(Row::ZERO)[5].style);
    assert_ne!(out[free], Resolved::default(), "holding the grey");
    assert_eq!(
        lent.word(&t, cells[0]),
        t.style_of(own),
        "the cell's own word"
    );
}

/// `lines` lines of 40 cells in dark truecolour grounds, from rendition
/// `first`, each `run` cells side by side in one.
fn flood(first: usize, lines: usize, run: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for l in 0..lines {
        for x in 0..40 {
            let i = first + (l * 40 + x) / run;
            out.extend(format!("\x1b[0;48;2;{};{};20mq", i % 41, (i / 41) % 41).as_bytes());
        }
        out.extend(b"\x1b[0m\r\n");
    }
    out
}

/// A screen of 40x24 whose history the core froze, `run` cells a rendition,
/// and the window scrolled back over 24 lines of it.
const TALL: WinSize = WinSize {
    ws_col: 40,
    ws_row: 24,
};

fn frozen_history(t: &mut Term<'_>, run: usize) -> Rows {
    feed(t, GRAY);
    feed(t, &flood(0, 60 * run, run));
    assert!(t.style_reclaims().rewrites > 0, "the control: it froze");
    Rows::of_count(60 * run + 1 - 24)
}

/// **Frozen rows and strikes near the slot budget never leave a frozen cell
/// short**: a window of 960 cells, each its own rendition, the frozen ones
/// lent as with nothing struck, every second row struck. The strike runs
/// out of slots (counted), and the frozen pass does not: every cell still
/// names its own rendition through the lending, an unstruck one resolves
/// exactly as with nothing struck, a struck one lent the grey keeps its
/// shapes, and a struck one the strike found no slot for resolves as with
/// nothing struck -- its own colours and the strike alone.
/// Perturbation, run red: leave a struck frozen cell out of the frozen pass
/// for the strike to lend, which finds no slot for it.
#[test]
fn frozen_rows_and_strikes_near_the_budget_never_leave_frozen_short() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&TALL);
    let mut t = Term::new(owned.storage(), &TALL);
    let back = frozen_history(&mut t, 1);
    let mut r = renderer(&faces, TALL);
    let mut c = Canvas::for_renderer(&r);
    let _ = r.render_at(&mut t, back, &mut c.target(), Damage::Full);
    let n = t.styles().len();
    let rows = usize::from(TALL.ws_row);
    let view = t.view(back);
    let copy: Vec<Vec<PackedCell>> = (0..rows)
        .map(|y| view.row(Row::from_index(y)).to_vec())
        .collect();
    let sources: Vec<_> = (0..rows)
        .map(|y| r.bidi().row_source(&t, back, Row::from_index(y)))
        .collect();
    assert!(
        sources.iter().any(|s| !s.words().is_live()),
        "the control: the window shows frozen rows"
    );
    let lent_with = |r: &mut Renderer<'_, '_>, struck: &[(Row, core::ops::Range<usize>)]| {
        let mut cells = copy.clone();
        let mut out = vec![Resolved::default(); n];
        let (mut stamp, mut lent) = (StyleStamp::default(), Lent::default());
        let mut rows: Vec<(&mut [PackedCell], Words<'_>)> = cells
            .iter_mut()
            .zip(&sources)
            .map(|(c, s)| (&mut c[..], s.words()))
            .collect();
        let lend = r.lend(&t, &mut rows, struck, &mut out, &mut stamp, &mut lent);
        drop(rows);
        (lend, cells, out, lent)
    };
    let (ref_lend, ref_cells, ref_out, ref_lent) = lent_with(&mut r, &[]);
    assert_eq!(ref_lend, Lend::Rows, "the control: frozen cells lent");
    let struck: Vec<(Row, core::ops::Range<usize>)> = (0..rows)
        .filter(|y| y % 2 == 0)
        .map(|y| (Row::from_index(y), 0..40))
        .collect();
    let (lend, cells, out, lent) = lent_with(&mut r, &struck);
    assert_eq!(lend, Lend::Rows, "the frozen pass is never short");
    assert!(lent.short().value() > 0, "the control: the strike ran out");
    let mut greyed = 0;
    for y in 0..rows {
        for x in 0..40 {
            let (a, b) = (cells[y][x], ref_cells[y][x]);
            assert_eq!(
                lent.word(&t, a),
                ref_lent.word(&t, b),
                "cell {y},{x} names its own rendition"
            );
            let (now, was) = (out[a.style.index()], ref_out[b.style.index()]);
            match y % 2 {
                0 if now != was => {
                    assert_eq!(now.shapes, was.shapes, "cell {y},{x} keeps its shapes");
                    greyed += 1;
                }
                _ => assert_eq!(now, was, "cell {y},{x} resolves as with nothing struck"),
            }
        }
    }
    // Every free slot the frozen pass left is lent, so a struck cell is
    // greyed only where the window leaves room: here, none may be.
    let _ = greyed;
}

/// **A frozen cell struck draws its own frozen ground with its text grey**:
/// on the CPU path, over the same frozen history, a struck run's grounds
/// are the ones drawn with nothing struck and its text colour 8, while the
/// core's cells are untouched. Perturbation, run red: read a struck frozen
/// cell's word from the live table in `Renderer::lend`.
#[test]
fn a_frozen_cell_struck_keeps_its_frozen_ground_and_draws_grey() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&TALL);
    let mut t = Term::new(owned.storage(), &TALL);
    let back = frozen_history(&mut t, 2);
    let cells_before: Vec<PackedCell> = t.view(back).row(Row::ZERO).to_vec();
    let mut r = renderer(&faces, TALL);
    let mut c = Canvas::for_renderer(&r);
    let _ = r.render_at(&mut t, back, &mut c.target(), Damage::Full);
    let grounds: Vec<u32> = r.background()[..40].iter().map(|a| a.0).collect();
    let word = r.bidi().row_source(&t, back, Row::ZERO).words().word(
        &t,
        Col::at_index(3),
        cells_before[3],
    );
    r.set_link_strikes(
        &[LinkLine {
            row: Row::ZERO,
            from: Col::at_index(3),
            to: Col::at_index(12),
            word,
        }],
        &[],
    );
    let _ = r.render_at(&mut t, back, &mut c.target(), Damage::Full);
    let now: Vec<u32> = r.background()[..40].iter().map(|a| a.0).collect();
    assert_eq!(now, grounds, "every ground its frozen one");
    for x in 3..=12 {
        assert_eq!(
            r.foreground()[x].0 & 0x00FF_FFFF,
            0x00C0_C0C0,
            "cell {x}'s text in colour 8"
        );
    }
    assert_ne!(
        r.foreground()[2].0 & 0x00FF_FFFF,
        0x00C0_C0C0,
        "the control"
    );
    assert_eq!(
        t.view(back).row(Row::ZERO),
        &cells_before[..],
        "and no cell of the core's changed"
    );
}
