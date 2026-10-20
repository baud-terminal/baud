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

/// Whether a session's compositor offers the cursor-shape global.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shapes {
    Offered,
    Absent,
}

/// A session with a pointer, as [`pointer_session`] opened it.
struct Pointed {
    conn: Connection,
    server: Server,
    w: Window,
    pointer: ObjectId,
    /// The shape device, if one was made.
    device: Option<ObjectId>,
    surface: ObjectId,
}

/// A session with a pointer: the globals bound -- `Shapes::Offered` adds
/// the cursor-shape manager, advertised at 2 -- the seat at `seat_version`,
/// and its capabilities naming the keyboard and the pointer.
fn pointer_session(seat_version: u32, shapes: Shapes) -> Pointed {
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
    let mut globals = vec![
        (1u32, "wl_compositor", 6u32),
        (2, "wl_shm", 2),
        (3, "xdg_wm_base", 6),
        (4, "wl_seat", seat_version),
    ];
    if shapes == Shapes::Offered {
        globals.push((5, "wp_cursor_shape_manager_v1", 2));
    }
    for (name, iface, version) in globals {
        server.send(
            registry,
            "wl_registry",
            "global",
            &[Arg::Uint(name), Arg::Str(Some(iface)), Arg::Uint(version)],
        );
    }
    server.send(sync, "wl_callback", "done", &[Arg::Uint(1)]);
    pump(&mut conn, &mut w);
    let (mut seat, mut surface, mut manager) = (None, None, None);
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            ("wl_registry", "bind", [_, Val::NewWith(i, v, id)]) if i == "wl_seat" => {
                assert_eq!(*v, seat_version.min(9));
                seat = Some(*id);
            }
            ("wl_registry", "bind", [_, Val::NewWith(i, v, id)])
                if i == "wp_cursor_shape_manager_v1" =>
            {
                assert_eq!(*v, 1, "the version whose shapes are the ones named");
                manager = Some(*id);
            }
            ("wl_compositor", "create_surface", [Val::New(id)]) => surface = Some(*id),
            ("wl_surface", "commit", _) => break,
            _ => {}
        }
    }
    assert_eq!(manager.is_some(), shapes == Shapes::Offered);
    let (seat, surface) = (seat.expect("the seat"), surface.expect("the surface"));
    server.send(
        seat,
        "wl_seat",
        "capabilities",
        &[Arg::Uint(
            protocol::wl_seat::capability::KEYBOARD | protocol::wl_seat::capability::POINTER,
        )],
    );
    pump(&mut conn, &mut w);
    server.expect("wl_seat", "get_keyboard");
    let r = server.expect("wl_seat", "get_pointer");
    let Val::New(pointer) = r.args[0] else {
        panic!()
    };
    let device = manager.map(|m| {
        let r = server.expect("wp_cursor_shape_manager_v1", "get_pointer");
        assert_eq!(r.sender, m);
        assert!(matches!(r.args[1], Val::Obj(p) if p == pointer));
        let Val::New(d) = r.args[0] else { panic!() };
        d
    });
    Pointed {
        conn,
        server,
        w,
        pointer,
        device,
        surface,
    }
}

fn fixed(v: f64) -> Arg<'static> {
    Arg::Fixed(baud_wayland::Fixed::from_f64(v))
}

fn pointer_drain(w: &mut Window) -> Vec<PointerEvent> {
    drain(w)
        .into_iter()
        .map(|e| match e {
            Event::Pointer(p) => p,
            other => panic!("not the pointer's: {other:?}"),
        })
        .collect()
}

const VERTICAL: u32 = protocol::wl_pointer::axis::VERTICAL_SCROLL;

