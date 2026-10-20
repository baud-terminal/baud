//! **xterm's logging, written** (`?46`, "Start logging", and `OSC 46`,
//! "Change Log File to Pt", `[[source: ctlseqs]]`; the core keeps the mode
//! and the name, `baud-core`'s `logging.rs`). The owner's call of
//! 2026-10-20, on by default and `[screen] logging = "off"` refusing both.
//!
//! **The host's bytes, as xterm logs them**: each part of a read the core
//! parsed while the mode was started, the core stopping a read at each
//! change so a part is parsed under one mode (`reads::feed`). So the log
//! runs from the byte after `?46 h` to the end of `?46 l`.
//!
//! **Where**: the name `OSC 46` gave; `-` is baud's standard output, as
//! xterm's; a relative name is read from the directory the shell started
//! in, and with no name the log is `baud.log.<host>.<date>.<pid>` there,
//! after xterm's `Xterm.log.<host>.<date>.<pid>`, the date and time in UTC
//! (`pty::start_dir`; Jev's leans, START_DIR and GENERATED_START_DIR). The
//! file is appended to, created `0644` if absent, and refused unless the
//! real user owns it and neither group nor others may write it, as xterm's
//! `open_userfile` (`[[source: xterm-misc]]`; Jev, XTERM_CHECKS,
//! 1.00/1.00).
//!
//! **Never on the read loop**: the open and every write are a writer
//! thread's, fed through a queue the loop only pushes to, so a stalled disk
//! or a FIFO with a slow reader waits on that thread alone. The open is
//! non-blocking, so a FIFO with no reader fails at once rather than waits.
//! What the queue holds is bounded at [`QUEUED_MAX`] bytes; past it a part
//! is dropped and warned once. When the writer cannot open or write, the
//! core's log is stopped (`Term::stop_log`), so `DECRQM ?46` says so, and
//! the reason is warned in baud's own log.
//!
//! **Nothing is lost when the session ends**: where the shell's exit ends
//! the app, or the window's close does on Linux, the loop asks the writer
//! to finish ([`HostLog::quit`]) and ends when it says it has, everything
//! before written and the file closed, as xterm's `FlushLog` has written
//! each read before the next. The writer wakes the loop to say so; a sink
//! that stalls is given up on after [`QUIT_MAX`], on the loop's clock.
//! A name made for a start with none is kept for the next, as xterm's
//! `GenerateLogPath` keeps it: "once opened we just reuse the same log
//! name" (`[[source: xterm-misc]]`).

#![allow(unsafe_code)]

use baud_core::{LogMode, Term};
use std::ffi::OsStr;
use std::io::Write as _;
use std::os::fd::AsRawFd as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant, SystemTime};

/// The most bytes the writer may have queued before a part is dropped:
/// four MiB, a thousand of the largest reads.
pub const QUEUED_MAX: usize = 4 << 20;

/// How long a quit keeps the app for a writer still writing: two
/// seconds, the paste's patience with a silent owner. A local disk takes
/// the whole queue in milliseconds; a pipe whose reader stopped never
/// would, and the app must still end.
pub const QUIT_MAX: Duration = Duration::from_secs(2);

/// Whether the log has finished: everything it was given written and the
/// file closed, or given up on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Finished {
    Yes,
    No,
}

