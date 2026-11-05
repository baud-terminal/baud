//! Variable fonts built to reach what HVAR and the phantom-point advance do
//! in rules the real fonts never test: baud-font's step 3 of note Z.
//!
//! Writes `baud-font/tests/var/hvar/*.ttf`. Each is 16 glyphs (.notdef,
//! g1..g15) of one square, advance 600, with gvar deltas on the phantom
//! points that disagree with HVAR, so a font whose HVAR is read gives
//! HVAR's answer and one whose HVAR is not gives gvar's. Axes: wght
//! 100-400-900, wdth 50-100-200.
//!
//! ```text
//! cases.ttf -- the HVAR table, written byte for byte here, so every field
//!   is what the test needs:
//!   regions  6: r0 wght peak 1 (wdth peak 0: factor 1), r1 wght
//!            0.2/0.5/0.8, r2 wdth -1, r3 wght start > peak (factor 1), r4
//!            wght -0.5/0.5/1 (straddles 0: factor 1), r5 wght and wdth
//!            peak 1
//!   data 0   word rows: two 16-bit columns then three 8-bit, regions
//!            0, 1, 6 (past the list: 0), 3, 4
//!   data 1   LONG_WORDS: one 32-bit column then two 16-bit, regions 0, 2,
//!            5
//!   data 2   no regions: every item 0
//!   data 3   one item
//!   advMap   format 0, two-byte entries, four inner bits, twelve entries;
//!            g12 to g15 read the last. Items reach: a 0.5 delta (rounds to
//!            1), a -0.5 delta (rounds to -1, where rounding the sum would
//!            give the hmtx advance), a sum floored at 0, an outer past the
//!            store, an inner past a set.
//! map-*.ttf -- the same store under other maps: none (the glyph id),
//!   format 1 with three-byte entries and twelve inner bits, one-byte
//!   entries with one inner bit, four-byte entries with sixteen, and format
//!   7 (the identity).
//! bad-*.ttf -- the same table with one thing broken: the map's offset past
//!   the table, the store's offset past it, data set 1's offset past it,
//!   majorVersion 2, regionCount's top bit set (HarfBuzz's HBUINT15 does
//!   not mask it -- a TODO in hb-open-type.hh -- so the count is 16 bits
//!   and the region list runs past the table), the side-bearing map's
//!   offset past the table, and a data set in range with more wide columns
//!   than regions. HarfBuzz refuses each whole table, so the advance is
//!   gvar's.
//! null-*.ttf -- an offset of 0, which is not malformed: a null store, a
//!   null region list, and a null data set 1. The table is read and those
//!   deltas are 0, so the advance is hmtx's -- where a refused table would
//!   be gvar's.
//! phantom.ttf -- no HVAR, so advances come from gvar's phantom points:
//!   lsb != xMin, a -0.5 extent, a USE_MY_METRICS composite whose own
//!   phantom deltas it replaces, one with two such components (the last
//!   wins), one nested two deep, a composite whose USE_MY_METRICS child is
//!   truncated (HarfBuzz's upem / 2) and one whose truncated child is not
//!   USE_MY_METRICS (not walked, so an ordinary advance), a composite with
//!   no USE_MY_METRICS component whose own phantom deltas stand, and
//!   USE_MY_METRICS fan-outs at 2,049 and 2,050 visits (HarfBuzz's
//!   graph-edge bound).
//! ```
//!
//! **Written here: every HVAR**, malformed or not, as its bytes: the
//! malformed ones are what no writer of a valid table produces, and the
//! valid ones share their store with them. **Written here: phantom's
//! `broken`**, a glyph that claims three contours and carries two bytes
//! of them ([`Broken`]). Deterministic: the head timestamps are fixed.
//! baud's own fonts, under the repository's licence
//! (`baud-font/tests/licences.rs` lists the directory).

use write_fonts::TableWriter;
use write_fonts::tables::glyf::{
    Anchor, Component, ComponentFlags, CompositeGlyph, GlyfLocaBuilder, Glyph, Transform,
};
use write_fonts::validate::{Validate, ValidationCtx};

use super::tuples::{Tuple, gvar, tuple};
use super::{
    Extremes, Totals, ZERO_EXTREMES, ZERO_TOTALS, add_glyph, bbox, bytes, cmap, finish, font, fvar,
    gid, head, hhea, hmtx, maxp, name, os2, os2_bytes, poly, post_v2, rect,
};

/// The path of a font here under the root.
macro_rules! p {
    ($n:literal) => {
        concat!("baud-font/tests/var/hvar/", $n)
    };
}

