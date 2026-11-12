//! **Notifications and the Dock's progress bar on macOS**: a program's
//! notification posted through `UNUserNotificationCenter`, its click read
//! back through the center's delegate, and a program's progress drawn on
//! the application's Dock tile (`baud-app`'s `notify.rs` decides what to
//! show; this only shows it).
//!
//! * **Only from the app's bundle.** `UNUserNotificationCenter` is "The
//!   central object for managing notification-related activities for your
//!   app or app extension" and `current()` "Returns your app's
//!   notification center" (`[[source: apple-unusernotificationcenter]]`,
//!   `[[source: apple-unusernotificationcenter-current]]`): neither the
//!   pages nor the headers (`[[source: macos-sdk-unusernotificationcenter-h]]`)
//!   say what a process with no bundle gets, so [`Center::shared`] refuses
//!   one whose main bundle has no identifier, and posts nothing.
//! * **Permission, asked when it is needed.** "you must obtain permission
//!   to use them", and "The first time your app makes this authorization
//!   request, the system prompts the person"; "Make the request in a
//!   context that helps people understand why"
//!   (`[[source: apple-asking-permission-notifications]]`). It is asked at
//!   the first notification a program sends, never at launch, and what is
//!   posted before the answer waits for it, in the process's one
//!   [`Center`], whose answer sends it on a turn of its own: a window that
//!   closed meanwhile does not take its post with it.
//! * **The delegate is set first**, as the class's page asks ("Always
//!   assign an object to the delegate property before performing any
//!   tasks"), so a notification shown while baud is active is presented
//!   (`willPresentNotification`) and a click reaches the program
//!   (`didReceiveNotificationResponse`). **One delegate for the process**
//!   ([`shared_delegate`]), as there is one [`Center`] for every window: the
//!   app has one center, and its delegate property is `weak`
//!   (`[[source: macos-sdk-unusernotificationcenter-h]]` 42), so a
//!   delegate a window owned left the others deaf when a newer window
//!   took the center or closed. Each window takes only the clicks on its own
//!   posts ([`Center::clicks`]).
//! * **The Dock tile**: "Draw the tile content yourself ... you must
//!   provide a custom content view" (`[[source: apple-nsdocktile]]`): the
//!   application's icon and a bar under it.
//!
//! `unsafe` is allowed here, as in [`crate::print`], for the messages this
//! sends by name -- the UserNotifications framework has no binding among
//! this crate's dependencies -- each with the argument and return types
//! its header declares (`UNUserNotificationCenter.h`,
//! `UNNotificationContent.h`, `UNNotificationRequest.h`,
//! `UNNotificationAction.h`, `UNNotificationCategory.h`,
//! `UNNotificationAttachment.h`, `UNNotificationSound.h`).
#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use block2::Block;
#[cfg(not(feature = "effect-probe"))]
use block2::RcBlock;
use macos_liquid_glass::objc2::rc::Retained;
#[cfg(not(feature = "effect-probe"))]
use macos_liquid_glass::objc2::runtime::Bool;
use macos_liquid_glass::objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol};
use macos_liquid_glass::objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, extern_protocol,
    msg_send,
};
use macos_liquid_glass::objc2_app_kit::{NSApplication, NSBezierPath, NSColor, NSView};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{
    NSArray, NSError, NSPoint, NSRect, NSSize, NSString, NSURL,
};
#[cfg(not(feature = "effect-probe"))]
use macos_liquid_glass::objc2_foundation::{NSBundle, NSSet};

use crate::turn::Turn;

#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}

extern_protocol!(
    /// The center's delegate (`UNUserNotificationCenter.h`).
    ///
    /// # Safety
    ///
    /// A type implementing it must answer the protocol's methods with the
    /// header's signatures, as [`Delegate`] does.
    // SAFETY: the name is the framework's protocol, which conforms to
    // `NSObject`. The lint does not see the section above through the
    // macro.
    #[allow(clippy::missing_safety_doc)]
    unsafe trait UNUserNotificationCenterDelegate: NSObjectProtocol {}
);

/// `UNNotificationPresentationOptionBanner | List | Sound`
/// (`UNUserNotificationCenter.h`): shown as it would be were baud not
/// active.
const PRESENT: usize = (1 << 4) | (1 << 3) | (1 << 1);
/// `UNAuthorizationOptionSound | UNAuthorizationOptionAlert`: no badge,
/// which baud never sets.
#[cfg(not(feature = "effect-probe"))]
const AUTHORIZE: usize = (1 << 1) | (1 << 2);
/// The identifier a click on the notification itself carries,
/// `UNNotificationDefaultActionIdentifier`.
pub const DEFAULT_ACTION: &str = "com.apple.UNNotificationDefaultActionIdentifier";

/// A click heard by the delegate: the request's identifier and the
/// action's.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Click {
    pub id: String,
    pub action: String,
}

/// The delegate's state: clicks heard, for the windows' next turns. A
/// `Mutex`, because the header does not say on which queue the delegate
/// is called.
#[derive(Default)]
pub struct Heard {
    clicks: Mutex<Vec<Click>>,
}

/// Whose a click is, as a window taking its clicks says ([`Center::clicks`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Whose {
    /// This window posted it: taken.
    This,
    /// Another open window did: left for it.
    Another,
    /// No open window did -- the one that posted it has closed: dropped.
    Closed,
}

impl Heard {
    /// **A click heard**, kept for the window that posted it, and every
    /// listening window woken to take its own ([`wake_listeners`]).
    fn heard(&self, click: Click) {
        if let Ok(mut c) = self.clicks.lock() {
            c.push(click);
        }
        wake_listeners();
    }

    /// The clicks `whose` says are this window's, taken in the order they
    /// were heard; another open window's left for it, and a closed one's
    /// dropped.
    fn take(&self, mut whose: impl FnMut(&str) -> Whose) -> Vec<Click> {
        let Ok(mut all) = self.clicks.lock() else {
            return Vec::new();
        };
        let mut mine = Vec::new();
        all.retain(|c| match whose(&c.id) {
            Whose::This => {
                mine.push(c.clone());
                false
            }
            Whose::Another => true,
            Whose::Closed => false,
        });
        mine
    }
}

