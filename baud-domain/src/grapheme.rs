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
        // Two spare slots so the index cannot leave the array; the build
        // stores fourteen classes, so nothing reaches these.
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
    /// The raw membership bits, for tooling that has to reason about how many
    /// of them a merged table would have to carry.
    pub fn bits(self) -> u8 {
        self.0
    }
    /// Rebuild from the bits a generated table stored.
    pub fn from_bits(b: u8) -> Lookback {
        Lookback(b)
    }
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
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum RiParity {
    #[default]
    Even,
    Odd,
}

impl RiParity {
    /// The parity after a character of class `cur`: an indicator after an
    /// even run opens a pair and one after an odd run closes it, whatever
    /// joined the run to the character before it (`÷ 0600 × 1F1E6 × 1F1E6 ÷`,
    /// GB9b then GB13), and anything else ends the run.
    #[inline(always)]
    fn after(self, cur: GraphemeClass) -> RiParity {
        match (cur, self) {
            (GraphemeClass::RegionalIndicator, RiParity::Even) => RiParity::Odd,
            _ => RiParity::Even,
        }
    }
}

/// GB11's lookback: `Extended_Pictographic Extend* ZWJ × Extended_Pictographic`.
///
/// Three states, not two flags. As `{pict_run, pict_zwj}` bools it was possible
/// to spell "in a run *and* zwj seen", which the rule has no meaning for.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
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
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum IncbLookback {
    #[default]
    None,
    /// A consonant, with only `InCB=Extend`/`Linker` since.
    Consonant,
    /// ...and at least one of those was the linker.
    ConsonantLinker,
}

/// The cluster machine the terminal runs: one state id into
/// `CLUSTER_NEXT`, and nothing else.
///
/// **It carried the rule fields too, until 2026-10-16, and the two halves
/// could disagree.** One struct held the table id and [`ClusterRules`]'s
/// four fields; `is_break_classified` moved the id, `is_break_by_rules`
/// and `set_after_ordinary` the fields, and `from_bits` set the fields
/// with the id at 0. So `new()` stepped by one ordinary character said
/// ready by the table and `Prev` by the fields, and the cluster emitter,
/// asking `ordinary_run_ready` of a `from_bits` machine, wrote no ready
/// state from `0be6329a` until `150e1dbb`. The rule form is its own type
/// now, so a machine holds one form and cannot answer with the other;
/// `the_two_forms_agree_in_lockstep_from_every_constructor` steps the two
/// side by side. The runtime struct went from five bytes to one, and
/// ingest2 was measured on it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct ClusterBreak {
    /// The whole machine, as one state id into `CLUSTER_NEXT`.
    table_state: u8,
}

/// The cluster machine as `[[source: uax29]]`'s rules over four fields:
/// the definition `CLUSTER_NEXT` answers to.
///
/// Tooling and tests only. `build/cluster.rs` compiles the table from a
/// second implementation of the rules, since a build script cannot call
/// the crate it builds, and `the_cluster_table_agrees_with_the_rules` and
/// `the_parts_tables_agree_with_the_rules` (`tests/cluster_states.rs`)
/// hold the table to this one at every reachable state, through
/// `to_bits`/`from_bits`. It has no table id, so nothing on it can
/// disagree with one.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct ClusterRules {
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

/// **How much of a cluster one cell holds**, which is how many columns a
/// syllable takes: `[[source: uax29]]` § 3.1 defines two variants of the
/// grapheme cluster, "the same rules ... except the rules GB9a, GB9b, and
/// GB9c", and a terminal that puts one cluster in a cell counts columns by
/// the variant it clusters by.
///
/// The machine is always the extended one ([`ClusterBreak`]); a join only
/// the extended rules make -- a spacing mark (GB9a), a character after a
/// Prepend (GB9b), a consonant after a virama (GB9c) -- is where the legacy
/// variants start a cell of their own, which [`ClusterBreak::parts`] answers
/// from a generated table. `the_legacy_variants_are_the_extended_machine_parted`
/// holds that composition to the variants' own rules over every reachable
/// pair of states.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ClusterCells {
    /// One legacy grapheme cluster a cell: each letter and each spacing mark
    /// a column, as glibc's `wcwidth`, unicode-width, vim and the Linux
    /// shells count. The state at power-on and after `RIS` and `DECSTR`.
    #[default]
    Legacy,
    /// One legacy grapheme cluster a cell, but a spacing mark joins the cell
    /// before it (GB9a kept): each letter a column and a spacing mark none,
    /// as the macOS shells and tmux count. The embedder's choice
    /// ([`SpacingMarks::Joined`]).
    LegacyMarksJoined,
    /// One extended grapheme cluster a cell, its first character's width:
    /// `?2027` set.
    Extended,
}

