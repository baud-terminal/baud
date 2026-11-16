//! The search tables, `OUT_DIR/search_data.rs`: what a search compares. A
//! search matches a query against the text by **canonical caseless
//! match**, `[[source: unicode17-ch3]]` D145: NFD(toCasefold(NFD(X))). R4's
//! toCasefold maps each character C to Case_Folding(C), "the mappings with
//! the status field value C and F" in CaseFolding.txt
//! (`[[source: unicode-casefolding]]`); NFD is the full canonical
//! decomposition of UnicodeData.txt's field 5, its untagged mappings
//! applied until none applies, then the canonical ordering of field 3's
//! combining classes (`[[source: unicode-data]]`). Hangul syllables
//! decompose by arithmetic (section 3.12) and are not listed.
//!
//! Per code point the table holds three things, all indexed and none
//! searched:
//!
//! * `d`: its full canonical decomposition, the first NFD;
//! * `g`: the canonical decomposition of each character of its full case
//!   folding, in order -- toCasefold and the second NFD's decomposition in
//!   one lookup, applied to what the first NFD left;
//! * `ccc`: its canonical combining class, for the two reorderings.
//!
//! Each is a two-stage trie on `codepoint >> 7` to a block of property
//! indices, numbered in code point order; a property names its two
//! sequences in one pool, where index 0 is "the code point itself". The
//! trie is read back at every code point before it is returned.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use crate::ucd::{self, at};
use baud_tablegen::{CODE_POINTS, Check, Emit, Errors, Input, Kind, LAST, PerLine, Source, Trie};
use std::collections::{BTreeMap, HashMap};

pub const UNICODE_DATA: Input = Input {
    file: "unicode-unicodedata.txt",
    key: "unicode-data",
    header: "0000;<control>;Cc;0;BN;;;;;N;NULL;;;;",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 2] = [UNICODE_DATA, ucd::CASE_FOLDING];

const SHIFT: u32 = 7;

/// Section 3.12's Hangul syllable constants.
const S_BASE: u32 = 0xAC00;
const S_COUNT: u32 = 11172;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;

fn is_hangul(cp: u32) -> bool {
    (S_BASE..S_BASE + S_COUNT).contains(&cp)
}

/// A Hangul syllable's decomposition, by section 3.12's arithmetic.
fn hangul(cp: u32) -> Vec<u32> {
    let s = cp - S_BASE;
    let mut out = vec![
        L_BASE + s / (V_COUNT * T_COUNT),
        V_BASE + (s % (V_COUNT * T_COUNT)) / T_COUNT,
    ];
    if !s.is_multiple_of(T_COUNT) {
        out.push(T_BASE + s % T_COUNT);
    }
    out
}

/// What the tables are computed from.
pub struct Ucd {
    pub ccc: Vec<u8>,
    pub decomp: HashMap<u32, Vec<u32>>,
    pub fold: BTreeMap<u32, Vec<u32>>,
}

impl Ucd {
    /// The full canonical decomposition, Hangul included.
    pub fn full(&self, cp: u32) -> Vec<u32> {
        if is_hangul(cp) {
            return hangul(cp);
        }
        match self.decomp.get(&cp) {
            None => vec![cp],
            Some(d) => d.iter().flat_map(|&x| self.full(x)).collect(),
        }
    }

    /// The table's `d`: a Hangul syllable is the code's, by arithmetic.
    pub fn listed(&self, cp: u32) -> Vec<u32> {
        if is_hangul(cp) {
            vec![cp]
        } else {
            self.full(cp)
        }
    }

    /// The table's `g`: read only on what the first NFD left, which holds
    /// no Hangul syllable, and no folding produces one.
    pub fn folded(&self, cp: u32) -> Vec<u32> {
        match self.fold.get(&cp) {
            None => self.listed(cp),
            Some(f) => f.iter().flat_map(|&x| self.listed(x)).collect(),
        }
    }
}

