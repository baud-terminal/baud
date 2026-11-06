//! The stock face and a [`Source`] merged into one font.
//!
//! The stock face's tables are read through read-fonts, converted to
//! write-fonts values and changed only where the source says. The values
//! the stage's own build recomputed on save are recomputed here the same
//! way: maxp's maxima, head's bounding box and its lsb-is-xMin flag,
//! hhea's extents and its count of long metrics, OS/2's first and last
//! character, and each new glyph's bounding box and left side bearing.

use std::collections::{BTreeMap, HashMap};

use read_fonts::tables::glyf::{CurvePoint, Glyph as ReadGlyph};
use read_fonts::tables::layout::Script as ReadScript;
use read_fonts::types::{GlyphId, GlyphId16, NameId, Tag};
use read_fonts::{FontRef, TableProvider};
use write_fonts::from_obj::ToOwnedTable;
use write_fonts::tables::cmap::Cmap;
use write_fonts::tables::gdef::{Gdef, MarkGlyphSets};
use write_fonts::tables::glyf::{
    Anchor as CompAnchor, Bbox, Component, ComponentFlags, CompositeGlyph, Contour, Glyph,
    SimpleGlyph, Transform,
};
use write_fonts::tables::gpos::{
    AnchorTable, BaseArray, BaseRecord, Gpos, Mark2Array, Mark2Record, MarkArray, MarkBasePos,
    MarkBasePosFormat1, MarkMarkPos, MarkMarkPosFormat1, MarkRecord, PositionChainContext,
    PositionLookup,
};
use write_fonts::tables::gsub::Gsub;
use write_fonts::tables::head::Head;
use write_fonts::tables::hhea::Hhea;
use write_fonts::tables::hmtx::{Hmtx, LongMetric};
use write_fonts::tables::layout::{
    ChainedSequenceContext, ChainedSequenceContextFormat3, ClassDef, CoverageTable, Lookup,
    LookupFlag, Script, ScriptRecord, SequenceLookupRecord,
};
use write_fonts::tables::maxp::Maxp;
use write_fonts::tables::name::{Name, NameRecord};
use write_fonts::tables::os2::Os2;
use write_fonts::tables::post::Post;
use write_fonts::{FontBuilder, dump_table};

use super::text::{Kind, Outline, Source, Subtable};

type Res<T> = Result<T, String>;

fn e<E: std::fmt::Display>(what: &'static str) -> impl Fn(E) -> String {
    move |err| format!("{what}: {err}")
}

/// The glyph data of one glyph as write-fonts encodes it, without the
/// trailing pad byte write-fonts adds to an odd-length glyph: the stock
/// faces and the stage store each glyph unpadded under a long loca, and
/// this keeps glyf as they carry it.
fn glyph_bytes(g: &Glyph) -> Res<Vec<u8>> {
    if matches!(g, Glyph::Empty) {
        return Ok(vec![]);
    }
    let mut d = dump_table(g).map_err(e("glyf"))?;
    let len = unpadded_len(&d)?;
    if len + 1 == d.len() && d[len] == 0 {
        d.truncate(len);
    } else if len != d.len() {
        return Err(format!("glyf: {len} bytes of {} are the glyph", d.len()));
    }
    Ok(d)
}

