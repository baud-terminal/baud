//! What baud **sends**: keys, and the modifiers held with them.
//!
//! The receive side had one specification to enumerate against. This side has
//! three layers -- the base every terminal implements, `[[source: fixterms]]`'s
//! `CSI key ; mods u`, and `[[source: kitty-keyboard]]`'s progressive
//! enhancement over that -- and `docs/input-coverage.md` is the enumeration.
//! These are the types the layers agree on; the encoding they disagree about
//! comes later.

use crate::*;

/// Which modifiers were held.
///
/// A bitset rather than four `bool` fields, and not only because
/// `baud-domain` is gated at zero of those: the wire form *is* a bitmask.
/// `[[source: ctlseqs]]` line 2565 gives fifteen rows of `Code | Modifiers`
/// and every one of them is `1 + mask` with Shift 1, Alt 2, Control 4 and
/// Meta 8 -- so shift-F5 is `CSI 1 5 ; 2 ~`. Four booleans would have to be
/// folded back into that number at every call site, which is where the folding
/// gets done differently in two places.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, PartialOrd, Ord)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Modifiers = Modifiers(0);
    pub const SHIFT: Modifiers = Modifiers(1);
    pub const ALT: Modifiers = Modifiers(2);
    pub const CTRL: Modifiers = Modifiers(4);
    /// Meta is not Alt, however many keyboards conflate them.
    /// `[[source: ctlseqs]]` reaches codes 9-16 only when `alwaysUseMods` is
    /// set, so a terminal that never sends Meta is not thereby wrong.
    pub const META: Modifiers = Modifiers(8);
    /// The four `[[source: kitty-keyboard]]` adds over the base layer. These
    /// have **no `[[source: ctlseqs]]` wire form** -- xterm cannot express
    /// super, hyper, caps-lock or num-lock -- so [`Modifiers::wire`] drops them
    /// and only [`Modifiers::kitty_wire`] carries them. The internal bit is
    /// chosen for a compact `wire` mask, not to match either protocol's
    /// numbering; the two wire methods each remap.
    pub const SUPER: Modifiers = Modifiers(16);
    pub const HYPER: Modifiers = Modifiers(32);
    pub const CAPS_LOCK: Modifiers = Modifiers(64);
    pub const NUM_LOCK: Modifiers = Modifiers(128);

    /// Every modifier the **ctlseqs** wire form can carry, which bounds
    /// [`Modifiers::wire`]. The kitty extras are outside it by construction.
    pub const ALL: Modifiers = Modifiers(15);

    pub fn with(self, o: Modifiers) -> Modifiers {
        Modifiers(self.0 | o.0)
    }
    pub fn without(self, o: Modifiers) -> Modifiers {
        Modifiers(self.0 & !o.0)
    }
    pub fn has(self, o: Modifiers) -> bool {
        self.0 & o.0 == o.0
    }
    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    /// The parameter xterm appends: `1 + mask`, so `NONE` is 1 and everything
    /// is 2 through 16.
    ///
    /// Emitting it for `NONE` is legal and xterm does not do it -- an
    /// unmodified F5 is `CSI 1 5 ~`, not `CSI 1 5 ; 1 ~` -- so an encoder asks
    /// [`Modifiers::is_none`] first. The value is still defined here rather
    /// than left to the caller, because "1 + mask" written at two call sites
    /// is where one of them becomes "mask".
    pub fn wire(self) -> u8 {
        // Mask to the four ctlseqs modifiers: super/hyper/caps/num have no
        // xterm wire form, so a base-layer key with one held encodes as though
        // it were not -- degrading to what xterm sends rather than inventing a
        // code past 16. On the existing four this is `1 + self.0` unchanged.
        1 + (self.0 & Self::ALL.0)
    }

    /// The `[[source: kitty-keyboard]]` modifier value: `1 + bitmask` over its
    /// eight modifiers, `shift 1, alt 2, ctrl 4, super 8, hyper 16, meta 32,
    /// caps_lock 64, num_lock 128`.
    ///
    /// **Not [`Modifiers::wire`] with a wider mask.** kitty's bit 8 is super
    /// where ctlseqs' is meta, so the two disagree on the meaning of a bit and
    /// each is a distinct remap of the internal set. `u16` because `1 + 255`
    /// does not fit a byte.
    pub fn kitty_wire(self) -> u16 {
        let mut k = 0u16;
        if self.has(Modifiers::SHIFT) {
            k |= 1;
        }
        if self.has(Modifiers::ALT) {
            k |= 2;
        }
        if self.has(Modifiers::CTRL) {
            k |= 4;
        }
        if self.has(Modifiers::SUPER) {
            k |= 8;
        }
        if self.has(Modifiers::HYPER) {
            k |= 16;
        }
        if self.has(Modifiers::META) {
            k |= 32;
        }
        if self.has(Modifiers::CAPS_LOCK) {
            k |= 64;
        }
        if self.has(Modifiers::NUM_LOCK) {
            k |= 128;
        }
        1 + k
    }

    /// The inverse, for reading a sequence back.
    ///
    /// `None` for 0 and for anything past 16: those are not modifier
    /// parameters, and silently clamping them would invent a keypress the
    /// application never made.
    pub fn from_wire(p: u8) -> Option<Modifiers> {
        match p {
            1..=16 => Some(Modifiers(p - 1)),
            _ => None,
        }
    }
}

