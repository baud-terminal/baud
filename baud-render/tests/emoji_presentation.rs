//! **A variation selector picks the face; a bare character takes the
//! configured default** (owner, 2026-09-30).
//!
//! `U+FE0F` asks for colour and `U+FE0E` for text. A character with neither
//! is drawn from a text face under `EmojiDefault::Font`, the default --
//! JetBrains Mono's own bolt for `⚡`, Menlo's heart for `❤` -- and under
//! `System` a character whose default presentation is emoji is colour, as
//! macOS's own text draws it. When no face draws a character the way it
//! asks, the stack's ordinary choice draws it: a `U+FE0F` with no colour face
//! is the text glyph, not `.notdef`.

mod support;

use baud_core::{Owned, Term};
use baud_domain::Codepoint;
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, FaceIndex};
use baud_render::{
    Blend, Damage, EmojiDefault, FaceStyle, Presentation, PxSize, Renderer, Stack, Theme,
};
use support::font;

fn cp(c: char) -> Codepoint {
    Codepoint::from_char(c)
}

#[test]
fn the_selector_wins_and_a_bare_character_takes_the_default() {
    use EmojiDefault::{Font, System};
    use Presentation::{Colour, Text};
    for (mode, base, sel, want) in [
        (Font, '❤', Some(cp('\u{fe0f}')), Colour),
        (Font, '❤', Some(cp('\u{fe0e}')), Text),
        (Font, '❤', None, Text),
        (Font, '⚡', None, Text),
        (System, '⚡', None, Colour),
        (System, '❤', None, Text),
        (System, '⚡', Some(cp('\u{fe0e}')), Text),
        (System, '⚡', Some(cp('\u{301}')), Colour),
        (Font, '⚡', Some(cp('\u{301}')), Text),
    ] {
        assert_eq!(
            mode.presentation(cp(base), sel),
            want,
            "{mode:?} {base} {sel:x?}"
        );
    }
}

const EMOJI: &str = "/System/Library/Fonts/Apple Color Emoji.ttc";
const MENLO: &str = "/System/Library/Fonts/Menlo.ttc";

#[test]
fn the_stack_finds_a_face_that_draws_the_way_asked() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: the faces are macOS's");
        return;
    }
    let jbm = font("JetBrainsMono-Regular.ttf");
    let emoji = std::fs::read(EMOJI).unwrap();
    let menlo = std::fs::read(MENLO).unwrap();
    let faces = [
        Face::parse(&jbm).unwrap(),
        Face::parse_index(&emoji, FaceIndex::of(0)).unwrap(),
        Face::parse_index(&menlo, FaceIndex::of(0)).unwrap(),
    ];
    let s = Stack::new(&faces).unwrap();
    let r = FaceStyle::Regular;
    let at = |c: char, p| s.select_presented(cp(c), r, p).index();
    // `❤`: JetBrains Mono lacks it; the emoji face draws it in colour and
    // Menlo as text, behind it in the stack.
    assert_eq!(
        s.select(cp('❤')).index(),
        1,
        "control: the chain finds colour first"
    );
    assert_eq!(at('❤', Presentation::Text), 2);
    assert_eq!(at('❤', Presentation::Colour), 1);
    // `⚡`: JetBrains Mono's own bolt is text, first in the stack.
    assert_eq!(at('⚡', Presentation::Text), 0);
    assert_eq!(at('⚡', Presentation::Colour), 1);
    // `🌍` has no text glyph anywhere: asking for text gets the colour face
    // rather than nothing.
    assert_eq!(at('🌍', Presentation::Text), 1);
    // `a` has no colour glyph: asking for colour gets the text face.
    assert_eq!(at('a', Presentation::Colour), 0);
}

