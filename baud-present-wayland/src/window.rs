//! **A toplevel window on a Wayland compositor, from the registry to a
//! frame and a key.** The handshake is the protocol's: bind the globals
//! (`wl_compositor`, `wl_shm`, `xdg_wm_base`, `wl_seat`), make a surface
//! and its xdg role, commit, and wait for the configure to ack; then a
//! frame is a buffer attached, damaged and committed with a frame callback
//! requested, and the next frame waits for that callback -- one frame in
//! flight, foot's pacing. The keyboard hands over a keymap as a file, which
//! `baud-xkb` compiles, and reports keys and modifier masks the compiler
//! turns into the key events the core encodes.

use crate::keys;
use crate::shm::{Pool, Px};
use baud_domain::{Focus, KeyEvent};
use baud_wayland::{
    Arg, Connection, Event as Wire, Handler, Interface, ObjectId, Opcode, Read, Sender, protocol,
};
use baud_xkb::{Keycode, Keymap, Mods, State};
use std::collections::VecDeque;
use std::io;

/// What the window reports to the app.
#[derive(Debug, PartialEq, Eq)]
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
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presented {
    Yes,
    /// Not configured yet, a frame still in flight, no buffer free, or the
    /// frame is smaller than the window.
    No,
}

#[derive(Default)]
struct Globals {
    compositor: Option<ObjectId>,
    shm: Option<ObjectId>,
    wm_base: Option<ObjectId>,
    seat: Option<ObjectId>,
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
    /// The size the last toplevel configure asked for, or our default.
    pending: (Px, Px),
    size: Option<(Px, Px)>,
    pool: Option<Pool>,
    callback: Option<ObjectId>,
    keymap: Option<Keymap>,
    mods: Mods,
    group: usize,
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
            pending: (Px(800), Px(600)),
            size: None,
            pool: None,
            callback: None,
            keymap: None,
            mods: Mods::default(),
            group: 0,
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
    /// at least the window's size -- premultiplied into a free buffer,
    /// with a frame callback for the next.
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

/// `0xAARRGGBB` straight to premultiplied, the encoding `ARGB8888` names.
fn premultiply(px: u32) -> u32 {
    let a = px >> 24;
    if a == 0xff {
        return px;
    }
    let ch = |shift: u32| ((px >> shift) & 0xff) * a / 255;
    (a << 24) | (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

impl Handler for Window {
    fn event(&mut self, s: &mut Sender<'_>, ev: Wire<'_>) {
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
                        self.globals.compositor =
                            Window::bind(s, self.registry, name, iface, version.min(6))
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
                        self.globals.seat =
                            Window::bind(s, self.registry, name, iface, version.min(9))
                    }
                    _ => {}
                }
            }
            ("xdg_wm_base", "ping", [Read::Uint(serial)]) => {
                let _ = s.request(
                    ev.object,
                    Opcode(protocol::xdg_wm_base::request::PONG),
                    &[Arg::Uint(*serial)],
                );
            }
            // Zero means the client chooses; keep what it had.
            ("xdg_toplevel", "configure", [Read::Int(w), Read::Int(h), Read::Array(_)])
                if *w > 0 && *h > 0 =>
            {
                self.pending = (
                    Px(u32::try_from(*w).unwrap_or(0)),
                    Px(u32::try_from(*h).unwrap_or(0)),
                );
            }
            ("xdg_toplevel", "close", _) => self.events.push_back(Event::Closed),
            ("xdg_surface", "configure", [Read::Uint(serial)]) => {
                let _ = s.request(
                    ev.object,
                    Opcode(protocol::xdg_surface::request::ACK_CONFIGURE),
                    &[Arg::Uint(*serial)],
                );
                if self.size != Some(self.pending) {
                    self.size = Some(self.pending);
                    self.events.push_back(Event::Configured {
                        width: self.pending.0,
                        height: self.pending.1,
                    });
                }
            }
            ("wl_seat", "capabilities", [Read::Uint(caps)])
                if caps & protocol::wl_seat::capability::KEYBOARD != 0
                    && self.keyboard.is_none() =>
            {
                let kb = s.new_id(Interface::named("wl_keyboard").expect("pinned"));
                let _ = s.request(
                    ev.object,
                    Opcode(protocol::wl_seat::request::GET_KEYBOARD),
                    &[Arg::NewId(kb)],
                );
                self.keyboard = Some(kb);
            }
            ("wl_keyboard", "keymap", [Read::Uint(format), Read::Fd(fd), Read::Uint(size)])
                if *format == protocol::wl_keyboard::keymap_format::XKB_V1 =>
            {
                if let Ok(dup) = fd.try_clone() {
                    self.keymap = Window::keymap_from(dup, *size).ok();
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
                    Read::Uint(_),
                    Read::Uint(_),
                    Read::Uint(key),
                    Read::Uint(state),
                ],
            ) if *state == protocol::wl_keyboard::key_state::PRESSED => {
                if let Some(km) = &self.keymap {
                    let mut st = State::new(km);
                    st.update(self.mods, self.group);
                    if let Some(e) = keys::press(&st, Keycode(*key + 8)) {
                        self.events.push_back(Event::Key(e));
                    }
                }
            }
            // The keymap's `enter` carries the keys already down as an array;
            // neither event's payload beyond its meaning is needed here.
            ("wl_keyboard", "enter", _) => self.events.push_back(Event::Focus(Focus::Gained)),
            ("wl_keyboard", "leave", _) => self.events.push_back(Event::Focus(Focus::Lost)),
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
