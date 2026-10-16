//! zlib over deflate, inflated a read at a time (`[[source: rfc1950-zlib]]`,
//! `[[source: rfc1951-deflate]]`).
//!
//! **Resumable, because both callers' streams span reads.** A PNG's zlib
//! stream is cut into `IDAT` chunks wherever its encoder chose, and a kitty
//! `o=z` payload arrives in protocol chunks that cross `read(2)` boundaries
//! wherever the kernel cut them (`[[source: kitty-graphics]]`, "only RFC
//! 1950 ZLIB based deflate compression is supported"). So [`Inflate`] is a
//! state the caller keeps across its reads, [`Inflate::feed`] takes whatever
//! bytes arrived, and the output is the same wherever the boundaries fell --
//! errors included. [`Inflate::run`] is the same over any source of slices,
//! which is how the PNG decoder feeds it its `IDAT` chunks.
//!
//! **How a read boundary is survived.** The decoder advances in steps that
//! each need at most 48 bits: the zlib header, a block header, a stored
//! block's lengths, one field of a dynamic block's header, one literal, or one
//! length with its distance. Before a step the bit accumulator is filled to
//! more than 56 bits or until the input runs out, and the step runs on a copy
//! of it, committed only when the step completes. A step that runs out of bits
//! therefore leaves the state as it was, every input byte already held in the
//! accumulator, and resumes on the next feed; and a step is only short of bits
//! when the input is spent, so it is decided on exactly the bits a one-shot
//! decode would have decided it on. No input is retained but the accumulator.
//!
//! **The output is the caller's buffer, sized exactly.** Nothing is allocated:
//! the whole inflated stream lands in `out`, which is also deflate's window,
//! and its length is the stream's expected length. A stream that would write
//! past it is refused at the step that would ([`ZlibError::Overrun`]), and one
//! that ends short of it is refused at its last block
//! ([`ZlibError::Short`]): the bound on a decompression bomb is the size the
//! caller declared, never what the stream claims.
//!
//! **What 2.3 requires is checked.** "A compliant decompressor must check CMF,
//! FLG, and ADLER32" (RFC 1950 2.3): CM must be 8, "Values of CINFO above 7
//! are not allowed", FCHECK must make the pair a multiple of 31, and FDICT is
//! refused because no caller's format defines a preset dictionary ("When the
//! other format does not use the preset dictionary feature, a compliant
//! decompressor must reject any stream in which the FDICT flag is set").
//!
//! **Huffman decoding is a table, with the canonical walk behind it.** Each
//! code is assigned as RFC 1951 3.2.2 assigns it, and a symbol is looked up
//! by the next 10 bits (8 for a distance): a slot holds the symbol whose
//! code those bits begin with and the code's length, filled for every code
//! that short. A slot no such code fills -- a longer code, or none -- falls
//! to the walk 3.2.2 describes, a bit at a time. The fixed codes' tables are
//! built at compile time and a dynamic block's once, after its lengths pass
//! the check that refuses an over-subscribed set; the walk stays as the
//! tests' oracle for every slot and every decode.

/// Why a stream was refused. Every variant is a property of the bytes, or of
/// the bytes against the caller's declared length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZlibError {
    /// CMF and FLG: CM not 8, CINFO above 7, FCHECK wrong, or FDICT set.
    Header,
    /// A block type of 3, a stored block whose NLEN is not LEN's complement,
    /// an over-subscribed or impossible code, or a symbol no code defines.
    Block,
    /// A back-reference to before the first byte written.
    Distance,
    /// The stream would write past the caller's buffer.
    Overrun,
    /// The final block ended with the caller's buffer not full.
    Short,
    /// The Adler-32 in the trailer does not match the inflated bytes.
    Adler,
    /// The input ended before the stream did ([`Inflate::finish`]).
    Truncated,
}

/// What a feed left the stream as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inflated {
    /// Every byte was taken and the stream continues: feed the next read.
    More,
    /// The trailer was read and checked. Bytes after it are counted by
    /// [`Inflate::excess`], not inflated.
    Done,
}

/// Whether the block being read is the stream's last (BFINAL).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Last {
    No,
    Yes,
}

/// Where the stream stands between steps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// CMF and FLG.
    Zlib,
    /// BFINAL and BTYPE.
    Block,
    /// A stored block's LEN and NLEN, after the byte boundary.
    StoredLen,
    /// A stored block's bytes, this many still to copy.
    Stored(StoredLeft),
    /// A dynamic block's HLIT, HDIST and HCLEN.
    Counts,
    /// The code length code's lengths, this many read.
    CodeLengths(Index),
    /// The literal/length and distance code lengths, this many read.
    Lengths(Index),
    /// A compressed block's symbols.
    Codes,
    /// The Adler-32, after the byte boundary.
    Trailer,
    /// The trailer was read and matched.
    Done,
    /// Refused. Sticky: no later byte makes the stream valid.
    Failed(ZlibError),
}