/// A store offset past every table here.
const PAST: u32 = 0xFFFF0;

fn f2(v: f64) -> [u8; 2] {
    ((v * 16384.0).round() as i16).to_be_bytes()
}

fn regions(top_bit: bool) -> Vec<u8> {
    let axes: [[(f64, f64, f64); 2]; 6] = [
        [(0.0, 1.0, 1.0), (0.0, 0.0, 0.0)],
        [(0.2, 0.5, 0.8), (0.0, 0.0, 0.0)],
        [(0.0, 0.0, 0.0), (-1.0, -1.0, 0.0)],
        [(0.8, 0.5, 1.0), (0.0, 0.0, 0.0)],
        [(-0.5, 0.5, 1.0), (0.0, 0.0, 0.0)],
        [(0.0, 1.0, 1.0), (0.0, 1.0, 1.0)],
    ];
    let mut out = vec![];
    out.extend(2u16.to_be_bytes());
    out.extend((if top_bit { 0x8000u16 } else { 0 } | axes.len() as u16).to_be_bytes());
    for region in axes {
        for (s, p, e) in region {
            out.extend(f2(s));
            out.extend(f2(p));
            out.extend(f2(e));
        }
    }
    out
}

/// Rows of deltas: the first `word_count` columns wide, the rest narrow.
fn var_data(
    items: &[&[i32]],
    word_count: u16,
    long_words: bool,
    region_indices: &[u16],
) -> Vec<u8> {
    let packed = word_count | if long_words { 0x8000 } else { 0 };
    let mut out = vec![];
    for v in [items.len() as u16, packed, region_indices.len() as u16] {
        out.extend(v.to_be_bytes());
    }
    for r in region_indices {
        out.extend(r.to_be_bytes());
    }
    for row in items {
        for (k, &v) in row.iter().enumerate() {
            let wide = k < word_count as usize;
            match (long_words, wide) {
                (true, true) => out.extend(v.to_be_bytes()),
                (false, false) => out.extend((v as i8).to_be_bytes()),
                _ => out.extend((v as i16).to_be_bytes()),
            }
        }
    }
    out
}

fn data() -> Vec<Vec<u8>> {
    vec![
        var_data(
            &[
                &[100, 0, 0, 0, 0],
                &[1, 0, 0, 0, 0],
                &[-1, 0, 0, 0, 0],
                &[0, 40, 50, -7, 9],
                &[-2000, 0, 0, 0, 0],
                &[3, -3, 0, 0, 0],
            ],
            2,
            false,
            &[0, 1, 6, 3, 4],
        ),
        var_data(&[&[30000, -300, 200], &[-70000, 5, 5]], 1, true, &[0, 2, 5]),
        var_data(&[&[], &[], &[]], 0, false, &[]),
        var_data(&[&[11, 0]], 1, false, &[0, 5]),
    ]
}

/// What is broken or null in a table; every field off is `cases.ttf`'s.
#[derive(Clone, Copy)]
struct Hvar {
    major: u16,
    bad_store: bool,
    bad_map: bool,
    bad_data: Option<usize>,
    top_bit: bool,
    bad_lsb: bool,
    bad_words: bool,
    null_store: bool,
    null_regions: bool,
    null_data: Option<usize>,
}

const CASES: Hvar = Hvar {
    major: 1,
    bad_store: false,
    bad_map: false,
    bad_data: None,
    top_bit: false,
    bad_lsb: false,
    bad_words: false,
    null_store: false,
    null_regions: false,
    null_data: None,
};

fn store(h: &Hvar) -> Vec<u8> {
    let region_list = regions(h.top_bit);
    let mut data = data();
    if h.bad_words {
        // More wide columns than regions: in range, and malformed.
        let mut d = vec![];
        for v in [1u16, 2, 1, 0] {
            d.extend(v.to_be_bytes());
        }
        d.extend(11i16.to_be_bytes());
        d.extend(0i16.to_be_bytes());
        data[3] = d;
    }
    let count = data.len();
    let head = 2 + 4 + 2 + 4 * count;
    let (mut offsets, mut blobs): (Vec<u32>, Vec<u8>) = (vec![], vec![]);
    let mut at = head + region_list.len();
    for (k, d) in data.iter().enumerate() {
        offsets.push(if Some(k) == h.bad_data {
            PAST
        } else if Some(k) == h.null_data {
            0
        } else {
            at as u32
        });
        blobs.extend(d);
        at += d.len();
    }
    let mut out = vec![];
    out.extend(1u16.to_be_bytes());
    out.extend((if h.null_regions { 0 } else { head as u32 }).to_be_bytes());
    out.extend((count as u16).to_be_bytes());
    for o in offsets {
        out.extend(o.to_be_bytes());
    }
    out.extend(region_list);
    out.extend(blobs);
    out
}

