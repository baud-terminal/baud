//! Scratch: renders the italic Hebrew options to raw RGBA for review.
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Target, Theme};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // italic_sheet <out.rgba> <px> <label> <face>...
    let out = &args[1];
    let px: f32 = args[2].parse().unwrap();
    let label = &args[3];
    let files: Vec<Vec<u8>> = args[4..]
        .iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect();
    let faces: Vec<Face<'_>> = files.iter().map(|d| Face::parse(d).unwrap()).collect();
    let cols: u16 = std::env::var("COLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let rows: u16 = std::env::var("ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let ws = WinSize {
        ws_row: rows,
        ws_col: cols,
    };
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(px).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    let (w, h) = r.pixel_size();
    let (wu, hu) = (
        usize::try_from(w.value()).unwrap(),
        usize::try_from(h.value()).unwrap(),
    );
    let mut pixels = vec![0u32; wu * hu];
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let text = format!(
        "{label}\r\n\u{5d1}\u{5bc}\u{5b0}\u{5e8}\u{5b5}\u{5d0}\u{5e9}\u{5c1}\u{5b4}\u{5d9}\u{5ea} \u{5d1}\u{5bc}\u{5b8}\u{5e8}\u{5b8}\u{5d0} \u{5d0}\u{5b1}\u{5dc}\u{5b9}\u{5d4}\u{5b4}\u{5d9}\u{5dd} \u{5d0}\u{5b5}\u{5ea} \u{5d4}\u{5b7}\u{5e9}\u{5bc}\u{5c1}\u{5b8}\u{5de}\u{5b7}\u{5d9}\u{5b4}\u{5dd}\r\n\
git commit -m \"\u{5ea}\u{5d9}\u{5e7}\u{5d5}\u{5df} \u{5d1}\u{5d0}\u{5d2}\"  v1.2 \u{5d1}\u{5bc}\u{5b0}\u{5e8}\u{5b5}\u{5d0}\u{5e9}\u{5c1}\u{5b4}\u{5d9}\u{5ea}\r\n\
\u{5d0}\u{5d1}\u{5d2}\u{5d3}\u{5d4}\u{5d5}\u{5d6}\u{5d7}\u{5d8}\u{5d9}\u{5da}\u{5db}\u{5dc}\u{5dd}\u{5de}\u{5df}\u{5e0}\u{5e1}\u{5e2}\u{5e3}\u{5e4}\u{5e5}\u{5e6}\u{5e7}\u{5e8}\u{5e9}\u{5ea} abc 0123\r\n\
\u{5e9}\u{5b8}\u{5c1} \u{5e9}\u{5b8}\u{5c2} \u{5d1}\u{5b8}\u{5bd} \u{5dc}\u{5b9} \u{5d5}\u{5b9} \u{5d0}\u{5b2} \u{5e7}\u{5bb} \u{5d8}\u{5b6} Italic Latin"
    );
    t.write_at(BytePos::ZERO, text.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
    let wp = Px::of(u32::try_from(wu).unwrap());
    let hp = Px::of(u32::try_from(hu).unwrap());
    let mut target = Target::new(&mut pixels, wp, wp, hp).unwrap();
    r.render(&mut t, &mut target, Damage::Full);
    let mut bytes = Vec::with_capacity(wu * hu * 4 + 8);
    bytes.extend((wu as u32).to_le_bytes());
    bytes.extend((hu as u32).to_le_bytes());
    for p in pixels {
        bytes.extend(p.to_le_bytes());
    }
    std::fs::write(out, bytes).unwrap();
}
