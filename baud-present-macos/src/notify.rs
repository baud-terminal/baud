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
//!   say what a process with no bundle gets, so [`Center::open`] refuses
//!   one whose main bundle has no identifier, and posts nothing.
//! * **Permission, asked when it is needed.** "you must obtain permission
//!   to use them", and "The first time your app makes this authorization
//!   request, the system prompts the person"; "Make the request in a
//!   context that helps people understand why"
//!   (`[[source: apple-asking-permission-notifications]]`). It is asked at
//!   the first notification a program sends, never at launch, and what is
//!   posted before the answer waits for it.
//! * **The delegate is set first**, as the class's page asks ("Always
//!   assign an object to the delegate property before performing any
//!   tasks"), so a notification shown while baud is active is presented
//!   (`willPresentNotification`) and a click reaches the program
//!   (`didReceiveNotificationResponse`). **One delegate for the process**
//!   ([`shared_delegate`]), however many windows open a [`Center`]: the
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
use std::sync::{Arc, Mutex};

use block2::{Block, RcBlock};
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, NSObjectProtocol};
use macos_liquid_glass::objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, extern_protocol,
    msg_send,
};
use macos_liquid_glass::objc2_app_kit::{NSApplication, NSBezierPath, NSColor, NSView};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{
    NSArray, NSBundle, NSError, NSPoint, NSRect, NSSet, NSSize, NSString, NSURL,
};

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
    fn push(&self, click: Click) {
        if let Ok(mut c) = self.clicks.lock() {
            c.push(click);
        }
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
            crate::unwind::guard("the notification delegate's will-present", (), || {
                handler.call((PRESENT,));
            });
        }

        /// A click, on the notification or one of its actions.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(&self, _center: &AnyObject, response: &AnyObject, handler: &Block<dyn Fn()>) {
            crate::unwind::guard("the notification delegate's click", (), || {
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
                self.ivars().push(Click { id, action });
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

/// Why notifications cannot be posted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unavailable {
    /// Run outside its bundle: no main bundle identifier.
    NoBundle,
    /// The framework's class is not there.
    NoCenter,
}

/// A turn's news: the answer, the first time there is one, and the posts
/// that waited for it and were refused.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Turn {
    pub answer: Option<Permission>,
    pub refused: Vec<String>,
}

impl Turn {
    const NOTHING: Turn = Turn {
        answer: None,
        refused: Vec::new(),
    };
}

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
    /// Its buttons' labels; a click on the `n`th is action `button-n`.
    pub buttons: Vec<String>,
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

/// **The notification center**, with its delegate and what waits for
/// permission.
pub struct Center {
    center: Retained<AnyObject>,
    delegate: Retained<Delegate>,
    permission: Arc<Mutex<Permission>>,
    /// The identifiers Notification Center last said it holds, until taken.
    delivered: Arc<Mutex<Option<Vec<String>>>>,
    asked: Cell<Permission>,
    waiting: RefCell<Vec<Post>>,
    categories: RefCell<Vec<(String, Retained<AnyObject>)>>,
}

impl Center {
    /// The center, the process's delegate set, or why not.
    pub fn open(_mtm: MainThreadMarker) -> Result<Center, Unavailable> {
        // An identifier, and a bundle that is an app's: a binary run on
        // its own has neither, and an `Info.plist` linked into one would
        // give it the first.
        let bundle = NSBundle::mainBundle();
        if bundle.bundleIdentifier().is_none() || !bundle.bundlePath().to_string().ends_with(".app")
        {
            return Err(Unavailable::NoBundle);
        }
        let Some(cls) = AnyClass::get(c"UNUserNotificationCenter") else {
            return Err(Unavailable::NoCenter);
        };
        // SAFETY: `+currentNotificationCenter`, nonnull.
        let center: Retained<AnyObject> = unsafe { msg_send![cls, currentNotificationCenter] };
        let delegate = shared_delegate();
        // SAFETY: `delegate` is `id <UNUserNotificationCenterDelegate>`,
        // held weakly by the center and strongly here and by the process
        // ([`DELEGATE`]); setting the same one again changes nothing.
        unsafe {
            let _: () = msg_send![&center, setDelegate: &*delegate];
        }
        Ok(Center {
            center,
            delegate,
            permission: Arc::new(Mutex::new(Permission::NotAsked)),
            delivered: Arc::new(Mutex::new(None)),
            asked: Cell::new(Permission::NotAsked),
            waiting: RefCell::new(Vec::new()),
            categories: RefCell::new(Vec::new()),
        })
    }

