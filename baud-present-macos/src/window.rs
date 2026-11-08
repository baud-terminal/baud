//! **What baud asks of a window and of the application**, beyond drawing:
//! a window raised with or without focus, the application activated, its
//! Dock icon set. Each is an effect on the desktop, so each is here, one
//! function apiece, and under the test-only `effect-probe` feature each is
//! recorded and the desktop left as it was.

use macos_liquid_glass::objc2::MainThreadMarker;
#[cfg(not(feature = "effect-probe"))]
use macos_liquid_glass::objc2_app_kit::NSApplication;
use macos_liquid_glass::objc2_app_kit::{NSImage, NSWindow};

#[cfg(feature = "effect-probe")]
use crate::probe::{EffectKind, record};

/// `window` raised in front of the others without taking focus:
/// `orderFrontRegardless`, the bell's raise.
pub fn order_front(window: &NSWindow) {
    #[cfg(feature = "effect-probe")]
    {
        let _ = window;
        record(EffectKind::Front);
    }
    #[cfg(not(feature = "effect-probe"))]
    window.orderFrontRegardless();
}

/// `window` raised and made the key window: what a click on its
/// notification does.
pub fn make_key_and_order_front(window: &NSWindow) {
    #[cfg(feature = "effect-probe")]
    {
        let _ = window;
        record(EffectKind::KeyFront);
    }
    #[cfg(not(feature = "effect-probe"))]
    window.makeKeyAndOrderFront(None);
}

/// The application made the active one.
pub fn activate(mtm: MainThreadMarker) {
    #[cfg(feature = "effect-probe")]
    {
        let _ = mtm;
        record(EffectKind::Activate);
    }
    #[cfg(not(feature = "effect-probe"))]
    NSApplication::sharedApplication(mtm).activate();
}

/// The application's Dock icon set to `icon`.
pub fn set_icon(mtm: MainThreadMarker, icon: &NSImage) {
    #[cfg(feature = "effect-probe")]
    {
        let _ = (mtm, icon);
        record(EffectKind::Icon);
    }
    #[cfg(not(feature = "effect-probe"))]
    macos_liquid_glass::set_application_icon(&NSApplication::sharedApplication(mtm), icon);
}
