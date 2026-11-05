//! Glyph order, glyf, gvar with every point's delta, hmtx and HVAR.

use read_fonts::tables::glyf::{Anchor, CompositeGlyph, Glyph, SimpleGlyph};
use read_fonts::tables::variations::DeltaSetIndex;
use read_fonts::types::{GlyphId, GlyphId16};
use read_fonts::{FontRef, ReadError, TableProvider};

use crate::dump::{Dump, Tokens};
use crate::var::{Samples, f2, num};

fn glyph_count(font: &FontRef) -> u32 {
    font.maxp().map_or(0, |m| m.num_glyphs() as u32)
}

/// Glyph names from font A's post table, for the report only: lines name
/// glyphs by id, so a renamed glyph shows once, in the glyph order.
pub struct GlyphNames(Vec<Option<String>>);

impl GlyphNames {
    pub fn of(font: &FontRef) -> GlyphNames {
        let post = font.post().ok();
        let names = (0..glyph_count(font))
            .map(|g| {
                let g16 = GlyphId16::new(g.try_into().ok()?);
                post.as_ref()?.glyph_name(g16).map(str::to_string)
            })
            .collect();
        GlyphNames(names)
    }

    pub fn label(&self, gid: u32) -> String {
        match self.0.get(gid as usize) {
            Some(Some(name)) => format!("glyph '{name}' (gid {gid})"),
            _ => format!("gid {gid}"),
        }
    }
}

/// The glyph count and each glyph's post name.
pub fn glyph_order(font: &FontRef) -> Dump {
    let mut d = Dump::default();
    let n = glyph_count(font);
    d.put("glyph count", n.to_string());
    let post = font.post().ok();
    for g in 0..n {
        let name = GlyphId16::new(g as u16);
        let name = post
            .as_ref()
            .and_then(|p| p.glyph_name(name))
            .unwrap_or("(none)");
        d.glyph(g, "post name", name);
    }
    d
}

fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".to_string();
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn glyph_at<'a>(font: &FontRef<'a>, g: u32) -> Result<Option<Glyph<'a>>, ReadError> {
    let loca = font.loca(None)?;
    let glyf = font.glyf()?;
    loca.get_glyf(GlyphId::new(g), &glyf)
}

fn simple(d: &mut Dump, g: u32, s: &SimpleGlyph) {
    let mut t = Tokens::default();
    t.push("simple");
    t.push(format!(
        "bbox={},{},{},{}",
        s.x_min(),
        s.y_min(),
        s.x_max(),
        s.y_max()
    ));
    let ends: Vec<String> = s
        .end_pts_of_contours()
        .iter()
        .map(|e| e.get().to_string())
        .collect();
    t.push(format!("ends={}", ends.join(",")));
    t.push(format!(
        "overlap={}",
        u8::from(s.has_overlapping_contours())
    ));
    t.push(format!("instructions={}", hex(s.instructions())));
    d.glyph(g, "outline", t.done());
    let mut t = Tokens::default();
    for (i, p) in s.points().enumerate() {
        let kind = match (p.on_curve, p.cubic) {
            (true, false) => "on",
            (true, true) => "on-cubic",
            (false, false) => "off",
            (false, true) => "off-cubic",
        };
        t.push(format!("p{i}={},{},{kind}", p.x, p.y));
    }
    d.glyph(g, "points", t.done());
}

/// The component flags that change what is drawn or measured. The rest
/// (argument size, which scale form, more components, instructions
/// present) say how the record is packed, and the values they pack are
/// compared themselves.
const COMPONENT_FLAGS: u16 = 0x0004 | 0x0200 | 0x0400 | 0x0800 | 0x1000;

fn composite(d: &mut Dump, g: u32, c: &CompositeGlyph) {
    let mut t = Tokens::default();
    t.push("composite");
    t.push(format!(
        "bbox={},{},{},{}",
        c.x_min(),
        c.y_min(),
        c.x_max(),
        c.y_max()
    ));
    t.push(format!(
        "instructions={}",
        hex(c.instructions().unwrap_or_default())
    ));
    d.glyph(g, "outline", t.done());
    let mut t = Tokens::default();
    for (i, comp) in c.components().enumerate() {
        let anchor = match comp.anchor {
            Anchor::Offset { x, y } => format!("offset({x},{y})"),
            Anchor::Point { base, component } => format!("points({base},{component})"),
        };
        let m = comp.transform;
        t.push(format!(
            "c{i}=glyph:{},{anchor},matrix({},{},{},{}),flags:{:#06x}",
            comp.glyph.to_u32(),
            f2(m.xx),
            f2(m.yx),
            f2(m.xy),
            f2(m.yy),
            comp.flags.bits() & COMPONENT_FLAGS
        ));
    }
    d.glyph(g, "components", t.done());
}

