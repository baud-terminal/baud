//! A child on a fresh pty, as Python's `pty.fork` started one and its `os`
//! calls drove it, waited on with `select` as the scripts' `select.select`
//! waited.

use std::ffi::{CStr, CString, OsStr, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::sync::{Once, OnceLock};
use std::time::Instant;

/// Seconds on one monotonic clock, as `time.monotonic()`.
pub fn clock() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// The locales `_Py_CoerceLegacyLocale` tries, in its order.
const TARGETS: [&CStr; 3] = [c"C.UTF-8", c"C.utf8", c"UTF-8"];

/// PEP 538, as Python's start-up did it before any script's first line:
/// where `LC_ALL` is unset or empty and `LC_CTYPE` resolves to C, this
/// process's `LC_CTYPE` becomes the first target `setlocale` takes whose
/// `nl_langinfo(CODESET)` is not empty, replacing a value already there. So a
/// child sees it, on the pty or off it, and a program started with no locale
/// draws UTF-8 as it did under the script. `-I` was how the scripts were
/// compared, and under it `PYTHONCOERCECLOCALE` is not read.
///
/// libc resolves the locale because what a name resolves to is its answer:
/// Darwin's `POSIX` is not C, and a name it does not know is.
pub fn coerce_locale() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if std::env::var_os("LC_ALL").is_some_and(|v| !v.is_empty()) {
            return;
        }
        if let Some(target) = coerced_ctype() {
            std::env::set_var("LC_CTYPE", target);
        }
    });
}

/// What `LC_CTYPE` becomes, this process's own locale left at C, where Rust
/// started it.
fn coerced_ctype() -> Option<&'static str> {
    // SAFETY: C strings live for each call and the answer is read before the
    // next. Only `coerce_locale` calls this, once, and a tool has no other
    // thread reading the locale or the environment then.
    unsafe {
        libc::setlocale(libc::LC_CTYPE, c"".as_ptr());
        let now = libc::setlocale(libc::LC_CTYPE, std::ptr::null());
        let mut found = None;
        if !now.is_null() && CStr::from_ptr(now) == c"C" {
            for target in TARGETS {
                if libc::setlocale(libc::LC_CTYPE, target.as_ptr()).is_null() {
                    continue;
                }
                let codeset = libc::nl_langinfo(libc::CODESET);
                if !codeset.is_null() && *codeset != 0 {
                    found = target.to_str().ok();
                    break;
                }
                libc::setlocale(libc::LC_CTYPE, c"".as_ptr());
            }
        }
        libc::setlocale(libc::LC_CTYPE, c"C".as_ptr());
        found
    }
}

/// An `os.environ`: the environment a child is given.
#[derive(Clone, Debug)]
pub struct Environ(Vec<(OsString, OsString)>);

impl Environ {
    /// This process's, its locale coerced as Python's start-up coerced it.
    pub fn current() -> Environ {
        coerce_locale();
        Environ(std::env::vars_os().collect())
    }

    /// `environ[name]`.
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_os_str())
    }

    /// `environ[name] = value`.
    pub fn set(&mut self, name: &str, value: &str) {
        match self.0.iter_mut().find(|(k, _)| k == name) {
            Some((_, v)) => *v = value.into(),
            None => self.0.push((name.into(), value.into())),
        }
    }

    /// `environ.setdefault(name, value)`.
    pub fn set_default(&mut self, name: &str, value: &str) {
        if self.get(name).is_none() {
            self.set(name, value);
        }
    }

    fn block(&self) -> Vec<CString> {
        self.0
            .iter()
            .filter_map(|(k, v)| {
                let mut line = k.clone();
                line.push("=");
                line.push(v);
                CString::new(line.into_vec()).ok()
            })
            .collect()
    }
}

/// How the program is looked for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lookup {
    /// `os.execvp`: along `PATH`, unless the name has a directory.
    Path,
    /// `os.execv`: the name as given.
    Direct,
}

