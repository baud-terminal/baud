//! **Whether the system has a Hebrew keyboard layout enabled**, which
//! DECHEBM needs: "DECHEBM is only valid when a Hebrew keyboard language
//! is selected" (`[[source: vt510-dechebm]]`; `baud_core::Term::
//! set_hebrew_layout`). On macOS, an enabled keyboard input source whose
//! languages name Hebrew, read when the app starts and again whenever it
//! gains the focus, as a layout is enabled in System Settings with baud in
//! the background. On Linux the keymap says (`baud_present_wayland`).

use baud_domain::HebrewLayout;

/// **Present when a language names Hebrew**: BCP 47's `he`, alone or with
/// a subtag, as Text Input Sources lists a layout's languages.
pub fn hebrew_among<S: AsRef<str>>(languages: impl IntoIterator<Item = S>) -> HebrewLayout {
    let hebrew = |l: &str| {
        let primary = l.split(['-', '_']).next().unwrap_or("");
        primary.eq_ignore_ascii_case("he")
    };
    match languages.into_iter().any(|l| hebrew(l.as_ref())) {
        true => HebrewLayout::Present,
        false => HebrewLayout::Absent,
    }
}

/// **One caller of the Text Input Sources API at a time, in the unit
/// tests**: HIToolbox aborts the process when two threads call it at once
/// ("Text Input Sources or Text Services Manager API is being called in
/// two threads concurrently"), and libtest runs each test on a thread of
/// its own. The app calls it on the main thread alone: `App::new`, from
/// `Opening::new`, which takes a `MainThreadMarker`; `Driver::set_focus`;
/// and the key path (`Driver::key`, `key_repeat::app_event`), AppKit's
/// responder calls. So the lock is the tests' alone, and the app's build
/// has none.
#[cfg(all(test, target_os = "macos"))]
fn tis() -> std::sync::MutexGuard<'static, ()> {
    static TIS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TIS.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// No lock outside the unit tests: the app calls on the main thread alone.
#[cfg(all(not(test), target_os = "macos"))]
fn tis() -> Unlocked {
    Unlocked
}

/// What [`tis`] holds outside the unit tests: nothing.
#[cfg(all(not(test), target_os = "macos"))]
struct Unlocked;

/// The languages of every enabled keyboard input source: the system's
/// list, read once.
#[cfg(target_os = "macos")]
fn enabled_languages() -> Vec<String> {
    use crate::sys::{
        CFArrayGetCount, CFArrayGetValueAtIndex, CFEqual, CFRelease, CfString,
        TIS_CATEGORY_KEYBOARD_INPUT_SOURCE, TIS_PROPERTY_INPUT_SOURCE_CATEGORY,
        TIS_PROPERTY_INPUT_SOURCE_LANGUAGES, TISCreateInputSourceList, TISGetInputSourceProperty,
    };
    let _tis = tis();
    let mut out = Vec::new();
    // SAFETY: a null dictionary and 0 ask for the enabled sources; the
    // array returned is owned here and released below.
    let list = unsafe { TISCreateInputSourceList(core::ptr::null(), 0) };
    if list.is_null() {
        return out;
    }
    // SAFETY: `list` is a live array until the release below.
    let n = unsafe { CFArrayGetCount(list.as_ptr()) };
    for i in 0..n {
        // SAFETY: `i` is below the count; the source is the array's.
        let source = unsafe { CFArrayGetValueAtIndex(list.as_ptr(), i) };
        // SAFETY: `source` is a live input source; the property and the
        // constant it is compared with are the framework's.
        let keyboard = unsafe {
            let category = TISGetInputSourceProperty(source, TIS_PROPERTY_INPUT_SOURCE_CATEGORY);
            !category.is_null()
                && CFEqual(category, TIS_CATEGORY_KEYBOARD_INPUT_SOURCE.as_ptr()) != 0
        };
        if !keyboard {
            continue;
        }
        // SAFETY: as above; the languages are an array of strings the
        // source owns.
        let langs =
            unsafe { TISGetInputSourceProperty(source, TIS_PROPERTY_INPUT_SOURCE_LANGUAGES) };
        if langs.is_null() {
            continue;
        }
        // SAFETY: `langs` is a live array the source owns.
        let m = unsafe { CFArrayGetCount(langs) };
        for j in 0..m {
            // SAFETY: `j` is below the count; the string is the array's.
            let s = CfString::borrowed(unsafe { CFArrayGetValueAtIndex(langs, j) });
            if let Some(l) = crate::pty::cf_text(s) {
                out.push(l);
            }
        }
    }
    // SAFETY: the array was created above and is released once.
    unsafe { CFRelease(list) };
    out
}

