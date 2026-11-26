//! **The cell path's lent memory, on each arm**: what the CPU writes
//! through the slice is what the stage reads once a frame has settled it.
//! In place that is every write; under staging it is the ranges owed and
//! no others, and the bytes copied are those ranges' and no more. Run on
//! each arm on lavapipe and on RADV.
//!
//! **Cell memory that fails part way leaves nothing made**: each step of
//! [`CellMemory::new`] forced to fail in turn, on each arm, and every
//! device object made before it destroyed when it returns; the same of the
//! sheets, of a presenter, of the pipelines and one built, of a frame
//! drawn on the instance path, and of a frame read back.
//!
//! **A 4K screen's copies are lent on the machine's committed arm**: the arm
//! is a fact about the machine -- its GPU, and the firmware that sets how
//! much of the GPU's memory the CPU sees -- so it is committed per GPU and
//! memory class in `tests/machines/<vendor>-<device>-<class>.txt`, keyed by
//! the GPU's PCI vendor and device ids and the class [`memory_class`] names,
//! and a presenter's arm at 4K is held to it on RADV.

#![cfg(target_os = "linux")]

mod common;

use ash::vk;
use baud_core::{Owned, Term};
use baud_domain::WinSize;
use baud_present_vulkan::cells::{CellMemory, CellSizes};
use baud_present_vulkan::draw::{Pass, Records, Scene, Texel, Texels};
use baud_present_vulkan::frame::{Failed, Gpu};
use baud_present_vulkan::hooks::{Calls, Live};
use baud_present_vulkan::lend::{Arm, Elems, Uploaded};
use baud_present_vulkan::pipeline::{Blend, Fragment, Pipelines};
use baud_present_vulkan::present::Presenter;
use baud_present_vulkan::vk::Has;
use baud_present_wayland::Px;
use baud_render::screens::SETS;
use std::path::{Path, PathBuf};

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
    let pipes = Pipelines::new(gpu).expect("the pipelines");
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
                gpu.fail_none();
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
    assert_eq!(
        before,
        Live(PIPELINES_HOLD),
        "the control: the pipelines' objects alone made before"
    );
    pipes.destroy();
    assert_eq!(gpu.objects_live(), Live(0), "the pipelines given back");
}

/// The objects pipelines hold before any is built: the four set layouts,
/// the two pipeline layouts, the render pass and the cache.
const PIPELINES_HOLD: usize = 8;

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
                gpu.fail_none();
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

/// The fallible device calls an image is made through: its memory, the
/// binding and the view.
const IMAGE_STEPS: u32 = 3;

/// The fallible device calls the sheets are made through: the sampler, the
/// two images, the curves' buffer and the two transparent texels' staging
/// buffers.
const SHEETS_STEPS: u32 = 1 + 2 * IMAGE_STEPS + 3 * BUFFER_STEPS;

/// **The sheets that fail part way leave nothing made**: the sampler, each
/// of the two images, the curves' buffer and the two transparent texels'
/// staging buffers forced to fail at each step, and every object made
/// before it destroyed. The controls: whole sheets hold objects, and the
/// steps failed are the sheets' every one. Named break: the sampler or the
/// first image left alive when a later step fails, a buffer whose memory,
/// binding or mapping fails left alive, or sheets dropped without being
/// destroyed.
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
    assert_eq!(failed, SHEETS_STEPS, "every step failed once");
}

/// **A presenter that fails part way leaves nothing made**: each step of
/// its cell memory and of its sheets forced to fail in turn, and every
/// device object made before it destroyed. The controls: a whole presenter
/// holds objects, and the steps failed are the cell memory's on its arm and
/// the sheets'. Named break: the cell memory left alive when the sheets
/// fail, or the pipelines when anything after them does.
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
        PIPELINES_STEPS + steps(gpu, arm) + SHEETS_STEPS,
        "{arm:?}: every step failed once"
    );
}

/// The fallible device calls pipelines are made through: the four set
/// layouts, the two pipeline layouts, the render pass and the cache.
const PIPELINES_STEPS: u32 = 8;

/// The fallible device calls a pipeline is built through: its two shader
/// modules and the pipeline.
const BUILD_STEPS: u32 = 3;

