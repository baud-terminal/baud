//! The general pasteboard, as a selection's copy and a paste read it: one
//! string type, replacing what was there. `docs/research/notes/k-selection.md`
//! is the spec.

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

/// Replace the pasteboard's contents with `text`.
pub fn write_text(text: &str) -> Written {
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
