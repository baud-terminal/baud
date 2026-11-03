//! The DEC 94-character sets beyond DEC Special Graphics and the national
//! replacement sets, generated from the sources pinned in
//! `docs/research/sources/MANIFEST.toml`.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-dec-sets.py`;
//! `dec_set_tables_match_the_pinned_sources` fails if this file and the pinned
//! sources disagree.
//!
//! Each table is indexed by `byte - 0x21`, 2/1 through 7/14, and holds every
//! position: SP and DEL are not graphic characters of a 94-character set
//! (`[[source: dec-std-070]]` 070-3 §3.4.1-3.4.2) and never index it. A
//! reserved position holds DEC's error character.

use crate::{Codepoint, DEC_ERROR_CHARACTER};

/// DEC's error character, at a reserved position.
const E: Codepoint = DEC_ERROR_CHARACTER;

/// DEC Supplemental, `ESC ( <` and `ESC ( % 5`: `[[source: xterm-charsets]]`'
/// `map_DEC_Supp_Graphic`, its 81 glyphs and, in its 13 `UNDEF` cells, the
/// error character, checked cell for cell against
/// `[[source: dec-special-graphics]]` Table 2-3's right half, whose 13 empty
/// cells are the ones `[[source: dec-pplv2-pm]]` Figure A-2 hatches and §A.2
/// says print the error character.
///
/// Where the macro departs from DEC's text, the standard, the cell is DEC's:
///
/// * `0x5f`: xterm's `UNI(0x5F, 0x005F)` draws `_`; DEC's Table 2-3,
///   `[[source: vt320-uu-appe]]` and `[[source: dec-pplv2-pm]]` Figure A-2
///   all have ß, so the table has ß (note AF §3.1).
pub(crate) static DEC_SUPPLEMENTAL: [Codepoint; 94] = [
    Codepoint(0x00a1), // 0x21 INVERTED EXCLAMATION MARK
    Codepoint(0x00a2), // 0x22 CENT SIGN
    Codepoint(0x00a3), // 0x23 POUND SIGN
    E,                 // 0x24 reserved
    Codepoint(0x00a5), // 0x25 YEN SIGN
    E,                 // 0x26 reserved
    Codepoint(0x00a7), // 0x27 SECTION SIGN
    Codepoint(0x00a4), // 0x28 CURRENCY SIGN
    Codepoint(0x00a9), // 0x29 COPYRIGHT SIGN
    Codepoint(0x00aa), // 0x2a FEMININE ORDINAL INDICATOR
    Codepoint(0x00ab), // 0x2b LEFT-POINTING DOUBLE ANGLE QUOTATION MARK
    E,                 // 0x2c reserved
    E,                 // 0x2d reserved
    E,                 // 0x2e reserved
    E,                 // 0x2f reserved
    Codepoint(0x00b0), // 0x30 DEGREE SIGN
    Codepoint(0x00b1), // 0x31 PLUS-MINUS SIGN
    Codepoint(0x00b2), // 0x32 SUPERSCRIPT TWO
    Codepoint(0x00b3), // 0x33 SUPERSCRIPT THREE
    E,                 // 0x34 reserved
    Codepoint(0x00b5), // 0x35 MICRO SIGN
    Codepoint(0x00b6), // 0x36 PILCROW SIGN
    Codepoint(0x00b7), // 0x37 MIDDLE DOT
    E,                 // 0x38 reserved
    Codepoint(0x00b9), // 0x39 SUPERSCRIPT ONE
    Codepoint(0x00ba), // 0x3a MASCULINE ORDINAL INDICATOR
    Codepoint(0x00bb), // 0x3b RIGHT-POINTING DOUBLE ANGLE QUOTATION MARK
    Codepoint(0x00bc), // 0x3c VULGAR FRACTION ONE QUARTER
    Codepoint(0x00bd), // 0x3d VULGAR FRACTION ONE HALF
    E,                 // 0x3e reserved
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
    E,                 // 0x50 reserved
    Codepoint(0x00d1), // 0x51 LATIN CAPITAL LETTER N WITH TILDE
    Codepoint(0x00d2), // 0x52 LATIN CAPITAL LETTER O WITH GRAVE
    Codepoint(0x00d3), // 0x53 LATIN CAPITAL LETTER O WITH ACUTE
    Codepoint(0x00d4), // 0x54 LATIN CAPITAL LETTER O WITH CIRCUMFLEX
    Codepoint(0x00d5), // 0x55 LATIN CAPITAL LETTER O WITH TILDE
    Codepoint(0x00d6), // 0x56 LATIN CAPITAL LETTER O WITH DIAERESIS
    Codepoint(0x0152), // 0x57 LATIN CAPITAL LIGATURE OE
    Codepoint(0x00d8), // 0x58 LATIN CAPITAL LETTER O WITH STROKE
    Codepoint(0x00d9), // 0x59 LATIN CAPITAL LETTER U WITH GRAVE
    Codepoint(0x00da), // 0x5a LATIN CAPITAL LETTER U WITH ACUTE
    Codepoint(0x00db), // 0x5b LATIN CAPITAL LETTER U WITH CIRCUMFLEX
    Codepoint(0x00dc), // 0x5c LATIN CAPITAL LETTER U WITH DIAERESIS
    Codepoint(0x0178), // 0x5d LATIN CAPITAL LETTER Y WITH DIAERESIS
    E,                 // 0x5e reserved
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
    E,                 // 0x70 reserved
    Codepoint(0x00f1), // 0x71 LATIN SMALL LETTER N WITH TILDE
    Codepoint(0x00f2), // 0x72 LATIN SMALL LETTER O WITH GRAVE
    Codepoint(0x00f3), // 0x73 LATIN SMALL LETTER O WITH ACUTE
    Codepoint(0x00f4), // 0x74 LATIN SMALL LETTER O WITH CIRCUMFLEX
    Codepoint(0x00f5), // 0x75 LATIN SMALL LETTER O WITH TILDE
    Codepoint(0x00f6), // 0x76 LATIN SMALL LETTER O WITH DIAERESIS
    Codepoint(0x0153), // 0x77 LATIN SMALL LIGATURE OE
    Codepoint(0x00f8), // 0x78 LATIN SMALL LETTER O WITH STROKE
    Codepoint(0x00f9), // 0x79 LATIN SMALL LETTER U WITH GRAVE
    Codepoint(0x00fa), // 0x7a LATIN SMALL LETTER U WITH ACUTE
    Codepoint(0x00fb), // 0x7b LATIN SMALL LETTER U WITH CIRCUMFLEX
    Codepoint(0x00fc), // 0x7c LATIN SMALL LETTER U WITH DIAERESIS
    Codepoint(0x00ff), // 0x7d LATIN SMALL LETTER Y WITH DIAERESIS
    E,                 // 0x7e reserved
];