impl ClusterCells {
    /// The cells `?2027` and the embedder's [`SpacingMarks`] choose.
    pub fn of(clusters: GraphemeMode, marks: SpacingMarks) -> ClusterCells {
        match (clusters, marks) {
            (GraphemeMode::Extended, _) => ClusterCells::Extended,
            (GraphemeMode::Legacy, SpacingMarks::Column) => ClusterCells::Legacy,
            (GraphemeMode::Legacy, SpacingMarks::Joined) => ClusterCells::LegacyMarksJoined,
        }
    }
}

/// **`?2027`'s two states**, the mode 2027 draft's (`[[source: mode2027]]`):
/// set, "every consecutively written character on the terminal stream that
/// is non-breakable as per UTS-29 will always end up in the same terminal's
/// grid cell"; reset, "as undefined as if this specification was not
/// implemented at all", which baud takes as legacy grapheme clusters.
/// "Both, Soft reset (DECSTR) as well as hard reset (RIS) will reset the
/// mode."
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GraphemeMode {
    #[default]
    Legacy,
    Extended,
}

/// **Whether a spacing mark takes a column** while `?2027` is reset: the
/// embedder's choice, `[text] join_spacing_marks` in baud's configuration.
/// `Column` is the default, UAX #44's "positive advance width" for `Mc`
/// (`[[source: uax44]]`) and what glibc, unicode-width, vim and the Linux
/// shells count; `Joined` is what the macOS shells and tmux count
/// (`baud-core/tests/devanagari_columns.tsv`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SpacingMarks {
    #[default]
    Column,
    Joined,
}

/// Whether `cp` is a spacing mark UAX #29 classes Extend (general category
/// `Mc`, Grapheme_Cluster_Break `Extend`): a binary search over the 46
/// generated ranges, on the cold side of a join alone.
pub fn is_spacing_extend(cp: Codepoint) -> bool {
    let v = cp.value();
    let ranges = &crate::grapheme_data::SPACING_EXTEND;
    let i = ranges.partition_point(|&(_, end)| end < v);
    ranges.get(i).is_some_and(|&(start, _)| start <= v)
}

/// **The Devanagari ranges**: the Devanagari block, U+0900-097F, its
/// Extended block, U+A8E0-A8FF, and the Vedic Extensions, U+1CD0-1CFF, each
/// first and last -- the letters a renderer draws a word of whole, the
/// danda and the digits with them.
pub const DEVANAGARI_RANGES: [(u32, u32); 3] =
    [(0x0900, 0x097F), (0xA8E0, 0xA8FF), (0x1CD0, 0x1CFF)];

/// **A bit for each 256-character block that holds a character a renderer
/// re-forming syllables and Devanagari words reads**: one a legacy cluster
/// parts at or after ([`ClusterBreak::parts_at`] under
/// [`ClusterCells::Legacy`]) -- a Prepend, whose successor GB9b joins, a
/// SpacingMark (GB9a), an InCB consonant (GB9c), or a spacing mark classed
/// Extend ([`is_spacing_extend`]) -- or one of [`DEVANAGARI_RANGES`].
/// Compiled at build time from the class table, the spacing Extend ranges
/// and those, one test of each distinct block's 256 entries, and a zero
/// word after the 68 for every word past U+10FFFF: text none of whose
/// characters is in a block this sets is never parted and holds no
/// Devanagari letter, so a row of it is let go by one load a cell.
pub static SYLLABLE_BLOCKS: [u64; 69] = syllable_blocks();