/// (outer, inner) per glyph for the twelve map entries; g12..g15 read the
/// last.
const ENTRIES: [(u32, u32); 12] = [
    (0, 0),
    (0, 1),
    (0, 2),
    (0, 3),
    (0, 4),
    (1, 0),
    (1, 1),
    (2, 0),
    (3, 5),
    (7, 0),
    (0, 9),
    (0, 5),
];

fn adv_map(format: u8, width: u8, inner_bits: u8, entries: &[(u32, u32)]) -> Vec<u8> {
    let entry_format = ((width - 1) << 4) | (inner_bits - 1);
    let mut out = vec![format, entry_format];
    match format {
        1 => out.extend((entries.len() as u32).to_be_bytes()),
        0 => out.extend((entries.len() as u16).to_be_bytes()),
        _ => return out,
    }
    for &(outer, inner) in entries {
        let u = (outer << inner_bits) | inner;
        out.extend(&u.to_be_bytes()[4 - width as usize..]);
    }
    out
}

fn hvar(map: Option<Vec<u8>>, h: Hvar) -> Vec<u8> {
    let s = store(&h);
    let mut store_at: u32 = 20;
    let mut map_at = if map.is_some() {
        store_at + s.len() as u32
    } else {
        0
    };
    if h.bad_store {
        store_at = PAST;
    }
    if h.null_store {
        store_at = 0;
    }
    if h.bad_map {
        map_at = PAST;
    }
    // A side-bearing map nothing reads still takes the table with it.
    let lsb_at = if h.bad_lsb { PAST } else { 0 };
    let mut out = vec![];
    out.extend(h.major.to_be_bytes());
    out.extend(0u16.to_be_bytes());
    for v in [store_at, map_at, lsb_at, 0] {
        out.extend(v.to_be_bytes());
    }
    out.extend(s);
    out.extend(map.unwrap_or_default());
    out
}

fn square(x0: i16, x1: i16) -> Glyph {
    Glyph::Simple(poly(&rect(x0, 0, x1, 700)))
}

/// A square is four points, then the phantoms: left, right, top, bottom.
fn phantom_tuple(region: [(f64, f64, f64); 2], left: i16, right: i16) -> Tuple {
    tuple(
        &region,
        &[
            None,
            None,
            None,
            None,
            Some((left, 0)),
            Some((right, 0)),
            None,
            None,
        ],
    )
}

const W: [(f64, f64, f64); 2] = [(0.0, 1.0, 1.0), (0.0, 0.0, 0.0)];
const D: [(f64, f64, f64); 2] = [(0.0, 0.0, 0.0), (0.0, 1.0, 1.0)];

/// The tables every font here shares, around its glyf, loca, hmtx, gvar
/// and, when it has one, HVAR.
struct Base<'a> {
    order: &'a [&'a str],
    glyf: (Vec<u8>, Vec<u8>, write_fonts::tables::loca::LocaFormat),
    metrics: &'a [(u16, i16)],
    gvar: Vec<u8>,
    head_box: (i16, i16, i16, i16),
    extremes: Extremes,
    totals: Totals,
    avg: i16,
}

fn base(b: Base, hvar: Option<Vec<u8>>) -> Vec<u8> {
    let (glyf, loca, format) = b.glyf;
    let (hmtx, long) = hmtx(b.metrics);
    let map: Vec<(u32, _)> = b.order[1..]
        .iter()
        .enumerate()
        .map(|(i, n)| (0x41 + i as u32, gid(b.order, n)))
        .collect();
    let last = 0x40 + b.order.len() as u16 - 1;
    let (x0, y0, x1, y1) = b.head_box;
    let mut tables = vec![
        (
            b"head",
            bytes(&head(0x0003, 0, bbox(x0, y0, x1, y1), Some(format))),
        ),
        (b"hhea", bytes(&hhea(900, -100, &b.extremes, long))),
        (b"maxp", bytes(&maxp(b.order.len() as u16, &b.totals))),
        (b"OS/2", os2_bytes(&os2(b.avg, 0x41, last, 0))),
        (b"hmtx", bytes(&hmtx)),
        (b"cmap", bytes(&cmap(&map))),
        (b"loca", loca),
        (b"glyf", glyf),
        (
            b"name",
            bytes(&name("baud hvar cases", "Regular", &["Weight", "Width"])),
        ),
        (b"post", bytes(&post_v2(b.order))),
        (
            b"fvar",
            bytes(&fvar(&[
                (b"wght", 100.0, 400.0, 900.0),
                (b"wdth", 50.0, 100.0, 200.0),
            ])),
        ),
        (b"gvar", b.gvar),
    ];
    if let Some(h) = hvar {
        tables.push((b"HVAR", h));
    }
    font(tables)
}

