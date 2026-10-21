//! **What a paste sends**, the same on both drivers: xterm's default control
//! characters replaced by a space (`FILTER`), the text bracketed as `?2004`
//! asks, its line breaks as `?2006` says, and each byte quoted with the
//! tty's literal-next character under `?2005` (`readline.rs`). The
//! macOS app pastes the pasteboard with Cmd-V; the Linux driver pastes the
//! clipboard with Ctrl+Shift+V and xterm's SELECT with the middle button
//! and Shift+Insert. Pure, so its tests run on every platform.

use crate::readline::{self, Newlines};
use baud_core::{PasteControls, Resource, Term};

/// **The control characters a paste replaces: xterm's default**, its
/// `disallowedPasteControls`, "BS,DEL,ENQ,EOT,ESC,NUL,STTY", each turned
/// into a space in every paste, bracketed or not, while every other C0
/// control passes (`[[source: xterm-man]]`; `removeControls`, `[[source:
/// xterm-button]]` 2582-2690). The conforming choice: Jev's lean,
/// XTERM_DEFAULT, 0.25/0.58, where keeping today's paste and foot's
/// stripping were departures neither order took to 0.9. Both drivers tell
/// the core, so `OSC 61 ; allowPasteControls` names them
/// (`host::set_window_abilities`).
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
/// text cannot carry a bracket's end the child would take as the end --
/// or, unbracketed, with line endings as typed input carries them unless
/// `?2006` keeps them. Under
/// `?2005` each byte of the text, never of a bracket, follows the
/// literal-next byte, as xterm quotes the selection and writes the
/// brackets apart (`[[source: xterm-button]]`).
pub fn bytes(text: &str, form: Form) -> Vec<u8> {
    let text = &filtered(text, form.specials);
    match form.bracket {
        Bracket::On => [
            b"\x1b[200~".as_slice(),
            &readline::quoted(text.as_bytes(), form.quote),
            b"\x1b[201~",
        ]
        .concat(),
        Bracket::Off => readline::body(text, form.newlines, form.quote),
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
    /// survives, so no end of a bracket can be forged, whole or in pieces
    /// that would meet once a piece was taken out.
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

    /// **`?2005` quotes the text and never a bracket; `?2006` keeps an
    /// unbracketed paste's line breaks.**
    #[test]
    fn readline_quoting_and_newlines_shape_a_paste() {
        let quoted = Form {
            bracket: Bracket::On,
            newlines: Newlines::AsReturn,
            quote: Some(readline::Quote::LNEXT),
            specials: Specials::default(),
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
        };
        assert_eq!(bytes("a\r\nb\n", literal), b"a\r\nb\n".to_vec());
        let typed = Form {
            bracket: Bracket::Off,
            ..quoted
        };
        assert_eq!(bytes("a\n", typed), b"\x16a\x16\r".to_vec());
    }

    /// **The form is the child's modes**: power-on is unbracketed, typed
    /// line breaks and no quoting, and the tty is asked only for its special
    /// characters; `?2004`, `?2006` and `?2005` each change their part, the
    /// last asking the tty for its literal-next character.
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
            }
        );
        let b = b"\x1b[?2004h\x1b[?2006h\x1b[?2005h";
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        assert_eq!(
            form(&t, || readline::Quote(0x16), tty),
            Form {
                bracket: Bracket::On,
                newlines: Newlines::Literal,
                quote: Some(readline::Quote(0x16)),
                specials: tty(),
            }
        );
    }
}
