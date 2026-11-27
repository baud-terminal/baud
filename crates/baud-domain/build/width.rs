//! The width tables, `OUT_DIR/width_data.rs`: two two-stage tries, each
//! `codepoint >> 8` to a block of 256 two-bit values, four a byte, low bits
//! first -- 0 narrow, 1 zero, 2 wide, 3 ambiguous -- which `Width::of` and
//! `Width::of_unicode8` read.
//!
//! * **zero** is General_Category Mn, Me or Cf (`[[source: gc-data]]`):
//!   combining marks and format characters, which take no column. U+00AD
//!   SOFT HYPHEN is Cf and is left out, because every terminal gives it a
//!   column; so is Prepended_Concatenation_Mark (`[[source: unicode-proplist-17]]`),
//!   UAX #44's "visible format controls, which precede and then span a
//!   sequence of other characters, usually digits" (`[[source: uax44]]`), a
//!   column each, as glibc gives them.
//! * **wide** is East_Asian_Width W or F (`[[source: eaw-data]]`), or
//!   Emoji_Presentation (`[[source: emoji-data]]`). Neither subsumes the
//!   other: a regional indicator is EAW N, so a flag measured from East
//!   Asian Width alone is half the width every terminal draws it.
//! * **ambiguous** is East_Asian_Width A, a column or two by locale, which
//!   `AmbiguousWidth` selects (`[[source: uax11]]` section 5).
//!
//! The three overlap, and the order is the rule: zero over wide over
//! ambiguous ([`width`]), so U+0300, Mn and EAW A, is zero, and U+302A, Mn
//! and EAW W, is zero too.
//!
//! **WIDTH8** is the same four values under Unicode 8.0's East_Asian_Width
//! (`[[source: eaw-data-8]]`), which iTerm2's `UnicodeVersion=8` selects:
//! no Emoji_Presentation, which Unicode 9 folded into Wide, so a regional
//! indicator is one column; and its zero set keeps the Cf
//! Prepended_Concatenation_Mark leaves out, the zero set Unicode 8 had no
//! property to narrow.
//!
//! Each file's header gives the unassigned code points of some blocks the
//! default W; [`WIDE_17`] and [`WIDE_8`] are those ranges, each checked
//! against its header's text. 17.0.0's file lists every one of them W as
//! well, so there they change nothing.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use crate::ucd::{self, EA, GC, at, index};
use baud_tablegen::{CODE_POINTS, Check, CpRange, Emit, Errors, Input, Kind, Source, Trie};

pub const EAST_ASIAN_WIDTH_8: Input = Input {
    file: "unicode-eastasianwidth-8.0.0.txt",
    key: "eaw-data-8",
    header: "# EastAsianWidth-8.0.0.txt",
};

pub const PROP_LIST: Input = Input {
    file: "unicode-proplist.txt",
    key: "unicode-proplist-17",
    header: "# PropList-17.0.0.txt",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 6] = [
    ucd::ALIASES,
    ucd::EAST_ASIAN_WIDTH,
    EAST_ASIAN_WIDTH_8,
    ucd::GENERAL_CATEGORY,
    PROP_LIST,
    ucd::EMOJI,
];

/// EastAsianWidth-17.0.0.txt's header: "The unassigned code points in the
/// following blocks default to "W"", and "All undesignated code points in
/// Planes 2 and 3, whether inside or outside of allocated blocks, default
/// to "W"".
pub const WIDE_17: [CpRange; 5] = [
    CpRange {
        lo: 0x3400,
        hi: 0x4DBF,
    },
    CpRange {
        lo: 0x4E00,
        hi: 0x9FFF,
    },
    CpRange {
        lo: 0xF900,
        hi: 0xFAFF,
    },
    CpRange {
        lo: 0x20000,
        hi: 0x2FFFD,
    },
    CpRange {
        lo: 0x30000,
        hi: 0x3FFFD,
    },
];

/// EastAsianWidth-8.0.0.txt's header: "The unassigned code points that
/// default to "W" include ranges in the following blocks", of which the
/// Extension B to E and Supplement blocks lie inside "Planes 2 and 3".
pub const WIDE_8: [CpRange; 5] = WIDE_17;

/// Prepended_Concatenation_Mark's rows in PropList-17.0.0.txt.
const PCM_ROWS: usize = 7;

const NARROW: u8 = 0;
const ZERO: u8 = 1;
const WIDE: u8 = 2;
const AMBIGUOUS: u8 = 3;

/// The width a code point takes from its three memberships: zero over wide
/// over ambiguous, and narrow when it has none.
pub fn width(zero: bool, wide: bool, ambiguous: bool) -> u8 {
    if zero {
        ZERO
    } else if wide {
        WIDE
    } else if ambiguous {
        AMBIGUOUS
    } else {
        NARROW
    }
}

/// The value at `cp` of a packed trie, read the way `Width::of` reads it.
pub fn packed_get(index: &[u8], packed: &[u8], cp: usize) -> u8 {
    let block = usize::from(index[cp >> 8]);
    let byte = packed[block * 64 + ((cp & 0xFF) >> 2)];
    (byte >> ((cp & 3) * 2)) & 3
}

/// `values` as a trie of 256-value blocks, each packed four a byte, and
/// checked at every code point through the packed read.
fn packed_trie(name: &str, values: &[u8], errs: &mut Errors) -> Option<(Vec<u8>, Vec<u8>, usize)> {
    let trie = errs.take(Trie::build(name, values, 8))?;
    let packed: Vec<u8> = trie
        .blocks
        .chunks(4)
        .map(|q| {
            q.iter()
                .enumerate()
                .fold(0, |b, (j, &v)| b | (v << (2 * j)))
        })
        .collect();
    if let Some(cp) =
        (0..CODE_POINTS).find(|&cp| packed_get(&trie.index, &packed, cp) != values[cp])
    {
        errs.push(baud_tablegen::Error::new(
            Kind::Invariant,
            name,
            format!("the packed trie disagrees with its values at U+{cp:04X}"),
        ));
        return None;
    }
    Some((trie.index, packed, trie.distinct))
}

