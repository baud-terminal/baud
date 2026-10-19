//! The six 96-character sets `ESC - . /` designate, generated from the
//! sources pinned in `docs/research/sources/MANIFEST.toml`.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-iso96-sets.py`;
//! `iso96_tables_match_the_pinned_sources` fails if this file and the
//! pinned sources disagree.
//!
//! Each table is indexed by `code - 0x20`, the 7-bit code, 2/0 through
//! 7/15: a 96-character set has a character at SP's and DEL's positions
//! too (`[[source: ecma35]]` 8.3.1). An unassigned position holds DEC's
//! error character.

use crate::{Codepoint, DEC_ERROR_CHARACTER};

/// DEC's error character, at an unassigned position.
const E: Codepoint = DEC_ERROR_CHARACTER;

/// ISO Latin-1 Supplemental, final `A`, ISO-IR 100: `[[source: iso8859-1]]`'s right half.
pub(crate) static LATIN_1: [Codepoint; 96] = [
    Codepoint(0x00a0), // 0x20 NO-BREAK SPACE
    Codepoint(0x00a1), // 0x21 INVERTED EXCLAMATION MARK
    Codepoint(0x00a2), // 0x22 CENT SIGN
    Codepoint(0x00a3), // 0x23 POUND SIGN
    Codepoint(0x00a4), // 0x24 CURRENCY SIGN
    Codepoint(0x00a5), // 0x25 YEN SIGN
    Codepoint(0x00a6), // 0x26 BROKEN BAR
    Codepoint(0x00a7), // 0x27 SECTION SIGN
    Codepoint(0x00a8), // 0x28 DIAERESIS
    Codepoint(0x00a9), // 0x29 COPYRIGHT SIGN
    Codepoint(0x00aa), // 0x2a FEMININE ORDINAL INDICATOR
    Codepoint(0x00ab), // 0x2b LEFT-POINTING DOUBLE ANGLE QUOTATION MARK
    Codepoint(0x00ac), // 0x2c NOT SIGN
    Codepoint(0x00ad), // 0x2d SOFT HYPHEN
    Codepoint(0x00ae), // 0x2e REGISTERED SIGN
    Codepoint(0x00af), // 0x2f MACRON
    Codepoint(0x00b0), // 0x30 DEGREE SIGN
    Codepoint(0x00b1), // 0x31 PLUS-MINUS SIGN
    Codepoint(0x00b2), // 0x32 SUPERSCRIPT TWO
    Codepoint(0x00b3), // 0x33 SUPERSCRIPT THREE
    Codepoint(0x00b4), // 0x34 ACUTE ACCENT
    Codepoint(0x00b5), // 0x35 MICRO SIGN
    Codepoint(0x00b6), // 0x36 PILCROW SIGN
    Codepoint(0x00b7), // 0x37 MIDDLE DOT
    Codepoint(0x00b8), // 0x38 CEDILLA
    Codepoint(0x00b9), // 0x39 SUPERSCRIPT ONE
    Codepoint(0x00ba), // 0x3a MASCULINE ORDINAL INDICATOR
    Codepoint(0x00bb), // 0x3b RIGHT-POINTING DOUBLE ANGLE QUOTATION MARK
    Codepoint(0x00bc), // 0x3c VULGAR FRACTION ONE QUARTER
    Codepoint(0x00bd), // 0x3d VULGAR FRACTION ONE HALF
    Codepoint(0x00be), // 0x3e VULGAR FRACTION THREE QUARTERS
    Codepoint(0x00bf), // 0x3f INVERTED QUESTION MARK
    Codepoint(0x00c0), // 0x40 LATIN CAPITAL LETTER A WITH GRAVE
    Codepoint(0x00c1), // 0x41 LATIN CAPITAL LETTER A WITH ACUTE
    Codepoint(0x00c2), // 0x42 LATIN CAPITAL LETTER A WITH CIRCUMFLEX
    Codepoint(0x00c3), // 0x43 LATIN CAPITAL LETTER A WITH TILDE
    Codepoint(0x00c4), // 0x44 LATIN CAPITAL LETTER A WITH DIAERESIS
    Codepoint(0x00c5), // 0x45 LATIN CAPITAL LETTER A WITH RING ABOVE
    Codepoint(0x00c6), // 0x46 LATIN CAPITAL LETTER AE
    Codepoint(0x00c7), // 0x47 LATIN CAPITAL LETTER C WITH CEDILLA
    Codepoint(0x00c8), // 0x48 LATIN CAPITAL LETTER E WITH GRAVE
    Codepoint(0x00c9), // 0x49 LATIN CAPITAL LETTER E WITH ACUTE
    Codepoint(0x00ca), // 0x4a LATIN CAPITAL LETTER E WITH CIRCUMFLEX
    Codepoint(0x00cb), // 0x4b LATIN CAPITAL LETTER E WITH DIAERESIS
    Codepoint(0x00cc), // 0x4c LATIN CAPITAL LETTER I WITH GRAVE
    Codepoint(0x00cd), // 0x4d LATIN CAPITAL LETTER I WITH ACUTE
    Codepoint(0x00ce), // 0x4e LATIN CAPITAL LETTER I WITH CIRCUMFLEX
    Codepoint(0x00cf), // 0x4f LATIN CAPITAL LETTER I WITH DIAERESIS
    Codepoint(0x00d0), // 0x50 LATIN CAPITAL LETTER ETH (Icelandic)
    Codepoint(0x00d1), // 0x51 LATIN CAPITAL LETTER N WITH TILDE
    Codepoint(0x00d2), // 0x52 LATIN CAPITAL LETTER O WITH GRAVE
    Codepoint(0x00d3), // 0x53 LATIN CAPITAL LETTER O WITH ACUTE
    Codepoint(0x00d4), // 0x54 LATIN CAPITAL LETTER O WITH CIRCUMFLEX
    Codepoint(0x00d5), // 0x55 LATIN CAPITAL LETTER O WITH TILDE
    Codepoint(0x00d6), // 0x56 LATIN CAPITAL LETTER O WITH DIAERESIS
    Codepoint(0x00d7), // 0x57 MULTIPLICATION SIGN
    Codepoint(0x00d8), // 0x58 LATIN CAPITAL LETTER O WITH STROKE
    Codepoint(0x00d9), // 0x59 LATIN CAPITAL LETTER U WITH GRAVE
    Codepoint(0x00da), // 0x5a LATIN CAPITAL LETTER U WITH ACUTE
    Codepoint(0x00db), // 0x5b LATIN CAPITAL LETTER U WITH CIRCUMFLEX
    Codepoint(0x00dc), // 0x5c LATIN CAPITAL LETTER U WITH DIAERESIS
    Codepoint(0x00dd), // 0x5d LATIN CAPITAL LETTER Y WITH ACUTE
    Codepoint(0x00de), // 0x5e LATIN CAPITAL LETTER THORN (Icelandic)
    Codepoint(0x00df), // 0x5f LATIN SMALL LETTER SHARP S (German)
    Codepoint(0x00e0), // 0x60 LATIN SMALL LETTER A WITH GRAVE
    Codepoint(0x00e1), // 0x61 LATIN SMALL LETTER A WITH ACUTE
    Codepoint(0x00e2), // 0x62 LATIN SMALL LETTER A WITH CIRCUMFLEX
    Codepoint(0x00e3), // 0x63 LATIN SMALL LETTER A WITH TILDE
    Codepoint(0x00e4), // 0x64 LATIN SMALL LETTER A WITH DIAERESIS
    Codepoint(0x00e5), // 0x65 LATIN SMALL LETTER A WITH RING ABOVE
    Codepoint(0x00e6), // 0x66 LATIN SMALL LETTER AE
    Codepoint(0x00e7), // 0x67 LATIN SMALL LETTER C WITH CEDILLA
    Codepoint(0x00e8), // 0x68 LATIN SMALL LETTER E WITH GRAVE
    Codepoint(0x00e9), // 0x69 LATIN SMALL LETTER E WITH ACUTE
    Codepoint(0x00ea), // 0x6a LATIN SMALL LETTER E WITH CIRCUMFLEX
    Codepoint(0x00eb), // 0x6b LATIN SMALL LETTER E WITH DIAERESIS
    Codepoint(0x00ec), // 0x6c LATIN SMALL LETTER I WITH GRAVE
    Codepoint(0x00ed), // 0x6d LATIN SMALL LETTER I WITH ACUTE
    Codepoint(0x00ee), // 0x6e LATIN SMALL LETTER I WITH CIRCUMFLEX
    Codepoint(0x00ef), // 0x6f LATIN SMALL LETTER I WITH DIAERESIS
    Codepoint(0x00f0), // 0x70 LATIN SMALL LETTER ETH (Icelandic)
    Codepoint(0x00f1), // 0x71 LATIN SMALL LETTER N WITH TILDE
    Codepoint(0x00f2), // 0x72 LATIN SMALL LETTER O WITH GRAVE
    Codepoint(0x00f3), // 0x73 LATIN SMALL LETTER O WITH ACUTE
    Codepoint(0x00f4), // 0x74 LATIN SMALL LETTER O WITH CIRCUMFLEX
    Codepoint(0x00f5), // 0x75 LATIN SMALL LETTER O WITH TILDE
    Codepoint(0x00f6), // 0x76 LATIN SMALL LETTER O WITH DIAERESIS
    Codepoint(0x00f7), // 0x77 DIVISION SIGN
    Codepoint(0x00f8), // 0x78 LATIN SMALL LETTER O WITH STROKE
    Codepoint(0x00f9), // 0x79 LATIN SMALL LETTER U WITH GRAVE
    Codepoint(0x00fa), // 0x7a LATIN SMALL LETTER U WITH ACUTE
    Codepoint(0x00fb), // 0x7b LATIN SMALL LETTER U WITH CIRCUMFLEX
    Codepoint(0x00fc), // 0x7c LATIN SMALL LETTER U WITH DIAERESIS
    Codepoint(0x00fd), // 0x7d LATIN SMALL LETTER Y WITH ACUTE
    Codepoint(0x00fe), // 0x7e LATIN SMALL LETTER THORN (Icelandic)
    Codepoint(0x00ff), // 0x7f LATIN SMALL LETTER Y WITH DIAERESIS
];

