//! **baud's Vulkan presentation on Linux.** A frame is drawn into an image
//! exported as a dmabuf and shown through `baud-present-wayland`'s
//! `present_dmabuf`. What it runs on is chosen from capabilities alone
//! (`choose.rs`); anything missing leaves the window on shared memory.

//!
//! Linux only: elsewhere the crate is empty, so `cargo test --workspace`
//! on macOS builds it and runs nothing that needs Vulkan.

#![cfg(target_os = "linux")]
#![deny(unsafe_code)]

pub mod cells;
pub mod choose;
pub mod chrome;
pub mod damage;
pub mod draw;
pub mod effect;
pub mod frame;
pub mod lend;
mod loader;
pub mod pipeline;
pub mod present;
pub mod sheets;
pub mod sys;
pub mod vk;