/// JIS-Katakana, `ESC ( I`: 2/1-5/15 are JIS X 0201's katakana, which
/// `[[source: dec-pplv2-pm]]` §A.12 names, through `[[source: unicode-jis0201]]`;
/// 6/0-7/14 are reserved and draw the error character (§A.12;
/// `[[source: vt382-rm]]` §2.4.6 and Figure 2-10).
///
/// Where `[[source: xterm-charsets]]`' macro, practice, differs:
///
/// * `0x7e`: xterm's macro lists no 0x7E and so draws `~`; DEC reserves 7/14
///   (§A.12; `[[source: vt382-rm]]` Figure 2-10).
pub(crate) static JIS_KATAKANA: [Codepoint; 94] = [
    Codepoint(0xff61), // 0x21 HALFWIDTH IDEOGRAPHIC FULL STOP
    Codepoint(0xff62), // 0x22 HALFWIDTH LEFT CORNER BRACKET
    Codepoint(0xff63), // 0x23 HALFWIDTH RIGHT CORNER BRACKET
    Codepoint(0xff64), // 0x24 HALFWIDTH IDEOGRAPHIC COMMA
    Codepoint(0xff65), // 0x25 HALFWIDTH KATAKANA MIDDLE DOT
    Codepoint(0xff66), // 0x26 HALFWIDTH KATAKANA LETTER WO
    Codepoint(0xff67), // 0x27 HALFWIDTH KATAKANA LETTER SMALL A
    Codepoint(0xff68), // 0x28 HALFWIDTH KATAKANA LETTER SMALL I
    Codepoint(0xff69), // 0x29 HALFWIDTH KATAKANA LETTER SMALL U
    Codepoint(0xff6a), // 0x2a HALFWIDTH KATAKANA LETTER SMALL E
    Codepoint(0xff6b), // 0x2b HALFWIDTH KATAKANA LETTER SMALL O
    Codepoint(0xff6c), // 0x2c HALFWIDTH KATAKANA LETTER SMALL YA
    Codepoint(0xff6d), // 0x2d HALFWIDTH KATAKANA LETTER SMALL YU
    Codepoint(0xff6e), // 0x2e HALFWIDTH KATAKANA LETTER SMALL YO
    Codepoint(0xff6f), // 0x2f HALFWIDTH KATAKANA LETTER SMALL TU
    Codepoint(0xff70), // 0x30 HALFWIDTH KATAKANA-HIRAGANA PROLONGED SOUND MARK
    Codepoint(0xff71), // 0x31 HALFWIDTH KATAKANA LETTER A
    Codepoint(0xff72), // 0x32 HALFWIDTH KATAKANA LETTER I
    Codepoint(0xff73), // 0x33 HALFWIDTH KATAKANA LETTER U
    Codepoint(0xff74), // 0x34 HALFWIDTH KATAKANA LETTER E
    Codepoint(0xff75), // 0x35 HALFWIDTH KATAKANA LETTER O
    Codepoint(0xff76), // 0x36 HALFWIDTH KATAKANA LETTER KA
    Codepoint(0xff77), // 0x37 HALFWIDTH KATAKANA LETTER KI
    Codepoint(0xff78), // 0x38 HALFWIDTH KATAKANA LETTER KU
    Codepoint(0xff79), // 0x39 HALFWIDTH KATAKANA LETTER KE
    Codepoint(0xff7a), // 0x3a HALFWIDTH KATAKANA LETTER KO
    Codepoint(0xff7b), // 0x3b HALFWIDTH KATAKANA LETTER SA
    Codepoint(0xff7c), // 0x3c HALFWIDTH KATAKANA LETTER SI
    Codepoint(0xff7d), // 0x3d HALFWIDTH KATAKANA LETTER SU
    Codepoint(0xff7e), // 0x3e HALFWIDTH KATAKANA LETTER SE
    Codepoint(0xff7f), // 0x3f HALFWIDTH KATAKANA LETTER SO
    Codepoint(0xff80), // 0x40 HALFWIDTH KATAKANA LETTER TA
    Codepoint(0xff81), // 0x41 HALFWIDTH KATAKANA LETTER TI
    Codepoint(0xff82), // 0x42 HALFWIDTH KATAKANA LETTER TU
    Codepoint(0xff83), // 0x43 HALFWIDTH KATAKANA LETTER TE
    Codepoint(0xff84), // 0x44 HALFWIDTH KATAKANA LETTER TO
    Codepoint(0xff85), // 0x45 HALFWIDTH KATAKANA LETTER NA
    Codepoint(0xff86), // 0x46 HALFWIDTH KATAKANA LETTER NI
    Codepoint(0xff87), // 0x47 HALFWIDTH KATAKANA LETTER NU
    Codepoint(0xff88), // 0x48 HALFWIDTH KATAKANA LETTER NE
    Codepoint(0xff89), // 0x49 HALFWIDTH KATAKANA LETTER NO
    Codepoint(0xff8a), // 0x4a HALFWIDTH KATAKANA LETTER HA
    Codepoint(0xff8b), // 0x4b HALFWIDTH KATAKANA LETTER HI
    Codepoint(0xff8c), // 0x4c HALFWIDTH KATAKANA LETTER HU
    Codepoint(0xff8d), // 0x4d HALFWIDTH KATAKANA LETTER HE
    Codepoint(0xff8e), // 0x4e HALFWIDTH KATAKANA LETTER HO
    Codepoint(0xff8f), // 0x4f HALFWIDTH KATAKANA LETTER MA
    Codepoint(0xff90), // 0x50 HALFWIDTH KATAKANA LETTER MI
    Codepoint(0xff91), // 0x51 HALFWIDTH KATAKANA LETTER MU
    Codepoint(0xff92), // 0x52 HALFWIDTH KATAKANA LETTER ME
    Codepoint(0xff93), // 0x53 HALFWIDTH KATAKANA LETTER MO
    Codepoint(0xff94), // 0x54 HALFWIDTH KATAKANA LETTER YA
    Codepoint(0xff95), // 0x55 HALFWIDTH KATAKANA LETTER YU
    Codepoint(0xff96), // 0x56 HALFWIDTH KATAKANA LETTER YO
    Codepoint(0xff97), // 0x57 HALFWIDTH KATAKANA LETTER RA
    Codepoint(0xff98), // 0x58 HALFWIDTH KATAKANA LETTER RI
    Codepoint(0xff99), // 0x59 HALFWIDTH KATAKANA LETTER RU
    Codepoint(0xff9a), // 0x5a HALFWIDTH KATAKANA LETTER RE
    Codepoint(0xff9b), // 0x5b HALFWIDTH KATAKANA LETTER RO
    Codepoint(0xff9c), // 0x5c HALFWIDTH KATAKANA LETTER WA
    Codepoint(0xff9d), // 0x5d HALFWIDTH KATAKANA LETTER N
    Codepoint(0xff9e), // 0x5e HALFWIDTH KATAKANA VOICED SOUND MARK
    Codepoint(0xff9f), // 0x5f HALFWIDTH KATAKANA SEMI-VOICED SOUND MARK
    E,                 // 0x60 reserved
    E,                 // 0x61 reserved
    E,                 // 0x62 reserved
    E,                 // 0x63 reserved
    E,                 // 0x64 reserved
    E,                 // 0x65 reserved
    E,                 // 0x66 reserved
    E,                 // 0x67 reserved
    E,                 // 0x68 reserved
    E,                 // 0x69 reserved
    E,                 // 0x6a reserved
    E,                 // 0x6b reserved
    E,                 // 0x6c reserved
    E,                 // 0x6d reserved
    E,                 // 0x6e reserved
    E,                 // 0x6f reserved
    E,                 // 0x70 reserved
    E,                 // 0x71 reserved
    E,                 // 0x72 reserved
    E,                 // 0x73 reserved
    E,                 // 0x74 reserved
    E,                 // 0x75 reserved
    E,                 // 0x76 reserved
    E,                 // 0x77 reserved
    E,                 // 0x78 reserved
    E,                 // 0x79 reserved
    E,                 // 0x7a reserved
    E,                 // 0x7b reserved
    E,                 // 0x7c reserved
    E,                 // 0x7d reserved
    E,                 // 0x7e reserved
];

