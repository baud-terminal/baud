//! The fonts baud carries inside its binary: baud's embedded font in four
//! styles -- JetBrains Mono, with baud's own Hebrew in every style --
//! Symbols Nerd Font Mono -- the Nerd Fonts icons alone, sized to one cell
//! -- behind them, and STIX Two Math last, for the symbols no other default
//! face has (the owner's call, 2026-10-02). So a machine with no font
//! configured still draws JetBrains Mono, its ligatures, real bold and
//! italic, Hebrew, and the icons. The owner's choice, 2026-09-29: the plain
//! styles plus the icon face rather than four patched Nerd Font files, which
//! carry the same ~10,600 icons again in every style.
//!
//! The Regular and Bold are `fonts/built/Regular.ttf` and `Bold.ttf`:
//! JetBrains Mono v2.304's Regular and Bold with 52 Hebrew code points added
//! -- the letters drawn from Cascadia Mono's Hebrew and the points redrawn on
//! JetBrains Mono's dot -- and every stock glyph, advance, mapping and GSUB
//! byte kept (`the_built_font_keeps_jetbrains_mono`), save one script
//! record: GSUB and GPOS each name DFLT's Script under `hebr` too, and
//! nameID 7 carries Cascadia's trademark notice after JetBrains Mono's
//! (`the_built_font_carries_hebr_and_both_trademark_notices`).
//! `scripts/build-fonts.sh` builds them from `fonts/jetbrains-mono/` and
//! `fonts/glyphs/languages/hebrew/` and refuses any bytes but these;
//! `fonts/built/MANIFEST` pins how, and `fonts/built/FONTLOG.txt` says what
//! changed, what was picked for the meantime and what is still the owner's.
//! The Italic and Bold Italic are `fonts/built/Italic.ttf` and
//! `BoldItalic.ttf`: JetBrains Mono v2.304's Italic and Bold Italic with the
//! same 52 Hebrew code points, the upright Hebrew leaning right 9 degrees
//! with the Latin (the owner's call, 2026-10-24) and its rounds corrected as
//! JetBrains Mono's own italic corrects them, in the built family's name, so
//! the four are one family that a `fonts` name finds (Jev, N_SHARE,
//! 0.92/0.96). Nerd Fonts v3.5.1's
//! `SymbolsNerdFontMono-Regular.ttf` and STIX Two v2.13b171's
//! `STIXTwoMath-Regular.otf` are unmodified. Each source directory's `SOURCE`
//! records where its files came from, and the licences sit beside them and
//! ship in the macOS app bundle (`scripts/bundle-macos.sh`); nothing packages
//! them for Linux yet.

/// A face compiled into the binary: its bytes, and a name for the log.
#[derive(Clone, Copy)]
pub struct Embedded {
    pub name: &'static str,
    pub bytes: &'static [u8],
}

/// JetBrains Mono Regular with baud's Hebrew: the first face of the default
/// stack, and the face behind a configured one.
pub const REGULAR: Embedded = Embedded {
    name: "Regular.ttf, JetBrains Mono with baud's Hebrew (embedded)",
    bytes: include_bytes!("../fonts/built/Regular.ttf"),
};

/// JetBrains Mono Bold with baud's Hebrew: the face bold text is drawn from.
pub const BOLD: Embedded = Embedded {
    name: "Bold.ttf, JetBrains Mono Bold with baud's Hebrew (embedded)",
    bytes: include_bytes!("../fonts/built/Bold.ttf"),
};

/// JetBrains Mono Italic with baud's Hebrew, leaning right with the Latin:
/// the face italic text is drawn from.
pub const ITALIC: Embedded = Embedded {
    name: "Italic.ttf, JetBrains Mono Italic with baud's Hebrew (embedded)",
    bytes: include_bytes!("../fonts/built/Italic.ttf"),
};

/// JetBrains Mono Bold Italic with baud's Hebrew, leaning right with the
/// Latin: the face bold italic text is drawn from.
pub const BOLD_ITALIC: Embedded = Embedded {
    name: "BoldItalic.ttf, JetBrains Mono Bold Italic with baud's Hebrew (embedded)",
    bytes: include_bytes!("../fonts/built/BoldItalic.ttf"),
};

