//! **A toplevel window on a Wayland compositor, from the registry to a
//! frame and a key.** The handshake is the protocol's: bind the globals
//! (`wl_compositor`, `wl_shm`, `xdg_wm_base`, `wl_seat`), make a surface
//! and its xdg role, commit, and wait for the configure to ack; then a
//! frame is a buffer attached, damaged in the rows it changed and
//! committed with a frame callback requested, and the next frame waits for
//! that callback -- one frame in flight, foot's pacing. The keyboard hands
//! over a keymap as a file, which `baud-xkb` compiles, and reports keys
//! and modifier masks the compiler
//! turns into the key events the core encodes. The pointer reports where
//! it is, its buttons and its scrolling a frame at a time (`pointer.rs`),
//! and shows the shape `cursor-shape-v1` names where the compositor offers
//! that global.

use crate::clip::{Board, Clip, Copied, Done, Pasting, Want};
use crate::cursor::{self, Drawn, Scale};
use crate::damage::{Changed, RowSet};
use crate::ime::{
    ActionCode, ByteAt, CursorAt, CursorRect, HintCode, Preedit, PreeditCursor, Serial, Step,
    TextInput,
};
use crate::keys::{self, Shortcut};
use crate::pointer::{
    At, AxisCode, Button, CursorShape, EventCode, Framing, Gather, Piece, PointerEvent, Press,
};
use crate::shm::{Pool, Px};
use crate::timing::{ClockId, Presentation, Stamp};
use baud_domain::{
    EventMillis, Focus, HebrewLayout, KeyEvent, KeyEventKind, LocatorDevice, Modifiers, premultiply,
};
use baud_wayland::{
    Arg, Connection, Event as Wire, Fixed, Handler, Interface, ObjectId, Opcode, Read, Sender,
    protocol,
};
use baud_xkb::compose::{Compose, Fed, Status, Table};
use baud_xkb::{Keycode, Keymap, Mods, State};
use std::collections::VecDeque;
use std::io;
use std::os::fd::BorrowedFd;
use std::time::{Duration, Instant};

/// What the window reports to the app.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// The compositor sized the window; a frame of this size can be shown.
    Configured {
        width: Px,
        height: Px,
    },
    /// The last frame reached the screen; the next may be presented.
    Frame,
    Key(KeyEvent),
    /// `wl_keyboard.enter` and `leave`: the surface gained or lost keyboard
    /// focus. The driver hands it to `Term::focus`, which reports it to the
    /// child only if it asked with `?1004`.
    Focus(Focus),
    /// What the pointer did, a `wl_pointer.frame` at a time.
    Pointer(PointerEvent),
    /// `wl_keyboard.modifiers` that changed what is held, read through
    /// [`Window::modifiers`]. A link under a resting pointer is shown
    /// while Ctrl is held, without the pointer moving.
    Modifiers,
    /// A clipboard key the terminal keeps (`keys::shortcut`): its press,
    /// in place of the key event, and its release goes nowhere.
    Shortcut(Shortcut),
    /// A paste read to its end ([`Window::paste`]).
    Pasted(Board, String),
    /// A paste whose owner sent nothing for `clip::RECEIVE_IDLE`.
    PasteAbandoned(Board),
    /// A copy made without the keyboard was cancelled before the keyboard
    /// came back (`clip::Done::Cancelled`).
    CopyCancelled(Board),
    /// What the input method is composing and its hints, drawn at the
    /// cursor until it commits or is cancelled (`ime.rs`); empty when there
    /// is none. The committed text arrives as key events, as a Compose
    /// sequence's does, and a `submit` action as the Return key.
    Preedit(Preedit),
    Closed,
}

/// Whether a pointer shape reached the compositor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shaped {
    /// Named through `cursor-shape-v1`.
    Set,
    /// No cursor-shape global, so drawn here (`cursor.rs`): the I-beam or
    /// the arrow, the nearest of the two.
    Drawn,
    /// No pointer, or the pointer is not over the surface: no `enter`
    /// serial to name; or no buffer could be made for the image.
    Unavailable,
}

/// Whether an activation reached the compositor ([`Window::activate`],
/// [`Window::activate_with`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activated {
    /// A token asked for: `activate` follows its `done`.
    Asked,
    /// A token asked for earlier has not come yet, and its `activate` is
    /// this one's: one token in flight, as foot keeps it, so a burst of
    /// bells does not flood the socket.
    Pending,
    /// `activate` sent with a token another client gave.
    Sent,
    /// The compositor offers no `xdg-activation-v1`, or the window has no
    /// surface yet: nothing sent.
    Unavailable,
}

/// Whether a bell reached the compositor ([`Window::ring_bell`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rung {
    /// Sent through `xdg-system-bell-v1`.
    Sent,
    /// The compositor offers no system bell: nothing sounds.
    NoBell,
}

/// Whether the compositor's surfaces take `set_buffer_scale`, which
/// `wl_surface` has from version 3.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Scaling {
    Buffer,
    #[default]
    None,
}

/// Whether the compositor can send `xdg_toplevel.configure_bounds`, from
/// the `xdg_wm_base` version bound ([`Window::configure_bounds`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConfigureBounds {
    /// Version 4 or later: it may send it.
    Offered,
    /// Below version 4, or nothing bound yet: it never comes.
    #[default]
    Never,
}

/// How the compositor last configured the toplevel, from the states its
/// `xdg_toplevel.configure` carries (`[[source: wp-xdg-shell]]`, the
/// `state` enum): full screen, where "the window geometry specified in the
/// configure event is a maximum"; maximised, where it "must be obeyed by
/// the client"; or neither, where the size is a hint and the client may
/// choose its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Floating,
    Maximized,
    FullScreen,
}

impl Shown {
    /// The states array, "32-bit unsigned integers in native endianness":
    /// full screen wins over maximised, as a full-screen surface keeps the
    /// state it returns to.
    fn of(states: &[u8]) -> Shown {
        let has = |want: u32| {
            states
                .chunks_exact(4)
                .any(|w| w.try_into().ok().map(u32::from_ne_bytes) == Some(want))
        };
        match (
            has(protocol::xdg_toplevel::state::FULLSCREEN),
            has(protocol::xdg_toplevel::state::MAXIMIZED),
        ) {
            (true, _) => Shown::FullScreen,
            (false, true) => Shown::Maximized,
            (false, false) => Shown::Floating,
        }
    }
}

/// Whether a request sets a toplevel state or unsets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Set,
    Unset,
}

/// What a resize the client chose came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resize {
    /// The window takes the size; a `Configured` event follows.
    Taken,
    /// Maximised or full screen, the compositor's size binds; or not yet
    /// configured.
    Bound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presented {
    Yes,
    /// Not configured yet, a frame still in flight, no buffer free, or the
    /// frame is smaller than the window. What it changed is kept, and goes
    /// with the next frame shown.
    No,
}

/// `wl_keyboard.repeat_info`: the time between repeats of a held key, and the
/// wait before the first. **Repeat is the client's job**: the protocol's
/// "repeated" key state is version 10's, and the seat is bound at 9, so no
/// compositor repeats a key for baud. A rate of zero is the compositor turning
/// repeat off. Until the event arrives there is none: the protocol guarantees
/// it before any key press on a keyboard of version 4 or later, and below that
/// the client chooses -- baud chooses no repeat rather than invent a rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RepeatInfo {
    period: Option<Duration>,
    delay: Duration,
}

/// What a press does to a Compose sequence.
enum Composing {
    /// Not part of one: the key is the key.
    Pass,
    /// A step, or the key that cancelled one: nothing is typed.
    Held,
    /// The end: this is typed.
    Typed(String),
}

/// The key held down and repeating, and when it next repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Held {
    key: Keycode,
    next: Instant,
}

#[derive(Default)]
struct Globals {
    compositor: Option<ObjectId>,
    shm: Option<ObjectId>,
    wm_base: Option<ObjectId>,
    /// Whether the `xdg_wm_base` bound has `configure_bounds`.
    configure_bounds: ConfigureBounds,
    seat: Option<ObjectId>,
    /// What the seat's version gives the pointer and the keyboard, which
    /// inherit it: the pointer's `frame` from version 5, and both `release`
    /// requests from 3.
    pointer_framing: Framing,
    device_release: Release,
    cursor_shape: Option<ObjectId>,
    /// `wp_presentation`, where the compositor offers it (`timing.rs`).
    presentation: Option<ObjectId>,
    /// the `xdg-system-bell-v1` global, where the compositor offers it.
    system_bell: Option<ObjectId>,
    /// the `xdg-activation-v1` global, where the compositor offers it.
    activation: Option<ObjectId>,
    scaling: Scaling,
}