/// A function key, `F1` through `F20`.
///
/// `[[source: ctlseqs]]` gives twelve in one table and eight more for the
/// VT220, reached on a PC keyboard as Control/F1 upward. One type rather than
/// two because an application cannot tell which table a key came from, and the
/// encoder needs the number.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub struct FnKey(u8);

impl FnKey {
    /// `F1` is 1. `None` outside 1..=20, rather than clamping to F20 -- the
    /// wrong function key is worse than none, and this is the same refusal
    /// `DA` makes by under-claiming.
    pub fn new(n: u8) -> Option<FnKey> {
        match n {
            1..=20 => Some(FnKey(n)),
            _ => None,
        }
    }
    pub fn number(self) -> u8 {
        self.0
    }
    /// **`SS3` becomes `CSI` for F1-F4 when a modifier is sent.**
    ///
    /// `[[source: ctlseqs]]` line 2564 calls this out as a special case, and
    /// it is the kind that is easy to miss: unmodified F1 is `SS3 P`, but
    /// shift-F1 is `CSI 1 ; 2 P`. An encoder that appends the modifier without
    /// changing the introducer sends something no application parses.
    pub fn changes_introducer_when_modified(self) -> bool {
        matches!(self.0, 1..=4)
    }
}

/// A key on the numeric keypad.
///
/// **A separate type from [`KeyCode`], because the keypad is a separate
/// table.** `[[source: ctlseqs]]` line 2498 gives twenty-four rows whose two
/// columns are chosen by DECKPAM/DECKPNM, and the digits are not the digits
/// above the letters: keypad `2` sends `CSI B` in application mode and the
/// main-row `2` never does. Folding them into `KeyCode::Char` would lose
/// exactly the distinction the table exists to make, and an application in
/// application-keypad mode would see text where it expected a cursor key.
///
/// `PF1`-`PF4` are here and *also* reachable as `F1`-`F4`, which is not a
/// duplicate: on a VT keyboard they are the keypad's own top row, and on a PC
/// keyboard the function keys send the same bytes. The encoder gives one
/// answer; which physical key produced it is the platform layer's business.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeypadKey {
    Space,
    Tab,
    Enter,
    Pf1,
    Pf2,
    Pf3,
    Pf4,
    Multiply,
    Add,
    Comma,
    Minus,
    /// Labelled `.` and Delete -- `[[source: ctlseqs]]` line 2508 gives its
    /// application form as `CSI 3 ~`, the editing keypad's Delete, and not the
    /// VT220 table's `SS3 n`.
    Period,
    Divide,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Equal,
    /// The navigation keys: what the digit keys and `.` are with NumLock
    /// off, `[[source: kitty-keyboard]]`'s `KP_LEFT` to `KP_BEGIN`.
    /// `[[source: ctlseqs]]` line 2462: the PC keypad has "cursor and
    /// scrolling operations printed on the keypad, which duplicate the
    /// smaller cursor and scrolling keypads". A separate identity from the
    /// digit on the same key, because kitty numbers them apart: `KP_4` is
    /// 57403 and `KP_LEFT` 57417.
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Insert,
    Delete,
    /// The key in the middle, labelled `5 (Begin)` at ctlseqs line 2518.
    /// It has no key of its own outside the keypad.
    Begin,
}

