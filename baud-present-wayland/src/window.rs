//! **A toplevel window on a Wayland compositor, from the registry to a
//! frame and a key.** The handshake is the protocol's: bind the globals
//! (`wl_compositor`, `wl_shm`, `xdg_wm_base`, `wl_seat`), make a surface
//! and its xdg role, commit, and wait for the configure to ack; then a
//! frame is a buffer attached, damaged and committed with a frame callback
//! requested, and the next frame waits for that callback -- one frame in
//! flight, foot's pacing. The keyboard hands over a keymap as a file, which
//! `baud-xkb` compiles, and reports keys and modifier masks the compiler
//! turns into the key events the core encodes. The pointer reports where
//! it is, its buttons and its scrolling a frame at a time (`pointer.rs`),
//! and shows the shape `cursor-shape-v1` names where the compositor offers
//! that global.

use crate::clip::{Board, Clip, Copied, Done, Pasting, Want};
use crate::cursor::{self, Drawn, Scale};
use crate::keys::{self, Shortcut};
use crate::pointer::{
    At, AxisCode, Button, CursorShape, EventCode, Framing, Gather, Piece, PointerEvent, Press,
};
use crate::shm::{Pool, Px};
use baud_domain::{EventMillis, Focus, KeyEvent, KeyEventKind, Modifiers, premultiply};
use baud_wayland::{
    Arg, Connection, Event as Wire, Fixed, Handler, Interface, ObjectId, Opcode, Read, Sender,
    protocol,
};
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
    /// A clipboard key the terminal keeps (`keys::shortcut`): its press,
    /// in place of the key event, and its release goes nowhere.
    Shortcut(Shortcut),
    /// A paste read to its end ([`Window::paste`]).
    Pasted(Board, String),
    /// A paste whose owner sent nothing for `clip::RECEIVE_IDLE`.
    PasteAbandoned(Board),
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

/// Whether the compositor's surfaces take `set_buffer_scale`, which
/// `wl_surface` has from version 3.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Scaling {
    Buffer,
    #[default]
    None,
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
    /// frame is smaller than the window.
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
    seat: Option<ObjectId>,
    /// What the seat's version gives the pointer, which inherits it:
    /// `frame` from version 5, `release` from 3.
    pointer_framing: Framing,
    pointer_release: Release,
    cursor_shape: Option<ObjectId>,
    scaling: Scaling,
}

/// The data device manager's version, at most: 3 is foot's, and what the
/// selection needs is in 1; 3 changes only a drag's rules.
const DATA_DEVICE_VERSION: u32 = 3;

