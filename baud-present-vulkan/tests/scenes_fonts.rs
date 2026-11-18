//! **The cell path's font scenes, drawn alike on every path**: screens of
//! styled, alternative, precomposed, joined and Devanagari text fed as
//! bytes, drawn by the scene harness (`scenes::Arms`) on the cell path, the
//! instance path and the CPU path, each frame held to the cell path's bars
//! -- the cell frame the instance frame exactly, and both the CPU path's
//! premultiplied frame, exact where no record blends and within a step
//! where one does -- on lavapipe on both memory arms, and on RADV staged
//! and on the arm its heaps allow. Each scene carries its own controls, so
//! that frames which agree are not frames that all dropped what the scene
//! draws. The cell frame is built from the window's copy with its bidi
//! layout, so every scene reads the cells in their drawn order.
//!
//! - [`alternative_fonts_draw_alike_on_every_arm`], `SGR 11`-`19` and
//!   `SGR 20` from the faces named for them.
//! - [`every_precomposed_letter_draws_alike_on_every_arm`], every
//!   precomposed Latin, Greek and Cyrillic letter, regular, bold and
//!   italic, alone and as its canonical decomposition typed, on stock
//!   JetBrains Mono and on the default chain's embedded faces.
//! - [`italic_hebrew_draws_alike_on_every_arm`], the pointed Genesis 1:1-5
//!   in the built faces' regular, italic and bold italic.
//! - [`an_over_wide_letter_draws_alike_on_every_arm`], a sad wider than its
//!   cell, full size and fitted, with BDSM set and reset.
//! - [`presentation_variants_draw_alike_on_every_arm`], SAPV's variants
//!   against the characters they stand for.
//! - [`a_syllable_draws_alike_on_every_arm`], the Hindi line under each
//!   count of spacing marks, wrapped, and bold on the app's stack.
//! - [`a_devanagari_word_and_its_lights_draw_alike_on_every_arm`], the
//!   block cursor on each cell of two words and each syllable selected.
//! - [`bold_and_italic_hindi_draw_alike_on_every_arm`], the Hindi line in
//!   the built faces' four styles.
//! - [`a_word_whose_face_changes_draws_alike_on_every_arm`], a rendition
//!   changing between and inside a word's syllables, and composed.
//!
//! Not drawn here, each for its reason:
//!
//! - `an_italic_letter_reads_its_word_on_every_arm`: it draws Noto Sans
//!   Syriac's `rlig` and SF Arabic's `rclt`, faces the repository does not
//!   carry;
//! - `a_status_lines_letter_reads_its_word_on_every_arm`: it draws Noto
//!   Sans Syriac's `rlig`, a face the repository does not carry;
//! - in [`every_precomposed_letter_draws_alike_on_every_arm`], the system
//!   faces of a host's default chain: the chain drawn is its embedded
//!   faces, the built Regular, the icon face, STIX Two Math and the built
//!   styles;
//! - in [`a_syllable_draws_alike_on_every_arm`], Tamil கொ drawn past its
//!   two cells: it needs a Tamil face, which the repository does not carry;
//!   the Devanagari is the built Regular's;
//! - in [`a_devanagari_word_and_its_lights_draw_alike_on_every_arm`], the
//!   Devanagari is the built Regular's; Noto Sans Devanagari's variable
//!   file is drawn, as Noto Sans Arabic is in the over-wide scene, only
//!   where the README's font cache holds the pinned file.
//!
//! The faces are JetBrains Mono and the Noto colour emoji subset from
//! `baud-font/tests/fonts`, `joining-cases.ttf` from `baud-font/tests/var`,
//! and the built faces, JetBrains Mono's styled faces, the Symbols Nerd
//! Font and STIX Two Math from `baud-app/fonts`, each under the licence
//! beside it.

#![cfg(target_os = "linux")]

mod common;
mod scenes;
#[path = "../../baud-font/tests/sha256/mod.rs"]
mod sha256;

use baud_core::{Owned, Term};
use baud_domain::{Chan, Codepoint, Col, Row, WinSize};
use baud_font::Face;
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::lend::Arm;
use baud_render::{
    Caret, Composition, CursorDrawn, FaceRole, FaceStyle, FontSlot, Highlight, Instance, Kind,
    Region, Renderer, Stack, Tally, Theme,
};
use common::{lavapipe, radv};
use scenes::{Arms, Frames, differing, feed, jetbrains_mono, repo_file};

/// Wide enough that a path which stops short of the right-hand side shows.
const WS: WinSize = WinSize {
    ws_col: 120,
    ws_row: 10,
};

/// The README's Hindi line.
const HINDI: &str = "टर्मिनल हर अक्षर को ग्रिड में रखता है।";

/// A renderer over `faces` with `roles` in `theme`, sized to [`WS`].
fn fresh<'f, 'a>(faces: &'f [Face<'a>], roles: &[FaceRole], theme: Theme) -> Renderer<'f, 'a> {
    scenes::renderer(Stack::with_roles(faces, roles).expect("a stack"), theme, WS)
}

/// **`f` handed the arms over `renderer` on `arm`, and a terminal of its
/// own** sized to [`WS`].
fn with_term<'f, 'a, R>(
    gpu: &Gpu,
    arm: Arm,
    renderer: Renderer<'f, 'a>,
    f: impl FnOnce(&mut Arms<'_, 'f, 'a>, &mut Term<'_>) -> R,
) -> R {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, renderer, &t);
    f(&mut arms, &mut t)
}

/// A pixel count or offset of the renderer's, as an index.
fn ix(v: u32) -> usize {
    usize::try_from(v).expect("an index")
}

/// The frame's width and the cell's width and height, in pixels.
fn geometry(r: &Renderer<'_, '_>) -> (usize, usize, usize) {
    let m = r.metrics();
    (
        ix(r.whole_size().0.value()),
        ix(m.cell_w.value()),
        ix(m.cell_h.value()),
    )
}

/// The pixels of the cell drawn at logical column `col` of row `row` of
/// `frame` (both from zero), wherever the layout draws it.
fn crop_cell(r: &Renderer<'_, '_>, frame: &[u32], (row, col): (usize, usize)) -> Vec<u32> {
    let (w, cw, ch) = geometry(r);
    let v = r
        .bidi()
        .visual_col(Row::from_index(row), Col::at_index(col))
        .index();
    (row * ch..(row + 1) * ch)
        .flat_map(|y| frame[y * w + v * cw..y * w + (v + 1) * cw].to_vec())
        .collect()
}

/// **The mask records of the last cell frame that the instance path's
/// frame does not draw**: each glyph a record draws is one of the instance
/// list's, the same atlas slot at the same place, so the same face's
/// glyph, which tells two faces that draw alike apart.
fn stray_glyphs(arms: &mut Arms<'_, '_, '_>, t: &Term<'_>) -> usize {
    let glyph = |i: &Instance| (i.kind, i.x, i.y, i.w, i.h, i.u, i.v);
    let drawn: Vec<_> = arms.renderer.instances().map(glyph).collect();
    arms.with_frame(t, |_, input| {
        input
            .records
            .iter()
            .filter(|i| i.kind == Kind::Mask && !drawn.contains(&glyph(i)))
            .count()
    })
}

