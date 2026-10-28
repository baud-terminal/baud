//! **The system's face for a character no face of the chain draws**: the
//! third tier of the order -- the configured font, then baud's embedded
//! faces, then the system's font -- so an Arabic or Syriac word draws from
//! a face macOS or Linux ships rather than as boxes. The Unicode Standard
//! allows the box only where no font has the character: one "unable to
//! render it, because it lacks a font for the code point" can show it "only
//! ... by some placeholder" (`[[source: unicode18-ch5]]` 5.3).
//!
//! **Asked when a frame first draws such a character** (when a frame meets
//! it rather than at launch, and for the characters drawn rather than every
//! one of their block): the renderer lists a character no face draws the
//! first time a cell is drawn with it (`Renderer::take_gaps`); [`extend`]
//! asks the system for those it can show, appends the faces it answers
//! behind the chain, and the driver restacks once. A session that never
//! draws such a character asks nothing, and a frame of text every face
//! covers does no new work. Each character is asked once a process and each
//! answer kept, so a config read again keeps the faces ([`keep`]) and a
//! character met again asks nothing.
//!
//! **On macOS the answer is CoreText's**: `CTFontCreateForString` from
//! CoreText's font made of the primary's own bytes, as kitty asks it per
//! cell (`[[source: kitty-6d5d0c4-core-text]]` `find_substitute_face`)
//! and ghostty per codepoint (`[[source: ghostty-discovery-8f0dd37]]`).
//! Its cascade list alone (`[[source: crossfont-darwin-0-8-1]]`,
//! alacritty's way) maps 33 of the 161 scripts' first letters where the
//! per-string answer maps 133: Syriac, Thaana and N'Ko are not in it.
//! From JetBrains Mono's bytes CoreText answers Courier New for Arabic
//! and Urdu, the monospaced face it gives Menlo too, and Noto Sans
//! Syriac for Syriac. The face it answers for the noncharacter U+FDD0,
//! LastResort, is its placeholder and is refused, as kitty and ghostty
//! refuse it, so such a character keeps the primary's `.notdef` box and
//! a later one is still asked.
//!
//! **Elsewhere the answer is fontconfig's charset match over baud's own
//! scan**: [`crate::discover::Catalogue::covering`].
//!
//! **Behind every stack**, `font_fallback = false` included: that switch is
//! documented for baud's built-in fonts, and the system tier is built for
//! every stack.

use crate::config::{Config, FaceFrom, FaceSpec};
use baud_domain::Codepoint;
use baud_font::Script;
use baud_render::FaceRole;

/// **Whether the system is asked for `c`**: an assigned character (its
/// script is not Unknown, which Scripts.txt gives the unassigned, private
/// use and surrogate code points) that is not a control (`Cc`), a line or
/// paragraph separator (`Zl`, `Zp`) or default-ignorable -- the characters
/// TUS 5.3 says show as a placeholder when no font has them, and none it
/// says to show as nothing. Unlike the graphic characters of TUS D50,
/// this admits the 32 visible format characters D50 leaves out
/// (U+0600-0605, U+06DD, U+070F, the Egyptian format controls and
/// others), which TUS 5.3 shows by a placeholder too. The three sets are
/// held to `DerivedGeneralCategory.txt` by a test.
pub fn asked_for(c: Codepoint) -> bool {
    Script::of(c) != Script::UNKNOWN
        && !matches!(c.value(), 0..=0x1f | 0x7f..=0x9f | 0x2028 | 0x2029)
        && !baud_domain::is_default_ignorable(c)
}

/// What the system has been asked this process, and the faces it answered,
/// in the order they were first needed.
#[derive(Default)]
struct Asked {
    asked: std::collections::BTreeSet<Codepoint>,
    faces: Vec<FaceSpec>,
}

fn asked() -> std::sync::MutexGuard<'static, Asked> {
    static ASKED: std::sync::OnceLock<std::sync::Mutex<Asked>> = std::sync::OnceLock::new();
    ASKED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// **The characters of `gaps` asked of the system, and every face it has