/// **What the system says**, macOS's enabled keyboard input sources.
#[cfg(target_os = "macos")]
pub fn hebrew_layout() -> HebrewLayout {
    hebrew_among(enabled_languages())
}

/// **What a key types with no modifier on the keyboard layout in use**:
/// kitty's key identity, "the lower-case (or more technically,
/// un-shifted) version of the key" (`[[source: kitty-keyboard]]`), which
/// no AppKit event carries -- `charactersIgnoringModifiers` keeps Shift,
/// so Shift on `1` is `!` there. Asked of the layout's own table,
/// `UCKeyTranslate` with no modifier, as kitty's macOS layer asks it
/// (`[[source: kitty-glfw-cocoa-window-851cb80]]`,
/// `vk_to_unicode_key_with_current_layout()`), and with
/// `kUCKeyTranslateNoDeadKeysMask` so a dead key names its own character;
/// kitty passes `kUCKeyTranslateNoDeadKeysBit`, which is 0, so it sets no
/// option there. `None` where the layout has
/// no table, or the key types a control code, a function key's private
/// character or nothing: the caller keeps what it had.
#[cfg(target_os = "macos")]
pub fn unshifted(
    layout: KeyLayout,
    key: &macos_liquid_glass::drawable::KeyPress,
) -> Option<baud_domain::Codepoint> {
    match layout {
        KeyLayout::Selected => unshifted_on_selected(key),
        #[cfg(test)]
        KeyLayout::Installed(id) => unshifted_on_installed(id, key),
    }
}

/// **The keyboard layout a key is read on**: the one the system has
/// selected, which is the layout the user types on and the only one the
/// app reads, or, in a test, an installed layout named by its input source
/// identifier, so what a test sees does not hang on the layout this Mac
/// has selected.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyLayout {
    /// The system's selected layout, read at each key.
    Selected,
    /// An installed layout, by its `kTISPropertyInputSourceID`.
    #[cfg(test)]
    Installed(&'static str),
}

#[cfg(all(test, target_os = "macos"))]
impl KeyLayout {
    /// The US layout, which every Mac has installed.
    pub const US: Self = Self::Installed("com.apple.keylayout.US");
    /// The Hebrew layout, also installed on every Mac: a layout whose
    /// letter keys type no Latin letter.
    pub const HEBREW: Self = Self::Installed("com.apple.keylayout.Hebrew");
}

/// [`unshifted`] on the layout the system has selected.
#[cfg(target_os = "macos")]
fn unshifted_on_selected(
    key: &macos_liquid_glass::drawable::KeyPress,
) -> Option<baud_domain::Codepoint> {
    use crate::sys::{CFRelease, TISCopyCurrentKeyboardLayoutInputSource};
    let _tis = tis();
    // SAFETY: no argument; the source returned is owned here and released
    // below.
    let source = unsafe { TISCopyCurrentKeyboardLayoutInputSource() };
    if source.is_null() {
        return None;
    }
    let c = translate(source.as_ptr(), key);
    // SAFETY: the source was copied above and is released once.
    unsafe { CFRelease(source) };
    c
}

