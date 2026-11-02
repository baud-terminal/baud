//! **A row of Devanagari is shaped as Devanagari**, through the renderer's
//! own runs: the path a row takes, where a face whose scripts select the
//! same lookups shapes every run as Common. Kohinoor Devanagari, macOS's
//! own Devanagari face, names the same lookups under `DFLT` and `dev2` for
//! the grid's features, so its runs were shaped as Common and its Hindi
//! drew VOWEL SIGN I after its consonant; the Indic engine reorders a
//! Devanagari run (`baud_font::Engine`), so a run is cut, and shaped, by
//! engine too. Each cell's glyphs must be the ones hb-shape gave the same
//! line in `baud-font/tests/shape/devanagari.txt`, in their order. A macOS
//! system face.

mod support;

use baud_domain::{ClusterBreak, Codepoint, Cols, Rows, WinSize};
use baud_font::{Face, FaceIndex};
use baud_render::{Blend, CellSpan, Code, PxSize, Renderer, Stack, Theme};
use support::*;

const ROW: WinSize = WinSize {
    ws_row: 1,
    ws_col: 48,
};

const KOHINOOR: &str = "/System/Library/Fonts/Kohinoor.ttc";

/// The file's FNV-1a digest, as the fixture generator writes it.
fn fnv64(data: &[u8]) -> String {
    let h = data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{h:016x}")
}

/// The cells the core's cluster machine makes of `cps`.
fn cells(cps: &[Codepoint]) -> Vec<core::ops::Range<usize>> {
    let mut machine = ClusterBreak::new();
    let mut out: Vec<core::ops::Range<usize>> = Vec::new();
    for (i, &c) in cps.iter().enumerate() {
        let brk = machine.is_break(c);
        match out.last_mut() {
            Some(last) if !brk => last.end = i + 1,
            _ => out.push(i..i + 1),
        }
    }
    out
}

/// hb-shape's glyph ids for `cps` on Kohinoor Devanagari, by the cell each
/// falls in, from the fixture file; and the file's digest.
fn expected(cps: &[Codepoint]) -> (Vec<Vec<u16>>, String) {
    let text = std::fs::read_to_string(format!(
        "{}/../baud-font/tests/shape/devanagari.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let want: String = cps
        .iter()
        .map(|c| format!("{:04X}", c.value()))
        .collect::<Vec<_>>()
        .join(" ");
    let line = text
        .lines()
        .find(|l| l.starts_with(KOHINOOR) && l.split('\t').nth(4) == Some(want.as_str()))
        .unwrap_or_else(|| panic!("no fixture line for {want}"));
    let f: Vec<&str> = line.split('\t').collect();
    let starts: Vec<usize> = cells(cps).iter().map(|r| r.start).collect();
    let mut by_cell = vec![Vec::new(); starts.len()];
    for g in f[5].trim_matches(['[', ']']).split('|') {
        let (id, rest) = g.split_once('=').unwrap();
        let cluster: usize = rest.split(['+', '@']).next().unwrap().parse().unwrap();
        let cell = starts.iter().rposition(|&s| s <= cluster).unwrap();
        by_cell[cell].push(id.parse().unwrap());
    }
    (by_cell, f[2].to_string())
}

#[test]
fn a_devanagari_row_is_shaped_in_its_script() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: Kohinoor Devanagari is a macOS system face");
        return;
    }
    let data = std::fs::read(KOHINOOR).unwrap();
    let faces = [Face::parse_index(&data, FaceIndex::of(0u32)).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(PX).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ROW.ws_col)),
        Rows::of_index(usize::from(ROW.ws_row)),
    )
    .unwrap();
    let mut reordered = 0usize;
    for line in ["टर्मिनल हर अक्षर को ग्रिड में रखता है।", "कि", "क्ति", "र्कि"]
    {
        let cps: Vec<Codepoint> = line.chars().map(Codepoint::from_char).collect();
        let (want, digest) = expected(&cps);
        assert_eq!(fnv64(&data), digest, "{KOHINOOR} is not the fixtures' file");
        let row: Vec<(Vec<Codepoint>, CellSpan)> = cells(&cps)
            .into_iter()
            .map(|c| (cps[c].to_vec(), CellSpan::One))
            .collect();
        let shaped = r.window(Code(0), &row);
        let got: Vec<Vec<u16>> = shaped
            .iter()
            .map(|cell| {
                cell.iter()
                    .map(|g| u16::try_from(g.id.index()).unwrap())
                    .collect()
            })
            .collect();
        // A space draws its glyph; hb-shape's space glyph is in the same
        // cell, so the cells compare whole.
        assert_eq!(got, want, "{line}: each cell's glyphs, against hb-shape");
        // The vowel sign I drawn first in its cell, before the consonant it
        // follows in the text, and left of it.
        for (cell, c) in shaped.iter().zip(cells(&cps)) {
            if cps[c.clone()].last() == Some(&Codepoint::from_char('\u{093F}')) {
                assert!(
                    cell.len() >= 2 && cell[0].dx < cell[cell.len() - 1].dx,
                    "{line}"
                );
                reordered += 1;
            }
        }
    }
    assert!(reordered >= 4, "{reordered} cells with VOWEL SIGN I");
}
