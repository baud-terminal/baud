//! **What a paste sends**, the same on both drivers: xterm's default control
//! characters replaced by a space (`FILTER`), the text bracketed as `?2004`
//! asks, its line breaks as `?2006` says, and each byte quoted with the
//! tty's literal-next character under `?2005` (`readline.rs`), in the
//! encoding the child reads: UTF-8, or under `ESC % @` ISO 8859-1, as
//! xterm converts a selection outside UTF-8 (`latin1`). The
//! macOS app pastes the pasteboard with Cmd-V; the Linux driver pastes the
//! clipboard with Ctrl+Shift+V and xterm's SELECT with the middle button
//! and Shift+Insert. Pure, so its tests run on every platform.

use crate::readline::{self, Newlines};
use baud_core::{PasteControls, Resource, Term};
use baud_domain::{AmbiguousWidth, Classified, Codepoint, InputEncoding, Width};

/// **The control characters a paste replaces: xterm's default**, its
/// `disallowedPasteControls`, "BS,DEL,ENQ,EOT,ESC,NUL,STTY", each turned
/// into a space in every paste, bracketed or not, while every other C0
/// control passes (`[[source: xterm-man]]`; `removeControls`,
/// `[[source: xterm-button]]` 2582-2690). The conforming choice: keeping
/// the paste whole, or stripping the controls out, would each depart from it.
/// Both drivers tell the core, so `OSC 61 ; allowPasteControls` names
/// them (`host::set_window_abilities`).
pub const FILTER: PasteControls = PasteControls::XtermDefault;

/// **The tty's special characters**, xterm's `STTY`: each `c_cc` entry
/// `removeControls` reads ([`crate::sys::STTY_CHARS`]) that holds a C0
/// control or DEL, skipping NUL and one switched off, as a set over the
/// 128 ASCII bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Specials(u128);

impl Specials {
    /// The set `cc` gives at `indices`, with `disabled` the value that
    /// switches an entry off.
    pub fn of(cc: &[u8], indices: &[usize], disabled: Disabled) -> Specials {
        Specials(
            indices
                .iter()
                .filter_map(|&i| cc.get(i).copied())
                .filter(|&b| b != disabled.0 && b != 0 && (b < 0x20 || b == 0x7f))
                .fold(0, |set, b| set | (1u128 << b)),
        )
    }

    fn has(self, ch: char) -> bool {
        u8::try_from(ch).is_ok_and(|b| b < 0x80 && self.0 & (1u128 << b) != 0)
    }
}

/// The `c_cc` value that switches an entry off, `_POSIX_VDISABLE`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Disabled(pub u8);

/// Whether the child asked for bracketed paste.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bracket {
    On,
    Off,
}

/// How a paste goes to the child: bracketed or not (`?2004`), its line
/// breaks (`?2006`), and the literal-next byte that quotes each of its
/// bytes (`?2005`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Form {
    pub bracket: Bracket,
    pub newlines: Newlines,
    pub quote: Option<readline::Quote>,
    /// The tty's special characters, which [`FILTER`]'s `STTY` replaces.
    pub specials: Specials,
    /// The encoding the child reads input in (`ESC % G`, `ESC % @`), and
    /// the width the terminal gives an ambiguous character, which says
    /// what a wide one is.
    pub encoding: InputEncoding,
    pub ambiguous: AmbiguousWidth,
}

/// **The form the child's modes give a paste now**, with `literal_next`
/// asked only under `?2005`, and the tty's special characters always,
/// for `STTY`.
pub fn form(
    term: &Term<'_>,
    literal_next: impl FnOnce() -> readline::Quote,
    specials: impl FnOnce() -> Specials,
) -> Form {
    Form {
        bracket: match term.bracketed_paste() {
            true => Bracket::On,
            false => Bracket::Off,
        },
        newlines: match term.resource(Resource::PasteLiteralNewline) {
            true => Newlines::Literal,
            false => Newlines::AsReturn,
        },
        quote: term.resource(Resource::PasteQuoted).then(literal_next),
        specials: match FILTER.stty() {
            true => specials(),
            false => Specials::default(),
        },
        encoding: term.input_encoding(),
        ambiguous: term.ambiguous_width(),
    }
}

