//! **Which cells of the screen are a picture drawn in text**, so the glass
//! can dim a TUI's panels and still draw a photo solid.
//!
//! A program with no image protocol draws a picture as cells: chafa, which
//! yazi and fzf previews fall back to, writes every cell as a glyph in one
//! colour on a background in another. Under the window's dim the glyph half
//! is drawn solid and the background half at the dim, so a picture splits
//! into solid slivers beside see-through holes -- measured on the owner's
//! yazi preview on 2026-09-22, and no `dim_dark` below 1.0 closes it.
//!
//! **One cell cannot tell a picture from a panel, and this looks at the
//! block instead.** chafa draws a picture's flat areas as spaces on a set
//! background, which is exactly what a TUI panel is, so any per-cell rule
//! either leaves those holes or turns btop solid. What separates the two is
//! how many colours a connected block of set backgrounds holds: a panel is
//! one colour or a handful, a photo is hundreds. A block is the cells whose
//! drawn background the application set, joined to their four neighbours;
//! it is a picture when it holds at least [`PICTURE_COLOURS`] distinct
//! background colours, and every cell of it is then drawn solid.
//!
//! **The threshold is measured, not chosen.** Over the corpus at every read
//! boundary and seven real captures of themed TUIs, the most distinct
//! backgrounds any block held was **8** (vim in diff mode under `habamax`),
//! then helix 5, mc 3, `vimhl` 3, delta 2, btop and asciiquarium 1. chafa
//! pictures of five system photos, at yazi's argv, from 12x6 to 90x45
//! cells: **48 to 2854** in truecolour, which is what chafa sends baud, and
//! 15 to 74 when forced to 240 colours -- where only a 12x6 thumbnail falls
//! under 16. So 16 sits twice the busiest TUI and a third of the smallest
//! truecolour picture.
//!
//! **What it gets wrong, stated.** A picture drawn in fewer than 16 colours
//! stays dim, and one drawn in 16 (chafa `--colors 16`) is found only where
//! it uses all of them in one block. A TUI that sets a background on every
//! cell and draws a picture inside it joins the two into one block, so the
//! whole application is drawn solid -- which is `app_backgrounds =
//! "opaque"`, the other answer, for that one screen. And a panel with sixteen
//! colours of background, a colour chart for one, is drawn solid, which is
//! what a chart's swatches want anyway. On the cell path a smooth scroll's
//! leaving row draws its picture cells dimmed for the ~60 ms of a slide,
//! because the overlay mask has no slot for that row.
//!
//! `cargo bench -p baud-bench --bench pictures` re-derives these numbers,
//! with the photo captures passed in through `PICTURES_EXTRA`.
//!
//! One pass over the screen: each row's runs of set backgrounds, joined to
//! the runs above them that they touch through a union-find, each block's
//! colours counted as it grows and capped at the threshold, so a block
//! stops counting the moment it is known to be a picture.

use baud_core::Term;
use baud_domain::{Col, ColorWord, Cols, PackedCell, Row, Rows, Video, Viewport};

use crate::units::Tally;

/// How many distinct background colours a block of set backgrounds holds
/// before it is a picture. See the module doc for the measurement.
pub const PICTURE_COLOURS: usize = 16;

/// Whether a cell belongs to a picture.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Pictured {
    #[default]
    No,
    Yes,
}

/// A block's distinct background colours, capped at [`PICTURE_COLOURS`].
/// The default colour marks an empty slot: a block holds only cells whose
/// background was set, so it never counts the default.
#[derive(Clone, Copy)]
struct Palette([ColorWord; PICTURE_COLOURS]);

impl Palette {
    const EMPTY: Palette = Palette([ColorWord::DEFAULT; PICTURE_COLOURS]);

    fn insert(&mut self, c: ColorWord) {
        for slot in &mut self.0 {
            if *slot == c {
                return;
            }
            if slot.is_default() {
                *slot = c;
                return;
            }
        }
    }

    fn absorb(&mut self, other: &Palette) {
        for &c in other.0.iter().take_while(|c| !c.is_default()) {
            if self.is_picture() {
                return;
            }
            self.insert(c);
        }
    }

    /// Every slot filled: at least [`PICTURE_COLOURS`] distinct colours.
    fn is_picture(&self) -> bool {
        !self.0[PICTURE_COLOURS - 1].is_default()
    }
}

/// A run's place in the frame's list of runs, which is also its node in
/// the union-find: the parent of a run is a run, never a cell or a count.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RunId(usize);

/// One row's run of set backgrounds, `[from, to)`.
#[derive(Clone, Copy)]
struct Run {
    row: Row,
    from: Col,
    to: Col,
}

/// The screen's picture cells, found once a frame.
pub struct Pictures {
    cols: Cols,
    rows: Rows,
    mask: Vec<Pictured>,
    last: Vec<Pictured>,
    // Scratch kept across frames, so a warm frame allocates nothing.
    runs: Vec<Run>,
    parent: Vec<RunId>,
    palettes: Vec<Palette>,
}

/// The background a cell draws with under `video`, when the application set
/// it -- the foreground under exactly one of `SGR 7` and `?5`, each of which
/// exchanges them. The same test as [`crate::Theme::set_bg`], on the colour
/// word rather than a yes or no.
fn drawn_bg(term: &Term<'_>, cell: PackedCell, video: Video) -> Option<ColorWord> {
    let s = term.style_of(cell);
    let (_, w) = video.drawn(s.attrs(), s.fg(), s.bg());
    (!w.is_default()).then_some(w)
}

impl Default for Pictures {
    fn default() -> Self {
        Self::new()
    }
}

