//! **The connection: a socket, the object ids, and the dispatch.**
//!
//! A client speaks by sending requests on objects it owns ids for and
//! listening for events on them; the server allocates from its own range
//! and tells the client when an id it freed may be reused
//! (`wl_display.delete_id`). Everything the wire needs to read an event --
//! which interface an id names -- is the `objects` table here, and the
//! only place an id becomes an interface is `new_id`.
//!
//! Requests are written into an outbox and flushed as one `sendmsg`; events
//! are received into an inbox and dispatched in order, each to the
//! `Handler`, which gets a `Sender` -- the outbox and the object table --
//! so it can answer an event with a request without holding the inbox the
//! event borrows.

use crate::sys;
use crate::table::{EventKey, Interface};
use crate::wire::{self, Arg, HEADER_BYTES, Header, ObjectId, Opcode, Read, WireError};
use std::collections::VecDeque;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

/// An event delivered to a handler.
#[derive(Debug)]
pub struct Event<'a> {
    pub object: ObjectId,
    pub interface: &'static Interface,
    pub opcode: Opcode,
    pub args: &'a [Read<'a>],
}

impl Event<'_> {
    /// The event's interface and opcode, for a handler that routes it by
    /// matching against `protocol::<interface>::on::<EVENT>`.
    pub fn key(&self) -> EventKey {
        EventKey {
            interface: self.interface.index,
            opcode: self.opcode,
        }
    }

    /// The event's name in the interface, for a log or a test.
    pub fn name(&self) -> &'static str {
        self.interface.events[usize::from(self.opcode.0)].name
    }
}

/// What a handler does with each event.
pub trait Handler {
    fn event(&mut self, sender: &mut Sender<'_>, event: Event<'_>);
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Wire(WireError),
    /// An event for an id this client never bound.
    UnknownObject(ObjectId),
    /// An opcode the object's interface does not declare.
    UnknownOpcode(ObjectId, Opcode),
    /// The server reported a protocol error and the connection is dead.
    Protocol {
        object: ObjectId,
        code: u32,
        message: String,
    },
    /// The peer hung up.
    Closed,
    /// The peer sent nothing by the deadline ([`Connection::roundtrip_until`]).
    TimedOut,
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}
impl From<WireError> for Error {
    fn from(e: WireError) -> Error {
        Error::Wire(e)
    }
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for Error {}

/// The object table: which interface each live id names. Client ids index
/// a vector from 1; server ids are few and kept in a list.
#[derive(Default)]
pub struct Objects {
    client: Vec<Option<&'static Interface>>,
    free: Vec<ObjectId>,
    server: Vec<(ObjectId, &'static Interface)>,
}

impl Objects {
    fn alloc(&mut self, iface: &'static Interface) -> ObjectId {
        match self.free.pop() {
            Some(id) => {
                self.client[usize::try_from(id.0).expect("an id") - 1] = Some(iface);
                id
            }
            None => {
                self.client.push(Some(iface));
                ObjectId(u32::try_from(self.client.len()).expect("fewer than 2^32 objects"))
            }
        }
    }

    /// The interface `id` names, if it is live.
    pub fn interface(&self, id: ObjectId) -> Option<&'static Interface> {
        if id >= ObjectId::SERVER_BASE {
            return self.server.iter().find(|(o, _)| *o == id).map(|(_, i)| *i);
        }
        self.client
            .get(usize::try_from(id.0.checked_sub(1)?).expect("an id"))
            .copied()
            .flatten()
    }

    /// A server-created object. **An id the server made before names the
    /// new object from now on**: the server reuses an id once the client
    /// has destroyed what it named, and no `delete_id` says so for a server
    /// id, so the entry is replaced -- a clipboard offer's id reused for a
    /// primary-selection offer would otherwise read its events against the
    /// old interface, and every offer would add an entry for good.
    fn insert_server(&mut self, id: ObjectId, iface: &'static Interface) {
        self.server.retain(|(o, _)| *o != id);
        self.server.push((id, iface));
    }

    /// A client id the server says is free again.
    fn release(&mut self, id: ObjectId) {
        if id < ObjectId::SERVER_BASE {
            if let Some(slot) = self
                .client
                .get_mut(usize::try_from(id.0).expect("an id") - 1)
            {
                *slot = None;
                self.free.push(id);
            }
        } else {
            self.server.retain(|(o, _)| *o != id);
        }
    }
}

/// The half of the connection a handler may use while an event is being
/// dispatched: the outbox and the object table.
pub struct Sender<'a> {
    out: &'a mut Vec<u8>,
    out_fds: &'a mut Vec<OwnedFd>,
    objects: &'a mut Objects,
}

