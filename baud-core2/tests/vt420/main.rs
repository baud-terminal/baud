//! VT420 — left and right margins, and the column operations that ride on them.
//!
//! DECSLRM, DECIC and DECDC. Small enough to be one module today; it is its own
//! directory because the protocol level is, and because `CSI s` is ambiguous
//! between SCOSC and DECSLRM in a way that belongs filed under VT420.

mod side_margins;
