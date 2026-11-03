//! **A Devanagari word drawn whole** (`baud-render/src/word.rs`): shaped as
//! one run at Noto Sans Devanagari's natural advances, centred in its
//! columns, into the blank cells beside it keeping a word space, shifted,
//! narrowed on the face's `wdth` axis, and never scaled; the block cursor
//! and the selection light the syllable that holds the cell, over its pen
//! span (`[[source: ecma48]]` 4.2.27, 4.2.6, 6.1.5).
//!
//! The built Regular carries Noto Sans Devanagari 2.007 and its `wdth`
//! axis (`baud-app/fonts/built/`), so these run on every system with
//! nothing fetched, on the face the default stack draws Hindi from.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::var::{Fixed, Variation};
use baud_font::{Face, Tag};
use baud_render::{
    Argb, Blend, Damage, FaceRole, Highlight, Instance, PxSize, Region, Renderer, Stack, Target,
    Theme, WordHow, WordSide,
};

const README: &str = "टर्मिनल हर अक्षर को ग्रिड में रखता है।";

fn read(path: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The built Regular at its full width, and its narrower instances, as
/// the app's default stack lays it out.
fn faces(regular: &[u8]) -> (Vec<Face<'_>>, Vec<FaceRole>) {
    let mut faces = vec![Face::parse(regular).unwrap()];
    let mut roles = vec![FaceRole::Fallback];
    let widths = baud_render::narrower_widths(&faces[0], &[]);
    assert_eq!(widths.len(), 15, "wdth 97.5 down to 62.5: {widths:?}");
    for w in widths {
        let mut f = Face::parse(regular).unwrap();
        f.set_variations(&[Variation {
            tag: Tag::from_bytes(*b"wdth"),
            value: Fixed::from_f32(w),
        }]);
        faces.push(f);
        roles.push(FaceRole::Narrower);
    }
    (faces, roles)
}

struct Scene<'f, 'a> {
    r: Renderer<'f, 'a>,
    ws: WinSize,
    px: f32,
}

impl<'f, 'a> Scene<'f, 'a> {
    fn new(faces: &'f [Face<'a>], roles: &[FaceRole], px: f32, ws: &WinSize) -> Scene<'f, 'a> {
        let ws = WinSize {
            ws_col: ws.ws_col,
            ws_row: ws.ws_row,
        };
        let r = Renderer::new(
            Stack::with_roles(faces, roles).unwrap(),
            PxSize::of(px).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .unwrap();
        Scene { r, ws, px }
    }

    /// One frame of `bytes` fed to a fresh terminal, `setup` first.
    fn draw(&mut self, setup: impl FnOnce(&mut Term<'_>), bytes: &str) {
        let mut owned = Owned::new(&self.ws);
        let mut t = Term::new(owned.storage(), &self.ws);
        setup(&mut t);
        t.write_at(BytePos::ZERO, bytes.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        let (w, h) = self.r.pixel_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let mut px = vec![0u32; n];
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        self.r.render(&mut t, &mut target, Damage::Full);
    }

    fn cell(&self) -> (i32, i32) {
        let m = self.r.metrics();
        (
            i32::try_from(m.cell_w.value()).unwrap(),
            i32::try_from(m.cell_h.value()).unwrap(),
        )
    }

    /// The instances drawn over row `y`'s band.
    fn on_row(&self, y: usize) -> Vec<Instance> {
        let (_, ch) = self.cell();
        let top = i32::try_from(y).unwrap() * ch;
        self.r
            .instances()
            .copied()
            .filter(|i| {
                let iy = i32::from(i.y.0);
                iy + i32::from(i.h.0) > top && iy < top + ch
            })
            .collect()
    }

    /// The full-height rectangles laid over row `y`: a light's.
    fn lights(&self, y: usize) -> Vec<(i32, i32, Argb)> {
        let (_, ch) = self.cell();
        let top = i32::try_from(y).unwrap() * ch;
        self.on_row(y)
            .iter()
            .filter(|i| {
                i.kind == baud_render::Kind::Solid
                    && i32::from(i.y.0) == top
                    && i32::from(i.h.0) == ch
            })
            .map(|i| {
                (
                    i32::from(i.x.0),
                    i32::from(i.x.0) + i32::from(i.w.0),
                    i.color,
                )
            })
            .collect()
    }
}

fn hex(cps: &str) -> String {
    cps.split_whitespace()
        .map(|h| char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap())
        .collect()
}

/// Noto's lines of `devanagari.txt`: the README's line, the examples of
/// Microsoft's specification and every word of two Premchand pages.
fn corpus() -> Vec<String> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-font/tests/shape/devanagari.txt"
    ))
    .unwrap();
    text.lines()
        .filter(|l| l.starts_with("noto-cache/"))
        .map(|l| hex(l.split('\t').nth(4).unwrap()))
        .collect()
}

