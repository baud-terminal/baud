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
//! `unsafe` lives in `pty.rs`, in `kitty_files.rs`'s shared-memory read
//! and in `pages.rs`'s release of freed image pages, which forbid nothing
//! and say why at every call; `app.rs` and `linux.rs` forbid it.

#[cfg(target_os = "macos")]
mod app;
// The visual bell's state is pure and its tests run on every platform; both
// drivers read it, and so does the pty, for the one spelling of `TERM`.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod bell;
// The cursor's blink clock: pure, so its tests run on every platform; both
// drivers read it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod caret;
// What a program's `OSC 52` copy does to the pasteboard: pure, so its tests
// run on every platform; only the macOS driver has a pasteboard today.
#[cfg(any(target_os = "macos", test))]
mod clip;
mod config;
// The font compiled into the binary: the default stack's first face.
mod embedded;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod host;
// The kitty file mediums' reader, which both drivers lend the core.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod kitty_files;
#[cfg(target_os = "macos")]
mod latency;
// The link policy is pure and its tests run on every platform; only the
// macOS driver consumes it today, so the bin builds it there alone.
#[cfg(any(target_os = "macos", test))]
mod link;
#[cfg(target_os = "linux")]
mod linux;
mod log;
// Freed image pages back to the system, which both drivers lend the core.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod pages;
// The theme files `[palette]` names: read at launch by both drivers, and
// its tests run on every platform.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod palette;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod pty;
// The child's reads into the core and its answers out: pure, so its tests
// run on every platform, and both drivers feed through it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod reads;
// xterm's readline modes, a click's cursor keys and a paste's shape: pure,
// so its tests run on every platform; only the macOS driver has a pointer
// and a paste, so the bin builds it there alone.
#[cfg(any(target_os = "macos", test))]
mod readline;
// The scrollbar's reveal is pure and its tests run on every platform; only
// the macOS driver draws a scrollbar, so the bin builds it there alone.
#[cfg(any(target_os = "macos", test))]
mod reveal;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod sys;
// The quad trail's driver is pure and its tests run on every platform; only
// the macOS driver draws a trail today, so the bin builds it there alone.
#[cfg(any(target_os = "macos", test))]
mod trail;
// What a program's title becomes on the window: pure, so its tests run on
// every platform, and both drivers read it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod title;
// What a program's page-width request does to the window: pure, so its tests
// run on every platform, and both drivers read it.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
mod width;
// The wheel's travel and its reports to a tracking application: pure, so its
// tests run on every platform; only the macOS driver has a pointer today, so
// the bin builds it there alone.
#[cfg(any(target_os = "macos", test))]
mod wheel;

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