/// The tables, or every failure found reading the inputs `read` returns by
/// their names under `data/`.
pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    let Ok([aliases, eaw, eaw8, dgc, props, emoji]) = <[Source; 6]>::try_from(loaded) else {
        return Err(errs);
    };
    ucd::emoji_version(&emoji, &mut errs);
    let (Some(ea), Some(gc)) = (
        ucd::values(&aliases, "ea", &EA, &mut errs),
        ucd::values(&aliases, "gc", &GC, &mut errs),
    ) else {
        return Err(errs);
    };
    let ea17 = ucd::east_asian_width(&eaw, &ea, &WIDE_17, &mut errs);
    let ea8 = ucd::east_asian_width(&eaw8, &ea, &WIDE_8, &mut errs);
    let cat = ucd::general_category(&dgc, &gc, &mut errs);
    let (pcm, pcm_rows) = ucd::binary(&props, "Prepended_Concatenation_Mark", None, &mut errs);
    errs.count(
        props.path(),
        "Prepended_Concatenation_Mark rows",
        pcm_rows,
        PCM_ROWS,
    );
    let names = ucd::emoji_properties();
    let (presentation, _) = ucd::binary(&emoji, "Emoji_Presentation", Some(&names), &mut errs);
    let (Some(ea17), Some(ea8), Some(cat)) = (ea17, ea8, cat) else {
        return Err(errs);
    };

    let marks = [index(&GC, "Mn"), index(&GC, "Me"), index(&GC, "Cf")];
    let (w, f, a) = (index(&EA, "W"), index(&EA, "F"), index(&EA, "A"));
    let mut current = vec![0u8; CODE_POINTS];
    let mut unicode8 = vec![0u8; CODE_POINTS];
    for cp in 0..CODE_POINTS {
        // U+00AD is Cf, and takes a column.
        let zero8 = marks.contains(&cat[cp]) && cp != 0xAD;
        let zero = zero8 && !pcm[cp];
        current[cp] = width(
            zero,
            ea17[cp] == w || ea17[cp] == f || presentation[cp],
            ea17[cp] == a,
        );
        unicode8[cp] = width(zero8, ea8[cp] == w || ea8[cp] == f, ea8[cp] == a);
    }
    // Positive controls: one of each value, both exclusions, and a flag's
    // half under each version.
    for (cp, want, want8, what) in [
        (0x0041, NARROW, NARROW, "LATIN CAPITAL LETTER A"),
        (0x4E00, WIDE, WIDE, "a CJK ideograph"),
        (0x0300, ZERO, ZERO, "a combining mark, EAW A"),
        (0x302A, ZERO, ZERO, "a combining mark, EAW W"),
        (0x00A1, AMBIGUOUS, AMBIGUOUS, "INVERTED EXCLAMATION MARK"),
        (0x00AD, AMBIGUOUS, AMBIGUOUS, "SOFT HYPHEN, Cf and EAW A"),
        (0x0600, NARROW, ZERO, "ARABIC NUMBER SIGN, a PCM"),
        (0x1F1E6, WIDE, NARROW, "a regional indicator"),
        (0x3FFFD, WIDE, WIDE, "the last plane 3 default"),
    ] {
        let got = (current[at(cp)], unicode8[at(cp)]);
        errs.require(Check::from(got == (want, want8)), || {
            eaw.error(
                Kind::Control,
                format!("U+{cp:04X} ({what}) is {got:?}, ({want}, {want8}) expected"),
            )
        });
    }
    let t17 = packed_trie("WIDTH", &current, &mut errs);
    let t8 = packed_trie("WIDTH8", &unicode8, &mut errs);
    let (Some((index17, packed17, n17)), Some((index8, packed8, n8))) = (t17, t8) else {
        return Err(errs);
    };

    let mut out = Emit::new(
        "width_data",
        "crates/baud-domain/build.rs (build/width.rs)",
        &[&eaw, &eaw8, &dgc, &props, &emoji, &aliases],
    );
    out.bytes(
        "WIDTH_INDEX",
        &[
            "Stage 1 of the width trie: codepoint >> 8 to a block number.",
            "",
            "Values: 0 narrow, 1 zero (gc Mn, Me or Cf but U+00AD and",
            "Prepended_Concatenation_Mark), 2 wide (East_Asian_Width W or F, or",
            "Emoji_Presentation), 3 ambiguous (East_Asian_Width A); zero over wide",
            "over ambiguous.",
        ],
        &index17,
    );
    out.bytes(
        "WIDTH_BLOCKS",
        &[
            &format!("Stage 2: {n17} distinct 256-codepoint blocks, four 2-bit"),
            "values per byte, low bits first.",
        ],
        &packed17,
    );
    out.bytes(
        "WIDTH8_INDEX",
        &[
            "Stage 1 of the **Unicode 8** width trie, which iTerm2's",
            "`UnicodeVersion=8` selects: the four values as [`WIDTH_INDEX`]'s,",
            "wide from East Asian Width 8.0's `W` and `F` and the unassigned",
            "code points its header defaults to `W`, with no",
            "`Emoji_Presentation`, so a regional indicator is one column.",
        ],
        &index8,
    );
    out.bytes(
        "WIDTH8_BLOCKS",
        &[&format!(
            "Stage 2: {n8} distinct 256-codepoint blocks, as [`WIDTH_BLOCKS`]."
        )],
        &packed8,
    );
    errs.or(out)
}
