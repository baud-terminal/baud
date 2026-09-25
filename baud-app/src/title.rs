//! What a program's title becomes on the window.
//!
//! `OSC 0` and `OSC 2` set the title in the core (`Term::title`), and until
//! this module nothing read it: the window said "baud" whatever vim or ssh
//! asked for. The core keeps the bytes as they arrived, so turning them into
//! text a window can show is the embedder's, and it is pure so a test reaches
//! it -- the window it lands on is AppKit's or a compositor's.

use baud_domain::InputEncoding;

/// The title to show for the bytes a program set, or `fallback` when it set
/// none or an empty one.
///
/// **The bytes are read in the encoding the terminal is in.** Under UTF-8 an
/// ill-formed sequence becomes U+FFFD, as the screen shows it; under ISO 8859-1
/// each byte is its own codepoint, which is what that encoding means. A title
/// set in one encoding and read after a switch to the other is read in the
/// second; the core keeps no record of which was in force.
///
/// **Controls become spaces**, C0, DEL and C1 alike. The parser drops most of
/// C0 from a string's body already, but a title is shown in a title bar and a
/// window list, where a stray control is not rendered the way a terminal
/// would, so this does not lean on that.
pub fn window_title(bytes: &[u8], encoding: InputEncoding, fallback: &str) -> String {
    let text: String = match encoding {
        InputEncoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        InputEncoding::Latin1 => bytes.iter().map(|&b| char::from(b)).collect(),
    };
    let shown: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    match shown.trim() {
        "" => fallback.to_string(),
        t => t.to_string(),
    }
}

/// Whether a title should be sent to the window: only when the bytes a
/// program set, or the encoding they are read in, changed since the last one
/// sent. A read costs one comparison of at most `OSC_MAX` bytes, and a
/// program that sets the same title on every prompt costs the window nothing.
pub struct Shown {
    last: Option<(Vec<u8>, InputEncoding)>,
}

impl Shown {
    pub fn new() -> Shown {
        Shown { last: None }
    }

    /// The title to send now, or `None` when the window already has it.
    pub fn next(
        &mut self,
        bytes: &[u8],
        encoding: InputEncoding,
        fallback: &str,
    ) -> Option<String> {
        if self
            .last
            .as_ref()
            .is_some_and(|(b, e)| b.as_slice() == bytes && *e == encoding)
        {
            return None;
        }
        self.last = Some((bytes.to_vec(), encoding));
        Some(window_title(bytes, encoding, fallback))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_utf8_title_is_its_text() {
        let t = window_title("vim — main.rs".as_bytes(), InputEncoding::Utf8, "baud");
        assert_eq!(t, "vim — main.rs");
    }

    #[test]
    fn an_empty_or_blank_title_is_the_fallback() {
        assert_eq!(window_title(b"", InputEncoding::Utf8, "baud"), "baud");
        assert_eq!(window_title(b"   ", InputEncoding::Utf8, "baud"), "baud");
    }

    #[test]
    fn controls_become_spaces_and_the_ends_are_trimmed() {
        assert_eq!(
            window_title(b"\ta\x7fb\x1bc ", InputEncoding::Utf8, "baud"),
            "a b c"
        );
        // C1 as a UTF-8 codepoint, U+0085.
        assert_eq!(
            window_title("x\u{85}y".as_bytes(), InputEncoding::Utf8, "baud"),
            "x y"
        );
    }

    #[test]
    fn ill_formed_utf8_is_the_replacement_character() {
        assert_eq!(
            window_title(b"a\xffb", InputEncoding::Utf8, "baud"),
            "a\u{fffd}b"
        );
    }

    #[test]
    fn latin1_bytes_are_their_own_codepoints() {
        assert_eq!(
            window_title(b"caf\xe9", InputEncoding::Latin1, "baud"),
            "café"
        );
        // And the same bytes under UTF-8 are not.
        assert_eq!(
            window_title(b"caf\xe9", InputEncoding::Utf8, "baud"),
            "caf\u{fffd}"
        );
    }

    #[test]
    fn a_title_is_sent_only_when_it_changes() {
        let mut s = Shown::new();
        let u = InputEncoding::Utf8;
        assert_eq!(
            s.next(b"", u, "baud"),
            Some("baud".into()),
            "the first is sent"
        );
        assert_eq!(s.next(b"", u, "baud"), None);
        assert_eq!(s.next(b"a", u, "baud"), Some("a".into()));
        assert_eq!(s.next(b"a", u, "baud"), None);
        assert_eq!(
            s.next(b"a", InputEncoding::Latin1, "baud"),
            Some("a".into()),
            "a change of encoding is a change"
        );
        assert_eq!(
            s.next(b"", u, "baud"),
            Some("baud".into()),
            "and back to none"
        );
    }
}
