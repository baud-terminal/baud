//! Replaying a recording through the core and painting it.
#![allow(dead_code)]

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_font::Px;
use baud_render::{Blend, Damage, Frame, PxSize, Renderer, Stack, Target, Theme};

pub const RECORDINGS: [&str; 9] = [
    "ascii/pipes.raw",
    "utf8/btop.raw",
    "ascii/cat-scroll.raw",
    "ascii/git-log.raw",
    "ascii/less-pager.raw",
    "utf8/cat-cjk.raw",
    "utf8/cava.raw",
    "utf8/less-cjk.raw",
    "utf8/vim-unicode.raw",
];

pub const CHUNK: usize = 4096;

/// **`theme` with a cursor of no colour of its own**: the cell's colours
/// swapped, the classic block, and a trail in the text's colour -- what a
/// test of the swap, or of a trail in the text's colour, draws under now
/// that the built-in themes' cursor is baud's amber.
pub fn classic(mut theme: Theme) -> Theme {
    theme.cursor = None;
    theme.cursor_text = None;
    theme.cursor_text_under = baud_domain::themefile::CursorText::UnderEveryCursor;
    theme.cursor_made = baud_render::CursorMade::Named;
    theme
}
pub const PX: f32 = 14.0;

pub fn recording(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../tools/tests/recordings/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

pub fn font(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../baud-font/tests/fonts/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

pub const WS: WinSize = WinSize {
    ws_row: 50,
    ws_col: 200,
};

/// A frame buffer sized for the renderer, painted through it.
pub struct Canvas {
    pub pixels: Vec<u32>,
    pub width: usize,
    pub height: usize,
}

impl Canvas {
    pub fn for_renderer(r: &Renderer<'_, '_>) -> Canvas {
        let (width, height) = r.pixel_size();
        let (width, height) = (
            usize::try_from(width.value()).unwrap(),
            usize::try_from(height.value()).unwrap(),
        );
        Canvas {
            pixels: vec![0; width * height],
            width,
            height,
        }
    }

    pub fn target(&mut self) -> Target<'_> {
        let (w, h) = (
            Px::of(u32::try_from(self.width).unwrap()),
            Px::of(u32::try_from(self.height).unwrap()),
        );
        Target::new(&mut self.pixels, w, w, h).unwrap()
    }

    pub fn digest(&self) -> u64 {
        self.pixels.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &p| {
            p.to_le_bytes().iter().fold(h, |h, &b| {
                (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
            })
        })
    }
}

/// The pixel rows `after` differs from `before` in, none outside a band
/// `bands` names: the rows that are not covered, empty when the bands
/// hold every change.
pub fn uncovered(before: &Canvas, after: &Canvas, bands: &[baud_render::Band]) -> Vec<Px> {
    let w = after.width;
    let row = |y: usize| Px::of(u32::try_from(y).unwrap());
    (0..after.height)
        .filter(|&y| before.pixels[y * w..(y + 1) * w] != after.pixels[y * w..(y + 1) * w])
        .map(row)
        .filter(|&y| {
            !bands
                .iter()
                .any(|b| b.top.value() <= y.value() && y.value() < b.bottom.value())
        })
        .collect()
}

pub fn renderer<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    let stack = Stack::new(faces).unwrap();
    Renderer::new(
        stack,
        PxSize::of(PX).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// Feeds `data` in `CHUNK` reads, calling `f` after each with the term.
pub fn replay(data: &[u8], mut f: impl FnMut(&mut Term<'_>, usize)) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut at = 0usize;
    let mut i = 0usize;
    while at < data.len() {
        let n = CHUNK.min(data.len() - at);
        t.write_at(BytePos::ZERO, &data[at..at + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        at += n;
        f(&mut t, i);
        i += 1;
    }
}

pub fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, c: &mut Canvas, damage: Damage) -> Frame {
    let mut target = c.target();
    let seal = t.seal_writes();
    r.render(t, seal, &mut target, damage)
}

/// Feeds all of `data`, then calls `f` once with the term.
pub fn replay_final(data: &[u8], f: impl FnOnce(&mut Term<'_>)) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut at = 0usize;
    while at < data.len() {
        let n = CHUNK.min(data.len() - at);
        t.write_at(BytePos::ZERO, &data[at..at + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        at += n;
    }
    f(&mut t)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `data` in base64, padded.
pub fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            match k <= q.len() {
                true => out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => out.push(b'='),
            }
        }
    }
    out
}

/// A kitty graphics command, `ctrl` and `payload` in base64, in chunks of
/// 4096 encoded bytes (`m=1` on every chunk but the last).
pub fn kitty(ctrl: &str, payload: &[u8]) -> Vec<u8> {
    let data = b64(payload);
    let mut v = Vec::new();
    let chunks: Vec<&[u8]> = match data.is_empty() {
        true => vec![&[][..]],
        false => data.chunks(4096).collect(),
    };
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        v.extend_from_slice(b"\x1b_G");
        match i {
            0 => v.extend_from_slice(format!("{ctrl},m={more}").as_bytes()),
            _ => v.extend_from_slice(format!("m={more}").as_bytes()),
        }
        if !chunk.is_empty() {
            v.push(b';');
            v.extend_from_slice(chunk);
        }
        v.extend_from_slice(b"\x1b\\");
    }
    v
}

/// `bytes` fed to the terminal.
pub fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}