/// How many of the frame's pixels have a hue: a colour emoji has many, a
/// text glyph drawn in the theme's grey none.
fn coloured(text: &str, mode: EmojiDefault) -> usize {
    let jbm = font("JetBrainsMono-Regular.ttf");
    let emoji = std::fs::read(EMOJI).unwrap();
    let menlo = std::fs::read(MENLO).unwrap();
    let faces = [
        Face::parse(&jbm).unwrap(),
        Face::parse_index(&emoji, FaceIndex::of(0)).unwrap(),
        Face::parse_index(&menlo, FaceIndex::of(0)).unwrap(),
    ];
    let ws = WinSize {
        ws_row: 2,
        ws_col: 8,
    };
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(28.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(8),
        Rows::of_index(2),
    )
    .unwrap();
    r.set_emoji_default(mode);
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let bytes = format!("{text}\x1b[?25l");
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut c = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    c.pixels
        .iter()
        .filter(|&&p| {
            let ch = [(p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff];
            ch.iter().max().unwrap() - ch.iter().min().unwrap() > 24
        })
        .count()
}

#[test]
fn each_presentation_reaches_the_pixels_on_the_cpu_path() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: the faces are macOS's");
        return;
    }
    use EmojiDefault::{Font, System};
    assert_eq!(coloured("❤", Font), 0, "a bare heart is Menlo's");
    assert!(coloured("❤\u{fe0f}", Font) > 50, "FE0F is colour");
    assert_eq!(coloured("❤\u{fe0e}", System), 0, "FE0E is text");
    assert_eq!(coloured("⚡", Font), 0, "a bare bolt is JetBrains Mono's");
    assert!(
        coloured("⚡", System) > 50,
        "system draws an emoji default in colour"
    );
    assert_eq!(coloured("⚡\u{fe0e}", System), 0, "FE0E wins over system");
}

/// Changing the default after a frame starts the faces again: the second
/// frame is the one a renderer made in that mode draws.
#[test]
fn changing_the_default_redraws_in_the_new_mode() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: the faces are macOS's");
        return;
    }
    let jbm = font("JetBrainsMono-Regular.ttf");
    let emoji = std::fs::read(EMOJI).unwrap();
    let faces = [
        Face::parse(&jbm).unwrap(),
        Face::parse_index(&emoji, FaceIndex::of(0)).unwrap(),
    ];
    let ws = WinSize {
        ws_row: 2,
        ws_col: 8,
    };
    let make = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(28.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(8),
            Rows::of_index(2),
        )
        .unwrap()
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let bytes = b"\xe2\x9a\xa1\x1b[?25l";
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut switched = make();
    let mut c = support::Canvas::for_renderer(&switched);
    support::paint(&mut switched, &mut t, &mut c, Damage::Full);
    let font_frame = c.pixels.clone();
    switched.set_emoji_default(EmojiDefault::System);
    support::paint(&mut switched, &mut t, &mut c, Damage::Incremental);
    let mut fresh = make();
    fresh.set_emoji_default(EmojiDefault::System);
    let mut f = support::Canvas::for_renderer(&fresh);
    support::paint(&mut fresh, &mut t, &mut f, Damage::Full);
    assert_ne!(
        font_frame, f.pixels,
        "control: the two modes draw differently"
    );
    assert_eq!(c.pixels, f.pixels, "an incremental frame after the change");
}

/// **The cell path's glyph map follows a change of default.** Its entries
/// are keyed on the codepoint and carry the face, so a bare `⚡` mapped
/// under `Font` names JetBrains Mono's bolt; after the default changes the
/// same key must name the colour face, or the cell path draws the old
/// face for as long as the map lives. The CPU frames above cannot see it:
/// they shape through the face each frame and never read the map.
#[test]
fn the_cell_paths_map_follows_a_change_of_default() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: the faces are macOS's");
        return;
    }
    use baud_render::{CellSpan, GlyphLook, GlyphMap, MaySwap, Resolved, StyleStamp};
    let jbm = font("JetBrainsMono-Regular.ttf");
    let emoji = std::fs::read(EMOJI).unwrap();
    let faces = [
        Face::parse(&jbm).unwrap(),
        Face::parse_index(&emoji, FaceIndex::of(0)).unwrap(),
    ];
    let ws = WinSize {
        ws_row: 2,
        ws_col: 8,
    };
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(28.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(8),
        Rows::of_index(2),
    )
    .unwrap();
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let bytes = "⚡".as_bytes();
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut backing = GlyphMap::backing();
    let mut map = GlyphMap::over(&mut backing);
    let mut styles = vec![Resolved::default(); t.styles().len()];
    let mut stamp = StyleStamp::default();
    let mut candidates = Vec::new();
    let mut face_of_bolt = |r: &mut Renderer<'_, '_>, t: &Term<'_>| {
        r.sync_styles(t, &mut styles, &mut stamp);
        map.ensure(
            t.view(Rows::ZERO),
            &styles,
            MaySwap::NONE,
            r,
            &mut candidates,
        );
        map.probe(
            cp('⚡'),
            FaceStyle::Regular,
            GlyphLook::Plain,
            CellSpan::Two,
        )
        .expect("the bolt is mapped")
        .face()
        .0
    };
    assert_eq!(face_of_bolt(&mut r, &t), 0, "Font: JetBrains Mono's bolt");
    r.set_emoji_default(EmojiDefault::System);
    assert_eq!(face_of_bolt(&mut r, &t), 1, "System: the colour face");
}
