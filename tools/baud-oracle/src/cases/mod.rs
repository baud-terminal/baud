//! **The cases fonts**: the small fonts baud wrote from nothing to reach
//! what no shipping face does, each built here from its recipe on
//! write-fonts 0.55.0 and written under `baud-font/tests/var/`.
//!
//! | case | writes | reaches |
//! |---|---|---|
//! | `bake` | `bake/cases.ttf` | what JetBrains Mono never makes the bake do ([`bake`]) |
//! | `bearing` | `bearing-cases.ttf`, `bearing-cases-bit1.ttf` | left side bearings that are not xMin ([`bearing`]) |
//! | `budget` | `budget-cases.ttf` | glyphs past a renderer's scratch budget ([`budget`]) |
//! | `cff2` | `cff2-cases.otf` | the CFF2 charstring rules the real CFF2 fonts leave untested ([`cff2`]) |
//! | `colour-refused` | `colour-refused-cases.ttf` | a colour glyph whose outline is refused ([`colour_refused`]) |
//! | `component` | `component-cases.ttf` | components placed by points and by scaled offsets ([`component`]) |
//!
//! Each table is a write-fonts value, serialised by write-fonts, except
//! where it cannot express the case; then the bytes are written here, or
//! patched after writing, and each such place says which field and why.
//! The fonts are compared as the bytes committed: `--check` writes
//! nothing and fails unless every font equals its committed file, which
//! pins each font's bytes to its recipe.
//!
//! ```text
//! nix-shell tools/shell.nix --run \
//!   'cargo run --release --manifest-path tools/Cargo.toml --bin baud-oracle -- cases all'
//! ```

pub mod bake;
pub mod bearing;
pub mod budget;
pub mod cff2;
pub mod colour_refused;
pub mod component;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use read_fonts::tables::glyf::CurvePoint;
use write_fonts::FontBuilder;
use write_fonts::tables::cmap::Cmap;
use write_fonts::tables::fvar::{AxisInstanceArrays, Fvar, VariationAxisRecord};
use write_fonts::tables::glyf::{Bbox, Contour, GlyfLocaBuilder, Glyph, SimpleGlyph};
use write_fonts::tables::head::{Flags, Head, MacStyle};
use write_fonts::tables::hhea::Hhea;
use write_fonts::tables::hmtx::{Hmtx, LongMetric};
use write_fonts::tables::loca::LocaFormat;
use write_fonts::tables::maxp::Maxp;
use write_fonts::tables::name::{Name, NameRecord};
use write_fonts::tables::os2::{Os2, SelectionFlags};
use write_fonts::tables::post::Post;
use write_fonts::types::{
    FWord, Fixed, GlyphId, LongDateTime, NameId, Tag, UfWord, Version16Dot16,
};

