//! The grapheme-cluster tables, `OUT_DIR/grapheme_data.rs`, compiled from
//! the pinned Unicode Character Database 17.0.0 files:
//!
//! * **BREAK**, a two-stage trie, `codepoint >> 8` to a block of 256 bytes,
//!   one a code point. The low four bits are the code point's
//!   Grapheme_Cluster_Break (`[[source: gbp-data]]`, UAX #29,
//!   `[[source: uax29]]`) as `GraphemeClass`'s declaration order, its
//!   `@missing` default Other included, each value checked against
//!   PropertyValueAliases.txt's `GCB` set
//!   (`[[source: unicode-propertyvaluealiases]]`); the high four are the
//!   lookback sets GB9c and GB11 read, `Lookback`'s bits: 1
//!   Extended_Pictographic (`[[source: emoji-data]]`), 2 InCB Linker, 4 InCB
//!   Consonant and 8 InCB Extend (`[[source: dcp-data]]`).
//! * **SPACING_EXTEND**, the code points of General_Category Mc
//!   (`[[source: gc-data]]`) that are Grapheme_Cluster_Break Extend, as
//!   sorted ranges, which GB9 joins in both of UAX #29's variants and which
//!   take a column while `?2027` is reset.
//!
//! GCB's E_Base, E_Base_GAZ, E_Modifier and Glue_After_Zwj have no
//! `GraphemeClass`, and a row giving one is refused: no row of 17.0.0
//! does.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use crate::ucd::{self, GC, at, index};
use baud_tablegen::{CODE_POINTS, Check, CodepointMap, Emit, Errors, Input, Kind, Source, Trie};

pub const BREAK_PROPERTY: Input = Input {
    file: "unicode-graphemebreakproperty.txt",
    key: "gbp-data",
    header: "# GraphemeBreakProperty-17.0.0.txt",
};

pub const CORE_PROPERTIES: Input = Input {
    file: "unicode-derivedcoreproperties.txt",
    key: "dcp-data",
    header: "# DerivedCoreProperties-17.0.0.txt",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 5] = [
    ucd::ALIASES,
    BREAK_PROPERTY,
    ucd::EMOJI,
    CORE_PROPERTIES,
    ucd::GENERAL_CATEGORY,
];

/// The `GCB` short names, in PropertyValueAliases.txt's order.
const GCB: [&str; 18] = [
    "CN", "CR", "EB", "EBG", "EM", "EX", "GAZ", "L", "LF", "LV", "LVT", "PP", "RI", "SM", "T", "V",
    "XX", "ZWJ",
];

/// `GraphemeClass`'s variants in declaration order, which the low nibble
/// stores, each with the `GCB` value it stands for.
const CLASSES: [(&str, &str); 14] = [
    ("Other", "XX"),
    ("Cr", "CR"),
    ("Lf", "LF"),
    ("Control", "CN"),
    ("Extend", "EX"),
    ("Zwj", "ZWJ"),
    ("RegionalIndicator", "RI"),
    ("Prepend", "PP"),
    ("SpacingMark", "SM"),
    ("L", "L"),
    ("V", "V"),
    ("T", "T"),
    ("Lv", "LV"),
    ("Lvt", "LVT"),
];

/// The `InCB` values, in PropertyValueAliases.txt's order.
const INCB: [&str; 4] = ["Consonant", "Extend", "Linker", "None"];

/// `Lookback`'s bits, the high nibble.
const PICTOGRAPHIC: u8 = 1;
const LINKER: u8 = 2;
const CONSONANT: u8 = 4;
const INCB_EXTEND: u8 = 8;

/// Spacing marks classed Extend, as ranges, in 17.0.0.
const SPACING_EXTEND: usize = 46;

/// The class of each `GCB` value, as its place in [`CLASSES`]; `None` for
/// the four with no class.
fn class_of(gcb: usize) -> Option<u8> {
    let short = GCB[gcb];
    CLASSES
        .iter()
        .position(|&(_, v)| v == short)
        .map(|i| u8::try_from(i).expect("14 classes"))
}

