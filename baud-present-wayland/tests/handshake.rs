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
    Array(Vec<u8>),
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
                            Read::Array(a) => Val::Array(a.to_vec()),
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
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 320, Changed::Whole),
        Presented::Yes
    );
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 320, Changed::Whole),
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
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 320, Changed::Whole),
        Presented::Yes
    );
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

/// The pixel words of buffer `i` of a pool of `w` x `h`, read from the
/// pool's file as the compositor would.
fn buffer_words(file: &std::fs::File, i: usize, w: usize, h: usize) -> Vec<u32> {
    use std::os::unix::fs::FileExt as _;
    let mut bytes = vec![0u8; w * 4 * h];
    file.read_exact_at(&mut bytes, u64::try_from(i * w * 4 * h).unwrap())
        .unwrap();
    bytes
        .chunks_exact(4)
        .map(|b| u32::from_ne_bytes(b.try_into().unwrap()))
        .collect()
}

/// The `damage_buffer` rectangles of one frame, up to its commit, and
/// whether it attached a buffer.
fn frame_requests(server: &mut Server) -> (Option<ObjectId>, Vec<[i32; 4]>, ObjectId) {
    let (mut attached, mut damage, mut callback) = (None, Vec::new(), None);
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            ("wl_surface", "attach", [Val::Obj(b), ..]) => attached = Some(*b),
            (
                "wl_surface",
                "damage_buffer",
                [Val::Int(x), Val::Int(y), Val::Int(w), Val::Int(h)],
            ) => {
                damage.push([*x, *y, *w, *h]);
            }
            ("wl_surface", "frame", [Val::New(cb)]) => callback = Some(*cb),
            ("wl_surface", "commit", _) => break,
            ("wl_shm", "create_pool", _) | ("wl_shm_pool", "create_buffer", _) => {}
            (i, n, a) => panic!("{i}.{n} {a:?}"),
        }
    }
    (attached, damage, callback.expect("a frame callback"))
}

