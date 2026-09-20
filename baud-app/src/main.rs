//! The terminal emulator as a process: read the pty, feed `baud-core`, render
//! with `baud-render`, present through the platform crate, encode input back.
//!
//! macOS first, through the owner's `macos-liquid-glass`: the window is the
//! crate's, the content view is its `DrawableView`, and this binary is the
//! `Responder` behind it -- one object on the main thread that owns the
//! terminal, the renderer, the presenter and the pty's write end. The pty's
//! read end lives on its own thread and hands chunks over a channel; the
//! display link's tick drains the channel, feeds the core, paints the rows
//! that changed and presents. That is "render on input, paced by the
//! display link" (D13): a chunk never waits longer than one frame.
//!
//! Linux second, through `baud-present-wayland`: one loop that waits on the
//! compositor's socket and the pty together, reads the pty where it is
//! polled, and presents on the CPU into `wl_shm` -- `linux.rs`, which is
//! type-checked against the Linux target from this Mac and has not yet run
//! on a compositor.
//!
//! `unsafe` lives in `pty.rs` alone, which forbids nothing and says why at
//! every call; `app.rs` and `linux.rs` forbid it.

#[cfg(target_os = "macos")]
mod app;
mod config;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod host;
#[cfg(target_os = "macos")]
mod latency;
// The link policy is pure and its tests run on every platform; only the
// macOS driver consumes it today, so the bin builds it there alone.
#[cfg(any(target_os = "macos", test))]
mod link;
#[cfg(target_os = "linux")]
mod linux;
mod log;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod pty;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod sys;

fn main() {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    host::startup_begin();
    log::init();
    #[cfg(target_os = "macos")]
    app::run();
    #[cfg(target_os = "linux")]
    linux::run();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    crate::warn!("baud-app presents on macOS and on Linux over Wayland, so far");
}
