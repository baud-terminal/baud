//! The frame: rows that changed, converted and painted.

use crate::atlas::{Atlas, AtlasStats, Code, Kind, Tex};
use crate::cache::{
    CacheStats, CellIx, ClusterMemo, Cover, Key, PLACED_MAX, Placed, Question, RUN_CELLS, RunCache,
    Shaped,
};
use crate::caret::{self, CursorPhase, Play};
use crate::cellmap::{
    BlockCursor, CellSpan, Curves, Ground, Line, Resolved, Selected, Shapes, Swaps, Text,
    takes_stress_mark,
};
use crate::color::{Argb, Theme, Video};
use crate::composite::{Blend, Blit, Page, Target, Texels};
use crate::effect::Vfx;
use crate::flash::{self, Strength};
use crate::instance::{Instance, Screen};
use crate::metrics::{Metrics, PxSize};
use crate::overlay::Composition;
use crate::picture::Pictures;
use crate::scrollbar::{self, Bar, Scroll};
use crate::select::Highlight;
use crate::stack::FaceStyle;
use crate::stack::{EmojiDefault, FaceIx, FaceTable, Stack};
use crate::trail::{self, Drawn, Rect, Smear, TrailLook};
use crate::transition::{self, Transition, Veil};
use crate::units::{Band, Tally};
use baud_core::Term;
use baud_domain::contrast::CurveRow;
use baud_domain::{
    Attrs, Chan, ClusterEpoch, ClusterSlot, Codepoint, Col, Cols, CursorStyle, GlyphId, ImageAlpha,
    ImageDraw, ImageGen, ImageId, ImagePx, ImageStore, KittyId, PackedCell, Row, Rows, ZIndex,
};
use baud_font::bytes::Count;
use baud_font::{
    Cluster, Digest as LookupDigest, Face, Features, Glyph, Ignorable, LookupSet, MAX_LOOKUPS,
    Normalised, PerScript, Phase, Px, Scale, Script, ScriptTag, Shaper, Unresolved, VarPoint,
    Walked,
};
use std::time::{Duration, Instant};

/// Whether to trust the previous frame or repaint everything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Damage {
    /// Repaint the rows that differ from the last frame.
    Incremental,
    /// Repaint every row.
    Full,
}

/// The blink phase of a frame -- one half of the cycle a driver toggles off
/// its display link, the hardware vsync clock. A cell that blinks draws its
/// text on [`Blink::Shown`] and hides it on [`Blink::Hidden`]. Blink is a
/// *frame* property, not part of the interned rendition, so it is resolved
/// each frame here rather than stored in a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Blink {
    #[default]
    Shown,
    Hidden,
}

/// A frame's phase for each blink rate: `slow` for `SGR 5`
/// (`Attrs::BLINK`), `rapid` for `SGR 6` (`Attrs::RAPID_BLINK`). Two clocks
/// a driver runs off one display link, so each rate flips on its own and a
/// cell hides only in its own rate's off phase.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BlinkPhases {
    pub slow: Blink,
    pub rapid: Blink,
}

impl BlinkPhases {
    /// Both shown: what a frame draws when nothing blinks, and under Reduce
    /// Motion.
    pub const SHOWN: BlinkPhases = BlinkPhases {
        slow: Blink::Shown,
        rapid: Blink::Shown,
    };

    /// The blink shapes this frame hides: [`Shapes::BLINK`] in the slow off
    /// phase, [`Shapes::RAPID_BLINK`] in the rapid one. The cell shader's
    /// `blink_hidden` is this mask, so the GPU and CPU test one thing.
    pub fn hidden(self) -> Shapes {
        let slow = match self.slow {
            Blink::Shown => Shapes::default(),
            Blink::Hidden => Shapes::BLINK,
        };
        match self.rapid {
            Blink::Shown => slow,
            Blink::Hidden => slow.with(Shapes::RAPID_BLINK),
        }
    }
}

/// Whether this frame shows the video `?5` does not say: a visual bell whose
/// two halves no frame drew separately, which a driver sees through
/// `Term::reverse_flips` and holds for as long as the entry it advertised
/// asks. **It is the opposite of the mode, not reverse video**, so a flash
/// over a screen in reverse video shows it normal for the length, as xterm's
/// two repaints would. Like [`Blink`], a frame property the driver sets
/// before a convert or a resolve, and never the core's state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VideoFlash {
    #[default]
    Off,
    On,
}

/// Moves on every [`Renderer::set_theme`]: every resolved slot depends on
/// the theme.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct ThemeGen(u32);

/// What a resolved slot depends on besides the slot: the theme, the video --
/// `?5` or a flash's -- and the colours the core holds, as an epoch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct StyleInputs {
    theme: ThemeGen,
    video: Video,
    colors: baud_domain::ColorEpoch,
}

/// **Where one resolved copy of the rendition table stands**: which table it
/// resolved, how many of the table's writes, and under which inputs, and how
/// many of its slots blink. One a copy -- the CPU path's own, and one for each
/// of the cell path's buffer sets, since a set the GPU is reading cannot be
/// written and so falls behind the others by a frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct StyleStamp {
    table: Option<baud_domain::TableSerial>,
    at: baud_domain::StyleWrites,
    inputs: Option<StyleInputs>,
    blinking: Tally,
    /// What the core had reclaimed when this copy was stamped.
    reclaims: Option<baud_domain::Reclaims>,
}

/// How a sync resolved a copy: the slots written since it was stamped, or
/// every live slot, because the table or an input changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Pass {
    #[default]
    Replay,
    Full,
}

/// Whether the table reclaimed a slot since the copy was stamped -- evicted a
/// slot a cell may name, or swept one free that a later write may give
/// another rendition where the last frame showed the old one. Either way the
/// cell's word is the same and its colour is not, so row damage cannot see
/// it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Evicted {
    #[default]
    No,
    Yes,
}

/// What one [`Renderer::sync_styles`] did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StyleSync {
    /// Slots resolved: none on a frame in which nothing was minted.
    pub resolved: Tally,
    pub pass: Pass,
    pub evicted: Evicted,
}

/// Which blink rates the last convert or resolve saw on screen -- a cell
/// with `Attrs::BLINK`, one with `Attrs::RAPID_BLINK`, both or neither -- so
/// a driver repaints on a rate's phase flip only when text at that rate is
/// on screen, and a slow-only screen does not repaint at the rapid rate.
/// The blink shapes seen.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Blinking(Shapes);

impl Blinking {
    pub const UNSEEN: Blinking = Blinking(Shapes(0));
    pub const SLOW: Blinking = Blinking(Shapes::BLINK);
    pub const RAPID: Blinking = Blinking(Shapes::RAPID_BLINK);
    pub const BOTH: Blinking = Blinking(Shapes::BLINKS);

    /// The rates a resolved slot with `shapes` blinks at.
    pub fn of(shapes: Shapes) -> Blinking {
        Blinking(Shapes(shapes.0 & Shapes::BLINKS.0))
    }

    pub fn with(self, o: Blinking) -> Blinking {
        Blinking(self.0.with(o.0))
    }

    /// Whether going from `from` to `to` changes what a cell on screen
    /// draws: a rate whose phase flipped and whose text is on screen.
    pub fn flips(self, from: BlinkPhases, to: BlinkPhases) -> bool {
        (from.hidden().0 ^ to.hidden().0) & self.0.0 != 0
    }
}

/// What [`Renderer::convert`] did, handed to [`Renderer::paint`] or to a
/// GPU path.
#[derive(Clone, Copy, Debug, Default)]
pub struct Converted {
    pub rows_dirty: Tally,
    /// Of the dirty rows, those under a partly transparent image, which are
    /// repainted on every frame it is on (`ImageAlpha::Partial`).
    pub rows_partial: Tally,
    pub rows: Tally,
    pub convert: Duration,
    pub exceptions: Exceptions,
}

/// What the converted rows held that one glyph a cell cannot draw, and
/// what finding it would cost a path that shapes only where it must. A
/// count, taken on the path that shapes every run, of the cells the cell
/// shader gets wrong and of the cells a per-face prefilter would send to
/// the shaper to find them:
///
/// * `differs`: cells whose shaped glyphs are not what the cell shader's
///   map gives for the codepoint alone -- a different glyph, a different
///   offset, more than one glyph or none. This is the cells the shader
///   draws wrong, and it is counted only under the `exceptions` feature,
///   because it keeps a memo of the single-codepoint answer per codepoint.
/// * `spanning`: cells covered by a glyph that spans more than its own
///   cluster -- a ligature drawn as one wide glyph.
/// * `marked`: cells carrying attached codepoints in the cluster store.
/// * `starters`: cells whose glyph some `GSUB` lookup the grid features
///   name can start on -- the face's own first-position coverage, so a
///   font with unusual lookups is covered by construction.
/// * `shaped`: cells of the runs holding at least one starter, which is
///   what a prefiltered path would shape each frame.
/// * `morx`: cells on a face with a `morx` table and no `GSUB`, which
///   has no coverage to prefilter on.
#[derive(Clone, Copy, Debug, Default)]
pub struct Exceptions {
    pub differs: Tally,
    pub spanning: Tally,
    pub marked: Tally,
    pub starters: Tally,
    pub shaped: Tally,
    pub morx: Tally,
    /// Cells of the runs holding an adjacent glyph pair the face's rules
    /// can begin on -- what a pair-prefiltered path would shape a frame.
    pub bigram: Tally,
}

/// The glyph pairs a face's grid features can begin a substitution on:
/// for a first glyph, a digest of what may follow it, derived from the
/// tables by `Gsub::pairs`. `admits` says whether two adjacent glyphs could
/// start a match; a run with no admitted pair shapes to what its cells
/// shape to alone.
#[derive(Clone, Default)]
pub struct Pairs {
    /// By first glyph: the digest of what may follow it, `None` for a
    /// glyph no rule begins on. Indexed rather than hashed: the walk asks
    /// once per adjacent pair whose entries carry both bits.
    after: Vec<Option<LookupDigest>>,
    /// Every glyph that may follow some first: the second bit's test.
    seconds: LookupDigest,
    walked: Option<Walked>,
}

/// Whether a glyph can begin a pair, end one, both or neither -- the two
/// bits the glyph map carries per entry so the walk consults the digests
/// only where both are set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairRole {
    Neither,
    Starts,
    Ends,
    Both,
}

impl Pairs {
    fn of(face: &Face<'_>) -> Pairs {
        let mut after: Vec<Option<LookupDigest>> = Vec::new();
        let mut seconds = LookupDigest::EMPTY;
        let walked = face.gsub().map(|gsub| {
            gsub.pairs(Features::GRID, &mut |first, d| {
                let i = first.index();
                if after.len() <= i {
                    after.resize(i + 1, None);
                }
                let slot = after[i].get_or_insert(LookupDigest::EMPTY);
                *slot = slot.union(d);
                seconds = seconds.union(d);
            })
        });
        Pairs {
            after,
            seconds,
            walked,
        }
    }

    pub fn role(&self, g: GlyphId) -> PairRole {
        let (starts, ends) = match self.walked {
            None => (false, false),
            Some(Walked::Unreadable) => (true, true),
            Some(Walked::Derived) => (
                self.after.get(g.index()).is_some_and(Option::is_some),
                self.seconds.may_have(LookupDigest::of(g)),
            ),
        };
        match (starts, ends) {
            (false, false) => PairRole::Neither,
            (true, false) => PairRole::Starts,
            (false, true) => PairRole::Ends,
            (true, true) => PairRole::Both,
        }
    }

    pub fn admits(&self, a: GlyphId, b: GlyphId) -> bool {
        match self.walked {
            None => false,
            Some(Walked::Unreadable) => true,
            Some(Walked::Derived) => self
                .after
                .get(a.index())
                .and_then(Option::as_ref)
                .is_some_and(|d| d.may_have(LookupDigest::of(b))),
        }
    }

    /// How many first glyphs the tables name.
    pub fn firsts(&self) -> Tally {
        Tally::of(self.after.iter().filter(|d| d.is_some()).count())
    }
}

/// How a face's substitutions can be prefiltered: a count behind the
/// `exceptions` feature, kept because the bench reports it.
#[cfg(feature = "exceptions")]
#[derive(Clone, Copy)]
enum Starter {
    /// The union of the face's lookup digests: a glyph outside it starts
    /// no substitution.
    Gsub(LookupDigest),
    /// A `morx` face: no coverage to ask.
    Morx,
    /// No substitution table at all.
    Plain,
}

/// One glyph of a shaped cluster: its atlas slot and the offsets, in
/// pixels, the shaper placed it at relative to the cell's origin.
#[derive(Clone, Copy, Debug)]
pub struct ClusterGlyph {
    pub id: GlyphId,
    /// Where the glyph comes from, so a smoothed look can draw the same
    /// glyph again for another text colour.
    pub source: GlyphSource,
    /// `None` for a glyph with no ink -- a space, a spacer a ligature
    /// leaves behind -- which is still the glyph the cell draws.
    pub slot: Option<crate::atlas::Slot>,
    pub dx: i32,
    pub dy: i32,
    /// Where the cluster sits in its span, from its base: every glyph of a
    /// cluster takes its base's phase, so a mark moves with the letter.
    pub phase: Phase,
}

/// Whether `cp` is an icon codepoint -- the Private Use Areas -- whose
/// placement the Nerd Fonts patcher's rules own (note Z, scope C), not the
/// centring a text glyph takes.
fn is_icon(cp: Codepoint) -> bool {
    let v = cp.value();
    (0xE000..=0xF8FF).contains(&v) || v >= 0xF_0000
}

/// Where a cluster's glyph is drawn from: a face's outline or image, or
/// baud's own mask for box drawing, which no look changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlyphSource {
    Face(FaceIx),
    Procedural,
}

/// What a codepoint shapes to alone: the glyph the cell shader draws for
/// it, its atlas slot when it has ink, and its pair role.
#[derive(Clone, Copy, Debug)]
pub struct Single {
    pub slot: Option<crate::atlas::Slot>,
    pub glyph: GlyphId,
    pub role: PairRole,
    pub face: FaceIx,
}

/// What one frame cost and did.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub rows_dirty: Tally,
    /// Of the dirty rows, those under a partly transparent image, which are
    /// repainted on every frame it is on (`ImageAlpha::Partial`).
    pub rows_partial: Tally,
    pub rows: Tally,
    pub instances: Tally,
    /// Of the instances, those on the colour texture and those solid.
    pub colored: Tally,
    pub solids: Tally,
    pub cache: CacheStats,
    pub atlas: AtlasStats,
    pub convert: Duration,
    pub composite: Duration,
    pub exceptions: Exceptions,
}

/// The video a frame draws in: the one the screen's `?5` asks for, or under
/// a flash the other one.
fn video_of(term: &Term<'_>, flash: VideoFlash) -> Video {
    match (term.reverse_video(), flash) {
        (true, VideoFlash::Off) | (false, VideoFlash::On) => Video::Reverse,
        (false, VideoFlash::Off) | (true, VideoFlash::On) => Video::Normal,
    }
}

fn px_i32(p: Px) -> i32 {
    i32::try_from(p.value()).unwrap_or(i32::MAX)
}

/// **The most grounds a repeat pass's cap is read from**: more under it,
/// and it draws nothing. A cap over a text colour costs every mixture a
/// glyph edge draws, about 2.4 ms in release on this Mac, and a picture
/// holds thousands.
const FLASH_GROUNDS: usize = 64;

/// The caps scanned a frame, at most: about 5 ms.
const FLASH_SCANS: usize = 2;

/// The caps the cache holds before it is emptied between frames: several
/// screens of text.
const FLASH_CACHED: usize = 512;

/// Whether a cell under a repeat pass draws text: a character other than a
/// space, so its text colours are grounds too.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Inked {
    Yes,
    No,
}

/// The cursor on a cell under a repeat pass, as this frame draws it: a
/// block, which swaps the cell's colours, or a bar or an underline, a shape
/// in its own colour over them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cursored {
    No,
    Block,
    Shape,
}

/// **The desktops a dimmed page is read over**, white and black: note AS's
/// bound (section 3), where the glass material darkens nothing and the tint
/// is off, a flat backdrop that no blur can move. A translucent ground is
/// read as itself over each, the way the window server composites it.
const DESKTOPS: [Argb; 2] = [Argb(0xFFFF_FFFF), Argb(0xFF00_0000)];

/// A screen position from pixel arithmetic, pinned to the record's range.
fn screen(v: i32) -> Screen {
    Screen(i16::try_from(v).unwrap_or(i16::MAX))
}

/// A filled rectangle `w` by `h` at `(x, y)` in `color`.
fn solid(x: Screen, y: Screen, w: Px, h: Px, color: Argb) -> Instance {
    Instance {
        x,
        y,
        w: Tex(u16::try_from(w.value()).unwrap_or(u16::MAX)),
        h: Tex(u16::try_from(h.value()).unwrap_or(u16::MAX)),
        u: Tex(0),
        v: Tex(0),
        color,
        kind: Kind::Solid,
        curve: CurveRow::default(),
    }
}

/// The contrast curve row text painted in `c` takes: its encoded colour,
/// alpha aside (`baud_domain::contrast`). Called once per rendition slot.
fn curve_row(c: Argb) -> CurveRow {
    CurveRow::of(c.r().get(), c.g().get(), c.b().get())
}

/// One cell's input to the shaper: its base and attached codepoints.
struct ClusterIn {
    cps: Vec<Codepoint>,
    /// Offset of the cell within the run.
    at: CellIx,
}

/// Which lookups a script selects on one face, numbered: two scripts with
/// the same number shape alike there, since the lookups are all a script
/// changes in baud's shaper, so a run may cross between them. A run is cut
/// only where this changes, not wherever the script does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Selection(u8);

/// Whether the scripts select different lookups on a face at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Varies {
    /// Every script selects the same lookups: a run is never cut by
    /// script, and no row's scripts need settling for this face. The
    /// embedded text faces are this, their `DFLT`, `latn` and `hebr` naming
    /// the same lookups; STIX Two Math is not.
    No,
    Yes,
}

/// One face's [`Selection`] per script.
struct Selections {
    by_script: PerScript<Selection>,
    varies: Varies,
}

impl Selections {
    /// The `GSUB` and `GPOS` lookups each script selects on `face` under
    /// the grid features, numbered by first appearance. The lookups are
    /// gathered once per pair of Script tables chosen, not once per script:
    /// a face lists a handful of tables, and 175 scripts share them.
    fn of(face: &Face<'_>) -> Selections {
        type Chosen = (Option<ScriptTag>, Option<ScriptTag>);
        let mut tables: Vec<(Chosen, Selection)> = Vec::new();
        let mut seen: Vec<(Option<LookupSet>, Option<LookupSet>)> = Vec::new();
        let by_script = PerScript::from_fn(|s| {
            let chosen = (
                face.gsub().and_then(|g| g.chosen(s)),
                face.gpos().and_then(|g| g.chosen(s)),
            );
            if let Some(&(_, sel)) = tables.iter().find(|(c, _)| *c == chosen) {
                return sel;
            }
            let key = (
                face.gsub().map(|g| g.lookups(s, Features::GRID)),
                face.gpos().map(|g| g.lookups(s, Features::GRID)),
            );
            let i = match seen.iter().position(|k| *k == key) {
                Some(i) => i,
                None => {
                    seen.push(key);
                    seen.len() - 1
                }
            };
            let sel = Selection(u8::try_from(i).unwrap_or(u8::MAX));
            tables.push((chosen, sel));
            sel
        });
        let varies = match by_script.uniform() {
            true => Varies::No,
            false => Varies::Yes,
        };
        Selections { by_script, varies }
    }
}

/// What a cell contributes to its row's scripts before context settles
/// them: its base's script, or for a base that names none, its whole
/// cluster's (UAX #24 5.2), which reads the marks only then. A cell with
/// nothing drawn joins whatever is around it.
fn unresolved(term: &Term<'_>, cell: PackedCell) -> Unresolved {
    let Some(base) = drawn_base(term, cell) else {
        return Unresolved::BLANK;
    };
    match Unresolved::of(core::slice::from_ref(&base)) {
        Unresolved::Neutral { .. } if cell.flags.is_marked() => {
            let cps: Vec<Codepoint> = core::iter::once(base)
                .chain(term.cluster_marks(cell).filter_map(Codepoint::new))
                .collect();
            Unresolved::of(&cps)
        }
        u => u,
    }
}

