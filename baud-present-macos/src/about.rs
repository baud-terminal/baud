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
//! **The logo is the README's film, repeating, when the bundle carries it**:
//! a frame at a time on the view's display link, `NSImageCurrentFrame` of its
//! one bitmap, for the time `NSImageCurrentFrameDuration` says
//! (`[[source: sdk27-nsbitmapimagerep-h]]`), the frame after the last being
//! the first, as the file's own count of zero has it. `NSImageView`'s
//! `animates` (`[[source: sdk27-nsimageview-h]]`) says nothing of how often
//! it plays, and that cannot be read off a window never shown.
//!
//! **It conforms to WCAG 2.2.2 by a pause.** The film lasts more than five
//! seconds beside the window's lines, so it needs a way for the user to
//! pause, stop or hide it (`[[source: wcag22]]`). Closing the window is not
//! claimed as that way: it takes the lines too, and
//! `[[source: wcag22-understanding-pause-stop-hide]]` asks for a mechanism
//! that leaves the rest usable. A click on the film or Space pauses it where
//! it is and plays it on from there, the film's tooltip says so, and
//! VoiceOver has it as the logo's action. The window takes keys from its
//! open, and a pause holds for the app's run, so a window opened again
//! starts from its first frame, paused. WCAG 2.3.1 is the file's census,
//! which counts the repeat and its join (`tools/baud-pictures`'s `logo.rs`).
//!
//! **The tooltip is a rect on the window's own view, over the film**, and
//! not the image view's `toolTip`: a point on the film is that view's by
//! `hitTest:`, and neither `NSView.h` nor Apple's page for `toolTip` says
//! how AppKit picks among the tooltips of views that overlap, so the film's
//! point has one tooltip, on the view a click there reaches, and no other.
//! `addToolTipRect:owner:userData:` holds its owner weakly ("You're
//! responsible for ensuring that owner remains valid"), so the view keeps
//! the string for as long as the rect is there.
//!
//! **It draws only while it can be seen.** The clock pauses while none of the
//! window is on screen, covered or the app hidden, and goes on from the same
//! frame; a close ends it. Under Reduce Motion the still is all it shows, and
//! a binary with no bundle shows it too, as it draws its own Dock icon.
//!
//! **Under the film, the author's picture beside the lines, as a signature
//! sits beside a letter's last lines**: round, [`FACE`] points, with the
//! lines at its right, left-aligned and centred on its middle, the two
//! centred across the window as one block, so the space left of the picture
//! is the space right of the longest line, both whole points. They sit [`GAP`] under where the
//! film moves ([`Content::moves_at`]), not under its whole box: the film's
//! frames leave the bottom of their canvas bare, the picture and the lines
//! stand in that bare band, drawn over the image view, and the pause and
//! its tooltip cover only where the film moves, so nothing clickable lies
//! on the film and no click on the lines pauses it. A click on the picture
//! goes to its address through [`Face::open`], the caller's opener, so this
//! module opens nothing itself; VoiceOver has it as a button with its
//! label, and the pointer is AppKit's pointing hand over it. A binary with
//! no bundle has no picture, and the lines, centred across the window, take
//! only their own height.
//!
//! **The release's age, a line of its own under the version**, in AppKit's
//! own words (`[[source: sdk27-nsrelativedatetimeformatter-h]]`): "3 hours
//! ago", "5 days ago", "2 weeks ago", the formatter choosing the unit. It is
//! worked out each time the window opens, from the commit's time stamped at
//! build, so it never goes stale in a window kept for the app's run. The
//! numeric style throughout, because the named style renames one of a unit
//! ("yesterday", "last week") where the line counts units; except under a
//! second, where the numeric style has only "in 0 seconds" and the named
//! style says "now". A commit time ahead of the clock is clamped to the
//! clock, so it reads "now" and never "in 1 hour". With no time, there is
//! no line, and the others close up round the picture.

#![allow(unsafe_code)]

