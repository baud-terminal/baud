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
            // PC-style, which is xterm's default. `[[source: ctlseqs]]` line
            // 2672 also gives VT220 forms -- `CSI 1 ~` and `CSI 4 ~` -- and
            // those are an *alternative*, reached through `sunKeyboard`, not an
            // addition. Sending both would be sending two different keys.
            KeyCode::Home => self.cursor_key(b'H', e.mods),
            KeyCode::End => self.cursor_key(b'F', e.mods),
            KeyCode::Insert => self.tilde_key(2, e.mods),
            KeyCode::Delete => self.tilde_key(3, e.mods),
            KeyCode::PageUp => self.tilde_key(5, e.mods),
            KeyCode::PageDown => self.tilde_key(6, e.mods),
            KeyCode::Function(f) => self.function_key(f, e.mods),
            _ => return false,
        }
        true
    }

    /// The parameter each function key carries, from `[[source: ctlseqs]]`.
    ///
    /// **The gaps are the point.** F5-F12 are 15, 17, 18, 19, 20, 21, 23, 24 --
    /// 16 and 22 are skipped -- and F13-F20 are 25, 26, 28, 29, 31, 32, 33, 34,
    /// skipping 27 and 30. A table written from memory as "15 upward" is wrong
    /// at F6 and wrong again at F11, and would send a neighbouring key rather
    /// than a missing one. `function_keys_match_the_specifications_tables`
    /// reads the tables at lines 2529 and 2685 and compares.
    ///
    /// F1-F4 are absent because they are not `~` keys at all: `SS3 P` through
    /// `SS3 S`, handled below.
    const FN_PARAM: [(u8, u8); 16] = [
        (5, 15),
        (6, 17),
        (7, 18),
        (8, 19),
        (9, 20),
        (10, 21),
        (11, 23),
        (12, 24),
        (13, 25),
        (14, 26),
        (15, 28),
        (16, 29),
        (17, 31),
        (18, 32),
        (19, 33),
        (20, 34),
    ];

    /// `SS3 P` for F1-F4, `CSI n ~` for the rest.
    fn function_key(&mut self, f: FnKey, mods: Modifiers) {
        // F1-F4 share the cursor keys' shape: a single final byte after `SS3`,
        // and `CSI 1 ; mod` when modified. That is the special case
        // `[[source: ctlseqs]]` line 2564 spells out, and
        // `FnKey::changes_introducer_when_modified` is the same rule.
        if f.changes_introducer_when_modified() {
            // **`SS3` unconditionally when unmodified -- DECCKM does not reach
            // here.** Routing these through `cursor_key` was wrong and the
            // specification's own table caught it: that function picks its
            // introducer from `cursor_keys_application`, so F1 came out as
            // `CSI P` in normal mode where line 2529 says `SS3 P` flatly.
            // A cursor key is the only thing DECCKM governs.
            let final_byte = b'P' + f.number() - 1;
            if mods.is_none() {
                let ss3 = self.c1.ss3();
                self.emit(ss3);
                self.emit(&[final_byte]);
            } else {
                let csi = self.c1.csi();
                self.emit(csi);
                self.emit(b"1;");
                self.emit_dec(mods.wire());
                self.emit(&[final_byte]);
            }
            return;
        }
        if let Some((_, p)) = Self::FN_PARAM.iter().find(|(n, _)| *n == f.number()) {
            self.tilde_key(*p, mods);
        }
    }

    /// `CSI n ~`, or `CSI n ; mod ~` when modified.
    ///
    /// The editing keys and F5 upward share this shape, and unlike the cursor
    /// keys they do not change with DECCKM -- `[[source: ctlseqs]]` line 2672
    /// gives the same sequence in both columns.
    fn tilde_key(&mut self, param: u8, mods: Modifiers) {
        let csi = self.c1.csi();
        self.emit(csi);
        self.emit_dec(param);
        if !mods.is_none() {
            self.emit(b";");
            self.emit_dec(mods.wire());
        }
        self.emit(b"~");
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
