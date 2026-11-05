//! The fixed tables, one line per field: head (without
//! `checkSumAdjustment`), hhea, maxp, OS/2, post, name, gasp, cvt, fpgm,
//! prep, fvar, avar and STAT.

use read_fonts::tables::stat::AxisValue;
use read_fonts::types::MajorMinor;
use read_fonts::{FontRef, ReadError, TableProvider};

use crate::dump::{Dump, Tokens};
use crate::var::{f2, fx};

fn mm(v: MajorMinor) -> String {
    format!("{}.{}", v.major, v.minor)
}

fn fields(d: &mut Dump, list: &[(&str, String)]) {
    for (field, value) in list {
        d.put(*field, value.clone());
    }
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map_or("absent".to_string(), |v| v.to_string())
}

pub fn head(font: &FontRef) -> Result<Dump, ReadError> {
    let h = font.head()?;
    let mut d = Dump::default();
    fields(
        &mut d,
        &[
            ("version", mm(h.version())),
            ("fontRevision", fx(h.font_revision())),
            ("magicNumber", format!("{:#010x}", h.magic_number())),
            ("flags", format!("{:#06x}", h.flags().bits())),
            ("unitsPerEm", h.units_per_em().to_string()),
            ("created", h.created().as_secs().to_string()),
            ("modified", h.modified().as_secs().to_string()),
            ("xMin", h.x_min().to_string()),
            ("yMin", h.y_min().to_string()),
            ("xMax", h.x_max().to_string()),
            ("yMax", h.y_max().to_string()),
            ("macStyle", format!("{:#06x}", h.mac_style().bits())),
            ("lowestRecPPEM", h.lowest_rec_ppem().to_string()),
            ("fontDirectionHint", h.font_direction_hint().to_string()),
            ("indexToLocFormat", h.index_to_loc_format().to_string()),
            ("glyphDataFormat", h.glyph_data_format().to_string()),
        ],
    );
    Ok(d)
}

pub fn hhea(font: &FontRef) -> Result<Dump, ReadError> {
    let h = font.hhea()?;
    let mut d = Dump::default();
    fields(
        &mut d,
        &[
            ("version", mm(h.version())),
            ("ascender", h.ascender().to_i16().to_string()),
            ("descender", h.descender().to_i16().to_string()),
            ("lineGap", h.line_gap().to_i16().to_string()),
            (
                "advanceWidthMax",
                h.advance_width_max().to_u16().to_string(),
            ),
            (
                "minLeftSideBearing",
                h.min_left_side_bearing().to_i16().to_string(),
            ),
            (
                "minRightSideBearing",
                h.min_right_side_bearing().to_i16().to_string(),
            ),
            ("xMaxExtent", h.x_max_extent().to_i16().to_string()),
            ("caretSlopeRise", h.caret_slope_rise().to_string()),
            ("caretSlopeRun", h.caret_slope_run().to_string()),
            ("caretOffset", h.caret_offset().to_string()),
            ("metricDataFormat", h.metric_data_format().to_string()),
            ("numberOfHMetrics", h.number_of_h_metrics().to_string()),
        ],
    );
    Ok(d)
}

pub fn maxp(font: &FontRef) -> Result<Dump, ReadError> {
    let m = font.maxp()?;
    let mut d = Dump::default();
    fields(
        &mut d,
        &[
            ("version", format!("{:?}", m.version())),
            ("numGlyphs", m.num_glyphs().to_string()),
            ("maxPoints", opt(m.max_points())),
            ("maxContours", opt(m.max_contours())),
            ("maxCompositePoints", opt(m.max_composite_points())),
            ("maxCompositeContours", opt(m.max_composite_contours())),
            ("maxZones", opt(m.max_zones())),
            ("maxTwilightPoints", opt(m.max_twilight_points())),
            ("maxStorage", opt(m.max_storage())),
            ("maxFunctionDefs", opt(m.max_function_defs())),
            ("maxInstructionDefs", opt(m.max_instruction_defs())),
            ("maxStackElements", opt(m.max_stack_elements())),
            ("maxSizeOfInstructions", opt(m.max_size_of_instructions())),
            ("maxComponentElements", opt(m.max_component_elements())),
            ("maxComponentDepth", opt(m.max_component_depth())),
        ],
    );
    Ok(d)
}

