use crate::*;

/// `[[source: uax29]]` Grapheme_Cluster_Break property values.
///
/// The unit a *user* calls a character. baud stores one [`Codepoint`] per cell,
/// which is a different unit: `U+1F469 U+200D U+1F4BB` is one cluster and one
/// glyph, and four codepoints' worth of columns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraphemeClass {
    Other,
    Cr,
    Lf,
    Control,
    Extend,
    Zwj,
    RegionalIndicator,
    Prepend,
    SpacingMark,
    L,
    V,
    T,
    Lv,
    Lvt,
}

impl GraphemeClass {
    /// Declaration order, which is what the generated trie stores.
    ///
    /// **Not `transmute` and not a cast.** The trie holds four-bit indices, and
    /// the correspondence between those and this enum is the one thing that
    /// would fail silently if the enum were reordered: every codepoint would
    /// get a different class and the code would still run.
    /// `the_break_trie_agrees_with_the_class_enum` walks both.
    pub const FROM_INDEX: [GraphemeClass; 16] = [
        GraphemeClass::Other,
        GraphemeClass::Cr,
        GraphemeClass::Lf,
        GraphemeClass::Control,
        GraphemeClass::Extend,
        GraphemeClass::Zwj,
        GraphemeClass::RegionalIndicator,
        GraphemeClass::Prepend,
        GraphemeClass::SpacingMark,
        GraphemeClass::L,
        GraphemeClass::V,
        GraphemeClass::T,
        GraphemeClass::Lv,
        GraphemeClass::Lvt,
        // Two spare slots so the index cannot leave the array; the generator
        // asserts fourteen classes, so nothing reaches these.
        GraphemeClass::Other,
        GraphemeClass::Other,
    ];
}

fn lookup(table: &[(u32, u32)], cp: u32) -> bool {
    let (mut lo, mut hi) = (0usize, table.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        let (a, b) = table[mid];
        if cp < a {
            hi = mid;
        } else if cp > b {
            lo = mid + 1;
        } else {
            return true;
        }
    }
    false
}

impl GraphemeClass {
    pub fn of(c: Codepoint) -> GraphemeClass {
        let cp = c.value();
        // ASCII without a binary search. Measured: the search ran for every
        // byte of ordinary text and cost 12x -- 1102 Mcp/s down to 93 -- on a
        // corpus where the answer is always one of four constants.
        if cp < 0x80 {
            return match cp {
                0x0a => GraphemeClass::Lf,
                0x0d => GraphemeClass::Cr,
                0x00..=0x1f | 0x7f => GraphemeClass::Control,
                _ => GraphemeClass::Other,
            };
        }
        // Two indexed loads, not eleven data-dependent branches. This was a
        // binary search over 1,386 ranges and profiled at **27.1%** of the CJK
        // path -- the largest single item in it -- with a ceiling arm returning
        // a constant worth +21 to +29%.
        //
        // `cp` is a `Codepoint`, so it is below 0x110000 and both indices are
        // in range by construction.
        let block = grapheme_data::BREAK_INDEX[(cp >> 8) as usize] as usize;
        let byte = grapheme_data::BREAK_BLOCKS[block * 128 + ((cp as usize & 0xff) >> 1)];
        GraphemeClass::FROM_INDEX[((byte >> ((cp & 1) * 4)) & 0xf) as usize]
    }
}

/// Whether a boundary falls between the codepoints fed so far and the next one.
///
/// A state machine rather than a two-argument predicate, because five of
/// `[[source: uax29]]`'s rules need more context than the immediately preceding
/// codepoint: GB11 looks back through `Extend*` to an `Extended_Pictographic`,
/// GB12/13 need the *parity* of a regional-indicator run, and GB9c needs to
/// know a linker appeared since the last consonant. A pairwise function cannot
/// express any of them, which is the usual reason a naive implementation
/// splits flags and country flags in the wrong places.
/// GB12/13 counts regional indicators **pairwise**, so what matters is the
/// parity of the run, not how many there are.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum RiParity {
    #[default]
    Even,
    Odd,
}

/// GB11's lookback: `Extended_Pictographic Extend* ZWJ × Extended_Pictographic`.
///
/// Three states, not two flags. As `{pict_run, pict_zwj}` bools it was possible
/// to spell "in a run *and* zwj seen", which the rule has no meaning for.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PictLookback {
    #[default]
    None,
    /// Inside `Extended_Pictographic Extend*`.
    Pictographic,
    /// The `ZWJ` has arrived; the next pictographic joins.
    PictographicZwj,
}

/// GB9c's lookback:
/// `Consonant [Extend|Linker]* Linker [Extend|Linker]* × Consonant`.
///
/// Three states for the same reason: "linker seen" is only meaningful *inside*
/// a consonant run, and two bools could say otherwise.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum IncbLookback {
    #[default]
    None,
    /// A consonant, with only `InCB=Extend`/`Linker` since.
    Consonant,
    /// ...and at least one of those was the linker.
    ConsonantLinker,
}

#[derive(Clone, Copy, Default)]
pub struct ClusterBreak {
    prev: Option<GraphemeClass>,
    ri: RiParity,
    pict: PictLookback,
    incb: IncbLookback,
}

impl ClusterBreak {
    pub fn new() -> ClusterBreak {
        ClusterBreak::default()
    }

    /// Is there a cluster boundary immediately **before** `c`?
    ///
    /// Rule numbers are `[[source: uax29]]`'s, and the order is theirs: the
    /// first matching rule wins, and GB999 ("anything else breaks") is last.
    pub fn is_break(&mut self, c: Codepoint) -> bool {
        let cur = GraphemeClass::of(c);
        let cp = c.value();
        let brk = self.decide(cur, cp);
        self.advance(cur, cp, brk);
        brk
    }

