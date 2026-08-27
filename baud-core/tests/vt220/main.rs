//! VT220 / VT320 — selective erase, status reports, and device control strings.
//!
//! What VT100 applications do not need but real ones do: DECSCA and the
//! selective erases, DECRQM, DECSCUSR, and the DCS layer carrying DECRQSS.

mod dcs;
mod decrqm_cursor_style;
mod replies;
mod selective_erase;
