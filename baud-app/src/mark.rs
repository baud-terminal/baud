//! **baud's logo in the titlebar strip**: the lockup, the word baud and
//! the mark ❯█, in a leading titlebar accessory, which AppKit puts a
//! button's gap right of the traffic lights (`strip.rs`), centred in the
//! title bar's height.
//!
//! **The mark keeps the size it had alone.** The lockups are
//! `assets/logo/baud-lockup-{dark,light}-tight.svg`, drawn by
//! `tools/baud-pictures` with the mark's cell 168 units tall,
//! as in `baud-mark-amber-tight.svg`, the mark that stood here before.
//! That mark's 300-unit box was fitted across a square a little larger
//! than a traffic light, so the lockup is drawn at the same points per
//! unit, the word added before the mark at the lockup's own spacing. Its
//! box is snug to the ink across, so the view's right edge is the
//! cursor's, and centred on the cell's rows, so the mark's cell sits on
//! the strip's centre line, where the mark alone sat.
//!
//! **The word follows the window's light or dark.** The dark lockup's
//! word is the brand sheet's light type, the light lockup's its dark type,
//! each with its own amber (`assets/logo/README.md`). Which one shows is
//! the window's `effectiveAppearance`, not a global preference key:
//! AppKit's header names "a view's, window's, or the app's
//! effectiveAppearance" as the way to determine the appearance
//! ([[source: sdk27-nsappearance-h]] line 27), and it is the appearance
//! AppKit draws the traffic lights in beside it. A picture in an image
//! view does not follow a flip by itself, so [`Mark::follow`] swaps it,
//! called from the driver's appearance path on every flip. Under
//! `[glass] style = "glass"` the strip's material follows Icon & widget
//! style (`material_appearance` in `app.rs`) while the lockup follows the
//! window, so when the two settings disagree, the word can sit on glass of
//! its own lightness. That case is open.
//!
//! **In a narrow window the lockup stays whole.** The activity lights
//! start a light's gap past its end ([`Place::lights_left`]) and drop
//! their leftmost lights first when the strip has no room for them all
//! (`lights.rs`). The window's title text is hidden by the window crate's
//! titled shape, so nothing else in the strip competes, and the window's
//! smallest width, 200 points, still holds the lockup.
//!
//! **Where Apple's guidance differs, recorded as open.** The Branding
//! guidelines say "Resist the temptation to display your logo throughout
//! your app or game unless it's essential for providing context" and
//! "Ensure branding always defers to content". The Toolbars guidelines
//! say "Don't title windows with your app name". The lockup is one
//! picture in the strip's otherwise unused left end, outside the content,
//! and the window's title text stays hidden.
//!
//! **The title bar places it, in the traffic lights' own view.** In full
//! screen AppKit detaches that view from the window, so the lockup goes
//! with the buttons rather than staying behind in the content
//! (`strip.rs`). Its x is AppKit's, read from its frame in the window
//! once laid out ([`Mark::end`]); its size is the mark alone's
//! ([`Place::beside`]); its centre is the title bar's, re-placed against
//! the accessory's height as AppKit sets it. [`Place::frame`], centred in
//! a container's top strip, is the README film's composed strip
//! (`film/chrome.rs`), where there is no window.
#![forbid(unsafe_code)]

use std::cell::Cell;

use macos_liquid_glass::drawable::Appearance;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{
    NSAppearanceCustomization, NSImage, NSImageView, NSView, NSWindow, NSWindowButton,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use macos_liquid_glass::window::{GlassWindow, TitlebarSide};

use crate::strip::Accessory;

/// Where the lockup starts, in points from the left, when the traffic
/// lights are not laid out: past where AppKit puts the zoom button.
const FALLBACK_X: CGFloat = 74.0;

/// The width of the box the mark alone was drawn in, in its units: the
/// square beside the traffic lights was this many units across, which
/// sets the points per unit the lockup is drawn at.
const MARK_BOX: CGFloat = 300.0;

/// **Which lockup the strip shows**: the light word over a dark
/// appearance, the dark word over a light one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Lockup {
    OnDark,
    OnLight,
}

impl Lockup {
    /// The lockup for a window that resolves to `appearance`.
    pub(crate) fn for_appearance(appearance: Appearance) -> Lockup {
        match appearance {
            Appearance::Dark => Lockup::OnDark,
            Appearance::Light => Lockup::OnLight,
        }
    }

