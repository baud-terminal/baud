//! The script tables, `OUT_DIR/script_data.rs`: the Unicode Script
//! property (`[[source: unicode-scripts]]`), its Script_Extensions
//! (`[[source: unicode-scriptextensions]]`), each script's ISO 15924 code
//! (`[[source: unicode-propertyvaluealiases]]`), the OpenType script tags
//! each script selects (`[[source: ms-opentype-scripttags]]`), each
//! script's direction with the letters and numbers that can turn a run of
//! a right-to-left script left to right (`[[source: unicode-derivedbidiclass]]`,
//! `[[source: gc-data]]`), and the paired brackets UAX #9 pairs
//! (`[[source: unicode-bidibrackets]]`, with `[[source: unicode-data]]`
//! for the canonical equivalents BD16 names).
//!
//! **A bracket pair takes the script of the text enclosing it**
//! (`[[source: uax24]]` 5.1): "paired punctuation, such as brackets or
//! quotation marks, belongs to the enclosing or outer level of the text
//! and should therefore match the script of the enclosing text". Which
//! characters pair is Bidi_Paired_Bracket and its type, and a pair is
//! matched as UAX #9's BD16 matches one (`[[source: uax9-bidi]]`),
//! U+2329/U+3008 and U+232A/U+3009 "treated as equivalent". Each pair gets
//! one number, shared by the two equivalents, so the resolver compares
//! numbers and never searches the file.
//!
//! **A script is right to left when every letter in it is Bidi_Class R or
//! AL.** UAX #9 resolves such a character to an odd level whatever the
//! paragraph's direction (`[[source: uax9-bidi]]`, W3 and I1-I2), so a run
//! of the script is laid out right to left. Over 17.0.0 that is 37
//! scripts: HarfBuzz's `hb_script_get_horizontal_direction` list and Old
//! Hungarian, which HarfBuzz leaves to the caller. A script whose letters
//! disagree stops the build.
//!
//! **The OpenType tag is derived, and the derivation was checked against
//! the registry once, by hand, not here:** the registry is a vendor page
//! held `external` (not committed), so the build cannot read it. Every
//! Unicode 17.0 script's tag below is a registered tag, and the registered
//! tags it does not produce are `byzm`, `math` and `musc` (not Unicode
//! scripts), `jamo` (the registry: "Use of this tag is not recommended ...
//! should use 'hang' instead"), and `jurc`, `pcun` and `seal` (scripts
//! Unicode 17.0 does not encode). Every derived tag was checked against the
//! registry's 187 tags.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use baud_tablegen::{CODE_POINTS, Check, Emit, Error, Errors, Input, Kind, Pos, Source, Trie};
use std::collections::{BTreeMap, HashMap};

pub const ALIASES: Input = Input {
    file: "unicode-propertyvaluealiases.txt",
    key: "unicode-propertyvaluealiases",
    header: "# PropertyValueAliases-17.0.0.txt",
};

pub const SCRIPTS: Input = Input {
    file: "unicode-scripts.txt",
    key: "unicode-scripts",
    header: "# Scripts-17.0.0.txt",
};

pub const EXTENSIONS: Input = Input {
    file: "unicode-scriptextensions.txt",
    key: "unicode-scriptextensions",
    header: "# ScriptExtensions-17.0.0.txt",
};

pub const GENERAL_CATEGORY: Input = Input {
    file: "unicode-derivedgeneralcategory.txt",
    key: "gc-data",
    header: "# DerivedGeneralCategory-17.0.0.txt",
};

pub const BIDI_CLASS: Input = Input {
    file: "unicode-derivedbidiclass.txt",
    key: "unicode-derivedbidiclass",
    header: "# DerivedBidiClass-17.0.0.txt",
};

pub const BRACKETS: Input = Input {
    file: "unicode-bidibrackets.txt",
    key: "unicode-bidibrackets",
    header: "# BidiBrackets-17.0.0.txt",
};

pub const UNICODE_DATA: Input = Input {
    file: "unicode-unicodedata.txt",
    key: "unicode-data",
    header: "0000;<control>;Cc;0;BN;;;;;N;NULL;;;;",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 7] = [
    ALIASES,
    SCRIPTS,
    EXTENSIONS,
    GENERAL_CATEGORY,
    BIDI_CLASS,
    BRACKETS,
    UNICODE_DATA,
];

