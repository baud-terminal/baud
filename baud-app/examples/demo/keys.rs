//! **What a step types, as key presses on the U.S. layout**: each character
//! the Mac's virtual key code of the key that types it, with Shift where
//! the U.S. layout needs it -- what System Events' `keystroke` posts.

use crate::session::Step;

/// The modifier held for a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    Nothing,
    Shift,
    Control,
}

/// One key press: the virtual key code, the modifier held, and the
/// character the event carries as its text. Under [`Held::Control`] the text
/// is what the layout types there, which the system fills in itself
/// (`mac::Poster::key`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub code: u16,
    pub held: Held,
    pub text: char,
}

/// **The U.S. ANSI layout, unshifted**: each character, the `kVK_` name
/// `Events.h` gives its key (`[[source: macos-sdk-events-h]]`) and that
/// key's code. The names are what the header probe checks the codes
/// against (`the_key_codes_are_the_headers_own`).
pub const US: [(char, &str, u16); 48] = [
    ('a', "kVK_ANSI_A", 0x00),
    ('s', "kVK_ANSI_S", 0x01),
    ('d', "kVK_ANSI_D", 0x02),
    ('f', "kVK_ANSI_F", 0x03),
    ('h', "kVK_ANSI_H", 0x04),
    ('g', "kVK_ANSI_G", 0x05),
    ('z', "kVK_ANSI_Z", 0x06),
    ('x', "kVK_ANSI_X", 0x07),
    ('c', "kVK_ANSI_C", 0x08),
    ('v', "kVK_ANSI_V", 0x09),
    ('b', "kVK_ANSI_B", 0x0B),
    ('q', "kVK_ANSI_Q", 0x0C),
    ('w', "kVK_ANSI_W", 0x0D),
    ('e', "kVK_ANSI_E", 0x0E),
    ('r', "kVK_ANSI_R", 0x0F),
    ('y', "kVK_ANSI_Y", 0x10),
    ('t', "kVK_ANSI_T", 0x11),
    ('1', "kVK_ANSI_1", 0x12),
    ('2', "kVK_ANSI_2", 0x13),
    ('3', "kVK_ANSI_3", 0x14),
    ('4', "kVK_ANSI_4", 0x15),
    ('6', "kVK_ANSI_6", 0x16),
    ('5', "kVK_ANSI_5", 0x17),
    ('=', "kVK_ANSI_Equal", 0x18),
    ('9', "kVK_ANSI_9", 0x19),
    ('7', "kVK_ANSI_7", 0x1A),
    ('-', "kVK_ANSI_Minus", 0x1B),
    ('8', "kVK_ANSI_8", 0x1C),
    ('0', "kVK_ANSI_0", 0x1D),
    (']', "kVK_ANSI_RightBracket", 0x1E),
    ('o', "kVK_ANSI_O", 0x1F),
    ('u', "kVK_ANSI_U", 0x20),
    ('[', "kVK_ANSI_LeftBracket", 0x21),
    ('i', "kVK_ANSI_I", 0x22),
    ('p', "kVK_ANSI_P", 0x23),
    ('l', "kVK_ANSI_L", 0x25),
    ('j', "kVK_ANSI_J", 0x26),
    ('\'', "kVK_ANSI_Quote", 0x27),
    ('k', "kVK_ANSI_K", 0x28),
    (';', "kVK_ANSI_Semicolon", 0x29),
    ('\\', "kVK_ANSI_Backslash", 0x2A),
    (',', "kVK_ANSI_Comma", 0x2B),
    ('/', "kVK_ANSI_Slash", 0x2C),
    ('n', "kVK_ANSI_N", 0x2D),
    ('m', "kVK_ANSI_M", 0x2E),
    ('.', "kVK_ANSI_Period", 0x2F),
    (' ', "kVK_Space", 0x31),
    ('`', "kVK_ANSI_Grave", 0x32),
];

/// **The U.S. layout's shifted characters**, each with the unshifted
/// character on the same key.
pub const SHIFTED: [(char, char); 21] = [
    ('!', '1'),
    ('@', '2'),
    ('#', '3'),
    ('$', '4'),
    ('%', '5'),
    ('^', '6'),
    ('&', '7'),
    ('*', '8'),
    ('(', '9'),
    (')', '0'),
    ('_', '-'),
    ('+', '='),
    ('{', '['),
    ('}', ']'),
    ('|', '\\'),
    (':', ';'),
    ('"', '\''),
    ('<', ','),
    ('>', '.'),
    ('?', '/'),
    ('~', '`'),
];

