//! **The activity lights in the macOS title strip**: nine lights at the
//! strip's right, each named with a modem's letters -- HS AA CD OH RD SD
//! CS FAX SYN, in the order a 1988 U.S. Robotics Courier manual's table
//! lists them (`baud_render::lights`) -- and each lit only for something
//! baud measured.
//!
//! **Where**: one image view a light, on a plate, in a trailing titlebar
//! accessory -- the title bar's own place for content at its right end,
//! in the traffic lights' view, which AppKit detaches from the window in
//! full screen (`strip.rs`) -- re-placed on every resize. The accessory
//! is sized at every layout to the lights and to the room right of the
//! lockup ([`crate::strip::lights_width`]), since AppKit lets the two
//! ends of a narrow title bar overlap. Each light's view has its own
//! tracking area (`baud_present_macos::hover`): the pointer crossing a
//! light is heard at once, and baud draws the light's hover text in the
//! frame on the next frame (`line_tip.rs`), as it does on Linux, with no
//! `toolTip`, whose system delay is the wait this replaces. What decides
//! which light the pointer is over is each light's frame on the screen
//! ([`Strip::screen_frames`]), through whichever window holds its view --
//! in full screen the title bar's own, not the terminal's
//! (`baud_present_macos::hover`). A light that would
//! reach baud's lockup in a narrow window is not drawn, the leftmost
//! first, so nothing covers the traffic lights or the lockup.
//!
//! **The colours are the theme's** (`Renderer::lights_look`): every lamp
//! one of the theme's sixteen palette entries in the set `[window]
//! activity_lights_colours` names, on a plate of the theme's background, a
//! tenth view under the lights, the letters its foreground -- the colours
//! the lights' own line draws on Linux. The plate is opaque, so a lamp
//! reads against the theme's background whatever the window's material
//! shows through the strip, glass or standard, light or dark; no AppKit
//! colour is read for them, and a change of theme, set or glow draws the
//! pictures again because the look they were drawn for moved.
//!
//! **The pixels are baud's renderer's** (`Renderer::activity_light`): each
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
//! = "bottom_line"` or `"top_line"`): the renderer draws the same pictures
//! there on the same plate (`baud_render::lights`,
//! `Renderer::set_lights_line`), and `line_tip.rs` shows each light's
//! hover text from its place on that line. The Linux window has no
//! title bar -- no compositor decorations are asked for and none are drawn
//! -- so the line is its default, and its lights are drawn there by the
//! Linux driver.