/// Each code point's combining class and canonical decomposition, from
/// UnicodeData.txt's fields 3 and 5; a Hangul syllable with a listed
/// decomposition is refused.
fn unicode_data(src: &Source, errs: &mut Errors) -> (Vec<u8>, HashMap<u32, Vec<u32>>) {
    let mut ccc = vec![0u8; CODE_POINTS];
    let mut decomp = HashMap::new();
    for entry in src.unicode_data() {
        let Some(e) = errs.take(entry) else {
            continue;
        };
        let class = e.row.field(3).parse::<u8>().map_err(|_| {
            e.row
                .error(Kind::Malformed, 3, "a combining class is a number 0 to 254")
        });
        let Some(class) = errs.take(class) else {
            continue;
        };
        for cp in e.range.iter() {
            ccc[at(cp)] = class;
        }
        let d = e.row.field(5);
        if d.is_empty() || d.starts_with('<') {
            continue;
        }
        let Some(d) = errs.take(e.row.code_points(5, 0)) else {
            continue;
        };
        if e.range.iter().any(is_hangul) {
            errs.push(e.row.error(
                Kind::Invariant,
                5,
                format!(
                    "{} is a Hangul syllable with a listed decomposition",
                    e.range
                ),
            ));
            continue;
        }
        for cp in e.range.iter() {
            decomp.insert(cp, d.clone());
        }
    }
    (ccc, decomp)
}

