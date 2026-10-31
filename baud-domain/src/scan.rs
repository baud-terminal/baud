use crate::StreamByte;
use core::marker::PhantomData;
use core::simd::Simd;
use core::simd::cmp::{SimdPartialEq, SimdPartialOrd};
use core::simd::num::SimdUint as _;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::*;

/// The result of a SIMD movemask: one bit per lane, set where the lane matched.
///
/// Typed because two different quantities were being carried as bare integers
/// here and they are trivially swappable: the *mask* is a bitset over lanes,
/// and `trailing_zeros()` of it is a lane *index*. Adding a mask to a byte
/// offset compiles; adding a lane index to one is what the code means. Six
/// sites across three scanners did this with `as u32` and `as usize`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LaneMask(pub(crate) u32);

impl LaneMask {
    /// The raw lane bits, for a caller matching a *pattern* across lanes
    /// rather than finding the first set one. `RouteBlock::stride_three` is
    /// the caller; nothing else should need this.
    pub fn bits(self) -> u32 {
        self.0
    }
    /// From a `movemask` intrinsic or a portable `Mask::to_bitmask`.
    pub fn from_movemask(bits: u32) -> LaneMask {
        LaneMask(bits)
    }
    pub fn any(self) -> bool {
        self.0 != 0
    }
    /// The first matching lane, if any. The only way to get an index out.
    pub fn first(self) -> Option<Lane> {
        self.any().then(|| Lane(self.0.trailing_zeros() as usize))
    }

    /// The first four characters, from the offset table.
    ///
    /// **One load replaces the prefix sum**, which is the whole reason this
    /// exists. Computing the offsets instead -- five `trailing_zeros` and a
    /// loop -- is a serial dependency chain in the middle of the vector path,
    /// and it measured 2.4x slower than the const-stride collector it was
    /// meant to replace.
    #[inline(always)]
    pub fn table_four(self) -> Option<CharStarts> {
        let v = LEAD_OFFSETS[(self.0 as usize) & ((1 << TABLE_BITS) - 1)];
        if v >> 16 == 0 {
            return None;
        }
        let o = [
            0usize,
            (v & 0xf) as usize,
            ((v >> 4) & 0xf) as usize,
            ((v >> 8) & 0xf) as usize,
            ((v >> 12) & 0xf) as usize,
        ];
        let mut starts = [Lane(0); START_COUNT];
        let mut lens = [ByteLen::ZERO; START_COUNT];
        let mut k = 0usize;
        while k < START_COUNT {
            starts[k] = Lane(o[k]);
            lens[k] = ByteLen::from_index(o[k + 1] - o[k]);
            k += 1;
        }
        Some(CharStarts { starts, lens })
    }

    /// The first four characters this lead mask describes, **starting at lane
    /// zero**.
    ///
    /// Needs **five** set lanes, not four: a character's length is the gap to
    /// the next lead, so the fifth boundary is what makes the fourth length
    /// known. Fewer than five means the block ends mid-character and the
    /// caller falls back to the scalar decoder, which owns that case.
    ///
    /// **Lane zero must be a lead**, and that is a correctness requirement
    /// rather than a convenience. Without it a block whose first byte is a
    /// continuation -- a slice taken mid-character, which happens on every
    /// read boundary -- would silently skip forward to the next boundary and
    /// decode a character the scalar decoder refuses at that position. The
    /// agreement test found exactly that.
    pub fn first_four(self) -> Option<CharStarts> {
        if self.0 & 1 == 0 {
            return None;
        }
        let mut bits = self.0;
        let mut at = [0usize; START_COUNT + 1];
        for slot in at.iter_mut() {
            if bits == 0 {
                return None;
            }
            let p = bits.trailing_zeros() as usize;
            *slot = p;
            bits &= bits - 1;
        }
        let mut starts = [Lane(0); START_COUNT];
        let mut lens = [ByteLen::ZERO; START_COUNT];
        for k in 0..START_COUNT {
            let n = at[k + 1] - at[k];
            // **The gap between leads is a character length only when the
            // bytes are well formed.** Stray continuation bytes -- which a PTY
            // delivers as readily as text -- make it arbitrarily large, and no
            // UTF-8 sequence exceeds four. Refusing here keeps
            // `CharStarts::len` in `1..=4` by construction, which is what lets
            // a caller size a gather buffer from it. A property test found
            // this as an out-of-bounds write on the first run.
            if n > 4 {
                return None;
            }
            starts[k] = Lane(at[k]);
            lens[k] = ByteLen::from_index(n);
        }
        Some(CharStarts { starts, lens })
    }
}

/// Which lane of a SIMD block matched. Not a byte length and not a byte
/// position -- it only becomes either by being added to the block's base.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lane(pub(crate) usize);

impl Lane {
    /// This lane's position in the block.
    ///
    /// Distinct from `offset_from`, which adds a scan's base. This is the one
    /// to use when indexing *within* the block that produced the mask.
    pub fn index(self) -> usize {
        self.0
    }

    /// The byte length from the start of a scan to this lane, given how many
    /// bytes were already consumed by earlier blocks.
    ///
    /// The addition lives here so a caller cannot write `lane + something_else`
    /// and get a plausible-looking number.
    pub fn offset_from(self, consumed: usize) -> ByteLen {
        ByteLen::from_index(consumed + self.0)
    }
}

/// How many characters a decode step produced. Never a bare count.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct LaneCount(usize);