/// Which introducer a key sequence carries.
///
/// A two-state enum and **not** a `bool`, for the reason [`MarginMode`] gives:
/// the choice is made three different ways -- DECCKM for the cursor keys, the
/// table itself for the keypad, and "always `CSI`" once a modifier is present
/// -- and a `bool` at those call sites says nothing about which rule applied.
/// Routing the keypad through the cursor keys' rule is a bug that has already
/// happened here once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Introducer {
    Csi,
    Ss3,
}

impl KeypadKey {
    /// The X keysym, `XK_KP_*` ([`KeyCode::keysym`]).
    pub fn keysym(self) -> u32 {
        use KeypadKey::*;
        match self {
            Space => 0xff80,
            Tab => 0xff89,
            Enter => 0xff8d,
            Pf1 => 0xff91,
            Pf2 => 0xff92,
            Pf3 => 0xff93,
            Pf4 => 0xff94,
            Home => 0xff95,
            Left => 0xff96,
            Up => 0xff97,
            Right => 0xff98,
            Down => 0xff99,
            PageUp => 0xff9a,
            PageDown => 0xff9b,
            End => 0xff9c,
            Begin => 0xff9d,
            Insert => 0xff9e,
            Delete => 0xff9f,
            Multiply => 0xffaa,
            Add => 0xffab,
            Comma => 0xffac,
            Minus => 0xffad,
            Period => 0xffae,
            Divide => 0xffaf,
            Digit0 => 0xffb0,
            Digit1 => 0xffb1,
            Digit2 => 0xffb2,
            Digit3 => 0xffb3,
            Digit4 => 0xffb4,
            Digit5 => 0xffb5,
            Digit6 => 0xffb6,
            Digit7 => 0xffb7,
            Digit8 => 0xffb8,
            Digit9 => 0xffb9,
            Equal => 0xffbd,
        }
    }

    /// The numeric column of `[[source: ctlseqs]]` line 2498: the literal
    /// character on the key.
    ///
    /// `None` for `Enter` and `PF1`-`PF4`, whose numeric forms are not their
    /// labels -- `CR` and `SS3 P`-`SS3 S`. The caller must handle those, and
    /// `None` is how this type says so rather than inventing a byte.
    pub fn numeric(self) -> Option<StreamByte> {
        let b = match self {
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
            | KeypadKey::Pf4 => return None,
            // A navigation key produces no character; NumLock on is the digit.
            _ => return None,
        };
        Some(StreamByte::from_stream(b))
    }

    /// The `SS3 <byte>` rows of line 2498's application column.
    ///
    /// **`StreamByte`, not [`FinalByte`].** `Space` is `SS3 SP` -- 0x20, below
    /// the `0x40..=0x7e` range `FinalByte` is the proof of. The byte after
    /// `SS3` is not a CSI final byte and typing it as one would either reject
    /// this row or quietly widen an invariant the parser relies on.
    ///
    /// `None` for every row that is *not* that shape, which is the half of
    /// this table worth naming: `2` is `CSI B`, `0` is `CSI 2 ~`, `1` is
    /// `SS3 F`. Eight rows would be wrong under a uniform rule.
    pub fn pc_application_byte(self) -> Option<StreamByte> {
        let b = match self {
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
        };
        Some(StreamByte::from_stream(b))
    }

