//! **The clipboard and the primary selection, as Wayland moves them.**
//! Two protocols with one shape: `wl_data_device` (`[[source: wayland]]`,
//! lines 571-1140) for the clipboard, and
//! the primary-selection device (`[[source:
//! wp-primary-selection-unstable-v1]]`) for the primary, the only
//! primary-selection protocol wayland-protocols ships. A device per seat;
//! a copy is a source that offers text types and answers `send` with the
//! text over the pipe the compositor hands over; a paste is the offer the
//! last `selection` event named, asked to `receive` into a pipe of ours,
//! read to its end.
//!
//! **Nothing here waits.** Every pipe is non-blocking and every transfer
//! is pumped a piece at a time from the driver's one wait
//! ([`Clip::fds`], [`Clip::pump`]), so a large paste or a slow owner
//! holds no thread and stops no frame.
//!
//! The text types are foot's five (`[[source: foot-2705e36-selection]]`,
//! 2011-2015): offered all, and on a paste the UTF-8 ones are preferred
//! to the rest, as foot ranks them (2596-2640). A paste whose owner sends
//! nothing for two seconds is abandoned, foot's timeout re-armed on every
//! read (2206, 2337).

use baud_wayland::{Arg, Interface, ObjectId, Opcode, Sender, protocol};
use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Which of the two selections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Board {
    /// `wl_data_device`'s selection: what Ctrl+Shift+C copies.
    Clipboard,
    /// The primary selection: what a finished drag sets and the middle
    /// button pastes.
    Primary,
}

/// The text types, offered in this order.
pub const OFFERED: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "TEXT",
    "STRING",
];

/// How much a paste prefers a text type: UTF-8 over the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Plain,
    Utf8,
}

/// A type's rank, or `None` for one that is not text.
fn rank(mime: &str) -> Option<Rank> {
    match mime {
        "text/plain;charset=utf-8" | "UTF8_STRING" => Some(Rank::Utf8),
        "text/plain" | "TEXT" | "STRING" => Some(Rank::Plain),
        _ => None,
    }
}

/// How long a paste waits on an owner that sends nothing.
pub const RECEIVE_IDLE: Duration = Duration::from_secs(2);

/// What a copy came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Copied {
    Set,
    /// Sent while the window lacks the keyboard, named with the last
    /// serial the client was given. Neither protocol's `set_selection`
    /// asks for the keyboard, but GNOME's compositor refuses a client
    /// without it and cancels the source, which arrives as
    /// [`Done::Cancelled`]; wlroots takes it unless a later copy came first
    /// (Jev's lean, LAND, 0.64/0.32).
    Unfocused,
    /// The compositor offers no manager for this selection, the seat has
    /// no device yet, or no input event has given a serial to name.
    Unavailable,
}

/// What asking for a paste started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pasting {
    /// The text is on its way, or already in the events (this client
    /// owns the selection).
    Started,
    /// Nothing to paste: no selection, none in a text type, or a paste
    /// already reading.
    Nothing,
}

/// Which way a transfer's descriptor is waited on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    Read,
    Write,
}

/// What a pump finished.
#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    /// A paste read to its end: the bytes, as UTF-8 with any other byte
    /// replaced (the types are text, and a `text/plain` owner names no
    /// charset).
    Pasted(Board, String),
    /// The owner sent nothing for [`RECEIVE_IDLE`].
    Abandoned(Board),
    /// A source set without the keyboard was cancelled before the keyboard
    /// came back: the compositor refused it, or another client's copy
    /// replaced it.
    Cancelled(Board),
}

/// Whether the window has the keyboard, as the selections' rules read it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Keyboard {
    #[default]
    Away,
    Here,
}

/// Which of a protocol's three objects an event is on.
#[derive(Clone, Copy)]
enum Object {
    Device,
    Offered,
    Source,
}

/// An offer the compositor introduced, and the best text type it named.
struct Offer {
    id: ObjectId,
    best: Option<(Rank, String)>,
}