define_class!(
    /// The center's delegate.
    #[unsafe(super(NSObject))]
    #[ivars = Heard]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl UNUserNotificationCenterDelegate for Delegate {}

    impl Delegate {
        /// "Asks the delegate how to handle a notification that arrived
        /// while the app was running in the foreground": as a banner, in
        /// the list, with its sound. baud posts while it is active only
        /// where the program asked for that.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(&self, _center: &AnyObject, _n: &AnyObject, handler: &Block<dyn Fn(usize)>) {
            crate::unwind::guard(crate::unwind::SiteName("the notification delegate's will-present"), (), || {
                handler.call((PRESENT,));
            });
        }

        /// A click, on the notification or one of its actions.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(&self, _center: &AnyObject, response: &AnyObject, handler: &Block<dyn Fn()>) {
            crate::unwind::guard(crate::unwind::SiteName("the notification delegate's click"), (), || {
                // SAFETY: `UNNotificationResponse`'s `actionIdentifier` and
                // `notification`, `UNNotification`'s `request` and
                // `UNNotificationRequest`'s `identifier`, each nonnull.
                let (action, id) = unsafe {
                    let action: Retained<NSString> = msg_send![response, actionIdentifier];
                    let n: Retained<AnyObject> = msg_send![response, notification];
                    let req: Retained<AnyObject> = msg_send![&n, request];
                    let id: Retained<NSString> = msg_send![&req, identifier];
                    (action.to_string(), id.to_string())
                };
                self.ivars().heard(Click { id, action });
            });
            // Answered whatever became of the click: the centre waits on it.
            handler.call(());
        }
    }
);

impl Delegate {
    fn new() -> Retained<Delegate> {
        let this = Delegate::alloc().set_ivars(Heard::default());
        // SAFETY: `NSObject`'s `init`.
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// The process's one delegate, made on the main thread the first time
    /// a center is opened and kept for the life of the process.
    static DELEGATE: std::cell::OnceCell<Retained<Delegate>> = const { std::cell::OnceCell::new() };
}

/// **The process's one delegate** ([`DELEGATE`]): every [`Center`] sets
/// and holds this one, so the center's weak reference never outlives it
/// and every window's clicks arrive in one place.
fn shared_delegate() -> Retained<Delegate> {
    DELEGATE.with(|d| d.get_or_init(Delegate::new).clone())
}

/// The identifier of the notification posted last, under the probe: the
/// one the event rig's click names.
#[cfg(feature = "effect-probe")]
static POSTED_LAST: Mutex<Option<String>> = Mutex::new(None);

/// **The event rig's click on the notification posted last**, its default
/// action, put where the delegate's `didReceiveNotificationResponse` puts
/// a click: the response object that method reads is the framework's to
/// make, and the probe build has no framework. Whether there was one to
/// click.
#[cfg(feature = "effect-probe")]
pub fn rig_click_the_last_post() -> bool {
    let Some(id) = POSTED_LAST.lock().ok().and_then(|p| p.clone()) else {
        return false;
    };
    shared_delegate().ivars().heard(Click {
        id,
        action: DEFAULT_ACTION.into(),
    });
    true
}

/// Why notifications cannot be posted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unavailable {
    /// Run outside its bundle: no main bundle identifier.
    NoBundle,
    /// The framework's class is not there.
    NoCenter,
}

/// **A window's news from the center since it last asked**: the answer,
/// the first time this window hears it; and the posts this window made
/// that waited for it and were refused, by identifier. What it allowed was
/// sent on the answer's own turn ([`Center::flush`]), never here.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct News {
    pub answer: Option<Permission>,
    pub refused: Vec<String>,
}

/// The guard's name for the turn the permission's answer schedules.
const ANSWER: crate::unwind::SiteName =
    crate::unwind::SiteName("the notification permission's answer");

/// Where permission stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Permission {
    NotAsked,
    Asked,
    Granted,
    Denied,
}

/// How loud a notification is: `UNNotificationInterruptionLevel`'s passive
/// and active, the two that need no entitlement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Passive,
    Active,
}

/// One notification to post.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Post {
    /// The request's identifier: the same one replaces it ("Calling
    /// -addNotificationRequest: will replace an existing notification
    /// request with the same identifier").
    pub id: String,
    pub title: String,
    /// "The subtitle of the notification", set as iTerm2 sets it; empty
    /// for none.
    pub subtitle: String,
    pub body: String,
    pub sound: PostSound,
    pub level: Level,
    /// Its buttons' labels, each with its number; a click on button `n`
    /// is action `button-n`.
    pub buttons: Vec<(baud_core::NoticeButton, String)>,
    /// An image file to attach, moved into the system's store when it is
    /// posted.
    pub image: Option<std::path::PathBuf>,
}

/// The notification's sound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PostSound {
    Default,
    Silent,
}

/// **The notification center: one for the process**, with its delegate,
/// the permission, what waits for it and every button category. The
/// framework's center is the app's (`+currentNotificationCenter`), so its
/// permission, its waiting posts and its category set are the process's
/// too: a window that closes before permission comes leaves its post
/// waiting here, and the grant sends it on a turn of its own
/// ([`Center::shared`]). Each window reads only its own news, refusals
/// and clicks.
pub struct Center {
    /// UserNotifications' center. **Not in the probe build**, so a method
    /// that would message it there does not compile: each one's probe arm
    /// records or answers in its place.
    #[cfg(not(feature = "effect-probe"))]
    center: Retained<AnyObject>,
    delegate: Retained<Delegate>,
    permission: Arc<Mutex<Permission>>,
    asked: Cell<Permission>,
    waiting: RefCell<Vec<Post>>,
    /// The identifiers of posts that waited and were refused, until the
    /// window that posted each takes it ([`Center::news`]) or closes.
    refused: RefCell<Vec<String>>,
    /// Every category registered: `setNotificationCategories:` replaces
    /// the app's whole set, so the one set holds every window's. Read only
    /// by what the probe configures out.
    #[cfg_attr(feature = "effect-probe", allow(dead_code))]
    categories: RefCell<Vec<(String, Retained<AnyObject>)>>,
    /// Under the probe, the identifiers sent and not removed: what
    /// Notification Center would hold, and the probe's answer when asked.
    #[cfg(feature = "effect-probe")]
    held: RefCell<Vec<String>>,
}

thread_local! {
    /// The process's one center, opened on the main thread the first time
    /// a window asks for it, or why there is none.
    static SHARED: std::cell::OnceCell<Result<std::rc::Rc<Center>, Unavailable>> =
        const { std::cell::OnceCell::new() };
}

/// `f` with the process's center, if one was opened on this thread.
fn with_shared(f: impl FnOnce(&Center)) {
    SHARED.with(|s| {
        if let Some(Ok(c)) = s.get() {
            f(c);
        }
    });
}

/// **The windows the center's news wakes**: every open window's wake and
/// its owed flag, by which its window's close finds and takes them out
/// ([`Listening`]). The center and its delegate are the process's, and a
/// click, a refusal, the permission's answer or the delivered list's may
/// be any window's, so each marks and wakes them all and every window
/// takes its own on that turn (`Center::news`, `Center::clicks`,
/// `Delivered::take`). A `Mutex`, as the framework's handlers and the
/// delegate run on queues the headers do not name.
static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

/// One listening window: what wakes it, and whether news is owed, its
/// flag shared with its [`Listening`], which finds it by that flag.
struct Listener {
    wake: Box<dyn Fn() + Send>,
    owed: Arc<AtomicBool>,
}

/// **Whether the center has news a window has not taken**: set just before
/// each wake that brings it, taken by the window's turn
/// ([`Listening::take`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoticesOwed {
    No,
    Yes,
}