/// **The ASCII a DEC Special Graphics character pastes as**, xterm's
/// `DECtoASCII` (`[[source: xterm-button-c]]` 1719-1728) for the DEC byte
/// the code point is drawn from: the corners and the crossing, `j` to `n`,
/// and the tees, `t` to `w`, `+`; the horizontal line `q` `-`; the
/// vertical line `x` `|`; and `#` for the rest. Which code point is which
/// byte is baud's own table (`baud_domain::DEC_GRAPHICS`), the 31 xterm's
/// `ucs2dec` maps too (`[[source: xterm-fontutils]]` 4870-4922).
fn dec_to_ascii(dec: Byte) -> Byte {
    Byte(match dec.0 {
        b'j'..=b'n' | b't'..=b'w' => b'+',
        b'q' => b'-',
        b'x' => b'|',
        _ => b'#',
    })
}

/// A byte of the encoded paste.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Byte(u8);

/// xterm's `defaultString` resource as it ships, "#" (`[[source:
/// xterm-reset]]` 517): what a character with no single byte becomes.
const DEFAULT_STRING: u8 = b'#';

/// **`text` as ISO 8859-1, xterm's way**, `UTF8toLatin1` and then
/// `removeControls` outside UTF-8 (`[[source: xterm-button-c]]` 1749-1809,
/// 2662-2686): a code point below 256 is its own byte, except a C1 control,
/// 0x80 to 0x9F, which is deleted -- a C1 byte is a control to a child
/// reading ISO 8859-1 (`[[source: ecma48]]` § 5.3, "in an 8-bit code by bit
/// combinations from 08/00 to 09/15"; `[[source: iso8859-1]]`,
/// `<control>`), and 0x9B is CSI, which could end a bracketed paste early
/// (deleted rather than turned into a space). Above 256, a DEC Special
/// Graphics character becomes `DECtoASCII`'s, one of xterm's DEC Technical
/// internal codes `?`, a character `AsciiEquivs` names its ASCII
/// (`[[source: xterm-scroll]]` 3699-3735: dashes and the minus sign `-`,
/// the single quotation marks and the angle brackets, and the fullwidth
/// forms), and anything else `#`; a wide character that took no DEC
/// position is followed by a space, so it keeps its two columns. xterm's
/// `is_UCS_SPECIAL`, U+FFF0 to U+FFFF, is `#` (`[[source: xterm-ptyx-h]]`
/// 1819).
pub fn latin1(text: &str, ambiguous: AmbiguousWidth) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let v = u32::from(ch);
        if let Ok(b) = u8::try_from(v) {
            if !(0x80..=0x9f).contains(&b) {
                out.push(b);
            }
            continue;
        }
        if (0xfff0..=0xffff).contains(&v) {
            out.push(DEFAULT_STRING);
            continue;
        }
        // xterm's `xtermIsInternalCs`: a DEC graphic, `` ` `` to `~`, its
        // positions 1 to 31 (`xtermIsDecGraphic`, `[[source:
        // xterm-fontutils-h]]` 157), or one of the seven DEC Technical
        // codes from `XTERM_PUA`, 0xEEEE (`[[source: xterm-ptyx-h]]` 380).
        let dec = baud_domain::DEC_GRAPHICS
            .iter()
            .zip(b'`'..=b'~')
            .find_map(|(&u, b)| (u == v).then_some(b));
        if let Some(b) = dec {
            out.push(dec_to_ascii(Byte(b)).0);
            continue;
        }
        if (0xeeee..=0xeeee + 6).contains(&v) {
            out.push(b'?');
            continue;
        }
        match ascii_equivalent(ch) {
            Some(b) => out.push(b.0),
            None => out.push(DEFAULT_STRING),
        }
        let policy = baud_domain::WidthPolicy::of(ambiguous, baud_domain::UnicodeWidths::Current);
        if Classified::of(Codepoint::from_char(ch)).width(policy) == Width::WIDE {
            out.push(b' ');
        }
    }
    out
}

/// xterm's `AsciiEquivs`, the characters groff substitutes and their
/// ASCII, and the fullwidth forms: `None` where it names none.
fn ascii_equivalent(ch: char) -> Option<Byte> {
    let v = u32::from(ch);
    let b = match v {
        0x2010..=0x2015 | 0x2212 => b'-',
        0x2018 => b'`',
        0x2019 => b'\'',
        0x201c | 0x201d => b'"',
        0x2329 => b'<',
        0x232a => b'>',
        0xff01..=0xff5e => u8::try_from(v - 0xff00 + 0x20).ok()?,
        _ => return None,
    };
    Some(Byte(b))
}

/// The text's bytes in the encoding the child reads.
fn encoded(text: &str, form: Form) -> Vec<u8> {
    match form.encoding {
        InputEncoding::Utf8 => text.as_bytes().to_vec(),
        InputEncoding::Latin1 => latin1(text, form.ambiguous),
    }
}

