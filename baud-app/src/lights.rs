//! **The activity lights in the macOS title strip**: nine lights at the
//! strip's right, each named with a modem's letters -- HS AA CD OH RD SD
//! CS FAX SYN, in the order a 1988 U.S. Robotics Courier manual's table
//! lists them (`baud_render::lights`) -- and each lit only for something
//! baud measured.
//!
//! **Where**: a module at the title bar's right end, a modem's panel: baud's
//! whole lockup and the nine lights on one plate ([`panel`]), each lamp in
//! its socket over its letters. One image view a light and one the plate, in
//! a trailing titlebar accessory -- the title bar's own place for content at
//! its right end, in the traffic lights' view, which AppKit detaches from
//! the window in full screen (`strip.rs`) -- re-placed on every resize. The
//! accessory is sized at every layout to the plate and to the room right of
//! the traffic lights ([`crate::strip::lights_width`]), since AppKit lets the
//! two ends of a narrow title bar overlap. The glass stays in the strip
//! round the module. Each light's view has its own tracking area
//! (`baud_present_macos::hover`): the pointer crossing a light is heard at
//! once, and baud draws the light's hover text in the frame on the next frame
//! (`line_tip.rs`), as it does on Linux, with no `toolTip`, whose system delay
//! is the wait this replaces. What decides which light the pointer is over is
//! each light's frame on the screen ([`Strip::screen_frames`]), through
//! whichever window holds its view -- in full screen the title bar's own, not
//! the terminal's (`baud_present_macos::hover`). A light whose place would
//! push the plate into the traffic lights in a narrow window is not drawn, the
//! leftmost first; the lockup stays whole and the plate shrinks with them.
//!
//! **The colours are the theme's** (`Renderer::lights_look`): every lamp
//! one of the theme's sixteen palette entries in the set `[window]
//! activity_lights_colours` names, on a plate of the theme's background, a
//! tenth view under the lights, the letters its foreground -- the colours
//! the lights' own line draws on Linux -- and the socket and the bevel made
//! from the plate. The plate is opaque, so a lamp reads against the theme's
//! background whatever the window's material shows through the strip, glass
//! or standard, light or dark; no AppKit colour is read for them, and a change
//! of theme, set or glow draws the pictures again because the look they were
//! drawn for moved. The lockup's form on the plate is the plate's too
//! (`mark.rs`).
//!
//! **The pixels are baud's renderer's** (`Renderer::panel_light`): each
//! light's three pictures, dark, lit and unread, and the plate's, drawn
//! when the scale or the look -- the theme, the set, the glow or Dim
//! Flashing Lights -- changes; a light that changes swaps
//! its view's picture and builds no frame of the grid. The clocks step on
//! the display link's tick, which already runs, from the stamps the pump
//! and the writes already take, and the process reads of OH and CD
//! (`front.rs`) run on that tick while the lights are on: nothing is added
//! to a byte's path, and nothing is timed off the link.
//!
//! **Or on a line of their own in the frame** (`[window] activity_lights
//! = "bottom_line"` or `"top_line"`): the renderer draws them there, each
//! lamp beside its letters, on a small plate (`baud_render::lights`,
//! `Renderer::set_lights_line`), with no plate in the title bar and the
//! lockup beside the traffic lights (`mark.rs`), and `line_tip.rs` shows each
//! light's hover text from its place on that line. The Linux window has no
//! title bar -- no compositor decorations are asked for and none are drawn --
//! so the line is its default, and its lights are drawn there by the Linux
//! driver.

use baud_present_macos::hover::Crossings;
use baud_render::lights::{
    PANEL_HEIGHT, PANEL_LOGO_GAP, PANEL_PAD, PANEL_RIGHT, draw_panel_plate, panel_margin,
};
use baud_render::{
    ActivityLights, Argb, ByLed, ByLight, Glow, Led, Light, LightsImage, LightsLook, Renderer,
    Shown,
};
use macos_liquid_glass::drawable::Waker;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{NSAccessibility, NSImage, NSImageScaling, NSImageView};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

/// What the pictures were drawn for: a change of either draws them again.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Drawn {
    pub(crate) scale: CGFloat,
    /// The theme's look in the configured set and glow, at Dim Flashing
    /// Lights' (`Renderer::lights_look`).
    pub(crate) look: LightsLook,
}

/// What a tick does to the lights.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Step {
    /// Nothing changed, or the lights are off.
    Nothing,
    /// A light changed: its view takes another of the pictures it has.
    Swap,
    /// The pictures are for another scale or look, or there are none: draw
    /// them, then show the state.
    Redraw,
}

/// **What a tick owes the lights in the title strip**: nothing while they
/// are off or on a line of their own in the frame; the
/// pictures again when what they were drawn for is not what is wanted; a
/// swap when a light's state moved from what its view shows.
pub(crate) fn step(
    setting: ActivityLights,
    wanted: Drawn,
    have: Option<Drawn>,
    shown: Shown,
    on_view: ByLed<Option<Light>>,
) -> Step {
    match setting {
        ActivityLights::Off | ActivityLights::BottomLine | ActivityLights::TopLine => Step::Nothing,
        ActivityLights::TitleBar if have != Some(wanted) => Step::Redraw,
        ActivityLights::TitleBar
            if Led::ALL
                .iter()
                .any(|&l| *on_view.get(l) != Some(*shown.get(l))) =>
        {
            Step::Swap
        }
        ActivityLights::TitleBar => Step::Nothing,
    }
}

/// How a lit light is drawn under the Dim Flashing Lights setting: with
/// its glow, or with none.
pub(crate) fn glow(flashing: baud_present_macos::settings::Flashing) -> Glow {
    match flashing {
        baud_present_macos::settings::Flashing::Full => Glow::Full,
        baud_present_macos::settings::Flashing::Dimmed => Glow::Dimmed,
    }
}

/// **What OH's or CD's reading tells its clock as a sighting**: a lit
/// reading is the light's thing seen, a refused one its read refused, a
/// dark one nothing; for a sampler that feeds it at every step (the film).
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) fn feed(
    activity: &mut baud_render::Activity,
    led: Led,
    reading: crate::front::Reading,
    now: baud_domain::HostTime,
) {
    match reading {
        crate::front::Reading::Lit => activity.seen(led, now),
        crate::front::Reading::Refused => activity.refused(led, now),
        crate::front::Reading::Dark => {}
    }
}

/// **What OH's or CD's reading holds its light at**: a lit reading the
/// light's thing held, a refused one its read refused, a dark one nothing;
/// a state, held until a reading says otherwise (`baud_render::Level`).
pub(crate) fn level(reading: crate::front::Reading) -> baud_render::Level {
    match reading {
        crate::front::Reading::Lit => baud_render::Level::Held,
        crate::front::Reading::Refused => baud_render::Level::Refused,
        crate::front::Reading::Dark => baud_render::Level::Free,
    }
}

/// A light's place in the strip, in points from the strip's left, and the
/// strip's width: from the renderer's layout at the scale drawn.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Spans {
    pub(crate) at: ByLed<(CGFloat, CGFloat)>,
    pub(crate) width: CGFloat,
}

