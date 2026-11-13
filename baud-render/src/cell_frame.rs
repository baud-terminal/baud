//! **The cell path's frame, built once for every caller**: the window
//! laid out, the set's copy of the screen put back and compared and taken
//! (`crate::screens`), a frozen or struck row lent its slots, the glyph map
//! walked unless the screen and the walk's key are the last frame's, and
//! the overlay's records and the records after them gathered -- what the
//! app's `present` builds for the cell shader, and what a headless count
//! of a frame's work reads (`baud-present-macos/tests/caret_frame.rs`), so
//! the two cannot describe different frames.
//!
//! The caller keeps what is its own: the inputs it sets on the renderer
//! before (the selection, the links, the blink and the cursor's phase, the
//! images), the GPU's buffers the slices here are lent from, which set is
//! free to build into, and what it hands the GPU after.

use crate::screens::{Built, Compare, Held, Screens, SetIx, Taking};
use crate::{
    Candidate, Composition, GlyphMap, Highlight, Instance, Leaving, Lent, Overlay, Renderer,
    Resolved, Scene, StyleStamp, Tally, WalkKey,
};
use baud_core::Term;
use baud_domain::{
    CellFlags, Codepoint, Col, Cols, Content, CursorStyle, PackedCell, Row, RowExtent, RowRot,
    Rows, Seal,
};
use baud_font::Px;

/// **What a cell frame is built from**, beyond the terminal and the
/// renderer's own state.
pub struct CellInputs<'i, P> {
    /// How far the window looks back into the history.
    pub back: Rows,
    /// Row 9's predicted echoes, each a codepoint at its logical cell.
    pub predictions: P,
    /// The cursor as the frame presents it, its cell and its style, before
    /// the echoes and a composition move it.
    pub live: Option<(Row, Col, CursorStyle)>,
    /// An input method's composition on the view, or none.
    pub composition: &'i Option<Composition>,
    /// The selection on the view, or none.
    pub highlight: Option<Highlight>,
    /// Whether a smooth scroll's leaving row is drawn this frame.
    pub leaving: Leaving,
    /// The slide's pan, read once for the frame.
    pub pan: Px,
}

/// **The cell path's memory a frame builds with**, lent by the caller: one
/// entry of each per-set slice a buffer set (`crate::screens::SETS`).
/// The map's entries and the sets' buffers are lent for lifetimes of
/// their own, `'g` and `'m`.
pub struct CellParts<'a, 'g, 'm> {
    pub map: &'a mut GlyphMap<'g>,
    pub exceptions: &'a mut Overlay,
    pub screens: &'a mut Screens,
    pub copies_mem: &'a mut [&'m mut [PackedCell]],
    pub styles_mem: &'a mut [&'m mut [Resolved]],
    pub overlay_mem: &'a mut [&'m mut [u8]],
    pub style_stamps: &'a mut [StyleStamp],
    pub lent: &'a mut [Lent],
    pub candidates: &'a mut Vec<Candidate>,
    pub records: &'a mut Vec<Instance>,
    pub grounds: &'a mut Vec<Instance>,
    pub bidi_cells: &'a mut Vec<PackedCell>,
    pub bidi_used: &'a mut Vec<RowExtent>,
    pub lent_cells: &'a mut Vec<PackedCell>,
    pub lent_used: &'a mut Vec<RowExtent>,
}

/// **What a built cell frame hands its caller**, for the GPU's input and
/// for a screen switch's kept frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellFrame {
    /// The set it was built into.
    pub at: SetIx,
    /// Whether it was built from the last frame's screen.
    pub built: Built,
    /// The view's columns and rows.
    pub cols: Cols,
    pub rows: Rows,
    /// The rows the shader draws: the window's, the status line's, the
    /// notice's and, while sliding, the leaving row.
    pub ring: Rows,
    /// The cursor drawn, if its phase draws one.
    pub cursor: Option<(Row, Col, CursorStyle)>,
    /// How many of the records the overlay's walk wrote.
    pub walked: Tally,
    /// Which records are the trail's and the blink animation's.
    pub moving: (usize, usize),
    /// How many records at the end are baud's own overlays.
    pub chrome: Tally,
}

