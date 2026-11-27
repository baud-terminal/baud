//! **The organisation picture's two lights, drawn by baud's CPU renderer.**
//!
//! `baud-pictures brand` (`tools/baud-pictures`) lays two of baud's own plasma lights
//! over the organisation picture's drawing (`docs/assets/logo/baud-github-org.svg`)
//! and takes them from here, each drawn alone on the brand ground in the
//! amber, so its light is the pixel's share of the way to the amber:
//!
//! * **The word's**: the launch splash's first pass [`WORD_T`] seconds in,
//!   0.1 ms before the second lights ([`splash::OUT`]), its seed
//!   [`splash::SEED`], with its box ending where the word does, at
//!   [`splash::SEAM`]'s first column ([`splash::pass_over`]). The generator
//!   gives the scale and origin that take the lockup's word onto the
//!   picture's.
//! * **The mark's**: the cursor's plasma trail [`MARK_AGE`] seconds into a
//!   move from the ❯'s cell into the next, seed [`MARK_SEED`], at
//!   [`MARK_PX`] px in JetBrains Mono Bold (baud's built face), the ❯ and a
//!   steady block in the cells it moves between; and the same frame
//!   without the trail, which the light is taken against.
//!
//! usage: `cargo run --release -p baud-render --example brand_plasma --
//! <out dir> <scale> <x> <y>`
//!
//! Writes `word.raw`, `mark-base.raw` and `mark-trail.raw` (little-endian
//! `0xAARRGGBB`, row-major) and `plasma.json` (the cursor's amber on the
//! dark and the light theme's page, [`BaudCursor::dark`] and
//! [`BaudCursor::light`], which the generator holds to the brand's; each canvas's
//! size, the cell metrics, the seeds and the times, and the splash's word
//! pen and block, which the generator holds to the lockup's) into
//! `<out dir>`. Before it
//! writes, it holds the word's light to the splash's own: left of the seam
//! less half the block's height (the plasma's unit), the word's frame is
//! the frame of [`splash::vfx`] at the same moment, pixel for pixel, and it
//! stops if one differs.

use baud_core::{Owned, Term};
use baud_domain::themefile::CellPaint;
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{
    Argb, BaudCursor, Blend, Blink, Damage, FaceRole, FaceStyle, Pt, PxSize, Rect, Renderer,
    Segment, Shape, Smear, Stack, Strength, Target, Theme, TrailLook, splash,
};

const GROUND: Argb = Argb(0xFF15_171B);
const AMBER: Argb = Argb(0xFFF4_A32C);

/// 0.1 ms before [`splash::OUT`], where the splash's own test holds the
/// logo whole (`block(at(OUT - 1e-4)) > 0.98`).
const WORD_T: f32 = 0.0999;
/// The word's canvas: 120 by 40 cells at 28 px, 2040 by 1480 pixels.
const WORD_PX: f32 = 28.0;
const WORD_WS: WinSize = WinSize {
    ws_row: 40,
    ws_col: 120,
};

const MARK_PX: f32 = 200.0;
const MARK_AGE: f32 = 0.12;
const MARK_SEED: Seed = Seed(3);
/// The ❯ in row 2, column 2, so a cell of margin on every side.
const MARK_WS: WinSize = WinSize {
    ws_row: 3,
    ws_col: 5,
};
const MARK_AT: (usize, usize) = (1, 1);

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// One full frame of `bytes` on a fresh terminal of `ws`.
fn frame(r: &mut Renderer<'_, '_>, ws: &WinSize, bytes: &[u8]) -> Vec<u32> {
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    feed(&mut t, bytes);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).expect("a canvas fits in memory");
    let mut px = vec![0u32; n];
    let mut target = Target::new(&mut px, w, w, h).expect("a target");
    let seal = t.seal_writes();
    r.render(&t, seal, &mut target, Damage::Full);
    px
}

fn write_raw(path: &str, px: &[u32]) {
    let bytes: Vec<u8> = px.iter().flat_map(|p| p.to_le_bytes()).collect();
    std::fs::write(path, bytes).expect("write a frame");
}

