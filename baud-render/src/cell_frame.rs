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

use crate::screens::{Built, Compare, Screens, SetIx, Taking};
use crate::{
    Candidate, Cold, Composition, GlyphMap, Highlight, Instance, Leaving, Lent, Overlay, Renderer,
    Resolved, Scene, StyleStamp, Tally, WalkKey,
};
use baud_domain::{
    CellFlags, Codepoint, Col, Cols, Content, CursorStyle, PackedCell, Row, RowExtent, RowRot,
    Rows, Seal,
};
use baud_domain::{FrameDamage, TermRead};
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
    pub lent_cells: &'a mut Vec<PackedCell>,
    pub lent_used: &'a mut Vec<RowExtent>,
}

/// The words of the cell shaders' uniform `CU`, 152 bytes: four bytes each,
/// every pair a `uint2` whose `.x` comes first.
pub const CU_WORDS: usize = 38;

/// **`CU`'s words, in the order every cell shader declares them**, each
/// GPU path's uniform written by these names: a pair's halves as `.x` and
/// `.y`. Tested against the Metal declaration and the Vulkan one, so the
/// two cannot fall out of step with each other or with this list.
pub const CU_NAMES: [&str; CU_WORDS] = [
    "viewport.x",
    "viewport.y",
    "cell.x",
    "cell.y",
    "grid.x",
    "grid.y",
    "sides.x",
    "sides.y",
    "cursor.x",
    "cursor.y",
    "cursor_kind",
    "baseline",
    "underline",
    "strike",
    "thickness",
    "map_slots",
    "map_shift",
    "stride",
    "rot",
    "ring",
    "sel_on",
    "sel_from_col",
    "sel_from_row",
    "sel_to_col",
    "sel_to_row",
    "pan",
    "slide_rows",
    "cursor_color",
    "blink_hidden",
    "cursor_alpha",
    "sel_bg",
    "sel_fg",
    "sel_bg_curve",
    "sel_fg_curve",
    "cursor_text",
    "cursor_text_curve",
    "blink_faint",
    "cursor_side",
];

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
    /// **The slot of the ring draw row 0 is at**: draw row `d` is at
    /// `(rot + d) % ring` of the set's copy, `stride` cells a row.
    pub rot: RowRot,
    /// The cursor drawn, if its phase draws one.
    pub cursor: Option<(Row, Col, CursorStyle)>,
    /// How many of the records the overlay's walk wrote.
    pub walked: Tally,
    /// Which records are the trail's and the blink animation's.
    pub moving: (usize, usize),
    /// How many records at the end are baud's own overlays.
    pub chrome: Tally,
    /// Whether the build drew into the atlas.
    pub cold: Cold,
}

/// **Builds the cell frame** into the set `building` names, once the
/// window is laid out; none, the layout done, when `building` names none.
/// `used` is left holding each row's mark as the view read it, at the
/// row's slot in the set's ring ([`CellFrame::rot`]), where the set's copy
/// holds the row's cells.
///
/// **The cell path's frame, its damage read here**: [`build_with`] with
/// [`Renderer::damage`] of `term` against the last frame built, for a
/// driver that does not hand the damage in.
#[allow(clippy::too_many_arguments)]
pub fn build<P>(
    renderer: &mut Renderer<'_, '_>,
    term: &impl TermRead,
    seal: Seal,
    inputs: &CellInputs<'_, P>,
    cell: &mut CellParts<'_, '_, '_>,
    used: &mut Vec<RowExtent>,
    building: impl FnOnce() -> Option<SetIx>,
) -> Option<CellFrame>
where
    P: Iterator<Item = (Row, Col, Codepoint)> + DoubleEndedIterator + Clone,
{
    let damage = renderer.damage(term, inputs.back);
    build_with(renderer, term, seal, &damage, inputs, cell, used, building)
}

