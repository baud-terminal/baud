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
//!
//! **Where the compositor offers the pacing globals, a frame keeps to the
//! screen's refresh** and the frame callback still paces it, as fifo-v1
//! asks: "Clients must use an additional mechanism such as frame callbacks
//! or timestamps to ensure throttling occurs under all conditions"
//! (`[[source: wp-fifo-v1]]`). Each new picture is held on screen a whole
//! refresh before the next replaces it, through fifo-v1's barrier on every
//! commit that attaches a buffer; a picture drawn for a moment is held
//! until that moment's refresh through `commit-timing-v1` (`timing.rs`);
//! and with tearing turned on (`[window] tearing`) the surface asks
//! `tearing-control-v1` for "minimal latency and tearing is acceptable",
//! and carries neither. Where a global is not offered, nothing of it is
//! sent.

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
use crate::scale::{self, BufferScale, Mapped, Told, Viewport};
use crate::shm::{Pool, Px};
use crate::timing::{ClockId, Presentation, Stamp, Unseen};
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
    /// The compositor sized the window, or the scale it is drawn at
    /// changed: the surface's size in its own points, and a frame of
    /// [`Window::buffer_size`] pixels, at [`Window::scale`], can be shown.
    /// One a dispatch: a later change replaces one not yet read, so a
    /// scale that arrives as `preferred_buffer_scale` and then as a
    /// fraction is laid out once, at the fraction.
    Configured {
        width: Px,
        height: Px,
    },
    /// The last frame reached the screen; the next may be presented.
    Frame,
    /// A key pressed, repeated or released. **A release names the key its
    /// press named and comes only for a press this window sent**: kitty's
    /// layer gives a release the press's key, shifted and alternate keys,
    /// "we want the same key value" (`[[source:
    /// kitty-glfw-input-c-6d5d0c4]]`, `_glfwInputKeyboard`), so Shift
    /// pressed while `q` is held lets go of `q`, not `Q`; and a key already
    /// down when the keyboard entered, a shortcut's or a Compose
    /// sequence's, releases nothing.
    Key(KeyEvent),
    /// A character a Compose sequence or the input method typed, or the
    /// input method's submit: a key press of no key, which no release
    /// follows.
    Typed(KeyEvent),
    /// `wl_keyboard.enter` and `leave`: the surface gained or lost keyboard
    /// focus. The driver hands it to `Term::focus`, which reports it to the
    /// child only if it asked with `?1004`.
    Focus(Focus),
    /// What the pointer did, a `wl_pointer.frame` at a time, with the
    /// modifiers held when its frame came: a report, a selection and a
    /// link click read these, not [`Window::modifiers`], which is what is
    /// held once every queued event has been read -- Ctrl let go in the
    /// same read as a Ctrl-click would otherwise take Ctrl from the click.
    Pointer(PointerEvent, Modifiers),
    /// `wl_keyboard.modifiers` that changed what is held, and what is held
    /// from then on. A link under a resting pointer is shown while Ctrl is
    /// held, without the pointer moving.
    Modifiers(Modifiers),
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
    /// No cursor-shape global, so drawn here (`cursor.rs`): the I-beam, the
    /// pointing hand or the arrow, the nearest of the three.
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

/// **Whether the window may be shown torn**, `[window] tearing`: `On`
/// asks `tearing-control-v1` for the `async` hint -- "presented with
/// minimal latency and tearing is acceptable" -- and `Off`, the protocol's
/// default `vsync`, "synchronized to the vertical blanking period"
/// (`[[source: wp-tearing-control-v1]]`). "The compositor is free to
/// dynamically respect or ignore this hint".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tearing {
    On,
    #[default]
    Off,
}

/// Whether a commit attaches a new picture, which fifo-v1's barrier holds
/// on screen a refresh, or carries the frame callback alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Picture {
    New,
    Same,
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
/// `wl_surface` has from version 3, and how the scale to draw at is
/// learnt: from version 6 the compositor says it,
/// `wl_surface.preferred_buffer_scale`; before, "It is intended that you
/// pick the same buffer scale as the scale of the output that the surface
/// is displayed on" (`set_buffer_scale`, `[[source: wayland]]`), the
/// outputs the surface has entered and each one's `wl_output.scale`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Scaling {
    /// Version 6 or later: `preferred_buffer_scale`.
    Preferred,
    /// Versions 3 to 5: the outputs' scales.
    Outputs,
    #[default]
    None,
}

/// A `wl_output` bound, by its registry name, and the scale it last said:
/// one until it says, "If it is not sent, the client should assume a scale
/// of 1". Its current mode's width and height, once one has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Output {
    name: GlobalName,
    id: ObjectId,
    scale: Scale,
    mode: Option<(Px, Px)>,
}

/// **An output's size in pixels and its scale** ([`Window::outputs`]):
/// its current `wl_output.mode`, "in physical hardware units of the
/// output device", and its `wl_output.scale`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputPx {
    pub width: Px,
    pub height: Px,
    pub scale: u32,
}