/// Whether `w` -- a codepoint, or a cell's word, every one past U+10FFFF
/// answered no -- is in a block [`SYLLABLE_BLOCKS`] sets: one load, the
/// block clamped to the zero word rather than tested, so a row's pass
/// has no branch a cell.
#[inline(always)]
pub fn in_syllable_block(w: u32) -> bool {
    let block = (w >> 8).min(68 * 64);
    SYLLABLE_BLOCKS[(block >> 6) as usize] >> (block & 63) & 1 == 1
}

pub(crate) const fn syllable_blocks() -> [u64; 69] {
    let blocks = grapheme_data::BREAK_BLOCKS;
    let index = grapheme_data::BREAK_INDEX;
    let distinct = blocks.len() / 256;
    let mut parts = [false; 256];
    let mut b = 0;
    while b < distinct {
        let mut i = 0;
        while i < 256 {
            let byte = blocks[b * 256 + i];
            let class = byte & 0xf;
            // The class in the low nibble, by `FROM_INDEX`; the lookback in
            // the high one, whose third bit is an InCB consonant
            // ([`Lookback::consonant`]).
            if class == GraphemeClass::Prepend as u8
                || class == GraphemeClass::SpacingMark as u8
                || (byte >> 4) & 4 != 0
            {
                parts[b] = true;
            }
            i += 1;
        }
        b += 1;
    }
    let mut out = [0u64; 69];
    let mut k = 0;
    while k < index.len() {
        if parts[index[k] as usize] {
            out[k / 64] |= 1 << (k % 64);
        }
        k += 1;
    }
    let spacing = &grapheme_data::SPACING_EXTEND;
    let mut r = 0;
    while r < spacing.len() {
        out = with_blocks(out, spacing[r]);
        r += 1;
    }
    let mut r = 0;
    while r < DEVANAGARI_RANGES.len() {
        out = with_blocks(out, DEVANAGARI_RANGES[r]);
        r += 1;
    }
    out
}

/// `out` with the bit of every block from `first` to `last` set.
const fn with_blocks(mut out: [u64; 69], (first, last): (u32, u32)) -> [u64; 69] {
    let mut k = (first >> 8) as usize;
    while k <= (last >> 8) as usize {
        out[k / 64] |= 1 << (k % 64);
        k += 1;
    }
    out
}

/// **Which rule of `[[source: uax29]]` joined a character to the cluster
/// before it**, as far as the two variants differ: [`ClusterRules::rule`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Join {
    /// A rule both variants have: GB3, GB6-GB8, GB9, GB11, GB12/13.
    Both,
    /// GB9a, `× SpacingMark`.
    SpacingMark,
    /// GB9b, `Prepend ×`.
    Prepend,
    /// GB9c, a consonant after a linker.
    Conjunct,
}

impl Join {
    /// Whether the variant `cells` makes this join too.
    pub fn kept_by(self, cells: ClusterCells) -> bool {
        match (self, cells) {
            (Join::Both, _) | (_, ClusterCells::Extended) => true,
            (Join::SpacingMark, ClusterCells::LegacyMarksJoined) => true,
            (Join::SpacingMark | Join::Prepend | Join::Conjunct, _) => false,
        }
    }
}

impl ClusterBreak {
    pub fn new() -> ClusterBreak {
        ClusterBreak::default()
    }

    /// The state a control leaves: nothing for the next character to join.
    ///
    /// `[[source: uax29]]` GB4 breaks after Control, CR and LF and GB5
    /// before them, so whatever follows a control starts a cluster, and every
    /// lookback -- GB9c's, GB11's, GB12/13's parity -- ends at it. The
    /// terminal acts on a control rather than writing it, so the machine is
    /// never fed one; this is what feeding it one would leave, said rather
    /// than computed, by `baud-core`'s `Act::EndCluster` where a character
    /// the machine is asked about follows a control.
    ///
    /// **It is `new()`, and that is checked rather than argued.**
    /// `the_state_after_a_control_is_the_state_before_any_text` feeds the
    /// table a Control from `new()` and compares the two rows on all 256
    /// inputs, break bit and successor alike, and walks the rules the same
    /// way. The state ids differ -- `prev` is `None` in one and `Control` in
    /// the other -- and no input can tell them apart, since GB1 at the start
    /// of text and GB4 after a control both break and both leave the next
    /// state to the character alone.
    pub fn after_control() -> ClusterBreak {
        ClusterBreak::new()
    }