use crate::closing::{Closing, when_closing, when_occlusion_changes};
use baud_domain::FrameNo;
use baud_render::Argb;
use core::cell::{Cell, RefCell};
use core::time::Duration;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::runtime::{AnyObject, ProtocolObject};
use macos_liquid_glass::objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use macos_liquid_glass::objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAccessibilityCustomAction,
    NSAppearanceCustomization, NSApplication, NSBezierPath, NSBitmapImageRep, NSColor, NSCursor,
    NSEvent, NSFont, NSFontAttributeName, NSGraphicsContext, NSImage, NSImageCacheMode,
    NSImageCurrentFrame, NSImageCurrentFrameDuration, NSImageFrameCount, NSImageScaling,
    NSImageView, NSLinkAttributeName, NSMenuItem, NSMutableParagraphStyle,
    NSParagraphStyleAttributeName, NSTextAlignment, NSTextField, NSToolTipTag, NSView,
    NSWindowOcclusionState, NSWindowStyleMask,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{
    NSArray, NSCalendar, NSDate, NSLocale, NSMutableAttributedString, NSNumber, NSObject,
    NSObjectProtocol, NSPoint, NSRange, NSRect, NSRelativeDateTimeFormatter,
    NSRelativeDateTimeFormatterStyle, NSRunLoop, NSRunLoopCommonModes, NSSize, NSString,
    NSTimeZone, ns_string,
};
use macos_liquid_glass::objc2_quartz_core::CADisplayLink;
use macos_liquid_glass::window::GlassWindow;
use std::rc::Rc;
use std::time::SystemTime;

/// **The logo's box, in points**: the README's film at the pixels it was made
/// at, 840 by 580, at 2x.
pub const LOGO: NSSize = NSSize::new(420.0, 290.0);
/// The bottom margin, and the gap between two lines, in points.
const MARGIN: CGFloat = 18.0;
const LINE_GAP: CGFloat = 3.0;
/// **The author's picture's side, in points**: 96, which the 400-pixel
/// file fills at 2x four times over, so it stays sharp.
pub const FACE: CGFloat = 96.0;
/// **From the picture's right to the lines' left, in points**: 14, or 15
/// when the window less the block is an odd number of points, so that the
/// two side margins come out whole and equal; a half-point margin would
/// put the picture's edge between two pixels on a 1x display.
const FACE_GAP: CGFloat = 14.0;
/// **From where the film moves to the picture and the lines, in points**:
/// AppKit's standard space between two sibling views, the `-` of the
/// visual format language (`[button]-[textField]`), which
/// `constraintsWithVisualFormat:` resolves to 8 on this SDK (and to 20 from
/// a superview's edge), read off AppKit itself; the header gives no number.
pub const GAP: CGFloat = 8.0;
/// The version line's size, AppKit's default control text; the other two take
/// `NSFont.smallSystemFontSize`.
const VERSION_SIZE: CGFloat = 13.0;

/// The logo's tooltip while the film shows: what pauses it, which a window
/// with no control on it has no other way to say.
const PAUSE_TIP: &str = "Click or press Space to pause or play";
/// The same, as VoiceOver names the logo's action.
const PAUSE_ACTION: &str = "Pause or play";

/// **The frame shown `elapsed` in**, of frames lasting `lasts`, the film
/// repeating: past its last frame comes its first, as the file's NETSCAPE2.0
/// block of count zero has it. A film with no length stays on its first.
pub fn frame_at(lasts: &[Duration], elapsed: Duration) -> FrameNo {
    let whole: u128 = lasts.iter().map(Duration::as_nanos).sum();
    let mut frame = FrameNo::of(0);
    if whole == 0 {
        return frame;
    }
    let mut left = elapsed.as_nanos() % whole;
    for d in lasts {
        if left < d.as_nanos() {
            break;
        }
        left -= d.as_nanos();
        frame = FrameNo::of(frame.raw() + 1);
    }
    frame
}

/// Whether the film's clock is running.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clock {
    Running,
    Stopped,
}

/// Whether the user has paused the film, WCAG 2.2.2's mechanism.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Hold {
    #[default]
    Free,
    Paused,
}

/// Whether any of the window is on screen, so that drawing can be seen.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Seen {
    #[default]
    Yes,
    No,
}

/// Whether the logo moves: the system's Reduce Motion, read when it opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    Plays,
    Still,
}

/// One appearance's logo: its film, if the bundle carries one, and the
/// still shown without one and under Reduce Motion.
pub struct Logo {
    pub film: Option<Retained<NSImage>>,
    pub still: Retained<NSImage>,
}

