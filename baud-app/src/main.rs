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
//! `unsafe` lives in `pty.rs` alone, which forbids nothing and says why at
//! every call; `app.rs` forbids it.

#[cfg(target_os = "macos")]
mod app;
mod config;
#[cfg(target_os = "macos")]
mod pty;

fn main() {
    #[cfg(target_os = "macos")]
    app::run();
    #[cfg(not(target_os = "macos"))]
    eprintln!("baud-app has a macOS presentation layer only, so far");
}
