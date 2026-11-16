//! Underline styles: single (`SGR 4`) and double (`SGR 21`).
//!
//! `SGR 21` is "doubly underlined" in ECMA-48 3rd and xterm. baud carried it as
//! a plain underline for a while, on the argument that a second attribute bit
//! "no renderer reads" would be set-and-never-used -- true until a renderer
//! drew it as two lines. It has its own bit now, and `DECRQSS` reports it as
//! 21. Its own bit, but not its own aspect: `21` is the family's double
//! style, the state `4:2` names, and replaces the family as every style does.

use crate::support;
use baud_core::*;
use baud_domain::Attrs;

fn style_at(bytes: &[u8], col: usize) -> StyleWord {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let cell = t.viewport()[col];
    t.styles().one().word_of(cell)
}

#[test]
fn double_underline_is_a_distinct_attribute() {
    let single = style_at(b"\x1b[4mX", 0).attrs();
    let double = style_at(b"\x1b[21mX", 0).attrs();
    assert!(single.has(Attrs::UNDERLINE), "SGR 4 sets UNDERLINE");
    assert!(
        !single.has(Attrs::DOUBLE_UNDERLINE),
        "SGR 4 is not double underline"
    );
    assert!(
        double.has(Attrs::DOUBLE_UNDERLINE),
        "SGR 21 sets DOUBLE_UNDERLINE"
    );
    assert!(
        !double.has(Attrs::UNDERLINE),
        "SGR 21 is double, not single"
    );
}

#[test]
fn underline_off_clears_both() {
    let after = style_at(b"\x1b[21m\x1b[24mX", 0).attrs();
    assert!(
        !after.has(Attrs::UNDERLINE) && !after.has(Attrs::DOUBLE_UNDERLINE),
        "SGR 24 clears single and double underline"
    );
}

// ── the colon styles, `[[source: kitty-underlines]]` ────────────────────────

/// Each `4:n` sets exactly its own style bit and no other.
#[test]
fn each_colon_style_sets_exactly_its_own_bit() {
    let cases: [(&[u8], Attrs); 5] = [
        (b"\x1b[4:1mX", Attrs::UNDERLINE),
        (b"\x1b[4:2mX", Attrs::DOUBLE_UNDERLINE),
        (b"\x1b[4:3mX", Attrs::CURLY_UNDERLINE),
        (b"\x1b[4:4mX", Attrs::DOTTED_UNDERLINE),
        (b"\x1b[4:5mX", Attrs::DASHED_UNDERLINE),
    ];
    for (bytes, want) in cases {
        let got = style_at(bytes, 0).attrs();
        assert!(
            got.has(want),
            "{:?} sets {want:?}",
            String::from_utf8_lossy(bytes)
        );
        let others = Attrs::UNDERLINES.without(want);
        assert!(
            !got.has(Attrs::BOLD) && !got.has(Attrs::ITALIC) && got.without(others) == got,
            "{:?} sets nothing else: got {got:?}",
            String::from_utf8_lossy(bytes)
        );
    }
}

/// **`4:3` is not `4;3`.** A parser that flattens the colon reads it as
/// underline *plus italic* -- which is exactly what sfte does, measured --
/// and an editor's diagnostic squiggle comes out slanted. The two spellings
/// must differ, and the colon one must not touch italic.
#[test]
fn the_colon_form_is_not_the_semicolon_form() {
    let colon = style_at(b"\x1b[4:3mX", 0).attrs();
    let semi = style_at(b"\x1b[4;3mX", 0).attrs();
    assert!(colon.has(Attrs::CURLY_UNDERLINE) && !colon.has(Attrs::ITALIC));
    assert!(semi.has(Attrs::UNDERLINE) && semi.has(Attrs::ITALIC));
    assert_ne!(colon, semi);
}

/// **Exactly one style is in force.** A `4:n` replaces the family rather
/// than adding to it, so a curly after a double is curly alone; and `4:0`,
/// like `24`, is every style off.
#[test]
fn a_style_replaces_the_family_and_4_0_clears_it() {
    let replaced = style_at(b"\x1b[4:2m\x1b[4:3mX", 0).attrs();
    assert!(replaced.has(Attrs::CURLY_UNDERLINE));
    assert!(
        !replaced.has(Attrs::DOUBLE_UNDERLINE),
        "the double was replaced"
    );
    let cleared = style_at(b"\x1b[4:3m\x1b[4:0mX", 0).attrs();
    assert!(!cleared.has(Attrs::UNDERLINES), "4:0 clears every style");
    let off = style_at(b"\x1b[4:5m\x1b[24mX", 0).attrs();
    assert!(
        !off.has(Attrs::UNDERLINES),
        "24 clears every style, dashed included"
    );
}

