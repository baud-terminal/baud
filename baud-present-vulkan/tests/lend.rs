//! **The cell path's lent memory, on each arm**: what the CPU writes
//! through the slice is what the stage reads once a frame has settled it.
//! In place that is every write; under staging it is the ranges owed and
//! no others, and the bytes copied are those ranges' and no more. Run on
//! each arm on lavapipe and on RADV.
//!
//! **Cell memory that fails part way leaves nothing made**: each step of
//! [`CellMemory::new`] forced to fail in turn, on each arm, and every
//! device object made before it destroyed when it returns; the same of the
//! sheets, of a presenter, and of a frame read back.

#![cfg(target_os = "linux")]

mod common;

use ash::vk;
use baud_core::{Owned, Term};
use baud_domain::WinSize;
use baud_present_vulkan::cells::{CellMemory, CellSizes};
use baud_present_vulkan::frame::{Failed, Gpu};
use baud_present_vulkan::hooks::{Calls, Live};
use baud_present_vulkan::lend::{Arm, Elems, Uploaded};
use baud_present_vulkan::pipeline::Pipelines;
use baud_present_vulkan::present::Presenter;
use baud_present_vulkan::vk::Has;
use baud_present_wayland::Px;
use baud_render::screens::SETS;

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

/// **The fallible device calls `CellMemory::new` makes on `arm`**: the
/// descriptor pool and the atlas's set; each glyph map lent; each set's
/// five buffers lent, its descriptor sets and its three mapped buffers;
/// the recorder; and the sync semaphore and the two query pools where the
/// device has them. A mapped buffer is three: its memory, its binding and
/// its mapping. A buffer lent is one mapped buffer in place, and under
/// staging a device buffer, its memory and binding, then a mapped one.
fn steps(gpu: &Gpu, arm: Arm) -> u32 {
    let lent = match arm {
        Arm::InPlace => BUFFER_STEPS,
        Arm::Staging => 2 + BUFFER_STEPS,
    };
    let has = |h: Has| u32::from(h == Has::Yes);
    let sets = u32::try_from(SETS).expect("a few sets");
    2 + 2 * lent
        + sets * (5 * lent + 1 + 3 * BUFFER_STEPS)
        + 1
        + has(gpu.sync_fd)
        + has(gpu.clock.stamps)
        + has(gpu.clock.statistics)
}

/// **Each step of `CellMemory::new` on `arm` forced to fail in turn**, and
/// after each the device objects alive what they were before it: the
/// failure is returned, and nothing it made is left. The controls: the
/// step after the last makes the memory whole, which holds objects and
/// gives them all back when destroyed, and the steps failed are every
/// fallible call the memory is made through. Named break: a step's early
/// return that drops what the steps before it made.
fn fails_whole(gpu: &Gpu, arm: Arm) {
    const WS: WinSize = WinSize {
        ws_col: 24,
        ws_row: 6,
    };
    let mut owned = Owned::new(&WS);
    let t = Term::new(owned.storage(), &WS);
    let (cols, rows) = t.geometry();
    let sizes = CellSizes {
        cols,
        rows,
        stride: t.stride(),
        styles: Elems(t.styles().one().len()),
    };
    let pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    let before = gpu.objects_live();
    let mut failed = 0;
    loop {
        gpu.fail_at(Calls(failed), Failed::OUT_OF_DEVICE_MEMORY);
        match CellMemory::new(gpu, &pipes, arm, sizes) {
            Err(e) => {
                assert_eq!(e, Failed::OUT_OF_DEVICE_MEMORY, "{arm:?}: step {failed}");
                assert_eq!(
                    gpu.objects_live(),
                    before,
                    "{arm:?}: objects left by a failure at step {failed}"
                );
                failed += 1;
            }
            Ok((mem, slices)) => {
                assert!(
                    gpu.objects_live().0 > before.0,
                    "the control: a whole memory holds objects"
                );
                drop(slices);
                mem.destroy(gpu);
                assert_eq!(gpu.objects_live(), before, "{arm:?}: destroyed whole");
                break;
            }
        }
        assert!(failed <= 1000, "{arm:?}: a step past every call");
    }
    assert_eq!(failed, steps(gpu, arm), "{arm:?}: every step failed once");
    assert_eq!(before, Live(0), "the control: nothing made before");
    pipes.destroy();
}

