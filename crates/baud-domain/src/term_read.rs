//! **What a reader of a terminal reads, as a trait**: [`TermRead`], and
//! [`ImageRead`] for the images.
//!
//! Each method gives the same result as the terminal's own method of the
//! same name, with the same signature, so a renderer written against
//! `&impl TermRead` draws what it drew against the terminal itself. Every
//! method takes `&self`: closing a write generation is a write, and the
//! stage that holds the terminal mutably takes its seal and passes it in as
//! a value.
//!
//! Rows are read by index: a row to its cells and its generation through
//! [`Viewport`], [`RowGens`] and [`StatusLine`]. No signature here carries a
//! ring rotation or a row pitch to compute with, so an implementor may hold
//! its rows in any layout that answers a row by its index.
//!
//! Dispatch is static: four reads return `impl Iterator` and [`TermRead::find`]
//! takes `impl FnMut`, so a reader takes `&impl TermRead` or `<T: TermRead>`.

use crate::{
    AmbiguousWidth, Band, BandsView, BidiDirection, BidiSupport, Chan, ClusterCells, ClusterEpoch,
    ClusterWrites, Col, ColorEpoch, ColorModes, Cols, Content, CopyDirection, CursorStyle,
    DynamicColor, Found, HistoryErasures, HistoryWipes, ImageAge, ImageGen, ImageId, ImageOrigin,
    ImagePlaced, ImageSince, ImageStore, ImageView, KittyId, KittyNumber, KittyPlacement,
    KittyRoot, KittyVirtual, LineIdx, Needle, PackedCell, PaintRole, PlaceholderCell,
    PlaceholderRuns, PlacementId, PlacementSlot, ProgramPaint, PromptMark, Reclaims, Resets, Row,
    RowGens, RowStyles, Rows, Screen, ScreenEpoch, Seal, SearchLines, SpecialColor, StatusGen,
    StatusLine, Stride, StringEntry, StringId, StyleWord, StyleWrites, SurfaceLine, TableSerial,
    UnicodeWidths, Video, Viewport, Vs16Width, WrapPending, Written, Zone,
};

/// **A terminal, read**: the reads a renderer makes of a terminal, each
/// answering as the terminal's own method of the same name does.
///
/// **A read never writes.** Closing a write generation is a write, and no
/// method here takes `&mut self`, so a reader holding `&impl TermRead`
/// cannot seal, resize or feed the terminal it reads.
///
/// ```compile_fail,E0599
/// fn f<T: baud_domain::TermRead>(t: &T) {
///     let _ = t.seal_writes();
/// }
/// ```
/// ```compile_fail,E0599
/// fn f<T: baud_domain::TermRead>(t: &T, ws: &baud_domain::WinSize) {
///     let _ = t.resize(ws);
/// }
/// ```
///
/// The legal path:
///
/// ```
/// fn f<T: baud_domain::TermRead>(t: &T) -> bool {
///     t.cursor_visible()
/// }
/// ```
///
/// **A frame's planning inputs are not reads.** The program's cursor guide,
/// what `?1001` highlights and whether a ReGIS page keeps blinking are read
/// once by the plan before a frame and handed to it, so a reader holding a
/// `TermRead` reaches none of them:
///
/// ```compile_fail,E0599
/// fn f<T: baud_domain::TermRead>(t: &T) {
///     let _ = t.highlight();
/// }
/// ```
/// ```compile_fail,E0599
/// fn f<T: baud_domain::TermRead>(t: &T) {
///     let _ = t.cursor_guide();
/// }
/// ```
/// ```compile_fail,E0599
/// fn f<T: baud_domain::TermRead>(t: &T) {
///     let _ = t.regis_blink();
/// }
/// ```
///
/// **Dispatch is static.** The image reads are a generic associated type
/// and four reads return `impl Iterator`, so the trait has no `dyn` form and
/// every call is resolved at compile time.
///
/// ```compile_fail,E0038
/// fn f(_: &dyn baud_domain::TermRead) {}
/// ```
///
/// The legal path:
///
/// ```
/// fn f(t: &impl baud_domain::TermRead) -> baud_domain::Rows {
///     t.window_back()
/// }
/// ```
pub trait TermRead {
    /// The images' reads, borrowed from the terminal for as long as `'s`.
    type Images<'s>: ImageRead + Copy
    where
        Self: 's;

