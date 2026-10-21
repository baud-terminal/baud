//! **A keysym is a number the keymap names**, and the names and the Unicode
//! equivalents come from the pinned header (`[[source: xkbcommon-keysyms]]`)
//! through `build.rs`. Three rules turn a keysym into text, and they are the
//! library's (`[[source: xkbcommon-h]]`): a Latin-1 keysym is its byte, a
//! keysym at or above `0x0100_0000` is that codepoint, and the rest are the
//! header's table -- with the keypad and the function keys that the X
//! protocol places in `0xff00..=0xffff` mapped to the ASCII they type.

mod table {
    include!(concat!(env!("OUT_DIR"), "/keysyms.rs"));
}

/// A keysym.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Keysym(pub u32);

impl Keysym {
    pub const NO_SYMBOL: Keysym = Keysym(0);
    /// The first codepoint-carrying keysym: `U+xxxx` as `0x0100_0000 | xxxx`.
    const DIRECT: u32 = 0x0100_0000;

    /// The keysym a name denotes: a header name, `Uxxxx`, or a number.
    pub fn named(name: &str) -> Option<Keysym> {
        if let Ok(i) = table::NAMES.binary_search_by(|(n, _)| (*n).cmp(name)) {
            return Some(Keysym(table::NAMES[i].1));
        }
        // `Uxxxx` is the direct form, kept as written: the library does not
        // fold it to the legacy keysym for the same character, and neither
        // does the keymap that names it so.
        if let Some(hex) = name.strip_prefix('U').or_else(|| name.strip_prefix("U+")) {
            if let Ok(cp) = u32::from_str_radix(hex, 16) {
                char::from_u32(cp)?;
                return Some(match cp < 0x100 {
                    true => Keysym(cp),
                    false => Keysym(Keysym::DIRECT | cp),
                });
            }
        }
        if let Some(hex) = name.strip_prefix("0x") {
            return u32::from_str_radix(hex, 16).ok().map(Keysym);
        }
        name.parse::<u32>().ok().map(Keysym)
    }

    /// **The keysym a Compose file names**, as `xkb_keysym_from_name()` reads
    /// one with no flags (`[[source: libxkbcommon-keysym-c]]`): a header
    /// name, exactly; `U` and one to eight hex digits, the direct form above
    /// `U+00FF` -- surrogates included, "code points here, not Unicode
    /// scalars" -- and below it the legacy keysym `xkb_utf32_to_keysym()`
    /// gives; `0x` and one to eight hex digits, as written, up to
    /// `0x1fffffff`; and an `XF86_` name tried again without the
    /// underscore. `NoSymbol` is no keysym. This is not [`Keysym::named`],
    /// which the keymap reader keeps.
    pub fn from_xkb_name(name: &str) -> Option<Keysym> {
        let hex8 = |h: &str| -> Option<u32> {
            match h.len() {
                1..=8 if h.bytes().all(|b| b.is_ascii_hexdigit()) => {
                    u32::from_str_radix(h, 16).ok()
                }
                _ => None,
            }
        };
        let k = if let Ok(i) = table::NAMES.binary_search_by(|(n, _)| (*n).cmp(name)) {
            Keysym(table::NAMES[i].1)
        } else if let Some(h) = name.strip_prefix('U') {
            // Below `U+0100`, `xkb_utf32_to_keysym()`
            // (`[[source: libxkbcommon-keysym-utf-c]]`): Latin-1's
            // printables are themselves, BackSpace to Clear, Return and
            // Escape the control keysyms, DEL `Delete`, NUL nothing, and the
            // rest a legacy keysym where the table has one, else direct.
            match hex8(h)? {
                v @ 0x100..=0x10_ffff => Keysym(Keysym::DIRECT + v),
                v @ (0x20..=0x7e | 0xa0..=0xff) => Keysym(v),
                v @ (0x08..=0x0b | 0x0d | 0x1b) => Keysym(v | 0xff00),
                0x7f => Keysym(0xffff),
                v => char::from_u32(v)
                    .filter(|&c| c != '\0')
                    .map_or(Keysym::NO_SYMBOL, Keysym::of_char),
            }
        } else if let Some(h) = name.strip_prefix("0x") {
            Keysym(hex8(h).filter(|&v| v <= 0x1fff_ffff)?)
        } else if let Some(rest) = name.strip_prefix("XF86_") {
            return Keysym::from_xkb_name(&alloc::format!("XF86{rest}"));
        } else {
            return None;
        };
        (k != Keysym::NO_SYMBOL).then_some(k)
    }