    /// The truthful state after a run of **printable ASCII**, which is also
    /// the state one ordinary character leaves.
    ///
    /// Printable ASCII never reaches the cluster machine: `Paint::piece` and
    /// `Paint::blit` write it without asking, and tell the machine with this.
    /// Printable ASCII is class `Other`, and an ASCII character is not
    /// pictographic, not a regional indicator and not `InCB`, so `prev =
    /// Other` with no lookback live is what any run of it leaves. `put_run`
    /// uses it too, after a run of ordinary multi-byte characters it writes in
    /// bulk, as the machine's advance across the whole run.
    ///
    /// **They told it `new()` until 2026-10-10, and that was a departure.**
    /// `baud-core2` kept the reset, `prev = None`, because the truthful state
    /// cost 1.5% -- cat-cjk 401.5-403.7 against 407.3-411.3, every test and 56
    /// properties at 20,000 cases identical, `cells_written` byte-identical --
    /// and because every rule that reads `prev` looked closed:
    ///
    /// * a zero-width mark after ASCII attaches either way, because
    ///   `Paint::glyph` routed `Width::ZERO` to `Paint::attach` whatever the
    ///   break decision was (`baud-core2`'s `put_cp` routed it to
    ///   `attach_mark`), until 2026-10-23, when one the machine breaks
    ///   before, after a control, became a cell of its own;
    /// * a `ZWJ` after ASCII opens no GB11 lookback in either state, because
    ///   GB11 needs a pictographic *before* the joiner;
    /// * a regional indicator after ASCII starts with `RiParity::Even` in
    ///   both.
    ///
    /// What the argument missed is every continuation that is not zero-width.
    /// `[[source: uax29]]`'s GB9 `× (Extend | ZWJ)` and GB9a `× SpacingMark`
    /// hold after `prev = Other`, but after `prev = None` GB1 breaks first, and
    /// only a `Width::ZERO` codepoint reaches `Paint::attach` whatever the
    /// break decision. So after printable ASCII a SpacingMark took a cell of
    /// its own (GB9a: `a U+0903` put the visarga beside the `a`), and so did an
    /// Extend that has a width (GB9: `a U+09BE`, a vowel sign of category `Mc`,
    /// and `a U+1F3FF`, an emoji modifier two columns wide), where the rules
    /// make each one cluster. After `é`, class `Other` too but written through
    /// the machine, each attached. Four lines of `[[source: gbtest]]` failed
    /// through the terminal for this reason -- 691 (`÷ 0020 × 0903 ÷`), 763
    /// (`÷ 0061 × 0903 ÷ 0062 ÷`), 766 (`÷ 0061 × 1F3FF ÷ 1F476 ÷`) and 767 --
    /// and with a letter written before them, so did the file's 32 control-free
    /// lines that begin with `U+0903`.
    /// `every_line_of_the_suite_clusters_through_the_terminal` holds them all,
    /// and `a_continuation_with_a_width_joins_the_ascii_letter_before_it` the
    /// `U+09BE` case the file does not have.
    pub fn after_ascii_run() -> ClusterBreak {
        ClusterBreak {
            // `prev = Other` with no lookback live is exactly the one
            // state `ordinary_run_ready` accepts, so under the table it
            // is that id: `ClusterRules::after_ascii_run`'s state.
            table_state: crate::cluster_data::CLUSTER_READY,
        }
    }

