//! What the grapheme, width and line-break tables read alike: the value
//! sets of PropertyValueAliases.txt (`[[source: unicode-propertyvaluealiases]]`),
//! General_Category from DerivedGeneralCategory.txt (`[[source: gc-data]]`),
//! East_Asian_Width with the defaults its header gives
//! (`[[source: eaw-data]]`, `[[source: eaw-data-8]]`), and a binary
//! property of a file that lists several, such as emoji-data.txt
//! (`[[source: emoji-data]]`). Each function computes and returns; none
//! writes anything.

use baud_tablegen::{
    CODE_POINTS, Check, CodepointMap, CpRange, Errors, Input, Kind, Source, ValueSet,
};

pub const ALIASES: Input = Input {
    file: "unicode-propertyvaluealiases.txt",
    key: "unicode-propertyvaluealiases",
    header: "# PropertyValueAliases-17.0.0.txt",
};

pub const GENERAL_CATEGORY: Input = Input {
    file: "unicode-derivedgeneralcategory.txt",
    key: "gc-data",
    header: "# DerivedGeneralCategory-17.0.0.txt",
};

pub const EAST_ASIAN_WIDTH: Input = Input {
    file: "unicode-eastasianwidth.txt",
    key: "eaw-data",
    header: "# EastAsianWidth-17.0.0.txt",
};

/// emoji-data.txt names no version on line 1; [`emoji_version`] checks the
/// line that does.
pub const EMOJI: Input = Input {
    file: "unicode-emoji-data.txt",
    key: "emoji-data",
    header: "# emoji-data.txt",
};

/// The `gc` short names, in PropertyValueAliases.txt's order.
pub const GC: [&str; 38] = [
    "C", "Cc", "Cf", "Cn", "Co", "Cs", "L", "LC", "Ll", "Lm", "Lo", "Lt", "Lu", "M", "Mc", "Me",
    "Mn", "N", "Nd", "Nl", "No", "P", "Pc", "Pd", "Pe", "Pf", "Pi", "Po", "Ps", "S", "Sc", "Sk",
    "Sm", "So", "Z", "Zl", "Zp", "Zs",
];

/// The `gc` values that name a group of categories (UAX #44,
/// `[[source: uax44]]`), which no code point has as its own.
const GC_GROUPS: [&str; 8] = ["C", "L", "LC", "M", "N", "P", "S", "Z"];

/// The `ea` short names, in PropertyValueAliases.txt's order.
pub const EA: [&str; 6] = ["A", "F", "H", "N", "Na", "W"];

/// The properties emoji-data.txt lists (UTS #51, `[[source: uts51-emoji]]`):
/// a row naming another is refused.
const EMOJI_PROPERTIES: [&str; 6] = [
    "Emoji",
    "Emoji_Presentation",
    "Emoji_Modifier",
    "Emoji_Modifier_Base",
    "Emoji_Component",
    "Extended_Pictographic",
];

/// The emoji version the tables are compiled from, a header line of
/// emoji-data.txt.
const EMOJI_VERSION: &str = "# Version: 17.0";

pub fn at(cp: u32) -> usize {
    usize::try_from(cp).unwrap_or(usize::MAX)
}

/// The position of `name` in `set`, which the caller declared.
pub fn index(set: &[&str], name: &str) -> u8 {
    let i = set
        .iter()
        .position(|v| *v == name)
        .expect("a declared name");
    u8::try_from(i).expect("fewer than 256 values")
}

/// `property`'s value set in PropertyValueAliases.txt, numbered as `order`.
pub fn values(
    aliases: &Source,
    property: &str,
    order: &[&str],
    errs: &mut Errors,
) -> Option<ValueSet> {
    match ValueSet::aliases(aliases, property, order) {
        Ok(v) => Some(v),
        Err(e) => {
            errs.extend(e);
            None
        }
    }
}

