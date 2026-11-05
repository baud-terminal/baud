//! A stage font read back into a [`Source`] against the stock face it was
//! built on. Only what the source can say is read: anything else that
//! differs is found by [`super::extract`]'s own check, which builds the
//! source again and compares the result with the stage.

use std::collections::BTreeMap;

use read_fonts::tables::glyf::Glyph as ReadGlyph;
use read_fonts::tables::gpos::{AnchorTable, MarkBasePos, MarkMarkPos, PositionSubtables};
use read_fonts::tables::layout::{ChainedSequenceContext, CoverageTable, Script};
use read_fonts::types::{GlyphId, GlyphId16};
use read_fonts::{FontRef, ReadError, TableProvider};

use super::text::{Component, Glyph, Kind, Lookup, Name, Outline, Source, Subtable};

type Res<T> = Result<T, String>;

fn e(what: &'static str) -> impl Fn(ReadError) -> String {
    move |err| format!("{what}: {err}")
}

fn names(f: &FontRef) -> Res<Vec<String>> {
    let post = f.post().map_err(e("post"))?;
    let n = f.maxp().map_err(e("maxp"))?.num_glyphs();
    (0..n)
        .map(|g| {
            post.glyph_name(GlyphId16::new(g))
                .map(str::to_string)
                .ok_or_else(|| format!("glyph {g} has no post name"))
        })
        .collect()
}

fn mappings(f: &FontRef) -> Res<BTreeMap<u32, GlyphId>> {
    let cmap = f.cmap().map_err(e("cmap"))?;
    let mut all = BTreeMap::new();
    for r in cmap.encoding_records() {
        let sub = r.subtable(cmap.offset_data()).map_err(e("cmap"))?;
        all.extend(sub.iter().filter(|(_, g)| g.to_u32() != 0));
    }
    Ok(all)
}

fn script_key(s: &Script) -> Res<String> {
    let lang = |l: read_fonts::tables::layout::LangSys| {
        format!(
            "{}:{:?}",
            l.required_feature_index(),
            l.feature_indices()
                .iter()
                .map(|i| i.get())
                .collect::<Vec<_>>()
        )
    };
    let mut k = match s.default_lang_sys() {
        Some(l) => lang(l.map_err(e("LangSys"))?),
        None => "-".into(),
    };
    for r in s.lang_sys_records() {
        let l = r.lang_sys(s.offset_data()).map_err(e("LangSys"))?;
        k.push_str(&format!(" {}={}", r.lang_sys_tag(), lang(l)));
    }
    Ok(k)
}