/// **A window listening to the center's news**, from its opening; dropped
/// with the window, which takes its wake out of [`LISTENERS`].
#[derive(Debug)]
pub struct Listening {
    owed: Arc<AtomicBool>,
}

impl Listening {
    /// **Whether news is owed this window**, taken: set by each producer
    /// just before its wake ([`wake_listeners`]) and by a timed step of the
    /// window's own ([`Listening::owe`]); a turn with none owed reads one
    /// flag and nothing else.
    pub fn take(&self) -> NoticesOwed {
        match self.owed.swap(false, Ordering::AcqRel) {
            true => NoticesOwed::Yes,
            false => NoticesOwed::No,
        }
    }

    /// **What marks news owed when it is dropped**, for a wake of the
    /// window's own -- a notice's `w` deadline -- to carry into its
    /// callback and let go there, before the turn it posts.
    pub fn owe(&self) -> Owe {
        Owe(Arc::clone(&self.owed))
    }
}

/// News owed a window once this is dropped ([`Listening::owe`]).
#[derive(Debug)]
pub struct Owe(Arc<AtomicBool>);

impl Drop for Owe {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        if let Ok(mut l) = LISTENERS.lock() {
            l.retain(|x| !Arc::ptr_eq(&x.owed, &self.owed));
        }
    }
}

/// **`wake` called with the center's news**, news marked owed first, until
/// the [`Listening`] is dropped.
pub fn listen(wake: impl Fn() + Send + 'static) -> Listening {
    let owed = Arc::new(AtomicBool::new(false));
    if let Ok(mut l) = LISTENERS.lock() {
        l.push(Listener {
            wake: Box::new(wake),
            owed: Arc::clone(&owed),
        });
    }
    Listening { owed }
}

/// How many windows listen: the registry empties as they close.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Listeners(usize);

/// How many windows listen now.
#[cfg(test)]
fn listeners() -> Listeners {
    Listeners(LISTENERS.lock().map_or(0, |l| l.len()))
}

/// **Every listening window marked owed and woken**, once what it will
/// take is stored.
fn wake_listeners() {
    if let Ok(l) = LISTENERS.lock() {
        for x in l.iter() {
            x.owed.store(true, Ordering::Release);
            (x.wake)();
        }
    }
}

/// **What a window was last told of permission**, so each window hears
/// the answer once ([`Center::news`]): a window's core is told none is
/// delivered when it is refused, whichever window asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Told(Permission);

impl Told {
    /// Told nothing yet.
    pub const NOTHING: Told = Told(Permission::NotAsked);
}

/// **Where a window's answer to "which are delivered" arrives**: its own,
/// so one window never takes the answer to another's question. Written by
/// the framework's handler on its own queue, read by the window.
#[derive(Clone, Default, Debug)]
pub struct Delivered(Arc<Mutex<Option<Vec<String>>>>);

impl Delivered {
    /// The answer to the last ask, once.
    pub fn take(&self) -> Option<Vec<String>> {
        self.0.lock().ok().and_then(|mut d| d.take())
    }
}

impl Center {
    /// **The process's center**, opened the first time and the same one
    /// after, the process's delegate set; or why there is none. **Under
    /// the effect probe there is no framework center**: the bundle is not
    /// asked and nothing is sent to UserNotifications, so the event rig's
    /// unbundled child has a `Center` whose methods record or answer in
    /// place of the framework.
    pub fn shared(_mtm: MainThreadMarker) -> Result<std::rc::Rc<Center>, Unavailable> {
        SHARED.with(|s| {
            s.get_or_init(|| Center::opened().map(std::rc::Rc::new))
                .clone()
        })
    }

    /// A center of its own, not the process's: the probe's tests, off the
    /// main thread, where [`Center::shared`] is not.
    #[cfg(feature = "effect-probe")]
    pub fn unshared() -> Result<Center, Unavailable> {
        Center::opened()
    }

    /// [`Center::shared`]'s opening, which reads no marker, so a test off
    /// the main thread can ask it.
    fn opened() -> Result<Center, Unavailable> {
        #[cfg(not(feature = "effect-probe"))]
        let center: Retained<AnyObject> = {
            // An identifier, and a bundle that is an app's: a binary run
            // on its own has neither, and an `Info.plist` linked into one
            // would give it the first.
            let bundle = NSBundle::mainBundle();
            if bundle.bundleIdentifier().is_none()
                || !bundle.bundlePath().to_string().ends_with(".app")
            {
                return Err(Unavailable::NoBundle);
            }
            let Some(cls) = AnyClass::get(c"UNUserNotificationCenter") else {
                return Err(Unavailable::NoCenter);
            };
            // SAFETY: `+currentNotificationCenter`, nonnull.
            unsafe { msg_send![cls, currentNotificationCenter] }
        };
        let delegate = shared_delegate();
        // SAFETY: `delegate` is `id <UNUserNotificationCenterDelegate>`,
        // held weakly by the center and strongly here and by the process
        // ([`DELEGATE`]); setting the same one again changes nothing.
        #[cfg(not(feature = "effect-probe"))]
        unsafe {
            let _: () = msg_send![&center, setDelegate: &*delegate];
        }
        Ok(Center {
            #[cfg(not(feature = "effect-probe"))]
            center,
            delegate,
            permission: Arc::new(Mutex::new(Permission::NotAsked)),
            asked: Cell::new(Permission::NotAsked),
            waiting: RefCell::new(Vec::new()),
            refused: RefCell::new(Vec::new()),
            categories: RefCell::new(Vec::new()),
            #[cfg(feature = "effect-probe")]
            held: RefCell::new(Vec::new()),
        })
    }

    fn permission(&self) -> Permission {
        self.permission.lock().map_or(Permission::Denied, |p| *p)
    }

    /// **Post one**, once permission is granted: asked now if it never
    /// was, and the post kept until the answer, whose own turn sends it
    /// ([`Center::flush`]). On the effect queue's turn, as both the post
    /// and the ask are effects.
    pub fn post(&self, t: &Turn<'_>, p: Post) {
        let _ = t;
        self.post_now(p);
    }

    /// [`Center::post`] without a turn: the probe's tests, off the main
    /// thread, where no turn is made.
    #[cfg(feature = "effect-probe")]
    pub fn post_off_turn(&self, p: Post) {
        self.post_now(p);
    }

    /// [`Center::post`]'s body.
    fn post_now(&self, p: Post) {
        #[cfg(feature = "effect-probe")]
        if let Ok(mut last) = POSTED_LAST.lock() {
            *last = Some(p.id.clone());
        }
        match self.permission() {
            Permission::Granted => self.send(&p),
            Permission::Denied => {}
            Permission::NotAsked | Permission::Asked => {
                self.waiting.borrow_mut().push(p);
                self.ask();
            }
        }
    }