/// Bytes of a stored block still to copy: at most 65,535 (3.2.4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct StoredLeft(usize);

/// A position in one of the dynamic header's arrays.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Index(usize);

/// Deflate's bits, least significant first from each byte (3.1.1): up to 64
/// held, the bits above `count` always zero.
#[derive(Clone, Copy)]
struct Bits {
    acc: u64,
    count: u32,
}

impl Bits {
    /// Fills to more than 56 bits, or until `input` is spent: enough for any
    /// one step when there is input to give it.
    #[inline(always)]
    fn refill(&mut self, input: &mut Input<'_>) {
        while self.count <= 56 {
            let Some(&b) = input.bytes.get(input.at) else {
                return;
            };
            input.at += 1;
            self.acc |= u64::from(b) << self.count;
            self.count += 8;
        }
    }

    /// The next `n` bits, or `None` when fewer are held.
    #[inline(always)]
    fn take(&mut self, n: u32) -> Option<u32> {
        if n > self.count {
            return None;
        }
        let v = self.acc & ((1u64 << n) - 1);
        self.acc >>= n;
        self.count -= n;
        u32::try_from(v).ok()
    }

    /// Drops the bits left in the current byte (a stored block, the trailer).
    fn align(&mut self) {
        let partial = self.count % 8;
        self.acc >>= partial;
        self.count -= partial;
    }
}

/// One feed's bytes and how far the accumulator has taken them.
struct Input<'a> {
    bytes: &'a [u8],
    at: usize,
}

const MAX_BITS: usize = 15;
const MAX_LIT: usize = 288;
const MAX_DIST: usize = 32;
const MAX_CODE_LENGTHS: usize = 19;

/// A canonical Huffman code as the specification defines it (3.2.2): how
/// many codes there are of each length, and the symbols in code order.
#[derive(Clone, Copy)]
struct Code<const N: usize> {
    count: [u16; MAX_BITS + 1],
    symbol: [u16; N],
}

impl<const N: usize> Code<N> {
    const EMPTY: Code<N> = Code {
        count: [0; MAX_BITS + 1],
        symbol: [0; N],
    };

    /// Builds the code from a length per symbol. An over-subscribed set of
    /// lengths is refused; an incomplete one is allowed, as the reference
    /// implementations allow it, because a single-code distance tree is
    /// legal and incomplete. A `const fn`, so the fixed codes are built at
    /// compile time; the casts widen a length of at most 15 and narrow a
    /// symbol of at most 319, so none loses a bit.
    const fn build(lengths: &[u8]) -> Result<Code<N>, ZlibError> {
        let mut code = Code::EMPTY;
        let mut i = 0;
        while i < lengths.len() {
            code.count[lengths[i] as usize] += 1;
            i += 1;
        }
        code.count[0] = 0;
        let mut left = 1i32;
        let mut len = 1;
        while len <= MAX_BITS {
            left <<= 1;
            left -= code.count[len] as i32;
            if left < 0 {
                return Err(ZlibError::Block);
            }
            len += 1;
        }
        let mut offs = [0u16; MAX_BITS + 1];
        len = 1;
        while len < MAX_BITS {
            offs[len + 1] = offs[len] + code.count[len];
            len += 1;
        }
        i = 0;
        while i < lengths.len() {
            let l = lengths[i] as usize;
            if l != 0 {
                let slot = offs[l] as usize;
                if slot >= N {
                    return Err(ZlibError::Block);
                }
                code.symbol[slot] = i as u16;
                offs[l] += 1;
            }
            i += 1;
        }
        Ok(code)
    }

