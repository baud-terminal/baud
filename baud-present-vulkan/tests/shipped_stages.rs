//! **Every stage the crate ships is bound by a layout it makes**: each
//! compiled stage in `shaders/spv` declares only what some pipeline layout
//! [`Pipelines::new`] hands the driver binds -- every resource at its set
//! and binding, of its kind, visible to its stage, and a push constant
//! inside a range that stage reads -- read off the layouts as the device
//! is given them, not off the crate's own tables, so a stage committed
//! with no pipeline that can draw it fails here.

#![cfg(target_os = "linux")]

mod spirv;

use ash::vk;
use baud_present_vulkan::choose::{Kind, SyncObj, choose};
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::pipeline::Pipelines;
use baud_present_vulkan::vk::Vulkan;
use baud_present_wayland::dmabuf::{
    Count, Device, Feedback, Format, Fourcc, Modifier, Tranche, TrancheFlags,
};
use spirv::{Binding, Bytes, Class, DescriptorSet, declared, push_bytes};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// A descriptor set layout as the device was given it: the handle it
/// made, and each binding's number, kind and stages.
struct SetMade {
    handle: vk::DescriptorSetLayout,
    bindings: Vec<(Binding, vk::DescriptorType, vk::ShaderStageFlags)>,
}

/// A pipeline layout as the device was given it: its set layouts in set
/// order, and its push-constant ranges.
struct LayoutMade {
    sets: Vec<vk::DescriptorSetLayout>,
    push: Vec<vk::PushConstantRange>,
}

/// Every layout made through [`watched`]'s device.
struct Made {
    sets: Vec<SetMade>,
    layouts: Vec<LayoutMade>,
}

static MADE: Mutex<Made> = Mutex::new(Made {
    sets: Vec::new(),
    layouts: Vec::new(),
});

/// The driver's own entry points the watched ones pass the call to.
struct Driver {
    set_layout: vk::PFN_vkCreateDescriptorSetLayout,
    pipeline_layout: vk::PFN_vkCreatePipelineLayout,
}

static DRIVER: OnceLock<Driver> = OnceLock::new();

/// `vkCreateDescriptorSetLayout`, passed to the driver and the layout it
/// made kept.
#[allow(unsafe_code)]
unsafe extern "system" fn create_set_layout(
    device: vk::Device,
    info: *const vk::DescriptorSetLayoutCreateInfo<'_>,
    alloc: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::DescriptorSetLayout,
) -> vk::Result {
    let driver = DRIVER.get().expect("the driver's entry points");
    // SAFETY: the crate's call, passed on as it was made.
    let made = unsafe { (driver.set_layout)(device, info, alloc, out) };
    if made == vk::Result::SUCCESS {
        // SAFETY: the create info the driver has just read, its bindings
        // `binding_count` long, and the handle it wrote.
        let (info, handle) = unsafe { (&*info, *out) };
        let bindings = match usize::try_from(info.binding_count).expect("a count") {
            0 => &[][..],
            // SAFETY: as above.
            n => unsafe { std::slice::from_raw_parts(info.p_bindings, n) },
        };
        let bindings = bindings
            .iter()
            .map(|b| (Binding(b.binding), b.descriptor_type, b.stage_flags))
            .collect();
        MADE.lock()
            .expect("the layouts made")
            .sets
            .push(SetMade { handle, bindings });
    }
    made
}

/// `vkCreatePipelineLayout`, passed to the driver and the layout it made
/// kept.
#[allow(unsafe_code)]
unsafe extern "system" fn create_pipeline_layout(
    device: vk::Device,
    info: *const vk::PipelineLayoutCreateInfo<'_>,
    alloc: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::PipelineLayout,
) -> vk::Result {
    let driver = DRIVER.get().expect("the driver's entry points");
    // SAFETY: the crate's call, passed on as it was made.
    let made = unsafe { (driver.pipeline_layout)(device, info, alloc, out) };
    if made == vk::Result::SUCCESS {
        // SAFETY: the create info the driver has just read, each list as
        // long as its count says.
        let info = unsafe { &*info };
        let sets = match usize::try_from(info.set_layout_count).expect("a count") {
            0 => Vec::new(),
            // SAFETY: as above.
            n => unsafe { std::slice::from_raw_parts(info.p_set_layouts, n) }.to_vec(),
        };
        let push = match usize::try_from(info.push_constant_range_count).expect("a count") {
            0 => Vec::new(),
            // SAFETY: as above.
            n => unsafe { std::slice::from_raw_parts(info.p_push_constant_ranges, n) }.to_vec(),
        };
        MADE.lock()
            .expect("the layouts made")
            .layouts
            .push(LayoutMade { sets, push });
    }
    made
}

/// **`gpu`'s device with its layout calls watched**: every entry point the
/// driver's, but `vkCreateDescriptorSetLayout` and `vkCreatePipelineLayout`,
/// which keep what they are given in [`MADE`].
#[allow(unsafe_code)]
fn watched(gpu: &Gpu) -> ash::Device {
    let fns = gpu.device.fp_v1_0();
    DRIVER.get_or_init(|| Driver {
        set_layout: fns.create_descriptor_set_layout,
        pipeline_layout: fns.create_pipeline_layout,
    });
    let handle = gpu.device.handle();
    let proc_addr = gpu.vulkan.instance.fp_v1_0().get_device_proc_addr;
    let set_layout: vk::PFN_vkCreateDescriptorSetLayout = create_set_layout;
    let pipeline_layout: vk::PFN_vkCreatePipelineLayout = create_pipeline_layout;
    // SAFETY: `handle` is `gpu`'s device, alive while `gpu` is, and every
    // entry point is the driver's for it or one of the two above, of the
    // same signature, which call the driver's.
    unsafe {
        ash::Device::load_with(
            |name| match name.to_bytes() {
                b"vkCreateDescriptorSetLayout" => set_layout as *const c_void,
                b"vkCreatePipelineLayout" => pipeline_layout as *const c_void,
                _ => proc_addr(handle, name.as_ptr())
                    .map_or(std::ptr::null(), |f| f as *const c_void),
            },
            handle,
        )
    }
}