    /// Is the machine in the state where a run of ordinary characters needs no
    /// work at all?
    ///
    /// The bulk path's precondition. `CLUSTER_READY` is the state of
    /// `prev = Other` with no lookback live; in it, every codepoint that is
    /// itself `Other` and in no lookback table breaks and leaves the id at
    /// `CLUSTER_READY` -- the same argument the fast path in `is_break`
    /// makes, extended along the run because each character re-establishes
    /// the precondition for the next.
    ///
    /// So the caller may skip the machine entirely for such a run, and the
    /// state afterwards is the state before. The fields are
    /// `ClusterRules::after_ascii_run`'s, and this id is theirs.
    ///
    /// **This doc and an `#[inline(always)]` sat above `set_after_ordinary` from
    /// `e0612eb` (2026-08-29), which inserted that function between them and this
    /// one.** The attribute was dropped rather than re-attached: neither function
    /// had a caller outside the tests (`set_after_ordinary` was deleted on
    /// 2026-10-16; `ClusterRules::after_ascii_run` is its state), and ingest2's
    /// `__text` is byte-identical without it against `4b703f8`; a forced attribute
    /// is a codegen change with a number the day a caller arrives.
    pub fn ordinary_run_ready(&self) -> bool {
        // Exactly one of the 94 reachable states satisfies this, so under the
        // table it is a compare rather than four field tests.
        self.table_state == crate::cluster_data::CLUSTER_READY
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
        self.is_break_classified(cur, lb)
    }

    /// The same decision, for a caller that already has the classification.
    ///
    /// **Split so one table lookup can serve both questions.** `Width::of` and
    /// `of_with_lookback` are separate tries over the same `cp >> 8` shape, so
    /// a caller that needs the width -- which is every caller on the text path,
    /// since the width decides how many cells the character occupies -- was
    /// paying two lookups for one codepoint. [`Classified`] answers both from
    /// one, and this is the half of `is_break` that does not need the table.
    /// The same decision as a **single indexed load**.
    ///
    /// `decide` walks the GB rules in order and `advance` writes four fields.
    /// Both are branchy, and on emoji-heavy text those branches are the
    /// unpredictable ones. The whole machine state is 94 reachable
    /// combinations and its input is the byte the flat property table already
    /// returns, so the transition fits one byte: bit 7 the break, bits 0..7
    /// the next state.
    #[inline(always)]
    pub fn is_break_classified(&mut self, cur: GraphemeClass, lb: Lookback) -> bool {
        let i = (self.table_state as usize) * 256 + ((cur as usize) | ((lb.bits() as usize) << 4));
        let e = crate::cluster_data::CLUSTER_NEXT[i];
        self.table_state = e & 0x7f;
        e & 0x80 != 0
    }

    /// **Whether a character the extended rules joined starts a cell of its
    /// own under `cells`**, asked of the machine as it was *before* the
    /// character (`self`): the join was GB9a, GB9b or GB9c, and `cells` is a
    /// variant without it. One bit of a generated table per (state, input),
    /// read only where the machine joined -- the cold side of a write.
    ///
    /// A character that parts takes the successor a boundary before it
    /// would leave ([`ClusterBreak::restart`]), which is the variant's own
    /// state.
    pub fn parts(self, cells: ClusterCells, cur: GraphemeClass, lb: Lookback) -> bool {
        let table = match cells {
            ClusterCells::Extended => return false,
            ClusterCells::Legacy => crate::cluster_data::CLUSTER_PARTS_LEGACY,
            ClusterCells::LegacyMarksJoined => crate::cluster_data::CLUSTER_PARTS_MARKS_JOINED,
        };
        let i = (self.table_state as usize) * 256 + ((cur as usize) | ((lb.bits() as usize) << 4));
        table[i >> 3] >> (i & 7) & 1 != 0
    }

    /// **[`ClusterBreak::parts`], and the spacing marks UAX #29 classes
    /// Extend**: a character the extended rules joined, `cp`, starts a cell
    /// of its own under legacy clusters too where it is general category
    /// `Mc` and GB9 joined it ([`is_spacing_extend`]) -- U+09BE BENGALI
    /// VOWEL SIGN AA, U+0BBE TAMIL VOWEL SIGN AA, the Kannada vowel signs
    /// U+0CC0-0CCB -- so each spacing vowel sign takes a column, as glibc's
    /// `wcwidth` and vim count, where both of `[[source: uax29]]`'s variants
    /// keep it in its letter's cluster; with spacing marks joined it joins
    /// as the others do. The one test every caller asks: the core where a
    /// write joined, the renderer re-forming a syllable, a composition's
    /// layout.
    pub fn parts_at(
        self,
        cells: ClusterCells,
        cur: GraphemeClass,
        lb: Lookback,
        cp: Codepoint,
    ) -> bool {
        // The width before the search: an Extend that takes no column -- a
        // nonspacing mark, a joiner, a selector, the most of them -- is no
        // spacing mark, and its own table entry, which the write just read,
        // says so without the search.
        self.parts(cells, cur, lb)
            || (cells == ClusterCells::Legacy
                && cur == GraphemeClass::Extend
                && Classified::of(cp).width(WidthPolicy::DEFAULT) != Width::ZERO
                && is_spacing_extend(cp))
    }

