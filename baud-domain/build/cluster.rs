//! The cluster machine, `OUT_DIR/cluster_data.rs`: UAX #29's grapheme
//! cluster boundary rules (`[[source: uax29]]` section 3.1.1) as a
//! transition table over the byte the BREAK trie stores a code point
//! ([`crate::grapheme`]), four bits of class and four of lookback.
//!
//! The machine's state is what the rules look back at: the class before,
//! whether an odd run of regional indicators is open (GB12 and GB13),
//! where an `Extended_Pictographic Extend* ZWJ` stands (GB11), and where an
//! `InCB=Consonant [Extend Linker]* Linker [Extend Linker]*` stands (GB9c).
//! 270 states are nominal and 94 are reachable from the start of text, so
//! a transition is one byte: bit 7 the break, bits 0..7 the next state.
//!
//! * **CLUSTER_NEXT**, one byte a (state, input), states numbered in the
//!   order a breadth-first walk from the start of text first reaches them,
//!   inputs in byte order.
//! * **CLUSTER_READY**, the one state in which a run of ordinary
//!   characters changes nothing: the class before is `Other` and no
//!   lookback is open.
//! * **CLUSTER_PARTS_LEGACY** and **CLUSTER_PARTS_MARKS_JOINED**, one bit a
//!   (state, input), set where the extended rules join and the variant
//!   breaks: the legacy grapheme cluster, the extended rules but GB9a, GB9b
//!   and GB9c (section 3.1.1's table), and the legacy cluster with GB9a
//!   kept, where a spacing mark joins.
//!
//! **A second implementation of the rules.** The crate's `ClusterRules` is
//! the first, and a build script cannot call the crate it builds; the
//! crate's `the_cluster_table_agrees_with_the_rules` holds this table to
//! that one at every reachable state and input, and its
//! `the_parts_tables_agree_with_the_rules` the two bit tables.
//!
//! A pure function of the rules: `main` writes what [`build`] returns.

use crate::grapheme::{CLASSES, CONSONANT, INCB_EXTEND, LINKER, PICTOGRAPHIC};
use baud_tablegen::{Check, Emit, Error, Errors, Kind};
use std::collections::HashMap;

/// Where a failure of the table is reported: no data file is read.
const PATH: &str = "build/cluster.rs";

/// The states reachable from the start of text, which the crate's
/// documentation and its run gate are written for.
const STATES: usize = 94;

/// One of UAX #29's grapheme cluster boundary rules. GB2, `Any ÷ eot`, is
/// not one: the machine is asked about a character, never about the end.
/// GB12 and GB13 differ only in what stands before the indicators, the
/// start of text or a character that is not one, and are one condition on
/// the parity of the run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rule {
    Gb1,
    Gb3,
    Gb4,
    Gb5,
    Gb6,
    Gb7,
    Gb8,
    Gb9,
    Gb9a,
    Gb9b,
    Gb9c,
    Gb11,
    Gb12And13,
    Gb999,
}

/// The extended grapheme cluster's rules, in UAX #29's order: the first
/// that matches decides.
pub const EXTENDED: [Rule; 14] = [
    Rule::Gb1,
    Rule::Gb3,
    Rule::Gb4,
    Rule::Gb5,
    Rule::Gb6,
    Rule::Gb7,
    Rule::Gb8,
    Rule::Gb9,
    Rule::Gb9a,
    Rule::Gb9b,
    Rule::Gb9c,
    Rule::Gb11,
    Rule::Gb12And13,
    Rule::Gb999,
];

/// What the legacy grapheme cluster excludes (section 3.1.1's table).
pub const LEGACY_EXCLUDES: [Rule; 3] = [Rule::Gb9a, Rule::Gb9b, Rule::Gb9c];

/// What the legacy cluster with spacing marks joined excludes: GB9a kept.
pub const MARKS_JOINED_EXCLUDES: [Rule; 2] = [Rule::Gb9b, Rule::Gb9c];