    /// Its SVG, the generator's file through `baud-app/assets/logo/`.
    pub(crate) fn svg(self) -> &'static [u8] {
        match self {
            Lockup::OnDark => include_bytes!("../assets/logo/baud-lockup-dark-tight.svg"),
            Lockup::OnLight => include_bytes!("../assets/logo/baud-lockup-light-tight.svg"),
        }
    }

    /// **The size the SVG says it is drawn at**, in its units: its root's
    /// `width` and `height`, which the generator writes as its box's.
    pub(crate) fn art(self) -> Option<NSSize> {
        let text = std::str::from_utf8(self.svg()).ok()?;
        let root = text.split("<svg").nth(1)?.split('>').next()?;
        let read = |name: &str| -> Option<CGFloat> {
            root.split(&format!(" {name}=\""))
                .nth(1)?
                .split('"')
                .next()?
                .parse()
                .ok()
        };
        Some(NSSize::new(read("width")?, read("height")?))
    }
}

/// The window's appearance as light or dark, resolved as AppKit resolves
/// it: the best match of its effective appearance against Aqua and Dark
/// Aqua.
fn appearance_of(window: &NSWindow) -> Appearance {
    if macos_liquid_glass::is_dark(&window.effectiveAppearance()) {
        Appearance::Dark
    } else {
        Appearance::Light
    }
}

/// **The lockup's place in the strip**: its left edge and its size, which
/// a resize does not change.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Place {
    pub(crate) x: CGFloat,
    pub(crate) width: CGFloat,
    pub(crate) height: CGFloat,
}

impl Place {
    /// **Beside the traffic lights**, from their frames once the window
    /// has laid them out: the lights' own gap past the zoom button, and
    /// the lockup `art` units in size at the points per unit the mark
    /// alone had, its box across a square a little larger than a button.
    /// `None` while they are not laid out, which a zero height says. The
    /// x is shared with the container: the titlebar view spans the
    /// window's width from its left edge.
    pub(crate) fn beside(
        close: NSRect,
        mini: NSRect,
        zoom: NSRect,
        inset: CGFloat,
        art: NSSize,
    ) -> Option<Place> {
        if zoom.size.height < 1.0 {
            return None;
        }
        let gap = (mini.origin.x - (close.origin.x + close.size.width)).max(0.0);
        let side = (zoom.size.height * 1.4).min(inset - 4.0).max(16.0);
        Some(Place::sized(
            zoom.origin.x + zoom.size.width + gap,
            side,
            art,
        ))
    }

    /// Where the lockup goes when the lights are not laid out.
    pub(crate) fn fallback(inset: CGFloat, art: NSSize) -> Place {
        Place::sized(FALLBACK_X, (inset - 2.0).clamp(18.0, 28.0), art)
    }

    /// The lockup at `x`, at the points per unit of the mark alone in a
    /// `side`-point square.
    fn sized(x: CGFloat, side: CGFloat, art: NSSize) -> Place {
        let k = side / MARK_BOX;
        Place {
            x,
            width: art.width * k,
            height: art.height * k,
        }
    }

    /// **The lockup's frame in a container `height` points tall**,
    /// bottom-left origin: centred in the container's top `inset` points,
    /// the strip: the README film's composed strip (`film/chrome.rs`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn frame(self, height: CGFloat, inset: CGFloat) -> NSRect {
        let centre = height - inset / 2.0;
        NSRect::new(
            NSPoint::new(self.x, centre - self.height / 2.0),
            NSSize::new(self.width, self.height),
        )
    }

    /// The lockup's right edge, the cursor's.
    pub(crate) fn end(self) -> CGFloat {
        self.x + self.width
    }

    /// **Where the activity lights may start**: a light's gap past the
    /// lockup's end, the gap between two of them. The README film's
    /// composed strip (`film/chrome.rs`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn lights_left(self) -> CGFloat {
        self.end() + CGFloat::from(baud_render::lights::PAIR)
    }
}

/// **The lockup on screen**: its view, in its titlebar accessory, its
/// size, both forms, and the one shown.
pub(crate) struct Mark {
    view: Retained<NSImageView>,
    accessory: Accessory,
    place: Place,
    on_dark: Retained<NSImage>,
    on_light: Retained<NSImage>,
    shown: Cell<Lockup>,
}

impl Mark {
    /// The view of the accessory the lockup is in
    /// (`full_screen_check.rs` reads it).
    pub(crate) fn accessory_view(&self) -> Retained<NSView> {
        self.accessory.view.clone()
    }

