//! xterm's XTGETTCAP table, compiled by `build/tcap.rs` from
//! `[[source: xterm-xtermcap-c]]`: each of xterm's 319 names, terminfo and
//! termcap, at the first row that names it, as xterm's `lookupTcapByName`
//! finds it, with the key baud's encoder takes for that row. X11 licence
//! (`LICENSES/xterm-COPYING.txt`).

include!(concat!(env!("OUT_DIR"), "/tcap_data.rs"));
