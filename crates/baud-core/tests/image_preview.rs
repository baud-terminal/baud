//! The image preview a file manager draws when the terminal gives it no
//! image protocol: chafa's text output, a grid of cells each carrying its
//! own foreground and background.
//!
//! This file used to say yazi drew upper half blocks (`U+2580`) with the
//! upper pixel as the foreground, and that baud's DA was what made it choose
//! that. Both were wrong. The half-block shape was assumed, not captured.
//! What yazi sends is chafa's `symbols` output: block elements, box-drawing
//! strokes and plain spaces for the flat areas. What decided the path on
//! 2026-09-22 was the parent terminal's `TERM_PROGRAM` leaking through
//! baud's pty, not the DA. The fixtures in `tools/tests/previews/` are real
//! captures, and `tools/tests/previews/README.md` says how each was made.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use baud_domain::{Attrs, ColorWord};

/// One cell of a text-drawn image: a truecolour foreground and background,
/// then a block glyph -- the shape of one cell of chafa's output.
fn cell(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> Vec<u8> {
    format!(
        "\x1b[38;2;{};{};{};48;2;{};{};{}m\u{2580}",
        fg.0, fg.1, fg.2, bg.0, bg.1, bg.2
    )
    .into_bytes()
}

/// A distinct foreground for every cell, built from `u8` components so there
/// is no cast: `(0, g, b)` walks the green/blue plane, one per cell, which is
/// 65_536 distinct colours -- far past a screenful. Non-zero blue keeps early
/// cells off the default.
fn distinct_foregrounds(count: usize) -> Vec<(u8, u8, u8)> {
    let mut v = Vec::with_capacity(count);
    'fill: for g in 0u8..=255 {
        for b in 1u8..=255 {
            v.push((0u8, g, b));
            if v.len() == count {
                break 'fill;
            }
        }
    }
    v
}

/// **The scramble a file manager's photo preview shows, proved without a
/// screenshot.** A text-drawn preview mints one distinct truecolour rendition
/// per cell, and a photo has thousands of them; every one is live at once,
/// because the whole image is on screen. When the rendition table is smaller
/// than the screen, past the last free slot `intern_style` evicts a slot an
/// on-screen cell still references, and that earlier cell then resolves through
/// `Term::style_of` to a *later* cell's colour -- the wrong colour, which is
/// the scramble. A garbage collector cannot help: nothing is dead to reclaim
/// while the image is displayed. The fix is capacity, so the table is sized to
/// the screen; this pins that a full screen of distinct renditions resolves
/// exactly.
///
/// Against a 1024-slot table this failed with 6976 of 8000 cells wrong, first
/// wrong row 0 -- the eviction corrupts the cells interned first, which is
/// exactly the top-band scramble the app showed over an otherwise coherent
/// image. Rows fill top to bottom, so the top band is what goes.
#[test]
fn a_photo_half_block_preview_does_not_scramble_its_colours() {
    let ws = support::CORPUS; // 200 x 50, sized so its cells hold distinct styles
    let rows = 40; // clear of the bottom, so no scroll retires a live cell
    let cols = usize::from(ws.ws_col);
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);

    // Every cell a unique foreground -- the shape of a real photo, where no two
    // adjacent pixels share a colour. They lay out row-major under autowrap,
    // one upper-half block a cell.
    let fgs = distinct_foregrounds(rows * cols);
    let bg = (7u8, 7u8, 7u8); // a constant lower pixel; the fg is what varies
    let mut bytes = Vec::new();
    for &fg in &fgs {
        bytes.extend(cell(fg, bg));
    }
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));

    let mut wrong = 0usize;
    let mut first_wrong_row = None;
    for (idx, &fg) in fgs.iter().enumerate() {
        let row = t
            .row(Row::from_index(idx / cols))
            .expect("a row on the grid");
        let sw = t.styles().one().word_of(row[idx % cols]);
        if sw.fg().as_rgb() != Some(fg) || sw.bg().as_rgb() != Some(bg) {
            wrong += 1;
            first_wrong_row.get_or_insert(idx / cols);
        }
    }
    assert_eq!(
        wrong,
        0,
        "{wrong} of {} cells resolve to the wrong colour (first wrong row {:?}); \
         the {rows}x{cols} preview mints that many distinct renditions and the \
         table must hold a screenful or on-screen cells are evicted",
        fgs.len(),
        first_wrong_row,
    );
}

/// A cell's colour as chafa sent it, parsed independently of baud.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sent {
    Default,
    Rgb(u8, u8, u8),
    Index(u8),
}

/// Whether `SGR 7` is in force: chafa inverts a cell rather than swapping
/// its two colours when that is the shorter spelling.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rev {
    Off,
    On,
}

/// One cell of chafa's output: the character and the rendition in force.
#[derive(Clone, Copy, Debug)]
struct Want {
    ch: char,
    fg: Sent,
    bg: Sent,
    rev: Rev,
}