/// What the window says.
pub struct Content<'a> {
    pub on_dark: Logo,
    pub on_light: Logo,
    /// Where the still goes in the logo's box, in points from its lower
    /// left: where the film draws the lockup.
    pub still_at: NSRect,
    /// **Where the film moves**, in the logo's box from its lower left: every
    /// pixel either film draws off its page, on every frame, lies in it. The
    /// pause and its tooltip are this, and what is under it is bare page.
    pub moves_at: NSRect,
    /// The page round the film, for the window behind it.
    pub page_dark: Argb,
    pub page_light: Argb,
    /// "Version 1.0.0 (abc1234)".
    pub version: &'a str,
    /// The address as shown, and where it goes.
    pub link: &'a str,
    pub url: &'a str,
    pub copyright: &'a str,
    /// The author's picture, when the bundle carries it.
    pub face: Option<Face<'a>>,
    /// When the build's commit was made, the age line's start; `None`, no
    /// age line.
    pub released: Option<SystemTime>,
}

/// **The author's picture and where a click on it goes.**
pub struct Face<'a> {
    /// Drawn round, filling [`FACE`] points.
    pub image: Retained<NSImage>,
    /// Where it goes, handed to `open` as it is.
    pub url: &'a str,
    /// What VoiceOver names it.
    pub label: &'a str,
    /// Its tooltip: the address, as the link line shows its own.
    pub tip: &'a str,
    /// **The opener**, the caller's: the app passes the one its other links
    /// go through, policy and all, and a test passes one that records.
    pub open: Rc<dyn Fn(&str)>,
}

/// **How old the release is, in AppKit's words.**
pub struct Ages {
    formatter: Retained<NSRelativeDateTimeFormatter>,
}

impl Ages {
    /// **The user's**: the formatter's own defaults, which the header gives
    /// as the current calendar, updating as it changes, and that calendar's
    /// locale. What the window uses.
    pub fn of_user() -> Ages {
        Ages {
            formatter: NSRelativeDateTimeFormatter::new(),
        }
    }

    /// **Fixed words**: `locale`, and the Gregorian calendar in UTC, so the
    /// words and the unit boundaries depend on nothing of the machine's.
    /// `None` when Foundation has no Gregorian calendar to give.
    pub fn fixed(locale: &str) -> Option<Ages> {
        let calendar = NSCalendar::calendarWithIdentifier(ns_string!("gregorian"))?;
        let locale = NSLocale::localeWithLocaleIdentifier(&NSString::from_str(locale));
        calendar.setTimeZone(&NSTimeZone::timeZoneForSecondsFromGMT(0));
        calendar.setLocale(Some(&locale));
        let formatter = NSRelativeDateTimeFormatter::new();
        formatter.setCalendar(Some(&calendar));
        formatter.setLocale(Some(&locale));
        Some(Ages { formatter })
    }

    /// **The age of a release made at `released`, at `now`**: the numeric
    /// style, "3 hours ago", the formatter choosing the unit; under a second
    /// the named style's "now", since the numeric has only "in 0 seconds";
    /// and a release ahead of the clock taken as made now, so never "in 1
    /// hour".
    pub fn words(&self, released: SystemTime, now: SystemTime) -> String {
        let since = |t: SystemTime| {
            t.duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64())
        };
        let now = since(now);
        let gone = (now - since(released)).max(0.0);
        self.formatter.setDateTimeStyle(match gone < 1.0 {
            true => NSRelativeDateTimeFormatterStyle::Named,
            false => NSRelativeDateTimeFormatterStyle::Numeric,
        });
        let then = NSDate::dateWithTimeIntervalSince1970(now - gone);
        let at = NSDate::dateWithTimeIntervalSince1970(now);
        self.formatter
            .localizedStringForDate_relativeToDate(&then, &at)
            .to_string()
    }
}

/// The picture's state: its image, its address, and the caller's opener.
struct FaceIvars {
    image: Retained<NSImage>,
    url: String,
    open: Rc<dyn Fn(&str)>,
}