/// The registry's exceptions to "the ISO 15924 code, lowercased":
/// Hiragana, Katakana and their union share `kana`, and the three-letter
/// tags pad with spaces (the registry spells Yi's with no-break spaces "to
/// ensure that two space characters are displayed. The actual tag byte
/// sequence is 0x79 0x69 0x20 0x20").
pub const EXCEPTIONS: [(&str, &str); 7] = [
    ("Hira", "kana"),
    ("Kana", "kana"),
    ("Hrkt", "kana"),
    ("Laoo", "lao "),
    ("Yiii", "yi  "),
    ("Nkoo", "nko "),
    ("Vaii", "vai "),
];

/// Scripts the registry gives a second tag for its newer layout model,
/// tried first: "Use of this tag ['mymr'] is not recommended. Fonts
/// supporting Unicode encoding of Myanmar script should use 'mym2'
/// instead", and the Indic `v.2` rows, which the registry lists in no
/// order, so putting them first is a choice recorded open, not the
/// registry's. HarfBuzz also tries an unregistered `dev3` and its kin;
/// baud tries only registered tags.
pub const V2: [(&str, &str); 10] = [
    ("Beng", "bng2"),
    ("Deva", "dev2"),
    ("Gujr", "gjr2"),
    ("Guru", "gur2"),
    ("Knda", "knd2"),
    ("Mlym", "mlm2"),
    ("Orya", "ory2"),
    ("Taml", "tml2"),
    ("Telu", "tel2"),
    ("Mymr", "mym2"),
];

/// Script values with no OpenType tag of their own: text in them selects
/// `DFLT`. Their order fixes indices 0, 1 and 2, which `Script` names.
pub const NEUTRAL: [&str; 3] = ["Zzzz", "Zyyy", "Zinh"];

/// The trie's block: 256 code points.
const SHIFT: u32 = 8;

/// What a code point is to a right-to-left run's direction.
const LETTER: u8 = 1;
const NUMBER: u8 = 2;

/// The counts the tables were compiled with.
const COUNTS: [(&str, usize); 6] = [
    ("Script values", 175),
    ("Script_Extensions sets", 119),
    ("right-to-left scripts", 37),
    ("paired brackets", 128),
    ("opening brackets", 64),
    ("bracket pairs", 63),
];

fn at(cp: u32) -> usize {
    usize::try_from(cp).expect("a code point indexes")
}

/// A failure at the start of line `line` of `src`.
fn on_line(src: &Source, line: usize, kind: Kind, message: String) -> Error {
    src.error(kind, message)
        .at(Pos { line, col: 1 }, src.line(line))
}

fn find<'a>(map: &[(&str, &'a str)], key: &str) -> Option<&'a str> {
    map.iter().find(|(k, _)| *k == key).map(|&(_, v)| v)
}

/// A property's value at each code point a file lists, `None` where it
/// lists none; a code point two rows list is refused.
fn classes<'a>(src: &'a Source, errs: &mut Errors) -> Vec<Option<&'a str>> {
    let mut out: Vec<Option<&'a str>> = vec![None; CODE_POINTS];
    let mut line = vec![0usize; CODE_POINTS];
    for row in src.rows() {
        let Some(r) = errs.take(row.expect(2).and_then(|()| row.range(0))) else {
            continue;
        };
        if let Some(cp) = r.iter().find(|&cp| line[at(cp)] != 0) {
            errs.push(row.error(
                if r.is_one() {
                    Kind::Duplicate
                } else {
                    Kind::Overlap
                },
                0,
                format!("U+{cp:04X} is already given on line {}", line[at(cp)]),
            ));
            continue;
        }
        for cp in r.iter() {
            out[at(cp)] = Some(row.field(1));
            line[at(cp)] = row.line();
        }
    }
    out
}

/// The tables, before they are written.
#[derive(Debug, Default)]
pub struct Tables {
    /// ISO 15924 codes in index order.
    pub codes: Vec<String>,
    /// Each code point's script index.
    pub script: Vec<u8>,
    /// The distinct Script_Extensions sets, set 0 the empty one.
    pub sets: Vec<Vec<u8>>,
    /// Each code point's set index.
    pub extension: Vec<u8>,
    /// Each script's two tags, empty for none.
    pub tags: Vec<(String, String)>,
    /// Each script's direction, 1 right to left.
    pub directions: Vec<u8>,
    pub run_kind: Vec<u8>,
    pub bracket: Vec<u8>,
}