/// **The character a cell draws**: its base ([`Term::base_of`]), and none
/// for a kitty Unicode placeholder, U+10EEEE, whose cell shows a slice of
/// an image or nothing (`[[source: kitty-graphics]]`, "Unicode
/// placeholders"): the protocol is the private agreement that gives this
/// private-use character its meaning, and it is never drawn as a glyph,
/// nor are its diacritics, as kitty's `font_for_cell` draws it with the
/// blank font (`[[source: kitty-fonts-c]]`). With no image to show, the
/// cell shows its background: Jev favoured BLANK over TUS 5.3's `.notdef`
/// at 0.80 and 0.84, under the bar, so it is the interim choice and an
/// open question. Every path that draws a glyph asks here; a copy asks
/// [`Term::base_of`] and gets the character back.
pub(crate) fn drawn_base(term: &Term<'_>, cell: PackedCell) -> Option<Codepoint> {
    term.base_of(cell)
        .as_codepoint()
        .filter(|c| !baud_domain::is_placeholder(*c))
}

/// Where the cursor is, when it is shown, and its shape.
type Cursor = Option<(Row, Col, CursorStyle)>;

/// Whether the previous frame's copy describes what is on screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
    /// Nothing painted yet, or the geometry changed: the next frame is full.
    Unknown,
    Known,
}

/// Whether a convert repaints the rows the frame's image placements cover.
/// The CPU path blits images over the rows it painted, so it needs them
/// repainted; a GPU path draws images in their own pass and does not.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ImageRows {
    Ignored,
    Repainted,
}

/// What [`Renderer::paint_rows`] draws between a repainted row's background
/// and its glyphs.
trait Under {
    fn paint(&self, r: &Renderer<'_, '_>, target: &mut Target<'_>, row: Row, page: Page);
}

/// Nothing: no placement is under the text.
struct NoUnder;

impl Under for NoUnder {
    #[inline(always)]
    fn paint(&self, _: &Renderer<'_, '_>, _: &mut Target<'_>, _: Row, _: Page) {}
}

/// The placements under the text, from the term's images.
struct UnderImages<'s, 'b>(&'s ImageStore<'b>);

impl Under for UnderImages<'_, '_> {
    fn paint(&self, r: &Renderer<'_, '_>, target: &mut Target<'_>, row: Row, page: Page) {
        r.paint_under(self.0, target, row, page);
    }
}

/// Whether a partly transparent placement over the text covers the band:
/// its rows are repainted every CPU frame, since its blit over its own last
/// blit would blend it twice. One under the text is blitted only into a
/// row being repainted, over the row's fresh background, so never twice.
fn partial_over(now: &[ImagePlacement], band: Band) -> bool {
    now.iter()
        .any(|p| p.alpha == ImageAlpha::Partial && p.layer == ImageLayer::Over && covers(p, band))
}

/// Whether a placement's pixel rows meet the band's.
fn covers(p: &ImagePlacement, band: Band) -> bool {
    let top = i64::from(p.y);
    top < i64::from(band.bottom.value()) && top + i64::from(p.h) > i64::from(band.top.value())
}

/// Whether the last CPU frame blitted a placement over the band that this
/// frame does not: an image left or moved from there, or another took its
/// rect. Whole placements are compared -- image, epoch and rect -- so a
/// different image in the same rect still counts. One that *arrives* needs no
/// repaint: its blit lands on the cells, which is what a repaint would give.
fn left_from(now: &[ImagePlacement], last: &[ImagePlacement], band: Band) -> bool {
    last.iter().any(|p| covers(p, band) && !now.contains(p))
}

/// Whether a placement under the text arrived on the band, or changed
/// there, since the last CPU frame. Its blit lands only in a row being
/// repainted, under the row's text, so a row it arrives on is repainted
/// where one over the text needs none.
fn arrived_under(now: &[ImagePlacement], last: &[ImagePlacement], band: Band) -> bool {
    now.iter()
        .any(|p| p.layer.under_text() && covers(p, band) && !last.contains(p))
}

/// Whether [`Renderer::set_images`] keeps a copy of each image's pixels for a
/// GPU to upload. The CPU path blits from the terminal's own pixels, so it
/// asks for placements alone.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Snapshots {
    Kept,
    Skipped,
}

/// **Which layer a placement draws in**: "Negative z-index values mean
/// that the images will be drawn under the text. This allows rendering of
/// text on top of images. Negative z-index values below INT32_MIN/2
/// (-1,073,741,824) will be drawn under cells with non-default background
/// colors" (`[[source: kitty-graphics]]`, "Controlling displayed image
/// layout"). Below, it is drawn over the default backgrounds and under
/// every other ([`crate::Ground`]), and under the text; under, over every
/// background and under the cells' glyphs, underlines and cursor; over,
/// after everything the row draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageLayer {
    Below,
    Under,
    Over,
}

impl ImageLayer {
    /// `INT32_MIN/2`: a `z` strictly below it is drawn under the
    /// non-default backgrounds.
    pub const BELOW_BACKGROUNDS: i32 = i32::MIN / 2;

    /// The layer a draw order puts a placement in.
    pub fn of(z: ZIndex) -> ImageLayer {
        match z.raw() {
            z if z < ImageLayer::BELOW_BACKGROUNDS => ImageLayer::Below,
            z if z < 0 => ImageLayer::Under,
            _ => ImageLayer::Over,
        }
    }

    /// Whether it is drawn under the text: below or under.
    pub fn under_text(self) -> bool {
        self != ImageLayer::Over
    }
}

/// **Where a placement's drawn pixels come from**: one image pixel a device
/// pixel from the texel at its visible rectangle's top-left, which the CPU
/// copies a row at a time; or through the placement's nearest-neighbour
/// tables, [`Renderer::image_map`], which every path reads when it is
/// scaled and the GPU paths read always.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageScale {
    Unit { x: ImagePx, y: ImagePx },
    Mapped,
}

/// Where a placement's tables start in [`Renderer::image_map`]'s store:
/// its `w` columns' texel x, then its `h` rows' texel y.
#[derive(Clone, Copy, Debug)]
pub struct MapAt(usize);

/// One visible image to draw: which image (and the store epoch it was seen
/// at, the GPU texture-cache key), where on screen at what size, in device
/// pixels -- the part of its drawn rectangle on the frame -- and in which
/// order. The origin is signed, so a partly-scrolled image hangs off the
/// top.
#[derive(Clone, Copy)]
pub struct ImagePlacement {
    /// Which stored image this draws.
    pub id: ImageId,
    /// The store epoch when it was snapshotted -- the cache key with `id`.
    pub epoch: ImageGen,
    /// The screen x of the top-left, device pixels, signed.
    pub x: i16,
    /// The screen y of the top-left, device pixels, signed.
    pub y: i16,
    /// The image width in device pixels.
    pub w: u16,
    /// The image height in device pixels.
    pub h: u16,
    /// Whether any of its pixels is partly transparent: the CPU path then
    /// repaints the rows under it on every frame.
    pub alpha: ImageAlpha,
    /// Its draw order, the kitty `z`; zero for a Sixel.
    pub z: ZIndex,
    /// Below the non-default backgrounds, under the text or over it, from
    /// `z`.
    pub layer: ImageLayer,
    /// The client's image id, which orders two placements at one `z`: "the
    /// image with the lower id is considered to have the lower z-index".
    /// None, ordered first, for a Sixel and a kitty image no id names.
    pub tie: KittyId,
    /// What it draws: the source rectangle, and the drawn one from its
    /// anchor cell, of which `x`, `y`, `w` and `h` are the visible part.
    pub draw: ImageDraw,
    /// One image pixel a device pixel, or through its tables.
    pub scale: ImageScale,
    /// Where its tables are.
    pub map: MapAt,
}

/// Two placements are one when they draw the same: everything but where
/// this frame's tables happen to sit.
impl PartialEq for ImagePlacement {
    fn eq(&self, o: &ImagePlacement) -> bool {
        (
            self.id, self.epoch, self.x, self.y, self.w, self.h, self.alpha, self.z, self.tie,
            self.draw, self.scale,
        ) == (
            o.id, o.epoch, o.x, o.y, o.w, o.h, o.alpha, o.z, o.tie, o.draw, o.scale,
        )
    }
}

impl Eq for ImagePlacement {}

/// One image's pixels to upload once: keyed by `id` and `epoch`, the store's
/// straight `0xAARRGGBB` words, `w * h` of them, row-major, unconverted. A
/// clear (unpainted Sixel) pixel is `0x00000000`. **Straight, not
/// premultiplied**: the fragment that samples the texture premultiplies
/// (`glyph_fragment`'s colour branch, `c.rgb * c.a`), the one premultiply on
/// the GPU paths, as `blend.over` is the CPU's.
pub struct ImageUpload {
    /// Which stored image these pixels are.
    pub id: ImageId,
    /// The store epoch -- the cache key with `id`.
    pub epoch: ImageGen,
    /// The image width in pixels.
    pub w: u16,
    /// The image height in pixels.
    pub h: u16,
    /// The straight pixels, `w * h`, row-major.
    pub pixels: Vec<u32>,
}

pub struct Renderer<'f, 'a> {
    stack: Stack<'f, 'a>,
    /// Which face draws each codepoint, per style and presentation
    /// (`Stack::table`): built here once, so `face_for` is a lookup.
    faces_of: FaceTable,
    /// Which face draws each marked cluster (`Stack::select_cluster`),
    /// remembered, so `face_for` stays a lookup for a cell with marks too.
    cluster_faces: ClusterMemo<FaceIx>,
    /// Whether a face draws all of a marked cluster, remembered, so a run
    /// the cache misses asks `shape` a lookup per marked cell too.
    cluster_covers: ClusterMemo<Cover>,
    scales: Vec<Scale>,
    /// Per face, a digest per `GSUB` lookup the grid features name, so a
    /// cache miss opens only the lookups that can touch the run.
    digests: Vec<Vec<LookupDigest>>,
    /// Per face, the prefilter the counts in [`Exceptions`] ask.
    #[cfg(feature = "exceptions")]
    starters: Vec<Starter>,
    /// Per face, the glyph pairs its rules can begin on.
    pairs: Vec<Pairs>,
    /// Per face, the longest sequence a rule can match, in glyphs; `None`
    /// for a face with no substitution rules or one whose tables could
    /// not be walked.
    extents: Vec<Option<Count>>,
    /// Per face, which lookups each script selects
    /// (`[[source: ms-opentype-chapter2]]`, ScriptList table), so a run is
    /// cut where they change.
    selections: Vec<Selections>,
    /// The row being converted, each cell's script settled in context
    /// (UAX #24, [`baud_font::resolve`]); filled only for a row with a run
    /// on a face whose scripts select different lookups.
    row_scripts: Vec<Script>,
    row_unresolved: Vec<Unresolved>,
    /// The faces by the code a map entry carries.
    faces: Vec<FaceIx>,
    /// The single-codepoint resolution per codepoint, memoised for the
    /// `differs` count: the face and glyph the cell shader's map would
    /// draw, with its offsets -- not an atlas slot, which a repack moves.
    #[cfg(feature = "exceptions")]
    singles: std::collections::HashMap<u32, Option<(FaceIx, Placed)>>,
    metrics: Metrics,
    theme: Theme,
    /// The video this frame draws in: the screen's `?5`, or under a flash the
    /// other one, read once a frame by `refresh_screen`.
    video: Video,
    /// The default foreground and background the screen draws this frame --
    /// the theme's, moved by `OSC 10`/`11` and swapped by the video. What is drawn
    /// "in the foreground" or "in the background" takes these: the margin,
    /// the scrollbar, a cursor trail and a screen fade.
    defaults: (Argb, Argb),
    /// The default background's colour as stored, under no exchange -- the
    /// theme's, or `OSC 11`'s -- read through [`Theme::defaults`] by
    /// `refresh_screen`: what a resolved slot's background is compared
    /// with for [`Shapes::DEFAULT_BG`] ([`Theme::default_bg`]). Under `?5`
    /// the default cells draw the foreground's colour, which is not it.
    page_default: Argb,
    blend: Blend,
    cache: RunCache,
    atlas: Atlas,
    /// How a character with no selector chooses between a text face and a
    /// colour one; the embedder's (`Renderer::set_emoji_default`).
    emoji: EmojiDefault,
    cols: Cols,
    rows: Rows,
    prev: Vec<PackedCell>,
    known: Prev,
    cur: Vec<PackedCell>,
    /// The cells of the frame being converted that belong to a picture
    /// drawn in text, whose backgrounds are drawn solid under the dim --
    /// [`Theme::finds_pictures`]. Found over the whole screen, because a
    /// picture is a property of a block of cells rather than of one.
    pictures: Pictures,
    /// **The CPU and instance paths' resolved rendition table**, kept from
    /// frame to frame and brought up to date by [`Renderer::sync_styles`]:
    /// only the slots the core wrote since, or every live slot when the table
    /// or an input changed. A cell's colours are one load from it.
    styles: Vec<Resolved>,
    styles_stamp: StyleStamp,
    /// Moves on every [`Renderer::set_theme`].
    theme_gen: ThemeGen,
    bg: Vec<Argb>,
    fg: Vec<Argb>,
    /// Per cell, the contrast curve row its glyph takes, beside the colour
    /// it is painted in: the slot's row for the side the swaps leave it on.
    curve: Vec<CurveRow>,
    /// Per cell, whether its text draws, beside its colours: the glyphs
    /// emitted for a hidden cell are dropped where they are emitted, so a
    /// run shapes the same in both blink phases.
    text: Vec<Text>,
    /// Per row, whether the last convert of it saw blinking text. A row a
    /// frame does not convert is unchanged, so its answer still holds, and
    /// `blink_seen` is the whole screen's.
    row_blinks: Vec<Blinking>,
    per_row: Vec<Vec<Instance>>,
    last_cursor: Cursor,
    /// The opacity the last converted frame drew its cursor at, so a fade
    /// that moves the cursor's opacity and nothing else still dirties its
    /// row.
    last_cursor_opacity: Chan,
    /// The selection on this frame's view, set by [`Renderer::set_highlight`]
    /// before a convert, and the one the last convert drew -- a row whose
    /// share of it changed is dirty, the cursor's own rule.
    highlight: Option<Highlight>,
    last_highlight: Option<Highlight>,
    /// The scrollbar on this frame and the last -- the knob, the track when
    /// widened, and how much of both is seen -- and its solids: a row whose
    /// slice of the bar changed is dirty (`Bar::slice`).
    bar: Option<Bar>,
    last_bar: Option<Bar>,
    bar_inst: Vec<Instance>,
    /// The pixels past the grid the frame keeps for a legacy scroller.
    gutter: Px,
    /// The cursor trail's ghost on this frame and the last, and its one
    /// solid instance: a row the ghost is over or was over is dirty, the
    /// scrollbar's rule, so the trail's damage stays bounded to the rows it
    /// sweeps rather than wiping the frame the way sfte's does.
    trail: Option<Drawn>,
    last_trail: Option<Drawn>,
    trail_inst: Vec<Instance>,
    /// An input method's composition on this frame and the last, and its
    /// records: the rows under either are dirty, and the cells under it are
    /// converted blank so its own glyphs replace theirs ([`Composition`]).
    composition: Option<Composition>,
    last_composition: Option<Composition>,
    compose_inst: Vec<Instance>,
    /// What the cursor is filled with, which a quad trail is drawn in: the
    /// application's `OSC 12` colour, else the default foreground a block
    /// cursor's reversed cell fills with.
    cursor_fill: Argb,
    /// **The most alpha a repeat pass's records carry** (note AL, option
    /// O2): the least [`flash::under_threshold`] of the cursor's colour over
    /// the backgrounds of the cells they cover, set by
    /// [`Renderer::bound_trail`] and read when the records are built.
    flash_cap: Chan,
    /// Why [`Renderer::flash_cap`] is what it is.
    flash_held: flash::Held,
    /// The caps already scanned, by cursor colour, text colour and page:
    /// a screen's worth, [`FLASH_CACHED`], emptied between frames when full.
    flash_caps: std::collections::HashMap<(Argb, Argb, Argb), Chan>,
    /// The alternate-screen fade's veil on this frame and the last, and its
    /// one full-screen solid: while it fades every row is dirty, because the
    /// veil covers the whole screen and the whole screen is what changed.
    fade: Option<Veil>,
    last_fade: Option<Veil>,
    fade_inst: Vec<Instance>,
    /// The cluster store's epoch when `prev` was taken. A cell naming a slot
    /// a sweep or a reset freed and another cluster took keeps its word, so
    /// a frame whose epoch moved repaints every row, as an eviction does.
    last_clusters: Option<ClusterEpoch>,
    /// This frame's blink phases, set by [`Renderer::set_blink`] before a
    /// convert or resolve, and which rates' blinking cells were seen -- so
    /// the driver repaints on a rate's phase flip only when text at that
    /// rate actually blinks.
    blink: BlinkPhases,
    blink_seen: Blinking,
    /// How many slots of the rendition table the last resolve found
    /// blinking. The cell path's walk tests a cell for blinking only while
    /// this is not zero.
    blink_slots: Tally,
    /// This frame's cursor blink phase, set by [`Renderer::set_cursor_blink`]:
    /// a phase of its own, because a blinking cursor keeps its own timing and
    /// restarts on input where `SGR 5` text keeps neither -- and a fade or
    /// the plasma glow part way through it ([`CursorPhase`]).
    cursor_blink: CursorPhase,
    /// The blink animation's solids -- the plasma glow, the fire, the ufo --
    /// where
    /// they are and the phase that drew them, on this frame and the last,
    /// and the solids: a row under either is dirty, the trail's rule, so
    /// their damage is the cursor's row.
    glow: Option<(Row, Col, CursorStyle, CursorPhase)>,
    last_glow: Option<(Row, Col, CursorStyle, CursorPhase)>,
    glow_inst: Vec<Instance>,
    /// Whether this frame shows the other video, set by
    /// [`Renderer::set_flash`] before a convert or resolve, the way the blink
    /// phase is.
    flash: VideoFlash,
    /// The visible image placements this frame, screen rects in device
    /// pixels, computed once by [`Renderer::set_images`] and consumed by
    /// both the CPU `paint_images` and the two GPU arms -- one description
    /// of where an image goes, not three.
    image_places: Vec<ImagePlacement>,
    /// Each placement's nearest-neighbour tables, at its [`MapAt`]: the
    /// texel x of every visible drawn column, then the texel y of every
    /// visible drawn row, from `baud_domain::ImageDraw::nearest`. Built
    /// with the placements; the CPU reads them for a scaled placement and
    /// the GPU paths for every one, so the paths sample one way.
    image_maps: Vec<u32>,
    /// The placements the last CPU frame drew, so the next one repaints the
    /// rows an image left as well as the rows it is on.
    last_image_places: Vec<ImagePlacement>,
    /// Each referenced image's pixels, straight, for a GPU to upload
    /// once. Keyed by id and store epoch; the CPU path reads the term's
    /// pixels directly instead. **Kept across frames** and refreshed only for a
    /// new `(id, epoch)`, so a static image is snapshotted once rather than
    /// rebuilt every frame -- a large image's per-frame allocation and
    /// per-pixel conversion, which the frame path would otherwise pay while it
    /// is on screen. Cleared whole when the store epoch moves. See
    /// [`Renderer::set_images`].
    image_uploads: Vec<ImageUpload>,
    /// The store epoch [`Renderer::image_uploads`] is snapshotted at; a change
    /// drops the whole snapshot cache, the [`baud_domain::ImageGen`] rule.
    image_epoch: ImageGen,
    /// How many image pixel-snapshots have been built, cumulative -- a static
    /// image bumps it once, not once a frame, which is what the gate checks.
    image_snapshots: Tally,
    /// How many codepoints were left out of a cluster before shaping because
    /// the face that keeps it lacks them, cumulative
    /// ([`Renderer::marks_unmapped`]).
    marks_unmapped: Tally,
    /// Glyph boxes the shaper's fallback mark placement was refused for
    /// want of scratch past [`crate::atlas::VAR_POINTS_MAX`], cumulative
    /// ([`Renderer::ink_refused`]).
    ink_refused: Tally,
    /// Which rows the last `convert` found changed; what `paint` repaints.
    dirty: Vec<bool>,
    storage: Vec<Glyph>,
    /// The shaper's scratch for a varied face's glyph boxes, grown to the
    /// need a refusal names and kept; empty while no face varies.
    ink_scratch: Vec<VarPoint>,
}

