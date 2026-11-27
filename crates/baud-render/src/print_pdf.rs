//! **baud's printer onto a PDF**: the pages [`crate::print`] laid out, as a
//! PDF 1.4 file (`[[source: pdf-reference-1-4]]`) for a print dialog that
//! takes a document rather than drawing views, as the Linux print portal
//! does.
//!
//! **Text is the faces' own outlines**, each glyph's contours a filled path,
//! so the paper shows what baud's faces draw and no font is embedded. Each
//! distinct glyph is drawn once, as a Form XObject (§ 4.9), and placed at
//! its column with `cm` and `Do`, so a page of one letter is one outline and
//! a placement a cell. A character no face in the chain has is the first
//! face's `.notdef`.
//!
//! **The looks are the panel's**: bold from the bold chain, underline a rule
//! under the baseline, reverse white on a black box the columns wide, blink
//! plain; a double-width line twice as wide, a double-height one twice the
//! size over two lines. A struck-over column draws every strike there, in
//! order. **A picture is an image XObject** at its column, `Picture::size`
//! on paper, its pixels RGB with the clear ones white, the paper's colour.
//! Streams are unfiltered.

use crate::print::{Half, Layout, Look, Page, Picture, Scale};
use baud_domain::{Codepoint, GlyphId};
use baud_font::{Drawn, Face, Sink, VarPoint};
use std::collections::HashMap;
use std::fmt::Write as _;

/// The paper and where the printable area starts on it, in points: the
/// sheet's size, and the left and top margins.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sheet {
    pub width: f64,
    pub height: f64,
    pub left: f64,
    pub top: f64,
}

/// The face chains a job is drawn from, each in fallback order: plain text
/// from `regular`, bold from `bold`.
#[derive(Clone, Copy)]
pub struct Faces<'a, 'f> {
    pub regular: &'a [Face<'f>],
    pub bold: &'a [Face<'f>],
}

/// **A face's advance and line height per point**, what `Layout::fit` sizes
/// the type by: the advance of `0` and the line's ascender to descender
/// plus its gap, each over the em.
pub fn measures(face: &Face<'_>) -> (f64, f64) {
    let em = f64::from(face.units_per_em().value().max(1));
    let advance = face
        .glyph(Codepoint::from_char('0'))
        .and_then(|g| face.advance(g))
        .map_or(em * 0.6, |a| f64::from(a.value()));
    let lm = face.line_metrics();
    let height = f64::from(lm.ascender.value()) - f64::from(lm.descender.value())
        + f64::from(lm.line_gap.value());
    (advance / em, height.max(em) / em)
}

/// A number as a content stream writes it: at most three decimals, no
/// trailing zeros, no `-0`.
fn num(v: f64) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    match s {
        "-0" | "" => "0".into(),
        s => s.into(),
    }
}

/// An object's number in the file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Obj(usize);

/// A glyph's outline as path operators in font units, and the box its
/// points lie in. Quadratic segments become cubic ones (§ 4.4.1 has only
/// cubics): the two control points two thirds of the way to the quadratic
/// one from each end.
struct Path {
    ops: String,
    at: (f32, f32),
    bbox: Option<(f32, f32, f32, f32)>,
}

impl Path {
    fn new() -> Path {
        Path {
            ops: String::new(),
            at: (0.0, 0.0),
            bbox: None,
        }
    }

    fn see(&mut self, x: f32, y: f32) {
        self.bbox = Some(match self.bbox {
            None => (x, y, x, y),
            Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
        });
    }

    fn point(&mut self, x: f32, y: f32) {
        self.see(x, y);
        let _ = write!(self.ops, "{} {} ", num(f64::from(x)), num(f64::from(y)));
    }
}

impl Sink for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.point(x, y);
        self.ops.push_str("m\n");
        self.at = (x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.point(x, y);
        self.ops.push_str("l\n");
        self.at = (x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x0, y0) = self.at;
        let two = 2.0 / 3.0;
        self.curve_to(
            x0 + two * (x1 - x0),
            y0 + two * (y1 - y0),
            x + two * (x1 - x),
            y + two * (y1 - y),
            x,
            y,
        );
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.point(x1, y1);
        self.point(x2, y2);
        self.point(x, y);
        self.ops.push_str("c\n");
        self.at = (x, y);
    }
    fn close(&mut self) {
        self.ops.push_str("h\n");
    }
}

