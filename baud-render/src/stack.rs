//! The faces a frame draws with, in fallback order.

use crate::atlas::{Code, VAR_POINTS_MAX};
use crate::units::Tally;
use baud_domain::{Attrs, Codepoint, FontId, GlyphId};
use baud_font::bytes::Count;
use baud_font::{Face, Normalised};

/// A face's position in the stack. A byte, because a fallback chain of
/// more than 255 faces is a configuration error rather than a font stack.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FaceIx(u8);

impl FaceIx {
    pub const PRIMARY: FaceIx = FaceIx(0);

    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// Which face a cell's text is drawn from, from its bold and italic.
/// `SGR 2` faint is not a weight here: it is drawn as a dimmer colour.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum FaceStyle {
    #[default]
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FaceStyle {
    pub const ALL: [FaceStyle; 4] = [
        FaceStyle::Regular,
        FaceStyle::Bold,
        FaceStyle::Italic,
        FaceStyle::BoldItalic,
    ];

    /// The style a rendition asks for.
    pub fn of(attrs: Attrs) -> FaceStyle {
        match (attrs.has(Attrs::BOLD), attrs.has(Attrs::ITALIC)) {
            (false, false) => FaceStyle::Regular,
            (true, false) => FaceStyle::Bold,
            (false, true) => FaceStyle::Italic,
            (true, true) => FaceStyle::BoldItalic,
        }
    }

    /// Two bits, as the cell shader reads them from a resolved slot.
    pub const fn code(self) -> Code {
        Code(match self {
            FaceStyle::Regular => 0,
            FaceStyle::Bold => 1,
            FaceStyle::Italic => 2,
            FaceStyle::BoldItalic => 3,
        })
    }

    /// The style without its italic: regular for italic, bold for bold
    /// italic.
    pub fn upright(self) -> FaceStyle {
        match self {
            FaceStyle::Italic => FaceStyle::Regular,
            FaceStyle::BoldItalic => FaceStyle::Bold,
            s => s,
        }
    }

    /// The style two bits name; the bits above them are ignored.
    pub const fn from_code(code: Code) -> FaceStyle {
        match code.0 & 3 {
            0 => FaceStyle::Regular,
            1 => FaceStyle::Bold,
            2 => FaceStyle::Italic,
            _ => FaceStyle::BoldItalic,
        }
    }
}

/// **Which font a rendition's text is drawn in**: `SGR 10`'s primary --
/// the fallback chain -- or one of `SGR 11`-`19`'s nine alternatives,
/// and over either `SGR 20`'s Fraktur face (`[[source: ecma48]]`
/// § 8.3.117). Five bits: the alternative in the low four, Fraktur above.
/// A slot the stack has no face for is never named
/// ([`Stack::font_of`]), so a rendition that selects an unconfigured font
/// resolves to the primary and keys the glyph map as it always has.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct FontSlot(u8);

impl FontSlot {
    /// `SGR 10`, the chain itself.
    pub const PRIMARY: FontSlot = FontSlot(0);
    /// `SGR 20`'s Fraktur face over the primary.
    pub const FRAKTUR: FontSlot = FontSlot(FontSlot::FRAKTUR_BIT);
    const FRAKTUR_BIT: u8 = 16;
    const ALTERNATIVE: u8 = 15;

    /// The font `SGR 10`-`19` selected: the primary for 10, else its
    /// alternative.
    pub fn alternative(id: FontId) -> FontSlot {
        FontSlot(u8::try_from(id.index()).unwrap_or(0) & FontSlot::ALTERNATIVE)
    }

    /// This font with the Fraktur face over it.
    pub fn with_fraktur(self) -> FontSlot {
        FontSlot(self.0 | FontSlot::FRAKTUR_BIT)
    }

    /// Whether the Fraktur face is over it.
    pub fn has_fraktur(self) -> bool {
        self.0 & FontSlot::FRAKTUR_BIT != 0
    }

    /// The alternative alone, the primary for none, without Fraktur.
    pub fn alternative_only(self) -> FontSlot {
        FontSlot(self.0 & FontSlot::ALTERNATIVE)
    }

    /// Five bits, as a resolved slot's shapes carry them.
    pub fn code(self) -> Code {
        Code(u32::from(self.0))
    }

    /// The font five bits name; the bits above them are ignored.
    pub fn from_code(code: Code) -> FontSlot {
        FontSlot(u8::try_from(code.0 & 31).unwrap_or(0))
    }

    /// One face a role can name: an alternative alone, or Fraktur alone.
    fn is_one(self) -> bool {
        self == FontSlot::FRAKTUR || (self.0 != 0 && self.0 & FontSlot::FRAKTUR_BIT == 0)
    }
}

/// **What a cell's glyph is looked up by**: the face style and the font.
/// A rendition in the primary font is its style alone, so every key a
/// default stack builds is the one it built before fonts were drawn.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct TextFace {
    pub style: FaceStyle,
    pub font: FontSlot,
}

/// **How far an italic leans, as a shear**: the tangent of its angle
/// from the vertical, a glyph's every row moved that much of its height
/// above the baseline, to the left ([`Stack::lean`]).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lean(f32);

impl Lean {
    pub fn tan(self) -> f32 {
        self.0
    }
}

impl TextFace {
    /// This face with its italic dropped: what a cell SAPV 17 slants left
    /// is drawn from ([`Stack::lean`]).
    pub fn upright(self) -> TextFace {
        TextFace {
            style: self.style.upright(),
            font: self.font,
        }
    }

    /// The style's two bits with the font's five above them: a style's own
    /// code in the primary font.
    pub fn code(self) -> Code {
        Code(self.style.code().0 | (self.font.code().0 << 2))
    }
}

impl From<FaceStyle> for TextFace {
    fn from(style: FaceStyle) -> TextFace {
        TextFace {
            style,
            font: FontSlot::PRIMARY,
        }
    }
}

/// How a character asks to be drawn: from a text face or a colour one.
/// `U+FE0E` asks for text and `U+FE0F` for colour; a character with neither
/// takes the renderer's default (`EmojiDefault`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Presentation {
    Text,
    Colour,
}

/// How a character with no selector is drawn: `font` or `system`. Under
/// `Font`, the default, it is the font's text glyph whatever its default
/// presentation -- `⚡` is JetBrains Mono's bolt -- and only `U+FE0F` asks for
/// colour. Under `System` a character whose default presentation is emoji
/// (`Emoji_Presentation=Yes`, `⚡` but not `❤`) is colour too, as macOS's own
/// text draws it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EmojiDefault {
    #[default]
    Font,
    System,
}

impl EmojiDefault {
    /// The presentation a character asks for: its selector -- the codepoint
    /// straight after it in its cluster -- when that is `U+FE0E` or
    /// `U+FE0F`, which wins; else this default's.
    pub fn presentation(self, base: Codepoint, selector: Option<Codepoint>) -> Presentation {
        match selector.map(Codepoint::value) {
            Some(0xfe0f) => Presentation::Colour,
            Some(0xfe0e) => Presentation::Text,
            _ => match self {
                EmojiDefault::System if baud_domain::vs16::presents_as_emoji(base) => {
                    Presentation::Colour
                }
                _ => Presentation::Text,
            },
        }
    }
}