    /// **Permission asked, once**: the answer comes later, on the
    /// framework's queue, and is written there; the posts that waited for
    /// it are then sent, or refused, on a main-queue turn of its own
    /// ([`Center::flush`]), which reaches the process's center through
    /// [`SHARED`] rather than carrying it, and needs no window.
    fn ask(&self) {
        if self.asked.get() != Permission::NotAsked {
            return;
        }
        self.asked.set(Permission::Asked);
        // Under the probe: recorded, and the grant given on a later turn,
        // as the system's handler answers: never inside the call that
        // asked, so a second post there waits too.
        #[cfg(feature = "effect-probe")]
        {
            crate::probe::record(crate::probe::EffectKind::Authorize);
            crate::turn::later(ANSWER, |t| {
                with_shared(|c| {
                    c.answer(Permission::Granted);
                    c.flush(t);
                });
            });
        }
        #[cfg(not(feature = "effect-probe"))]
        {
            let shared = Arc::clone(&self.permission);
            let done = RcBlock::new(move |granted: Bool, _e: *mut NSError| {
                crate::unwind::guard(ANSWER, (), || {
                    if let Ok(mut p) = shared.lock() {
                        *p = match granted.as_bool() {
                            true => Permission::Granted,
                            false => Permission::Denied,
                        };
                    }
                    crate::turn::later(ANSWER, |t| with_shared(|c| c.flush(t)));
                });
            });
            // SAFETY: `-requestAuthorizationWithOptions:completionHandler:`,
            // the options an `NSUInteger`, the handler `void (^)(BOOL,
            // NSError *)`.
            unsafe {
                let _: () = msg_send![&self.center, requestAuthorizationWithOptions: AUTHORIZE, completionHandler: &*done];
            }
        }
    }

    /// The answer written, as the framework's handler writes it.
    #[cfg(feature = "effect-probe")]
    fn answer(&self, p: Permission) {
        answered_elsewhere(&self.permission, p);
    }

    /// **The answer, and what waited for it, on its own turn**: once
    /// granted every waiting post sent, once refused each one's identifier
    /// kept for the window that posted it; before the answer, nothing.
    pub fn flush(&self, t: &Turn<'_>) {
        let _ = t;
        self.flush_now();
    }

    /// The probe's tests' answer and its turn, off the main thread: the
    /// answer written and what waited flushed, as the handler's turn does.
    #[cfg(feature = "effect-probe")]
    pub fn answered_off_turn(&self, p: Permission) {
        self.answer(p);
        self.flush_now();
    }

    /// [`Center::flush`]'s body.
    fn flush_now(&self) {
        match self.permission() {
            Permission::Granted => {
                let waiting = core::mem::take(&mut *self.waiting.borrow_mut());
                for post in &waiting {
                    self.send(post);
                }
            }
            Permission::Denied => {
                let waiting = core::mem::take(&mut *self.waiting.borrow_mut());
                self.refused
                    .borrow_mut()
                    .extend(waiting.into_iter().map(|post| post.id));
            }
            Permission::NotAsked | Permission::Asked => return,
        }
        // Answered: every window hears it, and takes its refusals, on the
        // turn this wakes (`Center::news`).
        wake_listeners();
    }

    /// **A window's news since it last asked**: the answer, once to each
    /// window ([`Told`]); and the refused posts that window made, taken,
    /// `whose` reading each one's identifier: another open window's left
    /// for it, a closed window's dropped. Nothing is sent here.
    pub fn news(&self, told: &mut Told, mut whose: impl FnMut(&str) -> Whose) -> News {
        let p = self.permission();
        let answer = match p {
            Permission::Granted | Permission::Denied if told.0 != p => {
                told.0 = p;
                Some(p)
            }
            _ => None,
        };
        let mut refused = Vec::new();
        self.refused.borrow_mut().retain(|id| match whose(id) {
            Whose::This => {
                refused.push(id.clone());
                false
            }
            Whose::Another => true,
            Whose::Closed => false,
        });
        News { answer, refused }
    }

    /// **A closed window's refusals forgotten**, on its close's turn, so
    /// the process's list never keeps one no window will read.
    pub fn forget_refused(&self, mut gone: impl FnMut(&str) -> bool) {
        self.refused.borrow_mut().retain(|id| !gone(id));
    }

    /// The permission the system last gave.
    pub fn allowed(&self) -> Permission {
        self.permission()
    }

    /// **Ask which notifications Notification Center holds**: "Fetches all
    /// of your app's delivered notifications that are still present in
    /// Notification Center", answered later into the asking window's
    /// `into`. On the effect queue's turn, as it messages the center.
    pub fn ask_delivered(&self, t: &Turn<'_>, into: &Delivered) {
        let _ = t;
        self.ask_delivered_now(into);
    }

    /// [`Center::ask_delivered`]'s body, which a test off the main thread
    /// can ask.
    fn ask_delivered_now(&self, into: &Delivered) {
        // Under the probe: no center to ask, and the answer, what it sent
        // and has not removed, comes as the handler's would, from another
        // thread.
        #[cfg(feature = "effect-probe")]
        if crate::probe::notice_list() == crate::probe::NoticeList::Answers {
            answered_elsewhere(&into.0, Some(self.held.borrow().clone()));
            wake_listeners();
        }
        #[cfg(not(feature = "effect-probe"))]
        {
            let shared = Arc::clone(&into.0);
            let done = RcBlock::new(move |list: core::ptr::NonNull<NSArray<AnyObject>>| {
                crate::unwind::guard(
                    crate::unwind::SiteName("the delivered notifications' answer"),
                    (),
                    || {
                        // SAFETY: the handler's `NSArray<UNNotification *> *`, nonnull.
                        let list = unsafe { list.as_ref() };
                        let ids: Vec<String> = list
                            .iter()
                            .map(|n| {
                                // SAFETY: a notification's `request` and its
                                // `identifier`, each nonnull.
                                unsafe {
                                    let req: Retained<AnyObject> = msg_send![&n, request];
                                    let id: Retained<NSString> = msg_send![&req, identifier];
                                    id.to_string()
                                }
                            })
                            .collect();
                        if let Ok(mut d) = shared.lock() {
                            *d = Some(ids);
                        }
                        // The asking window takes it on the turn this wakes.
                        wake_listeners();
                    },
                );
            });
            // SAFETY: `-getDeliveredNotificationsWithCompletionHandler:`, the
            // handler `void (^)(NSArray<UNNotification *> *)`.
            unsafe {
                let _: () =
                    msg_send![&self.center, getDeliveredNotificationsWithCompletionHandler: &*done];
            }
        }
    }

    /// **The clicks on this window's posts heard since the last call**:
    /// `whose` reads each click's request identifier; another open
    /// window's are left for it, and a closed window's dropped.
    pub fn clicks(&self, whose: impl FnMut(&str) -> Whose) -> Vec<Click> {
        self.delegate.ivars().take(whose)
    }

    /// **Close one**: removed from Notification Center, or, still waiting
    /// for permission, before it shows; on the effect queue's turn.
    pub fn remove(&self, t: &Turn<'_>, id: &str) {
        let _ = t;
        self.remove_now(id);
    }

