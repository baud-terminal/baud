//! `SGR 11`-`19` draw from the nine alternative fonts a stack names, and a
//! font it names no face for draws as `SGR 10`, the primary, does
//! (`[[source: ecma48]]` § 8.3.117), on the CPU path; the cell shader's
//! half is the pixel oracle in `baud-present-macos`.
//!
//! The oracle for "drawn from the alternative font" is that font itself:
//! text in a slot whose face is JetBrains Mono Italic must paint exactly
//! what the same text paints with the Italic file as the only text face,
//! the icon face behind it either way. JetBrains Mono's styles share
//! their metrics, so the cell is the same size on both sides. The control
//! is the same text on a stack with no face for the slot, which paints
//! the primary.

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

struct Files {
    regular: Vec<u8>,
    italic: Vec<u8>,
    bold_italic: Vec<u8>,
    symbols: Vec<u8>,
}

impl Files {
    fn load() -> Files {
        Files {
            regular: app_font("jetbrains-mono/JetBrainsMono-Regular.ttf"),
            italic: app_font("jetbrains-mono/JetBrainsMono-Italic.ttf"),
            bold_italic: app_font("jetbrains-mono/JetBrainsMono-BoldItalic.ttf"),
            symbols: app_font("symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
        }
    }
}

fn parse<'a>(files: &[&'a Vec<u8>]) -> Vec<Face<'a>> {
    files.iter().map(|d| Face::parse(d).unwrap()).collect()
}