/// What one face does with one codepoint, for [`Stack::choose`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Draws {
    No,
    Text,
    Colour,
}

/// The face every codepoint is drawn from, per style and presentation:
/// [`Stack::select_presented`] as a lookup (`Stack::table`). Two stages: a
/// block of 256 codepoints names a row, and identical rows are stored once,
/// so the blocks no face reaches -- most of Unicode -- share one.
pub struct FaceTable {
    /// `COMBOS` runs of `BLOCKS` row ids, one run per style and
    /// presentation; [`FaceTable::UNFILLED`] until the block is decided.
    blocks: Vec<u16>,
    rows: Vec<[u8; 256]>,
    /// Row to id, so a block identical to one already decided shares it.
    index: crate::seeded::Map<[u8; 256], u16>,
    /// Per block, the faces whose `cmap` could reach it; emptied when the
    /// block is filled.
    proposed: Vec<Vec<u8>>,
    /// The row every combination takes where no face reaches.
    idle: [u16; FaceTable::COMBOS],
    /// How many blocks have been filled on demand.
    filled: Filled,
    /// Characters passed over on a face that refuses its glyph for them
    /// ([`FaceTable::refused`]).
    refused: Tally,
    /// The characters no face draws that a cell was looked up for since
    /// the last [`FaceTable::take_gaps`], and every such character this
    /// table has listed, a bit a codepoint, so each is listed once and a
    /// cell drawn with one again pays a bit test: made at the first miss.
    gaps: Vec<Codepoint>,
    met: Vec<u64>,
}

impl FaceTable {
    const BLOCKS: usize = 0x1100;
    const COMBOS: usize = 8;

    const UNFILLED: u16 = u16::MAX;

    /// A row's entry for a character no face draws in any way: the primary
    /// draws its `.notdef`, and [`FaceTable::face`] lists it. A stack holds
    /// at most 255 faces, so no face is numbered 255.
    const NOBODY: u8 = u8::MAX;

    fn empty(proposed: Vec<Vec<u8>>) -> FaceTable {
        FaceTable {
            blocks: vec![FaceTable::UNFILLED; FaceTable::COMBOS * FaceTable::BLOCKS],
            rows: Vec::new(),
            index: crate::seeded::Map::default(),
            proposed,
            idle: [0; FaceTable::COMBOS],
            filled: Filled(0),
            refused: Tally::default(),
            gaps: Vec::new(),
            met: Vec::new(),
        }
    }

    /// **The characters no face draws that cells were drawn with since the
    /// last call**, each once per table, in the order first met: what the
    /// caller's system fallback is asked for. Assigned or not, control or
    /// not: which of them a system face could draw is the caller's to
    /// decide.
    pub fn take_gaps(&mut self) -> Vec<Codepoint> {
        core::mem::take(&mut self.gaps)
    }

    /// How many characters a face maps to a glyph it draws not at all
    /// (`drawing`: its outline refused or past the scratch budget, and no
    /// colour image that draws; or a `COLR` layer that cannot draw over an
    /// outline that is empty or cannot draw either) were
    /// passed over for another face, once per face and character, counted
    /// when the character's block is decided -- the first time any cell
    /// lands in it. Such a glyph the table keeps, the primary's when no
    /// other face carries the character, is not counted here: the atlas
    /// counts it when it draws the primary's `.notdef` in its place
    /// (`AtlasStats::refused`). A colour glyph whose outline is refused and
    /// whose strike image or layers draw is kept, so never counted here.
    pub fn refused(&self) -> Tally {
        self.refused
    }

    /// The styles and presentations in table order: the style's two-bit code
    /// times two, plus one for colour.
    fn combos() -> impl Iterator<Item = (FaceStyle, Presentation)> {
        FaceStyle::ALL
            .into_iter()
            .flat_map(|s| [(s, Presentation::Text), (s, Presentation::Colour)])
    }

    fn combo(style: FaceStyle, p: Presentation) -> Combo {
        Combo(
            usize::try_from(style.code().0).unwrap_or(0) * 2
                + usize::from(p == Presentation::Colour),
        )
    }

    fn intern(&mut self, row: [u8; 256]) -> RowId {
        if let Some(&id) = self.index.get(&row) {
            return RowId(id);
        }
        let id = u16::try_from(self.rows.len()).unwrap_or(u16::MAX);
        self.rows.push(row);
        self.index.insert(row, id);
        RowId(id)
    }

    /// The face `c` is drawn from in `style` when it asks for `p`, deciding
    /// `c`'s block from `stack` the first time any cell lands in it. `stack`
    /// must be the one the table was made from.
    pub fn face(
        &mut self,
        stack: &Stack<'_, '_>,
        c: Codepoint,
        style: FaceStyle,
        p: Presentation,
    ) -> FaceIx {
        let v = c.value();
        let blk = Block::of(c).0;
        let at = FaceTable::combo(style, p).0 * FaceTable::BLOCKS + blk;
        if self.blocks[at] == FaceTable::UNFILLED {
            stack.fill(self, Block(blk));
        }
        let ix = self.rows[usize::from(self.blocks[at])][usize::try_from(v & 0xff).unwrap_or(0)];
        if ix == FaceTable::NOBODY {
            return self.missed(c);
        }
        FaceIx(ix)
    }

    /// A character no face draws, met: listed the first time, and drawn as
    /// the primary's `.notdef`. Out of line, so a cell some face draws pays
    /// one compare.
    #[cold]
    #[inline(never)]
    fn missed(&mut self, c: Codepoint) -> FaceIx {
        if self.met.is_empty() {
            self.met = vec![0; 0x11_0000 / 64];
        }
        let v = usize::try_from(c.value()).unwrap_or(0);
        if let Some(word) = self.met.get_mut(v / 64) {
            let bit = 1u64 << (v % 64);
            if *word & bit == 0 {
                *word |= bit;
                self.gaps.push(c);
            }
        }
        FaceIx::PRIMARY
    }
}

/// What the table holds, for a test or a report to print: the bytes of the
/// block index and the distinct rows, the rows, and how many blocks have been
/// decided on demand since it was made.
impl core::fmt::Display for FaceTable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} KB, {} distinct rows, {} blocks filled on demand",
            (self.blocks.len() * 2 + self.rows.len() * 256) / 1024,
            self.rows.len(),
            self.filled.0
        )
    }
}

/// A block of 256 codepoints: the table's first stage.
#[derive(Clone, Copy)]
struct Block(usize);

impl Block {
    fn of(c: Codepoint) -> Block {
        Block(usize::try_from(c.value() >> 8).unwrap_or(0))
    }
}

/// A style and presentation's run in the block index.
#[derive(Clone, Copy)]
struct Combo(usize);

/// A distinct row's index.
#[derive(Clone, Copy)]
struct RowId(u16);

/// Blocks decided on demand.
#[derive(Clone, Copy)]
struct Filled(usize);

