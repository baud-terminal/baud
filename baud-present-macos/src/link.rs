//! Handing an admitted link to the OS: open it with the default handler
//! for its scheme, or reveal a file in Finder -- on the main queue's next
//! turn, never inside the callback that asked ([`later`]).
//!
//! Safe calls only -- every `NSWorkspace` and `NSURL` call here is a safe
//! fn in the pinned bindings, reached through the window crate's
//! re-exports the way `clipboard.rs` reaches `NSPasteboard` -- and no
//! policy: which URIs get here is `baud-app`'s decision
//! (`baud-app/src/link.rs`), and this module does not repeat it beyond one
//! belt-and-braces check that the scheme `NSURL` read is the scheme the
//! string names. No subprocess, no `open(1)`, no argv: the URL is one
//! `NSURL`.

use macos_liquid_glass::objc2::rc::{Retained, autoreleasepool};
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

/// What a link asks of the OS.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ask {
    /// Open with the default handler for the scheme.
    Open,
    /// Select the file in Finder. Never launches it, which is the whole
    /// reason a `file://` link takes this route and not `Open`.
    Reveal,
}

/// Who carries an [`Ask`] out: [`workspace`] in the app, a recorder in a
/// test. A plain `fn`, so the block that takes it to the main queue is
/// `Send` and holds nothing of the caller's.
pub type Hand = fn(Ask, &str) -> Opened;

/// **`NSWorkspace` itself**, called only from the block [`later`] queues.
///
/// `activateFileViewerSelectingURLs:` is not a quick call: AppKit performs
/// it as a Finder service, and the services handler waits for the reply in
/// a nested event loop (`+[NSServicesMenuHandler _performServiceFromEntry:...]`
/// -> `_checkForCancellationUntilDate` -> `nextEventMatchingMask:`), which
/// runs whatever the main run loop has queued -- the view's own `wake:` and
/// `presentSurface:` performs among them. Made from a display-link tick it
/// re-entered the view while the tick held its responder, and the panic
/// that followed aborted the app (`DrawableView`'s responder is a
/// `RefCell`, borrowed for the whole of each callback). `openURL:` is a
/// synchronous LaunchServices call whose header says nothing of the run
/// loop either way; it goes the same route, since nothing a tick does
/// should wait on another process.
pub fn workspace(ask: Ask, url: &str) -> Opened {
    autoreleasepool(|_| match parse(url) {
        Err(o) => o,
        Ok(u) => match ask {
            Ask::Open => {
                if NSWorkspace::sharedWorkspace().openURL(&u) {
                    Opened::Taken
                } else {
                    Opened::Declined
                }
            }
            Ask::Reveal => {
                NSWorkspace::sharedWorkspace()
                    .activateFileViewerSelectingURLs(&NSArray::from_slice(&[&*u]));
                Opened::Taken
            }
        },
    })
}

/// **Hand a link to the OS after the callback that asked has returned.**
///
/// The parse is answered now -- `Unparsed` and `Mismatch` are the caller's
/// to say at once, and `NSURL` runs no loop -- and the hand-over is a block
/// on the main queue, which runs on a later turn of the main run loop and
/// so outside every responder callback, none of which runs the loop
/// itself, with no responder borrowed: a nested loop inside `hand` then
/// delivers the view's performs into a responder that is free to take
/// them. `done` hears what `hand` answered,
/// on the main thread, after `hand` has returned; it is the caller's way
/// back in, through a waker, and holds no borrow across the hand-over.
/// The block runs through [`crate::unwind::guard`], so a panic in it is
/// stopped there rather than in libdispatch, which would abort.
pub fn later(
    ask: Ask,
    url: &str,
    hand: Hand,
    done: impl FnOnce(Opened) + Send + 'static,
) -> Result<(), Opened> {
    autoreleasepool(|_| parse(url).map(drop))?;
    let url = url.to_owned();
    dispatch2::DispatchQueue::main().exec_async(move || {
        crate::unwind::guard("a link handed to the OS", (), move || done(hand(ask, &url)));
    });
    Ok(())
}

/// This machine's name, as `file://host/path` links name it.
pub fn host_name() -> String {
    NSProcessInfo::processInfo().hostName().to_string()
}

/// The application LaunchServices would open `url` with, without opening
/// it -- the side-effect-free way to ask whether a scheme has a handler.
pub fn handler(url: &str) -> Option<String> {
    autoreleasepool(|_| {
        let u = parse(url).ok()?;
        let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&u)?;
        app.absoluteString().map(|s| s.to_string())
    })
}

/// **Whether an app opens `url`**, asked as [`handler`] asks: none when the
/// string does not parse as the URL it names. Called off the main thread by
/// `baud-app`'s link worker with a fixed address, never a program's bytes:
/// `NSWorkspace` carries no main-actor mark in the SDK's header, and every
/// object the call makes is released in its own autorelease pool, since that
/// thread has none of AppKit's.
pub fn has_handler(url: &str) -> Option<bool> {
    autoreleasepool(|_| {
        let u = parse(url).ok()?;
        Some(
            NSWorkspace::sharedWorkspace()
                .URLForApplicationToOpenURL(&u)
                .is_some(),
        )
    })
}
