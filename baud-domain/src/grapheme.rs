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

/// Which lookback tables a codepoint belongs to, as four bits.
///
/// Carried in the high nibble of the class trie, because the rules ask for the
/// class and these together. The bit order is the generator's `LOOKBACK` list
/// and `the_break_trie_agrees_with_the_class_enum` pins it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lookback(pub(crate) u8);

impl Lookback {
    pub const NONE: Lookback = Lookback(0);
    /// In no lookback table at all -- 98.9% of the space.
    #[inline(always)]
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
    #[inline(always)]
    pub fn pictographic(self) -> bool {
        self.0 & 1 != 0
    }
    #[inline(always)]
    pub fn linker(self) -> bool {
        self.0 & 2 != 0
    }
    #[inline(always)]
    pub fn consonant(self) -> bool {
        self.0 & 4 != 0
    }
    #[inline(always)]
    pub fn incb_extend(self) -> bool {
        self.0 & 8 != 0
    }
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
        GraphemeClass::of_with_lookback(c).0
    }

    /// The class **and** the four lookback-table bits, from one indexed load.
    ///
    /// They live in one byte because they are wanted in one place: the rules
    /// ask for the class, then -- for the few codepoints that matter -- whether
    /// it is `Extended_Pictographic` or an Indic consonant, linker or extend.
    /// That used to be a 32-codepoint bitmap probe followed by up to four
    /// binary searches over 629 ranges.
    ///
    /// It also makes membership **exact** where the bitmap was a superset, so
    /// more codepoints reach `is_break`'s fast path.
    #[inline(always)]
    pub fn of_with_lookback(c: Codepoint) -> (GraphemeClass, Lookback) {
        let cp = c.value();
        if cp < 0x80 {
            let k = match cp {
                0x0a => GraphemeClass::Lf,
                0x0d => GraphemeClass::Cr,
                0x00..=0x1f | 0x7f => GraphemeClass::Control,
                _ => GraphemeClass::Other,
            };
            return (k, Lookback::NONE);
        }
        let block = grapheme_data::BREAK_INDEX[(cp >> 8) as usize] as usize;
        let byte = grapheme_data::BREAK_BLOCKS[block * 256 + (cp as usize & 0xff)];
        (
            GraphemeClass::FROM_INDEX[(byte & 0xf) as usize],
            Lookback(byte >> 4),
        )
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

/// Why a run of ordinary characters cannot be taken in bulk.
///
/// Named states rather than a `bool`, because the answer decides which lever
/// to pull: `Prev` means the run has not started, `Pictographic` means an
/// emoji opened a GB11 lookback, and the two call for different work.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunBlocker {
    /// Nothing blocks it.
    Ready,
    /// The previous codepoint was not an ordinary one -- or there was none,
    /// which is the state a reset leaves behind.
    Prev,
    /// An odd number of regional indicators is pending.
    Regional,
    /// An `Extended_Pictographic` run is open, so GB11 may still join.
    Pictographic,
    /// An Indic conjunct linker is pending, so GB9c may still join.
    Indic,
}

impl ClusterBreak {
    pub fn new() -> ClusterBreak {
        ClusterBreak::default()
    }

    /// Is the machine in the state where a run of ordinary characters needs no
    /// work at all?
    ///
    /// The bulk path's precondition. If `prev` is `Other` and no lookback is
    /// live, then for any run of codepoints that are themselves `Other` and in
    /// no lookback table, `is_break` returns `true` every time and leaves every
    /// field exactly as it found it -- the same argument the fast path in
    /// `is_break` makes, extended along the run because each character
    /// re-establishes the precondition for the next.
    ///
    /// So the caller may skip the machine entirely for such a run, and the
    /// state afterwards is the state before.
    #[inline(always)]
    /// The state a completed run of ordinary characters leaves behind.
    ///
    /// What `advance_plain(Other, true)` writes, which is the same for every
    /// such character: `prev` is `Other` and nothing is pending. Setting it
    /// once for a run is the whole of the machine's work for all of them.
    pub fn set_after_ordinary(&mut self) {
        self.prev = Some(GraphemeClass::Other);
        self.ri = RiParity::Even;
        self.pict = PictLookback::None;
        self.incb = IncbLookback::None;
    }

    /// Which of `ordinary_run_ready`'s four conditions said no.
    ///
    /// **A predicate that answers "no" cannot be optimised against.** The bulk
    /// wide path is gated on this state and was being refused on 92% of a CJK
    /// recording's wide characters, while every hand-written shape passed it
    /// 100% of the time -- so the shape that refuses it is one nobody had
    /// thought to write down, which is exactly the case a boolean hides.
    pub fn ordinary_run_blocker(&self) -> RunBlocker {
        if self.prev != Some(GraphemeClass::Other) {
            RunBlocker::Prev
        } else if self.ri != RiParity::Even {
            RunBlocker::Regional
        } else if self.pict != PictLookback::None {
            RunBlocker::Pictographic
        } else if self.incb != IncbLookback::None {
            RunBlocker::Indic
        } else {
            RunBlocker::Ready
        }
    }

    pub fn ordinary_run_ready(&self) -> bool {
        self.prev == Some(GraphemeClass::Other)
            && self.ri == RiParity::Even
            && self.pict == PictLookback::None
            && self.incb == IncbLookback::None
    }