/// One selection's side: the manager, the seat's device, the offers
/// introduced and not yet named, the one the last `selection` named, and
/// the source this client owns.
#[derive(Default)]
struct Side {
    manager: Option<ObjectId>,
    device: Option<ObjectId>,
    introduced: Vec<Offer>,
    selection: Option<Offer>,
    source: Option<(ObjectId, Rc<[u8]>)>,
    /// The source set without the keyboard, until the keyboard returns.
    away: Option<ObjectId>,
}

/// The two protocols' requests, by selection: (create the source, get the
/// device, the source's `offer`, its `destroy`, the device's
/// `set_selection`, the offer's `receive`, its `destroy`).
struct Ops {
    source: &'static str,
    create_source: Opcode,
    get_device: Opcode,
    source_offer: Opcode,
    source_destroy: Opcode,
    set_selection: Opcode,
    receive: Opcode,
    offer_destroy: Opcode,
}

fn ops(b: Board) -> Ops {
    match b {
        Board::Clipboard => Ops {
            source: "wl_data_source",
            create_source: Opcode(protocol::wl_data_device_manager::request::CREATE_DATA_SOURCE),
            get_device: Opcode(protocol::wl_data_device_manager::request::GET_DATA_DEVICE),
            source_offer: Opcode(protocol::wl_data_source::request::OFFER),
            source_destroy: Opcode(protocol::wl_data_source::request::DESTROY),
            set_selection: Opcode(protocol::wl_data_device::request::SET_SELECTION),
            receive: Opcode(protocol::wl_data_offer::request::RECEIVE),
            offer_destroy: Opcode(protocol::wl_data_offer::request::DESTROY),
        },
        Board::Primary => Ops {
            source: "zwp_primary_selection_source_v1",
            create_source: Opcode(
                protocol::zwp_primary_selection_device_manager_v1::request::CREATE_SOURCE,
            ),
            get_device: Opcode(
                protocol::zwp_primary_selection_device_manager_v1::request::GET_DEVICE,
            ),
            source_offer: Opcode(protocol::zwp_primary_selection_source_v1::request::OFFER),
            source_destroy: Opcode(protocol::zwp_primary_selection_source_v1::request::DESTROY),
            set_selection: Opcode(
                protocol::zwp_primary_selection_device_v1::request::SET_SELECTION,
            ),
            receive: Opcode(protocol::zwp_primary_selection_offer_v1::request::RECEIVE),
            offer_destroy: Opcode(protocol::zwp_primary_selection_offer_v1::request::DESTROY),
        },
    }
}

/// How many of a text's bytes a `send` has written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Written(usize);

/// A `send` being answered: the text and how much of it is written.
struct Sending {
    fd: File,
    text: Rc<[u8]>,
    at: Written,
}

/// A paste being read.
struct Receiving {
    fd: File,
    board: Board,
    got: Vec<u8>,
    /// When it last read anything, for [`RECEIVE_IDLE`].
    last: Instant,
}

/// Both selections and the transfers in flight.
#[derive(Default)]
pub struct Clip {
    clipboard: Side,
    primary: Side,
    sending: Vec<Sending>,
    receiving: Option<Receiving>,
    done: Vec<Done>,
    keyboard: Keyboard,
}

impl Clip {
    fn side(&mut self, b: Board) -> &mut Side {
        match b {
            Board::Clipboard => &mut self.clipboard,
            Board::Primary => &mut self.primary,
        }
    }

    fn side_of(&self, b: Board) -> &Side {
        match b {
            Board::Clipboard => &self.clipboard,
            Board::Primary => &self.primary,
        }
    }

    /// The manager global for `b`, bound.
    pub fn bound(&mut self, b: Board, manager: ObjectId) {
        self.side(b).manager = Some(manager);
    }

