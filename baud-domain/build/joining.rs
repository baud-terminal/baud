//! The cursive-joining tables, `OUT_DIR/joining_data.rs`: one byte a code
//! point as a two-stage trie indexed by `codepoint >> 8` and then the low
//! byte, Joining_Type in the low three bits (`JoiningType`'s declaration
//! order: U, T, C, D, R, L) and, in bits 3-5, the four Joining_Group values
//! the Unicode Standard's rules name: LAM (1) and ALEF (2), which its
//! obligatory ligature rules name (chapter 9.2.3), and ALAPH (3) and
//! DALATH RISH (4), which its rules for Syriac alaph name (chapter 9.3,
//! S1-S3; `[[source: unicode-ch9]]`). Both come from ArabicShaping.txt
//! (`[[source: unicode-arabicshaping]]`); a code point it does not list is
//! T when its General Category is Mn, Me or Cf and U otherwise, as its
//! header says (`[[source: gc-data]]`). The result is held to
//! DerivedJoiningType.txt (`[[source: unicode-derivedjoiningtype]]`) at
//! every code point.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use crate::ucd::{self, GC, at, index};
use baud_tablegen::{
    CODE_POINTS, Check, CodepointMap, Emit, Errors, Input, Kind, LAST, Source, Trie, ValueSet,
};

pub const ARABIC_SHAPING: Input = Input {
    file: "unicode-arabicshaping.txt",
    key: "unicode-arabicshaping",
    header: "# ArabicShaping-17.0.0.txt",
};

pub const DERIVED_JOINING_TYPE: Input = Input {
    file: "unicode-derivedjoiningtype.txt",
    key: "unicode-derivedjoiningtype",
    header: "# DerivedJoiningType-17.0.0.txt",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 4] = [
    ucd::ALIASES,
    ucd::GENERAL_CATEGORY,
    ARABIC_SHAPING,
    DERIVED_JOINING_TYPE,
];

/// The `jt` short names, in `JoiningType`'s declaration order, which the
/// low three bits store.
pub const TYPES: [&str; 6] = ["U", "T", "C", "D", "R", "L"];

/// `JoiningType`'s variants, in the same order.
const VARIANTS: [&str; 6] = [
    "NonJoining",
    "Transparent",
    "JoinCausing",
    "DualJoining",
    "RightJoining",
    "LeftJoining",
];

/// The joining groups the rules name, as ArabicShaping.txt spells them,
/// and the number bits 3-5 store for each.
pub const GROUPS: [(&str, u8); 4] = [("LAM", 1), ("ALEF", 2), ("ALAPH", 3), ("DALATH RISH", 4)];

/// ArabicShaping.txt's rows, one a code point.
const LISTED: usize = 834;

