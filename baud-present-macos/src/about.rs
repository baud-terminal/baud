//! **The About window**: the logo, the build's version, where baud lives and
//! what it may be used for, behind the app menu's About item.
//!
//! **Native first, and why this is not the standard panel.** AppKit's
//! `orderFrontStandardAboutPanelWithOptions:` takes an icon as an `NSImage`,
//! a name, a marketing version and a build version it writes as "Version 1.0
//! (58.4)", and credits (`[[source: sdk27-nsapplication-h]]`,
//! `NSAboutPanelOption*`). Its icon is one image with nothing to say how
//! often it plays, it has no key for a copyright line, and it opens as it is
//! built, so no test could look at it without showing a window. Its version
//! line is kept word for word; the window is the window crate's and its
//! contents are AppKit's own image view and labels.
//!
//! **The logo is the README's film, for [`PLAYS`], when the bundle carries
//! it**: a frame at a time on the view's display link, `NSImageCurrentFrame`
//! of its one bitmap, for the time `NSImageCurrentFrameDuration` says
//! (`[[source: sdk27-nsbitmapimagerep-h]]`). `NSImageView`'s `animates`
//! (`[[source: sdk27-nsimageview-h]]`) says nothing of how often it plays,
//! and that cannot be read off a window never shown. The file repeats and
//! never rests, and a window with no way to pause it stops by itself inside
//! WCAG 2.2.2's five seconds (`[[source: wcag22]]`), so it cuts to the
//! lockup still, placed where the film draws the lockup, and rests there.
//! Under Reduce Motion the still is all it shows, and a binary with no bundle
//! shows it too, as it draws its own Dock icon.

#![allow(unsafe_code)]

use baud_domain::FrameNo;
use baud_render::Argb;
use core::cell::{Cell, RefCell};
use core::time::Duration;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::runtime::AnyObject;
use macos_liquid_glass::objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use macos_liquid_glass::objc2_app_kit::{
    NSAppearanceCustomization, NSApplication, NSBitmapImageRep, NSColor, NSFont,
    NSFontAttributeName, NSImage, NSImageCacheMode, NSImageCurrentFrame,
    NSImageCurrentFrameDuration, NSImageFrameCount, NSImageScaling, NSImageView,
    NSLinkAttributeName, NSMenuItem, NSMutableParagraphStyle, NSParagraphStyleAttributeName,
    NSTextAlignment, NSTextField, NSView, NSWindowStyleMask,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{
    NSMutableAttributedString, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect,
    NSRunLoop, NSRunLoopCommonModes, NSSize, NSString, ns_string,
};
use macos_liquid_glass::objc2_quartz_core::CADisplayLink;
use macos_liquid_glass::window::GlassWindow;

/// **The logo's box, in points**: the README's film at the pixels it was made
/// at, 840 by 580, at 2x.
pub const LOGO: NSSize = NSSize::new(420.0, 290.0);
/// The space under the logo for the three lines, in points.
const LINES: CGFloat = 76.0;
/// The bottom margin, and the gap between two lines, in points.
const MARGIN: CGFloat = 18.0;
const LINE_GAP: CGFloat = 3.0;
/// The version line's size, AppKit's default control text; the other two take
/// `NSFont.smallSystemFontSize`.
const VERSION_SIZE: CGFloat = 13.0;

/// **How long the film plays before it cuts to the still**: nineteen of the
/// film's blink phases at 250 ms, the most that end inside WCAG 2.2.2's five
/// seconds with the block shown, as the still shows it, so the cut moves
/// only the plasma. `baud-app/tests/about_window.rs` holds this to the file.
pub const PLAYS: Duration = Duration::from_millis(4750);

/// **The frame shown `elapsed` in**, of frames lasting `lasts`, and whether
/// the film is over: at `plays`, or when its frames run out if that is sooner.
pub fn frame_at(lasts: &[Duration], plays: Duration, elapsed: Duration) -> (FrameNo, Playing) {
    let mut end = Duration::ZERO;
    let mut frame = FrameNo::of(0);
    for (i, d) in lasts.iter().enumerate() {
        end += *d;
        if elapsed < end.min(plays) {
            return (frame, Playing::Yes);
        }
        if end >= plays || i + 1 == lasts.len() {
            break;
        }
        frame = FrameNo::of(frame.raw() + 1);
    }
    (frame, Playing::Over)
}

/// Whether the film has more frames to show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Playing {
    Yes,
    Over,
}

