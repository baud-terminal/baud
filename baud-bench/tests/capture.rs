//! The capture tools as processes, each branch taken over a stand-in: `bash
//! --noprofile --norc` in a scratch home, with `HISTFILE` at `/dev/null`, and
//! nothing that opens a window. A real program's run is in `MEASUREMENTS.md`,
//! "The capture tools are examples". A test's child ends itself, so the tool
//! ends on the pty's EOF rather than on a clock, but for the tests of the clock
//! itself, whose child the tool kills.

use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use baud_bench::capture::pty::{Child, Environ, Lookup, Spawn};
use baud_bench::capture::{kitten, kitten::Direct, resize, shell_start, tui, vim_unicode};
use baud_bench::capture::{marks, yazi_files, yazi_walk};

mod exec;

const TOOL: &str = "CAPTURE_TEST_TOOL";
const ARGS: &str = "CAPTURE_TEST_ARGS";
const LIBTEST_HEADER: &str = "\nrunning 1 test\n";
const LEAVES: &str = "CAPTURE_TEST_LEAVES";

/// Not a check: this test binary run as a `SHELL` that hangs its terminal
/// up and goes on running, the session's leader by `exec`. It ignores the
/// hangup's `SIGHUP`, writes its pid to `$HOME/leaves.pid`, hangs the
/// terminal up and sleeps 20 s. Unset, it does nothing.
#[test]
fn as_a_shell_that_leaves_its_terminal() {
    if std::env::var_os(LEAVES).is_none() {
        return;
    }
    let home = std::env::var_os("HOME").expect("the tool gives the shell a HOME");
    fs::write(
        Path::new(&home).join("leaves.pid"),
        std::process::id().to_string(),
    )
    .expect("the pid");
    // SAFETY: `SIG_IGN` for a signal this process may ignore.
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
    hang_up_the_terminal();
    std::thread::sleep(Duration::from_secs(20));
}

/// `revoke` of the terminal's path hangs it up, every descriptor of it
/// with it, the tool's slave too; Darwin refuses `TIOCNOTTY` of a session's
/// leader.
#[cfg(target_os = "macos")]
fn hang_up_the_terminal() {
    unsafe extern "C" {
        /// `int revoke(const char *path)`, from `<unistd.h>`.
        fn revoke(path: *const libc::c_char) -> libc::c_int;
    }
    // SAFETY: `ttyname` answers a NUL-terminated path or null, and the path
    // is read by `revoke` before anything else could reuse its buffer.
    let got = unsafe {
        let path = libc::ttyname(0);
        assert!(
            !path.is_null(),
            "ttyname: {}",
            std::io::Error::last_os_error()
        );
        revoke(path)
    };
    assert_eq!(got, 0, "revoke: {}", std::io::Error::last_os_error());
}

/// Linux's `TIOCNOTTY` from a session's leader gives the terminal up, and
/// the descriptors the shell holds stay on it.
#[cfg(not(target_os = "macos"))]
fn hang_up_the_terminal() {
    // SAFETY: `TIOCNOTTY` takes no argument.
    let got = unsafe { libc::ioctl(0, libc::TIOCNOTTY) };
    assert_eq!(got, 0, "TIOCNOTTY: {}", std::io::Error::last_os_error());
}

/// Not a check: this test binary run as a tool, because the tools are
/// examples and a test can name no example's path. Unset, it does nothing.
#[test]
fn as_the_tool() {
    let Ok(tool) = std::env::var(TOOL) else {
        return;
    };
    let joined = std::env::var_os(ARGS).unwrap_or_default();
    let args: Vec<std::ffi::OsString> = if joined.is_empty() {
        Vec::new()
    } else {
        joined
            .as_bytes()
            .split(|b| *b == 0x1f)
            .map(|w| std::ffi::OsStr::from_bytes(w).to_os_string())
            .collect()
    };
    let code = match tool.as_str() {
        "capture-tui" => tui::run(&args),
        "capture-resize" => resize::run(&args),
        "capture-yazi-walk" => yazi_walk::run(&args),
        "record-kitten-icat" => kitten::run(&args),
        "record-shell-start" => shell_start::run(&args),
        "gen-marks-payload" => marks::run(&args),
        "gen-vim-unicode" => vim_unicode::run(&args),
        "gen-yazi-files" => yazi_files::run(&args),
        "winsize" => print_winsize(),
        "sigxfsz" => print_sigxfsz(),
        "late-reader" => late_reader(&args),
        "exit-kept" => exit_kept(&args),
        "bound" => bound(&args),
        "beside" => beside(&args),
        "ttys" => print_ttys(),
        "closes" => closes_the_terminal(),
        other => panic!("no tool {other}"),
    };
    std::process::exit(code);
}

/// What stdin's pty says its size is, for a stand-in to report.
fn print_winsize() -> i32 {
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: a live winsize for the call.
    let got = unsafe { libc::ioctl(0, libc::TIOCGWINSZ, &raw mut ws) };
    assert_eq!(got, 0, "stdin is a pty");
    println!(
        "{} {} {} {}",
        ws.ws_col, ws.ws_row, ws.ws_xpixel, ws.ws_ypixel
    );
    0
}

/// Whether `SIGXFSZ` is ignored, for a stand-in to report.
fn print_sigxfsz() -> i32 {
    // SAFETY: an action zeroed and then filled in by the call, which sets none.
    let ignored = unsafe {
        let mut now: libc::sigaction = std::mem::zeroed();
        assert_eq!(
            libc::sigaction(libc::SIGXFSZ, std::ptr::null(), &raw mut now),
            0
        );
        now.sa_sigaction == libc::SIG_IGN
    };
    println!("SIGXFSZ ignored: {ignored}");
    0
}

/// `argv` on a pty of its own.
fn on_a_pty(argv: &[std::ffi::OsString], env: &Environ) -> Child {
    let spawn = Spawn::new(Lookup::Path, argv, None, env, 127).expect("a command");
    Child::fork(&spawn).expect("a child on a pty")
}

/// All the master gives, by the path every tool reads it.
fn read_to_eof(child: &Child) -> Vec<u8> {
    let mut all = Vec::new();
    while child.readable(60.0).expect("select") {
        match child.read(65536) {
            Ok(b) if !b.is_empty() => all.extend_from_slice(&b),
            _ => break,
        }
    }
    all
}

/// The child's exit code and what it wrote, read only once a second has gone.
fn late_reader(args: &[std::ffi::OsString]) -> i32 {
    let child = on_a_pty(args, &Environ::current());
    std::thread::sleep(Duration::from_secs(1));
    let all = read_to_eof(&child);
    let code = child.wait().expect("the child");
    child.close();
    print!("{code} {}", String::from_utf8_lossy(&all));
    0
}

/// Asks after a silent child until it has gone, then reaps it.
fn exit_kept(args: &[std::ffi::OsString]) -> i32 {
    let child = on_a_pty(args, &Environ::current());
    while !child.exited() {
        std::thread::sleep(Duration::from_millis(1));
    }
    print!(
        "{}",
        child.wait().expect("the child is still there to reap")
    );
    child.close();
    0
}

/// What a quarter of a second's `readable` says of a child that stays silent,
/// and how long it took.
fn bound(args: &[std::ffi::OsString]) -> i32 {
    let child = on_a_pty(args, &Environ::current());
    let start = Instant::now();
    let ready = child.readable(0.25).expect("select");
    let took = start.elapsed().as_secs_f64();
    println!("{ready} {} {took}", child.exited());
    let _ = child.kill(libc::SIGKILL);
    child.close();
    0
}

/// This binary as the `ttys` tool, beside a pty held open and then on a pty of
/// its own: the held pty's name, then what each of the two found open.
fn beside(args: &[std::ffi::OsString]) -> i32 {
    let env = Environ::current();
    let child = on_a_pty(args, &env);
    // SAFETY: the master of a pty, and the name is copied before another call.
    let name = unsafe { std::ffi::CStr::from_ptr(libc::ptsname(child.fd)) };
    println!("held {}", name.to_str().expect("a device name"));
    let exe = std::env::current_exe().expect("this test binary");
    let words = ["--exact", "as_the_tool", "--nocapture", "--format", "terse"];
    let o = Command::new(&exe)
        .args(words)
        .env(TOOL, "ttys")
        .output()
        .expect("the tool runs");
    print!("{}", String::from_utf8_lossy(&o.stdout));
    let mut argv = vec![exe.into_os_string()];
    argv.extend(words.iter().map(std::ffi::OsString::from));
    let mut own = env;
    own.set(TOOL, "ttys");
    let second = on_a_pty(&argv, &own);
    print!("{}", String::from_utf8_lossy(&read_to_eof(&second)));
    second.close();
    let _ = child.kill(libc::SIGKILL);
    child.close();
    0
}

/// Closes the terminal and lives on for a second.
fn closes_the_terminal() -> i32 {
    // SAFETY: fds of this process, and nothing here writes to them again.
    unsafe {
        libc::close(0);
        libc::close(1);
        libc::close(2);
    }
    std::thread::sleep(Duration::from_secs(1));
    0
}

/// Each open fd that is a terminal, and its name.
fn print_ttys() -> i32 {
    for fd in 0..256 {
        // SAFETY: a number, and the name is copied before another call.
        unsafe {
            let name = libc::ttyname(fd);
            if !name.is_null() {
                println!(
                    "tty {fd} {}",
                    std::ffi::CStr::from_ptr(name).to_string_lossy()
                );
            }
        }
    }
    0
}

