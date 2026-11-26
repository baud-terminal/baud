//! **Compositions, drawn alike on every path**: screens fed as bytes with
//! an input method's composition on them, drawn by the scene harness
//! (`scenes::Arms`) on the cell path, the instance path and the CPU path,
//! each frame held to the cell
//! path's bars -- the cell frame the instance frame exactly, and both the
//! CPU path's premultiplied frame, exact where no record blends and within
//! a step where one does -- on lavapipe on both memory arms, and on RADV
//! staged and on the arm its heaps allow. Each frame a scene compares with
//! another is compared on each path against the same path's frame, so a
//! composition drawn as its text written is that on the cell, instance and
//! CPU paths alike.
//!
//! - [`a_composition_on_a_right_to_left_row_draws_as_the_text_written`]:
//!   `cd` and `c(` composed after `ab` and over a red `xy` in a Hebrew
//!   paragraph, on a single- and a double-width line, each the frame of the
//!   same text written underlined at the cursor.
//! - [`a_clustered_composition_draws_as_its_text_written_on_every_arm`]:
//!   two CJK characters, a ZWJ family, a flag, an emoji under `U+FE0F`, a
//!   spacing mark, a box line and `∑` on a line drawn in reverse with
//!   `?2500` before and after a joining letter is drawn, and a vowel sign
//!   and a skin tone composed after their base, each the frame of its text
//!   written.
//! - [`a_composition_against_a_devanagari_word_draws_as_written_on_every_arm`]:
//!   Devanagari composed before, after and between written letters, with a
//!   letter selected, under `?2027`, and the input method's cursor on a
//!   composed syllable, on the Devanagari baud ships in its built face.
//! - [`a_composition_over_varied_text_draws_as_written_on_every_arm`]:
//!   `xyz` over digits SAPV 2 varies, and two vavs where SAPV 17 slants one
//!   italic cell of a Hebrew line, each the frame of its text written.
//!
//! Not drawn here, each for its reason:
//!
//! - `a_composition_of_arabic_draws_joined_as_written_on_every_arm`,
//!   `arabic_decrlm_wrote_draws_joined_with_its_forms_on_every_arm`,
//!   `arabic_letters_draw_joined_on_every_arm` and
//!   `a_letters_word_reads_its_digits_as_drawn_on_every_arm`: their letters
//!   and digits are drawn by Courier New's Arabic, a face the repository
//!   does not carry, and no face it carries holds the teh, yeh, lam, alef,
//!   meem, hah, dal or Arabic-Indic digits they write;
//! - `a_joined_letter_reads_its_word_on_every_arm` and
//!   `sapv_meets_a_letters_word_on_every_arm`: their subject is Noto Sans
//!   Syriac's `rlig` and SF Arabic's `rclt`, faces the repository does not
//!   carry;
//! - `a_slanted_lam_alef_is_read_in_its_word_on_every_arm`: its subject is
//!   SF Arabic's `rclt` forms of kaf, a face the repository does not carry;
//! - the window copy's arm: it needs an API only the macOS presenter has.
//!
//! The faces are baud's built faces from `baud-app/fonts/built` (JetBrains
//! Mono with Hebrew and Devanagari), JetBrains Mono and the Noto colour
//! emoji subset from `baud-font/tests/fonts`, each under the licence beside
//! it.

#![cfg(target_os = "linux")]

mod common;
mod scenes;

use baud_core::{Owned, Term};
use baud_domain::{AmbiguousWidth, Codepoint, Col, Row, WinSize};
use baud_font::Face;
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::lend::Arm;
use baud_render::{
    Caret, Composition, FaceRole, FaceStyle, Highlight, Joins, Measure, Region, Renderer, Stack,
    Theme,
};
use common::{lavapipe, radv};
use scenes::{Arms, Frames, differing, feed, jetbrains_mono, repo_file};

/// Wide enough that a path which stops short of the right-hand side shows.
const WS: WinSize = WinSize {
    ws_col: 120,
    ws_row: 10,
};

/// The faces the right-to-left scenes draw with: baud's built face, which
/// holds the Hebrew letters, and the emoji subset.
fn rtl_faces() -> (Vec<u8>, Vec<u8>) {
    (
        repo_file("baud-app/fonts/built/Regular.ttf"),
        repo_file("baud-font/tests/fonts/NotoColorEmoji-subset.ttf"),
    )
}