impl Rule {
    /// Whether the rule joins, `×`, rather than breaks, `÷`.
    fn joins(self) -> bool {
        !matches!(self, Rule::Gb1 | Rule::Gb4 | Rule::Gb5 | Rule::Gb999)
    }
}

/// GB11's lookback: nothing open, inside `Extended_Pictographic Extend*`,
/// or past the `ZWJ` that ends it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Pictographic {
    None,
    Run,
    RunZwj,
}

/// GB9c's lookback: nothing open, a consonant with only `InCB=Extend` and
/// `InCB=Linker` since, and at least one of those the linker.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Conjunct {
    None,
    Consonant,
    ConsonantLinker,
}

/// The machine between two characters.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct State {
    /// The class of the character before, `None` at the start of text.
    prev: Option<u8>,
    /// GB12 and GB13: an odd run of regional indicators is open.
    odd: bool,
    pict: Pictographic,
    incb: Conjunct,
}

impl State {
    /// The start of text.
    pub const START: State = State {
        prev: None,
        odd: false,
        pict: Pictographic::None,
        incb: Conjunct::None,
    };
}

/// The classes the rules name, as the BREAK trie's low nibble stores them:
/// their places in [`CLASSES`].
struct Classes {
    other: u8,
    cr: u8,
    lf: u8,
    control: u8,
    extend: u8,
    zwj: u8,
    regional: u8,
    prepend: u8,
    spacing: u8,
    l: u8,
    v: u8,
    t: u8,
    lv: u8,
    lvt: u8,
}

/// The place of the class `name` in [`CLASSES`].
pub fn class(name: &str) -> u8 {
    let i = CLASSES
        .iter()
        .position(|&(n, _)| n == name)
        .expect("a class");
    u8::try_from(i).expect("14 classes")
}

impl Classes {
    fn new() -> Classes {
        Classes {
            other: class("Other"),
            cr: class("Cr"),
            lf: class("Lf"),
            control: class("Control"),
            extend: class("Extend"),
            zwj: class("Zwj"),
            regional: class("RegionalIndicator"),
            prepend: class("Prepend"),
            spacing: class("SpacingMark"),
            l: class("L"),
            v: class("V"),
            t: class("T"),
            lv: class("Lv"),
            lvt: class("Lvt"),
        }
    }
}

/// Whether `rule` matches between the state `s` and a character of class
/// `cur` in the lookback sets `lb`.
fn matches(c: &Classes, rule: Rule, s: &State, cur: u8, lb: u8) -> bool {
    let prev = s.prev;
    let was = |k: u8| prev == Some(k);
    match rule {
        // sot ÷ Any
        Rule::Gb1 => prev.is_none(),
        // CR × LF
        Rule::Gb3 => was(c.cr) && cur == c.lf,
        // (Control | CR | LF) ÷
        Rule::Gb4 => was(c.control) || was(c.cr) || was(c.lf),
        // ÷ (Control | CR | LF)
        Rule::Gb5 => cur == c.control || cur == c.cr || cur == c.lf,
        // L × (L | V | LV | LVT)
        Rule::Gb6 => was(c.l) && [c.l, c.v, c.lv, c.lvt].contains(&cur),
        // (LV | V) × (V | T)
        Rule::Gb7 => (was(c.lv) || was(c.v)) && (cur == c.v || cur == c.t),
        // (LVT | T) × T
        Rule::Gb8 => (was(c.lvt) || was(c.t)) && cur == c.t,
        // × (Extend | ZWJ)
        Rule::Gb9 => cur == c.extend || cur == c.zwj,
        // × SpacingMark
        Rule::Gb9a => cur == c.spacing,
        // Prepend ×
        Rule::Gb9b => was(c.prepend),
        // \p{InCB=Consonant} [\p{InCB=Extend}\p{InCB=Linker}]*
        // \p{InCB=Linker} [\p{InCB=Extend}\p{InCB=Linker}]* × \p{InCB=Consonant}
        Rule::Gb9c => s.incb == Conjunct::ConsonantLinker && lb & CONSONANT != 0,
        // \p{Extended_Pictographic} Extend* ZWJ × \p{Extended_Pictographic}
        Rule::Gb11 => s.pict == Pictographic::RunZwj && lb & PICTOGRAPHIC != 0,
        // sot (RI RI)* RI × RI, and [^RI] (RI RI)* RI × RI
        Rule::Gb12And13 => cur == c.regional && s.odd,
        // Any ÷ Any
        Rule::Gb999 => true,
    }
}