/// **A frame copies and damages only the rows it changed, and a buffer used
/// again is brought up to date first** (`damage.rs`). The first frame is
/// whole; the second goes to the new second buffer, which is copied whole
/// and damaged in its ten rows alone; the third reuses the first buffer,
/// which missed the second frame's rows, and copies those with its own --
/// twenty rows -- and damages its own ten. A compositor that releases at
/// once has the same buffer back missing nothing, and a frame refused while
/// one is in flight goes with the next. In each, the buffer's file holds the
/// whole frame. A frame that changed nothing attaches nothing and still
/// asks for the callback.
#[test]
fn a_frame_copies_and_damages_only_the_rows_it_changed() {
    let (mut conn, mut server, mut w, top, xdg) = configured(&[]);
    let (wu, hu) = (640usize, 480usize);
    let strip = |top: u32, bottom: u32| Strip {
        top: Px(top),
        bottom: Px(bottom),
    };
    let paint = |f: &mut [u32], top: usize, bottom: usize, px: u32| {
        f[top * wu..bottom * wu].fill(px);
    };
    let mut frame = vec![0xff10_2030u32; wu * hu];
    assert_eq!(
        w.present(&mut conn.sender(), &frame, wu, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    let r = server.expect("wl_shm", "create_pool");
    let Val::Int(size) = r.args[2] else { panic!() };
    assert_eq!(size, 2 * 2560 * 480);
    let file = std::fs::File::from(server.fds.pop_front().unwrap());
    let mut buffers = Vec::new();
    for _ in 0..2 {
        let r = server.expect("wl_shm_pool", "create_buffer");
        let Val::New(b) = r.args[0] else { panic!() };
        buffers.push(b);
    }
    let (attached, damage, cb) = frame_requests(&mut server);
    assert_eq!(attached, Some(buffers[0]));
    assert_eq!(damage, [[0, 0, 640, 480]], "the first frame is whole");
    assert_eq!(w.rows_copied(), Px(480));
    let done = |server: &mut Server, cb: ObjectId| {
        server.send(cb, "wl_callback", "done", &[Arg::Uint(0)]);
    };
    done(&mut server, cb);
    pump(&mut conn, &mut w);

    // The second frame, while the compositor holds the first buffer.
    paint(&mut frame, 10, 20, 0xff00_ff00);
    assert_eq!(
        w.present(
            &mut conn.sender(),
            &frame,
            wu,
            Changed::Strips(&[strip(10, 20)])
        ),
        Presented::Yes
    );
    conn.flush().unwrap();
    let (attached, damage, cb) = frame_requests(&mut server);
    assert_eq!(attached, Some(buffers[1]));
    assert_eq!(damage, [[0, 10, 640, 10]], "only the rows it changed");
    assert_eq!(w.rows_copied(), Px(480), "a new buffer is copied whole");
    assert_eq!(buffer_words(&file, 1, wu, hu), frame);

    // The third, into the first buffer again: it missed rows 10..20.
    server.send(buffers[0], "wl_buffer", "release", &[]);
    done(&mut server, cb);
    pump(&mut conn, &mut w);
    paint(&mut frame, 50, 60, 0xff00_00ff);
    assert_eq!(
        w.present(
            &mut conn.sender(),
            &frame,
            wu,
            Changed::Strips(&[strip(50, 60)])
        ),
        Presented::Yes
    );
    conn.flush().unwrap();
    let (attached, damage, cb) = frame_requests(&mut server);
    assert_eq!(attached, Some(buffers[0]));
    assert_eq!(damage, [[0, 50, 640, 10]]);
    assert_eq!(w.rows_copied(), Px(20), "its own ten and the ten it missed");
    assert_eq!(buffer_words(&file, 0, wu, hu), frame);

    // Released at once: the first buffer again, missing nothing.
    server.send(buffers[0], "wl_buffer", "release", &[]);
    server.send(buffers[1], "wl_buffer", "release", &[]);
    done(&mut server, cb);
    pump(&mut conn, &mut w);
    paint(&mut frame, 100, 101, 0xffff_ffff);
    assert_eq!(
        w.present(
            &mut conn.sender(),
            &frame,
            wu,
            Changed::Strips(&[strip(100, 101)])
        ),
        Presented::Yes
    );
    conn.flush().unwrap();
    let (attached, damage, cb) = frame_requests(&mut server);
    assert_eq!(attached, Some(buffers[0]));
    assert_eq!(damage, [[0, 100, 640, 1]]);
    assert_eq!(w.rows_copied(), Px(1));
    assert_eq!(buffer_words(&file, 0, wu, hu), frame);

    // Refused while that frame is in flight, then sent with the next.
    paint(&mut frame, 150, 160, 0xff80_8080);
    assert_eq!(
        w.present(
            &mut conn.sender(),
            &frame,
            wu,
            Changed::Strips(&[strip(150, 160)])
        ),
        Presented::No
    );
    server.send(buffers[0], "wl_buffer", "release", &[]);
    done(&mut server, cb);
    pump(&mut conn, &mut w);
    assert_eq!(
        w.present(&mut conn.sender(), &frame, wu, Changed::Strips(&[])),
        Presented::Yes
    );
    conn.flush().unwrap();
    let (attached, damage, cb) = frame_requests(&mut server);
    assert_eq!(attached, Some(buffers[0]));
    assert_eq!(damage, [[0, 150, 640, 10]], "the refused frame's rows");
    assert_eq!(buffer_words(&file, 0, wu, hu), frame);

    // Nothing changed: no buffer, and the callback still asked for.
    server.send(buffers[0], "wl_buffer", "release", &[]);
    done(&mut server, cb);
    pump(&mut conn, &mut w);
    assert_eq!(
        w.present(&mut conn.sender(), &frame, wu, Changed::Strips(&[])),
        Presented::Yes
    );
    conn.flush().unwrap();
    let (attached, damage, cb) = frame_requests(&mut server);
    assert_eq!((attached, damage), (None, Vec::new()));
    assert_eq!(w.rows_copied(), Px(0));
    assert!(w.in_flight(), "paced by the callback as any frame");

    // A new width is a new pool at the same height, shown whole whatever
    // the frame says.
    done(&mut server, cb);
    configure(&mut server, top, xdg, 320, 480, &[]);
    pump(&mut conn, &mut w);
    server.expect("xdg_surface", "ack_configure");
    assert_eq!(
        w.present(&mut conn.sender(), &frame, wu, Changed::Strips(&[])),
        Presented::Yes
    );
    conn.flush().unwrap();
    server.expect("wl_buffer", "destroy");
    server.expect("wl_buffer", "destroy");
    server.expect("wl_shm_pool", "destroy");
    let (attached, damage, _) = frame_requests(&mut server);
    assert!(attached.is_some());
    assert_eq!(damage, [[0, 0, 320, 480]]);
    assert_eq!(w.rows_copied(), Px(480));
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
    assert_eq!(
        w.pointer_device(),
        baud_domain::LocatorDevice::Present,
        "the control: a pointer named"
    );
    pump(&mut conn, &mut w);
    let r = server.expect("wp_cursor_shape_device_v1", "destroy");
    assert_eq!(Some(r.sender), device);
    let r = server.expect("wl_pointer", "release");
    assert_eq!(r.sender, pointer);
    assert_eq!(pointer_drain(&mut w), [PointerEvent::Left]);
    assert_eq!(
        w.pointer_device(),
        baud_domain::LocatorDevice::Absent,
        "the driver tells the core there is no locator"
    );
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

/// The keyboard on the surface, with the enter's own serial.
fn keyboard_enter(server: &mut Server, keyboard: ObjectId, serial: u32, surface: ObjectId) {
    server.send(
        keyboard,
        "wl_keyboard",
        "enter",
        &[Arg::Uint(serial), Arg::Object(surface), Arg::Array(&[])],
    );
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
        surface,
        data_manager,
        data_device,
        ..
    } = clip_session();
    assert_eq!(
        w.copy(&mut conn.sender(), Board::Clipboard, "early"),
        Copied::Unavailable,
        "no input event yet, so no serial"
    );
    keyboard_enter(&mut server, keyboard, 41, surface);
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
        surface,
        primary_manager,
        primary_device,
        ..
    } = clip_session();
    keyboard_enter(&mut server, keyboard, 6, surface);
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

const KEY_F: u32 = 33;

/// **Ctrl+Shift+F opens the find field** (Ghostty's and Alacritty's
/// binding): a shortcut, not a key, repeating and releasing nothing; Ctrl+F
/// alone is the program's key.
#[test]
fn ctrl_shift_f_is_the_find_fields() {
    use baud_domain::{KeyEventKind, Modifiers};
    let Clipped {
        mut conn,
        mut server,
        mut w,
        keyboard: kb,
        ..
    } = clip_session();
    repeat_info(&mut server, kb, 25, 600);
    let (shift, ctrl) = (1u32, 4u32);
    depressed(&mut server, kb, ctrl | shift);
    key_at(&mut server, kb, 3, KEY_F, PRESSED);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), [Event::Shortcut(Shortcut::Find)]);
    assert_eq!(w.repeat_deadline(), None, "a shortcut does not repeat");
    key_at(&mut server, kb, 4, KEY_F, RELEASED);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), [], "nor release");
    depressed(&mut server, kb, ctrl);
    key_at(&mut server, kb, 5, KEY_F, PRESSED);
    pump(&mut conn, &mut w);
    assert_eq!(
        drain(&mut w),
        [typed(
            'f',
            Modifiers::NONE.with(Modifiers::CTRL),
            KeyEventKind::Press
        )]
    );
}