/// The Script property and Script_Extensions, from PropertyValueAliases,
/// Scripts.txt and ScriptExtensions.txt.
fn scripts(aliases: &Source, sc_src: &Source, scx_src: &Source, t: &mut Tables, errs: &mut Errors) {
    // Long name -> ISO 15924 code, and every alias -> the code.
    let mut code_of: HashMap<&str, &str> = HashMap::new();
    let mut short_to_code: HashMap<&str, &str> = HashMap::new();
    for row in aliases.rows().filter(|r| r.field(0) == "sc") {
        if row.len() < 3 {
            errs.push(row.error(
                Kind::FieldCount,
                row.len(),
                format!("{} fields, at least 3 expected", row.len()),
            ));
            continue;
        }
        code_of.insert(row.field(2), row.field(1));
        for i in 1..row.len() {
            short_to_code.insert(row.field(i), row.field(1));
        }
    }
    for (long, code) in [("Hebrew", "Hebr"), ("Common", "Zyyy")] {
        let got = code_of.get(long).copied();
        errs.require(Check::from(got == Some(code)), || {
            aliases.error(
                Kind::Control,
                format!("sc `{long}` is {got:?}, {code:?} expected"),
            )
        });
    }
    let mut named: BTreeMap<&str, Vec<(baud_tablegen::CpRange, usize)>> = BTreeMap::new();
    for row in sc_src.rows() {
        let Some(r) = errs.take(row.expect(2).and_then(|()| row.range(0))) else {
            continue;
        };
        match code_of.get(row.field(1)) {
            Some(code) => named.entry(*code).or_default().push((r, row.line())),
            None => errs.push(row.error(
                Kind::UnknownValue,
                1,
                format!(
                    "`{}` is no sc long name in PropertyValueAliases.txt",
                    row.field(1)
                ),
            )),
        }
    }
    let mut codes: Vec<&str> = NEUTRAL.to_vec();
    codes.extend(named.keys().copied().filter(|c| !NEUTRAL.contains(c)));
    if codes.len() > 255 {
        errs.push(sc_src.error(
            Kind::Invariant,
            format!("{} scripts: a script index is a u8", codes.len()),
        ));
        return;
    }
    let index: HashMap<&str, u8> = codes
        .iter()
        .enumerate()
        .map(|(i, c)| (*c, u8::try_from(i).expect("checked")))
        .collect();
    // Unassigned code points are Unknown (Zzzz), index 0.
    let mut sc = vec![0u8; CODE_POINTS];
    let mut set_by = vec![0usize; CODE_POINTS];
    for (code, ranges) in &named {
        for &(r, line) in ranges {
            if let Some(cp) = r.iter().find(|&cp| set_by[at(cp)] != 0) {
                errs.push(on_line(
                    sc_src,
                    line,
                    Kind::Overlap,
                    format!("U+{cp:04X} is already given on line {}", set_by[at(cp)]),
                ));
                continue;
            }
            for cp in r.iter() {
                sc[at(cp)] = index[code];
                set_by[at(cp)] = line;
            }
        }
    }
    // Script_Extensions: a set per listed code point, by short alias. The
    // file lists only code points whose value is not {Script}; everything
    // else has {Script} and is stored as 0, "none listed".
    let mut sets: Vec<Vec<u8>> = vec![Vec::new()];
    let mut set_index: HashMap<Vec<u8>, u8> = HashMap::from([(Vec::new(), 0)]);
    let mut scx = vec![0u8; CODE_POINTS];
    let mut scx_line = vec![0usize; CODE_POINTS];
    for row in scx_src.rows() {
        let Some(r) = errs.take(row.expect(2).and_then(|()| row.range(0))) else {
            continue;
        };
        let mut members = Vec::new();
        for alias in row.field(1).split_whitespace() {
            match short_to_code.get(alias).and_then(|c| index.get(c)) {
                Some(&i) => members.push(i),
                None => errs.push(row.error(
                    Kind::UnknownValue,
                    1,
                    format!("`{alias}` is no script Scripts.txt gives a character"),
                )),
            }
        }
        members.sort_unstable();
        let n = match set_index.get(&members) {
            Some(&n) => n,
            None => {
                let Ok(n) = u8::try_from(sets.len()) else {
                    errs.push(row.error(
                        Kind::Invariant,
                        1,
                        "more than 256 Script_Extensions sets: a set index is a u8",
                    ));
                    continue;
                };
                set_index.insert(members.clone(), n);
                sets.push(members);
                n
            }
        };
        if let Some(cp) = r.iter().find(|&cp| scx_line[at(cp)] != 0) {
            errs.push(row.error(
                Kind::Overlap,
                0,
                format!("U+{cp:04X} is already given on line {}", scx_line[at(cp)]),
            ));
            continue;
        }
        for cp in r.iter() {
            scx[at(cp)] = n;
            scx_line[at(cp)] = row.line();
        }
    }
    // UAX #24: Script_Extensions(c) != {Script(c)} only where Script(c) is
    // Common or Inherited. The resolver relies on it.
    for cp in 0..CODE_POINTS {
        let s = &sets[usize::from(scx[cp])];
        if scx[cp] != 0 && !matches!(sc[cp], 1 | 2) && s[..] == [sc[cp]] {
            errs.push(on_line(
                scx_src,
                scx_line[cp],
                Kind::Invariant,
                format!(
                    "U+{cp:04X}: Script_Extensions is {{Script}}, which the file lists only \
                     for Common or Inherited"
                ),
            ));
        }
    }
    let tag = |c: &str| find(&EXCEPTIONS, c).map_or_else(|| c.to_lowercase(), str::to_string);
    t.tags = codes
        .iter()
        .map(|&c| {
            if NEUTRAL.contains(&c) {
                (String::new(), String::new())
            } else if let Some(v2) = find(&V2, c) {
                (v2.to_string(), tag(c))
            } else {
                (tag(c), String::new())
            }
        })
        .collect();
    for (code, want) in [
        ("Hebr", ("hebr", "")),
        ("Deva", ("dev2", "deva")),
        ("Hira", ("kana", "")),
    ] {
        let got = index.get(code).map(|&i| &t.tags[usize::from(i)]);
        errs.require(
            Check::from(got.is_some_and(|(a, b)| (a.as_str(), b.as_str()) == want)),
            || {
                aliases.error(
                    Kind::Control,
                    format!("{code}'s tags are {got:?}, {want:?} expected"),
                )
            },
        );
    }
    for ((c, (a, b)), i) in codes.iter().zip(&t.tags).zip(0..) {
        for tg in [a, b] {
            errs.require(
                Check::from(tg.is_empty() || (tg.len() == 4 && tg.is_ascii())),
                || {
                    sc_src.error(
                        Kind::Invariant,
                        format!("script {i}, {c}: tag {tg:?} is not four bytes"),
                    )
                },
            );
        }
        errs.require(Check::from(c.len() == 4 && c.is_ascii()), || {
            sc_src.error(
                Kind::Invariant,
                format!("script {i}: code {c:?} is not four bytes"),
            )
        });
    }
    t.codes = codes.iter().map(|c| c.to_string()).collect();
    t.script = sc;
    t.sets = sets;
    t.extension = scx;
}