    /// [`Center::remove`]'s body, which a test off the main thread can ask.
    fn remove_now(&self, id: &str) {
        self.waiting.borrow_mut().retain(|p| p.id != id);
        #[cfg(feature = "effect-probe")]
        {
            self.held.borrow_mut().retain(|h| h != id);
            crate::probe::record(crate::probe::EffectKind::Unnotice);
        }
        #[cfg(not(feature = "effect-probe"))]
        remove(&self.center, id);
    }

    fn send(&self, p: &Post) {
        #[cfg(feature = "effect-probe")]
        {
            let mut held = self.held.borrow_mut();
            held.retain(|h| *h != p.id);
            held.push(p.id.clone());
            crate::probe::record(crate::probe::EffectKind::Notice);
        }
        #[cfg(not(feature = "effect-probe"))]
        {
            let category = self.category(&p.buttons);
            let Some(req) = request_of(p, category.as_deref()) else {
                return;
            };
            // SAFETY: `-addNotificationRequest:withCompletionHandler:` with a
            // nil handler, which the header allows.
            unsafe {
                let _: () = msg_send![
                    &self.center,
                    addNotificationRequest: &*req,
                    withCompletionHandler: Option::<&Block<dyn Fn(*mut NSError)>>::None
                ];
            }
        }
    }

    /// The category whose actions are these buttons, registered the first
    /// time they are seen; `None` with none.
    #[cfg(not(feature = "effect-probe"))]
    fn category(&self, buttons: &[(baud_core::NoticeButton, String)]) -> Option<String> {
        if buttons.is_empty() {
            return None;
        }
        let named: Vec<String> = buttons
            .iter()
            .map(|(n, label)| format!("{}\u{1e}{label}", n.number()))
            .collect();
        let key = format!("baud-buttons-{}", named.join("\u{1f}"));
        if self.categories.borrow().iter().any(|(k, _)| *k == key) {
            return Some(key);
        }
        let cat = category_object(&key, buttons)?;
        self.categories.borrow_mut().push((key.clone(), cat));
        let all: Vec<Retained<AnyObject>> = self
            .categories
            .borrow()
            .iter()
            .map(|(_, c)| c.clone())
            .collect();
        let set = NSSet::from_retained_slice(&all);
        // SAFETY: `-setNotificationCategories:`, an `NSSet` of categories,
        // which replaces the set: every one seen is in it.
        unsafe {
            let _: () = msg_send![&self.center, setNotificationCategories: &*set];
        }
        Some(key)
    }
}

/// **A post's request**, with its content -- title, subtitle, body,
/// sound, level, category and image -- and no trigger, which delivers it
/// now. Built without the center, so a test can build one.
#[cfg_attr(feature = "effect-probe", allow(dead_code))]
fn request_of(p: &Post, category: Option<&str>) -> Option<Retained<AnyObject>> {
    let content = new_of(c"UNMutableNotificationContent")?;
    // SAFETY: `UNMutableNotificationContent`'s setters, each with the type
    // its header declares (`[[source: macos-sdk-unnotificationcontent-h]]`
    // 106-125 for the body, the subtitle and the title); `+defaultSound`
    // nonnull.
    unsafe {
        let _: () = msg_send![&content, setTitle: &*NSString::from_str(&p.title)];
        let _: () = msg_send![&content, setSubtitle: &*NSString::from_str(&p.subtitle)];
        let _: () = msg_send![&content, setBody: &*NSString::from_str(&p.body)];
        match (p.sound, AnyClass::get(c"UNNotificationSound")) {
            (PostSound::Default, Some(s)) => {
                let sound: Retained<AnyObject> = msg_send![s, defaultSound];
                let _: () = msg_send![&content, setSound: &*sound];
            }
            _ => {
                let _: () = msg_send![&content, setSound: Option::<&AnyObject>::None];
            }
        }
        let level: usize = match p.level {
            Level::Passive => 0,
            Level::Active => 1,
        };
        let _: () = msg_send![&content, setInterruptionLevel: level];
        if let Some(cat) = category {
            let _: () = msg_send![&content, setCategoryIdentifier: &*NSString::from_str(cat)];
        }
        if let Some(path) = &p.image {
            if let Some(a) = attachment(path) {
                let list = NSArray::from_retained_slice(&[a]);
                let _: () = msg_send![&content, setAttachments: &*list];
            }
        }
    }
    let req_cls = AnyClass::get(c"UNNotificationRequest")?;
    // SAFETY: `+requestWithIdentifier:content:trigger:`, nonnull, a nil
    // trigger delivering it now.
    Some(unsafe {
        msg_send![
            req_cls,
            requestWithIdentifier: &*NSString::from_str(&p.id),
            content: &*content,
            trigger: Option::<&AnyObject>::None
        ]
    })
}

/// A category whose actions are `buttons`, each `button-n` by the number
/// it carries, under `key`.
#[cfg_attr(feature = "effect-probe", allow(dead_code))]
fn category_object(
    key: &str,
    buttons: &[(baud_core::NoticeButton, String)],
) -> Option<Retained<AnyObject>> {
    let action_cls = AnyClass::get(c"UNNotificationAction")?;
    let cat_cls = AnyClass::get(c"UNNotificationCategory")?;
    let actions: Vec<Retained<AnyObject>> = buttons
        .iter()
        .map(|(n, label)| {
            // SAFETY: `+actionWithIdentifier:title:options:`, nonnull, no
            // options.
            unsafe {
                msg_send![
                    action_cls,
                    actionWithIdentifier: &*NSString::from_str(&format!("button-{}", n.number())),
                    title: &*NSString::from_str(label),
                    options: 0usize
                ]
            }
        })
        .collect();
    let actions = NSArray::from_retained_slice(&actions);
    let none: Retained<NSArray<NSString>> = NSArray::new();
    // SAFETY: `+categoryWithIdentifier:actions:intentIdentifiers:options:`,
    // nonnull, no options.
    Some(unsafe {
        msg_send![
            cat_cls,
            categoryWithIdentifier: &*NSString::from_str(key),
            actions: &*actions,
            intentIdentifiers: &*none,
            options: 0usize
        ]
    })
}

/// `[Class new]`, by the class's name.
#[cfg_attr(feature = "effect-probe", allow(dead_code))]
fn new_of(name: &core::ffi::CStr) -> Option<Retained<AnyObject>> {
    let cls = AnyClass::get(name)?;
    // SAFETY: `+new`, which every `NSObject` class answers.
    Some(unsafe { msg_send![cls, new] })
}

/// An image file as an attachment, or `None` when the system refuses it.
#[cfg_attr(feature = "effect-probe", allow(dead_code))]
fn attachment(path: &std::path::Path) -> Option<Retained<AnyObject>> {
    let cls = AnyClass::get(c"UNNotificationAttachment")?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
    let mut err: *mut NSError = core::ptr::null_mut();
    // SAFETY: `+attachmentWithIdentifier:URL:options:error:`, nullable, a
    // nil options dictionary and an out-error.
    unsafe {
        msg_send![
            cls,
            attachmentWithIdentifier: &*NSString::from_str("icon"),
            URL: &*url,
            options: Option::<&AnyObject>::None,
            error: &mut err
        ]
    }
}

