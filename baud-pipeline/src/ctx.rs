//! **The two stages' thread contexts**: an [`IngestCtx`] is held by the
//! thread that reads and parses, a [`FrameCtx`] by the thread that builds
//! and presents. A call that may block takes `&FrameCtx`, so the ingest
//! thread, which holds only an `IngestCtx`, cannot make one.
//!
//! **Each is made only by [`spawn`], on the thread it belongs to**, and
//! never leaves it: neither is `Send`, `Sync` or `Clone`, and neither has a
//! public constructor. A stage's closure is handed its context by value and
//! cannot make the other stage's.
//!
//! Made outside the crate, neither compiles:
//!
//! ```compile_fail,E0451
//! let c = baud_pipeline::ctx::IngestCtx {
//!     here: std::marker::PhantomData,
//!     thread: std::thread::current().id(),
//! };
//! ```
//!
//! ```compile_fail,E0451
//! let c = baud_pipeline::ctx::FrameCtx {
//!     here: std::marker::PhantomData,
//!     thread: std::thread::current().id(),
//! };
//! ```
//!
//! Sent to another thread, it does not compile:
//!
//! ```compile_fail,E0277
//! use baud_pipeline::ctx::{spawn, IngestCtx};
//! spawn("ingest", |ingest: IngestCtx| {
//!     std::thread::spawn(move || drop(ingest));
//! });
//! ```
//!
//! Shared with another thread by reference, it does not compile:
//!
//! ```compile_fail,E0277
//! use baud_pipeline::ctx::{spawn, FrameCtx};
//! spawn("frame", |frame: FrameCtx| {
//!     std::thread::scope(|t| {
//!         t.spawn(|| drop(&frame));
//!     });
//! });
//! ```
//!
//! Cloned, it does not compile:
//!
//! ```compile_fail,E0599
//! use baud_pipeline::ctx::{spawn, IngestCtx};
//! spawn("ingest", |ingest: IngestCtx| {
//!     let twice = ingest.clone();
//! });
//! ```
//!
//! A blocking call made from the ingest thread does not compile:
//!
//! ```compile_fail,E0308
//! use baud_pipeline::ctx::{spawn, wait, IngestCtx};
//! let (_tx, rx) = std::sync::mpsc::channel::<u8>();
//! spawn("ingest", move |ingest: IngestCtx| {
//!     wait(&ingest, &rx);
//! });
//! ```

use std::io;
use std::marker::PhantomData;
use std::sync::mpsc::Receiver;
use std::thread::{JoinHandle, ThreadId};

/// **What keeps a context on its thread**: a raw pointer is neither `Send`
/// nor `Sync`, so neither is a context that holds one.
type Here = PhantomData<*const ()>;

/// **The ingest thread's context**: held by the stage that owns the
/// terminal and the pty, and taken by nothing that may block.
pub struct IngestCtx {
    here: Here,
    thread: ThreadId,
}

/// **The frame thread's context**: held by the stage that owns the window,
/// the renderer and the GPU, and taken by every call that may block.
pub struct FrameCtx {
    here: Here,
    thread: ThreadId,
}

mod sealed {
    /// Makes a context on the thread that calls it. Private to the crate,
    /// so only [`super::spawn`] calls it.
    pub trait Made {
        fn made() -> Self;
    }
}

/// **A stage's context**: [`IngestCtx`] or [`FrameCtx`], and no other.
pub trait Ctx: sealed::Made {
    /// The thread the context was made on, which is the only one it is
    /// ever on.
    fn thread(&self) -> ThreadId;
}

impl sealed::Made for IngestCtx {
    fn made() -> Self {
        IngestCtx {
            here: PhantomData,
            thread: std::thread::current().id(),
        }
    }
}

impl sealed::Made for FrameCtx {
    fn made() -> Self {
        FrameCtx {
            here: PhantomData,
            thread: std::thread::current().id(),
        }
    }
}

impl Ctx for IngestCtx {
    fn thread(&self) -> ThreadId {
        self.thread
    }
}

impl Ctx for FrameCtx {
    fn thread(&self) -> ThreadId {
        self.thread
    }
}

/// **A stage on a thread of its own**, named `name`: the thread makes the
/// stage's context and hands it to `stage` by value.
pub fn spawn<C, R>(
    name: &str,
    stage: impl FnOnce(C) -> R + Send + 'static,
) -> io::Result<JoinHandle<R>>
where
    C: Ctx,
    R: Send + 'static,
{
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || stage(C::made()))
}

/// **Waits for the next message on `rx`**, blocking the calling thread:
/// the frame thread's alone. `None` when every sender is gone.
pub fn wait<T>(_frame: &FrameCtx, rx: &Receiver<T>) -> Option<T> {
    rx.recv().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;

    /// **Each stage's context is made on the stage's own thread**, and the
    /// two stages are two threads, neither of them the caller's.
    ///
    /// Named break: `spawn` making the context before it starts the thread
    /// -- the context's thread is the caller's, not the stage's.
    #[test]
    fn each_context_is_made_on_its_stages_own_thread() {
        let ingest = spawn("ingest", |c: IngestCtx| {
            (c.thread(), thread::current().id())
        })
        .unwrap()
        .join()
        .unwrap();
        let frame = spawn("frame", |c: FrameCtx| (c.thread(), thread::current().id()))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(
            ingest.0, ingest.1,
            "the ingest context on its stage's thread"
        );
        assert_eq!(frame.0, frame.1, "the frame context on its stage's thread");
        assert_ne!(ingest.1, frame.1, "two stages, two threads");
        assert_ne!(ingest.1, thread::current().id(), "not the caller's thread");
    }

    /// **A stage's thread carries the name it was spawned with.**
    ///
    /// Named break: `spawn` dropping the name -- the thread is unnamed.
    #[test]
    fn a_stages_thread_carries_its_name() {
        let name = spawn("ingest", |_: IngestCtx| {
            thread::current().name().map(str::to_owned)
        })
        .unwrap()
        .join()
        .unwrap();
        assert_eq!(name.as_deref(), Some("ingest"));
    }

    /// **The frame thread's wait gets the message queued**, and gets none
    /// once every sender is gone.
    ///
    /// Named break: `wait` reading the channel as closed while a message is
    /// queued -- the first wait reads `None`.
    #[test]
    fn the_frame_thread_waits_for_a_message() {
        let (tx, rx) = mpsc::channel();
        tx.send(7u8).unwrap();
        drop(tx);
        let got = spawn("frame", move |c: FrameCtx| (wait(&c, &rx), wait(&c, &rx)))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(got, (Some(7), None));
    }
}
