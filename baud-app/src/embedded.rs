//! The fonts baud carries inside its binary: JetBrains Mono in its four
//! styles, Symbols Nerd Font Mono -- the Nerd Fonts icons alone, sized to
//! one cell -- behind them, and STIX Two Math last, for the symbols no other
//! default face has (the owner's call, 2026-10-02). So a machine with no font configured still
//! draws JetBrains Mono, its ligatures, real bold and italic, and the icons.
//! The owner's choice, 2026-09-29: the plain styles plus the icon face
//! rather than four patched Nerd Font files, which carry the same ~10,600
//! icons again in every style.
//!
//! The files are JetBrains Mono v2.304's `fonts/ttf/JetBrainsMono-*.ttf`,
//! Nerd Fonts v3.5.1's `SymbolsNerdFontMono-Regular.ttf` and STIX Two
//! v2.13b171's `STIXTwoMath-Regular.otf`, unmodified; each
//! directory's `SOURCE` records where they came from, and their licences sit
//! beside them and ship in the app bundle.

/// A face compiled into the binary: its bytes, and a name for the log.
#[derive(Clone, Copy)]
pub struct Embedded {
    pub name: &'static str,
    pub bytes: &'static [u8],
}

pub const REGULAR: Embedded = Embedded {
    name: "JetBrainsMono-Regular.ttf (embedded)",
    bytes: include_bytes!("../fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"),
};

pub const BOLD: Embedded = Embedded {
    name: "JetBrainsMono-Bold.ttf (embedded)",
    bytes: include_bytes!("../fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
};

pub const ITALIC: Embedded = Embedded {
    name: "JetBrainsMono-Italic.ttf (embedded)",
    bytes: include_bytes!("../fonts/jetbrains-mono/JetBrainsMono-Italic.ttf"),
};

pub const BOLD_ITALIC: Embedded = Embedded {
    name: "JetBrainsMono-BoldItalic.ttf (embedded)",
    bytes: include_bytes!("../fonts/jetbrains-mono/JetBrainsMono-BoldItalic.ttf"),
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

    /// The bytes compiled in are the releases the SOURCE files name: each
    /// file's length and an FNV-1a over every byte, so a replaced or
    /// truncated file fails here rather than drawing something else; and
    /// each style is the style its font says it is.
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
            (REGULAR, 273_900, 0x4a1b_35e9_fc69_20d2_u64, 0),
            (BOLD, 277_828, 0x6ab8_99db_bae4_1e07, 1),
            (ITALIC, 276_840, 0x328e_eb84_de91_fb86, 2),
            (BOLD_ITALIC, 279_832, 0x2643_2855_1691_33bf, 3),
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
        let mit = std::fs::read_to_string(format!("{dir}/symbols-nerd-font/LICENSE")).unwrap();
        assert!(mit.contains("The MIT License"));
        let readme = std::fs::read_to_string(format!("{dir}/symbols-nerd-font/README.md")).unwrap();
        assert!(
            readme.contains("| Icon set name"),
            "Nerd Fonts' icon-set licence table"
        );
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
        ] {
            assert!(script.contains(f), "bundle-macos.sh copies {f}");
        }
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
                let (buf, _) = shaper.finish(Features::GRID);
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
