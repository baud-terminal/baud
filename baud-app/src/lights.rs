//! **The activity lights in the macOS title strip**: RD for bytes read from
//! the child, SD for bytes the kernel took from baud, at the strip's right
//! (the owner: "on the title - most right LEDs like old style modems
//! rx/tx"; F1-20).
//!
//! **Where**: an image view in baud's own container, as the app icon is,
//! re-placed on every resize -- the title strip is AppKit's, drawn over the
//! glass, and the owner's window crate has no titlebar-accessory API
//! (note AH, 5.1), so baud draws where it already draws (Jev, SUBVIEW,
//! 1.00/1.00). The pixels are baud's renderer's
//! (`Renderer::activity_lights`), one picture for each of the four states,
//! drawn when the scale, the appearance or Dim Flashing Lights changes; a
//! light that changes swaps the view's picture and builds no frame of the
//! grid. The clocks step on the display link's tick, which already runs,
//! from the stamp the reader already puts on each read (the latest a
//! drain brought) and a stamp on each write the kernel took any of:
//! nothing is added to a byte's path, and nothing is timed off the link.
//!
//! The Linux window has no title bar -- no compositor decorations are asked
//! for and none are drawn -- so the lights are this driver's alone.

use baud_render::{
    ActivityLights, Argb, Backdrop, ByShown, GROUND_DARK, GROUND_LIGHT, Glow, LightsImage,
    LightsLook, Renderer, Shown,
};
use macos_liquid_glass::drawable::Appearance;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSImage, NSImageScaling, NSImageView, NSView, NSWindow,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

/// What the four pictures were drawn for: a change of any of them draws
/// them again.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Drawn {
    pub(crate) scale: CGFloat,
    pub(crate) glow: Glow,
    /// The appearance the body under the strip draws in.
    pub(crate) appearance: Appearance,
    /// Every grey the strip can be over that body (`under`).
    pub(crate) backdrop: Backdrop,
}

/// The window's body, which the transparent title strip shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Body {
    /// The standard material, `[glass] style = "standard"`.
    Standard,
    /// Liquid Glass, regular or clear.
    Glass,
}

/// **The greys the strip can be over `body` drawn in `appearance`**: the
/// standard material's measured range for its half, and anything from
/// black to white over the glass -- the backdrops the text floor reads
/// against (`backdrop` in `app.rs`, note AS and note AT).
pub(crate) fn under(body: Body, appearance: Appearance) -> Backdrop {
    match (body, appearance) {
        (Body::Standard, Appearance::Dark) => Backdrop::under_window_dark(),
        (Body::Standard, Appearance::Light) => Backdrop::under_window_light(),
        (Body::Glass, _) => Backdrop::any(),
    }
}

/// What a tick does to the lights.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Step {
    /// Nothing changed, or the lights are off.
    Nothing,
    /// A light changed: the view takes another of the pictures it has.
    Swap,
    /// The pictures are for another scale, appearance or glow, or there are
    /// none: draw them, then show the state.
    Redraw,
}

/// **What a tick owes the lights**: nothing while they are off; the four
/// pictures again when what they were drawn for is not what is wanted; a
/// swap when only the state moved.
pub(crate) fn step(
    setting: ActivityLights,
    wanted: Drawn,
    have: Option<Drawn>,
    shown: Shown,
    on_view: Option<Shown>,
) -> Step {
    match setting {
        ActivityLights::Off => Step::Nothing,
        ActivityLights::On if have != Some(wanted) => Step::Redraw,
        ActivityLights::On if on_view != Some(shown) => Step::Swap,
        ActivityLights::On => Step::Nothing,
    }
}

/// How a lit light is drawn under the Dim Flashing Lights setting.
pub(crate) fn glow(flashing: baud_present_macos::settings::Flashing) -> Glow {
    match flashing {
        baud_present_macos::settings::Flashing::Full => Glow::Full,
        baud_present_macos::settings::Flashing::Dimmed => Glow::Dimmed,
    }
}