impl<'f, 'a> Renderer<'f, 'a> {
    /// A renderer for `cols` by `rows` cells; `None` when the primary face
    /// yields no cell geometry at `size`.
    pub fn new(
        stack: Stack<'f, 'a>,
        size: PxSize,
        theme: Theme,
        blend: Blend,
        cols: Cols,
        rows: Rows,
    ) -> Option<Renderer<'f, 'a>> {
        let metrics = Metrics::of(stack.primary(), size)?;
        let scales: Vec<Scale> = stack
            .faces()
            .iter()
            .map(|f| Scale::of(size.value(), f.units_per_em()))
            .collect();
        let digests = stack
            .faces()
            .iter()
            .map(|f| {
                let mut d = Vec::new();
                if let Some(gsub) = f.gsub() {
                    d.resize(MAX_LOOKUPS, LookupDigest::EMPTY);
                    gsub.digests(Features::GRID, &mut d);
                }
                d
            })
            .collect();
        #[cfg(feature = "exceptions")]
        let starters = stack
            .faces()
            .iter()
            .map(|f| match (f.gsub(), f.morx()) {
                (Some(gsub), _) => {
                    let mut d = vec![LookupDigest::EMPTY; MAX_LOOKUPS];
                    gsub.digests(Features::GRID, &mut d);
                    Starter::Gsub(d.into_iter().fold(LookupDigest::EMPTY, LookupDigest::union))
                }
                (None, Some(_)) => Starter::Morx,
                (None, None) => Starter::Plain,
            })
            .collect();
        let pairs = stack.faces().iter().map(Pairs::of).collect();
        let extents = stack
            .faces()
            .iter()
            .map(|f| f.gsub().and_then(|g| g.extent(Features::GRID)))
            .collect();
        let selections: Vec<Selections> = stack.faces().iter().map(Selections::of).collect();
        let faces = stack.each().collect();
        let defaults = (theme.fg, theme.bg);
        let mut r = Renderer {
            faces_of: stack.table(),
            cluster_faces: ClusterMemo::new(Question::Face, FaceIx::PRIMARY),
            cluster_covers: ClusterMemo::new(Question::Cover, Cover::Whole),
            stack,
            scales,
            #[cfg(feature = "exceptions")]
            starters,
            pairs,
            extents,
            selections,
            row_scripts: Vec::new(),
            row_unresolved: Vec::new(),
            faces,
            #[cfg(feature = "exceptions")]
            singles: std::collections::HashMap::new(),
            digests,
            metrics,
            theme,
            video: Video::Normal,
            defaults,
            page_default: defaults.1,
            blend,
            cache: RunCache::new(),
            atlas: Atlas::new(),
            emoji: EmojiDefault::Font,
            cols: Cols::of_index(0),
            rows: Rows::of_index(0),
            prev: Vec::new(),
            known: Prev::Unknown,
            cur: Vec::new(),
            pictures: Pictures::new(),
            styles: Vec::new(),
            styles_stamp: StyleStamp::default(),
            theme_gen: ThemeGen::default(),
            bg: Vec::new(),
            fg: Vec::new(),
            curve: Vec::new(),
            text: Vec::new(),
            row_blinks: Vec::new(),
            per_row: Vec::new(),
            last_cursor: None,
            last_cursor_opacity: Chan::from_u8(0xFF),
            highlight: None,
            last_highlight: None,
            bar: None,
            last_bar: None,
            gutter: Px::of(0),
            bar_inst: Vec::new(),
            trail: None,
            last_trail: None,
            trail_inst: Vec::new(),
            composition: None,
            last_composition: None,
            compose_inst: Vec::new(),
            cursor_fill: Argb::default(),
            flash_cap: Chan::from_u8(0),
            flash_held: flash::Held::Full,
            flash_caps: std::collections::HashMap::new(),
            fade: None,
            last_fade: None,
            fade_inst: Vec::new(),
            last_clusters: None,
            blink: BlinkPhases::SHOWN,
            blink_slots: Tally::default(),
            blink_seen: Blinking::default(),
            cursor_blink: CursorPhase::SHOWN,
            glow: None,
            last_glow: None,
            glow_inst: Vec::new(),
            flash: VideoFlash::default(),
            image_places: Vec::new(),
            image_maps: Vec::new(),
            last_image_places: Vec::new(),
            image_uploads: Vec::new(),
            image_epoch: ImageGen::ZERO,
            image_snapshots: Tally::of(0),
            marks_unmapped: Tally::of(0),
            ink_refused: Tally::of(0),
            dirty: Vec::new(),
            storage: vec![Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 256],
            ink_scratch: Vec::new(),
        };
        r.resize(cols, rows);
        Some(r)
    }

    pub fn metrics(&self) -> Metrics {
        self.metrics
    }

    /// A new pixel size for the same faces -- the backing scale changed, or
    /// the font size -- so the metrics, the per-face scales and the atlas are
    /// rebuilt and the next convert repaints every row. `None` leaves the
    /// renderer as it was, when the primary face cannot be measured at that
    /// size.
    ///
    /// **The run cache is kept.** A `Shaped` holds glyph ids, cell spans and
    /// offsets *in font units* (`Placed`), and the current `scale` is applied
    /// at composite -- none of it depends on the pixel size, so shaping a run
    /// at the old size still names the right glyphs at the new one. Only the
    /// atlas holds pixels, so only the atlas resets; clearing the cache too
    /// re-shaped every visible run for nothing, which was ~1.4 ms of the cold
    /// frame a font change forces (btop 14->24 px: convert 2.5 ms to 1.1 ms,
    /// cache misses 163 to 0). The rasterising that remains is the size
    /// change itself and cannot be skipped.
    pub fn rescale(&mut self, size: PxSize) -> Option<()> {
        let metrics = Metrics::of(self.stack.primary(), size)?;
        self.scales = self
            .stack
            .faces()
            .iter()
            .map(|f| Scale::of(size.value(), f.units_per_em()))
            .collect();
        self.metrics = metrics;
        self.atlas.reset();
        #[cfg(feature = "exceptions")]
        self.singles.clear();
        self.known = Prev::Unknown;
        Some(())
    }

    /// How a character with no selector is drawn (`EmojiDefault`). A change
    /// moves faces under glyphs the atlas, the map and the overlay hold, so
    /// it starts them again, as a change of smoothing does.
    pub fn set_emoji_default(&mut self, emoji: EmojiDefault) {
        if emoji == self.emoji {
            return;
        }
        self.emoji = emoji;
        self.atlas.reset();
        #[cfg(feature = "exceptions")]
        self.singles.clear();
        self.known = Prev::Unknown;
    }

    /// What [`Renderer::set_emoji_default`] last set.
    pub fn emoji_default(&self) -> EmojiDefault {
        self.emoji
    }

    /// The face a cluster is drawn from in `style`, `cps` being a cell's
    /// base and the codepoints attached to it: the one place the
    /// presentation and the marks are applied, so a path cannot pick a face
    /// another path does not. The base's face is a lookup into the
    /// [`FaceTable`], its presentation from the codepoint after it; a cell
    /// with marks then takes the first face, that one and then the fallback
    /// chain, that has the base and every mark ([`Stack::select_cluster`]),
    /// remembered per cluster so it too is a probe -- one a frame, or two
    /// for a marked cell that ends a run, which is asked again as the next
    /// run's first (`Renderer::face_of_cell`). A cell with no
    /// marks stops at the table, as it always has: the cluster half is out
    /// of line (`Renderer::cluster_face`) so the table lookup is all a
    /// caller inlines.
    #[inline]
    pub fn face_for(&mut self, cps: &[Codepoint], style: FaceStyle) -> FaceIx {
        let Some(&base) = cps.first() else {
            return FaceIx::PRIMARY;
        };
        let p = self.emoji.presentation(base, cps.get(1).copied());
        let face = self.faces_of.face(&self.stack, base, style, p);
        match cps {
            [_] => face,
            _ => self.cluster_face(cps, face),
        }
    }

    /// [`Renderer::face_for`] for a cell with marks, given `face`, its
    /// base's.
    #[inline(never)]
    fn cluster_face(&mut self, cps: &[Codepoint], face: FaceIx) -> FaceIx {
        let stack = &self.stack;
        self.cluster_faces
            .answer(face, cps, || stack.select_cluster(cps, face))
    }

    /// [`Renderer::face_for`] for the cell at `cell` of the grid: with its
    /// marks when it has them, read from the cluster store, and with its
    /// base alone when it has none -- which reads nothing and allocates
    /// nothing, so the run path's unmarked cells pay what they paid before
    /// a marked cell's face became its whole cluster's. A marked cell reads
    /// its marks here and again where the run gathers its codepoints, which
    /// is a second read of a few codepoints a marked cell may pay -- onto
    /// the stack, a base and the most the cluster store holds
    /// (`ClusterSlot::CAPACITY`), so the read allocates nothing.
    fn face_of_cell(&mut self, term: &Term<'_>, cell: PackedCell, base: Codepoint) -> FaceIx {
        let style = self.face_style(cell);
        if !cell.flags.is_marked() {
            return self.face_for(core::slice::from_ref(&base), style);
        }
        let mut cps = [base; 1 + ClusterSlot::CAPACITY];
        let mut marks = 0;
        for (slot, m) in cps[1..]
            .iter_mut()
            .zip(term.cluster_marks(cell).filter_map(Codepoint::new))
        {
            *slot = m;
            marks += 1;
        }
        self.face_for(&cps[..=marks], style)
    }

    /// How many codepoints have been left out of a cluster before it was
    /// shaped, since the renderer was made: once no face in the stack draws
    /// a cluster whole, the ones the face that keeps it -- its base's --
    /// lacks, which it would have drawn as a `.notdef`
    /// ([`Renderer::face_for`], `Normalised::without_unmapped`). That is
    /// not "marks no face draws": a mark another face maps is counted when
    /// that face lacks the letter or another of the marks, and an attached
    /// codepoint that is not a combining mark -- a flag's second regional
    /// indicator, an emoji joined after a ZWJ -- is counted as a mark is.
    /// Not counted: the codepoints the shaper's normalisation cuts past its
    /// sixteenth, and the marks on a box-drawing, block or braille cell,
    /// which baud draws from its own masks and never shapes.
    ///
    /// Counted when a cluster is shaped, so a run the cache answers is not
    /// counted again, and a cell drawn by both paths -- the run and the cell
    /// path's overlay -- is counted once by each: a ligature window shaped
    /// in chunks counts a cell only in the chunk that owns it, not again as
    /// a neighbour's context
    /// (`mark_coverage::the_overlay_counts_a_dropped_mark_once_across_window_chunks`).
    pub fn marks_unmapped(&self) -> Tally {
        self.marks_unmapped
    }

    /// How many glyph boxes the fallback mark placement of a face with no
    /// `GPOS` was refused, cumulative: a face read at a variation draws a
    /// glyph only through scratch, as any face draws a composite that
    /// matches points, the renderer grows it to the need the
    /// shaper reports and shapes the run again, and a need past
    /// [`crate::atlas::VAR_POINTS_MAX`] is refused -- its marks stay at the
    /// pen -- and counted here. Counted each time a run is shaped, for
    /// every cluster it holds: a ligature window's context cells, which
    /// [`Renderer::marks_unmapped`] leaves to the chunk that owns them,
    /// count here in every chunk that shapes them.
    pub fn ink_refused(&self) -> Tally {
        self.ink_refused
    }

    /// The table [`Renderer::face_for`] reads, built from the stack when the
    /// renderer is made.
    pub fn face_table(&self) -> &FaceTable {
        &self.faces_of
    }

    /// Where a cluster whose base is `base`, on `face`, sits in `span`:
    /// the grid phase in one cell; in two, its base's advance centred in
    /// the span -- half the slack, whole pixels and the fraction. The
    /// owner's call (Jev A 0.97, over 13 of 14 terminals' left edge): a
    /// one-cell ⚡ in its two cells, an emoji a third of a cell narrower than
    /// two, and a CJK fallback face's ideographs all sit in the middle. An
    /// icon is left to the patcher's rules, and a base with no advance, or
    /// one far wider than the span, keeps the grid phase.
    pub fn phase_in(&self, face: FaceIx, base: Codepoint, span: CellSpan) -> Phase {
        let grid = self.metrics.phase;
        if span == CellSpan::One || is_icon(base) {
            return grid;
        }
        let f = self.stack.face(face);
        let two = Px::of(self.metrics.cell_w.value().saturating_mul(2));
        f.glyph(base)
            .and_then(|g| f.default_advance(g))
            .and_then(|adv| Phase::in_span(self.scales[face.index()], adv, two))
            .unwrap_or(grid)
    }

    /// The selection to draw on the next frame, in the view's screen
    /// coordinates, or none. One rule on every path: a selected cell's
    /// resolved colours swap, before the block cursor's own swap.
    pub fn set_highlight(&mut self, highlight: Option<Highlight>) {
        self.highlight = highlight;
    }

    /// The scrollbar for the next frame, or none: where the window sits,
    /// how much of the thumb is seen and whether the pointer widened it,
    /// each the driver's. Its solids are the knob and, widened, the track,
    /// in the default foreground the screen draws -- the theme's, or
    /// `OSC 10`'s, or in reverse video the background, whether `?5` or a
    /// flash put it there -- and every path draws them -- the CPU and
    /// instance paths from the instance list, the cell path from
    /// [`Renderer::scrollbar`] appended to its records.
    pub fn set_scrollbar(&mut self, scroll: Option<Scroll>) {
        let size = self.frame_size();
        self.bar = scroll.and_then(|s| scrollbar::bar(s, size, self.metrics.cell_h));
        self.bar_inst.clear();
        if let Some(b) = self.bar {
            self.bar_inst
                .extend(scrollbar::instances(b, self.defaults.0));
        }
    }

    /// The scrollbar's instances, for a path that draws records of its own.
    pub fn scrollbar(&self) -> &[Instance] {
        &self.bar_inst
    }

    /// An input method's composition for the next frame, or none: drawn at
    /// its cell by the instance and CPU paths' next convert, with the cursor
    /// after it ([`Composition::cursor`]), on the live view only. The cell
    /// path draws it through [`crate::Overlay::composed`].
    pub fn set_composition(&mut self, c: Option<Composition>) {
        self.composition = c;
    }

    /// The cursor trail for the next frame, or none: a fading ghost
    /// sweeping from where the cursor was to where it is, the quad whose
    /// corners chase the cursor's, or the box swept from a tail chasing it. Every path draws it -- the CPU compositor
    /// in `paint`, the instance path from the instance list, the cell path
    /// from [`Renderer::trail`] appended to its records. The driver hands a
    /// ghost's `progress` or a smear's corners, each computed from the display
    /// link's timestamp, and this only draws them, so the frame is a pure
    /// function of the value and the pixel oracle can compare two GPU arms
    /// on it.
    pub fn set_trail(&mut self, look: Option<TrailLook>) {
        self.trail = look.map(|l| match l {
            TrailLook::Ghost(t) => {
                Drawn::Ghost(trail::ghost(t, self.metrics.cell_w, self.metrics.cell_h))
            }
            TrailLook::Smear(s) => Drawn::Smear(s),
            TrailLook::Vfx(v) => Drawn::Vfx(v, Px::of(0), Px::of(0)),
        });
        self.build_trail();
    }

    /// The trail's records, in the colours in force: one for a ghost, a
    /// quad or a sweep; an effect's bands and particles, and the rows they
    /// span.
    fn build_trail(&mut self) {
        self.trail_inst.clear();
        let frame = self.pixel_size();
        match &mut self.trail {
            Some(Drawn::Ghost(g)) => self.trail_inst.push(trail::instance(*g, self.cursor_fill)),
            Some(Drawn::Smear(s)) => self
                .trail_inst
                .push(trail::quad_instance(s, self.cursor_fill)),
            Some(Drawn::Vfx(v, top, bottom)) => {
                v.instances(frame, self.cursor_fill, &mut self.trail_inst);
                // A repeat pass keeps one record a pixel: the bands, a cell
                // row each and disjoint, and not the particles that stack
                // over them, whose alphas would compound past the cap.
                if v.emitter.strength == Strength::UnderThreshold {
                    self.trail_inst.retain(|r| r.kind == Kind::Band);
                }
                let (mut lo, mut hi) = (i32::MAX, i32::MIN);
                for inst in &self.trail_inst {
                    lo = lo.min(i32::from(inst.y.0));
                    hi = hi.max(i32::from(inst.y.0) + i32::from(inst.h.0));
                }
                // The CPU path paints the emitter's quad, which an effect's
                // records need not cover: the ufo's bands hold only its
                // saucer, beam and wake.
                for p in v.emitter.corners.0 {
                    lo = lo.min(p.y.floor() as i32);
                    hi = hi.max(p.y.ceil() as i32 + 1);
                }
                let row = |v: i32| Px::of(u32::try_from(v.max(0)).unwrap_or(0));
                (*top, *bottom) = if lo < hi {
                    (row(lo), row(hi))
                } else {
                    (Px::of(0), Px::of(0))
                };
            }
            None => {}
        }
        if self.strength() == Strength::UnderThreshold {
            let k = self.flash_cap;
            for r in &mut self.trail_inst {
                r.color = r.color.with_alpha(flash::capped(r.color.a(), k));
            }
        }
    }

    /// How this frame's trail is drawn, as the driver weighed it.
    fn strength(&self) -> Strength {
        match &self.trail {
            Some(Drawn::Ghost(g)) => g.strength,
            Some(Drawn::Smear(s)) => s.strength,
            Some(Drawn::Vfx(v, ..)) => v.emitter.strength,
            None => Strength::Full,
        }
    }

