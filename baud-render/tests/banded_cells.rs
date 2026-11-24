//! **A banded terminal's cell frames resolve as one table's do**: each
//! buffer set's copy, read through that set's base for each row
//! ([`Renderer::cell_bases`]), gives every cell the rendition the same
//! feed gives it in the single mode, a display line joining two bands
//! included, on every set a frame is built into.

mod support;

use baud_core::{BandRows, BandSlots, Owned, Provision, Term};
use baud_domain::{
    BytePos, CacheGeometry, Cols, Depth, PackedCell, ReadLen, RowExtent, Rows, WinSize,
};
use baud_font::{Face, Px};
use baud_render::cell_frame::{CellInputs, CellParts, build};
use baud_render::screens::{SETS, Screens, SetIx};
use baud_render::{
    Blend, Candidate, GlyphMap, Instance, Leaving, Lent, MapEntry, Overlay, PxSize, Renderer,
    Resolved, Stack, StyleStamp, cell_styles_len,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 32,
    ws_col: 40,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for c in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, c);
        t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
    }
}

fn renderer<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        baud_render::Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// Paragraphs of three rows across bands of four, a word of its own
/// rendition across column 40, right-to-left text, and a prompt after
/// them, so every row above it is drawn at its words.
fn the_long_feed() -> Vec<u8> {
    let mut out = Vec::new();
    for r in 0..48usize {
        out.extend(
            format!(
                "\x1b[0;38;2;{};{};90mline {r:02} wraps here \
                 \x1b[1;38;2;{};40;40mwordrunningpasttherightedgeofit\
                 \x1b[22;48;2;20;{};60m and on, שלום עולם {r} מה שלומך, until a third row\x1b[0m\r\n",
                (r * 5) % 256,
                255 - (r * 5) % 256,
                (r * 3 + 60) % 256,
                r * 4
            )
            .as_bytes(),
        );
    }
    out.extend(b"\x1b]133;A\x07$ ");
    out
}

/// The cell path's buffers, one of each a set, as the app keeps them,
/// each set's table sized by [`cell_styles_len`].
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
    lent_cells: Vec<PackedCell>,
    lent_used: Vec<RowExtent>,
    used: Vec<RowExtent>,
}

impl<'m> Cells<'m> {
    fn new(backing: &'m mut [MapEntry], t: &Term<'_>) -> Cells<'m> {
        let stride = t.stride().index();
        let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
        let n = cell_styles_len(t, Cols::of_index(cols), Rows::of_index(rows)).0;
        Cells {
            map: GlyphMap::over(backing),
            exceptions: Overlay::new(),
            screens: Screens::new(),
            copies_mem: core::array::from_fn(|_| vec![PackedCell::EMPTY; (rows + 2) * stride]),
            styles_mem: core::array::from_fn(|_| vec![Resolved::default(); n]),
            overlay_mem: core::array::from_fn(|_| vec![0u8; cols * (rows + 1)]),
            style_stamps: [StyleStamp::default(); SETS],
            lent: core::array::from_fn(|_| Lent::default()),
            candidates: Vec::new(),
            records: Vec::new(),
            grounds: Vec::new(),
            lent_cells: Vec::new(),
            lent_used: Vec::new(),
            used: Vec::new(),
        }
    }
}

/// **Each window cell of set `at` after a frame built into it**, `back`
/// lines into the history: its text, and its rendition read through the
/// set's base for its row.
fn built(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    back: Rows,
    cell: &mut Cells<'_>,
    at: SetIx,
) -> Vec<(baud_domain::Content, Resolved)> {
    let seal = t.seal_writes();
    let t = &*t;
    r.fit(t);
    r.set_images(t, back);
    r.splash_yields_to(t, Px::of(0));
    r.place_marks(t, back, Leaving::None, Px::of(0));
    r.place_lights(t);
    let inputs = CellInputs {
        back,
        predictions: core::iter::empty(),
        live: None,
        composition: &None,
        highlight: None,
        leaving: Leaving::None,
        pan: Px::of(0),
    };
    {
        let mut copies = cell.copies_mem.each_mut().map(|c| &mut c[..]);
        let mut styles = cell.styles_mem.each_mut().map(|c| &mut c[..]);
        let mut overlay = cell.overlay_mem.each_mut().map(|c| &mut c[..]);
        build(
            r,
            t,
            seal,
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
                lent_cells: &mut cell.lent_cells,
                lent_used: &mut cell.lent_used,
            },
            &mut cell.used,
            move || Some(at),
        )
        .expect("the set is free");
    }
    let stride = t.stride().index();
    let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
    let (bases, _) = r.cell_bases(at);
    let styles = &cell.styles_mem[at.0];
    let mut out = Vec::new();
    for y in 0..rows {
        let base = bases.get(y).map_or(0, |&b| usize::try_from(b).unwrap());
        for x in 0..cols {
            let c = cell.copies_mem[at.0][y * stride + x];
            out.push((c.content, styles[base + c.style.index()]));
        }
    }
    out
}