/// **Each light's frame in its superview and the text it shows on hover**,
/// bottom-left origin: the strip flush with the container's top, `right`
/// in from its right edge, the strip's `inset` tall, its left end on a
/// whole device pixel at `scale`; a light that would start left of `left`
/// is not placed, so in a narrow window the leftmost go first.
pub(crate) fn placed(
    container: NSSize,
    inset: CGFloat,
    right: CGFloat,
    left: CGFloat,
    scale: CGFloat,
    spans: &Spans,
) -> ByLed<Option<(NSRect, &'static str)>> {
    let scale = scale.max(1.0);
    let x0 = ((container.width - right - spans.width) * scale).round() / scale;
    let y = container.height - inset;
    ByLed::of(|led| {
        let (start, width) = *spans.at.get(led);
        let x = x0 + start;
        (x >= left).then(|| {
            (
                NSRect::new(NSPoint::new(x, y), NSSize::new(width, inset)),
                led.tip(),
            )
        })
    })
}

/// **The lamps on the panel, as the renderer lays them out at a scale**:
/// each column's place, and each lamp's letters as they are before any
/// rounding, their left and their width, in points from the first column's
/// left.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Columns {
    pub(crate) spans: Spans,
    pub(crate) letters: ByLed<(CGFloat, CGFloat)>,
}

impl Columns {
    /// How far the last lamp's letters end from the first column's left.
    fn end(&self) -> CGFloat {
        let (x, w) = *self.letters.get(Led::Syn);
        x + w
    }
}

/// **The module at the title bar's right end, laid out**: the plate, the
/// lockup, the first light on it and each light's frame with its hover text.
/// One source for the app and the film's composed strip
/// (`film/chrome.rs`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Panel {
    /// The plate, not counting the bevel's edge just outside it.
    pub(crate) plate: NSRect,
    pub(crate) logo: NSRect,
    pub(crate) first: Led,
    pub(crate) lamps: ByLed<Option<(NSRect, &'static str)>>,
}

/// **The plate's left for the first light `led`**, its right edge at
/// `right`, before rounding: the pad, the lockup, the gap and the lights from
/// `led` on, and the pad. With no lockup, no gap for it.
fn exact_left(columns: &Columns, led: Led, right: CGFloat, logo: NSSize) -> CGFloat {
    let (x, _) = *columns.letters.get(led);
    let gap = match logo.width > 0.0 {
        true => CGFloat::from(PANEL_LOGO_GAP),
        false => 0.0,
    };
    let pad = CGFloat::from(PANEL_PAD);
    right - pad - (columns.end() - x) - gap - logo.width - pad
}

/// **The module in a `container` with the strip at its top**, `inset` tall,
/// bottom-left origin, every x from the container's left. The plate's right
/// edge `right` in from the container's, on a whole device pixel at
/// `scale`; the last lamp's letters [`PANEL_PAD`] short of it; the lockup
/// [`PANEL_LOGO_GAP`] before the first light's letters, whole, and the
/// plate's left [`PANEL_PAD`] before it, both on whole device pixels. The
/// plate is [`PANEL_HEIGHT`] tall, or `inset` if that is less, and it and
/// the lockup are centred on the strip. A light whose place would start the
/// plate left of `left`, the traffic lights' end, is not placed, the leftmost
/// first, measured before rounding so that the width a window gives up a
/// light at is the panel's own and not the screen's. None with no light
/// placed: there is no plate then, and the lockup goes back beside the
/// traffic lights.
pub(crate) fn panel(
    container: NSSize,
    inset: CGFloat,
    left: CGFloat,
    right: CGFloat,
    scale: CGFloat,
    columns: &Columns,
    logo: NSSize,
) -> Option<Panel> {
    let scale = scale.max(1.0);
    let whole = |v: CGFloat| (v * scale).round() / scale;
    let pad = CGFloat::from(PANEL_PAD);
    let exact_right = container.width - right;
    let right = whole(exact_right);
    let x0 = whole(right - pad - columns.end());
    let first = Led::ALL
        .into_iter()
        .find(|&l| exact_left(columns, l, exact_right, logo) >= left)?;
    // Every light's frame, and only the ones from the first on kept.
    let all = placed(
        container,
        inset,
        container.width - x0 - columns.spans.width,
        CGFloat::NEG_INFINITY,
        scale,
        &columns.spans,
    );
    let from = Led::ALL.iter().position(|&l| l == first)?;
    let lamps = ByLed::of(|led| {
        let at = Led::ALL.iter().position(|&l| l == led)?;
        (at >= from).then(|| *all.get(led)).flatten()
    });
    let (x, _) = *columns.letters.get(first);
    let gap = match logo.width > 0.0 {
        true => CGFloat::from(PANEL_LOGO_GAP),
        false => 0.0,
    };
    let logo_x = whole(x0 + x - gap - logo.width);
    let plate_left = logo_x - pad;
    let strip = container.height - inset;
    let h = CGFloat::from(PANEL_HEIGHT).min(inset);
    Some(Panel {
        plate: NSRect::new(
            NSPoint::new(
                plate_left,
                strip + ((inset - h) / 2.0 * scale).round() / scale,
            ),
            NSSize::new(right - plate_left, h),
        ),
        logo: crate::mark::snapped(
            NSRect::new(
                NSPoint::new(logo_x, strip + (inset - logo.height) / 2.0),
                logo,
            ),
            scale,
        ),
        first,
        lamps,
    })
}

/// **How far the plate's picture reaches past the plate**, in points.
pub(crate) fn plate_margin(scale: CGFloat) -> CGFloat {
    panel_margin(scale as f32).map_or(0.0, |m| CGFloat::from(m.value()) / scale)
}

/// The views, their pictures, and what each shows.
pub(crate) struct Strip {
    /// The trailing titlebar accessory they are in, none until the window
    /// is shown.
    holder: Option<crate::strip::Accessory>,
    views: ByLed<Option<Retained<NSImageView>>>,
    /// The plate under them, a view of its own added before them so they
    /// sit above it, and its picture for each light it can start at.
    plate: Option<Retained<NSImageView>>,
    pictures: Option<(Drawn, ByLed<ByLight<Retained<NSImage>>>)>,
    plates: Option<ByLed<Retained<NSImage>>>,
    on_view: ByLed<Option<Light>>,
    /// Each light's place, from the renderer's layout.
    columns: Columns,
    /// The lockup's size, which the plate is laid out around.
    logo: NSSize,
    /// The scale the spans are whole device pixels at.
    scale: CGFloat,
    /// The colour of the plate's picture, which the lockup's form is chosen
    /// on: what was last drawn, as `columns` and `scale` are, and read only
    /// while there is a plate.
    colour: Argb,
    /// The lights' tracking areas' owner, made with the views, and whom it
    /// wakes at a crossing.
    crossings: Option<Crossings>,
    waker: Option<Waker>,
}

impl Strip {
    /// The lights in `holder`, none until the window is shown, on a plate
    /// with a lockup `logo` in size.
    pub(crate) fn new(
        holder: Option<crate::strip::Accessory>,
        waker: Option<Waker>,
        logo: NSSize,
    ) -> Strip {
        Strip {
            holder,
            views: ByLed::default(),
            plate: None,
            pictures: None,
            plates: None,
            on_view: ByLed::default(),
            columns: Columns {
                spans: Spans {
                    at: ByLed::default(),
                    width: 0.0,
                },
                letters: ByLed::default(),
            },
            logo,
            scale: 1.0,
            colour: Argb(0),
            crossings: None,
            waker,
        }
    }

