//! The `differs` count is what the cell shader draws wrong, and a count
//! that reads zero on a whole corpus needs a positive control: text that
//! must differ under a face whose lookups substitute in context, and text
//! that must not. JetBrains Mono's `->` shapes to two glyphs, one per
//! cell, each replaced by its context -- neither is the glyph the cell's
//! codepoint alone gives -- so both cells differ; `ab` shapes to what
//! each letter shapes to alone, so neither does.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};

const WS: WinSize = WinSize {
    ws_row: 1,
    ws_col: 16,
};

fn differs(text: &[u8]) -> u64 {
    let path = format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).expect("the committed face");
    let faces = [Face::parse(&bytes).expect("a face")];
    let mut r = Renderer::new(
        Stack::new(&faces).expect("a stack"),
        PxSize::of(14.0).expect("a size"),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .expect("metrics");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    t.write_at(BytePos::ZERO, text);
    t.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
    r.convert(&t, Rows::ZERO, Damage::Full)
        .exceptions
        .differs
        .value()
}

#[test]
fn a_contextual_ligature_differs_in_both_cells_and_plain_text_in_none() {
    assert_eq!(
        differs(b"ab cd"),
        0,
        "plain text is what each cell gives alone"
    );
    assert_eq!(
        differs(b"->"),
        2,
        "both halves of an arrow are replaced in context"
    );
    assert_eq!(differs(b"a -> b"), 2, "and the letters around it are not");
}
