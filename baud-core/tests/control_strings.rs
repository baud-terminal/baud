#![deny(dead_code)]
//! **Tier 0: a control string is consumed, whatever baud does with its body.**
//!
//! `docs/vt-coverage.md` puts image protocols out of scope and then says the
//! thing that is *not* optional: "the `APC` string must still be consumed --
//! that is Tier 0". A terminal that does not swallow the body renders the
//! payload as text, and a kitty graphics command carries base64 by the kilobyte.
//!
//! That row also said "and it is a bug today", which was true when written and
//! is not now. **The claim outlived the defect**, which is the same drift this
//! project keeps finding in its own documents -- a refusal whose reason expired,
//! a gate scoped to the wrong crate, a row no byte could reach. A document
//! asserting a live bug that no longer exists is as wrong as one asserting
//! coverage it does not have, and it is harder to notice because it reads as
//! caution.
//!
//! So the property is pinned here rather than described there: every control
//! string introducer swallows its body, and the text after the terminator still
//! lands on the screen.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

/// `[[source: williams]]` names the four string types; each is introduced
/// differently and all end at `ST`.
const STRINGS: [(&str, &[u8]); 5] = [
    // A real kitty graphics command: `APC G <control data> ; <base64> ST`.
    (
        "APC G -- kitty graphics",
        b"\x1b_Ga=T,f=100,s=10,v=10;iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB\x1b\\",
    ),
    ("DCS -- device control", b"\x1bPsomething\x1b\\"),
    ("SOS -- start of string", b"\x1bXanything at all\x1b\\"),
    ("PM -- privacy message", b"\x1b^private\x1b\\"),
    // OSC is the one string type baud acts on, and it must still consume.
    ("OSC -- window title", b"\x1b]0;a title\x1b\\"),
];

/// The body never reaches the grid, and what follows it does.
///
/// **Both halves matter.** A parser that ate everything to the end of the read
/// would also pass a "nothing was drawn" assertion, so the text after the
/// terminator is what separates consuming the string from consuming the stream.
#[test]
fn every_control_string_is_consumed_and_the_text_after_it_is_not() {
    for (name, body) in STRINGS {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);

        let mut bytes = body.to_vec();
        bytes.extend_from_slice(b"AFTER");
        t.write_at(BytePos::ZERO, &bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));

        let row = t.row(Row::from_index(0));
        let text: String = row
            .iter()
            .filter_map(|c| c.content.as_codepoint())
            .filter_map(|p| char::from_u32(p.value()))
            .collect();
        assert_eq!(
            text.trim_end(),
            "AFTER",
            "{name}: the body must be swallowed and the text after it must not"
        );
    }
}

/// **A string split across reads is still one string.**
///
/// A kitty graphics payload is measured in kilobytes, so it never arrives in one
/// `read(2)` -- the case the corpus cannot show, because no recording carries an
/// image. A parser that reset at a read boundary would render the tail of every
/// image as text, and it would do so only in production.
#[test]
fn a_control_string_survives_a_read_boundary() {
    let body = b"\x1b_Ga=T,f=100,s=10,v=10;iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB\x1b\\AFTER";
    for chunk in [1usize, 2, 3, 7, 16] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        for part in body.chunks(chunk) {
            t.write_at(BytePos::ZERO, part);
            t.feed(BytePos::ZERO, ReadLen::from_index(part.len()));
        }
        let row = t.row(Row::from_index(0));
        let text: String = row
            .iter()
            .filter_map(|c| c.content.as_codepoint())
            .filter_map(|p| char::from_u32(p.value()))
            .collect();
        assert_eq!(
            text.trim_end(),
            "AFTER",
            "an APC split into {chunk}-byte reads leaked its body"
        );
    }
}