/// ISO Latin-2 Supplemental, final `B`, ISO-IR 101: `[[source: iso8859-2]]`'s right half.
pub(crate) static LATIN_2: [Codepoint; 96] = [
    Codepoint(0x00a0), // 0x20 NO-BREAK SPACE
    Codepoint(0x0104), // 0x21 LATIN CAPITAL LETTER A WITH OGONEK
    Codepoint(0x02d8), // 0x22 BREVE
    Codepoint(0x0141), // 0x23 LATIN CAPITAL LETTER L WITH STROKE
    Codepoint(0x00a4), // 0x24 CURRENCY SIGN
    Codepoint(0x013d), // 0x25 LATIN CAPITAL LETTER L WITH CARON
    Codepoint(0x015a), // 0x26 LATIN CAPITAL LETTER S WITH ACUTE
    Codepoint(0x00a7), // 0x27 SECTION SIGN
    Codepoint(0x00a8), // 0x28 DIAERESIS
    Codepoint(0x0160), // 0x29 LATIN CAPITAL LETTER S WITH CARON
    Codepoint(0x015e), // 0x2a LATIN CAPITAL LETTER S WITH CEDILLA
    Codepoint(0x0164), // 0x2b LATIN CAPITAL LETTER T WITH CARON
    Codepoint(0x0179), // 0x2c LATIN CAPITAL LETTER Z WITH ACUTE
    Codepoint(0x00ad), // 0x2d SOFT HYPHEN
    Codepoint(0x017d), // 0x2e LATIN CAPITAL LETTER Z WITH CARON
    Codepoint(0x017b), // 0x2f LATIN CAPITAL LETTER Z WITH DOT ABOVE
    Codepoint(0x00b0), // 0x30 DEGREE SIGN
    Codepoint(0x0105), // 0x31 LATIN SMALL LETTER A WITH OGONEK
    Codepoint(0x02db), // 0x32 OGONEK
    Codepoint(0x0142), // 0x33 LATIN SMALL LETTER L WITH STROKE
    Codepoint(0x00b4), // 0x34 ACUTE ACCENT
    Codepoint(0x013e), // 0x35 LATIN SMALL LETTER L WITH CARON
    Codepoint(0x015b), // 0x36 LATIN SMALL LETTER S WITH ACUTE
    Codepoint(0x02c7), // 0x37 CARON
    Codepoint(0x00b8), // 0x38 CEDILLA
    Codepoint(0x0161), // 0x39 LATIN SMALL LETTER S WITH CARON
    Codepoint(0x015f), // 0x3a LATIN SMALL LETTER S WITH CEDILLA
    Codepoint(0x0165), // 0x3b LATIN SMALL LETTER T WITH CARON
    Codepoint(0x017a), // 0x3c LATIN SMALL LETTER Z WITH ACUTE
    Codepoint(0x02dd), // 0x3d DOUBLE ACUTE ACCENT
    Codepoint(0x017e), // 0x3e LATIN SMALL LETTER Z WITH CARON
    Codepoint(0x017c), // 0x3f LATIN SMALL LETTER Z WITH DOT ABOVE
    Codepoint(0x0154), // 0x40 LATIN CAPITAL LETTER R WITH ACUTE
    Codepoint(0x00c1), // 0x41 LATIN CAPITAL LETTER A WITH ACUTE
    Codepoint(0x00c2), // 0x42 LATIN CAPITAL LETTER A WITH CIRCUMFLEX
    Codepoint(0x0102), // 0x43 LATIN CAPITAL LETTER A WITH BREVE
    Codepoint(0x00c4), // 0x44 LATIN CAPITAL LETTER A WITH DIAERESIS
    Codepoint(0x0139), // 0x45 LATIN CAPITAL LETTER L WITH ACUTE
    Codepoint(0x0106), // 0x46 LATIN CAPITAL LETTER C WITH ACUTE
    Codepoint(0x00c7), // 0x47 LATIN CAPITAL LETTER C WITH CEDILLA
    Codepoint(0x010c), // 0x48 LATIN CAPITAL LETTER C WITH CARON
    Codepoint(0x00c9), // 0x49 LATIN CAPITAL LETTER E WITH ACUTE
    Codepoint(0x0118), // 0x4a LATIN CAPITAL LETTER E WITH OGONEK
    Codepoint(0x00cb), // 0x4b LATIN CAPITAL LETTER E WITH DIAERESIS
    Codepoint(0x011a), // 0x4c LATIN CAPITAL LETTER E WITH CARON
    Codepoint(0x00cd), // 0x4d LATIN CAPITAL LETTER I WITH ACUTE
    Codepoint(0x00ce), // 0x4e LATIN CAPITAL LETTER I WITH CIRCUMFLEX
    Codepoint(0x010e), // 0x4f LATIN CAPITAL LETTER D WITH CARON
    Codepoint(0x0110), // 0x50 LATIN CAPITAL LETTER D WITH STROKE
    Codepoint(0x0143), // 0x51 LATIN CAPITAL LETTER N WITH ACUTE
    Codepoint(0x0147), // 0x52 LATIN CAPITAL LETTER N WITH CARON
    Codepoint(0x00d3), // 0x53 LATIN CAPITAL LETTER O WITH ACUTE
    Codepoint(0x00d4), // 0x54 LATIN CAPITAL LETTER O WITH CIRCUMFLEX
    Codepoint(0x0150), // 0x55 LATIN CAPITAL LETTER O WITH DOUBLE ACUTE
    Codepoint(0x00d6), // 0x56 LATIN CAPITAL LETTER O WITH DIAERESIS
    Codepoint(0x00d7), // 0x57 MULTIPLICATION SIGN
    Codepoint(0x0158), // 0x58 LATIN CAPITAL LETTER R WITH CARON
    Codepoint(0x016e), // 0x59 LATIN CAPITAL LETTER U WITH RING ABOVE
    Codepoint(0x00da), // 0x5a LATIN CAPITAL LETTER U WITH ACUTE
    Codepoint(0x0170), // 0x5b LATIN CAPITAL LETTER U WITH DOUBLE ACUTE
    Codepoint(0x00dc), // 0x5c LATIN CAPITAL LETTER U WITH DIAERESIS
    Codepoint(0x00dd), // 0x5d LATIN CAPITAL LETTER Y WITH ACUTE
    Codepoint(0x0162), // 0x5e LATIN CAPITAL LETTER T WITH CEDILLA
    Codepoint(0x00df), // 0x5f LATIN SMALL LETTER SHARP S
    Codepoint(0x0155), // 0x60 LATIN SMALL LETTER R WITH ACUTE
    Codepoint(0x00e1), // 0x61 LATIN SMALL LETTER A WITH ACUTE
    Codepoint(0x00e2), // 0x62 LATIN SMALL LETTER A WITH CIRCUMFLEX
    Codepoint(0x0103), // 0x63 LATIN SMALL LETTER A WITH BREVE
    Codepoint(0x00e4), // 0x64 LATIN SMALL LETTER A WITH DIAERESIS
    Codepoint(0x013a), // 0x65 LATIN SMALL LETTER L WITH ACUTE
    Codepoint(0x0107), // 0x66 LATIN SMALL LETTER C WITH ACUTE
    Codepoint(0x00e7), // 0x67 LATIN SMALL LETTER C WITH CEDILLA
    Codepoint(0x010d), // 0x68 LATIN SMALL LETTER C WITH CARON
    Codepoint(0x00e9), // 0x69 LATIN SMALL LETTER E WITH ACUTE
    Codepoint(0x0119), // 0x6a LATIN SMALL LETTER E WITH OGONEK
    Codepoint(0x00eb), // 0x6b LATIN SMALL LETTER E WITH DIAERESIS
    Codepoint(0x011b), // 0x6c LATIN SMALL LETTER E WITH CARON
    Codepoint(0x00ed), // 0x6d LATIN SMALL LETTER I WITH ACUTE
    Codepoint(0x00ee), // 0x6e LATIN SMALL LETTER I WITH CIRCUMFLEX
    Codepoint(0x010f), // 0x6f LATIN SMALL LETTER D WITH CARON
    Codepoint(0x0111), // 0x70 LATIN SMALL LETTER D WITH STROKE
    Codepoint(0x0144), // 0x71 LATIN SMALL LETTER N WITH ACUTE
    Codepoint(0x0148), // 0x72 LATIN SMALL LETTER N WITH CARON
    Codepoint(0x00f3), // 0x73 LATIN SMALL LETTER O WITH ACUTE
    Codepoint(0x00f4), // 0x74 LATIN SMALL LETTER O WITH CIRCUMFLEX
    Codepoint(0x0151), // 0x75 LATIN SMALL LETTER O WITH DOUBLE ACUTE
    Codepoint(0x00f6), // 0x76 LATIN SMALL LETTER O WITH DIAERESIS
    Codepoint(0x00f7), // 0x77 DIVISION SIGN
    Codepoint(0x0159), // 0x78 LATIN SMALL LETTER R WITH CARON
    Codepoint(0x016f), // 0x79 LATIN SMALL LETTER U WITH RING ABOVE
    Codepoint(0x00fa), // 0x7a LATIN SMALL LETTER U WITH ACUTE
    Codepoint(0x0171), // 0x7b LATIN SMALL LETTER U WITH DOUBLE ACUTE
    Codepoint(0x00fc), // 0x7c LATIN SMALL LETTER U WITH DIAERESIS
    Codepoint(0x00fd), // 0x7d LATIN SMALL LETTER Y WITH ACUTE
    Codepoint(0x0163), // 0x7e LATIN SMALL LETTER T WITH CEDILLA
    Codepoint(0x02d9), // 0x7f DOT ABOVE
];

