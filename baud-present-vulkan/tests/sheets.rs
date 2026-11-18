//! **The glyph sheets persist, and a glyph reaches them as its own
//! rectangle**: an atlas brought up the first time is taken whole; a glyph
//! drawn after is copied as the rectangle it was written in and no more,
//! and the glyph before it still samples as it did; a frame with nothing
//! new copies nothing; and a sheet the atlas replaces is taken whole again.
//! Every glyph is read back through a draw that samples it, so a copy to
//! the wrong place or with the wrong stride shows as wrong pixels.

#![cfg(target_os = "linux")]

mod common;

use baud_domain::Codepoint;
use baud_present_vulkan::draw::{Pass, Records, Scene, Texel, Texels};
use baud_present_vulkan::frame::{Gpu, Premultiplied};
use baud_present_vulkan::pipeline::Pipelines;
use baud_present_vulkan::sheets::{Copied, Sheets};
use baud_present_wayland::Px;
use baud_render::{Atlas, PxSize, SizeKey, Slot};
use common::record;

const W: u32 = 64;
const H: u32 = 32;
const BLACK: u32 = 0xFF00_0000;
const WHITE: u32 = 0xFFFF_FFFF;

/// A box-drawing mask in `atlas`, at an 8 by 16 cell.
fn glyph(atlas: &mut Atlas, cp: u32) -> Slot {
    let cell = baud_render::boxes::Cell {
        w: baud_font::Px::of(8),
        h: baud_font::Px::of(16),
    };
    atlas
        .synthetic(
            Codepoint::new(cp).expect("a codepoint"),
            cell,
            baud_font::Px::of(12),
        )
        .expect("a box-drawing mask")
}

/// **A frame of `slots`, each drawn white at its own place over black**,
/// read back: each glyph at x = 16 a slot, y = 0, at its atlas size.
fn frame(
    gpu: &Gpu,
    pipes: &mut Pipelines,
    sheets: &Sheets,
    atlas: &Atlas,
    slots: &[Slot],
) -> Vec<u32> {
    let t = gpu.target(Px(W), Px(H)).expect("a target");
    let side = atlas.side().value() as f32;
    let cside = atlas.color_side().value() as f32;
    let mut records = Vec::new();
    for (i, s) in (0i16..).zip(slots) {
        records.extend_from_slice(&record(
            (16 * i, 0),
            (s.w.0, s.h.0),
            (s.x.0, s.y.0),
            (WHITE, 0),
        ));
    }
    // A black background a cell, the grid over the whole target.
    let (cols, rows) = (W / 8, H / 16);
    let bg: Vec<u8> = (0..cols * rows).flat_map(|_| BLACK.to_le_bytes()).collect();
    let scene = Scene {
        u: [
            W as f32,
            H as f32,
            8.0,
            16.0,
            cols as f32,
            rows as f32,
            side,
            side,
            cside,
            cside,
            0.0,
            0.0,
        ],
        backgrounds: Texels {
            width: Px(cols),
            height: Px(rows),
            texel: Texel::Argb,
            bytes: &bg,
        },
        sheets,
        records: &records,
        count: Records(u32::try_from(slots.len()).expect("a count")),
        framing: None,
    };
    gpu.clear(&t, Premultiplied(BLACK), None)
        .expect("a black target");
    gpu.draw(pipes, &t, &scene, Pass::Whole)
        .expect("a drawn frame");
    let got = gpu.read(&t).expect("the frame read back");
    gpu.drop_target(t);
    got
}

