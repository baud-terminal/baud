//! **The instance and cell paths' pipelines**: one render pass over a
//! frame's target, the descriptor layouts every draw binds, and a pipeline
//! a fragment stage and a blend, built once and kept.
//!
//! The instance path's layout, [`INSTANCE_SETS`]:
//!
//! | set.binding | what | stages |
//! |---|---|---|
//! | 0.0 | the records, `Inst[]` | vertex |
//! | 0.1 | `U` | vertex, fragment |
//! | 1.0 | the mask atlas | fragment |
//! | 1.1 | the colour atlas | fragment |
//! | 1.2 | the contrast curves, bytes | fragment |
//! | 1.3 | the cells' backgrounds, a texel a cell | fragment |
//! | 2.0 | an effect's `EffectU` | fragment |
//!
//! The cell path's, [`CELL_SETS`], its own so no effect draw carries its
//! buffers, and a [`CELL_PUSH`]-byte push constant, the part a pass draws,
//! read by the vertex stages:
//!
//! | set.binding | what | stages |
//! |---|---|---|
//! | 0.0 | the grid, a `PackedCell` a `uvec2` | vertex |
//! | 0.1 | the renditions, nine words a slot | vertex |
//! | 0.2 | the glyph map, six words an entry | vertex |
//! | 0.3 | the overlay, a byte a cell | vertex |
//! | 0.4 | `CU` | vertex |
//! | 0.5 | each draw row's first rendition slot | vertex |
//! | 1.x | the instance path's set 1 | fragment |
//!
//! Every output is premultiplied, and every blend's alpha is the
//! premultiplied over. [`Pipelines::get`] builds on the calling thread, for
//! the pipelines built before the first frame; [`Pipelines::ready`] never
//! blocks one, building on a worker from the same cache and answering
//! `None` until the build is done.

use crate::frame::Failed;
use ash::vk;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

/// A shader stage's SPIR-V, as committed beside its source.
struct Spirv(&'static [u8]);

/// Each stage's SPIR-V, one include a file, each path written out so the
/// credits scan reads what the binary embeds (`xtask credits`).
const BG_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/bg.frag.spv"
)));
const BG_VERT: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/bg.vert.spv"
)));
const CELL_BG_VERT: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/cell_bg.vert.spv"
)));
const CELL_GLYPH_VERT: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/cell_glyph.vert.spv"
)));
const CELL_RING_VERT: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/cell_ring.vert.spv"
)));
const CELL_SHAPE_VERT: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/cell_shape.vert.spv"
)));
const FIREWORKS_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/fireworks.frag.spv"
)));
const FIREWORKS_VERT: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/fireworks.vert.spv"
)));
const FLAT_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/flat.frag.spv"
)));
const FX_FIRE_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/fx_fire.frag.spv"
)));
const FX_ICE_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/fx_ice.frag.spv"
)));
const FX_LIGHTNING_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/fx_lightning.frag.spv"
)));
const FX_LIQUID_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/fx_liquid.frag.spv"
)));
const FX_ORB_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/fx_orb.frag.spv"
)));
const FX_PLASMA_FACTOR_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/fx_plasma_factor.frag.spv"
)));
const FX_PLASMA_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/fx_plasma.frag.spv"
)));
const FX_PLASMA_MULTIPLY_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/fx_plasma_multiply.frag.spv"
)));
const FX_UFO_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/fx_ufo.frag.spv"
)));
const GLYPH_FRAG: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/glyph.frag.spv"
)));
const GLYPH_VERT: Spirv = Spirv(include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/spv/glyph.vert.spv"
)));

/// A pipeline's vertex and fragment stages, by what they draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fragment {
    /// The cells' backgrounds, over the whole target (`bg_fragment`).
    Background,
    /// A record: a mask, a colour glyph or a flat colour
    /// (`glyph_fragment`).
    Glyph,
    /// An effect trail's records, by its own fragment over the records'
    /// vertex stage, reading its `EffectU` at set 2 (`fx_*.frag`).
    Effect(Fx),
    /// The fireworks' stars, by their own vertex and fragment stages and
    /// their `FwU` at set 2 (`fireworks.vert`, `fireworks.frag`).
    Fireworks,
    /// The cell path's backgrounds, a cell an instance (`cell_bg.vert`,
    /// `flat.frag`).
    CellBg,
    /// The cell path's glyphs (`cell_glyph.vert`, `glyph.frag`).
    CellGlyph,
    /// The cell path's lines and marks, the shape slots a cell
    /// (`cell_shape.vert`, `glyph.frag`).
    CellShape,
    /// The cell path's hollow cursor (`cell_ring.vert`, `glyph.frag`).
    CellRing,
}

