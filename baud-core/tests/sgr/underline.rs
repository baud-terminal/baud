//! Underline styles: single (`SGR 4`) and double (`SGR 21`).
//!
//! `SGR 21` is "doubly underlined" in ECMA-48 3rd and xterm. baud carried it as
//! a plain underline for a while, on the argument that a second attribute bit
//! "no renderer reads" would be set-and-never-used -- true until a renderer
//! drew it as two lines. It has its own bit now, and `DECRQSS` reports it as
//! 21.

use crate::support;
use baud_core::*;
use baud_domain::Attrs;

fn style_at(bytes: &[u8], col: usize) -> StyleWord {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let cell = support::screen(&t)[col];
    t.style_of(cell)
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