/// What the core's log is written through, and what the feed does at each
/// part of a read (`reads::feed`).
pub trait Tee {
    /// A part of a read the core parsed while a log was started.
    fn take(&mut self, part: &[u8]);
    /// The core after a part: open or close as `?46` left it, and stop the
    /// core's log when the writer could not log.
    fn follow(&mut self, term: &mut Term<'_>);
}

/// No log: a test that writes none.
#[cfg(test)]
pub struct NoLog;

#[cfg(test)]
impl Tee for NoLog {
    fn take(&mut self, _: &[u8]) {}
    fn follow(&mut self, _: &mut Term<'_>) {}
}

/// Where a started log goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    File(PathBuf),
    Stdout,
}

/// The target of a start: `-` standard output, an absolute name itself, a
/// relative one in `dir`, and none the generated name in `dir`.
pub fn target(name: Option<&[u8]>, dir: &Path, generated: impl FnOnce() -> String) -> Target {
    match name {
        Some(b"-") => Target::Stdout,
        Some(n) => {
            let p = Path::new(OsStr::from_bytes(n));
            match p.is_absolute() {
                true => Target::File(p.to_path_buf()),
                false => Target::File(dir.join(p)),
            }
        }
        None => Target::File(dir.join(generated())),
    }
}

/// xterm's generated form, `<host>.<yyyy.mm.dd.hh.mm.ss>.<pid>` after
/// `baud.log.`, at `since_epoch` in UTC.
pub fn generated_name(host: &str, since_epoch: Duration, pid: Pid) -> String {
    let secs = since_epoch.as_secs();
    let (day, rest) = (secs / 86_400, secs % 86_400);
    // Days to a civil date, Howard Hinnant's `civil_from_days` for days
    // since 1970-01-01, which is never negative here.
    let z = day + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    format!(
        "baud.log.{host}.{y:04}.{m:02}.{d:02}.{:02}.{:02}.{:02}.{}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        pid.0
    )
}

/// A process id, as the generated name carries it.
#[derive(Clone, Copy)]
pub struct Pid(pub u32);

/// This host's name, or `localhost` when the system gives none.
fn host() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is `buf.len()` bytes and outlives the call.
    let r =
        unsafe { crate::sys::gethostname(buf.as_mut_ptr().cast(), crate::sys::NameLen(buf.len())) };
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    match (r, &buf[..end]) {
        (0, name) if !name.is_empty() => String::from_utf8_lossy(name).into_owned(),
        _ => "localhost".to_string(),
    }
}

/// Which open a message or a failure belongs to, so a failure of an open
/// the core has since stopped does not stop the next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Epoch(u64);

enum Msg {
    Open(Epoch, Target),
    Part(Vec<u8>),
    Close,
    /// The session's end: answered on the sender, then the callback run,
    /// once every message before it is done.
    Quit(Sender<()>, Box<dyn FnOnce() + Send>),
    /// Answered once every message before it is done: a test's wait.
    #[cfg(test)]
    Sync(Sender<()>),
}

/// Whether the writer has been told to open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Opened {
    Open,
    Closed,
}

/// Whether the parts dropped since the log opened were said.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Said {
    Yes,
    No,
}

struct Writer {
    tx: Sender<Msg>,
    failed: Receiver<(Epoch, String)>,
    queued: Arc<AtomicUsize>,
}

/// Where the session's end stands with the writer.
enum Quitting {
    Not,
    /// Asked; the writer answers on `done`, and the app ends anyway at `by`.
    Asked {
        by: Instant,
        done: Receiver<()>,
    },
    Done,
}

/// The app's log: the directory a start without an absolute name goes to,
/// the writer, made at the first start, and the name made for a start with
/// none, kept for the next.
pub struct HostLog {
    dir: PathBuf,
    writer: Option<Writer>,
    opened: Opened,
    epoch: Epoch,
    limit: Limit,
    dropped: Dropped,
    said: Said,
    generated: Option<String>,
    namer: fn() -> String,
    quitting: Quitting,
}

/// How many bytes the queue may hold ([`QUEUED_MAX`] but in a test).
#[derive(Clone, Copy)]
struct Limit(usize);

/// The bytes dropped because the queue was full.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Dropped(usize);

impl HostLog {
    pub fn new(dir: PathBuf) -> HostLog {
        HostLog::bounded(dir, Limit(QUEUED_MAX))
    }

    fn bounded(dir: PathBuf, limit: Limit) -> HostLog {
        HostLog {
            dir,
            writer: None,
            opened: Opened::Closed,
            epoch: Epoch(0),
            limit,
            dropped: Dropped(0),
            said: Said::No,
            generated: None,
            namer: generate,
            quitting: Quitting::Not,
        }
    }