#[test]
fn cell_memory_that_fails_part_way_leaves_nothing_on_lavapipe() {
    let gpu = common::lavapipe();
    for arm in [Arm::InPlace, Arm::Staging] {
        fails_whole(&gpu, arm);
    }
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn cell_memory_that_fails_part_way_leaves_nothing_on_radv() {
    let gpu = common::radv();
    fails_whole(&gpu, Arm::Staging);
    if gpu.arm(64 << 20) == Arm::InPlace {
        fails_whole(&gpu, Arm::InPlace);
    }
}

/// **`make` forced to fail at each of its fallible device calls in turn**,
/// and after each the device objects alive what they were before it: the
/// failure is returned, and nothing it made is left. The step after the
/// last makes it whole, and `unmake` gives back what it holds. The steps
/// failed.
fn each_step_fails<T>(
    gpu: &Gpu,
    what: &str,
    mut make: impl FnMut() -> Result<T, Failed>,
    unmake: impl FnOnce(T),
) -> u32 {
    let before = gpu.objects_live();
    let mut failed = 0;
    loop {
        gpu.fail_at(Calls(failed), Failed::OUT_OF_DEVICE_MEMORY);
        match make() {
            Err(e) => {
                assert_eq!(e, Failed::OUT_OF_DEVICE_MEMORY, "{what}: step {failed}");
                assert_eq!(
                    gpu.objects_live(),
                    before,
                    "{what}: objects left by a failure at step {failed}"
                );
                failed += 1;
            }
            Ok(made) => {
                unmake(made);
                assert_eq!(gpu.objects_live(), before, "{what}: given back whole");
                return failed;
            }
        }
        assert!(failed <= 1000, "{what}: a step past every call");
    }
}

/// The fallible device calls a host-visible buffer is made through: its
/// memory, the binding and the mapping.
const BUFFER_STEPS: u32 = 3;

/// **The sheets that fail part way leave nothing made**: each of the
/// curves' buffer and the two transparent texels' staging buffers forced to
/// fail at each step, and every buffer made before it destroyed. The
/// controls: whole sheets hold objects, and the steps failed are three
/// buffers' each. Named break: a buffer whose memory, binding or mapping
/// fails left alive, or sheets dropped without being destroyed.
fn sheets_fail_whole(gpu: &Gpu) {
    assert_eq!(
        gpu.objects_live(),
        Live(0),
        "the control: nothing made before"
    );
    let failed = each_step_fails(
        gpu,
        "sheets",
        || gpu.sheets(&baud_domain::contrast::CURVES[..]),
        |s| {
            assert!(gpu.objects_live().0 > 0, "the control: sheets hold objects");
            s.destroy(gpu);
        },
    );
    assert_eq!(failed, 3 * BUFFER_STEPS, "every step failed once");
}

/// **A presenter that fails part way leaves nothing made**: each step of
/// its cell memory and of its sheets forced to fail in turn, and every
/// device object made before it destroyed. The controls: a whole presenter
/// holds objects, and the steps failed are the cell memory's on its arm and
/// the sheets'. Named break: the cell memory left alive when the sheets
/// fail.
fn presenter_fails_whole(gpu: &Gpu) {
    const WS: WinSize = WinSize {
        ws_col: 24,
        ws_row: 6,
    };
    let mut owned = Owned::new(&WS);
    let t = Term::new(owned.storage(), &WS);
    let (_, rows) = t.geometry();
    let need = t.stride().index()
        * (rows.index() + 1)
        * core::mem::size_of::<baud_core::PackedCell>()
        * SETS;
    let arm = gpu.arm(u64::try_from(need).expect("a size"));
    assert_eq!(
        gpu.objects_live(),
        Live(0),
        "the control: nothing made before"
    );
    let failed = each_step_fails(
        gpu,
        "presenter",
        || Presenter::new(gpu, &t),
        |p| {
            assert!(
                gpu.objects_live().0 > 0,
                "the control: a presenter holds objects"
            );
            p.destroy(gpu);
        },
    );
    assert_eq!(
        failed,
        steps(gpu, arm) + 3 * BUFFER_STEPS,
        "{arm:?}: every step failed once"
    );
}

/// **A read that fails part way leaves nothing made**: its buffer's
/// memory, binding and mapping and the submission forced to fail in turn,
/// and the buffer destroyed after each. The controls: the read whole is
/// every pixel, and the steps failed are a buffer's and the submission.
/// Named break: the buffer left alive when its memory or binding fails.
fn read_fails_whole(gpu: &Gpu) {
    let t = gpu.target(Px(8), Px(4)).expect("a target");
    assert_eq!(
        gpu.objects_live(),
        Live(0),
        "the control: nothing made before"
    );
    let failed = each_step_fails(
        gpu,
        "read",
        || gpu.read(&t),
        |words| assert_eq!(words.len(), 8 * 4, "the control: every pixel"),
    );
    assert_eq!(failed, BUFFER_STEPS + 1, "every step failed once");
    gpu.drop_target(t);
}

#[test]
fn sheets_that_fail_part_way_leave_nothing_on_lavapipe() {
    sheets_fail_whole(&common::lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn sheets_that_fail_part_way_leave_nothing_on_radv() {
    sheets_fail_whole(&common::radv());
}

#[test]
fn a_presenter_that_fails_part_way_leaves_nothing_on_lavapipe() {
    presenter_fails_whole(&common::lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_presenter_that_fails_part_way_leaves_nothing_on_radv() {
    presenter_fails_whole(&common::radv());
}

#[test]
fn a_read_that_fails_part_way_leaves_nothing_on_lavapipe() {
    read_fails_whole(&common::lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_read_that_fails_part_way_leaves_nothing_on_radv() {
    read_fails_whole(&common::radv());
}
