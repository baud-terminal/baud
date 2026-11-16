//! **Shift+Page Up and Shift+Page Down page the history**, one behaviour on
//! every system, settled as each key arrives.
//!
//! Shift+Page Up pages back and Shift+Page Down forward, a window's rows at
//! a time; Shift with the keypad's Page Up and Page Down does the same.
//! Plain Page Up and Page Down stay the program's. On a Mac laptop the
//! chord is fn+Shift+Up/Down.
//!
//! Only on the main screen, which keeps the history: on the alternate
//! screen the chord is the program's, as a full-screen program expects (a
//! pager or an editor in application keypad or cursor-key mode). A program
//! on the main screen never receives Shift+Page Up or Shift+Page Down.
//!
//! The screen is the one in force as of the bytes baud has read when the
//! key arrives -- never a flag kept from the last frame, which a program
//! switching screens just before the key would make wrong. Bytes a wait
//! the program asked for still holds (a ReGIS delay, a held query, a
//! one-shot input) are not fed early, and while any are held the key is
//! the program's, as they may switch the screen. The Linux driver feeds
//! the held bytes already owed a feed before it decides; the macOS app
//! feeds them on the turn that releases them, and until that turn they
//! count as held. A switch not yet read is the one the decision cannot
//! see: on Linux, one still in the pty; on macOS, one the reader thread
//! has taken that the app has not yet drained, as well. Both drivers call
//! [`decide`] at the key's event and [`page`] to move the window.

use baud_core::{Screen, Term};
use baud_domain::{KeyCode, KeyEvent, KeyEventKind, KeypadKey, Modifiers, Rows};

/// Which way a history key pages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum HistoryPage {
    Back,
    Forward,
}

/// What the terminal has read and not yet decoded when a key arrives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Undecoded {
    /// Every byte read is on the screen.
    None,
    /// A wait the program asked for holds bytes read and not yet decoded.
    Held,
}

/// **Whether `k` pages the history while `screen` is the one in force**, as
/// of the bytes baud has read, with `undecoded` what a wait still holds of
/// them: Shift with Page Up or keypad Page Up pages back, with Page Down or
/// keypad Page Down forward, pressed or repeating, on the main screen with
/// nothing held. Shift is the only modifier held -- Caps Lock and Num Lock,
/// which are not held keys, do not count -- and a release decides nothing:
/// the press it ends was either the terminal's, and the driver sent the
/// program nothing to end, or the program's.
///
/// **The modifiers are as reported, consumed or not**; the keypad's own
/// case is settled here. With Num Lock on, xkb's Shift+KP_9 is
/// KP_Page_Up with Shift consumed -- Shift taking the keypad back from
/// digits -- and is reported as keypad Page Up with Shift and Num Lock. That
/// is not the chord, so a keypad Page key pages only with Num Lock off;
/// Page Up and Page Down themselves page with Num Lock either way.
pub(crate) fn decide(k: &KeyEvent, screen: Screen, undecoded: Undecoded) -> Option<HistoryPage> {
    let held = k
        .mods
        .without(Modifiers::CAPS_LOCK)
        .without(Modifiers::NUM_LOCK);
    let num_lock = k.mods.has(Modifiers::NUM_LOCK);
    let page = match k.code {
        KeyCode::PageUp => HistoryPage::Back,
        KeyCode::PageDown => HistoryPage::Forward,
        KeyCode::Keypad(KeypadKey::PageUp) if !num_lock => HistoryPage::Back,
        KeyCode::Keypad(KeypadKey::PageDown) if !num_lock => HistoryPage::Forward,
        _ => return None,
    };
    let chord = held == Modifiers::SHIFT && k.kind != KeyEventKind::Release;
    (chord && screen == Screen::Normal && undecoded == Undecoded::None).then_some(page)
}

/// Whether [`page`] moved the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Paged {
    Moved,
    Still,
}