/// JIS-Roman, `ESC ( J`: ASCII with 5/12 the Yen sign (`[[source: vt382-rm]]`
/// §2.4.5; `[[source: unicode-jis0201]]`).
///
/// Open, drawing its byte until answered:
///
/// * `0x7e`: Q-JIS-Roman (note AF): `[[source: vt382-rm]]` §2.4.5 names it the
///   "over line" and `[[source: dec-pplv2-pm]]` Table A-2 "Macron";
///   U+203E or U+00AF.
///
/// Where `[[source: xterm-charsets]]`' macro, practice, differs:
///
/// * `0x7e`: xterm's `UNI(0x7E, 0x203E)` is one answer to Q-JIS-Roman, open.
pub(crate) static JIS_ROMAN: [Codepoint; 94] = [
    Codepoint(0x0021), // 0x21 EXCLAMATION MARK
    Codepoint(0x0022), // 0x22 QUOTATION MARK
    Codepoint(0x0023), // 0x23 NUMBER SIGN
    Codepoint(0x0024), // 0x24 DOLLAR SIGN
    Codepoint(0x0025), // 0x25 PERCENT SIGN
    Codepoint(0x0026), // 0x26 AMPERSAND
    Codepoint(0x0027), // 0x27 APOSTROPHE
    Codepoint(0x0028), // 0x28 LEFT PARENTHESIS
    Codepoint(0x0029), // 0x29 RIGHT PARENTHESIS
    Codepoint(0x002a), // 0x2a ASTERISK
    Codepoint(0x002b), // 0x2b PLUS SIGN
    Codepoint(0x002c), // 0x2c COMMA
    Codepoint(0x002d), // 0x2d HYPHEN-MINUS
    Codepoint(0x002e), // 0x2e FULL STOP
    Codepoint(0x002f), // 0x2f SOLIDUS
    Codepoint(0x0030), // 0x30 DIGIT ZERO
    Codepoint(0x0031), // 0x31 DIGIT ONE
    Codepoint(0x0032), // 0x32 DIGIT TWO
    Codepoint(0x0033), // 0x33 DIGIT THREE
    Codepoint(0x0034), // 0x34 DIGIT FOUR
    Codepoint(0x0035), // 0x35 DIGIT FIVE
    Codepoint(0x0036), // 0x36 DIGIT SIX
    Codepoint(0x0037), // 0x37 DIGIT SEVEN
    Codepoint(0x0038), // 0x38 DIGIT EIGHT
    Codepoint(0x0039), // 0x39 DIGIT NINE
    Codepoint(0x003a), // 0x3a COLON
    Codepoint(0x003b), // 0x3b SEMICOLON
    Codepoint(0x003c), // 0x3c LESS-THAN SIGN
    Codepoint(0x003d), // 0x3d EQUALS SIGN
    Codepoint(0x003e), // 0x3e GREATER-THAN SIGN
    Codepoint(0x003f), // 0x3f QUESTION MARK
    Codepoint(0x0040), // 0x40 COMMERCIAL AT
    Codepoint(0x0041), // 0x41 LATIN CAPITAL LETTER A
    Codepoint(0x0042), // 0x42 LATIN CAPITAL LETTER B
    Codepoint(0x0043), // 0x43 LATIN CAPITAL LETTER C
    Codepoint(0x0044), // 0x44 LATIN CAPITAL LETTER D
    Codepoint(0x0045), // 0x45 LATIN CAPITAL LETTER E
    Codepoint(0x0046), // 0x46 LATIN CAPITAL LETTER F
    Codepoint(0x0047), // 0x47 LATIN CAPITAL LETTER G
    Codepoint(0x0048), // 0x48 LATIN CAPITAL LETTER H
    Codepoint(0x0049), // 0x49 LATIN CAPITAL LETTER I
    Codepoint(0x004a), // 0x4a LATIN CAPITAL LETTER J
    Codepoint(0x004b), // 0x4b LATIN CAPITAL LETTER K
    Codepoint(0x004c), // 0x4c LATIN CAPITAL LETTER L
    Codepoint(0x004d), // 0x4d LATIN CAPITAL LETTER M
    Codepoint(0x004e), // 0x4e LATIN CAPITAL LETTER N
    Codepoint(0x004f), // 0x4f LATIN CAPITAL LETTER O
    Codepoint(0x0050), // 0x50 LATIN CAPITAL LETTER P
    Codepoint(0x0051), // 0x51 LATIN CAPITAL LETTER Q
    Codepoint(0x0052), // 0x52 LATIN CAPITAL LETTER R
    Codepoint(0x0053), // 0x53 LATIN CAPITAL LETTER S
    Codepoint(0x0054), // 0x54 LATIN CAPITAL LETTER T
    Codepoint(0x0055), // 0x55 LATIN CAPITAL LETTER U
    Codepoint(0x0056), // 0x56 LATIN CAPITAL LETTER V
    Codepoint(0x0057), // 0x57 LATIN CAPITAL LETTER W
    Codepoint(0x0058), // 0x58 LATIN CAPITAL LETTER X
    Codepoint(0x0059), // 0x59 LATIN CAPITAL LETTER Y
    Codepoint(0x005a), // 0x5a LATIN CAPITAL LETTER Z
    Codepoint(0x005b), // 0x5b LEFT SQUARE BRACKET
    Codepoint(0x00a5), // 0x5c YEN SIGN
    Codepoint(0x005d), // 0x5d RIGHT SQUARE BRACKET
    Codepoint(0x005e), // 0x5e CIRCUMFLEX ACCENT
    Codepoint(0x005f), // 0x5f LOW LINE
    Codepoint(0x0060), // 0x60 GRAVE ACCENT
    Codepoint(0x0061), // 0x61 LATIN SMALL LETTER A
    Codepoint(0x0062), // 0x62 LATIN SMALL LETTER B
    Codepoint(0x0063), // 0x63 LATIN SMALL LETTER C
    Codepoint(0x0064), // 0x64 LATIN SMALL LETTER D
    Codepoint(0x0065), // 0x65 LATIN SMALL LETTER E
    Codepoint(0x0066), // 0x66 LATIN SMALL LETTER F
    Codepoint(0x0067), // 0x67 LATIN SMALL LETTER G
    Codepoint(0x0068), // 0x68 LATIN SMALL LETTER H
    Codepoint(0x0069), // 0x69 LATIN SMALL LETTER I
    Codepoint(0x006a), // 0x6a LATIN SMALL LETTER J
    Codepoint(0x006b), // 0x6b LATIN SMALL LETTER K
    Codepoint(0x006c), // 0x6c LATIN SMALL LETTER L
    Codepoint(0x006d), // 0x6d LATIN SMALL LETTER M
    Codepoint(0x006e), // 0x6e LATIN SMALL LETTER N
    Codepoint(0x006f), // 0x6f LATIN SMALL LETTER O
    Codepoint(0x0070), // 0x70 LATIN SMALL LETTER P
    Codepoint(0x0071), // 0x71 LATIN SMALL LETTER Q
    Codepoint(0x0072), // 0x72 LATIN SMALL LETTER R
    Codepoint(0x0073), // 0x73 LATIN SMALL LETTER S
    Codepoint(0x0074), // 0x74 LATIN SMALL LETTER T
    Codepoint(0x0075), // 0x75 LATIN SMALL LETTER U
    Codepoint(0x0076), // 0x76 LATIN SMALL LETTER V
    Codepoint(0x0077), // 0x77 LATIN SMALL LETTER W
    Codepoint(0x0078), // 0x78 LATIN SMALL LETTER X
    Codepoint(0x0079), // 0x79 LATIN SMALL LETTER Y
    Codepoint(0x007a), // 0x7a LATIN SMALL LETTER Z
    Codepoint(0x007b), // 0x7b LEFT CURLY BRACKET
    Codepoint(0x007c), // 0x7c VERTICAL LINE
    Codepoint(0x007d), // 0x7d RIGHT CURLY BRACKET
    Codepoint(0x007e), // 0x7e TILDE
];