/// Which chain, which face in it, which glyph: one form each.
type GlyphKey = (bool, usize, usize);

/// The file being built: its objects in number order, the glyphs drawn so
/// far, and the resources every page shares.
struct Doc {
    objects: Vec<Vec<u8>>,
    forms: HashMap<GlyphKey, Option<Obj>>,
    xobjects: String,
    var: Vec<VarPoint>,
}

impl Doc {
    /// Reserves the next object's number, filled in by `set`.
    fn reserve(&mut self) -> Obj {
        self.objects.push(Vec::new());
        Obj(self.objects.len())
    }

    fn set(&mut self, o: Obj, body: Vec<u8>) {
        self.objects[o.0 - 1] = body;
    }

    fn add(&mut self, body: Vec<u8>) -> Obj {
        let o = self.reserve();
        self.set(o, body);
        o
    }

    /// A stream object of `dict`'s entries and `data`.
    fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
        let mut b = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        b.extend_from_slice(data);
        b.extend_from_slice(b"\nendstream");
        b
    }

    /// **The form that draws `g` of `face`**, made the first time: `None`
    /// for a glyph with no outline, a space.
    fn form(&mut self, key: GlyphKey, face: &Face<'_>, g: GlyphId) -> Option<Obj> {
        if let Some(known) = self.forms.get(&key) {
            return *known;
        }
        let mut path = Path::new();
        let drawn = crate::atlas::outline(&mut self.var, face, g, &mut path);
        let made = match (drawn, path.bbox) {
            (Drawn::Outline, Some((x0, y0, x1, y1))) => {
                let dict = format!(
                    "/Type /XObject /Subtype /Form /BBox [{} {} {} {}]",
                    num(f64::from(x0)),
                    num(f64::from(y0)),
                    num(f64::from(x1)),
                    num(f64::from(y1))
                );
                path.ops.push_str("f\n");
                let o = self.add(Doc::stream(&dict, path.ops.as_bytes()));
                let _ = write!(self.xobjects, "/G{} {} 0 R ", o.0, o.0);
                Some(o)
            }
            _ => None,
        };
        self.forms.insert(key, made);
        made
    }

    /// **A picture's image**, its pixels RGB, a clear one the paper's
    /// white.
    fn image(&mut self, p: &Picture) -> Option<Obj> {
        let (w, h) = (p.width.index(), p.height.index());
        if w == 0 || h == 0 {
            return None;
        }
        let mut rgb = Vec::with_capacity(w * h * 3);
        for px in &p.pixels {
            let (r, g, b) = px.rgb();
            match px.alpha().get() {
                0 => rgb.extend_from_slice(&[255, 255, 255]),
                _ => rgb.extend_from_slice(&[r.get(), g.get(), b.get()]),
            }
        }
        let dict = format!(
            "/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8"
        );
        let o = self.add(Doc::stream(&dict, &rgb));
        let _ = write!(self.xobjects, "/I{} {} 0 R ", o.0, o.0);
        Some(o)
    }
}

