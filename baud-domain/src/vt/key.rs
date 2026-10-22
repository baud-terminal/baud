//! Encoding a key press into the bytes an application reads.
//!
//! **Here rather than in a core, because it is a pure function of the key and
//! the mode state in [`KeyState`].** It lived in `baud-core2` and nothing in it
//! touched the grid, the arena or the cursor; porting it into `baud-core`
//! would have been a second spelling of the input protocol, which is what a
//! hand-written CSI recogniser beside the table already cost this repo once.
//! `baud-core`'s `Term::key` calls this, the way its parser calls
//! [`Csi::parse_with`].
//!
//! The output is a value rather than a sink, mirroring [`Report::encode`]: a
//! key is at most 40 bytes ([`KeyBytes::MAX_LEN`] has the measurement), so
//! returning them lets the caller put them wherever its replies already go and
//! keeps this crate free of the borrow.

use crate::*;

/// `ESC`. VT52's introducer is the bare escape, which is not a C1 control and
/// so does not come from `c1` -- `S8C1T` cannot move it, because a VT52 has no
/// 8-bit forms to move it to.
const ESC: u8 = 0x1b;

/// `modifyOtherKeys`, the resource that decides whether an ordinary key with
/// modifiers is sent as an escape sequence instead of as a character.
///
/// **This is the negotiation `[[source: fixterms]]` does not have.** That
/// document is a proposal: it says terminals should send `CSI codepoint ;
/// modifier u` and defines no handshake at all. A terminal that simply started
/// doing so would break every application that has not asked, which is the same
/// reasoning that makes `DA` under-claim. xterm's answer is this resource, set
/// by XTMODKEYS -- so the `CSI u` form is reachable only through here or through
/// `[[source: kitty-keyboard]]`'s stack, and it is not independently
/// implementable.
///
/// `[[source: ctlseqs]]` line 2357 onward gives the levels, 0 to 3, and
/// [`KeyResources`] holds the resource; this is its reading for the
/// encoder, a disabled one (`CSI > 4 n`) reading as `Off`, as xterm's
/// `ModifyOtherKeys` takes nothing below 1.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum OtherKeys {
    /// The base layer. Every ordinary key sends its character.
    #[default]
    Off,
    /// Shift and Control behave as they always did; other modifiers -- Alt and
    /// Meta -- make an ordinary key an escape sequence.
    Level1,
    /// Every modifier applies, so `shift-Tab` stops being `CSI Z`.
    Level2,
    /// "Unmodified keys also are sent as escape sequences": every ordinary
    /// key, Return, Tab, Backspace and Escape among them.
    Level3,
}

/// `formatOtherKeys`, which picks *which* sequence [`OtherKeys`] sends.
///
/// Two formats carry the same information and applications disagree about which
/// they want, so this is a format switch rather than a better answer:
/// `[[source: ctlseqs]]` line 2247 names the fixterms form's drawback in its own
/// words -- "applications may confuse it with `CSI u` (restore-cursor)" -- and
/// that collision is real in this codebase, where `Csi::RestoreCursor` is
/// exactly `CSI u`. It is not an ambiguity baud can suffer from, because these
/// bytes are terminal-to-application and SCORC is application-to-terminal, but
/// it is why the default stays xterm's.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum OtherKeysFormat {
    /// `CSI 27 ; modifier ; codepoint ~`, xterm's own and the default.
    #[default]
    Xterm,
    /// `CSI codepoint ; modifier u`, which is `[[source: fixterms]]`'s form.
    CsiU,
}

impl OtherKeys {
    /// The `Pv` of `CSI > 4 ; Pv m` that selects this level, which is what
    /// XTQMODKEYS and DECRQSS report so the answer sets it back.
    pub fn level(self) -> u8 {
        match self {
            OtherKeys::Off => 0,
            OtherKeys::Level1 => 1,
            OtherKeys::Level2 => 2,
            OtherKeys::Level3 => 3,
        }
    }
}

/// One of xterm's seven key-modifier resources, by the `Pp` XTMODKEYS,
/// XTFMTKEYS, `CSI > Ps n` and their queries name it with
/// (`[[source: ctlseqs]]`; 5 is "reserved for input via the string action"
/// and names none here).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyResource {
    /// `modifyKeyboard`, 0: which categories the legacy and vt220 keyboards
    /// may modify. baud's keyboard is xterm's default one, which never reads
    /// it, so it is stored and reported and changes no key, as in xterm
    /// (Jev's lean, STORE_AND_REPORT, 0.64/0.72 on the second ask).
    Keyboard,
    /// `modifyCursorKeys`, 1: the arrows, Home and End, and at 4 the
    /// editing keys too.
    Cursor,
    /// `modifyFunctionKeys`, 2.
    Function,
    /// `modifyKeypadKeys`, 3.
    Keypad,
    /// `modifyOtherKeys`, 4: the ordinary keys.
    Other,
    /// `modifyModifierKeys`, 6: Shift, Control and the other modifier keys.
    Modifier,
    /// `modifySpecialKeys`, 7: "special keys (e.g., escape not in the other
    /// categories)", `[[source: xterm-man]]`.
    Special,
}

impl KeyResource {
    /// The seven, in `Pp` order.
    pub const ALL: [KeyResource; 7] = [
        KeyResource::Keyboard,
        KeyResource::Cursor,
        KeyResource::Function,
        KeyResource::Keypad,
        KeyResource::Other,
        KeyResource::Modifier,
        KeyResource::Special,
    ];

    /// The resource `Pp` names, or `None` for 5 and anything past 7.
    pub fn from_param(pp: u16) -> Option<KeyResource> {
        Some(match pp {
            0 => KeyResource::Keyboard,
            1 => KeyResource::Cursor,
            2 => KeyResource::Function,
            3 => KeyResource::Keypad,
            4 => KeyResource::Other,
            6 => KeyResource::Modifier,
            7 => KeyResource::Special,
            _ => return None,
        })
    }

    /// Its `Pp`.
    pub fn pp(self) -> u8 {
        match self {
            KeyResource::Keyboard => 0,
            KeyResource::Cursor => 1,
            KeyResource::Function => 2,
            KeyResource::Keypad => 3,
            KeyResource::Other => 4,
            KeyResource::Modifier => 6,
            KeyResource::Special => 7,
        }
    }

    fn slot(self) -> usize {
        match self {
            KeyResource::Keyboard => 0,
            KeyResource::Cursor => 1,
            KeyResource::Function => 2,
            KeyResource::Keypad => 3,
            KeyResource::Other => 4,
            KeyResource::Modifier => 5,
            KeyResource::Special => 6,
        }
    }

    /// The first value, xterm's resource defaults (`[[source:
    /// xterm-reset]]`'s file, charproc.c 686-701): 2 for the cursor and
    /// function keys, 0 for every other.
    fn first(self) -> ModifyValue {
        match self {
            KeyResource::Cursor | KeyResource::Function => ModifyValue::PARAM,
            _ => ModifyValue(0),
        }
    }

    /// `Pv` as this resource's value, or `None` for one no text documents
    /// (Jev's lean, REFUSE_COUNT, 0.82/0.90 then 0.88/0.86): `modifyKeyboard`'s
    /// four bits, 0 to 15; `modifyOtherKeys` 0 to 3; the others 0 to 4
    /// (`[[source: xterm-man]]`). -1 is reached only by `CSI > Pp n`, "which
    /// cannot be set with the other sequence".
    pub fn value(self, pv: u16) -> Option<ModifyValue> {
        let top = match self {
            KeyResource::Keyboard => 15,
            KeyResource::Other => 3,
            _ => 4,
        };
        i8::try_from(pv).ok().filter(|v| *v <= top).map(ModifyValue)
    }
}

/// A key-modifier resource's value, -1 (disabled) to 15.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModifyValue(i8);

impl ModifyValue {
    /// -1, "a resource value of "-1", which cannot be set with the other
    /// sequence" (`[[source: ctlseqs]]`, `CSI > Ps n`).
    pub const DISABLED: ModifyValue = ModifyValue(-1);
    /// 2, `mfkParam`: the modifier is the second parameter.
    const PARAM: ModifyValue = ModifyValue(2);
    /// 4, `mfkExtended`: the key is sent in `modifyOtherKeys` 3's form.
    const EXTENDED: ModifyValue = ModifyValue(4);

    pub fn get(self) -> i8 {
        self.0
    }
    /// The value as xterm's XTQMODKEYS writes it, an unsigned short, so
    /// -1 is 65535.
    pub fn reported(self) -> u16 {
        u16::from_ne_bytes(i16::from(self.0).to_ne_bytes())
    }
}

/// What one XTMODKEYS or `CSI > Ps n` does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeySet {
    /// `CSI > m`: "If no parameters are given, all resources are reset to
    /// their initial values" -- all seven, as the text says, where xterm's
    /// loop takes 1 to 5 (Jev's lean, ALL_SEVEN, 0.76/0.76 then
    /// 0.68/0.76).
    ResetAll,
    /// `CSI > Pp m`: "If the second parameter is omitted, the resource is
    /// reset to its initial value."
    Reset(KeyResource),
    /// `CSI > Pp ; Pv m`, and for `Pp` 4 `CSI > 4 ; Pv : mask m`.
    Set(KeyResource, ModifyValue, Modifiers),
    /// `CSI > Ps n`, `Ps` omitted being modifyFunctionKeys.
    Disable(KeyResource),
}

/// What one XTFMTKEYS does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FormatSet {
    ResetAll,
    Reset(KeyResource),
    Set(KeyResource, OtherKeysFormat),
}