/// Each script's direction and what each code point is to a
/// right-to-left run, from General_Category and Bidi_Class.
fn directions(
    gc_src: &Source,
    bidi_src: &Source,
    gc: &[Option<&str>],
    bidi: &[Option<&str>],
    t: &mut Tables,
    errs: &mut Errors,
) {
    // Right to left when every letter (gc L*) of the script is Bidi_Class
    // R or AL; Common's letters are mixed and it stays left to right.
    let n = t.codes.len();
    let mut kinds = vec![(false, false); n];
    for cp in 0..CODE_POINTS {
        if !gc[cp].is_some_and(|g| g.starts_with('L')) {
            continue;
        }
        let Some(b) = bidi[cp] else {
            errs.push(bidi_src.error(
                Kind::Invariant,
                format!("U+{cp:04X} is a letter the file gives no Bidi_Class"),
            ));
            continue;
        };
        let k = &mut kinds[usize::from(t.script[cp])];
        if matches!(b, "R" | "AL") {
            k.1 = true;
        } else {
            k.0 = true;
        }
    }
    let mut rtl = vec![false; n];
    for (i, &(ltr, r)) in kinds.iter().enumerate().skip(NEUTRAL.len()) {
        if ltr && r {
            errs.push(gc_src.error(
                Kind::Invariant,
                format!("{}: letters of both directions", t.codes[i]),
            ));
        }
        rtl[i] = r && !ltr;
    }
    let idx = |c: &str| t.codes.iter().position(|x| x == c);
    for (c, want) in [("Hebr", true), ("Arab", true), ("Latn", false)] {
        let got = idx(c).map(|i| rtl[i]);
        errs.require(Check::from(got == Some(want)), || {
            bidi_src.error(
                Kind::Control,
                format!("{c} is right to left: {got:?}, {want} expected"),
            )
        });
    }
    t.directions = rtl.iter().map(|&r| u8::from(r)).collect();
    // What can turn a run of a right-to-left script left to right: a
    // decimal digit or a regional indicator with no letter beside it, the
    // rule HarfBuzz's `hb_ensure_native_direction` applies when its caller
    // asks for left to right. Only a code point a right-to-left run can
    // hold is classed -- its Script or one of its Script_Extensions is
    // right to left, or it is Common or Inherited -- which keeps the table
    // to the blocks those scripts touch.
    let mut run = vec![0u8; CODE_POINTS];
    for cp in 0..CODE_POINTS {
        let s = usize::from(t.script[cp]);
        let held = rtl[s]
            || matches!(s, 1 | 2)
            || t.sets[usize::from(t.extension[cp])]
                .iter()
                .any(|&m| rtl[usize::from(m)]);
        let Some(g) = gc[cp].filter(|_| held) else {
            continue;
        };
        if g.starts_with('L') {
            run[cp] = LETTER;
        } else if g == "Nd" || (0x1F1E6..=0x1F1FF).contains(&cp) {
            run[cp] = NUMBER;
        }
    }
    for (cp, want) in [
        (0x05D0, LETTER),
        (0x0661, NUMBER),
        (0x0031, NUMBER),
        (0x1F1E6, NUMBER),
        (0x02B9, LETTER),
        (0x0041, 0),
        (0x0915, 0),
    ] {
        let got = run[at(cp)];
        errs.require(Check::from(got == want), || {
            gc_src.error(
                Kind::Control,
                format!("U+{cp:04X} is {got} to a right-to-left run, {want} expected"),
            )
        });
    }
    t.run_kind = run;
}