/// **Pipelines that fail part way leave nothing made**: each of their set
/// layouts, pipeline layouts, render pass and cache, then each of one
/// pipeline's shader modules and the pipeline, forced to fail in turn, and
/// every object made before it destroyed. The controls: pipelines with one
/// built hold the pipelines' objects and the pipeline, and the steps failed
/// are those every one. Named break: a set layout, layout or the pass left
/// alive when a later one fails, or the vertex stage's module when the
/// fragment stage's fails.
fn pipelines_fail_whole(gpu: &Gpu) {
    assert_eq!(
        gpu.objects_live(),
        Live(0),
        "the control: nothing made before"
    );
    let failed = each_step_fails(
        gpu,
        "pipelines",
        || {
            let mut pipes = Pipelines::new(gpu)?;
            match pipes.get(Fragment::Glyph, Blend::Normal) {
                Ok(_) => Ok(pipes),
                Err(e) => {
                    assert_eq!(
                        gpu.objects_live(),
                        Live(PIPELINES_HOLD),
                        "pipelines: a failed build leaves no module"
                    );
                    pipes.destroy();
                    Err(e)
                }
            }
        },
        |pipes| {
            assert_eq!(
                gpu.objects_live(),
                Live(PIPELINES_HOLD + 1),
                "the control: the pipelines' objects and the one built"
            );
            pipes.destroy();
        },
    );
    assert_eq!(
        failed,
        PIPELINES_STEPS + BUILD_STEPS,
        "every step failed once"
    );
}