    /// **The screen points the pointer crossed a light's edge at** since
    /// the last call, in order: where the driver settles a light's tip.
    pub(crate) fn crossed(&self) -> Vec<NSPoint> {
        self.crossings
            .as_ref()
            .map_or_else(Vec::new, Crossings::take)
    }

    /// **Each shown light's frame on the screen**, from its view, through
    /// the window that holds it, which in full screen is the title bar's
    /// and not the terminal's; none for a light not shown.
    pub(crate) fn screen_frames(&self) -> ByLed<Option<NSRect>> {
        ByLed::of(|led| {
            self.views
                .get(led)
                .as_ref()
                .filter(|v| !v.isHidden())
                .and_then(|v| baud_present_macos::hover::on_screen(v))
        })
    }

    /// The plate's view, which `baud-app/tests/titlebar_mark.rs` looks at.
    #[allow(dead_code)]
    pub(crate) fn plate_view(&self) -> Option<&NSImageView> {
        self.plate.as_deref()
    }

    /// A light's view, which `baud-app/tests/titlebar_mark.rs` looks at.
    #[allow(dead_code)]
    pub(crate) fn lamp_view(&self, led: Led) -> Option<&NSImageView> {
        self.views.get(led).as_deref()
    }

    /// The lights' columns, which `baud-app/tests/titlebar_mark.rs` lays out
    /// against.
    #[allow(dead_code)]
    pub(crate) fn columns(&self) -> Columns {
        self.columns
    }

    /// The view of the accessory they are in, none until the window is
    /// shown (`full_screen_check.rs` reads it).
    #[cfg_attr(feature = "effect-probe", allow(dead_code))]
    pub(crate) fn holder_view(
        &self,
    ) -> Option<Retained<macos_liquid_glass::objc2_app_kit::NSView>> {
        self.holder.as_ref().map(|h| h.view.clone())
    }

    /// The face or the window changed under the pictures: the next tick
    /// draws them again.
    pub(crate) fn stale(&mut self) {
        self.pictures = None;
        self.plates = None;
        self.on_view = ByLed::default();
    }

    /// **The lights at `shown`**, under `setting`, drawn for a strip of
    /// `inset` points and placed between the strip's `edges`: the views
    /// put up, their pictures drawn and swapped, or taken down -- off, or
    /// on their own line in the frame.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn show(
        &mut self,
        setting: ActivityLights,
        wanted: Drawn,
        shown: Shown,
        renderer: &Renderer<'_, '_>,
        edges: crate::strip::Edges,
        inset: CGFloat,
    ) {
        let Some(container) = self.holder.as_ref().map(|h| h.view.clone()) else {
            return;
        };
        if setting != ActivityLights::TitleBar {
            for led in Led::ALL {
                if let Some(v) = self.views.get_mut(led).take() {
                    v.removeFromSuperview();
                }
            }
            if let Some(v) = self.plate.take() {
                v.removeFromSuperview();
            }
            self.on_view = ByLed::default();
            self.place(edges);
            return;
        }
        let have = self.pictures.as_ref().map(|(d, _)| *d);
        match step(setting, wanted, have, shown, self.on_view) {
            Step::Nothing => return,
            Step::Redraw => {
                let Some(columns) = columns(renderer, wanted.scale) else {
                    return;
                };
                let Some(drawn) = pictures(renderer, &wanted.look, wanted, inset) else {
                    return;
                };
                let Some(plates) = plates(&wanted.look, &columns, self.logo, wanted.scale, inset)
                else {
                    return;
                };
                self.columns = columns;
                self.scale = wanted.scale;
                self.colour = wanted.look.plate;
                self.pictures = Some((wanted, drawn));
                self.plates = Some(plates);
                self.on_view = ByLed::default();
            }
            Step::Swap => {}
        }
        if self.views.iter().any(|(_, v)| v.is_none()) || self.plate.is_none() {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            // The plate first, so every light's view is added above it.
            if self.plate.is_none() {
                let v = NSImageView::new(mtm);
                v.setImageScaling(NSImageScaling::ScaleNone);
                container.addSubview(&v);
                self.plate = Some(v);
            }
            for led in Led::ALL {
                if self.views.get(led).is_some() {
                    continue;
                }
                let v = NSImageView::new(mtm);
                v.setImageScaling(NSImageScaling::ScaleNone);
                // No `toolTip`: baud draws the hover text at once, where
                // AppKit's would wait out the system's delay and show
                // over baud's. The pointer crossing the light is heard.
                let waker = self.waker;
                self.crossings
                    .get_or_insert_with(|| Crossings::new(mtm, waker))
                    .watch(&v);
                // What VoiceOver says of the light: its letters, and its
                // hover text set as the element's help.
                v.setAccessibilityLabel(Some(&NSString::from_str(led.letters())));
                v.setAccessibilityHelp(Some(&NSString::from_str(led.tip())));
                container.addSubview(&v);
                *self.views.get_mut(led) = Some(v);
            }
        }
        self.place(edges);
        let Some((_, p)) = &self.pictures else {
            return;
        };
        for led in Led::ALL {
            let light = *shown.get(led);
            if *self.on_view.get(led) == Some(light) {
                continue;
            }
            if let Some(v) = self.views.get(led) {
                v.setImage(Some(p.get(led).get(light)));
                *self.on_view.get_mut(led) = Some(light);
            }
        }
    }

    /// **How wide the lights are, in points**: none with no lights.
    pub(crate) fn across(&self) -> CGFloat {
        match self.plate {
            Some(_) => self.columns.spans.width,
            None => 0.0,
        }
    }

    /// **What the lockup on the plate is placed for**: how wide the lights
    /// are and the scale they are drawn at, a change of either moving the
    /// plate, and the plate's colour, a change of which can change the
    /// lockup's form (`Mark::lights_moved`).
    pub(crate) fn fitted(&self) -> crate::mark::Fitted {
        crate::mark::Fitted {
            across: self.across(),
            scale: self.scale,
            plate: self.plate.as_ref().map(|_| self.colour),
        }
    }

    /// **Re-place every view at the strip's right**, and hide the ones a
    /// narrow window has no room for: on every resize. The accessory first,
    /// to the plate's width and the room `edges` leave, none with no
    /// lights shown; then the plate and each light in it, in its own points,
    /// at the height AppKit gave it. Where the lockup stands on the plate,
    /// for `Mark::fit`; none with no plate, or none placed.
    pub(crate) fn place(&self, edges: crate::strip::Edges) -> Option<crate::mark::OnPlate> {
        let holder = self.holder.as_ref()?;
        let inset = holder.view.frame().size.height;
        let at = match self.plate {
            Some(_) => panel(
                NSSize::new(edges.width, inset),
                inset,
                edges.left,
                edges.right,
                self.scale,
                &self.columns,
                self.logo,
            ),
            None => None,
        };
        let margin = plate_margin(self.scale);
        let width = at.map_or(0.0, |p| p.plate.size.width);
        let (room, _) = holder.fit_lights(edges, width, margin, self.scale);
        let shift = edges.width - room.width;
        let moved = |r: NSRect| NSRect::new(NSPoint::new(r.origin.x - shift, r.origin.y), r.size);
        if let Some(v) = &self.plate {
            match (at, &self.plates) {
                (Some(p), Some(pictures)) => {
                    let r = moved(p.plate);
                    v.setImage(Some(pictures.get(p.first)));
                    v.setFrame(NSRect::new(
                        NSPoint::new(r.origin.x - margin, r.origin.y - margin),
                        NSSize::new(r.size.width + 2.0 * margin, r.size.height + 2.0 * margin),
                    ));
                    v.setHidden(false);
                }
                _ => v.setHidden(true),
            }
        }
        for led in Led::ALL {
            let Some(v) = self.views.get(led) else {
                continue;
            };
            match at.and_then(|p| *p.lamps.get(led)) {
                Some((rect, _)) => {
                    v.setFrame(moved(rect));
                    v.setHidden(false);
                }
                None => v.setHidden(true),
            }
        }
        at.map(|p| crate::mark::OnPlate {
            holder: holder.view.clone(),
            logo: moved(p.logo),
            plate_left: p.plate.origin.x,
            placed: match p.first == Led::ALL[0] {
                true => crate::mark::Placed::Every,
                false => crate::mark::Placed::Fewer,
            },
            colour: self.colour,
        })
    }
}

