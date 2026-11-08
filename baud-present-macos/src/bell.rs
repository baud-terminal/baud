//! **The system beep**: the bell's sound, AppKit's `NSBeep`, which "plays
//! the system beep" through the user's alert sound and volume. Under the
//! test-only `effect-probe` feature it is recorded and nothing sounds.

/// The system beep.
pub fn beep() {
    #[cfg(feature = "effect-probe")]
    crate::probe::record(crate::probe::EffectKind::Beep);
    #[cfg(not(feature = "effect-probe"))]
    macos_liquid_glass::objc2_app_kit::NSBeep();
}