// evdev codes for the zoom keys.
const KEY_0: u32 = 11;
const KEY_MINUS: u32 = 12;
const KEY_EQUAL: u32 = 13;
const KEY_KPMINUS: u32 = 74;
const KEY_KPPLUS: u32 = 78;
const KEY_KP0: u32 = 82;

/// **The zoom keys are the terminal's**: Ctrl+equal, Ctrl+plus (Shift on
/// the equal key) and Ctrl+KP_Add zoom in, Ctrl+minus and Ctrl+KP_Subtract
/// out, Ctrl+0 and Ctrl+KP_0 (NumLock on) back, foot's defaults. Ctrl+Alt
/// on the minus key is a key, the chord being exact, and so is Ctrl+Shift
/// on the minus key, which types an underscore no binding names.
#[test]
fn the_zoom_keys_are_kept_from_the_program() {
    let Clipped {
        mut conn,
        mut server,
        mut w,
        keyboard: kb,
        ..
    } = clip_session();
    let (shift, ctrl, alt, num) = (1u32, 4u32, 8u32, 16u32);
    let mut serial = 10;
    for (mask, locked, code, want) in [
        (ctrl, 0, KEY_EQUAL, Shortcut::ZoomIn),
        (ctrl | shift, 0, KEY_EQUAL, Shortcut::ZoomIn),
        (ctrl, 0, KEY_KPPLUS, Shortcut::ZoomIn),
        (ctrl, 0, KEY_MINUS, Shortcut::ZoomOut),
        (ctrl, 0, KEY_KPMINUS, Shortcut::ZoomOut),
        (ctrl, 0, KEY_0, Shortcut::ZoomReset),
        (ctrl, num, KEY_KP0, Shortcut::ZoomReset),
    ] {
        server.send(
            kb,
            "wl_keyboard",
            "modifiers",
            &[
                Arg::Uint(1),
                Arg::Uint(mask),
                Arg::Uint(0),
                Arg::Uint(locked),
                Arg::Uint(0),
            ],
        );
        key_at(&mut server, kb, serial, code, PRESSED);
        key_at(&mut server, kb, serial + 1, code, RELEASED);
        serial += 2;
        pump(&mut conn, &mut w);
        assert_eq!(drain(&mut w), [Event::Shortcut(want)], "{want:?} {code}");
    }
    for mask in [ctrl | alt, ctrl | shift] {
        depressed(&mut server, kb, mask);
        key_at(&mut server, kb, serial, KEY_MINUS, PRESSED);
        key_at(&mut server, kb, serial + 1, KEY_MINUS, RELEASED);
        serial += 2;
        pump(&mut conn, &mut w);
        let got = drain(&mut w);
        assert!(
            !got.is_empty() && got.iter().all(|e| matches!(e, Event::Key(_))),
            "{mask}: keys, not shortcuts: {got:?}"
        );
    }
}

/// **The clipboard's keys are found on a Cyrillic layout too**, as foot
/// finds a binding: with the `us,de,ru` keymap locked to Russian, the C key
/// types `Cyrillic_es`, and Ctrl+Shift on it is still the copy, through
/// the key's keysym in the first layout; Ctrl+Shift on V the paste.
/// Control alone is still a key, and so is the chord on a key whose first
/// layout has no `c`.
#[test]
fn the_clipboard_keys_are_found_on_a_cyrillic_layout() {
    use baud_domain::KeyEventKind;
    let Clipped {
        mut conn,
        mut server,
        mut w,
        keyboard: kb,
        ..
    } = clip_session();
    let keymap = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-xkb/tests/fixtures/keymap-us_de_ru__grp_win_space_toggle.xkb"
    ))
    .unwrap();
    let mut kf = tempfile();
    kf.write_all(&keymap).unwrap();
    kf.write_all(&[0]).unwrap();
    server.send(
        kb,
        "wl_keyboard",
        "keymap",
        &[
            Arg::Uint(protocol::wl_keyboard::keymap_format::XKB_V1),
            Arg::Fd(kf.as_fd()),
            Arg::Uint(u32::try_from(keymap.len() + 1).unwrap()),
        ],
    );
    let (shift, ctrl) = (1u32, 4u32);
    let locked = |server: &mut Server, mask: u32| {
        server.send(
            kb,
            "wl_keyboard",
            "modifiers",
            &[
                Arg::Uint(1),
                Arg::Uint(mask),
                Arg::Uint(0),
                Arg::Uint(0),
                Arg::Uint(2),
            ],
        );
    };
    // The layout is Russian: the C key types es.
    locked(&mut server, 0);
    key_at(&mut server, kb, 3, KEY_C, PRESSED);
    key_at(&mut server, kb, 4, KEY_C, RELEASED);
    pump(&mut conn, &mut w);
    let got = drain(&mut w);
    assert!(
        matches!(&got[..], [Event::Key(e), Event::Key(_)]
            if e.code == baud_domain::KeyCode::Char(baud_domain::Codepoint::from_char('\u{441}'))),
        "es on the C key: {got:?}"
    );
    for (code, want) in [(KEY_C, Shortcut::Copy), (KEY_V, Shortcut::Paste)] {
        locked(&mut server, ctrl | shift);
        key_at(&mut server, kb, 5, code, PRESSED);
        pump(&mut conn, &mut w);
        assert_eq!(drain(&mut w), [Event::Shortcut(want)], "{want:?}");
        key_at(&mut server, kb, 6, code, RELEASED);
        pump(&mut conn, &mut w);
        assert_eq!(drain(&mut w), [], "nor release");
    }
    locked(&mut server, ctrl);
    key_at(&mut server, kb, 7, KEY_C, PRESSED);
    key_at(&mut server, kb, 8, KEY_C, RELEASED);
    locked(&mut server, ctrl | shift);
    key_at(&mut server, kb, 9, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    let got = drain(&mut w);
    assert!(
        got.iter()
            .all(|e| matches!(e, Event::Key(k) if k.kind != KeyEventKind::Repeat)),
        "keys, not shortcuts: {got:?}"
    );
    assert_eq!(got.len(), 3, "{got:?}");
}