    // Geometry and cursor.

    /// The live geometry, which a resize can change.
    fn geometry(&self) -> (Cols, Rows);
    /// The distance between rows of the terminal's cells.
    fn stride(&self) -> Stride;
    /// Where the cursor is drawn, clamped to the last column.
    fn cursor(&self) -> (Row, Col);
    /// Whether the main display's cursor is drawn (`?25` DECTCEM).
    fn cursor_visible(&self) -> bool;
    /// The cursor to draw: its style and whether it blinks.
    fn cursor_style(&self) -> CursorStyle;
    /// Whether the next graphic character wraps before it is written.
    fn wrap_pending(&self) -> WrapPending;
    /// How far back into the history the window drawn is.
    fn window_back(&self) -> Rows;
    /// How far back a view asked for `back` lines up is, bounded by the
    /// history.
    fn looking_back(&self, back: Rows) -> Rows;
    /// The rows the status line takes beside the main display.
    fn status_rows(&self) -> Rows;

    // Rows and views.

    /// One screen row's cells, or `None` if `y` is not on the grid.
    fn row(&self, y: Row) -> Option<&[PackedCell]>;
    /// The screen scrolled `back` lines into the history.
    fn view(&self, back: Rows) -> Viewport<'_>;
    /// The live screen.
    fn viewport(&self) -> Viewport<'_>;
    /// The window `back` lines into the history, each row with its write
    /// generation.
    fn row_gens(&self, back: Rows) -> RowGens<'_>;
    /// The status line as a window of one row, while one is shown.
    fn status_view(&self) -> Option<Viewport<'_>>;
    /// The status line to draw below the main display, while one is shown.
    fn status_line(&self) -> Option<StatusLine<'_>>;
    /// The status line's write generation, while a row is lent for it.
    fn status_gen(&self) -> Option<StatusGen>;
    /// Which way row `r` of the window `back` lines up is drawn.
    fn line_direction(&self, back: Rows, r: Row) -> BidiDirection;
    /// Where a view `back` lines up starts, in the live screen's image
    /// numbering.
    fn image_origin(&self, back: Rows) -> ImageOrigin;

    // Change signals.

    /// The seal a copy read now is built under.
    fn sealed(&self) -> Seal;
    /// Whether any surface was written after `kept`.
    fn wrote_since(&self, kept: &Seal) -> bool;
    /// Which surface is live.
    fn screen(&self) -> Screen;
    /// Which surface is up, and which time: it moves on every switch.
    fn screen_epoch(&self) -> ScreenEpoch;
    /// Which full reset this is: it moves on every `RIS`.
    fn resets(&self) -> Resets;
    /// Which erasure of the primary's history this is.
    fn history_erasures(&self) -> HistoryErasures;
    /// Which selective erase of the saved lines this is.
    fn history_wipes(&self) -> HistoryWipes;
    /// Moves whenever a cluster slot may have been given to another
    /// cluster.
    fn cluster_epoch(&self) -> ClusterEpoch;
    /// Moves on every cluster written into a slot ([`ClusterWrites`]).
    fn cluster_writes(&self) -> ClusterWrites;
    /// Moves when a colour a rendition resolves through changes.
    fn color_epoch(&self) -> ColorEpoch;
    /// How many writes the rendition table has taken.
    fn style_writes(&self) -> StyleWrites;
    /// Which rendition table this terminal writes.
    fn style_table(&self) -> TableSerial;
    /// The rendition slots written after the given write count, oldest first.
    fn styles_written(&self, since: StyleWrites) -> Written<'_>;
    /// The rows each band's table covers; `None` in the single mode, where
    /// one table covers every row.
    fn band_rows(&self) -> Option<Rows>;
    /// Which rendition table band `b` writes.
    fn band_style_table(&self, b: Band) -> TableSerial;
    /// How many writes band `b`'s rendition table has taken.
    fn band_style_writes(&self, b: Band) -> StyleWrites;
    /// Band `b`'s rendition slots written after the given write count,
    /// oldest first.
    fn band_styles_written(&self, b: Band, since: StyleWrites) -> Written<'_>;

