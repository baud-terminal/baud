//! The Linux tools against what the Python they replaced did on the same
//! bytes, on this machine and with no compositor: `linux_tools/goldens.rs`
//! was written from a real run of the Python, so every case there is one
//! perturbation of one branch of one reader, and a case is held to its
//! stdout, its exit code and the bytes of every file it wrote.

#[path = "linux_tools/goldens.rs"]
mod goldens;
#[path = "linux_tools/pointer_goldens.rs"]
mod pointer_goldens;

use std::ffi::OsString;
use std::fs;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use baud_bench::key_latency::sha256;
use baud_bench::linux::{Done, UNFLUSHED, damage, frames, pixels, pointer, trace};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A folder of the test's own, under the target directory.
fn folder() -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let at = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("linux-tools-{}-{n}", std::process::id()));
    fs::create_dir_all(&at).expect("the test's folder");
    at
}

/// Every file under `dir`, relative to it.
fn files_under(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("the folder reads") {
        let entry = entry.expect("an entry");
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            files_under(&entry.path(), &format!("{name}/"), out);
        } else {
            out.push(name);
        }
    }
}

fn run_tool(case: &goldens::Case, args: &[OsString]) -> Done {
    match case.tool {
        "linux-damage" => damage::run(args),
        "linux-frames" => frames::run(args),
        "linux-pixels" => pixels::run(args, Cursor::new(case.stdin)),
        "linux-pointer-trace" => trace::run(Cursor::new(case.stdin)),
        other => panic!("no tool {other}"),
    }
}

/// One case: its files laid out, the tool run, the three things compared.
fn check(case: &goldens::Case, failures: &mut Vec<String>) {
    let dir = folder();
    for sub in case.dirs {
        fs::create_dir_all(dir.join(sub)).expect("a directory for the case");
    }
    for (name, bytes) in case.files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().expect("a file's folder")).expect("the file's folder");
        fs::write(path, bytes).expect("a file for the case");
    }
    let args: Vec<OsString> = case
        .args
        .iter()
        .map(|a| a.replace("{D}", &dir.to_string_lossy()).into())
        .collect();
    let done = run_tool(case, &args);
    let usage = case.code == 2;
    let label = format!("{}: {}", case.tool, case.name);
    if done.code != case.code {
        failures.push(format!(
            "{label}: exit {} against {}: {}",
            done.code, case.code, done.why
        ));
    }
    if !usage && done.stdout != case.stdout {
        failures.push(format!(
            "{label}: stdout\n--- rust\n{}\n--- python\n{}",
            String::from_utf8_lossy(&done.stdout),
            String::from_utf8_lossy(case.stdout)
        ));
    }
    let mut now = Vec::new();
    files_under(&dir, "", &mut now);
    now.retain(|f| !case.files.iter().any(|(given, _)| given == f));
    now.sort();
    let want: Vec<&str> = case.made.iter().map(|m| m.0).collect();
    if now != want {
        failures.push(format!("{label}: wrote {now:?} against {want:?}"));
    } else {
        for (name, hash) in case.made {
            let bytes = fs::read(dir.join(name)).expect("a file the tool wrote");
            let got = sha256::digest(&bytes);
            if got != *hash {
                failures.push(format!("{label}: {name}'s bytes differ"));
            }
        }
    }
    fs::remove_dir_all(dir).expect("the test's folder goes");
}

