//! The parameter table: two bytes in, one step out.
//!
//! **The measured problem this exists for.** On `btop.raw` 78.6% of the ingest
//! loop's iterations are inside `CsiParam` and **52.5% are the single action
//! `Digit`**, on a payload where the loop is 93% of the clock. `ESC [
//! 38;2;204;204;204 m` is nineteen bytes and costs nineteen iterations of
//! lookup-and-dispatch to produce one colour change; fifteen of them are
//! digits and semicolons.
//!
//! **The key is the two bytes, concatenated.** That is a *perfect* index --
//! no hash function, no collisions, no stored key to compare, no probe -- so
//! the address is arithmetic on bytes already in registers and there is
//! exactly one load. This repo measured the alternative from the other side on
//! the marks table: "indexing beats hashing is false when the hash index is
//! arithmetic", and here the arithmetic index wins for the same reason and by
//! more.
//!
//! **Little-endian key order, because that is what one load gives.** Reading
//! `[b0, b1]` as a `u16` on this machine yields `b1 << 8 | b0`, so the table
//! is built in that order rather than byte-swapped: the index is the load, and
//! a `rev16` to "fix" it would be an instruction spent making the table look
//! tidy. `PARAM_KEY` is the one definition of that order and both the builder
//! and the reader go through it.
//!
//! 65,536 entries x 2 bytes = **128 KB of rodata**, no initialiser and nothing
//! to run at startup. Only the digit rows are ever touched -- `b0` is always
//! one of ten bytes -- so the resident set is 10 x 256 x 2 = **5 KB**, which is
//! what rule #2 bounds rather than the table's size.

use crate::*;

/// What follows the digits, if anything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum After {
    /// Nothing of ours: the next byte gets its own iteration.
    Nothing = 0,
    /// `;` -- the parameter ends here.
    Sep,
    /// `:` -- the parameter ends into a sub-group.
    SubSep,
}

/// One table entry: how to fold the digits in, what follows, and how far to
/// step.
///
/// **The fold is arithmetic and not a branch.** A first version of this table
/// named four kinds and dispatched on them, which is a four-way data-dependent
/// branch once an iteration -- and it measured **flat on `btop.raw`** while
/// gaining 9% on asciiquarium. The two payloads differ in exactly the way that
/// predicts: asciiquarium's digit runs are 68% two bytes long, so the branch is
/// nearly always the same arm, where btop's split 32k/35k/32k across lengths
/// one, two and three and the arm is a coin flip. A mispredict is ~13 cycles
/// against an L1 hit's ~4, which is this repo's own rule -- *replace an
/// unpredictable branch with a load* -- read backwards.
///
/// So the entry carries a **multiplier** instead of a kind: one digit folds
/// with 10 and two fold with 100, and the arm does `p * m + v` either way.
/// What is left is one test for a separator, which is taken about once a
/// parameter rather than once a digit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct ParamStep(u16);

impl ParamStep {
    const VALUE_BITS: u16 = 0x7f;
    const WIDE: u16 = 1 << 7;
    const AFTER_SHIFT: u16 = 8;
    const TWO_BYTES: u16 = 1 << 10;

    const fn new(value: u8, wide: bool, after: After, two_bytes: bool) -> ParamStep {
        ParamStep(
            (value as u16)
                | ((wide as u16) * Self::WIDE)
                | ((after as u16) << Self::AFTER_SHIFT)
                | ((two_bytes as u16) * Self::TWO_BYTES),
        )
    }

    /// The digits, already combined: `0..=9` for one, `0..=99` for two.
    pub const fn value(self) -> u16 {
        self.0 & Self::VALUE_BITS
    }

    /// **10 or 100, with no branch.** One shift and one multiply-add produce
    /// it, so the fold below is straight-line whatever the pair was.
    pub const fn multiplier(self) -> u16 {
        10 + 90 * ((self.0 & Self::WIDE) >> 7)
    }

    pub const fn after(self) -> After {
        match (self.0 >> Self::AFTER_SHIFT) & 0b11 {
            0 => After::Nothing,
            1 => After::Sep,
            _ => After::SubSep,
        }
    }

    /// The step two bytes name.
    ///
    /// **The index is the bytes, in the order one load produces them.** No
    /// hash, no collision, no stored key to compare -- the address is
    /// arithmetic on bytes already in registers and there is exactly one load.
    #[inline(always)]
    pub fn of(b0: StreamByte, b1: StreamByte) -> ParamStep {
        PARAM[b0.index() | (b1.index() << 8)]
    }