/// **The `wdth` values a face's narrower instances are read at**
/// ([`FaceRole::Narrower`]): from the width it is read at -- `settings`'
/// `wdth`, else its axis's default -- down to its axis's least or 62.5,
/// whichever is wider, in steps of 2.5, widest first, each a width Noto
/// Sans Devanagari's axis reads between its named instances. None for a
/// face with no `wdth` axis, one read at its narrowest already, or one
/// that draws no Devanagari -- the one script whose words are narrowed --
/// so a stack of other variable faces takes no instance.
pub fn narrower_widths(face: &Face<'_>, settings: &[baud_font::var::Variation]) -> Vec<f32> {
    const STEP: f32 = 2.5;
    const LEAST: f32 = 62.5;
    // DEVANAGARI LETTER KA.
    if face.glyph(Codepoint::from_char('\u{915}')).is_none() {
        return Vec::new();
    }
    let wdth = baud_font::Tag::from_bytes(*b"wdth");
    let mut i = baud_font::bytes::Index::ZERO;
    let axis = loop {
        match face.axis(i) {
            Some(a) if a.tag == wdth => break a,
            Some(_) => i = i.next(),
            None => return Vec::new(),
        }
    };
    let now = settings
        .iter()
        .find(|v| v.tag == wdth)
        .map_or(axis.default, |v| v.value)
        .to_f32();
    let floor = axis.min.to_f32().max(LEAST);
    let mut out = Vec::new();
    let mut w = now - STEP;
    while w >= floor - 1e-3 {
        out.push(w);
        w -= STEP;
    }
    out
}

/// What a face is for in a [`Stack`]: part of the fallback chain every
/// character tries in order, the face one style tries first, the face iTerm2's
/// badge is drawn in, whatever face the text is ([`Stack::badge`]), one
/// style of an alternative font or of the Fraktur face ([`FontSlot`]), or
/// a narrower instance of a variable face ([`Stack::narrower`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceRole {
    Fallback,
    Style(FaceStyle),
    Badge,
    Font(FontSlot, FaceStyle),
    /// **The face before it, read narrower on its `wdth` axis**: the nearest
    /// face before it that is not itself narrower, at a narrower width, the
    /// instances listed widest first. Never in the chain: a Devanagari word
    /// too wide for its columns is drawn at the widest of them that fits
    /// (`crate::word`), at full height, never scaled.
    Narrower,
}

/// An ordered list of faces: the primary first, then the fallbacks tried
/// per cluster in order, and beside them at most one face per style.
/// Discovery is the caller's; this crate only draws.
pub struct Stack<'f, 'a> {
    faces: &'f [Face<'a>],
    /// The Bold, Italic and Bold Italic faces, where the caller has them,
    /// and the badge's. Neither is ever in the fallback chain: a character
    /// the regular faces lack is not looked for in a bold one.
    styled: Styled,
    /// The alternative fonts' and the Fraktur face's styles, each a font
    /// alone ([`FontSlot`]), a style and its face: kept out of the chain as
    /// the styled faces are.
    fonts: Vec<(FontSlot, FaceStyle, FaceIx)>,
    /// **Each face's base, by its place, where it is a narrower instance**
    /// ([`FaceRole::Narrower`]): the nearest face before it that is not
    /// itself one, so a base's instances are the faces just after it, in the
    /// order given, widest first. Kept out of the chain; built with the
    /// stack, so a face's base is one load, and its instances
    /// ([`Stack::narrower`]) a range of places, `instances` long.
    narrows: Vec<Option<FaceIx>>,
    /// How many narrower instances follow each face, by its place: none for
    /// a face the caller gave none, and for an instance.
    instances: Vec<u8>,
    /// **The faces that answer for what the stack draws**, in its order:
    /// every face but a narrower instance, which draws what its base draws
    /// ([`Stack::answering`]). Built with the stack, so a search over them
    /// does not pass the default stack's 60 instances.
    answers: Vec<FaceIx>,
    /// Each face's role, by its place: what [`Stack::is_styled`] reads, one
    /// load a face where it scanned the narrower instances, which the
    /// default stack holds 60 of.
    roles: Vec<FaceRole>,
    /// **The fallback chain, in order**: every face whose role is
    /// [`FaceRole::Fallback`], the styled faces, the fonts' and the narrower
    /// instances left out. A search of the chain walks it alone: the face
    /// table asks it, for each character of a block it decides, which
    /// faces draw the character in colour, and walking the default stack's
    /// 60 narrower instances too cost a launch's first frame 0.8 million
    /// instructions.
    chain: Vec<FaceIx>,
}

/// One optional face per style; regular has none, being the chain. And
/// the badge's face, kept out of the chain the same way.
#[derive(Clone, Copy, Default)]
struct Styled {
    bold: Option<FaceIx>,
    italic: Option<FaceIx>,
    bold_italic: Option<FaceIx>,
    badge: Option<FaceIx>,
}

impl Styled {
    fn get(&self, style: FaceStyle) -> Option<FaceIx> {
        match style {
            FaceStyle::Regular => None,
            FaceStyle::Bold => self.bold,
            FaceStyle::Italic => self.italic,
            FaceStyle::BoldItalic => self.bold_italic,
        }
    }

    fn slot(&mut self, style: FaceStyle) -> Option<&mut Option<FaceIx>> {
        match style {
            FaceStyle::Regular => None,
            FaceStyle::Bold => Some(&mut self.bold),
            FaceStyle::Italic => Some(&mut self.italic),
            FaceStyle::BoldItalic => Some(&mut self.bold_italic),
        }
    }
}

