//! Encoding a key press into the bytes an application reads.
//!
//! **Here rather than in a core, because it is a pure function of the key and
//! five pieces of mode state.** It lived in `baud-core2` and nothing in it
//! touched the grid, the arena or the cursor; porting it into `baud-core`
//! would have been a second spelling of the input protocol, which is what a
//! hand-written CSI recogniser beside the table already cost this repo once.
//! Both cores call this, the way both call [`Csi::parse_with`].
//!
//! The output is a value rather than a sink, mirroring [`Report::encode`]: a
//! key is at most a dozen bytes, so returning them lets the caller put them
//! wherever its replies already go and keeps this crate free of the borrow.

use crate::*;

/// `ESC`. VT52's introducer is the bare escape, which is not a C1 control and
/// so does not come from `c1` -- `S8C1T` cannot move it, because a VT52 has no
/// 8-bit forms to move it to.
const ESC: u8 = 0x1b;

/// Everything the encoder reads, and nothing else.
///
/// **Five fields and not eight, because `Modes` already carries four of
/// them.** DECCKM, LNM, `?1036` metaSendsEscape, `?1039` altSendsEscape and
/// `?1034` eightBitInput are all `ModeBit`s, so they travel as one word and
/// this struct needs no `bool` -- which `audit_all.sh` gates this crate at
/// **zero** of, on the grounds that a `bool` field is always a missing
/// two-state enum.
#[derive(Clone, Copy)]
pub struct KeyState {
    pub modes: Modes,
    pub keypad: KeypadMode,
    /// Whether `CSI` and `SS3` go out as one byte or two. A key sequence is
    /// terminal-to-application, so `S8C1T` governs it exactly as it governs a
    /// report.
    pub c1: C1Transmit,
    pub terminal: TerminalMode,
    /// What the *keyboard* sends, not what the parser decodes -- but they are
    /// the same setting, and that is the point. `ESC % @` switches decoding to
    /// ISO 8859-1, and an encoder that kept emitting UTF-8 would break the
    /// round trip that makes the mode useful.
    pub encoding: InputEncoding,
}

/// The bytes one key press produces.
pub struct KeyBytes {
    buf: [u8; KeyBytes::MAX_LEN],
    len: ByteLen,
    /// Whether every byte fit. It cannot fail -- see [`KeyBytes::MAX_LEN`] --
    /// and if it ever does the key is dropped whole rather than half-sent,
    /// which is the same rule the reply buffer follows: a truncated sequence
    /// is a wrong answer, and an application reads its first byte as the start
    /// of something that never arrives.
    fit: Fit,
}

/// Whether a key's bytes all fit in [`KeyBytes`].
///
/// **A two-state enum and not a `bool`**, because `audit_all.sh` gates this
/// crate at zero `bool` fields on the grounds that one is always a missing
/// enum -- and this is the case in point: `full: bool` read as "the buffer is
/// full", which is the opposite of the state it marked.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Whole,
    Overflowed,
}

impl KeyBytes {
    /// The longest key this can produce, with the arithmetic rather than a
    /// guess: an 8-bit `CSI` is 1 byte and a 7-bit one 2, the parameter is at
    /// most 5 digits, the modifier is `;` plus 2, and the final is 1 -- so
    /// `CSI 65535 ; 16 ~` is 11. The other long form is `ESC` plus a four-byte
    /// UTF-8 character, which is 5.
    pub const MAX_LEN: usize = 12;

    fn new() -> KeyBytes {
        KeyBytes {
            buf: [0; KeyBytes::MAX_LEN],
            len: ByteLen::ZERO,
            fit: Fit::Whole,
        }
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len.index()]
    }
    fn push(&mut self, b: u8) {
        match self.len.index() < self.buf.len() {
            true => {
                self.buf[self.len.index()] = b;
                self.len = ByteLen::from_index(self.len.index() + 1);
            }
            false => self.fit = Fit::Overflowed,
        }
    }
    fn all(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.push(*b);
        }
    }
    fn byte(&mut self, b: StreamByte) {
        self.push(b.raw_byte());
    }
    fn num(&mut self, v: u16) {
        for b in Decimal::of(v).as_bytes() {
            self.push(*b);
        }
    }
    fn introducer(&mut self, i: Introducer, c1: C1Transmit) {
        self.all(match i {
            Introducer::Csi => c1.csi(),
            Introducer::Ss3 => c1.ss3(),
        });
    }
    /// Emit `bytes`, with a single `ESC` in front if a prefix modifier applied.
    ///
    /// One call site per outcome, so the prefix cannot be written for a key
    /// that then turns out to be unencodable. Once, not once per modifier:
    /// `ESC ESC x` is Escape followed by Alt-x, a different keystroke.
    fn prefixed(&mut self, esc: Prefix, bytes: &[u8]) {
        if esc == Prefix::Escape {
            self.push(ESC);
        }
        self.all(bytes);
    }
    /// `CR`, and `LF` too under LNM.
    ///
    /// `[[source: ctlseqs]]` makes the mode govern what LF *does* on output;
    /// on input it is what Return sends, and the two are the same question
    /// asked from either end. Shared by Return and the keypad's Enter, which
    /// are different keys with the same answer.
    fn ret(&mut self, s: KeyState) {
        self.push(0x0d);
        if s.modes.is_set(ModeBit::LineFeedNewLine) {
            self.push(0x0a);
        }
    }
}

