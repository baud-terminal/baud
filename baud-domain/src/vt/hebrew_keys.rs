//! **DECHEBM, `CSI ? 35 h` / `CSI ? 35 l`** (`[[source: vt510-dechebm]]`):
//! "This control function sets the keyboard mapping to Hebrew/N-A and
//! resets the mapping to North American."
//!
//! **Group 2 is the standard Israeli layout, by the key's place**: DEC's
//! own Hebrew key legends are figures, so a key the platform's layout
//! typed its unshifted character on sends the letter SI-1452's first level
//! puts on that physical key, as xkeyboard-config's `il` layout
//! transcribes it (`[[source: xkb-symbols-il-2ec770a]]`, "an implemetation
//! of the Israeli standard SI-1452 (2013)"). The key is named by its place
//! on the US layout, which both apps give each key event. With Shift a key
//! types SI-1452's second level, the US layout's but for six keys, which
//! type their mirror: `(` and `)`, `{` and `}`, `<` and `>`, each the
//! other.
//!
//! **Only with a Hebrew keyboard**: "DECHEBM is only valid when a Hebrew
//! keyboard language is selected", and baud's keyboard is the system's, so
//! the embedder says whether the system has a Hebrew layout enabled
//! ([`HebrewLayout`]).

use crate::*;

/// **`?35` DECHEBM's two states** (`[[source: vt510-dechebm]]`): set, "the
/// keyboard mapping to Hebrew/N-A", the keyboard's Group 2; reset, "North
/// American", its Group 1. North American at power-on and after `RIS`;
/// `DECSTR` leaves it, as its table does not list it (`[[source:
/// vt510-decstr]]`). Global: it maps the keyboard, which both displays
/// share.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HebrewKeyboard {
    #[default]
    NorthAmerican,
    Hebrew,
}

/// **Whether the system has a Hebrew keyboard layout enabled**, as the
/// embedder finds it: DECHEBM acts only while it has, "only valid when a
/// Hebrew keyboard language is selected" (`[[source: vt510-dechebm]]`).
/// `Absent` until the embedder says; no sequence changes it, `RIS`
/// included.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HebrewLayout {
    Present,
    #[default]
    Absent,
}

/// SI-1452's first level, where it differs from the US layout's: each
/// key by the character the US layout types on it unshifted, and what
/// the Israeli layout types there (`[[source: xkb-symbols-il-2ec770a]]`,
/// `xkb_symbols "basic"`, the keys `TLDE`, `AD01` to `AD12`, `AC01` to
/// `AC11` and `AB01` to `AB10`). The digits, `-`, `=` and `\` type the
/// same on both.
const SI_1452: [(u8, char); 34] = [
    (b'`', ';'),
    (b'q', '/'),
    (b'w', '\''),
    (b'e', '\u{5E7}'),
    (b'r', '\u{5E8}'),
    (b't', '\u{5D0}'),
    (b'y', '\u{5D8}'),
    (b'u', '\u{5D5}'),
    (b'i', '\u{5DF}'),
    (b'o', '\u{5DD}'),
    (b'p', '\u{5E4}'),
    (b'[', ']'),
    (b']', '['),
    (b'a', '\u{5E9}'),
    (b's', '\u{5D3}'),
    (b'd', '\u{5D2}'),
    (b'f', '\u{5DB}'),
    (b'g', '\u{5E2}'),
    (b'h', '\u{5D9}'),
    (b'j', '\u{5D7}'),
    (b'k', '\u{5DC}'),
    (b'l', '\u{5DA}'),
    (b';', '\u{5E3}'),
    (b'\'', ','),
    (b'z', '\u{5D6}'),
    (b'x', '\u{5E1}'),
    (b'c', '\u{5D1}'),
    (b'v', '\u{5D4}'),
    (b'b', '\u{5E0}'),
    (b'n', '\u{5DE}'),
    (b'm', '\u{5E6}'),
    (b',', '\u{5EA}'),
    (b'.', '\u{5E5}'),
    (b'/', '.'),
];

