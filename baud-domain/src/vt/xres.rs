//! XTGETXRES, `DCS + Q Pt ST`: a program asks for xterm resources by name
//! and is told the value baud starts from.
//!
//! `[[source: ctlseqs]]` 510-533: "The string following the "Q" is a list of
//! names encoded in hexadecimal (2 digits per character) separated by ; which
//! correspond to xterm resource names", answered `DCS 1 + R Pt ST` "adding to
//! Pt an = , and the value of the corresponding xterm resource", or `DCS 0 +
//! R Pt ST` for an invalid request; "Only boolean, numeric and string
//! resources for the VT100 widget are supported", and "XTerm evaluates
//! resources at startup time".
//!
//! **The answer is xterm's `do_dcs`, byte for byte** (`[[source:
//! xterm-misc]]`'s file, misc.c 5251-5306; Jev, XL_XTERM, 0.98/0.98): the
//! first name decides the `1` or `0`; each name is echoed as it was sent,
//! a known one followed by `=` and its value in upper-case hexadecimal
//! (`x_encode_hex`); the list ends after the first unknown name, which is
//! echoed with the `;` after it if one was sent; an empty name or an odd
//! count of hex digits ends it before that name (`x_decode_hex`,
//! `[[source: xterm-xstrings-c]]`), and the reply is sent only once a first
//! name was read.
//!
//! **Which names answer is [`XResource::of`]** (Jev, X_STARTUP,
//! 0.98/0.88 then 0.98/0.97): the seven `allow*` resources, from the facts
//! `OSC 60` reports; each resource that names the initial value of a mode
//! or setting baud has, at baud's power-on value; and the five the
//! embedder gives the core at start, the cursor's blink and shape, the
//! pointer mode and the ambiguous width. Every other name is unknown, as xterm answers a
//! name it has no resource for -- `termName` among them, an application
//! resource and not the widget's. xterm's widget resources and their types
//! are its `xterm_resources` table and `vt100ResourceToString`
//! (`[[source: xterm-charproc-c-bccd466]]`, charproc.c 404-830 and
//! 10381-10425).

use crate::*;

/// A resource baud answers, by where its value comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XResource {
    /// One of the `allow*` resources: `true` while `OSC 60` lists it.
    Allow(AllowCategory),
    /// A value baud fixes at power-on.
    Fixed(XValue),
    /// `cursorBlink`, a string: whether the cursor the embedder set at
    /// start blinks (`Term::set_initial_cursor_style`), `true` or `false`.
    CursorBlink,
    /// `cursorUnderLine` and `cursorBar`, booleans: whether the cursor the
    /// embedder set at start is an underline, or a bar.
    CursorUnderLine,
    CursorBar,
    /// `pointerMode`, an integer: the embedder's initial XTSMPOINTER mode
    /// (`Term::set_initial_pointer_mode`).
    PointerMode,
    /// `cjkWidth`, a boolean: whether ambiguous-width characters are wide
    /// (`Term::set_ambiguous_width`).
    CjkWidth,
}

/// A resource's value, as xterm's `vt100ResourceToString` spells its type:
/// a boolean `true` or `false`, an integer in decimal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XValue {
    True,
    False,
    Int(u8),
}

impl XValue {
    /// A two-state value from whether it holds.
    pub const fn of(on: bool) -> XValue {
        match on {
            true => XValue::True,
            false => XValue::False,
        }
    }

    /// The value as xterm writes it, before it is hex-encoded.
    pub fn text(self, out: &mut [u8; 3]) -> &[u8] {
        match self {
            XValue::True => b"true",
            XValue::False => b"false",
            XValue::Int(n) => {
                let mut i = out.len();
                let mut v = n;
                loop {
                    i -= 1;
                    out[i] = b'0' + v % 10;
                    v /= 10;
                    if v == 0 {
                        break;
                    }
                }
                &out[i..]
            }
        }
    }
}

/// The longest value text: `false`.
pub const XVALUE_MAX: usize = 5;

/// The shortest names [`XResource::of`] knows: `c132` and `utf8`.
pub const XNAME_MIN: usize = 4;

/// The longest name [`XResource::of`] knows: `privateColorRegisters`. A
/// longer name is unknown without being decoded whole.
pub const XNAME_MAX: usize = 21;