impl LaneCount {
    pub const ZERO: LaneCount = LaneCount(0);
    pub fn of(n: usize) -> LaneCount {
        LaneCount(n)
    }
    pub fn index(self) -> usize {
        self.0
    }
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

/// How many bits of the lead mask the offset table is indexed by.
///
/// Twelve, and that is forced: four characters of three-byte text span twelve
/// bytes, so anything narrower cannot describe four CJK characters and the
/// step degrades to two.
const TABLE_BITS: usize = 12;

/// Four character offsets and a count, packed, per lead-mask pattern.
///
/// **16 KB, and the working set is under one.** The published decoder stores a
/// sixteen-byte shuffle per pattern -- 64 KB, which this project's second rule
/// refuses. Storing the four *offsets* instead is four bytes a pattern, and
/// the shuffle is rebuilt from them with two vector operations rather than the
/// serial chain that made the computed version lose.
///
/// The residency claim is measured, not assumed: over `cat-cjk.raw` only
/// **212 of the 4096 patterns occur at all**, the three phases of three-byte
/// alignment account for 46% of positions, and the touched set is **~848
/// bytes**. Real text has almost no entropy in this mask.
const fn lead_entry(m: usize) -> u32 {
    // Lane zero must be a lead, or the block starts mid-character.
    if m & 1 == 0 {
        return 0;
    }
    let (mut offs, mut n, mut i) = ([0u8; 5], 0usize, 0usize);
    while i < TABLE_BITS && n < 5 {
        if (m >> i) & 1 == 1 {
            offs[n] = i as u8;
            n += 1;
        }
        i += 1;
    }
    // Five boundaries describe four lengths. Fewer means the block ends
    // mid-character, which the scalar decoder owns.
    if n < 5 {
        return 0;
    }
    let mut k = 0usize;
    while k < 4 {
        let l = offs[k + 1] - offs[k];
        // No UTF-8 sequence is longer than four bytes; a wider gap is stray
        // continuation bytes, which the scalar path handles.
        if l == 0 || l > 4 {
            return 0;
        }
        k += 1;
    }
    (offs[1] as u32)
        | ((offs[2] as u32) << 4)
        | ((offs[3] as u32) << 8)
        | ((offs[4] as u32) << 12)
        | (4u32 << 16)
}

const fn lead_table() -> [u32; 1 << TABLE_BITS] {
    let mut t = [0u32; 1 << TABLE_BITS];
    let mut m = 0usize;
    while m < (1 << TABLE_BITS) {
        t[m] = lead_entry(m);
        m += 1;
    }
    t
}

/// The most table this project will carry on a per-byte path.
///
/// **The second rule as a compile-time constraint, not a comment.** Structures
/// touched per byte must stay resident; the smallest L1 data cache baud
/// targets is 32 KB, and a table larger than half of it would evict the
/// working set it is supposed to accelerate. The published decoder's 64 KB
/// shuffle table fails this by four times, which is why the offsets are stored
/// and the shuffle rebuilt.
pub const PER_BYTE_TABLE_BUDGET: usize = 16 * 1024;

/// Built at compile time; there is no initialisation and nothing to relocate.
static LEAD_OFFSETS: [u32; 1 << TABLE_BITS] = lead_table();

// The budget is checked, not asserted in prose. Widening `TABLE_BITS` past
// twelve stops the build rather than quietly costing cache.
const _: () = assert!(
    core::mem::size_of::<[u32; 1 << TABLE_BITS]>() <= PER_BYTE_TABLE_BUDGET,
    "the lead-offset table exceeds the per-byte cache budget"
);

/// How many characters `CharStarts` describes.
pub const START_COUNT: usize = 4;

/// Where the first few characters of a block begin, and how long each is.
///
/// **Derived from the lead mask alone.** A character's length is the distance
/// to the next lead byte, so this needs no table and no per-character branch:
/// UTF-8's self-synchronisation puts boundaries in the bytes themselves. That
/// is the property a class-agnostic decoder rests on.
#[derive(Clone, Copy)]
pub struct CharStarts {
    starts: [Lane; START_COUNT],
    lens: [ByteLen; START_COUNT],
}

impl CharStarts {
    /// Where character `k` begins, as a lane of the block.
    pub fn start(self, k: usize) -> Lane {
        self.starts[k]
    }
    /// How many bytes character `k` occupies. Always `1..=4`.
    pub fn len(self, k: usize) -> ByteLen {
        self.lens[k]
    }
    /// Total bytes the described characters occupy.
    pub fn bytes(self) -> ByteLen {
        let mut n = 0usize;
        for l in self.lens.iter() {
            n += l.index();
        }
        ByteLen::from_index(n)
    }
}

/// A lane mask in aarch64's **nibble** encoding, as `shrn #4` produces it.
///
/// **A different encoding, not a different meaning, and that is exactly why it
/// is a different type.** `[[source: arm-neon-bitmask]]` shows that NEON's
/// cheapest `movemask` equivalent is `shrn` -- read the 128-bit compare result
/// as eight 16-bit lanes, shift right by four and narrow to eight bytes, which
/// leaves a 64-bit value carrying **four bits per input byte** rather than one.
///
/// So the first set lane is `trailing_zeros() / 4`, not `trailing_zeros()`.
/// Handing that `u64` to `LaneMask` would compile and be wrong by a factor of
/// four -- silently, and only on inputs where the match is not in lane zero.
/// Both types produce a `Lane` and neither lets an index out any other way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NibbleMask(u64);

impl NibbleMask {
    /// From `vshrn_n_u16(cmp, 4)` reinterpreted as a `u64`.
    pub fn from_shrn(bits: u64) -> NibbleMask {
        NibbleMask(bits)
    }
    pub fn any(self) -> bool {
        self.0 != 0
    }
    /// The first matching lane, if any. Four bits a lane, so the count of
    /// trailing zeros is four times the index.
    pub fn first(self) -> Option<Lane> {
        self.any()
            .then(|| Lane(self.0.trailing_zeros() as usize / 4))
    }
    /// How many leading lanes match, which is what a *run* asks rather than
    /// what a *search* does.
    ///
    /// The counterpart of [`NibbleMask::first`] and the reason it is here
    /// rather than spelled at the call site: a matching lane is the nibble
    /// `0xf`, so four trailing one-bits are one lane, and dividing by anything
    /// but four reads a plausible wrong length. `LaneMask` answers the same
    /// question with `trailing_ones()` undivided, and the two encodings
    /// differing by exactly this factor is how the bug would look.
    pub fn leading_run(self) -> usize {
        self.0.trailing_ones() as usize / 4
    }
}

/// How many `StreamByte`s one vector holds -- one register.
///
/// Sixteen and not more on purpose. This is the whole working set of a scan
/// step, and the rule this project is built on is that structures touched per
/// byte stay resident; a wider block would not change what is resident but a
/// *table* indexed by the block would, which is why the UTF-8 operations below
/// compute rather than look up.
pub const LANE_COUNT: usize = 16;

/// A block of `StreamByte`s as a vector, tagged with what it has been proven
/// to hold.
///
/// **The register counterpart of [`Bytes`].** `Bytes<K>` is a byte slice
/// carrying a proof of its contents; this is sixteen of those bytes in a
/// vector, carrying the same proof. Same `Kind`, same markers, same
/// `Kind::ENCODING` bridge to the runtime `Encoding` -- there is deliberately
/// no second vocabulary for this, and an earlier draft of this type invented
/// one and collided with the domain three times before the existing one was
/// found.
///
/// The tag is a type and not a field because the *operations* differ:
/// `leads()` is meaningful on UTF-8 and meaningless on printable ASCII, so it
/// lives on `Vectorized<Utf8>` and asking for it on `Vectorized<Printable>`
/// does not compile. A runtime tag would put every method on one type, allow
/// the wrong call, and cost a branch in a loop measured in single percent.
///
/// **The three-byte path is UTF-8's, and its type says so.** The two stride
/// scans, the route map and the four-character decode ask about leads and
/// continuations, which printable ASCII (`0x20..=0x7e`) never holds, and
/// `baud-core` calls them only from `Act::Char3` (`stride_four_run` not at
/// all), an edge only the UTF-8 graph has: under ISO 8859-1 a high byte is a
/// C1 control or a character of its own. They sat on `Vectorized<Printable>`,
/// a tag that said the opposite of the bytes, and now live on
/// `Vectorized<Utf8>` beside `leads()`. The tag names how the bytes are read,
/// not that they are well formed -- these operations are what check that. On
/// a printable block each one is refused:
///
/// ```
/// # use baud_domain::*;
/// let b = Vectorized::<Utf8>::load(StreamByte::slice_from("世界世界世界".as_bytes()));
/// assert_eq!((b.stride_three_run(), b.stride_four_run()), (5, 0));
/// assert_eq!(b.stride_three_quad()[1], Codepoint::new(0x754c));
/// assert_eq!(b.routes().stride_three(), 5);
/// ```
///
/// ```compile_fail,E0599
/// # use baud_domain::*;
/// let b = Vectorized::<Printable>::load(StreamByte::slice_from("世界世界世界".as_bytes()));
/// b.stride_three_run();
/// ```
///
/// ```compile_fail,E0599
/// # use baud_domain::*;
/// let b = Vectorized::<Printable>::load(StreamByte::slice_from("世界世界世界".as_bytes()));
/// b.stride_four_run();
/// ```
///
/// ```compile_fail,E0599
/// # use baud_domain::*;
/// let b = Vectorized::<Printable>::load(StreamByte::slice_from("世界世界世界".as_bytes()));
/// b.stride_three_quad();
/// ```
///
/// ```compile_fail,E0599
/// # use baud_domain::*;
/// let b = Vectorized::<Printable>::load(StreamByte::slice_from("世界世界世界".as_bytes()));
/// b.routes();
/// ```
///
/// `repr(transparent)` over the vector, so this costs nothing: the newtypes in
/// this crate lower to the same instructions, measured on the ingest path with
/// every range overlapping.
///
/// **The point is that a scanner never writes `as`.** Every scanner here used
/// to build a raw `Simd<u8, 16>`, compare it, call `to_bitmask()` and cast the
/// result `as u32` -- and that cast is where an intrinsic's return value enters
/// the type system, which is the boundary the audit exists to hold.
/// **Zerocopy, like every other wire-facing type here.** The lanes are stored
/// as an array rather than a `Simd` so the type can derive `FromBytes` and
/// `IntoBytes`: a block of stream bytes *is* sixteen bytes, and reinterpreting
/// the arena as one must not need a copy or a cast. `Simd::from_array` on the
/// way into an operation is free -- it is a register move the optimiser
/// removes -- and the tag is a `PhantomData`, so the whole struct is exactly
/// its sixteen bytes.
#[repr(transparent)]
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct Vectorized<K: Kind> {
    v: [u8; LANE_COUNT],
    _kind: PhantomData<K>,
}

// `Clone` and `Copy` by hand: the markers are uninhabited enums, so a derive
// would demand `K: Copy` and no `Kind` can satisfy it. The tag is a
// `PhantomData` and the payload is sixteen bytes, so copying is trivially
// sound however uninhabited the tag is.
impl<K: Kind> Clone for Vectorized<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: Kind> Copy for Vectorized<K> {}

impl<K: Kind> Vectorized<K> {
    /// The lanes as a vector. Free: a register move the optimiser removes.
    #[inline(always)]
    fn lanes(self) -> Simd<u8, LANE_COUNT> {
        Simd::from_array(self.v)
    }