pub fn os2(font: &FontRef) -> Result<Dump, ReadError> {
    let o = font.os2()?;
    let mut d = Dump::default();
    let panose: Vec<String> = o.panose_10().iter().map(|b| b.to_string()).collect();
    fields(
        &mut d,
        &[
            ("version", o.version().to_string()),
            ("xAvgCharWidth", o.x_avg_char_width().to_string()),
            ("usWeightClass", o.us_weight_class().to_string()),
            ("usWidthClass", o.us_width_class().to_string()),
            ("fsType", format!("{:#06x}", o.fs_type())),
            ("ySubscriptXSize", o.y_subscript_x_size().to_string()),
            ("ySubscriptYSize", o.y_subscript_y_size().to_string()),
            ("ySubscriptXOffset", o.y_subscript_x_offset().to_string()),
            ("ySubscriptYOffset", o.y_subscript_y_offset().to_string()),
            ("ySuperscriptXSize", o.y_superscript_x_size().to_string()),
            ("ySuperscriptYSize", o.y_superscript_y_size().to_string()),
            (
                "ySuperscriptXOffset",
                o.y_superscript_x_offset().to_string(),
            ),
            (
                "ySuperscriptYOffset",
                o.y_superscript_y_offset().to_string(),
            ),
            ("yStrikeoutSize", o.y_strikeout_size().to_string()),
            ("yStrikeoutPosition", o.y_strikeout_position().to_string()),
            ("sFamilyClass", o.s_family_class().to_string()),
            ("panose", panose.join(",")),
            (
                "ulUnicodeRange1",
                format!("{:#010x}", o.ul_unicode_range_1()),
            ),
            (
                "ulUnicodeRange2",
                format!("{:#010x}", o.ul_unicode_range_2()),
            ),
            (
                "ulUnicodeRange3",
                format!("{:#010x}", o.ul_unicode_range_3()),
            ),
            (
                "ulUnicodeRange4",
                format!("{:#010x}", o.ul_unicode_range_4()),
            ),
            ("achVendID", o.ach_vend_id().to_string()),
            ("fsSelection", format!("{:#06x}", o.fs_selection().bits())),
            ("usFirstCharIndex", o.us_first_char_index().to_string()),
            ("usLastCharIndex", o.us_last_char_index().to_string()),
            ("sTypoAscender", o.s_typo_ascender().to_string()),
            ("sTypoDescender", o.s_typo_descender().to_string()),
            ("sTypoLineGap", o.s_typo_line_gap().to_string()),
            ("usWinAscent", o.us_win_ascent().to_string()),
            ("usWinDescent", o.us_win_descent().to_string()),
            (
                "ulCodePageRange1",
                opt(o.ul_code_page_range_1().map(|v| format!("{v:#010x}"))),
            ),
            (
                "ulCodePageRange2",
                opt(o.ul_code_page_range_2().map(|v| format!("{v:#010x}"))),
            ),
            ("sxHeight", opt(o.sx_height())),
            ("sCapHeight", opt(o.s_cap_height())),
            ("usDefaultChar", opt(o.us_default_char())),
            ("usBreakChar", opt(o.us_break_char())),
            ("usMaxContext", opt(o.us_max_context())),
            (
                "usLowerOpticalPointSize",
                opt(o.us_lower_optical_point_size()),
            ),
            (
                "usUpperOpticalPointSize",
                opt(o.us_upper_optical_point_size()),
            ),
        ],
    );
    Ok(d)
}