/// A renderer over `faces` on the dark theme, sized to [`WS`].
fn fresh<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    scenes::renderer(Stack::new(faces).expect("a stack"), Theme::dark(), WS)
}

/// The codepoints of `s`.
fn cps(s: &str) -> Vec<Codepoint> {
    s.chars().map(Codepoint::from_char).collect()
}

/// **`a` and `b` the same frame on every path**: the cell frames equal,
/// the instance frames equal and the CPU frames equal, pixel for pixel.
fn alike(a: &Frames, b: &Frames, what: &str) {
    for (path, x, y) in [
        ("cell", &a.cell, &b.cell),
        ("instance", &a.instance, &b.instance),
        ("CPU", &a.cpu, &b.cpu),
    ] {
        let d = differing(x, y);
        assert_eq!(d, 0, "{what}: on the {path} path, {d} pixels apart");
    }
}

/// **`a` and `b` different frames on every path**: the premise that what
/// one draws and the other does not reached the pixels.
fn apart(a: &Frames, b: &Frames, what: &str) {
    for (path, x, y) in [
        ("cell", &a.cell, &b.cell),
        ("instance", &a.instance, &b.instance),
        ("CPU", &a.cpu, &b.cpu),
    ] {
        assert!(differing(x, y) > 0, "{what}: on the {path} path, no pixel");
    }
}

