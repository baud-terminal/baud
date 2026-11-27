//! **The faces the film draws with, and the check that no system face is
//! needed.** baud's own font draws Latin, Hebrew with its points, box
//! drawing and the Nerd Font icons; the rest comes from open faces pinned
//! by release tag and SHA-256, fetched once into a cache outside the
//! repository, all five under the SIL Open Font License 1.1.
//!
//! baud asks the system for a face only for a character no face of its
//! chain draws (`crates/baud-app/src/fallback.rs`), and that tier stands behind
//! every stack, `font_fallback = false` included, so no setting keeps it
//! out. What keeps the film free of the system's glyphs is this check:
//! every character the session shows from what it controls resolves, by
//! baud-render's own face table, to baud's faces or a pinned one.

use std::collections::BTreeSet;

use baud_domain::Codepoint;
use baud_font::{Face, Script};
use baud_render::{EmojiDefault, FaceStyle, Stack};

/// An open face the session draws with, fetched from `url` and refused
/// unless it hashes to `sha256`.
pub struct Pinned {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub draws: &'static str,
}

impl Pinned {
    /// The file's name, as the cache and the scratch directory keep it.
    pub fn file(&self) -> &'static str {
        self.url.rsplit('/').next().unwrap_or(self.url)
    }
}

/// **The pinned faces, in the order baud tries them.** Noto Color Emoji
/// is the CBDT build baud-font reads, at tag `v2.051`; Noto Sans CJK SC at
/// `Sans2.004`, the two `tools/scripts/readme-capture.sh` pins; Noto Naskh
/// Arabic 2.021, Noto Sans Devanagari 2.007 and Noto Sans Symbols 2 2.008
/// from Noto's monthly release `noto-monthly-release-2026.10.01`. A Naskh
/// letter wider than its cell, as the medial sad is, is fitted to the cell
/// (`crates/baud-render/src/fit.rs`). One CJK face draws all three languages, its
/// Han in their Chinese forms, as baud picks a face by character and not
/// by language. Noto Sans Symbols 2 draws the whole Braille Patterns block,
/// U+2800 to U+28FF, which btop draws its graphs with and baud's own font
/// has none of: without it the film's braille came from the system.
pub const PINNED: [Pinned; 5] = [
    Pinned {
        name: "Noto Color Emoji",
        url: "https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.051/fonts/NotoColorEmoji.ttf",
        sha256: "72a635cb3d2f3524c51620cdde406b217204e8a6a06c6a096ff8ed4b5fd6e27b",
        draws: "emoji and flags",
    },
    Pinned {
        name: "Noto Sans CJK SC",
        url: "https://raw.githubusercontent.com/notofonts/noto-cjk/Sans2.004/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf",
        sha256: "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b",
        draws: "Chinese, Japanese, Korean",
    },
    Pinned {
        name: "Noto Naskh Arabic",
        url: "https://raw.githubusercontent.com/notofonts/notofonts.github.io/noto-monthly-release-2026.10.01/fonts/NotoNaskhArabic/unhinted/ttf/NotoNaskhArabic-Regular.ttf",
        sha256: "c34fdbd98af4dbc45ca192a23d2eeb77032add83086f5fe32957d23b3f36b221",
        draws: "Arabic",
    },
    Pinned {
        name: "Noto Sans Devanagari",
        url: "https://raw.githubusercontent.com/notofonts/notofonts.github.io/noto-monthly-release-2026.10.01/fonts/NotoSansDevanagari/unhinted/ttf/NotoSansDevanagari-Regular.ttf",
        sha256: "9c7d935139ea6a1e6ad9dbac4f6d27ece1e04bca8123c8888d00a0f9df4724cd",
        draws: "Hindi",
    },
    Pinned {
        name: "Noto Sans Symbols 2",
        url: "https://raw.githubusercontent.com/notofonts/notofonts.github.io/noto-monthly-release-2026.10.01/fonts/NotoSansSymbols2/unhinted/ttf/NotoSansSymbols2-Regular.ttf",
        sha256: "c4a0a80f0041ce4be81e2478faad22776d23edb98ae3f0d19bd37044820ecf9d",
        draws: "btop's braille",
    },
];

/// baud's own faces, the files the binary embeds (`crates/baud-app/src/embedded.rs`):
/// the text face, which the config names `baud Mono`, the icon
/// face, and STIX Two Math, which a configured stack keeps last.
pub const REGULAR: &[u8] = include_bytes!("../../fonts/built/Regular.ttf");
pub const SYMBOLS: &[u8] =
    include_bytes!("../../fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf");
pub const STIX: &[u8] = include_bytes!("../../fonts/stix-two-math/STIXTwoMath-Regular.otf");

