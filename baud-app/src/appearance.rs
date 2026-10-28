//! **Which half of the palette the Linux window draws**: the config's
//! `theme`, and under `auto` the desktop's preference as the Settings portal
//! reports it (`bus.rs`). A desktop that prefers dark draws the dark half
//! and one that prefers light the light; one that states no preference --
//! value 0, which the portal says an unknown value means too, and what GNOME
//! reports when its dark style is off (`[[source: kitty-themes-rst]]`) --
//! draws the light half, libadwaita's rule for an application's default
//! scheme (`[[source: libadwaita-style-manager]]`, `get_is_dark`). With no
//! portal heard, `auto` draws dark, as on macOS before the system has said
//! anything.

use crate::access::{Contrast, MotionSource};
use crate::bus::{Origin, Scheme};
use crate::config::{SystemMotion, ThemeChoice};
use crate::palette::Half;

/// The half `choice` draws once the desktop has said `scheme`, or before.
pub(crate) fn half_for(choice: ThemeChoice, scheme: Option<Scheme>) -> Half {
    match (choice, scheme) {
        (ThemeChoice::Light, _) => Half::Light,
        (ThemeChoice::Dark, _) => Half::Dark,
        (ThemeChoice::Auto, Some(Scheme::Dark) | None) => Half::Dark,
        (ThemeChoice::Auto, Some(Scheme::Light | Scheme::NoPreference)) => Half::Light,
    }
}

/// **Only a change of the half drawn switches the palette**, as on macOS,
/// where a switch drops every colour a program set: the half to switch to,
/// or none.
pub(crate) fn switch(choice: ThemeChoice, drawn: Half, scheme: Scheme) -> Option<Half> {
    let next = half_for(choice, Some(scheme));
    (next != drawn).then_some(next)
}

/// **What a switch does to a program's colours**: a change the user made
/// on the desktop drops them with the old half, as a macOS flip does (note
/// Y, decision 5), and the answer to the launch's read keeps them. That
/// read races the shell's first output, and a palette set by a shell hook
/// before the answer came would otherwise be wiped at every launch on a
/// light desktop, where on macOS the first callback lands before any
/// output and drops nothing.
pub(crate) fn overrides(origin: Origin) -> baud_core::Overrides {
    match origin {
        Origin::Changed => baud_core::Overrides::Drop,
        Origin::Read => baud_core::Overrides::Keep,
    }
}

/// **The desktop's contrast preference as the cells are drawn**: the
/// portal's "Higher contrast" is macOS's Increase Contrast, and no
/// preference, or a value it does not know, is the standard contrast
/// (`[[source: xdg-portal-settings-v2]]`).
pub(crate) fn contrast_of(c: crate::bus::Contrast) -> Contrast {
    match c {
        crate::bus::Contrast::Higher => Contrast::Increased,
        crate::bus::Contrast::NoPreference => Contrast::Standard,
    }
}

/// **The desktop's motion preference as the effects take it**: the
/// portal's "Reduced motion" is macOS's Reduce Motion, unless
/// `BAUD_MOTION` fixed the preference, which the desktop then never moves.
pub(crate) fn motion_of(source: MotionSource, m: crate::bus::Motion) -> SystemMotion {
    crate::access::system_motion(source, || m == crate::bus::Motion::Reduced)
}