    /// Load one block. The caller has checked there are `LANE_COUNT` bytes.
    #[inline(always)]
    pub fn load(buf: &[StreamByte]) -> Vectorized<K> {
        let mut v = [0u8; LANE_COUNT];
        v.copy_from_slice(&StreamByte::slice_as_bytes(buf)[..LANE_COUNT]);
        Vectorized {
            v,
            _kind: PhantomData,
        }
    }
}

impl Vectorized<Utf8> {
    /// How many consecutive three-byte characters this block begins with,
    /// **without building a route map**.
    ///
    /// `routes()` costs a `tbl` and the `7F` correction, and `stride_three`
    /// then takes **two** lane-mask extractions off it -- and aarch64 has no
    /// `movemask`, so each is a shift-narrow plus a move across register
    /// files. None of that is needed to answer this question: a three-byte
    /// lead is `(b & 0xF0) == 0xE0` and a continuation is `(b & 0xC0) == 0x80`,
    /// both direct compares on the raw bytes. Selecting between them by lane
    /// position gets the whole answer into **one** extraction.
    ///
    /// The two forms may disagree by design and
    /// `the_direct_stride_scan_never_overcounts` is what pins the contract:
    /// where a lead sits at a position that should hold a continuation, the
    /// route form returns 0 and this returns the characters before it. Both
    /// are safe -- the scalar tail continues from wherever the vector stops --
    /// and the totals agree, which `cells_written` checks.
    #[inline(always)]
    pub fn stride_three_run(self) -> usize {
        use core::simd::Mask;
        let v = self.lanes();
        let is_lead = (v & Simd::splat(0xf0)).simd_eq(Simd::splat(0xe0));
        let is_cont = (v & Simd::splat(0xc0)).simd_eq(Simd::splat(0x80));
        // Lanes 0, 3, 6, 9, 12 must lead; every other lane must continue.
        const AT_LEAD: [bool; LANE_COUNT] = [
            true, false, false, true, false, false, true, false, false, true, false, false, true,
            false, false, false,
        ];
        let at_lead: Mask<i8, LANE_COUNT> = Mask::from_array(AT_LEAD);
        let ok = (at_lead & is_lead) | (!at_lead & is_cont);
        // Leading run of correct lanes; three lanes to a character.
        (ok.to_bitmask().trailing_ones() as usize / 3).min(5)
    }