define_class!(
    /// **The author's picture**: drawn round, and a click on it opens its
    /// address through the caller's opener.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = FaceIvars]
    struct FaceView;

    impl FaceView {
        /// The image clipped to the circle its bounds hold, drawn in the
        /// view's own pass, so a render with no window has it round too.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let b = self.bounds();
            NSGraphicsContext::saveGraphicsState_class();
            NSBezierPath::bezierPathWithOvalInRect(b).addClip();
            self.ivars().image.drawInRect(b);
            NSGraphicsContext::restoreGraphicsState_class();
        }

        /// The press is taken here so that its release comes to this view;
        /// what it does waits for the release, as a button's does.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}

        /// **A release over the picture opens it**; one dragged off it does
        /// nothing, as a button's click is let go of.
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            let b = self.bounds();
            if (0.0..b.size.width).contains(&at.x) && (0.0..b.size.height).contains(&at.y) {
                self.press();
            }
        }

        /// A click on the picture in a window behind works at once, as on
        /// a link: it is the click's whole purpose.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::pointingHandCursor());
        }

        /// VoiceOver's press, the click's twin.
        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_press(&self) -> bool {
            self.press();
            true
        }
    }

    unsafe impl NSObjectProtocol for FaceView {}
);

impl FaceView {
    fn new(mtm: MainThreadMarker, face: &Face<'_>, frame: NSRect) -> Retained<FaceView> {
        let this = FaceView::alloc(mtm).set_ivars(FaceIvars {
            image: face.image.clone(),
            url: face.url.to_owned(),
            open: face.open.clone(),
        });
        // SAFETY: `NSView`'s designated initialiser.
        let view: Retained<FaceView> = unsafe { msg_send![super(this), initWithFrame: frame] };
        view.setToolTip(Some(&NSString::from_str(face.tip)));
        view.setAccessibilityElement(true);
        // SAFETY: a constant AppKit declares, read and not written.
        view.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        view.setAccessibilityLabel(Some(&NSString::from_str(face.label)));
        view
    }

