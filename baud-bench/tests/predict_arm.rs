//! **`frame_gpu`'s predict arm holds a run that draws, and the same run
//! each time.** The arm prices the frames holding
//! [`baud_bench::predict`]'s run against the same frames without it; a run
//! that drew nothing would price the plain frame under the held one's name.
//! The CPU path draws no predicted echo, so this builds what the cell path
//! hands the GPU, with no GPU: the glyph map and the overlay walked over a
//! screen of text with a prompt on its last row, then the run drawn over
//! them. The held frame's mask differs from the plain one's at the run's
//! cells and nowhere else, its records are the plain ones and more, the
//! run is the same when built again, and the frame built again without it
//! is the plain frame exactly, the control that says the difference is the
//! run's. The bench checks its own GPU frames when it runs.

use baud_bench::predict;
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Col, Cols, CursorStyle, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Blend, GlyphMap, Instance, MASK_RECORD, Overlay, PxSize, Renderer, Resolved, Scene, Stack,
    Theme,
};

const WS: WinSize = WinSize {
    ws_col: 64,
    ws_row: 20,
};

/// What the cell path hands the GPU for one frame's overlay.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Frame {
    /// One byte a cell, the status row's included.
    mask: Vec<u8>,
    /// What draws the masked cells.
    records: Vec<Instance>,
}

/// **One frame of `t`'s overlay, as the bench builds it**: the map and the
/// overlay walked, with no cursor, then `run` drawn over them.
fn frame(
    r: &mut Renderer<'_, '_>,
    t: &Term<'_>,
    styles: &[Resolved],
    map: &mut GlyphMap<'_>,
    o: &mut Overlay,
    run: &[(Row, Col, Codepoint)],
) -> Frame {
    let mut candidates = Vec::new();
    map.ensure(t, t.viewport(), styles, r, &mut candidates);
    let (cols, rows) = (t.viewport().cols().index(), t.viewport().rows().index());
    let mut mask = vec![0u8; cols * (rows + 1)];
    let mut records = Vec::new();
    o.ensure(
        &Scene {
            term: t,
            view: t.viewport(),
            styles,
            cursor: None,
            highlight: None,
            map,
            candidates: &candidates,
            pan: Px::of(0),
            composition: None,
        },
        r,
        &mut mask,
        &mut records,
    );
    predict::draw(o, r, t, run, styles, None, &mut mask, &mut records);
    Frame { mask, records }
}

#[test]
fn the_held_run_masks_its_cells_and_adds_records_over_the_plain_frame() {
    let face = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the committed face");
    let faces = [Face::parse(&face).expect("a face")];
    let (cols, rows) = (
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    );
    let mut r = Renderer::new(
        Stack::new(&faces).expect("a stack"),
        PxSize::of(14.0).expect("a size"),
        Theme::dark(),
        Blend::Native,
        cols,
        rows,
    )
    .expect("metrics");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut text = Vec::new();
    for i in 0..WS.ws_row - 1 {
        text.extend_from_slice(format!("{i:>4}  drwxr-xr-x  baud  staff  src\r\n").as_bytes());
    }
    // The prompt the run is typed at, on the last row.
    text.extend_from_slice(b"$ ");
    t.write_at(BytePos::ZERO, &text);
    t.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
    let mut styles = vec![Resolved::default(); t.styles().one().len()];
    r.resolve_styles_into(&t, &mut styles);
    let mut backing = GlyphMap::backing();
    let mut map = GlyphMap::over(&mut backing);
    let mut o = Overlay::new();

    // The run: the typed text at the prompt, the same when built again.
    let run = predict::run(cols, rows);
    assert_eq!(run, predict::run(cols, rows), "the same run every rep");
    let last = Row::from_index(rows.index() - 1);
    let typed: Vec<Codepoint> = predict::TYPED
        .chars()
        .filter_map(|c| Codepoint::new(u32::from(c)))
        .collect();
    let at: Vec<(Row, Col)> = (0..typed.len())
        .map(|i| (last, Col::at_index(predict::start().index() + i)))
        .collect();
    assert_eq!(
        run.iter().map(|&(_, _, cp)| cp).collect::<Vec<_>>(),
        typed,
        "the run is the typed text"
    );
    assert_eq!(
        run.iter()
            .map(|&(row, col, _)| (row, col))
            .collect::<Vec<_>>(),
        at,
        "on the last row from the prompt's end"
    );
    assert_eq!(
        t.cursor(),
        (last, predict::start()),
        "the prompt leaves the cursor where the run starts"
    );
    // A screen too narrow for it cuts it at its last column.
    let narrow = Cols::of_index(predict::start().index() + 3);
    assert_eq!(
        predict::run(narrow, rows),
        run[..3].to_vec(),
        "cut at the edge"
    );

    // The drawn cursor sits after the run, in its own style.
    let style = CursorStyle::SteadyBlock;
    let end = run.last().expect("a run").1.after();
    assert_eq!(
        predict::cursor_after(Some((last, predict::start(), style)), &run),
        Some((last, end, style))
    );
    assert_eq!(
        predict::cursor_after(None, &run),
        None,
        "no cursor stays none"
    );
    let kept = Some((Row::from_index(3), Col::at_index(7), style));
    assert_eq!(
        predict::cursor_after(kept, &[]),
        kept,
        "no run moves nothing"
    );

    let plain = frame(&mut r, &t, &styles, &mut map, &mut o, &[]);
    let held = frame(&mut r, &t, &styles, &mut map, &mut o, &run);
    let w = cols.index();
    let run_cell = |i: usize| {
        run.iter()
            .any(|&(row, col, _)| row.index() * w + col.index() == i)
    };
    for (i, (&p, &h)) in plain.mask.iter().zip(&held.mask).enumerate() {
        match run_cell(i) {
            true => {
                assert_eq!(
                    p & MASK_RECORD,
                    0,
                    "cell {i}: the walk left the run's cell to it"
                );
                assert_ne!(h & MASK_RECORD, 0, "cell {i}: the held run masks its cell");
            }
            false => assert_eq!(p, h, "cell {i}: the run masks only its own cells"),
        }
    }
    let added = held.records.len() - plain.records.len();
    eprintln!(
        "{} records plain, {} held: {added} the run's",
        plain.records.len(),
        held.records.len()
    );
    assert!(added > 0, "the held run adds records");
    assert_eq!(
        held.records[..plain.records.len()],
        plain.records[..],
        "after the walk's own"
    );
    assert_eq!(
        frame(&mut r, &t, &styles, &mut map, &mut o, &run),
        held,
        "the same held frame every rep"
    );
    assert_eq!(
        frame(&mut r, &t, &styles, &mut map, &mut o, &[]),
        plain,
        "the control: the run let go draws the plain frame"
    );
}

/// The bench builds its held frames from this module's run and draws it
/// with the module's own loop, the cursor after it.
#[test]
fn the_bench_draws_the_fixed_run_over_its_frames() {
    let code: String = include_str!("../benches/frame_gpu.rs")
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for call in [
        "baud_bench::predict::run(",
        "baud_bench::predict::draw(",
        "baud_bench::predict::cursor_after(",
        "FRAME_PREDICT",
    ] {
        assert!(code.contains(call), "frame_gpu calls `{call}`");
    }
}
