//! **baud's logo in the titlebar strip**: the lockup, the word baud and
//! the mark ❯█, on the plate with the activity lights, at the title bar's
//! right end (`lights::panel`), centred in the title bar's height. **With no
//! lights in the title bar** (`activity_lights` off, or on a line of their
//! own) there is no plate, and it stands in a leading titlebar accessory,
//! which AppKit puts a button's gap right of the traffic lights
//! (`strip.rs`).
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
//! **The word follows what it stands on.** The dark lockup's word is the
//! brand sheet's light type, the light lockup's its dark type, the amber the
//! same in both (`assets/logo/README.md`).
//!
//! **On the plate, the plate decides**, whatever the window is
//! ([`Lockup::on_plate`]): the form whose word has the higher WCAG ratio
//! against the plate's own colour, the ratio the plate's letters are held to
//! (`LightsLook::of`). So the word reads at 17.29:1 on the dark plate and
//! 14.01:1 on the light, a theme of the other half than the window's
//! included. Neither form reaches 4.5:1 on the greys #707070 to #878787, the
//! least being 3.85:1 at #7B7B7B. The plate's colour is part of [`Fitted`], so
//! the tick that draws a new plate fits the word to it, however the theme
//! came to change. The amber is not picked: 9.17:1 on #101010 and 1.80:1 on
//! #EFEFEF.
//!
//! **With no plate, the window decides**: its `effectiveAppearance`, not a
//! global preference key. AppKit's header names "a view's, window's, or the
//! app's effectiveAppearance" as the way to determine the appearance
//! ([[source: sdk27-nsappearance-h]] line 27), and it is the appearance
//! AppKit draws the traffic lights in beside it. A picture in an image view
//! does not follow a flip by itself, so [`Mark::follow`] swaps it, called
//! from the driver's appearance path on every flip. Under `[glass] style =
//! "glass"` the strip's material follows Icon & widget style
//! (`material_appearance` in `app.rs`) while the lockup follows the window,
//! so when the two settings disagree, the word can sit on glass of its own
//! lightness. That case is open for those settings.
//!
//! **In a narrow window the lockup stays whole.** The activity lights drop
//! their leftmost lights first when the strip has no room for them all, and
//! the plate shrinks with them (`lights.rs`). The window's title text is
//! hidden by the window crate's titled shape, so nothing else in the strip
//! competes, and the window's smallest width, 200 points, still holds the
//! lockup.
//!
//! **Where Apple's guidance differs, recorded as open.** The Branding
//! guidelines say "Resist the temptation to display your logo throughout
//! your app or game unless it's essential for providing context" and
//! "Ensure branding always defers to content". The Toolbars guidelines
//! say "Don't title windows with your app name". The lockup is one
//! picture in the strip's otherwise unused left end, outside the content,
//! and the window's title text stays hidden.
//!
//! **The build's name**, `<version>+<hash>`, is at the bar's left, in the
//! leading accessory, so it goes with the title bar in full screen: AppKit's
//! own label, its small system font and its secondary label colour, which
//! follows the window's appearance by itself. It is the first thing a
//! narrow window gives up: shown only while every activity light still fits
//! past it ([`name_shows`]), so no light drops for it, and hidden the
//! accessory has nothing in it. With no plate it follows the lockup, while
//! it fits ([`version_shows`]).
//!
//! **The title bar places it, in the traffic lights' own view.** In full
//! screen AppKit detaches that view from the window, so the lockup goes
//! with the buttons rather than staying behind in the content
//! (`strip.rs`), in whichever accessory it is in. On the glass its x is
//! AppKit's, read from its frame in the window once laid out; on the plate
//! the plate says where ([`OnPlate`]). Its size is the mark alone's
//! ([`Place::beside`]); its centre is the title bar's, re-placed against
//! the accessory's height as AppKit sets it.
#![forbid(unsafe_code)]

use std::cell::Cell;