/// The longest gap between two frame callbacks read as a frame's length:
/// a longer one is a pause in drawing, not a refresh.
const CADENCE_MAX: Duration = Duration::from_millis(100);

/// The data device manager's version, at most: 3 is foot's, and what the
/// selection needs is in 1; 3 changes only a drag's rules.
const DATA_DEVICE_VERSION: u32 = 3;

/// How a pointer or a keyboard the seat no longer has is let go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Release {
    /// Version 3 or later: `wl_pointer.release` or `wl_keyboard.release`.
    Request,
    /// Below 3 the interface has no request, and the object is dropped.
    #[default]
    Forget,
}

pub struct Window {
    title: String,
    app_id: String,
    registry: ObjectId,
    /// The callback after the registry burst.
    sync: Option<ObjectId>,
    globals: Globals,
    surface: Option<ObjectId>,
    xdg_surface: Option<ObjectId>,
    toplevel: Option<ObjectId>,
    keyboard: Option<ObjectId>,
    pointer: Option<ObjectId>,
    /// The pointer's cursor-shape device, where the global exists.
    shape_device: Option<ObjectId>,
    /// The serial of the pointer's last `enter` on the surface, while it is
    /// over it: what `set_shape` and `set_cursor` must name.
    entered: Option<u32>,
    /// The pointer image drawn here, without the shape global: its surface,
    /// made at the first need, and the two images at the scale they were
    /// drawn at, the I-beam's buffer first.
    cursor_surface: Option<ObjectId>,
    cursor_images: Option<(Scale, Pool, [ObjectId; 2])>,
    /// `wl_surface.preferred_buffer_scale` on the window's surface, the
    /// scale the pointer is drawn at; one until the compositor says.
    preferred: Scale,
    gather: Gather,
    /// The size the last toplevel configure asked for, or our default.
    pending: (Px, Px),
    size: Option<(Px, Px)>,
    /// The states the last toplevel configure carried, and those the last
    /// acknowledged one did.
    pending_shown: Shown,
    shown: Shown,
    /// `xdg_toplevel.configure_bounds`: "the bounds a window geometry size
    /// is recommended to constrain to", such as "the size of a monitor
    /// excluding any panels"; `None` while unknown.
    bounds: Option<(Px, Px)>,
    pool: Option<Pool>,
    /// The rows frames changed that no buffer has shown yet: those of a
    /// frame refused, sent with the next one.
    owed: RowSet,
    /// The rows the last frame shown copied into its buffer.
    copied: Px,
    callback: Option<ObjectId>,
    /// The presentation clock, from `clock_id`, and the last frame the
    /// compositor said it showed (`timing.rs`).
    clock: Option<ClockId>,
    presented: Option<Presentation>,
    /// The time the last frame callback carried, and the interval between
    /// the last two that came within a tenth of a second of each other.
    callback_time: Option<Duration>,
    cadence: Option<Duration>,
    keymap: Option<Keymap>,
    /// Whether the keymap has a Hebrew layout ([`keys::hebrew_layout`]),
    /// read when it arrives.
    hebrew: HebrewLayout,
    mods: Mods,
    group: usize,
    repeat: RepeatInfo,
    held: Option<Held>,
    /// The keys a shortcut or a Compose sequence took, so their releases
    /// reach nobody: more than one, as a dead key may still be down when
    /// the letter after it is pressed.
    swallowed: Vec<Keycode>,
    /// The session's Compose table (`compose.rs`) and where the keys fed so
    /// far stand in it.
    compose: Option<Table>,
    composing: Compose,
    /// The input method's text-input object (`ime.rs`).
    ime: TextInput,
    /// The serial of the last key, button or keyboard enter: what a copy's
    /// `set_selection` names, "serial number of the event that triggered
    /// this request" (`[[source: wayland]]`). A program's `OSC 52` has no
    /// event of its own and names the last one, as foot names its last
    /// keyboard serial (`[[source: foot-2705e36-osc]]`).
    serial: Option<u32>,
    /// The activation token asked for and not yet come ([`Window::activate`]).
    token: Option<ObjectId>,
    clip: Clip,
    events: VecDeque<Event>,
}

impl Window {
    /// Ask for the registry and a sync after it: the globals arrive on the
    /// next dispatch and the window binds what it needs as they do, and
    /// the sync's callback -- which the compositor answers once the burst
    /// is out -- is when it creates its surface.
    pub fn open(
        conn: &mut Connection,
        title: &str,
        app_id: &str,
    ) -> Result<Window, baud_wayland::client::Error> {
        let mut s = conn.sender();
        let registry = s.new_id(Interface::named("wl_registry").expect("pinned"));
        s.request(
            ObjectId::DISPLAY,
            Opcode(protocol::wl_display::request::GET_REGISTRY),
            &[Arg::NewId(registry)],
        )?;
        let sync = s.new_id(Interface::named("wl_callback").expect("pinned"));
        s.request(
            ObjectId::DISPLAY,
            Opcode(protocol::wl_display::request::SYNC),
            &[Arg::NewId(sync)],
        )?;
        Ok(Window {
            sync: Some(sync),
            title: title.to_string(),
            app_id: app_id.to_string(),
            registry,
            globals: Globals::default(),
            surface: None,
            xdg_surface: None,
            toplevel: None,
            keyboard: None,
            pointer: None,
            shape_device: None,
            entered: None,
            cursor_surface: None,
            cursor_images: None,
            preferred: Scale(1),
            gather: Gather::new(Framing::Framed),
            pending: (Px(800), Px(600)),
            size: None,
            pending_shown: Shown::Floating,
            shown: Shown::Floating,
            bounds: None,
            pool: None,
            owed: RowSet::default(),
            copied: Px(0),
            callback: None,
            clock: None,
            presented: None,
            callback_time: None,
            cadence: None,
            keymap: None,
            hebrew: HebrewLayout::Absent,
            mods: Mods::default(),
            group: 0,
            repeat: RepeatInfo {
                period: None,
                delay: Duration::ZERO,
            },
            held: None,
            swallowed: Vec::new(),
            compose: None,
            composing: Compose::default(),
            ime: TextInput::default(),
            serial: None,
            token: None,
            clip: Clip::default(),
            events: VecDeque::new(),
        })
    }

    pub fn next_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    pub fn size(&self) -> Option<(Px, Px)> {
        self.size
    }

    pub fn in_flight(&self) -> bool {
        self.callback.is_some()
    }

    /// Set the window's title, as a program's `OSC 0` or `OSC 2` asks.
    ///
    /// Kept as well as sent, so a title set before the toplevel exists is
    /// the one its role is created with. After that `xdg_toplevel.set_title`
    /// takes effect when the compositor receives it: the pinned xdg-shell
    /// does not double-buffer the title, where it says so of the state it
    /// does. The driver's loop flushes it at the end of the iteration that
    /// set it. Where it is shown is the compositor's: a titlebar only under
    /// server-side decorations, which baud does not negotiate, else a task
    /// bar or window list.
    pub fn set_title(&mut self, s: &mut Sender<'_>, title: &str) {
        self.title = title.to_string();
        if let Some(top) = self.toplevel {
            let _ = s.request(
                top,
                Opcode(protocol::xdg_toplevel::request::SET_TITLE),
                &[Arg::Str(Some(&self.title))],
            );
        }
    }

    /// **The pointer's shape**, a `wp_cursor_shape_device_v1.shape` value
    /// (`crate::pointer::shape`), named with the last `enter`'s serial, as
    /// set_shape requires: "The serial parameter must match the latest
    /// wl_pointer.enter ... serial number sent to the client. Otherwise the
    /// request will be ignored." **Without the global the image is drawn
    /// here** and set with `wl_pointer.set_cursor`, whose serial "must
    /// match the latest wl_pointer.enter serial number" too: the I-beam for
    /// the text shapes and the arrow for the rest (`cursor::Drawn::of`), so
    /// the image is never left undefined.
    pub fn set_pointer_shape(&mut self, s: &mut Sender<'_>, shape: CursorShape) -> Shaped {
        let Some(serial) = self.entered else {
            return Shaped::Unavailable;
        };
        let Some(device) = self.shape_device else {
            return self.draw_pointer(s, Drawn::of(shape));
        };
        match s.request(
            device,
            Opcode(protocol::wp_cursor_shape_device_v1::request::SET_SHAPE),
            &[Arg::Uint(serial), Arg::Uint(shape.0)],
        ) {
            Ok(()) => Shaped::Set,
            Err(_) => Shaped::Unavailable,
        }
    }

