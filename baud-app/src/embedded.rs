//! The font baud carries inside its binary: the default stack's first face,
//! so a machine with no font configured still draws JetBrains Mono, its
//! ligatures and the Nerd Fonts icons. The owner's choice, 2026-09-29.
//!
//! The file is Nerd Fonts v3.5.1's `JetBrainsMonoNerdFontMono-Regular.ttf`,
//! unmodified; `fonts/jetbrains-mono-nerd-font/SOURCE` records where it came
//! from, and the release's `OFL.txt` and `README.md` sit beside it and ship
//! in the app bundle.

/// A face compiled into the binary: its bytes, and a name for the log.
#[derive(Clone, Copy)]
pub struct Embedded {
    pub name: &'static str,
    pub bytes: &'static [u8],
}

/// JetBrains Mono Nerd Font Mono, Regular.
pub const JETBRAINS_MONO: Embedded = Embedded {
    name: "JetBrainsMonoNerdFontMono-Regular.ttf (embedded)",
    bytes: include_bytes!(
        "../fonts/jetbrains-mono-nerd-font/JetBrainsMonoNerdFontMono-Regular.ttf"
    ),
};

#[cfg(test)]
mod tests {
    use super::*;
    use baud_domain::{Codepoint, GlyphId};
    use baud_font::{Cluster, Face, FaceIndex, Features, Glyph, Ignorable, Shaper};

    fn face() -> Face<'static> {
        Face::parse_index(JETBRAINS_MONO.bytes, FaceIndex::of(0)).expect("a face")
    }

    /// The bytes compiled in are the release SOURCE names: its length, and
    /// an FNV-1a over every byte, so a replaced or truncated file fails here
    /// rather than drawing something else.
    #[test]
    fn the_embedded_font_is_the_pinned_release() {
        let b = JETBRAINS_MONO.bytes;
        assert_eq!(b.len(), 2_573_248);
        let fnv = b.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &x| {
            (h ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3)
        });
        assert_eq!(fnv, 0x5869_0e52_8621_544c);
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fonts/jetbrains-mono-nerd-font"
        );
        let source = std::fs::read_to_string(format!("{dir}/SOURCE")).unwrap();
        assert!(
            source.contains("f2a5ea6cfab397445ffab00c0370927b66d61e560a05db5db271b42006381c1a")
        );
        assert!(source.contains("2573248"));
        let ofl = std::fs::read_to_string(format!("{dir}/OFL.txt")).unwrap();
        assert!(ofl.contains("Copyright 2020 The JetBrains Mono Project Authors"));
        assert!(ofl.contains("SIL OPEN FONT LICENSE Version 1.1"));
        let readme = std::fs::read_to_string(format!("{dir}/README.md")).unwrap();
        assert!(
            readme.contains("| Icon set name"),
            "Nerd Fonts' icon-set licence table"
        );
    }

    /// The bundle carries the two licence files beside the binary that
    /// carries the font.
    #[test]
    fn the_app_bundle_ships_the_fonts_licences() {
        let script = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../scripts/bundle-macos.sh"
        ))
        .unwrap();
        for f in ["OFL.txt", "README.md"] {
            assert!(
                script.contains(&format!("jetbrains-mono-nerd-font/{f}")),
                "bundle-macos.sh copies {f}"
            );
        }
    }

    /// What the owner chose it for: Powerline and Nerd icons in the face, and
    /// `=>` joined under the features baud shapes a grid with.
    #[test]
    fn it_carries_the_icons_and_forms_the_ligatures() {
        let f = face();
        for cp in [0x61, 0xE0A0, 0xE0B0, 0xF07B, 0xE7A8] {
            assert!(
                f.glyph(Codepoint::new(cp).unwrap()).is_some(),
                "U+{cp:04X} is in the face"
            );
        }
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
        assert_ne!(shape("=>"), [plain('='), plain('>')], "=> forms a ligature");
        assert_eq!(
            shape("ab"),
            [plain('a'), plain('b')],
            "the control does not"
        );
    }
}
