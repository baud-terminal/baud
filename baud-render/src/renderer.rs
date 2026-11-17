//! The frame: rows that changed, converted and painted.

use crate::atlas::{Atlas, AtlasStats, Code, Flip, Kind, Rescaled, SizeKey, Stretch, Tex};
use crate::badge::{self, BadgeLook};
use crate::cache::{
    CacheStats, CellIx, ClusterMemo, Cover, Key, PLACED_MAX, Placed, Question, RUN_CELLS, RunCache,
    Shaped,
};
use crate::caret::{self, CursorPhase, Play};
use crate::cellmap::{
    BlockCursor, BlockPaint, CellSpan, CursorLook, Curves, Ground, Hilite, Line, Resolved,
    Selected, Shapes, Swaps, Text, takes_stress_mark,
};
use crate::color::{Argb, BaudCursor, CursorMade, Theme, Video};
use crate::composite::{Blend, Blit, Mix, Page, Target, Texels};
use crate::counters::{Counter, Site};
use crate::effect::Vfx;
use crate::find::{Current, FindLook};
use crate::flash::{self, Strength};
use crate::frozen::Words;
use crate::guide::{self, GuideLook};
use crate::instance::{Instance, Screen};
use crate::linked::{
    HoverPaint, LineColour, LineStyle, LinkHover, LinkLine, LinkLook, MissingPaint,
};
use crate::metrics::{Metrics, PxSize};
use crate::overlay::Composition;
use crate::picture::Pictures;
use crate::scrollbar::{self, Bar, Scroll};
use crate::select::Highlight;
use crate::splash::{self, SplashLook};
use crate::stack::{EmojiDefault, FaceIx, FaceTable, Stack};
use crate::stack::{FaceStyle, FontSlot, Presentation, TextFace};
use crate::trail::{self, Drawn, Rect, Smear, TrailLook};
use crate::transition::{self, Transition, Veil};
use crate::units::{Band, Tally, slots};
use baud_core::{BidiSupport, PaintRole, ProgramPaint, Term};
use baud_domain::contrast::CurveRow;
use baud_domain::themefile::CellPaint;
use baud_domain::{
    Attrs, BidiDirection, Chan, ClusterEpoch, ClusterSlot, Codepoint, Col, Cols, CursorStyle,
    FrameNo, GlyphId, ImageAge, ImageAlpha, ImageDraw, ImageGen, ImageGround, ImageId, ImagePx,
    ImageScreen, ImageSince, ImageStore, KittyId, KittyRoot, LineRendition, PackedCell, Pixel,
    PlaceSerial, Row, RowAt, RowGens, Rows, Seal, Viewport, ZIndex,
};
use baud_font::bytes::Count;
use baud_font::{
    Cluster, Digest as LookupDigest, Direction, Engine, Face, Features, Glyph, Ignorable,
    LookupSet, MAX_LOOKUPS, MarkPlacement, Normalised, PerScript, Phase, Px, Scale, Script,
    ScriptTag, Shaper, Substitution, Tag, Unresolved, VarPoint, Walked,
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
    /// The off phase drawn faint rather than hidden: the text half way to
    /// the page it is drawn on, as `SGR 2` mixes it (`Resolved::drawn`).
    /// Blinking text's off phase while the system asks for dimmed flashing
    /// lights; the rates are unchanged.
    Faint,
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
        self.in_phase(Blink::Hidden)
    }

    /// The blink shapes this frame draws faint, [`Blink::Faint`]'s rates:
    /// the cell shader's `blink_faint`.
    pub fn faint(self) -> Shapes {
        self.in_phase(Blink::Faint)
    }

    /// These phases with each hidden off phase drawn faint instead: what a
    /// driver draws while the system asks for dimmed flashing lights. A
    /// shown phase stays shown, so phases held shown stay steady.
    pub fn dimmed(self) -> BlinkPhases {
        let dim = |b: Blink| match b {
            Blink::Hidden => Blink::Faint,
            Blink::Shown | Blink::Faint => b,
        };
        BlinkPhases {
            slow: dim(self.slow),
            rapid: dim(self.rapid),
        }
    }

    /// The blink shapes of the rates whose phase is `phase`.
    fn in_phase(self, phase: Blink) -> Shapes {
        let slow = match self.slow == phase {
            true => Shapes::BLINK,
            false => Shapes::default(),
        };
        match self.rapid == phase {
            true => slow.with(Shapes::RAPID_BLINK),
            false => slow,
        }
    }
}

/// Whether this frame shows the video `?5` does not say: a visual bell whose
/// two halves no frame drew separately, which a driver sees through
/// `Term::reverse_flips` and holds for as long as the entry it advertised
/// asks, or the video a change of `?5` past WCAG 2.3.1's budget leaves on
/// screen until the budget allows it (`bell::ScreenFlash`). **It is the
/// opposite of the mode, not reverse video**, so a flash
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
pub(crate) struct ThemeGen(u32);

/// What a resolved slot depends on besides the slot: the theme, the video --
/// `?5` or a flash's -- and the colours the core holds, as an epoch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct StyleInputs {
    theme: ThemeGen,
    video: Video,
    colors: baud_domain::ColorEpoch,
}

/// **Where one resolved copy of the rendition table stands**: which table it
/// resolved, how many of the table's writes, and under which inputs, and how
/// many of its slots blink or carry a decoration. One a copy -- the CPU
/// path's own, and one for each of the cell path's buffer sets, since a set
/// the GPU is reading cannot be written and so falls behind the others by a
/// frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StyleStamp {
    table: Option<baud_domain::TableSerial>,
    at: baud_domain::StyleWrites,
    inputs: Option<StyleInputs>,
    blinking: Tally,
    /// The slots of the copy, up to the table's length, that carry a shape
    /// of [`Shapes::DECORATIONS`]: every one of them, a slot no cell names
    /// included, so the count is never below the slots a cell can name.
    decorated: Tally,
    /// What the core had reclaimed when this copy was stamped.
    reclaims: Option<baud_domain::Reclaims>,
    /// **The slots the copy lends a frame's frozen cells** (`crate::frozen`):
    /// a lent id means what it means only under this lending, so two copies
    /// stamped alike resolve every id alike.
    lending: crate::frozen::Lending,
}

impl StyleStamp {
    /// A slot lent to a frozen row, or given back, stopped carrying `s`
    /// (`crate::frozen`): the counts follow what the slots hold.
    pub(crate) fn lent_lose(&mut self, s: Shapes) {
        if s.blinks() {
            self.blinking.lose();
        }
        if s.any(Shapes::DECORATIONS) {
            self.decorated.lose();
        }
    }

    /// A slot lent to a frozen row, or given back, carries `s` now.
    pub(crate) fn lent_gain(&mut self, s: Shapes) {
        if s.blinks() {
            self.blinking.bump();
        }
        if s.any(Shapes::DECORATIONS) {
            self.decorated.bump();
        }
    }

    /// The copy holds `l` now.
    pub(crate) fn set_lending(&mut self, l: crate::frozen::Lending) {
        self.lending = l;
    }

    /// Whether a slot of this copy may draw a decoration: none when the
    /// copy was synced and no slot of it carries one, so a frame can leave
    /// the decorations' pass undrawn. A copy never synced may hold any.
    pub fn decorations(&self) -> Decorations {
        match (self.table, self.decorated.value()) {
            (Some(_), 0) => Decorations::Absent,
            _ => Decorations::Present,
        }
    }
}

/// Whether a resolved copy of the table holds a slot that draws a
/// decoration ([`StyleStamp::decorations`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decorations {
    Absent,
    Present,
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
    /// draws: a rate whose phase changed -- shown, hidden or faint -- and
    /// whose text is on screen.
    pub fn flips(self, from: BlinkPhases, to: BlinkPhases) -> bool {
        let moved = (from.hidden().0 ^ to.hidden().0) | (from.faint().0 ^ to.faint().0);
        moved & self.0.0 != 0
    }
}

/// What [`Renderer::convert`] did, handed to [`Renderer::paint`] or to a
/// GPU path.
#[derive(Clone, Copy, Debug, Default)]
pub struct Converted {
    pub rows_dirty: Tally,
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
#[derive(Clone, Default, PartialEq, Debug)]
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
        // A face that substitutes through `morx` begins no `GSUB` pair.
        let gsub = face
            .gsub()
            .filter(|_| Substitution::of(face) == Substitution::Gsub);
        let walked = gsub.map(|gsub| {
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
    /// The shaper's offsets in font units, which a cell fitted to its one
    /// cell places its glyphs from ([`Renderer::fit_glyphs`]).
    pub(crate) units: Units,
}

/// **A glyph's shaper offsets from its cell's origin, in font units**:
/// what [`Placed`] carries, kept on a [`ClusterGlyph`] beside the pixels
/// they were rounded to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Units {
    dx: i16,
    dy: i16,
}

impl Units {
    /// No offset: a glyph baud draws itself.
    pub(crate) const ZERO: Units = Units { dx: 0, dy: 0 };

    /// `p`'s offsets.
    pub(crate) fn of(p: &Placed) -> Units {
        Units { dx: p.dx, dy: p.dy }
    }
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
/// it, its atlas slot when it has ink, its pair role, and whether that
/// glyph is all of it.
#[derive(Clone, Copy, Debug)]
pub struct Single {
    pub slot: Option<crate::atlas::Slot>,
    pub glyph: GlyphId,
    pub role: PairRole,
    pub face: FaceIx,
    pub glyphs: Glyphs,
    /// Whether the glyph is a letter of a joining script reaching past its
    /// cell, which rule D may fit (`crate::fit`).
    pub(crate) overflow: crate::fit::Overflow,
}

/// How many glyphs a codepoint shapes to alone. One is what a map entry
/// draws. Several is a precomposed letter its face lacks, drawn
/// decomposed -- U+01FA as an `A`, a ring and an acute -- or a face's
/// multiple substitution: the map holds the first, and the cell is drawn
/// by records ([`crate::Overlay`]), as a marked cell is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyphs {
    One,
    Several,
}

/// What one frame cost and did.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub rows_dirty: Tally,
    /// The image placements the repainted rows examined to find those
    /// under their text, twice a row -- the cells they reach, then the
    /// draw: a row's own when the frame indexed them, every one when it
    /// repainted too few rows to ([`INDEX_ROWS`]). The row's pass over the
    /// placements over its text, which blits them after its glyphs, is not
    /// counted here: its cost is the pixels it blits
    /// (`Counter::ImagePixelsBlitted`).
    pub places_examined: Tally,
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

/// **The most faces [`Renderer`] remembers for text in an alternative font
/// or under Fraktur**, after which the memo starts again: its key is a
/// codepoint a program prints, so it is bounded as every table keyed on
/// what a program sends is. 4,096 codepoints a font and style cover a
/// screen of text in every font at once; a miss asks the stack again
/// (`Stack::select_font`), a few `cmap` lookups.
const FONT_FACES_MAX: usize = 4096;

/// **The most grounds a repeat pass's cap is read from**: more under it,
/// and it draws nothing. A cap over a text colour costs every mixture a
/// glyph edge draws, 0.3-1.8 ms in release on this Mac (`flash_screens`'
/// scan line), and a picture holds thousands.
const FLASH_GROUNDS: usize = 64;

/// The caps scanned a frame, at most: two, under 4 ms.
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

/// Whether the cursor guide is drawn over a cell under a repeat pass: its
/// background is then the guide over the page, at each of its alphas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Guided {
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

/// The marks a find overlay draws on `row`.
fn marks_on(f: &Option<FindLook>, row: Row) -> impl Iterator<Item = &crate::find::Mark> {
    f.iter().flat_map(move |f| {
        f.marks
            .iter()
            .filter(move |m| m.line == crate::select::WindowLine::of(row))
    })
}

/// The find field, when it is on `row`.
fn field_on(f: &Option<FindLook>, row: Row) -> Option<&crate::find::Field> {
    f.as_ref().map(|f| &f.field).filter(|f| f.row == row)
}

/// The link preview's bar, when it is on `row`.
fn bar_on(l: &Option<LinkLook>, row: Row) -> Option<&crate::find::Field> {
    l.as_ref().map(|l| &l.bar).filter(|b| b.row == row)
}

/// The embedder's notice, when it is on `row`.
fn field_at(f: &Option<crate::find::Field>, row: Row) -> Option<&crate::find::Field> {
    f.as_ref().filter(|f| f.row == row)
}

/// The hover tip's line on `row`.
fn tip_on(t: &Option<crate::tip::TipLook>, row: Row) -> Option<&crate::find::Field> {
    t.as_ref()
        .and_then(|t| t.lines.iter().find(|f| f.row == row))
}

/// The lines at rest on `row`.
fn lines_on(l: &[LinkLine], row: Row) -> impl Iterator<Item = &LinkLine> {
    l.iter().filter(move |s| s.row == row)
}

/// The runs of the link under the pointer on `row`.
fn hover_on(h: Option<&LinkHover>, row: Row) -> impl Iterator<Item = &crate::linked::LinkSpan> {
    h.into_iter()
        .flat_map(move |h| h.spans.iter().filter(move |s| s.row == row))
}

/// **The visual runs stored columns `from..=to` of window row `r` are
/// drawn in, each handed to `each`**: on a row drawn in its own order with
/// no display held, the one run of its own cells, with no list built, as
/// [`crate::bidi::Orders::stored_runs`] finds it on a window row;
/// elsewhere the runs that finds.
fn each_run(
    orders: &crate::bidi::Orders,
    (r, from, to): (Row, Col, Col),
    mut each: impl FnMut(Row, Col, Cols),
) {
    if orders.display().is_none() && !orders.reordered(r) {
        each(
            r,
            from,
            Cols::of_index(to.index().saturating_sub(from.index()) + 1),
        );
        return;
    }
    for (row, at, n) in orders.stored_runs(r, from, to) {
        each(row, at, n);
    }
}

/// The fields the link under the pointer draws on `row`.
fn bars_on(b: &[HoverBar], row: Row) -> impl Iterator<Item = &HoverBar> {
    b.iter().filter(move |b| b.comp.row == row)
}

/// **A field the link under the pointer is drawn as**, and the cell under
/// it, whose ground `hover = "text"` keeps ([`crate::HoverPaint`]).
#[derive(PartialEq, Eq, Debug)]
pub struct HoverBar {
    pub comp: Composition,
    pub under: PackedCell,
}

impl Clone for HoverBar {
    fn clone(&self) -> HoverBar {
        HoverBar {
            comp: self.comp.clone(),
            under: self.under,
        }
    }

    /// In the buffers it has: a frame copies these over last frame's.
    fn clone_from(&mut self, source: &HoverBar) {
        self.comp.row = source.comp.row;
        self.comp.col = source.comp.col;
        self.comp.text.clone_from(&source.comp.text);
        self.comp.measure = source.comp.measure;
        self.comp.hints.clone_from(&source.comp.hints);
        self.comp.caret = source.comp.caret;
        self.under = source.under;
    }
}

/// The link preview's underlined runs on `row`.
fn spans_on(l: &Option<LinkLook>, row: Row) -> impl Iterator<Item = &crate::linked::LinkSpan> {
    l.iter()
        .flat_map(move |l| l.spans.iter().filter(move |s| s.row == row))
}

/// **Visual run `(row, at, n)` of a hovered link as fields**: its cells'
/// characters, shown where they are drawn, one field for `"box"`, one per
/// rendition for `"text"`, whose ground is its cells' own. Each in a bar of
/// `spare`'s, whose buffers last frame's fields grew.
fn hover_fields(
    (term, words): (&Term<'_>, Words<'_>),
    orders: &crate::bidi::Orders,
    view: &Viewport<'_>,
    (row, at, n): (Row, Col, Cols),
    paint: HoverPaint,
    under: &impl Fn(Row, Col, Col) -> PackedCell,
    (out, spare): (&mut Vec<HoverBar>, &mut Vec<HoverBar>),
) {
    let hint = match paint {
        HoverPaint::Text => crate::overlay::Hints::LINK_TEXT,
        HoverPaint::Box | HoverPaint::Line => crate::overlay::Hints::MESSAGE,
    };
    let mut open: Option<HoverBar> = None;
    for v in at.index()..at.index() + n.index() {
        let v = Col::at_index(v);
        let l = orders.logical_col(row, v);
        let Some(&cell) = view.row(row).get(l.index()) else {
            continue;
        };
        if cell.content.is_wide_tail() {
            continue;
        }
        let ground = under(row, v, l);
        let parted = paint == HoverPaint::Text
            && open.as_ref().is_some_and(|b| b.under.style != ground.style);
        if parted {
            out.extend(open.take());
        }
        let bar = open.get_or_insert_with(|| {
            let under = match paint {
                HoverPaint::Text => ground,
                HoverPaint::Box | HoverPaint::Line => PackedCell::EMPTY,
            };
            let mut comp = spare.pop().map_or_else(
                || Composition {
                    row,
                    col: v,
                    text: Vec::new(),
                    measure: Default::default(),
                    hints: Vec::new(),
                    caret: crate::overlay::Caret::Hidden,
                },
                |b| b.comp,
            );
            comp.row = row;
            comp.col = v;
            comp.text.clear();
            comp.hints.clear();
            HoverBar { comp, under }
        });
        let before = bar.comp.text.len();
        cell_cps_into(term, words, cell, &mut bar.comp.text);
        if bar.comp.text.len() == before {
            bar.comp.text.push(Codepoint::from_char(' '));
        }
        bar.comp.hints.resize(bar.comp.text.len(), hint);
    }
    out.extend(open);
}

/// The palette entry a match is washed in, `SGR 33`'s yellow, which WezTerm
/// gives its current match (`[[source: wezterm-copy-e2a98e1]]`): the theme's
/// own yellow, so a palette a program or a theme sets moves it too.
const MARK_SLOT: Chan = Chan::from_offset(33, 30);
/// How much of it: enough to find on a page, little enough to read the
/// text through.
const MARK_ALPHA: u8 = 0x66;

/// How many sizes of the badge's glyphs the atlas holds before it starts
/// over (`Renderer::set_badge`).
const BADGE_SIZES: usize = 2;

/// Whether a trail the driver asks for is held back: one lit while the
/// splash's plasma or the fireworks held the effect's slot, until it
/// lands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HeldTrail {
    Yes,
    No,
}

/// Whether a CPU frame repainted the exit-status strip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StripFresh {
    Yes,
    No,
}

/// A filled rectangle `w` by `h` at `(x, y)` in `color`.
/// **A link's line over `w` pixels from `left` at `y`, `thick` thick**, in
/// `[link] line`'s style: one solid, dots a thickness wide a thickness
/// apart, or none.
fn line_solids(
    out: &mut Vec<Instance>,
    style: LineStyle,
    (left, y): (i32, Screen),
    (w, thick): (u32, Px),
    colour: Argb,
) {
    match style {
        LineStyle::Solid => out.push(solid(screen(left), y, Px::of(w), thick, colour)),
        LineStyle::Off => {}
        LineStyle::Dotted => {
            let step = thick.value().max(1);
            let mut dx = 0;
            while dx < w {
                out.push(solid(
                    screen(left.saturating_add(i32::try_from(dx).unwrap_or(i32::MAX))),
                    y,
                    Px::of(step),
                    thick,
                    colour,
                ));
                dx = dx.saturating_add(step.saturating_mul(2));
            }
        }
    }
}

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

/// The first word of a word piece's run-cache key (`crate::word`): a
/// word's run is laid end to end, never a grid run's.
const WORD_KEY: u32 = 0x5744_0000;

/// The second word of the key of a word piece shaped a cell a cluster,
/// each glyph keeping its cell ([`baud_font::ClusterLevel::Characters`]).
const WORD_CELLS_KEY: u32 = 0x5743_0000;

/// A word's pieces measured on one instance ([`Renderer::word_measure`]):
/// each shaped, where each starts, the face each glyph is drawn from, and
/// the word's measure.
type Measured = (
    Vec<Shaped>,
    Vec<PieceAt>,
    Vec<Vec<FaceIx>>,
    crate::word::Measure,
);

/// **The space kept between two Devanagari words**, in ems of the word's
/// face: 0.26, Noto Sans Devanagari's own space at its full width, for
/// every count, size and instance (`crate::word`).
const WORD_GAP_EM: f32 = 0.26;

/// **A glyph laid at its natural pen is placed to a quarter of a pixel**:
/// its mask rasterised at one of four phases, so a word's headline joins
/// where the face's does, and a glyph takes at most four atlas slots.
const PEN_STEPS: f32 = 4.0;

/// **`inst`, a glyph of a word in its syllable's `own` colours, cut where
/// the ground under it is another**: at every cell edge and lit
/// rectangle's edge it crosses, each piece whose ground is not its
/// syllable's taken in that ground's text colour, the pieces alike in
/// colour kept whole -- so no ink lies in the colour of the ground under
/// it. A colour glyph is drawn whole: its colours are its own.
fn cut_by_ground(
    inst: Instance,
    own: crate::word::Colours,
    cell: Px,
    lit: &[(i32, i32, crate::word::Colours)],
    ground: &dyn Fn(i32) -> crate::word::Colours,
    out: &mut Vec<Instance>,
) {
    let x0 = i32::from(inst.x.0);
    let x1 = x0 + i32::from(inst.w.0);
    let cell = px_i32(cell).max(1);
    if inst.kind != Kind::Mask || x1 <= x0 {
        out.push(inst);
        return;
    }
    let mut cuts: Vec<i32> = vec![x0, x1];
    let mut e = (x0.div_euclid(cell) + 1) * cell;
    while e < x1 {
        cuts.push(e);
        e += cell;
    }
    for &(l, r, _) in lit {
        for v in [l, r] {
            if v > x0 && v < x1 {
                cuts.push(v);
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    let colour = |a: i32| {
        let g = ground(a);
        match g.bg == own.bg {
            true => own,
            false => g,
        }
    };
    let mut a = x0;
    let mut now = colour(x0);
    for &b in cuts.iter().skip(1) {
        let next = match b < x1 {
            true => colour(b),
            false => now,
        };
        if b < x1 && (next.fg, next.curve) == (now.fg, now.curve) {
            continue;
        }
        out.push(Instance {
            x: Screen(i16::try_from(a).unwrap_or(i16::MAX)),
            w: Tex(u16::try_from(b - a).unwrap_or(0)),
            u: Tex(inst.u.0.saturating_add(u16::try_from(a - x0).unwrap_or(0))),
            color: now.fg,
            curve: now.curve,
            ..inst
        });
        a = b;
        now = next;
    }
}

/// **A line's geometry, the one function every drawn thing on it goes
/// through.** A character on a line of rendition `size` takes a box
/// `stretch.x()` cells wide and `stretch.y()` tall (`[[source:
/// dec-std-070]]` § 5.6.1): column `c`'s box starts at `x() * c` cells, and
/// a double-height line's box starts at its row's top for the top half and
/// a row above it for the bottom half, each row showing its half of it.
/// What this returns is the top of that box, in pixels, for a row whose
/// band begins at `top`.
pub(crate) fn box_top(size: LineRendition, band: Band) -> i32 {
    let top = px_i32(band.top);
    match size {
        LineRendition::DoubleBottom => top - (px_i32(band.bottom) - top),
        _ => top,
    }
}

/// The part of `inst` inside `band`, or nothing: a double-height line's
/// row draws only its half of each character's box.
pub(crate) fn clip_to(inst: Instance, band: Band) -> Option<Instance> {
    let (top, bottom) = (px_i32(band.top), px_i32(band.bottom));
    let (_, inside) = inst.cut_at(screen(top));
    let (above, _) = inside?.cut_at(screen(bottom));
    above
}

/// A shape built in a single-width cell's coordinates -- an underline, a
/// strike, an edge -- moved onto a line of rendition `size`: a solid is
/// stretched with the character it belongs to, `x` and its width by the
/// stretch across, and on a double-height line its offset in the cell and
/// its height by two down, then cut to the row's half; a mask keeps its
/// texels and only moves to its box.
pub(crate) fn stretched(inst: Instance, size: LineRendition, band: Band) -> Option<Instance> {
    let st = Stretch::of(size);
    let top = px_i32(band.top);
    let x = i32::from(inst.x.0) * st.x();
    let off = i32::from(inst.y.0) - top;
    let y = box_top(size, band) + off * st.y();
    let moved = match inst.kind {
        Kind::Solid => Instance {
            x: screen(x),
            y: screen(y),
            w: Tex(inst.w.0.saturating_mul(u16::try_from(st.x()).unwrap_or(1))),
            h: Tex(inst.h.0.saturating_mul(u16::try_from(st.y()).unwrap_or(1))),
            ..inst
        },
        _ => Instance {
            x: screen(x),
            y: screen(y),
            ..inst
        },
    };
    match st.y() {
        1 => Some(moved),
        _ => clip_to(moved, band),
    }
}

/// The contrast curve row text painted in `c` takes: its encoded colour,
/// alpha aside (`baud_domain::contrast`). Called once per rendition slot.
fn curve_row(c: Argb) -> CurveRow {
    CurveRow::of(c.r().get(), c.g().get(), c.b().get())
}

/// One cell's input to the shaper: its base and attached codepoints, a
/// range of its run's ([`ClusterList`]), so a run's cells cost no buffer
/// each.
#[derive(Clone)]
struct ClusterIn {
    /// Where the cell's codepoints are among its run's.
    cps: core::ops::Range<u32>,
    /// Offset of the cell within the run.
    at: CellIx,
}

impl ClusterIn {
    /// The cell's codepoints, of its run's `all`: none for a range `all`
    /// does not hold.
    fn of<'a>(&self, all: &'a [Codepoint]) -> &'a [Codepoint] {
        match (
            usize::try_from(self.cps.start),
            usize::try_from(self.cps.end),
        ) {
            (Ok(from), Ok(to)) => all.get(from..to).unwrap_or(&[]),
            _ => &[],
        }
    }
}

/// **A run's clusters as the shaper takes them**: one cell's codepoints
/// alone, at the run's first cell, or each cluster a range of the run's
/// codepoints ([`ClusterList`]). Borrowed either way, so a caller holding
/// a cell's codepoints shapes them without copying them.
#[derive(Clone, Copy)]
enum Clusters<'a> {
    /// One cell, at the run's first.
    One(&'a [Codepoint]),
    /// Each of `each` a range of `cps`.
    Ranged {
        cps: &'a [Codepoint],
        each: &'a [ClusterIn],
    },
}

impl<'a> Clusters<'a> {
    /// Each cluster's codepoints and its offset in the run, in order.
    fn iter(self) -> impl Iterator<Item = (&'a [Codepoint], CellIx)> {
        let (one, all, each): (_, &[Codepoint], &[ClusterIn]) = match self {
            Clusters::One(cps) => (Some((cps, CellIx(0))), &[], &[]),
            Clusters::Ranged { cps, each } => (None, cps, each),
        };
        one.into_iter()
            .chain(each.iter().map(move |cl| (cl.of(all), cl.at)))
    }

    /// The first cluster's codepoints: none for a run of none.
    fn first(self) -> &'a [Codepoint] {
        self.iter().next().map_or(&[], |(cps, _)| cps)
    }
}

/// **A run's clusters, gathered**: every cell's codepoints end to end in
/// one buffer and each cell's range of them in another, so gathering a
/// run of `n` cells costs the two buffers, kept where the caller keeps
/// them (`RowScratch`), and never one a cell.
///
/// **A range is a `u32`, checked**: a cluster whose codepoints end past
/// `u32::MAX` of the run's is left out of the run -- its codepoints
/// taken back -- and never given a range that wrapped onto another
/// cluster's. A run holds at most [`RUN_CELLS`] cells of a base and
/// [`ClusterSlot::CAPACITY`] marks each, cleared at each run's start, so
/// it never comes near.
#[derive(Default)]
struct ClusterList {
    cps: Vec<Codepoint>,
    each: Vec<ClusterIn>,
}

impl ClusterList {
    /// No cluster.
    fn clear(&mut self) {
        self.cps.clear();
        self.each.clear();
    }

    /// One cluster of `cps`, at `at`: left out, its codepoints taken
    /// back, where its range would end past a `u32`.
    fn push(&mut self, cps: &[Codepoint], at: CellIx) {
        let from = self.cps.len();
        self.cps.extend_from_slice(cps);
        match (u32::try_from(from), u32::try_from(self.cps.len())) {
            (Ok(start), Ok(end)) => self.each.push(ClusterIn {
                cps: start..end,
                at,
            }),
            _ => self.cps.truncate(from),
        }
    }

    fn clusters(&self) -> Clusters<'_> {
        Clusters::Ranged {
            cps: &self.cps,
            each: &self.each,
        }
    }
}

use row_scratch::RowScratch;

/// **The buffers one row's conversion fills**, the renderer's
/// ([`Renderer::convert_row`]), kept from row to row and frame to frame so
/// a converted row costs no buffer of its own: about two hundred a row
/// before, a codepoint buffer a cell and a cluster list a run among them.
///
/// **Empty when a conversion starts, by type**: [`RowScratch::begin`] is
/// the only way to have one -- it takes the kept buffers, or none on the
/// first row, and gives them back cleared, every one -- so no row reads
/// another's cells, map, shapes or clusters.
mod row_scratch {
    use super::{ClusterList, Col, Instance, PackedCell};

    pub(super) struct RowScratch {
        /// The row's cells, copied so shaping may borrow the renderer.
        pub(super) row: Vec<PackedCell>,
        /// A reordered row's logical column for each visual one.
        pub(super) logical: Vec<Col>,
        /// A reordered row's seams, where shaping stops.
        pub(super) seams: Vec<bool>,
        /// On a display line, the stored cell each visual cell shows.
        pub(super) stored: Vec<Option<(crate::select::WindowLine, Col)>>,
        /// The row's shapes, drawn after its glyphs.
        pub(super) shapes: Vec<Instance>,
        /// A sized line's ring parts, on its geometry already.
        pub(super) rings: Vec<Instance>,
        /// The run being gathered: its codepoints and its clusters' ranges.
        pub(super) run: ClusterList,
        /// Made here alone, so [`RowScratch::begin`] is the only
        /// constructor.
        _begun: Begun,
    }

    struct Begun;

    impl RowScratch {
        /// The buffers `kept` holds, each cleared, or new ones when none
        /// are kept.
        pub(super) fn begin(kept: Option<RowScratch>) -> RowScratch {
            let Some(mut s) = kept else {
                return RowScratch {
                    row: Vec::new(),
                    logical: Vec::new(),
                    seams: Vec::new(),
                    stored: Vec::new(),
                    shapes: Vec::new(),
                    rings: Vec::new(),
                    run: ClusterList::default(),
                    _begun: Begun,
                };
            };
            s.row.clear();
            s.logical.clear();
            s.seams.clear();
            s.stored.clear();
            s.shapes.clear();
            s.rings.clear();
            s.run.cps.clear();
            s.run.each.clear();
            s
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::{CellIx, Codepoint, Px, Screen, solid};
        use super::*;

        /// **`begin` gives back every kept buffer empty**: each filled,
        /// then begun again. Named breaks: each buffer left as it was.
        #[test]
        fn begin_empties_every_buffer() {
            let mut s = RowScratch::begin(None);
            let shape = solid(
                Screen(0),
                Screen(0),
                Px::of(1),
                Px::of(1),
                Default::default(),
            );
            s.row.push(PackedCell::EMPTY);
            s.logical.push(Col::ZERO);
            s.seams.push(true);
            s.stored.push(None);
            s.shapes.push(shape);
            s.rings.push(shape);
            s.run.push(&[Codepoint::from_char('a')], CellIx(0));
            let s = RowScratch::begin(Some(s));
            assert!(s.row.is_empty(), "the row's cells");
            assert!(s.logical.is_empty(), "the logical map");
            assert!(s.seams.is_empty(), "the seams");
            assert!(s.stored.is_empty(), "the stored cells");
            assert!(s.shapes.is_empty(), "the shapes");
            assert!(s.rings.is_empty(), "the rings");
            assert!(s.run.cps.is_empty(), "the run's codepoints");
            assert!(s.run.each.is_empty(), "the run's clusters");
        }
    }
}

use veils::{Force, VeilKind, VeilNow, VeilReach, VeilRest, Veiled};

/// **baud's veils over the screen, and the records they were drawn as**
/// (FS-C16): the fields the link under the pointer is drawn as, the find
/// field, the link preview's bar, the hover tip's lines and the notice,
/// which every path draws over the images after the text, each composed
/// on its own row as it shows ([`crate::overlay::Laid::OnScreen`]). A
/// frame whose veils, and everything their records are drawn from, are
/// the last one's draws the records it kept, read in place, and composes
/// none: the pointer at rest over a link costs a caret frame no
/// allocation, where every frame composed its fields again before.
///
/// **What the records are drawn from** ([`VeilKey`]): each veil -- its
/// composition, the cell its ground is read from -- and the row it lies
/// on, cell for cell as a composition's records read it, the live
/// screen's in the row's visual order; the width and the main display's
/// rows; the metrics; the selection; the blink's phases, the video, the
/// fields' selection colours and the message pair; where the rendition
/// table stands (its lending among it); the cluster store's epoch; and how
/// many times the atlas grew, as a sized line's key counts it. The key
/// covers every veil, so a change of any composes them again.
///
/// **Not the input method's composition**: a veil is composed laid as the
/// screen shows it, with no cursor block over it, so its records read the
/// composition nowhere -- the row's visual order, which a composition
/// written on that row moves, only through the cells the key holds -- and
/// a composition that changes beside a kept veil composes none
/// (`a_composition_that_changed_composes_no_veil`, and
/// `no_veil_is_composed_from_the_composition`, which holds every veil
/// compose to that).
///
/// **Kept only under its key, by type**: [`Veiled::records`] is the one
/// way to the records, and it composes them again whenever the last
/// [`Veiled::hold`] found the key moved, or the frame is full -- so no
/// frame draws records another veil was composed into.
mod veils {
    use super::{BlinkPhases, Composition, HoverBar, Instance, PackedCell, StyleStamp, Tally};
    use crate::cellmap::Hilite;
    use crate::color::Video;
    use crate::find::Field;
    use crate::message::MessageColours;
    use crate::metrics::Metrics;
    use crate::overlay::{Caret, Hints, Measure};
    use crate::select::Highlight;
    use baud_domain::{ClusterEpoch, Codepoint, Col, Cols, Row, Rows, Viewport};

    /// Which veil a kept one is, in the order a frame draws them.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum VeilKind {
        /// A field of the link under the pointer.
        Hover,
        /// The find field, its caret drawn after it.
        Find,
        /// The link preview's bar.
        LinkBar,
        /// A line of the hover tip.
        Tip,
        /// The embedder's notice, on its own row.
        Notice,
    }

    /// **One veil as it was drawn**: its composition, the cell its ground
    /// is read from, and the columns it covers on the width it was kept at.
    pub(super) struct VeilPart {
        pub(super) kind: VeilKind,
        pub(super) comp: Composition,
        pub(super) under: PackedCell,
        pub(super) span: (Col, Col),
    }

    /// One veil as the frame has it, borrowed where it is kept.
    #[derive(Clone, Copy)]
    enum VeilSource<'a> {
        Hover(&'a HoverBar),
        /// The find field, the link preview's bar or a line of the tip.
        Field(VeilKind, &'a Field),
        /// The notice's text, and the row it is drawn on.
        Notice(&'a [Codepoint], Row),
    }

    /// The notice's text as its field shows it on a screen `cols` wide:
    /// cut to the width, or padded to it with spaces.
    pub(super) fn notice_text(
        text: &[Codepoint],
        cols: Cols,
    ) -> impl Iterator<Item = Codepoint> + '_ {
        text.iter()
            .copied()
            .chain(core::iter::repeat(Codepoint::from_char(' ')))
            .take(cols.index())
    }

    impl VeilSource<'_> {
        fn kind(self) -> VeilKind {
            match self {
                VeilSource::Hover(_) => VeilKind::Hover,
                VeilSource::Field(kind, _) => kind,
                VeilSource::Notice(..) => VeilKind::Notice,
            }
        }

        /// Whether `part` is this veil, drawn on a screen `cols` wide.
        fn is(self, part: &VeilPart, cols: Cols) -> bool {
            let c = &part.comp;
            // A field and the notice are drawn as the overlay draws any
            // field: measured as the core measures at power-on, its caret
            // its own ([`Field::composition`]), its ground the default cell.
            let field = c.measure == Measure::default()
                && c.caret == Caret::Hidden
                && part.under == PackedCell::EMPTY;
            part.kind == self.kind()
                && match self {
                    VeilSource::Hover(b) => *c == b.comp && part.under == b.under,
                    VeilSource::Field(_, f) => {
                        field
                            && c.row == f.row
                            && c.col == f.col
                            && c.text == f.text
                            && c.hints == f.hints
                    }
                    VeilSource::Notice(text, row) => {
                        field
                            && c.row == row
                            && c.col == Col::ZERO
                            && c.text.iter().copied().eq(notice_text(text, cols))
                            && c.hints.len() == cols.index()
                            && c.hints.iter().all(|&h| h == Hints::MESSAGE)
                    }
                }
        }

        /// This veil, drawn on a screen `cols` wide, into `part`'s buffers.
        fn copy_into(self, part: &mut VeilPart, cols: Cols) {
            part.kind = self.kind();
            let c = &mut part.comp;
            match self {
                VeilSource::Hover(b) => {
                    c.row = b.comp.row;
                    c.col = b.comp.col;
                    c.text.clone_from(&b.comp.text);
                    c.measure = b.comp.measure;
                    c.hints.clone_from(&b.comp.hints);
                    c.caret = b.comp.caret;
                    part.under = b.under;
                }
                VeilSource::Field(_, f) => {
                    c.row = f.row;
                    c.col = f.col;
                    c.text.clone_from(&f.text);
                    c.measure = Measure::default();
                    c.hints.clone_from(&f.hints);
                    c.caret = Caret::Hidden;
                    part.under = PackedCell::EMPTY;
                }
                VeilSource::Notice(text, row) => {
                    c.row = row;
                    c.col = Col::ZERO;
                    c.text.clear();
                    c.text.extend(notice_text(text, cols));
                    c.measure = Measure::default();
                    c.hints.clear();
                    c.hints.resize(cols.index(), Hints::MESSAGE);
                    c.caret = Caret::Hidden;
                    part.under = PackedCell::EMPTY;
                }
            }
            let (start, end) = c.columns(cols);
            part.span = (Col::at_index(start), Col::at_index(end));
        }

        /// This veil, drawn on a screen `cols` wide, new.
        fn made(self, cols: Cols) -> VeilPart {
            let mut part = VeilPart {
                kind: self.kind(),
                comp: Composition {
                    row: Row::from_index(0),
                    col: Col::ZERO,
                    text: Vec::new(),
                    measure: Measure::default(),
                    hints: Vec::new(),
                    caret: Caret::Hidden,
                },
                under: PackedCell::EMPTY,
                span: (Col::ZERO, Col::ZERO),
            };
            self.copy_into(&mut part, cols);
            part
        }
    }

    /// The rest of a [`VeilKey`], each part a value.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(super) struct VeilRest {
        pub(super) cols: Cols,
        pub(super) main: Rows,
        pub(super) metrics: Metrics,
        pub(super) selection: Option<Highlight>,
        pub(super) blink: BlinkPhases,
        pub(super) video: Video,
        pub(super) hilite: Hilite,
        pub(super) message: MessageColours,
        pub(super) styles: StyleStamp,
        pub(super) clusters: ClusterEpoch,
        pub(super) atlas: Tally,
    }

    /// **The frame's veils as it has them, borrowed**: each where the
    /// renderer keeps it, the live screen its records read the rows of,
    /// and the layout that orders them.
    pub(super) struct VeilNow<'a> {
        pub(super) bars: &'a [HoverBar],
        pub(super) find: Option<&'a Field>,
        pub(super) link_bar: Option<&'a Field>,
        pub(super) tips: &'a [Field],
        pub(super) notice: Option<(&'a [Codepoint], Row)>,
        pub(super) screen: Viewport<'a>,
        pub(super) orders: &'a crate::bidi::Orders,
        pub(super) rest: VeilRest,
    }

    impl<'a> VeilNow<'a> {
        /// Every veil, in the order a frame draws them.
        fn sources(&self) -> impl Iterator<Item = VeilSource<'a>> + use<'a> {
            let (bars, tips): (&'a [HoverBar], &'a [Field]) = (self.bars, self.tips);
            let field = |kind: VeilKind| move |f: &'a Field| VeilSource::Field(kind, f);
            bars.iter()
                .map(VeilSource::Hover)
                .chain(self.find.map(field(VeilKind::Find)))
                .chain(self.link_bar.map(field(VeilKind::LinkBar)))
                .chain(tips.iter().map(field(VeilKind::Tip)))
                .chain(self.notice.map(|(text, row)| VeilSource::Notice(text, row)))
        }

        /// Row `row` as a veil's records read it: the live screen's cell
        /// each visual column shows, none past its cells
        /// (`crate::overlay::composition_parts`).
        fn cells(&self, row: Row) -> impl Iterator<Item = Option<PackedCell>> + '_ {
            let cells = self.screen.row(row);
            (0..self.rest.cols.index()).map(move |v| {
                cells
                    .get(self.orders.logical_col(row, Col::at_index(v)).index())
                    .copied()
            })
        }
    }

    /// **What a frame's veils were composed from**, compared where each
    /// stands ([`VeilKey::holds`]) and copied into the kept buffers only
    /// when they differ ([`VeilKey::take`]), so a steady veil costs a frame
    /// no buffer.
    struct VeilKey {
        parts: Vec<VeilPart>,
        /// Each part's row, as its records read it, one after another.
        cells: Vec<Option<PackedCell>>,
        rest: VeilRest,
    }

    impl VeilKey {
        /// Whether the veils composed under this key are the ones `now`
        /// draws: every part compared, the rows they lie on among them.
        fn holds(&self, now: &VeilNow<'_>) -> bool {
            let cols = now.rest.cols;
            self.rest == now.rest
                && self.parts.len() == now.sources().count()
                && self
                    .parts
                    .iter()
                    .zip(now.sources())
                    .all(|(p, s)| s.is(p, cols))
                && self.cells.len() == self.parts.len() * cols.index()
                && self
                    .parts
                    .iter()
                    .zip(self.cells.chunks(cols.index().max(1)))
                    .all(|(p, kept)| now.cells(p.comp.row).eq(kept.iter().copied()))
        }

        /// The key `now` makes, into this key's buffers.
        fn take(&mut self, now: &VeilNow<'_>) {
            let cols = now.rest.cols;
            self.parts.truncate(now.sources().count());
            for (i, s) in now.sources().enumerate() {
                match self.parts.get_mut(i) {
                    Some(p) => s.copy_into(p, cols),
                    None => self.parts.push(s.made(cols)),
                }
            }
            self.cells.clear();
            for p in &self.parts {
                self.cells.extend(now.cells(p.comp.row));
            }
            self.rest = now.rest;
        }

        /// The key `now` makes, new.
        fn of(now: &VeilNow<'_>) -> VeilKey {
            let mut key = VeilKey {
                parts: Vec::new(),
                cells: Vec::new(),
                rest: now.rest,
            };
            key.take(now);
            key
        }
    }

    /// Which veils a path draws: the CPU and instance paths those on the
    /// main display's rows and the notice on its own; the cell path every
    /// one.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum VeilReach {
        Main,
        Every,
    }

    impl VeilReach {
        fn takes(self, part: &VeilPart, main: Rows) -> bool {
            match self {
                VeilReach::Every => true,
                VeilReach::Main => {
                    part.kind == VeilKind::Notice || part.comp.row.index() < main.index()
                }
            }
        }
    }

    /// Whether a frame composes every veil again whatever the key says:
    /// the CPU path's full frame does.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum Force {
        No,
        Full,
    }

    /// Whether the kept records were composed under the key held.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Drawn {
        Stale,
        Kept,
    }

    /// Whether the find field is among the veils drawn, so its caret goes
    /// after it.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum FindDrawn {
        No,
        Yes,
    }

    /// **One path's veils and the records they were composed into**: the
    /// fields of the link under the pointer and the find field's before the
    /// find field's caret, and the link preview's bar, the tip's lines and
    /// the notice's after it, as a frame draws them.
    pub(super) struct Veiled {
        reach: VeilReach,
        key: Option<VeilKey>,
        drawn: Drawn,
        before: Vec<Instance>,
        after: Vec<Instance>,
        find: FindDrawn,
        /// Every veil composed, over the renderer's life.
        composed: Tally,
    }

    impl Veiled {
        /// None held, for a path that draws the veils `reach` takes.
        pub(super) fn reaching(reach: VeilReach) -> Veiled {
            Veiled {
                reach,
                key: None,
                drawn: Drawn::Stale,
                before: Vec::new(),
                after: Vec::new(),
                find: FindDrawn::No,
                composed: Tally::default(),
            }
        }

        /// **Holds the key `now` makes**: the records kept stay drawn only
        /// where it is the key they were composed under.
        pub(super) fn hold(&mut self, now: &VeilNow<'_>) {
            if self.key.as_ref().is_some_and(|k| k.holds(now)) {
                return;
            }
            match &mut self.key {
                Some(k) => k.take(now),
                None => self.key = Some(VeilKey::of(now)),
            }
            self.drawn = Drawn::Stale;
        }

        /// The veils this path draws, each its kind, its row and the
        /// columns it covers, `start..end`.
        pub(super) fn spans(&self) -> impl Iterator<Item = (VeilKind, Row, Col, Col)> + '_ {
            let reach = self.reach;
            self.key.iter().flat_map(move |k| {
                let main = k.rest.main;
                k.parts
                    .iter()
                    .filter(move |p| reach.takes(p, main))
                    .map(|p| (p.kind, p.comp.row, p.span.0, p.span.1))
            })
        }

        /// **The veils' records, appended to `out`**: composed again --
        /// `compose` appending a veil's records to the buffer it is handed
        /// -- where the last [`Veiled::hold`] found the key moved or
        /// `force` asks it, else the records kept, read in place; the find
        /// field's `caret` after its field.
        pub(super) fn records(
            &mut self,
            force: Force,
            caret: &[Instance],
            mut compose: impl FnMut(&VeilPart, &mut Vec<Instance>),
            out: &mut Vec<Instance>,
        ) {
            if self.drawn == Drawn::Stale || force == Force::Full {
                self.before.clear();
                self.after.clear();
                self.find = FindDrawn::No;
                let reach = self.reach;
                if let Some(k) = &self.key {
                    let main = k.rest.main;
                    for p in k.parts.iter().filter(|p| reach.takes(p, main)) {
                        match p.kind {
                            VeilKind::Hover | VeilKind::Find => compose(p, &mut self.before),
                            VeilKind::LinkBar | VeilKind::Tip | VeilKind::Notice => {
                                compose(p, &mut self.after)
                            }
                        }
                        if p.kind == VeilKind::Find {
                            self.find = FindDrawn::Yes;
                        }
                        self.composed.bump();
                    }
                }
                self.drawn = Drawn::Kept;
            }
            out.extend_from_slice(&self.before);
            if self.find == FindDrawn::Yes {
                out.extend_from_slice(caret);
            }
            out.extend_from_slice(&self.after);
        }

        /// Every veil this path composed, over the renderer's life.
        pub(super) fn composed(&self) -> Tally {
            self.composed
        }
    }
}

use links::{
    KeptSolids, LinkInputs, OwnOrder, Placing, Set, SolidsKey, StruckFound, StruckKey, StruckMoved,
};
pub use links::{StruckCells, StruckGen};

/// **The frame's links, kept** (FS-C6): what the lines at rest, the strikes
/// and `hover = "line"`'s solids are drawn from -- the driver's part held
/// behind the setters that alone write it -- and the solids last placed,
/// with the key they were placed under. No cell is read for a solid: a
/// line's place is its row, its columns and the row's order, and its colour
/// its rendition's word resolved under the frame's colours. So a caret frame
/// over links nothing moved places none.
mod links {
    use super::{CopyOrder, Force, Instance, StyleInputs, Tally};
    use crate::Orders;
    use crate::bidi::LaidSeal;
    use crate::cellmap::Hilite;
    use crate::linked::{LineColour, LineStyle, LinkHover, LinkLine, MissingPaint};
    use crate::message::MessageColours;
    use crate::metrics::Metrics;
    use crate::select::Highlight;
    use baud_domain::{Col, Content, Row, RowGens, Rows, Seal, Viewport, WindowAt};
    use core::ops::Range;
    use core::sync::atomic::{AtomicU64, Ordering};

    /// **How many times the driver's link inputs moved**: one more each
    /// time a setter of [`LinkInputs`] is handed a value other than the one
    /// it holds, and never otherwise, so the driver's own compare of its
    /// lists each frame is where a Paths answer, a pointer or a config read
    /// reaches the key.
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    pub(super) struct InputsGen(Tally);

    /// Whether a setter was handed another value than the one it held.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum Set {
        Same,
        Moved,
    }

    /// **What a frame's links are drawn from, as the driver set it**: the
    /// lines at rest and their style and colour, the strikes and those the
    /// gesture holds, `[link] missing_color`, and the link under the
    /// pointer. Each field is written only by its setter, which moves
    /// [`InputsGen`] when, and only when, the value differs, and is read
    /// through its accessor.
    pub(super) struct LinkInputs {
        lines: Vec<LinkLine>,
        style: LineStyle,
        colour: LineColour,
        strikes: Vec<LinkLine>,
        held: Vec<LinkLine>,
        missing: MissingPaint,
        hover: Option<LinkHover>,
        moves: InputsGen,
    }

    impl LinkInputs {
        /// No link, the lines solid in the box's colour, the strikes'
        /// text grey.
        pub(super) fn none() -> LinkInputs {
            LinkInputs {
                lines: Vec::new(),
                style: LineStyle::Solid,
                colour: LineColour::Box,
                strikes: Vec::new(),
                held: Vec::new(),
                missing: MissingPaint::Gray,
                hover: None,
                moves: InputsGen::default(),
            }
        }

        pub(super) fn moves(&self) -> InputsGen {
            self.moves
        }

        pub(super) fn lines(&self) -> &[LinkLine] {
            &self.lines
        }

        pub(super) fn style(&self) -> LineStyle {
            self.style
        }

        pub(super) fn colour(&self) -> LineColour {
            self.colour
        }

        /// The runs struck at rest, then the ones the gesture holds.
        pub(super) fn strikes(&self) -> &[LinkLine] {
            &self.strikes
        }

        pub(super) fn held(&self) -> &[LinkLine] {
            &self.held
        }

        pub(super) fn missing(&self) -> MissingPaint {
            self.missing
        }

        pub(super) fn hover(&self) -> Option<&LinkHover> {
            self.hover.as_ref()
        }

        fn moved(&mut self) -> Set {
            self.moves.0.bump();
            Set::Moved
        }

        /// The lines at rest, copied only when they changed.
        pub(super) fn set_lines(&mut self, lines: &[LinkLine]) -> Set {
            if self.lines == lines {
                return Set::Same;
            }
            self.lines.clear();
            self.lines.extend_from_slice(lines);
            self.moved()
        }

        pub(super) fn set_style(&mut self, style: LineStyle) -> Set {
            if self.style == style {
                return Set::Same;
            }
            self.style = style;
            self.moved()
        }

        pub(super) fn set_colour(&mut self, colour: LineColour) -> Set {
            if self.colour == colour {
                return Set::Same;
            }
            self.colour = colour;
            self.moved()
        }

        /// The strikes at rest then the held ones, and the held ones
        /// apart, each copied only when it changed.
        pub(super) fn set_strikes(&mut self, at_rest: &[LinkLine], held: &[LinkLine]) -> Set {
            let mut set = Set::Same;
            let same = self.strikes.len() == at_rest.len() + held.len()
                && self.strikes.iter().eq(at_rest.iter().chain(held));
            if !same {
                self.strikes.clear();
                self.strikes.extend_from_slice(at_rest);
                self.strikes.extend_from_slice(held);
                set = self.moved();
            }
            if self.held != held {
                self.held.clear();
                self.held.extend_from_slice(held);
                set = self.moved();
            }
            set
        }

        pub(super) fn set_missing(&mut self, paint: MissingPaint) -> Set {
            if self.missing == paint {
                return Set::Same;
            }
            self.missing = paint;
            self.moved()
        }

        /// The link under the pointer, compared before it is copied into
        /// the buffer the last one left.
        pub(super) fn set_hover(&mut self, hover: Option<&LinkHover>) -> Set {
            if self.hover.as_ref() == hover {
                return Set::Same;
            }
            match (hover, &mut self.hover) {
                (Some(h), Some(mine)) => mine.clone_from(h),
                (h, mine) => *mine = h.cloned(),
            }
            self.moved()
        }
    }

    /// **Everything a placement's solids are drawn from but the rows'
    /// order**: the driver's inputs, the colours a word resolves to (the
    /// theme, the video and the core's colours), the fields' selection
    /// colours, `[message]`'s colours and the cell geometry.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) struct SolidsKey {
        pub(super) inputs: InputsGen,
        pub(super) colours: StyleInputs,
        pub(super) hilite: Hilite,
        pub(super) message: MessageColours,
        pub(super) metrics: Metrics,
    }

    /// **Whether every row a link's solid is drawn on is drawn in its own
    /// order, and no display is held**: there a run is its own cells,
    /// whatever wrote the row's paragraph or any other, so a key that held
    /// then and holds now places the solids where they are, whatever the
    /// maps were laid out as since.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum OwnOrder {
        Every,
        NotEvery,
    }

    /// **What a placement was made under**: its key, whether every row a
    /// solid was drawn on was in its own order with no display held, and
    /// the layout's seal then ([`Orders::laid_seal`]): a solid on a row in
    /// another order, or under a display, is placed from the rows' maps
    /// and the display, and from nothing else the key does not hold.
    #[derive(Clone, Copy, Debug)]
    pub(super) struct PlacedUnder {
        key: SolidsKey,
        order: OwnOrder,
        laid: LaidSeal,
    }

    impl PlacedUnder {
        /// **Whether solids placed under this are the ones `now`, `order`
        /// and `orders` place**: the key the same, and either every row a
        /// solid is drawn on in its own order then and now, or no row's
        /// order or joins nor the display laid out otherwise since
        /// ([`Orders::moved_since`]) -- a row laid out again as it was is
        /// not.
        fn holds(&self, now: SolidsKey, order: OwnOrder, orders: &Orders) -> bool {
            self.key == now
                && ((self.order == OwnOrder::Every && order == OwnOrder::Every)
                    || !orders.moved_since(&self.laid))
        }
    }

    /// The solids to place again, into a kept buffer, and what they are
    /// placed under.
    pub(super) struct Placement {
        under: PlacedUnder,
        solids: Vec<Instance>,
    }

    impl Placement {
        pub(super) fn solids(&mut self) -> &mut Vec<Instance> {
            &mut self.solids
        }
    }

    /// What [`KeptSolids::hold`] found.
    pub(super) enum Placing {
        /// The solids kept are this frame's.
        Kept,
        /// The solids are placed again.
        Place(Placement),
    }

    /// **The solids of the lines at rest, the strikes and `hover = "line"`,
    /// and what they were placed under**: `None` until a placement is
    /// given back, and while one is out.
    pub(super) struct KeptSolids {
        under: Option<PlacedUnder>,
        solids: Vec<Instance>,
    }

    impl KeptSolids {
        pub(super) fn none() -> KeptSolids {
            KeptSolids {
                under: None,
                solids: Vec::new(),
            }
        }

        /// **Holds the key `now`**: the solids kept are this frame's where
        /// what they were placed under holds `now`, `order` and `orders`
        /// ([`PlacedUnder::holds`]) -- on a row in another order and under
        /// a held display as on a row in its own -- and the frame is not
        /// one that draws everything again (`force`). Otherwise their
        /// buffer is handed out, emptied, to be placed into under `now`,
        /// `order` and the layout's seal now, and given back
        /// ([`KeptSolids::placed`]); what they were placed under is
        /// forgotten until then.
        pub(super) fn hold(
            &mut self,
            now: SolidsKey,
            order: OwnOrder,
            orders: &Orders,
            force: Force,
        ) -> Placing {
            if force == Force::No
                && self
                    .under
                    .as_ref()
                    .is_some_and(|u| u.holds(now, order, orders))
            {
                return Placing::Kept;
            }
            self.under = None;
            let mut solids = core::mem::take(&mut self.solids);
            solids.clear();
            Placing::Place(Placement {
                under: PlacedUnder {
                    key: now,
                    order,
                    laid: orders.laid_seal(),
                },
                solids,
            })
        }

        /// The solids `p` placed, kept under what it was placed under.
        pub(super) fn placed(&mut self, p: Placement) {
            self.under = Some(p.under);
            self.solids = p.solids;
        }

        /// The solids the last placement made: the one read of them.
        pub(super) fn solids(&self) -> &[Instance] {
            &self.solids
        }
    }

    /// **Everything the struck cells are found from but the rows' content
    /// and order**: the driver's inputs (the strikes and `[link]
    /// missing_color` among them), the selection, the copy's order and
    /// rows, and the window's physical rows.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) struct StruckKey {
        pub(super) inputs: InputsGen,
        pub(super) highlight: Option<Highlight>,
        pub(super) order: CopyOrder,
        pub(super) rows: Rows,
        pub(super) at: WindowAt,
    }

    /// Whether the struck cells a frame lends are the last frame's.
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    pub(super) enum StruckFound {
        /// Kept: their key held and no row they are on was written.
        Kept,
        /// Found again.
        #[default]
        Again,
    }

    /// **Which struck cells a lending reads, named by value**: a value the
    /// process gives out once -- to a renderer when it is made, to a find
    /// that gives cells other, by value, than the find before it gave, a
    /// run on another row or over other columns, and to cells lost and
    /// found again from nothing ([`crate::Renderer::take_struck`]) -- and
    /// kept by a find that gives the same cells and while they are kept.
    /// So two equal generations, of one renderer or of two, name the same
    /// struck cells, and a renderer made again starts at one no reader
    /// holds: what a
    /// reader of the struck cells keys on to know they moved without a cell
    /// written. A change of `[link] missing_color` that strikes the same
    /// cells moves the driver's inputs, not this.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct StruckGen(u64);

    impl StruckGen {
        /// A generation the process gave out no time before.
        pub(super) fn fresh() -> StruckGen {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            StruckGen(NEXT.fetch_add(1, Ordering::Relaxed))
        }
    }

    /// Whether a find gave other struck cells, by value, than the find
    /// before it.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum StruckMoved {
        Same,
        Moved,
    }

    /// **The struck runs, written only by a find that is compared**: the
    /// runs and the buffer of the find before, which trade places at each
    /// find, so neither is made again, and nothing outside this module
    /// writes either. `Runs::again` is the one way in, and it says
    /// whether the cells came out otherwise, so a generation keyed on it
    /// cannot miss a change of the cells.
    mod found {
        use super::StruckMoved;
        use baud_domain::Row;
        use core::ops::Range;

        #[derive(Default)]
        pub(super) struct Runs {
            cells: Vec<(Row, Range<usize>)>,
            was: Vec<(Row, Range<usize>)>,
        }

        impl Runs {
            /// The runs the last find gave.
            pub(super) fn cells(&self) -> &[(Row, Range<usize>)] {
                &self.cells
            }

            /// **The runs found again by `find`**, into the emptied buffer
            /// of the find before the last, and compared, by value, with the
            /// last find's.
            pub(super) fn again(
                &mut self,
                find: impl FnOnce(&mut Vec<(Row, Range<usize>)>),
            ) -> StruckMoved {
                core::mem::swap(&mut self.cells, &mut self.was);
                self.cells.clear();
                find(&mut self.cells);
                match self.cells == self.was {
                    true => StruckMoved::Same,
                    false => StruckMoved::Moved,
                }
            }
        }
    }

    /// **The cells of a copy's rows each struck link's text covers**, as
    /// [`crate::Renderer::lend`] takes them: a run of columns a row, the
    /// rows in order, by visual column for a copy in drawn order
    /// ([`CopyOrder::Visual`]) and by stored column for one in the core's
    /// ([`CopyOrder::Stored`]). Kept with the key they were found under, the
    /// seal the term held then and the layout's seal
    /// ([`Orders::laid_seal`]), and found again into the same buffer only
    /// when that key moved, a row the find read was written since -- a
    /// selection reads its cells' content -- or any row's order or the
    /// display was laid out otherwise since ([`Orders::moved_since`]): a
    /// row drawn in another order keeps its struck cells while its order
    /// holds, as one in its own does. Taken out of the renderer for the
    /// lending it is read by and put back ([`crate::Renderer::take_struck`],
    /// [`crate::Renderer::put_struck`]). Made only by the renderer, so the
    /// struck cells a driver puts back are ones the renderer found.
    pub struct StruckCells {
        key: Option<(StruckKey, Seal, LaidSeal)>,
        runs: found::Runs,
        /// The rows whose cells the last find read: each strike's own, and
        /// every row its runs were drawn from -- another than its own under
        /// a display.
        read: Vec<Row>,
        found: StruckFound,
    }

    impl StruckCells {
        /// None found yet: the first [`StruckCells::hold`] finds them.
        pub(super) fn new() -> StruckCells {
            StruckCells {
                key: None,
                runs: found::Runs::default(),
                read: Vec::new(),
                found: StruckFound::Again,
            }
        }

        /// The runs, a row's in order and the rows in order.
        pub fn cells(&self) -> &[(Row, Range<usize>)] {
            self.runs.cells()
        }

        /// Whether the last [`StruckCells::hold`] kept them.
        pub(super) fn found(&self) -> StruckFound {
            self.found
        }

        /// **The cells for `now`**: kept where they were found under `now`,
        /// no row the find read was written since the seal they were found
        /// under (`gens`), and no row's order nor the display was laid out
        /// otherwise since the layout's seal then (`orders`); otherwise
        /// found again from `strikes`, drawn through `orders`, over `view`,
        /// the window the strikes were read on, and kept under `now`, `seal`
        /// and the layout's seal now. Whether they came out otherwise than
        /// the last find gave, by value: kept cells did not. A selected cell
        /// is left out, as it keeps the selection's colours, and so is every
        /// cell while `missing_color = "none"`, which changes no colour.
        #[allow(clippy::too_many_arguments)]
        pub(super) fn hold(
            &mut self,
            now: StruckKey,
            gens: &RowGens<'_>,
            seal: Seal,
            strikes: &[LinkLine],
            missing: MissingPaint,
            orders: &Orders,
            view: &Viewport<'_>,
        ) -> StruckMoved {
            let kept = self.key.as_ref().is_some_and(|(k, was, laid)| {
                *k == now
                    && !orders.moved_since(laid)
                    && !self.read.iter().any(|&r| gens.after(r, was))
            });
            if kept {
                self.found = StruckFound::Kept;
                return StruckMoved::Same;
            }
            self.found = StruckFound::Again;
            self.key = Some((now, seal, orders.laid_seal()));
            self.read.clear();
            let read = &mut self.read;
            self.runs.again(|cells| {
                if !strikes.is_empty() && missing != MissingPaint::Kept {
                    find(cells, read, now, strikes, orders, view);
                }
            })
        }
    }

    /// **The cells `strikes` cover**, found into the emptied `cells` and
    /// the rows the find read into `read`, as [`StruckCells::hold`] says.
    fn find(
        cells: &mut Vec<(Row, Range<usize>)>,
        read: &mut Vec<Row>,
        now: StruckKey,
        strikes: &[LinkLine],
        orders: &Orders,
        view: &Viewport<'_>,
    ) {
        for l in strikes {
            if !read.contains(&l.row) {
                read.push(l.row);
            }
            for (row, at, n) in orders.stored_runs(l.row, l.from, l.to) {
                if row.index() >= now.rows.index() {
                    continue;
                }
                if !read.contains(&row) {
                    read.push(row);
                }
                for v in at.index()..at.index() + n.index() {
                    let logical = orders.logical_col(row, Col::at_index(v));
                    let content = view
                        .row(row)
                        .get(logical.index())
                        .map_or(Content::EMPTY, |c| c.content);
                    if now
                        .highlight
                        .is_some_and(|h| orders.selects(&h, row, logical, content))
                    {
                        continue;
                    }
                    let x = match now.order {
                        CopyOrder::Visual => v,
                        CopyOrder::Stored => logical.index(),
                    };
                    match cells.last_mut() {
                        Some((r, cols)) if *r == row && cols.end == x => cols.end = x + 1,
                        _ => cells.push((row, x..x + 1)),
                    }
                }
            }
        }
        // The rows in order, a row's runs in the order found: the order
        // the lending meets its cells in, which numbers its slots. An
        // insertion sort, stable and in place: a handful of runs.
        for i in 1..cells.len() {
            let mut j = i;
            while j > 0 && cells[j - 1].0.index() > cells[j].0.index() {
                cells.swap(j - 1, j);
                j -= 1;
            }
        }
    }
}

/// **The most cells of its word either side a joined cell is shaped with**,
/// whatever a face's rules reach: twice the 8 the widest face measured
/// reaches ([`baud_font::joined_reach`]: Noto Sans Mongolian 8, Noto Sans
/// Syriac 7, SF Arabic 5, Courier New 3), and few enough that the cell,
/// its context and a pair fit a run's forms (64 cells) and the glyphs kept
/// of them a [`Shaped`].
const WORD_REACH_MAX: u32 = 16;

/// The widest of `reaches`.
fn widest_of(reaches: &[Count]) -> Count {
    reaches
        .iter()
        .copied()
        .max_by_key(|c| c.value())
        .unwrap_or(Count::ZERO)
}

/// The cells of a joined cell's word before it and after it, as the
/// layout keeps them ([`crate::bidi::Orders::around`]).
pub(crate) type WordAround<'w> = (&'w [crate::bidi::WordCell], &'w [crate::bidi::WordCell]);

/// **The cells of a joined cell's word its shaping reads on one face**,
/// each its codepoints and its form: before it in logical order, the
/// nearest last, and after it, the nearest first.
#[derive(Default)]
pub(crate) struct Word {
    pub(crate) before: Vec<(Vec<Codepoint>, Option<baud_domain::JoiningForm>)>,
    pub(crate) after: Vec<(Vec<Codepoint>, Option<baud_domain::JoiningForm>)>,
}

impl Word {
    /// No cell either side: a letter shaped alone.
    pub(crate) fn is_empty(&self) -> bool {
        self.before.is_empty() && self.after.is_empty()
    }

    /// `key` with the word folded in: each side's count, then each cell's
    /// codepoints and form, so two cells shaped with different words are
    /// two entries. Bit 31 is set on each count, which no codepoint and
    /// no other word folded into a run's key has. A letter with no word
    /// keeps the key it had alone.
    fn fold(&self, key: Key) -> Key {
        if self.is_empty() {
            return key;
        }
        let count = |n: usize, side: u32| 1 << 31 | side | u32::try_from(n).unwrap_or(0);
        let mut key = key.fold(count(self.before.len(), 0));
        for (cps, form) in self.before.iter().chain(&self.after) {
            for c in cps {
                key = key.fold(c.value());
            }
            key = match form {
                Some(f) => fold_form(key, FormKey::Cell, *f),
                None => key.fold(1 << 31 | 1 << 27),
            };
        }
        key.fold(count(self.after.len(), 1 << 28))
    }
}

/// What a joining form is folded into a run's key for: a joined cell or a
/// lam-alef pair, so neither shares an entry with a run of the same
/// codepoints. The word folded has bit 30 set, which no codepoint has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FormKey {
    Cell,
    Pair,
}

/// `key` with a joining form folded in for `what`, the form in the word's
/// low three bits.
fn fold_form(key: Key, what: FormKey, form: baud_domain::JoiningForm) -> Key {
    let mark = match what {
        FormKey::Cell => 1 << 30,
        FormKey::Pair => 1 << 29 | 1 << 30,
    };
    key.fold(
        mark | match form {
            baud_domain::JoiningForm::Isolated => 0,
            baud_domain::JoiningForm::Final => 1,
            baud_domain::JoiningForm::Initial => 2,
            baud_domain::JoiningForm::Medial => 3,
            baud_domain::JoiningForm::Final2 => 4,
            baud_domain::JoiningForm::Final3 => 5,
            baud_domain::JoiningForm::Medial2 => 6,
        },
    )
}

/// Which side of a lam-alef pair its lam is drawn on: the right in a
/// right-to-left run, the left where the pair is drawn in its stored order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LamAt {
    Left,
    Right,
}

/// Whether a face formed a lam-alef pair's ligature.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Ligated {
    No,
    Yes,
}

/// A lam-alef pair's glyphs: those its lam's cell draws, those its alef's
/// does, and whether they are the ligature.
#[derive(Clone, Debug)]
pub(crate) struct PairGlyphs {
    pub(crate) lams: Vec<ClusterGlyph>,
    pub(crate) alefs: Vec<ClusterGlyph>,
    pub(crate) ligated: Ligated,
}

/// **Where a lam-alef pair's ligature is drawn**: over the cells from the
/// lam's to the alef's, so many of them, the lam on the given side -- two
/// for a pair in adjacent cells, more where erased cells lie between them;
/// or in the lam's cell alone, for a pair a soft wrap parts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PairSpan {
    Cells(LamAt, Cols),
    LamCell,
}

impl PairSpan {
    /// The cells the ligature is centred over.
    fn cells(self) -> Cols {
        match self {
            PairSpan::Cells(_, n) => n,
            PairSpan::LamCell => Cols::of_index(1),
        }
    }
}

/// Which lookups a script selects on one face, how they place its marks,
/// which engine shapes it ([`Engine`]) and which way it runs, numbered: two
/// scripts with the same number shape alike there, since those are all a
/// script changes in baud's shaper, so a run may cross between them. A run
/// is cut only where this changes, not wherever the script does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Selection(u8);

/// Whether the scripts select different lookups on a face at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Varies {
    /// Every script selects the same lookups and places marks alike: a run
    /// is never cut by script, and no row's scripts need settling for this
    /// face. STIX Two Math is not this, and since they carry Devanagari's
    /// own tables neither are the embedded text faces, whose `DFLT`, `latn`
    /// and `hebr` name the same lookups all the same. Such a face's runs
    /// are shaped as Common, left to right, whatever their script.
    No,
    Yes,
}

/// One face's [`Selection`] per script.
#[derive(Debug)]
struct Selections {
    by_script: PerScript<Selection>,
    /// Whether a script selects what Common selects -- the same lookups,
    /// mark placement and engine -- and so is shaped as Common, left to
    /// right, even on a face that varies for another script: direction
    /// alone does not cut a run. The embedded faces vary for Devanagari
    /// alone, and their Latin and Hebrew rows shape as they did on a face
    /// that varies for nothing.
    alike: PerScript<bool>,
    varies: Varies,
}

impl Selections {
    /// The `GSUB` and `GPOS` lookups each script selects on `face` under
    /// the grid features, how its marks are placed ([`MarkPlacement`]), the
    /// engine that shapes it ([`Engine`]: Devanagari on a face with a
    /// Devanagari table is reordered, whatever lookups the grid features
    /// name) and its direction, numbered by first appearance. The lookups are gathered
    /// once per pair of Script tables chosen with a placement, not once per
    /// script: a face lists a handful of tables, and 175 scripts share
    /// them. A face varies when the lookups or the placement do; direction
    /// alone does not make it vary, so a face whose scripts all select
    /// alike keeps shaping every run as Common.
    fn of(face: &Face<'_>) -> Selections {
        type Chosen = (Option<ScriptTag>, Option<ScriptTag>, MarkPlacement, Engine);
        type Shaping = (Option<LookupSet>, Option<LookupSet>, MarkPlacement, Engine);
        let mut tables: Vec<(Chosen, usize)> = Vec::new();
        let mut seen: Vec<Shaping> = Vec::new();
        let mut numbered: Vec<(usize, Direction)> = Vec::new();
        let by_script = PerScript::from_fn(|s| {
            let chosen = (
                face.gsub().and_then(|g| g.chosen(s)),
                face.gpos().and_then(|g| g.chosen(s)),
                MarkPlacement::of(face, s),
                Engine::of(face, s),
            );
            let i = match tables.iter().find(|(c, _)| *c == chosen) {
                Some(&(_, i)) => i,
                None => {
                    let key = (
                        face.gsub().map(|g| g.lookups(s, Features::JOINING)),
                        face.gpos().map(|g| g.lookups(s, Features::JOINING)),
                        chosen.2,
                        chosen.3,
                    );
                    let i = match seen.iter().position(|k| *k == key) {
                        Some(i) => i,
                        None => {
                            seen.push(key);
                            seen.len() - 1
                        }
                    };
                    tables.push((chosen, i));
                    i
                }
            };
            let whole = (i, s.direction());
            let n = match numbered.iter().position(|k| *k == whole) {
                Some(n) => n,
                None => {
                    numbered.push(whole);
                    numbered.len() - 1
                }
            };
            Selection(u8::try_from(n).unwrap_or(u8::MAX))
        });
        let shaping = PerScript::from_fn(|s| {
            numbered
                .get(usize::from(by_script[s].0))
                .map_or(0, |&(i, _)| i)
        });
        let varies = match shaping.uniform() {
            true => Varies::No,
            false => Varies::Yes,
        };
        let alike = PerScript::from_fn(|s| shaping[s] == shaping[Script::COMMON]);
        Selections {
            by_script,
            alike,
            varies,
        }
    }

    /// The script a run in `s` is shaped in on this face, and its
    /// selection: Common's where `s` selects alike ([`Selections::alike`]).
    fn pick(&self, s: Script) -> (Script, Selection) {
        match self.alike[s] {
            true => (Script::COMMON, self.by_script[Script::COMMON]),
            false => (s, self.by_script[s]),
        }
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

/// A row's cells as [`Renderer::runs`] reads them: the cells in their
/// drawn order, where shaping must stop, and the joined and mirrored
/// cells, by visual column.
/// **Where one glyph of a laid word is drawn**: its syllable, its face
/// and glyph, its slot at the quarter pixel it falls on, its place from the
/// row's left edge in whole pixels, its pen to the quarter, and its rise in
/// pixels.
#[derive(PartialEq, Debug)]
struct GlyphPlace {
    syllable: crate::word::SyllableIx,
    face: FaceIx,
    id: GlyphId,
    slot: Option<crate::atlas::Slot>,
    whole: i32,
    pen: f32,
    dy: i32,
}

/// **A word laid and placed, for drawing in any light**
/// ([`Renderer::word_layout`]): how and at which instance it was placed,
/// its pen's start, each syllable's pen boundary and its ink, in pixels
/// from the row's left edge, and each of its glyphs' places
/// ([`Renderer::word_glyph_places`]). What a frame paints a word from
/// ([`Renderer::word_painted`]), and what [`crate::kept::KeptRows`] keeps
/// of it: no shaped run, which the run cache holds.
#[derive(PartialEq, Debug)]
pub(crate) struct WordLayout {
    laid: crate::word::Laid,
    x0: f32,
    bounds: Vec<f32>,
    ink: crate::fit::Ink,
    places: Vec<GlyphPlace>,
}

/// **A word laid** ([`Renderer::lay_word`]): how and at which instance it
/// was placed, its pieces shaped, the face each is shaped on and where it
/// starts, the face each glyph is drawn from, each glyph's syllable, its
/// pen's start and each syllable's pen boundary in pixels from the row's
/// left edge, and its measure there.
struct LaidWord {
    laid: crate::word::Laid,
    shapes: Vec<Shaped>,
    at: Vec<PieceAt>,
    drawn: Vec<Vec<FaceIx>>,
    owner: Vec<Vec<usize>>,
    x0: f32,
    bounds: Vec<f32>,
    measure: crate::word::Measure,
}

/// **Where a piece of a word starts** ([`Renderer::lay_word`]): the face it
/// is shaped on, and its pen's start from the word's -- `px` pixels where
/// its face's run of pieces starts, then `units` of that face's font units,
/// the pieces of one face going on from one another in font units as one
/// run does, so a word in one face is laid as a run.
#[derive(Clone, Copy, PartialEq, Debug)]
struct PieceAt {
    face: FaceIx,
    px: f32,
    units: i32,
}

impl PieceAt {
    /// The pen of glyph `g` of this piece, in pixels from the word's.
    fn pen(self, r: &Renderer<'_, '_>, g: &Placed) -> f32 {
        self.px + r.scales[self.face.index()].px((self.units + i32::from(g.dx)) as f32)
    }

    /// Where the piece after this one, `sh`, starts on the same face.
    fn after(self, sh: &Shaped) -> PieceAt {
        PieceAt {
            units: self.units + i32::from(sh.end()),
            ..self
        }
    }

    /// This start in pixels from the word's pen: where a piece on another
    /// face goes on from, and the word's advance after its last.
    fn edge(self, r: &Renderer<'_, '_>) -> f32 {
        self.px + r.scales[self.face.index()].px(self.units as f32)
    }
}

/// A row's Devanagari words, each one's syllables in their light, the
/// row's cells as they draw (`crate::word`), the syllables the core parted
/// on it (`crate::syllable`), and the row kept that holds its words laid
/// (`crate::kept`), for the runs.
type Worded<'w> = (
    &'w [crate::word::Word],
    &'w [crate::word::WordPaint],
    &'w [crate::word::Under],
    &'w [crate::syllable::Syllable],
    Option<crate::kept::KeptIx>,
);

type RowCells<'a> = (
    &'a [PackedCell],
    &'a [bool],
    &'a [Option<crate::bidi::Joined>],
    &'a [(Col, crate::bidi::Mirrored)],
    &'a [Col],
);

/// How a run's cell is drawn beyond its shaping: mirrored, the base it was
/// shaped from, and how far it leans left, an italic SAPV 17 slants.
#[derive(Clone, Copy)]
struct CellDrawn {
    mirrored: Option<crate::bidi::Mirrored>,
    base: Codepoint,
    lean: Option<crate::stack::Lean>,
}

/// **Where a glyph sits in its span**, in pixels: the span's width, a
/// cell's for each cell it takes, and the glyph's offset from its left
/// edge, before its slot's `left`.
#[derive(Clone, Copy)]
pub(crate) struct InSpan {
    pub width: i32,
    pub dx: i32,
}

impl InSpan {
    /// **The offset mirrored in the span**: the image, `dx` plus `slot`'s
    /// `left` from the left edge, put as far from the right edge, so a
    /// flipped image there is the unflipped one's pixels mirrored about
    /// the span's centre. Answered as an offset still to be added to
    /// `left`, as every path places a glyph.
    pub(crate) fn mirrored(self, slot: crate::atlas::Slot) -> i32 {
        self.width - self.dx - 2 * slot.left - i32::from(slot.w.0)
    }
}

/// **The character a cell draws**: its base ([`Term::base_of`]), and none
/// for a kitty Unicode placeholder, U+10EEEE, whose cell shows a slice of
/// an image or nothing (`[[source: kitty-graphics]]`, "Unicode
/// placeholders"): the protocol is the private agreement that gives this
/// private-use character its meaning, and it is never drawn as a glyph, nor
/// are its diacritics, as kitty's `font_for_cell` draws it with the blank
/// font (`[[source: kitty-fonts-c]]`). With no image to show, the cell
/// shows its background, rather than TUS 5.3's `.notdef`: the interim
/// choice and an open question. Every path that draws a glyph asks here; a
/// copy asks [`Term::base_of`] and gets the character back. **A combining
/// mark stored alone draws on a no-break space** ([`Content::drawn`], TUS
/// 5.13), so the mark sits in its own cell rather than over the one before.
pub(crate) fn drawn_base(term: &Term<'_>, cell: PackedCell) -> Option<Codepoint> {
    term.base_of(cell)
        .drawn()
        .as_codepoint()
        .filter(|c| !baud_domain::is_placeholder(*c))
}

/// A cell's base and the codepoints attached to it, as the shaper takes a
/// cluster: its marks read as [`drawn_marks`] reads them.
pub(crate) fn cell_cps(term: &Term<'_>, words: Words<'_>, cell: PackedCell) -> Vec<Codepoint> {
    let mut cps = Vec::new();
    cell_cps_into(term, words, cell, &mut cps);
    cps
}

/// [`cell_cps`], after what `out` holds.
fn cell_cps_into(term: &Term<'_>, words: Words<'_>, cell: PackedCell, out: &mut Vec<Codepoint>) {
    out.extend(drawn_base(term, cell));
    if cell.flags.is_marked() {
        out.extend(drawn_marks(term, words, cell).filter_map(Codepoint::new));
    }
}

/// **The text of word `w` of `row`** ([`crate::word::WordText`]): each
/// syllable's characters as its cells draw them, and how many each cell
/// holds.
pub(crate) fn word_text(
    term: &Term<'_>,
    words: Words<'_>,
    row: &[PackedCell],
    w: &crate::word::Word,
) -> crate::word::WordText {
    let mut held = vec![0usize; w.cells.index()];
    let parts = w
        .syllables
        .iter()
        .map(|s| {
            let mut cps = Vec::new();
            for c in s.at.index()..s.at.index() + s.cells.index() {
                let Some(&cell) = row.get(c).filter(|c| !c.content.is_wide_tail()) else {
                    continue;
                };
                let before = cps.len();
                cps.extend(cell_cps(term, words, cell));
                if let Some(h) = c.checked_sub(w.at.index()).and_then(|x| held.get_mut(x)) {
                    *h = cps.len() - before;
                }
            }
            cps
        })
        .collect();
    crate::word::WordText { parts, held }
}

/// **The marks a cell draws**: its cluster's, but the vowels SAPV 16
/// writes it without, "vowels are not presented" (`[[source: ecma48]]`
/// 8.3.108; [`baud_domain::is_vowel`]). The cell keeps them, and a copy
/// gives them. Its rendition is read where its id names it (`words`): a
/// frame's lent copy names a frozen row's word in a lent slot
/// (`crate::frozen`). A cell of a display line that joins rows is read by
/// its own row's ([`crate::frozen::Words::Runs`]) before it reaches here.
pub(crate) fn drawn_marks<'t>(
    term: &'t Term<'_>,
    words: Words<'_>,
    cell: PackedCell,
) -> impl Iterator<Item = u32> + 't {
    let hide = term.variants_seen()
        && words.word(term, Col::ZERO, cell).variants().vowels() == baud_domain::Vowels::Hidden;
    term.cluster_marks(cell)
        .filter(move |&m| !(hide && char::from_u32(m).is_some_and(baud_domain::is_vowel)))
}

/// Whether the cell at `at` of `row` carries a run of `shape` on: it has
/// the shape and its text draws -- a hidden cell draws nothing of its own
/// (`Resolved::text`), so a run's outline closes beside it. Past the row's
/// edge nothing does. The cell shader's `carries` is the same test.
pub(crate) fn carries_on(
    styles: &[Resolved],
    row: &[PackedCell],
    at: Option<Col>,
    shape: Shapes,
    blink: BlinkPhases,
) -> bool {
    at.and_then(|c| row.get(c.index()))
        .and_then(|n| styles.get(n.style.index()))
        .is_some_and(|n| n.shapes.has(shape) && n.text(blink).draws())
}

/// Which end of its encircled run (`SGR 52`) the cell at `col` of `row`
/// is: `None` for a cell that is not encircled or is between the ends.
pub(crate) fn ring_part(
    styles: &[Resolved],
    row: &[PackedCell],
    col: Col,
    blink: BlinkPhases,
) -> Option<crate::boxes::Ring> {
    use crate::boxes::Ring;
    let here = row.get(col.index())?;
    if !styles
        .get(here.style.index())?
        .shapes
        .has(Shapes::ENCIRCLED)
    {
        return None;
    }
    let c = col.index();
    let left = carries_on(
        styles,
        row,
        c.checked_sub(1).map(Col::at_index),
        Shapes::ENCIRCLED,
        blink,
    );
    let right = carries_on(
        styles,
        row,
        Some(Col::at_index(c + 1)),
        Shapes::ENCIRCLED,
        blink,
    );
    match (left, right) {
        (true, true) => None,
        (false, true) => Some(Ring::Starts),
        (true, false) => Some(Ring::Ends),
        (false, false) => Some(Ring::Alone),
    }
}

/// Whether any slot a renderer resolved has been encircled (`SGR 52`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rings {
    Unused,
    Used,
}

impl Rings {
    /// Used from the first encircled slot on.
    fn after(self, shapes: Shapes) -> Rings {
        if shapes.has(Shapes::ENCIRCLED) {
            Rings::Used
        } else {
            self
        }
    }
}

/// Where the cursor is, when it is shown, and its shape.
type Cursor = Option<(Row, Col, CursorStyle)>;

/// **Everything a sized line's records are converted from**
/// ([`Renderer::sized_lines`]) beside the renderer's own glyph caches,
/// which give the same glyph for the same question: the row's cells and
/// rendition, the cursor on it with its opacity, the selection over it, the
/// blink's phases, the video, the selection's colours, `OSC 12`'s cursor
/// colour, where the
/// rendition table stands, the cluster store's epoch, the cells of the row
/// the picture finder put in a picture, and how many times the atlas was
/// reset. Only a reset moves a slot a record names: a glyph placed
/// anywhere takes a free place and a growth keeps every slot at its texels
/// (`Atlas::grow_mask`), so the atlas's generation, which every placement
/// moves, would reconvert every sized row for a glyph on another line.
///
/// **Compared where the line stands** ([`SizedKey::holds`]): its cells,
/// its pictured cells and its composition are compared with the frame's
/// own, borrowed, and copied into the kept key's buffers only when they
/// differ ([`SizedKey::take`]), so a steady line costs a frame no buffer.
struct SizedKey {
    from: SizedFrom,
    pictured: Vec<crate::Pictured>,
    /// The composition on the line, which a line that joins draws among
    /// its own glyphs ([`Splice`]).
    composition: Option<Composition>,
    rest: SizedRest,
}

/// The rest of a [`SizedKey`], each part a value.
#[derive(Clone, Copy, PartialEq, Eq)]
struct SizedRest {
    size: LineRendition,
    cursor: Option<(Col, CursorStyle, Chan)>,
    look: CursorLook,
    selection: Option<Highlight>,
    blink: BlinkPhases,
    video: Video,
    hilite: Hilite,
    cursor_color: Option<(u8, u8, u8)>,
    cursor_paint: BlockPaint,
    styles: StyleStamp,
    clusters: ClusterEpoch,
    atlas: Tally,
}

/// **A frame's window as the cell path's build keyed it** (FS-C5): how far
/// it looks back, and the seal the build closed its writes under, read
/// with the window under one `&Term`, so a sized line's key can name its
/// row by where it is stored. Made only in this crate, by the frame's
/// build (`crate::cell_frame::build`): a caller that draws a window of
/// its own copy -- the last frame's, as a screen switch's slide draws it
/// (`Overlay::ensure`) -- has none, and its sized lines are compared cell
/// for cell.
#[derive(Clone, Copy)]
pub(crate) struct Framed {
    back: Rows,
    seal: Seal,
}

impl Framed {
    /// The window `back` lines into the history of the term a build read,
    /// under `seal`, the term's as it read it.
    pub(crate) fn of(back: Rows, seal: Seal) -> Framed {
        Framed { back, seal }
    }
}

/// **What a sized line's key holds its cells by** (FS-C5).
enum SizedFrom {
    /// The row as stored: where it is stored and the seal the frame's
    /// window was keyed under. It holds while the same stored row is drawn
    /// at the line's row and was not written after that seal
    /// (`RowGens::after`), and reads no cell.
    Stored { at: RowAt, seal: Seal },
    /// The cells as drawn, compared cell for cell: a row the layout drew
    /// in another order, which reads the terminal beyond its cells, every
    /// row of a display it re-broke, and every row of a window no build
    /// keyed ([`Framed`]).
    Cells(Vec<PackedCell>),
}

/// A sized line's cells as the frame has them: where the row is stored,
/// with the window's generations and seal, or the cells as drawn.
enum SizedCells<'a, 'g> {
    Stored {
        at: RowAt,
        seal: Seal,
        gens: &'a RowGens<'g>,
        y: Row,
    },
    Drawn(&'a [PackedCell]),
}

impl SizedFrom {
    /// Whether the cells this key was taken from are `now`'s: the same
    /// stored row, written since the key's seal or not, or the same cells,
    /// compared. A line that moves between the two misses once.
    fn holds(&self, now: &SizedCells<'_, '_>) -> bool {
        match (self, now) {
            (
                SizedFrom::Stored { at, seal },
                SizedCells::Stored {
                    at: here, gens, y, ..
                },
            ) => at == here && !gens.after(*y, seal),
            (SizedFrom::Cells(kept), SizedCells::Drawn(cells)) => {
                crate::counters::same_cells(Site::Sized, kept, cells)
            }
            _ => false,
        }
    }

    /// The key `now` makes, new.
    fn of(now: &SizedCells<'_, '_>) -> SizedFrom {
        match now {
            SizedCells::Stored { at, seal, .. } => SizedFrom::Stored {
                at: *at,
                seal: *seal,
            },
            SizedCells::Drawn(cells) => {
                crate::counters::add(Counter::CellsCopied, Site::Sized, Tally::of(cells.len()));
                SizedFrom::Cells(cells.to_vec())
            }
        }
    }

    /// The key `now` makes, into this key's buffer where both are cells.
    fn take(&mut self, now: &SizedCells<'_, '_>) {
        match (self, now) {
            (SizedFrom::Cells(kept), SizedCells::Drawn(cells)) => {
                kept.clear();
                kept.extend_from_slice(cells);
                crate::counters::add(Counter::CellsCopied, Site::Sized, Tally::of(cells.len()));
            }
            (me, now) => *me = SizedFrom::of(now),
        }
    }
}

/// A sized line's parts as the frame has them, borrowed.
struct SizedNow<'a, 'g> {
    cells: SizedCells<'a, 'g>,
    pictured: &'a [crate::Pictured],
    composition: Option<&'a Composition>,
    rest: SizedRest,
}

impl SizedKey {
    /// Whether the line converted under this key is the line `now` is:
    /// every part compared, its cells by what the key holds them by.
    fn holds(&self, now: &SizedNow<'_, '_>) -> bool {
        self.rest == now.rest
            && self.from.holds(&now.cells)
            && self.pictured[..] == now.pictured[..]
            && self.composition.as_ref() == now.composition
    }

    /// The key `now` makes, into this key's buffers.
    fn take(&mut self, now: &SizedNow<'_, '_>) {
        self.from.take(&now.cells);
        self.pictured.clear();
        self.pictured.extend_from_slice(now.pictured);
        match (&mut self.composition, now.composition) {
            (Some(kept), Some(c)) => kept.clone_from(c),
            (kept, c) => *kept = c.cloned(),
        }
        self.rest = now.rest;
    }

    /// The key `now` makes, new.
    fn of(now: &SizedNow<'_, '_>) -> SizedKey {
        SizedKey {
            from: SizedFrom::of(&now.cells),
            pictured: now.pictured.to_vec(),
            composition: now.composition.cloned(),
            rest: now.rest,
        }
    }
}

/// **A composition drawn in its row's order**, on a row the layout
/// joined: its pages first, then each cluster's glyphs at the visual
/// column it is drawn at, among the row's own, as the same text written
/// is drawn -- the instance path draws a row's glyphs left to right, and
/// where a joined letter's ink meets its neighbour's, the later one lies
/// over it. Taken from [`crate::overlay::Composed::splice`].
pub(crate) struct Splice {
    pub(crate) row: Row,
    pub(crate) pages: Vec<Instance>,
    pub(crate) glyphs: Vec<(usize, Vec<Instance>)>,
}

/// One sized line's records and what they were converted from.
struct SizedRow {
    key: SizedKey,
    records: Vec<Instance>,
}

/// The pixel rows a row's glyphs cover, `top..bottom`: past the row's
/// band where a glyph reaches out of its cell, up or down, as a stacked
/// accent does. The CPU compositor draws those glyphs into the bands they
/// reach, in the instance path's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Reach {
    top: i32,
    bottom: i32,
}

impl Reach {
    /// A row with no glyph.
    const NONE: Reach = Reach {
        top: i32::MAX,
        bottom: i32::MIN,
    };

    /// The rows `glyphs` cover.
    fn of(glyphs: &[Instance]) -> Reach {
        glyphs.iter().fold(Reach::NONE, |r, g| {
            let top = i32::from(g.y.0);
            Reach {
                top: r.top.min(top),
                bottom: r.bottom.max(top + i32::from(g.h.0)),
            }
        })
    }

    /// Whether it covers a pixel row of `band`.
    fn meets(self, band: Band) -> bool {
        let (top, bottom) = (px_i32(band.top), px_i32(band.bottom));
        self.top < bottom && self.bottom > top
    }

    /// Whether it covers a pixel row outside `band`.
    fn leaves(self, band: Band) -> bool {
        self.top < self.bottom && (self.top < px_i32(band.top) || self.bottom > px_i32(band.bottom))
    }
}

/// Whether the previous frame's copy describes what is on screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
    /// Nothing painted yet, or the geometry changed: the next frame is full.
    Unknown,
    Known,
}

/// **What a frame does with a row of this path's copy of the window**
/// (`Renderer::convert`): leaves it as the last frame left it, compares it
/// with the last frame's as it stands -- a row veiled or lent, now or last
/// -- or takes it from the terminal again and compares it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MirrorRow {
    Kept,
    Compared,
    Filled,
}

/// **Which rows of this path's copy a frame lent** (`Renderer::lend_cpu`):
/// none; only the rows whose struck cells it rewrote, each the terminal's
/// again at the next frame's start ([`Mirror::unlend`]); or every row, for
/// a window that shows a frozen row, whose lending moves what an
/// unchanged cell draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LentRows {
    None,
    /// The rows whose struck cells it rewrote: the one lending under which
    /// a row it rewrote, this frame or the last, may be kept unread, where
    /// it lent them as the last frame did (`Renderer::convert`).
    Struck,
    /// Every row, each taken from the terminal again and compared, this
    /// frame and the next.
    Every,
}

use mirror::Mirror;

/// **The CPU path's copy of the window, and the key it was taken under**
/// (FS-C5): its cells, each row's line rendition, and the seals, window
/// and display a convert last brought it up to date under, so the next
/// convert takes from the terminal only the rows written or laid out again
/// since.
///
/// At the end of every convert the copy equals the last frame's, `prev`,
/// row for row, so a row the key keeps is neither read nor compared. That
/// holds only while the convert is the copy's one writer, so the fields
/// are this module's: every other writer -- the cell path's sized lines,
/// its status line and a smooth scroll's leaving row, which convert rows
/// of their own on these buffers -- writes through [`Mirror::scribble`],
/// which forgets the key first, and a resize forgets it too. **A copy so
/// written is read as no key at all** (`Keeps::Nothing`): the next
/// convert takes every row and compares it, as a full frame does.
mod mirror {
    use crate::Orders;
    use crate::screens::{Dirt, Keeps, Now, ScreenGen, SetKey};
    use baud_core::Term;
    use baud_domain::{Cols, LineRendition, PackedCell, Row, Rows, Seal, StyleId, Viewport};

    #[derive(Default)]
    pub(super) struct Mirror {
        cells: Vec<PackedCell>,
        sizes: Vec<LineRendition>,
        key: SetKey,
        /// **Each cell the convert's lending rewrote**, by its place in
        /// `cells`, with the id it held, in the order rewritten: what puts
        /// the copy back as the terminal's before the next convert reads
        /// it ([`Mirror::unlend`]). Kept here, not with the lending, so a
        /// give-back outside the convert cannot lose it.
        lent: Vec<(usize, StyleId)>,
    }

    /// The cells, read as they stand.
    impl core::ops::Deref for Mirror {
        type Target = [PackedCell];

        fn deref(&self) -> &[PackedCell] {
            &self.cells
        }
    }

    impl Mirror {
        /// Blank cells, `cols` a row over `rows` single-width rows, under no
        /// key.
        pub(super) fn resize(&mut self, cols: Cols, rows: Rows) {
            self.cells = vec![PackedCell::EMPTY; cols.index() * rows.index()];
            self.sizes = vec![LineRendition::Single; rows.index()];
            self.key.forget();
            self.lent.clear();
        }

        /// **The cells the convert's lending rewrote**, `rewrites` by row of
        /// the copy, column and the id each held, `cols` cells a row
        /// (`crate::Lent::rewrites`): kept for [`Mirror::unlend`]. Copied
        /// on every convert, one entry a rewritten cell -- as many as the
        /// struck cells, or every frozen cell lent -- into a buffer whose
        /// room is kept.
        pub(super) fn lend(&mut self, rewrites: &[(usize, usize, StyleId)], cols: Cols) {
            self.lent.clear();
            self.lent.extend(
                rewrites
                    .iter()
                    .map(|&(y, x, was)| (y * cols.index() + x, was)),
            );
        }

        /// **The copy put back as the terminal's** where the last convert's
        /// lending rewrote it, the last rewrite first, before this convert
        /// reads it: each row it touched, `cols` cells a row, into `rows`.
        /// A copy written since by anything but the convert holds no
        /// record ([`Mirror::scribble`]), and is taken whole.
        pub(super) fn unlend(&mut self, cols: Cols, rows: &mut Vec<usize>) {
            rows.clear();
            for &(i, was) in self.lent.iter().rev() {
                if let Some(c) = self.cells.get_mut(i) {
                    c.style = was;
                }
                // A lending rewrites row after row, so a row's cells come
                // together: the list is asked only where the row changes.
                let y = i / cols.index().max(1);
                if rows.last() != Some(&y) && !rows.contains(&y) {
                    rows.push(y);
                }
            }
            self.lent.clear();
        }

        /// Each row's line rendition, as it stands.
        pub(super) fn sizes(&self) -> &[LineRendition] {
            &self.sizes
        }

        /// **The copy written by anything but the convert**: the key
        /// forgotten first, so the next convert takes every row.
        pub(super) fn scribble(&mut self) -> (&mut [PackedCell], &mut [LineRendition]) {
            self.key.forget();
            self.lent.clear();
            (&mut self.cells, &mut self.sizes)
        }

        /// **The copy written by the convert** (`Renderer::convert_with`),
        /// the key kept: the rows it fills from the terminal, and those it
        /// veils, composes over, notices or lends, each of which it then
        /// compares, copies into `prev` and takes again the next frame.
        pub(super) fn converting(&mut self) -> (&mut [PackedCell], &mut [LineRendition]) {
            (&mut self.cells, &mut self.sizes)
        }

        /// How much of the copy the key keeps for `now`, drawn through
        /// `orders` (`SetKey::keeps`): nothing once scribbled or resized.
        pub(super) fn keeps(&self, term: &Term<'_>, now: &Now<'_>, orders: &Orders) -> Keeps {
            self.key.keeps(term, now, orders)
        }

        /// Whether row `r` may not be the window's now: written or laid out
        /// again since the key's seals (`SetKey::dirt`).
        pub(super) fn dirty(
            &self,
            now: &Now<'_>,
            orders: &Orders,
            view: &Viewport<'_>,
            r: Row,
        ) -> bool {
            self.key.dirt(now, orders, view, r) != Dirt::Kept
        }

        /// **The convert done**, the copy equal to the frame drawn: keyed
        /// under `seal`, the term's as it was read, and the seal of
        /// `orders`, the maps it was drawn through.
        pub(super) fn settle(
            &mut self,
            seal: Seal,
            now: &Now<'_>,
            view: &Viewport<'_>,
            orders: &Orders,
        ) {
            self.key
                .record((seal, ScreenGen::default()), now, view, orders);
        }
    }
}

/// Where a picture's anchor row is: a row of the window drawn, or that
/// many lines above its first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Anchor {
    At(Row),
    Above(Rows),
}

/// Whether a convert repaints the rows the frame's image placements cover.
/// The CPU path blits images over the rows it painted, so it needs them
/// repainted; a GPU path draws images in their own pass and does not.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ImageRows {
    Ignored,
    Repainted,
}

/// Whether a layout reads the predicted echoes as written: the cell
/// path's, which draws them, or the instance and CPU paths', which do not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Predicted {
    Read,
    Unread,
}

/// Nothing written over any row, for a layout with no [`Beside`].
static NOTHING_WRITTEN: crate::bidi::Written = crate::bidi::Written::NOTHING;

/// Whether a layout drew any row in another order, or a display: what the
/// find marks and the link underline follow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reordered {
    No,
    Yes,
}

/// **What the find marks and the link underline are drawn from past the
/// layout's order**: the cell and line metrics their bands and lines are
/// placed by, the window's height they are clipped to, and the colours
/// they are drawn in.
#[derive(Clone, Copy, PartialEq)]
struct MarksKey {
    metrics: Metrics,
    rows: Rows,
    defaults: (Argb, Argb),
    wash: Argb,
}

/// **What a renderer's layout reads besides the window's cells**: the
/// composition and, on the cell path, the predicted echoes, written over
/// the rows they are on (`bidi::Written`), and the echoes the cell path
/// draws this frame ([`Renderer::set_predicted`]); and the row a smooth
/// scroll's slide draws above the window, laid out over the window one
/// line up, as it was drawn when it was that window's first
/// ([`Renderer::copy_leaving`]), with its cells in that order. That
/// layout is made on the first slide: it holds the algorithm's working
/// state for a paragraph of [`crate::CONTEXT`] codepoints.
#[derive(Default)]
struct Beside {
    written: crate::bidi::Written,
    predicted: Vec<(Row, Col, Codepoint)>,
    leaving: Option<crate::bidi::Reorder>,
    leaving_cells: Vec<PackedCell>,
}

/// What [`Renderer::paint_rows`] draws between a repainted row's background
/// and its glyphs.
trait Under {
    /// Paints, returning how many placements it examined for the row.
    fn paint(&self, r: &Renderer<'_, '_>, target: &mut Target<'_>, row: Row, page: Page) -> Tally;
}

/// **How much of a dirty row a frame repaints**: the whole of it, or,
/// when only the trail's or the splash's records changed over it, the
/// columns from the leftmost pixel one of them reaches on the row, in this
/// frame or the last, to the rightmost. The rest of the row is the last
/// frame's: no record was over it in either, and nothing else on it
/// changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Repaint {
    Whole,
    Cols { left: Px, right: Px },
}

/// **A dirty row's pixels as this frame repaints them**: the target held
/// to the columns the row repaints ([`Repaint`]), the row, and its band.
/// **Made by [`Renderer::repainted`] alone, by type**: its fields are this
/// module's, and `repainted` is the one maker in it. **An image over the
/// text is blitted only through it** ([`RowTarget::blit`]), into the band
/// and the columns the row's own pass has just drawn again, so a placement
/// never lands over its own last blit -- which a partly transparent one
/// would blend twice -- and a row nothing repaints keeps the last frame's
/// pixels, image and overlays alike. `Target::blit_image` is the crate's
/// own, and is reached from here and from [`Renderer::paint_under`] alone
/// (`baud-render/tests/image.rs` reads every crate's sources for it).
mod row_target {
    use super::{Band, ImagePlacement, Page, Renderer, Repaint, Row};
    use crate::composite::Target;
    use baud_domain::ImageView;
    use baud_font::Px;

    pub(super) struct RowTarget<'t> {
        target: Target<'t>,
        row: Row,
        band: Band,
    }

    impl<'t> core::ops::Deref for RowTarget<'t> {
        type Target = Target<'t>;

        fn deref(&self) -> &Target<'t> {
            &self.target
        }
    }

    impl core::ops::DerefMut for RowTarget<'_> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.target
        }
    }

    impl RowTarget<'_> {
        /// The row repainted.
        pub(super) fn row(&self) -> Row {
            self.row
        }

        /// The row's band.
        pub(super) fn band(&self) -> Band {
            self.band
        }

        /// Blits placement `p` of `view` into the row: its band, and the
        /// columns it repaints.
        pub(super) fn blit(
            &mut self,
            r: &Renderer<'_, '_>,
            p: &ImagePlacement,
            view: &ImageView<'_>,
            page: Page,
        ) {
            self.target.blit_image(
                &r.blit_of(p, self.band),
                view,
                (p.ground, p.composite),
                r.blend,
                page,
            );
        }
    }

    impl Renderer<'_, '_> {
        /// `target` over what dirty row `row` repaints ([`Repaint`]), with
        /// the row and its band: the one place a [`RowTarget`] is made.
        pub(super) fn repainted<'t>(&self, target: &'t mut Target<'_>, row: Row) -> RowTarget<'t> {
            let target = match self.repaint.get(row.index()) {
                Some(&Repaint::Cols { left, right }) => target.within(left, right),
                _ => target.within(Px::of(0), Px::of(u32::MAX)),
            };
            RowTarget {
                target,
                row,
                band: self.band(row),
            }
        }
    }
}

use row_target::RowTarget;

/// **The pixels placement `p`'s image holds now, if the store still holds
/// that image**: `None` once it is freed or its slot holds another image
/// ([`ImageSince::Gone`], which the placement's `born` tells). A frame
/// places its images first ([`Renderer::place_images`]) from the store it
/// then paints from, so on a frame this is always the image. The kept
/// frame ([`Renderer::render_hidden`]) places nothing, and there an image
/// gone is kept as the last frame drew it, and one whose pixels moved is
/// drawn in its new pixels on every row it covers ([`since_placed`]).
fn held<'s>(imgs: &'s ImageStore<'_>, p: &ImagePlacement) -> Option<baud_domain::ImageView<'s>> {
    match imgs.since(p.id, p.born, p.epoch) {
        ImageSince::Gone => None,
        ImageSince::Redrawn | ImageSince::Same => imgs.image(p.id),
    }
}

/// **What became of placement `p`'s image since the frame that placed
/// it**, asked of that image and not of the store's epoch, which every
/// other image's store, edit and free moves too ([`ImageStore::since`]):
/// gone, redrawn -- a pixel of it written since, or another of its frames
/// shown, which the animation clock moves without a write -- or the same.
fn since_placed(imgs: &ImageStore<'_>, p: &ImagePlacement) -> ImageSince {
    match imgs.since(p.id, p.born, p.epoch) {
        ImageSince::Same if imgs.image(p.id).is_some_and(|v| v.frame != p.frame) => {
            ImageSince::Redrawn
        }
        since => since,
    }
}

/// Whether any of `records` meets the pixel rows of `band`.
fn records_meet(records: &[Instance], band: Band) -> bool {
    records.iter().any(|r| record_meets(r, band))
}

/// Whether `r`'s pixel rows meet `band`'s.
fn record_meets(r: &Instance, band: Band) -> bool {
    let (top, h) = (i64::from(r.y.0), i64::from(r.h.0));
    h > 0 && top < i64::from(band.bottom.value()) && top + h > i64::from(band.top.value())
}

/// Nothing: no placement is under the text.
struct NoUnder;

impl Under for NoUnder {
    #[inline(always)]
    fn paint(&self, _: &Renderer<'_, '_>, _: &mut Target<'_>, _: Row, _: Page) -> Tally {
        Tally::of(0)
    }
}

/// The placements under the text, from the term's images.
struct UnderImages<'s, 'b>(&'s ImageStore<'b>);

impl Under for UnderImages<'_, '_> {
    fn paint(&self, r: &Renderer<'_, '_>, target: &mut Target<'_>, row: Row, page: Page) -> Tally {
        r.paint_under(self.0, target, row, page)
    }
}

/// What pixel `p` of an image with a background fill draws as.
///
/// A fill word draws as `ground`, and so does a shared Sixel's pixel drawn
/// in a clear ink ([`Pixel::UNSAID`]), which was the fill there before the
/// store told it from one; every other word as it is.
pub(crate) fn filled(p: Pixel, ground: Pixel) -> Pixel {
    if p == Pixel::FILL || p == Pixel::UNSAID {
        ground
    } else {
        p
    }
}

/// Whether a placement's pixel rows meet the band's.
fn covers(p: &ImagePlacement, band: Band) -> bool {
    let top = i64::from(p.y);
    top < i64::from(band.bottom.value()) && top + i64::from(p.h) > i64::from(band.top.value())
}

/// Whether the last CPU frame drew a placement over the band that this
/// frame does not: an image left or moved from there, or another took its
/// rect. Whole placements are compared -- image, epoch and rect -- so a
/// different image in the same rect still counts. `gone` is
/// [`PlaceDiff::gone`], the last frame's placements this one does not
/// hold.
fn left_from(last: &[ImagePlacement], gone: &[usize], band: Band) -> bool {
    gone.iter().any(|&j| covers(&last[j], band))
}

/// Whether a placement arrived on the band, or changed there, since the
/// last CPU frame, under the text or over it: every placement is blitted
/// only into a row being repainted ([`RowTarget`]), so a row one arrives
/// on is repainted for it. A new image, a moved one, one drawn in other
/// pixels (a new store `epoch`) and an animation's next frame (a new
/// `frame`) each arrive, since [`ImagePlacement`]'s key holds them.
/// `arrived` is [`PlaceDiff::arrived`].
fn arrived_on(now: &[ImagePlacement], arrived: &[usize], band: Band) -> bool {
    arrived.iter().any(|&i| covers(&now[i], band))
}

/// **The frame's placements by the screen rows they cover**: row `r`'s
/// are `items[start[r]..start[r + 1]]`, indices into the frame's list in
/// its draw order, each a placement whose pixel rows meet the row's band
/// ([`covers`]). Built by a count and a fill, on the CPU path only and
/// only when enough rows ask ([`INDEX_ROWS`]), so a repainted row reads
/// its own placements and not every one: a picture placed one cell at a
/// time over a 400x150 screen is 60,000 placements, and each row asked
/// about all of them twice made a frame rows times placements.
#[derive(Default)]
struct PlaceRows {
    start: Vec<usize>,
    items: Vec<usize>,
    /// Scratch: each placement's rows, then each row's next free item.
    spans: Vec<(usize, usize)>,
    fill: Vec<usize>,
    built: Built,
}

/// Whether [`PlaceRows`] indexes this frame's placements.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Built {
    #[default]
    Stale,
    Fresh,
}

/// **The rows that must ask before the placements are indexed**: a build
/// costs about what three rows' walks of every placement cost -- 4.5 M
/// instructions against 1.4 M a row for 59,600 placements, its count and
/// fill against the two passes [`Renderer::paint_under`] makes a row,
/// measured with `frame`'s `FRAME_PERCELL` -- so a frame repainting
/// fewer than four rows walks, and one repainting more indexes.
const INDEX_ROWS: usize = 4;

impl PlaceRows {
    /// Index `places` over `rows` rows of `cell_h` pixels, unless this
    /// frame's are indexed already.
    fn ensure(&mut self, places: &[ImagePlacement], rows: Rows, cell_h: Px) {
        if self.built == Built::Fresh {
            return;
        }
        self.built = Built::Fresh;
        let n = rows.index();
        self.start.clear();
        self.start.resize(n + 1, 0);
        self.spans.clear();
        let ch = i64::from(cell_h.value().max(1));
        let last = i64::try_from(n).unwrap_or(0) - 1;
        // The rows `r` with `r * ch < y + h` and `(r + 1) * ch > y`,
        // [`covers`] solved for `r`, on the screen; empty as `(1, 0)`.
        for p in places {
            let top = i64::from(p.y);
            let first = top.div_euclid(ch).max(0);
            let end = ((top + i64::from(p.h) + ch - 1).div_euclid(ch) - 1).min(last);
            let span = match first <= end {
                true => (
                    usize::try_from(first).unwrap_or(0),
                    usize::try_from(end).unwrap_or(0),
                ),
                false => (1, 0),
            };
            for r in span.0..=span.1 {
                self.start[r + 1] += 1;
            }
            self.spans.push(span);
        }
        for r in 0..n {
            self.start[r + 1] += self.start[r];
        }
        self.items.clear();
        self.items.resize(self.start[n], 0);
        self.fill.clear();
        self.fill.extend_from_slice(&self.start[..n]);
        for (i, &(first, end)) in self.spans.iter().enumerate() {
            for r in first..=end {
                self.items[self.fill[r]] = i;
                self.fill[r] += 1;
            }
        }
    }

    /// The placements on `row`, in draw order, if this frame's are
    /// indexed: none past the rows built.
    fn row(&self, row: Row) -> Option<&[usize]> {
        if self.built == Built::Stale {
            return None;
        }
        let r = row.index();
        match (self.start.get(r), self.start.get(r + 1)) {
            (Some(&a), Some(&b)) => Some(&self.items[a..b]),
            _ => Some(&[]),
        }
    }
}

/// **Which placements differ between two CPU frames**, found once a frame
/// by sorting rather than by searching one list for each of the other's:
/// a per-cell picture is a placement a cell, 660 for yazi's preview and
/// 10,000 for a 200x50 screen, and a search a placement a row made the
/// frame's diff quadratic in them. Each list is a set, as `contains`
/// read it: a placement held twice is held.
#[derive(Default)]
struct PlaceDiff {
    /// The last frame's placements this frame does not hold, by index.
    gone: Vec<usize>,
    /// This frame's placements the last did not hold, by index.
    arrived: Vec<usize>,
    /// Scratch: each list's indices in key order.
    now_order: Vec<usize>,
    last_order: Vec<usize>,
}

impl PlaceDiff {
    /// Fill both lists from this frame's placements and the last's. Two
    /// equal lists, a still picture's every frame, are one comparison.
    fn diff(&mut self, now: &[ImagePlacement], last: &[ImagePlacement]) {
        self.gone.clear();
        self.arrived.clear();
        if now == last {
            return;
        }
        let order = |v: &[ImagePlacement], o: &mut Vec<usize>| {
            o.clear();
            o.extend(0..v.len());
            o.sort_unstable_by_key(|&i| v[i].key());
        };
        order(now, &mut self.now_order);
        order(last, &mut self.last_order);
        missing(
            now,
            &self.now_order,
            last,
            &self.last_order,
            &mut self.arrived,
        );
        missing(last, &self.last_order, now, &self.now_order, &mut self.gone);
    }
}

/// The indices of `a`'s placements that `b` does not hold, both walked in
/// key order once.
fn missing(
    a: &[ImagePlacement],
    a_order: &[usize],
    b: &[ImagePlacement],
    b_order: &[usize],
    out: &mut Vec<usize>,
) {
    let mut j = 0;
    for &i in a_order {
        let k = a[i].key();
        while j < b_order.len() && b[b_order[j]].key() < k {
            j += 1;
        }
        if j == b_order.len() || b[b_order[j]].key() != k {
            out.push(i);
        }
    }
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
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ImageScale {
    Unit { x: ImagePx, y: ImagePx },
    Mapped,
}

/// Where a placement's tables start in [`Renderer::image_map`]'s store:
/// its `w` columns' texel x, then its `h` rows' texel y.
#[derive(Clone, Copy, Debug)]
pub struct MapAt(usize);

/// **How a placement's pixels meet what is under them**: source-over, every
/// image's, or replacing it, a Sixel's whose background fill draws as
/// [`crate::SixelBg::Cleared`] on the glass. A replacing image holds only
/// opaque words and its fill (its data's colours are opaque, and every
/// position it leaves at zero is a fill word), so replacing is source-over
/// for its data and leaves the fill's own alpha where it lands. The CPU
/// stores those words; the GPU draws it with blending off.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ImageComposite {
    Over,
    Replace,
}

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
    /// Which frame of a kitty animation it shows -- the cache key with
    /// `id` and `epoch`: a frame shown in place of another is other
    /// pixels at the same epoch.
    pub frame: FrameNo,
    /// The screen x of the top-left, device pixels, signed.
    pub x: i16,
    /// The screen y of the top-left, device pixels, signed.
    pub y: i16,
    /// The image width in device pixels.
    pub w: u16,
    /// The image height in device pixels.
    pub h: u16,
    /// Whether any of its pixels is partly transparent: the CPU path then
    /// blends it, and blits it only into the rows it repaints, never over
    /// its own last blit.
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
    /// **When its image was stored** ([`ImageAge`]), which orders two
    /// placements that tie on `z` and on `tie`, where the text leaves the
    /// order undefined ("If the images have the same z-index and the same
    /// id, then the behavior is undefined", `[[source: kitty-graphics]]`):
    /// the image stored later over the one stored before. kitty orders
    /// every tie on `z` this way, by `image_id`, the internal id it gives
    /// each image in the order they are made (`[[source:
    /// kitty-graphics-c]]`, `lt`); baud follows the text's "lower id" first,
    /// so ids 5 then 3 draw 5 over 3 where kitty draws 3 over 5, and orders
    /// by this only where the ids tie -- among images with no id, and
    /// placements of one image.
    pub born: ImageAge,
    /// **When the placement was made** ([`PlaceSerial`]), which orders two
    /// placements of one image, the later over the earlier, as kitty's
    /// sort orders them last by `ref_id`. Neither is where the placement
    /// sits in the store's table: a placement made after another is freed
    /// takes the lowest free slot, so slot order put a frame mpv sent after
    /// a deletion under the one before it.
    pub serial: PlaceSerial,
    /// What it draws: the source rectangle, and the drawn one from its
    /// anchor cell, of which `x`, `y`, `w` and `h` are the visible part.
    pub draw: ImageDraw,
    /// One image pixel a device pixel, or through its tables.
    pub scale: ImageScale,
    /// Where its tables are.
    pub map: MapAt,
    /// What its [`Pixel::FILL`] words draw as this frame
    /// ([`Renderer::fill_pixel`]): a Sixel's background fill, and
    /// [`Pixel::CLEAR`] for every image without one.
    pub ground: Pixel,
    /// Over what is under it, or replacing it.
    pub composite: ImageComposite,
}

/// Two placements are one when they draw the same and stack the same:
/// everything but where this frame's tables happen to sit. The stacking
/// keys, `born` and `serial`, are in it, so a placement deleted and put
/// again in one frame, which draws the same and stacks higher, is one the
/// CPU path's diff finds changed.
impl PartialEq for ImagePlacement {
    fn eq(&self, o: &ImagePlacement) -> bool {
        self.key() == o.key()
    }
}

impl Eq for ImagePlacement {}

impl ImagePlacement {
    /// What [`PartialEq`] compares, in an order: two placements are equal
    /// exactly when their keys are, so two lists are matched by sorting.
    #[allow(clippy::type_complexity)]
    fn key(
        &self,
    ) -> (
        ImageId,
        (ImageGen, FrameNo),
        i16,
        i16,
        u16,
        u16,
        ImageAlpha,
        ZIndex,
        (KittyId, ImageAge, PlaceSerial),
        ImageDraw,
        ImageScale,
        (u32, ImageComposite),
    ) {
        (
            self.id,
            (self.epoch, self.frame),
            self.x,
            self.y,
            self.w,
            self.h,
            self.alpha,
            self.z,
            (self.tie, self.born, self.serial),
            self.draw,
            self.scale,
            (self.ground.bits(), self.composite),
        )
    }
}

/// One image's pixels to upload once: keyed by `id` and `epoch`, the store's
/// straight `0xAARRGGBB` words, `w * h` of them, row-major, unconverted. A
/// clear (unpainted Sixel) pixel is `0x00000000`. **Straight, not
/// premultiplied**: the fragment that samples the texture premultiplies
/// (`glyph_fragment`'s colour branch, `c.rgb * c.a`), the one premultiply on
/// the GPU paths, as `blend.over` is the CPU's.
pub struct ImageUpload {
    /// Which stored image these pixels are.
    pub id: ImageId,
    /// The store epoch -- the cache key with `id`, `frame` and `ground`.
    pub epoch: ImageGen,
    /// Which frame of a kitty animation these pixels are.
    pub frame: FrameNo,
    /// What its [`Pixel::FILL`] words were drawn as in `pixels`, so a
    /// change of the default background uploads the image again.
    pub ground: Pixel,
    /// The image width in pixels.
    pub w: u16,
    /// The image height in pixels.
    pub h: u16,
    /// The straight pixels, `w * h`, row-major.
    pub pixels: Vec<u32>,
}

/// **The order a copy of the window holds each row's cells in**
/// ([`Renderer::take_struck`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyOrder {
    /// As drawn, left to right: the CPU and instance paths' row buffer.
    Visual,
    /// As the core stores them: the cell path's copy, which the shader
    /// reorders through the row's map.
    Stored,
}

pub struct Renderer<'f, 'a> {
    stack: Stack<'f, 'a>,
    /// Which face draws each codepoint, per style and presentation
    /// (`Stack::table`): built here once, so `face_for` is a lookup.
    faces_of: FaceTable,
    /// Which face draws each marked cluster (`Stack::select_cluster`),
    /// remembered, so `face_for` stays a lookup for a cell with marks too.
    cluster_faces: ClusterMemo<FaceIx>,
    /// Which face draws a codepoint in an alternative font or under
    /// Fraktur (`Stack::select_font`), by codepoint and by the style, font
    /// and presentation's code, remembered up to `FONT_FACES_MAX`: empty on
    /// a stack with no font faces, whose cells never ask.
    font_faces: crate::seeded::Map<(u32, u32), FaceIx>,
    /// Whether a face draws all of a marked cluster, remembered, so a run
    /// the cache misses asks `shape` a lookup per marked cell too.
    cluster_covers: ClusterMemo<Cover>,
    scales: Vec<Scale>,
    /// Per face, where its layout tables are below: its own, or a narrower
    /// instance's base's ([`Layout`]).
    layouts: Vec<Layout>,
    /// Per face, which faces number their glyphs as it does
    /// ([`GlyphOrder`]).
    orders: Vec<GlyphOrder>,
    /// Per layout, a digest per `GSUB` lookup the grid features name, so a
    /// cache miss opens only the lookups that can touch the run.
    digests: ByLayout<Vec<LookupDigest>>,
    /// Per layout, the prefilter the counts in [`Exceptions`] ask.
    #[cfg(feature = "exceptions")]
    starters: ByLayout<Starter>,
    /// Per layout, the glyph pairs its rules can begin on.
    pairs: ByLayout<Pairs>,
    /// Per layout, the longest sequence a rule can match, in glyphs; `None`
    /// for a face with no substitution rules or one whose tables could
    /// not be walked.
    extents: ByLayout<Option<Count>>,
    /// Per layout, how many cells of its word either side a joined cell's
    /// shaping reads ([`baud_font::joined_reach`]), at most
    /// [`WORD_REACH_MAX`].
    reaches: ByLayout<Count>,
    /// Per layout, which lookups each script selects
    /// (`[[source: ms-opentype-chapter2]]`, ScriptList table), so a run is
    /// cut where they change.
    selections: ByLayout<Selections>,
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
    singles: crate::seeded::Map<u32, Option<(FaceIx, Placed)>>,
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
    /// **The alpha a reversed default page is drawn at**
    /// ([`Theme::reversed_alpha`]), with the default pair as stored that
    /// it was found for: found again only when the theme or that pair
    /// moves, so no frame and no cell pays for the search.
    reversed: (Chan, (Argb, Argb)),
    blend: Blend,
    cache: RunCache,
    atlas: Atlas,
    /// How a character with no selector chooses between a text face and a
    /// colour one; the embedder's (`Renderer::set_emoji_default`).
    emoji: EmojiDefault,
    cols: Cols,
    /// The rows the frame draws: the main display's and the status line's
    /// below them, when a program shows one (`Term::status_rows`).
    rows: Rows,
    /// The main display's rows, the first of `rows`.
    main: Rows,
    prev: Vec<PackedCell>,
    known: Prev,
    /// The frame being converted, and its key ([`Mirror`]). Each row's
    /// line rendition in it, and in the last, is part of a row's damage,
    /// since DECDWL on a line whose cells it leaves alone, or DECSWL,
    /// changes how the row is drawn and no cell.
    cur: Mirror,
    prev_size: Vec<LineRendition>,
    /// What the frame does with each row of `cur`, in a buffer kept
    /// across frames.
    mirror_rows: Vec<MirrorRow>,
    /// The rows the last frame and this one veiled, composed over or
    /// noticed in `cur`, and whether either lent every row: the rows the
    /// next frame takes again, since `cur` holds them as drawn, not as
    /// stored.
    veiled_last: Vec<usize>,
    veiled_now: Vec<usize>,
    lent_last: LentRows,
    /// The rows of `cur` the last frame's lending rewrote, put back as the
    /// terminal's by [`Mirror::unlend`] at this frame's start, in a buffer
    /// kept across frames.
    lent_rows: Vec<usize>,
    /// What each slot the last frame's lending gave held, read before it
    /// is given back ([`crate::frozen::Lent::moved_from`]), in a buffer
    /// kept across frames.
    held_was: crate::frozen::LentValues,
    /// **The cells the strikes cover**, kept while nothing they are found
    /// from moved ([`StruckCells`]): made with the renderer, so none means
    /// only that a lending has them out ([`Renderer::take_struck`]) or
    /// lost them -- dropped with no put back, a panic between the two
    /// among the ways.
    struck: Option<StruckCells>,
    /// **Their generation** ([`StruckGen`]), kept here and not with them,
    /// so it reads true while they are out: moved by
    /// [`Renderer::take_struck`] before it hands them out.
    struck_gen: StruckGen,
    /// The finds of the struck cells made since the renderer was made.
    struck_finds: Tally,
    /// The cells of the frame being converted that belong to a picture
    /// drawn in text, whose backgrounds are drawn solid under the dim --
    /// [`Theme::finds_pictures`]. Found over the whole screen, because a
    /// picture is a property of a block of cells rather than of one.
    pictures: Pictures,
    /// **Each row of the window in the order UAX #9 gives it**, laid out
    /// once a frame by [`Renderer::lay_out_bidi`] and read by every path:
    /// the cursor, the selection, the find marks and the link underline
    /// keep logical columns and are drawn through it (`bidi.rs`).
    bidi: crate::bidi::Reorder,
    /// The window laid out for kitty's placeholders, which are placed
    /// before the frame's own layout: made at the first frame that holds a
    /// virtual placement.
    placeholder_bidi: Option<Box<crate::bidi::Reorder>>,
    /// The window one line up, laid out for the placeholders of its first
    /// line, the leaving row a smooth scroll slides out.
    placeholder_leaving: Option<Box<crate::bidi::Reorder>>,
    /// **What that layout reads besides the window's cells** (`Beside`),
    /// made the first time a frame has any: one pointer on a renderer that
    /// never composes, predicts or slides.
    beside: Option<Box<Beside>>,
    /// **The window and the status line in that order, kept with their
    /// keys** (`crate::bidi::DrawnOrder`): for the pictures drawn in text,
    /// which are found over the cells as they are drawn on every path, and
    /// lent by move to the cell path's frame (`crate::cell_frame::build`).
    drawn_order: crate::bidi::DrawnOrder,
    /// **The seal of the maps the last conversion drew through**
    /// ([`crate::bidi::Orders::laid_seal`]): a form, a mirror or a slant
    /// moves with no cell of its row changing -- a letter typed on the
    /// next row of a wrapped word, a row of the same characters either way
    /// round, its paragraph's direction set by the row before -- and the
    /// row's maps are written then, so a row laid out again otherwise since
    /// is converted again, and no row's maps are copied or compared here:
    /// the layout compares a row it lays out again with what it held, and
    /// one that came out as it was reads as not laid out again.
    laid_drawn: Option<crate::bidi::LaidSeal>,
    /// Whether this renderer has shaped a letter that takes a joining form:
    /// until it has, a layout with BDSM reset reads no row for joins
    /// ([`crate::bidi::Joins`]).
    joins_seen: crate::bidi::Joins,
    /// Whether a finished paragraph is drawn at its words
    /// ([`Renderer::set_word_wrap`]).
    word_wrap: crate::WordWrap,
    /// The find marks' wash, from the palette `set_find` read, so a frame's
    /// layout can draw the marks again through its order.
    find_wash: Argb,
    /// What the find marks and the link underline were last drawn from
    /// past the layout's order, and the seal of the maps they were drawn
    /// through, by [`Renderer::marks_after_layout`].
    marks_laid: Option<(MarksKey, crate::bidi::LaidSeal)>,
    /// **The CPU and instance paths' resolved rendition table**, kept from
    /// frame to frame and brought up to date by [`Renderer::sync_styles`]:
    /// only the slots the core wrote since, or every live slot when the table
    /// or an input changed. A cell's colours are one load from it.
    pub(crate) styles: Vec<Resolved>,
    pub(crate) styles_stamp: StyleStamp,
    /// **The slots of `styles` lent a frame's frozen cells** (`crate::frozen`):
    /// a window scrolled back over history the core froze, converted
    /// through its own copy of the window, its frozen cells given slots no
    /// other cell of the frame names. Given back before `styles` is brought
    /// up to date again.
    pub(crate) cpu_lent: crate::frozen::Lent,
    /// The lendings numbered so far, every table's (`StyleStamp`).
    pub(crate) lendings: crate::frozen::Lendings,
    /// The words the frame being drawn names in its table's lent slots
    /// ([`Renderer::set_frame_lent`]).
    pub(crate) frame_words: crate::frozen::Held,
    /// Those slots, and the lending they were lent under.
    pub(crate) frame_slots: Vec<baud_domain::StyleId>,
    pub(crate) frame_lending: crate::frozen::Lending,
    /// Each screen row's records the last time [`Renderer::sized_lines`]
    /// converted it as a sized line, and what they were converted from:
    /// a frame whose row and inputs are the same reuses them.
    sized_rows: Vec<Option<SizedRow>>,
    /// How many of those conversions were made rather than reused.
    sized_converted: Tally,
    /// How many rows the syllable walk has read ([`Renderer::syllable_rows`]).
    syllable_rows: Tally,
    /// How many words have been laid ([`Renderer::words_laid`]).
    words_laid: Tally,
    /// **Each row's words, kept while the row is the same**
    /// (`crate::kept`), so a frame lays only the words of the rows that
    /// changed.
    kept: crate::kept::KeptRows,
    /// Moves on every [`Renderer::set_theme`], and on every
    /// [`Renderer::restack`]: a resolved slot names the face style and the
    /// font its stack has faces for, so a copy resolved against the old
    /// stack is resolved again.
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
    /// Per cell, whether a Devanagari word drawn whole holds it
    /// (`crate::word`): its own colours are drawn unlit, the selection and
    /// the block cursor over its syllable's span.
    worded: Vec<crate::word::Worded>,
    /// Per row, the words the last convert of it drew, where they went.
    laid: Vec<Vec<crate::word::WordLaid>>,
    /// Per row, whether the last convert of it found a word, so a row of
    /// none clears no cell's [`Renderer::worded`] mark.
    row_worded: Vec<crate::word::Worded>,
    /// Per row, whether the last convert of it saw blinking text. A row a
    /// frame does not convert is unchanged, so its answer still holds, and
    /// `blink_seen` is the whole screen's.
    row_blinks: Vec<Blinking>,
    /// Per row, its instances, with the tallies a frame reports kept as
    /// each row is written (`crate::per_row`).
    per_row: crate::per_row::PerRow,
    /// **The buffers a row's conversion fills** ([`RowScratch`]), kept
    /// between rows and frames: [`Renderer::convert_row`] takes them,
    /// begins them empty and puts them back. None while a conversion holds
    /// them; a conversion inside another would begin buffers of its own
    /// rather than read the outer one's, and none is: `convert_with`,
    /// `hide_cursor`, `sized_lines`, `status_records` and `sized_leaving`
    /// each convert their rows one after another, never one inside another.
    row_scratch: Option<RowScratch>,
    /// Per row, how many of its instances are glyphs, which come first;
    /// the rest are its decorations.
    glyph_ends: Vec<usize>,
    /// Per row, the pixel rows its glyphs cover, and what they covered
    /// before this frame's convert: a row a glyph reaches, or reached, is
    /// repainted with the row it belongs to.
    reach: Vec<Reach>,
    reach_was: Vec<Reach>,
    last_cursor: Cursor,
    /// The opacity the last converted frame drew its cursor at, so a fade
    /// that moves the cursor's opacity and nothing else still dirties its
    /// row.
    last_cursor_opacity: Chan,
    /// How the cursor is drawn ([`Renderer::set_cursor_look`]), and how
    /// the last converted frame drew it, so a flip with nothing else
    /// changed still dirties the cursor's row.
    cursor_look: CursorLook,
    last_cursor_look: CursorLook,
    /// The selection on this frame's view, set by [`Renderer::set_highlight`]
    /// before a convert, and the one the last convert drew -- a row whose
    /// share of it changed is dirty, the cursor's own rule.
    highlight: Option<Highlight>,
    last_highlight: Option<Highlight>,
    /// The colours the selection draws in, read from the terminal each frame
    /// ([`Term::highlight_colors`]): reverse video until an application sets
    /// `OSC 17` or `19`.
    hilite: Hilite,
    /// The same with no copy pulse in it, which baud's own fields draw in:
    /// the pulse is the selection's, and the address and the find field keep
    /// their colours through it ([`Renderer::set_selection_pulse`]).
    field_hilite: Hilite,
    /// The colours a reverse-video selection takes while the window is not
    /// key, or none: [`Renderer::set_inactive_selection`].
    inactive: Option<Hilite>,
    /// The scrollbar on this frame and the last -- the knob, the track when
    /// widened, and how much of both is seen -- and its solids: a row whose
    /// slice of the bar changed is dirty (`Bar::slice`).
    bar: Option<Bar>,
    last_bar: Option<Bar>,
    bar_inst: Vec<Instance>,
    /// The pixels past the grid the frame keeps for a legacy scroller.
    gutter: Px,
    /// **The exit-status marks** ([`crate::status`]): whether the strip is
    /// kept, the marks on the window this frame, their records in the
    /// strip's own pixels, and what the CPU path last painted there -- the
    /// clear colour and those records -- so it repaints the strip only when
    /// one of them changed or the frame is new.
    status: crate::status::StatusMarks,
    placed: Vec<crate::status::Placed>,
    strip_inst: Vec<Instance>,
    strip_painted: Option<(Argb, Vec<Instance>)>,
    /// Whether the last CPU frame repainted the strip, for
    /// [`Renderer::painted`]: its whole height then.
    strip_fresh: StripFresh,
    /// **The word wrap's continuation marks** ([`crate::wrapmark`]):
    /// whether they are drawn, the band of the window's padding the driver
    /// lends each side for them ([`Renderer::set_margins`]), their records
    /// in the whole frame's pixels, and what the CPU path last painted in
    /// the bands, as the strip keeps it.
    wrap_marker: crate::wrapmark::WrapMarker,
    margins: Px,
    margin_inst: Vec<Instance>,
    margin_painted: Option<(Argb, Vec<Instance>)>,
    margin_fresh: StripFresh,
    /// **The activity lights' own line** ([`Renderer::set_lights_line`]):
    /// a band of the whole frame outside the grid, above or below it.
    lights: crate::lights::LineState,
    /// The cursor trail's ghost on this frame and the last, and its one
    /// solid instance: a row the ghost is over or was over is dirty, the
    /// scrollbar's rule, so the trail's damage stays bounded to the rows it
    /// sweeps rather than wiping the frame the way sfte's does.
    trail: Option<Drawn>,
    last_trail: Option<Drawn>,
    trail_inst: Vec<Instance>,
    /// The trail's and the splash's records the last convert drew: a row
    /// dirty only for them repaints the columns they and this frame's
    /// reach on it ([`Repaint::Cols`]).
    last_trail_inst: Vec<Instance>,
    last_splash_inst: Vec<Instance>,
    /// The trail the driver asked for, which [`Renderer::trail`] draws
    /// unless the splash's plasma holds the effect's slot, and whether a
    /// trail lit while it did is still being held back.
    asked_trail: Option<Drawn>,
    held_trail: HeldTrail,
    /// **The fireworks** the driver asked for ([`Renderer::set_fireworks`]),
    /// which hold the effect's slot after the splash's plasma.
    fireworks: crate::fireworks::Shows,
    /// **The launch splash** on this frame and the last ([`SplashLook`]),
    /// the scale its glyphs were first drawn at, and its records: the
    /// lockup's glyphs and block, drawn with baud's own overlays over
    /// everything but the thumb and the fade. Under full motion its plasma
    /// is the frame's effect.
    splash: Option<SplashLook>,
    last_splash: Option<SplashLook>,
    splash_scale: Option<f32>,
    splash_inst: Vec<Instance>,
    /// **What of the frame the shell has drawn**, which the splash's
    /// records and its plasma are cut out of ([`Renderer::splash_yields_to`]).
    splash_holes: Vec<splash::Hole>,
    /// The splash's block drawn as a blinking cursor at this phase
    /// ([`Renderer::set_splash_cursor`]): shown, its own, unless set.
    splash_cursor: CursorPhase,
    /// The plasma layers the CPU path draws with the splash on this frame
    /// and the last ([`Renderer::set_splash_layers`]): a frame with either
    /// repaints whole.
    splash_layers: Vec<splash::Layer>,
    last_splash_layers: Vec<splash::Layer>,
    /// The colour the splash's layers take ([`Renderer::set_splash_ink`]).
    splash_ink: Option<Argb>,
    /// The lockup's glyphs as slabs to the layers ([`splash::Slabs`]).
    splash_slabs: Option<splash::Slabs>,
    /// An input method's composition on this frame and the last, and its
    /// records: the rows under either are dirty, and the cells under it are
    /// converted blank so its own glyphs replace theirs ([`Composition`]).
    composition: Option<Composition>,
    last_composition: Option<Composition>,
    compose_inst: Vec<Instance>,
    /// The cells this frame's composition draws ([`crate::overlay::Taken`]):
    /// the letters beside it its words take in, converted blank and drawn
    /// unlit, and what its cells will hold, which the row's words read.
    taken: crate::overlay::Taken,
    /// The composition's pages and glyphs on a row that joins, drawn in
    /// that row's order by [`Renderer::convert_row`] ([`Splice`]).
    splice: Option<Splice>,
    /// The find overlay on this frame and the last ([`FindLook`]), and its
    /// solids: each match's wash and the field's caret. The rows under
    /// either frame's marks or field are dirty, and the cells under the
    /// field are converted blank, as under a composition.
    find: Option<FindLook>,
    last_find: Option<FindLook>,
    find_inst: Vec<Instance>,
    /// The find field's caret, drawn with the field over the images.
    find_caret: Vec<Instance>,
    /// The link preview on this frame and the last ([`LinkLook`]), and its
    /// solids, the link's underline. The rows under either frame's runs or
    /// bar are dirty, and the cells under the bar are converted blank, as
    /// under the find field.
    link: Option<LinkLook>,
    last_link: Option<LinkLook>,
    /// **The frame's links as the driver set them** (`links`): the lines
    /// at rest and how they are drawn, the struck links -- the runs at rest,
    /// then those the gesture's modifier strikes
    /// ([`Renderer::set_link_strikes`]), each struck through with the lines'
    /// solids and its text drawn grey by its own cells, through slots the
    /// frame's table lends it ([`Renderer::take_struck`], `crate::frozen`)
    /// -- `[link] missing_color`, and the link under the pointer, whose
    /// `hover = "line"` goes with the lines' solids and `"box"` and
    /// `"text"` with the bar, over the images.
    links: LinkInputs,
    /// **The solids of the lines, the strikes and `hover = "line"`**
    /// (`Renderer::place_links`), kept while nothing they are drawn from
    /// moved.
    kept_solids: KeptSolids,
    /// The lines at rest, the strikes and the link under the pointer the
    /// last frame drew: a row whose lines changed is dirty.
    last_link_lines: Vec<LinkLine>,
    last_link_strikes: Vec<LinkLine>,
    last_link_hover: Option<LinkHover>,
    hover_bars: Vec<HoverBar>,
    last_hover_bars: Vec<HoverBar>,
    /// Bars no field took, keeping their buffers for the next ([`hover_fields`]).
    hover_spare: Vec<HoverBar>,
    /// The hover tip on this frame and the last ([`crate::tip`]): its
    /// lines' rows are dirty, and the cells under them converted blank,
    /// as under the link preview's bar.
    tip: Option<crate::tip::TipLook>,
    last_tip: Option<crate::tip::TipLook>,
    /// What a field in the message pair is drawn in, and whether the
    /// selection takes it for a moment ([`crate::message`]).
    message_colours: crate::message::MessageColours,
    selection_pulse: crate::message::SelectionPulse,
    /// The embedder's notice, its own row below the grid and the status
    /// line ([`Renderer::set_notice`]), and the field the last frame drew
    /// it as.
    notice: Option<Vec<Codepoint>>,
    last_notice: Option<crate::find::Field>,
    link_inst: Vec<Instance>,
    /// **baud's own fields over the images**: the find field's records and
    /// caret and the link preview's bar, which the instance and CPU paths
    /// draw after the images over the text, with the thumb and the fade
    /// ([`Renderer::chrome`]), so a program's picture can neither hide the
    /// address the bar shows nor show one of its own in its place.
    chrome_inst: Vec<Instance>,
    /// **baud's veils as the instance and CPU paths drew them last, and as
    /// the cell path did** (FS-C16), each with the records they were
    /// composed into, drawn again while their key holds ([`Veiled`]).
    cpu_veils: Veiled,
    cell_veils: Veiled,
    /// What the cursor is filled with, which a quad trail is drawn in: the
    /// application's `OSC 12` colour, else the default foreground a block
    /// cursor's reversed cell fills with.
    cursor_fill: Argb,
    /// What a block cursor is drawn in, and a bar or underline's fill: the
    /// cursor colour -- a program's `OSC 12`, else the configured one the
    /// core holds -- else the theme's, which may name the cell's own, and
    /// the theme's text under the block ([`Renderer::block_paint`]).
    block_paint: BlockPaint,
    /// baud's cursor on the last page it was asked for
    /// ([`Theme::bauds_cursor_on`]), which a flip of the video, an `OSC 11`
    /// or a new theme asks again, once.
    bauds: (Argb, BaudCursor),
    /// **The most alpha a repeat pass's records carry** (note AL, option
    /// O2): the least [`flash::under_threshold`] of the cursor's colour over
    /// the backgrounds of the cells they cover, set by
    /// [`Renderer::bound_trail`] and read when the records are built.
    flash_cap: Chan,
    /// Why [`Renderer::flash_cap`] is what it is.
    flash_held: flash::Held,
    /// The caps already scanned, by cursor colour, text colour and page:
    /// a screen's worth, [`FLASH_CACHED`], emptied between frames when full.
    /// And by the trail's mix ([`Renderer::trail_mix`]), the composite each
    /// is scanned over.
    flash_caps: crate::seeded::Map<(Argb, Argb, Argb, Mix), Chan>,
    /// The alternate-screen fade's veil on this frame and the last, and its
    /// one full-screen solid: while it fades every row is dirty, because the
    /// veil covers the whole screen and the whole screen is what changed.
    fade: Option<Veil>,
    last_fade: Option<Veil>,
    /// The slide's faint veil ([`transition::faint_alpha`]) and the
    /// default background it was found over, found again only when that
    /// changes.
    faint: Option<(Argb, Chan)>,
    fade_inst: Vec<Instance>,
    /// The visual bell's solid on this frame and the last
    /// ([`Renderer::set_bell_tint`]), and its record: while it fades every
    /// row is dirty, as under the screen fade.
    bell: Option<crate::bell::BellTint>,
    last_bell: Option<crate::bell::BellTint>,
    bell_inst: Vec<Instance>,
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
    /// Whether a slot this renderer resolved has been encircled (`SGR
    /// 52`): from then on the glyph map holds the outline's parts for the
    /// cell shader, and before, nothing draws them into the atlas.
    rings: Rings,
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
    /// iTerm2's cursor guide while it is shown ([`Renderer::set_cursor_guide`]),
    /// where it lies on this frame and the last, and its records: a row
    /// under either is dirty, as the glow's is.
    guide: Option<GuideLook>,
    guide_at: Option<guide::Placed>,
    last_guide_at: Option<guide::Placed>,
    guide_inst: Vec<Instance>,
    /// iTerm2's badge ([`Renderer::set_badge`]): the look and frame size
    /// its records were built for, the records, the rectangle they cover
    /// on this frame and the last, and a count of rebuilds, which marks
    /// the rows under either rectangle dirty.
    badge_key: Option<(BadgeLook, (Px, Px))>,
    badge_inst: Vec<Instance>,
    /// The badge's records as this frame draws them, cut around what it
    /// stays out from under ([`Renderer::place_grounds`]), and those holes.
    badge_drawn: Vec<Instance>,
    badge_holes: Vec<[i32; 4]>,
    /// The pixel sizes the badge's glyphs are held at in the atlas: at most
    /// [`BADGE_SIZES`], past which the atlas starts over.
    badge_sizes: Vec<f32>,
    badge_rect: Option<[u32; 4]>,
    last_badge_rect: Option<[u32; 4]>,
    badge_built: Tally,
    last_badge_built: Tally,
    /// Whether this frame shows the other video, set by
    /// [`Renderer::set_flash`] before a convert or resolve, the way the blink
    /// phase is.
    flash: VideoFlash,
    /// The visible image placements this frame, screen rects in device
    /// pixels, computed once by [`Renderer::set_images`] and consumed by
    /// both the CPU path's row blits and the two GPU arms -- one description
    /// of where an image goes, not three.
    image_places: Vec<ImagePlacement>,
    /// [`Renderer::image_places`] by the rows each covers, built with it.
    place_rows: PlaceRows,
    /// Each placement's nearest-neighbour tables, at its [`MapAt`]: the
    /// texel x of every visible drawn column, then the texel y of every
    /// visible drawn row, from `baud_domain::ImageDraw::nearest`. Built
    /// with the placements; the CPU reads them for a scaled placement and
    /// the GPU paths for every one, so the paths sample one way.
    image_maps: Vec<u32>,
    /// **Where each virtual placement's placeholders begin this frame**,
    /// by its serial: the least draw row and the least display column of
    /// the runs showing it, what a relative placement under it is placed
    /// against ("The x position is the minimum of all the placeholder x
    /// positions and the y position is the minimum of all the placeholder
    /// y positions", `[[source: kitty-graphics]]`, "Relative placements").
    virtual_at: Vec<(PlaceSerial, i64, i64)>,
    /// The placements the last CPU frame drew, so the next one repaints the
    /// rows an image left as well as the rows it is on.
    last_image_places: Vec<ImagePlacement>,
    /// Which placements this CPU frame and the last differ by.
    place_diff: PlaceDiff,
    /// Each referenced image's pixels, straight, for a GPU to upload
    /// once. Keyed by id and store epoch; the CPU path reads the term's
    /// pixels directly instead. **Kept across frames** and refreshed only for a
    /// new `(id, epoch, ground)`, so a static image is snapshotted once rather than
    /// rebuilt every frame -- a large image's per-frame allocation and
    /// per-pixel conversion, which the frame path would otherwise pay while it
    /// is on screen. Cleared whole when the store epoch moves. See
    /// [`Renderer::set_images`].
    image_uploads: Vec<ImageUpload>,
    /// What a Sixel's fill draws as in the placements being built
    /// ([`Renderer::fill_pixel`]), read from the term as they are built.
    image_fill: Pixel,
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
    /// How much of each dirty row `paint` repaints.
    repaint: Vec<Repaint>,
    /// **The threads a CPU frame draws an effect on, at most**: the
    /// machine's, unless [`Renderer::set_threads`] said otherwise.
    threads: core::num::NonZeroUsize,
    /// **What every convert since the last [`Renderer::take_redraw`]
    /// changed**, for a GPU path that draws into surfaces it keeps: the
    /// rows dirty only for the cursor, its blink animation, its trail or
    /// the cursor guide, else [`Redraw::Whole`]. Gathered across converts,
    /// so a frame converted and never drawn -- a draw refused while one is
    /// in flight -- still names its rows to the frame that is.
    redraw: crate::Redraw,
    storage: Vec<Glyph>,
    /// The shaper's scratch for a varied face's glyph boxes, grown to the
    /// need a refusal names and kept; empty while no face varies.
    ink_scratch: Vec<VarPoint>,
}

/// **Where a face's layout tables are** in the renderer's per-layout
/// lists ([`ByLayout`]): what is built from its `GSUB`, `GPOS`, `GDEF` and
/// `morx` -- lookup digests, pair tables, extents, reaches, per-script
/// selections. One per face, but a narrower instance
/// ([`crate::FaceRole::Narrower`]) takes its base's: by its role it is its
/// base's face read at another width, and the tables are built from those
/// four alone, a face's variation coordinates entering none of them, so
/// they are the same tables (`same_layout`, asserted in debug builds). The default stack's 60 narrower instances
/// built them 60 times more: with the face table's proposals, 2.5 ms of
/// `Renderer::new` and 7 MB at launch in a release build.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Layout(u8);

/// One entry per [`Layout`]: a face's is `self[layouts[face]]`.
struct ByLayout<T>(Vec<T>);

impl<T> ByLayout<T> {
    fn get(&self, at: Layout) -> Option<&T> {
        self.0.get(usize::from(at.0))
    }

    /// Every layout's entry, once each.
    fn all(&self) -> &[T] {
        &self.0
    }
}

impl<T> core::ops::Index<Layout> for ByLayout<T> {
    type Output = T;
    fn index(&self, at: Layout) -> &T {
        &self.0[usize::from(at.0)]
    }
}

impl<T> FromIterator<T> for ByLayout<T> {
    fn from_iter<I: IntoIterator<Item = T>>(i: I) -> ByLayout<T> {
        ByLayout(i.into_iter().collect())
    }
}

/// Whether `a` and `b` hold the same layout tables, byte for byte: the
/// same `GSUB`, `GPOS`, `GDEF` and `morx`, or both without one. A narrower
/// instance's contract with its base ([`Layout`]), checked where debug
/// assertions are on: 141 KB a built face, 8.4 MB over the default
/// stack's 60, which a release launch does not read.
fn same_layout(a: &Face<'_>, b: &Face<'_>) -> bool {
    [Tag::GSUB, Tag::GPOS, Tag::GDEF, Tag::MORX]
        .into_iter()
        .all(|t| a.table(t).map(|x| x.as_slice()) == b.table(t).map(|x| x.as_slice()))
}

/// **Which faces number their glyphs alike**: one per face, a narrower
/// instance its base's, two faces the same where [`same_glyphs`] holds of
/// their bases. A glyph id one of them shapes is the glyph the other draws
/// at that id, so a syllable shaped in one face draws each glyph from the
/// face its own cell names where the two are alike
/// ([`Renderer::word_at`]): the built Regular and Bold are, and the built
/// Italic and Bold Italic, and no upright face is an italic one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct GlyphOrder(u8);

/// **Whether `a` and `b` number their glyphs alike**: as many glyphs, and
/// the same `cmap`, `GSUB` and `morx`, byte for byte, or both without one
/// -- so every character and every glyph a substitution forms from it
/// lands on the same id in both. A face whose bold adds or changes one
/// substitution is not alike, and a syllable in two renditions keeps its
/// first cell's face there. Asked once per stack, of its faces that are
/// not narrower instances, and the tables are read only for a pair that
/// holds as many glyphs: the built Regular and Bold each hold 2663, with
/// 18,560 bytes of `cmap` and 88,844 of `GSUB` alike.
fn same_glyphs(a: &Face<'_>, b: &Face<'_>) -> bool {
    a.glyph_count() == b.glyph_count()
        && [Tag::CMAP, Tag::GSUB, Tag::MORX]
            .into_iter()
            .all(|t| a.table(t).map(|x| x.as_slice()) == b.table(t).map(|x| x.as_slice()))
}

/// What the renderer reads of each face of its stack, built once per
/// stack: by [`Renderer::new`], and again by [`Renderer::restack`]; the
/// layout tables once per [`Layout`].
struct PerFace {
    scales: Vec<Scale>,
    layouts: Vec<Layout>,
    orders: Vec<GlyphOrder>,
    digests: ByLayout<Vec<LookupDigest>>,
    #[cfg(feature = "exceptions")]
    starters: ByLayout<Starter>,
    pairs: ByLayout<Pairs>,
    extents: ByLayout<Option<Count>>,
    reaches: ByLayout<Count>,
    selections: ByLayout<Selections>,
    faces: Vec<FaceIx>,
}

impl PerFace {
    fn of(stack: &Stack<'_, '_>, size: PxSize) -> PerFace {
        let scales = stack
            .faces()
            .iter()
            .map(|f| Scale::of(size.value(), f.units_per_em()))
            .collect();
        // Each face's layout, and the face each layout is built from: a
        // narrower instance takes its base's.
        let mut layouts: Vec<Layout> = Vec::with_capacity(stack.faces().len());
        let mut built: Vec<&Face<'_>> = Vec::new();
        for ix in stack.each() {
            let shared = stack.narrows(ix).and_then(|b| {
                debug_assert!(
                    same_layout(stack.face(ix), stack.face(b)),
                    "a narrower instance holds its base's layout tables"
                );
                layouts.get(b.index()).copied()
            });
            layouts.push(shared.unwrap_or_else(|| {
                built.push(stack.face(ix));
                Layout(u8::try_from(built.len() - 1).unwrap_or(u8::MAX))
            }));
        }
        let digests = built
            .iter()
            .map(|f| {
                let mut d = Vec::new();
                if let Some(gsub) = f.gsub() {
                    d.resize(MAX_LOOKUPS, LookupDigest::EMPTY);
                    gsub.digests(Features::JOINING, &mut d);
                }
                d
            })
            .collect();
        #[cfg(feature = "exceptions")]
        let starters = built
            .iter()
            .map(|f| match (Substitution::of(f), f.gsub()) {
                (Substitution::Gsub, Some(gsub)) => {
                    let mut d = vec![LookupDigest::EMPTY; MAX_LOOKUPS];
                    gsub.digests(Features::GRID, &mut d);
                    Starter::Gsub(d.into_iter().fold(LookupDigest::EMPTY, LookupDigest::union))
                }
                (Substitution::Morx, _) => Starter::Morx,
                _ => Starter::Plain,
            })
            .collect();
        let pairs = built.iter().map(|f| Pairs::of(f)).collect();
        let extents = built
            .iter()
            .map(|f| {
                f.gsub()
                    .filter(|_| Substitution::of(f) == Substitution::Gsub)
                    .and_then(|g| g.extent(Features::GRID))
            })
            .collect();
        let reaches = built
            .iter()
            .map(|f| Count::of(baud_font::joined_reach(f).value().min(WORD_REACH_MAX)))
            .collect();
        let selections = built.iter().map(|f| Selections::of(f)).collect();
        // Each layout's face's order: the first face before it alike, else
        // its own.
        let mut alike: Vec<&Face<'_>> = Vec::new();
        let by_layout: Vec<GlyphOrder> = built
            .iter()
            .map(|f| {
                let at = alike
                    .iter()
                    .position(|g| same_glyphs(g, f))
                    .unwrap_or_else(|| {
                        alike.push(f);
                        alike.len() - 1
                    });
                GlyphOrder(u8::try_from(at).unwrap_or(u8::MAX))
            })
            .collect();
        let orders = layouts
            .iter()
            .map(|l| {
                by_layout
                    .get(usize::from(l.0))
                    .copied()
                    .unwrap_or(GlyphOrder(0))
            })
            .collect();
        let faces = stack.each().collect();
        PerFace {
            scales,
            layouts,
            orders,
            digests,
            #[cfg(feature = "exceptions")]
            starters,
            pairs,
            extents,
            reaches,
            selections,
            faces,
        }
    }
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
        let PerFace {
            scales,
            layouts,
            orders,
            digests,
            #[cfg(feature = "exceptions")]
            starters,
            pairs,
            extents,
            reaches,
            selections,
            faces,
        } = PerFace::of(&stack, size);
        let defaults = (theme.fg, theme.bg);
        let reversed = (theme.reversed_alpha(defaults.0, defaults.1), defaults);
        let mut r = Renderer {
            faces_of: stack.table(),
            cluster_faces: ClusterMemo::new(Question::Face, FaceIx::PRIMARY),
            font_faces: crate::seeded::Map::default(),
            cluster_covers: ClusterMemo::new(Question::Cover, Cover::Whole),
            stack,
            scales,
            layouts,
            orders,
            #[cfg(feature = "exceptions")]
            starters,
            pairs,
            extents,
            reaches,
            selections,
            row_scripts: Vec::new(),
            row_unresolved: Vec::new(),
            faces,
            #[cfg(feature = "exceptions")]
            singles: crate::seeded::Map::default(),
            digests,
            metrics,
            theme,
            video: Video::Normal,
            reversed,
            defaults,
            page_default: defaults.1,
            blend,
            cache: RunCache::new(),
            atlas: Atlas::new(),
            emoji: EmojiDefault::Font,
            cols: Cols::of_index(0),
            rows: Rows::of_index(0),
            main: Rows::of_index(0),
            prev: Vec::new(),
            known: Prev::Unknown,
            cur: Mirror::default(),
            prev_size: Vec::new(),
            mirror_rows: Vec::new(),
            veiled_last: Vec::new(),
            veiled_now: Vec::new(),
            lent_last: LentRows::None,
            lent_rows: Vec::new(),
            held_was: crate::frozen::LentValues::default(),
            struck: Some(StruckCells::new()),
            struck_gen: StruckGen::fresh(),
            struck_finds: Tally::ZERO,
            pictures: Pictures::new(),
            bidi: crate::bidi::Reorder::new(),
            placeholder_bidi: None,
            placeholder_leaving: None,
            beside: None,
            drawn_order: crate::bidi::DrawnOrder::default(),
            laid_drawn: None,
            joins_seen: crate::bidi::Joins::Unseen,
            word_wrap: crate::WordWrap::default(),
            find_wash: Argb(0),
            marks_laid: None,
            styles: Vec::new(),
            styles_stamp: StyleStamp::default(),
            cpu_lent: crate::frozen::Lent::default(),
            lendings: crate::frozen::Lendings::default(),
            frame_words: crate::frozen::Held::default(),
            frame_slots: Vec::new(),
            frame_lending: crate::frozen::Lending::default(),
            sized_rows: Vec::new(),
            sized_converted: Tally::default(),
            syllable_rows: Tally::default(),
            words_laid: Tally::default(),
            kept: crate::kept::KeptRows::default(),
            theme_gen: ThemeGen::default(),
            bg: Vec::new(),
            fg: Vec::new(),
            curve: Vec::new(),
            text: Vec::new(),
            worded: Vec::new(),
            laid: Vec::new(),
            row_worded: Vec::new(),
            row_blinks: Vec::new(),
            per_row: crate::per_row::PerRow::default(),
            row_scratch: None,
            glyph_ends: Vec::new(),
            reach: Vec::new(),
            reach_was: Vec::new(),
            last_cursor: None,
            last_cursor_opacity: Chan::from_u8(0xFF),
            cursor_look: CursorLook::Shaped,
            last_cursor_look: CursorLook::Shaped,
            highlight: None,
            last_highlight: None,
            hilite: Hilite::REVERSE,
            field_hilite: Hilite::REVERSE,
            inactive: None,
            bar: None,
            last_bar: None,
            gutter: Px::of(0),
            status: crate::status::StatusMarks::Off,
            placed: Vec::new(),
            strip_inst: Vec::new(),
            strip_painted: None,
            strip_fresh: StripFresh::No,
            wrap_marker: crate::wrapmark::WrapMarker::On,
            margins: Px::of(0),
            margin_inst: Vec::new(),
            margin_painted: None,
            margin_fresh: StripFresh::No,
            lights: crate::lights::LineState::new(),
            bar_inst: Vec::new(),
            trail: None,
            last_trail: None,
            trail_inst: Vec::new(),
            last_trail_inst: Vec::new(),
            last_splash_inst: Vec::new(),
            asked_trail: None,
            held_trail: HeldTrail::No,
            fireworks: [None; 2],
            splash: None,
            last_splash: None,
            splash_scale: None,
            splash_inst: Vec::new(),
            splash_holes: Vec::new(),
            splash_cursor: CursorPhase::SHOWN,
            splash_layers: Vec::new(),
            last_splash_layers: Vec::new(),
            splash_ink: None,
            splash_slabs: None,
            composition: None,
            last_composition: None,
            taken: crate::overlay::Taken::default(),
            compose_inst: Vec::new(),
            splice: None,
            find: None,
            last_find: None,
            find_inst: Vec::new(),
            find_caret: Vec::new(),
            link: None,
            last_link: None,
            links: LinkInputs::none(),
            kept_solids: KeptSolids::none(),
            last_link_lines: Vec::new(),
            last_link_strikes: Vec::new(),
            last_link_hover: None,
            hover_bars: Vec::new(),
            last_hover_bars: Vec::new(),
            hover_spare: Vec::new(),
            tip: None,
            last_tip: None,
            message_colours: crate::message::MessageColours::default(),
            selection_pulse: crate::message::SelectionPulse::Off,
            notice: None,
            last_notice: None,
            link_inst: Vec::new(),
            chrome_inst: Vec::new(),
            cpu_veils: Veiled::reaching(VeilReach::Main),
            cell_veils: Veiled::reaching(VeilReach::Every),
            cursor_fill: Argb::default(),
            block_paint: BlockPaint::SWAP,
            bauds: (Argb::default(), BaudCursor::dark()),
            flash_cap: Chan::from_u8(0),
            flash_held: flash::Held::Full,
            flash_caps: crate::seeded::Map::default(),
            fade: None,
            last_fade: None,
            faint: None,
            fade_inst: Vec::new(),
            bell: None,
            last_bell: None,
            bell_inst: Vec::new(),
            last_clusters: None,
            blink: BlinkPhases::SHOWN,
            blink_slots: Tally::default(),
            rings: Rings::Unused,
            blink_seen: Blinking::default(),
            cursor_blink: CursorPhase::SHOWN,
            glow: None,
            last_glow: None,
            glow_inst: Vec::new(),
            guide: None,
            guide_at: None,
            last_guide_at: None,
            guide_inst: Vec::new(),
            badge_key: None,
            badge_inst: Vec::new(),
            badge_drawn: Vec::new(),
            badge_holes: Vec::new(),
            badge_sizes: Vec::new(),
            badge_rect: None,
            last_badge_rect: None,
            badge_built: Tally::default(),
            last_badge_built: Tally::default(),
            flash: VideoFlash::default(),
            image_places: Vec::new(),
            place_rows: PlaceRows::default(),
            image_maps: Vec::new(),
            virtual_at: Vec::new(),
            last_image_places: Vec::new(),
            place_diff: PlaceDiff::default(),
            image_uploads: Vec::new(),
            image_fill: Pixel::CLEAR,
            image_epoch: ImageGen::ZERO,
            image_snapshots: Tally::of(0),
            marks_unmapped: Tally::of(0),
            ink_refused: Tally::of(0),
            dirty: Vec::new(),
            repaint: Vec::new(),
            threads: std::thread::available_parallelism().unwrap_or(core::num::NonZeroUsize::MIN),
            redraw: crate::Redraw::Whole,
            storage: vec![Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 256],
            ink_scratch: Vec::new(),
        };
        r.atlas.at(SizeKey::of(size));
        r.resize(cols, rows);
        r.main = rows;
        r.bidi.set_reach(r.widest_reach());
        Some(r)
    }

    /// The widest reach of the stack's faces: how many cells of its word
    /// either side of a joined cell the layout keeps.
    fn widest_reach(&self) -> Count {
        widest_of(self.reaches.all())
    }

    /// **How many cells of its word either side of a joined cell this
    /// renderer's layout keeps** ([`crate::bidi::Reorder::set_reach`]): the
    /// widest [`baud_font::joined_reach`] of its faces, at most 16. A
    /// layout made beside it with this reach finds the joins it finds.
    pub fn joined_reach(&self) -> Count {
        self.widest_reach()
    }

    pub fn metrics(&self) -> Metrics {
        self.metrics
    }

    /// **The frame's totals of its instances counted afresh** -- all of
    /// them, those on the colour texture, those solid -- by walking every
    /// row's instances, as each frame did before the totals were kept as
    /// the rows are written: the count harness holds every frame's
    /// [`Frame`] totals to this.
    #[cfg(feature = "counters")]
    pub fn instances_recounted(&self) -> (Tally, Tally, Tally) {
        let k = self.per_row.recounted();
        (k.instances(), k.colored(), k.solids())
    }

    /// A new pixel size for the same faces -- the backing scale changed, or
    /// the font size -- so the metrics and the per-face scales are rebuilt,
    /// the atlas draws at the new size, and the next convert repaints every
    /// row. `None` leaves the renderer as it was, when the primary face
    /// cannot be measured at that size.
    ///
    /// **The run cache is kept.** A `Shaped` holds glyph ids, cell spans and
    /// offsets *in font units* (`Placed`), and the current `scale` is applied
    /// at composite -- none of it depends on the pixel size, so shaping a run
    /// at the old size still names the right glyphs at the new one. Clearing
    /// the cache re-shaped every visible run for nothing, which was ~1.4 ms
    /// of the cold frame a font change forces (btop 14->24 px: convert 2.5
    /// ms to 1.1 ms, cache misses 163 to 0).
    ///
    /// **And the glyphs already drawn are kept.** Only the atlas holds
    /// pixels, and it holds them per size ([`Atlas::at`]): a zoom to a size
    /// not drawn at rasterises what the frame shows, as it always did, and
    /// a zoom back to a size held since the atlas last started over (at
    /// most [`crate::atlas::SIZES`]) rasterises nothing and replaces
    /// neither texture. This is not a sliding window of the last four
    /// sizes: walking 14->15->16->17->18 starts over at 18, so going back
    /// to 17 draws 17 again.
    pub fn rescale(&mut self, size: PxSize) -> Option<()> {
        let metrics = Metrics::of(self.stack.primary(), size)?;
        self.scales = self
            .stack
            .faces()
            .iter()
            .map(|f| Scale::of(size.value(), f.units_per_em()))
            .collect();
        self.metrics = metrics;
        // The badge's glyphs are drawn at a scale of their own, in the key,
        // so they stand at every cell size, and the sizes they are held at
        // still count until the atlas starts over. Its records name the old
        // cell grid: built again at the next `set_badge`.
        if self.atlas.at(SizeKey::of(size)) == Rescaled::Restarted {
            self.badge_sizes.clear();
        }
        self.badge_key = None;
        #[cfg(feature = "exceptions")]
        self.singles.clear();
        self.known = Prev::Unknown;
        Some(())
    }

    /// **Another stack of faces at `size`** -- a program's `OSC 50` naming
    /// another family -- so every table read per face is built again, the
    /// faces-of table, the cluster memos, the run cache and the atlas
    /// start over, and the next convert repaints every row: a run cache
    /// entry names glyph ids of the faces it was shaped in, which a new
    /// stack renumbers. `None` leaves the renderer as it was, when the new
    /// primary face cannot be measured at that size.
    pub fn restack(&mut self, stack: Stack<'f, 'a>, size: PxSize) -> Option<()> {
        let metrics = Metrics::of(stack.primary(), size)?;
        let PerFace {
            scales,
            layouts,
            orders,
            digests,
            #[cfg(feature = "exceptions")]
            starters,
            pairs,
            extents,
            reaches,
            selections,
            faces,
        } = PerFace::of(&stack, size);
        self.faces_of = stack.table();
        self.stack = stack;
        self.theme_gen = ThemeGen(self.theme_gen.0.wrapping_add(1));
        self.scales = scales;
        self.layouts = layouts;
        self.orders = orders;
        self.digests = digests;
        #[cfg(feature = "exceptions")]
        {
            self.starters = starters;
            self.singles.clear();
        }
        self.pairs = pairs;
        self.extents = extents;
        self.reaches = reaches;
        self.bidi.set_reach(self.widest_reach());
        self.selections = selections;
        self.faces = faces;
        self.cluster_faces = ClusterMemo::new(Question::Face, FaceIx::PRIMARY);
        self.font_faces.clear();
        self.cluster_covers = ClusterMemo::new(Question::Cover, Cover::Whole);
        self.cache = RunCache::new();
        self.metrics = metrics;
        self.atlas.reset();
        self.atlas.at(SizeKey::of(size));
        // The badge's glyphs went with the rest: built again at the next
        // `set_badge`.
        self.badge_key = None;
        self.badge_sizes.clear();
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
        self.badge_key = None;
        self.badge_sizes.clear();
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
    ///
    /// **A cell in an alternative font or under Fraktur** takes the font's
    /// faces first and then the face the table gives
    /// ([`Stack::select_font`]), remembered per codepoint, style, font and
    /// presentation (`Renderer::font_face`); a cell in the primary font
    /// never reaches it.
    #[inline]
    pub fn face_for(&mut self, cps: &[Codepoint], face: impl Into<TextFace>) -> FaceIx {
        let look = face.into();
        if look.font != FontSlot::PRIMARY {
            return self.font_face_for(cps, look);
        }
        let Some(&base) = cps.first() else {
            return FaceIx::PRIMARY;
        };
        let p = self.emoji.presentation(base, cps.get(1).copied());
        let face = self.faces_of.face(&self.stack, base, look.style, p);
        match cps {
            [_] => face,
            _ => self.cluster_face(cps, face),
        }
    }

    /// [`Renderer::face_for`] for a cell in an alternative font or under
    /// Fraktur: out of line, so a cell in the primary font pays one test.
    #[cold]
    #[inline(never)]
    fn font_face_for(&mut self, cps: &[Codepoint], look: TextFace) -> FaceIx {
        let Some(&base) = cps.first() else {
            return FaceIx::PRIMARY;
        };
        let p = self.emoji.presentation(base, cps.get(1).copied());
        let chain = self.faces_of.face(&self.stack, base, look.style, p);
        let face = self.font_face(base, look, p, chain);
        match cps {
            [_] => face,
            _ => self.cluster_face(cps, face),
        }
    }

    /// [`Renderer::face_for`]'s base in an alternative font or under
    /// Fraktur, given `chain`, the table's face for it.
    #[inline(never)]
    fn font_face(
        &mut self,
        base: Codepoint,
        look: TextFace,
        p: Presentation,
        chain: FaceIx,
    ) -> FaceIx {
        let colour = match p {
            Presentation::Text => 0,
            Presentation::Colour => 1,
        };
        let key = (base.value(), look.code().0 * 2 + colour);
        if let Some(&face) = self.font_faces.get(&key) {
            return face;
        }
        let face = self.stack.select_font(base, look, p, chain);
        if self.font_faces.len() >= FONT_FACES_MAX {
            self.font_faces.clear();
        }
        self.font_faces.insert(key, face);
        face
    }

    /// How many faces for text in an alternative font or under Fraktur
    /// are remembered: at most `FONT_FACES_MAX`.
    pub fn font_faces_held(&self) -> Tally {
        Tally::of(self.font_faces.len())
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
    fn face_of_cell(
        &mut self,
        (term, words): (&Term<'_>, Words<'_>),
        cell: PackedCell,
        base: Codepoint,
    ) -> FaceIx {
        let style = self.face_style(cell);
        self.face_of_cell_in((term, words), cell, base, style)
    }

    /// [`Renderer::face_of_cell`] in `style`: a cell SAPV 17 slants left
    /// asks in its upright style.
    fn face_of_cell_in(
        &mut self,
        (term, words): (&Term<'_>, Words<'_>),
        cell: PackedCell,
        base: Codepoint,
        style: TextFace,
    ) -> FaceIx {
        if !cell.flags.is_marked() {
            return self.face_for(core::slice::from_ref(&base), style);
        }
        let mut cps = [base; 1 + ClusterSlot::CAPACITY];
        let mut marks = 0;
        for (slot, m) in cps[1..]
            .iter_mut()
            .zip(drawn_marks(term, words, cell).filter_map(Codepoint::new))
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

    /// The characters no face of the stack draws that the frames since the
    /// last call drew, each once ([`FaceTable::take_gaps`]): what the caller
    /// asks the system for.
    pub fn take_gaps(&mut self) -> Vec<Codepoint> {
        self.faces_of.take_gaps()
    }

    /// Where a cluster whose base is `base`, on `face`, sits in `span`: the
    /// grid phase in one cell; in two, its base's advance centred in the
    /// span -- half the slack, whole pixels and the fraction. Centred, over
    /// 13 of 14 terminals' left edge: a one-cell ⚡ in its two cells, an
    /// emoji a third of a cell narrower than two, and a CJK fallback face's
    /// ideographs all sit in the middle. An icon is left to the patcher's
    /// rules, and a base with no advance, or one far wider than the span,
    /// keeps the grid phase.
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

    /// The selection the next frame draws ([`Renderer::set_highlight`]).
    pub(crate) fn highlight(&self) -> Option<Highlight> {
        self.highlight
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

    /// **Whether a finished paragraph is drawn at its words**
    /// ([`crate::WordWrap`], the `wrap_words` setting): on, a word that does
    /// not fit drops whole to the next line and a run of the other direction
    /// the wrap would split gets lines of its own ([`crate::rebreak`]); off,
    /// every row is drawn as the terminal cut it. The next layout follows it.
    pub fn set_word_wrap(&mut self, wrap: crate::WordWrap) {
        self.word_wrap = wrap;
        self.bidi.set_word_wrap(wrap);
    }

    /// Whether a finished paragraph is drawn at its words.
    pub fn word_wrap(&self) -> crate::WordWrap {
        self.word_wrap
    }

    /// An input method's composition for the next frame, or none: drawn at
    /// its cell by the instance and CPU paths' next convert, with the cursor
    /// after it ([`Composition::cursor`]), on the live view only. The cell
    /// path draws it through [`crate::Overlay::composed`].
    ///
    /// A composition that holds a letter that takes a joining form opens
    /// the layout's joins ([`crate::bidi::Joins::Seen`]) before it is laid
    /// out, so the frame that first draws one draws it joined, as the same
    /// text written is, on every path.
    pub fn set_composition(&mut self, c: Option<Composition>) {
        if c.as_ref().is_some_and(Composition::joins) {
            self.joins_seen = crate::bidi::Joins::Seen;
        }
        self.composition = c;
    }

    /// **The predicted echoes the cell path draws on its next frame**, each
    /// a codepoint at its logical cell, or none: its layout
    /// ([`Renderer::lay_out_bidi_copied`]) reads each as written there, so a
    /// prediction's row is drawn in the order its echo will be. The
    /// instance and CPU paths draw no prediction, and their layout reads
    /// none.
    pub fn set_predicted(&mut self, p: impl IntoIterator<Item = (Row, Col, Codepoint)>) {
        let mut p = p.into_iter().peekable();
        if self.beside.is_none() && p.peek().is_none() {
            return;
        }
        let beside = self.beside.get_or_insert_with(Box::default);
        beside.predicted.clear();
        beside.predicted.extend(p);
    }

    /// **What the layout of the window `back` lines up reads as written**:
    /// the composition, on the live view and on a row of the main display,
    /// in the rendition of the cell it starts on and over the columns its
    /// line is drawn in ([`Composition::width_on`]), and, `with` the cell
    /// path's, each predicted echo on a single-width row.
    ///
    /// Inlined into each of its two callers: left to the compiler with two,
    /// `btop`'s frames retired 0.06% more instructions with nothing
    /// written.
    #[inline(always)]
    fn write_over(&mut self, term: &Term<'_>, back: Rows, with: Predicted) {
        let predicting = match self.beside.as_deref_mut() {
            Some(b) => {
                b.written.clear();
                with == Predicted::Read && !b.predicted.is_empty()
            }
            None => false,
        };
        // Nearly every frame: nothing to write, and no row is read.
        if self.composition.is_none() && !predicting {
            return;
        }
        let back = term.looking_back(back);
        if back.index() != 0 {
            return;
        }
        let vp = term.view(back);
        let beside = self.beside.get_or_insert_with(Box::default);
        // A predicted echo is not written over a line that is not
        // single-width.
        let single = |r: Row| r.index() < vp.rows().index() && !vp.rendition(r).is_double();
        if let Some(c) = self
            .composition
            .as_ref()
            .filter(|c| c.row.index() < vp.rows().index())
        {
            let style = vp
                .row(c.row)
                .get(c.col.index())
                .map_or(PackedCell::EMPTY.style, |cell| cell.style);
            let cells = c.written(c.width_on(&vp), style);
            beside.written.write(&vp, c.row, c.col, &cells);
        }
        if predicting {
            let Beside {
                written, predicted, ..
            } = &mut **beside;
            for &(r, col, cp) in predicted.iter().filter(|&&(r, _, _)| single(r)) {
                let cell = PackedCell {
                    content: baud_domain::Content::codepoint(cp),
                    style: PackedCell::EMPTY.style,
                    flags: baud_domain::CellFlags::NONE,
                };
                written.write(&vp, r, col, &[cell]);
            }
        }
    }

    /// **The find overlay for the next frame**, or none: every match but
    /// the current one washed in the palette's yellow, the current one
    /// left to the selection the driver sets ([`Renderer::set_highlight`]),
    /// and the field drawn over its cells in the selection's colours with
    /// its caret. The instance and CPU paths draw it in their convert; the
    /// cell path appends [`Renderer::find_solids`] to its records with the
    /// content, and draws the field and its caret among baud's veils
    /// ([`Renderer::veils_onto`]), over the images.
    pub fn set_find(&mut self, term: &Term<'_>, look: Option<FindLook>) {
        self.find_wash = self
            .theme
            .entry(MARK_SLOT, |ch| term.palette(ch))
            .with_alpha(Chan::from_u8(MARK_ALPHA));
        self.find = look;
        self.build_find();
    }

    /// The find marks' washes and the field's caret, the marks through this
    /// frame's order: a match on a reordered row is as many washes as the
    /// runs its cells are drawn in.
    fn build_find(&mut self) {
        let mut out = core::mem::take(&mut self.find_inst);
        out.clear();
        self.find_caret.clear();
        let look = self.find.take();
        if let Some(f) = &look {
            let (cw, ch) = (self.metrics.cell_w.value(), self.metrics.cell_h.value());
            let x = |c: Col| {
                screen(
                    i32::try_from(u32::try_from(c.index()).unwrap_or(0).saturating_mul(cw))
                        .unwrap_or(i32::MAX),
                )
            };
            let y = |r: Row| screen(i32::try_from(self.band(r).top.value()).unwrap_or(i32::MAX));
            let wash = self.find_wash;
            // A mark's line past the window is drawn on none of its rows
            // unless a display draws it on a window line; a run on the row
            // after the window's would wash the status line or the padding.
            let rows = self.rows.index();
            for m in f.marks.iter().filter(|m| m.current == Current::No) {
                for (row, at, n) in self
                    .bidi
                    .orders()
                    .line_runs(m.line, m.from, m.to)
                    .into_iter()
                    .filter(|&(row, _, _)| row.index() < rows)
                {
                    let n = u32::try_from(n.index()).unwrap_or(0);
                    out.push(solid(
                        x(at),
                        y(row),
                        Px::of(n.saturating_mul(cw)),
                        Px::of(ch),
                        wash,
                    ));
                }
            }
            let line = self.metrics.thickness.value().max(2);
            let caret = solid(
                x(f.field.caret),
                y(f.field.row),
                Px::of(line),
                Px::of(ch),
                self.defaults.1,
            );
            self.find_caret.push(caret);
        }
        self.find_inst = out;
        self.find = look;
    }

    /// **Lays out the window `back` lines into the history in the order
    /// UAX #9 gives each row**, for the frame about to be drawn, and draws
    /// the find marks and the link underline through it. [`Renderer::convert`]
    /// calls it; the cell path calls it before it reads the window
    /// ([`crate::Orders::window_kept`]) through [`Renderer::lay_out_bidi_copied`].
    /// While BDSM is reset nothing is reordered, and once the renderer has
    /// shaped a letter that takes a joining form each row's joins are kept
    /// while the row was not written: this is the layout for a caller with
    /// no compare of its own. The marks are drawn again only where the
    /// layout did not keep every row's order (`Renderer::marks_after_layout`).
    pub fn lay_out_bidi(&mut self, term: &Term<'_>, back: Rows) {
        self.lay_out_bidi_with(term, back, Predicted::Unread);
    }

    fn lay_out_bidi_with(&mut self, term: &Term<'_>, back: Rows, with: Predicted) {
        // A display ([`crate::rebreak`]) moves cells between rows with no
        // row reordered, so the marks follow it as they follow an order.
        let was = self.reordered();
        self.write_over(term, back, with);
        match self.beside.as_deref().filter(|b| !b.written.is_empty()) {
            Some(b) => self
                .bidi
                .lay_out_written(term, back, self.joins_seen, &b.written),
            None => self.bidi.lay_out_as(term, back, self.joins_seen),
        }
        self.marks_after_layout(was);
    }

    /// **The find marks and the link underline drawn again through the
    /// order just laid out**, where the frame before was drawn in another
    /// order or this one is (`was`), unless no row's maps were written
    /// since they were drawn through them ([`crate::Orders::moved_since`])
    /// and nothing else the two are drawn from moved ([`MarksKey`]): a
    /// frame that wrote nothing collects no run of either. What else moves
    /// them draws them again itself: [`Renderer::set_find`],
    /// [`Renderer::set_link`] and [`Renderer::rebuild_overlays`].
    fn marks_after_layout(&mut self, was: Reordered) {
        let now = self.reordered();
        let key = MarksKey {
            metrics: self.metrics,
            rows: self.rows,
            defaults: self.defaults,
            wash: self.find_wash,
        };
        let orders = self.bidi.orders();
        if (was, now) == (Reordered::No, Reordered::No)
            || self
                .marks_laid
                .is_some_and(|(k, laid)| k == key && !orders.moved_since(&laid))
        {
            return;
        }
        self.build_find();
        self.build_link();
        self.marks_laid = Some((key, self.bidi.orders().laid_seal()));
    }

    /// Whether the last layout drew any row in another order, or a
    /// display ([`crate::rebreak`]), which moves cells between rows with
    /// no row reordered: what the marks follow.
    fn reordered(&self) -> Reordered {
        match self.bidi.orders().any() || self.bidi.orders().display().is_some() {
            true => Reordered::Yes,
            false => Reordered::No,
        }
    }

    /// **The window laid out with BDSM reset for [`Renderer::convert`]**,
    /// the last frame's joins kept for it to find again on the rows its
    /// compare finds changed ([`crate::Reorder::keep_displayed`]), and the
    /// find marks and the link underline drawn again if the frame before
    /// was reordered or this one is: a line SCP set right to left is drawn
    /// in reverse with the mode reset, the composition's row read as its
    /// text written ([`crate::bidi::Written`]). No predicted echo is read.
    fn keep_bidi_displayed(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        changes: crate::bidi::Changes,
    ) -> crate::bidi::Rejoin {
        let was = self.reordered();
        let rejoin = match self.composition.is_some() || self.beside.is_some() {
            true => self.keep_written(term, back, changes),
            false => {
                self.bidi
                    .keep_displayed(term, back, changes, &crate::bidi::Written::default())
            }
        };
        self.marks_after_layout(was);
        rejoin
    }

    /// [`Renderer::keep_bidi_displayed`]'s layout over what is written:
    /// out of line, as [`crate::bidi::Written::write`] is, so a frame with
    /// no composition and no echo ever shown pays one test for it.
    #[inline(never)]
    fn keep_written(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        changes: crate::bidi::Changes,
    ) -> crate::bidi::Rejoin {
        self.write_over(term, back, Predicted::Unread);
        let b = self.beside.get_or_insert_with(Box::default);
        self.bidi.keep_displayed(term, back, changes, &b.written)
    }

    /// **The window laid out for the cell path**, which compares each
    /// frame's screen with the copy its last frame made (research decision
    /// D4): [`Renderer::lay_out_bidi`], except with BDSM reset once the
    /// renderer has shaped a letter that takes a joining form. Then nothing
    /// is reordered and no row is read: the last frame's joins are kept,
    /// and the answer says whether the caller finds them again on the rows
    /// its compare finds changed or on every row
    /// ([`Renderer::rejoin_bidi`]).
    pub fn lay_out_bidi_copied(
        &mut self,
        term: &Term<'_>,
        back: Rows,
    ) -> Option<crate::bidi::Rejoin> {
        match (term.bidi_support(), self.joins_seen) {
            (BidiSupport::Explicit, crate::bidi::Joins::Seen) => {
                Some(self.keep_bidi_displayed(term, back, crate::bidi::Changes::Copied))
            }
            _ => {
                self.lay_out_bidi_with(term, back, Predicted::Read);
                None
            }
        }
    }

    /// **The row a smooth scroll's slide draws above the window `back`
    /// lines into the history**, `view(back + 1)`'s first row, into `dst`
    /// as the cell shader reads it: in the order that row was drawn in when
    /// it was the window's first -- laid out over the window one line up
    /// ([`crate::Reorder::lay_out_top`]), its paragraph read across the
    /// rows below it and the context above, so a right-to-left line slides
    /// out as it was shown (`[[source: ecma48]]` 7.2.1, IMPLICIT; UAX #9
    /// L1-L2 order every display line) -- each marked cell given its base
    /// ([`crate::cellmap::copy_for_shader`]).
    pub fn copy_leaving(&mut self, term: &Term<'_>, back: Rows, dst: &mut [PackedCell]) {
        let above = Rows::of_index(back.index() + 1);
        self.lay_out_leaving(term, back);
        let Beside {
            leaving,
            leaving_cells,
            ..
        } = &mut **self.beside.get_or_insert_with(Box::default);
        let leaving = leaving.get_or_insert_with(crate::bidi::Reorder::new);
        let up = leaving.view(term, above);
        let src = up.row(Row::ZERO);
        let n = dst.len().min(src.len());
        leaving_cells.clear();
        leaving_cells.resize(n, PackedCell::EMPTY);
        leaving
            .orders()
            .permute(Row::ZERO, &src[..n], leaving_cells);
        crate::cellmap::copy_for_shader(term, leaving_cells, &mut dst[..n]);
    }

    /// The leaving row of the window `back` lines into the history laid
    /// out in the order [`Renderer::copy_leaving`] draws it.
    fn lay_out_leaving(&mut self, term: &Term<'_>, back: Rows) {
        let above = Rows::of_index(back.index() + 1);
        let reach = widest_of(self.reaches.all());
        let (wrap, joins) = (self.word_wrap, self.joins_seen);
        let beside = &mut **self.beside.get_or_insert_with(Box::default);
        let leaving = beside.leaving.get_or_insert_with(crate::bidi::Reorder::new);
        leaving.set_reach(reach);
        leaving.set_word_wrap(wrap);
        leaving.lay_out_top(term, above, joins);
    }

    /// **Where the leaving row [`Renderer::copy_leaving`] copies names its
    /// cells' ids**, in the order it is drawn: the first line of the window
    /// one line up, `back` the window's, as its layout draws it
    /// ([`crate::bidi::Orders::row_source`]). Live before a slide.
    pub fn leaving_source<'t>(
        &self,
        term: &'t Term<'_>,
        back: Rows,
    ) -> crate::frozen::RowSource<'t> {
        let above = Rows::of_index(back.index() + 1);
        match self.beside.as_deref().and_then(|b| b.leaving.as_ref()) {
            Some(l) => l.orders().row_source(term, above, Row::ZERO),
            None => crate::frozen::RowSource::One(crate::frozen::live(term)),
        }
    }

    /// **The leaving row's ids as the frame's copy names them**, `row`
    /// being that copy's leaving row after its frozen cells were lent
    /// slots (`crate::frozen`): what the overlay's walk of the row reads
    /// through the frame's table.
    pub fn leaving_lent(&mut self, row: &[PackedCell]) {
        if let Some(b) = self.beside.as_deref_mut() {
            for (c, l) in b.leaving_cells.iter_mut().zip(row) {
                c.style = l.style;
            }
        }
    }

    /// **The leaving row [`Renderer::copy_leaving`] last copied, in its
    /// drawn order and with its marks**: what the cell path's walk reads to
    /// draw by records what the map does not -- its joined, mirrored and
    /// marked cells and its ligatures -- as it does on a row of the window
    /// ([`crate::Overlay::leaving`]). None before a slide.
    pub(crate) fn leaving_cells(&self) -> Option<Vec<PackedCell>> {
        self.beside
            .as_deref()
            .filter(|b| b.leaving.is_some())
            .map(|b| b.leaving_cells.clone())
    }

    /// The window's layout and the leaving row's exchanged
    /// ([`Renderer::copy_leaving`]), so a walk or a conversion of the
    /// leaving row reads its order, joins and mirrors as row 0's; called
    /// again to put them back.
    pub(crate) fn swap_leaving_layout(&mut self) {
        if let Some(leaving) = self.beside.as_deref_mut().and_then(|b| b.leaving.as_mut()) {
            core::mem::swap(&mut self.bidi, leaving);
        }
    }

    /// The joins [`Renderer::lay_out_bidi_copied`] kept, found again: on
    /// every row for [`crate::Rejoin::Every`], else on each row `changed`
    /// says changed since the last frame, one test of the row each.
    pub fn rejoin_bidi(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        how: crate::bidi::Rejoin,
        changed: impl Fn(Row) -> bool,
    ) {
        let vp = term.view(term.looking_back(back));
        let written = self
            .beside
            .as_deref()
            .map_or(&NOTHING_WRITTEN, |b| &b.written);
        for y in 0..vp.rows().index() {
            let r = Row::from_index(y);
            if how == crate::bidi::Rejoin::Every || changed(r) {
                self.bidi.rejoin(term, &vp, r, written);
            }
        }
        self.bidi.rejoined();
    }

    /// **The first letter that takes a joining form, shaped by the cell
    /// path's walk with BDSM reset**, after the window was laid out with
    /// none seen (`before`), and so unjoined: the window laid out again,
    /// every row joined, and kept for the next frame's compare
    /// ([`Renderer::lay_out_bidi_copied`]). Once a renderer's life.
    pub fn join_after_walk(&mut self, term: &Term<'_>, back: Rows, before: crate::bidi::Joins) {
        if before == crate::bidi::Joins::Unseen
            && self.joins_seen == crate::bidi::Joins::Seen
            && term.bidi_support() == BidiSupport::Explicit
        {
            if let Some(how) = self.lay_out_bidi_copied(term, back) {
                self.rejoin_bidi(term, back, how, |_| true);
            }
        }
    }

    /// The order the last layout drew each row in.
    pub fn bidi(&self) -> &crate::bidi::Orders {
        self.bidi.orders()
    }

    /// **The window and the status line in drawn order, with their keys,
    /// lent by move** to the cell path's frame, which puts them back
    /// ([`Renderer::put_drawn_order`]): the buffers and the key that
    /// describes them go together, and a frame that never puts them back
    /// leaves none, so the next one draws every row.
    pub(crate) fn take_drawn_order(&mut self) -> crate::bidi::DrawnOrder {
        core::mem::take(&mut self.drawn_order)
    }

    /// What [`Renderer::take_drawn_order`] lent, put back.
    pub(crate) fn put_drawn_order(&mut self, drawn: crate::bidi::DrawnOrder) {
        self.drawn_order = drawn;
    }

    /// Whether this renderer has shaped a letter that takes a joining form,
    /// so a layout with BDSM reset reads rows for their joins: a caller
    /// that lays out before it draws and sees this change while it draws
    /// lays out again ([`crate::bidi::Joins`]).
    pub fn joins_seen(&self) -> crate::bidi::Joins {
        self.joins_seen
    }

    /// What the last layout read to find its answers
    /// ([`crate::Reorder::work`]): the rows whose write generation it
    /// asked, the rows it tested, and the rows it laid out again.
    pub fn bidi_work(&self) -> crate::bidi::Work {
        self.bidi.work()
    }

    /// The find overlay's marks, each match's wash, for a path that draws
    /// records of its own: with the content, under the images over the
    /// text.
    pub fn find_solids(&self) -> &[Instance] {
        &self.find_inst
    }

    /// **The link preview for the next frame**, or none: the link's cells
    /// underlined at the face's underline, in the default foreground, and
    /// its address in a bar drawn over its cells in the message pair
    /// ([`crate::message`]), as the find field is. The instance and CPU paths draw it in
    /// their convert, the bar over the find field and over the images; the
    /// cell path draws the bar through [`crate::Overlay::composed`], after
    /// the images, and appends [`Renderer::link_solids`] to its records
    /// with the content.
    pub fn set_link(&mut self, look: Option<LinkLook>) {
        self.link = look;
        self.build_link();
    }

    /// The link preview's underline, in the default foreground as it is
    /// now: again when the defaults move.
    fn build_link(&mut self) {
        let mut out = core::mem::take(&mut self.link_inst);
        out.clear();
        if let Some(l) = &self.link {
            let cw = self.metrics.cell_w.value();
            let x = |c: Col| {
                screen(
                    i32::try_from(u32::try_from(c.index()).unwrap_or(0).saturating_mul(cw))
                        .unwrap_or(i32::MAX),
                )
            };
            let (under, thick) = (self.metrics.underline.value(), self.metrics.thickness);
            let y = |r: Row| {
                screen(
                    i32::try_from(self.band(r).top.value().saturating_add(under))
                        .unwrap_or(i32::MAX),
                )
            };
            for s in &l.spans {
                for (row, at, n) in self.bidi.orders().stored_runs(s.row, s.from, s.to) {
                    let n = u32::try_from(n.index()).unwrap_or(0);
                    out.push(solid(
                        x(at),
                        y(row),
                        Px::of(n.saturating_mul(cw)),
                        thick,
                        self.defaults.0,
                    ));
                }
            }
        }
        self.link_inst = out;
    }

    /// The link preview's solids, the underline, for a path that draws
    /// records of its own: with the content, under the images over the
    /// text.
    pub fn link_solids(&self) -> &[Instance] {
        &self.link_inst
    }

    /// **The lines at rest for the next frames**: the scan's runs, whose
    /// solids [`Renderer::place_links`] builds once the frame's layout is
    /// known. Copied only when they changed, so a frame that changed nothing
    /// allocates nothing. **A change of `style` repaints every row**, as
    /// [`Renderer::set_theme`] does: the rows a line is on compare as
    /// unchanged, since no cell and no run moved. A config read again, so
    /// the whole frame.
    pub fn set_link_lines(&mut self, lines: &[LinkLine], style: LineStyle) {
        self.links.set_lines(lines);
        if self.links.set_style(style) == Set::Moved {
            self.known = Prev::Unknown;
        }
    }

    /// **The colour the lines at rest take**: `[message]`'s box colour, or,
    /// under `[link] hover = "line"`, the text's own. **A change repaints
    /// every row**, as a change of the lines' style does
    /// ([`Renderer::set_link_lines`]).
    pub fn set_line_colour(&mut self, colour: LineColour) {
        if self.links.set_colour(colour) == Set::Moved {
            self.known = Prev::Unknown;
        }
    }

    /// **The struck links for the next frames**: the scan's runs at rest,
    /// then the ones the gesture's modifier strikes over the link under the
    /// pointer, each a run of one rendition. Copied only when they changed,
    /// so a frame with none allocates nothing. Nothing of them reaches a
    /// cell: a struck link selects and copies as it is written.
    pub fn set_link_strikes(&mut self, at_rest: &[LinkLine], held: &[LinkLine]) {
        self.links.set_strikes(at_rest, held);
    }

    /// `[link] missing_color`, read at draw against the colours in force.
    /// **A change repaints every row**, as a change of the lines' style
    /// does ([`Renderer::set_link_lines`]): a struck cell lent no slot -- one
    /// the selection covers -- keeps its id, so only its strike moves
    /// colour, and its row compares as unchanged.
    pub fn set_link_missing(&mut self, paint: MissingPaint) {
        if self.links.set_missing(paint) == Set::Moved {
            self.known = Prev::Unknown;
        }
    }

    /// `[link] missing_color`, as [`Renderer::set_link_missing`] last set
    /// it.
    pub(crate) fn missing_paint(&self) -> MissingPaint {
        self.links.missing()
    }

    /// **The cells of a copy's first `rows` rows each struck link's text
    /// covers**, as [`Renderer::lend`] takes them, taken out of the renderer
    /// for the lending and put back after it ([`Renderer::put_struck`]): by
    /// visual column for a copy in drawn order ([`CopyOrder::Visual`], this
    /// path's own), by stored column for one in the core's
    /// ([`CopyOrder::Stored`], the cell path's). A selected cell is left
    /// out, as it keeps the selection's colours, and so is every cell while
    /// `missing_color = "none"`, which changes no colour. `view` is the
    /// window `back` lines up the strikes were read on. **Kept** while
    /// nothing they are found from moved ([`StruckCells`]): a caret frame
    /// over a struck link finds none. Their generation
    /// ([`Renderer::struck_gen`]) is moved here, before they are handed
    /// out, where a find gave other cells. **Lost** -- taken with none put
    /// back since, by a second take or a panic between a take and its put
    /// -- they are found again from nothing under a generation the process
    /// gave none before, so no generation read while the lost cells were
    /// held names what is found now.
    pub fn take_struck(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        view: &Viewport<'_>,
        rows: Rows,
        order: CopyOrder,
    ) -> StruckCells {
        let mut struck = match self.struck.take() {
            Some(struck) => struck,
            None => {
                self.struck_gen = StruckGen::fresh();
                StruckCells::new()
            }
        };
        let gens = term.row_gens(back);
        let now = StruckKey {
            inputs: self.links.moves(),
            highlight: self.highlight,
            order,
            rows,
            at: gens.window(),
        };
        let moved = struck.hold(
            now,
            &gens,
            term.sealed(),
            self.links.strikes(),
            self.links.missing(),
            self.bidi.orders(),
            view,
        );
        if struck.found() == StruckFound::Again {
            self.struck_finds.bump();
        }
        if moved == StruckMoved::Moved {
            self.struck_gen = StruckGen::fresh();
        }
        struck
    }

    /// **The struck cells put back** once the lending that read them is
    /// done ([`Renderer::take_struck`]), so the next frame keeps them. Put
    /// back once a take: a second put, with the renderer holding cells
    /// already, is a driver's fault, asserted in a debug build.
    pub fn put_struck(&mut self, struck: StruckCells) {
        debug_assert!(
            self.struck.is_none(),
            "the struck cells put back twice for one take"
        );
        self.struck = Some(struck);
    }

    /// **How many times the struck cells were found again** since the
    /// renderer was made ([`StruckCells`]): a caret frame over struck links
    /// nothing moved finds none, on a row drawn in another order as on one
    /// in its own.
    pub fn struck_finds(&self) -> Tally {
        self.struck_finds
    }

    /// **The struck cells' generation** ([`StruckGen`]): one the process
    /// gave no other renderer, moved when a find gives other cells than
    /// the one before it -- a strike moved to another row, or its cells
    /// found at other columns -- and when they were lost
    /// ([`Renderer::take_struck`]), and never on a find that gives the same
    /// cells nor while they are kept. It reads true while the cells are
    /// out for a lending, as it is kept here and moved before they go. It
    /// moves only on a frame that takes the struck cells: the cell path
    /// takes them every frame, the CPU path only on a frame it lends -- a
    /// strike set, or a window over frozen history -- so there, with
    /// neither, it names the cells its last lending took. A reader that
    /// keeps what it made from the struck cells keys on it, as one keeps
    /// rows on their generations; [`Renderer::struck_finds`] moves on every
    /// find, the same cells or not.
    pub fn struck_gen(&self) -> StruckGen {
        self.struck_gen
    }

    /// **The link under the pointer for the next frame**, or none, copied
    /// into the buffer the last one left where it is another.
    pub fn set_link_hover(&mut self, hover: Option<&LinkHover>) {
        self.links.set_hover(hover);
    }

    /// **The solids and fields of the frame's links, once its layout is
    /// known**: each line at the cells the row's order draws it on, at rest
    /// in the box's colour as this frame resolves it ([`LineColour`]) and,
    /// under the pointer, in the colour its rendition resolves to now, so a
    /// palette or a video that moved moves it; none where that text does
    /// not draw; and the look of the link under the pointer. The
    /// instance and CPU paths call it from their convert, the cell path
    /// before its walk, which reads the look's fields as empty; `view` is
    /// the window as that path's cells name their renditions. **The solids
    /// are kept** while nothing they are drawn from moved (`links`); the
    /// look's fields read cells, and are built every frame.
    pub fn place_links(&mut self, term: &Term<'_>, view: &Viewport<'_>) {
        self.place_links_with(term, view, Force::No, |y, _, l| {
            view.row(y)
                .get(l.index())
                .copied()
                .unwrap_or(PackedCell::EMPTY)
        });
    }

    /// [`Renderer::place_links`], `under` naming the cell a field's ground
    /// is read from: its row, visual column and logical column; `force`
    /// places the solids again whatever their key says.
    fn place_links_with(
        &mut self,
        term: &Term<'_>,
        view: &Viewport<'_>,
        force: Force,
        under: impl Fn(Row, Col, Col) -> PackedCell,
    ) {
        let now = SolidsKey {
            inputs: self.links.moves(),
            colours: self.style_inputs(term),
            hilite: self.field_hilite,
            message: self.message_colours,
            metrics: self.metrics,
        };
        let order = self.own_order();
        if let Placing::Place(mut p) = self.kept_solids.hold(now, order, self.bidi.orders(), force)
        {
            self.place_solids(term, p.solids());
            crate::counters::add(
                Counter::LinkInstancesBuilt,
                Site::Links,
                Tally::of(p.solids().len()),
            );
            self.kept_solids.placed(p);
        }
        let mut bars = core::mem::take(&mut self.hover_bars);
        let mut spare = core::mem::take(&mut self.hover_spare);
        spare.append(&mut bars);
        // The link under the pointer drawn as a box or as its text: fields
        // whose ground is read from the cells under them.
        if let Some(h) = self
            .links
            .hover()
            .filter(|h| matches!(h.paint, HoverPaint::Box | HoverPaint::Text))
        {
            let words = self.frame_held();
            let orders = self.bidi.orders();
            for s in &h.spans {
                each_run(orders, (s.row, s.from, s.to), |row, at, n| {
                    hover_fields(
                        (term, words.words()),
                        orders,
                        view,
                        (row, at, n),
                        h.paint,
                        &under,
                        (&mut bars, &mut spare),
                    );
                });
            }
        }
        self.hover_bars = bars;
        self.hover_spare = spare;
    }

    /// **Whether every row a link's solid is drawn on is drawn in its own
    /// order, with no display held**: the rows of the lines at rest, of the
    /// strikes and of `hover = "line"`'s spans. One question a row, and
    /// none where no row of the window is reordered.
    fn own_order(&self) -> OwnOrder {
        let orders = self.bidi.orders();
        if orders.display().is_some() {
            return OwnOrder::NotEvery;
        }
        if !orders.any() {
            return OwnOrder::Every;
        }
        let hovered = self
            .links
            .hover()
            .filter(|h| h.paint == HoverPaint::Line)
            .into_iter()
            .flat_map(|h| h.spans.iter().map(|s| s.row));
        let mut rows = self
            .links
            .lines()
            .iter()
            .chain(self.links.strikes())
            .map(|l| l.row)
            .chain(hovered);
        match rows.any(|r| orders.reordered(r)) {
            true => OwnOrder::NotEvery,
            false => OwnOrder::Every,
        }
    }

    /// **The solids of the lines at rest, the strikes and `hover = "line"`,
    /// appended to `out`**: from the driver's links, the frame's colours,
    /// the cell geometry and the rows' order, and from no cell.
    fn place_solids(&self, term: &Term<'_>, out: &mut Vec<Instance>) {
        let cw = self.metrics.cell_w.value();
        let thick = self.metrics.thickness;
        let under_y = self.metrics.underline.value();
        let x = |c: Col| {
            i32::try_from(u32::try_from(c.index()).unwrap_or(0).saturating_mul(cw))
                .unwrap_or(i32::MAX)
        };
        let y = |r: Row| {
            screen(
                i32::try_from(self.band(r).top.value().saturating_add(under_y)).unwrap_or(i32::MAX),
            )
        };
        // No row reordered and none re-broken: a run is its own cells, and
        // no list is built for it.
        let plain = !self.bidi.orders().any() && self.bidi.orders().display().is_none();
        let mut last: Option<(baud_domain::StyleWord, Option<Argb>)> = None;
        // The box's colour as this frame resolves it, as the box itself
        // takes it: `"none"` reads the selection's colours now.
        let rest = match self.links.colour() {
            LineColour::Box => Some(
                self.message_rendition(self.resolve_slot(term, Default::default()))
                    .bg,
            ),
            LineColour::Text => None,
        };
        let style = self.links.style();
        for l in self.links.lines() {
            let colour = match last {
                Some((word, c)) if word == l.word => c,
                _ => {
                    let c = self.line_colour(term, l.word);
                    last = Some((l.word, c));
                    c
                }
            };
            let Some(colour) = colour else { continue };
            // At rest, the box's colour; the link under the pointer keeps the
            // line its look has always had, in the text's colour.
            let colour = match (self.hovered_line(l), rest) {
                (false, Some(rest)) => rest,
                _ => colour,
            };
            if style == LineStyle::Off {
                continue;
            }
            let mut push = |row: Row, at: Col, n: usize| {
                let w = u32::try_from(n).unwrap_or(0).saturating_mul(cw);
                line_solids(out, style, (x(at), y(row)), (w, thick), colour);
            };
            match plain {
                true => push(
                    l.row,
                    l.from,
                    l.to.index().saturating_sub(l.from.index()) + 1,
                ),
                false => {
                    for (row, at, n) in self.bidi.orders().stored_runs(l.row, l.from, l.to) {
                        push(row, at, n.index());
                    }
                }
            }
        }
        // **The struck links**: a strike at the face's strike position over
        // each run, in the colour its text is drawn in -- `missing_color`
        // where that reads, through the slot each struck cell is lent
        // (`crate::frozen`, [`Renderer::take_struck`]), so the text itself
        // is drawn by the cells, as any other. **And the live line under it**:
        // `[link] line`'s style and thickness, in the box's colour at rest
        // and, under the pointer -- the gesture's strikes, or a run the
        // link under the pointer covers -- in the text's, as a live line
        // takes them; at `Metrics::struck_underline`, which keeps it clear of
        // the strike. None with `line = "off"`, and none over a cell a
        // program underlines itself, whose own line is drawn.
        if !self.links.strikes().is_empty() {
            let strike_y = self.metrics.strike.value();
            let sy = |r: Row| {
                screen(
                    i32::try_from(self.band(r).top.value().saturating_add(strike_y))
                        .unwrap_or(i32::MAX),
                )
            };
            let struck_under = self.metrics.struck_underline().value();
            let uy = |r: Row| {
                screen(
                    i32::try_from(self.band(r).top.value().saturating_add(struck_under))
                        .unwrap_or(i32::MAX),
                )
            };
            let mut last: Option<(baud_domain::StyleWord, Option<Argb>)> = None;
            for l in self.links.strikes() {
                let colour = match last {
                    Some((word, c)) if word == l.word => c,
                    _ => {
                        let resolved = self.resolve_slot(term, l.word);
                        let c = self.drawn_colour(self.missing_rendition(term, resolved));
                        last = Some((l.word, c));
                        c
                    }
                };
                let Some(colour) = colour else { continue };
                let under = self.links.held().contains(l) || self.hovered_line(l);
                let line = match (under, rest) {
                    (false, Some(rest)) => rest,
                    _ => colour,
                };
                let own = l.word.attrs().only(Attrs::UNDERLINES) != Attrs::NONE;
                for (row, at, n) in self.bidi.orders().stored_runs(l.row, l.from, l.to) {
                    let w = u32::try_from(n.index()).unwrap_or(0).saturating_mul(cw);
                    out.push(solid(screen(x(at)), sy(row), Px::of(w), thick, colour));
                    if !own {
                        line_solids(out, style, (x(at), uy(row)), (w, thick), line);
                    }
                }
            }
        }
        // The link under the pointer drawn as a line.
        if let Some(h) = self.links.hover().filter(|h| h.paint == HoverPaint::Line) {
            let pair = self.message_rendition(self.resolve_slot(term, Default::default()));
            for s in &h.spans {
                for (row, at, n) in self.bidi.orders().stored_runs(s.row, s.from, s.to) {
                    let w = u32::try_from(n.index()).unwrap_or(0).saturating_mul(cw);
                    out.push(solid(screen(x(at)), y(row), Px::of(w), thick, pair.bg));
                }
            }
        }
    }

    /// Whether line `l` lies under the link the pointer is over.
    fn hovered_line(&self, l: &LinkLine) -> bool {
        self.links.hover().is_some_and(|h| {
            h.spans
                .iter()
                .any(|s| s.row == l.row && s.from <= l.from && l.to <= s.to)
        })
    }

    /// The colour a line over a cell of rendition `word` is drawn in under
    /// the pointer, as an underline of that cell would be; none where its
    /// text does not draw, where no line is drawn at rest either.
    fn line_colour(&self, term: &Term<'_>, word: baud_domain::StyleWord) -> Option<Argb> {
        let d = self.resolve_slot(term, word).drawn(Swaps {
            pictured: crate::picture::Pictured::No,
            line: Line::No,
            selected: Selected::No,
            hilite: self.field_hilite,
            block: BlockCursor::No,
            blink: BlinkPhases::SHOWN,
            cursor: Chan::from_u8(0xFF),
        });
        d.text.draws().then(|| d.rule(d.fg))
    }

    /// The colour `r`'s text is drawn in, as a line over it would be; none
    /// where its text does not draw.
    fn drawn_colour(&self, r: Resolved) -> Option<Argb> {
        let d = r.drawn(Swaps {
            pictured: crate::picture::Pictured::No,
            line: Line::No,
            selected: Selected::No,
            hilite: self.field_hilite,
            block: BlockCursor::No,
            blink: BlinkPhases::SHOWN,
            cursor: Chan::from_u8(0xFF),
        });
        d.text.draws().then(|| d.rule(d.fg))
    }

    /// The lines at rest, and `hover = "line"`'s, for a path that draws
    /// records of its own: with the content, moved by a slide with the text
    /// they lie under.
    pub fn link_lines_lowered(&self, pan: Px, out: &mut Vec<Instance>) {
        let down = i16::try_from(pan.value()).unwrap_or(i16::MAX);
        out.extend(self.kept_solids.solids().iter().map(|g| Instance {
            y: Screen(g.y.0.saturating_add(down)),
            ..*g
        }));
    }

    /// The fields the link under the pointer is drawn as, with the cell
    /// each reads its ground from, copied out for a caller to read: every
    /// path draws them where they are kept ([`Renderer::veils_onto`] on
    /// the cell path).
    pub fn hover_bars(&self) -> Vec<(Composition, PackedCell)> {
        self.hover_bars
            .iter()
            .map(|b| (b.comp.clone(), b.under))
            .collect()
    }

    /// **The hovered link's fields as the cells they cover**, each its row,
    /// its first visual column and its width on a screen `cols` wide: what
    /// the cell path's walk reads as empty (`crate::overlay`), as the
    /// instance path blanks those cells in its convert, so a record the
    /// walk would draw under a field with no page of its own -- `"text"`'s
    /// -- does not show through it.
    pub(crate) fn hover_spans(&self, cols: Cols) -> impl Iterator<Item = (Row, Col, Cols)> + '_ {
        self.hover_bars.iter().map(move |b| {
            let (start, end) = b.comp.columns(cols);
            (
                b.comp.row,
                Col::at_index(start),
                Cols::of_index(end.saturating_sub(start)),
            )
        })
    }

    /// **`cursor`, unless a message covers its cell.** A block drawn over
    /// the address would hide a letter of it, and a program chooses both
    /// where the cursor stands and, with `OSC 12`, what it is filled with, so
    /// the frame draws no cursor there while the message is up: the address
    /// is what a person checks before a click. Every path takes its cursor
    /// through this ([`Renderer::convert`], the cell path's `cursor_at`).
    pub fn clear_of_messages(
        &self,
        term: &Term<'_>,
        cursor: Option<(Row, Col, CursorStyle)>,
    ) -> Option<(Row, Col, CursorStyle)> {
        let (r, c, style) = cursor?;
        // Past the screen's rows is the status line's cursor: no message lies
        // there, and no row of the screen answers for it.
        let screen = term.viewport();
        let wide = r.index() < screen.rows().index()
            && screen
                .row(r)
                .get(c.index() + 1)
                .is_some_and(|n| n.content.is_wide_tail());
        let v = self.bidi.orders().visual_col(r, c).index();
        let last = v + usize::from(wide);
        let covers = |f: &crate::find::Field| {
            f.row == r
                && f.hints
                    .iter()
                    .any(|h| h.has(crate::overlay::Hints::MESSAGE))
                && f.col.index() <= last
                && v < f.col.index() + f.text.len()
        };
        let bar = self.link.as_ref().is_some_and(|l| covers(&l.bar));
        let tip = self
            .tip
            .as_ref()
            .is_some_and(|t| t.lines.iter().any(covers));
        (!bar && !tip).then_some((r, c, style))
    }

    /// **The rendition `hover = "text"` draws a cell of rendition `under`
    /// in**: the message pair's box colour as its text on the cell's own
    /// ground, in the colours in force, where that reads
    /// [`crate::LINK_TEXT_CONTRAST`]; elsewhere the cell as it is.
    pub(crate) fn link_text_rendition(&self, under: Resolved) -> Resolved {
        let ground = under
            .drawn(Swaps {
                pictured: crate::picture::Pictured::No,
                line: Line::No,
                selected: Selected::No,
                hilite: self.field_hilite,
                block: BlockCursor::No,
                blink: BlinkPhases::SHOWN,
                cursor: Chan::from_u8(0xFF),
            })
            .bg;
        let page = self.message_rendition(under).bg;
        let opaque = ground.with_alpha(Chan::from_u8(0xFF));
        match crate::color::contrast_of(page, opaque) >= crate::LINK_TEXT_CONTRAST {
            true => Resolved {
                shapes: under.shapes,
                ..Resolved::fixed(ground, page)
            },
            false => under,
        }
    }

    /// **The rendition a struck link's cell of rendition `under` is drawn
    /// in**: `under` with only its text in `[link] missing_color` -- colour
    /// 8 through the live palette, so an `OSC 4` that moved it is read here
    /// -- where that reads [`crate::MISSING_CONTRAST`] against the page the
    /// cell is drawn on: solid, or at its alpha over the window's material
    /// as the glass floor reads it. Its page, its shapes and its underline
    /// are its own, so a struck cell on the glass is as dim as its
    /// neighbours and a program's `SGR 58` colour stays. Elsewhere, and for
    /// `"none"`, the cell as it is.
    pub(crate) fn missing_rendition(&self, term: &Term<'_>, under: Resolved) -> Resolved {
        let text = match self.links.missing() {
            MissingPaint::Kept => return under,
            MissingPaint::Colour(c) => c,
            MissingPaint::Gray => {
                let eight = baud_domain::StyleWord::DEFAULT
                    .with_fg(baud_domain::ColorWord::indexed(Chan::from_u8(8)));
                self.theme
                    .resolve(
                        eight,
                        Video::Normal,
                        |ch| term.palette(ch),
                        |d| term.dynamic_color(d),
                    )
                    .0
            }
        };
        let drawn = under.drawn(Swaps {
            pictured: crate::picture::Pictured::No,
            line: Line::No,
            selected: Selected::No,
            hilite: self.field_hilite,
            block: BlockCursor::No,
            blink: BlinkPhases::SHOWN,
            cursor: Chan::from_u8(0xFF),
        });
        // Text that does not draw -- concealed, a blink's off phase -- is
        // not given a colour to draw in.
        if !drawn.text.draws() {
            return under;
        }
        let text = text.with_alpha(Chan::from_u8(0xFF));
        let page = drawn.bg;
        let reads = crate::color::contrast_over(
            text,
            page.with_alpha(Chan::from_u8(0xFF)),
            page.a(),
            self.theme.backdrop,
        );
        if reads < crate::MISSING_CONTRAST {
            return under;
        }
        let curves = Curves::of(curve_row(text), curve_row(under.bg));
        Resolved {
            fg: text,
            glass: text,
            line: text,
            curves,
            glass_curves: curves,
            line_curves: curves,
            ..under
        }
    }

    /// **The hover tip for the next frame**, or none: a word's meaning,
    /// its lines fields in the selection's colours over the main
    /// display's last rows (`tip::tip_look`). The instance and CPU paths
    /// draw it in their convert over the link preview's bar and the
    /// images; the cell path draws its lines among baud's veils
    /// ([`Renderer::veils_onto`]) after the images, as it draws the bar.
    pub fn set_tip(&mut self, look: Option<crate::tip::TipLook>) {
        self.tip = look;
    }

    /// **The colours a message is drawn in** ([`crate::message`]), which a
    /// field whose hints name [`crate::Hints::MESSAGE`] reads at draw.
    /// **A change repaints every row**, as a change of the lines' style
    /// does ([`Renderer::set_link_lines`]): the lines at rest are drawn in
    /// the box's colour, and their rows compare as unchanged.
    pub fn set_message_colours(&mut self, colours: crate::message::MessageColours) {
        if self.message_colours != colours {
            self.message_colours = colours;
            self.known = Prev::Unknown;
        }
    }

    /// **The selection drawn in the message pair for the next frames**, or
    /// its own colours: the copy signal's pulse. Every path reads the
    /// selection's colours through [`Renderer::hilite`], and a change of
    /// them repaints every row, so it costs a frame to start and one to
    /// end. A field of baud's reads [`Renderer::field_hilite`], so the find
    /// field, the link preview's bar and the notice do not flash with it.
    pub fn set_selection_pulse(&mut self, pulse: crate::message::SelectionPulse) {
        self.selection_pulse = pulse;
    }

    /// The rendition a field in the message pair is drawn in, over `under`,
    /// the default cell as the frame's table resolved it: the selection's
    /// colours on it are what `"none"` names, read at draw, so the pair is
    /// checked against the colours in force.
    pub(crate) fn message_rendition(&self, under: Resolved) -> Resolved {
        let selected = under.drawn(Swaps {
            pictured: crate::picture::Pictured::No,
            line: Line::No,
            selected: Selected::Yes,
            hilite: self.field_hilite,
            block: BlockCursor::No,
            blink: BlinkPhases::SHOWN,
            cursor: Chan::from_u8(0xFF),
        });
        let pair = self.message_colours.in_force(crate::message::Pair {
            page: selected.bg,
            text: selected.fg,
        });
        Resolved::fixed(pair.page, pair.text)
    }

    /// **The embedder's notice for the next frames**, or none: `baud-app`'s
    /// config reload says here what a file it read again could not act on.
    /// A row of its own, below the main display and the status line, which
    /// [`Renderer::fit`] adds while one is set -- the embedder gives the
    /// terminal one row fewer -- and which every path draws as a field
    /// across the width: the instance and CPU paths
    /// in their convert, the cell path among baud's veils
    /// ([`Renderer::veils_onto`]) over a blank row of its window copy
    /// ([`Renderer::notice_shown`]). In the message pair
    /// ([`crate::message`]), so a program cannot make it unreadable.
    pub fn set_notice(&mut self, text: Option<Vec<Codepoint>>) {
        self.notice = text;
    }

    /// The rows the notice takes: one while it is set.
    pub fn notice_rows(&self) -> Rows {
        match self.notice {
            Some(_) => Rows::of_index(1),
            None => Rows::ZERO,
        }
    }

    /// The notice as a field on the last row, padded or cut to the width,
    /// once [`Renderer::fit`] has made that row; none otherwise.
    fn notice_field(&self) -> Option<crate::find::Field> {
        let (text, row) = self.notice_at()?;
        let cols = self.cols.index();
        Some(crate::find::Field {
            row,
            col: Col::ZERO,
            hints: vec![crate::overlay::Hints::MESSAGE; cols],
            caret: Col::at_index(cols),
            text: veils::notice_text(text, self.cols).collect(),
        })
    }

    /// The notice's text and the last row, its own, once
    /// [`Renderer::fit`] has made that row; none otherwise.
    fn notice_at(&self) -> Option<(&[Codepoint], Row)> {
        let text = self.notice.as_deref()?;
        let row = self.rows.index().checked_sub(1)?;
        (row >= self.main.index()).then_some((text, Row::from_index(row)))
    }

    /// **The row the notice is drawn on**, while one shows: the cell
    /// path blanks that row of its window copy under the notice's field.
    pub fn notice_shown(&self) -> Option<Row> {
        self.notice_at().map(|(_, row)| row)
    }

    /// The notice as the cell path draws it, over the blank row the window
    /// copy holds after the main display and the status line; none while
    /// no notice is set.
    pub fn notice_bar(&self) -> Option<Composition> {
        self.notice_field().map(|f| f.composition())
    }

    /// **The frame's veils, as their key reads them** (`veils`): each
    /// where this renderer keeps it, the live screen, and the rest of what
    /// their records are drawn from, the rendition table as `styles` stands
    /// and the width `cols` among it.
    fn veils_now<'s>(
        &'s self,
        term: &'s Term<'_>,
        (styles, cols): (StyleStamp, Cols),
    ) -> VeilNow<'s> {
        VeilNow {
            bars: &self.hover_bars,
            find: self.find.as_ref().map(|f| &f.field),
            link_bar: self.link.as_ref().map(|l| &l.bar),
            tips: self.tip.as_ref().map_or(&[][..], |t| &t.lines[..]),
            notice: self.notice_at(),
            screen: term.viewport(),
            orders: self.bidi.orders(),
            rest: VeilRest {
                cols,
                main: self.main,
                metrics: self.metrics,
                selection: self.highlight,
                blink: self.blink,
                video: self.video,
                hilite: self.field_hilite,
                message: self.message_colours,
                styles,
                clusters: term.cluster_epoch(),
                atlas: self.atlas.stats().grown,
            },
        }
    }

    /// **baud's veils over the cell path's frame** (FS-C16): the fields of
    /// the link under the pointer, the find field and its caret, the link
    /// preview's bar, the hover tip's lines and the notice, each its cells
    /// masked off the shader on a screen `cols` wide and its records
    /// appended, in that order, read through `styles`, the set's table,
    /// whose stamp is `stamp`. Composed again only when something they are
    /// drawn from moved ([`Veiled`]); a frame whose veils hold appends the
    /// records the last one composed, and allocates nothing for them. No
    /// veil reads the input method's composition, which the caller draws
    /// itself.
    pub fn veils_onto(
        &mut self,
        term: &Term<'_>,
        (styles, stamp): (&[Resolved], StyleStamp),
        cols: Cols,
        (mask, records): (&mut [u8], &mut Vec<Instance>),
    ) {
        let mut veils =
            core::mem::replace(&mut self.cell_veils, Veiled::reaching(VeilReach::Every));
        veils.hold(&self.veils_now(term, (stamp, cols)));
        for (_, row, start, end) in veils.spans() {
            for x in start.index()..end.index() {
                if let Some(slot) = mask.get_mut(row.index() * cols.index() + x) {
                    *slot |= crate::overlay::MASK_RECORD;
                }
            }
        }
        let caret = core::mem::take(&mut self.find_caret);
        veils.records(
            Force::No,
            &caret,
            |v, out| {
                crate::overlay::composition_records(
                    self,
                    term,
                    (&v.comp, crate::overlay::Laid::OnScreen),
                    v.under,
                    styles,
                    None,
                    cols,
                    LineRendition::Single,
                    Px::of(0),
                    out,
                );
            },
            records,
        );
        (self.find_caret, self.cell_veils) = (caret, veils);
    }

    /// **Every veil composed** over this renderer's life, on every path: a
    /// frame whose veils and what they are drawn from hold composes none
    /// (FS-C16).
    pub fn veils_composed(&self) -> Tally {
        let mut n = self.cpu_veils.composed();
        n.add(self.cell_veils.composed());
        n
    }

    /// The cursor trail for the next frame, or none: a fading ghost
    /// sweeping from where the cursor was to where it is, the quad whose
    /// corners chase the cursor's, the box swept from a tail chasing it, or
    /// an effect riding the quad. Every path draws it -- the CPU compositor
    /// in `paint`, the instance path from the instance list, the cell path
    /// from [`Renderer::trail`] appended to its records. The driver hands a
    /// ghost's `progress` or a smear's corners, each computed from the display
    /// link's timestamp, and this only draws them, so the frame is a pure
    /// function of the value and the pixel oracle can compare two GPU arms
    /// on it.
    pub fn set_trail(&mut self, look: Option<TrailLook>) {
        self.asked_trail = look.map(|l| match l {
            TrailLook::Ghost(t) => {
                Drawn::Ghost(trail::ghost(t, self.metrics.cell_w, self.metrics.cell_h))
            }
            TrailLook::Smear(s) => Drawn::Smear(s),
            TrailLook::Vfx(v) => Drawn::Vfx(v, Px::of(0), Px::of(0)),
        });
        self.choose_trail();
    }

    /// **The trail this frame draws**: the splash's plasma while it
    /// burns, since a frame has one effect's slot on every GPU path; then
    /// the fireworks while they burn; else the driver's, unless one lit
    /// under either is still in flight -- held back until it lands, so it
    /// never appears halfway.
    fn choose_trail(&mut self) {
        let plasma = self.splash_vfx();
        let fireworks = self.fireworks.iter().any(Option::is_some);
        if (plasma.is_some() || fireworks) && self.asked_trail.is_some() {
            self.held_trail = HeldTrail::Yes;
        } else if self.asked_trail.is_none() {
            self.held_trail = HeldTrail::No;
        }
        self.trail = match (plasma, fireworks, self.held_trail) {
            (Some((v, _)), _, _) => Some(Drawn::Vfx(v, Px::of(0), Px::of(0))),
            (None, true, _) => Some(Drawn::Fireworks(
                crate::fireworks::Stamp::of(&self.fireworks),
                Px::of(0),
                Px::of(0),
            )),
            (None, false, HeldTrail::Yes) => None,
            (None, false, HeldTrail::No) => self.asked_trail,
        };
        self.build_trail();
    }

    /// **The fireworks for the next frame** (`crate::fireworks`): each
    /// show the driver has in flight, the older first, at most two -- a
    /// third would be a request while two burn, which the driver's hold
    /// folds into the newer. Every path draws them in the effect's slot,
    /// which they hold while any burns, a trail lit meanwhile held until
    /// it lands as under the splash: the CPU path from their model, the
    /// GPU paths from [`Renderer::trail`]'s records and
    /// [`Renderer::fireworks_uniform`]. Their stars' rows are the damage,
    /// as an effect's are.
    pub fn set_fireworks(&mut self, shows: &[crate::fireworks::Show]) {
        let next = match shows {
            [] => [None, None],
            [one] => [Some(*one), None],
            [.., a, b] => [Some(*a), Some(*b)],
        };
        if next == self.fireworks && !next.iter().any(Option::is_some) {
            return;
        }
        self.fireworks = next;
        self.choose_trail();
    }

    /// **What the GPU's fireworks fragment function reads beyond the
    /// records** while they hold the effect's slot: where the frame sits in
    /// the target, and the hole round the cursor
    /// ([`crate::fireworks::uniform`]); none otherwise.
    pub fn fireworks_uniform(
        &self,
        origin: [f32; 2],
    ) -> Option<[f32; crate::fireworks::UNIFORM_FLOATS]> {
        let (centre, radius) = self.fireworks_hole()?;
        Some(crate::fireworks::uniform(origin, centre, radius))
    }

    /// The hole round the cursor the fireworks are drawn without, while
    /// they hold the slot: the newest show's centre, and its radius in
    /// pixels.
    fn fireworks_hole(&self) -> Option<(trail::Pt, f32)> {
        match &self.trail {
            Some(Drawn::Fireworks(..)) => self
                .fireworks
                .iter()
                .rev()
                .flatten()
                .next()
                .map(|s| (s.hole, crate::fireworks::HOLE * s.scale)),
            _ => None,
        }
    }

    /// **The launch splash** for the frames to come, or none (`splash.rs`):
    /// the lockup in the theme's half, centred, at the size its first frame
    /// chose, its records drawn with baud's own overlays on every path --
    /// in [`Renderer::instances`] after the find field and the link bar,
    /// the CPU path's over the images, and the cell path's from
    /// [`Renderer::splash`] -- and, under full motion, its plasma in the
    /// effect's slot. A change of it repaints the whole frame, as the
    /// screen fade does.
    pub fn set_splash(&mut self, look: Option<SplashLook>) {
        // Gone and staying gone, every frame after the launch: nothing to
        // build, and the trail as `set_trail` left it.
        if look.is_none() && self.splash.is_none() {
            return;
        }
        self.splash = look;
        if look.is_none() {
            self.splash_scale = None;
            self.splash_holes.clear();
        }
        self.build_splash();
        self.choose_trail();
    }

    /// **The splash's block drawn as a blinking cursor at `phase`**, from
    /// the frames to come: the block at the phase's opacity of its own, and
    /// over its cell what the phase draws over a cursor's -- the glow, the
    /// fire, the ufo -- by the one drawing the terminal's cursor takes
    /// (`caret::blink`), in the half's amber. Records like the rest of the
    /// splash's, so every path draws them. [`CursorPhase::SHOWN`], where it
    /// starts, draws the block as the splash always has; no driver sets
    /// another. `baud-app`'s logo film hands it the phase the app's caret
    /// plans, which is how the README's logo blinks as baud's cursor does.
    pub fn set_splash_cursor(&mut self, phase: CursorPhase) {
        if phase != self.splash_cursor {
            self.splash_cursor = phase;
            self.build_splash();
        }
    }

    /// **Plasma layers the CPU path draws with the splash**, from the next
    /// [`Renderer::render`]: each behind the lockup's glyphs or in front of
    /// them at its emitter's opacity ([`splash::Layer`]), in
    /// [`Renderer::set_splash_ink`]'s colour, through
    /// [`Renderer::set_splash_slabs`]'s slabs if any. None by default, and
    /// none is drawn without the splash. Only the CPU path draws them and no
    /// driver sets them: `baud-app`'s logo film does. A frame with layers,
    /// or the frame after one, repaints whole.
    pub fn set_splash_layers(&mut self, layers: &[splash::Layer]) {
        self.splash_layers.clear();
        self.splash_layers.extend_from_slice(layers);
    }

    /// **The colour the splash's layers take**: `None`, the default, the
    /// half's amber.
    pub fn set_splash_ink(&mut self, ink: Option<Argb>) {
        self.splash_ink = ink;
    }

    /// **The lockup's glyphs as slabs to the splash's layers**, from the next
    /// frame ([`splash::Slabs`]); `None`, the default, draws the layers
    /// under or over the glyphs and nothing more. Slabs whose fields do not
    /// each hold a weight for every pixel of their box are taken as none.
    pub fn set_splash_slabs(&mut self, slabs: Option<splash::Slabs>) {
        self.splash_slabs = slabs.filter(splash::Slabs::fits);
    }

    /// The splash's records, for a path that draws records of its own:
    /// with baud's own overlays, before the thumb and the fade.
    pub fn splash(&self) -> &[Instance] {
        &self.splash_inst
    }

    /// **The splash yields to what the shell has drawn on `term`**, from
    /// this frame on: every cell of its screen as this frame shows it that
    /// holds a character other than a space, a background of its own or
    /// reverse video, and the cursor's cell while it is shown -- each
    /// row's from the first such cell to the last, the whole row where it
    /// is drawn in another order -- is cut out of the splash's records and
    /// its plasma ([`splash::yielded`]). So no cell the shell has drawn is
    /// ever under the logo: its text and its prompt are drawn as they are
    /// with no splash, from the frame they arrive on, and the logo plays
    /// around them. An image is not yet among them: its placements are
    /// laid out after the splash is handed over. `lift` is the slide's pan
    /// this frame, zero where the grid is not sliding: the grid and the
    /// leaving row are drawn lowered by it and the splash is not, so each
    /// row's hole reaches `lift` further down, covering the row where it
    /// is drawn and where the logo would have been over it, and the
    /// leaving row's span is a hole on the window's top `lift` pixels.
    /// Each driver calls it after [`Renderer::set_splash`] on every frame,
    /// once the slide's pan is known; nothing while no splash is set.
    pub fn splash_yields_to(&mut self, term: &Term<'_>, lift: Px) {
        if self.splash.is_none() {
            self.splash_holes.clear();
            return;
        }
        if lift.value() > 0 {
            self.lay_out_leaving(term, term.window_back());
        }
        let holes = self.holes_of(term, lift);
        if holes != self.splash_holes {
            self.splash_holes = holes;
            self.build_splash();
            self.build_trail();
        }
    }

    /// What of `term`'s screen, as this frame shows it, the shell has
    /// drawn: [`Renderer::splash_yields_to`]'s holes, a row's at most one,
    /// each reaching `lift` below its row, and the leaving row's on the
    /// top `lift` pixels while the grid slides.
    fn holes_of(&self, term: &Term<'_>, lift: Px) -> Vec<splash::Hole> {
        let back = term.window_back();
        let view = term.view(back);
        let px = |v: usize, unit: Px| {
            i32::try_from(v)
                .unwrap_or(i32::MAX)
                .saturating_mul(i32::try_from(unit.value()).unwrap_or(i32::MAX))
        };
        let (cw, ch) = (self.metrics.cell_w, self.metrics.cell_h);
        let pan = px(1, lift);
        let cols = view.cols().index();
        let cursor = (back.index() == 0 && term.cursor_visible()).then(|| term.cursor());
        let mut out = Vec::new();
        // One row's columns, from its first drawn cell to its last, the
        // whole row where `reordered`; in a double-width row, twice each.
        let span_of = |view: &Viewport<'_>,
                       r: Row,
                       cursor: Option<Col>,
                       reordered: bool|
         -> Option<(i32, i32)> {
            let styles = view.styles(r);
            let mut span: Option<(usize, usize)> = None;
            let mut take = |x: usize| span = Some(span.map_or((x, x), |(a, _)| (a, x)));
            for (x, c) in view.row(r).iter().enumerate().take(cols) {
                let k = c.content;
                let ink = !k.is_empty() && k.as_codepoint().is_none_or(|p| p.value() != 0x20);
                let word = term.style_in(styles, *c);
                if ink || !word.bg().is_default() || word.attrs().has(Attrs::REVERSE) {
                    take(x);
                }
            }
            if let Some(cc) = cursor {
                take(cc.index().min(cols.saturating_sub(1)));
            }
            let (a, b) = span?;
            let (a, b) = match reordered {
                true => (0, cols.saturating_sub(1)),
                false => (a, b),
            };
            let k = match view.rendition(r) {
                LineRendition::Single => 1,
                _ => 2,
            };
            Some((px(a * k, cw), px((b + 1) * k, cw)))
        };
        for y in 0..view.rows().index().min(self.rows.index()) {
            let r = Row::from_index(y);
            let at = cursor.filter(|&(cr, _)| cr == r).map(|(_, c)| c);
            let Some((left, right)) = span_of(&view, r, at, self.bidi.orders().reordered(r)) else {
                continue;
            };
            out.push(splash::Hole {
                left,
                top: px(y, ch),
                right,
                bottom: px(y + 1, ch).saturating_add(pan),
            });
        }
        // The leaving row, drawn above the window's first with its bottom
        // `lift` down, in the order `Renderer::copy_leaving` draws it.
        if pan > 0 {
            let up = term.view(Rows::of_index(back.index() + 1));
            let reordered = self
                .beside
                .as_deref()
                .and_then(|b| b.leaving.as_ref())
                .is_some_and(|l| l.orders().reordered(Row::ZERO));
            if let Some((left, right)) = span_of(&up, Row::ZERO, None, reordered) {
                out.push(splash::Hole {
                    left,
                    top: 0,
                    right,
                    bottom: pan,
                });
            }
        }
        out
    }

    /// The splash's plasma this frame, and how much of its light is drawn
    /// ([`splash::glow`]), until the splash is gone.
    fn splash_vfx(&self) -> Option<(Vfx, f32)> {
        let look = self.splash?;
        let (scale, at) = self.splash_place()?;
        match look.phase {
            splash::SplashPhase::Plasma(age) if age < splash::GONE => {
                Some((splash::vfx(at, scale, age), splash::glow(age)))
            }
            _ => None,
        }
    }

    /// The splash's scale and where its box starts in the frame.
    fn splash_place(&self) -> Option<(f32, trail::Pt)> {
        let scale = self.splash_scale?;
        let (w, h) = self.pixel_size();
        let side = |p: Px| f32::from(u16::try_from(p.value()).unwrap_or(u16::MAX));
        Some((scale, splash::origin((side(w), side(h)), scale)))
    }

    /// The lockup's two colours on the theme's half.
    fn splash_colours(&self) -> splash::Colours {
        match self.theme.polarity() {
            Some(baud_domain::ColorScheme::Light) => splash::light(),
            _ => splash::DARK,
        }
    }

    /// **The splash's records**: `baud` and `❯` in the badge's face --
    /// baud's embedded JetBrains Mono Bold, 700 where the lockup asks 500
    /// for the wordmark and no embedded face has it, rather than the Regular
    /// -- at the lockup's sizes and pens, and the block, each at its opacity
    /// this phase.
    fn build_splash(&mut self) {
        self.splash_inst.clear();
        let Some(look) = self.splash else {
            return;
        };
        let (w, h) = self.pixel_size();
        let side = |p: Px| f32::from(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let scale = *self
            .splash_scale
            .get_or_insert_with(|| splash::scale((side(w), side(h))));
        let Some((_, at)) = self.splash_place() else {
            return;
        };
        if scale <= 0.0 {
            return;
        }
        let colours = self.splash_colours();
        let face = self.stack.badge();
        let f = self.stack.face(face);
        let upem = f.units_per_em();
        let mut out = Vec::new();
        for (line, text, colour) in [
            (splash::WORDMARK, "baud", colours.wordmark),
            (splash::PROMPT, "\u{276F}", colours.amber),
        ] {
            let px = line.size * scale;
            let s = Scale::of(px, upem);
            let mut pen = at.x + line.x * scale;
            let baseline = (at.y + line.baseline * scale).round() as i32;
            for c in text.chars() {
                let Some(id) = f.glyph(Codepoint::from_char(c)) else {
                    continue;
                };
                let advance = f
                    .default_advance(id)
                    .map_or(0.0, |a| s.px(f32::from(a.value())));
                if let Some(slot) = self.atlas.slot_sized(f, face, id, s) {
                    let x = pen.round() as i32 + slot.left;
                    let y = baseline + slot.top;
                    let (l, r) = (x as f32, (x + i32::from(slot.w.0)) as f32);
                    let alpha = splash::ink(look, at, scale, l, r);
                    out.push(Instance {
                        x: Screen(i16::try_from(x).unwrap_or(i16::MAX)),
                        y: Screen(i16::try_from(y).unwrap_or(i16::MAX)),
                        w: slot.w,
                        h: slot.h,
                        u: slot.x,
                        v: slot.y,
                        color: splash::at_alpha(colour, alpha),
                        kind: slot.kind,
                        curve: curve_row(colour),
                    });
                }
                pen += advance + line.tracking * scale;
            }
        }
        let b = splash::placed(splash::BLOCK, at, scale);
        let (x0, y0) = (b.left.round(), b.top.round());
        let (x1, y1) = (b.right.round(), b.bottom.round());
        let blink = self.splash_cursor;
        let shown = f32::from(blink.opacity.get()) / 255.0;
        out.push(Instance {
            x: Screen(i16::try_from(x0 as i64).unwrap_or(i16::MAX)),
            y: Screen(i16::try_from(y0 as i64).unwrap_or(i16::MAX)),
            w: Tex(u16::try_from((x1 - x0).max(0.0) as i64).unwrap_or(0)),
            h: Tex(u16::try_from((y1 - y0).max(0.0) as i64).unwrap_or(0)),
            u: Tex(0),
            v: Tex(0),
            color: splash::at_alpha(colours.amber, splash::block(look, at, scale) * shown),
            kind: Kind::Solid,
            curve: curve_row(colours.amber),
        });
        // What the phase draws over a cursor's cell, over the block's.
        if blink.glow.get() > 0 || blink.play != Play::Still {
            let px = |v: f32| Px::of(u32::try_from(v.max(0.0) as i64).unwrap_or(0));
            let cell = (px(x0), px(y0), px(x1 - x0), px(y1 - y0));
            caret::blink(cell, cell, blink, colours.amber, &mut out);
        }
        // A record at no opacity draws nothing: left out, so a vanished
        // glyph costs no draw.
        out.retain(|r| r.color.a().get() > 0);
        // And none over what the shell has drawn.
        splash::yielded(&mut out, &self.splash_holes);
        self.splash_inst = out;
    }

    /// The trail's records, in the colours in force: one for a ghost, a
    /// quad or a sweep; an effect's bands and particles, and the rows they
    /// span.
    fn build_trail(&mut self) {
        self.trail_inst.clear();
        let frame = self.pixel_size();
        // The splash's plasma is the colour of the glyph it is over (the
        // records split where the word ends, `splash::paint`); every trail
        // the cursor's colour.
        let splash = self.splash_vfx();
        let effect_fill = match splash {
            Some(_) => self.splash_colours().amber,
            None => self.cursor_fill,
        };
        match &mut self.trail {
            Some(Drawn::Fireworks(_, top, bottom)) => {
                for s in self.fireworks.iter().flatten() {
                    crate::fireworks::records(s, frame, &mut self.trail_inst);
                }
                let (mut lo, mut hi) = (i32::MAX, i32::MIN);
                for inst in &self.trail_inst {
                    lo = lo.min(i32::from(inst.y.0));
                    hi = hi.max(i32::from(inst.y.0) + i32::from(inst.h.0));
                }
                let row = |v: i32| Px::of(u32::try_from(v.max(0)).unwrap_or(0));
                (*top, *bottom) = if lo < hi {
                    (row(lo), row(hi))
                } else {
                    (Px::of(0), Px::of(0))
                };
            }
            Some(Drawn::Ghost(g)) => self.trail_inst.push(trail::instance(*g, self.cursor_fill)),
            Some(Drawn::Smear(s)) => self
                .trail_inst
                .push(trail::quad_instance(s, self.cursor_fill)),
            Some(Drawn::Vfx(v, top, bottom)) => {
                v.instances(frame, effect_fill, &mut self.trail_inst);
                // A repeat pass keeps the bands, a cell row each, and not
                // the particles that stack over them, whose alphas would
                // compound past the cap. The bands tile the pixel rows
                // (`Vfx::instances`), so each pixel is under one record.
                if v.emitter.strength == Strength::UnderThreshold {
                    self.trail_inst.retain(|r| r.kind == Kind::Band);
                }
                let (mut lo, mut hi) = (i32::MAX, i32::MIN);
                for inst in &self.trail_inst {
                    lo = lo.min(i32::from(inst.y.0));
                    hi = hi.max(i32::from(inst.y.0) + i32::from(inst.h.0));
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
        if let (Some(_), Some((scale, at))) = (splash, self.splash_place()) {
            let colours = self.splash_colours();
            splash::paint(&mut self.trail_inst, at, scale, colours);
            // The plasma over none of what the shell has drawn, as the
            // logo it carries (`Renderer::splash_yields_to`).
            splash::yielded(&mut self.trail_inst, &self.splash_holes);
        }
        if self.strength() == Strength::UnderThreshold {
            flash::cap_alpha(&mut self.trail_inst, self.flash_cap);
        }
        // What is left of the splash's plasma fades out as the logo goes:
        // every record's alpha scaled, as a cap scales it.
        if let Some((_, glow)) = splash.filter(|&(_, g)| g < 1.0) {
            flash::cap_alpha(&mut self.trail_inst, Chan::from_unit(glow));
        }
        // A hidden cursor's trail fading out (`Smear::opacity`): every
        // record's alpha scaled by it, as the glow scales them, so each GPU
        // path draws the fade from the records it already reads.
        let k = self.opacity();
        if k != Smear::OPAQUE {
            flash::cap_alpha(&mut self.trail_inst, k);
        }
    }

    /// How much of this frame's trail is drawn, as the driver handed it:
    /// a smear's opacity, an effect's emitter's, and all of a ghost.
    fn opacity(&self) -> Chan {
        match &self.trail {
            Some(Drawn::Smear(s)) => s.opacity,
            Some(Drawn::Vfx(v, ..)) => v.emitter.opacity,
            Some(Drawn::Ghost(_) | Drawn::Fireworks(..)) | None => Smear::OPAQUE,
        }
    }

    /// How this frame's trail is drawn, as the driver weighed it.
    fn strength(&self) -> Strength {
        match &self.trail {
            Some(Drawn::Ghost(g)) => g.strength,
            Some(Drawn::Smear(s)) => s.strength,
            Some(Drawn::Vfx(v, ..)) => v.emitter.strength,
            Some(Drawn::Fireworks(..)) | None => Strength::Full,
        }
    }

    /// **The cap a repeat pass is drawn at, from the cells under it**: the
    /// least [`flash::under_threshold`] of the cursor's colour over each
    /// colour those cells draw, made opaque -- every one's background, and
    /// the text colours of one that holds a character or sets an attribute
    /// (its solid and glass text, its line and its underline), each as this
    /// frame draws it, a blinking cell's faint phase faint -- so a pass
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
        // The window as the last layout drew it: the display's lines where
        // it re-broke a finished paragraph ([`crate::rebreak`]), so each
        // cell under the pass is the one drawn there.
        let held = self.bidi.orders().display_held();
        let view = match &held {
            Some(d) => d.window(crate::frozen::live_table(term)),
            None => term.view(term.window_back()),
        };
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
            .then(|| match &self.composition {
                Some(c) if c.row.index() < rows => c.cursor(c.width_on(&view)),
                _ => Some(term.cursor()),
            })
            .flatten()
            .map(|(r, c)| (r, c, term.cursor_style()))
            .filter(|&(_, _, style)| self.cursor_drawn(style));
        // Each cell's rendition read where its id names it: a window over
        // history the core froze names a generation's (`crate::frozen`).
        let back = term.looking_back(term.window_back());
        let mut ids: Vec<(baud_domain::StyleWord, Inked, Selected, Cursored, Guided)> = Vec::new();
        // The guide's row and the block it leaves out, in cells.
        let guide_row = self
            .guide_at
            .map(|g| (g.top / u32::try_from(ch).unwrap_or(1), g.hole));
        'walk: for r in &self.trail_inst {
            let (x, y) = (i32::from(r.x.0), i32::from(r.y.0));
            let (x1, y1) = (x + i32::from(r.w.0), y + i32::from(r.h.0));
            for row in at(y) / ch..at(y1).div_ceil(ch).min(rows) {
                let row = Row::from_index(row);
                let cells = view.row(row);
                let source = self.bidi.orders().row_source_logical(term, back, row);
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
                    let orders = self.bidi.orders();
                    let selected = match self.highlight.is_some_and(|h| {
                        orders.selects(&h, row, orders.logical_col(row, col), cell.content)
                    }) {
                        true => Selected::Yes,
                        false => Selected::No,
                    };
                    let cursored = match cursor {
                        Some((cr, cc, style)) if cr == row && cc == col => {
                            match self.swaps(style) {
                                true => Cursored::Block,
                                false => Cursored::Shape,
                            }
                        }
                        _ => Cursored::No,
                    };
                    let guided = match guide_row {
                        Some((gr, hole))
                            if usize::try_from(gr).ok() == Some(row.index())
                                && !hole.is_some_and(|(hx, hw)| {
                                    let x = u32::try_from((first + c) * cw).unwrap_or(u32::MAX);
                                    x >= hx && x < hx.saturating_add(hw)
                                }) =>
                        {
                            Guided::Yes
                        }
                        _ => Guided::No,
                    };
                    let word = source.words().word(term, Col::at_index(first + c), *cell);
                    let id = (word, inked, selected, cursored, guided);
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
        let meets = |px0: i32, py0: i32, px1: i32, py1: i32| {
            self.trail_inst.iter().any(|r| {
                let (x, y) = (i32::from(r.x.0), i32::from(r.y.0));
                x < px1 && px0 < x + i32::from(r.w.0) && y < py1 && py0 < y + i32::from(r.h.0)
            })
        };
        let imaged = self.image_places.iter().any(|p| {
            let (px0, py0) = (i32::from(p.x), i32::from(p.y));
            p.layer.under_text() && meets(px0, py0, px0 + i32::from(p.w), py0 + i32::from(p.h))
        });
        // **The badge is a ground no cell names, as an image under the text
        // is**: a pass that meets its rectangle draws nothing.
        let side = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        let imaged = imaged
            || self
                .badge_rect
                .is_some_and(|b| meets(side(b[0]), side(b[1]), side(b[2]), side(b[3])));
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
        // The page a cell under the guide shows: the guide's colour over it
        // at its middle's alpha and at its lines'.
        let guides = |page: Argb| -> [Option<Argb>; 2] {
            match self.guide {
                Some(g) => [g.colour.a(), guide::twice(g.colour.a())]
                    .map(|a| Some(opaque(g.colour).mix(page, a))),
                None => [None, None],
            }
        };
        for &(word, inked, selected, cursored, guided) in &ids {
            let res = self.resolve_slot(term, word);
            let text = inked == Inked::Yes || word.attrs() != baud_domain::Attrs::NONE;
            let n = if text { 5 } else { 1 };
            // The text in the colours this frame draws it: a blinking
            // cell's faint phase half way to its page, as `drawn` mixes it.
            let shown = |c: Argb| match res.text(self.blink) {
                Text::Faint => c.faint_on(res.bg),
                Text::Drawn | Text::Hidden => c,
            };
            let rule = match res.shapes.has(Shapes::UNDERLINE_FOLLOWS_FG) {
                true => res.fg,
                false => res.underline,
            };
            for colour in [
                res.bg,
                shown(res.fg),
                shown(res.glass),
                shown(res.line),
                shown(rule),
            ]
            .into_iter()
            .take(n)
            {
                ground(colour, res.bg, &mut triples);
                if guided == Guided::Yes {
                    for page in guides(res.bg).into_iter().flatten() {
                        ground(page, page, &mut triples);
                        ground(colour, page, &mut triples);
                    }
                }
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
                hilite: self.hilite,
                block: match cursored {
                    Cursored::Block => BlockCursor::Yes(self.block_paint),
                    _ => BlockCursor::No,
                },
                blink: self.blink,
                cursor: cursor.map_or(Chan::from_u8(0xFF), |(_, _, s)| self.cursor_opacity(s)),
            });
            let pages = core::iter::once(Some(d.bg)).chain(match guided {
                Guided::Yes => guides(d.bg),
                Guided::No => [None, None],
            });
            for page in pages.flatten() {
                ground(page, page, &mut triples);
                if text {
                    for colour in [d.fg, d.rule(res.line), d.rule(rule)] {
                        ground(colour, page, &mut triples);
                    }
                }
                if cursored == Cursored::Shape {
                    ground(self.shape_colour(d.bg, d.cursor_fg), page, &mut triples);
                }
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
        // Each ground scanned over the composite the trail is drawn by: the
        // plasma's `screen` or `multiply` moves a ground further than
        // source-over does at one alpha.
        let mix = self.trail_mix();
        for (text, page) in triples {
            if k.get() == 0 && budget == 0 {
                break;
            }
            let c = match self.flash_caps.get(&(ink, text, page, mix)) {
                Some(&c) => c,
                None if budget > 0 => {
                    budget -= 1;
                    let c = flash::under_threshold_over_by(ink, text, page, mix);
                    self.flash_caps.insert((ink, text, page, mix), c);
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

    /// **How this frame's trail meets what is under it** ([`Mix`]), the one
    /// decision every path draws by -- the CPU's compositor and both GPU
    /// paths' blend state. **The plasma in the terminal is drawn in front of
    /// the text**: by `screen` on a dark theme, which lights the letters it
    /// crosses, and by `multiply` on a light one, which shades them, still
    /// in the cursor's colour. That colour is the text's by default, and
    /// source-over in a letter's own colour leaves the letter as it was, so
    /// the plasma read as behind the text though it was drawn over it. Over
    /// the page each draws as source-over does within a few steps: `screen`
    /// is source-over on black and `multiply` on white. The splash's plasma,
    /// every other trail, a theme with no polarity and the `Linear` blend
    /// draw source-over. A repeat's cap is scanned over the same mix
    /// (`Renderer::bound_trail`, [`flash::under_threshold_by`]).
    pub fn trail_mix(&self) -> Mix {
        let plasma = matches!(self.trail, Some(Drawn::Vfx(v, ..)) if v.effect == crate::effect::Effect::Plasma);
        if !plasma || self.blend != Blend::Native || self.splash_vfx().is_some() {
            return Mix::Normal;
        }
        match self.theme.polarity() {
            Some(baud_domain::ColorScheme::Dark) => Mix::Screen,
            Some(baud_domain::ColorScheme::Light) => Mix::Multiply,
            None => Mix::Normal,
        }
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
    ///
    /// A hollow cursor ([`CursorLook::Hollow`]) fills its cell's box,
    /// whatever its shape: the box the outline is drawn round, which a
    /// trail chases.
    pub fn cursor_rect(&self, row: Row, col: Col, style: CursorStyle) -> Rect {
        let style = match self.cursor_look {
            CursorLook::Shaped => style,
            CursorLook::Hollow => CursorStyle::SteadyBlock,
        };
        let (left, top, w, h) = self.cursor_px(row, col, style);
        let f = |v: u32| f32::from(u16::try_from(v).unwrap_or(u16::MAX));
        Rect {
            left: f(left.value()),
            top: f(top.value()),
            right: f(left.value() + w.value()),
            bottom: f(top.value() + h.value()),
        }
    }

    /// **How the cursor is drawn from the next convert or resolve on**: in
    /// its shape, or hollow whatever its shape. The driver sets
    /// [`CursorLook::Hollow`] while another window has the keyboard and
    /// `[cursor] unfocused_hollow` is on, and [`CursorLook::Shaped`]
    /// otherwise; a flip repaints the cursor's row.
    pub fn set_cursor_look(&mut self, look: CursorLook) {
        self.cursor_look = look;
    }

    /// What [`Renderer::set_cursor_look`] last set: the cell path draws a
    /// hollow cursor from it.
    pub fn cursor_look(&self) -> CursorLook {
        self.cursor_look
    }

    /// Whether a cursor of `style` swaps its cell's colours: a block drawn
    /// in its shape. A hollow cursor swaps nothing.
    fn swaps(&self, style: CursorStyle) -> bool {
        self.cursor_look == CursorLook::Shaped
            && matches!(style, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock)
    }

    /// **The hollow cursor's four solids** at `(r, col)`, in `colour`: round
    /// the box a block cursor fills there ([`Renderer::cursor_px`], on the
    /// line's geometry), its top and bottom across the box and its sides
    /// between them, as alacritty's `hollow` lays them out
    /// (`[[source: alacritty-display-cursor]]` 73-90). Each is the bar's
    /// line thick, `max(thickness, 2)`: one point at the default size on a
    /// 2x display, kitty's 1pt (`[[source: kitty-decorations-851cb80]]`
    /// 231-238) and foot's one logical pixel (`[[source:
    /// foot-render-cb27717]]` 342-356), at most half the box's narrower
    /// side. The cell shader's `cell_ring_vertex` draws the same four.
    /// Out of line, so a window in front pays nothing for it in
    /// `convert_row`.
    #[cold]
    #[inline(never)]
    fn hollow(&self, r: Row, col: Col, colour: Argb) -> [Instance; 4] {
        let (left, top, w, h) = self.cursor_px(r, col, CursorStyle::SteadyBlock);
        let (x, y, w, h) = (left.value(), top.value(), w.value(), h.value());
        let t = self.metrics.thickness.value().max(2).min(w.min(h) / 2);
        let i = |v: u32| screen(i32::try_from(v).unwrap_or(i32::MAX));
        let side = h.saturating_sub(2 * t);
        [
            solid(i(x), i(y), Px::of(w), Px::of(t), colour),
            solid(i(x), i(y + h - t), Px::of(w), Px::of(t), colour),
            solid(i(x), i(y + t), Px::of(t), Px::of(side), colour),
            solid(i(x + w - t), i(y + t), Px::of(t), Px::of(side), colour),
        ]
    }

    /// [`Renderer::cursor_rect`] in whole pixels: left, top, width, height.
    ///
    /// **On a line that is not single-width the cursor is its character's
    /// box's**, through the line's geometry ([`stretched`]): two cells wide,
    /// and on a double-height line each row's half of a shape twice as tall,
    /// so an underline cursor shows on the bottom half alone.
    ///
    /// **`col` is the cursor's logical column**: it is drawn over the
    /// visual column this frame's order puts that cell at, and a bar on the
    /// side its character runs from ([`crate::CursorSide`]).
    fn cursor_px(&self, row: Row, col: Col, style: CursorStyle) -> (Px, Px, Px, Px) {
        let orders = self.bidi.orders();
        let side = orders.side(row, col);
        let col = orders.visual_col(row, col);
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
            CursorStyle::BlinkingBar | CursorStyle::SteadyBar => match side {
                crate::bidi::CursorSide::Left => (x, top, line, ch),
                crate::bidi::CursorSide::Right => (x + cw.saturating_sub(line), top, line, ch),
            },
            _ => (x, top, cw, ch),
        };
        let size = self.size_of(row);
        if size.is_double() {
            let i = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
            let shape = solid(
                screen(i(left)),
                screen(i(top)),
                Px::of(w),
                Px::of(h),
                Argb(0),
            );
            return match stretched(shape, size, self.band(row)) {
                Some(s) => (
                    Px::of(u32::try_from(s.x.0).unwrap_or(0)),
                    Px::of(u32::try_from(s.y.0).unwrap_or(0)),
                    Px::of(u32::from(s.w.0)),
                    Px::of(u32::from(s.h.0)),
                ),
                None => (Px::of(left * 2), Px::of(top), Px::of(0), Px::of(0)),
            };
        }
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
    ///
    /// The slide's veil ([`crate::VeilFrom::Faint`]) starts from the least
    /// alpha that leaves none of these a flash from the background or from
    /// one another: the 16 colours the palette starts with, the default
    /// foreground, and the corners of the colour cube.
    pub fn set_screen_fade(&mut self, fade: Option<Transition>) {
        let (w, h) = self.pixel_size();
        let faint = match fade.map(|t| t.from) {
            Some(crate::VeilFrom::Faint) => self.faint_veil(),
            _ => Chan::from_u8(0),
        };
        self.fade = fade.map(|t| transition::veil(t, w, h, faint));
        self.fade_inst.clear();
        if let Some(v) = self.fade {
            self.fade_inst
                .push(transition::instance(v, self.defaults.1));
        }
    }

    /// The slide's faint veil's alpha over the default background drawn
    /// now, found when that background changes.
    fn faint_veil(&mut self) -> Chan {
        let ground = self.defaults.1.with_alpha(Chan::from_u8(u8::MAX));
        if let Some((g, a)) = self.faint {
            if g == ground {
                return a;
            }
        }
        let c = |v: u8| Chan::from_u8(v);
        let corners = (0u8..8).map(|k| {
            let on = |b: u8| c(if k & b != 0 { u8::MAX } else { 0 });
            Argb::rgb(on(1), on(2), on(4))
        });
        let inks: Vec<Argb> = self.theme.palette[..16]
            .iter()
            .copied()
            .chain([self.theme.fg, self.defaults.0])
            .chain(corners)
            .map(|i| i.with_alpha(c(u8::MAX)))
            .collect();
        let keep = transition::faint_alpha(ground, &inks);
        let veil = c(u8::MAX - keep.get());
        self.faint = Some((ground, veil));
        veil
    }

    /// The screen fade's instances, for a path that draws records of its
    /// own -- drawn last, over the cells, the trail and the scrollbar.
    pub fn screen_fade(&self) -> &[Instance] {
        &self.fade_inst
    }

    /// **The visual bell in a colour for the next frame**, or none: one
    /// solid over the whole screen in the bell's colour at the fade's alpha
    /// ([`crate::bell`]), which every path draws over the cells and the
    /// images over the text and under baud's own overlays -- the find
    /// field, the link preview's bar, the scrollbar and the screen fade --
    /// as kitty draws its bell before its scrollbar and its hyperlink
    /// target (`[[source: kitty-6d5d0c4-shaders]]` 1983-1988). The driver
    /// hands the alpha its clock gives; this only places it, so the pixel
    /// oracle can compare the paths on it. The other-video flash is
    /// [`Renderer::set_flash`]'s.
    pub fn set_bell_tint(&mut self, tint: Option<crate::bell::BellTint>) {
        self.bell = tint;
        self.build_bell();
    }

    fn build_bell(&mut self) {
        let (w, h) = self.pixel_size();
        self.bell_inst.clear();
        if let Some(t) = self.bell {
            let v = Veil {
                w,
                h,
                alpha: t.alpha,
            };
            self.bell_inst.push(transition::instance(v, t.colour));
        }
    }

    /// The visual bell's instances, for a path that draws records of its
    /// own -- drawn first of baud's own overlays.
    pub fn bell_tint(&self) -> &[Instance] {
        &self.bell_inst
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
            let shape = self.cursor_px(r, c, style);
            caret::blink(cell, shape, phase, self.cursor_fill, &mut self.glow_inst);
        }
    }

    /// **iTerm2's badge** for the frames to come, or none (`badge.rs`): its
    /// text laid out and its glyphs rasterised at the size it fits at, its
    /// records drawn over the backgrounds and the images under the text,
    /// before the cursor guide and under the text, on every path -- first
    /// in [`Renderer::instances`], the CPU path's after a row's images, and
    /// the cell path's from [`Renderer::badge`] with its grounds. Built
    /// again only when the look or the frame's size changes.
    pub fn set_badge(&mut self, look: Option<BadgeLook>) {
        let frame = self.frame_size();
        let key = look.map(|l| (l, frame));
        if key == self.badge_key {
            return;
        }
        self.badge_key = key.clone();
        self.badge_built.bump();
        self.badge_inst.clear();
        self.badge_rect = None;
        if let Some((look, _)) = key {
            self.build_badge(&look, frame);
        }
        self.cut_badge();
    }

    /// The badge's records cut around this frame's holes.
    fn cut_badge(&mut self) {
        self.badge_drawn.clear();
        for &inst in &self.badge_inst {
            badge::around(inst, &self.badge_holes, &mut self.badge_drawn);
        }
    }

    /// **The activity lights, drawn in the badge's face** -- baud's
    /// embedded JetBrains Mono Bold (`Stack::badge`) -- for the window's
    /// title strip ([`crate::lights::draw`]).
    pub fn activity_lights(
        &self,
        look: &crate::LightsLook,
        shown: crate::Shown,
        scale: f32,
        height: f32,
        out: &mut crate::LightsImage,
    ) -> Option<()> {
        let face = self.stack.face(self.stack.badge());
        crate::lights::draw(face, look, shown, scale, height, out)
    }

    /// **One activity light at `light`**, in the badge's face, the picture
    /// a view of its own shows ([`crate::lights::draw_led`]).
    pub fn activity_light(
        &self,
        look: &crate::LightsLook,
        led: crate::Led,
        light: crate::Light,
        scale: f32,
        height: f32,
        out: &mut crate::LightsImage,
    ) -> Option<()> {
        let face = self.stack.face(self.stack.badge());
        crate::lights::draw_led(face, look, led, light, scale, height, out)
    }

    /// **One activity light at `light` on the title bar's panel**, in the
    /// badge's face ([`crate::lights::draw_panel_led`]).
    pub fn panel_light(
        &self,
        look: &crate::LightsLook,
        led: crate::Led,
        light: crate::Light,
        scale: f32,
        height: f32,
        out: &mut crate::LightsImage,
    ) -> Option<()> {
        let face = self.stack.face(self.stack.badge());
        crate::lights::draw_panel_led(face, look, led, light, scale, height, out)
    }

    /// Each lamp's column on the panel at `scale`, in device pixels, and
    /// their width ([`crate::lights::panel_layout`]).
    pub fn panel_layout(
        &self,
        scale: f32,
    ) -> Option<(crate::ByLed<crate::lights::Slot>, baud_font::Px)> {
        crate::lights::panel_layout(self.stack.face(self.stack.badge()), scale)
    }

    /// Each lamp's letters on the panel, their left and width in points
    /// ([`crate::lights::panel_letters`]).
    pub fn panel_letters(&self) -> crate::ByLed<(f32, f32)> {
        crate::lights::panel_letters(self.stack.face(self.stack.badge()))
    }

    /// Where each activity light sits at `scale`, in device pixels, and
    /// the strip's width ([`crate::lights::layout`]).
    pub fn activity_lights_layout(
        &self,
        scale: f32,
    ) -> Option<(crate::ByLed<crate::lights::Slot>, baud_font::Px)> {
        crate::lights::layout(self.stack.face(self.stack.badge()), scale)
    }

    /// The activity lights' width in points, in the badge's face.
    pub fn activity_lights_width(&self) -> f32 {
        crate::lights::width(self.stack.face(self.stack.badge()))
    }

    /// The badge's records, for a path that draws records of its own:
    /// first among its grounds, before the cursor guide.
    pub fn badge(&self) -> &[Instance] {
        &self.badge_drawn
    }

    /// The face a badge's cluster is drawn from: the badge's own face
    /// ([`Stack::badge`]) when it draws every character of it, else the
    /// face `style` would draw it from in the text.
    fn badge_face(&mut self, cps: &[Codepoint], style: FaceStyle) -> FaceIx {
        if cps.iter().all(|&c| self.stack.badge_carries(c)) {
            self.stack.badge()
        } else {
            self.face_for(cps, style)
        }
    }

    /// The badge's records for `look` over a frame of `frame` pixels, and
    /// the rectangle they cover.
    fn build_badge(&mut self, look: &BadgeLook, frame: (Px, Px)) {
        let pieces = badge::pieces(&look.text);
        if pieces.iter().all(|p| p.space == badge::Space::Yes) {
            return;
        }
        // The badge face's grid at a point size, unrounded: its `M`'s
        // advance a column, its ascent and its line, as `Metrics::of`.
        let primary = self.stack.face(self.stack.badge());
        let Some(m) = primary.glyph(baud_domain::Codepoint::from_char('M')) else {
            return;
        };
        let Some(advance) = primary.default_advance(m) else {
            return;
        };
        let lm = primary.line_metrics();
        let upem = primary.units_per_em();
        let grid = |points: u32| {
            let px = f32::from(u16::try_from(points).unwrap_or(u16::MAX)) * look.scale;
            let s = Scale::of(px, upem);
            badge::Grid {
                advance: s.px(f32::from(advance.value())),
                ascent: s.px(f32::from(lm.ascender.value())),
                height: s.px(
                    f32::from(lm.ascender.value()) - f32::from(lm.descender.value())
                        + f32::from(lm.line_gap.value()),
                ),
            }
        };
        let (fw, fh) = (
            f32::from(u16::try_from(frame.0.value()).unwrap_or(u16::MAX)),
            f32::from(u16::try_from(frame.1.value()).unwrap_or(u16::MAX)),
        );
        let max = (
            fw * look.fraction.0.clamp(0.01, 1.0),
            fh * look.fraction.1.clamp(0.0, 1.0),
        );
        let (points, lines, g) = badge::fit(&pieces, grid, max);
        let px = f32::from(u16::try_from(points).unwrap_or(u16::MAX)) * look.scale;
        // **The atlas holds the badge at two sizes at most.** A slot is
        // never freed, so a window dragged through every size the badge fits
        // at would fill the sheet with glyphs no frame draws; a third size
        // starts the atlas over, as a font size change does, and every row
        // is converted again with the glyphs drawn afresh.
        if !self.badge_sizes.contains(&px) {
            if self.badge_sizes.len() >= BADGE_SIZES {
                self.atlas.reset();
                self.badge_sizes.clear();
                self.known = Prev::Unknown;
            }
            self.badge_sizes.push(px);
        }
        let cells = |n: u32| f32::from(u16::try_from(n).unwrap_or(u16::MAX)) * g.advance;
        // Every line right-aligned at the box's right edge, `right` from
        // the frame's.
        let right = fw - f32::from(u16::try_from(look.right.value()).unwrap_or(0));
        let y0 = f32::from(u16::try_from(look.top.value()).unwrap_or(0));
        let curve = curve_row(look.colour);
        // The outline: a line `STROKE` of the point size wide, centred on
        // each glyph's outline (`badge::STROKE`).
        let stroke = badge::STROKE * px;
        let outline_curve = curve_row(look.outline);
        let mut out = Vec::new();
        for (n, line) in lines.iter().enumerate() {
            let top = y0 + f32::from(u16::try_from(n).unwrap_or(u16::MAX)) * g.height;
            if top + g.height > y0 + max.1 + 0.5 && n > 0 {
                break;
            }
            let baseline = (top + g.ascent).round() as i32;
            let mut col = 0u32;
            let left = right - cells(line.cells);
            // Runs of pieces on one face, each shaped on its own grid.
            let mut at = line.from;
            while at < line.to {
                let p = &pieces[at];
                if p.space == badge::Space::Yes || p.cells == 0 {
                    col += p.cells;
                    at += 1;
                    continue;
                }
                let face = self.badge_face(&p.cps, look.style);
                let (start, mut run, mut clusters) = (col, 0u32, ClusterList::default());
                while at < line.to {
                    let q = &pieces[at];
                    let same = q.space == badge::Space::No
                        && q.cells > 0
                        && self.badge_face(&q.cps, look.style) == face;
                    if !same || run + q.cells > 250 {
                        break;
                    }
                    clusters.push(&q.cps, CellIx(u8::try_from(run).unwrap_or(u8::MAX)));
                    run += q.cells;
                    at += 1;
                }
                col += run;
                let script = Script::of_cluster(clusters.clusters().first());
                let shaped = self.shape(face, script, clusters.clusters(), &[], ..);
                let f = self.stack.face(face);
                let scale = Scale::of(px, f.units_per_em());
                for p in shaped.glyphs() {
                    let Some(slot) = self.atlas.slot_faded(f, face, p.id, scale, look.drawn_at)
                    else {
                        continue;
                    };
                    let pen = left + cells(start + u32::from(p.cell.0)) + scale.px(f32::from(p.dx));
                    let x = pen.round() as i32 + slot.left;
                    let y = baseline - scale.px(f32::from(p.dy)).round() as i32 + slot.top;
                    out.push(Instance {
                        x: Screen(i16::try_from(x).unwrap_or(i16::MAX)),
                        y: Screen(i16::try_from(y).unwrap_or(i16::MAX)),
                        w: slot.w,
                        h: slot.h,
                        u: slot.x,
                        v: slot.y,
                        color: look.colour,
                        kind: slot.kind,
                        curve,
                    });
                    // Its outline over it, a glyph at a time, as the text
                    // system strokes a glyph after filling it.
                    if slot.kind != Kind::Mask || look.outline.a().get() == 0 {
                        continue;
                    }
                    let Some(ring) = self.atlas.slot_stroked(f, face, p.id, scale, stroke) else {
                        continue;
                    };
                    let (x, y) = (x - slot.left + ring.left, y - slot.top + ring.top);
                    out.push(Instance {
                        x: Screen(i16::try_from(x).unwrap_or(i16::MAX)),
                        y: Screen(i16::try_from(y).unwrap_or(i16::MAX)),
                        w: ring.w,
                        h: ring.h,
                        u: ring.x,
                        v: ring.y,
                        color: look.outline,
                        kind: ring.kind,
                        curve: outline_curve,
                    });
                }
            }
        }
        let mut rect: Option<[i32; 4]> = None;
        for i in &out {
            let (x, y) = (i32::from(i.x.0), i32::from(i.y.0));
            let (x1, y1) = (x + i32::from(i.w.0), y + i32::from(i.h.0));
            rect = Some(match rect {
                None => [x, y, x1, y1],
                Some(r) => [r[0].min(x), r[1].min(y), r[2].max(x1), r[3].max(y1)],
            });
        }
        let at = |v: i32| u32::try_from(v.max(0)).unwrap_or(0);
        self.badge_rect = rect.map(|r| [at(r[0]), at(r[1]), at(r[2]), at(r[3])]);
        self.badge_inst = out;
    }

    /// **iTerm2's cursor guide** for the frames to come, or none: drawn on
    /// the cursor's row, over the backgrounds and the images under the
    /// text and under the text and the cursor, on every path (`guide.rs`).
    /// Its row is placed by [`Renderer::place_grounds`].
    pub fn set_cursor_guide(&mut self, look: Option<GuideLook>) {
        self.guide = look;
    }

    /// **The cursor's line in a window `back` lines into the history**, for
    /// the cursor guide: the live cursor's row lowered by the window's
    /// offset while it is still inside the main display, the status line's
    /// row while DECSASD puts the cursor there, since that line is in view
    /// whatever the window looks back on, and none while DECTCEM hides the
    /// cursor, as iTerm2's CPU path hides the guide. Both of iTerm2's paths
    /// draw the guide on the cursor's line, shifted with the scroll, for as
    /// long as that line is in view: `drawCursrGuideIfNeededInRect`,
    /// iTerm2's spelling
    /// (`[[source: iterm2-7ce037e-text-drawing-helper]]`), and the Metal
    /// path's `loadCursorInfoWithDrawingHelper`
    /// (`[[source: iterm2-964a75d-metal-per-frame-state]]`).
    pub fn cursor_line(term: &Term<'_>, back: Rows) -> Option<Row> {
        let main = term.geometry().1;
        if term.status_line().and_then(|l| l.cursor).is_some() {
            return Some(Row::from_index(main.index()));
        }
        let back = term.looking_back(back);
        let row = term.cursor().0.index() + back.index();
        (term.cursor_visible() && row < main.index()).then(|| Row::from_index(row))
    }

    /// **What lies under the glyphs this frame, placed**, from the live
    /// cursor `at` while the cursor is shown -- `None` while it is hidden,
    /// looked back from or off the screen drawn -- and `line`, the cursor's
    /// line in the window ([`Renderer::cursor_line`]): the cursor guide on
    /// `at`'s row, else on `line` while the window looks back, and both the
    /// guide and the badge leaving out the cell of a block the phase draws,
    /// which iTerm2 draws over both; the badge leaves out an input method's
    /// composition as well, whose background iTerm2 fills over everything
    /// (`drawInputMethodEditorTextAt`, `[[source:
    /// iterm2-7ce037e-text-drawing-helper]]`). [`Renderer::convert`] calls
    /// this itself; a cell-path driver calls it with the cursor it computed
    /// and draws [`Renderer::badge`] and [`Renderer::cursor_guide`] before
    /// the glyphs.
    pub fn place_grounds(&mut self, at: Option<(Row, Col, CursorStyle)>, line: Option<Row>) {
        let rows = self.rows.index();
        let side = |v: Px| i32::try_from(v.value()).unwrap_or(i32::MAX);
        self.badge_holes.clear();
        if let Some((r, c, style)) = at.filter(|(r, _, _)| r.index() < rows) {
            let block = matches!(style, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock);
            if block && self.cursor_drawn(style) {
                let (x, y, w, h) = self.cursor_px(r, c, style);
                let (x, y) = (side(x), side(y));
                self.badge_holes.push([x, y, x + side(w), y + side(h)]);
            }
        }
        if let Some(comp) = self.composition.as_ref().filter(|c| c.row.index() < rows) {
            let (start, end) = comp.columns(self.cols);
            // A cell two wide on a double-width or double-height line.
            let cw =
                side(self.metrics.cell_w).saturating_mul(Stretch::of(self.size_of(comp.row)).x());
            let band = self.band(comp.row);
            let at = |n: usize| i32::try_from(n).unwrap_or(i32::MAX).saturating_mul(cw);
            // Each visual run the row's order draws its cells in.
            let runs = match end > start {
                true => {
                    self.bidi
                        .orders()
                        .runs(comp.row, Col::at_index(start), Col::at_index(end - 1))
                }
                false => vec![(Col::at_index(start), Cols::of_index(0))],
            };
            for (v, n) in runs {
                self.badge_holes.push([
                    at(v.index()),
                    side(band.top),
                    at(v.index() + n.index()),
                    side(band.bottom),
                ]);
            }
        }
        self.cut_badge();
        self.guide_at = self.guide.and_then(|look| {
            let drawn = at.filter(|(r, _, _)| r.index() < rows);
            let r = drawn
                .map(|(r, _, _)| r)
                .or(line)
                .filter(|r| r.index() < rows)?;
            let hole = drawn.and_then(|(r, c, style)| {
                let block = matches!(style, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock);
                (block && self.cursor_drawn(style)).then(|| {
                    let (x, _, w, _) = self.cursor_px(r, c, style);
                    (x.value(), w.value())
                })
            });
            Some(guide::Placed {
                top: self.band(r).top.value(),
                hole,
                look,
            })
        });
        self.guide_inst.clear();
        if let Some(p) = self.guide_at {
            let width = self.frame_size().0.value();
            guide::records(p, self.metrics.cell_h, Px::of(width), &mut self.guide_inst);
        }
    }

    /// The guide's records, for a path that draws records of its own:
    /// after the backgrounds and the images under the text, before the
    /// glyphs, as [`Renderer::instances`] has them first.
    pub fn cursor_guide(&self) -> &[Instance] {
        &self.guide_inst
    }

    /// [`Renderer::cursor_guide`] after `out`'s records, lowered `pan`
    /// pixels: what the cell path draws while a smooth scroll slides its
    /// grid, so the guide stays on the cursor's row as the row moves.
    pub fn cursor_guide_lowered(&self, pan: Px, out: &mut Vec<Instance>) {
        let down = i16::try_from(pan.value()).unwrap_or(i16::MAX);
        out.extend(self.guide_inst.iter().map(|g| Instance {
            y: Screen(g.y.0.saturating_add(down)),
            ..*g
        }));
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

    /// The colours a selection draws in on the last convert or resolve:
    /// `OSC 17`'s and `19`'s, as [`Term::highlight_colors`] gives them, each
    /// the theme's ([`Theme::selection`]) where the program set none, and
    /// with neither, the pair [`Renderer::set_inactive_selection`] set while
    /// the window is not key. What a caller that draws the selection itself
    /// -- the cell path -- reads.
    pub fn hilite(&self) -> Hilite {
        self.hilite
    }

    /// The selection's colours as a field of baud's draws them, a copy's
    /// pulse aside ([`Renderer::set_selection_pulse`]).
    pub(crate) fn field_hilite(&self) -> Hilite {
        self.field_hilite
    }

    /// **The selection's colours while the window is not key**, or `None`
    /// while it is: the macOS app sets AppKit's unemphasized pair here, the
    /// colours AppKit draws a selection in "when the associated
    /// control/view/window is not key" (`[[source: sdk27-nscolor-h]]`
    /// 232-235). Only reverse video takes it, the default: a selection
    /// colour of its own, the program's or the theme's, is drawn as it is.
    /// The next convert or resolve reads it, and a change repaints every
    /// row as a change of `OSC 17` does.
    pub fn set_inactive_selection(&mut self, pair: Option<Hilite>) {
        self.inactive = pair;
    }

    /// What [`Renderer::set_inactive_selection`] last set.
    pub fn inactive_selection(&self) -> Option<Hilite> {
        self.inactive
    }

    /// The video the last convert or resolve drew in: `?5`'s, or under a
    /// flash the other one. The cursor's colours a caller that draws the
    /// cursor itself -- the cell path -- reads are
    /// [`Renderer::block_paint`], resolved in this video.
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
        self.bauds = (Argb::default(), BaudCursor::dark());
        self.theme_gen = ThemeGen(self.theme_gen.0.wrapping_add(1));
        self.known = Prev::Unknown;
        self.defaults = self.theme.defaults(self.video, |_| None);
        self.page_default = self.theme.bg;
        let stored = (self.theme.fg, self.theme.bg);
        self.reversed = (self.theme.reversed_alpha(stored.0, stored.1), stored);
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
        let stored = self
            .theme
            .defaults(Video::Normal, |d| term.dynamic_color(d));
        self.page_default = stored.1;
        self.reversed = (self.reversed_of(stored), stored);
        // **baud's own cursor is the page drawn's**, a reversed video's or
        // a flash's included, while no program names a cursor colour: the
        // amber the core was told was the theme's page's, where a theme
        // file's reversed page may need another.
        let bauds = match (
            self.theme.cursor_made,
            term.program_paint(PaintRole::Cursor),
        ) {
            (CursorMade::Bauds | CursorMade::BaudsFill, ProgramPaint::Unset) => {
                let page = defaults.1.with_alpha(Chan::from_u8(0xFF));
                if self.bauds.0 != page {
                    self.bauds = (page, self.theme.bauds_cursor_on(page));
                }
                Some(self.bauds.1)
            }
            _ => None,
        };
        // The trail's colour is the block's: baud's own, else the core's
        // (a program's `OSC 12`, else the configured one it was told), else
        // the theme's where it is a colour, else the text, which a
        // program's automatic colour, the swap, takes too.
        let theme_fill = match term.program_paint(PaintRole::Cursor) {
            ProgramPaint::Automatic => None,
            _ => self.theme.cursor.and_then(CellPaint::colour),
        };
        let fill = match bauds {
            Some(b) => b.fill,
            None => term.cursor_color_in(self.video).map_or(
                theme_fill.map_or(defaults.0, |c| c.with_alpha(Chan::from_u8(0xFF))),
                |(r, g, b)| Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)),
            ),
        };
        if defaults != self.defaults || fill != self.cursor_fill {
            self.defaults = defaults;
            self.cursor_fill = fill;
            self.rebuild_overlays();
        }
        // **A change of the selection's colours repaints every row**: row
        // damage compares cells, renditions and where the selection lies,
        // and an `OSC 17` moves none of them. Rare, so the whole frame.
        // Each colour is the program's while it set one, else the
        // theme's, as `OSC 10`/`11` lay over the theme's defaults.
        let rgb = |c: Option<(u8, u8, u8)>| {
            c.map(|(r, g, b)| Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)))
        };
        // A program's automatic colour (kitty's `OSC 21` empty value)
        // is the terminal's own -- reverse video, the cell's colours --
        // over the theme's.
        let paint = |role, theme: Option<CellPaint<Argb>>| match term.program_paint(role) {
            ProgramPaint::Colour(c) => rgb(Some(c)).map(CellPaint::Colour),
            ProgramPaint::Automatic => None,
            ProgramPaint::Unset => theme,
        };
        let hilite = Hilite::of(
            paint(PaintRole::SelectionBackground, self.theme.selection),
            paint(PaintRole::SelectionForeground, self.theme.selection_text),
        );
        // Reverse video in a window that is not key is AppKit's
        // unemphasized pair; a colour of its own stays (note AZ).
        let hilite = match self.inactive {
            Some(pair) if hilite == Hilite::REVERSE => pair,
            _ => hilite,
        };
        // baud's own fields keep these colours through a pulse.
        self.field_hilite = hilite;
        // The copy signal's pulse: the selection in the message pair, the
        // pair read against the selection's own colours on the page.
        let hilite = match self.selection_pulse {
            crate::message::SelectionPulse::Off => hilite,
            crate::message::SelectionPulse::On => {
                let (text, page) = hilite.paint(defaults.1, defaults.0);
                let pair = self
                    .message_colours
                    .in_force(crate::message::Pair { page, text });
                Hilite::of(
                    Some(CellPaint::Colour(pair.page)),
                    Some(CellPaint::Colour(pair.text)),
                )
            }
        };
        if hilite != self.hilite {
            self.hilite = hilite;
            self.known = Prev::Unknown;
        }
        // **The cursor's colours**: the core's (a program's `OSC 12`, else
        // the configured colour it was told), else the theme's, which may
        // name the cell's own; and the theme's text under a block, which a
        // program's cursor colour leaves as it is, as kitty, alacritty,
        // ghostty and foot leave theirs, and kitty's `OSC 21 ; cursor_text`
        // sets. A program's automatic colour, `OSC 21`'s empty value, is the
        // cell's colours swapped, over the theme's. **A change repaints
        // every row**, as the selection's does: the cursor's cell compares
        // as unchanged when only its colours moved. Rare, so the whole
        // frame; the sized rows' key reads it too.
        let fill = match (bauds, term.program_paint(PaintRole::Cursor)) {
            (Some(b), _) => Some(CellPaint::Colour(b.fill)),
            (None, ProgramPaint::Automatic) => None,
            (None, _) => rgb(term.cursor_color_in(self.video))
                .map(CellPaint::Colour)
                .or(self.theme.cursor),
        };
        // The theme's text under the fill in force, which a kitty theme's
        // `cursor none` sets aside while the cursor has no colour; baud's
        // own text with baud's own fill, unless the config names one.
        let text = match (bauds, self.theme.cursor_made) {
            (Some(b), CursorMade::Bauds) => Some(CellPaint::Colour(b.text)),
            _ => self.theme.block_text(fill),
        };
        let block_paint = BlockPaint::of(fill, paint(PaintRole::CursorText, text));
        if block_paint != self.block_paint {
            self.block_paint = block_paint;
            self.known = Prev::Unknown;
        }
    }

    /// What a block cursor is drawn in on the last convert or resolve: the
    /// fill and the text under it ([`BlockPaint`]). What a caller that draws
    /// the cursor itself -- the cell path -- reads.
    pub fn block_paint(&self) -> BlockPaint {
        self.block_paint
    }

    /// The colour a bar or underline cursor takes over a cell whose page is
    /// `page` and whose text, without the line rule, is `text`: the cursor's
    /// fill, a colour or the cell's own, else the text.
    fn shape_colour(&self, page: Argb, text: Argb) -> Argb {
        match self.block_paint.fill {
            Some(CellPaint::Colour(c)) => c,
            Some(CellPaint::Page) => page.with_alpha(Chan::from_u8(0xFF)),
            Some(CellPaint::Text) | None => text,
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
        self.build_splash();
        self.build_trail();
        self.build_link();
        self.build_bell();
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

    /// How many sized rows [`Renderer::sized_lines`] has converted, rather
    /// than reused, since the renderer was made.
    pub fn sized_converted(&self) -> Tally {
        self.sized_converted
    }

    /// **How many rows a convert has walked for their syllables and words**
    /// since the renderer was made (`crate::syllable`): a row of no
    /// character a legacy cluster parts at or after and no Devanagari
    /// letter is let go by one pass of a block table and never counted,
    /// and a row whose cells a row kept holds (`crate::kept`) is not
    /// walked again.
    pub fn syllable_rows(&self) -> Tally {
        self.syllable_rows
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
        self.defaults.1.with_alpha(self.page_alpha(self.video))
    }

    /// **The alpha the default page is drawn at under `video`**: the
    /// window's dim, raised under `?5` as every reversed default page is
    /// ([`Theme::reversed_alpha`]), so the margin is the cells' page.
    fn page_alpha(&self, video: Video) -> Chan {
        match video {
            Video::Normal => self.theme.bg_alpha,
            Video::Reverse => self.reversed.0,
        }
    }

    /// [`Theme::reversed_alpha`] for the default pair as stored, `stored`:
    /// the one found already while the pair is the one it was found for.
    fn reversed_of(&self, stored: (Argb, Argb)) -> Chan {
        match self.reversed {
            (alpha, pair) if pair == stored => alpha,
            _ => self.theme.reversed_alpha(stored.0, stored.1),
        }
    }

    /// **The background colour, as iTerm2's colour map keeps it**: the one
    /// `OSC 11` set, else the theme's, opaque, in either video -- the
    /// colour iTerm2's badge outlines its text in
    /// (`kColorMapBackground`, `[[source: iterm2-964a75d-ptytextview]]`),
    /// which reverse video does not swap (`[[source:
    /// iterm2-964a75d-color-map]]`).
    pub fn background_of(&self, term: &Term<'_>) -> Argb {
        let (_, bg) = self
            .theme
            .defaults(Video::Normal, |d| term.dynamic_color(d));
        bg.with_alpha(Chan::from_u8(0xFF))
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
        let alpha = match video {
            Video::Normal => self.theme.bg_alpha,
            Video::Reverse => self.reversed_of(
                self.theme
                    .defaults(Video::Normal, |d| term.dynamic_color(d)),
            ),
        };
        bg.with_alpha(alpha)
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

    /// **Whether the exit-status strip is kept** ([`crate::status`]): on,
    /// the frame keeps a cell's width at its left for the marks
    /// ([`Renderer::strip`]), which a driver's window arithmetic counts as
    /// it counts the gutter.
    pub fn set_status_marks(&mut self, marks: crate::status::StatusMarks) {
        if marks != self.status {
            self.status = marks;
            self.strip_painted = None;
            if marks == crate::status::StatusMarks::Off {
                self.placed.clear();
                self.strip_inst.clear();
            }
        }
    }

    pub fn status_marks(&self) -> crate::status::StatusMarks {
        self.status
    }

    /// The strip's width: a cell's, while the marks are on; else none.
    pub fn strip(&self) -> Px {
        match self.status {
            crate::status::StatusMarks::On => self.metrics.cell_w,
            crate::status::StatusMarks::Off => Px::of(0),
        }
    }

    /// **The whole frame**: the left margin, the strip, then
    /// [`Renderer::frame_size`]'s grid and gutter, then the right margin,
    /// and the activity lights' line above or below all of it
    /// ([`Renderer::lights_height`]). What a driver allocates and a
    /// presenter centres; the grid is drawn [`Renderer::grid_left`] pixels
    /// in from its left and [`Renderer::grid_top`] down from its top.
    pub fn whole_size(&self) -> (Px, Px) {
        let (w, h) = self.frame_size();
        let beside = self
            .strip()
            .value()
            .saturating_add(self.margins().value().saturating_mul(2));
        (
            Px::of(w.value().saturating_add(beside)),
            Px::of(h.value().saturating_add(self.lights.height().value())),
        )
    }

    /// **Where the grid's frame starts in the whole frame**, in pixels
    /// from its left: past the left margin and the exit-status strip.
    pub fn grid_left(&self) -> Px {
        Px::of(self.margins().value().saturating_add(self.strip().value()))
    }

    /// **Whether the word wrap's continuation marks are drawn**
    /// (`[screen] wrap_marker`, [`crate::wrapmark`]): beside each line the
    /// display re-wrapped, in the margins the driver lends.
    pub fn set_wrap_marker(&mut self, marker: crate::wrapmark::WrapMarker) {
        if marker != self.wrap_marker {
            self.wrap_marker = marker;
            self.margin_painted = None;
            self.known = Prev::Unknown;
        }
    }

    pub fn wrap_marker(&self) -> crate::wrapmark::WrapMarker {
        self.wrap_marker
    }

    /// **The band of the window's padding each side lends the marks**, in
    /// pixels: the driver's padding, which the whole frame then holds as a
    /// margin at its left and its right, so the grid stays where it was and
    /// keeps every column. Lent only while the marks are drawn: with the
    /// word wrap or the marks off the margins are none
    /// ([`Renderer::margins`]).
    pub fn set_margins(&mut self, band: Px) {
        if band != self.margins {
            self.margins = band;
            self.margin_painted = None;
            self.known = Prev::Unknown;
        }
    }

    /// The margin at each side of the whole frame: the band lent, while the
    /// word wrap and its marks are both on; else none.
    pub fn margins(&self) -> Px {
        match (self.word_wrap, self.wrap_marker) {
            (crate::WordWrap::On, crate::wrapmark::WrapMarker::On) => self.margins,
            _ => Px::of(0),
        }
    }

    /// **The activity lights' own line**, at `edge` of the whole frame and
    /// drawn at `scale` device pixels a point, or none: the line the
    /// VT420 kept for its keyboard indicators, "at the bottom of the
    /// screen, below the status line" (`[[source: vt420-uu-ch4]]`), or the
    /// same line at the top. It is outside [`Renderer::frame_size`], as the
    /// exit-status strip is: no row of the grid, the status line's or the
    /// notice's, so nothing a program is told, nothing the scrollbar spans
    /// and nothing a smooth scroll slides holds it, and a driver's window
    /// arithmetic counts it as it counts the strip. Only as tall as the
    /// lights and their margins ([`crate::LineRows`]), never a text row.
    pub fn set_lights_line(&mut self, edge: Option<crate::LineEdge>, scale: f32) {
        let line = edge.and_then(|edge| {
            Some(crate::lights::Line {
                edge,
                rows: crate::LineRows::at(scale)?,
                scale,
            })
        });
        if line != self.lights.line {
            self.lights.line = line;
            self.lights.painted = None;
            self.lights.inst.clear();
            self.known = Prev::Unknown;
        }
    }

    /// The edge the activity lights' line is at, or none.
    pub fn lights_edge(&self) -> Option<crate::LineEdge> {
        self.lights.line.map(|l| l.edge)
    }

    /// The activity lights' line's height in pixels: none without one.
    pub fn lights_height(&self) -> Px {
        self.lights.height()
    }

    /// **Where the grid's frame starts in the whole frame**, in pixels from
    /// its top: below the lights' line when it is at the top, else at the
    /// top.
    pub fn grid_top(&self) -> Px {
        match self.lights_edge() {
            Some(crate::LineEdge::Top) => self.lights.height(),
            _ => Px::of(0),
        }
    }

    /// **Where the lights' line starts in the whole frame**, in pixels from
    /// its top: the top, or below everything [`Renderer::frame_size`] holds
    /// -- the grid, the status line and the notice.
    pub fn lights_top(&self) -> Px {
        match self.lights_edge() {
            Some(crate::LineEdge::Top) => Px::of(0),
            _ => self.frame_size().1,
        }
    }

    /// **What the lights show and how a lit one is drawn**, from the
    /// driver's clocks ([`crate::Activity`]) and its Dim Flashing Lights
    /// setting: the next [`Renderer::place_lights`] lays them out.
    pub fn set_lights(&mut self, shown: crate::Shown, glow: crate::Glow) {
        self.lights.shown = shown;
        self.lights.glow = glow;
    }

    /// **Which palette entries the lights are lit in and how strongly they
    /// glow**, `[window] activity_lights_colours` and `activity_lights_glow`:
    /// the set and the halo every look is made with, on the lights' own
    /// line and in the title strip ([`Renderer::lights_look`]).
    pub fn set_lights_colours(&mut self, set: crate::LightsColours, halo: crate::Halo) {
        self.lights.colours = set;
        self.lights.halo = halo;
    }

    /// **The lights' look in this theme**, glowing at `glow`: every lamp
    /// one of the theme's sixteen entries in the configured set, on a
    /// plate of its background, its letters its foreground
    /// ([`crate::LightsLook::of`]). The theme's own colours, not the
    /// screen's: reverse video and a program's `OSC 4` move the program's
    /// colours, not baud's plate and lamps.
    pub fn lights_look(&self, glow: crate::Glow) -> crate::LightsLook {
        crate::LightsLook::of(
            &self.theme.palette,
            self.theme.fg,
            self.theme.bg,
            self.lights.colours,
            glow,
            self.lights.halo,
        )
    }

    /// **The lights' records for the next frame** ([`crate::lights::LineState`]):
    /// their look from the theme ([`Renderer::lights_look`]) -- the plate
    /// the theme's background on the line's band, which is the page as the
    /// next convert resolves it, in either video, as the VT420's line was
    /// "in the same video background as the main display" -- the plate and
    /// each placed light naming its picture on the colour sheet, drawn there
    /// once a look. Every path draws them: the CPU path in
    /// [`Renderer::render_at`], which calls this itself, the GPU paths from
    /// [`Renderer::lights_records`] in a pass over the line.
    pub fn place_lights(&mut self, _term: &Term<'_>) {
        if self.lights.line.is_none() {
            self.lights.inst.clear();
            return;
        }
        let key = crate::lights::LookKey {
            look: self.lights_look(self.lights.glow),
            scale: self.lights.line.map_or(0.0, |l| l.scale),
        };
        // The line's width less the right margin, so the lights keep their
        // place against the grid's frame however wide the margins are.
        let width = Px::of(
            self.whole_size()
                .0
                .value()
                .saturating_sub(self.margins().value()),
        );
        let face = self.stack.face(self.stack.badge());
        self.lights.place(face, &mut self.atlas, key, width);
    }

    /// The lights' records, in the line's own pixels: x from its left, y
    /// from its top.
    pub fn lights_records(&self) -> &[Instance] {
        &self.lights.inst
    }

    /// **Each placed light's left and width on the line**, in its pixels:
    /// where a driver puts the light's hover text ([`crate::Led::tip`]).
    pub fn lights_places(&self) -> crate::ByLed<Option<(Px, Px)>> {
        self.lights.places
    }

    /// Paints the lights' line into `target`, the line's own rows: the
    /// clear colour, then the lights, when either changed since the last
    /// paint or the frame is new.
    fn paint_lights(&mut self, target: &mut Target<'_>, damage: Damage) {
        let clear = self.clear_color();
        let fresh = match &self.lights.painted {
            Some((c, insts)) => damage == Damage::Full || *c != clear || *insts != self.lights.inst,
            None => true,
        };
        if !fresh {
            return;
        }
        self.lights.fresh = crate::lights::Fresh::Yes;
        let all = Band {
            top: Px::of(0),
            bottom: self.lights.height(),
        };
        target.fill_band(all, &[], self.metrics.cell_w, clear);
        let page = self.page();
        for inst in &self.lights.inst {
            target.over(inst, &self.atlas, self.blend, page, all);
        }
        self.lights.painted = Some((clear, self.lights.inst.clone()));
    }

    /// **The marks for the next frame**, read from the prompts on the
    /// window `back` lines up the history (`Term::prompts_from`): each in
    /// its palette entry over the strip, at its row's top, lowered by `pan`
    /// and with the line above the window when a smooth scroll draws it
    /// (`leaving`), as the cell path's grid is. Every path draws them: the
    /// CPU path in [`Renderer::render_at`], the GPU paths from
    /// [`Renderer::strip_records`] in a pass over the strip.
    pub fn place_marks(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        leaving: crate::status::Leaving,
        pan: Px,
    ) {
        self.strip_inst.clear();
        self.place_wrap_marks(term, back, leaving, pan);
        if self.status == crate::status::StatusMarks::Off {
            return;
        }
        let mut placed = core::mem::take(&mut self.placed);
        crate::status::placed(term, back, self.main_rows(), leaving, &mut placed);
        // The window as the display draws it now: a frame places its marks
        // before it lays its rows out.
        let shown = self.bidi.display_now(term, back);
        let cell = crate::boxes::Cell {
            w: self.metrics.cell_w,
            h: self.metrics.cell_h,
        };
        let (ch, base) = (px_i32(self.metrics.cell_h), px_i32(self.metrics.baseline));
        let lift = px_i32(pan);
        for p in &placed {
            let Some(slot) = self.atlas.status_mark(p.mark, cell, self.metrics.baseline) else {
                continue;
            };
            let top = match p.row {
                crate::status::ViewRow::Leaving => -ch,
                // On the display row its prompt is drawn on, where the
                // display re-broke a paragraph above it.
                crate::status::ViewRow::On(r) => {
                    match shown.as_ref().map_or(Some(r), |d| d.row_of(r)) {
                        Some(d) => i32::try_from(self.band(d).top.value()).unwrap_or(i32::MAX),
                        None => continue,
                    }
                }
            };
            let color = self.theme.entry(p.mark.slot(), |c| term.palette(c));
            self.strip_inst.push(Instance {
                x: screen(slot.left),
                y: screen(top + lift + base + slot.top),
                w: slot.w,
                h: slot.h,
                u: slot.x,
                v: slot.y,
                color,
                kind: slot.kind,
                // The identity: a mark is a shape, not text to bend.
                curve: CurveRow::default(),
            });
        }
        self.placed = placed;
    }

    /// The marks' records, in the strip's own pixels: x from its left edge,
    /// y as the grid's rows.
    pub fn strip_records(&self) -> &[Instance] {
        &self.strip_inst
    }

    /// **The word wrap's continuation marks for the next frame**
    /// ([`crate::wrapmark`]), placed by [`Renderer::place_marks`]: one in
    /// the margin at the start side of each line of the window that
    /// continues a paragraph the display re-wrapped at its words, at that
    /// line's top, lowered by `pan` and with the line above the window when
    /// a smooth scroll draws it (`leaving`), as the strip's marks are. None
    /// on the alternate screen, with no display, and where the margins
    /// have no room ([`crate::wrapmark::room`]).
    fn place_wrap_marks(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        leaving: crate::status::Leaving,
        pan: Px,
    ) {
        self.margin_inst.clear();
        let band = self.margins();
        let Some(w) = crate::wrapmark::room(band, self.metrics.cell_w) else {
            return;
        };
        if term.screen() == baud_core::Screen::Alternate {
            return;
        }
        let Some(shown) = self.bidi.display_now(term, back) else {
            return;
        };
        let page = self.clear_color_of(term);
        let color = crate::color::reads_at(
            self.theme
                .entry(Chan::from_u8(8), |c| term.palette(c))
                .with_alpha(Chan::from_u8(0xFF)),
            page.with_alpha(Chan::from_u8(0xFF)),
            page.a(),
            self.theme.backdrop,
            crate::Floor::GLASS_TRIGGER,
        );
        let cell = crate::boxes::Cell {
            w: self.metrics.cell_w,
            h: self.metrics.cell_h,
        };
        let (ch, base) = (px_i32(self.metrics.cell_h), px_i32(self.metrics.baseline));
        let lift = px_i32(pan);
        let whole = px_i32(self.whole_size().0);
        // The band's left, and the box's inside it, centred.
        let inset = (px_i32(band) - px_i32(w)) / 2;
        // The line above the window when a smooth scroll draws it, then
        // the window's rows, each with its top.
        let leaving_line = match leaving {
            crate::status::Leaving::Drawn => Some((Anchor::Above(Rows::of_count(1)), -ch)),
            crate::status::Leaving::None => None,
        };
        let window = (0..self.main.index()).map(|y| {
            let top = i32::try_from(y).unwrap_or(i32::MAX).saturating_mul(ch);
            (Anchor::At(Row::from_index(y)), top)
        });
        for (at, top) in leaving_line.into_iter().chain(window) {
            let Some(dir) = shown.continues(at) else {
                continue;
            };
            let side = crate::wrapmark::Side::of(dir);
            let Some(slot) = self.atlas.wrap_mark(side, w, cell, self.metrics.baseline) else {
                continue;
            };
            let x = match side {
                crate::wrapmark::Side::Left => inset,
                crate::wrapmark::Side::Right => whole - px_i32(band) + inset,
            };
            self.margin_inst.push(Instance {
                x: screen(x + slot.left),
                y: screen(top + lift + base + slot.top),
                w: slot.w,
                h: slot.h,
                u: slot.x,
                v: slot.y,
                color,
                kind: slot.kind,
                // The identity: a mark is a shape, not text to bend.
                curve: CurveRow::default(),
            });
        }
    }

    /// **The continuation marks' records**, in the whole frame's pixels: x
    /// from its left edge, the margins at its two ends, y as the grid's
    /// rows. What a GPU path draws over the grid's rows across the whole
    /// width, after the strip's.
    pub fn margin_records(&self) -> &[Instance] {
        &self.margin_inst
    }

    /// Paints the margins into `target`, the whole frame's width over the
    /// grid's rows: the clear colour in each margin, then the marks, when
    /// either changed since the last paint or the frame is new.
    fn paint_margins(&mut self, target: &mut Target<'_>, damage: Damage) {
        let band = self.margins();
        if band.value() == 0 {
            return;
        }
        let clear = self.clear_color();
        let fresh = match &self.margin_painted {
            Some((c, insts)) => damage == Damage::Full || *c != clear || *insts != self.margin_inst,
            None => true,
        };
        if !fresh {
            return;
        }
        self.margin_fresh = StripFresh::Yes;
        let w = self.whole_size().0;
        let all = Band {
            top: Px::of(0),
            bottom: self.frame_size().1,
        };
        for left in [Px::of(0), Px::of(w.value().saturating_sub(band.value()))] {
            if let Some(mut own) = target.columns(left, band) {
                own.fill_band(all, &[], self.metrics.cell_w, clear);
            }
        }
        let page = self.page();
        for inst in &self.margin_inst {
            target.over(inst, &self.atlas, self.blend, page, all);
        }
        self.margin_painted = Some((clear, self.margin_inst.clone()));
    }

    /// Paints the strip into `target`, the strip's own columns: the clear
    /// colour the GPU passes clear to, then the marks, when either changed
    /// since the last paint or the frame is new.
    fn paint_strip(&mut self, target: &mut Target<'_>, damage: Damage) {
        let clear = self.clear_color();
        let fresh = match &self.strip_painted {
            Some((c, insts)) => damage == Damage::Full || *c != clear || *insts != self.strip_inst,
            None => true,
        };
        if !fresh {
            return;
        }
        self.strip_fresh = StripFresh::Yes;
        let (_, h) = self.frame_size();
        let all = Band {
            top: Px::of(0),
            bottom: h,
        };
        target.fill_band(all, &[], self.metrics.cell_w, clear);
        let page = self.page();
        for inst in &self.strip_inst {
            target.over(inst, &self.atlas, self.blend, page, all);
        }
        self.strip_painted = Some((clear, self.strip_inst.clone()));
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
        self.cur.resize(cols, rows);
        self.prev_size = vec![LineRendition::Single; rows.index()];
        self.bg = vec![Argb(0); cells];
        self.fg = vec![Argb(0); cells];
        self.curve = vec![CurveRow::default(); cells];
        self.text = vec![Text::Drawn; cells];
        self.worded = vec![crate::word::Worded::No; cells];
        self.laid = vec![Vec::new(); rows.index()];
        self.row_worded = vec![crate::word::Worded::No; rows.index()];
        self.row_blinks = vec![Blinking::UNSEEN; rows.index()];
        self.per_row.reset(rows);
        self.glyph_ends = vec![0; rows.index()];
        self.reach = vec![Reach::NONE; rows.index()];
        self.reach_was = vec![Reach::NONE; rows.index()];
        self.dirty = vec![false; rows.index()];
        self.repaint = vec![Repaint::Whole; rows.index()];
        self.known = Prev::Unknown;
        self.kept.fit(rows);
    }

    fn row_cells(&self, r: Row) -> core::ops::Range<usize> {
        let cols = self.cols.index();
        r.index() * cols..(r.index() + 1) * cols
    }

    /// **The columns the trail's and the splash's records reach on
    /// `band`'s rows**, this frame's and the last's: [`Repaint::Cols`], empty
    /// when none of them meets it. A quad's or a sweep's one record is its
    /// bounds, the window's width on a jump across it, so a smear gives the
    /// columns its shape can cover on the rows instead ([`Smear::columns`]),
    /// which hold every pixel it draws.
    fn records_on(&self, band: Band) -> Repaint {
        let smear = |d: &Option<Drawn>| match d {
            Some(Drawn::Smear(s)) => Some(*s),
            _ => None,
        };
        let (now, was) = (smear(&self.trail), smear(&self.last_trail));
        let (mut lo, mut hi) = (i64::MAX, i64::MIN);
        // `build_trail` puts a smear's record first.
        for r in self
            .trail_inst
            .iter()
            .skip(usize::from(now.is_some()))
            .chain(self.last_trail_inst.iter().skip(usize::from(was.is_some())))
            .chain(&self.splash_inst)
            .chain(&self.last_splash_inst)
            .filter(|r| record_meets(r, band))
        {
            lo = lo.min(i64::from(r.x.0));
            hi = hi.max(i64::from(r.x.0) + i64::from(r.w.0));
        }
        for (left, right) in now
            .iter()
            .chain(&was)
            .filter_map(|s| s.columns(band.top, band.bottom))
        {
            lo = lo.min(i64::from(left.value()));
            hi = hi.max(i64::from(right.value()));
        }
        let px = |v: i64| Px::of(u32::try_from(v.max(0)).unwrap_or(u32::MAX));
        match lo < hi {
            true => Repaint::Cols {
                left: px(lo),
                right: px(hi),
            },
            false => Repaint::Cols {
                left: Px::of(0),
                right: Px::of(0),
            },
        }
    }

    fn band(&self, r: Row) -> Band {
        let h = self.metrics.cell_h.value();
        let top = u32::try_from(r.index()).unwrap_or(0).saturating_mul(h);
        Band {
            top: Px::of(top),
            bottom: Px::of(top.saturating_add(h)),
        }
    }

    /// **The pixel rows the last [`Renderer::render`] wrote**, top to
    /// bottom: each run of repainted rows as one band. An image is blitted
    /// only into the rows a frame repaints, and a row one arrived on, left
    /// or changed on is repainted for it (`render_at`), so its pixels are in
    /// these runs. What a presenter copies and tells its compositor
    /// changed; a pixel outside every band is the frame before's. Bands may
    /// overlap.
    pub fn painted(&self) -> impl Iterator<Item = Band> + '_ {
        let h = self.metrics.cell_h.value();
        let rows = self.rows.index().min(self.dirty.len());
        let mut r = 0;
        let runs = core::iter::from_fn(move || {
            while r < rows && !self.dirty[r] {
                r += 1;
            }
            let start = r;
            while r < rows && self.dirty[r] {
                r += 1;
            }
            let px = |i: usize| Px::of(u32::try_from(i).unwrap_or(u32::MAX).saturating_mul(h));
            (start < r).then(|| Band {
                top: px(start),
                bottom: px(r),
            })
        });
        let strip = (self.strip_fresh == StripFresh::Yes || self.margin_fresh == StripFresh::Yes)
            .then_some(Band {
                top: Px::of(0),
                bottom: self.frame_size().1,
            });
        // In the whole frame: below the lights' line when it is at the top,
        // and the line itself when this frame repainted it.
        let down = self.grid_top();
        let line = (self.lights.fresh == crate::lights::Fresh::Yes).then(|| {
            let top = self.lights_top();
            Band {
                top,
                bottom: Px::of(top.value().saturating_add(self.lights.height().value())),
            }
        });
        runs.chain(strip)
            .map(move |b| Band {
                top: Px::of(b.top.value().saturating_add(down.value())),
                bottom: Px::of(b.bottom.value().saturating_add(down.value())),
            })
            .chain(line)
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
        // **An image is part of the rows a frame repaints.** Images drew
        // only on a full frame, and a live terminal renders incrementally
        // after its first, so on the Linux driver, which is CPU-only, and
        // on the macOS CPU path, a Sixel a program printed did not appear
        // until a resize. So the placements are computed first, and every
        // row a placement arrived on, changed on or left since the last CPU
        // frame is repainted (`arrived_on`, `left_from`); a still image's
        // rows are not. A repainted row draws its images with its pixels:
        // one under the text inside the row's own pass, over its
        // background and under its glyphs, and one over the text after the
        // row's glyphs, the trail and the glow and before baud's overlays,
        // through the row's own target (`RowTarget`), so into its band and
        // the columns it repaints alone. No placement is ever blitted over
        // its own last blit -- which a partly transparent one
        // (`ImageAlpha::Partial`) would blend twice -- nor over an overlay
        // drawn on an earlier frame, and a frame that repaints no row an
        // image covers blits none of it.
        //
        // **One write generation closed a frame, before anything reads**
        // (`Term::seal_writes`): the copy the convert keeps is taken under
        // it, and the next frame takes again only the rows written after.
        let _ = term.seal_writes();
        self.fit(term);
        // A frame with the splash's layers, or the one after it, repaints
        // whole: no row's reach counts their records.
        let damage = match self.splash_layers.is_empty() && self.last_splash_layers.is_empty() {
            true => damage,
            false => Damage::Full,
        };
        self.place_images(term, back, Snapshots::Skipped);
        let converted = self.convert_with(term, back, damage, ImageRows::Repainted);
        self.strip_fresh = StripFresh::No;
        self.margin_fresh = StripFresh::No;
        self.lights.fresh = crate::lights::Fresh::No;
        // With the activity lights' line kept, `target` is the whole frame:
        // the rest is painted into the rows beside the line, and the line on
        // its own.
        let line = self.lights.height();
        if line.value() == 0 {
            return self.paint_beside(term, target, converted, damage);
        }
        self.place_lights(term);
        let (top, h) = (self.grid_top(), self.frame_size().1);
        let frame = match target.rows(top, h) {
            Some(mut rest) => self.paint_beside(term, &mut rest, converted, damage),
            None => Frame::default(),
        };
        if let Some(mut own) = target.rows(self.lights_top(), line) {
            self.paint_lights(&mut own, damage);
        }
        frame
    }

    /// [`Renderer::render_at`]'s paint of the rows beside the lights' line:
    /// the grid, and the exit-status strip beside it when kept.
    fn paint_beside(
        &mut self,
        term: &Term<'_>,
        target: &mut Target<'_>,
        converted: Converted,
        damage: Damage,
    ) -> Frame {
        let under = self.image_places.iter().any(|p| p.layer.under_text());
        let over = Some(term.images());
        // With the exit-status strip or the margins kept, `target` is the
        // whole frame's width: the grid is painted into the columns past the
        // left margin and the strip, the strip on its own, and the margins.
        let (strip, band) = (self.strip(), self.margins());
        if strip.value() == 0 && band.value() == 0 {
            return match under {
                true => self.paint_rows(target, converted, UnderImages(term.images()), over),
                false => self.paint_rows(target, converted, NoUnder, over),
            };
        }
        let (w, _) = self.frame_size();
        let frame = match target.columns(self.grid_left(), w) {
            Some(mut grid) => match under {
                true => self.paint_rows(&mut grid, converted, UnderImages(term.images()), over),
                false => self.paint_rows(&mut grid, converted, NoUnder, over),
            },
            None => Frame::default(),
        };
        if strip.value() > 0 {
            if let Some(mut own) = target.columns(band, strip) {
                self.paint_strip(&mut own, damage);
            }
        }
        self.paint_margins(target, damage);
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
    /// the CPU path's row blits and the two GPU arms all read the list and its
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
        self.virtual_at.clear();
        self.image_fill = self.fill_pixel(term);
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
        // The window as the display draws it now, whichever layout ran
        // last: a frame may place its pictures before it lays its rows out.
        let shown = self.bidi.display_now(term, back);
        let cw = i64::from(self.metrics.cell_w.value());
        let ch = i64::from(self.metrics.cell_h.value());
        let cell = |p: Px| ImagePx::of(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let (cell_w, cell_h) = (cell(self.metrics.cell_w), cell(self.metrics.cell_h));
        let screen_h = self.main.index() as i64 * ch;
        let frame_w = i64::from(self.frame_size().0.value());
        let grid_w = self.cols.index() as i64 * cw;
        for (pid, p) in imgs.placements() {
            if p.screen != origin.screen {
                continue;
            }
            // Under a virtual parent: placed below, from its placeholders.
            if p.kitty.parent.is_relative()
                && !matches!(imgs.kitty_root(pid), KittyRoot::Screen { .. })
            {
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
            // A relative placement left of the first column or above the
            // first line hangs off the edge by what its anchor was clamped by.
            let (over_c, over_r) = (
                p.kitty.over_cols.index() as i64,
                p.kitty.over_rows.index() as i64,
            );
            let x0 = (p.col.index() as i64 - over_c) * cw + i64::from(dest.x);
            let y0 = (p.line.value() as i64 - over_r - top) * ch - cut_top + i64::from(dest.y);
            // **On a row drawn right to left, the box mirrored**: its
            // anchor row's direction places the whole box,
            // `cols - (col + w)` from the left, and its pixels are not
            // flipped -- terminal-wg's "images must not be flipped by
            // default in RTL mode ... In paragraphs of RTL direction, the
            // overall layout should be the mirrored version of the LTR
            // one" (`[[source: terminal-wg-bidi-misc]]`), the anchor being
            // where the picture is in the text, UAX #9 3.2's object
            // (`[[source: uax9-bidi]]`).
            let anchor = match p.line.value().checked_sub(origin.top.value()) {
                Some(y) => Anchor::At(Row::from_index(usize::try_from(y).unwrap_or(usize::MAX))),
                None => Anchor::Above(Rows::of_index(
                    usize::try_from(origin.top.value() - p.line.value()).unwrap_or(0),
                )),
            };
            // Where the display re-broke the anchor row's paragraph
            // ([`crate::rebreak`]), the direction of the line the row is
            // drawn on, the one it moves the picture to below: a run the
            // display gave lines of its own runs its own way.
            let dir = shown
                .as_ref()
                .and_then(|d| d.line_direction(anchor))
                .unwrap_or_else(|| self.anchor_direction(term, back, anchor));
            let x0 = match dir {
                BidiDirection::Rtl => grid_w - x0 - i64::from(dest.w),
                BidiDirection::Ltr => x0,
            };
            // **Where the display re-broke a paragraph** ([`crate::rebreak`]),
            // the picture moved with its anchor row's text, as a scroll moves
            // it: the row's own move where it is one of the window's
            // paragraphs, pushed above the window or not, and the move of
            // the text above the window where it is anchored higher.
            let y0 = match &shown {
                Some(d) => y0 + d.shift(anchor) as i64 * ch,
                None => y0,
            };
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
                p.serial,
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
            // **Where each run is drawn**: the cells are characters, and a
            // row drawn in another order moves them, so a strip is placed
            // over its run's cells where the order draws them, its pixels
            // unflipped. The window is laid out for it here, the frame's
            // own layout coming after the placements, with the display's
            // wrap the frame draws ([`crate::rebreak`]), and its rows read
            // as that layout draws them.
            let mut order = self.placeholder_bidi.take().unwrap_or_default();
            order.set_word_wrap(self.word_wrap);
            order.lay_out_as(term, back, crate::bidi::Joins::Unseen);
            let vp = order.view(term, back);
            // The leaving row too, draw row -1: the line the cell path
            // slides in above the window, the first line of the window one
            // line up as its layout draws it ([`Renderer::copy_leaving`]),
            // in that order -- where the display re-broke a paragraph below
            // it, that display line, not the stored row above the window.
            // Only where the history holds one: the clamp would read the
            // screen's own first row there.
            let above = Rows::of_count(back.index() + 1);
            let mut up = self.placeholder_leaving.take().unwrap_or_default();
            let leaving = term.looking_back(above) == above;
            if leaving {
                up.set_word_wrap(self.word_wrap);
                up.lay_out_top(term, above, crate::bidi::Joins::Unseen);
            }
            let up_vp = up.view(term, above);
            let leaving = leaving.then(|| {
                let top = Row::ZERO;
                let drawn = up.orders().reordered(top).then(|| (up.orders(), top));
                (
                    -1,
                    up_vp.row(top),
                    up_vp.rendition(top),
                    drawn,
                    up_vp.styles(top),
                )
            });
            let rows = (0..self.main.index()).map(|r| {
                let y = Row::from_index(r);
                let drawn = order.orders().reordered(y).then(|| (order.orders(), y));
                (r as i64, vp.row(y), vp.rendition(y), drawn, vp.styles(y))
            });
            for (r, row, size, drawn, styles) in leaving.into_iter().chain(rows) {
                self.place_placeholders(term, imgs, epoch, (r, row, size, styles), drawn, snap);
            }
            self.placeholder_leaving = Some(up);
            self.placeholder_bidi = Some(order);
            self.place_under_virtuals(imgs, epoch, origin.screen, snap);
        }
        // The text's order: by `z`, then by the client's id; then, where the
        // text leaves it undefined, kitty's: by when the image was stored,
        // then by when the placement was made (`ImagePlacement::born`).
        self.image_places
            .sort_by_key(|p| (p.z, p.tie.raw(), p.born, p.serial));
        self.place_rows.built = Built::Stale;
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
    ///
    /// **On a line that is not single-width (`size`) a placeholder is drawn
    /// as the character it is**: each cell's part of the image fills the box
    /// its character takes there, twice as wide, and on a double-height line
    /// twice as tall with each row showing its half
    /// (`[[source: dec-std-070]]` § 5.6.1, DECDWL displays "all characters
    /// ... in double-width rendition"). kitty draws no sized line. **Which
    /// way the row a picture is anchored on is drawn**: row `r` of the
    /// window `back` lines into the history, a row above it read in the
    /// window that shows it first (a tall picture whose top has scrolled off
    /// keeps its place), and left to right for a line the history no longer
    /// holds.
    fn anchor_direction(&self, term: &Term<'_>, back: Rows, at: Anchor) -> BidiDirection {
        let back = term.looking_back(back);
        match at {
            Anchor::At(y) => term.line_direction(back, y),
            Anchor::Above(k) => {
                let up = Rows::of_index(back.index() + k.index());
                match term.looking_back(up) == up {
                    true => term.line_direction(up, Row::ZERO),
                    false => BidiDirection::Ltr,
                }
            }
        }
    }

    fn place_placeholders(
        &mut self,
        term: &Term<'_>,
        imgs: &ImageStore<'_>,
        epoch: ImageGen,
        (r, row, size, styles): (
            i64,
            &[PackedCell],
            LineRendition,
            baud_domain::RowStyles<'_>,
        ),
        drawn: Option<(&crate::bidi::Orders, Row)>,
        snap: Snapshots,
    ) {
        let st = Stretch::of(size);
        let (sx, sy) = (i64::from(st.x()), i64::from(st.y()));
        let (ux, uy) = (st.x().unsigned_abs(), st.y().unsigned_abs());
        let cw = i64::from(self.metrics.cell_w.value());
        let ch = i64::from(self.metrics.cell_h.value());
        let cell = |p: Px| ImagePx::of(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let (cell_w, cell_h) = (cell(self.metrics.cell_w), cell(self.metrics.cell_h));
        let frame_w = i64::from(self.frame_size().0.value());
        for run in term.placeholder_runs(row, styles) {
            let Some(v) = term.kitty_virtual(run.image, run.placement) else {
                continue;
            };
            let col = run.at.index() as i64 * sx;
            match self.virtual_at.iter_mut().find(|e| e.0 == v.serial) {
                Some(e) => (e.1, e.2) = (e.1.min(r), e.2.min(col)),
                None => self.virtual_at.push((v.serial, r, col)),
            }
            let Some(view) = imgs.image(v.image) else {
                continue;
            };
            let mut draw = v.place.draw(view.width, view.height, cell_w, cell_h);
            // Scaled with the line's characters: the box a cell is drawn in.
            draw.dest = baud_domain::PxRect {
                x: draw.dest.x.saturating_mul(ux),
                y: draw.dest.y.saturating_mul(uy),
                w: draw.dest.w.saturating_mul(ux),
                h: draw.dest.h.saturating_mul(uy),
            };
            let dest = draw.dest;
            if dest.is_empty() {
                continue;
            }
            // The top of the boxes on this row: a row above it for the
            // bottom half of a double-height line.
            let box_top = match size {
                LineRendition::DoubleBottom => (r - 1) * ch,
                _ => r * ch,
            };
            // The run's first column as drawn: the least visual column of
            // its cells, on a row drawn in another order.
            let at = match drawn {
                Some((o, y)) => {
                    let last = Col::at_index(run.at.index() + run.cells.index().saturating_sub(1));
                    o.visual_col(y, run.at).min(o.visual_col(y, last)).index() as i64
                }
                None => run.at.index() as i64,
            };
            let x0 = (at - run.col.index() as i64) * cw * sx + i64::from(dest.x);
            let y0 = box_top - run.row.index() as i64 * ch * sy + i64::from(dest.y);
            let (left, top) = (at * cw * sx, r * ch);
            let right = left + run.cells.index() as i64 * cw * sx;
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
                v.serial,
                snap,
            );
        }
    }

    /// **The relative placements under a virtual parent**, each placed at
    /// the least row and column of its root's placeholders found this
    /// frame plus the offsets down its chain, and not drawn while none of
    /// them is on screen, as kitty keeps but does not draw one
    /// (`[[source: kitty-graphics-c]]` `grman_update_layers`).
    fn place_under_virtuals(
        &mut self,
        imgs: &ImageStore<'_>,
        epoch: ImageGen,
        screen: ImageScreen,
        snap: Snapshots,
    ) {
        if self.virtual_at.is_empty() {
            return;
        }
        let cw = i64::from(self.metrics.cell_w.value());
        let ch = i64::from(self.metrics.cell_h.value());
        let cell = |p: Px| ImagePx::of(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let (cell_w, cell_h) = (cell(self.metrics.cell_w), cell(self.metrics.cell_h));
        let screen_h = self.main.index() as i64 * ch;
        let frame_w = i64::from(self.frame_size().0.value());
        let mut under = Vec::new();
        for (pid, p) in imgs.placements() {
            if p.screen != screen || !p.kitty.parent.is_relative() {
                continue;
            }
            if let KittyRoot::Virtual { serial, h, v } = imgs.kitty_root(pid) {
                if let Some(&(_, r, c)) = self.virtual_at.iter().find(|e| e.0 == serial) {
                    under.push((*p, r + v, c + h));
                }
            }
        }
        for (p, row, col) in under {
            let Some(view) = imgs.image(p.image) else {
                continue;
            };
            let draw = p.kitty.draw(view.width, view.height, cell_w, cell_h);
            let dest = draw.dest;
            if dest.is_empty() {
                continue;
            }
            let x0 = col * cw + i64::from(dest.x);
            let y0 = row * ch + i64::from(dest.y);
            let (i0, i1) = ((-x0).max(0), i64::from(dest.w).min(frame_w - x0));
            let (j0, j1) = ((-ch - y0).max(0), i64::from(dest.h).min(screen_h - y0));
            self.push_image(
                imgs,
                epoch,
                p.image,
                &view,
                draw,
                (x0, y0),
                (i0, i1, j0, j1),
                p.z,
                p.serial,
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
        serial: PlaceSerial,
        snap: Snapshots,
    ) {
        if i0 >= i1 || j0 >= j1 {
            return;
        }
        crate::counters::add(Counter::ImageTablesBuilt, Site::Images, Tally::of(1));
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
        let ground = match view.ground {
            ImageGround::Clear => Pixel::CLEAR,
            ImageGround::Filled { .. } => self.image_fill,
        };
        // A fill drawn at the window's dim: over what is under it, a
        // partial image that is blended, and repainted under each blit; or
        // replacing it, which a second blit leaves as the first did.
        let (alpha, composite) = match (view.ground, self.page(), self.theme.sixel_bg) {
            (ImageGround::Filled { .. }, Page::Glass, crate::SixelBg::Dimmed) => {
                (ImageAlpha::Partial, ImageComposite::Over)
            }
            (ImageGround::Filled { .. }, Page::Glass, crate::SixelBg::Cleared) => {
                (view.alpha, ImageComposite::Replace)
            }
            _ => (view.alpha, ImageComposite::Over),
        };
        self.image_places.push(ImagePlacement {
            id,
            epoch,
            frame: view.frame,
            x: clamp16(x0 + i0),
            y: clamp16(y0 + j0),
            w: side(i1 - i0),
            h: side(j1 - j0),
            alpha,
            z,
            layer: ImageLayer::of(z),
            tie: imgs.kitty_names(id).map_or(KittyId::NONE, |(k, _)| k),
            born: imgs.born(id).unwrap_or(ImageAge::ZERO),
            serial,
            draw,
            scale,
            map: at,
            ground,
            composite,
        });
        // Snapshot the pixels once per referenced `(id, epoch, ground)`:
        // the GPU reads this to upload, the CPU reads the term itself. The
        // words are the store's own, straight, but a fill word, which is
        // drawn as `ground`: the fragment premultiplies, so a clear
        // (unpainted Sixel) pixel, `0x00000000`, leaves the cell showing
        // through and a partial one blends by its alpha.
        if snap == Snapshots::Kept
            && !self.image_uploads.iter().any(|u| {
                u.id == id && u.epoch == epoch && u.frame == view.frame && u.ground == ground
            })
        {
            // One snapshot an image: the one drawn in an older fill colour
            // goes, so a fill that follows `OSC 11` keeps no copy per colour,
            // and an animation's other frame goes, so it keeps none per
            // frame.
            self.image_uploads
                .retain(|u| !(u.id == id && u.epoch == epoch));
            let pixels = match view.ground {
                ImageGround::Clear => view.pixels.iter().map(|p| p.bits()).collect(),
                ImageGround::Filled { .. } => view
                    .pixels
                    .iter()
                    .map(|&p| filled(p, ground).bits())
                    .collect(),
            };
            self.image_uploads.push(ImageUpload {
                id,
                epoch,
                frame: view.frame,
                ground,
                w: view.width.raw(),
                h: view.height.raw(),
                pixels,
            });
            self.image_snapshots = self.image_snapshots.inc();
        }
    }

    /// **What a Sixel's background fill draws as this frame**: the default
    /// background unreversed, `OSC 11`'s colour else the theme's, opaque, as
    /// `[[source: vt3xx-sixel]]` has it, "Pixel positions specified as 0 are
    /// set to the current background color". Under `?5` too: a VT340 fills
    /// with colour-map entry 0 on a reversed screen
    /// (`[[source: vt340test-background-select-2-png]]`), and xterm's
    /// graphics read no reverse video (`[[source: xterm-graphics-c]]`). Read
    /// each frame, as a VT340 looks its map up at every refresh.
    ///
    /// **On the see-through window as `[glass] sixel_backgrounds` says**
    /// ([`crate::SixelBg`]): opaque, the default, as on an opaque window;
    /// or the same colour at the window's dim, the alpha an erased cell's
    /// background has there, which the placement draws over what is under
    /// it or in its place ([`ImageComposite`]).
    ///
    /// Read from `term` itself, as the placements are built, and not from
    /// the defaults the frame's convert refreshes after them: from those,
    /// the frame after an `OSC 11` drew the fill in the colour before.
    pub fn fill_pixel(&self, term: &Term<'_>) -> Pixel {
        let bg = self
            .theme
            .defaults(Video::Normal, |d| term.dynamic_color(d))
            .1;
        let [_, r, g, b] = bg.0.to_be_bytes();
        let (r, g, b) = (Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b));
        match (self.page(), self.theme.sixel_bg) {
            (Page::Opaque, _) | (Page::Glass, crate::SixelBg::Opaque) => Pixel::opaque(r, g, b),
            (Page::Glass, crate::SixelBg::Dimmed | crate::SixelBg::Cleared) => {
                Pixel::straight(self.theme.bg_alpha, r, g, b)
            }
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

    /// **Blits the placements over the text that cover the row `target`
    /// repaints into it**, in draw order, after the row's glyphs and before
    /// baud's own overlays: through the row's own target, so into its band
    /// and the columns it repaints alone ([`RowTarget`]). The row's own
    /// placements if they are indexed, else every one asked whether it
    /// covers the row, as [`Renderer::paint_under`] reads them. Each in the
    /// pixels its image holds ([`held`]): on the kept frame an image gone
    /// is skipped here and kept as the last frame drew it, and every row of
    /// one whose pixels moved is repainted ([`Renderer::paint_kept`]).
    fn paint_over(&self, imgs: &ImageStore<'_>, target: &mut RowTarget<'_>, page: Page) {
        let band = target.band();
        let (on, all): (&[usize], &[ImagePlacement]) = match self.place_rows.row(target.row()) {
            Some(on) => (on, &[]),
            None => (&[], &self.image_places),
        };
        let places = on
            .iter()
            .map(|&i| &self.image_places[i])
            .chain(all.iter().filter(|p| covers(p, band)));
        for p in places.filter(|p| p.layer == ImageLayer::Over) {
            let Some(view) = held(imgs, p) else {
                continue;
            };
            target.blit(self, p, &view, page);
        }
    }

    /// Blits the placements under the text that cover `row`'s band into it,
    /// in order: a repainted row's, over its background. Those below the
    /// non-default backgrounds first, and then those backgrounds over them,
    /// at their own alpha, as over the desktop where there is no image, so
    /// the default ones show the image and the others show it through their
    /// alpha; then the rest. The GPU paths draw the same steps.
    fn paint_under(
        &self,
        imgs: &ImageStore<'_>,
        target: &mut Target<'_>,
        row: Row,
        page: Page,
    ) -> Tally {
        let band = self.band(row);
        // The row's own placements if they are indexed, else every one
        // asked whether it covers the row.
        let (on, all): (&[usize], &[ImagePlacement]) = match self.place_rows.row(row) {
            Some(on) => (on, &[]),
            None => (&[], &self.image_places),
        };
        let places = || {
            on.iter()
                .map(|&i| &self.image_places[i])
                .chain(all.iter().filter(move |p| covers(p, band)))
        };
        // The cells the placements below the backgrounds reach in this
        // row: the only ones cleared and composited again, so a frame pays
        // for the image's columns and not the row's.
        let cw = i64::from(self.metrics.cell_w.value()).max(1);
        let reach = places()
            .filter(|p| p.layer == ImageLayer::Below)
            .map(|p| (i64::from(p.x), i64::from(p.x) + i64::from(p.w)))
            .reduce(|(a, b), (x0, x1)| (a.min(x0), b.max(x1)))
            .map_or(0..0, |(a, b)| {
                let cell = |v: i64| usize::try_from(v.max(0) / cw).unwrap_or(0);
                cell(a)..cell(b + cw - 1)
            });
        let set = |c: usize| reach.contains(&c) && self.set_at(row, Col::at_index(c));
        let mut below = false;
        for p in places() {
            if p.layer == ImageLayer::Under && below {
                self.cover_set(target, row, page, set);
                below = false;
            }
            if p.layer == ImageLayer::Over {
                continue;
            }
            let Some(view) = held(imgs, p) else {
                continue;
            };
            if p.layer == ImageLayer::Below && !below {
                self.clear_set(target, row, page, set);
                below = true;
            }
            target.blit_image(
                &self.blit_of(p, band),
                &view,
                (p.ground, p.composite),
                self.blend,
                page,
            );
        }
        if below {
            self.cover_set(target, row, page, set);
        }
        // Each of the two passes examined these.
        Tally::of(2 * (on.len() + all.len()))
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
        let orders = self.bidi.orders();
        // Only a row of the window takes a selection, as `convert_row`
        // draws it: the status line's row and the notice rows after it are
        // asked nothing, though a selection's unclipped ends run onto them.
        let selected = match self
            .highlight
            .filter(|_| self.is_window_row(row))
            .is_some_and(|h| orders.selects(&h, row, orders.logical_col(row, col), cell.content))
        {
            true => Selected::Yes,
            false => Selected::No,
        };
        let block = match self.last_cursor {
            Some((r, cc, style)) if r == row && cc == col && self.swaps(style) => {
                BlockCursor::Yes(BlockPaint::SWAP)
            }
            _ => BlockCursor::No,
        };
        // A word's cell is drawn unlit, its light over its syllable's span.
        let at = row.index() * self.cols.index() + col.index();
        if self.worded.get(at) == Some(&crate::word::Worded::Yes) {
            return res.ground(Selected::No, BlockCursor::No);
        }
        res.ground(selected, block)
    }

    /// **The Devanagari words the last convert of row `r` drew**, and where
    /// (`crate::word`): none on a row it did not convert since it was last
    /// drawn, or that holds none.
    pub fn words_on(&self, r: Row) -> &[crate::word::WordLaid] {
        self.laid.get(r.index()).map_or(&[], Vec::as_slice)
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
    /// **The lines of `view` that are not single-width, converted as the
    /// instance path converts them**, for the path that draws the grid
    /// itself: each such row into `out`, and its glyphs and decorations,
    /// cursor shapes included, in the order the instance path draws them,
    /// kept by the renderer and read in place ([`Renderer::sized_records`]).
    /// The cell shader
    /// draws their backgrounds from the grid, a character's under both its
    /// columns, and leaves the rest to these (`Overlay::ensure`, which masks
    /// the rows), so a sized line is drawn by one conversion on every path.
    /// Nothing is read or converted when every line is single-width. An
    /// input method's `composition` on a sized line blanks the cells under
    /// it, as [`Renderer::convert`] does, for its own records to draw.
    ///
    /// Each line's records are kept while what it is drawn from holds, its
    /// cells compared with the line's key: `view` is a window of a copy
    /// this call cannot name the seal of (`Renderer::sized_framed`).
    pub fn sized_lines(
        &mut self,
        term: &Term<'_>,
        (view, table): (&Viewport<'_>, &[Resolved]),
        cursor: Option<(Row, Col, CursorStyle)>,
        composition: Option<&Composition>,
        out: &mut Vec<Row>,
    ) {
        self.sized_framed(term, (view, table), None, cursor, composition, out);
    }

    /// **[`Renderer::sized_lines`] over the window a frame's build keyed**
    /// (`framed`, FS-C5): a line drawn as stored is keyed by where its row
    /// is stored and the build's seal, and its cells are neither read nor
    /// copied while that row was not written since; one the layout drew in
    /// another order, any line while a re-broken display is drawn, and
    /// every line with no `framed`, by its cells, compared.
    pub(crate) fn sized_framed(
        &mut self,
        term: &Term<'_>,
        (view, table): (&Viewport<'_>, &[Resolved]),
        framed: Option<Framed>,
        cursor: Option<(Row, Col, CursorStyle)>,
        composition: Option<&Composition>,
        out: &mut Vec<Row>,
    ) {
        out.clear();
        let (cols, rows) = (view.cols(), view.rows());
        // **A row kept only while its line is sized**: a line set back to
        // single-width forgets its records, here when no line is sized and
        // in the walk below when others still are, so
        // [`Renderer::sized_records`] never answers for a row that is not
        // a sized line now.
        if !(0..rows.index()).any(|y| view.rendition(Row::from_index(y)).is_double()) {
            for kept in &mut self.sized_rows {
                *kept = None;
            }
            return;
        }
        self.sized_ready(term);
        // `view` is the frame's copy, whose frozen cells name the slots its
        // table, `table`, lent them (`crate::frozen`): lent this table too.
        self.adopt_frame(table);
        if self.theme.finds_pictures() {
            let frame = self.frame_held();
            let lent = !frame.words().is_live();
            let words = |y: Row| match lent {
                true => frame.words(),
                false => Words::Row(view.styles(y)),
            };
            self.pictures.find_in(term, view, words, self.video);
        } else {
            self.pictures.clear(cols, rows);
        }
        let ncols = cols.index();
        if self.sized_rows.len() != rows.index() {
            self.sized_rows = (0..rows.index()).map(|_| None).collect();
        }
        // The window's stored rows and the build's seal, where a build
        // keyed it and no display is re-broken.
        let held = self.bidi.orders().display().is_some();
        let stored = framed
            .filter(|_| !held)
            .map(|f| (term.row_gens(f.back), f.seal));
        for y in 0..rows.index() {
            let r = Row::from_index(y);
            let size = view.rendition(r);
            if !size.is_double() {
                self.sized_rows[y] = None;
                continue;
            }
            let at = stored.as_ref().filter(|_| !self.bidi.orders().reordered(r));
            if at.is_none() {
                self.sized_fill(view, r, composition);
            }
            // **Converted again only when something it is drawn from moved**:
            // a steady screen, or one whose other rows change, reuses the
            // line's records. Under the picture finder a row enters a block
            // without a cell of its own changing, so the key holds what the
            // finder found on the row this frame.
            let rest = SizedRest {
                size,
                cursor: cursor
                    .filter(|&(cr, _, _)| cr == r)
                    .map(|(_, c, s)| (c, s, self.cursor_opacity(s))),
                look: self.cursor_look,
                selection: self.highlight.filter(|h| h.covers_row(r)),
                blink: self.blink,
                video: self.video,
                hilite: self.hilite(),
                cursor_color: term.cursor_color_in(self.video),
                cursor_paint: self.block_paint,
                styles: self.styles_stamp,
                clusters: term.cluster_epoch(),
                atlas: self.atlas.stats().grown,
            };
            let mut kept = self.sized_rows[y].take();
            let reuse = {
                let cells = match at {
                    Some((gens, seal)) => SizedCells::Stored {
                        at: gens.window().at(r),
                        seal: *seal,
                        gens,
                        y: r,
                    },
                    None => SizedCells::Drawn(&self.cur[y * ncols..(y + 1) * ncols]),
                };
                let now = SizedNow {
                    cells,
                    pictured: self.pictures.row(r),
                    composition: composition.filter(|c| c.row == r),
                    rest,
                };
                let reuse = kept.as_ref().is_some_and(|s| s.key.holds(&now));
                if !reuse {
                    match kept.as_mut() {
                        Some(s) => s.key.take(&now),
                        None => {
                            kept = Some(SizedRow {
                                key: SizedKey::of(&now),
                                records: Vec::new(),
                            });
                        }
                    }
                }
                reuse
            };
            self.sized_rows[y] = kept;
            if !reuse {
                if at.is_some() {
                    self.sized_fill(view, r, composition);
                }
                let mut exc = Exceptions::default();
                // A composition on a line that joins, drawn in its order.
                self.splice = None;
                if let Some(c) = composition.filter(|c| c.row == r) {
                    let under = view
                        .row(r)
                        .get(self.bidi.orders().visual_col(r, c.col).index())
                        .copied()
                        .unwrap_or(PackedCell::EMPTY);
                    let _ = self.composed_parts(term, c, under, cursor, cols);
                }
                self.convert_row(term, r, cursor, &mut exc);
                self.splice = None;
                self.sized_converted.bump();
                if let Some(s) = self.sized_rows[y].as_mut() {
                    s.records.clear();
                    s.records.extend_from_slice(&self.per_row[r]);
                }
            }
            out.push(r);
        }
    }

    /// **A sized line's records**, as [`Renderer::sized_lines`] last kept
    /// them for row `r`, read in place: none for a row it did not keep,
    /// and none for a row whose line its last call found single-width.
    pub fn sized_records(&self, r: Row) -> &[Instance] {
        self.sized_rows
            .get(r.index())
            .and_then(Option::as_ref)
            .map_or(&[][..], |s| s.records.as_slice())
    }

    /// **The slots this path's table lent its last frozen cells, put back**
    /// (`crate::frozen`), before the table is brought up to date: the sync
    /// rewrites only the slots the core logged.
    fn give_back_cpu(&mut self) {
        let (mut styles, mut lent) = (
            core::mem::take(&mut self.styles),
            core::mem::take(&mut self.cpu_lent),
        );
        let mut stamp = self.styles_stamp;
        self.give_back(&mut styles, &mut stamp, &mut lent);
        (self.styles, self.styles_stamp, self.cpu_lent) = (styles, stamp, lent);
    }

    /// **Each frozen cell of this path's copy of the window lent a slot of
    /// its table** ([`Renderer::lend_frozen`]), and each struck cell its
    /// grey ([`Renderer::lend`]): every row the copy holds, in place
    /// ([`crate::frozen::FlatCopy`]), the window's through the table each
    /// of its cells names, where it shows a frozen row -- and the status
    /// line's and the notice's, which are live. Whether the struck cells
    /// were the last frame's.
    fn lend_cpu(&mut self, term: &Term<'_>, back: Rows) -> StruckFound {
        let ncols = self.cols.index().max(1);
        let frozen = self.bidi.orders().window_frozen(term, back);
        let struck = self.take_struck(term, back, &term.view(back), self.main, CopyOrder::Visual);
        // With a display held a window row names runs, made as the layout
        // finds them: once, here, as before. With none each row is one
        // table, asked of the layout as the lending reads it.
        let display = frozen && self.bidi.orders().display().is_some();
        let sources: Vec<crate::frozen::RowSource<'_>> = match display {
            true => (0..self.main.index())
                .map(|r| {
                    self.bidi
                        .orders()
                        .row_source(term, back, Row::from_index(r))
                })
                .collect(),
            false => Vec::new(),
        };
        let words = match (frozen, display) {
            (false, _) => crate::frozen::CopyWords::Live,
            (true, false) => crate::frozen::CopyWords::Window {
                back,
                window: self.main,
                after: Words::LIVE,
            },
            (true, true) => crate::frozen::CopyWords::Each {
                rows: &sources,
                after: Words::LIVE,
            },
        };
        let (mut styles, mut lent, mut cur) = (
            core::mem::take(&mut self.styles),
            core::mem::take(&mut self.cpu_lent),
            core::mem::take(&mut self.cur),
        );
        let mut stamp = self.styles_stamp;
        {
            let cells = cur.converting().0;
            let rows = Rows::of_count(cells.len().div_ceil(ncols));
            let mut copy = crate::frozen::FlatCopy::new(
                cells,
                baud_domain::Stride::of_cells(ncols),
                self.cols,
                rows,
                words,
            );
            let _ = self.lend_rows(
                term,
                &mut copy,
                struck.cells(),
                &mut styles,
                &mut stamp,
                &mut lent,
            );
        }
        (self.styles, self.styles_stamp, self.cpu_lent, self.cur) = (styles, stamp, lent, cur);
        let found = struck.found();
        self.put_struck(struck);
        found
    }

    /// The renderer fitted to `term` and its rendition table brought up to
    /// date, for a conversion outside [`Renderer::convert`].
    fn sized_ready(&mut self, term: &Term<'_>) {
        self.fit(term);
        self.give_back_cpu();
        let mut styles = core::mem::take(&mut self.styles);
        if styles.len() != term.styles().one().len() {
            styles = vec![Resolved::default(); term.styles().one().len()];
            self.styles_stamp = StyleStamp::default();
        }
        let mut stamp = self.styles_stamp;
        let _ = self.sync_styles(term, &mut styles, &mut stamp);
        (self.styles, self.styles_stamp) = (styles, stamp);
    }

    /// **The size of the line a smooth scroll's leaving row draws**: the
    /// first line of the window one line up as its layout draws it, laid
    /// out here -- where the display re-broke a paragraph below it
    /// ([`crate::rebreak`]), that display line's, which may be a
    /// single-width line of the paragraph where the stored row above the
    /// window is double. What [`crate::Overlay::leaving`] and
    /// [`Renderer::sized_leaving`] decide the row's path by.
    pub(crate) fn leaving_rendition(&mut self, term: &Term<'_>, back: Rows) -> LineRendition {
        let above = Rows::of_index(back.index() + 1);
        let reach = self.widest_reach();
        let leaving = self
            .beside
            .get_or_insert_with(Box::default)
            .leaving
            .get_or_insert_with(crate::bidi::Reorder::new);
        leaving.set_reach(reach);
        leaving.set_word_wrap(self.word_wrap);
        leaving.lay_out_top(term, above, self.joins_seen);
        leaving.view(term, above).rendition(Row::ZERO)
    }

    /// **Sized line `r` of `view` into this path's copy, as drawn**, to be
    /// converted or compared from: its cells and rendition, and the cells
    /// under a composition on it blanked where the row's order draws them,
    /// as `Renderer::convert` blanks them -- `view` is in that order, so a
    /// cell the composition's columns name is at its visual column, and a
    /// mirror L4 gives the composition's text is in its cell there.
    fn sized_fill(&mut self, view: &Viewport<'_>, r: Row, composition: Option<&Composition>) {
        let (y, ncols) = (r.index(), view.cols().index());
        let (cells, sizes) = self.cur.scribble();
        cells[y * ncols..(y + 1) * ncols].copy_from_slice(view.row(r));
        sizes[y] = view.rendition(r);
        crate::counters::add(Counter::CellsCopied, Site::Sized, Tally::of(ncols));
        if let Some(c) = composition.filter(|c| c.row == r) {
            let (start, end) = c.columns(c.width_on(view));
            let orders = self.bidi.orders();
            for x in start..end {
                let v = orders.visual_col(r, Col::at_index(x)).index();
                if let Some(cell) = self.cur.scribble().0.get_mut(y * ncols + v) {
                    *cell = PackedCell {
                        content: baud_domain::Content::EMPTY,
                        style: cell.style,
                        flags: baud_domain::CellFlags::NONE,
                    };
                }
            }
        }
    }

    /// **The leaving row of a smooth scroll, when its line is not
    /// single-width**: the line just above the window `back` lines into the
    /// history, in the order it was drawn in as that window's first,
    /// converted as the instance path converts a line of its rendition and
    /// lifted one row, to draw row -1, where the cell path slides it in. The cell path
    /// draws that row from the map alone, at single width, and masks it for
    /// these when `size` is not ([`crate::Overlay::leaving`]). Nothing for a
    /// single-width line.
    ///
    /// Converted on row 0's buffers, which are saved and put back whole --
    /// its cells, rendition, records, colours, blink, reach and glyph ends
    /// -- so the frame the instance path reuses is untouched; with no
    /// picture, as the cell path never takes the leaving row for one, and
    /// no cursor, which is never on it.
    pub fn sized_leaving(&mut self, term: &Term<'_>, back: Rows, out: &mut Vec<Instance>) {
        out.clear();
        let above = Rows::of_index(back.index() + 1);
        // The row in the order it was drawn in as the first of the window
        // one line up, converted through that layout's map -- its seams,
        // joins and mirrors -- put in place of the window's for the one
        // conversion. Its size is that line's: where the display re-broke
        // a paragraph below it, the line drawn there, not the stored row.
        let size = self.leaving_rendition(term, back);
        if !size.is_double() {
            return;
        }
        self.sized_ready(term);
        let Some(leaving) = self.beside.as_deref().and_then(|b| b.leaving.as_ref()) else {
            return;
        };
        let up = leaving.view(term, above);
        let mut cells = vec![PackedCell::EMPTY; up.cols().index()];
        leaving
            .orders()
            .permute(Row::ZERO, up.row(Row::ZERO), &mut cells);
        let n = self.cols.index().min(cells.len());
        if n == 0 || self.rows.index() == 0 {
            return;
        }
        let r0 = Row::ZERO;
        let range = self.row_cells(r0);
        let kept = (
            self.cur[range.clone()].to_vec(),
            self.cur.sizes()[0],
            self.per_row.take(r0),
            self.bg[range.clone()].to_vec(),
            self.fg[range.clone()].to_vec(),
            self.curve[range.clone()].to_vec(),
            self.text[range.clone()].to_vec(),
            self.row_blinks[0],
            self.glyph_ends[0],
            self.reach[0],
        );
        let mut pictures = core::mem::take(&mut self.pictures);
        self.pictures.clear(self.cols, self.rows);
        let (to, sizes) = self.cur.scribble();
        to[range.start..range.start + n].copy_from_slice(&cells[..n]);
        sizes[0] = size;
        // A line the core froze, drawn in its own renditions
        // (`crate::frozen`): its cells lent slots of this path's table.
        let source = self.leaving_source(term, back);
        if !source.words().is_live() {
            let (mut styles, mut lent, mut cur) = (
                core::mem::take(&mut self.styles),
                core::mem::take(&mut self.cpu_lent),
                core::mem::take(&mut self.cur),
            );
            let mut stamp = self.styles_stamp;
            let mut rows = [(
                &mut cur.scribble().0[range.start..range.start + n],
                source.words(),
            )];
            let _ = self.lend_frozen(term, &mut rows, &mut styles, &mut stamp, &mut lent);
            (self.styles, self.styles_stamp, self.cpu_lent, self.cur) = (styles, stamp, lent, cur);
        }
        let mut exc = Exceptions::default();
        self.swap_leaving_layout();
        self.convert_row(term, r0, None, &mut exc);
        self.swap_leaving_layout();
        let ch = i16::try_from(self.metrics.cell_h.value()).unwrap_or(i16::MAX);
        out.extend(self.per_row[r0].iter().map(|i| Instance {
            y: Screen(i.y.0.saturating_sub(ch)),
            ..*i
        }));
        core::mem::swap(&mut self.pictures, &mut pictures);
        let (cur, size, per_row, bg, fg, curve, text, blinks, ends, reach) = kept;
        let (to, sizes) = self.cur.scribble();
        to[range.clone()].copy_from_slice(&cur);
        sizes[0] = size;
        self.per_row.set(r0, per_row);
        self.bg[range.clone()].copy_from_slice(&bg);
        self.fg[range.clone()].copy_from_slice(&fg);
        self.curve[range.clone()].copy_from_slice(&curve);
        self.text[range].copy_from_slice(&text);
        self.row_blinks[0] = blinks;
        self.glyph_ends[0] = ends;
        self.reach[0] = reach;
    }

    /// The frame fitted to `term`: its main display, and the status line's
    /// row below it while one is shown.
    pub fn fit(&mut self, term: &Term<'_>) {
        let (cols, main) = term.geometry();
        let rows =
            Rows::of_index(main.index() + term.status_rows().index() + self.notice_rows().index());
        if cols.index() != self.cols.index() || rows.index() != self.rows.index() {
            self.resize(cols, rows);
        }
        if main != self.main {
            self.main = main;
            self.known = Prev::Unknown;
        }
    }

    /// The main display's rows, above the status line's.
    pub fn main_rows(&self) -> Rows {
        self.main
    }

    /// Whether frame row `r` is a row of the window: one of the main
    /// display's, above the status line's row and the notice rows after it.
    fn is_window_row(&self, r: Row) -> bool {
        r.index() < self.main.index()
    }

    /// **The status line's row, converted as the instance path converts a
    /// line**, for the path that draws the grid itself: its glyphs and
    /// decorations, its cursor's shapes included, at the row below the main
    /// display, appended to `out`, which the caller keeps, so the line
    /// costs no buffer of its own. The cell shader draws its backgrounds
    /// from the window copy's row after the main display's and leaves the
    /// rest to these. Nothing while no status line is shown.
    pub fn status_records(
        &mut self,
        term: &Term<'_>,
        cursor: Option<(Row, Col, CursorStyle)>,
        out: &mut Vec<Instance>,
    ) {
        self.sized_ready(term);
        let Some(line) = term.status_line() else {
            return;
        };
        let r = Row::from_index(self.main.index());
        if r.index() >= self.rows.index() {
            return;
        }
        let range = self.row_cells(r);
        let n = range.len().min(line.cells.len());
        self.bidi.orders().permute(
            r,
            &line.cells[..n],
            &mut self.cur.scribble().0[range.start..range.start + n],
        );
        crate::counters::add(Counter::CellsCopied, Site::Status, Tally::of(n));
        self.cur.scribble().1[r.index()] = line.rendition;
        let mut exc = Exceptions::default();
        self.convert_row(term, r, cursor, &mut exc);
        out.extend_from_slice(&self.per_row[r]);
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
        // The lending the last frame's copy was drawn under, which this one
        // is compared with below, and what each slot it lent held, read into
        // a kept buffer. And `cur` put back as the terminal's where that
        // lending rewrote it, before anything reads it: the rows it touched
        // are compared below unless this frame lends them alike.
        let lent_was = self.styles_stamp.lending;
        self.cpu_lent
            .lent_values_into(&self.styles, &mut self.held_was);
        self.cur.unlend(self.cols, &mut self.lent_rows);
        self.give_back_cpu();
        let mut styles = core::mem::take(&mut self.styles);
        if styles.len() != term.styles().one().len() {
            styles = vec![Resolved::default(); term.styles().one().len()];
            self.styles_stamp = StyleStamp::default();
        }
        let mut stamp = self.styles_stamp;
        let sync = self.sync_styles(term, &mut styles, &mut stamp);
        (self.styles, self.styles_stamp) = (styles, stamp);
        let (cols, rows, main) = (self.cols, self.rows, self.main);
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
        // With BDSM reset no row is reordered and each row's joins are its
        // own, so they are kept from the last frame and found again below
        // only for the rows this frame's compare finds changed: a row that
        // did not change is not read for them.
        // Until the renderer has shaped a letter that takes a form, no row
        // can hold one it has drawn, and none is read for joins: a frame
        // that shapes the first is converted again below, every row joined.
        let seen = self.joins_seen;
        let rejoin = match (term.bidi_support(), seen) {
            (BidiSupport::Explicit, crate::bidi::Joins::Seen) => {
                Some(self.keep_bidi_displayed(term, back, crate::bidi::Changes::Converted))
            }
            _ => {
                self.lay_out_bidi(term, back);
                None
            }
        };
        // **The window as the layout drew it**: the display's lines where it
        // re-broke a finished paragraph ([`crate::rebreak`]), else the
        // core's own rows.
        let held = self.bidi.orders().display_held();
        let vp = match &held {
            Some(d) => d.window(crate::frozen::live_table(term)),
            None => term.view(back),
        };
        // **The status line's row, below the main display's**, whatever the
        // window looks back on: it is no line of the history. In the order
        // its own layout gives it, the maps' row after the window's.
        let status = term.status_line();
        let lined = usize::from(status.is_some() && main.index() < rows.index());
        // **The embedder's notice, on its own row below both**: blank cells
        // in the default rendition, drawn over by its field below.
        let notice = self.notice_field();
        // **Which rows `cur` takes from the terminal again** (FS-C5): every
        // row when its key does not hold -- none yet, another window, other
        // scalars, another display, a frame that knows nothing of the
        // last; else the rows written since its seal or laid out again
        // since its layout's, and the status line's by its own generation
        // and its own row of the maps. And the rows the last frame veiled,
        // composed over or noticed, or every row if it lent a frozen window
        // slots: `cur` holds those as drawn, not as stored. The cells its
        // struck links were lent are the terminal's again already
        // (`Mirror::unlend`, above). Every other row is the terminal's
        // already, and the last frame's: it is neither read nor compared.
        let orders = self.bidi.orders();
        let now = crate::screens::Now::of(term, back, (&vp, status.as_ref()), term.stride());
        let keeps = match self.known {
            Prev::Unknown => crate::screens::Keeps::Nothing,
            Prev::Known => self.cur.keeps(term, &now, orders),
        };
        self.mirror_rows.clear();
        for r in 0..rows.index() {
            let taken = match keeps {
                crate::screens::Keeps::Nothing => true,
                crate::screens::Keeps::All => false,
                crate::screens::Keeps::Rows => {
                    r < main.index() + lined
                        && self.cur.dirty(&now, orders, &vp, Row::from_index(r))
                }
            };
            self.mirror_rows.push(match taken {
                true => MirrorRow::Filled,
                false => MirrorRow::Kept,
            });
        }
        if self.lent_last == LentRows::Every {
            self.mirror_rows.fill(MirrorRow::Filled);
        }
        for &r in &self.veiled_last {
            if let Some(m) = self.mirror_rows.get_mut(r) {
                *m = MirrorRow::Filled;
            }
        }
        if let Some(m) = notice
            .as_ref()
            .and_then(|f| self.mirror_rows.get_mut(f.row.index()))
        {
            *m = MirrorRow::Filled;
        }
        let mut copied = 0;
        for r in 0..main.index() {
            if self.mirror_rows[r] != MirrorRow::Filled {
                continue;
            }
            let row = Row::from_index(r);
            let (cells, sizes) = self.cur.converting();
            orders.permute(row, vp.row(row), &mut cells[r * ncols..(r + 1) * ncols]);
            sizes[r] = vp.rendition(row);
            copied += ncols;
        }
        crate::counters::add(Counter::CellsCopied, Site::Convert, Tally::of(copied));
        if let Some(line) = status.as_ref().filter(|_| lined == 1) {
            let r = main.index();
            if self.mirror_rows[r] == MirrorRow::Filled {
                let n = ncols.min(line.cells.len());
                orders.permute(
                    Row::from_index(r),
                    &line.cells[..n],
                    &mut self.cur.converting().0[r * ncols..r * ncols + n],
                );
                crate::counters::add(Counter::CellsCopied, Site::Convert, Tally::of(n));
                self.cur.converting().1[r] = line.rendition;
            }
        }
        if let Some(f) = &notice {
            let r = f.row.index();
            let (cells, sizes) = self.cur.converting();
            cells[r * ncols..(r + 1) * ncols].fill(PackedCell::EMPTY);
            sizes[r] = LineRendition::Single;
        }
        // The rows this frame veils, composes over or notices in `cur`, and
        // whether it lends every row, for the compare below and the next
        // frame's fill.
        self.veiled_now.clear();
        self.veiled_now
            .extend(notice.as_ref().map(|f| f.row.index()));
        let mut lent_now = LentRows::None;
        // **A window over history the core froze** (`crate::frozen`): each
        // frozen cell of this path's copy given a slot of its table no
        // other cell of the frame names, its generation's word resolved
        // there, so every reader below reads one table, as the cell path's
        // do; and after them each struck link's cells, a slot a source
        // rendition holding its grey. The copy is this frame's damage
        // record too, and a lent id means what it means only under its
        // lending: a frame lent otherwise than the last repaints every row
        // where that moved what an unchanged cell draws (`full` below,
        // `Lent::moved_from`). A frozen window lends every row; struck
        // links alone lend the rows whose cells the lending rewrites, each
        // kept below while it is lent as the last frame lent it.
        let mut struck = StruckFound::Again;
        let frozen_window = self.bidi.orders().window_frozen(term, back);
        if frozen_window || !self.links.strikes().is_empty() {
            struck = self.lend_cpu(term, back);
            lent_now = match frozen_window {
                true => LentRows::Every,
                false => LentRows::Struck,
            };
        }
        self.cur.lend(self.cpu_lent.rewrites(), cols);
        self.frame_words = self.cpu_lent.held();
        // The frame's links, on this frame's layout, a field's ground read
        // from this path's copy, whose ids the lending above named; the
        // solids placed again on a frame drawn whole.
        {
            let cur = core::mem::take(&mut self.cur);
            let force = match damage {
                Damage::Full => Force::Full,
                Damage::Incremental => Force::No,
            };
            self.place_links_with(term, &vp, force, |y, v, _| {
                cur.get(y.index() * ncols + v.index())
                    .copied()
                    .unwrap_or(PackedCell::EMPTY)
            });
            self.cur = cur;
        }
        // An input method's composition, on the live view: the cells under it
        // converted blank in their own renditions, so their backgrounds stay
        // and their glyphs give way to its records, built below. The cell it
        // starts on lends the records its rendition.
        let composition = self
            .composition
            .clone()
            .filter(|c| back.index() == 0 && c.row.index() < main.index());
        // Laid out with it written there (`bidi::Written`), its cells are
        // drawn where the row's order puts them, as the same text written.
        let mut under = PackedCell::EMPTY;
        self.taken = crate::overlay::Taken::default();
        if let Some(c) = &composition {
            let (start, end) = c.columns(c.width_on(&vp));
            let at = c.row.index() * ncols;
            self.veiled_now.push(c.row.index());
            if let Some(&cell) = vp.row(c.row).get(start) {
                under = cell;
            }
            let orders = self.bidi.orders();
            for x in start..end {
                let v = orders.visual_col(c.row, Col::at_index(x)).index();
                if let Some(cell) = self.cur.converting().0.get_mut(at + v) {
                    *cell = PackedCell {
                        content: baud_domain::Content::EMPTY,
                        style: cell.style,
                        flags: baud_domain::CellFlags::NONE,
                    };
                }
            }
            // The letters written beside it that its words take in, the
            // same way: its records draw them, in one word with its own.
            self.taken = c.taken(term, orders, c.width_on(&vp), vp.rendition(c.row));
            for v in self.taken.beside(c.row) {
                if let Some(cell) = self.cur.converting().0.get_mut(at + v) {
                    *cell = PackedCell {
                        content: baud_domain::Content::EMPTY,
                        style: cell.style,
                        flags: baud_domain::CellFlags::NONE,
                    };
                }
            }
        }
        // **baud's veils, held against the last frame's** (FS-C16): the
        // find field, the link preview's bar, the fields of the link under
        // the pointer and the hover tip's lines, on whatever view the window
        // shows, each the cells under it converted blank in their
        // renditions, as under a composition, and drawn over by its records
        // below -- kept while nothing they are drawn from moved. The
        // notice's row is blank already.
        let mut veils = core::mem::replace(&mut self.cpu_veils, Veiled::reaching(VeilReach::Main));
        veils.hold(&self.veils_now(term, (self.styles_stamp, cols)));
        for (_, row, start, end) in veils.spans().filter(|v| v.0 != VeilKind::Notice) {
            self.veiled_now.push(row.index());
            let at = row.index() * ncols;
            for cell in &mut self.cur.converting().0[at + start.index()..at + end.index()] {
                *cell = PackedCell {
                    content: baud_domain::Content::EMPTY,
                    style: cell.style,
                    flags: baud_domain::CellFlags::NONE,
                };
            }
        }
        if self.theme.finds_pictures() {
            let lent = self.cpu_lent.held();
            match lent.words().is_live() {
                true => {
                    let drawn =
                        self.bidi
                            .orders()
                            .window_kept(term, back, &mut self.drawn_order.window);
                    self.pictures.find(term, &drawn, self.video);
                }
                // The copy lent its frozen cells slots: read it, in the
                // drawn order the window is, through their words.
                false => {
                    let n = main.index() * ncols;
                    let used = vec![baud_domain::RowExtent::of(ncols); main.index()];
                    let drawn = Viewport::new(
                        &self.cur[..n],
                        &used,
                        baud_domain::Stride::of_cells(ncols),
                        cols,
                        main,
                        main,
                        baud_domain::RowRot::NONE,
                        crate::frozen::live_table(term),
                    );
                    self.pictures
                        .find_in(term, &drawn, |_| lent.words(), self.video);
                }
            }
        } else {
            self.pictures.clear(cols, main);
        }
        // No cursor while looking back: it belongs to the live screen, which
        // is `back` lines below the window. While DECSASD selects the status
        // line the cursor is on its row.
        let on_line = status
            .and_then(|l| l.cursor)
            .map(|c| (Row::from_index(main.index()), c, term.cursor_style()));
        let live: Cursor = match on_line {
            Some(at) => (back.index() == 0).then_some(at),
            None => (term.cursor_visible() && back.index() == 0)
                .then(|| match &composition {
                    Some(c) => c.cursor(c.width_on(&vp)),
                    None => Some(term.cursor()),
                })
                .flatten()
                .map(|(r, c)| (r, c, term.cursor_style())),
        };
        // No cursor over an address or a message (`Renderer::clear_of_messages`).
        let live = self.clear_of_messages(term, live);
        self.set_cursor_glow_at(live);
        self.place_grounds(live, Self::cursor_line(term, back));
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
            || (self.styles_stamp.lending != lent_was
                && self
                    .cpu_lent
                    .moved_from(&self.held_was, &self.styles, &self.cur[..])
                    == crate::frozen::Moved::Yes)
            || self.fade != self.last_fade
            || self.bell != self.last_bell
            || clusters != self.last_clusters;
        let cursor_row = |c: Cursor| c.map(|(r, _, _)| r.index());
        let find_changed = self.find != self.last_find;
        let link_changed = self.link != self.last_link;
        let lines_changed = self.links.lines() != self.last_link_lines;
        let strikes_changed = self.links.strikes() != self.last_link_strikes;
        let over_changed = self.links.hover() != self.last_link_hover.as_ref()
            || self.hover_bars != self.last_hover_bars;
        let tip_changed = self.tip != self.last_tip;
        let mut rows_dirty = Tally::default();
        let mut exceptions = Exceptions::default();
        // Whether a row is dirty for anything but the cursor, its blink
        // animation, its trail or the cursor guide, which makes this frame's
        // redraw whole -- and so does the splash, whose records a GPU path
        // draws whole while it changes.
        let mut beyond_cursor = full || self.splash != self.last_splash;
        // The splash's records changed: a row they meet, now or in the last
        // frame, is dirty, for them alone.
        let splash_changed = self.splash_inst != self.last_splash_inst;
        // Which placements left and arrived, and whether any arrived or
        // changed: once a frame, so a frame with none asks no row about it.
        let mut diff = core::mem::take(&mut self.place_diff);
        if images == ImageRows::Repainted {
            diff.diff(&self.image_places, &self.last_image_places);
        }
        let any_new = images == ImageRows::Repainted && !diff.arrived.is_empty();
        let mut composed_parts: Option<crate::overlay::Composed> = None;
        self.splice = None;
        // **The rows compared with the last frame's**: those taken again,
        // and those veiled or lent now, which `cur` holds as drawn. Every
        // other row is the last frame's, unread.
        if lent_now == LentRows::Every {
            self.mirror_rows.fill(MirrorRow::Filled);
        }
        // **A row whose struck cells were lent, last frame or this one**:
        // the lending writes the same id into the same cell, so the row is
        // the last frame's, only where this frame lent struck cells alone
        // (`LentRows::Struck`), the struck cells were kept, the lending is
        // the last one (the same slots for the same source renditions) and
        // no frozen cell was lent in either frame. Else it is compared: a
        // number made again for an evicted lending reads as another one,
        // the safe side. (A frame that lent nothing found no struck cells,
        // and one that lent every row compares every row above.)
        //
        // **The lending's term is a guard no input moves alone**, kept so a
        // kept row's lent ids are read under the lending they were given
        // in. With the struck cells kept, struck cells alone lent and no
        // frozen cell lent in either frame, a lending other than the last
        // lent the same cells (kept), from the same source renditions (a
        // written row is taken again) and by the same lender, so only its
        // slots differ: each a slot no cell of the frame named, holding the
        // one struck rendition the same inputs resolve for its source, as
        // the last lending's held it -- the same pixels. A slot the last
        // lending gave that holds another rendition now while a cell of the
        // copy names it makes the frame full above (`Lent::moved_from`).
        let lent_alike = lent_now == LentRows::Struck
            && struck == StruckFound::Kept
            && self.styles_stamp.lending == lent_was
            && self.held_was.frozen_lent() == crate::frozen::FrozenLent::No
            && self.cpu_lent.frozen_lent() == crate::frozen::FrozenLent::No;
        if !lent_alike {
            let rewritten = self.cpu_lent.rewrites().iter().map(|&(y, _, _)| y);
            for r in self.lent_rows.iter().copied().chain(rewritten) {
                if let Some(m) = self
                    .mirror_rows
                    .get_mut(r)
                    .filter(|m| **m == MirrorRow::Kept)
                {
                    *m = MirrorRow::Compared;
                }
            }
        }
        for &r in &self.veiled_now {
            if let Some(m) = self
                .mirror_rows
                .get_mut(r)
                .filter(|m| **m == MirrorRow::Kept)
            {
                *m = MirrorRow::Compared;
            }
        }
        for r in 0..rows.index() {
            let range = self.row_cells(Row::from_index(r));
            let changed = match self.mirror_rows[r] {
                MirrorRow::Kept => false,
                MirrorRow::Compared | MirrorRow::Filled => {
                    crate::counters::add(
                        Counter::CellsCompared,
                        Site::Convert,
                        Tally::of(range.len()),
                    );
                    self.cur[range.clone()] != self.prev[range]
                        || self.cur.sizes()[r] != self.prev_size[r]
                }
            };
            // A window row's joins found again, with BDSM reset, where its
            // cells changed -- or may have: a composition, the find field
            // or the link bar blanks part of a row this frame or blanked it
            // the last, and the compare cannot see a cell change under it.
            if let Some(how) = rejoin.filter(|_| r < main.index()) {
                let row = Row::from_index(r);
                let on = |c: &Option<Composition>| c.as_ref().is_some_and(|c| c.row == row);
                let veiled = on(&composition)
                    || on(&self.last_composition)
                    || field_on(&self.find, row).is_some()
                    || field_on(&self.last_find, row).is_some()
                    || bar_on(&self.link, row).is_some()
                    || bar_on(&self.last_link, row).is_some()
                    || bars_on(&self.hover_bars, row).next().is_some()
                    || bars_on(&self.last_hover_bars, row).next().is_some()
                    || tip_on(&self.tip, row).is_some()
                    || tip_on(&self.last_tip, row).is_some();
                if how == crate::bidi::Rejoin::Every || full || changed || veiled {
                    let written = self
                        .beside
                        .as_deref()
                        .map_or(&NOTHING_WRITTEN, |b| &b.written);
                    self.bidi.rejoin(term, &vp, row, written);
                }
            }
            // The composition's records, once its row's joins are found:
            // on a row that joins, its pages and glyphs drawn in the row's
            // order, which converts it again every frame.
            if let Some(c) = composition.as_ref().filter(|c| c.row.index() == r) {
                composed_parts = Some(self.composed_parts(term, c, under, cursor, cols));
            }
            let spliced = self.splice.as_ref().is_some_and(|s| s.row.index() == r);
            let cursor_here = cursor_row(cursor) == Some(r);
            let cursor_was = cursor_row(self.last_cursor) == Some(r);
            let cursor_moved = cursor_here != cursor_was
                || (cursor_here
                    && (cursor != self.last_cursor
                        || opacity != self.last_cursor_opacity
                        || self.cursor_look != self.last_cursor_look));
            let glow_row = |g: Option<(Row, Col, CursorStyle, CursorPhase)>| {
                g.is_some_and(|(gr, _, _, _)| gr.index() == r)
            };
            let glow_moved =
                (glow_row(self.glow) || glow_row(self.last_glow)) && self.glow != self.last_glow;
            let guide_row = |g: Option<guide::Placed>| {
                g.is_some_and(|g| g.top == self.band(Row::from_index(r)).top.value())
            };
            let guide_moved = (guide_row(self.guide_at) || guide_row(self.last_guide_at))
                && self.guide_at != self.last_guide_at;
            let band_now = self.band(Row::from_index(r));
            let badge_row = |b: Option<[u32; 4]>| {
                b.is_some_and(|b| b[1] < band_now.bottom.value() && band_now.top.value() < b[3])
            };
            let badge_moved = self.badge_built != self.last_badge_built
                && (badge_row(self.badge_rect) || badge_row(self.last_badge_rect));
            let row = Row::from_index(r);
            // **A display line that is not its own stored row** draws cells
            // of other rows, a line's past the window among them
            // ([`crate::rebreak`]): what is selected or marked on it is not
            // read off its own row, so any change to either repaints it.
            let rebroken_line = self
                .bidi
                .orders()
                .display()
                .is_some_and(|d| d.same(row) != Some(row));
            let selected = |h: Option<Highlight>| h.filter(|h| h.covers_row(row));
            let selection_moved = selected(self.highlight) != selected(self.last_highlight)
                || (rebroken_line && self.highlight != self.last_highlight);
            let band = self.band(row);
            let slice = |b: Option<Bar>| b.and_then(|b| b.slice(band.top, band.bottom));
            let bar_moved = slice(self.bar) != slice(self.last_bar);
            let under_trail =
                |d: Option<Drawn>| d.is_some_and(|d| d.overlaps(band.top, band.bottom));
            let trail_moved = under_trail(self.trail) != under_trail(self.last_trail)
                || (under_trail(self.trail) && self.trail != self.last_trail);
            let splashed = splash_changed
                && (records_meet(&self.splash_inst, band)
                    || records_meet(&self.last_splash_inst, band));
            // A picture is the block's, so a row can move in or out of one
            // without a cell of its own changing.
            let pictured = self.pictures.row_changed(row);
            // A composition that appeared, changed or went: the row it is on
            // and the row it was on, whether or not a cell word moved -- a
            // composition over the same text changes none.
            let on = |c: &Option<Composition>| c.as_ref().is_some_and(|c| c.row == row);
            let composed = ((on(&composition) || on(&self.last_composition))
                && composition != self.last_composition)
                || spliced;
            // A match marked, unmarked or moved on this row, or the find
            // field come, gone or changed on it.
            let found = find_changed
                && (rebroken_line
                    || marks_on(&self.find, row).ne(marks_on(&self.last_find, row))
                    || field_on(&self.find, row) != field_on(&self.last_find, row));
            // A link's run underlined or not, or its bar come, gone or
            // changed on this row.
            let linked = link_changed
                && (spans_on(&self.link, row).ne(spans_on(&self.last_link, row))
                    || bar_on(&self.link, row) != bar_on(&self.last_link, row));
            // A line at rest, or the look of the link under the pointer, come,
            // gone or moved on this row.
            let linked = linked
                || (lines_changed
                    && lines_on(self.links.lines(), row).ne(lines_on(&self.last_link_lines, row)))
                || (strikes_changed
                    && lines_on(self.links.strikes(), row)
                        .ne(lines_on(&self.last_link_strikes, row)))
                || (over_changed
                    && (hover_on(self.links.hover(), row)
                        .ne(hover_on(self.last_link_hover.as_ref(), row))
                        || bars_on(&self.hover_bars, row).ne(bars_on(&self.last_hover_bars, row))));
            // A tip's line come, gone or changed on this row.
            let tipped = tip_changed && tip_on(&self.tip, row) != tip_on(&self.last_tip, row);
            // The notice come, gone or changed on this row.
            let noticed = field_at(&notice, row) != field_at(&self.last_notice, row);
            // A row whose maps -- its order, its joining forms, its mirrored
            // and slanted cells -- were laid out or joined again otherwise
            // since the last conversion drew through them: any of them may
            // have moved with no cell of the row changing. One laid out
            // again as it was is not read so, and a frame drawn by rows
            // stays drawn by rows.
            let relaid = self
                .laid_drawn
                .is_none_or(|laid| self.bidi.orders().laid_after(row, &laid));
            // An image the CPU path drew on this row last frame is not
            // there now, or one arrived or changed on it, under the text or
            // over it: see `render_at`.
            let imaged = images == ImageRows::Repainted
                && (left_from(&self.last_image_places, &diff.gone, band)
                    || (any_new && arrived_on(&self.image_places, &diff.arrived, band)));
            // The guide is the cursor's row's: it moves with the cursor
            // and is drawn by that row's band, the cursor's own.
            let beyond = changed
                || badge_moved
                || selection_moved
                || bar_moved
                || pictured
                || imaged
                || composed
                || found
                || linked
                || tipped
                || noticed
                || relaid;
            beyond_cursor |= beyond;
            let whole = full || beyond || cursor_moved || glow_moved || guide_moved;
            self.dirty[r] = whole || trail_moved || splashed;
            // A row dirty only for the trail or the splash repaints the
            // columns their records reach on it, now and in the last frame:
            // every other pixel of it is the last frame's.
            self.repaint[r] = match (whole, self.dirty[r]) {
                (false, true) => self.records_on(band),
                _ => Repaint::Whole,
            };
        }
        self.place_diff = diff;
        if rejoin.is_some() {
            self.bidi.rejoined();
        }
        self.laid_drawn = Some(self.bidi.orders().laid_seal());
        if images == ImageRows::Repainted {
            self.last_image_places.clone_from(&self.image_places);
        }
        self.compose_inst.clear();
        if let Some(parts) = composed_parts {
            parts.append_to(&mut self.compose_inst);
        }
        // The matches' marks and the link's underline with the content;
        // the find field, its caret and the link's bar are baud's own and
        // go over the images (`chrome_inst`).
        self.compose_inst.extend_from_slice(&self.find_inst);
        self.compose_inst.extend_from_slice(&self.link_inst);
        self.compose_inst
            .extend_from_slice(self.kept_solids.solids());
        self.chrome_inst.clear();
        // baud's veils over the images: the link under the pointer's
        // fields under baud's own, the find field with its caret, the
        // link's bar, the tip's lines and the notice -- the records kept
        // while their key holds, composed again when it moved or the frame
        // is full.
        let force = match full {
            true => Force::Full,
            false => Force::No,
        };
        let styles = core::mem::take(&mut self.styles);
        let caret = core::mem::take(&mut self.find_caret);
        let mut records = core::mem::take(&mut self.chrome_inst);
        veils.records(
            force,
            &caret,
            |v, out| {
                crate::overlay::composition_records(
                    self,
                    term,
                    (&v.comp, crate::overlay::Laid::OnScreen),
                    v.under,
                    &styles,
                    None,
                    cols,
                    LineRendition::Single,
                    Px::of(0),
                    out,
                );
            },
            &mut records,
        );
        (self.styles, self.find_caret, self.chrome_inst) = (styles, caret, records);
        self.cpu_veils = veils;
        self.reach_was.clone_from(&self.reach);
        for r in 0..rows.index() {
            if self.dirty[r] {
                self.convert_row(term, Row::from_index(r), cursor, &mut exceptions);
                rows_dirty.bump();
            }
        }
        self.splice = None;
        // A converted row's glyphs that reach into another row's band, or
        // reached into it before, are drawn there by the CPU compositor:
        // that row is repainted too, from the instances it already holds.
        let mut reached: Vec<usize> = Vec::new();
        for q in 0..rows.index() {
            if !self.dirty[q] {
                continue;
            }
            for e in [self.reach_was[q], self.reach[q]] {
                if !e.leaves(self.band(Row::from_index(q))) {
                    continue;
                }
                reached.extend(
                    (0..rows.index()).filter(|&r| r != q && e.meets(self.band(Row::from_index(r)))),
                );
            }
        }
        for r in reached {
            self.dirty[r] = true;
            self.repaint[r] = Repaint::Whole;
        }
        let this = match beyond_cursor {
            true => crate::Redraw::Whole,
            false => (0..rows.index())
                .filter(|&r| self.dirty[r])
                .fold(crate::Redraw::NOTHING, |d, r| d.with(Row::from_index(r))),
        };
        self.redraw = self.redraw.union(this);
        self.blink_seen = self
            .row_blinks
            .iter()
            .fold(Blinking::UNSEEN, |a, &b| a.with(b));
        // **`prev` brought up to `cur`**, the rows compared copied: every
        // other row is the same in both already, so `cur` equals `prev` row
        // for row, and the next frame keeps what it does not take again.
        // The renditions are copied whole, and stay this frame's after it, for
        // the cursor's box a caller asks for between frames
        // (`Renderer::cursor_rect`).
        let mut copied = 0;
        for r in 0..rows.index() {
            if self.mirror_rows[r] == MirrorRow::Kept {
                continue;
            }
            let range = self.row_cells(Row::from_index(r));
            copied += range.len();
            self.prev[range.clone()].copy_from_slice(&self.cur[range]);
        }
        crate::counters::add(Counter::CellsCopied, Site::Convert, Tally::of(copied));
        self.prev_size.clear();
        self.prev_size.extend_from_slice(self.cur.sizes());
        self.cur
            .settle(term.sealed(), &now, &vp, self.bidi.orders());
        core::mem::swap(&mut self.veiled_last, &mut self.veiled_now);
        self.lent_last = lent_now;
        self.known = Prev::Known;
        self.last_cursor = cursor;
        self.last_cursor_opacity = opacity;
        self.last_cursor_look = self.cursor_look;
        self.last_glow = self.glow;
        self.last_guide_at = self.guide_at;
        self.last_badge_rect = self.badge_rect;
        self.last_badge_built = self.badge_built;
        self.last_highlight = self.highlight;
        self.last_bar = self.bar;
        self.last_trail = self.trail;
        self.last_trail_inst.clone_from(&self.trail_inst);
        self.last_fade = self.fade;
        self.last_bell = self.bell;
        self.last_splash = self.splash;
        self.last_splash_inst.clone_from(&self.splash_inst);
        self.last_clusters = clusters;
        self.last_composition = composition;
        self.last_find.clone_from(&self.find);
        self.last_link.clone_from(&self.link);
        self.last_link_lines.clear();
        self.last_link_lines.extend_from_slice(self.links.lines());
        self.last_link_strikes.clear();
        self.last_link_strikes
            .extend_from_slice(self.links.strikes());
        match (self.links.hover(), &mut self.last_link_hover) {
            (Some(h), Some(mine)) => mine.clone_from(h),
            (h, mine) => *mine = h.cloned(),
        }
        self.last_hover_bars.clone_from(&self.hover_bars);
        self.last_tip.clone_from(&self.tip);
        self.last_notice = notice;
        // **The first letter that takes a form, shaped this frame with BDSM
        // reset**, was drawn unjoined, since no row was read for joins: the
        // frame again, every row read for its joins this time, and the rows
        // that join converted again, since their joins moved. Once a
        // renderer's life.
        // The rows this pass dirtied stay dirty, for the paint after.
        if seen == crate::bidi::Joins::Unseen
            && self.joins_seen == crate::bidi::Joins::Seen
            && term.bidi_support() == BidiSupport::Explicit
        {
            let dirtied = self.dirty.clone();
            let mut again = self.convert_with(term, back, damage, images);
            for (d, was) in self.dirty.iter_mut().zip(dirtied) {
                *d |= was;
            }
            // The second pass reads this frame's records as the last's.
            self.repaint.fill(Repaint::Whole);
            // Both passes' rows and time.
            again.rows_dirty.add(rows_dirty);
            again.convert = t0.elapsed();
            return again;
        }
        Converted {
            rows_dirty,
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

    /// **What the converts since the last [`Renderer::take_redraw`]
    /// changed**: the rows dirty for the cursor, its blink animation, its
    /// trail or the cursor guide alone, or [`Redraw::Whole`] once any
    /// frame among them changed anything else. What changed only *in the
    /// converted rows*: images, the exit-status strip and a frame's size
    /// are the drawing path's to compare.
    pub fn redraw(&self) -> crate::Redraw {
        self.redraw
    }

    /// [`Renderer::redraw`], and none from here: for a path that has drawn
    /// a frame with it.
    pub fn take_redraw(&mut self) -> crate::Redraw {
        core::mem::replace(&mut self.redraw, crate::Redraw::NOTHING)
    }

    /// **The last frame's rows again with no cursor**, for a screen
    /// switch's slide, whose old screen is drawn with its cursor hidden, as
    /// sfte's outgoing pass is: the cursor, its blink animation, the cursor
    /// guide and the trail taken off, and every row they were drawn on
    /// converted again from this renderer's own copy of the last frame's
    /// cells, which is the old screen however far the terminal has moved
    /// on since. Nothing else of that frame changes. A GPU path then draws
    /// [`Renderer::instances`] as they stand; the CPU path paints them with
    /// [`Renderer::render_hidden`]. The next convert takes its change
    /// against this frame. Nothing is converted before a first frame.
    pub fn hide_cursor(&mut self, term: &Term<'_>) -> Converted {
        self.hide(term).unwrap_or_default()
    }

    /// [`Renderer::hide_cursor`], `None` where it declines: before a first
    /// frame, or after a resize or anything else that leaves this renderer
    /// no last frame to convert from (`Prev::Unknown`), when the kept frame
    /// is what the target holds.
    fn hide(&mut self, term: &Term<'_>) -> Option<Converted> {
        let t0 = Instant::now();
        let rows = self.rows.index();
        let mut exceptions = Exceptions::default();
        if self.known != Prev::Known || self.prev.len() < rows * self.cols.index() {
            return None;
        }
        // What the cursor brought, as the last frame drew it, then gone.
        let mut was: Vec<Instance> = Vec::new();
        was.extend_from_slice(&self.glow_inst);
        was.extend_from_slice(&self.guide_inst);
        was.extend_from_slice(&self.trail_inst);
        let trail_was = self.trail;
        let cursor_was = self.last_cursor.map(|(r, _, _)| r.index());
        // The fireworks are the live frame's, as the trail is: the driver
        // hands them again for the next.
        self.fireworks = [None; 2];
        self.set_trail(None);
        self.set_cursor_glow_at(None);
        self.place_grounds(None, None);
        self.reach_was.clone_from(&self.reach);
        // The rows are converted from `cur`, which every convert leaves equal
        // to `prev`, the last frame drawn. The cell path may have written
        // rows of it since (`Mirror::scribble`), so it is made `prev`'s
        // again first, which the next convert then takes whole.
        let (cells, _) = self.cur.scribble();
        cells.copy_from_slice(&self.prev);
        let mut converted = Tally::default();
        for r in 0..rows {
            let band = self.band(Row::from_index(r));
            let under = cursor_was == Some(r)
                || records_meet(&was, band)
                || trail_was.is_some_and(|d| d.overlaps(band.top, band.bottom));
            self.dirty[r] = under;
            self.repaint[r] = Repaint::Whole;
            if under {
                self.convert_row(term, Row::from_index(r), None, &mut exceptions);
                converted.bump();
            }
        }
        // A converted row's glyphs that reach into another row's band are
        // painted there too, as a convert has them.
        let mut reached: Vec<usize> = Vec::new();
        for q in (0..rows).filter(|&q| self.dirty[q]) {
            for e in [self.reach_was[q], self.reach[q]] {
                if !e.leaves(self.band(Row::from_index(q))) {
                    continue;
                }
                reached.extend(
                    (0..rows).filter(|&r| r != q && e.meets(self.band(Row::from_index(r)))),
                );
            }
        }
        for r in reached {
            self.dirty[r] = true;
        }
        self.last_cursor = None;
        self.last_glow = None;
        self.last_guide_at = self.guide_at;
        self.last_badge_rect = self.badge_rect;
        self.last_trail = None;
        self.last_trail_inst.clear();
        self.redraw = crate::Redraw::Whole;
        Some(Converted {
            rows_dirty: converted,
            rows: Tally::of(rows),
            convert: t0.elapsed(),
            exceptions,
        })
    }

    /// [`Renderer::hide_cursor`], painted into `target`, which must still
    /// hold the last frame: the grid's columns of it, past the left margin
    /// and the exit-status strip when either is kept
    /// ([`Renderer::grid_left`]), in the rows beside the activity lights'
    /// line when it is kept ([`Renderer::grid_top`]), as
    /// [`Renderer::render_at`] paints them, and
    /// the images on those rows, blitted into them alone: an image on no row
    /// painted keeps the last frame's pixels, never blitted over them, which
    /// a partly transparent one would blend twice. **An image the terminal no longer holds**
    /// -- the alternate screen's, which leaving it deletes before the
    /// switch is seen, or one whose slot holds another image now -- cannot
    /// be blitted again, so on the rows painted it is kept as the last
    /// frame drew it: whole over the text, which the cursor was under, and
    /// under the text but in the cursor's cell. **An image whose pixels
    /// moved since the last frame** -- a pixel of it written, or another of
    /// its frames shown, asked of that image (`since_placed`) -- has every
    /// row it covers painted again, whole, and is drawn there in the pixels
    /// it holds now, under the text or over it: drawn into the cursor's
    /// rows alone it showed new pixels beside old, and kept there as the
    /// last frame drew it it showed the cursor through a partly transparent
    /// image for as long as the kept frame is shown. **A gone image's
    /// pieces go back clipped to the images redrawn above it** -- whose
    /// pixels moved, drawn after it in the passes' order -- which the rows
    /// painted drew live: a piece holds what the last frame drew, an image
    /// over the gone one in its last pixels among it, and put back whole
    /// over one redrawn it showed those over the new. Nowhere else: an
    /// image the same as the last frame drew it is in the piece as it is
    /// now, and one under the gone image is under it in the piece, so a
    /// clip against either showed it through the gone image, or over it.
    /// Each piece's clip goes back right after the piece, so a gone image
    /// over the redrawn one still goes over both. Two residues, of a kept
    /// frame that draws no gone image afresh: through a partly transparent
    /// gone image over one redrawn, the redrawn one's last pixels show; and
    /// a partly transparent image redrawn over a gone one shows its new
    /// pixels with nothing of the gone one under them. Where
    /// [`Renderer::hide_cursor`] declines, with no last frame to convert
    /// from, nothing is painted for an image either: the kept frame is what
    /// the target holds.
    pub fn render_hidden(&mut self, term: &Term<'_>, target: &mut Target<'_>) -> Frame {
        let cell = self.last_cursor.map(|(r, c, _)| {
            let (x, y, w, h) = self.cursor_px(r, c, CursorStyle::SteadyBlock);
            let at = |p: Px| usize::try_from(p.value()).unwrap_or(usize::MAX);
            (
                at(x)..at(x).saturating_add(at(w)),
                at(y)..at(y).saturating_add(at(h)),
            )
        });
        let converted = self.hide(term);
        // With the activity lights' line kept, `target` is the whole frame:
        // the grid is painted into the rows beside the line.
        if self.lights.height().value() == 0 {
            return self.kept_beside(term, target, converted, cell);
        }
        let (top, h) = (self.grid_top(), self.frame_size().1);
        match target.rows(top, h) {
            Some(mut rest) => self.kept_beside(term, &mut rest, converted, cell),
            None => Frame::default(),
        }
    }

    /// [`Renderer::render_hidden`]'s paint of the rows beside the lights'
    /// line: the grid's columns of them, past the left margin and the
    /// exit-status strip when either is kept.
    fn kept_beside(
        &mut self,
        term: &Term<'_>,
        target: &mut Target<'_>,
        converted: Option<Converted>,
        cell: Option<(core::ops::Range<usize>, core::ops::Range<usize>)>,
    ) -> Frame {
        let left = self.grid_left();
        if left.value() == 0 {
            return self.paint_kept(term, target, converted, cell);
        }
        let (w, _) = self.frame_size();
        match target.columns(left, w) {
            Some(mut grid) => self.paint_kept(term, &mut grid, converted, cell),
            None => Frame::default(),
        }
    }

    /// [`Renderer::render_hidden`]'s paint into the grid's columns, `cell`
    /// the cursor's cell as the last frame drew it, `converted` `None`
    /// where [`Renderer::hide_cursor`] declined.
    fn paint_kept(
        &mut self,
        term: &Term<'_>,
        grid: &mut Target<'_>,
        converted: Option<Converted>,
        cell: Option<(core::ops::Range<usize>, core::ops::Range<usize>)>,
    ) -> Frame {
        let at = |v: i32| usize::try_from(v.max(0)).unwrap_or(0);
        type Span = (core::ops::Range<usize>, core::ops::Range<usize>);
        type Piece = (core::ops::Range<usize>, core::ops::Range<usize>, Vec<u32>);
        // A gone image's piece, its place in `image_places`' order (the
        // order the passes draw in), and what goes back over it.
        type Gone = (usize, Piece, Vec<Piece>);
        let (mut under_gone, mut over_gone): (Vec<Gone>, Vec<Gone>) = (Vec::new(), Vec::new());
        let imgs = term.images();
        // Every row an image whose pixels moved since the last frame covers
        // is painted again, whole, from the last frame's cells, and the image
        // drawn there in the pixels it holds now, under the text or over it:
        // so no row of it shows the old pixels beside a row in the new. Not
        // where the cursor's rows were not converted either: with no last
        // frame, its rows are blank.
        if converted.is_some() {
            for i in 0..self.image_places.len() {
                let p = self.image_places[i];
                if since_placed(imgs, &p) != ImageSince::Redrawn {
                    continue;
                }
                for r in 0..self.rows.index() {
                    if covers(&p, self.band(Row::from_index(r))) {
                        self.dirty[r] = true;
                        self.repaint[r] = Repaint::Whole;
                    }
                }
            }
        }
        let converted = converted.unwrap_or_default();
        // A placement's pixel columns and rows in the grid.
        let span = |p: &ImagePlacement| -> Span {
            let (x, y) = (i32::from(p.x), i32::from(p.y));
            (at(x)..at(x + i32::from(p.w)), at(y)..at(y + i32::from(p.h)))
        };
        // The images redrawn, which the rows painted draw live in their new
        // pixels, each with its place in `image_places`' order: a gone
        // image's piece is clipped to those after it alone, the ones drawn
        // over it.
        let redrawn: Vec<(usize, Span)> = self
            .image_places
            .iter()
            .enumerate()
            .filter(|&(_, p)| since_placed(imgs, p) == ImageSince::Redrawn)
            .map(|(i, p)| (i, span(p)))
            .collect();
        // An image the terminal no longer holds is kept from the last frame
        // on the rows painted ([`held`]), and the passes skip it.
        let kept = |p: &ImagePlacement| imgs.since(p.id, p.born, p.epoch) == ImageSince::Gone;
        for (i, p) in self
            .image_places
            .iter()
            .enumerate()
            .filter(|&(_, p)| kept(p))
        {
            let (x, y) = (i32::from(p.x), i32::from(p.y));
            let xs = at(x)..at(x + i32::from(p.w));
            for r in (0..self.rows.index()).filter(|&r| self.dirty[r]) {
                let band = self.band(Row::from_index(r));
                let top = at(y).max(usize::try_from(band.top.value()).unwrap_or(0));
                let bottom =
                    at(y + i32::from(p.h)).min(usize::try_from(band.bottom.value()).unwrap_or(0));
                if top < bottom && !xs.is_empty() {
                    let piece = (xs.clone(), top..bottom, grid.take(xs.clone(), top..bottom));
                    match p.layer {
                        ImageLayer::Over => over_gone.push((i, piece, Vec::new())),
                        ImageLayer::Under | ImageLayer::Below => {
                            under_gone.push((i, piece, Vec::new()))
                        }
                    }
                }
            }
        }
        let under = self.image_places.iter().any(|p| p.layer.under_text());
        let over = Some(term.images());
        let frame = match under {
            true => self.paint_rows(grid, converted, UnderImages(term.images()), over),
            false => self.paint_rows(grid, converted, NoUnder, over),
        };
        // What the rows painted drew where a piece meets an image redrawn
        // over it, taken before any piece goes back: the piece's clip.
        for (i, (xs, ys, _), clip) in under_gone.iter_mut().chain(over_gone.iter_mut()) {
            for (_, (lx, ly)) in redrawn.iter().filter(|(j, _)| *j > *i) {
                let x = xs.start.max(lx.start)..xs.end.min(lx.end);
                let y = ys.start.max(ly.start)..ys.end.min(ly.end);
                if !x.is_empty() && !y.is_empty() {
                    clip.push((x.clone(), y.clone(), grid.take(x, y)));
                }
            }
        }
        // Each piece goes back, then its clip over it, in the passes' order.
        fn put_back(grid: &mut Target<'_>, gone: Vec<Gone>) {
            for (_, (xs, ys, px), clip) in gone {
                grid.put(xs, ys, &px);
                for (xs, ys, px) in clip {
                    grid.put(xs, ys, &px);
                }
            }
        }
        if !under_gone.is_empty() {
            let fresh = cell.map(|(xs, ys)| (xs.clone(), ys.clone(), grid.take(xs, ys)));
            put_back(grid, under_gone);
            if let Some((xs, ys, px)) = fresh {
                grid.put(xs, ys, &px);
            }
        }
        put_back(grid, over_gone);
        frame
    }

    /// Paints the rows the last [`Renderer::convert`] found dirty into
    /// `target`, which must still hold the frame before.
    pub fn paint(&mut self, target: &mut Target<'_>, converted: Converted) -> Frame {
        self.paint_rows(target, converted, NoUnder, None)
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
        over: Option<&ImageStore<'_>>,
    ) -> Frame {
        let t1 = Instant::now();
        let mut frame = Frame {
            rows_dirty: converted.rows_dirty,
            rows: converted.rows,
            convert: converted.convert,
            exceptions: converted.exceptions,
            ..Frame::default()
        };
        let page = self.page();
        // Whether any row's glyphs reach out of its band, which the loop
        // below then draws into the bands they reach: once a frame, so a
        // screen whose glyphs stay in their rows asks no row about it.
        let reaching =
            (0..self.rows.index()).any(|q| self.reach[q].leaves(self.band(Row::from_index(q))));
        // The placements by row, for the passes under the text and over it
        // alike, when this frame draws images and repaints enough rows.
        if over.is_some()
            && !self.image_places.is_empty()
            && self.dirty[..self.rows.index()]
                .iter()
                .filter(|&&d| d)
                .count()
                >= INDEX_ROWS
        {
            self.place_rows
                .ensure(&self.image_places, self.rows, self.metrics.cell_h);
        }
        // The screen's instances, tallied as its rows were written: no row
        // is walked for them here.
        let totals = self.per_row.totals();
        (frame.instances, frame.colored, frame.solids) =
            (totals.instances(), totals.colored(), totals.solids());
        for r in 0..self.rows.index() {
            let row = Row::from_index(r);
            if !self.dirty[r] {
                continue;
            }
            let band = self.band(row);
            let margin = self.clear_color();
            let mut target = self.repainted(target, row);
            let target = &mut target;
            target.fill_band(
                band,
                &self.bg[self.row_cells(row)],
                self.metrics.cell_w,
                margin,
            );
            let examined = under.paint(self, target, row, page);
            frame.places_examined.add(examined);
            // The badge, then the cursor guide, over the backgrounds and the
            // images under the text, before the glyphs: first in
            // `instances()`.
            for inst in self.badge_drawn.iter().chain(&self.guide_inst) {
                target.over(inst, &self.atlas, self.blend, page, band);
            }
            // In the order `instances()` hands the GPU instance path: the
            // glyphs of the rows above that reach down into this band, the
            // row's own glyphs and decorations, the glyphs of the rows below
            // that reach up into it -- each row's glyphs and then its
            // decorations, rows top to bottom -- then the trail; baud's own
            // overlays, the scrollbar and the screen fade over everything,
            // come after the images, below the loop. The trail and the fade
            // dirtied these rows (`trail_moved`, and a fade makes the frame
            // full), so a row repainted without them would draw them out.
            // Only the glyphs whose own rows meet the band: a row that
            // reaches holds mostly glyphs that do not.
            let meets = |g: &&Instance| Reach::of(core::slice::from_ref(*g)).meets(band);
            if reaching {
                for q in (0..r).filter(|&q| self.reach[q].meets(band)) {
                    for inst in self.per_row[Row::from_index(q)][..self.glyph_ends[q]]
                        .iter()
                        .filter(meets)
                    {
                        target.over(inst, &self.atlas, self.blend, page, band);
                    }
                }
            }
            for inst in &self.per_row[row] {
                target.over(inst, &self.atlas, self.blend, page, band);
            }
            if reaching {
                for q in (r + 1..self.rows.index()).filter(|&q| self.reach[q].meets(band)) {
                    for inst in self.per_row[Row::from_index(q)][..self.glyph_ends[q]]
                        .iter()
                        .filter(meets)
                    {
                        target.over(inst, &self.atlas, self.blend, page, band);
                    }
                }
            }
            // The composition's records, over its row: `instances()` hands
            // the GPU instance path them in the same place.
            for inst in &self.compose_inst {
                target.over(inst, &self.atlas, self.blend, page, band);
            }
        }
        // The trail over every repainted row, then the cursor's glow: each
        // draw clipped to its row's band, so a pixel is drawn over in the
        // order a row's own pass would draw it. An effect's records are
        // drawn over all the rows at once, on as many threads as their
        // pixels are worth (`Target::effect_over`).
        // A repeat pass's quad carries the cap, as its record does, and a
        // fading one its opacity; an effect's records carry both in their
        // colour.
        let fill = match self.strength() {
            Strength::Full => self.cursor_fill,
            Strength::UnderThreshold => self
                .cursor_fill
                .with_alpha(flash::capped(self.cursor_fill.a(), self.flash_cap)),
        };
        let fill = fill.with_alpha(flash::capped(fill.a(), self.opacity()));
        let dirty = (0..self.rows.index()).filter(|&r| self.dirty[r]);
        match self.trail {
            Some(Drawn::Vfx(v, ..)) => {
                let pieces: Vec<(Band, Px, Px)> = dirty
                    .map(|r| {
                        let band = self.band(Row::from_index(r));
                        match self.repaint.get(r) {
                            Some(&Repaint::Cols { left, right }) => (band, left, right),
                            _ => (band, Px::of(0), Px::of(u32::MAX)),
                        }
                    })
                    .collect();
                target.effect_over(
                    &v,
                    &self.trail_inst,
                    (self.blend, page, self.trail_mix()),
                    &pieces,
                    self.threads,
                );
            }
            trail => {
                let hole = self.fireworks_hole();
                for r in dirty {
                    let band = self.band(Row::from_index(r));
                    let mut target = self.repainted(target, Row::from_index(r));
                    match (trail, hole) {
                        (Some(Drawn::Fireworks(..)), Some(hole)) => {
                            target.fireworks(&self.trail_inst, hole, self.blend, page, band)
                        }
                        (Some(Drawn::Smear(s)), _) => target.quad(&s, fill, self.blend, page, band),
                        _ => {
                            for inst in &self.trail_inst {
                                target.over(inst, &self.atlas, self.blend, page, band);
                            }
                        }
                    }
                }
            }
        }
        self.splash_layers_over(target, splash::Depth::Behind, page);
        for r in (0..self.rows.index()).filter(|&r| self.dirty[r]) {
            let band = self.band(Row::from_index(r));
            let mut target = self.repainted(target, Row::from_index(r));
            for inst in &self.glow_inst {
                target.over(inst, &self.atlas, self.blend, page, band);
            }
        }
        // The images over the text, then baud's own overlays over them
        // (`Renderer::chrome`), on the rows repainted alone: each row's
        // through its own target, so an image lands only on the pixels the
        // row drew again and an overlay a row kept stays uncovered.
        if let Some(imgs) = over {
            for r in (0..self.rows.index()).filter(|&r| self.dirty[r]) {
                let mut row = self.repainted(target, Row::from_index(r));
                self.paint_over(imgs, &mut row, page);
            }
        }
        for r in (0..self.rows.index()).filter(|&r| self.dirty[r]) {
            let band = self.band(Row::from_index(r));
            let mut target = self.repainted(target, Row::from_index(r));
            for inst in self
                .bell_inst
                .iter()
                .chain(&self.chrome_inst)
                .chain(&self.splash_inst)
                .chain(&self.bar_inst)
                .chain(&self.fade_inst)
            {
                target.over(inst, &self.atlas, self.blend, page, band);
            }
        }
        self.splash_layers_over(target, splash::Depth::Front, page);
        self.last_splash_layers.clone_from(&self.splash_layers);
        frame.composite = t1.elapsed();
        frame.cache = self.cache.stats();
        frame.atlas = self.atlas.stats();
        frame
    }

    /// **The splash's plasma layers at `depth`** ([`splash::Layer`]), each
    /// one's records as a trail's are made -- by [`Vfx::instances`], in the
    /// layers' ink, a repeat pass's bands alone and capped, its emitter's
    /// opacity on every record -- drawn over the dirty rows by the
    /// compositor every effect takes, through the slabs if any
    /// ([`splash::Slabs`]): behind, the light the layers drew is taken back
    /// off the keyline; in front, the glyphs under the layers' core are cut
    /// before they are drawn and the rim lit after. Nothing without the
    /// splash or with no layer.
    fn splash_layers_over(&self, target: &mut Target<'_>, depth: splash::Depth, page: Page) {
        if self.splash.is_none() || self.splash_layers.is_empty() {
            return;
        }
        let pieces = self.dirty_pieces();
        let ink = self.splash_ink.unwrap_or(self.splash_colours().amber);
        // Slabs whose box the frame does not hold are none.
        let slabs = self.splash_slabs.as_ref().filter(|s| {
            let (xs, ys) = s.span();
            target.take(xs, ys).len() == s.glyph.len()
        });
        let mut recs = Vec::new();
        let (front, behind) = match (slabs, depth) {
            (Some(s), splash::Depth::Front) => {
                let front = self.splash_light(splash::Depth::Front, s, &pieces, page);
                let behind = match s.rim.gain > 0.0 {
                    true => Some(self.splash_light(splash::Depth::Behind, s, &pieces, page)),
                    false => None,
                };
                if s.knot.strength > 0.0 {
                    let (xs, ys) = s.span();
                    let mut px = target.take(xs.clone(), ys.clone());
                    s.cut_knot(&mut px, &front);
                    target.put(xs, ys, &px);
                }
                (Some(front), behind)
            }
            _ => (None, None),
        };
        let before = match (slabs, depth) {
            (Some(s), splash::Depth::Behind) => {
                let (xs, ys) = s.span();
                Some(target.take(xs, ys))
            }
            _ => None,
        };
        for l in self.splash_layers.iter().filter(|l| l.depth == depth) {
            self.layer_records(l, ink, &mut recs);
            target.effect_over(
                &l.vfx,
                &recs,
                (self.blend, page, Mix::Normal),
                &pieces,
                self.threads,
            );
        }
        let Some(s) = slabs else {
            return;
        };
        let (xs, ys) = s.span();
        let mut px = target.take(xs.clone(), ys.clone());
        match (before, front, behind) {
            (Some(before), _, _) => s.cut_behind(&mut px, &before),
            (None, Some(front), Some(behind)) => s.light_rim(&mut px, &front, &behind),
            _ => return,
        }
        target.put(xs, ys, &px);
    }

    /// **A splash layer's records in `ink`**: a repeat pass's bands alone,
    /// capped as a trail's are, and its emitter's opacity on every record.
    fn layer_records(&self, l: &splash::Layer, ink: Argb, recs: &mut Vec<Instance>) {
        recs.clear();
        l.vfx.instances(self.pixel_size(), ink, recs);
        if l.vfx.emitter.strength == Strength::UnderThreshold {
            recs.retain(|r| r.kind == Kind::Band);
            flash::cap_alpha(recs, self.flash_cap);
        }
        let k = l.vfx.emitter.opacity;
        if k != Smear::OPAQUE {
            flash::cap_alpha(recs, k);
        }
    }

    /// **The splash layers' own light at `depth`** over the slabs' box, 0
    /// to 1 a pixel: the layers drawn white over black on a frame of their
    /// own, as [`Renderer::splash_layers_over`] draws them.
    fn splash_light(
        &self,
        depth: splash::Depth,
        s: &splash::Slabs,
        pieces: &[(Band, Px, Px)],
        page: Page,
    ) -> Vec<f32> {
        let (w, h) = self.pixel_size();
        let mut black = vec![0xFF00_0000u32; slots(w) * slots(h)];
        let mut recs = Vec::new();
        if let Some(mut t) = Target::new(&mut black, w, w, h) {
            for l in self.splash_layers.iter().filter(|l| l.depth == depth) {
                self.layer_records(l, Argb(0xFFFF_FFFF), &mut recs);
                t.effect_over(
                    &l.vfx,
                    &recs,
                    (self.blend, page, Mix::Normal),
                    pieces,
                    self.threads,
                );
            }
        }
        let (xs, ys) = s.span();
        let lit = Target::new(&mut black, w, w, h).map(|t| t.take(xs, ys));
        lit.unwrap_or_default()
            .into_iter()
            .map(|p| f32::from(Argb(p).r().get()) / 255.0)
            .collect()
    }

    /// The dirty rows' bands and the columns each repaints, as an effect is
    /// drawn over them.
    fn dirty_pieces(&self) -> Vec<(Band, Px, Px)> {
        (0..self.rows.index())
            .filter(|&r| self.dirty[r])
            .map(|r| {
                let band = self.band(Row::from_index(r));
                match self.repaint.get(r) {
                    Some(&Repaint::Cols { left, right }) => (band, left, right),
                    _ => (band, Px::of(0), Px::of(u32::MAX)),
                }
            })
            .collect()
    }

    /// **The threads a CPU frame draws an effect on, at most**, from the
    /// next frame: one draws every effect on the thread that renders. The
    /// pixels are the same at any number.
    pub fn set_threads(&mut self, n: core::num::NonZeroUsize) {
        self.threads = n;
    }

    /// Every instance of the current frame, row by row: what a GPU path
    /// uploads whole (D2: nobody uploads partial ranges).
    pub fn instances(&self) -> impl Iterator<Item = &Instance> + '_ {
        self.badge_drawn
            .iter()
            .chain(self.guide_inst.iter())
            .chain(self.per_row.iter().flatten())
            .chain(self.compose_inst.iter())
            .chain(self.trail_inst.iter())
            .chain(self.glow_inst.iter())
            .chain(self.bell_inst.iter())
            .chain(self.chrome_inst.iter())
            .chain(self.splash_inst.iter())
            .chain(self.bar_inst.iter())
            .chain(self.fade_inst.iter())
    }

    /// How many of [`Renderer::instances`], at the end, are baud's own
    /// overlays -- the visual bell's solid, the find field and its caret,
    /// the link preview's bar, the scrollbar's thumb and the screen fade --
    /// which a path draws
    /// after the images over the text, as kitty draws its scrollbar and
    /// hyperlink target after them (`[[source: kitty-hyperlink-target]]`).
    pub fn chrome(&self) -> Tally {
        Tally::of(
            self.bell_inst.len()
                + self.chrome_inst.len()
                + self.splash_inst.len()
                + self.bar_inst.len()
                + self.fade_inst.len(),
        )
    }

    /// The background word of every cell, row-major, `cols` by `rows`.
    pub fn background(&self) -> &[Argb] {
        &self.bg
    }

    /// The text colour every cell was last drawn in, row-major, `cols` by
    /// `rows`: what the swaps and the frame's lending left it.
    pub fn foreground(&self) -> &[Argb] {
        &self.fg
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
        style: impl Into<TextFace>,
        span: CellSpan,
    ) -> Vec<ClusterGlyph> {
        let style = style.into();
        let Some(&base) = cps.first() else {
            return Vec::new();
        };
        if crate::boxes::covers(base) {
            // A box, block or braille cell is baud's own mask, whatever is
            // attached to it: the shaper is not consulted.
            return vec![ClusterGlyph {
                id: GlyphId::from_shaper(0),
                source: GlyphSource::Procedural,
                slot: self.procedural(base, span),
                dx: 0,
                dy: 0,
                phase: Phase::ZERO,
                units: Units::ZERO,
            }];
        }
        let face = self.face_for(cps, style);
        let phase = self.phase_in(face, base, span);
        // A marked cell alone: the script its own code points name (UAX #24
        // 5.2), or `DFLT`'s for one that names none.
        let shaped = self.shape(face, Script::of_cluster(cps), Clusters::One(cps), &[], ..);
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
                units: Units::of(p),
            });
        }
        out
    }

    /// [`Renderer::cluster`] on a line drawn at `stretch`: each glyph
    /// rasterised at the box a character there takes, as the line's runs
    /// are ([`Atlas::slot_at`], [`Atlas::synthetic_at`]), its offsets
    /// scaled with it. A composition on a sized line is drawn by these.
    pub(crate) fn cluster_at(
        &mut self,
        cps: &[Codepoint],
        style: TextFace,
        span: CellSpan,
        stretch: Stretch,
    ) -> Vec<ClusterGlyph> {
        if stretch == Stretch::One {
            return self.cluster(cps, style, span);
        }
        let Some(&base) = cps.first() else {
            return Vec::new();
        };
        if crate::boxes::covers(base) {
            return vec![ClusterGlyph {
                id: GlyphId::from_shaper(0),
                source: GlyphSource::Procedural,
                slot: self.procedural_at(base, (span, crate::boxes::Facing::Written), stretch),
                dx: 0,
                dy: 0,
                phase: Phase::ZERO,
                units: Units::ZERO,
            }];
        }
        let face = self.face_for(cps, style);
        let phase = self.phase_in(face, base, span).doubled();
        let shaped = self.shape(face, Script::of_cluster(cps), Clusters::One(cps), &[], ..);
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let mut out = Vec::new();
        for p in shaped.glyphs() {
            let slot = self.atlas.slot_at(f, face, p.id, scale, phase, stretch);
            out.push(ClusterGlyph {
                id: p.id,
                source: GlyphSource::Face(face),
                slot,
                dx: scale.px(f32::from(p.dx)).round() as i32 * stretch.x(),
                dy: scale.px(f32::from(p.dy)).round() as i32 * stretch.y(),
                phase,
                units: Units::of(p),
            });
        }
        out
    }

    /// **A syllable shaped alone and drawn across exactly `cells` cells**
    /// (`crate::syllable`): its glyphs centred in the cells, to the whole
    /// pixel, by the syllable's advance -- from its leftmost pen position to
    /// the end of its last spacing glyph -- at the face's size whatever its
    /// width, never scaled: one wider than its cells overlaps the cells
    /// beside it. A Devanagari syllable inside a word is drawn with the
    /// word (`crate::word`); this draws the rest, and an input method's
    /// composition. Every glyph's offset is from the syllable's first cell.
    pub(crate) fn syllable(
        &mut self,
        cps: &[Codepoint],
        style: TextFace,
        cells: Cols,
        stretch: Stretch,
    ) -> Vec<ClusterGlyph> {
        if cps.is_empty() {
            return Vec::new();
        }
        let face = self.face_for(cps, style);
        let shaped = self.shape(face, Script::of_cluster(cps), Clusters::One(cps), &[], ..);
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let gdef = f.gdef();
        let glyphs = shaped.glyphs();
        let lo = glyphs
            .iter()
            .map(|p| i32::from(p.dx))
            .min()
            .unwrap_or(0)
            .min(0);
        let hi = glyphs
            .iter()
            .filter(|p| !gdef.is_mark(p.id))
            .filter_map(|p| Some(i32::from(p.dx) + i32::from(f.advance(p.id)?.value())))
            .max()
            .unwrap_or(0);
        let units = (hi - lo).max(0);
        let cell_w = self.metrics.cell_w.value();
        let span_px = cell_w.saturating_mul(u32::try_from(cells.index()).unwrap_or(1)) as f32;
        let adv_px = scale.px(units as f32);
        let mut out = Vec::new();
        // Centred by whole pixels: half the slack, from the cells' left
        // edge, each glyph rasterised at no phase of its own.
        let off = ((span_px - adv_px) / 2.0).round() as i32;
        for p in glyphs {
            out.push(ClusterGlyph {
                id: p.id,
                source: GlyphSource::Face(face),
                slot: self
                    .atlas
                    .slot_at(f, face, p.id, scale, Phase::ZERO, stretch),
                dx: (scale.px((i32::from(p.dx) - lo) as f32).round() as i32 + off) * stretch.x(),
                dy: scale.px(f32::from(p.dy)).round() as i32 * stretch.y(),
                phase: Phase::ZERO,
                units: Units::of(p),
            });
        }
        out
    }

    /// **The colours cell `col` of row `r` draws in under `light`**: its
    /// rendition's, swapped by the selection and the block cursor as
    /// [`Renderer::convert_row`] swaps them.
    fn paint_of(
        &self,
        r: Row,
        col: Col,
        cell: PackedCell,
        light: crate::word::Light,
    ) -> crate::cellmap::Drawn {
        let res = self
            .styles
            .get(cell.style.index())
            .copied()
            .unwrap_or_default();
        res.drawn(Swaps {
            pictured: self.pictures.at(r, col),
            line: Line::of(cell.content),
            selected: light.selected,
            hilite: self.hilite,
            block: light.block,
            blink: self.blink,
            cursor: light.cursor,
        })
    }

    /// **One piece of a word on `face`, shaped as a run** (`crate::word`):
    /// the syllables `range` of `syllables`, each a cluster at its first
    /// cell's offset from the piece's, their pens laid end to end, through
    /// the run cache under a key of their codepoints and cells -- a word's
    /// key is never a grid run's -- with the piece's advance in font units
    /// kept beside the glyphs ([`Shaped::end`]).
    ///
    /// **A syllable `split` names is a cluster a cell** -- each cell of the
    /// word from `at` holding `text.held` of its characters -- and the piece
    /// is shaped with each glyph keeping its cell
    /// ([`baud_font::ClusterLevel::Characters`]), the glyphs and pens
    /// those of the syllable shaped whole: what draws each glyph from its
    /// own cell's face ([`Renderer::lay_word`]).
    fn word_piece(
        &mut self,
        face: FaceIx,
        script: Script,
        (text, syllables): (&crate::word::WordText, &[crate::syllable::Syllable]),
        (at, split): (Col, &[bool]),
        range: core::ops::Range<usize>,
    ) -> Shaped {
        let first = syllables[range.start].at.index();
        let level = match range.clone().any(|i| split.get(i) == Some(&true)) {
            true => baud_font::ClusterLevel::Characters,
            false => baud_font::ClusterLevel::Monotone,
        };
        let mut key = Key::start()
            .fold(WORD_KEY)
            .fold(u32::try_from(face.index()).unwrap_or(0));
        if level == baud_font::ClusterLevel::Characters {
            key = key.fold(WORD_CELLS_KEY);
        }
        let mut clusters = ClusterList::default();
        for i in range {
            let part = &text.parts[i];
            for cp in part {
                key = key.fold(cp.value());
            }
            let s = syllables[i];
            key = key.fold(u32::try_from(s.cells.index()).unwrap_or(0) << 24);
            let offset = |c: usize| CellIx(u8::try_from(c - first).unwrap_or(u8::MAX));
            if split.get(i) != Some(&true) {
                clusters.push(part, offset(s.at.index()));
                continue;
            }
            let mut from = 0usize;
            for c in s.at.index()..s.at.index() + s.cells.index() {
                let n = c
                    .checked_sub(at.index())
                    .and_then(|x| text.held.get(x))
                    .copied()
                    .unwrap_or(0);
                let Some(cps) = part.get(from..from + n).filter(|cps| !cps.is_empty()) else {
                    continue;
                };
                key = key
                    .fold(u32::try_from(c - first).unwrap_or(0))
                    .fold(u32::try_from(n).unwrap_or(0));
                clusters.push(cps, offset(c));
                from += n;
            }
        }
        // A hit is a word's run, laid end to end; a grid run under the same
        // key, which only a collision of the fold could give, is shaped again.
        if let Some(hit) = self
            .cache
            .get(key)
            .filter(|h| h.pens() == baud_font::Pens::Run)
        {
            return hit;
        }
        let mut shaped = self.shape_laid(
            face,
            script,
            clusters.clusters(),
            &[],
            ..,
            (baud_font::Pens::Run, level),
        );
        shaped.set_run(self.stack.face(face));
        self.cache.put(key, shaped);
        shaped
    }

    /// **A word's pieces, each on its face, and its
    /// [`crate::word::Measure`]**, in pixels from its pen's start: each
    /// piece goes on from the one before, in font units on one face and in
    /// pixels where the face changes ([`PieceAt`]); `faces` gives each
    /// piece's face. And the face each glyph is drawn from: its piece's,
    /// or for a cell of the word from `at` that `alt` names, that face --
    /// whose ink the measure takes.
    fn word_measure(
        &mut self,
        script: Script,
        cut: (&crate::word::WordText, &[crate::syllable::Syllable]),
        (at, split, alt): (Col, &[bool], &[Option<FaceIx>]),
        pieces: &[core::ops::Range<usize>],
        faces: impl Fn(usize) -> FaceIx,
    ) -> Measured {
        let syllables = cut.1;
        let shapes: Vec<Shaped> = pieces
            .iter()
            .enumerate()
            .map(|(i, p)| self.word_piece(faces(i), script, cut, (at, split), p.clone()))
            .collect();
        let mut ink = crate::fit::Ink::NONE;
        let mut starts: Vec<PieceAt> = Vec::with_capacity(shapes.len());
        let mut drawn: Vec<Vec<FaceIx>> = Vec::with_capacity(shapes.len());
        // Where the next piece starts.
        let mut next = PieceAt {
            face: faces(0),
            px: 0.0,
            units: 0,
        };
        for (i, sh) in shapes.iter().enumerate() {
            let face = faces(i);
            if face != next.face {
                next = PieceAt {
                    face,
                    px: next.edge(self),
                    units: 0,
                };
            }
            let here = next;
            starts.push(here);
            let first = pieces
                .get(i)
                .and_then(|p| syllables.get(p.start))
                .map_or(at.index(), |s| s.at.index());
            let mut on = Vec::with_capacity(sh.glyphs().len());
            for p in sh.glyphs() {
                let cell = first + usize::from(p.cell.0);
                let from = cell
                    .checked_sub(at.index())
                    .and_then(|x| alt.get(x))
                    .copied()
                    .flatten()
                    .unwrap_or(face);
                on.push(from);
                let along = here.pen(self, p);
                let s1 = self.scales[from.index()].px(1.0);
                let f = self.stack.face(from);
                if let Some(b) = self.atlas.ink(f, from, p.id) {
                    ink = ink.with(along + b.left as f32 * s1, along + b.right as f32 * s1);
                }
            }
            drawn.push(on);
            next = here.after(sh);
        }
        let advance = next.edge(self);
        if ink.left > ink.right {
            ink = crate::fit::Ink {
                left: 0.0,
                right: advance,
            };
        }
        (shapes, starts, drawn, crate::word::Measure { advance, ink })
    }

    /// **Lays a word whose syllables hold `text`** (`crate::word`), the
    /// cells `syllables` from `at` on a row `cols` wide, in `room`, each
    /// cell in its face of `faces`: shaped as runs of syllables on one face
    /// -- each syllable on its first cell's, cut where the face changes, as
    /// a run of the grid is cut where its cells' face does -- laid end to
    /// end, on each face and then each narrower instance of it in turn, the
    /// first [`crate::word::lay`] takes, and each syllable's pen boundary,
    /// in pixels from the row's left edge.
    ///
    /// **A cell whose face is not its syllable's draws its glyphs from its
    /// own face** where that face numbers its glyphs as the syllable's does
    /// ([`GlyphOrder`]): the built Regular and Bold, and the built Italic
    /// and Bold Italic. The syllable is shaped once in its first cell's
    /// face with each cell's glyphs keeping their cell
    /// ([`Renderer::word_piece`]), and each glyph drawn at the pen that
    /// shaping gave it, from the face of the cell it keeps, at the same
    /// step of that face's narrower instances: a glyph the face forms from
    /// characters of two cells takes the cell HarfBuzz's CHARACTERS level
    /// keeps, the first matched glyph's in glyph order: the sign's for र्'s
    /// reph with ि, which the sign carries, and क्'s for the conjunct क्ष
    /// (`[[source: harfbuzz-usermanual-clusters]]`). Between faces not
    /// alike, upright and italic, the syllable is drawn whole in its first
    /// cell's face.
    fn lay_word(
        &mut self,
        (text, syllables): (&crate::word::WordText, &[crate::syllable::Syllable]),
        faces: &[TextFace],
        (at, cells, room): (Col, Cols, crate::word::Room),
        (cols, prev): (Cols, Option<f32>),
    ) -> Option<LaidWord> {
        let parts = &text.parts;
        let all: Vec<Codepoint> = parts.concat();
        if all.is_empty()
            || syllables.is_empty()
            || parts.len() != syllables.len()
            || faces.len() != cells.index()
            || text.held.len() != cells.index()
        {
            return None;
        }
        self.words_laid.bump();
        let cell_of = |c: Col| c.index().checked_sub(at.index());
        // Each syllable's face: its first cell's.
        let styles: Vec<TextFace> = syllables
            .iter()
            .map(|s| cell_of(s.at).and_then(|x| faces.get(x)).copied())
            .collect::<Option<_>>()?;
        // **Each run of syllables in one face**: a run of one style, on the
        // face its letters take in it, and runs on one face shaped as one.
        let mut runs: Vec<(core::ops::Range<usize>, FaceIx)> = Vec::new();
        let mut k = 0;
        while k < syllables.len() {
            let mut m = k + 1;
            while m < syllables.len() && styles[m] == styles[k] {
                m += 1;
            }
            let face = match (k, m) {
                (0, m) if m == syllables.len() => self.face_for(&all, styles[k]),
                _ => self.face_for(&parts[k..m].concat(), styles[k]),
            };
            match runs.last_mut() {
                Some((r, f)) if *f == face => r.end = m,
                _ => runs.push((k..m, face)),
            }
            k = m;
        }
        // **Each cell drawn from a face of its own**, where its face is not
        // its syllable's and numbers its glyphs alike: the face its letters
        // take in its rendition, and each syllable that holds one.
        let mut alt: Vec<Option<FaceIx>> = vec![None; faces.len()];
        let mut split = vec![false; syllables.len()];
        for (run, face) in &runs {
            for k in run.clone() {
                let s = syllables[k];
                for c in s.at.index()..s.at.index() + s.cells.index() {
                    let Some(x) = cell_of(Col::at_index(c)) else {
                        continue;
                    };
                    let (Some(&look), Some(&n)) = (faces.get(x), text.held.get(x)) else {
                        continue;
                    };
                    if n == 0 || look == styles[k] {
                        continue;
                    }
                    let other = self.face_for(&parts[k], look);
                    if other != *face
                        && self.orders.get(other.index()) == self.orders.get(face.index())
                    {
                        alt[x] = Some(other);
                        split[k] = true;
                    }
                }
            }
        }
        let base = runs[0].1;
        let instances: Vec<Vec<FaceIx>> = runs
            .iter()
            .map(|&(_, f)| core::iter::once(f).chain(self.stack.narrower(f)).collect())
            .collect();
        // Each such cell's face and its narrower instances, the step of
        // which it is drawn at that its syllable's run is drawn at.
        let alt: Vec<Option<Vec<FaceIx>>> = alt
            .iter()
            .map(|b| b.map(|f| core::iter::once(f).chain(self.stack.narrower(f)).collect()))
            .collect();
        let n_inst = instances.iter().map(Vec::len).max().unwrap_or(1);
        let script = Script::of_cluster(&all);
        // **A piece's pens fit its sixteen-bit offsets**: at two ems a cell at
        // most, a piece of 16,000 / upem cells -- 16 on a face of 1000 units
        // -- cut where a syllable ends, the next going on from its end; and
        // cut where the face changes.
        let mut pieces: Vec<core::ops::Range<usize>> = Vec::new();
        let mut piece_run: Vec<usize> = Vec::new();
        for (ri, (run, face)) in runs.iter().enumerate() {
            let upem = u32::from(self.stack.face(*face).units_per_em().value()).max(1);
            let per = usize::try_from(16_000 / upem)
                .unwrap_or(1)
                .clamp(1, RUN_CELLS);
            let (mut from, mut n) = (run.start, 0usize);
            for i in run.clone() {
                let s = syllables[i];
                if n > 0 && n + s.cells.index() > per {
                    pieces.push(from..i);
                    piece_run.push(ri);
                    from = i;
                    n = 0;
                }
                n += s.cells.index();
            }
            pieces.push(from..run.end);
            piece_run.push(ri);
        }
        let cut = (text, syllables);
        let cell = self.metrics.cell_w.value() as f32;
        let upem0 = u32::from(self.stack.face(base).units_per_em().value()).max(1);
        let scale0 = self.scales[base.index()];
        let grid = crate::word::Grid {
            cell,
            gap: WORD_GAP_EM * scale0.px(upem0 as f32),
            cols,
        };
        let face_at = |j: usize, i: usize| {
            let inst = &instances[piece_run[i]];
            inst[j.min(inst.len() - 1)]
        };
        let mut shaped: Vec<Option<Measured>> = (0..n_inst).map(|_| None).collect();
        let before = prev.map(|p| p - cell * at.index() as f32);
        let measure_at = |r: &mut Self, j: usize| {
            let alt_j: Vec<Option<FaceIx>> = alt
                .iter()
                .map(|inst| {
                    inst.as_ref()
                        .and_then(|v| v.get(j.min(v.len() - 1)).copied())
                })
                .collect();
            r.word_measure(script, cut, (at, &split, &alt_j), &pieces, |i| {
                face_at(j, i)
            })
        };
        let laid = crate::word::lay((at, cells), room, grid, (n_inst, before), |j| {
            let measured = measure_at(self, j);
            let m = measured.3;
            shaped[j] = Some(measured);
            Some(m)
        });
        let j = laid.instance.min(n_inst - 1);
        let (shapes, at_of, drawn, measure) = match shaped[j].take() {
            Some(s) => s,
            None => measure_at(self, j),
        };
        let x0 = cell * at.index() as f32 + laid.off;
        // Each syllable's pen boundary, from the first glyph of its cluster,
        // and the word's end; each glyph's syllable, the one holding its
        // cell.
        let n_syl = syllables.len();
        let mut bounds = vec![f32::NAN; n_syl + 1];
        let mut owner: Vec<Vec<usize>> = Vec::with_capacity(shapes.len());
        for ((piece, sh), &here) in pieces.iter().zip(&shapes).zip(&at_of) {
            let first = syllables[piece.start].at.index();
            let mut own = Vec::with_capacity(sh.glyphs().len());
            for p in sh.glyphs() {
                let c = usize::from(p.cell.0);
                let k = (piece.clone())
                    .find(|&k| {
                        let from = syllables[k].at.index() - first;
                        from <= c && c < from + syllables[k].cells.index()
                    })
                    .unwrap_or(piece.start);
                if bounds[k].is_nan() {
                    bounds[k] = x0 + here.pen(self, p);
                }
                own.push(k);
            }
            owner.push(own);
        }
        bounds[n_syl] = x0 + measure.advance;
        for k in (0..n_syl).rev() {
            if bounds[k].is_nan() {
                bounds[k] = bounds[k + 1];
            }
        }
        for k in 1..=n_syl {
            bounds[k] = bounds[k].max(bounds[k - 1]);
        }
        Some(LaidWord {
            laid: crate::word::Laid {
                instance: j,
                ..laid
            },
            shapes,
            at: at_of,
            drawn,
            owner,
            x0,
            bounds,
            measure,
        })
    }

    /// **Where each glyph of a laid word is drawn**: its syllable, the face
    /// it is drawn from, its glyph, its slot at the quarter pixel it falls
    /// on, and its place from the row's left edge, whole pixels and that
    /// quarter.
    fn word_glyph_places(&mut self, w: &LaidWord) -> Vec<GlyphPlace> {
        let mut out = Vec::new();
        for (((sh, own), &here), drawn) in w.shapes.iter().zip(&w.owner).zip(&w.at).zip(&w.drawn) {
            let shaped_at = self.scales[here.face.index()];
            for ((p, &k), &face) in sh.glyphs().iter().zip(own).zip(drawn) {
                let scale = self.scales[face.index()];
                let f = self.stack.face(face);
                let at = w.x0 + here.pen(self, p);
                let mut whole = at.floor();
                let mut q = ((at - whole) * PEN_STEPS).round() / PEN_STEPS;
                if q >= 1.0 {
                    whole += 1.0;
                    q = 0.0;
                }
                let phase = Phase::of_fraction(q);
                let slot = self
                    .atlas
                    .slot_at(f, face, p.id, scale, phase, Stretch::One);
                out.push(GlyphPlace {
                    syllable: crate::word::SyllableIx(u16::try_from(k).unwrap_or(u16::MAX)),
                    face,
                    id: p.id,
                    slot,
                    whole: whole as i32,
                    pen: whole + q,
                    dy: shaped_at.px(f32::from(p.dy)).round() as i32,
                });
            }
        }
        out
    }

    /// **A Devanagari word drawn whole** into `out` (`crate::word`): shaped
    /// as one run at its face's natural advances, laid in its columns by
    /// [`crate::word::lay`] -- centred, into the blank cells beside it,
    /// shifted, at a narrower instance, never scaled -- each glyph at its
    /// pen to a quarter of a pixel, in its syllable's colours.
    ///
    /// **A syllable the selection or the block cursor lights is lit over its
    /// pen span** (`[[source: ecma48]]` 4.2.27): a rectangle in its light's
    /// ground laid under the glyphs, reaching the word's edge cell where the
    /// cell past it is lit alike, and every glyph mask cut where the ground
    /// under it is another than its syllable's, each piece in that ground's
    /// text colour -- so no ink is drawn in the colour of the ground it lies
    /// on, whatever lit or coloured the cells under it. `under` is every
    /// cell of the row as it draws, the word's cells unlit; `paint` the
    /// word's syllables'. What every path draws a word from, in the
    /// instance path's place on the screen.
    ///
    /// **Word `i` of the kept row `kept` names** (`crate::kept`) is laid
    /// again only where it was laid after a word ending elsewhere or with a
    /// cell in another face, else painted from its kept layout in this
    /// frame's light: the cursor and the selection never lay it. A row that
    /// names none is laid.
    ///
    /// **`styles` are the faces its cells' renditions name in the table the
    /// frame draws with** ([`crate::word::faces_of`]): the renderer's own
    /// on the instance and CPU paths, the buffer set's on the cell path,
    /// which the renderer's own is not kept to. Each syllable is shaped in
    /// its first cell's face, and each glyph drawn from its own cell's
    /// where the two number their glyphs alike ([`Renderer::lay_word`]).
    pub(crate) fn word_at(
        &mut self,
        (term, styles): (&Term<'_>, &[TextFace]),
        (r, row, kept): (
            Row,
            &[PackedCell],
            Option<(crate::kept::KeptIx, crate::kept::WordIx)>,
        ),
        (w, paint, prev): (&crate::word::Word, &crate::word::WordPaint, Option<f32>),
        under: &[crate::word::Under],
        out: &mut Vec<Instance>,
    ) -> Option<crate::word::WordLaid> {
        row.get(w.at.index())?;
        let cols = Cols::of_index(row.len());
        let Some(ki) = kept else {
            let layout = self.word_layout(term, row, (w, styles), (cols, prev))?;
            return Some(self.word_painted(r, &layout, (w, paint), under, out));
        };
        if !self.kept.has_word(ki, prev, styles) {
            let layout = self.word_layout(term, row, (w, styles), (cols, prev));
            self.kept.keep_word(ki, prev, styles, layout);
        } else if cfg!(debug_assertions) {
            // A debug build lays a kept word again, uncounted, and requires
            // the layout kept to be the one it lays.
            let laid = self.words_laid;
            let again = self.word_layout(term, row, (w, styles), (cols, prev));
            self.words_laid = laid;
            debug_assert!(
                again.as_ref() == self.kept.word(ki),
                "a kept word's layout is the one its row lays"
            );
        }
        let layout = self.kept.word(ki)?;
        Some(self.word_painted(r, layout, (w, paint), under, out))
    }

    /// **Word `w` of `row` laid and placed** ([`WordLayout`]), each cell
    /// in its face of `styles`, on a row `cols` wide after a word ending at
    /// `prev`.
    fn word_layout(
        &mut self,
        term: &Term<'_>,
        row: &[PackedCell],
        (w, styles): (&crate::word::Word, &[TextFace]),
        (cols, prev): (Cols, Option<f32>),
    ) -> Option<WordLayout> {
        let held = self.cpu_lent.held();
        let text = word_text(term, held.words(), row, w);
        self.word_placed((&text, styles), w, (cols, prev))
    }

    /// **The kept row of window row `at`, holding `cells` in the order
    /// they are drawn in** (`crate::kept`), its syllables and words as
    /// `find` finds them where no row kept holds those cells so.
    pub(crate) fn kept_row(
        &mut self,
        term: &Term<'_>,
        (at, rows): (Row, Rows),
        cells: (&[PackedCell], &[Col]),
        find: impl FnOnce() -> crate::kept::Found,
    ) -> (
        crate::kept::KeptIx,
        std::rc::Rc<crate::kept::Found>,
        crate::kept::Asked,
    ) {
        let stamp = crate::kept::Stamp {
            clusters: term.cluster_epoch(),
            metrics: self.metrics,
            scale: self.scales.first().copied(),
            grown: self.atlas.stats().grown,
            theme: self.theme_gen,
            styles: term.variants_seen().then(|| {
                (
                    term.style_table(),
                    term.style_writes(),
                    self.styles_stamp.lending,
                )
            }),
        };
        self.kept.row(stamp, (at, rows.index()), cells, find)
    }

    /// How many words have been laid since the renderer was made
    /// ([`Renderer::lay_word`]): a word of a row kept since an earlier
    /// frame is painted from its layout and not counted (`crate::kept`).
    pub fn words_laid(&self) -> Tally {
        self.words_laid
    }

    /// **Where a word's ink ends** as [`Renderer::lay_word`] lays it, in
    /// pixels from the row's left edge: what the word after it closes on
    /// where that one fits at no width (`crate::word::lay`).
    pub(crate) fn word_ink(
        &mut self,
        cut: (&crate::word::WordText, &[crate::syllable::Syllable]),
        styles: &[TextFace],
        place: (Col, Cols, crate::word::Room),
        (cols, prev): (Cols, Option<f32>),
    ) -> Option<f32> {
        self.lay_word(cut, styles, place, (cols, prev))
            .map(|w| w.x0 + w.measure.ink.right)
    }

    /// [`Renderer::word_at`] for a word whose syllables hold `text`, each
    /// cell in its face of `styles`, on a row `cols` wide: what an input
    /// method's composition draws a word it holds by, the letters written
    /// beside it that it takes in among its syllables, as its commit will
    /// be drawn.
    pub(crate) fn word_drawn(
        &mut self,
        r: Row,
        (text, styles): (&crate::word::WordText, &[TextFace]),
        (w, paint, prev): (&crate::word::Word, &crate::word::WordPaint, Option<f32>),
        (under, cols): (&[crate::word::Under], Cols),
        out: &mut Vec<Instance>,
    ) -> Option<crate::word::WordLaid> {
        let layout = self.word_placed((text, styles), w, (cols, prev))?;
        Some(self.word_painted(r, &layout, (w, paint), under, out))
    }

    /// **A word whose syllables hold `text` laid and its glyphs placed**
    /// ([`WordLayout`]), each cell in its face of `styles`, on a row `cols`
    /// wide after a word ending at `prev`: what [`Renderer::word_painted`]
    /// draws it from.
    fn word_placed(
        &mut self,
        (text, styles): (&crate::word::WordText, &[TextFace]),
        w: &crate::word::Word,
        (cols, prev): (Cols, Option<f32>),
    ) -> Option<WordLayout> {
        let laid_word = self.lay_word(
            (text, &w.syllables),
            styles,
            (w.at, w.cells, w.room),
            (cols, prev),
        )?;
        let places = self.word_glyph_places(&laid_word);
        Some(WordLayout {
            laid: laid_word.laid,
            x0: laid_word.x0,
            bounds: laid_word.bounds,
            ink: laid_word.measure.ink,
            places,
        })
    }

    /// **Word `w` drawn into `out` from its layout**, in its syllables'
    /// `paint` and over `under`, every cell of row `r` as it draws: the
    /// lit rectangles over the syllables' pen spans, then each glyph at its
    /// place, cut where its ground is another's ([`Renderer::word_at`]).
    fn word_painted(
        &self,
        r: Row,
        laid_word: &WordLayout,
        (w, paint): (&crate::word::Word, &crate::word::WordPaint),
        under: &[crate::word::Under],
        out: &mut Vec<Instance>,
    ) -> crate::word::WordLaid {
        let n_syl = w.syllables.len();
        let bounds = &laid_word.bounds;
        let spans: Vec<(i32, i32)> = (0..n_syl)
            .map(|k| (bounds[k].round() as i32, bounds[k + 1].round() as i32))
            .collect();
        // The lit rectangles: each run of syllables one light lights alike.
        let cell_i = i32::try_from(self.metrics.cell_w.value())
            .unwrap_or(1)
            .max(1);
        let edge_l = i32::try_from(w.at.index()).unwrap_or(0) * cell_i;
        let edge_r = i32::try_from(w.end().index()).unwrap_or(0) * cell_i;
        let light_at = |x: Option<usize>| x.and_then(|x| under.get(x)).map(|u| u.light);
        let mut lit: Vec<(i32, i32, crate::word::Colours)> = Vec::new();
        let mut k = 0;
        while k < n_syl {
            let (colours, light, _) = paint.syllables[k];
            if !light.lit() {
                k += 1;
                continue;
            }
            let mut m = k;
            while m + 1 < n_syl
                && paint.syllables[m + 1].1 == light
                && paint.syllables[m + 1].0 == colours
            {
                m += 1;
            }
            let block = matches!(light.block, BlockCursor::Yes(_));
            let mut l = spans[k].0;
            let mut rr = spans[m].1;
            if k == 0 && !block && light_at(w.at.index().checked_sub(1)) == Some(light) {
                l = l.min(edge_l);
            }
            if m + 1 == n_syl && !block && light_at(Some(w.end().index())) == Some(light) {
                rr = rr.max(edge_r);
            }
            if rr > l {
                lit.push((l, rr, colours));
            }
            k = m + 1;
        }
        let band = self.band(r);
        let top = i32::try_from(band.top.value()).unwrap_or(0);
        let baseline = i32::try_from(self.metrics.baseline.value()).unwrap_or(0);
        for &(l, rr, c) in &lit {
            out.push(solid(
                screen(l),
                screen(top),
                Px::of(u32::try_from(rr - l).unwrap_or(0)),
                self.metrics.cell_h,
                c.bg,
            ));
        }
        // What the ground at `x` is drawn in: a lit rectangle's, else the
        // cell's own.
        let ground = |x: i32| -> crate::word::Colours {
            if let Some(&(_, _, c)) = lit.iter().find(|&&(l, rr, _)| x >= l && x < rr) {
                return c;
            }
            let c = usize::try_from(x.div_euclid(cell_i)).unwrap_or(0);
            under
                .get(c.min(under.len().saturating_sub(1)))
                .map_or(paint.syllables[0].0, |u| u.colours)
        };
        let mut pens = Vec::new();
        let mut faces = Vec::new();
        for g in &laid_word.places {
            pens.push((g.id, g.pen));
            faces.push(g.face);
            let (colours, _, text) = paint.syllables[usize::from(g.syllable.0)];
            let Some(slot) = g.slot.filter(|_| text.draws()) else {
                continue;
            };
            let inst = Instance {
                x: Screen(i16::try_from(g.whole + slot.left).unwrap_or(i16::MAX)),
                y: Screen(i16::try_from(top + baseline - g.dy + slot.top).unwrap_or(i16::MAX)),
                w: slot.w,
                h: slot.h,
                u: slot.x,
                v: slot.y,
                color: colours.fg,
                kind: slot.kind,
                curve: colours.curve,
            };
            cut_by_ground(inst, colours, self.metrics.cell_w, &lit, &ground, out);
        }
        crate::word::WordLaid {
            at: w.at,
            cells: w.cells,
            room: (
                w.room.left.0.0,
                w.room.left.1,
                w.room.right.0.0,
                w.room.right.1,
            ),
            instance: laid_word.laid.instance,
            how: laid_word.laid.how,
            ink: (
                laid_word.x0 + laid_word.ink.left,
                laid_word.x0 + laid_word.ink.right,
            ),
            syllables: w.syllables.iter().map(|s| (s.at, s.cells)).collect(),
            spans,
            pens,
            faces,
        }
    }

    /// The syllable `s` of row `r` drawn into `out` from its first cell
    /// ([`Renderer::syllable`]): every cell's base and marks, in order, as
    /// one cluster, in the first cell's style and colours.
    fn syllable_at(
        &mut self,
        term: &Term<'_>,
        r: Row,
        row: &[PackedCell],
        s: crate::syllable::Syllable,
        out: &mut Vec<Instance>,
    ) {
        let mut cps = Vec::new();
        let held = self.cpu_lent.held();
        for cell in row.iter().skip(s.at.index()).take(s.cells.index()) {
            if !cell.content.is_wide_tail() {
                cps.extend(cell_cps(term, held.words(), *cell));
            }
        }
        let Some(&first) = row.get(s.at.index()) else {
            return;
        };
        let style = self.face_style(first);
        let st = Stretch::of(self.size_of(r));
        let g = self.syllable(&cps, style, s.cells, st);
        self.emit_glyphs(r, s.at, &g, out);
    }

    /// **[`Renderer::cluster`] drawn flipped**, for a cell L4 mirrors that
    /// has no Bidi_Mirroring_Glyph ([`crate::bidi::Mirrored::Flipped`]): the
    /// first glyph, the base's, flipped left to right
    /// ([`Atlas::slot_flipped_at`]), and every glyph's place mirrored in
    /// `span` ([`InSpan::mirrored`]), a mark keeping its own image -- what
    /// [`Renderer::emit`] draws for the same cell on the instance path.
    pub fn cluster_flipped(
        &mut self,
        cps: &[Codepoint],
        style: impl Into<TextFace>,
        span: CellSpan,
    ) -> Vec<ClusterGlyph> {
        self.cluster_flipped_at(cps, style.into(), span, Stretch::One)
    }

    /// [`Renderer::cluster_flipped`] on a line drawn at `stretch`, as
    /// [`Renderer::cluster_at`] draws [`Renderer::cluster`] there: what
    /// [`Renderer::emit`] draws for a flipped cell on a sized line. A
    /// composition on a mirrored line is drawn by these.
    pub(crate) fn cluster_flipped_at(
        &mut self,
        cps: &[Codepoint],
        style: TextFace,
        span: CellSpan,
        stretch: Stretch,
    ) -> Vec<ClusterGlyph> {
        let Some(&base) = cps.first() else {
            return Vec::new();
        };
        if crate::boxes::covers(base) {
            // A box drawing character mirrored whole in its cells
            // ([`crate::boxes::Facing`]), whatever is attached to it.
            return vec![ClusterGlyph {
                id: GlyphId::from_shaper(0),
                source: GlyphSource::Procedural,
                slot: self.procedural_at(base, (span, crate::boxes::Facing::Mirrored), stretch),
                dx: 0,
                dy: 0,
                phase: Phase::ZERO,
                units: Units::ZERO,
            }];
        }
        let face = self.face_for(cps, style);
        let phase = match stretch {
            Stretch::One => self.phase_in(face, base, span),
            Stretch::Tall | Stretch::Wide => self.phase_in(face, base, span).doubled(),
        };
        let shaped = self.shape(face, Script::of_cluster(cps), Clusters::One(cps), &[], ..);
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0) * stretch.x();
        let width = cell_w * i32::try_from(span.cells().index()).unwrap_or(1);
        let mut out = Vec::new();
        for (i, p) in shaped.glyphs().iter().enumerate() {
            let slot = match i {
                0 => self
                    .atlas
                    .slot_flipped_at(f, face, p.id, scale, phase, stretch),
                _ => self.atlas.slot_at(f, face, p.id, scale, phase, stretch),
            };
            let dx = scale.px(f32::from(p.dx)).round() as i32 * stretch.x();
            out.push(ClusterGlyph {
                id: p.id,
                source: GlyphSource::Face(face),
                slot,
                dx: slot.map_or(dx, |s| InSpan { width, dx }.mirrored(s)),
                dy: scale.px(f32::from(p.dy)).round() as i32 * stretch.y(),
                phase,
                units: Units::of(p),
            });
        }
        out
    }

    /// **How far a cell in `style` leans when SAPV 17 slants it left**: its
    /// italic face's angle ([`crate::stack::Stack::lean`]), or `None` for a
    /// style that draws upright.
    pub(crate) fn lean(&self, style: TextFace) -> Option<crate::stack::Lean> {
        self.stack.lean(style)
    }

    /// **`glyphs` sheared left by `lean`**, as a cell SAPV 17 slants left
    /// draws them: each glyph's image from a face sheared about the
    /// baseline ([`Atlas::slot_sheared_at`]), its place kept -- the shear
    /// moves `left` by what the image moved -- and the first flipped
    /// before it is sheared with `flip`, a mirrored cell's base. A glyph
    /// baud draws itself, box drawing and the like, is drawn upright, as
    /// its italic is.
    pub(crate) fn shear_glyphs(
        &mut self,
        glyphs: &mut [ClusterGlyph],
        lean: crate::stack::Lean,
        stretch: Stretch,
        flip: Flip,
    ) {
        for (i, g) in glyphs.iter_mut().enumerate() {
            let (GlyphSource::Face(face), Some(slot)) = (g.source, g.slot) else {
                continue;
            };
            let flip = match i {
                0 => flip,
                _ => Flip::Kept,
            };
            let f = self.stack.face(face);
            let scale = self.scales[face.index()];
            let own = match flip {
                Flip::Kept => self.atlas.slot_at(f, face, g.id, scale, g.phase, stretch),
                Flip::Flipped => self
                    .atlas
                    .slot_flipped_at(f, face, g.id, scale, g.phase, stretch),
            };
            let sheared =
                self.atlas
                    .slot_sheared_at(f, face, g.id, scale, g.phase, stretch, (flip, lean));
            if let (Some(own), Some(s)) = (own, sheared) {
                g.slot = Some(crate::atlas::Slot {
                    left: slot.left + s.left - own.left,
                    top: slot.top + s.top - own.top,
                    ..s
                });
            }
        }
    }

    /// The word around a joined cell holding `cps` in `style`, at visual
    /// `at` ([`Renderer::word_on`]), on the face that draws it.
    pub(crate) fn joined_word(
        &mut self,
        (term, styles, frame): (&Term<'_>, &[Resolved], Words<'_>),
        cps: &[Codepoint],
        style: TextFace,
        (around, at): (WordAround<'_>, (Row, Col)),
    ) -> Word {
        let face = self.face_for(cps, style);
        self.word_on((term, styles, frame), face, around, at)
    }

    /// [`Renderer::cluster`] for a cell whose letter the layout joined:
    /// shaped in `form` ([`crate::bidi::Orders::joined`]) with `word`
    /// around it ([`Renderer::joined_word`]).
    pub(crate) fn cluster_joined(
        &mut self,
        cps: &[Codepoint],
        style: impl Into<TextFace>,
        form: baud_domain::JoiningForm,
        word: &Word,
    ) -> Vec<ClusterGlyph> {
        let style = style.into();
        let Some(&base) = cps.first() else {
            return Vec::new();
        };
        let face = self.face_for(cps, style);
        let phase = self.phase_in(face, base, CellSpan::One);
        let shaped = self.shape_in_word(
            face,
            Script::of_cluster(cps),
            Clusters::One(cps),
            &[Some(form)],
            word,
        );
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
                units: Units::of(p),
            });
        }
        out
    }

    /// [`Renderer::cluster_joined`] on a line drawn at `stretch`, as
    /// [`Renderer::cluster_at`] draws [`Renderer::cluster`] there: what the
    /// line's own runs draw for a joined cell. A composition on a sized
    /// line is drawn by these.
    pub(crate) fn cluster_joined_at(
        &mut self,
        cps: &[Codepoint],
        style: TextFace,
        form: baud_domain::JoiningForm,
        word: &Word,
        stretch: Stretch,
    ) -> Vec<ClusterGlyph> {
        if stretch == Stretch::One {
            return self.cluster_joined(cps, style, form, word);
        }
        let Some(&base) = cps.first() else {
            return Vec::new();
        };
        let face = self.face_for(cps, style);
        let phase = self.phase_in(face, base, CellSpan::One).doubled();
        let shaped = self.shape_in_word(
            face,
            Script::of_cluster(cps),
            Clusters::One(cps),
            &[Some(form)],
            word,
        );
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let mut out = Vec::new();
        for p in shaped.glyphs() {
            let slot = self.atlas.slot_at(f, face, p.id, scale, phase, stretch);
            out.push(ClusterGlyph {
                id: p.id,
                source: GlyphSource::Face(face),
                slot,
                dx: scale.px(f32::from(p.dx)).round() as i32 * stretch.x(),
                dy: scale.px(f32::from(p.dy)).round() as i32 * stretch.y(),
                phase,
                units: Units::of(p),
            });
        }
        out
    }

    /// **A lam-alef pair drawn together** (`[[source: unicode-ch9]]` 9.2.3):
    /// the lam's codepoints, in `lam_form`, and the alef's, final, shaped as
    /// one run in that order, so the face's `rlig` -- or the presentation
    /// form, on a face without `GSUB` forms -- forms the obligatory ligature.
    /// The ligature is centred over the cells `span` gives, as a character
    /// two cells wide is, and the marks placed on it move with it; a face
    /// that forms none draws each letter in its own cell. Answers the glyphs
    /// the lam's cell draws and those the alef's does, each from its own
    /// cell's origin -- both from the lam's for [`PairSpan::LamCell`] -- on a
    /// line drawn at `stretch`, and whether the face formed the ligature;
    /// `None` when the two letters are drawn by different faces, and are
    /// drawn each alone. `word` is the cells around the pair its face draws
    /// ([`Renderer::word_on`]), before the lam and after the alef, shaped
    /// with it.
    pub(crate) fn lam_alef(
        &mut self,
        (lam, alef): (&[Codepoint], &[Codepoint]),
        lam_form: baud_domain::JoiningForm,
        style: TextFace,
        (span, word): (PairSpan, &Word),
        stretch: Stretch,
    ) -> Option<PairGlyphs> {
        let face = self.face_for(lam, style);
        if self.face_for(alef, style) != face {
            return None;
        }
        let mut key = fold_form(
            Key::start().fold(u32::try_from(face.index()).unwrap_or(0)),
            FormKey::Pair,
            lam_form,
        );
        for c in lam
            .iter()
            .chain([Codepoint::from_char('\u{FFFF}')].iter())
            .chain(alef)
        {
            key = key.fold(c.value());
        }
        let key = word.fold(key);
        let shaped = match self.cache.get(key) {
            Some(s) => s,
            None => {
                let mut clusters = ClusterList::default();
                clusters.push(lam, CellIx(0));
                clusters.push(alef, CellIx(1));
                let forms = [Some(lam_form), Some(baud_domain::JoiningForm::Final)];
                let s = self.shape_in_word(
                    face,
                    Script::of_cluster(lam),
                    clusters.clusters(),
                    &forms,
                    word,
                );
                self.cache.put(key, s);
                s
            }
        };
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0) * stretch.x();
        let ligature = shaped.glyphs().iter().find(|p| p.last != p.cell).copied();
        let grid = match stretch.x() {
            1 => self.metrics.phase,
            _ => self.metrics.phase.doubled(),
        };
        let cells = u32::try_from(span.cells().index()).unwrap_or(u32::MAX);
        let phase = match ligature {
            Some(l) => {
                let over = Px::of(self.metrics.cell_w.value().saturating_mul(cells));
                let centred = f
                    .default_advance(l.id)
                    .and_then(|adv| Phase::in_span(scale, adv, over))
                    .unwrap_or(self.metrics.phase);
                match stretch.x() {
                    1 => centred,
                    _ => centred.doubled(),
                }
            }
            None => grid,
        };
        let (mut lams, mut alefs) = (Vec::new(), Vec::new());
        // The cells from the span's left one to its right one.
        let across = i32::try_from(cells.saturating_sub(1)).unwrap_or(0) * cell_w;
        for p in shaped.glyphs() {
            let of_lam = p.cell == CellIx(0);
            // A ligature and what is placed on it are drawn from the span's
            // left cell; each is answered from its own cell.
            let right = match span {
                PairSpan::Cells(lam_at, _) => of_lam == (lam_at == LamAt::Right),
                PairSpan::LamCell => false,
            };
            let shift = match (ligature.is_some(), right) {
                (true, true) => -across,
                _ => 0,
            };
            let slot = self.atlas.slot_at(f, face, p.id, scale, phase, stretch);
            let g = ClusterGlyph {
                id: p.id,
                source: GlyphSource::Face(face),
                slot,
                dx: scale.px(f32::from(p.dx)).round() as i32 * stretch.x() + shift,
                dy: scale.px(f32::from(p.dy)).round() as i32 * stretch.y(),
                phase,
                units: Units::of(p),
            };
            match of_lam {
                true => lams.push(g),
                false => alefs.push(g),
            }
        }
        let ligated = match ligature {
            Some(_) => Ligated::Yes,
            None => Ligated::No,
        };
        Some(PairGlyphs {
            lams,
            alefs,
            ligated,
        })
    }

    /// The glyph a codepoint draws as a cluster of one: the
    /// face the stack selects, shaped alone so a single substitution still
    /// applies, its atlas slot with the shaper's offsets folded into the
    /// placement. `None` when it draws nothing. What the cell path's map
    /// holds.
    pub fn single(
        &mut self,
        cp: Codepoint,
        style: impl Into<TextFace>,
        span: CellSpan,
    ) -> Option<Single> {
        let style = style.into();
        if crate::boxes::covers(cp) {
            // `PairRole::Neither` is load-bearing: with both pair bits clear
            // the cell is never a ligature candidate, never gathered into a
            // window and never re-shaped by a font, so the face field is
            // never read for it.
            return Some(Single {
                slot: self.procedural(cp, span),
                glyph: GlyphId::from_shaper(0),
                role: PairRole::Neither,
                face: FaceIx::PRIMARY,
                glyphs: Glyphs::One,
                overflow: crate::fit::Overflow::Inside,
            });
        }
        let face = self.face_for(core::slice::from_ref(&cp), style);
        // A code point alone: its own script, or `DFLT`'s for one with none
        // -- the specification's "text [that] does not have a specific
        // script". In a row, a Common character takes its neighbours'
        // script instead. The two agree wherever the two select the same
        // lookups; on STIX Two Math, the one embedded face whose scripts
        // differ, the only lookup `DFLT` adds to `latn` is a ligature that
        // begins on a Greek capital, which no Common code point is.
        let shaped = self.shape(
            face,
            Script::of(cp),
            Clusters::One(core::slice::from_ref(&cp)),
            &[],
            ..,
        );
        let p = *shaped.glyphs().first()?;
        let glyphs = match shaped.glyphs().len() {
            1 => Glyphs::One,
            _ => Glyphs::Several,
        };
        let scale = self.scales[face.index()];
        // A letter of a joining script over 1.1 cells wide, which the
        // overlay draws by a record so rule D judges it against the cells
        // beside it (`crate::fit`).
        let overflow = match crate::fit::takes_forms(cp) {
            true => {
                let mine = shaped
                    .glyphs()
                    .iter()
                    .map(|q| (q.id, scale.px(f32::from(q.dx))));
                let ink = self.ink_of(face, mine, Stretch::One);
                let cell = self.metrics.cell_w.value() as f32;
                match crate::fit::Overwide::of(ink, cell) {
                    Some(_) => crate::fit::Overflow::Past,
                    None => crate::fit::Overflow::Inside,
                }
            }
            false => crate::fit::Overflow::Inside,
        };
        let f = self.stack.face(face);
        let role = self.pairs[self.layouts[face.index()]].role(p.id);
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
            glyphs,
            overflow,
        })
    }

    /// Whether two adjacent glyphs on the face a map entry names can
    /// begin a substitution there.
    pub fn admits(&self, face: Code, a: GlyphId, b: GlyphId) -> bool {
        usize::try_from(face.0)
            .ok()
            .and_then(|i| self.layouts.get(i))
            .and_then(|&l| self.pairs.get(l))
            .is_some_and(|p| p.admits(a, b))
    }

    /// The longest rule the face a map entry names can match, in glyphs.
    pub fn extent(&self, face: Code) -> Option<Count> {
        usize::try_from(face.0)
            .ok()
            .and_then(|i| self.layouts.get(i))
            .and_then(|&l| self.extents.get(l))
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
        let (scripts, picks): (Vec<Script>, Vec<Selection>) =
            match (self.layouts.get(facex.index())).and_then(|&l| self.selections.get(l)) {
                Some(sel) if sel.varies == Varies::Yes => {
                    let unresolved: Vec<Unresolved> =
                        cells.iter().map(|(cps, _)| Unresolved::of(cps)).collect();
                    let mut settled = vec![Script::COMMON; cells.len()];
                    // As a row (`settle_row`): nothing that could part
                    // from Common is all Common.
                    let may_part = unresolved.iter().any(|u| match *u {
                        Unresolved::Named(s) => !sel.alike[s],
                        Unresolved::Neutral { joins, .. } => !joins.is_empty(),
                    });
                    if may_part {
                        baud_font::resolve(&unresolved, &mut settled);
                    }
                    settled.iter().map(|&s| sel.pick(s)).unzip()
                }
                Some(sel) => (
                    vec![Script::COMMON; cells.len()],
                    vec![sel.by_script[Script::COMMON]; cells.len()],
                ),
                None => (
                    vec![Script::COMMON; cells.len()],
                    vec![Selection::default(); cells.len()],
                ),
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
            let mut clusters = ClusterList::default();
            for (i, (cps, _)) in cells[lo..hi].iter().enumerate() {
                clusters.push(cps, CellIx(u8::try_from(i).unwrap_or(u8::MAX)));
            }
            // What the context cells drop is counted by the chunk that
            // owns them, so a mark is counted once however the window is
            // cut.
            let own = CellIx(u8::try_from(at - lo).unwrap_or(u8::MAX))
                ..CellIx(u8::try_from(end - lo).unwrap_or(u8::MAX));
            let shaped = self.shape(facex, script, clusters.clusters(), &[], own);
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
                        units: Units::of(p),
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
        let table = term.styles().one().slots();
        let mut n = Tally::default();
        let mut blinking = Tally::default();
        for (slot, dst) in table.iter().zip(out.iter_mut()) {
            *dst = self.resolve_slot(term, slot.unwrap_or(baud_domain::StyleWord::DEFAULT));
            if dst.shapes.blinks() {
                blinking.bump();
            }
            self.rings = self.rings.after(dst.shapes);
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
        let slots = term.styles().one().slots();
        // A copy of this table under these inputs replays; a reader ahead
        // of the log, which only a copy of another table can be, is told
        // every slot by `styles_written` itself.
        let fresh = stamp.table == Some(table) && stamp.inputs == Some(inputs);
        let mut sync = StyleSync::default();
        if !fresh {
            sync.pass = Pass::Full;
            stamp.blinking = Tally::default();
            stamp.decorated = Tally::default();
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
                    if fresh && dst.shapes.any(Shapes::DECORATIONS) {
                        stamp.decorated.lose();
                    }
                    if new.shapes.any(Shapes::DECORATIONS) {
                        stamp.decorated.bump();
                    }
                    self.rings = self.rings.after(new.shapes);
                    *dst = new;
                    sync.resolved.bump();
                }
                // **A copy resolved afresh from the log** resolved the slots
                // written since the table began and left the rest as an
                // earlier table had them: no cell names one of those now, but
                // a later write to one would take back a count it never
                // gave. Counted whole, so a slot a later write replaces was
                // counted if it carries a decoration.
                if !fresh {
                    stamp.decorated = Tally::of(
                        out.iter()
                            .take(slots.len())
                            .filter(|d| d.shapes.any(Shapes::DECORATIONS))
                            .count(),
                    );
                }
            }
            baud_core::Written::Every => {
                sync.pass = Pass::Full;
                stamp.blinking = Tally::default();
                stamp.decorated = Tally::default();
                for (slot, dst) in slots.iter().zip(out.iter_mut()) {
                    *dst = self.resolve_slot(term, slot.unwrap_or(baud_domain::StyleWord::DEFAULT));
                    if dst.shapes.blinks() {
                        stamp.blinking.bump();
                    }
                    if dst.shapes.any(Shapes::DECORATIONS) {
                        stamp.decorated.bump();
                    }
                    self.rings = self.rings.after(dst.shapes);
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
            decorated: stamp.decorated,
            reclaims: Some(reclaims),
            lending: stamp.lending,
        };
        self.blink_slots = stamp.blinking;
        sync
    }

    /// What a resolved rendition depends on beside its word: the theme, the
    /// video and the core's colours.
    pub(crate) fn style_inputs(&self, term: &Term<'_>) -> StyleInputs {
        StyleInputs {
            theme: self.theme_gen,
            video: self.video,
            colors: term.color_epoch(),
        }
    }

    /// **One rendition resolved as a slot of the table is**: what a reader
    /// of a row frozen off the screen draws a cell in, from its word
    /// (`Term::style_in`).
    pub fn resolved_word(&self, term: &Term<'_>, style: baud_domain::StyleWord) -> Resolved {
        self.resolve_slot(term, style)
    }

    /// One rendition resolved through the theme and the live palette to what
    /// a cell draws with, before anything a frame lays over it -- the
    /// selection, the cursor, the blink's phase ([`Resolved::drawn`]). The one
    /// spelling of the rendition rules: the cell path's table holds it a slot
    /// at a time and the CPU and instance paths take it a cell at a time.
    pub(crate) fn resolve_slot(&self, term: &Term<'_>, style: baud_domain::StyleWord) -> Resolved {
        // xterm's special colours first, while a mode is on: the rendition
        // a cell draws with, its attribute dropped where a colour replaced
        // it (`ColorModes::apply`), so everything below -- the colours, the
        // face, the underline, the blink -- reads the one rendition.
        // A colour no program set is the text foreground, the stored one
        // with any `OSC 10` override, as the core's `?` answers it -- for
        // colorRV too, which lands on the page.
        let style = term
            .color_modes()
            .apply(style, |c| match term.special_color(c) {
                Some((r, g, b)) => baud_domain::ColorWord::rgb(
                    Chan::from_u8(r),
                    Chan::from_u8(g),
                    Chan::from_u8(b),
                ),
                None => {
                    let (fg, _) = self
                        .theme
                        .defaults(Video::Normal, |d| term.dynamic_color(d));
                    baud_domain::ColorWord::rgb(fg.r(), fg.g(), fg.b())
                }
            });
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
        // **The floor again, on the glass**, measured against the page at
        // its alpha over what the material can show, `Theme::backdrop` --
        // black to white on the glass -- since the renderer draws onto a
        // target with nothing under it and the glass is the window server's
        // (`Floor::lift_on_glass`). It covers a page the program set; the
        // default page is left out: its own text is held at the bound by
        // the dims (note AS, section 3). A picture's, an opaque rendition's
        // and a swap's page is solid, and `Resolved::drawn` gives them
        // `fg`.
        let glass = if page_word.is_default() || opaque_bg || self.theme.bg_alpha.get() == 0xFF {
            fg
        } else {
            self.theme
                .floor
                .lift_on_glass(fg, bg, self.theme.bg_alpha, self.theme.backdrop)
        };
        // **Every background is translucent, not only the default one**.
        // A picture's and an opaque rendition's are made solid by
        // `Resolved::drawn`. A reversed default page -- `SGR 7`'s bar,
        // `?5`'s screen -- is raised until the default text reads on it
        // (`Theme::raised`); its text is the page's own, so the glass floor
        // above leaves it, as it leaves the default page's.
        let alpha = match Theme::raised(style, self.video) {
            true => self.reversed.0,
            false => self.theme.bg_alpha,
        };
        let bg = bg.with_alpha(alpha);
        let attrs = style.attrs();
        // The font the text is drawn in (`Stack::font_of`): the primary,
        // whose style is the one the chain has a face for, or an
        // alternative or Fraktur, whose style is the one asked for, since
        // the font's own family may have a face the primary lacks.
        let font = self.stack.font_of(style.font(), attrs);
        let face_style = match font == FontSlot::PRIMARY {
            true => self.stack.effective(FaceStyle::of(attrs)),
            false => FaceStyle::of(attrs),
        };
        let mut shapes = Shapes::default().with_face(face_style).with_font(font);
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
            (Attrs::ENCIRCLED, Shapes::ENCIRCLED),
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
        // An explicit `SGR 58` colour gets the same rule against the
        // page; a following underline is the floored text.
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

    /// The face style and font a cell's text is drawn from, from its
    /// resolved slot.
    fn face_style(&self, cell: PackedCell) -> TextFace {
        crate::cellmap::face_of(&self.styles, cell)
    }

    /// Whether `face`'s scripts select different lookups.
    fn varies(&self, face: FaceIx) -> Varies {
        self.layouts
            .get(face.index())
            .and_then(|&l| self.selections.get(l))
            .map_or(Varies::No, |s| s.varies)
    }

    /// Settles the script of every cell of `row` in context into
    /// [`Renderer::row_scripts`]. A row with no cell that could settle to a
    /// script some varying face shapes apart from Common ([`Selections::alike`])
    /// is left all Common, which [`Selections::pick`] would make of it
    /// anyway: no named script apart and no cell whose Script_Extensions
    /// could take one, ASCII skipped outright while Latin and Common are
    /// alike everywhere. So a Latin row on the embedded faces, which vary
    /// for Devanagari alone, is not resolved.
    fn settle_row(&mut self, term: &Term<'_>, row: &[PackedCell]) {
        self.row_scripts.clear();
        self.row_scripts.resize(row.len(), Script::COMMON);
        let apart = |s: Script| {
            self.selections
                .all()
                .iter()
                .any(|sel| sel.varies == Varies::Yes && !sel.alike[s])
        };
        let ascii_alike = !apart(Script::COMMON) && !apart(Script::of(Codepoint::from_char('a')));
        let may_part = row.iter().any(|&cell| {
            let Some(base) = drawn_base(term, cell) else {
                return false;
            };
            if ascii_alike && base.value() < 0x80 && !cell.flags.is_marked() {
                return false;
            }
            match unresolved(term, cell) {
                Unresolved::Named(s) => apart(s),
                Unresolved::Neutral { joins, .. } => !joins.is_empty(),
            }
        });
        if !may_part {
            return;
        }
        self.row_unresolved.clear();
        for cell in row {
            self.row_unresolved.push(unresolved(term, *cell));
        }
        baud_font::resolve(&self.row_unresolved, &mut self.row_scripts);
    }

    /// The script a run on `face` from column `c` of the row being
    /// converted is shaped in, and the lookups that selects there. A face
    /// whose scripts all select alike is shaped as Common, which is the
    /// same shaping.
    fn script_at(&self, face: FaceIx, col: Col) -> (Script, Selection) {
        match (self.layouts.get(face.index())).and_then(|&l| self.selections.get(l)) {
            Some(sel) if sel.varies == Varies::Yes => {
                let s = self
                    .row_scripts
                    .get(col.index())
                    .copied()
                    .unwrap_or(Script::COMMON);
                sel.pick(s)
            }
            _ => (Script::COMMON, Selection::default()),
        }
    }

    /// **A composition's records on the instance path**, as
    /// [`crate::overlay::composition_parts`] makes them: on a row the
    /// layout joined, its pages and glyphs kept for that row's conversion
    /// to draw in the row's order ([`Splice`]), and the rest -- its rules,
    /// and on any other row the whole -- answered for after the rows.
    fn composed_parts(
        &mut self,
        term: &Term<'_>,
        c: &Composition,
        under: PackedCell,
        cursor: Cursor,
        cols: Cols,
    ) -> crate::overlay::Composed {
        let styles = core::mem::take(&mut self.styles);
        let size = self.size_of(c.row);
        let mut parts = crate::overlay::composition_parts(
            self,
            term,
            (c, crate::overlay::Laid::AsWritten),
            under,
            &styles,
            cursor,
            cols,
            size,
            Px::of(0),
        );
        self.styles = styles;
        self.splice = parts.splice(self.bidi.orders(), c.row);
        self.taken.set_inks(parts.inks.clone());
        parts
    }

    /// Resolves one row's colours and glyphs.
    fn convert_row(&mut self, term: &Term<'_>, r: Row, cursor: Cursor, exc: &mut Exceptions) {
        let range = self.row_cells(r);
        crate::counters::add(
            Counter::CellsConverted,
            Site::Convert,
            Tally::of(range.len()),
        );
        // **The row's buffers** ([`RowScratch`]), the renderer's, taken for
        // this conversion and put back after it, each empty from `begin`:
        // a row converted costs no buffer of its own.
        let mut scratch = RowScratch::begin(self.row_scratch.take());
        // A copy of the row, so shaping may borrow the renderer mutably.
        scratch.row.extend_from_slice(&self.cur[range.clone()]);
        let n = scratch.row.len();
        // **A reordered row's map, copied for the same reason**: the logical
        // column each visual cell shows, which the selection and the
        // pictures are kept by, and where shaping must stop (`bidi.rs`).
        let orders = self.bidi.orders();
        if orders.reordered(r) {
            scratch
                .logical
                .extend((0..n).map(|v| orders.logical_col(r, Col::at_index(v))));
            scratch
                .seams
                .extend((0..n).map(|v| orders.seam(r, Col::at_index(v))));
        }
        let (row, logical, seams) = (&scratch.row[..], &scratch.logical[..], &scratch.seams[..]);
        let cursor_visual = cursor.map(|(cr, cc, s)| (cr, cc, orders.visual_col(cr, cc), s));
        let logical_of = |v: Col| logical.get(v.index()).copied().unwrap_or(v);
        // **On a display line, the stored cell each visual cell shows**
        // ([`crate::rebreak`]), which a selection is kept by.
        if orders.display().is_some() {
            scratch
                .stored
                .extend((0..n).map(|v| orders.stored_line_at(r, logical_of(Col::at_index(v)))));
        }
        let stored = &scratch.stored[..];
        // **Only a row of the window takes a selection.** The status line's
        // row and the notice rows after it are no line of the window, but a
        // selection's unclipped ends (`Highlight::ends`) run past the
        // window's last row onto them when it ends below a scrolled-back
        // window, so the selection is asked nothing there, and the hilite,
        // which swaps only a selected cell, goes with it.
        let highlight = self.highlight.filter(|_| self.is_window_row(r));
        let selects = |h: &Highlight, col: Col, content: baud_domain::Content| match stored
            .get(col.index())
        {
            Some(Some((sl, sc))) => h.selects_line(*sl, *sc, content),
            Some(None) => false,
            None => h.selects(r, logical_of(col), content),
        };
        let mut instances = self.per_row.take(r);
        instances.clear();
        // **The line's rendition** (`[[source: dec-std-070]]` § 5.6.1): on a
        // line that is not single-width each character is drawn over two
        // columns, from the half of the cells it holds -- one more on an odd
        // width, the column no character's field covers, drawn as the cell
        // past the line is -- and every shape through the line's geometry.
        let size = self.size_of(r);
        let st = Stretch::of(size);
        let (chars, line_end) = match size.is_double() {
            true => (
                row.len().div_ceil(2),
                size.end_of_line(Cols::of_index(row.len())).index(),
            ),
            false => (row.len(), row.len()),
        };
        // **The row's Devanagari words** (`crate::word`), on a single-width
        // line: each drawn whole, its cells' own colours unlit and the
        // selection and the block cursor drawn over its syllables' spans.
        // The syllables the core parted, which the words and the runs read.
        // A row that holds none is not walked: as this frame's layout found
        // it in its own pass over the row, else by that pass of a block
        // table (`crate::syllable::may_hold`).
        let walked = &row[..line_end.min(row.len())];
        let held = match self.bidi.orders().syllables(r) {
            crate::bidi::Opens::Yes => true,
            crate::bidi::Opens::No => false,
            crate::bidi::Opens::Unknown => crate::syllable::may_hold(term, walked),
        };
        // **Kept while the row is the same** (`crate::kept`): a single-width
        // row which no display re-breaks and no composition is on is walked
        // and its words laid only where no row kept holds its cells in the
        // order they are drawn in.
        let keep = held
            && !size.is_double()
            && !self.taken.on(r)
            && self.composition.as_ref().is_none_or(|c| c.row != r)
            && self.bidi.orders().display().is_none();
        // The frame's lent words (`crate::frozen`), which a cell's marks are
        // read through.
        let lent_words = self.cpu_lent.held();
        let (found, kept) = match (keep, held) {
            (_, false) => (None, None),
            (true, true) => {
                let rows = self.rows;
                let (k, found, asked) = self.kept_row(term, (r, rows), (walked, logical), || {
                    let (parted, letters) =
                        crate::syllable::walk((term, lent_words.words()), walked, logical_of);
                    let words = match letters {
                        crate::word::Letters::Seen => {
                            crate::word::of_row(term, walked, logical_of, &parted, |_| None)
                        }
                        crate::word::Letters::None => Vec::new(),
                    };
                    crate::kept::Found { parted, words }
                });
                if asked == crate::kept::Asked::Found {
                    self.syllable_rows.bump();
                }
                (Some(found), Some(k))
            }
            (false, true) => {
                self.syllable_rows.bump();
                let (parted, letters) =
                    crate::syllable::walk((term, lent_words.words()), walked, logical_of);
                // A composition's cells read as what they will hold, and the
                // letters beside it that its words take in, which it draws,
                // drawn unlit here as a word's own cells are
                // ([`crate::overlay::Taken`]).
                let taken = &self.taken;
                let words = match size.is_double() || letters == crate::word::Letters::None {
                    true => Vec::new(),
                    false => crate::word::of_row(term, walked, logical_of, &parted, |x| {
                        taken.held(r, Col::at_index(x))
                    }),
                };
                (
                    Some(std::rc::Rc::new(crate::kept::Found { parted, words })),
                    None,
                )
            }
        };
        let (syllables, words): (&[crate::syllable::Syllable], &[crate::word::Word]) = match &found
        {
            Some(f) => (&f.parted, &f.words),
            None => (&[], &[]),
        };
        // The cells beside a composition its words take in, read where the
        // composition keeps them rather than gathered.
        let on = self.taken.on(r);
        let beside = on && self.taken.beside(r).next().is_some();
        let mut lights: Vec<crate::word::Light> = Vec::new();
        if self.row_worded[r.index()] == crate::word::Worded::Yes {
            for x in range.clone() {
                self.worded[x] = crate::word::Worded::No;
            }
        }
        self.row_worded[r.index()] = match words.is_empty() && !beside {
            true => crate::word::Worded::No,
            false => crate::word::Worded::Yes,
        };
        if !words.is_empty() || beside {
            lights = vec![crate::word::Light::NONE; row.len()];
            for w in words {
                for x in w.at.index()..w.end().index() {
                    self.worded[range.start + x] = crate::word::Worded::Yes;
                }
            }
            for x in self.taken.beside(r).filter(|&x| on && x < row.len()) {
                self.worded[range.start + x] = crate::word::Worded::Yes;
            }
        }
        // The bar or underline cursor, or the hollow box: on the one cell
        // the cursor is on, four shapes at most, held here.
        let mut cursor_shapes: [Option<Instance>; 4] = [None; 4];
        // A sized line's ring parts, on its geometry already (`edges`).
        let rings = &mut scratch.rings;
        // Colours first, cell by cell, with the shapes a rendition asks
        // for: an underline, a strike, an underline or bar cursor. A block
        // cursor is the cell's colours swapped. Shapes go on after the
        // glyphs, so a strike crosses its letter rather than hiding under
        // an opaque colour glyph.
        let shapes = &mut scratch.shapes;
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0);
        let top = i32::try_from(self.band(r).top.value()).unwrap_or(0);
        // The cursor's colours, read once a frame (`refresh_screen`) and used
        // where the cursor draws: the block fills with the cursor colour,
        // rather than the swap, and takes the text colour under it; the bar
        // or underline takes the colour. As this frame's video shows it,
        // which under a flash is the colour a flip would leave.
        let block_paint = self.block_paint;
        let mut blinks = Blinking::UNSEEN;
        for (c, cell) in row.iter().enumerate().take(chars) {
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
            let selected = if highlight.is_some_and(|h| selects(&h, col, cell.content)) {
                Selected::Yes
            } else {
                Selected::No
            };
            // The cursor is kept by its logical cell and drawn where that
            // cell is: `here` holds the logical column, for `cursor_px`.
            let here = cursor_visual
                .filter(|&(cr, _, cv, _)| cr == r && cv == col)
                .map(|(cr, cc, _, s)| (cr, cc, s));
            let block = match here {
                Some((_, _, style)) if self.swaps(style) => BlockCursor::Yes(block_paint),
                _ => BlockCursor::No,
            };
            let opacity = here.map_or(Chan::from_u8(0xFF), |(_, _, s)| self.cursor_opacity(s));
            // A word's cell draws unlit: what lights it is kept for its
            // syllable's span.
            let (selected, block) = match lights.get_mut(c) {
                Some(l) => {
                    *l = crate::word::Light {
                        selected,
                        block,
                        cursor: opacity,
                    };
                    match self.worded[range.start + c] {
                        crate::word::Worded::Yes => (Selected::No, BlockCursor::No),
                        crate::word::Worded::No => (selected, block),
                    }
                }
                None => (selected, block),
            };
            let d = res.drawn(Swaps {
                pictured: self.pictures.at(r, col),
                line: Line::of(cell.content),
                selected,
                hilite: self.hilite,
                block,
                blink: self.blink,
                cursor: opacity,
            });
            let (fg, hidden) = (d.fg, !d.text.draws());
            self.fg[range.start + c] = fg;
            self.curve[range.start + c] = d.curve;
            self.text[range.start + c] = d.text;
            // The background is the screen's, a column at a time: a sized
            // line's character covers two.
            let wide = usize::try_from(st.x()).unwrap_or(1);
            for x in c * wide..(c * wide + wide).min(row.len()) {
                self.bg[range.start + x] = d.bg;
            }
            let x = i32::try_from(c).unwrap_or(0) * cell_w;
            // The underline's own colour, or the foreground after the swaps.
            let ul_color = if res.shapes.has(Shapes::UNDERLINE_FOLLOWS_FG) {
                fg
            } else {
                d.rule(res.underline)
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
            // The overline, the frame, an encircled run and the stress mark,
            // out of line behind one test: most cells carry none of them.
            if !hidden && res.shapes.any(Shapes::EDGES) {
                self.edges(term, r, col, row, res, d, shapes, rings);
            }
            if let Some((_, lc, style)) = here.filter(|&(_, _, s)| !self.swaps(s)) {
                // The bar or underline cursor, or the hollow box, draws in
                // the cursor colour if one is set, else the cell's
                // foreground, at its opacity.
                let cur = self
                    .shape_colour(d.bg, d.cursor_fg)
                    .scaled(self.cursor_opacity(style));
                match self.cursor_look {
                    CursorLook::Shaped => {
                        // Already on the line's geometry (`Renderer::cursor_px`).
                        let (left, y, w, h) = self.cursor_px(r, lc, style);
                        cursor_shapes[0] =
                            Some(solid(screen(px_i32(left)), screen(px_i32(y)), w, h, cur));
                    }
                    CursorLook::Hollow => cursor_shapes = self.hollow(r, lc, cur).map(Some),
                }
            }
        }
        if size.is_double() {
            let band = self.band(r);
            // Each shape moved onto the line's geometry where it stands, and
            // one that falls outside the line's band left out.
            shapes.retain_mut(|s| match stretched(*s, size, band) {
                Some(moved) => {
                    *s = moved;
                    true
                }
                None => false,
            });
        }
        shapes.extend_from_slice(rings);
        shapes.extend(cursor_shapes.into_iter().flatten());
        self.row_blinks[r.index()] = blinks;
        // A composition's pages on this row, under every glyph of it.
        if let Some(s) = self.splice.as_ref().filter(|s| s.row == r) {
            instances.extend_from_slice(&s.pages);
        }
        // Each word's syllables in their light, and the row's cells as they
        // draw, the words' unlit.
        let (paints, under): (Vec<crate::word::WordPaint>, Vec<crate::word::Under>) =
            match words.is_empty() {
                true => (Vec::new(), Vec::new()),
                false => {
                    let under = (0..row.len())
                        .map(|x| crate::word::Under {
                            colours: crate::word::Colours {
                                fg: self.fg[range.start + x],
                                bg: self.bg[range.start + x],
                                curve: self.curve[range.start + x],
                            },
                            light: lights[x],
                        })
                        .collect();
                    let paints = words
                        .iter()
                        .map(|w| crate::word::WordPaint {
                            syllables: w
                                .syllables
                                .iter()
                                .map(|s| {
                                    let light = (s.at.index()..s.at.index() + s.cells.index())
                                        .fold(crate::word::Light::NONE, |l, x| l.with(lights[x]));
                                    let d = self.paint_of(r, s.at, row[s.at.index()], light);
                                    (
                                        crate::word::Colours {
                                            fg: d.fg,
                                            bg: d.bg,
                                            curve: d.curve,
                                        },
                                        light,
                                        d.text,
                                    )
                                })
                                .collect(),
                        })
                        .collect();
                    (paints, under)
                }
            };
        self.laid[r.index()].clear();
        let worded = (words, &paints[..], &under[..], syllables, kept);
        self.row_runs(
            term,
            r,
            (row, seams),
            (st, line_end),
            worded,
            (&mut instances, &mut scratch.run),
            exc,
        );
        self.glyph_ends[r.index()] = instances.len();
        self.reach[r.index()] = Reach::of(&instances);
        instances.extend_from_slice(shapes);
        self.per_row.set(r, instances);
        self.row_scratch = Some(scratch);
    }

    /// **One row's runs of cells shaped by one face**, for
    /// [`Renderer::convert_row`]: a row that holds no joined or mirrored
    /// cell, no syllable and no word by the loop that reads none of them,
    /// as it ran before joining existed, and one that holds one by
    /// [`Renderer::joined_runs`].
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn row_runs(
        &mut self,
        term: &Term<'_>,
        r: Row,
        (row, seams): (&[PackedCell], &[bool]),
        at: (Stretch, usize),
        words: Worded<'_>,
        out: (&mut Vec<Instance>, &mut ClusterList),
        exc: &mut Exceptions,
    ) {
        // The test alone here, nothing kept from it: the loop every other row
        // runs is inlined below, and holds no copy of a row's joins live.
        let orders = self.bidi.orders();
        match orders.joins(r).is_none()
            && orders.mirrors(r).is_empty()
            && orders.slants(r).is_empty()
            && words.0.is_empty()
            && words.3.is_empty()
        {
            true => self.runs::<false>(term, r, (row, seams, &[], &[], &[]), at, words, out, exc),
            false => self.joined_runs(term, r, (row, seams), at, words, out, exc),
        }
    }

    /// [`Renderer::row_runs`] for a row that holds a joined or a mirrored
    /// cell, a syllable or a word: each joined cell shaped alone in its
    /// form, a lam-alef pair drawn together, a mirrored cell drawn as L4 has
    /// it ([`crate::bidi::Mirrored`]), and each word and syllable drawn whole
    /// (`crate::word`, `crate::syllable`). Out of line, so the loop every
    /// other row runs holds none of it. The row's joined and mirrored cells
    /// are copied, so shaping may borrow the renderer mutably.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn joined_runs(
        &mut self,
        term: &Term<'_>,
        r: Row,
        (row, seams): (&[PackedCell], &[bool]),
        at: (Stretch, usize),
        words: Worded<'_>,
        out: (&mut Vec<Instance>, &mut ClusterList),
        exc: &mut Exceptions,
    ) {
        let orders = self.bidi.orders();
        let joins = orders.joins(r).map(<[_]>::to_vec).unwrap_or_default();
        let mirrors = orders.mirrors(r).to_vec();
        let slants = orders.slants(r).to_vec();
        let cells = (row, seams, &joins[..], &mirrors[..], &slants[..]);
        self.runs::<true>(term, r, cells, at, words, out, exc);
    }

    /// The runs of one row, `LAID` for a row that holds a joined or a
    /// mirrored cell, a syllable or a word.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn runs<const LAID: bool>(
        &mut self,
        term: &Term<'_>,
        r: Row,
        (row, seams, joins, mirrors, slants): RowCells<'_>,
        (st, line_end): (Stretch, usize),
        (words, paints, grounds, syllables, kept): Worded<'_>,
        (instances, run): (&mut Vec<Instance>, &mut ClusterList),
        exc: &mut Exceptions,
    ) {
        // The words of the frame's lent slots (`crate::frozen`), held so the
        // runs may borrow the renderer mutably while they read them.
        let held = self.cpu_lent.held();
        let seam = |v: usize| seams.get(v).copied().unwrap_or(false);
        let joined = |v: usize| match LAID {
            true => joins.get(v).copied().flatten(),
            false => None,
        };
        // A mirrored cell is at an odd level, so a seam stands before it and
        // after it: it is a run of its own, and only a run's first cell is
        // asked.
        // By a binary search: the list is by visual column, left to right,
        // and a row of wide box lines under `?2500` holds one a cell.
        let mirrored = |v: usize| match LAID {
            true => mirrors
                .binary_search_by_key(&v, |&(c, _)| c.index())
                .ok()
                .and_then(|k| mirrors.get(k))
                .map(|&(_, m)| m),
            false => None,
        };
        let under = |v: usize| match mirrored(v) {
            Some(crate::bidi::Mirrored::Under(m)) => Some(m),
            _ => None,
        };
        // A cell SAPV 17 slants left is at an odd level too, a run of its
        // own.
        let slanted = |v: usize| LAID && slants.contains(&Col::at_index(v));
        // Then runs of cells shaped by one face, in one selection of its
        // lookups. The row's scripts are settled the first time a run is on
        // a face whose scripts select different lookups, and not at all on a
        // row drawn only by faces whose scripts all select alike -- which is
        // every row the embedded faces draw alone.
        let mut settled = false;
        let mut c = 0usize;
        // The row's mirrored cells walked with `c`, which only grows.
        let mut next = 0;
        // **The row's syllables** (`crate::syllable`), each drawn whole
        // from its first cell and kept out of every run.
        let mut syl = crate::syllable::Walk::new(syllables);
        // The row's words, walked with `c` as its syllables are.
        let mut wi = 0usize;
        let word_starts = |c: usize| {
            !words.is_empty() && words.binary_search_by_key(&c, |w| w.at.index()).is_ok()
        };
        while c < line_end {
            // A composition's glyphs at this cell, in the row's order.
            if LAID {
                self.spliced_at(r, Col::at_index(c), instances);
            }
            let m = match LAID {
                true => {
                    while mirrors.get(next).is_some_and(|&(v, _)| v.index() < c) {
                        next += 1;
                    }
                    mirrors
                        .get(next)
                        .filter(|&&(v, _)| v.index() == c)
                        .map(|&(_, m)| m)
                }
                false => None,
            };
            let mirror = match m {
                Some(crate::bidi::Mirrored::Under(m)) => Some(m),
                _ => None,
            };
            let Some(base) = mirror.or_else(|| drawn_base(term, row[c])) else {
                c += 1;
                continue;
            };
            // A word, then a syllable no word holds, each drawn whole: on
            // the laid loop alone, which every row holding one takes.
            if LAID {
                while words.get(wi).is_some_and(|w| w.at.index() < c) {
                    wi += 1;
                }
                if let Some(w) = words.get(wi).filter(|w| w.at.index() == c) {
                    // The word before it on the row, laid already: its ink's
                    // right edge, which a residue closes on.
                    let prev = self.taken.ink_before(
                        r,
                        w.at,
                        self.laid[r.index()].last().map(|l| (l.at, l.ink.1)),
                    );
                    let styles = crate::word::faces_of(&self.styles, row, w);
                    let laid = self.word_at(
                        (term, &styles),
                        (r, row, kept.map(|k| (k, crate::kept::WordIx(wi)))),
                        (w, &paints[wi], prev),
                        grounds,
                        instances,
                    );
                    if let Some(l) = laid {
                        self.laid[r.index()].push(l);
                    }
                    c = w.end().index().max(c + 1);
                    continue;
                }
                if let Some(s) = syl.at(Col::at_index(c)) {
                    self.syllable_at(term, r, row, s, instances);
                    c += s.cells.index();
                    continue;
                }
            }
            if crate::boxes::covers(base) {
                // Box drawing, a block element or braille: baud's own mask
                // at the cell origin, across both cells of a wide one, never
                // shaped, never in a run -- mirrored whole where L4 draws it
                // flipped (`?2500` on a box line two cells wide).
                let span = CellSpan::at(row, Col::at_index(c));
                let facing = match m {
                    Some(crate::bidi::Mirrored::Flipped) => crate::boxes::Facing::Mirrored,
                    _ => crate::boxes::Facing::Written,
                };
                if let Some(slot) = self.procedural_at(base, (span, facing), st) {
                    self.emit_at(r, Col::at_index(c), slot, instances);
                }
                c = span.after(Col::at_index(c)).index();
                continue;
            }
            // **A lam-alef pair**, drawn together over its cells from its
            // left one, out of line: a row with no joined cell asks once.
            if LAID && self.pair_at(term, r, (row, joins), (c, line_end), instances) {
                c += 1;
                continue;
            }
            let style = self.face_style(row[c]);
            let lean = match slanted(c) {
                true => self.lean(style),
                false => None,
            };
            let face = match lean {
                Some(_) => {
                    self.face_of_cell_in((term, held.words()), row[c], base, style.upright())
                }
                None => self.face_of_cell_in((term, held.words()), row[c], base, style),
            };
            let varies = self.varies(face);
            if !settled && varies == Varies::Yes {
                self.settle_row(term, row);
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
            // The run's clusters, gathered into the row's buffers.
            run.clear();
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
            let layout = self.layouts[face.index()];
            #[cfg(feature = "exceptions")]
            let prefilter = self.starters[layout];
            #[cfg(feature = "exceptions")]
            let mut starters_here = Tally::default();
            #[cfg(feature = "exceptions")]
            let mut prev_glyph: Option<GlyphId> = None;
            #[cfg(feature = "exceptions")]
            let mut pair_here = Tally::default();
            while c < line_end && c - start < RUN_CELLS {
                let cell = row[c];
                let Some(cp) = under(c).or_else(|| drawn_base(term, cell)) else {
                    break;
                };
                // The run's first cell is taken whatever the test says: the
                // face above was chosen for it, and a run that could stop
                // before its first cell would never advance -- a hang, not
                // a wrong pixel, if the two ever disagreed. Both ask
                // `face_of_cell`, so a marked cell is asked with its whole
                // cluster both times.
                // **A joined cell is shaped alone**, in its form: a form
                // is the layout's, from the logical text, and a run in the
                // drawn order would show a face's contextual lookups the
                // letters reversed.
                if c > start
                    && (seam(c)
                        || (LAID
                            && (joined(c).is_some()
                                || joined(start).is_some()
                                || syl.at(Col::at_index(c)).is_some()
                                || word_starts(c)))
                        || crate::boxes::covers(cp)
                        || self.face_of_cell((term, held.words()), cell, cp) != face
                        || (varies == Varies::Yes
                            && self.script_at(face, Col::at_index(c)).1 != selection))
                {
                    break;
                }
                let next = CellSpan::at(row, Col::at_index(c))
                    .after(Col::at_index(c))
                    .index();
                #[cfg(feature = "exceptions")]
                {
                    let glyph = self.stack.face(face).glyph(cp);
                    if let (Some(p), Some(g)) = (prev_glyph, glyph) {
                        if self.pairs[layout].admits(p, g) {
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
                // The cell's base and marks, onto the stack -- a base and
                // the most the cluster store holds (`ClusterSlot::CAPACITY`)
                // -- then into the run's buffers.
                let mut cps = [cp; 1 + ClusterSlot::CAPACITY];
                let mut held_cps = 1;
                key = key.fold(cp.value());
                if cell.flags.is_marked() {
                    exc.marked.bump();
                    let marks = drawn_marks(term, held.words(), cell).filter_map(Codepoint::new);
                    for (slot, mc) in cps[1..].iter_mut().zip(marks) {
                        *slot = mc;
                        held_cps += 1;
                        key = key.fold(mc.value());
                    }
                }
                key = key.fold(u32::try_from(next - c).unwrap_or(0) << 24);
                run.push(
                    &cps[..held_cps],
                    CellIx(u8::try_from(c - start).unwrap_or(u8::MAX)),
                );
                c = next;
            }
            // A joined cell is a run of its own (above), shaped in its form
            // with the cells of its word around it on this face, all of
            // which its key folds.
            let here = match LAID {
                true => joined(start),
                false => None,
            };
            let form = [here.map(|j| j.form)];
            let mut word = Word::default();
            if let Some(j) = here {
                key = fold_form(key, FormKey::Cell, j.form);
                let (before, after) = self.bidi.orders().around(r, &j);
                let (before, after) = (before.to_vec(), after.to_vec());
                let styles = core::mem::take(&mut self.styles);
                word = self.word_on(
                    (term, &styles, held.words()),
                    face,
                    (&before, &after),
                    (r, Col::at_index(start)),
                );
                self.styles = styles;
                key = word.fold(key);
            }
            let form: &[Option<baud_domain::JoiningForm>] = match form[0] {
                Some(_) => &form,
                None => &[],
            };
            let shaped = match self.cache.get(key) {
                Some(s) => s,
                None => {
                    let s = self.shape_in_word(face, script, run.clusters(), form, &word);
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
            self.count_differs(term, r, Col::at_index(start), run.clusters(), &shaped, exc);
            // A mirrored cell's run is drawn by the loop that reads its
            // mirror; every other run by the loop that reads none.
            let at = Col::at_index(start);
            match (mirrored(start), lean) {
                (None, None) => self.emit::<false>(term, face, r, at, (&shaped, None), instances),
                (mirrored, lean) => {
                    let drawn = CellDrawn {
                        mirrored,
                        base,
                        lean,
                    };
                    self.emit::<true>(term, face, r, at, (&shaped, Some(drawn)), instances);
                }
            }
        }
    }

    /// The glyphs a composition draws at visual cell `c` of row `r`, if it
    /// is spliced into that row ([`Splice`]), into `out`.
    fn spliced_at(&self, r: Row, c: Col, out: &mut Vec<Instance>) {
        if let Some(s) = self.splice.as_ref().filter(|s| s.row == r) {
            for (_, g) in s.glyphs.iter().filter(|&&(x, _)| x == c.index()) {
                out.extend_from_slice(g);
            }
        }
    }

    /// **The cells of a joined cell's word its shaping reads on `face`**:
    /// of the layout's word around it ([`crate::bidi::Orders::around`]),
    /// the nearest [`baud_font::joined_reach`] cells either side, each side
    /// ending at the first cell another face draws, whose glyphs no lookup
    /// of this face reads, or that draws nothing. The joined cell is at
    /// visual `at`, and its word in its level run: at a right-to-left
    /// level, a cell SAPV 17 slants is drawn by its upright style's face,
    /// as the cell itself is. Each cell's style is read from `styles`, the
    /// table the frame draws with ([`Renderer::word_at`]).
    pub(crate) fn word_on(
        &mut self,
        (term, styles, frame): (&Term<'_>, &[Resolved], Words<'_>),
        face: FaceIx,
        (before, after): WordAround<'_>,
        (r, v): (Row, Col),
    ) -> Word {
        let reach = usize::try_from(
            self.layouts
                .get(face.index())
                .and_then(|&l| self.reaches.get(l))
                .map_or(0, |c| c.value()),
        )
        .unwrap_or(0);
        let mut word = Word::default();
        if reach == 0 {
            return word;
        }
        let leans = self.bidi.orders().right_to_left(r, v) && term.variants_seen();
        let mut take = |w: &crate::bidi::WordCell| {
            let base = drawn_base(term, w.cell)?;
            // **A cell of the word its own row names**: one the core froze
            // keeps its word, and a live one whose slot the frame lent a
            // frozen row reads the live table (`crate::frozen`); either is
            // resolved from its word rather than read from `styles`.
            let lent = !frame.is_live()
                && frame.word(term, Col::ZERO, w.cell)
                    != crate::frozen::live(term).word(w.cell.style);
            let style = match (w.word, lent) {
                (Some(word), _) => self.resolve_slot(term, word).shapes.text_face(),
                (None, true) => self
                    .resolve_slot(term, crate::frozen::live(term).word(w.cell.style))
                    .shapes
                    .text_face(),
                (None, false) => crate::cellmap::face_of(styles, w.cell),
            };
            let style = match leans && crate::bidi::slanted(term, w.words(), Col::ZERO, w.cell) {
                true => self.lean(style).map_or(style, |_| style.upright()),
                false => style,
            };
            (self.face_of_cell_in((term, w.words()), w.cell, base, style) == face)
                .then(|| (cell_cps(term, w.words(), w.cell), w.form))
        };
        for w in before.iter().rev().take(reach) {
            match take(w) {
                Some(c) => word.before.push(c),
                None => break,
            }
        }
        word.before.reverse();
        for w in after.iter().take(reach) {
            match take(w) {
                Some(c) => word.after.push(c),
                None => break,
            }
        }
        word
    }

    /// **`own` -- a joined cell, or a lam-alef pair's two -- shaped on
    /// `face` in `forms` with `word` around it**, as one run in logical
    /// order, so a face's contextual lookups across letters (`rlig`,
    /// `rclt`, `calt`) meet the letters they read: the glyphs that start on
    /// `own`'s cells, each at the cell index `own` gives it. With no word,
    /// or where the word took `own`'s letters into a glyph across cells
    /// ([`baud_font::keeps_its_cells`]), [`Renderer::shape`] of `own`
    /// alone.
    fn shape_in_word(
        &mut self,
        face: FaceIx,
        script: Script,
        own: Clusters<'_>,
        forms: &[Option<baud_domain::JoiningForm>],
        word: &Word,
    ) -> Shaped {
        if word.is_empty() {
            return self.shape(face, script, own, forms, ..);
        }
        let (b, owned) = (word.before.len(), own.iter().count());
        let mut clusters = ClusterList::default();
        let mut all_forms = Vec::with_capacity(b + owned + word.after.len());
        let mut pushed = 0usize;
        let mut push = |cps: &[Codepoint], form: Option<baud_domain::JoiningForm>| {
            let at = CellIx(u8::try_from(pushed).unwrap_or(u8::MAX));
            pushed += 1;
            clusters.push(cps, at);
            all_forms.push(form);
        };
        for (cps, form) in &word.before {
            push(cps, *form);
        }
        for (i, (cps, _)) in own.iter().enumerate() {
            push(cps, forms.get(i).copied().flatten());
        }
        for (cps, form) in &word.after {
            push(cps, *form);
        }
        let first = CellIx(u8::try_from(b).unwrap_or(u8::MAX));
        let end = CellIx(u8::try_from(b + owned).unwrap_or(u8::MAX));
        let shaped = self.shape(face, script, clusters.clusters(), &all_forms, first..end);
        let spans = shaped
            .glyphs()
            .iter()
            .map(|p| (Cluster(u16::from(p.cell.0)), Cluster(u16::from(p.last.0))));
        let own_cells = Cluster(u16::from(first.0))..Cluster(u16::from(end.0));
        if !baud_font::keeps_its_cells(own_cells, spans) {
            return self.shape(face, script, own, forms, ..);
        }
        let last = end.0.saturating_sub(first.0).saturating_sub(1);
        let back = |c: CellIx| CellIx(c.0.saturating_sub(first.0).min(last));
        let mut out = Shaped::EMPTY;
        out.set_joins(shaped.joins());
        for p in shaped.glyphs() {
            let moved = Placed {
                cell: back(p.cell),
                last: back(p.last),
                ..*p
            };
            if out.push(moved).is_none() {
                break;
            }
        }
        out
    }

    /// `clusters` shaped as one run on `face`, each in the joining form
    /// `forms` gives it at its index, if any; what a face that keeps a
    /// cluster cannot draw is left out and counted for the clusters `own`
    /// holds -- all of them, except in a ligature window's chunk and a
    /// joined cell's word, whose context cells are another's to count --
    /// and only the glyphs that start on those clusters kept.
    fn shape(
        &mut self,
        face: FaceIx,
        script: Script,
        clusters: Clusters<'_>,
        forms: &[Option<baud_domain::JoiningForm>],
        own: impl core::ops::RangeBounds<CellIx>,
    ) -> Shaped {
        self.shape_laid(
            face,
            script,
            clusters,
            forms,
            own,
            (baud_font::Pens::Cell, baud_font::ClusterLevel::Monotone),
        )
    }

    /// [`Renderer::shape`] with the clusters laid as `pens` says: from each
    /// cell's origin, or end to end as a word is set (`crate::word`); and
    /// each glyph keeping its cell where `level` says.
    fn shape_laid(
        &mut self,
        face: FaceIx,
        script: Script,
        clusters: Clusters<'_>,
        forms: &[Option<baud_domain::JoiningForm>],
        own: impl core::ops::RangeBounds<CellIx>,
        (pens, level): (baud_font::Pens, baud_font::ClusterLevel),
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
            shaper.accelerate(&self.digests[self.layouts[face.index()]]);
            shaper.scratch(&mut self.ink_scratch);
            if pens == baud_font::Pens::Run {
                shaper.lay_as_run();
            }
            if level == baud_font::ClusterLevel::Characters {
                shaper.keep_cells();
            }
            let mut unmapped = 0usize;
            for (i, (cps, cell)) in clusters.iter().enumerate() {
                let at = Cluster(u16::from(cell.0));
                let form = forms.get(i).copied().flatten();
                // **A mark the face cannot draw is left out, and counted.** A
                // cluster reaches a face that cannot draw it whole only when no
                // face can -- `face_for` would have chosen that one -- and the
                // `.notdef` it would map such a mark to is a base with a full
                // advance in JetBrains Mono: the box drew a cell to the right
                // and every mark after it over the box. What is left is the
                // letter and the marks the face has, which the shaper takes as
                // it takes any cluster; a face that draws the cluster whole is
                // handed it untouched, and which of the two a cluster is on a
                // face is remembered, as its face is, so a run the cache misses
                // pays a probe per marked cell rather than a normalisation. The
                // dropped set follows from the face and the codepoints alone,
                // which the run cache's key folds, so a cached run never needs
                // it recomputed.
                //
                // `without_unmapped` starts from the cluster's canonical form,
                // so a base and a mark whose canonical composite no face draws
                // are that composite's one `.notdef`, nothing left out: A +
                // U+0311 is Ȃ's box. Only a mark with no composite is left out
                // and counted.
                let push = |shaper: &mut Shaper<'_, '_, '_>, cps: &[Codepoint]| match form {
                    Some(f) => shaper.push_joined(cps, at, f),
                    None => shaper.push_cluster(cps, at),
                };
                let pushed = match cps {
                    [_, _, ..] => {
                        let cover = self
                            .cluster_covers
                            .answer(face, cps, || match Normalised::of(f, cps).covered() {
                                true => Cover::Whole,
                                false => Cover::Partial,
                            });
                        if cover == Cover::Whole {
                            push(&mut shaper, cps)
                        } else {
                            let kept = Normalised::without_unmapped(f, cps);
                            let pushed = push(&mut shaper, kept.as_slice());
                            if pushed.is_some() && own.contains(&cell) {
                                unmapped = unmapped.saturating_add(
                                    usize::try_from(kept.dropped().value()).unwrap_or(0),
                                );
                            }
                            pushed
                        }
                    }
                    cps => push(&mut shaper, cps),
                };
                if pushed.is_none() {
                    break;
                }
            }
            // **A run that holds a letter that takes a joining form** is
            // shaped with the joining features, and marks the renderer as
            // having drawn one; any other with the grid's, as before joining
            // existed. The shaper tested each base as it was pushed.
            let joins = match shaper.joins() {
                true => crate::bidi::Joins::Seen,
                false => crate::bidi::Joins::Unseen,
            };
            let features = match joins {
                crate::bidi::Joins::Seen => {
                    self.joins_seen = crate::bidi::Joins::Seen;
                    Features::JOINING
                }
                crate::bidi::Joins::Unseen => Features::GRID,
            };
            let (buf, shaped) = shaper.finish(features, script);
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
            out.set_joins(joins);
            for g in buf.as_slice() {
                let at = CellIx(u8::try_from(g.cluster.0).unwrap_or(u8::MAX));
                if g.ignorable == Ignorable::Yes || !own.contains(&at) {
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
        clusters: Clusters<'_>,
        shaped: &Shaped,
        exc: &mut Exceptions,
    ) {
        let range = self.row_cells(r);
        for (_, at) in clusters.iter() {
            let col = start.index() + usize::from(at.0);
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
                    let one = Clusters::One(core::slice::from_ref(&cp));
                    let s = self
                        .shape(face, Script::of(cp), one, &[], ..)
                        .glyphs()
                        .first()
                        .map(|p| (face, *p));
                    self.singles.insert(memo, s);
                    s
                }
            };
            let face = self.face_for(core::slice::from_ref(&cp), style);
            // The cluster's glyphs, read in place: its first, and whether
            // it has a second.
            let mut mine = shaped.glyphs().iter().filter(|p| p.cell == at);
            let (one, more) = (mine.next(), mine.next().is_some());
            let same = match (alone, one, more) {
                (None, None, _) => true,
                (Some((af, a)), Some(p), false) => {
                    af == face && a.id == p.id && a.dx == p.dx && a.dy == p.dy
                }
                _ => false,
            };
            if !same {
                exc.differs.bump();
            }
        }
    }

    /// The decorations drawn at a cell's edges and over it -- the overline
    /// and its double, the frame, an end of an encircled run and the
    /// stress mark -- for cell `col` of row `r`, whose text draws, into
    /// `shapes`, after the glyphs. `row` is the row's cells, whose
    /// neighbours decide where a run's outline starts and ends; `term`
    /// holds a cluster's base and marks, which decide its stress mark.
    /// On a line not single-width a ring's part goes into `rings`, already
    /// on the line's geometry, since the stretch that moves `shapes` there
    /// moves a mask without scaling it.
    #[allow(clippy::too_many_arguments)]
    fn edges(
        &mut self,
        term: &Term<'_>,
        r: Row,
        col: Col,
        row: &[PackedCell],
        res: Resolved,
        d: crate::cellmap::Drawn,
        shapes: &mut Vec<Instance>,
        rings: &mut Vec<Instance>,
    ) {
        let c = col.index();
        let fg = d.fg;
        let cell = row.get(c).copied().unwrap_or(PackedCell::EMPTY);
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0);
        let top = i32::try_from(self.band(r).top.value()).unwrap_or(0);
        let x = i32::try_from(c).unwrap_or(0) * cell_w;
        // The overline and the frame, in the text's colour as the strike
        // is: `SGR 58` is "the underline color" (`[[source:
        // kitty-underlines]]`) and neither is an underline. The frame's
        // top is the overline's line, drawn once when both are set.
        let framed = res.shapes.has(Shapes::FRAMED);
        let (styles, blink) = (&self.styles, self.blink);
        let carries = |at: Option<usize>, shape: Shapes| {
            carries_on(styles, row, at.map(Col::at_index), shape, blink)
        };
        // `SGR 52`: which end of its run an encircled cell is at, and
        // whether it is between the ends, where it draws the frame's
        // top and bottom lines.
        let encircled = res.shapes.has(Shapes::ENCIRCLED);
        let ring = ring_part(styles, row, col, blink);
        let between = encircled && ring.is_none();
        if framed || between || res.shapes.has(Shapes::OVERLINE) {
            shapes.push(solid(
                screen(x),
                screen(top),
                self.metrics.cell_w,
                self.metrics.thickness,
                fg,
            ));
        }
        if res.shapes.has(Shapes::DOUBLE_OVERLINE) {
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
        if framed || between {
            let th = px_i32(self.metrics.thickness).max(1);
            let h = px_i32(self.metrics.cell_h);
            shapes.push(solid(
                screen(x),
                screen(top + h - th),
                self.metrics.cell_w,
                self.metrics.thickness,
                fg,
            ));
        }
        if framed {
            let th = px_i32(self.metrics.thickness).max(1);
            // A side only where the run of framed cells starts or ends:
            // the row's edge, or a neighbour that is not framed or whose
            // text is hidden (`carries`).
            let framed_at = |i: Option<usize>| carries(i, Shapes::FRAMED);
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
        // An end of an encircled run: its part of the outline, a mask
        // at the cell origin as a procedural glyph is, in the text's
        // colour and bent by its curve as the glyph is.
        if let Some(ring) = ring {
            let size = self.size_of(r);
            if size.is_double() {
                self.sized_ring(r, col, ring, size, d, rings);
            } else if let Some(slot) = self.ring_slot(ring) {
                shapes.push(Instance {
                    x: screen(x + slot.left),
                    y: screen(top + px_i32(self.metrics.baseline) + slot.top),
                    w: slot.w,
                    h: slot.h,
                    u: slot.x,
                    v: slot.y,
                    color: fg,
                    kind: slot.kind,
                    curve: d.curve,
                });
            }
        }
        // `SGR 64`'s stress mark, as the web draws an emphasis mark for
        // text with no language (`[[source: css-text-decor-3]]`): over
        // the character, a filled dot, in the text's colour, once a
        // character, so across a wide character's two cells, judged by
        // its base and marks. Inside the cell, a square two thicknesses
        // on a side, as all three paths draw a decoration.
        if res.shapes.has(Shapes::STRESS)
            && takes_stress_mark(term.base_of(cell), term.cluster_marks(cell))
        {
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
    }

    /// **A ring's part on a line not single-width**, at the box its
    /// character takes there (`[[source: dec-std-070]]` § 5.6.1): the mask
    /// built at that box ([`Atlas::ring_at`]) placed at the box's origin,
    /// and on a double-height line cut to the row's half, as a glyph is.
    fn sized_ring(
        &mut self,
        r: Row,
        col: Col,
        ring: crate::boxes::Ring,
        size: LineRendition,
        d: crate::cellmap::Drawn,
        out: &mut Vec<Instance>,
    ) {
        let st = Stretch::of(size);
        let cell = crate::boxes::Cell {
            w: self.metrics.cell_w,
            h: self.metrics.cell_h,
        };
        let th = Px::of(self.metrics.thickness.value().max(1));
        let Some(slot) = self
            .atlas
            .ring_at(ring, cell, th, self.metrics.baseline, st)
        else {
            return;
        };
        let band = self.band(r);
        let x = i32::try_from(col.index()).unwrap_or(0) * px_i32(self.metrics.cell_w) * st.x();
        let y = box_top(size, band) + px_i32(self.metrics.baseline) * st.y();
        let inst = Instance {
            x: screen(x + slot.left),
            y: screen(y + slot.top),
            w: slot.w,
            h: slot.h,
            u: slot.x,
            v: slot.y,
            color: d.fg,
            kind: slot.kind,
            curve: d.curve,
        };
        out.extend(match st.y() {
            1 => Some(inst),
            _ => clip_to(inst, band),
        });
    }

    /// Whether a slot this renderer resolved has been encircled.
    pub(crate) fn rings_used(&self) -> bool {
        self.rings == Rings::Used
    }

    /// The atlas mask for one part of an encircled run's outline at this
    /// cell size, in the line thickness the frame's lines take.
    pub(crate) fn ring_slot(&mut self, ring: crate::boxes::Ring) -> Option<crate::atlas::Slot> {
        let cell = crate::boxes::Cell {
            w: self.metrics.cell_w,
            h: self.metrics.cell_h,
        };
        let th = Px::of(self.metrics.thickness.value().max(1));
        self.atlas.ring(ring, cell, th, self.metrics.baseline)
    }

    /// Row `r`'s line rendition in the frame being converted: what its
    /// glyphs are drawn at.
    fn size_of(&self, r: Row) -> LineRendition {
        self.cur
            .sizes()
            .get(r.index())
            .copied()
            .unwrap_or(LineRendition::Single)
    }

    /// The atlas mask for a procedural codepoint at this cell size, across
    /// the cells `span` gives.
    fn procedural(&mut self, cp: Codepoint, span: CellSpan) -> Option<crate::atlas::Slot> {
        self.procedural_at(cp, (span, crate::boxes::Facing::Written), Stretch::One)
    }

    /// The same for a line drawn at `stretch`, facing as `facing` says
    /// ([`Atlas::synthetic_across`]).
    fn procedural_at(
        &mut self,
        cp: Codepoint,
        at: (CellSpan, crate::boxes::Facing),
        stretch: Stretch,
    ) -> Option<crate::atlas::Slot> {
        let cell = crate::boxes::Cell {
            w: self.metrics.cell_w,
            h: self.metrics.cell_h,
        };
        self.atlas
            .synthetic_across(cp, cell, at, self.metrics.baseline, stretch)
    }

    /// One instance for a slot placed at the origin of cell `col` of row
    /// `r`, in the cell's foreground: the procedural glyphs' emitter.
    fn emit_at(&mut self, r: Row, col: Col, slot: crate::atlas::Slot, out: &mut Vec<Instance>) {
        let size = self.size_of(r);
        let st = Stretch::of(size);
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0) * st.x();
        let baseline = i32::try_from(self.metrics.baseline.value()).unwrap_or(0) * st.y();
        let top = box_top(size, self.band(r));
        let range = self.row_cells(r);
        // A hidden cell draws nothing of its own (`Resolved::text`).
        if self
            .text
            .get(range.start + col.index())
            .is_some_and(|t| !t.draws())
        {
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
        let inst = Instance {
            x: Screen(i16::try_from(x).unwrap_or(i16::MAX)),
            y: Screen(i16::try_from(y).unwrap_or(i16::MAX)),
            w: slot.w,
            h: slot.h,
            u: slot.x,
            v: slot.y,
            color,
            kind: slot.kind,
            curve,
        };
        // A double-height row draws its half of the character's box.
        let inst = match st.y() {
            1 => Some(inst),
            _ => clip_to(inst, self.band(r)),
        };
        out.extend(inst);
    }

    /// The lam-alef pair visual cell `c` of row `r` holds a letter of,
    /// drawn into `out` through [`Renderer::lam_alef`] from its left cell:
    /// whether the cell is drawn by it -- the left cell drawing both, the
    /// right one, drawn already, nothing. A pair a soft wrap parts draws its
    /// ligature in the lam's cell and nothing in the alef's, when
    /// [`Renderer::wrapped_ligates`] says the face forms it.
    #[inline(never)]
    fn pair_at(
        &mut self,
        term: &Term<'_>,
        r: Row,
        (row, joins): (&[PackedCell], &[Option<crate::bidi::Joined>]),
        (c, line_end): (usize, usize),
        out: &mut Vec<Instance>,
    ) -> bool {
        use crate::bidi::{Joined, PairEnd, PairPart};
        let joined = |v: usize| joins.get(v).copied().flatten();
        let Some(Joined {
            form,
            pair: Some((part, kind)),
            ..
        }) = joined(c)
        else {
            return false;
        };
        let st = Stretch::of(self.size_of(r));
        let (span, end) = match part {
            PairPart::Lam(n, e) | PairPart::Alef(n, e) => (n, e),
            PairPart::LamWrapped(alef) => {
                let lam = row[c];
                let style = self.face_style(lam);
                if !self.wrapped_ligates(term, (lam, alef), kind, style, st) {
                    return false;
                }
                let held = self.cpu_lent.held();
                let lam_cps = cell_cps(term, held.words(), lam);
                // The other letter is another row's, as the layout kept it.
                let alef_cps = cell_cps(term, Words::LIVE, alef);
                let lean = self.slant_at(r, Col::at_index(c), style);
                let style = lean.map_or(style, |_| style.upright());
                let face = self.face_for(&lam_cps, style);
                let styles = core::mem::take(&mut self.styles);
                let word = self.pair_word((term, &styles, held.words()), r, (c, None), face);
                self.styles = styles;
                let Some(mut g) = self.lam_alef(
                    (&lam_cps, &alef_cps),
                    form,
                    style,
                    (PairSpan::LamCell, &word),
                    st,
                ) else {
                    return false;
                };
                if let Some(l) = lean {
                    self.shear_glyphs(&mut g.lams, l, st, Flip::Kept);
                    self.shear_glyphs(&mut g.alefs, l, st, Flip::Kept);
                }
                self.emit_glyphs(r, Col::at_index(c), &g.lams, out);
                self.emit_glyphs(r, Col::at_index(c), &g.alefs, out);
                return true;
            }
            PairPart::AlefWrapped(lam) => {
                let style = self.face_style(lam);
                return self.wrapped_ligates(term, (lam, row[c]), kind, style, st);
            }
        };
        // The pair's other cell, at the span's other end, holding the other
        // letter at the other end of the same span.
        let far = span.index().saturating_sub(1);
        let o = match end {
            PairEnd::Left => c.checked_add(far),
            PairEnd::Right => c.checked_sub(far),
        };
        let Some(o) = o.filter(|&o| o != c && o < line_end) else {
            return false;
        };
        let (lam_c, alef_c) = match (part, joined(o).and_then(|j| j.pair)) {
            (PairPart::Lam(..), Some((PairPart::Alef(h, e), _))) if h == span && e != end => (c, o),
            (PairPart::Alef(..), Some((PairPart::Lam(h, e), _))) if h == span && e != end => (o, c),
            _ => return false,
        };
        // **A partner a composition holds** is converted blank, and drawn
        // as written there: this cell, the pair's other, draws both.
        let composed = row[o].content == baud_domain::Content::EMPTY;
        let (lam_cell, alef_cell) = (
            self.shown_cell(r, Col::at_index(lam_c), row),
            self.shown_cell(r, Col::at_index(alef_c), row),
        );
        let lam_form = joined(lam_c).map_or(form, |j| j.form);
        let held = self.cpu_lent.held();
        let lam_cps = cell_cps(term, held.words(), lam_cell);
        let alef_cps = cell_cps(term, held.words(), alef_cell);
        let style = self.face_style(lam_cell);
        let lean = self.slant_at(r, Col::at_index(lam_c), style);
        let style = lean.map_or(style, |_| style.upright());
        let lam_at = match lam_c > alef_c {
            true => LamAt::Right,
            false => LamAt::Left,
        };
        let face = self.face_for(&lam_cps, style);
        let styles = core::mem::take(&mut self.styles);
        let word = self.pair_word(
            (term, &styles, held.words()),
            r,
            (lam_c, Some(alef_c)),
            face,
        );
        self.styles = styles;
        let Some(mut g) = self.lam_alef(
            (&lam_cps, &alef_cps),
            lam_form,
            style,
            (PairSpan::Cells(lam_at, span), &word),
            st,
        ) else {
            return false;
        };
        if let Some(l) = lean {
            self.shear_glyphs(&mut g.lams, l, st, Flip::Kept);
            self.shear_glyphs(&mut g.alefs, l, st, Flip::Kept);
        }
        // The left cell draws both; the right one was drawn with it, unless
        // a composition holds the left.
        if end == PairEnd::Left || composed {
            self.emit_glyphs(r, Col::at_index(lam_c), &g.lams, out);
            self.emit_glyphs(r, Col::at_index(alef_c), &g.alefs, out);
        }
        true
    }

    /// **The cell visual column `v` of row `r` shows, as written**:
    /// `row`'s, or, where it is converted blank under a composition or an
    /// echo, what the layout read written there ([`crate::bidi::Written`]).
    fn shown_cell(&self, r: Row, v: Col, row: &[PackedCell]) -> PackedCell {
        let cell = row.get(v.index()).copied().unwrap_or(PackedCell::EMPTY);
        if cell.content != baud_domain::Content::EMPTY {
            return cell;
        }
        self.written_cell(r, v).unwrap_or(cell)
    }

    /// What the layout read written at visual column `v` of row `r`
    /// ([`crate::bidi::Written`]), if anything is written over the row.
    pub(crate) fn written_cell(&self, r: Row, v: Col) -> Option<PackedCell> {
        let l = self.bidi.orders().logical_col(r, v).index();
        self.beside
            .as_deref()
            .and_then(|b| b.written.cells(r))
            .and_then(|w| w.get(l))
            .copied()
    }

    /// How far visual column `v` of row `r`, a cell in `style`, leans left:
    /// SAPV 17's slant where the layout found it ([`crate::bidi::Orders::slants`]).
    fn slant_at(&self, r: Row, v: Col, style: TextFace) -> Option<crate::stack::Lean> {
        match self.bidi.orders().slanted(r, v) {
            true => self.lean(style),
            false => None,
        }
    }

    /// **The word around a lam-alef pair**, on `face`, the one its lam
    /// draws: the cells before the lam, as the lam's
    /// [`crate::bidi::Joined`] at visual column `lam` of row `r` keeps
    /// them, and those after the alef -- the alef's own, at `alef` on the
    /// same row, or, for a pair a soft wrap parts, the lam's after its
    /// alef.
    pub(crate) fn pair_word(
        &mut self,
        (term, styles, frame): (&Term<'_>, &[Resolved], Words<'_>),
        r: Row,
        (lam, alef): (usize, Option<usize>),
        face: FaceIx,
    ) -> Word {
        let orders = self.bidi.orders();
        let Some(l) = orders.joined(r, Col::at_index(lam)) else {
            return Word::default();
        };
        let (before, lam_after) = orders.around(r, &l);
        let after = match alef.and_then(|a| orders.joined(r, Col::at_index(a))) {
            Some(a) => orders.around(r, &a).1,
            None => lam_after.get(1..).unwrap_or(&[]),
        };
        let (before, after) = (before.to_vec(), after.to_vec());
        self.word_on(
            (term, styles, frame),
            face,
            (&before, &after),
            (r, Col::at_index(lam)),
        )
    }

    /// **Whether a lam-alef pair a soft wrap parts is drawn as the
    /// ligature**, in the lam's cell: the two cells' base letters shaped
    /// together, the lam in the form `kind` gives, on the lam's face in
    /// `style`. Both letters' rows ask with the same cells, so the alef's
    /// cell draws nothing exactly when the lam's draws the ligature, and
    /// each draws its own letter on a face that forms none.
    pub(crate) fn wrapped_ligates(
        &mut self,
        term: &Term<'_>,
        (lam, alef): (PackedCell, PackedCell),
        kind: baud_domain::LamAlef,
        style: TextFace,
        stretch: Stretch,
    ) -> bool {
        let (Some(l), Some(a)) = (drawn_base(term, lam), drawn_base(term, alef)) else {
            return false;
        };
        let form = match kind {
            baud_domain::LamAlef::Joined => baud_domain::JoiningForm::Medial,
            baud_domain::LamAlef::Nominal => baud_domain::JoiningForm::Initial,
        };
        self.lam_alef(
            (&[l], &[a]),
            form,
            style,
            (PairSpan::LamCell, &Word::default()),
            stretch,
        )
        .is_some_and(|g| g.ligated == Ligated::Yes)
    }

    /// Glyphs placed from cell `col`'s origin, drawn in that cell's
    /// colours, as [`Renderer::emit`] draws a run's: a lam-alef pair's,
    /// whose glyphs [`Renderer::lam_alef`] placed.
    fn emit_glyphs(&self, r: Row, col: Col, glyphs: &[ClusterGlyph], out: &mut Vec<Instance>) {
        let col = col.index();
        let size = self.size_of(r);
        let st = Stretch::of(size);
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0) * st.x();
        let baseline = i32::try_from(self.metrics.baseline.value()).unwrap_or(0) * st.y();
        let band = self.band(r);
        let top = box_top(size, band);
        let range = self.row_cells(r);
        if self.text.get(range.start + col).is_some_and(|t| !t.draws()) {
            return;
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
        for g in glyphs {
            let Some(slot) = g.slot else {
                continue;
            };
            let x = i32::try_from(col).unwrap_or(0) * cell_w + g.dx + slot.left;
            let y = top + baseline - g.dy + slot.top;
            let inst = Instance {
                x: Screen(i16::try_from(x).unwrap_or(i16::MAX)),
                y: Screen(i16::try_from(y).unwrap_or(i16::MAX)),
                w: slot.w,
                h: slot.h,
                u: slot.x,
                v: slot.y,
                color,
                kind: slot.kind,
                curve,
            };
            let inst = match st.y() {
                1 => Some(inst),
                _ => clip_to(inst, band),
            };
            out.extend(inst);
        }
    }

    /// **The cells of a run fitted to their one cell by rule D**
    /// (`crate::fit`): of the cells from visual column `start` of row `r`
    /// whose glyphs `shaped` holds on `face`, each whose base is a letter
    /// that takes a joining form and whose ink is over 1.1 cells wide, where
    /// a cell its ink reaches is not blank or not on the line, with the
    /// scale and move that fit it.
    fn narrows(
        &mut self,
        term: &Term<'_>,
        face: FaceIx,
        r: Row,
        start: Col,
        shaped: &Shaped,
    ) -> Vec<(CellIx, crate::fit::Narrow)> {
        let size = self.size_of(r);
        let st = Stretch::of(size);
        let range = self.row_cells(r);
        let end = match size.is_double() {
            true => Col::at_index(size.end_of_line(Cols::of_index(range.len())).index()),
            false => Col::at_index(range.len()),
        };
        let cell = self.metrics.cell_w.value() as f32 * st.xf();
        let scale = self.scales[face.index()];
        let glyphs = shaped.glyphs();
        let mut out = Vec::new();
        for (i, p) in glyphs.iter().enumerate() {
            // Each cell once, at its first glyph.
            if glyphs[..i].iter().any(|q| q.cell == p.cell) {
                continue;
            }
            let col = start.index() + usize::from(p.cell.0);
            let base = self
                .cur
                .get(range.start + col)
                .and_then(|c| drawn_base(term, *c));
            if !base.is_some_and(crate::fit::takes_forms) {
                continue;
            }
            let mine = glyphs
                .iter()
                .filter(|q| q.cell == p.cell)
                .map(|q| (q.id, scale.px(f32::from(q.dx)) * st.xf()));
            let ink = self.ink_of(face, mine, st);
            let Some(wide) = crate::fit::Overwide::of(ink, cell) else {
                continue;
            };
            let row = &self.cur[range.clone()];
            let blank = |x: Col| {
                row.get(x.index())
                    .map_or(crate::fit::Blank::No, |c| crate::fit::Blank::of(term, *c))
            };
            if let crate::fit::Fitting::Fitted(n) = wide.fitting(Col::at_index(col), end, blank) {
                out.push((p.cell, n));
            }
        }
        out
    }

    /// **Where the glyphs of one cell on `face` put ink**, each `along`
    /// pixels across from the cell's origin, on a line drawn at `stretch`:
    /// the union of their outlines' boxes ([`Atlas::ink`]), in pixels from
    /// the origin. What every path measures a cell's ink by, so they fit
    /// the same cells.
    fn ink_of(
        &mut self,
        face: FaceIx,
        glyphs: impl Iterator<Item = (GlyphId, f32)>,
        stretch: Stretch,
    ) -> crate::fit::Ink {
        let s = self.scales[face.index()].px(1.0) * stretch.xf();
        let f = self.stack.face(face);
        let mut ink = crate::fit::Ink::NONE;
        for (id, along) in glyphs {
            if let Some(b) = self.atlas.ink(f, face, id) {
                ink = ink.with(along + b.left as f32 * s, along + b.right as f32 * s);
            }
        }
        ink
    }

    /// **A record's cell over 1.1 cells wide**, or `None`: [`Renderer::ink_of`]
    /// of one cell's `glyphs` on a line drawn at `stretch`, from their
    /// offsets in font units, as [`Renderer::narrows`] measures a run's.
    pub(crate) fn overwide(
        &mut self,
        glyphs: &[ClusterGlyph],
        stretch: Stretch,
    ) -> Option<crate::fit::Overwide> {
        let face = glyphs.iter().find_map(|g| match g.source {
            GlyphSource::Face(f) => Some(f),
            GlyphSource::Procedural => None,
        })?;
        let scale = self.scales[face.index()];
        let mine = glyphs
            .iter()
            .filter(|g| g.source == GlyphSource::Face(face))
            .map(|g| (g.id, scale.px(f32::from(g.units.dx)) * stretch.xf()));
        let ink = self.ink_of(face, mine, stretch);
        crate::fit::Overwide::of(ink, self.metrics.cell_w.value() as f32 * stretch.xf())
    }

    /// **`glyphs` -- one cell's, on a line drawn at `stretch` -- fitted to
    /// their cell** by `narrow` (`crate::fit`): each drawn that much smaller
    /// ([`Atlas::slot_fitted_at`]) and placed from its offsets in font
    /// units, as [`Renderer::emit`] places a fitted cell's. A glyph baud
    /// draws itself keeps its place.
    pub(crate) fn fit_glyphs(
        &mut self,
        glyphs: &mut [ClusterGlyph],
        narrow: crate::fit::Narrow,
        stretch: Stretch,
    ) {
        for g in glyphs.iter_mut() {
            let GlyphSource::Face(face) = g.source else {
                continue;
            };
            let f = self.stack.face(face);
            let scale = self.scales[face.index()];
            let at = (Phase::ZERO, stretch);
            g.slot = self
                .atlas
                .slot_fitted_at(f, face, g.id, scale, at, narrow.factor());
            g.dx = narrow.across(scale.px(f32::from(g.units.dx)) * stretch.xf());
            g.dy = narrow.up(scale.px(f32::from(g.units.dy)) * stretch.yf());
            g.phase = Phase::ZERO;
        }
    }

    /// Instances for a shaped run starting at column `start` of row `r`;
    /// with `MIRRORED`, drawn as `drawn` says: flipped, for a mirrored cell
    /// ([`crate::bidi::Mirrored::Flipped`]), its first glyph's image
    /// reversed and every glyph's place mirrored in the span. A run with
    /// no mirrored cell takes the arm that reads no mirror.
    fn emit<const MIRRORED: bool>(
        &mut self,
        term: &Term<'_>,
        face: FaceIx,
        r: Row,
        start: Col,
        (shaped, drawn): (&Shaped, Option<CellDrawn>),
        out: &mut Vec<Instance>,
    ) {
        let drawn = drawn.filter(|_| MIRRORED);
        let flipped = drawn.is_some_and(|d| d.mirrored == Some(crate::bidi::Mirrored::Flipped));
        let lean = drawn.and_then(|d| d.lean);
        // **A letter of a joining script wider than its cell**, fitted to it
        // where rule D says so (`crate::fit`): asked only of a run that held
        // a letter that takes a joining form, and not of a flipped or
        // slanted cell.
        let narrows = match shaped.joins() == crate::bidi::Joins::Seen && !flipped && lean.is_none()
        {
            true => self.narrows(term, face, r, start, shaped),
            false => Vec::new(),
        };
        let size = self.size_of(r);
        let st = Stretch::of(size);
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0) * st.x();
        let baseline = i32::try_from(self.metrics.baseline.value()).unwrap_or(0) * st.y();
        let band = self.band(r);
        let top = box_top(size, band);
        let range = self.row_cells(r);
        for (i, p) in shaped.glyphs().iter().enumerate() {
            let col = start.index() + usize::from(p.cell.0);
            // A glyph belongs to the cell it is placed at; a hidden cell's
            // are dropped here rather than kept out of the run, so a run
            // shapes the same in both blink phases.
            if self.text.get(range.start + col).is_some_and(|t| !t.draws()) {
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
                Some(_) if MIRRORED && span == CellSpan::Two && drawn.is_some() => {
                    let base = drawn.map_or(Codepoint::from_char(' '), |d| d.base);
                    self.phase_in(face, base, CellSpan::Two)
                }
                Some(cell) if span == CellSpan::Two => term
                    .base_of(cell)
                    .as_codepoint()
                    .map_or(self.metrics.phase, |b| {
                        self.phase_in(face, b, CellSpan::Two)
                    }),
                _ => self.metrics.phase,
            };
            // On a sized line the glyph rasterised at its box's size, its
            // offsets and its place in the box with it.
            let phase = match st.x() {
                1 => phase,
                _ => phase.doubled(),
            };
            // A flipped cell's base is its first glyph; its marks keep their
            // own images.
            let narrow = match narrows.is_empty() {
                true => None,
                false => narrows.iter().find(|&&(c, _)| c == p.cell).map(|&(_, n)| n),
            };
            let slot = match (narrow, MIRRORED && flipped && i == 0) {
                (Some(n), _) => {
                    let at = (Phase::ZERO, st);
                    self.atlas
                        .slot_fitted_at(f, face, p.id, scale, at, n.factor())
                }
                (None, true) => self.atlas.slot_flipped_at(f, face, p.id, scale, phase, st),
                (None, false) => self.atlas.slot_at(f, face, p.id, scale, phase, st),
            };
            let Some(slot) = slot else {
                continue;
            };
            let (dx, dy) = match narrow {
                Some(n) => (
                    n.across(scale.px(f32::from(p.dx)) * st.xf()),
                    n.up(scale.px(f32::from(p.dy)) * st.yf()),
                ),
                None => (
                    scale.px(f32::from(p.dx)).round() as i32 * st.x(),
                    scale.px(f32::from(p.dy)).round() as i32 * st.y(),
                ),
            };
            let dx = match MIRRORED && flipped {
                true => InSpan {
                    width: cell_w * i32::try_from(span.cells().index()).unwrap_or(1),
                    dx,
                }
                .mirrored(slot),
                false => dx,
            };
            // A cell SAPV 17 slants left draws its image sheared, placed as
            // the upright one is: the mirrored place above is the upright
            // image's.
            let slot = match lean.filter(|_| MIRRORED) {
                Some(l) => {
                    let flip = match flipped && i == 0 {
                        true => Flip::Flipped,
                        false => Flip::Kept,
                    };
                    self.atlas
                        .slot_sheared_at(f, face, p.id, scale, phase, st, (flip, l))
                        .unwrap_or(slot)
                }
                None => slot,
            };
            let x = i32::try_from(col).unwrap_or(0) * cell_w + dx + slot.left;
            let y = top + baseline - dy + slot.top;
            let inst = Instance {
                x: Screen(i16::try_from(x).unwrap_or(i16::MAX)),
                y: Screen(i16::try_from(y).unwrap_or(i16::MAX)),
                w: slot.w,
                h: slot.h,
                u: slot.x,
                v: slot.y,
                color,
                kind: slot.kind,
                curve,
            };
            // A double-height row draws its half of the character's box.
            let inst = match st.y() {
                1 => Some(inst),
                _ => clip_to(inst, band),
            };
            out.extend(inst);
        }
    }
}

#[cfg(test)]
mod place_diff {
    use super::*;
    use baud_domain::PxRect;

    /// The search the sorted diff replaced, one list against the other.
    fn searched_left(now: &[ImagePlacement], last: &[ImagePlacement], band: Band) -> bool {
        last.iter().any(|p| covers(p, band) && !now.contains(p))
    }

    fn searched_arrived(now: &[ImagePlacement], last: &[ImagePlacement], band: Band) -> bool {
        now.iter().any(|p| covers(p, band) && !last.contains(p))
    }

    /// A pool of placements: equal draws are equal placements, so two
    /// lists share some, differ by others and repeat a few.
    fn pool() -> Vec<ImagePlacement> {
        let placement_of = |n: u32| -> ImagePlacement {
            let rect = PxRect {
                x: n % 3,
                y: 0,
                w: 4,
                h: 4,
            };
            let layer = [ImageLayer::Below, ImageLayer::Under, ImageLayer::Over]
                [usize::try_from(n % 3).unwrap()];
            ImagePlacement {
                id: ImageId::NONE,
                epoch: ImageGen::ZERO,
                frame: FrameNo::ROOT,
                x: i16::try_from(n % 5 * 8).unwrap(),
                y: i16::try_from(n % 7 * 9).unwrap() - 9,
                w: 8,
                h: u16::try_from(n % 4 * 7 + 1).unwrap(),
                alpha: match n % 4 {
                    0 => ImageAlpha::Partial,
                    _ => ImageAlpha::OpaqueOrClear,
                },
                z: ZIndex::of(i32::try_from(n % 2).unwrap() - 1),
                layer,
                tie: KittyId::of(n % 6),
                born: ImageAge::ZERO,
                serial: PlaceSerial::NONE,
                draw: ImageDraw {
                    src: rect,
                    dest: rect,
                },
                scale: match n % 5 {
                    0 => ImageScale::Mapped,
                    k => ImageScale::Unit {
                        x: ImagePx::of(u16::try_from(k).unwrap()),
                        y: ImagePx::ZERO,
                    },
                },
                map: MapAt(usize::try_from(n).unwrap()),
                ground: Pixel::CLEAR,
                composite: ImageComposite::Over,
            }
        };
        (0..40u32).map(placement_of).collect()
    }

    /// **The sorted diff answers every row as the search did**: 4,000
    /// pairs of lists drawn from a pool of 40 placements, repeats among
    /// them, against eight 9-pixel bands, for the rows an image left and
    /// the rows one arrived on, under the text or over it.
    #[test]
    fn the_sorted_diff_answers_every_row_as_the_search_did() {
        let mut seed = 0x9e37_79b9_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let pool = pool();
        let mut diff = PlaceDiff::default();
        let (mut left, mut arrived) = (0, 0);
        for _ in 0..4000 {
            let list = |next: &mut dyn FnMut() -> u32| -> Vec<ImagePlacement> {
                let n = next() % 24;
                (0..n)
                    .map(|_| pool[usize::try_from(next() % 40).unwrap()])
                    .collect()
            };
            let now = list(&mut next);
            let last = match next() % 4 {
                0 => now.clone(),
                _ => list(&mut next),
            };
            diff.diff(&now, &last);
            for r in 0..8u32 {
                let band = Band {
                    top: Px::of(r * 9),
                    bottom: Px::of(r * 9 + 9),
                };
                let l = left_from(&last, &diff.gone, band);
                let a = arrived_on(&now, &diff.arrived, band);
                assert_eq!(l, searched_left(&now, &last, band));
                assert_eq!(a, searched_arrived(&now, &last, band));
                left += usize::from(l);
                arrived += usize::from(a);
            }
        }
        // Both answers were taken both ways, so neither side is constant.
        assert!(left > 0 && arrived > 0, "{left} {arrived}");
        assert!(left < 32_000 && arrived < 32_000, "{left} {arrived}");
    }
    /// **The row index names every placement a row's band meets, in draw
    /// order, and no other**: the pool's 40 placements, some above the
    /// top, some taller than a row, some no taller than a pixel, against
    /// [`covers`] on each of eight 9-pixel bands -- the test every row
    /// asked of every placement before the index.
    #[test]
    fn the_row_index_names_what_covers_answers_in_draw_order() {
        let pool = pool();
        let mut rows = PlaceRows::default();
        for (n, ch) in [(8usize, 9u32), (3, 9), (8, 1), (5, 40)] {
            rows.built = Built::Stale;
            rows.ensure(&pool, Rows::of_count(n), Px::of(ch));
            let mut named = 0;
            for r in 0..n {
                let top = u32::try_from(r).unwrap() * ch;
                let band = Band {
                    top: Px::of(top),
                    bottom: Px::of(top + ch),
                };
                let want: Vec<usize> = (0..pool.len())
                    .filter(|&i| covers(&pool[i], band))
                    .collect();
                assert_eq!(
                    rows.row(Row::from_index(r)),
                    Some(&want[..]),
                    "{n} rows of {ch}, row {r}"
                );
                named += want.len();
            }
            assert!(
                named > 0,
                "{n} rows of {ch}: the premise, some placement on screen"
            );
            assert_eq!(rows.row(Row::from_index(n)), Some(&[][..]), "past the rows");
        }
    }
}

/// **A narrower instance's layout tables are its base's** (`Layout`).
#[cfg(test)]
mod layouts {
    use super::*;

    fn built_face(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/../baud-app/fonts/built/{name}.ttf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    /// The face in `data` read at `wdth` `w`.
    fn at_width<'a>(data: &'a [u8], w: f32) -> Face<'a> {
        let mut f = Face::parse(data).unwrap();
        f.set_variations(&[baud_font::var::Variation {
            tag: Tag::from_bytes(*b"wdth"),
            value: baud_font::var::Fixed::from_f32(w),
        }]);
        f
    }

    /// **A face's layout tables do not depend on the width it is read at**:
    /// each built face and its instance at 62.5, the narrowest, each the
    /// only face of its stack and so building its own, give the same
    /// lookup digests, pair tables, extents, reaches and per-script
    /// selections, value for value -- the premise on which a narrower
    /// instance reads its base's. The control: the two are read at
    /// different coordinates, and advance a Devanagari letter differently.
    #[test]
    fn a_faces_layout_tables_are_the_same_at_every_width() {
        let px = PxSize::of(28.0).unwrap();
        for name in ["Regular", "Bold", "Italic", "BoldItalic"] {
            let data = built_face(name);
            let full = [Face::parse(&data).unwrap()];
            let narrow = [at_width(&data, 62.5)];
            assert_ne!(full[0].coords(), narrow[0].coords(), "{name}: the control");
            let ka = full[0].glyph(Codepoint::from_char('\u{915}')).unwrap();
            assert_ne!(
                full[0].advance(ka),
                narrow[0].advance(ka),
                "{name}: the control, KA narrower"
            );
            let a = PerFace::of(&Stack::new(&full).unwrap(), px);
            let b = PerFace::of(&Stack::new(&narrow).unwrap(), px);
            let l = Layout(0);
            assert!(!a.digests[l].is_empty(), "{name}: the premise, a GSUB");
            assert!(a.digests[l] == b.digests[l], "{name}: digests");
            assert_eq!(a.pairs[l], b.pairs[l], "{name}: pairs");
            assert_eq!(a.extents[l], b.extents[l], "{name}: extents");
            assert_eq!(a.reaches[l], b.reaches[l], "{name}: reaches");
            assert_eq!(
                format!("{:?}", a.selections[l]),
                format!("{:?}", b.selections[l]),
                "{name}: selections"
            );
        }
    }

    /// **A narrower instance reads its base's layout tables, built once**:
    /// the built Regular and its 15 instances, as the app's default stack
    /// lays them, make one layout, where they made 16 -- 2.5 ms of a
    /// launch's renderer and 7 MB over the four built faces. Each instance
    /// reads its own base's: the Regular's and the Bold's, each with its
    /// instances, make two. The control: the same file at another width,
    /// given the role of a fallback, builds its own.
    #[test]
    fn a_narrower_instance_reads_its_bases_layout() {
        let px = PxSize::of(28.0).unwrap();
        let data = built_face("Regular");
        let first = Face::parse(&data).unwrap();
        let widths = crate::narrower_widths(&first, &[]);
        assert_eq!(widths.len(), 15, "the premise: wdth 97.5 down to 62.5");
        let mut faces = vec![first];
        let mut roles = vec![crate::FaceRole::Fallback];
        for w in &widths {
            faces.push(at_width(&data, *w));
            roles.push(crate::FaceRole::Narrower);
        }
        let stack = Stack::with_roles(&faces, &roles).unwrap();
        let per = PerFace::of(&stack, px);
        assert_eq!(per.digests.all().len(), 1, "one layout for 16 faces");
        assert!(per.layouts.iter().all(|&l| l == Layout(0)));
        assert_eq!(per.scales.len(), 16, "a scale for every face");
        assert_eq!(per.faces.len(), 16, "every face by its code");
        // Two bases, each with two instances.
        let bold = built_face("Bold");
        let faces = [
            Face::parse(&data).unwrap(),
            at_width(&data, 90.0),
            at_width(&data, 75.0),
            Face::parse(&bold).unwrap(),
            at_width(&bold, 90.0),
            at_width(&bold, 75.0),
        ];
        let n = crate::FaceRole::Narrower;
        let bold_role = crate::FaceRole::Style(crate::FaceStyle::Bold);
        let roles = [crate::FaceRole::Fallback, n, n, bold_role, n, n];
        let per = PerFace::of(&Stack::with_roles(&faces, &roles).unwrap(), px);
        assert_eq!(per.digests.all().len(), 2, "the Regular's and the Bold's");
        let (r, b) = (Layout(0), Layout(1));
        assert_eq!(per.layouts, [r, r, r, b, b, b]);
        // The control: a fallback face is not an instance.
        let faces = [Face::parse(&data).unwrap(), at_width(&data, 75.0)];
        let fallback = [crate::FaceRole::Fallback; 2];
        let per = PerFace::of(&Stack::with_roles(&faces, &fallback).unwrap(), px);
        assert_eq!(per.digests.all().len(), 2, "the control: its own layout");
    }

    /// **Another file given the narrower role fails in a debug build**:
    /// the Bold after the Regular as its instance, whose tables are not the
    /// Regular's, panics rather than read them.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "a narrower instance holds its base's layout tables")]
    fn another_file_as_a_narrower_instance_is_refused_in_debug() {
        let (regular, bold) = (built_face("Regular"), built_face("Bold"));
        let faces = [Face::parse(&regular).unwrap(), at_width(&bold, 75.0)];
        let roles = [crate::FaceRole::Fallback, crate::FaceRole::Narrower];
        let _ = PerFace::of(
            &Stack::with_roles(&faces, &roles).unwrap(),
            PxSize::of(28.0).unwrap(),
        );
    }
}

/// **At most twice the window's rows are kept** (`crate::kept`).
#[cfg(test)]
mod kept_bound {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{Content, WinSize};

    /// A row of `cols` cells, the first of them `c`.
    fn row(c: Codepoint, cols: Cols) -> Vec<PackedCell> {
        let mut cells = vec![PackedCell::EMPTY; cols.index()];
        cells[0].content = Content::codepoint(c);
        cells
    }

    /// **A window made smaller keeps twice its new rows**: sixteen rows
    /// kept on a window of eight, the window cut to two keeps four, and
    /// they are the four asked for last.
    #[test]
    fn a_window_made_smaller_keeps_twice_its_new_rows() {
        let data = std::fs::read(format!(
            "{}/../baud-app/fonts/built/Regular.ttf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let faces = [Face::parse(&data).unwrap()];
        let ws = WinSize {
            ws_col: 16,
            ws_row: 8,
        };
        let (cols, rows) = (Cols::of_index(16), Rows::of_index(8));
        let mut owned = Owned::new(&ws);
        let t = Term::new(owned.storage(), &ws);
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            cols,
            rows,
        )
        .unwrap();
        let found = || crate::kept::Found {
            parted: Vec::new(),
            words: Vec::new(),
        };
        let ask = |r: &mut Renderer<'_, '_>, n: u8, (y, rows): (usize, Rows)| {
            let cells = row(Codepoint::from_char(char::from(b'A' + n)), cols);
            r.kept_row(&t, (Row::from_index(y), rows), (&cells, &[]), found)
                .2
        };
        for n in 0..16 {
            ask(&mut r, n, (usize::from(n % 8), rows));
        }
        assert_eq!(r.kept.held(), Rows::of_index(16), "the premise: 16 kept");
        let small = Rows::of_index(2);
        r.resize(cols, small);
        assert!(
            r.kept.held().index() <= 4,
            "at most twice the window's rows kept: {:?} for a window of 2",
            r.kept.held()
        );
        for n in 12..16 {
            let asked = ask(&mut r, n, (usize::from(n % 2), small));
            assert_ne!(asked, crate::kept::Asked::Found, "row {n}, asked last");
        }
    }
}

#[cfg(test)]
mod splash_cursor {
    use super::*;
    use baud_core::Owned;
    use baud_domain::WinSize;

    /// **The splash's block blinks as the terminal's cursor does**: at each
    /// phase -- hidden, the glow part way, the fire and the ufo part way, a
    /// fade part way -- the splash's records are its shown ones with the
    /// block at the phase's opacity, then what the phase draws over a
    /// cursor's cell, by the cursor's own drawing (`caret::blink`), over
    /// the block's. Shown, it draws the splash's own records. The controls:
    /// the glow, the fire and the ufo add records, and hidden takes the
    /// block away.
    #[test]
    fn the_splash_block_blinks_as_the_cursor_does() {
        let data = std::fs::read(format!(
            "{}/../baud-app/fonts/built/Bold.ttf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let faces = [Face::parse(&data).unwrap()];
        let ws = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let mut owned = Owned::new(&ws);
        let t = Term::new(owned.storage(), &ws);
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(80),
            Rows::of_index(24),
        )
        .unwrap();
        r.fit(&t);
        r.set_splash(Some(SplashLook {
            phase: splash::SplashPhase::Hold,
        }));
        let shown = r.splash().to_vec();
        r.set_splash_cursor(CursorPhase::SHOWN);
        assert_eq!(r.splash(), &shown[..], "shown: the splash's own");
        let (block, glyphs) = shown.split_last().unwrap();
        let at = |v: i16| Px::of(u32::try_from(i32::from(v)).unwrap());
        let len = |v: Tex| Px::of(u32::from(v.0));
        let cell = (at(block.x.0), at(block.y.0), len(block.w), len(block.h));
        let amber = splash::DARK.amber;
        let part = Chan::from_u8(128);
        for phase in [
            CursorPhase::HIDDEN,
            CursorPhase {
                glow: part,
                ..CursorPhase::HIDDEN
            },
            CursorPhase::fire(part),
            CursorPhase::ufo(part),
            CursorPhase {
                opacity: part,
                ..CursorPhase::SHOWN
            },
        ] {
            r.set_splash_cursor(phase);
            let mut want = glyphs.to_vec();
            let opacity = f32::from(phase.opacity.get()) / 255.0;
            want.push(Instance {
                color: splash::at_alpha(amber, opacity),
                ..*block
            });
            caret::blink(cell, cell, phase, amber, &mut want);
            want.retain(|i| i.color.a().get() > 0);
            assert_eq!(r.splash(), &want[..], "{phase:?}");
            match phase.play {
                Play::Still if phase.glow.get() == 0 && phase.opacity.get() == 0 => {
                    assert_eq!(r.splash().len(), glyphs.len(), "hidden: no block");
                }
                Play::Still if phase.glow.get() == 0 => {}
                _ => assert!(r.splash().len() > shown.len(), "{phase:?}: drawn over it"),
            }
        }
        r.set_splash_cursor(CursorPhase::SHOWN);
        assert_eq!(r.splash(), &shown[..], "shown again");
    }
}