    /// The application column of `[[source: ctlseqs]]` line 2713, the VT52
    /// keypad, whose introducer is `ESC ?`.
    ///
    /// These finals are the *VT220* table's, not 2498's: `2` is `r` here and
    /// `CSI B` there, because a VT52 keypad has no cursor duties to borrow.
    pub fn vt52_byte(self) -> Option<StreamByte> {
        let b = match self {
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
            // The 2713 table names its rows by digit alone: a VT52 keypad
            // has no navigation labels to send.
            _ => return None,
        };
        Some(StreamByte::from_stream(b))
    }

    /// The rows of line 2498 that are cursor keys in disguise, as
    /// `(introducer, final)`.
    ///
    /// **The introducer is fixed per row and is not DECCKM's.** The arrows and
    /// Begin are `CSI`; End and Home are `SS3`. Four rows one way and two the
    /// other -- not a pattern, which is why each names its own and why routing
    /// these through the cursor keys' rule sent `CSI F` for a key the table
    /// spells `SS3 F`.
    pub fn cursor_shaped(self) -> Option<(Introducer, FinalByte)> {
        let (i, b) = match self {
            KeypadKey::Digit2 => (Introducer::Csi, b'B'),
            KeypadKey::Digit4 => (Introducer::Csi, b'D'),
            KeypadKey::Digit6 => (Introducer::Csi, b'C'),
            KeypadKey::Digit8 => (Introducer::Csi, b'A'),
            // `CSI E` is CNL as an output operation and Begin as an input one.
            // Different directions, so not a conflict.
            KeypadKey::Digit5 => (Introducer::Csi, b'E'),
            KeypadKey::Digit1 => (Introducer::Ss3, b'F'),
            KeypadKey::Digit7 => (Introducer::Ss3, b'H'),
            _ => return None,
        };
        Some((i, FinalByte(b)))
    }

    /// The editing-keypad rows of line 2498, which carry `CSI <param> ~`.
    pub fn tilde_param(self) -> Option<Param> {
        Some(Param(match self {
            KeypadKey::Digit0 => 2,
            KeypadKey::Period => 3,
            KeypadKey::Digit9 => 5,
            KeypadKey::Digit3 => 6,
            _ => return None,
        }))
    }

    /// The key a navigation key duplicates (`[[source: ctlseqs]]` line
    /// 2462), which is what it sends outside application keypad mode:
    /// `[[source: kitty-keyboard]]`, "All keypad keys are reported as their
    /// equivalent non-keypad keys". `None` for every other row, and for
    /// `Begin`, which duplicates no key.
    pub fn duplicates(self) -> Option<KeyCode> {
        Some(match self {
            KeypadKey::Left => KeyCode::Left,
            KeypadKey::Right => KeyCode::Right,
            KeypadKey::Up => KeyCode::Up,
            KeypadKey::Down => KeyCode::Down,
            KeypadKey::PageUp => KeyCode::PageUp,
            KeypadKey::PageDown => KeyCode::PageDown,
            KeypadKey::Home => KeyCode::Home,
            KeypadKey::End => KeyCode::End,
            KeypadKey::Insert => KeyCode::Insert,
            KeypadKey::Delete => KeyCode::Delete,
            _ => return None,
        })
    }

    /// The digit sharing a navigation key's row of `[[source: ctlseqs]]`
    /// line 2498, which names each row by both -- "4 (LeftArrow)",
    /// "5 (Begin)", ". (Delete)". Under DECKPAM a navigation key sends that
    /// row's application column. `None` for every key that is not a
    /// navigation key.
    pub fn shares_row_with(self) -> Option<KeypadKey> {
        Some(match self {
            KeypadKey::Delete => KeypadKey::Period,
            KeypadKey::Insert => KeypadKey::Digit0,
            KeypadKey::End => KeypadKey::Digit1,
            KeypadKey::Down => KeypadKey::Digit2,
            KeypadKey::PageDown => KeypadKey::Digit3,
            KeypadKey::Left => KeypadKey::Digit4,
            KeypadKey::Begin => KeypadKey::Digit5,
            KeypadKey::Right => KeypadKey::Digit6,
            KeypadKey::Home => KeypadKey::Digit7,
            KeypadKey::Up => KeypadKey::Digit8,
            KeypadKey::PageUp => KeypadKey::Digit9,
            _ => return None,
        })
    }

