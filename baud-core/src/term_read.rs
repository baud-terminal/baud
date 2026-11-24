//! **`Term` read through [`TermRead`]**: each method calls `Term`'s own
//! method of the same name, so a reader written against the trait reads
//! what it reads from the terminal itself.

use crate::Term;
use baud_domain::*;

impl<'b> TermRead for Term<'b> {
    type Images<'s>
        = &'s ImageStore<'b>
    where
        Self: 's;

    fn geometry(&self) -> (Cols, Rows) {
        Term::geometry(self)
    }

    fn stride(&self) -> Stride {
        Term::stride(self)
    }

    fn cursor(&self) -> (Row, Col) {
        Term::cursor(self)
    }

    fn cursor_visible(&self) -> bool {
        Term::cursor_visible(self)
    }

    fn cursor_style(&self) -> CursorStyle {
        Term::cursor_style(self)
    }

    fn wrap_pending(&self) -> WrapPending {
        Term::wrap_pending(self)
    }

    fn window_back(&self) -> Rows {
        Term::window_back(self)
    }

    fn looking_back(&self, back: Rows) -> Rows {
        Term::looking_back(self, back)
    }

    fn status_rows(&self) -> Rows {
        Term::status_rows(self)
    }

    fn row(&self, y: Row) -> Option<&[PackedCell]> {
        Term::row(self, y)
    }

