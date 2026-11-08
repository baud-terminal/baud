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

/// Replace the pasteboard's contents with `text`. The probe build records
/// the write where it runs and writes nothing (`crate::probe`).
pub fn write_text(text: &str) -> Written {
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
    let board = NSPasteboard::generalPasteboard();
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
pub fn clear() {
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
/// field holds.
pub fn write_find(text: &str) -> Written {
    let board = find_board();
    board.clearContents();
    let kind = NSString::from_str(STRING_TYPE);
    match board.setString_forType(&NSString::from_str(text), &kind) {
        true => Written::Taken,
        false => Written::Refused,
    }
}