    /// How many complete **four-byte** characters lead this block, at most four.
    ///
    /// **Sixteen bytes is exactly four four-byte characters**, where the
    /// stride-three form gets five and a spare lane -- so this one has no
    /// ragged edge and its cap is the register rather than a choice.
    ///
    /// It exists because `Act::Char4` is **37.8% of `cat-cjk.raw`'s
    /// iterations** and 50% of its runs by encoded length, and until this
    /// landed `run_of` vector-scanned **only** stride three: every four-byte
    /// run went through the scalar loop at four table lookups a character. The
    /// comment there still said three-byte was "81% of cat-cjk", which was
    /// true of characters before clustering and is not true of runs now.
    pub fn stride_four_run(self) -> usize {
        use core::simd::Mask;
        let v = self.lanes();
        // A four-byte lead is `11110xxx`; a continuation is `10xxxxxx`.
        let is_lead = (v & Simd::splat(0xf8)).simd_eq(Simd::splat(0xf0));
        let is_cont = (v & Simd::splat(0xc0)).simd_eq(Simd::splat(0x80));
        // Lanes 0, 4, 8, 12 must lead; every other lane must continue.
        const AT_LEAD: [bool; LANE_COUNT] = [
            true, false, false, false, true, false, false, false, true, false, false, false, true,
            false, false, false,
        ];
        let at_lead: Mask<i8, LANE_COUNT> = Mask::from_array(AT_LEAD);
        let ok = (at_lead & is_lead) | (!at_lead & is_cont);
        // Leading run of correct lanes; four lanes to a character.
        (ok.to_bitmask().trailing_ones() as usize / 4).min(4)
    }
}

/// Which route a byte takes out of a ground node.
///
/// **This is the graph's ground row, reduced to what a block can answer.** The
/// parse graph is indexed by `(node, byte)` and gives a full edge; a *block*
/// classifier cannot carry a node, because the node changes within the block.
/// What it can answer is the question every byte of ground-state input poses --
/// which route starts here and how many bytes it consumes -- and that is a
/// function of the byte alone.
///
/// **It is a function of the high nibble alone, with one correction**, and
/// that is the fact that makes it one instruction: a nibble-indexed shuffle
/// (`vqtbl1q_u8`, `pshufb`) is a 16-entry table applied to 16 bytes at once,
/// and there are exactly 16 high nibbles. `0x7f` is the correction, because it
/// shares a nibble with printable ASCII and is not printable -- the same
/// `0x7f` that once went missing from the AVX2 stop mask.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Route {
    /// `00..1F`: a C0 control, which executes or begins a sequence.
    Control = 0,
    /// `20..7E`: printable ASCII. The only route the run scanner can take.
    Text = 1,
    /// `7F`: neither text nor control. Its own route because it is the
    /// exception the nibble cannot express.
    Del = 2,
    /// `80..BF`: a UTF-8 continuation, or a C1 control outside UTF-8.
    Cont = 3,
    /// `C0..DF`: a two-byte lead.
    Lead2 = 4,
    /// `E0..EF`: a three-byte lead -- CJK, Hangul, most of the BMP.
    Lead3 = 5,
    /// `F0..FF`: a four-byte lead -- the astral planes, which is where the
    /// emoji are and therefore where the cluster machine is needed.
    Lead4 = 6,
}

impl Route {
    /// How many bytes the character starting here occupies, or zero when the
    /// byte does not start one.
    pub const fn width(self) -> usize {
        match self {
            Route::Lead2 => 2,
            Route::Lead3 => 3,
            Route::Lead4 => 4,
            Route::Cont => 0,
            _ => 1,
        }
    }

    /// The route for one byte, scalar. **The definition the block classifier
    /// answers to**, exhaustively, over all 256 bytes.
    pub const fn of(b: StreamByte) -> Route {
        match b.raw_byte() {
            0x00..=0x1f => Route::Control,
            0x20..=0x7e => Route::Text,
            0x7f => Route::Del,
            0x80..=0xbf => Route::Cont,
            0xc0..=0xdf => Route::Lead2,
            0xe0..=0xef => Route::Lead3,
            _ => Route::Lead4,
        }
    }

    /// The 16-entry table the shuffle uses, indexed by the high nibble.
    const BY_NIBBLE: [u8; 16] = [
        Route::Control as u8,
        Route::Control as u8,
        Route::Text as u8,
        Route::Text as u8,
        Route::Text as u8,
        Route::Text as u8,
        Route::Text as u8,
        Route::Text as u8, // 7x, with 0x7f corrected below
        Route::Cont as u8,
        Route::Cont as u8,
        Route::Cont as u8,
        Route::Cont as u8,
        Route::Lead2 as u8,
        Route::Lead2 as u8,
        Route::Lead3 as u8,
        Route::Lead4 as u8,
    ];
}

/// Sixteen bytes' routes, in one register.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RouteBlock(Simd<u8, LANE_COUNT>);

impl RouteBlock {
    /// Lanes whose route is exactly `r`.
    #[inline(always)]
    pub fn is(self, r: Route) -> LaneMask {
        LaneMask::from_movemask(self.0.simd_eq(Simd::splat(r as u8)).to_bitmask() as u32)
    }

    /// Lanes that begin a multi-byte character, of any length.
    ///
    /// **One compare for all three lead lengths**, which the route's ordering
    /// is chosen to allow: `Lead2`, `Lead3` and `Lead4` are the three largest
    /// values, so "is a lead" is `route >= Lead2`. That matters because
    /// `to_bitmask` has no single instruction on aarch64 -- it lowers to a
    /// shift-narrow and a move across register files -- so the mask
    /// extractions, not the classification, are what a block walk spends its
    /// time on. Six of them a block was 40% of the per-character budget.
    ///
    /// Which length is a question for the *run*, not for every lane, and one
    /// scalar test on the lead byte answers it.
    #[inline(always)]
    pub fn leads(self) -> LaneMask {
        LaneMask::from_movemask(self.0.simd_ge(Simd::splat(Route::Lead2 as u8)).to_bitmask() as u32)
    }

    /// Lanes that are **not** `r` -- for finding where a run of one route ends.
    #[inline(always)]
    pub fn is_not(self, r: Route) -> LaneMask {
        LaneMask::from_movemask(self.0.simd_ne(Simd::splat(r as u8)).to_bitmask() as u32)
    }