    fn view(&self, back: Rows) -> Viewport<'_> {
        Term::view(self, back)
    }

    fn viewport(&self) -> Viewport<'_> {
        Term::viewport(self)
    }

    fn row_gens(&self, back: Rows) -> RowGens<'_> {
        Term::row_gens(self, back)
    }

    fn status_view(&self) -> Option<Viewport<'_>> {
        Term::status_view(self)
    }

    fn status_line(&self) -> Option<StatusLine<'_>> {
        Term::status_line(self)
    }

    fn status_gen(&self) -> Option<StatusGen> {
        Term::status_gen(self)
    }

    fn line_direction(&self, back: Rows, r: Row) -> BidiDirection {
        Term::line_direction(self, back, r)
    }

    fn image_origin(&self, back: Rows) -> ImageOrigin {
        Term::image_origin(self, back)
    }

    fn sealed(&self) -> Seal {
        Term::sealed(self)
    }

    fn wrote_since(&self, kept: &Seal) -> bool {
        Term::wrote_since(self, kept)
    }

    fn screen(&self) -> Screen {
        Term::screen(self)
    }

    fn screen_epoch(&self) -> ScreenEpoch {
        Term::screen_epoch(self)
    }

    fn resets(&self) -> Resets {
        Term::resets(self)
    }

    fn history_erasures(&self) -> HistoryErasures {
        Term::history_erasures(self)
    }

    fn history_wipes(&self) -> HistoryWipes {
        Term::history_wipes(self)
    }

    fn cluster_epoch(&self) -> ClusterEpoch {
        Term::cluster_epoch(self)
    }

    fn color_epoch(&self) -> ColorEpoch {
        Term::color_epoch(self)
    }

    fn style_writes(&self) -> StyleWrites {
        Term::style_writes(self)
    }

    fn style_table(&self) -> TableSerial {
        Term::style_table(self)
    }

    fn styles_written(&self, since: StyleWrites) -> Written<'_> {
        Term::styles_written(self, since)
    }

    fn band_rows(&self) -> Option<Rows> {
        Term::band_rows(self)
    }

    fn band_style_table(&self, b: Band) -> TableSerial {
        Term::band_style_table(self, b)
    }

    fn band_style_writes(&self, b: Band) -> StyleWrites {
        Term::band_style_writes(self, b)
    }

    fn band_styles_written(&self, b: Band, since: StyleWrites) -> Written<'_> {
        Term::band_styles_written(self, b, since)
    }

    fn styles(&self) -> BandsView<'_> {
        Term::styles(self)
    }

    fn style_in(&self, styles: RowStyles<'_>, cell: PackedCell) -> StyleWord {
        Term::style_in(self, styles, cell)
    }

    fn style_reclaims(&self) -> Reclaims {
        Term::style_reclaims(self)
    }

    fn band_style_reclaims(&self, b: Band) -> Reclaims {
        Term::band_style_reclaims(self, b)
    }

    fn palette(&self, index: Chan) -> Option<(u8, u8, u8)> {
        Term::palette(self, index)
    }

    fn dynamic_color(&self, which: DynamicColor) -> Option<(u8, u8, u8)> {
        Term::dynamic_color(self, which)
    }

    fn special_color(&self, which: SpecialColor) -> Option<(u8, u8, u8)> {
        Term::special_color(self, which)
    }

    fn cursor_color_in(&self, video: Video) -> Option<(u8, u8, u8)> {
        Term::cursor_color_in(self, video)
    }

    fn program_paint(&self, role: PaintRole) -> ProgramPaint {
        Term::program_paint(self, role)
    }

    fn color_modes(&self) -> ColorModes {
        Term::color_modes(self)
    }

    fn reverse_video(&self) -> bool {
        Term::reverse_video(self)
    }

    fn base_of(&self, cell: PackedCell) -> Content {
        Term::base_of(self, cell)
    }

    fn cluster_marks(&self, cell: PackedCell) -> impl Iterator<Item = u32> + '_ {
        Term::cluster_marks(self, cell)
    }

    fn cluster_cells(&self) -> ClusterCells {
        Term::cluster_cells(self)
    }

    fn ambiguous_width(&self) -> AmbiguousWidth {
        Term::ambiguous_width(self)
    }

    fn unicode_widths(&self) -> UnicodeWidths {
        Term::unicode_widths(self)
    }

    fn vs16_width(&self) -> Vs16Width {
        Term::vs16_width(self)
    }

    fn variants_seen(&self) -> bool {
        Term::variants_seen(self)
    }

    fn bidi_support(&self) -> BidiSupport {
        Term::bidi_support(self)
    }

    fn copy_direction(&self) -> CopyDirection {
        Term::copy_direction(self)
    }

    fn string_entry(&self, id: StringId) -> Option<StringEntry> {
        Term::string_entry(self, id)
    }

    fn strings_seen(&self) -> bool {
        Term::strings_seen(self)
    }

    fn find(&self, needle: &Needle, from: LineIdx, to: LineIdx, each: impl FnMut(Found)) {
        Term::find(self, needle, from, to, each)
    }

    fn search_lines(&self) -> SearchLines {
        Term::search_lines(self)
    }

    fn line_start(&self, line: LineIdx) -> LineIdx {
        Term::line_start(self, line)
    }

    fn surface_line(&self, line: LineIdx) -> Option<SurfaceLine> {
        Term::surface_line(self, line)
    }

    fn prompts(&self) -> impl Iterator<Item = PromptMark> + '_ {
        Term::prompts(self)
    }

    fn prompts_from(&self, back: Rows) -> impl Iterator<Item = PromptMark> + '_ {
        Term::prompts_from(self, back)
    }

    fn prompt_row(&self) -> Option<Row> {
        Term::prompt_row(self)
    }

    fn primary_zone(&self) -> Zone {
        Term::primary_zone(self)
    }

    fn images(&self) -> &ImageStore<'b> {
        Term::images(self)
    }

    fn kitty_virtual(&self, image: KittyId, placement: KittyPlacement) -> Option<KittyVirtual> {
        Term::kitty_virtual(self, image, placement)
    }

    fn kitty_virtuals(&self) -> bool {
        Term::kitty_virtuals(self)
    }

    fn placeholder_runs<'r>(
        &'r self,
        row: &'r [PackedCell],
        styles: RowStyles<'r>,
    ) -> PlaceholderRuns<impl Iterator<Item = PlaceholderCell> + 'r> {
        Term::placeholder_runs(self, row, styles)
    }

    fn cursor_guide(&self) -> Option<CursorGuide> {
        Term::cursor_guide(self)
    }

    fn highlight(&self) -> Option<Highlighted> {
        Term::highlight(self)
    }

    fn regis_blink(&self) -> RegisBlink {
        Term::regis_blink(self)
    }
}