    /// Each selection's device, once its manager and the seat both exist,
    /// whichever came second.
    pub fn devices(&mut self, s: &mut Sender<'_>, seat: Option<ObjectId>) {
        let Some(seat) = seat else { return };
        for b in [Board::Clipboard, Board::Primary] {
            let side = self.side(b);
            let (Some(manager), None) = (side.manager, side.device) else {
                continue;
            };
            let name = match b {
                Board::Clipboard => "wl_data_device",
                Board::Primary => "zwp_primary_selection_device_v1",
            };
            let device = s.new_id(Interface::named(name).expect("pinned"));
            let _ = s.request(
                manager,
                ops(b).get_device,
                &[Arg::NewId(device), Arg::Object(seat)],
            );
            side.device = Some(device);
        }
    }

    /// Whether `b` has a device to copy to and paste from.
    pub fn has(&self, b: Board) -> bool {
        self.side_of(b).device.is_some()
    }

    /// **`text` becomes `b`'s selection**: a fresh source -- a used one
    /// "may not be used in any further set_selection" -- offering the text
    /// types, set with `serial`, the input event that asked; the source it
    /// replaces is destroyed. Without the keyboard it is sent all the same,
    /// and said ([`Copied::Unfocused`]).
    pub fn copy(
        &mut self,
        s: &mut Sender<'_>,
        b: Board,
        text: &str,
        serial: Option<u32>,
    ) -> Copied {
        let o = ops(b);
        let keyboard = self.keyboard;
        let side = self.side(b);
        let (Some(manager), Some(device), Some(serial)) = (side.manager, side.device, serial)
        else {
            return Copied::Unavailable;
        };
        let source = s.new_id(Interface::named(o.source).expect("pinned"));
        let _ = s.request(manager, o.create_source, &[Arg::NewId(source)]);
        for mime in OFFERED {
            let _ = s.request(source, o.source_offer, &[Arg::Str(Some(mime))]);
        }
        let _ = s.request(
            device,
            o.set_selection,
            &[Arg::Object(source), Arg::Uint(serial)],
        );
        if let Some((old, _)) = side.source.replace((source, Rc::from(text.as_bytes()))) {
            let _ = s.request(old, o.source_destroy, &[]);
        }
        match keyboard {
            Keyboard::Here => {
                side.away = None;
                Copied::Set
            }
            Keyboard::Away => {
                side.away = Some(source);
                Copied::Unfocused
            }
        }
    }

    /// The source this client offers `b`'s selection from, while it owns
    /// it: none once another client's copy cancelled it.
    pub fn offering(&self, b: Board) -> Option<ObjectId> {
        self.side_of(b).source.as_ref().map(|(id, _)| *id)
    }

    /// **`b` given up if `source` still offers it**, as xterm's
    /// `DisownSelection` gives up a selection it owns, `XtDisownSelection`
    /// doing nothing for one it no longer owns (`[[source: xterm-button]]`):
    /// [`Clip::clear`], or `None` when another copy replaced the source,
    /// whose selection is not this one's to empty.
    pub fn give_up(
        &mut self,
        s: &mut Sender<'_>,
        b: Board,
        source: ObjectId,
        serial: Option<u32>,
    ) -> Option<Copied> {
        match self.offering(b) == Some(source) {
            true => Some(self.clear(s, b, serial)),
            false => None,
        }
    }

    /// **`b` emptied**: "To unset the selection, set the source to NULL."
    pub fn clear(&mut self, s: &mut Sender<'_>, b: Board, serial: Option<u32>) -> Copied {
        let o = ops(b);
        let keyboard = self.keyboard;
        let side = self.side(b);
        let (Some(device), Some(serial)) = (side.device, serial) else {
            return Copied::Unavailable;
        };
        let _ = s.request(
            device,
            o.set_selection,
            &[Arg::Object(ObjectId::NULL), Arg::Uint(serial)],
        );
        if let Some((old, _)) = side.source.take() {
            let _ = s.request(old, o.source_destroy, &[]);
        }
        side.away = None;
        match keyboard {
            Keyboard::Here => Copied::Set,
            Keyboard::Away => Copied::Unfocused,
        }
    }

