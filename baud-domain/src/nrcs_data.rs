//! National Replacement Character Sets: `[[source: xterm-charsets]]`'s
//! macros, their keysyms resolved through `[[source: xorg-keysymdef]]`,
//! and the positions those macros lack that a DEC text draws, each cited
//! on its table, compiled by `build/nrcs.rs`.
//!
//! Each set replaces a handful of ASCII positions -- "the term
//! *replacement* says that the character set is formed by replacing some
//! of the characters with more useful ones for a given language".
//! Everything not listed is ASCII.

use crate::Codepoint;

include!(concat!(env!("OUT_DIR"), "/nrcs_data.rs"));

/// DEC's error character, stored as U+2426 SYMBOL FOR SUBSTITUTE FORM TWO.
///
/// DEC names no codepoint for it: `[[source: dec-std-070]]` §3.4.3 has it
/// "usually imaged as a REVERSE QUESTION MARK", and `[[source: dec-pplv2-pm]]`
/// p. 6-4 calls it "a reverse question mark". U+2426 is ISO 2047's picture of
/// SUB, the control DEC ties it to, and its reference glyph is a reversed
/// question mark (`[[source: unicode18-chart-2400]]`). U+FFFD, one of the
/// three other options asked (with U+2E2E and U+2592), departs from that
/// description by name and by glyph.
///
/// **It is drawn at a reserved position of a DEC set baud translates**,
/// DEC Supplemental's 13 and JIS-Katakana's 31 (`build/dec_sets.rs`): "The
/// error character is printed when one of these reserved characters is
/// received by the device" (`[[source: dec-pplv2-pm]]` §A.2, §A.12). Where
/// else it is drawn waits on open questions: A0 with a 94-set in GR, SUB's
/// display (`docs/vt-coverage.md`), and Greek NRC 7/9 and 7/10, which no
/// DEC table charts.
pub const DEC_ERROR_CHARACTER: Codepoint = Codepoint(0x2426);

/// `ESC ( A` — United Kingdom, whose one substitution xterm spells inline
/// in `charsets.c` rather than as a macro.
static UK: [(u8, u32); 1] = [(0x23, 0x00a3)];

/// United Kingdom, which is a `Charset` of its own rather than an `Nrcs`.
pub const fn uk_table() -> &'static [(u8, u32)] {
    &UK
}