/// The two removals from `center`; built only without the probe, whose
/// callers record the removal in its place.
#[cfg(not(feature = "effect-probe"))]
fn remove(center: &AnyObject, id: &str) {
    let ids = NSArray::from_retained_slice(&[NSString::from_str(id)]);
    // SAFETY: the two removals, each an `NSArray<NSString *>`.
    unsafe {
        let _: () = msg_send![center, removeDeliveredNotificationsWithIdentifiers: &*ids];
        let _: () = msg_send![center, removePendingNotificationRequestsWithIdentifiers: &*ids];
    }
}

/// **The probe's answer, given as UserNotifications gives one**: written
/// into `shared` by another thread, and joined, so the read after it sees
/// it.
#[cfg(feature = "effect-probe")]
fn answered_elsewhere<T: Send + 'static>(shared: &Arc<Mutex<T>>, answer: T) {
    let shared = Arc::clone(shared);
    let _ = std::thread::spawn(move || {
        if let Ok(mut s) = shared.lock() {
            *s = answer;
        }
    })
    .join();
}

/// **Close one by its identifier**: for an expiry, on the turn
/// [`crate::turn::after`] schedules, which holds the identifier and not
/// the center: the process's own, [`Center::shared`].
pub fn remove_by_id(t: &Turn<'_>, id: &str) {
    let _ = t;
    remove_by_id_now(id);
}

/// [`remove_by_id`]'s body, which a test off the main thread can ask.
fn remove_by_id_now(id: &str) {
    with_shared(|c| c.remove_now(id));
}

/// A progress bar's look: its fill, and its colour by state.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bar {
    /// How full, 0 to 1; `None` runs with no fraction, drawn full and
    /// faint.
    pub fraction: Option<f64>,
    pub tone: Tone,
}

/// The bar's colour: the accent colour, red for an error, yellow for a
/// pause, the colours Windows' taskbar gives ConEmu's states.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Normal,
    Error,
    Paused,
}

define_class!(
    /// The Dock tile's content: the application's icon and a bar.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Cell<Option<Bar>>]
    struct Tile;

    impl Tile {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            crate::unwind::guard(crate::unwind::SiteName("the Dock tile's draw"), (), || {
                let b = self.bounds();
                let app = NSApplication::sharedApplication(self.mtm());
                if let Some(icon) = app.applicationIconImage() {
                    icon.drawInRect(b);
                }
                let Some(bar) = self.ivars().get() else {
                    return;
                };
                let (w, h) = (b.size.width, b.size.height);
                let track = NSRect::new(NSPoint::new(w * 0.1, h * 0.06), NSSize::new(w * 0.8, h * 0.12));
                let r: CGFloat = track.size.height / 2.0;
                NSColor::colorWithWhite_alpha(0.0, 0.55).setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(track, r, r).fill();
                let colour = match bar.tone {
                    Tone::Normal => NSColor::controlAccentColor(),
                    Tone::Error => NSColor::systemRedColor(),
                    Tone::Paused => NSColor::systemYellowColor(),
                };
                let (fill, alpha) = match bar.fraction {
                    Some(f) => (f.clamp(0.0, 1.0), 1.0),
                    None => (1.0, 0.45),
                };
                colour.colorWithAlphaComponent(alpha).setFill();
                let inner = NSRect::new(track.origin, NSSize::new(track.size.width * fill, track.size.height));
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(inner, r, r).fill();
            });
        }
    }
);

thread_local! {
    /// The tile's view, made at the first bar and kept.
    static TILE: RefCell<Option<Retained<Tile>>> = const { RefCell::new(None) };
}

/// **The Dock tile shows `bar`**, or the plain icon for `None`; on the
/// effect queue's turn.
pub fn show_bar(t: &Turn<'_>, bar: Option<Bar>) {
    #[cfg(feature = "effect-probe")]
    {
        let _ = (t, bar);
        crate::probe::record(crate::probe::EffectKind::DockBar);
    }
    #[cfg(not(feature = "effect-probe"))]
    {
        let mtm = t.main_thread();
        let app = NSApplication::sharedApplication(mtm);
        let tile = app.dockTile();
        match bar {
            None => tile.setContentView(None),
            Some(b) => TILE.with(|t| {
                let mut t = t.borrow_mut();
                let view = t.get_or_insert_with(|| {
                    let this = Tile::alloc(mtm).set_ivars(Cell::new(None));
                    // SAFETY: `NSView`'s `init`.
                    unsafe { msg_send![super(this), init] }
                });
                view.ivars().set(Some(b));
                let size = tile.size();
                view.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), size));
                tile.setContentView(Some(view));
            }),
        }
        tile.display();
    }
}

/// How long the Dock icon asks for attention.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ask {
    /// "yes ... bouncing the dock icon indefinitely": a critical request,
    /// which bounces until the app is active.
    UntilActive,
    /// "once ... a single time": an informational one.
    Once,
}

/// **Ask for the user's attention** (`[[source:
/// apple-request-user-attention]]`): "Activating the app cancels the user
/// attention request", and "Sending requestUserAttention(_:) to an app that
/// is already active has no effect". The request's id, to cancel it. On
/// the effect queue's turn.
pub fn attention(t: &Turn<'_>, ask: Ask) -> AttentionId {
    #[cfg(feature = "effect-probe")]
    {
        let _ = (t, ask);
        crate::probe::record(crate::probe::EffectKind::Attention);
        AttentionId(0)
    }
    #[cfg(not(feature = "effect-probe"))]
    {
        use macos_liquid_glass::objc2_app_kit::NSRequestUserAttentionType as T;
        AttentionId(
            NSApplication::sharedApplication(t.main_thread()).requestUserAttention(match ask {
                Ask::UntilActive => T::CriticalRequest,
                Ask::Once => T::InformationalRequest,
            }),
        )
    }
}

/// A request [`attention`] made, to cancel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttentionId(isize);

/// Cancel a request `attention` made: "no to cancel a previous request";
/// on the effect queue's turn.
pub fn cancel_attention(t: &Turn<'_>, id: AttentionId) {
    #[cfg(feature = "effect-probe")]
    {
        let _ = (t, id);
        crate::probe::record(crate::probe::EffectKind::Unattention);
    }
    #[cfg(not(feature = "effect-probe"))]
    NSApplication::sharedApplication(t.main_thread()).cancelUserAttentionRequest(id.0);
}

#[cfg(test)]
mod tests {
    //! **The messages the center would be sent, sent to the objects
    //! alone**: a request built with its content, a category with its
    //! actions, and the delegate registered -- in a debug build, where
    //! objc2 checks each message's argument and return types against the
    //! method's own encoding. Nothing here reaches the notification
    //! center, permission or the Dock.
    use super::*;