/// Return, `kVK_Return`, whose text is a carriage return.
pub const RETURN: Key = Key {
    code: 0x24,
    held: Held::Nothing,
    text: '\r',
};

/// The `kVK_` name of [`RETURN`]'s code, for the header probe.
#[cfg(test)]
pub const RETURN_NAME: &str = "kVK_Return";

/// **Escape**, `kVK_Escape`, whose text is the ESC control: what helix
/// leaves insert mode by. baud encodes the key itself (`Term::key`), as
/// the program on screen has asked for, so a program that pushed kitty's
/// keyboard flags hears it in their form, not as a bare ESC.
pub const ESCAPE: Key = Key {
    code: 0x35,
    held: Held::Nothing,
    text: '\u{1b}',
};

/// The `kVK_` name of [`ESCAPE`]'s code, for the header probe.
#[cfg(test)]
pub const ESCAPE_NAME: &str = "kVK_Escape";

fn code_of(c: char) -> Option<u16> {
    US.iter()
        .find(|(k, _, _)| *k == c)
        .map(|(_, _, code)| *code)
}

/// The key that types `c` on the U.S. layout: its own, the unshifted
/// character's with Shift, or a capital's lower case with Shift.
pub fn key_for(c: char) -> Option<Key> {
    if let Some(code) = code_of(c) {
        return Some(Key {
            code,
            held: Held::Nothing,
            text: c,
        });
    }
    let base = SHIFTED
        .iter()
        .find(|(s, _)| *s == c)
        .map(|(_, b)| *b)
        .or_else(|| c.is_ascii_uppercase().then(|| c.to_ascii_lowercase()))?;
    code_of(base).map(|code| Key {
        code,
        held: Held::Shift,
        text: c,
    })
}

/// **Control and a letter's key**, whose text is the control code the
/// U.S. layout types there, the letter's bit 6 cleared; or `c` back when it
/// is no lower-case letter.
pub fn control_key(c: char) -> Result<Key, char> {
    let code = code_of(c).filter(|_| c.is_ascii_lowercase()).ok_or(c)?;
    let text = char::from_u32(u32::from(c) & 0x1f).ok_or(c)?;
    Ok(Key {
        code,
        held: Held::Control,
        text,
    })
}

/// The keys that type `text`, or the first character no key types.
pub fn keys_for(text: &str) -> Result<Vec<Key>, char> {
    text.chars().map(|c| key_for(c).ok_or(c)).collect()
}

/// **Every key a step presses, Return included**, or the first character
/// no key types. A step that types nothing presses nothing.
pub fn presses(step: Step) -> Result<Vec<Key>, char> {
    match step {
        Step::Run(t) | Step::Line(t) => {
            let mut keys = keys_for(t)?;
            keys.push(RETURN);
            Ok(keys)
        }
        Step::Keys(t) => keys_for(t),
        Step::Escape => Ok(vec![ESCAPE]),
        Step::Ctrl(c) => Ok(vec![control_key(c)?]),
        Step::Prompt
        | Step::Hold(_)
        | Step::Wheel { .. }
        | Step::Hover { .. }
        | Step::Drag { .. } => Ok(Vec::new()),
    }
}

/// **The gap before each next key**: `KEY_GAP_MS` +-25%, drawn from a
/// SplitMix64 seeded with baud's pid and the step's key count, as the
/// helper the first recording was typed with seeded its own, so a take
/// replays.
pub struct Gaps(u64);