/// `key` through the `uchr` table of the input source `source`.
#[cfg(target_os = "macos")]
fn translate(
    source: *const core::ffi::c_void,
    key: &macos_liquid_glass::drawable::KeyPress,
) -> Option<baud_domain::Codepoint> {
    use crate::sys::{
        CFDataGetBytePtr, DeadKeyState, KeyboardType, LMGetKbdType,
        TIS_PROPERTY_UNICODE_KEY_LAYOUT_DATA, TISGetInputSourceProperty, UC_KEY_ACTION_DISPLAY,
        UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK, UCKeyTranslate, UniChar, UniCharCount,
    };
    // SAFETY: `source` is a live input source; the property is a `CFData`
    // the source owns, or null.
    let data = unsafe { TISGetInputSourceProperty(source, TIS_PROPERTY_UNICODE_KEY_LAYOUT_DATA) };
    if data.is_null() {
        return None;
    }
    let mut dead: DeadKeyState = 0;
    let mut chars: [UniChar; 4] = [0; 4];
    let mut n: UniCharCount = 0;
    // SAFETY: the table is the data's bytes, live while the source is;
    // `chars` holds the four code units the call is told it may write,
    // and `dead` and `n` are this frame's.
    let status = unsafe {
        UCKeyTranslate(
            CFDataGetBytePtr(data).cast(),
            key.key_code,
            UC_KEY_ACTION_DISPLAY,
            0,
            KeyboardType::from(LMGetKbdType()),
            UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK,
            &mut dead,
            4,
            &mut n,
            chars.as_mut_ptr(),
        )
    };
    if status != 0 {
        return None;
    }
    let typed = chars.get(..usize::try_from(n).ok()?)?;
    let c = char::decode_utf16(typed.iter().copied()).next()?.ok()?;
    // A control code, and the private characters AppKit and the layouts
    // give the function keys, are no key's character.
    let private = ('\u{e000}'..='\u{f8ff}').contains(&c);
    match c.is_control() || private {
        true => None,
        false => Some(baud_domain::Codepoint::from_char(c)),
    }
}