/// **No glyph is scaled, and no two words' ink comes closer than a word
/// space**, over every word of the corpus run together at 80 columns, at
/// 24 and 28 px, under the default count, `?2027` and spacing marks
/// joined: Noto's own space, 0.26 em, between two words placed by the
/// word rule, and half of it beside one that fits at no width. Before,
/// a syllable wider than its cells was drawn smaller (`scaled` counted
/// it), and a syllable centred in its own columns left nothing kept
/// between words.
#[test]
fn no_glyph_is_scaled_and_words_keep_a_word_space() {
    let regular = read("baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let words = corpus();
    let text = format!("\x1b[?25l{}", words.join(" "));
    let ws = WinSize {
        ws_col: 80,
        ws_row: 40,
    };
    let mut seen = std::collections::BTreeMap::new();
    let mut overs = Vec::new();
    for px in [24.0f32, 28.0] {
        for (name, setup) in [("default", ""), ("joined", "J"), ("?2027", "\x1b[?2027h")] {
            let mut s = Scene::new(&faces, &roles, px, &ws);
            let bytes = match setup {
                "J" => text.clone(),
                _ => format!("{setup}{text}"),
            };
            s.draw(
                |t| {
                    if setup == "J" {
                        t.set_spacing_marks(baud_domain::SpacingMarks::Joined);
                    }
                },
                &bytes,
            );
            assert_eq!(
                s.r.atlas_stats().scaled.value(),
                0,
                "{name} at {px}: a glyph was drawn at another size"
            );
            let gap = 0.26 * s.px;
            let mut laid = 0usize;
            for y in 0..usize::from(ws.ws_row) {
                let row = s.r.words_on(Row::from_index(y));
                laid += row.len();
                let (cw, _) = s.cell();
                let cw = cw as f32;
                for w in row {
                    *seen.entry(format!("{name} {:?}", w.how)).or_insert(0usize) += 1;
                    if w.instance > 0 {
                        *seen.entry(format!("{name} narrowed")).or_insert(0usize) += 1;
                    }
                    if w.how == WordHow::Over {
                        overs.push(format!("{name} {px} row {y} col {}", w.at.index()));
                        continue;
                    }
                    // A wall's cell and the row's edge are never entered.
                    let (kl, sl, kr, sr) = w.room;
                    if sl != WordSide::Word {
                        let wall = (w.at.index() - kl) as f32 * cw;
                        assert!(
                            w.ink.0 >= wall - 0.02,
                            "{name} at {px}, row {y}: the word at {} reaches left of {wall}: {w:?}",
                            w.at.index()
                        );
                    }
                    if sr != WordSide::Word {
                        let wall = (w.at.index() + w.cells.index() + kr) as f32 * cw;
                        assert!(
                            w.ink.1 <= wall + 0.02,
                            "{name} at {px}, row {y}: the word at {} reaches right of {wall}: {w:?}",
                            w.at.index()
                        );
                    }
                }
                for pair in row.windows(2) {
                    let (a, b) = (&pair[0], &pair[1]);
                    if b.at.index() != a.at.index() + a.cells.index() + 1 {
                        continue;
                    }
                    let floor = match (a.how, b.how) {
                        (WordHow::Over, _) | (_, WordHow::Over) => continue,
                        (WordHow::Residue, _) | (_, WordHow::Residue) => gap / 2.0,
                        _ => gap,
                    };
                    assert!(
                        b.ink.0 - a.ink.1 >= floor - 0.02,
                        "{name} at {px}, row {y}: words at {} and {} {:.2} px apart, under {floor:.2}",
                        a.at.index(),
                        b.at.index(),
                        b.ink.0 - a.ink.1
                    );
                }
            }
            assert!(laid >= 300, "{name} at {px}: {laid} words laid");
        }
    }
    eprintln!("{seen:#?}\nover: {overs:#?}");
    assert!(
        seen.keys().any(|k| k.ends_with("narrowed")),
        "some word took a narrower instance: {seen:?}"
    );
}

/// **A word is laid where hb-shape runs it**: each word of the README's
/// line at full width, its glyphs at the pens HarfBuzz's advances reach in
/// `devanagari.txt`'s whole line on the built Regular, each to the quarter
/// pixel it is
/// placed at -- so its headline joins where the face's does, and nowhere
/// does the grid open it. Before, each syllable was centred in its own
/// cells, its pen moved by the slack around it.
#[test]
fn a_word_is_laid_where_harfbuzz_runs_it() {
    let regular = read("baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-font/tests/shape/devanagari.txt"
    ))
    .unwrap();
    let body = fixture
        .lines()
        .find(|l| {
            l.starts_with("baud-app/fonts/built/Regular.ttf")
                && hex(l.split('\t').nth(4).unwrap()) == README
        })
        .and_then(|l| l.split('\t').nth(5))
        .unwrap();
    // hb-shape's glyphs and the pen each starts at, from the line's start.
    let mut hb = Vec::new();
    let mut pen = 0i32;
    for g in body.trim_matches(|c| c == '[' || c == ']').split('|') {
        let (id, rest) = g.split_once('=').unwrap();
        let (place, ax) = rest.split_once('+').unwrap();
        let dx: i32 = place
            .split_once('@')
            .map_or(0, |(_, o)| o.split_once(',').unwrap().0.parse().unwrap());
        hb.push((id.parse::<u16>().unwrap(), pen + dx));
        pen += ax.parse::<i32>().unwrap();
    }
    for px in [24.0f32, 28.0] {
        let ws = WinSize {
            ws_col: 60,
            ws_row: 4,
        };
        let mut s = Scene::new(&faces, &roles, px, &ws);
        s.draw(|_| {}, &format!("\x1b[?25l\x1b[2;3H{README}"));
        let words = s.r.words_on(Row::from_index(1));
        assert_eq!(words.len(), 8, "{words:?}");
        let k = px / 1000.0;
        for w in words {
            assert_eq!(w.instance, 0, "{w:?}");
            let ids: Vec<u16> = w
                .pens
                .iter()
                .map(|g| u16::try_from(g.0.index()).unwrap())
                .collect();
            let at = hb
                .windows(ids.len())
                .position(|win| win.iter().map(|g| g.0).eq(ids.iter().copied()))
                .unwrap_or_else(|| panic!("{ids:?} not a run of hb-shape's glyphs"));
            for (i, &(_, drawn)) in w.pens.iter().enumerate() {
                let want = w.pens[0].1 + k * (hb[at + i].1 - hb[at].1) as f32;
                assert!(
                    (drawn - want).abs() <= 0.25 + 1e-3,
                    "at {px}, glyph {i} of the word at {}: drawn at {drawn}, hb-shape's pen {want}",
                    w.at.index()
                );
            }
        }
    }
}