/// **An input method's composition on a right-to-left row, on `arm`**: the
/// row is laid out with the composition's text in its cells, so its cells
/// are drawn where UAX #9 puts them and the row's other cells move as a
/// write would move them -- `cd` composed after `ab` in a Hebrew paragraph
/// is drawn `abcd`, the `ab` moved left to make room -- and the cells it
/// covers are hidden where the order draws them: composed over the `xy` of
/// `abxy`, no `x` shows. On a double-width line, laid out over its half,
/// the same. Each composed as `cd` and as `c(`, whose `(` resolves right to
/// left there and is drawn `)`; the reference, the same text written
/// underlined at the cursor, the cursor after it, in the rendition of the
/// logical cell, red over `xy`.
fn composition_right_to_left(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let shalom = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} ab";
    let red_xy = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} ab\x1b[31mxy\x1b[39m";
    for (line, red, width) in [
        (shalom, "", ""),
        (red_xy, "\x1b[31m", ""),
        (shalom, "", "\x1b#6"),
        (red_xy, "\x1b[31m", "\x1b#6"),
    ] {
        for text in ["cd", "c("] {
            let what = format!("{arm:?}: {line:?}{width:?} {text}");
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
            feed(
                &mut t,
                format!(
                    "\x1bc\x1b[2 q\x1b[3;1H{width}{line}\x1b[4;3HThe quick brown fox jumps 0123456789\x1b[3;8H"
                )
                .as_bytes(),
            );
            let (row, col) = t.cursor();
            arms.composition = Some(Composition {
                row,
                col,
                text: cps(text),
                measure: Measure::default(),
                hints: Vec::new(),
                caret: Caret::End,
            });
            let composed = arms.check(&mut t, &format!("{what}: composed"));
            assert!(
                arms.renderer.bidi().reordered(row),
                "{what}: the premise: the row is reordered"
            );
            arms.composition = None;
            let plain = arms.check(&mut t, &format!("{what}: plain"));
            feed(
                &mut t,
                format!("{red}\x1b[4m{text}\x1b[24m\x1b[39m").as_bytes(),
            );
            let written = arms.check(&mut t, &format!("{what}: written"));
            apart(
                &composed,
                &plain,
                &format!("{what}: the composition is drawn"),
            );
            apart(
                &written,
                &plain,
                &format!("{what}: the written text is drawn"),
            );
            alike(
                &composed,
                &written,
                &format!("{what}: as the same text written underlined"),
            );
        }
    }
}

#[test]
fn a_composition_on_a_right_to_left_row_draws_as_the_text_written() {
    let gpu = lavapipe();
    composition_right_to_left(&gpu, Arm::InPlace);
    composition_right_to_left(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_composition_on_a_right_to_left_row_draws_as_the_text_written_on_radv() {
    let gpu = radv();
    composition_right_to_left(&gpu, Arm::Staging);
    composition_right_to_left(&gpu, gpu.arm(1));
}

/// **A composition the core clusters, on `arm`**: two CJK characters over
/// four cells, a ZWJ family, a flag, an emoji under `U+FE0F` and a spacing
/// mark on its letter, composed over a line of text -- and on a line drawn
/// in reverse with `?2500`, under ambiguous width "wide", a box line two
/// cells wide and `∑`, mirrored, before and after a letter that takes a
/// joining form is drawn -- each the frame of the same text written
/// underlined at the cursor, the cursor after it, and each against the
/// frame without it. A vowel sign composed after its consonant and a skin
/// tone after its emoji, on a line drawn in order and in reverse, are
/// drawn on the cell before the cursor ([`Composition::at_cursor`]), the
/// frame of that cell rewritten underlined with them, `HPB` stepping back
/// to it on the reversed line.
fn clustered_composition(gpu: &Gpu, arm: Arm) {
    let (built, noto) = rtl_faces();
    let faces = [
        Face::parse(&built).expect("the built face"),
        Face::parse(&noto).expect("the Noto subset"),
    ];
    let plain =
        "\x1bc\x1b[2 q\x1b[3;1HThe quick brown fox jumps over the lazy dog 0123456789\x1b[3;5H";
    let mirrored = "\x1bc\x1b[2 q\x1b[8l\x1b[3;1H\x1b[?2500h\x1b[2 kThe quick brown fox jumps over the lazy dog 0123456789\x1b[3;5H";
    // `pre` written after "The quick ", the cursor after it.
    let headed = |pre: &str, at: usize, path: &str| {
        format!(
            "\x1bc\x1b[2 q\x1b[8l\x1b[3;1H{path}The quick {pre}brown fox jumps over the lazy dog\x1b[3;{at}H"
        )
    };
    let (ra, sign) = ("\u{930}", "\u{941}");
    let (thumb, tone) = ("\u{1f44d}", "\u{1f3fd}");
    for (scene, text, joins, redo) in [
        (
            plain.to_string(),
            "\u{4e2d}\u{6587}",
            Joins::Unseen,
            ("", 0),
        ),
        (
            plain.to_string(),
            "\u{1f468}\u{200d}\u{1f469}",
            Joins::Unseen,
            ("", 0),
        ),
        (
            plain.to_string(),
            "\u{1f1ef}\u{1f1f5}",
            Joins::Unseen,
            ("", 0),
        ),
        (
            plain.to_string(),
            "\u{2764}\u{fe0f}",
            Joins::Unseen,
            ("", 0),
        ),
        (plain.to_string(), "a\u{903}b", Joins::Unseen, ("", 0)),
        (
            mirrored.to_string(),
            "\u{250C}\u{2500}\u{2502}",
            Joins::Unseen,
            ("", 0),
        ),
        (mirrored.to_string(), "a\u{2211}b", Joins::Unseen, ("", 0)),
        (
            mirrored.to_string(),
            "\u{250C}\u{2500}\u{2502}",
            Joins::Seen,
            ("", 0),
        ),
        (mirrored.to_string(), "a\u{2211}b", Joins::Seen, ("", 0)),
        (headed(ra, 12, ""), sign, Joins::Unseen, (ra, 1)),
        (headed(thumb, 13, ""), tone, Joins::Unseen, (thumb, 2)),
        (headed(ra, 12, "\x1b[2 k"), sign, Joins::Unseen, (ra, 1)),
        (
            headed(thumb, 13, "\x1b[2 k"),
            tone,
            Joins::Unseen,
            (thumb, 2),
        ),
    ] {
        let what = format!("{arm:?}: {text:?} {joins:?}");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
        // After a letter that takes a joining form is drawn, the layout
        // with BDSM reset keeps the last frame's joins.
        if joins == Joins::Seen {
            let joined = "\x1b[8l\x1b[1;1HThe quick brown fox jumps over the lazy dog 0123456789 \u{628}\u{628}\u{628}";
            feed(&mut t, joined.as_bytes());
            arms.check(&mut t, &format!("{what}: a joining letter drawn"));
        }
        assert_eq!(
            arms.renderer.joins_seen(),
            joins,
            "{what}: the premise: the layout"
        );
        if scene == mirrored {
            t.set_ambiguous_width(AmbiguousWidth::Wide);
        }
        feed(&mut t, scene.as_bytes());
        let c = Composition::at_cursor(&t, &cps(text), &[], Caret::End);
        let head = c.col.index() + redo.1 == t.cursor().1.index();
        assert!(head, "{what}: the premise: its head");
        arms.composition = Some(c);
        let composed = arms.check(&mut t, &format!("{what}: composed"));
        arms.composition = None;
        let plain = arms.check(&mut t, &format!("{what}: plain"));
        let (pre, back) = redo;
        // Back by cells, HPB (ECMA-48 8.3.58, the active data position):
        // on a line drawn in reverse with BDSM reset it moves left on the
        // screen, away from the head.
        let rewrite = match back {
            0 => String::new(),
            n => format!("\x1b[{n}j"),
        };
        feed(
            &mut t,
            format!("{rewrite}\x1b[4m{pre}{text}\x1b[24m").as_bytes(),
        );
        let written = arms.check(&mut t, &format!("{what}: written"));
        apart(
            &composed,
            &plain,
            &format!("{what}: the composition is drawn"),
        );
        alike(
            &composed,
            &written,
            &format!("{what}: as the same text written"),
        );
    }
}

#[test]
fn a_clustered_composition_draws_as_its_text_written_on_every_arm() {
    let gpu = lavapipe();
    clustered_composition(&gpu, Arm::InPlace);
    clustered_composition(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_clustered_composition_draws_as_its_text_written_on_every_arm_on_radv() {
    let gpu = radv();
    clustered_composition(&gpu, Arm::Staging);
    clustered_composition(&gpu, gpu.arm(1));
}

/// **A composition against a Devanagari word, on `arm`**: the letters
/// written against it that its own continue are one word with it once
/// committed, which it draws whole, the grid leaving their cells to it.
/// On the built face's Devanagari behind JetBrains Mono: कमल composed after
/// a blank; मल and म composed after a written क, once with the क selected;
/// क composed before a written ल, the block cursor after it on ल; म between
/// a written क and ल; क् before a written ष, the conjunct formed across
/// them; मल after कि under `?2027`; `x` composed against a written कमल; and
/// under `?2027` क्ति composed after a written क and a blank, and क
/// composed before a written क्ति -- each the frame of the same text
/// written underlined at the cursor, the cursor after it, and each against
/// the frame without it. And with the input method's cursor on a syllable
/// of a composed word, the three paths alike.
fn devanagari_word(gpu: &Gpu, arm: Arm) {
    let data = jetbrains_mono();
    let built = repo_file("baud-app/fonts/built/Regular.ttf");
    let faces = [
        Face::parse(&data).expect("JetBrains Mono"),
        Face::parse(&built).expect("the built face"),
    ];
    let ka_glyph = faces[1].glyph(Codepoint::from_char('\u{915}')).is_some();
    assert!(ka_glyph, "the premise: the built face draws Devanagari");
    // The क of `ab` and `क` on row 3, selected alone.
    let ka = Highlight::new(
        (Row::from_index(2), Col::at_index(2)),
        (Row::from_index(2), Col::at_index(2)),
        Region::Stream,
    );
    for (base, text, selected) in [
        ("ab ", "\u{915}\u{92e}\u{932}", None),
        ("ab\u{915}", "\u{92e}\u{932}", None),
        ("ab\u{915}", "\u{92e}", None),
        ("ab\u{915}", "\u{92e}\u{932}", Some(ka)),
        ("ab  \u{932}\x1b[2D", "\u{915}", None),
        ("ab \u{915} \u{932}\x1b[2D", "\u{92e}", None),
        ("ab  \u{937}\x1b[2D", "\u{915}\u{94d}", None),
        ("\x1b[?2027hab\u{915}\u{93f}", "\u{92e}\u{932}", None),
        ("ab  \u{915}\u{92e}\u{932}\x1b[4D", "x", None),
        ("\x1b[?2027h\u{915} ", "\u{915}\u{94d}\u{924}\u{93f}", None),
        (
            "\x1b[?2027h  \u{915}\u{94d}\u{924}\u{93f}\x1b[1G",
            "\u{915}",
            None,
        ),
    ] {
        let what = format!("{arm:?}: {base:?} {text:?}");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
        arms.highlight = selected;
        feed(
            &mut t,
            format!(
                "\x1bc\x1b[2 q\x1b[5;1HThe quick brown fox jumps over the lazy dog 0123456789\x1b[3;1H{base}"
            )
            .as_bytes(),
        );
        arms.composition = Some(Composition::at_cursor(&t, &cps(text), &[], Caret::End));
        let composed = arms.check(&mut t, &format!("{what}: composed"));
        arms.composition = None;
        let plain = arms.check(&mut t, &format!("{what}: plain"));
        feed(&mut t, format!("\x1b[4m{text}\x1b[24m").as_bytes());
        let written = arms.check(&mut t, &format!("{what}: written"));
        apart(
            &composed,
            &plain,
            &format!("{what}: the composition is drawn"),
        );
        alike(&composed, &written, &format!("{what}: as written"));
    }
    // The input method's cursor on म of कमल composed: the block on its
    // cell, the word's ink over it in the cursor's text colour.
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, fresh(&faces), &t);
    feed(
        &mut t,
        b"\x1bc\x1b[2 q\x1b[5;1HThe quick brown fox jumps over the lazy dog 0123456789\x1b[3;1Hab ",
    );
    let word = cps("\u{915}\u{92e}\u{932}");
    arms.composition = Some(Composition::at_cursor(&t, &word, &[], Caret::End));
    let end = arms.check(
        &mut t,
        &format!("{arm:?}: the cursor after a composed word"),
    );
    arms.composition = Some(Composition::at_cursor(&t, &word, &[], Caret::At(1)));
    let on = arms.check(
        &mut t,
        &format!("{arm:?}: the cursor on a composed syllable"),
    );
    apart(
        &on,
        &end,
        &format!("{arm:?}: the cursor moved onto the word"),
    );
}

#[test]
fn a_composition_against_a_devanagari_word_draws_as_written_on_every_arm() {
    let gpu = lavapipe();
    devanagari_word(&gpu, Arm::InPlace);
    devanagari_word(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_composition_against_a_devanagari_word_draws_as_written_on_every_arm_on_radv() {
    let gpu = radv();
    devanagari_word(&gpu, Arm::Staging);
    devanagari_word(&gpu, gpu.arm(1));
}

/// **A composition over text SAPV varies, on `arm`** (ECMA-48 8.3.108):
/// `xyz` composed over digits value 2 draws as the Arabic script's is drawn
/// as `xyz`, not as the digits under it; and two vavs composed where value
/// 17 slants one italic cell of a Hebrew line, in that cell's rendition,
/// both lean left, as written they do, though the cell after the first is
/// upright. On the built Regular and Italic faces.
fn varied_text(gpu: &Gpu, arm: Arm) {
    let regular = repo_file("baud-app/fonts/built/Regular.ttf");
    let italic = repo_file("baud-app/fonts/built/Italic.ttf");
    let faces = [
        Face::parse(&regular).expect("the built face"),
        Face::parse(&italic).expect("the built italic face"),
    ];
    let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Italic)];
    let r = scenes::renderer(
        Stack::with_roles(&faces, &roles).expect("a stack"),
        Theme::dark(),
        WS,
    );
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, r, &t);
    let slanted =
        "\u{5D0}\u{5D1}\x1b[3m\x1b[17 ]\u{5D2}\x1b[0m\x1b[0 ]\u{5D3}\u{5D4}\u{5D5}\x1b[4D";
    for (scene, text, pen) in [
        ("\x1b[2 ]12345\x1b[4D", "xyz", ""),
        (slanted, "\u{5D5}\u{5D5}", "\x1b[3m\x1b[17 ]"),
    ] {
        let what = format!("{arm:?}: {text:?}");
        feed(
            &mut t,
            b"\x1bc\x1b[2 q\x1b[8;3HThe quick brown fox jumps 0123456789\x1b[3;1H",
        );
        feed(&mut t, scene.as_bytes());
        arms.composition = Some(Composition::at_cursor(&t, &cps(text), &[], Caret::End));
        let composed = arms.check(&mut t, &format!("{what}: composed"));
        arms.composition = None;
        let plain = arms.check(&mut t, &format!("{what}: plain"));
        feed(&mut t, format!("{pen}\x1b[4m{text}\x1b[24m").as_bytes());
        let written = arms.check(&mut t, &format!("{what}: written"));
        apart(&composed, &plain, &format!("{what}: drawn"));
        alike(&composed, &written, &format!("{what}: as written"));
    }
}

#[test]
fn a_composition_over_varied_text_draws_as_written_on_every_arm() {
    let gpu = lavapipe();
    varied_text(&gpu, Arm::InPlace);
    varied_text(&gpu, Arm::Staging);
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_composition_over_varied_text_draws_as_written_on_every_arm_on_radv() {
    let gpu = radv();
    varied_text(&gpu, Arm::Staging);
    varied_text(&gpu, gpu.arm(1));
}
