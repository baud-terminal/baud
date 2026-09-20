//! Handing an admitted link to the OS: open it with the default handler
//! for its scheme, or reveal a file in Finder.
//!
//! Safe calls only -- every `NSWorkspace` and `NSURL` call here is a safe
//! fn in the pinned bindings, reached through the window crate's
//! re-exports the way `clipboard.rs` reaches `NSPasteboard` -- and no
//! policy: which URIs get here is `baud-app`'s decision
//! (`baud-app/src/link.rs`), and this module does not repeat it beyond one
//! belt-and-braces check that the scheme `NSURL` read is the scheme the
//! string names. No subprocess, no `open(1)`, no argv: the URL is one
//! `NSURL`.

use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::NSWorkspace;
use macos_liquid_glass::objc2_foundation::{NSArray, NSProcessInfo, NSString, NSURL};

/// What became of a link handed over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opened {
    /// The OS took it.
    Taken,
    /// `NSURL` could not parse it.
    Unparsed,
    /// `NSURL` read a different scheme than the string names.
    Mismatch,
    /// The OS declined: no handler for the scheme, or the handler refused.
    Declined,
}

/// Whether the scheme `NSURL` parsed is the one the string names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Agree {
    Yes,
    No,
}

/// The bytes before the first colon -- what the policy admitted -- against
/// what `NSURL` read, ASCII-case-insensitively. Inert for every input the
/// policy admits, because `NSURL` reads the scheme the same way; it is
/// here so a parse that ever reinterpreted the prefix could not open
/// something the policy did not admit.
pub fn scheme_agrees(url: &str, parsed: &NSURL) -> Agree {
    let named = url.split(':').next().unwrap_or("");
    match parsed.scheme() {
        Some(s) if s.to_string().eq_ignore_ascii_case(named) => Agree::Yes,
        _ => Agree::No,
    }
}

/// Parse, and refuse a parse that reinterpreted the scheme.
pub fn parse(url: &str) -> Result<Retained<NSURL>, Opened> {
    let u = NSURL::URLWithString(&NSString::from_str(url)).ok_or(Opened::Unparsed)?;
    match scheme_agrees(url, &u) {
        Agree::Yes => Ok(u),
        Agree::No => Err(Opened::Mismatch),
    }
}

/// Open with the default handler for the scheme.
pub fn open(url: &str) -> Opened {
    match parse(url) {
        Err(o) => o,
        Ok(u) => {
            if NSWorkspace::sharedWorkspace().openURL(&u) {
                Opened::Taken
            } else {
                Opened::Declined
            }
        }
    }
}

/// Select the file in Finder. Never launches it, which is the whole reason
/// a `file://` link takes this route and not [`open`].
pub fn reveal(url: &str) -> Opened {
    match parse(url) {
        Err(o) => o,
        Ok(u) => {
            NSWorkspace::sharedWorkspace()
                .activateFileViewerSelectingURLs(&NSArray::from_slice(&[&*u]));
            Opened::Taken
        }
    }
}

/// This machine's name, as `file://host/path` links name it.
pub fn host_name() -> String {
    NSProcessInfo::processInfo().hostName().to_string()
}

/// The application LaunchServices would open `url` with, without opening
/// it -- the side-effect-free way to ask whether a scheme has a handler.
pub fn handler(url: &str) -> Option<String> {
    let u = parse(url).ok()?;
    let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&u)?;
    app.absoluteString().map(|s| s.to_string())
}