/// **The window a page back or forward through the history**: the
/// window's rows at a time, held to what the history keeps.
pub(crate) fn page(t: &mut Term<'_>, p: HistoryPage) -> Paged {
    let rows = t.geometry().1.index();
    let kept = t.scrollback().len();
    let was = t.primary_window_back().index();
    let back = match p {
        HistoryPage::Back => was.saturating_add(rows).min(kept),
        HistoryPage::Forward => was.saturating_sub(rows),
    };
    if back == was {
        return Paged::Still;
    }
    t.set_window_back(Rows::of_count(back));
    Paged::Moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 10,
        ws_col: 20,
    };

    fn feed(t: &mut Term<'_>, b: &[u8]) {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }

    fn shift(code: KeyCode) -> KeyEvent {
        KeyEvent::press(code, Modifiers::SHIFT)
    }

    /// **The chords, the screens and the screen in force, as of the bytes
    /// baud has read, when the key arrives.** Shift with either Page key or its keypad twin pages, Caps
    /// Lock or Num Lock on; a repeat pages as a press does; plain Page Up,
    /// Ctrl+Shift and a release do not; the alternate screen keeps every
    /// one for the program. Then the race: a program switches to the
    /// alternate screen (`?1049h`) and the key comes with no frame between,
    /// so the program gets it, and back on the main screen (`?1049l`) the
    /// same key pages again. Named break: decide against the screen read
    /// before the switch was fed, as a flag kept from the last frame is --
    /// the key after `?1049h` pages, and this goes red.
    #[test]
    fn shift_page_pages_the_main_screens_history_against_the_screen_in_force() {
        use HistoryPage::{Back, Forward};
        let main = Screen::Normal;
        assert_eq!(
            decide(&shift(KeyCode::PageUp), main, Undecoded::None),
            Some(Back)
        );
        assert_eq!(
            decide(&shift(KeyCode::PageDown), main, Undecoded::None),
            Some(Forward)
        );
        let kp_up = KeyCode::Keypad(KeypadKey::PageUp);
        let kp_down = KeyCode::Keypad(KeypadKey::PageDown);
        assert_eq!(decide(&shift(kp_up), main, Undecoded::None), Some(Back));
        assert_eq!(
            decide(&shift(kp_down), main, Undecoded::None),
            Some(Forward)
        );
        let locked = Modifiers::SHIFT
            .with(Modifiers::CAPS_LOCK)
            .with(Modifiers::NUM_LOCK);
        let caps = KeyEvent::press(KeyCode::PageUp, locked);
        assert_eq!(
            decide(&caps, main, Undecoded::None),
            Some(Back),
            "locks are not held keys"
        );
        // The keypad with Num Lock: off, Shift+KP_Page_Up is the chord; on,
        // keypad Page Up with Shift is Shift+KP_9 with the Shift consumed,
        // and the program's.
        let none = Undecoded::None;
        let num = Modifiers::SHIFT.with(Modifiers::NUM_LOCK);
        assert_eq!(
            decide(&shift(kp_up), main, none),
            Some(Back),
            "Num Lock off"
        );
        let kp9 = KeyEvent::press(kp_up, num);
        assert_eq!(decide(&kp9, main, none), None, "Num Lock on: Shift+KP_9");
        let kp3 = KeyEvent::press(kp_down, num);
        assert_eq!(decide(&kp3, main, none), None, "Num Lock on: Shift+KP_3");
        let main_num = KeyEvent::press(KeyCode::PageUp, num);
        assert_eq!(
            decide(&main_num, main, none),
            Some(Back),
            "Page Up itself, Num Lock on"
        );
        let caps_only =
            KeyEvent::press(KeyCode::PageUp, Modifiers::SHIFT.with(Modifiers::CAPS_LOCK));
        assert_eq!(
            decide(&caps_only, main, Undecoded::None),
            Some(Back),
            "Caps Lock with Shift+Page Up"
        );
        let repeat = KeyEvent {
            kind: KeyEventKind::Repeat,
            ..shift(KeyCode::PageUp)
        };
        assert_eq!(
            decide(&repeat, main, Undecoded::None),
            Some(Back),
            "a held chord keeps paging"
        );
        let release = KeyEvent {
            kind: KeyEventKind::Release,
            ..shift(KeyCode::PageUp)
        };
        assert_eq!(decide(&release, main, Undecoded::None), None);
        let plain = KeyEvent::press(KeyCode::PageUp, Modifiers::NONE);
        assert_eq!(
            decide(&plain, main, Undecoded::None),
            None,
            "plain Page Up is the program's"
        );
        let ctrl_shift = KeyEvent::press(KeyCode::PageUp, Modifiers::SHIFT.with(Modifiers::CTRL));
        assert_eq!(decide(&ctrl_shift, main, Undecoded::None), None);
        for k in [KeyCode::PageUp, KeyCode::PageDown, kp_up, kp_down] {
            assert_eq!(
                decide(&shift(k), Screen::Alternate, Undecoded::None),
                None,
                "{k:?}"
            );
        }

        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let key = shift(KeyCode::PageUp);
        assert_eq!(
            decide(&key, t.screen(), Undecoded::None),
            Some(Back),
            "the control: main screen"
        );
        feed(&mut t, b"\x1b[?1049h");
        assert_eq!(
            decide(&key, t.screen(), Undecoded::None),
            None,
            "after ?1049h, with no frame between, the program gets the key"
        );
        feed(&mut t, b"\x1b[?1049l");
        assert_eq!(
            decide(&key, t.screen(), Undecoded::None),
            Some(Back),
            "back on the main screen"
        );
    }

    /// **A switch read and not yet decoded leaves the key the program's**:
    /// a ReGIS delay (`S(T60)`) holds the rest of its read, `?1049h` among
    /// it, so the screen still reads main; the key comes then, and the
    /// program must get it. At the delay's deadline the held bytes are fed
    /// and the screen is the alternate, the key the program's again. Named
    /// break: decide on the screen alone, ignoring what is held -- the key
    /// pages a screen the program has already left, and this goes red.
    #[test]
    fn a_switch_held_behind_a_wait_leaves_the_key_the_programs() {
        use crate::reads::{Pace, paced};
        use std::time::{Duration, Instant};
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_regis_hold(baud_core::RegisHold::On);
        let mut pace = Pace::new();
        let t0 = Instant::now();
        let log = &mut crate::hostlog::NoLog;
        let read: &[u8] = b"\x1bPpS(T60)\x1b\\\x1b[?1049h";
        let _ = paced(
            &mut t,
            &mut pace,
            t0,
            [(read, ())],
            |_| {},
            |_, _| {},
            log,
            &mut Vec::new(),
        );
        assert!(pace.holds(), "the switch is read and held");
        assert_eq!(t.screen(), Screen::Normal, "not decoded yet");
        let key = shift(KeyCode::PageUp);
        assert_eq!(
            decide(&key, t.screen(), Undecoded::Held),
            None,
            "held: the program gets the key"
        );
        assert_eq!(
            decide(&key, t.screen(), Undecoded::None),
            Some(HistoryPage::Back),
            "the control: with nothing held the same screen would page"
        );
        let late = t0 + Duration::from_secs(2);
        assert!(pace.due(late));
        let none: [(&[u8], ()); 0] = [];
        let _ = paced(
            &mut t,
            &mut pace,
            late,
            none,
            |_| {},
            |_, _| {},
            log,
            &mut Vec::new(),
        );
        assert!(!pace.holds());
        assert_eq!(t.screen(), Screen::Alternate, "fed at the deadline");
        assert_eq!(decide(&key, t.screen(), Undecoded::None), None);
    }

    /// **A page is the window's rows, held to the history**: back a page,
    /// back again to the oldest line kept and no further, then forward to
    /// the live screen and no further. Perturbation: page by one row -- the
    /// first step lands at 1, not 10.
    #[test]
    fn a_page_is_the_windows_rows_held_to_the_history() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        for i in 0..25 {
            feed(&mut t, format!("{i}\r\n").as_bytes());
        }
        let kept = t.scrollback().len();
        assert!(kept > 10, "a history of more than a page: {kept}");
        assert_eq!(page(&mut t, HistoryPage::Back), Paged::Moved);
        assert_eq!(t.primary_window_back().index(), 10);
        while page(&mut t, HistoryPage::Back) == Paged::Moved {}
        assert_eq!(t.primary_window_back().index(), kept, "held to the history");
        assert_eq!(page(&mut t, HistoryPage::Back), Paged::Still);
        while page(&mut t, HistoryPage::Forward) == Paged::Moved {}
        assert_eq!(t.primary_window_back().index(), 0, "the live screen");
        assert_eq!(page(&mut t, HistoryPage::Forward), Paged::Still);
    }
}