pub fn read(base: &[u8], stage: &[u8], style: &str) -> Res<Source> {
    let b = FontRef::new(base).map_err(e("base"))?;
    let s = FontRef::new(stage).map_err(e("stage"))?;
    let bn = names(&b)?;
    let sn = names(&s)?;
    if sn.len() < bn.len() || sn[..bn.len()] != bn[..] {
        return Err("the stage does not start with the stock glyph order".into());
    }
    let name_of = |g: GlyphId| -> Res<String> {
        sn.get(g.to_u32() as usize)
            .cloned()
            .ok_or_else(|| format!("glyph {g} is past the stage's"))
    };
    let mut src = Source {
        style: style.to_string(),
        head_modified: s.head().map_err(e("head"))?.modified().as_secs(),
        ..Source::default()
    };

    // OS/2 and name.
    let (bo, so) = (b.os2().map_err(e("OS/2"))?, s.os2().map_err(e("OS/2"))?);
    let fields = [
        (
            "ulUnicodeRange1",
            bo.ul_unicode_range_1(),
            so.ul_unicode_range_1(),
        ),
        (
            "ulUnicodeRange2",
            bo.ul_unicode_range_2(),
            so.ul_unicode_range_2(),
        ),
        (
            "ulUnicodeRange3",
            bo.ul_unicode_range_3(),
            so.ul_unicode_range_3(),
        ),
        (
            "ulUnicodeRange4",
            bo.ul_unicode_range_4(),
            so.ul_unicode_range_4(),
        ),
        (
            "ulCodePageRange1",
            bo.ul_code_page_range_1().unwrap_or(0),
            so.ul_code_page_range_1().unwrap_or(0),
        ),
        (
            "ulCodePageRange2",
            bo.ul_code_page_range_2().unwrap_or(0),
            so.ul_code_page_range_2().unwrap_or(0),
        ),
    ];
    for (f, x, y) in fields {
        if x != y {
            src.os2.push((f.to_string(), y));
        }
    }
    let records = |f: &FontRef| -> Res<BTreeMap<(u16, u16, u16, u16), String>> {
        let name = f.name().map_err(e("name"))?;
        name.name_record()
            .iter()
            .map(|r| {
                let text = r
                    .string(name.string_data())
                    .map_err(e("name"))?
                    .chars()
                    .collect::<String>();
                Ok((
                    (
                        r.name_id().to_u16(),
                        r.platform_id(),
                        r.encoding_id(),
                        r.language_id(),
                    ),
                    text,
                ))
            })
            .collect()
    };
    let (br, sr) = (records(&b)?, records(&s)?);
    if let Some(k) = br.keys().find(|k| !sr.contains_key(k)) {
        return Err(format!("the stage drops name record {k:?}"));
    }
    for ((id, platform, encoding, language), text) in &sr {
        if br.get(&(*id, *platform, *encoding, *language)) != Some(text) {
            src.names.push(Name {
                id: *id,
                platform: *platform,
                encoding: *encoding,
                language: *language,
                text: text.clone(),
            });
        }
    }

    // The appended glyphs.
    let classes: BTreeMap<GlyphId16, u16> = match s.gdef().map_err(e("GDEF"))?.glyph_class_def() {
        Some(c) => c
            .map_err(e("GDEF"))?
            .iter()
            .map(|(g, k)| (GlyphId16::new(g.to_u32() as u16), k as u16))
            .collect(),
        None => BTreeMap::new(),
    };
    let mut unicodes: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let stock = mappings(&b)?;
    for (u, g) in mappings(&s)? {
        if stock.contains_key(&u) {
            continue;
        }
        unicodes.entry(g.to_u32()).or_default().push(u);
    }
    let loca = s.loca(None).map_err(e("loca"))?;
    let glyf = s.glyf().map_err(e("glyf"))?;
    let hmtx = s.hmtx().map_err(e("hmtx"))?;
    for g in bn.len()..sn.len() {
        let gid = GlyphId::new(g as u32);
        let outline = match loca.get_glyf(gid, &glyf).map_err(e("glyf"))? {
            None => Outline::Empty,
            Some(ReadGlyph::Simple(sg)) => {
                let pts: Vec<_> = sg.points().collect();
                let mut contours = vec![];
                let mut start = 0;
                for end in sg.end_pts_of_contours() {
                    let end = end.get() as usize + 1;
                    contours.push(
                        pts[start..end]
                            .iter()
                            .map(|p| (p.x, p.y, p.on_curve))
                            .collect(),
                    );
                    start = end;
                }
                Outline::Simple {
                    contours,
                    instructions: sg.instructions().to_vec(),
                    overlap: sg.has_overlapping_contours(),
                }
            }
            Some(ReadGlyph::Composite(c)) => {
                if c.instructions().is_some_and(|i| !i.is_empty()) {
                    return Err(format!("{}: a composite with instructions", sn[g]));
                }
                let mut out = vec![];
                for k in c.components() {
                    let read_fonts::tables::glyf::Anchor::Offset { x, y } = k.anchor else {
                        return Err(format!("{}: a component placed by points", sn[g]));
                    };
                    out.push(Component {
                        glyph: sn
                            .get(k.glyph.to_u32() as usize)
                            .cloned()
                            .ok_or("a component past the glyph order")?,
                        dx: x,
                        dy: y,
                        flags: k.flags.bits() & (0x0004 | 0x0200 | 0x0400 | 0x0800 | 0x1000),
                    });
                }
                Outline::Composite(out)
            }
        };
        src.glyphs.push(Glyph {
            name: sn[g].clone(),
            advance: hmtx.advance(gid).ok_or("hmtx: no advance")?,
            class: classes.get(&GlyphId16::new(g as u16)).copied().unwrap_or(0),
            unicodes: unicodes.remove(&(g as u32)).unwrap_or_default(),
            outline,
        });
    }
    if let Some((g, u)) = unicodes.iter().next() {
        return Err(format!("U+{:04X} newly maps the stock glyph {g}", u[0]));
    }

    // Scripts and features.
    for (tag, list, flist, blist, bflist) in [
        (
            "GSUB",
            s.gsub()
                .map_err(e("GSUB"))?
                .script_list()
                .map_err(e("GSUB"))?,
            s.gsub()
                .map_err(e("GSUB"))?
                .feature_list()
                .map_err(e("GSUB"))?,
            b.gsub()
                .map_err(e("GSUB"))?
                .script_list()
                .map_err(e("GSUB"))?,
            b.gsub()
                .map_err(e("GSUB"))?
                .feature_list()
                .map_err(e("GSUB"))?,
        ),
        (
            "GPOS",
            s.gpos()
                .map_err(e("GPOS"))?
                .script_list()
                .map_err(e("GPOS"))?,
            s.gpos()
                .map_err(e("GPOS"))?
                .feature_list()
                .map_err(e("GPOS"))?,
            b.gpos()
                .map_err(e("GPOS"))?
                .script_list()
                .map_err(e("GPOS"))?,
            b.gpos()
                .map_err(e("GPOS"))?
                .feature_list()
                .map_err(e("GPOS"))?,
        ),
    ] {
        let mut stock: Vec<(String, String)> = vec![];
        for r in blist.script_records() {
            let sc = r.script(blist.offset_data()).map_err(e("script"))?;
            stock.push((r.script_tag().to_string(), script_key(&sc)?));
        }
        for r in list.script_records() {
            let t = r.script_tag().to_string();
            if stock.iter().any(|(st, _)| *st == t) {
                continue;
            }
            let key = script_key(&r.script(list.offset_data()).map_err(e("script"))?)?;
            let from = stock
                .iter()
                .find(|(_, k)| *k == key)
                .ok_or_else(|| format!("{tag}: script {t} is no stock script's copy"))?;
            src.scripts.push((
                tag.to_string(),
                t.trim().to_string(),
                from.0.trim().to_string(),
            ));
        }
        let lists = |l: &read_fonts::tables::layout::FeatureList| -> Res<Vec<(String, Vec<u16>)>> {
            l.feature_records()
                .iter()
                .map(|r| {
                    let f = r.feature(l.offset_data()).map_err(e("feature"))?;
                    Ok((
                        r.feature_tag().to_string(),
                        f.lookup_list_indices().iter().map(|i| i.get()).collect(),
                    ))
                })
                .collect()
        };
        let (have, was) = (lists(&flist)?, lists(&bflist)?);
        if have.len() != was.len() || have.iter().zip(&was).any(|(a, b)| a.0 != b.0) {
            return Err(format!(
                "{tag}: the stage's feature records are not the stock ones"
            ));
        }
        for (i, ((t, now), (_, then))) in have.iter().zip(&was).enumerate() {
            if now != then {
                src.features
                    .push((tag.to_string(), i as u16, t.trim().to_string(), now.clone()));
            }
        }
    }

    // GDEF's appended mark sets.
    let sets = |f: &FontRef| -> Res<Vec<Vec<String>>> {
        let Some(m) = f.gdef().map_err(e("GDEF"))?.mark_glyph_sets_def() else {
            return Ok(vec![]);
        };
        let m = m.map_err(e("GDEF"))?;
        m.coverages()
            .iter()
            .map(|c| {
                let c = c.map_err(e("GDEF"))?;
                c.iter().map(&name_of).collect()
            })
            .collect()
    };
    let (have, was) = (sets(&s)?, sets(&b)?);
    if have.len() < was.len() || have[..was.len()] != was[..] {
        return Err("GDEF: the stock mark sets changed".into());
    }
    src.mark_sets = have[was.len()..].to_vec();

    // GPOS's appended lookups.
    let cov = |c: Result<CoverageTable, ReadError>| -> Res<Vec<String>> {
        c.map_err(e("coverage"))?.iter().map(&name_of).collect()
    };
    let anchor = |a: Result<AnchorTable, ReadError>| -> Res<(i16, i16)> {
        match a.map_err(e("anchor"))? {
            AnchorTable::Format1(a) => Ok((a.x_coordinate(), a.y_coordinate())),
            _ => Err("an anchor not in format 1".into()),
        }
    };
    let base_lookups = b
        .gpos()
        .map_err(e("GPOS"))?
        .lookup_list()
        .map_err(e("GPOS"))?
        .lookup_count();
    let gsub_counts = (
        b.gsub()
            .map_err(e("GSUB"))?
            .lookup_list()
            .map_err(e("GSUB"))?
            .lookup_count(),
        s.gsub()
            .map_err(e("GSUB"))?
            .lookup_list()
            .map_err(e("GSUB"))?
            .lookup_count(),
    );
    if gsub_counts.0 != gsub_counts.1 {
        return Err("GSUB: the stage appends lookups".into());
    }
    let list = s
        .gpos()
        .map_err(e("GPOS"))?
        .lookup_list()
        .map_err(e("GPOS"))?;
    for i in base_lookups..list.lookup_count() {
        let l = list.lookups().get(i as usize).map_err(e("lookup"))?;
        let flag = l.lookup_flag().to_bits();
        let mark_set = l.mark_filtering_set();
        let (kind, subtables) = match l.subtables().map_err(e("lookup"))? {
            PositionSubtables::MarkToBase(subs) => {
                let mut out = vec![];
                for st in subs.iter() {
                    let MarkBasePos::Format1(f) = st.map_err(e("mark-base"))? else {
                        return Err(format!("lookup {i}: a mark-base subtable not in format 1"));
                    };
                    let marks = f.mark_array().map_err(e("marks"))?;
                    let mc = cov(f.mark_coverage())?;
                    let mut m = vec![];
                    for (g, r) in mc.into_iter().zip(marks.mark_records()) {
                        m.push((
                            g,
                            r.mark_class(),
                            anchor(r.mark_anchor(marks.offset_data()))?,
                        ));
                    }
                    let bases = f.base_array().map_err(e("bases"))?;
                    let bc = cov(f.base_coverage())?;
                    let mut bs = vec![];
                    for (g, r) in bc.into_iter().zip(bases.base_records().iter()) {
                        let r = r.map_err(e("base"))?;
                        let a = r
                            .base_anchors(bases.offset_data())
                            .iter()
                            .map(|a| a.map(&anchor).transpose())
                            .collect::<Res<Vec<_>>>()?;
                        bs.push((g, a));
                    }
                    out.push(Subtable::Attach {
                        marks: m,
                        bases: bs,
                    });
                }
                (Kind::MarkBase, out)
            }
            PositionSubtables::MarkToMark(subs) => {
                let mut out = vec![];
                for st in subs.iter() {
                    let MarkMarkPos::Format1(f) = st.map_err(e("mark-mark"))? else {
                        return Err(format!("lookup {i}: a mark-mark subtable not in format 1"));
                    };
                    let marks = f.mark1_array().map_err(e("marks"))?;
                    let mc = cov(f.mark1_coverage())?;
                    let mut m = vec![];
                    for (g, r) in mc.into_iter().zip(marks.mark_records()) {
                        m.push((
                            g,
                            r.mark_class(),
                            anchor(r.mark_anchor(marks.offset_data()))?,
                        ));
                    }
                    let bases = f.mark2_array().map_err(e("mark2"))?;
                    let bc = cov(f.mark2_coverage())?;
                    let mut bs = vec![];
                    for (g, r) in bc.into_iter().zip(bases.mark2_records().iter()) {
                        let r = r.map_err(e("mark2"))?;
                        let a = r
                            .mark2_anchors(bases.offset_data())
                            .iter()
                            .map(|a| a.map(&anchor).transpose())
                            .collect::<Res<Vec<_>>>()?;
                        bs.push((g, a));
                    }
                    out.push(Subtable::Attach {
                        marks: m,
                        bases: bs,
                    });
                }
                (Kind::MarkMark, out)
            }
            PositionSubtables::ChainContextual(subs) => {
                let mut out = vec![];
                for st in subs.iter() {
                    let ChainedSequenceContext::Format3(f) = st.map_err(e("chain"))? else {
                        return Err(format!("lookup {i}: a chain subtable not in format 3"));
                    };
                    out.push(Subtable::Chain {
                        backtrack: f
                            .backtrack_coverages()
                            .iter()
                            .map(&cov)
                            .collect::<Res<_>>()?,
                        input: f.input_coverages().iter().map(&cov).collect::<Res<_>>()?,
                        lookahead: f
                            .lookahead_coverages()
                            .iter()
                            .map(&cov)
                            .collect::<Res<_>>()?,
                        apply: f
                            .seq_lookup_records()
                            .iter()
                            .map(|r| (r.sequence_index(), r.lookup_list_index()))
                            .collect(),
                    });
                }
                (Kind::Chain, out)
            }
            _ => return Err(format!("lookup {i}: a lookup type the source cannot say")),
        };
        src.lookups.push(Lookup {
            index: i,
            kind,
            flag,
            mark_set,
            subtables,
        });
    }
    Ok(src)
}