/// A global's numeric name in the registry, which `global_remove` names
/// it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GlobalName(u32);

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
                .as_chunks::<4>()
                .0
                .iter()
                .any(|w| u32::from_ne_bytes(*w) == want)
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
    /// The pacing globals, where the compositor offers them: fifo-v1's and
    /// commit-timing-v1's managers, and tearing-control-v1's.
    fifo: Option<ObjectId>,
    commit_timing: Option<ObjectId>,
    tearing: Option<ObjectId>,
    /// the `xdg-system-bell-v1` global, where the compositor offers it.
    system_bell: Option<ObjectId>,
    /// the `xdg-activation-v1` global, where the compositor offers it.
    activation: Option<ObjectId>,
    scaling: Scaling,
    /// Every `wl_output`, bound at version 2 for its `scale` event, which
    /// the window and the pointer's image are drawn at below
    /// `wl_compositor` version 6.
    outputs: Vec<Output>,
    /// The viewporter and fractional-scale-v1's manager, where the
    /// compositor offers them: a fractional scale is drawn only with both
    /// (`scale.rs`).
    viewporter: Option<ObjectId>,
    fractional: Option<ObjectId>,
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
    /// The callback after the outputs' binds, asked for when the registry's
    /// comes and any output was bound: their `mode`, `scale` and `done`
    /// precede its `done`.
    outputs_sync: Option<ObjectId>,
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
    /// made at the first need, its viewport, made at the first fraction --
    /// a second for one surface is `viewport_exists` -- the three images
    /// at the scale they were drawn at, in `cursor::Drawn::ALL`'s order, in
    /// two pools of two buffers, the second's last one unused, and the
    /// image shown while the pointer is over the window and not hidden.
    cursor_surface: Option<ObjectId>,
    cursor_viewport: Option<ObjectId>,
    cursor_images: Option<(BufferScale, [Pool; 2], [ObjectId; 3])>,
    cursor_shown: Option<Drawn>,
    /// `wl_surface.preferred_buffer_scale` on the window's surface, the
    /// integer scale the window and the pointer are drawn at from version
    /// 6; one until the compositor says.
    preferred: Scale,
    /// The outputs the window's surface is on, from `wl_surface.enter` and
    /// `leave`: below version 6, the window and the pointer are drawn at
    /// the largest of their scales.
    on: Vec<ObjectId>,
    /// The surface's viewport and fractional-scale object, each made once
    /// both globals and the surface exist -- a second of either for one
    /// surface is a protocol error, `viewport_exists` and
    /// `fractional_scale_exists` -- and the last `preferred_scale` it
    /// carried, in 120ths; `None` until the compositor sends one.
    viewport: Option<ObjectId>,
    fractional_scale: Option<ObjectId>,
    fraction: Option<BufferScale>,
    /// The buffer scale and the viewport's destination the last commit
    /// that attached a buffer carried, which the next changes only with a
    /// buffer of the size they make (`present`).
    told: Told,
    /// The surface size and scale the last [`Event::Configured`] said.
    announced: Option<((Px, Px), BufferScale)>,
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
    /// The feedback object of the last frame committed, until the
    /// compositor says it showed or discarded it, and the refresh it could
    /// first reach where that was known when it was committed: a frame
    /// that holds a refresh (`timing.rs`).
    unseen: Option<(ObjectId, Option<Stamp>)>,
    /// The surface's fifo-v1 object, commit timer and tearing control,
    /// each made once, "Only one such object may exist for a surface";
    /// the tearing control only once tearing is turned on.
    fifo: Option<ObjectId>,
    timer: Option<ObjectId>,
    tearing_control: Option<ObjectId>,
    tearing: Tearing,
    /// The moment the next frame presented is drawn for
    /// ([`Window::draw_for`]), taken by that frame.
    drawn_for: Option<Instant>,
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
    /// The keys down whose press was sent as a [`Event::Key`], and that
    /// press: what each one's release names.
    down: Vec<(Keycode, KeyEvent)>,
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
            outputs_sync: None,
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
            cursor_viewport: None,
            cursor_images: None,
            cursor_shown: None,
            preferred: Scale(1),
            on: Vec::new(),
            viewport: None,
            fractional_scale: None,
            fraction: None,
            told: Told::FRESH,
            announced: None,
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
            unseen: None,
            fifo: None,
            timer: None,
            tearing_control: None,
            tearing: Tearing::Off,
            drawn_for: None,
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
            down: Vec::new(),
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

    /// Whether an event is waiting for [`Window::next_event`]: one read
    /// before a caller's wait, as at launch, which the socket will not
    /// wake it for.
    pub fn has_events(&self) -> bool {
        !self.events.is_empty()
    }

    /// The surface's size in its own points, as the compositor configured
    /// it or [`Window::resize`] chose it.
    pub fn size(&self) -> Option<(Px, Px)> {
        self.size
    }

    /// **The scale a frame is drawn at** (`scale.rs`): the fraction the
    /// compositor prefers where it and a viewport are offered, else the
    /// integer scale the surface's version learns, one until it is told.
    pub fn scale(&self) -> BufferScale {
        self.mapping().0
    }

    /// The frame a present must hand over, in pixels: the surface's size
    /// at [`Window::scale`], rounded halfway away from zero.
    pub fn buffer_size(&self) -> Option<(Px, Px)> {
        let s = self.scale();
        self.size.map(|(w, h)| (s.buffer(w), s.buffer(h)))
    }

    /// The scale in force and how a commit tells it.
    fn mapping(&self) -> (BufferScale, Mapped) {
        let viewport = match self.viewport {
            Some(_) => Viewport::Made,
            None => Viewport::None,
        };
        scale::chosen(self.fraction, viewport, self.whole_scale())
    }

    /// **The size or the scale changed**: an [`Event::Configured`], unless
    /// both are what the last one said, in place of one not yet read.
    fn announce(&mut self) {
        let Some(size) = self.size else {
            return;
        };
        let now = (size, self.scale());
        if self.announced == Some(now) {
            return;
        }
        self.announced = Some(now);
        self.events
            .retain(|e| !matches!(e, Event::Configured { .. }));
        self.events.push_back(Event::Configured {
            width: size.0,
            height: size.1,
        });
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
    /// the text shapes, the hand for `pointer` and the arrow for the rest
    /// (`cursor::Drawn::of`), so the image is never left undefined.
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
            Ok(()) => {
                self.cursor_shown = None;
                Shaped::Set
            }
            Err(_) => Shaped::Unavailable,
        }
    }

    /// **The integer scale the surface's version learns**, which the
    /// window and the pointer are drawn at where no fraction is in force:
    /// the compositor's preferred scale from version 6, the largest scale
    /// of the outputs the window is on before it, and one where surfaces
    /// take no buffer scale.
    fn whole_scale(&self) -> Scale {
        match self.globals.scaling {
            Scaling::Preferred => self.preferred,
            Scaling::Outputs => self
                .globals
                .outputs
                .iter()
                .filter(|o| self.on.contains(&o.id))
                .map(|o| o.scale)
                .max_by_key(|s| s.0)
                .unwrap_or(Scale(1)),
            Scaling::None => Scale(1),
        }
    }

    /// **The pointer drawn here, at the window's scale and told as the
    /// window's is** (`scale.rs`): the surface made once, the three images
    /// drawn at the scale into two pools -- again when the scale changes --
    /// and the one asked for attached and given the cursor role with its
    /// hotspot in surface points, "in surface-local coordinates"
    /// (`wl_pointer.set_cursor`, `[[source: wayland]]`). A fraction is
    /// drawn the way fractional-scale-v1 has a client draw a surface: the
    /// image the 24 points times the scale, 36 pixels at 1.5, "The
    /// wl_surface buffer scale should remain set to 1", and the cursor's
    /// own viewport told the 24 points (`[[source:
    /// wp-fractional-scale-v1]]`), so the compositor shows the pixels as
    /// drawn rather than shrinking an image drawn at the integer scale
    /// beside the fraction. A fraction once in force stays in force
    /// (`scale::chosen`), so the destination is never unset. The images
    /// are never written again, so a buffer the compositor holds is never
    /// touched.
    fn draw_pointer(&mut self, s: &mut Sender<'_>, d: Drawn) -> Shaped {
        let (Some(compositor), Some(shm), Some(pointer), Some(serial)) = (
            self.globals.compositor,
            self.globals.shm,
            self.pointer,
            self.entered,
        ) else {
            return Shaped::Unavailable;
        };
        let (scale, mapped) = self.mapping();
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
        let side = scale.buffer(Px(cursor::SIZE)).0;
        if self
            .cursor_images
            .as_ref()
            .is_none_or(|(drawn_at, _, _)| *drawn_at != scale)
        {
            if let Some((_, old, _)) = self.cursor_images.take() {
                for pool in old {
                    pool.destroy(s);
                }
            }
            let first = Pool::new(s, shm, Px(side), Px(side));
            let second = Pool::new(s, shm, Px(side), Px(side));
            let (mut first, mut second) = match (first, second) {
                (Ok(a), Ok(b)) => (a, b),
                (a, b) => {
                    for pool in [a, b].into_iter().flatten() {
                        pool.destroy(s);
                    }
                    return Shaped::Unavailable;
                }
            };
            let stride = first.stride();
            let wide = usize::try_from(side).unwrap_or(0);
            let mut ids = [ObjectId::NULL; 3];
            for ((id, pixels), (slot, image)) in first
                .buffers_mut()
                .chain(second.buffers_mut())
                .zip(ids.iter_mut().zip(Drawn::ALL))
            {
                let drawn = cursor::draw(image, scale);
                for (row, src) in pixels.chunks_mut(stride).zip(drawn.chunks(wide)) {
                    row[..wide].copy_from_slice(src);
                }
                *slot = id;
            }
            self.cursor_images = Some((scale, [first, second], ids));
        }
        let Some((_, _, ids)) = &self.cursor_images else {
            return Shaped::Unavailable;
        };
        // In `Drawn::ALL`'s order, as the buffers were filled.
        let buffer = match d {
            Drawn::IBeam => ids[0],
            Drawn::Arrow => ids[1],
            Drawn::Hand => ids[2],
        };
        let _ = s.request(
            surface,
            Opcode(protocol::wl_surface::request::ATTACH),
            &[Arg::Object(buffer), Arg::Int(0), Arg::Int(0)],
        );
        let logical = i32::try_from(cursor::SIZE).unwrap_or(0);
        let told = Told::of(mapped, (Px(cursor::SIZE), Px(cursor::SIZE)));
        if self.globals.scaling != Scaling::None {
            let _ = s.request(
                surface,
                Opcode(protocol::wl_surface::request::SET_BUFFER_SCALE),
                &[Arg::Int(i32::try_from(told.buffer_scale.0).unwrap_or(1))],
            );
        }
        if told.destination.is_some() {
            if let Some(viewport) = self.cursor_viewport(s, surface) {
                let _ = s.request(
                    viewport,
                    Opcode(protocol::wp_viewport::request::SET_DESTINATION),
                    &[Arg::Int(logical), Arg::Int(logical)],
                );
            }
        }
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
        self.cursor_shown = Some(d);
        Shaped::Drawn
    }

    /// The drawn pointer's viewport, made at its first need where the
    /// viewporter is offered.
    fn cursor_viewport(&mut self, s: &mut Sender<'_>, surface: ObjectId) -> Option<ObjectId> {
        if self.cursor_viewport.is_none() {
            let viewporter = self.globals.viewporter?;
            let id = s.new_id(Interface::named("wp_viewport").expect("pinned"));
            let _ = s.request(
                viewporter,
                Opcode(protocol::wp_viewporter::request::GET_VIEWPORT),
                &[Arg::NewId(id), Arg::Object(surface)],
            );
            self.cursor_viewport = Some(id);
        }
        self.cursor_viewport
    }

    /// **A shown pointer follows the window's scale**: drawn here and over
    /// the window, its image is drawn again when the scale the window is
    /// drawn at changes -- an output entered or left, a new preferred
    /// scale -- as the frame is, rather than when the pointer next comes
    /// back or the next shape is set. A hidden pointer stays hidden.
    fn follow_scale(&mut self, s: &mut Sender<'_>) {
        let Some(d) = self.cursor_shown else {
            return;
        };
        if self
            .cursor_images
            .as_ref()
            .is_some_and(|(drawn_at, _, _)| *drawn_at != self.scale())
        {
            let _ = self.draw_pointer(s, d);
        }
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
    /// left them. A queued pointer event carries the ones held when it
    /// came ([`Event::Pointer`]), which a report and a selection read.
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

    /// **Whether the outputs offered at launch have said their modes**: the
    /// registry's burst read and, where it offered an output, the round
    /// trip after the binds answered. With no output offered nothing more
    /// is asked, and this is so once the registry is read.
    pub fn outputs_settled(&self) -> bool {
        self.sync.is_none() && self.outputs_sync.is_none()
    }

    /// The outputs whose current mode has been said.
    pub fn outputs(&self) -> impl Iterator<Item = OutputPx> + '_ {
        self.globals.outputs.iter().filter_map(|o| {
            o.mode.map(|(width, height)| OutputPx {
                width,
                height,
                scale: o.scale.0,
            })
        })
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

    /// **A size the client chooses**, in the surface's points, which a
    /// floating toplevel may: the
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
        self.size = Some(size);
        self.announce();
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
    /// would be: Shift pressed during a held `q` repeats the `q` key with
    /// Shift, its text `Q`. **At most one second's worth**, then the next
    /// is a period from `now`: a loop that was blocked longer does not type
    /// a burst the user never saw.
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
        // The pacing objects whose globals are already bound are made
        // before the first commit, so every content update can carry them;
        // a global bound later, and the tearing control, get theirs when
        // they come (surface_objects).
        self.surface = Some(surface);
        self.surface_objects(s);
        let _ = s.request(surface, Opcode(protocol::wl_surface::request::COMMIT), &[]);
        self.xdg_surface = Some(xdg);
        self.toplevel = Some(top);
    }

    /// **The surface's pacing and scale objects**, each once both its
    /// global and the surface exist, whichever came second: a second
    /// `get_fifo`, `get_timer`, `get_tearing_control`, `get_viewport` or
    /// `get_fractional_scale` for one surface is a protocol error. The
    /// tearing control is made only while tearing is on, and asks for the
    /// `async` hint, which "is double-buffered" and so goes with the next
    /// commit. The viewport and the fractional-scale object are made
    /// together, once both globals are bound, as foot makes them
    /// (`[[source: foot-2705e36-wayland]]`, `wayl_win_init`): the fraction
    /// "the compositor suggests" is drawn only through the viewport.
    fn surface_objects(&mut self, s: &mut Sender<'_>) {
        let Some(surface) = self.surface else {
            return;
        };
        if let (Some(v), Some(f), None) = (
            self.globals.viewporter,
            self.globals.fractional,
            self.viewport,
        ) {
            let id = s.new_id(Interface::named("wp_viewport").expect("pinned"));
            let _ = s.request(
                v,
                Opcode(protocol::wp_viewporter::request::GET_VIEWPORT),
                &[Arg::NewId(id), Arg::Object(surface)],
            );
            self.viewport = Some(id);
            let id = s.new_id(Interface::named("wp_fractional_scale_v1").expect("pinned"));
            let _ = s.request(
                f,
                Opcode(protocol::wp_fractional_scale_manager_v1::request::GET_FRACTIONAL_SCALE),
                &[Arg::NewId(id), Arg::Object(surface)],
            );
            self.fractional_scale = Some(id);
        }
        if let (Some(m), None) = (self.globals.fifo, self.fifo) {
            let id = s.new_id(Interface::named("wp_fifo_v1").expect("pinned"));
            let _ = s.request(
                m,
                Opcode(protocol::wp_fifo_manager_v1::request::GET_FIFO),
                &[Arg::NewId(id), Arg::Object(surface)],
            );
            self.fifo = Some(id);
        }
        if let (Some(m), None) = (self.globals.commit_timing, self.timer) {
            let id = s.new_id(Interface::named("wp_commit_timer_v1").expect("pinned"));
            let _ = s.request(
                m,
                Opcode(protocol::wp_commit_timing_manager_v1::request::GET_TIMER),
                &[Arg::NewId(id), Arg::Object(surface)],
            );
            self.timer = Some(id);
        }
        if let (Tearing::On, Some(m), None) =
            (self.tearing, self.globals.tearing, self.tearing_control)
        {
            let id = s.new_id(Interface::named("wp_tearing_control_v1").expect("pinned"));
            let _ = s.request(
                m,
                Opcode(protocol::wp_tearing_control_manager_v1::request::GET_TEARING_CONTROL),
                &[Arg::NewId(id), Arg::Object(surface)],
            );
            self.tearing_control = Some(id);
            self.hint(s);
        }
    }

    /// The tearing control's hint, as the setting says.
    fn hint(&self, s: &mut Sender<'_>) {
        use protocol::wp_tearing_control_v1::presentation_hint;
        let Some(control) = self.tearing_control else {
            return;
        };
        let hint = match self.tearing {
            Tearing::On => presentation_hint::ASYNC,
            Tearing::Off => presentation_hint::VSYNC,
        };
        let _ = s.request(
            control,
            Opcode(protocol::wp_tearing_control_v1::request::SET_PRESENTATION_HINT),
            &[Arg::Uint(hint)],
        );
    }

    /// **Tearing turned on or off** (`[window] tearing`), at once: the hint
    /// goes with the next commit. Off before tearing was ever on sends
    /// nothing, the protocol's default being `vsync`.
    pub fn set_tearing(&mut self, s: &mut Sender<'_>, t: Tearing) {
        if t == self.tearing {
            return;
        }
        self.tearing = t;
        match self.tearing_control {
            Some(_) => self.hint(s),
            None => self.surface_objects(s),
        }
    }

    /// Whether the compositor has been asked to show the window torn:
    /// tearing on and its global offered.
    fn torn(&self) -> bool {
        self.tearing == Tearing::On && self.tearing_control.is_some()
    }

    /// **The next frame presented is drawn for `at`**, the moment
    /// [`Window::next_shown`] gave: its picture -- a trail, a slide, a fade
    /// -- is a function of that moment, so where the compositor offers
    /// `commit-timing-v1` it is held until that moment's refresh
    /// (`timing.rs`). A frame without one is shown as soon as the
    /// compositor can. Taken by the next [`Window::present`], whatever it
    /// returns.
    pub fn draw_for(&mut self, at: Instant) {
        self.drawn_for = Some(at);
    }

    /// **What a commit carries to keep to the refresh**, before it, unless
    /// the window is shown torn, which asks for neither: a new picture
    /// fifo-v1's barrier, set and waited on, so it "will be active for at
    /// least one refresh cycle" and waits for the one before to have been;
    /// a frame drawn for a moment the commit timer's timestamp
    /// (`timing.rs`), at most one a commit -- a second is a protocol
    /// error. Nothing where the globals are not offered, or the compositor
    /// has said no refresh. The stamp sent, if one was.
    fn pace(
        &self,
        s: &mut Sender<'_>,
        picture: Picture,
        drawn_for: Option<Instant>,
    ) -> Option<Stamp> {
        if self.torn() {
            return None;
        }
        if let (Picture::New, Some(fifo)) = (picture, self.fifo) {
            let _ = s.request(
                fifo,
                Opcode(protocol::wp_fifo_v1::request::SET_BARRIER),
                &[],
            );
            let _ = s.request(
                fifo,
                Opcode(protocol::wp_fifo_v1::request::WAIT_BARRIER),
                &[],
            );
        }
        let stamp = match (self.timer, drawn_for, self.presented) {
            (Some(timer), Some(at), Some(p)) => crate::sys::clock_now(p.clock)
                .and_then(|clock| p.stamp_for(at, clock, Instant::now()))
                .map(|t| (timer, t)),
            _ => None,
        };
        let (timer, t) = stamp?;
        let [hi, lo, nanos] = t.wire();
        let _ = s.request(
            timer,
            Opcode(protocol::wp_commit_timer_v1::request::SET_TIMESTAMP),
            &[Arg::Uint(hi), Arg::Uint(lo), Arg::Uint(nanos)],
        );
        Some(t)
    }

    /// Whether the last frame committed is still unseen (`timing.rs`).
    fn unseen(&self) -> Unseen {
        match self.unseen {
            Some((_, held)) => Unseen::Frame(held),
            None => Unseen::None,
        }
    }

    /// **The frame being committed, as it will be unseen**: its feedback
    /// object and the refresh it holds -- the first it can reach past the
    /// frame before, or the one its stamp holds it to where that is later
    /// -- read before it replaces the frame before. Nothing without
    /// feedback to say when it is seen.
    fn held(
        &self,
        feedback: Option<ObjectId>,
        stamp: Option<Stamp>,
    ) -> Option<(ObjectId, Option<Stamp>)> {
        let fb = feedback?;
        let first = self.presented.and_then(|p| {
            let clock = crate::sys::clock_now(p.clock)?;
            let next = p.refresh_after(clock, self.unseen())?;
            let stamped = stamp.map(|t| Stamp(t.0.saturating_add(p.refresh / 2)));
            Some(stamped.map_or(next, |t| t.max(next)))
        });
        Some((fb, first))
    }

    /// Show `frame` -- `stride` words a row, `0xAARRGGBB` straight alpha,
    /// at least [`Window::buffer_size`] -- premultiplied into a free buffer, the
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
        let drawn_for = self.drawn_for.take();
        let (Some(surface), Some(shm), Some(size)) = (self.surface, self.globals.shm, self.size)
        else {
            return Presented::No;
        };
        // The buffer is the surface at the scale in force, and what the
        // commit tells of the scale goes with it.
        let (scale, mapped) = self.mapping();
        let (w, h) = (scale.buffer(size.0), scale.buffer(size.1));
        let told = Told::of(mapped, size);
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
        // A buffer scale or a destination changes only with a buffer of
        // the size it makes (`tell`): the whole frame goes with it.
        if told != self.told {
            rows = RowSet::all(h);
        }
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
            let fb = presentation.map(|p| {
                let fb = s.new_id(Interface::named("wp_presentation_feedback").expect("pinned"));
                let _ = s.request(
                    p,
                    Opcode(protocol::wp_presentation::request::FEEDBACK),
                    &[Arg::Object(surface), Arg::NewId(fb)],
                );
                fb
            });
            (cb, fb)
        };
        if rows.is_empty() {
            let (callback, feedback) = cb(s);
            let stamp = self.pace(s, Picture::Same, drawn_for);
            self.unseen = self.held(feedback, stamp);
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
        self.tell(s, surface, told);
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
        let (callback, feedback) = cb(s);
        let stamp = self.pace(s, Picture::New, drawn_for);
        self.unseen = self.held(feedback, stamp);
        let _ = s.request(surface, Opcode(protocol::wl_surface::request::COMMIT), &[]);
        self.callback = Some(callback);
        self.copied = copy.count();
        // Shown: nothing owed, and the set kept for the next frame's.
        rows.clear();
        self.owed = rows;
        Presented::Yes
    }

    /// **The scale a commit carries, with the buffer whose size it makes**:
    /// the buffer scale and the viewport's destination, each sent only
    /// when it changes, and only in a commit that attaches a buffer --
    /// "at commit time the supplied buffer size must be an integer
    /// multiple of the buffer_scale. If that's not the case, an
    /// invalid_size error is sent" (`attach`, `[[source: wayland]]`). Both
    /// are double-buffered, so they apply with the buffer. A fraction
    /// keeps the buffer scale at one and sets the destination to the
    /// surface size; an integer scale unsets a destination set before,
    /// "If width is -1 and height is -1, the destination size is unset"
    /// (`[[source: wp-viewporter]]`).
    fn tell(&mut self, s: &mut Sender<'_>, surface: ObjectId, told: Told) {
        let int = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        if told.buffer_scale != self.told.buffer_scale {
            let _ = s.request(
                surface,
                Opcode(protocol::wl_surface::request::SET_BUFFER_SCALE),
                &[Arg::Int(int(told.buffer_scale.0))],
            );
        }
        if told.destination != self.told.destination {
            if let Some(viewport) = self.viewport {
                let (w, h) = told
                    .destination
                    .map_or((-1, -1), |(w, h)| (int(w.0), int(h.0)));
                let _ = s.request(
                    viewport,
                    Opcode(protocol::wp_viewport::request::SET_DESTINATION),
                    &[Arg::Int(w), Arg::Int(h)],
                );
            }
        }
        self.told = told;
    }

    /// **When a frame drawn now will first be seen**: the next refresh
    /// after the last frame the compositor said it showed -- while the
    /// frame committed since is unseen, past the refresh that one holds --
    /// where it offers `wp_presentation` and says a refresh, and `now`
    /// otherwise -- what the bell's flash, the cursor's blink and the
    /// effects are planned for, as the macOS app plans for its display
    /// link's target.
    pub fn next_shown(&self, now: Instant) -> Instant {
        let unseen = self.unseen();
        self.presented
            .and_then(|p| p.next(unseen))
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
        self.down.clear();
        self.composing.reset();
        self.clip.focus_lost(s);
        self.mods = Mods::default();
        self.group = 0;
        self.events.push_back(Event::Focus(Focus::Lost));
    }

    /// An enter, leave, motion or button event into the frame.
    fn pointer_event(&mut self, e: PointerEvent) {
        let held = self.modifiers();
        let events = &mut self.events;
        self.gather
            .event(e, &mut |e| events.push_back(Event::Pointer(e, held)));
    }

    /// An axis event into the frame.
    fn axis(&mut self, axis: AxisCode, piece: Piece) {
        let held = self.modifiers();
        let events = &mut self.events;
        self.gather.axis(axis, piece, &mut |e| {
            events.push_back(Event::Pointer(e, held))
        });
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
                        self.globals.scaling = match version {
                            6.. => Scaling::Preferred,
                            3.. => Scaling::Outputs,
                            _ => Scaling::None,
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
                    // Version 1 of each, the only one: fifo-v1's barrier,
                    // commit-timing-v1's timer and tearing-control-v1's
                    // hint, each a surface's once its surface exists.
                    "wp_fifo_manager_v1" => {
                        self.globals.fifo = Window::bind(s, self.registry, name, iface, 1);
                        self.surface_objects(s);
                    }
                    "wp_commit_timing_manager_v1" => {
                        self.globals.commit_timing = Window::bind(s, self.registry, name, iface, 1);
                        self.surface_objects(s);
                    }
                    "wp_tearing_control_manager_v1" => {
                        self.globals.tearing = Window::bind(s, self.registry, name, iface, 1);
                        self.surface_objects(s);
                    }
                    "xdg_system_bell_v1" => {
                        self.globals.system_bell =
                            Window::bind(s, self.registry, name, iface, version.min(1));
                    }
                    "xdg_activation_v1" => {
                        self.globals.activation =
                            Window::bind(s, self.registry, name, iface, version.min(1));
                    }
                    // An output's scale, which a compositor older than
                    // version 6 leaves the pointer's image to: `wl_output`
                    // has `scale` from version 2.
                    "wl_output" if version >= 2 => {
                        if let Some(id) = Window::bind(s, self.registry, name, iface, 2) {
                            self.globals.outputs.push(Output {
                                name: GlobalName(name),
                                id,
                                scale: Scale(1),
                                mode: None,
                            });
                        }
                    }
                    // Version 1 of each, the only one: a fractional scale
                    // is drawn through the viewport, both made for the
                    // surface once both are bound (`scale.rs`).
                    "wp_viewporter" => {
                        self.globals.viewporter = Window::bind(s, self.registry, name, iface, 1);
                        self.surface_objects(s);
                    }
                    "wp_fractional_scale_manager_v1" => {
                        self.globals.fractional = Window::bind(s, self.registry, name, iface, 1);
                        self.surface_objects(s);
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
            // preferred buffer scale for this surface is 1". The window and
            // the pointer it draws itself are drawn at it while no fraction
            // is in force.
            ("wl_surface", "preferred_buffer_scale", [Read::Int(f)])
                if Some(ev.object) == self.surface =>
            {
                self.preferred = Scale(u32::try_from(*f).unwrap_or(1).max(1));
                self.announce();
            }
            // "The sent scale is the numerator of a fraction with a
            // denominator of 120" (`[[source: wp-fractional-scale-v1]]`).
            ("wp_fractional_scale_v1", "preferred_scale", [Read::Uint(f)])
                if Some(ev.object) == self.fractional_scale && *f > 0 =>
            {
                self.fraction = Some(BufferScale(*f));
                self.announce();
            }
            ("wl_surface", "enter", [Read::Object(o)])
                if Some(ev.object) == self.surface && !self.on.contains(o) =>
            {
                self.on.push(*o);
                self.announce();
            }
            ("wl_surface", "leave", [Read::Object(o)]) if Some(ev.object) == self.surface => {
                self.on.retain(|on| on != o);
                self.announce();
            }
            // "The compositor will emit a non-zero, positive value for
            // scale."
            ("wl_output", "scale", [Read::Int(f)]) => {
                if let Some(o) = self.globals.outputs.iter_mut().find(|o| o.id == ev.object) {
                    o.scale = Scale(u32::try_from(*f).unwrap_or(1).max(1));
                }
                self.announce();
            }
            // "The current mode is always the last mode that was received
            // with the current flag set" (0x1); any other is a mode the
            // output could take.
            ("wl_output", "mode", [Read::Uint(flags), Read::Int(w), Read::Int(h), _])
                if flags & 0x1 != 0 =>
            {
                if let Some(o) = self.globals.outputs.iter_mut().find(|o| o.id == ev.object) {
                    let px = |v: &i32| Px(u32::try_from(*v).unwrap_or(0));
                    o.mode = Some((px(w), px(h)));
                }
            }
            // An output unplugged: its scale counts no more.
            ("wl_registry", "global_remove", [Read::Uint(name)]) => {
                let name = GlobalName(*name);
                if let Some(at) = self.globals.outputs.iter().position(|o| o.name == name) {
                    let gone = self.globals.outputs.remove(at);
                    self.on.retain(|on| *on != gone.id);
                }
                self.announce();
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
                self.size = Some(self.pending);
                self.announce();
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
                        self.cursor_shown = None;
                        if self.entered.take().is_some() {
                            let held = self.modifiers();
                            self.events
                                .push_back(Event::Pointer(PointerEvent::Left, held));
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
                self.cursor_shown = None;
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
                let held = self.modifiers();
                let events = &mut self.events;
                self.gather
                    .frame(&mut |e| events.push_back(Event::Pointer(e, held)));
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
                let now = self.modifiers();
                if now != was {
                    self.events.push_back(Event::Modifiers(now));
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
                                    self.events.push_back(Event::Typed(keys::typed(c)));
                                }
                                self.swallowed.push(code);
                                self.held = None;
                                return;
                            }
                        }
                        if let Some(e) = self.key_event(code, KeyEventKind::Press) {
                            self.events.push_back(Event::Key(e));
                            self.down.retain(|&(k, _)| k != code);
                            self.down.push((code, e));
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
                        let Some(i) = self.down.iter().position(|&(k, _)| k == code) else {
                            return;
                        };
                        let (_, press) = self.down.swap_remove(i);
                        // The modifiers are the release's own, as the
                        // keyboard holds them now.
                        let mods = self
                            .key_event(code, KeyEventKind::Release)
                            .map_or_else(|| self.modifiers(), |e| e.mods);
                        self.events.push_back(Event::Key(KeyEvent {
                            kind: KeyEventKind::Release,
                            mods,
                            ..press
                        }));
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
                                self.events.push_back(Event::Typed(keys::typed(c)));
                            }
                        }
                        Step::Submit => self.events.push_back(Event::Typed(KeyEvent::press(
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
                if self.unseen.is_some_and(|(fb, _)| fb == ev.object) {
                    self.unseen = None;
                }
                let secs = u64::from(*hi) << 32 | u64::from(*lo);
                if let (Some(clock), true) = (self.clock, *nanos < 1_000_000_000) {
                    self.presented = Some(Presentation {
                        clock,
                        at: Stamp(Duration::new(secs, *nanos)),
                        refresh: Duration::from_nanos(u64::from(*refresh)),
                    });
                }
            }
            // A frame not shown holds no refresh.
            ("wp_presentation_feedback", "discarded", _)
                if self.unseen.is_some_and(|(fb, _)| fb == ev.object) =>
            {
                self.unseen = None;
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
                    // The binds went out as the burst was read, so a sync
                    // now is answered after the outputs have said.
                    if !self.globals.outputs.is_empty() {
                        let cb = s.new_id(Interface::named("wl_callback").expect("pinned"));
                        if s.request(
                            ObjectId::DISPLAY,
                            Opcode(protocol::wl_display::request::SYNC),
                            &[Arg::NewId(cb)],
                        )
                        .is_ok()
                        {
                            self.outputs_sync = Some(cb);
                        }
                    }
                    self.create_surface(s);
                }
                if self.outputs_sync == Some(ev.object) {
                    self.outputs_sync = None;
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
        self.follow_scale(s);
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
    /// `wl_shm.format`); one rounding on both systems gives the same
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