/// A property value under UAX44-LM3 (`[[source: uax44]]` section 5.9.3):
/// case, spaces, underscores and hyphens ignored. ArabicShaping.txt's
/// header gives the same rule for its field 3: the formal value "consists
/// of the same name parts joined by underscores".
fn loose(v: &str) -> String {
    v.chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The `jg` values of PropertyValueAliases.txt, every alias of each, under
/// [`loose`].
fn joining_groups(aliases: &Source) -> Vec<String> {
    aliases
        .rows()
        .filter(|r| r.field(0) == "jg")
        .flat_map(|r| (1..r.len()).map(move |i| loose(r.field(i))))
        .collect()
}

/// Each code point's Joining_Type, as its place in [`TYPES`], and its
/// joining group's number, 0 for none of [`GROUPS`]: the header's default
/// by General Category, then ArabicShaping.txt's rows.
pub fn joining(
    aliases: &Source,
    jt: &ValueSet,
    dgc: &Source,
    shaping: &Source,
    errs: &mut Errors,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let gc = ucd::values(aliases, "gc", &GC, errs)?;
    let cat = ucd::general_category(dgc, &gc, errs)?;
    let groups = joining_groups(aliases);
    errs.require(Check::from(!groups.is_empty()), || {
        aliases.error(Kind::Count, "no `jg` rows")
    });
    let marks = [index(&GC, "Mn"), index(&GC, "Me"), index(&GC, "Cf")];
    let (u, t) = (index(&TYPES, "U"), index(&TYPES, "T"));
    let mut types: Vec<u8> = cat
        .iter()
        .map(|c| if marks.contains(c) { t } else { u })
        .collect();
    let mut group = vec![0u8; CODE_POINTS];
    let mut listed = CodepointMap::<u8>::new();
    let mut rows = 0;
    for row in shaping.rows() {
        rows += 1;
        let cp = errs.take(row.expect(4).and_then(|()| row.code_point(0)));
        let v = errs.take(jt.check(&row, 2));
        let (Some(cp), Some(v)) = (cp, v) else {
            continue;
        };
        let g = row.field(3);
        if !groups.contains(&loose(g)) {
            errs.push(row.error(
                Kind::UnknownValue,
                3,
                format!(
                    "jg `{g}` is not one of the `jg` values of {}",
                    aliases.path()
                ),
            ));
            continue;
        }
        let r = baud_tablegen::CpRange { lo: cp, hi: cp };
        if errs.take(listed.assign(shaping, &row, r, 1)).is_none() {
            continue;
        }
        types[at(cp)] = u8::try_from(v).expect("six types");
        group[at(cp)] = GROUPS.iter().find(|(n, _)| *n == g).map_or(0, |&(_, n)| n);
    }
    errs.count(shaping.path(), "rows", rows, LISTED);
    Some((types, group))
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    let Ok([aliases, dgc, shaping, derived]) = <[Source; 4]>::try_from(loaded) else {
        return Err(errs);
    };
    let Some(jt) = ucd::values(&aliases, "jt", &TYPES, &mut errs) else {
        return Err(errs);
    };
    let Some((types, group)) = joining(&aliases, &jt, &dgc, &shaping, &mut errs) else {
        return Err(errs);
    };

    // Positive controls: beh dual, alef right, hamza and ZWNJ non-joining,
    // tatweel and ZWJ join-causing, fatha transparent (Mn, unlisted), ALM
    // transparent (Cf, unlisted), the isolates non-joining as UAX #9 3.5
    // says, a Latin letter non-joining; lam in LAM and alef with madda in
    // ALEF, beh in neither; alaph alone in ALAPH and the four letters of
    // DALATH RISH.
    let mut controls: Vec<(u32, &str)> = vec![
        (0x0628, "D"),
        (0x0627, "R"),
        (0x0621, "U"),
        (0x200C, "U"),
        (0x0640, "C"),
        (0x200D, "C"),
        (0x064E, "T"),
        (0x061C, "T"),
        (0x0041, "U"),
    ];
    controls.extend((0x2066..=0x2069).map(|cp| (cp, "U")));
    for (cp, want) in controls {
        let got = TYPES[usize::from(types[at(cp)])];
        errs.require(Check::from(got == want), || {
            shaping.error(
                Kind::Control,
                format!("U+{cp:04X} is jt {got}, {want} expected"),
            )
        });
    }
    for (cp, want) in [(0x0644, 1), (0x0622, 2), (0x0628, 0)] {
        let got = group[at(cp)];
        errs.require(Check::from(got == want), || {
            shaping.error(
                Kind::Control,
                format!("U+{cp:04X} is in group {got}, {want} expected"),
            )
        });
    }
    for (g, want) in [(3, &[0x0710][..]), (4, &[0x0715, 0x0716, 0x072A, 0x072F])] {
        let got: Vec<u32> = (0..=LAST).filter(|&cp| group[at(cp)] == g).collect();
        errs.require(Check::from(got == want), || {
            shaping.error(
                Kind::Control,
                format!("group {g} is {got:X?}, {want:X?} expected"),
            )
        });
    }

    // Held to the derived file, its one `@missing` line included.
    let mut held = CodepointMap::<u8>::new();
    let mut defaults = 0;
    for row in derived.missing() {
        defaults += 1;
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let v = errs.take(jt.check_long(&row, 1));
        if let (Some(r), Some(v)) = (r, v) {
            errs.require(
                Check::from(r.lo == 0 && r.hi == LAST && TYPES[v] == "U"),
                || {
                    row.error(
                        Kind::Control,
                        0,
                        "the default is not 0000..10FFFF Non_Joining",
                    )
                },
            );
            held.default_over(r, u8::try_from(v).expect("six types"));
        }
    }
    errs.count(derived.path(), "@missing lines", defaults, 1);
    for row in derived.rows() {
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let v = jt.check(&row, 1).or_else(|_| jt.check_long(&row, 1));
        let v = errs.take(v);
        if let (Some(r), Some(v)) = (r, v) {
            errs.take(held.assign(&derived, &row, r, u8::try_from(v).expect("six types")));
        }
    }
    let Some(held) = errs.take(held.complete(derived.path())) else {
        return Err(errs);
    };
    if let Some(cp) = (0..CODE_POINTS).find(|&cp| held[cp] != types[cp]) {
        errs.push(derived.error(
            Kind::Control,
            format!(
                "U+{cp:04X} is jt {} in this file and {} from ArabicShaping.txt",
                TYPES[usize::from(held[cp])],
                TYPES[usize::from(types[cp])]
            ),
        ));
    }

    let values: Vec<u8> = types
        .iter()
        .zip(&group)
        .map(|(&t, &g)| t | (g << 3))
        .collect();
    let Some(trie) = errs.take(Trie::build("JOINING", &values, 8)) else {
        return Err(errs);
    };
    let mut out = Emit::new(
        "joining_data",
        "baud-domain/build.rs (build/joining.rs)",
        &[&shaping, &dgc, &derived, &aliases],
    );
    out.bytes(
        "JOINING_INDEX",
        &[
            "Joining_Type and the LAM, ALEF, ALAPH and DALATH RISH joining groups,",
            "as a two-stage trie.",
            &format!(
                "Stage 1: codepoint >> 8 to one of {} distinct blocks.",
                trie.distinct
            ),
            "",
            "**Low three bits** are `JoiningType` discriminants in declaration",
            "order (`[[source: unicode-arabicshaping]]`, its header's T and U",
            "defaults by General Category); bits 3-5 are 1 for the LAM joining",
            "group and 2 for ALEF, the two chapter 9.2.3's ligature rules name,",
            "and 3 for ALAPH and 4 for DALATH RISH, the two 9.3's alaph rules",
            "name.",
        ],
        &trie.index,
    );
    out.bytes(
        "JOINING_BLOCKS",
        &["Stage 2: one byte a code point."],
        &trie.blocks,
    );
    out.enum_order("crate::joining::JoiningType", &VARIANTS);
    errs.or(out)
}
