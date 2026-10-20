//! **baud's Wayland presentation layer, over the pure-protocol connection.**
//! A `wl_shm` window with its xdg role, frames paced on the frame callback,
//! the pointer a frame at a time with its shape through `cursor-shape-v1`,
//! and the keyboard through `baud-xkb` -- the register's D13 for Linux, CPU
//! path first. The GPU path (Vulkan over dmabuf) is not here.
//!
//! Every piece is tested against a fake compositor over a socket pair, so
//! the handshake, the buffer traffic and the keys are exercised on any
//! box; what it cannot show here is a window, and the gate for that is the
//! app on a Linux box.

#![deny(unsafe_code)]

pub mod keys;
pub mod pointer;
pub mod shm;
pub mod sys;
pub mod window;

pub use pointer::{At, Button, CursorShape, EventCode, PointerEvent, Press, Scroll};
pub use shm::{Pool, Px};
pub use window::{Event, Presented, Resize, Shaped, Shown, Toggle, Window};