    // Renditions and colours.

    /// Every band's live rendition table, read-only.
    fn styles(&self) -> BandsView<'_>;
    /// The rendition `cell` stands for, on a row that resolves through
    /// `styles`.
    fn style_in(&self, styles: RowStyles<'_>, cell: PackedCell) -> StyleWord;
    /// What the rendition table has reclaimed.
    fn style_reclaims(&self) -> Reclaims;
    /// What band `b`'s rendition table has reclaimed.
    fn band_style_reclaims(&self, b: Band) -> Reclaims;
    /// A palette entry a program set with `OSC 4`, if any.
    fn palette(&self, index: Chan) -> Option<(u8, u8, u8)>;
    /// The default foreground or background a program set, if any.
    fn dynamic_color(&self, which: DynamicColor) -> Option<(u8, u8, u8)>;
    /// The special colour `which` as a program set it, if any.
    fn special_color(&self, which: SpecialColor) -> Option<(u8, u8, u8)>;
    /// The cursor colour as a frame drawn in `video` shows it.
    fn cursor_color_in(&self, video: Video) -> Option<(u8, u8, u8)>;
    /// What a program asked `role` to be drawn in.
    fn program_paint(&self, role: PaintRole) -> ProgramPaint;
    /// Which special-colour modes are on.
    fn color_modes(&self) -> ColorModes;
    /// Whether reverse video is set (`?5` DECSCNM).
    fn reverse_video(&self) -> bool;

    // Text and clusters.

    /// The character `cell` shows as its base.
    fn base_of(&self, cell: PackedCell) -> Content;
    /// The codepoints attached to `cell`'s base.
    fn cluster_marks(&self, cell: PackedCell) -> impl Iterator<Item = u32> + '_;
    /// How much of a cluster a cell holds.
    fn cluster_cells(&self) -> ClusterCells;
    /// The width of East Asian ambiguous characters.
    fn ambiguous_width(&self) -> AmbiguousWidth;
    /// Which Unicode version's widths the next character is measured by.
    fn unicode_widths(&self) -> UnicodeWidths;
    /// How wide a base with `U+FE0F` is.
    fn vs16_width(&self) -> Vs16Width;
    /// Whether any presentation variant other than the default has been
    /// selected.
    fn variants_seen(&self) -> bool;
    /// Whether lines are drawn in the bidirectional order (BDSM).
    fn bidi_support(&self) -> BidiSupport;
    /// Which way a copy reads each line (`?96` DECRLCM).
    fn copy_direction(&self) -> CopyDirection;
    /// The string `id` names, or `None` for no string.
    fn string_entry(&self, id: StringId) -> Option<StringEntry>;
    /// Whether any cell can name a string.
    fn strings_seen(&self) -> bool;

    // Search and prompts.

    /// Every match of `needle` in the lines from `from` to before `to`, in
    /// reading order, to `each`.
    fn find(&self, needle: &Needle, from: LineIdx, to: LineIdx, each: impl FnMut(Found));
    /// The lines of the live surface a search reads.
    fn search_lines(&self) -> SearchLines;
    /// The first line of the wrapped line `line` is part of.
    fn line_start(&self, line: LineIdx) -> LineIdx;
    /// Where `line` lies on the live surface, or `None` outside
    /// [`TermRead::search_lines`].
    fn surface_line(&self, line: LineIdx) -> Option<SurfaceLine>;
    /// The prompts the shell drew on the primary, oldest first.
    fn prompts(&self) -> impl Iterator<Item = PromptMark> + '_;
    /// The prompts on the window `back` lines up and below it, oldest
    /// first.
    fn prompts_from(&self, back: Rows) -> impl Iterator<Item = PromptMark> + '_;
    /// The screen row the current prompt block begins on, while the shell
    /// is at a prompt.
    fn prompt_row(&self) -> Option<Row>;
    /// The primary's zone, whichever screen is up.
    fn primary_zone(&self) -> Zone;

    // Images.

    /// The stored images and their placements.
    fn images(&self) -> Self::Images<'_>;
    /// The virtual placement a placeholder run shows.
    fn kitty_virtual(&self, image: KittyId, placement: KittyPlacement) -> Option<KittyVirtual>;
    /// Whether any virtual placement is live.
    fn kitty_virtuals(&self) -> bool;
    /// The Unicode placeholders of `row`, resolved into runs.
    fn placeholder_runs<'r>(
        &'r self,
        row: &'r [PackedCell],
        styles: RowStyles<'r>,
    ) -> PlaceholderRuns<impl Iterator<Item = PlaceholderCell> + 'r>;
}

