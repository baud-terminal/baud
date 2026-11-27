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
    // BDSM reset, so the row is shaped in the order its cells hold: the
    // subject is where a script run is cut, which a right-to-left run
    // shaped a cell at a time (UAX #9 3.5) would hide.
    let bytes = format!("\x1b[8l{text}");
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
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

/// **A Hebrew point draws in its letter's cell on Arial Unicode MS and
/// Arial Hebrew, which put it elsewhere** (Courier New's
/// points stayed in their cell, and the shaping fixtures hold where in
/// it they sit). Arial Unicode MS's `GPOS`
/// selects no Script table for Hebrew, so its points kept their advances
/// and drew a cell or two right; they are placed by the fallback, and bet
/// with dagesh draws the face's presentation form. Arial Hebrew carries
/// `GSUB` and `morx`, and its `GSUB` drew meteg and the cantillation marks
/// as one empty glyph; it shapes through `morx`, here behind JetBrains
/// Mono, since it maps no Latin. A Latin letter leads each
/// row, so the Hebrew is shaped as Hebrew only if the run is cut where the
/// placement changes -- on Arial Unicode MS, Latin and Hebrew select the
/// same (no) lookups. macOS system faces.
#[test]
fn a_hebrew_point_draws_in_its_letters_cell_on_arial_unicode_ms_and_arial_hebrew() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: Arial Unicode MS and Arial Hebrew are macOS system faces");
        return;
    }
    let unicode = std::fs::read("/System/Library/Fonts/Supplemental/Arial Unicode.ttf").unwrap();
    let faces = [Face::parse(&unicode).unwrap()];
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
            "Arial Unicode MS {pointed:?}: the cells the points changed"
        );
    }
    // Arial Hebrew maps no Latin, so it follows JetBrains Mono, as a
    // system face follows the configured one.
    let jbm = font("JetBrainsMono-Regular.ttf");
    let hebrew = std::fs::read("/System/Library/Fonts/ArialHB.ttc").unwrap();
    let faces = [Face::parse(&jbm).unwrap(), Face::parse(&hebrew).unwrap()];
    // Meteg draws inside its letter's cell. Etnahta's glyph is centred on
    // its origin, which no lookup of the face moves and the pen leaves on
    // the letter's left edge, so it straddles that edge -- where hb-shape
    // puts it too. Before, neither drew at all.
    for (bare, pointed, cells) in [
        ("aב\u{5B8}", "aב\u{5B8}\u{5BD}", vec![1]),
        ("aב", "aב\u{591}", vec![0, 1]),
    ] {
        let (b, cell_w, _) = painted(&faces, bare);
        let (p, _, _) = painted(&faces, pointed);
        assert_eq!(
            changed_cells(&b, &p, cell_w),
            cells,
            "Arial Hebrew {pointed:?}: the cells the marks changed"
        );
    }
}

/// How the built Regular's `GPOS` is edited in memory: renamed out of the
/// table directory, or kept with every Script table renamed past any
/// script's tag.
#[derive(Clone, Copy)]
enum Gpos {
    Gone,
    Unselected,
}

/// The built Regular with its `GPOS` edited as `how` says.
fn regular_with(how: Gpos) -> Vec<u8> {
    let mut d = app_font("built/Regular.ttf");
    let be16 = |d: &[u8], at: usize| usize::from(u16::from_be_bytes([d[at], d[at + 1]]));
    let entry = (0..be16(&d, 4))
        .map(|i| 12 + 16 * i)
        .find(|&at| &d[at..at + 4] == b"GPOS")
        .expect("the built Regular has a GPOS");
    match how {
        Gpos::Gone => d[entry..entry + 4].copy_from_slice(b"GPOZ"),
        Gpos::Unselected => {
            let at = usize::try_from(u32::from_be_bytes([
                d[entry + 8],
                d[entry + 9],
                d[entry + 10],
                d[entry + 11],
            ]))
            .unwrap();
            let list = at + be16(&d, at + 4);
            for (k, i) in (0..be16(&d, list)).enumerate() {
                let rec = list + 2 + 6 * i;
                d[rec..rec + 4].copy_from_slice(&[
                    b'z',
                    b'z',
                    b'z',
                    b'0' + u8::try_from(k).unwrap(),
                ]);
            }
        }
    }
    d
}

/// **A face whose `GPOS` selects no Script table for Hebrew draws Hebrew as
/// a face with no `GPOS` does** -- the points placed by the fallback --
/// though Latin and Hebrew select the same (no) `GPOS` lookups and the
/// same `GSUB` ones there. So the renderer must count the placement among
/// what a script changes, or it shapes such a face's every run as Common
/// and leaves the points where the pen put them. The built Regular, its
/// `GPOS` kept with every Script table renamed, against the same face with
/// its `GPOS` gone, pixel for pixel, on every platform.
#[test]
fn a_face_whose_gpos_selects_nothing_for_hebrew_draws_it_as_one_with_no_gpos() {
    let unselected = regular_with(Gpos::Unselected);
    let gone = regular_with(Gpos::Gone);
    let unselected = [Face::parse(&unselected).unwrap()];
    let gone = [Face::parse(&gone).unwrap()];
    for text in ["aב\u{5B8}", "aש\u{5B8}\u{5C1}לו\u{5B9}ם", "ab"] {
        assert_eq!(
            painted(&unselected, text).0,
            painted(&gone, text).0,
            "{text:?}: drawn as with no GPOS"
        );
    }
    // The control: with its GPOS kept whole, the face places the qamats
    // by its anchors, elsewhere than the fallback does.
    let kept = app_font("built/Regular.ttf");
    let kept = [Face::parse(&kept).unwrap()];
    assert_ne!(painted(&kept, "aב\u{5B8}").0, painted(&gone, "aב\u{5B8}").0);
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

/// **A bracket pair is in the run of the text enclosing it** (UAX #24
/// 5.1, "paired punctuation ... should therefore match the script of the
/// enclosing text"), so a run is cut on both sides of what the pair
/// holds. On STIX Two Math, whose `latn` and `grek` select different
/// lookups, `a(α)` is three runs -- `a(`, `α`, `)` -- where the
/// preceding-character rule alone made it two and shaped the closer
/// Greek.
#[test]
fn a_bracket_pair_is_in_the_enclosing_run() {
    let stix = app_font("stix-two-math/STIXTwoMath-Regular.otf");
    let stix = [Face::parse(&stix).unwrap()];
    assert_eq!(painted(&stix, "a(α)b").2, 3, "the control: a(, α, )b");
    assert_eq!(painted(&stix, "a(α)").2, 3, "a(, α, )");
    assert_eq!(painted(&stix, "α(a)").2, 3, "α(, a, )");
    assert_eq!(painted(&stix, "a«α»").2, 2, "a quotation mark is no pair");
}

/// **And inside Hebrew** on Courier New, whose `hebr` and `latn` select
/// different lookups: `א(a)` is `א(`, `a`, `)`. A macOS system face.
#[test]
fn a_bracket_pair_inside_hebrew_is_in_the_hebrew_run() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: Courier New is a macOS system face");
        return;
    }
    let data = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf").unwrap();
    let faces = [Face::parse(&data).unwrap()];
    assert_eq!(painted(&faces, "א(a)ב").2, 3, "the control");
    assert_eq!(painted(&faces, "א(a)").2, 3, "א(, a, )");
}