/// Which pipeline layout a pipeline is built against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layout {
    Instance,
    Cell,
}

/// A descriptor's kind, as a layout binds it and a stage declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Descriptor {
    Sampler,
    Uniform,
    Storage,
}

impl Descriptor {
    fn vk(self) -> vk::DescriptorType {
        match self {
            Descriptor::Sampler => vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            Descriptor::Uniform => vk::DescriptorType::UNIFORM_BUFFER,
            Descriptor::Storage => vk::DescriptorType::STORAGE_BUFFER,
        }
    }
}

/// **One set's bindings**, in binding order: each its kind and the stages
/// that declare it.
pub type Set = &'static [(Descriptor, vk::ShaderStageFlags)];

const VERTEX: vk::ShaderStageFlags = vk::ShaderStageFlags::VERTEX;
const FRAGMENT: vk::ShaderStageFlags = vk::ShaderStageFlags::FRAGMENT;
const BOTH: vk::ShaderStageFlags =
    vk::ShaderStageFlags::from_raw(VERTEX.as_raw() | FRAGMENT.as_raw());

/// The instance path's sets, the first table at the head of this module.
pub const INSTANCE_SETS: [Set; 3] = [
    &[(Descriptor::Storage, VERTEX), (Descriptor::Uniform, BOTH)],
    &[
        (Descriptor::Sampler, FRAGMENT),
        (Descriptor::Sampler, FRAGMENT),
        (Descriptor::Storage, FRAGMENT),
        (Descriptor::Sampler, FRAGMENT),
    ],
    &[(Descriptor::Uniform, FRAGMENT)],
];

/// The cell path's sets: its own set 0 and the instance path's set 1, so
/// one set of atlases binds to both.
pub const CELL_SETS: [Set; 2] = [
    &[
        (Descriptor::Storage, VERTEX),
        (Descriptor::Storage, VERTEX),
        (Descriptor::Storage, VERTEX),
        (Descriptor::Storage, VERTEX),
        (Descriptor::Uniform, VERTEX),
        (Descriptor::Storage, VERTEX),
    ],
    INSTANCE_SETS[1],
];

/// The cell path's push constant in bytes, `part`, read by the vertex
/// stages; the instance path has none.
pub const CELL_PUSH: u32 = 4;

/// **An effect trail's fragment shader**, one an effect
/// (`baud_render::effect::Effect`) and the plasma's second for its
/// `multiply` mix, which writes the factor its backdrop is taken at as the
/// second source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fx {
    Fire,
    Plasma,
    PlasmaMultiply,
    /// The plasma's `multiply` factor alone, `1 - as + cs`, for the first of
    /// the two passes a device without a second source blends it in.
    PlasmaFactor,
    Lightning,
    Ice,
    Ufo,
    Orb,
    Liquid,
}

impl Fx {
    pub const ALL: [Fx; 9] = [
        Fx::Fire,
        Fx::Plasma,
        Fx::PlasmaMultiply,
        Fx::PlasmaFactor,
        Fx::Lightning,
        Fx::Ice,
        Fx::Ufo,
        Fx::Orb,
        Fx::Liquid,
    ];

    fn stage(self) -> Spirv {
        match self {
            Fx::Fire => FX_FIRE_FRAG,
            Fx::Plasma => FX_PLASMA_FRAG,
            Fx::PlasmaMultiply => FX_PLASMA_MULTIPLY_FRAG,
            Fx::PlasmaFactor => FX_PLASMA_FACTOR_FRAG,
            Fx::Lightning => FX_LIGHTNING_FRAG,
            Fx::Ice => FX_ICE_FRAG,
            Fx::Ufo => FX_UFO_FRAG,
            Fx::Orb => FX_ORB_FRAG,
            Fx::Liquid => FX_LIQUID_FRAG,
        }
    }
}