/// One case: its name and the fonts it writes, each as a path under the
/// root and its bytes.
pub struct Case {
    pub name: &'static str,
    pub fonts: fn() -> Vec<(&'static str, Vec<u8>)>,
}

/// Every case, in the order `cases all` writes them.
pub const ALL: [Case; 6] = [
    Case {
        name: "bake",
        fonts: bake::fonts,
    },
    Case {
        name: "bearing",
        fonts: bearing::fonts,
    },
    Case {
        name: "budget",
        fonts: budget::fonts,
    },
    Case {
        name: "cff2",
        fonts: cff2::fonts,
    },
    Case {
        name: "colour-refused",
        fonts: colour_refused::fonts,
    },
    Case {
        name: "component",
        fonts: component::fonts,
    },
];

/// Whether every font of `case` equals the file under `root`, naming the
/// first that does not.
pub fn check(case: &Case, root: &Path) -> Result<(), String> {
    for (path, bytes) in (case.fonts)() {
        let file = root.join(path);
        let committed = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        if committed != bytes {
            return Err(format!(
                "{path}: {} bytes built, {} committed, and they differ",
                bytes.len(),
                committed.len()
            ));
        }
    }
    Ok(())
}

/// `baud-oracle cases <name>|all [--root DIR] [--check]`.
pub fn main(args: &[String]) -> ExitCode {
    let mut root = crate::repo();
    let mut check_only = false;
    let mut names = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check" => check_only = true,
            "--root" => match it.next() {
                Some(r) => root = PathBuf::from(r),
                None => return usage(),
            },
            n => names.push(n.to_string()),
        }
    }
    let chosen: Vec<&Case> = if names == ["all"] {
        ALL.iter().collect()
    } else {
        let mut out = vec![];
        for n in &names {
            match ALL.iter().find(|c| c.name == n) {
                Some(c) => out.push(c),
                None => return usage(),
            }
        }
        out
    };
    if chosen.is_empty() {
        return usage();
    }
    let mut failed = false;
    for case in chosen {
        if check_only {
            if let Err(e) = check(case, &root) {
                eprintln!("cases {}: {e}", case.name);
                failed = true;
            }
            continue;
        }
        for (path, bytes) in (case.fonts)() {
            let file = root.join(path);
            if let Err(e) = std::fs::write(&file, &bytes) {
                eprintln!("{}: {e}", file.display());
                return ExitCode::FAILURE;
            }
            println!("{} {}", file.display(), bytes.len());
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn usage() -> ExitCode {
    let names: Vec<&str> = ALL.iter().map(|c| c.name).collect();
    eprintln!(
        "usage: baud-oracle cases {}|all [--root DIR] [--check]",
        names.join("|")
    );
    ExitCode::FAILURE
}

// ---------------------------------------------------------------------------
// What every case shares: the defaults the cases fonts were made with, each
// set here as a value rather than left to a writer's choice.

/// The glyph id of `name` in `order`.
pub fn gid(order: &[&str], name: &str) -> GlyphId {
    let i = order
        .iter()
        .position(|n| *n == name)
        .unwrap_or_else(|| panic!("no glyph {name}"));
    GlyphId::new(i as u32)
}

/// A simple glyph of on-curve contours, its box recomputed from them.
pub fn contours(list: &[&[(i16, i16)]]) -> SimpleGlyph {
    let mut g = SimpleGlyph {
        contours: list
            .iter()
            .map(|c| {
                Contour::from(
                    c.iter()
                        .map(|&(x, y)| CurvePoint::on_curve(x, y))
                        .collect::<Vec<_>>(),
                )
            })
            .collect(),
        ..Default::default()
    };
    g.recompute_bounding_box();
    g
}

/// One on-curve polygon, the shape most cases are drawn from.
pub fn poly(points: &[(i16, i16)]) -> SimpleGlyph {
    contours(&[points])
}

/// A rectangle's points: bottom-left, bottom-right, top-right, top-left.
pub fn rect(x0: i16, y0: i16, x1: i16, y1: i16) -> [(i16, i16); 4] {
    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

/// A box from its four edges.
pub fn bbox(x_min: i16, y_min: i16, x_max: i16, y_max: i16) -> Bbox {
    Bbox {
        x_min,
        y_min,
        x_max,
        y_max,
    }
}

/// The glyph's box, or none for an empty glyph.
pub fn glyph_box(g: &Glyph) -> Option<Bbox> {
    match g {
        Glyph::Empty => None,
        Glyph::Simple(s) => Some(s.bbox),
        Glyph::Composite(c) => Some(c.bbox),
    }
}

/// glyf and loca from glyphs in glyph order.
pub fn glyf_loca(glyphs: &[Glyph]) -> (Vec<u8>, Vec<u8>, LocaFormat) {
    let mut b = GlyfLocaBuilder::new();
    for g in glyphs {
        b.add_glyph(g).expect("a valid glyph");
    }
    let (glyf, loca, format) = b.build();
    (
        write_fonts::dump_table(&glyf).expect("glyf"),
        write_fonts::dump_table(&loca).expect("loca"),
        format,
    )
}

/// hmtx from each glyph's (advance, lsb), with the trailing run of equal
/// advances kept as one long metric and the rest as side bearings alone:
/// the number of long metrics hhea must name.
pub fn hmtx(metrics: &[(u16, i16)]) -> (Hmtx, u16) {
    let mut n = metrics.len();
    let last = metrics.last().expect("a glyph").0;
    while n > 1 && metrics[n - 2].0 == last {
        n -= 1;
    }
    let h_metrics = metrics[..n]
        .iter()
        .map(|&(advance, side_bearing)| LongMetric {
            advance,
            side_bearing,
        })
        .collect();
    let left_side_bearings = metrics[n..].iter().map(|m| m.1).collect();
    (
        Hmtx {
            h_metrics,
            left_side_bearings,
        },
        n as u16,
    )
}

/// head with the cases fonts' fields: revision 1.0, 1000 units, the given
/// flags, timestamps and box, and the loca format the glyf builder chose.
pub fn head(flags: u16, stamp: i64, b: Bbox, loca: Option<LocaFormat>) -> Head {
    Head {
        font_revision: Fixed::from_f64(1.0),
        checksum_adjustment: 0,
        magic_number: 0x5F0F_3CF5,
        flags: Flags::from_bits_truncate(flags),
        units_per_em: 1000,
        created: LongDateTime::new(stamp),
        modified: LongDateTime::new(stamp),
        x_min: b.x_min,
        y_min: b.y_min,
        x_max: b.x_max,
        y_max: b.y_max,
        mac_style: MacStyle::empty(),
        lowest_rec_ppem: 3,
        font_direction_hint: 2,
        index_to_loc_format: match loca {
            Some(LocaFormat::Long) => 1,
            _ => 0,
        },
    }
}

/// hhea's horizontal extremes: advance width max, min left and right side
/// bearings and x max extent.
pub struct Extremes {
    pub advance_width_max: u16,
    pub min_lsb: i16,
    pub min_rsb: i16,
    pub x_max_extent: i16,
}

/// Extremes left at zero, as a font written without recalculating them
/// carries.
pub const ZERO_EXTREMES: Extremes = Extremes {
    advance_width_max: 0,
    min_lsb: 0,
    min_rsb: 0,
    x_max_extent: 0,
};

/// hhea with the given ascent, descent and extremes.
pub fn hhea(ascender: i16, descender: i16, e: &Extremes, long_metrics: u16) -> Hhea {
    Hhea {
        ascender: FWord::new(ascender),
        descender: FWord::new(descender),
        line_gap: FWord::new(0),
        advance_width_max: UfWord::new(e.advance_width_max),
        min_left_side_bearing: FWord::new(e.min_lsb),
        min_right_side_bearing: FWord::new(e.min_rsb),
        x_max_extent: FWord::new(e.x_max_extent),
        caret_slope_rise: 1,
        caret_slope_run: 0,
        caret_offset: 0,
        number_of_h_metrics: long_metrics,
    }
}

/// maxp 1.0's glyph totals: points, contours, composite points and
/// contours, component elements and depth.
pub struct Totals {
    pub points: u16,
    pub contours: u16,
    pub composite_points: u16,
    pub composite_contours: u16,
    pub component_elements: u16,
    pub component_depth: u16,
}

/// Totals left at zero, as a font written without recalculating them
/// carries.
pub const ZERO_TOTALS: Totals = Totals {
    points: 0,
    contours: 0,
    composite_points: 0,
    composite_contours: 0,
    component_elements: 0,
    component_depth: 0,
};

/// maxp 1.0 for a glyf font: two zones and no hinting resources.
pub fn maxp(num_glyphs: u16, t: &Totals) -> Maxp {
    Maxp {
        num_glyphs,
        max_points: Some(t.points),
        max_contours: Some(t.contours),
        max_composite_points: Some(t.composite_points),
        max_composite_contours: Some(t.composite_contours),
        max_zones: Some(2),
        max_twilight_points: Some(0),
        max_storage: Some(0),
        max_function_defs: Some(0),
        max_instruction_defs: Some(0),
        max_stack_elements: Some(0),
        max_size_of_instructions: Some(0),
        max_component_elements: Some(t.component_elements),
        max_component_depth: Some(t.component_depth),
    }
}

/// maxp 0.5, the glyph count alone, for a CFF2 font.
pub fn maxp_cff(num_glyphs: u16) -> Maxp {
    Maxp {
        num_glyphs,
        ..Default::default()
    }
}

/// OS/2 version 3 with the cases fonts' fields: weight 400, width 5,
/// embedding "preview and print" (fsType 4), Basic Latin in the first
/// word of Unicode ranges, vendor `????` and break char U+0020; the average
/// width, the first and last code points and the second word of Unicode
/// ranges given (bit 45, Geometric Shapes, is `0x2000`; bit 60, Private
/// Use Area, `0x1000_0000`).
///
/// write-fonts writes these fields as version 4, the same layout; the
/// version word is set to 3 after writing, by [`os2_bytes`].
pub fn os2(x_avg_char_width: i16, first: u16, last: u16, ranges_2: u32) -> Os2 {
    Os2 {
        x_avg_char_width,
        us_weight_class: 400,
        us_width_class: 5,
        fs_type: 4,
        ul_unicode_range_1: 1,
        ul_unicode_range_2: ranges_2,
        ach_vend_id: Tag::new(b"????"),
        fs_selection: SelectionFlags::empty(),
        us_first_char_index: first,
        us_last_char_index: last,
        ul_code_page_range_1: Some(0),
        ul_code_page_range_2: Some(0),
        sx_height: Some(0),
        s_cap_height: Some(0),
        us_default_char: Some(0),
        us_break_char: Some(32),
        us_max_context: Some(0),
        ..Default::default()
    }
}

/// **Patch: OS/2 `version` = 3.** write-fonts names a table with these
/// fields version 4 (`Os2::compute_version`); versions 2 to 4 share one
/// layout, and the cases fonts are version 3.
pub fn os2_bytes(t: &Os2) -> Vec<u8> {
    let mut b = write_fonts::dump_table(t).expect("OS/2");
    assert_eq!(&b[..2], &[0, 4], "write-fonts' OS/2 version");
    b[..2].copy_from_slice(&3u16.to_be_bytes());
    b
}

/// name with the family and style (IDs 1 and 2) and each axis's name from
/// ID 256, on the Macintosh Roman and the Windows Unicode BMP platforms.
pub fn name(family: &str, style: &str, axes: &[&str]) -> Name {
    let mut strings: Vec<(u16, &str)> = vec![(1, family), (2, style)];
    for (i, a) in axes.iter().enumerate() {
        strings.push((256 + i as u16, a));
    }
    let mut records = vec![];
    for (platform, encoding, language) in [(1u16, 0u16, 0u16), (3, 1, 0x409)] {
        for &(id, s) in &strings {
            records.push(NameRecord::new(
                platform,
                encoding,
                language,
                NameId::new(id),
                s.to_string().into(),
            ));
        }
    }
    Name::new(records)
}

/// post version 2 with every glyph's name.
pub fn post_v2(order: &[&str]) -> Post {
    Post::new_v2(order.iter().copied())
}

/// post version 3: no glyph names, as a CFF2 font carries.
pub fn post_v3() -> Post {
    Post {
        version: Version16Dot16::VERSION_3_0,
        ..Default::default()
    }
}

/// cmap with format 4 subtables for Unicode BMP and Windows BMP.
pub fn cmap(map: &[(u32, GlyphId)]) -> Cmap {
    Cmap::from_mappings(
        map.iter()
            .map(|&(c, g)| (char::from_u32(c).expect("a char"), g)),
    )
    .expect("no conflict")
}

/// fvar with the given (tag, min, default, max) axes, named from ID 256,
/// and no instances.
pub fn fvar(axes: &[(&[u8; 4], f64, f64, f64)]) -> Fvar {
    Fvar::new(AxisInstanceArrays::new(
        axes.iter()
            .enumerate()
            .map(|(i, &(tag, min, default, max))| VariationAxisRecord {
                axis_tag: Tag::new(tag),
                min_value: Fixed::from_f64(min),
                default_value: Fixed::from_f64(default),
                max_value: Fixed::from_f64(max),
                flags: 0,
                axis_name_id: NameId::new(256 + i as u16),
            })
            .collect(),
        vec![],
    ))
}

/// The font: every table given as bytes, ordered and checksummed by
/// write-fonts' builder.
pub fn font(tables: Vec<(&[u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut b = FontBuilder::new();
    for (tag, bytes) in tables {
        b.add_raw(Tag::new(tag), bytes);
    }
    b.build()
}

/// A table's bytes.
pub fn bytes<T: write_fonts::FontWrite + write_fonts::validate::Validate>(t: &T) -> Vec<u8> {
    write_fonts::dump_table(t).expect("a table")
}

/// One gvar tuple over (start, peak, end) per axis, a delta per point and
/// phantom; `None` leaves the point unreferenced, its delta inferred.
pub fn tuple(
    region: &[(f32, f32, f32)],
    deltas: &[Option<(i16, i16)>],
) -> write_fonts::tables::gvar::GlyphDeltas {
    use write_fonts::tables::gvar::{GlyphDelta, GlyphDeltas, Tent};
    use write_fonts::types::F2Dot14;
    GlyphDeltas::new(
        region
            .iter()
            .map(|&(s, p, e)| {
                Tent::new(
                    F2Dot14::from_f32(p),
                    Some((F2Dot14::from_f32(s), F2Dot14::from_f32(e))),
                )
            })
            .collect(),
        deltas
            .iter()
            .map(|d| match d {
                Some((x, y)) => GlyphDelta::required(*x, *y),
                None => GlyphDelta::optional(0, 0),
            })
            .collect(),
    )
}

/// Every delta of a tuple referenced.
pub fn all(deltas: &[(i16, i16)]) -> Vec<Option<(i16, i16)>> {
    deltas.iter().copied().map(Some).collect()
}

/// gvar over `axes` axes: each glyph's tuples, none for a glyph not named.
pub fn gvar(
    order: &[&str],
    axes: u16,
    tuples: Vec<(&str, Vec<write_fonts::tables::gvar::GlyphDeltas>)>,
) -> Vec<u8> {
    use write_fonts::tables::gvar::{GlyphVariations, Gvar};
    let mut by_glyph: Vec<Vec<_>> = order.iter().map(|_| vec![]).collect();
    for (n, t) in tuples {
        by_glyph[gid(order, n).to_u32() as usize] = t;
    }
    let variations = by_glyph
        .into_iter()
        .enumerate()
        .map(|(i, t)| GlyphVariations::new(GlyphId::new(i as u32), t))
        .collect();
    bytes(&Gvar::new(variations, axes).expect("a gvar"))
}

/// COLR version 0: each colour glyph's layers, (glyph, palette entry) from
/// the bottom up, the base records in glyph order.
pub fn colr_v0(order: &[&str], colour: &[(&str, &[(&str, u16)])]) -> Vec<u8> {
    use write_fonts::tables::colr::{BaseGlyph, Colr, Layer};
    use write_fonts::types::GlyphId16;
    let g16 = |n: &str| GlyphId16::new(gid(order, n).to_u32() as u16);
    let mut sorted: Vec<_> = colour.to_vec();
    sorted.sort_by_key(|(n, _)| gid(order, n));
    let (mut bases, mut layers) = (vec![], vec![]);
    for (n, ls) in sorted {
        bases.push(BaseGlyph::new(g16(n), layers.len() as u16, ls.len() as u16));
        layers.extend(ls.iter().map(|&(l, p)| Layer::new(g16(l), p)));
    }
    let (nb, nl) = (bases.len() as u16, layers.len() as u16);
    bytes(&Colr::new(nb, Some(bases), Some(layers), nl))
}

/// CPAL version 0: one palette of (red, green, blue, alpha) entries.
pub fn cpal_v0(colours: &[(u8, u8, u8, u8)]) -> Vec<u8> {
    use write_fonts::tables::cpal::{ColorRecord, Cpal};
    let records: Vec<ColorRecord> = colours
        .iter()
        .map(|&(red, green, blue, alpha)| ColorRecord {
            blue,
            green,
            red,
            alpha,
        })
        .collect();
    let n = records.len() as u16;
    bytes(&Cpal::new(n, 1, n, Some(records), vec![0]))
}