/// ISO Greek Supplemental, final `F`, ISO-IR 126: `[[source: iso8859-7]]`'s right half.
///
/// Unassigned here and mapped by the file, which maps a later edition: `0xa4`, `0xa5`, `0xaa`.
pub(crate) static GREEK: [Codepoint; 96] = [
    Codepoint(0x00a0), // 0x20 NO-BREAK SPACE
    Codepoint(0x2018), // 0x21 LEFT SINGLE QUOTATION MARK
    Codepoint(0x2019), // 0x22 RIGHT SINGLE QUOTATION MARK
    Codepoint(0x00a3), // 0x23 POUND SIGN
    E,                 // 0x24 unassigned
    E,                 // 0x25 unassigned
    Codepoint(0x00a6), // 0x26 BROKEN BAR
    Codepoint(0x00a7), // 0x27 SECTION SIGN
    Codepoint(0x00a8), // 0x28 DIAERESIS
    Codepoint(0x00a9), // 0x29 COPYRIGHT SIGN
    E,                 // 0x2a unassigned
    Codepoint(0x00ab), // 0x2b LEFT-POINTING DOUBLE ANGLE QUOTATION MARK
    Codepoint(0x00ac), // 0x2c NOT SIGN
    Codepoint(0x00ad), // 0x2d SOFT HYPHEN
    E,                 // 0x2e unassigned
    Codepoint(0x2015), // 0x2f HORIZONTAL BAR
    Codepoint(0x00b0), // 0x30 DEGREE SIGN
    Codepoint(0x00b1), // 0x31 PLUS-MINUS SIGN
    Codepoint(0x00b2), // 0x32 SUPERSCRIPT TWO
    Codepoint(0x00b3), // 0x33 SUPERSCRIPT THREE
    Codepoint(0x0384), // 0x34 GREEK TONOS
    Codepoint(0x0385), // 0x35 GREEK DIALYTIKA TONOS
    Codepoint(0x0386), // 0x36 GREEK CAPITAL LETTER ALPHA WITH TONOS
    Codepoint(0x00b7), // 0x37 MIDDLE DOT
    Codepoint(0x0388), // 0x38 GREEK CAPITAL LETTER EPSILON WITH TONOS
    Codepoint(0x0389), // 0x39 GREEK CAPITAL LETTER ETA WITH TONOS
    Codepoint(0x038a), // 0x3a GREEK CAPITAL LETTER IOTA WITH TONOS
    Codepoint(0x00bb), // 0x3b RIGHT-POINTING DOUBLE ANGLE QUOTATION MARK
    Codepoint(0x038c), // 0x3c GREEK CAPITAL LETTER OMICRON WITH TONOS
    Codepoint(0x00bd), // 0x3d VULGAR FRACTION ONE HALF
    Codepoint(0x038e), // 0x3e GREEK CAPITAL LETTER UPSILON WITH TONOS
    Codepoint(0x038f), // 0x3f GREEK CAPITAL LETTER OMEGA WITH TONOS
    Codepoint(0x0390), // 0x40 GREEK SMALL LETTER IOTA WITH DIALYTIKA AND TONOS
    Codepoint(0x0391), // 0x41 GREEK CAPITAL LETTER ALPHA
    Codepoint(0x0392), // 0x42 GREEK CAPITAL LETTER BETA
    Codepoint(0x0393), // 0x43 GREEK CAPITAL LETTER GAMMA
    Codepoint(0x0394), // 0x44 GREEK CAPITAL LETTER DELTA
    Codepoint(0x0395), // 0x45 GREEK CAPITAL LETTER EPSILON
    Codepoint(0x0396), // 0x46 GREEK CAPITAL LETTER ZETA
    Codepoint(0x0397), // 0x47 GREEK CAPITAL LETTER ETA
    Codepoint(0x0398), // 0x48 GREEK CAPITAL LETTER THETA
    Codepoint(0x0399), // 0x49 GREEK CAPITAL LETTER IOTA
    Codepoint(0x039a), // 0x4a GREEK CAPITAL LETTER KAPPA
    Codepoint(0x039b), // 0x4b GREEK CAPITAL LETTER LAMDA
    Codepoint(0x039c), // 0x4c GREEK CAPITAL LETTER MU
    Codepoint(0x039d), // 0x4d GREEK CAPITAL LETTER NU
    Codepoint(0x039e), // 0x4e GREEK CAPITAL LETTER XI
    Codepoint(0x039f), // 0x4f GREEK CAPITAL LETTER OMICRON
    Codepoint(0x03a0), // 0x50 GREEK CAPITAL LETTER PI
    Codepoint(0x03a1), // 0x51 GREEK CAPITAL LETTER RHO
    E,                 // 0x52 unassigned
    Codepoint(0x03a3), // 0x53 GREEK CAPITAL LETTER SIGMA
    Codepoint(0x03a4), // 0x54 GREEK CAPITAL LETTER TAU
    Codepoint(0x03a5), // 0x55 GREEK CAPITAL LETTER UPSILON
    Codepoint(0x03a6), // 0x56 GREEK CAPITAL LETTER PHI
    Codepoint(0x03a7), // 0x57 GREEK CAPITAL LETTER CHI
    Codepoint(0x03a8), // 0x58 GREEK CAPITAL LETTER PSI
    Codepoint(0x03a9), // 0x59 GREEK CAPITAL LETTER OMEGA
    Codepoint(0x03aa), // 0x5a GREEK CAPITAL LETTER IOTA WITH DIALYTIKA
    Codepoint(0x03ab), // 0x5b GREEK CAPITAL LETTER UPSILON WITH DIALYTIKA
    Codepoint(0x03ac), // 0x5c GREEK SMALL LETTER ALPHA WITH TONOS
    Codepoint(0x03ad), // 0x5d GREEK SMALL LETTER EPSILON WITH TONOS
    Codepoint(0x03ae), // 0x5e GREEK SMALL LETTER ETA WITH TONOS
    Codepoint(0x03af), // 0x5f GREEK SMALL LETTER IOTA WITH TONOS
    Codepoint(0x03b0), // 0x60 GREEK SMALL LETTER UPSILON WITH DIALYTIKA AND TONOS
    Codepoint(0x03b1), // 0x61 GREEK SMALL LETTER ALPHA
    Codepoint(0x03b2), // 0x62 GREEK SMALL LETTER BETA
    Codepoint(0x03b3), // 0x63 GREEK SMALL LETTER GAMMA
    Codepoint(0x03b4), // 0x64 GREEK SMALL LETTER DELTA
    Codepoint(0x03b5), // 0x65 GREEK SMALL LETTER EPSILON
    Codepoint(0x03b6), // 0x66 GREEK SMALL LETTER ZETA
    Codepoint(0x03b7), // 0x67 GREEK SMALL LETTER ETA
    Codepoint(0x03b8), // 0x68 GREEK SMALL LETTER THETA
    Codepoint(0x03b9), // 0x69 GREEK SMALL LETTER IOTA
    Codepoint(0x03ba), // 0x6a GREEK SMALL LETTER KAPPA
    Codepoint(0x03bb), // 0x6b GREEK SMALL LETTER LAMDA
    Codepoint(0x03bc), // 0x6c GREEK SMALL LETTER MU
    Codepoint(0x03bd), // 0x6d GREEK SMALL LETTER NU
    Codepoint(0x03be), // 0x6e GREEK SMALL LETTER XI
    Codepoint(0x03bf), // 0x6f GREEK SMALL LETTER OMICRON
    Codepoint(0x03c0), // 0x70 GREEK SMALL LETTER PI
    Codepoint(0x03c1), // 0x71 GREEK SMALL LETTER RHO
    Codepoint(0x03c2), // 0x72 GREEK SMALL LETTER FINAL SIGMA
    Codepoint(0x03c3), // 0x73 GREEK SMALL LETTER SIGMA
    Codepoint(0x03c4), // 0x74 GREEK SMALL LETTER TAU
    Codepoint(0x03c5), // 0x75 GREEK SMALL LETTER UPSILON
    Codepoint(0x03c6), // 0x76 GREEK SMALL LETTER PHI
    Codepoint(0x03c7), // 0x77 GREEK SMALL LETTER CHI
    Codepoint(0x03c8), // 0x78 GREEK SMALL LETTER PSI
    Codepoint(0x03c9), // 0x79 GREEK SMALL LETTER OMEGA
    Codepoint(0x03ca), // 0x7a GREEK SMALL LETTER IOTA WITH DIALYTIKA
    Codepoint(0x03cb), // 0x7b GREEK SMALL LETTER UPSILON WITH DIALYTIKA
    Codepoint(0x03cc), // 0x7c GREEK SMALL LETTER OMICRON WITH TONOS
    Codepoint(0x03cd), // 0x7d GREEK SMALL LETTER UPSILON WITH TONOS
    Codepoint(0x03ce), // 0x7e GREEK SMALL LETTER OMEGA WITH TONOS
    E,                 // 0x7f unassigned
];

