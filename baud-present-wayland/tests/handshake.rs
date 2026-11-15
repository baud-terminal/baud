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

/// **Every byte the server has sent, not one read's worth.** Linux ends a
/// stream read after the message that carried a descriptor
/// (`unix_stream_read_generic()`, [[source: linux-af-unix]]), so a keymap
/// arrives alone and the keys sent after it wait for the next read; macOS
/// returns them together. The app's loop reads again whenever its wait
/// says the socket is readable, so the pump reads until nothing is left:
/// the server is this thread, so what it sent is already queued.
fn pump(conn: &mut Connection, w: &mut Window) {
    conn.flush().unwrap();
    conn.receive().unwrap();
    // The rest, until a read that does not wait finds nothing queued.
    let probe = UnixStream::from(conn.as_fd().try_clone_to_owned().unwrap());
    probe.set_nonblocking(true).unwrap();
    loop {
        match conn.receive() {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => panic!("receive: {e}"),
        }
    }
    probe.set_nonblocking(false).unwrap();
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
    // The registry's round trip is a `wl_callback` too, and not a frame:
    // the launch logo starts on the first `Event::Frame`.
    assert_eq!(w.next_event(), None, "the sync's callback is no frame");
    // The viewporter is bound as it comes, as a fractional scale is drawn
    // through it; with no fractional-scale global nothing is made with it.
    let mut bound = Vec::new();
    let mut ids: HashMap<&str, ObjectId> = HashMap::new();
    for _ in 0..5 {
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
                "wp_viewporter" => "viewporter",
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
            ("wl_seat".to_string(), 9),
            ("wp_viewporter".to_string(), 1)
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
    assert_eq!(w.next_event(), None, "no frame before a frame is shown");

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

    // The callback and the release: the next frame may go, on the other
    // buffer. Nothing before it was a frame: the first `Event::Frame`, on
    // which the launch logo starts, answers the first commit that attached
    // a buffer, and a server "should avoid signaling" it "if the surface is
    // not visible in any way" ([[source: wayland]], `wl_surface.frame`).
    assert_eq!(w.next_event(), None, "no frame before its callback");
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
    let got = drain(&mut w);
    assert_eq!(
        got,
        [
            Event::Key(KeyEvent::press(
                KeyCode::Char(Codepoint::from_char('q')),
                Modifiers::NONE
            )),
            // Shift-q: the `q` key with Shift, as kitty names it, its
            // capital the text and the shifted key.
            shifted('q', 'Q', KeyEventKind::Press),
            // Control-q: a chord, whose `q` is no text.
            chorded('q', Modifiers::CTRL, KeyEventKind::Press),
            // The release of the Control-q, delivered as one: the core sends
            // it only where kitty's event types asked for releases.
            chorded('q', Modifiers::CTRL, KeyEventKind::Release),
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
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_ne_bytes(*b))
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

/// The monotonic clock's id on this system, which the test's compositor
/// names as its presentation clock: Linux's `CLOCK_MONOTONIC` and
/// Darwin's.
#[cfg(target_os = "linux")]
const MONOTONIC: u32 = 1;
#[cfg(target_os = "macos")]
const MONOTONIC: u32 = 6;

/// **A frame waits for the callback, and is timed by the presentation
/// feedback** (`timing.rs`). Where the compositor offers `wp_presentation`,
/// every commit asks for feedback; until a frame is presented, a frame is
/// planned for now; once one is, for the next refresh after now -- ahead
/// of now and within one period. A discarded frame leaves the last one
/// shown. The frame in flight holds the next until its callback.
#[test]
fn frames_wait_for_the_callback_and_are_timed_by_the_presentation() {
    use std::time::{Duration, Instant};
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
        (4, "wp_presentation", 2),
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
    let (mut presentation, mut xdg, mut top) = (None, None, None);
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            ("wl_registry", "bind", [_, Val::NewWith(i, v, id)]) if i == "wp_presentation" => {
                assert_eq!(*v, 2);
                presentation = Some(*id);
            }
            ("xdg_wm_base", "get_xdg_surface", [Val::New(id), _]) => xdg = Some(*id),
            ("xdg_surface", "get_toplevel", [Val::New(id)]) => top = Some(*id),
            ("wl_surface", "commit", _) => break,
            _ => {}
        }
    }
    let presentation = presentation.expect("bound");
    server.send(
        presentation,
        "wp_presentation",
        "clock_id",
        &[Arg::Uint(MONOTONIC)],
    );
    configure(&mut server, top.unwrap(), xdg.unwrap(), 64, 32, &[]);
    pump(&mut conn, &mut w);
    server.expect("xdg_surface", "ack_configure");
    assert!(matches!(w.next_event(), Some(Event::Configured { .. })));
    let frame = vec![0xff00_0000u32; 64 * 32];
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::No,
        "the frame in flight holds the next"
    );
    conn.flush().unwrap();
    let (mut callback, mut feedback) = (None, None);
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            ("wl_surface", "frame", [Val::New(cb)]) => callback = Some(*cb),
            ("wp_presentation", "feedback", [Val::Obj(_), Val::New(fb)]) => feedback = Some(*fb),
            ("wl_surface", "commit", _) => break,
            _ => {}
        }
    }
    let (callback, feedback) = (callback.unwrap(), feedback.expect("feedback asked"));
    let now = Instant::now();
    assert_eq!(w.next_shown(now), now, "nothing presented yet: now");

    // Shown a millisecond ago at 60 Hz: the next refresh is ahead.
    let before = Instant::now();
    let clock = baud_present_wayland::sys::clock_now(ClockId(i32::try_from(MONOTONIC).unwrap()))
        .expect("the monotonic clock");
    let shown = clock.0 - Duration::from_millis(1);
    let secs = shown.as_secs();
    let refresh = 16_666_667u32;
    server.send(
        feedback,
        "wp_presentation_feedback",
        "presented",
        &[
            Arg::Uint(u32::try_from(secs >> 32).unwrap()),
            Arg::Uint(u32::try_from(secs & 0xffff_ffff).unwrap()),
            Arg::Uint(shown.subsec_nanos()),
            Arg::Uint(refresh),
            Arg::Uint(0),
            Arg::Uint(1),
            Arg::Uint(0),
        ],
    );
    server.send(callback, "wl_callback", "done", &[Arg::Uint(16)]);
    pump(&mut conn, &mut w);
    assert_eq!(w.next_event(), Some(Event::Frame));
    let p = w.presented().expect("presented");
    assert_eq!(p.refresh, Duration::from_nanos(u64::from(refresh)));
    assert_eq!(
        w.frame_interval(),
        Some(p.refresh),
        "the refresh is a frame"
    );
    // The refresh lands a period after it was shown, read before the
    // clock was: a stall past a period only makes it later. Darwin's
    // CLOCK_MONOTONIC reads whole microseconds, and the test's read and
    // the window's are each cut down to one, so the bound allows two.
    let now = Instant::now();
    let next = w.next_shown(now);
    let resolution = Duration::from_micros(2);
    assert!(
        next + resolution >= before + p.refresh - Duration::from_millis(1)
            && next <= now + p.refresh,
        "{:?} after now",
        next.checked_duration_since(now)
    );

    // The next frame's feedback discarded: the last presentation stands.
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Strips(&[])),
        Presented::Yes
    );
    conn.flush().unwrap();
    let feedback = loop {
        let r = server.next();
        if let ("wp_presentation", "feedback", [_, Val::New(fb)]) = (r.iface, r.name, &r.args[..]) {
            break *fb;
        }
    };
    server.send(feedback, "wp_presentation_feedback", "discarded", &[]);
    pump(&mut conn, &mut w);
    assert_eq!(w.presented(), Some(p));
}

/// Requests as the server read them: interface, name and arguments.
type Requests = Vec<(&'static str, &'static str, Vec<Val>)>;

/// A session with the pacing globals among `extra`, tearing set to
/// `tearing` before the registry is read: the window, the server, every
/// request up to the surface's first commit, and the id each global was
/// bound as, by its interface, the registry's among them. A
/// `wp_presentation` bound is told the monotonic clock.
fn paced_session(
    extra: &[(&str, u32)],
    tearing: Tearing,
) -> (
    Connection,
    Server,
    Window,
    Requests,
    HashMap<String, ObjectId>,
) {
    let (c, s) = UnixStream::pair().unwrap();
    let mut conn = Connection::over(c);
    let mut server = Server::new(s);
    let mut w = Window::open(&mut conn, "baud", "org.baud.terminal").unwrap();
    w.set_tearing(&mut conn.sender(), tearing);
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
    let globals = [("wl_compositor", 6u32), ("wl_shm", 2), ("xdg_wm_base", 6)];
    for (name, (iface, version)) in (1u32..).zip(globals.iter().chain(extra)) {
        server.send(
            registry,
            "wl_registry",
            "global",
            &[Arg::Uint(name), Arg::Str(Some(iface)), Arg::Uint(*version)],
        );
    }
    server.send(sync, "wl_callback", "done", &[Arg::Uint(1)]);
    pump(&mut conn, &mut w);
    let (mut seen, mut bound, mut xdg, mut top) = (Vec::new(), HashMap::new(), None, None);
    bound.insert("wl_registry".to_string(), registry);
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            ("wl_registry", "bind", [_, Val::NewWith(i, v, id)]) => {
                if extra.iter().any(|(e, _)| e == i) {
                    let want = if i == "wp_presentation" { 2 } else { 1 };
                    assert_eq!(*v, want, "{i}");
                }
                bound.insert(i.clone(), *id);
            }
            ("xdg_wm_base", "get_xdg_surface", [Val::New(id), _]) => xdg = Some(*id),
            ("xdg_surface", "get_toplevel", [Val::New(id)]) => top = Some(*id),
            ("wl_surface", "commit", _) => break,
            _ => {}
        }
        seen.push((r.iface, r.name, r.args));
    }
    if let Some(&p) = bound.get("wp_presentation") {
        server.send(p, "wp_presentation", "clock_id", &[Arg::Uint(MONOTONIC)]);
    }
    configure(&mut server, top.unwrap(), xdg.unwrap(), 64, 32, &[]);
    pump(&mut conn, &mut w);
    server.expect("xdg_surface", "ack_configure");
    assert!(matches!(w.next_event(), Some(Event::Configured { .. })));
    (conn, server, w, seen, bound)
}

/// One commit's requests, by interface and name, up to and with the
/// commit, the arguments of each kept.
fn commit_requests(server: &mut Server) -> Requests {
    let mut out = Vec::new();
    loop {
        let r = server.next();
        let end = (r.iface, r.name) == ("wl_surface", "commit");
        out.push((r.iface, r.name, r.args));
        if end {
            return out;
        }
    }
}

/// The names of `requests`, without their arguments, the pool's setup
/// and the damage left out.
fn names(requests: &[(&'static str, &'static str, Vec<Val>)]) -> Vec<String> {
    requests
        .iter()
        .filter(|(i, n, _)| {
            !matches!(
                (*i, *n),
                ("wl_shm", "create_pool")
                    | ("wl_shm_pool", "create_buffer")
                    | ("wl_surface", "damage_buffer")
            )
        })
        .map(|(i, n, _)| format!("{i}.{n}"))
        .collect()
}

/// The frame callback and the feedback object a commit asked for.
fn callback_and_feedback(
    requests: &[(&'static str, &'static str, Vec<Val>)],
) -> (ObjectId, Option<ObjectId>) {
    let mut cb = None;
    let mut fb = None;
    for (i, n, a) in requests {
        match (*i, *n, &a[..]) {
            ("wl_surface", "frame", [Val::New(id)]) => cb = Some(*id),
            ("wp_presentation", "feedback", [_, Val::New(id)]) => fb = Some(*id),
            _ => {}
        }
    }
    (cb.expect("a frame callback"), fb)
}

/// The frame `requests` committed, shown `ago` before now on the
/// monotonic clock at a refresh of `refresh`, its callback done and every
/// buffer released: when it was shown, on the presentation clock.
fn shown(
    server: &mut Server,
    conn: &mut Connection,
    w: &mut Window,
    requests: &[(&'static str, &'static str, Vec<Val>)],
    ago: std::time::Duration,
    refresh: std::time::Duration,
) -> std::time::Duration {
    let (callback, feedback) = callback_and_feedback(requests);
    let clock = baud_present_wayland::sys::clock_now(ClockId(i32::try_from(MONOTONIC).unwrap()))
        .expect("the monotonic clock");
    let at = clock.0 - ago;
    if let Some(fb) = feedback {
        server.send(
            fb,
            "wp_presentation_feedback",
            "presented",
            &[
                Arg::Uint(u32::try_from(at.as_secs() >> 32).unwrap()),
                Arg::Uint(u32::try_from(at.as_secs() & 0xffff_ffff).unwrap()),
                Arg::Uint(at.subsec_nanos()),
                Arg::Uint(u32::try_from(refresh.as_nanos()).unwrap()),
                Arg::Uint(0),
                Arg::Uint(1),
                Arg::Uint(0),
            ],
        );
    }
    for (i, n, a) in requests {
        if let (&"wl_surface", &"attach", [Val::Obj(b), ..]) = (i, n, &a[..]) {
            server.send(*b, "wl_buffer", "release", &[]);
        }
    }
    server.send(callback, "wl_callback", "done", &[Arg::Uint(16)]);
    pump(conn, w);
    while w.next_event().is_some() {}
    at
}

/// The timestamp a commit carried, on the presentation clock.
fn stamp_of(requests: &[(&'static str, &'static str, Vec<Val>)]) -> Option<std::time::Duration> {
    requests
        .iter()
        .find_map(|(i, n, a)| match (*i, *n, &a[..]) {
            (
                "wp_commit_timer_v1",
                "set_timestamp",
                [Val::Uint(hi), Val::Uint(lo), Val::Uint(ns)],
            ) => Some(std::time::Duration::new(
                u64::from(*hi) << 32 | u64::from(*lo),
                *ns,
            )),
            _ => None,
        })
}

const PACED: [(&str, u32); 3] = [
    ("wp_presentation", 2),
    ("wp_fifo_manager_v1", 1),
    ("wp_commit_timing_manager_v1", 1),
];