/// **A copy made without the keyboard is sent and said**: before any
/// keyboard enter and after a leave the source is set with the last serial
/// the client was given and the copy answers `Unfocused`; a cancel of that
/// source before the keyboard returns is `CopyCancelled`, on its own
/// board. A cancel of a source set with the keyboard says nothing, and the
/// keyboard's return stops the watch on one set without it.
#[test]
fn a_copy_without_the_keyboard_is_sent_and_its_cancel_said() {
    let Clipped {
        mut conn,
        mut server,
        mut w,
        keyboard,
        surface,
        ..
    } = clip_session();
    let sources = |server: &mut Server, iface: &str| {
        let r = server.expect(
            iface,
            match iface {
                "wl_data_device_manager" => "create_data_source",
                _ => "create_source",
            },
        );
        let Val::New(id) = r.args[0] else { panic!() };
        server.objects.insert(
            id,
            Interface::named(match iface {
                "wl_data_device_manager" => "wl_data_source",
                _ => "zwp_primary_selection_source_v1",
            })
            .unwrap(),
        );
        loop {
            let r = server.next();
            if r.name == "set_selection" {
                let Val::Uint(serial) = r.args[1] else {
                    panic!()
                };
                return (id, serial);
            }
        }
    };
    let manager = |b| match b {
        Board::Clipboard => "wl_data_device_manager",
        Board::Primary => "zwp_primary_selection_device_manager_v1",
    };
    let source_iface = |b| match b {
        Board::Clipboard => "wl_data_source",
        Board::Primary => "zwp_primary_selection_source_v1",
    };
    // A key before any enter: the copy is sent, with the key's serial.
    key_at(&mut server, keyboard, 5, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    drain(&mut w);
    for b in [Board::Clipboard, Board::Primary] {
        assert_eq!(w.copy(&mut conn.sender(), b, "early"), Copied::Unfocused);
        conn.flush().unwrap();
        let (id, serial) = sources(&mut server, manager(b));
        assert_eq!(serial, 5, "the last serial");
        server.send(id, source_iface(b), "cancelled", &[]);
        pump(&mut conn, &mut w);
        assert_eq!(drain(&mut w), [Event::CopyCancelled(b)], "{b:?}");
        server.expect(source_iface(b), "destroy");
    }
    // With the keyboard: set, and a cancel is another client's copy.
    keyboard_enter(&mut server, keyboard, 8, surface);
    pump(&mut conn, &mut w);
    drain(&mut w);
    assert_eq!(
        w.copy(&mut conn.sender(), Board::Clipboard, "here"),
        Copied::Set
    );
    conn.flush().unwrap();
    let (id, serial) = sources(&mut server, manager(Board::Clipboard));
    assert_eq!(serial, 8);
    server.send(id, "wl_data_source", "cancelled", &[]);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), [], "a focused copy's cancel is not said");
    server.expect("wl_data_source", "destroy");
    // Gone again: sent with the last serial, the enter's.
    server.send(
        keyboard,
        "wl_keyboard",
        "leave",
        &[Arg::Uint(9), Arg::Object(surface)],
    );
    pump(&mut conn, &mut w);
    drain(&mut w);
    assert_eq!(
        w.copy(&mut conn.sender(), Board::Clipboard, "away"),
        Copied::Unfocused
    );
    assert_eq!(
        w.clear(&mut conn.sender(), Board::Primary),
        Copied::Unfocused
    );
    conn.flush().unwrap();
    let (id, serial) = sources(&mut server, manager(Board::Clipboard));
    assert_eq!(serial, 8, "the enter's, the last a key or enter gave");
    // The keyboard back before the cancel: the watch is over.
    keyboard_enter(&mut server, keyboard, 10, surface);
    server.send(id, "wl_data_source", "cancelled", &[]);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), [Event::Focus(baud_domain::Focus::Gained)]);
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

/// **Whether `configure_bounds` can come is the bound `xdg_wm_base`'s**:
/// the event is "since 4" (`[[source: wp-xdg-shell]]`), so a compositor
/// offering version 3 never sends it, one offering 4 or 6 may, and before
/// the global is bound nothing is known.
#[test]
fn configure_bounds_comes_from_xdg_wm_base_version_4() {
    for (offered, want) in [
        (3u32, ConfigureBounds::Never),
        (4, ConfigureBounds::Offered),
        (6, ConfigureBounds::Offered),
    ] {
        let (c, s) = UnixStream::pair().unwrap();
        let mut conn = Connection::over(c);
        let mut server = Server::new(s);
        let mut w = Window::open(&mut conn, "baud", "org.baud.terminal").unwrap();
        conn.flush().unwrap();
        assert_eq!(w.configure_bounds(), ConfigureBounds::Never, "before");
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
            (3, "xdg_wm_base", offered),
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
        assert_eq!(w.configure_bounds(), want, "xdg_wm_base {offered}");
    }
}

/// The keymap the compositor shares, by fixture name.
fn send_named_keymap(server: &mut Server, keyboard: ObjectId, name: &str) {
    let keymap = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../baud-xkb/tests/fixtures")
            .join(name),
    )
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

/// libX11's `en_US.UTF-8` Compose table, as pinned.
fn en_us_compose() -> baud_xkb::compose::Table {
    let text = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/libx11-1.8.13-compose-en_US.UTF-8.txt"
    ))
    .unwrap();
    baud_xkb::compose::Table::parse(&text, &mut baud_xkb::compose::NoFiles).unwrap()
}

const KEY_GRAVE: u32 = 41;
const KEY_APOSTROPHE: u32 = 40;
const KEY_E: u32 = 18;
const KEY_A: u32 = 30;