/// The icon face: behind the text faces in the default stack, and behind a
/// configured `fonts` stack unless `font_fallback = false`.
pub const SYMBOLS: Embedded = Embedded {
    name: "SymbolsNerdFontMono-Regular.ttf (embedded)",
    bytes: include_bytes!("../fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
};

/// The symbol face, last in the default stack on both systems: U+23F5 and
/// U+23FA, which Claude Code draws, are in no other default face, and of
/// every font installed on macOS only STIX Two Math has them as text.
pub const STIX: Embedded = Embedded {
    name: "STIXTwoMath-Regular.otf (embedded)",
    bytes: include_bytes!("../fonts/stix-two-math/STIXTwoMath-Regular.otf"),
};

#[cfg(test)]
mod tests {
    use super::*;
    use baud_domain::{Codepoint, GlyphId};
    use baud_font::{Cluster, Face, FaceIndex, Features, Glyph, Ignorable, Shaper};

    fn face(e: Embedded) -> Face<'static> {
        Face::parse_index(e.bytes, FaceIndex::of(0)).expect("a face")
    }

    /// The bytes compiled in are the files the SOURCE files and the built
    /// font's MANIFEST name: each file's length and an FNV-1a over every
    /// byte, so a replaced, rebuilt or truncated file fails here rather than
    /// drawing something else; and each style is the style its font says it
    /// is. All four text faces are the built files, whose sha256s MANIFEST
    /// records as `scripts/build-fonts.sh`'s outputs.
    #[test]
    fn the_embedded_fonts_are_the_pinned_releases() {
        let fnv = |b: &[u8]| {
            b.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &x| {
                (h ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3)
            })
        };
        // `head.macStyle`, read from the table directory: bit 0 bold, bit 1
        // italic. The font's own claim about its style, not its file name's.
        let mac_style = |b: &[u8]| {
            let be16 = |at: usize| u16::from_be_bytes([b[at], b[at + 1]]);
            let be32 = |at: usize| u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
            (0..usize::from(be16(4)))
                .map(|i| 12 + 16 * i)
                .find(|&rec| &b[rec..rec + 4] == b"head")
                .map(|rec| be16(usize::try_from(be32(rec + 8)).unwrap() + 44))
                .expect("a head table")
        };
        for (e, len, hash, style) in [
            (REGULAR, 279_896, 0x93ca_b38f_36ee_e20b_u64, 0),
            (BOLD, 284_460, 0x4085_4d87_8c3f_2868, 1),
            (ITALIC, 283_296, 0xc1cd_4ef4_0248_7116, 2),
            (BOLD_ITALIC, 286_868, 0x0d2d_8cad_ff35_adc4, 3),
            (SYMBOLS, 2_610_012, 0x0b68_1ba2_bef4_facd, 0),
            (STIX, 838_652, 0x8f3f_ef33_d83d_1544, 0),
        ] {
            assert_eq!(e.bytes.len(), len, "{}", e.name);
            assert_eq!(fnv(e.bytes), hash, "{}", e.name);
            assert_eq!(mac_style(e.bytes), style, "{}: head.macStyle", e.name);
        }
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/fonts");
        let jbm = std::fs::read_to_string(format!("{dir}/jetbrains-mono/SOURCE")).unwrap();
        for pin in [
            "6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf",
            "5590990c82e097397517f275f430af4546e1c45cff408bde4255dad142479dcb",
        ] {
            assert!(jbm.contains(pin), "{pin}");
        }
        let sym = std::fs::read_to_string(format!("{dir}/symbols-nerd-font/SOURCE")).unwrap();
        assert!(sym.contains("fe471e538392f51910faab985fa8e192a39dd3426125edd15b71b3680df0e749"));
        assert!(sym.contains("2610012"));
        let stix = std::fs::read_to_string(format!("{dir}/stix-two-math/SOURCE")).unwrap();
        assert!(stix.contains("3a5f3f26f40d5698b3c62dd085d48d6663696a3f80825aab8b553d5097518e8c"));
        assert!(stix.contains("838652"));
        let stix_ofl = std::fs::read_to_string(format!("{dir}/stix-two-math/OFL.txt")).unwrap();
        assert!(stix_ofl.contains("The STIX Fonts Project Authors"));
        assert!(stix_ofl.contains("SIL OPEN FONT LICENSE Version 1.1"));
        let ofl = std::fs::read_to_string(format!("{dir}/jetbrains-mono/OFL.txt")).unwrap();
        assert!(ofl.contains("Copyright 2020 The JetBrains Mono Project Authors"));
        assert!(ofl.contains("SIL OPEN FONT LICENSE Version 1.1"));
        // The built font: what MANIFEST says the build wrote, the Hebrew's
        // source and its licence, and the licence the built files carry --
        // both projects' copyright lines and Cascadia's Reserved Font Name,
        // which binds the built font as a modified version.
        let manifest = std::fs::read_to_string(format!("{dir}/built/MANIFEST")).unwrap();
        for out in [
            "output 769a5d99505247e42c8cfd0d013f68a1ad1cf5f4c64ccd96d73f1a3387c881bb baud-app/fonts/built/Regular.ttf",
            "output 13ce890885c189eeb3c21bef5cca32f4f96c362383fb1ecc7a428154f5575fe8 baud-app/fonts/built/Bold.ttf",
            "output c6b0e8a3375541de332ab95249471c5da8dcc0e4c5d801e8bbd2079e813712be baud-app/fonts/built/Italic.ttf",
            "output 183877058f5ee024254897522bee85066f5b795350f7bba72e90ca9a45f503f1 baud-app/fonts/built/BoldItalic.ttf",
        ] {
            assert!(manifest.contains(out), "MANIFEST: {out}");
        }
        let hebrew = format!("{dir}/glyphs/languages/hebrew");
        let src = std::fs::read_to_string(format!("{hebrew}/SOURCE")).unwrap();
        assert!(src.contains("08026f617eea315caca59898784e999f6cdb2d0c34d2040deeb042bcdc359fa2"));
        assert!(src.contains("714804"));
        let cascadia = std::fs::read_to_string(format!("{hebrew}/OFL.txt")).unwrap();
        assert!(cascadia.contains("Copyright (c) 2019 - Present, Microsoft Corporation"));
        assert!(cascadia.contains("SIL OPEN FONT LICENSE Version 1.1"));
        let built = std::fs::read_to_string(format!("{dir}/built/OFL.txt")).unwrap();
        for line in [
            "Copyright 2020 The JetBrains Mono Project Authors",
            "Copyright (c) 2019 - Present, Microsoft Corporation, with Reserved Font Name Cascadia Code.",
            "SIL OPEN FONT LICENSE Version 1.1",
        ] {
            assert!(built.contains(line), "built/OFL.txt: {line}");
        }
        let log = std::fs::read_to_string(format!("{dir}/built/FONTLOG.txt")).unwrap();
        assert!(log.contains("WHAT CHANGED AGAINST STOCK JETBRAINS MONO 2.304"));
        let authors = std::fs::read_to_string(format!("{dir}/built/AUTHORS.txt")).unwrap();
        assert!(authors.contains("Liron Lavi Turkenich"));
        let mit = std::fs::read_to_string(format!("{dir}/symbols-nerd-font/LICENSE")).unwrap();
        assert!(mit.contains("The MIT License"));
        let readme = std::fs::read_to_string(format!("{dir}/symbols-nerd-font/README.md")).unwrap();
        assert!(
            readme.contains("| Icon set name"),
            "Nerd Fonts' icon-set licence table"
        );
    }

    /// **The embedded font is one family of four**: its Regular, Bold,
    /// Italic and Bold Italic share one family name, as the OpenType name
    /// table's nameID 1 has a style-linking group share one
    /// (`[[source: ms-opentype-name]]`), so a `fonts` name finds all four.
    #[test]
    fn the_embedded_font_is_one_family() {
        let family = |e: Embedded| {
            let f = face(e);
            let mut out = [0u8; 128];
            f.family().and_then(|n| {
                n.utf8(&mut out)
                    .map(|b| String::from_utf8_lossy(b).into_owned())
            })
        };
        let regular = family(REGULAR).expect("a family name");
        for e in [BOLD, ITALIC, BOLD_ITALIC] {
            assert_eq!(family(e).as_deref(), Some(regular.as_str()), "{}", e.name);
        }
    }

    /// The bundle carries every licence file beside the binary that carries
    /// the fonts.
    #[test]
    fn the_app_bundle_ships_the_fonts_licences() {
        let script = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../scripts/bundle-macos.sh"
        ))
        .unwrap();
        for f in [
            "jetbrains-mono/OFL.txt",
            "jetbrains-mono/AUTHORS.txt",
            "symbols-nerd-font/LICENSE",
            "symbols-nerd-font/README.md",
            "stix-two-math/OFL.txt",
            "stix-two-math/SOURCE",
            "built/OFL.txt",
            "built/FONTLOG.txt",
            "built/AUTHORS.txt",
            "built/MANIFEST",
            "glyphs/languages/hebrew/OFL.txt",
            "glyphs/languages/hebrew/SOURCE",
        ] {
            assert!(script.contains(f), "bundle-macos.sh copies {f}");
        }
    }

    /// The Hebrew `scripts/build-fonts.sh` adds to JetBrains Mono, 52 code
    /// points: the 20 points, and maqaf, paseq and sof pasuq, in U+05B0-05C5
    /// and U+05C7; the 27 letters U+05D0-05EA; and geresh and gershayim
    /// U+05F3-05F4. Read from the four built styles with fontTools 4.63.0, a
    /// reader that is not baud's.
    fn shipped() -> Vec<Codepoint> {
        (0x05B0..=0x05C5)
            .chain([0x05C7])
            .chain(0x05D0..=0x05EA)
            .chain([0x05F3, 0x05F4])
            .map(|v| Codepoint::new(v).unwrap())
            .collect()
    }

    /// A stock JetBrains Mono file, the base the built styles are made from.
    fn stock(file: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/fonts/jetbrains-mono/{file}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    /// **Every style maps the Hebrew it ships** -- the owner's boxes, read
    /// at the font. Regular, Bold, Italic and Bold Italic each map all 52
    /// code points and nothing else in the Hebrew block (U+05C6, nun
    /// hafukha, is one the build does not draw); the stock files they were
    /// built from map none, the control that the Hebrew is the build's. So
    /// italic Hebrew is drawn from the italic faces, not upright from the
    /// chain.
    #[test]
    fn every_style_maps_the_hebrew_it_ships() {
        let hebrew = shipped();
        assert_eq!(hebrew.len(), 52, "the list itself");
        let block = |f: &Face<'_>| -> Vec<Codepoint> {
            (0x0590..=0x05FF)
                .filter_map(Codepoint::new)
                .filter(|&c| f.glyph(c).is_some())
                .collect()
        };
        for (e, base) in [
            (REGULAR, "JetBrainsMono-Regular.ttf"),
            (BOLD, "JetBrainsMono-Bold.ttf"),
            (ITALIC, "JetBrainsMono-Italic.ttf"),
            (BOLD_ITALIC, "JetBrainsMono-BoldItalic.ttf"),
        ] {
            let f = face(e);
            for &c in &hebrew {
                assert!(
                    f.glyph(c).is_some(),
                    "{}: U+{:04X} is not mapped, so it draws as a box",
                    e.name,
                    c.value()
                );
            }
            assert_eq!(
                block(&f),
                hebrew,
                "{}: the Hebrew block maps exactly the 52",
                e.name
            );
            let data = stock(base);
            assert!(
                block(&Face::parse(&data).unwrap()).is_empty(),
                "the control: stock {base} maps Hebrew"
            );
        }
    }

    /// How far a glyph's outline leans: the least-squares slope of x on y
    /// over every point its outline names, on- and off-curve -- 0 for a
    /// shape with no lean, tan(9 degrees) = 0.158 added by a 9-degree shear
    /// to the right.
    fn lean(f: &Face<'_>, c: Codepoint) -> f64 {
        let g = f.glyph(c).expect("mapped");
        let mut p = Path(Vec::new());
        f.outline(g, &mut p);
        let pts: Vec<(f64, f64)> =
            p.0.iter()
                .flat_map(|cmd| {
                    // Path's kinds: 0 move, 1 line, 2 quad, 3 cubic, 4 close.
                    let n = match cmd[0] {
                        k if k < 1.5 => 1,
                        k if k < 2.5 => 2,
                        k if k < 3.5 => 3,
                        _ => 0,
                    };
                    (0..n).map(move |k| (f64::from(cmd[1 + 2 * k]), f64::from(cmd[2 + 2 * k])))
                })
                .collect();
        let n = f64::from(u32::try_from(pts.len()).unwrap());
        let (mx, my) = pts
            .iter()
            .fold((0.0, 0.0), |(a, b), (x, y)| (a + x / n, b + y / n));
        let (sxy, syy) = pts.iter().fold((0.0, 0.0), |(a, b), (x, y)| {
            (a + (x - mx) * (y - my), b + (y - my) * (y - my))
        });
        sxy / syy
    }

    /// **Italic Hebrew leans right, with the Latin** -- the owner's call of
    /// 2026-10-24 ("learn right R"). Every Hebrew letter of the Italic and
    /// Bold Italic leans right of its upright by 9 degrees, JetBrains Mono
    /// Italic's angle: its outline's slope, x on y, exceeds the Regular's or
    /// the Bold's by 0.158, within 0.03. The control: JetBrains Mono's own
    /// Latin in the same files leans by the same angle the same way (`l`,
    /// `H`), so a Hebrew leaning left, or upright, fails beside it.
    #[test]
    fn italic_hebrew_leans_right_with_the_latin() {
        let tan9 = 9.0_f64.to_radians().tan();
        let mut off = Vec::new();
        for (it, up) in [(ITALIC, REGULAR), (BOLD_ITALIC, BOLD)] {
            let (fi, fu) = (face(it), face(up));
            for c in (0x05D0..=0x05EA).filter_map(Codepoint::new) {
                let d = lean(&fi, c) - lean(&fu, c);
                if (d - tan9).abs() > 0.03 {
                    off.push(format!("{} U+{:04X}: {d:.3}", it.name, c.value()));
                }
            }
            for c in ['l', 'H'] {
                let c = Codepoint::new(u32::from(c)).unwrap();
                let d = lean(&fi, c) - lean(&fu, c);
                assert!(
                    (d - tan9).abs() <= 0.03,
                    "the control: {}'s {:?} leans {d:.3}",
                    it.name,
                    c
                );
            }
        }
        assert!(
            off.is_empty(),
            "letters not leaning 9 degrees right: {off:#?}"
        );
    }

    /// Where samekh's bowl sides end below: the lower end of each of its
    /// four side strokes (straight segments at least 20 units long that
    /// begin below y 300 and run up at the face's slant), left to right --
    /// outer left, inner left, inner right, outer right -- as heights.
    fn samekh_side_bottoms(f: &Face<'_>, slant: f64) -> Vec<f64> {
        let g = f.glyph(Codepoint::new(0x05E1).unwrap()).expect("mapped");
        let mut p = Path(Vec::new());
        f.outline(g, &mut p);
        let mut sides = Vec::new();
        let (mut at, mut start) = ((0.0, 0.0), (0.0, 0.0));
        for cmd in &p.0 {
            let pt = |k: usize| (f64::from(cmd[k]), f64::from(cmd[k + 1]));
            let next = match cmd[0] {
                k if k < 0.5 => {
                    start = pt(1);
                    at = start;
                    continue;
                }
                k if k < 1.5 => pt(1),
                k if k < 2.5 => pt(3),
                k if k < 3.5 => pt(5),
                _ => start,
            };
            let ((x0, y0), (x1, y1)) = if at.1 < next.1 {
                (at, next)
            } else {
                (next, at)
            };
            let straight = cmd[0] < 1.5 || cmd[0] > 3.5;
            if straight
                && y1 - y0 >= 20.0
                && y0 < 300.0
                && (x1 - x0 - slant * (y1 - y0)).abs() < 1.0
            {
                sides.push((x0 - slant * y0, y0));
            }
            at = next;
        }
        sides.sort_by(|a, b| a.0.total_cmp(&b.0));
        sides.into_iter().map(|(_, y)| y).collect()
    }

    /// **Italic samekh's bowl takes JetBrains Mono Italic's joint
    /// levelling.** JetBrains Mono's italic moves a bowl's outer
    /// curve/straight joint to its inner joint's height where the upright's
    /// outer straight stopped short (o 12.6-12.7 units, O 10.1); baud's
    /// italics move each Hebrew bowl's outer joint toward its inner one by
    /// the gap, at most 13 units (optical.py, `level_rec`). Samekh's
    /// upright gaps are 31.2 (Regular) and 45.5 (Bold) on both sides, so
    /// in the Italic and Bold Italic each side's gap is the upright's less
    /// 13, within a unit of rounding. A plain shear keeps the upright gap,
    /// and a full level leaves none: both fail.
    #[test]
    fn italic_samekh_levels_its_bowl_joints_by_up_to_13_units() {
        let tan9 = 9.0_f64.to_radians().tan();
        for (it, up) in [(ITALIC, REGULAR), (BOLD_ITALIC, BOLD)] {
            let (bi, bu) = (
                samekh_side_bottoms(&face(it), tan9),
                samekh_side_bottoms(&face(up), 0.0),
            );
            assert_eq!(
                (bi.len(), bu.len()),
                (4, 4),
                "{}: four sides, {bi:?} {bu:?}",
                it.name
            );
            for (outer, inner) in [(0, 1), (3, 2)] {
                let (gu, gi) = (bu[outer] - bu[inner], bi[outer] - bi[inner]);
                assert!(gu > 13.0, "{}: the upright gap {gu}", up.name);
                assert!(
                    (gi - (gu - 13.0)).abs() <= 1.0,
                    "{}: side {outer}'s joint gap is {gi:.1}, the upright's {gu:.1} less 13 is {:.1}",
                    it.name,
                    gu - 13.0
                );
            }
        }
    }

    /// Records an outline's commands exactly, to compare two glyphs.
    struct Path(Vec<[f32; 7]>);

    impl baud_font::Sink for Path {
        fn move_to(&mut self, x: f32, y: f32) {
            self.0.push([0.0, x, y, 0.0, 0.0, 0.0, 0.0]);
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.0.push([1.0, x, y, 0.0, 0.0, 0.0, 0.0]);
        }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            self.0.push([2.0, x1, y1, x, y, 0.0, 0.0]);
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            self.0.push([3.0, x1, y1, x2, y2, x, y]);
        }
        fn close(&mut self) {
            self.0.push([4.0; 7]);
        }
    }

    /// Where table `tag` lies in a font file, read from its table
    /// directory: (offset, length).
    fn table(data: &[u8], tag: &[u8; 4]) -> (usize, usize) {
        let be16 = |at: usize| usize::from(u16::from_be_bytes([data[at], data[at + 1]]));
        let be32 = |at: usize| {
            usize::try_from(u32::from_be_bytes([
                data[at],
                data[at + 1],
                data[at + 2],
                data[at + 3],
            ]))
            .unwrap()
        };
        (0..be16(4))
            .map(|k| 12 + 16 * k)
            .find(|&rec| &data[rec..rec + 4] == tag)
            .map(|rec| (be32(rec + 8), be32(rec + 12)))
            .unwrap()
    }

    /// Table `tag`'s bytes.
    fn table_bytes<'a>(data: &'a [u8], tag: &[u8; 4]) -> &'a [u8] {
        let (at, len) = table(data, tag);
        &data[at..at + len]
    }

    /// What a Script table says: its default language system's required
    /// feature and feature indices, and how many other language systems it
    /// has.
    type Script = (u16, Vec<u16>, u16);

    /// A GSUB or GPOS table's script records, each tag with its Script.
    fn scripts(t: &[u8]) -> Vec<([u8; 4], Script)> {
        let be16 = |at: usize| u16::from_be_bytes([t[at], t[at + 1]]);
        let list = usize::from(be16(4));
        (0..usize::from(be16(list)))
            .map(|i| {
                let rec = list + 2 + 6 * i;
                let script = list + usize::from(be16(rec + 4));
                let lang = script + usize::from(be16(script));
                let features = (0..usize::from(be16(lang + 4)))
                    .map(|k| be16(lang + 6 + 2 * k))
                    .collect();
                (
                    t[rec..rec + 4].try_into().unwrap(),
                    (be16(lang + 2), features, be16(script + 2)),
                )
            })
            .collect()
    }

    /// A GSUB or GPOS table with its `hebr` script record taken out and the
    /// offsets that cross it moved back 6: what the build's
    /// `with_script_copy` adds, undone (build.py), so the rest can be held
    /// to the stock table byte for byte. A table with no `hebr` record is
    /// returned as it is.
    fn without_hebr(t: &[u8]) -> Vec<u8> {
        let be16 = |at: usize| usize::from(u16::from_be_bytes([t[at], t[at + 1]]));
        let (list, n) = (be16(4), be16(be16(4)));
        let Some(k) = (0..n).find(|&i| &t[list + 2 + 6 * i..list + 6 + 6 * i] == b"hebr") else {
            return t.to_vec();
        };
        // Version 1.0: the ScriptList follows the ten-byte header.
        assert_eq!((be16(0), be16(2), list), (1, 0, 10), "header");
        let mut out = t[..6].to_vec();
        let put = |out: &mut Vec<u8>, v: usize| {
            out.extend_from_slice(&u16::try_from(v).unwrap().to_be_bytes());
        };
        put(&mut out, be16(6) - 6);
        put(&mut out, be16(8) - 6);
        put(&mut out, n - 1);
        for rec in (0..n).filter(|&i| i != k).map(|i| list + 2 + 6 * i) {
            out.extend_from_slice(&t[rec..rec + 4]);
            put(&mut out, be16(rec + 4) - 6);
        }
        out.extend_from_slice(&t[list + 2 + 6 * n..]);
        out
    }

    /// Which of the six properties of "JetBrains Mono, with the Hebrew
    /// added" `built` fails against `base`, by name; empty when it keeps
    /// them all. GSUB is held to the stock table byte for byte once its
    /// `hebr` script record is taken out; that the record is DFLT's is
    /// `the_built_font_carries_hebr_and_both_trademark_notices`'s.
    fn lost(built: &Face<'_>, base: &Face<'_>) -> Vec<&'static str> {
        let mut lost = Vec::new();
        let gsub = baud_font::Tag::GSUB;
        if built.table(gsub).map(|t| without_hebr(t.as_slice()))
            != base.table(gsub).map(|t| t.as_slice().to_vec())
        {
            lost.push("GSUB");
        }
        if u32::from(built.glyph_count().value()) != u32::from(base.glyph_count().value()) + 52 {
            lost.push("glyph count");
        }
        // Every code point the stock file maps, to the same glyph.
        let mut same = true;
        base.candidates(|lo, hi| {
            for c in (lo.value()..=hi.value()).filter_map(Codepoint::new) {
                if let Some(g) = base.glyph(c) {
                    same &= built.glyph(c) == Some(g);
                }
            }
        });
        if !same {
            lost.push("cmap");
        }
        // What the built file maps and the stock does not is the 52.
        let mut added = Vec::new();
        built.candidates(|lo, hi| {
            for c in (lo.value()..=hi.value()).filter_map(Codepoint::new) {
                if built.glyph(c).is_some() && base.glyph(c).is_none() {
                    added.push(c);
                }
            }
        });
        added.sort_by_key(|c| c.value());
        added.dedup();
        if added != shipped() {
            lost.push("added");
        }
        let glyphs = (0..base.glyph_count().value()).map(GlyphId::from_shaper);
        if glyphs.clone().any(|g| built.advance(g) != base.advance(g)) {
            lost.push("advance");
        }
        let outline = |f: &Face<'_>, g| {
            let mut p = Path(Vec::new());
            let drawn = f.outline(g, &mut p);
            (drawn, p.0)
        };
        if glyphs
            .into_iter()
            .any(|g| outline(built, g) != outline(base, g))
        {
            lost.push("outline");
        }
        lost
    }

    /// **The built font keeps JetBrains Mono**: each built style against
    /// the stock file of its style keeps GSUB byte for byte (once the `hebr`
    /// script record is taken out), maps every code
    /// point the stock file maps to the same glyph and adds exactly the 52,
    /// carries the stock glyphs plus 52, and keeps every stock glyph's
    /// advance and outline -- the Latin, its ligatures and the icons'
    /// neighbours draw as they did. Each check has its control, so none of
    /// them is a probe that cannot fire.
    #[test]
    fn the_built_font_keeps_jetbrains_mono() {
        let (r, b, i, bi) = (
            stock("JetBrainsMono-Regular.ttf"),
            stock("JetBrainsMono-Bold.ttf"),
            stock("JetBrainsMono-Italic.ttf"),
            stock("JetBrainsMono-BoldItalic.ttf"),
        );
        let (r, b, i, bi) = (
            Face::parse(&r).unwrap(),
            Face::parse(&b).unwrap(),
            Face::parse(&i).unwrap(),
            Face::parse(&bi).unwrap(),
        );
        assert_eq!(lost(&face(REGULAR), &r), Vec::<&str>::new(), "Regular");
        assert_eq!(lost(&face(BOLD), &b), Vec::<&str>::new(), "Bold");
        assert_eq!(lost(&face(ITALIC), &i), Vec::<&str>::new(), "Italic");
        assert_eq!(
            lost(&face(BOLD_ITALIC), &bi),
            Vec::<&str>::new(),
            "Bold Italic"
        );
        // The controls. The stock file the build starts from fails exactly
        // the two checks about what the build adds.
        assert_eq!(lost(&r, &r), ["glyph count", "added"], "stock over stock");
        // Bold against Regular differs in outline only: JetBrains Mono is
        // monospaced and its upright styles share one glyph order.
        assert_eq!(lost(&face(BOLD), &r), ["outline"], "Bold over Regular");
        // The Italic maps the same characters, to glyphs in another order.
        assert_eq!(
            lost(&face(REGULAR), &i),
            ["GSUB", "glyph count", "cmap", "advance", "outline"],
            "Regular over Italic"
        );
        // One advance changed, and one GSUB byte: each fails its own check
        // and only it.
        let mut wider = REGULAR.bytes.to_vec();
        let (hmtx, _) = table(&wider, b"hmtx");
        // Glyph 1's advance, the second longHorMetric.
        wider[hmtx + 5] ^= 1;
        assert_eq!(lost(&Face::parse(&wider).unwrap(), &r), ["advance"]);
        let mut gsub = REGULAR.bytes.to_vec();
        let (at, len) = table(&gsub, b"GSUB");
        gsub[at + len - 1] ^= 1;
        assert_eq!(lost(&Face::parse(&gsub).unwrap(), &r), ["GSUB"]);
    }

    /// A font's nameID 7 records: (platform, encoding, language, text).
    fn trademarks(data: &[u8]) -> Vec<(u16, u16, u16, String)> {
        let t = table_bytes(data, b"name");
        let be16 = |at: usize| u16::from_be_bytes([t[at], t[at + 1]]);
        let strings = usize::from(be16(4));
        (0..usize::from(be16(2)))
            .map(|i| 6 + 12 * i)
            .filter(|&rec| be16(rec + 6) == 7)
            .map(|rec| {
                let at = strings + usize::from(be16(rec + 10));
                let text = &t[at..at + usize::from(be16(rec + 8))];
                let text = if be16(rec) == 1 {
                    String::from_utf8_lossy(text).into_owned()
                } else {
                    let units: Vec<u16> = text
                        .chunks(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    String::from_utf16(&units).unwrap()
                };
                (be16(rec), be16(rec + 2), be16(rec + 4), text)
            })
            .collect()
    }

    /// Which of the four `data` fails, by name: GSUB and GPOS each carry
    /// exactly DFLT, `hebr` and latn, in tag order, with `hebr` saying what
    /// DFLT says; GPOS's `hebr` names the mark and mkmk features; and
    /// nameID 7 is `notice`, in one Windows-English record.
    fn not_carried(data: &[u8], notice: &str) -> Vec<&'static str> {
        let mut not = Vec::new();
        for (tag, name) in [(b"GSUB", "GSUB hebr"), (b"GPOS", "GPOS hebr")] {
            let s = scripts(table_bytes(data, tag));
            let of = |want: &[u8; 4]| s.iter().find(|(t, _)| t == want).map(|(_, v)| v);
            let tags: Vec<[u8; 4]> = s.iter().map(|(t, _)| *t).collect();
            if tags != [*b"DFLT", *b"hebr", *b"latn"] || of(b"hebr") != of(b"DFLT") {
                not.push(name);
            }
        }
        let gpos = table_bytes(data, b"GPOS");
        let features = usize::from(u16::from_be_bytes([gpos[6], gpos[7]]));
        let hebr: Option<Vec<&[u8]>> =
            scripts(gpos)
                .into_iter()
                .find(|(t, _)| t == b"hebr")
                .map(|(_, (_, indices, _))| {
                    indices
                        .iter()
                        .map(|&k| {
                            let rec = features + 2 + 6 * usize::from(k);
                            &gpos[rec..rec + 4]
                        })
                        .collect()
                });
        if hebr != Some(vec![b"mark".as_slice(), b"mkmk".as_slice()]) {
            not.push("GPOS hebr mark");
        }
        if trademarks(data) != [(3, 1, 0x409, notice.to_owned())] {
            not.push("nameID 7");
        }
        not
    }

    /// **The built font carries a `hebr` script record and both trademark
    /// notices** -- the two conformance fixes of 2026-10-08. OpenType's DFLT
    /// rule is met without a `hebr` record, but Microsoft's Hebrew script
    /// spec registers `hebr`, and HarfBuzz's Hebrew shaper applies GPOS only
    /// when the script it chose is `hebr` (note AB 7.3); with DFLT alone it
    /// places the points by combining class and the anchors go unused. So
    /// GSUB and GPOS each carry `hebr` naming what DFLT names -- in GPOS,
    /// mark and mkmk. baud selects latn first, so what baud draws does not
    /// move. And nameID 7 is JetBrains Mono's notice followed by Cascadia's,
    /// each as its own source file carries it: "Any trademark notices must
    /// remain in any derivative fonts" (OFL-FAQ 1.1-update7, Q3.7). The
    /// controls: the stock files fail all four, and a copy of the built
    /// Regular with its record renamed, or pointed at latn's Script, fails
    /// exactly what that breaks.
    #[test]
    fn the_built_font_carries_hebr_and_both_trademark_notices() {
        let jbm = "JetBrains Mono is a trademark of JetBrains s.r.o.";
        let cascadia = "Cascadia Code is a trademark of the Microsoft group of companies.";
        let source = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fonts/glyphs/languages/hebrew/CascadiaMono.ttf"
        ))
        .unwrap();
        assert_eq!(
            trademarks(&source),
            [(3, 1, 0x409, cascadia.to_owned())],
            "Cascadia Mono's own notice"
        );
        let both = format!("{jbm} {cascadia}");
        for (e, base) in [
            (REGULAR, "JetBrainsMono-Regular.ttf"),
            (BOLD, "JetBrainsMono-Bold.ttf"),
            (ITALIC, "JetBrainsMono-Italic.ttf"),
            (BOLD_ITALIC, "JetBrainsMono-BoldItalic.ttf"),
        ] {
            let stock = stock(base);
            assert_eq!(
                trademarks(&stock),
                [(3, 1, 0x409, jbm.to_owned())],
                "stock {base}'s own notice"
            );
            assert_eq!(
                not_carried(e.bytes, &both),
                Vec::<&str>::new(),
                "{}",
                e.name
            );
            assert_eq!(
                not_carried(&stock, &both),
                ["GSUB hebr", "GPOS hebr", "GPOS hebr mark", "nameID 7"],
                "the control: stock {base}"
            );
        }
        // The record found by its tag in a table at `at`: where it lies.
        let record = |data: &[u8], at: usize, want: &[u8; 4]| {
            let list = at + usize::from(u16::from_be_bytes([data[at + 4], data[at + 5]]));
            let n = usize::from(u16::from_be_bytes([data[list], data[list + 1]]));
            (0..n)
                .map(|i| list + 2 + 6 * i)
                .find(|&rec| &data[rec..rec + 4] == want)
                .unwrap()
        };
        let mut renamed = REGULAR.bytes.to_vec();
        let (gpos, _) = table(&renamed, b"GPOS");
        let rec = record(&renamed, gpos, b"hebr");
        renamed[rec + 3] = b'x';
        assert_eq!(
            not_carried(&renamed, &both),
            ["GPOS hebr", "GPOS hebr mark"],
            "GPOS's hebr renamed hebx"
        );
        let mut latin = REGULAR.bytes.to_vec();
        let (gsub, _) = table(&latin, b"GSUB");
        let (hebr, latn) = (record(&latin, gsub, b"hebr"), record(&latin, gsub, b"latn"));
        let to = [latin[latn + 4], latin[latn + 5]];
        latin[hebr + 4..hebr + 6].copy_from_slice(&to);
        assert_eq!(
            not_carried(&latin, &both),
            ["GSUB hebr"],
            "GSUB's hebr pointed at latn's Script"
        );
    }

    /// The icons are in the icon face and not in the text faces, which is
    /// why it stands behind them.
    #[test]
    fn the_icon_face_carries_the_icons_the_text_faces_lack() {
        let icons = face(SYMBOLS);
        for cp in [0xE0A0, 0xF07B, 0xE7A8, 0xF0001] {
            let c = Codepoint::new(cp).unwrap();
            assert!(icons.glyph(c).is_some(), "U+{cp:04X} is in the icon face");
        }
        let folder = Codepoint::new(0xF07B).unwrap();
        for e in [REGULAR, BOLD, ITALIC, BOLD_ITALIC] {
            assert!(face(e).glyph(folder).is_none(), "{}: no Nerd icon", e.name);
        }
    }

    /// The symbols the symbol face is there for are in it and in no face in
    /// front of it: Claude Code's mode arrow U+23F5 and its bullet U+23FA.
    #[test]
    fn the_symbol_face_carries_what_the_faces_before_it_lack() {
        let stix = face(STIX);
        for cp in [0x23F5, 0x23FA] {
            let c = Codepoint::new(cp).unwrap();
            assert!(stix.glyph(c).is_some(), "U+{cp:04X} is in STIX Two Math");
            for e in [REGULAR, SYMBOLS] {
                assert!(face(e).glyph(c).is_none(), "{}: no U+{cp:04X}", e.name);
            }
        }
    }

    /// What the owner chose JetBrains Mono for: `=>` joined under the
    /// features baud shapes a grid with, in every style.
    #[test]
    fn every_style_forms_the_ligatures() {
        for e in [REGULAR, BOLD, ITALIC, BOLD_ITALIC] {
            let f = face(e);
            let shape = |s: &str| {
                let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
                let mut shaper = Shaper::new(&f, &mut storage);
                for (i, c) in s.chars().enumerate() {
                    let cp = [Codepoint::new(u32::from(c)).unwrap()];
                    shaper
                        .push_cluster(&cp, Cluster(u16::try_from(i).unwrap()))
                        .unwrap();
                }
                // "=>" names no script: `DFLT`, as the specification says.
                let (buf, _) = shaper.finish(Features::GRID, baud_font::Script::COMMON);
                buf.as_slice().iter().map(|g| g.id).collect::<Vec<_>>()
            };
            let plain = |c: char| f.glyph(Codepoint::new(u32::from(c)).unwrap()).unwrap();
            assert_ne!(
                shape("=>"),
                [plain('='), plain('>')],
                "{}: => joins",
                e.name
            );
            assert_eq!(
                shape("ab"),
                [plain('a'), plain('b')],
                "{}: the control",
                e.name
            );
        }
    }
}