impl XResource {
    /// The resource a decoded name is, matched exactly as xterm's
    /// `findVT100Resource` compares it (`strcmp`). Each fixed value is
    /// baud's power-on state of the mode or setting the resource starts,
    /// which `every_mode_resource_is_the_fresh_terminals_mode` holds to a
    /// fresh terminal's `DECRQM` answers. Three read the other way round:
    /// `jumpScroll` is DECSCLM `?4` reset, `sixelScrolling` is DECSDM `?80`
    /// reset, and `titeInhibit` is `?1046` reset.
    pub fn of(name: &[u8]) -> Option<XResource> {
        use XResource::{Allow, Fixed};
        use XValue::{False, Int, True};
        Some(match name {
            b"allowColorOps" => Allow(AllowCategory::ColorOps),
            b"allowFontOps" => Allow(AllowCategory::FontOps),
            b"allowMouseOps" => Allow(AllowCategory::MouseOps),
            b"allowPasteControls" => Allow(AllowCategory::PasteControls),
            b"allowTcapOps" => Allow(AllowCategory::TcapOps),
            b"allowTitleOps" => Allow(AllowCategory::TitleOps),
            b"allowWindowOps" => Allow(AllowCategory::WindowOps),
            // The modes' initial values.
            b"appcursorDefault" => Fixed(False),
            b"appkeypadDefault" => Fixed(False),
            b"jumpScroll" => Fixed(True),
            b"autoWrap" => Fixed(True),
            b"reverseWrap" => Fixed(False),
            b"c132" => Fixed(False),
            b"reverseVideo" => Fixed(False),
            b"marginBell" => Fixed(False),
            // `?30`, shown at power-on, a reset keeping it.
            b"scrollBar" => Fixed(True),
            // `?14` is never what baud does, and `?1014`, `?1044` and
            // `?1023` are what it always does: permanent, or read-only.
            b"cursorBlinkXOR" => Fixed(False),
            b"fastScroll" => Fixed(True),
            b"keepClipboard" => Fixed(True),
            b"privateWidth" => Fixed(True),
            // `?1020`: UTF-8 at power-on, which `ESC % @` can leave, so
            // `true` and not xterm's `always`.
            b"utf8" => Fixed(True),
            b"curses" => Fixed(False),
            b"alternateScroll" => Fixed(True),
            b"bellIsUrgent" => Fixed(False),
            b"popOnBell" => Fixed(False),
            b"metaSendsEscape" => Fixed(True),
            b"altSendsEscape" => Fixed(True),
            b"eightBitInput" => Fixed(False),
            // `?1034` sets the variable `eightBitMeta` starts, as well.
            b"eightBitMeta" => Fixed(False),
            b"deleteIsDEL" => Fixed(False),
            b"backarrowKey" => Fixed(False),
            b"numLock" => Fixed(True),
            b"privateColorRegisters" => Fixed(True),
            b"sixelScrolling" => Fixed(True),
            b"sixelScrollsRight" => Fixed(False),
            b"titeInhibit" => Fixed(False),
            b"scrollTtyOutput" => Fixed(False),
            b"scrollKey" => Fixed(True),
            b"keepSelection" => Fixed(True),
            b"selectToClipboard" => Fixed(False),
            b"printerFormFeed" => Fixed(False),
            b"printerExtent" => Fixed(False),
            b"logging" => Fixed(False),
            // `S8C1T` at power-on: seven-bit replies (the DECRQSS `"p`
            // answer, `64;1"p`).
            b"eightBitControl" => Fixed(False),
            // XTSMTITLE: no title mode set at power-on.
            b"titleModes" => Fixed(Int(0)),
            // The key-modifier resources at power-on, as XTQMODKEYS and
            // XTQFMTKEYS answer them.
            b"modifyKeyboard" => Fixed(Int(0)),
            b"modifyCursorKeys" => Fixed(Int(2)),
            b"modifyFunctionKeys" => Fixed(Int(2)),
            b"modifyKeypadKeys" => Fixed(Int(0)),
            b"modifyOtherKeys" => Fixed(Int(0)),
            b"modifyModifierKeys" => Fixed(Int(0)),
            b"modifySpecialKeys" => Fixed(Int(0)),
            b"formatCursorKeys" => Fixed(Int(0)),
            b"formatFunctionKeys" => Fixed(Int(0)),
            b"formatKeypadKeys" => Fixed(Int(0)),
            b"formatOtherKeys" => Fixed(Int(0)),
            b"formatModifierKeys" => Fixed(Int(0)),
            b"formatSpecialKeys" => Fixed(Int(0)),
            b"cursorBlink" => XResource::CursorBlink,
            b"cursorUnderLine" => XResource::CursorUnderLine,
            b"cursorBar" => XResource::CursorBar,
            b"pointerMode" => XResource::PointerMode,
            b"cjkWidth" => XResource::CjkWidth,
            _ => return None,
        })
    }
}

