//! XTGETTCAP, `DCS + q Pt ST`: what a program learns of the keys it will
//! receive, by asking the terminal rather than its terminfo entry.
//!
//! `[[source: ctlseqs]]` 545-582. The names are "termcap or terminfo
//! capability names for special keyboard keys", hex-encoded and separated by
//! `;`, plus `Co`/`colors`, `TN`/`name` and `RGB`; every other capability is
//! "intentionally not part of this interface". The answer is `DCS 1 + r Pt ST`
//! "adding to Pt an = , and the value of the corresponding string that xterm
//! would send", every pair in one response, or `DCS 0 + r ST`; "An invalid
//! name (one not found in xterm's tables) ends processing of the list".
//!
//! **The names are xterm's table**, `[[source: xterm-xtermcap-c]]` 70-244,
//! compiled into `tcap_data` by `build/tcap.rs` and indexed at build
//! time: a request costs a hash and at most [`TCAP_PROBES`]
//! compares, never a walk of the table. **A key's value is what baud's key
//! sends now**: the row's key and modifiers through [`encode_key`] under the
//! live [`KeyState`], the press xterm simulates through `Input()`
//! (`[[source: xterm-misc]]` 5227-5232). A key that sends nothing in that
//! state -- one baud has no code for, F21-F35 before a kitty flag, any key
//! under KAM -- answers the empty string and the list goes on, xterm's own
//! answer for Undo, Clear and every key under KAM (note AM, section 3).

use super::tcap_data::{MUL, ROWS};
use crate::*;

/// What one of xterm's names answers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcapValue {
    /// A key, pressed with these modifiers.
    Key(KeyCode, Modifiers),
    /// A key in xterm's table that baud has no code for: Help, Find, Select,
    /// Clear, Undo and F36-F63. It sends nothing, so it answers empty.
    Absent,
    /// `Co`/`colors`: the palette's 256 colours, xterm's `NUM_ANSI_COLORS`.
    Colors,
    /// `RGB`: 8 bits a channel, xterm's answer while it draws direct colour,
    /// which baud always does.
    Rgb,
    /// `TN`/`name`: the terminal description the embedder names, `TERM`.
    Name,
}

/// A row's key with its modifier parameter, for the generated table.
pub(crate) const fn tc_key(code: KeyCode, param: u8) -> TcapValue {
    TcapValue::Key(code, row_mods(param))
}

/// A row's function key `F<n>`, F1-F20.
pub(crate) const fn tc_fn(n: u8, param: u8) -> TcapValue {
    match FnKey::new(n) {
        Some(k) => tc_key(KeyCode::Function(k), param),
        None => panic!("a function key past F20 is not an FnKey"),
    }
}

/// A row's keypad key.
pub(crate) const fn tc_pad(k: KeypadKey, param: u8) -> TcapValue {
    tc_key(KeyCode::Keypad(k), param)
}

/// A row's modifier parameter as baud's [`Modifiers`]: 0 is none, and 2-8
/// are xterm's `1 + mask` with shift 1, alt 2 and control 4
/// (`[[source: ctlseqs]]`'s modifier table; `SHIFT` is `MOD_NONE +
/// MOD_SHIFT`, xtermcap.c 60).
const fn row_mods(param: u8) -> Modifiers {
    let bits = match param {
        0 => 0,
        2..=8 => param - 1,
        _ => panic!("a modifier parameter outside 0 and 2-8"),
    };
    let mut out = Modifiers::NONE;
    if bits & 1 != 0 {
        out = out.with(Modifiers::SHIFT);
    }
    if bits & 2 != 0 {
        out = out.with(Modifiers::ALT);
    }
    if bits & 4 != 0 {
        out = out.with(Modifiers::CTRL);
    }
    out
}

/// The longest name, in bytes: `kpZRO8` and its kind are six, and seven
/// leaves the length byte of [`pack`] room.
const NAME_MAX: usize = 7;

/// The shortest name, in bytes: the termcap codes are two. It bounds how
/// many pairs one request can hold ([`tcap_reply_max`]).
pub const TCAP_NAME_MIN: usize = {
    let mut min = usize::MAX;
    let mut i = 0;
    while i < ROWS.len() {
        let n = ROWS[i].0.len();
        assert!(n >= 1 && n <= NAME_MAX, "a name outside 1-7 bytes");
        if n < min {
            min = n;
        }
        i += 1;
    }
    min
};
const _: () = assert!(TCAP_NAME_MIN == 2);

/// A name as one word: its length in the top byte, its bytes below.
const fn pack(name: &[u8]) -> Option<u64> {
    if name.is_empty() || name.len() > NAME_MAX {
        return None;
    }
    let mut v = name.len() as u64;
    let mut i = 0;
    while i < name.len() {
        v = (v << 8) | name[i] as u64;
        i += 1;
    }
    Some(v)
}

