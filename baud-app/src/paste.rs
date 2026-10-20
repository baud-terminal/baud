//! **What a paste sends**, the same on both drivers: the text bracketed as
//! `?2004` asks, its line breaks as `?2006` says, and each byte quoted with
//! the tty's literal-next character under `?2005` (`readline.rs`). The
//! macOS app pastes the pasteboard with Cmd-V; the Linux driver pastes the
//! clipboard with Ctrl+Shift+V and xterm's SELECT with the middle button
//! and Shift+Insert. Pure, so its tests run on every platform.

use crate::readline::{self, Newlines};
use baud_core::{Resource, Term};

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
}

/// **The form the child's modes give a paste now**, with `literal_next`
/// asked only under `?2005`, the one mode that needs the tty.
pub fn form(term: &Term<'_>, literal_next: impl FnOnce() -> readline::Quote) -> Form {
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
    }
}

/// The bytes a paste sends: bracketed as `?2004` specifies, with a forged
/// end inside the text removed, because the child would take it as the
/// end; or, unbracketed, with line endings as typed input carries them
/// unless `?2006` keeps them. Under `?2005` each byte of the text, never of
/// a bracket, follows the literal-next byte, as xterm quotes the selection
/// and writes the brackets apart (`[[source: xterm-button]]`).
pub fn bytes(text: &str, form: Form) -> Vec<u8> {
    match form.bracket {
        Bracket::On => {
            let body = text.replace("\x1b[201~", "");
            [
                b"\x1b[200~".as_slice(),
                &readline::quoted(body.as_bytes(), form.quote),
                b"\x1b[201~",
            ]
            .concat()
        }
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
        };
        assert_eq!(
            bytes(forged, form(Bracket::On)),
            b"\x1b[200~ab\r\nc\n\x1b[201~".to_vec()
        );
        assert_eq!(
            bytes(forged, form(Bracket::Off)),
            b"a\x1b[201~b\rc\r".to_vec()
        );
    }

    /// **`?2005` quotes the text and never a bracket; `?2006` keeps an
    /// unbracketed paste's line breaks.**
    #[test]
    fn readline_quoting_and_newlines_shape_a_paste() {
        let quoted = Form {
            bracket: Bracket::On,
            newlines: Newlines::AsReturn,
            quote: Some(readline::Quote::LNEXT),
        };
        assert_eq!(
            bytes("ab", quoted),
            b"\x1b[200~\x16a\x16b\x1b[201~".to_vec()
        );
        let literal = Form {
            bracket: Bracket::Off,
            newlines: Newlines::Literal,
            quote: None,
        };
        assert_eq!(bytes("a\r\nb\n", literal), b"a\r\nb\n".to_vec());
        let typed = Form {
            bracket: Bracket::Off,
            ..quoted
        };
        assert_eq!(bytes("a\n", typed), b"\x16a\x16\r".to_vec());
    }

    /// **The form is the child's modes**: power-on is unbracketed, typed
    /// line breaks and no quoting, and the tty is not asked; `?2004`,
    /// `?2006` and `?2005` each change their part, the last asking the tty.
    #[test]
    fn the_form_is_the_childs_modes() {
        const WS: WinSize = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let never = || panic!("the tty asked without ?2005");
        assert_eq!(
            form(&t, never),
            Form {
                bracket: Bracket::Off,
                newlines: Newlines::AsReturn,
                quote: None
            }
        );
        let b = b"\x1b[?2004h\x1b[?2006h\x1b[?2005h";
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        assert_eq!(
            form(&t, || readline::Quote(0x16)),
            Form {
                bracket: Bracket::On,
                newlines: Newlines::Literal,
                quote: Some(readline::Quote(0x16))
            }
        );
    }
}