    fn decide(&self, cur: GraphemeClass, cp: u32) -> bool {
        use GraphemeClass::*;
        // GB1: sot ÷ Any -- there *is* a boundary before the first codepoint,
        // which is what the leading `÷` in every conformance case means.
        let Some(prev) = self.prev else {
            return true;
        };
        // GB3: CR × LF
        if prev == Cr && cur == Lf {
            return false;
        }
        // GB4: (Control | CR | LF) ÷
        if matches!(prev, Control | Cr | Lf) {
            return true;
        }
        // GB5: ÷ (Control | CR | LF)
        if matches!(cur, Control | Cr | Lf) {
            return true;
        }
        // GB6/7/8: Hangul syllable sequences.
        if prev == L && matches!(cur, L | V | Lv | Lvt) {
            return false;
        }
        if matches!(prev, Lv | V) && matches!(cur, V | T) {
            return false;
        }
        if matches!(prev, Lvt | T) && cur == T {
            return false;
        }
        // GB9: × (Extend | ZWJ). GB9a: × SpacingMark. GB9b: Prepend ×.
        if matches!(cur, Extend | Zwj) || cur == SpacingMark || prev == Prepend {
            return false;
        }
        // GB9c: an Indic conjunct stays whole.
        if self.incb == IncbLookback::ConsonantLinker && lookup(&grapheme_data::INCB_CONSONANT, cp)
        {
            return false;
        }
        // GB11: ExtPict Extend* ZWJ × ExtPict
        if self.pict == PictLookback::PictographicZwj
            && lookup(&grapheme_data::EXTENDED_PICTOGRAPHIC, cp)
        {
            return false;
        }
        // GB12/13: RI × RI, pairwise.
        if cur == RegionalIndicator && self.ri == RiParity::Odd {
            return false;
        }
        // GB999: Any ÷ Any
        true
    }

    fn advance(&mut self, cur: GraphemeClass, cp: u32, brk: bool) {
        use GraphemeClass::*;
        self.prev = Some(cur);

        // The four lookback tables start at U+00A9, U+0300, U+0915 and U+094D,
        // so nothing below U+00A9 can be in any of them. Without this guard
        // every codepoint paid four binary searches to learn it was not an
        // emoji, a virama or an Indic consonant. Measured: ASCII segmentation
        // went from 93 to 338 Mcp/s with this and the class fast path.
        if cp < 0xa9 {
            self.ri = RiParity::Even;
            self.pict = PictLookback::None;
            self.incb = IncbLookback::None;
            return;
        }

        // **Above the guard, every codepoint paid two binary searches to learn
        // it is neither an emoji nor an Indic consonant.** `LOOKBACK_BLOCKS`
        // is one bit per 32 codepoints, set when the block holds any entry of
        // the four lookback tables; 391 of 34,816 are set, so 98.9% of the
        // space answers here. btop draws with `U+2500..U+257F` and profiled at
        // 12-13% in `is_break`.
        //
        // 32 codepoints a bit and not 256, because `U+25xx` holds box drawing
        // *and* emoji -- `U+25AA`, `U+25B6`, `U+25FB..FE` -- so a wider block
        // cannot separate them.
        let blk = (cp >> 5) as usize;
        let listed = grapheme_data::LOOKBACK_BLOCKS
            .get(blk / 64)
            .is_some_and(|w| w & (1u64 << (blk % 64)) != 0);
        if !listed {
            // Exactly what the four searches below would have concluded: not
            // pictographic, not a consonant, not a linker, not an extend. The
            // `in_run` arms survive because they do not depend on a lookup.
            self.ri = if cur == RegionalIndicator && brk {
                RiParity::Odd
            } else {
                RiParity::Even
            };
            let in_run = self.pict == PictLookback::Pictographic;
            self.pict = if in_run && cur == Extend {
                PictLookback::Pictographic
            } else if in_run && cur == Zwj {
                PictLookback::PictographicZwj
            } else {
                PictLookback::None
            };
            self.incb = IncbLookback::None;
            return;
        }

        // A boundary resets the parity, so a third indicator starts a new pair
        // rather than extending the flag before it.
        // An indicator that *paired* (no boundary before it) closes the pair
        // and leaves the parity even, so a third starts a fresh one. Both arms
        // returning `Odd` is what broke `RI RI RI`: the third indicator joined
        // the flag before it instead of beginning the next.
        self.ri = if cur == RegionalIndicator && brk {
            RiParity::Odd
        } else {
            RiParity::Even
        };

        let in_run = self.pict == PictLookback::Pictographic;
        self.pict =
            if lookup(&grapheme_data::EXTENDED_PICTOGRAPHIC, cp) || (in_run && cur == Extend) {
                // `Extend` does not end the run: GB11 looks back *through* it.
                PictLookback::Pictographic
            } else if in_run && cur == Zwj {
                PictLookback::PictographicZwj
            } else {
                PictLookback::None
            };

        self.incb = if lookup(&grapheme_data::INCB_CONSONANT, cp) {
            IncbLookback::Consonant
        } else if self.incb == IncbLookback::None {
            IncbLookback::None
        } else if lookup(&grapheme_data::INCB_LINKER, cp) {
            IncbLookback::ConsonantLinker
        } else if lookup(&grapheme_data::INCB_EXTEND, cp) {
            self.incb
        } else {
            IncbLookback::None
        };
    }
}