/// The paired brackets, from BidiBrackets.txt, each checked against the
/// files pinned beside it, and BD16's canonical equivalents from
/// UnicodeData.txt.
fn brackets(
    br_src: &Source,
    ud: &Source,
    gc: &[Option<&str>],
    bidi: &[Option<&str>],
    t: &mut Tables,
    errs: &mut Errors,
) -> [usize; 3] {
    // BidiBrackets.txt's header: "two characters, A and B, form a bracket
    // pair if A has gc=Ps and B has gc=Pe, both have bc=ON and Bidi_M=Y,
    // and bmg of A is B".
    let mut pair: BTreeMap<u32, (u32, bool, usize)> = BTreeMap::new();
    for row in br_src.rows() {
        let parsed = row
            .expect(3)
            .and_then(|()| Ok((row.code_point(0)?, row.code_point(1)?)));
        let Some((cp, other)) = errs.take(parsed) else {
            continue;
        };
        let open = match row.field(2) {
            "o" => true,
            "c" => false,
            "n" => continue,
            v => {
                errs.push(row.error(
                    Kind::UnknownValue,
                    2,
                    format!("Bidi_Paired_Bracket_Type `{v}`, one of o, c and n expected"),
                ));
                continue;
            }
        };
        if pair.insert(cp, (other, open, row.line())).is_some() {
            errs.push(row.error(Kind::Duplicate, 0, format!("U+{cp:04X} is listed twice")));
        }
    }
    let opening = pair.values().filter(|v| v.1).count();
    for (&cp, &(other, open, line)) in &pair {
        let fail =
            |what: &str| on_line(br_src, line, Kind::Invariant, format!("U+{cp:04X} {what}"));
        if !pair
            .get(&other)
            .is_some_and(|&(back, o, _)| back == cp && o != open)
        {
            errs.push(fail("and its pair are not each other's, one opening"));
        }
        let want = if open { "Ps" } else { "Pe" };
        if gc[at(cp)] != Some(want) {
            errs.push(fail(&format!("is gc {:?}, {want} expected", gc[at(cp)])));
        }
        if bidi[at(cp)] != Some("ON") {
            errs.push(fail(&format!("is bc {:?}, ON expected", bidi[at(cp)])));
        }
        // A pair is in one Script value: Common, or both in one script
        // (Tibetan's and Ogham's), which needs no resolving.
        let (a, b) = (at(cp), at(other));
        if t.script[a] != t.script[b]
            || t.sets[usize::from(t.extension[a])] != t.sets[usize::from(t.extension[b])]
        {
            errs.push(fail("and its pair are not in one script"));
        }
    }
    // BD16's equivalents: a bracket whose canonical decomposition is one
    // code point is that bracket.
    let mut canonical: BTreeMap<u32, u32> = BTreeMap::new();
    for e in ud.unicode_data().into_iter().filter_map(Result::ok) {
        let cp = e.range.lo;
        let d = e.row.field(5);
        if pair.contains_key(&cp) && !d.is_empty() && !d.starts_with('<') && !d.contains(' ') {
            if let Some(to) = errs.take(e.row.code_point(5)) {
                canonical.insert(cp, to);
            }
        }
    }
    let want = BTreeMap::from([(0x2329, 0x3008), (0x232A, 0x3009)]);
    errs.require(Check::from(canonical == want), || {
        ud.error(
            Kind::Control,
            format!("the brackets' canonical equivalents are {canonical:X?}, {want:X?} expected"),
        )
    });
    let mut pair_of: BTreeMap<u32, u32> = BTreeMap::new();
    for (&cp, _) in pair
        .iter()
        .filter(|(cp, v)| v.1 && !canonical.contains_key(*cp))
    {
        let n = u32::try_from(pair_of.len() + 1).expect("a few pairs");
        pair_of.insert(cp, n);
    }
    for (&cp, &to) in &canonical {
        if pair.get(&cp).is_some_and(|v| v.1) {
            match pair_of.get(&to) {
                Some(&n) => {
                    pair_of.insert(cp, n);
                }
                None => errs.push(ud.error(
                    Kind::Invariant,
                    format!("U+{cp:04X} is equivalent to U+{to:04X}, which opens no pair"),
                )),
            }
        }
    }
    let distinct: std::collections::BTreeSet<u32> = pair_of.values().copied().collect();
    let mut bracket = vec![0u8; CODE_POINTS];
    for (&cp, &n) in &pair_of {
        let (Ok(open), Ok(close)) = (u8::try_from(n * 2), u8::try_from(n * 2 + 1)) else {
            errs.push(br_src.error(
                Kind::Invariant,
                format!("pair {n}: a bracket's number times two, plus one, is a u8"),
            ));
            continue;
        };
        bracket[at(cp)] = open;
        if let Some(&(other, _, _)) = pair.get(&cp) {
            bracket[at(other)] = close;
        }
    }
    for (cp, want) in [(0x28, 2), (0x29, 3)] {
        errs.require(Check::from(bracket[cp] == want), || {
            br_src.error(
                Kind::Control,
                format!("U+{cp:04X}'s bracket is {}, {want} expected", bracket[cp]),
            )
        });
    }
    for (a, b) in [(0x2329, 0x3008), (0x232A, 0x3009)] {
        errs.require(Check::from(bracket[a] == bracket[b]), || {
            br_src.error(
                Kind::Control,
                format!("U+{a:04X} and U+{b:04X} are not one bracket"),
            )
        });
    }
    t.bracket = bracket;
    [pair.len(), opening, distinct.len()]
}