    /// The picture's address, to the caller's opener.
    fn press(&self) {
        let ivars = self.ivars();
        (ivars.open)(&ivars.url);
    }
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
    /// Where the film moves: the pause, and its tooltip.
    pause_at: NSRect,
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
    /// How long the film has played, paused time not counted.
    played: Cell<Duration>,
    /// When the clock last ticked; `None` until its first tick after a start
    /// or a pause, so that a pause counts for nothing.
    ticked: Cell<Option<f64>>,
    shows: Cell<Shows>,
    hold: Cell<Hold>,
    seen: Cell<Seen>,
    /// The film's tooltip while it shows: its tag, the string it reads, which
    /// the rect holds weakly, and the rect.
    tip: RefCell<Option<(NSToolTipTag, Retained<NSString>, NSRect)>>,
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

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        /// A point on the film is this view's, not the image view's above it,
        /// so a click there reaches [`AboutView::mouse_down`]; `point` is in
        /// the superview's points, as `NSView`'s header has it.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // The view's bounds are its frame's size at the origin, unscaled.
            let o = self.frame().origin;
            let local = NSPoint::new(point.x - o.x, point.y - o.y);
            if self.on_film(local) {
                Some(Retained::into_super(self.retain()))
            } else {
                // SAFETY: `NSView`'s own, with the point it was sent.
                unsafe { msg_send![super(self), hitTest: point] }
            }
        }

        /// VoiceOver's way to the pause: the logo's one custom action.
        #[unsafe(method(pauseOrPlayFromAccessibility))]
        fn pause_from_accessibility(&self) -> bool {
            self.pause_or_play();
            true
        }

        /// A click on the film pauses or plays it; any other goes where a
        /// click on the page always went.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            if self.on_film(at) {
                self.pause_or_play();
                if let Some(window) = self.window() {
                    window.makeFirstResponder(Some(self));
                }
            } else {
                // SAFETY: `NSResponder`'s own, with the event it was sent.
                let () = unsafe { msg_send![super(self), mouseDown: event] };
            }
        }

        /// Space pauses or plays the film: the click's keyboard twin. Every
        /// other key, Escape's close among them, goes on up the chain.
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let space = event
                .charactersIgnoringModifiers()
                .is_some_and(|c| c.to_string() == " ");
            if space && self.ivars().shows.get() == Shows::Film {
                self.pause_or_play();
            } else {
                // SAFETY: `NSResponder`'s own, with the event it was sent.
                let () = unsafe { msg_send![super(self), keyDown: event] };
            }
        }
    }

    unsafe impl NSObjectProtocol for AboutView {}
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
                    self.tip_over(Some(s.pause_at));
                    self.offer_pause(&s.logo);
                }
                _ => {
                    s.logo.setFrame(s.still_at);
                    s.logo.setImage(Some(&p.still));
                    self.tip_over(None);
                    s.logo.setAccessibilityCustomActions(None);
                }
            }
            if let Some(window) = self.window() {
                window.setBackgroundColor(Some(&p.page));
            }
            NSView::setNeedsDisplay(&s.logo, true);
            NSView::setNeedsDisplay(self, true);
        }
    }

    /// One tick: the film on by the time since the last. A frame not yet
    /// over draws nothing, so a display faster than the film costs no more.
    fn advance(&self, link: &CADisplayLink) {
        let now = link.timestamp();
        let since = self
            .ivars()
            .ticked
            .replace(Some(now))
            .map_or(Duration::ZERO, |then| {
                Duration::try_from_secs_f64(now - then).unwrap_or(Duration::ZERO)
            });
        let played = self.ivars().played.get().saturating_add(since);
        self.ivars().played.set(played);
        if let Some(s) = &*self.ivars().shown.borrow() {
            if let Some(film) = &s.picture(self.look()).film {
                let frame = frame_at(&film.lasts, played);
                if frame != film.current.get() {
                    film.show(frame);
                    NSView::setNeedsDisplay(&s.logo, true);
                }
            }
        }
    }

    /// **The film's tooltip over `rect`, or none**: the one before it taken
    /// off first, so a flip or a reopen never leaves two.
    fn tip_over(&self, rect: Option<NSRect>) {
        if let Some((tag, _, _)) = self.ivars().tip.borrow_mut().take() {
            self.removeToolTip(tag);
        }
        if let Some(rect) = rect {
            let owner = NSString::from_str(PAUSE_TIP);
            // SAFETY: an `NSString` is an owner the page allows ("It can
            // therefore simply be an NSString object"); the view keeps it
            // until the rect is removed; no user data.
            let tag =
                unsafe { self.addToolTipRect_owner_userData(rect, &owner, core::ptr::null_mut()) };
            *self.ivars().tip.borrow_mut() = Some((tag, owner, rect));
        }
    }

    /// The pause as VoiceOver offers it, on the logo while the film shows.
    fn offer_pause(&self, logo: &NSImageView) {
        let name = NSString::from_str(PAUSE_ACTION);
        // SAFETY: the selector is this class's, takes nothing and answers a
        // `BOOL`, as the header asks; the action holds its target weakly,
        // and the view outlives the logo it holds.
        let action = unsafe {
            NSAccessibilityCustomAction::initWithName_target_selector(
                NSAccessibilityCustomAction::alloc(),
                &name,
                ProtocolObject::from_ref(self),
                sel!(pauseOrPlayFromAccessibility),
            )
        };
        logo.setAccessibilityCustomActions(Some(&NSArray::from_retained_slice(&[action])));
    }

    /// Whether `at`, in this view's points, is where the film moves.
    fn on_film(&self, at: NSPoint) -> bool {
        let shows = self.ivars().shows.get() == Shows::Film;
        shows
            && self.ivars().shown.borrow().as_ref().is_some_and(|s| {
                let r = s.pause_at;
                let (x, y) = (at.x - r.origin.x, at.y - r.origin.y);
                (0.0..r.size.width).contains(&x) && (0.0..r.size.height).contains(&y)
            })
    }

    /// The clock runs while the film is neither paused nor out of sight, and
    /// what it was stopped for counts for nothing when it runs again.
    fn pace(&self) {
        let runs = self.ivars().hold.get() == Hold::Free && self.ivars().seen.get() == Seen::Yes;
        if let Some(link) = &*self.ivars().link.borrow() {
            link.setPaused(!runs);
        }
        self.ivars().ticked.set(None);
    }

    /// The user's pause, or the film on from where it paused.
    fn pause_or_play(&self) {
        if self.ivars().shows.get() == Shows::Film {
            self.ivars().hold.set(match self.ivars().hold.get() {
                Hold::Free => Hold::Paused,
                Hold::Paused => Hold::Free,
            });
            self.pace();
        }
    }

    /// Whether any of the window is on screen.
    fn set_seen(&self, seen: Seen) {
        self.ivars().seen.set(seen);
        self.pace();
    }

    fn stop(&self) {
        if let Some(link) = self.ivars().link.borrow_mut().take() {
            link.invalidate();
        }
    }

    /// The film from its start, or the still, by `motion`.
    fn start(&self, motion: Motion) {
        self.stop();
        self.ivars().played.set(Duration::ZERO);
        self.ivars().ticked.set(None);
        self.ivars().seen.set(Seen::Yes);
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
        // A new link runs, and a pause the user made holds across a reopen.
        self.pace();
    }
}