/// Whether `rules` break before the character: the first that matches
/// decides.
fn breaks(c: &Classes, rules: &[Rule], s: &State, cur: u8, lb: u8) -> bool {
    rules
        .iter()
        .find(|&&r| matches(c, r, s, cur, lb))
        .is_none_or(|r| !r.joins())
}

/// The state after the character. No rule's decision moves it: GB12 and
/// GB13 count the regional indicators since the last character that is
/// not one, whatever joined them to it.
fn advance(c: &Classes, s: &State, cur: u8, lb: u8) -> State {
    // An indicator after an even run opens a pair and one after an odd run
    // closes it, a Prepend before the run included:
    // `÷ 0600 × 1F1E6 × 1F1E6 ÷` by GB9b then GB13.
    let odd = cur == c.regional && !s.odd;
    // `Extend` does not end the run: GB11 looks back through it.
    let in_run = s.pict == Pictographic::Run;
    let pict = if lb & PICTOGRAPHIC != 0 || (in_run && cur == c.extend) {
        Pictographic::Run
    } else if in_run && cur == c.zwj {
        Pictographic::RunZwj
    } else {
        Pictographic::None
    };
    let incb = if lb & CONSONANT != 0 {
        Conjunct::Consonant
    } else if s.incb == Conjunct::None {
        Conjunct::None
    } else if lb & LINKER != 0 {
        Conjunct::ConsonantLinker
    } else if lb & INCB_EXTEND != 0 {
        s.incb
    } else {
        Conjunct::None
    };
    State {
        prev: Some(cur),
        odd,
        pict,
        incb,
    }
}

/// The inputs: every byte whose low nibble is a class, in byte order.
fn inputs() -> impl Iterator<Item = u8> {
    let classes = u8::try_from(CLASSES.len()).expect("14 classes");
    (0..=u8::MAX).filter(move |b| b & 0xf < classes)
}

/// `n` reachable states as a count of seven-bit state ids, recording the
/// failures: a count other than the one the crate is written for, and a
/// state id past seven bits.
pub fn check_states(n: usize, errs: &mut Errors) -> Option<u8> {
    errs.count(PATH, "reachable states", n, STATES);
    errs.take(state_ids(n))
}

/// `n` states as a state id of seven bits, or the failure.
fn state_ids(n: usize) -> Result<u8, Error> {
    u8::try_from(n).ok().filter(|&n| n <= 0x80).ok_or_else(|| {
        Error::new(
            Kind::Invariant,
            PATH,
            format!("{n} states will not fit a seven-bit state id"),
        )
    })
}

/// The one ready state of `ready`, or the failure.
pub fn the_ready_state(ready: &[bool]) -> Result<u8, Error> {
    let all: Vec<usize> = (0..ready.len()).filter(|&i| ready[i]).collect();
    match all[..] {
        [one] => Ok(u8::try_from(one).expect("a state id")),
        _ => Err(Error::new(
            Kind::Invariant,
            PATH,
            format!("ready states {all:?}, exactly one expected"),
        )),
    }
}