/// **Every light's column and letters in points at `scale`**, from the
/// renderer's layout in device pixels.
pub(crate) fn columns(renderer: &Renderer<'_, '_>, scale: CGFloat) -> Option<Columns> {
    let (slots, width) = renderer.panel_layout(scale as f32)?;
    let pt = |px: baud_font::Px| CGFloat::from(px.value()) / scale;
    let letters = renderer.panel_letters();
    Some(Columns {
        spans: Spans {
            at: ByLed::of(|l| {
                let s = slots.get(l);
                (pt(s.start), pt(s.width))
            }),
            width: pt(width),
        },
        letters: ByLed::of(|l| {
            let (x, w) = *letters.get(l);
            (CGFloat::from(x), CGFloat::from(w))
        }),
    })
}

/// **The plate's picture for each light it can start at**, `look`'s at
/// `scale`: as [`panel`] places it, so a narrow window that drops the
/// leftmost lights shows the plate under the rest. Any width does: the
/// plate's own width at a scale is the same at all of them.
fn plates(
    look: &LightsLook,
    columns: &Columns,
    logo: NSSize,
    scale: CGFloat,
    inset: CGFloat,
) -> Option<ByLed<Retained<NSImage>>> {
    let px = |pt: CGFloat| {
        u32::try_from((pt * scale).round() as i64)
            .ok()
            .map(baud_font::Px::of)
    };
    let one = |led: Led| {
        let room = NSSize::new(10_000.0, inset);
        let right = CGFloat::from(PANEL_RIGHT);
        let left = exact_left(columns, led, room.width - right, logo) - 1.0;
        let plate = panel(room, inset, left, right, scale, columns, logo)?;
        let mut img = LightsImage::new();
        draw_panel_plate(
            look,
            px(plate.plate.size.width)?,
            px(plate.plate.size.height)?,
            scale as f32,
            &mut img,
        )?;
        baud_present_macos::lights::picture(&img, scale)
    };
    Some(ByLed {
        hs: one(Led::Hs)?,
        aa: one(Led::Aa)?,
        cd: one(Led::Cd)?,
        oh: one(Led::Oh)?,
        rd: one(Led::Rd)?,
        sd: one(Led::Sd)?,
        cs: one(Led::Cs)?,
        fax: one(Led::Fax)?,
        syn: one(Led::Syn)?,
    })
}