/// Which of AppKit's label colours a line takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ink {
    Label,
    Secondary,
}

/// A line of `text`, set as `align` says.
fn line(
    mtm: MainThreadMarker,
    text: &str,
    size: CGFloat,
    ink: Ink,
    align: NSTextAlignment,
) -> Retained<NSTextField> {
    let f = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    f.setFont(Some(&NSFont::systemFontOfSize(size)));
    f.setAlignment(align);
    let colour = match ink {
        Ink::Label => NSColor::labelColor(),
        Ink::Secondary => NSColor::secondaryLabelColor(),
    };
    f.setTextColor(Some(&colour));
    f
}

/// The link line: `text` going to `url`, as AppKit's own label takes one.
fn link_line(
    mtm: MainThreadMarker,
    text: &str,
    url: &str,
    size: CGFloat,
    align: NSTextAlignment,
) -> Retained<NSTextField> {
    let s = NSMutableAttributedString::initWithString(
        NSMutableAttributedString::alloc(),
        &NSString::from_str(text),
    );
    let all = NSRange::new(0, s.length());
    let paragraph = NSMutableParagraphStyle::new();
    paragraph.setAlignment(align);
    let font = NSFont::systemFontOfSize(size);
    let to = NSString::from_str(url);
    // SAFETY: the keys AppKit declares, each with the class its header gives.
    unsafe {
        s.addAttribute_value_range(NSLinkAttributeName, &to, all);
        s.addAttribute_value_range(NSFontAttributeName, &font, all);
        s.addAttribute_value_range(NSParagraphStyleAttributeName, &paragraph, all);
    }
    let f = NSTextField::labelWithAttributedString(&s, mtm);
    // What makes a label's link clickable.
    f.setSelectable(true);
    f.setAllowsEditingTextAttributes(true);
    f
}

/// **Where the window's parts are, in its content's points**: what a test
/// holds against the layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// The window's content.
    pub content: NSSize,
    /// The film's box, the image view's frame.
    pub film: NSRect,
    /// Where the film moves, which a click pauses and the tooltip covers.
    pub pause: NSRect,
    /// The author's picture, if there is one.
    pub face: Option<NSRect>,
    /// The lines, from the top: the version, the age if there is one, the
    /// link and the copyright.
    pub lines: Vec<NSRect>,
    /// Each line's text's own width, `fittingSize`'s, in the same order.
    pub text: Vec<CGFloat>,
}

/// **The About window, built and not shown.**
pub struct About {
    window: GlassWindow,
    view: Retained<AboutView>,
    version: Retained<NSTextField>,
    age: Option<Retained<NSTextField>>,
    link: Retained<NSTextField>,
    copyright: Retained<NSTextField>,
    face: Option<Retained<FaceView>>,
    released: Option<SystemTime>,
    ages: Ages,
    film_at: NSRect,
    pause_at: NSRect,
    centred: Cell<bool>,
    /// The window's close and its coming and going from the screen, heard
    /// for as long as this lives.
    _heard: [Closing; 2],
}