/// The US layout's second level, where it is not the first's capital:
/// each key by the character the US layout types on it unshifted, and
/// what it types with Shift (`baud-xkb/tests/fixtures/keymap-us.xkb`,
/// the keys `TLDE`, `AE01` to `AE12`, `AD11`, `AD12`, `BKSL`, `AC10`,
/// `AC11` and `AB08` to `AB10`).
const US_SHIFTED: [(u8, u8); 21] = [
    (b'`', b'~'),
    (b'1', b'!'),
    (b'2', b'@'),
    (b'3', b'#'),
    (b'4', b'$'),
    (b'5', b'%'),
    (b'6', b'^'),
    (b'7', b'&'),
    (b'8', b'*'),
    (b'9', b'('),
    (b'0', b')'),
    (b'-', b'_'),
    (b'=', b'+'),
    (b'[', b'{'),
    (b']', b'}'),
    (b'\\', b'|'),
    (b';', b':'),
    (b'\'', b'"'),
    (b',', b'<'),
    (b'.', b'>'),
    (b'/', b'?'),
];

/// SI-1452's second level, where it differs from the US layout's: each
/// key by the character the US layout types on it with Shift, and what
/// the Israeli layout types there (`[[source: xkb-symbols-il-2ec770a]]`,
/// `xkb_symbols "basic"`, the keys `AE09`, `AE10`, `AD11`, `AD12`,
/// `AB08` and `AB09`): each key of a mirrored pair types the other.
const SI_1452_SHIFTED: [(u8, char); 6] = [
    (b'(', ')'),
    (b')', '('),
    (b'{', '}'),
    (b'}', '{'),
    (b'<', '>'),
    (b'>', '<'),
];

/// [`SI_1452`] indexed by the US character, built at compile time: the
/// Israeli layout's character, or 0 for a key the two type alike.
const BY_US: [u32; 128] = {
    let mut t = [0u32; 128];
    let mut i = 0;
    while i < SI_1452.len() {
        t[SI_1452[i].0 as usize] = SI_1452[i].1 as u32;
        i += 1;
    }
    t
};

/// [`US_SHIFTED`] indexed by the US character, the letters' capitals
/// added, built at compile time: what the key types with Shift, or 0 for
/// a character the US layout types on no key unshifted.
const US_SECOND: [u8; 128] = {
    let mut t = [0u8; 128];
    let mut i = 0;
    while i < US_SHIFTED.len() {
        t[US_SHIFTED[i].0 as usize] = US_SHIFTED[i].1;
        i += 1;
    }
    let mut c = b'a';
    while c <= b'z' {
        t[c as usize] = c.to_ascii_uppercase();
        c += 1;
    }
    t
};

/// [`SI_1452_SHIFTED`] indexed by the US layout's shifted character,
/// built at compile time: the Israeli layout's, or 0 for a key the two
/// type alike.
const BY_US_SHIFTED: [u32; 128] = {
    let mut t = [0u32; 128];
    let mut i = 0;
    while i < SI_1452_SHIFTED.len() {
        t[SI_1452_SHIFTED[i].0 as usize] = SI_1452_SHIFTED[i].1 as u32;
        i += 1;
    }
    t
};

/// The entry of `table` for an ASCII `c`, or `None` for 0 or past it.
fn ascii_entry<T: Copy + Into<u32>>(table: &[T; 128], c: Codepoint) -> Option<Codepoint> {
    let i = usize::try_from(c.value()).ok()?;
    match table.get(i).copied().map(Into::into) {
        Some(0) | None => None,
        Some(v) => Codepoint::new(v),
    }
}

/// **What the key the US layout types `us` on types in Group 2**, the
/// standard Israeli layout's first level; `None` for a key it types
/// alike.
pub fn hebrew_key(us: Codepoint) -> Option<Codepoint> {
    ascii_entry(&BY_US, us)
}

/// **What the key the US layout types `shifted` on with Shift types in
/// Group 2 with Shift**, the standard Israeli layout's second level;
/// `None` for a key it types alike.
pub fn hebrew_key_shifted(shifted: Codepoint) -> Option<Codepoint> {
    ascii_entry(&BY_US_SHIFTED, shifted)
}

/// **What the US layout types with Shift on the key where it types `us`
/// unshifted**; `None` for a character it types on no key unshifted.
pub fn us_shifted(us: Codepoint) -> Option<Codepoint> {
    ascii_entry(&US_SECOND, us)
}

/// The first of the letters alef to tav, U+05D0.
const ALEF: u32 = 0x5D0;

/// The DEC 7-bit Hebrew set's code for each letter alef to tav, built at
/// compile time from the table the set draws by ([`nrcs_table`]), so the
/// keyboard sends the code the screen draws the letter for.
const SEVEN_BIT: [u8; 27] = {
    let set = nrcs_table(Nrcs::Hebrew);
    let mut t = [0u8; 27];
    let mut i = 0;
    while i < set.len() {
        let (code, cp) = set[i];
        assert!(cp >= ALEF && cp < ALEF + 27, "the set holds alef to tav");
        t[(cp - ALEF) as usize] = code;
        i += 1;
    }
    t
};