/// Whether the film's clock is running.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clock {
    Running,
    Stopped,
}

/// Whether the logo moves: the system's Reduce Motion, read when it opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    Plays,
    Still,
}

/// One appearance's logo: its film, if the bundle carries one, and the
/// still shown without one, under Reduce Motion, and once the film has cut.
pub struct Logo {
    pub film: Option<Retained<NSImage>>,
    pub still: Retained<NSImage>,
}

/// What the window says.
pub struct Content<'a> {
    pub on_dark: Logo,
    pub on_light: Logo,
    /// Where the still goes in the logo's box, in points from its lower
    /// left: where the film draws the lockup, so the cut moves nothing.
    pub still_at: NSRect,
    /// The page round the film, for the window behind it.
    pub page_dark: Argb,
    pub page_light: Argb,
    /// "Version 1.0.0 (abc1234)".
    pub version: &'a str,
    /// The address as shown, and where it goes.
    pub link: &'a str,
    pub url: &'a str,
    pub copyright: &'a str,
}

/// A logo made ready: its still, and its film's frames if it has one.
struct Picture {
    still: Retained<NSImage>,
    film: Option<Film>,
    page: Retained<NSColor>,
}

/// Which of a logo's two the window shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Shows {
    Film,
    #[default]
    Still,
}

struct Film {
    image: Retained<NSImage>,
    rep: Retained<NSBitmapImageRep>,
    lasts: Vec<Duration>,
    current: Cell<FrameNo>,
}

impl Film {
    /// **Its one bitmap and what its frames last**, as AppKit reads them, or
    /// `None` when AppKit reads no frames from it.
    fn of(image: &Retained<NSImage>) -> Option<Film> {
        let reps = image.representations();
        let rep = reps.firstObject()?.downcast::<NSBitmapImageRep>().ok()?;
        // SAFETY: a key the header declares for a GIF's bitmap, read-only.
        let n = unsafe { rep.valueForProperty(NSImageFrameCount) }?
            .downcast::<NSNumber>()
            .ok()?
            .as_u32();
        if n == 0 {
            return None;
        }
        let film = Film {
            image: image.clone(),
            rep,
            lasts: Vec::new(),
            current: Cell::new(FrameNo::of(0)),
        };
        let mut lasts = Vec::with_capacity(n.try_into().ok()?);
        for frame in 0..n {
            film.show(FrameNo::of(frame));
            // SAFETY: as above, for the frame just made current.
            let d = unsafe { film.rep.valueForProperty(NSImageCurrentFrameDuration) }?
                .downcast::<NSNumber>()
                .ok()?;
            lasts.push(Duration::try_from_secs_f64(d.as_f64()).ok()?);
        }
        Some(Film { lasts, ..film })
    }

    fn show(&self, frame: FrameNo) {
        let n = NSNumber::new_u32(frame.raw());
        // SAFETY: the header's key for a GIF's bitmap, its value an NSNumber.
        unsafe {
            self.rep
                .setProperty_withValue(NSImageCurrentFrame, Some(&n));
        }
        self.current.set(frame);
    }

    fn last(&self) -> FrameNo {
        let n = self.lasts.len().saturating_sub(1);
        FrameNo::of(n.try_into().unwrap_or(0))
    }
}

impl Picture {
    fn of(logo: &Logo, ink: Argb) -> Picture {
        let unit = |c: baud_domain::Chan| CGFloat::from(c.get()) / 255.0;
        let page = NSColor::colorWithSRGBRed_green_blue_alpha(
            unit(ink.r()),
            unit(ink.g()),
            unit(ink.b()),
            1.0,
        );
        let film = logo.film.as_ref().and_then(|image| {
            let film = Film::of(image)?;
            // Never a cached copy of one frame.
            image.setCacheMode(NSImageCacheMode::Never);
            film.show(FrameNo::of(0));
            Some(film)
        });
        Picture {
            still: logo.still.clone(),
            film,
            page,
        }
    }
}