/// Every case of one tool, reporting every failure at once.
fn check_tool(tool: &str, at_least: usize) {
    let cases: Vec<&goldens::Case> = goldens::CASES.iter().filter(|c| c.tool == tool).collect();
    assert!(cases.len() >= at_least, "{tool}: {} cases", cases.len());
    let mut failures = Vec::new();
    for case in cases {
        check(case, &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn linux_damage_does_what_the_python_did() {
    check_tool("linux-damage", 50);
}

#[test]
fn linux_frames_does_what_the_python_did() {
    check_tool("linux-frames", 70);
}

#[test]
fn linux_pointer_trace_does_what_the_python_did() {
    check_tool("linux-pointer-trace", 45);
}

#[test]
fn linux_pixels_does_what_the_python_did() {
    check_tool("linux-pixels", 260);
}

/// How `Done` ends a process when stdout refuses: no panic, and the code
/// a program that could not flush exits with.
#[test]
fn a_closed_pipe_is_a_code_and_not_a_panic() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut err = Vec::new();
    let code = Done::with("a line\n", 0).finish_to(&mut Closed, &mut err);
    assert_eq!(code, UNFLUSHED);
    let code = Done::with("a line\n", 1).finish_to(&mut Closed, &mut err);
    assert_eq!(code, UNFLUSHED, "it wins over the tool's own code");
    let code = Done::with("", 2).finish_to(&mut Closed, &mut err);
    assert_eq!(code, 2, "nothing to write refuses nothing");
    let mut out = Vec::new();
    assert_eq!(
        Done::failed("so far\n", "why").finish_to(&mut out, &mut err),
        1
    );
    assert_eq!(out, b"so far\n");
    assert_eq!(err, b"why\n");
}

/// `head -1` over a real pipe: the reader has gone before the writer
/// writes.
#[cfg(unix)]
#[test]
fn a_real_closed_pipe_is_a_code_too() {
    let (reader, writer) = UnixStream::pair().expect("a pair");
    drop(reader);
    let mut writer = writer;
    let mut err = Vec::new();
    assert_eq!(
        Done::with("a line\n", 0).finish_to(&mut writer, &mut err),
        UNFLUSHED
    );
}

/// The usage the Python printed, its program's name without `.py`, is every
/// line of the usage here, but for the one that said how it was written.
#[test]
fn the_usage_is_the_python_s_but_for_its_name() {
    let case = goldens::CASES
        .iter()
        .find(|c| c.name == "an unknown command")
        .expect("the case");
    let python = String::from_utf8_lossy(case.stdout);
    assert!(python.contains("linux-pixels.py diff"), "the control fires");
    let ours: Vec<&str> = pixels::USAGE.lines().collect();
    let mut differ = Vec::new();
    for line in python.lines() {
        let renamed = line.replace("linux-pixels.py", "linux-pixels");
        if !ours.contains(&renamed.as_str()) {
            differ.push(line.to_string());
        }
    }
    assert_eq!(
        differ,
        ["Standard library only, so it runs on the Mac and in the container alike."]
    );
    let done = pixels::run(&["nope".into()], Cursor::new(b""));
    assert_eq!(done.code, 2);
    assert_eq!(done.stdout, format!("{}\n", pixels::USAGE).into_bytes());
}

/// A picture large enough to take every path through the deflate: blocks that
/// end full, a window that slides, matches at every length. Python made its
/// PNG, at both scales, from the same recipe.
fn picture() -> Vec<u8> {
    let (w, h) = (400usize, 300usize);
    let mut px = Vec::with_capacity(w * h * 4);
    let mut state = 12345u32;
    for y in 0..h {
        for x in 0..w {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let noise = state.to_be_bytes()[1];
            let v = match (y / 25) % 4 {
                0 => 40,
                1 => (x / 3).to_le_bytes()[0],
                2 => noise,
                _ => noise & 3,
            };
            px.extend_from_slice(&[v, v.wrapping_mul(3), x.to_le_bytes()[0], 255]);
        }
    }
    px
}

/// `zlib.compress(raw, 9)` through the PNG writer, on what `gen` called this
/// recipe. Its goldens are the Python's, its zlib 1.3.1.
#[test]
fn a_large_png_has_the_bytes_zlib_chose_at_level_nine() {
    let dir = folder();
    let px = picture();
    let mut pam =
        b"P7\nWIDTH 400\nHEIGHT 300\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n".to_vec();
    pam.extend_from_slice(&px);
    let input = dir.join("in.pam");
    fs::write(&input, pam).expect("the picture");
    for (scale, want) in goldens::PICTURES {
        let output = dir.join("out.png");
        let done = pixels::run(
            &[
                "png".into(),
                input.clone().into(),
                output.clone().into(),
                scale.to_string().into(),
            ],
            Cursor::new(b""),
        );
        assert_eq!(done.code, 0, "{}", done.why);
        let bytes = fs::read(&output).expect("the PNG");
        assert_eq!(sha256::digest(&bytes), *want, "at scale {scale}");
    }
    fs::remove_dir_all(dir).expect("the test's folder goes");
}

/// A picture that makes a deflate search: its top half two symbols, so hash
/// chains run long, and its bottom a block repeated with a byte changed every
/// 200 or so, so matches run long and tell each limit of level 9's row apart.
fn searching() -> Vec<u8> {
    let (w, h) = (1000usize, 200usize);
    let mut state = 1u32;
    let mut step = || {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
        state >> 16
    };
    let block: Vec<u8> = (0..9000).map(|_| step().to_le_bytes()[0]).collect();
    let mut px = Vec::with_capacity(w * h * 4);
    let mut at = 0;
    for y in 0..h {
        for _ in 0..w {
            for _ in 0..3 {
                let r = step();
                if y < h / 2 {
                    px.push((r & 1).to_le_bytes()[0]);
                } else {
                    let v = block[at % 9000];
                    px.push(if r % 200 == 0 { v.wrapping_add(1) } else { v });
                    at += 1;
                }
            }
            px.push(255);
        }
    }
    px
}

#[test]
fn level_nine_searches_as_far_as_zlib_does() {
    let dir = folder();
    let mut pam =
        b"P7\nWIDTH 1000\nHEIGHT 200\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n".to_vec();
    pam.extend_from_slice(&searching());
    let (input, output) = (dir.join("in.pam"), dir.join("out.png"));
    fs::write(&input, pam).expect("the picture");
    let done = pixels::run(
        &["png".into(), input.into(), output.clone().into()],
        Cursor::new(b""),
    );
    assert_eq!(done.code, 0, "{}", done.why);
    let bytes = fs::read(&output).expect("the PNG");
    assert_eq!(sha256::digest(&bytes), goldens::SEARCHED);
    fs::remove_dir_all(dir).expect("the test's folder goes");
}

/// A fake compositor, the Python generator's twin: the globals on
/// `get_registry`, `done` and `delete_id` on `sync`, an error or silence after
/// the request a case names, every request kept. It returns at the client's
/// hanging up.
fn compositor(
    sock: UnixStream,
    globals: Vec<(String, u32)>,
    error_after: Option<usize>,
    close_after: Option<usize>,
    split: pointer_goldens::Split,
) -> (Vec<u8>, Vec<Vec<u8>>) {
    fn string(out: &mut Vec<u8>, s: &str) {
        let n = s.len() + 1;
        out.extend_from_slice(&u32::try_from(n).expect("a short name").to_ne_bytes());
        out.extend_from_slice(s.as_bytes());
        out.push(0);
        out.resize(out.len() + (4 - n % 4) % 4, 0);
    }
    fn msg(obj: u32, op: u32, body: &[u8]) -> Vec<u8> {
        let size = u32::try_from(8 + body.len()).expect("a short event");
        let mut out = obj.to_ne_bytes().to_vec();
        out.extend_from_slice(&((size << 16) | op).to_ne_bytes());
        out.extend_from_slice(body);
        out
    }
    let tell = |bytes: &[u8]| {
        use pointer_goldens::Split;
        let mut s = &sock;
        let gone = |r: std::io::Result<()>| {
            // A client that has gone is the end of the run, read below; any
            // other refusal would leave the client waiting for this event.
            if let Err(e) = r {
                assert!(
                    matches!(
                        e.kind(),
                        std::io::ErrorKind::BrokenPipe
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::NotConnected
                    ),
                    "the fake compositor's write: {e}"
                );
            }
        };
        if split == Split::Two {
            // Short of a header, then short of a body, then the rest.
            let body = bytes.len() - 2;
            gone(s.write_all(&bytes[..3]));
            std::thread::sleep(std::time::Duration::from_millis(2));
            gone(s.write_all(&bytes[3..body]));
            std::thread::sleep(std::time::Duration::from_millis(2));
            gone(s.write_all(&bytes[body..]));
        } else {
            gone(s.write_all(bytes));
        }
    };
    let (mut wire, mut keymaps) = (Vec::new(), Vec::new());
    let mut inbox: Vec<u8> = Vec::new();
    let (mut seen, mut said) = (0, false);
    let mut buf = [0u8; 65536];
    loop {
        let mut fds = Vec::new();
        let n = match baud_wayland::sys::recv(&sock, &mut buf, &mut fds) {
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::NotConnected
                ) =>
            {
                0
            }
            Err(e) => panic!("the fake compositor's read: {e}, {seen} requests in"),
        };
        for fd in fds {
            let mut file = fs::File::from(fd);
            let mut bytes = Vec::new();
            file.seek(SeekFrom::Start(0)).expect("the keymap seeks");
            file.read_to_end(&mut bytes).expect("the keymap reads");
            keymaps.push(bytes);
        }
        if n == 0 {
            break;
        }
        wire.extend_from_slice(&buf[..n]);
        inbox.extend_from_slice(&buf[..n]);
        while inbox.len() >= 8 {
            let word = |at: usize| {
                u32::from_ne_bytes([inbox[at], inbox[at + 1], inbox[at + 2], inbox[at + 3]])
            };
            let (obj, head) = (word(0), word(4));
            let size = usize::try_from(head >> 16).expect("sixteen bits");
            let op = head & 0xffff;
            if inbox.len() < size {
                break;
            }
            let arg = if size >= 12 { word(8) } else { 0 };
            inbox.drain(..size);
            seen += 1;
            // The compositor stops talking: nothing more for this client, who
            // reads next after a `sync`, and that read sees the socket gone.
            // Closed there and not half closed at the request named: on macOS
            // a half close here left the client blocked in `recv`, 130 runs in
            // 120,000, the cause not found (`MEASUREMENTS.md`).
            if close_after.is_some_and(|k| seen >= k) {
                if obj == 1 && op == 0 {
                    return (wire, keymaps);
                }
                continue;
            }
            if error_after.is_some_and(|k| seen >= k) && !said {
                said = true;
                let mut body = Vec::new();
                body.extend_from_slice(&5u32.to_ne_bytes());
                body.extend_from_slice(&3u32.to_ne_bytes());
                string(&mut body, "broken");
                tell(&msg(1, 0, &body));
            }
            if obj == 1 && op == 1 {
                for (i, (name, version)) in globals.iter().enumerate() {
                    let mut body = Vec::new();
                    let number = u32::try_from(i + 1).expect("few globals");
                    body.extend_from_slice(&number.to_ne_bytes());
                    string(&mut body, name);
                    body.extend_from_slice(&version.to_ne_bytes());
                    tell(&msg(arg, 0, &body));
                }
            } else if obj == 1 && op == 0 {
                tell(&msg(arg, 0, &0u32.to_ne_bytes()));
                tell(&msg(1, 1, &arg.to_ne_bytes()));
            }
        }
    }
    (wire, keymaps)
}

/// Every script: the requests' bytes, the keymaps they passed, what it
/// printed, how it ended and what it said, equal to the Python's on Linux.
#[test]
fn linux_pointer_asks_what_the_python_asked() {
    let mut failures = Vec::new();
    assert!(pointer_goldens::CASES.len() >= 100);
    for case in pointer_goldens::CASES {
        let (client, server) = UnixStream::pair().expect("a pair");
        // A run that stops waits for nobody: it fails, named, instead of
        // hanging the suite. The longest script waits ten seconds of its own.
        let patience = Some(std::time::Duration::from_secs(120));
        client.set_read_timeout(patience).expect("a timeout");
        server.set_read_timeout(patience).expect("a timeout");
        let globals: Vec<(String, u32)> = case
            .globals
            .iter()
            .map(|(n, v)| ((*n).to_string(), *v))
            .collect();
        let (error_after, close_after, split) = (case.error_after, case.close_after, case.split);
        let fake = std::thread::spawn(move || {
            compositor(server, globals, error_after, close_after, split)
        });
        let mut out = Vec::new();
        let ended = pointer::script(
            pointer::Wire::over(client),
            case.keymap,
            case.size,
            &mut Cursor::new(case.script.as_bytes()),
            &mut out,
        );
        let Ok((wire, keymaps)) = fake.join() else {
            failures.push(format!(
                "{}: the fake compositor stopped: {ended:?}",
                case.name
            ));
            continue;
        };
        let (code, said) = match &ended {
            Ok(()) => (0, String::new()),
            Err(pointer::Stop::Unflushed) => (UNFLUSHED, String::new()),
            Err(pointer::Stop::Failed(why)) => (1, why.clone()),
        };
        if code != case.code {
            failures.push(format!(
                "{}: exit {code} against {}: {said}",
                case.name, case.code
            ));
        }
        if !case.said.is_empty() && said != case.said {
            failures.push(format!(
                "{}: said {said:?} against {:?}",
                case.name, case.said
            ));
        }
        if wire != case.wire {
            failures.push(format!(
                "{}: the requests differ\n--- rust\n{wire:?}\n--- python\n{:?}",
                case.name, case.wire
            ));
        }
        if keymaps != case.keymaps {
            failures.push(format!("{}: the keymaps differ", case.name));
        }
        if out != case.stdout {
            failures.push(format!("{}: stdout differs", case.name));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// A stdout that refuses the one line this tool prints as it goes: the code
/// the Python exited with, 120, and no panic, after its 200 tries of 50 ms.
#[test]
fn a_timed_out_waitfor_on_a_closed_stdout_is_a_code() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (client, server) = UnixStream::pair().expect("a pair");
    let globals = [
        ("wl_seat", 7),
        ("zwlr_virtual_pointer_manager_v1", 2),
        ("zwp_virtual_keyboard_manager_v1", 1),
    ]
    .map(|(n, v)| (n.to_string(), v))
    .to_vec();
    let fake = std::thread::spawn(move || {
        compositor(server, globals, None, None, pointer_goldens::Split::Whole)
    });
    let began = std::time::Instant::now();
    let ended = pointer::script(
        pointer::Wire::over(client),
        b"k",
        ("1024", "768"),
        &mut Cursor::new(b"waitfor false\n".as_slice()),
        &mut Closed,
    );
    fake.join().expect("the fake compositor");
    assert_eq!(ended, Err(pointer::Stop::Unflushed));
    assert!(began.elapsed() >= std::time::Duration::from_secs(10));
}

/// How a run starts: its arguments and its environment. A socket's path has
/// a short limit, so these folders are under `/tmp` and not the target's.
#[test]
fn linux_pointer_starts_as_the_python_started() {
    let mut failures = Vec::new();
    for (name, args, env, listen, keymap, code, requests) in pointer_goldens::STARTS {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(format!("/tmp/baud-lp-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("a folder");
        let fill = |s: &str| s.replace("{D}", &dir.to_string_lossy());
        if *keymap {
            fs::write(dir.join("km"), b"k").expect("a keymap");
        }
        let fake = (!listen.is_empty()).then(|| {
            let listener = UnixListener::bind(dir.join(listen)).expect("a socket");
            std::thread::spawn(move || {
                let (sock, _) = listener.accept().expect("the client");
                let globals = [
                    ("wl_seat", 7),
                    ("zwlr_virtual_pointer_manager_v1", 2),
                    ("zwp_virtual_keyboard_manager_v1", 1),
                ]
                .map(|(n, v)| (n.to_string(), v))
                .to_vec();
                compositor(sock, globals, None, None, pointer_goldens::Split::Whole)
            })
        });
        let argv: Vec<OsString> = args.iter().map(|a| fill(a).into()).collect();
        let vars: Vec<(String, String)> = env
            .iter()
            .map(|(k, v)| ((*k).to_string(), fill(v)))
            .collect();
        let lookup = |name: &str| {
            vars.iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| OsString::from(v))
        };
        let mut out = Vec::new();
        let ended = pointer::start(
            &argv,
            &lookup,
            &mut Cursor::new(b"move 1 1\nclick left\n".as_slice()),
            &mut out,
        );
        let got = i32::from(ended.is_err());
        if got != *code {
            failures.push(format!("{name}: exit {got} against {code}: {ended:?}"));
        }
        if let Some(fake) = fake {
            // A client that never connected leaves the fake in `accept`.
            drop(UnixStream::connect(dir.join(listen)));
            let (wire, _) = fake.join().expect("the fake compositor");
            if sha256::digest(&wire) != *requests {
                failures.push(format!("{name}: the requests differ"));
            }
        }
        fs::remove_dir_all(&dir).expect("the folder goes");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The opcodes the tool spells are the pinned XMLs' order: a request's opcode
/// is its place among the interface's `<request>`s.
#[test]
fn the_opcodes_are_the_pinned_protocols() {
    let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/research/sources");
    let requests = |file: &str, iface: &str| -> Vec<String> {
        let xml = fs::read_to_string(sources.join(file)).expect("the pinned XML");
        let from = xml
            .find(&format!("<interface name=\"{iface}\""))
            .expect("the interface");
        let body = &xml[from..];
        let end = body.find("</interface>").expect("its end");
        body[..end]
            .split("<request name=\"")
            .skip(1)
            .map(|r| r.split('"').next().expect("a name").to_string())
            .collect()
    };
    let pointer = requests(
        "wlr-virtual-pointer-unstable-v1.xml",
        "zwlr_virtual_pointer_v1",
    );
    for (op, name) in [
        (1, "motion_absolute"),
        (2, "button"),
        (3, "axis"),
        (4, "frame"),
        (5, "axis_source"),
        (7, "axis_discrete"),
        (8, "destroy"),
    ] {
        assert_eq!(pointer[op], name);
    }
    let keyboard = requests(
        "virtual-keyboard-unstable-v1.xml",
        "zwp_virtual_keyboard_v1",
    );
    for (op, name) in [(0, "keymap"), (1, "key"), (2, "modifiers"), (3, "destroy")] {
        assert_eq!(keyboard[op], name);
    }
    let manager = requests(
        "wlr-virtual-pointer-unstable-v1.xml",
        "zwlr_virtual_pointer_manager_v1",
    );
    assert_eq!(manager[0], "create_virtual_pointer");
    let manager = requests(
        "virtual-keyboard-unstable-v1.xml",
        "zwp_virtual_keyboard_manager_v1",
    );
    assert_eq!(manager[0], "create_virtual_keyboard");
}
