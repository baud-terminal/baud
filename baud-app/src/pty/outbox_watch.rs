//! **The reply outbox heard when the master has room again** (FS-B2).
//!
//! A write the kernel took in part or not at all (`Wrote::Part`,
//! `Wrote::Nothing`) leaves bytes owed in the [`Outbox`]: the subsidiary's
//! input queue is full until the child reads. What offers them again is the
//! kernel saying there is room, `EVFILT_WRITE` on the master
//! (`sys::EVFILT_WRITE`), registered once (`EV_ONESHOT`) by the write that
//! left bytes owed and by no other, in a kqueue heard on the main queue as
//! the config's watch is (`reload.rs`): a dispatch source on the kqueue's
//! descriptor, whose handler takes the event and wakes the window. That
//! wake's turn writes again, and registers again only while bytes are
//! still owed. No thread of baud's waits on it. A registration whose
//! bytes another turn wrote first fires once, on a master with room, and
//! that wake writes nothing; once nothing is owed nothing registers.
//!
//! Before it, the rest waited for the window's next turn of any kind. The
//! display link pauses when nothing owes a frame, so a still window whose
//! child blocked reading the rest of its reply waited for ever, and the
//! child with it; then the link ticked while bytes were owed, which a
//! covered window's paused link never did.

use super::{Outbox, Owed, Pty, Wrote, write_some};
use crate::sys::{EV_ADD, EV_ONESHOT, EVFILT_WRITE, Kevent};
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Arc;

/// A kevent record with nothing in it: a change's base, and an event list's.
const EMPTY: Kevent = Kevent {
    ident: 0,
    filter: 0,
    flags: 0,
    fflags: 0,
    data: 0,
    udata: std::ptr::null_mut(),
};

/// **A window's ear for its master's room to write**: a kqueue for
/// `EVFILT_WRITE` on the master and a dispatch source on the kqueue's
/// descriptor, which wakes the window. Made at the window's opening, beside
/// the pty's reader. Held by the window ahead of its pty, so it is dropped
/// first: the source cancelled, and the kqueue closed by the source's
/// cancel handler, once no handler can run (`reload::Listening`'s order).
/// With no kqueue from the kernel it hears nothing, and bytes owed wait for
/// the window's next turn, as they did before it; the window said so when
/// it opened.
pub struct OutboxWatch {
    heard: Option<Heard>,
    /// Whether a registration the kernel refused was said: once a window,
    /// not once a short write -- a flood the kernel takes in many pieces
    /// would say it after each.
    refused: core::cell::Cell<Said>,
}

/// Whether the window has said a refused registration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Said {
    Not,
    Once,
}

/// What [`OutboxWatch::arm`] did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Armed {
    /// The kernel took the registration.
    Registered,
    /// No kqueue to register in, or no descriptor to register.
    Deaf,
    /// The kernel refused it, and this call said so.
    RefusedSaid,
    /// The kernel refused it, already said for this window.
    RefusedAgain,
}

/// The kqueue and the source on it.
struct Heard {
    kq: Arc<OwnedFd>,
    source: dispatch2::DispatchRetained<dispatch2::DispatchSource>,
}

/// **Cancelled when dropped**, so no handler runs after the window it
/// wakes has let it go. The kqueue is not closed here: "Closing the
/// descriptor or port before the cancellation handler is invoked may result
/// in a race condition" (`[[source: macos-sdk-dispatch-source-h]]`
/// 461-468), so the cancel handler frees the context, and with it the
/// context's share of the descriptor.
impl Drop for Heard {
    fn drop(&mut self) {
        self.source.cancel();
    }
}

/// What the handler needs, behind its one context pointer: the kqueue held
/// open until the cancel handler has run, and the window's wake.
struct Context {
    kq: Arc<OwnedFd>,
    wake: Box<dyn Fn()>,
}

/// Whether a drain of the kqueue took an event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Room {
    Heard,
    Quiet,
}

