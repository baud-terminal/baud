//! xterm's stack controls and its title modes (`[[source: ctlseqs]]`):
//! XTTITLEPOS, which reports the title stack, and XTSMTITLE and XTRMTITLE,
//! which choose how a program's title is read.
//!
//! The stacks themselves live in the core. What is here is the grammar:
//! which control was asked, and what each title mode is.

use crate::{ModeChange, Param};

/// One of xterm's stack controls. **Fieldless**, as [`crate::AreaOp`] is: its
/// parameters are read where it is performed, so `csi_final` keeps no more
/// of them live than it did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackOp {
    /// XTTITLEPOS, `CSI # S`: "Report position on title-stack ... CSI Pn;
    /// Pm # S where Pn is the current index into the title stack, Pm is the
    /// maximum index for the title stack" ([`crate::Report::TitlePosition`]).
    TitlePosition,
}

/// One of XTSMTITLE's features, by its `Ps`: "Ps = 0 -> Set window/icon
/// labels using hexadecimal. Ps = 1 -> Query window/icon labels using
/// hexadecimal. Ps = 2 -> Set window/icon labels using UTF-8. Ps = 3 ->
/// Query window/icon labels using UTF-8" (`[[source: ctlseqs]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TitleMode {
    SetHex,
    QueryHex,
    SetUtf8,
    QueryUtf8,
}

impl TitleMode {
    /// The feature `p` names, or `None` past 3, which xterm's
    /// `ValidTitleMode` refuses (`[[source: xterm-reset]]`'s file).
    pub fn from_param(p: Param) -> Option<TitleMode> {
        match p.value() {
            0 => Some(TitleMode::SetHex),
            1 => Some(TitleMode::QueryHex),
            2 => Some(TitleMode::SetUtf8),
            3 => Some(TitleMode::QueryUtf8),
            _ => None,
        }
    }

    /// Its `Ps`.
    pub fn code(self) -> u8 {
        match self {
            TitleMode::SetHex => 0,
            TitleMode::QueryHex => 1,
            TitleMode::SetUtf8 => 2,
            TitleMode::QueryUtf8 => 3,
        }
    }

    /// Whether the feature shapes a title report, `CSI 20 t` or `CSI 21 t`,
    /// which baud refuses: such a mode is refused with them and never
    /// recorded, since nothing would read it.
    pub fn is_query(self) -> bool {
        matches!(self, TitleMode::QueryHex | TitleMode::QueryUtf8)
    }

    /// The bit it takes in [`TitleModes`], xterm's `xBIT(Ps)`.
    fn bit(self) -> u8 {
        1 << self.code()
    }
}

/// The title modes in force, one bit a feature as xterm's `title_modes`
/// keeps them. Only the two set modes are ever recorded
/// ([`TitleMode::is_query`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TitleModes(u8);

impl TitleModes {
    /// "The initial (compiled-in) default", xterm's `DEF_TITLE_MODES`, 0
    /// (`[[source: xterm-main-h]]`): no feature set. XTSMTITLE and XTRMTITLE
    /// with no parameter both return to it, and so does `RIS`.
    pub const DEFAULT: TitleModes = TitleModes(0);

    pub fn has(self, m: TitleMode) -> bool {
        self.0 & m.bit() != 0
    }

    /// The modes with `m` set or reset.
    pub fn with(self, m: TitleMode, change: ModeChange) -> TitleModes {
        match change {
            ModeChange::Set => TitleModes(self.0 | m.bit()),
            ModeChange::Reset => TitleModes(self.0 & !m.bit()),
        }
    }

    /// The modes set, lowest `Ps` first.
    pub fn set(self) -> impl Iterator<Item = TitleMode> {
        [
            TitleMode::SetHex,
            TitleMode::QueryHex,
            TitleMode::SetUtf8,
            TitleMode::QueryUtf8,
        ]
        .into_iter()
        .filter(move |&m| self.has(m))
    }
}

/// Decode a title sent in hexadecimal into `out`, as xterm's `x_decode_hex`
/// and `ChangeGroup` read one (`[[source: xterm-xstrings-c]]`,
/// `[[source: xterm-misc]]`): **every byte a hex digit, either case, and an
/// even count of them**, or `None` and the title is left as it was; the
/// decoded name ends at its first byte below 32. Returns the bytes kept.
pub fn decode_hex_title<'o>(hex: &[u8], out: &'o mut [u8]) -> Option<&'o [u8]> {
    if !hex.len().is_multiple_of(2) || hex.len() / 2 > out.len() {
        return None;
    }
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
    let mut n = 0;
    for pair in hex.chunks_exact(2) {
        out[n] = (digit(pair[0])? << 4) | digit(pair[1])?;
        n += 1;
    }
    let end = out[..n].iter().position(|&b| b < 32).unwrap_or(n);
    Some(&out[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hex_title_decodes_in_either_case_and_ends_at_a_control() {
        let mut out = [0u8; 16];
        assert_eq!(decode_hex_title(b"4869", &mut out), Some(&b"Hi"[..]));
        assert_eq!(decode_hex_title(b"6a4B", &mut out), Some(&b"jK"[..]));
        assert_eq!(decode_hex_title(b"41074243", &mut out), Some(&b"A"[..]));
        assert_eq!(decode_hex_title(b"", &mut out), Some(&b""[..]));
    }

    #[test]
    fn an_odd_count_or_a_non_digit_is_no_title() {
        let mut out = [0u8; 16];
        assert_eq!(decode_hex_title(b"486", &mut out), None);
        assert_eq!(decode_hex_title(b"48 69", &mut out), None);
        assert_eq!(decode_hex_title(b"zz", &mut out), None);
    }

    #[test]
    fn the_title_modes_set_and_reset_one_bit_each() {
        let m = TitleModes::DEFAULT
            .with(TitleMode::SetHex, ModeChange::Set)
            .with(TitleMode::SetUtf8, ModeChange::Set);
        assert!(m.has(TitleMode::SetHex) && m.has(TitleMode::SetUtf8));
        assert!(!m.has(TitleMode::QueryHex));
        let m = m.with(TitleMode::SetHex, ModeChange::Reset);
        let mut set = m.set();
        assert_eq!((set.next(), set.next()), (Some(TitleMode::SetUtf8), None));
    }
}