/// **Every set's cells, banded and single, at the bottom and looking
/// back, frame after frame**: the window drawn at its words, some display
/// line joining two bands, the frames built into the sets in turn and
/// each read through its own set's bases.
/// Breaks: read a set's bases from another set's band window.
#[test]
fn every_set_reads_its_cells_as_one_table_does() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let geom = CacheGeometry::new(None, None, None, None);
    let p = Provision::banded_in(&WS, BandRows::of(Rows::of_index(4)), BandSlots(1_024))
        .expect("40x32 is admitted banded");
    let mut banded = Owned::with_history(p, &geom, Depth::of(100));
    let mut single = Owned::with_history(&WS, &geom, Depth::of(100));
    let mut tb = Term::new(banded.storage(), &WS);
    let mut ts = Term::new(single.storage(), &WS);
    feed(&mut tb, &the_long_feed());
    feed(&mut ts, &the_long_feed());
    let (mut rb, mut rs) = (renderer(&faces), renderer(&faces));
    let (mut kb, mut ks) = (GlyphMap::backing(), GlyphMap::backing());
    let mut cb = Cells::new(&mut kb, &tb);
    let mut cs = Cells::new(&mut ks, &ts);
    let backs = [0, 10, 3, 0, 10, 0].map(Rows::of_count);
    for (k, back) in backs.into_iter().enumerate() {
        let at = SetIx(k % SETS);
        let want = built(&mut rs, &mut ts, back, &mut cs, at);
        let got = built(&mut rb, &mut tb, back, &mut cb, at);
        let differ: Vec<usize> = (0..want.len()).filter(|&i| want[i] != got[i]).collect();
        assert_eq!(
            differ.len(),
            0,
            "frame {k}, {back:?} back, set {at:?}: cells that differ, first at {:?}",
            differ
                .first()
                .map(|i| (i / usize::from(WS.ws_col), i % usize::from(WS.ws_col)))
        );
        assert!(
            rb.cell_lines_joined(at).value() > 0,
            "frame {k}: some line joins two bands"
        );
    }
}

/// **What a set's table holds at 960x254, allocated and written**: the
/// bands as large as storage makes them at that width, a screen whose
/// every row is in renditions of its own, and frames into every set at
/// the bottom and as far back as the history goes. Each set's table is
/// filled with a value no frame resolves before its first frame, so the
/// entries a frame wrote are those that differ after, and the 4 KiB pages
/// they are in are what a zeroed allocation makes resident. Prints the
/// figures; holds that a set writes fewer pages than it allocates.
#[test]
fn a_set_s_table_at_960x254_is_mostly_address_space() {
    let ws = WinSize {
        ws_row: 254,
        ws_col: 960,
    };
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let geom = CacheGeometry::new(None, None, None, None);
    let p = Provision::banded(&ws, BandRows::of(Rows::of_index(10_000)))
        .expect("960x254 is admitted banded");
    let mut owned = Owned::with_history(p, &geom, Depth::of(1_000));
    let mut t = Term::new(owned.storage(), &ws);
    let mut bytes = Vec::new();
    for r in 0..600usize {
        bytes.extend(
            format!(
                "\x1b[0;38;2;{};{};90mrow {r} \x1b[48;2;20;{};{}m{}\x1b[0m\r\n",
                r % 256,
                (r * 7) % 256,
                (r * 3) % 256,
                (r * 11) % 256,
                "x".repeat(900)
            )
            .as_bytes(),
        );
    }
    feed(&mut t, &bytes);
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        baud_render::Theme::dark(),
        Blend::Native,
        Cols::of_index(960),
        Rows::of_index(254),
    )
    .unwrap();
    let n = cell_styles_len(&t, Cols::of_index(960), Rows::of_index(254)).0;
    let mut backing = GlyphMap::backing();
    let mut cell = Cells::new(&mut backing, &t);
    let unset = Resolved {
        fg: baud_render::Argb(0xdead_beef),
        ..Resolved::default()
    };
    for s in &mut cell.styles_mem {
        *s = vec![unset; n];
    }
    let stride = t.stride().index();
    for c in &mut cell.copies_mem {
        *c = vec![PackedCell::EMPTY; 256 * stride];
    }
    for c in &mut cell.overlay_mem {
        *c = vec![0u8; 960 * 255];
    }
    for (k, back) in [0, usize::MAX, 0]
        .map(Rows::of_count)
        .into_iter()
        .enumerate()
    {
        built(&mut r, &mut t, back, &mut cell, SetIx(k % SETS));
    }
    let size = core::mem::size_of::<Resolved>();
    let pages = n * size / 4096 + 1;
    for (set, s) in cell.styles_mem.iter().enumerate() {
        let mut written: Vec<usize> = (0..n)
            .filter(|&i| s[i] != unset)
            .map(|i| i * size / 4096)
            .collect();
        written.dedup();
        println!(
            "set {set}: {n} entries, {} bytes, {pages} pages allocated; {} pages written",
            n * size,
            written.len()
        );
        assert!(
            written.len() < pages,
            "set {set} writes fewer pages than it has"
        );
    }
}
