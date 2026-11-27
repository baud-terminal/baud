//! **The keyboard state a compositor reports, and the lookup on it.**
//!
//! On Wayland the modifier and group state arrives ready-made in
//! `wl_keyboard.modifiers`: the depressed, latched and locked masks and the
//! locked group, as the compositor computed them by running the keymap's
//! actions. The client applies them (`update`) and asks what a key produces
//! under them: the level from the key's type, the keysym at that level, and
//! the text -- with the two transformations `[[source: xkbcommon-h]]`
//! specifies: Lock active and not consumed capitalises, and Control active
//! and not consumed turns the text into its control character.

use crate::compile::{CONTROL, Keycode, Keymap, LOCK, ModMask, REAL};
use crate::keysym::Keysym;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub depressed: ModMask,
    pub latched: ModMask,
    pub locked: ModMask,
}

impl Mods {
    pub fn effective(self) -> ModMask {
        (self.depressed | self.latched | self.locked) & REAL
    }
}

/// What a key produces under the state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lookup {
    pub layout: usize,
    pub level: usize,
    pub sym: Keysym,
    /// The modifiers the lookup consumed.
    pub consumed: ModMask,
}

#[derive(Clone, Debug)]
pub struct State<'k> {
    keymap: &'k Keymap,
    mods: Mods,
    group: usize,
}

impl<'k> State<'k> {
    pub fn new(keymap: &'k Keymap) -> State<'k> {
        State {
            keymap,
            mods: Mods::default(),
            group: 0,
        }
    }

    /// `wl_keyboard.modifiers`: the three masks and the locked group.
    pub fn update(&mut self, mods: Mods, group: usize) {
        self.mods = mods;
        self.group = match self.keymap.num_groups {
            0 => 0,
            n => group % n,
        };
    }

    pub fn mods(&self) -> Mods {
        self.mods
    }

    pub fn group(&self) -> usize {
        self.group
    }

    /// The level and keysym of `keycode`, if the keymap has the key.
    pub fn lookup(&self, keycode: Keycode) -> Option<Lookup> {
        let key = self.keymap.key(keycode)?;
        // A keycode the symbols section never bound has nothing to look up:
        // the library answers an invalid layout and level, and so does this.
        if key.groups.iter().all(|g| g.levels.is_empty()) {
            return None;
        }
        // A key with fewer groups than the keyboard wraps.
        let layout = self.group % key.groups.len();
        let group = &key.groups[layout];
        let t = &self.keymap.types[group.type_index];
        let active = self.mods.effective() & t.mask;
        let (level, preserved) = t
            .map
            .iter()
            .find(|(m, _)| *m == active)
            .map(|(m, l)| {
                let p = t
                    .preserve
                    .iter()
                    .find(|(pm, _)| pm == m)
                    .map_or(0, |(_, p)| *p);
                (*l, p)
            })
            .unwrap_or((0, 0));
        let level = level.min(group.levels.len().saturating_sub(1));
        let syms = group.levels.get(level).map(Vec::as_slice).unwrap_or(&[]);
        let sym = match syms {
            [s] => *s,
            _ => Keysym::NO_SYMBOL,
        };
        Some(Lookup {
            layout,
            level,
            sym,
            consumed: t.mask & !preserved,
        })
    }

    /// The keysym, with the capitalisation transformation applied.
    pub fn sym(&self, keycode: Keycode) -> Keysym {
        let Some(l) = self.lookup(keycode) else {
            return Keysym::NO_SYMBOL;
        };
        match self.mods.effective() & LOCK != 0 && l.consumed & LOCK == 0 {
            true => l.sym.to_upper(),
            false => l.sym,
        }
    }

    /// The text the key types under the state, with both transformations.
    pub fn text(&self, keycode: Keycode) -> Option<char> {
        let l = self.lookup(keycode)?;
        let eff = self.mods.effective();
        let mut c = l.sym.to_char()?;
        if eff & LOCK != 0 && l.consumed & LOCK == 0 {
            let mut up = c.to_uppercase();
            if let (Some(u), None) = (up.next(), up.next()) {
                c = u;
            }
        }
        if eff & CONTROL != 0 && l.consumed & CONTROL == 0 {
            c = control(c)?;
        }
        Some(c)
    }

    pub fn repeats(&self, keycode: Keycode) -> bool {
        self.keymap.key(keycode).is_some_and(|k| k.repeats)
    }
}

/// The control character a printable types with Control held: the X
/// protocol's table, which is ASCII masked to five bits from `@` through
/// `~` (so the backtick is NUL and the letters their control codes), and
/// a handful of others.
fn control(c: char) -> Option<char> {
    let v = u32::from(c);
    Some(match c {
        '@'..='~' | ' ' => char::from_u32(v & 0x1f)?,
        '2' => '\0',
        '3' => '\u{1b}',
        '4' => '\u{1c}',
        '5' => '\u{1d}',
        '6' => '\u{1e}',
        '7' => '\u{1f}',
        '8' => '\u{7f}',
        '/' => '\u{1f}',
        '?' => '\u{7f}',
        _ => c,
    })
}