/// **The theme a half is drawn with**: `palette::half_theme`, the floor
/// as the contrast setting asks for it ([`Contrast::floor`]) -- what
/// macOS's `accessible` does to the floor, so a light/dark switch under
/// higher contrast keeps the stricter floor. Linux has no transparency
/// key to follow, so the dim is the half's.
pub(crate) fn drawn_theme(
    half: Half,
    glass: &crate::config::Glass,
    floor: baud_render::Floor,
    halves: &crate::palette::Halves,
    contrast: Contrast,
) -> baud_render::Theme {
    crate::palette::half_theme(half, glass, contrast.floor(floor), halves)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **`auto` follows the desktop and a pinned theme does not**; no
    /// preference is light, and no portal dark. Perturbation: draw no
    /// preference dark -- the no-preference assertion fails.
    #[test]
    fn auto_follows_the_portal_and_a_pinned_theme_stays() {
        use ThemeChoice::{Auto, Dark, Light};
        assert_eq!(half_for(Auto, None), Half::Dark);
        assert_eq!(half_for(Auto, Some(Scheme::Dark)), Half::Dark);
        assert_eq!(half_for(Auto, Some(Scheme::Light)), Half::Light);
        assert_eq!(half_for(Auto, Some(Scheme::NoPreference)), Half::Light);
        for s in [Scheme::Dark, Scheme::Light, Scheme::NoPreference] {
            assert_eq!(half_for(Dark, Some(s)), Half::Dark);
            assert_eq!(half_for(Light, Some(s)), Half::Light);
            assert_eq!(switch(Dark, Half::Dark, s), None, "pinned");
        }
        assert_eq!(switch(Auto, Half::Dark, Scheme::Light), Some(Half::Light));
        assert_eq!(switch(Auto, Half::Light, Scheme::Light), None, "no change");
    }

    /// **The launch's answer keeps a program's colours and a change drops
    /// them**, through the core: a palette entry set before the portal
    /// answered survives the read, and goes at the user's change, with
    /// `?2031`'s one report. Perturbation: drop on the read -- the entry is
    /// gone and the first assertion fails.
    #[test]
    fn the_launch_answer_keeps_a_programs_colours_and_a_change_drops_them() {
        use baud_core::{Owned, Term};
        use baud_domain::{BytePos, ReadLen, WinSize};
        const WS: WinSize = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        let dark = baud_render::Theme::dark();
        let light = baud_render::Theme::light();
        crate::host::tell_defaults(&mut t, &dark, baud_core::Overrides::Keep);
        feed(&mut t, b"\x1b]4;1;rgb:12/34/56\x1b\\\x1b[?2031h");
        let ask = |t: &mut Term<'_>| {
            t.clear_reply();
            feed(t, b"\x1b]4;1;?\x1b\\");
            let r = t.reply().to_vec();
            t.clear_reply();
            r
        };
        let set = ask(&mut t);
        crate::host::tell_defaults(&mut t, &light, overrides(Origin::Read));
        t.clear_reply();
        assert_eq!(ask(&mut t), set, "the launch's answer keeps it");
        crate::host::tell_defaults(&mut t, &dark, overrides(Origin::Changed));
        let report = t.reply().to_vec();
        assert_ne!(ask(&mut t), set, "the user's change drops it");
        assert_eq!(report, b"\x1b[?997;1n", "and reports the change once");
    }

    /// **Higher contrast on the desktop draws as Increase Contrast does on
    /// macOS**: the floor a trigger of at least 3.0 and a target of at
    /// least 4.5 on either half, over `floor = "off"` too, a stricter
    /// floor kept; the standard contrast, the control, draws the floor as
    /// configured, and the dim is the half's either way. Perturbation:
    /// map higher contrast to the standard one -- the stricter floor's
    /// assertion fails.
    #[test]
    fn higher_contrast_draws_the_stricter_floor_on_either_half() {
        use baud_render::Floor;
        let lift = |trigger, target| Floor::Lift { trigger, target };
        let glass = crate::config::Glass::defaults();
        let halves = crate::palette::Halves::default();
        let higher = contrast_of(crate::bus::Contrast::Higher);
        let normal = contrast_of(crate::bus::Contrast::NoPreference);
        assert_eq!((higher, normal), (Contrast::Increased, Contrast::Standard));
        for half in [Half::Dark, Half::Light] {
            for (configured, want) in [
                (Floor::DEFAULT, lift(3.0, 4.5)),
                (Floor::Off, lift(3.0, 4.5)),
                (lift(2.0, 4.5), lift(3.0, 4.5)),
                (lift(7.0, 7.0), lift(7.0, 7.0)),
            ] {
                let plain = crate::palette::half_theme(half, &glass, configured, &halves);
                let up = drawn_theme(half, &glass, configured, &halves, higher);
                assert_eq!(up.floor, want, "{half:?} {configured:?}");
                assert_eq!(up.bg_alpha, plain.bg_alpha, "the dim is the half's");
                let off = drawn_theme(half, &glass, configured, &halves, normal);
                assert_eq!(off.floor, configured, "the control");
            }
        }
    }

    /// **Reduced motion on the desktop is macOS's Reduce Motion, and
    /// `BAUD_MOTION` outranks it**: the portal's reduced is reduced and no
    /// preference full, while a fixed preference is what it fixed
    /// whatever the desktop says. Perturbation: ask the portal under a
    /// fixed source -- the fixed-full assertion fails.
    #[test]
    fn the_desktops_motion_is_reduce_motion_unless_fixed() {
        use crate::bus::Motion;
        use SystemMotion::{Full, Reduced};
        let sys = MotionSource::System;
        assert_eq!(motion_of(sys, Motion::Reduced), Reduced);
        assert_eq!(motion_of(sys, Motion::NoPreference), Full);
        for fixed in [Full, Reduced] {
            for m in [Motion::Reduced, Motion::NoPreference] {
                assert_eq!(motion_of(MotionSource::Fixed(fixed), m), fixed);
            }
        }
    }
}