/// answered behind `cfg`'s chain** ([`keep`]): whether `cfg` changed, so
/// the driver restacks only when a face was added.
pub fn extend(cfg: &mut Config, gaps: &[Codepoint]) -> bool {
    let wanted: Vec<Codepoint> = {
        let mut a = asked();
        gaps.iter()
            .copied()
            .filter(|&c| asked_for(c) && a.asked.insert(c))
            .collect()
    };
    if !wanted.is_empty() {
        let found = answer(cfg, &wanted);
        let mut a = asked();
        for f in found {
            if !a.faces.iter().any(|g| same_face(g, &f)) {
                a.faces.push(f);
            }
        }
    }
    keep(cfg)
}

/// **Every face the system has answered this process, behind `cfg`'s
/// chain**: each not already in it appended after its last fallback face,
/// ahead of the styled faces, so it is reached only for what every face
/// before it lacks. Whether `cfg` changed.
pub fn keep(cfg: &mut Config) -> bool {
    let faces = asked().faces.clone();
    let mut changed = false;
    for f in faces {
        if cfg.faces.len() >= usize::from(u8::MAX) {
            break;
        }
        if cfg.faces.iter().any(|g| same_face(g, &f)) {
            continue;
        }
        let at = cfg
            .faces
            .iter()
            .rposition(|g| g.role == FaceRole::Fallback)
            .map_or(0, |i| i + 1);
        cfg.faces.insert(at, f);
        changed = true;
    }
    changed
}

/// Whether two specs name one face of one file.
fn same_face(a: &FaceSpec, b: &FaceSpec) -> bool {
    a.path == b.path && a.index == b.index && a.from == b.from
}

/// The faces the system answers for `wanted`, as fallback faces.
fn answer(cfg: &Config, wanted: &[Codepoint]) -> Vec<FaceSpec> {
    found(cfg, wanted)
        .into_iter()
        .map(|p| FaceSpec::picked(&p, FaceRole::Fallback))
        .collect()
}

/// fontconfig's charset match over the scan, led by the primary's family.
#[cfg(not(target_os = "macos"))]
fn found(cfg: &Config, wanted: &[Codepoint]) -> Vec<crate::discover::Picked> {
    let family = primary(cfg).and_then(|(bytes, index)| {
        let face = baud_font::Face::parse_index(&bytes, index).ok()?;
        let mut out = [0u8; 256];
        let name = face.family()?.utf8(&mut out)?;
        Some(String::from_utf8_lossy(name).into_owned())
    });
    crate::discover::Catalogue::system().covering(family.as_deref(), wanted)
}

/// CoreText's answers from the primary's bytes.
#[cfg(target_os = "macos")]
fn found(cfg: &Config, wanted: &[Codepoint]) -> Vec<crate::discover::Picked> {
    let Some(f) = cfg.faces.first() else {
        return Vec::new();
    };
    coretext::answers_from(&f.path, f.index, || primary(cfg).map(|p| p.0), wanted)
        .into_iter()
        .filter_map(|(path, ps)| crate::discover::named_in(&path, &ps))
        .collect()
}

/// The chain's first face's bytes and index: what the system is asked from.
fn primary(cfg: &Config) -> Option<(std::borrow::Cow<'static, [u8]>, baud_font::FaceIndex)> {
    let f = cfg.faces.first()?;
    let bytes = match f.from {
        FaceFrom::Embedded(b) => std::borrow::Cow::Borrowed(b),
        FaceFrom::File => std::borrow::Cow::Owned(std::fs::read(&f.path).ok()?),
        FaceFrom::Family => return None,
    };
    Some((bytes, f.index))
}