    fn permission(&self) -> Permission {
        self.permission.lock().map_or(Permission::Denied, |p| *p)
    }

    /// **Post one**, once permission is granted: asked now if it never
    /// was, and the post kept until the answer.
    pub fn post(&self, p: Post) {
        match self.permission() {
            Permission::Granted => self.send(&p),
            Permission::Denied => {}
            Permission::NotAsked | Permission::Asked => {
                self.waiting.borrow_mut().push(p);
                self.ask();
            }
        }
    }

    fn ask(&self) {
        if self.asked.get() != Permission::NotAsked {
            return;
        }
        self.asked.set(Permission::Asked);
        let shared = Arc::clone(&self.permission);
        let done = RcBlock::new(move |granted: Bool, _e: *mut NSError| {
            crate::unwind::guard("the notification permission's answer", (), || {
                if let Ok(mut p) = shared.lock() {
                    *p = match granted.as_bool() {
                        true => Permission::Granted,
                        false => Permission::Denied,
                    };
                }
            });
        });
        // SAFETY: `-requestAuthorizationWithOptions:completionHandler:`,
        // the options an `NSUInteger`, the handler `void (^)(BOOL,
        // NSError *)`.
        unsafe {
            let _: () = msg_send![&self.center, requestAuthorizationWithOptions: AUTHORIZE, completionHandler: &*done];
        }
    }

    /// **What came since the last turn**: the posts that waited, sent once
    /// permission came, or dropped when it was refused, their identifiers
    /// given back; and the answer, said once.
    pub fn turn(&self) -> Turn {
        let p = self.permission();
        let waiting: Vec<Post> = match p {
            Permission::Granted | Permission::Denied => {
                self.waiting.borrow_mut().drain(..).collect()
            }
            Permission::NotAsked | Permission::Asked => return Turn::NOTHING,
        };
        let refused = match p {
            Permission::Granted => {
                for post in &waiting {
                    self.send(post);
                }
                Vec::new()
            }
            _ => waiting.into_iter().map(|post| post.id).collect(),
        };
        let first = self.asked.get() == Permission::Asked;
        if first {
            self.asked.set(p);
        }
        Turn {
            answer: first.then_some(p),
            refused,
        }
    }

    /// The permission the system last gave.
    pub fn allowed(&self) -> Permission {
        self.permission()
    }

    /// **Ask which notifications Notification Center holds**: "Fetches all
    /// of your app's delivered notifications that are still present in
    /// Notification Center", answered later through [`Center::delivered`].
    pub fn ask_delivered(&self) {
        let shared = Arc::clone(&self.delivered);
        let done = RcBlock::new(move |list: core::ptr::NonNull<NSArray<AnyObject>>| {
            crate::unwind::guard("the delivered notifications' answer", (), || {
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
            });
        });
        // SAFETY: `-getDeliveredNotificationsWithCompletionHandler:`, the
        // handler `void (^)(NSArray<UNNotification *> *)`.
        unsafe {
            let _: () =
                msg_send![&self.center, getDeliveredNotificationsWithCompletionHandler: &*done];
        }
    }

    /// The answer to the last [`Center::ask_delivered`], once.
    pub fn delivered(&self) -> Option<Vec<String>> {
        self.delivered.lock().ok().and_then(|mut d| d.take())
    }

    /// **The clicks on this window's posts heard since the last call**:
    /// `whose` reads each click's request identifier; another open
    /// window's are left for it, and a closed window's dropped.
    pub fn clicks(&self, whose: impl FnMut(&str) -> Whose) -> Vec<Click> {
        self.delegate.ivars().take(whose)
    }

    /// **Close one**: removed from Notification Center, or before it shows.
    pub fn remove(&self, id: &str) {
        remove(&self.center, id);
    }