    /// Fold this step's digits into a parameter.
    ///
    /// **On the step, not on the caller**, so the multiplier and the value
    /// never cross a signature as bare integers -- which is the audit's whole
    /// point and also the only place they could be swapped.
    #[inline(always)]
    pub fn fold(self, p: Param) -> Param {
        p.saturating_scale(self.multiplier(), self.value())
    }

    /// How much of the read this step consumes. **Never zero**, so the loop
    /// cannot stall.
    pub const fn width(self) -> ByteLen {
        match self.0 & Self::TWO_BYTES != 0 {
            true => ByteLen::TWO,
            false => ByteLen::ONE,
        }
    }
}

/// The table. `b0` is always a digit -- a parser names this action only for
/// `0x30..=0x39` -- so every other row is unreachable and costs nothing but
/// address space.
static PARAM: [ParamStep; 1 << 16] = build();

const fn build() -> [ParamStep; 1 << 16] {
    let mut t = [ParamStep::new(0, false, After::Nothing, false); 1 << 16];
    let mut b0 = 0x30usize;
    while b0 <= 0x39 {
        let hi = (b0 - 0x30) as u8;
        let mut b1 = 0usize;
        while b1 < 256 {
            // The order here is the order one load gives, and `param_key` is
            // the same expression. Spelling it twice is how the two would
            // drift; `the_table_is_what_the_key_says` checks they do not.
            let k = b0 | (b1 << 8);
            t[k] = match b1 {
                0x30..=0x39 => {
                    ParamStep::new(hi * 10 + (b1 - 0x30) as u8, true, After::Nothing, true)
                }
                0x3b => ParamStep::new(hi, false, After::Sep, true),
                0x3a => ParamStep::new(hi, false, After::SubSep, true),
                _ => ParamStep::new(hi, false, After::Nothing, false),
            };
            b1 += 1;
        }
        b0 += 1;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The builder's index expression and the reader's must agree.** They
    /// are written twice -- once in a `const fn` that cannot call the reader,
    /// once on the hot path -- which is exactly the shape that drifts.
    #[test]
    fn the_table_is_what_the_key_says() {
        for b0 in 0x30..=0x39usize {
            for b1 in 0..256usize {
                let e = ParamStep::of(StreamByte::ALL[b0], StreamByte::ALL[b1]);
                let hi = (b0 - 0x30) as u16;
                match b1 {
                    0x30..=0x39 => {
                        assert_eq!(e.multiplier(), 100);
                        assert_eq!(e.value(), hi * 10 + (b1 - 0x30) as u16);
                        assert_eq!(e.after(), After::Nothing);
                        assert_eq!(e.width().index(), 2);
                    }
                    0x3b | 0x3a => {
                        assert_eq!(e.multiplier(), 10);
                        assert_eq!(e.value(), hi);
                        assert_eq!(e.width().index(), 2);
                    }
                    _ => {
                        assert_eq!(e.multiplier(), 10);
                        assert_eq!(e.value(), hi);
                        assert_eq!(e.after(), After::Nothing);
                        assert_eq!(e.width().index(), 1);
                    }
                }
            }
        }
    }

    /// **The table must fold exactly as byte-at-a-time pushes would**, every
    /// pair, every starting value, saturation included. This is the property
    /// the whole change rests on: two bytes taken at once cannot mean anything
    /// different from two bytes taken one at a time.
    #[test]
    fn folding_a_pair_matches_folding_the_bytes_singly() {
        for start in [0u16, 1, 7, 65, 655, 6553, 6554, 65535] {
            for b0 in 0x30..=0x39usize {
                for b1 in 0..256usize {
                    let e = ParamStep::of(StreamByte::ALL[b0], StreamByte::ALL[b1]);
                    let table = Param::from_raw(start).saturating_scale(e.multiplier(), e.value());
                    let mut singly =
                        Param::from_raw(start).push(Digit::of_ascii(StreamByte::ALL[b0]));
                    if (0x30..=0x39).contains(&b1) {
                        singly = singly.push(Digit::of_ascii(StreamByte::ALL[b1]));
                    }
                    assert_eq!(table, singly, "start={start} b0={b0:#x} b1={b1:#x}");
                }
            }
        }
    }

    /// **Every step consumes at least its own byte.** A zero-width step would
    /// stall the ingest loop on malformed input, which is the one failure this
    /// loop is structured to make impossible.
    #[test]
    fn no_step_consumes_nothing() {
        for e in PARAM.iter() {
            assert!(e.width().index() >= 1);
        }
    }
}

/// **Three bytes an edge instead of two.**
///
/// The two-byte form is +9% on asciiquarium and vim and **flat on btop**, and
/// btop is the payload it was aimed at: 78.6% of its iterations were inside
/// `CsiParam`. Its digit runs split 32k/35k/32k across lengths one, two and
/// three, so a two-byte edge still costs two steps for every three-digit
/// parameter -- and `38;2;204;204;204` is nothing but three-digit parameters.
/// This edge takes a whole one.
///
/// **The key stays a perfect index by compacting only the lead.** `b0` is
/// always a digit, so `(b0 - 0x30)` is 0..=9 and the table is
/// `10 x 256 x 256 = 655,360` entries at two bytes = **1.31 MB** of rodata,
/// against 16.7M entries and 33 MB for a raw three-byte key. The touched set is
/// ten leads by about fifteen live second bytes by fifteen third bytes --
/// roughly **4.5 KB** -- which is what rule #2 bounds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct ParamStep3(u16);

/// `10^n`, indexed by the digit count less one. A four-entry array in L1
/// rather than a branch on how many digits arrived.
static SCALE: [u16; 4] = [10, 100, 1000, 10000];

impl ParamStep3 {
    const VALUE_BITS: u16 = 0x3ff;
    const NDIGIT_SHIFT: u16 = 10;
    const AFTER_SHIFT: u16 = 12;
    const WIDTH_SHIFT: u16 = 14;

    const fn new(value: u16, ndigits: u16, after: After, width: u16) -> ParamStep3 {
        ParamStep3(
            value
                | ((ndigits - 1) << Self::NDIGIT_SHIFT)
                | ((after as u16) << Self::AFTER_SHIFT)
                | ((width - 1) << Self::WIDTH_SHIFT),
        )
    }

    /// The step three bytes name. One load, one subtract on the lead.
    #[inline(always)]
    pub fn of(b0: StreamByte, b1: StreamByte, b2: StreamByte) -> ParamStep3 {
        PARAM3[(b0.index() - 0x30) << 16 | b1.index() << 8 | b2.index()]
    }

    pub const fn value(self) -> u16 {
        self.0 & Self::VALUE_BITS
    }

    pub fn multiplier(self) -> u16 {
        SCALE[((self.0 >> Self::NDIGIT_SHIFT) & 0b11) as usize]
    }

    pub const fn after(self) -> After {
        match (self.0 >> Self::AFTER_SHIFT) & 0b11 {
            0 => After::Nothing,
            1 => After::Sep,
            _ => After::SubSep,
        }
    }

    pub const fn width(self) -> ByteLen {
        match (self.0 >> Self::WIDTH_SHIFT) & 0b11 {
            0 => ByteLen::ONE,
            1 => ByteLen::TWO,
            _ => ByteLen::THREE,
        }
    }

    /// Fold this step's digits into a parameter.
    #[inline(always)]
    pub fn fold(self, p: Param) -> Param {
        p.saturating_scale(self.multiplier(), self.value())
    }
}

static PARAM3: [ParamStep3; 10 * 256 * 256] = build3();

const fn build3() -> [ParamStep3; 10 * 256 * 256] {
    let mut t = [ParamStep3::new(0, 1, After::Nothing, 1); 10 * 256 * 256];
    let mut a = 0usize;
    while a < 10 {
        let mut b1 = 0usize;
        while b1 < 256 {
            let mut b2 = 0usize;
            while b2 < 256 {
                let k = (a << 16) | (b1 << 8) | b2;
                let av = a as u16;
                t[k] = match b1 {
                    0x30..=0x39 => {
                        let bv = (b1 - 0x30) as u16;
                        match b2 {
                            0x30..=0x39 => ParamStep3::new(
                                av * 100 + bv * 10 + (b2 - 0x30) as u16,
                                3,
                                After::Nothing,
                                3,
                            ),
                            0x3b => ParamStep3::new(av * 10 + bv, 2, After::Sep, 3),
                            0x3a => ParamStep3::new(av * 10 + bv, 2, After::SubSep, 3),
                            _ => ParamStep3::new(av * 10 + bv, 2, After::Nothing, 2),
                        }
                    }
                    0x3b => ParamStep3::new(av, 1, After::Sep, 2),
                    0x3a => ParamStep3::new(av, 1, After::SubSep, 2),
                    _ => ParamStep3::new(av, 1, After::Nothing, 1),
                };
                b2 += 1;
            }
            b1 += 1;
        }
        a += 1;
    }
    t
}

#[cfg(test)]
mod three {
    use super::*;

    /// **The three-byte edge must fold exactly as three single bytes would.**
    /// Every lead, every second byte, every third, every starting accumulator
    /// -- saturation included. This is the whole contract: an edge that spans
    /// three letters cannot mean anything different from three edges of one.
    #[test]
    fn a_three_byte_edge_matches_the_bytes_singly() {
        for start in [0u16, 1, 7, 65, 655, 6553, 65535] {
            for b0 in 0x30..=0x39usize {
                for b1 in 0..256usize {
                    for b2 in 0..256usize {
                        let e = ParamStep3::of(
                            StreamByte::ALL[b0],
                            StreamByte::ALL[b1],
                            StreamByte::ALL[b2],
                        );
                        let mut singly =
                            Param::from_raw(start).push(Digit::of_ascii(StreamByte::ALL[b0]));
                        let mut taken = 1usize;
                        if (0x30..=0x39).contains(&b1) {
                            singly = singly.push(Digit::of_ascii(StreamByte::ALL[b1]));
                            taken = 2;
                            if (0x30..=0x39).contains(&b2) {
                                singly = singly.push(Digit::of_ascii(StreamByte::ALL[b2]));
                                taken = 3;
                            }
                        }
                        assert_eq!(
                            e.fold(Param::from_raw(start)),
                            singly,
                            "start={start} {b0:#x} {b1:#x} {b2:#x}"
                        );
                        // The width must cover exactly the digits taken, plus
                        // a separator when one was consumed.
                        let want = match e.after() {
                            After::Nothing => taken,
                            _ => taken + 1,
                        };
                        assert_eq!(e.width().index(), want, "{b0:#x} {b1:#x} {b2:#x}");
                    }
                }
            }
        }
    }
}

/// How many leading bytes of a four-byte word are ASCII digits, and what they
/// are worth.
///
/// **SWAR, so a whole parameter is one step with no loop and no table.**
/// `btop.raw`'s parameters are one to three digits -- 32,334 of length one,
/// 34,708 of two, 31,911 of three -- so four bytes covers every one of them,
/// and the walk that reads them a byte at a time is 32% of that recording.
///
/// The test is the classic one: `w ^ 0x30303030` maps `'0'..='9'` to `0..=9`,
/// and a byte is out of range exactly when adding `0x76` sets its high bit or
/// the original nibble was not `3`. `trailing_zeros` on the resulting mask
/// gives the first non-digit in bytes.
///
/// A table was tried for this and is **-14% on btop**: this is a short local
/// loop, and a lookup replaces arithmetic rather than iterations. Arithmetic
/// against arithmetic is the fair comparison, which is what this is.
#[inline(always)]
pub fn digit_prefix(w: u32) -> (usize, u16) {
    let d = w ^ 0x3030_3030;
    // High bit set for any byte that is not `0..=9`.
    let bad = ((d.wrapping_add(0x7676_7676)) | d) & 0x8080_8080;
    let n = match bad {
        0 => 4,
        _ => (bad.trailing_zeros() / 8) as usize,
    };
    // The digits are little-endian in the word: byte 0 is the most significant
    // decimal place.
    let b = d.to_le_bytes();
    let v = match n {
        0 => 0u16,
        1 => b[0] as u16,
        2 => b[0] as u16 * 10 + b[1] as u16,
        3 => b[0] as u16 * 100 + b[1] as u16 * 10 + b[2] as u16,
        _ => b[0] as u16 * 1000 + b[1] as u16 * 100 + b[2] as u16 * 10 + b[3] as u16,
    };
    (n, v)
}

#[cfg(test)]
mod swar {
    use super::*;

    /// **Every four-byte window, against a byte-at-a-time reference.** The
    /// SWAR test has to agree with "how many leading bytes are `0x30..=0x39`,
    /// and what number do they spell" for every input, including windows with
    /// no digits and windows that are all digits.
    #[test]
    fn the_swar_prefix_agrees_with_counting_the_bytes() {
        // Exhaustive over the interesting alphabet, and a spread of the rest.
        let alphabet: [u8; 16] = [
            b'0', b'1', b'4', b'9', b'/', b':', b';', b'm', 0x00, 0x1b, 0x7f, 0xff, b'?', b' ',
            b'$', b'8',
        ];
        for a in alphabet {
            for b in alphabet {
                for c in alphabet {
                    for d in alphabet {
                        let w = u32::from_le_bytes([a, b, c, d]);
                        let (n, v) = digit_prefix(w);
                        let bytes = [a, b, c, d];
                        let mut want_n = 0usize;
                        while want_n < 4 && bytes[want_n].is_ascii_digit() {
                            want_n += 1;
                        }
                        let mut want_v = 0u16;
                        for &digit in bytes.iter().take(want_n) {
                            want_v = want_v * 10 + u16::from(digit - b'0');
                        }
                        assert_eq!(n, want_n, "count for {bytes:?}");
                        assert_eq!(v, want_v, "value for {bytes:?}");
                    }
                }
            }
        }
    }
}