    /// **The session's end**: ask the writer, once, to write what it holds
    /// and close the file, then run `woken` so the loop looks again; say
    /// whether it has. Finished at once with no writer, and at [`QUIT_MAX`]
    /// after the ask whatever the writer is doing, warned. The loop calls
    /// it at each turn until finished: a later call's `woken` is dropped.
    pub fn quit(&mut self, now: Instant, woken: impl FnOnce() + Send + 'static) -> Finished {
        if let Quitting::Not = self.quitting {
            self.opened = Opened::Closed;
            self.quitting = match &self.writer {
                None => Quitting::Done,
                Some(w) => {
                    let (tx, done) = channel();
                    let _ = w.tx.send(Msg::Close);
                    let _ = w.tx.send(Msg::Quit(tx, Box::new(woken)));
                    Quitting::Asked {
                        by: now + QUIT_MAX,
                        done,
                    }
                }
            };
            return match self.quitting {
                Quitting::Done => Finished::Yes,
                _ => Finished::No,
            };
        }
        if let Quitting::Asked { by, done } = &self.quitting {
            match done.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) if now < *by => return Finished::No,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    let held = self
                        .writer
                        .as_ref()
                        .map_or(0, |w| w.queued.load(Ordering::Acquire));
                    crate::warn!(
                        "log: the writer had not finished {QUIT_MAX:?} after the session ended; \
                         up to {held} bytes are not in the log"
                    );
                }
                // Answered, or a writer that is gone: nothing more will be
                // written either way.
                Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
            }
            self.quitting = Quitting::Done;
        }
        Finished::Yes
    }

    /// When a quit asked of the writer is given up on, for a loop that
    /// waits on a timeout.
    #[cfg(target_os = "linux")]
    pub fn quit_by(&self) -> Option<Instant> {
        match self.quitting {
            Quitting::Asked { by, .. } => Some(by),
            _ => None,
        }
    }

    /// The bytes dropped so far because the writer fell behind.
    #[cfg(test)]
    fn dropped(&self) -> Dropped {
        self.dropped
    }

    fn writer(&mut self) -> &Writer {
        self.writer.get_or_insert_with(|| {
            let (tx, rx) = channel();
            let (failed_tx, failed) = channel();
            let queued = Arc::new(AtomicUsize::new(0));
            let q = Arc::clone(&queued);
            // Detached: it ends when the log is dropped and its queue is
            // empty. Nothing joins it; a quit waits for its answer on the
            // loop's turns (`quit`).
            let _ = std::thread::spawn(move || write_log(rx, failed_tx, q));
            Writer { tx, failed, queued }
        })
    }

    /// Wait for the writer to do what it was sent, then follow the core: a
    /// test's way to see a failure, which arrives from the writer.
    #[cfg(test)]
    fn settle(&mut self, term: &mut Term<'_>) {
        self.wait();
        self.follow(term);
    }

    #[cfg(test)]
    fn wait(&self) {
        if let Some(w) = &self.writer {
            let (done, wait) = channel();
            let _ = w.tx.send(Msg::Sync(done));
            let _ = wait.recv();
        }
    }

    /// Quit as the app does and wait for the writer's answer: for a test,
    /// which reads the file after.
    #[cfg(test)]
    fn finish(mut self) {
        let (tx, answered) = channel::<()>();
        if self.quit(Instant::now(), move || drop(tx)) == Finished::No {
            // The callback's sender dropped: the writer has run it.
            let _ = answered.recv();
            assert_eq!(self.quit(Instant::now(), || {}), Finished::Yes);
        }
    }
}

impl Tee for HostLog {
    fn take(&mut self, part: &[u8]) {
        if part.is_empty() || self.opened == Opened::Closed || !self.live() {
            return;
        }
        let limit = self.limit.0;
        let w = self.writer();
        let held = w.queued.load(Ordering::Acquire);
        if held + part.len() > limit {
            self.dropped = Dropped(self.dropped.0 + part.len());
            if self.said == Said::No {
                self.said = Said::Yes;
                crate::warn!("log: the writer fell behind; output is being dropped from the log");
            }
            return;
        }
        w.queued.fetch_add(part.len(), Ordering::AcqRel);
        let _ = w.tx.send(Msg::Part(part.to_vec()));
    }