impl Fragment {
    fn stages(self) -> (Spirv, Spirv) {
        match self {
            Fragment::Background => (BG_VERT, BG_FRAG),
            Fragment::Glyph => (GLYPH_VERT, GLYPH_FRAG),
            Fragment::Effect(fx) => (GLYPH_VERT, fx.stage()),
            Fragment::Fireworks => (FIREWORKS_VERT, FIREWORKS_FRAG),
            Fragment::CellBg => (CELL_BG_VERT, FLAT_FRAG),
            Fragment::CellGlyph => (CELL_GLYPH_VERT, GLYPH_FRAG),
            Fragment::CellShape => (CELL_SHAPE_VERT, GLYPH_FRAG),
            Fragment::CellRing => (CELL_RING_VERT, GLYPH_FRAG),
        }
    }

    /// Every pipeline's stages, each fragment once.
    pub fn every() -> impl Iterator<Item = Fragment> {
        [
            Fragment::Background,
            Fragment::Glyph,
            Fragment::Fireworks,
            Fragment::CellBg,
            Fragment::CellGlyph,
            Fragment::CellShape,
            Fragment::CellRing,
        ]
        .into_iter()
        .chain(Fx::ALL.map(Fragment::Effect))
    }

    /// The layout its pipeline is built against.
    pub fn layout(self) -> Layout {
        match self {
            Fragment::Background | Fragment::Glyph | Fragment::Effect(_) | Fragment::Fireworks => {
                Layout::Instance
            }
            Fragment::CellBg | Fragment::CellGlyph | Fragment::CellShape | Fragment::CellRing => {
                Layout::Cell
            }
        }
    }

    /// Its vertex and fragment stages' SPIR-V, for the tests that hold a
    /// stage's declarations to its layout.
    pub fn spirv(self) -> [&'static [u8]; 2] {
        let (v, f) = self.stages();
        [v.0, f.0]
    }
}

/// How a draw's premultiplied output meets what is under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blend {
    /// Written in place of what is under it.
    Replace,
    /// The premultiplied over: `src + dst (1 - as)`.
    Normal,
    /// `src + dst (1 - src)`.
    Screen,
    /// W3C Compositing's `multiply` in premultiplied numbers, `cs (1 - ab)
    /// + cb (1 - as + cs)`, by a second source (`dualSrcBlend`): the
    /// fragment writes the colour and, as source 1, the factor `1 - as + cs`
    /// the backdrop is taken at ([`Fx::PlasmaMultiply`]).
    Multiply,
    /// `multiply`'s first pass where the device has no second source: the
    /// backdrop's colour times the factor the fragment writes
    /// ([`Fx::PlasmaFactor`]), its alpha kept for the second pass.
    MultiplyFactor,
    /// `multiply`'s second pass: the colour times `1 - ab`, added, and the
    /// alpha source-over.
    MultiplyAdd,
}

impl Blend {
    fn attachment(self) -> vk::PipelineColorBlendAttachmentState {
        use vk::BlendFactor as F;
        let all = vk::ColorComponentFlags::RGBA;
        let blend = |colour: (F, F), alpha: (F, F)| {
            vk::PipelineColorBlendAttachmentState::default()
                .blend_enable(true)
                .src_color_blend_factor(colour.0)
                .dst_color_blend_factor(colour.1)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(alpha.0)
                .dst_alpha_blend_factor(alpha.1)
                .alpha_blend_op(vk::BlendOp::ADD)
                .color_write_mask(all)
        };
        let over = (F::ONE, F::ONE_MINUS_SRC_ALPHA);
        match self {
            Blend::Replace => vk::PipelineColorBlendAttachmentState::default()
                .blend_enable(false)
                .color_write_mask(all),
            Blend::Normal => blend(over, over),
            Blend::Screen => blend((F::ONE, F::ONE_MINUS_SRC_COLOR), over),
            Blend::Multiply => blend((F::ONE_MINUS_DST_ALPHA, F::SRC1_COLOR), over),
            Blend::MultiplyFactor => blend((F::ZERO, F::SRC_COLOR), (F::ZERO, F::ONE)),
            Blend::MultiplyAdd => blend((F::ONE_MINUS_DST_ALPHA, F::ONE), over),
        }
    }
}

