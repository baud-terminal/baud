//! The faces a frame draws with, in fallback order.

use crate::atlas::Code;
use baud_domain::{Attrs, Codepoint};
use baud_font::Face;

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

/// How a character asks to be drawn: from a text face or a colour one.
/// `U+FE0E` asks for text and `U+FE0F` for colour; a character with neither
/// takes the renderer's default (`EmojiDefault`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Presentation {
    Text,
    Colour,
}

/// How a character with no selector is drawn: the owner's `font` or
/// `system` (2026-09-30). Under `Font`, the default, it is the font's text
/// glyph whatever its default presentation -- `⚡` is JetBrains Mono's bolt --
/// and only `U+FE0F` asks for colour. Under `System` a character whose
/// default presentation is emoji (`Emoji_Presentation=Yes`, `⚡` but not `❤`)
/// is colour too, as macOS's own text draws it.
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
    index: std::collections::HashMap<[u8; 256], u16>,
    /// Per block, the faces whose `cmap` could reach it; emptied when the
    /// block is filled.
    proposed: Vec<Vec<u8>>,
    /// The row every combination takes where no face reaches.
    idle: [u16; FaceTable::COMBOS],
    /// How many blocks have been filled on demand.
    filled: Filled,
}

impl FaceTable {
    const BLOCKS: usize = 0x1100;
    const COMBOS: usize = 8;

    const UNFILLED: u16 = u16::MAX;

    fn empty(proposed: Vec<Vec<u8>>) -> FaceTable {
        FaceTable {
            blocks: vec![FaceTable::UNFILLED; FaceTable::COMBOS * FaceTable::BLOCKS],
            rows: Vec::new(),
            index: std::collections::HashMap::new(),
            proposed,
            idle: [0; FaceTable::COMBOS],
            filled: Filled(0),
        }
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
        FaceIx(self.rows[usize::from(self.blocks[at])][usize::try_from(v & 0xff).unwrap_or(0)])
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

/// What a face is for in a [`Stack`]: part of the fallback chain every
/// character tries in order, or the face one style tries first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceRole {
    Fallback,
    Style(FaceStyle),
}

/// An ordered list of faces: the primary first, then the fallbacks tried
/// per cluster in order, and beside them at most one face per style.
/// Discovery is the caller's; this crate only draws.
pub struct Stack<'f, 'a> {
    faces: &'f [Face<'a>],
    /// The Bold, Italic and Bold Italic faces, where the caller has them.
    /// A styled face is never in the fallback chain: a character the
    /// regular faces lack is not looked for in a bold one.
    styled: Styled,
}

/// One optional face per style; regular has none, being the chain.
#[derive(Clone, Copy, Default)]
struct Styled {
    bold: Option<FaceIx>,
    italic: Option<FaceIx>,
    bold_italic: Option<FaceIx>,
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

    fn holds(&self, ix: FaceIx) -> bool {
        [self.bold, self.italic, self.bold_italic].contains(&Some(ix))
    }
}

