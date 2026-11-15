//! **baud's Wayland presentation layer, over the pure-protocol connection.**
//! A `wl_shm` window with its xdg role, frames paced on the frame callback
//! and timed by the presentation feedback, held to the refresh through
//! `fifo-v1` and `commit-timing-v1` and shown torn on request through
//! `tearing-control-v1`, each where the compositor offers it (`timing.rs`),
//! the pointer a frame at a time with its shape through `cursor-shape-v1`
//! or, without it, an image of its own (`cursor.rs`), the frame drawn at
//! the scale the compositor prefers, a fraction through `wp_viewport`
//! (`scale.rs`), the clipboard and the
//! primary selection (`clip.rs`), an input method through `text-input-v3`
//! (`ime.rs`), Compose sequences and dead keys (`compose.rs`), and the
//! keyboard through `baud-xkb` -- the register's D13 for Linux, CPU
//! path first. The GPU path (Vulkan over dmabuf) is not here.
//!
//! Every piece is tested against a fake compositor over a socket pair, so
//! the handshake, the buffer traffic and the keys are exercised on any
//! box; what it cannot show here is a window, and the gate for that is the
//! app on a Linux box.

#![deny(unsafe_code)]

pub mod clip;
pub mod compose;
pub mod cursor;
pub mod damage;
pub mod ime;
pub mod keys;
pub mod pointer;
pub mod scale;
pub mod shm;
pub mod sys;
pub mod timing;
pub mod window;

pub use clip::{Board, Copied, Pasting, Want};
pub use damage::{Changed, Strip};
pub use ime::{ByteAt, CursorAt, CursorRect, Hint, Hinted, Preedit, PreeditCursor};
pub use keys::Shortcut;
pub use pointer::{At, Button, CursorShape, EventCode, PointerEvent, Press, Scroll};
pub use scale::BufferScale;
pub use shm::{Pool, Px};
pub use timing::{ClockId, Presentation, Stamp, Unseen};
pub use window::{
    Activated, ConfigureBounds, Event, OutputPx, Presented, Resize, Rung, Shaped, Shown, Tearing,
    Toggle, Window,
};