    /// One symbol, a bit at a time out of `bits`: at each length, `first` is
    /// the first code of that length and `index` where its symbols start.
    /// `Ok(None)` when the code runs past the bits held, which a step treats
    /// as running out of input; a walk of fifteen held bits that finds no
    /// symbol is refused. The code length code is always walked; the other
    /// two only past their table ([`Huffman::decode`]).
    #[inline(always)]
    fn decode(&self, bits: &mut Bits) -> Result<Option<u16>, ZlibError> {
        let mut acc = bits.acc;
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..=MAX_BITS {
            code |= i32::from(acc & 1 != 0);
            acc >>= 1;
            let count = i32::from(self.count[len]);
            if code - count < first {
                let used = u32::try_from(len).map_err(|_| ZlibError::Block)?;
                if used > bits.count {
                    return Ok(None);
                }
                bits.acc >>= used;
                bits.count -= used;
                return self
                    .symbol
                    .get(usize::try_from(index + (code - first)).map_err(|_| ZlibError::Block)?)
                    .copied()
                    .map(Some)
                    .ok_or(ZlibError::Block);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        // Bits past `count` are zero, and in a canonical code the zero
        // extension of the held bits is the smallest code at each length,
        // so a walk that fails on them would fail on any continuation too.
        // Waiting for fifteen bits changes only which feed reports `Block`,
        // never the verdict: a truncated stream reads `Truncated` instead.
        if bits.count < 15 {
            Ok(None)
        } else {
            Err(ZlibError::Block)
        }
    }
}

/// Bits a literal/length lookup reads at once: every fixed code, and the
/// codes a dynamic block gives its common symbols.
const LIT_FAST: usize = 10;
/// Bits a distance lookup reads at once.
const DIST_FAST: usize = 8;
const LIT_SLOTS: usize = 1 << LIT_FAST;
const DIST_SLOTS: usize = 1 << DIST_FAST;

/// One slot of a lookup table: the symbol whose code the slot's index
/// begins with, and that code's length. [`Slot::WALK`], length 0, where no
/// code of at most the table's bits does -- a longer code's prefix, or no
/// code at all -- and the canonical walk decides.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Slot(u16);

impl Slot {
    const WALK: Slot = Slot(0);
    /// A symbol is below 288 and takes the low nine bits; the length the
    /// four above them.
    const SYMBOL_BITS: u32 = 9;

    const fn new(symbol: u16, len: u16) -> Slot {
        Slot((len << Self::SYMBOL_BITS) | symbol)
    }

    #[inline(always)]
    fn len(self) -> u32 {
        u32::from(self.0 >> Self::SYMBOL_BITS)
    }

    #[inline(always)]
    fn symbol(self) -> u16 {
        self.0 & ((1 << Self::SYMBOL_BITS) - 1)
    }
}

/// A canonical code and its lookup table: `S` slots, indexed by the next
/// `log2(S)` bits of the stream as they arrive.
#[derive(Clone, Copy)]
struct Huffman<const N: usize, const S: usize> {
    code: Code<N>,
    fast: [Slot; S],
}

impl<const N: usize, const S: usize> Huffman<N, S> {
    const EMPTY: Huffman<N, S> = Huffman {
        code: Code::EMPTY,
        fast: [Slot::WALK; S],
    };

    /// The code from a length per symbol, refused as [`Code::build`] refuses
    /// it, the table built only for a set that passed.
    const fn build(lengths: &[u8]) -> Result<Huffman<N, S>, ZlibError> {
        match Code::build(lengths) {
            Ok(code) => Ok(Huffman {
                code,
                fast: fast_table(lengths),
            }),
            Err(e) => Err(e),
        }
    }

    /// One symbol: the slot the next bits index, else the walk. A slot's
    /// code is the one the walk would find at that length, since no shorter
    /// code is its prefix, so the two agree on the symbol, on the bits
    /// taken, and on `Ok(None)` when the code runs past the bits held; bits
    /// past `count` are zero for both. The `as` keeps the low bits of the
    /// accumulator on any target, and those are the index.
    #[inline(always)]
    fn decode(&self, bits: &mut Bits) -> Result<Option<u16>, ZlibError> {
        let slot = self.fast[(bits.acc as usize) & (S - 1)];
        let len = slot.len();
        if len == 0 {
            return self.code.decode(bits);
        }
        if len > bits.count {
            return Ok(None);
        }
        bits.acc >>= len;
        bits.count -= len;
        Ok(Some(slot.symbol()))
    }
}

/// The lookup table for a set of lengths that [`Code::build`] accepted.
/// Codes are assigned as 3.2.2's steps 2 and 3 assign them, and each code
/// of at most `log2(S)` bits fills every slot whose low bits are the code
/// reversed: "Huffman codes are packed starting with the most-significant
/// bit of the code" (3.1.1), so its first bit is the index's lowest.
const fn fast_table<const S: usize>(lengths: &[u8]) -> [Slot; S] {
    let bits = S.trailing_zeros() as usize;
    let mut count = [0u32; MAX_BITS + 1];
    let mut i = 0;
    while i < lengths.len() {
        count[lengths[i] as usize] += 1;
        i += 1;
    }
    count[0] = 0;
    // Step 2: the smallest code of each length.
    let mut next = [0u32; MAX_BITS + 1];
    let mut code = 0u32;
    let mut len = 1;
    while len <= MAX_BITS {
        code = (code + count[len - 1]) << 1;
        next[len] = code;
        len += 1;
    }
    // Step 3: consecutive codes of each length in symbol order.
    let mut table = [Slot::WALK; S];
    i = 0;
    while i < lengths.len() {
        let l = lengths[i] as usize;
        if l != 0 {
            let c = next[l];
            next[l] += 1;
            if l <= bits {
                let mut rev = 0usize;
                let mut k = 0;
                while k < l {
                    rev |= (((c >> (l - 1 - k)) & 1) as usize) << k;
                    k += 1;
                }
                let slot = Slot::new(i as u16, l as u16);
                while rev < S {
                    table[rev] = slot;
                    rev += 1 << l;
                }
            }
        }
        i += 1;
    }
    table
}

/// The fixed literal/length code's lengths (3.2.6).
const FIXED_LIT_LENGTHS: [u8; MAX_LIT] = {
    let mut l = [8u8; MAX_LIT];
    let mut i = 144;
    while i < 256 {
        l[i] = 9;
        i += 1;
    }
    while i < 280 {
        l[i] = 7;
        i += 1;
    }
    l
};

/// The fixed codes (3.2.6), built at compile time. Distance codes 30 and 31
/// "will never actually occur", and are left out, so the walk refuses them.
const FIXED_LIT: Huffman<MAX_LIT, LIT_SLOTS> = match Huffman::build(&FIXED_LIT_LENGTHS) {
    Ok(h) => h,
    Err(_) => panic!("the fixed literal/length code is complete"),
};
const FIXED_DIST: Huffman<MAX_DIST, DIST_SLOTS> = match Huffman::build(&[5u8; 30]) {
    Ok(h) => h,
    Err(_) => panic!("the fixed distance code is not over-subscribed"),
};

/// The order the code length code's lengths are sent in (3.2.7).
const ORDER: [usize; MAX_CODE_LENGTHS] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Length and distance bases and extra bits (3.2.5).
const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// A dynamic block's header as far as it has been read (3.2.7).
#[derive(Clone, Copy)]
struct Dynamic {
    nlen: usize,
    ndist: usize,
    ncode: usize,
    lengths: [u8; MAX_LIT + MAX_DIST],
    code_lengths: Code<MAX_CODE_LENGTHS>,
}

/// What one pass over a compressed block's symbols ended on.
enum Symbols {
    Starved,
    EndOfBlock,
}

/// A zlib stream part-way through: what [`Inflate::feed`] resumes from.
/// 3,736 bytes, almost all of it the current block's two codes and their
/// tables; `Copy`, so a caller keeps it in whatever state outlives its reads.
#[derive(Clone, Copy)]
pub struct Inflate {
    bits: Bits,
    /// Bytes written to the caller's buffer.
    len: usize,
    /// Bytes fed after the trailer.
    excess: usize,
    phase: Phase,
    last: Last,
    lit: Huffman<MAX_LIT, LIT_SLOTS>,
    dist: Huffman<MAX_DIST, DIST_SLOTS>,
    dynamic: Dynamic,
}

impl Inflate {
    /// Nothing read.
    pub const NEW: Inflate = Inflate {
        bits: Bits { acc: 0, count: 0 },
        len: 0,
        excess: 0,
        phase: Phase::Zlib,
        last: Last::No,
        lit: Huffman::EMPTY,
        dist: Huffman::EMPTY,
        dynamic: Dynamic {
            nlen: 0,
            ndist: 0,
            ncode: 0,
            lengths: [0; MAX_LIT + MAX_DIST],
            code_lengths: Code::EMPTY,
        },
    };

    /// Inflates the next bytes of the stream into `out`, which is the same
    /// buffer on every feed and exactly as long as the inflated stream.
    ///
    /// Returns [`Inflated::More`] when every byte was taken and the stream
    /// continues, [`Inflated::Done`] once the trailer is read; a refusal is
    /// returned again by every later feed.
    pub fn feed(&mut self, input: &[u8], out: &mut [u8]) -> Result<Inflated, ZlibError> {
        let mut input = Input {
            bytes: input,
            at: 0,
        };
        match self.steps(&mut input, out) {
            Ok(done) => Ok(done),
            Err(e) => {
                self.phase = Phase::Failed(e);
                Err(e)
            }
        }
    }

    /// [`Self::feed`] over every slice `source` yields, in order, stopping
    /// at the trailer: the stream generic over where its bytes come from.
    pub fn run<'s, S>(&mut self, source: S, out: &mut [u8]) -> Result<Inflated, ZlibError>
    where
        S: IntoIterator<Item = &'s [u8]>,
    {
        for bytes in source {
            if self.feed(bytes, out)? == Inflated::Done {
                return Ok(Inflated::Done);
            }
        }
        Ok(Inflated::More)
    }

    /// The input has ended: whether the stream ended with it.
    pub fn finish(&self) -> Result<(), ZlibError> {
        match self.phase {
            Phase::Done => Ok(()),
            Phase::Failed(e) => Err(e),
            _ => Err(ZlibError::Truncated),
        }
    }

    /// Bytes fed after the trailer: what a format that ends with its zlib
    /// stream decides about.
    pub fn excess(&self) -> usize {
        self.excess
    }

    /// Bytes written to the caller's buffer so far.
    pub fn written(&self) -> usize {
        self.len
    }

    fn steps(&mut self, input: &mut Input<'_>, out: &mut [u8]) -> Result<Inflated, ZlibError> {
        loop {
            match self.phase {
                Phase::Failed(e) => return Err(e),
                Phase::Done => {
                    // Checked before the accumulator is filled, so every
                    // byte after the trailer is counted here or below.
                    self.excess += input.bytes.len() - input.at;
                    input.at = input.bytes.len();
                    return Ok(Inflated::Done);
                }
                _ => {}
            }
            self.bits.refill(input);
            let mut b = self.bits;
            match self.phase {
                Phase::Failed(e) => return Err(e),
                Phase::Done => return Ok(Inflated::Done),
                Phase::Codes => match self.symbols(input, out)? {
                    Symbols::Starved => return Ok(Inflated::More),
                    Symbols::EndOfBlock => {
                        self.end_block(out)?;
                        continue;
                    }
                },
                Phase::Stored(left) => {
                    if self.stored(left, input, out)? {
                        self.end_block(out)?;
                        continue;
                    }
                    return Ok(Inflated::More);
                }
                Phase::Zlib => {
                    let (Some(cmf), Some(flg)) = (b.take(8), b.take(8)) else {
                        return Ok(Inflated::More);
                    };
                    let cm = cmf & 0x0f;
                    let cinfo = cmf >> 4;
                    if cm != 8 || cinfo > 7 || (cmf * 256 + flg) % 31 != 0 || flg & 0x20 != 0 {
                        return Err(ZlibError::Header);
                    }
                    self.phase = Phase::Block;
                }
                Phase::Block => {
                    let (Some(last), Some(kind)) = (b.take(1), b.take(2)) else {
                        return Ok(Inflated::More);
                    };
                    self.last = if last == 1 { Last::Yes } else { Last::No };
                    self.phase = match kind {
                        0 => Phase::StoredLen,
                        1 => {
                            (self.lit, self.dist) = (FIXED_LIT, FIXED_DIST);
                            Phase::Codes
                        }
                        2 => Phase::Counts,
                        _ => return Err(ZlibError::Block),
                    };
                }
                Phase::StoredLen => {
                    b.align();
                    let (Some(len), Some(nlen)) = (b.take(16), b.take(16)) else {
                        return Ok(Inflated::More);
                    };
                    if len != !nlen & 0xffff {
                        return Err(ZlibError::Block);
                    }
                    let len = usize::try_from(len).map_err(|_| ZlibError::Block)?;
                    if len > out.len() - self.len {
                        return Err(ZlibError::Overrun);
                    }
                    self.phase = Phase::Stored(StoredLeft(len));
                }
                Phase::Counts => {
                    let (Some(nlen), Some(ndist), Some(ncode)) = (b.take(5), b.take(5), b.take(4))
                    else {
                        return Ok(Inflated::More);
                    };
                    let d = &mut self.dynamic;
                    d.nlen = small(nlen)? + 257;
                    d.ndist = small(ndist)? + 1;
                    d.ncode = small(ncode)? + 4;
                    if d.nlen > 286 || d.ndist > 30 {
                        return Err(ZlibError::Block);
                    }
                    d.lengths = [0; MAX_LIT + MAX_DIST];
                    self.phase = Phase::CodeLengths(Index(0));
                }
                Phase::CodeLengths(Index(k)) => {
                    let d = &mut self.dynamic;
                    let Some(l) = b.take(3) else {
                        return Ok(Inflated::More);
                    };
                    d.lengths[ORDER[k]] = u8::try_from(l).map_err(|_| ZlibError::Block)?;
                    self.phase = if k + 1 < d.ncode {
                        Phase::CodeLengths(Index(k + 1))
                    } else {
                        d.code_lengths = Code::build(&d.lengths[..MAX_CODE_LENGTHS])?;
                        d.lengths[..MAX_CODE_LENGTHS].fill(0);
                        Phase::Lengths(Index(0))
                    };
                }
                Phase::Lengths(Index(i)) => {
                    let Some(next) = self.length(&mut b, i)? else {
                        return Ok(Inflated::More);
                    };
                    let d = &self.dynamic;
                    if next < d.nlen + d.ndist {
                        self.phase = Phase::Lengths(Index(next));
                    } else {
                        // The end-of-block code must exist.
                        if d.lengths[256] == 0 {
                            return Err(ZlibError::Block);
                        }
                        self.lit = Huffman::build(&d.lengths[..d.nlen])?;
                        self.dist = Huffman::build(&d.lengths[d.nlen..d.nlen + d.ndist])?;
                        self.phase = Phase::Codes;
                    }
                }
                Phase::Trailer => {
                    b.align();
                    let mut adler = 0u32;
                    for _ in 0..4 {
                        let Some(byte) = b.take(8) else {
                            return Ok(Inflated::More);
                        };
                        adler = (adler << 8) | byte;
                    }
                    if adler32(&out[..self.len]) != adler {
                        return Err(ZlibError::Adler);
                    }
                    self.phase = Phase::Done;
                    // The whole bytes still held came after the trailer.
                    self.excess += usize::try_from(b.count / 8).map_err(|_| ZlibError::Block)?;
                    b = Bits { acc: 0, count: 0 };
                }
            }
            self.bits = b;
        }
    }

    /// One symbol of the code length alphabet, at position `i` of the
    /// lengths: where the next one goes, or `None` when the bits ran out.
    fn length(&mut self, b: &mut Bits, i: usize) -> Result<Option<usize>, ZlibError> {
        let d = &mut self.dynamic;
        let total = d.nlen + d.ndist;
        let Some(sym) = d.code_lengths.decode(b)? else {
            return Ok(None);
        };
        let (n, value) = match sym {
            0..=15 => (1, u8::try_from(sym).map_err(|_| ZlibError::Block)?),
            16 => {
                if i == 0 {
                    return Err(ZlibError::Block);
                }
                let Some(extra) = b.take(2) else {
                    return Ok(None);
                };
                (3 + small(extra)?, d.lengths[i - 1])
            }
            17 => {
                let Some(extra) = b.take(3) else {
                    return Ok(None);
                };
                (3 + small(extra)?, 0)
            }
            18 => {
                let Some(extra) = b.take(7) else {
                    return Ok(None);
                };
                (11 + small(extra)?, 0)
            }
            _ => return Err(ZlibError::Block),
        };
        if i + n > total {
            return Err(ZlibError::Block);
        }
        d.lengths[i..i + n].fill(value);
        Ok(Some(i + n))
    }

    /// A compressed block's symbols until end-of-block, or until the bits
    /// run out with the input spent (3.2.3).
    fn symbols(&mut self, input: &mut Input<'_>, out: &mut [u8]) -> Result<Symbols, ZlibError> {
        let mut bits = self.bits;
        let mut len = self.len;
        let (lit, dist) = (&self.lit, &self.dist);
        let ended = loop {
            bits.refill(input);
            let mut b = bits;
            let Some(sym) = lit.decode(&mut b)? else {
                break Symbols::Starved;
            };
            match sym {
                0..=255 => {
                    *out.get_mut(len).ok_or(ZlibError::Overrun)? =
                        u8::try_from(sym).map_err(|_| ZlibError::Block)?;
                    len += 1;
                }
                256 => {
                    bits = b;
                    break Symbols::EndOfBlock;
                }
                257..=285 => {
                    let i = usize::from(sym - 257);
                    let Some(extra) = b.take(u32::from(LEN_EXTRA[i])) else {
                        break Symbols::Starved;
                    };
                    let run = usize::from(LEN_BASE[i]) + small(extra)?;
                    let Some(d) = dist.decode(&mut b)? else {
                        break Symbols::Starved;
                    };
                    let d = usize::from(d);
                    if d >= 30 {
                        return Err(ZlibError::Block);
                    }
                    let Some(extra) = b.take(u32::from(DIST_EXTRA[d])) else {
                        break Symbols::Starved;
                    };
                    let back = usize::from(DIST_BASE[d]) + small(extra)?;
                    copy(out, &mut len, Run(run), Back(back))?;
                }
                _ => return Err(ZlibError::Block),
            }
            bits = b;
        };
        self.bits = bits;
        self.len = len;
        Ok(ended)
    }

    /// A stored block's bytes: first those the accumulator holds, then
    /// straight from the input. Whether the block is complete.
    fn stored(
        &mut self,
        left: StoredLeft,
        input: &mut Input<'_>,
        out: &mut [u8],
    ) -> Result<bool, ZlibError> {
        let mut left = left.0;
        // The accumulator is byte-aligned here, so it holds whole bytes.
        while left > 0 {
            let Some(byte) = self.bits.take(8) else {
                break;
            };
            *out.get_mut(self.len).ok_or(ZlibError::Overrun)? =
                u8::try_from(byte).map_err(|_| ZlibError::Block)?;
            self.len += 1;
            left -= 1;
        }
        let n = left.min(input.bytes.len() - input.at);
        // **A loop, not `copy_from_slice`, and that is measured.** With
        // `[u8]` `copy_from_slice` calls in this module, `png.rs` and
        // `Base64::feed`, LLVM compiled `View::put_ascii_run` and
        // `put_narrow_run` differently, their source unchanged, and `ingest2`
        // retired 740.6 M instructions on `cat` against the base's 734.9 M.
        // Written as loops and array stores, its `__TEXT,__text` is the
        // base's byte for byte (the record, "The decoders move to
        // baud-domain, and inflate resumes across reads").
        let dst = out
            .get_mut(self.len..self.len + n)
            .ok_or(ZlibError::Overrun)?;
        for (d, &s) in dst.iter_mut().zip(&input.bytes[input.at..input.at + n]) {
            *d = s;
        }
        input.at += n;
        self.len += n;
        left -= n;
        self.phase = Phase::Stored(StoredLeft(left));
        Ok(left == 0)
    }

    /// After a block's last byte: the next block, or, after the final one,
    /// the check that the caller's buffer is exactly full and the trailer.
    fn end_block(&mut self, out: &[u8]) -> Result<(), ZlibError> {
        self.phase = match self.last {
            Last::No => Phase::Block,
            Last::Yes if self.len != out.len() => return Err(ZlibError::Short),
            Last::Yes => Phase::Trailer,
        };
        Ok(())
    }
}

/// How many bytes a back-reference copies (3.2.5's length).
#[derive(Clone, Copy)]
struct Run(usize);

/// How far behind the write position it copies from (3.2.5's distance);
/// the two are the same width and a swapped pair decodes to plausible
/// garbage, so they do not share a type.
#[derive(Clone, Copy)]
struct Back(usize);

/// A back-reference: `run` bytes from `back` behind the write position, byte
/// by byte so an overlapping copy repeats as the format intends.
#[inline(always)]
fn copy(out: &mut [u8], len: &mut usize, run: Run, back: Back) -> Result<(), ZlibError> {
    if back.0 == 0 || back.0 > *len {
        return Err(ZlibError::Distance);
    }
    if run.0 > out.len() - *len {
        return Err(ZlibError::Overrun);
    }
    for _ in 0..run.0 {
        out[*len] = out[*len - back.0];
        *len += 1;
    }
    Ok(())
}

/// A field of at most a few bits as a count.
fn small(v: u32) -> Result<usize, ZlibError> {
    usize::try_from(v).map_err(|_| ZlibError::Block)
}

/// Adler-32 (RFC 1950 8.2), over the inflated bytes.
fn adler32(bytes: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &x in bytes {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// The lookup tables against the canonical walk, which stays as the oracle:
/// every slot of every table, and every decode at the bit counts a step can
/// meet, over the fixed codes and generated dynamic ones -- complete and
/// incomplete, with codes longer than a table reads.
#[cfg(test)]
mod tests {
    use super::*;

    /// A xorshift, so every set is the same on every run.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            usize::try_from(self.next() % u64::try_from(n).expect("small")).expect("small")
        }
    }

    /// Whether a generated set is filled to a complete code.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Fill {
        Complete,
        AsDrawn,
    }

    /// A code's Kraft sum in units of 2^-15: 32,768 is complete.
    fn kraft(lengths: &[u8]) -> u32 {
        lengths
            .iter()
            .filter(|&&l| l != 0)
            .map(|&l| 1u32 << (15 - u32::from(l)))
            .sum()
    }

    /// Lengths drawn from 0..=15, lengthened at random until the set is not
    /// over-subscribed, which leaves most codes long; then, for
    /// [`Fill::Complete`], unused symbols given codes and codes shortened
    /// until the sum is one.
    fn draw(rng: &mut Rng, out: &mut [u8], fill: Fill) {
        for l in out.iter_mut() {
            *l = if rng.below(4) == 0 {
                0
            } else {
                u8::try_from(1 + rng.below(15)).expect("small")
            };
        }
        while kraft(out) > 1 << 15 {
            let i = rng.below(out.len());
            if (1..15).contains(&out[i]) {
                out[i] += 1;
            }
        }
        if fill == Fill::AsDrawn {
            return;
        }
        while kraft(out) < 1 << 15 {
            let left = (1u32 << 15) - kraft(out);
            let i = rng.below(out.len());
            let l = u32::from(out[i]);
            if l == 0 {
                out[i] = 15;
            } else if l > 1 && 1u32 << (15 - l) <= left {
                out[i] -= 1;
            }
        }
    }

    /// Every slot of the table holds what the walk finds in the slot's own
    /// bits: the symbol and its length where a code of at most that many
    /// bits begins them, [`Slot::WALK`] where none does.
    fn slots_agree<const N: usize, const S: usize>(lengths: &[u8]) {
        let h = Huffman::<N, S>::build(lengths).expect("an accepted set");
        let n = S.trailing_zeros();
        for i in 0..S {
            let mut b = Bits {
                acc: u64::try_from(i).expect("small"),
                count: n,
            };
            let expect = match h.code.decode(&mut b) {
                Ok(Some(sym)) => Slot::new(sym, u16::try_from(n - b.count).expect("small")),
                Ok(None) => Slot::WALK,
                Err(e) => panic!("{e:?} on {n} held bits"),
            };
            assert_eq!(h.fast[i], expect, "slot {i:#b} of {lengths:?}");
        }
    }

    /// The bit counts a decode can meet: none, fewer than any code, about a
    /// table's width, fewer than fifteen and more, and a full accumulator.
    const COUNTS: [u32; 12] = [0, 1, 4, 7, 8, 9, 10, 11, 14, 15, 16, 64];

    /// The lookup's verdict and the bits it leaves equal the walk's for
    /// every pattern of the next fifteen bits at every count: the symbol,
    /// `Ok(None)` short of bits, and `Err(Block)` on fifteen bits that
    /// spell no code.
    fn decodes_agree<const N: usize, const S: usize>(lengths: &[u8]) {
        let h = Huffman::<N, S>::build(lengths).expect("an accepted set");
        for count in COUNTS {
            let held = count.min(15);
            for p in 0..1u64 << held {
                let acc = if count == 64 {
                    p | (p.wrapping_mul(0x9e37_79b9_7f4a_7c15) << 15)
                } else {
                    p
                };
                let (mut a, mut b) = (Bits { acc, count }, Bits { acc, count });
                let (fast, walk) = (h.decode(&mut a), h.code.decode(&mut b));
                assert_eq!(fast, walk, "{p:#b} at {count} of {lengths:?}");
                assert_eq!((a.acc, a.count), (b.acc, b.count), "{p:#b} at {count}");
            }
        }
    }

    /// The sets: the fixed codes, a single one-bit distance code and none
    /// (3.2.7), and generated literal/length and distance sets of every
    /// size, complete and as drawn.
    fn each_set(mut check: impl FnMut(&[u8], &[u8])) {
        check(&FIXED_LIT_LENGTHS, &[5u8; 30]);
        check(&FIXED_LIT_LENGTHS, &[1]);
        check(&FIXED_LIT_LENGTHS, &[0, 1]);
        check(&FIXED_LIT_LENGTHS, &[0]);
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        for k in 0..48 {
            let fill = if k % 2 == 0 {
                Fill::Complete
            } else {
                Fill::AsDrawn
            };
            let mut lit = [0u8; MAX_LIT];
            let mut dist = [0u8; 30];
            let nlit = 257 + rng.below(30);
            let ndist = 1 + rng.below(30);
            draw(&mut rng, &mut lit[..nlit], fill);
            draw(&mut rng, &mut dist[..ndist], fill);
            check(&lit[..nlit], &dist[..ndist]);
        }
    }

    #[test]
    fn the_sets_reach_codes_past_each_table_and_incomplete_codes() {
        // The positive control: a table whose sets all fit it never takes
        // the walk, and agreeing there would test nothing.
        let (mut long_lit, mut long_dist, mut incomplete, mut sets) = (0, 0, 0, 0);
        each_set(|lit, dist| {
            sets += 1;
            long_lit += usize::from(lit.iter().any(|&l| usize::from(l) > LIT_FAST));
            long_dist += usize::from(dist.iter().any(|&l| usize::from(l) > DIST_FAST));
            incomplete += usize::from(kraft(lit) < 1 << 15);
            incomplete += usize::from(kraft(dist) < 1 << 15);
        });
        assert_eq!(sets, 52);
        assert!(
            long_lit >= 24,
            "{long_lit} literal sets past {LIT_FAST} bits"
        );
        assert!(
            long_dist >= 12,
            "{long_dist} distance sets past {DIST_FAST} bits"
        );
        assert!(incomplete >= 24, "{incomplete} incomplete sets");
    }

    #[test]
    fn every_slot_holds_what_the_canonical_walk_finds() {
        each_set(|lit, dist| {
            slots_agree::<MAX_LIT, LIT_SLOTS>(lit);
            slots_agree::<MAX_DIST, DIST_SLOTS>(dist);
        });
    }

    #[test]
    fn the_lookup_decodes_as_the_walk_at_every_bit_count() {
        each_set(|lit, dist| {
            decodes_agree::<MAX_LIT, LIT_SLOTS>(lit);
            decodes_agree::<MAX_DIST, DIST_SLOTS>(dist);
        });
    }

    #[test]
    fn the_fixed_tables_are_the_fixed_codes() {
        // 3.2.6's own rows: literal 0 is 00110000, 144 is 110010000, 256
        // is 0000000 and 280 is 11000000, each read from its first bit.
        let rows: [(u16, u16, u16); 4] = [
            (0, 0b0011_0000, 8),
            (144, 0b1_1001_0000, 9),
            (256, 0, 7),
            (280, 0b1100_0000, 8),
        ];
        for (sym, code, len) in rows {
            let mut rev = 0usize;
            for k in 0..len {
                rev |= usize::from((code >> (len - 1 - k)) & 1) << k;
            }
            assert_eq!(FIXED_LIT.fast[rev], Slot::new(sym, len), "{sym}");
        }
        assert_eq!(FIXED_DIST.fast[0b11111], Slot::WALK);
        assert_eq!(FIXED_DIST.fast[0b10000], Slot::new(1, 5));
    }
}