/// **Where the view goes in its superview**, bottom-left origin: flush
/// with the strip's top, `right` in from the right edge, the strip's
/// `inset` tall.
pub(crate) fn frame(container: NSSize, inset: CGFloat, right: CGFloat, width: CGFloat) -> NSRect {
    NSRect::new(
        NSPoint::new(
            (container.width - right - width).max(0.0),
            container.height - inset,
        ),
        NSSize::new(width, inset),
    )
}

/// The view, its pictures, and what it shows.
pub(crate) struct Strip {
    view: Option<Retained<NSImageView>>,
    pictures: Option<(Drawn, ByShown<Retained<NSImage>>)>,
    on_view: Option<Shown>,
    /// The strip's width in points, from the renderer's face.
    width: CGFloat,
    /// The gap kept at the strip's right, the traffic lights' at its left.
    right: CGFloat,
    /// The looks drawn so far this session, by what they were made from: a
    /// dimmed look scans every grey of the material for its caps, 41 to 54
    /// ms on this machine, so a flip back to an appearance, or a move to a
    /// display of another scale, does not scan again.
    looks: Vec<(LookKey, LightsLook)>,
}

/// What a look is made from: the lit and letter colours, the strip's grey,
/// the glow and the backdrop.
type LookKey = ([Argb; 2], Argb, Argb, Glow, Backdrop);

impl Strip {
    pub(crate) fn new(right: CGFloat) -> Strip {
        Strip {
            view: None,
            pictures: None,
            on_view: None,
            width: 0.0,
            right,
            looks: Vec::new(),
        }
    }

    /// The appearance or the face changed under the pictures: the next
    /// tick draws them again.
    pub(crate) fn stale(&mut self) {
        self.pictures = None;
        self.on_view = None;
    }

    /// **The lights at `shown`**, under `setting`, in `container`'s strip
    /// of `inset` points: the view put up, its pictures drawn and swapped,
    /// or taken down.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn show(
        &mut self,
        setting: ActivityLights,
        wanted: Drawn,
        shown: Shown,
        renderer: &Renderer<'_, '_>,
        window: &NSWindow,
        container: &NSView,
        inset: CGFloat,
    ) {
        if setting == ActivityLights::Off {
            if let Some(v) = self.view.take() {
                v.removeFromSuperview();
            }
            self.on_view = None;
            return;
        }
        let have = self.pictures.as_ref().map(|(d, _)| *d);
        match step(setting, wanted, have, shown, self.on_view) {
            Step::Nothing => return,
            Step::Redraw => {
                let appearance = window.effectiveAppearance();
                let look = self.look(&appearance, wanted);
                let Some(drawn) = pictures(renderer, &look, wanted, inset) else {
                    return;
                };
                self.width = CGFloat::from(renderer.activity_lights_width());
                self.pictures = Some((wanted, drawn));
            }
            Step::Swap => {}
        }
        if self.view.is_none() {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let v = NSImageView::new(mtm);
            v.setImageScaling(NSImageScaling::ScaleNone);
            v.setToolTip(Some(&NSString::from_str(
                "RD: bytes coming in from the program. SD: bytes going out to it.",
            )));
            container.addSubview(&v);
            self.view = Some(v);
        }
        self.place(container, inset);
        if let (Some(v), Some((_, p))) = (&self.view, &self.pictures) {
            v.setImage(Some(p.get(shown)));
            self.on_view = Some(shown);
        }
    }

    /// The look for `wanted` under `appearance`: AppKit's colours, a dark
    /// light mixed into that appearance's strip, from the session's looks
    /// when one was made from the same colours and glow.
    fn look(&mut self, appearance: &NSAppearance, wanted: Drawn) -> LightsLook {
        let (lit, label) = baud_present_macos::lights::colours(appearance);
        let ground = match wanted.appearance {
            Appearance::Dark => GROUND_DARK,
            Appearance::Light => GROUND_LIGHT,
        };
        let key = (lit, label, ground, wanted.glow, wanted.backdrop);
        if let Some((_, l)) = self.looks.iter().find(|(k, _)| *k == key) {
            return *l;
        }
        let look = LightsLook::new(lit, label, ground, wanted.glow, wanted.backdrop);
        self.looks.push((key, look));
        look
    }

    /// Re-place the view at the strip's right: on every resize.
    pub(crate) fn place(&self, container: &NSView, inset: CGFloat) {
        if let Some(v) = &self.view {
            v.setFrame(frame(container.frame().size, inset, self.right, self.width));
        }
    }
}

