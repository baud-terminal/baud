//! The bidirectional tables, `OUT_DIR/bidi_data.rs`, compiled from the
//! pinned Unicode Character Database 17.0.0 files. Two tries, both indexed
//! by `codepoint >> 8` and then the low byte:
//!
//! * **BIDI**, one byte a code point: Bidi_Class in the low five bits
//!   (`BidiClass`'s declaration order, UAX #9 Table 4, `[[source: uax9-bidi]]`),
//!   Bidi_Mirrored in bit 5, and Bidi_Paired_Bracket_Type in bits 6-7 (0
//!   none, 1 open, 2 close). Bidi_Class is DerivedBidiClass.txt with its
//!   `@missing` defaults, applied in the file's order, for the code points
//!   it does not list (`[[source: unicode-derivedbidiclass]]`), each value
//!   checked against PropertyValueAliases.txt's `bc` set
//!   (`[[source: unicode-propertyvaluealiases]]`); Bidi_Mirrored is
//!   UnicodeData.txt field 9 (`[[source: unicode-data]]`); the bracket type
//!   is BidiBrackets.txt (`[[source: unicode-bidibrackets]]`).
//! * **MIRROR**, two bytes a code point: the low nine bits index
//!   `MIRROR_GLYPHS`, the code point's Bidi_Mirroring_Glyph
//!   (`[[source: unicode-bidimirroring]]`), 0 for none; bits 9-14 are the
//!   bracket pair it belongs to, 0 for none, so that BD16's comparison "or
//!   its canonical equivalent" is one equality: the two canonically
//!   equivalent brackets, U+2329 and U+232A, take the pair of U+3008 and
//!   U+3009, their decompositions in UnicodeData.txt.
//! * **BOX_COUNTERPART**, a byte for each Box Drawing character
//!   U+2500-U+257F: the offset in the block of the character whose
//!   UnicodeData.txt name says the same with LEFT and RIGHT exchanged, for
//!   terminal-wg's box mirroring, `?2500`. A name is read as what it draws
//!   -- each arm and its weight, a dash's count, an arc, a diagonal -- so
//!   that "LIGHT LEFT AND HEAVY RIGHT" finds "HEAVY LEFT AND LIGHT RIGHT",
//!   whose words come in another order; a name with no side is its own.
//!
//! A pure function of its five inputs: `main` reads them and writes what
//! [`build`] returns.

use baud_tablegen::{
    CODE_POINTS, Check, CodepointMap, CpRange, Emit, Entry, Errors, Input, Kind, Source, Trie,
    ValueSet,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 5] = [
    Input {
        file: "unicode-propertyvaluealiases.txt",
        key: "unicode-propertyvaluealiases",
        header: "# PropertyValueAliases-17.0.0.txt",
    },
    Input {
        file: "unicode-derivedbidiclass.txt",
        key: "unicode-derivedbidiclass",
        header: "# DerivedBidiClass-17.0.0.txt",
    },
    // UnicodeData.txt carries no header: its first entry is the control.
    Input {
        file: "unicode-unicodedata.txt",
        key: "unicode-data",
        header: "0000;<control>;Cc;0;BN;;;;;N;NULL;;;;",
    },
    Input {
        file: "unicode-bidimirroring.txt",
        key: "unicode-bidimirroring",
        header: "# BidiMirroring-17.0.0.txt",
    },
    Input {
        file: "unicode-bidibrackets.txt",
        key: "unicode-bidibrackets",
        header: "# BidiBrackets-17.0.0.txt",
    },
];

/// `BidiClass`'s declaration order, UAX #9 Table 4's, which the trie's low
/// five bits store and the generated pin holds the enum to.
const TABLE_4: [&str; 23] = [
    "L", "R", "AL", "EN", "ES", "ET", "AN", "CS", "NSM", "BN", "B", "S", "WS", "ON", "LRE", "LRO",
    "RLE", "RLO", "PDF", "LRI", "RLI", "FSI", "PDI",
];