    /// `PF1`-`PF4` as function keys, which is what they send.
    pub fn pf(self) -> Option<FnKey> {
        FnKey::new(match self {
            KeypadKey::Pf1 => 1,
            KeypadKey::Pf2 => 2,
            KeypadKey::Pf3 => 3,
            KeypadKey::Pf4 => 4,
            _ => return None,
        })
    }
}

impl FnKey {
    /// The `CSI <param> ~` parameter this key carries, from
    /// `[[source: ctlseqs]]` lines 2529 and 2685.
    ///
    /// **The gaps are the point.** F5-F12 are 15, 17, 18, 19, 20, 21, 23, 24 --
    /// 16 and 22 are skipped -- and F13-F20 are 25, 26, 28, 29, 31, 32, 33, 34,
    /// skipping 27 and 30. A table written from memory as "15 upward" is wrong
    /// at F6 and wrong again at F11, and would send a neighbouring key rather
    /// than a missing one.
    ///
    /// `None` for F1-F4, which are not `~` keys at all: they are `SS3 P`
    /// through `SS3 S`, and [`FnKey::changes_introducer_when_modified`] is the
    /// other half of that rule.
    pub fn tilde_param(self) -> Option<Param> {
        let p: u16 = match self.0 {
            5 => 15,
            6 => 17,
            7 => 18,
            8 => 19,
            9 => 20,
            10 => 21,
            11 => 23,
            12 => 24,
            13 => 25,
            14 => 26,
            15 => 28,
            16 => 29,
            17 => 31,
            18 => 32,
            19 => 33,
            20 => 34,
            _ => return None,
        };
        Some(Param(p))
    }

    /// `SS3 P`-`SS3 S`: the final byte F1-F4 send.
    ///
    /// In range for [`FinalByte`], unlike the keypad's `SP`.
    pub fn ss3_final(self) -> Option<FinalByte> {
        self.changes_introducer_when_modified()
            .then(|| FinalByte(b'P' + self.0 - 1))
    }
}

/// Whether a key's bytes carry the `ESC` prefix that Alt or Meta adds.
///
/// A two-state enum and not a `bool`, for the reason [`MarginMode`] gives and
/// the signature audit enforces: the call site already passes a byte slice and
/// a `bool` beside it says nothing about which of the two prefix modes decided
/// it. `[[source: ctlseqs]]` line 2337 has separate columns for Alt and Meta,
/// and both fold into this one answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prefix {
    None,
    Escape,
}

/// A key, named by what it means rather than where it sits on a keyboard.
///
/// Physical position is the platform layer's problem; baud has no event
/// source. What arrives here is already resolved to a key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyCode {
    /// A text key, named by its **unshifted** codepoint -- the key's identity,
    /// not the character it produced. `shift+a`, `ctrl+a` and a plain `a` are
    /// all `Char('a')`; what each *produces* is [`KeyEvent::text`]. This is the
    /// identity `[[source: kitty-keyboard]]` reports as the key number ("the
    /// codepoint used is always the un-shifted version"), and it is what the
    /// base layer folds against for control codes -- `ctrl+a` folds `a` to
    /// `0x01`.
    Char(Codepoint),
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,
    Function(FnKey),
    /// A key on the numeric keypad, which has its own table and its own mode.
    Keypad(KeypadKey),
    Enter,
    Tab,
    Backspace,
    Escape,
    /// A key with no legacy form: `[[source: kitty-keyboard]]`'s Private Use
    /// Area keys -- the locks, F21-F35, media keys and the individual modifier
    /// keys. The base layer sends nothing for these (`[[source: ctlseqs]]` has
    /// no encoding); only the kitty encoder gives them a form, their dedicated
    /// PUA number.
    Pua(PuaKey),
}