/// **The images, read**: the reads a renderer makes of the image store,
/// each answering as the store's own method of the same name does.
///
/// **Only reads.** A reader holding an `ImageRead` cannot store, place or
/// free an image.
///
/// ```compile_fail,E0599
/// fn f<I: baud_domain::ImageRead>(mut i: I, id: baud_domain::ImageId) {
///     i.free(id);
/// }
/// ```
///
/// The legal path:
///
/// ```
/// fn f<I: baud_domain::ImageRead>(i: I) -> baud_domain::ImageGen {
///     i.generation()
/// }
/// ```
pub trait ImageRead {
    /// The store's generation: it moves on every change a frame draws.
    fn generation(&self) -> ImageGen;
    /// Moves on every placement made ([`ImagePlaced`]).
    fn placed(&self) -> ImagePlaced;
    /// What became of image `id`, born at `born`, after generation
    /// `then`.
    fn image_since(&self, id: ImageId, born: ImageAge, then: ImageGen) -> ImageSince;
    /// Image `id`'s pixels and size, while it is stored.
    fn image(&self, id: ImageId) -> Option<ImageView<'_>>;
    /// Every live placement, with its id.
    fn placements(&self) -> impl Iterator<Item = (PlacementId, &PlacementSlot)> + '_;
    /// What placement `pid` is anchored to.
    fn kitty_root(&self, pid: PlacementId) -> KittyRoot;
    /// The image id and number image `id` was given, if any.
    fn kitty_names(&self, id: ImageId) -> Option<(KittyId, KittyNumber)>;
    /// When image `id` was stored, while it is.
    fn born(&self, id: ImageId) -> Option<ImageAge>;
}

impl ImageRead for &ImageStore<'_> {
    fn generation(&self) -> ImageGen {
        ImageStore::generation(self)
    }
    fn placed(&self) -> ImagePlaced {
        ImageStore::placed(self)
    }
    fn image_since(&self, id: ImageId, born: ImageAge, then: ImageGen) -> ImageSince {
        ImageStore::since(self, id, born, then)
    }
    fn image(&self, id: ImageId) -> Option<ImageView<'_>> {
        ImageStore::image(self, id)
    }
    fn placements(&self) -> impl Iterator<Item = (PlacementId, &PlacementSlot)> + '_ {
        ImageStore::placements(self)
    }
    fn kitty_root(&self, pid: PlacementId) -> KittyRoot {
        ImageStore::kitty_root(self, pid)
    }
    fn kitty_names(&self, id: ImageId) -> Option<(KittyId, KittyNumber)> {
        ImageStore::kitty_names(self, id)
    }
    fn born(&self, id: ImageId) -> Option<ImageAge> {
        ImageStore::born(self, id)
    }
}