fn renderer<'f>(stack: Stack<'f, 'f>, px: f32, theme: Theme, ws: &WinSize) -> Renderer<'f, 'f> {
    Renderer::new(
        stack,
        PxSize::of(px).expect("a size"),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .expect("a renderer")
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [_, out, scale, x, y] = a.as_slice() else {
        panic!("usage: brand_plasma <out dir> <scale> <x> <y>");
    };
    let num = |s: &String| -> f32 { s.parse().expect("a number") };
    let (scale, at) = (
        num(scale),
        Pt {
            x: num(x),
            y: num(y),
        },
    );
    let fonts = concat!(env!("CARGO_MANIFEST_DIR"), "/../baud-app/fonts/built/");
    let regular = std::fs::read(format!("{fonts}Regular.ttf")).expect("Regular.ttf");
    let bold = std::fs::read(format!("{fonts}Bold.ttf")).expect("Bold.ttf");

    // The word's light: the trail is drawn in the cursor's fill, here the
    // amber, on an empty screen with the cursor hidden.
    let faces = [
        Face::parse(&regular).expect("parse"),
        Face::parse(&bold).expect("parse"),
    ];
    let mut theme = Theme::dark();
    theme.bg = GROUND;
    theme.fg = AMBER;
    theme.cursor = Some(CellPaint::Colour(AMBER));
    theme.cursor_made = baud_render::CursorMade::Named;
    let stack = Stack::with_roles(
        &faces,
        &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
    )
    .expect("a stack");
    let mut r = renderer(stack, WORD_PX, theme, &WORD_WS);
    let word = Rect {
        left: splash::WORDMARK.x,
        top: splash::BLOCK.top,
        right: splash::SEAM.0,
        bottom: splash::BLOCK.bottom,
    };
    let lit = |r: &mut Renderer<'_, '_>, v: Vfx| {
        r.set_trail(Some(TrailLook::Vfx(v)));
        let px = frame(r, &WORD_WS, b"\x1b[?25l");
        r.set_trail(None);
        px
    };
    let light = lit(
        &mut r,
        splash::pass_over(word, at, scale, splash::SEED, WORD_T),
    );
    let whole = lit(&mut r, splash::vfx(at, scale, WORD_T));
    let (w, h) = r.pixel_size();
    let (ww, wh) = (
        usize::try_from(w.value()).expect("a width"),
        usize::try_from(h.value()).expect("a height"),
    );
    // The gate: left of the seam less half the block's height, which is
    // the plasma's unit, the splash's own light.
    let half = 0.5 * (splash::BLOCK.bottom - splash::BLOCK.top);
    let cols = (at.x + (splash::SEAM.0 - half) * scale)
        .floor()
        .clamp(0.0, w.value() as f32);
    let cols = usize::try_from(cols as i64).unwrap_or(0);
    let differing = (0..wh)
        .flat_map(|row| (0..cols).map(move |col| row * ww + col))
        .filter(|&i| light[i] != whole[i])
        .count();
    assert_eq!(
        differing, 0,
        "the word's light is not the splash's left of the seam"
    );
    assert!(
        light.iter().zip(&whole).any(|(a, b)| a != b),
        "the control: the word's light differs from the splash's somewhere"
    );
    write_raw(&format!("{out}/word.raw"), &light);

    // The mark's light: the ❯ and a steady block cursor after it, and the
    // trail of a move from the ❯'s cell into the cursor's.
    let faces = [Face::parse(&bold).expect("parse")];
    let mut theme = Theme::dark();
    theme.fg = AMBER;
    theme.bg = GROUND;
    let mut r = renderer(
        Stack::new(&faces).expect("a stack"),
        MARK_PX,
        theme,
        &MARK_WS,
    );
    let m = r.metrics();
    let (cw, ch) = (m.cell_w.value() as f32, m.cell_h.value() as f32);
    let cell = |c: usize| Rect {
        left: c as f32 * cw,
        top: MARK_AT.0 as f32 * ch,
        right: c as f32 * cw + cw,
        bottom: MARK_AT.0 as f32 * ch + ch,
    };
    let (from, to) = (cell(MARK_AT.1), cell(MARK_AT.1 + 1));
    let tail = Pt {
        x: (from.left + from.right) / 2.0,
        y: (from.top + from.bottom) / 2.0,
    };
    let ig = Ignition::ignite(tail, to, MARK_SEED, ch);
    let mark = format!("\x1b[2 q\x1b[{};{}H\u{276F}", MARK_AT.0 + 1, MARK_AT.1 + 1);
    r.set_cursor_blink(Blink::Shown);
    let base = frame(&mut r, &MARK_WS, mark.as_bytes());
    r.set_trail(Some(TrailLook::Vfx(Vfx {
        effect: Effect::Plasma,
        emitter: Smear {
            corners: Segment::new(from.corners(), to).at(MARK_AGE),
            hole: to,
            shape: Shape::Quad,
            strength: Strength::Full,
            opacity: Smear::OPAQUE,
        },
        ignition: ig.at(MARK_AGE),
        axis: ig.dir,
    })));
    let trail = frame(&mut r, &MARK_WS, mark.as_bytes());
    write_raw(&format!("{out}/mark-base.raw"), &base);
    write_raw(&format!("{out}/mark-trail.raw"), &trail);
    let (mw, mh) = r.pixel_size();

    // The cursor's amber on each theme's page, which the generator holds
    // to the brand's, the amber the logo files take.
    let hex = |c: Argb| format!("#{:06X}", c.0 & 0xFF_FFFF);
    let (on_dark, on_light) = (hex(BaudCursor::dark().fill), hex(BaudCursor::light().fill));
    let json = format!(
        "{{\"amber\": {{\"dark\": \"{on_dark}\", \"light\": \"{on_light}\"}},\n \
         \"word\": {{\"width\": {}, \"height\": {}, \"px\": {WORD_PX}, \"seed\": \"{:08X}\", \
         \"t\": {WORD_T}, \"pen\": [{}, {}], \"box\": [{}, {}, {}, {}], \"hole\": [{}, {}, {}, {}], \
         \"scale\": {scale}, \"origin\": [{}, {}], \"gate_columns\": {cols}, \"gate_differing\": {differing}}},\n \
         \"mark\": {{\"width\": {}, \"height\": {}, \"px\": {MARK_PX}, \"cell_w\": {}, \"cell_h\": {}, \
         \"baseline\": {}, \"row\": {}, \"col\": {}, \"seed\": {}, \"age\": {MARK_AGE}}}}}\n",
        w.value(),
        h.value(),
        splash::SEED.0,
        splash::WORDMARK.x,
        splash::WORDMARK.baseline,
        word.left,
        word.top,
        word.right,
        word.bottom,
        splash::BLOCK.left,
        splash::BLOCK.top,
        splash::BLOCK.right,
        splash::BLOCK.bottom,
        at.x,
        at.y,
        mw.value(),
        mh.value(),
        m.cell_w.value(),
        m.cell_h.value(),
        m.baseline.value(),
        MARK_AT.0,
        MARK_AT.1,
        MARK_SEED.0,
    );
    std::fs::write(format!("{out}/plasma.json"), json).expect("write plasma.json");
}
