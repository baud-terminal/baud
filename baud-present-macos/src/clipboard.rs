//! The general pasteboard, as a selection's copy and a paste read it: one
//! string type, replacing what was there. And the find pasteboard, the string a find searches for.

use macos_liquid_glass::objc2_app_kit::NSPasteboard;
use macos_liquid_glass::objc2_foundation::NSString;

/// The UTI of a plain UTF-8 string on the pasteboard -- the value of
/// AppKit's `NSPasteboardTypeString`, spelled here so no extern static has
/// to be read, which the bindings make `unsafe`.
const STRING_TYPE: &str = "public.utf8-plain-text";

/// Whether the pasteboard took the text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Written {
    Taken,
    Refused,
}

/// Replace the pasteboard's contents with `text`, on a turn of its own
/// (`crate::turn`): a pasteboard write is a desktop effect, never made
/// inside a callback. The probe build records the write where it runs and
/// writes nothing (`crate::probe`).
pub fn write_text(_turn: &crate::turn::Turn<'_>, text: &str) -> Written {
    #[cfg(feature = "effect-probe")]
    {
        let _ = text;
        crate::probe::record(crate::probe::EffectKind::Copy);
        Written::Taken
    }
    #[cfg(not(feature = "effect-probe"))]
    written(text)
}

#[cfg(not(feature = "effect-probe"))]
fn written(text: &str) -> Written {
    written_to(&NSPasteboard::generalPasteboard(), text)
}

/// Replace `board`'s contents with `text`, as the one string type
/// [`STRING_TYPE`]: the text's UTF-8 bytes, which AppKit also lists under
/// the legacy name it keeps for that type.
#[cfg(not(feature = "effect-probe"))]
fn written_to(board: &NSPasteboard, text: &str) -> Written {
    board.clearContents();
    let kind = NSString::from_str(STRING_TYPE);
    match board.setString_forType(&NSString::from_str(text), &kind) {
        true => Written::Taken,
        false => Written::Refused,
    }
}

/// Empty the pasteboard: what a program's `OSC 52` whose data is neither
/// base64 nor `?` asks for -- `[[source: ctlseqs]]`, "the selection is
/// cleared".
pub fn clear(_turn: &crate::turn::Turn<'_>) {
    #[cfg(feature = "effect-probe")]
    crate::probe::record(crate::probe::EffectKind::Clear);
    #[cfg(not(feature = "effect-probe"))]
    NSPasteboard::generalPasteboard().clearContents();
}

/// The pasteboard's string, if it holds one.
pub fn read_text() -> Option<String> {
    let board = NSPasteboard::generalPasteboard();
    let kind = NSString::from_str(STRING_TYPE);
    board.stringForType(&kind).map(|s| s.to_string())
}

/// The find pasteboard's name -- the value of AppKit's
/// `NSPasteboardNameFind`, read from the SDK and spelled here as
/// [`STRING_TYPE`] is: "The pasteboard that holds information about the
/// current state of the active application's find panel" (`[[source:
/// apple-nspasteboard-name-find]]`), shared by every application.
const FIND_BOARD: &str = "Apple CFPasteboard find";

fn find_board() -> macos_liquid_glass::objc2::rc::Retained<NSPasteboard> {
    NSPasteboard::pasteboardWithName(&NSString::from_str(FIND_BOARD))
}

/// The find pasteboard's string, if it holds one: the last search, made
/// here or in another application.
pub fn read_find() -> Option<String> {
    let kind = NSString::from_str(STRING_TYPE);
    find_board().stringForType(&kind).map(|s| s.to_string())
}

/// Replace the find pasteboard's string with `text`, the query a find
/// field holds, on a turn of its own.
pub fn write_find(_turn: &crate::turn::Turn<'_>, text: &str) -> Written {
    #[cfg(feature = "effect-probe")]
    {
        let _ = text;
        crate::probe::record(crate::probe::EffectKind::FindText);
        Written::Taken
    }
    #[cfg(not(feature = "effect-probe"))]
    found(text)
}

#[cfg(not(feature = "effect-probe"))]
fn found(text: &str) -> Written {
    written_to(&find_board(), text)
}

#[cfg(all(test, not(feature = "effect-probe")))]
mod tests {
    use super::*;

    /// A pasteboard of this test's own, by name: the general one and the
    /// find one are never written.
    const TEST_BOARD: &str = "baud test pasteboard";

    /// **Copied text is written as UTF-8**: pointed Hebrew, an accented
    /// letter precomposed and decomposed, and a four-byte emoji, written
    /// through the writer a copy takes onto a pasteboard of the test's own,
    /// read back under `public.utf8-plain-text` as exactly the text's UTF-8
    /// bytes, that type the first the pasteboard lists.
    ///
    /// The type is spelled here, not read from [`STRING_TYPE`], so the
    /// writer's type is checked rather than echoed. Perturbation: set
    /// [`STRING_TYPE`] to `public.plain-text` or
    /// `com.apple.traditional-mac-plain-text`, and the pasteboard holds no
    /// UTF-8 string and lists that type first.
    #[test]
    fn copied_text_is_written_as_its_utf8_bytes() {
        const TEXT: &str =
            "\u{5e9}\u{5b8}\u{5c1}\u{5dc}\u{5d5}\u{5b9}\u{5dd} caf\u{e9} cafe\u{301} \u{1f980}";
        let board = NSPasteboard::pasteboardWithName(&NSString::from_str(TEST_BOARD));
        assert_eq!(written_to(&board, TEXT), Written::Taken);
        const UTF8: &str = "public.utf8-plain-text";
        let bytes = board
            .dataForType(&NSString::from_str(UTF8))
            .map(|d| d.to_vec());
        assert_eq!(bytes.as_deref(), Some(TEXT.as_bytes()));
        let types: Vec<String> = board
            .types()
            .map(|t| t.to_vec().iter().map(|k| k.to_string()).collect())
            .unwrap_or_default();
        assert_eq!(types.first().map(String::as_str), Some(UTF8), "{types:?}");
    }
}
