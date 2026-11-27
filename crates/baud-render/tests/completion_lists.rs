//! **A completion list is drawn and stays drawn** (`tools/tests/completion/`):
//! the recorded bash and zsh sessions that print the prompt again below
//! the list, painted on the CPU path after every read as a driver paints
//! them, with `[screen] wrap_words` on and off. The last frame is the
//! frame the same keys paint with no `OSC 133` marks, pixel for pixel;
//! the marks once erased the list as it was drawn
//! (`crates/baud-core/tests/completion_lists.rs`).

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme, WordWrap};
use support::{Canvas, font};

/// A recording's bytes, its window and the length of each read, from its
/// `.steps` file, whose `#` lines say what was sent.
fn load(name: &str) -> (Vec<u8>, WinSize, Vec<usize>) {
    let dir = format!(
        "{}/../../tools/tests/completion",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read(format!("{dir}/{name}.raw")).expect("the recording");
    let text = std::fs::read_to_string(format!("{dir}/{name}.steps")).expect("its steps");
    let mut ws = None;
    let reads: Vec<usize> = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (size, len) = l.split_once(' ').expect("COLSxROWS <len>");
            let (c, r) = size.split_once('x').expect("COLSxROWS");
            ws = Some(WinSize {
                ws_col: c.parse().expect("cols"),
                ws_row: r.parse().expect("rows"),
            });
            len.parse().expect("length")
        })
        .collect();
    assert_eq!(reads.iter().sum::<usize>(), raw.len(), "{name}");
    (raw, ws.expect("a read"), reads)
}

fn renderer<'f>(faces: &'f [Face<'f>], ws: &WinSize, wrap: WordWrap) -> Renderer<'f, 'f> {
    let mut r = Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(support::PX).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    r.set_word_wrap(wrap);
    r
}

/// Every read of `name` fed and a frame painted after it, the first in
/// full and the rest as they changed; the last frame's pixels.
fn last_frame(faces: &[Face<'_>], name: &str, wrap: WordWrap) -> Canvas {
    let (raw, ws, reads) = load(name);
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    // The cursor is hidden so that a frame is the cells alone.
    t.write_at(BytePos::ZERO, b"\x1b[?25l");
    t.feed(BytePos::ZERO, ReadLen::from_index(6));
    let mut r = renderer(faces, &ws, wrap);
    let mut canvas = Canvas::for_renderer(&r);
    let mut damage = Damage::Full;
    let mut at = 0;
    for n in reads {
        t.write_at(BytePos::ZERO, &raw[at..at + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        at += n;
        // The shells show the cursor again as they draw: keep it hidden.
        t.write_at(BytePos::ZERO, b"\x1b[?25l");
        t.feed(BytePos::ZERO, ReadLen::from_index(6));
        let seal = t.seal_writes();
        r.render(&t, seal, &mut canvas.target(), damage);
        damage = Damage::Incremental;
    }
    canvas
}

/// A frame of nothing, the cursor hidden.
fn empty_frame(faces: &[Face<'_>], ws: &WinSize, wrap: WordWrap) -> Canvas {
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    t.write_at(BytePos::ZERO, b"\x1b[?25l");
    t.feed(BytePos::ZERO, ReadLen::from_index(6));
    let mut r = renderer(faces, ws, wrap);
    let mut canvas = Canvas::for_renderer(&r);
    let seal = t.seal_writes();
    r.render(&t, seal, &mut canvas.target(), Damage::Full);
    canvas
}

/// One cell row's pixels.
fn band(c: &Canvas, ws: &WinSize, row: usize) -> Vec<u32> {
    let ch = c.height / usize::from(ws.ws_row);
    c.pixels[row * ch * c.width..(row + 1) * ch * c.width].to_vec()
}

/// **bash's double Tab and zsh's list that does not fit below its
/// prompt**: with the word wrap on and off, the last frame is the frame
/// the unmarked session paints, and the list's rows are painted in it.
#[test]
fn the_list_is_painted_as_the_unmarked_session_paints_it() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    for (marked, plain) in [
        ("bash-double", "bash-double-plain"),
        ("zsh-tall", "zsh-tall-plain"),
    ] {
        let ws = load(plain).1;
        for wrap in [WordWrap::On, WordWrap::Off] {
            let want = last_frame(&faces, plain, wrap);
            // The premise: the unmarked frame paints the list's first row,
            // which an empty row's band does not equal.
            let blank = empty_frame(&faces, &ws, wrap);
            assert_ne!(
                band(&want, &ws, 1),
                band(&blank, &ws, 1),
                "{plain}: the list's row is painted"
            );
            let got = last_frame(&faces, marked, wrap);
            let rows: Vec<usize> = (0..usize::from(ws.ws_row))
                .filter(|&y| band(&got, &ws, y) != band(&want, &ws, y))
                .collect();
            assert!(
                rows.is_empty(),
                "{marked}, wrap {wrap:?}: rows {rows:?} differ from {plain}'s"
            );
        }
    }
}