use macos_liquid_glass::drawable::Appearance;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{
    NSAppearanceCustomization, NSColor, NSFont, NSImage, NSImageScaling, NSImageView, NSTextField,
    NSView, NSWindow, NSWindowButton,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use macos_liquid_glass::window::{GlassWindow, TitlebarSide};

use baud_render::{Argb, Backdrop, contrast_over};

use crate::strip::Accessory;

/// Where the lockup starts, in points from the left, when the traffic
/// lights are not laid out: past where AppKit puts the zoom button.
const FALLBACK_X: CGFloat = 74.0;

/// The width of the box the mark alone was drawn in, in its units: the
/// square beside the traffic lights was this many units across, which
/// sets the points per unit the lockup is drawn at.
const MARK_BOX: CGFloat = 300.0;

/// **The side of that square, in points**: 22.4, so the lockup is drawn at
/// 22.4 points per 300 units, 51.37 across with the mark's cell 12.544
/// tall, whatever size AppKit gives its window buttons. It was once read
/// as 1.4 times the zoom button's height, which made 22.4 from a 16-point
/// button frame; linked against the macOS 27 SDK AppKit's
/// `standardWindowButton:forStyleMask:` gives a 14-point one, and the
/// lockup came out at 19.6, 86% of its size.
const MARK_SIDE: CGFloat = 22.4;

/// **The square's side in a title bar `inset` points tall**: [`MARK_SIDE`],
/// unless the bar is too short to hold it with two points above and
/// below; then the bar less those, but never under 16 points.
fn side_in(inset: CGFloat) -> CGFloat {
    MARK_SIDE.min(inset - 4.0).max(16.0)
}

/// **Which lockup the strip shows**: the light word on what is dark, the
/// dark word on what is light.
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

    /// **The lockup for a `plate` of this colour**: the form whose word has
    /// the higher contrast against it, opaque as the plate is.
    pub(crate) fn on_plate(plate: Argb) -> Lockup {
        let reads = |form: Lockup| {
            form.word().map_or(0.0, |word| {
                contrast_over(
                    word,
                    plate,
                    baud_domain::Chan::from_u8(0xFF),
                    Backdrop::any(),
                )
            })
        };
        match reads(Lockup::OnDark) >= reads(Lockup::OnLight) {
            true => Lockup::OnDark,
            false => Lockup::OnLight,
        }
    }

    /// **The lockup for what it stands on**: `plate`'s colour if there is a
    /// plate, else the window's `appearance`. The one choice, for the app's
    /// strip and the film's (`film/chrome.rs`).
    pub(crate) fn standing(plate: Option<Argb>, appearance: Appearance) -> Lockup {
        match plate {
            Some(colour) => Lockup::on_plate(colour),
            None => Lockup::for_appearance(appearance),
        }
    }

    /// **Its word's colour**, as its SVG fills the word, the file's first
    /// path.
    pub(crate) fn word(self) -> Option<Argb> {
        let text = std::str::from_utf8(self.svg()).ok()?;
        let path = text.split("<path").nth(1)?;
        let fill = path.split(" fill=\"#").nth(1)?.get(..6)?;
        Some(Argb(0xFF00_0000 | u32::from_str_radix(fill, 16).ok()?))
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
    /// alone had ([`side_in`]), which the buttons' size does not change.
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
        Some(Place::sized(
            zoom.origin.x + zoom.size.width + gap,
            side_in(inset),
            art,
        ))
    }

    /// Where the lockup goes when the lights are not laid out, at the same
    /// size as beside them.
    pub(crate) fn fallback(inset: CGFloat, art: NSSize) -> Place {
        Place::sized(FALLBACK_X, side_in(inset), art)
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

    /// The lockup's right edge, the cursor's.
    pub(crate) fn end(self) -> CGFloat {
        self.x + self.width
    }

    /// The lockup's size, which the plate is laid out around.
    pub(crate) fn size(self) -> NSSize {
        NSSize::new(self.width, self.height)
    }
}

/// **`frame` on whole device pixels at `scale`**: its size rounded to the
/// nearest whole pixels, and its origin to the whole pixel nearest where
/// that size keeps the frame's centre. A lockup between pixels is drawn
/// with its edges part-covered, so the block's top row and edge column
/// show no pixel of its amber. The picture is scaled to fill the frame on
/// each axis ([`Mark::put`]), so its edges are the frame's: at 2x the
/// lockup is 103 by 25 pixels for 102.73 by 25.09, its shape 0.6% wider.
pub(crate) fn snapped(frame: NSRect, scale: CGFloat) -> NSRect {
    let scale = scale.max(1.0);
    let on = |v: CGFloat| (v * scale).round() / scale;
    let size = NSSize::new(on(frame.size.width), on(frame.size.height));
    let at = |from: CGFloat, exact: CGFloat, whole: CGFloat| on(from + (exact - whole) / 2.0);
    NSRect::new(
        NSPoint::new(
            at(frame.origin.x, frame.size.width, size.width),
            at(frame.origin.y, frame.size.height, size.height),
        ),
        size,
    )
}

/// **The gap between the lockup and the build's name**, in points: a
/// light's gap, the strip's one spacing.
pub(crate) const VERSION_GAP: CGFloat = baud_render::lights::PAIR as CGFloat;

/// **The build's name's size, in points**: `NSFont.smallSystemFontSize`,
/// which `baud-app/tests/titlebar_mark.rs` holds this to. Here so the composed
/// strip, which asks AppKit nothing, says it at the same size.
pub(crate) const VERSION_POINTS: CGFloat = 11.0;

/// **The build's name's baseline on the plate's strip**, in points down
/// from the strip's top: 18 of the 28 it was drawn for.
pub(crate) const VERSION_BASELINE: CGFloat = 18.0;

/// **The lockup's frame in a leading accessory `h` points tall**, at
/// `scale`: at its left, centred in the height, on whole device pixels.
pub(crate) fn beside_at(h: CGFloat, size: NSSize, scale: CGFloat) -> NSRect {
    snapped(
        NSRect::new(NSPoint::new(0.0, (h - size.height) / 2.0), size),
        scale,
    )
}

/// How many of the lights a window has room for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Placed {
    Every,
    Fewer,
}

