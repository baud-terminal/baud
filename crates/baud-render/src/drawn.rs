//! **What the last frame was drawn from**, kept by every build, and the
//! terminal read against it: the frame's damage
//! ([`crate::Renderer::damage`]), every pixel for a reason
//! [`WholeWhy`] names, else the rows written since, the cursor as it
//! changed and the overlays set otherwise since.

use baud_domain::{
    ClusterEpoch, Col, ColorEpoch, Cols, CursorChange, CursorStyle, DirtyRows, FrameDamage,
    HistoryErasures, HistoryWipes, OverlayChanges, Reclaims, Resets, Row, Rows, ScreenEpoch, Seal,
    TermRead, WholeWhy, WindowAt,
};
use baud_font::Px;

use crate::Argb;

/// Which path built a frame: the CPU path's convert, or the cell path's
/// build (`crate::cell_frame::build`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Arm {
    Cpu,
    Cell,
}

/// The cursor as the terminal places it: its cell and style while it is
/// shown, none while it is hidden.
type TermCursor = Option<(Row, Col, CursorStyle)>;

/// **What a buffer holds around the rows**: the surface lent, its size
/// and the whole frame's top left corner in it (none while the surface is
/// the whole frame), and the colour the margin around the rows is painted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Around {
    pub(crate) surface: Option<((Px, Px), (Px, Px))>,
    pub(crate) margin: Argb,
}

/// **The terminal as the last frame was built from it**: the seal its
/// writes were closed under, the window, the geometry, every epoch that
/// moves what an unwritten cell draws, the cursor, and the path that built
/// it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DrawnKey {
    seal: Seal,
    at: WindowAt,
    grid: (Cols, Rows),
    status: Rows,
    screen: ScreenEpoch,
    resets: Resets,
    erasures: HistoryErasures,
    wipes: HistoryWipes,
    colours: ColorEpoch,
    clusters: ClusterEpoch,
    reclaims: Reclaims,
    cursor: TermCursor,
    around: Around,
    pub(crate) arm: Arm,
}

/// The cursor `term` places, none while it is hidden.
fn cursor_of(term: &impl TermRead) -> TermCursor {
    term.cursor_visible().then(|| {
        let (r, c) = term.cursor();
        (r, c, term.cursor_style())
    })
}

impl DrawnKey {
    /// **The key of a frame built now** from `term`'s window `back` lines
    /// up, under `seal`, by `arm`, into a buffer holding `around`.
    pub(crate) fn of(
        term: &impl TermRead,
        seal: Seal,
        back: Rows,
        arm: Arm,
        around: Around,
    ) -> DrawnKey {
        DrawnKey {
            seal,
            at: term.row_gens(term.looking_back(back)).window(),
            grid: term.geometry(),
            status: term.status_rows(),
            screen: term.screen_epoch(),
            resets: term.resets(),
            erasures: term.history_erasures(),
            wipes: term.history_wipes(),
            colours: term.color_epoch(),
            clusters: term.cluster_epoch(),
            reclaims: term.style_reclaims(),
            cursor: cursor_of(term),
            around,
            arm,
        }
    }
}

/// **What a frame of `term`'s window `back` lines up must draw again of
/// the frame `key` was built from**, `overlays` set otherwise since, into
/// a buffer holding `around`: every pixel when there is no such frame,
/// when the geometry, the screen, a reset, an erasure of the history or
/// the saved lines, a colour, a cluster or a rendition moved what an
/// unwritten cell draws, or when the surface or the margin's colour is
/// another;
/// every row of the window when the window shows other rows; else the
/// rows written after its seal, the status line's row after the main
/// display's when its own generation says so.
pub(crate) fn damage(
    key: Option<&DrawnKey>,
    term: &impl TermRead,
    back: Rows,
    overlays: OverlayChanges,
    around: Around,
) -> FrameDamage {
    let Some(k) = key else {
        return FrameDamage::Whole(WholeWhy::Requested);
    };
    let why = [
        (
            term.geometry() != k.grid || term.status_rows() != k.status,
            WholeWhy::Resized,
        ),
        (term.screen_epoch() != k.screen, WholeWhy::Screen),
        (term.resets() != k.resets, WholeWhy::Reset),
        (term.history_erasures() != k.erasures, WholeWhy::Erased),
        (term.history_wipes() != k.wipes, WholeWhy::Wiped),
        (term.color_epoch() != k.colours, WholeWhy::Colours),
        (term.cluster_epoch() != k.clusters, WholeWhy::Clusters),
        (
            term.style_reclaims().recolours_since(k.reclaims),
            WholeWhy::Renditions,
        ),
        (around != k.around, WholeWhy::Surface),
    ];
    if let Some(&(_, why)) = why.iter().find(|(moved, _)| *moved) {
        return FrameDamage::Whole(why);
    }
    let gens = term.row_gens(term.looking_back(back));
    let main = k.grid.1.index();
    let moved = gens.window() != k.at;
    let written = (0..main)
        .map(Row::from_index)
        .filter(|&y| moved || gens.after(y, &k.seal));
    let status = (k.status.index() > 0
        && (moved || term.status_gen().is_none_or(|g| g.after(&k.seal))))
    .then(|| Row::from_index(main));
    let Some(rows) = written
        .chain(status)
        .try_fold(DirtyRows::NONE, DirtyRows::with)
    else {
        return FrameDamage::Whole(WholeWhy::Overflow);
    };
    let cursor = match (cursor_of(term), k.cursor) {
        (now, was) if now == was => CursorChange::Same,
        (Some((r, c, _)), Some((wr, wc, _))) if (r, c) != (wr, wc) => CursorChange::Moved,
        _ => CursorChange::Restyled,
    };
    FrameDamage::Rows {
        rows,
        cursor,
        overlays,
    }
}

/// **Every pixel, asked for**: what a caller that draws a frame whole
/// hands the build.
pub(crate) const TOLD: FrameDamage = FrameDamage::Whole(WholeWhy::Requested);