/// Which logo an appearance takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Look {
    Dark,
    Light,
}

impl Look {
    fn other(self) -> Look {
        match self {
            Look::Dark => Look::Light,
            Look::Light => Look::Dark,
        }
    }
}

struct Shown {
    on_dark: Picture,
    on_light: Picture,
    logo: Retained<NSImageView>,
    /// The logo view's frame round the film, and round the still.
    film_at: NSRect,
    still_at: NSRect,
}

impl Shown {
    fn picture(&self, look: Look) -> &Picture {
        match look {
            Look::Dark => &self.on_dark,
            Look::Light => &self.on_light,
        }
    }
}

#[derive(Default)]
struct Ivars {
    shown: RefCell<Option<Shown>>,
    link: RefCell<Option<Retained<CADisplayLink>>>,
    started: Cell<Option<f64>>,
    shows: Cell<Shows>,
}

define_class!(
    /// The window's content: the logo for its appearance on the page's
    /// colour, and the film's clock.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct AboutView;

    impl AboutView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if let Some(s) = &*self.ivars().shown.borrow() {
                s.picture(self.look()).page.setFill();
                macos_liquid_glass::objc2_app_kit::NSRectFill(self.bounds());
            }
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn appearance_changed(&self) {
            self.follow();
        }

        #[unsafe(method(tick:))]
        fn tick(&self, link: &CADisplayLink) {
            self.advance(link);
        }
    }
);

impl AboutView {
    fn look(&self) -> Look {
        match macos_liquid_glass::is_dark(&self.effectiveAppearance()) {
            true => Look::Dark,
            false => Look::Light,
        }
    }

    /// The logo for the appearance now, on the same frame of the film as
    /// the appearance it left.
    fn follow(&self) {
        if let Some(s) = &*self.ivars().shown.borrow() {
            let look = self.look();
            let (p, other) = (s.picture(look), s.picture(look.other()));
            if let (Some(a), Some(b)) = (&p.film, &other.film) {
                a.show(b.current.get().min(a.last()));
            }
        }
        self.put();
    }

    /// The logo view on the film's current frame, or on the still.
    fn put(&self) {
        if let Some(s) = &*self.ivars().shown.borrow() {
            let p = s.picture(self.look());
            match (self.ivars().shows.get(), &p.film) {
                (Shows::Film, Some(a)) => {
                    s.logo.setFrame(s.film_at);
                    s.logo.setImage(Some(&a.image));
                }
                _ => {
                    s.logo.setFrame(s.still_at);
                    s.logo.setImage(Some(&p.still));
                }
            }
            if let Some(window) = self.window() {
                window.setBackgroundColor(Some(&p.page));
            }
            NSView::setNeedsDisplay(&s.logo, true);
            NSView::setNeedsDisplay(self, true);
        }
    }

    /// One tick: the frame for the time since the first, and at the film's
    /// end or once the window is gone, the still with the clock stopped.
    fn advance(&self, link: &CADisplayLink) {
        let now = link.timestamp();
        let started = self.ivars().started.get().unwrap_or(now);
        self.ivars().started.set(Some(started));
        let shut = !self.window().is_some_and(|w| w.isVisible());
        let over = match &*self.ivars().shown.borrow() {
            Some(s) => match &s.picture(self.look()).film {
                Some(film) if !shut => {
                    let since =
                        Duration::try_from_secs_f64(now - started).unwrap_or(Duration::ZERO);
                    let (frame, playing) = frame_at(&film.lasts, PLAYS, since);
                    film.show(frame);
                    NSView::setNeedsDisplay(&s.logo, true);
                    playing == Playing::Over
                }
                _ => true,
            },
            None => true,
        };
        if over {
            self.rest();
        }
    }