/// **Where the compositor offers fifo-v1 and commit-timing-v1, a frame
/// keeps to the refresh** (`window.rs`, `timing.rs`). Each global is bound
/// at version 1 and the surface given one fifo object and one timer before
/// its first commit. Every commit that attaches a buffer sets the barrier
/// and waits on it, after the frame callback and the feedback and before
/// the commit; a commit carrying the callback alone carries no barrier. A
/// frame drawn for a moment is stamped, once, half a period before the
/// refresh nearest that moment on the grid of whole periods from the last
/// frame shown -- exactly, whatever the clocks read -- and a frame drawn
/// for no moment, or before anything was shown, carries no stamp.
#[test]
fn the_pacing_globals_hold_each_picture_a_refresh_and_a_drawn_moment_to_its_refresh() {
    let (mut conn, mut server, mut w, setup, bound) = paced_session(&PACED, Tearing::Off);
    let surface = setup
        .iter()
        .find_map(|(i, n, a)| match (*i, *n, &a[..]) {
            ("wl_compositor", "create_surface", [Val::New(id)]) => Some(*id),
            _ => None,
        })
        .expect("a surface");
    let made: Vec<_> = setup
        .iter()
        .filter(|(i, _, _)| i.starts_with("wp_fifo") || i.starts_with("wp_commit"))
        .map(|(i, n, a)| (*i, *n, a.clone()))
        .collect();
    assert_eq!(made.len(), 2, "{:?}", names(&setup));
    for (iface, name, args) in &made {
        assert!(
            matches!(
                (*iface, *name),
                ("wp_fifo_manager_v1", "get_fifo") | ("wp_commit_timing_manager_v1", "get_timer")
            ),
            "{iface}.{name}"
        );
        assert!(
            matches!(&args[..], [Val::New(_), Val::Obj(s)] if *s == surface),
            "{args:?}"
        );
    }
    assert!(
        bound.contains_key("wp_fifo_manager_v1")
            && bound.contains_key("wp_commit_timing_manager_v1")
    );
    let frame = vec![0xff00_0000u32; 64 * 32];

    // Nothing shown yet: the barrier, and no stamp for a moment there is
    // no refresh to place.
    w.draw_for(std::time::Instant::now());
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    let first = commit_requests(&mut server);
    assert_eq!(
        names(&first),
        [
            "wl_surface.attach",
            "wl_surface.frame",
            "wp_presentation.feedback",
            "wp_fifo_v1.set_barrier",
            "wp_fifo_v1.wait_barrier",
            "wl_surface.commit"
        ]
    );
    let refresh = std::time::Duration::from_secs(1);
    let r = refresh;
    let at = shown(
        &mut server,
        &mut conn,
        &mut w,
        &first,
        std::time::Duration::from_millis(3),
        refresh,
    );

    // Drawn for the next refresh: stamped half a period before it.
    let moment = w.next_shown(std::time::Instant::now());
    w.draw_for(moment);
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    let second = commit_requests(&mut server);
    assert_eq!(
        names(&second),
        [
            "wl_surface.attach",
            "wl_surface.frame",
            "wp_presentation.feedback",
            "wp_fifo_v1.set_barrier",
            "wp_fifo_v1.wait_barrier",
            "wp_commit_timer_v1.set_timestamp",
            "wl_surface.commit"
        ]
    );
    let stamp = stamp_of(&second).expect("a stamp");
    let refresh_at = stamp + r / 2;
    assert!(refresh_at > at, "{refresh_at:?} after {at:?}");
    assert_eq!(
        (refresh_at - at).as_nanos() % r.as_nanos(),
        0,
        "on the grid"
    );
    assert_eq!(
        (refresh_at - at).as_nanos() / r.as_nanos(),
        1,
        "the next refresh"
    );
    let at = shown(
        &mut server,
        &mut conn,
        &mut w,
        &second,
        std::time::Duration::from_millis(3),
        refresh,
    );

    // Drawn for the refresh after the next: stamped a period later.
    let moment = w.next_shown(std::time::Instant::now()) + r;
    w.draw_for(moment);
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    let third = commit_requests(&mut server);
    let refresh_at = stamp_of(&third).expect("a stamp") + r / 2;
    assert_eq!((refresh_at - at).as_nanos(), 2 * r.as_nanos());
    let _ = shown(
        &mut server,
        &mut conn,
        &mut w,
        &third,
        std::time::Duration::from_millis(3),
        refresh,
    );

    // Drawn for no moment: the barrier alone.
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    let fourth = commit_requests(&mut server);
    assert_eq!(stamp_of(&fourth), None);
    assert!(names(&fourth).contains(&"wp_fifo_v1.wait_barrier".to_string()));
    let _ = shown(
        &mut server,
        &mut conn,
        &mut w,
        &fourth,
        std::time::Duration::from_millis(3),
        refresh,
    );

    // The callback alone, drawn for a moment: the stamp and no barrier;
    // drawn for none, neither.
    w.draw_for(w.next_shown(std::time::Instant::now()));
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Strips(&[])),
        Presented::Yes
    );
    conn.flush().unwrap();
    let fifth = commit_requests(&mut server);
    assert_eq!(
        names(&fifth),
        [
            "wl_surface.frame",
            "wp_presentation.feedback",
            "wp_commit_timer_v1.set_timestamp",
            "wl_surface.commit"
        ]
    );
    let _ = shown(
        &mut server,
        &mut conn,
        &mut w,
        &fifth,
        std::time::Duration::from_millis(3),
        refresh,
    );
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Strips(&[])),
        Presented::Yes
    );
    conn.flush().unwrap();
    let sixth = commit_requests(&mut server);
    assert_eq!(
        names(&sixth),
        [
            "wl_surface.frame",
            "wp_presentation.feedback",
            "wl_surface.commit"
        ]
    );

    // A moment given to a frame refused goes with it: the next is drawn
    // for none.
    w.draw_for(w.next_shown(std::time::Instant::now()));
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::No,
        "in flight"
    );
    let _ = shown(
        &mut server,
        &mut conn,
        &mut w,
        &sixth,
        std::time::Duration::from_millis(3),
        refresh,
    );
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    assert_eq!(stamp_of(&commit_requests(&mut server)), None);
}

/// **The frame after one in flight is planned for the refresh after
/// that one's** (`timing.rs`): a compositor that sends a frame's callback
/// once it has latched it and its presentation only when it is shown --
/// weston does -- has that refresh taken when the next frame is drawn,
/// so the next is first seen a refresh later. With the feedback
/// outstanding, the next frame is planned and stamped for the refresh
/// two after the last shown; once the frame committed last is shown, the
/// next refresh after now again, and a `discarded` for a frame already
/// replaced changes nothing.
#[test]
fn the_frame_after_one_in_flight_is_planned_for_the_refresh_after_it() {
    let (mut conn, mut server, mut w, _, _) = paced_session(&PACED, Tearing::Off);
    let frame = vec![0xff00_0000u32; 64 * 32];
    let refresh = std::time::Duration::from_secs(1);
    let r = refresh;
    let ms3 = std::time::Duration::from_millis(3);
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    let first = commit_requests(&mut server);
    let at = shown(&mut server, &mut conn, &mut w, &first, ms3, refresh);

    // The second frame's callback comes, its feedback does not.
    w.draw_for(w.next_shown(std::time::Instant::now()));
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    let second = commit_requests(&mut server);
    let k = |requests: &Requests, at: std::time::Duration| {
        let t = stamp_of(requests).expect("a stamp") + r / 2;
        assert_eq!((t - at).as_nanos() % r.as_nanos(), 0, "on the grid");
        (t - at).as_nanos() / r.as_nanos()
    };
    assert_eq!(k(&second, at), 1);
    let (callback, feedback) = callback_and_feedback(&second);
    for (i, n, a) in &second {
        if let (&"wl_surface", &"attach", [Val::Obj(b), ..]) = (i, n, &a[..]) {
            server.send(*b, "wl_buffer", "release", &[]);
        }
    }
    server.send(callback, "wl_callback", "done", &[Arg::Uint(32)]);
    pump(&mut conn, &mut w);
    while w.next_event().is_some() {}
    let now = std::time::Instant::now();
    let next = w.next_shown(now);
    assert!(next > now + r / 2, "past the refresh the second holds");
    w.draw_for(next);
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    let third = commit_requests(&mut server);
    assert_eq!(k(&third, at), 2, "the refresh after the one in flight");

    // The second discarded after the third replaced it, and the third
    // shown: the fourth is planned for the next refresh again.
    let feedback = feedback.expect("feedback");
    server.send(feedback, "wp_presentation_feedback", "discarded", &[]);
    let at = shown(&mut server, &mut conn, &mut w, &third, ms3, refresh);
    w.draw_for(w.next_shown(std::time::Instant::now()));
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    assert_eq!(k(&commit_requests(&mut server), at), 1);
}

/// **After a pause, the frame after one in flight is still planned past
/// it**: the last frame shown three refreshes and a little ago, a frame
/// committed now is stamped for the fourth refresh after it; with that
/// frame's callback come and its feedback not, the next is stamped for
/// the fifth, not the fourth that frame holds.
#[test]
fn after_a_pause_the_frame_after_one_in_flight_is_planned_past_it() {
    let (mut conn, mut server, mut w, _, _) = paced_session(&PACED, Tearing::Off);
    let frame = vec![0xff00_0000u32; 64 * 32];
    let r = std::time::Duration::from_secs(1);
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    let first = commit_requests(&mut server);
    let ago = r * 3 + std::time::Duration::from_millis(3);
    let at = shown(&mut server, &mut conn, &mut w, &first, ago, r);
    let k = |requests: &Requests| {
        let t = stamp_of(requests).expect("a stamp") + r / 2;
        assert_eq!((t - at).as_nanos() % r.as_nanos(), 0, "on the grid");
        (t - at).as_nanos() / r.as_nanos()
    };
    w.draw_for(w.next_shown(std::time::Instant::now()));
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    let second = commit_requests(&mut server);
    assert_eq!(k(&second), 4);
    let (callback, _) = callback_and_feedback(&second);
    for (i, n, a) in &second {
        if let (&"wl_surface", &"attach", [Val::Obj(b), ..]) = (i, n, &a[..]) {
            server.send(*b, "wl_buffer", "release", &[]);
        }
    }
    server.send(callback, "wl_callback", "done", &[Arg::Uint(32)]);
    pump(&mut conn, &mut w);
    while w.next_event().is_some() {}
    w.draw_for(w.next_shown(std::time::Instant::now()));
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    assert_eq!(
        k(&commit_requests(&mut server)),
        5,
        "past the refresh the second holds"
    );
}

/// **Without the pacing globals a frame carries what it always did**: a
/// compositor that offers only `wp_presentation` is sent the frame
/// callback and the feedback and nothing more, a moment to draw for or
/// tearing turned on included; one that offers the timer but no
/// presentation clock gets no stamp, there being no clock to name a time
/// on; and a fifo-v1 or commit-timing-v1 global that comes after the
/// surface gives it its object then, once.
#[test]
fn without_the_pacing_globals_a_frame_carries_nothing_more() {
    let frame = vec![0xff00_0000u32; 64 * 32];
    let (mut conn, mut server, mut w, setup, _) =
        paced_session(&[("wp_presentation", 2)], Tearing::On);
    assert!(
        !names(&setup).iter().any(|n| n.starts_with("wp_fifo")
            || n.starts_with("wp_commit")
            || n.starts_with("wp_tearing")),
        "{:?}",
        names(&setup)
    );
    let refresh = std::time::Duration::from_nanos(16_666_667);
    let mut last = Vec::new();
    for _ in 0..2 {
        w.draw_for(w.next_shown(std::time::Instant::now()));
        assert_eq!(
            w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
            Presented::Yes
        );
        conn.flush().unwrap();
        last = commit_requests(&mut server);
        assert_eq!(
            names(&last),
            [
                "wl_surface.attach",
                "wl_surface.frame",
                "wp_presentation.feedback",
                "wl_surface.commit"
            ]
        );
        let _ = shown(
            &mut server,
            &mut conn,
            &mut w,
            &last,
            std::time::Duration::from_millis(3),
            refresh,
        );
    }
    drop(last);

    // The timer, and no presentation to read a clock from.
    let (mut conn, mut server, mut w, _, _) =
        paced_session(&[("wp_commit_timing_manager_v1", 1)], Tearing::Off);
    w.draw_for(std::time::Instant::now());
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    assert_eq!(
        names(&commit_requests(&mut server)),
        ["wl_surface.attach", "wl_surface.frame", "wl_surface.commit"]
    );

    // The globals offered once the surface exists.
    let (mut conn, mut server, mut w, _, bound) = paced_session(&[], Tearing::Off);
    let registry = bound["wl_registry"];
    for (name, iface) in [
        (20u32, "wp_fifo_manager_v1"),
        (21, "wp_commit_timing_manager_v1"),
    ] {
        server.send(
            registry,
            "wl_registry",
            "global",
            &[Arg::Uint(name), Arg::Str(Some(iface)), Arg::Uint(1)],
        );
    }
    pump(&mut conn, &mut w);
    let mut got = Vec::new();
    for _ in 0..4 {
        let r = server.next();
        got.push(format!("{}.{}", r.iface, r.name));
    }
    assert_eq!(
        got,
        [
            "wl_registry.bind",
            "wp_fifo_manager_v1.get_fifo",
            "wl_registry.bind",
            "wp_commit_timing_manager_v1.get_timer"
        ]
    );
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    assert_eq!(
        names(&commit_requests(&mut server)),
        [
            "wl_surface.attach",
            "wl_surface.frame",
            "wp_fifo_v1.set_barrier",
            "wp_fifo_v1.wait_barrier",
            "wl_surface.commit"
        ]
    );
}

/// **Tearing on asks the compositor for `async` and drops the pacing**
/// (`[window] tearing`): with tearing-control-v1 offered and tearing on,
/// the surface's control is made before its first commit and hinted
/// `async`, and a frame carries neither the barrier nor a stamp; turned
/// off, the hint goes back to `vsync` and both return; on again, `async`.
/// Off from the start makes no control at all; and on with no
/// tearing-control offered keeps the pacing, the frames being vsync'd.
#[test]
fn tearing_on_asks_for_async_and_drops_the_pacing() {
    use baud_wayland::protocol::wp_tearing_control_v1::presentation_hint::{ASYNC, VSYNC};
    let frame = vec![0xff00_0000u32; 64 * 32];
    let mut offered = PACED.to_vec();
    offered.push(("wp_tearing_control_manager_v1", 1));
    let refresh = std::time::Duration::from_nanos(16_666_667);
    let ms3 = std::time::Duration::from_millis(3);
    let (mut conn, mut server, mut w, setup, _) = paced_session(&offered, Tearing::On);
    let tearing: Vec<_> = setup
        .iter()
        .filter(|(i, _, _)| i.starts_with("wp_tearing"))
        .map(|(i, n, a)| (format!("{i}.{n}"), a.clone()))
        .collect();
    assert_eq!(tearing.len(), 2, "{tearing:?}");
    assert_eq!(
        tearing[0].0,
        "wp_tearing_control_manager_v1.get_tearing_control"
    );
    assert_eq!(tearing[1].0, "wp_tearing_control_v1.set_presentation_hint");
    assert!(
        matches!(tearing[1].1[..], [Val::Uint(ASYNC)]),
        "{:?}",
        tearing[1].1
    );
    let plain = [
        "wl_surface.attach",
        "wl_surface.frame",
        "wp_presentation.feedback",
        "wl_surface.commit",
    ];
    let next = |w: &mut Window, conn: &mut Connection, server: &mut Server| {
        w.draw_for(w.next_shown(std::time::Instant::now()));
        assert_eq!(
            w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
            Presented::Yes
        );
        conn.flush().unwrap();
        let r = commit_requests(server);
        let _ = shown(server, conn, w, &r, ms3, refresh);
        r
    };
    let _ = next(&mut w, &mut conn, &mut server);
    assert_eq!(
        names(&next(&mut w, &mut conn, &mut server)),
        plain,
        "torn: neither"
    );

    w.set_tearing(&mut conn.sender(), Tearing::Off);
    let r = next(&mut w, &mut conn, &mut server);
    assert_eq!(
        names(&r),
        [
            "wp_tearing_control_v1.set_presentation_hint",
            "wl_surface.attach",
            "wl_surface.frame",
            "wp_presentation.feedback",
            "wp_fifo_v1.set_barrier",
            "wp_fifo_v1.wait_barrier",
            "wp_commit_timer_v1.set_timestamp",
            "wl_surface.commit"
        ]
    );
    assert!(matches!(r[0].2[..], [Val::Uint(VSYNC)]), "{:?}", r[0].2);
    w.set_tearing(&mut conn.sender(), Tearing::On);
    let r = next(&mut w, &mut conn, &mut server);
    assert_eq!(names(&r)[0], "wp_tearing_control_v1.set_presentation_hint");
    assert!(matches!(r[0].2[..], [Val::Uint(ASYNC)]), "{:?}", r[0].2);
    assert_eq!(names(&r)[1..], plain);

    // Off from the start: no control; turned on, it is made and hinted.
    let (mut conn, mut server, mut w, setup, _) = paced_session(&offered, Tearing::Off);
    assert!(
        !names(&setup)
            .iter()
            .any(|n| n.starts_with("wp_tearing_control_manager_v1.get"))
    );
    let _ = next(&mut w, &mut conn, &mut server);
    w.set_tearing(&mut conn.sender(), Tearing::On);
    let r = next(&mut w, &mut conn, &mut server);
    assert_eq!(
        names(&r)[..2],
        [
            "wp_tearing_control_manager_v1.get_tearing_control",
            "wp_tearing_control_v1.set_presentation_hint"
        ]
    );
    assert_eq!(names(&r)[2..], plain);

    // On with no tearing-control: vsync'd, so paced.
    let (mut conn, mut server, mut w, _, _) = paced_session(&PACED, Tearing::On);
    let _ = next(&mut w, &mut conn, &mut server);
    let r = next(&mut w, &mut conn, &mut server);
    assert!(names(&r).contains(&"wp_fifo_v1.wait_barrier".to_string()));
    assert!(stamp_of(&r).is_some());
}

