//! **Unicode's line breaking algorithm**, UAX #14 revision 55 for Unicode
//! 17.0.0 (`[[source: uax14]]`), with its default rules and no tailoring:
//! where a line of text may break, must break, and must not. Each code
//! point's class is one indexed load from a table generated from
//! `LineBreak.txt` (`linebreak_data.rs`), and the rules LB2 to LB31 are
//! read at each boundary in the order the annex gives them, the first that
//! applies deciding. `every_line_break_test_case_breaks_where_it_says`
//! holds every case of Unicode's `LineBreakTest.txt` to it.

use crate::linebreak_data::{LINE_BLOCKS, LINE_INDEX};

/// A Line_Break class, resolved as LB1 resolves it with no other criteria:
/// AI, SG and XX are AL, SA is CM for a mark and AL otherwise, CJ is NS.
/// `QuPi` and `QuPf` are QU whose General_Category is Pi and Pf, which
/// LB15a and LB15b single out. The order is the table's, pinned by
/// `the_line_trie_agrees_with_the_class_enum`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(clippy::upper_case_acronyms)]
pub enum LineClass {
    AL,
    BA,
    BB,
    BK,
    B2,
    CB,
    CL,
    CM,
    CP,
    CR,
    EB,
    EM,
    EX,
    GL,
    H2,
    H3,
    HH,
    HL,
    HY,
    ID,
    IN,
    IS,
    JL,
    JT,
    JV,
    LF,
    NL,
    NS,
    NU,
    OP,
    PO,
    PR,
    QU,
    RI,
    SP,
    SY,
    WJ,
    ZW,
    ZWJ,
    AK,
    AP,
    AS,
    VF,
    VI,
    QuPi,
    QuPf,
}

use LineClass::*;

impl LineClass {
    /// Every class, in the table's order.
    pub const ALL: [LineClass; 46] = [
        AL, BA, BB, BK, B2, CB, CL, CM, CP, CR, EB, EM, EX, GL, H2, H3, HH, HL, HY, ID, IN, IS, JL,
        JT, JV, LF, NL, NS, NU, OP, PO, PR, QU, RI, SP, SY, WJ, ZW, ZWJ, AK, AP, AS, VF, VI, QuPi,
        QuPf,
    ];

    /// Whether it is QU, whatever its General_Category.
    fn quote(self) -> bool {
        matches!(self, QU | QuPi | QuPf)
    }
}

/// What the table says of one character: its class, whether it is East
/// Asian (East_Asian_Width F, W or H, the annex's `$EastAsian`), and
/// whether it is Extended_Pictographic and unassigned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineProps {
    pub class: LineClass,
    pub east_asian: EastAsian,
    pub pictographic: Unassigned,
}

/// Whether a character is East Asian for LB19a and LB30.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EastAsian {
    Yes,
    No,
}

/// Whether a character is an unassigned Extended_Pictographic one, which
/// LB30b keeps with a following emoji modifier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unassigned {
    Pictographic,
    No,
}

impl LineProps {
    /// The properties of `c`: one load from each stage of the table.
    pub fn of(c: char) -> LineProps {
        let cp = u32::from(c);
        let block = LINE_INDEX[usize::try_from(cp >> 8).unwrap_or(0)];
        let at = usize::from(block) * 256 + usize::try_from(cp & 0xFF).unwrap_or(0);
        let byte = LINE_BLOCKS[at];
        LineProps {
            class: LineClass::ALL[usize::from(byte & 0x3F)],
            east_asian: match byte & 0x40 {
                0 => EastAsian::No,
                _ => EastAsian::Yes,
            },
            pictographic: match byte & 0x80 {
                0 => Unassigned::No,
                _ => Unassigned::Pictographic,
            },
        }
    }
}

/// What may happen before a character.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineBreak {
    /// The line must end here: after a hard line break, and at the end.
    Mandatory,
    /// The line may end here.
    Allowed,
    /// It must not.
    Prohibited,
}

