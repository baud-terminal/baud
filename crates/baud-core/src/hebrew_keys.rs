//! **DECHEBM, `CSI ? 35 h` / `CSI ? 35 l`** (`[[source: vt510-dechebm]]`):
//! the keyboard's Group 2, Hebrew, while set, and only while the system
//! has a Hebrew keyboard layout (`baud_domain::HebrewLayout`), "only valid
//! when a Hebrew keyboard language is selected".
//!
//! **A key** the system's layout typed its own unshifted character on
//! sends the letter the standard Israeli layout puts on that physical key
//! (`baud_domain::hebrew_key`), so on a US, AZERTY or QWERTZ layout the
//! key where the US layout has `a` sends shin. **With Shift** a key types
//! SI-1452's second level, which is the US layout's but for six keys that
//! type their mirror (`baud_domain::hebrew_key_shifted`): the key where
//! the US layout types `{` sends `}`, `(` sends `)` and `<` sends `>`, and
//! each the other way. Both apps send a shifted key as the character it
//! typed, Shift consumed, with the US layout's character on its place
//! beside it where that differs, so a key is taken as shifted when it
//! typed what the US layout or SI-1452 types there with Shift. On Wayland
//! a key whose unshifted character is the US layout's names no place, and
//! is sent as typed, an open departure (the `?35` row of
//! `docs/vt-coverage.md`): a US layout's Shift+`9` and a Hebrew layout's
//! arrive alike. A letter Caps Lock made a capital is sent as typed, as
//! SI-1452's letter keys type their capital with Caps Lock. A key held
//! with Control, Alt, Meta, Super or Hyper is not mapped.
//!
//! **Under 7-bit characters**, DECHEM reset ("Reset (7-bit char)",
//! `[[source: vt510-chapter4]]`), "Makes DEC 7-bit Hebrew the keyboard
//! character set": a Hebrew letter, mapped or typed on the system's
//! Hebrew layout, is sent as its code 60 to 7A; and "Causes received
//! characters in the range of 60 to 7A (hex) to be displayed using the
//! DEC 7-bit Hebrew when ASCII is designated or invoked": ASCII in GL is
//! drawn as that set ([`Paint::hebrew_group_two`]). "When 8-bit characters
//! are selected", DECHEM set, the letter is sent as itself, and "It does
//! not affect ... the processing of received characters".
//!
//! **At Level 4**, the VT510's, as DECRLM and DECHEM act: the page's "or
//! the terminal is set to VT100 mode" is not reached, an open departure,
//! which acting at every level would close.

use baud_domain::{
    CellFlags, Charset, Codepoint, HebrewEncoding, HebrewKeyboard, HebrewLayout, KeyCode, KeyEvent,
    Modifiers, Width, hebrew_key, hebrew_key_shifted, seven_bit_hebrew, us_shifted,
};

use crate::Term;
use crate::paint::Paint;

impl<M: crate::bands::BandMode> Paint<'_, '_, M> {
    /// **Whether ASCII in GL is drawn as DEC 7-bit Hebrew**: DECHEBM's
    /// Group 2 active, with a Hebrew layout, under 7-bit characters.
    pub(crate) fn hebrew_group_two(&self) -> bool {
        self.cold.hebrew_keyboard == HebrewKeyboard::Hebrew
            && self.cold.hebrew_layout == HebrewLayout::Present
            && self.cold.hebrew == HebrewEncoding::National
    }
}

/// **The second level on the key where the US layout types `us`
/// unshifted**: what the US layout types there with Shift, and what
/// SI-1452 does.
fn second_level(us: Codepoint) -> Option<(Codepoint, Codepoint)> {
    let shifted = us_shifted(us)?;
    Some((shifted, hebrew_key_shifted(shifted).unwrap_or(shifted)))
}

impl Term<'_> {
    /// **Whether the system has a Hebrew keyboard layout enabled**, which
    /// DECHEBM needs (`crate::hebrew_keys`): the embedder's to say, at
    /// start and whenever it may have changed, and `Absent` until it does.
    /// No sequence changes it, `RIS` included. GL's set is drawn again by
    /// it.
    pub fn set_hebrew_layout(&mut self, l: HebrewLayout) {
        if self.cold.hebrew_layout == l {
            return;
        }
        self.cold.hebrew_layout = l;
        struct Invoke;
        impl crate::term::PaintJob for Invoke {
            type Out = (
                Charset,
                CellFlags,
                Option<(Codepoint, Width, crate::paint::Pen)>,
            );
            fn run<M: crate::bands::BandMode>(
                self,
                p: &mut crate::paint::Paint<'_, '_, M>,
            ) -> Self::Out {
                p.invoke();
                (p.charset, p.flags, p.last_cp)
            }
        }
        (self.charset, self.flags, self.last_cp) = self.outside_read(Invoke);
    }

    /// What [`Term::set_hebrew_layout`] last set.
    pub fn hebrew_layout(&self) -> HebrewLayout {
        self.cold.hebrew_layout
    }

    /// **Key `e` through DECHEBM's Group 2**, or `e` while it is not
    /// active.
    pub(crate) fn hebrew_mapped(&self, e: KeyEvent) -> KeyEvent {
        if self.cold.hebrew_keyboard != HebrewKeyboard::Hebrew
            || self.cold.hebrew_layout != HebrewLayout::Present
        {
            return e;
        }
        let (KeyCode::Char(identity), Some(text)) = (e.code, e.text) else {
            return e;
        };
        // Shift and the locks leave the key mapped; any other modifier
        // held does not.
        let locks = Modifiers::SHIFT
            .with(Modifiers::CAPS_LOCK)
            .with(Modifiers::NUM_LOCK);
        if !e.mods.without(locks).is_none() {
            return e;
        }
        let typed_hebrew = seven_bit_hebrew(text).is_some();
        let place = e.base_layout.unwrap_or(identity);
        let (letter, us) = match typed_hebrew {
            true => (text, e.base_layout),
            // The second level: the key typed what the US layout or
            // SI-1452 types with Shift on its place, which the app names
            // beside the character.
            false if second_level(place).is_some_and(|(us, si)| text == us || text == si) => {
                match second_level(place) {
                    Some((_, si)) if si != text => (si, Some(place)),
                    _ => return e,
                }
            }
            false if text == identity => match hebrew_key(place) {
                Some(h) => (h, Some(place)),
                None => return e,
            },
            false => return e,
        };
        let sent = match self.cold.hebrew {
            HebrewEncoding::National => seven_bit_hebrew(letter).unwrap_or(letter),
            HebrewEncoding::Multinational => letter,
        };
        KeyEvent {
            code: KeyCode::Char(letter),
            text: Some(sent),
            base_layout: us,
            ..e
        }
    }
}