/// A session through its first configure with `extra` globals offered
/// after the three it needs: the window, the server, and the ids the
/// extras were bound as, in order.
fn session_with(extra: &[(&str, u32)]) -> (Connection, Server, Window, Vec<ObjectId>) {
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
    let globals = [("wl_compositor", 6u32), ("wl_shm", 2), ("xdg_wm_base", 6)];
    for (name, (iface, version)) in (1u32..).zip(globals.iter().chain(extra)) {
        server.send(
            registry,
            "wl_registry",
            "global",
            &[Arg::Uint(name), Arg::Str(Some(iface)), Arg::Uint(*version)],
        );
    }
    server.send(sync, "wl_callback", "done", &[Arg::Uint(1)]);
    pump(&mut conn, &mut w);
    let (mut bound, mut xdg, mut top) = (Vec::new(), None, None);
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            ("wl_registry", "bind", [_, Val::NewWith(i, _, id)])
                if extra.iter().any(|(e, _)| e == i) =>
            {
                bound.push(*id)
            }
            ("xdg_wm_base", "get_xdg_surface", [Val::New(id), _]) => xdg = Some(*id),
            ("xdg_surface", "get_toplevel", [Val::New(id)]) => top = Some(*id),
            ("wl_surface", "commit", _) => break,
            _ => {}
        }
    }
    configure(&mut server, top.unwrap(), xdg.unwrap(), 64, 32, &[]);
    pump(&mut conn, &mut w);
    server.expect("xdg_surface", "ack_configure");
    assert!(matches!(w.next_event(), Some(Event::Configured { .. })));
    (conn, server, w, bound)
}

/// **The system bell rings where the compositor offers it**:
/// the `xdg-system-bell-v1` global is bound at version 1 and a ring names the
/// window's surface; without it a ring sends nothing and says so.
#[test]
fn the_system_bell_rings_where_the_compositor_offers_it() {
    let (mut conn, mut server, mut w, bound) = session_with(&[("xdg_system_bell_v1", 1)]);
    let bell = bound[0];
    assert_eq!(w.ring_bell(&mut conn.sender()), Rung::Sent);
    conn.flush().unwrap();
    let r = server.expect("xdg_system_bell_v1", "ring");
    assert_eq!(r.sender, bell);
    assert!(
        matches!(r.args[0], Val::Obj(o) if o != ObjectId::NULL),
        "the window's surface"
    );

    let (mut conn, mut server, mut w, _) = session_with(&[]);
    assert_eq!(w.ring_bell(&mut conn.sender()), Rung::NoBell);
    // The control: the next request is the title, not a ring.
    w.set_title(&mut conn.sender(), "after");
    conn.flush().unwrap();
    server.expect("xdg_toplevel", "set_title");
}

/// **The window is activated through `xdg-activation-v1`**: a token asked
/// for with the window's surface and committed, `activate` on the surface
/// with the token its `done` brings and the token's object destroyed; a
/// second ask while the first token is out sends nothing; a token another
/// client gave is passed to `activate` as it is; and without the global
/// nothing is sent.
#[test]
fn the_window_is_activated_with_a_token_it_asks_for_or_is_given() {
    let (mut conn, mut server, mut w, bound) = session_with(&[("xdg_activation_v1", 1)]);
    let activation = bound[0];
    let ask = |server: &mut Server| {
        let r = server.expect("xdg_activation_v1", "get_activation_token");
        assert_eq!(r.sender, activation);
        let Val::New(token) = r.args[0] else {
            panic!("{:?}", r.args)
        };
        let r = server.expect("xdg_activation_token_v1", "set_surface");
        assert_eq!(r.sender, token);
        let Val::Obj(surface) = r.args[0] else {
            panic!("{:?}", r.args)
        };
        assert_ne!(surface, ObjectId::NULL, "the window's surface");
        let r = server.expect("xdg_activation_token_v1", "commit");
        assert_eq!(r.sender, token);
        (token, surface)
    };
    assert_eq!(w.activate(&mut conn.sender()), Activated::Asked);
    conn.flush().unwrap();
    let (token, surface) = ask(&mut server);
    // One token in flight: the second ask sends nothing -- the next request
    // is the title.
    assert_eq!(w.activate(&mut conn.sender()), Activated::Pending);
    w.set_title(&mut conn.sender(), "between");
    conn.flush().unwrap();
    server.expect("xdg_toplevel", "set_title");
    server.send(
        token,
        "xdg_activation_token_v1",
        "done",
        &[Arg::Str(Some("tok-1"))],
    );
    pump(&mut conn, &mut w);
    let r = server.expect("xdg_activation_v1", "activate");
    assert_eq!(r.sender, activation);
    assert!(
        matches!(&r.args[..], [Val::Str(Some(t)), Val::Obj(o)] if t == "tok-1" && *o == surface),
        "{:?}",
        r.args
    );
    let r = server.expect("xdg_activation_token_v1", "destroy");
    assert_eq!(r.sender, token);
    // The token came, so the next ask asks again.
    assert_eq!(w.activate(&mut conn.sender()), Activated::Asked);
    conn.flush().unwrap();
    let (second, _) = ask(&mut server);
    assert_ne!(second, token, "a fresh token object");
    // A token another client gave goes straight to `activate`.
    assert_eq!(
        w.activate_with(&mut conn.sender(), "from-the-server"),
        Activated::Sent
    );
    conn.flush().unwrap();
    let r = server.expect("xdg_activation_v1", "activate");
    assert!(
        matches!(&r.args[..], [Val::Str(Some(t)), Val::Obj(o)] if t == "from-the-server" && *o == surface),
        "{:?}",
        r.args
    );

    let (mut conn, mut server, mut w, _) = session_with(&[]);
    assert_eq!(w.activate(&mut conn.sender()), Activated::Unavailable);
    assert_eq!(
        w.activate_with(&mut conn.sender(), "from-the-server"),
        Activated::Unavailable
    );
    // The control: the next request is the title, not a token.
    w.set_title(&mut conn.sender(), "after");
    conn.flush().unwrap();
    server.expect("xdg_toplevel", "set_title");
}

/// **A frame's length is the gap between frame callbacks** where no
/// presentation feedback says a refresh: none before two callbacks, the
/// gap once two come 16 ms apart, and a gap past a tenth of a second --
/// a pause in drawing -- leaves it.
#[test]
fn a_frames_length_is_the_gap_between_frame_callbacks() {
    let (mut conn, mut server, mut w, _) = session_with(&[]);
    let frame = vec![0u32; 64 * 32];
    let next = |server: &mut Server, conn: &mut Connection, w: &mut Window, ms: u32| {
        assert_eq!(
            w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
            Presented::Yes
        );
        conn.flush().unwrap();
        let (attached, _, cb) = frame_requests(server);
        server.send(attached.unwrap(), "wl_buffer", "release", &[]);
        server.send(cb, "wl_callback", "done", &[Arg::Uint(ms)]);
        pump(conn, w);
        while w.next_event().is_some() {}
    };
    assert_eq!(w.frame_interval(), None);
    next(&mut server, &mut conn, &mut w, 1000);
    assert_eq!(w.frame_interval(), None, "one callback is no gap");
    next(&mut server, &mut conn, &mut w, 1016);
    let ms = std::time::Duration::from_millis;
    assert_eq!(w.frame_interval(), Some(ms(16)));
    next(&mut server, &mut conn, &mut w, 1500);
    assert_eq!(w.frame_interval(), Some(ms(16)), "a pause is not a frame");
    next(&mut server, &mut conn, &mut w, 1508);
    assert_eq!(w.frame_interval(), Some(ms(8)));
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

/// The events a step left, but a change of the modifiers: the key tests
/// read the keys, and `a_change_of_the_modifiers_is_an_event` the changes.
fn drain(w: &mut Window) -> Vec<Event> {
    std::iter::from_fn(|| w.next_event())
        .filter(|e| !matches!(e, Event::Modifiers(_)))
        .collect()
}

/// **A change of what the keyboard holds is an event**, so a driver can
/// show the link under a resting pointer while Ctrl is held; the same
/// state sent again, which a compositor does after every key, is not.
/// Perturbation: push the event on every `modifiers` -- the second
/// assertion fails.
#[test]
fn a_change_of_the_modifiers_is_an_event() {
    let (mut conn, mut server, mut w, kb, _) = keyboard_session();
    let mods = |w: &mut Window| {
        std::iter::from_fn(|| w.next_event())
            .filter(|e| matches!(e, Event::Modifiers(_)))
            .count()
    };
    let _ = mods(&mut w);
    depressed(&mut server, kb, 1);
    pump(&mut conn, &mut w);
    assert_eq!(mods(&mut w), 1, "Shift pressed");
    depressed(&mut server, kb, 1);
    pump(&mut conn, &mut w);
    assert_eq!(mods(&mut w), 0, "the same state again");
    depressed(&mut server, kb, 0);
    pump(&mut conn, &mut w);
    assert_eq!(mods(&mut w), 1, "Shift let go");
    assert!(!w.modifiers().has(baud_domain::Modifiers::SHIFT));
}

fn typed(c: char, mods: baud_domain::Modifiers, kind: baud_domain::KeyEventKind) -> Event {
    use baud_domain::{Codepoint, KeyCode, KeyEvent};
    Event::Key(KeyEvent {
        kind,
        ..KeyEvent::press(KeyCode::Char(Codepoint::from_char(c)), mods)
    })
}

/// The key `c` in a Control, Alt or Super chord: no text, `c` the chord's
/// character.
fn chorded(c: char, mods: baud_domain::Modifiers, kind: baud_domain::KeyEventKind) -> Event {
    use baud_domain::{Codepoint, KeyCode, KeyEvent};
    Event::Key(KeyEvent {
        kind,
        text: None,
        chord_text: Some(Codepoint::from_char(c)),
        ..KeyEvent::press(KeyCode::Char(Codepoint::from_char(c)), mods)
    })
}

/// The key `key` typed with Shift, `typed` its text and its shifted key.
fn shifted(key: char, typed: char, kind: baud_domain::KeyEventKind) -> Event {
    use baud_domain::{Codepoint, KeyCode, KeyEvent, Modifiers};
    let typed = Some(Codepoint::from_char(typed));
    Event::Key(KeyEvent {
        kind,
        text: typed,
        shifted: typed,
        ..KeyEvent::press(KeyCode::Char(Codepoint::from_char(key)), Modifiers::SHIFT)
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
    // Shift pressed while q is held: the repeat is the key with Shift, its
    // capital the text and the shifted key.
    depressed(&mut server, kb, 1);
    pump(&mut conn, &mut w);
    w.tick(due + 120 * MS);
    assert_eq!(drain(&mut w), [shifted('q', 'Q', KeyEventKind::Repeat)]);
    key(&mut server, kb, KEY_Q, RELEASED);
    pump(&mut conn, &mut w);
    assert_eq!(
        drain(&mut w),
        [typed('q', Modifiers::SHIFT, KeyEventKind::Release)],
        "the release is delivered, naming the key its press named"
    );
    assert_eq!(w.repeat_deadline(), None, "and the repeat stops");
    w.tick(due + std::time::Duration::from_secs(5));
    assert!(drain(&mut w).is_empty());
}

/// **A release names the key its press named, and follows only a press
/// the window sent**, as kitty's layer pairs them (`[[source:
/// kitty-glfw-input-c-6d5d0c4]]`, `_glfwInputKeyboard`): `q` let go with
/// Shift now held is `q` with the release's Shift, not `Q`; a key already
/// down when the keyboard entered, whose press was never sent, releases
/// nothing, and nor does a key pressed before the keyboard left and let go
/// after it came back.
/// Perturbation: name the release by the keyboard's state at the release
/// and the first assertion reads `Q`; send a release with no press and the
/// second reads one.
#[test]
fn a_release_names_its_press_and_follows_only_a_press_sent() {
    use baud_domain::{KeyEventKind, Modifiers};
    let (mut conn, mut server, mut w, kb, surface) = keyboard_session();
    key(&mut server, kb, KEY_Q, PRESSED);
    depressed(&mut server, kb, 1);
    key(&mut server, kb, KEY_Q, RELEASED);
    depressed(&mut server, kb, 0);
    pump(&mut conn, &mut w);
    assert_eq!(
        drain(&mut w),
        [
            typed('q', Modifiers::NONE, KeyEventKind::Press),
            typed('q', Modifiers::SHIFT, KeyEventKind::Release)
        ]
    );
    // Down before the keyboard entered: its release alone arrives.
    key(&mut server, kb, KEY_E, RELEASED);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), [], "no press was sent");
    // Pressed, the keyboard leaves and comes back, then let go.
    key(&mut server, kb, KEY_E, PRESSED);
    server.send(
        kb,
        "wl_keyboard",
        "leave",
        &[Arg::Uint(2), Arg::Object(surface)],
    );
    keyboard_enter(&mut server, kb, 3, surface);
    key(&mut server, kb, KEY_E, RELEASED);
    pump(&mut conn, &mut w);
    let keys: Vec<Event> = drain(&mut w)
        .into_iter()
        .filter(|e| matches!(e, Event::Key(_)))
        .collect();
    assert_eq!(keys, [typed('e', Modifiers::NONE, KeyEventKind::Press)]);
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
    keyboard: ObjectId,
    /// The shape device, if one was made.
    device: Option<ObjectId>,
    surface: ObjectId,
    /// The window's fractional-scale object, where the globals were
    /// offered.
    fractional: Option<ObjectId>,
}

/// A session with a pointer: the globals bound -- `Shapes::Offered` adds
/// the cursor-shape manager, advertised at 2 -- the seat at `seat_version`,
/// and its capabilities naming the keyboard and the pointer.
fn pointer_session(seat_version: u32, shapes: Shapes) -> Pointed {
    pointer_session_at(seat_version, shapes, Scales::Preferred)
}

/// How the compositor tells a client its scale.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scales {
    /// `wl_compositor` version 6: `preferred_buffer_scale`.
    Preferred,
    /// Version 4, sway 1.7's, and one `wl_output`: its `scale`.
    Outputs,
    /// Version 6 with the viewporter and fractional-scale-v1's manager,
    /// sway 1.10's: a fraction.
    Fractional,
}

