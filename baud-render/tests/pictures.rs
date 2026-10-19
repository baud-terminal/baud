//! **A picture drawn in text is found from its neighbours, and nothing
//! else is.** `Pictures` joins the cells whose drawn background the
//! application set into blocks through their four neighbours, and a block
//! holding [`PICTURE_COLOURS`] distinct background colours is a picture:
//! its backgrounds stay solid under `[glass] app_backgrounds = "dim"`.
//!
//! The rule is only worth having if it is quiet on every TUI and fires on
//! every preview, so both halves are gated on real bytes: the corpus and
//! the four panel captures (`tests/panels/`) at every read boundary,
//! and chafa's two captures (`tests/previews/`) placed as yazi places them.
//! Quiet on every TUI holds on a normal screen; under `?5`, where every
//! cell draws its foreground as its background, btop and cava are found
//! too, and a test pins that arm.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    AppBg, Blend, Damage, PICTURE_COLOURS, PictureBg, Pictured, Pictures, PxSize, Renderer, Stack,
    Theme, Video,
};
use support::{Canvas, font, paint};

const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 40,
};

/// `tests/panels/`'s captures: three themed editors, and herdr stepping
/// through the seventeen themes of its settings menu.
const PANELS: [&str; 4] = ["helix", "vimdiff", "vimhl", "herdr-themes"];