    /// The still, with the clock stopped.
    fn rest(&self) {
        self.stop();
        self.ivars().shows.set(Shows::Still);
        self.put();
    }

    fn stop(&self) {
        if let Some(link) = self.ivars().link.borrow_mut().take() {
            link.invalidate();
        }
    }

    /// The film from its start, or the still, by `motion`.
    fn start(&self, motion: Motion) {
        self.stop();
        self.ivars().started.set(None);
        let plays = motion == Motion::Plays
            && self.ivars().shown.borrow().as_ref().is_some_and(|s| {
                for film in [&s.on_dark.film, &s.on_light.film].into_iter().flatten() {
                    film.show(FrameNo::of(0));
                }
                s.picture(self.look()).film.is_some()
            });
        self.ivars().shows.set(match plays {
            true => Shows::Film,
            false => Shows::Still,
        });
        self.put();
        if plays {
            // SAFETY: `tick:` is this class's and takes the link; the view
            // holds the link and invalidates it, which releases its target.
            let link = unsafe { self.displayLinkWithTarget_selector(self, sel!(tick:)) };
            // SAFETY: the main run loop, and the mode its own header names.
            unsafe {
                link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes);
            }
            *self.ivars().link.borrow_mut() = Some(link);
        }
    }
}

/// Which of AppKit's label colours a line takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ink {
    Label,
    Secondary,
}

/// A centred line of `text`.
fn line(mtm: MainThreadMarker, text: &str, size: CGFloat, ink: Ink) -> Retained<NSTextField> {
    let f = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    f.setFont(Some(&NSFont::systemFontOfSize(size)));
    f.setAlignment(NSTextAlignment::Center);
    let colour = match ink {
        Ink::Label => NSColor::labelColor(),
        Ink::Secondary => NSColor::secondaryLabelColor(),
    };
    f.setTextColor(Some(&colour));
    f
}

/// The link line: `text` going to `url`, as AppKit's own label takes one.
fn link_line(mtm: MainThreadMarker, text: &str, url: &str, size: CGFloat) -> Retained<NSTextField> {
    let s = NSMutableAttributedString::initWithString(
        NSMutableAttributedString::alloc(),
        &NSString::from_str(text),
    );
    let all = NSRange::new(0, s.length());
    let centred = NSMutableParagraphStyle::new();
    centred.setAlignment(NSTextAlignment::Center);
    let font = NSFont::systemFontOfSize(size);
    let to = NSString::from_str(url);
    // SAFETY: the keys AppKit declares, each with the class its header gives.
    unsafe {
        s.addAttribute_value_range(NSLinkAttributeName, &to, all);
        s.addAttribute_value_range(NSFontAttributeName, &font, all);
        s.addAttribute_value_range(NSParagraphStyleAttributeName, &centred, all);
    }
    let f = NSTextField::labelWithAttributedString(&s, mtm);
    // What makes a label's link clickable.
    f.setSelectable(true);
    f.setAllowsEditingTextAttributes(true);
    f
}

/// **The About window, built and not shown.**
pub struct About {
    window: GlassWindow,
    view: Retained<AboutView>,
    version: Retained<NSTextField>,
    link: Retained<NSTextField>,
    copyright: Retained<NSTextField>,
    centred: Cell<bool>,
}

