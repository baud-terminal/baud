//! A row is shaped in runs of one script's lookups: text in a script the
//! face has a Script table for is shaped with that table, else `DFLT`
//! (`[[source: ms-opentype-chapter2]]`, ScriptList table), and a run is cut
//! where the lookups the settled script selects change.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};
use support::*;

const ROW: WinSize = WinSize {
    ws_row: 1,
    ws_col: 16,
};

fn app_font(path: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../baud-app/fonts/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A renderer for one 16-cell row on `faces`.
fn one_row<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(PX).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ROW.ws_col)),
        Rows::of_index(usize::from(ROW.ws_row)),
    )
    .unwrap()
}

/// `text` painted on a fresh one-row renderer: the pixels, the cell width,
/// and how many runs the row was shaped in (every run misses a fresh cache).
fn painted(faces: &[Face<'_>], text: &str) -> (Vec<u32>, usize, u64) {
    let mut r = one_row(faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&ROW);
    let mut t = Term::new(owned.storage(), &ROW);
    t.write_at(BytePos::ZERO, text.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
    paint(&mut r, &mut t, &mut c, Damage::Full);
    let cell_w = usize::try_from(r.metrics().cell_w.value()).unwrap();
    (c.pixels, cell_w, r.cache_stats().misses.value())
}

/// **A run is cut where the script's lookups change, and only there.**
/// STIX Two Math names different `GSUB` lookups under `latn` and `grek`,
/// so Latin beside Greek is two runs, and the hyphen between them goes
/// with the script before it (UAX #24 5.1); JetBrains Mono names the same
/// lookups under every script, so the same text is one run there.
#[test]
fn a_run_is_cut_where_the_scripts_lookups_change() {
    let stix = app_font("stix-two-math/STIXTwoMath-Regular.otf");
    let jbm = font("JetBrainsMono-Regular.ttf");
    let stix = [Face::parse(&stix).unwrap()];
    let jbm = [Face::parse(&jbm).unwrap()];
    assert_eq!(
        painted(&stix, "ab").2,
        1,
        "the control: one script, one run"
    );
    assert_eq!(
        painted(&stix, "a-b").2,
        1,
        "a hyphen between Latin is Latin"
    );
    assert_eq!(painted(&stix, "aα").2, 2, "Latin, then Greek");
    assert_eq!(painted(&stix, "α-a").2, 2, "the hyphen goes with the Greek");
    assert_eq!(painted(&jbm, "aα").2, 1, "one lookup set, one run");
}

/// Which cells of a one-row frame differ between two paintings.
fn changed_cells(a: &[u32], b: &[u32], cell_w: usize) -> Vec<usize> {
    let width = cell_w * usize::from(ROW.ws_col);
    let mut cells: Vec<usize> = a
        .iter()
        .zip(b)
        .enumerate()
        .filter(|(_, (p, q))| p != q)
        .map(|(i, _)| (i % width) / cell_w)
        .collect();
    cells.sort_unstable();
    cells.dedup();
    cells
}

/// **A Hebrew point draws in its letter's cell** -- the fault found on
/// Courier New, which macOS falls back to for Hebrew (note AA, section 6):
/// its `latn` table names no features, so a point shaped `latn` was left at
/// the pen one cell right of its letter, and its `hebr` table attaches
/// it. A Latin letter before the word keeps the row's first run Latin, so
/// the Hebrew is shaped `hebr` only if the run is cut where the script
/// changes. A macOS system face.
#[test]
fn a_hebrew_point_draws_in_its_letters_cell() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: Courier New is a macOS system face");
        return;
    }
    let data = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf").unwrap();
    let faces = [Face::parse(&data).unwrap()];
    // bet, then bet with qamats; then shalom bare and with qamats on the
    // shin and holam on the vav, one point a letter.
    for (bare, pointed, cells) in [
        ("aב", "aב\u{5B8}", vec![1]),
        ("aב", "aב\u{5BC}", vec![1]),
        ("aשלום", "aש\u{5B8}לו\u{5B9}ם", vec![1, 3]),
    ] {
        let (b, cell_w, _) = painted(&faces, bare);
        let (p, _, _) = painted(&faces, pointed);
        assert_eq!(
            changed_cells(&b, &p, cell_w),
            cells,
            "{pointed:?}: the cells the points changed"
        );
    }
}