/// The corpus geometry, which the recordings and the panels were taken at.
const CORPUS: WinSize = WinSize {
    ws_row: 50,
    ws_col: 200,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// The screen after `bytes`, and which of its cells are a picture.
fn found(ws: &WinSize, bytes: &[u8]) -> Vec<Vec<Pictured>> {
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    feed(&mut t, bytes);
    let mut p = Pictures::new();
    let view = t.view(Rows::ZERO);
    let cells = p.find(&t, &view, Video::Normal);
    let grid: Vec<Vec<Pictured>> = (0..usize::from(ws.ws_row))
        .map(|y| {
            (0..usize::from(ws.ws_col))
                .map(|x| p.at(Row::from_index(y), Col::at_index(x)))
                .collect()
        })
        .collect();
    let counted = grid
        .iter()
        .flatten()
        .filter(|&&c| c == Pictured::Yes)
        .count();
    assert_eq!(
        usize::try_from(cells.value()).unwrap(),
        counted,
        "the tally is the cells marked"
    );
    grid
}

fn yes(grid: &[Vec<Pictured>]) -> usize {
    grid.iter()
        .flatten()
        .filter(|&&c| c == Pictured::Yes)
        .count()
}

/// `n` cells from the cursor, each a space on its own truecolour background.
fn colours(n: u32, seed: u32) -> String {
    (0..n)
        .map(|i| format!("\x1b[48;2;{};{};90m ", (i * 13 + seed) % 256, seed % 256))
        .collect::<String>()
        + "\x1b[0m"
}

/// **The threshold is the measured one**, and it is a threshold: a block of
/// sixteen colours is a picture in every cell, one of fifteen in none.
#[test]
fn sixteen_colours_are_a_picture_and_fifteen_are_not() {
    assert_eq!(PICTURE_COLOURS, 16, "the measured threshold moved");
    let at = |n: u32| found(&WS, format!("\x1b[2;3H{}", colours(n, 7)).as_bytes());
    let g = at(16);
    assert_eq!(yes(&g), 16);
    assert!((2..18).all(|x| g[1][x] == Pictured::Yes));
    assert_eq!(g[1][1], Pictured::No, "the default cell beside it");
    assert_eq!(yes(&at(15)), 0, "fifteen colours are a panel");
    // Repeats count once: sixteen cells of eight colours are no picture.
    let repeated = format!("\x1b[2;3H{}{}", colours(8, 7), colours(8, 7));
    assert_eq!(yes(&found(&WS, repeated.as_bytes())), 0, "repeats counted");
}

/// Joined through the four neighbours: two eight-colour runs sharing one
/// column across two rows are one sixteen-colour block, the same runs
/// touching only at a corner are two blocks of eight, and a default cell
/// between two runs on one row keeps them apart.
#[test]
fn a_block_joins_through_its_four_neighbours_and_not_its_corners() {
    let over = format!("\x1b[2;1H{}\x1b[3;8H{}", colours(8, 7), colours(8, 150));
    assert_eq!(yes(&found(&WS, over.as_bytes())), 16, "one shared column");
    let corner = format!("\x1b[2;1H{}\x1b[3;9H{}", colours(8, 7), colours(8, 150));
    assert_eq!(
        yes(&found(&WS, corner.as_bytes())),
        0,
        "a corner is no join"
    );
    let gap = format!("\x1b[2;1H{} {}", colours(8, 7), colours(8, 150));
    assert_eq!(
        yes(&found(&WS, gap.as_bytes())),
        0,
        "a default cell between"
    );
}

/// **A join found late merges two blocks counted apart.** Two
/// eight-colour runs on row 2, apart; on row 3 a one-colour run under the
/// first and one cell under the second; and a run on row 4 that reaches
/// both. Before row 4 the blocks hold eight and nine colours, so only its
/// union makes sixteen, and every cell of every run -- the ones counted
/// before the join included -- must be a picture.
#[test]
fn a_late_join_merges_two_blocks_counted_apart() {
    let bytes = format!(
        "\x1b[2;1H{}\x1b[2;12H{}\x1b[3;1H\x1b[48;2;7;7;90m  \x1b[0m\
         \x1b[4;1H\x1b[48;2;7;7;90m{}\x1b[0m\x1b[3;12H\x1b[48;2;7;7;90m \x1b[0m",
        colours(8, 7),
        colours(8, 150),
        " ".repeat(12)
    );
    let g = found(&WS, bytes.as_bytes());
    assert_eq!(yes(&g), 8 + 8 + 2 + 12 + 1);
    assert_eq!(
        g[1][11],
        Pictured::Yes,
        "the second run, joined two rows down"
    );
    // And without the bottom row the two stay eight apiece.
    let apart = format!("\x1b[2;1H{}\x1b[2;12H{}", colours(8, 7), colours(8, 150));
    assert_eq!(yes(&found(&WS, apart.as_bytes())), 0);
}

/// **The drawn background is what counts.** Under `SGR 7` a set foreground
/// is drawn as the background, so sixteen reversed cells of sixteen
/// foregrounds are a picture -- and sixteen reversed cells of sixteen
/// *backgrounds* over the default foreground are not, because what they
/// draw behind the glyph is the default.
#[test]
fn reverse_draws_the_foreground_as_the_background() {
    let fg: String = (0..16u32)
        .map(|i| format!("\x1b[7;38;2;{};40;40m ", i * 15))
        .collect::<String>()
        + "\x1b[0m";
    assert_eq!(yes(&found(&WS, format!("\x1b[2;1H{fg}").as_bytes())), 16);
    let bg: String = (0..16u32)
        .map(|i| format!("\x1b[7;48;2;{};40;40m ", i * 15))
        .collect::<String>()
        + "\x1b[0m";
    assert_eq!(yes(&found(&WS, format!("\x1b[2;1H{bg}").as_bytes())), 0);
}

/// A capture, placed as yazi places it: each line at the pane's origin, a
/// row further down each time, behind a prompt line.
fn placed(name: &str, row0: usize, col0: usize) -> (Vec<u8>, usize) {
    let text = std::fs::read_to_string(format!(
        "{}/../tests/previews/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the capture");
    let mut bytes = b"\x1b[1;1H~/photos $ yazi".to_vec();
    let mut rows = 0;
    for (i, line) in text.split('\n').filter(|l| !l.is_empty()).enumerate() {
        bytes.extend(format!("\x1b[{};{}H", row0 + i + 1, col0 + 1).into_bytes());
        bytes.extend(line.as_bytes());
        rows += 1;
    }
    (bytes, rows)
}

/// **Both of chafa's captures are one picture in every cell**, and nothing
/// around them is: 1148 cells each, the prompt and the default background
/// untouched.
#[test]
fn every_chafa_capture_is_a_picture_in_every_cell() {
    for name in ["chafa-truecolour.ans", "chafa-240.ans"] {
        let (bytes, rows) = placed(name, 3, 20);
        assert_eq!(rows, 28, "{name} is not the 41x28 it was");
        let g = found(&CORPUS, &bytes);
        assert_eq!(yes(&g), 41 * 28, "{name}: every preview cell");
        for (y, row) in g.iter().enumerate() {
            for (x, &c) in row.iter().enumerate() {
                let inside = (3..3 + 28).contains(&y) && (20..20 + 41).contains(&x);
                assert_eq!(c == Pictured::Yes, inside, "{name}: cell {y},{x}");
            }
        }
    }
}

/// **On a normal screen no recording and no themed editor draws a
/// picture, at any read boundary.** Under `?5` two of them do, and
/// `under_reverse_video_only_btop_and_cava_draw_a_picture` holds that arm.
/// The nine recordings and `tests/panels/`'s four captures --
/// vim's diff mode under `habamax`, the busiest block measured at 8
/// colours, helix, vim with a search highlighted, and herdr previewing
/// each of its seventeen themes, whose busiest block is 3 colours on a
/// finished screen and 5 at a read that splits a frame -- each fed a read
/// at a time, with the finder asked after every read.
///
/// Positive control: the same harness, fed a chafa capture after the
/// last read, finds it -- so a zero is the rule being quiet, not the
/// harness blind.
#[test]
fn no_recording_or_panel_draws_a_picture_at_any_read_boundary() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests");
    let streams = support::RECORDINGS
        .iter()
        .map(|r| format!("{root}/recordings/{r}"))
        .chain(PANELS.map(|p| format!("{root}/panels/{p}.raw")));
    let (preview, _) = placed("chafa-truecolour.ans", 3, 20);
    let mut boundaries = 0usize;
    let mut set_bg_seen = 0usize;
    for path in streams {
        let bytes = std::fs::read(&path).expect("the stream");
        let mut owned = Owned::new(&CORPUS);
        let mut t = Term::new(owned.storage(), &CORPUS);
        let mut p = Pictures::new();
        for chunk in bytes.chunks(support::CHUNK) {
            feed(&mut t, chunk);
            let view = t.view(Rows::ZERO);
            let cells = p.find(&t, &view, Video::Normal);
            assert_eq!(cells.value(), 0, "{path}: read {boundaries} drew a picture");
            boundaries += 1;
        }
        let view = t.view(Rows::ZERO);
        for y in 0..usize::from(CORPUS.ws_row) {
            for &c in view.row(Row::from_index(y)) {
                let s = t.style_of(c);
                if !s.bg().is_default() {
                    set_bg_seen += 1;
                }
            }
        }
        feed(&mut t, b"\x1b[0m\x1b[H\x1b[2J");
        feed(&mut t, &preview);
        let view = t.view(Rows::ZERO);
        assert_eq!(
            p.find(&t, &view, Video::Normal).value(),
            41 * 28,
            "{path}: the control"
        );
    }
    assert!(boundaries > 500, "{boundaries} boundaries");
    // The streams set backgrounds, so the zeros above were asked of panels.
    assert!(set_bg_seen > 20_000, "{set_bg_seen} set backgrounds");
}

/// **Under `?5` btop and cava draw pictures, and nothing else in the
/// corpus does.** The same thirteen streams as the normal-screen guard
/// above, fed a read at a time with the finder asked under
/// [`Video::Reverse`] after every read. Every cell is exchanged under `?5`,
/// so a cell's drawn background is its stored foreground, and the
/// background the finder reads is the drawn one (the record, "`?5`
/// exchanges every cell's colours", open question 3, where the owner is
/// asked whether it should be). btop's and cava's coloured text then makes
/// a block of sixteen colours or more: btop at 125 of its 128 reads, 7200
/// cells at most, and cava at 152 of 156, 4257 cells at most. cava erases
/// under a foreground, and an erase keeps only the background (the record,
/// "An erase clears every attribute but the background"), so its erased
/// cells carry no foreground and draw the default page under `?5`.
///
/// Positive control: chafa's capture, fed after each stream, is 1139 of
/// its 1148 cells under `?5`. The nine it loses are its own `SGR 7` cells
/// with a set foreground over the default background, which `?5`
/// exchanges a second time onto the default.
///
/// And the zeros were asked of colour: the eleven quiet streams' last
/// screens hold 17,598 cells whose drawn background under `?5` is set.
/// asciiquarium, vimdiff and vimhl erase under a foreground too, and their
/// erased cells draw the default page under `?5` for the same reason.
#[test]
fn under_reverse_video_only_btop_and_cava_draw_a_picture() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests");
    let streams = support::RECORDINGS
        .iter()
        .map(|r| format!("{root}/recordings/{r}"))
        .chain(PANELS.map(|p| format!("{root}/panels/{p}.raw")));
    let (preview, _) = placed("chafa-truecolour.ans", 3, 20);
    let mut quiet_set_bg = 0usize;
    // Each stream: its name, reads, reads with a picture, the most cells
    // marked at one read, and the control fed after it.
    let mut got = Vec::new();
    let mut want = Vec::new();
    for path in streams {
        let bytes = std::fs::read(&path).expect("the stream");
        let name = path.rsplit('/').next().unwrap().to_owned();
        let mut owned = Owned::new(&CORPUS);
        let mut t = Term::new(owned.storage(), &CORPUS);
        let mut p = Pictures::new();
        let (mut reads, mut pictured, mut most) = (0usize, 0usize, 0u64);
        for chunk in bytes.chunks(support::CHUNK) {
            feed(&mut t, chunk);
            let view = t.view(Rows::ZERO);
            let cells = p.find(&t, &view, Video::Reverse).value();
            reads += 1;
            pictured += usize::from(cells > 0);
            most = most.max(cells);
        }
        let (expect_pictured, expect_most) = match name.as_str() {
            "btop.raw" => (125, 7200),
            "cava.raw" => (152, 4_257),
            _ => {
                let view = t.view(Rows::ZERO);
                for y in 0..usize::from(CORPUS.ws_row) {
                    for &c in view.row(Row::from_index(y)) {
                        let s = t.style_of(c);
                        let (_, page) = Video::Reverse.drawn(s.attrs(), s.fg(), s.bg());
                        quiet_set_bg += usize::from(!page.is_default());
                    }
                }
                (0, 0)
            }
        };
        feed(&mut t, b"\x1b[0m\x1b[H\x1b[2J");
        feed(&mut t, &preview);
        let view = t.view(Rows::ZERO);
        let control = p.find(&t, &view, Video::Reverse).value();
        want.push((
            name.clone(),
            reads,
            expect_pictured,
            expect_most,
            41 * 28 - 9,
        ));
        got.push((name, reads, pictured, most, control));
    }
    assert_eq!(
        want.iter().map(|w| w.1).collect::<Vec<_>>(),
        [82, 128, 110, 123, 8, 80, 156, 21, 34, 16, 8, 4, 54],
        "the reads each stream is fed in"
    );
    assert_eq!(
        got, want,
        "name, reads, with a picture, most cells, control"
    );
    assert_eq!(quiet_set_bg, 17_598, "set backgrounds drawn under ?5");
}