/// What a pipeline is keyed by.
type Key = (Fragment, Blend);

/// The layouts and the render pass every pipeline is built against: what
/// a worker needs to build one, and nothing it would have to share
/// mutably.
#[derive(Clone)]
struct Shape {
    device: ash::Device,
    cache: vk::PipelineCache,
    layout: vk::PipelineLayout,
    cell_layout: vk::PipelineLayout,
    pass: vk::RenderPass,
}

impl Shape {
    /// **One pipeline**: a four-vertex strip, no vertex input (the vertex
    /// stage reads its record by instance), a viewport and scissor set at
    /// draw time, and `blend`.
    #[allow(unsafe_code)]
    fn build(&self, (fragment, blend): Key) -> Result<vk::Pipeline, Failed> {
        let d = &self.device;
        let (vs, fs) = fragment.stages();
        let module = |s: &Spirv| -> Result<vk::ShaderModule, Failed> {
            let words =
                ash::util::read_spv(&mut std::io::Cursor::new(s.0)).map_err(|_| Failed::Spirv)?;
            let info = vk::ShaderModuleCreateInfo::default().code(&words);
            // SAFETY: valid SPIR-V words (spirv-val, `shaders_reproduce`).
            Ok(unsafe { d.create_shader_module(&info, None) }?)
        };
        let (vm, fm) = (module(&vs)?, module(&fs)?);
        let entry = c"main";
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vm)
                .name(entry),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fm)
                .name(entry),
        ];
        let input = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_STRIP);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let attachments = [blend.attachment()];
        let blending = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        let dynamic = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic);
        let info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .color_blend_state(&blending)
            .dynamic_state(&dynamic)
            .layout(match fragment.layout() {
                Layout::Instance => self.layout,
                Layout::Cell => self.cell_layout,
            })
            .render_pass(self.pass)
            .subpass(0)];
        // SAFETY: every handle is this device's; the cache is internally
        // synchronised (it was not made externally synchronised), so a
        // worker and the frame may build through it at once.
        let made = unsafe { d.create_graphics_pipelines(self.cache, &info, None) };
        // SAFETY: the modules are not used past pipeline creation.
        unsafe {
            d.destroy_shader_module(vm, None);
            d.destroy_shader_module(fm, None);
        }
        made.map(|p| p[0]).map_err(|(_, r)| Failed::Vulkan(r))
    }
}

/// A build on the worker, and the gate a test holds it at.
struct Building {
    done: Receiver<(Key, Result<vk::Pipeline, Failed>)>,
    /// The read end of the worker's wake: a byte for each build handed back,
    /// so the loop waiting on it wakes for a finished build ([`Pipelines::ready_fd`]).
    woken: Option<UnixStream>,
    jobs: Sender<Key>,
    worker: Option<JoinHandle<()>>,
    #[cfg(any(test, feature = "test-hooks"))]
    gate: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}

/// **The pipelines a frame draws with**, each a fragment stage and a
/// blend, built once and kept; and the layouts and pass they share.
pub struct Pipelines {
    shape: Shape,
    /// The instance path's set layouts, [`INSTANCE_SETS`].
    pub sets: [vk::DescriptorSetLayout; 3],
    /// The cell path's set 0, [`CELL_SETS`]; its set 1 is `sets[1]`.
    pub cell_set: vk::DescriptorSetLayout,
    built: HashMap<Key, vk::Pipeline>,
    pending: HashMap<Key, ()>,
    building: Building,
    /// Draws skipped because their pipeline was still building
    /// ([`Pipelines::ready`]).
    pub not_ready: Skipped,
    /// `multiply` draws blended in two passes, on a device without a second
    /// source ([`crate::frame::Multiply::TwoPass`]).
    pub multiply_two_pass: Skipped,
    /// Builds that failed, whose draws are skipped from then on.
    failed: HashMap<Key, Failed>,
}

/// Whether [`Pipelines::finished`] kept a pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Built {
    Yes,
    No,
}

/// Draws counted, as [`Pipelines::not_ready`] counts them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Skipped(pub u64);