/// U+25CC DOTTED CIRCLE, which LB28a names.
const DOTTED_CIRCLE: char = '\u{25CC}';

/// The text and its classes, read by position.
struct Text<'t> {
    cps: &'t [char],
}

impl Text<'_> {
    fn props(&self, i: usize) -> LineProps {
        LineProps::of(self.cps[i])
    }

    fn raw(&self, i: usize) -> LineClass {
        self.props(i).class
    }

    /// **LB9**: whether the CM or ZWJ at `i` joins the character before it,
    /// whose class it then takes. A base is any class but BK, CR, LF, NL, SP
    /// and ZW, and a joined mark's base is its base's.
    fn joined(&self, i: usize) -> bool {
        if i == 0 || !matches!(self.raw(i), CM | ZWJ) {
            return false;
        }
        !matches!(self.base(i - 1).1, BK | CR | LF | NL | SP | ZW)
    }

    /// The character `i` is joined to, or itself, and its class: LB9's
    /// base, and LB10's AL for a mark left alone.
    fn base(&self, mut i: usize) -> (usize, LineClass) {
        while self.joined(i) {
            i -= 1;
        }
        let c = match self.raw(i) {
            CM | ZWJ => AL,
            c => c,
        };
        (i, c)
    }

    /// The class the rules after LB9 see at `i`.
    fn class(&self, i: usize) -> LineClass {
        self.base(i).1
    }

    /// The nearest character before `i` that is no joined mark, the
    /// position the rules after LB9 see before it.
    fn before(&self, i: usize) -> Option<usize> {
        let mut j = i.checked_sub(1)?;
        while self.joined(j) {
            j = j.checked_sub(1)?;
        }
        Some(j)
    }

    /// The nearest character after `i` that is no joined mark.
    fn after(&self, i: usize) -> Option<usize> {
        let mut j = i + 1;
        while j < self.cps.len() && self.joined(j) {
            j += 1;
        }
        (j < self.cps.len()).then_some(j)
    }

    /// From the character at `i` back over spaces (`SP*`), the first that
    /// is not one, if any.
    fn over_spaces(&self, i: usize) -> Option<usize> {
        let mut j = i;
        while self.class(j) == SP {
            j = self.before(j)?;
        }
        Some(j)
    }

    fn east_asian(&self, i: usize) -> bool {
        let (b, _) = self.base(i);
        // A mark LB10 makes AL has U+0041's East_Asian_Width, Na.
        !matches!(self.raw(b), CM | ZWJ) && self.props(b).east_asian == EastAsian::Yes
    }

    fn dotted(&self, i: usize) -> bool {
        self.cps[self.base(i).0] == DOTTED_CIRCLE
    }
}

/// **The break before each character of `cps` after the first, and at the
/// end**: `f(i, b)` for each `i` from 1 to `cps.len()`, where `i` is the
/// boundary before `cps[i]` and `cps.len()` the end of the text. LB2 never
/// breaks at the start, and LB3 always at the end, so an empty text has
/// no boundaries.
pub fn line_breaks(cps: &[char], mut f: impl FnMut(usize, LineBreak)) {
    let t = Text { cps };
    for i in 1..cps.len() {
        f(i, boundary(&t, i));
    }
    if !cps.is_empty() {
        f(cps.len(), LineBreak::Mandatory);
    }
}