use baud_present_macos::hover::Crossings;
use baud_render::lights::{PLATE_PAD, PLATE_RADIUS_TITLE, PLATE_TITLE, draw_plate};
use baud_render::{
    ActivityLights, ByLed, ByLight, Glow, Led, Light, LightsImage, LightsLook, Renderer, Shown,
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

/// **What OH's or CD's reading tells its clock**: a lit reading is the
/// light's thing seen, a refused one its read refused, a dark one
/// nothing.
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
/// -- baud's lockup -- is not placed, so in a narrow window the leftmost go
/// first.
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

/// **The plate's frame under the lights `frames` placed**, in the same
/// points: from [`PLATE_PAD`] before the first placed light to as far past
/// the last, [`PLATE_TITLE`] tall -- or the strip's `inset` if that is
/// less -- centred on the strip, its top on a whole device pixel at
/// `scale`; none with no light placed. The light the plate starts at
/// with it, whose plate picture it shows.
pub(crate) fn plate_frame(
    frames: &ByLed<Option<(NSRect, &'static str)>>,
    inset: CGFloat,
    scale: CGFloat,
) -> Option<(Led, NSRect)> {
    let scale = scale.max(1.0);
    let shown = || {
        Led::ALL
            .into_iter()
            .filter_map(|l| frames.get(l).map(|f| (l, f.0)))
    };
    let (led, first) = shown().next()?;
    let (_, last) = shown().next_back()?;
    let pad = CGFloat::from(PLATE_PAD);
    let h = CGFloat::from(PLATE_TITLE).min(inset);
    let y = first.origin.y + ((inset - h) / 2.0 * scale).round() / scale;
    let w = last.origin.x + last.size.width - first.origin.x + 2.0 * pad;
    Some((
        led,
        NSRect::new(NSPoint::new(first.origin.x - pad, y), NSSize::new(w, h)),
    ))
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
    spans: Spans,
    /// The scale the spans are whole device pixels at.
    scale: CGFloat,
    /// The lights' tracking areas' owner, made with the views, and whom it
    /// wakes at a crossing.
    crossings: Option<Crossings>,
    waker: Option<Waker>,
}

impl Strip {
    pub(crate) fn new(holder: Option<crate::strip::Accessory>, waker: Option<Waker>) -> Strip {
        Strip {
            holder,
            views: ByLed::default(),
            plate: None,
            pictures: None,
            plates: None,
            on_view: ByLed::default(),
            spans: Spans {
                at: ByLed::default(),
                width: 0.0,
            },
            scale: 1.0,
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

    /// The view of the accessory they are in, none until the window is
    /// shown (`full_screen_check.rs` reads it).
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
                let Some(spans) = spans(renderer, wanted.scale) else {
                    return;
                };
                let Some(drawn) = pictures(renderer, &wanted.look, wanted, inset) else {
                    return;
                };
                let Some(plates) = plates(&wanted.look, &spans, wanted.scale, inset) else {
                    return;
                };
                self.spans = spans;
                self.scale = wanted.scale;
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

    /// Re-place every view at the strip's right, and hide the ones a
    /// narrow window has no room for: on every resize. The accessory first,
    /// to the lights' width and the room `edges` leave, none with no
    /// lights shown; then each light in it, in its own points, at the
    /// height AppKit gave it.
    pub(crate) fn place(&self, edges: crate::strip::Edges) {
        let Some(holder) = &self.holder else {
            return;
        };
        let across = match self.plate {
            Some(_) => self.spans.width,
            None => 0.0,
        };
        let (room, left) = holder.fit_lights(edges, across, CGFloat::from(PLATE_PAD), self.scale);
        let inset = room.height;
        let frames = placed(room, inset, edges.right, left, self.scale, &self.spans);
        if let Some(v) = &self.plate {
            let at = plate_frame(&frames, inset, self.scale);
            match (at, &self.plates) {
                (Some((led, rect)), Some(p)) => {
                    v.setImage(Some(p.get(led)));
                    v.setFrame(rect);
                    v.setHidden(false);
                }
                _ => v.setHidden(true),
            }
        }
        for led in Led::ALL {
            let Some(v) = self.views.get(led) else {
                continue;
            };
            match frames.get(led) {
                Some((rect, _)) => {
                    v.setFrame(*rect);
                    v.setHidden(false);
                }
                None => v.setHidden(true),
            }
        }
    }
}

/// Every light's place in points at `scale`, from the renderer's layout in
/// device pixels.
pub(crate) fn spans(renderer: &Renderer<'_, '_>, scale: CGFloat) -> Option<Spans> {
    let (slots, width) = renderer.activity_lights_layout(scale as f32)?;
    let pt = |px: baud_font::Px| CGFloat::from(px.value()) / scale;
    Some(Spans {
        at: ByLed::of(|l| {
            let s = slots.get(l);
            (pt(s.start), pt(s.width))
        }),
        width: pt(width),
    })
}

/// **The plate's picture for each light it can start at**, `look`'s plate
/// colour at `scale`: from [`PLATE_PAD`] before that light to as far past
/// the last, as [`plate_frame`] places it, so a narrow window that drops
/// the leftmost lights shows the plate under the rest.
fn plates(
    look: &LightsLook,
    spans: &Spans,
    scale: CGFloat,
    inset: CGFloat,
) -> Option<ByLed<Retained<NSImage>>> {
    let (end, w) = *spans.at.get(Led::Syn);
    let pad = CGFloat::from(PLATE_PAD);
    let h = CGFloat::from(PLATE_TITLE).min(inset);
    let px = |pt: CGFloat| {
        u32::try_from((pt * scale).round() as i64)
            .ok()
            .map(baud_font::Px::of)
    };
    let one = |led: Led| {
        let (start, _) = *spans.at.get(led);
        let mut img = LightsImage::new();
        let width = end + w - start + 2.0 * pad;
        draw_plate(
            look,
            px(width)?,
            px(h)?,
            PLATE_RADIUS_TITLE * scale as f32,
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
        renderer.activity_light(
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

    /// **In the titlebar accessory the lights are where the window's own
    /// strip had them**: fitted between the strip's edges
    /// (`strip::lights_width`, `strip::lights_room`) and placed in the
    /// accessory's own points, each light and the plate, moved by where
    /// AppKit puts a trailing accessory -- flush with the window's right
    /// edge -- land on the frames `placed` gives across the whole window,
    /// at 960, 786, 800.25 and 200 points and scales 1 and 2, the narrow
    /// window dropping the same leftmost lights. Perturbation: put the
    /// accessory's width, rather than its left edge, on a whole device
    /// pixel and the 800.25-point window's lights move by half of one.
    #[test]
    fn the_lights_accessory_keeps_the_lights_where_the_strip_had_them() {
        use crate::strip::{Edges, lights_room, lights_width};
        let spans = spans_for_test();
        let pad = CGFloat::from(PLATE_PAD);
        for scale in [1.0, 2.0] {
            for width in [960.0, 786.0, 800.25, 200.0] {
                let edges = Edges {
                    width,
                    left: 129.366,
                    right: 9.0,
                };
                let window = NSSize::new(width, 600.0);
                let strip = placed(window, 28.0, edges.right, edges.left, scale, &spans);
                let h = lights_width(edges, spans.width, pad, scale);
                let (room, left) = lights_room(edges, NSSize::new(h, 28.0));
                let inside = placed(room, 28.0, edges.right, left, scale, &spans);
                let shift = width - h;
                let moved = |r: NSRect| {
                    NSRect::new(NSPoint::new(r.origin.x + shift, r.origin.y + 572.0), r.size)
                };
                for led in Led::ALL {
                    assert_eq!(
                        inside.get(led).map(|(r, t)| (moved(r), t)),
                        *strip.get(led),
                        "{led:?} at {width} points, scale {scale}"
                    );
                }
                assert_eq!(
                    plate_frame(&inside, 28.0, scale).map(|(l, r)| (l, moved(r))),
                    plate_frame(&strip, 28.0, scale),
                    "the plate at {width} points, scale {scale}"
                );
                if let Some((_, r)) = plate_frame(&inside, 28.0, scale) {
                    assert!(r.origin.x >= 0.0, "the plate is whole at {width}: {r:?}");
                }
            }
        }
        let narrow = Edges {
            width: 200.0,
            left: 129.366,
            right: 9.0,
        };
        let h = lights_width(narrow, spans.width, pad, 2.0);
        let (room, left) = lights_room(narrow, NSSize::new(h, 28.0));
        let inside = placed(room, 28.0, 9.0, left, 2.0, &spans);
        assert!(
            inside.hs.is_none() && inside.syn.is_some(),
            "the leftmost go"
        );
    }

    /// **In a narrow window the lights yield and the lockup stays whole**:
    /// the lockup beside AppKit's traffic lights (14x16 at x 9, 29 and
    /// 49, `mark.rs`), the lights a light's gap past its end, and as the
    /// window narrows to its smallest, 200 points, the leftmost lights go
    /// while the lockup keeps its frame inside the window. At 800 every
    /// light is placed; at 310 the strip starts at 126, past the lockup's
    /// end, about 120.4, and short of it plus the gap, about 129.4, so the
    /// first light goes; at 200 the three rightmost stay. Perturbation: give
    /// `placed` the lockup's end rather than `lights_left` and the
    /// 310-point window keeps a light in the gap.
    #[test]
    fn a_narrow_window_drops_lights_and_keeps_the_lockup_whole() {
        use crate::mark::{Lockup, Place};
        let r = |x| NSRect::new(NSPoint::new(x, 6.0), NSSize::new(14.0, 16.0));
        let art = Lockup::OnDark.art().expect("the lockup's size");
        let place = Place::beside(r(9.0), r(29.0), r(49.0), 28.0, art).expect("laid out");
        let left = place.lights_left();
        assert!((left - place.end() - 9.0).abs() < 1e-9, "{left}");
        let spans = spans_for_test();
        let mut kept = Vec::new();
        for width in [800.0, 310.0, 200.0] {
            let lights = placed(NSSize::new(width, 500.0), 28.0, 9.0, left, 2.0, &spans);
            for led in Led::ALL {
                if let Some((f, _)) = lights.get(led) {
                    assert!(
                        f.origin.x >= left,
                        "{width}: {led:?} at {f:?} reaches the lockup"
                    );
                }
            }
            kept.push(
                Led::ALL
                    .iter()
                    .filter(|&&l| lights.get(l).is_some())
                    .count(),
            );
            let frame = place.frame(500.0, 28.0);
            assert!(
                frame.origin.x >= 0.0 && place.end() <= width,
                "{width}: {frame:?}"
            );
        }
        assert_eq!(kept, [9, 8, 3], "the leftmost go first");
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
    /// JetBrains Mono Bold in the theme's look, each light's its width and
    /// the strip's height in points at the scale asked, its lit and dark
    /// pictures different, and for OH and CD, which can be unread, the
    /// unread one a third; the plate's for each light it can start at
    /// [`PLATE_TITLE`] tall and wide from that light to past SYN, by the
    /// pad at each end.
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
            for glow in [Glow::Full, Glow::Dimmed] {
                let look = r.lights_look(glow);
                let wanted = Drawn { scale: 2.0, look };
                let p = pictures(&r, &look, wanted, 28.0).expect("every picture");
                let s = spans(&r, 2.0).expect("the layout");
                let plate = plates(&look, &s, 2.0, 28.0).expect("every plate");
                assert!((s.width - CGFloat::from(r.activity_lights_width())).abs() <= 0.5);
                let (syn, syn_w) = *s.at.get(Led::Syn);
                for led in Led::ALL {
                    let (start, width) = *s.at.get(led);
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
                    let want = syn + syn_w - start + 2.0 * CGFloat::from(PLATE_PAD);
                    assert_eq!(size.height, CGFloat::from(PLATE_TITLE), "{name} {led:?}");
                    assert!((size.width - want).abs() <= 0.5, "{name} {led:?}: {size:?}");
                }
            }
        }
    }

    /// **The plate sits under the lights placed, by its pad past each end,
    /// centred on the strip**: in a wide window from HS's left less the pad
    /// to SYN's right and the pad, [`PLATE_TITLE`] tall and centred in the
    /// 28-point strip, starting at HS; in a narrow one from the first light
    /// kept; and none with none kept. Perturbation: start the plate at HS
    /// whatever is kept, and the narrow window's plate reaches the lockup.
    #[test]
    fn the_plate_sits_under_the_lights_placed() {
        let spans = spans_for_test();
        let wide = placed(NSSize::new(800.0, 500.0), 28.0, 9.0, 100.0, 2.0, &spans);
        let (led, rect) = plate_frame(&wide, 28.0, 2.0).expect("a plate");
        let pad = CGFloat::from(PLATE_PAD);
        let (hs, syn) = (wide.hs.unwrap().0, wide.syn.unwrap().0);
        assert_eq!(led, Led::Hs);
        assert_eq!(rect.origin.x, hs.origin.x - pad);
        assert_eq!(
            rect.origin.x + rect.size.width,
            syn.origin.x + syn.size.width + pad
        );
        assert_eq!((rect.origin.y, rect.size.height), (476.0, 20.0));
        let narrow = placed(NSSize::new(250.0, 500.0), 28.0, 9.0, 100.0, 2.0, &spans);
        let (led, rect) = plate_frame(&narrow, 28.0, 2.0).expect("a plate");
        assert_eq!(led, Led::Cd, "from the first light kept");
        assert_eq!(rect.origin.x, narrow.cd.unwrap().0.origin.x - pad);
        assert!(rect.origin.x >= 100.0 - pad, "clear of the lockup");
        let none = placed(NSSize::new(50.0, 500.0), 28.0, 9.0, 100.0, 2.0, &spans);
        assert_eq!(plate_frame(&none, 28.0, 2.0), None);
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
    /// OH and CD only through `Front::tick`, and SYN and FAX from the
    /// core's state at the tick, the frame baud holds or shows; a drain that brought bytes gives RD its latest read's
    /// stamp and HS each full read's; a reply the core queued during a feed,
    /// or one nobody typed at a flush, lights AA; a write the kernel took
    /// any of stamps SD, and one it refused any of, or bytes still owed at
    /// the tick, darken CS; the process
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
        for want in [
            "crate::front::Reads::of(setting, line, self.app.interval)",
            "self.front.tick(reads, &mut self.procs, shell, now)",
            "crate::line_lamps::read(&self.app.term, &self.app.outbox, cd, oh)",
            "crate::line_lamps::give(&mut self.app.term, given)",
            "crate::lights::feed(&mut self.app.activity, Led::Oh, oh, now);",
            "crate::lights::feed(&mut self.app.activity, Led::Cd, cd, now);",
            "let synced = self.synced.seen(&self.app.term);",
            "self.app.term.images().any_placed()",
            "if !self.app.outbox.is_empty() {\n                self.app.activity.seen(Led::Cs, now);",
            "crate::access::lights_pace(self.app.motion)",
            "let glow = crate::lights::glow(self.app.flashing);",
            "look: self.app.renderer.lights_look(glow),",
            "self.lights.show(",
        ] {
            assert!(show.contains(want), "{want}\n{show}");
        }
        let pump = body(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"));
        for want in [
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