/// **The source's handler**, on the queue it was made for: every event the
/// kqueue holds taken, which deletes each one-shot registration, and the
/// window woken if there was one.
extern "C" fn on_room(ctx: *mut core::ffi::c_void) {
    // SAFETY: the context `listen` boxed for this source, freed only by
    // `on_cancel`, after which no event handler runs ("Cancellation
    // prevents any further invocation of the event handler block",
    // `[[source: macos-sdk-dispatch-source-h]]` 519-521).
    let ctx = unsafe { &*ctx.cast::<Context>() };
    // A panic here would abort: libdispatch's handler cannot unwind.
    baud_present_macos::unwind::guard(
        baud_present_macos::unwind::SiteName("the master's room to write"),
        (),
        || {
            if take(&ctx.kq) == Room::Heard {
                (ctx.wake)();
            }
        },
    );
}

/// **The source's cancel handler**: submitted "once the source's event
/// handler has finished, indicating it is now safe to close the source's
/// handle" (`[[source: macos-sdk-dispatch-source-h]]` 523-525), it frees
/// the context; the kqueue closes here unless the watch still holds it.
extern "C" fn on_cancel(ctx: *mut core::ffi::c_void) {
    // SAFETY: the context `listen` boxed for this source, which runs this
    // once, after its last event handler.
    drop(unsafe { Box::from_raw(ctx.cast::<Context>()) });
}

/// Every event `kq` holds, taken with a zero timeout: whether there was one.
fn take(kq: &OwnedFd) -> Room {
    let mut events = [EMPTY; 4];
    let zero = crate::sys::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let mut room = Room::Quiet;
    loop {
        // SAFETY: no changes; an event list of its length; a zero timeout,
        // so the call returns at once.
        let n = unsafe {
            crate::sys::kevent(
                kq.as_raw_fd(),
                std::ptr::null(),
                0,
                events.as_mut_ptr(),
                4,
                &raw const zero,
            )
        };
        if n > 0 {
            room = Room::Heard;
        }
        if n < 4 {
            return room;
        }
    }
}

impl OutboxWatch {
    /// **The watch, heard on `queue`** -- the main queue in the app --
    /// calling `wake` after each event. A kqueue the kernel refuses is said
    /// once, here.
    pub fn listen(queue: &dispatch2::DispatchQueue, wake: impl Fn() + 'static) -> OutboxWatch {
        use dispatch2::DispatchObject;
        use std::os::fd::FromRawFd;
        // SAFETY: no arguments; a descriptor or -1.
        let fd = unsafe { crate::sys::kqueue() };
        let Ok(handle) = usize::try_from(fd) else {
            crate::warn!(
                "pty: no kqueue for the master's room to write ({}): a reply the \
                 kernel takes in part waits for the window's next turn",
                std::io::Error::last_os_error()
            );
            return OutboxWatch {
                heard: None,
                refused: core::cell::Cell::new(Said::Not),
            };
        };
        // SAFETY: a descriptor just made, owned by nothing else.
        let kq = Arc::new(unsafe { OwnedFd::from_raw_fd(fd) });
        let ctx = Box::into_raw(Box::new(Context {
            kq: Arc::clone(&kq),
            wake: Box::new(wake),
        }));
        // SAFETY: the read source's type, a descriptor the context holds
        // open until the cancel handler has run, no mask ("unused (pass
        // zero for now)"), and a queue the source retains.
        let source = unsafe {
            dispatch2::DispatchSource::new(
                (&raw const dispatch2::_dispatch_source_type_read).cast_mut(),
                handle,
                0,
                Some(queue),
            )
        };
        // SAFETY: the context is the box `on_room` reads and `on_cancel`
        // frees, valid until then.
        unsafe { source.set_context(ctx.cast()) };
        source.set_event_handler_f(on_room);
        source.set_cancel_handler_f(on_cancel);
        source.activate();
        OutboxWatch {
            heard: Some(Heard { kq, source }),
            refused: core::cell::Cell::new(Said::Not),
        }
    }

    /// **What `out` owes the child, written through `pty`**, as much as the
    /// kernel takes now ([`Pty::write`]); while any is left, the master's
    /// room to write registered, so the window is woken to write the rest.
    /// A refused terminal lets its bytes go and registers nothing.
    pub fn write(&self, pty: &Pty, out: &mut Outbox) -> Wrote {
        match pty {
            Pty::Live(l) => self.write_to(l.master, out),
            Pty::Refused(_) => pty.write(out),
        }
    }

    /// [`OutboxWatch::write`] to `master`: registered on [`Owed::Bytes`]
    /// alone, every time a write leaves bytes owed -- a flood the kernel
    /// takes in many pieces registers after each.
    fn write_to(&self, master: crate::sys::Fd, out: &mut Outbox) -> Wrote {
        let wrote = write_some(master, out);
        match out.owed() {
            Owed::Bytes => {
                let _ = self.arm(master);
            }
            Owed::Nothing => {}
        }
        wrote
    }