/// **A dead key types nothing, and the letter after it types the two
/// composed**, over the pinned US International keymap, whose apostrophe
/// is `dead_acute` and whose backtick is `dead_grave` and, shifted,
/// `dead_tilde`, and libX11's `en_US.UTF-8` table: `'` `e` is `é`; a dead
/// key still down when the letter is pressed composes the same, and
/// neither release reaches the program, nor that of a cancelling key held
/// through the next sequence; Shift between the two is not part
/// of the sequence, so `'` Shift+`e` is `É`; `'` `q`, which the table does
/// not continue, types nothing and the `q` after it is a `q`; a sequence
/// that composes three codepoints types three characters; and without a
/// table the dead key is no key at all and the `e` is an `e`.
#[test]
fn a_dead_key_composes_with_the_next_letter() {
    use baud_domain::{KeyEventKind, Modifiers};
    let (mut conn, mut server, mut w, kb, _) = keyboard_session();
    send_named_keymap(&mut server, kb, "keymap-us_intl.xkb");
    pump(&mut conn, &mut w);
    let tap = |server: &mut Server, code: u32| {
        key(server, kb, code, PRESSED);
        key(server, kb, code, RELEASED);
    };
    let text = |s: &str| -> Vec<Event> { s.chars().map(|c| Event::Key(keys::typed(c))).collect() };
    // No table: the dead key is nothing, and the e is an e.
    tap(&mut server, KEY_APOSTROPHE);
    tap(&mut server, KEY_E);
    pump(&mut conn, &mut w);
    assert_eq!(
        drain(&mut w),
        [
            typed('e', Modifiers::NONE, KeyEventKind::Press),
            typed('e', Modifiers::NONE, KeyEventKind::Release)
        ]
    );
    w.set_compose(Some(en_us_compose()));
    tap(&mut server, KEY_APOSTROPHE);
    tap(&mut server, KEY_E);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), text("é"), "' then e");
    // The dead key still down when the e goes down: one é, no release.
    key(&mut server, kb, KEY_APOSTROPHE, PRESSED);
    key(&mut server, kb, KEY_E, PRESSED);
    key(&mut server, kb, KEY_APOSTROPHE, RELEASED);
    key(&mut server, kb, KEY_E, RELEASED);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), text("é"), "overlapped");
    // A cancelling q still down through the next sequence: its release
    // is swallowed too, as are the others.
    key(&mut server, kb, KEY_APOSTROPHE, PRESSED);
    key(&mut server, kb, KEY_Q, PRESSED);
    key(&mut server, kb, KEY_APOSTROPHE, RELEASED);
    tap(&mut server, KEY_APOSTROPHE);
    key(&mut server, kb, KEY_E, PRESSED);
    key(&mut server, kb, KEY_Q, RELEASED);
    key(&mut server, kb, KEY_E, RELEASED);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), text("é"), "a cancel held through a sequence");
    // Shift between: the Shift key is fed and ignored.
    tap(&mut server, KEY_APOSTROPHE);
    key(&mut server, kb, KEY_LEFTSHIFT, PRESSED);
    depressed(&mut server, kb, 1);
    tap(&mut server, KEY_E);
    depressed(&mut server, kb, 0);
    key(&mut server, kb, KEY_LEFTSHIFT, RELEASED);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), text("É"), "' Shift+e");
    // A cancel swallows its key; the next key is itself.
    tap(&mut server, KEY_APOSTROPHE);
    tap(&mut server, KEY_Q);
    tap(&mut server, KEY_Q);
    pump(&mut conn, &mut w);
    assert_eq!(
        drain(&mut w),
        [
            typed('q', Modifiers::NONE, KeyEventKind::Press),
            typed('q', Modifiers::NONE, KeyEventKind::Release)
        ],
        "' q q"
    );
    // dead_grave, dead_tilde, A: three codepoints.
    tap(&mut server, KEY_GRAVE);
    depressed(&mut server, kb, 1);
    tap(&mut server, KEY_GRAVE);
    tap(&mut server, KEY_A);
    depressed(&mut server, kb, 0);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), text("A\u{303}\u{300}"), "` ~ A");
    // Held down, neither the dead key nor the composed one repeats.
    repeat_info(&mut server, kb, 25, 1);
    key(&mut server, kb, KEY_APOSTROPHE, PRESSED);
    pump(&mut conn, &mut w);
    assert_eq!(w.repeat_deadline(), None, "a dead key held");
    key(&mut server, kb, KEY_E, PRESSED);
    pump(&mut conn, &mut w);
    assert_eq!(w.repeat_deadline(), None, "a composed key held");
    assert_eq!(drain(&mut w), text("é"));
}

/// **A sequence does not outlive the keyboard**: a dead key, then the
/// keyboard leaves and comes back, and the e is an e.
#[test]
fn a_sequence_ends_when_the_keyboard_leaves() {
    use baud_domain::{KeyEventKind, Modifiers};
    let (mut conn, mut server, mut w, kb, surface) = keyboard_session();
    send_named_keymap(&mut server, kb, "keymap-us_intl.xkb");
    pump(&mut conn, &mut w);
    w.set_compose(Some(en_us_compose()));
    key(&mut server, kb, KEY_APOSTROPHE, PRESSED);
    key(&mut server, kb, KEY_APOSTROPHE, RELEASED);
    server.send(
        kb,
        "wl_keyboard",
        "leave",
        &[Arg::Uint(2), Arg::Object(surface)],
    );
    keyboard_enter(&mut server, kb, 3, surface);
    key(&mut server, kb, KEY_E, PRESSED);
    pump(&mut conn, &mut w);
    let keys: Vec<Event> = drain(&mut w)
        .into_iter()
        .filter(|e| matches!(e, Event::Key(_)))
        .collect();
    assert_eq!(keys, [typed('e', Modifiers::NONE, KeyEventKind::Press)]);
}