    /// **The lockup put in `glass`'s title bar**, a leading accessory
    /// beside its traffic lights, sized for a title bar `inset` points tall,
    /// in the form for the window's appearance now, and centred at once in
    /// the height AppKit gave it. `None` when an SVG does not decode.
    pub(crate) fn put(mtm: MainThreadMarker, glass: &GlassWindow, inset: CGFloat) -> Option<Mark> {
        let window = glass.ns_window();
        let on_dark = macos_liquid_glass::image_from_bytes(Lockup::OnDark.svg())?;
        let on_light = macos_liquid_glass::image_from_bytes(Lockup::OnLight.svg())?;
        let art = Lockup::OnDark.art()?;
        let lights = (
            window.standardWindowButton(NSWindowButton::CloseButton),
            window.standardWindowButton(NSWindowButton::MiniaturizeButton),
            window.standardWindowButton(NSWindowButton::ZoomButton),
        );
        let place = match lights {
            (Some(c), Some(m), Some(z)) => {
                Place::beside(c.frame(), m.frame(), z.frame(), inset, art)
            }
            _ => None,
        }
        .unwrap_or_else(|| Place::fallback(inset, art));
        let shown = Lockup::for_appearance(appearance_of(window));
        let view = macos_liquid_glass::icon_view(
            mtm,
            match shown {
                Lockup::OnDark => &on_dark,
                Lockup::OnLight => &on_light,
            },
        );
        // The accessory is the lockup's width; AppKit sets its height.
        let accessory = Accessory::put(
            mtm,
            glass,
            TitlebarSide::Leading,
            NSSize::new(place.width, inset),
        );
        // No autoresizing mask: a drag to a tiny window clamped the frame
        // and it never recovered, so the lockup is placed on every resize.
        accessory.view.addSubview(&view);
        let mark = Mark {
            view,
            accessory,
            place,
            on_dark,
            on_light,
            shown: Cell::new(shown),
        };
        mark.place();
        Some(mark)
    }

    /// **Centred in its accessory's height now**: at the launch, once the
    /// body has the container, and on every resize, since AppKit sets the
    /// height when it lays the title bar out.
    pub(crate) fn place(&self) {
        let h = self.accessory.view.frame().size.height;
        self.view.setFrame(NSRect::new(
            NSPoint::new(0.0, (h - self.place.height) / 2.0),
            NSSize::new(self.place.width, self.place.height),
        ));
    }

    /// **The form for `appearance`**, the window's: its picture swapped
    /// when the form changes, and nothing else. Both forms are one box,
    /// so the frame stays.
    pub(crate) fn follow(&self, appearance: Appearance) {
        let want = Lockup::for_appearance(appearance);
        if want == self.shown.get() {
            return;
        }
        self.view.setImage(Some(match want {
            Lockup::OnDark => &self.on_dark,
            Lockup::OnLight => &self.on_light,
        }));
        self.shown.set(want);
    }

    /// **The form for `window`'s appearance now**, read as [`Mark::put`]
    /// reads it.
    pub(crate) fn follow_window(&self, window: &NSWindow) {
        self.follow(appearance_of(window));
    }

    /// The form shown, which `baud-app/tests/titlebar_mark.rs` looks at.
    #[allow(dead_code)]
    pub(crate) fn shown(&self) -> Lockup {
        self.shown.get()
    }

    /// **The lockup's right edge in the window**, where AppKit put its
    /// accessory; [`Place::end`] while the title bar is not laid out yet,
    /// which an accessory at the window's left edge says.
    pub(crate) fn end(&self) -> CGFloat {
        let at = self.accessory.in_window();
        if at.origin.x < 1.0 {
            return self.place.end();
        }
        at.origin.x + self.place.width
    }

    /// Where the activity lights may start: a light's gap past
    /// [`Mark::end`].
    #[allow(dead_code)]
    pub(crate) fn lights_left(&self) -> CGFloat {
        self.end() + CGFloat::from(baud_render::lights::PAIR)
    }

    /// The lockup's accessory view, which `baud-app/tests/titlebar_mark.rs`
    /// looks at.
    #[allow(dead_code)]
    pub(crate) fn accessory(&self) -> &Accessory {
        &self.accessory
    }

