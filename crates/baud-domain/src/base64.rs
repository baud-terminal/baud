//! Base64, decoded a byte at a time: `[[source: rfc4648-base64]]` section 4.
//!
//! **Incremental, because every caller's data spans reads.** An `OSC 52` body
//! of any size crosses a `read(2)` boundary wherever the kernel cut it, and a
//! kitty `t=d` payload is chunked by the protocol itself (`t-images.md`), so a
//! four-character quantum can arrive as one character in one read and three in
//! the next. The carry -- at most three sextets -- is the decoder's state and
//! nothing else, so the caller keeps one `Base64` across its reads and the
//! octets come out the same wherever the boundaries fell. One decoder for both.
//!
//! **No buffer.** Each quantum's octets are handed back as they complete, and
//! where they go -- a bounded caller buffer, counted past its end -- is the
//! caller's: this crate allocates nothing.
//!
//! **What it accepts, against the text.**
//!
//! * The alphabet of Table 1 and `=`, and nothing else: section 3.3, "MUST
//!   reject the encoded data if it contains characters outside the base
//!   alphabet ... unless the specification referring to this document
//!   explicitly states otherwise". `[[source: ctlseqs]]` names the RFC for
//!   `OSC 52` and states nothing otherwise, so a line feed, a space or a pad
//!   before the end rejects the data, and so do excess pads, which section
//!   3.3 lets only such a specification ignore.
//! * Padding where section 4 puts it: `xx==` and `xxx=` end the data.
//! * **The unpadded final quanta `xx` and `xxx`.** Section 3.2's "MUST
//!   include appropriate pad characters" binds the encoder, and its reason --
//!   "when assumptions about the size of transported data cannot be made" --
//!   does not hold for a string whose end is its terminator. Rejecting would
//!   be allowed too; this is a choice, recorded where `finish` makes it.
//! * **Non-zero pad bits.** Section 3.5: decoders "MAY chose to reject an
//!   encoding if the pad bits have not been set to zero". This one does not.

/// The alphabet's values, indexed by byte: 0-63 for Table 1's characters,
/// [`NOT_BASE64`] for every other byte. Built once at compile time, so a
/// character is one load rather than a range test per class.
const VALUE: [u8; 256] = {
    let mut t = [NOT_BASE64; 256];
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut i = 0;
    while i < 64 {
        t[alphabet[i] as usize] = i as u8;
        i += 1;
    }
    t
};

/// What [`VALUE`] holds for a byte outside the alphabet, `=` included.
const NOT_BASE64: u8 = 0xFF;

/// How many sextets of the quantum in progress are held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Held {
    Zero,
    One,
    Two,
    Three,
}

/// Where the decoder stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Taking characters of the alphabet.
    Open,
    /// Two characters and one `=`: only the second `=` may follow.
    HalfPadded,
    /// The final quantum's pads arrived: the data has ended, and anything
    /// after them is a pad before the end.
    Ended,
    /// Rejected. Sticky: nothing that follows makes the data valid.
    Invalid,
}

/// The decoder's state: the sextets of the quantum in progress and where the
/// data stands. Eight bytes, `Copy`, so a caller keeps it in whatever state
/// struct outlives its reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Base64 {
    /// The held sextets, the first in the highest bits.
    acc: u32,
    held: Held,
    phase: Phase,
}

/// How many octets a quantum released.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OctetCount {
    Zero,
    One,
    Two,
    Three,
}

/// The octets one quantum decoded to: three for a whole quantum, one or two
/// for a short final one, none for empty data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Octets {
    bytes: [u8; 3],
    count: OctetCount,
}

impl Octets {
    /// No octets: what empty data, or a padded end, leaves to finish.
    pub const NONE: Octets = Octets {
        bytes: [0; 3],
        count: OctetCount::Zero,
    };

    /// The decoded octets.
    pub fn as_slice(&self) -> &[u8] {
        let n = match self.count {
            OctetCount::Zero => 0,
            OctetCount::One => 1,
            OctetCount::Two => 2,
            OctetCount::Three => 3,
        };
        &self.bytes[..n]
    }
}