/// A directory of a test's own, gone when the test is.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("baud-capture-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a scratch directory");
        Scratch(dir)
    }

    fn at(&self, name: &str) -> String {
        self.0
            .join(name)
            .to_str()
            .expect("a UTF-8 path")
            .to_string()
    }

    /// The words that run `script` in bash, in this scratch home.
    fn bash(&self, script: &str) -> Vec<String> {
        [
            "env".to_string(),
            format!("HOME={}", self.0.display()),
            "HISTFILE=/dev/null".to_string(),
            "bash".to_string(),
            "--noprofile".to_string(),
            "--norc".to_string(),
            "-c".to_string(),
            script.to_string(),
        ]
        .to_vec()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// What a tool did.
struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

/// A tool as a process, given extra environment.
fn tool_in(name: &str, args: &[String], env: &[(&str, &str)]) -> Ran {
    tool_as(name, args, |cmd| {
        for (k, v) in env {
            cmd.env(k, v);
        }
    })
}

/// A tool as a process, its `Command` set up by `setup`.
fn tool_as(name: &str, args: &[String], setup: impl FnOnce(&mut Command)) -> Ran {
    let mut cmd = Command::new(std::env::current_exe().expect("this test binary"));
    cmd.args(["--exact", "as_the_tool", "--nocapture", "--format", "terse"])
        .env(TOOL, name)
        .env(ARGS, args.join("\u{1f}"));
    setup(&mut cmd);
    let o = cmd.output().expect("the tool runs");
    let out = String::from_utf8_lossy(&o.stdout);
    Ran {
        code: o.status.code(),
        out: out.strip_prefix(LIBTEST_HEADER).unwrap_or(&out).to_string(),
        err: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// A tool as a process.
fn tool(name: &str, args: &[String]) -> Ran {
    tool_in(name, args, &[])
}

/// Owned arguments.
fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(ToString::to_string).collect()
}

/// The program `name` where this process's `PATH` finds it: a system with
/// no `/usr/bin` or `/bin` beyond `env` and `sh` has its tools elsewhere.
fn on_path(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").expect("a PATH");
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("{name} is not on PATH"))
}

/// `words`, then a command.
fn with_cmd(w: &[&str], cmd: &[String]) -> Vec<String> {
    let mut all = words(w);
    all.push("--".to_string());
    all.extend_from_slice(cmd);
    all
}

fn read(path: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn text(path: &str) -> String {
    String::from_utf8(read(path)).expect("UTF-8")
}

/// The usage and error line a refusal ends with.
fn refused(r: &Ran, msg: &str) {
    assert_eq!(r.code, Some(2), "{}", r.err);
    assert!(r.err.starts_with("usage: "), "{}", r.err);
    assert!(r.err.ends_with(&format!(": error: {msg}\n")), "{}", r.err);
}

// capture-tui

#[test]
fn the_bytes_reach_the_file_and_each_read_a_step() {
    let s = Scratch::new("tui-bytes");
    let (raw, steps) = (s.at("o.raw"), s.at("o.steps"));
    let r = tool(
        "capture-tui",
        &with_cmd(
            &[
                "--seconds",
                "30",
                "--rows",
                "7",
                "--cols",
                "33",
                "--keys",
                r"a\n",
                "--key-delay",
                "0.02",
                "--first-key",
                "0.02",
                "--steps",
                &steps,
                "--out",
                &raw,
            ],
            &s.bash(r#"read x; stty size; printf 'got:%s\n' "$x""#),
        ),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    let bytes = read(&raw);
    assert_eq!(bytes, b"a\r\n7 33\r\ngot:a\r\n");
    assert_eq!(r.out, format!("{} bytes -> {raw}\n", bytes.len()));
    let list = text(&steps);
    let mut lines = list.lines();
    assert_eq!(
        lines.next(),
        Some("# COLSxROWS <bytes of the .raw that one read returned>")
    );
    let (mut sent, mut sum) = (Vec::new(), 0);
    for l in lines {
        if let Some(key) = l.strip_prefix("# sent ") {
            sent.push(key.to_string());
        } else {
            let (size, n) = l.split_once(' ').expect("a read's line");
            assert_eq!(size, "33x7");
            sum += n.parse::<usize>().expect("a length");
        }
    }
    assert_eq!(sent, ["b'a'", r"b'\n'"]);
    assert_eq!(sum, bytes.len());
}

#[test]
fn a_separator_sends_each_piece_whole() {
    let s = Scratch::new("tui-sep");
    let (raw, steps) = (s.at("o.raw"), s.at("o.steps"));
    let r = tool(
        "capture-tui",
        &with_cmd(
            &[
                "--seconds",
                "30",
                "--keys",
                r"ab,,c\n",
                "--key-sep",
                ",",
                "--key-delay",
                "0.02",
                "--first-key",
                "0.02",
                "--steps",
                &steps,
                "--out",
                &raw,
            ],
            &s.bash("read x; [ \"$x\" = abc ] && printf yes"),
        ),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(read(&raw).ends_with(b"yes"));
    let list = text(&steps);
    let sent: Vec<&str> = list
        .lines()
        .filter(|l| l.starts_with('#'))
        .skip(1)
        .collect();
    assert_eq!(sent, ["# sent b'ab'", r"# sent b'c\n'"]);
}

#[test]
fn a_query_is_answered_and_the_answer_stepped() {
    let s = Scratch::new("tui-answer");
    let (raw, steps) = (s.at("o.raw"), s.at("o.steps"));
    let r = tool(
        "capture-tui",
        &with_cmd(
            &[
                "--seconds",
                "30",
                "--answer",
                r"\x1b[c=yes\n",
                "--steps",
                &steps,
                "--out",
                &raw,
            ],
            &s.bash(r#"stty -echo; printf '\033[c'; read x; printf 'R:%s' "$x""#),
        ),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(read(&raw), b"\x1b[cR:yes");
    assert!(text(&steps).contains("# answered b'\\x1b[c' with b'yes\\n'\n"));
}

/// `action="append"`: each `--answer` is kept, where a repeated `--scan` is not.
#[test]
fn every_answer_given_is_kept() {
    let s = Scratch::new("tui-answers");
    let (raw, steps) = (s.at("o.raw"), s.at("o.steps"));
    let r = tool(
        "capture-tui",
        &with_cmd(
            &[
                "--seconds",
                "30",
                "--answer",
                "a=1",
                "--answer",
                "b=2",
                "--steps",
                &steps,
                "--out",
                &raw,
            ],
            &s.bash(r#"stty -echo; printf a; read -r -n1 x; printf b; read -r -n1 y; printf '%s%s' "$x" "$y""#),
        ),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(read(&raw), b"ab12");
    let got = text(&steps);
    assert!(got.contains("# answered b'a' with b'1'\n"), "{got}");
    assert!(got.contains("# answered b'b' with b'2'\n"), "{got}");
}

/// An answer with no `=` was a tuple of one, which died at the first read.
#[test]
fn an_answer_with_no_equals_dies_at_the_first_read() {
    let s = Scratch::new("tui-tuple");
    let raw = s.at("o.raw");
    let r = tool(
        "capture-tui",
        &with_cmd(
            &["--seconds", "30", "--answer", "q", "--out", &raw],
            &s.bash("printf out"),
        ),
    );
    assert_eq!(r.code, Some(1));
    assert!(r.err.contains("not enough values"), "{}", r.err);
    assert_eq!(r.out, "");
}

#[test]
fn nothing_read_is_exit_one() {
    let s = Scratch::new("tui-silent");
    let raw = s.at("o.raw");
    let r = tool(
        "capture-tui",
        &with_cmd(&["--seconds", "30", "--out", &raw], &s.bash("exit 0")),
    );
    assert_eq!(r.code, Some(1));
    assert_eq!(r.out, format!("0 bytes -> {raw}\n"));
    assert_eq!(read(&raw), b"");
}

#[test]
fn no_time_at_all_reads_nothing_and_still_writes_the_file() {
    let s = Scratch::new("tui-zero");
    let raw = s.at("o.raw");
    let r = tool(
        "capture-tui",
        &with_cmd(&["--seconds", "0", "--out", &raw], &s.bash("printf out")),
    );
    assert_eq!(r.code, Some(1));
    assert_eq!(r.out, format!("0 bytes -> {raw}\n"));
    assert_eq!(read(&raw), b"");
}

/// A program that will not start: Python's `execvp` walk and its message, on
/// the pty, and exit 127 there, which the tool does not read.
#[test]
fn a_program_that_will_not_start_says_so_on_the_pty() {
    let s = Scratch::new("tui-exec");
    let raw = s.at("o.raw");
    let r = tool_in(
        "capture-tui",
        &words(&["--seconds", "30", "--out", &raw, "--", "no-such-program-x"]),
        &[("PATH", "/usr/bin:/bin")],
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(
        String::from_utf8(read(&raw)).unwrap(),
        "exec no-such-program-x: [Errno 2] No such file or directory\r\n"
    );
    let r = tool(
        "capture-tui",
        &words(&["--seconds", "30", "--out", &raw, "--", "./nope/x"]),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(
        String::from_utf8(read(&raw)).unwrap(),
        "exec ./nope/x: [Errno 2] No such file or directory\r\n"
    );
}

/// A reader held off past `ptsclose`'s wait still gets everything, whether
/// the child wrote what the queue holds and exited, or wrote more than it
/// holds. Without the parent's slave fd the first two lose it all on Darwin,
/// every time. No test can show the race in `forkpty` gone, and this is the
/// same discard with the race taken out of it; `MEASUREMENTS.md`, "A child's
/// last bytes on the pty", counts the race. The children are plain programs: a
/// bash child did not lose its bytes this way with the fd gone.
#[test]
fn a_late_reader_gets_all_a_child_wrote_before_it_exited() {
    let s = Scratch::new("pty-late");
    // A PATH of the folder `echo` is in, whichever that is.
    let echo = on_path("echo");
    let dir = echo.parent().and_then(Path::to_str).expect("echo's folder");
    let path = [("PATH", dir)];
    let r = tool_in("late-reader", &words(&["./nope/x"]), &path);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(
        r.out,
        "127 exec ./nope/x: [Errno 2] No such file or directory\r\n"
    );
    let r = tool_in("late-reader", &words(&["echo", "one"]), &path);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(r.out, "0 one\r\n");
    let long = s.at("long");
    fs::write(&long, "x".repeat(5000)).unwrap();
    let r = tool_in("late-reader", &words(&["cat", &long]), &path);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(r.out, format!("0 {}", "x".repeat(5000)));
}

/// `exited` leaves the zombie: the exit code is still there for `wait`.
#[test]
fn asking_whether_the_child_has_gone_does_not_reap_it() {
    let s = Scratch::new("pty-exited");
    let r = tool("exit-kept", &s.bash("exit 7"));
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(r.out, "7");
}

/// A tool's bound is the time asked for, in ticks or not, and a child still
/// running is not called gone.
#[test]
fn a_silent_running_child_times_out_when_asked() {
    let s = Scratch::new("pty-bound");
    let r = tool("bound", &s.bash("exec sleep 30"));
    assert_eq!(r.code, Some(0), "{}", r.err);
    let words: Vec<&str> = r.out.split_whitespace().collect();
    assert_eq!(words[..2], ["false", "false"], "{}", r.out);
    let took: f64 = words[2].parse().expect("a number");
    assert!((0.25..10.0).contains(&took), "{took}");

    let raw = s.at("o.raw");
    let start = Instant::now();
    let r = tool(
        "capture-tui",
        &with_cmd(&["--seconds", "1", "--out", &raw], &s.bash("exec sleep 30")),
    );
    let took = start.elapsed().as_secs_f64();
    assert_eq!(r.code, Some(1), "{}", r.err);
    assert_eq!(r.out, format!("0 bytes -> {raw}\n"));
    assert!((1.0..20.0).contains(&took), "{took}");
}

/// A child that exits having written nothing ends the tool at once rather than
/// at `--seconds`, as `0 bytes` and exit 1.
#[test]
fn a_child_that_exits_silently_ends_the_tool_promptly() {
    let s = Scratch::new("pty-silent");
    let raw = s.at("o.raw");
    let start = Instant::now();
    let r = tool(
        "capture-tui",
        &with_cmd(
            &["--seconds", "40", "--out", &raw],
            &words(&[on_path("true").to_str().expect("a UTF-8 path")]),
        ),
    );
    assert!(start.elapsed() < Duration::from_secs(20));
    assert_eq!(r.code, Some(1), "{}", r.err);
    assert_eq!(r.out, format!("0 bytes -> {raw}\n"));
    assert_eq!(read(&raw), b"");
}

/// A grandchild left running when the child exits, SIGHUP ignored from before
/// it started. On Darwin the terminal is revoked at a session leader's exit
/// (`[[source: xnu-kern-exit]]`): the later write is lost and the tool ends
/// there. On Linux it is captured and the tool ends when it closes. Python's
/// `capture-tui`, under `-I`, gave these bytes on each.
#[test]
fn a_grandchild_outliving_the_child_ends_as_it_did_under_python() {
    let s = Scratch::new("pty-grandchild");
    let raw = s.at("o.raw");
    let start = Instant::now();
    let r = tool(
        "capture-tui",
        &with_cmd(
            &["--seconds", "40", "--out", &raw],
            &s.bash("trap '' HUP; (sleep 1; printf late) & printf early; exit 0"),
        ),
    );
    assert!(start.elapsed() < Duration::from_secs(20));
    assert_eq!(r.code, Some(0), "{}", r.err);
    let want: &[u8] = if cfg!(target_os = "macos") {
        b"early"
    } else {
        b"earlylate"
    };
    assert_eq!(read(&raw), want);
}

/// The other side of what the slave fd costs, and a departure from Python: a
/// child that closes the terminal and lives on no longer ends the tool's read
/// there, but when it exits. `MEASUREMENTS.md`, "A child's last bytes on the
/// pty". A plain program again: a bash child's read did not end at such a close
/// with the fd gone.
#[test]
fn a_child_that_closes_the_terminal_ends_the_tool_when_it_exits() {
    let s = Scratch::new("pty-closes");
    let raw = s.at("o.raw");
    let exe = std::env::current_exe().expect("this test binary");
    let start = Instant::now();
    let r = tool(
        "capture-tui",
        &with_cmd(
            &["--seconds", "40", "--out", &raw],
            &words(&[
                "/usr/bin/env",
                &format!("{TOOL}=closes"),
                exe.to_str().expect("a UTF-8 path"),
                "--exact",
                "as_the_tool",
                "--nocapture",
                "--format",
                "terse",
            ]),
        ),
    );
    let took = start.elapsed().as_secs_f64();
    assert!((1.0..20.0).contains(&took), "{took}");
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(read(&raw), b"\r\nrunning 1 test\r\n");
}

/// The fd the parent keeps is its own: a program run beside the pty, or on a
/// second pty, finds its own terminals open and never the first pty's slave.
#[test]
fn the_slave_kept_is_not_inherited() {
    let s = Scratch::new("pty-beside");
    let r = tool("beside", &s.bash("exec sleep 30"));
    assert_eq!(r.code, Some(0), "{}", r.err);
    let held = r.out.lines().next().unwrap().strip_prefix("held ").unwrap();
    let ttys: Vec<&str> = r
        .out
        .lines()
        .filter_map(|l| l.strip_prefix("tty "))
        .collect();
    // The second's own 0, 1 and 2: the listing does see a slave.
    let own: Vec<&str> = ttys
        .iter()
        .filter_map(|t| t.split_whitespace().nth(1))
        .filter(|n| *n != held && !n.ends_with("ptmx"))
        .collect();
    assert!(own.len() >= 3, "{}", r.out);
    assert!(
        !ttys.iter().any(|t| t.ends_with(&format!(" {held}"))),
        "{}",
        r.out
    );
}

/// A name that is not UTF-8 is shown as `sys.stderr` showed it, each byte
/// UTF-8 does not take as `\\udcXX`, and text that is UTF-8 as it is.
#[test]
fn a_name_that_is_not_utf8_is_shown_as_python_showed_it() {
    let s = Scratch::new("tui-name");
    let raw = s.at("o.raw");
    let cases: [(&[u8], &str); 3] = [
        (b"\xff", "exec \\udcff: "),
        (b"a\xffb/c", "exec a\\udcffb/c: "),
        ("caf\u{e9}".as_bytes(), "exec caf\u{e9}: "),
    ];
    for (name, want) in cases {
        let mut args = format!("--seconds\x1f30\x1f--out\x1f{raw}\x1f--\x1f").into_bytes();
        args.extend_from_slice(name);
        let r = tool_as("capture-tui", &[], |cmd| {
            cmd.env("PATH", "/usr/bin:/bin")
                .env(ARGS, std::ffi::OsStr::from_bytes(&args));
        });
        assert_eq!(r.code, Some(0), "{}", r.err);
        assert_eq!(
            String::from_utf8(read(&raw)).unwrap(),
            format!("{want}[Errno 2] No such file or directory\r\n")
        );
    }
}

/// Python's start-up ignores `SIGXFSZ`, and an ignored signal stays ignored
/// across an exec, so a program on the pty inherits that.
#[test]
fn a_child_on_the_pty_ignores_sigxfsz() {
    let s = Scratch::new("tui-sigxfsz");
    let raw = s.at("o.raw");
    let exe = std::env::current_exe().unwrap();
    let r = tool(
        "capture-tui",
        &with_cmd(
            &["--seconds", "30", "--out", &raw],
            &s.bash(&format!(
                "{TOOL}=sigxfsz exec {} --exact as_the_tool --nocapture --format terse",
                exe.display()
            )),
        ),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    let got = String::from_utf8_lossy(&read(&raw)).into_owned();
    assert!(got.contains("SIGXFSZ ignored: true"), "{got}");
}

/// A file that is not executable is reported ahead of the name not being
/// found later along the path, as `_execvpe` saves it.
#[test]
fn a_permission_error_outranks_a_later_miss() {
    let s = Scratch::new("tui-perm");
    fs::write(s.at("plain"), "not a program").unwrap();
    let raw = s.at("o.raw");
    let path = format!("{}:/nonexistent-dir", s.0.display());
    let r = tool_in(
        "capture-tui",
        &words(&["--seconds", "30", "--out", &raw, "--", "plain"]),
        &[("PATH", &path)],
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(
        String::from_utf8(read(&raw)).unwrap(),
        "exec plain: [Errno 13] Permission denied\r\n"
    );
}

/// A path entry ending in a slash finds the file, and an empty one names it
/// as it is, so it is looked for in the directory the tool runs in: `plain`
/// is refused there, where `/plain` would not be found.
#[test]
fn a_path_entry_is_joined_as_python_joined_it() {
    let s = Scratch::new("tui-join");
    fs::write(s.at("plain"), "not a program").unwrap();
    let raw = s.at("o.raw");
    let run = |path: &str, dir: &Path, want: &str| {
        let r = tool_as(
            "capture-tui",
            &words(&["--seconds", "30", "--out", &raw, "--", "plain"]),
            |cmd| {
                cmd.env("PATH", path).current_dir(dir);
            },
        );
        assert_eq!(r.code, Some(0), "{}", r.err);
        assert_eq!(String::from_utf8(read(&raw)).unwrap(), want, "{path}");
    };
    let denied = "exec plain: [Errno 13] Permission denied\r\n";
    let missing = "exec plain: [Errno 2] No such file or directory\r\n";
    let elsewhere = Path::new(env!("CARGO_MANIFEST_DIR"));
    run(&format!("{}/", s.0.display()), elsewhere, denied);
    run("", elsewhere, missing);
    run("", &s.0, denied);
}

#[test]
fn term_is_kept_or_given() {
    let s = Scratch::new("tui-term");
    let raw = s.at("o.raw");
    let script = s.bash(r#"printf '%s' "$TERM""#);
    let args = with_cmd(&["--seconds", "30", "--out", &raw], &script);
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "as_the_tool", "--nocapture", "--format", "terse"])
        .env(TOOL, "capture-tui")
        .env(ARGS, args.join("\u{1f}"))
        .env_remove("TERM");
    assert_eq!(cmd.output().unwrap().status.code(), Some(0));
    assert_eq!(read(&raw), b"xterm-256color");
    let r = tool_in("capture-tui", &args, &[("TERM", "vt100")]);
    assert_eq!(r.code, Some(0));
    assert_eq!(read(&raw), b"vt100");
}

/// A tool's environment with no locale of the caller's, then `set`: what
/// `/usr/bin/env` printed under it.
fn env_under_locale(s: &Scratch, set: &[(&str, &str)]) -> Vec<String> {
    let raw = s.at("env.raw");
    let r = tool_as(
        "capture-tui",
        &words(&["--seconds", "30", "--out", &raw, "--", "/usr/bin/env"]),
        |cmd| {
            for name in ["LANG", "LC_ALL", "LC_CTYPE", "TERM"] {
                cmd.env_remove(name);
            }
            for (k, v) in set {
                cmd.env(k, v);
            }
        },
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    String::from_utf8(read(&raw))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Python's start-up coerces a C locale before any script runs, and a child
/// inherits it. The variable comes after the caller's and before `TERM`, which
/// the tool adds itself.
#[test]
fn a_c_locale_reaches_the_child_coerced() {
    let s = Scratch::new("tui-locale");
    let got = env_under_locale(&s, &[]);
    assert_eq!(
        got[got.len() - 2..],
        ["LC_CTYPE=C.UTF-8", "TERM=xterm-256color"],
        "{got:?}"
    );
    let ctype = |got: &[String]| {
        got.iter()
            .filter(|l| l.starts_with("LC_CTYPE="))
            .cloned()
            .collect::<Vec<_>>()
    };
    // Each of these resolves to C, and an empty LC_ALL is no LC_ALL.
    for set in [
        &[("LC_CTYPE", "C")][..],
        &[("LANG", "C")][..],
        &[("LANG", "nonsense")][..],
        &[("LC_ALL", "")][..],
    ] {
        let got = env_under_locale(&s, set);
        assert_eq!(ctype(&got), ["LC_CTYPE=C.UTF-8"], "{set:?}");
    }
    // A replaced value stays where the caller's was: `Command` sorts what it
    // is given, so it sits ahead of the variables named after it.
    let got = env_under_locale(&s, &[("LC_CTYPE", "C")]);
    let at = got.iter().position(|l| l.starts_with("LC_CTYPE=")).unwrap();
    assert!(at < got.len() - 2, "{got:?}");
    // A set LC_ALL stops it, whatever it says.
    for set in [&[("LC_ALL", "C")][..], &[("LC_ALL", "nonsense")][..]] {
        let got = env_under_locale(&s, set);
        assert_eq!(ctype(&got), [] as [String; 0], "{set:?}");
    }
}

/// A caller's locale that is not C is left alone. `en_US.UTF-8` is a Mac's by
/// default; a Linux host may not have it, and without it the answer differs.
#[cfg(target_os = "macos")]
#[test]
fn a_locale_that_is_not_c_is_left_alone() {
    let s = Scratch::new("tui-locale-kept");
    let got = env_under_locale(&s, &[("LANG", "en_US.UTF-8")]);
    assert!(!got.iter().any(|l| l.starts_with("LC_CTYPE=")), "{got:?}");
    let got = env_under_locale(&s, &[("LC_CTYPE", "POSIX")]);
    assert!(got.iter().any(|l| l == "LC_CTYPE=POSIX"), "{got:?}");
    // Python read no `PYTHONCOERCECLOCALE` under `-I`.
    let got = env_under_locale(&s, &[("PYTHONCOERCECLOCALE", "0")]);
    assert!(got.iter().any(|l| l == "LC_CTYPE=C.UTF-8"), "{got:?}");
}

/// The programs a tool runs off the pty inherit the coercion too: `nix` and
/// `sips` here, each a stand-in that writes what it saw and fails.
#[test]
fn a_program_run_beside_the_pty_is_coerced_too() {
    let s = Scratch::new("locale-children");
    let seen = s.at("seen");
    for stub in ["nix", "sips"] {
        exec::script(
            Path::new(&s.at(stub)),
            &format!("#!/bin/sh\nprintf '%s' \"${{LC_CTYPE-unset}}\" > '{seen}'\nexit 7\n"),
        );
    }
    let path = format!("{}:/usr/bin:/bin", s.0.display());
    for (tool_name, args) in [
        ("record-kitten-icat", ["--out", &s.at("k")].as_slice()),
        ("gen-yazi-files", [s.at("y").as_str()].as_slice()),
    ] {
        let _ = fs::remove_file(&seen);
        let r = tool_as(tool_name, &words(args), |cmd| {
            cmd.env("PATH", &path);
            for name in ["LANG", "LC_ALL", "LC_CTYPE"] {
                cmd.env_remove(name);
            }
        });
        assert_eq!(r.code, Some(1), "{tool_name}: {}", r.err);
        assert_eq!(read(&seen), b"C.UTF-8", "{tool_name}");
    }
}

/// With nothing asked for, 100x30, and `stty` shows it after the first key.
#[test]
fn the_window_defaults_to_a_hundred_by_thirty() {
    let s = Scratch::new("tui-size");
    let raw = s.at("o.raw");
    let r = tool(
        "capture-tui",
        &with_cmd(
            &["--keys", r"\n", "--key-delay", "0.02", "--out", &raw],
            &s.bash("read x; stty size"),
        ),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(read(&raw), b"\r\n30 100\r\n");
}

/// The first key waits `--first-key`, or `--key-delay` when that is not given.
#[test]
fn the_first_key_waits() {
    let s = Scratch::new("tui-first");
    let (raw, steps) = (s.at("o.raw"), s.at("o.steps"));
    let cmd = s.bash("printf up; exec sleep 30");
    for wait in [
        ["--first-key", "30", "--key-delay", "0.01"],
        ["--key-delay", "30", "--rows", "24"],
    ] {
        let mut args = words(&[
            "--seconds",
            "0.3",
            "--keys",
            "a",
            "--steps",
            &steps,
            "--out",
            &raw,
        ]);
        args.extend(words(&wait));
        args.push("--".to_string());
        args.extend_from_slice(&cmd);
        let r = tool("capture-tui", &args);
        assert!(r.err.is_empty(), "{wait:?}: {}", r.err);
        assert!(!text(&steps).contains("# sent"), "{wait:?}");
    }
    let r = tool(
        "capture-tui",
        &with_cmd(
            &[
                "--seconds",
                "30",
                "--keys",
                r"\n",
                "--first-key",
                "0.01",
                "--key-delay",
                "30",
                "--steps",
                &steps,
                "--out",
                &raw,
            ],
            &s.bash("read x"),
        ),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(text(&steps).contains("# sent"));
}

#[test]
fn every_double_dash_is_taken_out_of_the_command() {
    let s = Scratch::new("tui-dashes");
    let raw = s.at("o.raw");
    let mut cmd = s.bash(r#"printf '%s,' "$@""#);
    cmd.extend(words(&["name", "a", "--", "b"]));
    let r = tool(
        "capture-tui",
        &with_cmd(&["--seconds", "30", "--out", &raw], &cmd),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(read(&raw), b"a,b,");
}

#[test]
fn the_tui_tool_dies_where_the_script_died() {
    let s = Scratch::new("tui-die");
    let raw = s.at("o.raw");
    let cmd = s.bash("exit 0");
    let r = tool("capture-tui", &words(&["--out", &raw]));
    assert_eq!((r.code, r.err.as_str()), (Some(2), "no command given\n"));
    for args in [
        vec!["--seconds", "1", "--keys", r"\xZ", "--out", &raw],
        vec!["--seconds", "1", "--keys", r"\u0100", "--out", &raw],
        vec!["--seconds", "1", "--answer", r"\xZ=x", "--out", &raw],
        vec!["--seconds", "1", "--cols", "70000", "--out", &raw],
        vec!["--seconds", "1", "--rows", "-1", "--out", &raw],
    ] {
        let r = tool("capture-tui", &with_cmd(&args, &cmd));
        assert_eq!(r.code, Some(1), "{args:?}: {}", r.err);
        assert_eq!(r.out, "");
    }
    // With bytes to write, so only the file can be what ended it.
    let missing = s.at("no/such/dir/o.raw");
    let r = tool(
        "capture-tui",
        &with_cmd(&["--seconds", "30", "--out", &missing], &s.bash("printf x")),
    );
    assert_eq!(r.code, Some(1));
    assert_eq!(r.out, "");
    let r = tool(
        "capture-tui",
        &with_cmd(
            &["--seconds", "30", "--steps", &missing, "--out", &raw],
            &s.bash("printf x"),
        ),
    );
    assert_eq!(r.code, Some(1));
}

// The command-line rules, each spelling once.

#[test]
fn a_long_option_is_taken_by_any_prefix_or_with_an_equals() {
    let s = Scratch::new("args-prefix");
    let raw = s.at("o.raw");
    for args in [
        vec!["--sec", "0", "--ou", &raw, "x"],
        vec!["--seconds=0", "--o", &raw, "x"],
        vec!["--se=0", "--out", &raw, "--", "x"],
    ] {
        let r = tool("capture-tui", &words(&args));
        assert_eq!(r.code, Some(1), "{args:?}: {}", r.err);
        assert_eq!(r.out, format!("0 bytes -> {raw}\n"));
    }
    // A word with a space is a positional: the command, here not started in
    // time, and not an option nobody recognized.
    let r = tool(
        "capture-tui",
        &words(&["--seconds", "0", "--out", &raw, "-x y"]),
    );
    assert_eq!(r.code, Some(1), "{}", r.err);
    // A negative number is a value, and the time it names has passed.
    for negative in ["-1", "-.5", "-1.5"] {
        let r = tool(
            "capture-tui",
            &words(&["--seconds", negative, "--out", &raw, "x"]),
        );
        assert_eq!(r.code, Some(1), "{negative}: {}", r.err);
        assert_eq!(r.out, format!("0 bytes -> {raw}\n"));
    }
}

#[test]
fn a_command_line_is_refused_as_argparse_refused_it() {
    let tui = |w: &[&str]| tool("capture-tui", &words(w));
    refused(
        &tui(&["--s", "1"]),
        "ambiguous option: --s could match --seconds, --steps",
    );
    refused(
        &tui(&["--k"]),
        "ambiguous option: --k could match --keys, --key-delay, --key-sep",
    );
    refused(
        &tui(&["--seconds"]),
        "argument --seconds: expected one argument",
    );
    refused(
        &tui(&["--seconds", "--out"]),
        "argument --seconds: expected one argument",
    );
    refused(
        &tui(&["--seconds", "--", "x"]),
        "argument --seconds: expected one argument",
    );
    refused(
        &tui(&["--seconds", "abc"]),
        "argument --seconds: invalid float value: 'abc'",
    );
    refused(
        &tui(&["--rows", "2.5"]),
        "argument --rows: invalid int value: '2.5'",
    );
    refused(&tui(&[]), "the following arguments are required: --out");
    // Every word from the first positional on is the command's, so `--out`
    // after one was not seen.
    refused(
        &tui(&["btop", "--out", "x"]),
        "the following arguments are required: --out",
    );
    refused(
        &tui(&["--bogus", "--out", "x"]),
        "unrecognized arguments: --bogus",
    );
    refused(
        &tui(&["--help=x"]),
        "argument -h/--help: ignored explicit argument 'x'",
    );
    refused(
        &tui(&["-h="]),
        "argument -h/--help: ignored explicit argument ''",
    );
    refused(
        &tui(&["-h=x"]),
        "argument -h/--help: ignored explicit argument 'x'",
    );
    refused(
        &tui(&["-h-"]),
        "argument -h/--help: ignored explicit argument '-'",
    );
    // A word with a space, or a bare `-`, is a positional: here, the command.
    refused(
        &tui(&["-x y"]),
        "the following arguments are required: --out",
    );
    refused(&tui(&["-"]), "the following arguments are required: --out");
    let resize = |w: &[&str]| tool("capture-resize", &words(w));
    refused(
        &resize(&[]),
        "the following arguments are required: --sizes, --out",
    );
    let kit = |w: &[&str]| tool("record-kitten-icat", &words(w));
    refused(&kit(&[]), "--out or --scan");
    refused(
        &kit(&["--scan"]),
        "argument --scan: expected at least one argument",
    );
    refused(&kit(&["x"]), "unrecognized arguments: x");
    refused(
        &kit(&["--scan", "--timeout", "1"]),
        "argument --scan: expected at least one argument",
    );
}

#[test]
fn help_goes_first_and_exits_zero() {
    for (tool_name, usage) in [
        ("capture-tui", "usage: capture-tui [-h]"),
        ("capture-resize", "usage: capture-resize [-h]"),
        ("capture-yazi-walk", "usage: capture-yazi-walk [-h]"),
        ("record-kitten-icat", "usage: record-kitten-icat [-h]"),
    ] {
        for args in [
            vec!["-h"],
            vec!["--help"],
            vec!["--he"],
            vec!["-hh"],
            vec!["-hx"],
            vec!["--bogus", "-h"],
        ] {
            let r = tool(tool_name, &words(&args));
            assert_eq!(r.code, Some(0), "{tool_name} {args:?}: {}", r.err);
            assert!(r.out.starts_with(usage), "{}", r.out);
            assert!(
                r.out.contains("show this help message and exit"),
                "{}",
                r.out
            );
        }
    }
    // An error before it is the one that counts.
    let r = tool("capture-tui", &words(&["--seconds", "abc", "-h"]));
    refused(&r, "argument --seconds: invalid float value: 'abc'");
}

// capture-resize

const SIZES: &str = "# a burst, and a key\n80x20\n60x10 50x6\nenter\n";

#[test]
fn each_size_is_a_step_and_a_burst_drains_once() {
    let s = Scratch::new("resize-steps");
    let sizes = s.at("s.sizes");
    fs::write(&sizes, SIZES).unwrap();
    let out = s.at("r");
    let began = std::time::Instant::now();
    let r = tool(
        "capture-resize",
        &with_cmd(
            &[
                "--sizes", &sizes, "--out", &out, "--settle", "0.05", "--step", "0.05", "--gap",
                "0", "--tail", "30",
            ],
            &s.bash("printf ready; read x; printf done"),
        ),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    // The Enter may be echoed ahead of a slow start's `ready`: what is sure is
    // the end, and that the child's end ended the tail: it did not wait out its thirty seconds.
    assert!(began.elapsed().as_secs() < 25);
    let raw = read(&format!("{out}.raw"));
    let shown = String::from_utf8_lossy(&raw);
    assert!(shown.contains("ready"), "{shown}");
    assert!(raw.ends_with(b"done"), "{raw:?}");
    let steps = text(&format!("{out}.steps"));
    let mut lines = steps.lines();
    assert_eq!(
        lines.next(),
        Some("# COLSxROWS <bytes of the .raw that arrived at that size>")
    );
    let parsed: Vec<(&str, usize)> = lines
        .map(|l| {
            let (size, n) = l.split_once(' ').expect("a step");
            (size, n.parse().expect("a length"))
        })
        .collect();
    let sizes_in_order: Vec<&str> = parsed.iter().map(|(s, _)| *s).collect();
    assert_eq!(
        sizes_in_order,
        ["80x20", "80x20", "60x10", "50x6", "50x6", "50x6"]
    );
    // The size of a burst before its last has no bytes.
    assert_eq!(parsed[2].1, 0);
    assert_eq!(parsed.iter().map(|(_, n)| n).sum::<usize>(), raw.len());
    assert_eq!(
        r.out,
        format!(
            "{out}.raw: {} bytes over 6 steps\n{out}.steps: 6 steps\n",
            raw.len()
        )
    );
}

/// fish draws no prompt until its `DA1` is answered, so a resize capture
/// answers a query its program writes, as `capture-tui` does.
#[test]
fn a_resize_capture_answers_a_query() {
    let s = Scratch::new("resize-answer");
    let sizes = s.at("s.sizes");
    fs::write(&sizes, "80x20\n").unwrap();
    let out = s.at("r");
    let r = tool(
        "capture-resize",
        &with_cmd(
            &[
                "--sizes",
                &sizes,
                "--out",
                &out,
                "--settle",
                "30",
                "--step",
                "0",
                "--tail",
                "0",
                "--answer",
                r"\x1b[c=yes\n",
            ],
            &s.bash(r#"stty -echo; printf '\033[c'; read x; printf 'R:%s' "$x""#),
        ),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(read(&format!("{out}.raw")), b"\x1b[cR:yes");
}

/// `TERM` is given only where it is unset.
#[test]
fn the_resize_tool_keeps_term_or_gives_it() {
    let s = Scratch::new("resize-term");
    let sizes = s.at("s.sizes");
    fs::write(&sizes, "80x20\n").unwrap();
    let out = s.at("r");
    let args = with_cmd(
        &[
            "--sizes", &sizes, "--out", &out, "--settle", "30", "--step", "0", "--tail", "0",
        ],
        &s.bash(r#"printf '%s' "$TERM""#),
    );
    let r = tool_in("capture-resize", &args, &[("TERM", "vt100")]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(read(&format!("{out}.raw")), b"vt100");
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "as_the_tool", "--nocapture", "--format", "terse"])
        .env(TOOL, "capture-resize")
        .env(ARGS, args.join("\u{1f}"))
        .env_remove("TERM");
    assert_eq!(cmd.output().unwrap().status.code(), Some(0));
    assert_eq!(read(&format!("{out}.raw")), b"xterm-256color");
}

/// The steps' header and first column of a committed recording, for its own
/// sizes file: what a run of the tool on it would have to write.
#[test]
fn the_steps_shape_is_the_committed_recording_s() {
    let steps = fs::read_to_string("../tests/resize/short.steps").expect("a committed fixture");
    let header = steps.lines().next().unwrap();
    assert_eq!(
        header,
        "# COLSxROWS <bytes of the .raw that arrived at that size>"
    );
    let sizes = fs::read_to_string("../tests/resize/short.sizes").expect("a committed fixture");
    let lines = resize::parse_sizes("short.sizes", &sizes).unwrap();
    let mut want = Vec::new();
    let resize::Line::Sizes(first) = lines
        .iter()
        .find(|l| matches!(l, resize::Line::Sizes(_)))
        .unwrap()
    else {
        unreachable!();
    };
    let mut at = first[0];
    want.push(at);
    for l in &lines {
        match l {
            resize::Line::Enter => want.push(at),
            resize::Line::Sizes(burst) => {
                want.extend_from_slice(burst);
                at = *burst.last().unwrap();
            }
        }
    }
    want.push(at);
    let got: Vec<(i64, i64)> = steps
        .lines()
        .skip(1)
        .map(|l| {
            let (c, r) = l.split_once(' ').unwrap().0.split_once('x').unwrap();
            (c.parse().unwrap(), r.parse().unwrap())
        })
        .collect();
    assert_eq!(got, want);
}

#[test]
fn the_resize_tool_dies_where_the_script_died() {
    let s = Scratch::new("resize-die");
    let out = s.at("r");
    let sizes = s.at("s.sizes");
    fs::write(&sizes, SIZES).unwrap();
    let quick = |extra: &[&str], sizes: &str, cmd: &[String]| {
        let mut a = words(&[
            "--sizes", sizes, "--out", &out, "--settle", "0", "--step", "0", "--tail", "0",
        ]);
        a.extend(words(extra));
        a.push("--".to_string());
        a.extend_from_slice(cmd);
        tool("capture-resize", &a)
    };
    let cmd = s.bash("read x");
    let r = tool(
        "capture-resize",
        &words(&["--sizes", &sizes, "--out", &out]),
    );
    assert_eq!(r.code, Some(1));
    assert_eq!(r.err, "give a command after --, e.g. -- /bin/zsh -i\n");
    let r = quick(&[], &s.at("missing"), &cmd);
    assert_eq!(r.code, Some(1));
    fs::write(s.at("bad"), "80x20\nnope\n").unwrap();
    let r = quick(&[], &s.at("bad"), &cmd);
    assert_eq!(r.code, Some(1));
    assert!(
        r.err.contains(":2: expected COLSxROWS or `enter`"),
        "{}",
        r.err
    );
    fs::write(s.at("none"), "# only this\n").unwrap();
    let r = quick(&[], &s.at("none"), &cmd);
    assert_eq!(r.code, Some(1));
    assert!(
        r.err
            .ends_with("no sizes, so there is nothing to capture\n"),
        "{}",
        r.err
    );
    fs::write(s.at("utf"), [0xff, b'\n']).unwrap();
    let r = quick(&[], &s.at("utf"), &cmd);
    assert_eq!(r.code, Some(1));
    // A gap `time.sleep` will not take, reached only in a burst.
    let r = quick(&["--gap", "-1"], &sizes, &cmd);
    assert_eq!(r.code, Some(1));
    assert!(r.err.contains("non-negative"), "{}", r.err);
    assert!(!Path::new(&format!("{out}.raw")).exists());
    fs::write(s.at("wide"), "70000x20\n").unwrap();
    let r = quick(&[], &s.at("wide"), &cmd);
    assert_eq!(r.code, Some(1));
    // Nothing the shell did is the tool's exit code. No `enter` here: a write
    // to a pty whose child has gone is `EIO`, and exit 1, as it was.
    fs::write(s.at("one"), "80x20\n").unwrap();
    let r = quick(&[], &s.at("one"), &words(&["no-such-program-x"]));
    assert_eq!(r.code, Some(0), "{}", r.err);
}

// capture-yazi-walk

/// A yazi: both bursts of queries, then a preview at the start and a preview
/// a key, or `$1` previews in all.
fn stand_in(previews: u32) -> String {
    format!(
        r#"stty -echo -icanon
printf 'q1\033[0c'; read -r -d c x
printf 'q2\033[0c'; read -r -d c x
printf '\033P9;1qbody0\033\\'
i=1
while [ $i -lt {previews} ]; do
  read -r -n 1 k
  printf '\033P9;1qbody%s%s\033\\' "$i" "$k"
  i=$((i+1))
done
exec sleep 30"#
    )
}

#[test]
fn a_walk_answers_twice_and_steps_each_preview() {
    let s = Scratch::new("walk-full");
    let raw = s.at("w.raw");
    let r = tool(
        "capture-yazi-walk",
        &with_cmd(
            &["--out", &raw, "--quiet", "0.05", "--limit", "60"],
            &s.bash(&stand_in(13)),
        ),
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    let bytes = read(&raw);
    assert_eq!(yazi_walk::finished_previews(&bytes), 13);
    let mut lines: Vec<&str> = r.out.lines().collect();
    let last = lines.pop().unwrap();
    assert_eq!(last, format!("{} bytes, 13 previews -> {raw}", bytes.len()));
    let sent: Vec<&str> = lines
        .iter()
        .map(|l| l.split_once('\t').expect("offset, then key").1)
        .collect();
    assert_eq!(sent.len(), 14);
    assert_eq!(
        sent[0],
        r"b'\x1bP>|baud\x1b\\\x1b[6;34;17t\x1b]11;rgb:1010/1010/1010\x07\x1b[?6;4c'"
    );
    assert_eq!(
        sent[1],
        r"b'\x1bP1$r1 q\x1b\\\x1b[?12;2$y\x1b[?0u\x1b[?6;4c'"
    );
    assert_eq!(sent[2..8], ["b'j'"; 6]);
    assert_eq!(sent[8..14], ["b'k'"; 6]);
    // The key each preview answers, in the order they were pressed.
    let shown = String::from_utf8_lossy(&bytes);
    assert!(shown.contains("body1j") && shown.contains("body6j"));
    assert!(shown.contains("body7k") && shown.contains("body12k"));
}

#[test]
fn a_walk_cut_short_is_exit_one_and_the_file_is_kept() {
    let s = Scratch::new("walk-short");
    let raw = s.at("w.raw");
    let r = tool(
        "capture-yazi-walk",
        &with_cmd(
            &["--out", &raw, "--quiet", "30", "--limit", "60"],
            &s.bash(r"printf '\033P9;1qonly\033\\'"),
        ),
    );
    assert_eq!(r.code, Some(1), "{}", r.err);
    assert_eq!(yazi_walk::finished_previews(&read(&raw)), 1);
    assert!(
        r.out.ends_with(&format!("1 previews -> {raw}\n")),
        "{}",
        r.out
    );
    assert_eq!(r.err, "");
}

#[test]
fn a_walk_that_never_finishes_gives_up_and_says_so() {
    let s = Scratch::new("walk-limit");
    let raw = s.at("w.raw");
    let r = tool(
        "capture-yazi-walk",
        &with_cmd(
            &["--out", &raw, "--quiet", "0.02", "--limit", "0.2"],
            &s.bash("exec sleep 30"),
        ),
    );
    assert_eq!(r.code, Some(1), "{}", r.err);
    assert_eq!(r.err, "gave up after 0.2 s: 0 previews\n");
    assert_eq!(r.out, format!("0 bytes, 0 previews -> {raw}\n"));
}

/// With nothing asked for, 117x40.
#[test]
fn a_walk_defaults_to_forty_rows_of_117() {
    let s = Scratch::new("walk-size");
    let raw = s.at("w.raw");
    let r = tool(
        "capture-yazi-walk",
        &with_cmd(
            &["--out", &raw, "--quiet", "0.02", "--limit", "60"],
            &s.bash(r#"until [ "$(stty size)" != "0 0" ]; do :; done; stty size"#),
        ),
    );
    assert_eq!(r.code, Some(1), "{}", r.err);
    assert_eq!(read(&raw), b"40 117\r\n");
}

#[test]
fn the_walk_tool_dies_where_the_script_died() {
    let s = Scratch::new("walk-die");
    let raw = s.at("w.raw");
    let r = tool("capture-yazi-walk", &words(&["--out", &raw]));
    assert_eq!(r.code, Some(1));
    assert_eq!(r.err, "give a command after --, e.g. -- yazi\n");
    let missing = s.at("no/such/w.raw");
    let r = tool(
        "capture-yazi-walk",
        &with_cmd(&["--out", &missing, "--limit", "0"], &s.bash("exit 0")),
    );
    assert_eq!(r.code, Some(1));
    let r = tool(
        "capture-yazi-walk",
        &with_cmd(&["--out", &raw, "--cols", "70000"], &s.bash("exit 0")),
    );
    assert_eq!(r.code, Some(1));
}

// record-kitten-icat

/// A kitten: it says what it was given, and goes on as `script` says.
fn stand_in_kitten(s: &Scratch, script: &str) -> String {
    let path = s.at("kitten");
    exec::script(
        Path::new(&path),
        &format!(
            "#!/usr/bin/env -S bash --noprofile --norc\nexport HISTFILE=/dev/null\nprintf 'args:%s|' \"$@\"\n{script}\n"
        ),
    );
    path
}

#[test]
fn stream_mode_passes_its_flags_and_needs_no_answer() {
    let s = Scratch::new("kitten-stream");
    let k = stand_in_kitten(&s, r"printf '\033_Ga=T;AA\033\\'");
    let m = kitten::mode_named("stream").unwrap();
    let got = kitten::record(&k, m, "pic.png", 80, 24, 30.0).unwrap();
    assert_eq!(got.code, 0);
    assert_eq!(
        got.raw,
        b"args:icat|args:--transfer-mode=stream|args:pic.png|\x1b_Ga=T;AA\x1b\\"
    );
    let m = kitten::mode_named("placeholder").unwrap();
    let got = kitten::record(&k, m, "p", 80, 24, 30.0).unwrap();
    assert!(
        got.raw.starts_with(
            b"args:icat|args:--transfer-mode=stream|args:--unicode-placeholder|args:p|"
        )
    );
}

/// The burst is answered when the DA arrives, and what is answered depends
/// on the mode.
#[test]
fn a_detect_mode_answers_once_the_da_has_arrived() {
    let s = Scratch::new("kitten-detect");
    let script = r#"stty -echo -icanon
printf '\033_Ga=q,t=d,i=1;AA\033\\\033_Ga=q,t=f,i=2;BB\033\\\033[c'
read -r -d c x
printf 'GOT:%q' "$x""#;
    let k = stand_in_kitten(&s, script);
    for (name, want) in [
        ("detect-da12", r"GOT:$'\E[?1;2'"),
        ("detect-da64", r"GOT:$'\E[?6;4'"),
        ("detect-direct", r"GOT:$'\E_Gi=1;OK\E\\\E[?6;4'"),
    ] {
        let got = kitten::record(&k, kitten::mode_named(name).unwrap(), "p", 80, 24, 30.0).unwrap();
        let shown = String::from_utf8_lossy(&got.raw);
        assert!(shown.ends_with(want), "{name}: {shown}");
        assert_eq!(got.code, 0);
    }
}

#[test]
fn how_kitten_ended_is_reported_and_a_hang_is_refused() {
    let s = Scratch::new("kitten-end");
    let m = kitten::mode_named("stream").unwrap();
    let k = stand_in_kitten(&s, "exit 3");
    assert_eq!(kitten::record(&k, m, "p", 80, 24, 30.0).unwrap().code, 3);
    let k = stand_in_kitten(&s, "kill -9 $$");
    assert_eq!(kitten::record(&k, m, "p", 80, 24, 30.0).unwrap().code, -9);
    let k = stand_in_kitten(&s, "exec sleep 30");
    let e = kitten::record(&k, m, "pic", 80, 24, 0.2).err().unwrap();
    assert_eq!(e, "kitten icat pic did not finish in 0.2s");
    // The detect burst's own errors end the run the same way.
    let k = stand_in_kitten(&s, r"printf '\033_Ga=q,bad;A\033\\\033[c'; exec sleep 30");
    let bad = kitten::mode_named("detect-da12").unwrap();
    assert!(kitten::record(&k, bad, "p", 80, 24, 30.0).is_err());
}

/// The window's pixels, `cols * 17` by `rows * 34`, as kitten reads them.
#[test]
fn the_pty_carries_the_pixels_the_app_reports() {
    let s = Scratch::new("kitten-pixels");
    let exe = std::env::current_exe().unwrap();
    let script = format!(
        "until [ \"$(stty size)\" != \"0 0\" ]; do :; done\n{TOOL}=winsize exec {} --exact as_the_tool --nocapture --format terse",
        exe.display()
    );
    let k = stand_in_kitten(&s, &script);
    let m = kitten::mode_named("stream").unwrap();
    let got = kitten::record(&k, m, "p", 113, 32, 30.0).unwrap();
    let shown = String::from_utf8_lossy(&got.raw);
    assert!(shown.contains("113 32 1921 1088"), "{shown}");
}

/// `if not args.out`: an empty `--out` is none, so nothing is built for it.
#[test]
fn an_empty_out_is_refused_as_no_out() {
    let s = Scratch::new("kitten-empty-out");
    for args in [&["--out="][..], &["--out", ""][..]] {
        let r = tool_in(
            "record-kitten-icat",
            &words(args),
            &[("PATH", s.0.to_str().unwrap())],
        );
        refused(&r, "--out or --scan");
    }
}

/// `nargs="+"` stores: a later `--scan` replaces the roots before it, whichever
/// way either is spelled. `a` holds the interlaced picture.
#[test]
fn a_later_scan_replaces_the_one_before() {
    let s = Scratch::new("kitten-scans");
    let png = |il: u8| {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend_from_slice(&100u32.to_be_bytes());
        b.extend_from_slice(&[0, 0, 0, 1, 8, 2, 0, 0, il]);
        b.extend_from_slice(b"xxxx");
        b
    };
    let (a, b) = (s.at("a"), s.at("b"));
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    fs::write(s.at("a/one.png"), png(1)).unwrap();
    fs::write(s.at("b/two.png"), png(0)).unwrap();
    let (a_eq, b_eq) = (format!("--scan={a}"), format!("--scan={b}"));
    let cases: [(&[&str], &str); 5] = [
        (&["--scan", &a, "--scan", &b], "1 PNGs, 0 interlaced"),
        (&["--scan", &b, "--scan", &a], "1 PNGs, 1 interlaced"),
        (&["--scan", &a, &b, "--scan", &b], "1 PNGs, 0 interlaced"),
        (&["--scan", &a, &b_eq], "1 PNGs, 0 interlaced"),
        (&[&a_eq, "--scan", &b, &a], "2 PNGs, 1 interlaced"),
    ];
    for (args, want) in cases {
        let r = tool("record-kitten-icat", &words(args));
        assert_eq!(r.code, Some(0), "{}", r.err);
        assert_eq!(r.out.lines().next(), Some(want), "{args:?}");
    }
}

#[test]
fn a_scan_walks_links_as_the_script_walked_them() {
    let s = Scratch::new("kitten-scan");
    let png = |w: u32, il: u8| {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&[0, 0, 0, 1, 8, 2, 0, 0, il]);
        b.extend_from_slice(b"xxxx");
        b
    };
    fs::create_dir_all(s.at("t/a")).unwrap();
    fs::write(s.at("t/a/one.png"), png(100, 0)).unwrap();
    fs::write(s.at("t/a/two.PNG"), png(1000, 1)).unwrap();
    fs::write(
        s.at("t/a/plain.png"),
        b"a file with no such header at all..",
    )
    .unwrap();
    fs::write(s.at("t/a/not.jpg"), png(100, 0)).unwrap();
    // A link to a file already counted, and to a directory, which is not
    // followed; and one that points nowhere.
    std::os::unix::fs::symlink("one.png", s.at("t/a/link.png")).unwrap();
    std::os::unix::fs::symlink("a", s.at("t/dirlink")).unwrap();
    std::os::unix::fs::symlink("gone", s.at("t/gone.png")).unwrap();
    let r = tool("record-kitten-icat", &words(&["--scan", &s.at("t")]));
    assert_eq!(r.code, Some(0), "{}", r.err);
    let mut lines = r.out.lines();
    assert_eq!(lines.next(), Some("2 PNGs, 1 interlaced"));
    assert_eq!(
        lines.next(),
        Some(
            "113x32 (1921 wide): 2 sent as-is, 66 bytes; Adam7 1 (50.00%), 33 bytes (50.00%); 0 sent raw"
        )
    );
    // Past a link to a directory the walk does not go: only `other` holds this.
    fs::create_dir_all(s.at("other")).unwrap();
    fs::write(s.at("other/far.png"), png(100, 0)).unwrap();
    fs::create_dir_all(s.at("linked")).unwrap();
    std::os::unix::fs::symlink(s.at("other"), s.at("linked/out")).unwrap();
    let r = tool("record-kitten-icat", &words(&["--scan", &s.at("linked")]));
    assert_eq!(r.out.lines().next(), Some("0 PNGs, 0 interlaced"));
    // Two roots over one tree count each file once.
    let r = tool(
        "record-kitten-icat",
        &words(&["--scan", &s.at("t"), &s.at("t/a")]),
    );
    assert_eq!(r.out.lines().next(), Some("2 PNGs, 1 interlaced"));

    fs::create_dir_all(s.at("empty")).unwrap();
    let r = tool("record-kitten-icat", &words(&["--scan", &s.at("empty")]));
    assert_eq!(r.code, Some(1));
    assert_eq!(r.out, "0 PNGs, 0 interlaced\n");
    let r = tool("record-kitten-icat", &words(&["--scan", &s.at("no-such")]));
    assert_eq!(r.code, Some(1));

    fs::create_dir_all(s.at("cut")).unwrap();
    fs::write(s.at("cut/cut.png"), &png(100, 0)[..20]).unwrap();
    let r = tool("record-kitten-icat", &words(&["--scan", &s.at("cut")]));
    assert_eq!(r.code, Some(1));
    assert_eq!(r.out, "");
}

/// `reply` answers no direct query on the silent mode: the arm above is not
/// a property of the stand-in.
#[test]
fn the_silent_modes_say_silent() {
    assert_eq!(kitten::mode_named("detect-da12").unwrap().3, Direct::Silent);
    assert_eq!(
        kitten::mode_named("detect-direct").unwrap().3,
        Direct::Answered
    );
    assert!(kitten::mode_named("other").is_none());
}

// record-shell-start

/// A shell that waits for its size, says what it was started with and exits
/// when told.
const FAKE_SHELL: &str = r#"#!/usr/bin/env -S bash --noprofile --norc
export HISTFILE=/dev/null
until [ "$(stty size)" != "0 0" ]; do :; done
stty size
printf '%s %s %s %s\n' "$TERM" "$COLORTERM" "$TERM_PROGRAM" "$PWD"
read line
"#;

/// Each read of a shell-start file.
fn parse_start(b: &[u8]) -> Vec<(f64, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < b.len() {
        let t = f64::from_le_bytes(b[at..at + 8].try_into().unwrap());
        let n = u32::from_le_bytes(b[at + 8..at + 12].try_into().unwrap());
        let n = usize::try_from(n).unwrap();
        out.push((t, b[at + 12..at + 12 + n].to_vec()));
        at += 12 + n;
    }
    out
}

#[test]
fn a_shell_start_records_each_read_with_its_time() {
    let s = Scratch::new("start");
    let shell = s.at("shell");
    exec::script(Path::new(&shell), FAKE_SHELL);
    let home = s.0.canonicalize().unwrap();
    let home = home.to_str().unwrap();
    let out = s.at("start.bin");
    let last = format!("xterm-256color truecolor baud {home}\r\n");
    let started = Instant::now();
    let r = tool_in(
        "record-shell-start",
        &words(&[&out, "--until", &last]),
        &[
            ("SHELL", &shell),
            ("HOME", home),
            ("TERM", "dumb"),
            ("COLORTERM", "other"),
            ("TERM_PROGRAM", "other"),
        ],
    );
    let took = started.elapsed().as_secs_f64();
    assert_eq!(r.code, Some(0), "{}", r.err);
    let reads = parse_start(&read(&out));
    let bytes: Vec<u8> = reads.iter().flat_map(|(_, b)| b.clone()).collect();
    let shown = String::from_utf8_lossy(&bytes);
    assert!(shown.starts_with("36 120\r\n"), "{shown}");
    assert!(shown.contains(&last), "{shown}");
    let times: Vec<f64> = reads.iter().map(|(t, _)| *t).collect();
    assert!(times.windows(2).all(|w| w[0] <= w[1]), "{times:?}");
    assert!(times.iter().all(|t| *t > 0.0 && *t < took), "{times:?}");
    let lines: Vec<&str> = r.out.lines().collect();
    assert_eq!(lines.len(), reads.len());
    for (line, (at, b)) in lines.iter().zip(&reads) {
        assert_eq!(*line, format!("{:9.1} ms {:6} bytes", at * 1e3, b.len()));
    }
}

/// **What a shell prints after the seconds is not recorded**: this shell
/// prints only once it has read a line, and the first line it is sent is
/// the `exit` that follows the seconds, so the file holds no read however
/// soon the shell starts.
#[test]
fn what_a_shell_prints_after_the_seconds_is_not_recorded() {
    let s = Scratch::new("start-late");
    let shell = s.at("shell");
    exec::script(
        Path::new(&shell),
        "#!/usr/bin/env -S bash --noprofile --norc\nexport HISTFILE=/dev/null\nread line\nprintf 'late\\n'\n",
    );
    let out = s.at("start.bin");
    let r = tool_in(
        "record-shell-start",
        &words(&[&out, "1"]),
        &[("SHELL", &shell), ("HOME", s.0.to_str().unwrap())],
    );
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(parse_start(&read(&out)).is_empty());
}

/// A home it cannot enter ends the child on its pty, with the reason there,
/// and the tool fails on that end before it sends `exit`, on Darwin and
/// Linux alike, not on the write, which only Darwin refuses.
#[test]
fn a_shell_start_with_no_home_fails_before_the_exit_it_sends() {
    let s = Scratch::new("start-home");
    let out = s.at("start.bin");
    let r = tool_in(
        "record-shell-start",
        &words(&[&out, "30"]),
        &[("SHELL", "/bin/sh"), ("HOME", "/no/such/home")],
    );
    assert_eq!(r.code, Some(1), "{}", r.err);
    assert!(r.err.starts_with("start: the shell ended"), "{}", r.err);
    assert!(!Path::new(&out).exists());
}

/// **A shell that hangs its terminal up and goes on running is killed after
/// 2 s**, and the tool says so: the `SHELL` is this binary as
/// [`as_a_shell_that_leaves_its_terminal`], which sleeps 20 s, and with no
/// bound the tool waits out the sleep and calls it an exit. Darwin's alone:
/// on Linux a read fails only once `readable` has seen the shell exit and
/// let the slave go, so no shell still running reaches the wait, and one
/// that hung its terminal up there is read until it ends or the
/// recording's seconds are up, then sent `exit` and killed after 2 s, as
/// the next test holds.
#[cfg(target_os = "macos")]
#[test]
fn a_shell_that_leaves_its_terminal_is_killed_within_the_bound() {
    let s = Scratch::new("start-left");
    let shell = s.at("shell");
    let me = std::env::current_exe().expect("this test binary");
    exec::script(
        Path::new(&shell),
        &format!(
            "#!/bin/sh\nexec '{}' --exact as_a_shell_that_leaves_its_terminal --test-threads=1\n",
            me.display()
        ),
    );
    let out = s.at("start.bin");
    let started = Instant::now();
    let r = tool_in(
        "record-shell-start",
        &words(&[&out, "30"]),
        &[
            ("SHELL", &shell),
            ("HOME", s.0.to_str().unwrap()),
            (LEAVES, "1"),
        ],
    );
    let took = started.elapsed();
    assert_eq!(r.code, Some(1), "{}", r.err);
    assert!(
        r.err
            .starts_with("start: the shell left its terminal without exiting"),
        "{}",
        r.err
    );
    assert!(took < Duration::from_secs(15), "{took:?}");
    assert!(!Path::new(&out).exists());
}

/// **On Linux a shell that hangs its terminal up and goes on running is
/// recorded for the seconds asked, then sent `exit` and killed after 2 s**:
/// `TIOCNOTTY` leaves the shell's descriptors on the terminal, so no read
/// fails, and the read loop stops at the recording's 1 s. The tool ends on
/// its ordinary path, exit 0 with the file written, well before the
/// helper's 20 s, and the helper is reaped, its pid gone.
#[cfg(target_os = "linux")]
#[test]
fn a_shell_that_hangs_its_terminal_up_is_recorded_for_the_seconds_asked() {
    let s = Scratch::new("start-hung");
    let shell = s.at("shell");
    let me = std::env::current_exe().expect("this test binary");
    exec::script(
        Path::new(&shell),
        &format!(
            "#!/bin/sh\nexec '{}' --exact as_a_shell_that_leaves_its_terminal --test-threads=1\n",
            me.display()
        ),
    );
    let out = s.at("start.bin");
    let started = Instant::now();
    let r = tool_in(
        "record-shell-start",
        &words(&[&out, "1"]),
        &[
            ("SHELL", &shell),
            ("HOME", s.0.to_str().unwrap()),
            (LEAVES, "1"),
        ],
    );
    let took = started.elapsed();
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(Path::new(&out).exists(), "the recording");
    assert!(took < Duration::from_secs(6), "{took:?}");
    let pid = fs::read_to_string(s.at("leaves.pid")).expect("the helper ran");
    assert!(
        !Path::new(&format!("/proc/{}", pid.trim())).exists(),
        "the helper, pid {pid}, is reaped"
    );
}

/// `os.execve` looks along no `PATH`: a `SHELL` with no directory is a name in
/// the directory it runs in, so `sh` does not start, and the tool fails.
#[test]
fn a_shell_with_no_directory_is_not_looked_for() {
    let s = Scratch::new("start-bare");
    let out = s.at("start.bin");
    let r = tool_in(
        "record-shell-start",
        &words(&[&out, "30"]),
        &[
            ("SHELL", "sh"),
            ("HOME", s.0.to_str().unwrap()),
            ("PATH", "/usr/bin:/bin"),
        ],
    );
    assert_eq!(r.code, Some(1), "{}", r.err);
    assert!(r.err.starts_with("start: the shell ended"), "{}", r.err);
    assert!(!Path::new(&out).exists());
}

#[test]
fn the_shell_start_tool_dies_where_the_script_died() {
    let s = Scratch::new("start-die");
    let r = tool("record-shell-start", &[]);
    assert_eq!(r.code, Some(1));
    assert!(
        r.err.contains("Usage: record-shell-start <out> [seconds]"),
        "{}",
        r.err
    );
    let r = tool("record-shell-start", &words(&[&s.at("o"), "abc"]));
    assert_eq!(r.code, Some(1));
    assert_eq!(r.err, "could not convert string to float: 'abc'\n");
    for args in [
        words(&[&s.at("o"), "--until"]),
        words(&[&s.at("o"), "--until", ""]),
    ] {
        let r = tool("record-shell-start", &args);
        assert_eq!(r.code, Some(1));
        assert!(
            r.err.contains("record-shell-start <out> --until <text>"),
            "{}",
            r.err
        );
    }
}

// The generators' command lines.

#[test]
fn the_marks_payload_tool_writes_and_refuses() {
    let s = Scratch::new("marks");
    let out = s.at("m.raw");
    let r = tool("gen-marks-payload", &words(&["covered", &out, "400"]));
    assert_eq!(r.code, Some(0), "{}", r.err);
    let bytes = read(&out);
    assert_eq!(r.out, format!("{} bytes, 2 lines\n", bytes.len()));
    assert!(bytes.ends_with(b"\r\n"));
    // The size it is given when it is given none.
    let r = tool("gen-marks-payload", &words(&["covered", &out]));
    assert_eq!(
        (r.code, r.out.as_str()),
        (Some(0), "1048653 bytes, 3297 lines\n")
    );
    let r = tool("gen-marks-payload", &words(&["marks", &out, "0"]));
    assert_eq!((r.code, r.out.as_str()), (Some(0), "0 bytes, 0 lines\n"));
    assert_eq!(read(&out), b"");
    for args in [
        vec![],
        vec!["marks"],
        vec!["other", "x"],
        vec!["marks", "x", "1", "2"],
    ] {
        let r = tool("gen-marks-payload", &words(&args));
        assert_eq!(r.code, Some(1), "{args:?}");
        assert!(r.err.starts_with("usage: gen-marks-payload"), "{}", r.err);
    }
    let r = tool("gen-marks-payload", &words(&["marks", &out, "12x"]));
    assert_eq!(r.code, Some(1));
    let r = tool(
        "gen-marks-payload",
        &words(&["marks", &s.at("no/such/m"), "1"]),
    );
    assert_eq!(r.code, Some(1));
}

#[test]
fn the_vim_file_tool_writes_and_refuses() {
    let s = Scratch::new("vim");
    let out = s.at("u.txt");
    let r = tool("gen-vim-unicode", &words(&[&out]));
    assert_eq!((r.code, r.out.as_str()), (Some(0), ""));
    assert_eq!(read(&out), vim_unicode::contents());
    assert!(!read(&out).ends_with(b"\n"), "no final newline");
    let r = tool("gen-vim-unicode", &[]);
    assert_eq!(r.code, Some(2));
    let r = tool("gen-vim-unicode", &words(&["a", "b"]));
    assert_eq!(r.code, Some(2));
    let r = tool("gen-vim-unicode", &words(&[&s.at("no/such/u")]));
    assert_eq!(r.code, Some(1));
}

#[test]
fn the_yazi_files_tool_refuses_a_wrong_count() {
    for args in [vec![], vec!["a", "b"]] {
        let r = tool("gen-yazi-files", &words(&args));
        assert_eq!(r.code, Some(1));
        assert!(r.err.contains("gen-yazi-files <dir>"), "{}", r.err);
    }
}

/// The whole tool, `sips` and all: on macOS, where the script ran.
#[cfg(target_os = "macos")]
#[test]
fn the_yazi_files_tool_writes_the_seven_and_nothing_else() {
    let s = Scratch::new("yazi-files");
    let dir = s.at("files");
    let r = tool("gen-yazi-files", &words(&[&dir]));
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(
        r.out,
        "['alpha.png', 'flat.png', 'photo.jpg', 'screenshot.png', 'small.png', 'sparse.pdf', 'text.pdf']\n"
    );
    let jpeg = read(&format!("{dir}/photo.jpg"));
    assert_eq!(&jpeg[..2], b"\xff\xd8");
    assert_eq!(&jpeg[jpeg.len() - 2..], b"\xff\xd9");
    // A file that was there is listed too, as `os.listdir` lists it.
    fs::write(format!("{dir}/zz"), "x").unwrap();
    let r = tool("gen-yazi-files", &words(&[&dir]));
    assert!(r.out.ends_with("'text.pdf', 'zz']\n"), "{}", r.out);
}

/// Where `sips` is not, the tool fails where the script failed.
#[cfg(not(target_os = "macos"))]
#[test]
fn the_yazi_files_tool_fails_without_sips() {
    let s = Scratch::new("yazi-files");
    let r = tool("gen-yazi-files", &words(&[&s.at("files")]));
    assert_eq!(r.code, Some(1));
}