/// post's header fields. The glyph names are compared in the glyph order.
pub fn post(font: &FontRef) -> Result<Dump, ReadError> {
    let p = font.post()?;
    let mut d = Dump::default();
    fields(
        &mut d,
        &[
            ("version", format!("{:?}", p.version())),
            ("italicAngle", fx(p.italic_angle())),
            (
                "underlinePosition",
                p.underline_position().to_i16().to_string(),
            ),
            (
                "underlineThickness",
                p.underline_thickness().to_i16().to_string(),
            ),
            ("isFixedPitch", p.is_fixed_pitch().to_string()),
            ("minMemType42", p.min_mem_type42().to_string()),
            ("maxMemType42", p.max_mem_type42().to_string()),
            ("minMemType1", p.min_mem_type1().to_string()),
            ("maxMemType1", p.max_mem_type1().to_string()),
            ("numGlyphs", opt(p.num_glyphs())),
        ],
    );
    Ok(d)
}

/// Every name record by its (platform, encoding, language, name ID), in
/// that order whatever order the table stores them in, with its string
/// decoded; then the language-tag records.
pub fn name(font: &FontRef) -> Result<Dump, ReadError> {
    let n = font.name()?;
    let mut d = Dump::default();
    d.put("version", n.version().to_string());
    let data = n.string_data();
    let mut records: Vec<_> = n.name_record().iter().collect();
    records.sort_by_key(|r| {
        (
            r.platform_id(),
            r.encoding_id(),
            r.language_id(),
            r.name_id().to_u16(),
        )
    });
    for r in records {
        let locus = format!(
            "platform={} encoding={} language={:#06x} nameID={}",
            r.platform_id(),
            r.encoding_id(),
            r.language_id(),
            r.name_id().to_u16()
        );
        let text = match r.string(data) {
            Ok(s) => s.chars().collect::<String>(),
            Err(e) => format!("unreadable:{e}"),
        };
        d.put(locus, text);
    }
    for (i, r) in n.lang_tag_record().unwrap_or_default().iter().enumerate() {
        let text = match r.lang_tag(data) {
            Ok(s) => s.chars().collect::<String>(),
            Err(e) => format!("unreadable:{e}"),
        };
        d.put(format!("langTag {i}"), text);
    }
    Ok(d)
}

pub fn gasp(font: &FontRef) -> Result<Dump, ReadError> {
    let g = font.gasp()?;
    let mut d = Dump::default();
    d.put("version", g.version().to_string());
    let mut t = Tokens::default();
    for (i, r) in g.gasp_ranges().iter().enumerate() {
        t.push(format!(
            "r{i}=ppem:{},behavior:{:#06x}",
            r.range_max_ppem(),
            r.range_gasp_behavior().bits()
        ));
    }
    d.put("ranges", t.done());
    Ok(d)
}

pub fn cvt(font: &FontRef) -> Result<Dump, ReadError> {
    let c = font.cvt()?;
    let mut d = Dump::default();
    let mut t = Tokens::default();
    for (i, v) in c.iter().enumerate() {
        t.push(format!("cvt{i}={}", v.get()));
    }
    d.put("values", t.done());
    Ok(d)
}

/// A program, one token per byte, so the first difference is named by
/// its offset.
pub fn bytes(label: &str, b: &[u8]) -> Dump {
    let mut d = Dump::default();
    d.put("length", b.len().to_string());
    let mut t = Tokens::default();
    for (i, v) in b.iter().enumerate() {
        t.push(format!("b{i}={v:02x}"));
    }
    d.put(label, t.done());
    d
}

