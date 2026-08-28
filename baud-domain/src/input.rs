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

    /// Every modifier the wire form can carry, which bounds `wire`.
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
        1 + self.0
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

/// A key, named by what it means rather than where it sits on a keyboard.
///
/// Physical position is the platform layer's problem; baud has no event
/// source. What arrives here is already resolved to a key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyCode {
    /// A key that produces text. The codepoint is what the layout resolved,
    /// not a scancode.
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
}

impl KeyCode {
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
    /// too. `Begin` has no `KeyCode` -- it exists only as keypad `5`.
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
/// An enum and not a pair of `bool`s. Only `[[source: kitty-keyboard]]` reports
/// anything but `Press`, and only when an application has asked for event
/// types -- so a terminal that reports presses alone is complete, and this
/// records which kind it *is* rather than two flags that can both be set.
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
}

impl KeyEvent {
    /// A plain press, which is every event on the base layer.
    pub fn press(code: KeyCode, mods: Modifiers) -> KeyEvent {
        KeyEvent {
            code,
            mods,
            kind: KeyEventKind::Press,
        }
    }
}