/// ISO Hebrew Supplemental, final `H`, ISO-IR 138: `[[source: iso8859-8]]`'s right half.
///
/// Unassigned here and mapped by the file, which maps a later edition: `0xfd`, `0xfe`.
pub(crate) static HEBREW: [Codepoint; 96] = [
    Codepoint(0x00a0), // 0x20 NO-BREAK SPACE
    E,                 // 0x21 unassigned
    Codepoint(0x00a2), // 0x22 CENT SIGN
    Codepoint(0x00a3), // 0x23 POUND SIGN
    Codepoint(0x00a4), // 0x24 CURRENCY SIGN
    Codepoint(0x00a5), // 0x25 YEN SIGN
    Codepoint(0x00a6), // 0x26 BROKEN BAR
    Codepoint(0x00a7), // 0x27 SECTION SIGN
    Codepoint(0x00a8), // 0x28 DIAERESIS
    Codepoint(0x00a9), // 0x29 COPYRIGHT SIGN
    Codepoint(0x00d7), // 0x2a MULTIPLICATION SIGN
    Codepoint(0x00ab), // 0x2b LEFT-POINTING DOUBLE ANGLE QUOTATION MARK
    Codepoint(0x00ac), // 0x2c NOT SIGN
    Codepoint(0x00ad), // 0x2d SOFT HYPHEN
    Codepoint(0x00ae), // 0x2e REGISTERED SIGN
    Codepoint(0x00af), // 0x2f MACRON
    Codepoint(0x00b0), // 0x30 DEGREE SIGN
    Codepoint(0x00b1), // 0x31 PLUS-MINUS SIGN
    Codepoint(0x00b2), // 0x32 SUPERSCRIPT TWO
    Codepoint(0x00b3), // 0x33 SUPERSCRIPT THREE
    Codepoint(0x00b4), // 0x34 ACUTE ACCENT
    Codepoint(0x00b5), // 0x35 MICRO SIGN
    Codepoint(0x00b6), // 0x36 PILCROW SIGN
    Codepoint(0x00b7), // 0x37 MIDDLE DOT
    Codepoint(0x00b8), // 0x38 CEDILLA
    Codepoint(0x00b9), // 0x39 SUPERSCRIPT ONE
    Codepoint(0x00f7), // 0x3a DIVISION SIGN
    Codepoint(0x00bb), // 0x3b RIGHT-POINTING DOUBLE ANGLE QUOTATION MARK
    Codepoint(0x00bc), // 0x3c VULGAR FRACTION ONE QUARTER
    Codepoint(0x00bd), // 0x3d VULGAR FRACTION ONE HALF
    Codepoint(0x00be), // 0x3e VULGAR FRACTION THREE QUARTERS
    E,                 // 0x3f unassigned
    E,                 // 0x40 unassigned
    E,                 // 0x41 unassigned
    E,                 // 0x42 unassigned
    E,                 // 0x43 unassigned
    E,                 // 0x44 unassigned
    E,                 // 0x45 unassigned
    E,                 // 0x46 unassigned
    E,                 // 0x47 unassigned
    E,                 // 0x48 unassigned
    E,                 // 0x49 unassigned
    E,                 // 0x4a unassigned
    E,                 // 0x4b unassigned
    E,                 // 0x4c unassigned
    E,                 // 0x4d unassigned
    E,                 // 0x4e unassigned
    E,                 // 0x4f unassigned
    E,                 // 0x50 unassigned
    E,                 // 0x51 unassigned
    E,                 // 0x52 unassigned
    E,                 // 0x53 unassigned
    E,                 // 0x54 unassigned
    E,                 // 0x55 unassigned
    E,                 // 0x56 unassigned
    E,                 // 0x57 unassigned
    E,                 // 0x58 unassigned
    E,                 // 0x59 unassigned
    E,                 // 0x5a unassigned
    E,                 // 0x5b unassigned
    E,                 // 0x5c unassigned
    E,                 // 0x5d unassigned
    E,                 // 0x5e unassigned
    Codepoint(0x2017), // 0x5f DOUBLE LOW LINE
    Codepoint(0x05d0), // 0x60 HEBREW LETTER ALEF
    Codepoint(0x05d1), // 0x61 HEBREW LETTER BET
    Codepoint(0x05d2), // 0x62 HEBREW LETTER GIMEL
    Codepoint(0x05d3), // 0x63 HEBREW LETTER DALET
    Codepoint(0x05d4), // 0x64 HEBREW LETTER HE
    Codepoint(0x05d5), // 0x65 HEBREW LETTER VAV
    Codepoint(0x05d6), // 0x66 HEBREW LETTER ZAYIN
    Codepoint(0x05d7), // 0x67 HEBREW LETTER HET
    Codepoint(0x05d8), // 0x68 HEBREW LETTER TET
    Codepoint(0x05d9), // 0x69 HEBREW LETTER YOD
    Codepoint(0x05da), // 0x6a HEBREW LETTER FINAL KAF
    Codepoint(0x05db), // 0x6b HEBREW LETTER KAF
    Codepoint(0x05dc), // 0x6c HEBREW LETTER LAMED
    Codepoint(0x05dd), // 0x6d HEBREW LETTER FINAL MEM
    Codepoint(0x05de), // 0x6e HEBREW LETTER MEM
    Codepoint(0x05df), // 0x6f HEBREW LETTER FINAL NUN
    Codepoint(0x05e0), // 0x70 HEBREW LETTER NUN
    Codepoint(0x05e1), // 0x71 HEBREW LETTER SAMEKH
    Codepoint(0x05e2), // 0x72 HEBREW LETTER AYIN
    Codepoint(0x05e3), // 0x73 HEBREW LETTER FINAL PE
    Codepoint(0x05e4), // 0x74 HEBREW LETTER PE
    Codepoint(0x05e5), // 0x75 HEBREW LETTER FINAL TSADI
    Codepoint(0x05e6), // 0x76 HEBREW LETTER TSADI
    Codepoint(0x05e7), // 0x77 HEBREW LETTER QOF
    Codepoint(0x05e8), // 0x78 HEBREW LETTER RESH
    Codepoint(0x05e9), // 0x79 HEBREW LETTER SHIN
    Codepoint(0x05ea), // 0x7a HEBREW LETTER TAV
    E,                 // 0x7b unassigned
    E,                 // 0x7c unassigned
    E,                 // 0x7d unassigned
    E,                 // 0x7e unassigned
    E,                 // 0x7f unassigned
];