/// [`pointer_session`] with the compositor's scales told as `scales` says.
fn pointer_session_at(seat_version: u32, shapes: Shapes, scales: Scales) -> Pointed {
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
    let compositor = match scales {
        Scales::Preferred | Scales::Fractional => 6,
        Scales::Outputs => 4,
    };
    let mut globals = vec![
        (1u32, "wl_compositor", compositor),
        (2, "wl_shm", 2),
        (3, "xdg_wm_base", 6),
        (4, "wl_seat", seat_version),
    ];
    if shapes == Shapes::Offered {
        globals.push((5, "wp_cursor_shape_manager_v1", 2));
    }
    if scales == Scales::Outputs {
        globals.push((6, "wl_output", 4));
    }
    if scales == Scales::Fractional {
        globals.push((7, "wp_viewporter", 1));
        globals.push((8, "wp_fractional_scale_manager_v1", 1));
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
    let (mut seat, mut surface, mut manager, mut fractional) = (None, None, None, None);
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            (
                "wp_fractional_scale_manager_v1",
                "get_fractional_scale",
                [Val::New(id), Val::Obj(on)],
            ) => {
                assert_eq!(Some(*on), surface, "the window's surface");
                fractional = Some(*id);
            }
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
    assert_eq!(fractional.is_some(), scales == Scales::Fractional);
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
    let r = server.expect("wl_seat", "get_keyboard");
    let Val::New(keyboard) = r.args[0] else {
        panic!()
    };
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
        keyboard,
        device,
        surface,
        fractional,
    }
}

fn fixed(v: f64) -> Arg<'static> {
    Arg::Fixed(baud_wayland::Fixed::from_f64(v))
}

fn pointer_drain(w: &mut Window) -> Vec<PointerEvent> {
    drain(w)
        .into_iter()
        .map(|e| match e {
            Event::Pointer(p, _) => p,
            other => panic!("not the pointer's: {other:?}"),
        })
        .collect()
}

const VERTICAL: u32 = protocol::wl_pointer::axis::VERTICAL_SCROLL;

/// **A pointer event carries the modifiers held when it came**, not the
/// ones held when the driver reads it: Ctrl pressed, a click, Ctrl let go,
/// all in one read, is a Ctrl-click, as `wl_keyboard.modifiers` "notifies
/// clients that the modifier and/or group state has changed" from that
/// point in the stream (`[[source: wayland]]`). Read when the driver
/// drained the queue, it was a plain click: the modifiers were the
/// release's by then. Perturbation: give each pointer event the
/// modifiers held at `next_event` -- the press and the enter read none.
#[test]
fn a_pointer_event_carries_the_modifiers_of_its_moment() {
    use baud_domain::Modifiers;
    let Pointed {
        mut conn,
        mut server,
        mut w,
        pointer,
        keyboard,
        surface,
        ..
    } = pointer_session(9, Shapes::Absent);
    let _ = drain(&mut w);
    let ctrl = Modifiers::NONE.with(Modifiers::CTRL);
    depressed(&mut server, keyboard, 1 << 2);
    server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[Arg::Uint(5), Arg::Object(surface), fixed(4.0), fixed(8.0)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    let button = |state| {
        [
            Arg::Uint(6),
            Arg::Uint(100),
            Arg::Uint(0x110),
            Arg::Uint(state),
        ]
    };
    server.send(pointer, "wl_pointer", "button", &button(1));
    server.send(pointer, "wl_pointer", "frame", &[]);
    depressed(&mut server, keyboard, 0);
    server.send(pointer, "wl_pointer", "button", &button(0));
    server.send(pointer, "wl_pointer", "frame", &[]);
    pump(&mut conn, &mut w);
    let got: Vec<Event> = std::iter::from_fn(|| w.next_event()).collect();
    let left = |press| PointerEvent::Button {
        button: Button::Left,
        press,
        time: baud_domain::EventMillis::of_millis(100),
    };
    assert_eq!(
        got,
        [
            Event::Modifiers(ctrl),
            Event::Pointer(PointerEvent::Entered(At { x: 4.0, y: 8.0 }), ctrl),
            Event::Pointer(left(Press::Pressed), ctrl),
            Event::Modifiers(Modifiers::NONE),
            Event::Pointer(left(Press::Released), Modifiers::NONE),
        ]
    );
    assert_eq!(w.modifiers(), Modifiers::NONE, "what is held now");
}

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
        ..
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
        ..
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
        ..
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
            at: 120,
            destination: None,
            scale: 1,
            hotspot: (12, 12),
            image: baud_present_wayland::cursor::Drawn::IBeam,
        },
        None,
    );
    // The pointer leaves and the window's surface asks for twice the
    // pixels: the next enter's image is drawn again at two, on the same
    // surface, and the hand for `pointer`, a link's shape.
    server.send(
        pointer,
        "wl_pointer",
        "leave",
        &[Arg::Uint(4), Arg::Object(surface)],
    );
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
            at: 240,
            destination: None,
            scale: 2,
            hotspot: (9, 2),
            image: baud_present_wayland::cursor::Drawn::Hand,
        },
        Some(cursor),
    );
    // Back at one, the arrow for a shape that is neither text nor a link.
    server.send(
        pointer,
        "wl_pointer",
        "leave",
        &[Arg::Uint(5), Arg::Object(surface)],
    );
    server.send(
        surface,
        "wl_surface",
        "preferred_buffer_scale",
        &[Arg::Int(1)],
    );
    server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[Arg::Uint(6), Arg::Object(surface), fixed(1.0), fixed(2.0)],
    );
    pump(&mut conn, &mut w);
    drain(&mut w);
    drawn_pointer(
        &mut conn,
        &mut server,
        &mut w,
        (CursorShape(shape::WAIT), 6),
        Drawing {
            side: 24,
            at: 120,
            destination: None,
            scale: 1,
            hotspot: (4, 3),
            image: baud_present_wayland::cursor::Drawn::Arrow,
        },
        Some(cursor),
    );
}

