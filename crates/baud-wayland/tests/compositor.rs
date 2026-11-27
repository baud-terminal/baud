//! **A fake compositor over a socket pair**: the other end of the
//! connection, speaking the same wire, so the client's requests are read
//! as bytes and its events are written as bytes, with no compositor on
//! the box. The gate that admits the window is a Linux box; this is the
//! gate that admits the protocol.

use baud_wayland::*;
use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;

/// The server side: reads one request and answers by writing raw events.
struct Server {
    sock: UnixStream,
    buf: Vec<u8>,
}

impl Server {
    fn next(&mut self) -> (Header, Vec<u8>) {
        loop {
            if let Some(h) = wire::header(&self.buf) {
                if self.buf.len() >= h.size {
                    let body = self.buf[wire::HEADER_BYTES..h.size].to_vec();
                    self.buf.drain(..h.size);
                    return (h, body);
                }
            }
            let mut chunk = [0u8; 4096];
            let n = self.sock.read(&mut chunk).unwrap();
            assert!(n > 0, "client hung up");
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    fn send(&mut self, sender: ObjectId, opcode: Opcode, sig: &[ArgKind], args: &[Arg<'_>]) {
        let mut buf = [0u8; 4096];
        let mut fds = Vec::new();
        let n = wire::write(&mut buf, &mut fds, sender, opcode, sig, args).unwrap();
        assert!(fds.is_empty(), "use send_with_fd");
        self.sock.write_all(&buf[..n]).unwrap();
    }
}

#[derive(Default)]
struct Globals {
    seen: Vec<(u32, String, u32)>,
    bound: Option<ObjectId>,
    keymap: Option<(u32, OwnedFd, u32)>,
    configured: Vec<u32>,
}

impl Handler for Globals {
    fn event(&mut self, s: &mut Sender<'_>, ev: Event<'_>) {
        match (ev.interface.name, ev.name()) {
            ("wl_registry", "global") => {
                if let [
                    Read::Uint(name),
                    Read::Str(Some(iface)),
                    Read::Uint(version),
                ] = ev.args
                {
                    self.seen.push((*name, iface.to_string(), *version));
                    if *iface == "wl_compositor" {
                        let id = s.new_id(Interface::named("wl_compositor").unwrap());
                        s.request(
                            ev.object,
                            Opcode(protocol::wl_registry::request::BIND),
                            &[
                                Arg::Uint(*name),
                                Arg::NewIdWith {
                                    interface: iface,
                                    version: (*version).min(6),
                                    id,
                                },
                            ],
                        )
                        .unwrap();
                        self.bound = Some(id);
                    }
                }
            }
            ("wl_keyboard", "keymap") => {
                if let [Read::Uint(format), Read::Fd(fd), Read::Uint(size)] = ev.args {
                    self.keymap = Some((*format, fd.try_clone().unwrap(), *size));
                }
            }
            ("xdg_surface", "configure") => {
                if let [Read::Uint(serial)] = ev.args {
                    self.configured.push(*serial);
                }
            }
            _ => {}
        }
    }
}

fn pair() -> (Connection, Server) {
    let (c, s) = UnixStream::pair().unwrap();
    (
        Connection::over(c),
        Server {
            sock: s,
            buf: Vec::new(),
        },
    )
}

/// The first exchange every client makes: `get_registry`, the globals
/// come back, the client binds one, and the bind arrives as bytes the
/// server can read against the same table.
#[test]
fn a_registry_round_trip_binds_a_global() {
    let (mut conn, mut server) = pair();
    let registry = conn
        .sender()
        .new_id(Interface::named("wl_registry").unwrap());
    assert_eq!(registry, ObjectId(2));
    conn.sender()
        .request(
            ObjectId::DISPLAY,
            Opcode(protocol::wl_display::request::GET_REGISTRY),
            &[Arg::NewId(registry)],
        )
        .unwrap();
    conn.flush().unwrap();
    let (h, body) = server.next();
    assert_eq!(
        (h.sender, h.opcode, h.size),
        (ObjectId::DISPLAY, Opcode(1), 12)
    );
    assert_eq!(body, 2u32.to_ne_bytes());
    // Two globals, then the client binds the compositor.
    let sig = protocol::wl_registry::EVENTS[0].args;
    server.send(
        registry,
        Opcode(0),
        sig,
        &[Arg::Uint(1), Arg::Str(Some("wl_shm")), Arg::Uint(2)],
    );
    server.send(
        registry,
        Opcode(0),
        sig,
        &[Arg::Uint(2), Arg::Str(Some("wl_compositor")), Arg::Uint(6)],
    );
    let mut g = Globals::default();
    let n = conn.roundtrip_once(&mut g).unwrap();
    assert_eq!(n, 2);
    assert_eq!(g.seen[0], (1, "wl_shm".to_string(), 2));
    assert_eq!(g.seen[1], (2, "wl_compositor".to_string(), 6));
    assert_eq!(g.bound, Some(ObjectId(3)));
    conn.flush().unwrap();
    let (h, body) = server.next();
    assert_eq!((h.sender, h.opcode), (registry, Opcode(0)));
    let mut out = Vec::new();
    wire::read(
        &body,
        protocol::wl_registry::REQUESTS[0].args,
        &mut VecDeque::new(),
        &mut out,
    )
    .unwrap();
    assert!(matches!(
        out.as_slice(),
        [
            Read::Uint(2),
            Read::NewIdWith {
                interface: "wl_compositor",
                version: 6,
                id: ObjectId(3)
            }
        ]
    ));
    assert_eq!(
        conn.objects().interface(ObjectId(3)).map(|i| i.name),
        Some("wl_compositor")
    );
}

/// **An fd crosses the socket beside its event.** A keymap arrives as a
/// descriptor; the client reads the bytes behind it.
#[test]
fn a_keymap_fd_arrives_beside_its_event() {
    let (mut conn, server) = pair();
    let keyboard = conn
        .sender()
        .new_id(Interface::named("wl_keyboard").unwrap());
    let mut file = tempfile();
    file.write_all(b"xkb_keymap { };").unwrap();
    let mut buf = [0u8; 64];
    let mut fds = Vec::new();
    let sig = protocol::wl_keyboard::EVENTS[0].args;
    let n = wire::write(
        &mut buf,
        &mut fds,
        keyboard,
        Opcode(0),
        sig,
        &[Arg::Uint(1), Arg::Fd(file.as_fd()), Arg::Uint(15)],
    )
    .unwrap();
    assert_eq!(n, 16, "the fd takes no bytes");
    baud_wayland_sys_send(&server.sock, &buf[..n], &fds);
    let mut g = Globals::default();
    assert_eq!(conn.roundtrip_once(&mut g).unwrap(), 1);
    let (format, fd, size) = g.keymap.take().expect("the keymap event");
    assert_eq!((format, size), (1, 15));
    let mut got = std::fs::File::from(fd);
    use std::io::Seek as _;
    got.seek(std::io::SeekFrom::Start(0)).unwrap();
    let mut s = String::new();
    got.read_to_string(&mut s).unwrap();
    assert_eq!(s, "xkb_keymap { };");
}

/// `wl_display.delete_id` frees a client id for reuse, and a protocol
/// error ends the connection with the server's message.
#[test]
fn delete_id_recycles_and_error_ends_the_connection() {
    let (mut conn, mut server) = pair();
    let a = conn
        .sender()
        .new_id(Interface::named("wl_surface").unwrap());
    let b = conn
        .sender()
        .new_id(Interface::named("wl_surface").unwrap());
    assert_eq!((a, b), (ObjectId(2), ObjectId(3)));
    server.send(
        ObjectId::DISPLAY,
        Opcode(1),
        protocol::wl_display::EVENTS[1].args,
        &[Arg::Uint(2)],
    );
    let mut g = Globals::default();
    assert_eq!(conn.roundtrip_once(&mut g).unwrap(), 1);
    assert!(conn.objects().interface(ObjectId(2)).is_none(), "released");
    let c = conn
        .sender()
        .new_id(Interface::named("wl_surface").unwrap());
    assert_eq!(c, ObjectId(2), "and reused");
    server.send(
        ObjectId::DISPLAY,
        Opcode(0),
        protocol::wl_display::EVENTS[0].args,
        &[
            Arg::Object(ObjectId(3)),
            Arg::Uint(1),
            Arg::Str(Some("invalid method")),
        ],
    );
    match conn.roundtrip_once(&mut g) {
        Err(client::Error::Protocol {
            object,
            code,
            message,
        }) => {
            assert_eq!(
                (object, code, message.as_str()),
                (ObjectId(3), 1, "invalid method")
            );
        }
        other => panic!("{other:?}"),
    }
}

/// **A server id the server makes again names the new object.** A
/// clipboard offer at `0xff000001`, destroyed by the client, and then a
/// primary-selection offer at the same id: its events read against the
/// primary's interface, not the clipboard's -- the server frees its own
/// ids without a `delete_id`, so only the new `new_id` can say.
#[test]
fn a_server_id_made_again_names_its_new_interface() {
    let (mut conn, mut server) = pair();
    let clipboard = conn
        .sender()
        .new_id(Interface::named("wl_data_device").unwrap());
    let primary = conn
        .sender()
        .new_id(Interface::named("zwp_primary_selection_device_v1").unwrap());
    let offer = ObjectId(0xff00_0001);
    server.send(
        clipboard,
        Opcode(protocol::wl_data_device::event::DATA_OFFER),
        protocol::wl_data_device::EVENTS[0].args,
        &[Arg::NewId(offer)],
    );
    let mut g = Globals::default();
    assert_eq!(conn.roundtrip_once(&mut g).unwrap(), 1);
    assert_eq!(
        conn.objects().interface(offer).map(|i| i.name),
        Some("wl_data_offer")
    );
    server.send(
        primary,
        Opcode(protocol::zwp_primary_selection_device_v1::event::DATA_OFFER),
        protocol::zwp_primary_selection_device_v1::EVENTS[0].args,
        &[Arg::NewId(offer)],
    );
    assert_eq!(conn.roundtrip_once(&mut g).unwrap(), 1);
    assert_eq!(
        conn.objects().interface(offer).map(|i| i.name),
        Some("zwp_primary_selection_offer_v1"),
        "the id names what the server made last"
    );
}

/// An xdg configure carries a serial the client must ack; the ack goes
/// back on the same object.
#[test]
fn a_configure_is_acked_on_its_surface() {
    let (mut conn, mut server) = pair();
    let xs = conn
        .sender()
        .new_id(Interface::named("xdg_surface").unwrap());
    server.send(
        xs,
        Opcode(protocol::xdg_surface::event::CONFIGURE),
        protocol::xdg_surface::EVENTS[0].args,
        &[Arg::Uint(77)],
    );
    let mut g = Globals::default();
    conn.roundtrip_once(&mut g).unwrap();
    assert_eq!(g.configured, [77]);
    conn.sender()
        .request(
            xs,
            Opcode(protocol::xdg_surface::request::ACK_CONFIGURE),
            &[Arg::Uint(77)],
        )
        .unwrap();
    conn.flush().unwrap();
    let (h, body) = server.next();
    assert_eq!(
        (h.sender, h.opcode),
        (xs, Opcode(protocol::xdg_surface::request::ACK_CONFIGURE))
    );
    assert_eq!(body, 77u32.to_ne_bytes());
}

fn tempfile() -> std::fs::File {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "baud-wayland-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    f
}

fn baud_wayland_sys_send(sock: &UnixStream, bytes: &[u8], fds: &[std::os::fd::BorrowedFd<'_>]) {
    let n = baud_wayland::sys::send(sock, bytes, fds).unwrap();
    assert_eq!(n, bytes.len());
}

/// **A round trip with a deadline ends at it when the server says
/// nothing**: a compositor that never answers ends `roundtrip_until` with
/// `Error::TimedOut` at its deadline, not a read that waits forever; one
/// that answers ends it with what it sent. Run on a thread, so a call that
/// waited would fail this test at five seconds instead of hanging it.
#[test]
fn a_round_trip_with_a_deadline_ends_at_it_when_the_server_says_nothing() {
    struct Count(usize);
    impl Handler for Count {
        fn event(&mut self, _: &mut Sender<'_>, _: Event<'_>) {
            self.0 += 1;
        }
    }
    let (c, s) = UnixStream::pair().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut conn = Connection::over(c);
        let start = std::time::Instant::now();
        let deadline = start + std::time::Duration::from_millis(200);
        let got = conn.roundtrip_until(&mut Count(0), deadline);
        let _ = tx.send((
            matches!(got, Err(baud_wayland::client::Error::TimedOut)),
            start.elapsed(),
        ));
    });
    let (timed_out, took) = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("roundtrip_until returned within five seconds");
    assert!(timed_out, "a silent server ends it as TimedOut");
    assert!(
        took >= std::time::Duration::from_millis(200),
        "not before its deadline: {took:?}"
    );
    drop(s);

    // The control: a server that answers ends it with the event.
    let (c, mut s) = UnixStream::pair().unwrap();
    let mut conn = Connection::over(c);
    let sync = conn
        .sender()
        .new_id(Interface::named("wl_callback").unwrap());
    let mut buf = [0u8; 64];
    let mut fds = Vec::new();
    let n = wire::write(
        &mut buf,
        &mut fds,
        sync,
        Opcode(0),
        Interface::named("wl_callback").unwrap().events[0].args,
        &[Arg::Uint(1)],
    )
    .unwrap();
    s.write_all(&buf[..n]).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut count = Count(0);
    assert_eq!(conn.roundtrip_until(&mut count, deadline).unwrap(), 1);
    assert_eq!(count.0, 1);
}