/// A `[[source: kitty-keyboard]]` Private Use Area key -- one with a dedicated
/// number in the functional table and no `[[source: ctlseqs]]` legacy form.
///
/// **The individual modifier keys are why this exists.** Flag 8, "report all
/// keys as escape codes", reports a lone `Left Shift` press, which needs a key
/// identity the base layer never had. The locks, F21-F35 and media keys come
/// with them. The keypad's navigation keys (`KP_LEFT` to `KP_BEGIN`) are
/// [`KeypadKey`]s, because they have legacy forms. `pua_keys_match_the_source`
/// gates every number against the extractor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PuaKey {
    CapsLock,
    ScrollLock,
    NumLock,
    PrintScreen,
    Pause,
    Menu,
    F21,
    F22,
    F23,
    F24,
    F25,
    F26,
    F27,
    F28,
    F29,
    F30,
    F31,
    F32,
    F33,
    F34,
    F35,
    MediaPlay,
    MediaPause,
    MediaPlayPause,
    MediaReverse,
    MediaStop,
    MediaFastForward,
    MediaRewind,
    MediaTrackNext,
    MediaTrackPrevious,
    MediaRecord,
    LowerVolume,
    RaiseVolume,
    MuteVolume,
    LeftShift,
    LeftControl,
    LeftAlt,
    LeftSuper,
    LeftHyper,
    LeftMeta,
    RightShift,
    RightControl,
    RightAlt,
    RightSuper,
    RightHyper,
    RightMeta,
    IsoLevel3Shift,
    IsoLevel5Shift,
}

impl PuaKey {
    /// The X keysym ([`KeyCode::keysym`]): `XK_F21` to `XK_F35`, the locks,
    /// `XK_Print`, `XK_Pause`, `XK_Menu`, the modifier keys, and the media
    /// keys' `XF86Audio*`, as kitty's keyboard layer maps them
    /// (`[[source: kitty-xkb-glfw-c]]`). `None` for play-pause and reverse,
    /// which kitty numbers and maps from no keysym.
    pub fn keysym(self) -> Option<u32> {
        Some(match self {
            PuaKey::CapsLock => 0xffe5,
            PuaKey::ScrollLock => 0xff14,
            PuaKey::NumLock => 0xff7f,
            PuaKey::PrintScreen => 0xff61,
            PuaKey::Pause => 0xff13,
            PuaKey::Menu => 0xff67,
            PuaKey::F21 => 0xffd2,
            PuaKey::F22 => 0xffd3,
            PuaKey::F23 => 0xffd4,
            PuaKey::F24 => 0xffd5,
            PuaKey::F25 => 0xffd6,
            PuaKey::F26 => 0xffd7,
            PuaKey::F27 => 0xffd8,
            PuaKey::F28 => 0xffd9,
            PuaKey::F29 => 0xffda,
            PuaKey::F30 => 0xffdb,
            PuaKey::F31 => 0xffdc,
            PuaKey::F32 => 0xffdd,
            PuaKey::F33 => 0xffde,
            PuaKey::F34 => 0xffdf,
            PuaKey::F35 => 0xffe0,
            PuaKey::MediaPlay => 0x1008_ff14,
            PuaKey::MediaPause => 0x1008_ff31,
            PuaKey::MediaPlayPause | PuaKey::MediaReverse => return None,
            PuaKey::MediaStop => 0x1008_ff15,
            PuaKey::MediaFastForward => 0x1008_ff97,
            PuaKey::MediaRewind => 0x1008_ff3e,
            PuaKey::MediaTrackNext => 0x1008_ff17,
            PuaKey::MediaTrackPrevious => 0x1008_ff16,
            PuaKey::MediaRecord => 0x1008_ff1c,
            PuaKey::LowerVolume => 0x1008_ff11,
            PuaKey::RaiseVolume => 0x1008_ff13,
            PuaKey::MuteVolume => 0x1008_ff12,
            PuaKey::LeftShift => 0xffe1,
            PuaKey::LeftControl => 0xffe3,
            PuaKey::LeftAlt => 0xffe9,
            PuaKey::LeftSuper => 0xffeb,
            PuaKey::LeftHyper => 0xffed,
            PuaKey::LeftMeta => 0xffe7,
            PuaKey::RightShift => 0xffe2,
            PuaKey::RightControl => 0xffe4,
            PuaKey::RightAlt => 0xffea,
            PuaKey::RightSuper => 0xffec,
            PuaKey::RightHyper => 0xffee,
            PuaKey::RightMeta => 0xffe8,
            PuaKey::IsoLevel3Shift => 0xfe03,
            PuaKey::IsoLevel5Shift => 0xfe11,
        })
    }