/// **Below `wl_compositor` version 6 the drawn pointer takes its scale
/// from the outputs the window is on**: "It is intended that you pick the
/// same buffer scale as the scale of the output that the surface is
/// displayed on" (`wl_surface.set_buffer_scale`, `[[source: wayland]]`),
/// which sway 1.7 leaves to `wl_output.scale` and `wl_surface.enter`, having
/// no `preferred_buffer_scale`. Each output is bound at version 2, the
/// first with `scale`; the window on an output at two draws the I-beam at
/// two, and once it has left it, the arrow at one.
#[test]
fn an_old_compositor_draws_the_pointer_at_its_outputs_scale() {
    let Pointed {
        mut conn,
        mut server,
        mut w,
        pointer,
        surface,
        ..
    } = pointer_session_at(7, Shapes::Absent, Scales::Outputs);
    let output = server
        .objects
        .iter()
        .find(|(_, i)| i.name == "wl_output")
        .map(|(id, _)| *id)
        .expect("the output bound");
    server.send(output, "wl_output", "scale", &[Arg::Int(2)]);
    server.send(output, "wl_output", "done", &[]);
    server.send(surface, "wl_surface", "enter", &[Arg::Object(output)]);
    server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[Arg::Uint(3), Arg::Object(surface), fixed(1.0), fixed(2.0)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    pump(&mut conn, &mut w);
    drain(&mut w);
    use protocol::wp_cursor_shape_device_v1::shape;
    let cursor = drawn_pointer(
        &mut conn,
        &mut server,
        &mut w,
        (CursorShape(shape::TEXT), 3),
        Drawing {
            side: 48,
            at: 240,
            destination: None,
            scale: 2,
            hotspot: (12, 12),
            image: baud_present_wayland::cursor::Drawn::IBeam,
        },
        None,
    );
    server.send(
        pointer,
        "wl_pointer",
        "leave",
        &[Arg::Uint(4), Arg::Object(surface)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    server.send(surface, "wl_surface", "leave", &[Arg::Object(output)]);
    server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[Arg::Uint(5), Arg::Object(surface), fixed(1.0), fixed(2.0)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    pump(&mut conn, &mut w);
    drain(&mut w);
    drawn_pointer(
        &mut conn,
        &mut server,
        &mut w,
        (CursorShape(shape::DEFAULT), 5),
        Drawing {
            side: 24,
            at: 120,
            destination: None,
            scale: 1,
            hotspot: (4, 3),
            image: baud_present_wayland::cursor::Drawn::Arrow,
        },
        Some(cursor),
    );
}

/// What one drawn pointer should come to: the image's side in pixels, the
/// scale it is drawn at in 120ths, the destination its viewport is told at
/// a fraction, the buffer scale told with it, its hotspot, and which image,
/// the pools holding them in `Drawn::ALL`'s order.
struct Drawing {
    side: i32,
    at: i32,
    destination: Option<(i32, i32)>,
    scale: i32,
    hotspot: (i32, i32),
    image: baud_present_wayland::cursor::Drawn,
}

/// The drawn pointer's surface, and its viewport once one is made.
#[derive(Clone, Copy)]
struct Cursor {
    surface: ObjectId,
    viewport: Option<ObjectId>,
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
        ..
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

/// **The requests a drawn pointer makes, and its pixels** (see
/// [`pointer_drawn`]), `shape` set with the enter's `serial`. Returns the
/// cursor's surface.
fn drawn_pointer(
    conn: &mut Connection,
    server: &mut Server,
    w: &mut Window,
    (shape, serial): (CursorShape, u32),
    want: Drawing,
    surface: Option<ObjectId>,
) -> ObjectId {
    let cursor = surface.map(|surface| Cursor {
        surface,
        viewport: None,
    });
    shaped_pointer(conn, server, w, (shape, serial), want, cursor).surface
}

/// [`drawn_pointer`] with the cursor's viewport kept.
fn shaped_pointer(
    conn: &mut Connection,
    server: &mut Server,
    w: &mut Window,
    (shape, serial): (CursorShape, u32),
    want: Drawing,
    cursor: Option<Cursor>,
) -> Cursor {
    assert_eq!(
        w.set_pointer_shape(&mut conn.sender(), shape),
        Shaped::Drawn
    );
    conn.flush().unwrap();
    pointer_drawn(server, (serial, want), cursor)
}

/// **What a drawn pointer sent, and its pixels**: the cursor's surface
/// made once, two pools of two buffers for the three images at the scale
/// -- the old pools destroyed when the scale changed -- the image
/// attached, the buffer scale told with it and, at a fraction, the
/// viewport's destination -- the viewport made once, at the first
/// fraction -- the image damaged in surface points, the cursor role given
/// with the enter's serial and the hotspot, and a commit. The pixels are
/// read back from the pool's file: the pixel below the hotspot, in the
/// image's own pixels, is opaque black.
fn pointer_drawn(
    server: &mut Server,
    (serial, want): (u32, Drawing),
    cursor: Option<Cursor>,
) -> Cursor {
    use std::os::unix::fs::FileExt as _;
    let mut cursor = match cursor {
        Some(c) => {
            for _ in 0..2 {
                server.expect("wl_buffer", "destroy");
                server.expect("wl_buffer", "destroy");
                server.expect("wl_shm_pool", "destroy");
            }
            c
        }
        None => {
            let r = server.expect("wl_compositor", "create_surface");
            let Val::New(c) = r.args[0] else { panic!() };
            Cursor {
                surface: c,
                viewport: None,
            }
        }
    };
    let mut buffers = Vec::new();
    for _ in 0..2 {
        server.expect("wl_shm", "create_pool");
        // The request just read put its file at the back; a second pool's
        // may be in front of it, received in the same read.
        let file = std::fs::File::from(server.fds.pop_back().expect("the pool's file"));
        let file = std::rc::Rc::new(file);
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
            buffers.push((id, offset, stride, std::rc::Rc::clone(&file)));
        }
    }
    let r = server.expect("wl_surface", "attach");
    assert_eq!(r.sender, cursor.surface);
    let (buffer, offset, stride, pool) = buffers[baud_present_wayland::cursor::Drawn::ALL
        .iter()
        .position(|d| *d == want.image)
        .expect("one of the three")]
    .clone();
    assert!(
        matches!(r.args[0], Val::Obj(b) if b == buffer),
        "{:?}",
        r.args
    );
    let r = server.expect("wl_surface", "set_buffer_scale");
    assert_eq!(r.sender, cursor.surface);
    assert!(matches!(r.args[..], [Val::Int(s)] if s == want.scale));
    if let Some((dw, dh)) = want.destination {
        let viewport = match cursor.viewport {
            Some(v) => v,
            None => {
                let r = server.expect("wp_viewporter", "get_viewport");
                let [Val::New(v), Val::Obj(on)] = r.args[..] else {
                    panic!("{:?}", r.args)
                };
                assert_eq!(on, cursor.surface, "the cursor's own viewport");
                cursor.viewport = Some(v);
                v
            }
        };
        let r = server.expect("wp_viewport", "set_destination");
        assert_eq!(r.sender, viewport);
        assert!(
            matches!(r.args[..], [Val::Int(w), Val::Int(h)] if (w, h) == (dw, dh)),
            "{:?}",
            r.args
        );
    }
    let r = server.expect("wl_surface", "damage");
    assert!(matches!(
        r.args[..],
        [Val::Int(0), Val::Int(0), Val::Int(24), Val::Int(24)]
    ));
    let r = server.expect("wl_pointer", "set_cursor");
    let (hx, hy) = want.hotspot;
    assert!(
        matches!(r.args[..], [Val::Uint(n), Val::Obj(c), Val::Int(x), Val::Int(y)]
            if n == serial && c == cursor.surface && (x, y) == (hx, hy)),
        "{:?}",
        r.args
    );
    let r = server.expect("wl_surface", "commit");
    assert_eq!(r.sender, cursor.surface);
    // The pixel just below the hotspot, in the image's own pixels.
    let (px, py) = (hx * want.at / 120, (hy + 1) * want.at / 120);
    let at = u64::try_from(offset + py * stride + px * 4).unwrap();
    let mut word = [0u8; 4];
    pool.read_exact_at(&mut word, at).unwrap();
    assert_eq!(
        u32::from_ne_bytes(word),
        0xff00_0000,
        "{:?} at {}: opaque black at the hotspot",
        want.image,
        want.at
    );
    cursor
}

/// **A drawn pointer beside a fraction is drawn at it, as the window is**
/// (`[[source: wp-fractional-scale-v1]]`): "A client can submit scaled
/// content by utilizing wp_viewport ... setting the destination rectangle
/// to the surface size before the scale factor is applied", the buffer
/// "calculated by multiplying the surface size by the intended scale", and
/// "The wl_surface buffer scale should remain set to 1". Where sway 1.10
/// sends 180 and an integer 2 beside it, the I-beam's 24 points are a
/// 36-pixel image -- not 48 shrunk by the compositor -- through a viewport
/// of the cursor's own told 24 x 24, at buffer scale one, its hotspot in
/// surface points (`wl_pointer.set_cursor`, "in surface-local
/// coordinates"). Left, and entered again at 240, the hand is drawn at two
/// through the same viewport, made once. Perturbations: the destination left untold, the
/// hotspot in the image's pixels, the image drawn at the integer scale --
/// each fails.
#[test]
fn a_drawn_pointer_at_a_fraction_is_drawn_through_its_viewport() {
    let Pointed {
        mut conn,
        mut server,
        mut w,
        pointer,
        surface,
        fractional,
        ..
    } = pointer_session_at(9, Shapes::Absent, Scales::Fractional);
    let fractional = fractional.expect("the window's fractional-scale object");
    server.send(
        fractional,
        "wp_fractional_scale_v1",
        "preferred_scale",
        &[Arg::Uint(180)],
    );
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
        &[Arg::Uint(3), Arg::Object(surface), fixed(1.0), fixed(2.0)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    pump(&mut conn, &mut w);
    drain(&mut w);
    use baud_present_wayland::cursor::Drawn;
    use protocol::wp_cursor_shape_device_v1::shape;
    let cursor = shaped_pointer(
        &mut conn,
        &mut server,
        &mut w,
        (CursorShape(shape::TEXT), 3),
        Drawing {
            side: 36,
            at: 180,
            destination: Some((24, 24)),
            scale: 1,
            hotspot: (12, 12),
            image: Drawn::IBeam,
        },
        None,
    );
    assert!(cursor.viewport.is_some());
    server.send(
        pointer,
        "wl_pointer",
        "leave",
        &[Arg::Uint(4), Arg::Object(surface)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    server.send(
        fractional,
        "wp_fractional_scale_v1",
        "preferred_scale",
        &[Arg::Uint(240)],
    );
    server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[Arg::Uint(5), Arg::Object(surface), fixed(1.0), fixed(2.0)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    pump(&mut conn, &mut w);
    drain(&mut w);
    shaped_pointer(
        &mut conn,
        &mut server,
        &mut w,
        (CursorShape(shape::POINTER), 5),
        Drawing {
            side: 48,
            at: 240,
            destination: Some((24, 24)),
            scale: 1,
            hotspot: (9, 2),
            image: Drawn::Hand,
        },
        Some(cursor),
    );
}

/// **A shown pointer follows the window's scale**: the window that moves
/// onto an output at two while the pointer stays over it draws its image
/// again at two, as it draws its frame, without waiting for the pointer to
/// come back or for the next shape. Hidden (`set_cursor` with no surface), or once the
/// pointer has left, it is not shown again by a scale; the next shape is
/// drawn at the scale then. sway 1.7's way: `wl_compositor` 4, the
/// outputs' scales. Perturbation: the image kept as shown after it is
/// hidden -- a scale shows the hidden pointer, and the probe after it
/// reads the redraw's requests where the title should be.
#[test]
fn a_shown_pointer_follows_the_windows_scale() {
    let Pointed {
        mut conn,
        mut server,
        mut w,
        pointer,
        surface,
        ..
    } = pointer_session_at(7, Shapes::Absent, Scales::Outputs);
    let output = server
        .objects
        .iter()
        .find(|(_, i)| i.name == "wl_output")
        .map(|(id, _)| *id)
        .expect("the output bound");
    server.send(output, "wl_output", "scale", &[Arg::Int(2)]);
    server.send(output, "wl_output", "done", &[]);
    server.send(
        pointer,
        "wl_pointer",
        "enter",
        &[Arg::Uint(3), Arg::Object(surface), fixed(1.0), fixed(2.0)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    pump(&mut conn, &mut w);
    drain(&mut w);
    use baud_present_wayland::cursor::Drawn;
    use protocol::wp_cursor_shape_device_v1::shape;
    let beam = |side, at, scale| Drawing {
        side,
        at,
        destination: None,
        scale,
        hotspot: (12, 12),
        image: Drawn::IBeam,
    };
    let cursor = shaped_pointer(
        &mut conn,
        &mut server,
        &mut w,
        (CursorShape(shape::TEXT), 3),
        beam(24, 120, 1),
        None,
    );
    // Onto the output at two, the pointer still over the window.
    server.send(surface, "wl_surface", "enter", &[Arg::Object(output)]);
    pump(&mut conn, &mut w);
    drain(&mut w);
    let cursor = pointer_drawn(&mut server, (3, beam(48, 240, 2)), Some(cursor));
    // Hidden, then off the output: nothing is drawn. The title is the
    // probe: the next request is it.
    assert_eq!(w.hide_pointer(&mut conn.sender()), Shaped::Set);
    conn.flush().unwrap();
    server.expect("wl_pointer", "set_cursor");
    server.send(surface, "wl_surface", "leave", &[Arg::Object(output)]);
    pump(&mut conn, &mut w);
    drain(&mut w);
    w.set_title(&mut conn.sender(), "probe");
    conn.flush().unwrap();
    server.expect("xdg_toplevel", "set_title");
    // Shown again at the scale in force, one; then the pointer leaves and
    // the window goes back to two: nothing is drawn.
    let cursor = shaped_pointer(
        &mut conn,
        &mut server,
        &mut w,
        (CursorShape(shape::TEXT), 3),
        beam(24, 120, 1),
        Some(cursor),
    );
    server.send(
        pointer,
        "wl_pointer",
        "leave",
        &[Arg::Uint(4), Arg::Object(surface)],
    );
    server.send(pointer, "wl_pointer", "frame", &[]);
    server.send(surface, "wl_surface", "enter", &[Arg::Object(output)]);
    pump(&mut conn, &mut w);
    drain(&mut w);
    w.set_title(&mut conn.sender(), "probe");
    conn.flush().unwrap();
    server.expect("xdg_toplevel", "set_title");
    let _ = cursor;
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

/// **A seat that loses its keyboard lets it go, and asks for the next
/// one**: "When the pointer capability is removed, a client should destroy
/// the wl_pointer objects associated with the seat where the capability was
/// removed, using the wl_pointer.release request. No further pointer events
/// will be received on these objects", and "The above behavior also applies
/// to wl_keyboard" (`[[source: wayland]]`, `wl_seat.capabilities`). So a
/// keyboard unplugged and plugged in again is a new object, asked for when
/// the capability comes back; a focused keyboard's loss is the focus lost,
/// an unfocused one's is nothing, and the keys of the new one type. Found
/// under sway's headless backend, where each wtype run is a keyboard of its
/// own and the second typed nothing.
#[test]
fn a_seat_that_loses_its_keyboard_releases_it_and_asks_for_the_next() {
    use baud_domain::{Focus, KeyEventKind, Modifiers};
    let (mut conn, mut server, mut w, kb, surface) = keyboard_session();
    keyboard_enter(&mut server, kb, 5, surface);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), [Event::Focus(Focus::Gained)], "the control");
    let seat = server
        .objects
        .iter()
        .find(|(_, i)| i.name == "wl_seat")
        .map(|(id, _)| *id)
        .unwrap();
    server.send(seat, "wl_seat", "capabilities", &[Arg::Uint(0)]);
    pump(&mut conn, &mut w);
    let r = server.expect("wl_keyboard", "release");
    assert_eq!(r.sender, kb);
    assert_eq!(drain(&mut w), [Event::Focus(Focus::Lost)]);
    server.send(
        seat,
        "wl_seat",
        "capabilities",
        &[Arg::Uint(protocol::wl_seat::capability::KEYBOARD)],
    );
    pump(&mut conn, &mut w);
    let r = server.expect("wl_seat", "get_keyboard");
    let Val::New(next) = r.args[0] else { panic!() };
    assert_ne!(next, kb, "a new keyboard object");
    send_keymap(&mut server, next);
    keyboard_enter(&mut server, next, 6, surface);
    key(&mut server, next, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    assert_eq!(
        drain(&mut w),
        [
            Event::Focus(Focus::Gained),
            typed('q', Modifiers::NONE, KeyEventKind::Press)
        ]
    );
    // A keyboard lost after its leave takes no focus: the leave did.
    server.send(
        next,
        "wl_keyboard",
        "leave",
        &[Arg::Uint(7), Arg::Object(surface)],
    );
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), [Event::Focus(Focus::Lost)]);
    server.send(seat, "wl_seat", "capabilities", &[Arg::Uint(0)]);
    pump(&mut conn, &mut w);
    let r = server.expect("wl_keyboard", "release");
    assert_eq!(r.sender, next);
    assert_eq!(drain(&mut w), [], "no second focus lost");
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

/// **A selection given up is emptied only while its source still offers
/// it**, as xterm's `DisownSelection` gives up a selection it owns and
/// `XtDisownSelection` does nothing for one it no longer owns: the source
/// a copy set is named by `offering`, and giving it up sets a NULL
/// source; once another copy replaced it -- this client's, or another
/// client's that cancelled it -- giving the old one up sends nothing.
#[test]
fn a_selection_is_given_up_only_while_it_is_offered() {
    let Clipped {
        mut conn,
        mut server,
        mut w,
        keyboard,
        surface,
        primary_device,
        ..
    } = clip_session();
    keyboard_enter(&mut server, keyboard, 6, surface);
    key_at(&mut server, keyboard, 7, KEY_Q, PRESSED);
    pump(&mut conn, &mut w);
    drain(&mut w);
    assert_eq!(w.offering(Board::Primary), None, "nothing offered yet");
    assert_eq!(
        w.copy(&mut conn.sender(), Board::Primary, "hello"),
        Copied::Set
    );
    let first = w.offering(Board::Primary).expect("offered");
    assert_eq!(
        w.give_up(&mut conn.sender(), Board::Primary, first),
        Some(Copied::Set)
    );
    assert_eq!(w.offering(Board::Primary), None, "given up");
    conn.flush().unwrap();
    server.expect("zwp_primary_selection_device_manager_v1", "create_source");
    for _ in 0..5 {
        server.expect("zwp_primary_selection_source_v1", "offer");
    }
    server.expect("zwp_primary_selection_device_v1", "set_selection");
    let r = server.expect("zwp_primary_selection_device_v1", "set_selection");
    assert_eq!(r.sender, primary_device);
    assert!(matches!(
        r.args[..],
        [Val::Obj(ObjectId::NULL), Val::Uint(7)]
    ));
    server.expect("zwp_primary_selection_source_v1", "destroy");
    // Replaced by a later copy of this client's: the old source is not
    // given up, and the new one stays offered.
    w.copy(&mut conn.sender(), Board::Primary, "one");
    let one = w.offering(Board::Primary).expect("offered");
    w.copy(&mut conn.sender(), Board::Primary, "two");
    let two = w.offering(Board::Primary).expect("offered");
    assert_ne!(one, two);
    assert_eq!(w.give_up(&mut conn.sender(), Board::Primary, one), None);
    assert_eq!(
        w.offering(Board::Primary),
        Some(two),
        "the later copy stays"
    );
    // Cancelled by another client's copy: nothing to give up.
    server.send(two, "zwp_primary_selection_source_v1", "cancelled", &[]);
    pump(&mut conn, &mut w);
    assert_eq!(w.offering(Board::Primary), None, "cancelled");
    assert_eq!(w.give_up(&mut conn.sender(), Board::Primary, two), None);
    // A marker: everything sent before its source is read, and no NULL
    // source is among it.
    w.copy(&mut conn.sender(), Board::Primary, "marker");
    let marker = w.offering(Board::Primary).expect("offered");
    conn.flush().unwrap();
    let mut nulls = 0;
    loop {
        let r = server.next();
        if r.name == "set_selection" && matches!(r.args[..], [Val::Obj(ObjectId::NULL), _]) {
            nulls += 1;
        }
        if r.name == "create_source" && matches!(r.args[..], [Val::New(id)] if id == marker) {
            break;
        }
    }
    assert_eq!(nulls, 0, "no other selection emptied");
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
        [chorded(
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
        [chorded(
            'f',
            Modifiers::NONE.with(Modifiers::CTRL),
            KeyEventKind::Press
        )]
    );
}

const KEY_N: u32 = 49;

/// **Ctrl+Shift+N asks for a new window** (foot's `spawn-terminal`,
/// kitty's `new_os_window`, gnome-terminal's `new-window`): a shortcut,
/// not a key, repeating and releasing nothing; Ctrl+N alone is the
/// program's key.
#[test]
fn ctrl_shift_n_asks_for_a_new_window() {
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
    key_at(&mut server, kb, 3, KEY_N, PRESSED);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), [Event::Shortcut(Shortcut::NewWindow)]);
    assert_eq!(w.repeat_deadline(), None, "a shortcut does not repeat");
    key_at(&mut server, kb, 4, KEY_N, RELEASED);
    pump(&mut conn, &mut w);
    assert_eq!(drain(&mut w), [], "nor release");
    depressed(&mut server, kb, ctrl);
    key_at(&mut server, kb, 5, KEY_N, PRESSED);
    pump(&mut conn, &mut w);
    assert_eq!(
        drain(&mut w),
        [chorded(
            'n',
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
    let text =
        |s: &str| -> Vec<Event> { s.chars().map(|c| Event::Typed(keys::typed(c))).collect() };
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
            Event::Typed(keys::typed('漢')),
            Event::Typed(keys::typed('字'))
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
        [Event::Typed(keys::typed('a')), Event::Preedit("b".into())]
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
        .filter(|e| !matches!(e, Event::Pointer(..)))
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
        Event::Typed(baud_domain::KeyEvent::press(
            baud_domain::KeyCode::Enter,
            baud_domain::Modifiers::NONE,
        ))
    };
    assert_eq!(
        typed_events(&mut i.w),
        [
            Event::Preedit(Preedit::default()),
            Event::Typed(keys::typed('仮')),
            Event::Typed(keys::typed('名')),
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

/// A configured window and the objects a test of its scale names.
struct Scaled {
    conn: Connection,
    server: Server,
    w: Window,
    surface: ObjectId,
    top: ObjectId,
    xdg: ObjectId,
    /// The surface's viewport and fractional-scale object, if made.
    viewport: Option<ObjectId>,
    fractional: Option<ObjectId>,
    /// The `wl_output`, if one was offered.
    output: Option<ObjectId>,
    /// The registry, which a global's removal is sent on.
    registry: ObjectId,
}

/// **A window over `globals`**, the first of them `wl_compositor`, then
/// `wl_shm` and `xdg_wm_base`, with what it made for its surface checked
/// to its first commit: a viewport and a fractional-scale object, once
/// each, only when both globals were offered. Configured at
/// `width` x `height` with `before` sent ahead of the configure, so a
/// scale the compositor says first is in the first `Configured`.
fn scaled(
    globals: &[(&str, u32)],
    (width, height): (i32, i32),
    before: impl FnOnce(&mut Server, ObjectId, Option<ObjectId>, Option<ObjectId>),
) -> Scaled {
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
    let (compositor, extra) = globals.split_first().expect("wl_compositor first");
    let base = [*compositor, ("wl_shm", 2), ("xdg_wm_base", 6)];
    for (name, (iface, version)) in (1u32..).zip(base.iter().chain(extra)) {
        server.send(
            registry,
            "wl_registry",
            "global",
            &[Arg::Uint(name), Arg::Str(Some(iface)), Arg::Uint(*version)],
        );
    }
    server.send(sync, "wl_callback", "done", &[Arg::Uint(1)]);
    pump(&mut conn, &mut w);
    let (mut surface, mut xdg, mut top) = (None, None, None);
    let (mut viewport, mut fractional, mut output) = (None, None, None);
    let mut binds = Vec::new();
    loop {
        let r = server.next();
        match (r.iface, r.name, &r.args[..]) {
            ("wl_registry", "bind", [_, Val::NewWith(i, v, id)]) => {
                binds.push((i.clone(), *v));
                if i == "wl_output" {
                    output = Some(*id);
                }
            }
            ("wl_compositor", "create_surface", [Val::New(id)]) => surface = Some(*id),
            ("xdg_wm_base", "get_xdg_surface", [Val::New(id), _]) => xdg = Some(*id),
            ("xdg_surface", "get_toplevel", [Val::New(id)]) => top = Some(*id),
            ("wp_viewporter", "get_viewport", [Val::New(id), Val::Obj(on)]) => {
                assert_eq!(Some(*on), surface, "the window's surface");
                assert!(viewport.is_none(), "one viewport a surface");
                viewport = Some(*id);
            }
            (
                "wp_fractional_scale_manager_v1",
                "get_fractional_scale",
                [Val::New(id), Val::Obj(on)],
            ) => {
                assert_eq!(Some(*on), surface, "the window's surface");
                assert!(
                    fractional.is_none(),
                    "one fractional-scale object a surface"
                );
                fractional = Some(*id);
            }
            ("wl_surface", "commit", _) => break,
            _ => {}
        }
    }
    let offered = |n: &str| globals.iter().any(|(g, _)| *g == n);
    for g in ["wp_viewporter", "wp_fractional_scale_manager_v1"] {
        assert_eq!(
            binds.iter().any(|(i, v)| i == g && *v == 1),
            offered(g),
            "{g} bound at version 1 where offered"
        );
    }
    let both = offered("wp_viewporter") && offered("wp_fractional_scale_manager_v1");
    assert_eq!(viewport.is_some(), both, "a viewport only with both");
    assert_eq!(
        fractional.is_some(),
        both,
        "a fraction asked for only with both"
    );
    let (surface, xdg, top) = (surface.unwrap(), xdg.unwrap(), top.unwrap());
    assert_eq!(w.scale(), BufferScale::ONE, "one until the compositor says");
    before(&mut server, surface, fractional, output);
    configure(&mut server, top, xdg, width, height, &[]);
    pump(&mut conn, &mut w);
    server.expect("xdg_surface", "ack_configure");
    Scaled {
        conn,
        server,
        w,
        surface,
        top,
        xdg,
        viewport,
        fractional,
        output,
        registry,
    }
}

/// Whether a commit attached a buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Attached {
    Yes,
    #[default]
    No,
}

/// What one frame's commit told of the scale: the buffers its pool made,
/// by width and height, whether it attached one, the buffer scale and
/// the viewport's destination it set, and its damage.
#[derive(Debug, Default, PartialEq)]
struct ScaleTold {
    buffers: Vec<(i32, i32)>,
    attached: Attached,
    buffer_scale: Option<i32>,
    destination: Option<(i32, i32)>,
    damage: Vec<[i32; 4]>,
}

/// A whole frame of `size` presented, and what its commit told.
fn present_scaled(t: &mut Scaled, (wu, hu): (usize, usize)) -> (Presented, ScaleTold) {
    let frame = vec![0xff20_4060u32; wu * hu];
    let shown =
        t.w.present(&mut t.conn.sender(), &frame, wu, Changed::Whole);
    t.conn.flush().unwrap();
    if shown == Presented::No {
        return (shown, ScaleTold::default());
    }
    let mut told = ScaleTold::default();
    let mut callback = None;
    for (iface, name, args) in commit_requests(&mut t.server) {
        match (iface, name, &args[..]) {
            ("wl_shm_pool", "create_buffer", [_, _, Val::Int(w), Val::Int(h), ..]) => {
                told.buffers.push((*w, *h))
            }
            ("wl_surface", "attach", _) => told.attached = Attached::Yes,
            ("wl_surface", "set_buffer_scale", [Val::Int(n)]) => {
                assert_eq!(
                    told.attached,
                    Attached::Yes,
                    "a buffer scale only with the buffer it applies to"
                );
                told.buffer_scale = Some(*n)
            }
            ("wp_viewport", "set_destination", [Val::Int(w), Val::Int(h)]) => {
                told.destination = Some((*w, *h))
            }
            (
                "wl_surface",
                "damage_buffer",
                [Val::Int(x), Val::Int(y), Val::Int(w), Val::Int(h)],
            ) => told.damage.push([*x, *y, *w, *h]),
            ("wl_surface", "frame", [Val::New(cb)]) => callback = Some(*cb),
            _ => {}
        }
    }
    let cb = callback.expect("a frame callback");
    t.server.send(cb, "wl_callback", "done", &[Arg::Uint(16)]);
    pump(&mut t.conn, &mut t.w);
    (shown, told)
}

/// The window's events, the frame callbacks left out.
fn scaled_events(w: &mut Window) -> Vec<Event> {
    std::iter::from_fn(|| w.next_event())
        .filter(|e| *e != Event::Frame)
        .collect()
}

const V6_FRACTIONAL: [(&str, u32); 3] = [
    ("wl_compositor", 6),
    ("wp_viewporter", 1),
    ("wp_fractional_scale_manager_v1", 1),
];

/// **A fractional scale draws a buffer the surface times the scale,
/// mapped back through the viewport** (`[[source:
/// wp-fractional-scale-v1]]`): "If a surface has a surface-local size of
/// 100 px by 50 px and wishes to submit buffers with a scale of 1.5, then
/// a buffer of 150px by 75 px should be used and the wp_viewport
/// destination rectangle should be 100 px by 50 px", and "The wl_surface
/// buffer scale should remain set to 1". A 640 x 408 window at 1.5 is a
/// 960 x 612 buffer, its destination 640 x 408, no buffer scale sent, the
/// damage in the buffer's pixels; the destination goes once, and the
/// next frame of the same size carries nothing of the scale.
#[test]
fn a_fraction_draws_the_surface_times_the_scale_through_the_viewport() {
    let mut t = scaled(&V6_FRACTIONAL, (640, 408), |server, _, fractional, _| {
        server.send(
            fractional.unwrap(),
            "wp_fractional_scale_v1",
            "preferred_scale",
            &[Arg::Uint(180)],
        );
    });
    assert_eq!(
        scaled_events(&mut t.w),
        [Event::Configured {
            width: Px(640),
            height: Px(408)
        }],
        "one event for the size and the scale"
    );
    assert_eq!(t.w.scale(), BufferScale(180));
    assert_eq!(t.w.size(), Some((Px(640), Px(408))), "the surface's points");
    assert_eq!(t.w.buffer_size(), Some((Px(960), Px(612))));
    let (shown, told) = present_scaled(&mut t, (960, 612));
    assert_eq!(shown, Presented::Yes);
    assert_eq!(
        told,
        ScaleTold {
            buffers: vec![(960, 612), (960, 612)],
            attached: Attached::Yes,
            buffer_scale: None,
            destination: Some((640, 408)),
            damage: vec![[0, 0, 960, 612]],
        }
    );
    let (_, told) = present_scaled(&mut t, (960, 612));
    assert_eq!(
        told,
        ScaleTold {
            attached: Attached::Yes,
            damage: vec![[0, 0, 960, 612]],
            ..ScaleTold::default()
        },
        "the destination is sent once"
    );
    // A frame at the surface's own size is too small for the buffer.
    let (shown, _) = present_scaled(&mut t, (640, 408));
    assert_eq!(shown, Presented::No);
}

/// **An odd surface at 1.5 rounds its buffer halfway away from zero**:
/// "For toplevel surfaces, the size is rounded halfway away from zero"
/// (`[[source: wp-fractional-scale-v1]]`), so 641 x 409 is 961.5 x 613.5,
/// a 962 x 614 buffer.
#[test]
fn an_odd_surface_at_a_fraction_rounds_half_away_from_zero() {
    let mut t = scaled(&V6_FRACTIONAL, (641, 409), |server, _, fractional, _| {
        server.send(
            fractional.unwrap(),
            "wp_fractional_scale_v1",
            "preferred_scale",
            &[Arg::Uint(180)],
        );
    });
    scaled_events(&mut t.w);
    assert_eq!(t.w.buffer_size(), Some((Px(962), Px(614))));
    let (_, told) = present_scaled(&mut t, (962, 614));
    assert_eq!(told.buffers, [(962, 614), (962, 614)]);
    assert_eq!(told.destination, Some((641, 409)));
}

/// **A fraction in force is not overruled by the integer scale**, in
/// either order: sway 1.10 sends `preferred_buffer_scale` 2 beside a
/// fraction of 1.5 (note BE), and the fraction is the one "the compositor
/// suggests that the client should use". Before any fraction the integer
/// scale is drawn, through `set_buffer_scale`; when the fraction comes the
/// next commit attaches the new buffer with the buffer scale back at one
/// and the destination set, together. Two scales in one dispatch are one
/// `Configured`.
#[test]
fn a_fraction_wins_over_the_integer_scale_in_either_order() {
    // The integer scale first, alone: drawn at 2 by its buffer scale.
    let mut t = scaled(&V6_FRACTIONAL, (640, 408), |server, surface, _, _| {
        server.send(
            surface,
            "wl_surface",
            "preferred_buffer_scale",
            &[Arg::Int(2)],
        );
    });
    assert_eq!(scaled_events(&mut t.w).len(), 1);
    assert_eq!(t.w.scale(), BufferScale(240));
    let (_, told) = present_scaled(&mut t, (1280, 816));
    assert_eq!(told.buffers, [(1280, 816), (1280, 816)]);
    assert_eq!((told.buffer_scale, told.destination), (Some(2), None));
    // Then the fraction: one event, the buffer at 1.5, the buffer scale
    // back at one and the destination, with the attach.
    let fractional = t.fractional.unwrap();
    t.server.send(
        fractional,
        "wp_fractional_scale_v1",
        "preferred_scale",
        &[Arg::Uint(180)],
    );
    pump(&mut t.conn, &mut t.w);
    assert_eq!(
        scaled_events(&mut t.w),
        [Event::Configured {
            width: Px(640),
            height: Px(408)
        }]
    );
    assert_eq!(t.w.scale(), BufferScale(180));
    let (_, told) = present_scaled(&mut t, (960, 612));
    assert_eq!(told.buffers, [(960, 612), (960, 612)]);
    assert_eq!(told.attached, Attached::Yes);
    assert_eq!(
        (told.buffer_scale, told.destination),
        (Some(1), Some((640, 408)))
    );
    // The integer scale again, after the fraction: nothing changes.
    t.server.send(
        t.surface,
        "wl_surface",
        "preferred_buffer_scale",
        &[Arg::Int(3)],
    );
    // And the same fraction again: no event either.
    t.server.send(
        fractional,
        "wp_fractional_scale_v1",
        "preferred_scale",
        &[Arg::Uint(180)],
    );
    pump(&mut t.conn, &mut t.w);
    assert_eq!(scaled_events(&mut t.w), []);
    assert_eq!(t.w.scale(), BufferScale(180));

    // Both in one dispatch on a configured window, the integer first:
    // one event, laid out once, at the fraction.
    let mut t = scaled(&V6_FRACTIONAL, (640, 408), |_, _, _, _| {});
    assert_eq!(scaled_events(&mut t.w).len(), 1);
    assert_eq!(t.w.scale(), BufferScale::ONE);
    t.server.send(
        t.surface,
        "wl_surface",
        "preferred_buffer_scale",
        &[Arg::Int(2)],
    );
    t.server.send(
        t.fractional.unwrap(),
        "wp_fractional_scale_v1",
        "preferred_scale",
        &[Arg::Uint(180)],
    );
    pump(&mut t.conn, &mut t.w);
    assert_eq!(
        scaled_events(&mut t.w),
        [Event::Configured {
            width: Px(640),
            height: Px(408)
        }],
        "one event for the two"
    );
    assert_eq!(t.w.scale(), BufferScale(180));

    // Both before the configure, the integer second: one event, the fraction.
    let mut t = scaled(
        &V6_FRACTIONAL,
        (640, 408),
        |server, surface, fractional, _| {
            server.send(
                fractional.unwrap(),
                "wp_fractional_scale_v1",
                "preferred_scale",
                &[Arg::Uint(180)],
            );
            server.send(
                surface,
                "wl_surface",
                "preferred_buffer_scale",
                &[Arg::Int(2)],
            );
        },
    );
    assert_eq!(scaled_events(&mut t.w).len(), 1);
    assert_eq!(t.w.scale(), BufferScale(180));
    assert_eq!(t.w.buffer_size(), Some((Px(960), Px(612))));
}

/// **Without fractional-scale-v1 the integer scale is the buffer scale**:
/// from version 6, `preferred_buffer_scale`, the buffer the surface times
/// it and `set_buffer_scale` sent with that buffer -- "you have to attach
/// a buffer that is larger (by a factor of scale in each dimension) than
/// the desired surface size" (`[[source: wayland]]`); no viewport is made
/// with the viewporter alone. A frame that changed nothing attaches
/// nothing and sends no buffer scale; one back at scale one sends it once.
#[test]
fn the_integer_scale_sets_the_buffer_scale_with_its_buffer() {
    let mut t = scaled(
        &[("wl_compositor", 6), ("wp_viewporter", 1)],
        (640, 408),
        |server, surface, _, _| {
            server.send(
                surface,
                "wl_surface",
                "preferred_buffer_scale",
                &[Arg::Int(2)],
            );
        },
    );
    assert_eq!(t.viewport, None);
    assert_eq!(scaled_events(&mut t.w).len(), 1);
    assert_eq!(t.w.buffer_size(), Some((Px(1280), Px(816))));
    let (_, told) = present_scaled(&mut t, (1280, 816));
    assert_eq!(
        told,
        ScaleTold {
            buffers: vec![(1280, 816), (1280, 816)],
            attached: Attached::Yes,
            buffer_scale: Some(2),
            destination: None,
            damage: vec![[0, 0, 1280, 816]],
        }
    );
    // Nothing changed: the callback alone.
    let frame = vec![0u32; 1280 * 816];
    assert_eq!(
        t.w.present(&mut t.conn.sender(), &frame, 1280, Changed::Strips(&[])),
        Presented::Yes
    );
    t.conn.flush().unwrap();
    let names: Vec<_> = commit_requests(&mut t.server)
        .iter()
        .map(|(i, n, _)| format!("{i}.{n}"))
        .collect();
    assert_eq!(names, ["wl_surface.frame", "wl_surface.commit"]);
    // Back to one.
    t.server.send(
        t.surface,
        "wl_surface",
        "preferred_buffer_scale",
        &[Arg::Int(1)],
    );
    pump(&mut t.conn, &mut t.w);
    assert_eq!(scaled_events(&mut t.w).len(), 1);
    assert_eq!(t.w.buffer_size(), Some((Px(640), Px(408))));
}

/// **Below version 6 the window is drawn at its outputs' scale**:
/// "It is intended that you pick the same buffer scale as the scale of
/// the output that the surface is displayed on" (`set_buffer_scale`,
/// `[[source: wayland]]`), sway 1.7's and weston's way, from
/// `wl_output.scale` and the surface's `enter` and `leave`.
#[test]
fn an_old_compositor_draws_the_window_at_its_outputs_scale() {
    let mut t = scaled(
        &[("wl_compositor", 4), ("wl_output", 4)],
        (640, 408),
        |_, _, _, _| {},
    );
    assert_eq!(scaled_events(&mut t.w).len(), 1);
    assert_eq!(t.w.scale(), BufferScale::ONE);
    let output = t.output.expect("the output bound");
    t.server.send(output, "wl_output", "scale", &[Arg::Int(2)]);
    t.server.send(output, "wl_output", "done", &[]);
    t.server
        .send(t.surface, "wl_surface", "enter", &[Arg::Object(output)]);
    pump(&mut t.conn, &mut t.w);
    assert_eq!(scaled_events(&mut t.w).len(), 1, "one event for the two");
    assert_eq!(t.w.scale(), BufferScale(240));
    let (_, told) = present_scaled(&mut t, (1280, 816));
    assert_eq!(told.buffers, [(1280, 816), (1280, 816)]);
    assert_eq!(told.buffer_scale, Some(2));
    t.server
        .send(t.surface, "wl_surface", "leave", &[Arg::Object(output)]);
    pump(&mut t.conn, &mut t.w);
    assert_eq!(scaled_events(&mut t.w).len(), 1);
    let (_, told) = present_scaled(&mut t, (640, 408));
    assert_eq!(told.buffers, [(640, 408), (640, 408)]);
    assert_eq!(told.buffer_scale, Some(1));
}

/// **The outputs are known by their current mode**, in pixels, with
/// their scale: `wl_output.mode` "is sent again if an output changes mode,
/// for the mode that is now current. In other words, the current mode is
/// always the last mode that was received with the current flag set", and
/// a mode without that flag (0x1) is another the output could take
/// (`[[source: wayland]]`). An output is listed once its current mode has
/// come, and goes with its global.
#[test]
fn the_outputs_are_their_current_modes_and_go_with_their_globals() {
    let mut t = scaled(
        &[("wl_compositor", 4), ("wl_output", 4)],
        (640, 408),
        |_, _, _, _| {},
    );
    assert_eq!(t.w.outputs().count(), 0, "no mode said yet");
    let output = t.output.expect("the output bound");
    let mode =
        |w: i32, h: i32, flags: u32| [Arg::Uint(flags), Arg::Int(w), Arg::Int(h), Arg::Int(60_000)];
    t.server
        .send(output, "wl_output", "mode", &mode(3840, 2160, 0x2));
    t.server.send(output, "wl_output", "done", &[]);
    pump(&mut t.conn, &mut t.w);
    assert_eq!(
        t.w.outputs().count(),
        0,
        "a mode not current is not the output's"
    );
    t.server
        .send(output, "wl_output", "mode", &mode(2560, 1440, 0x1 | 0x2));
    t.server.send(output, "wl_output", "scale", &[Arg::Int(2)]);
    t.server
        .send(output, "wl_output", "mode", &mode(1280, 720, 0x2));
    t.server.send(output, "wl_output", "done", &[]);
    pump(&mut t.conn, &mut t.w);
    let px = |w: u32, h: u32| OutputPx {
        width: Px(w),
        height: Px(h),
        scale: 2,
    };
    assert_eq!(t.w.outputs().collect::<Vec<_>>(), [px(2560, 1440)]);
    t.server
        .send(output, "wl_output", "mode", &mode(1920, 1080, 0x1));
    t.server.send(output, "wl_output", "done", &[]);
    pump(&mut t.conn, &mut t.w);
    assert_eq!(
        t.w.outputs().collect::<Vec<_>>(),
        [px(1920, 1080)],
        "the last current"
    );
    // The output is the fourth global offered, after wl_compositor,
    // wl_shm and xdg_wm_base.
    t.server
        .send(t.registry, "wl_registry", "global_remove", &[Arg::Uint(4)]);
    pump(&mut t.conn, &mut t.w);
    assert_eq!(t.w.outputs().count(), 0, "the output unplugged");
}

/// **An output turned a quarter is listed as the window sees it**: a mode
/// is "in physical hardware units of the output device", before
/// `wl_output.transform`, so under 90, 270, flipped_90 and flipped_270
/// (1, 3, 5, 7) its width and height are the window's height and width,
/// and under the others they are as the mode says (`[[source:
/// wayland]]`). The transform comes in `geometry`, before or after the
/// mode, and a new one turns the mode already said.
#[test]
fn an_output_turned_a_quarter_is_listed_with_its_sides_swapped() {
    let mut t = scaled(
        &[("wl_compositor", 4), ("wl_output", 4)],
        (640, 408),
        |_, _, _, _| {},
    );
    let output = t.output.expect("the output bound");
    let geometry = |transform: i32| {
        [
            Arg::Int(0),
            Arg::Int(0),
            Arg::Int(600),
            Arg::Int(340),
            Arg::Int(0),
            Arg::Str(Some("make")),
            Arg::Str(Some("model")),
            Arg::Int(transform),
        ]
    };
    let listed = |t: &mut Scaled| {
        pump(&mut t.conn, &mut t.w);
        t.w.outputs()
            .map(|o| (o.width, o.height))
            .collect::<Vec<_>>()
    };
    t.server.send(output, "wl_output", "geometry", &geometry(1));
    t.server.send(
        output,
        "wl_output",
        "mode",
        &[
            Arg::Uint(0x1),
            Arg::Int(3840),
            Arg::Int(2160),
            Arg::Int(60_000),
        ],
    );
    t.server.send(output, "wl_output", "done", &[]);
    assert_eq!(listed(&mut t), [(Px(2160), Px(3840))], "90");
    for (transform, turned) in [
        (0, false),
        (2, false),
        (3, true),
        (4, false),
        (5, true),
        (6, false),
        (7, true),
    ] {
        t.server
            .send(output, "wl_output", "geometry", &geometry(transform));
        t.server.send(output, "wl_output", "done", &[]);
        let want = match turned {
            true => (Px(2160), Px(3840)),
            false => (Px(3840), Px(2160)),
        };
        assert_eq!(listed(&mut t), [want], "transform {transform}");
    }
}

/// **The output's physical dots to the inch** (`[font] dpi = "physical"`):
/// its current mode's width over its `geometry` width in millimetres, for
/// the output the surface is on -- none before it has entered one, none
/// where the size is "0 if not known", and the same under a quarter turn,
/// both being in the output's own orientation. Perturbation: divide by the
/// height's millimetres -- the 700 x 400 mm output reads 243 dpi, not 139.
#[test]
fn the_physical_dpi_is_the_entered_outputs_mode_over_its_millimetres() {
    let mut t = scaled(
        &[("wl_compositor", 4), ("wl_output", 4)],
        (640, 408),
        |_, _, _, _| {},
    );
    let output = t.output.expect("the output bound");
    let geometry = |w: i32, h: i32, transform: i32| {
        [
            Arg::Int(0),
            Arg::Int(0),
            Arg::Int(w),
            Arg::Int(h),
            Arg::Int(0),
            Arg::Str(Some("make")),
            Arg::Str(Some("model")),
            Arg::Int(transform),
        ]
    };
    let close = |dpi: Option<f64>| dpi.is_some_and(|d| (d - 3840.0 * 25.4 / 700.0).abs() < 1e-9);
    t.server
        .send(output, "wl_output", "geometry", &geometry(700, 400, 0));
    t.server.send(
        output,
        "wl_output",
        "mode",
        &[
            Arg::Uint(0x1),
            Arg::Int(3840),
            Arg::Int(2160),
            Arg::Int(60_000),
        ],
    );
    t.server.send(output, "wl_output", "done", &[]);
    pump(&mut t.conn, &mut t.w);
    assert_eq!(t.w.physical_dpi(), None, "not on the output yet");
    t.server
        .send(t.surface, "wl_surface", "enter", &[Arg::Object(output)]);
    pump(&mut t.conn, &mut t.w);
    assert!(close(t.w.physical_dpi()), "{:?}", t.w.physical_dpi());
    t.server
        .send(output, "wl_output", "geometry", &geometry(700, 400, 1));
    t.server.send(output, "wl_output", "done", &[]);
    pump(&mut t.conn, &mut t.w);
    assert!(close(t.w.physical_dpi()), "a turn changes nothing");
    t.server
        .send(output, "wl_output", "geometry", &geometry(0, 0, 0));
    t.server.send(output, "wl_output", "done", &[]);
    pump(&mut t.conn, &mut t.w);
    assert_eq!(t.w.physical_dpi(), None, "0 if not known");
}

/// **The outputs are learnt in a second round trip, and only where there
/// are outputs**: an output's `mode`, `scale` and `done` come only after
/// it is bound, and it is bound while the registry's burst is read, so the
/// window asks for a second `sync` after its binds and the outputs are
/// settled when that is answered. With no `wl_output` offered nothing
/// more is asked, and they are settled when the registry is.
#[test]
fn the_outputs_settle_a_round_trip_after_the_registry_and_at_once_without_one() {
    for offered in [true, false] {
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
        let mut globals = vec![("wl_compositor", 6u32), ("wl_shm", 2), ("xdg_wm_base", 6)];
        if offered {
            globals.push(("wl_output", 4));
        }
        for (name, (iface, version)) in (1u32..).zip(&globals) {
            server.send(
                registry,
                "wl_registry",
                "global",
                &[Arg::Uint(name), Arg::Str(Some(iface)), Arg::Uint(*version)],
            );
        }
        assert!(!w.outputs_settled(), "nothing read yet");
        server.send(sync, "wl_callback", "done", &[Arg::Uint(1)]);
        pump(&mut conn, &mut w);
        let (mut output, mut second) = (None, None);
        loop {
            let r = server.next();
            match (r.iface, r.name, &r.args[..]) {
                ("wl_registry", "bind", [_, Val::NewWith(i, _, id)]) if i == "wl_output" => {
                    output = Some(*id)
                }
                ("wl_display", "sync", [Val::New(id)]) => {
                    assert!(second.is_none(), "one second sync");
                    second = Some(*id);
                }
                ("wl_surface", "commit", _) => break,
                _ => {}
            }
        }
        if !offered {
            assert_eq!(second, None, "no output, no second round trip");
            assert!(w.outputs_settled(), "settled with the registry");
            assert_eq!(w.outputs().count(), 0);
            continue;
        }
        assert!(!w.outputs_settled(), "the output has not said");
        let output = output.expect("the output bound");
        let second = second.expect("a sync after the binds");
        server.send(
            output,
            "wl_output",
            "mode",
            &[
                Arg::Uint(0x1),
                Arg::Int(2560),
                Arg::Int(1440),
                Arg::Int(60_000),
            ],
        );
        server.send(output, "wl_output", "scale", &[Arg::Int(2)]);
        server.send(output, "wl_output", "done", &[]);
        server.send(second, "wl_callback", "done", &[Arg::Uint(2)]);
        pump(&mut conn, &mut w);
        assert!(w.outputs_settled(), "settled by the second sync");
        assert_eq!(
            w.outputs().collect::<Vec<_>>(),
            [OutputPx {
                width: Px(2560),
                height: Px(1440),
                scale: 2,
            }]
        );
    }
}

/// **A new destination goes with a buffer even when the buffer's size is
/// the same**: below one -- 60 is a half -- 640 and 639 points are both
/// 320 pixels, 319.5 rounded away from zero, so a resize to 639 makes no
/// new pool, and a frame that changed nothing still attaches the buffer
/// whole with the destination 639 x 408, or the surface would stay 640
/// points wide. An integer 2 and a fraction of 2 make the same buffer,
/// and the move from one to the other goes with a buffer too: the buffer
/// scale back at one and the destination set.
#[test]
fn a_new_destination_goes_with_a_buffer_of_the_same_size() {
    let mut t = scaled(&V6_FRACTIONAL, (640, 408), |server, _, fractional, _| {
        server.send(
            fractional.unwrap(),
            "wp_fractional_scale_v1",
            "preferred_scale",
            &[Arg::Uint(60)],
        );
    });
    scaled_events(&mut t.w);
    assert_eq!(t.w.buffer_size(), Some((Px(320), Px(204))));
    let (_, told) = present_scaled(&mut t, (320, 204));
    assert_eq!(told.destination, Some((640, 408)));
    assert_eq!(t.w.resize(Px(639), Px(408)), Resize::Taken);
    scaled_events(&mut t.w);
    assert_eq!(t.w.buffer_size(), Some((Px(320), Px(204))));
    let frame = vec![0u32; 320 * 204];
    assert_eq!(
        t.w.present(&mut t.conn.sender(), &frame, 320, Changed::Strips(&[])),
        Presented::Yes
    );
    t.conn.flush().unwrap();
    let requests = commit_requests(&mut t.server);
    let names: Vec<_> = requests
        .iter()
        .map(|(i, n, _)| format!("{i}.{n}"))
        .collect();
    assert!(
        names.contains(&"wl_surface.attach".to_string()),
        "{names:?}"
    );
    assert!(
        requests
            .iter()
            .any(|(i, n, a)| (*i, *n) == ("wp_viewport", "set_destination")
                && matches!(a[..], [Val::Int(639), Val::Int(408)])),
        "{names:?}"
    );

    // An integer 2, then a fraction of 2: the same 1280 x 816 buffer.
    let mut t = scaled(&V6_FRACTIONAL, (640, 408), |server, surface, _, _| {
        server.send(
            surface,
            "wl_surface",
            "preferred_buffer_scale",
            &[Arg::Int(2)],
        );
    });
    scaled_events(&mut t.w);
    let (_, told) = present_scaled(&mut t, (1280, 816));
    assert_eq!((told.buffer_scale, told.destination), (Some(2), None));
    t.server.send(
        t.fractional.unwrap(),
        "wp_fractional_scale_v1",
        "preferred_scale",
        &[Arg::Uint(240)],
    );
    pump(&mut t.conn, &mut t.w);
    assert_eq!(t.w.buffer_size(), Some((Px(1280), Px(816))));
    let frame = vec![0u32; 1280 * 816];
    assert_eq!(
        t.w.present(&mut t.conn.sender(), &frame, 1280, Changed::Strips(&[])),
        Presented::Yes
    );
    t.conn.flush().unwrap();
    let requests = commit_requests(&mut t.server);
    let at = |name: &str| requests.iter().position(|(_, n, _)| *n == name);
    assert!(at("attach").is_some(), "the move goes with a buffer");
    assert!(
        requests
            .iter()
            .any(|(_, n, a)| *n == "set_buffer_scale" && matches!(a[..], [Val::Int(1)]))
    );
    assert!(requests.iter().any(
        |(_, n, a)| *n == "set_destination" && matches!(a[..], [Val::Int(640), Val::Int(408)])
    ));
}

/// **A change of size and scale together is one `Configured`**, and the
/// frame after it one buffer of the new size: a resize the client chose,
/// in the surface's points, takes the scale in force.
#[test]
fn a_resize_at_a_fraction_is_the_new_size_times_the_scale() {
    let mut t = scaled(&V6_FRACTIONAL, (640, 408), |server, _, fractional, _| {
        server.send(
            fractional.unwrap(),
            "wp_fractional_scale_v1",
            "preferred_scale",
            &[Arg::Uint(150)],
        );
    });
    scaled_events(&mut t.w);
    assert_eq!(t.w.buffer_size(), Some((Px(800), Px(510))));
    assert_eq!(t.w.resize(Px(320), Px(200)), Resize::Taken);
    assert_eq!(
        scaled_events(&mut t.w),
        [Event::Configured {
            width: Px(320),
            height: Px(200)
        }]
    );
    assert_eq!(t.w.buffer_size(), Some((Px(400), Px(250))));
    let (_, told) = present_scaled(&mut t, (400, 250));
    assert_eq!(told.buffers, [(400, 250), (400, 250)]);
    assert_eq!(told.destination, Some((320, 200)));
    let _ = (t.top, t.xdg);
}

/// The surface's dmabuf feedback object, which the window asked for
/// before its first commit.
fn feedback_object(server: &Server) -> ObjectId {
    *server
        .objects
        .iter()
        .find(|(_, i)| i.name == "zwp_linux_dmabuf_feedback_v1")
        .expect("the surface's feedback")
        .0
}

const TILED: dmabuf::Modifier = dmabuf::Modifier(0x0200_0000_1234_5678);
const ARGB_TILED: dmabuf::Format = dmabuf::Format {
    fourcc: dmabuf::Fourcc::ARGB8888,
    modifier: TILED,
};
const ARGB_LINEAR: dmabuf::Format = dmabuf::Format {
    fourcc: dmabuf::Fourcc::ARGB8888,
    modifier: dmabuf::Modifier::LINEAR,
};

/// A whole burst on `fb`: a table of `ARGB_LINEAR` and `ARGB_TILED`, a
/// main device, and one scan-out tranche of the tiled pair and one of the
/// linear; `done` only where `done` says.
fn send_formats(server: &mut Server, fb: ObjectId, done: bool) {
    let mut table = tempfile();
    for f in [ARGB_LINEAR, ARGB_TILED] {
        table.write_all(&f.fourcc.0.to_ne_bytes()).unwrap();
        table.write_all(&[0; 4]).unwrap();
        table.write_all(&f.modifier.0.to_ne_bytes()).unwrap();
    }
    let iface = "zwp_linux_dmabuf_feedback_v1";
    server.send(
        fb,
        iface,
        "format_table",
        &[Arg::Fd(table.as_fd()), Arg::Uint(32)],
    );
    let dev = 0xe280_u64.to_ne_bytes();
    server.send(fb, iface, "main_device", &[Arg::Array(&dev)]);
    for (flags, index) in [(1u32, 1u16), (0, 0)] {
        server.send(fb, iface, "tranche_target_device", &[Arg::Array(&dev)]);
        server.send(fb, iface, "tranche_flags", &[Arg::Uint(flags)]);
        server.send(
            fb,
            iface,
            "tranche_formats",
            &[Arg::Array(&index.to_ne_bytes())],
        );
        server.send(fb, iface, "tranche_done", &[]);
    }
    if done {
        server.send(fb, iface, "done", &[]);
    }
}

/// **A surface's formats come as its feedback and are read only whole**:
/// the window asks `zwp_linux_dmabuf_v1` at version 4 or later for its
/// surface's feedback before its first commit; a burst without its `done`
/// publishes nothing, and with it, the main device and the tranches in
/// the order sent, each format read from the table by its index. Below
/// version 4 nothing is bound and there are no formats to import with.
#[test]
fn a_surfaces_formats_come_as_its_feedback_and_are_read_whole() {
    let (mut conn, mut server, mut w, bound) = session_with(&[("zwp_linux_dmabuf_v1", 5)]);
    assert_eq!(bound.len(), 1);
    let fb = feedback_object(&server);
    send_formats(&mut server, fb, false);
    pump(&mut conn, &mut w);
    assert!(w.buffer_formats().is_none(), "no done, no formats");
    assert_eq!(w.formats_sent(), dmabuf::Count(0));
    server.send(fb, "zwp_linux_dmabuf_feedback_v1", "done", &[]);
    pump(&mut conn, &mut w);
    let f = w.buffer_formats().expect("a whole burst");
    assert_eq!(f.main, Some(dmabuf::Device(0xe280)));
    assert_eq!(f.tranches.len(), 2);
    assert!(f.tranches[0].flags.has(dmabuf::TrancheFlags::SCANOUT));
    assert_eq!(f.tranches[0].formats, vec![ARGB_TILED]);
    assert_eq!(f.tranches[1].formats, vec![ARGB_LINEAR]);
    assert_eq!(f.unknown_indices, dmabuf::Count(0));
    assert_eq!(w.formats_sent(), dmabuf::Count(1));

    let (mut conn, server, mut w, bound) = session_with(&[("zwp_linux_dmabuf_v1", 3)]);
    assert!(bound.is_empty(), "version 3 has no feedback");
    assert!(
        !server
            .objects
            .values()
            .any(|i| i.name == "zwp_linux_dmabuf_feedback_v1")
    );
    let plane = tempfile();
    let planes = [dmabuf::Plane {
        fd: plane.as_fd(),
        offset: dmabuf::Bytes(0),
        stride: dmabuf::Bytes(256),
    }];
    let d = dmabuf::Dmabuf {
        width: Px(64),
        height: Px(32),
        format: ARGB_LINEAR,
        planes: &planes,
    };
    assert_eq!(
        w.import(&mut conn.sender(), &d),
        dmabuf::Imported::NoFormats
    );
}

/// The requests up to and with the next `wl_surface.commit`, by name
/// with their arguments.
fn until_commit(server: &mut Server) -> Vec<(String, Vec<Val>)> {
    commit_requests(server)
        .into_iter()
        .map(|(i, n, a)| (format!("{i}.{n}"), a))
        .collect()
}

fn frame_done(server: &mut Server, requests: &[(String, Vec<Val>)]) {
    let cb = requests
        .iter()
        .find_map(|(n, a)| match (n.as_str(), &a[..]) {
            ("wl_surface.frame", [Val::New(id)]) => Some(*id),
            _ => None,
        })
        .expect("a frame callback");
    server.send(cb, "wl_callback", "done", &[Arg::Uint(1)]);
}

/// **A dmabuf is made a buffer of its planes and shown with its points**:
/// an offered pair is imported through `create_params`, an `add` with the
/// plane's fd, index, offset, stride and modifier, `create_immed` and the
/// params' `destroy`; a pair not offered sends nothing. A frame with
/// points makes the surface's syncobj object once and sets both points,
/// split high word first, before the attach, the damage and the commit;
/// conflicting points and a buffer not of the window's size send nothing;
/// a frame without points, and a shm frame after it, destroy the object
/// before they attach.
#[test]
fn a_dmabuf_is_imported_and_shown_with_its_points() {
    let (mut conn, mut server, mut w, bound) = session_with(&[
        ("zwp_linux_dmabuf_v1", 6),
        ("wp_linux_drm_syncobj_manager_v1", 1),
    ]);
    let (manager, syncobj) = (bound[0], bound[1]);
    let fb = feedback_object(&server);
    send_formats(&mut server, fb, true);
    pump(&mut conn, &mut w);

    let plane = tempfile();
    let planes = [dmabuf::Plane {
        fd: plane.as_fd(),
        offset: dmabuf::Bytes(64),
        stride: dmabuf::Bytes(256),
    }];
    let size = w.buffer_size().unwrap();
    let mut d = dmabuf::Dmabuf {
        width: size.0,
        height: size.1,
        format: ARGB_TILED,
        planes: &planes,
    };
    let dmabuf::Imported::Buffer(buffer) = w.import(&mut conn.sender(), &d) else {
        panic!("an offered pair is imported")
    };
    conn.flush().unwrap();
    let r = server.expect("zwp_linux_dmabuf_v1", "create_params");
    assert_eq!(r.sender, manager);
    let Val::New(params) = r.args[0] else {
        panic!()
    };
    let r = server.expect("zwp_linux_buffer_params_v1", "add");
    assert_eq!(r.sender, params);
    assert!(matches!(
        r.args[..],
        [
            Val::Fd,
            Val::Uint(0),
            Val::Uint(64),
            Val::Uint(256),
            Val::Uint(0x0200_0000),
            Val::Uint(0x1234_5678)
        ]
    ));
    let r = server.expect("zwp_linux_buffer_params_v1", "create_immed");
    assert!(matches!(
        r.args[..],
        [Val::New(b), Val::Int(64), Val::Int(32), Val::Uint(f), Val::Uint(0)]
            if b == buffer && f == dmabuf::Fourcc::ARGB8888.0
    ));
    server.expect("zwp_linux_buffer_params_v1", "destroy");
    d.format = dmabuf::Format {
        fourcc: dmabuf::Fourcc::XRGB8888,
        modifier: dmabuf::Modifier::LINEAR,
    };
    assert_eq!(
        w.import(&mut conn.sender(), &d),
        dmabuf::Imported::NotOffered
    );

    let syncfd = tempfile();
    let t = w
        .import_timeline(&mut conn.sender(), syncfd.as_fd())
        .expect("the manager is bound");
    conn.flush().unwrap();
    let r = server.expect("wp_linux_drm_syncobj_manager_v1", "import_timeline");
    assert_eq!(r.sender, syncobj);
    assert!(matches!(r.args[..], [Val::New(id), Val::Fd] if id == t.0));

    let points = |a, b| dmabuf::Points {
        acquire: (t, dmabuf::Point(a)),
        release: (t, dmabuf::Point(b)),
    };
    let whole = Changed::Whole;
    let s = &mut conn.sender();
    assert_eq!(
        w.present_dmabuf(s, buffer, (Px(1), Px(1)), Some(points(1, 2)), whole),
        DmabufShown::WrongSize
    );
    assert_eq!(
        w.present_dmabuf(s, buffer, size, Some(points(2, 2)), whole),
        DmabufShown::ConflictingPoints
    );
    let far = (5 << 32) + 1;
    assert_eq!(
        w.present_dmabuf(s, buffer, size, Some(points(far, far + 1)), whole),
        DmabufShown::Yes
    );
    conn.flush().unwrap();
    let first = until_commit(&mut server);
    let names: Vec<&str> = first.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "wp_linux_drm_syncobj_manager_v1.get_surface",
            "wp_linux_drm_syncobj_surface_v1.set_acquire_point",
            "wp_linux_drm_syncobj_surface_v1.set_release_point",
            "wl_surface.attach",
            "wl_surface.damage_buffer",
            "wl_surface.frame",
            "wl_surface.commit",
        ],
        "the refusals sent nothing"
    );
    assert!(matches!(
        first[1].1[..],
        [Val::Obj(o), Val::Uint(5), Val::Uint(1)] if o == t.0
    ));
    assert!(matches!(
        first[2].1[..],
        [Val::Obj(o), Val::Uint(5), Val::Uint(2)] if o == t.0
    ));
    assert!(matches!(first[3].1[..], [Val::Obj(b), ..] if b == buffer));
    assert!(matches!(
        first[4].1[..],
        [Val::Int(0), Val::Int(0), Val::Int(64), Val::Int(32)]
    ));
    assert_eq!(
        w.present_dmabuf(&mut conn.sender(), buffer, size, Some(points(3, 4)), whole),
        DmabufShown::No,
        "a frame in flight"
    );

    frame_done(&mut server, &first);
    pump(&mut conn, &mut w);
    let strips = [Strip {
        top: Px(4),
        bottom: Px(6),
    }];
    let s = &mut conn.sender();
    let changed = Changed::Strips(&strips);
    assert_eq!(
        w.present_dmabuf(s, buffer, size, Some(points(3, 4)), changed),
        DmabufShown::Yes
    );
    conn.flush().unwrap();
    let second = until_commit(&mut server);
    assert_eq!(
        second[0].0, "wp_linux_drm_syncobj_surface_v1.set_acquire_point",
        "the object is made once"
    );
    assert!(second.iter().any(|(n, a)| n == "wl_surface.damage_buffer"
        && matches!(a[..], [Val::Int(0), Val::Int(4), Val::Int(64), Val::Int(2)])));

    frame_done(&mut server, &second);
    pump(&mut conn, &mut w);
    assert_eq!(
        w.present_dmabuf(&mut conn.sender(), buffer, size, None, whole),
        DmabufShown::Yes
    );
    conn.flush().unwrap();
    let third = until_commit(&mut server);
    assert_eq!(third[0].0, "wp_linux_drm_syncobj_surface_v1.destroy");
    assert_eq!(third[1].0, "wl_surface.attach");

    // Points again, then a shm frame: the object is made anew and goes
    // before the shm buffer is attached.
    frame_done(&mut server, &third);
    pump(&mut conn, &mut w);
    let s = &mut conn.sender();
    assert_eq!(
        w.present_dmabuf(s, buffer, size, Some(points(5, 6)), whole),
        DmabufShown::Yes
    );
    conn.flush().unwrap();
    let fourth = until_commit(&mut server);
    assert_eq!(fourth[0].0, "wp_linux_drm_syncobj_manager_v1.get_surface");
    frame_done(&mut server, &fourth);
    pump(&mut conn, &mut w);
    let frame = vec![0xff00_00ffu32; 64 * 32];
    assert_eq!(
        w.present(&mut conn.sender(), &frame, 64, Changed::Whole),
        Presented::Yes
    );
    conn.flush().unwrap();
    let fifth = until_commit(&mut server);
    let at = |name: &str| fifth.iter().position(|(n, _)| n == name).unwrap();
    assert!(at("wp_linux_drm_syncobj_surface_v1.destroy") < at("wl_surface.attach"));
    assert!(
        at("wl_shm.create_pool") < at("wl_surface.attach"),
        "a new pool"
    );
}

/// **Without the syncobj manager there is no timeline and no frame with
/// points**, and a frame without them is shown as it is.
#[test]
fn without_the_syncobj_manager_frames_go_without_points() {
    let (mut conn, mut server, mut w, _) = session_with(&[("zwp_linux_dmabuf_v1", 5)]);
    let fb = feedback_object(&server);
    send_formats(&mut server, fb, true);
    pump(&mut conn, &mut w);
    let syncfd = tempfile();
    assert!(
        w.import_timeline(&mut conn.sender(), syncfd.as_fd())
            .is_none()
    );
    let plane = tempfile();
    let planes = [dmabuf::Plane {
        fd: plane.as_fd(),
        offset: dmabuf::Bytes(0),
        stride: dmabuf::Bytes(256),
    }];
    let size = w.buffer_size().unwrap();
    let d = dmabuf::Dmabuf {
        width: size.0,
        height: size.1,
        format: ARGB_LINEAR,
        planes: &planes,
    };
    let dmabuf::Imported::Buffer(buffer) = w.import(&mut conn.sender(), &d) else {
        panic!()
    };
    let t = dmabuf::Timeline(ObjectId(900));
    let points = dmabuf::Points {
        acquire: (t, dmabuf::Point(1)),
        release: (t, dmabuf::Point(2)),
    };
    let s = &mut conn.sender();
    assert_eq!(
        w.present_dmabuf(s, buffer, size, Some(points), Changed::Whole),
        DmabufShown::NoExplicitSync
    );
    assert_eq!(
        w.present_dmabuf(s, buffer, size, None, Changed::Whole),
        DmabufShown::Yes
    );
    conn.flush().unwrap();
    server.expect("zwp_linux_dmabuf_v1", "create_params");
    for name in ["add", "create_immed", "destroy"] {
        server.expect("zwp_linux_buffer_params_v1", name);
    }
    let names: Vec<String> = until_commit(&mut server)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        names,
        [
            "wl_surface.attach",
            "wl_surface.damage_buffer",
            "wl_surface.frame",
            "wl_surface.commit"
        ]
    );
}