/// **`text` drawn as the first frame of a renderer of its own** over
/// `faces` with `roles`, `setup` done to the terminal first: the cell
/// frame drawn before any conversion resolved the renderer's renditions,
/// held to the bars, and every glyph its records draw one of the instance
/// path's. The instance frame, the cells the records drew, and the cell
/// `crop` of the frame.
fn first_frame(
    gpu: &Gpu,
    arm: Arm,
    (faces, roles): (&[Face<'_>], &[FaceRole]),
    setup: impl FnOnce(&mut Term<'_>),
    text: &str,
    crop: (usize, usize),
    what: &str,
) -> (Vec<u32>, Tally, Vec<u32>) {
    with_term(gpu, arm, fresh(faces, roles, Theme::dark()), |arms, t| {
        setup(t);
        feed(t, text.as_bytes());
        let f = arms.check(t, &format!("{arm:?}: {what}"));
        assert_eq!(
            stray_glyphs(arms, t),
            0,
            "{arm:?}: {what}: glyph records the instance path does not draw"
        );
        let cell = crop_cell(&arms.renderer, &f.instance, crop);
        (f.instance, arms.exceptions().stats().cells, cell)
    })
}

/// **`SGR 11`-`19` and `SGR 20` from their fonts, on `arm`**: font 1 is
/// JetBrains Mono Italic and font 9 its Bold Italic, each named alone,
/// with font 1's Bold beside it, and the Fraktur face's stand-in is its
/// Bold; each named font drawn over text, a ligature, a wide character, a
/// marked cluster and an icon no text face carries, bold in font 1, font
/// 2, which no face is named for, and Fraktur alone and over font 1. The
/// premises: the same text in font 10 paints otherwise, the Fraktur lines
/// alone paint otherwise, and font 2's lines paint what font 10's do.
fn alternative_fonts(gpu: &Gpu, arm: Arm) {
    let regular = repo_file("baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf");
    let symbols = repo_file("baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf");
    let italic = repo_file("baud-app/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf");
    let bold = repo_file("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf");
    let bold_italic = repo_file("baud-app/fonts/jetbrains-mono/JetBrainsMono-BoldItalic.ttf");
    let faces = [
        Face::parse(&regular).expect("JetBrains Mono"),
        Face::parse(&symbols).expect("the icon face"),
        Face::parse(&italic).expect("the italic face"),
        Face::parse(&bold).expect("the bold face"),
        Face::parse(&bold_italic).expect("the bold-italic face"),
        Face::parse(&bold).expect("the bold face"),
    ];
    let font =
        |n: u16| FontSlot::alternative(baud_domain::FontId::from_sgr(10 + n).expect("an SGR font"));
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Font(font(1), FaceStyle::Regular),
        FaceRole::Font(font(1), FaceStyle::Bold),
        FaceRole::Font(font(9), FaceStyle::Regular),
        FaceRole::Font(FontSlot::FRAKTUR, FaceStyle::Regular),
    ];
    let body = "ab->x \u{4e2d}\u{6587} e\u{301}q \u{f07b} WiW the quick brown fox jumps 0123";
    let lines = |sgr: [&str; 6]| {
        let mut out = String::from("\x1bc\x1b[?25l");
        for (i, s) in sgr.iter().enumerate() {
            if i > 0 {
                out.push_str("\r\n");
            }
            out.push_str(&format!("\x1b[{s}m{body}\x1b[0m"));
        }
        out
    };
    with_term(gpu, arm, fresh(&faces, &roles, Theme::dark()), |arms, t| {
        feed(t, lines(["10", "10", "1;10", "10", "10", "10"]).as_bytes());
        let plain = arms.check(t, &format!("{arm:?}: font 10"));
        feed(t, lines(["10", "10", "1;10", "10", "20", "20"]).as_bytes());
        let fraktur = arms.check(t, &format!("{arm:?}: Fraktur"));
        assert!(
            differing(&plain.instance, &fraktur.instance) > 0,
            "the Fraktur face never reached the instance path's pixels"
        );
        feed(
            t,
            lines(["11", "19", "1;11", "12", "20", "11;20"]).as_bytes(),
        );
        let named = arms.check(t, &format!("{arm:?}: the alternative fonts"));
        assert!(
            differing(&plain.instance, &named.instance) > 0,
            "the alternative fonts never reached the instance path's pixels"
        );
        feed(t, lines(["12", "12", "1;12", "12", "12", "12"]).as_bytes());
        let unnamed = arms.check(t, &format!("{arm:?}: font 2"));
        assert_eq!(
            differing(&plain.instance, &unnamed.instance),
            0,
            "font 2 left the primary"
        );
    });
}

#[test]
fn alternative_fonts_draw_alike_on_every_arm() {
    let gpu = lavapipe();
    alternative_fonts(&gpu, Arm::InPlace);
    alternative_fonts(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn alternative_fonts_draw_alike_on_every_arm_on_radv() {
    let gpu = radv();
    alternative_fonts(&gpu, Arm::Staging);
    alternative_fonts(&gpu, gpu.arm(1));
}

/// The precomposed letters of the Latin, Greek and Cyrillic blocks: every
/// alphabetic one-column code point from U+00C0 to U+024F, U+0386 to U+04FF
/// and U+1E00 to U+1FFF, whether or not a face maps it.
fn precomposed_letters() -> Vec<char> {
    [
        0x00c0..=0x024f,
        0x0386..=0x03ff,
        0x0400..=0x04ff,
        0x1e00..=0x1eff,
        0x1f00..=0x1fff,
    ]
    .into_iter()
    .flatten()
    .filter_map(char::from_u32)
    .filter(|c| c.is_alphabetic())
    .filter(|&c| {
        baud_domain::Classified::of(Codepoint::from_char(c))
            .width(baud_domain::WidthPolicy::DEFAULT)
            == baud_domain::Width::NARROW
    })
    .collect()
}

/// `c`'s full canonical decomposition in canonical order
/// (`[[source: unicode18-ch3]]` D68, D109), from `baud_font`'s tables:
/// `None` for a letter that has none.
fn canonical_decomposition(c: char) -> Option<String> {
    fn push(c: Codepoint, out: &mut Vec<Codepoint>) {
        match baud_font::decompose(c) {
            Some((a, b)) => {
                push(a, out);
                if let Some(b) = b {
                    push(b, out);
                }
            }
            None => out.push(c),
        }
    }
    let cp = Codepoint::from_char(c);
    baud_font::decompose(cp)?;
    let mut out = Vec::new();
    push(cp, &mut out);
    // Marks after a starter in canonical order: a stable sort by class.
    let mut i = 1;
    while i < out.len() {
        let class = baud_font::combining_class(out[i]).0;
        let mut j = i;
        while class != 0 && j > 0 && baud_font::combining_class(out[j - 1]).0 > class {
            out.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
    Some(
        out.into_iter()
            .filter_map(|c| char::from_u32(c.value()))
            .collect(),
    )
}

/// **The size of the chunks `n` items fall into, at most `most` a chunk
/// and as even as that allows**, so no screen is left a handful of letters.
fn even(n: usize, most: usize) -> usize {
    let chunks = n.div_ceil(most.max(1)).max(1);
    n.div_ceil(chunks).max(1)
}

/// **Where the `i`th letter of a screen is written**: one a cell, two
/// empty columns and two empty rows between them, so each letter has the
/// cells around it to itself -- a stacked accent the row above, a hook the
/// row below. Its column and row, from zero.
fn letter_at(i: usize) -> (usize, usize) {
    let per_row = usize::from(WS.ws_col) / 3;
    (1 + 3 * (i % per_row), 1 + 3 * (i / per_row))
}

/// How many letters a screen holds.
fn per_screen() -> usize {
    (usize::from(WS.ws_col) / 3) * (usize::from(WS.ws_row) / 3)
}

/// The pixels of a letter's cell at `(col, row)` and the eight around it,
/// in a frame `w` wide of cells `cw` by `ch`.
fn near(
    (w, cw, ch): (usize, usize, usize),
    (col, row): (usize, usize),
) -> impl Iterator<Item = usize> {
    ((row - 1) * ch..(row + 2) * ch)
        .flat_map(move |py| ((col - 1) * cw..(col + 2) * cw).map(move |px| py * w + px))
}

/// **The glyph map warmed by the cell path alone**, as the app's cell path
/// draws, which converts nothing: a line of Latin in the default
/// rendition, so the scene drawn next meets the renderer's own rendition
/// table unresolved, as the cell path's first frame meets it.
fn warm(arms: &mut Arms<'_, '_, '_>, t: &mut Term<'_>) {
    feed(
        t,
        b"\x1bc\x1b[?25lThe quick brown fox jumps over the lazy dog 0123456789",
    );
    let _ = arms.cell_frame(t);
}

/// **Every letter of `letters`, in rendition `sgr`, drawn on the three
/// paths**, each screen the first frame of a renderer of its own after
/// [`warm`] and held to the bars; no letter's ink, on any path, leaves its
/// nine cells, and the instance path drew most of the letters.
fn letters_gate<'f, 'a>(
    gpu: &Gpu,
    arm: Arm,
    fresh: &dyn Fn() -> Renderer<'f, 'a>,
    letters: &[char],
    (sgr, what): (&str, &str),
) {
    let mut empty: Option<Frames> = None;
    let mut inked = 0;
    for (k, screen) in letters
        .chunks(even(letters.len(), per_screen()))
        .enumerate()
    {
        with_term(gpu, arm, fresh(), |arms, t| {
            warm(arms, t);
            let mut text = format!("\x1bc\x1b[?25l{sgr}");
            for (i, &c) in screen.iter().enumerate() {
                let (col, row) = letter_at(i);
                text.push_str(&format!("\x1b[{};{}H{c}", row + 1, col + 1));
            }
            feed(t, text.as_bytes());
            let f = arms.check(t, &format!("{arm:?}: {what}, letters screen {k}"));
            let geo = geometry(&arms.renderer);
            if empty.is_none() {
                feed(t, b"\x1bc\x1b[?25l");
                // Nothing blends on an empty screen, so the three frames
                // are one, pixel for pixel.
                let what = format!("{arm:?}: {what}, an empty screen");
                let e = arms.draw(t);
                e.cell_is_instance(&what);
                assert!(!e.blended.contains(&true), "{what}: a record blends");
                assert_eq!(
                    differing(&e.instance, &e.cpu),
                    0,
                    "{what}: the instance frame against the CPU path's"
                );
                empty = Some(e);
            }
            let e = empty.as_ref().expect("the empty screen");
            let mut mine = vec![false; f.instance.len()];
            for i in 0..screen.len() {
                for p in near(geo, letter_at(i)) {
                    mine[p] = true;
                }
            }
            for (path, px, none) in [
                ("instance", &f.instance, &e.instance),
                ("cell", &f.cell, &e.cell),
                ("CPU", &f.cpu, &e.cpu),
            ] {
                let stray = (0..px.len())
                    .filter(|&p| !mine[p] && px[p] != none[p])
                    .count();
                assert_eq!(
                    stray, 0,
                    "{arm:?}: {what}, screen {k}: {stray} pixels of the {path} path lie \
                     outside every letter's cells"
                );
            }
            inked += (0..screen.len())
                .filter(|&i| near(geo, letter_at(i)).any(|p| f.instance[p] != e.instance[p]))
                .count();
        });
    }
    assert!(
        inked > letters.len() / 2,
        "{what}: the premise, the instance path drew most of the letters, {inked} of {}",
        letters.len()
    );
}

/// **Every letter of `letters` with a canonical decomposition drawn on
/// the three paths twice**, as the precomposed letter and as its
/// decomposition typed, at the places [`letters_gate`] uses, each form the
/// first frame of a renderer of its own: on each path the two forms draw
/// the same pixels in each letter's nine cells (`[[source:
/// unicode18-ch3]]` D70, C6). The letters whose forms differ, each with
/// the pixels by which they differ on the instance, cell and CPU paths.
fn forms_gate<'f, 'a>(
    gpu: &Gpu,
    arm: Arm,
    fresh: &dyn Fn() -> Renderer<'f, 'a>,
    letters: &[char],
    (sgr, what): (&str, &str),
) -> Vec<String> {
    let pairs: Vec<(char, String)> = letters
        .iter()
        .filter_map(|&c| canonical_decomposition(c).map(|d| (c, d)))
        .collect();
    let mut split = Vec::new();
    for (k, screen) in pairs.chunks(even(pairs.len(), per_screen())).enumerate() {
        let mut frames = Vec::new();
        let mut geo = (0, 0, 0);
        for decomposed in [false, true] {
            frames.push(with_term(gpu, arm, fresh(), |arms, t| {
                warm(arms, t);
                let mut text = format!("\x1bc\x1b[?25l{sgr}");
                for (i, (c, d)) in screen.iter().enumerate() {
                    let (col, row) = letter_at(i);
                    text.push_str(&format!("\x1b[{};{}H", row + 1, col + 1));
                    match decomposed {
                        false => text.push(*c),
                        true => text.push_str(d),
                    }
                }
                feed(t, text.as_bytes());
                geo = geometry(&arms.renderer);
                arms.check(
                    t,
                    &format!("{arm:?}: {what}, forms screen {k}, decomposed {decomposed}"),
                )
            }));
        }
        for (i, (c, d)) in screen.iter().enumerate() {
            let n = |pick: fn(&Frames) -> &Vec<u32>| {
                near(geo, letter_at(i))
                    .filter(|&p| pick(&frames[0])[p] != pick(&frames[1])[p])
                    .count()
            };
            let d3 = [n(|f| &f.instance), n(|f| &f.cell), n(|f| &f.cpu)];
            if d3 != [0, 0, 0] {
                split.push(format!(
                    "{arm:?}: {what} U+{:04X} {c} against {:04X?}: the decomposition differs \
                     by {} px on the instance path, {} on the cell path, {} on the CPU path",
                    u32::from(*c),
                    d.chars().map(u32::from).collect::<Vec<_>>(),
                    d3[0],
                    d3[1],
                    d3[2]
                ));
            }
        }
    }
    split
}

/// **Every precomposed letter on `arm`**, on stock JetBrains Mono with the
/// Noto subset and its bold and italic faces behind it, over a page
/// configured at alpha 56 and drawn at the alpha the dark theme's text
/// reads at from there ([`Theme::glass_reach`]), where U+01FA shapes to
/// several glyphs and U+00E9 to one -- the premise -- and on the default
/// chain's embedded faces, in its order: regular, bold and italic, alone
/// and against its decomposition.
fn precomposed(gpu: &Gpu, arm: Arm) {
    let letters = precomposed_letters();
    for c in ['Ǻ', 'Ȁ', 'Ṩ', 'Ȅ', 'Ǡ', 'Ḉ', 'ǰ', 'ḉ', 'é'] {
        assert!(letters.contains(&c), "the premise: {c} is swept");
    }
    let theme = Theme {
        bg_alpha: Chan::from_u8(56),
        ..Theme::dark()
    };
    let theme = Theme {
        bg_alpha: theme.glass_reach(),
        ..theme
    };
    let fallback = |p: &str| (repo_file(p), FaceRole::Fallback);
    let styled = |p: &str, s: FaceStyle| (repo_file(p), FaceRole::Style(s));
    let stock = vec![
        fallback("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        fallback("baud-font/tests/fonts/NotoColorEmoji-subset.ttf"),
        styled(
            "baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf",
            FaceStyle::Bold,
        ),
        styled(
            "baud-app/fonts/jetbrains-mono/JetBrainsMono-Italic.ttf",
            FaceStyle::Italic,
        ),
    ];
    let embedded = vec![
        fallback("baud-app/fonts/built/Regular.ttf"),
        fallback("baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
        fallback("baud-app/fonts/stix-two-math/STIXTwoMath-Regular.otf"),
        styled("baud-app/fonts/built/Bold.ttf", FaceStyle::Bold),
        styled("baud-app/fonts/built/Italic.ttf", FaceStyle::Italic),
        styled("baud-app/fonts/built/BoldItalic.ttf", FaceStyle::BoldItalic),
    ];
    let mut split = Vec::new();
    for (chain, files) in [("stock", &stock), ("embedded", &embedded)] {
        let faces: Vec<Face<'_>> = files
            .iter()
            .map(|(b, _)| Face::parse(b).expect("a face"))
            .collect();
        let roles: Vec<FaceRole> = files.iter().map(|(_, r)| *r).collect();
        let make = || fresh(&faces, &roles, theme.clone());
        if chain == "stock" {
            let mut r = make();
            let mut alone = |c: char| {
                r.cluster(
                    &[Codepoint::from_char(c)],
                    FaceStyle::Regular,
                    baud_render::CellSpan::One,
                )
                .len()
            };
            assert!(
                alone('Ǻ') > 1,
                "the premise: U+01FA shapes to several glyphs"
            );
            assert_eq!(alone('é'), 1, "the premise: U+00E9 shapes to one");
        }
        for (name, sgr) in [("regular", ""), ("bold", "\x1b[1m"), ("italic", "\x1b[3m")] {
            let what = format!("{chain} {name}");
            split.extend(forms_gate(gpu, arm, &make, &letters, (sgr, &what)));
            letters_gate(gpu, arm, &make, &letters, (sgr, &what));
        }
    }
    assert!(
        split.is_empty(),
        "{} letters draw otherwise than their decompositions: {split:#?}",
        split.len()
    );
}

#[test]
fn every_precomposed_letter_draws_alike_on_every_arm() {
    let gpu = lavapipe();
    precomposed(&gpu, Arm::InPlace);
    precomposed(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn every_precomposed_letter_draws_alike_on_every_arm_on_radv() {
    let gpu = radv();
    precomposed(&gpu, Arm::Staging);
    precomposed(&gpu, gpu.arm(1));
}

/// The pointed Genesis 1:1-5 (`baud-core/tests/cluster_bounds.rs`), one
/// verse a row.
const GENESIS_1_1_TO_5: [&str; 5] = [
    "בְּרֵאשִׁית בָּרָא אֱלֹהִים אֵת הַשָּׁמַיִם וְאֵת הָאָרֶץ׃",
    "וְהָאָרֶץ הָיְתָה תֹהוּ וָבֹהוּ וְחֹשֶׁךְ עַל־פְּנֵי תְהוֹם וְרוּחַ אֱלֹהִים מְרַחֶפֶת עַל־פְּנֵי הַמָּיִם׃",
    "וַיֹּאמֶר אֱלֹהִים יְהִי אוֹר וַיְהִי־אוֹר׃",
    "וַיַּרְא אֱלֹהִים אֶת־הָאוֹר כִּי־טוֹב וַיַּבְדֵּל אֱלֹהִים בֵּין הָאוֹר וּבֵין הַחֹשֶׁךְ׃",
    "וַיִּקְרָא אֱלֹהִים לָאוֹר יוֹם וְלַחֹשֶׁךְ קָרָא לָיְלָה וַיְהִי־עֶרֶב וַיְהִי־בֹקֶר יוֹם אֶחָד׃",
];

/// The app's faces: the built Regular, the icon face, and the built Bold,
/// Italic and Bold Italic, each with its role.
fn app_files() -> Vec<(Vec<u8>, FaceRole)> {
    vec![
        (
            repo_file("baud-app/fonts/built/Regular.ttf"),
            FaceRole::Fallback,
        ),
        (
            repo_file("baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"),
            FaceRole::Fallback,
        ),
        (
            repo_file("baud-app/fonts/built/Bold.ttf"),
            FaceRole::Style(FaceStyle::Bold),
        ),
        (
            repo_file("baud-app/fonts/built/Italic.ttf"),
            FaceRole::Style(FaceStyle::Italic),
        ),
        (
            repo_file("baud-app/fonts/built/BoldItalic.ttf"),
            FaceRole::Style(FaceStyle::BoldItalic),
        ),
    ]
}

/// The faces of [`app_files`] alone, each with its role.
fn app_faces(files: &[(Vec<u8>, FaceRole)]) -> (Vec<Face<'_>>, Vec<FaceRole>) {
    files
        .iter()
        .map(|(d, r)| (Face::parse(d).expect("a face"), *r))
        .unzip()
}

/// **The app's stack** over [`app_files`]: each face followed by its
/// narrower instances.
fn app_stack(files: &[(Vec<u8>, FaceRole)]) -> (Vec<Face<'_>>, Vec<FaceRole>) {
    let (mut faces, mut roles) = (Vec::new(), Vec::new());
    for (data, role) in files {
        let face = Face::parse(data).expect("a face");
        let widths = baud_render::narrower_widths(&face, &[]);
        faces.push(face);
        roles.push(*role);
        for w in widths {
            let mut narrow = Face::parse(data).expect("a face");
            narrow.set_variations(&[baud_font::var::Variation {
                tag: baud_font::Tag::from_bytes(*b"wdth"),
                value: baud_font::var::Fixed::from_f32(w),
            }]);
            faces.push(narrow);
            roles.push(FaceRole::Narrower);
        }
    }
    assert!(
        roles.iter().filter(|r| **r == FaceRole::Narrower).count() >= 60,
        "the premise: every built face has its narrower instances"
    );
    (faces, roles)
}

/// **The pointed Genesis 1:1-5 in regular, `SGR 3` and `SGR 1;3`, on
/// `arm`**, on the built faces' four styles behind the icon face, over a
/// page configured at alpha 56 and drawn at the alpha the dark theme's
/// text reads at from there ([`Theme::glass_reach`]), in stored order
/// (BDSM reset). The controls: the core keeps every point, and the italic
/// and bold-italic frames differ from the regular one and from each other,
/// so the italic faces' Hebrew is what the paths agree on.
fn italic_hebrew(gpu: &Gpu, arm: Arm) {
    let files = app_files();
    let (faces, roles) = app_faces(&files);
    let theme = Theme {
        bg_alpha: Chan::from_u8(56),
        ..Theme::dark()
    };
    let theme = Theme {
        bg_alpha: theme.glass_reach(),
        ..theme
    };
    let frames = with_term(gpu, arm, fresh(&faces, &roles, theme), |arms, t| {
        let mut frames = Vec::new();
        for (name, sgr) in [
            ("regular", ""),
            ("italic", "\x1b[3m"),
            ("bold italic", "\x1b[1;3m"),
        ] {
            let text = format!(
                "\x1bc\x1b[8l\x1b[?25l{sgr}{}",
                GENESIS_1_1_TO_5.join("\r\n")
            );
            feed(t, text.as_bytes());
            assert_eq!(t.marks_dropped, 0, "{name}: the core keeps every point");
            frames.push(arms.check(t, &format!("{arm:?}: {name} Hebrew")).instance);
        }
        frames
    });
    assert!(frames[1] != frames[0], "italic Hebrew is the Regular's");
    assert!(
        frames[2] != frames[0],
        "bold-italic Hebrew is the Regular's"
    );
    assert!(frames[2] != frames[1], "bold-italic Hebrew is the Italic's");
}

#[test]
fn italic_hebrew_draws_alike_on_every_arm() {
    let gpu = lavapipe();
    italic_hebrew(&gpu, Arm::InPlace);
    italic_hebrew(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn italic_hebrew_draws_alike_on_every_arm_on_radv() {
    let gpu = radv();
    italic_hebrew(&gpu, Arm::Staging);
    italic_hebrew(&gpu, gpu.arm(1));
}

/// Where the README's font cache is: `NOTO_CACHE`, else `baud-readme` in
/// the XDG cache, else in `~/.cache`.
fn readme_cache() -> Option<std::path::PathBuf> {
    std::env::var_os("NOTO_CACHE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(|d| std::path::PathBuf::from(d).join("baud-readme"))
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/baud-readme"))
        })
}

/// The README's font cache's `file`, held to its pin `sha`, where the
/// cache holds it.
fn cached_face(file: &str, sha: &str) -> Option<Vec<u8>> {
    let bytes = readme_cache().and_then(|d| std::fs::read(d.join(file)).ok())?;
    assert_eq!(
        sha256::sha256_hex(&bytes),
        sha,
        "the cache's {file} is not the pinned one"
    );
    Some(bytes)
}

/// **A letter of a joining script wider than its cell, on `arm`**, by rule
/// D (`baud-render/src/fit.rs`): full size into the cells its ink reaches
/// when they are blank, else fitted to its one cell, its marks with it. On
/// `joining-cases.ttf`, whose sad is 1300 units isolated and 950 medial
/// against a cell of 600: a sad beside a reh, one beside two blank cells,
/// one carrying a fatha, medial sads joined between behs, one beside a
/// wide character, and one in the last column -- each with BDSM set and
/// reset, against the control of every sad written as a reh. Then a
/// letter typed into the blank cell a full-size sad reaches, which fits
/// it, and erased again, full size again. Then, where the README's font
/// cache holds it, Noto Sans Arabic 2.013's line.
fn over_wide(gpu: &Gpu, arm: Arm) {
    let built = repo_file("baud-app/fonts/built/Regular.ttf");
    let cases = repo_file("baud-font/tests/var/joining-cases.ttf");
    let faces = [
        Face::parse(&built).expect("the built Regular"),
        Face::parse(&cases).expect("joining-cases.ttf"),
    ];
    let roles = [FaceRole::Fallback, FaceRole::Fallback];
    let rows = [
        "\u{0635}\u{0631}",
        "\u{0635}  \u{0631}",
        "\u{0635}\u{064E}\u{0631}",
        "\u{0628}\u{0635}\u{0635}\u{0628}",
        "\u{0635}\u{4E2D}",
    ];
    // A line of Latin under the words.
    let fox = "\x1b[9;3HThe quick brown fox jumps 0123456789".as_bytes();
    with_term(gpu, arm, fresh(&faces, &roles, Theme::dark()), |arms, t| {
        let mut three =
            |t: &mut Term<'_>, what: &str| arms.check(t, &format!("{arm:?}: {what}")).instance;
        for implicit in [true, false] {
            feed(t, b"\x1b[?25l\x1b[2J\x1b[H");
            feed(t, if implicit { b"\x1b[8h" } else { b"\x1b[8l" });
            feed(t, fox);
            for (i, row) in rows.iter().enumerate() {
                feed(t, format!("\x1b[{};3H{row}", i + 2).as_bytes());
            }
            feed(
                t,
                format!("\x1b[8;{}H\u{0635}", usize::from(WS.ws_col)).as_bytes(),
            );
            let drawn = three(t, if implicit { "BDSM set" } else { "BDSM reset" });
            feed(t, b"\x1b[2J");
            feed(t, fox);
            for (i, row) in rows.iter().enumerate() {
                let rehs = row.replace('\u{0635}', "\u{0631}");
                feed(t, format!("\x1b[{};3H{rehs}", i + 2).as_bytes());
            }
            let control = three(t, "the control");
            assert!(differing(&drawn, &control) > 0, "the control differs");
        }
        feed(t, b"\x1b[2J\x1b[8l");
        feed(t, fox);
        feed(t, "\x1b[2;3H\u{0635}  \u{0631}".as_bytes());
        let full = three(t, "full size");
        feed(t, "\x1b[2;4H\u{0628}".as_bytes());
        let typed = three(t, "typed beside");
        assert!(
            differing(&full, &typed) > 0,
            "the sad fitted once typed beside"
        );
        feed(t, b"\x1b[2;4H ");
        let again = three(t, "erased again");
        assert_eq!(differing(&full, &again), 0, "full size again");
    });
    const SHA: &str = "bd86ca02f087d7f3c3788ba458fb6b73744c7639ed276b8d870dba6def6c40d0";
    if let Some(noto) = cached_face("NotoSansArabic-Regular.ttf", SHA) {
        let faces = [
            Face::parse(&built).expect("the built Regular"),
            Face::parse(&noto).expect("Noto Sans Arabic"),
        ];
        with_term(gpu, arm, fresh(&faces, &roles, Theme::dark()), |arms, t| {
            for mode in [b"\x1b[8h", b"\x1b[8l"] {
                feed(t, b"\x1b[?25l\x1b[2J\x1b[H");
                feed(t, mode);
                feed(t, fox);
                feed(
                    t,
                    "\x1b[2;3H\u{0627}\u{0644}\u{0637}\u{0631}\u{0641}\u{064A}\u{0629} \
                         \u{062A}\u{0636}\u{0639} \u{0643}\u{0644} \u{062D}\u{0631}\u{0641} \
                         \u{0641}\u{064A} \u{0627}\u{0644}\u{0634}\u{0628}\u{0643}\u{0629}."
                        .as_bytes(),
                );
                feed(t, "\x1b[4;3H\u{0636} \u{0635}\u{064E}".as_bytes());
                arms.check(t, &format!("{arm:?}: Noto Sans Arabic"));
            }
        });
    }
}

#[test]
fn an_over_wide_letter_draws_alike_on_every_arm() {
    let gpu = lavapipe();
    over_wide(&gpu, Arm::InPlace);
    over_wide(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn an_over_wide_letter_draws_alike_on_every_arm_on_radv() {
    let gpu = radv();
    over_wide(&gpu, Arm::Staging);
    over_wide(&gpu, gpu.arm(1));
}

/// **SAPV's variants on `arm`, and as the characters they stand for**:
/// Arabic-Indic digits (2), a decimal comma (10), a vowel hidden (16) and,
/// with BDSM reset, a right-to-left line's brackets left as stored (15),
/// against the same rows written with the characters drawn; and the
/// slanted rows (17) of a right-to-left path, against the same rows
/// upright, with BDSM reset and set.
fn presentation_variants(gpu: &Gpu, arm: Arm) {
    let built = repo_file("baud-app/fonts/built/Regular.ttf");
    let italic = repo_file("baud-app/fonts/built/Italic.ttf");
    let faces = [
        Face::parse(&built).expect("the built Regular"),
        Face::parse(&italic).expect("the built Italic"),
    ];
    let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Italic)];
    let cols = usize::from(WS.ws_col);
    // A right-to-left path with BDSM reset, at level 1 throughout: italic
    // Latin, brackets, a vav, a square root L4 flips and a lam-alef pair,
    // each sheared, the brackets as mirrors; and the same on a line BDSM
    // lays out, a Hebrew word with a Latin one at level 2 between.
    const SLANTED: &str = "\x1b[8l\x1b[5;1H\x1b[2 k\x1b[3m\x1b[17 ]l(ab)\u{5D5}\u{221A}\
         \u{644}\u{627}\x1b[0m\x1b[0 ]\x1b[5;1H\x1b[8h\x1b[6;1H\x1b[3m\x1b[17 ]\
         \u{5E9}\u{5DC}\u{5D5}\u{5DD} ok \u{5D5}(\u{221A})\x1b[0m\x1b[0 ]";
    let varied = format!(
        "\x1b[?25l\x1b[2J\x1b[H\x1b[2 ]12\x1b[0 ] \x1b[10 ]3.14\x1b[0 ]\r\n\
         \x1b[16 ]\u{5D1}\u{5BC}\u{5B0}\x1b[0 ]\r\n\
         \x1b[8l\x1b[3;1H\x1b[2 k\x1b[15 ](ab)\x1b[0 ]\x1b[3;1H\x1b[8h\
         {SLANTED}\
         \x1b[10;3HThe quick brown fox jumps 0123456789"
    );
    let written = format!(
        "\x1bc\x1b[?25l\x1b[2J\x1b[H\u{661}\u{662} 3,14\r\n\
         \u{5D1}\u{5BC}\r\n\
         \x1b[8l\x1b[3;{}H)ba(\x1b[3;1H\x1b[8h\
         {SLANTED}\
         \x1b[10;3HThe quick brown fox jumps 0123456789",
        cols - 3
    );
    with_term(gpu, arm, fresh(&faces, &roles, Theme::dark()), |arms, t| {
        let mut three =
            |t: &mut Term<'_>, what: &str| arms.check(t, &format!("{arm:?}: {what}")).instance;
        feed(t, varied.as_bytes());
        // The third row is drawn reversed with BDSM reset; the frames
        // are taken with it reset so both scenes draw that row alike.
        feed(t, b"\x1b[8l");
        let got = three(t, "varied");
        feed(t, written.as_bytes());
        feed(t, b"\x1b[8l");
        let want = three(t, "written");
        assert_eq!(
            differing(&got, &want),
            0,
            "drawn as the characters they stand for"
        );
        // The control: without 17 the slanted row draws otherwise.
        let upright = written.replace("\x1b[17 ]", "");
        feed(t, upright.as_bytes());
        feed(t, b"\x1b[8l");
        let leaning_right = three(t, "without 17");
        assert_ne!(differing(&got, &leaning_right), 0, "17 slants the row");
        // With BDSM set, the sixth row's Hebrew, at the level UAX #9
        // gives it.
        let bdsm = |bytes: &str| format!("\x1bc{bytes}\x1b[8h");
        feed(t, bdsm(&varied).as_bytes());
        let implicit = three(t, "with BDSM set");
        feed(t, bdsm(&varied.replace("\x1b[17 ]", "")).as_bytes());
        let plain = three(t, "with BDSM set, without 17");
        assert_ne!(differing(&implicit, &plain), 0, "17 slants with BDSM set");
    });
}

#[test]
fn presentation_variants_draw_alike_on_every_arm() {
    let gpu = lavapipe();
    presentation_variants(&gpu, Arm::InPlace);
    presentation_variants(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn presentation_variants_draw_alike_on_every_arm_on_radv() {
    let gpu = radv();
    presentation_variants(&gpu, Arm::Staging);
    presentation_variants(&gpu, gpu.arm(1));
}

/// The spacing-mark counts every Devanagari scene is drawn under: each
/// letter and spacing vowel sign a cell, spacing marks joined, and
/// `?2027`'s whole syllable a cell.
const COUNTS: [(&str, baud_domain::SpacingMarks); 3] = [
    ("", baud_domain::SpacingMarks::Column),
    ("", baud_domain::SpacingMarks::Joined),
    ("\x1b[?2027h", baud_domain::SpacingMarks::Column),
];

/// **A syllable on `arm`**: the README's Hindi line in the built
/// Regular's Devanagari behind JetBrains Mono, regular and bold, and a
/// syllable a soft wrap parts, its vowel sign alone at the next row's
/// start, under each count. The premises: the syllables went to the
/// records, and the three counts' frames differ, the columns having moved.
/// Then the line and its bold on the app's stack, whose built Bold draws
/// Devanagari, under each count, each the first frame of a renderer of its
/// own ([`first_frame`]), so a path that drew a bold syllable from the
/// regular face would draw another glyph than the instance path's.
fn syllable(gpu: &Gpu, arm: Arm) {
    let data = jetbrains_mono();
    let built = repo_file("baud-app/fonts/built/Regular.ttf");
    let faces = [
        Face::parse(&data).expect("JetBrains Mono"),
        Face::parse(&built).expect("the built Regular"),
    ];
    let roles = [FaceRole::Fallback, FaceRole::Fallback];
    // ट, र् and म in the last three columns, ि at the next row's start.
    let wrapped = format!("\x1b[4;{}H{HINDI}", usize::from(WS.ws_col) - 2);
    let frames = with_term(gpu, arm, fresh(&faces, &roles, Theme::dark()), |arms, t| {
        let mut frames = Vec::new();
        for (setup, marks) in COUNTS {
            t.set_spacing_marks(marks);
            feed(
                t,
                format!(
                    "\x1bc\x1b[?25l{setup}{HINDI}\r\n\x1b[1m{HINDI}\x1b[m\r\n\
                         the quick brown fox jumps over 0123456789{wrapped}"
                )
                .as_bytes(),
            );
            let f = arms.check(t, &format!("{arm:?}: {setup:?} {marks:?}"));
            let stats = arms.exceptions().stats();
            assert!(
                stats.cells.value() >= 12,
                "{setup:?} {marks:?}: the syllables went to the records: {stats:?}"
            );
            frames.push(f.instance);
        }
        t.set_spacing_marks(baud_domain::SpacingMarks::Column);
        frames
    });
    assert!(
        differing(&frames[0], &frames[1]) > 0,
        "the setting moved nothing"
    );
    assert!(differing(&frames[0], &frames[2]) > 0, "?2027 moved nothing");
    // **On the app's stack**, the bold line in the built Bold.
    let files = app_files();
    let (faces, roles) = app_stack(&files);
    let mut r = fresh(&faces, &roles, Theme::dark());
    let cps: Vec<Codepoint> = HINDI.chars().map(Codepoint::from_char).collect();
    assert_ne!(
        r.face_for(&cps, FaceStyle::Bold),
        r.face_for(&cps, FaceStyle::Regular),
        "the premise: the bold face draws the line's Devanagari"
    );
    drop(r);
    for (setup, marks) in COUNTS {
        first_frame(
            gpu,
            arm,
            (&faces, &roles),
            |t| t.set_spacing_marks(marks),
            &format!("\x1b[?25l{setup}{HINDI}\r\n\x1b[1m{HINDI}\x1b[m"),
            (0, 0),
            &format!("{setup:?} {marks:?} on the app's stack"),
        );
    }
}

#[test]
fn a_syllable_draws_alike_on_every_arm() {
    let gpu = lavapipe();
    syllable(&gpu, Arm::InPlace);
    syllable(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_syllable_draws_alike_on_every_arm_on_radv() {
    let gpu = radv();
    syllable(&gpu, Arm::Staging);
    syllable(&gpu, gpu.arm(1));
}

/// **A Devanagari word, the block cursor on each of its cells and a
/// selection of each of its syllables, on `arm`**
/// (`baud-render/src/word.rs`): the README's Hindi line, each word shaped
/// whole and laid in its columns, the block on every cell of टर्मिनल and
/// अक्षर lighting the syllable that holds the cell, and each of their
/// syllables selected alone -- under each count, in the built Regular's
/// Devanagari behind JetBrains Mono, and, where the README's font cache
/// holds it, in Noto Sans Devanagari's variable file with its narrower
/// instances. The block a word's syllable takes is the records', the cell
/// shader drawing none (`Overlay::cursor_drawn`); the premises: the words
/// went to the records, and the block on two syllables draws two frames.
/// Each stack drawn compares at least 47 frames.
fn devanagari_lights(gpu: &Gpu, arm: Arm) {
    const SHA: &str = "9f82923465ef82fb86c6b99cbdd4947ba832fe3335948de815b18adfd75947c7";
    let data = jetbrains_mono();
    let built = repo_file("baud-app/fonts/built/Regular.ttf");
    let noto = cached_face("NotoSansDevanagari[wdth,wght].ttf", SHA);
    let mut stacks: Vec<(Vec<Face<'_>>, Vec<FaceRole>)> = vec![(
        vec![
            Face::parse(&data).expect("JetBrains Mono"),
            Face::parse(&built).expect("the built Regular"),
        ],
        vec![FaceRole::Fallback, FaceRole::Fallback],
    )];
    if let Some(n) = &noto {
        let mut faces = vec![
            Face::parse(&data).expect("JetBrains Mono"),
            Face::parse(n).expect("Noto Sans Devanagari"),
        ];
        let mut roles = vec![FaceRole::Fallback, FaceRole::Fallback];
        for w in baud_render::narrower_widths(&faces[1], &[]) {
            let mut f = Face::parse(n).expect("Noto Sans Devanagari");
            f.set_variations(&[baud_font::var::Variation {
                tag: baud_font::Tag::from_bytes(*b"wdth"),
                value: baud_font::var::Fixed::from_f32(w),
            }]);
            faces.push(f);
            roles.push(FaceRole::Narrower);
        }
        stacks.push((faces, roles));
    }
    for (faces, roles) in &stacks {
        let drawn = with_term(gpu, arm, fresh(faces, roles, Theme::dark()), |arms, t| {
            let mut drawn = 0usize;
            for (setup, marks) in COUNTS {
                t.set_spacing_marks(marks);
                let screen = format!(
                    "\x1bc\x1b[?25l{setup}\x1b[2;3H{HINDI}\x1b[4;1Hthe quick brown fox 0123456789"
                );
                arms.highlight = None;
                feed(t, screen.as_bytes());
                arms.check(t, &format!("{arm:?}: {setup:?} {marks:?}: the line"));
                assert!(
                    arms.exceptions().stats().cells.value() >= 12,
                    "{setup:?} {marks:?}: the words went to the records"
                );
                let words = arms.renderer.words_on(Row::from_index(1)).to_vec();
                assert_eq!(words.len(), 8, "{setup:?} {marks:?}: {words:?}");
                let mut frames = Vec::new();
                for w in [&words[0], &words[2]] {
                    for c in w.at.index()..w.at.index() + w.cells.index() {
                        feed(t, format!("\x1b[2;{}H\x1b[?25h", c + 1).as_bytes());
                        let what = format!("{arm:?}: {setup:?} {marks:?}: the block on column {c}");
                        frames.push(arms.check(t, &what).instance);
                        assert_eq!(
                            arms.exceptions().cursor_drawn(),
                            CursorDrawn::Records,
                            "{what}: the records draw it"
                        );
                        drawn += 1;
                    }
                    feed(t, b"\x1b[?25l");
                    for &(at, n) in &w.syllables {
                        let row = Row::from_index(1);
                        arms.highlight = Some(Highlight::new(
                            (row, at),
                            (row, Col::at_index(at.index() + n.index() - 1)),
                            Region::Stream,
                        ));
                        let what = format!(
                            "{arm:?}: {setup:?} {marks:?}: column {} selected",
                            at.index()
                        );
                        arms.check(t, &what);
                        drawn += 1;
                    }
                    arms.highlight = None;
                }
                assert!(
                    frames.windows(2).any(|f| differing(&f[0], &f[1]) > 0),
                    "{setup:?} {marks:?}: the block moved nothing"
                );
            }
            drawn
        });
        assert!(
            drawn >= 47,
            "{drawn} frames compared, {} faces",
            faces.len()
        );
    }
}

#[test]
fn a_devanagari_word_and_its_lights_draw_alike_on_every_arm() {
    let gpu = lavapipe();
    devanagari_lights(&gpu, Arm::InPlace);
    devanagari_lights(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_devanagari_word_and_its_lights_draw_alike_on_every_arm_on_radv() {
    let gpu = radv();
    devanagari_lights(&gpu, Arm::Staging);
    devanagari_lights(&gpu, gpu.arm(1));
}

/// **Bold and italic Hindi on `arm`**, in the faces the built faces' four
/// styles give them, behind the icon face: the Hindi line in regular,
/// `SGR 1`, `SGR 3` and `SGR 1;3`, and bold with `?2027` set; a word whose
/// rendition changes inside it, either way; and a bold Latin line. Each is
/// the first frame of a renderer of its own ([`first_frame`]). The
/// premises: the words went to the records, the bold faces' frames are not
/// the regular one, and the italic face draws the line's Devanagari with
/// glyphs of its own.
fn bold_and_italic_hindi(gpu: &Gpu, arm: Arm) {
    let files = app_files();
    let (faces, roles) = app_faces(&files);
    let mut frames = Vec::new();
    for (name, setup, text, words) in [
        ("regular", "", HINDI, 12),
        ("bold", "\x1b[1m", HINDI, 12),
        ("italic", "\x1b[3m", HINDI, 12),
        ("bold italic", "\x1b[1;3m", HINDI, 12),
        ("bold under ?2027", "\x1b[?2027h\x1b[1m", HINDI, 12),
        ("bold from inside a word", "", "र्\x1b[1mमि", 3),
        ("regular from inside a word", "\x1b[1m", "र्\x1b[mमि", 3),
        ("bold Latin", "\x1b[1m", "the quick brown fox", 0),
    ] {
        let (frame, cells, _) = first_frame(
            gpu,
            arm,
            (&faces, &roles),
            |_| {},
            &format!(
                "\x1b[?25l\x1b[2;3H{setup}{text}\x1b[m\x1b[4;1Hthe quick brown fox 0123456789"
            ),
            (1, 2),
            name,
        );
        assert!(
            cells.value() >= words,
            "{name}: the words went to the records: {cells:?}"
        );
        frames.push(frame);
    }
    assert!(frames[1] != frames[0], "bold Hindi is the Regular's");
    assert!(frames[3] != frames[0], "bold-italic Hindi is the Regular's");
    let mut r = fresh(&faces, &roles, Theme::dark());
    let cps: Vec<Codepoint> = HINDI.chars().map(Codepoint::from_char).collect();
    assert_ne!(
        r.face_for(&cps, FaceStyle::Italic),
        r.face_for(&cps, FaceStyle::Regular),
        "the premise: the italic face draws the line's Devanagari"
    );
}

#[test]
fn bold_and_italic_hindi_draw_alike_on_every_arm() {
    let gpu = lavapipe();
    bold_and_italic_hindi(&gpu, Arm::InPlace);
    bold_and_italic_hindi(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn bold_and_italic_hindi_draw_alike_on_every_arm_on_radv() {
    let gpu = radv();
    bold_and_italic_hindi(&gpu, Arm::Staging);
    bold_and_italic_hindi(&gpu, gpu.arm(1));
}

/// **The joins of `row` as the renderer last laid them**: each joined
/// cell's column, form and pair, and its word's letters and forms either
/// side, each told as one line.
fn joins(r: &Renderer<'_, '_>, row: Row) -> Option<Vec<String>> {
    let letters = |ws: &[baud_render::WordCell]| -> Vec<_> {
        ws.iter()
            .map(|w| (w.cell.content.as_codepoint(), w.form))
            .collect()
    };
    r.bidi().row_joins(row).map(|rj| {
        rj.cells()
            .iter()
            .enumerate()
            .filter_map(|(v, j)| {
                let (b, a) = rj.around(j.as_ref()?);
                j.map(|j| {
                    format!(
                        "{v} {:?} {:?} {:?} {:?}",
                        j.form,
                        j.pair,
                        letters(b),
                        letters(a)
                    )
                })
            })
            .collect()
    })
}

/// **`scene` fed, `text` composed at its cursor, against `text` written
/// there underlined**, on the three paths: equal, each against the frame
/// without it, and the composition's row joined as the written row is.
/// What differs is told to `fail`; the answer is whether the row joined.
fn composed_as_written(
    arms: &mut Arms<'_, '_, '_>,
    t: &mut Term<'_>,
    (scene, text): (&str, &str),
    what: &str,
    fail: &dyn Fn(String),
) -> bool {
    let arm = arms.arm();
    feed(t, scene.as_bytes());
    let (row, col) = t.cursor();
    arms.composition = Some(Composition {
        row,
        col,
        text: text.chars().map(Codepoint::from_char).collect(),
        measure: baud_render::Measure::default(),
        hints: Vec::new(),
        caret: Caret::End,
    });
    let composed = arms
        .check(t, &format!("{arm:?}: {what}: composed"))
        .instance;
    let laid = joins(&arms.renderer, row);
    arms.composition = None;
    let plain = arms.check(t, &format!("{arm:?}: {what}: plain")).instance;
    feed(t, format!("\x1b[4m{text}\x1b[24m").as_bytes());
    let written = arms.check(t, &format!("{arm:?}: {what}: written")).instance;
    if laid != joins(&arms.renderer, row) {
        fail(format!("{what}: joined otherwise than written"));
    }
    assert!(differing(&composed, &plain) > 0, "{what}: drawn");
    let d = differing(&composed, &written);
    if d > 0 {
        fail(format!(
            "{what}: as the same text written underlined, {d} pixels"
        ));
    }
    laid.is_some()
}

/// **A word whose face changes between its syllables, on `arm`**
/// (`[[source: ecma48]]` 8.3.117), on the app's stack: each syllable in its
/// own face, each mixed word drawn as neither the word in its first
/// syllable's face nor in its last's; a change inside one syllable to the
/// bold or bold italic face of its first cell's family drawn glyph by
/// glyph from each cell's face; a change between upright and italic inside
/// one drawn in its first cell's face, as the syllable in one rendition; a
/// change of colour alone drawn, of the text or of its ground; and मल
/// composed after a bold क, and मि after a bold र्, drawn as the same
/// letters written there. Each frame the first of a renderer of its own
/// ([`first_frame`]), the compositions each on a renderer of its own.
fn word_face_changes(gpu: &Gpu, arm: Arm) {
    let files = app_files();
    let (faces, roles) = app_stack(&files);
    let frame = |text: &str, what: &str| {
        first_frame(
            gpu,
            arm,
            (&faces, &roles),
            |_| {},
            &format!("\x1b[?25l\x1b[2;3H{text}\x1b[m\x1b[4;1Hthe quick brown fox 0123456789"),
            (1, 2),
            what,
        )
        .0
    };
    for (what, mixed, first, last) in [
        ("bold after", "टर्मि\x1b[1mनल", "टर्मिनल", "\x1b[1mटर्मिनल"),
        (
            "regular after",
            "\x1b[1mटर्मि\x1b[mनल",
            "\x1b[1mटर्मिनल",
            "टर्मिनल",
        ),
        (
            "bold after under ?2027",
            "\x1b[?2027hटर्मि\x1b[1mनल",
            "\x1b[?2027hटर्मिनल",
            "\x1b[?2027h\x1b[1mटर्मिनल",
        ),
        ("bold after a vowel", "अ\x1b[1mक्षर", "अक्षर", "\x1b[1mअक्षर"),
    ] {
        let drawn = frame(mixed, what);
        assert!(
            drawn != frame(first, &format!("{what}: its first face")),
            "{what}: drawn in its first syllable's face"
        );
        assert!(
            drawn != frame(last, &format!("{what}: its last face")),
            "{what}: drawn in its last syllable's face"
        );
    }
    // The built Italic draws Devanagari upright, as the Regular's pixels.
    for (what, mixed) in [
        ("italic after", "टर्मि\x1b[3mनल"),
        ("bold italic after", "टर्मि\x1b[1;3mनल"),
    ] {
        frame(mixed, what);
    }
    for (what, inside, first, last) in [
        (
            "bold inside a syllable",
            "टर्\x1b[1mमि\x1b[mनल",
            "टर्मिनल",
            "\x1b[1mटर्मिनल",
        ),
        (
            "regular inside a syllable",
            "\x1b[1mर्\x1b[mमि",
            "\x1b[1mर्मि",
            "र्मि",
        ),
        ("a bold sign", "म\x1b[1mि", "मि", "\x1b[1mमि"),
        (
            "bold italic inside a syllable",
            "\x1b[3mर्\x1b[1;3mमि",
            "\x1b[3mर्मि",
            "\x1b[1;3mर्मि",
        ),
    ] {
        let drawn = frame(inside, what);
        assert!(
            drawn != frame(first, &format!("{what}: its first face")),
            "{what}: drawn in its first cell's face"
        );
        assert!(
            drawn != frame(last, &format!("{what}: its last face")),
            "{what}: drawn in its last cell's face"
        );
    }
    for (what, inside, whole) in [
        ("a conjunct on its first cell", "क्\x1b[1mष", "क्ष"),
        ("italic inside a syllable", "र्\x1b[3mमि", "र्मि"),
        (
            "bold upright inside a syllable",
            "\x1b[3mर्\x1b[23;1mमि",
            "\x1b[3mर्मि",
        ),
    ] {
        assert_eq!(
            differing(
                &frame(inside, what),
                &frame(whole, &format!("{what}: whole"))
            ),
            0,
            "{what}: drawn as the syllable in its first cell's face"
        );
    }
    assert!(
        frame("टर्मि\x1b[31mनल", "red after") != frame("टर्मिनल", "red after: plain"),
        "the premise: the colour draws"
    );
    for (what, ground) in [
        ("a red ground after", "टर्मि\x1b[41mनल"),
        ("a green ground before", "\x1b[42mटर्मि\x1b[49mनल"),
    ] {
        assert!(
            frame(ground, what) != frame("टर्मिनल", &format!("{what}: plain")),
            "the premise: {what} draws"
        );
    }
    let failed = std::cell::RefCell::new(Vec::<String>::new());
    let fail = |what: String| failed.borrow_mut().push(what);
    for (taken, composed, what) in [
        ("\u{915}", "\u{92e}\u{932}", "a bold letter taken in"),
        (
            "\u{930}\u{94d}",
            "\u{92e}\u{93f}",
            "a bold part of the syllable taken in",
        ),
    ] {
        with_term(gpu, arm, fresh(&faces, &roles, Theme::dark()), |arms, t| {
            composed_as_written(
                arms,
                t,
                (
                    &format!(
                        "\x1bc\x1b[2 q\x1b[5;1HThe quick brown fox jumps over the lazy dog \
                             0123456789\x1b[3;1Hab\x1b[1m{taken}\x1b[m"
                    ),
                    composed,
                ),
                what,
                &fail,
            );
        });
    }
    let failed = failed.into_inner();
    assert!(failed.is_empty(), "{arm:?}: {failed:#?}");
}

#[test]
fn a_word_whose_face_changes_draws_alike_on_every_arm() {
    let gpu = lavapipe();
    word_face_changes(&gpu, Arm::InPlace);
    word_face_changes(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_word_whose_face_changes_draws_alike_on_every_arm_on_radv() {
    let gpu = radv();
    word_face_changes(&gpu, Arm::Staging);
    word_face_changes(&gpu, gpu.arm(1));
}