/// The syllable of `w` that holds column `c`, by index.
fn syllable_of(w: &baud_render::WordLaid, c: usize) -> usize {
    w.syllables
        .iter()
        .position(|&(at, n)| c >= at.index() && c < at.index() + n.index())
        .unwrap()
}

/// **The block cursor lights the syllable that holds its cell**, on every
/// cell of टर्मिनल and अक्षर, under the default count, spacing marks joined
/// and `?2027`: one rectangle laid under the glyphs exactly over that
/// syllable's pen span, every glyph piece inside it in another colour than
/// the text's, every piece outside it in the text's, and the cell's own
/// ground unlit. Cut at the cell, the block on न's cell lit most of ल.
#[test]
fn the_block_cursor_lights_the_syllable_that_holds_its_cell() {
    let regular = read("baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let ws = WinSize {
        ws_col: 60,
        ws_row: 4,
    };
    let mut checked = 0;
    for (name, setup) in [("default", ""), ("joined", "J"), ("?2027", "\x1b[?2027h")] {
        let mut s = Scene::new(&faces, &roles, 28.0, &ws);
        let join = |t: &mut Term<'_>| {
            if setup == "J" {
                t.set_spacing_marks(baud_domain::SpacingMarks::Joined);
            }
        };
        let pre = if setup == "J" { "" } else { setup };
        s.draw(join, &format!("{pre}\x1b[?25l\x1b[2;3H{README}"));
        let words: Vec<_> = s.r.words_on(Row::from_index(1)).to_vec();
        let text = s
            .on_row(1)
            .iter()
            .find(|i| i.kind == baud_render::Kind::Mask)
            .map(|i| i.color)
            .unwrap();
        for w in [&words[0], &words[2]] {
            for c in w.at.index()..w.at.index() + w.cells.index() {
                s.draw(
                    join,
                    &format!("{pre}\x1b[?25l\x1b[2;3H{README}\x1b[2;{}H\x1b[?25h", c + 1),
                );
                let lit = s.lights(1);
                let span = w.spans[syllable_of(w, c)];
                assert_eq!(
                    lit.iter().map(|l| (l.0, l.1)).collect::<Vec<_>>(),
                    vec![span],
                    "{name}: the block on column {c} lights {lit:?}, its syllable is {span:?}"
                );
                for i in s
                    .on_row(1)
                    .iter()
                    .filter(|i| i.kind == baud_render::Kind::Mask)
                {
                    let (x0, x1) = (i32::from(i.x.0), i32::from(i.x.0) + i32::from(i.w.0));
                    if x0 >= span.0 && x1 <= span.1 {
                        assert_ne!(
                            i.color, text,
                            "{name}: column {c}, a piece inside the block"
                        );
                    } else if (x1 <= span.0 || x0 >= span.1)
                        && x0 >= w.ink.0 as i32 - 2
                        && x1 <= w.ink.1 as i32 + 2
                    {
                        assert_eq!(
                            i.color, text,
                            "{name}: column {c}, a piece outside the block"
                        );
                    }
                }
                checked += 1;
            }
        }
    }
    assert!(checked >= 20, "{checked} cursor places");
}

