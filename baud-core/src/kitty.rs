//! The kitty graphics protocol's wiring (`[[source: kitty-graphics]]`; note
//! V, V19 steps 5-6): an `APC G` body routed to `baud_domain::KittyStream`,
//! which holds the protocol, and what it asks of the terminal done here --
//! the cursor moved past a placement, the replies queued, and the deferred
//! decode stepped between reads.
//!
//! **Routed by a prefix, not a graph action**, as `clip.rs`'s `52;` is: an
//! `APC` is told apart from `SOS` and `PM` by its introducer
//! (`StrKind::of`), and a kitty command from any other `APC` by its first
//! body byte (`Paint::str_put`). The graph's tables are what they were, so
//! the text path's are too.

use crate::osc::StrKind;
use crate::paint::Paint;
use crate::term::Term;
use baud_domain::*;

impl Paint<'_, '_> {
    /// The `G` of an `APC G`: a kitty graphics chunk begins.
    #[cold]
    #[inline(never)]
    pub(crate) fn kitty_start(&mut self) {
        // No image on the status line, as `sixel_begin` has it.
        if self.on_status() {
            self.cold.str_kind = StrKind::Other;
            return;
        }
        self.cold.str_kind = StrKind::Kitty;
        // An iTerm2 file still arriving gives the scratch up (`crate::file`).
        self.file_yield();
        let s = &mut *self.strs;
        s.kitty.begin(self.images, s.kitty_scratch);
        self.rehook();
    }

    /// One body byte of a kitty chunk.
    #[cold]
    #[inline(never)]
    pub(crate) fn kitty_put(&mut self, b: StreamByte) {
        let s = &mut *self.strs;
        if s.kitty.put(b.raw_byte(), self.images, s.kitty_scratch) == KittyByte::Separator {
            self.kitty_replies();
        }
        self.rehook();
    }

    /// The `n` body bytes at `from`, which the graph puts, at once
    /// (`Paint::str_run`).
    pub(crate) fn kitty_run(&mut self, from: BytePos, n: ByteLen) {
        let run = &self.arena[from.index()..from.index() + n.index()];
        let s = &mut *self.strs;
        if s.kitty.put_run(run, self.images, s.kitty_scratch) == KittyByte::Separator {
            self.kitty_replies();
        }
        self.rehook();
    }

    /// The chunk's terminator: the command runs, and the cursor moves past
    /// what it placed.
    #[cold]
    #[inline(never)]
    pub(crate) fn kitty_end(&mut self) {
        let at = self.kitty_at();
        if self.strs.kitty.wants_drawn(self.images) {
            self.kitty_mark_drawn(&at);
        }
        let s = &mut *self.strs;
        let moved = s.kitty.end(&at, self.images, s.kitty_scratch);
        // A placement text later in this read may land on (`Paint::hooks`),
        // as after each call above that hands the stream the store.
        self.rehook();
        self.kitty_replies();
        if let Some(m) = moved {
            self.kitty_move(m);
        }
    }

    /// **Where the live screen's placeholders draw each virtual
    /// placement**, for a delete by a place: a relative placement under
    /// one is drawn at the least line and display column of its runs plus
    /// its offsets, as a renderer places it, so the delete tests the cells
    /// it is drawn at, "Delete all placements that intersect a specific
    /// cell" (`[[source: kitty-graphics]]`, "Deleting images"). A run on a
    /// line not single-width begins at twice its column, where it is drawn.
    #[cold]
    #[inline(never)]
    fn kitty_mark_drawn(&mut self, at: &KittyAt) {
        let s = &mut *self.strs;
        s.kitty.clear_drawn();
        for r in 0..self.rows.index() {
            let row = Row::from_index(r);
            let stretch = match self.view.rendition(row).is_double() {
                true => 2,
                false => 1,
            };
            let (view, clusters, tmpl) = (&self.view, &*self.clusters, &*self.tmpl);
            let cells = (0..self.cols.index()).map(|c| {
                placeholder_cell(
                    clusters,
                    tmpl,
                    RowStyles::Live,
                    view.get(row, Col::at_index(c)),
                )
            });
            let line = at.top.plus(Rows::of_count(r));
            for run in PlaceholderRuns::new(cells) {
                let col = Col::at_index(run.at.index() * stretch);
                s.kitty
                    .mark_drawn(run.image, run.placement, (line, col), self.images);
            }
        }
    }