/// **The pointer arrives a frame at a time** (`[[source: wayland]]`,
/// `wl_pointer.frame`): an event waits for the frame that closes its
/// group; enter, motion and the buttons keep their order, with the
/// kernel's button code and the event's stamp; a wheel's `axis_value120`
/// is its detents, a touchpad's `axis` its length, and `axis_stop` follows
/// the scroll it ends. The shape is set with the last enter's serial and
/// not once the pointer has left.
#[test]
fn the_pointer_reports_by_frame_and_takes_its_shape() {
    let Pointed {
        mut conn,
        mut server,
        mut w,
        pointer,
        device,
        surface,
    } = pointer_session(9, Shapes::Offered);
    let device = device.expect("the shape device");
    assert_eq!(
        w.set_pointer_shape(&mut conn.sender(), CursorShape(9)),
        Shaped::Unavailable,
        "no enter, no serial"
    );
    server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[
            Arg::Uint(11),
            Arg::Object(surface),
            fixed(10.5),
            fixed(20.25),
        ],
    );
    pump(&mut conn, &mut w);
    assert_eq!(pointer_drain(&mut w), [], "held to the frame");
    server.send(pointer, "wl_pointer", "frame", &[]);
    pump(&mut conn, &mut w);
    assert_eq!(
        pointer_drain(&mut w),
        [PointerEvent::Entered(At { x: 10.5, y: 20.25 })]
    );
    let text = CursorShape(protocol::wp_cursor_shape_device_v1::shape::TEXT);
    assert_eq!(w.set_pointer_shape(&mut conn.sender(), text), Shaped::Set);
    conn.flush().unwrap();
    let r = server.expect("wp_cursor_shape_device_v1", "set_shape");
    assert_eq!(r.sender, device);
    assert!(
        matches!(r.args[..], [Val::Uint(11), Val::Uint(9)]),
        "{:?}",
        r.args
    );

    server.send(
        pointer,
        "wl_pointer",
        "motion",
        &[Arg::Uint(100), fixed(30.0), fixed(40.0)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    for state in [
        protocol::wl_pointer::button_state::PRESSED,
        protocol::wl_pointer::button_state::RELEASED,
    ] {
        server.send(
            pointer,
            "wl_pointer",
            "button",
            &[
                Arg::Uint(12),
                Arg::Uint(110),
                Arg::Uint(0x110),
                Arg::Uint(state),
            ],
        );
        server.send(pointer, "wl_pointer", "frame", &[]);
    }
    // A wheel's half detent, its length beside it, and its source.
    server.send(
        pointer,
        "wl_pointer",
        "axis_source",
        &[Arg::Uint(protocol::wl_pointer::axis_source::WHEEL)],
    );
    server.send(
        pointer,
        "wl_pointer",
        "axis_value120",
        &[Arg::Uint(VERTICAL), Arg::Int(60)],
    );
    server.send(
        pointer,
        "wl_pointer",
        "axis",
        &[Arg::Uint(120), Arg::Uint(VERTICAL), fixed(7.5)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    // A touchpad's last movement and its lift.
    server.send(
        pointer,
        "wl_pointer",
        "axis_source",
        &[Arg::Uint(protocol::wl_pointer::axis_source::FINGER)],
    );
    server.send(
        pointer,
        "wl_pointer",
        "axis",
        &[Arg::Uint(130), Arg::Uint(VERTICAL), fixed(-3.0)],
    );
    server.send(
        pointer,
        "wl_pointer",
        "axis_stop",
        &[Arg::Uint(130), Arg::Uint(VERTICAL)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    pump(&mut conn, &mut w);
    assert_eq!(
        pointer_drain(&mut w),
        [
            PointerEvent::Moved(At { x: 30.0, y: 40.0 }),
            PointerEvent::Button {
                button: Button::Left,
                press: Press::Pressed,
                time: baud_domain::EventMillis::of_millis(110)
            },
            PointerEvent::Button {
                button: Button::Left,
                press: Press::Released,
                time: baud_domain::EventMillis::of_millis(110)
            },
            PointerEvent::Scroll(Scroll::Steps(0.5)),
            PointerEvent::Scroll(Scroll::Distance(-3.0)),
            PointerEvent::ScrollStopped,
        ]
    );

    server.send(
        pointer,
        "wl_pointer",
        "leave",
        &[Arg::Uint(13), Arg::Object(surface)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    pump(&mut conn, &mut w);
    assert_eq!(pointer_drain(&mut w), [PointerEvent::Left]);
    assert_eq!(
        w.set_pointer_shape(&mut conn.sender(), text),
        Shaped::Unavailable,
        "the serial went with the pointer"
    );
}

/// **A seat that loses its pointer lets it go**: the shape device
/// destroyed, being inert, and the pointer released, its version having
/// the request; the pointer that was over the surface has left.
#[test]
fn a_seat_that_loses_its_pointer_releases_it() {
    let Pointed {
        mut conn,
        mut server,
        mut w,
        pointer,
        device,
        surface,
    } = pointer_session(9, Shapes::Offered);
    server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[Arg::Uint(1), Arg::Object(surface), fixed(0.0), fixed(0.0)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    pump(&mut conn, &mut w);
    drain(&mut w);
    let seat = server
        .objects
        .iter()
        .find(|(_, i)| i.name == "wl_seat")
        .map(|(id, _)| *id)
        .unwrap();
    server.send(
        seat,
        "wl_seat",
        "capabilities",
        &[Arg::Uint(protocol::wl_seat::capability::KEYBOARD)],
    );
    pump(&mut conn, &mut w);
    let r = server.expect("wp_cursor_shape_device_v1", "destroy");
    assert_eq!(Some(r.sender), device);
    let r = server.expect("wl_pointer", "release");
    assert_eq!(r.sender, pointer);
    assert_eq!(pointer_drain(&mut w), [PointerEvent::Left]);
}

/// **Below version 5 each event is its own frame**, and without the
/// cursor-shape global nothing names a shape.
#[test]
fn an_old_pointer_reports_at_once_and_no_global_draws_its_own() {
    let Pointed {
        mut conn,
        mut server,
        mut w,
        pointer,
        device,
        surface,
    } = pointer_session(4, Shapes::Absent);
    assert_eq!(device, None);
    use protocol::wp_cursor_shape_device_v1::shape;
    assert_eq!(
        w.set_pointer_shape(&mut conn.sender(), CursorShape(shape::TEXT)),
        Shaped::Unavailable,
        "no enter, no serial"
    );
    server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[Arg::Uint(3), Arg::Object(surface), fixed(1.0), fixed(2.0)],
    );
    server.send(
        pointer,
        "wl_pointer",
        "axis",
        &[Arg::Uint(1), Arg::Uint(VERTICAL), fixed(10.0)],
    );
    pump(&mut conn, &mut w);
    assert_eq!(
        pointer_drain(&mut w),
        [
            PointerEvent::Entered(At { x: 1.0, y: 2.0 }),
            PointerEvent::Scroll(Scroll::Distance(10.0))
        ]
    );
    let cursor = drawn_pointer(
        &mut conn,
        &mut server,
        &mut w,
        (CursorShape(shape::TEXT), 3),
        Drawing {
            side: 24,
            scale: 1,
            hotspot: (12, 12),
            image: baud_present_wayland::cursor::Drawn::IBeam,
        },
        None,
    );
    // The window's surface asks for twice the pixels: the next enter's
    // image is drawn again at two, on the same surface, and the arrow for a
    // shape that is not text.
    server.send(
        surface,
        "wl_surface",
        "preferred_buffer_scale",
        &[Arg::Int(2)],
    );
    server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[Arg::Uint(5), Arg::Object(surface), fixed(1.0), fixed(2.0)],
    );
    pump(&mut conn, &mut w);
    drain(&mut w);
    drawn_pointer(
        &mut conn,
        &mut server,
        &mut w,
        (CursorShape(shape::POINTER), 5),
        Drawing {
            side: 48,
            scale: 2,
            hotspot: (4, 3),
            image: baud_present_wayland::cursor::Drawn::Arrow,
        },
        Some(cursor),
    );
}

/// What one drawn pointer should come to: the image's side in pixels, its
/// scale, its hotspot, and which image, the pool holding the I-beam first.
struct Drawing {
    side: i32,
    scale: i32,
    hotspot: (i32, i32),
    image: baud_present_wayland::cursor::Drawn,
}

/// **The pointer is hidden with no surface** (`[[source: wayland]]`,
/// `wl_pointer.set_cursor`: "If surface is NULL, the pointer image is
/// hidden"), named with the last enter's serial, and not before an enter;
/// a shape set after shows it again, as XTSMPOINTER's motion does.
#[test]
fn the_pointer_hides_with_no_surface_and_a_shape_shows_it() {
    let Pointed {
        mut conn,
        mut server,
        mut w,
        pointer,
        device,
        surface,
    } = pointer_session(9, Shapes::Offered);
    let device = device.expect("the shape device");
    assert_eq!(
        w.hide_pointer(&mut conn.sender()),
        Shaped::Unavailable,
        "no enter, no serial"
    );
    server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[Arg::Uint(7), Arg::Object(surface), fixed(1.0), fixed(2.0)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    pump(&mut conn, &mut w);
    pointer_drain(&mut w);
    assert_eq!(w.hide_pointer(&mut conn.sender()), Shaped::Set);
    conn.flush().unwrap();
    let r = server.expect("wl_pointer", "set_cursor");
    assert_eq!(r.sender, pointer);
    assert!(
        matches!(r.args[..], [Val::Uint(7), Val::Obj(c), Val::Int(0), Val::Int(0)]
            if c == ObjectId::NULL),
        "{:?}",
        r.args
    );
    let text = CursorShape(protocol::wp_cursor_shape_device_v1::shape::TEXT);
    assert_eq!(w.set_pointer_shape(&mut conn.sender(), text), Shaped::Set);
    conn.flush().unwrap();
    let r = server.expect("wp_cursor_shape_device_v1", "set_shape");
    assert_eq!(r.sender, device);
    assert!(
        matches!(r.args[..], [Val::Uint(7), Val::Uint(_)]),
        "{:?}",
        r.args
    );
}

/// **The requests a drawn pointer makes, and its pixels**: the cursor's
/// surface made once, a pool of two images at the scale -- the old pool
/// destroyed when the scale changed -- the image attached at its scale and
/// damaged in logical pixels, the cursor role given with the enter's
/// serial and the hotspot, and a commit. The pixels are read back from the
/// pool's file: the image's hotspot pixel is opaque black. Returns the
/// cursor's surface.
fn drawn_pointer(
    conn: &mut Connection,
    server: &mut Server,
    w: &mut Window,
    (shape, serial): (CursorShape, u32),
    want: Drawing,
    surface: Option<ObjectId>,
) -> ObjectId {
    use std::os::unix::fs::FileExt as _;
    assert_eq!(
        w.set_pointer_shape(&mut conn.sender(), shape),
        Shaped::Drawn
    );
    conn.flush().unwrap();
    let cursor = match surface {
        Some(c) => {
            server.expect("wl_buffer", "destroy");
            server.expect("wl_buffer", "destroy");
            server.expect("wl_shm_pool", "destroy");
            c
        }
        None => {
            let r = server.expect("wl_compositor", "create_surface");
            let Val::New(c) = r.args[0] else { panic!() };
            c
        }
    };
    server.expect("wl_shm", "create_pool");
    let pool = std::fs::File::from(server.fds.pop_front().expect("the pool's file"));
    let mut buffers = Vec::new();
    for _ in 0..2 {
        let r = server.expect("wl_shm_pool", "create_buffer");
        let [
            Val::New(id),
            Val::Int(offset),
            Val::Int(wide),
            Val::Int(high),
            Val::Int(stride),
            Val::Uint(format),
        ] = r.args[..]
        else {
            panic!("{:?}", r.args)
        };
        assert_eq!((wide, high), (want.side, want.side));
        assert_eq!(format, protocol::wl_shm::format::ARGB8888);
        buffers.push((id, offset, stride));
    }
    let r = server.expect("wl_surface", "attach");
    assert_eq!(r.sender, cursor);
    let (buffer, offset, stride) = match want.image {
        baud_present_wayland::cursor::Drawn::IBeam => buffers[0],
        baud_present_wayland::cursor::Drawn::Arrow => buffers[1],
    };
    assert!(
        matches!(r.args[0], Val::Obj(b) if b == buffer),
        "{:?}",
        r.args
    );
    let r = server.expect("wl_surface", "set_buffer_scale");
    assert!(matches!(r.args[..], [Val::Int(s)] if s == want.scale));
    let r = server.expect("wl_surface", "damage");
    assert!(matches!(
        r.args[..],
        [Val::Int(0), Val::Int(0), Val::Int(24), Val::Int(24)]
    ));
    let r = server.expect("wl_pointer", "set_cursor");
    let (hx, hy) = want.hotspot;
    assert!(
        matches!(r.args[..], [Val::Uint(n), Val::Obj(c), Val::Int(x), Val::Int(y)]
            if n == serial && c == cursor && (x, y) == (hx, hy)),
        "{:?}",
        r.args
    );
    let r = server.expect("wl_surface", "commit");
    assert_eq!(r.sender, cursor);
    // The pixel just below the hotspot, in the image's own pixels.
    let (px, py) = (hx * want.scale, hy * want.scale + want.scale);
    let at = u64::try_from(offset + py * stride + px * 4).unwrap();
    let mut word = [0u8; 4];
    pool.read_exact_at(&mut word, at).unwrap();
    assert_eq!(
        u32::from_ne_bytes(word),
        0xff00_0000,
        "opaque black at the hotspot"
    );
    cursor
}

/// **The modifiers a button reads are the keyboard's**: the effective
/// state the last `modifiers` event left, Shift and Control here.
#[test]
fn a_button_reads_the_keyboards_modifiers() {
    use baud_domain::Modifiers;
    let (mut conn, mut server, mut w, kb, _) = keyboard_session();
    assert_eq!(w.modifiers(), Modifiers::NONE);
    depressed(&mut server, kb, (1 << 0) | (1 << 2));
    pump(&mut conn, &mut w);
    assert_eq!(
        w.modifiers(),
        Modifiers::NONE.with(Modifiers::SHIFT).with(Modifiers::CTRL)
    );
}

/// **The keyboard's leave lifts the modifiers it held**: "In the
/// wl_keyboard logical state, this event resets all values to their
/// defaults" (`[[source: wayland]]`, `wl_keyboard.leave`), and a pointer
/// over an unfocused window reads them, so Alt held into an Alt-Tab is not
/// Meta on the wheel that follows.
#[test]
fn the_keyboards_leave_lifts_its_modifiers() {
    use baud_domain::Modifiers;
    let (mut conn, mut server, mut w, kb, surface) = keyboard_session();
    depressed(&mut server, kb, (1 << 0) | (1 << 3));
    pump(&mut conn, &mut w);
    assert_eq!(
        w.modifiers(),
        Modifiers::NONE.with(Modifiers::SHIFT).with(Modifiers::ALT),
        "held before the leave"
    );
    server.send(
        kb,
        "wl_keyboard",
        "leave",
        &[Arg::Uint(2), Arg::Object(surface)],
    );
    pump(&mut conn, &mut w);
    assert_eq!(w.modifiers(), Modifiers::NONE, "lifted by the leave");
}

/// A session with both selections: the globals bound -- the data device
/// manager advertised at 4, the primary-selection manager at 1 -- the
/// seat's two devices asked for, the keyboard made and its keymap sent.
struct Clipped {
    conn: Connection,
    server: Server,
    w: Window,
    keyboard: ObjectId,
    surface: ObjectId,
    data_manager: ObjectId,
    data_device: ObjectId,
    primary_manager: ObjectId,
    primary_device: ObjectId,
}

fn clip_session() -> Clipped {
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
        (5, "wl_data_device_manager", 4),
        (6, "zwp_primary_selection_device_manager_v1", 1),
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
    let mut ids: HashMap<String, ObjectId> = HashMap::new();
    let mut versions: HashMap<String, u32> = HashMap::new();
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            ("wl_registry", "bind", [_, Val::NewWith(i, v, id)]) => {
                ids.insert(i.clone(), *id);
                versions.insert(i.clone(), *v);
            }
            ("wl_data_device_manager", "get_data_device", [Val::New(d), Val::Obj(seat)]) => {
                assert_eq!(Some(seat), ids.get("wl_seat"), "the seat's device");
                ids.insert("data_device".into(), *d);
            }
            (
                "zwp_primary_selection_device_manager_v1",
                "get_device",
                [Val::New(d), Val::Obj(seat)],
            ) => {
                assert_eq!(Some(seat), ids.get("wl_seat"), "the seat's device");
                ids.insert("primary_device".into(), *d);
            }
            ("wl_compositor", "create_surface", [Val::New(id)]) => {
                ids.insert("surface".into(), *id);
            }
            ("wl_surface", "commit", _) => break,
            _ => {}
        }
    }
    assert_eq!(versions["wl_data_device_manager"], 3, "foot's version");
    assert_eq!(versions["zwp_primary_selection_device_manager_v1"], 1);
    server.send(
        ids["wl_seat"],
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
    Clipped {
        conn,
        server,
        w,
        keyboard,
        surface: ids["surface"],
        data_manager: ids["wl_data_device_manager"],
        data_device: ids["data_device"],
        primary_manager: ids["zwp_primary_selection_device_manager_v1"],
        primary_device: ids["primary_device"],
    }
}

/// A key event with its own serial.
fn key_at(server: &mut Server, keyboard: ObjectId, serial: u32, code: u32, state: u32) {
    server.send(
        keyboard,
        "wl_keyboard",
        "key",
        &[
            Arg::Uint(serial),
            Arg::Uint(0),
            Arg::Uint(code),
            Arg::Uint(state),
        ],
    );
}

/// Larger than any pipe's buffer, so both directions meet a full pipe.
fn big_text() -> String {
    (0..300_000u32)
        .map(|i| char::from(b'a' + u8::try_from(i % 26).unwrap()))
        .collect::<String>()
        + "\u{e9}"
}

/// Pump `w`'s transfers until `done` says so or five seconds pass.
fn pump_until(w: &mut Window, mut done: impl FnMut(&mut Window) -> bool) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !done(w) {
        assert!(std::time::Instant::now() < until, "the transfer stalled");
        w.pump_transfers(std::time::Instant::now());
        std::thread::yield_now();
    }
}

/// **A copy is a fresh source offering foot's five text types, set with
/// the serial of the event that asked, and its text goes out through the
/// pipe a `send` hands over** -- all of it, past a full pipe, and the pipe
/// closed at the end so the reader sees it. A copy before any input event
/// has no serial to name and is not made; a second copy replaces the
/// first source, destroyed; a `cancelled` source is destroyed and its text
/// is no longer pasted from.
#[test]
fn a_copy_offers_text_and_answers_send_through_the_pipe() {
    let Clipped {
        mut conn,
        mut server,
        mut w,
        keyboard,
        data_manager,
        data_device,
        ..
    } = clip_session();
    assert_eq!(
        w.copy(&mut conn.sender(), Board::Clipboard, "early"),
        Copied::Unavailable,
        "no input event yet, so no serial"
    );
    key_at(&mut server, keyboard, 42, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    drain(&mut w);
    let text = big_text();
    assert_eq!(
        w.copy(&mut conn.sender(), Board::Clipboard, &text),
        Copied::Set
    );
    conn.flush().unwrap();
    let r = server.expect("wl_data_device_manager", "create_data_source");
    assert_eq!(r.sender, data_manager);
    let Val::New(source) = r.args[0] else {
        panic!()
    };
    let mut offered = Vec::new();
    for _ in 0..5 {
        let r = server.expect("wl_data_source", "offer");
        assert_eq!(r.sender, source);
        let Val::Str(Some(m)) = &r.args[0] else {
            panic!()
        };
        offered.push(m.clone());
    }
    assert_eq!(
        offered,
        [
            "text/plain;charset=utf-8",
            "text/plain",
            "UTF8_STRING",
            "TEXT",
            "STRING"
        ]
    );
    let r = server.expect("wl_data_device", "set_selection");
    assert_eq!(r.sender, data_device);
    assert!(
        matches!(r.args[..], [Val::Obj(s), Val::Uint(42)] if s == source),
        "{:?}",
        r.args
    );

    // The compositor asks for it; another client reads the pipe.
    let (mut reader, writer) = std::io::pipe().unwrap();
    server.send(
        source,
        "wl_data_source",
        "send",
        &[
            Arg::Str(Some("text/plain;charset=utf-8")),
            Arg::Fd(writer.as_fd()),
        ],
    );
    drop(writer);
    let got = std::thread::spawn(move || {
        let mut got = Vec::new();
        reader.read_to_end(&mut got).unwrap();
        got
    });
    pump(&mut conn, &mut w);
    assert_eq!(w.transfers().count(), 1, "the send is waited on");
    pump_until(&mut w, |w| w.transfers().count() == 0);
    assert_eq!(got.join().unwrap(), text.as_bytes(), "every byte, then EOF");

    // Copied again: a fresh source, and the old one destroyed.
    assert_eq!(
        w.copy(&mut conn.sender(), Board::Clipboard, "second"),
        Copied::Set
    );
    conn.flush().unwrap();
    let r = server.expect("wl_data_device_manager", "create_data_source");
    let Val::New(second) = r.args[0] else {
        panic!()
    };
    assert_ne!(second, source, "a used source is never set again");
    for _ in 0..5 {
        server.expect("wl_data_source", "offer");
    }
    server.expect("wl_data_device", "set_selection");
    let r = server.expect("wl_data_source", "destroy");
    assert_eq!(r.sender, source);

    // Another client copied: the source is cancelled and destroyed, and a
    // paste no longer reads it.
    server.send(second, "wl_data_source", "cancelled", &[]);
    pump(&mut conn, &mut w);
    let r = server.expect("wl_data_source", "destroy");
    assert_eq!(r.sender, second);
    assert_eq!(
        w.paste(&mut conn.sender(), Board::Clipboard),
        Pasting::Nothing
    );
}

/// The offer the compositor makes on `board`'s device, as its three
/// events.
fn offer(
    server: &mut Server,
    device: ObjectId,
    board: Board,
    offer: ObjectId,
    mimes: &[&str],
) -> ObjectId {
    let (dev, off) = match board {
        Board::Primary => (
            "zwp_primary_selection_device_v1",
            "zwp_primary_selection_offer_v1",
        ),
        Board::Clipboard => ("wl_data_device", "wl_data_offer"),
    };
    server.objects.insert(offer, Interface::named(off).unwrap());
    server.send(device, dev, "data_offer", &[Arg::NewId(offer)]);
    for m in mimes {
        server.send(offer, off, "offer", &[Arg::Str(Some(m))]);
    }
    server.send(device, dev, "selection", &[Arg::Object(offer)]);
    offer
}

/// **A paste asks the named offer for its best text type and reads the
/// pipe to its end**: UTF-8 before plain text, an image never; the pipe's
/// write end given to the compositor is not kept, or the end would never
/// come; a new selection destroys the old offer, and the keyboard's leave
/// destroys the one it had, as the protocol says each is valid until.
#[test]
fn a_paste_reads_the_offer_to_its_end() {
    let Clipped {
        mut conn,
        mut server,
        mut w,
        keyboard,
        surface,
        data_device,
        ..
    } = clip_session();
    let first = offer(
        &mut server,
        data_device,
        Board::Clipboard,
        ObjectId(0xff00_0001),
        &[
            "text/plain",
            "image/png",
            "text/plain;charset=utf-8",
            "TEXT",
        ],
    );
    pump(&mut conn, &mut w);
    assert_eq!(
        w.paste(&mut conn.sender(), Board::Clipboard),
        Pasting::Started
    );
    assert_eq!(
        w.paste(&mut conn.sender(), Board::Clipboard),
        Pasting::Nothing,
        "one paste at a time"
    );
    conn.flush().unwrap();
    let r = server.expect("wl_data_offer", "receive");
    assert_eq!(r.sender, first);
    assert!(
        matches!(&r.args[..], [Val::Str(Some(m)), Val::Fd] if m == "text/plain;charset=utf-8"),
        "{:?}",
        r.args
    );
    let fd = server.fds.pop_front().expect("the pipe's write end");
    let text = big_text();
    let sent = text.clone();
    let owner = std::thread::spawn(move || {
        let mut f = std::fs::File::from(fd);
        f.write_all(sent.as_bytes()).unwrap();
    });
    let mut pasted = Vec::new();
    pump_until(&mut w, |w| {
        pasted.extend(drain(w));
        !pasted.is_empty()
    });
    owner.join().unwrap();
    assert_eq!(pasted, [Event::Pasted(Board::Clipboard, text)]);

    // A new selection: the old offer goes.
    let second = offer(
        &mut server,
        data_device,
        Board::Clipboard,
        ObjectId(0xff00_0002),
        &["UTF8_STRING"],
    );
    pump(&mut conn, &mut w);
    let r = server.expect("wl_data_offer", "destroy");
    assert_eq!(r.sender, first);
    // The keyboard leaves: the offer it had goes too.
    server.send(
        keyboard,
        "wl_keyboard",
        "leave",
        &[Arg::Uint(9), Arg::Object(surface)],
    );
    pump(&mut conn, &mut w);
    let r = server.expect("wl_data_offer", "destroy");
    assert_eq!(r.sender, second);
    assert_eq!(
        w.paste(&mut conn.sender(), Board::Clipboard),
        Pasting::Nothing
    );
}

/// **The primary is the same shape on its own protocol**, and a paste of a
/// selection this client owns is its own text at once, with no transfer:
/// the compositor would only hand it back to itself.
#[test]
fn the_primary_copies_and_pastes_on_its_own_protocol() {
    let Clipped {
        mut conn,
        mut server,
        mut w,
        keyboard,
        primary_manager,
        primary_device,
        ..
    } = clip_session();
    key_at(&mut server, keyboard, 7, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    drain(&mut w);
    assert_eq!(
        w.copy(&mut conn.sender(), Board::Primary, "hello"),
        Copied::Set
    );
    conn.flush().unwrap();
    let r = server.expect("zwp_primary_selection_device_manager_v1", "create_source");
    assert_eq!(r.sender, primary_manager);
    let Val::New(source) = r.args[0] else {
        panic!()
    };
    for _ in 0..5 {
        server.expect("zwp_primary_selection_source_v1", "offer");
    }
    let r = server.expect("zwp_primary_selection_device_v1", "set_selection");
    assert_eq!(r.sender, primary_device);
    assert!(matches!(r.args[..], [Val::Obj(s), Val::Uint(7)] if s == source));
    assert_eq!(
        w.paste(&mut conn.sender(), Board::Primary),
        Pasting::Started
    );
    assert_eq!(
        drain(&mut w),
        [Event::Pasted(Board::Primary, "hello".into())]
    );
    assert_eq!(w.transfers().count(), 0, "no pipe for its own text");

    // Another client's primary, pasted through the pipe.
    server.send(source, "zwp_primary_selection_source_v1", "cancelled", &[]);
    let theirs = offer(
        &mut server,
        primary_device,
        Board::Primary,
        ObjectId(0xff00_0003),
        &["text/plain"],
    );
    pump(&mut conn, &mut w);
    server.expect("zwp_primary_selection_source_v1", "destroy");
    assert_eq!(
        w.paste(&mut conn.sender(), Board::Primary),
        Pasting::Started
    );
    conn.flush().unwrap();
    let r = server.expect("zwp_primary_selection_offer_v1", "receive");
    assert_eq!(r.sender, theirs);
    let fd = server.fds.pop_front().unwrap();
    std::fs::File::from(fd).write_all(b"theirs").unwrap();
    let mut pasted = Vec::new();
    pump_until(&mut w, |w| {
        pasted.extend(drain(w));
        !pasted.is_empty()
    });
    assert_eq!(pasted, [Event::Pasted(Board::Primary, "theirs".into())]);

    // An empty primary: a NULL source.
    assert_eq!(w.clear(&mut conn.sender(), Board::Primary), Copied::Set);
    conn.flush().unwrap();
    let r = server.expect("zwp_primary_selection_device_v1", "set_selection");
    assert!(matches!(
        r.args[..],
        [Val::Obj(ObjectId::NULL), Val::Uint(7)]
    ));
}

/// **A paste whose owner sends nothing for two seconds is abandoned**,
/// foot's timeout: the deadline is the driver's to wait to, and a pump at
/// it reports the paste dropped and frees the next.
#[test]
fn a_silent_owner_is_abandoned_after_two_seconds() {
    let Clipped {
        mut conn,
        mut server,
        mut w,
        data_device,
        ..
    } = clip_session();
    offer(
        &mut server,
        data_device,
        Board::Clipboard,
        ObjectId(0xff00_0001),
        &["TEXT"],
    );
    pump(&mut conn, &mut w);
    let asked = std::time::Instant::now();
    assert_eq!(
        w.paste(&mut conn.sender(), Board::Clipboard),
        Pasting::Started
    );
    conn.flush().unwrap();
    server.expect("wl_data_offer", "receive");
    let _held_open = server.fds.pop_front().unwrap();
    let deadline = w.transfer_deadline().expect("a deadline");
    assert!(deadline >= asked + std::time::Duration::from_secs(2));
    w.pump_transfers(deadline - MS);
    assert_eq!(drain(&mut w), [], "not yet");
    w.pump_transfers(deadline);
    assert_eq!(drain(&mut w), [Event::PasteAbandoned(Board::Clipboard)]);
    assert_eq!(w.transfer_deadline(), None);
    assert_eq!(
        w.paste(&mut conn.sender(), Board::Clipboard),
        Pasting::Started,
        "the next paste may start"
    );
}

// evdev codes for the shortcuts.
const KEY_C: u32 = 46;
const KEY_V: u32 = 47;
const KEY_INSERT: u32 = 110;

/// **The clipboard's keys are the terminal's**: Ctrl+Shift+C, Ctrl+Shift+V
/// and Shift+Insert arrive as shortcuts, not keys, repeat nothing and
/// release nothing; Ctrl+C, Ctrl+Caps Lock+C and Ctrl+Alt+Shift+C are keys
/// as before, the chord being exact.
#[test]
fn the_clipboard_keys_are_kept_from_the_program() {
    use baud_domain::{KeyEventKind, Modifiers};
    let Clipped {
        mut conn,
        mut server,
        mut w,
        keyboard: kb,
        ..
    } = clip_session();
    repeat_info(&mut server, kb, 25, 600);
    let (shift, lock, ctrl, alt) = (1u32, 2u32, 4u32, 8u32);
    for (mask, code, want) in [
        (ctrl | shift, KEY_C, Shortcut::Copy),
        (ctrl | shift, KEY_V, Shortcut::Paste),
        (shift, KEY_INSERT, Shortcut::PasteSelection),
    ] {
        depressed(&mut server, kb, mask);
        key_at(&mut server, kb, 3, code, PRESSED);
        pump(&mut conn, &mut w);
        assert_eq!(drain(&mut w), [Event::Shortcut(want)], "{want:?}");
        assert_eq!(w.repeat_deadline(), None, "a shortcut does not repeat");
        key_at(&mut server, kb, 4, code, RELEASED);
        pump(&mut conn, &mut w);
        assert_eq!(drain(&mut w), [], "nor release");
    }
    depressed(&mut server, kb, ctrl);
    key_at(&mut server, kb, 5, KEY_C, PRESSED);
    pump(&mut conn, &mut w);
    assert_eq!(
        drain(&mut w),
        [typed(
            'c',
            Modifiers::NONE.with(Modifiers::CTRL),
            KeyEventKind::Press
        )]
    );
    key_at(&mut server, kb, 6, KEY_C, RELEASED);
    server.send(
        kb,
        "wl_keyboard",
        "modifiers",
        &[
            Arg::Uint(1),
            Arg::Uint(ctrl),
            Arg::Uint(0),
            Arg::Uint(lock),
            Arg::Uint(0),
        ],
    );
    key_at(&mut server, kb, 7, KEY_C, PRESSED);
    key_at(&mut server, kb, 8, KEY_C, RELEASED);
    depressed(&mut server, kb, ctrl | alt | shift);
    key_at(&mut server, kb, 9, KEY_C, PRESSED);
    pump(&mut conn, &mut w);
    let got = drain(&mut w);
    assert!(
        got.iter().all(|e| matches!(e, Event::Key(_))),
        "keys, not shortcuts: {got:?}"
    );
    assert_eq!(got.len(), 4, "{got:?}");
}

/// **No manager, no copy**: a compositor without the globals gets no
/// device requests, and a copy or a paste is refused rather than sent.
#[test]
fn without_the_globals_nothing_is_copied() {
    let (mut conn, mut server, mut w, kb, _) = keyboard_session();
    key_at(&mut server, kb, 3, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    for b in [Board::Clipboard, Board::Primary] {
        assert!(!w.has(b));
        assert_eq!(w.copy(&mut conn.sender(), b, "x"), Copied::Unavailable);
        assert_eq!(w.paste(&mut conn.sender(), b), Pasting::Nothing);
    }
}