    fn send(&self, p: &Post) {
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

    /// The category whose actions are these buttons, registered the first
    /// time they are seen; `None` with none.
    fn category(&self, buttons: &[String]) -> Option<String> {
        if buttons.is_empty() {
            return None;
        }
        let key = format!("baud-buttons-{}", buttons.join("\u{1f}"));
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

/// A category whose actions are `buttons`, `button-1` and on, under `key`.
fn category_object(key: &str, buttons: &[String]) -> Option<Retained<AnyObject>> {
    let action_cls = AnyClass::get(c"UNNotificationAction")?;
    let cat_cls = AnyClass::get(c"UNNotificationCategory")?;
    let actions: Vec<Retained<AnyObject>> = buttons
        .iter()
        .enumerate()
        .map(|(i, label)| {
            // SAFETY: `+actionWithIdentifier:title:options:`, nonnull, no
            // options.
            unsafe {
                msg_send![
                    action_cls,
                    actionWithIdentifier: &*NSString::from_str(&format!("button-{}", i + 1)),
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
fn new_of(name: &core::ffi::CStr) -> Option<Retained<AnyObject>> {
    let cls = AnyClass::get(name)?;
    // SAFETY: `+new`, which every `NSObject` class answers.
    Some(unsafe { msg_send![cls, new] })
}

/// An image file as an attachment, or `None` when the system refuses it.
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

fn remove(center: &AnyObject, id: &str) {
    let ids = NSArray::from_retained_slice(&[NSString::from_str(id)]);
    // SAFETY: the two removals, each an `NSArray<NSString *>`.
    unsafe {
        let _: () = msg_send![center, removeDeliveredNotificationsWithIdentifiers: &*ids];
        let _: () = msg_send![center, removePendingNotificationRequestsWithIdentifiers: &*ids];
    }
}

/// **Close one by its identifier from any turn**: for an expiry scheduled
/// on the main queue, which holds the identifier and not the center.
pub fn remove_by_id(id: &str) {
    let Some(cls) = AnyClass::get(c"UNUserNotificationCenter") else {
        return;
    };
    // SAFETY: `+currentNotificationCenter`, nonnull.
    let center: Retained<AnyObject> = unsafe { msg_send![cls, currentNotificationCenter] };
    remove(&center, id);
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
            crate::unwind::guard("the Dock tile's draw", (), || {
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

/// **The Dock tile shows `bar`**, or the plain icon for `None`.
pub fn show_bar(mtm: MainThreadMarker, bar: Option<Bar>) {
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
/// is already active has no effect". The request's id, to cancel it.
pub fn attention(mtm: MainThreadMarker, ask: Ask) -> AttentionId {
    use macos_liquid_glass::objc2_app_kit::NSRequestUserAttentionType as T;
    AttentionId(
        NSApplication::sharedApplication(mtm).requestUserAttention(match ask {
            Ask::UntilActive => T::CriticalRequest,
            Ask::Once => T::InformationalRequest,
        }),
    )
}

/// A request [`attention`] made, to cancel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttentionId(isize);

/// Cancel a request `attention` made: "no to cancel a previous request".
pub fn cancel_attention(mtm: MainThreadMarker, id: AttentionId) {
    NSApplication::sharedApplication(mtm).cancelUserAttentionRequest(id.0);
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
        b.ivars().push(click("w0-1"));
        let c = shared_delegate();
        assert_eq!(c.ivars().take(|_| Whose::This), vec![click("w0-1")]);
    }

    /// **A click goes to the window that posted it**: each window takes
    /// only its own, in the order heard, another open window's waits for
    /// it, and a closed window's is dropped.
    #[test]
    fn a_click_goes_to_the_window_that_posted_it() {
        let heard = Heard::default();
        for id in ["w0-1", "w1-1", "w9-1", "w0-2"] {
            heard.push(click(id));
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

    #[test]
    fn a_post_and_its_category_build_without_the_center() {
        let p = Post {
            id: "baud-test-1".into(),
            title: "Build".into(),
            subtitle: "main".into(),
            body: "make; make install".into(),
            sound: PostSound::Silent,
            level: Level::Passive,
            buttons: vec!["Open".into(), "Skip".into()],
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
        let c = category_object("baud-buttons-x", &p.buttons).expect("a category");
        // SAFETY: `UNNotificationCategory`'s `actions`, nonnull.
        let n: usize = unsafe {
            let actions: Retained<NSArray<AnyObject>> = msg_send![&c, actions];
            actions.count()
        };
        assert_eq!(n, 2);
        let d = Delegate::new();
        assert!(d.ivars().clicks.lock().expect("unpoisoned").is_empty());
    }
}