    /// How many complete **three-byte** characters lead this block.
    ///
    /// **This is what a block loop needs and a homogeneity gate cannot give
    /// it.** The bulk writers key on a run of one character *class*, which
    /// `cat-cjk.raw` essentially never has -- its median wide run is one. A run
    /// of one *stride* is a far weaker condition and a far commoner one: every
    /// character in the BMP above `U+07FF` is three bytes, so CJK, Hangul,
    /// Devanagari and box drawing all qualify together.
    ///
    /// Three bytes rather than two or four because it is the common case by a
    /// wide margin -- 81% of `cat-cjk.raw` is on the UTF-8 route and almost
    /// all of it is three-byte -- and because a four-byte lead is where the
    /// emoji are, which need the cluster machine one codepoint at a time.
    ///
    /// Returns 0 when the block does not begin with one, so a caller can fall
    /// through without a second test.
    pub fn stride_three(self) -> usize {
        // Lanes 0, 3, 6, 9, 12 -- where a lead must sit for the stride to be
        // three all the way.
        const LEADS: u32 = 0b0001_0010_0100_1001;
        let leads = self.is(Route::Lead3).bits();
        let conts = self.is(Route::Cont).bits();
        // The first lane that is neither a three-byte lead nor a continuation
        // bounds the run; five characters is all sixteen lanes can hold.
        let covered = (!(leads | conts) & 0xffff).trailing_zeros() as usize;
        let k = (covered / 3).min(5);
        if k == 0 {
            return 0;
        }
        let span = (1u32 << (3 * k)) - 1;
        // Every lead exactly on a multiple of three, and nothing else a lead.
        match leads & span == LEADS & span {
            true => k,
            false => 0,
        }
    }

    /// The route of one lane, for the scalar tail and for tests.
    #[inline(always)]
    pub fn at(self, i: usize) -> Route {
        ROUTE_FROM_INDEX[self.0[i] as usize]
    }
}

/// How many bytes the character starting at this byte occupies, for all 256 of
/// them. **Zero means the byte cannot start one** -- a continuation.
///
/// `Route::of` is a `const fn` and answers the same question, but it is a
/// seven-way range match. The run scanner read this per byte of a run until it
/// asked Table 3-7's `utf8_width`, which refuses `C0 C1 F5-FF`. A table is
/// one load whose address depends on the byte alone, so it issues immediately
/// and never joins the loop-carried chain --
/// `[[source: regex-automata-dense]]` adds exactly such a load for byte classes
/// and calls the cost negligible.
///
/// `the_route_width_table_agrees_with_the_routes` checks all 256 against
/// `Route::of(..).width()`, which stays as the definition this answers to.
pub static ROUTE_WIDTH: [u8; 256] = route_widths();

const fn route_widths() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        t[i] = Route::of(StreamByte::ALL[i]).width() as u8;
        i += 1;
    }
    t
}

/// The character length this byte claims, or zero if it continues one.
#[inline(always)]
pub fn lead_width(b: StreamByte) -> usize {
    ROUTE_WIDTH[b.index()] as usize
}

/// Index back to the enum, so `RouteBlock::at` needs no cast in the caller.
static ROUTE_FROM_INDEX: [Route; 7] = [
    Route::Control,
    Route::Text,
    Route::Del,
    Route::Cont,
    Route::Lead2,
    Route::Lead3,
    Route::Lead4,
];

impl Vectorized<Utf8> {
    /// **Four three-byte characters, decoded in vectors.**
    ///
    /// The route map has already established that this block begins with four
    /// back-to-back `1110xxxx 10yyyyyy 10zzzzzz` characters, so every byte's
    /// position is a *compile-time constant*: the leads are lanes 0, 3, 6, 9
    /// and their continuations follow. That is what makes the swizzle indices
    /// constants and the whole decode branchless.
    ///
    /// **This is the step the block classifier was built to enable.** Finding
    /// character boundaries is the part of a UTF-8 decoder that resists
    /// vectorising -- it is a prefix sum over the lead mask, and computing it
    /// per block is what made two previous attempts *slower* than decoding one
    /// character at a time. Here the boundaries are not computed at all: the
    /// stride is three, so they are known before the load.
    ///
    /// Returns codepoints, not raw values: at this length the shortest-form
    /// floor is the constant `U+0800`, so the whole validity question --
    /// overlong, surrogate, out of range -- is answerable here and does not
    /// have to be handed back with the bits. `Codepoint::decoded` is the
    /// surrogate and range half, spelled once in the domain.
    ///
    /// **`None` for a lane that is ill-formed**, where an overlong or a
    /// surrogate decoded to one replacement: the run writer ends the run
    /// there, and the bytes are taken again one maximal subpart at a time
    /// (`[[source: unicode-ch3]]` 3.9.6), which for these is one a byte.
    #[inline(always)]
    pub fn stride_three_quad(self) -> [Option<Codepoint>; 4] {
        use core::simd::simd_swizzle;
        let v = self.lanes();
        // Lane `3i` leads character `i`; `3i+1` and `3i+2` continue it.
        let lead: Simd<u8, 4> = simd_swizzle!(v, [0, 3, 6, 9]);
        let c1: Simd<u8, 4> = simd_swizzle!(v, [1, 4, 7, 10]);
        let c2: Simd<u8, 4> = simd_swizzle!(v, [2, 5, 8, 11]);
        let lead = lead.cast::<u32>() & Simd::splat(0x0f);
        let c1 = c1.cast::<u32>() & Simd::splat(0x3f);
        let c2 = c2.cast::<u32>() & Simd::splat(0x3f);
        let raw = (lead << Simd::splat(12)) | (c1 << Simd::splat(6)) | c2;
        // Below `U+0800` a three-byte form is overlong -- `E0 80 AF` renders
        // `/` without this, which is the path-traversal overlong -- and
        // `ED A0..BF` spells a surrogate; `from_three` refuses both, which
        // are Table 3-7's two narrowed second bytes for `E0` and `ED`. Past
        // `U+10FFFF` a three-byte form cannot reach.
        let v = raw.to_array();
        [
            Codepoint::from_three(v[0]),
            Codepoint::from_three(v[1]),
            Codepoint::from_three(v[2]),
            Codepoint::from_three(v[3]),
        ]
    }