impl<'f, 'a> Stack<'f, 'a> {
    /// `None` for an empty list or one longer than a byte can index.
    pub fn new(faces: &'f [Face<'a>]) -> Option<Stack<'f, 'a>> {
        if faces.is_empty() || faces.len() > usize::from(u8::MAX) {
            return None;
        }
        let mut stack = Stack {
            faces,
            styled: Styled::default(),
            fonts: Vec::new(),
            narrows: vec![None; faces.len()],
            instances: vec![0; faces.len()],
            answers: Vec::new(),
            roles: vec![FaceRole::Fallback; faces.len()],
            chain: Vec::new(),
        };
        stack.chain = stack.each().collect();
        stack.answers = stack.each().collect();
        Some(stack)
    }

    /// A stack whose faces each carry a role. `None` when the lengths
    /// differ, the primary is not a fallback face, a style, the badge or a
    /// font's style is named twice, a face is given the regular style,
    /// which is the chain itself, or a font role names the primary or an
    /// alternative and Fraktur at once.
    pub fn with_roles(faces: &'f [Face<'a>], roles: &[FaceRole]) -> Option<Stack<'f, 'a>> {
        if roles.len() != faces.len() || roles.first() != Some(&FaceRole::Fallback) {
            return None;
        }
        let mut stack = Stack::new(faces)?;
        // The nearest face so far that is not a narrower instance: the base
        // of each instance after it.
        let mut base = FaceIx::PRIMARY;
        for (i, role) in roles.iter().enumerate() {
            let ix = FaceIx(u8::try_from(i).ok()?);
            if *role != FaceRole::Narrower {
                base = ix;
            }
            let slot = match role {
                FaceRole::Fallback => continue,
                FaceRole::Narrower => {
                    stack.narrows[i] = Some(base);
                    stack.instances[base.index()] += 1;
                    continue;
                }
                FaceRole::Style(style) => stack.styled.slot(*style)?,
                FaceRole::Badge => &mut stack.styled.badge,
                FaceRole::Font(font, style) => {
                    let named = stack.fonts.iter().any(|f| (f.0, f.1) == (*font, *style));
                    if !font.is_one() || named {
                        return None;
                    }
                    stack.fonts.push((*font, *style, ix));
                    continue;
                }
            };
            if slot.is_some() {
                return None;
            }
            *slot = Some(ix);
        }
        stack.roles = roles.to_vec();
        let chain = stack.each().filter(|ix| !stack.is_styled(*ix)).collect();
        stack.chain = chain;
        let answers = stack
            .each()
            .filter(|ix| stack.narrows(*ix).is_none())
            .collect();
        stack.answers = answers;
        Some(stack)
    }

    /// **The face iTerm2's badge is drawn in**: the one given
    /// [`FaceRole::Badge`], which baud-app gives its embedded JetBrains Mono
    /// Bold whatever font the text is in, as iTerm2's badge has a face of
    /// its own (`badgeFont:`, `[[source:
    /// iterm2-964a75d-profile-preferences]]`); else the bold face, else the
    /// primary.
    pub fn badge(&self) -> FaceIx {
        self.styled
            .badge
            .or(self.styled.get(self.effective(FaceStyle::Bold)))
            .unwrap_or(FaceIx::PRIMARY)
    }

    /// Whether the badge's face draws `c`.
    pub fn badge_carries(&self, c: Codepoint) -> bool {
        self.carries(self.badge(), c) != Draws::No
    }

    /// The style whose face a cell of `style` is drawn from: its own, when
    /// the stack has one, else regular -- the fallback chain -- and nothing
    /// is synthesised. Which face a style has is the caller's: baud-app
    /// gives a configured family's styles by CSS Fonts' style search, which
    /// can give Bold Italic the Bold face of a family with no italic.
    pub fn effective(&self, style: FaceStyle) -> FaceStyle {
        if self.styled.get(style).is_some() {
            style
        } else {
            FaceStyle::Regular
        }
    }

    /// Whether `ix` is a styled face, the badge's, a font's or a narrower
    /// instance rather than one of the chain: any role but
    /// [`FaceRole::Fallback`].
    pub fn is_styled(&self, ix: FaceIx) -> bool {
        self.roles
            .get(ix.index())
            .is_some_and(|r| *r != FaceRole::Fallback)
    }

    /// **The face `ix` is a narrower instance of** ([`FaceRole::Narrower`]):
    /// the nearest face before it that is not itself narrower; `None` for
    /// any other face.
    pub fn narrows(&self, ix: FaceIx) -> Option<FaceIx> {
        self.narrows.get(ix.index()).copied().flatten()
    }

    /// **The narrower instances of face `of`**, widest first
    /// ([`FaceRole::Narrower`]): none for a face the caller gave none. The
    /// places just after it, as many as it has: no search.
    pub fn narrower(&self, of: FaceIx) -> impl Iterator<Item = FaceIx> + '_ {
        let n = self.instances.get(of.index()).copied().unwrap_or(0);
        (1..=n).filter_map(move |k| of.0.checked_add(k).map(FaceIx))
    }

    /// Whether the stack has a face for `font`, an alternative alone or
    /// Fraktur alone.
    pub fn has_font(&self, font: FontSlot) -> bool {
        self.fonts.iter().any(|f| f.0 == font)
    }

    /// **The font a rendition's text is drawn in**: its alternative
    /// (`SGR 11`-`19`) where the stack has a face for it, and the Fraktur
    /// face over that where `SGR 20` is set and the stack has one; the
    /// primary for what it has no face for, so an unconfigured selection
    /// draws as `SGR 10` does. Fraktur is first and the alternative behind
    /// it when both are set: 20 names a look the text defines, 11-19 a
    /// slot it leaves undefined.
    pub fn font_of(&self, id: FontId, attrs: Attrs) -> FontSlot {
        let alt = FontSlot::alternative(id);
        let alt = match alt != FontSlot::PRIMARY && self.has_font(alt) {
            true => alt,
            false => FontSlot::PRIMARY,
        };
        match attrs.has(Attrs::FRAKTUR) && self.has_font(FontSlot::FRAKTUR) {
            true => alt.with_fraktur(),
            false => alt,
        }
    }

    /// The face `font` (one alone) draws `style` from: its own face for the
    /// style, else its regular face -- a style the font's family has no
    /// face for was left out by the caller, which picks the nearest by CSS
    /// Fonts' search, and is never synthesised.
    fn font_face(&self, font: FontSlot, style: FaceStyle) -> Option<FaceIx> {
        let find = |s: FaceStyle| {
            self.fonts
                .iter()
                .find(|f| (f.0, f.1) == (font, s))
                .map(|f| f.2)
        };
        find(style).or_else(|| find(FaceStyle::Regular))
    }

    /// **The face a character in an alternative font or under Fraktur is
    /// drawn from**, given `chain`, the face the primary would draw it from
    /// ([`Stack::select_presented`]): the font's own faces first -- Fraktur's,
    /// then the alternative's -- and then the chain, configured before
    /// embedded before system, as for every other face. The presentation
    /// is honoured in [`Stack::select_presented`]'s order: a face that
    /// draws `c` as asked, the font's before the chain's, then one that
    /// draws it at all.
    pub fn select_font(
        &self,
        c: Codepoint,
        face: TextFace,
        p: Presentation,
        chain: FaceIx,
    ) -> FaceIx {
        let want = match p {
            Presentation::Text => Draws::Text,
            Presentation::Colour => Draws::Colour,
        };
        let fraktur = face
            .font
            .has_fraktur()
            .then(|| self.font_face(FontSlot::FRAKTUR, face.style))
            .flatten();
        let alt = face.font.alternative_only();
        let alt = (alt != FontSlot::PRIMARY)
            .then(|| self.font_face(alt, face.style))
            .flatten();
        let own = [fraktur, alt];
        let draws = |ix: FaceIx| self.carries(ix, c);
        own.into_iter()
            .flatten()
            .find(|ix| draws(*ix) == want)
            .or_else(|| (draws(chain) == want).then_some(chain))
            .or_else(|| own.into_iter().flatten().find(|ix| draws(*ix) != Draws::No))
            .unwrap_or(chain)
    }

    /// **How far `face`'s italic leans**, for a cell SAPV 17 slants left,
    /// drawn as its upright glyph sheared by it (sheared left by the italic
    /// face's angle): the italic face's own `post.italicAngle`,
    /// counter-clockwise degrees from the vertical and negative for a face
    /// that leans right (`[[source: ms-opentype-post]]`), as far to the left;
    /// CSS's 14 degrees for an `oblique` given no angle where it gives none
    /// (`[[source: css-fonts-4-wd-2026-10-16]]`), as
    /// `baud_font::Described::slope` reads it. `None` for a face not italic,
    /// and for an italic the stack has no face for: that is drawn upright
    /// from the regular face and nothing is synthesised for it
    /// ([`Stack::effective`]), so it leans neither way.
    pub fn lean(&self, face: TextFace) -> Option<Lean> {
        if face.style.upright() == face.style {
            return None;
        }
        let own = |font: FontSlot| {
            self.fonts
                .iter()
                .find(|f| (f.0, f.1) == (font, face.style))
                .map(|f| f.2)
        };
        let ix = match face.font {
            FontSlot::PRIMARY => self.styled.get(face.style),
            f if f.has_fraktur() => own(FontSlot::FRAKTUR),
            f => own(f.alternative_only()),
        }?;
        let angle = self
            .face(ix)
            .table(baud_font::Tag::POST)
            .and_then(|t| t.read::<baud_font::var::Fixed>(baud_font::bytes::Offset::of(4u32)));
        let degrees = match angle {
            Some(a) if a < baud_font::var::Fixed::ZERO => -a.to_f32(),
            _ => baud_font::Angle::OBLIQUE.get(),
        };
        Some(Lean(degrees.to_radians().tan()))
    }

    pub fn primary(&self) -> &'f Face<'a> {
        &self.faces[0]
    }

    pub fn faces(&self) -> &'f [Face<'a>] {
        self.faces
    }

    /// Every face's index, in stack order.
    pub fn each(&self) -> impl Iterator<Item = FaceIx> + '_ {
        (0..self.faces.len())
            .filter_map(|i| u8::try_from(i).ok())
            .map(FaceIx)
    }

    pub fn face(&self, ix: FaceIx) -> &'f Face<'a> {
        self.faces.get(ix.index()).unwrap_or(&self.faces[0])
    }

    /// How face `ix` draws `c` ([`drawing`]), or [`Draws::No`] when its
    /// `cmap` does not map it. A face that draws it not at all is passed
    /// over as one that does not map it is, so the next face that carries
    /// the character draws it, else the primary's `.notdef`: TUS § 5.3
    /// (`[[source: unicode18-ch5]]`) leaves a character a face cannot
    /// render "some placeholder", never a blank.
    ///
    /// **A character with a canonical decomposition is carried taken
    /// apart, too**, by a face that lacks it and has its parts
    /// ([`Normalised::covered`]): the face its decomposition typed would
    /// take ([`Stack::select_cluster`]), since the two are canonical
    /// equivalents (`[[source: unicode18-ch3]]` C6), and in the chain's
    /// own order -- so Ǻ is the embedded face's Å and acute, in bold
    /// where the bold face has them, rather than Menlo's Ǻ two faces
    /// down.
    fn carries(&self, ix: FaceIx, c: Codepoint) -> Draws {
        let f = self.face(ix);
        match f.glyph(c) {
            Some(g) => drawing(f, g),
            None if baud_font::decompose(c).is_some() && Normalised::comes_apart(f, c) => {
                Draws::Text
            }
            None => Draws::No,
        }
    }

    /// `c`, or -- when it has a canonical decomposition and no face carries
    /// it whole or taken apart -- its first part all the way down, whose
    /// face then draws its `.notdef`, as that face would for its
    /// decomposition typed (`Stack::select_cluster` keeps the base's face).
    fn drawn_as(&self, c: Codepoint) -> Codepoint {
        if baud_font::decompose(c).is_none()
            || self
                .answers
                .iter()
                .any(|ix| self.carries(*ix, c) != Draws::No)
        {
            return c;
        }
        let mut base = c;
        while let Some((a, _)) = baud_font::decompose(base) {
            base = a;
        }
        base
    }

    /// The face a character of `style` is drawn from: the style's own
    /// face when it carries `c` ([`Stack::carries`]), else the fallback
    /// chain.
    pub fn select_styled(&self, c: Codepoint, style: FaceStyle) -> FaceIx {
        let own = self
            .styled
            .get(self.effective(style))
            .filter(|f| self.carries(*f, c) != Draws::No);
        own.unwrap_or_else(|| self.select(c))
    }

    /// The face a character of `style` is drawn from when it asks for a
    /// presentation: the first face that carries `c` ([`Stack::carries`])
    /// with a glyph that is colour, or is not, in the order
    /// [`Stack::select_styled`] tries them -- the style's own
    /// face, then the chain. When no face draws `c` that way, the face
    /// `select_styled` picks: a heart with `U+FE0F` and no colour face is the
    /// text heart, not `.notdef`.
    pub fn select_presented(&self, c: Codepoint, style: FaceStyle, p: Presentation) -> FaceIx {
        let c = self.drawn_as(c);
        let want = match p {
            Presentation::Text => Draws::Text,
            Presentation::Colour => Draws::Colour,
        };
        let wants = |ix: FaceIx| self.carries(ix, c) == want;
        let own = self.styled.get(self.effective(style)).filter(|f| wants(*f));
        own.or_else(|| self.chain.iter().copied().find(|ix| wants(*ix)))
            .unwrap_or_else(|| self.select_styled(c, style))
    }

    /// [`Stack::select_presented`] decided from what each face does with
    /// the character rather than by asking the faces: `draws[i]` is face
    /// `i`'s answer. The order is `select_presented`'s and then
    /// `select_styled`'s, spelled once more; [`FaceTable`] is built from this
    /// and held to `select_presented` at every codepoint by a test.
    fn choose(&self, draws: &[Draws], style: FaceStyle, p: Presentation) -> FaceIx {
        let want = match p {
            Presentation::Text => Draws::Text,
            Presentation::Colour => Draws::Colour,
        };
        let at = |ix: FaceIx| draws.get(ix.index()).copied().unwrap_or(Draws::No);
        let chain = |ok: &dyn Fn(Draws) -> bool| self.chain.iter().copied().find(|ix| ok(at(*ix)));
        let own = self.styled.get(self.effective(style));
        own.filter(|f| at(*f) == want)
            .or_else(|| chain(&|d| d == want))
            .or_else(|| own.filter(|f| at(*f) != Draws::No))
            .or_else(|| chain(&|d| d != Draws::No))
            .unwrap_or(FaceIx::PRIMARY)
    }

    /// The table [`Stack::select_presented`] becomes a lookup into. It
    /// starts with every block unfilled: each face's `cmap` but a narrower
    /// instance's proposes the blocks it could reach (`Face::candidates`,
    /// `Stack::answering`), and every block is
    /// decided the first time a cell lands in it ([`FaceTable::face`]), so
    /// a launch pays for the blocks its first frame touches and not for all
    /// of CJK. A character no face draws is entered as no face's, which
    /// the lookup turns into the primary's `.notdef` and lists
    /// ([`FaceTable::take_gaps`]); a block no face proposes and where
    /// nothing comes apart is all such characters.
    pub fn table(&self) -> FaceTable {
        let mut proposed: Vec<Vec<u8>> = vec![Vec::new(); FaceTable::BLOCKS];
        for (i, face) in self.answering() {
            let Ok(i) = u8::try_from(i) else { continue };
            face.candidates(|a, b| {
                for blk in Block::of(a).0..=Block::of(b).0 {
                    if let Some(list) = proposed.get_mut(blk) {
                        if list.last() != Some(&i) {
                            list.push(i);
                        }
                    }
                }
            });
        }
        // A block that holds a character with a canonical decomposition is
        // decided by `fill` whoever proposes it: a face may draw such a
        // character taken apart, or its first part's `.notdef`, without
        // mapping anything in its block (U+1F00 in bold, from the bold
        // face's alpha; U+2F800 from a CJK face's U+4E3D).
        let mut decomposing = vec![false; FaceTable::BLOCKS];
        baud_font::each_decomposable(|a, b| {
            for blk in (a >> 8)..=(b >> 8) {
                if let Some(d) = usize::try_from(blk)
                    .ok()
                    .and_then(|i| decomposing.get_mut(i))
                {
                    *d = true;
                }
            }
        });
        let mut t = FaceTable::empty(proposed);
        let row = t.intern([FaceTable::NOBODY; 256]);
        t.idle = [row.0; FaceTable::COMBOS];
        for (blk, decomposes) in decomposing.into_iter().enumerate() {
            if t.proposed[blk].is_empty() && !decomposes {
                for combo in 0..FaceTable::COMBOS {
                    t.blocks[combo * FaceTable::BLOCKS + blk] = t.idle[combo];
                }
            }
        }
        t
    }

    /// **The faces a block is proposed by and its entries asked of**, by
    /// place: every face but a narrower instance, which is never chosen,
    /// being out of the chain, and draws what the face it narrows draws, so
    /// its answers decide nothing -- and the default stack holds 60.
    fn answering(&self) -> impl Iterator<Item = (usize, &Face<'a>)> + '_ {
        self.answers.iter().map(|ix| (ix.index(), self.face(*ix)))
    }

    /// Decide every entry of block `blk` of `t`, for all eight styles and
    /// presentations at once: one `cmap` lookup and [`drawing`] per face that
    /// proposed the block, per codepoint. A face that maps a character to a
    /// glyph it draws not at all is counted in [`FaceTable::refused`] when
    /// no style or presentation keeps it.
    fn fill(&self, t: &mut FaceTable, blk: Block) {
        let blk = blk.0;
        let faces = core::mem::take(&mut t.proposed[blk]);
        let base = u32::try_from(blk).unwrap_or(0) * 256;
        let mut draws = vec![Draws::No; self.faces.len()];
        let mut refused = vec![false; self.faces.len()];
        let mut rows = [[0u8; 256]; FaceTable::COMBOS];
        // Each face's answers for the parts and first parts of the block's
        // precomposed characters, asked once each.
        let mut parts: crate::seeded::Map<(usize, u32), bool> = crate::seeded::Map::default();
        let mut leaves: crate::seeded::Map<(usize, u32), Draws> = crate::seeded::Map::default();
        for lo in 0..256u32 {
            let v = base + lo;
            let cp = Codepoint::new(v);
            for &i in &faces {
                let f = &self.faces[usize::from(i)];
                let (draw, gone) = match cp.and_then(|c| f.glyph(c)) {
                    None => (Draws::No, false),
                    Some(g) => {
                        let d = drawing(f, g);
                        (d, d == Draws::No)
                    }
                };
                draws[usize::from(i)] = draw;
                refused[usize::from(i)] = gone;
            }
            let at = usize::try_from(lo).unwrap_or(0);
            // A character with a canonical decomposition is decided by
            // every face, as `Stack::carries` and `Stack::drawn_as` decide
            // it: any face may draw it taken apart, whether or not its
            // `cmap` reaches the block.
            let decomposed = cp.filter(|&c| baud_font::decompose(c).is_some()).map(|c| {
                // `draws` and `refused` say what the proposing faces map; a
                // face that does not propose the block maps nothing in it.
                // The rest of `Stack::carries` is the parts, asked of every
                // face through the block's answers.
                let mut d = draws.clone();
                for (i, f) in self.answering() {
                    let mapped = d[i] != Draws::No || refused[i];
                    let mut has = |x: Codepoint| match parts.entry((i, x.value())) {
                        std::collections::hash_map::Entry::Occupied(e) => *e.get(),
                        std::collections::hash_map::Entry::Vacant(e) => {
                            *e.insert(f.glyph(x).is_some())
                        }
                    };
                    if !mapped && Normalised::comes_apart_by(c, &mut has) {
                        d[i] = Draws::Text;
                    }
                }
                if d.iter().all(|&x| x == Draws::No) {
                    // `Stack::drawn_as`: the first part's faces.
                    let mut base = c;
                    while let Some((a, _)) = baud_font::decompose(base) {
                        base = a;
                    }
                    for (i, f) in self.answering() {
                        d[i] = *leaves
                            .entry((i, base.value()))
                            .or_insert_with(|| f.glyph(base).map_or(Draws::No, |g| drawing(f, g)));
                    }
                }
                d
            });
            let chosen = decomposed.as_deref().unwrap_or(&draws);
            let nobody = chosen.iter().all(|&d| d == Draws::No);
            for (combo, (style, p)) in FaceTable::combos().enumerate() {
                rows[combo][at] = match nobody {
                    true => FaceTable::NOBODY,
                    false => self.choose(chosen, style, p).0,
                };
            }
            for &i in &faces {
                // No face's entry is drawn by the primary, which keeps it.
                let keeps = |e: u8| e == i || (e == FaceTable::NOBODY && i == FaceIx::PRIMARY.0);
                if refused[usize::from(i)] && rows.iter().all(|row| !keeps(row[at])) {
                    t.refused.bump();
                }
            }
        }
        for (combo, row) in rows.into_iter().enumerate() {
            let id = t.intern(row);
            t.blocks[combo * FaceTable::BLOCKS + blk] = id.0;
        }
        t.filled.0 += 1;
    }

    /// The face a cluster with marks is drawn from, given `first`, the face
    /// its base alone is drawn from ([`Stack::select_presented`], its style
    /// and presentation honoured): the first face, `first` and then the
    /// fallback chain in order, that has the base and every mark that is not
    /// default-ignorable after the shaper's own normalisation
    /// ([`Normalised::covered`]); `first` when none has, which then draws the
    /// marks it has and drops the rest (`Renderer::shape`). Before this a
    /// mark the base's face lacked drew as a `.notdef` that JetBrains Mono
    /// advances a full cell, so every mark after it landed over the box in
    /// the next cell.
    ///
    /// **A styled face is not in the chain**, here as everywhere: a bold
    /// cluster whose bold face lacks a mark is looked for in the regular
    /// faces like any other the bold face cannot draw, and drawn regular if
    /// one of them has it all. Ghostty does the same; the alternative, a
    /// bold letter whose mark is dropped, loses the mark to keep the weight.
    pub fn select_cluster(&self, cps: &[Codepoint], first: FaceIx) -> FaceIx {
        let covers = |ix: FaceIx| Normalised::of(self.face(ix), cps).covered();
        if covers(first) {
            return first;
        }
        self.chain
            .iter()
            .copied()
            .filter(|ix| *ix != first)
            .find(|ix| covers(*ix))
            .unwrap_or(first)
    }

    /// The first face that carries `c` ([`Stack::carries`]: maps it to a
    /// glyph it draws, as colour or as text); the primary when none does,
    /// so its `.notdef` is what a character nobody carries draws as -- and
    /// what a glyph the primary cannot draw draws as, the atlas drawing the
    /// `.notdef` in its place (`Atlas::slot`).
    pub fn select(&self, c: Codepoint) -> FaceIx {
        self.chain
            .iter()
            .copied()
            .find(|ix| self.carries(*ix, c) != Draws::No)
            .unwrap_or(FaceIx::PRIMARY)
    }
}

