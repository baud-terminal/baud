//! **The activity lights in the macOS title strip**: nine lights at the
//! strip's right, each named with a modem's letters -- HS AA CD OH RD SD
//! CS FAX SYN, in the order a 1988 U.S. Robotics Courier manual's table
//! lists them (`baud_render::lights`) -- and each lit only for something
//! baud measured.
//!
//! **Where**: one image view a light in baud's own container, as the app
//! icon is, re-placed on every resize -- the title strip is AppKit's, drawn
//! over the glass, and the `macos-liquid-glass` window crate has no
//! titlebar-accessory API (note AH, 5.1), so baud draws where it already
//! draws. A view a light gives each its own hover text, AppKit's
//! `toolTip`, shown only while the pointer is over that light, and its
//! frame is the tooltip's rectangle wherever the view is placed. A light
//! that would reach baud's mark in a narrow window is not drawn, the
//! leftmost first, so nothing covers the traffic lights or the mark.
//!
//! **The pixels are baud's renderer's** (`Renderer::activity_light`): each
//! light's three pictures, dark, lit and unread, drawn when the scale, the
//! appearance or Dim Flashing Lights changes; a light that changes swaps
//! its view's picture and builds no frame of the grid. The clocks step on
//! the display link's tick, which already runs, from the stamps the pump
//! and the writes already take, and the process reads of OH and CD
//! (`front.rs`) run on that tick while the lights are on: nothing is added
//! to a byte's path, and nothing is timed off the link.
//!
//! **Or on a line of their own in the frame** (`[window] activity_lights
//! = "bottom_line"` or `"top_line"`): the renderer draws the same pictures
//! there, from the theme's colours, on both systems (`baud_render::lights`,
//! `Renderer::set_lights_line`), and this module puts each light's hover
//! text over its place on that line ([`LineTips`]). The Linux window has no
//! title bar -- no compositor decorations are asked for and none are drawn
//! -- so the line is its default, and its lights are drawn there by the
//! Linux driver.

use baud_render::{
    ActivityLights, Argb, Backdrop, ByLed, ByLight, GROUND_DARK, GROUND_LIGHT, Glow, Led, Light,
    LightsImage, LightsLook, Renderer, Shown,
};
use macos_liquid_glass::drawable::Appearance;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSImage, NSImageScaling, NSImageView, NSView, NSWindow,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

/// What the pictures were drawn for: a change of any of them draws them
/// again.
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
    /// A light changed: its view takes another of the pictures it has.
    Swap,
    /// The pictures are for another scale, appearance or glow, or there are
    /// none: draw them, then show the state.
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

/// How a lit light is drawn under the Dim Flashing Lights setting.
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
/// -- baud's mark -- is not placed, so in a narrow window the leftmost go
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

/// The views, their pictures, and what each shows.
pub(crate) struct Strip {
    views: ByLed<Option<Retained<NSImageView>>>,
    pictures: Option<(Drawn, ByLed<ByLight<Retained<NSImage>>>)>,
    on_view: ByLed<Option<Light>>,
    /// Each light's place, from the renderer's layout.
    spans: Spans,
    /// The scale the spans are whole device pixels at.
    scale: CGFloat,
    /// The gap kept at the strip's right, the traffic lights' at its left,
    /// and the least x a light may start at: past baud's mark.
    right: CGFloat,
    left: CGFloat,
    /// The looks drawn so far this session, by what they were made from: a
    /// dimmed look scans each of its three colours over the backdrop, so a
    /// flip back to an appearance, or a move to a display of another
    /// scale, does not make it again.
    looks: Vec<(LookKey, LightsLook)>,
}

/// What a look is made from: the lit and letter colours, the strip's grey,
/// the glow and the backdrop.
type LookKey = (ByLed<Argb>, Argb, Argb, Glow, Backdrop);

impl Strip {
    pub(crate) fn new(right: CGFloat, left: CGFloat) -> Strip {
        Strip {
            views: ByLed::default(),
            pictures: None,
            on_view: ByLed::default(),
            spans: Spans {
                at: ByLed::default(),
                width: 0.0,
            },
            scale: 1.0,
            right,
            left,
            looks: Vec::new(),
        }
    }

    /// The appearance or the face changed under the pictures: the next
    /// tick draws them again.
    pub(crate) fn stale(&mut self) {
        self.pictures = None;
        self.on_view = ByLed::default();
    }