/// ISO Latin-Cyrillic, final `L`, ISO-IR 144: `[[source: iso8859-5]]`'s right half.
pub(crate) static LATIN_CYRILLIC: [Codepoint; 96] = [
    Codepoint(0x00a0), // 0x20 NO-BREAK SPACE
    Codepoint(0x0401), // 0x21 CYRILLIC CAPITAL LETTER IO
    Codepoint(0x0402), // 0x22 CYRILLIC CAPITAL LETTER DJE
    Codepoint(0x0403), // 0x23 CYRILLIC CAPITAL LETTER GJE
    Codepoint(0x0404), // 0x24 CYRILLIC CAPITAL LETTER UKRAINIAN IE
    Codepoint(0x0405), // 0x25 CYRILLIC CAPITAL LETTER DZE
    Codepoint(0x0406), // 0x26 CYRILLIC CAPITAL LETTER BYELORUSSIAN-UKRAINIAN I
    Codepoint(0x0407), // 0x27 CYRILLIC CAPITAL LETTER YI
    Codepoint(0x0408), // 0x28 CYRILLIC CAPITAL LETTER JE
    Codepoint(0x0409), // 0x29 CYRILLIC CAPITAL LETTER LJE
    Codepoint(0x040a), // 0x2a CYRILLIC CAPITAL LETTER NJE
    Codepoint(0x040b), // 0x2b CYRILLIC CAPITAL LETTER TSHE
    Codepoint(0x040c), // 0x2c CYRILLIC CAPITAL LETTER KJE
    Codepoint(0x00ad), // 0x2d SOFT HYPHEN
    Codepoint(0x040e), // 0x2e CYRILLIC CAPITAL LETTER SHORT U
    Codepoint(0x040f), // 0x2f CYRILLIC CAPITAL LETTER DZHE
    Codepoint(0x0410), // 0x30 CYRILLIC CAPITAL LETTER A
    Codepoint(0x0411), // 0x31 CYRILLIC CAPITAL LETTER BE
    Codepoint(0x0412), // 0x32 CYRILLIC CAPITAL LETTER VE
    Codepoint(0x0413), // 0x33 CYRILLIC CAPITAL LETTER GHE
    Codepoint(0x0414), // 0x34 CYRILLIC CAPITAL LETTER DE
    Codepoint(0x0415), // 0x35 CYRILLIC CAPITAL LETTER IE
    Codepoint(0x0416), // 0x36 CYRILLIC CAPITAL LETTER ZHE
    Codepoint(0x0417), // 0x37 CYRILLIC CAPITAL LETTER ZE
    Codepoint(0x0418), // 0x38 CYRILLIC CAPITAL LETTER I
    Codepoint(0x0419), // 0x39 CYRILLIC CAPITAL LETTER SHORT I
    Codepoint(0x041a), // 0x3a CYRILLIC CAPITAL LETTER KA
    Codepoint(0x041b), // 0x3b CYRILLIC CAPITAL LETTER EL
    Codepoint(0x041c), // 0x3c CYRILLIC CAPITAL LETTER EM
    Codepoint(0x041d), // 0x3d CYRILLIC CAPITAL LETTER EN
    Codepoint(0x041e), // 0x3e CYRILLIC CAPITAL LETTER O
    Codepoint(0x041f), // 0x3f CYRILLIC CAPITAL LETTER PE
    Codepoint(0x0420), // 0x40 CYRILLIC CAPITAL LETTER ER
    Codepoint(0x0421), // 0x41 CYRILLIC CAPITAL LETTER ES
    Codepoint(0x0422), // 0x42 CYRILLIC CAPITAL LETTER TE
    Codepoint(0x0423), // 0x43 CYRILLIC CAPITAL LETTER U
    Codepoint(0x0424), // 0x44 CYRILLIC CAPITAL LETTER EF
    Codepoint(0x0425), // 0x45 CYRILLIC CAPITAL LETTER HA
    Codepoint(0x0426), // 0x46 CYRILLIC CAPITAL LETTER TSE
    Codepoint(0x0427), // 0x47 CYRILLIC CAPITAL LETTER CHE
    Codepoint(0x0428), // 0x48 CYRILLIC CAPITAL LETTER SHA
    Codepoint(0x0429), // 0x49 CYRILLIC CAPITAL LETTER SHCHA
    Codepoint(0x042a), // 0x4a CYRILLIC CAPITAL LETTER HARD SIGN
    Codepoint(0x042b), // 0x4b CYRILLIC CAPITAL LETTER YERU
    Codepoint(0x042c), // 0x4c CYRILLIC CAPITAL LETTER SOFT SIGN
    Codepoint(0x042d), // 0x4d CYRILLIC CAPITAL LETTER E
    Codepoint(0x042e), // 0x4e CYRILLIC CAPITAL LETTER YU
    Codepoint(0x042f), // 0x4f CYRILLIC CAPITAL LETTER YA
    Codepoint(0x0430), // 0x50 CYRILLIC SMALL LETTER A
    Codepoint(0x0431), // 0x51 CYRILLIC SMALL LETTER BE
    Codepoint(0x0432), // 0x52 CYRILLIC SMALL LETTER VE
    Codepoint(0x0433), // 0x53 CYRILLIC SMALL LETTER GHE
    Codepoint(0x0434), // 0x54 CYRILLIC SMALL LETTER DE
    Codepoint(0x0435), // 0x55 CYRILLIC SMALL LETTER IE
    Codepoint(0x0436), // 0x56 CYRILLIC SMALL LETTER ZHE
    Codepoint(0x0437), // 0x57 CYRILLIC SMALL LETTER ZE
    Codepoint(0x0438), // 0x58 CYRILLIC SMALL LETTER I
    Codepoint(0x0439), // 0x59 CYRILLIC SMALL LETTER SHORT I
    Codepoint(0x043a), // 0x5a CYRILLIC SMALL LETTER KA
    Codepoint(0x043b), // 0x5b CYRILLIC SMALL LETTER EL
    Codepoint(0x043c), // 0x5c CYRILLIC SMALL LETTER EM
    Codepoint(0x043d), // 0x5d CYRILLIC SMALL LETTER EN
    Codepoint(0x043e), // 0x5e CYRILLIC SMALL LETTER O
    Codepoint(0x043f), // 0x5f CYRILLIC SMALL LETTER PE
    Codepoint(0x0440), // 0x60 CYRILLIC SMALL LETTER ER
    Codepoint(0x0441), // 0x61 CYRILLIC SMALL LETTER ES
    Codepoint(0x0442), // 0x62 CYRILLIC SMALL LETTER TE
    Codepoint(0x0443), // 0x63 CYRILLIC SMALL LETTER U
    Codepoint(0x0444), // 0x64 CYRILLIC SMALL LETTER EF
    Codepoint(0x0445), // 0x65 CYRILLIC SMALL LETTER HA
    Codepoint(0x0446), // 0x66 CYRILLIC SMALL LETTER TSE
    Codepoint(0x0447), // 0x67 CYRILLIC SMALL LETTER CHE
    Codepoint(0x0448), // 0x68 CYRILLIC SMALL LETTER SHA
    Codepoint(0x0449), // 0x69 CYRILLIC SMALL LETTER SHCHA
    Codepoint(0x044a), // 0x6a CYRILLIC SMALL LETTER HARD SIGN
    Codepoint(0x044b), // 0x6b CYRILLIC SMALL LETTER YERU
    Codepoint(0x044c), // 0x6c CYRILLIC SMALL LETTER SOFT SIGN
    Codepoint(0x044d), // 0x6d CYRILLIC SMALL LETTER E
    Codepoint(0x044e), // 0x6e CYRILLIC SMALL LETTER YU
    Codepoint(0x044f), // 0x6f CYRILLIC SMALL LETTER YA
    Codepoint(0x2116), // 0x70 NUMERO SIGN
    Codepoint(0x0451), // 0x71 CYRILLIC SMALL LETTER IO
    Codepoint(0x0452), // 0x72 CYRILLIC SMALL LETTER DJE
    Codepoint(0x0453), // 0x73 CYRILLIC SMALL LETTER GJE
    Codepoint(0x0454), // 0x74 CYRILLIC SMALL LETTER UKRAINIAN IE
    Codepoint(0x0455), // 0x75 CYRILLIC SMALL LETTER DZE
    Codepoint(0x0456), // 0x76 CYRILLIC SMALL LETTER BYELORUSSIAN-UKRAINIAN I
    Codepoint(0x0457), // 0x77 CYRILLIC SMALL LETTER YI
    Codepoint(0x0458), // 0x78 CYRILLIC SMALL LETTER JE
    Codepoint(0x0459), // 0x79 CYRILLIC SMALL LETTER LJE
    Codepoint(0x045a), // 0x7a CYRILLIC SMALL LETTER NJE
    Codepoint(0x045b), // 0x7b CYRILLIC SMALL LETTER TSHE
    Codepoint(0x045c), // 0x7c CYRILLIC SMALL LETTER KJE
    Codepoint(0x00a7), // 0x7d SECTION SIGN
    Codepoint(0x045e), // 0x7e CYRILLIC SMALL LETTER SHORT U
    Codepoint(0x045f), // 0x7f CYRILLIC SMALL LETTER DZHE
];