    /// **The cap a repeat pass is drawn at, from the cells under it**: the
    /// least [`flash::under_threshold`] of the cursor's colour over each
    /// colour those cells draw, made opaque -- every one's background, and
    /// the text colours of one that holds a character or sets an attribute
    /// (its solid and glass text, its line and its underline) -- so a pass
    /// over a red cell is held under the red ground's thresholds, one over
    /// text under the text's, and one over the page under the page's. The
    /// records are rebuilt when it moves. Run where the frame's styles are
    /// read ([`Renderer::sync_styles`]), after the trail is set, on every
    /// path. A pass whose records meet an image placed under the text, or
    /// below the backgrounds, draws nothing: a cap of 0. The image test is a
    /// rectangle overlap of each such placement with each record, so a
    /// partly transparent image also counts as a ground, the safe
    /// direction.
    ///
    /// **What the frame lays over a cell is a ground too.** A selected cell
    /// and the cell under the cursor add the colours [`Resolved::drawn`]
    /// gives them this frame -- the swapped background and text, a fading
    /// block's mixture, a bar's or an underline's own colour -- beside the
    /// rendition's. **A dimmed page is read over the desktop**: a ground
    /// drawn translucent adds itself over [`DESKTOPS`], white and black,
    /// note AS's bound, since what a screen shows there is the page over
    /// what is behind the window. A coloured desktop between them is not
    /// read.
    fn bound_trail(&mut self, term: &Term<'_>) {
        if self.strength() == Strength::Full {
            self.flash_held = flash::Held::Full;
            return;
        }
        let view = term.view(term.window_back());
        let (cols, rows) = (view.cols().index(), view.rows().index());
        let side = |v: u32| usize::try_from(v.max(1)).unwrap_or(1);
        let (cw, ch) = (
            side(self.metrics.cell_w.value()),
            side(self.metrics.cell_h.value()),
        );
        let at = |v: i32| usize::try_from(v.max(0)).unwrap_or(0);
        let blank = |c: baud_domain::Content| {
            c.is_empty() || c.as_codepoint().is_some_and(|p| p.value() == 0x20)
        };
        // The cursor as `convert_with` finds it: the live screen's, at an
        // input method's composition while one is shown, and only while
        // this frame's phase draws it.
        let live = term.looking_back(term.window_back()).index() == 0;
        let cursor = (term.cursor_visible() && live)
            .then(|| {
                let (r, c) = match &self.composition {
                    Some(c) if c.row.index() < rows => c.cursor(view.cols()),
                    _ => term.cursor(),
                };
                (r, c, term.cursor_style())
            })
            .filter(|&(_, _, style)| self.cursor_drawn(style));
        let mut ids: Vec<(baud_domain::StyleId, Inked, Selected, Cursored)> = Vec::new();
        'walk: for r in &self.trail_inst {
            let (x, y) = (i32::from(r.x.0), i32::from(r.y.0));
            let (x1, y1) = (x + i32::from(r.w.0), y + i32::from(r.h.0));
            for row in at(y) / ch..at(y1).div_ceil(ch).min(rows) {
                let row = Row::from_index(row);
                let cells = view.row(row);
                let first = (at(x) / cw).min(cols);
                for (c, cell) in cells[first..at(x1).div_ceil(cw).min(cols)]
                    .iter()
                    .enumerate()
                {
                    let col = Col::at_index(first + c);
                    let inked = if blank(cell.content) {
                        Inked::No
                    } else {
                        Inked::Yes
                    };
                    let selected = match self
                        .highlight
                        .is_some_and(|h| h.selects(row, col, cell.content))
                    {
                        true => Selected::Yes,
                        false => Selected::No,
                    };
                    let cursored = match cursor {
                        Some((cr, cc, style)) if cr == row && cc == col => match style {
                            CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock => {
                                Cursored::Block
                            }
                            _ => Cursored::Shape,
                        },
                        _ => Cursored::No,
                    };
                    let id = (cell.style, inked, selected, cursored);
                    if !ids.contains(&id) {
                        ids.push(id);
                        if ids.len() > FLASH_GROUNDS {
                            break 'walk;
                        }
                    }
                }
            }
        }
        // An image under the text is a ground the cells do not name: their
        // styles are the page's while the pixels are the image's.
        let imaged = self.image_places.iter().any(|p| {
            let (px0, py0) = (i32::from(p.x), i32::from(p.y));
            let (px1, py1) = (px0 + i32::from(p.w), py0 + i32::from(p.h));
            p.layer.under_text()
                && self.trail_inst.iter().any(|r| {
                    let (x, y) = (i32::from(r.x.0), i32::from(r.y.0));
                    x < px1 && px0 < x + i32::from(r.w.0) && y < py1 && py0 < y + i32::from(r.h.0)
                })
        });
        let ink = self.cursor_fill.with_alpha(Chan::from_u8(0xFF));
        let opaque = |c: Argb| c.with_alpha(Chan::from_u8(0xFF));
        let mut triples: Vec<(Argb, Argb)> = Vec::new();
        // A text colour over a page, and each over the desktops while the
        // page is drawn translucent.
        let ground = |text: Argb, page: Argb, triples: &mut Vec<(Argb, Argb)>| {
            let (text, solid) = (opaque(text), opaque(page));
            let seen = core::iter::once(solid).chain(
                DESKTOPS
                    .into_iter()
                    .filter(|_| page.a().get() < 0xFF)
                    .map(|d| solid.mix(d, page.a())),
            );
            for page in seen {
                if !triples.contains(&(text, page)) {
                    triples.push((text, page));
                }
            }
        };
        let cursor_color = term
            .cursor_color_in(self.video)
            .map(|(r, g, b)| Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)));
        for &(id, inked, selected, cursored) in &ids {
            let word = term
                .styles()
                .get(id.index())
                .copied()
                .flatten()
                .unwrap_or(baud_domain::StyleWord::DEFAULT);
            let res = self.resolve_slot(term, word);
            let text = inked == Inked::Yes || word.attrs() != baud_domain::Attrs::NONE;
            let n = if text { 5 } else { 1 };
            for colour in [res.bg, res.fg, res.glass, res.line, res.underline]
                .into_iter()
                .take(n)
            {
                ground(colour, res.bg, &mut triples);
            }
            if selected == Selected::No && cursored == Cursored::No {
                continue;
            }
            // The cell as this frame draws it under the selection and the
            // cursor, the swaps `convert_row` takes.
            let d = res.drawn(Swaps {
                pictured: crate::picture::Pictured::No,
                line: Line::No,
                selected,
                block: match cursored {
                    Cursored::Block => BlockCursor::Yes(cursor_color),
                    _ => BlockCursor::No,
                },
                blink: self.blink,
                cursor: cursor.map_or(Chan::from_u8(0xFF), |(_, _, s)| self.cursor_opacity(s)),
            });
            ground(d.bg, d.bg, &mut triples);
            if text {
                for colour in [d.fg, res.line, res.underline] {
                    ground(colour, d.bg, &mut triples);
                }
            }
            if cursored == Cursored::Shape {
                ground(cursor_color.unwrap_or(d.cursor_fg), d.bg, &mut triples);
            }
        }
        // **Bounded a frame, and sound while it is**: the repeat draws
        // nothing, a cap of 0, when more than [`FLASH_GROUNDS`] grounds lie
        // under the pass (a picture, a gradient), when an image placed under
        // the text lies beneath one of its records, or while a ground's cap
        // is not yet scanned; [`FLASH_SCANS`] new caps are scanned a frame.
        if self.flash_caps.len() > FLASH_CACHED {
            self.flash_caps.clear();
        }
        let mut budget = FLASH_SCANS;
        let mut k = Chan::from_u8(0xFF);
        let over = ids.len() > FLASH_GROUNDS || triples.len() > FLASH_GROUNDS;
        if imaged || over {
            k = Chan::from_u8(0);
        }
        let mut bound = false;
        for (text, page) in triples {
            if k.get() == 0 && budget == 0 {
                break;
            }
            let c = match self.flash_caps.get(&(ink, text, page)) {
                Some(&c) => c,
                None if budget > 0 => {
                    budget -= 1;
                    let c = flash::under_threshold_over(ink, text, page);
                    self.flash_caps.insert((ink, text, page), c);
                    c
                }
                None => {
                    k = Chan::from_u8(0);
                    continue;
                }
            };
            bound |= c.get() == 0;
            if c.get() < k.get() {
                k = c;
            }
        }
        self.flash_held = match () {
            _ if k.get() > 0 => flash::Held::Capped,
            _ if imaged => flash::Held::Imaged,
            _ if over => flash::Held::Grounds,
            _ if bound => flash::Held::Bound,
            _ => flash::Held::Scanning,
        };
        if k != self.flash_cap {
            self.flash_cap = k;
            self.build_trail();
        }
    }

    /// The most alpha the trail's records carry while it draws a repeat
    /// pass, for a bench or a test to hold them to.
    pub fn flash_cap(&self) -> Chan {
        self.flash_cap
    }

    /// Why this frame's repeat pass carries [`Renderer::flash_cap`]: drawn
    /// as it is, capped, or drawing nothing, and for which reason.
    pub fn flash_held(&self) -> flash::Held {
        self.flash_held
    }

    /// The caps scanned and kept, which a bench reads to see the cache
    /// emptied.
    pub fn flash_cached(&self) -> Tally {
        Tally::of(self.flash_caps.len())
    }

    /// The effect this frame draws, for a GPU path to hand its fragment
    /// function the uniform the records do not carry.
    pub fn vfx(&self) -> Option<Vfx> {
        match self.trail {
            Some(Drawn::Vfx(v, ..)) => Some(v),
            _ => None,
        }
    }

    /// The quad or the sweep this frame draws, for a GPU path to hand its
    /// fragment function the corners the record does not carry.
    pub fn smear(&self) -> Option<Smear> {
        match self.trail {
            Some(Drawn::Smear(s)) => Some(s),
            _ => None,
        }
    }

    /// The rectangle a cursor of `style` at `(row, col)` fills, in pixels
    /// of the frame: the whole cell for a block, a line along its foot for
    /// an underline, one along its left edge for a bar. **The one place
    /// that rectangle is spelled** -- `convert_row` draws the cursor from
    /// it and a quad trail chases and spares it, so the two cannot differ
    /// by the pixel that would leave the landed trail a fringe.
    pub fn cursor_rect(&self, row: Row, col: Col, style: CursorStyle) -> Rect {
        let (left, top, w, h) = self.cursor_px(row, col, style);
        let f = |v: u32| f32::from(u16::try_from(v).unwrap_or(u16::MAX));
        Rect {
            left: f(left.value()),
            top: f(top.value()),
            right: f(left.value() + w.value()),
            bottom: f(top.value() + h.value()),
        }
    }

    /// [`Renderer::cursor_rect`] in whole pixels: left, top, width, height.
    fn cursor_px(&self, row: Row, col: Col, style: CursorStyle) -> (Px, Px, Px, Px) {
        let x = u32::try_from(col.index())
            .unwrap_or(0)
            .saturating_mul(self.metrics.cell_w.value());
        let top = self.band(row).top.value();
        let (cw, ch) = (self.metrics.cell_w.value(), self.metrics.cell_h.value());
        let line = self.metrics.thickness.value().max(2);
        let (left, top, w, h) = match style {
            CursorStyle::BlinkingUnderline | CursorStyle::SteadyUnderline => {
                (x, top + ch.saturating_sub(line), cw, line)
            }
            CursorStyle::BlinkingBar | CursorStyle::SteadyBar => (x, top, line, ch),
            _ => (x, top, cw, ch),
        };
        (Px::of(left), Px::of(top), Px::of(w), Px::of(h))
    }

    /// The cursor trail's instances, for a path that draws records of its
    /// own -- drawn after the cells and under the scrollbar.
    pub fn trail(&self) -> &[Instance] {
        &self.trail_inst
    }

    /// The alternate-screen fade for the next frame, or none: one
    /// full-screen translucent solid in the default background the screen
    /// draws -- the video and `OSC 11` included -- that every
    /// path draws on top of everything else, so the new screen appears out
    /// of the background. The driver hands a `progress` it computed from the
    /// display link's timestamp; this only interpolates, so the pixel oracle
    /// can compare two GPU arms on it.
    pub fn set_screen_fade(&mut self, fade: Option<Transition>) {
        let (w, h) = self.pixel_size();
        self.fade = fade.map(|t| transition::veil(t, w, h));
        self.fade_inst.clear();
        if let Some(v) = self.fade {
            self.fade_inst
                .push(transition::instance(v, self.defaults.1));
        }
    }

    /// The screen fade's instances, for a path that draws records of its
    /// own -- drawn last, over the cells, the trail and the scrollbar.
    pub fn screen_fade(&self) -> &[Instance] {
        &self.fade_inst
    }

    /// Sets the blink phases the next convert or resolve draws with, one a
    /// rate.
    pub fn set_blink(&mut self, blink: BlinkPhases) {
        self.blink = blink;
    }

    /// This frame's `SGR 5` and `SGR 6` phases, for the cell shader, which
    /// hides blinking text itself: a resolved slot is the same in every
    /// phase.
    pub fn blink(&self) -> BlinkPhases {
        self.blink
    }

    /// Whether the rendition table holds a slot blinking at either rate, as
    /// of the last resolve: the cell path's walk tests cells only then. An
    /// on/off answer, not which rates: [`Blinking::BOTH`] when any slot
    /// blinks, slow-only included, and [`Blinking::UNSEEN`] when none does.
    pub(crate) fn blink_slots(&self) -> Blinking {
        match self.blink_slots.value() {
            0 => Blinking::UNSEEN,
            _ => Blinking::BOTH,
        }
    }

    /// The cell path's answer to [`Renderer::blink_seen`], from its walk.
    pub(crate) fn set_blink_seen(&mut self, seen: Blinking) {
        self.blink_seen = seen;
    }

    /// The cursor's blink phase this frame. The driver owns the clock and
    /// the rules for when a cursor blinks at all -- focused, visible, on the
    /// live screen -- and hands [`Blink::Shown`] whenever it does not, so a
    /// steady cursor is never hidden.
    ///
    /// A [`Blink`] is a hard blink's phase; a [`CursorPhase`] carries a fade's
    /// opacity and the plasma glow's pulse as well.
    pub fn set_cursor_blink(&mut self, phase: impl Into<CursorPhase>) {
        self.cursor_blink = phase.into();
    }

    /// Whether a cursor of `style` is drawn this frame: always, unless it
    /// blinks and the phase leaves none of it. The one spelling of the rule,
    /// which the CPU and instance paths apply in [`Renderer::convert`] and a
    /// cell-path driver applies to the cursor it hands the shader.
    pub fn cursor_drawn(&self, style: CursorStyle) -> bool {
        self.cursor_opacity(style) != Chan::from_u8(0)
    }

    /// How much of a cursor of `style` this frame draws: all of a steady
    /// one, and of a blinking one the phase's opacity. The block's swap and
    /// the bar's or underline's shape are taken this far on every path --
    /// the cell shader's `cursor_alpha` is this.
    pub fn cursor_opacity(&self, style: CursorStyle) -> Chan {
        if style.blinks() {
            self.cursor_blink.opacity
        } else {
            Chan::from_u8(0xFF)
        }
    }

    /// **The blink animation's solids over the cursor at `at`** -- the
    /// plasma glow, the fire, the ufo -- built for this frame: the live cursor
    /// whether or not the phase draws it, `None` when there is none on the
    /// screen being drawn. Nothing for a steady style or a phase without
    /// them. [`Renderer::convert`] calls this itself; a cell-path driver
    /// calls it with the cursor it computed and draws
    /// [`Renderer::cursor_glow`] after the trail's records.
    pub fn set_cursor_glow_at(&mut self, at: Option<(Row, Col, CursorStyle)>) {
        let phase = self.cursor_blink;
        let plays = phase.glow != Chan::from_u8(0) || phase.play != Play::Still;
        self.glow = at
            .filter(|&(_, _, style)| style.blinks() && plays)
            .map(|(r, c, style)| (r, c, style, phase));
        self.glow_inst.clear();
        if let Some((r, c, style, phase)) = self.glow {
            let cell = self.cursor_px(r, c, CursorStyle::SteadyBlock);
            caret::glow(cell, phase.glow, self.cursor_fill, &mut self.glow_inst);
            let shape = self.cursor_px(r, c, style);
            caret::play(
                cell,
                shape,
                phase.play,
                self.cursor_fill,
                &mut self.glow_inst,
            );
        }
    }

    /// The blink animation's solids, for a path that draws records of its
    /// own: after the trail's and under the scrollbar, as
    /// [`Renderer::instances`] has them.
    pub fn cursor_glow(&self) -> &[Instance] {
        &self.glow_inst
    }

    /// Show the other video this frame, or stop. Nothing is rebuilt here:
    /// the next convert or resolve reads it (`refresh_screen`), which is
    /// where a `?5` flip is read too, so the two take the same path.
    pub fn set_flash(&mut self, flash: VideoFlash) {
        self.flash = flash;
    }

    /// The video the last convert or resolve drew in: `?5`'s, or under a
    /// flash the other one. What a caller that draws the cursor itself -- the
    /// cell path -- reads the `OSC 12` colour in ([`Term::cursor_color_in`]).
    pub fn video(&self) -> Video {
        self.video
    }

    /// Replace the palette every rendition resolves against.
    ///
    /// **Every cell's colour changes, so the damage cache has to go**: row
    /// damage compares a row's cell words and the digest of the renditions
    /// they resolve to, and a theme swap moves neither -- the words are the
    /// same and a `StyleId` still indexes the same slot. Without
    /// [`Prev::Unknown`] a light/dark flip would repaint nothing at all,
    /// which is the same shape as the blink phase needing a forced repaint
    /// because a blink changes no cell content either.
    ///
    /// The scrollbar, the trail and the fade cache the defaults at the
    /// moment they were set, so they are rebuilt here rather than left
    /// drawing the old colours until they are next set. The `OSC 10`/`11`
    /// half of the defaults is the terminal's, which this cannot read, so
    /// the next convert or resolve puts it back (`refresh_screen`).
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.theme_gen = ThemeGen(self.theme_gen.0.wrapping_add(1));
        self.known = Prev::Unknown;
        self.defaults = self.theme.defaults(self.video, |_| None);
        self.page_default = self.theme.bg;
        self.rebuild_overlays();
    }

    /// Reads the screen's own colours for this frame -- the video, `?5` or a
    /// flash's other one, and the `OSC 10`/`11` defaults -- and, when they
    /// moved, rebuilds the solids already built in the old ones. **They are
    /// set before a convert**, so without the rebuild the frame a program
    /// flips `?5` on, or a flash starts on, would draw its scrollbar, trail
    /// and fade in the colours of the frame before.
    fn refresh_screen(&mut self, term: &Term<'_>) {
        self.video = video_of(term, self.flash);
        let defaults = self.theme.defaults(self.video, |d| term.dynamic_color(d));
        self.page_default = self
            .theme
            .defaults(Video::Normal, |d| term.dynamic_color(d))
            .1;
        let fill = term
            .cursor_color_in(self.video)
            .map_or(defaults.0, |(r, g, b)| {
                Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
            });
        if defaults != self.defaults || fill != self.cursor_fill {
            self.defaults = defaults;
            self.cursor_fill = fill;
            self.rebuild_overlays();
        }
    }

    /// The scrollbar, the trail and the fade, rebuilt in the defaults in
    /// force from the geometry each was last set with.
    fn rebuild_overlays(&mut self) {
        self.bar_inst.clear();
        if let Some(b) = self.bar {
            self.bar_inst
                .extend(scrollbar::instances(b, self.defaults.0));
        }
        self.build_trail();
        self.fade_inst.clear();
        if let Some(v) = self.fade {
            self.fade_inst
                .push(transition::instance(v, self.defaults.1));
        }
    }

    /// The theme in force, so a driver can tell the core what the defaults
    /// draw as (`Term::set_default_colors`) from the one place that knows.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Which rates of blinking text are on screen as of the last frame, so a
    /// driver repaints on a rate's phase flip only when text at that rate
    /// blinks: a cell on screen with `Attrs::BLINK` or `Attrs::RAPID_BLINK`,
    /// over the whole screen and not only the rows the frame converted. The
    /// CPU and instance paths answer from `convert`; the cell path from the
    /// overlay's walk (`Overlay::ensure`), since a slot in the table that no
    /// cell on screen names is not on screen.
    pub fn blink_seen(&self) -> Blinking {
        self.blink_seen
    }

    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }

    pub fn atlas_stats(&self) -> AtlasStats {
        self.atlas.stats()
    }

    /// The default-background colour, straight (not premultiplied), for a
    /// presenter to clear the margin around the grid to -- so the gutter is
    /// the terminal's own background rather than a see-through edge. It is
    /// what a default cell resolves to, as of the last convert or resolve:
    /// the theme's background, or `OSC 11`'s, or in reverse video -- `?5`'s,
    /// or a flash's -- the foreground,
    /// at the window's alpha. It was the theme's whatever `OSC 11` said, so a
    /// program that moved the background got a frame of the old colour.
    pub fn clear_color(&self) -> Argb {
        self.defaults.1.with_alpha(self.theme.bg_alpha)
    }

    /// [`Renderer::clear_color`] as the next convert of `term` will answer
    /// it, for a driver that fills a margin the renderer never paints
    /// before it renders -- the Linux frame is the window's size, and the
    /// strips past the grid are filled once, so a `?5` or an `OSC 11` would
    /// leave them the old colour. It reads the flash already set, so a driver
    /// sets that first.
    pub fn clear_color_of(&self, term: &Term<'_>) -> Argb {
        let video = video_of(term, self.flash);
        let (_, bg) = self.theme.defaults(video, |d| term.dynamic_color(d));
        bg.with_alpha(self.theme.bg_alpha)
    }

    /// The pixels past the grid's right edge the frame keeps for the
    /// scroller: [`crate::gutter`] of the style in force, which the driver
    /// sets where it lays the grid out. A change repaints every row, since
    /// the frame is a different width.
    pub fn set_gutter(&mut self, gutter: Px) {
        if gutter != self.gutter {
            self.gutter = gutter;
            self.known = Prev::Unknown;
        }
    }

    /// The pixel size of the frame: the grid, and the scroller's gutter
    /// beside it. What a presenter letterboxes, what a CPU target is, and
    /// what the scrollbar is drawn and hit-tested against; the cells are
    /// [`Renderer::pixel_size`].
    pub fn frame_size(&self) -> (Px, Px) {
        let (w, h) = self.pixel_size();
        (Px::of(w.value().saturating_add(self.gutter.value())), h)
    }

    /// The pixel size of the whole grid.
    pub fn pixel_size(&self) -> (Px, Px) {
        (
            Px::of(
                u32::try_from(self.cols.index())
                    .unwrap_or(0)
                    .saturating_mul(self.metrics.cell_w.value()),
            ),
            Px::of(
                u32::try_from(self.rows.index())
                    .unwrap_or(0)
                    .saturating_mul(self.metrics.cell_h.value()),
            ),
        )
    }

    fn resize(&mut self, cols: Cols, rows: Rows) {
        self.cols = cols;
        self.rows = rows;
        let cells = cols.index() * rows.index();
        self.prev = vec![PackedCell::EMPTY; cells];
        self.cur = vec![PackedCell::EMPTY; cells];
        self.bg = vec![Argb(0); cells];
        self.fg = vec![Argb(0); cells];
        self.curve = vec![CurveRow::default(); cells];
        self.text = vec![Text::Drawn; cells];
        self.row_blinks = vec![Blinking::UNSEEN; rows.index()];
        self.per_row = (0..rows.index()).map(|_| Vec::new()).collect();
        self.dirty = vec![false; rows.index()];
        self.known = Prev::Unknown;
    }

    fn row_cells(&self, r: Row) -> core::ops::Range<usize> {
        let cols = self.cols.index();
        r.index() * cols..(r.index() + 1) * cols
    }

    fn band(&self, r: Row) -> Band {
        let h = self.metrics.cell_h.value();
        let top = u32::try_from(r.index()).unwrap_or(0).saturating_mul(h);
        Band {
            top: Px::of(top),
            bottom: Px::of(top.saturating_add(h)),
        }
    }

    /// Reads the terminal, converts the rows that changed and paints them
    /// into `target`: [`Renderer::convert`] then [`Renderer::paint`].
    pub fn render(
        &mut self,
        term: &mut Term<'_>,
        target: &mut Target<'_>,
        damage: Damage,
    ) -> Frame {
        self.render_at(term, Rows::ZERO, target, damage)
    }

    /// [`Renderer::render`] of a window `back` lines up the history: the
    /// rows [`Term::view`] shows, and the images on them. What a driver
    /// that scrolls the CPU path back calls, so the text moves with the
    /// thumb, the selection and the link under a click.
    pub fn render_at(
        &mut self,
        term: &mut Term<'_>,
        back: Rows,
        target: &mut Target<'_>,
        damage: Damage,
    ) -> Frame {
        // **Images draw on every frame, and the rows one left are repainted.**
        // They drew only on a full frame, and a live terminal renders
        // incrementally after its first, so on the Linux driver, which is
        // CPU-only, and on the macOS CPU path, a Sixel a program printed did
        // not appear until a resize. Blitting every frame is sound because
        // the blit is idempotent -- an image pixel is opaque or clear, so
        // blitting it again over the pixel it left changes nothing -- and the
        // one hazard is an image never erased where it moved from. So the
        // placements are computed first, and the rows under a placement the
        // last CPU frame blitted and this one does not are repainted; a still
        // image's rows are not, and a new one's need not be, since its blit
        // lands on the cells. That holds for an opaque-or-clear image; one
        // with a partly transparent pixel (`ImageAlpha::Partial`) blitted
        // over its own last blit would blend twice, so the rows under it are
        // repainted on every frame it is on, and counted in `rows_partial`.
        //
        // An image under the text (`ImageLayer::Under`) is blitted inside the
        // row loop instead, over a repainted row's background and under its
        // glyphs, so it is never over its own last blit; the rows one
        // arrives on are repainted for it (`arrived_under`).
        self.fit(term);
        self.place_images(term, back, Snapshots::Skipped);
        let converted = self.convert_with(term, back, damage, ImageRows::Repainted);
        let under = self.image_places.iter().any(|p| p.layer.under_text());
        let frame = match under {
            true => self.paint_rows(target, converted, UnderImages(term.images())),
            false => self.paint_rows(target, converted, NoUnder),
        };
        self.paint_images(term, target);
        frame
    }

    /// Computes where every visible image placement lands this frame, into
    /// [`Renderer::image_placements`], and snapshots each referenced image's
    /// pixels into [`Renderer::image_uploads`] for a GPU to upload once.
    ///
    /// A placement anchors to an absolute line of its screen; its screen row
    /// is `line - top`, where [`Term::image_origin`] gives `top` for a view
    /// `back` lines up with the same clamp [`Term::view`] uses -- so as output
    /// scrolls the image moves up with its line, a scrollback view shows the
    /// images anchored there, and an image and the grid row under it are drawn
    /// from one answer. A placement on the screen that is not up is not
    /// drawn: the shell's image is not the editor's. What it draws from the
    /// anchor cell -- the source rectangle, `c`/`r`, `X`/`Y` -- is
    /// `baud_domain::KittyPlace::draw` at this renderer's cell size, a Sixel
    /// its whole image at its own size. Of the drawn rectangle only the part
    /// on the frame is kept, from one row above the top, where the leaving
    /// row slides under smooth scroll, to the bottom; a placement with none
    /// there is skipped, and the GPU rasteriser and the CPU blit clip what is
    /// above the top.
    ///
    /// This is the one place the placement-to-screen arithmetic is written:
    /// the CPU `paint_images` and the two GPU arms all read the list and its
    /// tables, so three paths cannot disagree about where an image goes or
    /// which pixel it shows -- which the pixel oracle could not otherwise
    /// tell from a blit bug.
    ///
    /// **Order is the text's**: by `z`, and at one `z` "the image with the
    /// lower id is considered to have the lower z-index"
    /// (`[[source: kitty-graphics]]`), so drawn first; at one id, which the
    /// text leaves undefined, in placement order. Kitty orders by its own
    /// internal id, the order images were made (`[[source: kitty-graphics-c]]`
    /// `grman_update_layers`), where the text names the client's id; the
    /// text is followed.
    pub fn set_images(&mut self, term: &Term<'_>, back: Rows) {
        self.place_images(term, back, Snapshots::Kept);
    }

    /// [`Renderer::set_images`], with or without the upload snapshots: the
    /// CPU path blits from the terminal's pixels and skips them, which kept a
    /// second converted copy of every visible image -- two megabytes for a
    /// 600x800 preview -- for a GPU that path does not have.
    fn place_images(&mut self, term: &Term<'_>, back: Rows, snap: Snapshots) {
        self.image_places.clear();
        self.image_maps.clear();
        let imgs = term.images();
        let epoch = imgs.generation();
        // The snapshot cache is kept across frames and dropped whole only when
        // the store epoch moves -- a `store`, `free` or `reset` -- so a static
        // image on screen is not re-snapshotted every frame. Within an epoch,
        // an id names the same pixels, so the `any(id)` check below is the
        // never-rebuild gate.
        if self.image_epoch != epoch {
            self.image_uploads.clear();
            self.image_epoch = epoch;
        }
        let origin = term.image_origin(back);
        let top = origin.top.value() as i64;
        let cw = i64::from(self.metrics.cell_w.value());
        let ch = i64::from(self.metrics.cell_h.value());
        let cell = |p: Px| ImagePx::of(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let (cell_w, cell_h) = (cell(self.metrics.cell_w), cell(self.metrics.cell_h));
        let screen_h = self.rows.index() as i64 * ch;
        let frame_w = i64::from(self.frame_size().0.value());
        for (_, p) in imgs.placements() {
            if p.screen != origin.screen {
                continue;
            }
            let Some(view) = imgs.image(p.image) else {
                continue;
            };
            let draw = p.kitty.draw(view.width, view.height, cell_w, cell_h);
            let dest = draw.dest;
            if dest.is_empty() {
                continue;
            }
            // A margin scroll's cut: the placement's line is its first shown
            // row, the rows cut off its top above it, and nothing is drawn
            // outside the rows it still shows (`ImageStore::scroll_band`).
            let rows_px = |r: Rows| r.index() as i64 * ch;
            let (cut_top, cut_bottom) = (rows_px(p.kitty.cut_top), rows_px(p.kitty.cut_bottom));
            let x0 = p.col.index() as i64 * cw + i64::from(dest.x);
            let y0 = (p.line.value() as i64 - top) * ch - cut_top + i64::from(dest.y);
            // The visible part, in drawn pixels from the drawn top-left:
            // columns on the frame, rows from one row above the top -- the
            // leaving row the cell path draws under smooth scroll, which
            // `draw_images` pans the image into -- to the bottom. A frame
            // that is not sliding draws that row at a negative y, which
            // every path clips.
            let (i0, i1) = ((-x0).max(0), i64::from(dest.w).min(frame_w - x0));
            let (mut j0, mut j1) = ((-ch - y0).max(0), i64::from(dest.h).min(screen_h - y0));
            if cut_top > 0 {
                j0 = j0.max(cut_top - i64::from(dest.y));
            }
            if cut_bottom > 0 {
                j1 = j1.min(rows_px(p.kitty.rows) - cut_bottom - i64::from(dest.y));
            }
            self.push_image(
                imgs,
                epoch,
                p.image,
                &view,
                draw,
                (x0, y0),
                (i0, i1, j0, j1),
                p.z,
                snap,
            );
        }
        // **kitty's Unicode placeholders** (`[[source: kitty-graphics]]`,
        // "Unicode placeholders"): each run of placeholder cells on screen
        // shows its strip of the virtual placement it names, drawn as a
        // placement anchored where the placement's top-left cell would be
        // -- `col` columns left of the run and `row` rows above it -- and
        // clipped to the run's cells, so the strips of one image meet with
        // no seam and sample one table. Only while a virtual placement is
        // held: a screen with none asks one compare a frame.
        if term.kitty_virtuals() {
            let back = term.looking_back(back);
            let vp = term.view(back);
            // The leaving row too, draw row -1: the line above the window,
            // `view(back + 1).row(0)`, the cells the cell path slides in
            // from the ring, resolved from those cells as when that line
            // was on screen. Only where the history holds one: the clamp
            // would read the screen's own first row there.
            let above = Rows::of_count(back.index() + 1);
            let leaving = (term.looking_back(above) == above)
                .then(|| term.view(above).row(Row::from_index(0)));
            let rows = (0..self.rows.index()).map(|r| (r as i64, vp.row(Row::from_index(r))));
            for (r, row) in leaving.map(|row| (-1, row)).into_iter().chain(rows) {
                self.place_placeholders(term, imgs, epoch, (r, row), snap);
            }
        }
        // The text's order: by `z`, then by the client's id; stable, so
        // placement order breaks what is left.
        self.image_places.sort_by_key(|p| (p.z, p.tie.raw()));
        // Drop snapshots of images no longer on screen, so an image scrolled off
        // the top does not hold its pixels until the next epoch change -- which
        // on a terminal showing one image is forever. The clear-every-frame
        // first version avoided this by accident; keeping the cache across
        // frames reintroduces it unless the leavers are pruned. An image that
        // oscillates on and off screen re-snapshots on each re-entry, which is
        // the price of not holding it, and far cheaper than a rebuild a frame.
        if snap == Snapshots::Kept {
            let places = &self.image_places;
            self.image_uploads
                .retain(|u| places.iter().any(|p| p.id == u.id));
        }
    }

    /// **The placeholder runs of draw row `r`** into this frame's list,
    /// `row` its cells: each run's strip of the virtual placement it
    /// names, clipped to the run's cells, `r` rows from the top of the
    /// frame -- `-1` for the leaving row the cell path slides in, which a
    /// frame that is not sliding draws at a negative y and every path
    /// clips.
    fn place_placeholders(
        &mut self,
        term: &Term<'_>,
        imgs: &ImageStore<'_>,
        epoch: ImageGen,
        (r, row): (i64, &[PackedCell]),
        snap: Snapshots,
    ) {
        let cw = i64::from(self.metrics.cell_w.value());
        let ch = i64::from(self.metrics.cell_h.value());
        let cell = |p: Px| ImagePx::of(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let (cell_w, cell_h) = (cell(self.metrics.cell_w), cell(self.metrics.cell_h));
        let frame_w = i64::from(self.frame_size().0.value());
        for run in term.placeholder_runs(row) {
            let Some(v) = term.kitty_virtual(run.image, run.placement) else {
                continue;
            };
            let Some(view) = imgs.image(v.image) else {
                continue;
            };
            let draw = v.place.draw(view.width, view.height, cell_w, cell_h);
            let dest = draw.dest;
            if dest.is_empty() {
                continue;
            }
            let at = run.at.index() as i64;
            let x0 = (at - run.col.index() as i64) * cw + i64::from(dest.x);
            let y0 = (r - run.row.index() as i64) * ch + i64::from(dest.y);
            let (left, top) = (at * cw, r * ch);
            let right = left + run.cells.index() as i64 * cw;
            let (i0, i1) = (
                (left - x0).max(0),
                (right - x0).min(i64::from(dest.w)).min(frame_w - x0),
            );
            let (j0, j1) = ((top - y0).max(0), (top + ch - y0).min(i64::from(dest.h)));
            self.push_image(
                imgs,
                epoch,
                v.image,
                &view,
                draw,
                (x0, y0),
                (i0, i1, j0, j1),
                // Never below its own cells' backgrounds: "The background
                // color is interpreted as the background color, visible if
                // the image is transparent" ("Unicode placeholders"), so the
                // image is over it. Otherwise the virtual placement's `z`,
                // which kitty sets to -1 for every placeholder image
                // (`[[source: kitty-graphics-c]]` `grman_put_cell_image`),
                // where the text says nothing; an open fork.
                v.z.max(ZIndex::of(ImageLayer::BELOW_BACKGROUNDS)),
                snap,
            );
        }
    }

    /// **One placement's visible part into this frame's list**: its
    /// tables, from `ImageDraw::nearest`, and its pixels snapshotted once
    /// for the GPU. `x0` and `y0` are the drawn rectangle's top-left on
    /// screen and `i0..i1` by `j0..j1` the part of it shown, in drawn pixels
    /// from that corner -- what a placement on screen and a placeholder's
    /// strip both reduce to.
    #[allow(clippy::too_many_arguments)]
    fn push_image(
        &mut self,
        imgs: &ImageStore<'_>,
        epoch: ImageGen,
        id: ImageId,
        view: &baud_domain::ImageView<'_>,
        draw: ImageDraw,
        (x0, y0): (i64, i64),
        (i0, i1, j0, j1): (i64, i64, i64, i64),
        z: ZIndex,
        snap: Snapshots,
    ) {
        if i0 >= i1 || j0 >= j1 {
            return;
        }
        let (src, dest) = (draw.src, draw.dest);
        let clamp16 = |v: i64| v.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16;
        let side = |v: i64| u16::try_from(v).unwrap_or(u16::MAX);
        let texel = |v: u32| ImagePx::of(u16::try_from(v).unwrap_or(u16::MAX));
        let at = MapAt(self.image_maps.len());
        let span = |v: i64| u32::try_from(v).unwrap_or(0);
        self.image_maps
            .extend((span(i0)..span(i1)).map(|i| ImageDraw::nearest(i, dest.w, src.x, src.w)));
        self.image_maps
            .extend((span(j0)..span(j1)).map(|j| ImageDraw::nearest(j, dest.h, src.y, src.h)));
        let scale = match (dest.w == src.w, dest.h == src.h) {
            (true, true) => ImageScale::Unit {
                x: texel(src.x + span(i0)),
                y: texel(src.y + span(j0)),
            },
            _ => ImageScale::Mapped,
        };
        self.image_places.push(ImagePlacement {
            id,
            epoch,
            x: clamp16(x0 + i0),
            y: clamp16(y0 + j0),
            w: side(i1 - i0),
            h: side(j1 - j0),
            alpha: view.alpha,
            z,
            layer: ImageLayer::of(z),
            tie: imgs.kitty_names(id).map_or(KittyId::NONE, |(k, _)| k),
            draw,
            scale,
            map: at,
        });
        // Snapshot the pixels once per referenced `(id, epoch)`: the GPU
        // reads this to upload, the CPU reads the term itself. The words
        // are the store's own, straight: the fragment premultiplies, so a
        // clear (unpainted Sixel) pixel, `0x00000000`, leaves the cell
        // showing through and a partial one blends by its alpha.
        if snap == Snapshots::Kept
            && !self
                .image_uploads
                .iter()
                .any(|u| u.id == id && u.epoch == epoch)
        {
            let pixels = view.pixels.iter().map(|p| p.bits()).collect();
            self.image_uploads.push(ImageUpload {
                id,
                epoch,
                w: view.width.raw(),
                h: view.height.raw(),
                pixels,
            });
            self.image_snapshots = self.image_snapshots.inc();
        }
    }

    /// A placement's nearest-neighbour tables: the texel x of each of its
    /// `w` visible drawn columns, and the texel y of each of its `h` rows.
    pub fn image_map(&self, p: &ImagePlacement) -> (&[u32], &[u32]) {
        let (w, h) = (usize::from(p.w), usize::from(p.h));
        let at = p.map.0;
        let xs = self.image_maps.get(at..at + w).unwrap_or(&[]);
        let ys = self.image_maps.get(at + w..at + w + h).unwrap_or(&[]);
        (xs, ys)
    }

    /// The [`Blit`] of a placement, writing the rows of `band`.
    fn blit_of(&self, p: &ImagePlacement, band: Band) -> Blit<'_> {
        let (xs, ys) = self.image_map(p);
        Blit {
            at: (i32::from(p.x), i32::from(p.y)),
            w: Px::of(u32::from(p.w)),
            h: Px::of(u32::from(p.h)),
            texels: match p.scale {
                ImageScale::Unit { x, y } => Texels::Unit {
                    x: Px::of(u32::from(x.raw())),
                    y: Px::of(u32::from(y.raw())),
                },
                ImageScale::Mapped => Texels::Mapped { xs, ys },
            },
            band,
        }
    }

    /// The visible image placements this frame, in device-pixel screen rects.
    pub fn image_placements(&self) -> &[ImagePlacement] {
        &self.image_places
    }

    /// Each referenced image's straight pixels, for a GPU to upload once.
    pub fn image_uploads(&self) -> &[ImageUpload] {
        &self.image_uploads
    }

    /// How many image pixel-snapshots have been built over this renderer's
    /// life. A static image on screen bumps it once, not once a frame, which is
    /// how the snapshot-cache gate distinguishes "built once" from "rebuilt".
    pub fn image_snapshots(&self) -> Tally {
        self.image_snapshots
    }

    /// Blits every image placement over the text over the frame, after
    /// the glyphs, in order, reading the list [`Renderer::set_images`] built
    /// and the term's own pixels.
    fn paint_images(&self, term: &Term<'_>, target: &mut Target<'_>) {
        let page = self.page();
        let imgs = term.images();
        let (_, h) = self.frame_size();
        let all = Band {
            top: Px::of(0),
            bottom: h,
        };
        for p in self
            .image_places
            .iter()
            .filter(|p| p.layer == ImageLayer::Over)
        {
            let Some(view) = imgs.image(p.id) else {
                continue;
            };
            target.blit_image(&self.blit_of(p, all), &view, self.blend, page);
        }
    }

    /// Blits the placements under the text that cover `row`'s band into
    /// it, in order: a repainted row's, over its background. Those below
    /// the non-default backgrounds first, and then those backgrounds over
    /// them, at their own alpha, as over the desktop where there is no
    /// image (the owner's pick of 2026-10-16), so the default ones show the
    /// image and the others show it through their alpha; then the rest.
    /// The GPU paths draw the same steps.
    fn paint_under(&self, imgs: &ImageStore<'_>, target: &mut Target<'_>, row: Row, page: Page) {
        let band = self.band(row);
        // The cells the placements below the backgrounds reach in this
        // row: the only ones cleared and composited again, so a frame pays
        // for the image's columns and not the row's.
        let cw = i64::from(self.metrics.cell_w.value()).max(1);
        let reach = self
            .image_places
            .iter()
            .filter(|p| p.layer == ImageLayer::Below && covers(p, band))
            .map(|p| (i64::from(p.x), i64::from(p.x) + i64::from(p.w)))
            .reduce(|(a, b), (x0, x1)| (a.min(x0), b.max(x1)))
            .map_or(0..0, |(a, b)| {
                let cell = |v: i64| usize::try_from(v.max(0) / cw).unwrap_or(0);
                cell(a)..cell(b + cw - 1)
            });
        let set = |c: usize| reach.contains(&c) && self.set_at(row, Col::at_index(c));
        let mut below = false;
        for p in self.image_places.iter().filter(|p| covers(p, band)) {
            if p.layer == ImageLayer::Under && below {
                self.cover_set(target, row, page, set);
                below = false;
            }
            if p.layer == ImageLayer::Over {
                continue;
            }
            let Some(view) = imgs.image(p.id) else {
                continue;
            };
            if p.layer == ImageLayer::Below && !below {
                self.clear_set(target, row, page, set);
                below = true;
            }
            target.blit_image(&self.blit_of(p, band), &view, self.blend, page);
        }
        if below {
            self.cover_set(target, row, page, set);
        }
    }

    /// Whether `row`'s cell at `col` draws a background that is not the
    /// default one ([`Ground::Set`]).
    fn set_at(&self, row: Row, col: Col) -> bool {
        let at = self.row_cells(row).start + col.index();
        self.ground(row, col, self.prev[at]) == Ground::Set
    }

    /// On the glass, clears the cells of `row` that `set` names, its
    /// [`Ground::Set`] ones, to nothing before a placement below the
    /// backgrounds is drawn there: the background pass drew them, and
    /// [`Renderer::cover_set`] composites them again over the image, so
    /// what is under the image there is what is under the window -- the
    /// desktop -- and not the cell's background twice. On an opaque page
    /// every background is opaque and replaces what is under it, so
    /// nothing is cleared.
    fn clear_set(
        &self,
        target: &mut Target<'_>,
        row: Row,
        page: Page,
        set: impl Fn(usize) -> bool,
    ) {
        if page == Page::Glass {
            target.clear_cells(self.band(row), self.cols, self.metrics.cell_w, set);
        }
    }

    /// Composites the backgrounds of the cells of `row` that `set` names
    /// over what a placement below the backgrounds drew there, at their
    /// own alpha: a fill on an opaque page, where they are opaque.
    fn cover_set(
        &self,
        target: &mut Target<'_>,
        row: Row,
        page: Page,
        set: impl Fn(usize) -> bool,
    ) {
        let range = self.row_cells(row);
        let bg = &self.bg[range];
        match page {
            Page::Opaque => target.fill_cells(self.band(row), bg, self.metrics.cell_w, set),
            Page::Glass => {
                target.over_cells(self.band(row), bg, self.metrics.cell_w, self.blend, set);
            }
        }
    }

    /// Whether the cell at `row` and `col`, as the last convert drew it,
    /// draws the default background: its rendition, the selection and the
    /// block cursor, by [`Resolved::ground`]. Asked only of a row a
    /// placement below the backgrounds covers, so no frame without one
    /// pays a store a cell for it.
    fn ground(&self, row: Row, col: Col, cell: PackedCell) -> Ground {
        let res = self
            .styles
            .get(cell.style.index())
            .copied()
            .unwrap_or_default();
        let selected = match self
            .highlight
            .is_some_and(|h| h.selects(row, col, cell.content))
        {
            true => Selected::Yes,
            false => Selected::No,
        };
        let block = match self.last_cursor {
            Some((r, cc, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock))
                if r == row && cc == col =>
            {
                BlockCursor::Yes(None)
            }
            _ => BlockCursor::No,
        };
        res.ground(selected, block)
    }

    /// Each cell's [`Ground`], row-major, as the last convert drew it:
    /// what the instance path's second background pass reads.
    pub fn grounds(&self, out: &mut Vec<Ground>) {
        out.clear();
        let cols = self.cols.index();
        for (i, &cell) in self.prev.iter().enumerate() {
            out.push(self.ground(Row::from_index(i / cols), Col::at_index(i % cols), cell));
        }
    }

    /// Sizes the renderer to the terminal's grid, when it differs. Every
    /// frame goes through it -- [`Renderer::convert`] calls it, and a path
    /// that never converts, the cell path, must call it before it asks
    /// [`Renderer::pixel_size`] for a surface: the app once built its
    /// surface for the 80x24 grid the renderer started with, and the layer
    /// stretched those cells over a 200x50 window.
    pub fn fit(&mut self, term: &Term<'_>) {
        let (cols, rows) = term.geometry();
        if cols.index() != self.cols.index() || rows.index() != self.rows.index() {
            self.resize(cols, rows);
        }
    }

    /// Reads the terminal and converts the rows that changed since the
    /// last frame into instances and background words, without painting.
    ///
    /// A GPU path takes [`Renderer::instances`], [`Renderer::background`]
    /// and [`Renderer::atlas`] from here; the CPU path calls
    /// [`Renderer::paint`].
    pub fn convert(&mut self, term: &Term<'_>, back: Rows, damage: Damage) -> Converted {
        self.convert_with(term, back, damage, ImageRows::Ignored)
    }

    fn convert_with(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        damage: Damage,
        images: ImageRows,
    ) -> Converted {
        let t0 = Instant::now();
        self.fit(term);
        let mut styles = core::mem::take(&mut self.styles);
        if styles.len() != term.styles().len() {
            styles = vec![Resolved::default(); term.styles().len()];
            self.styles_stamp = StyleStamp::default();
        }
        let mut stamp = self.styles_stamp;
        let sync = self.sync_styles(term, &mut styles, &mut stamp);
        (self.styles, self.styles_stamp) = (styles, stamp);
        let (cols, rows) = term.geometry();
        let ncols = cols.index();
        // Each row read where the core holds it, through the rotation and
        // the ring, `back` lines into the history when the user is looking
        // back; the copy into `cur` is this path's own frame-to-frame damage
        // record, not the core's, and a change of `back` dirties the rows it
        // moves the way any other change does. The window is the core's
        // clamp of `back` -- what `Term::view` draws -- so the cursor below
        // agrees with it: on the alternate screen, which keeps no history, a
        // window left scrolled back on the primary is the live screen, and
        // the program that owns it owns the cursor.
        let back = term.looking_back(back);
        let vp = term.view(back);
        for r in 0..rows.index() {
            self.cur[r * ncols..(r + 1) * ncols].copy_from_slice(vp.row(Row::from_index(r)));
        }
        // An input method's composition, on the live view: the cells under it
        // converted blank in their own renditions, so their backgrounds stay
        // and their glyphs give way to its records, built below. The cell it
        // starts on lends the records its rendition.
        let composition = self
            .composition
            .clone()
            .filter(|c| back.index() == 0 && c.row.index() < rows.index());
        let mut under = PackedCell::EMPTY;
        if let Some(c) = &composition {
            let (start, end) = c.columns(cols);
            let at = c.row.index() * ncols;
            if let Some(&cell) = self.cur.get(at + start) {
                under = cell;
            }
            for cell in &mut self.cur[at + start..at + end] {
                *cell = PackedCell {
                    content: baud_domain::Content::EMPTY,
                    style: cell.style,
                    flags: baud_domain::CellFlags::NONE,
                };
            }
        }
        if self.theme.finds_pictures() {
            self.pictures.find(term, &vp, self.video);
        } else {
            self.pictures.clear(cols, rows);
        }
        // No cursor while looking back: it belongs to the live screen, which
        // is `back` lines below the window.
        let live: Cursor = (term.cursor_visible() && back.index() == 0).then(|| {
            let (r, c) = match &composition {
                Some(c) => c.cursor(cols),
                None => term.cursor(),
            };
            (r, c, term.cursor_style())
        });
        self.set_cursor_glow_at(live);
        let cursor: Cursor = live.filter(|&(_, _, style)| self.cursor_drawn(style));
        let opacity = cursor.map_or(Chan::from_u8(0xFF), |(_, _, s)| self.cursor_opacity(s));
        // Every slot resolved again -- a theme, a video or a palette changed
        // -- repaints every cell whose words did not change, and so does an
        // eviction, which gives a slot some cell names another rendition, and
        // so does a sweep that freed a slot: a later write can give it
        // another rendition where the last frame drew the old one, under the
        // same word (`Reclaims::recolours_since`). A slot filled since
        // otherwise changes no drawn cell: no cell named it before, and a
        // cell that names it now is a changed word.
        //
        // The screen fade veils every row, so a change to it -- spawned,
        // stepped or cleared -- makes the whole frame dirty. It is not a
        // per-row predicate like the trail's band overlap.
        //
        // A moved cluster epoch is the eviction's twin for marks: a slot a
        // sweep or a reset freed can hold another cluster now, under the
        // word a cell that named it in the last frame still has.
        let clusters = Some(term.cluster_epoch());
        let full = damage == Damage::Full
            || self.known == Prev::Unknown
            || sync.pass == Pass::Full
            || sync.evicted == Evicted::Yes
            || self.fade != self.last_fade
            || clusters != self.last_clusters;
        let cursor_row = |c: Cursor| c.map(|(r, _, _)| r.index());
        let mut rows_dirty = Tally::default();
        let mut rows_partial = Tally::default();
        let mut exceptions = Exceptions::default();
        // Whether a placement under the text arrived or changed: once a
        // frame, so a frame with none asks no row about it.
        let under_new = images == ImageRows::Repainted
            && self
                .image_places
                .iter()
                .any(|p| p.layer.under_text() && !self.last_image_places.contains(p));
        for r in 0..rows.index() {
            let range = self.row_cells(Row::from_index(r));
            let changed = self.cur[range.clone()] != self.prev[range];
            let cursor_here = cursor_row(cursor) == Some(r);
            let cursor_was = cursor_row(self.last_cursor) == Some(r);
            let cursor_moved = cursor_here != cursor_was
                || (cursor_here
                    && (cursor != self.last_cursor || opacity != self.last_cursor_opacity));
            let glow_row = |g: Option<(Row, Col, CursorStyle, CursorPhase)>| {
                g.is_some_and(|(gr, _, _, _)| gr.index() == r)
            };
            let glow_moved =
                (glow_row(self.glow) || glow_row(self.last_glow)) && self.glow != self.last_glow;
            let row = Row::from_index(r);
            let selected = |h: Option<Highlight>| h.filter(|h| h.covers_row(row));
            let selection_moved = selected(self.highlight) != selected(self.last_highlight);
            let band = self.band(row);
            let slice = |b: Option<Bar>| b.and_then(|b| b.slice(band.top, band.bottom));
            let bar_moved = slice(self.bar) != slice(self.last_bar);
            let under_trail =
                |d: Option<Drawn>| d.is_some_and(|d| d.overlaps(band.top, band.bottom));
            let trail_moved = under_trail(self.trail) != under_trail(self.last_trail)
                || (under_trail(self.trail) && self.trail != self.last_trail);
            // A picture is the block's, so a row can move in or out of one
            // without a cell of its own changing.
            let pictured = self.pictures.row_changed(row);
            // A composition that appeared, changed or went: the row it is on
            // and the row it was on, whether or not a cell word moved -- a
            // composition over the same text changes none.
            let on = |c: &Option<Composition>| c.as_ref().is_some_and(|c| c.row == row);
            let composed = (on(&composition) || on(&self.last_composition))
                && composition != self.last_composition;
            // An image the CPU path blitted over this row last frame is not
            // there now: see `render_at`.
            let imaged = images == ImageRows::Repainted
                && left_from(&self.image_places, &self.last_image_places, band);
            // And one the CPU path blits over this row that is partly
            // transparent: repainted every frame, and counted.
            let partial = images == ImageRows::Repainted && partial_over(&self.image_places, band);
            // And one under the text that arrived or changed on this row.
            let imaged = imaged
                || (under_new && arrived_under(&self.image_places, &self.last_image_places, band));
            if partial {
                rows_partial.bump();
            }
            self.dirty[r] = full
                || changed
                || cursor_moved
                || glow_moved
                || selection_moved
                || bar_moved
                || trail_moved
                || pictured
                || imaged
                || partial
                || composed;
        }
        if images == ImageRows::Repainted {
            self.last_image_places.clone_from(&self.image_places);
        }
        self.compose_inst.clear();
        if let Some(c) = &composition {
            let styles = core::mem::take(&mut self.styles);
            let mut records = core::mem::take(&mut self.compose_inst);
            crate::overlay::composition_records(
                self,
                c,
                under,
                &styles,
                cursor,
                cols,
                Px::of(0),
                &mut records,
            );
            (self.styles, self.compose_inst) = (styles, records);
        }
        for r in 0..rows.index() {
            if self.dirty[r] {
                self.convert_row(term, Row::from_index(r), cursor, &mut exceptions);
                rows_dirty.bump();
            }
        }
        self.blink_seen = self
            .row_blinks
            .iter()
            .fold(Blinking::UNSEEN, |a, &b| a.with(b));
        core::mem::swap(&mut self.prev, &mut self.cur);
        self.known = Prev::Known;
        self.last_cursor = cursor;
        self.last_cursor_opacity = opacity;
        self.last_glow = self.glow;
        self.last_highlight = self.highlight;
        self.last_bar = self.bar;
        self.last_trail = self.trail;
        self.last_fade = self.fade;
        self.last_clusters = clusters;
        self.last_composition = composition;
        Converted {
            rows_dirty,
            rows_partial,
            rows: Tally::of(rows.index()),
            convert: t0.elapsed(),
            exceptions,
        }
    }

    /// The page this frame composites over: the glass whenever the theme
    /// dims the window. At a dim of 255 every word `paint` fills with is
    /// opaque -- each background takes the dim's alpha, and a swap or a
    /// picture makes one opaque -- so nothing it draws over is translucent.
    fn page(&self) -> Page {
        if self.theme.bg_alpha.get() == 0xFF {
            Page::Opaque
        } else {
            Page::Glass
        }
    }

    /// Paints the rows the last [`Renderer::convert`] found dirty into
    /// `target`, which must still hold the frame before.
    pub fn paint(&mut self, target: &mut Target<'_>, converted: Converted) -> Frame {
        self.paint_rows(target, converted, NoUnder)
    }

    /// [`Renderer::paint`], with what is drawn between a repainted row's
    /// background and its glyphs: nothing, or the images under the text.
    /// One copy for each, so a frame with no image under the text runs the
    /// loop it ran before there were any.
    fn paint_rows(
        &mut self,
        target: &mut Target<'_>,
        converted: Converted,
        under: impl Under,
    ) -> Frame {
        let t1 = Instant::now();
        let mut frame = Frame {
            rows_dirty: converted.rows_dirty,
            rows_partial: converted.rows_partial,
            rows: converted.rows,
            convert: converted.convert,
            exceptions: converted.exceptions,
            ..Frame::default()
        };
        let page = self.page();
        for r in 0..self.rows.index() {
            let row = Row::from_index(r);
            frame.instances.add(Tally::of(self.per_row[r].len()));
            for inst in &self.per_row[r] {
                match inst.kind {
                    Kind::Color => frame.colored.bump(),
                    Kind::Solid => frame.solids.bump(),
                    Kind::Mask | Kind::Quad | Kind::Band | Kind::Particle => {}
                }
            }
            if !self.dirty[r] {
                continue;
            }
            let band = self.band(row);
            let margin = self.clear_color();
            target.fill_band(
                band,
                &self.bg[self.row_cells(row)],
                self.metrics.cell_w,
                margin,
            );
            under.paint(self, target, row, page);
            // In the order `instances()` hands the GPU instance path: the
            // row's own, then the trail under the scrollbar, then the screen
            // fade over everything. The trail and the fade dirtied these rows
            // (`trail_moved`, and a fade makes the frame full), so a row
            // repainted without them would draw them out.
            for inst in &self.per_row[r] {
                target.over(inst, &self.atlas, self.blend, page, band);
            }
            // The composition's records, over its row: `instances()` hands
            // the GPU instance path them in the same place.
            for inst in &self.compose_inst {
                target.over(inst, &self.atlas, self.blend, page, band);
            }
            // On the CPU an effect is its emitter, the quad it rides: the
            // effect is the GPU's.
            // A repeat pass's quad carries the cap, as its record does.
            let fill = match self.strength() {
                Strength::Full => self.cursor_fill,
                Strength::UnderThreshold => self
                    .cursor_fill
                    .with_alpha(flash::capped(self.cursor_fill.a(), self.flash_cap)),
            };
            match self.trail {
                Some(Drawn::Smear(s)) => {
                    target.quad(&s, fill, self.blend, page, band);
                }
                Some(Drawn::Vfx(v, ..)) => {
                    target.quad(&v.emitter, fill, self.blend, page, band);
                }
                _ => {
                    for inst in &self.trail_inst {
                        target.over(inst, &self.atlas, self.blend, page, band);
                    }
                }
            }
            for inst in self
                .glow_inst
                .iter()
                .chain(&self.bar_inst)
                .chain(&self.fade_inst)
            {
                target.over(inst, &self.atlas, self.blend, page, band);
            }
        }
        frame.composite = t1.elapsed();
        frame.cache = self.cache.stats();
        frame.atlas = self.atlas.stats();
        frame
    }

    /// Every instance of the current frame, row by row: what a GPU path
    /// uploads whole (D2: nobody uploads partial ranges).
    pub fn instances(&self) -> impl Iterator<Item = &Instance> + '_ {
        self.per_row
            .iter()
            .flatten()
            .chain(self.compose_inst.iter())
            .chain(self.trail_inst.iter())
            .chain(self.glow_inst.iter())
            .chain(self.bar_inst.iter())
            .chain(self.fade_inst.iter())
    }

    /// The background word of every cell, row-major, `cols` by `rows`.
    pub fn background(&self) -> &[Argb] {
        &self.bg
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// The atlas, for a copy that takes its dirty rectangles.
    pub fn atlas_mut(&mut self) -> &mut Atlas {
        &mut self.atlas
    }

    /// The grid the last frame was converted for.
    pub fn grid(&self) -> (Cols, Rows) {
        (self.cols, self.rows)
    }

    /// A whole cluster -- a base and its attached codepoints -- shaped by
    /// itself: every glyph with its atlas slot and its offsets in pixels.
    /// What the overlay draws for a marked cell; the shaper is the one
    /// definition of what a family or a base with a mark looks like.
    pub fn cluster(
        &mut self,
        cps: &[Codepoint],
        style: FaceStyle,
        span: CellSpan,
    ) -> Vec<ClusterGlyph> {
        let Some(&base) = cps.first() else {
            return Vec::new();
        };
        if crate::boxes::covers(base) {
            // A box, block or braille cell is baud's own mask, whatever is
            // attached to it: the shaper is not consulted.
            return vec![ClusterGlyph {
                id: GlyphId::from_shaper(0),
                source: GlyphSource::Procedural,
                slot: self.procedural(base),
                dx: 0,
                dy: 0,
                phase: Phase::ZERO,
            }];
        }
        let face = self.face_for(cps, style);
        let phase = self.phase_in(face, base, span);
        let clusters = [ClusterIn {
            cps: cps.to_vec(),
            at: CellIx(0),
        }];
        // A marked cell alone: the script its own code points name (UAX #24
        // 5.2), or `DFLT`'s for one that names none.
        let shaped = self.shape(face, Script::of_cluster(cps), &clusters, ..);
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let mut out = Vec::new();
        for p in shaped.glyphs() {
            let slot = self.atlas.slot(f, face, p.id, scale, phase);
            out.push(ClusterGlyph {
                id: p.id,
                source: GlyphSource::Face(face),
                slot,
                dx: scale.px(f32::from(p.dx)).round() as i32,
                dy: scale.px(f32::from(p.dy)).round() as i32,
                phase,
            });
        }
        out
    }

    /// The glyph a codepoint draws as a cluster of one: the
    /// face the stack selects, shaped alone so a single substitution still
    /// applies, its atlas slot with the shaper's offsets folded into the
    /// placement. `None` when it draws nothing. What the cell path's map
    /// holds.
    pub fn single(&mut self, cp: Codepoint, style: FaceStyle, span: CellSpan) -> Option<Single> {
        if crate::boxes::covers(cp) {
            // `PairRole::Neither` is load-bearing: with both pair bits clear
            // the cell is never a ligature candidate, never gathered into a
            // window and never re-shaped by a font, so the face field is
            // never read for it.
            return Some(Single {
                slot: self.procedural(cp),
                glyph: GlyphId::from_shaper(0),
                role: PairRole::Neither,
                face: FaceIx::PRIMARY,
            });
        }
        let face = self.face_for(core::slice::from_ref(&cp), style);
        let clusters = [ClusterIn {
            cps: vec![cp],
            at: CellIx(0),
        }];
        // A code point alone: its own script, or `DFLT`'s for one with none
        // -- the specification's "text [that] does not have a specific
        // script". In a row, a Common character takes its neighbours'
        // script instead. The two agree wherever the two select the same
        // lookups; on STIX Two Math, the one embedded face whose scripts
        // differ, the only lookup `DFLT` adds to `latn` is a ligature that
        // begins on a Greek capital, which no Common code point is.
        let shaped = self.shape(face, Script::of(cp), &clusters, ..);
        let p = *shaped.glyphs().first()?;
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let role = self.pairs[face.index()].role(p.id);
        let phase = self.phase_in(face, cp, span);
        let slot = self
            .atlas
            .slot(f, face, p.id, scale, phase)
            .map(|mut slot| {
                slot.left += scale.px(f32::from(p.dx)).round() as i32;
                slot.top -= scale.px(f32::from(p.dy)).round() as i32;
                slot
            });
        Some(Single {
            slot,
            glyph: p.id,
            role,
            face,
        })
    }

    /// Whether two adjacent glyphs on the face a map entry names can
    /// begin a substitution there.
    pub fn admits(&self, face: Code, a: GlyphId, b: GlyphId) -> bool {
        usize::try_from(face.0)
            .ok()
            .and_then(|i| self.pairs.get(i))
            .is_some_and(|p| p.admits(a, b))
    }

    /// The longest rule the face a map entry names can match, in glyphs.
    pub fn extent(&self, face: Code) -> Option<Count> {
        usize::try_from(face.0)
            .ok()
            .and_then(|i| self.extents.get(i))
            .copied()
            .flatten()
    }

    /// A window of cells -- each a base with its attached codepoints --
    /// shaped together as one run on the face named, answered per
    /// cell: the glyphs the shaper placed at that cell, with slots and
    /// offsets. A cell with no glyph of its own -- swallowed into a
    /// ligature -- answers an empty list.
    pub fn window(
        &mut self,
        face: Code,
        cells: &[(Vec<Codepoint>, CellSpan)],
    ) -> Vec<Vec<ClusterGlyph>> {
        // **Shaped in chunks, because `Shaped` holds `PLACED_MAX` glyphs
        // and a longer run used to lose its tail in silence.** `shape`
        // stops pushing once the buffer is full and breaks, so a window of
        // 120 cells was shaped for 96 of them and the rest fell back to
        // the glyph the codepoint has alone. On a band that runs the width
        // of the screen -- asciiquarium's waterline is exactly that -- the
        // right-hand quarter drew unshaped while the left drew shaped, and
        // nothing said so: the pixel gate's scene was forty columns wide,
        // which is narrower than the bug.
        //
        // Each chunk is shaped with `ext` cells of context on either side
        // -- the face's longest rule, the same span the window itself is
        // built from -- and only the middle is kept, so a substitution
        // that straddles a chunk boundary still sees what it needs.
        //
        // **And in pieces of one selection of the face's lookups**, as a row
        // is: a window's cells settle their scripts among themselves (UAX
        // #24, [`baud_font::resolve`]), and a piece is cut where the lookups
        // the script selects change, so no chunk shapes across that edge.
        let Some(&facex) = usize::try_from(face.0).ok().and_then(|i| self.faces.get(i)) else {
            return Vec::new();
        };
        let mut out: Vec<Vec<ClusterGlyph>> = vec![Vec::new(); cells.len()];
        let scripts: Vec<Script> = match self.selections.get(facex.index()) {
            Some(sel) if sel.varies == Varies::Yes => {
                let unresolved: Vec<Unresolved> =
                    cells.iter().map(|(cps, _)| Unresolved::of(cps)).collect();
                let mut settled = vec![Script::COMMON; cells.len()];
                baud_font::resolve(&unresolved, &mut settled);
                settled
            }
            _ => vec![Script::COMMON; cells.len()],
        };
        let picks: Vec<Selection> = match self.selections.get(facex.index()) {
            Some(sel) => scripts.iter().map(|&s| sel.by_script[s]).collect(),
            None => vec![Selection::default(); cells.len()],
        };
        let mut lo = 0;
        while lo < cells.len() {
            let script = scripts[lo];
            let mut hi = lo + 1;
            while hi < cells.len() && picks[hi] == picks[lo] {
                hi += 1;
            }
            self.window_piece(face, facex, script, &cells[lo..hi], &mut out[lo..hi]);
            lo = hi;
        }
        out
    }

    /// One piece of [`Renderer::window`], in one selection of the face's
    /// lookups, shaped in chunks with the face's longest rule of context
    /// either side.
    fn window_piece(
        &mut self,
        face: Code,
        facex: FaceIx,
        script: Script,
        cells: &[(Vec<Codepoint>, CellSpan)],
        out: &mut [Vec<ClusterGlyph>],
    ) {
        let ext = self
            .extent(face)
            .and_then(|e| usize::try_from(e.value()).ok())
            .unwrap_or(1)
            .max(1);
        let scale = self.scales[facex.index()];
        let f = self.stack.face(facex);
        // Half the glyph budget in cells, so a cell that shapes to two
        // glyphs still fits, and the context on both sides fits with it.
        let step = (PLACED_MAX / 2).saturating_sub(2 * ext).max(1);
        let mut at = 0;
        while at < cells.len() {
            let end = (at + step).min(cells.len());
            let lo = at.saturating_sub(ext);
            let hi = (end + ext).min(cells.len());
            let clusters: Vec<ClusterIn> = cells[lo..hi]
                .iter()
                .enumerate()
                .map(|(i, (cps, _))| ClusterIn {
                    cps: cps.clone(),
                    at: CellIx(u8::try_from(i).unwrap_or(u8::MAX)),
                })
                .collect();
            // What the context cells drop is counted by the chunk that
            // owns them, so a mark is counted once however the window is
            // cut.
            let own = CellIx(u8::try_from(at - lo).unwrap_or(u8::MAX))
                ..CellIx(u8::try_from(end - lo).unwrap_or(u8::MAX));
            let shaped = self.shape(facex, script, &clusters, own);
            for p in shaped.glyphs() {
                let cell = lo + usize::from(p.cell.0);
                // Only this chunk's own cells; the context either side
                // belongs to the chunk that owns it.
                if cell < at || cell >= end {
                    continue;
                }
                let phase = cells
                    .get(cell)
                    .and_then(|(cps, span)| cps.first().map(|&b| self.phase_in(facex, b, *span)))
                    .unwrap_or(self.metrics.phase);
                let slot = self.atlas.slot(f, facex, p.id, scale, phase);
                if let Some(o) = out.get_mut(cell) {
                    o.push(ClusterGlyph {
                        id: p.id,
                        source: GlyphSource::Face(facex),
                        slot,
                        dx: scale.px(f32::from(p.dx)).round() as i32,
                        dy: scale.px(f32::from(p.dy)).round() as i32,
                        phase,
                    });
                }
            }
            at = end;
        }
    }

    /// The terminal's rendition table resolved through the theme and the
    /// live palette into `out`, one entry a slot, by [`Renderer::resolve_slot`]
    /// -- the rules `convert` applies a cell at a time, so the cell path draws
    /// a rendition the way the instance path does, without a second spelling
    /// of them. `out` is the memory a shader reads, so nothing is copied
    /// after this. Slots past `out` are not resolved; the count written is
    /// returned.
    ///
    /// **A slot is the same in every blink phase** (`Shapes::BLINKS`), and
    /// whether blinking text is on screen is not this table's answer: a slot
    /// no cell names is not on screen, and the table frees one only when it
    /// sweeps. It counts the blinking slots, and the overlay's walk answers.
    pub fn resolve_styles_into(&mut self, term: &Term<'_>, out: &mut [Resolved]) -> Tally {
        self.refresh_screen(term);
        let table = term.styles();
        let mut n = Tally::default();
        let mut blinking = Tally::default();
        for (slot, dst) in table.iter().zip(out.iter_mut()) {
            *dst = self.resolve_slot(term, slot.unwrap_or(baud_domain::StyleWord::DEFAULT));
            if dst.shapes.blinks() {
                blinking.bump();
            }
            n.bump();
        }
        self.blink_slots = blinking;
        n
    }

    /// **Bring one resolved copy of the rendition table up to date**, and say
    /// what that took. A copy resolved against this table, under the same
    /// theme, video and colours, resolves only the slots the core wrote since
    /// it was stamped (`Term::styles_written`) -- none, on a frame in which no
    /// rendition was minted. A copy of another table, or under another input,
    /// resolves every live slot. It never resolves a slot no cell can name.
    ///
    /// Where [`Renderer::resolve_styles_into`] resolved every slot of the
    /// table a frame -- 159 us of a 190 us idle frame at the app's 65,536
    /// slots -- this is a handful of compares on a steady screen. `out` is
    /// the memory a reader reads: the CPU path's own table, or a buffer set
    /// the GPU draws from, each with its own [`StyleStamp`].
    pub fn sync_styles(
        &mut self,
        term: &Term<'_>,
        out: &mut [Resolved],
        stamp: &mut StyleStamp,
    ) -> StyleSync {
        self.refresh_screen(term);
        self.bound_trail(term);
        let inputs = StyleInputs {
            theme: self.theme_gen,
            video: self.video,
            colors: term.color_epoch(),
        };
        let table = term.style_table();
        let now = term.style_writes();
        let slots = term.styles();
        // A copy of this table under these inputs replays; a reader ahead
        // of the log, which only a copy of another table can be, is told
        // every slot by `styles_written` itself.
        let fresh = stamp.table == Some(table) && stamp.inputs == Some(inputs);
        let mut sync = StyleSync::default();
        if !fresh {
            sync.pass = Pass::Full;
            stamp.blinking = Tally::default();
        }
        let since = if fresh {
            stamp.at
        } else {
            baud_domain::StyleWrites::NEVER
        };
        match term.styles_written(since) {
            baud_core::Written::Slots(parts) => {
                for &k in parts.iter().flat_map(|p| p.iter()) {
                    let Some(dst) = out.get_mut(k.index()) else {
                        continue;
                    };
                    let new = self.resolve_slot(
                        term,
                        slots[k.index()].unwrap_or(baud_domain::StyleWord::DEFAULT),
                    );
                    if fresh && dst.shapes.blinks() {
                        stamp.blinking.lose();
                    }
                    if new.shapes.blinks() {
                        stamp.blinking.bump();
                    }
                    *dst = new;
                    sync.resolved.bump();
                }
            }
            baud_core::Written::Every => {
                sync.pass = Pass::Full;
                stamp.blinking = Tally::default();
                for (slot, dst) in slots.iter().zip(out.iter_mut()) {
                    *dst = self.resolve_slot(term, slot.unwrap_or(baud_domain::StyleWord::DEFAULT));
                    if dst.shapes.blinks() {
                        stamp.blinking.bump();
                    }
                    sync.resolved.bump();
                }
            }
        }
        // The core's own count of what it reclaimed, and not writes past the
        // table's length: a table that sweeps passes its length without
        // evicting, and logs every slot a sweep frees.
        let reclaims = term.style_reclaims();
        if fresh && stamp.reclaims.is_some_and(|r| reclaims.recolours_since(r)) {
            sync.evicted = Evicted::Yes;
        }
        *stamp = StyleStamp {
            table: Some(table),
            at: now,
            inputs: Some(inputs),
            blinking: stamp.blinking,
            reclaims: Some(reclaims),
        };
        self.blink_slots = stamp.blinking;
        sync
    }

    /// One rendition resolved through the theme and the live palette to what
    /// a cell draws with, before anything a frame lays over it -- the
    /// selection, the cursor, the blink's phase ([`Resolved::drawn`]). The one
    /// spelling of the rendition rules: the cell path's table holds it a slot
    /// at a time and the CPU and instance paths take it a cell at a time.
    fn resolve_slot(&self, term: &Term<'_>, style: baud_domain::StyleWord) -> Resolved {
        let (fg, bg) = self.theme.resolve(
            style,
            self.video,
            |ch| term.palette(ch),
            |d| term.dynamic_color(d),
        );
        // **The contrast floor, on the opaque colours**: before the alpha
        // below, and before the underline, so a following underline takes
        // the floored text and `UNDERLINE_FOLLOWS_FG` still holds (note Y,
        // section 8). The words are the ones drawn, exchanged under `SGR 7`
        // and under `?5` by the one rule the colours took.
        let (text_word, page_word) = self.video.drawn(style.attrs(), style.fg(), style.bg());
        // A box-drawing line draws the text as asked (`Line`).
        let line = fg;
        let fg = self.theme.floor.lift(fg, bg, (text_word, page_word));
        let opaque_bg = self.theme.opaque_bg(style, self.video);
        // **The floor again, on the glass** (Jev, logged: 0.96 and 0.99),
        // measured against the page at its alpha over anything from black
        // to white (Jev's lean, 0.60/0.59, decisions 736-737), since the
        // renderer draws onto a target with nothing under it and the glass
        // is the window server's (`Floor::lift_on_glass`). It covers a page
        // the program set; the default page is left out: its own text is
        // held at the bound by the dims (note AS, section 3). A picture's,
        // an opaque rendition's and a swap's page is solid, and
        // `Resolved::drawn` gives them `fg`.
        let glass = if page_word.is_default() || opaque_bg || self.theme.bg_alpha.get() == 0xFF {
            fg
        } else {
            self.theme.floor.lift_on_glass(fg, bg, self.theme.bg_alpha)
        };
        // **Every background is translucent, not only the default one**:
        // the owner's call of 2026-09-20. A picture's and an opaque
        // rendition's are made solid by `Resolved::drawn`.
        let bg = bg.with_alpha(self.theme.bg_alpha);
        let attrs = style.attrs();
        let mut shapes = Shapes::default().with_face(self.stack.effective(FaceStyle::of(attrs)));
        if opaque_bg {
            shapes = shapes.with(Shapes::OPAQUE_BG);
        }
        if Theme::default_bg(bg, self.page_default) {
            shapes = shapes.with(Shapes::DEFAULT_BG);
        }
        // The contrast curve a glyph takes on either side of a swap
        // (`baud_domain::contrast`), chosen here once per rendition so a
        // pixel only looks its row up: from the opaque colours, the ones a
        // glyph is painted in; alpha never enters the curve.
        let curves = Curves::of(curve_row(fg), curve_row(bg));
        let glass_curves = Curves::of(curve_row(glass), curve_row(bg));
        let line_curves = Curves::of(curve_row(line), curve_row(bg));
        // Which underline is drawn is the style's own bit. The three colon
        // styles *replace* the straight line rather than adding to it, so the
        // straight-line bit is set only for the two that draw one.
        //
        // `SGR 60` and `61` are the "ideogram underline" and "ideogram
        // double underline" of `[[source: ecma48]]` § 8.3.117, a line "on
        // the right side" in vertical text and under it in horizontal
        // text, which is all baud writes; so they draw the straight and the
        // double line, beside whatever `SGR 4` style is in force. `62` and
        // `63` are the overline and the double overline the same way.
        if attrs.has(Attrs::UNDERLINE)
            || attrs.has(Attrs::DOUBLE_UNDERLINE)
            || attrs.has(Attrs::IDEOGRAM_UNDERLINE)
            || attrs.has(Attrs::IDEOGRAM_DOUBLE_UNDERLINE)
        {
            shapes = shapes.with(Shapes::UNDERLINE);
        }
        if attrs.has(Attrs::OVERLINED)
            || attrs.has(Attrs::IDEOGRAM_OVERLINE)
            || attrs.has(Attrs::IDEOGRAM_DOUBLE_OVERLINE)
        {
            shapes = shapes.with(Shapes::OVERLINE);
        }
        for (attr, shape) in [
            (Attrs::IDEOGRAM_DOUBLE_UNDERLINE, Shapes::DOUBLE_UNDERLINE),
            (Attrs::IDEOGRAM_DOUBLE_OVERLINE, Shapes::DOUBLE_OVERLINE),
            (Attrs::FRAMED, Shapes::FRAMED),
            (Attrs::IDEOGRAM_STRESS, Shapes::STRESS),
            (Attrs::DOUBLE_UNDERLINE, Shapes::DOUBLE_UNDERLINE),
            (Attrs::CURLY_UNDERLINE, Shapes::CURLY_UNDERLINE),
            (Attrs::DOTTED_UNDERLINE, Shapes::DOTTED_UNDERLINE),
            (Attrs::DASHED_UNDERLINE, Shapes::DASHED_UNDERLINE),
            (Attrs::STRIKE, Shapes::STRIKE),
            (Attrs::BLINK, Shapes::BLINK),
            (Attrs::RAPID_BLINK, Shapes::RAPID_BLINK),
            (Attrs::HIDDEN, Shapes::CONCEAL),
        ] {
            if attrs.has(attr) {
                shapes = shapes.with(shape);
            }
        }
        let underline = self.theme.underline(style, fg, |ch| term.palette(ch));
        // An explicit `SGR 58` colour gets the same rule against the page
        // (Jev, logged: 0.98); a following underline is the floored text.
        let underline = if style.underline().is_default() {
            underline
        } else {
            self.theme
                .floor
                .lift(underline, bg, (style.underline(), page_word))
        };
        if underline == fg && style.underline().is_default() {
            shapes = shapes.with(Shapes::UNDERLINE_FOLLOWS_FG);
        }
        Resolved {
            fg,
            bg,
            underline,
            shapes,
            curves,
            glass,
            glass_curves,
            line,
            line_curves,
        }
    }

    /// The face style a cell's text is drawn from, from its resolved slot.
    fn face_style(&self, cell: PackedCell) -> FaceStyle {
        crate::cellmap::face_of(&self.styles, cell)
    }

    /// Whether `face`'s scripts select different lookups.
    fn varies(&self, face: FaceIx) -> Varies {
        self.selections
            .get(face.index())
            .map_or(Varies::No, |s| s.varies)
    }

    /// Settles the script of every cell of `row` in context into
    /// [`Renderer::row_scripts`].
    fn settle_row(&mut self, term: &Term<'_>, row: &[PackedCell]) {
        self.row_unresolved.clear();
        for cell in row {
            self.row_unresolved.push(unresolved(term, *cell));
        }
        self.row_scripts.clear();
        self.row_scripts.resize(row.len(), Script::COMMON);
        baud_font::resolve(&self.row_unresolved, &mut self.row_scripts);
    }

    /// The script a run on `face` from column `c` of the row being
    /// converted is shaped in, and the lookups that selects there. A face
    /// whose scripts all select alike is shaped as Common, which is the
    /// same shaping.
    fn script_at(&self, face: FaceIx, col: Col) -> (Script, Selection) {
        match self.selections.get(face.index()) {
            Some(sel) if sel.varies == Varies::Yes => {
                let s = self
                    .row_scripts
                    .get(col.index())
                    .copied()
                    .unwrap_or(Script::COMMON);
                (s, sel.by_script[s])
            }
            _ => (Script::COMMON, Selection::default()),
        }
    }

    /// Resolves one row's colours and glyphs.
    fn convert_row(&mut self, term: &Term<'_>, r: Row, cursor: Cursor, exc: &mut Exceptions) {
        let range = self.row_cells(r);
        // A copy of the row, so shaping may borrow the renderer mutably.
        let row: Vec<PackedCell> = self.cur[range.clone()].to_vec();
        let mut instances = core::mem::take(&mut self.per_row[r.index()]);
        instances.clear();
        // Colours first, cell by cell, with the shapes a rendition asks
        // for: an underline, a strike, an underline or bar cursor. A block
        // cursor is the cell's colours swapped. Shapes go on after the
        // glyphs, so a strike crosses its letter rather than hiding under
        // an opaque colour glyph.
        let mut shapes: Vec<Instance> = Vec::new();
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0);
        let top = i32::try_from(self.band(r).top.value()).unwrap_or(0);
        // `OSC 12` cursor colour, if the application set one. Read once for the
        // row -- it is terminal-global -- and used where the cursor draws: the
        // block cursor fills with it (rather than the reverse-video swap) and
        // the bar or underline cursor's shape takes it. As this frame's video
        // shows it, which under a flash is the colour a flip would leave.
        let cursor_color = term
            .cursor_color_in(self.video)
            .map(|(r, g, b)| Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)));
        let mut blinks = Blinking::UNSEEN;
        for (c, cell) in row.iter().enumerate() {
            let col = Col::at_index(c);
            // One load from the resolved table, which `sync_styles` keeps.
            let res = self
                .styles
                .get(cell.style.index())
                .copied()
                .unwrap_or_default();
            blinks = blinks.with(Blinking::of(res.shapes));
            // A wide tail follows its lead, so a block's edge never cuts a
            // character in two: `Highlight::selects`.
            let selected = if self
                .highlight
                .is_some_and(|h| h.selects(r, col, cell.content))
            {
                Selected::Yes
            } else {
                Selected::No
            };
            let here = cursor.filter(|&(cr, cc, _)| cr == r && cc == col);
            let block = match here {
                Some((_, _, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock)) => {
                    BlockCursor::Yes(cursor_color)
                }
                _ => BlockCursor::No,
            };
            let d = res.drawn(Swaps {
                pictured: self.pictures.at(r, col),
                line: Line::of(cell.content),
                selected,
                block,
                blink: self.blink,
                cursor: here.map_or(Chan::from_u8(0xFF), |(_, _, s)| self.cursor_opacity(s)),
            });
            let (fg, hidden) = (d.fg, d.text == Text::Hidden);
            self.fg[range.start + c] = fg;
            self.curve[range.start + c] = d.curve;
            self.bg[range.start + c] = d.bg;
            self.text[range.start + c] = d.text;
            let x = i32::try_from(c).unwrap_or(0) * cell_w;
            // The underline's own colour, or the foreground after the swaps.
            let ul_color = if res.shapes.has(Shapes::UNDERLINE_FOLLOWS_FG) {
                fg
            } else {
                res.underline
            };
            // The three colon styles, `[[source: kitty-underlines]]`, drawn as
            // runs of solid quads so they need no texel and composite exactly
            // as the straight line does. Amplitude and period are the line's
            // thickness, which is what keeps them legible at every size.
            let curly = res.shapes.has(Shapes::CURLY_UNDERLINE);
            let dotted = res.shapes.has(Shapes::DOTTED_UNDERLINE);
            if !hidden && (curly || dotted || res.shapes.has(Shapes::DASHED_UNDERLINE)) {
                let ul = px_i32(self.metrics.underline);
                let th = px_i32(self.metrics.thickness).max(1);
                let w = px_i32(self.metrics.cell_w);
                let low = px_i32(self.metrics.cell_h) - th;
                let seg = Px::of(u32::try_from(th).unwrap_or(1));
                let mut dx = 0;
                let mut k = 0;
                while dx < w {
                    let (on, dy) = if curly {
                        // A zigzag: up for a thickness, down for a thickness.
                        (true, if k % 2 == 0 { 0 } else { th })
                    } else if dotted {
                        (k % 2 == 0, 0)
                    } else {
                        (k % 3 != 2, 0)
                    };
                    if on {
                        let y = (ul + dy).min(low.max(ul));
                        shapes.push(solid(
                            screen(x + dx),
                            screen(top + y),
                            seg,
                            self.metrics.thickness,
                            ul_color,
                        ));
                    }
                    dx += th;
                    k += 1;
                }
            }
            if !hidden && res.shapes.has(Shapes::UNDERLINE) {
                let ul = px_i32(self.metrics.underline);
                shapes.push(solid(
                    screen(x),
                    screen(top + ul),
                    self.metrics.cell_w,
                    self.metrics.thickness,
                    ul_color,
                ));
                if res.shapes.has(Shapes::DOUBLE_UNDERLINE) {
                    // A second line below, one thickness of gap between them,
                    // clamped so it stays inside the cell.
                    let gap = i32::try_from(self.metrics.thickness.value().max(1)).unwrap_or(1) + 1;
                    let low = px_i32(self.metrics.cell_h) - px_i32(self.metrics.thickness);
                    let y = (ul + gap).min(low.max(ul));
                    shapes.push(solid(
                        screen(x),
                        screen(top + y),
                        self.metrics.cell_w,
                        self.metrics.thickness,
                        ul_color,
                    ));
                }
            }
            if !hidden && res.shapes.has(Shapes::STRIKE) {
                shapes.push(solid(
                    screen(x),
                    screen(top + px_i32(self.metrics.strike)),
                    self.metrics.cell_w,
                    self.metrics.thickness,
                    fg,
                ));
            }
            // The overline and the frame, in the text's colour as the strike
            // is: `SGR 58` is "the underline color" (`[[source:
            // kitty-underlines]]`) and neither is an underline. The frame's
            // top is the overline's line, drawn once when both are set.
            let framed = res.shapes.has(Shapes::FRAMED);
            if !hidden && (framed || res.shapes.has(Shapes::OVERLINE)) {
                shapes.push(solid(
                    screen(x),
                    screen(top),
                    self.metrics.cell_w,
                    self.metrics.thickness,
                    fg,
                ));
            }
            if !hidden && res.shapes.has(Shapes::DOUBLE_OVERLINE) {
                // A second line starting one thickness and a pixel below the
                // first, as the double underline's does, clamped inside the
                // cell.
                let th = px_i32(self.metrics.thickness);
                let low = px_i32(self.metrics.cell_h) - th;
                let y = (th.max(1) + 1).min(low.max(0));
                shapes.push(solid(
                    screen(x),
                    screen(top + y),
                    self.metrics.cell_w,
                    self.metrics.thickness,
                    fg,
                ));
            }
            if !hidden && framed {
                let th = px_i32(self.metrics.thickness).max(1);
                let h = px_i32(self.metrics.cell_h);
                shapes.push(solid(
                    screen(x),
                    screen(top + h - th),
                    self.metrics.cell_w,
                    self.metrics.thickness,
                    fg,
                ));
                // A side only where the run of framed cells starts or ends:
                // the row's edge, or a neighbour that is not framed.
                let framed_at = |i: Option<usize>| {
                    i.and_then(|i| row.get(i))
                        .and_then(|n| self.styles.get(n.style.index()))
                        .is_some_and(|n| n.shapes.has(Shapes::FRAMED))
                };
                let side = Px::of(u32::try_from(th).unwrap_or(1));
                if !framed_at(c.checked_sub(1)) {
                    shapes.push(solid(screen(x), screen(top), side, self.metrics.cell_h, fg));
                }
                if !framed_at(c.checked_add(1)) {
                    shapes.push(solid(
                        screen(x + cell_w - th),
                        screen(top),
                        side,
                        self.metrics.cell_h,
                        fg,
                    ));
                }
            }
            // `SGR 64`'s stress mark, as the web draws an emphasis mark for
            // text with no language (`[[source: css-text-decor-3]]`): over
            // the character, a filled dot, in the text's colour, once a
            // character, so across a wide character's two cells. Inside
            // the cell, a square two thicknesses on a side, as all three
            // paths draw a decoration.
            if !hidden && res.shapes.has(Shapes::STRESS) && takes_stress_mark(cell.content) {
                let wide = row.get(c + 1).is_some_and(|n| n.content.is_wide_tail());
                let d = 2 * px_i32(self.metrics.thickness).max(1);
                let h = px_i32(self.metrics.cell_h);
                let dx = ((if wide { 2 } else { 1 }) * cell_w - d).max(0) / 2;
                let side = Px::of(u32::try_from(d).unwrap_or(2));
                shapes.push(solid(
                    screen(x + dx),
                    screen(top + d.min((h - d).max(0))),
                    side,
                    side,
                    fg,
                ));
            }
            // The bar or underline cursor draws in the application's cursor
            // colour (`OSC 12`) if it set one, else the cell's foreground.
            let cur = cursor_color.unwrap_or(d.cursor_fg);
            let cur = here.map_or(cur, |(_, _, s)| cur.scaled(self.cursor_opacity(s)));
            if let Some((
                _,
                _,
                style @ (CursorStyle::BlinkingUnderline
                | CursorStyle::SteadyUnderline
                | CursorStyle::BlinkingBar
                | CursorStyle::SteadyBar),
            )) = here
            {
                let (left, y, w, h) = self.cursor_px(r, col, style);
                shapes.push(solid(screen(px_i32(left)), screen(px_i32(y)), w, h, cur));
            }
        }
        self.row_blinks[r.index()] = blinks;
        // Then runs of cells shaped by one face, in one selection of its
        // lookups. The row's scripts are settled the first time a run is on
        // a face whose scripts select different lookups, and not at all on a
        // row drawn only by faces whose scripts all select alike -- which is
        // every row the embedded faces draw alone.
        let mut settled = false;
        let mut c = 0usize;
        while c < row.len() {
            let Some(base) = drawn_base(term, row[c]) else {
                c += 1;
                continue;
            };
            if crate::boxes::covers(base) {
                // Box drawing, a block element or braille: baud's own mask
                // at the cell origin, never shaped, never in a run.
                if let Some(slot) = self.procedural(base) {
                    self.emit_at(r, Col::at_index(c), slot, &mut instances);
                }
                c = CellSpan::at(&row, Col::at_index(c))
                    .after(Col::at_index(c))
                    .index();
                continue;
            }
            let face = self.face_of_cell(term, row[c], base);
            let varies = self.varies(face);
            if !settled && varies == Varies::Yes {
                self.settle_row(term, &row);
                settled = true;
            }
            let (script, selection) = self.script_at(face, Col::at_index(c));
            // Gather the run: consecutive drawn cells on this face. A bold
            // cell and a regular one are on two faces when a bold face
            // exists, so the style splits the run through the face, and a
            // cell whose marks move it to another face splits it too. So
            // does a cell whose script selects other lookups on this face:
            // a Hebrew word in a face whose `hebr` table differs from its
            // `latn` one is its own run.
            let start = c;
            let mut clusters: Vec<ClusterIn> = Vec::new();
            // The key folds the face and then every codepoint of every cell.
            // That is all a run's shaping depends on: a cluster moved to
            // another face is on that face's key, and the marks a face that
            // keeps a cluster cannot draw -- left out before shaping, in
            // `shape` -- are decided by that face and those codepoints alone.
            //
            // And the selection: a run of Common cells takes its script from
            // the cells around it, which the key does not fold, so the
            // lookups that script selects are folded instead -- all but the
            // first, so a face whose scripts all select alike keys its runs
            // exactly as it did before scripts were read.
            let mut key = Key::start().fold(u32::try_from(face.index()).unwrap_or(0));
            if selection != Selection::default() {
                key = key.fold(u32::from(selection.0));
            }
            // The counts behind the `exceptions` feature: a cmap lookup, a
            // digest test and a pair test per cell, which the bench reads
            // and the app must not pay for.
            #[cfg(feature = "exceptions")]
            let prefilter = self.starters[face.index()];
            #[cfg(feature = "exceptions")]
            let mut starters_here = Tally::default();
            #[cfg(feature = "exceptions")]
            let mut prev_glyph: Option<GlyphId> = None;
            #[cfg(feature = "exceptions")]
            let mut pair_here = Tally::default();
            while c < row.len() && c - start < RUN_CELLS {
                let cell = row[c];
                let Some(cp) = drawn_base(term, cell) else {
                    break;
                };
                // The run's first cell is taken whatever the test says: the
                // face above was chosen for it, and a run that could stop
                // before its first cell would never advance -- a hang, not
                // a wrong pixel, if the two ever disagreed. Both ask
                // `face_of_cell`, so a marked cell is asked with its whole
                // cluster both times.
                if c > start
                    && (crate::boxes::covers(cp)
                        || self.face_of_cell(term, cell, cp) != face
                        || (varies == Varies::Yes
                            && self.script_at(face, Col::at_index(c)).1 != selection))
                {
                    break;
                }
                let next = CellSpan::at(&row, Col::at_index(c))
                    .after(Col::at_index(c))
                    .index();
                #[cfg(feature = "exceptions")]
                {
                    let glyph = self.stack.face(face).glyph(cp);
                    if let (Some(p), Some(g)) = (prev_glyph, glyph) {
                        if self.pairs[face.index()].admits(p, g) {
                            pair_here.bump();
                        }
                    }
                    prev_glyph = glyph;
                    match prefilter {
                        Starter::Gsub(d) => {
                            if let Some(g) = glyph {
                                let mut one = LookupDigest::EMPTY;
                                one.add(g);
                                if d.may_have(one) {
                                    starters_here.bump();
                                }
                            }
                        }
                        Starter::Morx => exc.morx.bump(),
                        Starter::Plain => {}
                    }
                }
                let mut cps = vec![cp];
                key = key.fold(cp.value());
                if cell.flags.is_marked() {
                    exc.marked.bump();
                    for m in term.cluster_marks(cell) {
                        if let Some(mc) = Codepoint::new(m) {
                            cps.push(mc);
                            key = key.fold(m);
                        }
                    }
                }
                key = key.fold(u32::try_from(next - c).unwrap_or(0) << 24);
                clusters.push(ClusterIn {
                    cps,
                    at: CellIx(u8::try_from(c - start).unwrap_or(u8::MAX)),
                });
                c = next;
            }
            let shaped = match self.cache.get(key) {
                Some(s) => s,
                None => {
                    let s = self.shape(face, script, &clusters, ..);
                    self.cache.put(key, s);
                    s
                }
            };
            #[cfg(feature = "exceptions")]
            {
                if starters_here.value() > 0 {
                    exc.starters.add(starters_here);
                    exc.shaped.add(Tally::of(c - start));
                }
                if pair_here.value() > 0 {
                    exc.bigram.add(Tally::of(c - start));
                }
            }
            for p in shaped.glyphs() {
                if p.last != p.cell {
                    exc.spanning
                        .add(Tally::of(usize::from(p.last.0) - usize::from(p.cell.0) + 1));
                }
            }
            #[cfg(feature = "exceptions")]
            self.count_differs(term, r, Col::at_index(start), &clusters, &shaped, exc);
            self.emit(term, face, r, Col::at_index(start), &shaped, &mut instances);
        }
        instances.extend(shapes);
        self.per_row[r.index()] = instances;
    }

    /// `clusters` shaped as one run on `face`; what a face that keeps a
    /// cluster cannot draw is left out and counted for the clusters `own`
    /// holds -- all of them, except in a ligature window's chunk, whose
    /// context cells are another chunk's to count.
    fn shape(
        &mut self,
        face: FaceIx,
        script: Script,
        clusters: &[ClusterIn],
        own: impl core::ops::RangeBounds<CellIx>,
    ) -> Shaped {
        // **A refused glyph box is shaped again with the scratch it named,
        // once.** The shaper reports the largest need of the run, so one
        // pass at that size draws every box it refused; a box past the
        // budget is refused again and counted. Counts are taken from the
        // pass kept.
        let mut grown = false;
        loop {
            let have = self.ink_scratch.len();
            let f = self.stack.face(face);
            let mut shaper = Shaper::new(f, &mut self.storage);
            shaper.accelerate(&self.digests[face.index()]);
            shaper.scratch(&mut self.ink_scratch);
            let mut unmapped = 0usize;
            for cl in clusters {
                let at = Cluster(u16::from(cl.at.0));
                // **A mark the face cannot draw is left out, and counted.** A
                // cluster reaches a face that cannot draw it whole only when no
                // face can -- `face_for` would have chosen that one -- and the
                // `.notdef` it would map such a mark to is a base with a full
                // advance in JetBrains Mono: the box drew a cell to the right
                // and every mark after it over the box (Jev A_DROP, 0.99). What
                // is left is the letter and the marks the face has, which the
                // shaper takes as it takes any cluster; a face that draws the
                // cluster whole is handed it untouched, and which of the two a
                // cluster is on a face is remembered, as its face is, so a run
                // the cache misses pays a probe per marked cell rather than a
                // normalisation. The dropped set follows from the face and the
                // codepoints alone, which the run cache's key folds, so a cached
                // run never needs it recomputed.
                let pushed = match cl.cps.as_slice() {
                    [_, _, ..] => {
                        let cover = self.cluster_covers.answer(face, &cl.cps, || {
                            match Normalised::of(f, &cl.cps).covered() {
                                true => Cover::Whole,
                                false => Cover::Partial,
                            }
                        });
                        if cover == Cover::Whole {
                            shaper.push_cluster(&cl.cps, at)
                        } else {
                            let kept = Normalised::without_unmapped(f, &cl.cps);
                            let pushed = shaper.push_cluster(kept.as_slice(), at);
                            if pushed.is_some() && own.contains(&cl.at) {
                                unmapped = unmapped.saturating_add(
                                    usize::try_from(kept.dropped().value()).unwrap_or(0),
                                );
                            }
                            pushed
                        }
                    }
                    cps => shaper.push_cluster(cps, at),
                };
                if pushed.is_none() {
                    break;
                }
            }
            let (buf, shaped) = shaper.finish(Features::GRID, script);
            // A need past the budget grows the scratch to the budget, so the
            // boxes within it still draw and only the one past it is refused.
            let want = shaped.ink_need.value().min(crate::atlas::VAR_POINTS_MAX);
            let want = usize::try_from(want).unwrap_or(usize::MAX);
            if !shaped.ink_refused.is_zero() && !grown && have < want {
                grown = true;
                self.ink_scratch.resize(want, VarPoint::EMPTY);
                continue;
            }
            self.marks_unmapped.add(Tally::of(unmapped));
            self.ink_refused.add(Tally::of(
                usize::try_from(shaped.ink_refused.value()).unwrap_or(usize::MAX),
            ));
            let mut out = Shaped::EMPTY;
            for g in buf.as_slice() {
                if g.ignorable == Ignorable::Yes {
                    continue;
                }
                let p = Placed {
                    id: g.id,
                    cell: CellIx(u8::try_from(g.cluster.0).unwrap_or(u8::MAX)),
                    last: CellIx(u8::try_from(g.last.0).unwrap_or(u8::MAX)),
                    dx: g.dx,
                    dy: g.dy,
                };
                if out.push(p).is_none() {
                    break;
                }
            }
            return out;
        }
    }

    /// The `differs` count: for each cluster of the run, whether the
    /// shaped glyphs are what the cell shader's map would draw for its
    /// base codepoint -- one glyph, the same atlas slot, the same offset.
    #[cfg(feature = "exceptions")]
    fn count_differs(
        &mut self,
        term: &Term<'_>,
        r: Row,
        start: Col,
        clusters: &[ClusterIn],
        shaped: &Shaped,
        exc: &mut Exceptions,
    ) {
        let range = self.row_cells(r);
        for cl in clusters {
            let col = start.index() + usize::from(cl.at.0);
            let Some(cell) = self.cur.get(range.start + col).copied() else {
                continue;
            };
            let Some(cp) = drawn_base(term, cell) else {
                continue;
            };
            let style = self.face_style(cell);
            // The single-codepoint answer depends on the style, so the memo
            // keys on both; a codepoint is 21 bits and the style sits above.
            let memo = crate::cellmap::CellKey::of(cp, style, CellSpan::One).0;
            let alone = match self.singles.get(&memo) {
                Some(s) => *s,
                None => {
                    let face = self.face_for(core::slice::from_ref(&cp), style);
                    let one = [ClusterIn {
                        cps: vec![cp],
                        at: CellIx(0),
                    }];
                    let s = self
                        .shape(face, Script::of(cp), &one, ..)
                        .glyphs()
                        .first()
                        .map(|p| (face, *p));
                    self.singles.insert(memo, s);
                    s
                }
            };
            let face = self.face_for(core::slice::from_ref(&cp), style);
            let mine: Vec<Placed> = shaped
                .glyphs()
                .iter()
                .copied()
                .filter(|p| p.cell == cl.at)
                .collect();
            let same = match (alone, mine.as_slice()) {
                (None, []) => true,
                (Some((af, a)), [p]) => af == face && a.id == p.id && a.dx == p.dx && a.dy == p.dy,
                _ => false,
            };
            if !same {
                exc.differs.bump();
            }
        }
    }

    /// The atlas mask for a procedural codepoint at this cell size.
    fn procedural(&mut self, cp: Codepoint) -> Option<crate::atlas::Slot> {
        let cell = crate::boxes::Cell {
            w: self.metrics.cell_w,
            h: self.metrics.cell_h,
        };
        self.atlas.synthetic(cp, cell, self.metrics.baseline)
    }

    /// One instance for a slot placed at the origin of cell `col` of row
    /// `r`, in the cell's foreground: the procedural glyphs' emitter.
    fn emit_at(&mut self, r: Row, col: Col, slot: crate::atlas::Slot, out: &mut Vec<Instance>) {
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0);
        let baseline = i32::try_from(self.metrics.baseline.value()).unwrap_or(0);
        let top = i32::try_from(self.band(r).top.value()).unwrap_or(0);
        let range = self.row_cells(r);
        // A hidden cell draws nothing of its own (`Resolved::text`).
        if self.text.get(range.start + col.index()) == Some(&Text::Hidden) {
            return;
        }
        let color = self
            .fg
            .get(range.start + col.index())
            .copied()
            .unwrap_or(self.defaults.0);
        let curve = self
            .curve
            .get(range.start + col.index())
            .copied()
            .unwrap_or_default();
        let x = i32::try_from(col.index()).unwrap_or(0) * cell_w + slot.left;
        let y = top + baseline + slot.top;
        out.push(Instance {
            x: Screen(i16::try_from(x).unwrap_or(i16::MAX)),
            y: Screen(i16::try_from(y).unwrap_or(i16::MAX)),
            w: slot.w,
            h: slot.h,
            u: slot.x,
            v: slot.y,
            color,
            kind: slot.kind,
            curve,
        });
    }

    /// Instances for a shaped run starting at column `start` of row `r`.
    fn emit(
        &mut self,
        term: &Term<'_>,
        face: FaceIx,
        r: Row,
        start: Col,
        shaped: &Shaped,
        out: &mut Vec<Instance>,
    ) {
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0);
        let baseline = i32::try_from(self.metrics.baseline.value()).unwrap_or(0);
        let top = i32::try_from(self.band(r).top.value()).unwrap_or(0);
        let range = self.row_cells(r);
        for p in shaped.glyphs() {
            let col = start.index() + usize::from(p.cell.0);
            // A glyph belongs to the cell it is placed at; a hidden cell's
            // are dropped here rather than kept out of the run, so a run
            // shapes the same in both blink phases.
            if self.text.get(range.start + col) == Some(&Text::Hidden) {
                continue;
            }
            let color = self
                .fg
                .get(range.start + col)
                .copied()
                .unwrap_or(self.defaults.0);
            let curve = self
                .curve
                .get(range.start + col)
                .copied()
                .unwrap_or_default();
            // The cluster's place in its span, from the cell its base
            // writes: a mark at that cell moves with it.
            let span = CellSpan::at(&self.cur[range.clone()], Col::at_index(col));
            let phase = match self.cur.get(range.start + col).copied() {
                Some(cell) if span == CellSpan::Two => term
                    .base_of(cell)
                    .as_codepoint()
                    .map_or(self.metrics.phase, |b| {
                        self.phase_in(face, b, CellSpan::Two)
                    }),
                _ => self.metrics.phase,
            };
            let Some(slot) = self.atlas.slot(f, face, p.id, scale, phase) else {
                continue;
            };
            let dx = scale.px(f32::from(p.dx)).round() as i32;
            let dy = scale.px(f32::from(p.dy)).round() as i32;
            let x = i32::try_from(col).unwrap_or(0) * cell_w + dx + slot.left;
            let y = top + baseline - dy + slot.top;
            out.push(Instance {
                x: Screen(i16::try_from(x).unwrap_or(i16::MAX)),
                y: Screen(i16::try_from(y).unwrap_or(i16::MAX)),
                w: slot.w,
                h: slot.h,
                u: slot.x,
                v: slot.y,
                color,
                kind: slot.kind,
                curve,
            });
        }
    }
}