fn renderer<'f, 'a>(faces: &'f [Face<'a>], roles: &[FaceRole]) -> Renderer<'f, 'a> {
    Renderer::new(
        Stack::with_roles(faces, roles).unwrap(),
        PxSize::of(16.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

fn feed(t: &mut Term<'_>, b: &[u8]) {
    t.write_at(BytePos::ZERO, b);
    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
}

/// The pixels `bytes` paints through a stack of `faces` with `roles`.
fn painted(faces: &[Face<'_>], roles: &[FaceRole], bytes: &[u8]) -> Vec<u32> {
    let mut r = renderer(faces, roles);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut c = support::Canvas::for_renderer(&r);
    feed(&mut t, bytes);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    c.pixels
}

/// Text, a ligature JetBrains Mono forms through `calt`, and a Nerd Fonts
/// icon no text face carries, which comes from the chain behind the font.
const TEXT: &[u8] = b"ab->x \xef\x81\xbb";

fn line(sgr: &str) -> Vec<u8> {
    [format!("\x1b[0;{sgr};38;2;200;200;200m").as_bytes(), TEXT].concat()
}

const PLAIN: [FaceRole; 2] = [FaceRole::Fallback, FaceRole::Fallback];

fn alternative(n: u16) -> FontSlot {
    FontSlot::alternative(baud_domain::FontId::from_sgr(10 + n).unwrap())
}

/// Each of the nine alternatives, named alone, paints its own face for its
/// `SGR` and the primary for every other one; the primary's own text is
/// untouched by it.
#[test]
fn each_alternative_font_draws_from_its_own_face() {
    let f = Files::load();
    let primary = parse(&[&f.regular, &f.symbols]);
    let alone = parse(&[&f.italic, &f.symbols]);
    let faces = parse(&[&f.regular, &f.symbols, &f.italic]);
    let want = painted(&alone, &PLAIN, &line("10"));
    let plain = painted(&primary, &PLAIN, &line("10"));
    assert!(
        want != plain,
        "the premise: the italic face paints otherwise"
    );
    for n in 1..=9u16 {
        let roles = [
            FaceRole::Fallback,
            FaceRole::Fallback,
            FaceRole::Font(alternative(n), FaceStyle::Regular),
        ];
        let got = painted(&faces, &roles, &line(&format!("{}", 10 + n)));
        assert!(got == want, "SGR {} does not paint its font", 10 + n);
        for m in (0..=9u16).filter(|m| *m != n) {
            let other = painted(&faces, &roles, &line(&format!("{}", 10 + m)));
            assert!(
                other == plain,
                "SGR {} with only font {n} named does not paint the primary",
                10 + m
            );
        }
    }
}

/// A stack that names no alternative draws every one of 11-19 as 10.
#[test]
fn an_alternative_font_with_no_face_draws_the_primary() {
    let f = Files::load();
    let faces = parse(&[&f.regular, &f.symbols]);
    let plain = painted(&faces, &PLAIN, &line("10"));
    for n in 11..=19 {
        assert!(
            painted(&faces, &PLAIN, &line(&format!("{n}"))) == plain,
            "SGR {n} with no font named left the primary"
        );
    }
}

/// Bold text in an alternative font draws from that font's Bold face where
/// the stack names one: the font's family, as the primary's styles are.
#[test]
fn an_alternative_font_draws_its_own_styles() {
    let f = Files::load();
    let faces = parse(&[&f.regular, &f.symbols, &f.italic, &f.bold_italic]);
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Font(alternative(1), FaceStyle::Regular),
        FaceRole::Font(alternative(1), FaceStyle::Bold),
    ];
    let alone = parse(&[&f.bold_italic, &f.symbols]);
    let want = painted(&alone, &PLAIN, &line("10"));
    let got = painted(&faces, &roles, &line("1;11"));
    assert!(got == want, "bold in font 1 is not font 1's Bold face");
    // Italic, which font 1 names no face for, draws its regular face.
    let regular = painted(&parse(&[&f.italic, &f.symbols]), &PLAIN, &line("10"));
    assert!(
        painted(&faces, &roles, &line("3;11")) == regular,
        "italic in font 1 left font 1's regular face"
    );
}

/// A rendition switched to an alternative font in place repaints, on an
/// incremental frame, what a full frame paints.
#[test]
fn a_font_change_repaints_incrementally_to_what_a_full_frame_paints() {
    let f = Files::load();
    let faces = parse(&[&f.regular, &f.symbols, &f.italic]);
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Font(alternative(1), FaceStyle::Regular),
    ];
    let first = line("10");
    let then = [b"\x1b[H".as_slice(), &line("11")].concat();
    let mut r = renderer(&faces, &roles);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut c = support::Canvas::for_renderer(&r);
    feed(&mut t, &first);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    let before = c.pixels.clone();
    feed(&mut t, &then);
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    let full = painted(&faces, &roles, &[first.as_slice(), &then].concat());
    assert!(c.pixels == full);
    assert!(
        c.pixels != before,
        "the control: the font change reached the pixels"
    );
}

/// **A new stack resolves every rendition again**: text already in font
/// 1, drawn as the primary while no face was named for it, is drawn in
/// the face a restack names, as a renderer built on that stack draws it.
#[test]
fn a_restack_that_names_a_font_redraws_its_text() {
    let f = Files::load();
    let primary = parse(&[&f.regular, &f.symbols]);
    let faces = parse(&[&f.regular, &f.symbols, &f.italic]);
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Font(alternative(1), FaceStyle::Regular),
    ];
    let mut r = renderer(&primary, &PLAIN);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut c = support::Canvas::for_renderer(&r);
    feed(&mut t, &line("11"));
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    let before = c.pixels.clone();
    r.restack(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(16.0).unwrap(),
    )
    .unwrap();
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    let want = painted(&faces, &roles, &line("11"));
    assert!(
        c.pixels == want,
        "the restack left font 1's text in the primary"
    );
    assert!(
        c.pixels != before,
        "the control: the restack reached the pixels"
    );
}

/// **What the renderer remembers for text in a font is bounded**: its key
/// is a codepoint a program prints, so six thousand distinct CJK
/// characters in font 1, in every style, leave it at its cap, 4,096.
#[test]
fn the_fonts_memo_is_bounded() {
    let f = Files::load();
    let faces = parse(&[&f.regular, &f.symbols, &f.italic]);
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Font(alternative(1), FaceStyle::Regular),
    ];
    let mut r = renderer(&faces, &roles);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut c = support::Canvas::for_renderer(&r);
    let mut most = 0;
    for chunk in (0x4e00u32..0x4e00 + 6000).collect::<Vec<_>>().chunks(8) {
        for style in ["11", "1;11", "3;11", "1;3;11"] {
            let text: String = chunk.iter().filter_map(|&u| char::from_u32(u)).collect();
            feed(&mut t, format!("\x1b[H\x1b[0;{style}m{text}").as_bytes());
            support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
            most = most.max(r.font_faces_held().value());
        }
    }
    assert!(most <= 4096, "the memo grew to {most}");
    assert!(most > 4000, "the premise: the memo filled, {most}");
}

/// A stack refuses a font role that names the primary, two fonts at once,
/// or one font's style twice.
#[test]
fn a_font_role_names_one_font_once() {
    let f = Files::load();
    let faces = parse(&[&f.regular, &f.italic, &f.bold_italic]);
    let with = |a: FaceRole, b: FaceRole| Stack::with_roles(&faces, &[FaceRole::Fallback, a, b]);
    let one = FaceRole::Font(alternative(1), FaceStyle::Regular);
    assert!(with(one, FaceRole::Fallback).is_some());
    assert!(with(one, one).is_none(), "a style named twice");
    assert!(
        with(
            FaceRole::Font(FontSlot::PRIMARY, FaceStyle::Regular),
            FaceRole::Fallback
        )
        .is_none(),
        "the primary named as a font"
    );
    assert!(
        with(
            FaceRole::Font(alternative(1).with_fraktur(), FaceStyle::Regular),
            FaceRole::Fallback
        )
        .is_none(),
        "an alternative and Fraktur at once"
    );
    assert!(
        with(one, FaceRole::Font(alternative(2), FaceStyle::Regular)).is_some(),
        "two fonts, one face each"
    );
}
