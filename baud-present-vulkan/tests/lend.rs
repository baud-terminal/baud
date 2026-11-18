//! **The cell path's lent memory, on each arm**: what the CPU writes
//! through the slice is what the stage reads once a frame has settled it.
//! In place that is every write; under staging it is the ranges owed and
//! no others, and the bytes copied are those ranges' and no more. Run on
//! each arm on lavapipe and on RADV.

#![cfg(target_os = "linux")]

mod common;

use ash::vk;
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::lend::{Arm, Elems, Uploaded};

/// Rows of eight words.
const COLS: usize = 8;
const ROWS: usize = 6;

/// A word naming `tag` and its index.
fn cell(tag: u32, i: usize) -> u32 {
    tag << 24 | u32::try_from(i).expect("a small index")
}

fn bytes_of(cells: &[u32]) -> Vec<u8> {
    cells.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn owes_what_it_was_told(gpu: &Gpu, arm: Arm) {
    let (mut lent, cells) = gpu
        .lend::<u32>(
            arm,
            Elems(COLS * ROWS),
            vk::BufferUsageFlags::STORAGE_BUFFER,
        )
        .expect("lent memory");
    assert_eq!(lent.arm(), arm);
    for (i, c) in cells.iter_mut().enumerate() {
        *c = cell(0xA0, i);
    }
    lent.owe(0..COLS * ROWS);
    gpu.settle(&mut lent).expect("settled");
    assert_eq!(
        gpu.read_back(&lent).expect("read back"),
        bytes_of(cells),
        "{arm:?}: every cell written and owed is read"
    );
    let first = lent.uploaded();

    // Every cell written again; rows 2 and 3 owed, as two runs that merge.
    let before = bytes_of(cells);
    for (i, c) in cells.iter_mut().enumerate() {
        *c = cell(0xB0, i);
    }
    lent.owe(2 * COLS..3 * COLS);
    lent.owe(3 * COLS..4 * COLS);
    gpu.settle(&mut lent).expect("settled");
    let got = gpu.read_back(&lent).expect("read back");
    let size = size_of::<u32>();
    let (owed_from, owed_to) = (2 * COLS * size, 4 * COLS * size);
    match arm {
        Arm::InPlace => {
            assert_eq!(got, bytes_of(cells), "in place the stage reads every write");
            assert_eq!(lent.uploaded(), Uploaded(0), "and nothing is copied");
        }
        Arm::Staging => {
            let mut want = before;
            want[owed_from..owed_to].copy_from_slice(&bytes_of(cells)[owed_from..owed_to]);
            assert_eq!(got, want, "under staging the owed rows and no others");
            assert_eq!(
                first,
                Uploaded(u64::try_from(COLS * ROWS * size).expect("bytes"))
            );
            assert_eq!(
                lent.uploaded().0 - first.0,
                u64::try_from(owed_to - owed_from).expect("bytes"),
                "the owed bytes copied, no more"
            );
        }
    }
    gpu.settle(&mut lent).expect("settled");
    assert_eq!(
        gpu.read_back(&lent).expect("read back"),
        got,
        "a frame that owes nothing copies nothing"
    );
    lent.destroy(gpu);
}

#[test]
fn lent_memory_is_read_as_owed_on_lavapipe() {
    let gpu = common::lavapipe();
    for arm in [Arm::InPlace, Arm::Staging] {
        owes_what_it_was_told(&gpu, arm);
    }
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn lent_memory_is_read_as_owed_on_radv() {
    let gpu = common::radv();
    let arm = gpu.arm(64 << 20);
    eprintln!("radv: {arm:?} for 64 MiB");
    owes_what_it_was_told(&gpu, Arm::Staging);
    if arm == Arm::InPlace {
        owes_what_it_was_told(&gpu, Arm::InPlace);
    }
}

#[test]
fn lavapipe_lends_in_place() {
    assert_eq!(common::lavapipe().arm(64 << 20), Arm::InPlace);
}
