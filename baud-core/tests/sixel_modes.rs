//! The Sixel modes `?80` (DECSDM), `?1070` and `?8452`, XTSMGRAPHICS, and the
//! colour an undefined Sixel register paints with (note V, V18: "built to
//! the spec (display mode and shared registers, not answered as fixed)").
//!
//! Every program here is fed whole, in reads of every size from one byte to
//! its length, and as two reads split at every byte, and each feeding must
//! leave what the assertion names: a mode set by one read and acted on in
//! the next is the case a chunk-invariant terminal has to get right.

use baud_core::*;
use baud_domain::{Chan, ImageBudget, ImagePx, ImageRefusals, LineIdx, Pixel, SixelClaim};

mod support;

/// How a terminal is made before the program is fed.
#[derive(Clone, Copy)]
struct Setup {
    claim: SixelClaim,
    provisioned: Provisioned,
    /// The cell's width and height in pixels, or unset.
    cell: Option<(u16, u16)>,
    /// The embedder's default foreground, or unsaid.
    fg: Option<(u8, u8, u8)>,
    /// The image arena, when provisioned.
    arena: Arena,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Provisioned {
    Yes,
    No,
}

/// How many pixels the image arena holds.
#[derive(Clone, Copy)]
enum Arena {
    /// 65,536.
    Plenty,
    /// 16,384: a screen of 200 by 50 or 200 by 72 pixels fits, 300 columns
    /// of either do not.
    Tight,
    /// 16 Mi.
    Large,
}

impl Arena {
    fn budget(self) -> ImageBudget {
        match self {
            Arena::Plenty => ImageBudget::of(1 << 16),
            Arena::Tight => ImageBudget::of(1 << 14),
            Arena::Large => ImageBudget::of(16 << 20),
        }
    }
}

/// Claimed and provisioned, an 8x16 cell, and a foreground the embedder
/// has said: DA lists Sixel, so the modes are acted on.
const CLAIMED: Setup = Setup {
    claim: SixelClaim::Yes,
    provisioned: Provisioned::Yes,
    cell: Some((8, 16)),
    fg: Some((10, 20, 30)),
    arena: Arena::Plenty,
};

const FG: (u8, u8, u8) = (10, 20, 30);

fn rgb((r, g, b): (u8, u8, u8)) -> Pixel {
    Pixel::opaque(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

fn red() -> Pixel {
    rgb((255, 0, 0))
}

/// Run `f` on a terminal made as `set` says and fed `pieces`, one read each.
fn fed(set: Setup, pieces: &[&[u8]], f: &dyn Fn(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = match set.provisioned {
        Provisioned::Yes => Owned::new(&ws).with_images(set.arena.budget()),
        Provisioned::No => Owned::new(&ws),
    };
    let mut t = Term::new(owned.storage(), &ws);
    t.set_sixel_claim(set.claim);
    if let Some((w, h)) = set.cell {
        t.set_cell_size(ImagePx::of(w), ImagePx::of(h));
    }
    if let Some(fg) = set.fg {
        t.set_default_colors(fg, (0, 0, 0));
    }
    for p in pieces {
        t.write_at(BytePos::ZERO, p);
        t.feed(BytePos::ZERO, ReadLen::from_index(p.len()));
    }
    f(&mut t);
}

/// `bytes` whole, in reads of every size, and split once at every byte.
fn every_read(set: Setup, bytes: &[u8], f: impl Fn(&mut Term<'_>, &str)) {
    fed(set, &[bytes], &|t| f(t, "whole"));
    for size in 1..bytes.len() {
        let pieces: Vec<&[u8]> = bytes.chunks(size).collect();
        fed(set, &pieces, &|t| f(t, &format!("reads of {size}")));
    }
    for at in 1..bytes.len() {
        let (a, b) = bytes.split_at(at);
        fed(set, &[a, b], &|t| f(t, &format!("split at {at}")));
    }
}

/// The one placement's line, column and image size, and its pixels.
fn placed(t: &Term<'_>) -> (LineIdx, Col, u16, u16, Vec<Pixel>) {
    let imgs = t.images();
    let mut all = imgs.placements();
    let (_, p) = all.next().expect("an image is placed");
    assert!(all.next().is_none(), "one image is placed");
    let v = imgs.image(p.image).expect("the placed image is live");
    (
        p.line,
        p.col,
        v.width.raw(),
        v.height.raw(),
        v.pixels.to_vec(),
    )
}

/// Every image's pixel at `(x, y)`, in placement order.
fn pixels_at(t: &Term<'_>, x: usize, y: usize) -> Vec<Pixel> {
    let imgs = t.images();
    imgs.placements()
        .map(|(_, p)| {
            let v = imgs.image(p.image).expect("live");
            v.pixels[y * v.width.index() + x]
        })
        .collect()
}

fn at(y: usize, x: usize) -> (Row, Col) {
    (Row::from_index(y), Col::at_index(x))
}

fn top(t: &Term<'_>) -> LineIdx {
    t.image_origin(Rows::ZERO).top
}

/// A 16x6 red Sixel at 1:1: two columns and one row of an 8x16 cell.
const RED: &[u8] = b"\x1bP0;1;0q\"1;1#1;2;100;0;0#1!16~\x1b\\";

/// **DECRQM answers each Sixel mode from its state while DA lists Sixel**:
/// `?80` and `?8452` reset and `?1070` set at power-on, and each the other
/// way once set or reset. Before, each answered 0, "not recognized".
#[test]
fn decrqm_answers_each_sixel_mode_from_its_state() {
    every_read(CLAIMED, b"\x1b[?80$p\x1b[?1070$p\x1b[?8452$p", |t, how| {
        assert_eq!(
            t.reply(),
            b"\x1b[?80;2$y\x1b[?1070;1$y\x1b[?8452;2$y",
            "{how}: power-on"
        );
    });
    every_read(
        CLAIMED,
        b"\x1b[?80h\x1b[?1070l\x1b[?8452h\x1b[?80$p\x1b[?1070$p\x1b[?8452$p",
        |t, how| {
            assert_eq!(
                t.reply(),
                b"\x1b[?80;1$y\x1b[?1070;2$y\x1b[?8452;1$y",
                "{how}: each flipped"
            );
            assert_eq!(t.unsupported_param, 0, "{how}: nothing counted");
        },
    );
}

/// **While DA does not list Sixel, the modes are not recognized**: claimed
/// but unprovisioned, or provisioned but unclaimed, a request is counted and
/// not kept, DECRQM answers 0, and a Sixel drawn anyway draws as at
/// power-on. `[[source: ctlseqs]]` puts the modes, DA's `4` and the `DCS q`
/// under one configuration.
#[test]
fn while_da_does_not_list_sixel_the_modes_are_counted_and_not_kept() {
    let unclaimed = Setup {
        claim: SixelClaim::No,
        ..CLAIMED
    };
    let unprovisioned = Setup {
        provisioned: Provisioned::No,
        ..CLAIMED
    };
    for (name, set) in [("unclaimed", unclaimed), ("unprovisioned", unprovisioned)] {
        let program = b"\x1b[?80h\x1b[?1070l\x1b[?8452h\x1b[?80$p\x1b[?1070$p\x1b[?8452$p";
        every_read(set, program, |t, how| {
            assert_eq!(
                t.reply(),
                b"\x1b[?80;0$y\x1b[?1070;0$y\x1b[?8452;0$y",
                "{name}, {how}"
            );
            assert_eq!(t.unsupported_param, 3, "{name}, {how}: three counted");
        });
    }
    let mut program = Vec::from(b"\x1b[?80h\x1b[?8452h\x1b[3;5H".as_slice());
    program.extend_from_slice(RED);
    every_read(unclaimed, &program, |t, how| {
        let (line, col, ..) = placed(t);
        assert_eq!(
            line,
            top(t).plus(Rows::of_count(2)),
            "{how}: at the cursor's row"
        );
        assert_eq!(col, Col::at_index(4), "{how}: and column");
        assert_eq!(t.cursor(), at(3, 4), "{how}: one row down, the column kept");
    });
}

/// **Withdrawing the claim returns the modes to power-on**, so no mode is
/// kept that DECRQM would then deny.
#[test]
fn withdrawing_the_claim_returns_the_modes_to_power_on() {
    fed(CLAIMED, &[b"\x1b[?80h\x1b[?1070l\x1b[?8452h"], &|t| {
        t.set_sixel_claim(SixelClaim::No);
        t.set_sixel_claim(SixelClaim::Yes);
        support::feed(t, b"\x1b[?80$p\x1b[?1070$p\x1b[?8452$p");
        assert_eq!(t.reply(), b"\x1b[?80;2$y\x1b[?1070;1$y\x1b[?8452;2$y");
    });
}

/// **`RIS` restores the three and `DECSTR` keeps them**, as xterm's full
/// reset restores them and its soft reset does not
/// (`[[source: xterm-reset]]`); XTSAVE and XTRESTORE carry them.
#[test]
fn ris_restores_the_sixel_modes_and_decstr_keeps_them() {
    let flip = b"\x1b[?80h\x1b[?1070l\x1b[?8452h";
    let ask = b"\x1b[?80$p\x1b[?1070$p\x1b[?8452$p";
    every_read(
        CLAIMED,
        &[flip.as_slice(), b"\x1bc", ask].concat(),
        |t, how| {
            assert_eq!(
                t.reply(),
                b"\x1b[?80;2$y\x1b[?1070;1$y\x1b[?8452;2$y",
                "{how}: RIS"
            );
        },
    );
    every_read(
        CLAIMED,
        &[flip.as_slice(), b"\x1b[!p", ask].concat(),
        |t, how| {
            assert_eq!(
                t.reply(),
                b"\x1b[?80;1$y\x1b[?1070;2$y\x1b[?8452;1$y",
                "{how}: DECSTR"
            );
        },
    );
    let save = b"\x1b[?80;1070;8452s\x1b[?80l\x1b[?1070h\x1b[?8452l\x1b[?80;1070;8452r";
    every_read(CLAIMED, &[flip.as_slice(), save, ask].concat(), |t, how| {
        assert_eq!(
            t.reply(),
            b"\x1b[?80;1$y\x1b[?1070;2$y\x1b[?8452;1$y",
            "{how}: XTRESTORE"
        );
    });
}

/// **`CSI 80 h` is ANSI mode 80, not DECSDM**, which timg sends: counted, and
/// the Sixel after it draws at the cursor.
#[test]
fn ansi_mode_80_is_not_decsdm() {
    let mut program = Vec::from(b"\x1b[80h\x1b[3;5H".as_slice());
    program.extend_from_slice(RED);
    program.extend_from_slice(b"\x1b[?80$p");
    every_read(CLAIMED, &program, |t, how| {
        assert_eq!(t.reply(), b"\x1b[?80;2$y", "{how}: DECSDM still reset");
        assert_eq!(placed(t).1, Col::at_index(4), "{how}: drawn at the cursor");
    });
}

/// **Display mode puts the image at the window's upper-left cell and leaves
/// the cursor where it was**: `[[source: vt3xx-sixel]]`, "the sixel active
/// position begins at the upper-left corner of the active graphics page",
/// and "the text cursor does not change". The window has scrolled first,
/// so its top line is not line 0, and the cursor is away from the corner.
#[test]
fn display_mode_draws_at_the_windows_corner_and_leaves_the_cursor() {
    let mut program = vec![b'\n'; 60];
    program.extend_from_slice(b"\x1b[?80h\x1b[10;5H");
    program.extend_from_slice(RED);
    every_read(CLAIMED, &program, |t, how| {
        let (line, col, w, h, _) = placed(t);
        assert_ne!(top(t), LineIdx::FIRST, "{how}: the window has scrolled");
        assert_eq!(line, top(t), "{how}: the window's top line");
        assert_eq!(col, Col::ZERO, "{how}: its first column");
        assert_eq!((w, h), (16, 6), "{how}: the whole image");
        assert_eq!(t.cursor(), at(9, 4), "{how}: the cursor is unmoved");
    });
    // At the bottom row, nothing scrolls either.
    let mut program = Vec::from(b"\x1b[?80h\x1b[50;1H".as_slice());
    program.extend_from_slice(RED);
    every_read(CLAIMED, &program, |t, how| {
        assert_eq!(top(t), LineIdx::FIRST, "{how}: nothing scrolled");
        assert_eq!(t.cursor(), at(49, 0), "{how}: the cursor is unmoved");
    });
}

/// **In display mode a graphics new line past the screen's bottom edge is
/// ignored**: `[[source: vt3xx-sixel]]`, "The terminal ignores any commands
/// that attempt to advance the active position below the bottom margin of
/// the graphics page". With a 1-pixel cell the 50-row screen is 50 pixels
/// tall: bands start at 0, 6 ... 48, the `-` to 54 moves nothing, so the
/// tenth band's sixel lands beside the ninth's, at column 1 of band 48, and
/// the rows past 50 are not kept. With scrolling on, the same body is ten
/// bands and 60 rows, one column wide.
#[test]
fn display_mode_ignores_a_new_line_past_the_bottom_edge() {
    let one_px = Setup {
        cell: Some((8, 1)),
        ..CLAIMED
    };
    let body = b"\x1bP0;1;0q\"1;1#1;2;100;0;0#1~-~-~-~-~-~-~-~-~-~\x1b\\";
    every_read(
        one_px,
        &[b"\x1b[?80h".as_slice(), body].concat(),
        |t, how| {
            let (_, _, w, h, px) = placed(t);
            assert_eq!((w, h), (2, 50), "{how}: two columns, cut at the edge");
            assert_eq!(
                px[48 * 2 + 1],
                red(),
                "{how}: the tenth band beside the ninth"
            );
            assert_eq!(px[49 * 2 + 1], red(), "{how}: down to the edge");
        },
    );
    every_read(one_px, body, |t, how| {
        let (_, _, w, h, _) = placed(t);
        assert_eq!((w, h), (1, 60), "{how}: scrolling keeps every band");
    });
}

/// A Sixel `w` pixels wide and `bands` six-pixel bands tall, solid red.
fn tall(w: usize, bands: usize) -> Vec<u8> {
    let mut v = Vec::from(b"\x1bP0;1;0q\"1;1#1;2;100;0;0".as_slice());
    for i in 0..bands {
        if i > 0 {
            v.push(b'-');
        }
        v.extend_from_slice(format!("#1!{w}~").as_bytes());
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

/// **In display mode no column past the screen's right edge is drawn**:
/// `[[source: dec-std-070]]` 070-9 §11.2.2, "Right Margin: Sixels defined to
/// be printed past the right margin are not printed". With a 1-pixel cell
/// the screen is 200 by 50 pixels. A 40x120 image keeps nine bands, the
/// eleven whose new lines are ignored land side by side along the ninth from
/// column 40, and past column 200 nothing is kept: the image is the screen,
/// 10,000 pixels, where 480 columns would be 24,000 and the arena holds
/// 16,384. Before, it was refused whole.
#[test]
fn display_mode_draws_nothing_past_the_right_edge() {
    let small = Setup {
        cell: Some((1, 1)),
        arena: Arena::Tight,
        ..CLAIMED
    };
    let body = [b"\x1b[?80h".as_slice(), &tall(40, 20)].concat();
    every_read(small, &body, |t, how| {
        let (_, _, w, h, px) = placed(t);
        assert_eq!((w, h), (200, 50), "{how}: the screen, no wider");
        assert_eq!(px[49 * 200 + 199], red(), "{how}: drawn to the edge");
        assert_eq!(px[40], Pixel::CLEAR, "{how}: only the last band piles");
        assert_eq!(
            t.images().refusals,
            ImageRefusals::NONE,
            "{how}: nothing refused"
        );
    });
}

/// **No row is laid out past the right edge either**, so the arena holds an
/// image the screen's size whatever its data asks for. On a 200 by 100 pixel
/// screen and a 16,384-pixel arena, a 72-row image is 14,400 pixels at the
/// screen's width and 21,600 at 300 columns. One body overstates `Ph` as
/// 1000 and draws to column 300 in its first band; the other doubles its
/// stride past the edge and returns with `$` to draw 280 columns. Each is
/// stored 200 by 72.
#[test]
fn display_mode_lays_no_row_out_past_the_right_edge() {
    let small = Setup {
        cell: Some((1, 2)),
        arena: Arena::Tight,
        ..CLAIMED
    };
    let more = "-#1!150~".repeat(11);
    let ph = format!("\x1b[?80h\x1bP0;1;0q\"1;1;1000;72#1;2;100;0;0#1!100~!100~!100~{more}\x1b\\");
    let doubled = format!("\x1b[?80h\x1bP0;1;0q\"1;1#1;2;100;0;0#1!150~!150~$!280~{more}\x1b\\");
    for (name, body) in [("Ph", ph), ("doubled", doubled)] {
        every_read(small, body.as_bytes(), |t, how| {
            let (_, _, w, h, px) = placed(t);
            assert_eq!((w, h), (200, 72), "{name}, {how}: the screen's width");
            assert_eq!(px[199], red(), "{name}, {how}: drawn to the edge");
            assert_eq!(t.images().refusals, ImageRefusals::NONE, "{name}, {how}");
        });
    }
}

/// **At a real screen's size**: a 400x1200 image on a 50x200 screen of 8x16
/// cells, 1600 by 800 pixels, was stored 26,800 wide and refused for the
/// arena. 134 bands fit, the rest pile along the last from column 400,
/// and the image is the screen. Fed whole and in reads of a few sizes: every
/// size is `display_mode_draws_nothing_past_the_right_edge`'s.
#[test]
fn display_mode_keeps_a_tall_image_at_the_screens_size() {
    let big = Setup {
        arena: Arena::Large,
        ..CLAIMED
    };
    let body = [b"\x1b[?80h".as_slice(), &tall(400, 200)].concat();
    for size in [body.len(), 1, 7, 64, 1000] {
        let pieces: Vec<&[u8]> = body.chunks(size).collect();
        fed(big, &pieces, &|t| {
            let (_, _, w, h, px) = placed(t);
            assert_eq!((w, h), (1600, 800), "reads of {size}: the screen");
            assert_eq!(px[799 * 1600 + 1599], red(), "reads of {size}: to the edge");
            assert_eq!(t.images().refusals, ImageRefusals::NONE, "reads of {size}");
        });
    }
}

/// **`?8452` puts the cursor to the right of the image** (`[[source:
/// ctlseqs]]`, "leaves cursor to right of graphic"), `ceil(16 / 8)` columns
/// past where it started, on the image's last row, as xterm's
/// `finished_parsing` does (`[[source: xterm-graphics-sixel]]`); past the
/// right margin it goes to the left margin of the next row, as xterm does,
/// and on the margin itself it stays.
#[test]
fn cursor_right_mode_leaves_the_cursor_past_the_image() {
    let mut program = Vec::from(b"\x1b[?8452h\x1b[3;11H".as_slice());
    program.extend_from_slice(RED);
    every_read(CLAIMED, &program, |t, how| {
        assert_eq!(placed(t).1, Col::at_index(10), "{how}: drawn at the cursor");
        assert_eq!(t.cursor(), at(2, 12), "{how}: two columns right, same row");
    });
    // 36 rows of a 16-row cell: rows 2, 3 and 4, and the cursor on 4.
    let mut program = Vec::from(b"\x1b[?8452h\x1b[3;11H".as_slice());
    program.extend_from_slice(&tall(16, 6));
    every_read(CLAIMED, &program, |t, how| {
        assert_eq!(t.cursor(), at(4, 12), "{how}: on the image's last row");
    });
    let mut program = Vec::from(b"\x1b[?8452h\x1b[3;199H".as_slice());
    program.extend_from_slice(RED);
    every_read(CLAIMED, &program, |t, how| {
        assert_eq!(t.cursor(), at(3, 0), "{how}: past the margin, the next row");
    });
    let mut program = Vec::from(b"\x1b[?8452h\x1b[3;198H".as_slice());
    program.extend_from_slice(RED);
    every_read(CLAIMED, &program, |t, how| {
        assert_eq!(t.cursor(), at(2, 199), "{how}: on the margin, it stays");
    });
    // Reset, the column stays where the image started.
    let mut program = Vec::from(b"\x1b[?8452h\x1b[?8452l\x1b[3;11H".as_slice());
    program.extend_from_slice(RED);
    every_read(CLAIMED, &program, |t, how| {
        assert_eq!(t.cursor(), at(3, 10), "{how}: reset keeps the column");
    });
}

/// **An undefined register paints the foreground shown**: `[[source:
/// dec-std-070]]` 070-9 §10, "Sixel color: Contrasting to background. Color
/// numbers: All unspecified color numbers image as the default Sixel
/// color". Register 7 is never defined; register 0, selected at entry, is
/// painted before any `#`. Under `?5` the foreground shown is the theme's
/// background. With no foreground said, the pixel is `DEFAULT`, nothing.
#[test]
fn an_undefined_register_paints_the_foreground_shown() {
    let body = b"\x1bP0;1;0q\"1;1~#7~\x1b\\";
    every_read(CLAIMED, body, |t, how| {
        let (_, _, _, _, px) = placed(t);
        assert_eq!(px[0], rgb(FG), "{how}: register 0 before any #");
        assert_eq!(px[1], rgb(FG), "{how}: register 7, never defined");
    });
    every_read(
        CLAIMED,
        &[b"\x1b[?5h".as_slice(), body].concat(),
        |t, how| {
            assert_eq!(
                placed(t).4[1],
                rgb((0, 0, 0)),
                "{how}: under ?5, the theme's background"
            );
        },
    );
    let unsaid = Setup {
        fg: None,
        ..CLAIMED
    };
    every_read(unsaid, body, |t, how| {
        assert_eq!(placed(t).4[1], Pixel::CLEAR, "{how}: no foreground said");
    });
}

/// **`?1070` reset carries a register into the next Sixel**; set, each Sixel
/// starts with none defined. Picture A defines register 5 red; picture B
/// paints with register 5 without defining it.
#[test]
fn shared_registers_carry_into_the_next_sixel_and_private_ones_do_not() {
    let a = b"\x1bP0;1;0q\"1;1#5;2;100;0;0#5~\x1b\\";
    let b = b"\x1bP0;1;0q\"1;1#5~\x1b\\\n";
    every_read(
        CLAIMED,
        &[b"\x1b[?1070l".as_slice(), a, b"\n", b].concat(),
        |t, how| {
            assert_eq!(pixels_at(t, 0, 0), [red(), red()], "{how}: shared");
        },
    );
    every_read(CLAIMED, &[a.as_slice(), b"\n", b].concat(), |t, how| {
        assert_eq!(pixels_at(t, 0, 0), [red(), rgb(FG)], "{how}: private");
    });
}

/// **Only a shared Sixel's definitions are shared**: one defined by a
/// private Sixel does not leak into the shared set, a shared definition
/// survives a private Sixel between, and `RIS` forgets them.
#[test]
fn a_private_sixel_neither_feeds_nor_clears_the_shared_set() {
    let define = |r: u8, rgb: &str| {
        [
            b"\x1bP0;1;0q\"1;1#".as_slice(),
            &[b'0' + r],
            b";2;",
            rgb.as_bytes(),
            b"#",
            &[b'0' + r],
            b"~\x1b\\\n",
        ]
        .concat()
    };
    let paint = |r: u8| [b"\x1bP0;1;0q\"1;1#".as_slice(), &[b'0' + r], b"~\x1b\\\n"].concat();
    let program = [
        define(5, "0;100;0"), // private: green in 5, kept by no one
        b"\x1b[?1070l".to_vec(),
        paint(5),             // shared, 5 undefined: the foreground
        define(6, "0;0;100"), // shared: blue in 6
        b"\x1b[?1070h".to_vec(),
        paint(6), // private: the foreground
        b"\x1b[?1070l".to_vec(),
        paint(6), // shared again: blue
    ]
    .concat();
    let blue = rgb((0, 0, 255));
    every_read(CLAIMED, &program, |t, how| {
        assert_eq!(
            pixels_at(t, 0, 0),
            [rgb((0, 255, 0)), rgb(FG), blue, rgb(FG), blue],
            "{how}"
        );
    });
    let reset = [
        b"\x1b[?1070l".to_vec(),
        define(6, "0;0;100"),
        b"\x1bc\x1b[?1070l".to_vec(),
        paint(6),
    ]
    .concat();
    every_read(CLAIMED, &reset, |t, how| {
        assert_eq!(pixels_at(t, 0, 0), [rgb(FG)], "{how}: RIS forgot 6");
    });
}

/// **A later shared Sixel recolours the shared pictures drawn with a
/// register it redefines**, as a VT340's one colour map does: "You can
/// draw an image, then change its appearance by changing the associated
/// output map location" (`[[source: vt3xx-screen-control]]`, "Output
/// Mapping"). Picture A draws register 1 red and register 2, undefined, in
/// the foreground; picture B defines register 1 green, and A's first
/// column turns green while its second keeps the foreground. A palette
/// alone, a shared body with no data, turns it blue. At power-on, `?1070`
/// set, B recolours nothing.
#[test]
fn a_later_shared_sixel_recolours_the_pictures_drawn_with_its_register() {
    let a = b"\x1bP0;1;0q\"1;1#1;2;100;0;0#1~#2~\x1b\\\n";
    let b = b"\x1bP0;1;0q\"1;1#1;2;0;100;0#1~~\x1b\\\n";
    let palette = b"\x1bP0;1;0q#1;2;0;0;100\x1b\\";
    let (green, blue) = (rgb((0, 255, 0)), rgb((0, 0, 255)));
    let shared = b"\x1b[?1070l".as_slice();
    every_read(CLAIMED, &[shared, a, b].concat(), |t, how| {
        assert_eq!(pixels_at(t, 0, 0), [green, green], "{how}: A's register 1");
        assert_eq!(
            pixels_at(t, 1, 0),
            [rgb(FG), green],
            "{how}: A's register 2"
        );
    });
    every_read(CLAIMED, &[shared, a, palette].concat(), |t, how| {
        assert_eq!(pixels_at(t, 0, 0), [blue], "{how}: a palette alone");
        assert_eq!(pixels_at(t, 1, 0), [rgb(FG)], "{how}");
    });
    every_read(CLAIMED, &[a.as_slice(), b].concat(), |t, how| {
        assert_eq!(pixels_at(t, 0, 0), [red(), green], "{how}: private");
    });
}

/// **A read of palettes recolours the stored pictures once**: two shared
/// 16x12 pictures drawn with register 1 red, then a hundred bodies that
/// only redefine register 1, each a bluer colour, in one read. Both
/// pictures end in the last colour, blue, and the store's epoch, which a
/// recolour that changes a pixel moves, moves once: a recolour at each
/// body's end passed over both pictures a hundred times and moved it a
/// hundred. The control, a body a read, moves it a hundred times, and
/// leaves the same pixels.
#[test]
fn a_read_of_palettes_recolours_the_stored_pictures_once() {
    let picture = b"\x1bP0;1;0q\"1;1#1;2;100;0;0#1!16~-!16~\x1b\\\n";
    let palettes: Vec<Vec<u8>> = (1..=100)
        .map(|b| format!("\x1bP0;1;0q#1;2;0;0;{b}\x1b\\").into_bytes())
        .collect();
    let blue = rgb((0, 0, 255));
    for (reads, moves) in [(1, 1), (100, 100)] {
        let ws = support::CORPUS;
        let mut owned = Owned::new(&ws).with_images(CLAIMED.arena.budget());
        let mut t = Term::new(owned.storage(), &ws);
        t.set_sixel_claim(SixelClaim::Yes);
        t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
        support::feed(
            &mut t,
            &[b"\x1b[?1070l".as_slice(), picture, picture].concat(),
        );
        assert_eq!(pixels_at(&t, 15, 11), [red(), red()], "the premise");
        let before = t.images().generation().raw();
        match reads {
            1 => support::feed(&mut t, &palettes.concat()),
            _ => palettes.iter().for_each(|b| support::feed(&mut t, b)),
        }
        let after = t.images().generation().raw();
        assert_eq!(
            after.wrapping_sub(before),
            moves,
            "{reads} reads: the passes"
        );
        let imgs = t.images();
        let mut n = 0;
        for (_, p) in imgs.placements() {
            let v = imgs.image(p.image).expect("live");
            assert_eq!(v.pixels.len(), 16 * 12);
            assert!(v.pixels.iter().all(|&c| c == blue), "{reads} reads");
            n += 1;
        }
        assert_eq!(n, 2, "both pictures stored");
    }
}

/// **A redefinition inside a body recolours the pixels drawn before it**,
/// at power-on and under shared registers: register 1 is red for the first
/// column and blue for the second, and both are blue once the body ends.
/// The pixels on a VT340's screen "are kept in memory as color indices
/// which map to the actual color to display" (`[[source:
/// vt340test-colormap]]`).
#[test]
fn a_redefinition_inside_a_body_recolours_the_pixels_drawn_before_it() {
    let body = b"\x1bP0;1;0q\"1;1#1;2;100;0;0#1~#1;2;0;0;100~\x1b\\";
    let blue = rgb((0, 0, 255));
    for pre in [&b""[..], b"\x1b[?1070l"] {
        every_read(CLAIMED, &[pre, body].concat(), |t, how| {
            assert_eq!(placed(t).4[..2], [blue, blue], "{how}");
        });
    }
}

/// **A shared picture drawn with no foreground said takes a register
/// defined later**, as one drawn in the foreground does: picture A paints
/// registers 1 and 0, neither defined, with a position left at zero
/// between, and a palette defines 0 green and 1 blue. A clear ink is
/// still the register's ink, so both its pixels take the new colours and
/// the zero position stays clear -- under `P2` 0 over a 6 by 6 raster,
/// the fill. The control is the same with the foreground said.
#[test]
fn a_shared_picture_drawn_with_no_foreground_said_takes_a_later_definition() {
    let palette = b"\x1bP0;1;0q#0;2;0;100;0#1;2;0;0;100\x1b\\";
    let (green, blue) = (rgb((0, 255, 0)), rgb((0, 0, 255)));
    let unsaid = Setup {
        fg: None,
        ..CLAIMED
    };
    let cases: [(&[u8], &[Pixel]); 2] = [
        (
            b"\x1bP0;1;0q\"1;1#1~#0~?~\x1b\\\n",
            &[blue, green, Pixel::CLEAR, green],
        ),
        (
            b"\x1bP0;0;0q\"1;1;6;6#1~#0~?~\x1b\\\n",
            &[blue, green, Pixel::FILL, green, Pixel::FILL, Pixel::FILL],
        ),
    ];
    for (set, what) in [(unsaid, "no foreground said"), (CLAIMED, "the control")] {
        for (picture, want) in cases {
            let program = [b"\x1b[?1070l".as_slice(), picture, palette].concat();
            every_read(set, &program, |t, how| {
                let row: Vec<Pixel> = (0..want.len()).map(|x| pixels_at(t, x, 0)[0]).collect();
                assert_eq!(row, want, "{what}, {how}");
            });
        }
    }
}

/// **A definition reaches the stored pictures at the end of the read that
/// carries it**, while its body is still open: picture A draws register 1
/// red, and a palette body arrives in three reads, defining it green in
/// the first and blue in the second and ending in the third. A frame is
/// drawn between reads, so A shows green after the first and blue after
/// the second, as a VT340's colour map, read at each refresh, shows each
/// definition (`[[source: dec-std-070]]` 070-5 §5.6.3.3, "During display
/// refresh, the active color map is used to translate color values"); a
/// definition is applied when the byte after it arrives. Fed in reads of
/// every size, the program ends blue.
#[test]
fn a_definition_reaches_the_pictures_at_the_end_of_the_read_that_carries_it() {
    let a = b"\x1b[?1070l\x1bP0;1;0q\"1;1#1;2;100;0;0#1~\x1b\\\n".as_slice();
    let reads: [&[u8]; 3] = [b"\x1bP0;1;0q#1;2;0;100;0#", b"1;2;0;0;100$", b"\x1b\\"];
    let (green, blue) = (rgb((0, 255, 0)), rgb((0, 0, 255)));
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(CLAIMED.arena.budget());
    let mut t = Term::new(owned.storage(), &ws);
    t.set_sixel_claim(SixelClaim::Yes);
    t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
    support::feed(&mut t, a);
    assert_eq!(pixels_at(&t, 0, 0), [red()], "the premise");
    for (read, want) in reads.iter().zip([green, blue, blue]) {
        support::feed(&mut t, read);
        assert_eq!(pixels_at(&t, 0, 0), [want], "after {read:?}");
    }
    every_read(CLAIMED, &[a, &reads.concat()].concat(), |t, how| {
        assert_eq!(pixels_at(t, 0, 0), [blue], "{how}");
    });
}

/// **XTSMGRAPHICS answers what baud has**, in xterm's shape
/// (`CASE_GRAPHICS_ATTRIBUTES`, `[[source: xterm-reset]]`'s file): 256
/// registers, which a set can only confirm; the text area in pixels, and a
/// `0 ; 0` maximum as `[[source: ctlseqs]]` asks of a terminal with no
/// maximum dimensions; ReGIS's page, the text area in pixels or the
/// VT300's 800 by 480, and a `0 ; 0` maximum, answered whatever DA claims;
/// an error in `Pi` for anything unknown, an error in `Pa` for an unknown
/// action, a failure for a geometry set or reset.
#[test]
fn xtsmgraphics_answers_the_registers_and_the_geometry() {
    let asks: [(&[u8], &[u8]); 15] = [
        (b"\x1b[?1;1;0S", b"\x1b[?1;0;256S"),
        (b"\x1b[?1;2;0S", b"\x1b[?1;0;256S"),
        (b"\x1b[?1;3;256S", b"\x1b[?1;0;256S"),
        (b"\x1b[?1;3;16S", b"\x1b[?1;3S"),
        (b"\x1b[?1;4;0S", b"\x1b[?1;0;256S"),
        (b"\x1b[?1;9;0S", b"\x1b[?1;2S"),
        (b"\x1b[?2;1;0S", b"\x1b[?2;0;1600;800S"),
        (b"\x1b[?2;2;0S", b"\x1b[?2;3S"),
        (b"\x1b[?2;4;0S", b"\x1b[?2;0;0;0S"),
        (b"\x1b[?3;1;0S", b"\x1b[?3;0;1600;800S"),
        (b"\x1b[?3;2;0S", b"\x1b[?3;3S"),
        (b"\x1b[?3;3;800S", b"\x1b[?3;3S"),
        (b"\x1b[?3;4;0S", b"\x1b[?3;0;0;0S"),
        (b"\x1b[?3;9;0S", b"\x1b[?3;2S"),
        (b"\x1b[?9;1;0S", b"\x1b[?9;1S"),
    ];
    for (ask, want) in asks {
        every_read(CLAIMED, ask, |t, how| {
            assert_eq!(t.reply(), want, "{}: {how}", String::from_utf8_lossy(ask));
            assert_eq!(top(t), LineIdx::FIRST, "{how}: nothing scrolled");
        });
    }
    // No cell size: the geometry is not known.
    let no_cell = Setup {
        cell: None,
        ..CLAIMED
    };
    every_read(no_cell, b"\x1b[?2;1;0S", |t, how| {
        assert_eq!(t.reply(), b"\x1b[?2;3S", "{how}");
    });
    // ReGIS's page is the VT300's then.
    every_read(no_cell, b"\x1b[?3;1;0S", |t, how| {
        assert_eq!(t.reply(), b"\x1b[?3;0;800;480S", "{how}");
    });
    // Not configured for graphics: a success becomes a failure, an error in
    // Pa stays one.
    let unclaimed = Setup {
        claim: SixelClaim::No,
        ..CLAIMED
    };
    every_read(unclaimed, b"\x1b[?1;1;0S\x1b[?1;9;0S", |t, how| {
        assert_eq!(t.reply(), b"\x1b[?1;3S\x1b[?1;2S", "{how}");
    });
    // ReGIS is always on: its geometry is answered.
    every_read(unclaimed, b"\x1b[?3;1;0S", |t, how| {
        assert_eq!(t.reply(), b"\x1b[?3;0;1600;800S", "{how}");
    });
}

/// **Only the three-parameter form is answered**: two or four parameters
/// are counted and get no reply, and none scrolls the screen as `CSI Ps S`
/// would.
#[test]
fn xtsmgraphics_of_another_arity_is_counted_and_unanswered() {
    for ask in [b"\x1b[?1;1S".as_slice(), b"\x1b[?2;3;100;100S"] {
        every_read(CLAIMED, ask, |t, how| {
            assert_eq!(t.reply(), b"", "{how}: no reply");
            assert_eq!(t.unsupported, 1, "{how}: counted");
            assert_eq!(top(t), LineIdx::FIRST, "{how}: nothing scrolled");
        });
    }
}