pub fn fvar(font: &FontRef) -> Result<Dump, ReadError> {
    let f = font.fvar()?;
    let mut d = Dump::default();
    d.put("version", mm(f.version()));
    for (i, a) in f.axes()?.iter().enumerate() {
        d.put(
            format!("axis {i}"),
            format!(
                "tag={} min={} default={} max={} flags={:#06x} nameID={}",
                a.axis_tag(),
                fx(a.min_value()),
                fx(a.default_value()),
                fx(a.max_value()),
                a.flags(),
                a.axis_name_id().to_u16()
            ),
        );
    }
    for (i, inst) in f.instances()?.iter().enumerate() {
        let inst = match inst {
            Ok(inst) => inst,
            Err(e) => {
                d.put(format!("instance {i}"), format!("unreadable:{e}"));
                continue;
            }
        };
        let coords: Vec<String> = inst.coordinates.iter().map(|c| fx(c.get())).collect();
        d.put(
            format!("instance {i}"),
            format!(
                "subfamilyNameID={} flags={:#06x} coordinates={} postScriptNameID={}",
                inst.subfamily_name_id.to_u16(),
                inst.flags,
                coords.join(","),
                opt(inst.post_script_name_id.map(|n| n.to_u16()))
            ),
        );
    }
    Ok(d)
}

pub fn avar(font: &FontRef) -> Result<Dump, ReadError> {
    let a = font.avar()?;
    let mut d = Dump::default();
    d.put("version", mm(a.version()));
    d.put("axisCount", a.axis_count().to_string());
    for (i, map) in a.axis_segment_maps().iter().enumerate() {
        let mut t = Tokens::default();
        match map {
            Ok(map) => {
                for v in map.axis_value_maps() {
                    t.push(format!(
                        "{}>{}",
                        f2(v.from_coordinate()),
                        f2(v.to_coordinate())
                    ));
                }
            }
            Err(e) => t.push(format!("unreadable:{e}")),
        }
        d.put(format!("axis {i} segments"), t.done());
    }
    if a.version().major >= 2 {
        // Version 2's mapping and store are compared as bytes.
        let bytes: Vec<String> = a
            .offset_data()
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        d.put("version 2 data", bytes.join(" "));
    }
    Ok(d)
}

pub fn stat(font: &FontRef) -> Result<Dump, ReadError> {
    let s = font.stat()?;
    let mut d = Dump::default();
    d.put("version", mm(s.version()));
    for (i, a) in s.design_axes()?.iter().enumerate() {
        d.put(
            format!("design axis {i}"),
            format!(
                "tag={} nameID={} ordering={}",
                a.axis_tag(),
                a.axis_name_id().to_u16(),
                a.axis_ordering()
            ),
        );
    }
    if let Some(values) = s.offset_to_axis_values() {
        for (i, v) in values?.axis_values().iter().enumerate() {
            let line = match v {
                Err(e) => format!("unreadable:{e}"),
                Ok(AxisValue::Format1(v)) => format!(
                    "format=1 axis={} flags={:#06x} nameID={} value={}",
                    v.axis_index(),
                    v.flags().bits(),
                    v.value_name_id().to_u16(),
                    fx(v.value())
                ),
                Ok(AxisValue::Format2(v)) => format!(
                    "format=2 axis={} flags={:#06x} nameID={} nominal={} min={} max={}",
                    v.axis_index(),
                    v.flags().bits(),
                    v.value_name_id().to_u16(),
                    fx(v.nominal_value()),
                    fx(v.range_min_value()),
                    fx(v.range_max_value())
                ),
                Ok(AxisValue::Format3(v)) => format!(
                    "format=3 axis={} flags={:#06x} nameID={} value={} linked={}",
                    v.axis_index(),
                    v.flags().bits(),
                    v.value_name_id().to_u16(),
                    fx(v.value()),
                    fx(v.linked_value())
                ),
                Ok(AxisValue::Format4(v)) => {
                    let rec: Vec<String> = v
                        .axis_values()
                        .iter()
                        .map(|r| format!("{}:{}", r.axis_index(), fx(r.value())))
                        .collect();
                    format!(
                        "format=4 flags={:#06x} nameID={} values={}",
                        v.flags().bits(),
                        v.value_name_id().to_u16(),
                        rec.join(",")
                    )
                }
            };
            d.put(format!("axis value {i}"), line);
        }
    }
    d.put(
        "elidedFallbackNameID",
        opt(s.elided_fallback_name_id().map(|n| n.to_u16())),
    );
    Ok(d)
}