const NAMES: [&str; 16] = [
    ".notdef", "g1", "g2", "g3", "g4", "g5", "g6", "g7", "g8", "g9", "g10", "g11", "g12", "g13",
    "g14", "g15",
];

fn hvar_font(table: Vec<u8>) -> Vec<u8> {
    let glyphs: Vec<Glyph> = NAMES.iter().map(|_| square(50, 550)).collect();
    let metrics = [(600, 50); 16];
    // gvar moves every right phantom by 77 at wght 900: an advance HVAR
    // does not override would show it.
    let tuples = NAMES
        .iter()
        .map(|n| (*n, vec![phantom_tuple(W, 0, 77)]))
        .collect();
    base(
        Base {
            order: &NAMES,
            glyf: super::glyf_loca(&glyphs),
            metrics: &metrics,
            gvar: gvar(&NAMES, 2, tuples),
            head_box: (50, 0, 550, 700),
            extremes: Extremes {
                advance_width_max: 600,
                min_lsb: 50,
                min_rsb: 50,
                x_max_extent: 550,
            },
            totals: Totals {
                points: 4,
                contours: 1,
                ..ZERO_TOTALS
            },
            avg: 600,
        },
        Some(table),
    )
}

const USE_MY_METRICS: bool = true;

fn component(order: &[&str], n: &str, umm: bool, x: i16) -> Component {
    Component::new(
        gid(order, n),
        Anchor::Offset { x, y: 0 },
        Transform::default(),
        ComponentFlags {
            use_my_metrics: umm,
            ..Default::default()
        },
    )
}

/// A composite with the box fontTools bounded it by, from its components
/// as `setupGlyf` saw them: `broken` was still a square then.
fn composite(order: &[&str], parts: &[(&str, bool, i16)], b: (i16, i16, i16, i16)) -> Glyph {
    let b = bbox(b.0, b.1, b.2, b.3);
    let c = |&(n, umm, x): &(&str, bool, i16)| component(order, n, umm, x);
    let mut g = CompositeGlyph::new(c(&parts[0]), b);
    for p in &parts[1..] {
        g.add_component(c(p), b);
    }
    g.bbox = b;
    Glyph::Composite(g)
}

/// **Written here: `broken`**, a glyph that claims three contours and
/// carries two bytes of them: its header (3, then the box 50, 0, 550,
/// 700) and one end point's high byte and its low. Its loca length, 12,
/// is the truncation.
struct Broken;

impl Broken {
    fn bytes() -> Vec<u8> {
        let mut b = vec![];
        for v in [3i16, 50, 0, 550, 700] {
            b.extend(v.to_be_bytes());
        }
        b.extend([0x00, 0x02]);
        b
    }
}

impl write_fonts::FontWrite for Broken {
    fn write_into(&self, writer: &mut TableWriter) {
        writer.write_slice(&Broken::bytes());
    }
}

impl Validate for Broken {
    fn validate_impl(&self, _: &mut ValidationCtx) {}
}

impl write_fonts::tables::glyf::SomeGlyph for Broken {}

const PHANTOM: [&str; 14] = [
    ".notdef", "a", "b", "half", "mine", "two", "deep", "broken", "badchild", "skip", "own",
    "fan64", "edge2049", "edge2050",
];

