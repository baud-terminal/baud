//! **The Hebrew stage, from its sources.** baud's embedded faces are
//! stock JetBrains Mono with Hebrew drawn from Cascadia Mono appended, and
//! then Noto Sans Devanagari merged in. This module builds the first part:
//! one style's stock face and its stage source
//! (`baud-app/fonts/glyphs/languages/hebrew/stage/<Style>.txt`, the
//! format [`text`] describes) merged into one font.
//!
//! The source holds what the stage adds to the stock face and nothing
//! else: the appended glyphs with their outlines, advances, code points
//! and GDEF classes; the appended mark glyph sets; the appended GPOS
//! lookups; the features whose lookup lists grew; the hebr script records
//! that name DFLT's Script table; the OS/2 range fields and the name
//! records it sets; and head's modified date. Everything else is the
//! stock face's, and what the stage's own build recomputed when it saved
//! is recomputed the same way ([`build`]).
//!
//! The sources were read out of the stage fonts the earlier parametric
//! build wrote, by [`extract`], which refuses to write a source unless
//! building it again gives a font `compare` holds equal to the one it was
//! read from.

mod build;
mod extract;
pub mod text;

pub use text::Source;

/// How many bytes of `glyph` (one glyph's data from glyf) its record
/// spans, read from its own counts and flags: the rest is padding.
pub use build::unpadded_len;

/// One style's stage: `source` merged into the stock face `base`.
pub fn build(base: &[u8], source: &str) -> Result<Vec<u8>, String> {
    build::build(base, &Source::parse(source)?)
}

/// The text written above every source.
pub const HEADER: &str = "\
The Hebrew stage of one of baud's embedded styles: what it adds to stock
JetBrains Mono, merged into it by `baud-fontbuild stage` (tools/
baud-fontbuild/src/stage/text.rs describes the format). The glyphs are
drawn from Cascadia Mono and the name records carry JetBrains Mono's and
Cascadia Mono's notices: data read out of both fonts, under each font's
SIL Open Font License 1.1 (CascadiaMono-OFL.txt and JetBrainsMono-OFL.txt
beside this file).";

/// The source of `stage` against `base`, for the style named `style`.
/// It is checked before it is returned: the source is built again and the
/// font it gives must compare equal to `stage`.
pub fn extract(base: &[u8], stage: &[u8], style: &str) -> Result<String, String> {
    let src = extract::read(base, stage, style)?;
    let text = src.write(HEADER);
    let again = build(base, &text)?;
    let report = crate::compare(stage, &again).map_err(|e| e.0)?;
    if !report.is_equal() {
        return Err(format!(
            "the source does not rebuild the stage:\n{}",
            report.differences.join("\n")
        ));
    }
    Ok(text)
}