    /// The key's number in `[[source: kitty-keyboard]]`'s functional table.
    pub fn kitty_number(self) -> u32 {
        match self {
            PuaKey::CapsLock => 57358,
            PuaKey::ScrollLock => 57359,
            PuaKey::NumLock => 57360,
            PuaKey::PrintScreen => 57361,
            PuaKey::Pause => 57362,
            PuaKey::Menu => 57363,
            PuaKey::F21 => 57384,
            PuaKey::F22 => 57385,
            PuaKey::F23 => 57386,
            PuaKey::F24 => 57387,
            PuaKey::F25 => 57388,
            PuaKey::F26 => 57389,
            PuaKey::F27 => 57390,
            PuaKey::F28 => 57391,
            PuaKey::F29 => 57392,
            PuaKey::F30 => 57393,
            PuaKey::F31 => 57394,
            PuaKey::F32 => 57395,
            PuaKey::F33 => 57396,
            PuaKey::F34 => 57397,
            PuaKey::F35 => 57398,
            PuaKey::MediaPlay => 57428,
            PuaKey::MediaPause => 57429,
            PuaKey::MediaPlayPause => 57430,
            PuaKey::MediaReverse => 57431,
            PuaKey::MediaStop => 57432,
            PuaKey::MediaFastForward => 57433,
            PuaKey::MediaRewind => 57434,
            PuaKey::MediaTrackNext => 57435,
            PuaKey::MediaTrackPrevious => 57436,
            PuaKey::MediaRecord => 57437,
            PuaKey::LowerVolume => 57438,
            PuaKey::RaiseVolume => 57439,
            PuaKey::MuteVolume => 57440,
            PuaKey::LeftShift => 57441,
            PuaKey::LeftControl => 57442,
            PuaKey::LeftAlt => 57443,
            PuaKey::LeftSuper => 57444,
            PuaKey::LeftHyper => 57445,
            PuaKey::LeftMeta => 57446,
            PuaKey::RightShift => 57447,
            PuaKey::RightControl => 57448,
            PuaKey::RightAlt => 57449,
            PuaKey::RightSuper => 57450,
            PuaKey::RightHyper => 57451,
            PuaKey::RightMeta => 57452,
            PuaKey::IsoLevel3Shift => 57453,
            PuaKey::IsoLevel5Shift => 57454,
        }
    }
}

impl KeyCode {
    /// The X keysym this key is, as xterm names a key (`[[source:
    /// xorg-keysymdef]]`): what xterm's `TypeOfKeysym` classes and
    /// `keysym2ucs` numbers when a key-modifier resource is at 4. Tab with
    /// Shift is `ISO_Left_Tab`, the US keymap's Shift level of Tab. `None`
    /// for a text key, which xterm reaches through `modifyOtherKeys`.
    /// `keysyms_are_the_headers` gates every row against the header.
    pub fn keysym(self, mods: Modifiers) -> Option<u32> {
        Some(match self {
            KeyCode::Char(_) => return None,
            KeyCode::Up => 0xff52,
            KeyCode::Down => 0xff54,
            KeyCode::Right => 0xff53,
            KeyCode::Left => 0xff51,
            KeyCode::Home => 0xff50,
            KeyCode::End => 0xff57,
            KeyCode::Insert => 0xff63,
            KeyCode::Delete => 0xffff,
            KeyCode::PageUp => 0xff55,
            KeyCode::PageDown => 0xff56,
            KeyCode::Function(f) => 0xffbe + u32::from(f.number()) - 1,
            KeyCode::Keypad(k) => k.keysym(),
            KeyCode::Enter => 0xff0d,
            KeyCode::Tab if mods.has(Modifiers::SHIFT) => 0xfe20,
            KeyCode::Tab => 0xff09,
            KeyCode::Backspace => 0xff08,
            KeyCode::Escape => 0xff1b,
            KeyCode::Pua(p) => p.keysym()?,
        })
    }

