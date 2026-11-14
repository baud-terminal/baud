//! **Every place a caret frame is counted at zero is seen to count**
//! (`src/counters.rs`): `baud-present-macos`'s frame-cost harness
//! (`tests/caret_frame.rs`) asserts that a caret frame compares, copies,
//! lays out and reads nothing at each of several sites, and a zero there
//! means something only while the count at that site is fed. A count
//! removed, or moved to another site, reads zero as well. So each pair
//! of a counter and a site those zeros are asserted at is fed here by a
//! write that must move it, and read moving by the number it should, with
//! the zero beside it on the frame after, nothing written. The pairs fed
//! by the harness's own controls, or here and by `src/screens.rs`'s
//! tests, and the three no source counts at all, are listed in [`FED`]
//! and [`GUARDS`];
//! [`the_sites_a_caret_frame_is_counted_at_zero_are_each_fed_or_count_nothing`]
//! reads them from the sources, and
//! [`every_site_and_counter_the_harness_names_is_fed_guarded_or_named_with_why`]
//! and [`every_check_a_pair_is_fed_by_is_in_its_file`] read the harness, so
//! a zero it asserts later at a pair these lists do not hold fails here.
//! The counters are on in this crate's test build alone
//! ([`the_counters_are_turned_on_in_this_crates_dev_dependencies_alone`]).

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::counters::{self, Counter, Counts, Site};
use baud_render::{
    AppBg, Blend, Damage, PictureBg, PxSize, Renderer, Reorder, Resolved, Stack, StyleStamp,
    Target, Theme,
};
use support::font;

const COLS: usize = 40;
const ROWS: usize = 8;
const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 8,
};

/// The cursor hidden, so no frame here repaints a row for it.
const HIDE: &str = "\x1b[?25l";

/// **A screen with a row drawn in another order and a row of its own**:
/// row 0 a right-to-left paragraph, its Hebrew word first, so it is drawn
/// in another order (UAX #9 P2-P3), and row 3 Latin, a paragraph of its
/// own that no write of row 3 lays any other row out with.
fn screen() -> String {
    format!("{HIDE}\u{5d0}\u{5d1}\u{5d2} abc\x1b[4;1Hplain text")
}

/// One cell written on row 3, which holds no row drawn in another order.
const WRITE: &[u8] = b"\x1b[4;5HX";

/// `bytes` fed to the terminal.
fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// **The app's default page**, dimmed below opaque with pictures drawn
/// solid, so the CPU path finds pictures drawn in text, through the window
/// in drawn order (`Theme::finds_pictures`), as `caret_frame.rs`'s C0
/// page does.
fn dimmed() -> Theme {
    Theme {
        bg_alpha: Chan::from_u8(56),
        app_bg: AppBg::Dimmed,
        picture_bg: PictureBg::Opaque,
        ..Theme::dark()
    }
}

/// A renderer over `faces` for the grid here, on `theme`.
fn made<'f, 'a>(faces: &'f [Face<'a>], theme: Theme) -> Renderer<'f, 'a> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(COLS),
        Rows::of_index(ROWS),
    )
    .unwrap()
}

/// **One CPU frame of the live window onto `px`**, as `damage` says, and
/// what it counted: `render_at` seals the frame's writes at its top.
fn cpu(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut Vec<u32>, damage: Damage) -> Counts {
    r.fit(t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    if px.len() != n {
        *px = vec![0; n];
    }
    let _ = counters::take();
    let mut target = Target::new(&mut px[..], w, w, h).unwrap();
    r.render_at(t, Rows::ZERO, &mut target, damage);
    counters::take()
}

/// `counter`'s count at `site`.
fn at(c: &Counts, site: Site, counter: Counter) -> u64 {
    c.at(site, counter).value()
}

/// **The CPU path's frames over [`screen`]**: a full frame and two
/// incremental ones, then [`WRITE`] and two more, each frame's counts
/// returned with whether row 0 was drawn in another order at it.
fn cpu_frames() -> Vec<(Counts, bool)> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = made(&faces, dimmed());
    assert!(
        dimmed().finds_pictures(),
        "the control: the page finds pictures"
    );
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, screen().as_bytes());
    let mut px = Vec::new();
    let mut out = Vec::new();
    for k in 0..5 {
        if k == 3 {
            let seal = t.seal_writes();
            feed(&mut t, WRITE);
            let gens = t.row_gens(Rows::ZERO);
            assert!(
                (0..ROWS).all(|y| gens.after(Row::from_index(y), &seal) == (y == 3)),
                "the isolation: row 3 written, and no other row"
            );
        }
        let damage = match k {
            0 => Damage::Full,
            _ => Damage::Incremental,
        };
        let c = cpu(&mut r, &mut t, &mut px, damage);
        out.push((c, r.bidi().reordered(Row::ZERO)));
    }
    out
}

