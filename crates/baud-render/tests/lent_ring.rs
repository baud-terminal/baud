//! **A lending reads the set's copy where it lies**: on a cell frame whose
//! window holds frozen history, the ring turned or not, the renditions lent free slots are rewritten in the set's copy and the walk
//! reads that copy through the ring's rotation, row `d` at slot
//! `(head + d) % ring`, with no cell of it copied to lay the rows out in
//! screen order; and the frame is, cell for cell, the one a renderer with
//! no frame before it builds whole.
//!
//! `[[source: dec-std-070]]`'s SGR, page 5-104, note 3: "In no way does the
//! selection of current rendition affect the rendition of characters already
//! entered into the display."

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, PackedCell, ReadLen, RowExtent, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::cell_frame::{CellFrame, CellInputs, CellParts, build_with};
use baud_render::counters::{self, Counter, Site};
use baud_render::screens::{SETS, Screens, SetIx};
use baud_render::{
    Blend, Candidate, GlyphMap, Instance, Leaving, Lent, MapEntry, Overlay, PxSize, Renderer,
    Resolved, Stack, StyleStamp, Theme,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 10,
    ws_col: 40,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for c in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, c);
        t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
    }
}

/// `lines` lines, every cell its own foreground and background, each line
/// the same letters, from `first`.
fn flood(first: usize, lines: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for l in 0..lines {
        for x in 0..40 {
            let i = first + l * 40 + x;
            let b = i.to_le_bytes();
            let letter = char::from(b'a' + u8::try_from(x % 26).unwrap());
            out.extend(
                format!(
                    "\x1b[0;38;2;{};{};9;48;2;9;{};{}m{letter}",
                    b[0], b[1], b[1], b[0]
                )
                .as_bytes(),
            );
        }
        out.extend(b"\x1b[0m\r\n");
    }
    out
}

fn made<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    let mut r = Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    r.set_threads(core::num::NonZeroUsize::MIN);
    r
}

struct Cells<'m> {
    map: GlyphMap<'m>,
    exceptions: Overlay,
    screens: Screens,
    copies_mem: [Vec<PackedCell>; SETS],
    styles_mem: [Vec<Resolved>; SETS],
    overlay_mem: [Vec<u8>; SETS],
    style_stamps: [StyleStamp; SETS],
    lent: [Lent; SETS],
    candidates: Vec<Candidate>,
    records: Vec<Instance>,
    grounds: Vec<Instance>,
    lent_used: Vec<RowExtent>,
    used: Vec<RowExtent>,
}

impl<'m> Cells<'m> {
    fn new(backing: &'m mut [MapEntry], t: &Term<'_>) -> Cells<'m> {
        let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
        let stride = t.stride().index();
        Cells {
            map: GlyphMap::over(backing),
            exceptions: Overlay::new(),
            screens: Screens::new(),
            copies_mem: core::array::from_fn(|_| vec![PackedCell::EMPTY; (rows + 2) * stride]),
            styles_mem: core::array::from_fn(|_| vec![Resolved::default(); t.styles().one().len()]),
            overlay_mem: core::array::from_fn(|_| vec![0u8; cols * (rows + 1)]),
            style_stamps: [StyleStamp::default(); SETS],
            lent: core::array::from_fn(|_| Lent::default()),
            candidates: Vec::new(),
            records: Vec::new(),
            grounds: Vec::new(),
            lent_used: Vec::new(),
            used: Vec::new(),
        }
    }
}

/// **The cell frame `r` builds into set 0** over the window `back` lines
/// up, as the app's `present` builds it with no composition, prediction or
/// slide.
fn cell_frame(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    back: Rows,
    cell: &mut Cells<'_>,
) -> CellFrame {
    let seal = t.seal_writes();
    let t = &*t;
    r.fit(t);
    r.set_images(t, back);
    r.splash_yields_to(t, Px::of(0));
    r.place_marks(t, back, Leaving::None, Px::of(0));
    r.place_lights(t);
    let live = t
        .cursor_visible()
        .then(|| t.cursor())
        .map(|(row, col)| (row, col, t.cursor_style()));
    let inputs = CellInputs {
        back,
        live,
        composition: &None,
        highlight: None,
        leaving: Leaving::None,
        pan: Px::of(0),
    };
    let mut copies = cell.copies_mem.each_mut().map(|c| &mut c[..]);
    let mut styles = cell.styles_mem.each_mut().map(|c| &mut c[..]);
    let mut overlay = cell.overlay_mem.each_mut().map(|c| &mut c[..]);
    let damage = r.damage(t, inputs.back);
    build_with(
        r,
        t,
        seal,
        &damage,
        &inputs,
        &mut CellParts {
            map: &mut cell.map,
            exceptions: &mut cell.exceptions,
            screens: &mut cell.screens,
            copies_mem: &mut copies,
            styles_mem: &mut styles,
            overlay_mem: &mut overlay,
            style_stamps: &mut cell.style_stamps,
            lent: &mut cell.lent,
            candidates: &mut cell.candidates,
            records: &mut cell.records,
            grounds: &mut cell.grounds,
            lent_used: &mut cell.lent_used,
        },
        &mut cell.used,
        || Some(SetIx(0)),
    )
    .expect("the set is free")
}

/// **What the frame draws, row by row in screen order**: each cell's
/// content and flags and the rendition its id resolves to in the set's
/// table, read at the cell's slot in the set's ring, and each row's mark.
fn drawn(cell: &Cells<'_>, f: &CellFrame, stride: usize) -> Vec<(Vec<String>, RowExtent)> {
    (0..f.rows.index())
        .map(|d| {
            let at = f.rot.advance(d, f.ring).index();
            let row = &cell.copies_mem[0][at * stride..at * stride + f.cols.index()];
            let cells = row
                .iter()
                .map(|c| {
                    let r = cell.styles_mem[0][c.style.index()];
                    format!("{:?} {:?} {r:?}", c.content, c.flags)
                })
                .collect();
            (cells, cell.used[at])
        })
        .collect()
}

#[test]
fn a_lending_reads_the_copy_in_place() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &flood(0, 30));
    assert!(t.style_reclaims().rewrites > 0, "the control: it froze");
    let stride = t.stride().index();
    let mut r = made(&faces);
    let mut backing = GlyphMap::backing();
    let mut cell = Cells::new(&mut backing, &t);
    cell.screens = Screens::ringed();
    for back in [20usize, 19, 18, 17, 16, 18] {
        t.set_window_back(Rows::of_count(back));
        let _ = counters::take();
        let f = cell_frame(&mut r, &mut t, Rows::of_count(back), &mut cell);
        let copied = counters::take().at(Site::Lent, Counter::CellsCopied);
        assert!(cell.lent[0].lent().value() > 0, "the control: {back} lent");
        assert_eq!(copied.value(), 0, "cells copied to lend {back} back");
        let mut fresh_r = made(&faces);
        let mut fresh_backing = GlyphMap::backing();
        let mut fresh = Cells::new(&mut fresh_backing, &t);
        let g = cell_frame(&mut fresh_r, &mut t, Rows::of_count(back), &mut fresh);
        assert_eq!(
            drawn(&cell, &f, stride),
            drawn(&fresh, &g, stride),
            "the window {back} lines back"
        );
        assert_eq!(cell.records, fresh.records, "records {back} back");
    }
}