    /// The machine after a boundary before a character of class `cur`:
    /// what a control leaves, stepped by it. A boundary forgets the state
    /// before it (`a_break_forgets_the_state_before_it`).
    pub fn restart(cur: GraphemeClass, lb: Lookback) -> ClusterBreak {
        let mut m = ClusterBreak::after_control();
        let _ = m.is_break_classified(cur, lb);
        m
    }
}

impl ClusterRules {
    pub fn new() -> ClusterRules {
        ClusterRules::default()
    }

    /// [`ClusterBreak::after_ascii_run`]'s state in the rule form: `prev`
    /// is `Other` and no lookback is live.
    pub fn after_ascii_run() -> ClusterRules {
        ClusterRules {
            prev: Some(GraphemeClass::Other),
            ..ClusterRules::default()
        }
    }

    /// The machine's whole state as nine bits, for tooling that has to
    /// enumerate it. `prev` is 15 values, `ri` 2, `pict` 3, `incb` 3.
    pub fn to_bits(self) -> u16 {
        let prev = match self.prev {
            None => 14u16,
            Some(c) => c as u16,
        };
        let ri = match self.ri {
            RiParity::Even => 0u16,
            RiParity::Odd => 1,
        };
        let pict = match self.pict {
            PictLookback::None => 0u16,
            PictLookback::Pictographic => 1,
            PictLookback::PictographicZwj => 2,
        };
        let incb = match self.incb {
            IncbLookback::None => 0u16,
            IncbLookback::Consonant => 1,
            IncbLookback::ConsonantLinker => 2,
        };
        prev | (ri << 4) | (pict << 5) | (incb << 7)
    }

    /// Inverse of [`ClusterRules::to_bits`].
    pub fn from_bits(b: u16) -> ClusterRules {
        ClusterRules {
            prev: match b & 0xf {
                14 => None,
                n => Some(GraphemeClass::FROM_INDEX[n as usize]),
            },
            ri: match (b >> 4) & 1 {
                0 => RiParity::Even,
                _ => RiParity::Odd,
            },
            pict: match (b >> 5) & 3 {
                0 => PictLookback::None,
                1 => PictLookback::Pictographic,
                _ => PictLookback::PictographicZwj,
            },
            incb: match (b >> 7) & 3 {
                0 => IncbLookback::None,
                1 => IncbLookback::Consonant,
                _ => IncbLookback::ConsonantLinker,
            },
        }
    }

    /// Which of [`ClusterBreak::ordinary_run_ready`]'s four conditions,
    /// read from the fields, says no.
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