/// **Whether baud would ask the system for a face for `c`** when no face
/// of its chain draws it: `fallback::asked_for`'s rule -- an assigned
/// character that is neither a control, a line or paragraph separator nor
/// default-ignorable.
pub fn asked_for(c: Codepoint) -> bool {
    Script::of(c) != Script::UNKNOWN
        && !matches!(c.value(), 0..=0x1f | 0x7f..=0x9f | 0x2028 | 0x2029)
        && !baud_domain::is_default_ignorable(c)
}

/// What the check found: for each face, the characters it draws, and the
/// characters no face draws.
pub struct Coverage {
    pub drawn: Vec<(String, Vec<char>)>,
    pub gaps: Vec<char>,
    pub checked: usize,
}

/// **The face each character of `text` is drawn from**, by baud-render's
/// own table: a `Stack` of `faces` in order, its `FaceTable`, each
/// character looked up in regular style with the presentation baud gives
/// it (`EmojiDefault::Font`, the default: `U+FE0F` after it asks for
/// colour). `gaps` is the table's own list of what no face draws -- the
/// list baud hands the system fallback -- less what it would not ask.
pub fn coverage(faces: &[(&str, &[u8])], text: &str) -> Result<Coverage, String> {
    let parsed = faces
        .iter()
        .map(|(name, bytes)| Face::parse(bytes).map_err(|e| format!("{name}: {e:?}")))
        .collect::<Result<Vec<Face>, String>>()?;
    let stack = Stack::new(&parsed).ok_or_else(|| "no faces, or more than 255".to_string())?;
    let mut table = stack.table();
    let chars: Vec<char> = text.chars().collect();
    let mut drawn: Vec<BTreeSet<char>> = vec![BTreeSet::new(); faces.len()];
    let mut checked = BTreeSet::new();
    for (i, &ch) in chars.iter().enumerate() {
        let c = Codepoint::from_char(ch);
        if !asked_for(c) {
            continue;
        }
        let next = chars.get(i + 1).map(|n| Codepoint::from_char(*n));
        let p = EmojiDefault::Font.presentation(c, next);
        let ix = table.face(&stack, c, FaceStyle::Regular, p);
        checked.insert(ch);
        if let Some(set) = drawn.get_mut(ix.index()) {
            set.insert(ch);
        }
    }
    let gaps: Vec<char> = table
        .take_gaps()
        .into_iter()
        .filter(|c| asked_for(*c))
        .filter_map(|c| char::from_u32(c.value()))
        .collect();
    for set in &mut drawn {
        for g in &gaps {
            set.remove(g);
        }
    }
    Ok(Coverage {
        drawn: faces
            .iter()
            .zip(drawn)
            .map(|((name, _), set)| (name.to_string(), set.into_iter().collect()))
            .collect(),
        gaps,
        checked: checked.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **baud's own faces draw the config, the prompts and pointed Hebrew,
    /// and a CJK character is a gap without a CJK face**: the table lists
    /// 终 as no face's, which is what would send baud to the system. The
    /// control that the check can fire, on faces this repository carries.
    /// Perturbation: drop the table's list of gaps and 终 is listed as
    /// nobody's gap, which the second check catches.
    #[test]
    fn baud_s_own_faces_draw_the_readme_and_a_cjk_character_is_a_gap() {
        let own = [
            ("baud Mono", REGULAR),
            ("Symbols Nerd Font Mono", SYMBOLS),
            ("STIX Two Math", STIX),
        ];
        let text = format!(
            "{}{}{}{}{}",
            crate::scratch::config(&["NotoColorEmoji.ttf"]),
            crate::session::PROMPT_SHOWN,
            crate::session::REMOTE_PROMPT,
            crate::session::FIREWORKS,
            crate::session::HELLO.lines().next().unwrap()
        );
        let c = coverage(&own, &text).unwrap();
        assert_eq!(c.gaps, Vec::<char>::new(), "baud's faces draw it all");
        assert!(
            c.drawn[0].1.contains(&'\u{05BC}'),
            "a dagesh, from baud's text face"
        );
        let cjk = coverage(&own, "a终").unwrap();
        assert_eq!(cjk.gaps, ['终']);
        assert_eq!(cjk.checked, 2);
        assert!(!cjk.drawn.iter().any(|(_, s)| s.contains(&'终')));
    }

    /// **What baud never asks the system for is not checked**: a control,
    /// a zero width joiner, a variation selector and an unassigned code
    /// point.
    #[test]
    fn controls_and_ignorables_are_not_asked_for() {
        for c in ['\n', '\u{200D}', '\u{FE0F}', '\u{0378}'] {
            assert!(
                !asked_for(Codepoint::from_char(c)),
                "U+{:04X}",
                u32::from(c)
            );
        }
        assert!(asked_for(Codepoint::from_char('终')));
        assert_eq!(PINNED[1].file(), "NotoSansCJKsc-Regular.otf");
    }
}
