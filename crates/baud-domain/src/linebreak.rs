//! **Unicode's line breaking algorithm**, UAX #14 revision 55 for Unicode
//! 17.0.0 (`[[source: uax14]]`): where a line of text may break, must
//! break, and must not. Each code point's class is one indexed load from a
//! table compiled from `LineBreak.txt` (`build/linebreak.rs`), and the
//! rules LB2 to LB31 are read at each boundary in the order the annex
//! gives them, the first that applies deciding.
//!
//! **Two entry points.** [`line_breaks`] is the algorithm with its default
//! rules and no tailoring, and
//! `every_line_break_test_case_breaks_where_it_says` holds every case of
//! Unicode's `LineBreakTest.txt` to it. [`line_breaks_in_paths`] is the
//! same algorithm with one tailoring, for file paths and URLs, which the
//! display's word wrap uses.
//!
//! **The tailoring, disclosed as UAX14-C1 requires** ("If the
//! implementation tailors the behavior of Section 6.2, Tailorable Line
//! Breaking Rules, that fact must be disclosed"). Inside a *path run* a
//! line breaks **before** a separator, not after it, so `/home/jordan`
//! wraps as `/home` then `/jordan`. A run is a stretch with no space, hard
//! break or ZW, past any opening bracket or quotation mark that leads it,
//! and it is a path run when it starts `/`, `~/`, `./` or `../`, holds
//! `://`, starts with a drive `X:\` or a UNC `\\` (where `\` separates
//! as `/` does), or holds two or more `/` and is not a number form, a run
//! built only from LB25's classes (NU SY IS PR PO OP CL CP HY). Every other
//! run keeps the default rules: `1/2`, `06/07/99`, `w/o`, `and/or` and
//! `src/main.rs` break exactly as UAX #14 breaks them. In a path run, with
//! *sep* a separator and *seg* a letter, a digit or `.`:
//!
//! * **T1** `SP ÷` before the run's first separator or its first
//!   character, so a path moves to the next line whole (`cd ÷/home`,
//!   `cd ÷../etc`), where LB13 forbids a break before `/` "even after
//!   spaces" and LB15d one before the `.` of `../`; not where the spaces
//!   follow an opening bracket or quotation mark, which LB14 and LB15a
//!   keep.
//! * **T2** `sep × sep`: never inside `//`, `///` or `\\`.
//! * **T3** `: × sep`: `https://` and `C:\` stay whole.
//! * **T4** no break before the separator after a segment of exactly `~`,
//!   `.` or `..`: `~/`, `./` and `../` stay whole.
//! * **B1** `seg ÷ sep` before a separator a segment follows, where LB13
//!   (`× SY`) and, for `\`, LB24 (`(AL | HL) × PR`) forbid it.
//! * **B2** `sep × seg`, removing LB31's break after the separator, except
//!   after a `://` run, where the default break stays.
//!
//! The rules are read after LB12a and before LB13, so no decision of
//! Section 6.1 (LB4-LB12a) can change: they alter LB13, LB24 and LB31
//! only, which § 8.1 permits ("alteration, removal, or addition of rules
//! applied after rule LB12"). A path run's digits break at its separators
//! too, which is the annex's own § 8 URL example ("suppresses SY × NU from
//! rule LB25 within the scope of a URL"). Finding the path runs is one
//! pass over the text, a class read per code point, and the class sets
//! the rules name are bit sets built at compile time.

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
pub fn line_breaks(cps: &[char], f: impl FnMut(usize, LineBreak)) {
    breaks(cps, Rules::Default, f);
}

/// **[`line_breaks`] with the path tailoring** this module's header
/// discloses: inside a path run a line breaks before a separator and not
/// after it, and a path after a space may move to the next line whole.
/// Outside every path run each boundary is [`line_breaks`]'s.
pub fn line_breaks_in_paths(cps: &[char], f: impl FnMut(usize, LineBreak)) {
    breaks(cps, Rules::Paths, f);
}

/// Which rules a boundary is read under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rules {
    Default,
    Paths,
}

fn breaks(cps: &[char], rules: Rules, mut f: impl FnMut(usize, LineBreak)) {
    let t = Text { cps };
    let mut run = PathRun::NONE;
    for i in 1..cps.len() {
        if rules == Rules::Paths && i >= run.end {
            run = PathRun::at(cps, i);
        }
        f(i, boundary(&t, i, &run));
    }
    if !cps.is_empty() {
        f(cps.len(), LineBreak::Mandatory);
    }
}

/// A set of classes, one bit a class by its place in [`LineClass::ALL`],
/// built at compile time.
#[derive(Clone, Copy)]
struct Classes(u64);

impl Classes {
    const fn of(cs: &[LineClass]) -> Classes {
        let mut bits = 0u64;
        let mut i = 0;
        while i < cs.len() {
            bits |= 1 << (cs[i] as u8);
            i += 1;
        }
        Classes(bits)
    }

    fn has(self, c: LineClass) -> bool {
        self.0 >> (c as u8) & 1 == 1
    }
}

/// What ends a run: a space, a hard break, ZW.
const RUN_ENDS: Classes = Classes::of(&[SP, BK, CR, LF, NL, ZW]);
/// What may lead a run before its first character: `(/usr`, `'~/x`.
const LEADS: Classes = Classes::of(&[OP, QU, QuPi, QuPf]);
/// LB25's alphabet: a run of these alone is a number form, `06/07/99`.
const NUMBER_FORM: Classes = Classes::of(&[NU, SY, IS, PR, PO, OP, CL, CP, HY]);
/// The classes of a path's segment, beside `.`.
const SEGMENT: Classes = Classes::of(&[AL, HL, NU]);

