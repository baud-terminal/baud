//! A colour font whose colour glyph has a refused glyf outline.
//!
//! Writes `baud-font/tests/var/colour-refused-cases.ttf`. `play`, mapped
//! from U+25B6 BLACK RIGHT-POINTING TRIANGLE, is a `COLR` v0 glyph of two
//! layers, and its own `glyf` entry is a composite whose first component is
//! placed by matched points, which "cannot be used for the first component
//! glyph" (`[[source: ms-opentype-glyf]]`): baud refuses that outline
//! (`Drawn::Refused`). U+25B6 has an emoji variation sequence
//! (`[[source: emoji-variation-sequences]]`: "25B6 FE0F ; emoji style")
//! and is not Emoji_Presentation, so it asks for colour only with U+FE0F.
//! The atlas draws a colour glyph from its layers before its outline, so a
//! refused outline does not stop the glyph drawing in colour;
//! baud-render's `tests/refused_glyph.rs` holds the stack to that.
//!
//! Glyphs, each advance 600 and lsb its xMin:
//!
//! ```text
//! .notdef  a 500 x 700 box at x 50
//! A        a 400 x 600 box at x 100, the control, mapped from A and from
//!          M, which a renderer measures its cell by
//! tri      (0,0) (400,0) (200,300), the first layer, palette entry 0 (red)
//! bar      a 100 x 40 box, the second layer, palette entry 1 (blue)
//! play     bar p0/c0 first, then tri: refused; COLR layers tri, bar
//! ```
//!
//! `play`'s header box is tri's, written as a value: a first component
//! placed by points has no position to bound it from. Deterministic: the
//! head timestamps are fixed. baud's own font, under the repository's
//! licence (`baud-font/tests/licences.rs` lists it).

use write_fonts::tables::glyf::{
    Anchor, Component, ComponentFlags, CompositeGlyph, Glyph, Transform,
};

use super::{
    ZERO_EXTREMES, ZERO_TOTALS, bbox, bytes, cmap, colr_v0, cpal_v0, font, gid, glyf_loca,
    glyph_box, head, hhea, hmtx, maxp, name, os2, os2_bytes, poly, post_v2, rect,
};

const ORDER: [&str; 5] = [".notdef", "A", "tri", "bar", "play"];

pub fn fonts() -> Vec<(&'static str, Vec<u8>)> {
    let component = |n: &str, anchor: Anchor| {
        Component::new(
            gid(&ORDER, n),
            anchor,
            Transform::default(),
            ComponentFlags::default(),
        )
    };
    let tri_box = bbox(0, 0, 400, 300);
    let mut play = CompositeGlyph::new(
        component(
            "bar",
            Anchor::Point {
                base: 0,
                component: 0,
            },
        ),
        tri_box,
    );
    play.add_component(component("tri", Anchor::Offset { x: 0, y: 0 }), tri_box);
    let glyphs = vec![
        Glyph::Simple(poly(&rect(50, 0, 550, 700))),
        Glyph::Simple(poly(&rect(100, 0, 500, 600))),
        Glyph::Simple(poly(&[(0, 0), (400, 0), (200, 300)])),
        Glyph::Simple(poly(&rect(0, 0, 100, 40))),
        Glyph::Composite(play),
    ];
    let metrics: Vec<(u16, i16)> = glyphs
        .iter()
        .map(|g| (600, glyph_box(g).map_or(0, |b| b.x_min)))
        .collect();
    let (glyf, loca, format) = glyf_loca(&glyphs);
    let (hmtx, long) = hmtx(&metrics);
    let map = [
        (0x41, gid(&ORDER, "A")),
        (0x4D, gid(&ORDER, "A")),
        (0x25B6, gid(&ORDER, "play")),
    ];
    let font = font(vec![
        (
            b"head",
            bytes(&head(0x0003, 0, bbox(0, 0, 550, 700), Some(format))),
        ),
        (b"hhea", bytes(&hhea(900, -200, &ZERO_EXTREMES, long))),
        (b"maxp", bytes(&maxp(ORDER.len() as u16, &ZERO_TOTALS))),
        (b"OS/2", os2_bytes(&os2(600, 0x41, 0x25B6, 0x2000))),
        (b"hmtx", bytes(&hmtx)),
        (b"cmap", bytes(&cmap(&map))),
        (b"loca", loca),
        (b"glyf", glyf),
        (
            b"name",
            bytes(&name("baud colour refused cases", "Regular", &[])),
        ),
        (b"post", bytes(&post_v2(&ORDER))),
        (
            b"COLR",
            colr_v0(&ORDER, &[("play", &[("tri", 0), ("bar", 1)])]),
        ),
        (b"CPAL", cpal_v0(&[(0xFF, 0, 0, 0xFF), (0, 0, 0xFF, 0xFF)])),
    ]);
    vec![("baud-font/tests/var/colour-refused-cases.ttf", font)]
}
