//! The light theme's sixteen are Modus Operandi (note Y, decision 3), but
//! entry 7, darkened to `#6c6c6c` on 2026-10-18.
//!
//! The values are held here as constants rather than read from the Modus
//! `.el` files, which are GPL and pinned `external`. What the change is for is
//! legibility on the light page: xterm's sixteen are designed for a black
//! page, and on `0xEF` GNU `ls`'s executables (bold green, xterm `#00ff00`)
//! read at 1.19:1 and its links (bold cyan) at 1.09:1.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, ReadLen, Row, WinSize};
use baud_render::{Argb, MODUS_OPERANDI, Theme, Video, dark_palette, light_palette, xterm_palette};

fn lin(c: Chan) -> f64 {
    let v = f64::from(c.get()) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn lum(c: Argb) -> f64 {
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

fn contrast(a: Argb, b: Argb) -> f64 {
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn hex(c: Argb) -> u32 {
    (u32::from(c.r().get()) << 16) | (u32::from(c.g().get()) << 8) | u32::from(c.b().get())
}

/// Decision 3's values: Modus Operandi's own.
const DECIDED: [u32; 16] = [
    0x000000, 0xa60000, 0x006800, 0x6f5500, 0x0031a9, 0x721045, 0x005e8b, 0xa6a6a6, 0x595959,
    0x972500, 0x00663f, 0x884900, 0x3548cf, 0x531ab6, 0x005f5f, 0xffffff,
];

/// The one change to Operandi, 2026-10-18: entry 7 at 4.57:1 on the
/// page, where Operandi's `#a6a6a6` reads at 2.12.
const ENTRY_7: u32 = 0x6c6c6c;

/// Every entry is Operandi's but 7, and [`MODUS_OPERANDI`] itself stays
/// Operandi's whole, the values its credit names.
#[test]
fn the_light_sixteen_are_the_decided_values() {
    let light = Theme::light();
    for (i, want) in DECIDED.iter().enumerate() {
        let drawn = if i == 7 { ENTRY_7 } else { *want };
        assert_eq!(hex(light.palette[i]), drawn, "entry {i}");
        let (r, g, b) = MODUS_OPERANDI[i];
        assert_eq!(
            (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
            *want
        );
    }
}

#[test]
fn every_colour_entry_reads_at_six_to_one_on_the_light_page() {
    let light = Theme::light();
    for i in (1..=6).chain(9..=14) {
        let c = contrast(light.palette[i], light.bg);
        assert!(c >= 6.0, "entry {i} is {c:.2}:1");
    }
}

/// Only the sixteen change: the cube and the ramp stay xterm's, the dark
/// theme is xterm's but for the four corrected on 2026-10-18, and
/// xterm's table itself is unmoved.
#[test]
fn nothing_but_the_light_sixteen_moves() {
    let x = xterm_palette();
    assert_eq!(hex(x[1]), 0xcd0000, "xterm's own red");
    assert_eq!(hex(x[15]), 0xffffff);
    let l = light_palette();
    for (i, (light, xterm)) in l.iter().zip(&x).enumerate().skip(16) {
        assert_eq!(hex(*light), hex(*xterm), "entry {i}");
    }
    let d = Theme::dark();
    assert_eq!(d.palette, dark_palette());
    let moved: Vec<usize> = (0..256).filter(|&i| d.palette[i] != x[i]).collect();
    assert_eq!(
        moved,
        [1, 4, 5, 12],
        "the dark entries that are not xterm's"
    );
}

/// `ls`, `lsd` and `bat --theme=ansi` captured through a pty
/// (`tools/tests/palette/ls-lsd-bat.raw`), replayed, every coloured glyph resolved
/// under the light theme: each reads at 6:1 or better. The same replay under
/// xterm's sixteen on the same page does not, which is what the change fixes.
#[test]
fn captured_listings_read_at_six_to_one_under_the_light_theme() {
    let rec = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/tests/palette/ls-lsd-bat.raw"
    ))
    .expect("the capture");
    let ws = WinSize {
        ws_row: 50,
        ws_col: 200,
    };
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, &rec);
    t.feed(BytePos::ZERO, ReadLen::from_index(rec.len()));
    let worst = |theme: &Theme| {
        let mut n = 0;
        let mut worst = f64::MAX;
        for y in 0..ws.ws_row {
            for cell in t
                .row(Row::from_index(usize::from(y)))
                .expect("a row on the grid")
            {
                let style = t.styles().one().word_of(*cell);
                if cell.content.is_empty() || style.fg().is_default() {
                    continue;
                }
                let (fg, bg) = theme.resolve(style, Video::Normal, |_| None, |_| None);
                worst = worst.min(contrast(fg, bg));
                n += 1;
            }
        }
        (n, worst)
    };
    let (n, w) = worst(&Theme::light());
    // `dir`, `link` and `run.sh` from `ls` and again from `lsd`, thirteen
    // each, and bat's `fn`, `main`, `"`, `hi`, `"`: ten.
    assert_eq!(n, 36, "the replay drew every coloured glyph of the capture");
    assert!(w >= 6.0, "the worst coloured glyph reads at {w:.2}:1");
    let mut xterm_light = Theme::light();
    xterm_light.palette = xterm_palette();
    let (_, w) = worst(&xterm_light);
    assert!(
        w < 1.3,
        "the control: xterm's sixteen on the light page read at {w:.2}:1"
    );
}