/// **A selection lights exactly the syllables it selects**: each syllable
/// of the README's line selected alone, under the default count and
/// `?2027`, is lit over its own pen span and nothing else. Cut at the
/// cells, selecting न inverted most of ल.
#[test]
fn a_selection_lights_exactly_the_syllables_it_selects() {
    let regular = read("baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let ws = WinSize {
        ws_col: 60,
        ws_row: 4,
    };
    let mut checked = 0;
    for setup in ["", "\x1b[?2027h"] {
        let mut s = Scene::new(&faces, &roles, 28.0, &ws);
        let line = format!("{setup}\x1b[?25l\x1b[2;3H{README}");
        s.draw(|_| {}, &line);
        let words: Vec<_> = s.r.words_on(Row::from_index(1)).to_vec();
        for w in &words {
            for (k, &(at, n)) in w.syllables.iter().enumerate() {
                let row = Row::from_index(1);
                s.r.set_highlight(Some(Highlight::new(
                    (row, at),
                    (row, Col::at_index(at.index() + n.index() - 1)),
                    Region::Stream,
                )));
                s.draw(|_| {}, &line);
                let lit: Vec<(i32, i32)> = s.lights(1).iter().map(|l| (l.0, l.1)).collect();
                assert_eq!(
                    lit,
                    vec![w.spans[k]],
                    "{setup:?}: syllable {k} of the word at {}",
                    w.at.index()
                );
                checked += 1;
            }
        }
        s.r.set_highlight(None);
    }
    assert!(checked >= 30, "{checked} selections");
}