/// xterm's key-modifier resources and their formats, as XTMODKEYS,
/// XTFMTKEYS and `CSI > Ps n` set them (xterm's `modify_now`,
/// `format_now` and `ignore_now`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyResources {
    modify: [ModifyValue; 7],
    format: [OtherKeysFormat; 7],
    /// `modifyOtherKeys`' mask, in the parameter's bits: Shift 1, Alt 2,
    /// Control 4, Meta 8.
    ignore: Modifiers,
}

impl Default for KeyResources {
    fn default() -> KeyResources {
        KeyResources {
            modify: KeyResource::ALL.map(KeyResource::first),
            format: [OtherKeysFormat::Xterm; 7],
            ignore: Modifiers::NONE,
        }
    }
}

impl KeyResources {
    pub fn modify(&self, r: KeyResource) -> ModifyValue {
        self.modify[r.slot()]
    }
    pub fn format(&self, r: KeyResource) -> OtherKeysFormat {
        self.format[r.slot()]
    }
    /// `modifyOtherKeys`' ignore mask.
    pub fn ignore(&self) -> Modifiers {
        self.ignore
    }
    /// `modifyOtherKeys` as the encoder reads it.
    pub fn other_keys(&self) -> OtherKeys {
        match self.modify(KeyResource::Other).get() {
            1 => OtherKeys::Level1,
            2 => OtherKeys::Level2,
            3 => OtherKeys::Level3,
            _ => OtherKeys::Off,
        }
    }
    pub fn apply(&mut self, set: KeySet) {
        match set {
            KeySet::ResetAll => {
                self.modify = KeyResource::ALL.map(KeyResource::first);
                self.ignore = Modifiers::NONE;
            }
            KeySet::Reset(r) => self.put(r, r.first(), Modifiers::NONE),
            KeySet::Set(r, v, mask) => self.put(r, v, mask),
            KeySet::Disable(r) => self.put(r, ModifyValue::DISABLED, Modifiers::NONE),
        }
    }
    pub fn apply_format(&mut self, set: FormatSet) {
        match set {
            FormatSet::ResetAll => self.format = [OtherKeysFormat::Xterm; 7],
            FormatSet::Reset(r) => self.format[r.slot()] = OtherKeysFormat::Xterm,
            FormatSet::Set(r, f) => self.format[r.slot()] = f,
        }
    }
    /// Every write sets the mask too, as xterm's `SET_MOD_FKEYS` writes
    /// `ignore_now` beside `modify_now`; only `modifyOtherKeys` has one.
    fn put(&mut self, r: KeyResource, v: ModifyValue, mask: Modifiers) {
        self.modify[r.slot()] = v;
        if r == KeyResource::Other {
            self.ignore = mask;
        }
    }
}

impl OtherKeysFormat {
    /// The `Pv` of `CSI > 4 ; Pv f` that selects this format.
    pub fn code(self) -> u8 {
        match self {
            OtherKeysFormat::Xterm => 0,
            OtherKeysFormat::CsiU => 1,
        }
    }
}

/// The five progressive enhancements of `[[source: kitty-keyboard]]`, as the
/// bitfield `CSI = flags ; mode u` carries them.
///
/// **Masked to the five defined bits on the way in.** The specification's
/// table defines five bits (html 1536-1556) and gives no rule for any other,
/// so the mask is baud's choice: a bit kept and never acted on is a mode
/// recorded and not acted on, which CLAUDE.md refuses, and `CSI ? u`, which
/// writes the stored bits as they are, would report it back. A stored
/// `KittyFlags` is therefore valid by construction and the encoder never has
/// to guard against a sixth bit. The protocol is all-or-nothing because its
/// support detection is whether the terminal answers `CSI ? u` at all: "If an
/// answer for the device attributes is received without getting back an
/// answer for the progressive enhancement the terminal does not support this
/// protocol." The core answers it with these flags, `CSI ? flags u`, since the
/// encoder honours all five (`docs/record.md`, "kitty's protocol is
/// all-or-nothing, and its own detection mechanism says so").
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct KittyFlags(u8);

impl KittyFlags {
    pub const NONE: KittyFlags = KittyFlags(0);
    pub const DISAMBIGUATE: KittyFlags = KittyFlags(0b1);
    pub const EVENT_TYPES: KittyFlags = KittyFlags(0b10);
    pub const ALTERNATE_KEYS: KittyFlags = KittyFlags(0b100);
    pub const ALL_AS_ESCAPES: KittyFlags = KittyFlags(0b1000);
    pub const ASSOCIATED_TEXT: KittyFlags = KittyFlags(0b1_0000);
    /// Every defined bit set. [`KittyFlags::from_param`] masks against this,
    /// so `CSI ? u`'s reply carries no other bit.
    pub const ALL: KittyFlags = KittyFlags(0b1_1111);

    /// The `flags` parameter of a set or push, with undefined bits dropped.
    pub fn from_param(p: Option<Param>) -> KittyFlags {
        KittyFlags((p.map_or(0, |x| x.0) as u8) & Self::ALL.0)
    }
    pub fn bits(self) -> u8 {
        self.0
    }
    pub fn has(self, f: KittyFlags) -> bool {
        self.0 & f.0 == f.0
    }
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
    /// Apply a `CSI = flags ; mode u` request to the current flags.
    pub fn apply(self, req: KittyFlags, mode: KittySetMode) -> KittyFlags {
        KittyFlags(match mode {
            KittySetMode::All => req.0,
            KittySetMode::Or => self.0 | req.0,
            KittySetMode::AndNot => self.0 & !req.0,
        })
    }
}

/// How `CSI = flags ; mode u` combines its flags with the current set.
///
/// `[[source: kitty-keyboard]]`: mode `1` sets the given bits and resets the
/// rest, `2` sets them and leaves the rest, `3` resets them and leaves the
/// rest. Mode is optional and defaults to `1`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum KittySetMode {
    /// `1`, the default: replace the whole set with the given bits.
    #[default]
    All,
    /// `2`: set the given bits, leave the rest.
    Or,
    /// `3`: reset the given bits, leave the rest.
    AndNot,
}

impl KittySetMode {
    /// An unrecognised mode falls to the default `All`, the same way an omitted
    /// one does -- the specification defines only 1, 2 and 3, and answering a
    /// fourth as its own thing would be acting on a value the protocol has no
    /// meaning for.
    pub fn from_param(p: Option<Param>) -> KittySetMode {
        match p.map_or(1, |x| x.0) {
            2 => KittySetMode::Or,
            3 => KittySetMode::AndNot,
            _ => KittySetMode::All,
        }
    }
}

/// One screen's `[[source: kitty-keyboard]]` flag stack.
///
/// **The main and alternate screens keep independent stacks**, which the
/// specification requires so an editor can change the mode in the alternate
/// screen without knowing or disturbing the shell's: the terminal holds one of
/// these per screen and swaps which is live on the `?1049` transition.
///
/// The stack is bounded, which the specification also requires -- "to prevent
/// Denial-of-Service attacks" -- so a push flood costs a bounded shift rather
/// than unbounded growth, and a push into a full stack evicts the oldest entry.
#[derive(Clone, Copy)]
pub struct KittyStack {
    current: KittyFlags,
    saved: [KittyFlags; KittyStack::DEPTH],
    depth: usize,
}

impl Default for KittyStack {
    fn default() -> KittyStack {
        KittyStack::new()
    }
}

impl KittyStack {
    pub const DEPTH: usize = 16;

    pub const fn new() -> KittyStack {
        KittyStack {
            current: KittyFlags::NONE,
            saved: [KittyFlags::NONE; Self::DEPTH],
            depth: 0,
        }
    }

    /// The flags in force, which is what the encoder reads.
    pub fn current(&self) -> KittyFlags {
        self.current
    }

    /// `CSI = flags ; mode u`.
    pub fn set(&mut self, req: KittyFlags, mode: KittySetMode) {
        self.current = self.current.apply(req, mode);
    }

    /// `CSI > flags u` -- save the current flags and set them to `flags`.
    /// A push into a full stack drops the oldest entry, as the spec directs.
    pub fn push(&mut self, flags: KittyFlags) {
        if self.depth == Self::DEPTH {
            self.saved.copy_within(1.., 0);
            self.depth -= 1;
        }
        self.saved[self.depth] = self.current;
        self.depth += 1;
        self.current = flags;
    }

    /// `CSI < number u` -- pop `count` entries. Emptying the stack resets all
    /// flags, which is the spec's rule and why popping the last entry does not
    /// restore its value.
    pub fn pop(&mut self, count: Count) {
        let n = count.at_least_one();
        if n >= self.depth {
            self.depth = 0;
            self.current = KittyFlags::NONE;
        } else {
            self.depth -= n;
            self.current = self.saved[self.depth];
        }
    }
}

/// DECBKM, `?67`: what the backarrow key sends (`[[source: vt510-decbkm]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BackarrowKey {
    /// Reset, the default: "If DECBKM is reset, ⌫ works as a delete key
    /// ... sends a DEL character", and "To be compatible with some software,
    /// DECBKM should be reset".
    #[default]
    Delete,
    /// Set: "the terminal sends a BS character to the host".
    Backspace,
}

/// `?1037`: what the editing keypad's Delete sends (`[[source: ctlseqs]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DeleteKey {
    /// Reset, the default: "Send VT220 Remove from the editing-keypad Delete
    /// key", `CSI 3 ~`.
    #[default]
    Remove,
    /// Set: "Send DEL from the editing-keypad Delete key".
    Del,
}

/// `?1035`: whether NumLock overrides the application keypad, xterm's
/// `numLock` resource (`[[source: ctlseqs]]`, "Use the NumLock key to
/// override the application mode").
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NumLockKeypad {
    /// Set, the default, as the resource's is (Jev's lean, SET,
    /// 0.66/0.58): a keypad key with NumLock on sends its numeric column
    /// under DECKPAM too. The embedder reports NumLock on a keypad key
    /// whenever it is on, as the Wayland driver does; macOS reports none,
    /// its keypad having no NumLock key.
    #[default]
    Overrides,
    /// Reset: NumLock does not change what the keypad sends.
    Ignored,
}