    fn follow(&mut self, term: &mut Term<'_>) {
        if !self.live() {
            return;
        }
        if let Some(w) = &self.writer {
            while let Ok((epoch, why)) = w.failed.try_recv() {
                crate::warn!("log: {why}");
                if epoch == self.epoch && self.opened == Opened::Open {
                    self.opened = Opened::Closed;
                    term.stop_log();
                }
            }
        }
        match (term.log_mode(), self.opened) {
            (LogMode::Started, Opened::Closed) => {
                // Made at the first start with no name and kept, as xterm's
                // `GenerateLogPath`.
                let (kept, namer) = (&mut self.generated, self.namer);
                let t = target(term.log_file(), &self.dir, || {
                    kept.get_or_insert_with(namer).clone()
                });
                self.epoch = Epoch(self.epoch.0 + 1);
                self.opened = Opened::Open;
                self.said = Said::No;
                let epoch = self.epoch;
                let _ = self.writer().tx.send(Msg::Open(epoch, t));
            }
            (LogMode::Stopped, Opened::Open) => {
                self.opened = Opened::Closed;
                let _ = self.writer().tx.send(Msg::Close);
            }
            _ => {}
        }
    }
}

impl HostLog {
    /// Whether the session is still going: nothing opens or is queued once
    /// a quit was asked.
    fn live(&self) -> bool {
        matches!(self.quitting, Quitting::Not)
    }
}

/// The generated name now, xterm's form.
fn generate() -> String {
    let since = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    generated_name(&host(), since, Pid(std::process::id()))
}

/// Where the writer writes.
enum Sink {
    File(std::fs::File),
    Stdout,
}

impl Sink {
    fn write_all(&mut self, part: &[u8]) -> std::io::Result<()> {
        match self {
            Sink::File(f) => f.write_all(part),
            // Flushed at each part: Rust's standard output holds a line.
            Sink::Stdout => {
                let mut out = std::io::stdout();
                out.write_all(part).and_then(|()| out.flush())
            }
        }
    }
}

/// The writer thread: each message in turn, until the log is dropped.
fn write_log(rx: Receiver<Msg>, failed: Sender<(Epoch, String)>, queued: Arc<AtomicUsize>) {
    let mut out: Option<(Epoch, Sink)> = None;
    for m in rx {
        match m {
            Msg::Open(epoch, t) => {
                out = match open(&t) {
                    Ok(sink) => Some((epoch, sink)),
                    Err(why) => {
                        let _ = failed.send((epoch, why));
                        None
                    }
                }
            }
            Msg::Part(part) => {
                queued.fetch_sub(part.len(), Ordering::AcqRel);
                if let Some((epoch, sink)) = out.as_mut() {
                    if let Err(e) = sink.write_all(&part) {
                        let _ = failed.send((*epoch, format!("writing the log failed: {e}")));
                        out = None;
                    }
                }
            }
            Msg::Close => out = None,
            Msg::Quit(done, woken) => {
                out = None;
                let _ = done.send(());
                woken();
            }
            #[cfg(test)]
            Msg::Sync(done) => {
                let _ = done.send(());
            }
        }
    }
}

