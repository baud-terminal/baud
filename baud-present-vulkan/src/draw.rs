//! **A frame drawn by the instance path into a target**: the cells'
//! backgrounds, then the records over them, each pipeline from
//! [`Pipelines`], and the image released to the compositor's queue family
//! after, as `Gpu::clear`, a test hook, leaves it. The glyphs are sampled from the
//! persistent [`Sheets`], whose owed copies the frame records ahead of its
//! draws; the scene's other buffers and textures are made for the frame and
//! destroyed after it.

use crate::chrome::Chrome;
use crate::frame::{Failed, Fallback, Gpu, Target};
use crate::lend::{Elems, PROPS_HOST};
use crate::pipeline::{Blend, Fragment, Pipelines};
use crate::sheets::Sheets;
use ash::vk;
use baud_present_wayland::Px;
use baud_render::Instance;

/// How a texture's texels are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Texel {
    /// One byte a texel: a mask's coverage.
    Coverage,
    /// Four bytes a texel, `0xAARRGGBB` little-endian, straight alpha.
    Argb,
}

impl Texel {
    fn format(self) -> vk::Format {
        match self {
            Texel::Coverage => vk::Format::R8_UNORM,
            Texel::Argb => vk::Format::B8G8R8A8_UNORM,
        }
    }
}

/// A texture's size and its texels, row after row with no padding.
pub struct Texels<'a> {
    pub width: Px,
    pub height: Px,
    pub texel: Texel,
    pub bytes: &'a [u8],
}

/// **What a draw does with the target's pixels.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// The backgrounds over the whole target, replacing it, then the
    /// records: a frame.
    Whole,
    /// The records alone, blended over what the target holds, which is
    /// taken back from the compositor's queue family as it was left
    /// (`Gpu::clear` under test hooks, an earlier draw): an effect over a page.
    Over,
}

/// How many records a draw has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Records(pub u32);

/// **What a frame draws**: the uniform, the cells' backgrounds a texel a
/// cell, and the records (`Instance::to_bytes`, 20 bytes each); the atlases
/// and the contrast curves are the persistent [`Sheets`].
pub struct Scene<'a> {
    /// `U`: viewport, cell, grid, mask, colour and pad, two floats each.
    pub u: [f32; 12],
    pub backgrounds: Texels<'a>,
    pub sheets: &'a Sheets,
    pub records: &'a [u8],
    pub count: Records,
    /// The frame around the grid, or none: the grid over the whole target.
    pub framing: Option<Framing<'a>>,
}

/// **The frame around the grid**: its chrome in the target, and the
/// records of its three strips (`Instance::to_bytes`) -- the exit-status
/// strip's, the lights' line's and the word wrap's marks', in that order.
/// A whole frame is cleared to the chrome's page, its backgrounds and
/// records drawn in the grid's viewport and each strip's records in its
/// own, source-over, as the cell path draws them. A pass over a frame (an
/// effect) takes the grid's pad and draws no strip.
pub struct Framing<'a> {
    pub chrome: Chrome,
    pub strips: [&'a [u8]; 3],
}

/// A buffer the CPU writes and the GPU reads.
pub(crate) struct Buffer {
    pub(crate) buffer: vk::Buffer,
    pub(crate) memory: vk::DeviceMemory,
}

/// A sampled image, its memory and its view.
pub(crate) struct Texture {
    pub(crate) image: vk::Image,
    memory: vk::DeviceMemory,
    pub(crate) view: vk::ImageView,
}