/// **A written row is compared at the convert, and no other**
/// (`Site::Convert`'s compare, which every CPU caret frame's zero there
/// stands beside): on [`screen`] the frame after [`WRITE`] takes row 3
/// again into its copy and compares its 40 cells with the last frame's,
/// and no other row's, the one drawn in another order among them; the
/// frame before the write and the frame after it, nothing written,
/// compare none. The control: row 0 is drawn in another order on every
/// frame. Named break: the compare counted at another site.
#[test]
fn a_written_row_is_the_row_compared_at_the_convert() {
    let frames = cpu_frames();
    let cols = u64::try_from(COLS).unwrap();
    for (k, (c, reordered)) in frames.iter().enumerate() {
        assert!(
            *reordered,
            "the control: frame {k} draws row 0 in another order"
        );
        println!(
            "cpu frame {k}: convert compared={}",
            at(c, Site::Convert, Counter::CellsCompared)
        );
    }
    let compared = |k: usize| at(&frames[k].0, Site::Convert, Counter::CellsCompared);
    assert_eq!(compared(2), 0, "the frame before the write compares none");
    assert_eq!(compared(3), cols, "the written row compared, and no other");
    assert_eq!(compared(4), 0, "the frame after compares none");
}

/// **A written row is copied again in drawn order, and no other**
/// (`Site::Layout`'s copy, which every caret frame's zero there stands
/// beside): on [`screen`], whose row 0 is drawn in another order, the CPU
/// path's copy of the window in drawn order -- read on the default page
/// to find pictures drawn in text -- copies the whole window on the first
/// frame, and on the frame after [`WRITE`] row 3's 40 cells alone, since
/// no other row was written or laid out again; the frame before the write
/// and the frame after it copy none. Named break: the copy counted at
/// another site.
#[test]
fn a_written_row_is_the_row_copied_again_in_drawn_order() {
    let frames = cpu_frames();
    let cols = u64::try_from(COLS).unwrap();
    let rows = u64::try_from(ROWS).unwrap();
    let copied = |k: usize| at(&frames[k].0, Site::Layout, Counter::CellsCopied);
    for (k, (c, _)) in frames.iter().enumerate() {
        println!(
            "cpu frame {k}: layout copied={}",
            at(c, Site::Layout, Counter::CellsCopied)
        );
    }
    assert_eq!(copied(0), rows * cols, "the first frame copies the window");
    assert_eq!(copied(2), 0, "the frame before the write copies none");
    assert_eq!(copied(3), cols, "the written row copied, and no other");
    assert_eq!(copied(4), 0, "the frame after copies none");
}

/// **A layout after a write reads the write generations it counts**
/// (`Site::Layout`'s `GensRead`, which every caret frame's zero there
/// stands beside): [`screen`] laid out by a layout of its own
/// (`Reorder::lay_out`), sealed first as a driver seals; laid out again
/// with nothing written it reads no generation, and after [`WRITE`] it
/// reads the generations its own tally of them says it read
/// (`Reorder::work`), some, and the count at the layout is that number;
/// the layout after, nothing written, reads none. Named break: the read
/// counted at another site.
#[test]
fn a_layout_after_a_write_counts_the_generations_it_read() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, screen().as_bytes());
    let mut o = Reorder::new();
    let mut lay_out = |t: &mut Term<'_>| {
        let _ = t.seal_writes();
        let _ = counters::take();
        o.lay_out(t, Rows::ZERO);
        let c = counters::take();
        (
            at(&c, Site::Layout, Counter::GensRead),
            o.work().gens.value(),
            o.orders().reordered(Row::ZERO),
        )
    };
    let first = lay_out(&mut t);
    assert!(first.2, "the control: row 0 is drawn in another order");
    assert_eq!(lay_out(&mut t), (0, 0, true), "nothing written reads none");
    feed(&mut t, WRITE);
    let (counted, read, reordered) = lay_out(&mut t);
    println!("layout after the write: counted={counted} read={read}");
    assert!(reordered, "the control: row 0 is still drawn otherwise");
    assert!(read > 0, "the control: the layout read generations");
    assert_eq!(counted, read, "the count at the layout is what it read");
    assert_eq!(lay_out(&mut t), (0, 0, true), "the layout after reads none");
}