/// **`text` with [`FILTER`]'s characters replaced by a space**, as xterm's
/// `removeControls` does before the brackets and the quoting: ASCII bytes
/// only, so the text stays UTF-8.
fn filtered(text: &str, specials: Specials) -> String {
    text.chars()
        .map(|ch| {
            let named = u8::try_from(ch).is_ok_and(|b| FILTER.replaces(b));
            match named || (FILTER.stty() && specials.has(ch)) {
                true => ' ',
                false => ch,
            }
        })
        .collect()
}

/// The bytes a paste sends: [`FILTER`]'s characters replaced, then
/// bracketed as `?2004` specifies -- no ESC survives the filter, so the
/// text cannot carry the 7-bit end, `ESC [ 2 0 1 ~`; under `ESC % @` the
/// text is ISO 8859-1 with bytes 0x80-0x9F deleted, as xterm drops them
/// outside UTF-8 ([`latin1`]), so no `9B` forms the 8-bit end either --
/// or, unbracketed, with line endings as typed input carries them unless
/// `?2006` keeps them. Under
/// `?2005` each byte of the text, never of a bracket, follows the
/// literal-next byte, as xterm quotes the selection and writes the
/// brackets apart (`[[source: xterm-button]]`).
pub fn bytes(text: &str, form: Form) -> Vec<u8> {
    let text = encoded(&filtered(text, form.specials), form);
    match form.bracket {
        Bracket::On => [
            b"\x1b[200~".as_slice(),
            &readline::quoted(&text, form.quote),
            b"\x1b[201~",
        ]
        .concat(),
        Bracket::Off => readline::body(&text, form.newlines, form.quote),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

    #[test]
    fn a_paste_is_bracketed_when_asked_and_its_line_endings_are_typed_otherwise() {
        let forged = "a\x1b[201~b\r\nc\n";
        let form = |bracket| Form {
            bracket,
            newlines: Newlines::AsReturn,
            quote: None,
            specials: Specials::default(),
            encoding: InputEncoding::Utf8,
            ambiguous: AmbiguousWidth::Narrow,
        };
        assert_eq!(
            bytes(forged, form(Bracket::On)),
            b"\x1b[200~a [201~b\r\nc\n\x1b[201~".to_vec()
        );
        assert_eq!(bytes(forged, form(Bracket::Off)), b"a [201~b\rc\r".to_vec());
    }

    /// **xterm's default paste controls become spaces, in every paste**:
    /// BS, DEL, ENQ, EOT, ESC and NUL, and the tty's special characters;
    /// every other C0 control passes, and so does text past ASCII. No ESC
    /// survives, so no 7-bit end of a bracket can be forged, whole or in
    /// pieces that would meet once a piece was taken out.
    #[test]
    fn a_paste_replaces_xterms_default_controls_with_a_space() {
        let specials = Specials::of(
            &[0x03, 0x1a, 0xff, 0x00, b'x', 0x7f, 0x15],
            &[0, 1, 2, 3, 4, 5, 6, 40],
            Disabled(0xff),
        );
        let form = |bracket| Form {
            bracket,
            newlines: Newlines::Literal,
            quote: None,
            specials,
            encoding: InputEncoding::Utf8,
            ambiguous: AmbiguousWidth::Narrow,
        };
        let text = "\x08\x7f\x05\x04\x1b\x00|\x07\t\n\r\x18\x01\x0e|\x03\x1a\x15x|\u{5d1}\u{1f600}";
        let want = "      |\x07\t\n\r\x18\x01\x0e|   x|\u{5d1}\u{1f600}";
        assert_eq!(bytes(text, form(Bracket::Off)), want.as_bytes());
        let inner = bytes(text, form(Bracket::On));
        assert_eq!(
            inner,
            [b"\x1b[200~".as_slice(), want.as_bytes(), b"\x1b[201~"].concat()
        );
        for forged in ["\x1b[201~", "\x1b[20\x1b[201~1~"] {
            let out = bytes(forged, form(Bracket::On));
            let body = &out[6..out.len() - 6];
            assert!(!body.contains(&0x1b), "{forged:?}: {body:?}");
        }
        // Without the tty's characters, ETX passes.
        let bare = Form {
            specials: Specials::default(),
            ..form(Bracket::Off)
        };
        assert_eq!(bytes("\x03", bare), b"\x03");
    }

    /// **Under ISO 8859-1 a paste is ISO 8859-1, xterm's way**: a Latin-1
    /// letter one byte, no byte 0x80 to 0x9F anywhere in the body -- U+201B
    /// is `#`, not the `E2 80 9B` whose `9B` the child reads as CSI, and a
    /// bare U+009B is gone, so no end of the brackets is forged -- the DEC
    /// line-drawing and groff characters their ASCII, a fullwidth letter
    /// its letter and a space, a wide character `#` and a space, an xterm
    /// internal DEC Technical code `?`, and `is_UCS_SPECIAL` `#`. The
    /// control is the same text under UTF-8, sent as it is. Perturbation:
    /// keep the C1 bytes -- the forged CSI survives and the body check
    /// fails.
    #[test]
    fn a_latin1_paste_is_latin1_with_no_c1_byte() {
        let form = |encoding| Form {
            bracket: Bracket::On,
            newlines: Newlines::Literal,
            quote: None,
            specials: Specials::default(),
            encoding,
            ambiguous: AmbiguousWidth::Narrow,
        };
        let text = "\u{e9}\u{201b}\u{9b}201~x\u{2014}\u{2500}\u{2502}\u{250c}\u{4e00}\u{ff21}\u{eeee}\u{fff9}\u{2019}";
        let out = bytes(text, form(InputEncoding::Latin1));
        let body = &out[6..out.len() - 6];
        assert_eq!(body, b"\xe9#201~x--|+# A ?#'");
        assert!(
            !body.iter().any(|b| (0x80..=0x9f).contains(b) || *b == 0x1b),
            "{body:?}"
        );
        let utf8 = bytes(text, form(InputEncoding::Utf8));
        assert_eq!(&utf8[6..utf8.len() - 6], text.as_bytes());
    }

    /// **The line breaks and the quoting work on the encoded bytes**: a
    /// Latin-1 paste's `é` is one quoted byte, and its line break the CR
    /// typed input sends.
    #[test]
    fn a_latin1_paste_is_shaped_and_quoted_by_the_byte() {
        let form = Form {
            bracket: Bracket::Off,
            newlines: Newlines::AsReturn,
            quote: Some(readline::Quote::LNEXT),
            specials: Specials::default(),
            encoding: InputEncoding::Latin1,
            ambiguous: AmbiguousWidth::Narrow,
        };
        assert_eq!(bytes("\u{e9}\n", form), b"\x16\xe9\x16\r");
    }

    /// **`?2005` quotes the text and never a bracket; `?2006` keeps an
    /// unbracketed paste's line breaks.**
    #[test]
    fn readline_quoting_and_newlines_shape_a_paste() {
        let quoted = Form {
            bracket: Bracket::On,
            newlines: Newlines::AsReturn,
            quote: Some(readline::Quote::LNEXT),
            specials: Specials::default(),
            encoding: InputEncoding::Utf8,
            ambiguous: AmbiguousWidth::Narrow,
        };
        assert_eq!(
            bytes("ab", quoted),
            b"\x1b[200~\x16a\x16b\x1b[201~".to_vec()
        );
        let literal = Form {
            bracket: Bracket::Off,
            newlines: Newlines::Literal,
            quote: None,
            specials: Specials::default(),
            encoding: InputEncoding::Utf8,
            ambiguous: AmbiguousWidth::Narrow,
        };
        assert_eq!(bytes("a\r\nb\n", literal), b"a\r\nb\n".to_vec());
        let typed = Form {
            bracket: Bracket::Off,
            ..quoted
        };
        assert_eq!(bytes("a\n", typed), b"\x16a\x16\r".to_vec());
    }

    /// **The form is the child's modes**: power-on is unbracketed, typed
    /// line breaks, no quoting and UTF-8, and the tty is asked only for its
    /// special characters; `?2004`, `?2006` and `?2005` each change their
    /// part, the last asking the tty for its literal-next character, and
    /// `ESC % @` the encoding.
    #[test]
    fn the_form_is_the_childs_modes() {
        const WS: WinSize = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let never = || panic!("the tty asked without ?2005");
        let tty = || Specials::of(&[0x03], &[0], Disabled(0xff));
        assert_eq!(
            form(&t, never, tty),
            Form {
                bracket: Bracket::Off,
                newlines: Newlines::AsReturn,
                quote: None,
                specials: tty(),
                encoding: InputEncoding::Utf8,
                ambiguous: AmbiguousWidth::Narrow,
            }
        );
        let b = b"\x1b[?2004h\x1b[?2006h\x1b[?2005h\x1b%@";
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        assert_eq!(
            form(&t, || readline::Quote(0x16), tty),
            Form {
                bracket: Bracket::On,
                newlines: Newlines::Literal,
                quote: Some(readline::Quote(0x16)),
                specials: tty(),
                encoding: InputEncoding::Latin1,
                ambiguous: AmbiguousWidth::Narrow,
            }
        );
    }
}
