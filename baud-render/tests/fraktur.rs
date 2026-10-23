//! `SGR 20`, "Fraktur (Gothic)" (`[[source: ecma48]]` § 8.3.117), draws
//! from the face a stack names for it, over an alternative font where
//! both are set, and draws as without it where the stack names none, on
//! the CPU path; the cell shader's half is the pixel oracle in
//! `baud-present-macos`.
//!
//! The Fraktur face here is a stand-in, JetBrains Mono Bold Italic, which
//! shares the primary's metrics, so "drawn from it" is the same pixels as
//! the text drawn with that file as the only text face.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, FaceRole, FaceStyle, FontSlot, PxSize, Renderer, Stack, Theme};

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 16,
};

fn app_font(path: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../baud-app/fonts/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn painted(files: &[&Vec<u8>], roles: &[FaceRole], bytes: &[u8]) -> Vec<u32> {
    let faces: Vec<Face<'_>> = files.iter().map(|d| Face::parse(d).unwrap()).collect();
    let mut r = Renderer::new(
        Stack::with_roles(&faces, roles).unwrap(),
        PxSize::of(16.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut c = support::Canvas::for_renderer(&r);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    c.pixels
}

fn line(sgr: &str, text: &str) -> Vec<u8> {
    format!("\x1b[0;{sgr};38;2;200;200;200m{text}").into_bytes()
}

fn font1() -> FontSlot {
    FontSlot::alternative(baud_domain::FontId::from_sgr(11).unwrap())
}

const TEXT: &str = "ab->x Gothic";
const PLAIN: [FaceRole; 1] = [FaceRole::Fallback];

/// 20 paints the Fraktur face; 23, "not italicized, not fraktur", and 0
/// paint the primary again; and with no Fraktur face named, 20 paints the
/// primary.
#[test]
fn sgr_20_draws_the_fraktur_face() {
    let regular = app_font("jetbrains-mono/JetBrainsMono-Regular.ttf");
    let fraktur = app_font("jetbrains-mono/JetBrainsMono-BoldItalic.ttf");
    let roles = [
        FaceRole::Fallback,
        FaceRole::Font(FontSlot::FRAKTUR, FaceStyle::Regular),
    ];
    let want = painted(&[&fraktur], &PLAIN, &line("10", TEXT));
    let plain = painted(&[&regular], &PLAIN, &line("10", TEXT));
    assert!(want != plain, "the premise: the stand-in paints otherwise");
    let got = painted(&[&regular, &fraktur], &roles, &line("20", TEXT));
    assert!(got == want, "SGR 20 does not paint the Fraktur face");
    for back in ["20;23", "20;0"] {
        assert!(
            painted(&[&regular, &fraktur], &roles, &line(back, TEXT)) == plain,
            "SGR {back} left the Fraktur face"
        );
    }
    assert!(
        painted(&[&regular], &PLAIN, &line("20", TEXT)) == plain,
        "SGR 20 with no Fraktur face named left the primary"
    );
}

/// **Fraktur is over the alternative font**: with both set and both named,
/// a character the Fraktur face carries is drawn from it, and one it lacks
/// from the alternative font before the chain (Jev, FRAKTUR_FIRST). The
/// alternative here is the Nerd Fonts icon face, which the chain lacks, so
/// the icon shows which face it came from.
#[test]
fn fraktur_is_over_the_alternative_font() {
    let regular = app_font("jetbrains-mono/JetBrainsMono-Regular.ttf");
    let fraktur = app_font("jetbrains-mono/JetBrainsMono-BoldItalic.ttf");
    let symbols = app_font("symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf");
    let roles = [
        FaceRole::Fallback,
        FaceRole::Font(FontSlot::FRAKTUR, FaceStyle::Regular),
        FaceRole::Font(font1(), FaceStyle::Regular),
    ];
    let text = "ab \u{f07b} x";
    let want = painted(
        &[&fraktur, &symbols],
        &[FaceRole::Fallback, FaceRole::Fallback],
        &line("10", text),
    );
    let got = painted(
        &[&regular, &fraktur, &symbols],
        &roles,
        &line("11;20", text),
    );
    assert!(got == want, "11;20 is not the Fraktur face over font 1");
    // The control: Fraktur alone leaves the icon to the chain, which lacks
    // it, so the icon cell paints otherwise.
    let alone = painted(&[&regular, &fraktur, &symbols], &roles, &line("20", text));
    assert!(alone != want, "the control: font 1 reached the icon");
    // And 11 alone is not 11;20: its letters are not the Fraktur face's.
    let font_only = painted(&[&regular, &fraktur, &symbols], &roles, &line("11", text));
    assert!(font_only != want, "the control: 11 alone is not 11;20");
}