    /// **The lights at `shown`**, under `setting`, in `container`'s strip
    /// of `inset` points: the views put up, their pictures drawn and
    /// swapped, or taken down -- off, or on their own line in the frame.
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
        if setting != ActivityLights::TitleBar {
            for led in Led::ALL {
                if let Some(v) = self.views.get_mut(led).take() {
                    v.removeFromSuperview();
                }
            }
            self.on_view = ByLed::default();
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
                let Some(spans) = spans(renderer, wanted.scale) else {
                    return;
                };
                self.spans = spans;
                self.scale = wanted.scale;
                self.pictures = Some((wanted, drawn));
                self.on_view = ByLed::default();
            }
            Step::Swap => {}
        }
        if self.views.iter().any(|(_, v)| v.is_none()) {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            for led in Led::ALL {
                if self.views.get(led).is_some() {
                    continue;
                }
                let v = NSImageView::new(mtm);
                v.setImageScaling(NSImageScaling::ScaleNone);
                v.setToolTip(Some(&NSString::from_str(led.tip())));
                container.addSubview(&v);
                *self.views.get_mut(led) = Some(v);
            }
        }
        self.place(container, inset);
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

    /// Re-place every view at the strip's right, and hide the ones a
    /// narrow window has no room for: on every resize.
    pub(crate) fn place(&self, container: &NSView, inset: CGFloat) {
        let frames = placed(
            container.frame().size,
            inset,
            self.right,
            self.left,
            self.scale,
            &self.spans,
        );
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

/// **Each light's hover rectangle on the lights' own line**, in the
/// terminal view's points, top-left origin as the view is flipped: the
/// whole frame at `origin` device pixels in the view, the line `top`
/// pixels down it and `height` tall, each placed light at its left and
/// width on the line (`Renderer::lights_places`); a light the line left
/// out has none.
pub(crate) fn line_tips(
    origin: (baud_font::Px, baud_font::Px),
    top: baud_font::Px,
    height: baud_font::Px,
    places: ByLed<Option<(baud_font::Px, baud_font::Px)>>,
    scale: CGFloat,
) -> ByLed<Option<NSRect>> {
    let scale = scale.max(1.0);
    let pt = |px: u32| CGFloat::from(px) / scale;
    let y = pt(origin.1.value().saturating_add(top.value()));
    ByLed::of(|led| {
        places.get(led).map(|(x, w)| {
            NSRect::new(
                NSPoint::new(pt(origin.0.value().saturating_add(x.value())), y),
                NSSize::new(pt(w.value()), pt(height.value())),
            )
        })
    })
}

/// **The hover texts of the lights on their own line**: a clear view a
/// light, over its place in the terminal view, with its `toolTip`, as the
/// title strip's image views carry theirs; it draws nothing and takes no
/// event a superview would, since a view's unhandled mouse event goes on
/// up the responder chain.
pub(crate) struct LineTips {
    views: ByLed<Option<Retained<NSView>>>,
    at: ByLed<Option<NSRect>>,
}

impl LineTips {
    pub(crate) fn new() -> LineTips {
        LineTips {
            views: ByLed::default(),
            at: ByLed::default(),
        }
    }

    /// Each light's view over `rects` in `view`, made the first time, and
    /// hidden where a light has no place -- or every one, with no line.
    pub(crate) fn place(&mut self, view: &NSView, rects: ByLed<Option<NSRect>>) {
        // Nothing to move, or no line and no view made for one.
        let none = |(_, v): (Led, &Option<Retained<NSView>>)| v.is_none();
        if rects == self.at
            && (self.views.iter().all(|(_, v)| v.is_some()) || self.views.iter().all(none))
        {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        for led in Led::ALL {
            let v = self.views.get_mut(led).get_or_insert_with(|| {
                let v = NSView::new(mtm);
                v.setToolTip(Some(&NSString::from_str(led.tip())));
                view.addSubview(&v);
                v
            });
            match rects.get(led) {
                Some(r) => {
                    v.setFrame(*r);
                    v.setHidden(false);
                }
                None => v.setHidden(true),
            }
        }
        self.at = rects;
    }
}

/// Every light's place in points at `scale`, from the renderer's layout in
/// device pixels.
fn spans(renderer: &Renderer<'_, '_>, scale: CGFloat) -> Option<Spans> {
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

    fn at() -> Drawn {
        Drawn {
            scale: 2.0,
            glow: Glow::Full,
            appearance: Appearance::Dark,
            backdrop: Backdrop::under_window_dark(),
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
    /// light swaps, the same state does nothing, a new scale, appearance
    /// or glow redraws, and off does nothing whatever moved.
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
    /// the ones that would reach the mark are not placed, the leftmost
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

    /// **Each light on its own line has its hover rectangle over its place**,
    /// in the flipped view's points: the whole frame 20 by 15 pixels into
    /// the view at 2x, the line 400 pixels down it and 30 tall, RD 100
    /// pixels along it and 40 wide is (60, 207.5) points and 20 by 15; a
    /// light the line left out has none. Perturbation: leave the frame's
    /// origin out -- every rectangle sits up and left of its light -- and
    /// this goes red.
    #[test]
    fn each_light_on_its_line_has_its_hover_rectangle() {
        use baud_font::Px;
        let places = ByLed {
            rd: Some((Px::of(100), Px::of(40))),
            syn: Some((Px::of(500), Px::of(60))),
            ..ByLed::default()
        };
        let r = line_tips(
            (Px::of(20), Px::of(15)),
            Px::of(400),
            Px::of(30),
            places,
            2.0,
        );
        let rect = |x, y, w, h| Some(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)));
        assert_eq!(r.rd, rect(60.0, 207.5, 20.0, 15.0));
        assert_eq!(r.syn, rect(260.0, 207.5, 30.0, 15.0));
        for led in Led::ALL
            .into_iter()
            .filter(|&l| l != Led::Rd && l != Led::Syn)
        {
            assert_eq!(*r.get(led), None, "{led:?}: left out");
        }
    }

    /// **Each light's rectangle shows its own text, and only its own**:
    /// the point at the middle of every placed light falls in that light's
    /// rectangle and in no other's, and the text there names that light's
    /// letters and nothing else's; the texts are nine different ones, each
    /// spelling out its letters, CS's saying it is inverted.
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
        assert!(Led::Cs.tip().contains("inverted"), "{}", Led::Cs.tip());
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

    /// **Each light's three pictures, headless, under either appearance and
    /// glow**: drawn by the renderer in baud's embedded JetBrains Mono Bold
    /// with AppKit's colours, each its light's width and the strip's height
    /// in points at the scale asked, its lit and dark pictures different,
    /// and for OH and CD, which can be unread, the unread one a third.
    #[test]
    fn each_lights_pictures_are_drawn_at_its_size() {
        use baud_domain::{Cols, Rows};
        use baud_render::{Blend, PxSize, Stack, Theme};
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
                let mut strip = Strip::new(9.0, 100.0);
                let look = strip.look(&ns, wanted);
                assert_eq!(strip.look(&ns, wanted), look, "made once");
                assert_eq!(strip.looks.len(), 1, "and kept");
                let p = pictures(&r, &look, wanted, 28.0).expect("every picture");
                let s = spans(&r, 2.0).expect("the layout");
                assert!((s.width - CGFloat::from(r.activity_lights_width())).abs() <= 0.5);
                for led in Led::ALL {
                    let (_, width) = *s.at.get(led);
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
                }
            }
        }
    }

    /// **Dim Flashing Lights draws every lit light faint**: macOS's setting,
    /// read into the same value the text blink's dimmed phases read, maps to
    /// the faint look when on and to the full one when off. Perturbation: map
    /// `Flashing::Dimmed` to `Glow::Full` -- the setting stops dimming the
    /// lights and this goes red.
    #[test]
    fn dim_flashing_lights_draws_the_lights_faint() {
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
    /// any of stamps SD, and one it refused any of stamps CS; the process
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
            "self.front.tick(setting, &mut self.procs, shell, now)",
            "crate::lights::feed(&mut self.app.activity, Led::Oh, oh, now);",
            "crate::lights::feed(&mut self.app.activity, Led::Cd, cd, now);",
            "self.app.term.synchronized_output()",
            "self.app.term.images().any_placed()",
            "crate::access::lights_pace(self.app.motion)",
            "glow: crate::lights::glow(self.app.flashing),",
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
        // puts the hover texts over them, every frame places their records,
        // and a re-lay sets the line at its scale and takes its pixels off
        // the grid's.
        for want in [
            "self.app.renderer.set_lights(shown, glow);",
            "self.place_line_tips();",
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
