//! kitty's VT parser, through a real PTY, on the same bytes as `multibench`.
//!
//! kitty cannot be a core in `multibench`: its parser is a CPython object. What
//! it exposes is the buffer-protocol cycle its own test suite drives the parser
//! with, the same shape as baud's `Term::write_at` and `Term::feed`, and this
//! harness calls those three methods of `Screen` for each read of up to 4096
//! bytes:
//!
//! ```text
//! test_create_write_buffer
//! test_commit_write_buffer   ~ baud: Term::write_at
//! test_parse_written_data    ~ baud: Term::feed
//! ```
//!
//! Python still runs here. It is kitty's own interpreter, loaded at run time from
//! a kitty the user built or installed, and it is part of what is measured. The
//! `ceiling` row runs the identical PTY loop with a sink that does nothing and
//! before any of kitty is loaded.
//!
//! The payloads are not generated here: `DUMP_PAYLOADS=<dir>` makes `multibench`
//! write the exact bytes it measures, so "same payloads" is a fact.

pub mod args;
pub mod cycle;
mod error;
pub mod locate;
pub mod pty;
pub mod python;
pub mod report;

#[cfg(test)]
mod testdir;

pub use error::Error;