impl Pictures {
    /// No frame found yet; the scratch grows on the first `find`.
    pub fn new() -> Pictures {
        Pictures {
            cols: Cols::of_index(0),
            rows: Rows::of_index(0),
            mask: Vec::new(),
            last: Vec::new(),
            runs: Vec::new(),
            parent: Vec::new(),
            palettes: Vec::new(),
        }
    }

    /// Finds the pictures on `view`, drawn under `video`, and returns how
    /// many cells they cover: a cell's drawn background is its foreground
    /// under `?5`, so the video decides which colours make a block.
    /// The frame before is kept, so [`Pictures::row_changed`] can say which
    /// rows a picture appeared on, moved across or left.
    pub fn find(&mut self, term: &Term<'_>, view: &Viewport<'_>, video: Video) -> Tally {
        self.begin(view.cols(), view.rows());
        let (cols, rows) = (view.cols().index(), view.rows().index());
        let mut above = 0..0;
        for y in 0..rows {
            let row = view.row(Row::from_index(y));
            let first = self.runs.len();
            // Where the scan of the row above resumes: the first of its
            // runs that ends past the current run's start. Runs are sorted
            // on both rows, so it only moves forward.
            let mut resume = above.start;
            let mut x = 0;
            while x < cols {
                let Some(c) = row.get(x).and_then(|&cell| drawn_bg(term, cell, video)) else {
                    x += 1;
                    continue;
                };
                let from = x;
                let mut palette = Palette::EMPTY;
                palette.insert(c);
                let mut prev = c;
                x += 1;
                while x < cols {
                    let Some(c) = row.get(x).and_then(|&cell| drawn_bg(term, cell, video)) else {
                        break;
                    };
                    // A run of one colour is the common case in a panel;
                    // the palette is asked only when the colour moves.
                    if c != prev && !palette.is_picture() {
                        palette.insert(c);
                    }
                    prev = c;
                    x += 1;
                }
                let id = RunId(self.runs.len());
                self.runs.push(Run {
                    row: Row::from_index(y),
                    from: Col::at_index(from),
                    to: Col::at_index(x),
                });
                self.parent.push(id);
                self.palettes.push(palette);
                while resume < above.end && self.runs[resume].to.index() <= from {
                    resume += 1;
                }
                let mut q = resume;
                while q < above.end && self.runs[q].from.index() < x {
                    self.union(RunId(q), id);
                    q += 1;
                }
            }
            above = first..self.runs.len();
        }
        let mut cells = Tally::default();
        for id in 0..self.runs.len() {
            let root = self.root(RunId(id));
            if !self.palettes[root.0].is_picture() {
                continue;
            }
            let run = self.runs[id];
            let at = run.row.index() * cols;
            for slot in &mut self.mask[at + run.from.index()..at + run.to.index()] {
                *slot = Pictured::Yes;
                cells.bump();
            }
        }
        cells
    }

    /// No pictures this frame: the rule is off. Keeps the frame before,
    /// like [`Pictures::find`], so turning the rule off repaints what it
    /// had drawn solid.
    pub fn clear(&mut self, cols: Cols, rows: Rows) {
        self.begin(cols, rows);
    }

    fn begin(&mut self, cols: Cols, rows: Rows) {
        let n = cols.index() * rows.index();
        core::mem::swap(&mut self.mask, &mut self.last);
        if self.cols != cols || self.rows != rows {
            // A frame of a different shape says nothing about this one.
            self.last.clear();
        }
        self.cols = cols;
        self.rows = rows;
        self.mask.clear();
        self.mask.resize(n, Pictured::No);
        self.runs.clear();
        self.parent.clear();
        self.palettes.clear();
    }

    /// Whether the cell at `row`, `col` of the last frame found belongs to
    /// a picture.
    pub fn at(&self, row: Row, col: Col) -> Pictured {
        if col.index() >= self.cols.index() {
            return Pictured::No;
        }
        self.mask
            .get(row.index() * self.cols.index() + col.index())
            .copied()
            .unwrap_or_default()
    }

    /// `row`'s cells as the last frame found them, a picture's or not.
    pub fn row(&self, row: Row) -> &[Pictured] {
        let cols = self.cols.index();
        let at = row.index() * cols;
        self.mask.get(at..at + cols).unwrap_or_default()
    }

    /// Whether `row`'s picture cells differ from the frame before -- a row
    /// whose cells did not change can still need repainting, because a
    /// picture is a property of the block and the block may have changed
    /// on another row.
    pub fn row_changed(&self, row: Row) -> bool {
        let cols = self.cols.index();
        let at = row.index() * cols;
        match (self.mask.get(at..at + cols), self.last.get(at..at + cols)) {
            (Some(now), Some(then)) => now != then,
            (Some(now), None) => now.contains(&Pictured::Yes),
            _ => false,
        }
    }

    /// The run that stands for `i`'s block, halving the path on the way.
    fn root(&mut self, mut i: RunId) -> RunId {
        while self.parent[i.0] != i {
            let up = self.parent[self.parent[i.0].0];
            self.parent[i.0] = up;
            i = up;
        }
        i
    }

    /// Joins two runs' blocks under the earlier root, which takes the other
    /// block's colours.
    fn union(&mut self, a: RunId, b: RunId) {
        let (ra, rb) = (self.root(a), self.root(b));
        if ra == rb {
            return;
        }
        let (keep, gone) = if ra.0 < rb.0 { (ra, rb) } else { (rb, ra) };
        self.parent[gone.0] = keep;
        let absorbed = self.palettes[gone.0];
        self.palettes[keep.0].absorb(&absorbed);
    }
}
