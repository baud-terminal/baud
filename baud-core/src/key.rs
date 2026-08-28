use crate::*;

impl<'a> Term<'a> {
    /// Encode a key press into the reply buffer.
    ///
    /// Returns whether anything was written. The reply buffer is the channel
    /// because that is where terminal-to-application bytes already go:
    /// `mouse_report` -- also an input event -- uses it, and a key and a mouse
    /// report travel the same pty in the same order.
    ///
    /// **The base layer only.** `docs/input-coverage.md` names three, and this
    /// is the one every terminal implements: an application that has not asked
    /// for an enhancement must receive what it would get from any other
    /// terminal, or it breaks. The same reasoning that makes `DA` under-claim.
    /// `[[source: fixterms]]`'s `CSI u` form and `[[source: kitty-keyboard]]`'s
    /// progressive enhancement are negotiated, and neither is implemented yet.
    pub fn key(&mut self, e: KeyEvent) -> bool {
        // Only presses exist on this layer. `[[source: kitty-keyboard]]` is
        // what reports releases and repeats, and an application that has not
        // enabled it must not start seeing them.
        if e.kind != KeyEventKind::Press {
            return false;
        }
        match e.code {
            KeyCode::Up => self.cursor_key(b'A', e.mods),
            KeyCode::Down => self.cursor_key(b'B', e.mods),
            KeyCode::Right => self.cursor_key(b'C', e.mods),
            KeyCode::Left => self.cursor_key(b'D', e.mods),
            // `[[source: ctlseqs]]` line 2485 puts Home and End in the same
            // table as the arrows, with the same normal/application split.
            KeyCode::Home => self.cursor_key(b'H', e.mods),
            KeyCode::End => self.cursor_key(b'F', e.mods),
            _ => return false,
        }
        true
    }

    /// `CSI A` / `SS3 A`, or `CSI 1 ; mod A` when modified.
    ///
    /// Three rules from `[[source: ctlseqs]]`, and the third is the one that is
    /// easy to miss:
    ///
    /// * DECCKM picks the introducer -- `CSI` in normal mode, `SS3` in
    ///   application mode (line 2473).
    /// * A modifier is appended as `1 ; mod` before the final byte (line 2565),
    ///   `1` being the first parameter's default.
    /// * **A modified key uses `CSI` even in application mode.** The
    ///   specification states it for F1-F4 at line 2564 and the same holds
    ///   here: `SS3` takes no parameters, so there is nowhere to put the
    ///   modifier. An encoder that kept `SS3` would emit something no
    ///   application parses.
    fn cursor_key(&mut self, final_byte: u8, mods: Modifiers) {
        let app = self.cursor_keys_application();
        if mods.is_none() {
            let intro = match app {
                true => self.c1.ss3(),
                false => self.c1.csi(),
            };
            self.emit(intro);
            self.emit(&[final_byte]);
            return;
        }
        let csi = self.c1.csi();
        self.emit(csi);
        self.emit(b"1;");
        self.emit_dec(mods.wire());
        self.emit(&[final_byte]);
    }
}