impl Pipelines {
    /// The layouts, the pass over a `B8G8R8A8_UNORM` target released to the
    /// compositor's queue family after the frame, an empty pipeline cache,
    /// and the worker that builds for [`Pipelines::ready`].
    #[allow(unsafe_code)]
    pub fn new(device: &ash::Device) -> Result<Pipelines, Failed> {
        let d = device;
        let layout_of = |set: Set| {
            let bindings: Vec<vk::DescriptorSetLayoutBinding<'_>> = (0u32..)
                .zip(set)
                .map(|(n, (kind, stages))| {
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(n)
                        .descriptor_type(kind.vk())
                        .descriptor_count(1)
                        .stage_flags(*stages)
                })
                .collect();
            let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
            // SAFETY: a device of the caller's.
            unsafe { d.create_descriptor_set_layout(&info, None) }
        };
        let [s0, s1, s2] = INSTANCE_SETS;
        let sets = [layout_of(s0)?, layout_of(s1)?, layout_of(s2)?];
        let cell_set = layout_of(CELL_SETS[0])?;
        let info = vk::PipelineLayoutCreateInfo::default().set_layouts(&sets);
        // SAFETY: layouts just made on this device.
        let layout = unsafe { d.create_pipeline_layout(&info, None) }?;
        let cell_sets = [cell_set, sets[1]];
        let push = [vk::PushConstantRange::default()
            .stage_flags(VERTEX)
            .offset(0)
            .size(CELL_PUSH)];
        let info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&cell_sets)
            .push_constant_ranges(&push);
        // SAFETY: as above.
        let cell_layout = unsafe { d.create_pipeline_layout(&info, None) }?;
        let attachment = [vk::AttachmentDescription::default()
            .format(vk::Format::B8G8R8A8_UNORM)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE)
            .initial_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
        let colour = [vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
        let subpass = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&colour)];
        let pass_info = vk::RenderPassCreateInfo::default()
            .attachments(&attachment)
            .subpasses(&subpass);
        // SAFETY: as above.
        let pass = unsafe { d.create_render_pass(&pass_info, None) }?;
        let cache_info = vk::PipelineCacheCreateInfo::default();
        // SAFETY: as above.
        let cache = unsafe { d.create_pipeline_cache(&cache_info, None) }?;
        let shape = Shape {
            device: d.clone(),
            cache,
            layout,
            cell_layout,
            pass,
        };
        let building = Building::start(shape.clone());
        Ok(Pipelines {
            shape,
            sets,
            cell_set,
            built: HashMap::new(),
            pending: HashMap::new(),
            building,
            not_ready: Skipped::default(),
            multiply_two_pass: Skipped::default(),
            failed: HashMap::new(),
        })
    }

    pub fn layout(&self) -> vk::PipelineLayout {
        self.shape.layout
    }

    pub fn cell_layout(&self) -> vk::PipelineLayout {
        self.shape.cell_layout
    }

    pub fn pass(&self) -> vk::RenderPass {
        self.shape.pass
    }

    /// **The pipeline, built here if it is not yet**: for the pipelines a
    /// frame cannot go without, made before the first frame.
    pub fn get(&mut self, fragment: Fragment, blend: Blend) -> Result<vk::Pipeline, Failed> {
        self.drain();
        let key = (fragment, blend);
        if let Some(p) = self.built.get(&key) {
            return Ok(*p);
        }
        let p = self.shape.build(key)?;
        self.built.insert(key, p);
        Ok(p)
    }

    /// **The pipeline if it is built, never waiting for it**: otherwise its
    /// build is started on the worker, once, and the draw that asked is
    /// skipped and counted in [`Pipelines::not_ready`].
    pub fn ready(&mut self, fragment: Fragment, blend: Blend) -> Option<vk::Pipeline> {
        self.drain();
        let key = (fragment, blend);
        if let Some(p) = self.built.get(&key) {
            return Some(*p);
        }
        if !self.failed.contains_key(&key) && self.pending.insert(key, ()).is_none() {
            // A worker that has ended leaves the key pending: its draws are
            // skipped and counted, as a build that never finishes.
            let _ = self.building.jobs.send(key);
        }
        self.not_ready.0 += 1;
        None
    }

    /// **The descriptor a finished build makes readable**, for the loop's
    /// wait: on it, the driver calls [`Pipelines::finished`] and owes a
    /// frame, so an effect whose pipeline was building is drawn though
    /// nothing else changed. `None` where the socket could not be made, and
    /// then a build is seen at the next [`Pipelines::ready`].
    pub fn ready_fd(&self) -> Option<BorrowedFd<'_>> {
        self.building.woken.as_ref().map(AsFd::as_fd)
    }

    /// **The builds finished since the last call, kept**, and the wake
    /// read: whether any pipeline became ready, which is a frame owed.
    pub fn finished(&mut self) -> Built {
        if let Some(r) = &self.building.woken {
            let mut buf = [0u8; 64];
            while matches!((&*r).read(&mut buf), Ok(n) if n > 0) {}
        }
        let before = self.built.len();
        self.drain();
        match self.built.len() > before {
            true => Built::Yes,
            false => Built::No,
        }
    }

    /// The builds the worker has finished, kept.
    fn drain(&mut self) {
        while let Ok((key, made)) = self.building.done.try_recv() {
            self.pending.remove(&key);
            match made {
                Ok(p) => {
                    self.built.insert(key, p);
                }
                Err(e) => {
                    self.failed.insert(key, e);
                }
            }
        }
    }

    /// **Every pipeline and layout destroyed**, after the worker has built
    /// what was asked of it and ended. The caller has waited for
    /// the device to be idle.
    #[allow(unsafe_code)]
    pub fn destroy(mut self) {
        self.building.stop();
        self.drain();
        let d = &self.shape.device;
        // SAFETY: the device is idle and the worker has ended, so nothing
        // uses these handles again.
        unsafe {
            for p in self.built.values() {
                d.destroy_pipeline(*p, None);
            }
            d.destroy_pipeline_cache(self.shape.cache, None);
            d.destroy_render_pass(self.shape.pass, None);
            d.destroy_pipeline_layout(self.shape.layout, None);
            d.destroy_pipeline_layout(self.shape.cell_layout, None);
            d.destroy_descriptor_set_layout(self.cell_set, None);
            for s in self.sets {
                d.destroy_descriptor_set_layout(s, None);
            }
        }
    }

    /// **A held build, for a test**: the worker takes the next job and
    /// waits before building it until [`Pipelines::release_builds`].
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn hold_builds(&self) {
        *self.building.gate.0.lock().unwrap() = true;
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn release_builds(&self) {
        *self.building.gate.0.lock().unwrap() = false;
        self.building.gate.1.notify_all();
    }
}