    /// **The pointer hidden** while it is over the window: `set_cursor`
    /// with no surface, "If surface is NULL, the pointer image is hidden"
    /// (`[[source: wayland]]`), named with the last `enter`'s serial as the
    /// request requires. Shown again by [`Window::set_pointer_shape`], and
    /// left to the compositor once the pointer leaves.
    pub fn hide_pointer(&mut self, s: &mut Sender<'_>) -> Shaped {
        let (Some(pointer), Some(serial)) = (self.pointer, self.entered) else {
            return Shaped::Unavailable;
        };
        match s.request(
            pointer,
            Opcode(protocol::wl_pointer::request::SET_CURSOR),
            &[
                Arg::Uint(serial),
                Arg::Object(ObjectId::NULL),
                Arg::Int(0),
                Arg::Int(0),
            ],
        ) {
            Ok(()) => Shaped::Set,
            Err(_) => Shaped::Unavailable,
        }
    }

    /// **The pointer drawn here**: the surface made once, the two images
    /// drawn at the window's buffer scale into one pool -- again when the
    /// scale changes -- and the one asked for attached, at that scale, and
    /// given the cursor role with its hotspot in logical pixels. The
    /// images are never written again, so a buffer the compositor holds is
    /// never touched.
    fn draw_pointer(&mut self, s: &mut Sender<'_>, d: Drawn) -> Shaped {
        let (Some(compositor), Some(shm), Some(pointer), Some(serial)) = (
            self.globals.compositor,
            self.globals.shm,
            self.pointer,
            self.entered,
        ) else {
            return Shaped::Unavailable;
        };
        let scale = match self.globals.scaling {
            Scaling::Buffer => self.preferred,
            Scaling::None => Scale(1),
        };
        let surface = match self.cursor_surface {
            Some(c) => c,
            None => {
                let c = s.new_id(Interface::named("wl_surface").expect("pinned"));
                let _ = s.request(
                    compositor,
                    Opcode(protocol::wl_compositor::request::CREATE_SURFACE),
                    &[Arg::NewId(c)],
                );
                self.cursor_surface = Some(c);
                c
            }
        };
        let side = cursor::SIZE * scale.0;
        if self
            .cursor_images
            .as_ref()
            .is_none_or(|(drawn_at, _, _)| *drawn_at != scale)
        {
            if let Some((_, old, _)) = self.cursor_images.take() {
                old.destroy(s);
            }
            let Ok(mut pool) = Pool::new(s, shm, Px(side), Px(side)) else {
                return Shaped::Unavailable;
            };
            let stride = pool.stride();
            let wide = usize::try_from(side).unwrap_or(0);
            let mut ids = [ObjectId::NULL; 2];
            for ((id, pixels), (slot, image)) in pool
                .buffers_mut()
                .zip(ids.iter_mut().zip([Drawn::IBeam, Drawn::Arrow]))
            {
                let drawn = cursor::draw(image, scale);
                for (row, src) in pixels.chunks_mut(stride).zip(drawn.chunks(wide)) {
                    row[..wide].copy_from_slice(src);
                }
                *slot = id;
            }
            self.cursor_images = Some((scale, pool, ids));
        }
        let Some((_, _, ids)) = &self.cursor_images else {
            return Shaped::Unavailable;
        };
        let buffer = match d {
            Drawn::IBeam => ids[0],
            Drawn::Arrow => ids[1],
        };
        let _ = s.request(
            surface,
            Opcode(protocol::wl_surface::request::ATTACH),
            &[Arg::Object(buffer), Arg::Int(0), Arg::Int(0)],
        );
        if self.globals.scaling == Scaling::Buffer {
            let _ = s.request(
                surface,
                Opcode(protocol::wl_surface::request::SET_BUFFER_SCALE),
                &[Arg::Int(i32::try_from(scale.0).unwrap_or(1))],
            );
        }
        let logical = i32::try_from(cursor::SIZE).unwrap_or(0);
        let _ = s.request(
            surface,
            Opcode(protocol::wl_surface::request::DAMAGE),
            &[
                Arg::Int(0),
                Arg::Int(0),
                Arg::Int(logical),
                Arg::Int(logical),
            ],
        );
        let (hx, hy) = d.hotspot();
        let _ = s.request(
            pointer,
            Opcode(protocol::wl_pointer::request::SET_CURSOR),
            &[
                Arg::Uint(serial),
                Arg::Object(surface),
                Arg::Int(hx),
                Arg::Int(hy),
            ],
        );
        let _ = s.request(surface, Opcode(protocol::wl_surface::request::COMMIT), &[]);
        Shaped::Drawn
    }

    /// **`text` becomes `board`'s selection** (`clip.rs`), named with the
    /// serial of the last input event.
    pub fn copy(&mut self, s: &mut Sender<'_>, board: Board, text: &str) -> Copied {
        self.clip.copy(s, board, text, self.serial)
    }

    /// The source this client offers `board`'s selection from, while it
    /// owns it (`clip.rs`).
    pub fn offering(&self, board: Board) -> Option<ObjectId> {
        self.clip.offering(board)
    }

    /// `board` given up if `source` still offers it (`clip.rs`), named
    /// with the serial of the last input event; `None` when it does not.
    pub fn give_up(
        &mut self,
        s: &mut Sender<'_>,
        board: Board,
        source: ObjectId,
    ) -> Option<Copied> {
        self.clip.give_up(s, board, source, self.serial)
    }

    /// `board` emptied, as a program's `OSC 52` clear asks.
    pub fn clear(&mut self, s: &mut Sender<'_>, board: Board) -> Copied {
        self.clip.clear(s, board, self.serial)
    }

    /// Whether the compositor gave the seat a device for `board`.
    pub fn has(&self, board: Board) -> bool {
        self.clip.has(board)
    }

    /// **A paste from `board`**, which arrives as [`Event::Pasted`] once
    /// read -- at once when this client owns the selection.
    pub fn paste(&mut self, s: &mut Sender<'_>, board: Board) -> Pasting {
        let p = self.clip.paste(s, board, Instant::now());
        self.take_done();
        p
    }

    /// The transfers' descriptors, for the driver's wait.
    pub fn transfers(&self) -> impl Iterator<Item = (BorrowedFd<'_>, Want)> {
        self.clip.fds()
    }

    /// When a paste waiting on its owner is abandoned.
    pub fn transfer_deadline(&self) -> Option<Instant> {
        self.clip.deadline()
    }

    /// Every transfer moved as far as its pipe allows (`Clip::pump`); a
    /// paste finished is an [`Event::Pasted`].
    pub fn pump_transfers(&mut self, now: Instant) {
        self.clip.pump(now);
        self.take_done();
    }

    fn take_done(&mut self) {
        for d in self.clip.take_done() {
            self.events.push_back(match d {
                Done::Pasted(b, text) => Event::Pasted(b, text),
                Done::Abandoned(b) => Event::PasteAbandoned(b),
                Done::Cancelled(b) => Event::CopyCancelled(b),
            });
        }
    }

    /// The modifiers held now, as the keyboard's last `modifiers` event
    /// left them: what a mouse report and a selection read beside a button.
    pub fn modifiers(&self) -> Modifiers {
        match self.keymap.as_ref() {
            Some(km) => {
                let mut st = State::new(km);
                st.update(self.mods, self.group);
                keys::modifiers(st.mods().effective())
            }
            None => keys::modifiers(self.mods.depressed | self.mods.latched | self.mods.locked),
        }
    }

    /// Whether the keymap the compositor sent has a Hebrew layout, which
    /// the driver tells the core for DECHEBM.
    pub fn hebrew_layout(&self) -> HebrewLayout {
        self.hebrew
    }

    /// Whether the seat has a pointer: its `capabilities` last named
    /// `pointer`, "The seat has pointer devices" (`[[source: wayland]]`),
    /// which the driver tells the core for `DSR ?55` and the locator.
    pub fn pointer_device(&self) -> LocatorDevice {
        match self.pointer {
            Some(_) => LocatorDevice::Present,
            None => LocatorDevice::Absent,
        }
    }

    /// How the compositor last configured the window.
    pub fn shown(&self) -> Shown {
        self.shown
    }

    /// The compositor's recommended bound on the window's size, the
    /// monitor's less its panels, once it has sent one.
    pub fn bounds(&self) -> Option<(Px, Px)> {
        self.bounds
    }