/// What one character did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Base64Step {
    /// Held: the quantum is not complete.
    Need,
    /// A quantum completed, and these are its octets.
    Octets(Octets),
    /// The data is rejected, now and for every character after.
    Invalid,
}

/// What one [`Base64::feed`] did: how many encoded bytes it took and how
/// many octets it wrote.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fed {
    pub read: usize,
    pub wrote: usize,
}

/// Why [`Base64::finish`] rejected the data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Base64Error {
    /// A character outside the alphabet, or a pad where section 4 puts none,
    /// arrived before the end.
    NotBase64,
    /// The data ended on one sextet, which no octet can be made of, or on two
    /// characters and a single pad.
    Truncated,
}

impl Base64 {
    /// Nothing held, nothing seen.
    pub const NEW: Base64 = Base64 {
        acc: 0,
        held: Held::Zero,
        phase: Phase::Open,
    };

    /// One character of the encoded data.
    ///
    /// **Inlined into its caller, and that is measured.** Out of line, the
    /// linker laid its 448 bytes between `Paint::index` and
    /// `View::put_ascii_run`, the two functions `cat`'s ingest calls, and
    /// `ingest2` read `cat` 8% slower with none of the clipboard code run;
    /// base's code with this one function added, called once off the ingest
    /// path, read the same 8%. Inlined, the five recordings overlap base; a
    /// `multibench` cost remains that this does not remove. Its one caller in
    /// the core is a cold function, which is where the code belongs.
    #[inline(always)]
    pub fn push(&mut self, b: u8) -> Base64Step {
        match self.phase {
            Phase::Open => {}
            Phase::HalfPadded if b == b'=' => {
                // `xx==`: one octet, from the first twelve bits.
                self.phase = Phase::Ended;
                return Base64Step::Octets(self.release(OctetCount::One));
            }
            Phase::HalfPadded | Phase::Ended | Phase::Invalid => return self.reject(),
        }
        let v = VALUE[b as usize];
        if v != NOT_BASE64 {
            let acc = (self.acc << 6) | u32::from(v);
            self.held = match self.held {
                Held::Zero => Held::One,
                Held::One => Held::Two,
                Held::Two => Held::Three,
                Held::Three => {
                    // The fourth sextet: a whole quantum, 24 bits, three octets.
                    self.acc = 0;
                    self.held = Held::Zero;
                    return Base64Step::Octets(octets(acc, OctetCount::Three));
                }
            };
            self.acc = acc;
            return Base64Step::Need;
        }
        match (b, self.held) {
            // `xx=` waits for its second pad; `xxx=` is two octets and the end.
            (b'=', Held::Two) => {
                self.phase = Phase::HalfPadded;
                Base64Step::Need
            }
            (b'=', Held::Three) => {
                self.phase = Phase::Ended;
                Base64Step::Octets(self.release(OctetCount::Two))
            }
            // A pad after none or one sextet, or any byte outside the
            // alphabet: section 3.3's rejection.
            _ => self.reject(),
        }
    }

