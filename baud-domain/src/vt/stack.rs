//! xterm's stack controls and its title modes (`[[source: ctlseqs]]`):
//! XTTITLEPOS, which reports the title stack; XTPUSHSGR and XTPOPSGR, which
//! save and restore the rendition; and XTSMTITLE and XTRMTITLE, which
//! choose how a program's title is read.
//!
//! The stacks themselves live in the core. What is here is the grammar:
//! which control was asked, and what each title mode is.

use crate::{Attrs, ModeChange, Param, StyleWord};

/// One of xterm's stack controls. **Fieldless**, as [`crate::AreaOp`] is: its
/// parameters are read where it is performed, so `csi_final` keeps no more
/// of them live than it did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackOp {
    /// XTTITLEPOS, `CSI # S`: "Report position on title-stack ... CSI Pn;
    /// Pm # S where Pn is the current index into the title stack, Pm is the
    /// maximum index for the title stack" ([`crate::Report::TitlePosition`]).
    TitlePosition,
    /// XTPUSHSGR, `CSI Pm # {` and its alias `CSI Pm # p`: "Push video
    /// attributes onto stack".
    PushRendition,
    /// XTPOPSGR, `CSI # }` and its alias `CSI # q`: "Pop video attributes
    /// from stack".
    PopRendition,
}

/// "The stack is limited to 10 levels" (`[[source: ctlseqs]]`, XTPUSHSGR),
/// xterm's `MAX_SAVED_SGR` (`[[source: xterm-ptyx-h]]`), the colour stack's
/// depth as well.
pub const SGR_STACK_DEPTH: usize = 10;

/// Which aspects of the rendition an XTPUSHSGR saved, as xterm's mask holds
/// them: bit `n - 1` for the SGR group `n` (`[[source: xterm-reset]]`'s
/// file, `CASE_XTERM_PUSH_SGR`). A bare push saves every aspect, the whole
/// rendition; a list saves the groups it names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SgrGroups(u32);

impl SgrGroups {
    /// "If no parameters are given, all of the video attributes are saved":
    /// every bit, so a bare push is told from any list, which reaches bit
    /// 30 at most.
    pub const ALL: SgrGroups = SgrGroups(u32::MAX);
    pub const NONE: SgrGroups = SgrGroups(0);

    /// The groups with `p` added: a value from 1 to 31 takes its bit, as
    /// xterm's `MAX_PUSH_SGR` bounds it, and any other, 0 among them, adds
    /// nothing. **10 and 11 name no group**: "the optional parameters
    /// correspond to the SGR encoding", and SGR 10 and 11 are fonts, which
    /// the text lists no group for. xterm reads them as 30 and 31,
    /// "deprecated - for compatibility" (`[[source: xterm-reset]]`'s file);
    /// Jev's lean for that, ALIAS, was 0.94/0.86, below the 0.9 in both
    /// orders a departure needs, so the text is built.
    pub fn with(self, p: Param) -> SgrGroups {
        match p.value() {
            n @ 1..=31 => SgrGroups(self.0 | 1 << (n - 1)),
            _ => self,
        }
    }

    fn has(self, group: u16) -> bool {
        self.0 & (1 << (group - 1)) != 0
    }
}