/// What [`frame`] draws: each slot's coverage as grey over black, from
/// the atlas's own texels, and black elsewhere.
fn want(atlas: &Atlas, slots: &[Slot]) -> Vec<u32> {
    let side = usize::try_from(atlas.side().value()).expect("a side");
    let mut out = vec![BLACK; usize::try_from(W * H).expect("a size")];
    let width = usize::try_from(W).expect("a width");
    for (i, s) in slots.iter().enumerate() {
        for y in 0..usize::from(s.h.0) {
            for x in 0..usize::from(s.w.0) {
                let c = u32::from(
                    atlas.pixels()[(usize::from(s.y.0) + y) * side + usize::from(s.x.0) + x],
                );
                out[y * width + 16 * i + x] = BLACK | c << 16 | c << 8 | c;
            }
        }
    }
    out
}

fn persists_and_copies_a_rectangle(gpu: &Gpu) {
    let mut pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    let curves: Vec<u8> = (0..=255u8).collect();
    let mut sheets = gpu.sheets(&curves).expect("the sheets");
    let mut atlas = Atlas::new();
    atlas.at(SizeKey::of(PxSize::of(14.0).expect("a size")));
    let across = glyph(&mut atlas, 0x2500);
    sheets.bring(gpu, &mut atlas).expect("brought up");
    let before = sheets.copied();
    let got = frame(gpu, &mut pipes, &sheets, &atlas, &[across]);
    assert_eq!(
        got,
        want(&atlas, &[across]),
        "the first glyph samples as drawn"
    );
    let side = u64::from(atlas.side().value());
    let cside = u64::from(atlas.color_side().value());
    assert_eq!(
        sheets.copied().0 - before.0,
        side * side + cside * cside,
        "a new atlas is taken whole, both sheets"
    );

    let down = glyph(&mut atlas, 0x2502);
    sheets.bring(gpu, &mut atlas).expect("brought up");
    let before = sheets.copied();
    let got = frame(gpu, &mut pipes, &sheets, &atlas, &[across, down]);
    assert_eq!(
        got,
        want(&atlas, &[across, down]),
        "the second glyph lands where the atlas put it, and the first is as it was"
    );
    assert_eq!(
        sheets.copied().0 - before.0,
        u64::from(down.w.0) * u64::from(down.h.0),
        "a glyph is copied as its own rectangle"
    );

    sheets.bring(gpu, &mut atlas).expect("brought up");
    let before = sheets.copied();
    frame(gpu, &mut pipes, &sheets, &atlas, &[across, down]);
    assert_eq!(
        sheets.copied(),
        before,
        "a frame with nothing new copies nothing"
    );

    atlas.reset();
    let again = glyph(&mut atlas, 0x253C);
    sheets.bring(gpu, &mut atlas).expect("brought up");
    let before = sheets.copied();
    let side = u64::from(atlas.side().value());
    let got = frame(gpu, &mut pipes, &sheets, &atlas, &[again]);
    assert_eq!(
        got,
        want(&atlas, &[again]),
        "a replaced sheet samples its new glyph"
    );
    assert!(
        sheets.copied().0 - before.0 >= side * side,
        "a replaced sheet is taken whole"
    );
    sheets.destroy(gpu);
    pipes.destroy();
}

#[test]
fn the_sheets_persist_and_a_glyph_is_copied_as_its_rectangle_on_lavapipe() {
    persists_and_copies_a_rectangle(&common::lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn the_sheets_persist_and_a_glyph_is_copied_as_its_rectangle_on_radv() {
    persists_and_copies_a_rectangle(&common::radv());
}

#[test]
fn a_copy_outside_its_sheet_is_refused() {
    let gpu = common::lavapipe();
    let mut sheets = gpu.sheets(&[0; 256]).expect("the sheets");
    let two = Texels {
        width: Px(2),
        height: Px(1),
        texel: Texel::Coverage,
        bytes: &[1, 2],
    };
    assert!(
        sheets
            .copy(
                &gpu,
                baud_present_vulkan::sheets::Which::Mask,
                (Px(0), Px(0)),
                &two
            )
            .is_err(),
        "two texels into a one-texel sheet"
    );
    assert_eq!(sheets.copied(), Copied(0), "nothing was recorded yet");
    sheets.destroy(&gpu);
}