    /// A read's worth of the encoded data, decoded into `out` from its start.
    ///
    /// **Any length, because a chunk is not a quantum.** kitty's text asks a
    /// client for chunks that are multiples of four but the last, and baud
    /// takes chunks of any length; a `read(2)` boundary falls
    /// anywhere regardless. So a quantum may start in one feed and end in
    /// the next, held in the same carry [`Self::push`] keeps, and the octets
    /// are the same wherever the reads were cut.
    ///
    /// Stops early, `read` short of `input`, only when the next character
    /// would complete a quantum whose octets do not fit in `out`: the caller
    /// moves its window on and feeds the rest. A character outside the
    /// alphabet rejects the data, now and on every later feed, and what was
    /// written before it is the caller's to discard. [`Self::finish`] still
    /// ends the data.
    pub fn feed(&mut self, input: &[u8], out: &mut [u8]) -> Result<Fed, Base64Error> {
        let mut fed = Fed { read: 0, wrote: 0 };
        loop {
            // Whole quanta while nothing is carried: four characters, one
            // load each, three octets, and no state to keep.
            if self.held == Held::Zero && self.phase == Phase::Open {
                while let (Some(&[c0, c1, c2, c3]), Some([d0, d1, d2])) = (
                    input.get(fed.read..fed.read + 4),
                    out.get_mut(fed.wrote..fed.wrote + 3),
                ) {
                    let v = [
                        VALUE[usize::from(c0)],
                        VALUE[usize::from(c1)],
                        VALUE[usize::from(c2)],
                        VALUE[usize::from(c3)],
                    ];
                    if v.contains(&NOT_BASE64) {
                        break;
                    }
                    let word = v.iter().fold(0u32, |w, &x| (w << 6) | u32::from(x));
                    [_, *d0, *d1, *d2] = word.to_be_bytes();
                    fed.read += 4;
                    fed.wrote += 3;
                }
            }
            let Some(&b) = input.get(fed.read) else {
                return Ok(fed);
            };
            // One character on a copy, kept only if its octets fit. Written
            // without `copy_from_slice`, as `zlib.rs`'s stored-block copy
            // explains.
            let mut next = *self;
            match next.push(b) {
                Base64Step::Need => {}
                Base64Step::Octets(o) => {
                    let o = o.as_slice();
                    let Some(dst) = out.get_mut(fed.wrote..fed.wrote + o.len()) else {
                        return Ok(fed);
                    };
                    for (d, &s) in dst.iter_mut().zip(o) {
                        *d = s;
                    }
                    fed.wrote += o.len();
                }
                Base64Step::Invalid => {
                    *self = next;
                    return Err(Base64Error::NotBase64);
                }
            }
            *self = next;
            fed.read += 1;
        }
    }

    /// The data has ended: the octets of an unpadded final quantum, or why it
    /// is rejected.
    ///
    /// **`xx` and `xxx` decode as `xx==` and `xxx=` would.** The choice the
    /// module's comment states: an encoder must pad (section 3.2), and a
    /// decoder that knows where the data ends loses nothing by not requiring
    /// it. `x` alone is six bits and no octet, and `xx=` is half a padding;
    /// both are rejected.
    pub fn finish(self) -> Result<Octets, Base64Error> {
        match (self.phase, self.held) {
            (Phase::Invalid, _) => Err(Base64Error::NotBase64),
            (Phase::HalfPadded, _) | (Phase::Open, Held::One) => Err(Base64Error::Truncated),
            (Phase::Ended, _) | (Phase::Open, Held::Zero) => Ok(Octets::NONE),
            (Phase::Open, Held::Two) => {
                let mut d = self;
                Ok(d.release(OctetCount::One))
            }
            (Phase::Open, Held::Three) => {
                let mut d = self;
                Ok(d.release(OctetCount::Two))
            }
        }
    }

    /// Whether the data has ended at its pads (`xx==`, `xxx=`): what follows
    /// is new data, which a caller that concatenates padded pieces -- a kitty
    /// `OSC 99` payload chunked before it was encoded -- starts afresh.
    pub fn has_ended(&self) -> bool {
        self.phase == Phase::Ended
    }

    /// The held sextets of a short final quantum as `count` octets, the
    /// quantum emptied. Shifted up to a whole 24 bits first, so the octets
    /// are the top of the word; the low bits two or three sextets leave are
    /// the pad bits, which section 3.5 lets this decoder ignore.
    fn release(&mut self, count: OctetCount) -> Octets {
        let word = match self.held {
            Held::Zero => 0,
            Held::One => self.acc << 18,
            Held::Two => self.acc << 12,
            Held::Three => self.acc << 6,
        };
        self.acc = 0;
        self.held = Held::Zero;
        octets(word, count)
    }