/// Encode a key press.
///
/// `None` is "this key sends nothing", which is a real answer rather than a
/// failure: a VT52 has no Insert key, ctlseqs gives no modified form for
/// `SS3 j`, and a codepoint above `U+00FF` has no ISO 8859-1 byte. Inventing a
/// sequence for any of them would be answering with a shape no application
/// asked for, which this project refuses on the output side too.
///
/// **The base layer only.** `docs/input-coverage.md` names three, and this is
/// the one every terminal implements: an application that has not asked for an
/// enhancement must receive what it would get from any other terminal, or it
/// breaks. The same reasoning that makes `DA` under-claim.
/// `[[source: fixterms]]`'s `CSI u` form and `[[source: kitty-keyboard]]`'s
/// progressive enhancement are negotiated, and neither is implemented yet.
pub fn encode_key(e: KeyEvent, s: KeyState) -> Option<KeyBytes> {
    // Only presses exist on this layer. `[[source: kitty-keyboard]]` is what
    // reports releases and repeats, and an application that has not enabled it
    // must not start seeing them.
    if e.kind != KeyEventKind::Press {
        return None;
    }
    let mut o = KeyBytes::new();
    // **VT52 is a different keyboard, not a different introducer.**
    // `[[source: ctlseqs]]` line 2698: "A VT52 does not have function keys,
    // but it does have a numeric keypad and cursor keys." So this is a
    // separate table rather than a prefix substitution, and the keys a VT52
    // does not have send nothing.
    let ok = match s.terminal {
        TerminalMode::Vt52 => vt52(&mut o, e.code, e.mods, s),
        TerminalMode::Ansi => ansi(&mut o, e.code, e.mods, s),
    };
    match ok && o.fit == Fit::Whole {
        true => Some(o),
        false => None,
    }
}

fn ansi(o: &mut KeyBytes, code: KeyCode, mods: Modifiers, s: KeyState) -> bool {
    match code {
        // The arrows plus Home and End: `[[source: ctlseqs]]` line 2485 puts
        // the latter in the arrows' table and calls them cursor keys, so
        // DECCKM governs all six.
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::Right
        | KeyCode::Left
        | KeyCode::Home
        | KeyCode::End => match code.cursor_final() {
            Some(f) => cursor_key(o, f, mods, s),
            None => return false,
        },
        // The editing keypad. Unlike the cursor keys these do not change with
        // DECCKM -- `[[source: ctlseqs]]` line 2672 gives the same sequence in
        // both columns.
        //
        // Home and End are sent PC-style above, from line 2485. Line 2672 also
        // gives VT220 forms, `CSI 1 ~` and `CSI 4 ~`, and those are an
        // *alternative* reached through `sunKeyboard` rather than an addition:
        // sending both would be sending two different keys.
        KeyCode::Insert | KeyCode::Delete | KeyCode::PageUp | KeyCode::PageDown => {
            match code.tilde_param() {
                Some(p) => tilde_key(o, p, mods, s),
                None => return false,
            }
        }
        KeyCode::Function(f) => function_key(o, f, mods, s),
        KeyCode::Keypad(k) => return keypad_key(o, k, mods, s),
        KeyCode::Char(cp) => return text_key(o, cp, mods, s),
        // `[[source: ctlseqs]]` line 253: BS is Ctrl-H. Which byte the
        // *Backspace key* sends is a different question -- DECBKM (`?67`)
        // chooses, and baud does not implement it, so this is xterm's default
        // with `backarrowKey` false.
        KeyCode::Backspace => o.push(0x7f),
        KeyCode::Escape => o.push(0x1b),
        KeyCode::Tab => match mods.has(Modifiers::SHIFT) {
            // CBT. Shift-Tab is the one text key that is a sequence, and it is
            // `CSI Z` rather than a modified HT.
            true => {
                o.introducer(Introducer::Csi, s.c1);
                o.all(b"Z");
            }
            false => o.push(0x09),
        },
        KeyCode::Enter => o.ret(s),
    }
    true
}

