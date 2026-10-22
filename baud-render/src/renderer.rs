//! The frame: rows that changed, converted and painted.

use crate::atlas::{Atlas, AtlasStats, Code, Kind, Stretch, Tex};
use crate::cache::{
    CacheStats, CellIx, ClusterMemo, Cover, Key, PLACED_MAX, Placed, Question, RUN_CELLS, RunCache,
    Shaped,
};
use crate::caret::{self, CursorPhase, Play};
use crate::cellmap::{
    BlockCursor, BlockPaint, CellSpan, Curves, Ground, Hilite, Line, Resolved, Selected, Shapes,
    Swaps, Text, takes_stress_mark,
};
use crate::color::{Argb, Theme, Video};
use crate::composite::{Blend, Blit, Page, Target, Texels};
use crate::effect::Vfx;
use crate::find::{Current, FindLook};
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
use baud_domain::themefile::CellPaint;
use baud_domain::{
    Attrs, Chan, ClusterEpoch, ClusterSlot, Codepoint, Col, Cols, CursorStyle, GlyphId, ImageAlpha,
    ImageDraw, ImageGen, ImageId, ImagePx, ImageStore, KittyId, LineRendition, PackedCell, Row,
    Rows, Viewport, ZIndex,
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
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
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
/// it, its atlas slot when it has ink, its pair role, and whether that
/// glyph is all of it.
#[derive(Clone, Copy, Debug)]
pub struct Single {
    pub slot: Option<crate::atlas::Slot>,
    pub glyph: GlyphId,
    pub role: PairRole,
    pub face: FaceIx,
    pub glyphs: Glyphs,
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
    /// repainted too few rows to ([`INDEX_ROWS`]).
    pub places_examined: Tally,
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
    f.iter()
        .flat_map(move |f| f.marks.iter().filter(move |m| m.row == row))
}

/// The find field, when it is on `row`.
fn field_on(f: &Option<FindLook>, row: Row) -> Option<&crate::find::Field> {
    f.as_ref().map(|f| &f.field).filter(|f| f.row == row)
}

/// The palette entry a match is washed in, `SGR 33`'s yellow, which WezTerm
/// gives its current match (`[[source: wezterm-copy-e2a98e1]]`): the theme's
/// own yellow, so a palette a program or a theme sets moves it too.
const MARK_SLOT: Chan = Chan::from_offset(33, 30);
/// How much of it: enough to find on a page, little enough to read the
/// text through.
const MARK_ALPHA: u8 = 0x66;

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
        .is_some_and(|n| n.shapes.has(shape) && n.text(blink) == Text::Drawn)
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
#[derive(Clone, PartialEq, Eq)]
struct SizedKey {
    cells: Vec<PackedCell>,
    size: LineRendition,
    cursor: Option<(Col, CursorStyle, Chan)>,
    selection: Option<Highlight>,
    blink: BlinkPhases,
    video: Video,
    hilite: Hilite,
    cursor_color: Option<(u8, u8, u8)>,
    styles: StyleStamp,
    clusters: ClusterEpoch,
    pictured: Vec<crate::Pictured>,
    atlas: Tally,
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
    /// Paints, returning how many placements it examined for the row.
    fn paint(&self, r: &Renderer<'_, '_>, target: &mut Target<'_>, row: Row, page: Page) -> Tally;
    /// Whether it reads the placements by row ([`PlaceRows`]).
    fn by_row(&self) -> Indexed;
}

/// Whether an [`Under`] reads the placements by row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Indexed {
    No,
    Yes,
}

/// Nothing: no placement is under the text.
struct NoUnder;

impl Under for NoUnder {
    #[inline(always)]
    fn paint(&self, _: &Renderer<'_, '_>, _: &mut Target<'_>, _: Row, _: Page) -> Tally {
        Tally::of(0)
    }

    fn by_row(&self) -> Indexed {
        Indexed::No
    }
}

/// The placements under the text, from the term's images.
struct UnderImages<'s, 'b>(&'s ImageStore<'b>);

impl Under for UnderImages<'_, '_> {
    fn paint(&self, r: &Renderer<'_, '_>, target: &mut Target<'_>, row: Row, page: Page) -> Tally {
        r.paint_under(self.0, target, row, page)
    }

    fn by_row(&self) -> Indexed {
        Indexed::Yes
    }
}

/// Whether a partly transparent placement over the text covers the band:
/// its rows are repainted every CPU frame, since its blit over its own last
/// blit would blend it twice. One under the text is blitted only into a
/// row being repainted, over the row's fresh background, so never twice.
fn partial_over(now: &[ImagePlacement], on: &[usize]) -> bool {
    on.iter()
        .any(|&i| now[i].alpha == ImageAlpha::Partial && now[i].layer == ImageLayer::Over)
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
/// `gone` is [`PlaceDiff::gone`], the last frame's placements this one
/// does not hold.
fn left_from(last: &[ImagePlacement], gone: &[usize], band: Band) -> bool {
    gone.iter().any(|&j| covers(&last[j], band))
}

/// Whether a placement under the text arrived on the band, or changed
/// there, since the last CPU frame. Its blit lands only in a row being
/// repainted, under the row's text, so a row it arrives on is repainted
/// where one over the text needs none. `arrived` is
/// [`PlaceDiff::arrived`].
fn arrived_under(now: &[ImagePlacement], arrived: &[usize], band: Band) -> bool {
    arrived
        .iter()
        .any(|&i| now[i].layer.under_text() && covers(&now[i], band))
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
        ImageGen,
        i16,
        i16,
        u16,
        u16,
        ImageAlpha,
        ZIndex,
        KittyId,
        ImageDraw,
        ImageScale,
    ) {
        (
            self.id, self.epoch, self.x, self.y, self.w, self.h, self.alpha, self.z, self.tie,
            self.draw, self.scale,
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
    /// The rows the frame draws: the main display's and the status line's
    /// below them, when a program shows one (`Term::status_rows`).
    rows: Rows,
    /// The main display's rows, the first of `rows`.
    main: Rows,
    prev: Vec<PackedCell>,
    known: Prev,
    cur: Vec<PackedCell>,
    /// Each row's line rendition in the frame being converted, and in the
    /// last: part of a row's damage, since DECDWL on a line whose cells it
    /// leaves alone, or DECSWL, changes how the row is drawn and no cell.
    cur_size: Vec<LineRendition>,
    prev_size: Vec<LineRendition>,
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
    /// Each screen row's records the last time [`Renderer::sized_lines`]
    /// converted it as a sized line, and what they were converted from:
    /// a frame whose row and inputs are the same reuses them.
    sized_rows: Vec<Option<SizedRow>>,
    /// How many of those conversions were made rather than reused.
    sized_converted: Tally,
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
    /// The selection on this frame's view, set by [`Renderer::set_highlight`]
    /// before a convert, and the one the last convert drew -- a row whose
    /// share of it changed is dirty, the cursor's own rule.
    highlight: Option<Highlight>,
    last_highlight: Option<Highlight>,
    /// The colours the selection draws in, read from the terminal each frame
    /// ([`Term::highlight_colors`]): reverse video until an application sets
    /// `OSC 17` or `19`.
    hilite: Hilite,
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
    /// The find overlay on this frame and the last ([`FindLook`]), and its
    /// solids: each match's wash and the field's caret. The rows under
    /// either frame's marks or field are dirty, and the cells under the
    /// field are converted blank, as under a composition.
    find: Option<FindLook>,
    last_find: Option<FindLook>,
    find_inst: Vec<Instance>,
    /// What the cursor is filled with, which a quad trail is drawn in: the
    /// application's `OSC 12` colour, else the default foreground a block
    /// cursor's reversed cell fills with.
    cursor_fill: Argb,
    /// What a block cursor is drawn in, and a bar or underline's fill: the
    /// cursor colour -- a program's `OSC 12`, else the configured one the
    /// core holds -- else the theme's, which may name the cell's own, and
    /// the theme's text under the block ([`Renderer::block_paint`]).
    block_paint: BlockPaint,
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
    /// Whether this frame shows the other video, set by
    /// [`Renderer::set_flash`] before a convert or resolve, the way the blink
    /// phase is.
    flash: VideoFlash,
    /// The visible image placements this frame, screen rects in device
    /// pixels, computed once by [`Renderer::set_images`] and consumed by
    /// both the CPU `paint_images` and the two GPU arms -- one description
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
    /// The placements the last CPU frame drew, so the next one repaints the
    /// rows an image left as well as the rows it is on.
    last_image_places: Vec<ImagePlacement>,
    /// Which placements this CPU frame and the last differ by.
    place_diff: PlaceDiff,
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

/// What the renderer reads of each face of its stack, built once per
/// stack: by [`Renderer::new`], and again by [`Renderer::restack`].
struct PerFace {
    scales: Vec<Scale>,
    digests: Vec<Vec<LookupDigest>>,
    #[cfg(feature = "exceptions")]
    starters: Vec<Starter>,
    pairs: Vec<Pairs>,
    extents: Vec<Option<Count>>,
    selections: Vec<Selections>,
    faces: Vec<FaceIx>,
}

impl PerFace {
    fn of(stack: &Stack<'_, '_>, size: PxSize) -> PerFace {
        let scales = stack
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
        PerFace {
            scales,
            digests,
            #[cfg(feature = "exceptions")]
            starters,
            pairs,
            extents,
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
            digests,
            #[cfg(feature = "exceptions")]
            starters,
            pairs,
            extents,
            selections,
            faces,
        } = PerFace::of(&stack, size);
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
            main: Rows::of_index(0),
            prev: Vec::new(),
            known: Prev::Unknown,
            cur: Vec::new(),
            cur_size: Vec::new(),
            prev_size: Vec::new(),
            pictures: Pictures::new(),
            styles: Vec::new(),
            styles_stamp: StyleStamp::default(),
            sized_rows: Vec::new(),
            sized_converted: Tally::default(),
            theme_gen: ThemeGen::default(),
            bg: Vec::new(),
            fg: Vec::new(),
            curve: Vec::new(),
            text: Vec::new(),
            row_blinks: Vec::new(),
            per_row: Vec::new(),
            glyph_ends: Vec::new(),
            reach: Vec::new(),
            reach_was: Vec::new(),
            last_cursor: None,
            last_cursor_opacity: Chan::from_u8(0xFF),
            highlight: None,
            last_highlight: None,
            hilite: Hilite::REVERSE,
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
            find: None,
            last_find: None,
            find_inst: Vec::new(),
            cursor_fill: Argb::default(),
            block_paint: BlockPaint::SWAP,
            flash_cap: Chan::from_u8(0),
            flash_held: flash::Held::Full,
            flash_caps: std::collections::HashMap::new(),
            fade: None,
            last_fade: None,
            fade_inst: Vec::new(),
            last_clusters: None,
            blink: BlinkPhases::SHOWN,
            blink_slots: Tally::default(),
            rings: Rings::Unused,
            blink_seen: Blinking::default(),
            cursor_blink: CursorPhase::SHOWN,
            glow: None,
            last_glow: None,
            glow_inst: Vec::new(),
            flash: VideoFlash::default(),
            image_places: Vec::new(),
            place_rows: PlaceRows::default(),
            image_maps: Vec::new(),
            last_image_places: Vec::new(),
            place_diff: PlaceDiff::default(),
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
        r.main = rows;
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
            digests,
            #[cfg(feature = "exceptions")]
            starters,
            pairs,
            extents,
            selections,
            faces,
        } = PerFace::of(&stack, size);
        self.faces_of = stack.table();
        self.stack = stack;
        self.scales = scales;
        self.digests = digests;
        #[cfg(feature = "exceptions")]
        {
            self.starters = starters;
            self.singles.clear();
        }
        self.pairs = pairs;
        self.extents = extents;
        self.selections = selections;
        self.faces = faces;
        self.cluster_faces = ClusterMemo::new(Question::Face, FaceIx::PRIMARY);
        self.cluster_covers = ClusterMemo::new(Question::Cover, Cover::Whole);
        self.cache = RunCache::new();
        self.metrics = metrics;
        self.atlas.reset();
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

    /// **The find overlay for the next frame**, or none: every match but
    /// the current one washed in the palette's yellow, the current one
    /// left to the selection the driver sets ([`Renderer::set_highlight`]),
    /// and the field drawn over its cells in the selection's colours with
    /// its caret. The instance and CPU paths draw it in their convert; the
    /// cell path draws the field through [`crate::Overlay::composed`] and
    /// appends [`Renderer::find_solids`] to its records.
    pub fn set_find(&mut self, term: &Term<'_>, look: Option<FindLook>) {
        let mut out = core::mem::take(&mut self.find_inst);
        out.clear();
        if let Some(f) = &look {
            let (cw, ch) = (self.metrics.cell_w.value(), self.metrics.cell_h.value());
            let x = |c: Col| {
                screen(
                    i32::try_from(u32::try_from(c.index()).unwrap_or(0).saturating_mul(cw))
                        .unwrap_or(i32::MAX),
                )
            };
            let y = |r: Row| screen(i32::try_from(self.band(r).top.value()).unwrap_or(i32::MAX));
            let wash = self
                .theme
                .entry(MARK_SLOT, |ch| term.palette(ch))
                .with_alpha(Chan::from_u8(MARK_ALPHA));
            for m in f.marks.iter().filter(|m| m.current == Current::No) {
                let n = u32::try_from(m.to.index().saturating_sub(m.from.index()) + 1).unwrap_or(0);
                out.push(solid(
                    x(m.from),
                    y(m.row),
                    Px::of(n.saturating_mul(cw)),
                    Px::of(ch),
                    wash,
                ));
            }
            let line = self.metrics.thickness.value().max(2);
            out.push(solid(
                x(f.field.caret),
                y(f.field.row),
                Px::of(line),
                Px::of(ch),
                self.defaults.1,
            ));
        }
        self.find_inst = out;
        self.find = look;
    }

    /// The find overlay's solids, for a path that draws records of its
    /// own: after the field's records.
    pub fn find_solids(&self) -> &[Instance] {
        &self.find_inst
    }

    /// The find field as the cell path draws it, or none.
    pub fn find_field(&self) -> Option<Composition> {
        self.find.as_ref().map(|f| f.field.composition())
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
            .then(|| match &self.composition {
                Some(c) if c.row.index() < rows => c.cursor(c.width_on(&view)),
                _ => Some(term.cursor()),
            })
            .flatten()
            .map(|(r, c)| (r, c, term.cursor_style()))
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
                hilite: self.hilite,
                block: match cursored {
                    Cursored::Block => BlockCursor::Yes(self.block_paint),
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
                ground(self.shape_colour(d.bg, d.cursor_fg), d.bg, &mut triples);
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
    ///
    /// **On a line that is not single-width the cursor is its character's
    /// box's**, through the line's geometry ([`stretched`]): two cells wide,
    /// and on a double-height line each row's half of a shape twice as tall,
    /// so an underline cursor shows on the bottom half alone.
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

    /// The colours a selection draws in on the last convert or resolve:
    /// `OSC 17`'s and `19`'s, as [`Term::highlight_colors`] gives them, each
    /// the theme's ([`Theme::selection`]) where the program set none. What a
    /// caller that draws the selection itself -- the cell path -- reads.
    pub fn hilite(&self) -> Hilite {
        self.hilite
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
        // **A change of the selection's colours repaints every row**: row
        // damage compares cells, renditions and where the selection lies,
        // and an `OSC 17` moves none of them. Rare, so the whole frame.
        // Each colour is the program's while it set one, else the
        // theme's, as `OSC 10`/`11` lay over the theme's defaults.
        let rgb = |c: Option<(u8, u8, u8)>| {
            c.map(|(r, g, b)| Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)))
        };
        let colors = term.highlight_colors();
        let hilite = Hilite::of(
            rgb(colors.bg)
                .map(CellPaint::Colour)
                .or(self.theme.selection),
            rgb(colors.fg)
                .map(CellPaint::Colour)
                .or(self.theme.selection_text),
        );
        if hilite != self.hilite {
            self.hilite = hilite;
            self.known = Prev::Unknown;
        }
        // **The cursor's colours**: the core's (a program's `OSC 12`, else
        // the configured colour it was told), else the theme's, which may
        // name the cell's own; and the theme's text under a block, which a
        // program's colour leaves as it is, as kitty, alacritty, ghostty and
        // foot leave theirs (Jev, 0.94 and 0.96). A change comes with a
        // theme, which repaints everything, or with `OSC 12`, as before.
        self.block_paint = BlockPaint::of(
            rgb(term.cursor_color_in(self.video))
                .map(CellPaint::Colour)
                .or(self.theme.cursor),
            self.theme.cursor_text,
        );
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

    /// How many sized rows [`Renderer::sized_lines`] has converted, rather
    /// than reused, since the renderer was made.
    pub fn sized_converted(&self) -> Tally {
        self.sized_converted
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
        self.prev_size = vec![LineRendition::Single; rows.index()];
        self.cur_size = vec![LineRendition::Single; rows.index()];
        self.bg = vec![Argb(0); cells];
        self.fg = vec![Argb(0); cells];
        self.curve = vec![CurveRow::default(); cells];
        self.text = vec![Text::Drawn; cells];
        self.row_blinks = vec![Blinking::UNSEEN; rows.index()];
        self.per_row = (0..rows.index()).map(|_| Vec::new()).collect();
        self.glyph_ends = vec![0; rows.index()];
        self.reach = vec![Reach::NONE; rows.index()];
        self.reach_was = vec![Reach::NONE; rows.index()];
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

    /// **The pixel rows the last [`Renderer::render`] wrote**, top to
    /// bottom: each run of repainted rows as one band, then the rows under
    /// each image blitted over the text, clipped to the frame -- every
    /// such image is blitted on every frame, and one that arrived changes
    /// rows no cell moved (`render_at`). What a presenter copies and
    /// tells its compositor changed; a pixel outside every band is the
    /// frame before's. Bands may overlap.
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
        let bottom = i64::from(self.frame_size().1.value());
        let images = self
            .image_places
            .iter()
            .filter(|p| p.layer == ImageLayer::Over)
            .filter_map(move |p| {
                let top = i64::from(p.y).clamp(0, bottom);
                let end = (i64::from(p.y) + i64::from(p.h)).clamp(0, bottom);
                let px = |v: i64| Px::of(u32::try_from(v).unwrap_or(0));
                (top < end).then(|| Band {
                    top: px(top),
                    bottom: px(end),
                })
            });
        runs.chain(images)
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
        let screen_h = self.main.index() as i64 * ch;
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
            let leaving = (term.looking_back(above) == above).then(|| {
                let up = term.view(above);
                (up.row(Row::from_index(0)), up.rendition(Row::from_index(0)))
            });
            let rows = (0..self.main.index()).map(|r| {
                let y = Row::from_index(r);
                (r as i64, vp.row(y), vp.rendition(y))
            });
            for (r, row, size) in leaving
                .map(|(row, size)| (-1, row, size))
                .into_iter()
                .chain(rows)
            {
                self.place_placeholders(term, imgs, epoch, (r, row, size), snap);
            }
        }
        // The text's order: by `z`, then by the client's id; stable, so
        // placement order breaks what is left.
        self.image_places.sort_by_key(|p| (p.z, p.tie.raw()));
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
    /// its character takes there, twice as wide, and on a double-height
    /// line twice as tall with each row showing its half (`[[source:
    /// dec-std-070]]` § 5.6.1, DECDWL displays "all characters ... in
    /// double-width rendition"). Decided by Jev 2026-10-19, AS_TEXT, 0.97
    /// and 0.95 on a re-ask; kitty draws no sized line.
    fn place_placeholders(
        &mut self,
        term: &Term<'_>,
        imgs: &ImageStore<'_>,
        epoch: ImageGen,
        (r, row, size): (i64, &[PackedCell], LineRendition),
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
        for run in term.placeholder_runs(row) {
            let Some(v) = term.kitty_virtual(run.image, run.placement) else {
                continue;
            };
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
            let at = run.at.index() as i64;
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
                BlockCursor::Yes(BlockPaint::SWAP)
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
    /// **The lines of `view` that are not single-width, converted as the
    /// instance path converts them**, for the path that draws the grid
    /// itself: each such row and its glyphs and decorations, cursor shapes
    /// included, in the order the instance path draws them. The cell shader
    /// draws their backgrounds from the grid, a character's under both its
    /// columns, and leaves the rest to these (`Overlay::ensure`, which masks
    /// the rows), so a sized line is drawn by one conversion on every path.
    /// Nothing is read or converted when every line is single-width. An
    /// input method's `composition` on a sized line blanks the cells under
    /// it, as [`Renderer::convert`] does, for its own records to draw.
    pub fn sized_lines(
        &mut self,
        term: &Term<'_>,
        view: &Viewport<'_>,
        cursor: Option<(Row, Col, CursorStyle)>,
        composition: Option<&Composition>,
        out: &mut Vec<(Row, Vec<Instance>)>,
    ) {
        out.clear();
        let (cols, rows) = (view.cols(), view.rows());
        if !(0..rows.index()).any(|y| view.rendition(Row::from_index(y)).is_double()) {
            return;
        }
        self.sized_ready(term);
        if self.theme.finds_pictures() {
            self.pictures.find(term, view, self.video);
        } else {
            self.pictures.clear(cols, rows);
        }
        let ncols = cols.index();
        if self.sized_rows.len() != rows.index() {
            self.sized_rows = (0..rows.index()).map(|_| None).collect();
        }
        for y in 0..rows.index() {
            let r = Row::from_index(y);
            let size = view.rendition(r);
            if !size.is_double() {
                continue;
            }
            self.cur[y * ncols..(y + 1) * ncols].copy_from_slice(view.row(r));
            self.cur_size[y] = size;
            if let Some(c) = composition.filter(|c| c.row == r) {
                let (start, end) = c.columns(c.width_on(view));
                for cell in &mut self.cur[y * ncols + start..y * ncols + end] {
                    *cell = PackedCell {
                        content: baud_domain::Content::EMPTY,
                        style: cell.style,
                        flags: baud_domain::CellFlags::NONE,
                    };
                }
            }
            // **Converted again only when something it is drawn from moved**:
            // a steady screen, or one whose other rows change, reuses the
            // line's records. Under the picture finder a row enters a block
            // without a cell of its own changing, so the key holds what the
            // finder found on the row this frame.
            let key = SizedKey {
                cells: self.cur[y * ncols..(y + 1) * ncols].to_vec(),
                size,
                cursor: cursor
                    .filter(|&(cr, _, _)| cr == r)
                    .map(|(_, c, s)| (c, s, self.cursor_opacity(s))),
                selection: self.highlight.filter(|h| h.covers_row(r)),
                blink: self.blink,
                video: self.video,
                hilite: self.hilite(),
                cursor_color: term.cursor_color_in(self.video),
                styles: self.styles_stamp,
                clusters: term.cluster_epoch(),
                pictured: self.pictures.row(r).to_vec(),
                atlas: self.atlas.stats().grown,
            };
            let reuse = self.sized_rows[y].as_ref().is_some_and(|s| s.key == key);
            if !reuse {
                let mut exc = Exceptions::default();
                self.convert_row(term, r, cursor, &mut exc);
                self.sized_converted.bump();
                self.sized_rows[y] = Some(SizedRow {
                    key,
                    records: self.per_row[y].clone(),
                });
            }
            if let Some(s) = &self.sized_rows[y] {
                out.push((r, s.records.clone()));
            }
        }
    }

    /// The renderer fitted to `term` and its rendition table brought up to
    /// date, for a conversion outside [`Renderer::convert`].
    fn sized_ready(&mut self, term: &Term<'_>) {
        self.fit(term);
        let mut styles = core::mem::take(&mut self.styles);
        if styles.len() != term.styles().len() {
            styles = vec![Resolved::default(); term.styles().len()];
            self.styles_stamp = StyleStamp::default();
        }
        let mut stamp = self.styles_stamp;
        let _ = self.sync_styles(term, &mut styles, &mut stamp);
        (self.styles, self.styles_stamp) = (styles, stamp);
    }

    /// **The leaving row of a smooth scroll, when its line is not
    /// single-width**: `cells`, the line just above the window, converted as
    /// the instance path converts a line of rendition `size` and lifted one
    /// row, to draw row -1, where the cell path slides it in. The cell path
    /// draws that row from the map alone, at single width, and masks it for
    /// these when `size` is not ([`crate::Overlay::leaving`]). Nothing for a
    /// single-width line.
    ///
    /// Converted on row 0's buffers, which are saved and put back whole --
    /// its cells, rendition, records, colours, blink, reach and glyph ends
    /// -- so the frame the instance path reuses is untouched; with no
    /// picture, as the cell path never takes the leaving row for one, and
    /// no cursor, which is never on it.
    pub fn sized_leaving(
        &mut self,
        term: &Term<'_>,
        cells: &[PackedCell],
        size: LineRendition,
        out: &mut Vec<Instance>,
    ) {
        out.clear();
        if !size.is_double() {
            return;
        }
        self.sized_ready(term);
        let n = self.cols.index().min(cells.len());
        if n == 0 || self.rows.index() == 0 {
            return;
        }
        let r0 = Row::ZERO;
        let range = self.row_cells(r0);
        let kept = (
            self.cur[range.clone()].to_vec(),
            self.cur_size[0],
            core::mem::take(&mut self.per_row[0]),
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
        self.cur[range.start..range.start + n].copy_from_slice(&cells[..n]);
        self.cur_size[0] = size;
        let mut exc = Exceptions::default();
        self.convert_row(term, r0, None, &mut exc);
        let ch = i16::try_from(self.metrics.cell_h.value()).unwrap_or(i16::MAX);
        out.extend(self.per_row[0].iter().map(|i| Instance {
            y: Screen(i.y.0.saturating_sub(ch)),
            ..*i
        }));
        core::mem::swap(&mut self.pictures, &mut pictures);
        let (cur, size, per_row, bg, fg, curve, text, blinks, ends, reach) = kept;
        self.cur[range.clone()].copy_from_slice(&cur);
        self.cur_size[0] = size;
        self.per_row[0] = per_row;
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
        let rows = Rows::of_index(main.index() + term.status_rows().index());
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

    /// **The status line's row, converted as the instance path converts a
    /// line**, for the path that draws the grid itself: its glyphs and
    /// decorations, its cursor's shapes included, at the row below the main
    /// display. The cell shader draws its backgrounds from the window copy's
    /// row after the main display's and leaves the rest to these. Nothing
    /// while no status line is shown.
    pub fn status_records(
        &mut self,
        term: &Term<'_>,
        cursor: Option<(Row, Col, CursorStyle)>,
        out: &mut Vec<Instance>,
    ) {
        out.clear();
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
        self.cur[range.start..range.start + n].copy_from_slice(&line.cells[..n]);
        self.cur_size[r.index()] = line.rendition;
        let mut exc = Exceptions::default();
        self.convert_row(term, r, cursor, &mut exc);
        out.extend_from_slice(&self.per_row[r.index()]);
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
        let vp = term.view(back);
        for r in 0..main.index() {
            self.cur[r * ncols..(r + 1) * ncols].copy_from_slice(vp.row(Row::from_index(r)));
            self.cur_size[r] = vp.rendition(Row::from_index(r));
        }
        // **The status line's row, below the main display's**, whatever the
        // window looks back on: it is no line of the history.
        let status = term.status_line();
        if let Some(line) = &status {
            let r = main.index();
            if r < rows.index() {
                let n = ncols.min(line.cells.len());
                self.cur[r * ncols..r * ncols + n].copy_from_slice(&line.cells[..n]);
                self.cur_size[r] = line.rendition;
            }
        }
        // An input method's composition, on the live view: the cells under it
        // converted blank in their own renditions, so their backgrounds stay
        // and their glyphs give way to its records, built below. The cell it
        // starts on lends the records its rendition.
        let composition = self
            .composition
            .clone()
            .filter(|c| back.index() == 0 && c.row.index() < main.index());
        let mut under = PackedCell::EMPTY;
        if let Some(c) = &composition {
            let (start, end) = c.columns(c.width_on(&vp));
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
        // The find field, on whatever view the window shows: the cells
        // under it converted blank in their renditions, as under a
        // composition, and drawn over by its records below in the
        // selection's colours.
        let field = self
            .find
            .as_ref()
            .map(|f| f.field.composition())
            .filter(|c| c.row.index() < main.index());
        if let Some(c) = &field {
            let (start, end) = c.columns(cols);
            let at = c.row.index() * ncols;
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
        let find_changed = self.find != self.last_find;
        let mut rows_dirty = Tally::default();
        let mut rows_partial = Tally::default();
        let mut exceptions = Exceptions::default();
        // Which placements left and arrived, and whether one under the
        // text arrived or changed, and whether one partly transparent is
        // over it: once a frame, so a frame with none asks no row about it.
        let mut diff = core::mem::take(&mut self.place_diff);
        if images == ImageRows::Repainted {
            diff.diff(&self.image_places, &self.last_image_places);
        }
        let under_new = images == ImageRows::Repainted
            && diff
                .arrived
                .iter()
                .any(|&i| self.image_places[i].layer.under_text());
        let any_partial = images == ImageRows::Repainted
            && self
                .image_places
                .iter()
                .any(|p| p.alpha == ImageAlpha::Partial && p.layer == ImageLayer::Over);
        // Every row asks whether one is over it.
        if any_partial {
            self.place_rows
                .ensure(&self.image_places, self.rows, self.metrics.cell_h);
        }
        for r in 0..rows.index() {
            let range = self.row_cells(Row::from_index(r));
            let changed = self.cur[range.clone()] != self.prev[range]
                || self.cur_size[r] != self.prev_size[r];
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
            // A match marked, unmarked or moved on this row, or the find
            // field come, gone or changed on it.
            let found = find_changed
                && (marks_on(&self.find, row).ne(marks_on(&self.last_find, row))
                    || field_on(&self.find, row) != field_on(&self.last_find, row));
            // An image the CPU path blitted over this row last frame is not
            // there now: see `render_at`.
            let imaged = images == ImageRows::Repainted
                && left_from(&self.last_image_places, &diff.gone, band);
            // And one the CPU path blits over this row that is partly
            // transparent: repainted every frame, and counted.
            let partial = any_partial
                && partial_over(&self.image_places, self.place_rows.row(row).unwrap_or(&[]));
            // And one under the text that arrived or changed on this row.
            let imaged =
                imaged || (under_new && arrived_under(&self.image_places, &diff.arrived, band));
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
                || composed
                || found;
        }
        self.place_diff = diff;
        if images == ImageRows::Repainted {
            self.last_image_places.clone_from(&self.image_places);
        }
        self.compose_inst.clear();
        if let Some(c) = &composition {
            let styles = core::mem::take(&mut self.styles);
            let mut records = core::mem::take(&mut self.compose_inst);
            let size = self.size_of(c.row);
            crate::overlay::composition_records(
                self,
                term,
                c,
                under,
                &styles,
                cursor,
                cols,
                size,
                Px::of(0),
                &mut records,
            );
            (self.styles, self.compose_inst) = (styles, records);
        }
        if let Some(c) = &field {
            let styles = core::mem::take(&mut self.styles);
            let mut records = core::mem::take(&mut self.compose_inst);
            crate::overlay::composition_records(
                self,
                term,
                c,
                PackedCell::EMPTY,
                &styles,
                None,
                cols,
                LineRendition::Single,
                Px::of(0),
                &mut records,
            );
            (self.styles, self.compose_inst) = (styles, records);
        }
        self.compose_inst.extend_from_slice(&self.find_inst);
        self.reach_was.clone_from(&self.reach);
        for r in 0..rows.index() {
            if self.dirty[r] {
                self.convert_row(term, Row::from_index(r), cursor, &mut exceptions);
                rows_dirty.bump();
            }
        }
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
        }
        self.blink_seen = self
            .row_blinks
            .iter()
            .fold(Blinking::UNSEEN, |a, &b| a.with(b));
        core::mem::swap(&mut self.prev, &mut self.cur);
        // Copied, not swapped as the cells are: `cur_size` stays this
        // frame's after it, for the cursor's box a caller asks for between
        // frames (`Renderer::cursor_rect`).
        self.prev_size.clone_from(&self.cur_size);
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
        self.last_find.clone_from(&self.find);
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
        // Whether any row's glyphs reach out of its band, which the loop
        // below then draws into the bands they reach: once a frame, so a
        // screen whose glyphs stay in their rows asks no row about it.
        let reaching =
            (0..self.rows.index()).any(|q| self.reach[q].leaves(self.band(Row::from_index(q))));
        if under.by_row() == Indexed::Yes
            && self.dirty[..self.rows.index()]
                .iter()
                .filter(|&&d| d)
                .count()
                >= INDEX_ROWS
        {
            self.place_rows
                .ensure(&self.image_places, self.rows, self.metrics.cell_h);
        }
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
            let examined = under.paint(self, target, row, page);
            frame.places_examined.add(examined);
            // In the order `instances()` hands the GPU instance path: the
            // glyphs of the rows above that reach down into this band, the
            // row's own glyphs and decorations, the glyphs of the rows below
            // that reach up into it -- each row's glyphs and then its
            // decorations, rows top to bottom -- then the trail under the
            // scrollbar, then the screen fade over everything. The trail and
            // the fade dirtied these rows (`trail_moved`, and a fade makes
            // the frame full), so a row repainted without them would draw
            // them out.
            // Only the glyphs whose own rows meet the band: a row that
            // reaches holds mostly glyphs that do not.
            let meets = |g: &&Instance| Reach::of(core::slice::from_ref(*g)).meets(band);
            if reaching {
                for q in (0..r).filter(|&q| self.reach[q].meets(band)) {
                    for inst in self.per_row[q][..self.glyph_ends[q]].iter().filter(meets) {
                        target.over(inst, &self.atlas, self.blend, page, band);
                    }
                }
            }
            for inst in &self.per_row[r] {
                target.over(inst, &self.atlas, self.blend, page, band);
            }
            if reaching {
                for q in (r + 1..self.rows.index()).filter(|&q| self.reach[q].meets(band)) {
                    for inst in self.per_row[q][..self.glyph_ends[q]].iter().filter(meets) {
                        target.over(inst, &self.atlas, self.blend, page, band);
                    }
                }
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

    /// [`Renderer::cluster`] on a line drawn at `stretch`: each glyph
    /// rasterised at the box a character there takes, as the line's runs
    /// are ([`Atlas::slot_at`], [`Atlas::synthetic_at`]), its offsets
    /// scaled with it. A composition on a sized line is drawn by these.
    pub(crate) fn cluster_at(
        &mut self,
        cps: &[Codepoint],
        style: FaceStyle,
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
                slot: self.procedural_at(base, stretch),
                dx: 0,
                dy: 0,
                phase: Phase::ZERO,
            }];
        }
        let face = self.face_for(cps, style);
        let phase = self.phase_in(face, base, span).doubled();
        let clusters = [ClusterIn {
            cps: cps.to_vec(),
            at: CellIx(0),
        }];
        let shaped = self.shape(face, Script::of_cluster(cps), &clusters, ..);
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
                glyphs: Glyphs::One,
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
        let glyphs = match shaped.glyphs().len() {
            1 => Glyphs::One,
            _ => Glyphs::Several,
        };
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
            glyphs,
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
                    self.rings = self.rings.after(new.shapes);
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
        // **The floor again, on the glass** (Jev, logged: 0.96 and 0.99),
        // measured against the page at its alpha over what the material
        // can show, `Theme::backdrop` -- black to white on the glass (Jev's
        // lean, 0.60/0.59, decisions 736-737) -- since the
        // renderer draws onto a target with nothing under it and the glass
        // is the window server's (`Floor::lift_on_glass`). It covers a page
        // the program set; the default page is left out: its own text is
        // held at the bound by the dims (note AS, section 3). A picture's,
        // an opaque rendition's and a swap's page is solid, and
        // `Resolved::drawn` gives them `fg`.
        let glass = if page_word.is_default() || opaque_bg || self.theme.bg_alpha.get() == 0xFF {
            fg
        } else {
            self.theme
                .floor
                .lift_on_glass(fg, bg, self.theme.bg_alpha, self.theme.backdrop)
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
        let mut cursor_shapes: Vec<Instance> = Vec::new();
        // A sized line's ring parts, on its geometry already (`edges`).
        let mut rings: Vec<Instance> = Vec::new();
        // Colours first, cell by cell, with the shapes a rendition asks
        // for: an underline, a strike, an underline or bar cursor. A block
        // cursor is the cell's colours swapped. Shapes go on after the
        // glyphs, so a strike crosses its letter rather than hiding under
        // an opaque colour glyph.
        let mut shapes: Vec<Instance> = Vec::new();
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
                    BlockCursor::Yes(block_paint)
                }
                _ => BlockCursor::No,
            };
            let d = res.drawn(Swaps {
                pictured: self.pictures.at(r, col),
                line: Line::of(cell.content),
                selected,
                hilite: self.hilite,
                block,
                blink: self.blink,
                cursor: here.map_or(Chan::from_u8(0xFF), |(_, _, s)| self.cursor_opacity(s)),
            });
            let (fg, hidden) = (d.fg, d.text == Text::Hidden);
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
            // The overline, the frame, an encircled run and the stress mark,
            // out of line behind one test: most cells carry none of them.
            if !hidden && res.shapes.any(Shapes::EDGES) {
                self.edges(term, r, col, &row, res, d, &mut shapes, &mut rings);
            }
            if let Some((
                _,
                _,
                style @ (CursorStyle::BlinkingUnderline
                | CursorStyle::SteadyUnderline
                | CursorStyle::BlinkingBar
                | CursorStyle::SteadyBar),
            )) = here
            {
                // The bar or underline cursor draws in the cursor colour if
                // one is set, else the cell's foreground, at its opacity.
                let cur = self
                    .shape_colour(d.bg, d.cursor_fg)
                    .scaled(self.cursor_opacity(style));
                // Already on the line's geometry (`Renderer::cursor_px`).
                let (left, y, w, h) = self.cursor_px(r, col, style);
                cursor_shapes.push(solid(screen(px_i32(left)), screen(px_i32(y)), w, h, cur));
            }
        }
        if size.is_double() {
            let band = self.band(r);
            shapes = shapes
                .into_iter()
                .filter_map(|s| stretched(s, size, band))
                .collect();
        }
        shapes.extend(rings);
        shapes.extend(cursor_shapes);
        self.row_blinks[r.index()] = blinks;
        // Then runs of cells shaped by one face, in one selection of its
        // lookups. The row's scripts are settled the first time a run is on
        // a face whose scripts select different lookups, and not at all on a
        // row drawn only by faces whose scripts all select alike -- which is
        // every row the embedded faces draw alone.
        let mut settled = false;
        let mut c = 0usize;
        while c < line_end {
            let Some(base) = drawn_base(term, row[c]) else {
                c += 1;
                continue;
            };
            if crate::boxes::covers(base) {
                // Box drawing, a block element or braille: baud's own mask
                // at the cell origin, never shaped, never in a run.
                if let Some(slot) = self.procedural_at(base, st) {
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
            while c < line_end && c - start < RUN_CELLS {
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
        self.glyph_ends[r.index()] = instances.len();
        self.reach[r.index()] = Reach::of(&instances);
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
                //
                // `without_unmapped` starts from the cluster's canonical form,
                // so a base and a mark whose canonical composite no face draws
                // are that composite's one `.notdef`, nothing left out: A +
                // U+0311 is Ȃ's box. Only a mark with no composite is left out
                // and counted.
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
        self.cur_size
            .get(r.index())
            .copied()
            .unwrap_or(LineRendition::Single)
    }

    /// The atlas mask for a procedural codepoint at this cell size.
    fn procedural(&mut self, cp: Codepoint) -> Option<crate::atlas::Slot> {
        self.procedural_at(cp, Stretch::One)
    }

    /// The same for a line drawn at `stretch` ([`Atlas::synthetic_at`]).
    fn procedural_at(&mut self, cp: Codepoint, stretch: Stretch) -> Option<crate::atlas::Slot> {
        let cell = crate::boxes::Cell {
            w: self.metrics.cell_w,
            h: self.metrics.cell_h,
        };
        self.atlas
            .synthetic_at(cp, cell, self.metrics.baseline, stretch)
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
        let size = self.size_of(r);
        let st = Stretch::of(size);
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0) * st.x();
        let baseline = i32::try_from(self.metrics.baseline.value()).unwrap_or(0) * st.y();
        let band = self.band(r);
        let top = box_top(size, band);
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
            // On a sized line the glyph rasterised at its box's size, its
            // offsets and its place in the box with it.
            let phase = match st.x() {
                1 => phase,
                _ => phase.doubled(),
            };
            let Some(slot) = self.atlas.slot_at(f, face, p.id, scale, phase, st) else {
                continue;
            };
            let dx = scale.px(f32::from(p.dx)).round() as i32 * st.x();
            let dy = scale.px(f32::from(p.dy)).round() as i32 * st.y();
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
        now.iter()
            .any(|p| p.layer.under_text() && covers(p, band) && !last.contains(p))
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
            }
        };
        (0..40u32).map(placement_of).collect()
    }

    /// **The sorted diff answers every row as the search did**: 4,000
    /// pairs of lists drawn from a pool of 40 placements, repeats among
    /// them, against eight 9-pixel bands, for the rows an image left and
    /// the rows one arrived on under the text.
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
                let a = arrived_under(&now, &diff.arrived, band);
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
