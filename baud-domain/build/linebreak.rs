//! The line-breaking table, `OUT_DIR/linebreak_data.rs`: a two-stage trie,
//! `codepoint >> 8` to a block of 256 bytes, one a code point.
//!
//! * **Low six bits**: the code point's Line_Break (`[[source: unicode-linebreak]]`),
//!   its `@missing` default XX included, each value checked against
//!   PropertyValueAliases.txt's `lb` set
//!   (`[[source: unicode-propertyvaluealiases]]`), resolved as UAX #14's
//!   LB1 resolves it in the absence of other criteria (`[[source: uax14]]`):
//!   AI, SG and XX to AL; SA to CM for General_Category Mn and Mc
//!   (`[[source: gc-data]]`) and to AL otherwise; CJ to NS. A QU of
//!   General_Category Pi or Pf is `QuPi` or `QuPf`, which LB15a and LB15b
//!   single out. Stored as `LineClass`'s declaration order.
//! * **Bit 6**: East_Asian_Width F, W or H (`[[source: eaw-data]]`), the
//!   annex's `$EastAsian`, for LB19a and LB30.
//! * **Bit 7**: Extended_Pictographic (`[[source: emoji-data]]`) and
//!   unassigned, General_Category Cn, for LB30b.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use crate::ucd::{self, EA, GC, at, index};
use baud_tablegen::{CODE_POINTS, Check, CodepointMap, Emit, Errors, Input, Kind, Source, Trie};

pub const LINE_BREAK: Input = Input {
    file: "unicode-linebreak.txt",
    key: "unicode-linebreak",
    header: "# LineBreak-17.0.0.txt",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 5] = [
    ucd::ALIASES,
    LINE_BREAK,
    ucd::GENERAL_CATEGORY,
    ucd::EAST_ASIAN_WIDTH,
    ucd::EMOJI,
];

/// The `lb` short names, in PropertyValueAliases.txt's order.
const LB: [&str; 49] = [
    "AI", "AK", "AL", "AP", "AS", "B2", "BA", "BB", "BK", "CB", "CJ", "CL", "CM", "CP", "CR", "EB",
    "EM", "EX", "GL", "H2", "H3", "HH", "HL", "HY", "ID", "IN", "IS", "JL", "JT", "JV", "LF", "NL",
    "NS", "NU", "OP", "PO", "PR", "QU", "RI", "SA", "SG", "SP", "SY", "VF", "VI", "WJ", "XX", "ZW",
    "ZWJ",
];

/// `LineClass`'s variants in declaration order, which the low six bits
/// store.
const CLASSES: [&str; 46] = [
    "AL", "BA", "BB", "BK", "B2", "CB", "CL", "CM", "CP", "CR", "EB", "EM", "EX", "GL", "H2", "H3",
    "HH", "HL", "HY", "ID", "IN", "IS", "JL", "JT", "JV", "LF", "NL", "NS", "NU", "OP", "PO", "PR",
    "QU", "RI", "SP", "SY", "WJ", "ZW", "ZWJ", "AK", "AP", "AS", "VF", "VI", "QuPi", "QuPf",
];

const EAST_ASIAN: u8 = 0x40;
const PICTOGRAPHIC: u8 = 0x80;

/// LB1's resolution of `lb` at a code point of General_Category `gc`, as
/// a [`CLASSES`] name.
fn resolve<'a>(lb: &'a str, gc: &str) -> &'a str {
    match lb {
        "AI" | "SG" | "XX" => "AL",
        "SA" if gc == "Mn" || gc == "Mc" => "CM",
        "SA" => "AL",
        "CJ" => "NS",
        "QU" if gc == "Pi" => "QuPi",
        "QU" if gc == "Pf" => "QuPf",
        other => other,
    }
}