    fn reject(&mut self) -> Base64Step {
        self.phase = Phase::Invalid;
        Base64Step::Invalid
    }
}

/// **A kitty graphics payload's base64, decoded as kitty decodes it**: one
/// chunk at a time, non-strict, and never an error.
///
/// kitty hands each command's payload to `base64_decode8` and ignores what
/// it returns (`[[source: kitty-base64-h]]`; `[[source:
/// kitty-parse-graphics-command-h]]`, `PAYLOAD`), and libbase64's tail
/// decoder writes an octet as soon as a character completes it and stops at
/// the first byte outside its table, `=` included, keeping what it wrote
/// (`[[source: libbase64-dec-tail-c]]`). So a chunk's octets are those of the
/// characters before its first byte that is no base64, a short final
/// quantum's whole octets among them, and the rest of the chunk is dropped:
/// mpv's status line written into a chunk, which kitty's parser keeps as
/// payload (`[[source: kitty-vt-parser-c]]`), ends that chunk's data where it
/// begins. `[[source: rfc4648-base64]]` 3.3 lets a decoder ignore such bytes
/// only where "the specification referring to this document explicitly
/// states otherwise", and kitty's graphics protocol is defined by kitty's
/// behaviour, so this is kitty's error handling for kitty's own protocol.
/// Every other user of base64 here keeps [`Base64`], which rejects.
///
/// The octets come out a quantum at a time and a short quantum's at the
/// stop or the chunk's end, the order kitty writes them in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KittyBase64 {
    acc: u32,
    held: Held,
    chunk: ChunkData,
}

/// Whether a chunk's data is still being read, or ended at a byte that is no
/// base64.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChunkData {
    Open,
    Stopped,
}

impl KittyBase64 {
    /// A chunk not begun.
    pub const NEW: KittyBase64 = KittyBase64 {
        acc: 0,
        held: Held::Zero,
        chunk: ChunkData::Open,
    };

    /// One payload byte: the octets it completes, or a stop's short quantum.
    #[inline(always)]
    pub fn push(&mut self, b: u8) -> Octets {
        if self.chunk == ChunkData::Stopped {
            return Octets::NONE;
        }
        let v = VALUE[usize::from(b)];
        if v == NOT_BASE64 {
            self.chunk = ChunkData::Stopped;
            return self.short();
        }
        let acc = (self.acc << 6) | u32::from(v);
        self.held = match self.held {
            Held::Zero => Held::One,
            Held::One => Held::Two,
            Held::Two => Held::Three,
            Held::Three => {
                self.acc = 0;
                self.held = Held::Zero;
                return octets(acc, OctetCount::Three);
            }
        };
        self.acc = acc;
        Octets::NONE
    }

    /// **Payload bytes in bulk**, as [`KittyBase64::push`] on each: whole
    /// quanta four characters at a time, the rest a byte at a time. Reads
    /// the whole of `input` once the chunk has stopped, since kitty drops
    /// the rest of it, and returns early only when `out` is full.
    pub fn feed(&mut self, input: &[u8], out: &mut [u8]) -> Fed {
        let mut fed = Fed { read: 0, wrote: 0 };
        loop {
            if self.chunk == ChunkData::Stopped {
                fed.read = input.len();
                return fed;
            }
            if self.held == Held::Zero {
                // Sixteen characters at a time while all are the alphabet,
                // then four, then the byte path for the stop.
                while let (Some(chars), Some(dst)) = (
                    input
                        .get(fed.read..fed.read + BLOCK_CHARS)
                        .and_then(|s| <&[u8; BLOCK_CHARS]>::try_from(s).ok()),
                    out.get_mut(fed.wrote..fed.wrote + BLOCK_OCTETS)
                        .and_then(|s| <&mut [u8; BLOCK_OCTETS]>::try_from(s).ok()),
                ) {
                    let Some(octets) = Block::decode(*chars) else {
                        break;
                    };
                    *dst = octets;
                    fed.read += BLOCK_CHARS;
                    fed.wrote += BLOCK_OCTETS;
                }
                while let (Some(&[c0, c1, c2, c3]), Some([d0, d1, d2])) = (
                    input.get(fed.read..fed.read + 4),
                    out.get_mut(fed.wrote..fed.wrote + 3),
                ) {
                    let v = [
                        VALUE[usize::from(c0)],
                        VALUE[usize::from(c1)],
                        VALUE[usize::from(c2)],
                        VALUE[usize::from(c3)],
                    ];
                    if v.contains(&NOT_BASE64) {
                        break;
                    }
                    let word = v.iter().fold(0u32, |w, &x| (w << 6) | u32::from(x));
                    [_, *d0, *d1, *d2] = word.to_be_bytes();
                    fed.read += 4;
                    fed.wrote += 3;
                }
            }
            let Some(&b) = input.get(fed.read) else {
                return fed;
            };
            let mut next = *self;
            let o = next.push(b);
            let o = o.as_slice();
            let Some(dst) = out.get_mut(fed.wrote..fed.wrote + o.len()) else {
                return fed;
            };
            for (d, &s) in dst.iter_mut().zip(o) {
                *d = s;
            }
            fed.wrote += o.len();
            *self = next;
            fed.read += 1;
        }
    }