pub fn tables(
    read: &dyn Fn(&str) -> std::io::Result<String>,
) -> Result<(Tables, Vec<Source>), Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    if loaded.len() != INPUTS.len() {
        return Err(errs);
    }
    let mut t = Tables::default();
    {
        let [aliases, sc, scx, gc_src, bidi_src, br, ud] = &loaded[..] else {
            unreachable!("seven inputs");
        };
        scripts(aliases, sc, scx, &mut t, &mut errs);
        if !errs.is_empty() {
            return Err(errs);
        }
        let gc = classes(gc_src, &mut errs);
        let bidi = classes(bidi_src, &mut errs);
        directions(gc_src, bidi_src, &gc, &bidi, &mut t, &mut errs);
        let [pairs, opening, numbered] = brackets(br, ud, &gc, &bidi, &mut t, &mut errs);
        let rtl = t.directions.iter().filter(|&&d| d == 1).count();
        let got = [t.codes.len(), t.sets.len(), rtl, pairs, opening, numbered];
        for ((what, want), got) in COUNTS.into_iter().zip(got) {
            errs.count(sc.path(), what, got, want);
        }
    }
    errs.or((t, loaded))
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let (t, loaded) = tables(read)?;
    let trie = |name: &str, values: &[u8]| Trie::build(name, values, SHIFT).map_err(Errors::from);
    let sc = trie("SCRIPT_INDEX", &t.script)?;
    let scx = trie("EXTENSION_INDEX", &t.extension)?;
    let run = trie("RUN_INDEX", &t.run_kind)?;
    let br = trie("BRACKET_INDEX", &t.bracket)?;
    let widest = t.sets.iter().map(Vec::len).max().unwrap_or(0);
    let slot = widest + 1;
    let codes: Vec<u8> = t.codes.iter().flat_map(|c| c.bytes()).collect();
    let tags: Vec<u8> = t
        .tags
        .iter()
        .flat_map(|(a, b)| {
            let pad = |s: &String| {
                if s.is_empty() {
                    vec![0; 4]
                } else {
                    s.as_bytes().to_vec()
                }
            };
            [pad(a), pad(b)].concat()
        })
        .collect();
    let sets: Vec<u8> = t
        .sets
        .iter()
        .flat_map(|m| {
            let mut s = m.clone();
            s.resize(slot, 0xff);
            s
        })
        .collect();
    let sources: Vec<&Source> = loaded.iter().collect();
    let mut out = Emit::new(
        "script_data",
        "crates/baud-font/build.rs (build/script.rs)",
        &sources,
    );
    out.code(&format!(
        "/// How many Script values there are; a `Script` indexes below this.\n\
         pub const SCRIPT_COUNT: usize = {};\n\n\
         /// The bytes of one Script_Extensions set in [`EXTENSION_SETS`].\n\
         pub const EXTENSION_SLOT: usize = {slot};",
        t.codes.len()
    ));
    out.bytes(
        "CODES",
        &[
            "Each script's ISO 15924 code, four bytes a script in index order:",
            "`Zzzz` (Unknown), `Zyyy` (Common) and `Zinh` (Inherited) first, then",
            "by code.",
        ],
        &codes,
    );
    out.bytes(
        "TAGS",
        &[
            "Each script's two OpenType tags, eight bytes a script, the one to try",
            "first first; a zero tag is none. Unknown, Common and Inherited have",
            "none.",
        ],
        &tags,
    );
    out.bytes(
        "EXTENSION_SETS",
        &[
            "The distinct Script_Extensions sets as script indices, one",
            "[`EXTENSION_SLOT`]-byte slot each, ended by `0xff`; set 0 is the",
            "empty set, \"the Script value alone\".",
        ],
        &sets,
    );
    let stage1 = |n: usize| format!("Stage 1: code point >> 8 to one of {n} blocks.");
    out.bytes(
        "SCRIPT_INDEX",
        &[
            "The Script property as a two-stage trie of script indices.",
            &stage1(sc.distinct),
        ],
        &sc.index,
    );
    out.bytes(
        "SCRIPT_BLOCKS",
        &["Stage 2 of [`SCRIPT_INDEX`]."],
        &sc.blocks,
    );
    out.bytes(
        "EXTENSION_INDEX",
        &[
            "Script_Extensions as a two-stage trie of set indices into",
            "[`EXTENSION_SETS`].",
            &stage1(scx.distinct),
        ],
        &scx.index,
    );
    out.bytes(
        "EXTENSION_BLOCKS",
        &["Stage 2 of [`EXTENSION_INDEX`]."],
        &scx.blocks,
    );
    out.bytes(
        "DIRECTIONS",
        &[
            "Each script's direction, a byte a script in index order: 1 where",
            "every letter of the script is Bidi_Class R or AL, else 0.",
        ],
        &t.directions,
    );
    out.bytes(
        "RUN_INDEX",
        &[
            "What a code point is to a right-to-left run's direction, as a",
            "two-stage trie: 1 a letter, 2 a decimal digit or regional indicator,",
            "0 neither or out of reach of such a run.",
            &stage1(run.distinct),
        ],
        &run.index,
    );
    out.bytes("RUN_BLOCKS", &["Stage 2 of [`RUN_INDEX`]."], &run.blocks);
    out.bytes(
        "BRACKET_INDEX",
        &[
            "Each code point's paired bracket, as a two-stage trie: 0 none, else",
            "its pair's number times two, plus one for a closer; U+2329 and",
            "U+3008 share a number, as U+232A and U+3009 do.",
            &stage1(br.distinct),
        ],
        &br.index,
    );
    out.bytes(
        "BRACKET_BLOCKS",
        &["Stage 2 of [`BRACKET_INDEX`]."],
        &br.blocks,
    );
    Ok(out)
}