/// How many bytes of `d` one glyph's record spans, read from its own
/// counts and flags (the OpenType glyf chapter's layout).
pub fn unpadded_len(d: &[u8]) -> Res<usize> {
    let be = |i: usize| -> Res<u16> {
        d.get(i..i + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .ok_or_else(|| "glyf: a glyph ends early".to_string())
    };
    let byte = |i: usize| d.get(i).copied().ok_or("glyf: a glyph ends early");
    let contours = be(0)? as i16;
    let mut at;
    if contours >= 0 {
        let n = contours as usize;
        let points = if n == 0 {
            0
        } else {
            be(10 + 2 * (n - 1))? as usize + 1
        };
        at = 10 + 2 * n;
        at += 2 + be(at)? as usize;
        let (mut xs, mut ys, mut i) = (0, 0, 0);
        while i < points {
            let flag = byte(at)?;
            at += 1;
            let repeat = if flag & 0x08 != 0 {
                at += 1;
                byte(at - 1)? as usize
            } else {
                0
            };
            let width = |short: u8, same: u8| {
                if flag & short != 0 {
                    1
                } else if flag & same != 0 {
                    0
                } else {
                    2
                }
            };
            xs += width(0x02, 0x10) * (repeat + 1);
            ys += width(0x04, 0x20) * (repeat + 1);
            i += repeat + 1;
        }
        at += xs + ys;
    } else {
        at = 10;
        let (mut more, mut instructions) = (true, false);
        while more {
            let flags = be(at)?;
            at += 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
            at += if flags & 0x0008 != 0 {
                2
            } else if flags & 0x0040 != 0 {
                4
            } else if flags & 0x0080 != 0 {
                8
            } else {
                0
            };
            more = flags & 0x0020 != 0;
            instructions |= flags & 0x0100 != 0;
        }
        if instructions {
            at += 2 + be(at)? as usize;
        }
    }
    Ok(at)
}

struct Metrics {
    bbox: Option<Bbox>,
    points: u16,
    contours: u16,
    /// Component nesting for a composite, 0 for a simple glyph.
    depth: u16,
    composite: bool,
}

fn union(a: Option<Bbox>, b: Bbox) -> Bbox {
    match a {
        Some(a) => a.union(b),
        None => b,
    }
}

/// The source merged into `base`, as font bytes.
pub fn build(base: &[u8], src: &Source) -> Res<Vec<u8>> {
    let f = FontRef::new(base).map_err(e("base"))?;
    let n_base = f.maxp().map_err(e("maxp"))?.num_glyphs() as u32;
    let post = f.post().map_err(e("post"))?;
    let mut names: Vec<String> = (0..n_base)
        .map(|g| {
            post.glyph_name(GlyphId16::new(g as u16))
                .map(str::to_string)
                .ok_or_else(|| format!("base: glyph {g} has no post name"))
        })
        .collect::<Res<_>>()?;
    names.extend(src.glyphs.iter().map(|g| g.name.clone()));
    let mut gid: HashMap<&str, u32> = HashMap::new();
    for (i, n) in names.iter().enumerate() {
        if gid.insert(n.as_str(), i as u32).is_some() {
            return Err(format!("glyph name {n} repeats"));
        }
    }
    let id = |n: &str| -> Res<GlyphId16> {
        gid.get(n)
            .map(|g| GlyphId16::new(*g as u16))
            .ok_or_else(|| format!("no glyph named {n}"))
    };
    let total = names.len() as u32;

    // glyf and loca, and what maxp, head and hhea are computed from.
    let loca = f.loca(None).map_err(e("loca"))?;
    let glyf = f.glyf().map_err(e("glyf"))?;
    let mut owned: Vec<Option<Glyph>> = vec![];
    let mut shapes: Vec<Shape> = vec![];
    for g in 0..n_base {
        let read = loca
            .get_glyf(GlyphId::new(g), &glyf)
            .map_err(e("base glyf"))?;
        let (glyph, shape) = match &read {
            None => (Glyph::Empty, Shape::Empty),
            Some(ReadGlyph::Simple(s)) => (
                Glyph::Simple(s.to_owned_table()),
                Shape::Simple {
                    bbox: Bbox {
                        x_min: s.x_min(),
                        y_min: s.y_min(),
                        x_max: s.x_max(),
                        y_max: s.y_max(),
                    },
                    points: s.num_points() as u16,
                    contours: s.number_of_contours() as u16,
                },
            ),
            Some(ReadGlyph::Composite(c)) => (
                Glyph::Composite(c.to_owned_table()),
                Shape::Composite {
                    comps: c.components().map(|k| (k.glyph.to_u32(), (0, 0))).collect(),
                    stored: Some(Bbox {
                        x_min: c.x_min(),
                        y_min: c.y_min(),
                        x_max: c.x_max(),
                        y_max: c.y_max(),
                    }),
                },
            ),
        };
        owned.push(Some(glyph));
        shapes.push(shape);
    }
    for g in &src.glyphs {
        match &g.outline {
            Outline::Empty => {
                owned.push(Some(Glyph::Empty));
                shapes.push(Shape::Empty);
            }
            Outline::Simple {
                contours,
                instructions,
                overlap,
            } => {
                let pts: Vec<&(i16, i16, bool)> = contours.iter().flatten().collect();
                let bbox = pts
                    .iter()
                    .fold(None, |b: Option<Bbox>, (x, y, _)| {
                        Some(union(
                            b,
                            Bbox {
                                x_min: *x,
                                y_min: *y,
                                x_max: *x,
                                y_max: *y,
                            },
                        ))
                    })
                    .ok_or_else(|| format!("{}: a contour with no points", g.name))?;
                let built = SimpleGlyph {
                    bbox,
                    contours: contours
                        .iter()
                        .map(|c| {
                            Contour::from(
                                c.iter()
                                    .map(|(x, y, on)| CurvePoint::new(*x, *y, *on))
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect(),
                    instructions: instructions.clone(),
                    overlaps: *overlap,
                };
                shapes.push(Shape::Simple {
                    bbox,
                    points: pts.len() as u16,
                    contours: contours.len() as u16,
                });
                owned.push(Some(Glyph::Simple(built)));
            }
            Outline::Composite(cs) => {
                let comps = cs
                    .iter()
                    .map(|c| Ok((id(&c.glyph)?.to_u32(), (c.dx, c.dy))))
                    .collect::<Res<Vec<_>>>()?;
                shapes.push(Shape::Composite {
                    comps,
                    stored: None,
                });
                owned.push(None);
            }
        }
    }
    let metrics = measure(&shapes)?;
    for ((g, slot), m) in src
        .glyphs
        .iter()
        .zip(&mut owned[n_base as usize..])
        .zip(&metrics[n_base as usize..])
    {
        if slot.is_some() {
            continue;
        }
        let Outline::Composite(cs) = &g.outline else {
            unreachable!("only a composite waits for its box")
        };
        let bbox = m
            .bbox
            .ok_or_else(|| format!("{}: a composite of nothing drawn", g.name))?;
        let mut built: Option<CompositeGlyph> = None;
        for c in cs {
            let k = Component::new(
                GlyphId::from(id(&c.glyph)?),
                CompAnchor::Offset { x: c.dx, y: c.dy },
                Transform::default(),
                flags(c.flags, &g.name)?,
            );
            match &mut built {
                None => built = Some(CompositeGlyph::new(k, bbox)),
                Some(b) => b.add_component(k, bbox),
            }
        }
        *slot = Some(Glyph::Composite(
            built.ok_or_else(|| format!("{}: no components", g.name))?,
        ));
    }
    let mut data: Vec<u8> = vec![];
    let mut offsets = vec![0u32];
    for g in owned.iter().flatten() {
        data.extend(glyph_bytes(g)?);
        offsets.push(data.len() as u32);
    }
    let loca_bytes: Vec<u8> = offsets.iter().flat_map(|o| o.to_be_bytes()).collect();

    // hmtx: the stock metrics, then each new glyph's advance with its
    // xMin as its side bearing.
    let hmtx = f.hmtx().map_err(e("hmtx"))?;
    let mut adv_lsb: Vec<(u16, i16)> = (0..n_base)
        .map(|g| {
            let g = GlyphId::new(g);
            Ok((
                hmtx.advance(g).ok_or("hmtx: no advance")?,
                hmtx.side_bearing(g).ok_or("hmtx: no side bearing")?,
            ))
        })
        .collect::<Res<_>>()?;
    for (g, m) in src.glyphs.iter().zip(&metrics[n_base as usize..]) {
        adv_lsb.push((g.advance, m.bbox.map_or(0, |b| b.x_min)));
    }
    // fontTools' rule: the trailing run of glyphs whose advance equals the
    // last glyph's keeps only its side bearings.
    let last = adv_lsb.last().map_or(0, |m| m.0);
    let mut long = adv_lsb.len();
    while long > 1 && adv_lsb[long - 2].0 == last {
        long -= 1;
    }
    let out_hmtx = Hmtx::new(
        adv_lsb[..long]
            .iter()
            .map(|(a, l)| LongMetric::new(*a, *l))
            .collect(),
        adv_lsb[long..].iter().map(|(_, l)| *l).collect(),
    );

    // hhea, maxp and head, recomputed as fontTools' recalc does.
    let mut hhea: Hhea = f.hhea().map_err(e("hhea"))?.to_owned_table();
    hhea.number_of_h_metrics = long as u16;
    hhea.advance_width_max = adv_lsb.iter().map(|m| m.0).max().unwrap_or(0).into();
    let drawn: Vec<(usize, Bbox)> = metrics
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.bbox.map(|b| (i, b)))
        .collect();
    if !drawn.is_empty() {
        let w = |b: &Bbox| b.x_max as i32 - b.x_min as i32;
        let lsb = drawn.iter().map(|(i, _)| adv_lsb[*i].1 as i32).min();
        let rsb = drawn
            .iter()
            .map(|(i, b)| adv_lsb[*i].0 as i32 - adv_lsb[*i].1 as i32 - w(b))
            .min();
        let ext = drawn.iter().map(|(i, b)| adv_lsb[*i].1 as i32 + w(b)).max();
        hhea.min_left_side_bearing = (lsb.unwrap_or(0) as i16).into();
        hhea.min_right_side_bearing = (rsb.unwrap_or(0) as i16).into();
        hhea.x_max_extent = (ext.unwrap_or(0) as i16).into();
    }

    let mut maxp: Maxp = f.maxp().map_err(e("maxp"))?.to_owned_table();
    maxp.num_glyphs = total as u16;
    let simple = metrics.iter().filter(|m| !m.composite);
    let comps = metrics.iter().filter(|m| m.composite);
    maxp.max_points = Some(simple.clone().map(|m| m.points).max().unwrap_or(0));
    maxp.max_contours = Some(simple.map(|m| m.contours).max().unwrap_or(0));
    maxp.max_composite_points = Some(comps.clone().map(|m| m.points).max().unwrap_or(0));
    maxp.max_composite_contours = Some(comps.clone().map(|m| m.contours).max().unwrap_or(0));
    maxp.max_component_depth = Some(comps.map(|m| m.depth).max().unwrap_or(0));
    maxp.max_component_elements = Some(max_elements(&f, src)?);

    let mut head: Head = f.head().map_err(e("head"))?.to_owned_table();
    let all = drawn.iter().fold(None, |b, (_, x)| Some(union(b, *x)));
    let all = all.unwrap_or_default();
    (head.x_min, head.y_min, head.x_max, head.y_max) = (all.x_min, all.y_min, all.x_max, all.y_max);
    if drawn.iter().all(|(i, b)| adv_lsb[*i].1 == b.x_min) {
        head.flags |= write_fonts::tables::head::Flags::LSB_AT_X_0;
    }
    head.modified = read_fonts::types::LongDateTime::new(src.head_modified);
    head.index_to_loc_format = 1;

    // cmap: every stock mapping and the new ones.
    let mut cmap: BTreeMap<u32, GlyphId> = stock_mappings(&f)?;
    for (g, i) in src.glyphs.iter().zip(n_base..) {
        for u in &g.unicodes {
            if cmap.insert(*u, GlyphId::new(i)).is_some() {
                return Err(format!("U+{u:04X} is mapped already"));
            }
        }
    }
    let out_cmap = Cmap::from_mappings(
        cmap.iter()
            .map(|(u, g)| Ok((char::from_u32(*u).ok_or("cmap: not a char")?, *g)))
            .collect::<Res<Vec<_>>>()?,
    )
    .map_err(e("cmap"))?;

    // OS/2: the source's fields, and the first and last character.
    let mut os2: Os2 = f.os2().map_err(e("OS/2"))?.to_owned_table();
    for (field, v) in &src.os2 {
        *match field.as_str() {
            "ulUnicodeRange1" => &mut os2.ul_unicode_range_1,
            "ulUnicodeRange2" => &mut os2.ul_unicode_range_2,
            "ulUnicodeRange3" => &mut os2.ul_unicode_range_3,
            "ulUnicodeRange4" => &mut os2.ul_unicode_range_4,
            "ulCodePageRange1" => os2.ul_code_page_range_1.get_or_insert(0),
            "ulCodePageRange2" => os2.ul_code_page_range_2.get_or_insert(0),
            other => return Err(format!("OS/2 field {other}")),
        } = *v;
    }
    if let (Some(first), Some(last)) = (cmap.keys().next(), cmap.keys().last()) {
        os2.us_first_char_index = (*first).min(0xFFFF) as u16;
        os2.us_last_char_index = (*last).min(0xFFFF) as u16;
    }

    // name: the source's records over the stock ones.
    let mut name: Name = f.name().map_err(e("name"))?.to_owned_table();
    for n in &src.names {
        let rec = NameRecord::new(
            n.platform,
            n.encoding,
            n.language,
            NameId::new(n.id),
            n.text.clone().into(),
        );
        match name.name_record.iter_mut().find(|r| {
            (r.platform_id, r.encoding_id, r.language_id, r.name_id)
                == (n.platform, n.encoding, n.language, NameId::new(n.id))
        }) {
            Some(r) => *r = rec,
            None => name.name_record.push(rec),
        }
    }
    name.name_record.sort_by_key(|r| {
        (
            r.platform_id,
            r.encoding_id,
            r.language_id,
            r.name_id.to_u16(),
        )
    });

    // post: the stock fields, with every glyph's name.
    let stock_post: Post = post.to_owned_table();
    let mut out_post = Post::new_v2(names.iter().map(String::as_str));
    out_post.italic_angle = stock_post.italic_angle;
    out_post.underline_position = stock_post.underline_position;
    out_post.underline_thickness = stock_post.underline_thickness;
    out_post.is_fixed_pitch = stock_post.is_fixed_pitch;
    out_post.min_mem_type42 = stock_post.min_mem_type42;
    out_post.max_mem_type42 = stock_post.max_mem_type42;
    out_post.min_mem_type1 = stock_post.min_mem_type1;
    out_post.max_mem_type1 = stock_post.max_mem_type1;

    // GDEF: each new glyph's class, and the appended mark glyph sets.
    let mut gdef: Gdef = f.gdef().map_err(e("GDEF"))?.to_owned_table();
    let read_gdef = f.gdef().map_err(e("GDEF"))?;
    let mut classes: Vec<(GlyphId16, u16)> = match read_gdef.glyph_class_def() {
        Some(c) => c
            .map_err(e("GDEF classes"))?
            .iter()
            .map(|(g, k)| (GlyphId16::new(g.to_u32() as u16), k as u16))
            .collect(),
        None => vec![],
    };
    for (g, i) in src.glyphs.iter().zip(n_base..) {
        if g.class != 0 {
            classes.push((GlyphId16::new(i as u16), g.class));
        }
    }
    gdef.glyph_class_def = Some(classes.into_iter().collect::<ClassDef>()).into();
    if !src.mark_sets.is_empty() {
        let mut sets: MarkGlyphSets = gdef
            .mark_glyph_sets_def
            .as_ref()
            .cloned()
            .unwrap_or_default();
        for s in &src.mark_sets {
            let cov: CoverageTable = s.iter().map(|g| id(g)).collect::<Res<Vec<_>>>()?.into();
            sets.coverages.push(cov.into());
        }
        gdef.mark_glyph_sets_def = Some(sets).into();
    }

    // GSUB and GPOS: scripts, features and the appended lookups.
    let read_gsub = f.gsub().map_err(e("GSUB"))?;
    let read_gpos = f.gpos().map_err(e("GPOS"))?;
    let mut gsub: Gsub = read_gsub.to_owned_table();
    let mut gpos: Gpos = read_gpos.to_owned_table();
    for (t, tag, from) in &src.scripts {
        let (records, list) = match t.as_str() {
            "GSUB" => (
                &mut gsub
                    .script_list
                    .as_mut()
                    .ok_or("GSUB: no scripts")?
                    .script_records,
                read_gsub.script_list().map_err(e("GSUB"))?,
            ),
            _ => (
                &mut gpos
                    .script_list
                    .as_mut()
                    .ok_or("GPOS: no scripts")?
                    .script_records,
                read_gpos.script_list().map_err(e("GPOS"))?,
            ),
        };
        let rec = list
            .script_records()
            .iter()
            .find(|r| r.script_tag() == tag4(from))
            .ok_or_else(|| format!("{t}: no {from} script"))?;
        let script: ReadScript = rec.script(list.offset_data()).map_err(e("script"))?;
        let script: Script = script.to_owned_table();
        if records.iter().any(|r| r.script_tag == tag4(tag)) {
            return Err(format!("{t}: {tag} is there already"));
        }
        records.push(ScriptRecord::new(tag4(tag), script));
        records.sort_by_key(|r| r.script_tag);
    }
    for (t, i, tag, lookups) in &src.features {
        let list = match t.as_str() {
            "GSUB" => gsub.feature_list.as_mut().ok_or("GSUB: no features")?,
            _ => gpos.feature_list.as_mut().ok_or("GPOS: no features")?,
        };
        let rec = list
            .feature_records
            .get_mut(*i as usize)
            .ok_or_else(|| format!("{t}: no feature {i}"))?;
        if rec.feature_tag != tag4(tag) {
            return Err(format!(
                "{t}: feature {i} is {}, not {tag}",
                rec.feature_tag
            ));
        }
        rec.feature.lookup_list_indices = lookups.clone();
    }
    let lookups = &mut gpos.lookup_list.as_mut().ok_or("GPOS: no lookups")?.lookups;
    for l in &src.lookups {
        if l.index as usize != lookups.len() {
            return Err(format!(
                "lookup {} would be GPOS lookup {}",
                l.index,
                lookups.len()
            ));
        }
        let flag = LookupFlag::from_bits_truncate(l.flag);
        let built = match l.kind {
            Kind::MarkBase => {
                let subs = l
                    .subtables
                    .iter()
                    .map(|s| {
                        attach(s, &id).map(|(m, mc, b, bc)| {
                            MarkBasePos::Format1(MarkBasePosFormat1::new(
                                mc,
                                bc,
                                m,
                                BaseArray::new(b.into_iter().map(BaseRecord::new).collect()),
                            ))
                        })
                    })
                    .collect::<Res<Vec<_>>>()?;
                let mut k = Lookup::new(flag, subs);
                k.mark_filtering_set = l.mark_set;
                PositionLookup::MarkToBase(k)
            }
            Kind::MarkMark => {
                let subs = l
                    .subtables
                    .iter()
                    .map(|s| {
                        attach(s, &id).map(|(m, mc, b, bc)| {
                            MarkMarkPos::Format1(MarkMarkPosFormat1::new(
                                mc,
                                bc,
                                m,
                                Mark2Array::new(b.into_iter().map(Mark2Record::new).collect()),
                            ))
                        })
                    })
                    .collect::<Res<Vec<_>>>()?;
                let mut k = Lookup::new(flag, subs);
                k.mark_filtering_set = l.mark_set;
                PositionLookup::MarkToMark(k)
            }
            Kind::Chain => {
                let subs = l
                    .subtables
                    .iter()
                    .map(|s| chain(s, &id))
                    .collect::<Res<Vec<_>>>()?;
                let mut k = Lookup::new(flag, subs);
                k.mark_filtering_set = l.mark_set;
                PositionLookup::ChainContextual(k)
            }
        };
        lookups.push(built.into());
    }

    let mut fb = FontBuilder::new();
    fb.add_raw(Tag::new(b"glyf"), data);
    fb.add_raw(Tag::new(b"loca"), loca_bytes);
    fb.add_table(&head).map_err(e("head"))?;
    fb.add_table(&hhea).map_err(e("hhea"))?;
    fb.add_table(&maxp).map_err(e("maxp"))?;
    fb.add_table(&out_hmtx).map_err(e("hmtx"))?;
    fb.add_table(&out_cmap).map_err(e("cmap"))?;
    fb.add_table(&os2).map_err(e("OS/2"))?;
    fb.add_table(&name).map_err(e("name"))?;
    fb.add_table(&out_post).map_err(e("post"))?;
    fb.add_table(&gdef).map_err(e("GDEF"))?;
    // GSUB: stock JetBrains Mono's lookups, the block of them and every
    // subtable they name, byte for byte under the lists written again (the
    // stage adds no GSUB lookup), as `baud-app`'s `keeps_lookups` holds it.
    let stock_gsub = f.table_data(Tag::new(b"GSUB")).ok_or("no GSUB")?;
    let (gsub_bytes, _) = crate::merge::spliced(
        stock_gsub.as_bytes(),
        &gsub
            .script_list
            .as_ref()
            .ok_or("GSUB: no scripts")?
            .script_records,
        &gsub
            .feature_list
            .as_ref()
            .ok_or("GSUB: no features")?
            .feature_records,
        vec![],
        gsub.lookup_list.as_ref().map_or(0, |l| l.lookups.len()),
    )?;
    fb.add_raw(Tag::new(b"GSUB"), gsub_bytes);
    fb.add_table(&gpos).map_err(e("GPOS"))?;
    for t in [b"gasp", b"cvt ", b"fpgm", b"prep"] {
        if let Some(d) = f.table_data(Tag::new(t)) {
            fb.add_raw(Tag::new(t), d.as_bytes().to_vec());
        }
    }
    let have: Vec<Tag> = f
        .table_directory()
        .table_records()
        .iter()
        .map(|r| r.tag())
        .collect();
    const WRITTEN: &[&[u8; 4]] = &[
        b"glyf", b"loca", b"head", b"hhea", b"maxp", b"hmtx", b"cmap", b"OS/2", b"name", b"post",
        b"GDEF", b"GSUB", b"GPOS", b"gasp", b"cvt ", b"fpgm", b"prep",
    ];
    if let Some(t) = have
        .iter()
        .find(|t| !WRITTEN.iter().any(|w| Tag::new(w) == **t))
    {
        return Err(format!("base: table {t} is not one this build writes"));
    }
    Ok(fb.build())
}

enum Shape {
    Empty,
    Simple {
        bbox: Bbox,
        points: u16,
        contours: u16,
    },
    /// Each component's glyph and offset, and the box the font stores, if
    /// it is a stock glyph.
    Composite {
        comps: Vec<(u32, (i16, i16))>,
        stored: Option<Bbox>,
    },
}

/// Every glyph's maxp counts and box. A composite counts as fontTools'
/// getCompositeMaxpValues counts it: points and contours summed through
/// nested components, depth one more than its deepest composite
/// component's. A stock composite keeps the box it stores; a new one's is
/// the union of its components' boxes moved by their offsets.
fn measure(shapes: &[Shape]) -> Res<Vec<Metrics>> {
    fn one(g: usize, shapes: &[Shape], memo: &mut Vec<Option<Metrics>>, depth: usize) -> Res<()> {
        if memo[g].is_some() {
            return Ok(());
        }
        if depth > 64 {
            return Err(format!("glyph {g}: components nest past 64"));
        }
        let m = match &shapes[g] {
            Shape::Empty => Metrics::empty(),
            Shape::Simple {
                bbox,
                points,
                contours,
            } => Metrics {
                bbox: Some(*bbox),
                points: *points,
                contours: *contours,
                depth: 0,
                composite: false,
            },
            Shape::Composite { comps, stored } => {
                let mut m = Metrics {
                    bbox: None,
                    points: 0,
                    contours: 0,
                    depth: 1,
                    composite: true,
                };
                for (c, (dx, dy)) in comps {
                    let c = *c as usize;
                    if c >= shapes.len() {
                        return Err(format!("glyph {g}: a component names glyph {c}"));
                    }
                    one(c, shapes, memo, depth + 1)?;
                    let k = memo[c].as_ref().expect("measured above");
                    m.points += k.points;
                    m.contours += k.contours;
                    if k.composite {
                        m.depth = m.depth.max(k.depth + 1);
                    }
                    if let Some(b) = k.bbox {
                        m.bbox = Some(union(
                            m.bbox,
                            Bbox {
                                x_min: b.x_min + dx,
                                y_min: b.y_min + dy,
                                x_max: b.x_max + dx,
                                y_max: b.y_max + dy,
                            },
                        ));
                    }
                }
                if stored.is_some() {
                    m.bbox = *stored;
                }
                m
            }
        };
        memo[g] = Some(m);
        Ok(())
    }
    let mut memo: Vec<Option<Metrics>> = (0..shapes.len()).map(|_| None).collect();
    for g in 0..shapes.len() {
        one(g, shapes, &mut memo, 0)?;
    }
    Ok(memo.into_iter().map(|m| m.expect("measured")).collect())
}

impl Metrics {
    fn empty() -> Metrics {
        Metrics {
            bbox: None,
            points: 0,
            contours: 0,
            depth: 0,
            composite: false,
        }
    }
}

/// maxComponentElements: the most components any composite has.
fn max_elements(f: &FontRef, src: &Source) -> Res<u16> {
    let loca = f.loca(None).map_err(e("loca"))?;
    let glyf = f.glyf().map_err(e("glyf"))?;
    let n = f.maxp().map_err(e("maxp"))?.num_glyphs() as u32;
    let mut most = 0;
    for g in 0..n {
        if let Some(ReadGlyph::Composite(c)) =
            loca.get_glyf(GlyphId::new(g), &glyf).map_err(e("glyf"))?
        {
            most = most.max(c.components().count() as u16);
        }
    }
    for g in &src.glyphs {
        if let Outline::Composite(cs) = &g.outline {
            most = most.max(cs.len() as u16);
        }
    }
    Ok(most)
}

fn flags(bits: u16, glyph: &str) -> Res<ComponentFlags> {
    const KNOWN: u16 = 0x0004 | 0x0200 | 0x0400 | 0x0800 | 0x1000;
    if bits & !KNOWN != 0 {
        return Err(format!("{glyph}: component flags {bits:#06x}"));
    }
    Ok(ComponentFlags {
        round_xy_to_grid: bits & 0x0004 != 0,
        use_my_metrics: bits & 0x0200 != 0,
        overlap_compound: bits & 0x0400 != 0,
        scaled_component_offset: bits & 0x0800 != 0,
        unscaled_component_offset: bits & 0x1000 != 0,
    })
}

fn tag4(s: &str) -> Tag {
    let mut b = [b' '; 4];
    for (i, c) in s.bytes().take(4).enumerate() {
        b[i] = c;
    }
    Tag::new(&b)
}

/// The stock face's mappings: its full-repertoire subtable's, which must
/// hold every BMP subtable's mapping too.
fn stock_mappings(f: &FontRef) -> Res<BTreeMap<u32, GlyphId>> {
    let cmap = f.cmap().map_err(e("cmap"))?;
    let mut by: Vec<BTreeMap<u32, GlyphId>> = vec![];
    for r in cmap.encoding_records() {
        let sub = r.subtable(cmap.offset_data()).map_err(e("cmap"))?;
        // A code point mapped to glyph 0 is unmapped (the cmap chapter:
        // glyph 0 is the missing glyph); read-fonts yields the format 4
        // table's closing 0xFFFF segment as such a mapping.
        by.push(sub.iter().filter(|(_, g)| g.to_u32() != 0).collect());
    }
    let full = by
        .iter()
        .max_by_key(|m| m.len())
        .cloned()
        .ok_or("cmap: no subtables")?;
    for m in &by {
        if m.iter().any(|(u, g)| full.get(u) != Some(g)) {
            return Err("cmap: the stock subtables disagree".into());
        }
    }
    Ok(full)
}

type Attached = (
    MarkArray,
    CoverageTable,
    Vec<Vec<Option<AnchorTable>>>,
    CoverageTable,
);

/// A mark-attachment subtable's parts, each array in its coverage's
/// glyph order.
fn attach(s: &Subtable, id: &dyn Fn(&str) -> Res<GlyphId16>) -> Res<Attached> {
    let Subtable::Attach { marks, bases } = s else {
        return Err("a chain subtable in a mark lookup".into());
    };
    let mut m: Vec<(GlyphId16, MarkRecord)> = marks
        .iter()
        .map(|(g, c, (x, y))| Ok((id(g)?, MarkRecord::new(*c, AnchorTable::format_1(*x, *y)))))
        .collect::<Res<_>>()?;
    m.sort_by_key(|(g, _)| *g);
    let mut b: Vec<(GlyphId16, Vec<Option<AnchorTable>>)> = bases
        .iter()
        .map(|(g, a)| {
            Ok((
                id(g)?,
                a.iter()
                    .map(|a| a.map(|(x, y)| AnchorTable::format_1(x, y)))
                    .collect(),
            ))
        })
        .collect::<Res<_>>()?;
    b.sort_by_key(|(g, _)| *g);
    let mc: CoverageTable = m.iter().map(|(g, _)| *g).collect::<Vec<_>>().into();
    let bc: CoverageTable = b.iter().map(|(g, _)| *g).collect::<Vec<_>>().into();
    Ok((
        MarkArray::new(m.into_iter().map(|(_, r)| r).collect()),
        mc,
        b.into_iter().map(|(_, a)| a).collect(),
        bc,
    ))
}

fn chain(s: &Subtable, id: &dyn Fn(&str) -> Res<GlyphId16>) -> Res<PositionChainContext> {
    let Subtable::Chain {
        backtrack,
        input,
        lookahead,
        apply,
    } = s
    else {
        return Err("a mark subtable in a chain lookup".into());
    };
    let covs = |seq: &Vec<Vec<String>>| -> Res<Vec<CoverageTable>> {
        seq.iter()
            .map(|c| {
                let mut g = c.iter().map(|g| id(g)).collect::<Res<Vec<_>>>()?;
                g.sort();
                Ok(g.into())
            })
            .collect()
    };
    Ok(PositionChainContext::from(ChainedSequenceContext::Format3(
        ChainedSequenceContextFormat3::new(
            covs(backtrack)?,
            covs(input)?,
            covs(lookahead)?,
            apply
                .iter()
                .map(|(s, l)| SequenceLookupRecord::new(*s, *l))
                .collect(),
        ),
    )))
}