/// What a child runs, made before the fork so the child allocates nothing.
pub struct Spawn {
    /// The paths to try, in order.
    tries: Vec<CString>,
    argv: Vec<CString>,
    envp: Vec<CString>,
    dir: Option<CString>,
    label: CString,
    code: libc::c_int,
}

fn c(s: &[u8]) -> Result<CString, String> {
    CString::new(s).map_err(|_| "embedded null byte".to_string())
}

/// How `sys.stderr` showed a name: `surrogateescape` read each byte UTF-8 does
/// not take as a surrogate, and `backslashreplace` wrote it as `\\udcXX`.
fn shown(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len());
    for chunk in name.utf8_chunks() {
        out.extend_from_slice(chunk.valid().as_bytes());
        for b in chunk.invalid() {
            out.extend_from_slice(format!("\\udc{b:02x}").as_bytes());
        }
    }
    out
}

/// `os.path.join(dir, file)`.
fn join(dir: &[u8], file: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.is_empty() && !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(file);
    p
}

impl Spawn {
    /// `argv` run with `env`, its first word found by `lookup`, or `argv0`
    /// in its place when given; failing to start prints a line naming
    /// `argv[0]` to the pty and exits `code`.
    pub fn new(
        lookup: Lookup,
        argv: &[OsString],
        argv0: Option<&OsStr>,
        env: &Environ,
        code: libc::c_int,
    ) -> Result<Spawn, String> {
        let file = argv[0].as_bytes();
        let mut tries = Vec::new();
        let named = file.contains(&b'/');
        if lookup == Lookup::Direct || named {
            tries.push(c(file)?);
        } else {
            let path = env
                .get("PATH")
                .map_or(b"/bin:/usr/bin".to_vec(), |p| p.as_bytes().to_vec());
            for dir in path.split(|b| *b == b':') {
                tries.push(c(&join(dir, file))?);
            }
        }
        let mut words = Vec::with_capacity(argv.len());
        words.push(c(argv0.unwrap_or(&argv[0]).as_bytes())?);
        for a in &argv[1..] {
            words.push(c(a.as_bytes())?);
        }
        Ok(Spawn {
            tries,
            argv: words,
            envp: env.block(),
            dir: None,
            label: c(&shown(file))?,
            code,
        })
    }

    /// `os.chdir(dir)` in the child, first; failing prints a line naming `dir`.
    pub fn in_dir(mut self, dir: &OsStr) -> Result<Spawn, String> {
        self.dir = Some(c(dir.as_bytes())?);
        Ok(self)
    }

    /// The child's side: no allocation, only what is safe after a fork.
    fn exec(&self, argv: &[*const libc::c_char], envp: &[*const libc::c_char]) -> ! {
        // Python's start-up ignores this, `subprocess` puts it back for what
        // it runs, and `pty.fork` does not.
        // SAFETY: async-signal-safe, with a disposition and no pointer.
        unsafe {
            libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        }
        if let Some(dir) = &self.dir {
            // SAFETY: a valid C string, for the call.
            if unsafe { libc::chdir(dir.as_ptr()) } != 0 {
                self.report(b"chdir ", dir, errno());
            }
        }
        let mut saved = None;
        let mut last = None;
        for path in &self.tries {
            // SAFETY: the path and both null-terminated arrays outlive the
            // call, which does not return on success.
            unsafe {
                libc::execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr());
            }
            let e = errno();
            last = Some(e);
            if e != libc::ENOENT && e != libc::ENOTDIR && saved.is_none() {
                saved = last;
            }
        }
        let e = saved.or(last).expect("at least one try");
        self.report(b"exec ", &self.label, e)
    }

    /// What Python's `print(f"exec {name}: {e}")` writes, then `_exit`:
    /// `os.execv`'s error names no file, so neither does this.
    fn report(&self, verb: &[u8], name: &CStr, e: libc::c_int) -> ! {
        let mut digits = [0u8; 12];
        let mut at = digits.len();
        let mut v = e;
        while v > 0 || at == digits.len() {
            at -= 1;
            digits[at] = b'0' + u8::try_from(v % 10).expect("a digit");
            v /= 10;
        }
        // SAFETY: strerror returns a C string; the writes take live slices.
        unsafe {
            let msg = std::ffi::CStr::from_ptr(libc::strerror(e));
            for part in [
                verb,
                name.to_bytes(),
                b": [Errno ",
                &digits[at..],
                b"] ",
                msg.to_bytes(),
                b"\n",
            ] {
                libc::write(2, part.as_ptr().cast(), part.len());
            }
            libc::_exit(self.code)
        }
    }
}

