//! ReGIS numerics: `[[source: dec-std-070]]` 070-8 § 8.4.3.3, "integer,
//! fixed point, floating point, and scientific notation formats are
//! accepted", decoded a character at a time.
//!
//! **Not truncated until the number is whole.** § 8.4.3.3.1: "all
//! implementations must RECOGNIZE when fractions occur, and not truncate to
//! integers until the number is COMPLETELY parsed and decoded. ... The
//! format 1.02e2 must resolve to 102, not 100." So the digits are kept as an
//! integer mantissa and a power of ten, and turned into a value once, at the
//! character that ends the number, in units of 1/65536: the precision a
//! high-resolution device is "expected to use" (§ 8.5.1.4.1.3).

/// A decoded ReGIS number, in units of 1/65536.
///
/// § 8.4.3.3.1 asks for "signed integer arithmetic in a range no less than
/// -32767 to +32767"; a value is held to `±2^31` whole units, so the
/// arithmetic a coordinate goes through -- a relative step, a scale to the
/// device -- cannot overflow the `i64` it is kept in.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub struct RegisNum(i64);

/// One whole unit.
const ONE: i64 = 1 << 16;
/// The largest magnitude kept: `2^31` whole units.
const LIMIT: i64 = 1 << 47;

impl RegisNum {
    /// Zero.
    pub const ZERO: RegisNum = RegisNum(0);
    /// One.
    pub const ONE: RegisNum = RegisNum(ONE);

    /// `n` whole units.
    pub const fn int(n: i32) -> RegisNum {
        RegisNum(n as i64 * ONE)
    }

    /// The value in units of 1/65536.
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// A value from units of 1/65536, held to the range.
    pub const fn from_raw(v: i64) -> RegisNum {
        RegisNum(if v > LIMIT {
            LIMIT
        } else if v < -LIMIT {
            -LIMIT
        } else {
            v
        })
    }

    /// The whole part, toward zero: the integer a low-resolution device
    /// keeps (§ 8.5.1.4.1.3, "must extract the integer portions").
    pub const fn trunc(self) -> i32 {
        (self.0 / ONE) as i32
    }

    /// To the nearest whole unit, a half away from zero.
    pub const fn round(self) -> i32 {
        let h = ONE / 2;
        if self.0 >= 0 {
            ((self.0 + h) / ONE) as i32
        } else {
            -(((-self.0) + h) / ONE) as i32
        }
    }

    /// Whether the value is below zero.
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// The sum, held to the range.
    pub const fn plus(self, o: RegisNum) -> RegisNum {
        RegisNum::from_raw(self.0 + o.0)
    }

    /// The difference, held to the range.
    pub const fn minus(self, o: RegisNum) -> RegisNum {
        RegisNum::from_raw(self.0 - o.0)
    }

    /// The negation.
    pub const fn neg(self) -> RegisNum {
        RegisNum(-self.0)
    }

    /// The magnitude.
    pub const fn abs(self) -> RegisNum {
        RegisNum(self.0.abs())
    }

    /// The product, held to the range.
    pub const fn times(self, o: RegisNum) -> RegisNum {
        let p = (self.0 as i128 * o.0 as i128) >> 16;
        RegisNum::from_raw(clamp128(p))
    }
}

const fn clamp128(v: i128) -> i64 {
    if v > LIMIT as i128 {
        LIMIT
    } else if v < -(LIMIT as i128) {
        -LIMIT
    } else {
        v as i64
    }
}

/// Where a number is in § 8.4.3.3's grammar:
///
/// ```text
/// <number>    ::= [+|-] <unsgnreal> [<exponent>]
/// <unsgnreal> ::= <digits>* [.] <digits>+ | <digits>+ [.] <digits>*
/// <exponent>  ::= E[+|-]<digits>+
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NumAt {
    /// Nothing yet, or only a sign.
    Start,
    /// In the whole part.
    Whole,
    /// After the decimal point.
    Fraction,
    /// After the `E`, which is the exponent's only if digits follow.
    Exp,
    /// After the exponent's sign.
    ExpSign,
    /// In the exponent's digits.
    ExpDigits,
}