impl Gaps {
    pub fn new(pid: u32, keys: usize) -> Gaps {
        Gaps(u64::from(pid) ^ keys as u64)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// The next gap, in milliseconds, within `gap` +-25%.
    pub fn next_ms(&mut self, gap: f64) -> f64 {
        let unit = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        gap * (0.75 + 0.5 * unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: u16, held: Held, text: char) -> Key {
        Key { code, held, text }
    }

    /// **`cat README.md` and Return are the U.S. keys System Events would
    /// press**: c a t space, then R E A D M E with Shift on their lower
    /// cases' keys, the period, m, d, and Return. Perturbation: drop the
    /// capitals' Shift and `R` is the `r` key alone, which types `r`.
    #[test]
    fn a_command_is_its_us_keys_with_shift_and_return() {
        let got = presses(Step::Run("cat README.md")).unwrap();
        let n = Held::Nothing;
        let s = Held::Shift;
        let want = [
            k(0x08, n, 'c'),
            k(0x00, n, 'a'),
            k(0x11, n, 't'),
            k(0x31, n, ' '),
            k(0x0F, s, 'R'),
            k(0x0E, s, 'E'),
            k(0x00, s, 'A'),
            k(0x02, s, 'D'),
            k(0x2E, s, 'M'),
            k(0x0E, s, 'E'),
            k(0x2F, n, '.'),
            k(0x2E, n, 'm'),
            k(0x02, n, 'd'),
            RETURN,
        ];
        assert_eq!(got, want);
    }

    /// **A shifted character is its unshifted neighbour's key with
    /// Shift**: helix's `:q` is Shift-semicolon then q, its `/character`
    /// begins with the slash key alone, and `--` and `?` are the minus and
    /// slash keys. A program's keys get no Return, a line's do.
    #[test]
    fn shifted_characters_are_their_keys_with_shift() {
        let line = presses(Step::Line(":q")).unwrap();
        assert_eq!(
            line,
            [
                k(0x29, Held::Shift, ':'),
                k(0x0C, Held::Nothing, 'q'),
                RETURN
            ]
        );
        assert_eq!(key_for('/'), Some(k(0x2C, Held::Nothing, '/')));
        assert_eq!(key_for('?'), Some(k(0x2C, Held::Shift, '?')));
        assert_eq!(key_for('-'), Some(k(0x1B, Held::Nothing, '-')));
        assert_eq!(key_for('~'), Some(k(0x32, Held::Shift, '~')));
        assert_eq!(presses(Step::Keys("ge")).unwrap().len(), 2);
        assert_eq!(presses(Step::Hold(10)).unwrap(), []);
    }

    /// **Escape is its own key, not a character typed**: the step presses
    /// `kVK_Escape` alone, no Return after it, and no character of the U.S.
    /// table types ESC, so `Keys` cannot stand in for it -- the text a
    /// program pushed kitty's flags for would be a bare ESC rather than
    /// baud's encoding of the key. Perturbation: give the step the Return
    /// key and the first assertion fails.
    #[test]
    fn escape_is_its_own_key() {
        assert_eq!(presses(Step::Escape).unwrap(), [ESCAPE]);
        assert_eq!(ESCAPE.code, 0x35);
        assert_eq!(key_for('\u{1b}'), None, "no character key types ESC");
        assert!(!US.iter().any(|(_, _, c)| *c == ESCAPE.code));
    }

    /// **Ctrl-d is the `d` key with Control held**, its text the control
    /// code the U.S. layout types there, and no Return after it; no character
    /// of the table types that code, so `Keys` cannot stand in for it. The
    /// control: a key that is no lower-case letter has no such press.
    /// Perturbation: hold Shift instead and the first assertion fails.
    #[test]
    fn ctrl_d_is_the_d_key_with_control() {
        assert_eq!(
            presses(Step::Ctrl('d')).unwrap(),
            [k(0x02, Held::Control, '\u{4}')]
        );
        assert_eq!(key_for('\u{4}'), None);
        assert_eq!(control_key('D'), Err('D'));
        assert_eq!(control_key('\u{e9}'), Err('\u{e9}'));
    }

    /// **Every character the README session types has a U.S. key**, and a
    /// character none does is refused by name rather than typed as
    /// something else.
    #[test]
    fn every_key_the_session_types_exists() {
        for step in crate::session::SESSION {
            assert!(presses(*step).is_ok(), "{step:?}");
        }
        assert_eq!(keys_for("caf\u{e9}"), Err('\u{e9}'));
        assert_eq!(key_for('\u{276F}'), None);
    }

    /// **The table has one code a key**, and every shifted character's
    /// base is in it.
    #[test]
    fn the_table_names_each_key_once() {
        let mut codes: Vec<u16> = US.iter().map(|(_, _, c)| *c).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), US.len());
        for (s, b) in SHIFTED {
            assert!(code_of(b).is_some(), "{s} is on {b}'s key");
        }
    }

    /// **The gaps stay within 25% of the gap and replay**: the same pid and
    /// count give the same gaps, and a different pid others.
    #[test]
    fn the_gaps_are_bounded_and_replay() {
        let mut a = Gaps::new(4242, 14);
        let mut b = Gaps::new(4242, 14);
        let mut c = Gaps::new(4243, 14);
        let (mut same, mut differ) = (true, false);
        for _ in 0..1000 {
            let (x, y, z) = (a.next_ms(40.0), b.next_ms(40.0), c.next_ms(40.0));
            assert!((30.0..=50.0).contains(&x), "{x}");
            same &= x == y;
            differ |= x != z;
        }
        assert!(same && differ);
    }
}