    /// **The whole ground row of the parse graph, for sixteen bytes, in one
    /// shuffle.**
    ///
    /// `swizzle_dyn` is `vqtbl1q_u8` on aarch64 and `pshufb` on x86-64: a
    /// 16-entry table applied to 16 indices at once. The indices are the high
    /// nibbles, which is why the table is 16 entries and not 256 -- the route
    /// is a function of the high nibble, and `0x7f` is the single exception,
    /// corrected with one compare and one select.
    ///
    /// This replaces asking a hand-written predicate per byte per question.
    /// `Scan::run`, `Scan::final_byte` and `Scan::st` each walk the block with
    /// their own compare; the routes answer all of them from one pass, and
    /// they answer the question those three cannot: **where each character
    /// starts and how long it is**, which is what a writer needs to take a run
    /// whose median length is one.
    #[inline(always)]
    pub fn routes(self) -> RouteBlock {
        let v = self.lanes();
        let mut tbl = [0u8; LANE_COUNT];
        let mut i = 0;
        while i < 16 {
            tbl[i] = Route::BY_NIBBLE[i];
            i += 1;
        }
        let r = Simd::from_array(tbl).swizzle_dyn(v >> Simd::splat(4));
        // `7F` shares the `7x` nibble with printable ASCII and is not text.
        // One compare and one blend, which is the whole correction.
        // **The `7F` correction, without a mask.** `Text` is 1 and `Del` is 2,
        // so the whole fix is "add one where the byte is `7F`" -- and
        // `1 - (b ^ 0x7F)` saturating *is* that indicator: the xor is zero
        // only at `7F`, where the subtract yields 1, and at least 1
        // everywhere else, where it saturates to 0. Two vector instructions
        // and no lane-mask conversion, which this toolchain's portable-simd
        // does not offer for `u8` anyway.
        let is_del = Simd::splat(1u8).saturating_sub(v ^ Simd::splat(0x7f));
        RouteBlock(r + is_del)
    }
}

impl Vectorized<Printable> {
    /// Lanes that are **not** printable ASCII: below `0x20`, `0x7f`, or above.
    ///
    /// One subtract and one unsigned compare: `b - 0x20 > 0x5e` is false
    /// exactly for `0x20..=0x7e`, so the pair of comparisons becomes one.
    #[inline(always)]
    pub fn not_printable(self) -> LaneMask {
        let v = self.lanes();
        let lo = Simd::splat(0x20);
        let span = Simd::splat(0x7e - 0x20);
        LaneMask::from_movemask((v - lo).simd_gt(span).to_bitmask() as u32)
    }

    /// How far a printable-ASCII run reaches, **extracting once per four
    /// vectors instead of once per vector**.
    ///
    /// aarch64 has no `movemask`. `to_bitmask` lowers to a shift-narrow and a
    /// move across register files, and that pair is most of what a
    /// sixteen-byte step costs: the compare itself is one instruction. Four
    /// compares OR together in vector registers for free, so a sixty-four-byte
    /// step pays **one** extraction rather than four, and only a block that
    /// actually ends the run pays the per-sixteen search.
    ///
    /// Measured against the one-at-a-time form: the scan was 10-17% of every
    /// text payload, priced by running it twice.
    ///
    /// Returns the length of the run, which is `s.len()` when it does not end
    /// inside the slice.
    #[inline(always)]
    pub fn printable_run(s: &[StreamByte]) -> usize {
        use core::simd::cmp::SimdOrd as _;
        use core::simd::num::SimdUint as _;
        let lo = Simd::splat(0x20u8);
        let mut n = 0usize;
        // **Sixteen at a time until the run has proven long.** The wide step
        // below is worth +7% where runs are a screen wide and **-5% on vim**,
        // whose runs are a few characters: a run that ends in the first block
        // must not pay for machinery aimed at one that does not. Four blocks
        // is the threshold, because that is where the wide step starts to have
        // anything to amortise.
        let narrow = (4 * LANE_COUNT).min(s.len());
        while n + LANE_COUNT <= narrow {
            if let Some(l) = Vectorized::<Printable>::load(&s[n..])
                .not_printable()
                .first()
            {
                return n + l.index();
            }
            n += LANE_COUNT;
        }
        // **Accumulated with `max`, not with a mask.** `b - 0x20` is `0..=0x5e`
        // exactly for a printable byte, so the largest such difference across
        // sixty-four bytes answers "were they all printable" -- and `umax` is
        // one instruction where a compare, a select and an or are three. Four
        // vectors accumulate in registers and pay **one** reduction, which on
        // aarch64 is the expensive half: there is no `movemask`, so extraction
        // is a shift-narrow and a move across register files.
        while n + 4 * LANE_COUNT <= s.len() {
            let mut acc = Simd::<u8, LANE_COUNT>::splat(0);
            let mut k = 0;
            while k < 4 {
                let v = Vectorized::<Printable>::load(&s[n + k * LANE_COUNT..]).lanes();
                acc = acc.simd_max(v - lo);
                k += 1;
            }
            if acc.reduce_max() > 0x7e - 0x20 {
                break;
            }
            n += 4 * LANE_COUNT;
        }
        while n + LANE_COUNT <= s.len() {
            if let Some(l) = Vectorized::<Printable>::load(&s[n..])
                .not_printable()
                .first()
            {
                return n + l.index();
            }
            n += LANE_COUNT;
        }
        while n < s.len() && s[n].is_printable_ascii() {
            n += 1;
        }
        n
    }

    /// Lanes holding a byte in `lo..=hi`, inclusive.
    #[inline(always)]
    pub fn in_range(self, lo: StreamByte, hi: StreamByte) -> LaneMask {
        let v = self.lanes();
        let l = v.simd_ge(Simd::splat(lo.0));
        let h = v.simd_le(Simd::splat(hi.0));
        LaneMask::from_movemask((l & h).to_bitmask() as u32)
    }

    /// Lanes equal to either of two bytes -- the string-terminator test.
    #[inline(always)]
    pub fn either(self, a: StreamByte, b: StreamByte) -> LaneMask {
        let v = self.lanes();
        let x = v.simd_eq(Simd::splat(a.0));
        let y = v.simd_eq(Simd::splat(b.0));
        LaneMask::from_movemask((x | y).to_bitmask() as u32)
    }
}

impl Vectorized<Utf8> {
    /// Lanes that **begin** a character: anything that is not `10xxxxxx`.
    ///
    /// This is the whole of UTF-8's self-synchronisation in one instruction,
    /// and it is what makes a class-agnostic decoder possible: character
    /// boundaries are a property of the bytes, not of what they decode to, so
    /// finding them needs no table and no branch per character.
    #[inline(always)]
    pub fn leads(self) -> LaneMask {
        let cont = (self.lanes() & Simd::splat(0xc0)).simd_eq(Simd::splat(0x80));
        LaneMask::from_movemask((!cont).to_bitmask() as u32)
    }

    /// Lanes holding a byte no UTF-8 sequence may contain.
    ///
    /// `0xc0`, `0xc1` would only ever encode an overlong form and `0xf5`
    /// upwards is past U+10FFFF. A block containing one is refused outright
    /// rather than decoded, which keeps every conformance decision in the
    /// scalar path instead of duplicated per instruction set.
    #[inline(always)]
    pub fn impossible(self) -> LaneMask {
        let v = self.lanes();
        let a = v.simd_eq(Simd::splat(0xc0));
        let b = v.simd_eq(Simd::splat(0xc1));
        let c = v.simd_ge(Simd::splat(0xf5));
        LaneMask::from_movemask((a | b | c).to_bitmask() as u32)
    }