/// Every glyph's outline: contours, points with their flags, the
/// instructions, components with offsets, matrices and the flags that
/// matter, and the bounding box.
pub fn glyf(font: &FontRef) -> Result<Dump, ReadError> {
    font.glyf()?;
    font.loca(None)?;
    let mut d = Dump::default();
    for g in 0..glyph_count(font) {
        match glyph_at(font, g) {
            Ok(None) => d.glyph(g, "outline", "empty"),
            Ok(Some(Glyph::Simple(s))) => simple(&mut d, g, &s),
            Ok(Some(Glyph::Composite(c))) => composite(&mut d, g, &c),
            Err(e) => d.glyph(g, "outline", format!("unreadable:{e}")),
        }
    }
    Ok(d)
}

/// The deltas of one contour's points for one tuple, unreferenced points
/// inferred from the referenced ones by the OpenType gvar chapter's
/// "Inferred deltas for un-referenced point numbers"
/// (`[[source: ms-opentype-gvar]]`): the nearest referenced points before
/// and after in point order, wrapping within the contour; per direction,
/// equal coordinates give their common delta or zero, a target outside
/// them takes the delta of the nearer one, and one between them is
/// interpolated linearly. A contour with no referenced point is
/// unaffected (zero), and one with a single referenced point moves with
/// it. The interpolation is written as HarfBuzz 12.3.0's `infer_delta`
/// writes it (`[[source: harfbuzz-12-3-0-gvar]]`), `prev + r * (next -
/// prev)`; both fonts go through this one function, so the choice cannot
/// part them.
pub fn iup_contour(coords: &[(f64, f64)], explicit: &[Option<(f64, f64)>]) -> Vec<(f64, f64)> {
    let n = coords.len();
    let referenced: Vec<usize> = (0..n).filter(|&i| explicit[i].is_some()).collect();
    if referenced.is_empty() {
        return vec![(0.0, 0.0); n];
    }
    let infer = |t: f64, pc: f64, nc: f64, pd: f64, nd: f64| -> f64 {
        if pc == nc {
            if pd == nd { pd } else { 0.0 }
        } else if t <= pc.min(nc) {
            if pc < nc { pd } else { nd }
        } else if t >= pc.max(nc) {
            if pc > nc { pd } else { nd }
        } else {
            let r = (t - pc) / (nc - pc);
            pd + r * (nd - pd)
        }
    };
    (0..n)
        .map(|i| {
            if let Some(d) = explicit[i] {
                return d;
            }
            let prev = *referenced
                .iter()
                .rev()
                .find(|&&r| r < i)
                .unwrap_or(referenced.last().unwrap());
            let next = *referenced
                .iter()
                .find(|&&r| r > i)
                .unwrap_or(&referenced[0]);
            let (pd, nd) = (explicit[prev].unwrap(), explicit[next].unwrap());
            let (pc, nc, tc) = (coords[prev], coords[next], coords[i]);
            (
                infer(tc.0, pc.0, nc.0, pd.0, nd.0),
                infer(tc.1, pc.1, nc.1, pd.1, nd.1),
            )
        })
        .collect()
}

/// What a glyph's deltas apply to: its points (or components) and the
/// four phantom points, and the contours inference runs over.
struct Target {
    coords: Vec<(f64, f64)>,
    contours: Vec<std::ops::Range<usize>>,
}