/// The mode state the encoder reads. [`encode_key`] reads the key, this and
/// nothing else, and it reads every field.
///
/// **Ten fields and not fourteen, because `Modes` carries five of them.**
/// DECCKM, LNM, `?1036` metaSendsEscape, `?1039` altSendsEscape and `?1034`
/// eightBitInput are the five `ModeBit`s the encoder reads, so they travel as
/// one word and this struct needs no `bool` -- which `audit_all.sh` gates this
/// crate at **zero** of, on the grounds that a `bool` field is always a missing
/// two-state enum. `baud-core`'s `Term::key` hands over its whole `Modes`, so
/// the word carries every other mode bit as well, and the encoder reads none
/// of them.
#[derive(Clone, Copy)]
pub struct KeyState {
    pub modes: Modes,
    pub keypad: KeypadMode,
    /// xterm's seven key-modifier resources and their formats, set by
    /// XTMODKEYS, XTFMTKEYS and `CSI > Ps n`.
    pub keys: KeyResources,
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
    /// `[[source: kitty-keyboard]]`'s active enhancement flags. `NONE` is the
    /// base layer -- every key its legacy form -- and any bit set routes the
    /// key through the kitty encoding instead.
    pub kitty: KittyFlags,
    /// DECBKM, `?67`.
    pub backarrow: BackarrowKey,
    /// `?1037`.
    pub delete: DeleteKey,
    /// `?1035`.
    pub num_lock: NumLockKeypad,
    /// The conformance level DECSCL selected: at Level 1 the keyboard is
    /// [`level_one_key`]'s.
    pub level: ConformanceLevel,
}

impl KeyState {
    /// The keypad's column for `k` carrying `mods`: DECKPAM's, unless
    /// `?1035` lets NumLock override it, as xterm's input.c forces
    /// `keypad_mode` off for a keypad key while NumLock is on
    /// (`[[source: xterm-input-c]]`). **Not for a navigation key**: xterm
    /// overrides only a key that produced one byte of text, `kd.nbytes ==
    /// 1`, and `KP_End` produces none.
    fn keypad_for(self, k: KeypadKey, mods: Modifiers) -> KeypadMode {
        let text = k.shares_row_with().is_none();
        match self.num_lock == NumLockKeypad::Overrides && mods.has(Modifiers::NUM_LOCK) && text {
            true => KeypadMode::Numeric,
            false => self.keypad,
        }
    }

