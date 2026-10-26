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

/// The languages of every enabled keyboard input source: the system's
/// list, read once.
#[cfg(target_os = "macos")]
fn enabled_languages() -> Vec<String> {
    use crate::sys::{
        CFArrayGetCount, CFArrayGetValueAtIndex, CFEqual, CFRelease, CfString,
        TIS_CATEGORY_KEYBOARD_INPUT_SOURCE, TIS_PROPERTY_INPUT_SOURCE_CATEGORY,
        TIS_PROPERTY_INPUT_SOURCE_LANGUAGES, TISCreateInputSourceList, TISGetInputSourceProperty,
    };
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
}
