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
            KeyCode::Up => self.vt52_esc(b'A'),
            KeyCode::Down => self.vt52_esc(b'B'),
            KeyCode::Right => self.vt52_esc(b'C'),
            KeyCode::Left => self.vt52_esc(b'D'),
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
    fn vt52_esc(&mut self, final_byte: u8) {
        self.emit(&[0x1b, final_byte]);
    }

    /// The VT52 keypad, `[[source: ctlseqs]]` line 2713.
    fn vt52_keypad(&mut self, k: KeypadKey) -> bool {
        // `ESC P`-`ESC S` in both columns, so the mode is not consulted.
        if let Some(n) = match k {
            KeypadKey::Pf1 => Some(0u8),
            KeypadKey::Pf2 => Some(1),
            KeypadKey::Pf3 => Some(2),
            KeypadKey::Pf4 => Some(3),
            _ => None,
        } {
            self.emit(&[0x1b, b'P' + n]);
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
                _ => match Self::keypad_numeric(k) {
                    Some(b) => {
                        self.emit(&[b]);
                        true
                    }
                    None => false,
                },
            };
        }
        // `ESC ? <final>`. The finals are the VT220 table's, not the PC one's:
        // `2` is `ESC ? r`, where the ANSI PC keypad sends `CSI B`. A VT52
        // keypad has no cursor duties to borrow.
        match Self::vt52_keypad_final(k) {
            Some(f) => {
                self.emit(&[0x1b, b'?', f]);
                true
            }
            None => false,
        }
    }

    /// The application-column finals at line 2713.
    fn vt52_keypad_final(k: KeypadKey) -> Option<u8> {
        Some(match k {
            KeypadKey::Space => b' ',
            KeypadKey::Tab => b'I',
            KeypadKey::Enter => b'M',
            KeypadKey::Multiply => b'j',
            KeypadKey::Add => b'k',
            KeypadKey::Comma => b'l',
            KeypadKey::Minus => b'm',
            KeypadKey::Period => b'n',
            KeypadKey::Divide => b'o',
            KeypadKey::Digit0 => b'p',
            KeypadKey::Digit1 => b'q',
            KeypadKey::Digit2 => b'r',
            KeypadKey::Digit3 => b's',
            KeypadKey::Digit4 => b't',
            KeypadKey::Digit5 => b'u',
            KeypadKey::Digit6 => b'v',
            KeypadKey::Digit7 => b'w',
            KeypadKey::Digit8 => b'x',
            KeypadKey::Digit9 => b'y',
            KeypadKey::Equal => b'X',
            KeypadKey::Pf1 | KeypadKey::Pf2 | KeypadKey::Pf3 | KeypadKey::Pf4 => return None,
        })
    }

    /// The keypad, `[[source: ctlseqs]]` line 2498.
    ///
    /// **The PC-style table, not the VT220 one.** ctlseqs gives both -- line
    /// 2498 for a Sun/PC keypad and line 2628 for the VT102/VT220 keypad
    /// reached through `sunKeyboard` -- and they disagree on eleven of the
    /// twenty-four rows. baud sends the first, which is the same choice
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
        if self.keypad() == KeypadMode::Numeric {
            // Enter is the one row whose numeric form is not its label: `CR`,
            // and LNM governs it exactly as it governs the main Return.
            return match k {
                KeypadKey::Enter => {
                    self.emit(&[0x0d]);
                    if self.modes.is_set(ModeBit::LineFeedNewLine) {
                        self.emit(&[0x0a]);
                    }
                    true
                }
                // `PF1`-`PF4` are `SS3 P`-`SS3 S` in **both** columns. They
                // are the only rows the mode does not change.
                KeypadKey::Pf1 | KeypadKey::Pf2 | KeypadKey::Pf3 | KeypadKey::Pf4 => {
                    self.keypad_pf(k, mods)
                }
                _ => match Self::keypad_numeric(k) {
                    Some(b) => {
                        self.emit(&[b]);
                        true
                    }
                    None => false,
                },
            };
        }
        match k {
            KeypadKey::Pf1 | KeypadKey::Pf2 | KeypadKey::Pf3 | KeypadKey::Pf4 => {
                self.keypad_pf(k, mods)
            }
            // The rows that are cursor keys in disguise -- and whose
            // introducer is **fixed by the table**, not by DECCKM. The arrows
            // and Begin are `CSI`; End and Home are `SS3`. That is not a
            // pattern, it is four rows one way and two the other, which is
            // why each names its own.
            //
            // `CSI E` is CNL as an *output* operation and Begin as an input
            // one. Different directions, so not a conflict.
            KeypadKey::Digit2 => {
                self.cursor_shaped(b'B', mods, false);
                true
            }
            KeypadKey::Digit4 => {
                self.cursor_shaped(b'D', mods, false);
                true
            }
            KeypadKey::Digit6 => {
                self.cursor_shaped(b'C', mods, false);
                true
            }
            KeypadKey::Digit8 => {
                self.cursor_shaped(b'A', mods, false);
                true
            }
            KeypadKey::Digit5 => {
                self.cursor_shaped(b'E', mods, false);
                true
            }
            KeypadKey::Digit1 => {
                self.cursor_shaped(b'F', mods, true);
                true
            }
            KeypadKey::Digit7 => {
                self.cursor_shaped(b'H', mods, true);
                true
            }
            // The editing-keypad rows: same parameters as the standalone keys.
            KeypadKey::Digit0 => {
                self.tilde_key(2, mods);
                true
            }
            KeypadKey::Period => {
                self.tilde_key(3, mods);
                true
            }
            KeypadKey::Digit9 => {
                self.tilde_key(5, mods);
                true
            }
            KeypadKey::Digit3 => {
                self.tilde_key(6, mods);
                true
            }
            // Everything else is `SS3 <final>`.
            //
            // **No modifier form, and none is invented.** ctlseqs gives the
            // `CSI 1 ; mod` shape for the cursor and function keys and gives
            // nothing for these, so a modified `SS3 j` would be a sequence
            // baud made up. Sending the unmodified form is what a terminal
            // does; sending an invented one is answering with the wrong shape.
            _ => match Self::keypad_application_final(k) {
                Some(f) => {
                    let ss3 = self.c1.ss3();
                    self.emit(ss3);
                    self.emit(&[f]);
                    true
                }
                None => false,
            },
        }
    }

    /// `SS3 P`-`SS3 S`, identical in both keypad modes.
    fn keypad_pf(&mut self, k: KeypadKey, mods: Modifiers) -> bool {
        let n = match k {
            KeypadKey::Pf1 => 1,
            KeypadKey::Pf2 => 2,
            KeypadKey::Pf3 => 3,
            KeypadKey::Pf4 => 4,
            _ => return false,
        };
        // Reuses the F1-F4 path, so the `SS3` -> `CSI 1 ; mod` switch under a
        // modifier is written once. They are the same four sequences.
        match FnKey::new(n) {
            Some(f) => {
                self.function_key(f, mods);
                true
            }
            None => false,
        }
    }

    /// The numeric column: the literal character on the key.
    fn keypad_numeric(k: KeypadKey) -> Option<u8> {
        Some(match k {
            KeypadKey::Space => b' ',
            KeypadKey::Tab => 0x09,
            KeypadKey::Multiply => b'*',
            KeypadKey::Add => b'+',
            KeypadKey::Comma => b',',
            KeypadKey::Minus => b'-',
            KeypadKey::Period => b'.',
            KeypadKey::Divide => b'/',
            KeypadKey::Digit0 => b'0',
            KeypadKey::Digit1 => b'1',
            KeypadKey::Digit2 => b'2',
            KeypadKey::Digit3 => b'3',
            KeypadKey::Digit4 => b'4',
            KeypadKey::Digit5 => b'5',
            KeypadKey::Digit6 => b'6',
            KeypadKey::Digit7 => b'7',
            KeypadKey::Digit8 => b'8',
            KeypadKey::Digit9 => b'9',
            KeypadKey::Equal => b'=',
            KeypadKey::Enter
            | KeypadKey::Pf1
            | KeypadKey::Pf2
            | KeypadKey::Pf3
            | KeypadKey::Pf4 => {
                return None;
            }
        })
    }

    /// The `SS3 <final>` rows of the application column.
    fn keypad_application_final(k: KeypadKey) -> Option<u8> {
        Some(match k {
            KeypadKey::Space => b' ',
            KeypadKey::Tab => b'I',
            KeypadKey::Enter => b'M',
            KeypadKey::Multiply => b'j',
            KeypadKey::Add => b'k',
            KeypadKey::Comma => b'l',
            KeypadKey::Minus => b'm',
            KeypadKey::Divide => b'o',
            KeypadKey::Equal => b'X',
            _ => return None,
        })
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
        self.cursor_shaped(final_byte, mods, app);
    }

    /// The same shape with the introducer supplied rather than read.
    ///
    /// **The keypad needs this and must not call [`Term::cursor_key`].** Its
    /// application column is chosen by DECKPAM, and keypad `1` is `SS3 F`
    /// there while keypad `2` is `CSI B` -- fixed per row, whatever DECCKM
    /// says. Reading DECCKM here sent `CSI F` for a key the table spells
    /// `SS3 F`, which is the same mistake F1-F4 made before the
    /// specification's own table caught it.
    fn cursor_shaped(&mut self, final_byte: u8, mods: Modifiers, app: bool) {
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