/// **Builds the cell frame** into the set `building` names, once the
/// window is laid out; none, the layout done, when `building` names none.
/// `used` is left holding each row's mark as the view read it.
///
/// **`seal` is the one the driver closed this frame's writes with**
/// (`Term::seal_writes`), taken once a frame before the build: the set's
/// copy and the sized lines' keys are recorded under it. A required
/// argument, so no driver builds a frame without sealing first; `term` is
/// shared, and the build itself seals nothing.
pub fn build<P>(
    renderer: &mut Renderer<'_, '_>,
    term: &Term<'_>,
    seal: Seal,
    inputs: &CellInputs<'_, P>,
    cell: &mut CellParts<'_, '_, '_>,
    used: &mut Vec<RowExtent>,
    building: impl FnOnce() -> Option<SetIx>,
) -> Option<CellFrame>
where
    P: Iterator<Item = (Row, Col, Codepoint)> + DoubleEndedIterator + Clone,
{
    let (leaving, pan, highlight) = (inputs.leaving, inputs.pan, inputs.highlight);
    let sliding = leaving == Leaving::Drawn;
    let composition = inputs.composition;
    let back = inputs.back;
    // The window in the order each row is drawn in, laid out once
    // for this frame: the core's own view when no row is
    // reordered. With BDSM reset, once a letter that takes a
    // joining form has been drawn, the last frame's joins are
    // kept for the rows the copy's compare finds changed.
    // Row 9's predicted echoes, on the live view, are laid out
    // as written, so a prediction's row is drawn in the order
    // its echo will be.
    let joins_seen = renderer.joins_seen();
    renderer.set_predicted(inputs.predictions.clone().filter(|_| back.index() == 0));
    let rejoin = renderer.lay_out_bidi_copied(term, back);
    let view = renderer
        .bidi()
        .window(term, back, cell.bidi_cells, cell.bidi_used);
    // Smooth scroll: at the live screen on the primary surface, a
    // non-zero pan draws one leaving row above the window.
    // The leaving row's glyphs are one row up, not in `view`, so
    // ensure the map over the view one line up too -- the
    // persistent map holds them from when the row was on screen,
    // and this covers a clear. It is ensured *first*, because
    // `ensure` clears `candidates` and refills them for the view
    // it walks (`cellmap`): the screen must be ensured last, or
    // the overlay's records name `above`'s rows -- the whole
    // screen one line up -- and every mark and ligature is drawn
    // a row off its cell for the length of the slide.
    // Which buffers this frame may write: any set no frame in
    // flight was committed with. `None` would mean every one is
    // held and there is nowhere to build that a live draw is not
    // reading -- which cannot happen while one frame is in
    // flight and three sets exist, and is a refusal rather than
    // a corruption if it ever does.
    let at = building()?;
    let set = at.0;
    // **The ids this set's copy was lent, put back**, and the
    // slots this set lent its last frame's frozen and struck
    // cells after them: no frame in flight reads this set, so
    // its copy is the screen it was copied from again, and a
    // set that holds the screen need not copy it afresh.
    cell.lent[set].restore(cell.copies_mem[set], term.stride());
    renderer.give_back(
        &mut cell.styles_mem[set][..],
        &mut cell.style_stamps[set],
        &mut cell.lent[set],
    );
    // The set's resolved table, brought up to date from the core's
    // log: only the slots written since this set was last built,
    // which on a steady screen is none. Before the map
    // walk, which reads each cell's face style from it.
    renderer.sync_styles(
        term,
        &mut cell.styles_mem[set][..],
        &mut cell.style_stamps[set],
    );
    // **The leaving row**: the line just above the window,
    // `view(back + 1).row(0)`, appended to the copy at index
    // `vrows`, where `slide_rows = 1` maps draw row 0 to it with
    // the identity rotation the copy is read under -- in the
    // order it was drawn in as the first row of the window one
    // line up, a marked cell given its base, as the shader reads
    // it (`Renderer::copy_leaving`). It is never selected and
    // never holds the cursor (the shader's row there is past
    // every one). The map is ensured over that row as copied,
    // and what that walk finds kept for the row's own records
    // (`Overlay::ensure_leaving`).
    if sliding {
        let (vcols, vrows) = (view.cols().index(), view.rows().index());
        let at = vrows * term.stride().index();
        let row = &mut cell.copies_mem[set][at..at + vcols];
        renderer.copy_leaving(term, back, row);
    }
    let cursor_at = inputs.live;
    // Row 9: the drawn cursor sits after the last predicted
    // glyph, for the shader and for the overlay's own paint.
    let cursor_at = match (cursor_at, inputs.predictions.clone().next_back()) {
        (Some((_, _, style)), Some((row, col, _))) => Some((row, col.after(), style)),
        (c, _) => c,
    };
    // A composition puts the drawn cursor after itself, as the
    // instance and CPU paths do in their convert.
    let cursor_at = match (cursor_at, &composition) {
        (Some((_, _, style)), Some(c)) => {
            c.cursor(c.width_on(&view)).map(|(r, col)| (r, col, style))
        }
        (c, _) => c,
    };
    // No cursor over an address or a message, as the instance and
    // CPU paths have it in their convert.
    let cursor_at = renderer.clear_of_messages(term, cursor_at);
    // **Whether this frame's screen is the last frame's**: its
    // rows, its status line and its line renditions against the
    // copy that frame left, row for row; never while sliding,
    // whose copy holds a leaving row and whose walk another
    // window. **D4: the window, copied into this frame's set**,
    // unless that set's copy holds the screen already -- a
    // frame that changed only the cursor, built into a set an
    // earlier one copied the same screen into. The rows the view
    // names, in screen order, so what the shader reads is a
    // still image of the screen rather than a grid the parser is
    // still writing -- which is what lets `pump` keep feeding
    // while this frame is in flight.
    let stride = term.stride().index();
    let (vcols, vrows) = (view.cols().index(), view.rows().index());
    // The status line in its drawn order, the row after the
    // window's in the renderer's maps.
    let mut status_cells = Vec::new();
    let status = term.status_line().map(|line| {
        renderer
            .bidi()
            .status_line(Row::from_index(vrows), line, &mut status_cells)
    });
    let lined = usize::from(status.is_some());
    let screens = &mut cell.screens;
    screens.compare = Compare::for_rejoin(rejoin);
    // **Whether a cell of this frame names a row the core froze**
    // (`crate::Lent`): the window's, a display line's
    // among them, or the leaving row's. No row at all while the
    // core has frozen nothing, which one test answers.
    let leaving_source = sliding.then(|| renderer.leaving_source(term, back));
    let frozen = renderer.bidi().window_frozen(term, back)
        || leaving_source
            .as_ref()
            .is_some_and(|s| !s.words().is_live());
    // A window that shows a frozen row is copied afresh, its ids
    // the core's. A copy an earlier frame lent was put back
    // above.
    if frozen {
        screens.forget(at);
    }
    // The last frame's copy, as a compare reads it: through the
    // ids its lending rewrote, which its set, maybe in flight,
    // still holds.
    let undo = screens
        .last_set()
        .and_then(|l| cell.lent.get(l.0))
        .map_or(&[][..], crate::Lent::rewrites);
    // Which rows the layout drew in another order, the status line's
    // after the window's, and whether it re-broke a display: rows whose
    // drawn cells read more of the terminal than their stored ones.
    let orders = renderer.bidi();
    let built = screens.take_lent(
        cell.copies_mem,
        at,
        &Taking {
            term,
            seal,
            back,
            view: &view,
            status: status.as_ref(),
            reordered: &|r| orders.reordered(r),
            held: Held::of(orders),
            stride: term.stride(),
            leaving,
        },
        undo,
    );
    // **A frozen row of the window drawn in its own renditions,
    // and a struck link's text in grey** (`crate::Lent`):
    // its copied cells given slots no row of the frame names, the
    // frozen first, and the walk and the overlay reading the
    // copy, so all three resolve each id in the one table. A
    // frame with neither asks one test a row.
    let struck = renderer.struck_cells(&view, view.rows(), crate::CopyOrder::Stored);
    let lend = match frozen || struck.iter().any(|r| !r.is_empty()) {
        false => crate::Lend::Nothing,
        true => {
            // Each window row through the table each of its cells
            // names, in the order the copy holds them; the row
            // after the window's the leaving row while sliding,
            // else the status line's, and the notice's after it.
            let sources: Vec<crate::RowSource<'_>> = (0..vrows)
                .map(|y| renderer.bidi().row_source(term, back, Row::from_index(y)))
                .collect();
            let mut rows: Vec<(&mut [PackedCell], crate::Words<'_>)> = Vec::new();
            for (y, chunk) in cell.copies_mem[set].chunks_mut(stride).enumerate() {
                let words = match (sources.get(y), &leaving_source) {
                    (Some(s), _) => s.words(),
                    (None, Some(s)) if y == vrows => s.words(),
                    _ => crate::Words::LIVE,
                };
                if y > vrows + 1 {
                    break;
                }
                let n = vcols.min(chunk.len());
                rows.push((&mut chunk[..n], words));
            }
            renderer.lend(
                term,
                &mut rows,
                &struck,
                &mut cell.styles_mem[set][..],
                &mut cell.style_stamps[set],
                &mut cell.lent[set],
            )
        }
    };
    // What the walk, the overlay and a sized line read the copy
    // through: the words its lent slots were given.
    renderer.set_frame_lent(&cell.lent[set]);
    // A copy lent frozen rows names other ids than the last
    // frame's copy did, so no compare of the two says the screen
    // is the same. One lent struck cells alone is the last
    // frame's under the same lending: its slots were given for
    // the same renditions, so every id means what it meant, and
    // the map is not walked again.
    let lending = cell.lent[set].lending();
    let built = match (frozen, lend, built) {
        (true, crate::Lend::Rows | crate::Lend::Short, _) => Built::New,
        (_, _, Built::Last) if lending == screens.lending => Built::Last,
        (_, _, Built::Last) => {
            screens.built(Built::New);
            Built::New
        }
        (_, _, Built::New) => Built::New,
    };
    screens.lending = lending;
    // **The leaving row's map ensured**, over the row as copied
    // and lent: before the screen's walk below, which clears the
    // candidates and refills them for the view it walks.
    if sliding {
        let at = vrows * stride;
        let row = &cell.copies_mem[set][at..at + vcols];
        renderer.leaving_lent(row);
        let used = [baud_domain::RowExtent::of(vcols)];
        let leaving = baud_domain::Viewport::new(
            row,
            &used,
            baud_domain::Stride::of_cells(vcols),
            view.cols(),
            Rows::of_count(1),
            Rows::of_count(1),
            RowRot::NONE,
        );
        cell.exceptions.ensure_leaving(
            cell.map,
            term,
            leaving,
            &cell.styles_mem[set][..],
            renderer,
        );
    }
    let view = match lend {
        crate::Lend::Nothing => view,
        _ => {
            cell.lent_cells.clear();
            cell.lent_cells
                .extend_from_slice(&cell.copies_mem[set][..vrows * stride]);
            crate::counters::add(
                crate::counters::Counter::CellsCopied,
                crate::counters::Site::Lent,
                Tally::of(vrows * stride),
            );
            cell.lent_used.clear();
            for y in 0..vrows {
                let r = Row::from_index(y);
                let mut e = baud_domain::RowExtent::of(vcols)
                    .with_rendition(view.rendition(r), view.cols());
                if view.wrapped(r) == baud_domain::Wrap::Continues {
                    e = e.wrapping(vcols);
                }
                cell.lent_used.push(e);
            }
            baud_domain::Viewport::new(
                cell.lent_cells,
                cell.lent_used,
                term.stride(),
                view.cols(),
                view.rows(),
                view.rows(),
                RowRot::NONE,
            )
        }
    };
    // The kept joins found again on the rows that compare found
    // changed, before the walk and the records read the forms.
    if let Some(how) = rejoin {
        renderer.rejoin_bidi(term, back, how, |r| screens.changed(r));
    }
    let key = WalkKey {
        table: cell.style_stamps[set],
        map: cell.map.generation(),
        grown: renderer.atlas_stats().grown,
        clusters: term.cluster_epoch(),
        back,
        cols: view.cols(),
        rows: view.rows(),
    };
    if built == Built::New || screens.walked != Some(key) {
        cell.map.ensure(
            term,
            view,
            &cell.styles_mem[set][..],
            renderer,
            cell.candidates,
        );
        screens.walked = (!sliding).then(|| WalkKey {
            map: cell.map.generation(),
            grown: renderer.atlas_stats().grown,
            ..key
        });
    }
    // **The first letter that takes a joining form**, shaped by
    // the walk with BDSM reset, was laid out unjoined, since no
    // row is read for joins before one is drawn: laid out again,
    // before the records draw the forms. Once a renderer's life.
    renderer.join_after_walk(term, back, joins_seen);
    // The lines at rest on this frame's layout and the look of
    // the link under the pointer, placed once the layout is
    // final and before the walk, which reads the look's fields
    // as empty, as the instance path blanks their cells.
    renderer.place_links(term, &view);
    let cursor = cursor_at.filter(|&(_, _, style)| renderer.cursor_drawn(style));
    // **The config's notice row, after the status line's**:
    // blank cells, whose default background the shader draws,
    // under the notice's field drawn with baud's own overlays
    // below (`Renderer::veils_onto`). No slide runs while it is
    // shown (`below_grid`). Filled on every frame, the copy of
    // the screen skipped or not: a set that holds the screen may
    // not hold the notice's row.
    let noticed = match renderer.notice_shown() {
        Some(_) => {
            let at = (vrows + lined) * stride;
            match cell.copies_mem[set].get_mut(at..at + vcols) {
                Some(row) => {
                    row.fill(PackedCell::EMPTY);
                    1
                }
                None => 0,
            }
        }
        None => 0,
    };
    cell.exceptions.ensure_framed(
        &Scene {
            term,
            view,
            styles: &cell.styles_mem[set][..],
            cursor,
            highlight,
            map: cell.map,
            candidates: cell.candidates,
            // Smooth scroll: the records slide with the grid,
            // gated on the same `sliding` as the grid's own pan
            // so a mark or a ligature never lags a settled frame.
            pan: if sliding { pan } else { Px::of(0) },
            composition: composition.as_ref(),
        },
        crate::renderer::Framed::of(back, seal),
        renderer,
        &mut cell.overlay_mem[set][..],
        cell.records,
    );
    let walked = cell.records.len();
    // Row 9: the predicted echoes, each a masked cell and a
    // record in the rendition of the cell to its left, on the
    // live view only. A cell the walk above already drew a
    // record for -- a mark, or a glyph inside a ligature
    // window -- is left to that record: two records for one
    // cell would draw two glyphs on top of each other, and the
    // echo is a frame away.
    if back.index() == 0 {
        for (row, col, cp) in inputs.predictions.clone() {
            // The rendition is the cell to the left's, Mosh's
            // heuristic: what was typed before this is what
            // this will look like, where the cell under the
            // cursor may hold the shell's own dimmed guess.
            // The logical cell, read from the core's view: the
            // window drawn may show another cell there.
            let at = if col.index() > 0 {
                col.index() - 1
            } else {
                col.index()
            };
            let style = term
                .view(back)
                .row(row)
                .get(at)
                .map_or(PackedCell::EMPTY.style, |c| c.style);
            cell.exceptions.predicted(
                renderer,
                (row, col),
                PackedCell {
                    content: Content::codepoint(cp),
                    style,
                    flags: CellFlags::NONE,
                },
                &cell.styles_mem[set][..],
                cursor,
                highlight,
                view.cols(),
                &mut cell.overlay_mem[set][..],
                cell.records,
            );
        }
    }
    // The composition: its cells masked off the shader and its
    // records -- glyphs and underline -- after the walk's, the
    // same records the instance and CPU paths draw.
    if let Some(c) = &composition {
        let under = view
            .row(c.row)
            .get(renderer.bidi().visual_col(c.row, c.col).index())
            .copied()
            .unwrap_or(PackedCell::EMPTY);
        cell.exceptions.composed(
            renderer,
            term,
            (c, crate::Laid::AsWritten),
            under,
            &cell.styles_mem[set][..],
            cursor,
            view.cols(),
            view.rendition(c.row),
            if sliding { pan } else { Px::of(0) },
            &mut cell.overlay_mem[set][..],
            cell.records,
        );
    }
    // Every match's mark and the link preview's underline, with
    // the content; the find field and the link's bar are baud's
    // own and go over the images, below.
    cell.records.extend_from_slice(renderer.find_solids());
    cell.records.extend_from_slice(renderer.link_solids());
    // The lines at rest, placed before the walk, moved with the
    // text under a slide.
    renderer.link_lines_lowered(if sliding { pan } else { Px::of(0) }, cell.records);
    // The leaving row: what the map does not draw of it drawn by
    // records, as on a row of the window, and a line that is not
    // single-width drawn whole by them, its row of the mask the
    // one after the screen's (`Overlay::leaving`).
    if sliding {
        cell.exceptions.leaving(
            renderer,
            term,
            (cell.map, &cell.styles_mem[set][..]),
            back,
            (view.cols(), view.rows()),
            pan,
            &mut cell.overlay_mem[set][..],
            cell.records,
        );
    }
    // The status line's records, its row of the mask marked.
    cell.exceptions.status(
        renderer,
        term,
        cursor,
        (view.cols(), view.rows()),
        &mut cell.overlay_mem[set][..],
        cell.records,
    );
    // The trail, thumb and screen fade ride with the records,
    // drawn after the cell passes: the trail under the thumb, and
    // the fade last so it veils everything during a switch.
    let moving_from = cell.records.len();
    cell.records.extend_from_slice(renderer.trail());
    // The plasma blink's glow over the cursor's cell, after the
    // trail as `Renderer::instances` has it: built from the
    // cursor before the phase's filter, since the glow is drawn
    // while the cursor is not.
    renderer.set_cursor_glow_at(cursor_at);
    cell.records.extend_from_slice(renderer.cursor_glow());
    // The badge, which stays where the view has it, and the
    // cursor guide from the same cursor, else on the cursor's
    // line while the window looks back, its records under the
    // glyphs (`CellInput::grounds`).
    renderer.place_grounds(cursor_at, crate::Renderer::cursor_line(term, back));
    cell.grounds.clear();
    cell.grounds.extend_from_slice(renderer.badge());
    renderer.cursor_guide_lowered(if sliding { pan } else { Px::of(0) }, cell.grounds);
    let moving = (moving_from, cell.records.len());
    // baud's own overlays, drawn over the images over the text
    // (`CellInput::chrome`): a `BEL`'s tint under the rest; the
    // find field over its cells, masked off the shader as a
    // composition is, and its caret; the link preview's bar the
    // same way; the thumb; the fade.
    let chrome_from = cell.records.len();
    cell.records.extend_from_slice(renderer.bell_tint());
    // The link under the pointer's fields, the find field and its caret,
    // the link's bar, the tip's lines and the notice: their cells masked
    // off the shader as a composition's are, and their records kept while
    // nothing they are drawn from moves (FS-C16).
    renderer.veils_onto(
        term,
        (&cell.styles_mem[set][..], cell.style_stamps[set]),
        view.cols(),
        (&mut cell.overlay_mem[set][..], cell.records),
    );
    cell.records.extend_from_slice(renderer.splash());
    cell.records.extend_from_slice(renderer.scrollbar());
    cell.records.extend_from_slice(renderer.screen_fade());
    let chrome = Tally::of(cell.records.len() - chrome_from);
    used.clear();
    used.extend((0..view.rows().index()).map(|y| view.extent(Row::from_index(y))));
    Some(CellFrame {
        at,
        built,
        cols: view.cols(),
        rows: view.rows(),
        ring: Rows::of_index(vrows + lined + noticed + usize::from(sliding)),
        cursor,
        walked: Tally::of(walked),
        moving,
        chrome,
    })
}