impl About {
    /// Built, and not shown.
    pub fn new(mtm: MainThreadMarker, content: &Content<'_>) -> About {
        let size = NSSize::new(LOGO.width, LOGO.height + LINES);
        let on_dark = Picture::of(&content.on_dark, content.page_dark);
        let on_light = Picture::of(&content.on_light, content.page_light);
        let window = GlassWindow::new(mtm, size, "About baud");
        let w = window.ns_window();
        // A close button and nothing to resize: the film has one size.
        w.setStyleMask(
            w.styleMask()
                .difference(NSWindowStyleMask::Resizable | NSWindowStyleMask::Miniaturizable),
        );
        w.setOpaque(true);
        window.set_close_on_escape(true);

        let this = AboutView::alloc(mtm).set_ivars(Ivars::default());
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
        // SAFETY: `NSView`'s designated initialiser.
        let view: Retained<AboutView> = unsafe { msg_send![super(this), initWithFrame: frame] };
        let logo = NSImageView::new(mtm);
        logo.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        logo.setAnimates(false);
        let film_at = NSRect::new(NSPoint::new(0.0, LINES), LOGO);
        let still_at = NSRect::new(
            NSPoint::new(content.still_at.origin.x, content.still_at.origin.y + LINES),
            content.still_at.size,
        );
        logo.setFrame(still_at);
        view.addSubview(&logo);

        let small = NSFont::smallSystemFontSize();
        let version = line(mtm, content.version, VERSION_SIZE, Ink::Label);
        version.setSelectable(true);
        let link = link_line(mtm, content.link, content.url, small);
        let copyright = line(mtm, content.copyright, small, Ink::Secondary);
        let mut y = MARGIN;
        for field in [&copyright, &link, &version] {
            let h = field.fittingSize().height;
            field.setFrame(NSRect::new(
                NSPoint::new(0.0, y),
                NSSize::new(size.width, h),
            ));
            view.addSubview(field);
            y += h + LINE_GAP;
        }
        *view.ivars().shown.borrow_mut() = Some(Shown {
            on_dark,
            on_light,
            logo,
            film_at,
            still_at,
        });
        window.set_content_view(&view);
        view.follow();
        About {
            window,
            view,
            version,
            link,
            copyright,
            centred: Cell::new(false),
        }
    }

    /// **Shown, and the logo from its start or at rest**; centred the first
    /// time, and where it was left after.
    pub fn open(&self, motion: Motion) {
        if !self.centred.replace(true) {
            self.window.center();
        }
        self.window.show();
        self.view.start(motion);
    }

    /// The window. A test renders and looks at it without showing it.
    pub fn window(&self) -> &GlassWindow {
        &self.window
    }

    /// The logo's view, in which a test finds what is drawn.
    pub fn logo_view(&self) -> Retained<NSImageView> {
        self.view
            .ivars()
            .shown
            .borrow()
            .as_ref()
            .expect("set in new")
            .logo
            .clone()
    }

    /// The three lines' text, as AppKit has them.
    pub fn lines(&self) -> [String; 3] {
        [&self.version, &self.link, &self.copyright].map(|f| f.stringValue().to_string())
    }

    /// Whether the logo shown has a film.
    pub fn has_film(&self) -> bool {
        let shown = self.view.ivars().shown.borrow();
        shown
            .as_ref()
            .is_some_and(|s| s.picture(self.view.look()).film.is_some())
    }

    /// The film at `frame`, as a tick puts it there.
    pub fn show_frame(&self, frame: FrameNo) {
        self.view.ivars().shows.set(Shows::Film);
        if let Some(s) = &*self.view.ivars().shown.borrow() {
            if let Some(film) = &s.picture(self.view.look()).film {
                film.show(frame);
            }
        }
        self.view.put();
    }

    /// The film from its start or the still, with no window shown: the part
    /// of [`About::open`] a test can reach.
    pub fn play(&self, motion: Motion) {
        self.view.start(motion);
    }

    /// **The cut**, as a tick makes it at the film's end: the still, with the
    /// clock stopped. A link made for a window never on screen does not tick,
    /// so this is the part a test can reach.
    pub fn cut(&self) {
        self.view.rest();
    }

    /// Which of the two the window shows.
    pub fn shows(&self) -> Shows {
        self.view.ivars().shows.get()
    }

    /// The film's current frame while it shows, and whether its clock is
    /// running.
    pub fn film_state(&self) -> (Option<FrameNo>, Clock) {
        let shown = self.view.ivars().shown.borrow();
        let frame = match self.shows() {
            Shows::Film => shown.as_ref().and_then(|s| {
                let film = s.picture(self.view.look()).film.as_ref()?;
                Some(film.current.get())
            }),
            Shows::Still => None,
        };
        let clock = match self.view.ivars().link.borrow().is_some() {
            true => Clock::Running,
            false => Clock::Stopped,
        };
        (frame, clock)
    }

