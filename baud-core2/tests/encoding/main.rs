//! Text decoding, as distinct from the control protocol.
//!
//! `tests/parser/` covers the bytes that are *not* text. This covers the bytes
//! that are: whether they decode, what happens when they do not, and how wide
//! the result is.

mod graphemes;
mod latin1;
mod utf8;
mod width;