    /// Gather bytes by index, four per character, into one register.
    ///
    /// `swizzle_dyn` is a runtime shuffle -- `tbl` on aarch64, `pshufb` on
    /// x86-64 -- so the gather is *computed* rather than looked up. The
    /// textbook decoder indexes a shuffle table by the lead-byte pattern,
    /// which for sixteen bytes is tens of kilobytes; this project's second
    /// rule is that what is touched per byte stays in cache, and a table that
    /// size does not.
    ///
    /// Indices at or past `LANE_COUNT` read as zero, which is what pads a
    /// one-, two- or three-byte sequence out to four lanes.
    #[inline(always)]
    pub fn gather(self, index: Vectorized<Utf8>) -> Vectorized<Utf8> {
        Vectorized {
            v: self.lanes().swizzle_dyn(index.lanes()).to_array(),
            _kind: PhantomData,
        }
    }

    /// Gather four characters into four 32-bit groups, in two vector steps.
    ///
    /// **This is where the previous attempt lost.** Building the gather index
    /// with a nested scalar loop -- `idx[4k+j] = off[k]+j` -- put a serial
    /// chain inside the vector path and made the general decoder 2.4x slower
    /// than the const-stride collector. Here the same index is one broadcast
    /// and one add:
    ///
    /// * repeat each character's offset four times, a const swizzle;
    /// * add `[0,1,2,3]` per group, a const vector;
    /// * lanes past a character's length index past the block, and
    ///   `swizzle_dyn` reads those as zero, which is exactly the padding a
    ///   one-, two- or three-byte sequence needs.
    #[inline(always)]
    pub fn gather_chars(self, starts: CharStarts) -> Vectorized<Utf8> {
        use core::simd::num::SimdUint;
        use core::simd::simd_swizzle;
        let offs = Simd::<u8, 4>::from_array([
            starts.start(0).index() as u8,
            starts.start(1).index() as u8,
            starts.start(2).index() as u8,
            starts.start(3).index() as u8,
        ]);
        let lens = Simd::<u8, 4>::from_array([
            starts.len(0).index() as u8,
            starts.len(1).index() as u8,
            starts.len(2).index() as u8,
            starts.len(3).index() as u8,
        ]);
        const REPEAT: [usize; LANE_COUNT] = [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3];
        let base: Simd<u8, LANE_COUNT> = simd_swizzle!(offs, REPEAT);
        let span: Simd<u8, LANE_COUNT> = simd_swizzle!(lens, REPEAT);
        let step =
            Simd::<u8, LANE_COUNT>::from_array([0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3]);
        // Past the character's length, index out of range so the lane reads
        // zero rather than a neighbour's byte.
        // Branch-free, and saturating arithmetic rather than a mask so no
        // comparison trait is needed: `step + 1 - span` saturates to zero for
        // every lane inside the character and is positive for the padding.
        // Scaling that past the block makes `swizzle_dyn` read those lanes as
        // zero, which is the padding a one-, two- or three-byte sequence
        // needs. Bounded: three plus one, times sixteen, is well inside a byte.
        let over = (step + Simd::splat(1)).saturating_sub(span) * Simd::splat(LANE_COUNT as u8);
        let idx = base + step + over;
        Vectorized {
            v: self.lanes().swizzle_dyn(idx).to_array(),
            _kind: PhantomData,
        }
    }

    /// Build an index block from raw lane positions.
    #[inline(always)]
    pub fn indices(idx: [u8; LANE_COUNT]) -> Vectorized<Utf8> {
        Vectorized {
            v: idx,
            _kind: PhantomData,
        }
    }

    /// Decode four gathered characters to codepoints, in vectors, with no
    /// scalar step per character.
    ///
    /// **This is the half that was missing, and its absence is why the block
    /// decoder lost to the scalar one.** `gather_chars` vectorises finding the
    /// boundaries; everything after it went through `Codepoint::from_quad`
    /// once per lane, which re-parses the lead byte, re-checks each
    /// continuation and re-tests the shortest form -- the whole scalar decoder,
    /// four times, on top of the vector setup. Measured: the block path ran
    /// `payload::cjk` at 2250 against `wide_run`'s 2570, and a consumer built
    /// on it lost 21% of cat-cjk. `wide_run` is faster because it is vector
    /// *to the codepoint*: swizzle, range-compare, shift, or. This does the
    /// same for arbitrary lengths.
    ///
    /// Three properties of the gather make it possible:
    ///
    /// * **Padding is zero.** Bytes past a character's length read as zero, so
    ///   a one-byte character's continuation lanes contribute nothing without
    ///   being masked off.
    /// * **The shift is a function of the length.** `6 * (len - i - 1)`,
    ///   saturating, which is a vector subtract -- and `core::simd` shifts
    ///   elementwise by a vector.
    /// * **The length must be checked against the lead byte.** `CharStarts`
    ///   derives a length from the *distance to the next lead*, which is what
    ///   makes it class-agnostic and also means `E4 41` reports length 1 for a
    ///   byte that declares three. Decoding that as one byte would invent a
    ///   character. The lead's declared length is `1 + (b >= 0xc0) + (b >=
    ///   0xe0) + (b >= 0xf0)`, three compares, and it has to agree.
    ///
    /// Returns the codepoints and how many **leading** lanes are valid, so a
    /// character truncated by the end of the block hands back the prefix
    /// exactly as the scalar path did.
    #[inline(always)]
    pub fn decode(self, starts: CharStarts) -> ([Codepoint; START_COUNT], LaneCount) {
        use core::simd::Select;
        use core::simd::cmp::{SimdPartialEq, SimdPartialOrd};
        use core::simd::num::SimdUint;
        let q = Simd::<u32, START_COUNT>::from_array(self.quads());
        let byte = |i: u32| (q >> Simd::splat(i * 8)) & Simd::splat(0xff);
        let (b0, b1, b2, b3) = (byte(0), byte(1), byte(2), byte(3));
        let len = Simd::<u32, START_COUNT>::from_array([
            starts.len(0).index() as u32,
            starts.len(1).index() as u32,
            starts.len(2).index() as u32,
            starts.len(3).index() as u32,
        ]);

        // The lead byte must declare the length the boundaries implied.
        let one = Simd::<u32, START_COUNT>::splat(1);
        let zero = Simd::<u32, START_COUNT>::splat(0);
        let ge = |v: u32| b0.simd_ge(Simd::splat(v)).select(one, zero);
        let declared = Simd::splat(1) + ge(0xc0) + ge(0xe0) + ge(0xf0);
        let is_cont = b0.simd_ge(Simd::splat(0x80)) & b0.simd_lt(Simd::splat(0xc0));
        let mut ok = declared.simd_eq(len) & !is_cont;

        // Continuation bytes, checked only where the character has them.
        let cont_ok = |b: Simd<u32, START_COUNT>, i: u32| {
            let present = len.simd_gt(Simd::splat(i));
            let shaped = (b & Simd::splat(0xc0)).simd_eq(Simd::splat(0x80));
            !present | shaped
        };
        ok &= cont_ok(b1, 1) & cont_ok(b2, 2) & cont_ok(b3, 3);

        // `0xff >> (len + 1)` is the lead's payload mask for two, three and
        // four bytes; one byte is the exception and keeps seven bits.
        let wide_mask = Simd::splat(0xffu32) >> (len + Simd::splat(1));
        let mask = len
            .simd_eq(one)
            .select(Simd::<u32, START_COUNT>::splat(0x7f), wide_mask);

        // `6 * (len - i - 1)`, saturating: the padding lanes are zero, so a
        // shift of zero on them is harmless and needs no branch.
        let sh = |i: u32| (len.saturating_sub(Simd::splat(i + 1))) * Simd::splat(6);
        let cp = ((b0 & mask) << sh(0))
            | ((b1 & Simd::splat(0x3f)) << sh(1))
            | ((b2 & Simd::splat(0x3f)) << sh(2))
            | (b3 & Simd::splat(0x3f));

        // Shortest form, surrogates and the upper bound. `E0 80 AF` decoding
        // to `/` is the overlong this repo has shipped once.
        const MIN: [u32; 5] = [0, 0, 0x80, 0x800, 0x1_0000];
        let min = Simd::<u32, START_COUNT>::from_array([
            MIN[starts.len(0).index()],
            MIN[starts.len(1).index()],
            MIN[starts.len(2).index()],
            MIN[starts.len(3).index()],
        ]);
        ok &= cp.simd_ge(min);
        ok &= cp.simd_le(Simd::splat(0x10_ffff));
        ok &= !(cp.simd_ge(Simd::splat(0xd800)) & cp.simd_lt(Simd::splat(0xe000)));

        let n = (ok.to_bitmask() as u32).trailing_ones() as usize;
        let v = cp.to_array();
        let mut out = [Codepoint::REPLACEMENT; START_COUNT];
        for k in 0..n {
            match Codepoint::new(v[k]) {
                Some(c) => out[k] = c,
                // Unreachable given the checks above; refusing the lane is the
                // safe reading of a disagreement rather than trusting either.
                None => return (out, LaneCount::of(k)),
            }
        }
        (out, LaneCount::of(n))
    }