    /// `ED 2`: the kitty images on screen go with the text.
    #[cold]
    #[inline(never)]
    pub(crate) fn kitty_clear(&mut self) {
        let at = self.kitty_at();
        self.strs.kitty.clear_screen(&at, self.images);
    }

    /// Where the terminal is, for the stream.
    fn kitty_at(&self) -> KittyAt {
        let (screen, top) = self.image_top();
        KittyAt {
            screen,
            top,
            row: Rows::of_count(self.cy.index()),
            // The display's column, where a placement begins, and the
            // columns a delete by the cursor's cell compares
            // (`Paint::display_col`).
            col: self.display_col(self.cx.col()),
            span: Cols::of_index(match self.view.rendition(self.cy).is_double() {
                true => 2,
                false => 1,
            }),
            rows: self.rows,
            cols: self.cols,
            cell_w: self.cell_width,
            cell_h: self.cell_height,
        }
    }

    /// **Move the cursor past a placement**: right by the columns the
    /// placement covers and down by its rows less one, so it ends on the
    /// placement's last row, then to the next row's start past the right
    /// edge and scrolling past the bottom. The protocol's text says "to the
    /// right by the number of cols in the image placement rectangle and down
    /// by the number of rows" (`[[source: kitty-graphics]]`, "Controlling
    /// displayed image layout"), a row more than this move; clients are
    /// written against the row less one. Down is `index`, so a placement at
    /// the bottom scrolls. Past the right margin the cursor goes to the left
    /// margin of the next row, the Sixel `?8452` rule; with no margins set
    /// that is column 0. A move longer than the ring scrolls no further than
    /// the ring, which it has blanked by then.
    ///
    /// **Right of the image on the row it lands on**: its right edge on the
    /// display, from the column it began at, as a column of that row, so a
    /// character position on a double line (`Paint::char_at_display`).
    fn kitty_move(&mut self, m: KittyMove) {
        let edge = Col::at_index(
            self.display_col(self.cx.col())
                .index()
                .saturating_add(m.cols.index()),
        );
        for _ in 0..m
            .rows
            .index()
            .saturating_sub(1)
            .min(self.view.ring().index())
        {
            self.index_bringing_in();
        }
        if m.cols.index() == 0 {
            return;
        }
        let right = self.char_at_display(edge);
        if right > self.line_fence() {
            self.index_bringing_in();
            self.cx = CursorCol::moved(self.cold.left);
        } else {
            self.cx = CursorCol::moved(right);
        }
    }

    /// The stream's replies into the reply buffer, each whole or counted,
    /// framed as `APC` ... `ST` in the controls S7C1T/S8C1T chose.
    pub(crate) fn kitty_replies(&mut self) {
        let frame = StringReply::apc();
        for body in self.strs.kitty.replies() {
            self.reply.lend(self.cold).string(frame, &[body]);
        }
        self.strs.kitty.clear_replies();
    }
}