/// The four pictures of `look` at `wanted`'s scale, `inset` points tall.
fn pictures(
    renderer: &Renderer<'_, '_>,
    look: &LightsLook,
    wanted: Drawn,
    inset: CGFloat,
) -> Option<ByShown<Retained<NSImage>>> {
    let mut img = LightsImage::new();
    let mut one = |s: Shown| {
        renderer.activity_lights(look, s, wanted.scale as f32, inset as f32, &mut img)?;
        baud_present_macos::lights::picture(&img, wanted.scale)
    };
    let [dark, rd, sd, both] = Shown::ALL.map(&mut one);
    Some(ByShown {
        dark: dark?,
        rd: rd?,
        sd: sd?,
        both: both?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_render::Light;

    fn at() -> Drawn {
        Drawn {
            scale: 2.0,
            glow: Glow::Full,
            appearance: Appearance::Dark,
            backdrop: Backdrop::under_window_dark(),
        }
    }
    const RD: Shown = Shown {
        rd: Light::Lit,
        sd: Light::Dark,
    };

    /// **A light that changes swaps a picture, and only a change of what
    /// they were drawn for draws them again**: a new state swaps, the same
    /// state does nothing, a new scale, appearance or glow redraws, and
    /// off does nothing whatever moved.
    #[test]
    fn a_change_of_light_swaps_and_a_change_of_look_redraws() {
        use ActivityLights::{Off, On};
        let at = at();
        assert_eq!(step(On, at, None, RD, None), Step::Redraw);
        assert_eq!(step(On, at, Some(at), RD, Some(Shown::DARK)), Step::Swap);
        assert_eq!(step(On, at, Some(at), RD, Some(RD)), Step::Nothing);
        for other in [
            Drawn { scale: 1.0, ..at },
            Drawn {
                glow: Glow::Dimmed,
                ..at
            },
            Drawn {
                appearance: Appearance::Light,
                ..at
            },
            Drawn {
                backdrop: Backdrop::any(),
                ..at
            },
        ] {
            assert_eq!(step(On, other, Some(at), RD, Some(RD)), Step::Redraw);
        }
        assert_eq!(step(Off, at, None, RD, None), Step::Nothing);
        assert_eq!(
            step(Off, at, Some(at), RD, Some(Shown::DARK)),
            Step::Nothing
        );
    }

    /// **The strip's greys are the body's**: the standard material's
    /// range for the half it draws in, anything over the glass.
    #[test]
    fn the_strip_is_read_over_the_bodys_greys() {
        assert_eq!(
            under(Body::Standard, Appearance::Dark),
            Backdrop::under_window_dark()
        );
        assert_eq!(
            under(Body::Standard, Appearance::Light),
            Backdrop::under_window_light()
        );
        for a in [Appearance::Dark, Appearance::Light] {
            assert_eq!(under(Body::Glass, a), Backdrop::any());
        }
    }

    /// **The view sits at the strip's right, the strip's height, its top
    /// the window's**, and a window narrower than the lights keeps them
    /// inside it.
    #[test]
    fn the_lights_sit_at_the_strips_right() {
        let f = frame(NSSize::new(800.0, 500.0), 28.0, 9.0, 50.0);
        assert_eq!(
            f,
            NSRect::new(NSPoint::new(741.0, 472.0), NSSize::new(50.0, 28.0))
        );
        let f = frame(NSSize::new(40.0, 100.0), 28.0, 9.0, 50.0);
        assert_eq!(f.origin.x, 0.0);
    }

    /// **The four pictures, headless, under either appearance and glow**:
    /// drawn by the renderer in baud's embedded JetBrains Mono Bold with
    /// AppKit's colours, each the strip's width and height in points at the
    /// scale asked, and a lit picture differs from the dark one.
    #[test]
    fn the_pictures_are_drawn_at_the_strips_size() {
        use baud_domain::{Cols, Rows};
        use baud_render::{Blend, PxSize, Stack, Theme};
        use macos_liquid_glass::objc2_foundation::NSString;
        let faces = [baud_font::Face::parse(crate::embedded::BOLD.bytes).unwrap()];
        let r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(28.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(6),
            Rows::of_index(2),
        )
        .unwrap();
        for (name, appearance) in [
            ("NSAppearanceNameAqua", Appearance::Light),
            ("NSAppearanceNameDarkAqua", Appearance::Dark),
        ] {
            let ns = NSAppearance::appearanceNamed(&NSString::from_str(name)).unwrap();
            for glow in [Glow::Full, Glow::Dimmed] {
                let wanted = Drawn {
                    scale: 2.0,
                    glow,
                    appearance,
                    backdrop: under(Body::Standard, appearance),
                };
                let mut strip = Strip::new(9.0);
                let look = strip.look(&ns, wanted);
                assert_eq!(strip.look(&ns, wanted), look, "made once");
                assert_eq!(strip.looks.len(), 1, "and kept");
                let p = pictures(&r, &look, wanted, 28.0).expect("four pictures");
                let width = CGFloat::from(r.activity_lights_width());
                for s in Shown::ALL {
                    let size = p.get(s).size();
                    assert_eq!(size.height, 28.0, "{name}");
                    assert!(
                        (size.width - width).abs() <= 0.5,
                        "{name}: {size:?} for {width}"
                    );
                }
                assert_ne!(
                    p.dark.TIFFRepresentation(),
                    p.both.TIFFRepresentation(),
                    "{name} {glow:?}: lit is not dark"
                );
            }
        }
    }

    /// **The tick lights them, the pump and the write feed them**: the
    /// driver steps the clocks on the display link's tick and shows them
    /// through this module, a drain that brought bytes gives RD its latest
    /// read's stamp, a write the kernel took any of stamps SD and one it
    /// took nothing of does not, the window's appearance makes the
    /// pictures stale, and a resize re-places them.
    #[test]
    fn the_driver_ticks_feeds_and_places_the_lights() {
        const SRC: &str = include_str!("app.rs");
        let body = |head: &str| {
            let at = SRC.find(head).unwrap_or_else(|| panic!("{head}"));
            let rest = &SRC[at..];
            rest.split_once("\n    }\n").map_or(rest, |(b, _)| b)
        };
        let frame = body(concat!("    fn frame(&mut self, ", "target: f64) {"));
        assert!(
            frame.contains("self.show_lights(target);"),
            "the tick shows them"
        );
        let show = body(concat!("    fn show_lights(&mut self, ", "target: f64) {"));
        assert!(show.contains("self.app.activity.step(now)"), "{show}");
        assert!(show.contains("self.lights.show("), "{show}");
        let pump = body(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"));
        assert!(pump.contains("self.app.activity.received(last.at);"), "RD");
        let flush = body(concat!("    fn flush_reply(&mut self) ", "-> Sent {"));
        assert!(
            flush.contains("Wrote::All => {\n                self.activity.sent(host_now());"),
            "{flush}"
        );
        assert!(
            flush.contains("Wrote::Part => {\n                self.activity.sent(host_now());"),
            "{flush}"
        );
        assert!(
            flush.contains("Wrote::Nothing => Sent::Nothing,"),
            "{flush}"
        );
        let style = body(concat!("    fn apply_style(&mut self) ", "{"));
        assert!(style.contains("self.lights.stale();"), "{style}");
        let resized = body(concat!(
            "    fn relayout(&mut self, ",
            "size: NSSize, scale: CGFloat) -> Drew {"
        ));
        assert!(resized.contains("self.place_lights();"), "{resized}");
    }
}