/// What a character did to a number in progress.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NumStep {
    /// It is part of the number.
    Took,
    /// It ends the number and is read again after it (§ 8.4.3.3: "The
    /// character causing the termination is rescanned as part of the next
    /// token following the number").
    Ends,
}

/// Which sign a number or its exponent was written with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Signed {
    None,
    Plus,
    Minus,
}

/// Whether any digit of the mantissa arrived.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Digits {
    None,
    Some,
}

/// A number being read: the mantissa's digits as an integer, the power of
/// ten they stand at, and the exponent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct NumAcc {
    pub(crate) sign: Signed,
    mant: u64,
    /// Powers of ten the mantissa is short by: whole digits past the
    /// eighteen it keeps, less the fraction digits it took.
    scale: i32,
    exp: i32,
    exp_sign: Signed,
    pub(crate) at: NumAt,
    pub(crate) digits: Digits,
}

/// The mantissa digits kept; any more only move the power of ten.
const MANT_DIGITS: u64 = 1_000_000_000_000_000_000;

impl NumAcc {
    pub(crate) const NEW: NumAcc = NumAcc {
        sign: Signed::None,
        mant: 0,
        scale: 0,
        exp: 0,
        exp_sign: Signed::None,
        at: NumAt::Start,
        digits: Digits::None,
    };

    /// A number begun by `c`, a digit, a sign or a decimal point.
    pub(crate) fn begin(c: u8) -> NumAcc {
        let mut n = NumAcc::NEW;
        let _ = n.step(c);
        n
    }

    /// Whether `c` can begin a number.
    pub(crate) const fn starts(c: u8) -> bool {
        matches!(c, b'0'..=b'9' | b'+' | b'-' | b'.')
    }

    /// Take `c`, or say it ends the number. A second sign, a second point or
    /// a second `E` ends it, the grammar admitting none of them (§ 8.4.3.3:
    /// "(Y1E2E3) would have to be interpreted as (Y100,E3)").
    pub(crate) fn step(&mut self, c: u8) -> NumStep {
        match (self.at, c) {
            (NumAt::Start, b'+') if self.sign == Signed::None && self.digits == Digits::None => {
                self.sign = Signed::Plus;
                NumStep::Took
            }
            (NumAt::Start, b'-') if self.sign == Signed::None && self.digits == Digits::None => {
                self.sign = Signed::Minus;
                NumStep::Took
            }
            (NumAt::Start | NumAt::Whole, b'0'..=b'9') => {
                self.digit(c - b'0', Part::Whole);
                self.at = NumAt::Whole;
                NumStep::Took
            }
            (NumAt::Start | NumAt::Whole, b'.') => {
                self.at = NumAt::Fraction;
                NumStep::Took
            }
            (NumAt::Fraction, b'0'..=b'9') => {
                self.digit(c - b'0', Part::Fraction);
                NumStep::Took
            }
            (NumAt::Whole | NumAt::Fraction, b'E' | b'e') if self.digits == Digits::Some => {
                self.at = NumAt::Exp;
                NumStep::Took
            }
            (NumAt::Exp, b'+') => {
                self.exp_sign = Signed::Plus;
                self.at = NumAt::ExpSign;
                NumStep::Took
            }
            (NumAt::Exp, b'-') => {
                self.exp_sign = Signed::Minus;
                self.at = NumAt::ExpSign;
                NumStep::Took
            }
            (NumAt::Exp | NumAt::ExpSign | NumAt::ExpDigits, b'0'..=b'9') => {
                self.exp = (self.exp * 10 + i32::from(c - b'0')).min(100);
                self.at = NumAt::ExpDigits;
                NumStep::Took
            }
            _ => NumStep::Ends,
        }
    }

    fn digit(&mut self, d: u8, part: Part) {
        self.digits = Digits::Some;
        if self.mant < MANT_DIGITS / 10 {
            self.mant = self.mant * 10 + u64::from(d);
            if part == Part::Fraction {
                self.scale -= 1;
            }
        } else if part == Part::Whole {
            self.scale += 1;
        }
    }