/// Every code point's General_Category, as its place in [`GC`]. The file
/// lists every code point, Cn included, so a code point it leaves out is
/// refused rather than defaulted.
pub fn general_category(src: &Source, gc: &ValueSet, errs: &mut Errors) -> Option<Vec<u8>> {
    let mut map = CodepointMap::<u8>::new();
    for row in src.rows() {
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let v = errs.take(gc.check(&row, 1));
        let (Some(r), Some(v)) = (r, v) else {
            continue;
        };
        if GC_GROUPS.contains(&row.field(1)) {
            errs.push(row.error(
                Kind::UnknownValue,
                1,
                format!(
                    "gc `{}` is a group of categories, not a code point's",
                    row.field(1)
                ),
            ));
            continue;
        }
        errs.take(map.assign(src, &row, r, u8::try_from(v).expect("38 values")));
    }
    errs.take(map.complete(src.path()))
}

/// Every code point's East_Asian_Width, as its place in [`EA`]: the file's
/// `@missing` default, then `wide`, the ranges its header says unlisted
/// code points there default to W, then the listed rows. Each of `wide` is
/// checked against the header's own text.
pub fn east_asian_width(
    src: &Source,
    ea: &ValueSet,
    wide: &[CpRange],
    errs: &mut Errors,
) -> Option<Vec<u8>> {
    let mut map = CodepointMap::<u8>::new();
    let mut defaults = 0;
    for row in src.missing() {
        defaults += 1;
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let v = errs.take(ea.check(&row, 1));
        if let (Some(r), Some(v)) = (r, v) {
            map.default_over(r, u8::try_from(v).expect("six values"));
        }
    }
    errs.count(src.path(), "@missing lines", defaults, 1);
    for r in wide {
        let named = format!("U+{:04X}..U+{:04X}", r.lo, r.hi);
        errs.require(Check::from(src.text().contains(&named)), || {
            src.error(
                Kind::Control,
                format!("the header does not give {named} as defaulting to \"W\""),
            )
        });
        map.default_over(*r, index(&EA, "W"));
    }
    for row in src.rows() {
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let v = errs.take(ea.check(&row, 1));
        if let (Some(r), Some(v)) = (r, v) {
            errs.take(map.assign(src, &row, r, u8::try_from(v).expect("six values")));
        }
    }
    errs.take(map.complete(src.path()))
}

/// Refused unless a header line of emoji-data.txt, before its first row, is
/// the version the tables are compiled from: its line 1 names no version.
pub fn emoji_version(src: &Source, errs: &mut Errors) {
    let found = src
        .text()
        .lines()
        .take_while(|l| l.is_empty() || l.starts_with('#'))
        .any(|l| l.trim_end() == EMOJI_VERSION);
    errs.require(Check::from(found), || {
        src.error(
            Kind::Header,
            format!(
                "no {EMOJI_VERSION:?} header line: is this the pinned file [{}]?",
                src.key()
            ),
        )
    });
}

/// The emoji-data.txt property names, for [`binary`].
pub fn emoji_properties() -> ValueSet {
    ValueSet::closed("emoji-data.txt property", "UTS #51", &EMOJI_PROPERTIES)
}

/// Whether each code point has binary `property`, from a file whose rows
/// are `<range> ; <property>`, with the number of rows that list it. Every
/// row is read as a range; when `names` is given, each row's property must
/// be one of them. Two rows of `property` over one code point are refused.
pub fn binary(
    src: &Source,
    property: &str,
    names: Option<&ValueSet>,
    errs: &mut Errors,
) -> (Vec<bool>, usize) {
    let mut map = CodepointMap::<u8>::new();
    let mut rows = 0;
    for row in src.rows() {
        let Some(r) = errs.take(row.range(0)) else {
            continue;
        };
        if let Some(names) = names {
            errs.take(names.check(&row, 1));
        }
        if row.field(1) != property {
            continue;
        }
        rows += 1;
        if errs.take(row.expect(2)).is_some() {
            errs.take(map.assign(src, &row, r, 1));
        }
    }
    let has = (0..CODE_POINTS)
        .map(|i| map.get(u32::try_from(i).expect("a code point")).is_some())
        .collect();
    (has, rows)
}