    /// The `CSI <param> ~` parameter for the editing keypad, from
    /// `[[source: ctlseqs]]` line 2672.
    ///
    /// Insert 2, Delete 3, PageUp 5, PageDown 6. Unlike the cursor keys these
    /// do **not** change with DECCKM -- ctlseqs gives the same sequence in both
    /// columns -- which is why they are a separate rule from `Home`/`End`.
    pub fn tilde_param(self) -> Option<Param> {
        Some(Param(match self {
            KeyCode::Insert => 2,
            KeyCode::Delete => 3,
            KeyCode::PageUp => 5,
            KeyCode::PageDown => 6,
            KeyCode::Function(f) => return f.tilde_param(),
            _ => return None,
        }))
    }

    /// The final byte for a key of the `<introducer> <final>` shape.
    ///
    /// Home and End are here because `[[source: ctlseqs]]` line 2485 puts them
    /// in the arrows' table and calls them cursor keys, so DECCKM governs them
    /// too. `Begin` has no `KeyCode` -- it exists only on the keypad, as
    /// [`KeypadKey::Begin`].
    pub fn cursor_final(self) -> Option<FinalByte> {
        Some(FinalByte(match self {
            KeyCode::Up => b'A',
            KeyCode::Down => b'B',
            KeyCode::Right => b'C',
            KeyCode::Left => b'D',
            KeyCode::End => b'F',
            KeyCode::Home => b'H',
            _ => return None,
        }))
    }
}

/// Whether a key went down, came up, or repeated.
///
/// An enum and not a pair of `bool`s: it records which kind the event *is*
/// rather than two flags that can both be set. Only
/// `[[source: kitty-keyboard]]`'s event types report the kind; everywhere else
/// "key repeat events are treated as key press events" and a release sends
/// nothing. So an embedder delivers every kind it knows, and the encoder
/// decides what each is worth: a repeat delivered as a press loses only the
/// `:2` an application asked for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum KeyEventKind {
    #[default]
    Press,
    Repeat,
    Release,
}

/// One key event, before any layer has encoded it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub mods: Modifiers,
    pub kind: KeyEventKind,
    /// The character this key produced, distinct from its identity in
    /// [`KeyCode::Char`]. For an unmodified letter the two are the same; under
    /// shift, AltGr, a dead key or an IME they differ -- `shift+a` is identity
    /// `a` and produced `A`. The base layer sends this, and it is
    /// `[[source: kitty-keyboard]]`'s associated text (flag 16). `None` for a
    /// key that produces none: a functional key, a bare modifier, or a control
    /// combination. A single codepoint; a multi-codepoint IME commit is a later
    /// widening.
    pub text: Option<Codepoint>,
    /// `[[source: kitty-keyboard]]`'s alternate keys (flag 4), for shortcut
    /// matching: the *shifted* codepoint (the layout's shifted version of the
    /// key, `a` -> `A`) and the *base-layout* codepoint (the same physical key
    /// in the standard PC-101 layout, so a Cyrillic `С` reports `c`). Both are
    /// what the platform supplies; `None` when it does not, or for a key with
    /// no such variant. Read only by the kitty encoder, and the shifted key is
    /// emitted only when shift is also in the modifiers.
    pub shifted: Option<Codepoint>,
    pub base_layout: Option<Codepoint>,
}

impl KeyEvent {
    /// A plain press, which is every event on the base layer.
    ///
    /// The produced text defaults to the key's own identity -- the unmodified
    /// case, where `Char('a')` produces `a`. An embedder with a shifted or IME
    /// character sets [`KeyEvent::text`] instead, and a functional key produces
    /// none.
    pub fn press(code: KeyCode, mods: Modifiers) -> KeyEvent {
        KeyEvent {
            code,
            mods,
            kind: KeyEventKind::Press,
            text: match code {
                KeyCode::Char(cp) => Some(cp),
                _ => None,
            },
            shifted: None,
            base_layout: None,
        }
    }
}