/// **The screen as it stands, made ready for the cell path** off
/// `term`'s window `back` lines up under `seal`: the CPU path's
/// [`Renderer::prepare`], which puts the glyphs the screen shows in the
/// atlas and its runs in the shaper's caches, then the map walked over
/// the window, so its entries hold them. No set is written, so a frame in
/// flight reads what it read; the next build draws every pixel. On a
/// terminal of more bands than one the map is left to the build, which
/// gives each band its own slots.
pub fn prepare(
    renderer: &mut Renderer<'_, '_>,
    term: &impl TermRead,
    seal: Seal,
    back: Rows,
    cell: &mut CellParts<'_, '_, '_>,
) {
    renderer.prepare(term, seal, back);
    if term.styles().bands().len() == 1 {
        renderer.walk_map(term, back, cell.map, cell.candidates);
    }
}

/// **`seal` is the one the driver closed this frame's writes with**
/// (`Term::seal_writes`), taken once a frame before the build: the set's
/// copy and the sized lines' keys are recorded under it. A required
/// argument, so no driver builds a frame without sealing first; `term` is
/// shared, and the build itself seals nothing.
///
/// **`damage` is what the frame draws again of the last one built**
/// ([`Renderer::damage`]): a [`FrameDamage::Whole`] is built as a screen
/// of its own, its map walked again. Either way the set takes the rows
/// written since its own copy, which the damage since the last frame is a
/// part of.
#[allow(clippy::too_many_arguments)]
pub fn build_with<P>(
    renderer: &mut Renderer<'_, '_>,
    term: &impl TermRead,
    seal: Seal,
    damage: &FrameDamage,
    inputs: &CellInputs<'_, P>,
    cell: &mut CellParts<'_, '_, '_>,
    used: &mut Vec<RowExtent>,
    building: impl FnOnce() -> Option<SetIx>,
) -> Option<CellFrame>
where
    P: Iterator<Item = (Row, Col, Codepoint)> + DoubleEndedIterator + Clone,
{
    let work = crate::renderer::atlas_work(&renderer.atlas_stats());
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
    // With BDSM reset and every row's joins to find again
    // (`crate::Rejoin::Every`), a frame that returns below before its
    // rejoin leaves that answer owed, so the next frame's is `Every`
    // again (`crate::Reorder::rejoined`).
    let rejoin = renderer.lay_out_bidi_copied(term, back);
    // Which buffers this frame may write: any set no frame in
    // flight was committed with. `None` would mean every one is
    // held and there is nowhere to build that a live draw is not
    // reading -- which cannot happen while one frame is in
    // flight and three sets exist, and is a refusal rather than
    // a corruption if it ever does.
    let at = building()?;
    let set = at.0;
    // **The window in drawn order, the renderer's own, lent by move with
    // its key** once the frame has a set to build into, and put back once
    // it is built: each row permuted again only where it was laid out or
    // written since the last frame drew it (`crate::Orders::window_kept`).
    let mut drawn = renderer.take_drawn_order();
    let view = renderer.bidi().window_kept(term, back, &mut drawn.window);
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
    // **The ids this set's copy was lent, put back**, and the
    // slots this set lent its last frame's frozen and struck
    // cells after them: no frame in flight reads this set, so
    // its copy is the screen it was copied from again, and a
    // set that holds the screen need not copy it afresh.
    cell.lent[set].restore_ringed(cell.copies_mem[set], (term.stride(), cell.screens.ring(at)));
    renderer.give_back(
        &mut cell.styles_mem[set][..],
        &mut cell.style_stamps[set],
        &mut cell.lent[set],
    );
    // The set's resolved table, brought up to date from the core's
    // log: only the slots written since this set was last built,
    // which on a steady screen is none. Before the map
    // walk, which reads each cell's face style from it. On a terminal of
    // more bands than one, each band the frame draws in a slot of the
    // set's own, the leaving row's first while sliding
    // (`Renderer::cell_bases`).
    let banded = term.styles().bands().len() > 1;
    match banded {
        false => {
            renderer.sync_styles(
                term,
                &mut cell.styles_mem[set][..],
                &mut cell.style_stamps[set],
            );
        }
        true => {
            let leaving_runs = sliding.then(|| renderer.leaving_runs(term, back));
            // The struck links' cells, as the copy's lending names them: a
            // row holding one is a line of its own.
            let found =
                renderer.take_struck(term, back, &view, view.rows(), crate::CopyOrder::Stored);
            let struck = found.cells().to_vec();
            renderer.put_struck(found);
            renderer.sync_cell_window(
                term,
                (back, at),
                leaving_runs,
                &struck,
                (&mut cell.styles_mem[set][..], &mut cell.style_stamps[set]),
            );
        }
    }
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
    // window's in the renderer's maps, kept with its key as the window is.
    let status = term.status_line().map(|line| {
        renderer
            .bidi()
            .status_kept(term, Row::from_index(vrows), line, &mut drawn.status)
    });
    let lined = usize::from(status.is_some());
    // **The config's notice row, after the status line's**, which the
    // set's ring holds too: shown while the copy has room for it.
    let noticed = match renderer.notice_shown() {
        Some(_) => usize::from(cell.copies_mem[set].len() >= (vrows + lined) * stride + vcols),
        None => 0,
    };
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
    // The maps the window was drawn through: which rows they laid out
    // again since each set's copy, the status line's after the window's,
    // and the display they re-broke.
    let built = screens.take_below(
        cell.copies_mem,
        at,
        &Taking {
            term,
            seal,
            back,
            view: &view,
            status: status.as_ref(),
            orders: renderer.bidi(),
            stride: term.stride(),
            leaving,
        },
        undo,
        Rows::of_count(noticed),
    );
    // **Where the set's copy holds each drawn row** for every read and
    // write below and for the shader: draw row `d` at `(rot + d) % ring`.
    let ring = screens.ring(at);
    // **A frozen row of the window drawn in its own renditions,
    // and a struck link's text in grey** (`crate::Lent`):
    // its copied cells given slots no row of the frame names, the
    // frozen first, and the walk and the overlay reading the
    // copy, so all three resolve each id in the one table. A
    // frame with neither asks one test a row.
    // The struck cells are kept while nothing they are found from moved
    // (`crate::StruckCells`), and the copy is lent in place, on buffers
    // the set's lending keeps: a caret frame over a struck link allocates
    // nothing for it.
    let struck = renderer.take_struck(term, back, &view, view.rows(), crate::CopyOrder::Stored);
    let lend = match frozen || !struck.cells().is_empty() {
        false => crate::Lend::Nothing,
        true => {
            // Each window row through the table each of its cells
            // names, in the order the copy holds them; the row
            // after the window's the leaving row while sliding,
            // else the status line's, and the notice's after it.
            // With a display held a window row names runs, made as
            // the layout finds them, once here; with none each row
            // is one table, asked as the lending reads it.
            let display = frozen && renderer.bidi().display().is_some();
            let sources: Vec<crate::RowSource<'_>> = match display {
                true => (0..vrows)
                    .map(|y| renderer.bidi().row_source(term, back, Row::from_index(y)))
                    .collect(),
                false => Vec::new(),
            };
            let after = leaving_source
                .as_ref()
                .map_or(crate::Words::LIVE, crate::RowSource::words);
            let words = match (frozen, display) {
                (false, _) => crate::frozen::CopyWords::Live,
                (true, false) => crate::frozen::CopyWords::Window {
                    back,
                    window: view.rows(),
                    after,
                },
                (true, true) => crate::frozen::CopyWords::Each {
                    rows: &sources,
                    after,
                },
            };
            let copy = &mut cell.copies_mem[set][..];
            let rows = Rows::of_count(copy.len().div_ceil(stride.max(1)).min(vrows + 2));
            let mut copy =
                crate::frozen::FlatCopy::new(copy, (term.stride(), ring), view.cols(), rows, words);
            renderer.lend_rows(
                term,
                &mut copy,
                struck.cells(),
                &mut cell.styles_mem[set][..],
                &mut cell.style_stamps[set],
                &mut cell.lent[set],
            )
        }
    };
    renderer.put_struck(struck);
    // **A display line joining two bands**, numbered its own in the set's
    // copy (`crate::band_window::Joined`), each rewrite kept with the
    // set's lending, so its next build puts it back and a compare reads
    // through it, and the line's entries of the loose region filled from
    // its bands' slots.
    if banded {
        let pairs = number_joined(
            renderer,
            term,
            (
                &mut cell.copies_mem[set][..],
                &mut cell.lent[set],
                &mut cell.styles_mem[set][..],
            ),
            (at, view.cols(), ring),
            (Rows::of_count(usize::from(sliding)), view.rows()),
        );
        renderer.keep_line_pairs(at, pairs);
    }
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
    // A frame drawn whole is a screen of its own.
    let built = match (damage, built) {
        (FrameDamage::Whole(_), Built::Last) => {
            screens.built(Built::New);
            Built::New
        }
        (_, built) => built,
    };
    screens.lending = lending;
    // **The leaving row's map ensured**, over the row as copied
    // and lent: before the screen's walk below, which clears the
    // candidates and refills them for the view it walks.
    if sliding {
        let at = ring.slot(Row::from_index(vrows)).index() * stride;
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
            renderer.leaving_table(term),
        );
        cell.exceptions.ensure_leaving(
            cell.map,
            term,
            leaving,
            &cell.styles_mem[set][..],
            renderer,
        );
    }
    // **The set's rows in the overlay's order** (`Renderer::overlay_rows`).
    let overlay = renderer.overlay_rows(term, at, leaving);
    // The copy the walk reads: the set's, where its lending rewrote ids or
    // a row is a line of its own (`crate::band_window::Joined`), whose ids
    // the copy numbered.
    let own = overlay.any_own();
    let view = match (&lend, own) {
        (crate::Lend::Nothing, false) => view,
        _ => {
            cell.lent_cells.clear();
            for y in 0..vrows {
                let at = ring.slot(Row::from_index(y)).index() * stride;
                cell.lent_cells
                    .extend_from_slice(&cell.copies_mem[set][at..at + stride]);
            }
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
            match own {
                true => overlay.view(
                    term,
                    (cell.lent_cells, cell.lent_used),
                    term.stride(),
                    (view.cols(), view.rows()),
                    (view.rows(), RowRot::NONE),
                ),
                false => baud_domain::Viewport::new(
                    cell.lent_cells,
                    cell.lent_used,
                    term.stride(),
                    view.cols(),
                    view.rows(),
                    view.rows(),
                    RowRot::NONE,
                    crate::frozen::live_table(term),
                ),
            }
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
    // **The rows the overlay finds pictures on again**: the rows that may
    // differ from the frame built before this one and the rows the damage
    // names, as the map's walk reads them; every row for a frame drawn
    // whole, one that slides, or the first after one that slid.
    let taken = match (damage, screens.differ(), screens.walked, sliding) {
        (FrameDamage::Rows { rows, .. }, Some(differ), Some(_), false) => Some(rows.union(differ)),
        _ => None,
    };
    if built == Built::New || screens.walked != Some(key) {
        // **The rows the walk reads**: the rows that may differ from the
        // frame built before this one, whose screen is the one the last
        // walk read, and the rows the damage names; every row for a frame
        // drawn whole, one that slides, or the first after one that slid,
        // whose walk read another window.
        let walk = match (damage, screens.differ(), screens.walked, sliding) {
            (
                FrameDamage::Rows {
                    rows,
                    cursor,
                    overlays,
                },
                Some(differ),
                Some(_),
                false,
            ) => FrameDamage::Rows {
                rows: rows.union(differ),
                cursor: *cursor,
                overlays: *overlays,
            },
            _ => crate::drawn::TOLD,
        };
        cell.map.ensure_with(
            term,
            view,
            &walk,
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
    if noticed == 1 {
        let at = ring.slot(Row::from_index(vrows + lined)).index() * stride;
        if let Some(row) = cell.copies_mem[set].get_mut(at..at + vcols) {
            row.fill(PackedCell::EMPTY);
        }
    }
    cell.exceptions.ensure_framed(
        &Scene {
            term,
            view,
            styles: overlay.tables(&cell.styles_mem[set][..]),
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
        (crate::renderer::Framed::of(back, seal), taken),
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
                overlay.tables(&cell.styles_mem[set][..]),
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
            (cell.map, overlay.tables(&cell.styles_mem[set][..])),
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
    renderer.animate(cursor, crate::drawn::Arm::Cell);
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
    // Each row's mark at its slot in the set's ring, where the copy holds
    // its cells: row `d` at `(head + d) % ring`.
    let kept = Rows::of_index(vrows + lined + noticed + usize::from(sliding));
    used.clear();
    used.resize(kept.index().max(view.rows().index()), RowExtent::default());
    for y in 0..view.rows().index() {
        used[ring.head().advance(y, kept).index()] = view.extent(Row::from_index(y));
    }
    let frame = CellFrame {
        at,
        built,
        cols: view.cols(),
        rows: view.rows(),
        ring: kept,
        rot: ring.head(),
        cursor,
        walked: Tally::of(walked),
        moving,
        chrome,
        cold: Cold::of(work, crate::renderer::atlas_work(&renderer.atlas_stats())),
    };
    renderer.put_drawn_order(drawn);
    renderer.drew(term, seal, back, crate::drawn::Arm::Cell);
    Some(frame)
}

/// **Each display line of set `set`'s frame that joins two bands, given
/// ids of its own in the set's copy**, `cols` cells a row, its window row
/// `slide` draw rows below its draw row: each logical column found where
/// the row's order draws it, its rewrite kept in the set's lending, and the
/// line's pairs resolved into the loose region from the set's slots. Each
/// line's pairs, by the window's order of its lines: what a reader of the
/// copy names the line's ids by.
fn number_joined(
    renderer: &Renderer<'_, '_>,
    term: &impl TermRead,
    (copy, lent, styles): (&mut [PackedCell], &mut Lent, &mut [Resolved]),
    (set, cols, ring): (SetIx, Cols, crate::screens::Ring),
    (slide, rows): (Rows, Rows),
) -> Vec<Vec<crate::band_window::Pair>> {
    let Some(window) = renderer.cell_window(set) else {
        return Vec::new();
    };
    let mut kept = Vec::new();
    let (stride, cols) = (term.stride().index(), cols.index());
    let bs = term.styles().band_slots();
    let orders = renderer.bidi();
    let (mut pairs, mut greys) = (Vec::new(), Vec::new());
    for j in window.all_joined() {
        // A window row is copied at its row, its runs logical; the leaving
        // row, draw row 0 while sliding, after the window's, its runs in
        // the order it is drawn already.
        let (y, logical) = match j.row.index().checked_sub(slide.index()) {
            Some(y) => (y, true),
            None => (rows.index(), false),
        };
        let at = ring.slot(Row::from_index(y)).index() * stride;
        let Some(row) = copy.get_mut(at..at + cols) else {
            kept.push(Vec::new());
            continue;
        };
        let r = Row::from_index(y);
        let at = |l: Col| match logical {
            true => orders.visual_col(r, l),
            false => l,
        };
        // A struck column is the copy's, as its lending names it.
        j.rewrite_drawn(row, (at, at), &mut pairs, |x, was| lent.rewrote(r, x, was));
        greys.clear();
        greys.extend(pairs.iter().map(|&(b, id, s)| match s {
            crate::band_window::Strike::Yes => Some(renderer.struck_resolved(term, b, id)),
            crate::band_window::Strike::No => None,
        }));
        j.fill((&pairs, &greys), (window, bs), styles);
        kept.push(pairs.clone());
    }
    kept
}
