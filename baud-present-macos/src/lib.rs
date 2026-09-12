//! macOS presentation: the window is the owner's `macos-liquid-glass`; this
//! crate supplies what goes inside `set_content_view`.
//!
//! The design is `docs/research/notes/j-rendering.md` D13, with the owner's
//! two macOS calls made: one IOSurface-backed texture that the CPU writes
//! under `IOSurfaceLock` or the GPU draws into, presented by setting a plain
//! `CALayer`'s contents -- the lower-level path, no drawable pool -- and a
//! non-opaque layer, because an opaque one paints the glass out (the crate's
//! own luma measurements, pinned as `mlg-measurements`).
//!
//! Empty until phase 5 of the approved plan. The crate compiles to nothing off
//! macOS, the `baud-isa-aarch64` pattern for a target-bound crate.
#![cfg(target_os = "macos")]
#![forbid(unsafe_code)]