impl About {
    /// Built, and not shown.
    pub fn new(mtm: MainThreadMarker, content: &Content<'_>) -> About {
        // The lines first: their sizes set the band under the film, and so
        // the window's.
        let align = match &content.face {
            Some(_) => NSTextAlignment::Left,
            None => NSTextAlignment::Center,
        };
        let small = NSFont::smallSystemFontSize();
        let version = line(mtm, content.version, VERSION_SIZE, Ink::Label, align);
        version.setSelectable(true);
        let ages = Ages::of_user();
        let age = content.released.map(|released| {
            let words = ages.words(released, SystemTime::now());
            line(mtm, &words, small, Ink::Label, align)
        });
        let link = link_line(mtm, content.link, content.url, small, align);
        let copyright = line(mtm, content.copyright, small, Ink::Secondary, align);
        let fields: Vec<&Retained<NSTextField>> = [Some(&copyright), Some(&link), age.as_ref()]
            .into_iter()
            .flatten()
            .chain([&version])
            .collect();
        let fits: Vec<NSSize> = fields.iter().map(|f| f.fittingSize()).collect();
        let gaps = LINE_GAP * CGFloat::from(u8::try_from(fields.len() - 1).unwrap_or(0));
        let block = fits.iter().map(|f| f.height).sum::<CGFloat>() + gaps;
        let widest = fits.iter().map(|f| f.width).fold(0.0, CGFloat::max);
        // The band under the film: the picture, or the lines alone.
        let band = match &content.face {
            Some(_) => FACE.max(block),
            None => block,
        };
        // Where the film moves sits `GAP` over the band; the box's bare
        // bottom reaches down into it.
        let moves = &content.moves_at;
        let film_y = MARGIN + band + GAP - moves.origin.y;
        let size = NSSize::new(LOGO.width, film_y + LOGO.height);

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
        let film_at = NSRect::new(NSPoint::new(0.0, film_y), LOGO);
        let in_box =
            |r: &NSRect| NSRect::new(NSPoint::new(r.origin.x, r.origin.y + film_y), r.size);
        let still_at = in_box(&content.still_at);
        let pause_at = in_box(moves);
        logo.setFrame(still_at);
        // First, so that the picture and the lines, in the film's bare band,
        // draw over it.
        view.addSubview(&logo);

        // The picture and the lines as one block, centred across the
        // window, the lines left-aligned at the picture's right; without the
        // picture, each line centred across it.
        let (x, width, face) = match content.face.as_ref() {
            Some(f) => {
                // Whole points throughout: the lines' column a whole number
                // wide, and the odd point, if any, into the picture's gap.
                let widest = widest.ceil();
                let rest = size.width - FACE - FACE_GAP - widest;
                let odd = rest.rem_euclid(2.0);
                let left = ((rest - odd) / 2.0).max(MARGIN);
                let x = left + FACE + FACE_GAP + odd;
                let width = widest.min(size.width - MARGIN - x);
                let at = NSRect::new(
                    NSPoint::new(left, MARGIN + (band - FACE) / 2.0),
                    NSSize::new(FACE, FACE),
                );
                let face = FaceView::new(mtm, f, at);
                view.addSubview(&face);
                (x, width, Some(face))
            }
            None => (0.0, size.width, None),
        };
        // Centred on the band's middle, the picture's.
        let mut y = MARGIN + (band - block) / 2.0;
        for (field, fit) in fields.iter().zip(&fits) {
            field.setFrame(NSRect::new(
                NSPoint::new(x, y),
                NSSize::new(width, fit.height),
            ));
            view.addSubview(field);
            y += fit.height + LINE_GAP;
        }
        *view.ivars().shown.borrow_mut() = Some(Shown {
            on_dark,
            on_light,
            logo,
            film_at,
            still_at,
            pause_at,
        });
        window.set_content_view(&view);
        view.follow();
        let closes = view.clone();
        let moves = view.clone();
        let heard = [
            when_closing(w, move || closes.stop()),
            when_occlusion_changes(w, move || {
                let on = moves
                    .window()
                    .is_some_and(|w| w.occlusionState().contains(NSWindowOcclusionState::Visible));
                moves.set_seen(match on {
                    true => Seen::Yes,
                    false => Seen::No,
                });
            }),
        ];
        About {
            window,
            view,
            version,
            age,
            link,
            copyright,
            face,
            released: content.released,
            ages,
            film_at,
            pause_at,
            centred: Cell::new(false),
            _heard: heard,
        }
    }

    /// **Shown, and the logo from its start or at rest**; centred the first
    /// time, and where it was left after.
    pub fn open(&self, motion: Motion) {
        self.age_at(SystemTime::now());
        if !self.centred.replace(true) {
            self.window.center();
        }
        self.window.show();
        self.view.start(motion);
        // Space reaches the view only as the first responder, and nothing
        // else in the window takes keys but a label a click has selected.
        self.window.ns_window().makeFirstResponder(Some(&self.view));
    }

    /// **The age line as it reads at `now`**: what [`About::open`] does with
    /// the clock as it opens, and what a test does with a time it fixes.
    pub fn age_at(&self, now: SystemTime) {
        if let (Some(field), Some(released)) = (&self.age, self.released) {
            field.setStringValue(&NSString::from_str(&self.ages.words(released, now)));
        }
    }

    /// The age line's text, if the window has one.
    pub fn age_line(&self) -> Option<String> {
        self.age.as_ref().map(|f| f.stringValue().to_string())
    }