/// The sequence at pool index `i`, `cp` itself at 0.
pub fn pooled(pool: &[u32], i: u16, cp: u32) -> Vec<u32> {
    let i = usize::from(i);
    if i == 0 {
        return vec![cp];
    }
    let n = usize::try_from(pool[i]).unwrap_or(0);
    pool[i + 1..i + 1 + n].to_vec()
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    let Ok([ud, cf]) = <[Source; 2]>::try_from(loaded) else {
        return Err(errs);
    };
    let (ccc, decomp) = unicode_data(&ud, &mut errs);
    let fold = ucd::case_folding(&cf, &mut errs);
    let u = Ucd { ccc, decomp, fold };
    for (cp, to) in &u.fold {
        errs.require(Check::from(!to.iter().any(|&x| is_hangul(x))), || {
            cf.error(
                Kind::Invariant,
                format!("U+{cp:04X} folds to a Hangul syllable"),
            )
        });
    }
    // Positive controls: sharp s, KELVIN SIGN, capital sharp s and the
    // combining ypogegrammeni fold as D145 needs; A with ring and ANGSTROM
    // SIGN decompose alike; I folds to i, not Turkic dotless i.
    for (cp, want) in [
        (0xDF, &[0x73, 0x73][..]),
        (0x212A, &[0x6B]),
        (0x1E9E, &[0x73, 0x73]),
        (0x345, &[0x3B9]),
    ] {
        let got = u.folded(cp);
        errs.require(Check::from(got == want), || {
            cf.error(
                Kind::Control,
                format!("U+{cp:04X} folds to {got:X?}, {want:X?} expected"),
            )
        });
    }
    for cp in [0xC5, 0x212B] {
        let got = u.full(cp);
        errs.require(Check::from(got == [0x41, 0x30A]), || {
            ud.error(
                Kind::Control,
                format!("U+{cp:04X} decomposes to {got:X?}, [41, 30A] expected"),
            )
        });
    }
    errs.require(
        Check::from(u.fold.get(&0x49).map(Vec::as_slice) == Some(&[0x69][..])),
        || {
            cf.error(
                Kind::Control,
                "U+0049 must fold to U+0069, not Turkic U+0131",
            )
        },
    );
    if !errs.is_empty() {
        return Err(errs);
    }

    let mut pool: Vec<u32> = vec![0];
    let mut seqs: HashMap<Vec<u32>, u16> = HashMap::new();
    let mut seq = |cps: Vec<u32>, cp: u32| -> u16 {
        if cps == [cp] {
            return 0;
        }
        if let Some(&i) = seqs.get(&cps) {
            return i;
        }
        // Past a u16, the check on the pool's length below refuses it.
        let i = u16::try_from(pool.len()).unwrap_or(0);
        pool.push(u32::try_from(cps.len()).expect("a short sequence"));
        pool.extend(&cps);
        seqs.insert(cps, i);
        i
    };
    let mut props: Vec<(u16, u16, u8)> = vec![(0, 0, 0)];
    let mut prop_ix: HashMap<(u16, u16, u8), u16> = HashMap::from([((0, 0, 0), 0)]);
    let mut entry = vec![0u16; CODE_POINTS];
    let mut overflow = false;
    let (mut d_max, mut g_max) = (1, 1);
    for cp in 0..=LAST {
        let (d, g) = (u.listed(cp), u.folded(cp));
        d_max = d_max.max(d.len());
        g_max = g_max.max(g.len());
        let p = (seq(d, cp), seq(g, cp), u.ccc[at(cp)]);
        let i = match prop_ix.get(&p) {
            Some(&i) => i,
            None => {
                let Ok(i) = u16::try_from(props.len()) else {
                    overflow = true;
                    break;
                };
                prop_ix.insert(p, i);
                props.push(p);
                i
            }
        };
        entry[at(cp)] = i;
    }
    errs.require(Check::from(!overflow && pool.len() <= 0xFFFF), || {
        ud.error(
            Kind::Invariant,
            format!(
                "{} properties or {} pool words do not fit a u16",
                props.len(),
                pool.len()
            ),
        )
    });
    let Some(trie) = errs.take(Trie::build("SEARCH", &entry, SHIFT)) else {
        return Err(errs);
    };
    let wrong = (0..=LAST).find(|&cp| {
        let p = props[usize::from(trie.get(at(cp), SHIFT).unwrap_or(0))];
        pooled(&pool, p.0, cp) != u.listed(cp)
            || pooled(&pool, p.1, cp) != u.folded(cp)
            || p.2 != u.ccc[at(cp)]
    });
    if let Some(cp) = wrong {
        errs.push(ud.error(
            Kind::Invariant,
            format!("the trie disagrees with the decompositions at U+{cp:04X}"),
        ));
    }

    let mut out = Emit::new(
        "search_data",
        "baud-domain/build.rs (build/search.rs)",
        &[&ud, &cf],
    );
    out.code(&format!(
        "/// The trie's block size, as a shift: `codepoint >> SEARCH_SHIFT` names a\n\
         /// block in [`SEARCH_INDEX`].\n\
         pub const SEARCH_SHIFT: u32 = {SHIFT};\n\
         \n\
         /// The longest full canonical decomposition a code point has, Hangul\n\
         /// syllables included.\n\
         pub const DECOMP_MAX: usize = {};\n\
         \n\
         /// The longest sequence a code point folds and decomposes to.\n\
         pub const FOLD_MAX: usize = {g_max};",
        d_max.max(3)
    ));
    out.array(
        "SEARCH_INDEX",
        "u8",
        &[&format!(
            "Stage 1: `codepoint >> {SHIFT}` to one of {} distinct blocks.",
            trie.distinct
        )],
        &trie.index,
        PerLine(24),
        u8::to_string,
    );
    out.array(
        "SEARCH_BLOCKS",
        "u16",
        &[
            &format!(
                "Stage 2: {} blocks of {}, each entry an index into",
                trie.distinct,
                1 << SHIFT
            ),
            "[`SEARCH_PROPS`].",
        ],
        &trie.blocks,
        PerLine(16),
        u16::to_string,
    );
    out.array(
        "SEARCH_PROPS",
        "(u16, u16, u8)",
        &[
            "`(d, g, ccc)`: the code point's decomposition and its folding, each an",
            "index into [`SEARCH_POOL`] (0 for the code point itself), and its",
            "canonical combining class.",
        ],
        &props,
        PerLine(6),
        |p| format!("({}, {}, {})", p.0, p.1, p.2),
    );
    out.array(
        "SEARCH_POOL",
        "u32",
        &["The sequences: at an index, a length and then that many code points."],
        &pool,
        PerLine(10),
        |v| format!("0x{v:04X}"),
    );
    errs.or(out)
}
