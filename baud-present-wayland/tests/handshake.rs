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
        // A request the client never sends fails the test instead of hanging
        // it: `next` reads until a whole request is in, and without a timeout
        // a missing one blocks in `recv` forever.
        sock.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
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

    // **A zero configure means the client chooses, and it must keep what it
    // had.** This is not an edge case: weston sends `0 x 0` for an ordinary
    // toplevel, so it is what a real compositor does on nearly every launch --
    // and it was the one branch of `xdg_toplevel::configure` no test reached,
    // because the case above always sends a size. Running baud against a real
    // headless weston is what exercised it; the guard held, and this pins it
    // so nothing removes the `w > 0 && h > 0` test as redundant.
    server.send(
        top,
        "xdg_toplevel",
        "configure",
        &[Arg::Int(0), Arg::Int(0), Arg::Array(&[])],
    );
    server.send(xdg, "xdg_surface", "configure", &[Arg::Uint(8)]);
    pump(&mut conn, &mut w);
    let r = server.expect("xdg_surface", "ack_configure");
    assert!(
        matches!(r.args[0], Val::Uint(8)),
        "a zero configure is still acked -- the ack is owed per xdg_surface \
         configure whatever the size says"
    );
    assert_eq!(
        w.size(),
        Some((Px(320), Px(200))),
        "a zero configure must not resize the window: the compositor is \
         deferring to the client, not asking for a zero-sized surface"
    );

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
    use baud_domain::{Codepoint, FnKey, KeyCode, KeyEvent, KeyEventKind, Modifiers};
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
            // The release of the Control-q, delivered as one: the core sends
            // it only where kitty's event types asked for releases.
            Event::Key(KeyEvent {
                kind: KeyEventKind::Release,
                ..KeyEvent::press(KeyCode::Char(Codepoint::from_char('q')), Modifiers::CTRL)
            }),
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

/// A session as far as a keyboard with the `us` keymap: the globals bound,
/// the surface made, the seat's keyboard created and its keymap sent. Returns
/// the keyboard and the surface, which `enter` and `leave` name.
fn keyboard_session() -> (Connection, Server, Window, ObjectId, ObjectId) {
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
    for (name, iface, version) in [
        (1u32, "wl_compositor", 6u32),
        (2, "wl_shm", 2),
        (3, "xdg_wm_base", 6),
        (4, "wl_seat", 9),
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
    let mut seat = None;
    let mut surface = None;
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            ("wl_registry", "bind", [_, Val::NewWith(i, _, id)]) if i == "wl_seat" => {
                seat = Some(*id);
            }
            ("wl_compositor", "create_surface", [Val::New(id)]) => surface = Some(*id),
            ("wl_surface", "commit", _) => break,
            _ => {}
        }
    }
    let (seat, surface) = (seat.expect("the seat"), surface.expect("the surface"));
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
    send_keymap(&mut server, keyboard);
    pump(&mut conn, &mut w);
    (conn, server, w, keyboard, surface)
}

fn send_keymap(server: &mut Server, keyboard: ObjectId) {
    let keymap = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-xkb/tests/fixtures/keymap-us.xkb"
    ))
    .unwrap();
    let mut kf = tempfile();
    kf.write_all(&keymap).unwrap();
    kf.write_all(&[0]).unwrap();
    server.send(
        keyboard,
        "wl_keyboard",
        "keymap",
        &[
            Arg::Uint(protocol::wl_keyboard::keymap_format::XKB_V1),
            Arg::Fd(kf.as_fd()),
            Arg::Uint(u32::try_from(keymap.len() + 1).unwrap()),
        ],
    );
}

// evdev codes, which the wire carries.
const KEY_Q: u32 = 16;
const KEY_UP: u32 = 103;
const KEY_LEFTSHIFT: u32 = 42;

fn key(server: &mut Server, keyboard: ObjectId, code: u32, state: u32) {
    server.send(
        keyboard,
        "wl_keyboard",
        "key",
        &[
            Arg::Uint(1),
            Arg::Uint(0),
            Arg::Uint(code),
            Arg::Uint(state),
        ],
    );
}

fn depressed(server: &mut Server, keyboard: ObjectId, mask: u32) {
    server.send(
        keyboard,
        "wl_keyboard",
        "modifiers",
        &[
            Arg::Uint(1),
            Arg::Uint(mask),
            Arg::Uint(0),
            Arg::Uint(0),
            Arg::Uint(0),
        ],
    );
}