/// How a pointer the seat no longer has is let go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Release {
    /// Version 3 or later: `wl_pointer.release`.
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
    callback: Option<ObjectId>,
    keymap: Option<Keymap>,
    mods: Mods,
    group: usize,
    repeat: RepeatInfo,
    held: Option<Held>,
    /// The key a shortcut took, so its release reaches nobody.
    swallowed: Option<Keycode>,
    /// The serial of the last key, button or keyboard enter: what a copy's
    /// `set_selection` names, "serial number of the event that triggered
    /// this request" (`[[source: wayland]]`). A program's `OSC 52` has no
    /// event of its own and names the last one, as foot names its last
    /// keyboard serial (`[[source: foot-2705e36-osc]]`).
    serial: Option<u32>,
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
            callback: None,
            keymap: None,
            mods: Mods::default(),
            group: 0,
            repeat: RepeatInfo {
                period: None,
                delay: Duration::ZERO,
            },
            held: None,
            swallowed: None,
            serial: None,
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

    /// How the compositor last configured the window.
    pub fn shown(&self) -> Shown {
        self.shown
    }

    /// The compositor's recommended bound on the window's size, the
    /// monitor's less its panels, once it has sent one.
    pub fn bounds(&self) -> Option<(Px, Px)> {
        self.bounds
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

    /// The clipboard shortcut a press of `code` makes under the modifiers in
    /// force (`keys::shortcut`), if any.
    fn shortcut(&self, code: Keycode) -> Option<Shortcut> {
        let km = self.keymap.as_ref()?;
        let mut st = State::new(km);
        st.update(self.mods, self.group);
        keys::shortcut(st.sym(code), st.mods().effective())
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
    /// uses, with a frame callback for the next.
    pub fn present(&mut self, s: &mut Sender<'_>, frame: &[u32], stride: usize) -> Presented {
        let (Some(surface), Some(shm), Some((w, h))) = (self.surface, self.globals.shm, self.size)
        else {
            return Presented::No;
        };
        if self.callback.is_some() {
            return Presented::No;
        }
        let (wu, hu) = (
            usize::try_from(w.0).unwrap_or(0),
            usize::try_from(h.0).unwrap_or(0),
        );
        if stride < wu || frame.len() < stride * hu {
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
                Err(_) => return Presented::No,
            }
        }
        let pool = self.pool.as_mut().expect("just made");
        let dst_stride = pool.stride();
        let Some((buffer, pixels)) = pool.free() else {
            return Presented::No;
        };
        for y in 0..hu {
            let src = &frame[y * stride..y * stride + wu];
            let dst = &mut pixels[y * dst_stride..y * dst_stride + wu];
            for (d, &px) in dst.iter_mut().zip(src) {
                *d = premultiply(px);
            }
        }
        pool.committed(buffer);
        let _ = s.request(
            surface,
            Opcode(protocol::wl_surface::request::ATTACH),
            &[Arg::Object(buffer), Arg::Int(0), Arg::Int(0)],
        );
        let _ = s.request(
            surface,
            Opcode(protocol::wl_surface::request::DAMAGE_BUFFER),
            &[
                Arg::Int(0),
                Arg::Int(0),
                Arg::Int(i32::try_from(w.0).unwrap_or(i32::MAX)),
                Arg::Int(i32::try_from(h.0).unwrap_or(i32::MAX)),
            ],
        );
        let cb = s.new_id(Interface::named("wl_callback").expect("pinned"));
        let _ = s.request(
            surface,
            Opcode(protocol::wl_surface::request::FRAME),
            &[Arg::NewId(cb)],
        );
        let _ = s.request(surface, Opcode(protocol::wl_surface::request::COMMIT), &[]);
        self.callback = Some(cb);
        Presented::Yes
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
                        self.globals.wm_base =
                            Window::bind(s, self.registry, name, iface, version.min(6))
                    }
                    "wl_seat" => {
                        let version = version.min(9);
                        self.globals.pointer_framing = match version >= 5 {
                            true => Framing::Framed,
                            false => Framing::Single,
                        };
                        self.globals.pointer_release = match version >= 3 {
                            true => Release::Request,
                            false => Release::Forget,
                        };
                        self.globals.seat =
                            Window::bind(s, self.registry, name, iface, version.min(9));
                        self.clip.devices(s, self.globals.seat);
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
                if caps & protocol::wl_seat::capability::KEYBOARD != 0 && self.keyboard.is_none() {
                    let kb = s.new_id(Interface::named("wl_keyboard").expect("pinned"));
                    let _ = s.request(
                        ev.object,
                        Opcode(protocol::wl_seat::request::GET_KEYBOARD),
                        &[Arg::NewId(kb)],
                    );
                    self.keyboard = Some(kb);
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
                        if self.globals.pointer_release == Release::Request {
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
                // one is not one this keymap was asked to repeat.
                self.held = None;
                if let Ok(dup) = fd.try_clone() {
                    self.keymap = Window::keymap_from(dup, *size).ok();
                }
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
                self.mods = Mods {
                    depressed: *dep,
                    latched: *lat,
                    locked: *lock,
                };
                self.group = usize::try_from(*group).unwrap_or(0);
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
                            self.swallowed = Some(code);
                            self.held = None;
                            return;
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
                        if self.swallowed == Some(code) {
                            self.swallowed = None;
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
                self.events.push_back(Event::Focus(Focus::Gained));
            }
            // A keyboard that left the surface delivers no release for what
            // it held, so the repeat stops here, and the modifiers go back
            // to none: "In the wl_keyboard logical state, this event resets
            // all values to their defaults" (`[[source: wayland]]`). The
            // pointer reads them outside the keyboard's focus, and the
            // compositor sends a fresh `modifiers` after the next `enter`.
            ("wl_keyboard", "leave", _) => {
                self.held = None;
                self.swallowed = None;
                self.clip.focus_lost(s);
                self.mods = Mods::default();
                self.group = 0;
                self.events.push_back(Event::Focus(Focus::Lost));
            }
            ("wl_buffer", "release", _) => {
                if let Some(p) = self.pool.as_mut() {
                    p.released(ev.object);
                }
            }
            ("wl_callback", "done", _) => {
                if self.sync == Some(ev.object) {
                    self.sync = None;
                    self.create_surface(s);
                }
                if self.callback == Some(ev.object) {
                    self.callback = None;
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