/// UnicodeData.txt's entries in 17.0.0: 40,575 lines, 20 of them the
/// second line of a First/Last range.
const ENTRIES: usize = 40_555;

fn class(name: &str) -> u8 {
    let i = TABLE_4
        .iter()
        .position(|c| *c == name)
        .expect("a TABLE_4 name");
    u8::try_from(i).expect("23 classes")
}

/// The tables, or every failure found reading the inputs `read` returns by
/// their names under `data/`.
pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    let Ok([aliases, classes, data, mirroring, brackets]) = <[Source; 5]>::try_from(loaded) else {
        return Err(errs);
    };
    let bc = match ValueSet::aliases(&aliases, "bc", &TABLE_4) {
        Ok(v) => v,
        Err(e) => {
            errs.extend(e);
            return Err(errs);
        }
    };

    // Bidi_Class: the `@missing` defaults first, in the file's order (each
    // later line narrows the one before), then every listed range.
    let mut bidi = CodepointMap::<u8>::new();
    let mut defaults = 0;
    for row in classes.missing() {
        defaults += 1;
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let v = errs.take(bc.check_long(&row, 1));
        if let (Some(r), Some(v)) = (r, v) {
            bidi.default_over(r, u8::try_from(v).expect("23 classes"));
        }
    }
    errs.count(classes.path(), "@missing lines", defaults, 24);
    for row in classes.rows() {
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let v = errs.take(bc.check(&row, 1));
        if let (Some(r), Some(v)) = (r, v) {
            errs.take(bidi.assign(&classes, &row, r, u8::try_from(v).expect("23 classes")));
        }
    }
    // Positive controls, one a default range: an unassigned code point in the
    // Hebrew block is R, one in Arabic is AL, one in Currency Symbols is ET,
    // a noncharacter is BN, and U+0378 (unassigned Greek) takes the file's L.
    for (cp, want) in [
        (0x05FF, "R"),
        (0x07BF, "AL"),
        (0x20CF, "ET"),
        (0xFDD0, "BN"),
        (0x0378, "L"),
        (0x0028, "ON"),
        (0x05D0, "R"),
        (0x0627, "AL"),
        (0x0030, "EN"),
        (0x2067, "RLI"),
        (0x202E, "RLO"),
        (0x2029, "B"),
    ] {
        let got = bidi.get(cp);
        errs.require(Check::from(got == Some(class(want))), || {
            classes.error(
                Kind::Control,
                format!(
                    "U+{cp:04X} is Bidi_Class {}, {want} expected",
                    got.and_then(|g| TABLE_4.get(usize::from(g)))
                        .unwrap_or(&"none")
                ),
            )
        });
    }
    let Some(bidi) = errs.take(bidi.complete(classes.path())) else {
        return Err(errs);
    };

    // Bidi_Mirrored: UnicodeData.txt field 9, ranges given as First/Last
    // pairs. Unlisted code points take the binary property's default, N.
    let yn = ValueSet::closed("Bidi_Mirrored", "UnicodeData.txt field 9", &["N", "Y"]);
    let entries: Vec<Entry<'_>> = data
        .unicode_data()
        .into_iter()
        .filter_map(|e| errs.take(e))
        .collect();
    errs.count(
        data.path(),
        "UnicodeData.txt entries",
        entries.len(),
        ENTRIES,
    );
    let mut listed = CodepointMap::<u8>::new();
    for e in &entries {
        if let Some(v) = errs.take(yn.check(&e.row, 9)) {
            let v = u8::try_from(v).expect("two values");
            errs.take(listed.assign(&data, &e.row, e.range, v));
        }
    }
    let mirrored = |cp: u32| listed.get(cp) == Some(1);
    for (cp, want) in [(0x28, 1), (0x2211, 1), (0x41, 0), (0xFD3E, 0), (0xFD3F, 0)] {
        // L4's own note: U+FD3E and U+FD3F "are not mirrored".
        errs.require(Check::from(mirrored(cp) == (want == 1)), || {
            data.error(
                Kind::Control,
                format!("U+{cp:04X} is Bidi_Mirrored {}", ["N", "Y"][1 - want]),
            )
        });
    }

    // Bidi_Mirroring_Glyph.
    let mut glyph = CodepointMap::<u32>::new();
    let mut glyphs_listed = 0;
    for row in mirroring.rows() {
        let pair = errs.take(
            row.expect(2)
                .and_then(|()| Ok((row.code_point(0)?, row.code_point(1)?))),
        );
        if let Some((cp, g)) = pair {
            glyphs_listed += 1;
            errs.take(glyph.assign(&mirroring, &row, CpRange { lo: cp, hi: cp }, g));
            errs.require(Check::from(mirrored(cp)), || {
                row.error(
                    Kind::Invariant,
                    0,
                    format!("U+{cp:04X} has a mirroring glyph and is not Bidi_Mirrored"),
                )
            });
        }
    }
    errs.count(mirroring.path(), "mirroring glyphs", glyphs_listed, 428);
    // Bidi_Mirrored=Yes with no glyph exists (U+2211 N-ARY SUMMATION): the
    // two properties are different, and L4 needs the first, a font the second.
    for (cp, want) in [(0x28, Some(0x29)), (0x29, Some(0x28)), (0x2211, None)] {
        errs.require(Check::from(glyph.get(cp) == want), || {
            mirroring.error(
                Kind::Control,
                format!(
                    "U+{cp:04X}'s mirroring glyph is {:X?}, {want:X?} expected",
                    glyph.get(cp)
                ),
            )
        });
    }

    // Bidi_Paired_Bracket and its type. BidiBrackets.txt's header: "two
    // characters, A and B, form a bracket pair if A has gc=Ps and B has
    // gc=Pe, both have bc=ON and Bidi_M=Y, and bmg of A is B".
    let types = ValueSet::closed(
        "Bidi_Paired_Bracket_Type",
        "BidiBrackets.txt field 2",
        &["n", "o", "c"],
    );
    let (open, close) = (1, 2);
    let mut bpb = CodepointMap::<u32>::new();
    let mut bpt = CodepointMap::<u8>::new();
    for row in brackets.rows() {
        let fields = errs.take(row.expect(3).and_then(|()| {
            Ok((
                row.code_point(0)?,
                row.code_point(1)?,
                types.check(&row, 2)?,
            ))
        }));
        let Some((cp, other, t)) = fields else {
            continue;
        };
        if t == 0 {
            continue;
        }
        let one = CpRange { lo: cp, hi: cp };
        errs.take(bpb.assign(&brackets, &row, one, other));
        errs.take(bpt.assign(&brackets, &row, one, u8::try_from(t).expect("three types")));
    }
    let pairs: Vec<(u32, u32)> = bpb.assigned().collect();
    let opens = bpt.assigned().filter(|&(_, t)| t == open).count();
    errs.count(brackets.path(), "paired brackets", pairs.len(), 128);
    errs.count(brackets.path(), "opening brackets", opens, 64);
    for &(cp, other) in &pairs {
        let types = BTreeSet::from([bpt.get(cp), bpt.get(other)]);
        let symmetric =
            bpb.get(other) == Some(cp) && types == BTreeSet::from([Some(open), Some(close)]);
        let on = bidi[at(cp)] == class("ON") && mirrored(cp);
        // So Bidi_Paired_Bracket is Bidi_Mirroring_Glyph for every bracket.
        let is_glyph = glyph.get(cp) == Some(other);
        for (holds, what) in [
            (symmetric, "is not its pair's pair, one open and one close"),
            (on, "is not bc=ON and Bidi_M=Y"),
            (is_glyph, "is not its pair's mirroring glyph"),
        ] {
            errs.require(Check::from(holds), || {
                brackets.error(Kind::Invariant, format!("bracket U+{cp:04X} {what}"))
            });
        }
    }
    // BD16's canonical equivalents, from the canonical decompositions:
    // "canonical equivalents only exist between U+3008/U+3009, and
    // U+2329/U+232A".
    let mut canonical: Vec<(u32, u32)> = Vec::new();
    for e in &entries {
        let dec = e.row.field(5);
        let single = !dec.is_empty() && !dec.starts_with('<') && !dec.contains(' ');
        if e.range.is_one() && bpb.is_assigned(e.range.lo) && single {
            if let Some(to) = errs.take(e.row.code_point(5)) {
                canonical.push((e.range.lo, to));
            }
        }
    }
    errs.require(
        Check::from(canonical == [(0x2329, 0x3008), (0x232A, 0x3009)]),
        || {
            data.error(
                Kind::Invariant,
                format!("the brackets' canonical decompositions are {canonical:X?}"),
            )
        },
    );
    let mut pair: BTreeMap<u32, u16> = BTreeMap::new();
    for &(cp, _) in &pairs {
        if bpt.get(cp) == Some(open) && !canonical.iter().any(|&(c, _)| c == cp) {
            let n = u16::try_from(pair.len() + 1).expect("64 brackets");
            pair.insert(cp, n);
        }
    }
    for &(cp, to) in &canonical {
        if bpt.get(cp) == Some(open) {
            if let Some(&n) = pair.get(&to) {
                pair.insert(cp, n);
            }
        }
    }
    let opened: Vec<(u32, u16)> = pair.iter().map(|(&c, &n)| (c, n)).collect();
    for (cp, n) in opened {
        if let Some(other) = bpb.get(cp) {
            pair.insert(other, n);
        }
    }
    let numbers: BTreeSet<u16> = pair.values().copied().collect();
    errs.require(
        Check::from(numbers.len() == 63 && numbers.last().is_some_and(|&m| m < 64)),
        || {
            brackets.error(
                Kind::Invariant,
                format!(
                    "{} bracket pairs, up to {:?}: 63 below 64 expected",
                    numbers.len(),
                    numbers.last()
                ),
            )
        },
    );
    let p = |cp: u32| pair.get(&cp).copied();
    errs.require(
        Check::from(p(0x28) == p(0x29) && p(0x2329) == p(0x3009) && p(0x2329) != p(0x28)),
        || {
            brackets.error(
                Kind::Control,
                "U+0028/U+0029 and U+2329/U+3009 are not paired as BD16 says",
            )
        },
    );

    // The two value arrays.
    let mut props = vec![0u8; CODE_POINTS];
    for (cp, v) in props.iter_mut().enumerate() {
        let c = u32::try_from(cp).expect("a code point");
        *v = bidi[cp] | (u8::from(mirrored(c)) << 5);
        match bpt.get(c) {
            Some(t) if t == open => *v |= 1 << 6,
            Some(t) if t == close => *v |= 2 << 6,
            _ => {}
        }
    }
    let distinct: BTreeSet<u32> = glyph.assigned().map(|(_, g)| g).collect();
    let glyphs: Vec<u32> = [0].into_iter().chain(distinct).collect();
    errs.require(Check::from(glyphs.len() < 512), || {
        mirroring.error(
            Kind::Invariant,
            format!("{} glyphs will not fit nine bits", glyphs.len()),
        )
    });
    let slot: HashMap<u32, u16> = glyphs
        .iter()
        .enumerate()
        .map(|(i, &g)| (g, u16::try_from(i).unwrap_or(u16::MAX)))
        .collect();
    let mut mirror = vec![0u16; CODE_POINTS];
    for (cp, g) in glyph.assigned() {
        mirror[at(cp)] = slot[&g];
    }
    for (&cp, &n) in &pair {
        mirror[at(cp)] |= n << 9;
    }
    let box_ = box_counterparts(&data, &entries, &mut errs);
    let bidi_trie = errs.take(Trie::build("BIDI", &props, 8));
    let mirror_trie = errs.take(Trie::build("MIRROR", &mirror, 8));
    let (Some(bidi_trie), Some(mirror_trie), Some(box_)) = (bidi_trie, mirror_trie, box_) else {
        return Err(errs);
    };

    let mut out = Emit::new(
        "bidi_data",
        "baud-domain/build.rs (build/bidi.rs)",
        &[&classes, &data, &mirroring, &brackets, &aliases],
    );
    let stage1 = |n: usize| format!("Stage 1: codepoint >> 8 to one of {n} distinct blocks.");
    out.bytes(
        "BIDI_INDEX",
        &[
            "Bidi_Class, Bidi_Mirrored and Bidi_Paired_Bracket_Type, as a two-stage",
            &format!("trie. {}", stage1(bidi_trie.distinct)),
            "",
            "**Low five bits** are `BidiClass` discriminants in declaration order",
            "(DerivedBidiClass.txt, its `@missing` defaults included); bit 5 is",
            "Bidi_Mirrored (UnicodeData.txt); bits 6-7 are 1 for an opening and 2",
            "for a closing paired bracket (BidiBrackets.txt).",
        ],
        &bidi_trie.index,
    );
    out.bytes(
        "BIDI_BLOCKS",
        &["Stage 2: one byte a code point."],
        &bidi_trie.blocks,
    );
    out.array(
        "MIRROR_INDEX",
        "u8",
        &[
            "Bidi_Mirroring_Glyph and the bracket pair, as a two-stage trie.",
            &stage1(mirror_trie.distinct),
        ],
        &mirror_trie.index,
        24,
        u8::to_string,
    );
    out.array(
        "MIRROR_BLOCKS",
        "u16",
        &[
            "Stage 2: the low nine bits index [`MIRROR_GLYPHS`], 0 for none",
            "(BidiMirroring.txt); bits 9-14 are the bracket pair, 0 for none,",
            "with U+2329 and U+232A in U+3008 and U+3009's pair.",
        ],
        &mirror_trie.blocks,
        16,
        u16::to_string,
    );
    out.array(
        "MIRROR_GLYPHS",
        "u32",
        &[&format!(
            "The {} distinct mirroring glyphs, after a 0 for none.",
            glyphs.len() - 1
        )],
        &glyphs,
        10,
        |v| format!("0x{v:04X}"),
    );
    out.array(
        "BOX_COUNTERPART",
        "u8",
        &[
            "Each Box Drawing character's left-right counterpart, as its offset",
            "in U+2500-U+257F: the name with LEFT and RIGHT exchanged",
            "(UnicodeData.txt), itself where the name has no side.",
        ],
        &box_,
        16,
        |v| format!("0x{v:02X}"),
    );
    out.enum_order("crate::bidi::BidiClass", &TABLE_4);
    out.code(
        "// `BidiProps::of` reads a class as `BidiClass::ALL[byte & 0x1F]`:\n\
         // `ALL` is every class at its own discriminant.\n\
         const _: () = {\n    \
         let all = crate::bidi::BidiClass::ALL;\n    \
         let mut i = 0;\n    \
         while i < all.len() {\n        \
         assert!(all[i] as usize == i, \"BidiClass::ALL is not in declaration order\");\n        \
         i += 1;\n    \
         }\n\
         };",
    );
    errs.or(out)
}