/// What separates a path run's segments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Separators {
    /// Not a path run.
    None,
    /// `/`.
    Slash,
    /// `/` and `\`: a drive `X:\` or UNC `\\` run.
    Both,
}

/// **The run a boundary is in**: from `start` to `end`, its first
/// character past any lead at `body`, and what separates its segments.
#[derive(Clone, Copy, Debug)]
struct PathRun {
    start: usize,
    body: usize,
    end: usize,
    seps: Separators,
}

impl PathRun {
    const NONE: PathRun = PathRun {
        start: 0,
        body: 0,
        end: 0,
        seps: Separators::None,
    };

    /// The run holding `cps[from]`, read to its end once: a run-ending
    /// character is a run of its own that is no path.
    fn at(cps: &[char], from: usize) -> PathRun {
        let class = |c: char| LineProps::of(c).class;
        if RUN_ENDS.has(class(cps[from])) {
            return PathRun {
                start: from,
                body: from,
                end: from + 1,
                seps: Separators::None,
            };
        }
        // The run started at the last run end before `from`.
        let start = (0..from)
            .rev()
            .find(|&k| RUN_ENDS.has(class(cps[k])))
            .map_or(0, |k| k + 1);
        let end = (from..cps.len())
            .find(|&k| RUN_ENDS.has(class(cps[k])))
            .unwrap_or(cps.len());
        let lead = cps[start..end]
            .iter()
            .take_while(|&&c| LEADS.has(class(c)))
            .count();
        let body = start + lead;
        let run = &cps[body..end];
        let windows =
            (run.len() >= 3 && run[0].is_ascii_alphabetic() && run[1] == ':' && run[2] == '\\')
                || run.starts_with(&['\\', '\\']);
        let rooted = run.starts_with(&['/'])
            || run.starts_with(&['~', '/'])
            || run.starts_with(&['.', '/'])
            || run.starts_with(&['.', '.', '/']);
        let scheme = run.windows(3).any(|w| w == [':', '/', '/']);
        let slashes = run.iter().filter(|&&c| c == '/').count();
        let number = run.iter().all(|&c| NUMBER_FORM.has(class(c)));
        let seps = match (windows, rooted || scheme || (slashes >= 2 && !number)) {
            (true, _) => Separators::Both,
            (false, true) => Separators::Slash,
            (false, false) => Separators::None,
        };
        PathRun {
            start,
            body,
            end,
            seps,
        }
    }

    /// What separates the segments at `k`: the run's, inside it.
    fn seps_at(&self, k: usize) -> Separators {
        match (self.start..self.end).contains(&k) {
            true => self.seps,
            false => Separators::None,
        }
    }

    /// Whether `cps[k]` is a separator of the run.
    fn sep(&self, cps: &[char], k: usize) -> bool {
        match self.seps_at(k) {
            Separators::None => false,
            Separators::Slash => cps[k] == '/',
            Separators::Both => matches!(cps[k], '/' | '\\'),
        }
    }
}

/// **The path tailoring at the boundary before `cps[b]`**, read after
/// LB12a and before LB13 (this module's header): `Some` decides, `None`
/// leaves the boundary to the default rules.
fn paths(t: &Text<'_>, b: usize, pa: usize, run: &PathRun) -> Option<LineBreak> {
    use LineBreak::{Allowed, Prohibited};
    if run.seps_at(b) == Separators::None {
        return None;
    }
    let cps = t.cps;
    let sep = |k: usize| run.sep(cps, k);
    // T1: a path after a space moves to the next line whole, but not
    // after an opening bracket or quotation mark and spaces (LB14, LB15a).
    // Its first character is a separator, or the `.` of `./` and `../`,
    // which LB15d (`× IS`) holds to the space.
    if t.class(pa) == SP && (sep(b) || b == run.body) {
        let opened = t
            .over_spaces(pa)
            .is_some_and(|k| t.class(k).quote() || t.class(k) == OP);
        return match opened {
            true => None,
            false => Some(Allowed),
        };
    }
    if run.seps_at(pa) == Separators::None {
        return None;
    }
    let seg = |k: usize| !sep(k) && (SEGMENT.has(t.class(k)) || cps[k] == '.');
    // T3, T2: never after a colon or inside a separator run.
    if (cps[pa] == ':' || sep(pa)) && sep(b) {
        return Some(Prohibited);
    }
    // The segment that ends at `k`, back to the separator or the run's
    // lead before it.
    let segment = |k: usize| {
        let mut j = k;
        while j > run.body && !sep(j - 1) {
            j -= 1;
        }
        &cps[j..=k]
    };
    // B2: no break after a separator before a segment, but after a run of
    // two or more that a colon leads, a URL's `://`.
    if sep(pa) && seg(b) {
        let mut n = 0;
        let mut j = pa;
        while j >= run.body && sep(j) {
            n += 1;
            match j.checked_sub(1) {
                Some(k) => j = k,
                None => break,
            }
        }
        let authority = n >= 2 && j >= run.body && cps[j] == ':' && !sep(j);
        return match authority {
            true => Some(Allowed),
            false => Some(Prohibited),
        };
    }
    // B1, T4: a break before a separator a segment follows, but not after
    // `~`, `.` or `..`.
    if seg(pa) && sep(b) && t.after(b).is_some_and(seg) {
        return match segment(pa) {
            ['~'] | ['.'] | ['.', '.'] => Some(Prohibited),
            _ => Some(Allowed),
        };
    }
    None
}

/// The rules at the boundary before `cps[b]`, `0 < b < len`, with the path
/// tailoring inside `run` when it is a path run.
fn boundary(t: &Text<'_>, b: usize, run: &PathRun) -> LineBreak {
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
    // The path tailoring, after every rule of Section 6.1.
    if let Some(d) = paths(t, b, pa, run) {
        return d;
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
