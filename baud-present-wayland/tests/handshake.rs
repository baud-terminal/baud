//! **The compositor's half of a session, played over a socket pair.** The
//! server reads the client's requests as bytes against the same tables and
//! answers with the events a compositor would send: the globals, the
//! configure, the frame callback, the buffer release, and a keyboard with
//! a real keymap. Nothing here needs a display.

use baud_present_wayland::*;
use baud_wayland::{
    Arg, ArgKind, Connection, Header, Interface, ObjectId, Opcode, Read, protocol, wire,
};
use std::collections::{HashMap, VecDeque};
use std::io::{Read as _, Write as _};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;

#[derive(Debug, Clone)]
enum Val {
    Int(i32),
    Uint(u32),
    Str(Option<String>),
    Obj(ObjectId),
    New(ObjectId),
    NewWith(String, u32, ObjectId),
    Array,
    Fd,
}

struct Request {
    sender: ObjectId,
    iface: &'static str,
    name: &'static str,
    args: Vec<Val>,
}

struct Server {
    sock: UnixStream,
    buf: Vec<u8>,
    fds: VecDeque<OwnedFd>,
    objects: HashMap<ObjectId, &'static Interface>,
}

impl Server {
    fn new(sock: UnixStream) -> Server {
        let mut objects = HashMap::new();
        objects.insert(ObjectId::DISPLAY, Interface::named("wl_display").unwrap());
        Server {
            sock,
            buf: Vec::new(),
            fds: VecDeque::new(),
            objects,
        }
    }

    fn next(&mut self) -> Request {
        loop {
            if let Some(Header {
                sender,
                opcode,
                size,
            }) = wire::header(&self.buf)
            {
                if self.buf.len() >= size {
                    let iface = *self
                        .objects
                        .get(&sender)
                        .unwrap_or_else(|| panic!("request on unknown object {sender:?}"));
                    let msg = &iface.requests[usize::from(opcode.0)];
                    let body = self.buf[wire::HEADER_BYTES..size].to_vec();
                    self.buf.drain(..size);
                    let mut out = Vec::new();
                    wire::read(&body, msg.args, &mut self.fds, &mut out).unwrap();
                    let mut args = Vec::new();
                    for (k, a) in msg.args.iter().zip(out) {
                        args.push(match a {
                            Read::Int(i) => Val::Int(i),
                            Read::Uint(u) => Val::Uint(u),
                            Read::Fixed(_) => Val::Int(0),
                            Read::Str(s) => Val::Str(s.map(str::to_string)),
                            Read::Object(o) => Val::Obj(o),
                            Read::NewId(id) => {
                                if let ArgKind::NewId { interface: Some(n) } = k {
                                    self.objects.insert(id, Interface::named(n).unwrap());
                                }
                                Val::New(id)
                            }
                            Read::NewIdWith {
                                interface,
                                version,
                                id,
                            } => {
                                self.objects
                                    .insert(id, Interface::named(interface).unwrap());
                                Val::NewWith(interface.to_string(), version, id)
                            }
                            Read::Array(_) => Val::Array,
                            Read::Fd(fd) => {
                                self.fds.push_back(fd);
                                Val::Fd
                            }
                        });
                    }
                    return Request {
                        sender,
                        iface: iface.name,
                        name: msg.name,
                        args,
                    };
                }
            }
            let mut chunk = vec![0u8; 65536];
            let mut fds = Vec::new();
            let n = baud_wayland::sys::recv(&self.sock, &mut chunk, &mut fds).unwrap();
            assert!(n > 0, "client hung up");
            self.buf.extend_from_slice(&chunk[..n]);
            self.fds.extend(fds);
        }
    }