/// The face in `chain` that has `c`, and its glyph: the first whose `cmap`
/// maps it, else the first face's `.notdef`.
fn find(chain: &[Face<'_>], c: char) -> Option<(usize, GlyphId)> {
    let cp = Codepoint::from_char(c);
    chain
        .iter()
        .enumerate()
        .find_map(|(i, f)| {
            f.glyph(cp)
                .filter(|g| *g != GlyphId::NOTDEF)
                .map(|g| (i, g))
        })
        .or_else(|| (!chain.is_empty()).then_some((0, GlyphId::NOTDEF)))
}

/// One page's content stream.
fn page(doc: &mut Doc, p: &Page, layout: Layout, sheet: Sheet, faces: Faces<'_, '_>) -> String {
    let mut out = String::from("0 g\n");
    let Some(first) = faces.regular.first() else {
        return out;
    };
    let em = f64::from(first.units_per_em().value().max(1));
    let lm = first.line_metrics();
    let ascent = f64::from(lm.ascender.value()) / em;
    let mut down = 0.0;
    for line in &p.lines {
        let top = sheet.height - sheet.top - down;
        let tall = line.height(layout).points(layout).0;
        down += tall;
        if let Some(pic) = &line.picture {
            if let Some(img) = doc.image(pic) {
                let (w, h) = pic.size(layout);
                let x = sheet.left + pic.column.points(layout).0;
                let _ = writeln!(
                    out,
                    "q {} 0 0 {} {} {} cm /I{} Do Q",
                    num(w.0),
                    num(h.0),
                    num(x),
                    num(top - h.0),
                    img.0
                );
            }
            continue;
        }
        let (wide, size) = match line.scale {
            Scale::Single => (1.0, layout.size),
            Scale::DoubleWidth => (2.0, layout.size),
            Scale::DoubleTop | Scale::DoubleBottom => (2.0, layout.size * 2.0),
        };
        let base = top - ascent * size;
        let mut x = sheet.left;
        for cell in &line.cells {
            let at = x;
            x += layout.column * wide;
            if cell.half == Half::Tail {
                continue;
            }
            for s in &cell.strikes {
                let span = s.width().points(layout).0 * wide;
                let reverse = s.look.has(Look::REVERSE);
                if reverse {
                    let _ = writeln!(
                        out,
                        "{} {} {} {} re f",
                        num(at),
                        num(top - tall),
                        num(span),
                        num(tall)
                    );
                }
                if s.look.has(Look::UNDERLINE) {
                    // A twentieth of the size, a tenth below the baseline.
                    let _ = writeln!(
                        out,
                        "{} {} {} {} re f",
                        num(at),
                        num(base - size * 0.1),
                        num(span),
                        num(size * 0.05)
                    );
                }
                if reverse {
                    out.push_str("1 g\n");
                }
                let bold = s.look.has(Look::BOLD);
                let chain = if bold { faces.bold } else { faces.regular };
                let mut pen = at;
                for c in s.text.chars() {
                    let Some((i, g)) = find(chain, c) else {
                        continue;
                    };
                    let face = &chain[i];
                    let scale = size / f64::from(face.units_per_em().value().max(1));
                    if let Some(form) = doc.form((bold, i, g.index()), face, g) {
                        let _ = writeln!(
                            out,
                            "q {} 0 0 {} {} {} cm /G{} Do Q",
                            num(scale * wide),
                            num(scale),
                            num(pen),
                            num(base),
                            form.0
                        );
                    }
                    // Marks after the first character are placed after
                    // its advance, where a font draws them back over it.
                    pen += face.advance(g).map_or(0.0, |a| f64::from(a.value())) * scale * wide;
                }
                if reverse {
                    out.push_str("0 g\n");
                }
            }
        }
    }
    out
}

/// **The pages as a PDF file**, one sheet each.
pub fn write(pages: &[Page], layout: Layout, sheet: Sheet, faces: Faces<'_, '_>) -> Vec<u8> {
    let mut doc = Doc {
        objects: Vec::new(),
        forms: HashMap::new(),
        xobjects: String::new(),
        var: Vec::new(),
    };
    let catalog = doc.reserve();
    let tree = doc.reserve();
    let resources = doc.reserve();
    let mut kids = Vec::new();
    for p in pages {
        let content = page(&mut doc, p, layout, sheet, faces);
        let stream = doc.add(Doc::stream("", content.as_bytes()));
        let body = format!(
            "<< /Type /Page /Parent {} 0 R /MediaBox [0 0 {} {}] /Resources {} 0 R /Contents {} 0 R >>",
            tree.0,
            num(sheet.width),
            num(sheet.height),
            resources.0,
            stream.0
        );
        kids.push(doc.add(body.into_bytes()));
    }
    let list: Vec<String> = kids.iter().map(|k| format!("{} 0 R", k.0)).collect();
    doc.set(
        tree,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            list.join(" "),
            kids.len()
        )
        .into_bytes(),
    );
    doc.set(
        catalog,
        format!("<< /Type /Catalog /Pages {} 0 R >>", tree.0).into_bytes(),
    );
    let xobjects = std::mem::take(&mut doc.xobjects);
    doc.set(
        resources,
        format!("<< /XObject << {xobjects}>> >>").into_bytes(),
    );
    // The file (§ 3.4): the header, a comment of four bytes past 127 so a
    // transfer takes the file as binary (§ 3.4.1), each object at the offset
    // its entry gives, the table of twenty-byte entries, the trailer.
    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::with_capacity(doc.objects.len());
    for (i, body) in doc.objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let table = out.len();
    let size = doc.objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {size} /Root {} 0 R >>\nstartxref\n{table}\n%%EOF\n",
            catalog.0
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print::{PictureBudget, paginate, read, widest};

    fn face_file(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/../baud-app/fonts/built/{name}.ttf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    /// A4, one-inch margins.
    const A4: Sheet = Sheet {
        width: 595.0,
        height: 842.0,
        left: 72.0,
        top: 72.0,
    };

    /// `job` printed on A4 from the built Regular and Bold.
    fn printed(job: &[u8]) -> Vec<u8> {
        let (regular, bold) = (face_file("Regular"), face_file("Bold"));
        let regular = [Face::parse(&regular).unwrap()];
        let bold = [Face::parse(&bold).unwrap()];
        let (advance, leading) = measures(&regular[0]);
        let (forms, _) = read(job, PictureBudget(1 << 20));
        let (w, h) = (A4.width - 2.0 * A4.left, A4.height - 2.0 * A4.top);
        let layout = Layout::fit(w, h, advance, leading, widest(&forms));
        let pages = paginate(&forms, layout);
        write(
            &pages,
            layout,
            A4,
            Faces {
                regular: &regular,
                bold: &bold,
            },
        )
    }

    fn text(pdf: &[u8]) -> String {
        String::from_utf8_lossy(pdf).into_owned()
    }

    /// **The file's frame holds**: the header, every xref entry at its
    /// object's `n 0 obj`, the trailer's `/Size` the entries, `startxref`
    /// at the table and `%%EOF` last.
    ///
    /// Read as bytes: an image's pixels are not UTF-8, and a lossy reading
    /// would move every offset after them.
    fn well_formed(pdf: &[u8]) {
        assert!(pdf.starts_with(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n"), "header");
        assert!(pdf.ends_with(b"%%EOF\n"), "end");
        let key = b"startxref\n";
        let after = pdf
            .windows(key.len())
            .rposition(|w| w == key)
            .expect("startxref")
            + key.len();
        let at: usize = std::str::from_utf8(&pdf[after..])
            .ok()
            .and_then(|t| t.lines().next())
            .and_then(|n| n.parse().ok())
            .expect("its offset");
        let s = text(pdf);
        let table = std::str::from_utf8(&pdf[at..]).expect("the table is ASCII");
        assert!(table.starts_with("xref\n0 "), "startxref names the table");
        let count: usize = table[7..]
            .lines()
            .next()
            .and_then(|n| n.parse().ok())
            .expect("the count");
        let entries = &table[table.find('\n').unwrap() + 1..];
        let entries = &entries[entries.find('\n').unwrap() + 1..];
        for i in 1..count {
            let e = &entries[i * 20..i * 20 + 20];
            let off: usize = e[..10].parse().expect("an offset");
            assert!(
                pdf[off..].starts_with(format!("{i} 0 obj\n").as_bytes()),
                "object {i} at {off}"
            );
        }
        assert!(s.contains(&format!("/Size {count}")), "the trailer's size");
    }

    #[test]
    fn a_line_of_text_is_one_sheet_of_the_paper_given() {
        let pdf = printed(b"Hello\r\n");
        well_formed(&pdf);
        assert_eq!(text(&pdf).matches("/Type /Page ").count(), 1, "sheets");
        assert!(text(&pdf).contains("/MediaBox [0 0 595 842]"));
    }

    /// **A form feed is a new sheet**.
    #[test]
    fn a_form_feed_starts_a_sheet() {
        let pdf = printed(b"one\r\n\x0ctwo\r\n");
        well_formed(&pdf);
        assert_eq!(text(&pdf).matches("/Type /Page ").count(), 2, "sheets");
    }

    /// **A glyph is drawn once and placed at each column**: four of one
    /// letter are one form and four placements.
    #[test]
    fn a_glyph_is_one_form_however_often_it_prints() {
        let s = text(&printed(b"AAAA\r\n"));
        assert_eq!(s.matches("/Subtype /Form").count(), 1, "{s}");
        assert_eq!(s.matches(" Do Q").count(), 4);
        let s = text(&printed(b"AB\r\n"));
        assert_eq!(s.matches("/Subtype /Form").count(), 2);
    }

    /// **A space prints nothing**: no outline, no form, no placement.
    #[test]
    fn a_space_puts_no_ink_down() {
        let s = text(&printed(b"A A\r\n"));
        assert_eq!(s.matches("/Subtype /Form").count(), 1);
        assert_eq!(s.matches(" Do Q").count(), 2);
    }

    /// **Bold is the bold face**: the same letter plain and bold is two
    /// forms.
    #[test]
    fn bold_is_drawn_from_the_bold_face() {
        let s = text(&printed(b"A\x1b[0;1mA\x1b[0m\r\n"));
        assert_eq!(s.matches("/Subtype /Form").count(), 2);
    }

    /// **Reverse is white on a black box**, and underline a rule: each a
    /// filled rectangle.
    #[test]
    fn reverse_and_underline_are_boxes() {
        let plain = text(&printed(b"A\r\n"));
        assert_eq!(plain.matches(" re f").count(), 0);
        let reverse = text(&printed(b"\x1b[0;7mA\x1b[0m\r\n"));
        assert_eq!(reverse.matches(" re f").count(), 1);
        assert!(reverse.contains("1 g"), "the glyph in white");
        let under = text(&printed(b"\x1b[0;4mA\x1b[0m\r\n"));
        assert_eq!(under.matches(" re f").count(), 1);
    }

    /// **A sixel dump is an image** of its pixels, RGB, at its size.
    #[test]
    fn a_picture_is_an_image_of_its_pixels() {
        // Two columns of six pixels, register 1 red; `P1` 0 is a 2:1
        // aspect, each row read twice, so twelve high.
        let pdf = printed(b"\x1bP0;0;0q#1;2;100;0;0#1~~\x1b\\");
        well_formed(&pdf);
        let s = text(&pdf);
        assert!(s.contains("/Subtype /Image"), "{s}");
        let dict = &s[s.find("/Subtype /Image").unwrap()..];
        let dict = &dict[..dict.find(">>").unwrap()];
        assert!(dict.contains("/Width 2 /Height 12"), "{dict}");
        assert!(s.contains("/ColorSpace /DeviceRGB /BitsPerComponent 8"));
        assert!(dict.contains("/Length 72"), "2 by 12 pixels of 3 bytes");
    }

    /// **A double-width line is drawn twice as wide**: its placement's
    /// horizontal scale is twice the vertical.
    #[test]
    fn a_double_width_line_is_twice_as_wide() {
        let s = text(&printed(b"\x1b#6A\r\n"));
        let place = s
            .lines()
            .find(|l| l.ends_with(" Do Q"))
            .expect("a placement");
        let n: Vec<f64> = place
            .split(' ')
            .skip(1)
            .take(4)
            .map(|v| v.parse().unwrap())
            .collect();
        assert!((n[0] - 2.0 * n[3]).abs() < 1e-3, "{place}");
    }

    /// **A mark is drawn after its letter's advance**, where a font draws a
    /// combining mark back over the character before it: `e` and U+0301 in
    /// one cell put the accent on the `e`.
    #[test]
    fn a_mark_follows_its_letter_s_advance() {
        let s = text(&printed(b"e\xcc\x81\r\n"));
        let x: Vec<f64> = s
            .lines()
            .filter(|l| l.ends_with(" Do Q"))
            .map(|l| l.split(' ').nth(5).unwrap().parse().unwrap())
            .collect();
        assert_eq!(x.len(), 2, "{s}");
        assert!(x[1] > x[0], "the mark at {} is not past {}", x[1], x[0]);
    }
}