    /// **The chunk has ended**: a short final quantum's whole octets, unless
    /// a stop already gave them, and the decoder ready for the next chunk,
    /// which kitty decodes on its own.
    pub fn end(&mut self) -> Octets {
        let o = match self.chunk {
            ChunkData::Open => self.short(),
            ChunkData::Stopped => Octets::NONE,
        };
        *self = KittyBase64::NEW;
        o
    }

    /// The held sextets' whole octets: none of one, one of two, two of
    /// three, as libbase64 has written them by then.
    fn short(&mut self) -> Octets {
        let (word, count) = match self.held {
            Held::Zero | Held::One => (0, OctetCount::Zero),
            Held::Two => (self.acc << 12, OctetCount::One),
            Held::Three => (self.acc << 6, OctetCount::Two),
        };
        self.acc = 0;
        self.held = Held::Zero;
        octets(word, count)
    }
}

/// The top `count` octets of a 24-bit word.
fn octets(word: u32, count: OctetCount) -> Octets {
    Octets {
        bytes: [(word >> 16) as u8, (word >> 8) as u8, word as u8],
        count,
    }
}

/// Characters one [`Block`] decodes: four quanta.
const BLOCK_CHARS: usize = 16;

/// Octets one [`Block`] writes: three a quantum.
const BLOCK_OCTETS: usize = 12;

/// **Sixteen characters of the alphabet to twelve octets**, four whole
/// quanta at once (`[[source: rfc4648-base64]]` section 4), for
/// [`KittyBase64::feed`] while nothing is held. It declines -- `None`, and
/// nothing decoded -- when any of the sixteen is outside Table 1's
/// alphabet, `=` included, so the caller's four-character loop and byte
/// path meet that character exactly where they would without it.
pub(crate) struct Block;

impl Block {
    /// The octets of `chars`, or `None` if any is outside the alphabet.
    pub(crate) fn decode(chars: [u8; BLOCK_CHARS]) -> Option<[u8; BLOCK_OCTETS]> {
        #[cfg(target_endian = "little")]
        {
            Block::lanes(chars)
        }
        #[cfg(target_endian = "big")]
        {
            compile_error!(
                "baud-domain: the kitty base64 block and pixel widening have no big-endian form"
            )
        }
    }