/// The VT52 keyboard: `[[source: ctlseqs]]` lines 2703 and 2713.
///
/// Two tables and both are short. The cursor keys are `ESC A`-`ESC D` and
/// **do not change with mode** -- the specification says so in the same
/// sentence that introduces them, and DECCKM is an ANSI mode a VT52 cannot be
/// in. The keypad's application column is `ESC ? <final>`, the one place in
/// this encoder where the introducer is three bytes.
///
/// **Modifiers are dropped rather than encoded.** ctlseqs gives no modified
/// form for any VT52 key and there is nowhere to put one -- the forms carry no
/// parameters. Control still reaches the application through [`text_key`],
/// because folding a character into its control code is what the keyboard does
/// rather than what the protocol encodes.
fn vt52(o: &mut KeyBytes, code: KeyCode, mods: Modifiers, s: KeyState) -> bool {
    match code {
        KeyCode::Up | KeyCode::Down | KeyCode::Right | KeyCode::Left => match code.cursor_final() {
            Some(f) => vt52_esc(o, f),
            None => return false,
        },
        KeyCode::Keypad(k) => return vt52_keypad(o, k, s),
        // The text keys are the keyboard, not the emulation.
        KeyCode::Char(cp) => return text_key(o, cp, mods, s),
        KeyCode::Backspace => o.push(0x7f),
        KeyCode::Escape => o.push(0x1b),
        // No CBT on a VT52: `CSI Z` is an ANSI sequence, so Tab is HT whatever
        // is held.
        KeyCode::Tab => o.push(0x09),
        KeyCode::Enter => o.ret(s),
        // Home, End, the editing keypad and the function keys. A VT52 has none
        // of them and ctlseqs gives no form, so nothing is sent.
        _ => return false,
    }
    true
}

/// `ESC <final>`, the VT52 cursor keys.
fn vt52_esc(o: &mut KeyBytes, final_byte: FinalByte) {
    o.push(ESC);
    o.byte(final_byte.byte());
}

/// The VT52 keypad, `[[source: ctlseqs]]` line 2713.
fn vt52_keypad(o: &mut KeyBytes, k: KeypadKey, s: KeyState) -> bool {
    // `ESC P`-`ESC S` in both columns, so the mode is not consulted. Same four
    // finals as the ANSI side's `SS3 P`-`SS3 S`.
    if let Some(f) = k.pf().and_then(FnKey::ss3_final) {
        vt52_esc(o, f);
        return true;
    }
    if s.keypad == KeypadMode::Numeric {
        return match k {
            KeypadKey::Enter => {
                o.ret(s);
                true
            }
            _ => match k.numeric() {
                Some(b) => {
                    o.byte(b);
                    true
                }
                None => false,
            },
        };
    }
    // `ESC ? <final>`. The finals are the VT220 table's, not the PC one's: `2`
    // is `ESC ? r`, where the ANSI PC keypad sends `CSI B`. A VT52 keypad has
    // no cursor duties to borrow.
    match k.vt52_byte() {
        Some(b) => {
            o.all(&[ESC, b'?']);
            o.byte(b);
            true
        }
        None => false,
    }
}