    fn click(id: &str) -> Click {
        Click {
            id: id.into(),
            action: DEFAULT_ACTION.into(),
        }
    }

    /// **Every center shares the process's one delegate**, so a second
    /// window's center does not take the clicks from the first's, and a
    /// window's close leaves the others a delegate: a click heard through
    /// one is there for a window holding the other. The control: two
    /// delegates made apart are two objects.
    #[test]
    fn every_center_shares_one_delegate() {
        let (a, b) = (shared_delegate(), shared_delegate());
        assert!(Retained::as_ptr(&a) == Retained::as_ptr(&b), "one delegate");
        let (x, y) = (Delegate::new(), Delegate::new());
        assert!(
            Retained::as_ptr(&x) != Retained::as_ptr(&y),
            "the control: two made apart are two"
        );
        drop(a);
        b.ivars().heard(click("w0-1"));
        let c = shared_delegate();
        assert_eq!(c.ivars().take(|_| Whose::This), vec![click("w0-1")]);
    }

    /// **A listening window is marked owed and woken by the center's news,
    /// and its close takes it out of the registry**: a click heard marks it
    /// owed before its wake, which the window takes, a timed step of its
    /// own marks it as its guard is let go, and once it is dropped
    /// the registry is as it was and the next news wakes it no more.
    /// Breaks: `Listening`'s drop taking nothing out -- "back to as it
    /// was" fails; `wake_listeners` waking without marking -- "owed with
    /// its wake" fails.
    #[test]
    fn a_window_listens_until_it_closes() {
        // Other tests here wake every listener too, so nothing is asserted
        // that another test's news could make true or false: only what this
        // window's own news and close must do.
        use std::sync::atomic::AtomicUsize;
        let before = listeners();
        let woke = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&woke);
        let l = listen(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(listeners(), Listeners(before.0 + 1), "listening");
        Heard::default().heard(click("w0-1"));
        assert!(woke.load(Ordering::Relaxed) >= 1, "woken");
        assert_eq!(l.take(), NoticesOwed::Yes, "owed with its wake");
        drop(l.owe());
        assert_eq!(l.take(), NoticesOwed::Yes, "its own timed step");
        drop(l);
        assert_eq!(listeners(), before, "back to as it was");
        let at_close = woke.load(Ordering::Relaxed);
        Heard::default().heard(click("w0-2"));
        assert_eq!(woke.load(Ordering::Relaxed), at_close, "no wake after");
    }