/// ISO Latin-5 Supplemental, final `M`, ISO-IR 148: `[[source: iso8859-9]]`'s right half.
pub(crate) static LATIN_5: [Codepoint; 96] = [
    Codepoint(0x00a0), // 0x20 NO-BREAK SPACE
    Codepoint(0x00a1), // 0x21 INVERTED EXCLAMATION MARK
    Codepoint(0x00a2), // 0x22 CENT SIGN
    Codepoint(0x00a3), // 0x23 POUND SIGN
    Codepoint(0x00a4), // 0x24 CURRENCY SIGN
    Codepoint(0x00a5), // 0x25 YEN SIGN
    Codepoint(0x00a6), // 0x26 BROKEN BAR
    Codepoint(0x00a7), // 0x27 SECTION SIGN
    Codepoint(0x00a8), // 0x28 DIAERESIS
    Codepoint(0x00a9), // 0x29 COPYRIGHT SIGN
    Codepoint(0x00aa), // 0x2a FEMININE ORDINAL INDICATOR
    Codepoint(0x00ab), // 0x2b LEFT-POINTING DOUBLE ANGLE QUOTATION MARK
    Codepoint(0x00ac), // 0x2c NOT SIGN
    Codepoint(0x00ad), // 0x2d SOFT HYPHEN
    Codepoint(0x00ae), // 0x2e REGISTERED SIGN
    Codepoint(0x00af), // 0x2f MACRON
    Codepoint(0x00b0), // 0x30 DEGREE SIGN
    Codepoint(0x00b1), // 0x31 PLUS-MINUS SIGN
    Codepoint(0x00b2), // 0x32 SUPERSCRIPT TWO
    Codepoint(0x00b3), // 0x33 SUPERSCRIPT THREE
    Codepoint(0x00b4), // 0x34 ACUTE ACCENT
    Codepoint(0x00b5), // 0x35 MICRO SIGN
    Codepoint(0x00b6), // 0x36 PILCROW SIGN
    Codepoint(0x00b7), // 0x37 MIDDLE DOT
    Codepoint(0x00b8), // 0x38 CEDILLA
    Codepoint(0x00b9), // 0x39 SUPERSCRIPT ONE
    Codepoint(0x00ba), // 0x3a MASCULINE ORDINAL INDICATOR
    Codepoint(0x00bb), // 0x3b RIGHT-POINTING DOUBLE ANGLE QUOTATION MARK
    Codepoint(0x00bc), // 0x3c VULGAR FRACTION ONE QUARTER
    Codepoint(0x00bd), // 0x3d VULGAR FRACTION ONE HALF
    Codepoint(0x00be), // 0x3e VULGAR FRACTION THREE QUARTERS
    Codepoint(0x00bf), // 0x3f INVERTED QUESTION MARK
    Codepoint(0x00c0), // 0x40 LATIN CAPITAL LETTER A WITH GRAVE
    Codepoint(0x00c1), // 0x41 LATIN CAPITAL LETTER A WITH ACUTE
    Codepoint(0x00c2), // 0x42 LATIN CAPITAL LETTER A WITH CIRCUMFLEX
    Codepoint(0x00c3), // 0x43 LATIN CAPITAL LETTER A WITH TILDE
    Codepoint(0x00c4), // 0x44 LATIN CAPITAL LETTER A WITH DIAERESIS
    Codepoint(0x00c5), // 0x45 LATIN CAPITAL LETTER A WITH RING ABOVE
    Codepoint(0x00c6), // 0x46 LATIN CAPITAL LETTER AE
    Codepoint(0x00c7), // 0x47 LATIN CAPITAL LETTER C WITH CEDILLA
    Codepoint(0x00c8), // 0x48 LATIN CAPITAL LETTER E WITH GRAVE
    Codepoint(0x00c9), // 0x49 LATIN CAPITAL LETTER E WITH ACUTE
    Codepoint(0x00ca), // 0x4a LATIN CAPITAL LETTER E WITH CIRCUMFLEX
    Codepoint(0x00cb), // 0x4b LATIN CAPITAL LETTER E WITH DIAERESIS
    Codepoint(0x00cc), // 0x4c LATIN CAPITAL LETTER I WITH GRAVE
    Codepoint(0x00cd), // 0x4d LATIN CAPITAL LETTER I WITH ACUTE
    Codepoint(0x00ce), // 0x4e LATIN CAPITAL LETTER I WITH CIRCUMFLEX
    Codepoint(0x00cf), // 0x4f LATIN CAPITAL LETTER I WITH DIAERESIS
    Codepoint(0x011e), // 0x50 LATIN CAPITAL LETTER G WITH BREVE
    Codepoint(0x00d1), // 0x51 LATIN CAPITAL LETTER N WITH TILDE
    Codepoint(0x00d2), // 0x52 LATIN CAPITAL LETTER O WITH GRAVE
    Codepoint(0x00d3), // 0x53 LATIN CAPITAL LETTER O WITH ACUTE
    Codepoint(0x00d4), // 0x54 LATIN CAPITAL LETTER O WITH CIRCUMFLEX
    Codepoint(0x00d5), // 0x55 LATIN CAPITAL LETTER O WITH TILDE
    Codepoint(0x00d6), // 0x56 LATIN CAPITAL LETTER O WITH DIAERESIS
    Codepoint(0x00d7), // 0x57 MULTIPLICATION SIGN
    Codepoint(0x00d8), // 0x58 LATIN CAPITAL LETTER O WITH STROKE
    Codepoint(0x00d9), // 0x59 LATIN CAPITAL LETTER U WITH GRAVE
    Codepoint(0x00da), // 0x5a LATIN CAPITAL LETTER U WITH ACUTE
    Codepoint(0x00db), // 0x5b LATIN CAPITAL LETTER U WITH CIRCUMFLEX
    Codepoint(0x00dc), // 0x5c LATIN CAPITAL LETTER U WITH DIAERESIS
    Codepoint(0x0130), // 0x5d LATIN CAPITAL LETTER I WITH DOT ABOVE
    Codepoint(0x015e), // 0x5e LATIN CAPITAL LETTER S WITH CEDILLA
    Codepoint(0x00df), // 0x5f LATIN SMALL LETTER SHARP S
    Codepoint(0x00e0), // 0x60 LATIN SMALL LETTER A WITH GRAVE
    Codepoint(0x00e1), // 0x61 LATIN SMALL LETTER A WITH ACUTE
    Codepoint(0x00e2), // 0x62 LATIN SMALL LETTER A WITH CIRCUMFLEX
    Codepoint(0x00e3), // 0x63 LATIN SMALL LETTER A WITH TILDE
    Codepoint(0x00e4), // 0x64 LATIN SMALL LETTER A WITH DIAERESIS
    Codepoint(0x00e5), // 0x65 LATIN SMALL LETTER A WITH RING ABOVE
    Codepoint(0x00e6), // 0x66 LATIN SMALL LETTER AE
    Codepoint(0x00e7), // 0x67 LATIN SMALL LETTER C WITH CEDILLA
    Codepoint(0x00e8), // 0x68 LATIN SMALL LETTER E WITH GRAVE
    Codepoint(0x00e9), // 0x69 LATIN SMALL LETTER E WITH ACUTE
    Codepoint(0x00ea), // 0x6a LATIN SMALL LETTER E WITH CIRCUMFLEX
    Codepoint(0x00eb), // 0x6b LATIN SMALL LETTER E WITH DIAERESIS
    Codepoint(0x00ec), // 0x6c LATIN SMALL LETTER I WITH GRAVE
    Codepoint(0x00ed), // 0x6d LATIN SMALL LETTER I WITH ACUTE
    Codepoint(0x00ee), // 0x6e LATIN SMALL LETTER I WITH CIRCUMFLEX
    Codepoint(0x00ef), // 0x6f LATIN SMALL LETTER I WITH DIAERESIS
    Codepoint(0x011f), // 0x70 LATIN SMALL LETTER G WITH BREVE
    Codepoint(0x00f1), // 0x71 LATIN SMALL LETTER N WITH TILDE
    Codepoint(0x00f2), // 0x72 LATIN SMALL LETTER O WITH GRAVE
    Codepoint(0x00f3), // 0x73 LATIN SMALL LETTER O WITH ACUTE
    Codepoint(0x00f4), // 0x74 LATIN SMALL LETTER O WITH CIRCUMFLEX
    Codepoint(0x00f5), // 0x75 LATIN SMALL LETTER O WITH TILDE
    Codepoint(0x00f6), // 0x76 LATIN SMALL LETTER O WITH DIAERESIS
    Codepoint(0x00f7), // 0x77 DIVISION SIGN
    Codepoint(0x00f8), // 0x78 LATIN SMALL LETTER O WITH STROKE
    Codepoint(0x00f9), // 0x79 LATIN SMALL LETTER U WITH GRAVE
    Codepoint(0x00fa), // 0x7a LATIN SMALL LETTER U WITH ACUTE
    Codepoint(0x00fb), // 0x7b LATIN SMALL LETTER U WITH CIRCUMFLEX
    Codepoint(0x00fc), // 0x7c LATIN SMALL LETTER U WITH DIAERESIS
    Codepoint(0x0131), // 0x7d LATIN SMALL LETTER DOTLESS I
    Codepoint(0x015f), // 0x7e LATIN SMALL LETTER S WITH CEDILLA
    Codepoint(0x00ff), // 0x7f LATIN SMALL LETTER Y WITH DIAERESIS
];