impl Sender<'_> {
    /// A fresh client id bound to `iface`, to pass as a request's `new_id`.
    pub fn new_id(&mut self, iface: &'static Interface) -> ObjectId {
        self.objects.alloc(iface)
    }

    pub fn interface(&self, id: ObjectId) -> Option<&'static Interface> {
        self.objects.interface(id)
    }

    /// Queue a request on `object`. The signature comes from the object's
    /// interface, so a wrong argument list is refused here rather than sent.
    /// An fd argument is duplicated into the outbox so the caller's stays
    /// theirs.
    pub fn request(
        &mut self,
        object: ObjectId,
        opcode: Opcode,
        args: &[Arg<'_>],
    ) -> Result<(), Error> {
        let iface = self
            .objects
            .interface(object)
            .ok_or(Error::UnknownObject(object))?;
        let msg = iface
            .requests
            .get(usize::from(opcode.0))
            .ok_or(Error::UnknownOpcode(object, opcode))?;
        let start = self.out.len();
        self.out.resize(start + 4096.max(HEADER_BYTES), 0);
        let mut fds: Vec<BorrowedFd<'_>> = Vec::new();
        let n = loop {
            match wire::write(
                &mut self.out[start..],
                &mut fds,
                object,
                opcode,
                msg.args,
                args,
            ) {
                Ok(n) => break n,
                Err(WireError::BufferTooSmall) if self.out.len() - start < 0x1_0000 => {
                    let more = (self.out.len() - start) * 2;
                    self.out.resize(start + more, 0);
                    fds.clear();
                }
                Err(e) => {
                    self.out.truncate(start);
                    return Err(e.into());
                }
            }
        };
        self.out.truncate(start + n);
        for fd in fds {
            self.out_fds.push(fd.try_clone_to_owned()?);
        }
        if msg.destructor {
            // The id stays reserved until `wl_display.delete_id` says the
            // server has let go of it.
        }
        Ok(())
    }
}

/// A client connection.
pub struct Connection {
    sock: UnixStream,
    objects: Objects,
    out: Vec<u8>,
    out_fds: Vec<OwnedFd>,
    inbox: Vec<u8>,
    in_fds: VecDeque<OwnedFd>,
}

