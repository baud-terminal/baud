//! National Replacement Character Sets, generated from the sources pinned in
//! `docs/research/sources/MANIFEST.toml`: `[[source: xterm-charsets]]`'s
//! macros, their keysyms resolved through `[[source: xorg-keysymdef]]`, and
//! the positions those macros lack that a DEC text draws, each cited on its
//! table.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-nrcs-tables.py`;
//! `nrcs_tables_match_the_pinned_source` fails if this file and the pinned
//! source disagree.
//!
//! Each set replaces a handful of ASCII positions -- "the term *replacement*
//! says that the character set is formed by replacing some of the characters
//! with more useful ones for a given language". Everything not listed is ASCII.

use crate::{Codepoint, Nrcs};

/// DEC's error character, stored as U+2426 SYMBOL FOR SUBSTITUTE FORM TWO.
///
/// DEC names no codepoint for it: `[[source: dec-std-070]]` §3.4.3 has it
/// "usually imaged as a REVERSE QUESTION MARK", and `[[source: dec-pplv2-pm]]`
/// p. 6-4 calls it "a reverse question mark". U+2426 is ISO 2047's picture of
/// SUB, the control DEC ties it to, and its reference glyph is a reversed
/// question mark (`[[source: unicode18-chart-2400]]`). U+FFFD, one of the
/// three other options asked (with U+2E2E and U+2592), departs from that
/// description by name and by glyph (note AF, Q-UNDEF). Note AE's AE8.1,
/// decided by Jev at 1.00 twice, the second with the options reordered.
///
/// **It is drawn at a reserved position of a DEC set baud translates**,
/// DEC Supplemental's 13 and JIS-Katakana's 31 (`dec_sets_data.rs`): "The
/// error character is printed when one of these reserved characters is
/// received by the device" (`[[source: dec-pplv2-pm]]` §A.2, §A.12). Where
/// else it is drawn waits on open questions: A0 with a 94-set in GR (note AE,
/// AE8), SUB's display (`docs/vt-coverage.md`), and Greek NRC 7/9 and 7/10,
/// which no DEC table charts (note AF, Q-UNDEF).
pub const DEC_ERROR_CHARACTER: Codepoint = Codepoint(0x2426);

