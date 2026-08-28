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