/// Lavapipe, offered ARGB8888 on LINEAR.
fn lavapipe() -> Gpu {
    let vulkan = Vulkan::open().expect("libvulkan, pinned in the development shell");
    let (physical, cpu) = vulkan
        .candidates()
        .into_iter()
        .find(|(_, c)| c.kind == Kind::Cpu)
        .expect("lavapipe, pinned in the development shell");
    let offered = Feedback {
        main: Some(Device(0)),
        tranches: vec![Tranche {
            device: None,
            flags: TrancheFlags(0),
            formats: vec![Format {
                fourcc: Fourcc::ARGB8888,
                modifier: Modifier::LINEAR,
            }],
        }],
        unknown_indices: Count(0),
    };
    let choice =
        choose(core::slice::from_ref(&cpu), &offered, SyncObj::Absent).expect("lavapipe alone");
    Gpu::new(vulkan, physical, choice).expect("a device")
}

/// The descriptor kind a resource of `class` is bound as.
fn kind(class: Class) -> vk::DescriptorType {
    match class {
        Class::Sampler => vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
        Class::Uniform => vk::DescriptorType::UNIFORM_BUFFER,
        Class::Storage => vk::DescriptorType::STORAGE_BUFFER,
    }
}

/// **What `layout` does not bind of a stage's declarations**: each of
/// `resources` not at its set and binding, of its kind, visible to
/// `stage`, and a push constant of `push` bytes outside every range `stage`
/// reads; empty where the layout binds the stage.
fn unbound(
    layout: &LayoutMade,
    sets: &[SetMade],
    resources: &[(DescriptorSet, Binding, Class)],
    push: Option<Bytes>,
    stage: vk::ShaderStageFlags,
) -> Vec<String> {
    let bound = |(set, binding, class): &(DescriptorSet, Binding, Class)| {
        let Some(handle) = layout
            .sets
            .iter()
            .zip(0..)
            .find_map(|(h, n)| (n == set.0).then_some(*h))
        else {
            return false;
        };
        sets.iter()
            .filter(|s| s.handle == handle)
            .flat_map(|s| &s.bindings)
            .any(|(b, k, stages)| *b == *binding && *k == kind(*class) && stages.contains(stage))
    };
    let pushed = |Bytes(n): Bytes| {
        layout
            .push
            .iter()
            .any(|r| r.stage_flags.contains(stage) && r.offset == 0 && r.size >= n)
    };
    resources
        .iter()
        .filter(|r| !bound(r))
        .map(|(s, b, c)| format!("{}.{} {c:?}", s.0, b.0))
        .chain(
            push.filter(|p| !pushed(*p))
                .map(|Bytes(n)| format!("a {n}-byte push constant")),
        )
        .collect()
}

/// **Every compiled stage in `shaders/spv` is bound by a pipeline layout
/// the crate gives the device**: the instance path's stages by its
/// layout, and the cell path's four vertex stages -- six buffers at set 0,
/// the grid, renditions, glyph map, overlay, `CU` as a uniform and the
/// band bases, and a 4-byte push constant -- by the cell layout. Named
/// break: the cell layout not made, or a binding of its set 0 changed in
/// kind or stage, which leaves the cell stages unbound, each named with
/// what the nearest layout lacks.
#[test]
fn every_shipped_stage_is_bound_by_a_layout_the_crate_makes() {
    let gpu = lavapipe();
    let device = watched(&gpu);
    let pipes = Pipelines::new(&device).expect("the pipelines");
    pipes.destroy();
    let made = MADE.lock().expect("the layouts made");
    assert!(!made.layouts.is_empty(), "no pipeline layout was watched");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders/spv");
    let mut stages: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "spv"))
        .collect();
    stages.sort();
    assert!(
        stages.iter().any(|p| p.ends_with("glyph.vert.spv")),
        "the instance path's vertex stage is among the stages read"
    );
    let mut missing = Vec::new();
    for path in &stages {
        let spv = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let d = declared(&spv);
        let stage = match d.model {
            0 => vk::ShaderStageFlags::VERTEX,
            4 => vk::ShaderStageFlags::FRAGMENT,
            m => panic!("{}: execution model {m}", path.display()),
        };
        let push = push_bytes(&spv);
        let nearest = made
            .layouts
            .iter()
            .map(|l| unbound(l, &made.sets, &d.resources, push, stage))
            .min_by_key(Vec::len)
            .expect("a layout");
        if !nearest.is_empty() {
            missing.push(format!(
                "{} (stage {}): {}",
                path.file_name().expect("a name").to_string_lossy(),
                stage.as_raw(),
                nearest.join(", ")
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "stages no pipeline layout the crate makes binds, each with what the \
         nearest layout lacks:\n{}",
        missing.join("\n")
    );
}