/// How `f` draws `g`, as the atlas would (`Atlas::slot`). As colour when
/// a strike holds an image for it, or its `COLR` layers all draw; a
/// refused outline does not stop either. As text when its outline draws
/// and it has no `COLR` layers or has one that cannot draw. Not at all
/// when its outline cannot draw -- the face refuses it, or it needs more
/// scratch than the atlas grows to, [`VAR_POINTS_MAX`]
/// ([`Face::refuses_past`]) -- or when it is a
/// `COLR` glyph with a layer that cannot, which the atlas refuses whole
/// rather than paint partly, and whose own outline cannot draw either
/// (refused or past the budget) or is, by its `glyf` header, empty. One
/// whose own outline has data is text: the atlas draws that outline, as
/// the `COLR` text allows ("If the base glyph ID has an outline, that may
/// be rendered as a non-color glyph instead",
/// `[[source: ms-opentype-colr]]`). The layers are looked up first,
/// so a glyph with none pays the strike walk and the outline test it paid
/// before a budget was tested.
fn drawing(f: &Face<'_>, g: GlyphId) -> Draws {
    let budget = Count::of(VAR_POINTS_MAX);
    match f.layers(g) {
        Some(layers) if layers.into_iter().all(|l| !f.refuses_past(l.glyph, budget)) => {
            Draws::Colour
        }
        _ if f.has_bitmap(g) => Draws::Colour,
        _ if f.refuses_past(g, budget) => Draws::No,
        Some(_) if !f.has_outline(g) => Draws::No,
        _ => Draws::Text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(path: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/../baud-app/fonts/{path}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    fn cp(c: char) -> Codepoint {
        Codepoint::new(u32::from(c)).unwrap()
    }

    const FOLDER: u32 = 0xF07B;

    /// The default stack's shape, on stock JetBrains Mono: Regular, the icon
    /// face, then the three styles. (The embedded Regular and Bold are these
    /// files with Hebrew added, which nothing here draws.) Text of a style
    /// takes its own face; a character that face lacks -- an icon -- takes
    /// the fallback chain, never another style's face; and regular text never
    /// lands in a styled face.
    #[test]
    fn a_style_draws_from_its_own_face_and_falls_back_to_the_chain() {
        let files = [
            font("jetbrains-mono/JetBrainsMono-Regular.ttf"),
            font("symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
            font("jetbrains-mono/JetBrainsMono-Bold.ttf"),
            font("jetbrains-mono/JetBrainsMono-Italic.ttf"),
            font("jetbrains-mono/JetBrainsMono-BoldItalic.ttf"),
        ];
        let faces: Vec<Face<'_>> = files.iter().map(|d| Face::parse(d).unwrap()).collect();
        let roles = [
            FaceRole::Fallback,
            FaceRole::Fallback,
            FaceRole::Style(FaceStyle::Bold),
            FaceRole::Style(FaceStyle::Italic),
            FaceRole::Style(FaceStyle::BoldItalic),
        ];
        let s = Stack::with_roles(&faces, &roles).unwrap();
        let folder = Codepoint::new(FOLDER).unwrap();
        assert!(
            faces[2].glyph(folder).is_none(),
            "the control: no icon in Bold"
        );
        for (style, face) in [
            (FaceStyle::Regular, 0),
            (FaceStyle::Bold, 2),
            (FaceStyle::Italic, 3),
            (FaceStyle::BoldItalic, 4),
        ] {
            assert_eq!(s.effective(style), style);
            assert_eq!(s.select_styled(cp('a'), style).index(), face, "{style:?}");
            assert_eq!(
                s.select_styled(folder, style).index(),
                1,
                "{style:?}: the icon face"
            );
        }
        assert_eq!(s.select(cp('a')).index(), 0);
        // A character nothing carries draws the primary's .notdef, whatever
        // the style: a styled face is not the primary.
        let none = Codepoint::new(0x10FFFD).unwrap();
        assert_eq!(s.select_styled(none, FaceStyle::Bold).index(), 0);
    }

    /// A style with no face of its own draws from the fallback chain -- the
    /// regular faces -- and never from another style's face: Bold Italic with
    /// only a Bold face present is drawn regular, not bold (kitty's rule).
    #[test]
    fn a_missing_style_falls_back_without_faking() {
        let r = font("jetbrains-mono/JetBrainsMono-Regular.ttf");
        let b = font("jetbrains-mono/JetBrainsMono-Bold.ttf");
        let i = font("jetbrains-mono/JetBrainsMono-Italic.ttf");
        let faces = [
            Face::parse(&r).unwrap(),
            Face::parse(&b).unwrap(),
            Face::parse(&i).unwrap(),
        ];
        let bold = FaceRole::Style(FaceStyle::Bold);
        let italic = FaceRole::Style(FaceStyle::Italic);
        let s = Stack::with_roles(&faces[..2], &[FaceRole::Fallback, bold]).unwrap();
        assert_eq!(s.effective(FaceStyle::Bold), FaceStyle::Bold, "the control");
        assert_eq!(s.effective(FaceStyle::BoldItalic), FaceStyle::Regular);
        assert_eq!(s.effective(FaceStyle::Italic), FaceStyle::Regular);
        assert_eq!(s.select_styled(cp('a'), FaceStyle::BoldItalic).index(), 0);
        assert_eq!(s.select_styled(cp('a'), FaceStyle::Italic).index(), 0);
        let s =
            Stack::with_roles(&faces, &[FaceRole::Fallback, FaceRole::Fallback, italic]).unwrap();
        assert_eq!(s.effective(FaceStyle::BoldItalic), FaceStyle::Regular);
        assert_eq!(s.effective(FaceStyle::Bold), FaceStyle::Regular);
        assert_eq!(s.select_styled(cp('a'), FaceStyle::BoldItalic).index(), 0);
        let s = Stack::new(&faces[..1]).unwrap();
        for style in FaceStyle::ALL {
            assert_eq!(s.effective(style), FaceStyle::Regular);
        }
    }

    /// A styled face is not in the fallback chain: a character only the
    /// Bold face carries is not drawn from it for regular text.
    #[test]
    fn a_styled_face_is_never_a_fallback() {
        let sym = font("symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf");
        let b = font("jetbrains-mono/JetBrainsMono-Bold.ttf");
        let faces = [Face::parse(&sym).unwrap(), Face::parse(&b).unwrap()];
        assert!(
            faces[0].glyph(cp('a')).is_none(),
            "the control: no 'a' in the icon face"
        );
        let s = Stack::with_roles(
            &faces,
            &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
        )
        .unwrap();
        assert_eq!(s.select(cp('a')).index(), 0);
        assert_eq!(s.select_styled(cp('a'), FaceStyle::Italic).index(), 0);
        assert_eq!(s.select_styled(cp('a'), FaceStyle::Bold).index(), 1);
    }

    /// **The badge's face is not in the fallback chain nor any style's**:
    /// a character only it carries is not drawn from it for the text,
    /// regular or bold, and its table agrees; [`Stack::badge`] names it,
    /// and without it names the bold face, else the primary.
    #[test]
    fn the_badge_face_is_never_the_texts() {
        let sym = font("symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf");
        let b = font("jetbrains-mono/JetBrainsMono-Bold.ttf");
        let faces = [Face::parse(&sym).unwrap(), Face::parse(&b).unwrap()];
        assert!(
            faces[0].glyph(cp('a')).is_none() && faces[1].glyph(cp('a')).is_some(),
            "the control: 'a' in the badge's face alone"
        );
        let s = Stack::with_roles(&faces, &[FaceRole::Fallback, FaceRole::Badge]).unwrap();
        assert_eq!(s.badge().index(), 1);
        assert!(s.badge_carries(cp('a')));
        assert_eq!(s.select(cp('a')).index(), 0);
        let mut table = s.table();
        for style in FaceStyle::ALL {
            assert_eq!(s.select_styled(cp('a'), style).index(), 0, "{style:?}");
            let p = Presentation::Text;
            assert_eq!(
                s.select_presented(cp('a'), style, p).index(),
                0,
                "{style:?}"
            );
            assert_eq!(table.face(&s, cp('a'), style, p).index(), 0, "{style:?}");
        }
        let bold = Stack::with_roles(
            &faces,
            &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
        )
        .unwrap();
        assert_eq!(bold.badge().index(), 1, "no badge face: the bold one");
        assert_eq!(
            Stack::new(&faces).unwrap().badge().index(),
            0,
            "else the primary"
        );
        let three = [
            Face::parse(&b).unwrap(),
            Face::parse(&b).unwrap(),
            Face::parse(&b).unwrap(),
        ];
        let f = FaceRole::Fallback;
        assert!(
            Stack::with_roles(&three, &[f, FaceRole::Badge, FaceRole::Badge]).is_none(),
            "the badge twice"
        );
        assert!(
            Stack::with_roles(&three[..2], &[FaceRole::Badge, f]).is_none(),
            "a badge primary"
        );
    }

    /// Roles that cannot describe a stack are refused.
    #[test]
    fn roles_that_cannot_describe_a_stack_are_refused() {
        let r = font("jetbrains-mono/JetBrainsMono-Regular.ttf");
        let faces = [Face::parse(&r).unwrap(), Face::parse(&r).unwrap()];
        let bold = FaceRole::Style(FaceStyle::Bold);
        let f = FaceRole::Fallback;
        assert!(
            Stack::with_roles(&faces, &[f, bold]).is_some(),
            "the control"
        );
        assert!(Stack::with_roles(&faces, &[f]).is_none(), "lengths differ");
        assert!(
            Stack::with_roles(&faces, &[bold, f]).is_none(),
            "a styled primary"
        );
        assert!(Stack::with_roles(&faces, &[f, FaceRole::Style(FaceStyle::Regular)]).is_none());
        let three = [
            Face::parse(&r).unwrap(),
            Face::parse(&r).unwrap(),
            Face::parse(&r).unwrap(),
        ];
        assert!(
            Stack::with_roles(&three, &[f, bold, bold]).is_none(),
            "bold twice"
        );
    }

    /// **A narrower instance proposes no block, answers no entry and is
    /// out of the chain**: the built Regular and its 15 instances, as the
    /// app's default stack lays them, propose what the Regular alone
    /// proposes, and their table names the face the Regular's alone names
    /// for every character of Basic Latin to Devanagari, Latin Extended
    /// Additional, whose characters come apart, and the box drawings, in
    /// every style and presentation, refusing as many. Each instance is
    /// styled -- out of the chain, which is the Regular alone -- and
    /// narrows the Regular.
    #[test]
    fn a_narrower_instance_proposes_nothing() {
        let data = font("built/Regular.ttf");
        let first = Face::parse(&data).unwrap();
        let widths = narrower_widths(&first, &[]);
        assert_eq!(widths.len(), 15, "the premise: wdth 97.5 down to 62.5");
        let mut faces = vec![first];
        let mut roles = vec![FaceRole::Fallback];
        for w in &widths {
            let mut f = Face::parse(&data).unwrap();
            f.set_variations(&[baud_font::var::Variation {
                tag: baud_font::Tag::from_bytes(*b"wdth"),
                value: baud_font::var::Fixed::from_f32(*w),
            }]);
            faces.push(f);
            roles.push(FaceRole::Narrower);
        }
        let s = Stack::with_roles(&faces, &roles).unwrap();
        let alone = Stack::new(&faces[..1]).unwrap();
        let (mut t, mut a) = (s.table(), alone.table());
        assert!(
            t.proposed.iter().flatten().all(|&i| i == 0),
            "only the Regular proposes"
        );
        assert!(t.proposed == a.proposed, "what the Regular alone proposes");
        let ranges = [0x0000..0x0A00u32, 0x1E00..0x1F00, 0x2500..0x2600];
        let mut named = 0usize;
        for v in ranges.into_iter().flatten() {
            let Some(c) = Codepoint::new(v) else { continue };
            for (style, p) in FaceTable::combos() {
                assert_eq!(
                    t.face(&s, c, style, p),
                    a.face(&alone, c, style, p),
                    "U+{v:04X}"
                );
            }
            named += usize::from(faces[0].glyph(c).is_some());
        }
        assert!(
            named > 600,
            "the premise: the Regular draws {named} of them"
        );
        assert_eq!(t.refused().value(), a.refused().value(), "refused alike");
        assert!(!s.is_styled(FaceIx::PRIMARY));
        assert_eq!(s.chain, [FaceIx::PRIMARY], "the chain is the Regular");
        assert_eq!(s.narrows(FaceIx::PRIMARY), None);
        for ix in s.each().skip(1) {
            assert!(s.is_styled(ix), "{ix:?} is out of the chain");
            assert_eq!(s.narrows(ix), Some(FaceIx::PRIMARY), "{ix:?}");
        }
    }

    #[test]
    fn a_style_is_its_attrs_and_its_code_round_trips() {
        assert_eq!(FaceStyle::of(Attrs::default()), FaceStyle::Regular);
        assert_eq!(FaceStyle::of(Attrs::BOLD), FaceStyle::Bold);
        assert_eq!(FaceStyle::of(Attrs::ITALIC), FaceStyle::Italic);
        assert_eq!(
            FaceStyle::of(Attrs::BOLD.with(Attrs::ITALIC)),
            FaceStyle::BoldItalic
        );
        for s in FaceStyle::ALL {
            assert_eq!(FaceStyle::from_code(s.code()), s);
        }
    }
}