/// The table, or every failure found reading the inputs `read` returns by
/// their names under `data/`.
pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    let Ok([aliases, lbt, dgc, eaw, emoji]) = <[Source; 5]>::try_from(loaded) else {
        return Err(errs);
    };
    ucd::emoji_version(&emoji, &mut errs);
    let (Some(lb), Some(gc), Some(ea)) = (
        ucd::values(&aliases, "lb", &LB, &mut errs),
        ucd::values(&aliases, "gc", &GC, &mut errs),
        ucd::values(&aliases, "ea", &EA, &mut errs),
    ) else {
        return Err(errs);
    };
    errs.require(Check::from(CLASSES.len() <= 64), || {
        lbt.error(
            Kind::Invariant,
            "more than 64 classes will not fit six bits",
        )
    });

    let mut line = CodepointMap::<u8>::new();
    let mut defaults = 0;
    for row in lbt.missing() {
        defaults += 1;
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let v = errs.take(lb.check(&row, 1));
        if let (Some(r), Some(v)) = (r, v) {
            line.default_over(r, u8::try_from(v).expect("49 values"));
        }
    }
    errs.count(lbt.path(), "@missing lines", defaults, 1);
    for row in lbt.rows() {
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let v = errs.take(lb.check(&row, 1));
        if let (Some(r), Some(v)) = (r, v) {
            errs.take(line.assign(&lbt, &row, r, u8::try_from(v).expect("49 values")));
        }
    }
    let cat = ucd::general_category(&dgc, &gc, &mut errs);
    let width = ucd::east_asian_width(&eaw, &ea, &[], &mut errs);
    let names = ucd::emoji_properties();
    let (pictographic, _) = ucd::binary(&emoji, "Extended_Pictographic", Some(&names), &mut errs);
    let line = errs.take(line.complete(lbt.path()));
    let (Some(line), Some(cat), Some(width)) = (line, cat, width) else {
        return Err(errs);
    };

    // Every (lb, gc) pair's class, so the walk below is a lookup.
    let mut resolved = [[0u8; GC.len()]; LB.len()];
    for (l, row) in resolved.iter_mut().enumerate() {
        for (g, v) in row.iter_mut().enumerate() {
            *v = index(&CLASSES, resolve(LB[l], GC[g]));
        }
    }
    let east = [index(&EA, "F"), index(&EA, "W"), index(&EA, "H")];
    let cn = index(&GC, "Cn");
    let mut bytes = vec![0u8; CODE_POINTS];
    for (cp, b) in bytes.iter_mut().enumerate() {
        *b = resolved[usize::from(line[cp])][usize::from(cat[cp])];
        if east.contains(&width[cp]) {
            *b |= EAST_ASIAN;
        }
        if pictographic[cp] && cat[cp] == cn {
            *b |= PICTOGRAPHIC;
        }
    }
    // Positive controls: each resolution LB1 makes, the two quotation
    // splits, and both flags.
    for (cp, want, flags) in [
        // AL; OP of East_Asian_Width A, not East Asian; XX; SG.
        (0x0041, "AL", 0),
        (0x00A1, "OP", 0),
        (0x0378, "AL", 0),
        (0xD800, "AL", 0),
        // HYPHEN, 17.0.0's Unambiguous_Hyphen.
        (0x2010, "HH", 0),
        // SA as a mark and as a letter; CJ, wide.
        (0x0E31, "CM", 0),
        (0x0E01, "AL", 0),
        (0x3041, "NS", EAST_ASIAN),
        (0x0022, "QU", 0),
        (0x201C, "QuPi", 0),
        (0x201D, "QuPf", 0),
        (0x4E00, "ID", EAST_ASIAN),
        // An assigned emoji, and an unassigned Extended_Pictographic.
        (0x1F600, "ID", EAST_ASIAN),
        (0x1FFFD, "ID", PICTOGRAPHIC),
    ] {
        let got = bytes[at(cp)];
        let wanted = index(&CLASSES, want) | flags;
        errs.require(Check::from(got == wanted), || {
            lbt.error(
                Kind::Control,
                format!("U+{cp:04X} is {got:#04X}, {want} with flags {flags:#04X} expected"),
            )
        });
    }

    let Some(trie) = errs.take(Trie::build("LINE", &bytes, 8)) else {
        return Err(errs);
    };
    let mut out = Emit::new(
        "linebreak_data",
        "baud-domain/build.rs (build/linebreak.rs)",
        &[&lbt, &dgc, &eaw, &emoji, &aliases],
    );
    out.bytes(
        "LINE_INDEX",
        &[
            "Line_Break, resolved by LB1, as a two-stage trie.",
            &format!(
                "Stage 1: codepoint >> 8 to one of {} distinct blocks.",
                trie.distinct
            ),
            "",
            "**Low six bits** are `LineClass` discriminants in declaration order;",
            "bit 6 is East_Asian_Width F, W or H; bit 7 is Extended_Pictographic",
            "and unassigned.",
        ],
        &trie.index,
    );
    out.bytes(
        "LINE_BLOCKS",
        &["Stage 2: one byte a code point."],
        &trie.blocks,
    );
    out.enum_order("crate::linebreak::LineClass", &CLASSES);
    out.code(
        "// `LineProps::of` reads a class as `LineClass::ALL[byte & 0x3F]`:\n\
         // `ALL` is every class at its own discriminant.\n\
         const _: () = {\n    \
         let all = crate::linebreak::LineClass::ALL;\n    \
         let mut i = 0;\n    \
         while i < all.len() {\n        \
         assert!(all[i] as usize == i, \"LineClass::ALL is not in declaration order\");\n        \
         i += 1;\n    \
         }\n\
         };",
    );
    errs.or(out)
}