impl Building {
    fn start(shape: Shape) -> Building {
        let (jobs, todo) = channel::<Key>();
        let (finished, done) = channel();
        // Both ends non-blocking: a full socket drops a wake the loop owes
        // already, and the loop never waits on a read.
        let pair = UnixStream::pair().and_then(|(r, w)| {
            r.set_nonblocking(true)?;
            w.set_nonblocking(true)?;
            Ok((r, w))
        });
        let (woken, wake) = match pair {
            Ok((r, w)) => (Some(r), Some(w)),
            Err(_) => (None, None),
        };
        #[cfg(any(test, feature = "test-hooks"))]
        let gate = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        #[cfg(any(test, feature = "test-hooks"))]
        let held = gate.clone();
        let worker = std::thread::Builder::new()
            .name("vk-pipelines".into())
            .spawn(move || {
                for key in todo {
                    #[cfg(any(test, feature = "test-hooks"))]
                    {
                        let (lock, cv) = &*held;
                        let mut on = lock.lock().unwrap();
                        while *on {
                            on = cv.wait(on).unwrap();
                        }
                    }
                    if finished.send((key, shape.build(key))).is_err() {
                        break;
                    }
                    if let Some(w) = &wake {
                        let _ = (&*w).write(&[1]);
                    }
                }
            })
            .ok();
        Building {
            done,
            woken,
            jobs,
            worker,
            #[cfg(any(test, feature = "test-hooks"))]
            gate,
        }
    }

    /// The jobs closed and the worker joined: it builds what was already
    /// asked for, and nothing after.
    fn stop(&mut self) {
        let (closed, _) = channel();
        drop(std::mem::replace(&mut self.jobs, closed));
        #[cfg(any(test, feature = "test-hooks"))]
        {
            *self.gate.0.lock().unwrap() = false;
            self.gate.1.notify_all();
        }
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