fn at(cp: u32) -> usize {
    usize::try_from(cp).unwrap_or(usize::MAX)
}

/// What a Box Drawing name draws: the set of (arm, weight) and the words
/// that are neither, or the words as they are for a diagonal. The name's
/// groups are split at AND; a group's weight is its arms', and a group
/// naming none takes the last named, so "LIGHT DOWN AND RIGHT" is light
/// both ways and "DOWN LIGHT AND RIGHT HEAVY" is not.
#[derive(PartialEq, Eq, Hash, Debug)]
enum Drawn {
    Words(Vec<String>),
    Arms(BTreeSet<(&'static str, Option<String>)>, Vec<String>),
}

const WEIGHTS: [&str; 4] = ["LIGHT", "HEAVY", "DOUBLE", "SINGLE"];

fn arms(word: &str) -> Option<&'static [&'static str]> {
    Some(match word {
        "UP" => &["UP"],
        "DOWN" => &["DOWN"],
        "LEFT" => &["LEFT"],
        "RIGHT" => &["RIGHT"],
        "VERTICAL" => &["UP", "DOWN"],
        "HORIZONTAL" => &["LEFT", "RIGHT"],
        _ => return None,
    })
}

fn drawn(words: &[String]) -> Drawn {
    if words.iter().any(|w| w == "DIAGONAL") {
        return Drawn::Words(words.to_vec());
    }
    let mut weight: Option<String> = None;
    let mut set = BTreeSet::new();
    let mut rest = Vec::new();
    for group in words.split(|w| w == "AND") {
        if let Some(w) = group.iter().find(|w| WEIGHTS.contains(&w.as_str())) {
            weight = Some(w.clone());
        }
        for w in group {
            if let Some(a) = arms(w) {
                set.extend(a.iter().map(|&arm| (arm, weight.clone())));
            } else if !WEIGHTS.contains(&w.as_str()) {
                rest.push(w.clone());
            }
        }
    }
    Drawn::Arms(set, rest)
}

/// Each Box Drawing character's counterpart, as its offset in the block;
/// `None` with the failures recorded.
fn box_counterparts(data: &Source, entries: &[Entry<'_>], errs: &mut Errors) -> Option<Vec<u8>> {
    let mut names: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for e in entries
        .iter()
        .filter(|e| (0x2500..=0x257F).contains(&e.range.lo))
    {
        let Some(name) = e.row.field(1).strip_prefix("BOX DRAWINGS ") else {
            errs.push(e.row.error(
                Kind::Invariant,
                1,
                "a Box Drawing name without BOX DRAWINGS",
            ));
            return None;
        };
        names.insert(
            e.range.lo,
            name.split_whitespace().map(String::from).collect(),
        );
    }
    errs.count(data.path(), "Box Drawing names", names.len(), 0x80);
    let mut by_drawn: HashMap<Drawn, u32> = HashMap::new();
    for (&cp, words) in &names {
        if let Some(earlier) = by_drawn.insert(drawn(words), cp) {
            errs.push(data.error(
                Kind::Duplicate,
                format!("U+{cp:04X} draws what U+{earlier:04X} draws"),
            ));
        }
    }
    let swap = |w: &String| match w.as_str() {
        "LEFT" => "RIGHT".to_string(),
        "RIGHT" => "LEFT".to_string(),
        _ => w.clone(),
    };
    let mut out = Vec::with_capacity(0x80);
    for (&cp, words) in &names {
        let swapped: Vec<String> = words.iter().map(swap).collect();
        let Some(&other) = by_drawn.get(&drawn(&swapped)) else {
            errs.push(data.error(Kind::Invariant, format!("U+{cp:04X} has no counterpart")));
            return None;
        };
        out.push(u8::try_from(other - 0x2500).expect("in the block"));
    }
    for (i, &o) in out.iter().enumerate() {
        let back = out.get(usize::from(o)).copied();
        errs.require(Check::from(back == u8::try_from(i).ok()), || {
            data.error(
                Kind::Invariant,
                format!(
                    "U+{:04X}'s counterpart's counterpart is not itself",
                    0x2500 + i
                ),
            )
        });
    }
    Some(out)
}