/// **A sized line kept by its cells is compared at the sized lines**
/// (`Site::Sized`'s compare, which every cell caret frame's zero there
/// stands beside): row 1 double width, the cell path's walk of the sized
/// lines made over the window with no build's seal to key it by
/// (`Renderer::sized_lines`), so the line is keyed by its 40 cells as
/// drawn. The first walk has no key to compare; the second is printed and
/// not asserted, since its key also holds the atlas's growth, which the
/// first walk's conversion may have moved; the third and the fourth each
/// compare the line's 40 cells. The control: the walk lists row 1 alone.
/// Named break: the compare counted at another site.
#[test]
fn a_sized_line_kept_by_its_cells_is_compared_at_the_sized_lines() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = made(&faces, Theme::dark());
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, format!("{HIDE}\x1b[2;1H\x1b#6wide line").as_bytes());
    let mut table = vec![Resolved::default(); t.styles().len()];
    r.sync_styles(&t, &mut table, &mut StyleStamp::default());
    let cols = u64::try_from(COLS).unwrap();
    let mut rows = Vec::new();
    let mut compared = Vec::new();
    for _ in 0..4 {
        let _ = counters::take();
        r.sized_lines(&t, (&t.viewport(), &table), None, None, &mut rows);
        let c = counters::take();
        assert_eq!(rows, [Row::from_index(1)], "the control: row 1 is sized");
        compared.push(at(&c, Site::Sized, Counter::CellsCompared));
    }
    println!("sized walks: compared={compared:?}");
    assert_eq!(compared[0], 0, "the first walk has nothing to compare");
    assert_eq!(
        compared[2..],
        [cols, cols],
        "the third and fourth walks compare the line's cells"
    );
}

/// Each `.rs` file under `dir`.
fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a source directory") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The parenthesised argument list opening at `open` in `flat`.
fn arguments(flat: &str, open: usize) -> &str {
    let mut depth = 0;
    for (i, ch) in flat[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &flat[open..=open + i];
                }
            }
            _ => {}
        }
    }
    &flat[open..]
}

/// `src`'s code lines, flat: the comment lines left out, and every space.
fn flat(src: &str) -> String {
    src.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .flat_map(str::split_whitespace)
        .collect()
}

/// The identifier `rest` starts with, empty when it starts with none.
fn ident(rest: &str) -> &str {
    let n = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    &rest[..n]
}

/// Whether the text before `at` in `s` ends inside an identifier, so a
/// `Site::` there is the tail of another name.
fn mid_name(s: &str, at: usize) -> bool {
    s[..at]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The name after the first `path` in `args`.
fn named<'s>(args: &'s str, path: &str) -> Option<&'s str> {
    let at = args.find(path)? + path.len();
    let rest = &args[at..];
    let n = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    (n > 0).then(|| &rest[..n])
}

