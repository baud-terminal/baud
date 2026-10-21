//! **What the system's accessibility settings ask of the terminal, for
//! both drivers**: the motion preference -- macOS's Reduce Motion, the
//! Linux desktop's `reduced-motion` -- and the contrast one -- macOS's
//! Increase Contrast, the Linux desktop's `contrast` (`[[source:
//! xdg-portal-settings-v2]]`). Each driver reads its own system; what a
//! setting does is decided here once, so it does the same on both.

/// Where the motion preference comes from: the system, or `BAUD_MOTION`,
/// read once at launch, for a check that must not change the owner's
/// settings -- an instrument like `BAUD_SCROLLER`, not a configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MotionSource {
    System,
    Fixed(crate::config::SystemMotion),
}

/// `BAUD_MOTION`: `reduced` or `full` fix the preference; anything else,
/// or nothing, leaves it to the system.
pub(crate) fn motion_override(v: Option<&str>) -> MotionSource {
    use crate::config::SystemMotion;
    match v {
        Some("reduced") => MotionSource::Fixed(SystemMotion::Reduced),
        Some("full") => MotionSource::Fixed(SystemMotion::Full),
        _ => MotionSource::System,
    }
}

/// The motion preference from `source`, asking the system through `read`
/// only when nothing fixed it.
pub(crate) fn system_motion(
    source: MotionSource,
    read: impl FnOnce() -> bool,
) -> crate::config::SystemMotion {
    use crate::config::SystemMotion;
    match source {
        MotionSource::Fixed(m) => m,
        MotionSource::System => match read() {
            true => SystemMotion::Reduced,
            false => SystemMotion::Full,
        },
    }
}

/// Increase Contrast: "UI should be presented with high contrast such as
/// utilizing a less subtle color palette or bolder lines"
/// ([[source: macos-sdk-nsaccessibility-h]] line 93); on Linux the
/// portal's `contrast` 1, "Higher contrast".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Contrast {
    #[default]
    Standard,
    Increased,
}

impl Contrast {
    /// **The floor drawn under this setting**: the configured one, or
    /// under increased contrast [`stricter`]'s.
    pub(crate) fn floor(self, floor: baud_render::Floor) -> baud_render::Floor {
        match self {
            Contrast::Standard => floor,
            Contrast::Increased => stricter(floor),
        }
    }
}

/// **Increase Contrast's floor**: a trigger of at least 3.0 and a target
/// of at least 4.5, the owner's note Y decision 9 (Jev's lean, 0.88/0.87,
/// decisions 718/719), the HIG's "higher contrast color scheme when the
/// system setting Increase Contrast is turned on"
/// ([[source: apple-hig-accessibility]]). It overrides `floor = "off"` while
/// the setting is on, as Reduce Transparency overrides the dim -- decided
/// by analogy in note Y, which the owner can reverse. A reader's stricter
/// floor is kept: "at least" is this reading of "stricter". The brief's
/// "target 4.5" and `max(configured, 4.5)` agree for every config whose
/// floor is at or below 4.5, because the config's `floor()` already parses
/// every target to at least 4.5; they differ only for a floor above 4.5,
/// such as `floor = 7`, which a literal 4.5 would loosen.
fn stricter(floor: baud_render::Floor) -> baud_render::Floor {
    let (trigger, target) = match floor {
        baud_render::Floor::Off => (0.0, 0.0),
        baud_render::Floor::Lift { trigger, target } => (trigger, target),
    };
    baud_render::Floor::Lift {
        trigger: trigger.max(3.0),
        target: target.max(4.5),
    }
}
