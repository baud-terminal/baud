//! **The stages of the app between the pty and the screen, and what
//! crosses between them.** Two stages, each on a thread of its own: the
//! ingest thread reads and parses, the frame thread builds and presents.
//!
//! - [`ctx`]: each stage's thread context, made only on that stage's
//!   thread; a call that may block takes the frame thread's.
//! - [`handoff`]: the frame handoff, three slots, each owned by one side at
//!   a time. The ingest thread fills a slot and publishes it; the frame
//!   thread takes the slot last published and draws from it. Neither waits
//!   for the other, and the types say which side holds each slot: a slot reached after
//!   it was handed on, or from two threads at once, is a compile error.
//! - [`lease`]: what the frame thread holds of the ingest thread's until
//!   the GPU is done with it. A lease is owned, and its drop hands the item
//!   back over a bounded ring that never fills; the ingest thread gets an
//!   item back only from what a drop handed back.
//!
//! **The one `unsafe` is in [`handoff`]**, the slots' deref and the `Sync`
//! it rests on, and its soundness argument is written beside it.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

pub mod ctx;
pub mod handoff;
pub mod lease;