/// CoreText, through the declarations in `sys.rs`.
#[cfg(target_os = "macos")]
mod coretext {
    use crate::sys::{
        CF_STRING_ENCODING_UTF8, CFDataCreate, CFRelease, CFStringCreateWithCharacters,
        CFStringGetCString, CFURLGetFileSystemRepresentation, CT_FONT_URL_ATTRIBUTE,
        CTFontCopyAttribute, CTFontCopyPostScriptName, CTFontCreateForString,
        CTFontCreateWithFontDescriptor, CTFontGetGlyphsForCharacters,
        CTFontManagerCreateFontDescriptorFromData, CfIndex, CfObject, CfRange, CgGlyph, UniChar,
    };
    use baud_domain::Codepoint;

    /// An object this module owns, released once when dropped.
    struct Owned(CfObject);

    impl Owned {
        fn new(o: CfObject) -> Option<Owned> {
            (!o.is_null()).then_some(Owned(o))
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: a non-null object handed out under the Create or
            // Copy rule, owned here alone and released once.
            unsafe { CFRelease(self.0) };
        }
    }

    /// `c` as UTF-16.
    fn units(c: Codepoint) -> Vec<UniChar> {
        char::from_u32(c.value()).map_or_else(Vec::new, |ch| {
            let mut buf = [0; 2];
            ch.encode_utf16(&mut buf).to_vec()
        })
    }

    /// The units' length as CoreFoundation counts it.
    fn count(u: &[UniChar]) -> CfIndex {
        CfIndex::try_from(u.len()).unwrap_or(0)
    }

    /// Whether `font` maps `c`, as CoreText reads its `cmap`.
    fn maps(font: &Owned, c: Codepoint) -> bool {
        let u = units(c);
        let mut glyphs: [CgGlyph; 2] = [0; 2];
        // SAFETY: `font` is a live font; `u` holds `count(&u)` units, at
        // most two, and `glyphs` has room for as many.
        let ok = unsafe {
            CTFontGetGlyphsForCharacters(font.0, u.as_ptr(), glyphs.as_mut_ptr(), count(&u))
        };
        ok && glyphs[0] != 0
    }

    /// The font CoreText substitutes for `c` from `base`.
    fn for_char(base: &Owned, c: Codepoint) -> Option<Owned> {
        let u = units(c);
        // SAFETY: `u` is valid for `count(&u)` units; the string copies them.
        let text = Owned::new(unsafe {
            CFStringCreateWithCharacters(core::ptr::null(), u.as_ptr(), count(&u))
        })?;
        let range = CfRange {
            location: 0,
            length: count(&u),
        };
        // SAFETY: `base` and `text` are live and `range` lies within `text`.
        Owned::new(unsafe { CTFontCreateForString(base.0, text.0, range) })
    }