/// **The reference parser**: chafa's output grammar only -- `SGR 0`,
/// `SGR 7`, `38;2;r;g;b`, `48;2;r;g;b`, `38;5;n`, `48;5;n`, several in one
/// sequence -- and panics on anything else, so a fixture that grew a shape
/// this cannot read fails loudly rather than being compared on a guess.
fn parse(line: &str, fg: &mut Sent, bg: &mut Sent, rev: &mut Rev) -> Vec<Want> {
    let mut out = Vec::new();
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            out.push(Want {
                ch: c,
                fg: *fg,
                bg: *bg,
                rev: *rev,
            });
            continue;
        }
        assert_eq!(
            it.next(),
            Some('['),
            "chafa sent an escape that is not a CSI"
        );
        let mut body = String::new();
        for c in it.by_ref() {
            if c == 'm' {
                break;
            }
            assert!(
                c.is_ascii_digit() || c == ';',
                "chafa sent a CSI that is not SGR"
            );
            body.push(c);
        }
        let ps: Vec<u8> = body
            .split(';')
            .map(|p| p.parse().expect("an SGR parameter"))
            .collect();
        let mut k = 0;
        while k < ps.len() {
            match ps[k] {
                0 => {
                    *fg = Sent::Default;
                    *bg = Sent::Default;
                    *rev = Rev::Off;
                    k += 1;
                }
                7 => {
                    *rev = Rev::On;
                    k += 1;
                }
                38 | 48 => {
                    let (colour, step) = match ps[k + 1] {
                        2 => (Sent::Rgb(ps[k + 2], ps[k + 3], ps[k + 4]), 5),
                        5 => (Sent::Index(ps[k + 2]), 3),
                        other => panic!("chafa sent colour form {other}"),
                    };
                    if ps[k] == 38 {
                        *fg = colour;
                    } else {
                        *bg = colour;
                    }
                    k += step;
                }
                other => panic!("chafa sent SGR {other}"),
            }
        }
    }
    out
}

fn resolved(c: ColorWord) -> Sent {
    if c.is_default() {
        Sent::Default
    } else if let Some((r, g, b)) = c.as_rgb() {
        Sent::Rgb(r, g, b)
    } else if let Some(n) = c.as_index() {
        Sent::Index(n)
    } else {
        panic!("a colour that is neither default, direct nor indexed")
    }
}

/// Feeds a chafa capture the way yazi 26.5.6 puts it on the screen --
/// split on `\n`, each line written at the pane's origin one row further
/// down, with a cursor move between -- and requires every cell to hold the
/// character and both colours chafa sent. Checks one capture.
fn preview_lands_cell_for_cell(file: &str) {
    let path = format!(
        "{}/../../tools/tests/previews/{file}",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).expect("the capture");
    let ws = support::CORPUS;
    let (row0, col0) = (5usize, 120usize); // the right-hand pane
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let (mut fg, mut bg, mut rev) = (Sent::Default, Sent::Default, Rev::Off);
    let mut want: Vec<Vec<Want>> = Vec::new();
    let mut bytes = Vec::new();
    for (i, line) in text.split('\n').filter(|l| !l.is_empty()).enumerate() {
        bytes.extend(format!("\x1b[{};{}H", row0 + i + 1, col0 + 1).into_bytes());
        bytes.extend(line.as_bytes());
        want.push(parse(line, &mut fg, &mut bg, &mut rev));
    }
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));

    // The positive control: a capture that parsed to nothing, or to one
    // class, would make the comparison below vacuous.
    let cells: usize = want.iter().map(Vec::len).sum();
    assert_eq!(want.len(), 28, "{file}: rows");
    assert_eq!(cells, 28 * 41, "{file}: cells");
    let count = |f: &dyn Fn(char) -> bool| want.iter().flatten().filter(|w| f(w.ch)).count();
    assert!(count(&|c| c == ' ') > 0, "{file}: no space cell");
    assert!(
        count(&|c| ('\u{2580}'..='\u{259f}').contains(&c)) > 0,
        "{file}: no block"
    );
    assert!(
        count(&|c| ('\u{2500}'..='\u{257f}').contains(&c)) > 0,
        "{file}: no box-drawing"
    );
    // Every cell's *drawn* background is one chafa set: its background,
    // or under `SGR 7` its foreground. chafa spells nine cells of each
    // capture the second way, which is what makes the reverse arm of the
    // opaque rule matter.
    let drawn_set = |w: &Want| match w.rev {
        Rev::Off => w.bg != Sent::Default,
        Rev::On => w.fg != Sent::Default,
    };
    assert!(
        want.iter().flatten().all(drawn_set),
        "{file}: a cell drawn on the default background is not what chafa sends"
    );
    assert!(
        want.iter().flatten().any(|w| w.rev == Rev::On),
        "{file}: no reversed cell, so the reverse arm goes untested"
    );

    let mut wrong = Vec::new();
    for (i, line) in want.iter().enumerate() {
        let row = t.row(Row::from_index(row0 + i)).expect("a row on the grid");
        for (j, w) in line.iter().enumerate() {
            let cell = row[col0 + j];
            let got = cell.content.as_codepoint().map(|p| p.value());
            let sw = t.styles().one().word_of(cell);
            let (gfg, gbg) = (resolved(sw.fg()), resolved(sw.bg()));
            let grev = if sw.attrs().has(Attrs::REVERSE) {
                Rev::On
            } else {
                Rev::Off
            };
            if got != Some(u32::from(w.ch)) || gfg != w.fg || gbg != w.bg || grev != w.rev {
                wrong.push((i, j, w.ch, got, (gfg, w.fg), (gbg, w.bg), (grev, w.rev)));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{file}: {} of {cells} cells differ; first {:?}",
        wrong.len(),
        wrong.first()
    );
}

/// The capture baud's own child environment produces: truecolour.
#[test]
fn a_truecolour_chafa_preview_lands_cell_for_cell() {
    preview_lands_cell_for_cell("chafa-truecolour.ans");
}

/// The capture baud received while `TERM_PROGRAM=Apple_Terminal` leaked
/// through: 240 colours, mostly spaces.
#[test]
fn a_240_colour_chafa_preview_lands_cell_for_cell() {
    preview_lands_cell_for_cell("chafa-240.ans");
}