    /// **A paste from `b`.** This client's own text at once, where it owns
    /// the selection; else the named offer asked to send its best text
    /// type into a non-blocking pipe, whose write end goes to the
    /// compositor and is closed here -- or the read would never see its
    /// end. One paste at a time, as foot keeps it.
    pub fn paste(&mut self, s: &mut Sender<'_>, b: Board, now: Instant) -> Pasting {
        if self.receiving.is_some() {
            return Pasting::Nothing;
        }
        let o = ops(b);
        let side = self.side_of(b);
        if let Some((_, text)) = &side.source {
            let text = String::from_utf8_lossy(text).into_owned();
            self.done.push(Done::Pasted(b, text));
            return Pasting::Started;
        }
        let Some(Offer {
            id,
            best: Some((_, mime)),
        }) = &side.selection
        else {
            return Pasting::Nothing;
        };
        let Ok((reader, writer)) = io::pipe() else {
            return Pasting::Nothing;
        };
        let read = File::from(OwnedFd::from(reader));
        if crate::sys::nonblocking(&read).is_err() {
            return Pasting::Nothing;
        }
        let write = OwnedFd::from(writer);
        if s.request(
            *id,
            o.receive,
            &[Arg::Str(Some(mime)), Arg::Fd(write.as_fd())],
        )
        .is_err()
        {
            return Pasting::Nothing;
        }
        // The request holds its own copy until the flush sends it.
        drop(write);
        self.receiving = Some(Receiving {
            fd: read,
            board: b,
            got: Vec::new(),
            last: now,
        });
        Pasting::Started
    }

    /// The descriptors the driver's wait should watch, and for what.
    pub fn fds(&self) -> impl Iterator<Item = (BorrowedFd<'_>, Want)> {
        self.sending
            .iter()
            .map(|t| (t.fd.as_fd(), Want::Write))
            .chain(self.receiving.iter().map(|r| (r.fd.as_fd(), Want::Read)))
    }

    /// When a paste reading now is abandoned if nothing more arrives.
    pub fn deadline(&self) -> Option<Instant> {
        self.receiving.as_ref().map(|r| r.last + RECEIVE_IDLE)
    }