    /// The path of the file `font` is in.
    fn path(font: &Owned) -> Option<String> {
        // SAFETY: `font` is live; the attribute key is a framework constant.
        let url = Owned::new(unsafe { CTFontCopyAttribute(font.0, CT_FONT_URL_ATTRIBUTE) })?;
        let mut buf = vec![0u8; 4096];
        let max = CfIndex::try_from(buf.len()).ok()?;
        // SAFETY: `url` is a live URL and `buf` is writable for `max` bytes.
        let ok = unsafe { CFURLGetFileSystemRepresentation(url.0, 1, buf.as_mut_ptr(), max) };
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0)?;
        String::from_utf8(buf[..end].to_vec()).ok()
    }

    /// `font`'s PostScript name.
    fn postscript(font: &Owned) -> Option<String> {
        // SAFETY: `font` is live.
        let name = Owned::new(unsafe { CTFontCopyPostScriptName(font.0) })?;
        let mut buf = [0u8; 256];
        let size = CfIndex::try_from(buf.len()).ok()?;
        // SAFETY: `name` is a live string and `buf` is writable for `size`.
        let ok = unsafe {
            CFStringGetCString(
                name.0.string(),
                buf.as_mut_ptr().cast(),
                size,
                CF_STRING_ENCODING_UTF8,
            )
        };
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0)?;
        String::from_utf8(buf[..end].to_vec()).ok()
    }

    /// CoreText's font made of `bytes`, at a size that does not matter: a
    /// substitution asks only which face.
    fn base(bytes: &[u8]) -> Option<Owned> {
        let len = CfIndex::try_from(bytes.len()).ok()?;
        // SAFETY: `bytes` is valid for `len` bytes; the data copies them.
        let data = Owned::new(unsafe { CFDataCreate(core::ptr::null(), bytes.as_ptr(), len) })?;
        // SAFETY: `data` is a live data.
        let desc = Owned::new(unsafe { CTFontManagerCreateFontDescriptorFromData(data.0) })?;
        // SAFETY: `desc` is a live descriptor; a null matrix is the identity.
        Owned::new(unsafe { CTFontCreateWithFontDescriptor(desc.0, 12.0, core::ptr::null()) })
    }

    thread_local! {
        /// CoreText's font for the last primary asked from, by its path
        /// and face index, and its placeholder face's file: made once per
        /// primary, as making it is most of an ask's cost.
        static BASE: core::cell::RefCell<Option<Base>> = const { core::cell::RefCell::new(None) };
    }

    /// CoreText's font for one primary, and what it answers for U+FDD0.
    struct Base {
        path: String,
        index: baud_font::FaceIndex,
        font: Owned,
        placeholder: Option<String>,
    }

    /// [`answers`] from the primary at `path`, face `index`, whose bytes
    /// `bytes` reads: CoreText's font for it made only when the primary
    /// differs from the last one asked from.
    pub fn answers_from(
        path: &str,
        index: baud_font::FaceIndex,
        bytes: impl FnOnce() -> Option<std::borrow::Cow<'static, [u8]>>,
        wanted: &[Codepoint],
    ) -> Vec<(String, String)> {
        BASE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let fresh = slot
                .as_ref()
                .is_none_or(|b| b.path != path || b.index != index);
            if fresh {
                *slot = bytes().and_then(|b| made(path, index, &b));
            }
            slot.as_ref().map_or_else(Vec::new, |b| asked(b, wanted))
        })
    }

    /// CoreText's font made of `bytes`, and its placeholder face's file.
    fn made(path: &str, index: baud_font::FaceIndex, bytes: &[u8]) -> Option<Base> {
        let font = base(bytes)?;
        let placeholder = Codepoint::new(0xFDD0)
            .and_then(|c| for_char(&font, c))
            .and_then(|f| self::path(&f));
        Some(Base {
            path: path.to_string(),
            index,
            font,
            placeholder,
        })
    }

    /// **Each face CoreText answers for `wanted` from the font made of
    /// `bytes`**, as its file's path and its PostScript name, each once and
    /// in the order of the first character it answers: a character an
    /// answer already maps is not asked again, and an answer that does not
    /// map its character, has no file, or is the face CoreText answers for
    /// the noncharacter U+FDD0 -- its placeholder face -- is refused.
    #[cfg(test)]
    pub fn answers(bytes: &[u8], wanted: &[Codepoint]) -> Vec<(String, String)> {
        made("", baud_font::FaceIndex::FIRST, bytes).map_or_else(Vec::new, |b| asked(&b, wanted))
    }

    /// [`answers`] from a font already made.
    fn asked(b: &Base, wanted: &[Codepoint]) -> Vec<(String, String)> {
        let (base, placeholder) = (&b.font, &b.placeholder);
        let mut fonts: Vec<(Owned, String, String)> = Vec::new();
        for &c in wanted {
            if fonts.iter().any(|(f, _, _)| maps(f, c)) {
                continue;
            }
            let Some(font) = for_char(base, c) else {
                continue;
            };
            let (Some(p), Some(ps)) = (path(&font), postscript(&font)) else {
                continue;
            };
            if Some(&p) == placeholder.as_ref() || !maps(&font, c) {
                continue;
            }
            if !fonts.iter().any(|(_, q, qs)| *q == p && *qs == ps) {
                fonts.push((font, p, ps));
            }
        }
        fonts.into_iter().map(|(_, p, ps)| (p, ps)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The characters asked for are the assigned ones that are not a
    /// control, a separator or default-ignorable**, held to the pinned
    /// data at every code point: `asked_for` is false exactly where
    /// `DerivedGeneralCategory.txt` gives `Cn`, `Co`, `Cs`, `Cc`, `Zl` or
    /// `Zp`, or `DerivedCoreProperties.txt` gives
    /// `Default_Ignorable_Code_Point`. Of the first three, Scripts.txt's
    /// Unknown is the test the code makes; the others are named.
    #[test]
    fn asked_for_is_the_pinned_data() {
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources");
        let mut not = vec![false; 0x11_0000];
        let mut assigned = vec![false; 0x11_0000];
        let ranges = |line: &str| -> Option<(usize, usize, String)> {
            let line = line.split('#').next()?.trim();
            let (r, v) = line.split_once(';')?;
            let (a, b) = r.trim().split_once("..").unwrap_or((r.trim(), r.trim()));
            Some((
                usize::from_str_radix(a, 16).ok()?,
                usize::from_str_radix(b, 16).ok()?,
                v.trim().to_string(),
            ))
        };
        let gc =
            std::fs::read_to_string(format!("{src}/unicode-derivedgeneralcategory.txt")).unwrap();
        for (a, b, v) in gc.lines().filter_map(ranges) {
            let drop = ["Cn", "Co", "Cs", "Cc", "Zl", "Zp"].contains(&v.as_str());
            for i in a..=b {
                not[i] |= drop;
                assigned[i] = true;
            }
        }
        let dcp =
            std::fs::read_to_string(format!("{src}/unicode-derivedcoreproperties.txt")).unwrap();
        for (a, b, v) in dcp.lines().filter_map(ranges) {
            if v == "Default_Ignorable_Code_Point" {
                for n in not.iter_mut().take(b + 1).skip(a) {
                    *n = true;
                }
            }
        }
        assert!(
            assigned.iter().all(|&a| a),
            "the file lists every code point"
        );
        let mut wrong = Vec::new();
        for (i, &n) in not.iter().enumerate() {
            let Some(c) = u32::try_from(i).ok().and_then(Codepoint::new) else {
                continue;
            };
            if asked_for(c) == n {
                wrong.push(i);
            }
        }
        assert!(
            wrong.is_empty(),
            "{} code points, first U+{:04X}",
            wrong.len(),
            wrong[0]
        );
        // The positive control: the letters this is for are asked.
        for v in [0x0628, 0x0712, 0x0600, 0x4E00] {
            assert!(asked_for(Codepoint::new(v).unwrap()), "U+{v:04X}");
        }
    }

    /// **CoreText's placeholder face is refused**: asked for the
    /// noncharacter U+FDD0, which no font can have, CoreText answers its
    /// placeholder face, and `answers` returns nothing; asked for beh and
    /// alaph it returns a face for each, by file and PostScript name.
    #[cfg(target_os = "macos")]
    #[test]
    fn coretexts_placeholder_is_refused() {
        let bytes = crate::embedded::REGULAR.bytes;
        let nonchar = Codepoint::new(0xFDD0).unwrap();
        assert!(coretext::answers(bytes, &[nonchar]).is_empty());
        let rtl: Vec<Codepoint> = [0x0628, 0x0710]
            .into_iter()
            .filter_map(Codepoint::new)
            .collect();
        let found = coretext::answers(bytes, &rtl);
        assert_eq!(found.len(), 2, "{found:?}");
        for (path, ps) in &found {
            assert!(
                crate::discover::named_in(path, ps).is_some(),
                "{path} holds {ps}"
            );
        }
    }
}