/// The underline family of the rendition `bytes` leaves under column 0: the
/// style bits alone, every other attribute stripped, so an assertion on it
/// says "this style and no other" in one comparison.
fn family(bytes: &[u8]) -> Attrs {
    let a = style_at(bytes, 0).attrs();
    a.without(a.without(Attrs::UNDERLINES))
}

/// **`21` is the double style, so it replaces the family as `4:n` does.**
///
/// `[[source: ecma48]]` § 8.3.117 gives "21 doubly underlined" and "24 not
/// underlined (neither singly nor doubly)" -- two values of one aspect with
/// one cancel -- and `[[source: kitty-underlines]]` names the same rendition
/// `4:2`. `21` was a plain `Set`, so `4:3` then `21` held curly *and* double,
/// and `4` then `21` single and double, while `21` then `4` already ended
/// single alone because a bare `4` replaces the family. The two orders now
/// agree: the last style is the one in force.
#[test]
fn twenty_one_replaces_the_family_in_either_order() {
    let cases: [(&[u8], Attrs, &str); 6] = [
        (
            b"\x1b[4:3m\x1b[21mX",
            Attrs::DOUBLE_UNDERLINE,
            "4:3 then 21",
        ),
        (b"\x1b[4m\x1b[21mX", Attrs::DOUBLE_UNDERLINE, "4 then 21"),
        (
            b"\x1b[4:5;21mX",
            Attrs::DOUBLE_UNDERLINE,
            "4:5;21 in one SGR",
        ),
        // Controls: these orders were already right, because the style that
        // arrives second replaced the family before this change too.
        (b"\x1b[21m\x1b[4:3mX", Attrs::CURLY_UNDERLINE, "21 then 4:3"),
        (b"\x1b[21m\x1b[4mX", Attrs::UNDERLINE, "21 then 4"),
        (b"\x1b[21m\x1b[24mX", Attrs::NONE, "21 then 24"),
    ];
    // Every case is checked before anything is asserted, so a red run names
    // each order that is wrong rather than stopping at the first.
    let wrong: Vec<String> = cases
        .iter()
        .filter(|(bytes, want, _)| family(bytes) != *want)
        .map(|(bytes, want, name)| format!("{name}: {:?}, want {want:?}", family(bytes)))
        .collect();
    assert!(
        wrong.is_empty(),
        "not exactly one style in force: {wrong:#?}"
    );
}

/// **`21` and `4:2` are one state, not two spellings that happen to agree
/// from a blank start.** From the default rendition a `Set` and a family
/// replace land on the same word, so only a start that already holds
/// another style tells them apart: after `4:3`, the two must leave the same
/// rendition, bit for bit, colours and all.
#[test]
fn twenty_one_and_4_2_are_one_rendition() {
    let starts = [&b"\x1b[4:3m"[..], b"\x1b[4m", b"\x1b[4:4m", b"\x1b[24m"];
    let wrong: Vec<String> = starts
        .iter()
        .filter_map(|start| {
            let ecma = style_at(&[*start, &b"\x1b[21mX"[..]].concat(), 0);
            let kitty = style_at(&[*start, &b"\x1b[4:2mX"[..]].concat(), 0);
            (ecma != kitty).then(|| {
                format!(
                    "after {:?}: 21 {:?}, 4:2 {:?}",
                    String::from_utf8_lossy(start),
                    ecma.attrs(),
                    kitty.attrs()
                )
            })
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "21 and 4:2 left different renditions: {wrong:#?}"
    );
}

/// A plain `4` after a colon style is the straight underline it always was,
/// and a bare `4` never reads the *next* parameter as a style.
#[test]
fn a_bare_4_stays_the_straight_underline() {
    let plain = style_at(b"\x1b[4:3m\x1b[4mX", 0).attrs();
    assert!(plain.has(Attrs::UNDERLINE) && !plain.has(Attrs::CURLY_UNDERLINE));
    // `4;1`: straight underline then bold, two parameters, not "style 1".
    let two = style_at(b"\x1b[4;1mX", 0).attrs();
    assert!(two.has(Attrs::UNDERLINE) && two.has(Attrs::BOLD));
}

/// An unknown style is refused, not read as some other style. `4:9` sets no
/// underline at all and the parameters after it are still applied.
#[test]
fn an_unknown_style_is_refused_and_the_rest_still_applies() {
    let got = style_at(b"\x1b[4:9;1mX", 0).attrs();
    assert!(!got.has(Attrs::UNDERLINES), "4:9 is not a style");
    assert!(got.has(Attrs::BOLD), "the parameter after it still applies");
}

/// The underline colour is the same colour whatever the style.
#[test]
fn the_underline_colour_applies_to_every_style() {
    let curly = style_at(b"\x1b[58;2;1;2;3;4:3mX", 0);
    let dotted = style_at(b"\x1b[58;2;1;2;3;4:4mX", 0);
    assert_eq!(curly.underline(), dotted.underline());
    assert!(!curly.underline().is_default());
}