    /// **A modifier key's keysym**, which a Compose sequence does not see:
    /// `xkb_keysym_is_modifier()`, Shift_L to Hyper_R, ISO_Lock to
    /// ISO_Level5_Lock, Mode_switch and Num_Lock -- "approximately what
    /// libX11 does as well" (`[[source: libxkbcommon-compose-state-c]]`).
    pub fn is_modifier(self) -> bool {
        matches!(self.0, 0xffe1..=0xffee | 0xfe01..=0xfe13 | 0xff7e | 0xff7f)
    }

    /// The keysym for a codepoint: the legacy keysym where the header names
    /// one, else the direct form.
    pub fn of_char(c: char) -> Keysym {
        let cp = u32::from(c);
        if (0x20..0x7f).contains(&cp) || (0xa0..=0xff).contains(&cp) {
            return Keysym(cp);
        }
        // The table is sorted by keysym, not by codepoint; a linear search
        // once per keysym conversion is a few thousand compares and this
        // is not a hot path.
        for (k, u) in table::UNICODE {
            if *u == cp && *k < Keysym::DIRECT {
                return Keysym(*k);
            }
        }
        Keysym(Keysym::DIRECT | cp)
    }

    /// The codepoint this keysym types, if any.
    pub fn to_char(self) -> Option<char> {
        let v = self.0;
        if (0x20..0x7f).contains(&v) || (0xa0..=0xff).contains(&v) {
            return char::from_u32(v);
        }
        if v >= Keysym::DIRECT {
            return char::from_u32(v - Keysym::DIRECT);
        }
        // Keypad: KP_0..KP_9 and the operators type their ASCII.
        match v {
            0xffb0..=0xffb9 => return char::from_u32(v - 0xffb0 + u32::from(b'0')),
            0xff80 => return Some(' '),
            0xff89 => return Some('\t'),
            0xff8d => return Some('\r'),
            0xffbd => return Some('='),
            0xffaa => return Some('*'),
            0xffab => return Some('+'),
            0xffac => return Some(','),
            0xffad => return Some('-'),
            0xffae => return Some('.'),
            0xffaf => return Some('/'),
            // The control keys the X protocol keeps in the low byte.
            0xff08 | 0xff09 | 0xff0a | 0xff0b | 0xff0d | 0xff1b => return char::from_u32(v & 0xff),
            0xffff => return Some('\u{7f}'),
            _ => {}
        }
        match table::UNICODE.binary_search_by_key(&v, |(k, _)| *k) {
            Ok(i) => char::from_u32(table::UNICODE[i].1),
            Err(_) => None,
        }
    }

    /// The upper-case form, if the keysym is a cased letter; else itself.
    ///
    /// The capitalisation transformation the library applies under Lock.
    pub fn to_upper(self) -> Keysym {
        let Some(c) = self.to_char() else {
            return self;
        };
        let mut up = c.to_uppercase();
        match (up.next(), up.next()) {
            (Some(u), None) if u != c => Keysym::of_char(u),
            _ => self,
        }
    }

    pub fn to_lower(self) -> Keysym {
        let Some(c) = self.to_char() else {
            return self;
        };
        let mut lo = c.to_lowercase();
        match (lo.next(), lo.next()) {
            (Some(l), None) if l != c => Keysym::of_char(l),
            _ => self,
        }
    }

    pub fn is_lower(self) -> bool {
        self.to_char().is_some_and(|c| c.is_lowercase())
    }

    pub fn is_upper(self) -> bool {
        self.to_char().is_some_and(|c| c.is_uppercase())
    }

    /// A keypad keysym, `KP_*`.
    pub fn is_keypad(self) -> bool {
        (0xff80..=0xffbd).contains(&self.0) || (0x1100_0000..=0x1100_ffff).contains(&self.0)
    }

    /// The name the header gives it, for messages.
    pub fn name(self) -> Option<&'static str> {
        table::NAMES
            .iter()
            .find(|(_, v)| *v == self.0)
            .map(|(n, _)| *n)
    }
}