/// The states reachable from the start of text, in the order a
/// breadth-first walk first reaches them, and the id of each. The map is
/// only looked up, never iterated.
fn reachable(c: &Classes) -> (Vec<State>, HashMap<State, usize>) {
    let mut order = vec![State::START];
    let mut id = HashMap::from([(State::START, 0)]);
    let mut i = 0;
    while i < order.len() {
        let s = order[i];
        i += 1;
        for b in inputs() {
            let (cur, lb) = (b & 0xf, b >> 4);
            let next = advance(c, &s, cur, lb);
            if let std::collections::hash_map::Entry::Vacant(v) = id.entry(next) {
                v.insert(order.len());
                order.push(next);
            }
        }
    }
    (order, id)
}

/// The tables, built by UAX #29's extended rules.
pub fn build() -> Result<Emit, Errors> {
    build_with(&EXTENDED)
}

/// The tables, built by `extended` as the extended rules, each variant
/// those without what it excludes: [`build`], or a rule dropped to see a
/// check fire.
pub fn build_with(extended: &[Rule]) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let c = Classes::new();
    let without = |out: &[Rule]| -> Vec<Rule> {
        extended
            .iter()
            .copied()
            .filter(|r| !out.contains(r))
            .collect()
    };
    let (legacy, marks) = (without(&LEGACY_EXCLUDES), without(&MARKS_JOINED_EXCLUDES));
    let (order, id) = reachable(&c);
    let Some(n) = check_states(order.len(), &mut errs) else {
        return Err(errs);
    };

    let mut next = vec![0u8; order.len() * 256];
    let mut parts_legacy = vec![0u8; order.len() * 256 / 8];
    let mut parts_marks = vec![0u8; order.len() * 256 / 8];
    for (si, s) in order.iter().enumerate() {
        for b in inputs() {
            let (cur, lb) = (b & 0xf, b >> 4);
            let brk = breaks(&c, extended, s, cur, lb);
            let to = id[&advance(&c, s, cur, lb)];
            let i = si * 256 + usize::from(b);
            next[i] = (u8::from(brk) << 7) | u8::try_from(to).expect("a state id");
            for (rules, bits) in [(&legacy, &mut parts_legacy), (&marks, &mut parts_marks)] {
                if !brk && breaks(&c, rules, s, cur, lb) {
                    bits[i >> 3] |= 1 << (i & 7);
                }
            }
        }
    }
    let ready: Vec<bool> = order
        .iter()
        .map(|s| {
            s.prev == Some(c.other)
                && !s.odd
                && s.pict == Pictographic::None
                && s.incb == Conjunct::None
        })
        .collect();
    let ready = errs.take(the_ready_state(&ready));

    // Positive controls: one sequence a rule, walked through the table
    // from the start of text, each with the breaks the rule gives it, and
    // where the legacy variants part what the extended rules join.
    let byte = |class: u8, lb: u8| class | (lb << 4);
    let consonant = byte(c.other, CONSONANT);
    let virama = byte(c.extend, LINKER);
    let pictograph = byte(c.other, PICTOGRAPHIC);
    let zwj = byte(c.zwj, INCB_EXTEND);
    let ri = byte(c.regional, 0);
    let walk = |text: &[u8]| -> Vec<bool> {
        let mut s = 0usize;
        text.iter()
            .map(|&b| {
                let e = next[s * 256 + usize::from(b)];
                s = usize::from(e & 0x7f);
                e & 0x80 != 0
            })
            .collect()
    };
    for (what, text, want) in [
        ("GB1, an Extend at the start", vec![c.extend], vec![true]),
        ("GB3, CR LF", vec![c.cr, c.lf], vec![true, false]),
        (
            "GB4, Control Extend",
            vec![c.control, c.extend],
            vec![true, true],
        ),
        ("GB5, Prepend LF", vec![c.prepend, c.lf], vec![true, true]),
        (
            "GB6 and GB7, L V T",
            vec![c.l, c.v, c.t],
            vec![true, false, false],
        ),
        ("GB8, LVT T", vec![c.lvt, c.t], vec![true, false]),
        (
            "GB9, Other Extend",
            vec![c.other, c.extend],
            vec![true, false],
        ),
        (
            "GB9a, Other SpacingMark",
            vec![c.other, c.spacing],
            vec![true, false],
        ),
        (
            "GB9b, Prepend Other",
            vec![c.prepend, c.other],
            vec![true, false],
        ),
        (
            "GB9c, a consonant, a virama, a consonant",
            vec![consonant, virama, consonant],
            vec![true, false, false],
        ),
        (
            "GB11, a pictograph, ZWJ, a pictograph",
            vec![pictograph, zwj, pictograph],
            vec![true, false, false],
        ),
        (
            "GB12 and GB13, RI RI RI",
            vec![ri, ri, ri],
            vec![true, false, true],
        ),
        (
            "GB13, Prepend RI RI RI",
            vec![c.prepend, ri, ri, ri],
            vec![true, false, false, true],
        ),
        (
            "GB999, Other Other",
            vec![c.other, c.other],
            vec![true, true],
        ),
    ] {
        let got = walk(&text);
        errs.require(Check::from(got == want), || {
            Error::new(
                Kind::Control,
                PATH,
                format!("{what} breaks {got:?}, {want:?} expected"),
            )
        });
    }
    let parts = |bits: &[u8], text: &[u8]| -> bool {
        let mut s = 0usize;
        let (last, head) = text.split_last().expect("a character");
        for &b in head {
            s = usize::from(next[s * 256 + usize::from(b)] & 0x7f);
        }
        let i = s * 256 + usize::from(*last);
        bits[i >> 3] >> (i & 7) & 1 != 0
    };
    for (what, text, legacy, marks) in [
        ("a spacing mark", vec![c.other, c.spacing], true, false),
        (
            "a character after a Prepend",
            vec![c.prepend, c.other],
            true,
            true,
        ),
        (
            "a consonant after a virama",
            vec![consonant, virama, consonant],
            true,
            true,
        ),
        ("an Extend", vec![c.other, c.extend], false, false),
    ] {
        let got = (parts(&parts_legacy, &text), parts(&parts_marks, &text));
        errs.require(Check::from(got == (legacy, marks)), || {
            Error::new(
                Kind::Control,
                PATH,
                format!(
                    "{what} parts {got:?} under (legacy, spacing marks joined), \
                     {:?} expected",
                    (legacy, marks)
                ),
            )
        });
    }
    let Some(ready) = ready else {
        return Err(errs);
    };

    let mut out = Emit::new(
        "cluster_data",
        "baud-domain/build.rs (build/cluster.rs) from UAX #29's rules [uax29]",
        &[],
    );
    out.bytes(
        "CLUSTER_NEXT",
        &[
            "UAX #29's extended rules as a transition table, one byte a (state,",
            "input): bit 7 is \"break here\", bits 0..7 the next state.",
            &format!("{n} states x 256 inputs, the input the BREAK trie's byte: the class in"),
            "the low four bits, the lookback sets in the high four.",
        ],
        &next,
    );
    out.code(&format!(
        "/// The one state in which a run of ordinary characters is provably a\n\
         /// no-op -- the class before is `Other` with no lookback live, so\n\
         /// `ordinary_run_ready` is a compare against this.\n\
         pub const CLUSTER_READY: u8 = {ready};\n\
         \n\
         /// States reachable from the start of text.\n\
         #[allow(dead_code)]\n\
         pub const CLUSTER_STATES: usize = {n};"
    ));
    out.bytes(
        "CLUSTER_PARTS_LEGACY",
        &[
            "**Where a join parts under legacy clusters**: one bit a (state,",
            "input), bit `i & 7` of byte `i >> 3` for `i = state * 256 + input`,",
            "set where the extended rules join and the legacy ones -- without",
            "GB9a, GB9b and GB9c -- break. Read only where the machine joined.",
        ],
        &parts_legacy,
    );
    out.bytes(
        "CLUSTER_PARTS_MARKS_JOINED",
        &["The same, with spacing marks joined: without GB9b and GB9c."],
        &parts_marks,
    );
    errs.or(out)
}