    /// The block as four 32-bit groups, one per character, little-endian.
    ///
    /// A plain array and not a `Simd`, so a consumer needs no `portable_simd`
    /// feature to read the result. The vector work stays behind this type.
    #[inline(always)]
    pub fn quads(self) -> [u32; START_COUNT] {
        // **Zerocopy, not a loop.** The gathered block *is* four 32-bit
        // groups; building them with `from_le_bytes` per group put a scalar
        // loop back in the vector path, which is the mistake this whole file
        // is about. `FromBytes` reinterprets the same sixteen bytes.
        #[cfg(target_endian = "little")]
        {
            <[u32; START_COUNT]>::read_from_bytes(&self.v).expect("sixteen bytes is four u32")
        }
        #[cfg(not(target_endian = "little"))]
        {
            let mut out = [0u32; START_COUNT];
            let b = self.v;
            for (k, o) in out.iter_mut().enumerate() {
                *o = u32::from_le_bytes([b[4 * k], b[4 * k + 1], b[4 * k + 2], b[4 * k + 3]]);
            }
            out
        }
    }
}

#[cfg(test)]
mod stride_direct {
    use crate::*;

    /// **The contract is "never claim a character that is not there".**
    ///
    /// The direct form and the route form may disagree on how much they take
    /// -- see `stride_three_run` -- so equality is the wrong test. What must
    /// hold is that every character the scan claims really is a well-formed
    /// three-byte sequence, because `put_run` decodes them without re-checking.
    #[test]
    fn the_direct_stride_scan_never_overcounts() {
        // A small alphabet with one byte from each class that matters, so the
        // combinations are walkable: a lead, a continuation, ASCII, a two-byte
        // lead, a four-byte lead, and a stray continuation-looking byte.
        const SYMS: [u8; 6] = [0xe3, 0x81, 0x41, 0xc3, 0xf0, 0xbf];
        let mut checked = 0usize;
        let mut claimed = 0usize;
        // Six positions is enough to cover two whole characters and the
        // boundary after them; the rest of the block is held at a continuation
        // so it cannot end the run early for an unrelated reason.
        for a in SYMS {
            for b in SYMS {
                for c in SYMS {
                    for d in SYMS {
                        for e in SYMS {
                            for f in SYMS {
                                let mut buf = [0x81u8; LANE_COUNT];
                                buf[..6].copy_from_slice(&[a, b, c, d, e, f]);
                                let bytes = StreamByte::slice_from(&buf);
                                let k = Vectorized::<Utf8>::load(bytes).stride_three_run();
                                assert!(k <= 5, "at most five characters fit");
                                for i in 0..k {
                                    let (x, y, z) = (buf[3 * i], buf[3 * i + 1], buf[3 * i + 2]);
                                    assert_eq!(x & 0xf0, 0xe0, "claimed lead at {i}: {x:02x}");
                                    assert_eq!(y & 0xc0, 0x80, "claimed cont at {i}: {y:02x}");
                                    assert_eq!(z & 0xc0, 0x80, "claimed cont at {i}: {z:02x}");
                                }
                                claimed += k;
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 6usize.pow(6), "every combination walked");
        assert!(
            claimed > 0,
            "the scan must accept something, or it cannot fire"
        );
    }

    /// It must take the whole block when the block is whole.
    #[test]
    fn a_full_block_of_three_byte_characters_is_five() {
        let mut buf = [0u8; LANE_COUNT];
        for i in 0..5 {
            buf[3 * i] = 0xe4;
            buf[3 * i + 1] = 0xb8;
            buf[3 * i + 2] = 0x80;
        }
        buf[15] = 0x41;
        let n = Vectorized::<Utf8>::load(StreamByte::slice_from(&buf)).stride_three_run();
        assert_eq!(n, 5);
    }
}