/// **No ink lies in the colour of the ground under it**: a program's
/// colours on one cell of टर्मिनल at a time -- black on yellow, as a search
/// hit draws -- with the block cursor on another and a selection on a
/// third, under the default count: every glyph piece's colour differs from
/// the ground under each of its columns, a lit rectangle's or the cell's.
/// Each syllable's glyphs drawn in its first cell's colours over the cells'
/// grounds lost the letters a hit's yellow lay under.
#[test]
fn no_ink_lies_in_the_colour_of_its_ground() {
    let regular = read("baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let ws = WinSize {
        ws_col: 40,
        ws_row: 4,
    };
    let mut s = Scene::new(&faces, &roles, 28.0, &ws);
    let mut pieces = 0usize;
    for hit in 2..8 {
        // Each cell the core gives टर्मिनल, the one at column `hit` in the
        // hit's colours.
        let row = Row::from_index(1);
        let mut bytes = String::from("\x1b[?25l\x1b[2;3H");
        let units = ["ट", "र्", "म", "ि", "न", "ल"];
        for (k, u) in units.iter().enumerate() {
            if k + 2 == hit {
                bytes.push_str(&format!("\x1b[30;43m{u}\x1b[m"));
            } else {
                bytes.push_str(u);
            }
        }
        bytes.push_str(" हर");
        let cursor = if hit == 7 { 3 } else { 7 };
        bytes.push_str(&format!("\x1b[2;{}H\x1b[?25h", cursor + 1));
        s.r.set_highlight(Some(Highlight::new(
            (row, Col::at_index(9)),
            (row, Col::at_index(10)),
            Region::Stream,
        )));
        s.draw(|_| {}, &bytes);
        let (cw, _) = s.cell();
        let lit = s.lights(1);
        let grounds: Vec<Argb> = s.r.background().to_vec();
        let ground = |x: i32| {
            lit.iter()
                .find(|l| x >= l.0 && x < l.1)
                .map(|l| l.2)
                .unwrap_or_else(|| grounds[40 + usize::try_from(x / cw).unwrap()])
        };
        for i in s
            .on_row(1)
            .iter()
            .filter(|i| i.kind == baud_render::Kind::Mask)
        {
            for x in i32::from(i.x.0)..i32::from(i.x.0) + i32::from(i.w.0) {
                assert_ne!(
                    i.color,
                    ground(x),
                    "a hit on column {hit}: ink at x {x} in its ground's colour"
                );
            }
            pieces += 1;
        }
    }
    assert!(pieces > 50, "{pieces} pieces");
}

/// **A word written against a comma or a hyphen is laid against it**, as
/// typeset text sets them: छाती, and लिखा-पढ़ी from Premchand's page, each
/// word's ink within 0.15 em of the punctuation's cell on the side it
/// touches -- flush, or, where its ink overhangs its pen, shifted to that
/// cell's edge -- under the default count, at 24 and 28 px. Centred in its
/// columns, छाती stood 0.5 em off its comma, and the hyphen floated
/// between its two words.
#[test]
fn a_word_written_against_punctuation_is_laid_against_it() {
    let regular = read("baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let ws = WinSize {
        ws_col: 40,
        ws_row: 4,
    };
    let mut against = 0;
    for px in [24.0f32, 28.0] {
        let mut s = Scene::new(&faces, &roles, px, &ws);
        s.draw(|_| {}, "\x1b[?25l\x1b[2;3Hछाती, लिखा-पढ़ी हुआ,");
        let (cw, _) = s.cell();
        let cw = cw as f32;
        let words = s.r.words_on(Row::from_index(1)).to_vec();
        assert_eq!(words.len(), 4, "{words:?}");
        for w in &words {
            let (kl, sl, kr, sr) = w.room;
            if kr == 0 && sr == WordSide::Wall && w.how != WordHow::Over {
                let edge = (w.at.index() + w.cells.index()) as f32 * cw;
                assert!(
                    edge - w.ink.1 <= 0.15 * px,
                    "at {px}: the word at {} stands {:.2} px off what follows it",
                    w.at.index(),
                    edge - w.ink.1
                );
                against += 1;
            }
            if kl == 0 && sl == WordSide::Wall && w.how != WordHow::Over {
                let edge = w.at.index() as f32 * cw;
                assert!(
                    w.ink.0 - edge <= 0.15 * px,
                    "at {px}: the word at {} stands {:.2} px off what precedes it",
                    w.at.index(),
                    w.ink.0 - edge
                );
                against += 1;
            }
        }
    }
    assert!(against >= 8, "{against} words laid against punctuation");
}