/// A window on a compositor that offers `text-input-v3`: the connection,
/// the server, the window, its text-input object, its surface and the
/// seat.
struct Ime {
    conn: Connection,
    server: Server,
    w: Window,
    ti: ObjectId,
    surface: ObjectId,
    seat: ObjectId,
}

/// The session with the manager offered at `offered`, bound at the lower
/// of it and 2.
fn ime_session(offered: u32) -> Ime {
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
    // The manager before the seat: the object waits for both.
    for (name, iface, version) in [
        (1u32, "wl_compositor", 6u32),
        (2, "wl_shm", 2),
        (3, "xdg_wm_base", 6),
        (5, "zwp_text_input_manager_v3", offered),
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
    let (mut seat, mut manager, mut ti, mut surface) = (None, None, None, None);
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            ("wl_registry", "bind", [_, Val::NewWith(i, v, id)]) if i == "wl_seat" => {
                seat = Some(*id);
                assert_eq!(*v, 9);
            }
            ("wl_registry", "bind", [_, Val::NewWith(i, v, id)])
                if i == "zwp_text_input_manager_v3" =>
            {
                assert_eq!(*v, offered.min(2), "the version bound");
                manager = Some(*id);
            }
            ("zwp_text_input_manager_v3", "get_text_input", [Val::New(id), Val::Obj(s)]) => {
                assert_eq!(Some(*s), seat, "for the seat");
                assert!(manager.is_some());
                ti = Some(*id);
            }
            ("wl_compositor", "create_surface", [Val::New(id)]) => surface = Some(*id),
            ("wl_surface", "commit", _) => break,
            _ => {}
        }
    }
    Ime {
        conn,
        server,
        w,
        ti: ti.expect("a text-input object"),
        surface: surface.expect("the surface"),
        seat: seat.expect("the seat"),
    }
}

fn rect(x: u32, y: u32, w: u32, h: u32) -> CursorRect {
    CursorRect {
        x: Px(x),
        y: Px(y),
        width: Px(w),
        height: Px(h),
    }
}

/// The request after everything the client has sent so far: a title set
/// now, so "nothing was sent" is the title coming next.
fn nothing_before_the_title(i: &mut Ime) {
    i.w.set_title(&mut i.conn.sender(), "fence");
    i.conn.flush().unwrap();
    let r = i.server.next();
    assert_eq!(
        (r.iface, r.name),
        ("xdg_toplevel", "set_title"),
        "{:?}",
        r.args
    );
}