    /// The backarrow key's byte: DECBKM's, **and Control inverts it**, xterm's
    /// `IsBackarrowToggle` ("Pressing the control key toggles this
    /// behavior") and the legacy table of `[[source: kitty-keyboard]]`, whose
    /// Control-Backspace is `0x8` (Jev's lean, XTERM_TOGGLE, 0.62/0.70).
    fn backarrow_byte(self, mods: Modifiers) -> u8 {
        match (self.backarrow == BackarrowKey::Backspace) != mods.has(Modifiers::CTRL) {
            true => 0x08,
            false => 0x7f,
        }
    }
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
    /// The longest key this can produce is 40 bytes, measured, and 48 bounds it.
    ///
    /// The longest is `[[source: kitty-keyboard]]`'s CSI-u form with every field
    /// at its widest, `CSI 1114111:1114111:1114111 ; 256:2 ; 1114111 u`: `CSI`
    /// (2), a key number, a shifted key and a base-layout key of up to seven
    /// digits each (U+10FFFF), a three-digit modifier (1 + 255) with a one-digit
    /// event type, a seven-digit associated text, five separators and the final.
    /// The base layer's longest is `modifyOtherKeys`' `CSI 27 ; 16 ; 65535 ~`, at
    /// 14. `baud-domain/tests/key_bytes.rs` holds each to the sum it writes out:
    /// the kitty layer over every key, all 256 modifier masks, every event kind,
    /// every flag set and both C1 transmissions; the base layer over every key,
    /// the modifier classes it distinguishes and presses, in full product with
    /// every state it reads.
    ///
    /// **The 8 bytes over 40 are what let that test see a longer key.** A key
    /// that would overflow is dropped whole rather than half-sent, which is the
    /// [`Fit::Overflowed`] contract, and comes back `None`, the answer a key that
    /// sends nothing gives -- so a key that outgrew this would vanish from the
    /// sweep rather than fail it, while one that grows into the headroom moves
    /// the measured maximum and fails it. The specification's colon-separated
    /// multi-codepoint associated text, the later widening [`KeyEvent::text`]
    /// names, adds up to 8 bytes a codepoint, so it re-derives this bound.
    pub const MAX_LEN: usize = 48;

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
    /// A `u32` parameter, for a kitty key number that is a codepoint above the
    /// `u16` ceiling.
    fn num32(&mut self, v: u32) {
        for b in Decimal32::of(v).as_bytes() {
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

/// Encode a key press into the bytes an application reads.
///
/// `None` is "this key sends nothing", which is a real answer rather than a
/// failure: a VT52 has no Insert key, ctlseqs gives no modified form for
/// `SS3 j`, and a codepoint above `U+00FF` has no ISO 8859-1 byte. Inventing a
/// sequence for any of them would be answering with a shape no application
/// asked for, which this project refuses on the output side too.
///
/// **Which layer encodes the key is [`KeyState::kitty`].** With no enhancement
/// flags set it is the base layer -- what every terminal sends, and what an
/// application that has not negotiated an enhancement must receive.
/// `[[source: kitty-keyboard]]`'s progressive enhancement routes here when a
/// flag is set. The core sets them only on an application's request (`CSI =`,
/// `CSI >` and `CSI <` with final `u`), and answers `CSI ? u` with the flags in
/// force -- the protocol's support claim, made since this encoder honours all
/// five enhancements.
pub fn encode_key(e: KeyEvent, s: KeyState) -> Option<KeyBytes> {
    if s.kitty.is_none() {
        encode_base(e, s)
    } else {
        kitty_encode(e, s)
    }
}

/// The base layer, `[[source: ctlseqs]]`'s legacy encoding.
fn encode_base(e: KeyEvent, s: KeyState) -> Option<KeyBytes> {
    // A repeat is a press here and a release is nothing.
    // `[[source: kitty-keyboard]]`: "Normally only key press events are
    // reported and key repeat events are treated as key press events." A held
    // key on this layer is the key pressed again, which every terminal sends;
    // only the event-types enhancement reports a release, and an application
    // that has not enabled it would read one as a second press. This used to
    // drop repeats as well, which would have typed a held key once the day an
    // embedder delivered one.
    if e.kind == KeyEventKind::Release {
        return None;
    }
    // **Control on a key whose own character is not ASCII takes the
    // keyboard's ASCII one** where the platform named it
    // ([`KeyEvent::control`]), as xterm's does through `XLookupString`,
    // which builds its string from the keysym it fell back to: the
    // identity every legacy form below folds or numbers, and the text a
    // key with no control form sends.
    let (code, text) = match (e.code, e.control) {
        (KeyCode::Char(id), Some(c)) if e.mods.has(Modifiers::CTRL) && id.value() > 0x7f => {
            (KeyCode::Char(c), Some(c))
        }
        (code, _) => (code, e.text),
    };
    let mut o = KeyBytes::new();
    // **VT52 is a different keyboard, not a different introducer.**
    // `[[source: ctlseqs]]` line 2698: "A VT52 does not have function keys,
    // but it does have a numeric keypad and cursor keys." So this is a
    // separate table rather than a prefix substitution, and the keys a VT52
    // does not have send nothing.
    let ok = match s.terminal {
        TerminalMode::Vt52 => vt52(&mut o, code, e.mods, text, s),
        TerminalMode::Ansi => ansi(&mut o, code, e.mods, text, s),
    };
    // **Level 1 transmits only 7-bit codes** (`[[source: dec-std-070]]`
    // 070-6 §6.2.4 a, p. 6-19): "Keys which would normally produce an 8-bit
    // code with the 8th bit set shall be inoperative". Under ISO 8859-1,
    // where a byte is a character of that code; under UTF-8 a key sends its
    // UTF-8 at every level (Jev's lean, LATIN1_ONLY, 0.83/0.77 then
    // 0.72/0.73), as received UTF-8 is decoded at every level.
    let eight = s.level == ConformanceLevel::One
        && s.encoding == InputEncoding::Latin1
        && o.as_bytes().iter().any(|b| *b >= 0x80);
    match ok && !eight && o.fit == Fit::Whole {
        true => Some(o),
        false => None,
    }
}

/// `[[source: kitty-keyboard]]`'s progressive-enhancement encoding, all five
/// enhancements.
///
/// A key the flags route (`kitty_disambiguated`, or all-keys) takes the CSI-u
/// form. Its number is a text key's unshifted codepoint, which
/// [`KeyCode::Char`] carries -- "the codepoint used is *always* the lower-case
/// (or more technically, un-shifted) version of the key", so `ctrl+shift+a` is
/// `97` -- a keypad key's from the table's `KP_*` rows (`PF1`-`PF4` take
/// F1-F4's forms), a Private Use Area key's its own, and any other key's from
/// the functional table. When their flags ask, the alternate keys ride as `:`
/// sub-fields of the number and the associated text as a third `;` field --
/// "Fields are separated by the semi-colon and sub-fields by the colon" (html
/// 1396-1397). A key the flags leave alone keeps its
/// base-layer bytes. `None` for a release without the event-types flag, for
/// the keypad's `Space` and `Tab`, which have no number in the table, and
/// wherever the base layer sends nothing.
fn kitty_encode(e: KeyEvent, s: KeyState) -> Option<KeyBytes> {
    let flags = s.kitty;
    // Event type: without `Report event types` only presses are reported, a
    // repeat is a press and a release is not reported at all; with it, press is
    // 1, repeat 2, release 3.
    let event = match (e.kind, flags.has(KittyFlags::EVENT_TYPES)) {
        (KeyEventKind::Release, false) => return None,
        (KeyEventKind::Release, true) => 3u16,
        (KeyEventKind::Repeat, true) => 2,
        _ => 1,
    };
    // `Report all keys as escape codes` routes every key through CSI-u;
    // otherwise only the ambiguous and functional keys do and the rest keep
    // their legacy bytes.
    let route =
        flags.has(KittyFlags::ALL_AS_ESCAPES) || kitty_disambiguated(e.code, e.mods, e.text);
    if !route {
        return encode_base(e, s);
    }
    // A text key's number is its unshifted codepoint, `KeyCode::Char` itself;
    // a functional key's is from the table, and a keypad key's from the
    // table's `KP_*` rows.
    let (number, final_byte) = match e.code {
        KeyCode::Char(cp) => (cp.value(), b'u'),
        // The PUA keys -- locks, F21-F35, media, individual modifiers -- carry
        // their dedicated number and always the `u` final.
        KeyCode::Pua(p) => (p.kitty_number(), b'u'),
        // The keypad's `PF` keys share F1-F4's forms; the rest carry kitty's
        // dedicated keypad numbers, so a keypad digit is distinct from the
        // main-row one -- which is the disambiguation the enhancement is for.
        KeyCode::Keypad(k) => match k.pf() {
            Some(f) => {
                let (n, fin) = kitty_functional(KeyCode::Function(f))?;
                (u32::from(n), fin)
            }
            None => kitty_keypad(k)?,
        },
        _ => {
            let (n, f) = kitty_functional(e.code)?;
            (u32::from(n), f)
        }
    };
    // Alternate keys (flag 4): the shifted codepoint only when shift is held,
    // and the base-layout codepoint whenever the platform supplied it. Both are
    // absent for functional keys, which carry no such variant.
    let alt = flags.has(KittyFlags::ALTERNATE_KEYS);
    let shifted = (alt && e.mods.has(Modifiers::SHIFT))
        .then_some(e.shifted)
        .flatten()
        .map(|c| c.value());
    let base = alt.then_some(e.base_layout).flatten().map(|c| c.value());
    // Associated text (flag 16): the produced codepoint, embedded in the escape
    // code. Without the flag, an all-keys report carries the key event only.
    // **Never on a release**, whatever text the platform attached to it: the
    // field is for "key events that generate text" (html 1663-1666), key
    // events are "those that produce text and those that do not" (1382), and
    // the legacy encoding, which sends exactly the produced text, sends "only
    // key press and repeat events" (1691) -- a release produces none.
    let text = (flags.has(KittyFlags::ASSOCIATED_TEXT) && e.kind != KeyEventKind::Release)
        .then_some(e.text)
        .flatten()
        .map(|c| c.value());
    let mut o = KeyBytes::new();
    kitty_csi_u(
        &mut o, number, final_byte, e.mods, event, shifted, base, text, s.c1,
    );
    match o.fit == Fit::Whole {
        true => Some(o),
        false => None,
    }
}

/// The text a key typed, if it is a character and not a control code.
fn typed(text: Option<Codepoint>) -> Option<Codepoint> {
    text.filter(|c| c.value() >= 0x20 && c.value() != 0x7f)
}

/// Whether a key is reported as a CSI-u escape code under `Disambiguate escape
/// codes` alone (without `Report all keys`).
///
/// `[[source: kitty-keyboard]]`: "the Esc, alt+key, ctrl+key, ctrl+alt+key,
/// shift+alt+key keys", plus every non-text keypad and functional key. The
/// Enter, Tab and Backspace keys are the exceptions -- they keep their legacy
/// bytes so a shell stays usable after a crashed program leaves the mode set.
///
/// **A lock or modifier key is not reported here**: "with this mode
/// [report all keys], events for pressing modifier keys are reported", and
/// `num_lock` and `caps_lock` are among the protocol's modifiers; kitty's
/// encoder drops a key `is_modifier_key` names without that flag
/// (`[[source: kitty-key-encoding-c]]` line 457). A Mac's Clear key,
/// NumLock, went out as `CSI 57360 u` under disambiguate alone.
///
/// **A keypad key that typed a character is a text key**, and keeps its
/// legacy bytes, its lock unsent: "all non text keypad keys" are the ones
/// reported, and "the Lock modifiers are not reported for text producing
/// keys". Typed is what the embedder's text says, as kitty's encoder sends
/// a key's text whenever it carries any that is not a control code
/// (`[[source: kitty-key-encoding-c]]`), and its keyboard layer gives none
/// while Control, Alt, Super, Hyper or Meta is held (`[[source:
/// kitty-xkb-glfw-c]]`); a digit went out as `CSI 57400 ; 129 u`.
fn kitty_disambiguated(code: KeyCode, mods: Modifiers, text: Option<Codepoint>) -> bool {
    match code {
        KeyCode::Enter | KeyCode::Tab | KeyCode::Backspace => false,
        KeyCode::Keypad(_) => typed(text).is_none(),
        // A lock or modifier key waits for "report all keys".
        KeyCode::Pua(p) => !p.is_modifier(),
        // A text key disambiguates only when a modifier makes it collide with a
        // control code; plain and shift-only text keys still send text.
        KeyCode::Char(_) => {
            mods.has(Modifiers::CTRL) || mods.has(Modifiers::ALT) || mods.has(Modifiers::META)
        }
        // Escape and every functional key.
        _ => true,
    }
}

/// A functional key's CSI-u number and final byte, from the "Functional key
/// definitions" table. `functional_keys_match_the_source` gates every row
/// against `scripts/extract-kitty-keys.py`, so a transcription slip fails the
/// build rather than sending a wrong key.
fn kitty_functional(code: KeyCode) -> Option<(u16, u8)> {
    Some(match code {
        KeyCode::Escape => (27, b'u'),
        KeyCode::Enter => (13, b'u'),
        KeyCode::Tab => (9, b'u'),
        KeyCode::Backspace => (127, b'u'),
        KeyCode::Up => (1, b'A'),
        KeyCode::Down => (1, b'B'),
        KeyCode::Right => (1, b'C'),
        KeyCode::Left => (1, b'D'),
        KeyCode::End => (1, b'F'),
        KeyCode::Home => (1, b'H'),
        KeyCode::Insert => (2, b'~'),
        KeyCode::Delete => (3, b'~'),
        KeyCode::PageUp => (5, b'~'),
        KeyCode::PageDown => (6, b'~'),
        // F3 is `13 ~` rather than `1 R`: the original spec allowed `CSI R`,
        // which collides with the Cursor Position Report, so it was removed.
        KeyCode::Function(f) => match f.number() {
            1 => (1, b'P'),
            2 => (1, b'Q'),
            3 => (13, b'~'),
            4 => (1, b'S'),
            5 => (15, b'~'),
            6 => (17, b'~'),
            7 => (18, b'~'),
            8 => (19, b'~'),
            9 => (20, b'~'),
            10 => (21, b'~'),
            11 => (23, b'~'),
            12 => (24, b'~'),
            13 => (57376, b'u'),
            14 => (57377, b'u'),
            15 => (57378, b'u'),
            16 => (57379, b'u'),
            17 => (57380, b'u'),
            18 => (57381, b'u'),
            19 => (57382, b'u'),
            20 => (57383, b'u'),
            _ => return None,
        },
        // Char, keypad and PUA keys are resolved by the caller, not here.
        KeyCode::Char(_) | KeyCode::Keypad(_) | KeyCode::Pua(_) => return None,
    })
}

/// A keypad key's `[[source: kitty-keyboard]]` number and final, from the
/// functional-key table's `KP_*` rows, navigation keys included; `KP_BEGIN`
/// is the one letter final, `1 E`. `PF1`-`PF4` are not here -- they share F1-F4's forms
/// and the caller routes them there -- and `Space`/`Tab` have no keypad form in
/// the table, so they send nothing under the enhancement. Gated against the
/// source by `keypad_keys_match_the_source`.
fn kitty_keypad(k: KeypadKey) -> Option<(u32, u8)> {
    use KeypadKey::*;
    let n: u32 = match k {
        Digit0 => 57399,
        Digit1 => 57400,
        Digit2 => 57401,
        Digit3 => 57402,
        Digit4 => 57403,
        Digit5 => 57404,
        Digit6 => 57405,
        Digit7 => 57406,
        Digit8 => 57407,
        Digit9 => 57408,
        Period => 57409,   // KP_DECIMAL
        Divide => 57410,   // KP_DIVIDE
        Multiply => 57411, // KP_MULTIPLY
        Minus => 57412,    // KP_SUBTRACT
        Add => 57413,      // KP_ADD
        Enter => 57414,    // KP_ENTER
        Equal => 57415,    // KP_EQUAL
        Comma => 57416,    // KP_SEPARATOR
        Left => 57417,
        Right => 57418,
        Up => 57419,
        Down => 57420,
        PageUp => 57421,
        PageDown => 57422,
        Home => 57423,
        End => 57424,
        Insert => 57425,
        Delete => 57426,
        // `1 E or 57427 ~`: the first spelling, the one the extractor keeps
        // and the legacy layer already sends.
        Begin => return Some((1, b'E')),
        Space | Tab | Pf1 | Pf2 | Pf3 | Pf4 => return None,
    };
    Some((n, b'u'))
}

/// Emit `CSI number[:[shifted][:base]] [; mods[:event] [; text]] final`, in
/// the two shapes `[[source: kitty-keyboard]]` defines.
///
/// The alternate keys are `number:shifted`, `number:shifted:base`, or, with a
/// base-layout key and no shifted key, `number::base`: "If the terminal wants
/// to send only a base layout key but no shifted key, it must use an empty
/// sub-field for the shifted key" (html 1422-1425).
///
/// The `u`/`~` finals carry the key number, with the alternate keys attached to
/// it, so a letter final never sends them; a letter final uses number `1` and
/// omits it entirely when there are no parameters, giving `CSI A` for an
/// unmodified arrow -- the same bytes the legacy form sends, which is why
/// disambiguation leaves an unmodified cursor key alone. The event sub-field is
/// present only for a non-press, and a modifier field of `1` is written to
/// carry it when no modifier is held.
#[allow(clippy::too_many_arguments)]
fn kitty_csi_u(
    o: &mut KeyBytes,
    number: u32,
    final_byte: u8,
    mods: Modifiers,
    event: u16,
    shifted: Option<u32>,
    base: Option<u32>,
    text: Option<u32>,
    c1: C1Transmit,
) {
    o.introducer(Introducer::Csi, c1);
    let letter = final_byte != b'u' && final_byte != b'~';
    let has_alt = shifted.is_some() || base.is_some();
    // The modifier field is present, as an explicit `1` when nothing is held,
    // whenever a modifier, an event sub-field or associated text has to be
    // carried -- `[[source: kitty-keyboard]]` lines 1466-1467: "the modifiers
    // field must have the value 1".
    let params = !mods.is_none() || event != 1 || text.is_some();

    // The key-number field, with the alternate-key sub-fields. A letter final
    // omits the number entirely when there is nothing else to carry; otherwise
    // it is `1`. Alternate keys attach to the number, so a letter form never
    // has them (functional keys carry no shifted/base variant).
    if letter && !params {
        // `CSI A` -- number omitted.
    } else if letter {
        o.num(1);
    } else {
        o.num32(number);
        if has_alt {
            o.push(b':');
            // `code::base` when only the base-layout key is present: the
            // shifted slot is left empty.
            if let Some(sh) = shifted {
                o.num32(sh);
            }
            if let Some(b) = base {
                o.push(b':');
                o.num32(b);
            }
        }
    }

    if params {
        o.push(b';');
        o.num(mods.kitty_wire());
        if event != 1 {
            o.push(b':');
            o.num(event);
        }
        if let Some(t) = text {
            o.push(b';');
            o.num32(t);
        }
    }
    o.push(final_byte);
}

fn ansi(
    o: &mut KeyBytes,
    code: KeyCode,
    mods: Modifiers,
    text: Option<Codepoint>,
    s: KeyState,
) -> bool {
    if let Some(sent) = level_one_key(o, code, mods, s) {
        return sent;
    }
    // **A resource at 4 takes its keys before anything else**, as xterm's
    // `Input` checks `mfkExtended` first (`[[source: xterm-input-c]]`).
    extended(o, code, mods, s) || ansi_key(o, code, mods, text, s)
}

/// **The keys Level 1 decides**, or `None` for a key it leaves as at every
/// level: `[[source: dec-std-070]]` 070-6 §6.2.4 (p. 6-19), "The Editing
/// Keypad keys with generic names E1, E2, E3, E4, E5, and E6 are
/// inoperative", F6 to F10 and F14 to F20 inoperative, and "F11 shall
/// transmit the ESC code (1/11)", F12 BS and F13 LF; and §6.2.1 b, "Shall
/// neither implement additional keys nor make operative keys that are not
/// part of Level 1 Operation". So Insert, Delete, Page Up, Page Down, Home
/// and End send nothing, and F5, which a VT100 lacks and an LK201 keeps
/// local (Jev's lean, EDITING_HOME_END_F5, 0.79/0.79 then 0.28/0.56);
/// "inoperative means that the key shall not transmit anything". F1 to F4
/// stay PF1 to PF4, and the arrows and the keypad their VT100 forms.
///
/// A function key that modifyFunctionKeys -1 remaps under Shift or
/// Control is left to [`remapped_function_key`], which rules the key it
/// becomes: Shift-F11 is F21, past F20.
fn level_one_key(o: &mut KeyBytes, code: KeyCode, mods: Modifiers, s: KeyState) -> Option<bool> {
    if s.level != ConformanceLevel::One {
        return None;
    }
    let remapped = s.keys.modify(KeyResource::Function) == ModifyValue::DISABLED
        && (mods.has(Modifiers::SHIFT) || mods.has(Modifiers::CTRL));
    match code {
        KeyCode::Function(_) if remapped => None,
        KeyCode::Insert
        | KeyCode::Delete
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Home
        | KeyCode::End => Some(false),
        KeyCode::Function(f) => match f.number() {
            5..=10 | 14..=20 => Some(false),
            11 => {
                o.push(0x1b);
                Some(true)
            }
            12 => {
                o.push(0x08);
                Some(true)
            }
            13 => {
                o.push(0x0a);
                Some(true)
            }
            _ => None,
        },
        _ => None,
    }
}

/// The ANSI keyboard once no resource at 4 has taken the key. **A keypad
/// navigation key comes back here as the key it duplicates, not to
/// [`ansi`]**: xterm classes `KP_Up` a keypad key before it moves it onto
/// `Up` (`[[source: xterm-input-c]]`, `TypeOfKeysym`, then the `XK_KP_Home`
/// remap), so modifyCursorKeys 4 reaches it through `modifyCursorKey`
/// alone.
fn ansi_key(
    o: &mut KeyBytes,
    code: KeyCode,
    mods: Modifiers,
    text: Option<Codepoint>,
    s: KeyState,
) -> bool {
    // A keypad navigation key reaches here as the key it duplicates.
    if let Some(sent) = level_one_key(o, code, mods, s) {
        return sent;
    }
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
        // `?1037` set: the editing keypad's Delete sends DEL, unmodified only
        // -- xterm's input.c sends Remove when `modify_parm != 0 ||
        // !xtermDeleteIsDEL` (`[[source: xterm-input-c]]`).
        KeyCode::Delete if s.delete == DeleteKey::Del && !carries(mods) => o.push(0x7f),
        KeyCode::Insert | KeyCode::Delete | KeyCode::PageUp | KeyCode::PageDown => {
            match code.tilde_param() {
                Some(p) => tilde_key(o, p, mods, s, ModifyValue::PARAM),
                None => return false,
            }
        }
        KeyCode::Function(f) => {
            return function_key(o, f, mods, s, s.keys.modify(KeyResource::Function));
        }
        KeyCode::Keypad(k) => return keypad_key(o, k, mods, text, s),
        // The PUA keys have no legacy form; the base layer sends nothing.
        KeyCode::Pua(_) => return false,
        KeyCode::Char(cp) => {
            // **`modifyOtherKeys` takes the key before the base layer does**,
            // because the whole point of the resource is to disambiguate
            // exactly the cases `text_key` cannot express: that function has no
            // way to say "Control was held" other than by sending a different
            // character.
            //
            // A codepoint past `u16` falls through instead. `Param` saturates
            // at 65535, so an astral character cannot be named by either
            // format, and a saturated parameter would name a *different*
            // character -- the same reason X10 mouse reporting refuses a
            // position it cannot encode rather than truncating it.
            match (other_keys_takes(mods, s), u16::try_from(cp.value())) {
                (true, Ok(v)) => other_key(o, v, mods, s),
                _ => return text_key(o, cp, text, mods, s),
            }
        }
        // `[[source: ctlseqs]]` line 253: BS is Ctrl-H. Which byte the
        // *Backspace key* sends is a different question, which DECBKM (`?67`)
        // answers, Control inverting it ([`KeyState::backarrow_byte`]).
        // The C0 keys. `[[source: fixterms]]`'s "Modified C0 controls" names the
        // codepoint each is encoded with -- Enter 13, Escape 27, Backspace 127,
        // Tab 9 -- which are their ASCII values rather than anything new; the
        // backarrow key's is the byte it sends, as xterm's toggle sets that
        // byte before `modifyOtherKeys` reads it.
        KeyCode::Backspace if other_keys_takes(mods, s) => {
            other_key(o, u16::from(s.backarrow_byte(mods)), mods, s)
        }
        KeyCode::Backspace => c0_key(o, s.backarrow_byte(mods), mods, s),
        KeyCode::Escape if other_keys_takes(mods, s) => other_key(o, 0x1b, mods, s),
        KeyCode::Escape => c0_key(o, 0x1b, mods, s),
        // **Shift-Tab is the discriminating case for level 2**, and
        // `[[source: ctlseqs]]` line 2237 says so in the example it chooses:
        // "shift-Tab sends `CSI 2 7 ; 2 ; 9 ~` **rather than** `CSI Z`". So the
        // resource does not merely add sequences, it replaces one the base
        // layer already sends -- which is why the order of these two arms is
        // load-bearing and why the tests assert `CSI Z` still comes out at
        // levels 0 and 1.
        KeyCode::Tab if other_keys_takes(mods, s) => other_key(o, 0x09, mods, s),
        KeyCode::Tab => match mods.has(Modifiers::SHIFT) {
            // CBT. Shift-Tab is the one text key that is a sequence, and it is
            // `CSI Z` rather than a modified HT -- with Alt, `ESC CSI Z`,
            // `[[source: kitty-keyboard]]`'s legacy table, the prefix the man
            // page says "applies as well to function key control sequences"
            // (`[[source: xterm-man]]`). A sequence has no 8-bit shift.
            true => {
                if meta_of(mods, s).0 == Prefix::Escape {
                    o.push(ESC);
                }
                o.introducer(Introducer::Csi, s.c1);
                o.all(b"Z");
            }
            false => c0_key(o, 0x09, mods, s),
        },
        KeyCode::Enter if other_keys_takes(mods, s) => other_key(o, 0x0d, mods, s),
        KeyCode::Enter => return_key(o, mods, s),
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
fn vt52(
    o: &mut KeyBytes,
    code: KeyCode,
    mods: Modifiers,
    text: Option<Codepoint>,
    s: KeyState,
) -> bool {
    match code {
        KeyCode::Up | KeyCode::Down | KeyCode::Right | KeyCode::Left => match code.cursor_final() {
            Some(f) => vt52_esc(o, f),
            None => return false,
        },
        // A navigation key is the key it duplicates: line 2713 names the
        // keypad's rows by digit alone, so there is no row to send.
        KeyCode::Keypad(k) => match k.duplicates() {
            Some(code) => return vt52(o, code, mods, text, s),
            None => return vt52_keypad(o, k, mods, text, s),
        },
        // The text keys are the keyboard, not the emulation.
        KeyCode::Char(cp) => return text_key(o, cp, text, mods, s),
        KeyCode::Backspace => c0_key(o, s.backarrow_byte(mods), mods, s),
        KeyCode::Escape => c0_key(o, 0x1b, mods, s),
        // No CBT on a VT52: `CSI Z` is an ANSI sequence, so Tab is HT whatever
        // is held.
        KeyCode::Tab => c0_key(o, 0x09, mods, s),
        KeyCode::Enter => return_key(o, mods, s),
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
fn vt52_keypad(
    o: &mut KeyBytes,
    k: KeypadKey,
    mods: Modifiers,
    text: Option<Codepoint>,
    s: KeyState,
) -> bool {
    // `ESC P`-`ESC S` in both columns, so the mode is not consulted. Same four
    // finals as the ANSI side's `SS3 P`-`SS3 S`.
    if let Some(f) = k.pf().and_then(FnKey::ss3_final) {
        vt52_esc(o, f);
        return true;
    }
    if s.keypad_for(k, mods) == KeypadMode::Numeric {
        return keypad_numeric(o, k, text, s);
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

/// The keypad, `[[source: ctlseqs]]` lines 2498 and 2628.
///
/// **A key's application row is the VT220 table's, and a navigation key's
/// the PC table's.** ctlseqs gives both -- line 2498 for a Sun/PC keypad and
/// line 2628 for the VT102/VT220 keypad -- and they disagree on the digits
/// and `.`, the count derived by
/// `the_keypad_digits_send_the_vt220_rows_where_the_tables_part` rather than
/// written here. xterm's own code on its PC keyboard reconciles them: a
/// digit keysym sends `SS3 p`-`SS3 y` (`kypd_apl`, `[[source: xterm-input-c]]`
/// 1390-1396) and a NumLock-off keysym becomes the cursor or editing key
/// whose form line 2498 lists (1185-1189). iTerm2, foot (`[[source:
/// foot-keymap-h-1-21]]`) and ghostty (`[[source:
/// ghostty-d9840f3-function-keys]]`) send the digits the same way (Jev,
/// VT220_SS3, 0.94/0.94); until 2026-10-20 a digit sent line 2498's column,
/// `5` being `CSI E`.
///
/// The two columns are chosen by DECKPAM/DECKPNM. The numeric column is the
/// literal character in every row but `Enter` and the `PF` keys.
fn keypad_key(
    o: &mut KeyBytes,
    k: KeypadKey,
    mods: Modifiers,
    text: Option<Codepoint>,
    s: KeyState,
) -> bool {
    // `PF1`-`PF4` are `SS3 P`-`SS3 S` in **both** columns: the only rows the
    // mode does not change. They are the same four sequences F1-F4 send, so
    // they take the same path and the `SS3` -> `CSI 1 ; mod` switch under a
    // modifier is written once.
    // modifyFunctionKeys does not reach them: xterm's PF keys are keypad
    // keys (`IsPFKey`), sent with the modifier second as baud sends F1-F4.
    if let Some(f) = k.pf() {
        return function_key(o, f, mods, s, ModifyValue::PARAM);
    }
    // A navigation key: under DECKPAM the row of 2498 it shares with its
    // digit ("4 (LeftArrow)"), and otherwise the key it duplicates, which
    // `[[source: kitty-keyboard]]` says of every keypad key: "reported as
    // their equivalent non-keypad keys". `Begin` duplicates no key, and its
    // row's `CSI E` is line 2518's spelling and the first of kitty's two
    // (`1 E or 57427 ~`).
    let mode = s.keypad_for(k, mods);
    if let Some(row) = k.shares_row_with() {
        return match (mode, k.duplicates()) {
            // The keypad's Delete is not the editing keypad's, so `?1037`
            // leaves it `CSI 3 ~` (xterm's `KP_Delete` is a keysym of its own).
            (KeypadMode::Numeric, Some(KeyCode::Delete)) => ansi_key(
                o,
                KeyCode::Delete,
                mods,
                None,
                KeyState {
                    delete: DeleteKey::Remove,
                    ..s
                },
            ),
            (KeypadMode::Numeric, Some(code)) => ansi_key(o, code, mods, None, s),
            // Under DECKPAM too the key is the one it duplicates at Level 1:
            // PageUp's row, `CSI 5 ~`, is the editing keypad's E5.
            _ => match k
                .duplicates()
                .and_then(|code| level_one_key(o, code, mods, s))
            {
                Some(sent) => sent,
                None => keypad_navigation(o, row, mods, s),
            },
        };
    }
    if mode == KeypadMode::Numeric {
        return keypad_numeric(o, k, text, s);
    }
    keypad_application(o, k, mods, s)
}

/// The numeric column, ANSI's (line 2498) and VT52's (line 2713): **the
/// character the key typed**, and its label where it typed none.
///
/// The tables give each row's label, which is what a US layout types; a
/// Mac's decimal key types `,` on German and French layouts and its digits
/// type U+0660-0669 on Arabic ones. kitty sends a keypad key's text
/// (`[[source: kitty-key-encoding-c]]`) and iTerm2 its characters
/// (`[[source: iterm2-7ce037e-vt100output]]`, `keypadDataForString`);
/// xterm sends the keysym's own character (`kypd_num`, `[[source:
/// xterm-input-c]]` line 1398), and an X layout whose key types `,` gives
/// it `KP_Separator`, whose character that is. A character the live
/// encoding cannot carry leaves the label, the key's own.
fn keypad_numeric(o: &mut KeyBytes, k: KeypadKey, text: Option<Codepoint>, s: KeyState) -> bool {
    // Enter is the one row whose numeric form is not its label: `CR`, and
    // LNM governs it exactly as it governs the main Return.
    if k == KeypadKey::Enter {
        o.ret(s);
        return true;
    }
    if let Some(c) = typed(text).and_then(|c| char::from_u32(c.value())) {
        let mut buf = [0u8; 4];
        match s.encoding {
            InputEncoding::Utf8 => {
                o.all(c.encode_utf8(&mut buf).as_bytes());
                return true;
            }
            InputEncoding::Latin1 => {
                if let Ok(b) = u8::try_from(u32::from(c)) {
                    o.push(b);
                    return true;
                }
            }
        }
    }
    match k.numeric() {
        Some(b) => {
            o.byte(b);
            true
        }
        None => false,
    }
}

/// A navigation key under DECKPAM: the application column of
/// `[[source: ctlseqs]]` line 2498 for the digit `row` on the same key,
/// "4 (LeftArrow)", the rows xterm's NumLock-off keysyms send.
fn keypad_navigation(o: &mut KeyBytes, row: KeypadKey, mods: Modifiers, s: KeyState) -> bool {
    // The rows that are cursor keys in disguise, whose introducer is fixed by
    // the table rather than by DECCKM.
    if let Some((intro, f)) = row.cursor_shaped() {
        cursor_shaped(o, intro, f, mods, s, s.keys.modify(KeyResource::Cursor));
        return true;
    }
    // The editing-keypad rows: same parameters as the standalone keys.
    if let Some(p) = row.tilde_param() {
        tilde_key(o, p, mods, s, ModifyValue::PARAM);
        return true;
    }
    false
}

/// The application column of `[[source: ctlseqs]]` line 2628 for a keypad
/// key, `PF1`-`PF4` aside: `SS3 <byte>` on every row, the digits and `.`
/// included ([`KeypadKey::application_byte`]; Jev, VT220_SS3, 0.94/0.94).
///
/// **A modifier is the parameter between them**, `SS3 5 r` for
/// Control-keypad-2: xterm's `MODIFIER_PARM` on a keypad key, which
/// `allowModifierParm` allows on its default keyboard (`[[source:
/// xterm-input-c]]` 1390-1396), as foot (`[[source: foot-keymap-h-1-21]]`,
/// `DEFAULT_MODS_FOR_KP`), ghostty (`[[source:
/// ghostty-d9840f3-function-keys]]`, `kpKeys`) and iTerm2 (`[[source:
/// iterm2-7ce037e-vt100output]]`) send it. ctlseqs gives the keypad no
/// modifier form and Jev split (NO_MODIFIER 0.10/0.06, then NO_MODIFIER
/// 0.20 against XTERM_PARAM 0.12); built by the standing rule, the owner's
/// "full VT420-class terminal. we need to support such protocols fully,
/// always!" (2026-09-25). NumLock is not a modifier here ([`Modifiers::wire`]).
fn keypad_application(o: &mut KeyBytes, k: KeypadKey, mods: Modifiers, s: KeyState) -> bool {
    match k.application_byte() {
        Some(b) => {
            o.introducer(Introducer::Ss3, s.c1);
            if carries(mods) {
                o.num(u16::from(mods.wire()));
            }
            o.byte(b);
            true
        }
        None => false,
    }
}

/// How Alt and Meta reach a key that sends one byte: the `ESC` prefix, and
/// the bit `?1034` adds, `0x80` or nothing.
///
/// `[[source: ctlseqs]]` line 2325 gives two ways for either modifier to
/// apply -- prefix `ESC`, or add 128 -- and line 2337's table has a separate
/// column per modifier: `?1039` for Alt, `?1036` for Meta. They are asked
/// independently because four of that table's sixteen rows disagree. The
/// prefix wins where both modes are set, which is what the table's last four
/// rows say: with both resources ON every modified row is `ESC x`.
///
/// The table's other branch -- `shift`, meaning add 128 -- is `?1034`
/// eightBitInput, and it applies only under ISO 8859-1. Under UTF-8 a byte
/// above 127 is a continuation byte rather than a character, so shifting
/// there would send something no application can decode.
fn meta_of(mods: Modifiers, s: KeyState) -> (Prefix, u8) {
    let modded = mods.has(Modifiers::ALT) || mods.has(Modifiers::META);
    let esc = match (mods.has(Modifiers::ALT) && s.modes.is_set(ModeBit::AltSendsEscape))
        || (mods.has(Modifiers::META) && s.modes.is_set(ModeBit::MetaSendsEscape))
    {
        true => Prefix::Escape,
        false => Prefix::None,
    };
    let shift = modded
        && esc == Prefix::None
        && s.modes.is_set(ModeBit::EightBitInput)
        && s.encoding == InputEncoding::Latin1;
    (esc, if shift { 0x80 } else { 0 })
}

/// Backspace, Escape and Tab: one C0 byte, which Alt and Meta reach exactly
/// as they reach a character. xterm sends these keys down the branch it
/// sends text down (`[[source: xterm-input-c]]`, `kd.nbytes > 0`), and
/// `[[source: kitty-keyboard]]`'s legacy table gives Alt-Backspace `0x1b
/// 0x7f`, Alt-Tab `0x1b 0x9` and Alt-Escape `0x1b 0x1b`.
fn c0_key(o: &mut KeyBytes, b: u8, mods: Modifiers, s: KeyState) {
    let (esc, high) = meta_of(mods, s);
    o.prefixed(esc, &[b | high]);
}

/// Return: `CR`, or `CR LF` under LNM, after the prefix Alt and Meta give
/// it (`0x1b 0xd`, the legacy table's). Shifted by `?1034` it is `0x8d`
/// alone: the byte is no longer a CR for LNM to follow, as xterm's
/// `unparseputc` adds the LF only after a CR.
fn return_key(o: &mut KeyBytes, mods: Modifiers, s: KeyState) {
    match meta_of(mods, s) {
        (_, 0x80) => o.push(0x8d),
        (esc, _) => {
            if esc == Prefix::Escape {
                o.push(ESC);
            }
            o.ret(s);
        }
    }
}

/// A key that produces text.
///
/// Two modifiers change the bytes rather than adding a parameter, and this
/// layer has no way to say "Control was held" other than by sending a
/// different character -- which is exactly the ambiguity
/// `[[source: fixterms]]` exists to remove, and why `CSI u` is the next layer
/// rather than a nicety.
fn text_key(
    o: &mut KeyBytes,
    id: Codepoint,
    text: Option<Codepoint>,
    mods: Modifiers,
    s: KeyState,
) -> bool {
    // Control folds the character into `0x00..0x1f`: `@` through `_` and the
    // lowercase letters differ from their control code by bit 6 alone.
    // Anything else has no control form and Control is simply dropped, which
    // is what a terminal does and what an application cannot detect.
    // The fold is on the key's *identity* -- `ctrl+a` is `0x01` whatever `a`
    // produced -- which is why `KeyCode::Char` is the unshifted codepoint.
    let base = match mods.has(Modifiers::CTRL) {
        true => match id.value() {
            v @ (0x40..=0x5f | 0x61..=0x7a) => u8::try_from(v & 0x1f).ok(),
            0x20 => Some(0x00),
            0x3f => Some(0x7f),
            _ => None,
        },
        false => None,
    };

    // Alt and Meta: the `ESC` prefix or `?1034`'s bit ([`meta_of`]).
    let (esc, high) = meta_of(mods, s);
    // **The prefix is emitted last, not here.** Emitting it before the
    // character is known to be encodable left a bare `ESC` in the reply when
    // the encoding could not represent the key -- `Alt` plus a codepoint above
    // `U+00FF` under ISO 8859-1 returned `false` having already written a
    // byte. An application reads that `ESC` as the start of a sequence that
    // never arrives. Found by `the_return_value_agrees_with_the_bytes`.

    // A control code is always one byte and always below 128, so the shift is
    // unconditional here. `Meta-Ctrl-A` becomes `0x81`, which under ISO 8859-1
    // is a C1 control -- and baud accepts 8-bit C1 controls in exactly that
    // encoding, so the two halves agree.
    if let Some(b) = base {
        o.prefixed(esc, &[b | high]);
        return true;
    }

    // Not a control code: send the produced character. A key that produced
    // none -- a bare modifier, or a combination with no text -- sends nothing,
    // which is the same real answer `None` is elsewhere.
    let Some(produced) = text else {
        return false;
    };
    let v = produced.value();
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
        o.prefixed(esc, &[if b < 0x80 { b | high } else { b }]);
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
///
/// `level` is modifyFunctionKeys (`[[source: xterm-man]]`): at -1 "xterm
/// uses the modifier keys to make an extended sequence of function keys
/// rather than adding a parameter" (`[[source: ctlseqs]]`) -- Shift adds
/// `ctrlFKeys`, 10, and Control twice it, and every other modifier is
/// dropped, as `modifyCursorKey` zeroes it. **The sources conflict on the
/// order:** `[[source: xterm-man]]` says that at modifyFunctionKeys 0
/// "Control adds the value given by the ctrlFKeys resource. Shift adds
/// twice"; xterm's `Input` does the reverse at -1, `mfkNone`, Shift adding
/// ctrl_fkeys and Control twice it (`[[source: xterm-input-c]]`). baud
/// follows the code, so Shift-F1 is F11.
///
/// Returns whether anything was sent: a key remapped onto one Level 1
/// makes inoperative sends nothing.
fn function_key(
    o: &mut KeyBytes,
    f: FnKey,
    mods: Modifiers,
    s: KeyState,
    level: ModifyValue,
) -> bool {
    let (shift, ctrl) = (mods.has(Modifiers::SHIFT), mods.has(Modifiers::CTRL));
    if level == ModifyValue::DISABLED && (shift || ctrl) {
        let n = f.number() + if shift { 10 } else { 0 } + if ctrl { 20 } else { 0 };
        return remapped_function_key(o, n, s);
    }
    // F1-F4 share the cursor keys' shape: a single final byte after `SS3`, and
    // `CSI 1 ; mod` when modified. **`SS3` unconditionally when unmodified --
    // DECCKM does not reach here.** Routing these through `cursor_key` was
    // wrong and the specification's own table caught it: that function picks
    // its introducer from DECCKM, so F1 came out as `CSI P` in normal mode
    // where line 2529 says `SS3 P` flatly. A cursor key is the only thing
    // DECCKM governs.
    if let Some(final_byte) = f.ss3_final() {
        cursor_shaped(o, Introducer::Ss3, final_byte, mods, s, level);
        return true;
    }
    // The gaps in the `~` parameters -- 16 and 22 skipped, then 27 and 30 --
    // live on `FnKey::tilde_param`, with the tables they came from.
    match f.tilde_param() {
        Some(p) => {
            tilde_key(o, p, mods, s, level);
            true
        }
        None => false,
    }
}

/// Function key `n` as modifyFunctionKeys -1 makes it, unmodified: F1-F20
/// as their tables give them, and past F20 the number xterm's
/// `decfuncvalue` gives, `42 + n - 21`, "made up and do not correspond to
/// any real terminal" (`[[source: xterm-input-c]]`; decided by Jev,
/// XTERM_42_REMAP_ONLY, 0.97/0.94 on the second ask). F31-F40 are
/// Control-Shift-F1 to F10.
///
/// **At Level 1 the key it becomes takes Level 1's rule**: Shift-F1 is F11
/// and sends ESC, Shift-F4 is F14 and sends nothing (`[[source:
/// dec-std-070]]` 070-6 §6.2.4), and a key past F20 sends nothing, §6.2.1
/// b's "Shall neither implement additional keys" (Jev, INOPERATIVE,
/// 0.94/0.94).
fn remapped_function_key(o: &mut KeyBytes, n: u8, s: KeyState) -> bool {
    match FnKey::new(n) {
        Some(f) => match level_one_key(o, KeyCode::Function(f), Modifiers::NONE, s) {
            Some(sent) => sent,
            None => function_key(o, f, Modifiers::NONE, s, ModifyValue::PARAM),
        },
        None if s.level == ConformanceLevel::One => false,
        None => {
            tilde_key(
                o,
                Param(42 + u16::from(n) - 21),
                Modifiers::NONE,
                s,
                ModifyValue::PARAM,
            );
            true
        }
    }
}

/// `CSI n ~`, or `CSI n ; mod ~` when modified.
///
/// The editing keys and F5 upward share this shape, and unlike the cursor keys
/// they do not change with DECCKM -- `[[source: ctlseqs]]` line 2672 gives the
/// same sequence in both columns.
///
/// `level` is modifyFunctionKeys for F5 up, and 2 for the editing keys,
/// which xterm sends with no `modifyCursorKey` (`[[source: xterm-input-c]]`):
/// -1 drops the modifier and 3 marks the sequence `>`; 0 to 2 are one form
/// here, the parameter already first.
fn tilde_key(o: &mut KeyBytes, param: Param, mods: Modifiers, s: KeyState, level: ModifyValue) {
    let carried = carries(mods) && level != ModifyValue::DISABLED;
    o.introducer(Introducer::Csi, s.c1);
    if carried && level.get() == 3 {
        o.push(b'>');
    }
    o.num(param.value());
    if carried {
        o.all(b";");
        o.num(u16::from(mods.wire()));
    }
    o.all(b"~");
}

/// Does `modifyOtherKeys` take this key press, instead of the base layer?
///
/// `[[source: ctlseqs]]` line 2224 onward, quoted because the two levels differ
/// in exactly one word:
///
/// * level 1 -- "the usual shift- and control-modifiers work as expected, but
///   **other** modifiers (such as alt- and meta-modifiers) cause ordinary keys
///   to be encoded as if they were function-keys";
/// * level 2 -- "**all** of the modifiers apply".
///
/// So level 1 asks whether Alt or Meta is held and level 2 whether anything is,
/// and an unmodified key is never taken at either -- that is level 3, which
/// takes every ordinary key, modified or not. **The ignore mask**
/// (`CSI > 4 ; Pv : mask m`) factors a key's one modifier out when it is in
/// the mask, xterm's `allowedCharModifiers`: at levels 1 and 2, whose
/// min_mod is 1, the key is left to the base layer; at 3, whose min_mod
/// is 0, it is still a sequence, sent with the modifier removed
/// ([`other_key`]), as an unmodified key is (`[[source: xterm-input-c]]`).
fn other_keys_takes(mods: Modifiers, s: KeyState) -> bool {
    if masked(mods, s).is_some() {
        return matches!(s.keys.other_keys(), OtherKeys::Level3);
    }
    match s.keys.other_keys() {
        OtherKeys::Off => false,
        OtherKeys::Level1 => mods.has(Modifiers::ALT) || mods.has(Modifiers::META),
        OtherKeys::Level2 => carries(mods),
        OtherKeys::Level3 => true,
    }
}

/// The modifier the ignore mask factors out of this key: of the four
/// xterm encodes, Shift, Alt, Control and Meta, exactly one held, and that
/// one in the mask.
fn masked(mods: Modifiers, s: KeyState) -> Option<Modifiers> {
    let held = Modifiers::from_wire(mods.wire()).unwrap_or(Modifiers::NONE);
    ((mods.wire() - 1).count_ones() == 1 && s.keys.ignore().has(held)).then_some(held)
}

/// Whether the base layer has a modifier to send: one of the four
/// `[[source: ctlseqs]]` codes. **A modifier with no code there is none**,
/// a lock, Super or Hyper ([`Modifiers::wire`] drops them), so a key
/// carrying only a lock is the key unmodified -- `CSI A`, where this sent
/// `CSI 1 ; 1 A`, a parameter naming no modifier, for every arrow on a
/// Wayland keyboard with NumLock on. xterm's `xtermStateToParam` maps no
/// lock either (`[[source: xterm-input-c]]`).
fn carries(mods: Modifiers) -> bool {
    mods.wire() != 1
}

/// An ordinary key, sent as a sequence because `modifyOtherKeys` is set.
///
/// Two formats carry the same three facts, and `[[source: ctlseqs]]` gives an
/// example of each:
///
/// * `formatOtherKeys` 0 -- `CSI 27 ; modifier ; codepoint ~`, so alt-Tab is
///   `CSI 27 ; 3 ; 9 ~` (line 2229);
/// * `formatOtherKeys` 1 -- `CSI codepoint ; modifier u`, so the same key is
///   `CSI 9 ; 3 u` (line 2249), which is `[[source: fixterms]]`'s form.
///
/// **The modifier is written even when it is 1.** Every other encoder here
/// omits a default parameter, and this one must not: the xterm form's `27` is a
/// literal first parameter rather than a key number, so `CSI 27 ; 1 ; 32 ~`
/// and `CSI 27 ; 32 ~` are different sequences, and the second is a modified
/// F-key. The fixterms form keeps the modifier for symmetry with it -- a bare
/// `CSI 9 u` would be SCORC's shape with a parameter.
///
/// A modifier the ignore mask factors out is not written: at level 3,
/// Shift-a under `CSI > 4 ; 3 : 1 m` is `CSI 27 ; 1 ; 97 ~`.
fn other_key(o: &mut KeyBytes, cp: u16, mods: Modifiers, s: KeyState) {
    let mods = match masked(mods, s) {
        Some(m) => mods.without(m),
        None => mods,
    };
    modified_form(o, cp, mods, s.keys.format(KeyResource::Other), s.c1);
}

/// `CSI 27 ; modifier ; code ~` or `CSI code ; modifier u`, `format`'s
/// choice, xterm's `modifyOtherKey`: for an ordinary key under
/// modifyOtherKeys and for any key a resource at 4 takes.
fn modified_form(
    o: &mut KeyBytes,
    cp: u16,
    mods: Modifiers,
    format: OtherKeysFormat,
    c1: C1Transmit,
) {
    o.introducer(Introducer::Csi, c1);
    match format {
        OtherKeysFormat::Xterm => {
            o.all(b"27;");
            o.num(u16::from(mods.wire()));
            o.all(b";");
            o.num(cp);
            o.all(b"~");
        }
        OtherKeysFormat::CsiU => {
            o.num(cp);
            o.all(b";");
            o.num(u16::from(mods.wire()));
            o.all(b"u");
        }
    }
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
    cursor_shaped(
        o,
        intro,
        final_byte,
        mods,
        s,
        s.keys.modify(KeyResource::Cursor),
    );
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
    level: ModifyValue,
) {
    if !carries(mods) || level == ModifyValue::DISABLED {
        o.introducer(intro, s.c1);
        o.byte(final_byte.byte());
        return;
    }
    // `level` is modifyCursorKeys, or modifyFunctionKeys for F1-F4, as
    // xterm's `modifyCursorKey` reads it (`[[source: xterm-input-c]]`): 0
    // keeps the introducer and puts the modifier first, "the old/obsolete
    // behavior"; from 1 a modified key uses `CSI` even in application mode,
    // "prefixes modified sequences with CSI", as `SS3` takes no parameters;
    // from 2, the first value, the modifier "is the second parameter"; and
    // 3 "marks the sequence with a '>' to hint that it is private"
    // (`[[source: xterm-man]]`).
    let v = level.get();
    o.introducer(if v >= 1 { Introducer::Csi } else { intro }, s.c1);
    if v >= 3 {
        o.push(b'>');
    }
    if v >= 2 {
        o.all(b"1;");
    }
    o.num(u16::from(mods.wire()));
    o.byte(final_byte.byte());
}

/// **A resource at 4 sends its keys in `modifyOtherKeys` 3's form**,
/// "sending an escape sequence according to" its format resource
/// (`[[source: xterm-man]]`), unmodified keys too, the modifier 1. The key's
/// class is xterm's `TypeOfKeysym` over its keysym, and the number its
/// keysym through `keysym2ucs` ([`KeyCode::keysym`], [`keysym_class`],
/// [`keysym_code`]). `false` for a key no resource at 4 takes, or whose
/// number is past a `u16` parameter -- a media key -- which then goes on as
/// it would have.
fn extended(o: &mut KeyBytes, code: KeyCode, mods: Modifiers, s: KeyState) -> bool {
    let Some(sym) = code.keysym(mods) else {
        return false;
    };
    let class = keysym_class(sym, s.delete);
    if class == KeyResource::Other || s.keys.modify(class).get() < ModifyValue::EXTENDED.get() {
        return false;
    }
    match keysym_code(sym) {
        Some(cp) => {
            modified_form(o, cp, mods, s.keys.format(class), s.c1);
            true
        }
        None => false,
    }
}

/// The resource a keysym's key belongs to, xterm's `TypeOfKeysym`
/// (`[[source: xterm-input-c]]`) over Xlib's macros (`[[source:
/// xlib-xutil-h]]`): the cursor keys and the editing keypad (Delete only
/// while `?1037` leaves it Remove, xterm's `IsEditKeypad`), the function
/// keys and the miscellaneous ones, the keypad, the modifier keys, and
/// every other predefined keysym as special.
pub fn keysym_class(sym: u32, delete: DeleteKey) -> KeyResource {
    match sym {
        0xff50..=0xff5f => KeyResource::Cursor,
        0xff63 | 0xff68 | 0xff60 => KeyResource::Cursor,
        0xffff if delete == DeleteKey::Remove => KeyResource::Cursor,
        0xffbe..=0xffe0 => KeyResource::Function,
        0xff80..=0xffbd => KeyResource::Keypad,
        0xffe1..=0xffee | 0xfe01..=0xfe13 | 0xff7e | 0xff7f => KeyResource::Modifier,
        0xff60..=0xff6b => KeyResource::Function,
        0xfe01..=0xffff | 0x1000_0000.. => KeyResource::Special,
        _ => KeyResource::Other,
    }
}

/// The number a keysym is sent as, xterm's `keysym2ucs` (`[[source:
/// xterm-keysym2ucs-c]]`): its table's row for the keysyms baud's keys
/// produce in this range -- BackSpace 8, Tab 9, Return 13, Pause 19,
/// Scroll_Lock 20, Escape 27, Delete 127 (the table also has Linefeed,
/// Clear, Sys_Req and the dead keys, which no baud key produces) -- else,
/// for 0xfd00 to 0xffff, the
/// keysym less 0x1d00, "the Unicode BMP private use area beginning at
/// U+E001" (`[[source: ctlseqs]]`). `None` past that range: a vendor
/// keysym's number is past what a `u16` parameter names.
pub fn keysym_code(sym: u32) -> Option<u16> {
    Some(match sym {
        0xff08 => 0x08,
        0xff09 => 0x09,
        0xff0d => 0x0d,
        0xff13 => 0x13,
        0xff14 => 0x14,
        0xff1b => 0x1b,
        0xffff => 0x7f,
        0xfd00..=0xfffe => u16::try_from(sym - 0x1d00).ok()?,
        _ => return None,
    })
}