    /// **Whether `configure_bounds` can ever arrive**: the event is "since
    /// 4" (`[[source: wp-xdg-shell]]`), so an `xdg_wm_base` bound below
    /// version 4 never sends it, and nothing is known before one is bound.
    pub fn configure_bounds(&self) -> ConfigureBounds {
        self.globals.configure_bounds
    }

    /// `xdg_toplevel.set_minimized`, `CSI 2 t`'s request. "There is no way
    /// to know if the surface is currently minimized, nor is there any way
    /// to unset minimization on this surface", so there is no undoing it
    /// from here.
    pub fn minimize(&mut self, s: &mut Sender<'_>) {
        self.toplevel_request(
            s,
            Opcode(protocol::xdg_toplevel::request::SET_MINIMIZED),
            &[],
        );
    }

    /// `xdg_toplevel.set_maximized` or `unset_maximized`; "Whether this
    /// configure actually sets the window maximized is subject to
    /// compositor policies", and the configure that follows says.
    pub fn maximize(&mut self, s: &mut Sender<'_>, t: Toggle) {
        let op = match t {
            Toggle::Set => Opcode(protocol::xdg_toplevel::request::SET_MAXIMIZED),
            Toggle::Unset => Opcode(protocol::xdg_toplevel::request::UNSET_MAXIMIZED),
        };
        self.toplevel_request(s, op, &[]);
    }

    /// `xdg_toplevel.set_fullscreen`, on the output the compositor
    /// chooses ("If this value is NULL, it's up to the compositor"), or
    /// `unset_fullscreen`.
    pub fn full_screen(&mut self, s: &mut Sender<'_>, t: Toggle) {
        match t {
            Toggle::Set => self.toplevel_request(
                s,
                Opcode(protocol::xdg_toplevel::request::SET_FULLSCREEN),
                &[Arg::Object(ObjectId::NULL)],
            ),
            Toggle::Unset => self.toplevel_request(
                s,
                Opcode(protocol::xdg_toplevel::request::UNSET_FULLSCREEN),
                &[],
            ),
        }
    }

    /// **A size the client chooses**, which a floating toplevel may: the
    /// configure's size is then "a hint", and zero means "the client should
    /// decide its own window dimension". Maximised, the size "must be
    /// obeyed", and full screen it "is a maximum", so neither takes one.
    /// Taken, it is the window's size and a `Configured` event says so,
    /// as a compositor's configure would; the next frame is presented at it.
    pub fn resize(&mut self, width: Px, height: Px) -> Resize {
        if self.shown != Shown::Floating || self.size.is_none() {
            return Resize::Bound;
        }
        let size = (Px(width.0.max(1)), Px(height.0.max(1)));
        self.pending = size;
        if self.size != Some(size) {
            self.size = Some(size);
            self.events.push_back(Event::Configured {
                width: size.0,
                height: size.1,
            });
        }
        Resize::Taken
    }