fn repeat_info(server: &mut Server, keyboard: ObjectId, rate: i32, delay: i32) {
    server.send(
        keyboard,
        "wl_keyboard",
        "repeat_info",
        &[Arg::Int(rate), Arg::Int(delay)],
    );
}

fn drain(w: &mut Window) -> Vec<Event> {
    std::iter::from_fn(|| w.next_event()).collect()
}

fn typed(c: char, mods: baud_domain::Modifiers, kind: baud_domain::KeyEventKind) -> Event {
    use baud_domain::{Codepoint, KeyCode, KeyEvent};
    Event::Key(KeyEvent {
        kind,
        ..KeyEvent::press(KeyCode::Char(Codepoint::from_char(c)), mods)
    })
}

const MS: std::time::Duration = std::time::Duration::from_millis(1);
const PRESSED: u32 = protocol::wl_keyboard::key_state::PRESSED;
const RELEASED: u32 = protocol::wl_keyboard::key_state::RELEASED;

/// **A held key repeats at the compositor's rate, after its delay, as the
/// modifiers now in force make it, until it is released.** Wayland leaves
/// repeat to the client: `repeat_info` gives the rate and the delay, and the
/// seat is bound below version 10, whose "repeated" key state is the only
/// way a compositor repeats a key itself. The deadline the window reports is
/// the anchor, so nothing here depends on how long the test takes.
#[test]
fn a_held_key_repeats_until_it_is_released() {
    use baud_domain::{KeyEventKind, Modifiers};
    let (mut conn, mut server, mut w, kb, _) = keyboard_session();
    repeat_info(&mut server, kb, 25, 600);
    key(&mut server, kb, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    assert_eq!(
        drain(&mut w),
        [typed('q', Modifiers::NONE, KeyEventKind::Press)]
    );
    let due = w.repeat_deadline().expect("a held q repeats");
    w.tick(due - MS);
    assert!(drain(&mut w).is_empty(), "nothing before the delay");
    w.tick(due + 80 * MS);
    assert_eq!(
        drain(&mut w),
        (0..3)
            .map(|_| typed('q', Modifiers::NONE, KeyEventKind::Repeat))
            .collect::<Vec<_>>(),
        "at the deadline and every 40 ms after it"
    );
    // Shift pressed while q is held: the repeat is the shifted key.
    depressed(&mut server, kb, 1);
    pump(&mut conn, &mut w);
    w.tick(due + 120 * MS);
    assert_eq!(
        drain(&mut w),
        [typed('Q', Modifiers::NONE, KeyEventKind::Repeat)]
    );
    key(&mut server, kb, KEY_Q, RELEASED);
    pump(&mut conn, &mut w);
    assert_eq!(
        drain(&mut w),
        [typed('Q', Modifiers::NONE, KeyEventKind::Release)],
        "the release is delivered"
    );
    assert_eq!(w.repeat_deadline(), None, "and the repeat stops");
    w.tick(due + std::time::Duration::from_secs(5));
    assert!(drain(&mut w).is_empty());
}

/// **What else stops a repeat, each on its own:** another key's press takes
/// the repeat over, a release of a key that is not the held one leaves it,
/// a key the keymap does not repeat (a modifier) leaves nothing repeating,
/// the keyboard leaving the surface, a new keymap, and a rate of zero.
#[test]
fn a_repeat_stops_where_a_keyboard_stops_it() {
    use baud_domain::{KeyCode, KeyEvent, KeyEventKind, Modifiers};
    let (mut conn, mut server, mut w, kb, surface) = keyboard_session();
    repeat_info(&mut server, kb, 25, 600);

    // Another key takes over, and releasing the first leaves it.
    key(&mut server, kb, KEY_Q, PRESSED);
    key(&mut server, kb, KEY_UP, PRESSED);
    key(&mut server, kb, KEY_Q, RELEASED);
    pump(&mut conn, &mut w);
    drain(&mut w);
    let due = w.repeat_deadline().expect("Up repeats");
    w.tick(due);
    assert_eq!(
        drain(&mut w),
        [Event::Key(KeyEvent {
            kind: KeyEventKind::Repeat,
            ..KeyEvent::press(KeyCode::Up, Modifiers::NONE)
        })]
    );

    // A modifier does not repeat: pressing it leaves nothing repeating.
    key(&mut server, kb, KEY_LEFTSHIFT, PRESSED);
    pump(&mut conn, &mut w);
    drain(&mut w);
    assert_eq!(w.repeat_deadline(), None, "Shift does not repeat");
    key(&mut server, kb, KEY_LEFTSHIFT, RELEASED);

    // The keyboard leaving the surface.
    key(&mut server, kb, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    assert!(w.repeat_deadline().is_some());
    server.send(
        kb,
        "wl_keyboard",
        "leave",
        &[Arg::Uint(2), Arg::Object(surface)],
    );
    pump(&mut conn, &mut w);
    assert_eq!(w.repeat_deadline(), None, "leave stops it");

    // A new keymap.
    key(&mut server, kb, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    assert!(w.repeat_deadline().is_some());
    send_keymap(&mut server, kb);
    pump(&mut conn, &mut w);
    assert_eq!(w.repeat_deadline(), None, "a new keymap stops it");

    // A rate of zero stops it, and keeps a later press from repeating.
    key(&mut server, kb, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    assert!(w.repeat_deadline().is_some());
    repeat_info(&mut server, kb, 0, 600);
    pump(&mut conn, &mut w);
    assert_eq!(w.repeat_deadline(), None, "rate zero stops it");
    key(&mut server, kb, KEY_Q, RELEASED);
    key(&mut server, kb, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    assert_eq!(w.repeat_deadline(), None, "and nothing repeats after");
}

/// **Before `repeat_info` there is no repeat**, and a loop blocked past many
/// periods types at most one second's worth, then waits a period from now.
#[test]
fn no_repeat_before_its_rate_and_no_burst_after_a_stall() {
    use baud_domain::{KeyEventKind, Modifiers};
    let (mut conn, mut server, mut w, kb, _) = keyboard_session();
    key(&mut server, kb, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    assert_eq!(w.repeat_deadline(), None, "no rate, no repeat");
    key(&mut server, kb, KEY_Q, RELEASED);
    repeat_info(&mut server, kb, 25, 600);
    key(&mut server, kb, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    drain(&mut w);
    let due = w.repeat_deadline().expect("repeats now");
    let late = due + std::time::Duration::from_secs(5);
    w.tick(late);
    assert_eq!(
        drain(&mut w),
        (0..25)
            .map(|_| typed('q', Modifiers::NONE, KeyEventKind::Repeat))
            .collect::<Vec<_>>(),
        "one second at 25 a second"
    );
    assert_eq!(w.repeat_deadline(), Some(late + 40 * MS));
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

/// **A title a program sets reaches the toplevel**, as `xdg_toplevel.set_title`
/// on the object the role created. And one set before the toplevel exists is
/// the one the role is created with, rather than the name the window opened
/// under.
#[test]
fn a_title_set_after_the_role_reaches_the_toplevel() {
    let (mut conn, mut server, mut w, _keyboard, _surface) = keyboard_session();
    w.set_title(&mut conn.sender(), "vim — main.rs");
    conn.flush().unwrap();
    let r = server.expect("xdg_toplevel", "set_title");
    assert!(
        matches!(&r.args[0], Val::Str(Some(t)) if t == "vim — main.rs"),
        "{:?}",
        r.args
    );
}

#[test]
fn a_title_set_before_the_role_is_the_one_it_is_created_with() {
    let (c, s) = UnixStream::pair().unwrap();
    let mut conn = Connection::over(c);
    let mut server = Server::new(s);
    let mut w = Window::open(&mut conn, "baud", "org.baud.terminal").unwrap();
    w.set_title(&mut conn.sender(), "ssh host");
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
    for (name, iface, version) in [
        (1u32, "wl_compositor", 6u32),
        (2, "wl_shm", 2),
        (3, "xdg_wm_base", 6),
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
    loop {
        let r = server.next();
        if (r.iface, r.name) == ("xdg_toplevel", "set_title") {
            assert!(
                matches!(&r.args[0], Val::Str(Some(t)) if t == "ssh host"),
                "{:?}",
                r.args
            );
            break;
        }
    }
}

/// A session through its first configure, `width` x `height` with `states`:
/// the toplevel and its xdg surface, the `Configured` event drained.
fn configured(states: &[u32]) -> (Connection, Server, Window, ObjectId, ObjectId) {
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
    for (name, iface, version) in [
        (1u32, "wl_compositor", 6u32),
        (2, "wl_shm", 2),
        (3, "xdg_wm_base", 6),
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
    let (mut xdg, mut top) = (None, None);
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            ("xdg_wm_base", "get_xdg_surface", [Val::New(id), _]) => xdg = Some(*id),
            ("xdg_surface", "get_toplevel", [Val::New(id)]) => top = Some(*id),
            ("wl_surface", "commit", _) => break,
            _ => {}
        }
    }
    let (xdg, top) = (xdg.expect("xdg surface"), top.expect("toplevel"));
    configure(&mut server, top, xdg, 640, 480, states);
    pump(&mut conn, &mut w);
    server.expect("xdg_surface", "ack_configure");
    assert!(matches!(w.next_event(), Some(Event::Configured { .. })));
    (conn, server, w, top, xdg)
}

fn configure(server: &mut Server, top: ObjectId, xdg: ObjectId, w: i32, h: i32, states: &[u32]) {
    let bytes: Vec<u8> = states.iter().flat_map(|s| s.to_ne_bytes()).collect();
    server.send(
        top,
        "xdg_toplevel",
        "configure",
        &[Arg::Int(w), Arg::Int(h), Arg::Array(&bytes)],
    );
    server.send(xdg, "xdg_surface", "configure", &[Arg::Uint(9)]);
}

/// **The window operations xdg-shell has reach the toplevel as its
/// requests**: `set_minimized`, `set_maximized` and `unset_maximized`,
/// `set_fullscreen` on no output in particular, and `unset_fullscreen`.
#[test]
fn the_window_operations_reach_the_toplevel_as_its_requests() {
    let (mut conn, mut server, mut w, top, _xdg) = configured(&[]);
    w.minimize(&mut conn.sender());
    w.maximize(&mut conn.sender(), Toggle::Set);
    w.maximize(&mut conn.sender(), Toggle::Unset);
    w.full_screen(&mut conn.sender(), Toggle::Set);
    w.full_screen(&mut conn.sender(), Toggle::Unset);
    conn.flush().unwrap();
    for name in [
        "set_minimized",
        "set_maximized",
        "unset_maximized",
        "set_fullscreen",
        "unset_fullscreen",
    ] {
        let r = server.expect("xdg_toplevel", name);
        assert_eq!(r.sender, top, "{name} on the toplevel");
        if name == "set_fullscreen" {
            assert!(
                matches!(r.args[..], [Val::Obj(o)] if o == ObjectId::NULL),
                "{:?}",
                r.args
            );
        }
    }
}

/// **The configure's states say whose size it is**: floating, the window
/// takes a size it chooses and reports it as a configure would; maximised
/// or full screen the compositor's binds, and full screen wins when both
/// are set. The states are those of the configure acknowledged.
#[test]
fn a_floating_window_takes_its_own_size_and_a_maximised_one_does_not() {
    let (mut conn, mut server, mut w, top, xdg) = configured(&[]);
    assert_eq!(w.shown(), Shown::Floating);
    assert_eq!(w.resize(Px(800), Px(600)), Resize::Taken);
    assert_eq!(
        w.next_event(),
        Some(Event::Configured {
            width: Px(800),
            height: Px(600)
        })
    );
    assert_eq!(w.size(), Some((Px(800), Px(600))));
    let maximized = protocol::xdg_toplevel::state::MAXIMIZED;
    let full = protocol::xdg_toplevel::state::FULLSCREEN;
    let activated = protocol::xdg_toplevel::state::ACTIVATED;
    for (states, shown) in [
        (&[activated, maximized][..], Shown::Maximized),
        (&[maximized, full][..], Shown::FullScreen),
        (&[activated][..], Shown::Floating),
    ] {
        configure(&mut server, top, xdg, 1920, 1080, states);
        pump(&mut conn, &mut w);
        server.expect("xdg_surface", "ack_configure");
        while w.next_event().is_some() {}
        assert_eq!(w.shown(), shown, "{states:?}");
        let want = match shown {
            Shown::Floating => Resize::Taken,
            _ => Resize::Bound,
        };
        assert_eq!(w.resize(Px(400), Px(300)), want, "{states:?}");
    }
}

/// `xdg_toplevel.configure_bounds`: the size the compositor recommends
/// staying inside, known once sent and unknown again on "0".
#[test]
fn the_bounds_are_the_compositors_until_it_says_unknown() {
    let (mut conn, mut server, mut w, top, _xdg) = configured(&[]);
    assert_eq!(w.bounds(), None);
    server.send(
        top,
        "xdg_toplevel",
        "configure_bounds",
        &[Arg::Int(2560), Arg::Int(1400)],
    );
    pump(&mut conn, &mut w);
    assert_eq!(w.bounds(), Some((Px(2560), Px(1400))));
    server.send(
        top,
        "xdg_toplevel",
        "configure_bounds",
        &[Arg::Int(0), Arg::Int(0)],
    );
    pump(&mut conn, &mut w);
    assert_eq!(w.bounds(), None);
}
