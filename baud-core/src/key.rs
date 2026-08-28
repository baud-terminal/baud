use crate::*;

/// `ESC`. VT52's introducer is the bare escape, which is not a C1 control and
/// so does not come from `self.c1` -- `S8C1T` cannot move it, because a VT52
/// has no 8-bit forms to move it to.
const ESC: u8 = 0x1b;

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
        // **VT52 is a different keyboard, not a different introducer.**
        // `[[source: ctlseqs]]` line 2698: "A VT52 does not have function
        // keys, but it does have a numeric keypad and cursor keys." So this
        // is a separate table rather than a prefix substitution, and the keys
        // a VT52 does not have send nothing -- inventing a form for Insert
        // there would be answering with a shape no application asked for.
        if self.terminal_mode() == TerminalMode::Vt52 {
            return self.vt52_key(e.code, e.mods);
        }
        match e.code {
            // The arrows plus Home and End: `[[source: ctlseqs]]` line 2485
            // puts the latter in the arrows' table and calls them cursor keys,
            // so DECCKM governs all six. The finals live on `KeyCode` with the
            // table they came from.
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::Right
            | KeyCode::Left
            | KeyCode::Home
            | KeyCode::End => match e.code.cursor_final() {
                Some(f) => self.cursor_key(f, e.mods),
                None => return false,
            },
            // The editing keypad. Unlike the cursor keys these do not change
            // with DECCKM -- `[[source: ctlseqs]]` line 2672 gives the same
            // sequence in both columns -- and their parameters live on
            // `KeyCode::tilde_param` with that table.
            //
            // Home and End are sent PC-style above, from line 2485. Line 2672
            // also gives VT220 forms, `CSI 1 ~` and `CSI 4 ~`, and those are an
            // *alternative* reached through `sunKeyboard` rather than an
            // addition: sending both would be sending two different keys.
            KeyCode::Insert | KeyCode::Delete | KeyCode::PageUp | KeyCode::PageDown => {
                match e.code.tilde_param() {
                    Some(p) => self.tilde_key(p, e.mods),
                    None => return false,
                }
            }
            KeyCode::Function(f) => self.function_key(f, e.mods),
            KeyCode::Keypad(k) => return self.keypad_key(k, e.mods),
            KeyCode::Char(cp) => return self.text_key(cp, e.mods),
            // `[[source: ctlseqs]]` line 253: BS is Ctrl-H. Which byte the
            // *Backspace key* sends is a different question -- DECBKM (`?67`)
            // chooses, and baud does not implement it, so this is xterm's
            // default with `backarrowKey` false. `docs/input-coverage.md`
            // records the mode as absent rather than the key as wrong.
            KeyCode::Backspace => self.emit(&[0x7f]),
            KeyCode::Escape => self.emit(&[0x1b]),
            KeyCode::Tab => match e.mods.has(Modifiers::SHIFT) {
                // CBT. Shift-Tab is the one text key that is a sequence, and
                // it is `CSI Z` rather than a modified HT.
                true => {
                    let csi = self.c1.csi();
                    self.emit(csi);
                    self.emit(b"Z");
                }
                false => self.emit(&[0x09]),
            },
            KeyCode::Enter => {
                // LNM. `[[source: ctlseqs]]` makes the mode govern what LF
                // *does* on output; on input it is what Return sends, and the
                // two are the same question asked from either end.
                self.emit(&[0x0d]);
                if self.modes.is_set(ModeBit::LineFeedNewLine) {
                    self.emit(&[0x0a]);
                }
            }
        }
        true
    }

    /// The VT52 keyboard: `[[source: ctlseqs]]` lines 2703 and 2713.
    ///
    /// Two tables and both are short. The cursor keys are `ESC A`-`ESC D` and
    /// **do not change with mode** -- the specification says so in the same
    /// sentence that introduces them, and DECCKM is an ANSI mode that a VT52
    /// cannot be in. The keypad's application column is `ESC ? <final>`, which
    /// is the one place in this encoder where the introducer is three bytes.
    ///
    /// `PF1`-`PF4` are `ESC P`-`ESC S` in both columns, the same four finals
    /// the ANSI side spells `SS3 P`-`SS3 S`. That is the shape of the whole
    /// difference: same finals, VT52's own introducers.
    ///
    /// **Modifiers are dropped rather than encoded.** ctlseqs gives no
    /// modified form for any VT52 key, and there is nowhere to put one -- the
    /// forms carry no parameters. Control still reaches the application
    /// through [`Term::text_key`], because folding a character into its
    /// control code is what the keyboard does rather than what the protocol
    /// encodes.
    fn vt52_key(&mut self, code: KeyCode, mods: Modifiers) -> bool {
        match code {
            // The same four finals the ANSI side sends, under VT52's own
            // introducer. `cursor_final` carries them with their table.
            KeyCode::Up | KeyCode::Down | KeyCode::Right | KeyCode::Left => {
                match code.cursor_final() {
                    Some(f) => self.vt52_esc(f),
                    None => return false,
                }
            }
            KeyCode::Keypad(k) => return self.vt52_keypad(k),
            // The text keys are the keyboard, not the emulation.
            KeyCode::Char(cp) => return self.text_key(cp, mods),
            KeyCode::Backspace => self.emit(&[0x7f]),
            KeyCode::Escape => self.emit(&[0x1b]),
            // No CBT on a VT52: `CSI Z` is an ANSI sequence, and Shift-Tab
            // has no VT52 form, so Tab is HT whatever is held.
            KeyCode::Tab => self.emit(&[0x09]),
            KeyCode::Enter => {
                self.emit(&[0x0d]);
                if self.modes.is_set(ModeBit::LineFeedNewLine) {
                    self.emit(&[0x0a]);
                }
            }
            // Home, End, the editing keypad and the function keys. A VT52 has
            // none of them and ctlseqs gives no form, so nothing is sent.
            _ => return false,
        }
        true
    }

    /// `ESC <final>`, the VT52 cursor keys.
    fn vt52_esc(&mut self, final_byte: FinalByte) {
        self.emit(&[ESC]);
        self.emit_byte(final_byte.byte());
    }

    /// The VT52 keypad, `[[source: ctlseqs]]` line 2713.
    fn vt52_keypad(&mut self, k: KeypadKey) -> bool {
        // `ESC P`-`ESC S` in both columns, so the mode is not consulted.
        // `ESC P`-`ESC S` in both columns, so the mode is not consulted.
        // Same four finals as the ANSI side's `SS3 P`-`SS3 S`.
        if let Some(f) = k.pf().and_then(FnKey::ss3_final) {
            self.vt52_esc(f);
            return true;
        }
        if self.keypad() == KeypadMode::Numeric {
            return match k {
                KeypadKey::Enter => {
                    self.emit(&[0x0d]);
                    if self.modes.is_set(ModeBit::LineFeedNewLine) {
                        self.emit(&[0x0a]);
                    }
                    true
                }
                _ => match k.numeric() {
                    Some(b) => {
                        self.emit_byte(b);
                        true
                    }
                    None => false,
                },
            };
        }
        // `ESC ? <final>`. The finals are the VT220 table's, not the PC one's:
        // `2` is `ESC ? r`, where the ANSI PC keypad sends `CSI B`. A VT52
        // keypad has no cursor duties to borrow.
        match k.vt52_byte() {
            Some(b) => {
                self.emit(&[ESC, b'?']);
                self.emit_byte(b);
                true
            }
            None => false,
        }
    }

    /// The keypad, `[[source: ctlseqs]]` line 2498.
    ///
    /// **The PC-style table, not the VT220 one.** ctlseqs gives both -- line
    /// 2498 for a Sun/PC keypad and line 2628 for the VT102/VT220 keypad
    /// reached through `sunKeyboard` -- and they disagree on most of the
    /// twenty-four rows. The exact count is derived from the two tables by
    /// `the_vt220_keypad_is_a_different_table_and_is_not_what_baud_sends`
    /// rather than written here: a number restated in a comment is one that
    /// goes quietly wrong if a later edition of ctlseqs reconciles them. baud sends the first, which is the same choice
    /// already made for Home and End, and for the same reason: sending both
    /// would be sending two different keys, and xterm's default is the one
    /// applications are built against.
    ///
    /// The two columns are chosen by DECKPAM/DECKPNM. The numeric column is
    /// the literal character in every row but `Enter` and the `PF` keys, and
    /// the application column is **not uniformly `SS3`** -- `2` is `CSI B`,
    /// `0` is `CSI 2 ~`, `1` is `SS3 F`. A table that assumed one introducer
    /// would be wrong in eight rows and wrong in a way each application would
    /// interpret as a different keystroke rather than as an error.
    fn keypad_key(&mut self, k: KeypadKey, mods: Modifiers) -> bool {
        // `PF1`-`PF4` are `SS3 P`-`SS3 S` in **both** columns: the only rows
        // the mode does not change. They are the same four sequences F1-F4
        // send, so they take the same path and the `SS3` -> `CSI 1 ; mod`
        // switch under a modifier is written once.
        if let Some(f) = k.pf() {
            self.function_key(f, mods);
            return true;
        }
        if self.keypad() == KeypadMode::Numeric {
            // Enter is the one row whose numeric form is not its label: `CR`,
            // and LNM governs it exactly as it governs the main Return.
            if k == KeypadKey::Enter {
                self.emit_return();
                return true;
            }
            return match k.numeric() {
                Some(b) => {
                    self.emit_byte(b);
                    true
                }
                None => false,
            };
        }
        // The rows that are cursor keys in disguise, whose introducer is fixed
        // by the table rather than by DECCKM.
        if let Some((intro, f)) = k.cursor_shaped() {
            self.cursor_shaped(intro, f, mods);
            return true;
        }
        // The editing-keypad rows: same parameters as the standalone keys.
        if let Some(p) = k.tilde_param() {
            self.tilde_key(p, mods);
            return true;
        }
        // Everything else is `SS3 <byte>`.
        //
        // **No modifier form, and none is invented.** ctlseqs gives the
        // `CSI 1 ; mod` shape for the cursor and function keys and gives
        // nothing for these, so a modified `SS3 j` would be a sequence baud
        // made up. Sending the unmodified form is what a terminal does;
        // sending an invented one is answering with the wrong shape.
        match k.pc_application_byte() {
            Some(b) => {
                self.emit_introducer(Introducer::Ss3);
                self.emit_byte(b);
                true
            }
            None => false,
        }
    }

    /// `CR`, and `LF` too under LNM.
    ///
    /// `[[source: ctlseqs]]` makes the mode govern what LF *does* on output;
    /// on input it is what Return sends, and the two are the same question
    /// asked from either end. Shared by Return and the keypad's Enter, which
    /// are different keys with the same answer.
    fn emit_return(&mut self) {
        self.emit(&[0x0d]);
        if self.modes.is_set(ModeBit::LineFeedNewLine) {
            self.emit(&[0x0a]);
        }
    }

    /// A key that produces text.
    ///
    /// Two modifiers change the bytes rather than adding a parameter, and this
    /// layer has no way to say "Control was held" other than by sending a
    /// different character -- which is exactly the ambiguity
    /// `[[source: fixterms]]` exists to remove, and why `CSI u` is the next
    /// layer rather than a nicety.
    fn text_key(&mut self, cp: Codepoint, mods: Modifiers) -> bool {
        // Control folds the character into `0x00..0x1f`: `@` through `_` and
        // the lowercase letters differ from their control code by bit 6 alone.
        // Anything else has no control form and Control is simply dropped,
        // which is what a terminal does and what an application cannot detect.
        let base = match mods.has(Modifiers::CTRL) {
            true => match cp.value() {
                v @ (0x40..=0x5f | 0x61..=0x7a) => u8::try_from(v & 0x1f).ok(),
                0x20 => Some(0x00),
                0x3f => Some(0x7f),
                _ => None,
            },
            false => None,
        };

        // `[[source: ctlseqs]]` line 2325 gives two ways for either modifier to
        // apply -- prefix `ESC`, or add 128 -- and line 2337's table has a
        // separate column per modifier: `?1039` for Alt, `?1036` for Meta.
        // They are asked independently here because four of that table's
        // sixteen rows disagree between them.
        //
        // The table's other branch -- `shift`, meaning add 128 -- is `?1034`
        // eightBitInput, and it applies only under ISO 8859-1. Under UTF-8 a
        // byte above 127 is a continuation byte rather than a character, so
        // shifting there would send something no application can decode.
        let modded = mods.has(Modifiers::ALT) || mods.has(Modifiers::META);
        let esc = (mods.has(Modifiers::ALT) && self.alt_sends_escape())
            || (mods.has(Modifiers::META) && self.meta_sends_escape());
        // Once, not once per modifier: `ESC ESC x` is Escape then Alt-x.
        if esc {
            self.emit(&[0x1b]);
        }
        // The prefix wins where both modes are set, which is what the table's
        // last four rows say: with both resources ON every modified row is
        // `ESC x`, and none of them is the shifted form.
        let shift = modded
            && !esc
            && self.eight_bit_input()
            && self.input_encoding() == InputEncoding::Latin1;

        // A control code is always one byte and always below 128, so the shift
        // is unconditional here. `Meta-Ctrl-A` becomes `0x81`, which under
        // ISO 8859-1 is a C1 control -- and baud accepts 8-bit C1 controls in
        // exactly that encoding, so the two halves agree.
        if let Some(b) = base {
            self.emit(&[b | if shift { 0x80 } else { 0 }]);
            return true;
        }

        let v = cp.value();
        // **Under ISO 8859-1 the keyboard sends ISO 8859-1.** `ESC % @`
        // switches how baud *decodes*, and an encoder that kept emitting UTF-8
        // would break the round trip that makes the mode useful: the
        // application set the encoding, and would read `é` back as two
        // characters. Symmetric with `?1020`, which reports the live encoding
        // rather than a compile-time one.
        if self.input_encoding() == InputEncoding::Latin1 {
            // One byte per character by definition. Above `U+00FF` there is no
            // byte, and no key on a Latin-1 keyboard produces one -- so this
            // sends nothing rather than sending UTF-8 the application would
            // misread. Same rule as answering a query with the wrong shape.
            let Ok(b) = u8::try_from(v) else {
                return false;
            };
            // `[[source: ctlseqs]]` line 2331: "shift the key from codes 0-127
            // to 128-255". A character already above 127 has no shifted form.
            self.emit(&[b | if shift && b < 0x80 { 0x80 } else { 0 }]);
            return true;
        }

        let Some(c) = char::from_u32(v) else {
            return false;
        };
        let mut buf = [0u8; 4];
        let bytes = c.encode_utf8(&mut buf).as_bytes();
        // `emit` borrows self mutably; the buffer is local, so copy the length
        // out first rather than holding the borrow.
        let n = bytes.len();
        let mut owned = [0u8; 4];
        owned[..n].copy_from_slice(bytes);
        self.emit(&owned[..n]);
        true
    }

    /// `SS3 P` for F1-F4, `CSI n ~` for the rest.
    fn function_key(&mut self, f: FnKey, mods: Modifiers) {
        // F1-F4 share the cursor keys' shape: a single final byte after `SS3`,
        // and `CSI 1 ; mod` when modified. **`SS3` unconditionally when
        // unmodified -- DECCKM does not reach here.** Routing these through
        // `cursor_key` was wrong and the specification's own table caught it:
        // that function picks its introducer from `cursor_keys_application`,
        // so F1 came out as `CSI P` in normal mode where line 2529 says
        // `SS3 P` flatly. A cursor key is the only thing DECCKM governs.
        if let Some(final_byte) = f.ss3_final() {
            self.cursor_shaped(Introducer::Ss3, final_byte, mods);
            return;
        }
        // The gaps in the `~` parameters -- 16 and 22 skipped, then 27 and 30
        // -- live on `FnKey::tilde_param`, with the tables they came from.
        if let Some(p) = f.tilde_param() {
            self.tilde_key(p, mods);
        }
    }

    /// `CSI n ~`, or `CSI n ; mod ~` when modified.
    ///
    /// The editing keys and F5 upward share this shape, and unlike the cursor
    /// keys they do not change with DECCKM -- `[[source: ctlseqs]]` line 2672
    /// gives the same sequence in both columns.
    fn tilde_key(&mut self, param: Param, mods: Modifiers) {
        self.emit_introducer(Introducer::Csi);
        self.emit_param(param);
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
    fn cursor_key(&mut self, final_byte: FinalByte, mods: Modifiers) {
        let intro = match self.cursor_keys_application() {
            true => Introducer::Ss3,
            false => Introducer::Csi,
        };
        self.cursor_shaped(intro, final_byte, mods);
    }

    /// The same shape with the introducer supplied rather than read.
    ///
    /// **The keypad needs this and must not call [`Term::cursor_key`].** Its
    /// application column is chosen by DECKPAM, and keypad `1` is `SS3 F`
    /// there while keypad `2` is `CSI B` -- fixed per row, whatever DECCKM
    /// says. Reading DECCKM here sent `CSI F` for a key the table spells
    /// `SS3 F`, which is the same mistake F1-F4 made before the
    /// specification's own table caught it.
    ///
    /// The introducer is an [`Introducer`] and not a `bool` for exactly that
    /// reason: the two call sites pick it by different rules, and a `bool`
    /// records neither.
    fn cursor_shaped(&mut self, intro: Introducer, final_byte: FinalByte, mods: Modifiers) {
        if mods.is_none() {
            self.emit_introducer(intro);
            self.emit_byte(final_byte.byte());
            return;
        }
        // A modified key uses `CSI` even in application mode: `SS3` takes no
        // parameters, so there is nowhere to put the modifier, and an encoder
        // that kept it would emit something no application parses.
        self.emit_introducer(Introducer::Csi);
        self.emit(b"1;");
        self.emit_dec(mods.wire());
        self.emit_byte(final_byte.byte());
    }

    fn emit_introducer(&mut self, i: Introducer) {
        let b = match i {
            Introducer::Csi => self.c1.csi(),
            Introducer::Ss3 => self.c1.ss3(),
        };
        self.emit(b);
    }

    fn emit_byte(&mut self, b: StreamByte) {
        self.emit(&[b.raw_byte()]);
    }
}