/// **Whether the build's name is shown beside the plate**: only if every
/// light is placed, and the plate starts a light's gap past the end of the
/// name, which starts at `left`. In the window's points. So no light drops
/// for it.
pub(crate) fn name_shows(
    placed: Placed,
    plate_left: CGFloat,
    left: CGFloat,
    version_width: CGFloat,
) -> bool {
    placed == Placed::Every && plate_left >= left + version_width + VERSION_GAP
}

/// **Whether the build's name is shown after the lockup, with no plate**, in
/// a window `width` points wide: only if it ends a light's gap short of
/// `right` from the window's edge, its left a light's gap past `lockup_end`.
pub(crate) fn version_shows(
    width: CGFloat,
    right: CGFloat,
    lockup_end: CGFloat,
    version_width: CGFloat,
) -> bool {
    let end = lockup_end + VERSION_GAP + version_width;
    width - right >= end + VERSION_GAP
}

/// **The plate the lockup stands on**, as `lights::Strip::place` has laid it
/// out: the accessory it is in, the lockup's frame there, where the plate
/// starts in the window, how many of the lights are placed, and the colour
/// the plate is drawn in.
pub(crate) struct OnPlate {
    pub(crate) holder: Retained<NSView>,
    pub(crate) logo: NSRect,
    pub(crate) plate_left: CGFloat,
    pub(crate) placed: Placed,
    pub(crate) colour: Argb,
}

/// **What the lockup is placed for**: how wide the lights are, the scale
/// they are drawn at, and the colour of the plate they are on, if any. A
/// change of any of them moves the lockup or changes its form
/// (`lights::Strip::fitted`, [`Mark::lights_moved`]).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub(crate) struct Fitted {
    pub(crate) across: CGFloat,
    pub(crate) scale: CGFloat,
    pub(crate) plate: Option<Argb>,
}

/// **The lockup on screen**: its view, on the plate or in its titlebar
/// accessory, its size, both forms, and the one shown; and the build's name.
pub(crate) struct Mark {
    view: Retained<NSImageView>,
    accessory: Accessory,
    place: Place,
    on_dark: Retained<NSImage>,
    on_light: Retained<NSImage>,
    shown: Cell<Lockup>,
    version: Retained<NSTextField>,
    version_size: NSSize,
    version_shown: Cell<bool>,
    /// The colour of the plate it stands on, none on the glass.
    plate: Cell<Option<Argb>>,
    fitted: Cell<Fitted>,
}

impl Mark {
    /// The view of the leading accessory, the build's name's, and the
    /// lockup's with no plate (`full_screen_check.rs` reads it).
    #[cfg_attr(feature = "effect-probe", allow(dead_code))]
    pub(crate) fn accessory_view(&self) -> Retained<NSView> {
        self.accessory.view.clone()
    }