fn phantom() -> Vec<u8> {
    let o = &PHANTOM;
    let full = (50, 0, 550, 700);
    let fan = |inner: &'static str, n: usize| vec![(inner, USE_MY_METRICS, 0); n];
    let edge = |a: usize| {
        let mut p = fan("fan64", 31);
        p.extend(fan("a", a));
        p
    };
    let glyphs = vec![
        square(50, 550),
        square(50, 550),
        square(80, 500),
        square(50, 550),
        composite(
            o,
            &[("a", USE_MY_METRICS, 0), ("b", false, 300)],
            (50, 0, 800, 700),
        ),
        composite(
            o,
            &[("a", USE_MY_METRICS, 0), ("b", USE_MY_METRICS, 0)],
            full,
        ),
        composite(o, &[("mine", USE_MY_METRICS, 0)], (50, 0, 800, 700)),
        Glyph::Empty,
        composite(o, &[("broken", USE_MY_METRICS, 0)], full),
        composite(o, &[("a", USE_MY_METRICS, 0), ("broken", false, 0)], full),
        // No USE_MY_METRICS component: the composite's own phantoms stand.
        composite(o, &[("b", false, 0)], (80, 0, 500, 700)),
        // HarfBuzz's visit bound on the USE_MY_METRICS walk: 1 + 31 * 65
        // + 33 visits is 2,049, which it walks; one more is refused (upem
        // / 2).
        composite(o, &fan("a", 64), full),
        composite(o, &edge(33), full),
        composite(o, &edge(34), full),
    ];
    let mut b = GlyfLocaBuilder::new();
    for (n, g) in o.iter().zip(&glyphs) {
        if *n == "broken" {
            b.add_glyph(&Broken).expect("a glyph");
        } else {
            add_glyph(&mut b, g);
        }
    }
    // lsb differs from xMin on every glyph: HarfBuzz starts the phantoms
    // at xMin - lsb, so the extent is the same and only the start moves.
    let metrics: Vec<(u16, i16)> = (0..o.len() as i16)
        .map(|i| (600 + 10 * i as u16, 20 + i))
        .collect();
    let s = Some;
    let tuples = vec![
        (
            "a",
            vec![phantom_tuple(W, 10, 55), phantom_tuple(D, -7, 13)],
        ),
        ("b", vec![phantom_tuple(W, -20, 90)]),
        // At wght 650 (0.5): 0 - 1 * 0.5 is a -0.5 extent, which rounds
        // away.
        ("half", vec![phantom_tuple(W, 0, -1)]),
        // The composites' own tuples move their phantoms; USE_MY_METRICS
        // then replaces them. Two component offsets, then the phantoms.
        (
            "mine",
            vec![tuple(
                &W,
                &[s((5, 0)), s((5, 0)), s((300, 0)), s((900, 0)), None, None],
            )],
        ),
        (
            "skip",
            vec![tuple(
                &W,
                &[s((0, 0)), s((0, 0)), s((0, 0)), s((41, 0)), None, None],
            )],
        ),
        (
            "own",
            vec![tuple(&W, &[s((0, 0)), s((12, 0)), s((60, 0)), None, None])],
        ),
    ];
    // Written with fontTools' bounding off: head's box, hhea's extremes
    // and maxp's totals are left at zero.
    base(
        Base {
            order: o,
            glyf: finish(b),
            metrics: &metrics,
            gvar: gvar(o, 2, tuples),
            head_box: (0, 0, 0, 0),
            extremes: ZERO_EXTREMES,
            totals: ZERO_TOTALS,
            avg: 665,
        },
        None,
    )
}

pub fn fonts() -> Vec<(&'static str, Vec<u8>)> {
    let m = |f, w, i| Some(adv_map(f, w, i, &ENTRIES));
    let one = [(0, 1), (1, 1), (0, 0), (1, 0), (2, 1)];
    let with = |h: fn(&mut Hvar)| {
        let mut t = CASES;
        h(&mut t);
        hvar(m(0, 2, 4), t)
    };
    let tables = vec![
        (p!("cases.ttf"), hvar(m(0, 2, 4), CASES)),
        (p!("map-none.ttf"), hvar(None, CASES)),
        (p!("map-f1-w3-i12.ttf"), hvar(m(1, 3, 12), CASES)),
        (
            p!("map-w1-i1.ttf"),
            hvar(Some(adv_map(0, 1, 1, &one)), CASES),
        ),
        (p!("map-w4-i16.ttf"), hvar(m(0, 4, 16), CASES)),
        (p!("map-f7.ttf"), hvar(m(7, 2, 4), CASES)),
        (p!("bad-map.ttf"), with(|t| t.bad_map = true)),
        (p!("bad-store.ttf"), with(|t| t.bad_store = true)),
        (p!("bad-data.ttf"), with(|t| t.bad_data = Some(1))),
        (p!("bad-version.ttf"), with(|t| t.major = 2)),
        (p!("bad-regions.ttf"), with(|t| t.top_bit = true)),
        (p!("bad-lsb-map.ttf"), with(|t| t.bad_lsb = true)),
        (p!("bad-words.ttf"), with(|t| t.bad_words = true)),
        (p!("null-store.ttf"), with(|t| t.null_store = true)),
        (p!("null-regions.ttf"), with(|t| t.null_regions = true)),
        (p!("null-data.ttf"), with(|t| t.null_data = Some(1))),
    ];
    let mut out: Vec<(&'static str, Vec<u8>)> = tables
        .into_iter()
        .map(|(path, table)| (path, hvar_font(table)))
        .collect();
    out.push((p!("phantom.ttf"), phantom()));
    out
}