/// Each light's three pictures of `look` at `wanted`'s scale, `inset`
/// points tall.
fn pictures(
    renderer: &Renderer<'_, '_>,
    look: &LightsLook,
    wanted: Drawn,
    inset: CGFloat,
) -> Option<ByLed<ByLight<Retained<NSImage>>>> {
    let mut img = LightsImage::new();
    let mut one = |led: Led, light: Light| {
        renderer.panel_light(
            look,
            led,
            light,
            wanted.scale as f32,
            inset as f32,
            &mut img,
        )?;
        baud_present_macos::lights::picture(&img, wanted.scale)
    };
    let mut all: Vec<Option<ByLight<Retained<NSImage>>>> = Vec::new();
    for led in Led::ALL {
        let [dark, lit, unread] = Light::ALL.map(|l| one(led, l));
        all.push(Some(ByLight {
            dark: dark?,
            lit: lit?,
            unread: unread?,
        }));
    }
    let mut it = all.into_iter();
    let mut next = || it.next().flatten();
    Some(ByLed {
        hs: next()?,
        aa: next()?,
        cd: next()?,
        oh: next()?,
        rd: next()?,
        sd: next()?,
        cs: next()?,
        fax: next()?,
        syn: next()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use baud_render::{Halo, LightsColours, Theme};

    /// The look of `theme` in `set` at `glow` and `halo`.
    fn look_of(theme: &Theme, set: LightsColours, glow: Glow, halo: Halo) -> LightsLook {
        LightsLook::of(&theme.palette, theme.fg, theme.bg, set, glow, halo)
    }

    fn at() -> Drawn {
        Drawn {
            scale: 2.0,
            look: look_of(
                &Theme::dark(),
                LightsColours::Meaning,
                Glow::Full,
                Halo::default(),
            ),
        }
    }

    fn rd_lit() -> Shown {
        ByLed {
            rd: Light::Lit,
            ..Shown::DARK
        }
    }

    fn showing(s: Shown) -> ByLed<Option<Light>> {
        ByLed::of(|l| Some(*s.get(l)))
    }

    /// **A light that changes swaps a picture, and only a change of what
    /// they were drawn for draws them again**: a new state of any one
    /// light swaps, the same state does nothing, a new scale, theme, set,
    /// glow strength or Dim Flashing Lights redraws, and off does nothing
    /// whatever moved.
    #[test]
    fn a_change_of_light_swaps_and_a_change_of_look_redraws() {
        use ActivityLights::{BottomLine, Off, TitleBar as On, TopLine};
        let at = at();
        let rd = rd_lit();
        assert_eq!(step(On, at, None, rd, ByLed::default()), Step::Redraw);
        assert_eq!(step(On, at, Some(at), rd, showing(Shown::DARK)), Step::Swap);
        assert_eq!(step(On, at, Some(at), rd, showing(rd)), Step::Nothing);
        for led in Led::ALL {
            let one = {
                let mut s = Shown::DARK;
                *s.get_mut(led) = Light::Unread;
                s
            };
            assert_eq!(
                step(On, at, Some(at), one, showing(Shown::DARK)),
                Step::Swap,
                "{led:?}"
            );
        }
        let dark = Theme::dark();
        let m = LightsColours::Meaning;
        for other in [
            Drawn { scale: 1.0, ..at },
            Drawn {
                look: look_of(&dark, m, Glow::Dimmed, Halo::default()),
                ..at
            },
            Drawn {
                look: look_of(&Theme::light(), m, Glow::Full, Halo::default()),
                ..at
            },
            Drawn {
                look: look_of(&dark, LightsColours::Iec, Glow::Full, Halo::default()),
                ..at
            },
            Drawn {
                look: look_of(&dark, m, Glow::Full, Halo::of(1.0)),
                ..at
            },
        ] {
            assert_eq!(step(On, other, Some(at), rd, showing(rd)), Step::Redraw);
        }
        for off in [Off, BottomLine, TopLine] {
            assert_eq!(step(off, at, None, rd, ByLed::default()), Step::Nothing);
            assert_eq!(
                step(off, at, Some(at), rd, showing(Shown::DARK)),
                Step::Nothing
            );
        }
    }

    /// Nine lights 20 points apart, each 15 wide, the last ending at 175.
    fn spans_for_test() -> Spans {
        let mut x = 0.0;
        Spans {
            at: ByLed::of(|_| {
                let s = (x, 15.0);
                x += 20.0;
                s
            }),
            width: 175.0,
        }
    }

    /// **The lights sit at the strip's right, the strip's height, its top
    /// the window's, left to right in the panel's order**, the last
    /// `right` in from the edge; and in a window too narrow for them all,
    /// the ones that would reach the lockup are not placed, the leftmost
    /// first, while the rest keep their places.
    #[test]
    fn the_lights_sit_at_the_strips_right_and_a_narrow_window_drops_the_leftmost() {
        let spans = spans_for_test();
        let wide = placed(NSSize::new(800.0, 500.0), 28.0, 9.0, 100.0, 2.0, &spans);
        let mut last_end = None;
        for led in Led::ALL {
            let (r, _) = wide.get(led).expect("every light in a wide window");
            assert_eq!((r.origin.y, r.size.height), (472.0, 28.0), "{led:?}");
            if let Some(end) = last_end {
                assert!(r.origin.x >= end, "{led:?} right of the last");
            }
            last_end = Some(r.origin.x + r.size.width);
        }
        assert_eq!(wide.syn.unwrap().0.origin.x + 15.0, 800.0 - 9.0);
        // 300 points: the strip starts at 116, so the first light, at 116,
        // clears a mark ending at 100; at 250 it starts at 66, so the
        // first three (66, 86) would reach it and the fourth (106) is
        // placed.
        let some = placed(NSSize::new(300.0, 500.0), 28.0, 9.0, 100.0, 2.0, &spans);
        assert!(Led::ALL.iter().all(|&l| some.get(l).is_some()));
        let narrow = placed(NSSize::new(250.0, 500.0), 28.0, 9.0, 100.0, 2.0, &spans);
        let kept: Vec<Led> = Led::ALL
            .into_iter()
            .filter(|&l| narrow.get(l).is_some())
            .collect();
        assert_eq!(&kept[..], &Led::ALL[2..], "the two leftmost go");
        for &l in &kept {
            assert_eq!(narrow.get(l).unwrap().0, {
                let (r, t) = wide.get(l).unwrap();
                assert_eq!(t, l.tip());
                NSRect::new(NSPoint::new(r.origin.x - 550.0, r.origin.y), r.size)
            });
        }
        // The strip's left end lands on a whole device pixel.
        let odd = placed(NSSize::new(800.25, 500.0), 28.0, 9.0, 100.0, 2.0, &spans);
        let x = odd.hs.unwrap().0.origin.x * 2.0;
        assert_eq!(x, x.round());
    }

    /// The columns the renderer lays out at `scale`, in baud's embedded
    /// JetBrains Mono Bold, and the lockup's size beside AppKit's buttons.
    fn real(scale: CGFloat) -> (Columns, NSSize) {
        use baud_domain::{Cols, Rows};
        use baud_render::{Blend, PxSize, Stack};
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
        let k = 22.4 / 300.0;
        let art = crate::mark::Lockup::OnDark
            .art()
            .expect("the lockup's size");
        (
            columns(&r, scale).unwrap(),
            NSSize::new(art.width * k, art.height * k),
        )
    }

    /// Where `strip::edges` starts the plate in a window AppKit has laid
    /// out: the zoom button's end, 69 points, and the gap between two
    /// traffic lights, 9.
    const LEFT: CGFloat = 78.0;

    /// The panel in a strip as tall as the window's, from [`LEFT`].
    fn panel_at(width: CGFloat, scale: CGFloat) -> Option<Panel> {
        let (c, logo) = real(scale);
        panel(
            NSSize::new(width, 28.0),
            28.0,
            LEFT,
            CGFloat::from(PANEL_RIGHT),
            scale,
            &c,
            logo,
        )
    }

    /// **The module's numbers**: the plate 221 points across, 220.97 before
    /// rounding, and 24 tall, y 2 to 26, its right edge 16 in from the
    /// window's; the lockup 7 from its left, 51.37 by 12.54, centred on the
    /// strip; 10 from the lockup to HS's letters and 7 from SYN's to the
    /// plate's right, each to within a device pixel; the nine in today's
    /// order, each column the strip's height. Perturbation: a pad of 4, a
    /// right inset of 9, or a gap of 9, and this goes red.
    #[test]
    fn the_module_has_its_numbers_on_the_plate() {
        for scale in [1.0, 2.0] {
            for width in [960.0, 786.0, 800.25, 1200.0] {
                let p = panel_at(width, scale).expect("a panel");
                let at = format!("{width} at {scale}x");
                let (c, logo) = real(scale);
                assert_eq!(p.first, Led::Hs, "{at}");
                let px = 1.0 / scale;
                let right = p.plate.origin.x + p.plate.size.width;
                assert!((right - (width - 16.0)).abs() <= px / 2.0, "{at}: {right}");
                assert_eq!(right * scale, (right * scale).round(), "{at}: whole");
                assert_eq!(p.plate.origin.y, 2.0, "{at}");
                assert_eq!(p.plate.size.height, 24.0, "{at}");
                assert_eq!(p.plate.size.width, 221.0, "{at}");
                assert_eq!(p.logo.origin.x - p.plate.origin.x, 7.0, "{at}");
                // The lockup's frame on whole device pixels, each edge within
                // one of where the exact size centred on the strip
                // puts it (`mark::snapped`).
                let exact = (28.0 - logo.height) / 2.0;
                for (got, want) in [
                    (p.logo.origin.y, exact),
                    (p.logo.origin.y + p.logo.size.height, exact + logo.height),
                    (
                        p.logo.origin.x + p.logo.size.width,
                        p.logo.origin.x + logo.width,
                    ),
                ] {
                    assert!((got - want).abs() <= px + 1e-9, "{at}: {:?}", p.logo);
                    assert_eq!(got * scale, (got * scale).round(), "{at}: {:?}", p.logo);
                }
                assert!((logo.width - 51.366).abs() < 0.01, "{at}: {logo:?}");
                assert!((logo.height - 12.544).abs() < 1e-9, "{at}: {logo:?}");
                assert!(
                    (p.logo.origin.y + p.logo.size.height / 2.0 - 14.0).abs() <= px / 2.0,
                    "{at}: centred"
                );
                let (hs, _) = p.lamps.hs.unwrap();
                let hs_letters = hs.origin.x + c.letters.hs.0 - c.spans.at.hs.0;
                let gap = hs_letters - (p.logo.origin.x + logo.width);
                assert!((gap - 10.0).abs() <= 2.0 * px / 2.0, "{at}: gap {gap}");
                let (syn, _) = p.lamps.syn.unwrap();
                let syn_end = syn.origin.x + c.letters.syn.0 - c.spans.at.syn.0 + c.letters.syn.1;
                assert!(
                    (right - syn_end - 7.0).abs() <= 2.0 * px / 2.0,
                    "{at}: pad {}",
                    right - syn_end
                );
                let mut end = None;
                for led in Led::ALL {
                    let (r, tip) = p.lamps.get(led).expect("all nine");
                    assert_eq!(tip, led.tip(), "{at}");
                    assert_eq!((r.origin.y, r.size.height), (0.0, 28.0), "{at} {led:?}");
                    if let Some(e) = end {
                        assert_eq!(r.origin.x, e, "{at} {led:?}: side by side");
                    }
                    end = Some(r.origin.x + r.size.width);
                }
            }
        }
    }

    /// **The lamps' letters, laid out**: 4.8 points a letter in JetBrains
    /// Mono Bold at 8, with 6.2 between one's letters and the next's, so
    /// 145.6 across the nine, in today's order. Perturbation: a letter gap
    /// of 9, and this goes red.
    #[test]
    fn the_lamps_letters_are_6_2_apart_and_145_6_across() {
        let (c, _) = real(2.0);
        let mut x = c.letters.hs.0;
        for led in Led::ALL {
            let (at, w) = *c.letters.get(led);
            assert!((at - x).abs() < 1e-4, "{led:?}");
            let letters = led.letters().len() as CGFloat;
            assert!((w - 4.8 * letters).abs() < 1e-4, "{led:?}: {w}");
            x = at + w + 6.2;
        }
        let across = c.end() - c.letters.hs.0;
        assert!((across - 145.6).abs() < 1e-4, "{across}");
        assert!((c.letters.hs.0 - 3.1).abs() < 1e-4);
    }

    /// **A narrow window drops the leftmost lamps, the lockup stays whole and
    /// the plate shrinks with them**, the plate starting where
    /// `strip::edges` starts it, [`LEFT`]: all nine from 315 points, 314
    /// drops HS; 340 keeps all nine; 260 keeps RD on, 220 FAX and SYN; SYN
    /// alone from 184; the plate's left never past [`LEFT`]; the plate's
    /// right where it was; the lockup whole at the plate's pad; the first
    /// lamp kept the plate's picture. At every half point from 200 to 400
    /// fewer or the same lamps as the next width up, and always the
    /// rightmost. Under 184 no lamp fits and there is no plate.
    /// Perturbation: start the plate at the first lamp whatever is kept, or
    /// test the rounded plate rather than the exact one (220 keeps CS), and
    /// this goes red.
    #[test]
    fn a_narrow_window_drops_the_leftmost_and_the_plate_shrinks() {
        let kept = |width: CGFloat, scale: CGFloat| -> Vec<Led> {
            panel_at(width, scale).map_or_else(Vec::new, |p| {
                Led::ALL
                    .into_iter()
                    .filter(|&l| p.lamps.get(l).is_some())
                    .collect()
            })
        };
        for scale in [1.0, 2.0] {
            assert_eq!(kept(340.0, scale), Led::ALL, "340 at {scale}x");
            assert_eq!(kept(315.0, scale), Led::ALL, "315 at {scale}x");
            assert_eq!(kept(314.0, scale), &Led::ALL[1..], "314 at {scale}x");
            assert_eq!(kept(260.0, scale), &Led::ALL[4..], "260 at {scale}x");
            assert_eq!(kept(220.0, scale), &Led::ALL[7..], "220 at {scale}x");
            assert_eq!(kept(184.0, scale), &Led::ALL[8..], "184 at {scale}x");
            assert_eq!(kept(183.75, scale), [], "183.75 at {scale}x");
            let mut width = 200.0;
            let mut last = 0;
            while width <= 400.0 {
                let k = kept(width, scale);
                assert!(k.len() >= last, "{width} at {scale}x");
                assert_eq!(k.last(), Some(&Led::Syn), "{width} at {scale}x");
                last = k.len();
                let p = panel_at(width, scale).unwrap();
                let at = format!("{width} at {scale}x");
                assert!(p.plate.origin.x >= LEFT, "{at}: {:?}", p.plate);
                assert_eq!(p.first, k[0], "{at}");
                assert_eq!(p.logo.origin.x - p.plate.origin.x, 7.0, "{at}");
                assert!(
                    (p.plate.origin.x + p.plate.size.width - (width - 16.0)).abs() <= 0.5 / scale,
                    "{at}"
                );
                let (c, logo) = real(scale);
                let (x, _) = *c.letters.get(p.first);
                let want = 7.0 + logo.width + 10.0 + (c.end() - x) + 7.0;
                assert!(
                    (p.plate.size.width - want).abs() <= 1.5 / scale,
                    "{at}: {} against {want}",
                    p.plate.size.width
                );
                width += 0.5;
            }
        }
    }

    /// **No lockup, no gap**: a window whose lockup did not decode still
    /// has the lamps on their plate, pad to pad.
    #[test]
    fn no_lockup_keeps_the_lamps_on_their_plate() {
        let (c, _) = real(2.0);
        let p = panel(
            NSSize::new(960.0, 28.0),
            28.0,
            69.0,
            CGFloat::from(PANEL_RIGHT),
            2.0,
            &c,
            NSSize::new(0.0, 0.0),
        )
        .unwrap();
        let want = 7.0 + (c.end() - c.letters.hs.0) + 7.0;
        assert!((p.plate.size.width - want).abs() <= 0.75, "{:?}", p.plate);
    }

    /// **The accessory holds the whole plate, its bevel's edge included, and
    /// is no wider**: the plate and the lamps moved into its own points keep
    /// their whole device pixels, at 960, 786, 800.25 and 200 points at
    /// scales 1 and 2, and the plate's picture fits in it. Perturbation: put
    /// the accessory's width, rather than its left edge, on a whole device
    /// pixel and the 800.25-point window's lamps move by half of one.
    #[test]
    fn the_accessory_holds_the_plate_and_keeps_its_pixels_whole() {
        use crate::strip::{Edges, lights_width};
        for scale in [1.0, 2.0] {
            for width in [960.0, 786.0, 800.25, 200.0] {
                let p = panel_at(width, scale).unwrap();
                let edges = Edges {
                    width,
                    left: 69.0,
                    right: CGFloat::from(PANEL_RIGHT),
                };
                let m = plate_margin(scale);
                let h = lights_width(edges, p.plate.size.width, m, scale);
                let shift = width - h;
                let at = format!("{width} at {scale}x");
                let left = p.plate.origin.x - shift - m;
                assert!(left.abs() <= 1.0 / scale + 1e-9, "{at}: {left}");
                assert!(left >= -1e-9, "{at}: the plate is whole: {left}");
                let frame = (p.plate.origin.x - shift) * scale;
                assert!((frame - frame.round()).abs() < 1e-9, "{at}: {frame}");
                for led in Led::ALL {
                    if let Some((r, _)) = p.lamps.get(led) {
                        let x = (r.origin.x - shift) * scale;
                        assert!((x - x.round()).abs() < 1e-9, "{at} {led:?}: {x}");
                        assert!(r.origin.x + r.size.width <= width, "{at} {led:?}");
                    }
                }
            }
        }
    }

    /// **The tip and the tracking area follow the columns**: every column of
    /// the panel holds its lamp's centre and its letters, and a point at the
    /// edge between two columns is over one, so the pointer is over at most
    /// one lamp anywhere in the strip. Perturbation: give each column its
    /// picture's old width, 13 points past its start, and two columns
    /// overlap.
    #[test]
    fn the_columns_partition_the_strip_for_the_tips() {
        for scale in [1.0, 2.0] {
            let p = panel_at(960.0, scale).unwrap();
            let (c, _) = real(scale);
            let inside = |r: &NSRect, x: CGFloat| x >= r.origin.x && x < r.origin.x + r.size.width;
            for led in Led::ALL {
                let (r, _) = p.lamps.get(led).unwrap();
                let (lx, lw) = *c.letters.get(led);
                let x0 = r.origin.x - c.spans.at.get(led).0;
                for x in [x0 + lx, x0 + lx + lw / 2.0, x0 + lx + lw - 0.01] {
                    let hits: Vec<Led> = Led::ALL
                        .into_iter()
                        .filter(|&o| inside(&p.lamps.get(o).unwrap().0, x))
                        .collect();
                    assert_eq!(hits, [led], "{x} at {scale}x");
                }
            }
            let from = p.lamps.hs.unwrap().0.origin.x;
            let to = {
                let r = p.lamps.syn.unwrap().0;
                r.origin.x + r.size.width
            };
            let mut x = from;
            while x < to {
                let n = Led::ALL
                    .into_iter()
                    .filter(|&o| inside(&p.lamps.get(o).unwrap().0, x))
                    .count();
                assert_eq!(n, 1, "{x} at {scale}x");
                x += 0.25;
            }
        }
    }

    /// **Each light's rectangle shows its own text, and only its own**:
    /// the point at the middle of every placed light falls in that light's
    /// rectangle and in no other's, and the text there names that light's
    /// letters and nothing else's; the texts are nine different ones, each
    /// spelling out its letters, CS's saying it is lit while the program
    /// takes what baud sends, as a modem's is, and nothing of an inversion.
    #[test]
    fn each_lights_rectangle_maps_to_its_own_hover_text() {
        let spans = spans_for_test();
        let frames = placed(NSSize::new(800.0, 500.0), 28.0, 9.0, 100.0, 2.0, &spans);
        let inside = |r: &NSRect, p: NSPoint| {
            p.x >= r.origin.x
                && p.x < r.origin.x + r.size.width
                && p.y >= r.origin.y
                && p.y < r.origin.y + r.size.height
        };
        for led in Led::ALL {
            let (r, _) = frames.get(led).unwrap();
            let mid = NSPoint::new(r.origin.x + r.size.width / 2.0, r.origin.y + 14.0);
            let hits: Vec<&str> = frames
                .iter()
                .filter_map(|(_, f)| *f)
                .filter(|(r, _)| inside(r, mid))
                .map(|(_, t)| t)
                .collect();
            assert_eq!(hits, [led.tip()], "{led:?}");
            let tip = led.tip();
            assert!(tip.starts_with(&format!("{} -- ", led.letters())), "{tip}");
            for other in Led::ALL.into_iter().filter(|&o| o != led) {
                assert_ne!(tip, other.tip());
                assert!(
                    !tip.starts_with(&format!("{} ", other.letters())),
                    "{tip} names {other:?}"
                );
            }
        }
        let cs = Led::Cs.tip();
        assert!(cs.contains("lit while the program takes"), "{cs}");
        assert!(!cs.contains("invert"), "{cs}");
        assert!(Led::Rd.tip().contains("Receive Data"));
        assert!(Led::Sd.tip().contains("Send Data"));
    }

    /// **A reading feeds its light's clock**: lit lights it, refused shows
    /// the ring, dark does nothing.
    #[test]
    fn a_reading_lights_rings_or_leaves_its_light() {
        use baud_domain::HostTime;
        use baud_render::{Activity, Pace};
        use core::time::Duration;
        let t = |ms: u64| HostTime::ZERO.plus(Duration::from_millis(1000 + ms));
        let mut a = Activity::new();
        feed(&mut a, Led::Oh, crate::front::Reading::Lit, t(0));
        feed(&mut a, Led::Cd, crate::front::Reading::Refused, t(0));
        feed(&mut a, Led::Rd, crate::front::Reading::Dark, t(0));
        let s = a.step(t(0), Pace::Full);
        assert_eq!((s.oh, s.cd, s.rd), (Light::Lit, Light::Unread, Light::Dark));
    }

    /// **Each light's three pictures and the plate's, headless, in either
    /// theme and glow**: drawn by the renderer in baud's embedded
    /// JetBrains Mono Bold in the theme's look, each light's its column's
    /// width and the strip's height in points at the scale asked, its lit and
    /// dark pictures different, and for OH and CD, which can be unread, the
    /// unread one a third; the plate's for each light it can start at
    /// [`PANEL_HEIGHT`] tall and its bevel's margin past it, as wide as
    /// [`panel`] makes it. Changed with the plate's geometry: it held the
    /// picture beside the letters, and the 20-point plate, before.
    #[test]
    fn each_lights_pictures_are_drawn_at_its_size() {
        use baud_domain::{Cols, Rows};
        use baud_render::{Blend, PxSize, Stack};
        let faces = [baud_font::Face::parse(crate::embedded::BOLD.bytes).unwrap()];
        for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
            let r = Renderer::new(
                Stack::new(&faces).unwrap(),
                PxSize::of(28.0).unwrap(),
                theme,
                Blend::Native,
                Cols::of_index(6),
                Rows::of_index(2),
            )
            .unwrap();
            let (_, logo) = real(2.0);
            for glow in [Glow::Full, Glow::Dimmed] {
                let look = r.lights_look(glow);
                let wanted = Drawn { scale: 2.0, look };
                let p = pictures(&r, &look, wanted, 28.0).expect("every picture");
                let c = columns(&r, 2.0).expect("the layout");
                let plate = plates(&look, &c, logo, 2.0, 28.0).expect("every plate");
                for led in Led::ALL {
                    let (_, width) = *c.spans.at.get(led);
                    let pics = p.get(led);
                    for light in Light::ALL {
                        let size = pics.get(light).size();
                        assert_eq!(size.height, 28.0, "{name} {led:?}");
                        assert_eq!(size.width, width, "{name} {led:?} {light:?}");
                    }
                    let tiff = Light::ALL.map(|l| pics.get(l).TIFFRepresentation());
                    assert_ne!(tiff[0], tiff[1], "{name} {glow:?} {led:?}: lit is not dark");
                    if led.reads_a_process() {
                        assert_ne!(tiff[0], tiff[2], "{name} {led:?}: unread is not dark");
                        assert_ne!(tiff[1], tiff[2], "{name} {led:?}: unread is not lit");
                    }
                    let size = plate.get(led).size();
                    let m = plate_margin(2.0);
                    assert_eq!(
                        size.height,
                        CGFloat::from(PANEL_HEIGHT) + 2.0 * m,
                        "{name} {led:?}"
                    );
                    let (x, _) = *c.letters.get(led);
                    let want = 7.0 + logo.width + 10.0 + (c.end() - x) + 7.0 + 2.0 * m;
                    assert!((size.width - want).abs() <= 1.0, "{name} {led:?}: {size:?}");
                }
            }
        }
    }

    /// **Dim Flashing Lights draws the lights without their glow**: macOS's
    /// setting, read into the same value the text blink's dimmed phases
    /// read, maps to the look with no halo when on and to the glowing one
    /// when off. Perturbation: map `Flashing::Dimmed` to `Glow::Full` --
    /// the setting stops taking the glow away -- and this goes red.
    #[test]
    fn dim_flashing_lights_draws_the_lights_without_their_glow() {
        use baud_present_macos::settings::Flashing;
        assert_eq!(glow(Flashing::Full), Glow::Full);
        assert_eq!(glow(Flashing::Dimmed), Glow::Dimmed);
    }

    /// **The tick lights them, the pump and the writes feed them**: the
    /// driver steps the clocks on the display link's tick at the motion
    /// preference's pace and shows them through this module at the glow
    /// the Dim Flashing Lights setting gives (`glow`); the tick reads
    /// every turn reads OH and CD only through `Front::read`, scheduling
    /// the next listing's wake (`front_turn`), and holds them as states;
    /// every turn feeds SYN from the core's state, holds FAX and CS as
    /// states and gives DEC's line its words (`lights_turn`), the lights
    /// asking for the tick that steps them (`Deadline::Lights`); a drain that brought bytes gives RD its latest read's
    /// stamp and HS each full read's; a reply the core queued during a feed,
    /// or one nobody typed at a flush, lights AA; a write the kernel took
    /// any of stamps SD, and one it refused any of, or bytes still owed,
    /// darken CS; the process
    /// watch tells the sampler; the window's appearance makes the pictures
    /// stale, and a resize re-places them.
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
        let front = body(concat!("    fn front_", "turn(&mut self) {"));
        for want in [
            "crate::front::Reads::of(self.app.activity_lights, line, self.app.interval)",
            ".read(reads, &mut self.procs, self.app.pty.child(), now);",
            ".schedule(self.front.next_listing(reads), now)",
        ] {
            assert!(front.contains(want), "{want}\n{front}");
        }
        assert!(!show.contains("self.front."), "no read at the tick\n{show}");
        for want in [
            "crate::access::lights_pace(self.app.motion)",
            "let glow = crate::lights::glow(self.app.flashing);",
            "look: self.app.renderer.lights_look(glow),",
            "self.lights.show(",
        ] {
            assert!(show.contains(want), "{want}\n{show}");
        }
        let turn = body(concat!("    fn lights_", "turn(&mut self) {"));
        for want in [
            "let synced = self.synced.seen(&self.app.term);",
            "self.app.term.images().any_placed()",
            "self.app.activity.hold(Led::Fax, fax, now);",
            "crate::line_lamps::cs_level(&self.app.outbox)",
            "self.hold_front(now);",
            "self.give_line_words(clause);",
        ] {
            assert!(turn.contains(want), "{want}\n{turn}");
        }
        let held = body(concat!("    fn hold_", "front(&mut self, now: HostTime) {"));
        for want in [
            "self.app.activity.hold(Led::Oh, oh, now);",
            "self.app.activity.hold(Led::Cd, cd, now);",
        ] {
            assert!(held.contains(want), "{want}\n{held}");
        }
        let pump = body(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"));
        for want in [
            "self.lights_turn();",
            "self.front_turn();",
            "self.app.activity.seen(Led::Rd, last.at);",
            "self.app.activity.seen(Led::Hs, chunk.at);",
            "baud_render::lights::full(&chunk.bytes)",
            "self.app.activity.seen(Led::Aa, host_now());",
            "self.front.moved();",
            "self.app.flush_reply(Asked::Unasked);",
        ] {
            assert!(pump.contains(want), "{want}");
        }
        let flush = body(concat!(
            "    fn flush_reply(&mut self, ",
            "asked: Asked) -> Sent {"
        ));
        for want in [
            "Asked::Unasked if !self.term.reply().is_empty() => self.activity.seen(Led::Aa, now),",
            "Wrote::All => {\n                self.activity.seen(Led::Sd, now);",
            "Wrote::Part => {\n                self.activity.seen(Led::Sd, now);\n                self.activity.seen(Led::Cs, now);",
            "Wrote::Nothing => {\n                self.activity.seen(Led::Cs, now);",
        ] {
            assert!(flush.contains(want), "{want}\n{flush}");
        }
        let focus = body(concat!(
            "    fn set_focus(&mut self, ",
            "f: baud_core::Focus) {"
        ));
        assert!(
            focus.contains("self.app.flush_reply(Asked::Unasked);"),
            "{focus}"
        );
        // What takes the lockup to the plate and the plate's colour to the
        // lockup: every tick's compare, every placing's fit, and the launch.
        let compare = show
            .find(".is_some_and(|m| m.lights_moved(self.lights.fitted()))")
            .expect("the tick compares what the lockup is placed for");
        assert!(
            show[compare..].contains("self.place_lights();"),
            "and places it\n{show}"
        );
        let place = body(concat!("    fn place_lights(&self) ", "{"));
        for want in [
            "let plate = self.lights.place(self.strip_edges());",
            "mark.fit(self.window.ns_window(), plate, self.lights.fitted());",
        ] {
            assert!(place.contains(want), "{want}\n{place}");
        }
        assert!(
            SRC.contains(concat!(
                "        driver.draw_lights();\n",
                "        driver.place_icon();\n",
                "        driver.place_lights();\n",
            )),
            "the launch draws them and places them at once"
        );
        let tell = body(concat!("    fn tell_pty(&mut self) ", "{"));
        assert!(tell.contains("self.flush_reply(Asked::Unasked);"), "{tell}");
        let style = body(concat!("    fn apply_style(&mut self) ", "{"));
        assert!(style.contains("self.lights.stale();"), "{style}");
        // On their own line: the tick hands the renderer what they show and
        // tells the tips where the lights are, every frame places their records,
        // and a re-lay sets the line at its scale and takes its pixels off
        // the grid's.
        for want in [
            "self.app.renderer.set_lights(shown, glow);",
            "self.place_tips();",
        ] {
            assert!(show.contains(want), "{want}\n{show}");
        }
        let present = body(concat!(
            "    fn present(&mut self, ",
            "screen: &DrawableView, show: Show) {"
        ));
        assert!(
            present.contains("self.renderer.place_lights(&self.term);"),
            "every frame places the line's lights"
        );
        let resized = body(concat!(
            "    fn relayout(&mut self, ",
            "size: NSSize, scale: CGFloat) -> Drew {"
        ));
        assert!(resized.contains("self.place_lights();"), "{resized}");
        for want in [
            ".set_lights_line(self.app.activity_lights.line(), scale as f32);",
            "crate::width::less_line(px_h, self.app.renderer.lights_height()),",
        ] {
            assert!(resized.contains(want), "{want}");
        }
    }
}
