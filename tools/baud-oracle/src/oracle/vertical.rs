//! HarfBuzz's and FreeType's drawings of the vertical cases, as
//! baud-font's oracle for components placed on vertical phantom points:
//! every glyph of `tests/var/vertical-cases.ttf` at the default instance
//! and at wght 100, 650 and 900, drawn by HarfBuzz and by FreeType as the
//! component oracle draws them, one row per engine. A glyph FreeType
//! refuses to load is written `ERROR` with FreeType's message. The two
//! engines disagree on some glyphs on purpose; baud-font's
//! `tests/vertical.rs` says which the specification sides with.
//!
//! Then the same font with its `vhea` and `vmtx` tags renamed in the table
//! directory: a face with no vertical metrics, which HarfBuzz reads as a
//! tsb of 0 and an advance height of unitsPerEm, drawn at the default
//! instance, engine `hb-novmtx`. (Renaming `vhea` alone leaves a `vmtx`
//! with no long records: HarfBuzz reads every tsb as 0 and, since this
//! table holds more two-byte entries than the face has glyphs, takes the
//! surplus's count as its number of advance heights and reads that many
//! u16s from the table's start, glyph g reading entry min(g, count - 1).)

use std::path::Path;

use crate::oracle::component::SETTINGS;
use crate::{BINDING, FREETYPE, HARFBUZZ, Oracle, contours_hb, fmt, ft, ft_row, hb, name};

/// `vertical.txt`.
pub const ORACLE: Oracle = Oracle {
    name: "vertical",
    file: "crates/baud-font/tests/var/vertical.txt",
    lines,
};

/// The font with `vhea` and `vmtx` renamed `xhea` and `xmtx` in its table
/// directory, each found exactly once.
pub fn without_vertical_metrics(font: &[u8]) -> Vec<u8> {
    let mut data = font.to_vec();
    let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
    for (tag, to) in [(b"vhea", b"xhea"), (b"vmtx", b"xmtx")] {
        let at: Vec<usize> = (0..n)
            .map(|i| 12 + 16 * i)
            .filter(|&o| &data[o..o + 4] == tag)
            .collect();
        assert_eq!(at.len(), 1, "one {} record", String::from_utf8_lossy(tag));
        data[at[0]..at[0] + 4].copy_from_slice(to);
    }
    data
}

fn lines(root: &Path) -> Vec<String> {
    let lib = ft::Library::new();
    crate::pinned(Some(&lib));
    let path = root.join("crates/baud-font/tests/var/vertical-cases.ttf");
    let blob = hb::Blob::from_file(&path);
    let hface = hb::Face::new(&blob);
    let names: Vec<String> = {
        let font = hb::Font::new(&hface);
        (0..hface.glyph_count()).map(|g| name(&font, g)).collect()
    };
    let mut out = vec![
        format!(
            "# glyph  name  wght (or - for the default)  engine  contours  (HarfBuzz {HARFBUZZ}, {BINDING}, FreeType {FREETYPE})"
        ),
        crate::generated_by("vertical"),
    ];
    for w in SETTINGS {
        let mut font = hb::Font::new(&hface);
        font.set_scale(hface.upem(), hface.upem());
        let mut face = lib.face(&path);
        if let Some(w) = w {
            font.set_variations(&[("wght", f64::from(w))]);
            face.set_var_design(&[f64::from(w)])
                .unwrap_or_else(|e| panic!("{}", ft::exception(e)));
        }
        let l = w.map_or("-".to_string(), |w| w.to_string());
        for gid in 1..hface.glyph_count() {
            let n = &names[gid as usize];
            out.push(format!(
                "{gid} {n} {l} hb {}",
                fmt(&contours_hb(&font, gid))
            ));
            out.push(format!("{gid} {n} {l} ft {}", ft_row(&mut face, gid)));
        }
    }
    let data = without_vertical_metrics(&std::fs::read(&path).expect("the cases font"));
    let nblob = hb::Blob::copy_of(&data);
    let nface = hb::Face::new(&nblob);
    let mut nfont = hb::Font::new(&nface);
    nfont.set_scale(nface.upem(), nface.upem());
    for gid in 1..nface.glyph_count() {
        let n = &names[gid as usize];
        out.push(format!(
            "{gid} {n} - hb-novmtx {}",
            fmt(&contours_hb(&nfont, gid))
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The positive control of the rename**: a directory with no `vmtx`
    /// record, or two, is refused rather than written as if renamed.
    #[test]
    fn the_rename_needs_exactly_one_of_each() {
        let mut dir = vec![0u8; 12 + 16 * 2];
        dir[5] = 2;
        dir[12..16].copy_from_slice(b"vhea");
        dir[28..32].copy_from_slice(b"vmtx");
        let renamed = without_vertical_metrics(&dir);
        assert_eq!(&renamed[12..16], b"xhea");
        assert_eq!(&renamed[28..32], b"xmtx");
        dir[28..32].copy_from_slice(b"vhea");
        assert!(std::panic::catch_unwind(|| without_vertical_metrics(&dir)).is_err());
    }
}