    /// **The lockup put in `glass`'s title bar**, a leading accessory
    /// beside its traffic lights, sized for a title bar `inset` points tall,
    /// in the form for the window's appearance now, and centred at once in
    /// the height AppKit gave it, `version` after it. `None` when an SVG
    /// does not decode.
    pub(crate) fn put(
        mtm: MainThreadMarker,
        glass: &GlassWindow,
        inset: CGFloat,
        version: &str,
    ) -> Option<Mark> {
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
        // To the frame's edges on both axes: the frame is on whole device
        // pixels ([`snapped`]), a hair off the picture's own shape.
        view.setImageScaling(NSImageScaling::ScaleAxesIndependently);
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
        let label = NSTextField::labelWithString(&NSString::from_str(version), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize(VERSION_POINTS)));
        label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        label.setHidden(true);
        let version_size = label.fittingSize();
        accessory.view.addSubview(&label);
        let mark = Mark {
            version: label,
            version_size,
            version_shown: Cell::new(false),
            plate: Cell::new(None),
            fitted: Cell::new(Fitted::default()),
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

    /// **Centred in its accessory's height now**, with no plate: at the
    /// launch, once the body has the container, and on every resize, since
    /// AppKit sets the height when it lays the title bar out. On the plate
    /// [`Mark::fit`] places it.
    pub(crate) fn place(&self) {
        if self.on_plate() {
            return;
        }
        let h = self.accessory.view.frame().size.height;
        let scale = self
            .accessory
            .view
            .window()
            .map_or(1.0, |w| w.backingScaleFactor());
        let at = beside_at(h, self.place.size(), scale);
        self.view.setFrame(at);
        self.version.setFrame(NSRect::new(
            NSPoint::new(
                at.size.width + VERSION_GAP,
                (h - self.version_size.height) / 2.0,
            ),
            self.version_size,
        ));
    }

    /// **The lockup and the build's name for `window` as it is now**, on
    /// every resize, after the lights are placed: on `plate`, the lockup in
    /// the plate's accessory at the frame it gives, the name alone in the
    /// leading accessory at its baseline, shown as [`name_shows`] says; with
    /// no plate, the lockup in the leading accessory and the name after it,
    /// shown as [`version_shows`] says. Either way in the form what it now
    /// stands on takes ([`Lockup::standing`]). `lights` is what
    /// [`Mark::lights_moved`] next compares with.
    pub(crate) fn fit(&self, window: &NSWindow, plate: Option<OnPlate>, lights: Fitted) {
        self.fitted.set(lights);
        let was_on_plate = self.on_plate();
        self.plate.set(plate.as_ref().map(|on| on.colour));
        self.show(Lockup::standing(self.plate.get(), appearance_of(window)));
        let h = self.accessory.view.frame().size.height;
        let (shows, width) = match &plate {
            Some(on) => {
                if !was_on_plate {
                    self.view.removeFromSuperview();
                    on.holder.addSubview(&self.view);
                }
                self.view.setFrame(on.logo);
                let left = crate::strip::edges(window).left;
                let shows = name_shows(on.placed, on.plate_left, left, self.version_size.width);
                let below = h / 2.0 - (VERSION_BASELINE - crate::strip::STRIP / 2.0);
                self.version.setFrame(NSRect::new(
                    NSPoint::new(0.0, below - self.version.baselineOffsetFromBottom()),
                    self.version_size,
                ));
                (shows, if shows { self.version_size.width } else { 0.0 })
            }
            None => {
                if was_on_plate {
                    self.view.removeFromSuperview();
                    self.accessory.view.addSubview(&self.view);
                }
                self.place();
                let shows = version_shows(
                    window.frame().size.width,
                    crate::strip::close_gap(window),
                    self.lockup_end(),
                    self.version_size.width,
                );
                let width = match shows {
                    true => self.view.frame().size.width + VERSION_GAP + self.version_size.width,
                    false => self.view.frame().size.width,
                };
                (shows, width)
            }
        };
        self.version.setHidden(!shows);
        self.version_shown.set(shows);
        self.accessory.set_width(width);
    }

    /// The lockup's size, which the plate is laid out around.
    pub(crate) fn size(&self) -> NSSize {
        self.place.size()
    }

    /// Whether the lights have become another width, are drawn at another
    /// scale or on a plate of another colour, since the last [`Mark::fit`], as
    /// they are when they are first drawn or turned off, the window moves to
    /// another screen, or the theme changes: one compare, for the path that
    /// runs on every tick.
    pub(crate) fn lights_moved(&self, lights: Fitted) -> bool {
        self.fitted.get() != lights
    }

    /// The label the build's name is in, which
    /// `baud-app/tests/titlebar_mark.rs` looks at.
    #[allow(dead_code)]
    pub(crate) fn version_label(&self) -> &NSTextField {
        &self.version
    }

    /// **The window is now `appearance`**: the form follows it with no plate,
    /// and on a plate stays the plate's.
    pub(crate) fn follow(&self, appearance: Appearance) {
        self.show(Lockup::standing(self.plate.get(), appearance));
    }

    /// **The form `want`**: its picture swapped when the form changes, and
    /// nothing else. Both forms are one box, so the frame stays.
    fn show(&self, want: Lockup) {
        if want == self.shown.get() {
            return;
        }
        self.view.setImage(Some(match want {
            Lockup::OnDark => &self.on_dark,
            Lockup::OnLight => &self.on_light,
        }));
        self.shown.set(want);
    }

    /// **[`Mark::follow`] with `window`'s appearance now**, read as
    /// [`Mark::put`] reads it.
    pub(crate) fn follow_window(&self, window: &NSWindow) {
        self.follow(appearance_of(window));
    }

    /// The form shown, which `baud-app/tests/titlebar_mark.rs` looks at.
    #[allow(dead_code)]
    pub(crate) fn shown(&self) -> Lockup {
        self.shown.get()
    }

    /// **The lockup's right edge in the window with no plate**, where AppKit
    /// put its accessory; [`Place::end`] while the title bar is not laid out
    /// yet, which an accessory at the window's left edge says.
    pub(crate) fn lockup_end(&self) -> CGFloat {
        let at = self.accessory.in_window();
        if at.origin.x < 1.0 {
            return self.place.end();
        }
        at.origin.x + self.view.frame().size.width
    }

    /// Whether the lockup is on the plate, which
    /// `baud-app/tests/titlebar_mark.rs` looks at.
    #[allow(dead_code)]
    pub(crate) fn on_plate(&self) -> bool {
        self.plate.get().is_some()
    }

    /// The leading accessory, which `baud-app/tests/titlebar_mark.rs` looks
    /// at.
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

    /// AppKit's traffic lights in a titled window linked against the macOS
    /// 14.4 SDK, each in the titlebar view's own coordinates: 14x16 at x 9,
    /// 29 and 49.
    fn lights() -> (NSRect, NSRect, NSRect) {
        lights_of(16.0)
    }

    /// The same buttons `height` points tall: 16 from
    /// `standardWindowButton:forStyleMask:` linked against the macOS 14.4
    /// SDK, 14 linked against 27.
    fn lights_of(height: CGFloat) -> (NSRect, NSRect, NSRect) {
        (
            rect(9.0, 6.0, 14.0, height),
            rect(29.0, 6.0, 14.0, height),
            rect(49.0, 6.0, 14.0, height),
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

    /// The word's contrast on `plate`.
    fn reads(form: Lockup, plate: Argb) -> f64 {
        contrast_over(
            form.word().expect("the word's fill"),
            plate,
            baud_domain::Chan::from_u8(0xFF),
            Backdrop::any(),
        )
    }

    /// **The word's fill is the file's, and its contrast is WCAG's against
    /// the plate alone**: the figures the note gives, worked by hand from
    /// the two fills. The positive control for what follows: a `word` that
    /// read another path, or a call that took the backdrop in, misses them.
    #[test]
    fn the_words_contrast_is_the_figures_the_note_gives() {
        assert_eq!(Lockup::OnDark.word(), Some(Argb(0xFF_F3_F4_F6)));
        assert_eq!(Lockup::OnLight.word(), Some(Argb(0xFF_1E_21_28)));
        for (form, plate, want) in [
            (Lockup::OnDark, 0xFF_10_10_10, 17.29),
            (Lockup::OnLight, 0xFF_EF_EF_EF, 14.01),
            (Lockup::OnLight, 0xFF_10_10_10, 1.18),
            (Lockup::OnDark, 0xFF_EF_EF_EF, 1.04),
            (Lockup::OnLight, 0xFF_80_80_80, 4.08),
            (Lockup::OnDark, 0xFF_80_80_80, 3.59),
        ] {
            let got = reads(form, Argb(plate));
            assert!((got - want).abs() < 0.005, "{form:?} {plate:08X}: {got}");
        }
    }

    /// **On a plate the plate's lightness decides, under either window**: a
    /// dark plate the light word under a light window, a light plate the dark
    /// word under a dark window. Perturbation: `standing` that reads the
    /// window alone, as the lockup did, fails both mismatched rows.
    #[test]
    fn a_plate_decides_the_form_whatever_the_window() {
        let (dark, light) = (Argb(0xFF_10_10_10), Argb(0xFF_EF_EF_EF));
        for window in [Appearance::Dark, Appearance::Light] {
            assert_eq!(Lockup::standing(Some(dark), window), Lockup::OnDark);
            assert_eq!(Lockup::standing(Some(light), window), Lockup::OnLight);
        }
    }

    /// **With no plate the window decides**, the two windows the two forms.
    /// Perturbation: `standing` that takes a default colour with no plate
    /// fails one window.
    #[test]
    fn no_plate_follows_the_window() {
        assert_eq!(Lockup::standing(None, Appearance::Dark), Lockup::OnDark);
        assert_eq!(Lockup::standing(None, Appearance::Light), Lockup::OnLight);
    }

    /// **A grey takes the better of the two, which is not the half its luma
    /// is in**: #7F7F7F's luma, 127, is under the middle and the dark word
    /// reads the better on it, 4.02:1 to 3.64:1; #767676 the light word, 4.13:1
    /// to 3.55:1. And every grey there is takes the higher of its two ratios.
    /// Perturbation: a form by luma against 128, or by `page_lightness`
    /// against a half, reads #7F7F7F wrong; `<` for `>=` in `on_plate` fails
    /// every row.
    #[test]
    fn a_grey_plate_takes_the_better_contrast() {
        assert_eq!(Lockup::on_plate(Argb(0xFF_7F_7F_7F)), Lockup::OnLight);
        assert_eq!(Lockup::on_plate(Argb(0xFF_80_80_80)), Lockup::OnLight);
        assert_eq!(Lockup::on_plate(Argb(0xFF_76_76_76)), Lockup::OnDark);
        for v in 0..=255u32 {
            let plate = Argb(0xFF00_0000 | (v * 0x01_01_01));
            let form = Lockup::on_plate(plate);
            let other = match form {
                Lockup::OnDark => Lockup::OnLight,
                Lockup::OnLight => Lockup::OnDark,
            };
            assert!(reads(form, plate) >= reads(other, plate), "{v:02X}");
        }
    }

    /// **The word reads at 4.5:1 on the plates of the themes tested**: the
    /// two built-in themes, a tinted dark file, a cream light file, black and
    /// white. Two fixed forms cannot on every plate: the greys #707070 to
    /// #878787 are under it, the least of the better ratio being 3.85:1 at
    /// #7B7B7B. Perturbation: swap the arms of
    /// `on_plate`'s match and every plate here falls to under 1.31:1.
    #[test]
    fn the_word_reads_on_the_plates_of_the_themes() {
        for plate in [
            0xFF_10_10_10u32,
            0xFF_EF_EF_EF,
            0xFF_1A_1B_26,
            0xFF_FD_F6_E3,
            0xFF_00_00_00,
            0xFF_FF_FF_FF,
        ] {
            let got = reads(Lockup::on_plate(Argb(plate)), Argb(plate));
            assert!(got >= 4.5, "{plate:08X}: {got}");
        }
        let at = |v: u32| {
            let plate = Argb(0xFF00_0000 | (v * 0x01_01_01));
            reads(Lockup::on_plate(plate), plate)
        };
        let under: Vec<u32> = (0..=255).filter(|&v| at(v) < 4.5).collect();
        assert_eq!(under, (0x70..=0x87).collect::<Vec<u32>>());
        let least = (0..=255).map(at).fold(f64::INFINITY, f64::min);
        assert!((least - at(0x7B)).abs() < 1e-12, "{least}");
        assert!((3.84..3.85).contains(&least), "{least}");
    }

    /// **The lockup keeps its size, and a button's gap past the buttons is
    /// where the bar's left starts**: 69, past the zoom button's right edge
    /// at 63, at 22.4 points per 300 units, so the mark's 168-unit cell is
    /// 12.544 points tall, as the mark alone was, and the whole 687.94-unit
    /// box 51.37 across and as tall as the cell. Perturbation: the points
    /// per unit of a 26-point square and the sizes fail.
    #[test]
    fn the_lockup_keeps_the_marks_size_past_the_buttons() {
        let (c, m, z) = lights();
        let a = art();
        let p = Place::beside(c, m, z, 28.0, a).expect("laid out");
        assert_eq!(p.x, 69.0);
        let k = 22.4 / MARK_BOX;
        assert!((168.0 * k - 12.544).abs() < 1e-9);
        assert!((p.width - a.width * k).abs() < 1e-9, "{p:?}");
        assert!((p.height - a.height * k).abs() < 1e-9, "{p:?}");
        assert!((p.width - 51.366).abs() < 0.01, "{p:?}");
        assert!((p.height - 12.544).abs() < 1e-9, "{p:?}");
        assert!((p.end() - (p.x + p.width)).abs() < 1e-9);
    }

    /// **The lockup is one size whatever the window buttons are**: 51.37
    /// points across and 12.544 tall beside 16-point buttons and beside
    /// 14-point ones, in a 28-point title bar and a 32-point one, and the
    /// same where the buttons are not laid out. The SDK an app links sizes
    /// the buttons: AppKit's `standardWindowButton:forStyleMask:` gives a
    /// 14x16 frame linked against the macOS 14.4 SDK and a 14x14 one linked
    /// against 27, and Apple says standard controls adopt "changes to shapes
    /// and sizes automatically when you rebuild your app with the latest
    /// version of Xcode" ([[source: apple-adopting-liquid-glass-2026-11-09]]).
    /// Break: the side from the zoom button's height,
    /// `zoom.size.height * 1.4`, and the 14-point rows read 44.95 across;
    /// the fallback's old `(inset - 2.0).clamp(18.0, 28.0)` reads 59.62 in a
    /// 28-point bar.
    #[test]
    fn the_lockup_is_one_size_whatever_the_window_buttons_are() {
        let a = art();
        let want = NSSize::new(a.width * 22.4 / MARK_BOX, a.height * 22.4 / MARK_BOX);
        assert!((want.width - 51.366).abs() < 0.01, "{want:?}");
        assert!((want.height - 12.544).abs() < 1e-9, "{want:?}");
        for button in [16.0, 14.0] {
            let (c, m, z) = lights_of(button);
            for inset in [28.0, 32.0] {
                let p = Place::beside(c, m, z, inset, a).expect("laid out");
                assert_eq!(p.size(), want, "{button}-point buttons, {inset}-point bar");
            }
        }
        for inset in [28.0, 32.0] {
            assert_eq!(Place::fallback(inset, a).size(), want, "{inset}-point bar");
        }
    }

    /// **The build's name gives way before a light and never takes one's
    /// room**: beside the plate only with every light placed and a light's
    /// gap past its end; with no plate only if it fits past the lockup.
    /// Perturbation: drop `placed` and a window that has dropped a light keeps
    /// its name. Only this holds it: a dropped light leaves the plate's left
    /// under a light's width past the traffic lights, too near for any name.
    #[test]
    fn the_name_of_the_build_shows_only_with_every_light() {
        assert!(name_shows(Placed::Every, 300.0, 69.0, 200.0));
        assert!(name_shows(Placed::Every, 278.0, 69.0, 200.0));
        assert!(
            !name_shows(Placed::Every, 277.5, 69.0, 200.0),
            "a gap short"
        );
        assert!(
            !name_shows(Placed::Fewer, 300.0, 69.0, 200.0),
            "a light has gone"
        );
        assert!(version_shows(400.0, 9.0, 120.0, 100.0));
        assert!(!version_shows(240.0, 9.0, 120.0, 100.0));
    }

    /// **Lights not laid out put the lockup at its x past where the buttons
    /// would be**, and a strip too short for the square keeps the 16-point
    /// floor rather than panicking: `clamp` panics when its floor passes
    /// its ceiling, which a 19-point strip would make it.
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
        assert!((f.width - a.width * 22.4 / MARK_BOX).abs() < 1e-9, "{f:?}");
        let low = Place::fallback(19.0, a);
        assert!((low.height - a.height * 16.0 / MARK_BOX).abs() < 1e-9);
        let (c, m, z) = lights();
        let short = Place::beside(c, m, z, 19.0, a).expect("laid out");
        assert!((short.height - a.height * 16.0 / MARK_BOX).abs() < 1e-9);
        assert!(short.height <= 19.0, "{short:?}");
    }
}