impl Term<'_> {
    /// **Advance a deferred kitty decode** by about `budget` bytes of work:
    /// a PNG, or `o=z` pixels, placed when their size was known and shown
    /// when this completes them. Call it between reads and on display-link
    /// ticks while it answers [`DecodeProgress::More`]; the reply the
    /// decode owes is in [`Term::reply`] once it completes. A command that
    /// arrives first finishes the decode itself, so nothing is lost by
    /// calling it late, only shown late.
    pub fn decode_step(&mut self, budget: DecodeBudget) -> DecodeProgress {
        let s = &mut self.store.strs;
        let p = s
            .kitty
            .step(budget, &mut self.store.images, s.kitty_scratch);
        // Framed with the controls in force now, as a reply written later
        // than its query is.
        let frame = StringReply::apc();
        for body in s.kitty.replies() {
            self.store.reply.lend(&self.cold).string(frame, &[body]);
        }
        s.kitty.clear_replies();
        p
    }

    /// **Lend the reader for the file mediums** `t=f`, `t=t` and `t=s`
    /// (`[[source: kitty-graphics]]`, "The transmission medium"): from here
    /// a file medium's path, `O`, `S` and bound go to `files` at the
    /// command's terminator, inside the read, and the bytes it hands back
    /// are decoded as a direct payload's are. The core never opens a file;
    /// the checks the text asks for -- regular files only, the sensitive
    /// parts of the filesystem refused, one identical error -- are the
    /// reader's. With none lent each answers that error.
    ///
    /// `'static`, because the reader lives in the kitty stream, behind its
    /// pointer, and not beside it in `Storage`, where a field moved every
    /// field after it and the text path's code with them; an embedder
    /// lends a reader that outlives the terminal: the macOS app holds it
    /// with the terminal and frees it when the window closes
    /// (`lent::Screen` in baud-app).
    pub fn set_kitty_files(&mut self, files: &'static mut dyn KittyFiles) {
        self.store.strs.kitty.set_files(files);
    }

    /// **Lend the way freed image pages go back to the operating system**
    /// ([`PageRelease`]): from here the whole pages a freed image's span
    /// leaves wholly free -- a delete, an eviction, `RIS`, `ED 2`, a
    /// placement expiring -- the rows a Sixel's claim wrote that its image
    /// does not hold, and the PNG scratch a decode is done with go to
    /// `release`, at the free, which is rare, and never on the per-byte
    /// path. The core makes no system call; with none lent a freed page
    /// stays committed until an image writes it again. An embedder setting,
    /// which `RIS` keeps.
    pub fn set_page_release(&mut self, release: PageRelease) {
        self.store.images.set_release(release);
    }

    /// **A tick of kitty's animation clock at `now`** (`[[source:
    /// kitty-graphics]]`, "Animation"): each running animation placed
    /// somewhere whose frame's gap has passed shows its next frame
    /// ([`ImageStore::animate`]), answered [`Animated::Changed`] when a
    /// frame on show changed, which owes a frame. The macOS loop calls it
    /// on each display-link tick and the Linux loop on each turn, with the
    /// system's motion preference: held, nothing moves on the clock.
    pub fn animate_images(&mut self, now: HostTime, motion: ImageMotion) -> Animated {
        self.store.images.animate(now, motion)
    }

    /// When the next animation frame is due after `now`, for a loop that
    /// waits on a deadline, a tick at `now` already taken
    /// ([`ImageStore::next_frame`]); none while nothing runs.
    pub fn next_image_frame(&self, now: HostTime) -> Option<HostTime> {
        self.store.images.next_frame(now)
    }

    /// What the kitty graphics protocol did: commands, refusals, ignored
    /// commands, cancelled chunks, images stored.
    pub fn kitty(&self) -> KittyCounts {
        self.store.strs.kitty.counts
    }

    /// How the graphics protocol's stream (APC G) was fed ([`KittyFeed`]):
    /// one call a byte on
    /// a per-byte path, one a run on the bulk path. Where the reads fell
    /// decides it, so it is beside [`Term::kitty`] and not in it.
    pub fn kitty_feed(&self) -> KittyFeed {
        self.store.strs.kitty.feed
    }
}

/// **What cell `c` is to a placeholder run** (`[[source: kitty-graphics]]`,
/// "Unicode placeholders"): U+10EEEE with its diacritics and the ids its
/// foreground and underline colours carry, or another character. The one
/// reading of a cell [`Term::placeholder_runs`] and a delete by a place
/// share.
pub(crate) fn placeholder_cell(
    clusters: &ClusterStore<'_>,
    tmpl: &[Option<StyleWord>],
    styles: RowStyles<'_>,
    c: PackedCell,
) -> PlaceholderCell {
    let base = clusters.base_of(c.content);
    if !base.as_codepoint().is_some_and(is_placeholder) {
        return PlaceholderCell::Other;
    }
    let marks = match c.content.placeholder_marks() {
        Some(m) => m,
        None => PlaceholderMarks::read(clusters.marks_of(c.content)),
    };
    let style = styles.word(tmpl, c.style);
    PlaceholderCell::Placeholder {
        marks,
        fg: style.fg(),
        underline: style.underline(),
    }
}