/// **A run's cache key carries the lookups its script selects**, because a
/// run of Common characters takes its script from the cells around it,
/// which the key does not fold. On STIX Two Math, `--` after a box-drawing
/// cell -- drawn by baud, never in a run -- is a run of its own: Greek
/// after Greek and Latin after Latin. The same two code points, two
/// selections, two shapings: four runs miss a fresh cache, not three.
#[test]
fn a_runs_cache_key_carries_its_selection() {
    const TWO: WinSize = WinSize {
        ws_row: 2,
        ws_col: 16,
    };
    let stix = app_font("stix-two-math/STIXTwoMath-Regular.otf");
    let faces = [Face::parse(&stix).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(PX).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(TWO.ws_col)),
        Rows::of_index(usize::from(TWO.ws_row)),
    )
    .unwrap();
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&TWO);
    let mut t = Term::new(owned.storage(), &TWO);
    let text = "α\u{2500}--\r\na\u{2500}--";
    t.write_at(BytePos::ZERO, text.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
    paint(&mut r, &mut t, &mut c, Damage::Full);
    let s = r.cache_stats();
    assert_eq!(
        (s.misses.value(), s.hits.value()),
        (4, 0),
        "α, Greek --, a, Latin --"
    );
}

/// **The cell path's windows are cut the same way.** A window on Courier
/// New holding a Latin letter and then bet with qamats is shaped in two
/// pieces, so the point lands on the bet, within its own cell; shaped as
/// one Latin window it sat a whole cell right. A macOS system face.
#[test]
fn a_window_is_cut_where_the_scripts_lookups_change() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: Courier New is a macOS system face");
        return;
    }
    let data = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf").unwrap();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = one_row(&faces);
    let cell_w = i32::try_from(r.metrics().cell_w.value()).unwrap();
    let cp = baud_domain::Codepoint::from_char;
    let cells = [
        (vec![cp('a')], baud_render::CellSpan::One),
        (vec![cp('ב'), cp('\u{5B8}')], baud_render::CellSpan::One),
    ];
    let shaped = r.window(baud_render::Code(0), &cells);
    assert_eq!(shaped[1].len(), 2, "bet and its qamats: {:?}", shaped[1]);
    for g in &shaped[1] {
        assert!(
            g.dx.abs() < cell_w / 2,
            "a glyph of the bet's cell is offset {} px, in a {cell_w}-px cell",
            g.dx
        );
    }
}

/// **A marked cell drawn by itself is shaped in its own script**, which
/// is how the cell path draws every pointed letter (`Renderer::cluster`,
/// through the overlay): bet with qamats on Courier New keeps the point in
/// the bet's cell. A macOS system face.
#[test]
fn a_marked_cell_alone_is_shaped_in_its_script() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: Courier New is a macOS system face");
        return;
    }
    let data = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf").unwrap();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = one_row(&faces);
    let cell_w = i32::try_from(r.metrics().cell_w.value()).unwrap();
    let cp = baud_domain::Codepoint::from_char;
    let glyphs = r.cluster(
        &[cp('ב'), cp('\u{5B8}')],
        baud_render::FaceStyle::Regular,
        baud_render::CellSpan::One,
    );
    assert_eq!(glyphs.len(), 2, "bet and its qamats: {glyphs:?}");
    for g in &glyphs {
        assert!(
            g.dx.abs() < cell_w / 2,
            "a glyph of the cell is offset {} px, in a {cell_w}-px cell",
            g.dx
        );
    }
}
