//! **The window's export and the dialog's parent**, `xdg-foreign-unstable-v2`
//! (`[[source: xdg-foreign-unstable-v2]]`) and the portal's window
//! identifier (`[[source: xdg-portal-window-identifiers]]`), as types: the
//! export asked for once, its handle taken once and never empty, and the
//! parent named only from that handle.

use baud_domain::dbus::ParentWindow;
use baud_domain::{Export, HandleRefused};

/// **The export is asked for once**: the second ask is refused.
#[test]
fn the_export_is_asked_for_once() {
    let mut e = Export::new();
    assert!(e.ask().is_some(), "the first ask");
    assert!(e.ask().is_none(), "a second ask");
    let _ = e.handle("h-1");
    assert!(e.ask().is_none(), "an ask after the handle");
}

/// **A handle before the ask is refused**, and leaves the export unasked.
#[test]
fn a_handle_before_the_ask_is_refused() {
    let mut e = Export::new();
    assert_eq!(e.handle("h-1").err(), Some(HandleRefused::NotAsked));
    assert_eq!(e.parent(), ParentWindow::Unnamed);
    assert!(e.ask().is_some(), "still unasked");
}

/// **An empty handle is refused**: it would name no window. A handle after
/// it is taken.
#[test]
fn an_empty_handle_is_refused() {
    let mut e = Export::new();
    let _ = e.ask();
    assert_eq!(e.handle("").err(), Some(HandleRefused::Empty));
    assert_eq!(e.parent(), ParentWindow::Unnamed);
    assert_eq!(e.handle("h-1").map(|h| h.as_str()), Ok("h-1"));
}

/// **A second handle keeps the first**: the handle is "a unique string",
/// and the second is refused as `Again`.
#[test]
fn a_second_handle_keeps_the_first() {
    let mut e = Export::new();
    let _ = e.ask();
    assert_eq!(e.handle("h-1").map(|h| h.as_str()), Ok("h-1"));
    assert_eq!(e.handle("h-2").err(), Some(HandleRefused::Again));
    assert_eq!(e.parent().to_string(), "wayland:h-1");
}

/// **The parent is unnamed and prints empty until the handle comes**, and
/// with it prints `wayland:` and the handle.
#[test]
fn the_parent_is_named_by_the_handle() {
    let mut e = Export::new();
    assert_eq!(e.parent(), ParentWindow::Unnamed);
    assert_eq!(e.parent().to_string(), "");
    let _ = e.ask();
    assert_eq!(e.parent().to_string(), "", "asked, no handle yet");
    let _ = e.handle("~12l9jdl.-a");
    assert!(matches!(e.parent(), ParentWindow::Wayland(h) if h.as_str() == "~12l9jdl.-a"));
    assert_eq!(e.parent().to_string(), "wayland:~12l9jdl.-a");
}