/// The keypad, `[[source: ctlseqs]]` line 2498.
///
/// **The PC-style table, not the VT220 one.** ctlseqs gives both -- line 2498
/// for a Sun/PC keypad and line 2628 for the VT102/VT220 keypad reached
/// through `sunKeyboard` -- and they disagree on most of the twenty-four rows.
/// The exact count is derived from the two tables by
/// `the_vt220_keypad_is_a_different_table_and_is_not_what_baud_sends` rather
/// than written here: a number restated in a comment is one that goes quietly
/// wrong if a later edition of ctlseqs reconciles them. baud sends the first,
/// the same choice already made for Home and End and for the same reason.
///
/// The two columns are chosen by DECKPAM/DECKPNM. The numeric column is the
/// literal character in every row but `Enter` and the `PF` keys, and the
/// application column is **not uniformly `SS3`** -- `2` is `CSI B`, `0` is
/// `CSI 2 ~`, `1` is `SS3 F`. A table that assumed one introducer would be
/// wrong in eight rows, and wrong in a way each application would interpret as
/// a different keystroke rather than as an error.
fn keypad_key(o: &mut KeyBytes, k: KeypadKey, mods: Modifiers, s: KeyState) -> bool {
    // `PF1`-`PF4` are `SS3 P`-`SS3 S` in **both** columns: the only rows the
    // mode does not change. They are the same four sequences F1-F4 send, so
    // they take the same path and the `SS3` -> `CSI 1 ; mod` switch under a
    // modifier is written once.
    if let Some(f) = k.pf() {
        function_key(o, f, mods, s);
        return true;
    }
    if s.keypad == KeypadMode::Numeric {
        // Enter is the one row whose numeric form is not its label: `CR`, and
        // LNM governs it exactly as it governs the main Return.
        if k == KeypadKey::Enter {
            o.ret(s);
            return true;
        }
        return match k.numeric() {
            Some(b) => {
                o.byte(b);
                true
            }
            None => false,
        };
    }
    // The rows that are cursor keys in disguise, whose introducer is fixed by
    // the table rather than by DECCKM.
    if let Some((intro, f)) = k.cursor_shaped() {
        cursor_shaped(o, intro, f, mods, s);
        return true;
    }
    // The editing-keypad rows: same parameters as the standalone keys.
    if let Some(p) = k.tilde_param() {
        tilde_key(o, p, mods, s);
        return true;
    }
    // Everything else is `SS3 <byte>`.
    //
    // **No modifier form, and none is invented.** ctlseqs gives the
    // `CSI 1 ; mod` shape for the cursor and function keys and gives nothing
    // for these, so a modified `SS3 j` would be a sequence baud made up.
    match k.pc_application_byte() {
        Some(b) => {
            o.introducer(Introducer::Ss3, s.c1);
            o.byte(b);
            true
        }
        None => false,
    }
}

/// A key that produces text.
///
/// Two modifiers change the bytes rather than adding a parameter, and this
/// layer has no way to say "Control was held" other than by sending a
/// different character -- which is exactly the ambiguity
/// `[[source: fixterms]]` exists to remove, and why `CSI u` is the next layer
/// rather than a nicety.
fn text_key(o: &mut KeyBytes, cp: Codepoint, mods: Modifiers, s: KeyState) -> bool {
    // Control folds the character into `0x00..0x1f`: `@` through `_` and the
    // lowercase letters differ from their control code by bit 6 alone.
    // Anything else has no control form and Control is simply dropped, which
    // is what a terminal does and what an application cannot detect.
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
    // apply -- prefix `ESC`, or add 128 -- and line 2337's table has a separate
    // column per modifier: `?1039` for Alt, `?1036` for Meta. They are asked
    // independently because four of that table's sixteen rows disagree.
    //
    // The table's other branch -- `shift`, meaning add 128 -- is `?1034`
    // eightBitInput, and it applies only under ISO 8859-1. Under UTF-8 a byte
    // above 127 is a continuation byte rather than a character, so shifting
    // there would send something no application can decode.
    let modded = mods.has(Modifiers::ALT) || mods.has(Modifiers::META);
    let esc = match (mods.has(Modifiers::ALT) && s.modes.is_set(ModeBit::AltSendsEscape))
        || (mods.has(Modifiers::META) && s.modes.is_set(ModeBit::MetaSendsEscape))
    {
        true => Prefix::Escape,
        false => Prefix::None,
    };
    // **The prefix is emitted last, not here.** Emitting it before the
    // character is known to be encodable left a bare `ESC` in the reply when
    // the encoding could not represent the key -- `Alt` plus a codepoint above
    // `U+00FF` under ISO 8859-1 returned `false` having already written a
    // byte. An application reads that `ESC` as the start of a sequence that
    // never arrives. Found by `the_return_value_agrees_with_the_bytes`.
    //
    // The prefix wins where both modes are set, which is what the table's last
    // four rows say: with both resources ON every modified row is `ESC x`.
    let shift = modded
        && esc == Prefix::None
        && s.modes.is_set(ModeBit::EightBitInput)
        && s.encoding == InputEncoding::Latin1;

    // A control code is always one byte and always below 128, so the shift is
    // unconditional here. `Meta-Ctrl-A` becomes `0x81`, which under ISO 8859-1
    // is a C1 control -- and baud accepts 8-bit C1 controls in exactly that
    // encoding, so the two halves agree.
    if let Some(b) = base {
        o.prefixed(esc, &[b | if shift { 0x80 } else { 0 }]);
        return true;
    }

    let v = cp.value();
    // **Under ISO 8859-1 the keyboard sends ISO 8859-1.** `ESC % @` switches
    // how baud *decodes*, and an encoder that kept emitting UTF-8 would break
    // the round trip that makes the mode useful: the application set the
    // encoding, and would read `é` back as two characters. Symmetric with
    // `?1020`, which reports the live encoding rather than a compile-time one.
    if s.encoding == InputEncoding::Latin1 {
        // One byte per character by definition. Above `U+00FF` there is no
        // byte, and no key on a Latin-1 keyboard produces one -- so this sends
        // nothing rather than sending UTF-8 the application would misread.
        let Ok(b) = u8::try_from(v) else {
            return false;
        };
        // `[[source: ctlseqs]]` line 2331: "shift the key from codes 0-127 to
        // 128-255". A character already above 127 has no shifted form.
        o.prefixed(esc, &[b | if shift && b < 0x80 { 0x80 } else { 0 }]);
        return true;
    }

    let Some(c) = char::from_u32(v) else {
        return false;
    };
    let mut buf = [0u8; 4];
    let bytes = c.encode_utf8(&mut buf).as_bytes();
    o.prefixed(esc, bytes);
    true
}