    /// One request on the toplevel, once it exists.
    fn toplevel_request(&mut self, s: &mut Sender<'_>, op: Opcode, args: &[Arg<'_>]) {
        if let Some(top) = self.toplevel {
            let _ = s.request(top, op, args);
        }
    }

    /// When the held key next repeats, for the loop to wait no longer than;
    /// `None` when no key is repeating.
    pub fn repeat_deadline(&self) -> Option<Instant> {
        self.held.map(|h| h.next)
    }

    /// A repeat of the held key for every period elapsed by `now`, each
    /// re-derived from the modifiers in force now, as a keyboard's own repeat
    /// would be: Shift pressed during a held `q` repeats `Q`. **At most one
    /// second's worth**, then the next is a period from `now`: a loop that
    /// was blocked longer does not type a burst the user never saw.
    pub fn tick(&mut self, now: Instant) {
        let (Some(mut held), Some(period)) = (self.held, self.repeat.period) else {
            return;
        };
        let mut owed = Duration::ZERO;
        while held.next <= now {
            if let Some(e) = self.key_event(held.key, KeyEventKind::Repeat) {
                self.events.push_back(Event::Key(e));
            }
            owed += period;
            held.next += period;
            if owed >= Duration::from_secs(1) {
                held.next = now + period;
                break;
            }
        }
        self.held = Some(held);
    }

    /// **The Compose table the keys are fed through**, `None` for none:
    /// a dead key or `Multi_key` then types nothing until the sequence ends.
    pub fn set_compose(&mut self, table: Option<Table>) {
        self.compose = table;
        self.composing.reset();
    }

    /// **What a press of `code` does to a Compose sequence**, fed the
    /// keysym the key produces under the modifiers in force, as foot feeds
    /// `xkb_state_key_get_one_sym()` (`[[source: foot-2705e36-input]]`
    /// 1623-1632). A step or a cancel types nothing -- the cancelling key
    /// is swallowed, "the approach taken by libX11" (`[[source:
    /// xkbcommon-compose-h]]`, and foot's and kitty's, `[[source:
    /// kitty-0-43-1-xkb-glfw]]` 721-737) -- and an end types what the
    /// sequence composed to.
    fn compose_press(&mut self, code: Keycode) -> Composing {
        let (Some(km), Some(table)) = (self.keymap.as_ref(), self.compose.as_ref()) else {
            return Composing::Pass;
        };
        let mut st = State::new(km);
        st.update(self.mods, self.group);
        if self.composing.feed(table, st.sym(code)) == Fed::Ignored {
            return Composing::Pass;
        }
        match self.composing.status(table) {
            Status::Nothing => Composing::Pass,
            Status::Composing | Status::Cancelled => Composing::Held,
            Status::Composed => Composing::Typed(
                self.composing
                    .composed(table)
                    .map(|c| c.utf8())
                    .unwrap_or_default(),
            ),
        }
    }

    /// **Where the cursor is**, for the input method's candidate window:
    /// sent when text input is on and the cell moved (`ime.rs`).
    pub fn set_text_cursor(&mut self, s: &mut Sender<'_>, r: CursorRect) {
        self.ime.set_cursor(s, r);
    }

    /// The clipboard shortcut a press of `code` makes under the modifiers in
    /// force (`keys::shortcut`), if any.
    fn shortcut(&self, code: Keycode) -> Option<Shortcut> {
        let km = self.keymap.as_ref()?;
        let mut st = State::new(km);
        st.update(self.mods, self.group);
        keys::shortcut(km, &st, code)
    }

    /// The key event `code` produces under the modifiers in force, as `kind`;
    /// `None` without a keymap, or for a key that produces none.
    fn key_event(&self, code: Keycode, kind: KeyEventKind) -> Option<KeyEvent> {
        let km = self.keymap.as_ref()?;
        let mut st = State::new(km);
        st.update(self.mods, self.group);
        keys::press(&st, code).map(|e| KeyEvent { kind, ..e })
    }

    fn bind(
        s: &mut Sender<'_>,
        registry: ObjectId,
        name: u32,
        iface: &str,
        version: u32,
    ) -> Option<ObjectId> {
        let id = s.new_id(Interface::named(iface)?);
        s.request(
            registry,
            Opcode(protocol::wl_registry::request::BIND),
            &[
                Arg::Uint(name),
                Arg::NewIdWith {
                    interface: iface,
                    version,
                    id,
                },
            ],
        )
        .ok()?;
        Some(id)
    }

    fn create_surface(&mut self, s: &mut Sender<'_>) {
        let (Some(compositor), Some(wm_base)) = (self.globals.compositor, self.globals.wm_base)
        else {
            return;
        };
        if self.surface.is_some() {
            return;
        }
        let surface = s.new_id(Interface::named("wl_surface").expect("pinned"));
        let _ = s.request(
            compositor,
            Opcode(protocol::wl_compositor::request::CREATE_SURFACE),
            &[Arg::NewId(surface)],
        );
        let xdg = s.new_id(Interface::named("xdg_surface").expect("pinned"));
        let _ = s.request(
            wm_base,
            Opcode(protocol::xdg_wm_base::request::GET_XDG_SURFACE),
            &[Arg::NewId(xdg), Arg::Object(surface)],
        );
        let top = s.new_id(Interface::named("xdg_toplevel").expect("pinned"));
        let _ = s.request(
            xdg,
            Opcode(protocol::xdg_surface::request::GET_TOPLEVEL),
            &[Arg::NewId(top)],
        );
        let _ = s.request(
            top,
            Opcode(protocol::xdg_toplevel::request::SET_TITLE),
            &[Arg::Str(Some(&self.title))],
        );
        let _ = s.request(
            top,
            Opcode(protocol::xdg_toplevel::request::SET_APP_ID),
            &[Arg::Str(Some(&self.app_id))],
        );
        let _ = s.request(surface, Opcode(protocol::wl_surface::request::COMMIT), &[]);
        self.surface = Some(surface);
        self.xdg_surface = Some(xdg);
        self.toplevel = Some(top);
    }

    /// Show `frame` -- `stride` words a row, `0xAARRGGBB` straight alpha,
    /// at least the window's size -- premultiplied into a free buffer, the
    /// encoding `ARGB8888` names, by the function the macOS presenter
    /// uses, with a frame callback for the next. `changed` is what it
    /// changed of the frame before: only those rows, and those the buffer
    /// missed while others were shown, are copied (`damage.rs`), and only
    /// those it changed are damaged, a strip of rows the buffer's width
    /// each. A frame that changed nothing, with nothing owed, attaches no
    /// buffer: the commit carries the frame callback alone, so the pacing
    /// is the same.
    pub fn present(
        &mut self,
        s: &mut Sender<'_>,
        frame: &[u32],
        stride: usize,
        changed: Changed<'_>,
    ) -> Presented {
        let (Some(surface), Some(shm), Some((w, h))) = (self.surface, self.globals.shm, self.size)
        else {
            return Presented::No;
        };
        let (wu, hu) = (
            usize::try_from(w.0).unwrap_or(0),
            usize::try_from(h.0).unwrap_or(0),
        );
        // What this frame changed and what refused frames did: kept
        // until a buffer shows it. A new height is every row.
        let mut rows = core::mem::take(&mut self.owed);
        if rows.height() != h {
            rows = RowSet::all(h);
        }
        rows.add_changed(changed);
        if self.callback.is_some() || stride < wu || frame.len() < stride * hu {
            self.owed = rows;
            return Presented::No;
        }
        if self.pool.as_ref().is_some_and(|p| p.size() != (w, h)) {
            if let Some(p) = self.pool.take() {
                p.destroy(s);
            }
        }
        if self.pool.is_none() {
            match Pool::new(s, shm, w, h) {
                Ok(p) => self.pool = Some(p),
                Err(_) => {
                    self.owed = rows;
                    return Presented::No;
                }
            }
            // The surface shows a buffer of another size, or none.
            rows = RowSet::all(h);
        }
        // The frame callback, and the presentation feedback where the
        // compositor offers it: both for this commit.
        let presentation = self.globals.presentation;
        let cb = |s: &mut Sender<'_>| {
            let cb = s.new_id(Interface::named("wl_callback").expect("pinned"));
            let _ = s.request(
                surface,
                Opcode(protocol::wl_surface::request::FRAME),
                &[Arg::NewId(cb)],
            );
            if let Some(p) = presentation {
                let fb = s.new_id(Interface::named("wp_presentation_feedback").expect("pinned"));
                let _ = s.request(
                    p,
                    Opcode(protocol::wp_presentation::request::FEEDBACK),
                    &[Arg::Object(surface), Arg::NewId(fb)],
                );
            }
            cb
        };
        if rows.is_empty() {
            let callback = cb(s);
            let _ = s.request(surface, Opcode(protocol::wl_surface::request::COMMIT), &[]);
            self.callback = Some(callback);
            self.copied = Px(0);
            self.owed = rows;
            return Presented::Yes;
        }
        let pool = self.pool.as_mut().expect("just made");
        let dst_stride = pool.stride();
        let Some((buffer, stale, pixels)) = pool.free() else {
            self.owed = rows;
            return Presented::No;
        };
        let mut copy = stale.clone();
        copy.union(&rows);
        let at = |p: Px| usize::try_from(p.0).unwrap_or(0);
        for strip in copy.runs() {
            for y in at(strip.top)..at(strip.bottom) {
                let src = &frame[y * stride..y * stride + wu];
                let dst = &mut pixels[y * dst_stride..y * dst_stride + wu];
                for (d, &px) in dst.iter_mut().zip(src) {
                    *d = premultiply(px);
                }
            }
        }
        pool.shown(buffer, &rows);
        pool.committed(buffer);
        let _ = s.request(
            surface,
            Opcode(protocol::wl_surface::request::ATTACH),
            &[Arg::Object(buffer), Arg::Int(0), Arg::Int(0)],
        );
        let int = |v: u32| Arg::Int(i32::try_from(v).unwrap_or(i32::MAX));
        for strip in rows.runs() {
            let _ = s.request(
                surface,
                Opcode(protocol::wl_surface::request::DAMAGE_BUFFER),
                &[
                    Arg::Int(0),
                    int(strip.top.0),
                    int(w.0),
                    int(strip.bottom.0 - strip.top.0),
                ],
            );
        }
        let callback = cb(s);
        let _ = s.request(surface, Opcode(protocol::wl_surface::request::COMMIT), &[]);
        self.callback = Some(callback);
        self.copied = copy.count();
        // Shown: nothing owed, and the set kept for the next frame's.
        rows.clear();
        self.owed = rows;
        Presented::Yes
    }

    /// **When a frame drawn now will first be seen**: the next refresh
    /// after the last frame the compositor said it showed, where it offers
    /// `wp_presentation` and says a refresh, and `now` otherwise -- what
    /// the bell's flash, the cursor's blink and the effects are planned
    /// for, as the macOS app plans for its display link's target.
    pub fn next_shown(&self, now: Instant) -> Instant {
        self.presented
            .and_then(|p| p.next())
            .map_or(now, |t| t.max(now))
    }

    /// **How long a frame is**: the compositor's refresh, where its
    /// presentation feedback says one, else the interval between the last
    /// two frame callbacks that came within a tenth of a second of each
    /// other -- their times are "in milliseconds, with an undefined base"
    /// (`[[source: wayland]]`), so only a difference means anything -- as
    /// the macOS app measures its display link's ticks; `None` before
    /// either is known.
    pub fn frame_interval(&self) -> Option<Duration> {
        self.presented
            .map(|p| p.refresh)
            .filter(|r| !r.is_zero())
            .or(self.cadence)
    }

    /// **Ring the system bell**, through `xdg-system-bell-v1` where the
    /// compositor offers it: "How ringing the bell is implemented is up to
    /// the compositor. It may be an audible sound, a visual feedback of
    /// some kind, or any other thing including nothing", and the surface
    /// named is the toplevel's (`[[source: xdg-system-bell-v1]]`).
    /// Without it nothing sounds.
    pub fn ring_bell(&mut self, s: &mut Sender<'_>) -> Rung {
        let Some(bell) = self.globals.system_bell else {
            return Rung::NoBell;
        };
        let surface = self
            .surface
            .map_or(Arg::Object(ObjectId::NULL), Arg::Object);
        let _ = s.request(
            bell,
            Opcode(protocol::xdg_system_bell_v1::request::RING),
            &[surface],
        );
        Rung::Sent
    }

    /// **Ask the compositor to activate the window**, through
    /// `xdg-activation-v1`: a token asked for with the window's surface as
    /// the one requesting it and no serial, and `activate` on the surface
    /// once the token's `done` brings it -- foot's urgency on a bell
    /// (`[[source: foot-2705e36-wayland]]` 2630-2658, 2830-2860). "It's up
    /// to the compositor to display this information as desired, for
    /// example by placing the surface above the rest", and a token asked
    /// for by a client without the keyboard "may be ineffective ... for
    /// focus stealing prevention" (`[[source: xdg-activation-v1]]`): the
    /// compositor brings the window forward or marks it for attention, as
    /// it decides. One token in flight at a time.
    pub fn activate(&mut self, s: &mut Sender<'_>) -> Activated {
        let (Some(activation), Some(surface)) = (self.globals.activation, self.surface) else {
            return Activated::Unavailable;
        };
        if self.token.is_some() {
            return Activated::Pending;
        }
        let token = s.new_id(Interface::named("xdg_activation_token_v1").expect("pinned"));
        let _ = s.request(
            activation,
            Opcode(protocol::xdg_activation_v1::request::GET_ACTIVATION_TOKEN),
            &[Arg::NewId(token)],
        );
        let _ = s.request(
            token,
            Opcode(protocol::xdg_activation_token_v1::request::SET_SURFACE),
            &[Arg::Object(surface)],
        );
        let _ = s.request(
            token,
            Opcode(protocol::xdg_activation_token_v1::request::COMMIT),
            &[],
        );
        self.token = Some(token);
        Activated::Asked
    }

    /// **Activate the window with a token another client gave**: the one a
    /// notification server's `ActivationToken` signal carries, "an
    /// activation token that can be used to activate a toplevel"
    /// (`[[source: freedesktop-notification-spec]]`), passed to
    /// `activate` on the window's surface (`[[source: xdg-activation-v1]]`).
    pub fn activate_with(&mut self, s: &mut Sender<'_>, token: &str) -> Activated {
        let (Some(activation), Some(surface)) = (self.globals.activation, self.surface) else {
            return Activated::Unavailable;
        };
        let _ = s.request(
            activation,
            Opcode(protocol::xdg_activation_v1::request::ACTIVATE),
            &[Arg::Str(Some(token)), Arg::Object(surface)],
        );
        Activated::Sent
    }

    /// The compositor's last presentation, if it offers feedback and has
    /// shown a frame.
    pub fn presented(&self) -> Option<Presentation> {
        self.presented
    }

    /// The rows the last frame shown copied into its buffer: what it
    /// changed and what the buffer had missed, none when it changed nothing.
    pub fn rows_copied(&self) -> Px {
        self.copied
    }

    /// The pointer's shape device, once both the pointer and the
    /// cursor-shape global exist, whichever came second.
    fn shape_device_for_pointer(&mut self, s: &mut Sender<'_>) {
        let (Some(manager), Some(pointer), None) =
            (self.globals.cursor_shape, self.pointer, self.shape_device)
        else {
            return;
        };
        let device = s.new_id(Interface::named("wp_cursor_shape_device_v1").expect("pinned"));
        let _ = s.request(
            manager,
            Opcode(protocol::wp_cursor_shape_manager_v1::request::GET_POINTER),
            &[Arg::NewId(device), Arg::Object(pointer)],
        );
        self.shape_device = Some(device);
    }

    /// **The keyboard's focus gone**, by its `leave` or with the keyboard
    /// itself: nothing held is released after it, so the repeat stops, and
    /// the modifiers go back to none -- "In the wl_keyboard logical state,
    /// this event resets all values to their defaults" (`[[source:
    /// wayland]]`). The pointer reads them outside the keyboard's focus, and
    /// the compositor sends a fresh `modifiers` after the next `enter`.
    fn keyboard_left(&mut self, s: &mut Sender<'_>) {
        self.held = None;
        self.swallowed.clear();
        self.composing.reset();
        self.clip.focus_lost(s);
        self.mods = Mods::default();
        self.group = 0;
        self.events.push_back(Event::Focus(Focus::Lost));
    }

    /// An enter, leave, motion or button event into the frame.
    fn pointer_event(&mut self, e: PointerEvent) {
        let events = &mut self.events;
        self.gather
            .event(e, &mut |e| events.push_back(Event::Pointer(e)));
    }

    /// An axis event into the frame.
    fn axis(&mut self, axis: AxisCode, piece: Piece) {
        let events = &mut self.events;
        self.gather
            .axis(axis, piece, &mut |e| events.push_back(Event::Pointer(e)));
    }

    /// The keymap the compositor shares as a file of `size` bytes. Read
    /// from offset zero explicitly: the descriptor shares its offset with
    /// the compositor's, which sits wherever it last wrote.
    fn keymap_from(fd: std::os::fd::OwnedFd, size: u32) -> io::Result<Keymap> {
        use std::os::unix::fs::FileExt as _;
        let f = std::fs::File::from(fd);
        let mut text = vec![0u8; usize::try_from(size).unwrap_or(0)];
        let mut at = 0;
        while at < text.len() {
            let n = f.read_at(&mut text[at..], u64::try_from(at).unwrap_or(0))?;
            if n == 0 {
                text.truncate(at);
                break;
            }
            at += n;
        }
        // The compositor's size includes the terminating NUL.
        let end = text.iter().position(|b| *b == 0).unwrap_or(text.len());
        let s = core::str::from_utf8(&text[..end])
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "keymap"))?;
        Keymap::from_text(s).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }
}

