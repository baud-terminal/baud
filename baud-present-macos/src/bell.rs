//! **The system beep**: the bell's sound, AppKit's `NSBeep`, which "plays
//! the system beep" through the user's alert sound and volume, made on a
//! [`Turn`](crate::turn::Turn) of its own. Under the test-only
//! `effect-probe` feature it is recorded and nothing sounds.

/// The system beep.
pub fn beep(t: &crate::turn::Turn<'_>) {
    let _ = t;
    #[cfg(feature = "effect-probe")]
    crate::probe::record(crate::probe::EffectKind::Beep);
    #[cfg(not(feature = "effect-probe"))]
    macos_liquid_glass::objc2_app_kit::NSBeep();
}
