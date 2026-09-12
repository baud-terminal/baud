//! The terminal emulator as a process: read the pty, feed `baud-core`, render
//! with `baud-render`, present through the platform crate, encode input back.
//!
//! Empty until phase 5 of the approved plan (`docs/research/notes/j-rendering.md`
//! decides the pieces; the plan sequences them). It exists now so the
//! workspace, the audit and the docs gates all know its name.
#![forbid(unsafe_code)]

fn main() {}