    /// The lockup's view, which `baud-app/tests/titlebar_mark.rs` looks at.
    #[allow(dead_code)]
    pub(crate) fn view(&self) -> &NSImageView {
        &self.view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: CGFloat, y: CGFloat, w: CGFloat, h: CGFloat) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
    }

    /// AppKit's traffic lights on macOS 27 in a titled window, each in the
    /// titlebar view's own coordinates: 14x16 at x 9, 29 and 49.
    fn lights() -> (NSRect, NSRect, NSRect) {
        (
            rect(9.0, 6.0, 14.0, 16.0),
            rect(29.0, 6.0, 14.0, 16.0),
            rect(49.0, 6.0, 14.0, 16.0),
        )
    }

    fn art() -> NSSize {
        Lockup::OnDark.art().expect("the lockup's SVG has a size")
    }

    /// **A dark appearance shows the light word and a light one the dark
    /// word**, each the generator's file for it, in one box. Perturbation:
    /// swap the two arms of `for_appearance` and the first two checks
    /// fail.
    #[test]
    fn a_dark_window_shows_the_light_word_and_a_light_one_the_dark() {
        assert_eq!(Lockup::for_appearance(Appearance::Dark), Lockup::OnDark);
        assert_eq!(Lockup::for_appearance(Appearance::Light), Lockup::OnLight);
        let word = |l: Lockup| {
            let text = std::str::from_utf8(l.svg()).expect("UTF-8");
            let path = text.split("<path").nth(1).expect("the word's path");
            path.split(" fill=\"").nth(1).expect("a fill")[..7].to_owned()
        };
        // The brand sheet's type on dark and type on light.
        assert_eq!(word(Lockup::OnDark), "#F3F4F6");
        assert_eq!(word(Lockup::OnLight), "#1E2128");
        assert_eq!(Lockup::OnDark.art(), Lockup::OnLight.art());
    }

    /// **The lockup is in the strip of the container it is in, whatever
    /// size the container was built at**, and its mark keeps its size: in
    /// the README film's window, 786x572 points with a 28-point titlebar,
    /// it starts at 69, past the zoom button's right edge at 63, at 22.4
    /// points per 300 units, so the mark's 168-unit cell is 12.544 points
    /// tall, as the mark alone was; it is centred on the strip's 558 and
    /// ends where its art does. The control is the size the container was
    /// built at, 960x600: the same place there is wholly above the
    /// 572-point container, which is where the film's mark was.
    /// Perturbation: centre on 600 rather than the height given and the
    /// first check fails.
    #[test]
    fn the_lockup_is_in_the_strip_of_the_container_it_is_in() {
        let (c, m, z) = lights();
        let a = art();
        let p = Place::beside(c, m, z, 28.0, a).expect("laid out");
        assert_eq!(p.x, 69.0);
        let k = 22.4 / MARK_BOX;
        assert!((168.0 * k - 12.544).abs() < 1e-9);
        assert!((p.width - a.width * k).abs() < 1e-9, "{p:?}");
        assert!((p.height - a.height * k).abs() < 1e-9, "{p:?}");
        let f = p.frame(572.0, 28.0);
        assert!(f.origin.y >= 572.0 - 28.0, "{f:?}");
        assert!(f.origin.y + f.size.height <= 572.0, "{f:?}");
        assert!(
            (f.origin.y + f.size.height / 2.0 - 558.0).abs() < 1e-9,
            "{f:?}"
        );
        assert!(f.origin.x >= z.origin.x + z.size.width, "{f:?}");
        let built = p.frame(600.0, 28.0);
        assert!(built.origin.y > 572.0, "the control: {built:?}");
        assert!((p.end() - (f.origin.x + f.size.width)).abs() < 1e-9);
        assert!((p.lights_left() - p.end() - 9.0).abs() < 1e-9);
    }

    /// **Lights not laid out fall back to the strip's own size**, and a
    /// strip too short for a light-sized square keeps the 16-point floor
    /// rather than panicking: `clamp` panics when its floor passes its
    /// ceiling, which a 19-point strip would make it.
    #[test]
    fn unlaid_lights_and_a_short_strip_still_place_the_lockup() {
        let (c, m, _) = lights();
        let a = art();
        assert_eq!(
            Place::beside(c, m, rect(49.0, 6.0, 14.0, 0.0), 28.0, a),
            None
        );
        let f = Place::fallback(28.0, a);
        assert_eq!(f.x, 74.0);
        assert!((f.width - a.width * 26.0 / MARK_BOX).abs() < 1e-9, "{f:?}");
        let (c, m, z) = lights();
        let short = Place::beside(c, m, z, 19.0, a).expect("laid out");
        assert!((short.height - a.height * 16.0 / MARK_BOX).abs() < 1e-9);
        assert!(short.height <= 19.0, "{short:?}");
    }
}
