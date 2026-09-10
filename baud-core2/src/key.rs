use crate::*;
use baud_domain::{KeyState, encode_key};

impl Term<'_> {
    /// Encode a key press into the reply buffer.
    ///
    /// Returns whether anything was written. The reply buffer is the channel
    /// because that is where terminal-to-application bytes already go:
    /// `mouse_report` -- also an input event -- uses it, and a key and a mouse
    /// report travel the same pty in the same order.
    ///
    /// **The encoding itself is `baud_domain::encode_key`**, and this is the
    /// part that is a terminal's rather than the protocol's: reading the five
    /// modes off `Term` and putting the bytes where replies go. The 485 lines
    /// that used to be here touched no grid, no arena and no cursor, so
    /// keeping them in a core meant `baud-core` could only have them by
    /// spelling the input protocol a second time -- which is what a
    /// hand-written CSI recogniser beside the table already cost this repo.
    pub fn key(&mut self, e: KeyEvent) -> bool {
        let s = KeyState {
            modes: self.modes,
            keypad: self.keypad(),
            c1: self.c1,
            terminal: self.terminal_mode(),
            encoding: self.input_encoding(),
        };
        match encode_key(e, s) {
            Some(k) => {
                self.emit(k.as_bytes());
                true
            }
            None => false,
        }
    }
}