/// `[[source: xterm-charsets]]`'s `map_NRCS_Dutch`.
static DUTCH: [(u8, u32); 9] = [
    (0x23, 0x00a3),
    (0x40, 0x00be),
    (0x5b, 0x0133),
    (0x5c, 0x00bd),
    (0x5d, 0x007c),
    (0x7b, 0x00a8),
    (0x7c, 0x0192),
    (0x7d, 0x00bc),
    (0x7e, 0x00b4),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_Finnish`.
static FINNISH: [(u8, u32); 9] = [
    (0x5b, 0x00c4),
    (0x5c, 0x00d6),
    (0x5d, 0x00c5),
    (0x5e, 0x00dc),
    (0x60, 0x00e9),
    (0x7b, 0x00e4),
    (0x7c, 0x00f6),
    (0x7d, 0x00e5),
    (0x7e, 0x00fc),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_French`.
static FRENCH: [(u8, u32); 9] = [
    (0x23, 0x00a3),
    (0x40, 0x00e0),
    (0x5b, 0x00b0),
    (0x5c, 0x00e7),
    (0x5d, 0x00a7),
    (0x7b, 0x00e9),
    (0x7c, 0x00f9),
    (0x7d, 0x00e8),
    (0x7e, 0x00a8),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_French_Canadian`.
static FRENCHCANADIAN: [(u8, u32); 10] = [
    (0x40, 0x00e0),
    (0x5b, 0x00e2),
    (0x5c, 0x00e7),
    (0x5d, 0x00ea),
    (0x5e, 0x00ee),
    (0x60, 0x00f4),
    (0x7b, 0x00e9),
    (0x7c, 0x00f9),
    (0x7d, 0x00e8),
    (0x7e, 0x00fb),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_German`.
static GERMAN: [(u8, u32); 8] = [
    (0x40, 0x00a7),
    (0x5b, 0x00c4),
    (0x5c, 0x00d6),
    (0x5d, 0x00dc),
    (0x7b, 0x00e4),
    (0x7c, 0x00f6),
    (0x7d, 0x00fc),
    (0x7e, 0x00df),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_Italian`.
static ITALIAN: [(u8, u32); 10] = [
    (0x23, 0x00a3),
    (0x40, 0x00a7),
    (0x5b, 0x00b0),
    (0x5c, 0x00e7),
    (0x5d, 0x00e9),
    (0x60, 0x00f9),
    (0x7b, 0x00e0),
    (0x7c, 0x00f2),
    (0x7d, 0x00e8),
    (0x7e, 0x00ec),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_Norwegian_Danish`.
static NORWEGIANDANISH: [(u8, u32); 10] = [
    (0x40, 0x00c4),
    (0x5b, 0x00c6),
    (0x5c, 0x00d8),
    (0x5d, 0x00c5),
    (0x5e, 0x00dc),
    (0x60, 0x00e4),
    (0x7b, 0x00e6),
    (0x7c, 0x00f8),
    (0x7d, 0x00e5),
    (0x7e, 0x00fc),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_Portuguese`.
static PORTUGUESE: [(u8, u32); 6] = [
    (0x5b, 0x00c3),
    (0x5c, 0x00c7),
    (0x5d, 0x00d5),
    (0x7b, 0x00e3),
    (0x7c, 0x00e7),
    (0x7d, 0x00f5),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_Spanish`.
static SPANISH: [(u8, u32); 8] = [
    (0x23, 0x00a3),
    (0x40, 0x00a7),
    (0x5b, 0x00a1),
    (0x5c, 0x00d1),
    (0x5d, 0x00bf),
    (0x7b, 0x00b0),
    (0x7c, 0x00f1),
    (0x7d, 0x00e7),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_Swedish`.
static SWEDISH: [(u8, u32); 10] = [
    (0x40, 0x00c9),
    (0x5b, 0x00c4),
    (0x5c, 0x00d6),
    (0x5d, 0x00c5),
    (0x5e, 0x00dc),
    (0x60, 0x00e9),
    (0x7b, 0x00e4),
    (0x7c, 0x00f6),
    (0x7d, 0x00e5),
    (0x7e, 0x00fc),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_Swiss`.
static SWISS: [(u8, u32); 12] = [
    (0x23, 0x00f9),
    (0x40, 0x00e0),
    (0x5b, 0x00e9),
    (0x5c, 0x00e7),
    (0x5d, 0x00ea),
    (0x5e, 0x00ee),
    (0x5f, 0x00e8),
    (0x60, 0x00f4),
    (0x7b, 0x00e4),
    (0x7c, 0x00f6),
    (0x7d, 0x00fc),
    (0x7e, 0x00fb),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_Greek`.
///
/// Not emitted, each until its question is answered:
///
/// * `0x79`, `XXX(..., UNDEF)` in the macro: awaits Q-UNDEF (note AF).
/// * `0x7a`, `XXX(..., UNDEF)` in the macro: awaits Q-UNDEF (note AF).
static GREEK: [(u8, u32); 24] = [
    (0x61, 0x0391),
    (0x62, 0x0392),
    (0x63, 0x0393),
    (0x64, 0x0394),
    (0x65, 0x0395),
    (0x66, 0x0396),
    (0x67, 0x0397),
    (0x68, 0x0398),
    (0x69, 0x0399),
    (0x6a, 0x039a),
    (0x6b, 0x039b),
    (0x6c, 0x039c),
    (0x6d, 0x039d),
    (0x6e, 0x03a7),
    (0x6f, 0x039f),
    (0x70, 0x03a0),
    (0x71, 0x03a1),
    (0x72, 0x03a3),
    (0x73, 0x03a4),
    (0x74, 0x03a5),
    (0x75, 0x03a6),
    (0x76, 0x039e),
    (0x77, 0x03a8),
    (0x78, 0x03a9),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_Hebrew`.
static HEBREW: [(u8, u32); 27] = [
    (0x60, 0x05d0),
    (0x61, 0x05d1),
    (0x62, 0x05d2),
    (0x63, 0x05d3),
    (0x64, 0x05d4),
    (0x65, 0x05d5),
    (0x66, 0x05d6),
    (0x67, 0x05d7),
    (0x68, 0x05d8),
    (0x69, 0x05d9),
    (0x6a, 0x05da),
    (0x6b, 0x05db),
    (0x6c, 0x05dc),
    (0x6d, 0x05dd),
    (0x6e, 0x05de),
    (0x6f, 0x05df),
    (0x70, 0x05e0),
    (0x71, 0x05e1),
    (0x72, 0x05e2),
    (0x73, 0x05e3),
    (0x74, 0x05e4),
    (0x75, 0x05e5),
    (0x76, 0x05e6),
    (0x77, 0x05e7),
    (0x78, 0x05e8),
    (0x79, 0x05e9),
    (0x7a, 0x05ea),
];

/// `[[source: xterm-charsets]]`'s `map_NRCS_Turkish`, and what it lacks,
/// typed from the DEC text that draws it:
///
/// * `0x21`, U+0131: `[[source: dec-pplv2-pm]]` Figure A-26, which draws a
///   dotless small i at 2/1 (note AF §2.4).
static TURKISH: [(u8, u32); 12] = [
    (0x21, 0x0131),
    (0x26, 0x011f),
    (0x40, 0x0130),
    (0x5b, 0x015e),
    (0x5c, 0x00d6),
    (0x5d, 0x00c7),
    (0x5e, 0x00dc),
    (0x60, 0x011e),
    (0x7b, 0x015f),
    (0x7c, 0x00f6),
    (0x7d, 0x00e7),
    (0x7e, 0x00fc),
];

/// `ESC ( A` — United Kingdom, whose one substitution xterm spells inline
/// in `charsets.c` rather than as a macro.
static UK: [(u8, u32); 1] = [(0x23, 0x00a3)];

/// The replacement table for a set: the positions it replaces, by byte.
pub const fn nrcs_table(set: Nrcs) -> &'static [(u8, u32)] {
    match set {
        Nrcs::Dutch => &DUTCH,
        Nrcs::Finnish => &FINNISH,
        Nrcs::French => &FRENCH,
        Nrcs::FrenchCanadian => &FRENCHCANADIAN,
        Nrcs::German => &GERMAN,
        Nrcs::Italian => &ITALIAN,
        Nrcs::NorwegianDanish => &NORWEGIANDANISH,
        Nrcs::Portuguese => &PORTUGUESE,
        Nrcs::Spanish => &SPANISH,
        Nrcs::Swedish => &SWEDISH,
        Nrcs::Swiss => &SWISS,
        Nrcs::Greek => &GREEK,
        Nrcs::Hebrew => &HEBREW,
        Nrcs::Turkish => &TURKISH,
    }
}

/// United Kingdom, which is a `Charset` of its own rather than an `Nrcs`.
pub const fn uk_table() -> &'static [(u8, u32)] {
    &UK
}