/// Slots in the index: a power of two, about three to a name.
const SLOTS: usize = 1024;

/// The bucket, from the **high** bits of a multiplicative hash, as
/// `StyleId::bucket` takes them, by the multiplier `build/tcap.rs` searches
/// for. The golden ratio's walks seven slots at worst over this table.
const fn bucket(k: u64) -> usize {
    (k.wrapping_mul(MUL) >> (64 - SLOTS.trailing_zeros())) as usize
}

/// The open-addressed index, built at compile time: each slot holds a row
/// of [`ROWS`] plus one, or 0.
struct Index {
    slot: [u16; SLOTS],
    /// The longest walk past a name's own bucket that any name needs.
    probes: usize,
}

const INDEX: Index = {
    let mut slot = [0u16; SLOTS];
    let mut probes = 0;
    let mut r = 0;
    while r < ROWS.len() {
        let k = match pack(ROWS[r].0) {
            Some(k) => k,
            None => panic!("a name pack cannot hold"),
        };
        let mut i = bucket(k);
        let mut n = 0;
        while slot[i] != 0 {
            match pack(ROWS[slot[i] as usize - 1].0) {
                Some(o) if o == k => panic!("a name twice in the table"),
                _ => {}
            }
            i = (i + 1) & (SLOTS - 1);
            n += 1;
        }
        slot[i] = (r + 1) as u16;
        if n > probes {
            probes = n;
        }
        r += 1;
    }
    Index { slot, probes }
};

/// The most compares a lookup makes past its first: measured over the
/// table when it is built, and held here so a regenerated table that walks
/// further fails to compile rather than slows quietly.
pub const TCAP_PROBES: usize = INDEX.probes;
const _: () = assert!(TCAP_PROBES <= 1);

/// One of xterm's names, by its exact bytes (`lookupTcapByName` compares
/// with `strcmp`, so case matters).
pub fn tcap_lookup(name: &[u8]) -> Option<TcapValue> {
    let k = pack(name)?;
    let mut i = bucket(k);
    for _ in 0..=TCAP_PROBES {
        let s = INDEX.slot[i] as usize;
        if s == 0 {
            return None;
        }
        let (n, v) = ROWS[s - 1];
        if n == name {
            return Some(v);
        }
        i = (i + 1) & (SLOTS - 1);
    }
    None
}

/// The longest `TERM` the core keeps for `TN`: ncurses' own names run to
/// 21 (`screen.xterm-256color`), and the bound enters [`tcap_reply_max`].
pub const TERM_NAME_MAX: usize = 32;

/// The terminal description `TN` and `name` answer: what the embedder
/// exports as `TERM`, or none, when no embedder said, and then `TN` is a
/// name the core cannot answer and is refused.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TermName {
    bytes: [u8; TERM_NAME_MAX],
    len: ByteLen,
}

impl TermName {
    /// No name told.
    pub const UNTOLD: TermName = TermName {
        bytes: [0; TERM_NAME_MAX],
        len: ByteLen::ZERO,
    };

    /// A `TERM` of 1 to [`TERM_NAME_MAX`] bytes, each `21-7E`: a terminfo
    /// name has no space or control in it (`[[source: terminfo-man5]]`,
    /// "the names ... are separated by vertical bars"). `None` otherwise.
    pub fn new(name: &[u8]) -> Option<TermName> {
        if name.is_empty()
            || name.len() > TERM_NAME_MAX
            || !name.iter().all(|b| (0x21..0x7f).contains(b))
        {
            return None;
        }
        let mut bytes = [0; TERM_NAME_MAX];
        bytes[..name.len()].copy_from_slice(name);
        Some(TermName {
            bytes,
            len: ByteLen::from_index(name.len()),
        })
    }

    /// The name, or `None` if none was told.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self.len.index() {
            0 => None,
            n => Some(&self.bytes[..n]),
        }
    }
}

/// The longest value one name answers: a key's bytes, `TN`'s name, or
/// `256`.
const VALUE_MAX: usize = {
    let mut v = KeyBytes::MAX_LEN;
    if TERM_NAME_MAX > v {
        v = TERM_NAME_MAX;
    }
    v
};

/// The longest answer to a list of `list` bytes: the introducer and `1+r`,
/// every name and `;` echoed as sent, then for each pair an `=` and its
/// value in hex, and the terminator. A pair takes at least
/// [`TCAP_NAME_MIN`] bytes of name, two hex digits each, and a `;` between,
/// so a list holds at most `(list + 1) / 5` of them.
pub const fn tcap_reply_max(list: usize) -> usize {
    let pairs = (list + 1) / (2 * TCAP_NAME_MIN + 1);
    2 + 3 + list + pairs * (1 + 2 * VALUE_MAX) + 2
}