impl<'f, 'a> Stack<'f, 'a> {
    /// `None` for an empty list or one longer than a byte can index.
    pub fn new(faces: &'f [Face<'a>]) -> Option<Stack<'f, 'a>> {
        (!faces.is_empty() && faces.len() <= usize::from(u8::MAX)).then_some(Stack {
            faces,
            styled: Styled::default(),
        })
    }

    /// A stack whose faces each carry a role. `None` when the lengths
    /// differ, the primary is not a fallback face, a style is named twice,
    /// or a face is given the regular style, which is the chain itself.
    pub fn with_roles(faces: &'f [Face<'a>], roles: &[FaceRole]) -> Option<Stack<'f, 'a>> {
        if roles.len() != faces.len() || roles.first() != Some(&FaceRole::Fallback) {
            return None;
        }
        let mut stack = Stack::new(faces)?;
        for (i, role) in roles.iter().enumerate() {
            if let FaceRole::Style(style) = role {
                let ix = FaceIx(u8::try_from(i).ok()?);
                let slot = stack.styled.slot(*style)?;
                if slot.is_some() {
                    return None;
                }
                *slot = Some(ix);
            }
        }
        Some(stack)
    }

    /// The style whose face a cell of `style` is drawn from: its own, when
    /// the stack has one, else regular -- the fallback chain. A missing style
    /// never borrows another style's face (Bold Italic is not drawn bold) and
    /// nothing is synthesised. Jev, 0.67, asked with baud's principles;
    /// kitty's rule.
    pub fn effective(&self, style: FaceStyle) -> FaceStyle {
        if self.styled.get(style).is_some() {
            style
        } else {
            FaceStyle::Regular
        }
    }

    /// Whether `ix` is a styled face rather than one of the chain.
    pub fn is_styled(&self, ix: FaceIx) -> bool {
        self.styled.holds(ix)
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

    /// The face a character of `style` is drawn from: the style's own
    /// face when it maps `c`, else the fallback chain.
    pub fn select_styled(&self, c: Codepoint, style: FaceStyle) -> FaceIx {
        let own = self
            .styled
            .get(self.effective(style))
            .filter(|f| self.face(*f).glyph(c).is_some());
        own.unwrap_or_else(|| self.select(c))
    }

    /// The face a character of `style` is drawn from when it asks for a
    /// presentation: the first face whose glyph for `c` is colour, or is not,
    /// in the order [`Stack::select_styled`] tries them -- the style's own
    /// face, then the chain. When no face draws `c` that way, the face
    /// `select_styled` picks: a heart with `U+FE0F` and no colour face is the
    /// text heart, not `.notdef`.
    pub fn select_presented(&self, c: Codepoint, style: FaceStyle, p: Presentation) -> FaceIx {
        let wants = |ix: FaceIx| {
            let f = self.face(ix);
            f.glyph(c)
                .is_some_and(|g| f.is_colour(g) == (p == Presentation::Colour))
        };
        let own = self.styled.get(self.effective(style)).filter(|f| wants(*f));
        own.or_else(|| self.each().find(|ix| !self.styled.holds(*ix) && wants(*ix)))
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
        let chain = |ok: &dyn Fn(Draws) -> bool| {
            self.each()
                .find(|ix| !self.styled.holds(*ix) && ok(at(*ix)))
        };
        let own = self.styled.get(self.effective(style));
        own.filter(|f| at(*f) == want)
            .or_else(|| chain(&|d| d == want))
            .or_else(|| own.filter(|f| at(*f) != Draws::No))
            .or_else(|| chain(&|d| d != Draws::No))
            .unwrap_or(FaceIx::PRIMARY)
    }

    /// The table [`Stack::select_presented`] becomes a lookup into. It
    /// starts with every block unfilled: each face's `cmap` proposes the
    /// blocks it could reach (`Face::candidates`), and a block no face
    /// proposes is decided now, as the primary throughout. The rest are
    /// decided the first time a cell lands in them ([`FaceTable::face`]),
    /// so a launch pays for the blocks its first frame touches and not for
    /// all of CJK.
    pub fn table(&self) -> FaceTable {
        let mut proposed: Vec<Vec<u8>> = vec![Vec::new(); FaceTable::BLOCKS];
        for (i, face) in self.faces.iter().enumerate() {
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
        let mut t = FaceTable::empty(proposed);
        let none = vec![Draws::No; self.faces.len()];
        for (combo, (style, p)) in FaceTable::combos().enumerate() {
            let row = [self.choose(&none, style, p).0; 256];
            t.idle[combo] = t.intern(row).0;
        }
        for blk in 0..FaceTable::BLOCKS {
            if t.proposed[blk].is_empty() {
                for combo in 0..FaceTable::COMBOS {
                    t.blocks[combo * FaceTable::BLOCKS + blk] = t.idle[combo];
                }
            }
        }
        t
    }

    /// Decide every entry of block `blk` of `t`, for all eight styles and
    /// presentations at once: one `cmap` lookup and colour test per face
    /// that proposed the block, per codepoint.
    fn fill(&self, t: &mut FaceTable, blk: Block) {
        let blk = blk.0;
        let faces = core::mem::take(&mut t.proposed[blk]);
        let mut draws = vec![Draws::No; self.faces.len()];
        let mut rows = [[0u8; 256]; FaceTable::COMBOS];
        for lo in 0..256u32 {
            let v = u32::try_from(blk).unwrap_or(0) * 256 + lo;
            let cp = Codepoint::new(v);
            for &i in &faces {
                let f = &self.faces[usize::from(i)];
                draws[usize::from(i)] = match cp.and_then(|c| f.glyph(c)) {
                    None => Draws::No,
                    Some(g) if f.is_colour(g) => Draws::Colour,
                    Some(_) => Draws::Text,
                };
            }
            let at = usize::try_from(lo).unwrap_or(0);
            for (combo, (style, p)) in FaceTable::combos().enumerate() {
                rows[combo][at] = self.choose(&draws, style, p).0;
            }
        }
        for (combo, row) in rows.into_iter().enumerate() {
            let id = t.intern(row);
            t.blocks[combo * FaceTable::BLOCKS + blk] = id.0;
        }
        t.filled.0 += 1;
    }

    /// The first face that maps `c`; the primary when none does, so its
    /// `.notdef` is what a character nobody carries draws as.
    pub fn select(&self, c: Codepoint) -> FaceIx {
        self.faces
            .iter()
            .enumerate()
            .filter_map(|(i, f)| Some((FaceIx(u8::try_from(i).ok()?), f)))
            .find(|(ix, f)| !self.styled.holds(*ix) && f.glyph(c).is_some())
            .map_or(FaceIx::PRIMARY, |(ix, _)| ix)
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

    /// The embedded shape: JetBrains Mono Regular, the icon face, then the
    /// three styles. Text of a style takes its own face; a character that
    /// face lacks -- an icon -- takes the fallback chain, never another
    /// style's face; and regular text never lands in a styled face.
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
    /// only a Bold face present is drawn regular, not bold (Jev, 0.67, asked
    /// with baud's principles; kitty's rule).
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