    /// `EVFILT_WRITE` on `master`, once (`EV_ADD | EV_ONESHOT`): the event
    /// comes when the kernel will take bytes, and is deleted as the handler
    /// takes it. Registered on a master that already has room, it comes at
    /// once, so a child that read between the write and this call is not
    /// missed. A refusal is said once a window ([`Said`]).
    fn arm(&self, master: crate::sys::Fd) -> Armed {
        let Some(heard) = &self.heard else {
            return Armed::Deaf;
        };
        let Ok(ident) = core::ffi::c_ulong::try_from(master) else {
            return Armed::Deaf;
        };
        let change = Kevent {
            ident,
            filter: EVFILT_WRITE,
            flags: EV_ADD | EV_ONESHOT,
            ..EMPTY
        };
        let zero = crate::sys::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: one change, no event list, a zero timeout.
        let r = unsafe {
            crate::sys::kevent(
                heard.kq.as_raw_fd(),
                &raw const change,
                1,
                std::ptr::null_mut(),
                0,
                &raw const zero,
            )
        };
        if r == 0 {
            return Armed::Registered;
        }
        // The kernel's reason, read before anything else can set errno.
        let why = std::io::Error::last_os_error();
        if self.refused.replace(Said::Once) == Said::Once {
            return Armed::RefusedAgain;
        }
        crate::warn!(
            "pty: the master's room to write was not registered ({why}): what \
             is owed waits for the window's next turn"
        );
        Armed::RefusedSaid
    }
}

#[cfg(test)]
mod tests {
    use super::super::{nonblocking, open_terminal};
    use super::{Armed, EMPTY, OutboxWatch};
    use crate::pty::{Outbox, Owed, Wrote};
    use crate::sys::{EV_ADD, EV_ONESHOT, EVFILT_WRITE, Kevent};
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
    use std::time::Duration;

    /// A bound on a hang, not a timing: a pass comes at once.
    const HANG: Duration = Duration::from_secs(10);

    /// How long a control waits for what must not come.
    const QUIET: Duration = Duration::from_millis(200);

    /// A kqueue of the test's own.
    fn kqueue() -> OwnedFd {
        use std::os::fd::FromRawFd;
        // SAFETY: no arguments; a descriptor or -1.
        let kq = unsafe { crate::sys::kqueue() };
        assert!(kq >= 0, "kqueue: {}", std::io::Error::last_os_error());
        // SAFETY: a descriptor just made, owned by nothing else.
        unsafe { OwnedFd::from_raw_fd(kq) }
    }

    /// `EVFILT_WRITE` on `fd`, once (`EV_ADD | EV_ONESHOT`).
    fn arm(kq: &OwnedFd, fd: crate::sys::Fd) {
        let change = Kevent {
            ident: core::ffi::c_ulong::try_from(fd).expect("an open descriptor"),
            filter: EVFILT_WRITE,
            flags: EV_ADD | EV_ONESHOT,
            ..EMPTY
        };
        let zero = crate::sys::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: one change, no event list, a zero timeout.
        let r = unsafe {
            crate::sys::kevent(
                kq.as_raw_fd(),
                &raw const change,
                1,
                std::ptr::null_mut(),
                0,
                &raw const zero,
            )
        };
        assert_eq!(r, 0, "EV_ADD: {}", std::io::Error::last_os_error());
    }

    /// The events `kq` holds, waiting at most `wait`.
    fn events(kq: &OwnedFd, wait: Duration) -> Vec<Kevent> {
        let mut list = [EMPTY; 4];
        let t = crate::sys::Timespec {
            tv_sec: wait.as_secs().try_into().expect("a bound in seconds"),
            tv_nsec: wait.subsec_nanos().into(),
        };
        // SAFETY: no changes; an event list of its length; a timeout.
        let n = unsafe {
            crate::sys::kevent(
                kq.as_raw_fd(),
                std::ptr::null(),
                0,
                list.as_mut_ptr(),
                4,
                &raw const t,
            )
        };
        let n = usize::try_from(n).expect("kevent failed");
        list[..n].to_vec()
    }