    /// The characters an unfinished exponent took and gave back: an `E`
    /// with no digit after it, and the sign after it, are not the number's
    /// (§ 8.4.3.3's `<exponent>` needs a digit), so they are read again as
    /// what they would be alone -- the `E` an option letter.
    pub(crate) fn unread(&self) -> &'static [u8] {
        match (self.at, self.exp_sign) {
            (NumAt::Exp, _) => b"E",
            (NumAt::ExpSign, Signed::Plus) => b"E+",
            (NumAt::ExpSign, _) => b"E-",
            _ => b"",
        }
    }

    /// The value, or `None` with no digit: a sign or a point alone is "a
    /// sign character or decimal point with no digits", an error whose
    /// recovery the text leaves to the device; it is read as no number.
    pub(crate) fn value(&self) -> Option<RegisNum> {
        if self.digits == Digits::None {
            return None;
        }
        let exp = match self.at {
            NumAt::ExpDigits => match self.exp_sign {
                Signed::Minus => -self.exp,
                _ => self.exp,
            },
            _ => 0,
        };
        let p = (self.scale + exp).clamp(-40, 40);
        let m = self.mant as i128 * ONE as i128;
        let v = if p >= 0 {
            let mut v = m;
            let mut i = 0;
            while i < p && v.unsigned_abs() <= (LIMIT as u128) * 10 {
                v *= 10;
                i += 1;
            }
            v
        } else {
            let d = pow10(-p);
            (m + d / 2) / d
        };
        let v = clamp128(v);
        Some(RegisNum::from_raw(match self.sign {
            Signed::Minus => -v,
            _ => v,
        }))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Whole,
    Fraction,
}

const fn pow10(n: i32) -> i128 {
    let mut v: i128 = 1;
    let mut i = 0;
    while i < n && i < 38 {
        v *= 10;
        i += 1;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(s: &[u8]) -> (Option<RegisNum>, usize) {
        let mut n = NumAcc::NEW;
        for (i, &c) in s.iter().enumerate() {
            if n.step(c) == NumStep::Ends {
                return (n.value(), i);
            }
        }
        (n.value(), s.len())
    }

    #[test]
    fn the_standards_own_examples_decode_as_it_says() {
        // § 8.4.3.3.1: "The format 1.02e2 must resolve to 102, not 100."
        assert_eq!(read(b"1.02e2").0, Some(RegisNum::int(102)));
        // "the form 100000e-3 resolves to 100".
        assert_eq!(read(b"100000e-3").0, Some(RegisNum::int(100)));
        assert_eq!(read(b"+15").0, Some(RegisNum::int(15)));
        assert_eq!(read(b"-25").0, Some(RegisNum::int(-25)));
        assert_eq!(read(b".1E3").0, Some(RegisNum::int(100)));
        assert_eq!(read(b"1000E-1").0, Some(RegisNum::int(100)));
        assert_eq!(read(b"6.25").0.map(|v| v.raw()), Some(6 * 65536 + 16384));
        // "(Y1E2E3) would have to be interpreted as (Y100,E3)".
        assert_eq!(read(b"1E2E3"), (Some(RegisNum::int(100)), 3));
    }

    #[test]
    fn an_e_with_no_digit_is_given_back() {
        let mut n = NumAcc::NEW;
        for &c in b"12E" {
            assert_eq!(n.step(c), NumStep::Took);
        }
        assert_eq!(n.step(b')'), NumStep::Ends);
        assert_eq!(n.value(), Some(RegisNum::int(12)));
        assert_eq!(n.unread(), b"E");
    }

    #[test]
    fn a_sign_alone_is_no_number() {
        assert_eq!(read(b"+]").0, None);
        assert_eq!(read(b".,").0, None);
    }

    #[test]
    fn a_huge_number_is_held_to_the_range() {
        assert_eq!(
            read(b"99999999999999999999999999").0.map(|v| v.raw()),
            Some(LIMIT)
        );
        assert_eq!(read(b"-1e40").0.map(|v| v.raw()), Some(-LIMIT));
        assert_eq!(read(b"1e-40").0, Some(RegisNum::ZERO));
    }
}