/// [`unshifted`] on the installed layout whose input source identifier is
/// `id`; `None` where no installed layout has it.
#[cfg(all(test, target_os = "macos"))]
fn unshifted_on_installed(
    id: &str,
    key: &macos_liquid_glass::drawable::KeyPress,
) -> Option<baud_domain::Codepoint> {
    use crate::sys::{
        CFArrayGetCount, CFArrayGetValueAtIndex, CFEqual, CFRelease, CFStringCreateWithCharacters,
        TIS_PROPERTY_INPUT_SOURCE_ID, TISCreateInputSourceList, TISGetInputSourceProperty, UniChar,
    };
    let id: Vec<UniChar> = id.encode_utf16().collect();
    // SAFETY: the code units are live for the call; the string is owned
    // here and released below.
    let wanted = unsafe {
        CFStringCreateWithCharacters(
            core::ptr::null(),
            id.as_ptr(),
            crate::sys::CfIndex::try_from(id.len()).ok()?,
        )
    };
    // SAFETY: a null dictionary and 1 ask for every installed source; the
    // array is owned here and released below.
    let _tis = tis();
    let list = unsafe { TISCreateInputSourceList(core::ptr::null(), 1) };
    let mut found = None;
    if !list.is_null() && !wanted.is_null() {
        // SAFETY: `list` is a live array until its release.
        let n = unsafe { CFArrayGetCount(list.as_ptr()) };
        for i in 0..n {
            // SAFETY: `i` is below the count; the source is the array's,
            // and its identifier is a string it owns.
            let source = unsafe { CFArrayGetValueAtIndex(list.as_ptr(), i) };
            let named = unsafe {
                let name = TISGetInputSourceProperty(source, TIS_PROPERTY_INPUT_SOURCE_ID);
                !name.is_null() && CFEqual(name, wanted.as_ptr()) != 0
            };
            if named {
                found = translate(source, key);
                break;
            }
        }
    }
    // SAFETY: each was created above, is released once, and only if made.
    unsafe {
        if !list.is_null() {
            CFRelease(list);
        }
        if !wanted.is_null() {
            CFRelease(wanted);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A Hebrew language makes it present**, alone or with a region or
    /// script subtag; any other language, none or an empty one does not.
    #[test]
    fn a_hebrew_language_makes_the_layout_present() {
        assert_eq!(hebrew_among(["en", "he"]), HebrewLayout::Present);
        assert_eq!(hebrew_among(["he-IL"]), HebrewLayout::Present);
        assert_eq!(hebrew_among(["HE_il"]), HebrewLayout::Present);
        assert_eq!(hebrew_among(["en", "hi", "hu"]), HebrewLayout::Absent);
        assert_eq!(hebrew_among(["heb"]), HebrewLayout::Absent);
        assert_eq!(hebrew_among([""]), HebrewLayout::Absent);
        assert_eq!(hebrew_among(Vec::<String>::new()), HebrewLayout::Absent);
    }

    /// **The US layout's own characters, read through its table**: the
    /// letter and digit keys type their unshifted characters -- `a` on
    /// `0x00`, `1` on `0x12`, where Shift types `!` -- the space bar a
    /// space, and the keys that type a control code (Return, `0x24`) or a
    /// function key's private character (Up, `0x7E`) name none.
    /// Perturbation: translate under Shift's Carbon modifier state and `a`
    /// reads `A`.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_layouts_table_gives_each_key_its_unshifted_character() {
        let key = |code: u16| macos_liquid_glass::drawable::KeyPress {
            key_code: code,
            characters: String::new(),
            unmodified: String::new(),
            modifiers: macos_liquid_glass::drawable::Modifiers::default(),
            repeat: false,
        };
        let c = |ch| Some(baud_domain::Codepoint::from_char(ch));
        assert_eq!(
            unshifted(KeyLayout::US, &key(0x00)),
            c('a'),
            "the US layout is installed"
        );
        assert_eq!(unshifted(KeyLayout::US, &key(0x12)), c('1'));
        assert_eq!(unshifted(KeyLayout::US, &key(0x1B)), c('-'));
        assert_eq!(unshifted(KeyLayout::US, &key(0x31)), c(' '));
        assert_eq!(
            unshifted(KeyLayout::US, &key(0x24)),
            None,
            "Return types a control code"
        );
        assert_eq!(
            unshifted(KeyLayout::US, &key(0x7E)),
            None,
            "Up types a private character"
        );
    }

    /// **Two threads reading layouts at once do not abort the process**:
    /// each reads the US and the Hebrew layout's `a` fifty times, as the
    /// tests above do on libtest's threads, and each read is the layout's
    /// own character. HIToolbox aborts on a second concurrent caller, so
    /// without [`tis`] the whole test binary dies here. Break: `tis` taking
    /// no lock, and the binary aborts with HIToolbox's "called in two
    /// threads concurrently".
    #[cfg(target_os = "macos")]
    #[test]
    fn two_threads_reading_layouts_at_once_do_not_abort() {
        let read = || {
            let a = macos_liquid_glass::drawable::KeyPress {
                key_code: 0x00,
                characters: String::new(),
                unmodified: String::new(),
                modifiers: macos_liquid_glass::drawable::Modifiers::default(),
                repeat: false,
            };
            for _ in 0..50 {
                assert_eq!(
                    unshifted(KeyLayout::US, &a),
                    Some(baud_domain::Codepoint::from_char('a'))
                );
                assert_eq!(
                    unshifted(KeyLayout::HEBREW, &a),
                    Some(baud_domain::Codepoint::from_char('\u{5e9}'))
                );
            }
        };
        let other = std::thread::spawn(read);
        read();
        other.join().expect("the other thread's reads");
    }

    /// **The layout a test names is the one read, whatever this Mac has
    /// selected**: the key at `0x00` is `a` on the US layout and `ש`
    /// (U+05E9) on the Hebrew one, so a test held to a layout sees that
    /// layout's characters on a machine typing in either. Perturbation:
    /// read the selected layout in place of the named one, and one of the
    /// two fails on any machine.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_named_layout_is_read_in_place_of_the_selected_one() {
        let a = macos_liquid_glass::drawable::KeyPress {
            key_code: 0x00,
            characters: String::new(),
            unmodified: String::new(),
            modifiers: macos_liquid_glass::drawable::Modifiers::default(),
            repeat: false,
        };
        let c = |ch| Some(baud_domain::Codepoint::from_char(ch));
        assert_eq!(unshifted(KeyLayout::US, &a), c('a'), "the US layout");
        assert_eq!(
            unshifted(KeyLayout::HEBREW, &a),
            c('\u{5e9}'),
            "the Hebrew layout"
        );
    }
}