    /// **Every transfer moved as far as its pipe allows now**: each
    /// `send` written until the pipe is full, and closed when the text is
    /// all out or the reader has gone; the paste read until the pipe is
    /// empty, and finished at its end or abandoned past its deadline.
    pub fn pump(&mut self, now: Instant) {
        self.sending.retain_mut(|t| {
            loop {
                let Some(rest) = t.text.get(t.at.0..).filter(|r| !r.is_empty()) else {
                    break false;
                };
                match t.fd.write(rest) {
                    Ok(0) => break false,
                    Ok(n) => t.at = Written(t.at.0 + n),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break true,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break false,
                }
            }
        });
        let Some(r) = self.receiving.as_mut() else {
            return;
        };
        let mut chunk = [0u8; 65536];
        let end = loop {
            match r.fd.read(&mut chunk) {
                Ok(0) => break Some(true),
                Ok(n) => {
                    r.got.extend_from_slice(&chunk[..n]);
                    r.last = now;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break None,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break Some(true),
            }
        };
        match end {
            Some(_) => {
                let r = self.receiving.take().expect("reading");
                let text = String::from_utf8_lossy(&r.got).into_owned();
                self.done.push(Done::Pasted(r.board, text));
            }
            None if now >= r.last + RECEIVE_IDLE => {
                let r = self.receiving.take().expect("reading");
                self.done.push(Done::Abandoned(r.board));
            }
            None => {}
        }
    }

    /// What the pumps and pastes finished since the last call.
    pub fn take_done(&mut self) -> Vec<Done> {
        std::mem::take(&mut self.done)
    }

    /// The keyboard left: a clipboard offer "is valid until ... the client
    /// loses keyboard focus", and the primary's the same, so both go.
    pub fn focus_lost(&mut self, s: &mut Sender<'_>) {
        self.keyboard = Keyboard::Away;
        for b in [Board::Clipboard, Board::Primary] {
            let o = ops(b);
            if let Some(offer) = self.side(b).selection.take() {
                let _ = s.request(offer.id, o.offer_destroy, &[]);
            }
        }
    }

    /// Whether the window has the keyboard's focus.
    pub(crate) fn keyboard(&self) -> Keyboard {
        self.keyboard
    }

    /// The keyboard came: a source set without it is no longer watched.
    pub fn focus_gained(&mut self) {
        self.keyboard = Keyboard::Here;
        self.clipboard.away = None;
        self.primary.away = None;
    }

    /// **An event of either protocol's**, or `false` for one that is not.
    pub fn event(&mut self, s: &mut Sender<'_>, ev: &baud_wayland::Event<'_>) -> bool {
        use Object::{Device, Offered, Source};
        use baud_wayland::Read;
        let (board, kind) = match ev.interface.name {
            "wl_data_device" => (Board::Clipboard, Device),
            "wl_data_offer" => (Board::Clipboard, Offered),
            "wl_data_source" => (Board::Clipboard, Source),
            "zwp_primary_selection_device_v1" => (Board::Primary, Device),
            "zwp_primary_selection_offer_v1" => (Board::Primary, Offered),
            "zwp_primary_selection_source_v1" => (Board::Primary, Source),
            _ => return false,
        };
        let o = ops(board);
        let side = match board {
            Board::Clipboard => &mut self.clipboard,
            Board::Primary => &mut self.primary,
        };
        match (kind, ev.name(), ev.args) {
            // The offer comes first, its types next, and then the event
            // that says what it is for.
            (Device, "data_offer", [Read::NewId(id)]) => side.introduced.push(Offer {
                id: *id,
                best: None,
            }),
            (Offered, "offer", [Read::Str(Some(mime))]) => {
                let offer = side
                    .introduced
                    .iter_mut()
                    .chain(side.selection.iter_mut())
                    .find(|f| f.id == ev.object);
                if let (Some(f), Some(r)) = (offer, rank(mime)) {
                    if f.best.as_ref().is_none_or(|(b, _)| r > *b) {
                        f.best = Some((r, (*mime).to_string()));
                    }
                }
            }
            // "The client must destroy the previous selection data_offer,
            // if any, upon receiving this event."
            (Device, "selection", [Read::Object(id)]) => {
                let named = side
                    .introduced
                    .iter()
                    .position(|f| f.id == *id)
                    .map(|i| side.introduced.remove(i));
                if let Some(old) = std::mem::replace(&mut side.selection, named) {
                    let _ = s.request(old.id, o.offer_destroy, &[]);
                }
            }
            // A drag's offer: baud takes no drop, and the offer goes when
            // the drag leaves or drops.
            (Device, "leave" | "drop", _) => {
                for f in side.introduced.drain(..) {
                    let _ = s.request(f.id, o.offer_destroy, &[]);
                }
            }
            (Source, "send", [Read::Str(Some(_)), Read::Fd(fd)]) => {
                let text = match &side.source {
                    Some((id, text)) if *id == ev.object => Some(text.clone()),
                    _ => None,
                };
                if let (Some(text), Ok(dup)) = (text, fd.try_clone()) {
                    let fd = File::from(dup);
                    if crate::sys::nonblocking(&fd).is_ok() {
                        self.sending.push(Sending {
                            fd,
                            text,
                            at: Written::default(),
                        });
                    }
                }
            }
            // Replaced by another client's copy: "This data source is no
            // longer valid."
            // A source this client replaced was destroyed when it was, and
            // a second destroy would name a dead object.
            (Source, "cancelled", _)
                if side.source.as_ref().is_some_and(|(id, _)| *id == ev.object) =>
            {
                side.source = None;
                let _ = s.request(ev.object, o.source_destroy, &[]);
                if side.away.take() == Some(ev.object) {
                    self.done.push(Done::Cancelled(board));
                }
            }
            _ => {}
        }
        true
    }
}