/// **An input method is enabled where the keyboard is, composes at the
/// cursor and types what it commits** (`[[source:
/// wp-text-input-unstable-v3]]`): on the object's `enter` on the window's
/// surface it is enabled with the content purpose `terminal` and committed;
/// the cursor's cell is sent with a commit when it moves and not when it
/// does not; a preedit arrives on `done` and a `done` without one clears
/// it; a commit types its characters after the preedit is cleared; a
/// `done` naming an older serial is applied and holds the cursor until a
/// current one; `leave` clears the preedit and sends nothing after it; an
/// `enter` on another surface enables nothing. A preedit carries its
/// `cursor_begin` and `cursor_end`, and a moved cursor over the same text
/// is a new preedit. On a compositor of version
/// 1, so no request version 2 adds reaches the wire.
#[test]
fn an_input_method_composes_at_the_cursor_and_commits_text() {
    let mut i = ime_session(1);
    let (ti, surface) = (i.ti, i.surface);
    let foreign = ObjectId(0xff00_0001);
    i.server
        .send(ti, "zwp_text_input_v3", "enter", &[Arg::Object(foreign)]);
    pump(&mut i.conn, &mut i.w);
    nothing_before_the_title(&mut i);
    i.server
        .send(ti, "zwp_text_input_v3", "enter", &[Arg::Object(surface)]);
    pump(&mut i.conn, &mut i.w);
    i.server.expect("zwp_text_input_v3", "enable");
    let r = i.server.expect("zwp_text_input_v3", "set_content_type");
    assert!(
        matches!(&r.args[..], [Val::Uint(0), Val::Uint(13)]),
        "no hint, the terminal purpose: {:?}",
        r.args
    );
    i.server.expect("zwp_text_input_v3", "commit");
    // The cursor: sent once, and again only when it moves.
    i.w.set_text_cursor(&mut i.conn.sender(), rect(27, 36, 9, 18));
    i.conn.flush().unwrap();
    let r = i.server.expect("zwp_text_input_v3", "set_cursor_rectangle");
    assert!(
        matches!(
            &r.args[..],
            [Val::Int(27), Val::Int(36), Val::Int(9), Val::Int(18)]
        ),
        "{:?}",
        r.args
    );
    i.server.expect("zwp_text_input_v3", "commit");
    i.w.set_text_cursor(&mut i.conn.sender(), rect(27, 36, 9, 18));
    nothing_before_the_title(&mut i);
    // Two commits so far: a preedit on done(2), its cursor at byte 3.
    let pre_at = |i: &mut Ime, t: &str, begin: i32, end: i32| {
        i.server.send(
            ti,
            "zwp_text_input_v3",
            "preedit_string",
            &[Arg::Str(Some(t)), Arg::Int(begin), Arg::Int(end)],
        );
    };
    let pre = |i: &mut Ime, t: &str| pre_at(i, t, 3, 3);
    let with = |t: &str, begin: i32, end: i32| Preedit {
        cursor: PreeditCursor {
            begin: CursorAt(begin),
            end: CursorAt(end),
        },
        ..Preedit::from(t)
    };
    let done = |i: &mut Ime, n: u32| {
        i.server
            .send(ti, "zwp_text_input_v3", "done", &[Arg::Uint(n)]);
    };
    pre(&mut i, "か");
    done(&mut i, 2);
    pump(&mut i.conn, &mut i.w);
    assert_eq!(drain(&mut i.w), [Event::Preedit("か".into())]);
    pre(&mut i, "かん");
    done(&mut i, 2);
    pump(&mut i.conn, &mut i.w);
    assert_eq!(drain(&mut i.w), [Event::Preedit(with("かん", 3, 3))]);
    // The cursor moved over the same text: a new preedit, its cursor a
    // range, then hidden; the same again is no change.
    pre_at(&mut i, "かん", 0, 6);
    done(&mut i, 2);
    pump(&mut i.conn, &mut i.w);
    assert_eq!(drain(&mut i.w), [Event::Preedit(with("かん", 0, 6))]);
    pre_at(&mut i, "かん", -1, -1);
    done(&mut i, 2);
    pump(&mut i.conn, &mut i.w);
    assert_eq!(drain(&mut i.w), [Event::Preedit(with("かん", -1, -1))]);
    pre_at(&mut i, "かん", -1, -1);
    done(&mut i, 2);
    pump(&mut i.conn, &mut i.w);
    assert_eq!(drain(&mut i.w), [], "no change");
    // A commit: the preedit cleared, then the text typed.
    i.server.send(
        ti,
        "zwp_text_input_v3",
        "commit_string",
        &[Arg::Str(Some("漢字"))],
    );
    done(&mut i, 2);
    pump(&mut i.conn, &mut i.w);
    assert_eq!(
        drain(&mut i.w),
        [
            Event::Preedit(Preedit::default()),
            Event::Key(keys::typed('漢')),
            Event::Key(keys::typed('字'))
        ]
    );
    // A commit and a new preedit in one done.
    i.server.send(
        ti,
        "zwp_text_input_v3",
        "commit_string",
        &[Arg::Str(Some("a"))],
    );
    pre_at(&mut i, "b", 1, 1);
    done(&mut i, 2);
    pump(&mut i.conn, &mut i.w);
    assert_eq!(
        drain(&mut i.w),
        [Event::Key(keys::typed('a')), Event::Preedit("b".into())]
    );
    // A done without a preedit clears it.
    done(&mut i, 2);
    pump(&mut i.conn, &mut i.w);
    assert_eq!(drain(&mut i.w), [Event::Preedit(Preedit::default())]);
    // An older serial: applied, and the cursor held until a current one.
    pre_at(&mut i, "x", 1, 1);
    done(&mut i, 1);
    pump(&mut i.conn, &mut i.w);
    assert_eq!(drain(&mut i.w), [Event::Preedit("x".into())]);
    i.w.set_text_cursor(&mut i.conn.sender(), rect(36, 36, 9, 18));
    nothing_before_the_title(&mut i);
    pre_at(&mut i, "x", 1, 1);
    done(&mut i, 2);
    pump(&mut i.conn, &mut i.w);
    let r = i.server.expect("zwp_text_input_v3", "set_cursor_rectangle");
    assert!(matches!(&r.args[..], [Val::Int(36), ..]), "{:?}", r.args);
    i.server.expect("zwp_text_input_v3", "commit");
    assert_eq!(drain(&mut i.w), []);
    // Leave: the preedit goes, and nothing is sent until the next enter.
    i.server
        .send(ti, "zwp_text_input_v3", "leave", &[Arg::Object(surface)]);
    pump(&mut i.conn, &mut i.w);
    assert_eq!(drain(&mut i.w), [Event::Preedit(Preedit::default())]);
    i.w.set_text_cursor(&mut i.conn.sender(), rect(45, 36, 9, 18));
    nothing_before_the_title(&mut i);
    // The next enter sends the cursor it holds with the enable.
    i.server
        .send(ti, "zwp_text_input_v3", "enter", &[Arg::Object(surface)]);
    pump(&mut i.conn, &mut i.w);
    i.server.expect("zwp_text_input_v3", "enable");
    i.server.expect("zwp_text_input_v3", "set_content_type");
    let r = i.server.expect("zwp_text_input_v3", "set_cursor_rectangle");
    assert!(matches!(&r.args[..], [Val::Int(45), ..]), "{:?}", r.args);
    i.server.expect("zwp_text_input_v3", "commit");
}

/// The window's events but the pointer's.
fn typed_events(w: &mut Window) -> Vec<Event> {
    drain(w)
        .into_iter()
        .filter(|e| !matches!(e, Event::Pointer(_)))
        .collect()
}