fn pointers(words: &[CString]) -> Vec<*const libc::c_char> {
    words
        .iter()
        .map(|w| w.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect()
}

fn errno() -> libc::c_int {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Darwin's answer when a pty open lost the kernel's redrive race.
#[cfg(target_os = "macos")]
const EREDRIVEOPEN: libc::c_int = -6;

/// A `forkpty` that lost the kernel's allocation race has not forked and is
/// made again, as the app's is (`baud-app/src/pty.rs`).
#[cfg(target_os = "macos")]
fn raced() -> bool {
    errno() == EREDRIVEOPEN
}

/// Linux's pty open has no such race.
#[cfg(not(target_os = "macos"))]
fn raced() -> bool {
    false
}

/// The parent's side of a child on a pty.
pub struct Child {
    /// The child's process id.
    pub pid: libc::pid_t,
    /// The pty's master.
    pub fd: libc::c_int,
}

impl Child {
    /// `pty.fork()`, then the program, on a pty with no size yet: the
    /// scripts set it after the fork.
    pub fn fork(spawn: &Spawn) -> Result<Child, String> {
        let argv = pointers(&spawn.argv);
        let envp = pointers(&spawn.envp);
        let mut master: libc::c_int = -1;
        let mut calls = 0;
        let pid = loop {
            calls += 1;
            // SAFETY: a live out-pointer, and null for the three it may take
            // as null.
            let got = unsafe {
                libc::forkpty(
                    &mut master,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if got >= 0 || calls == 16 || !raced() {
                break got;
            }
        };
        if pid < 0 {
            return Err(format!("forkpty: {}", io::Error::last_os_error()));
        }
        if pid == 0 {
            spawn.exec(&argv, &envp);
        }
        Ok(Child { pid, fd: master })
    }

    /// `fcntl.ioctl(fd, TIOCSWINSZ, struct.pack("HHHH", ...))`: a number that
    /// does not fit is `struct.error`.
    pub fn set_size(&self, cols: i64, rows: i64, xpixel: i64, ypixel: i64) -> Result<(), String> {
        let field = |v: i64| {
            u16::try_from(v).map_err(|_| "ushort format requires 0 <= number <= 65535".to_string())
        };
        let ws = libc::winsize {
            ws_row: field(rows)?,
            ws_col: field(cols)?,
            ws_xpixel: field(xpixel)?,
            ws_ypixel: field(ypixel)?,
        };
        // SAFETY: a live winsize for the call.
        if unsafe { libc::ioctl(self.fd, libc::TIOCSWINSZ, &raw const ws) } != 0 {
            return Err(format!("TIOCSWINSZ: {}", io::Error::last_os_error()));
        }
        Ok(())
    }

    /// `select.select([fd], [], [], seconds)`: whether a read would not
    /// block. A timeout `select` could not take is its `OverflowError`.
    pub fn readable(&self, seconds: f64) -> Result<bool, String> {
        let until = clock() + seconds;
        loop {
            let left = (until - clock()).max(0.0);
            if left.is_nan() || left * 1e9 >= 9.2e18 {
                return Err("timeout is too large".to_string());
            }
            let mut tv = libc::timeval {
                tv_sec: left.trunc() as libc::time_t,
                tv_usec: (left.fract() * 1e6).ceil().min(999_999.0) as libc::suseconds_t,
            };
            // SAFETY: an fd_set zeroed and then set, and a live timeout, both
            // for the call.
            let (got, set) = unsafe {
                let mut set: libc::fd_set = std::mem::zeroed();
                libc::FD_ZERO(&raw mut set);
                libc::FD_SET(self.fd, &raw mut set);
                let got = libc::select(
                    self.fd + 1,
                    &raw mut set,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &raw mut tv,
                );
                (got, set)
            };
            if got < 0 {
                if errno() == libc::EINTR {
                    continue;
                }
                return Err(format!("select: {}", io::Error::last_os_error()));
            }
            // SAFETY: `set` is the one select filled in.
            return Ok(got > 0 && unsafe { libc::FD_ISSET(self.fd, &raw const set) });
        }
    }

    /// `os.read(fd, cap)`.
    pub fn read(&self, cap: usize) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; cap];
        loop {
            // SAFETY: `cap` writable bytes, for the call.
            let n = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), cap) };
            if let Ok(n) = usize::try_from(n) {
                buf.truncate(n);
                return Ok(buf);
            }
            if errno() != libc::EINTR {
                return Err(io::Error::last_os_error());
            }
        }
    }

    /// `os.write(fd, data)`, one call, whatever it took: what the scripts
    /// did, for writes the size of a key.
    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        loop {
            // SAFETY: `data` is live for the call.
            let n = unsafe { libc::write(self.fd, data.as_ptr().cast(), data.len()) };
            if n >= 0 {
                return Ok(());
            }
            if errno() != libc::EINTR {
                return Err(format!("write: {}", io::Error::last_os_error()));
            }
        }
    }

    /// `os.kill(pid, sig)`.
    pub fn kill(&self, sig: libc::c_int) -> io::Result<()> {
        // SAFETY: only a pid and a number.
        if unsafe { libc::kill(self.pid, sig) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// `SIGTERM`, 0.3 s, `SIGKILL`, and a process already gone is no failure
    /// at either step, as the scripts' `ProcessLookupError` was not.
    pub fn stop(&self) -> Result<(), String> {
        self.kill(libc::SIGTERM)
            .and_then(|()| {
                std::thread::sleep(std::time::Duration::from_millis(300));
                self.kill(libc::SIGKILL)
            })
            .or_else(|e| {
                if e.raw_os_error() == Some(libc::ESRCH) {
                    Ok(())
                } else {
                    Err(format!("kill: {e}"))
                }
            })
    }

    /// `os.waitpid(pid, 0)`, its status as `waitstatus_to_exitcode` gives it:
    /// the exit code, or minus the signal.
    pub fn wait(&self) -> Result<i32, String> {
        let mut status = 0;
        loop {
            // SAFETY: a live out-pointer for the call.
            let got = unsafe { libc::waitpid(self.pid, &raw mut status, 0) };
            if got >= 0 {
                break;
            }
            if errno() != libc::EINTR {
                return Err(format!("waitpid: {}", io::Error::last_os_error()));
            }
        }
        if libc::WIFSIGNALED(status) {
            Ok(-libc::WTERMSIG(status))
        } else {
            Ok(libc::WEXITSTATUS(status))
        }
    }

    /// `os.waitpid(pid, os.WNOHANG)`, the answer dropped.
    pub fn reap(&self) {
        let mut status = 0;
        // SAFETY: a live out-pointer for the call.
        unsafe {
            libc::waitpid(self.pid, &raw mut status, libc::WNOHANG);
        }
    }

    /// `os.close(fd)`.
    pub fn close(&self) {
        // SAFETY: the master, closed once, by the tool that opened it.
        unsafe {
            libc::close(self.fd);
        }
    }
}
