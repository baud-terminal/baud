//! **A titled window's bar under the SDK the app links, in points**: the
//! numbers the film's composed strip is laid out by (`chrome.rs`), kept in
//! a file of their own with no dependency so that the window test
//! (`tests/titlebar_mark.rs`) holds these same items to a real window's.
//!
//! Linked against the macOS 27 SDK, AppKit sizes each traffic light's
//! frame 14 by 14 points, where the 14.4 SDK gave 14 by 16
//! (`mark.rs`, `the_lockup_is_one_size_whatever_the_window_buttons_are`),
//! and the strip's left edge, a gap past the zoom button
//! (`strip::left_edge`), ends at 78 points where it ended at 69
//! (`tests/titlebar_mark.rs`, `name_yields_first`). The x of each button
//! is derived from those, not measured: the close button at 9, as before,
//! and the three spaced alike, 9 + 3 x 14 + 3 x 9 = 78, so 9, 32 and 55.
//! The bar is 32 points tall, its content less its content layout rect
//! (`strip::inset_of`), and each button's y centres it there,
//! (32 - 14) / 2 = 9. The window test checks every number against a
//! window's own.

/// **The bar's height**, in points.
pub(crate) const INSET: f64 = 32.0;

/// **The traffic lights' frames**, close, minimise and zoom: x, y, width
/// and height in points, as AppKit's frames are.
pub(crate) const BUTTONS: [(f64, f64, f64, f64); 3] = [
    (9.0, 9.0, 14.0, 14.0),
    (32.0, 9.0, 14.0, 14.0),
    (55.0, 9.0, 14.0, 14.0),
];