    /// **A pty pair with no program**: the subsidiary raw with no echo, so
    /// the bytes written to the master wait in its input queue as written,
    /// and both ends non-blocking.
    fn pair() -> (OwnedFd, OwnedFd) {
        let (master, replica) = open_terminal(crate::sys::Winsize::default())
            .unwrap_or_else(|why| panic!("no pty pair: {why:?}"));
        let raw = std::process::Command::new("/bin/stty")
            .args(["raw", "-echo"])
            .stdin(std::process::Stdio::from(
                replica.try_clone().expect("a copy of the subsidiary"),
            ))
            .status()
            .expect("stty runs");
        assert!(raw.success(), "stty raw -echo: {raw}");
        nonblocking(master.as_raw_fd()).expect("the master non-blocking");
        nonblocking(replica.as_raw_fd()).expect("the subsidiary non-blocking");
        (master, replica)
    }

    /// Printable bytes, so no line discipline setting reads one as a
    /// control.
    fn printable() -> impl Iterator<Item = u8> {
        (b' '..=b'~').cycle()
    }

    /// Writes printable bytes to `fd` until the kernel answers `EAGAIN`:
    /// the bytes it took, in order.
    fn fill(fd: crate::sys::Fd) -> Vec<u8> {
        let payload: Vec<u8> = printable().take(1 << 20).collect();
        let mut took = 0usize;
        // A bound on a queue that never fills, far past Darwin's 1022.
        while took < payload.len() {
            let rest = &payload[took..(took + 256).min(payload.len())];
            // SAFETY: the slice is valid for its length and the fd is open.
            let n = unsafe { crate::sys::write(fd, rest.as_ptr().cast(), rest.len()) };
            match usize::try_from(n) {
                Ok(n) => took += n,
                Err(_) => {
                    let e = std::io::Error::last_os_error();
                    assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock, "{e}");
                    return payload[..took].to_vec();
                }
            }
        }
        panic!("the queue never filled: {took} bytes taken");
    }

    /// Reads `fd`, non-blocking, until it is empty: what it held.
    fn empty(fd: crate::sys::Fd) -> Vec<u8> {
        let mut buf = [0u8; 4096];
        let mut got = Vec::new();
        loop {
            // SAFETY: the buffer is valid for its length and the fd is open.
            let n = unsafe { crate::sys::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
            match usize::try_from(n) {
                Ok(0) => return got,
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(_) => {
                    let e = std::io::Error::last_os_error();
                    assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock, "{e}");
                    return got;
                }
            }
        }
    }

    /// A watch heard on a queue of the test's own, its wake a message on
    /// the receiver: the receiver disconnects once the cancel handler has
    /// freed the context, and the wake with it.
    fn watch(queue: &dispatch2::DispatchQueue) -> (OutboxWatch, Receiver<()>) {
        let (tx, rx) = channel();
        let tx = std::sync::Mutex::new(tx);
        let w = OutboxWatch::listen(queue, move || {
            let _ = tx.lock().map(|t| t.send(()));
        });
        assert!(w.heard.is_some(), "the kernel gave no kqueue");
        (w, rx)
    }

    /// **Darwin's kqueue reports room to write on a pty master once its
    /// subsidiary reads** (`EVFILT_WRITE`), which the outbox's watch rests
    /// on. `kqueue(2)` names sockets, pipes and fifos and not a pty, and
    /// `poll(2)` says it "currently does not support devices" yet watches
    /// the master (`sys::POLLNVAL`), so the page is not the proof. A pty
    /// pair, no program: the subsidiary raw with no echo, the master
    /// written until `EAGAIN`, then `EVFILT_WRITE` armed once
    /// (`EV_ONESHOT`). The controls: armed on a full queue, nothing at
    /// once, and nothing within a bound while no one reads. Then the
    /// subsidiary read to empty, and the event comes, for the master, and
    /// once: the registration is gone after it. Armed again on a queue with
    /// room, it comes at once, so a watch armed after the child already
    /// read is not left waiting for a read that has happened.
    #[test]
    fn a_master_reports_room_to_write_once_its_subsidiary_reads() {
        let (master, replica) = pair();
        let took = fill(master.as_raw_fd());
        assert!(!took.is_empty(), "the queue took nothing");
        let kq = kqueue();
        arm(&kq, master.as_raw_fd());
        // The controls: a full queue reports no room, at once or while no
        // one reads.
        assert_eq!(events(&kq, Duration::ZERO).len(), 0, "room on a full queue");
        assert_eq!(events(&kq, QUIET).len(), 0, "room while no one reads");
        assert!(
            empty(replica.as_raw_fd()) == took,
            "every byte read, in order"
        );
        let heard = events(&kq, HANG);
        assert_eq!(heard.len(), 1, "room once the subsidiary read");
        let master_id = core::ffi::c_ulong::try_from(master.as_raw_fd()).expect("an open master");
        assert_eq!(heard[0].ident, master_id);
        assert_eq!(heard[0].filter, EVFILT_WRITE);
        // Once: the registration went with its event.
        assert_eq!(events(&kq, QUIET).len(), 0, "a one-shot reported twice");
        // Armed on a queue with room, it comes at once.
        arm(&kq, master.as_raw_fd());
        let again = events(&kq, HANG);
        assert_eq!(again.len(), 1, "armed with room, no event");
        assert_eq!(again[0].ident, master_id);
    }

    /// **A flood the kernel takes in pieces is written whole, each piece on
    /// the wake its room brings** (FS-B2): 8 KiB owed to a child whose
    /// queue holds about a kilobyte, written through the watch, and the
    /// child's reads the only thing that moves it. Each write that leaves
    /// bytes owed registers the master's room again, so every piece comes,
    /// at least three of them short, and every byte arrives once and in
    /// order. The controls: no wake while the queue is full and no one
    /// reads, and none once the last write took everything, which
    /// registers nothing. Breaks: no registration while bytes are owed,
    /// and the first wake never comes; a registration on the first short
    /// write alone, and the second never comes.
    #[test]
    fn a_flood_written_in_pieces_is_woken_for_each() {
        let (master, replica) = pair();
        let queue = dispatch2::DispatchQueue::new("baud.outbox.flood", None);
        let (w, rx) = watch(&queue);
        let payload: Vec<u8> = printable().take(8192).collect();
        let mut out = Outbox::default();
        out.push(&payload);
        let mut got = Vec::new();
        let mut short = 0usize;
        loop {
            match w.write_to(master.as_raw_fd(), &mut out) {
                Wrote::All => break,
                Wrote::Part | Wrote::Nothing => short += 1,
            }
            assert_eq!(out.owed(), Owed::Bytes);
            if short == 1 {
                assert_eq!(
                    rx.recv_timeout(QUIET),
                    Err(RecvTimeoutError::Timeout),
                    "woken while the queue was full and no one read"
                );
            }
            got.extend(empty(replica.as_raw_fd()));
            rx.recv_timeout(HANG).unwrap_or_else(|e| {
                panic!(
                    "no wake for room after short write {short}: {e:?}, {} read, {} owed",
                    got.len(),
                    out.bytes.len()
                )
            });
        }
        assert!(short >= 3, "{short} short writes: the flood fit in fewer");
        assert_eq!(
            rx.recv_timeout(QUIET),
            Err(RecvTimeoutError::Timeout),
            "woken with nothing owed"
        );
        got.extend(empty(replica.as_raw_fd()));
        assert_eq!(got.len(), payload.len());
        assert!(got == payload, "every byte, once, in order");
    }

    /// When a dropped watch's master closes, against its queue's turn.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Closed {
        /// Before the queue runs again, as a window's fields drop.
        BeforeTheQueue,
        /// After the cancel handler has run: the event still the kqueue's
        /// to deliver when the queue runs again.
        AfterTheCancel,
    }

    /// **A watch dropped with the master's room registered runs no handler
    /// after**, and its kqueue closes once the cancel handler has run
    /// (`reload::Listening`'s order): the queue held, a short write
    /// registers the room, the child reads so the event waits in the
    /// kqueue, and the watch is dropped. Let go, the queue runs the cancel
    /// handler, which frees the context and its wake, and no handler before
    /// it: the wake's receiver disconnects with nothing sent. Twice: the
    /// master closed straight after the drop, as a window's fields drop,
    /// which takes the registration with it ("Events which are attached to
    /// file descriptors are automatically deleted on the last close of the
    /// descriptor", `[[source: macos-man-kqueue]]` 191-193); and the master
    /// held open until the cancel handler has run, so only the cancel stands
    /// between the waiting event and a handler. The flood's test is the
    /// control that the same watch, kept, is woken. Break: the drop not
    /// cancelling the source, and the second wakes.
    #[test]
    fn a_watch_dropped_with_room_registered_wakes_no_one() {
        use dispatch2::DispatchObject;
        let mut heard = Vec::new();
        for closed in [Closed::BeforeTheQueue, Closed::AfterTheCancel] {
            let (master, replica) = pair();
            let queue = dispatch2::DispatchQueue::new("baud.outbox.drop", None);
            let (w, rx) = watch(&queue);
            let mut out = Outbox::default();
            out.push(&printable().take(4096).collect::<Vec<u8>>());
            assert_eq!(w.write_to(master.as_raw_fd(), &mut out), Wrote::Part);
            // Nothing on the queue runs while it is suspended, the cancel
            // handler included.
            queue.suspend();
            let _ = empty(replica.as_raw_fd());
            // The event waits in the kqueue: its descriptor reads ready.
            let kq = w.heard.as_ref().expect("a kqueue").kq.as_raw_fd();
            let mut p = crate::sys::PollFd {
                fd: kq,
                events: crate::sys::POLLIN,
                revents: 0,
            };
            // SAFETY: one initialised record and its count; a bound on a
            // hang.
            let n = unsafe { crate::sys::poll(&mut p, 1, 10_000) };
            assert_eq!(n, 1, "{closed:?}: the room never reached the kqueue");
            drop(w);
            let open = match closed {
                Closed::BeforeTheQueue => {
                    drop(master);
                    None
                }
                Closed::AfterTheCancel => Some(master),
            };
            queue.resume();
            heard.push((closed, rx.recv_timeout(HANG)));
            drop(open);
        }
        // Both read before either is judged, so a break shows each.
        assert_eq!(
            heard,
            [
                (Closed::BeforeTheQueue, Err(RecvTimeoutError::Disconnected)),
                (Closed::AfterTheCancel, Err(RecvTimeoutError::Disconnected)),
            ],
            "Ok: a handler woke after the drop; Timeout: the context was never freed"
        );
    }

    /// **A stream that closed lets what is owed go**: nothing written to a
    /// child that has gone reaches it, so the outbox keeps nothing for a
    /// later turn to write again, fail, and register the master's room
    /// for. The outbox's own reading, and the driver's closed arm. Break:
    /// the closed arm keeping the outbox, and the source check fails.
    #[test]
    fn a_closed_stream_lets_what_is_owed_go() {
        let mut outbox = Outbox::default();
        assert_eq!(outbox.owed(), Owed::Nothing);
        outbox.push(b"\x1b[?6;4c");
        assert_eq!(outbox.owed(), Owed::Bytes);
        outbox.let_go();
        assert_eq!(outbox.owed(), Owed::Nothing);
        let closed = crate::app_source::split_once("    fn pump(&mut self, trigger: Trigger) {")
            .and_then(|(_, pump)| pump.split_once("if drained.stream == Stream::Closed {"))
            .and_then(|(_, arm)| arm.split_once("\n        }\n"))
            .expect("the closed stream's arm")
            .0;
        assert!(closed.contains("self.app.outbox.let_go();"), "{closed}");
    }

    /// **A refused registration is said once a window, not once a short
    /// write** (W-2): a flood the kernel takes in many pieces arms after
    /// each, and a kernel that refuses every registration would otherwise
    /// put a line in the log for each piece. A descriptor no process has
    /// open, `0x7fff_fff0`, is refused (`EBADF`) on every call: the first
    /// says it, every later one does not. The control: an open master is
    /// registered, and says nothing. Break: the warn on every refusal, and
    /// the second call says it again.
    #[test]
    fn a_refused_registration_is_said_once_a_window() {
        let queue = dispatch2::DispatchQueue::new("baud.outbox.refused", None);
        let (w, _rx) = watch(&queue);
        let (master, _replica) = pair();
        assert_eq!(w.arm(master.as_raw_fd()), Armed::Registered, "the control");
        let none: crate::sys::Fd = 0x7fff_fff0;
        assert_eq!(w.arm(none), Armed::RefusedSaid, "the first refusal is said");
        for _ in 0..3 {
            assert_eq!(w.arm(none), Armed::RefusedAgain, "said again");
        }
        assert_eq!(w.arm(master.as_raw_fd()), Armed::Registered, "still heard");
    }
}