/// `SS3 P` for F1-F4, `CSI n ~` for the rest.
fn function_key(o: &mut KeyBytes, f: FnKey, mods: Modifiers, s: KeyState) {
    // F1-F4 share the cursor keys' shape: a single final byte after `SS3`, and
    // `CSI 1 ; mod` when modified. **`SS3` unconditionally when unmodified --
    // DECCKM does not reach here.** Routing these through `cursor_key` was
    // wrong and the specification's own table caught it: that function picks
    // its introducer from DECCKM, so F1 came out as `CSI P` in normal mode
    // where line 2529 says `SS3 P` flatly. A cursor key is the only thing
    // DECCKM governs.
    if let Some(final_byte) = f.ss3_final() {
        cursor_shaped(o, Introducer::Ss3, final_byte, mods, s);
        return;
    }
    // The gaps in the `~` parameters -- 16 and 22 skipped, then 27 and 30 --
    // live on `FnKey::tilde_param`, with the tables they came from.
    if let Some(p) = f.tilde_param() {
        tilde_key(o, p, mods, s);
    }
}

/// `CSI n ~`, or `CSI n ; mod ~` when modified.
///
/// The editing keys and F5 upward share this shape, and unlike the cursor keys
/// they do not change with DECCKM -- `[[source: ctlseqs]]` line 2672 gives the
/// same sequence in both columns.
fn tilde_key(o: &mut KeyBytes, param: Param, mods: Modifiers, s: KeyState) {
    o.introducer(Introducer::Csi, s.c1);
    o.num(param.value());
    if !mods.is_none() {
        o.all(b";");
        o.num(u16::from(mods.wire()));
    }
    o.all(b"~");
}

/// `CSI A` / `SS3 A`, or `CSI 1 ; mod A` when modified.
///
/// Three rules from `[[source: ctlseqs]]`, and the third is easy to miss:
///
/// * DECCKM picks the introducer -- `CSI` in normal mode, `SS3` in application
///   mode (line 2473).
/// * A modifier is appended as `1 ; mod` before the final byte (line 2565),
///   `1` being the first parameter's default.
/// * **A modified key uses `CSI` even in application mode.** The specification
///   states it for F1-F4 at line 2564 and the same holds here: `SS3` takes no
///   parameters, so there is nowhere to put the modifier.
fn cursor_key(o: &mut KeyBytes, final_byte: FinalByte, mods: Modifiers, s: KeyState) {
    let intro = match s.modes.is_set(ModeBit::CursorKeys) {
        true => Introducer::Ss3,
        false => Introducer::Csi,
    };
    cursor_shaped(o, intro, final_byte, mods, s);
}

/// The same shape with the introducer supplied rather than read.
///
/// **The keypad needs this and must not call [`cursor_key`].** Its application
/// column is chosen by DECKPAM, and keypad `1` is `SS3 F` there while keypad
/// `2` is `CSI B` -- fixed per row, whatever DECCKM says. Reading DECCKM here
/// sent `CSI F` for a key the table spells `SS3 F`.
///
/// The introducer is an [`Introducer`] and not a `bool` for exactly that
/// reason: the two call sites pick it by different rules, and a `bool` records
/// neither.
fn cursor_shaped(
    o: &mut KeyBytes,
    intro: Introducer,
    final_byte: FinalByte,
    mods: Modifiers,
    s: KeyState,
) {
    if mods.is_none() {
        o.introducer(intro, s.c1);
        o.byte(final_byte.byte());
        return;
    }
    // A modified key uses `CSI` even in application mode: `SS3` takes no
    // parameters, so there is nowhere to put the modifier, and an encoder that
    // kept it would emit something no application parses.
    o.introducer(Introducer::Csi, s.c1);
    o.all(b"1;");
    o.num(u16::from(mods.wire()));
    o.byte(final_byte.byte());
}