/// Open a target with xterm's checks: appended, created `0644`, opened
/// without blocking, then refused unless the real user owns it and no one
/// else may write it. Once open, the descriptor blocks again, on this
/// thread alone.
fn open(t: &Target) -> Result<Sink, String> {
    let path = match t {
        Target::Stdout => return Ok(Sink::Stdout),
        Target::File(p) => p,
    };
    let shown = path.display();
    let f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o644)
        .custom_flags(crate::sys::O_NONBLOCK)
        .open(path)
        .map_err(|e| format!("cannot open {shown}: {e}"))?;
    let meta = f
        .metadata()
        .map_err(|e| format!("cannot read {shown}: {e}"))?;
    // SAFETY: `getuid` takes nothing and cannot fail.
    if meta.uid() != unsafe { crate::sys::getuid() } {
        return Err(format!("{shown} is not yours"));
    }
    if meta.mode() & 0o022 != 0 {
        return Err(format!("{shown} may be written by others"));
    }
    let fd: crate::sys::Fd = f.as_raw_fd();
    // SAFETY: `fd` is the open file's, and `F_GETFL`/`F_SETFL` take and
    // return its status flags.
    let flags = unsafe { crate::sys::fcntl(fd, crate::sys::F_GETFL) };
    if flags < 0
        || unsafe { crate::sys::fcntl(fd, crate::sys::F_SETFL, flags & !crate::sys::O_NONBLOCK) }
            < 0
    {
        return Err(format!("cannot set {shown} to wait"));
    }
    Ok(Sink::File(f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{Logging, Owned, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 4,
        ws_col: 20,
    };

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("baud-hostlog-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("scratch");
        d
    }

    /// Each read through the feed, the log following the core.
    fn run(log: &mut HostLog, t: &mut Term<'_>, reads: &[&[u8]]) {
        crate::reads::feed(t, reads.iter().copied(), |_| {}, |_| {}, log);
    }

    /// **The log is the host's bytes from after `?46 h` to the end of `?46
    /// l`, appended to the file `OSC 46` named**, across reads and a read
    /// that holds several changes, and a second start appends.
    #[test]
    fn the_log_holds_what_the_host_sent_while_started() {
        let dir = scratch("bytes");
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_logging(Logging::On);
        let mut log = HostLog::new(dir.clone());
        run(
            &mut log,
            &mut t,
            &[
                b"\x1b]46;a.log\x07before\x1b[?46hone ",
                b"two \x1b[?46lnot\x1b[?46h three",
                b"\x1b[?46l after",
            ],
        );
        log.finish();
        let got = std::fs::read(dir.join("a.log")).expect("the log");
        assert_eq!(got, b"one two \x1b[?46l three\x1b[?46l");
        let mut log = HostLog::new(dir.clone());
        run(&mut log, &mut t, &[b"\x1b[?46hmore\x1b[?46l"]);
        log.finish();
        let got = std::fs::read(dir.join("a.log")).expect("the log");
        assert!(got.ends_with(b"\x1b[?46lmore\x1b[?46l"), "appended");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **With no name, the generated one in the start directory; an
    /// absolute name as given, a relative one in the directory, `-` the
    /// standard output.**
    #[test]
    fn the_target_is_named_or_generated() {
        let dir = Path::new("/home/u");
        let name = || "g".to_string();
        assert_eq!(target(None, dir, name), Target::File(dir.join("g")));
        assert_eq!(
            target(Some(b"/var/x.log"), dir, name),
            Target::File(PathBuf::from("/var/x.log"))
        );
        assert_eq!(
            target(Some(b"x/y.log"), dir, name),
            Target::File(dir.join("x/y.log"))
        );
        assert_eq!(target(Some(b"-"), dir, name), Target::Stdout);
        assert_eq!(
            generated_name("h", Duration::from_secs(1_760_965_384), Pid(42)),
            "baud.log.h.2025.10.20.13.03.04.42"
        );
        assert_eq!(
            generated_name("h", Duration::from_secs(951_782_400), Pid(7)),
            "baud.log.h.2000.02.29.00.00.00.7",
            "a leap day"
        );
        let dir = scratch("generated");
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_logging(Logging::On);
        let mut log = HostLog::new(dir.clone());
        run(&mut log, &mut t, &[b"\x1b[?46hx\x1b[?46l"]);
        log.finish();
        let names: Vec<_> = std::fs::read_dir(&dir)
            .expect("the dir")
            .map(|e| {
                e.expect("an entry")
                    .file_name()
                    .into_string()
                    .expect("a name")
            })
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].starts_with("baud.log."), "{names:?}");
        assert!(
            names[0].ends_with(&format!(".{}", std::process::id())),
            "{names:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A file others may write is refused, and the core's log stops**, as
    /// xterm's `open_userfile` refuses it; so is a FIFO with no reader, at
    /// once and without blocking, and a directory.
    #[test]
    fn a_file_xterm_would_refuse_stops_the_log() {
        let dir = scratch("refuse");
        let shared = dir.join("shared.log");
        std::fs::write(&shared, b"").expect("the file");
        std::fs::set_permissions(&shared, std::os::unix::fs::PermissionsExt::from_mode(0o664))
            .expect("chmod");
        let fifo = dir.join("fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        assert!(made.success(), "mkfifo");
        for name in [&b"shared.log"[..], b"fifo", b"."] {
            let mut s = Owned::new(&WS);
            let mut t = Term::new(s.storage(), &WS);
            t.set_logging(Logging::On);
            let mut seq = b"\x1b]46;".to_vec();
            seq.extend_from_slice(name);
            seq.extend_from_slice(b"\x07\x1b[?46hx");
            let mut log = HostLog::new(dir.clone());
            run(&mut log, &mut t, &[&seq]);
            assert_eq!(t.log_mode(), LogMode::Started, "{name:?} asked");
            log.settle(&mut t);
            assert_eq!(t.log_mode(), LogMode::Stopped, "{name:?} refused");
            log.finish();
        }
        assert_eq!(
            std::fs::read(&shared).expect("the file"),
            b"",
            "nothing written"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A writer that falls behind drops what does not fit, and counts
    /// it**: the queue's bound, not the read loop, gives.
    #[test]
    fn a_full_queue_drops_and_counts() {
        let dir = scratch("full");
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_logging(Logging::On);
        let mut log = HostLog::bounded(dir.clone(), Limit(0));
        run(&mut log, &mut t, &[b"\x1b]46;f.log\x07\x1b[?46habc"]);
        assert_eq!(log.dropped(), Dropped(3));
        log.finish();
        assert_eq!(std::fs::read(dir.join("f.log")).expect("the log"), b"");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Off, nothing is written and nothing opens**, and a quit is
    /// finished at once.
    #[test]
    fn off_nothing_is_written() {
        let dir = scratch("off");
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let mut log = HostLog::new(dir.clone());
        run(&mut log, &mut t, &[b"\x1b]46;o.log\x07\x1b[?46habc"]);
        assert!(log.writer.is_none(), "no writer made");
        assert_eq!(log.quit(Instant::now(), || {}), Finished::Yes);
        log.finish();
        assert!(!dir.join("o.log").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A named pipe the test holds the read end of, `0644` whatever the
    /// umask, so the writer opens it and then waits on it once its buffer
    /// is full.
    fn held_pipe(dir: &Path) -> std::fs::File {
        let fifo = dir.join("q.fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        assert!(made.success(), "mkfifo");
        std::fs::set_permissions(&fifo, std::os::unix::fs::PermissionsExt::from_mode(0o644))
            .expect("chmod");
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(crate::sys::O_NONBLOCK)
            .open(&fifo)
            .expect("the read end")
    }

    /// Read the pipe until its first bytes: the writer has opened it.
    /// Before that a read finds no writer and reads nothing.
    fn first_bytes(r: &mut std::fs::File) -> Vec<u8> {
        let mut buf = vec![0u8; 4096];
        loop {
            match std::io::Read::read(r, &mut buf) {
                Ok(n) if n > 0 => {
                    buf.truncate(n);
                    return buf;
                }
                Ok(_) => std::thread::yield_now(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::yield_now(),
                Err(e) => panic!("reading the pipe: {e}"),
            }
        }
    }

    /// Sixteen times the most a pipe holds by default on either system,
    /// 64 KiB, so the writer is still writing while the test looks.
    const MANY: usize = 1 << 20;

    /// The host's last reads, the log still started when the shell exits.
    fn session(log: &mut HostLog) -> Vec<u8> {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_logging(Logging::On);
        let mut read = b"\x1b]46;q.fifo\x07\x1b[?46h".to_vec();
        let body = vec![b'x'; MANY];
        read.extend_from_slice(&body);
        // As the pty gives it, 4096 bytes a read.
        let reads: Vec<&[u8]> = read.chunks(4096).collect();
        run(log, &mut t, &reads);
        assert_eq!(t.log_mode(), LogMode::Started, "still logging");
        body
    }

    /// **A quit is finished only once everything queued before it is
    /// written and the file closed**, and the writer wakes the loop to say
    /// so: the session's last read reaches the log before the app ends.
    #[test]
    fn a_quit_finishes_once_the_last_parts_are_written() {
        let dir = scratch("quit");
        let mut r = held_pipe(&dir);
        let mut log = HostLog::new(dir.clone());
        let sent = session(&mut log);
        let mut got = first_bytes(&mut r);
        let (tx, woken) = channel();
        assert_eq!(
            log.quit(Instant::now(), move || {
                let _ = tx.send(());
            }),
            Finished::No,
            "asked while the writer writes"
        );
        // The pipe is full and nobody reads it: the writer is mid-write.
        assert!(woken.try_recv().is_err(), "not woken before the write");
        assert_eq!(log.quit(Instant::now(), || {}), Finished::No);
        // SAFETY: the read end's descriptor; its status flags in and out.
        let fd: crate::sys::Fd = r.as_raw_fd();
        let flags = unsafe { crate::sys::fcntl(fd, crate::sys::F_GETFL) };
        assert!(flags >= 0, "flags");
        let set =
            unsafe { crate::sys::fcntl(fd, crate::sys::F_SETFL, flags & !crate::sys::O_NONBLOCK) };
        assert!(set >= 0, "blocking");
        // To the end: the writer closed the file.
        std::io::Read::read_to_end(&mut r, &mut got).expect("the rest");
        woken.recv().expect("woken");
        assert_eq!(log.quit(Instant::now(), || {}), Finished::Yes);
        assert_eq!(got.len(), sent.len());
        assert!(got == sent, "every byte of the last read");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A writer that never finishes is given up on at the bound**, so a
    /// log into a pipe nobody reads cannot keep the app.
    #[test]
    fn a_stalled_writer_is_given_up_on_at_the_bound() {
        let dir = scratch("stall");
        let mut r = held_pipe(&dir);
        let mut log = HostLog::new(dir.clone());
        let _ = session(&mut log);
        let _ = first_bytes(&mut r);
        let now = Instant::now();
        assert_eq!(log.quit(now, || {}), Finished::No);
        assert_eq!(log.quit(now + QUIT_MAX / 2, || {}), Finished::No);
        assert_eq!(log.quit(now + QUIT_MAX, || {}), Finished::Yes);
        // The read end goes: the writer's write fails and it ends.
        drop(r);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A name made for a start with none is kept for the next**, as
    /// xterm's `GenerateLogPath` keeps it, so a stop and a second start
    /// append to the first file.
    #[test]
    fn a_generated_name_is_kept_for_the_next_start() {
        static MADE: AtomicUsize = AtomicUsize::new(0);
        fn counted() -> String {
            format!("made.{}", MADE.fetch_add(1, Ordering::Relaxed))
        }
        let dir = scratch("kept");
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_logging(Logging::On);
        let mut log = HostLog::new(dir.clone());
        log.namer = counted;
        run(
            &mut log,
            &mut t,
            &[b"\x1b[?46hone\x1b[?46l", b"\x1b[?46htwo\x1b[?46l"],
        );
        log.finish();
        let names: Vec<_> = std::fs::read_dir(&dir)
            .expect("the dir")
            .map(|e| e.expect("an entry").file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        let got = std::fs::read(dir.join(&names[0])).expect("the log");
        assert_eq!(got, b"one\x1b[?46ltwo\x1b[?46l");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