/// The tables, or every failure found reading the inputs `read` returns by
/// their names under `data/`.
pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    let Ok([aliases, gbp, emoji, dcp, dgc]) = <[Source; 5]>::try_from(loaded) else {
        return Err(errs);
    };
    ucd::emoji_version(&emoji, &mut errs);
    let (Some(gcb), Some(incb), Some(gc)) = (
        ucd::values(&aliases, "GCB", &GCB, &mut errs),
        ucd::values(&aliases, "InCB", &INCB, &mut errs),
        ucd::values(&aliases, "gc", &GC, &mut errs),
    ) else {
        return Err(errs);
    };
    // The nibbles: the Python's asserts, kept as the encoding's rule.
    errs.require(Check::from(CLASSES.len() <= 16), || {
        gbp.error(
            Kind::Invariant,
            "more than 16 classes will not fit four bits",
        )
    });

    // Grapheme_Cluster_Break: the `@missing` default, then every row, each
    // value spelled as its long name.
    let mut class = CodepointMap::<u8>::new();
    let mut defaults = 0;
    for row in gbp.missing() {
        defaults += 1;
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let v = errs.take(gcb.check_long(&row, 1));
        if let (Some(r), Some(Some(c))) = (r, v.map(class_of)) {
            class.default_over(r, c);
        }
    }
    errs.count(gbp.path(), "@missing lines", defaults, 1);
    for row in gbp.rows() {
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let Some(v) = errs.take(gcb.check_long(&row, 1)) else {
            continue;
        };
        let Some(c) = class_of(v) else {
            errs.push(row.error(
                Kind::UnknownValue,
                1,
                format!("GCB `{}` has no GraphemeClass", row.field(1)),
            ));
            continue;
        };
        if let Some(r) = r {
            errs.take(class.assign(&gbp, &row, r, c));
        }
    }

    // InCB, DerivedCoreProperties.txt's one enumerated property: rows of
    // three fields, its `@missing` default None.
    let mut conjunct = CodepointMap::<u8>::new();
    let mut incb_rows = 0;
    for row in dcp.missing().filter(|r| r.field(1) == "InCB") {
        let r = errs.take(row.expect(3).and_then(|()| row.range(0)));
        if let (Some(r), Some(v)) = (r, errs.take(incb.check(&row, 2))) {
            conjunct.default_over(r, u8::try_from(v).expect("four values"));
        }
    }
    for row in dcp.rows() {
        let Some(r) = errs.take(row.range(0)) else {
            continue;
        };
        if row.field(1) != "InCB" {
            continue;
        }
        incb_rows += 1;
        let v = errs.take(row.expect(3).and_then(|()| incb.check(&row, 2)));
        if let Some(v) = v {
            errs.take(conjunct.assign(&dcp, &row, r, u8::try_from(v).expect("four values")));
        }
    }
    errs.count(dcp.path(), "InCB rows", incb_rows, 505);

    let names = ucd::emoji_properties();
    let (pictographic, _) = ucd::binary(&emoji, "Extended_Pictographic", Some(&names), &mut errs);
    let cat = ucd::general_category(&dgc, &gc, &mut errs);
    let class = errs.take(class.complete(gbp.path()));
    let conjunct = errs.take(conjunct.complete(dcp.path()));
    let (Some(class), Some(conjunct), Some(cat)) = (class, conjunct, cat) else {
        return Err(errs);
    };

    let (consonant, linker, extend) = (
        index(&INCB, "Consonant"),
        index(&INCB, "Linker"),
        index(&INCB, "Extend"),
    );
    let mut bytes = vec![0u8; CODE_POINTS];
    for (cp, b) in bytes.iter_mut().enumerate() {
        let mut flags = 0;
        if pictographic[cp] {
            flags |= PICTOGRAPHIC;
        }
        flags |= match conjunct[cp] {
            v if v == linker => LINKER,
            v if v == consonant => CONSONANT,
            v if v == extend => INCB_EXTEND,
            _ => 0,
        };
        *b = class[cp] | (flags << 4);
    }
    let class_named = |name: &str| {
        let i = CLASSES
            .iter()
            .position(|&(n, _)| n == name)
            .expect("a class");
        u8::try_from(i).expect("14 classes")
    };
    // Positive controls: a code point of each class, and of each lookback
    // set.
    for (cp, want, flags) in [
        (0x000D, "Cr", 0),
        (0x000A, "Lf", 0),
        (0x0000, "Control", 0),
        (0x0041, "Other", 0),
        (0x0301, "Extend", INCB_EXTEND),
        (0x200D, "Zwj", INCB_EXTEND),
        (0x1F1E6, "RegionalIndicator", 0),
        (0x0600, "Prepend", 0),
        (0x0903, "SpacingMark", 0),
        (0x1100, "L", 0),
        (0x1160, "V", 0),
        (0x11A8, "T", 0),
        (0xAC00, "Lv", 0),
        (0xAC01, "Lvt", 0),
        (0x1F600, "Other", PICTOGRAPHIC),
        (0x0915, "Other", CONSONANT),
        (0x094D, "Extend", LINKER),
    ] {
        let got = bytes[at(cp)];
        let wanted = class_named(want) | (flags << 4);
        errs.require(Check::from(got == wanted), || {
            gbp.error(
                Kind::Control,
                format!("U+{cp:04X} is {got:#04X}, {want} with lookback {flags} expected"),
            )
        });
    }

    // GB9's spacing marks: gc Mc and GCB Extend, coalesced.
    let (mc, ex) = (index(&GC, "Mc"), class_named("Extend"));
    let mut spacing: Vec<(u32, u32)> = Vec::new();
    for cp in 0..CODE_POINTS {
        if cat[cp] != mc || class[cp] != ex {
            continue;
        }
        let c = u32::try_from(cp).expect("a code point");
        match spacing.last_mut() {
            Some(last) if last.1 + 1 == c => last.1 = c,
            _ => spacing.push((c, c)),
        }
    }
    errs.count(
        dgc.path(),
        "spacing Extend ranges",
        spacing.len(),
        SPACING_EXTEND,
    );
    errs.require(
        Check::from(spacing.first() == Some(&(0x09BE, 0x09BE))),
        || {
            dgc.error(
                Kind::Control,
                "U+09BE BENGALI VOWEL SIGN AA is not the first spacing Extend",
            )
        },
    );

    let Some(trie) = errs.take(Trie::build("BREAK", &bytes, 8)) else {
        return Err(errs);
    };
    let mut out = Emit::new(
        "grapheme_data",
        "baud-domain/build.rs (build/grapheme.rs)",
        &[&gbp, &emoji, &dcp, &dgc, &aliases],
    );
    out.bytes(
        "BREAK_INDEX",
        &[
            "Grapheme_Cluster_Break and the lookback sets, as a two-stage trie.",
            &format!(
                "Stage 1: codepoint >> 8 to one of {} distinct blocks.",
                trie.distinct
            ),
            "",
            "**Low four bits** are `GraphemeClass` discriminants in declaration",
            "order; **high four bits** are `Lookback`'s: Extended_Pictographic,",
            "InCB Linker, InCB Consonant, InCB Extend.",
        ],
        &trie.index,
    );
    out.bytes(
        "BREAK_BLOCKS",
        &["Stage 2: one byte a code point, the class and the lookback sets."],
        &trie.blocks,
    );
    out.array(
        "SPACING_EXTEND",
        "(u32, u32)",
        &[
            "The spacing marks UAX #29 classes Extend: general category `Mc` and",
            "Grapheme_Cluster_Break `Extend`, sorted, for a binary search on the",
            "cold side of a join.",
        ],
        &spacing,
        1,
        |&(lo, hi)| format!("(0x{lo:04x}, 0x{hi:04x})"),
    );
    let variants: Vec<&str> = CLASSES.iter().map(|&(n, _)| n).collect();
    out.enum_order("crate::grapheme::GraphemeClass", &variants);
    out.code(
        "// `of_with_lookback` reads a class as `FROM_INDEX[byte & 0xF]`:\n\
         // `FROM_INDEX` is every class at its own discriminant.\n\
         const _: () = {\n    \
         let all = crate::grapheme::GraphemeClass::FROM_INDEX;\n    \
         let mut i = 0;\n    \
         while i < 14 {\n        \
         assert!(all[i] as usize == i, \"GraphemeClass::FROM_INDEX is not in declaration order\");\n        \
         i += 1;\n    \
         }\n\
         };",
    );
    errs.or(out)
}