/// **At version 2 the hints, the action and the on-screen keyboard**
/// (`[[source: wp-text-input-unstable-v3]]`): a compositor offering 3 is
/// bound at 2; the enable carries the content hint `preedit_shown` and
/// the one action, `submit`; a preedit's hints arrive with it on `done`,
/// sent before or after it, a value the protocol does not name dropped,
/// and a new preedit has none until its own; a `submit` is the Return key after the commit; `none`,
/// `language` and `delete_surrounding_text` do nothing and send nothing;
/// a press of the primary button asks for the on-screen keyboard with a
/// commit, and a press of another does not.
#[test]
fn an_input_method_at_version_2_hints_submits_and_asks_for_a_keyboard() {
    use protocol::zwp_text_input_v3::{action, content_hint, preedit_hint};
    let mut i = ime_session(3);
    let (ti, surface, seat) = (i.ti, i.surface, i.seat);
    i.server.send(
        seat,
        "wl_seat",
        "capabilities",
        &[Arg::Uint(protocol::wl_seat::capability::POINTER)],
    );
    pump(&mut i.conn, &mut i.w);
    let r = i.server.expect("wl_seat", "get_pointer");
    let Val::New(pointer) = r.args[0] else {
        panic!()
    };
    // The text-input requests sent before a fence, whatever the pointer's
    // own (its drawn cursor) are around them.
    let sent = |i: &mut Ime| {
        i.w.set_title(&mut i.conn.sender(), "fence");
        i.conn.flush().unwrap();
        let mut out = Vec::new();
        loop {
            let r = i.server.next();
            match (r.iface, r.name) {
                ("xdg_toplevel", "set_title") => break out,
                ("zwp_text_input_v3", name) => out.push(name),
                _ => {}
            }
        }
    };
    i.server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[Arg::Uint(7), Arg::Object(surface), fixed(1.0), fixed(1.0)],
    );
    pump(&mut i.conn, &mut i.w);
    assert_eq!(sent(&mut i), Vec::<&str>::new());
    // A press before text input is on asks for nothing.
    let press = |i: &mut Ime, button: u32| {
        i.server.send(
            pointer,
            "wl_pointer",
            "button",
            &[Arg::Uint(8), Arg::Uint(1), Arg::Uint(button), Arg::Uint(1)],
        );
        i.server.send(pointer, "wl_pointer", "frame", &[]);
        pump(&mut i.conn, &mut i.w);
    };
    press(&mut i, 0x110);
    assert_eq!(sent(&mut i), Vec::<&str>::new());
    i.server
        .send(ti, "zwp_text_input_v3", "enter", &[Arg::Object(surface)]);
    pump(&mut i.conn, &mut i.w);
    i.server.expect("zwp_text_input_v3", "enable");
    let r = i.server.expect("zwp_text_input_v3", "set_content_type");
    assert!(
        matches!(&r.args[..], [Val::Uint(h), Val::Uint(13)] if *h == content_hint::PREEDIT_SHOWN),
        "preedit_shown, the terminal purpose: {:?}",
        r.args
    );
    let r = i
        .server
        .expect("zwp_text_input_v3", "set_available_actions");
    assert!(
        matches!(&r.args[..], [Val::Array(a)] if a[..] == action::SUBMIT.to_ne_bytes()),
        "submit, alone: {:?}",
        r.args
    );
    i.server.expect("zwp_text_input_v3", "commit");
    let hint = |i: &mut Ime, start: u32, end: u32, h: u32| {
        i.server.send(
            ti,
            "zwp_text_input_v3",
            "preedit_hint",
            &[Arg::Uint(start), Arg::Uint(end), Arg::Uint(h)],
        );
    };
    let pre = |i: &mut Ime, t: &str| {
        i.server.send(
            ti,
            "zwp_text_input_v3",
            "preedit_string",
            &[Arg::Str(Some(t)), Arg::Int(6), Arg::Int(6)],
        );
    };
    let done = |i: &mut Ime| {
        i.server
            .send(ti, "zwp_text_input_v3", "done", &[Arg::Uint(1)]);
        pump(&mut i.conn, &mut i.w);
    };
    pre(&mut i, "かな");
    hint(&mut i, 0, 3, preedit_hint::SELECTION);
    hint(&mut i, 3, 6, preedit_hint::WHOLE);
    hint(&mut i, 0, 6, 99);
    done(&mut i);
    let hinted = |start, end, hint| Hinted {
        start: ByteAt(start),
        end: ByteAt(end),
        hint,
    };
    assert_eq!(
        typed_events(&mut i.w),
        [Event::Preedit(Preedit {
            hints: vec![hinted(0, 3, Hint::Selection), hinted(3, 6, Hint::Whole)],
            ..Preedit::from("かな")
        })]
    );
    pre(&mut i, "かな");
    done(&mut i);
    assert_eq!(
        typed_events(&mut i.w),
        [Event::Preedit("かな".into())],
        "no hints"
    );
    // A hint sent before its string is the string's: the text fixes no
    // order between them, only that both apply on `done`.
    hint(&mut i, 0, 6, preedit_hint::PREDICTION);
    pre(&mut i, "かな");
    done(&mut i);
    assert_eq!(
        typed_events(&mut i.w),
        [Event::Preedit(Preedit {
            hints: vec![hinted(0, 6, Hint::Prediction)],
            ..Preedit::from("かな")
        })]
    );
    // A submit with a commit: the preedit goes, the text, then Return.
    i.server.send(
        ti,
        "zwp_text_input_v3",
        "commit_string",
        &[Arg::Str(Some("仮名"))],
    );
    i.server.send(
        ti,
        "zwp_text_input_v3",
        "action",
        &[Arg::Uint(action::SUBMIT), Arg::Uint(4)],
    );
    done(&mut i);
    let enter = || {
        Event::Key(baud_domain::KeyEvent::press(
            baud_domain::KeyCode::Enter,
            baud_domain::Modifiers::NONE,
        ))
    };
    assert_eq!(
        typed_events(&mut i.w),
        [
            Event::Preedit(Preedit::default()),
            Event::Key(keys::typed('仮')),
            Event::Key(keys::typed('名')),
            enter()
        ]
    );
    i.server.send(
        ti,
        "zwp_text_input_v3",
        "action",
        &[Arg::Uint(action::SUBMIT), Arg::Uint(5)],
    );
    done(&mut i);
    assert_eq!(typed_events(&mut i.w), [enter()]);
    i.server.send(
        ti,
        "zwp_text_input_v3",
        "action",
        &[Arg::Uint(action::NONE), Arg::Uint(6)],
    );
    i.server
        .send(ti, "zwp_text_input_v3", "language", &[Arg::Str(Some("ja"))]);
    i.server.send(
        ti,
        "zwp_text_input_v3",
        "delete_surrounding_text",
        &[Arg::Uint(3), Arg::Uint(0)],
    );
    done(&mut i);
    assert_eq!(typed_events(&mut i.w), []);
    assert_eq!(sent(&mut i), Vec::<&str>::new());
    // The primary button asks for the keyboard; the secondary does not.
    press(&mut i, 0x111);
    assert_eq!(sent(&mut i), Vec::<&str>::new());
    press(&mut i, 0x110);
    assert_eq!(sent(&mut i), ["show_input_panel", "commit"]);
}