fn dim_renderer<'f, 'a>(faces: &'f [Face<'a>], pictures: PictureBg) -> Renderer<'f, 'a> {
    let theme = Theme {
        bg_alpha: Chan::from_u8(56),
        app_bg: AppBg::Dimmed,
        picture_bg: pictures,
        ..Theme::dark()
    };
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// **A row whose cells did not change is repainted when its block
/// became a picture.** Fifteen colours on row 2 and a paint; then one more
/// colour on row 3, under the first cell, which makes row 2 a picture
/// without writing a cell of it. Row damage compares a row's cells, so
/// the finder has to name the row -- an incremental frame must equal a
/// full repaint, both ways, and must differ from the frame before.
#[test]
fn a_block_that_becomes_a_picture_repaints_its_unchanged_rows() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    for pictures in [PictureBg::Opaque, PictureBg::Dimmed] {
        let mut inc = dim_renderer(&faces, pictures);
        let mut full = dim_renderer(&faces, pictures);
        let mut a = Canvas::for_renderer(&inc);
        let mut b = Canvas::for_renderer(&full);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let steps = [
            format!("\x1b[?25l\x1b[2;3H{}", colours(15, 7)),
            "\x1b[3;3H\x1b[48;2;250;250;250m \x1b[0m".to_string(),
            "\x1b[3;3H \x1b[0m".to_string(),
        ];
        let cell_h = usize::try_from(inc.metrics().cell_h.value()).unwrap();
        // Row 2's band of pixels -- its cells are written once, at step 0.
        let band = |c: &Canvas| c.pixels[cell_h * c.width..2 * cell_h * c.width].to_vec();
        let mut bands = Vec::new();
        for (i, step) in steps.iter().enumerate() {
            feed(&mut t, step.as_bytes());
            paint(&mut inc, &mut t, &mut a, Damage::Incremental);
            paint(&mut full, &mut t, &mut b, Damage::Full);
            assert!(
                a.pixels == b.pixels,
                "{pictures:?}: step {i} differs from a full repaint"
            );
            bands.push(band(&a));
        }
        // Under `opaque` the unchanged row went solid and back; under `dim`
        // it never moved -- so the equality above was asked of a row that
        // needed repainting, and not only of the cell that was written.
        let moved = bands[0] != bands[1];
        assert_eq!(
            moved,
            pictures == PictureBg::Opaque,
            "{pictures:?}: row 2 at step 1"
        );
        assert_eq!(bands[0], bands[2], "{pictures:?}: row 2 came back");
    }
}