/// The longest XTGETXRES answer to a list of `list` bytes: every name the
/// shortest known one, each with the longest value, and the one unknown name
/// echoed is no longer than the list.
pub const fn xres_reply_max(list: usize) -> usize {
    let per = 2 * XNAME_MIN + 1;
    let names = list / per + 1;
    b"\x1bP".len() + b"1+R".len() + list + names * (1 + 2 * XVALUE_MAX) + b"\x1b\\".len()
}

/// A hex digit's value, either case, as `x_hex2int` reads it.
fn hex_value(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        _ => b - b'A' + 10,
    }
}

/// One name's hex digits from `at`: where they end, and the decoded name
/// when it is short enough to be one [`XResource::of`] knows. `None` for an
/// odd count, which `x_decode_hex` refuses.
fn decode_name(
    list: &[u8],
    at: usize,
    name: &mut [u8; XNAME_MAX],
) -> Option<(usize, Option<usize>)> {
    let digits = list[at..]
        .iter()
        .take_while(|b| b.is_ascii_hexdigit())
        .count();
    if digits % 2 != 0 {
        return None;
    }
    let len = digits / 2;
    if len > XNAME_MAX {
        return Some((at + digits, None));
    }
    for (i, pair) in list[at..at + digits].chunks_exact(2).enumerate() {
        name[i] = (hex_value(pair[0]) << 4) | hex_value(pair[1]);
    }
    Some((at + digits, Some(len)))
}

/// Answer an XTGETXRES whose list, the bytes after its `Q`, is `list`, into
/// `out`, in the controls `c1` names, each known resource's value from
/// `value`. **Whole or nothing**: `None`, and nothing to send, when `out`
/// cannot hold it; `Some(ByteLen::ZERO)`, and nothing to send, when the list
/// has no first name xterm reads.
pub fn encode_xres_reply(
    list: &[u8],
    c1: C1Transmit,
    value: impl Fn(XResource) -> XValue,
    out: &mut [u8],
) -> Option<ByteLen> {
    const D: &[u8; 16] = b"0123456789ABCDEF";
    let mut w = 0;
    let mut fits = true;
    let mut put = |w: &mut usize, bytes: &[u8]| match out.get_mut(*w..*w + bytes.len()) {
        Some(dst) if fits => {
            dst.copy_from_slice(bytes);
            *w += bytes.len();
        }
        _ => fits = false,
    };
    let mut at = 0;
    let mut first = true;
    let mut name = [0u8; XNAME_MAX];
    loop {
        if at == list.len() {
            break;
        }
        let Some((end, len)) = decode_name(list, at, &mut name) else {
            break;
        };
        if end == at {
            break;
        }
        let found = len.and_then(|n| XResource::of(&name[..n]));
        if first {
            put(&mut w, c1.dcs());
            put(
                &mut w,
                match found {
                    Some(_) => b"1+R",
                    None => b"0+R",
                },
            );
            first = false;
        }
        put(&mut w, &list[at..end]);
        if let Some(r) = found {
            put(&mut w, b"=");
            let mut buf = [0u8; 3];
            for &b in value(r).text(&mut buf) {
                put(&mut w, &[D[usize::from(b >> 4)], D[usize::from(b & 15)]]);
            }
        }
        at = end;
        if list.get(at) == Some(&b';') {
            put(&mut w, b";");
            at += 1;
        }
        if found.is_none() {
            break;
        }
    }
    if !first {
        put(&mut w, c1.st());
    }
    match fits {
        true => Some(ByteLen::from_index(w)),
        false => None,
    }
}