/// **A block blinked off lights nothing**: with the block on a cell of
/// टर्मिनल at no opacity, the row draws exactly what it draws with no
/// cursor, under the default count and `?2027` -- no rectangle laid in the
/// syllable's own ground, which over a translucent ground would darken it,
/// and no glyph recoloured. At full opacity the two differ, the premise.
#[test]
fn a_block_blinked_off_lights_nothing() {
    let regular = read("baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let ws = WinSize {
        ws_col: 40,
        ws_row: 4,
    };
    for setup in ["", "\x1b[?2027h"] {
        let mut s = Scene::new(&faces, &roles, 28.0, &ws);
        let line = format!("{setup}\x1b[?25l\x1b[2;3H{README}");
        s.draw(|_| {}, &line);
        let none = s.on_row(1);
        let on = format!("{line}\x1b[2;7H\x1b[?25h");
        s.r.set_cursor_blink(baud_render::CursorPhase::SHOWN);
        s.draw(|_| {}, &on);
        assert_ne!(s.on_row(1), none, "{setup:?}: the block draws");
        s.r.set_cursor_blink(baud_render::Blink::Hidden);
        s.draw(|_| {}, &on);
        assert_eq!(
            s.on_row(1),
            none,
            "{setup:?}: blinked off, as with no cursor"
        );
    }
}

/// **A selection across two words and the space between lights it
/// without a hole**: from टर्मिनल's first cell to हर's last, every pixel
/// column from the first word's ink to the second's is lit, by a word's
/// rectangle or by the space's own selected ground -- the strips between a
/// word's pens and its cells' edges lit with the cells beside them.
#[test]
fn a_selection_across_words_leaves_no_hole() {
    let regular = read("baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let ws = WinSize {
        ws_col: 40,
        ws_row: 4,
    };
    for setup in ["", "\x1b[?2027h"] {
        let mut s = Scene::new(&faces, &roles, 28.0, &ws);
        let line = format!("{setup}\x1b[?25l\x1b[2;3H{README}");
        s.draw(|_| {}, &line);
        let words: Vec<_> = s.r.words_on(Row::from_index(1)).to_vec();
        let (a, b) = (&words[0], &words[1]);
        let row = Row::from_index(1);
        s.r.set_highlight(Some(Highlight::new(
            (row, a.at),
            (row, Col::at_index(b.at.index() + b.cells.index() - 1)),
            Region::Stream,
        )));
        s.draw(|_| {}, &line);
        let lit = s.lights(1);
        let colour = lit.first().map(|l| l.2).expect("the words are lit");
        let (cw, _) = s.cell();
        let grounds: Vec<Argb> = s.r.background().to_vec();
        let from = a.ink.0.ceil() as i32;
        let to = b.ink.1.floor() as i32;
        for x in from..to {
            let by_word = lit.iter().any(|l| x >= l.0 && x < l.1);
            let by_cell = grounds[40 + usize::try_from(x / cw).unwrap()] == colour;
            assert!(by_word || by_cell, "{setup:?}: column {x} unlit: {lit:?}");
        }
    }
}

/// **A rectangular selection lights each row's syllables over their own
/// spans**: the block over two rows of the same column range lights, on
/// each row, the pen spans of the syllables its columns hold, so its edges
/// follow the letters and need not line up -- the same presentation
/// position the stream selection marks.
#[test]
fn a_rectangular_selection_lights_each_rows_syllables() {
    let regular = read("baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let ws = WinSize {
        ws_col: 40,
        ws_row: 5,
    };
    let mut s = Scene::new(&faces, &roles, 28.0, &ws);
    let text = format!("\x1b[?25l\x1b[2;3H{README}\x1b[3;3Hअक्षर टर्मिनल को");
    s.draw(|_| {}, &text);
    s.r.set_highlight(Some(Highlight::new(
        (Row::from_index(1), Col::at_index(4)),
        (Row::from_index(2), Col::at_index(5)),
        Region::Block,
    )));
    s.draw(|_| {}, &text);
    for y in [1usize, 2] {
        let w = &s.r.words_on(Row::from_index(y))[0];
        let want: Vec<(i32, i32)> = w
            .syllables
            .iter()
            .zip(&w.spans)
            .filter(|((at, n), _)| at.index() <= 5 && at.index() + n.index() > 4)
            .map(|(_, &span)| span)
            .collect();
        let lit: Vec<(i32, i32)> = s.lights(y).iter().map(|l| (l.0, l.1)).collect();
        let joined = (want.first().unwrap().0, want.last().unwrap().1);
        assert_eq!(lit, vec![joined], "row {y}: {lit:?} for {want:?}");
    }
}