    /// The rules, walked. **Not the hot path** --
    /// [`ClusterBreak::is_break_classified`] is a table now -- but
    /// `the_cluster_table_agrees_with_the_rules` checks every reachable state
    /// of the table against it, so this stays as the definition the table
    /// answers to.
    pub fn is_break_by_rules(&mut self, cur: GraphemeClass, lb: Lookback) -> bool {
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
            false => self.advance(cur, lb),
            true => self.advance_plain(cur),
        }
        brk
    }

    /// `advance` for a codepoint in no lookback table: three writes.
    ///
    /// Exactly what the four searches in `advance` would have concluded -- not
    /// pictographic, not a consonant, not a linker, not an extend -- with the
    /// `in_run` arms kept because they depend on state rather than on a lookup.
    #[inline(always)]
    fn advance_plain(&mut self, cur: GraphemeClass) {
        use GraphemeClass::*;
        self.prev = Some(cur);
        self.ri = self.ri.after(cur);
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
        self.rule(cur, lb).is_none()
    }

    /// **The rule that joins `cur` to the cluster before it**, or `None` at
    /// a boundary: the extended rules in `[[source: uax29]]`'s order, the
    /// first that matches deciding, as [`ClusterRules::is_break_by_rules`]
    /// decides.
    #[inline(always)]
    pub fn rule(&self, cur: GraphemeClass, lb: Lookback) -> Option<Join> {
        use GraphemeClass::*;
        // GB1: sot ÷ Any -- there *is* a boundary before the first codepoint,
        // which is what the leading `÷` in every conformance case means.
        let prev = self.prev?;
        // GB3: CR × LF
        if prev == Cr && cur == Lf {
            return Some(Join::Both);
        }
        // GB4: (Control | CR | LF) ÷
        if matches!(prev, Control | Cr | Lf) {
            return None;
        }
        // GB5: ÷ (Control | CR | LF)
        if matches!(cur, Control | Cr | Lf) {
            return None;
        }
        // GB6/7/8: Hangul syllable sequences.
        if prev == L && matches!(cur, L | V | Lv | Lvt) {
            return Some(Join::Both);
        }
        if matches!(prev, Lv | V) && matches!(cur, V | T) {
            return Some(Join::Both);
        }
        if matches!(prev, Lvt | T) && cur == T {
            return Some(Join::Both);
        }
        // GB9: × (Extend | ZWJ).
        if matches!(cur, Extend | Zwj) {
            return Some(Join::Both);
        }
        // GB9a: × SpacingMark.
        if cur == SpacingMark {
            return Some(Join::SpacingMark);
        }
        // GB9b: Prepend ×.
        if prev == Prepend {
            return Some(Join::Prepend);
        }
        // GB9c: an Indic conjunct stays whole.
        if self.incb == IncbLookback::ConsonantLinker && lb.consonant() {
            return Some(Join::Conjunct);
        }
        // GB11: ExtPict Extend* ZWJ × ExtPict
        if self.pict == PictLookback::PictographicZwj && lb.pictographic() {
            return Some(Join::Both);
        }
        // GB12/13: RI × RI, pairwise.
        if cur == RegionalIndicator && self.ri == RiParity::Odd {
            return Some(Join::Both);
        }
        // GB999: Any ÷ Any
        None
    }

    /// **The variant `cells`' own machine**, a step of it: the rules
    /// `[[source: uax29]]` § 3.1 gives that variant -- the extended ones
    /// without GB9a, GB9b and GB9c for legacy clusters, and without GB9b and
    /// GB9c where spacing marks join -- the first that matches deciding, and
    /// the state advanced by the decision. The definition the parted
    /// extended machine ([`ClusterBreak::parts`]) answers to; tooling and
    /// tests only.
    pub fn is_break_in(&mut self, cells: ClusterCells, cur: GraphemeClass, lb: Lookback) -> bool {
        let brk = match self.rule(cur, lb) {
            None => true,
            Some(j) => !j.kept_by(cells) && !self.later_rule_joins(cur, lb),
        };
        match lb.is_none() {
            false => self.advance(cur, lb),
            true => self.advance_plain(cur),
        }
        brk
    }

    /// Whether a rule after GB9c joins `cur` -- GB11, GB12/13 -- for a
    /// variant that dropped the rule that matched first.
    fn later_rule_joins(&self, cur: GraphemeClass, lb: Lookback) -> bool {
        (self.pict == PictLookback::PictographicZwj && lb.pictographic())
            || (cur == GraphemeClass::RegionalIndicator && self.ri == RiParity::Odd)
    }

    #[inline(always)]
    fn advance(&mut self, cur: GraphemeClass, lb: Lookback) {
        use GraphemeClass::*;
        self.prev = Some(cur);

        // **Only reached when the codepoint is in a lookback table.**
        // `is_break` splits on `Lookback::is_none`, so the guards this used to
        // open with -- `cp < 0xa9`, then a bitmap probe -- are both dead here:
        // `advance_plain` handles everything they used to send home early, and
        // it is inlined.

        self.ri = self.ri.after(cur);

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