    fn send(&mut self, sender: ObjectId, iface: &str, event: &str, args: &[Arg<'_>]) {
        let i = Interface::named(iface).unwrap();
        let op = i.events.iter().position(|m| m.name == event).unwrap();
        let mut buf = [0u8; 4096];
        let mut fds = Vec::new();
        let n = wire::write(
            &mut buf,
            &mut fds,
            sender,
            Opcode(u16::try_from(op).unwrap()),
            i.events[op].args,
            args,
        )
        .unwrap();
        if fds.is_empty() {
            self.sock.write_all(&buf[..n]).unwrap();
        } else {
            assert_eq!(
                baud_wayland::sys::send(&self.sock, &buf[..n], &fds).unwrap(),
                n
            );
        }
    }

    fn expect(&mut self, iface: &str, name: &str) -> Request {
        let r = self.next();
        assert_eq!((r.iface, r.name), (iface, name), "args {:?}", r.args);
        r
    }
}

fn pump(conn: &mut Connection, w: &mut Window) {
    conn.flush().unwrap();
    conn.receive().unwrap();
    conn.dispatch(w).unwrap();
    conn.flush().unwrap();
}

#[test]
fn a_window_opens_shows_a_frame_and_types() {
    let (c, s) = UnixStream::pair().unwrap();
    let mut conn = Connection::over(c);
    let mut server = Server::new(s);
    let mut w = Window::open(&mut conn, "baud", "org.baud.terminal").unwrap();
    conn.flush().unwrap();
    let r = server.expect("wl_display", "get_registry");
    let Val::New(registry) = r.args[0] else {
        panic!()
    };
    server
        .objects
        .insert(registry, Interface::named("wl_registry").unwrap());
    let r = server.expect("wl_display", "sync");
    let Val::New(sync) = r.args[0] else { panic!() };

    // The globals: the client binds the four it needs and makes its surface.
    for (name, iface, version) in [
        (1u32, "wl_compositor", 6u32),
        (2, "wl_shm", 2),
        (3, "xdg_wm_base", 6),
        (4, "wl_seat", 9),
        (5, "wp_viewporter", 1),
    ] {
        server.send(
            registry,
            "wl_registry",
            "global",
            &[Arg::Uint(name), Arg::Str(Some(iface)), Arg::Uint(version)],
        );
    }
    server.send(sync, "wl_callback", "done", &[Arg::Uint(1)]);
    pump(&mut conn, &mut w);
    let mut bound = Vec::new();
    let mut ids: HashMap<&str, ObjectId> = HashMap::new();
    for _ in 0..4 {
        let r = server.expect("wl_registry", "bind");
        let Val::NewWith(iface, version, id) = &r.args[1] else {
            panic!()
        };
        bound.push((iface.clone(), *version));
        ids.insert(
            match iface.as_str() {
                "wl_compositor" => "compositor",
                "wl_shm" => "shm",
                "xdg_wm_base" => "wm_base",
                "wl_seat" => "seat",
                other => panic!("{other}"),
            },
            *id,
        );
    }
    assert_eq!(
        bound,
        [
            ("wl_compositor".to_string(), 6),
            ("wl_shm".to_string(), 2),
            ("xdg_wm_base".to_string(), 6),
            ("wl_seat".to_string(), 9)
        ]
    );
    let r = server.expect("wl_compositor", "create_surface");
    let Val::New(surface) = r.args[0] else {
        panic!()
    };
    let r = server.expect("xdg_wm_base", "get_xdg_surface");
    let Val::New(xdg) = r.args[0] else { panic!() };
    assert!(matches!(r.args[1], Val::Obj(o) if o == surface));
    let r = server.expect("xdg_surface", "get_toplevel");
    let Val::New(top) = r.args[0] else { panic!() };
    let r = server.expect("xdg_toplevel", "set_title");
    assert!(matches!(&r.args[0], Val::Str(Some(t)) if t == "baud"));
    server.expect("xdg_toplevel", "set_app_id");
    server.expect("wl_surface", "commit");
    assert_eq!(w.size(), None, "nothing is configured yet");

    // The configure: the client acks and reports the size.
    server.send(
        top,
        "xdg_toplevel",
        "configure",
        &[Arg::Int(320), Arg::Int(200), Arg::Array(&[])],
    );
    server.send(xdg, "xdg_surface", "configure", &[Arg::Uint(7)]);
    pump(&mut conn, &mut w);
    let r = server.expect("xdg_surface", "ack_configure");
    assert!(matches!(r.args[0], Val::Uint(7)));
    assert_eq!(
        w.next_event(),
        Some(Event::Configured {
            width: Px(320),
            height: Px(200)
        })
    );
    assert_eq!(w.size(), Some((Px(320), Px(200))));

    // A frame: the pool arrives with its file, two buffers, then attach,
    // damage, a frame callback and the commit.
    let frame = vec![0x80ff_0000u32; 320 * 200];
    assert_eq!(w.present(&mut conn.sender(), &frame, 320), Presented::Yes);
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 320),
        Presented::No,
        "one frame in flight"
    );
    conn.flush().unwrap();
    let r = server.expect("wl_shm", "create_pool");
    let Val::New(pool) = r.args[0] else { panic!() };
    assert!(matches!(r.args[1], Val::Fd));
    let Val::Int(size) = r.args[2] else { panic!() };
    // 320 px is 1280 bytes a row, so the stride is 1280; two buffers of 200 rows.
    assert_eq!(size, 2 * 1280 * 200);
    let fd = server
        .fds
        .pop_front()
        .expect("the pool's file arrived beside the request");
    let mut file = std::fs::File::from(fd);
    let mut first = [0u8; 4];
    file.read_exact(&mut first).unwrap();
    assert_eq!(
        u32::from_ne_bytes(first),
        0x8080_0000,
        "premultiplied: half alpha halves the red"
    );
    let mut buffers = Vec::new();
    for _ in 0..2 {
        let r = server.expect("wl_shm_pool", "create_buffer");
        assert_eq!(r.sender, pool);
        let Val::New(b) = r.args[0] else { panic!() };
        assert!(matches!(r.args[4], Val::Int(1280)), "stride in bytes");
        assert!(matches!(r.args[5], Val::Uint(0)), "ARGB8888");
        buffers.push(b);
    }
    let r = server.expect("wl_surface", "attach");
    assert!(matches!(r.args[0], Val::Obj(b) if b == buffers[0]));
    let r = server.expect("wl_surface", "damage_buffer");
    assert!(matches!(r.args[2], Val::Int(320)));
    let r = server.expect("wl_surface", "frame");
    let Val::New(callback) = r.args[0] else {
        panic!()
    };
    server.expect("wl_surface", "commit");

    // The callback and the release: the next frame may go, on the other buffer.
    server.send(callback, "wl_callback", "done", &[Arg::Uint(16)]);
    pump(&mut conn, &mut w);
    assert_eq!(w.next_event(), Some(Event::Frame));
    assert!(!w.in_flight());
    assert_eq!(w.present(&mut conn.sender(), &frame, 320), Presented::Yes);
    conn.flush().unwrap();
    let r = server.expect("wl_surface", "attach");
    assert!(
        matches!(r.args[0], Val::Obj(b) if b == buffers[1]),
        "the compositor still holds the first"
    );
    server.expect("wl_surface", "damage_buffer");
    let r = server.expect("wl_surface", "frame");
    let Val::New(callback2) = r.args[0] else {
        panic!()
    };
    server.expect("wl_surface", "commit");
    server.send(buffers[0], "wl_buffer", "release", &[]);
    server.send(callback2, "wl_callback", "done", &[Arg::Uint(32)]);
    pump(&mut conn, &mut w);
    assert_eq!(w.next_event(), Some(Event::Frame));

    // The keyboard: capabilities, a keymap as a file, modifiers, keys.
    let seat = ids["seat"];
    server.send(
        seat,
        "wl_seat",
        "capabilities",
        &[Arg::Uint(protocol::wl_seat::capability::KEYBOARD)],
    );
    pump(&mut conn, &mut w);
    let r = server.expect("wl_seat", "get_keyboard");
    let Val::New(keyboard) = r.args[0] else {
        panic!()
    };
    let keymap = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-xkb/tests/fixtures/keymap-us.xkb"
    ))
    .unwrap();
    let mut kf = tempfile();
    kf.write_all(&keymap).unwrap();
    kf.write_all(&[0]).unwrap();
    let size = u32::try_from(keymap.len() + 1).unwrap();
    server.send(
        keyboard,
        "wl_keyboard",
        "keymap",
        &[
            Arg::Uint(protocol::wl_keyboard::keymap_format::XKB_V1),
            Arg::Fd(kf.as_fd()),
            Arg::Uint(size),
        ],
    );
    server.send(
        keyboard,
        "wl_keyboard",
        "modifiers",
        &[
            Arg::Uint(1),
            Arg::Uint(0),
            Arg::Uint(0),
            Arg::Uint(0),
            Arg::Uint(0),
        ],
    );
    // evdev KEY_Q is 16; the wire carries evdev codes.
    server.send(
        keyboard,
        "wl_keyboard",
        "key",
        &[
            Arg::Uint(2),
            Arg::Uint(100),
            Arg::Uint(16),
            Arg::Uint(protocol::wl_keyboard::key_state::PRESSED),
        ],
    );
    server.send(
        keyboard,
        "wl_keyboard",
        "modifiers",
        &[
            Arg::Uint(3),
            Arg::Uint(1),
            Arg::Uint(0),
            Arg::Uint(0),
            Arg::Uint(0),
        ],
    );
    server.send(
        keyboard,
        "wl_keyboard",
        "key",
        &[
            Arg::Uint(4),
            Arg::Uint(101),
            Arg::Uint(16),
            Arg::Uint(protocol::wl_keyboard::key_state::PRESSED),
        ],
    );
    server.send(
        keyboard,
        "wl_keyboard",
        "modifiers",
        &[
            Arg::Uint(5),
            Arg::Uint(4),
            Arg::Uint(0),
            Arg::Uint(0),
            Arg::Uint(0),
        ],
    );
    server.send(
        keyboard,
        "wl_keyboard",
        "key",
        &[
            Arg::Uint(6),
            Arg::Uint(102),
            Arg::Uint(16),
            Arg::Uint(protocol::wl_keyboard::key_state::PRESSED),
        ],
    );
    server.send(
        keyboard,
        "wl_keyboard",
        "key",
        &[
            Arg::Uint(7),
            Arg::Uint(103),
            Arg::Uint(16),
            Arg::Uint(protocol::wl_keyboard::key_state::RELEASED),
        ],
    );
    server.send(
        keyboard,
        "wl_keyboard",
        "modifiers",
        &[
            Arg::Uint(8),
            Arg::Uint(0),
            Arg::Uint(0),
            Arg::Uint(0),
            Arg::Uint(0),
        ],
    );
    // KEY_UP is 103; KEY_ENTER 28; KEY_F1 59.
    for k in [103u32, 28, 59] {
        server.send(
            keyboard,
            "wl_keyboard",
            "key",
            &[
                Arg::Uint(9),
                Arg::Uint(104),
                Arg::Uint(k),
                Arg::Uint(protocol::wl_keyboard::key_state::PRESSED),
            ],
        );
    }
    pump(&mut conn, &mut w);
    use baud_domain::{Codepoint, FnKey, KeyCode, KeyEvent, Modifiers};
    let got: Vec<Event> = std::iter::from_fn(|| w.next_event()).collect();
    assert_eq!(
        got,
        [
            Event::Key(KeyEvent::press(
                KeyCode::Char(Codepoint::from_char('q')),
                Modifiers::NONE
            )),
            Event::Key(KeyEvent::press(
                KeyCode::Char(Codepoint::from_char('Q')),
                Modifiers::NONE
            )),
            Event::Key(KeyEvent::press(
                KeyCode::Char(Codepoint::from_char('q')),
                Modifiers::CTRL
            )),
            Event::Key(KeyEvent::press(KeyCode::Up, Modifiers::NONE)),
            Event::Key(KeyEvent::press(KeyCode::Enter, Modifiers::NONE)),
            Event::Key(KeyEvent::press(
                KeyCode::Function(FnKey::new(1).unwrap()),
                Modifiers::NONE
            )),
        ]
    );

    // A ping is answered, and a close is reported.
    server.send(ids["wm_base"], "xdg_wm_base", "ping", &[Arg::Uint(99)]);
    server.send(top, "xdg_toplevel", "close", &[]);
    pump(&mut conn, &mut w);
    let r = server.expect("xdg_wm_base", "pong");
    assert!(matches!(r.args[0], Val::Uint(99)));
    assert_eq!(w.next_event(), Some(Event::Closed));
}

fn tempfile() -> std::fs::File {
    let path = std::env::temp_dir().join(format!(
        "baud-wlp-{}-{:?}",
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