    /// What each frame of the film shown lasts, as AppKit read it.
    pub fn film_lasts(&self) -> Vec<Duration> {
        let shown = self.view.ivars().shown.borrow();
        shown
            .as_ref()
            .and_then(|s| s.picture(self.view.look()).film.as_ref())
            .map_or(Vec::new(), |f| f.lasts.clone())
    }

    /// The link line's destination.
    pub fn link_url(&self) -> Option<String> {
        let s = self.link.attributedStringValue();
        let mut range = NSRange::new(0, 0);
        // SAFETY: the key AppKit declares, at an index the string has.
        let v = unsafe { s.attribute_atIndex_effectiveRange(NSLinkAttributeName, 0, &mut range) }?;
        Some(v.downcast::<NSString>().ok()?.to_string())
    }
}

define_class!(
    /// What the About item's action reaches: the item's target, since the
    /// action is not one AppKit's responder chain answers.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Box<dyn Fn()>]
    pub struct AboutTarget;

    impl AboutTarget {
        #[unsafe(method(openAbout:))]
        fn open_about(&self, _sender: Option<&AnyObject>) {
            (self.ivars())();
        }
    }

    unsafe impl NSObjectProtocol for AboutTarget {}
);

/// **The app menu's About item**, first in the menu as the HIG has it, with
/// a separator under it: the window crate's menu has none to give it, and
/// its item's action needs an `unsafe` a consumer that forbids it cannot
/// write. Held by the caller for the process, as `NSMenuItem.target` is
/// weak. `None` with no app menu.
pub fn add_menu_item(
    app: &NSApplication,
    mtm: MainThreadMarker,
    title: &str,
    open: impl Fn() + 'static,
) -> Option<Retained<AboutTarget>> {
    let menu = app.mainMenu()?.itemAtIndex(0)?.submenu()?;
    let target = AboutTarget::alloc(mtm).set_ivars(Box::new(open) as Box<dyn Fn()>);
    // SAFETY: `NSObject`'s `init`.
    let target: Retained<AboutTarget> = unsafe { msg_send![super(target), init] };
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(title));
    // SAFETY: `openAbout:` is the target's, and takes the sender.
    unsafe {
        item.setAction(Some(sel!(openAbout:)));
        item.setTarget(Some(&target));
    }
    item.setKeyEquivalent(ns_string!(""));
    menu.insertItem_atIndex(&item, 0);
    menu.insertItem_atIndex(&NSMenuItem::separatorItem(mtm), 1);
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The frame at each moment, and the cut**: each frame for its own
    /// delay, over at `plays` however long the file runs, over when its
    /// frames run out if that is sooner, and an empty film over at once.
    /// Perturbation: ignore `plays` and the cases at 50 ms fail.
    #[test]
    fn a_frame_lasts_its_delay_and_the_film_ends_at_the_cut() {
        let ms = Duration::from_millis;
        let d = [ms(20), ms(30), ms(1000)];
        let f = FrameNo::of;
        let at = |plays, elapsed| frame_at(&d, ms(plays), ms(elapsed));
        assert_eq!(at(50, 0), (f(0), Playing::Yes));
        assert_eq!(at(50, 19), (f(0), Playing::Yes));
        assert_eq!(at(50, 20), (f(1), Playing::Yes));
        assert_eq!(at(50, 49), (f(1), Playing::Yes));
        assert_eq!(at(50, 50), (f(1), Playing::Over));
        assert_eq!(at(50, 9000), (f(1), Playing::Over));
        assert_eq!(at(900, 899), (f(2), Playing::Yes));
        assert_eq!(at(900, 900), (f(2), Playing::Over));
        assert_eq!(at(5000, 1049), (f(2), Playing::Yes));
        assert_eq!(at(5000, 1050), (f(2), Playing::Over));
        assert_eq!(frame_at(&[], ms(50), ms(0)), (f(0), Playing::Over));
    }
}