    /// Is there a cluster boundary immediately **before** `c`?
    ///
    /// Rule numbers are `[[source: uax29]]`'s, and the order is theirs: the
    /// first matching rule wins, and GB999 ("anything else breaks") is last.
    /// `#[inline(always)]`, with `decide` and `advance` deliberately **not**.
    ///
    /// The fast path below is six compares and a table lookup, and it answers
    /// for every ordinary character. Left as a real call it profiled at
    /// **24.6%** of `cat-cjk.raw` -- its own leaf frame, which is the tell --
    /// and the same shape cost `View::clear_partners` 11% until it was
    /// inlined.
    ///
    /// **`decide` and `advance` are inlined too, and that reverses an earlier
    /// decision.** They were kept out of line so their emoji and Indic arms
    /// would not land in `feed_with`, which this repo has measured being
    /// sensitive to code size. That was measured on ASCII-heavy payloads,
    /// where the fast path above answers and neither is ever called. Emoji are
    /// the opposite case: a pictographic character cannot take the fast path,
    /// so both were real calls on every codepoint of a ZWJ sequence -- seven
    /// of them in a family. Inlined, `opcost` prices that cluster at **52.2 ns
    /// against 58.1**, cat-cjk gains 2.5% and less-cjk 4%, and every other
    /// payload is flat.
    #[inline(always)]
    pub fn is_break(&mut self, c: Codepoint) -> bool {
        let (cur, lb) = GraphemeClass::of_with_lookback(c);
        // **An ordinary character after an ordinary character, with no lookback
        // live.** Han, kana and most punctuation are all `Other`, so this is
        // the shape a page of CJK text has, and both halves below are provably
        // no-ops for it:
        //
        // * `decide` falls through every rule to GB999 (`Any ÷ Any`). GB3-9b
        //   need a `Control`, a Hangul class, an `Extend`, a `Zwj`, a
        //   `SpacingMark` or a `Prepend` on one side; GB9c and GB11 need the
        //   `incb`/`pict` lookbacks, which this requires to be clear; GB12/13
        //   need a regional indicator.
        // * `advance` writes back exactly what is already there: `prev` is
        //   already `Other`, `ri` stays `Even` because `cur` is not a regional
        //   indicator, `pict` stays `None` because the run it would continue is
        //   not open, and `incb` is set to `None`.
        //
        // `listed` is not optional. Emoji are class `Other` *and*
        // `Extended_Pictographic`, so without it a rocket would take this path
        // and never open a pictographic run -- which is GB11, and every ZWJ
        // sequence.
        if cur == GraphemeClass::Other
            && self.prev == Some(GraphemeClass::Other)
            && self.ri == RiParity::Even
            && self.pict == PictLookback::None
            && self.incb == IncbLookback::None
            && lb.is_none()
        {
            return true;
        }
        let brk = self.decide(cur, lb);
        // **Split by the bitmap, not inside the callee.** A codepoint outside
        // the lookback blocks -- 98.9% of the space, and everything in Hangul,
        // Han and kana -- needs three state writes and no table at all. Left
        // inside `advance`, that cheap case would pay for the emoji and Indic
        // arms it never reaches.
        match lb.is_none() {
            false => self.advance(cur, brk, lb),
            true => self.advance_plain(cur, brk),
        }
        brk
    }

    /// `advance` for a codepoint in no lookback table: three writes.
    ///
    /// Exactly what the four searches in `advance` would have concluded -- not
    /// pictographic, not a consonant, not a linker, not an extend -- with the
    /// `in_run` arms kept because they depend on state rather than on a lookup.
    #[inline(always)]
    fn advance_plain(&mut self, cur: GraphemeClass, brk: bool) {
        use GraphemeClass::*;
        self.prev = Some(cur);
        self.ri = match cur == RegionalIndicator && brk {
            true => RiParity::Odd,
            false => RiParity::Even,
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
    }

    #[inline(always)]
    fn decide(&self, cur: GraphemeClass, lb: Lookback) -> bool {
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
        if self.incb == IncbLookback::ConsonantLinker && lb.consonant() {
            return false;
        }
        // GB11: ExtPict Extend* ZWJ × ExtPict
        if self.pict == PictLookback::PictographicZwj && lb.pictographic() {
            return false;
        }
        // GB12/13: RI × RI, pairwise.
        if cur == RegionalIndicator && self.ri == RiParity::Odd {
            return false;
        }
        // GB999: Any ÷ Any
        true
    }

    #[inline(always)]
    fn advance(&mut self, cur: GraphemeClass, brk: bool, lb: Lookback) {
        use GraphemeClass::*;
        self.prev = Some(cur);

        // **Only reached when the codepoint is in a lookback table.**
        // `is_break` splits on `Lookback::is_none`, so the guards this used to
        // open with -- `cp < 0xa9`, then a bitmap probe -- are both dead here:
        // `advance_plain` handles everything they used to send home early, and
        // it is inlined.

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
        self.pict = if lb.pictographic() || (in_run && cur == Extend) {
            // `Extend` does not end the run: GB11 looks back *through* it.
            PictLookback::Pictographic
        } else if in_run && cur == Zwj {
            PictLookback::PictographicZwj
        } else {
            PictLookback::None
        };

        self.incb = if lb.consonant() {
            IncbLookback::Consonant
        } else if self.incb == IncbLookback::None {
            IncbLookback::None
        } else if lb.linker() {
            IncbLookback::ConsonantLinker
        } else if lb.incb_extend() {
            self.incb
        } else {
            IncbLookback::None
        };
    }
}