impl Gpu {
    /// A host-visible buffer holding `bytes` (at least four, as an empty
    /// buffer is not one Vulkan makes). A failure at any step destroys what
    /// the steps before it made ([`Gpu::bound`]).
    #[allow(unsafe_code)]
    pub(crate) fn buffer(
        &self,
        bytes: &[u8],
        usage: vk::BufferUsageFlags,
    ) -> Result<Buffer, Failed> {
        let size = vk::DeviceSize::try_from(bytes.len()).map_err(|_| Failed::NoMemoryType)?;
        let (made, at) = self.bound(size, usage, PROPS_HOST)?;
        let Some(at) = at else {
            self.drop_buffer(made);
            return Err(Failed::NoMemoryType);
        };
        // SAFETY: the mapping is the whole memory, at least `bytes` long,
        // host-coherent, and unmapped before any submission reads it.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), at, bytes.len());
            self.device.unmap_memory(made.memory);
        }
        Ok(made)
    }

    /// **A device image of `texel`, `width` by `height`**, its memory and
    /// its view, written by transfers and sampled by the fragment stage; its
    /// texels undefined until a copy. A failure at any step destroys what
    /// the steps before it made.
    #[allow(unsafe_code)]
    pub(crate) fn image(&self, texel: Texel, width: Px, height: Px) -> Result<Texture, Failed> {
        let d = &self.device;
        let (w, h) = (width.0.max(1), height.0.max(1));
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(texel.format())
            .extent(vk::Extent3D {
                width: w,
                height: h,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        // SAFETY: a device of this struct.
        let image = unsafe { d.create_image(&info, None) }?;
        self.object_made();
        let unmake = |memory: Option<vk::DeviceMemory>| {
            // SAFETY: neither is in use; the image goes first.
            unsafe {
                d.destroy_image(image, None);
                if let Some(m) = memory {
                    d.free_memory(m, None);
                }
            }
            self.object_dropped();
            if memory.is_some() {
                self.object_dropped();
            }
        };
        // SAFETY: the image was just made.
        let req = unsafe { d.get_image_memory_requirements(image) };
        let Some(kind) =
            self.memory_type(&req, vk::MemoryPropertyFlags::DEVICE_LOCAL, Fallback::Any)
        else {
            unmake(None);
            return Err(Failed::NoMemoryType);
        };
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(kind.0);
        // SAFETY: as above.
        let memory = match self
            .call()
            .and_then(|()| unsafe { d.allocate_memory(&alloc, None) }.map_err(Failed::from))
        {
            Ok(m) => m,
            Err(e) => {
                unmake(None);
                return Err(e);
            }
        };
        self.object_made();
        // SAFETY: as above; bound once.
        let bound = self
            .call()
            .and_then(|()| unsafe { d.bind_image_memory(image, memory, 0) }.map_err(Failed::from));
        if let Err(e) = bound {
            unmake(Some(memory));
            return Err(e);
        }
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(texel.format())
            .subresource_range(range);
        // SAFETY: as above.
        let view = match self
            .call()
            .and_then(|()| unsafe { d.create_image_view(&view_info, None) }.map_err(Failed::from))
        {
            Ok(v) => v,
            Err(e) => {
                unmake(Some(memory));
                return Err(e);
            }
        };
        self.object_made();
        Ok(Texture {
            image,
            memory,
            view,
        })
    }

    /// **A texture uploaded from `t`**: [`Gpu::image`], the texels copied
    /// in from a staging buffer, left readable by the fragment stage; the
    /// CPU waits for the copy.
    #[allow(unsafe_code)]
    pub(crate) fn texture(&self, t: &Texels<'_>) -> Result<Texture, Failed> {
        let d = &self.device;
        let (w, h) = (t.width.0.max(1), t.height.0.max(1));
        let made = self.image(t.texel, t.width, t.height)?;
        let image = made.image;
        let staging = self.buffer(t.bytes, vk::BufferUsageFlags::TRANSFER_SRC)?;
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let to_dst = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(range);
        let to_read = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(range);
        let region = [vk::BufferImageCopy::default()
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D {
                width: w,
                height: h,
                depth: 1,
            })];
        let copied = self.submit_and_wait(|cb| {
            // SAFETY: barriers and a copy naming this image and the staging
            // buffer, which outlive the wait.
            unsafe {
                d.cmd_pipeline_barrier(
                    cb,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[to_dst],
                );
                d.cmd_copy_buffer_to_image(
                    cb,
                    staging.buffer,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &region,
                );
                d.cmd_pipeline_barrier(
                    cb,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[to_read],
                );
            }
        });
        self.drop_buffer(staging);
        if let Err(e) = copied {
            self.drop_texture(made);
            return Err(e);
        }
        Ok(made)
    }

    #[allow(unsafe_code)]
    pub(crate) fn drop_buffer(&self, b: Buffer) {
        // SAFETY: no submission uses it: every one is waited for.
        unsafe {
            self.device.destroy_buffer(b.buffer, None);
            self.device.free_memory(b.memory, None);
        }
        self.object_dropped();
        self.object_dropped();
    }

    #[allow(unsafe_code)]
    pub(crate) fn drop_texture(&self, t: Texture) {
        // SAFETY: as above.
        unsafe {
            self.device.destroy_image_view(t.view, None);
            self.device.destroy_image(t.image, None);
            self.device.free_memory(t.memory, None);
        }
        for _ in 0..3 {
            self.object_dropped();
        }
    }

    /// **`scene` drawn into `t` by `pass`**: under [`Pass::Whole`] the
    /// backgrounds over the whole target, replacing what was there, then the
    /// records blended over them; under [`Pass::Over`] the records alone over
    /// what the target holds. Premultiplied; the image released to the
    /// compositor's queue family after, and the CPU waits for it.
    pub fn draw(
        &self,
        pipes: &mut Pipelines,
        t: &Target,
        scene: &Scene<'_>,
        pass: Pass,
    ) -> Result<(), Failed> {
        self.draw_with(
            pipes,
            t,
            scene,
            pass,
            (Fragment::Glyph, Blend::Normal),
            None,
        )
    }

    /// [`Gpu::draw`] with the records drawn by `records`' pipeline, and, for
    /// an effect or the fireworks, its uniform (`EffectU`, `FwU`) bound at
    /// set 2 (`effect.rs`).
    #[allow(unsafe_code)]
    pub(crate) fn draw_with(
        &self,
        pipes: &mut Pipelines,
        t: &Target,
        scene: &Scene<'_>,
        pass: Pass,
        records: (Fragment, Blend),
        effect: Option<&[u8]>,
    ) -> Result<(), Failed> {
        let d = &self.device;
        // The backgrounds' pipeline only for a whole frame: a pass over the
        // target never builds it.
        let bg_pipe = match pass {
            Pass::Whole => Some(pipes.get(Fragment::Background, Blend::Replace)?),
            Pass::Over => None,
        };
        let glyph_pipe = pipes.get(records.0, records.1)?;
        // A framed frame's grid `U` takes the grid's pad, and, on a whole
        // frame, each strip a `U` of its own and its records after the
        // scene's. The strips are the whole frame's: a pass over a frame (an
        // effect) draws over them and not them.
        let (grid_u, strips) = match &scene.framing {
            Some(f) => {
                let mut u = scene.u;
                [u[10], u[11]] = f.chrome.pad();
                let first = Elems(usize::try_from(scene.count.0).unwrap_or(0));
                debug_assert!(
                    f.strips.iter().all(|s| s.len() % Instance::BYTES == 0),
                    "a strip's records are whole records"
                );
                let lens = f.strips.map(|s| s.len() / Instance::BYTES);
                let strips = (pass == Pass::Whole).then(|| f.chrome.strips(first, lens));
                (u, strips)
            }
            None => (scene.u, None),
        };
        // A strip's records drawn as the cell path draws them, whatever
        // pipeline the grid's records are drawn by.
        let strip_pipe = match strips {
            Some(_) => Some(pipes.get(Fragment::Glyph, Blend::Normal)?),
            None => None,
        };
        let mut uniforms = vec![grid_u];
        if let Some(strips) = &strips {
            let (cell, sides) = ((scene.u[2], scene.u[3]), (scene.u[6], scene.u[8]));
            uniforms.extend(strips.iter().map(|s| s.u(cell, sides)));
        }
        let mut us = Vec::with_capacity(uniforms.len());
        for u in &uniforms {
            let bytes: Vec<u8> = u.iter().flat_map(|f| f.to_le_bytes()).collect();
            us.push(self.buffer(&bytes, vk::BufferUsageFlags::UNIFORM_BUFFER)?);
        }
        let all: Vec<u8>;
        let record_bytes = match &scene.framing {
            Some(f) => {
                all = [scene.records, f.strips[0], f.strips[1], f.strips[2]].concat();
                &all[..]
            }
            None => scene.records,
        };
        let insts = self.buffer(record_bytes, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let effect_u = match effect {
            Some(bytes) => Some(self.buffer(bytes, vk::BufferUsageFlags::UNIFORM_BUFFER)?),
            None => None,
        };
        // Set 2 only for an effect, whose fragment reads it; a set 0 for each
        // `U`, the grid's first.
        let set_count: usize = if effect_u.is_some() { 3 } else { 2 };
        let extra = us.len() - 1;
        let backgrounds = self.texture(&scene.backgrounds)?;
        let sheets = scene.sheets;
        let sampler = sheets.sampler;
        let (mask_view, color_view, curves_buffer) = sheets.bound();
        let sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(u32::try_from(2 + extra).unwrap_or(5)),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(u32::try_from(2 + extra).unwrap_or(5)),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(3),
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(u32::try_from(set_count + extra).unwrap_or(6))
            .pool_sizes(&sizes);
        // SAFETY: as above.
        let pool = unsafe { d.create_descriptor_pool(&pool_info, None) }?;
        self.object_made();
        let mut layouts = vec![pipes.sets[0], pipes.sets[1], pipes.sets[2]];
        layouts.truncate(set_count);
        layouts.extend(std::iter::repeat_n(pipes.sets[0], extra));
        let alloc = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&layouts);
        // SAFETY: the pool holds exactly these sets.
        let sets = unsafe { d.allocate_descriptor_sets(&alloc) }?;
        let whole = |b: &Buffer| {
            [vk::DescriptorBufferInfo::default()
                .buffer(b.buffer)
                .range(vk::WHOLE_SIZE)]
        };
        let insts_info = whole(&insts);
        let u_infos: Vec<[vk::DescriptorBufferInfo; 1]> = us.iter().map(whole).collect();
        let curves_info = [vk::DescriptorBufferInfo::default()
            .buffer(curves_buffer)
            .range(vk::WHOLE_SIZE)];
        let image = |view: vk::ImageView| {
            [vk::DescriptorImageInfo::default()
                .sampler(sampler)
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
        };
        let (mask_info, color_info, bg_info) =
            (image(mask_view), image(color_view), image(backgrounds.view));
        let write = |set: vk::DescriptorSet, binding: u32, kind: vk::DescriptorType| {
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(binding)
                .descriptor_type(kind)
        };
        let storage = vk::DescriptorType::STORAGE_BUFFER;
        let uniform = vk::DescriptorType::UNIFORM_BUFFER;
        let sampled = vk::DescriptorType::COMBINED_IMAGE_SAMPLER;
        let effect_info = effect_u.as_ref().map(whole);
        let mut writes = vec![
            write(sets[0], 0, storage).buffer_info(&insts_info),
            write(sets[0], 1, uniform).buffer_info(&u_infos[0]),
            write(sets[1], 0, sampled).image_info(&mask_info),
            write(sets[1], 1, sampled).image_info(&color_info),
            write(sets[1], 2, storage).buffer_info(&curves_info),
            write(sets[1], 3, sampled).image_info(&bg_info),
        ];
        if let Some(info) = &effect_info {
            writes.push(write(sets[2], 0, uniform).buffer_info(info));
        }
        // The strips' set 0s, after the sets the layout binds.
        let strip_sets = &sets[set_count..];
        for (set, u) in strip_sets.iter().zip(&u_infos[1..]) {
            writes.push(write(*set, 0, storage).buffer_info(&insts_info));
            writes.push(write(*set, 1, uniform).buffer_info(u));
        }
        // SAFETY: every set, buffer and view named is alive.
        unsafe { d.update_descriptor_sets(&writes, &[]) };
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let view_info = vk::ImageViewCreateInfo::default()
            .image(t.image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(vk::Format::B8G8R8A8_UNORM)
            .subresource_range(range);
        // SAFETY: the target's image, alive while `t` is.
        let target_view = unsafe { d.create_image_view(&view_info, None) }?;
        self.object_made();
        let attachments = [target_view];
        let fb_info = vk::FramebufferCreateInfo::default()
            .render_pass(pipes.pass())
            .attachments(&attachments)
            .width(t.width.0)
            .height(t.height.0)
            .layers(1);
        // SAFETY: as above.
        let framebuffer = unsafe { d.create_framebuffer(&fb_info, None) }?;
        self.object_made();
        // A whole frame draws over every pixel, so what the image held is not
        // kept: from UNDEFINED. A pass over it keeps it: taken back from the
        // compositor's queue family in the layout it was released in.
        let (from, families) = match pass {
            Pass::Whole => (
                vk::ImageLayout::UNDEFINED,
                (vk::QUEUE_FAMILY_IGNORED, vk::QUEUE_FAMILY_IGNORED),
            ),
            Pass::Over => (
                vk::ImageLayout::GENERAL,
                (vk::QUEUE_FAMILY_FOREIGN_EXT, self.family.0),
            ),
        };
        let to_colour = vk::ImageMemoryBarrier::default()
            .old_layout(from)
            .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .dst_access_mask(
                vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            )
            .src_queue_family_index(families.0)
            .dst_queue_family_index(families.1)
            .image(t.image)
            .subresource_range(range);
        let release = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .src_queue_family_index(self.family.0)
            .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .image(t.image)
            .subresource_range(range);
        let extent = vk::Extent2D {
            width: t.width.0,
            height: t.height.0,
        };
        // The grid's viewport: the whole target, or a framed frame's grid.
        let grid_view = scene.framing.as_ref().map(|f| f.chrome.boxed.grid);
        let viewport = [grid_view.map_or(
            vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: t.width.0 as f32,
                height: t.height.0 as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            },
            |v| v.vk(),
        )];
        let drawable = grid_view.is_none_or(|v| v.drawable());
        let page = scene.framing.as_ref().map(|f| {
            [vk::ClearAttachment {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                color_attachment: 0,
                clear_value: vk::ClearValue {
                    color: f.chrome.clear.clear(),
                },
            }]
        });
        let target_size = (t.width, t.height);
        // The cells' rectangle from the grid's pad: `U`'s grid times its cell.
        let cells = scene.framing.as_ref().map(|f| {
            let span = |n: f32, cell: f32| Px(u32::try_from((n * cell) as i64).unwrap_or(0));
            crate::chrome::scissor(
                target_size,
                f.chrome.pad(),
                (span(scene.u[4], scene.u[2]), span(scene.u[5], scene.u[3])),
            )
        });
        let scissor = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent,
        }];
        let begin = vk::RenderPassBeginInfo::default()
            .render_pass(pipes.pass())
            .framebuffer(framebuffer)
            .render_area(scissor[0]);
        let layout = pipes.layout();
        let drawn = self.submit_and_wait(|cb| {
            sheets.record(d, cb);
            // SAFETY: every handle recorded is alive until the wait ends;
            // the barriers name the target and this queue's family.
            unsafe {
                d.cmd_pipeline_barrier(
                    cb,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[to_colour],
                );
                d.cmd_begin_render_pass(cb, &begin, vk::SubpassContents::INLINE);
                // A framed whole frame starts as its page.
                if let (Some(page), Pass::Whole) = (&page, pass) {
                    let rect = [vk::ClearRect {
                        rect: scissor[0],
                        base_array_layer: 0,
                        layer_count: 1,
                    }];
                    d.cmd_clear_attachments(cb, page, &rect);
                }
                d.cmd_set_scissor(cb, 0, &scissor);
                d.cmd_bind_descriptor_sets(
                    cb,
                    vk::PipelineBindPoint::GRAPHICS,
                    layout,
                    0,
                    &sets[..set_count],
                    &[],
                );
                if drawable {
                    d.cmd_set_viewport(cb, 0, &viewport);
                    if let Some(bg_pipe) = bg_pipe {
                        // A framed frame's backgrounds cover its cells and
                        // no more: the gutter beside them keeps the page.
                        if let Some(cells) = cells {
                            d.cmd_set_scissor(cb, 0, &[cells]);
                        }
                        d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, bg_pipe);
                        d.cmd_draw(cb, 4, 1, 0, 0);
                        d.cmd_set_scissor(cb, 0, &scissor);
                    }
                    d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, glyph_pipe);
                    d.cmd_draw(cb, 4, scene.count.0, 0, 0);
                }
                // Each strip's records in its own viewport and scissor, under
                // its own `U`, source-over.
                if let Some(p) = strip_pipe {
                    d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, p);
                }
                for (st, set) in strips.iter().flatten().zip(strip_sets) {
                    if !st.drawn() {
                        continue;
                    }
                    d.cmd_bind_descriptor_sets(
                        cb,
                        vk::PipelineBindPoint::GRAPHICS,
                        layout,
                        0,
                        &[*set],
                        &[],
                    );
                    d.cmd_set_viewport(cb, 0, &[st.view.vk()]);
                    d.cmd_set_scissor(cb, 0, &[st.scissor(target_size)]);
                    let n = u32::try_from(st.to.0 - st.from.0).unwrap_or(0);
                    let first = u32::try_from(st.from.0).unwrap_or(0);
                    d.cmd_draw(cb, 4, n, 0, first);
                }
                d.cmd_end_render_pass(cb);
                d.cmd_pipeline_barrier(
                    cb,
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[release],
                );
            }
        });
        // SAFETY: the submission has finished, or never started.
        unsafe {
            d.destroy_framebuffer(framebuffer, None);
            d.destroy_image_view(target_view, None);
            d.destroy_descriptor_pool(pool, None);
        }
        for _ in 0..3 {
            self.object_dropped();
        }
        sheets.finished(self);
        for b in us.into_iter().chain([insts]).chain(effect_u) {
            self.drop_buffer(b);
        }
        self.drop_texture(backgrounds);
        drawn
    }
}