#[test]
fn pipelines_that_fail_part_way_leave_nothing_on_lavapipe() {
    pipelines_fail_whole(&common::lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn pipelines_that_fail_part_way_leave_nothing_on_radv() {
    pipelines_fail_whole(&common::radv());
}

/// The fallible device calls a texture is uploaded through: its image's,
/// its staging buffer's and the copy's submission.
const TEXTURE_STEPS: u32 = IMAGE_STEPS + BUFFER_STEPS + 1;

/// The fallible device calls a whole frame without framing or an effect
/// is drawn through, its pipelines built: the `U` buffer, the records'
/// buffer, the backgrounds' texture, the descriptor pool, its sets, the
/// target's view, the framebuffer and the submission.
const DRAW_STEPS: u32 = 2 * BUFFER_STEPS + TEXTURE_STEPS + 4 + 1;

/// **A frame that fails part way leaves nothing made**: each step of a
/// whole frame on the instance path forced to fail in turn, and every
/// device object made before it destroyed. The controls: the frame drawn
/// whole after them is the frame drawn before them pixel for pixel and
/// leaves the objects as they were, and the steps failed are a whole
/// frame's every one. Named break: a buffer, the backgrounds' texture, the
/// pool or the view left alive when a later step fails, or the texture's
/// image when its staging buffer fails.
fn draw_fails_whole(gpu: &Gpu) {
    let mut pipes = Pipelines::new(gpu).expect("the pipelines");
    let t = gpu.target(Px(16), Px(8)).expect("a target");
    let bg: Vec<u8> = (0..8u32)
        .flat_map(|i| (0xff00_0000 | (i * 0x0010_2030)).to_le_bytes())
        .collect();
    let mask = [0x80u8; 64];
    let curves: Vec<u8> = (0..=255u8).collect();
    let sheets = common::sheets(
        gpu,
        Texels {
            width: Px(8),
            height: Px(8),
            texel: Texel::Coverage,
            bytes: &mask,
        },
        Texels {
            width: Px(1),
            height: Px(1),
            texel: Texel::Argb,
            bytes: &[0u8; 4],
        },
        &curves,
    );
    let records = common::record((4, 2), (8, 8), (0, 0), (0xfff0_e010, 0));
    let scene = Scene {
        u: [16.0, 8.0, 4.0, 4.0, 4.0, 2.0, 8.0, 8.0, 1.0, 1.0, 0.0, 0.0],
        backgrounds: Texels {
            width: Px(4),
            height: Px(2),
            texel: Texel::Argb,
            bytes: &bg,
        },
        sheets: &sheets,
        records: &records,
        count: Records(1),
        framing: None,
    };
    gpu.draw(&mut pipes, &t, &scene, Pass::Whole)
        .expect("the frame before");
    let want = gpu.read(&t).expect("the frame before, read back");
    let failed = each_step_fails(
        gpu,
        "draw",
        || gpu.draw(&mut pipes, &t, &scene, Pass::Whole),
        |()| (),
    );
    assert_eq!(failed, DRAW_STEPS, "every step failed once");
    assert_eq!(
        gpu.read(&t).expect("the frame after, read back"),
        want,
        "the control: the frame drawn whole is the frame before"
    );
    sheets.destroy(gpu);
    pipes.destroy();
    gpu.drop_target(t);
    assert_eq!(gpu.objects_live(), Live(0), "everything given back");
}

#[test]
fn a_frame_that_fails_part_way_leaves_nothing_on_lavapipe() {
    draw_fails_whole(&common::lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_frame_that_fails_part_way_leaves_nothing_on_radv() {
    draw_fails_whole(&common::radv());
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

/// The largest 4K shape: 3840x2160 at an eight by sixteen cell, less a
/// margin.
const FOUR_K: WinSize = WinSize {
    ws_col: 478,
    ws_row: 135,
};

fn arm_word(arm: Arm) -> &'static str {
    match arm {
        Arm::InPlace => "in place",
        Arm::Staging => "staging",
    }
}

/// **The arm a machine's file commits**: its one line, `arm: in place` or
/// `arm: staging`; anything else is no arm.
fn committed(text: &str) -> Option<Arm> {
    match text.lines().collect::<Vec<_>>()[..] {
        ["arm: in place"] => Some(Arm::InPlace),
        ["arm: staging"] => Some(Arm::Staging),
        _ => None,
    }
}

/// **Whether `live`, the arm `device` took, is the arm committed at
/// `path`**, whose text is `text` where the file exists. A machine with no
/// file, or a file that is not one arm line, fails naming the file to
/// commit; a different arm fails naming both.
fn judged(path: &Path, text: Option<&str>, device: &str, live: Arm) -> Result<(), String> {
    let add = format!(
        "commit {} holding one line, `arm: in place` or `arm: staging`: the arm {device}'s \
         memory on this machine is to take for a 4K screen's copies, from its memory types",
        path.display()
    );
    match text.map(committed) {
        None => Err(format!(
            "no arm is committed for this GPU and memory class: {add} (it took {})",
            arm_word(live)
        )),
        Some(None) => Err(format!("{} is not one arm line: {add}", path.display())),
        Some(Some(want)) if want == live => Ok(()),
        Some(Some(want)) => Err(format!(
            "{device} took {} where {} commits {}",
            arm_word(live),
            path.display(),
            arm_word(want)
        )),
    }
}

/// MiB, in bytes.
const MIB: vk::DeviceSize = 1 << 20;

/// **How much of the GPU's own memory the CPU sees, as a word**: `unified`
/// where the GPU has no memory of its own (an integrated GPU, or the CPU
/// itself); otherwise from the largest device-local heap with a memory type
/// that is device-local and host-visible -- `bar-full` where that heap is
/// as large as the largest device-local heap (a resizable BAR: the CPU sees
/// all of it), `bar-<n>m` where it is a window of n MiB (a fixed BAR,
/// commonly 256), and `bar-none` where no such type exists. The window's
/// size is named rather than "small" so that two windows never share a
/// file. The firmware's BAR is what moves one GPU's memory types between
/// machines, and the arm depends on those types.
fn memory_class(
    kind: vk::PhysicalDeviceType,
    props: &vk::PhysicalDeviceMemoryProperties,
) -> String {
    if kind == vk::PhysicalDeviceType::INTEGRATED_GPU || kind == vk::PhysicalDeviceType::CPU {
        return "unified".to_owned();
    }
    let heaps = &props.memory_heaps[..usize::try_from(props.memory_heap_count)
        .unwrap_or(0)
        .min(props.memory_heaps.len())];
    let types = &props.memory_types[..usize::try_from(props.memory_type_count)
        .unwrap_or(0)
        .min(props.memory_types.len())];
    let local = heaps
        .iter()
        .filter(|h| h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
        .map(|h| h.size)
        .max()
        .unwrap_or(0);
    let mapped = vk::MemoryPropertyFlags::DEVICE_LOCAL | vk::MemoryPropertyFlags::HOST_VISIBLE;
    let seen = types
        .iter()
        .filter(|t| t.property_flags.contains(mapped))
        .filter_map(|t| heaps.get(usize::try_from(t.heap_index).ok()?))
        .map(|h| h.size)
        .max();
    match seen {
        None => "bar-none".to_owned(),
        Some(n) if n >= local => "bar-full".to_owned(),
        Some(n) => format!("bar-{}m", n / MIB),
    }
}

/// **This machine's file, and the device's name**: the GPU's PCI vendor
/// and device ids, in hex, and its [`memory_class`].
#[allow(unsafe_code)]
fn machine_file(gpu: &Gpu) -> (PathBuf, String) {
    // SAFETY: a physical device of this instance.
    let (p, mem) = unsafe {
        let i = &gpu.vulkan.instance;
        (
            i.get_physical_device_properties(gpu.physical),
            i.get_physical_device_memory_properties(gpu.physical),
        )
    };
    let name = p
        .device_name_as_c_str()
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = format!(
        "{:04x}-{:04x}-{}.txt",
        p.vendor_id,
        p.device_id,
        memory_class(p.device_type, &mem)
    );
    (
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/machines")
            .join(file),
        name,
    )
}

/// Memory of `heaps`, each `(bytes, device-local)`, and `types`, each
/// `(heap, device-local and host-visible)`.
fn memory(
    heaps: &[(vk::DeviceSize, bool)],
    types: &[(u32, bool)],
) -> vk::PhysicalDeviceMemoryProperties {
    let mut p = vk::PhysicalDeviceMemoryProperties::default();
    for (i, &(size, local)) in heaps.iter().enumerate() {
        p.memory_heaps[i] = vk::MemoryHeap {
            size,
            flags: match local {
                true => vk::MemoryHeapFlags::DEVICE_LOCAL,
                false => vk::MemoryHeapFlags::empty(),
            },
        };
    }
    for (i, &(heap, mapped)) in types.iter().enumerate() {
        p.memory_types[i] = vk::MemoryType {
            heap_index: heap,
            property_flags: match mapped {
                true => {
                    vk::MemoryPropertyFlags::DEVICE_LOCAL | vk::MemoryPropertyFlags::HOST_VISIBLE
                }
                false => vk::MemoryPropertyFlags::DEVICE_LOCAL,
            },
        };
    }
    p.memory_heap_count = u32::try_from(heaps.len()).expect("heaps");
    p.memory_type_count = u32::try_from(types.len()).expect("types");
    p
}

/// **A memory class names how much of the GPU's memory the CPU sees**:
/// all of a 16 GiB heap is `bar-full`, a 256 MiB window beside it is
/// `bar-256m`, no mapped local type is `bar-none`, and an integrated GPU
/// or the CPU is `unified` whatever its heaps.
#[test]
fn a_memory_class_names_how_much_of_the_gpu_s_memory_the_cpu_sees() {
    let discrete = vk::PhysicalDeviceType::DISCRETE_GPU;
    let vram = 16 << 30;
    let host = (32 << 30, false);
    let full = memory(&[(vram, true), host], &[(0, false), (0, true)]);
    let window = memory(
        &[(vram, true), host, (256 * MIB, true)],
        &[(0, false), (2, true)],
    );
    let none = memory(&[(vram, true), host], &[(0, false)]);
    assert_eq!(memory_class(discrete, &full), "bar-full");
    assert_eq!(memory_class(discrete, &window), "bar-256m");
    assert_eq!(memory_class(discrete, &none), "bar-none");
    for kind in [
        vk::PhysicalDeviceType::INTEGRATED_GPU,
        vk::PhysicalDeviceType::CPU,
    ] {
        assert_eq!(memory_class(kind, &window), "unified");
    }
}

/// **A machine's arm passes only as its committed line**: the arm
/// committed passes; the other arm, a file that is not one arm line, and no
/// file at all each fail, the last two naming the file to commit.
#[test]
fn a_machine_s_arm_passes_only_as_its_committed_line() {
    let path = Path::new("tests/machines/1002-7550-bar-full.txt");
    let device = "a GPU";
    for (arm, other) in [(Arm::Staging, Arm::InPlace), (Arm::InPlace, Arm::Staging)] {
        let line = format!("arm: {}\n", arm_word(arm));
        assert_eq!(judged(path, Some(&line), device, arm), Ok(()), "{line}");
        let wrong = judged(path, Some(&line), device, other).expect_err("the other arm");
        assert!(wrong.contains("commits"), "{wrong}");
    }
    for text in [
        None,
        Some(""),
        Some("arm: cached\n"),
        Some("arm: staging\narm: staging\n"),
    ] {
        let e = judged(path, text, device, Arm::Staging).expect_err("no arm committed");
        assert!(
            e.contains("commit tests/machines/1002-7550-bar-full.txt"),
            "{e}"
        );
    }
}

/// **A 4K screen's copies are lent on this machine's committed arm**: a
/// presenter for [`FOUR_K`] is made, and the arm its cell memory took is
/// the one `tests/machines/` commits for this GPU and memory class. The arm is
/// read from the presenter, never from the memory properties the choice
/// reads. An RX 9070 XT under RADV stages: its device-local memory the CPU
/// sees is uncached. Another machine -- an integrated GPU on unified
/// memory, say -- adds its file when this fails there naming it, with the
/// arm its memory is to take read from the device's memory types: `arm: in
/// place` where some device-local type is host-visible, coherent and cached.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_4k_screen_s_copies_are_lent_on_the_machine_s_committed_arm_on_radv() {
    let gpu = common::radv();
    let (path, device) = machine_file(&gpu);
    let mut owned = Owned::new(&FOUR_K);
    let t = Term::new(owned.storage(), &FOUR_K);
    let p = Presenter::new(&gpu, &t).expect("a presenter at 4K");
    let live = p.arm();
    p.destroy(&gpu);
    let text = std::fs::read_to_string(&path).ok();
    eprintln!("{device}: {} at 4K, {}", arm_word(live), path.display());
    if let Err(e) = judged(&path, text.as_deref(), &device, live) {
        panic!("{e}");
    }
}