/// A surface point from the wire's fixed-point pair.
fn at(x: Fixed, y: Fixed) -> At {
    At {
        x: x.to_f64(),
        y: y.to_f64(),
    }
}

impl Handler for Window {
    fn event(&mut self, s: &mut Sender<'_>, ev: Wire<'_>) {
        if self.clip.event(s, &ev) {
            // A cancel of a copy made without the keyboard is said.
            self.take_done();
            return;
        }
        match (ev.interface.name, ev.name(), ev.args) {
            (
                "wl_registry",
                "global",
                [
                    Read::Uint(name),
                    Read::Str(Some(iface)),
                    Read::Uint(version),
                ],
            ) => {
                let (name, version) = (*name, *version);
                match *iface {
                    "wl_compositor" => {
                        let version = version.min(6);
                        self.globals.scaling = match version >= 3 {
                            true => Scaling::Buffer,
                            false => Scaling::None,
                        };
                        self.globals.compositor =
                            Window::bind(s, self.registry, name, iface, version)
                    }
                    "wl_shm" => {
                        self.globals.shm =
                            Window::bind(s, self.registry, name, iface, version.min(2))
                    }
                    "xdg_wm_base" => {
                        let version = version.min(6);
                        self.globals.wm_base = Window::bind(s, self.registry, name, iface, version);
                        self.globals.configure_bounds = match self.globals.wm_base {
                            Some(_) if version >= 4 => ConfigureBounds::Offered,
                            _ => ConfigureBounds::Never,
                        };
                    }
                    "wl_seat" => {
                        let version = version.min(9);
                        self.globals.pointer_framing = match version >= 5 {
                            true => Framing::Framed,
                            false => Framing::Single,
                        };
                        self.globals.device_release = match version >= 3 {
                            true => Release::Request,
                            false => Release::Forget,
                        };
                        self.globals.seat =
                            Window::bind(s, self.registry, name, iface, version.min(9));
                        self.clip.devices(s, self.globals.seat);
                        self.ime.object_for(s, self.globals.seat);
                    }
                    // Version 2 where the compositor has it: `ime.rs`
                    // sends what version 2 adds only to an object of it.
                    "zwp_text_input_manager_v3" => {
                        let version = version.min(2);
                        self.ime.manager = Window::bind(s, self.registry, name, iface, version);
                        self.ime.version = version;
                        self.ime.object_for(s, self.globals.seat);
                    }
                    "wl_data_device_manager" => {
                        if let Some(m) = Window::bind(
                            s,
                            self.registry,
                            name,
                            iface,
                            version.min(DATA_DEVICE_VERSION),
                        ) {
                            self.clip.bound(Board::Clipboard, m);
                            self.clip.devices(s, self.globals.seat);
                        }
                    }
                    "zwp_primary_selection_device_manager_v1" => {
                        if let Some(m) = Window::bind(s, self.registry, name, iface, version.min(1))
                        {
                            self.clip.bound(Board::Primary, m);
                            self.clip.devices(s, self.globals.seat);
                        }
                    }
                    // Version 2 where offered: what it adds is a refresh
                    // for an output without a constant rate, which
                    // `timing.rs` reads as any refresh.
                    "wp_presentation" => {
                        self.globals.presentation =
                            Window::bind(s, self.registry, name, iface, version.min(2));
                    }
                    "xdg_system_bell_v1" => {
                        self.globals.system_bell =
                            Window::bind(s, self.registry, name, iface, version.min(1));
                    }
                    "xdg_activation_v1" => {
                        self.globals.activation =
                            Window::bind(s, self.registry, name, iface, version.min(1));
                    }
                    // Version 1: its 34 shapes are every one `pointer::shape`
                    // names.
                    "wp_cursor_shape_manager_v1" => {
                        self.globals.cursor_shape =
                            Window::bind(s, self.registry, name, iface, version.min(1));
                        self.shape_device_for_pointer(s);
                    }
                    _ => {}
                }
            }
            // "This event indicates the preferred buffer scale for this
            // surface", from version 6, and "Before receiving this event the
            // preferred buffer scale for this surface is 1". The window
            // draws at one; the pointer it draws itself, at this.
            ("wl_surface", "preferred_buffer_scale", [Read::Int(f)])
                if Some(ev.object) == self.surface =>
            {
                self.preferred = Scale(u32::try_from(*f).unwrap_or(1).max(1));
            }
            ("xdg_wm_base", "ping", [Read::Uint(serial)]) => {
                let _ = s.request(
                    ev.object,
                    Opcode(protocol::xdg_wm_base::request::PONG),
                    &[Arg::Uint(*serial)],
                );
            }
            // Zero means the client chooses; keep what it had. The states
            // are read either way.
            ("xdg_toplevel", "configure", [Read::Int(w), Read::Int(h), Read::Array(states)]) => {
                self.pending_shown = Shown::of(states);
                if *w > 0 && *h > 0 {
                    self.pending = (
                        Px(u32::try_from(*w).unwrap_or(0)),
                        Px(u32::try_from(*h).unwrap_or(0)),
                    );
                }
            }
            // "If width and height are 0, it means bounds is unknown".
            ("xdg_toplevel", "configure_bounds", [Read::Int(w), Read::Int(h)]) => {
                self.bounds = match (u32::try_from(*w), u32::try_from(*h)) {
                    (Ok(w), Ok(h)) if w > 0 && h > 0 => Some((Px(w), Px(h))),
                    _ => None,
                };
            }
            ("xdg_toplevel", "close", _) => self.events.push_back(Event::Closed),
            ("xdg_surface", "configure", [Read::Uint(serial)]) => {
                let _ = s.request(
                    ev.object,
                    Opcode(protocol::xdg_surface::request::ACK_CONFIGURE),
                    &[Arg::Uint(*serial)],
                );
                self.shown = self.pending_shown;
                if self.size != Some(self.pending) {
                    self.size = Some(self.pending);
                    self.events.push_back(Event::Configured {
                        width: self.pending.0,
                        height: self.pending.1,
                    });
                }
            }
            ("wl_seat", "capabilities", [Read::Uint(caps)]) => {
                let keyboard = caps & protocol::wl_seat::capability::KEYBOARD != 0;
                match (keyboard, self.keyboard) {
                    (true, None) => {
                        let kb = s.new_id(Interface::named("wl_keyboard").expect("pinned"));
                        let _ = s.request(
                            ev.object,
                            Opcode(protocol::wl_seat::request::GET_KEYBOARD),
                            &[Arg::NewId(kb)],
                        );
                        self.keyboard = Some(kb);
                    }
                    // The seat lost its keyboard: released where its
                    // version has the request, as the pointer is below;
                    // "no further ... events will be received" on it, so a
                    // keyboard that comes back is asked for anew. A focused
                    // one takes the focus with it.
                    (false, Some(kb)) => {
                        if self.globals.device_release == Release::Request {
                            let _ =
                                s.request(kb, Opcode(protocol::wl_keyboard::request::RELEASE), &[]);
                        }
                        self.keyboard = None;
                        if self.clip.keyboard() == crate::clip::Keyboard::Here {
                            self.keyboard_left(s);
                        }
                    }
                    _ => {}
                }
                let pointer = caps & protocol::wl_seat::capability::POINTER != 0;
                match (pointer, self.pointer) {
                    (true, None) => {
                        let p = s.new_id(Interface::named("wl_pointer").expect("pinned"));
                        let _ = s.request(
                            ev.object,
                            Opcode(protocol::wl_seat::request::GET_POINTER),
                            &[Arg::NewId(p)],
                        );
                        self.pointer = Some(p);
                        self.gather = Gather::new(self.globals.pointer_framing);
                        self.shape_device_for_pointer(s);
                    }
                    // The seat lost its pointer: "When the pointer capability
                    // is removed from the wl_seat, the
                    // wp_cursor_shape_device_v1 object becomes inert", and the
                    // pointer is released where its version has the request.
                    (false, Some(p)) => {
                        if let Some(d) = self.shape_device.take() {
                            let _ = s.request(
                                d,
                                Opcode(protocol::wp_cursor_shape_device_v1::request::DESTROY),
                                &[],
                            );
                        }
                        if self.globals.device_release == Release::Request {
                            let _ =
                                s.request(p, Opcode(protocol::wl_pointer::request::RELEASE), &[]);
                        }
                        self.pointer = None;
                        if self.entered.take().is_some() {
                            self.events.push_back(Event::Pointer(PointerEvent::Left));
                        }
                    }
                    _ => {}
                }
            }
            // The pointer, gathered a frame at a time (`pointer.rs`).
            ("wl_pointer", "enter", [Read::Uint(serial), _, Read::Fixed(x), Read::Fixed(y)]) => {
                self.entered = Some(*serial);
                let e = PointerEvent::Entered(at(*x, *y));
                self.pointer_event(e);
            }
            ("wl_pointer", "leave", _) => {
                self.entered = None;
                self.pointer_event(PointerEvent::Left);
            }
            ("wl_pointer", "motion", [_, Read::Fixed(x), Read::Fixed(y)]) => {
                self.pointer_event(PointerEvent::Moved(at(*x, *y)));
            }
            (
                "wl_pointer",
                "button",
                [
                    Read::Uint(serial),
                    Read::Uint(time),
                    Read::Uint(button),
                    Read::Uint(state),
                ],
            ) => {
                self.serial = Some(*serial);
                let press = match *state {
                    protocol::wl_pointer::button_state::PRESSED => Press::Pressed,
                    _ => Press::Released,
                };
                // A press of the primary button asks for an on-screen
                // keyboard while text input is on (`ime.rs`).
                if press == Press::Pressed && Button::of(EventCode(*button)) == Button::Left {
                    self.ime.pressed(s);
                }
                self.pointer_event(PointerEvent::Button {
                    button: Button::of(EventCode(*button)),
                    press,
                    time: EventMillis::of_millis(*time),
                });
            }
            ("wl_pointer", "axis", [_, Read::Uint(axis), Read::Fixed(v)]) => {
                self.axis(AxisCode(*axis), Piece::Length(v.to_f64()));
            }
            ("wl_pointer", "axis_value120", [Read::Uint(axis), Read::Int(v)]) => {
                self.axis(AxisCode(*axis), Piece::Value120(*v));
            }
            ("wl_pointer", "axis_discrete", [Read::Uint(axis), Read::Int(d)]) => {
                self.axis(AxisCode(*axis), Piece::Discrete(*d));
            }
            ("wl_pointer", "axis_stop", [_, Read::Uint(axis)]) => {
                self.axis(AxisCode(*axis), Piece::Stop);
            }
            ("wl_pointer", "frame", _) => {
                let events = &mut self.events;
                self.gather
                    .frame(&mut |e| events.push_back(Event::Pointer(e)));
            }
            ("wl_keyboard", "keymap", [Read::Uint(format), Read::Fd(fd), Read::Uint(size)])
                if *format == protocol::wl_keyboard::keymap_format::XKB_V1 =>
            {
                // A new keymap names keys afresh; a key held under the old
                // one is not one this keymap was asked to repeat, and a
                // sequence begun under it is not this keymap's.
                self.held = None;
                self.composing.reset();
                if let Ok(dup) = fd.try_clone() {
                    self.keymap = Window::keymap_from(dup, *size).ok();
                }
                self.hebrew = match self.keymap.as_ref() {
                    Some(km) => keys::hebrew_layout(km),
                    None => HebrewLayout::Absent,
                };
            }
            ("wl_keyboard", "repeat_info", [Read::Int(rate), Read::Int(delay)]) => {
                self.repeat = RepeatInfo {
                    period: u32::try_from(*rate)
                        .ok()
                        .filter(|&r| r > 0)
                        .map(|r| Duration::from_secs(1) / r),
                    delay: Duration::from_millis(u64::try_from(*delay).unwrap_or(0)),
                };
                if self.repeat.period.is_none() {
                    self.held = None;
                }
            }
            (
                "wl_keyboard",
                "modifiers",
                [
                    Read::Uint(_),
                    Read::Uint(dep),
                    Read::Uint(lat),
                    Read::Uint(lock),
                    Read::Uint(group),
                ],
            ) => {
                let was = self.modifiers();
                self.mods = Mods {
                    depressed: *dep,
                    latched: *lat,
                    locked: *lock,
                };
                self.group = usize::try_from(*group).unwrap_or(0);
                // Only a change of what is held: a compositor sends the
                // event after every key, most of them changing nothing.
                if self.modifiers() != was {
                    self.events.push_back(Event::Modifiers);
                }
            }
            (
                "wl_keyboard",
                "key",
                [
                    Read::Uint(serial),
                    Read::Uint(_),
                    Read::Uint(key),
                    Read::Uint(state),
                ],
            ) => {
                self.serial = Some(*serial);
                let code = Keycode(*key + 8);
                match *state {
                    protocol::wl_keyboard::key_state::PRESSED => {
                        // A clipboard key is the terminal's: no key event,
                        // no repeat, and its release swallowed.
                        if let Some(sc) = self.shortcut(code) {
                            self.events.push_back(Event::Shortcut(sc));
                            self.swallowed.push(code);
                            self.held = None;
                            self.composing.reset();
                            return;
                        }
                        // A Compose sequence takes the key: no key event,
                        // no repeat, and its release swallowed; at its end
                        // the composed text is typed, a character at a
                        // time as the macOS app types an input manager's.
                        match self.compose_press(code) {
                            Composing::Pass => {}
                            Composing::Held => {
                                self.swallowed.push(code);
                                self.held = None;
                                return;
                            }
                            Composing::Typed(text) => {
                                for c in text.chars() {
                                    self.events.push_back(Event::Key(keys::typed(c)));
                                }
                                self.swallowed.push(code);
                                self.held = None;
                                return;
                            }
                        }
                        if let Some(e) = self.key_event(code, KeyEventKind::Press) {
                            self.events.push_back(Event::Key(e));
                        }
                        // A press takes the repeat over, whatever was held;
                        // a key the keymap does not repeat -- a modifier --
                        // leaves nothing repeating.
                        let repeats = self.keymap.as_ref().is_some_and(|km| {
                            let mut st = State::new(km);
                            st.update(self.mods, self.group);
                            st.repeats(code)
                        });
                        self.held = match (repeats, self.repeat.period) {
                            (true, Some(_)) => Some(Held {
                                key: code,
                                next: Instant::now() + self.repeat.delay,
                            }),
                            _ => None,
                        };
                    }
                    // Delivered as a release, which the core sends only
                    // where kitty's event types asked for one.
                    protocol::wl_keyboard::key_state::RELEASED => {
                        if self.held.is_some_and(|h| h.key == code) {
                            self.held = None;
                        }
                        if let Some(i) = self.swallowed.iter().position(|&k| k == code) {
                            self.swallowed.swap_remove(i);
                            return;
                        }
                        if let Some(e) = self.key_event(code, KeyEventKind::Release) {
                            self.events.push_back(Event::Key(e));
                        }
                    }
                    _ => {}
                }
            }
            // The keymap's `enter` carries the keys already down as an array;
            // neither event's payload beyond its meaning is needed here.
            ("wl_keyboard", "enter", [Read::Uint(serial), ..]) => {
                self.serial = Some(*serial);
                self.clip.focus_gained();
                self.events.push_back(Event::Focus(Focus::Gained));
            }
            ("wl_keyboard", "leave", _) => self.keyboard_left(s),
            // The input method (`ime.rs`): enabled on the object's own
            // enter on this surface, its state applied on `done`.
            ("zwp_text_input_v3", "enter", [Read::Object(surface)])
                if self.ime.is(ev.object) && Some(*surface) == self.surface =>
            {
                self.ime.entered(s);
            }
            ("zwp_text_input_v3", "leave", _) if self.ime.is(ev.object) => {
                if let Some(p) = self.ime.left() {
                    self.events.push_back(Event::Preedit(p));
                }
            }
            (
                "zwp_text_input_v3",
                "preedit_string",
                [Read::Str(text), Read::Int(begin), Read::Int(end)],
            ) if self.ime.is(ev.object) => {
                let cursor = PreeditCursor {
                    begin: CursorAt(*begin),
                    end: CursorAt(*end),
                };
                self.ime.preedit_string(*text, cursor);
            }
            (
                "zwp_text_input_v3",
                "preedit_hint",
                [Read::Uint(start), Read::Uint(end), Read::Uint(hint)],
            ) if self.ime.is(ev.object) => {
                self.ime
                    .preedit_hint(ByteAt(*start), ByteAt(*end), HintCode(*hint));
            }
            ("zwp_text_input_v3", "action", [Read::Uint(action), _]) if self.ime.is(ev.object) => {
                self.ime.action(ActionCode(*action));
            }
            // Read and not acted on (`ime.rs`): the input method's language,
            // and a deletion from surrounding text the terminal never sent.
            ("zwp_text_input_v3", "language" | "delete_surrounding_text", _)
                if self.ime.is(ev.object) => {}
            ("zwp_text_input_v3", "commit_string", [Read::Str(text)]) if self.ime.is(ev.object) => {
                self.ime.commit_string(*text);
            }
            ("zwp_text_input_v3", "done", [Read::Uint(serial)]) if self.ime.is(ev.object) => {
                for step in self.ime.done(s, Serial(*serial)) {
                    match step {
                        Step::Preedit(p) => self.events.push_back(Event::Preedit(p)),
                        Step::Commit(t) => {
                            for c in t.chars() {
                                self.events.push_back(Event::Key(keys::typed(c)));
                            }
                        }
                        Step::Submit => self.events.push_back(Event::Key(KeyEvent::press(
                            baud_domain::KeyCode::Enter,
                            baud_domain::Modifiers::NONE,
                        ))),
                    }
                }
            }
            ("wl_buffer", "release", _) => {
                if let Some(p) = self.pool.as_mut() {
                    p.released(ev.object);
                }
            }
            ("wp_presentation", "clock_id", [Read::Uint(id)]) => {
                self.clock = i32::try_from(*id).ok().map(ClockId);
            }
            // "presented" and "discarded" each end their feedback object;
            // a frame discarded leaves the last one shown the latest.
            (
                "wp_presentation_feedback",
                "presented",
                [
                    Read::Uint(hi),
                    Read::Uint(lo),
                    Read::Uint(nanos),
                    Read::Uint(refresh),
                    ..,
                ],
            ) => {
                let secs = u64::from(*hi) << 32 | u64::from(*lo);
                if let (Some(clock), true) = (self.clock, *nanos < 1_000_000_000) {
                    self.presented = Some(Presentation {
                        clock,
                        at: Stamp(Duration::new(secs, *nanos)),
                        refresh: Duration::from_nanos(u64::from(*refresh)),
                    });
                }
            }
            // The token asked for: `activate` with it, and the token's
            // object let go -- "The received token stays valid".
            ("xdg_activation_token_v1", "done", [Read::Str(Some(token))])
                if self.token == Some(ev.object) =>
            {
                self.token = None;
                if let (Some(activation), Some(surface)) = (self.globals.activation, self.surface) {
                    let _ = s.request(
                        activation,
                        Opcode(protocol::xdg_activation_v1::request::ACTIVATE),
                        &[Arg::Str(Some(token)), Arg::Object(surface)],
                    );
                }
                let _ = s.request(
                    ev.object,
                    Opcode(protocol::xdg_activation_token_v1::request::DESTROY),
                    &[],
                );
            }
            ("wl_callback", "done", args) => {
                if self.sync == Some(ev.object) {
                    self.sync = None;
                    self.create_surface(s);
                }
                if self.callback == Some(ev.object) {
                    self.callback = None;
                    if let [Read::Uint(ms)] = args {
                        let at = Duration::from_millis(u64::from(*ms));
                        let gap = self.callback_time.and_then(|t| at.checked_sub(t));
                        if let Some(g) = gap.filter(|g| !g.is_zero() && *g < CADENCE_MAX) {
                            self.cadence = Some(g);
                        }
                        self.callback_time = Some(at);
                    }
                    self.events.push_back(Event::Frame);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The buffer word is the straight word premultiplied, rounded**, as
    /// the macOS presenter's is: at every channel value `c` and alpha `a`,
    /// in each of R, G and B with other values beside it, the channel is
    /// the integer nearest `c a / 255` and the alpha passes through.
    /// `wl_shm`'s formats are premultiplied (`[[source: wayland]]`,
    /// `wl_shm.format`); one rounding on both systems is the owner's same
    /// pixels on every OS. It holds `premultiply` as imported here, the
    /// function `present` calls per pixel; a `present` that skipped it
    /// would not be caught by this test.
    #[test]
    fn the_buffer_word_rounds_as_on_macos() {
        let mut wrong = 0u32;
        for a in 0..=255u32 {
            for c in 0..=255u32 {
                let ok = [16u32, 8, 0].iter().all(|&shift| {
                    let rest = 0x5a_c3_96 & !(0xff << shift);
                    let got = premultiply((a << 24) | rest | (c << shift));
                    let ch = (got >> shift) & 0xff;
                    got >> 24 == a && (255 * ch).abs_diff(c * a) <= 127
                });
                if !ok {
                    wrong += 1;
                }
            }
        }
        assert_eq!(wrong, 0, "(C, a) pairs off the nearest, of 65536");
    }
}