    /// The author's picture's view, in which a test finds what is drawn and
    /// sends it a click.
    pub fn face_view(&self) -> Option<Retained<NSView>> {
        self.face.as_ref().map(|f| Retained::into_super(f.clone()))
    }

    /// Where each part is.
    pub fn layout(&self) -> Layout {
        let fields: Vec<&Retained<NSTextField>> = [
            Some(&self.version),
            self.age.as_ref(),
            Some(&self.link),
            Some(&self.copyright),
        ]
        .into_iter()
        .flatten()
        .collect();
        Layout {
            content: self.view.frame().size,
            film: self.film_at,
            pause: self.pause_at,
            face: self.face.as_ref().map(|f| f.frame()),
            lines: fields.iter().map(|f| f.frame()).collect(),
            text: fields.iter().map(|f| f.fittingSize().width).collect(),
        }
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

    /// What a click on the film and Space do. A link made for a window never
    /// on screen does not tick, so its clock's state is what a test can read.
    pub fn pause_or_play(&self) {
        self.view.pause_or_play();
    }

    /// What the window hears when it goes off screen or comes back.
    pub fn set_seen(&self, seen: Seen) {
        self.view.set_seen(seen);
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
        let runs = |link: &Retained<CADisplayLink>| !link.isPaused();
        let clock = match self.view.ivars().link.borrow().as_ref().is_some_and(runs) {
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

    /// The film's tooltip, its text and where it is, while one is there.
    pub fn pause_tip(&self) -> Option<(String, NSRect)> {
        let tip = self.view.ivars().tip.borrow();
        tip.as_ref()
            .map(|(_, text, rect)| (text.to_string(), *rect))
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

    /// **The frame at each moment, repeating**: each frame for its own
    /// delay, the first again after the last however many times round, and
    /// a film with no frames or no length on its first. Perturbation: drop
    /// the `%` and the cases past 1,050 ms stay on the last frame.
    #[test]
    fn a_frame_lasts_its_delay_and_the_film_comes_round() {
        let ms = Duration::from_millis;
        let d = [ms(20), ms(30), ms(1000)];
        let f = FrameNo::of;
        let at = |elapsed| frame_at(&d, ms(elapsed));
        assert_eq!(at(0), f(0));
        assert_eq!(at(19), f(0));
        assert_eq!(at(20), f(1));
        assert_eq!(at(49), f(1));
        assert_eq!(at(50), f(2));
        assert_eq!(at(1049), f(2));
        assert_eq!(at(1050), f(0));
        assert_eq!(at(1070), f(1));
        assert_eq!(at(1050 * 1000 + 50), f(2));
        assert_eq!(frame_at(&[], ms(50)), f(0));
        assert_eq!(frame_at(&[Duration::ZERO, Duration::ZERO], ms(50)), f(0));
    }

    /// **The age in AppKit's words, the unit the formatter's**, in a fixed
    /// locale, `en_US_POSIX`, and the Gregorian calendar in UTC, so the words
    /// and where a unit turns depend on nothing of the machine's; the window
    /// takes the user's (`Ages::of_user`). A minute, hours, days, weeks and
    /// months, and "now" for zero, for under a second and for a commit
    /// ahead of the clock. Perturbation: drop the clamp and the last reads
    /// "in 1 hour"; drop the named style under a second and zero reads "in
    /// 0 seconds"; take the named style throughout and a week reads "last
    /// week".
    #[test]
    fn the_age_is_in_appkits_words_and_never_ahead() {
        let ages = Ages::fixed("en_US_POSIX").expect("a Gregorian calendar");
        // 2027-01-15 08:00:00 UTC.
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let ago = |gone: Duration| ages.words(now - gone, now);
        let secs = Duration::from_secs;
        let days = |n: u64| Duration::from_secs(n * 86_400);
        assert_eq!(ago(secs(60)), "1 minute ago");
        assert_eq!(ago(secs(3 * 3600)), "3 hours ago");
        assert_eq!(ago(days(5)), "5 days ago");
        assert_eq!(ago(days(7)), "1 week ago");
        assert_eq!(ago(days(14)), "2 weeks ago");
        assert_eq!(ago(days(92)), "3 months ago");
        assert_eq!(ago(secs(1)), "1 second ago");
        assert_eq!(ago(Duration::ZERO), "now");
        assert_eq!(ago(Duration::from_millis(400)), "now");
        assert_eq!(ages.words(now + secs(3600), now), "now");
    }
}