    /// **All sixteen at once**: each character's class by range compares
    /// -- `A-Z`, `a-z`, `0-9`, `+`, `/`, Table 1's runs -- and its value
    /// by the offset its class adds, modulo 256: -65, -71, +4, +19, +16.
    /// Then each four sextets, read as one word of four bytes, the first in
    /// the lowest, are packed as `(s0 << 18) | (s1 << 12) | (s2 << 6) | s3`,
    /// and the word's three low bytes, highest first, are its quantum's
    /// octets. Reading a word's bytes lowest first is little-endian, which
    /// this form needs.
    #[cfg(target_endian = "little")]
    fn lanes(chars: [u8; BLOCK_CHARS]) -> Option<[u8; BLOCK_OCTETS]> {
        use core::simd::cmp::{SimdPartialEq, SimdPartialOrd};
        use core::simd::{Select, Simd, ToBytes, simd_swizzle};
        type V = Simd<u8, BLOCK_CHARS>;
        type W = Simd<u32, 4>;
        let c = V::from_array(chars);
        let within = |lo: u8, hi: u8| c.simd_ge(V::splat(lo)) & c.simd_le(V::splat(hi));
        let upper = within(b'A', b'Z');
        let lower = within(b'a', b'z');
        let digit = within(b'0', b'9');
        let plus = c.simd_eq(V::splat(b'+'));
        let slash = c.simd_eq(V::splat(b'/'));
        if !(upper | lower | digit | plus | slash).all() {
            return None;
        }
        let offset = upper.select(
            V::splat(0u8.wrapping_sub(65)),
            lower.select(
                V::splat(0u8.wrapping_sub(71)),
                digit.select(V::splat(4), plus.select(V::splat(19), V::splat(16))),
            ),
        );
        let w = W::from_ne_bytes(c + offset);
        let word = ((w & W::splat(0xFF)) << 18)
            | ((w & W::splat(0xFF00)) << 4)
            | ((w & W::splat(0xFF_0000)) >> 10)
            | (w >> 24);
        let octets: Simd<u8, BLOCK_OCTETS> =
            simd_swizzle!(word.to_ne_bytes(), [2, 1, 0, 6, 5, 4, 10, 9, 8, 14, 13, 12]);
        Some(octets.to_array())
    }
}

/// [`Block`] against the scalar path it stands in for.
#[cfg(test)]
mod tests {
    use super::*;

    /// The alphabet in Table 1's order.
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    /// What the scalar path makes of sixteen characters: [`KittyBase64::push`]
    /// on each from a fresh decoder, or `None` where one stops the chunk
    /// inside them.
    fn scalar(chars: [u8; BLOCK_CHARS]) -> Option<[u8; BLOCK_OCTETS]> {
        let mut d = KittyBase64::NEW;
        let mut out = [0u8; BLOCK_OCTETS];
        let mut n = 0;
        for c in chars {
            let o = d.push(c);
            if d.chunk == ChunkData::Stopped {
                return None;
            }
            for &x in o.as_slice() {
                out[n] = x;
                n += 1;
            }
        }
        Some(out)
    }

    /// **The block decodes as the scalar path**: every byte value in each of
    /// the sixteen lanes, the rest `A`, and every character of the alphabet
    /// in every lane beside the others. It declines exactly where the scalar
    /// path stops inside the sixteen, and otherwise writes its octets.
    #[test]
    fn the_block_decodes_as_the_scalar_path() {
        let mut declined = 0;
        for lane in 0..BLOCK_CHARS {
            for b in 0..=u8::MAX {
                let mut chars = [b'A'; BLOCK_CHARS];
                chars[lane] = b;
                let want = scalar(chars);
                assert_eq!(Block::decode(chars), want, "byte {b:#04x} in lane {lane}");
                declined += usize::from(want.is_none());
            }
        }
        // The control: 192 of the 256 bytes are outside the alphabet.
        assert_eq!(declined, BLOCK_CHARS * 192);
        for start in 0..ALPHABET.len() {
            let mut chars = [0u8; BLOCK_CHARS];
            for (i, c) in chars.iter_mut().enumerate() {
                *c = ALPHABET[(start + i * 5) % ALPHABET.len()];
            }
            let want = scalar(chars);
            assert!(want.is_some(), "the control: all sixteen are the alphabet");
            assert_eq!(Block::decode(chars), want, "from {start}");
        }
    }
}
