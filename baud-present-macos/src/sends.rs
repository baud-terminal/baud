//! **The counted seams**: the AppKit and Core Animation calls a turn or a
//! frame makes, each in a function of its own that makes the call and,
//! under the test-only `effect-probe` feature, counts it by its [`Kind`]
//! ([`crate::probe::sent`]). The probe counts only while baud's own work
//! is on the main thread's stack -- a responder method or a turn -- so the
//! frame rig's harness, which runs between them, never moves the count.
//!
//! What a still screen sends is then a count: the event rig's CPU twin of
//! R1 holds it at none over the ticks it offers and the bare wakes it
//! posts at quiet. `turn_doors`' `every_send_on_a_turn_goes_through_a_counted_seam`
//! holds the other half: in the driver's sources, a function that sends
//! to AppKit is one of these or is named on its table of functions that
//! run only on an event of their own.
//!
//! The display link's door is counted where it is (`display_link.rs`,
//! `probe::link`), and the rig adds its sends to these.

use macos_liquid_glass::drawable::DrawableView;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{
    NSCursor, NSImage, NSImageView, NSView, NSWindow, NSWindowOcclusionState, NSWindowStyleMask,
};
use macos_liquid_glass::objc2_foundation::{NSRect, NSSize, NSString};
use macos_liquid_glass::window::GlassWindow;
use objc2_io_surface::IOSurface;

/// **What a seam sent**, by the property it set or read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A layer's contents: a frame's surface shown (`set_surface`).
    LayerContents,
    /// A view's frame set, or its size.
    LayerFrame,
    /// A view shown or hidden.
    Hidden,
    /// An image view's picture.
    Image,
    /// The pointer's shape set, or hidden until it moves.
    Cursor,
    /// The window's title.
    Title,
    /// The minimized window's title.
    MiniTitle,
    /// A property of the window or a view read.
    WindowRead(Read),
}

/// **Which property a [`Kind::WindowRead`] read**, so a red says which.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Read {
    /// The view's window.
    Window,
    /// A view's frame.
    ViewFrame,
    /// Whether the window is key.
    Key,
    /// Whether any of the window is visible.
    Occlusion,
    /// Whether the window is minimized.
    Miniaturized,
    /// Whether the window is full screen.
    FullScreen,
    /// Whether a sheet is attached.
    Sheet,
}

impl Kind {
    /// Every kind: where each one's count is kept is its place here.
    pub const ALL: [Kind; KINDS] = [
        Kind::LayerContents,
        Kind::LayerFrame,
        Kind::Hidden,
        Kind::Image,
        Kind::Cursor,
        Kind::Title,
        Kind::MiniTitle,
        Kind::WindowRead(Read::Window),
        Kind::WindowRead(Read::ViewFrame),
        Kind::WindowRead(Read::Key),
        Kind::WindowRead(Read::Occlusion),
        Kind::WindowRead(Read::Miniaturized),
        Kind::WindowRead(Read::FullScreen),
        Kind::WindowRead(Read::Sheet),
    ];

    /// The kind's name, for the rig's line.
    pub const fn name(self) -> &'static str {
        match self {
            Kind::LayerContents => "contents",
            Kind::LayerFrame => "frame",
            Kind::Hidden => "hidden",
            Kind::Image => "image",
            Kind::Cursor => "cursor",
            Kind::Title => "title",
            Kind::MiniTitle => "mini_title",
            Kind::WindowRead(Read::Window) => "read_window",
            Kind::WindowRead(Read::ViewFrame) => "read_view_frame",
            Kind::WindowRead(Read::Key) => "read_key",
            Kind::WindowRead(Read::Occlusion) => "read_occlusion",
            Kind::WindowRead(Read::Miniaturized) => "read_miniaturized",
            Kind::WindowRead(Read::FullScreen) => "read_full_screen",
            Kind::WindowRead(Read::Sheet) => "read_sheet",
        }
    }
}

/// How many kinds there are.
pub const KINDS: usize = 14;

/// **Whether a view, or the pointer, is shown.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seen {
    Shown,
    Hidden,
}

/// A send of `kind`, counted under the probe; nothing otherwise.
fn count(kind: Kind) {
    #[cfg(feature = "effect-probe")]
    crate::probe::sent(kind);
    #[cfg(not(feature = "effect-probe"))]
    let _ = kind;
}

/// **A frame's surface shown** on the view's layer.
pub fn set_surface(view: &DrawableView, surface: &IOSurface) {
    count(Kind::LayerContents);
    view.set_surface(surface);
}

/// **A view's frame set.**
pub fn set_frame(view: &NSView, frame: NSRect) {
    count(Kind::LayerFrame);
    view.setFrame(frame);
}

/// **A view's size set**, its origin kept.
pub fn set_frame_size(view: &NSView, size: NSSize) {
    count(Kind::LayerFrame);
    view.setFrameSize(size);
}

/// **A view shown or hidden.**
pub fn set_seen(view: &NSView, seen: Seen) {
    count(Kind::Hidden);
    view.setHidden(seen == Seen::Hidden);
}

/// **An image view's picture set.**
pub fn set_image(view: &NSImageView, image: &NSImage) {
    count(Kind::Image);
    view.setImage(Some(image));
}

/// **The pointer shown as `cursor`.**
pub fn set_cursor(cursor: &NSCursor) {
    count(Kind::Cursor);
    cursor.set();
}

/// **The pointer hidden until it moves**, or shown again.
pub fn pointer_until_moved(seen: Seen) {
    count(Kind::Cursor);
    NSCursor::setHiddenUntilMouseMoves(seen == Seen::Hidden);
}

/// **The window's title set.**
pub fn set_title(window: &GlassWindow, title: &str) {
    count(Kind::Title);
    window.set_title(title);
}

/// **The minimized window's title set.**
pub fn set_mini_title(window: &NSWindow, title: &str) {
    count(Kind::MiniTitle);
    window.setMiniwindowTitle(Some(&NSString::from_str(title)));
}

/// **The view's window**, if it has one.
pub fn window_of(view: &NSView) -> Option<Retained<NSWindow>> {
    count(Kind::WindowRead(Read::Window));
    view.window()
}

/// **A view's frame.**
pub fn frame_of(view: &NSView) -> NSRect {
    count(Kind::WindowRead(Read::ViewFrame));
    view.frame()
}

/// **Whether the window is key.**
pub fn is_key(window: &NSWindow) -> bool {
    count(Kind::WindowRead(Read::Key));
    window.isKeyWindow()
}

/// **Whether any of the window is visible** on screen.
pub fn is_seen(window: &NSWindow) -> bool {
    count(Kind::WindowRead(Read::Occlusion));
    window
        .occlusionState()
        .contains(NSWindowOcclusionState::Visible)
}

/// **Whether the window is minimized.**
pub fn is_miniaturized(window: &NSWindow) -> bool {
    count(Kind::WindowRead(Read::Miniaturized));
    window.isMiniaturized()
}

/// **Whether the window is full screen.**
pub fn is_full_screen(window: &NSWindow) -> bool {
    count(Kind::WindowRead(Read::FullScreen));
    window.styleMask().contains(NSWindowStyleMask::FullScreen)
}

/// **Whether a sheet is attached to the window.**
pub fn has_sheet(window: &NSWindow) -> bool {
    count(Kind::WindowRead(Read::Sheet));
    window.attachedSheet().is_some()
}
