//! **What baud asks of a window and of the application**, beyond drawing:
//! a window raised with or without focus, put behind the others, into the
//! Dock or out of it, into full screen or out of it, or given a frame; the
//! application activated, its Dock icon set. Each is an effect on the desktop, so each is here, one
//! function apiece, and under the test-only `effect-probe` feature each is
//! recorded and the desktop left as it was.

use macos_liquid_glass::objc2::MainThreadMarker;
#[cfg(not(feature = "effect-probe"))]
use macos_liquid_glass::objc2_app_kit::NSApplication;
use macos_liquid_glass::objc2_app_kit::{NSImage, NSWindow};
use macos_liquid_glass::objc2_foundation::NSRect;

#[cfg(feature = "effect-probe")]
use crate::probe::{EffectKind, record};

/// `window` raised in front of the others without taking focus:
/// `orderFrontRegardless`, the bell's raise and `CSI 5 t`'s, each on a
/// [`Turn`](crate::turn::Turn) of its own.
pub fn order_front(t: &crate::turn::Turn<'_>, window: &NSWindow) {
    let _ = t;
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

/// `window` restored from the Dock, `deminiaturize:`: a program's `CSI 1 t`.
pub fn deminiaturize(window: &NSWindow) {
    #[cfg(feature = "effect-probe")]
    {
        let _ = window;
        record(EffectKind::Deminiaturize);
    }
    #[cfg(not(feature = "effect-probe"))]
    window.deminiaturize(None);
}

/// `window` put in the Dock, `miniaturize:`: a program's `CSI 2 t`.
pub fn miniaturize(window: &NSWindow) {
    #[cfg(feature = "effect-probe")]
    {
        let _ = window;
        record(EffectKind::Miniaturize);
    }
    #[cfg(not(feature = "effect-probe"))]
    window.miniaturize(None);
}

/// `window` put behind the others, `orderBack:`, "without changing either
/// the key window or the main window": a program's `CSI 6 t`.
pub fn order_back(window: &NSWindow) {
    #[cfg(feature = "effect-probe")]
    {
        let _ = window;
        record(EffectKind::Back);
    }
    #[cfg(not(feature = "effect-probe"))]
    window.orderBack(None);
}

/// `window` into full screen or out of it, `toggleFullScreen:`: a
/// program's `CSI 10 t`.
pub fn toggle_full_screen(window: &NSWindow) {
    #[cfg(feature = "effect-probe")]
    {
        let _ = window;
        record(EffectKind::FullScreen);
    }
    #[cfg(not(feature = "effect-probe"))]
    window.toggleFullScreen(None);
}

/// `window`'s frame, title bar included, set to `frame` and drawn,
/// `setFrame:display:`: a program's move, resize or maximise, and the
/// status line's rows.
pub fn set_frame(window: &NSWindow, frame: NSRect) {
    #[cfg(feature = "effect-probe")]
    {
        let _ = (window, frame);
        record(EffectKind::Frame);
    }
    #[cfg(not(feature = "effect-probe"))]
    window.setFrame_display(frame, true);
}

/// **A new window, running a program in place of a shell**, opened by
/// `opens` on a main-queue turn of its own (`t`), after the click that
/// asked has returned: a link's `action = "window"`. The window and its
/// terminal are the app's to build, so `opens` builds them; this is the
/// one door to it, so under the test-only `effect-probe` feature the
/// opening is recorded and no window is made.
pub fn open_running(t: &crate::turn::Turn<'_>, opens: impl FnOnce()) {
    let _ = t;
    #[cfg(feature = "effect-probe")]
    {
        let _ = opens;
        record(EffectKind::OpenWindow);
    }
    #[cfg(not(feature = "effect-probe"))]
    opens();
}