impl Connection {
    /// Connect the way `libwayland-client` does: `WAYLAND_SOCKET` is an
    /// inherited fd, else `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`, default
    /// `wayland-0`.
    pub fn connect() -> io::Result<Connection> {
        if let Ok(fd) = std::env::var("WAYLAND_SOCKET") {
            let raw: i32 = fd
                .parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "WAYLAND_SOCKET"))?;
            return Ok(Connection::over(UnixStream::from(sys::inherited(raw)?)));
        }
        let dir = std::env::var("XDG_RUNTIME_DIR")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set"))?;
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
        let path = match display.starts_with('/') {
            true => std::path::PathBuf::from(display),
            false => std::path::Path::new(&dir).join(display),
        };
        Ok(Connection::over(UnixStream::connect(path)?))
    }

    /// A connection over a stream already open -- a socket pair in a test.
    pub fn over(sock: UnixStream) -> Connection {
        let mut objects = Objects::default();
        let display = objects.alloc(&crate::protocol::wl_display::INTERFACE);
        debug_assert_eq!(display, ObjectId::DISPLAY);
        Connection {
            sock,
            objects,
            out: Vec::new(),
            out_fds: Vec::new(),
            inbox: Vec::new(),
            in_fds: VecDeque::new(),
        }
    }

    /// The outbox and object table, for requests outside a dispatch.
    pub fn sender(&mut self) -> Sender<'_> {
        Sender {
            out: &mut self.out,
            out_fds: &mut self.out_fds,
            objects: &mut self.objects,
        }
    }

    pub fn objects(&self) -> &Objects {
        &self.objects
    }

    /// Send everything queued, fds beside the first bytes.
    pub fn flush(&mut self) -> io::Result<()> {
        let mut at = 0;
        while at < self.out.len() {
            let fds: Vec<BorrowedFd<'_>> = match at {
                0 => self.out_fds.iter().map(|f| f.as_fd()).collect(),
                _ => Vec::new(),
            };
            let n = sys::send(&self.sock, &self.out[at..], &fds)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "sendmsg wrote nothing",
                ));
            }
            at += n;
        }
        self.out.clear();
        self.out_fds.clear();
        Ok(())
    }

    /// Block for more bytes from the server. `Ok(0)` is the peer gone. A
    /// read that fails -- `WouldBlock` on a socket set not to wait, an
    /// interrupted call -- leaves the inbox as it was: the space the read
    /// was given is not bytes the server sent.
    pub fn receive(&mut self) -> io::Result<usize> {
        let start = self.inbox.len();
        self.inbox.resize(start + 4096, 0);
        let mut fds = Vec::new();
        let n = match sys::recv(&self.sock, &mut self.inbox[start..], &mut fds) {
            Ok(n) => n,
            Err(e) => {
                self.inbox.truncate(start);
                return Err(e);
            }
        };
        self.inbox.truncate(start + n);
        self.in_fds.extend(fds);
        Ok(n)
    }

    /// Dispatch every whole message in the inbox. Returns how many.
    pub fn dispatch<H: Handler>(&mut self, h: &mut H) -> Result<usize, Error> {
        let mut at = 0;
        let mut count = 0;
        let mut args: Vec<Read<'_>> = Vec::new();
        while let Some(Header {
            sender,
            opcode,
            size,
        }) = wire::header(&self.inbox[at..])
        {
            if size < HEADER_BYTES || !size.is_multiple_of(4) {
                return Err(WireError::BadSize.into());
            }
            if at + size > self.inbox.len() {
                break;
            }
            let body = &self.inbox[at + HEADER_BYTES..at + size];
            let iface = self
                .objects
                .interface(sender)
                .ok_or(Error::UnknownObject(sender))?;
            let msg = iface
                .events
                .get(usize::from(opcode.0))
                .ok_or(Error::UnknownOpcode(sender, opcode))?;
            args.clear();
            wire::read(body, msg.args, &mut self.in_fds, &mut args)?;
            // The two display events the connection owns.
            if sender == ObjectId::DISPLAY {
                match usize::from(opcode.0) {
                    0 => {
                        let (object, code, message) = match args.as_slice() {
                            [Read::Object(o), Read::Uint(c), Read::Str(m)] => {
                                (*o, *c, m.unwrap_or("").to_string())
                            }
                            _ => return Err(WireError::Kind.into()),
                        };
                        return Err(Error::Protocol {
                            object,
                            code,
                            message,
                        });
                    }
                    1 => {
                        if let [Read::Uint(id)] = args.as_slice() {
                            self.objects.release(ObjectId(*id));
                        }
                        at += size;
                        count += 1;
                        continue;
                    }
                    _ => {}
                }
            }
            // A `new_id` in an event is a server-created object: bind it
            // before the handler sees the event.
            if let Some(i) = msg.creates {
                for a in &args {
                    if let Read::NewId(id) = a {
                        self.objects.insert_server(*id, i);
                    }
                }
            }
            let mut sender_half = Sender {
                out: &mut self.out,
                out_fds: &mut self.out_fds,
                objects: &mut self.objects,
            };
            h.event(
                &mut sender_half,
                Event {
                    object: sender,
                    interface: iface,
                    opcode,
                    args: &args,
                },
            );
            at += size;
            count += 1;
        }
        self.inbox.drain(..at);
        Ok(count)
    }

    /// Flush, then receive and dispatch until at least one message has
    /// been handled. The loop a client runs when it has nothing to wait on
    /// but the server.
    pub fn roundtrip_once<H: Handler>(&mut self, h: &mut H) -> Result<usize, Error> {
        self.flush()?;
        loop {
            let n = self.dispatch(h)?;
            if n > 0 {
                return Ok(n);
            }
            if self.receive()? == 0 {
                return Err(Error::Closed);
            }
        }
    }

    /// [`Connection::roundtrip_once`] that gives up at `deadline`:
    /// [`Error::TimedOut`] when the server has sent nothing whole by then.
    /// Each read waits at most what is left, through the socket's read
    /// timeout, which is cleared again after it.
    pub fn roundtrip_until<H: Handler>(
        &mut self,
        h: &mut H,
        deadline: std::time::Instant,
    ) -> Result<usize, Error> {
        self.flush()?;
        loop {
            let n = self.dispatch(h)?;
            if n > 0 {
                return Ok(n);
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Err(Error::TimedOut);
            }
            self.sock.set_read_timeout(Some(left))?;
            let got = self.receive();
            self.sock.set_read_timeout(None)?;
            match got {
                Ok(0) => return Err(Error::Closed),
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(Error::TimedOut);
                }
                Err(e) => return Err(Error::Io(e)),
            }
        }
    }
}

impl AsFd for Connection {
    /// The socket, for a caller that waits on it beside other descriptors
    /// and calls `receive` only when the wait says there is something.
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.sock.as_fd()
    }
}