/// **Each pair of a counter and a site that `src` counts**, as `(counter,
/// site)` named as the source names them, and how many calls were read:
/// every `counters::add` by its two arguments, and every compare spelled
/// through `counters::same_cells` or `counters::compared` as
/// `CellsCompared` at its site. Comment lines are left out, and the
/// counters' own module, which defines the calls.
fn counted(src: &str, into: &mut std::collections::BTreeSet<(String, String)>) -> usize {
    let flat = flat(src);
    let mut calls = 0;
    for (call, compare) in [
        ("counters::add(", false),
        ("add(Counter::", false),
        ("same_cells(Site::", true),
        ("counters::same_cells(", true),
        ("compared(Site::", true),
        ("counters::compared(", true),
    ] {
        for (at, _) in flat.match_indices(call) {
            let open = at + call.find('(').expect("a call");
            let args = arguments(&flat, open);
            let counter = match compare {
                true => Some("CellsCompared"),
                false => named(args, "Counter::"),
            };
            if let (Some(counter), Some(site)) = (counter, named(args, "Site::")) {
                calls += 1;
                into.insert((counter.to_string(), site.to_string()));
            }
        }
    }
    calls
}

/// **Every site a caret frame is counted at zero is fed, or counts
/// nothing**, read from `src`'s calls ([`counted`]): the pairs
/// `caret_frame.rs` asserts zero on caret frames are each counted
/// somewhere, and each is fed by a check that reads it moving ([`FED`],
/// with the check), and the three it asserts zero that no source counts
/// ([`GUARDS`]) are counted nowhere, so their zeros are guards that a
/// count does not come back there. The two pairs fed by a control on
/// their counter's total ([`TOTALS`]) are each counted at their one site.
/// The controls:
/// the reader finds at least as many calls as there were when this was
/// written, and every fed pair.
#[test]
fn the_sites_a_caret_frame_is_counted_at_zero_are_each_fed_or_count_nothing() {
    let mut files = Vec::new();
    sources(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut pairs = std::collections::BTreeSet::new();
    let mut calls = 0;
    for f in files.iter().filter(|f| !f.ends_with("counters.rs")) {
        calls += counted(&std::fs::read_to_string(f).expect("a source"), &mut pairs);
    }
    assert!(
        calls >= 50,
        "the counter calls are read: {calls}, {pairs:?}"
    );
    for (counter, site, by) in FED {
        assert!(
            pairs.contains(&(counter.to_string(), site.to_string())),
            "{counter} at {site}, fed by {by}, is counted in the sources: {pairs:?}"
        );
    }
    for (counter, site) in GUARDS {
        assert!(
            !pairs.contains(&(counter.to_string(), site.to_string())),
            "{counter} at {site} is asserted zero as a guard and counted in the sources: \
             it needs a check that feeds it"
        );
    }
    for (counter, site) in TOTALS {
        let sites: Vec<&str> = pairs
            .iter()
            .filter(|(c, _)| c == counter)
            .map(|(_, s)| s.as_str())
            .collect();
        assert_eq!(
            sites,
            [site],
            "{counter} is fed by a control on its total, so it is counted at {site} alone"
        );
    }
}

/// **Each pair of a counter and a site a caret frame is asserted zero at,
/// and what feeds it**: a check here, one of `src/screens.rs`'s, or one of
/// the harness's (`caret_frame's <check>`), with which of its counts.
const FED: [(&str, &str, &str); 14] = [
    (
        "CellsCompared",
        "Screens",
        "screens::tests::a_row_laid_out_again_unwritten_is_the_row_compared_at_the_copy, \
             screens::tests::a_row_a_lending_rewrote_is_compared_again_through_its_undo",
    ),
    (
        "CellsCopied",
        "Screens",
        "caret_frame's a_caret_frame_drawn_as_stored_copies_and_compares_no_cell, its control",
    ),
    (
        "CellsCompared",
        "Convert",
        "a_written_row_is_the_row_compared_at_the_convert",
    ),
    (
        "CellsCopied",
        "Convert",
        "caret_frame's a_caret_frame_drawn_as_stored_copies_and_compares_no_cell, its control",
    ),
    (
        "CellsCompared",
        "Sized",
        "a_sized_line_kept_by_its_cells_is_compared_at_the_sized_lines",
    ),
    (
        "CellsCopied",
        "Sized",
        "caret_frame's a_caret_frame_drawn_as_stored_copies_and_compares_no_cell, its control",
    ),
    (
        "DisplaysLaid",
        "Rebreak",
        "caret_frame's a_caret_frame_over_a_re_broken_display_lays_out_and_reads_nothing_at_the_re_cut, \
             its control on the first frame",
    ),
    (
        "GensRead",
        "Rebreak",
        "caret_frame's a_write_outside_the_display_reads_its_rows_once",
    ),
    (
        "BidiRowsLaid",
        "Layout",
        "caret_frame's every_counter_moves_on_a_full_frame, on its one site",
    ),
    (
        "GensRead",
        "Layout",
        "a_layout_after_a_write_counts_the_generations_it_read",
    ),
    (
        "CellsCopied",
        "Layout",
        "a_written_row_is_the_row_copied_again_in_drawn_order",
    ),
    (
        "CellsCopied",
        "Lent",
        "caret_frame's a_struck_link_lent_a_slot_counts_what_its_caret_frames_cost, its control",
    ),
    (
        "LinkInstancesBuilt",
        "Links",
        "caret_frame's a_caret_frame_places_no_link_record, its control on the first frames",
    ),
    (
        "InstancesTallied",
        "Totals",
        "caret_frame's a_caret_frame_tallies_the_rows_it_converts, against the first frame",
    ),
];

/// **The pairs a caret frame is asserted zero at that no source counts**:
/// guards that a count does not come back there, not checks a frame can
/// fail. The re-cut compares and copies no cell (it keeps a display by
/// the rows' generations, `GensRead`), and the bidirectional layout
/// compares none (it keeps a paragraph the same way).
const GUARDS: [(&str, &str); 3] = [
    ("CellsCompared", "Rebreak"),
    ("CellsCopied", "Rebreak"),
    ("CellsCompared", "Layout"),
];

/// **The pairs of [`FED`] whose control reads their counter's total** over
/// every site -- the rows laid out and the instances tallied -- which feeds
/// the pair only while the counter is counted at that site alone.
const TOTALS: [(&str, &str); 2] = [("BidiRowsLaid", "Layout"), ("InstancesTallied", "Totals")];

/// **Named in the harness, and in no pair a caret frame is asserted zero
/// at**, each with why it is named there.
const NAMED: [(&str, &str); 3] = [
    (
        "Site::ALL",
        "every site, walked to print what each compared and copied",
    ),
    (
        "Counter::ALL",
        "every counter, walked to print each and, on a full frame, to read each moving",
    ),
    (
        "Counter::OverlayCellsBuilt",
        "the overlay's cells: read moving on the cell path's first frames, and zero on the \
         CPU path's full frame, which builds no overlay; no caret frame is asserted zero at \
         it until FS-C1, which adds its pair to `FED` and takes it out of this list",
    ),
];

/// The harness whose zeros these pairs stand beside, under the workspace.
const HARNESS: &str = "baud-present-macos/tests/caret_frame.rs";

/// `p` under the workspace.
fn workspace(p: &str) -> String {
    std::fs::read_to_string(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// **Every `Site::X` and `Counter::Y` that `src` names**, its comments
/// included.
fn names(src: &str) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for path in ["Site::", "Counter::"] {
        for (at, _) in src.match_indices(path) {
            let name = ident(&src[at + path.len()..]);
            if !name.is_empty() && !mid_name(src, at) {
                out.insert(format!("{path}{name}"));
            }
        }
    }
    out
}

/// **Every pair that `src`'s code spells together**, `Site::X, Counter::Y`,
/// as `(counter, site)`.
fn spelled(src: &str) -> Vec<(String, String)> {
    let flat = flat(src);
    let mut out = Vec::new();
    for (at, _) in flat.match_indices("Site::") {
        let site = ident(&flat[at + "Site::".len()..]);
        let rest = &flat[at + "Site::".len() + site.len()..];
        let counter = rest
            .strip_prefix(",Counter::")
            .map(ident)
            .unwrap_or_default();
        if !site.is_empty() && !counter.is_empty() {
            out.push((counter.to_string(), site.to_string()));
        }
    }
    out
}

/// **What `harness` names that no list here holds**, sorted: each pair it
/// spells together ([`spelled`]) that is in neither [`FED`] nor
/// [`GUARDS`], and each site and counter it names ([`names`]) that is in
/// no pair of either and not in `named`.
fn unclassified(harness: &str, named: &[(&str, &str)]) -> Vec<String> {
    let pairs: Vec<(&str, &str)> = FED.iter().map(|&(c, s, _)| (c, s)).chain(GUARDS).collect();
    let mut held: std::collections::BTreeSet<String> = pairs
        .iter()
        .flat_map(|&(c, s)| [format!("Counter::{c}"), format!("Site::{s}")])
        .collect();
    held.extend(named.iter().map(|&(n, _)| n.to_string()));
    let mut wrong = std::collections::BTreeSet::new();
    for (counter, site) in spelled(harness) {
        if !pairs.iter().any(|&(c, s)| c == counter && s == site) {
            wrong.insert(format!("pair Site::{site}, Counter::{counter}"));
        }
    }
    for name in names(harness) {
        if !held.contains(&name) {
            wrong.insert(name);
        }
    }
    wrong.into_iter().collect()
}

/// **The checks `fed` names that are not where it says**, and how many
/// names it read: `caret_frame's <check>` a function of `harness` and one
/// of the names it lists to run (it runs without the test harness, so a
/// check it does not list never runs), `screens::tests::<check>` a
/// function of `screens`, and a bare name a function of `own`; the words
/// around them (`its control`) are no name.
fn uncited(
    fed: &[(&str, &str, &str)],
    harness: &str,
    screens: &str,
    own: &str,
) -> (usize, Vec<String>) {
    let mut read = 0;
    let mut wrong = Vec::new();
    for &(counter, site, by) in fed {
        for part in by.split(',').map(str::trim) {
            let (file, text, name) = if let Some(rest) = part.strip_prefix("caret_frame's ") {
                ("caret_frame.rs", harness, ident(rest))
            } else if let Some(rest) = part.strip_prefix("screens::tests::") {
                ("src/screens.rs", screens, ident(rest))
            } else if !part.is_empty() && ident(part) == part {
                ("this file", own, part)
            } else {
                continue;
            };
            read += 1;
            let runs = file != "caret_frame.rs" || text.contains(&format!("\"{name}\""));
            if name.is_empty() || !text.contains(&format!("fn {name}(")) || !runs {
                wrong.push(format!("{counter} at {site}: {name} is no check in {file}"));
            }
        }
    }
    (read, wrong)
}

/// **Every site and counter the harness names is fed, guarded, or named
/// with why** (`caret_frame.rs`, read), so a zero it asserts later at a
/// pair that [`FED`] and [`GUARDS`] do not hold fails here. Read by pair
/// where the harness spells one together (`Site::X, Counter::Y`), and by
/// name everywhere else, since it also holds sites and counters in
/// variables -- a loop over three sites, a list of counters asked of one
/// site through a helper -- whose pairs its text does not spell: a name
/// in no pair of either list and not in [`NAMED`] fails. The controls: the
/// reader finds at least the names and the pairs there were when this was
/// written, each name in [`NAMED`] is still in the harness, and the
/// harness with a pair of two unlisted names and a pair of two listed
/// ones spliced in is read with each.
#[test]
fn every_site_and_counter_the_harness_names_is_fed_guarded_or_named_with_why() {
    let harness = workspace(HARNESS);
    let found = names(&harness);
    assert!(
        found.len() >= 17 && found.contains("Site::Screens") && found.contains("Counter::GensRead"),
        "the control: the harness's names are read: {found:?}"
    );
    let together = spelled(&harness);
    assert!(
        together.len() >= 7,
        "the control: the pairs it spells together are read: {together:?}"
    );
    for (name, why) in NAMED {
        assert!(
            found.contains(name),
            "{name} ({why}) is named in the harness"
        );
    }
    assert_eq!(
        unclassified(&harness, &NAMED),
        Vec::<String>::new(),
        "named in {HARNESS} and held in no fed or guard pair, nor named with why"
    );
    let spliced = format!(
        "{harness}\nfn spliced(c: &Counts) {{\n    let _ = (Site::Spliced, Counter::Spliced);\n    \
         let _ = c.at(Site::Screens, Counter::DisplaysLaid);\n}}\n"
    );
    assert_eq!(
        unclassified(&spliced, &NAMED),
        [
            "Counter::Spliced",
            "Site::Spliced",
            "pair Site::Screens, Counter::DisplaysLaid",
            "pair Site::Spliced, Counter::Spliced",
        ],
        "the control: what is spliced in is read"
    );
}

/// **Every check a pair is fed by is in its file** ([`FED`], read by
/// [`uncited`]), so a check renamed or taken out of the harness leaves no
/// pair citing it. The controls: the reader reads at least the names there
/// were when this was written, and a list with a name misspelled for each
/// file is read with each.
#[test]
fn every_check_a_pair_is_fed_by_is_in_its_file() {
    let harness = workspace(HARNESS);
    let screens = workspace("baud-render/src/screens.rs");
    let own = workspace("baud-render/tests/fed_counters.rs");
    let (read, wrong) = uncited(&FED, &harness, &screens, &own);
    assert_eq!(wrong, Vec::<String>::new(), "a pair is fed by no check");
    assert!(read >= 15, "the control: the checks are read: {read}");
    let mut misspelled = FED;
    misspelled[0].2 =
        "screens::tests::a_row_laid_out_again_unwritten_is_the_row_compared_at_the_cop";
    misspelled[1].2 = "caret_frame's a_caret_frame_drawn_as_stored_copies_and_compares_no_cel";
    misspelled[2].2 = "a_written_row_is_the_row_compared_at_the_convrt";
    assert_eq!(
        uncited(&misspelled, &harness, &screens, &own).1,
        [
            "CellsCompared at Screens: a_row_laid_out_again_unwritten_is_the_row_compared_at_the_cop \
             is no check in src/screens.rs",
            "CellsCopied at Screens: a_caret_frame_drawn_as_stored_copies_and_compares_no_cel \
             is no check in caret_frame.rs",
            "CellsCompared at Convert: a_written_row_is_the_row_compared_at_the_convrt \
             is no check in this file",
        ],
        "the control: each misspelling is read"
    );
}

/// **Which lines of a manifest name the counters**, as `(section, key)`:
/// each line of code naming `counters`, its comment left out.
fn turned_on(toml: &str) -> Vec<(String, String)> {
    let mut section = String::new();
    let mut on = Vec::new();
    for line in toml.lines() {
        let code = line.split('#').next().unwrap_or_default().trim();
        if code.starts_with('[') {
            section = code.to_string();
        } else if code.contains("counters") {
            let key = code.split('=').next().unwrap_or_default().trim();
            on.push((section.clone(), key.to_string()));
        }
    }
    on
}

/// **The counters are on in this crate's test build alone**
/// (`Cargo.toml`): `counters` is named by the feature's own definition and
/// by this crate's tests' dependency on itself, without its defaults, and
/// by no other line -- not `default`, and not a feature that turns it on
/// with itself -- so nothing that ships carries a store on a per-cell path.
/// The controls: the manifest with `counters` added to `default`, and
/// added to `neon-scan`, which `default` turns on, is read with each.
#[test]
fn the_counters_are_turned_on_in_this_crates_dev_dependencies_alone() {
    let toml = workspace("baud-render/Cargo.toml");
    let line = |s: &str, k: &str| (s.to_string(), k.to_string());
    assert_eq!(
        turned_on(&toml),
        [
            line("[dev-dependencies]", "baud-render"),
            line("[features]", "counters")
        ],
        "the counters are named by their definition and this crate's tests alone"
    );
    assert!(
        toml.lines().any(|l| l.starts_with("baud-render = ")
            && l.contains("default-features = false")
            && l.contains("\"counters\"")),
        "this crate's tests take it without its defaults, with the counters"
    );
    for (from, to, key) in [
        (
            "default = [\"neon-scan\"]",
            "default = [\"neon-scan\", \"counters\"]",
            "default",
        ),
        (
            "neon-scan = [\"baud-core/neon-scan\"]",
            "neon-scan = [\"baud-core/neon-scan\", \"counters\"]",
            "neon-scan",
        ),
    ] {
        assert!(
            toml.contains(from),
            "the control: {from} is in the manifest"
        );
        let on = turned_on(&toml.replace(from, to));
        assert!(
            on.contains(&line("[features]", key)),
            "the control: {key} turning the counters on is read: {on:?}"
        );
    }
}