/// The rules at the boundary before `cps[b]`, `0 < b < len`.
fn boundary(t: &Text<'_>, b: usize) -> LineBreak {
    use LineBreak::{Allowed, Mandatory, Prohibited};
    let a = b - 1;
    let (ra, rb) = (t.raw(a), t.raw(b));
    // LB4, LB5: hard line breaks.
    if ra == BK {
        return Mandatory;
    }
    if ra == CR && rb == LF {
        return Prohibited;
    }
    if matches!(ra, CR | LF | NL) {
        return Mandatory;
    }
    // LB6, LB7.
    if matches!(rb, BK | CR | LF | NL | SP | ZW) {
        return Prohibited;
    }
    // LB8: ZW SP* ÷.
    let mut j = Some(a);
    while let Some(k) = j.filter(|&k| t.raw(k) == SP) {
        j = k.checked_sub(1);
    }
    if j.is_some_and(|k| t.raw(k) == ZW) {
        return Allowed;
    }
    // LB8a: ZWJ ×.
    if ra == ZWJ {
        return Prohibited;
    }
    // LB9: X (CM | ZWJ)* is X.
    if t.joined(b) {
        return Prohibited;
    }
    let (ca, cb) = (t.class(a), t.class(b));
    let pa = t.before(b).unwrap_or(a);
    let before_pa = t.before(pa);
    let next = t.after(b);
    let next_class = next.map(|n| t.class(n));
    // LB11, LB12, LB12a.
    if cb == WJ || ca == WJ || ca == GL {
        return Prohibited;
    }
    if cb == GL && !matches!(ca, SP | BA | HY | HH) {
        return Prohibited;
    }
    // LB13.
    if matches!(cb, CL | CP | EX | SY) {
        return Prohibited;
    }
    let spaced = t.over_spaces(pa);
    let spaced_class = spaced.map(|k| t.class(k));
    // LB14: OP SP* ×.
    if spaced_class == Some(OP) {
        return Prohibited;
    }
    // LB15a: (sot | BK | CR | LF | NL | OP | QU | GL | SP | ZW) [Pi & QU] SP* ×.
    if let Some(q) = spaced.filter(|&k| t.class(k) == QuPi) {
        let opens = match t.before(q) {
            None => true,
            Some(k) => {
                let c = t.class(k);
                c.quote() || matches!(c, BK | CR | LF | NL | OP | GL | SP | ZW)
            }
        };
        if opens {
            return Prohibited;
        }
    }
    // LB15b: × [Pf & QU] (SP | GL | WJ | CL | QU | CP | EX | IS | SY | BK | CR
    // | LF | NL | ZW | eot).
    if cb == QuPf {
        let closes = match next_class {
            None => true,
            Some(c) => {
                c.quote()
                    || matches!(
                        c,
                        SP | GL | WJ | CL | CP | EX | IS | SY | BK | CR | LF | NL | ZW
                    )
            }
        };
        if closes {
            return Prohibited;
        }
    }
    // LB15c: SP ÷ IS NU.
    if ca == SP && cb == IS && next_class == Some(NU) {
        return Allowed;
    }
    // LB15d.
    if cb == IS {
        return Prohibited;
    }
    // LB16: (CL | CP) SP* × NS.
    if cb == NS && matches!(spaced_class, Some(CL | CP)) {
        return Prohibited;
    }
    // LB17: B2 SP* × B2.
    if cb == B2 && spaced_class == Some(B2) {
        return Prohibited;
    }
    // LB18.
    if ca == SP {
        return Allowed;
    }
    // LB19: × [QU - Pi], [QU - Pf] ×.
    if matches!(cb, QU | QuPf) || matches!(ca, QU | QuPi) {
        return Prohibited;
    }
    // LB19a, unless surrounded by East Asian characters.
    if cb.quote() {
        if !t.east_asian(pa) {
            return Prohibited;
        }
        if next.is_none_or(|n| !t.east_asian(n)) {
            return Prohibited;
        }
    }
    if ca.quote() {
        if !t.east_asian(b) {
            return Prohibited;
        }
        if before_pa.is_none_or(|k| !t.east_asian(k)) {
            return Prohibited;
        }
    }
    // LB20.
    if cb == CB || ca == CB {
        return Allowed;
    }
    // LB20a: (sot | BK | CR | LF | NL | SP | ZW | CB | GL) (HY | HH) × (AL | HL).
    if matches!(ca, HY | HH) && matches!(cb, AL | HL) {
        let initial =
            before_pa.is_none_or(|k| matches!(t.class(k), BK | CR | LF | NL | SP | ZW | CB | GL));
        if initial {
            return Prohibited;
        }
    }
    // LB21.
    if matches!(cb, BA | HH | HY | NS) || ca == BB {
        return Prohibited;
    }
    // LB21a: HL (HY | HH) × [^HL].
    if matches!(ca, HY | HH) && cb != HL && before_pa.is_some_and(|k| t.class(k) == HL) {
        return Prohibited;
    }
    // LB21b, LB22.
    if (ca == SY && cb == HL) || cb == IN {
        return Prohibited;
    }
    // LB23, LB23a, LB24.
    let letter = |c: LineClass| matches!(c, AL | HL);
    if (letter(ca) && cb == NU) || (ca == NU && letter(cb)) {
        return Prohibited;
    }
    if (ca == PR && matches!(cb, ID | EB | EM)) || (matches!(ca, ID | EB | EM) && cb == PO) {
        return Prohibited;
    }
    if (matches!(ca, PR | PO) && letter(cb)) || (letter(ca) && matches!(cb, PR | PO)) {
        return Prohibited;
    }
    // LB25: numbers.
    let number_before = |from: usize| {
        let mut k = Some(from);
        while let Some(j) = k.filter(|&j| matches!(t.class(j), SY | IS)) {
            k = t.before(j);
        }
        k.is_some_and(|j| t.class(j) == NU)
    };
    if matches!(cb, PO | PR) {
        let from = match ca {
            CL | CP => before_pa,
            _ => Some(pa),
        };
        if from.is_some_and(number_before) {
            return Prohibited;
        }
    }
    if matches!(ca, PO | PR) {
        if cb == NU {
            return Prohibited;
        }
        if cb == OP {
            let n2 = next.and_then(|n| t.after(n)).map(|n| t.class(n));
            if next_class == Some(NU) || (next_class == Some(IS) && n2 == Some(NU)) {
                return Prohibited;
            }
        }
    }
    if matches!(ca, HY | IS) && cb == NU {
        return Prohibited;
    }
    if cb == NU && number_before(pa) {
        return Prohibited;
    }
    // LB26, LB27: Korean syllable blocks.
    if (ca == JL && matches!(cb, JL | JV | H2 | H3))
        || (matches!(ca, JV | H2) && matches!(cb, JV | JT))
        || (matches!(ca, JT | H3) && cb == JT)
    {
        return Prohibited;
    }
    let hangul = |c: LineClass| matches!(c, JL | JV | JT | H2 | H3);
    if (hangul(ca) && cb == PO) || (ca == PR && hangul(cb)) {
        return Prohibited;
    }
    // LB28.
    if letter(ca) && letter(cb) {
        return Prohibited;
    }
    // LB28a: Brahmic orthographic syllables.
    let base = |k: usize| matches!(t.class(k), AK | AS) || t.dotted(k);
    let akd = |k: usize| t.class(k) == AK || t.dotted(k);
    if ca == AP && base(b) {
        return Prohibited;
    }
    if base(pa) && matches!(cb, VF | VI) {
        return Prohibited;
    }
    if ca == VI && akd(b) && before_pa.is_some_and(base) {
        return Prohibited;
    }
    if base(pa) && base(b) && next_class == Some(VF) {
        return Prohibited;
    }
    // LB29.
    if ca == IS && letter(cb) {
        return Prohibited;
    }
    // LB30.
    if matches!(ca, AL | HL | NU) && cb == OP && !t.east_asian(b) {
        return Prohibited;
    }
    if ca == CP && !t.east_asian(pa) && matches!(cb, AL | HL | NU) {
        return Prohibited;
    }
    // LB30a: an odd count of regional indicators before.
    if ca == RI && cb == RI {
        let mut n = 0usize;
        let mut k = Some(pa);
        while let Some(j) = k.filter(|&j| t.class(j) == RI) {
            n += 1;
            k = t.before(j);
        }
        if n % 2 == 1 {
            return Prohibited;
        }
    }
    // LB30b.
    if cb == EM && (ca == EB || t.props(t.base(pa).0).pictographic == Unassigned::Pictographic) {
        return Prohibited;
    }
    // LB31.
    Allowed
}
