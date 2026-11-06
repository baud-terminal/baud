//! The termcap keyboard's table, `?1050`, compiled by
//! `build/termcap_keys.rs`: each key of xterm's XTGETTCAP table that the
//! pinned xterm-256color entry gives a string, with its modifier
//! parameter, and the string of the key's first row that the entry has.
//! The strings are ncurses' xterm-256color entry's
//! (`LICENSES/ncurses-COPYING.txt`), the rows and their order xterm's
//! xtermcap.c (`LICENSES/xterm-COPYING.txt`).

include!(concat!(env!("OUT_DIR"), "/termcap_data.rs"));