/// The rendition XTPOPSGR leaves: `now` with the groups `saved_as` names
/// taken from `saved`, as xterm's `xtermPopSGR` restores them
/// (`[[source: xterm-misc]]`). A bare push restores the whole rendition,
/// what xterm has and what it does not -- an underline colour, a font,
/// fraktur, overline, framing, the ideogram lines -- which no listed group
/// names and a list therefore leaves as they are (Jev's lean, NEAREST,
/// 0.86/0.38).
///
/// **5 restores both blink rates**, one aspect by the owner's call of
/// 2026-10-13 ("Own rate, exclusive"). **4 and 21 restore xterm's two
/// underline bits** onto baud's one underline style (Jev's lean,
/// XTERM_BITS, 0.28/0.18): 4 is whether a single or styled underline is
/// in force, 21 whether a double one is, and with one of them alone the
/// other keeps its current value.
pub fn restore_rendition(saved: StyleWord, now: StyleWord, saved_as: SgrGroups) -> StyleWord {
    if saved_as == SgrGroups::ALL {
        return saved;
    }
    let (was, is) = (saved.attrs(), now.attrs());
    let mut attrs = is;
    for (group, bits) in [
        (1, Attrs::BOLD),
        (2, Attrs::DIM),
        (3, Attrs::ITALIC),
        (5, Attrs::BLINKS),
        (7, Attrs::REVERSE),
        (8, Attrs::HIDDEN),
        (9, Attrs::STRIKE),
    ] {
        if saved_as.has(group) {
            attrs = attrs.without(bits).with(Attrs(was.0 & bits.0));
        }
    }
    let family = |a: Attrs| Attrs(a.0 & Attrs::UNDERLINES.0);
    let double = |a: Attrs| a.has(Attrs::DOUBLE_UNDERLINE);
    let underline = match (saved_as.has(4), saved_as.has(21)) {
        (true, true) => family(was),
        (true, false) if double(is) => family(is),
        (true, false) if double(was) => Attrs::NONE,
        (true, false) => family(was),
        (false, true) if double(was) => Attrs::DOUBLE_UNDERLINE,
        (false, true) if double(is) => Attrs::NONE,
        (false, _) => family(is),
    };
    attrs = attrs.without(Attrs::UNDERLINES).with(underline);
    let mut w = now.with_attrs(attrs);
    if saved_as.has(30) {
        w = w.with_fg(saved.fg());
    }
    if saved_as.has(31) {
        w = w.with_bg(saved.bg());
    }
    w
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

/// What two renditions share, for XTREPORTSGR's "attributes which are
/// common to all cells": the attributes both have, and each colour and the
/// font where both agree, else the default. Every aspect baud records, as
/// DECRQSS's SGR answer carries them (Jev, ALL_COMMON, 0.96/0.98), where
/// xterm's `xtermReportSGR` keeps its own subset (`[[source: xterm-misc]]`).
pub fn common_rendition(a: StyleWord, b: StyleWord) -> StyleWord {
    let same = |x: crate::ColorWord, y: crate::ColorWord| match x == y {
        true => x,
        false => crate::ColorWord::DEFAULT,
    };
    StyleWord::DEFAULT
        .with_attrs(Attrs(a.attrs().0 & b.attrs().0))
        .with_fg(same(a.fg(), b.fg()))
        .with_bg(same(a.bg(), b.bg()))
        .with_underline(same(a.underline(), b.underline()))
        .with_font(match a.font() == b.font() {
            true => a.font(),
            false => crate::FontId::PRIMARY,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Chan, ColorWord};

    fn groups(list: &[u16]) -> SgrGroups {
        list.iter()
            .fold(SgrGroups::NONE, |g, &n| g.with(Param::from_raw(n)))
    }

    #[test]
    fn a_list_restores_only_the_groups_it_names() {
        let red = ColorWord::indexed(Chan::from_u8(1));
        let saved = StyleWord::DEFAULT.with_attrs(Attrs::BOLD).with_fg(red);
        let now = StyleWord::DEFAULT.with_attrs(Attrs::ITALIC).with_bg(red);
        let w = restore_rendition(saved, now, groups(&[1, 30]));
        assert_eq!(w.attrs(), Attrs::BOLD.with(Attrs::ITALIC));
        assert_eq!((w.fg(), w.bg()), (red, red));
        assert_eq!(restore_rendition(saved, now, SgrGroups::ALL), saved);
        assert_eq!(restore_rendition(saved, now, SgrGroups::NONE), now);
    }

    #[test]
    fn ten_and_eleven_name_no_group() {
        assert_eq!(groups(&[10, 11]).0 & groups(&[30, 31]).0, 0);
        assert_eq!(groups(&[0, 32, 65535]), SgrGroups::NONE);
    }

    #[test]
    fn four_and_twenty_one_are_xterms_two_underline_bits() {
        let u = |a| StyleWord::DEFAULT.with_attrs(a);
        let single = u(Attrs::UNDERLINE);
        let curly = u(Attrs::CURLY_UNDERLINE);
        let double = u(Attrs::DOUBLE_UNDERLINE);
        let none = StyleWord::DEFAULT;
        let r = |s, n, g: &[u16]| restore_rendition(s, n, groups(g)).attrs();
        assert_eq!(r(curly, double, &[4, 21]), Attrs::CURLY_UNDERLINE);
        assert_eq!(r(curly, none, &[4]), Attrs::CURLY_UNDERLINE);
        assert_eq!(r(none, single, &[4]), Attrs::NONE);
        assert_eq!(r(none, double, &[4]), Attrs::DOUBLE_UNDERLINE);
        assert_eq!(r(double, single, &[4]), Attrs::NONE);
        assert_eq!(r(double, single, &[21]), Attrs::DOUBLE_UNDERLINE);
        assert_eq!(r(none, double, &[21]), Attrs::NONE);
        assert_eq!(r(none, curly, &[21]), Attrs::CURLY_UNDERLINE);
    }

    #[test]
    fn five_restores_both_blink_rates() {
        let saved = StyleWord::DEFAULT.with_attrs(Attrs::RAPID_BLINK);
        let now = StyleWord::DEFAULT.with_attrs(Attrs::BLINK);
        assert_eq!(
            restore_rendition(saved, now, groups(&[5])).attrs(),
            Attrs::RAPID_BLINK
        );
    }

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