/// **A Hebrew letter in the DEC 7-bit Hebrew set**, DECHEBM's keyboard
/// character set "when 7-bit characters are selected", the set DECHEM
/// designates (`[[source: xterm-charsets]]`'s `map_NRCS_Hebrew`, alef to
/// tav at 60 to 7A). `None` for a character that is not a letter alef to
/// tav.
pub fn seven_bit_hebrew(cp: Codepoint) -> Option<Codepoint> {
    let i = cp.value().checked_sub(ALEF)?;
    let code = *SEVEN_BIT.get(usize::try_from(i).ok()?)?;
    Codepoint::new(u32::from(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(c: char) -> Codepoint {
        Codepoint::from_char(c)
    }

    /// **The table is xkeyboard-config's `il` first level**, read off the
    /// file's own lines: each key's US character and its Hebrew keysym.
    #[test]
    fn the_layout_is_si_1452s_first_level() {
        assert_eq!(hebrew_key(cp('t')), Some(cp('\u{5D0}')), "alef");
        assert_eq!(hebrew_key(cp(',')), Some(cp('\u{5EA}')), "tav");
        assert_eq!(hebrew_key(cp('a')), Some(cp('\u{5E9}')), "shin");
        assert_eq!(hebrew_key(cp('q')), Some(cp('/')));
        assert_eq!(hebrew_key(cp('[')), Some(cp(']')));
        assert_eq!(hebrew_key(cp('1')), None, "a digit types alike");
        assert_eq!(hebrew_key(cp('-')), None, "the minus types alike");
        assert_eq!(hebrew_key(cp('\u{80}')), None);
        assert_eq!(hebrew_key(cp('Q')), None, "Shift types Latin capitals");
        assert_eq!(hebrew_key(cp('\u{5D0}')), None);
        let letters = SI_1452
            .iter()
            .filter(|(_, h)| ('\u{5D0}'..='\u{5EA}').contains(h))
            .count();
        assert_eq!(letters, 27, "every letter, the five finals included");
    }

    /// **The second level differs from the US layout's on six keys**,
    /// xkeyboard-config's `il` `AE09`, `AE10`, `AD11`, `AD12`, `AB08` and
    /// `AB09`: every other key's second level, the capitals included, is
    /// the US layout's.
    #[test]
    fn the_second_level_is_the_us_layouts_but_for_six_keys() {
        assert_eq!(hebrew_key_shifted(cp('(')), Some(cp(')')), "AE09");
        assert_eq!(hebrew_key_shifted(cp(')')), Some(cp('(')), "AE10");
        assert_eq!(hebrew_key_shifted(cp('{')), Some(cp('}')), "AD11");
        assert_eq!(hebrew_key_shifted(cp('}')), Some(cp('{')), "AD12");
        assert_eq!(hebrew_key_shifted(cp('<')), Some(cp('>')), "AB08");
        assert_eq!(hebrew_key_shifted(cp('>')), Some(cp('<')), "AB09");
        let differ = (0u32..128)
            .filter_map(Codepoint::new)
            .filter(|&c| hebrew_key_shifted(c).is_some())
            .count();
        assert_eq!(differ, 6);
        for c in [':', '"', '?', '~', 'Q', 'A', '!', '_', '|'] {
            assert_eq!(hebrew_key_shifted(cp(c)), None, "{c}");
        }
        assert_eq!(us_shifted(cp('[')), Some(cp('{')));
        assert_eq!(us_shifted(cp(';')), Some(cp(':')));
        assert_eq!(us_shifted(cp('a')), Some(cp('A')));
        assert_eq!(us_shifted(cp('{')), None, "no key types it unshifted");
        let keys = (0u32..128)
            .filter_map(Codepoint::new)
            .filter(|&c| us_shifted(c).is_some())
            .count();
        assert_eq!(keys, 47, "the US layout's 47 text keys");
    }

    #[test]
    fn the_seven_bit_set_puts_alef_to_tav_at_60_to_7a() {
        assert_eq!(seven_bit_hebrew(cp('\u{5D0}')), Some(cp('`')));
        assert_eq!(seven_bit_hebrew(cp('\u{5EA}')), Some(cp('z')));
        assert_eq!(seven_bit_hebrew(cp('a')), None);
        assert_eq!(seven_bit_hebrew(cp('\u{5EB}')), None);
    }
}