fn target(font: &FontRef, g: u32) -> Result<Target, String> {
    match glyph_at(font, g) {
        Ok(None) => Ok(Target {
            coords: vec![(0.0, 0.0); 4],
            contours: vec![],
        }),
        Ok(Some(Glyph::Simple(s))) => {
            let mut coords: Vec<(f64, f64)> =
                s.points().map(|p| (p.x as f64, p.y as f64)).collect();
            let mut contours = vec![];
            let mut start = 0;
            for end in s.end_pts_of_contours() {
                let end = end.get() as usize + 1;
                if end > coords.len() || end <= start {
                    return Err(format!("contour end {end} past {} points", coords.len()));
                }
                contours.push(start..end);
                start = end;
            }
            // Phantom points are never inferred: an unreferenced one is 0.
            coords.extend([(0.0, 0.0); 4]);
            Ok(Target { coords, contours })
        }
        Ok(Some(Glyph::Composite(c))) => {
            // A composite infers nothing: an unreferenced component is not
            // moved ("applies only to simple glyphs").
            Ok(Target {
                coords: vec![(0.0, 0.0); c.components().count() + 4],
                contours: vec![],
            })
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Every glyph's tuples, each as its region and the delta of every
/// point. A region states start, peak and end on every axis, the implicit
/// ones filled in as the common formats chapter defines them, so a
/// shared, private, embedded or intermediate spelling of one region is
/// one region. A glyph's tuples are summed, so their order is not
/// compared: they are sorted.
pub fn gvar(font: &FontRef) -> Result<Dump, ReadError> {
    let gvar = font.gvar()?;
    let mut d = Dump::default();
    d.put("axis count", gvar.axis_count().to_string());
    d.put("glyph count", gvar.glyph_count().to_string());
    let axes = gvar.axis_count() as usize;
    for g in 0..glyph_count(font) {
        let data = match gvar.glyph_variation_data(GlyphId::new(g)) {
            Ok(None) => {
                d.glyph(g, "tuples", "0");
                continue;
            }
            Ok(Some(data)) => data,
            Err(e) => {
                d.glyph(g, "tuples", format!("unreadable:{e}"));
                continue;
            }
        };
        let target = match target(font, g) {
            Ok(t) => t,
            Err(e) => {
                d.glyph(g, "tuples", format!("glyph unreadable:{e}"));
                continue;
            }
        };
        let n = target.coords.len();
        let mut tuples: Vec<(String, String)> = vec![];
        for tuple in data.tuples() {
            let peak = tuple.peak();
            let (start, end) = (tuple.intermediate_start(), tuple.intermediate_end());
            let region: Vec<String> = (0..axes)
                .map(|i| {
                    let p = peak.get(i).unwrap_or_default();
                    let zero = read_fonts::types::F2Dot14::ZERO;
                    let s = start.as_ref().and_then(|t| t.get(i)).unwrap_or(p.min(zero));
                    let e = end.as_ref().and_then(|t| t.get(i)).unwrap_or(p.max(zero));
                    format!("({},{},{})", f2(s), f2(p), f2(e))
                })
                .collect();
            let mut explicit: Vec<Option<(f64, f64)>> = vec![None; n];
            let mut stray = vec![];
            for delta in tuple.deltas() {
                match explicit.get_mut(delta.position as usize) {
                    Some(slot) => *slot = Some((delta.x_delta as f64, delta.y_delta as f64)),
                    None => stray.push(delta.position),
                }
            }
            let mut full: Vec<(f64, f64)> =
                explicit.iter().map(|e| e.unwrap_or((0.0, 0.0))).collect();
            if !tuple.has_deltas_for_all_points() {
                for c in &target.contours {
                    let inferred = iup_contour(&target.coords[c.clone()], &explicit[c.clone()]);
                    full[c.clone()].copy_from_slice(&inferred);
                }
            }
            let mut t = Tokens::default();
            for (i, (x, y)) in full.iter().enumerate() {
                t.push(format!("p{i}={},{}", num(*x), num(*y)));
            }
            for p in stray {
                t.push(format!("stray-point={p}"));
            }
            tuples.push((region.join(""), t.done()));
        }
        tuples.sort();
        d.glyph(g, "tuples", tuples.len().to_string());
        for (k, (region, deltas)) in tuples.into_iter().enumerate() {
            d.glyph(g, format!("tuple {k} region"), region);
            d.glyph(g, format!("tuple {k} deltas"), deltas);
        }
    }
    Ok(d)
}

/// Each glyph's advance and left side bearing, however hhea's
/// numberOfHMetrics packs them (hhea compares that field itself).
pub fn hmtx(font: &FontRef) -> Result<Dump, ReadError> {
    let hmtx = font.hmtx()?;
    let mut d = Dump::default();
    for g in 0..glyph_count(font) {
        let gid = GlyphId::new(g);
        let adv = hmtx
            .advance(gid)
            .map_or("none".to_string(), |v| v.to_string());
        let lsb = hmtx
            .side_bearing(gid)
            .map_or("none".to_string(), |v| v.to_string());
        d.glyph(g, "metrics", format!("advance={adv} lsb={lsb}"));
    }
    Ok(d)
}

/// Each glyph's advance, left and right side bearing deltas resolved at
/// every sample, through the mappings if there are any.
pub fn hvar(font: &FontRef, ctx: &Samples) -> Result<Dump, ReadError> {
    let hvar = font.hvar()?;
    let store = hvar.item_variation_store()?;
    let mut d = Dump::default();
    d.put(
        "version",
        format!("{}.{}", hvar.version().major, hvar.version().minor),
    );
    let adv = hvar.advance_width_mapping().transpose()?;
    let lsb = hvar.lsb_mapping().transpose()?;
    let rsb = hvar.rsb_mapping().transpose()?;
    d.put(
        "mappings",
        format!("lsb={} rsb={}", lsb.is_some(), rsb.is_some()),
    );
    for g in 0..glyph_count(font) {
        let direct = DeltaSetIndex {
            outer: 0,
            inner: g as u16,
        };
        let mut t = Tokens::default();
        let index = match &adv {
            Some(m) => m.get(g),
            None => Ok(direct),
        };
        t.push(match index {
            Ok(i) => format!("advance={}", ctx.resolve(Some(&store), i)),
            Err(e) => format!("advance=err:{e}"),
        });
        for (label, map) in [("lsb", &lsb), ("rsb", &rsb)] {
            if let Some(m) = map {
                t.push(match m.get(g) {
                    Ok(i) => format!("{label}={}", ctx.resolve(Some(&store), i)),
                    Err(e) => format!("{label}=err:{e}"),
                });
            }
        }
        d.glyph(g, "deltas", t.done());
    }
    Ok(d)
}
