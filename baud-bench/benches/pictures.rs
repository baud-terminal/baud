//! **Where `[glass] pictures`'s threshold comes from, kept runnable.**
//!
//! `baud_render::PICTURE_COLOURS` says a connected block of set
//! backgrounds holding sixteen colours or more is a picture drawn in text.
//! This prints what that rests on: for every stream, the most distinct
//! background colours any one block held at any read boundary, and for
//! every capture of a text-drawn picture, how many its block holds. The
//! threshold is sound while every stream's number sits well under it and
//! every capture's well over.
//!
//! Always read: the nine recordings, `tests/panels/`'s three themed-editor
//! captures, and `tests/previews/`'s two chafa captures. The captures the
//! threshold was also measured on -- chafa over five macOS system photos,
//! at yazi's argv, from 12x6 to 90x45 cells -- are Apple's pictures and are
//! not committed, so they come in through `PICTURES_EXTRA`: a
//! comma-separated list of directories, each `.raw` in them replayed as a
//! stream and each `.ans` placed as a capture.
//!
//! It prints and asserts nothing, like every bench here; the gate is
//! `baud-render/tests/pictures.rs`.
//!
//! Usage: `cargo bench -p baud-bench --bench pictures`

use std::collections::{HashSet, VecDeque};

use baud_core::{Owned, Term};
use baud_domain::{Attrs, BytePos, ColorWord, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_render::{PICTURE_COLOURS, Pictures, Video};

/// The corpus geometry, which every stream here was captured at.
const WS: WinSize = WinSize {
    ws_row: 50,
    ws_col: 200,
};

const CHUNK: usize = 4096;

/// A colour word as a set key: truecolour and indexed apart.
fn key(w: ColorWord) -> Option<u32> {
    if let Some((r, g, b)) = w.as_rgb() {
        return Some(1 << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b));
    }
    w.as_index().map(|i| 2 << 24 | u32::from(i))
}

/// The background a cell draws with when the application set it -- the
/// foreground under `SGR 7` -- as `baud_render`'s finder reads it.
fn drawn(t: &Term<'_>, c: PackedCell) -> Option<u32> {
    let s = t.style_of(c);
    let w = if s.attrs().has(Attrs::REVERSE) {
        s.fg()
    } else {
        s.bg()
    };
    if w.is_default() { None } else { key(w) }
}

/// Every block of set backgrounds on the screen, joined through the four
/// neighbours, as (cells, distinct colours). A breadth-first walk rather
/// than the finder's union-find, so the two are separate spellings.
fn blocks(t: &Term<'_>) -> Vec<(usize, usize)> {
    let (rows, cols) = (usize::from(WS.ws_row), usize::from(WS.ws_col));
    let view = t.view(Rows::ZERO);
    let mut grid = vec![None; rows * cols];
    for y in 0..rows {
        for (x, &c) in view.row(Row::from_index(y)).iter().take(cols).enumerate() {
            grid[y * cols + x] = drawn(t, c);
        }
    }
    let mut seen = vec![false; rows * cols];
    let mut out = Vec::new();
    for start in 0..rows * cols {
        if seen[start] || grid[start].is_none() {
            continue;
        }
        seen[start] = true;
        let mut queue = VecDeque::from([start]);
        let mut colours = HashSet::new();
        let mut cells = 0;
        while let Some(i) = queue.pop_front() {
            cells += 1;
            colours.extend(grid[i]);
            let (y, x) = (i / cols, i % cols);
            let mut next = |j: usize| {
                if !seen[j] && grid[j].is_some() {
                    seen[j] = true;
                    queue.push_back(j);
                }
            };
            if x > 0 {
                next(i - 1);
            }
            if x + 1 < cols {
                next(i + 1);
            }
            if y > 0 {
                next(i - cols);
            }
            if y + 1 < rows {
                next(i + cols);
            }
        }
        out.push((cells, colours.len()));
    }
    out
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// A stream, a read at a time: the busiest block at any read, and the most
/// cells the finder marked at any read.
fn stream(name: &str, bytes: &[u8]) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut finder = Pictures::new();
    let (mut busiest, mut its_cells, mut marked, mut reads) = (0, 0, 0u64, 0);
    for chunk in bytes.chunks(CHUNK) {
        feed(&mut t, chunk);
        reads += 1;
        for (cells, colours) in blocks(&t) {
            if colours > busiest {
                (busiest, its_cells) = (colours, cells);
            }
        }
        let view = t.view(Rows::ZERO);
        marked = marked.max(finder.find(&t, &view, Video::Normal).value());
    }
    println!(
        "stream   {name:28} reads {reads:4}  busiest block {busiest:5} colours in {its_cells:5} cells  finder marked {marked}"
    );
}

/// A capture placed the way yazi places one: each line at the pane's
/// origin, a row further down each time.
fn capture(name: &str, text: &str) {
    let mut bytes = Vec::new();
    for (i, line) in text.split('\n').filter(|l| !l.is_empty()).enumerate() {
        bytes.extend(format!("\x1b[{};21H", i + 4).into_bytes());
        bytes.extend(line.as_bytes());
    }
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &bytes);
    let mut all = blocks(&t);
    all.sort_by_key(|&(cells, _)| std::cmp::Reverse(cells));
    let view = t.view(Rows::ZERO);
    let marked = Pictures::new().find(&t, &view, Video::Normal).value();
    let fewest = all.iter().map(|&(_, colours)| colours).min().unwrap_or(0);
    println!(
        "capture  {name:44} blocks {:2}  largest {:?}  fewest colours in a block {fewest:4}  finder marked {marked}",
        all.len(),
        all.first()
    );
}

fn main() {
    baud_bench::warm::refuse_while_warm();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests");
    println!("threshold: {PICTURE_COLOURS} colours; geometry 200x50, reads of {CHUNK}");
    for r in [
        "ascii/pipes.raw",
        "ascii/cat-scroll.raw",
        "ascii/git-log.raw",
        "ascii/less-pager.raw",
        "utf8/btop.raw",
        "utf8/cat-cjk.raw",
        "utf8/cava.raw",
        "utf8/less-cjk.raw",
        "utf8/vim-unicode.raw",
    ] {
        stream(
            r,
            &std::fs::read(format!("{root}/recordings/{r}")).expect("recording"),
        );
    }
    let mut dirs = vec![format!("{root}/panels"), format!("{root}/previews")];
    if let Ok(extra) = std::env::var("PICTURES_EXTRA") {
        dirs.extend(extra.split(',').map(str::to_string));
    }
    for dir in dirs {
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .expect("a directory")
            .filter_map(|e| Some(e.ok()?.path()))
            .collect();
        paths.sort();
        for p in paths {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
            let Some(name) = name else { continue };
            match p.extension().and_then(|e| e.to_str()) {
                Some("raw") => stream(&name, &std::fs::read(&p).expect("stream")),
                Some("ans") => capture(&name, &std::fs::read_to_string(&p).expect("capture")),
                _ => {}
            }
        }
    }
}