    /// **A click goes to the window that posted it**: each window takes
    /// only its own, in the order heard, another open window's waits for
    /// it, and a closed window's is dropped.
    #[test]
    fn a_click_goes_to_the_window_that_posted_it() {
        let heard = Heard::default();
        for id in ["w0-1", "w1-1", "w9-1", "w0-2"] {
            heard.heard(click(id));
        }
        let whose = |me: &'static str| {
            move |id: &str| match id.split('-').next() {
                Some(w) if w == me => Whose::This,
                Some("w0" | "w1") => Whose::Another,
                _ => Whose::Closed,
            }
        };
        assert_eq!(heard.take(whose("w1")), vec![click("w1-1")]);
        assert_eq!(
            heard.take(whose("w0")),
            vec![click("w0-1"), click("w0-2")],
            "the first window's, though the second took its turn first"
        );
        assert_eq!(
            heard.take(|_| Whose::This),
            vec![],
            "the closed window's dropped"
        );
    }

    /// The buttons `labels`, joined by U+2028, as the core numbers them:
    /// only the core makes a `NoticeButton`.
    fn numbered(labels: &str) -> Vec<(baud_core::NoticeButton, String)> {
        use baud_core::*;
        let ws = baud_domain::WinSize {
            ws_col: 20,
            ws_row: 4,
        };
        let mut owned = Owned::new(&ws).with_notices(NoticeBudget::with_icon(1024));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_notice_support(Some(NoticeSupport {
            actions: Activation::FOCUS_AND_REPORT,
            close_events: CloseEvents::Tracked,
            occasions: Occasions::UnfocusedAndInvisible,
            urgencies: Urgencies::All,
            sounds: Sounds::Standard,
            expiry: ExpirySupport::Expires,
            icons: IconSupport::NamesAndImages,
            buttons: ButtonSupport::Shown,
        }));
        let seq = format!("\x1b]99;i=n:d=0;T\x1b\\\x1b]99;i=n:p=buttons;{labels}\x1b\\");
        t.write_at(baud_domain::BytePos::ZERO, seq.as_bytes());
        t.feed(
            baud_domain::BytePos::ZERO,
            baud_domain::ReadLen::from_index(seq.len()),
        );
        let n = t.notices().next().expect("published");
        n.buttons()
            .map(|(k, b)| (k, String::from_utf8_lossy(b).into_owned()))
            .collect()
    }

    #[test]
    fn a_post_and_its_category_build_without_the_center() {
        let p = Post {
            id: "baud-test-1".into(),
            title: "Build".into(),
            subtitle: "main".into(),
            body: "make; make install".into(),
            sound: PostSound::Silent,
            level: Level::Passive,
            buttons: numbered("Open\u{2028}Skip"),
            image: None,
        };
        let req = request_of(&p, Some("baud-buttons-x")).expect("a request");
        // SAFETY: `UNNotificationRequest`'s `identifier` and `content`,
        // `UNNotificationContent`'s `title`, `subtitle` and
        // `categoryIdentifier`, each nonnull.
        let (id, title, sub, cat) = unsafe {
            let id: Retained<NSString> = msg_send![&req, identifier];
            let content: Retained<AnyObject> = msg_send![&req, content];
            let title: Retained<NSString> = msg_send![&content, title];
            let sub: Retained<NSString> = msg_send![&content, subtitle];
            let cat: Retained<NSString> = msg_send![&content, categoryIdentifier];
            (
                id.to_string(),
                title.to_string(),
                sub.to_string(),
                cat.to_string(),
            )
        };
        assert_eq!(
            (id.as_str(), title.as_str(), sub.as_str(), cat.as_str()),
            ("baud-test-1", "Build", "main", "baud-buttons-x")
        );
        // A subtitle alone, iTerm2's with no title, is a request.
        let sub_only = Post {
            title: String::new(),
            body: String::new(),
            ..p.clone()
        };
        assert!(request_of(&sub_only, None).is_some());
        let loud = Post {
            sound: PostSound::Default,
            level: Level::Active,
            ..p.clone()
        };
        assert!(request_of(&loud, None).is_some());
        // Each action is named by its button's number, here the second
        // `Go` of a list whose middle label is controls alone: `button-3`.
        let gaps = numbered("Go\u{2028}\u{85}\u{2028}Go");
        let shown: Vec<_> = gaps.into_iter().filter(|(_, l)| l != "\u{85}").collect();
        let c = category_object("baud-buttons-x", &shown).expect("a category");
        // SAFETY: `UNNotificationCategory`'s `actions` and each action's
        // `identifier`, nonnull.
        let ids: Vec<String> = unsafe {
            let actions: Retained<NSArray<AnyObject>> = msg_send![&c, actions];
            (0..actions.count())
                .map(|i| {
                    let a: Retained<AnyObject> = msg_send![&actions, objectAtIndex: i];
                    let id: Retained<NSString> = msg_send![&a, identifier];
                    id.to_string()
                })
                .collect()
        };
        assert_eq!(ids, ["button-1", "button-3"]);
        let d = Delegate::new();
        assert!(d.ivars().clicks.lock().expect("unpoisoned").is_empty());
    }

    /// **Outside an app's bundle there is no center**, in every build but
    /// the probe's: the test process is a bare binary, so the bundle check
    /// refuses it before anything is sent to UserNotifications.
    /// Perturbation: drop the check's `return` and the test process asks
    /// the framework for a center it has no bundle for.
    #[cfg(not(feature = "effect-probe"))]
    #[test]
    fn a_bare_binary_has_no_center() {
        assert!(matches!(Center::opened(), Err(Unavailable::NoBundle)));
    }

    /// A post with this identifier.
    #[cfg(feature = "effect-probe")]
    fn post(id: &str) -> Post {
        Post {
            id: id.into(),
            title: "Build".into(),
            subtitle: String::new(),
            body: "done".into(),
            sound: PostSound::Silent,
            level: Level::Passive,
            buttons: Vec::new(),
            image: None,
        }
    }

    /// The kinds recorded since the last call, on this thread.
    #[cfg(feature = "effect-probe")]
    fn kinds() -> Vec<crate::probe::EffectKind> {
        crate::probe::take().iter().map(|r| r.kind).collect()
    }

    /// **Under the probe the answer comes as the system's does, and what
    /// waited for it is sent on the answer's own turn**: the first post
    /// records the ask alone, both posts wait with no answer yet, the news
    /// and a flush before the answer send nothing, and the answer's turn
    /// sends both, in order, with no window's news asked. A window then
    /// hears the answer once; a post goes at once; a removal records one;
    /// and the delivered list is what was sent and not removed, once.
    /// Breaks: the grant given inside `ask` (the second post sends at
    /// once), and the flush left out of the answer's turn (nothing sent).
    #[cfg(feature = "effect-probe")]
    #[test]
    fn under_the_probe_what_waited_is_sent_on_the_answers_turn() {
        use crate::probe::EffectKind;
        // Off the main thread, so a record runs no nested AppKit call.
        assert!(MainThreadMarker::new().is_none(), "off the main thread");
        let _ = kinds();
        let c = Center::opened().expect("a center under the probe");
        c.post_now(post("a"));
        c.post_now(post("b"));
        assert_eq!(kinds(), [EffectKind::Authorize], "asked once, nothing sent");
        assert_eq!(
            c.allowed(),
            Permission::NotAsked,
            "no answer inside the asking call"
        );
        let mut told = Told::NOTHING;
        let news = c.news(&mut told, |_| Whose::This);
        assert_eq!(news.answer, None, "no answer yet");
        c.flush_now();
        assert!(kinds().is_empty(), "nothing sent before the answer");
        c.answered_off_turn(Permission::Granted);
        assert_eq!(
            kinds(),
            [EffectKind::Notice, EffectKind::Notice],
            "what waited, sent on the answer's turn"
        );
        let news = c.news(&mut told, |_| Whose::This);
        assert_eq!(
            (news.answer, news.refused.len()),
            (Some(Permission::Granted), 0)
        );
        assert_eq!(c.news(&mut told, |_| Whose::This).answer, None, "once");
        c.post_now(post("c"));
        assert_eq!(kinds(), [EffectKind::Notice], "granted: sent at once");
        c.remove_now("c");
        assert_eq!(kinds(), [EffectKind::Unnotice]);
        let into = Delivered::default();
        c.ask_delivered_now(&into);
        assert_eq!(
            into.take(),
            Some(vec![String::from("a"), String::from("b")]),
            "what was sent and not removed"
        );
        assert_eq!(into.take(), None, "once");
        assert!(kinds().is_empty(), "asking what is delivered is no effect");
    }

    /// **A post closed while it waits for permission is never sent**: the
    /// removal takes it out of the waiting posts, so the grant finds none
    /// and sends nothing, and the list held is empty. Break: the removal
    /// leaves the waiting post, and the grant sends it.
    #[cfg(feature = "effect-probe")]
    #[test]
    fn a_post_closed_before_the_answer_is_never_sent() {
        use crate::probe::EffectKind;
        let _ = kinds();
        let c = Center::opened().expect("a center under the probe");
        c.post_now(post("a"));
        c.remove_now("a");
        assert_eq!(kinds(), [EffectKind::Authorize, EffectKind::Unnotice]);
        c.answered_off_turn(Permission::Granted);
        assert!(kinds().is_empty(), "nothing sent");
        let into = Delivered::default();
        c.ask_delivered_now(&into);
        assert_eq!(into.take(), Some(Vec::new()));
    }

    /// **A refusal reaches every window, and each window takes only its
    /// own refused posts**: two windows' posts wait, the answer is no, and
    /// each window hears it once -- so each one's core is told none is
    /// delivered -- and takes its own post's identifier; a closed window's
    /// is dropped, and one forgotten at its close is gone. Breaks: the
    /// answer said once for the process (the second window never hears
    /// it), and every window taking every refusal.
    #[cfg(feature = "effect-probe")]
    #[test]
    fn a_refusal_reaches_every_window_and_each_takes_its_own() {
        let _ = kinds();
        let c = Center::opened().expect("a center under the probe");
        for id in ["w1-a", "w2-b", "w3-c", "w4-d"] {
            c.post_now(post(id));
        }
        c.answered_off_turn(Permission::Denied);
        let _ = kinds();
        let of = |w: &'static str| {
            move |id: &str| match id.split('-').next() {
                Some(x) if x == w => Whose::This,
                Some("w3") => Whose::Closed,
                _ => Whose::Another,
            }
        };
        c.forget_refused(|id| id.starts_with("w4-"));
        let (mut one, mut two) = (Told::NOTHING, Told::NOTHING);
        let first = c.news(&mut one, of("w1"));
        assert_eq!(first.answer, Some(Permission::Denied));
        assert_eq!(first.refused, ["w1-a"]);
        let second = c.news(&mut two, of("w2"));
        assert_eq!(
            second.answer,
            Some(Permission::Denied),
            "the second hears it too"
        );
        assert_eq!(second.refused, ["w2-b"], "a closed window's dropped");
        let again = c.news(&mut one, of("w1"));
        assert_eq!((again.answer, again.refused.len()), (None, 0), "once each");
        assert!(c.refused.borrow().is_empty(), "none kept");
        assert!(kinds().is_empty(), "nothing sent");
    }
}