/// One name of the list, from `at`: its hex digits up to the next `;` or
/// the list's end, decoded and found, with where it ended. **Refused**, as
/// xterm refuses it (`x_decode_hex`, `[[source: xterm-xstrings-c]]`
/// 96-123, and `[[source: xterm-xtermcap-c]]` 391-402), when it is empty,
/// has an odd number of digits, holds a byte that is not a hex digit, or is
/// longer than any name; and a `TN` the core was not told is refused too.
fn name_at(list: &[u8], at: usize, term: &TermName) -> (Option<TcapValue>, usize) {
    let digits = list[at..]
        .iter()
        .take_while(|b| b.is_ascii_hexdigit())
        .count();
    let end = at + digits;
    let ends = end == list.len() || list[end] == b';';
    if digits == 0 || digits % 2 != 0 || !ends || digits / 2 > NAME_MAX {
        return (None, end);
    }
    let mut name = [0u8; NAME_MAX];
    for (k, pair) in list[at..end].chunks_exact(2).enumerate() {
        name[k] = (hex_digit(pair[0]) << 4) | hex_digit(pair[1]);
    }
    let found = match tcap_lookup(&name[..digits / 2]) {
        Some(TcapValue::Name) if term.as_bytes().is_none() => None,
        v => v,
    };
    (found, end)
}

/// A hex digit's value, either case, as `x_hex2int` reads it.
fn hex_digit(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        _ => b - b'A' + 10,
    }
}

/// Bytes written into a reply, or the first that did not fit.
struct Put<'a> {
    out: &'a mut [u8],
    w: usize,
    fit: Fit,
}

impl Put<'_> {
    fn raw(&mut self, bytes: &[u8]) {
        match self.out.get_mut(self.w..self.w + bytes.len()) {
            Some(dst) if self.fit == Fit::Whole => {
                dst.copy_from_slice(bytes);
                self.w += bytes.len();
            }
            _ => self.fit = Fit::Overflowed,
        }
    }
    /// A value, two upper-case hex digits a byte, as xterm's `unparseputc`
    /// writes it while `tc_query_code` is set (`[[source: xterm-reset]]`'s
    /// file, charproc.c, 9467-9479).
    fn hex(&mut self, bytes: &[u8]) {
        const D: &[u8; 16] = b"0123456789ABCDEF";
        for &b in bytes {
            self.raw(&[D[usize::from(b >> 4)], D[usize::from(b & 15)]]);
        }
    }
}

/// Answer an XTGETTCAP whose list, the bytes after its `q`, is `list`, into
/// `out`, in the controls `c1` names. **Whole or nothing**: `None`, and
/// nothing to send, when `out` cannot hold all of it, since a truncated
/// reply is a wrong answer.
///
/// One response, `DCS 1 + r n1=v1;n2=v2 ST`, ending at the first name
/// refused, without the `;` before it; `DCS 0 + r ST` when the first is.
/// A name is echoed as it was sent, in either case, as xterm copies the
/// request's bytes.
pub fn encode_tcap_reply(
    list: &[u8],
    c1: C1Transmit,
    keys: KeyState,
    term: TermName,
    out: &mut [u8],
) -> Option<ByteLen> {
    let mut p = Put {
        out,
        w: 0,
        fit: Fit::Whole,
    };
    p.raw(c1.dcs());
    let (mut found, mut end) = name_at(list, 0, &term);
    let mut at = 0;
    match found {
        None => p.raw(b"0+r"),
        Some(_) => p.raw(b"1+r"),
    }
    while let Some(v) = found {
        p.raw(&list[at..end]);
        p.raw(b"=");
        match v {
            TcapValue::Key(code, mods) => {
                // KAM: "The terminal ignores all keystrokes that send
                // characters to the host" (`[[source: vt510-kam]]`), and
                // xterm's `Input()` returns before encoding
                // (`[[source: xterm-input-c]]` 1015-1016).
                if !keys.modes.is_set(ModeBit::KeyboardAction) {
                    if let Some(k) = encode_key(KeyEvent::press(code, mods), keys) {
                        p.hex(k.as_bytes());
                    }
                }
            }
            TcapValue::Absent => {}
            TcapValue::Colors => p.hex(b"256"),
            TcapValue::Rgb => p.hex(b"8"),
            TcapValue::Name => p.hex(term.as_bytes().unwrap_or_default()),
        }
        if end == list.len() {
            break;
        }
        // `end` is a `;`, which `name_at` required.
        at = end + 1;
        (found, end) = name_at(list, at, &term);
        if found.is_some() {
            p.raw(b";");
        }
    }
    p.raw(c1.st());
    match p.fit {
        Fit::Whole => Some(ByteLen::from_index(p.w)),
        Fit::Overflowed => None,
    }
}
