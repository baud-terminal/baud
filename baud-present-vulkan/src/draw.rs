//! **A frame drawn by the instance path into a target**: the cells'
//! backgrounds, then the records over them, each pipeline from
//! [`Pipelines`], and the image released to the compositor's queue family
//! after, as [`Gpu::clear`] leaves it. The scene's buffers and textures are
//! made for the frame and destroyed after it.

use crate::frame::{Failed, Fallback, Gpu, Target};
use crate::pipeline::{Blend, Fragment, Pipelines};
use ash::vk;
use baud_present_wayland::Px;

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
    /// ([`Gpu::clear`], an earlier draw): an effect over a page.
    Over,
}

/// How many records a draw has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Records(pub u32);

/// **What a frame draws**: the uniform, the cells' backgrounds a texel a
/// cell, the two atlases, the contrast curves and the records
/// (`Instance::to_bytes`, 20 bytes each).
pub struct Scene<'a> {
    /// `U`: viewport, cell, grid, mask, colour and pad, two floats each.
    pub u: [f32; 12],
    pub backgrounds: Texels<'a>,
    pub mask: Texels<'a>,
    pub color: Texels<'a>,
    pub curves: &'a [u8],
    pub records: &'a [u8],
    pub count: Records,
}

/// A buffer the CPU writes and the GPU reads.
pub(crate) struct Buffer {
    pub(crate) buffer: vk::Buffer,
    memory: vk::DeviceMemory,
}

/// A sampled image, its memory and its view.
pub(crate) struct Texture {
    image: vk::Image,
    memory: vk::DeviceMemory,
    pub(crate) view: vk::ImageView,
}

impl Gpu {
    /// A host-visible buffer holding `bytes` (at least four, as an empty
    /// buffer is not one Vulkan makes).
    #[allow(unsafe_code)]
    pub(crate) fn buffer(
        &self,
        bytes: &[u8],
        usage: vk::BufferUsageFlags,
    ) -> Result<Buffer, Failed> {
        let d = &self.device;
        let size =
            vk::DeviceSize::try_from(bytes.len().max(4)).map_err(|_| Failed::NoMemoryType)?;
        let info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: a device of this struct.
        let buffer = unsafe { d.create_buffer(&info, None) }?;
        // SAFETY: the buffer was just made.
        let req = unsafe { d.get_buffer_memory_requirements(buffer) };
        let want = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let kind = self
            .memory_type(&req, want, Fallback::None)
            .ok_or(Failed::NoMemoryType)?;
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(kind.0);
        // SAFETY: as above; the memory is bound once, at offset 0.
        let memory = unsafe { d.allocate_memory(&alloc, None) }?;
        // SAFETY: the memory is host-visible and coherent, mapped over the
        // bytes copied and unmapped before any submission reads it.
        unsafe {
            d.bind_buffer_memory(buffer, memory, 0)?;
            let at = d.map_memory(memory, 0, size, vk::MemoryMapFlags::empty())?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), at.cast::<u8>(), bytes.len());
            d.unmap_memory(memory);
        }
        Ok(Buffer { buffer, memory })
    }

    /// **A texture uploaded from `t`**: a device image, the texels copied
    /// in from a staging buffer, left readable by the fragment stage.
    #[allow(unsafe_code)]
    pub(crate) fn texture(&self, t: &Texels<'_>) -> Result<Texture, Failed> {
        let d = &self.device;
        let (w, h) = (t.width.0.max(1), t.height.0.max(1));
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(t.texel.format())
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
        // SAFETY: the image was just made.
        let req = unsafe { d.get_image_memory_requirements(image) };
        let kind = self
            .memory_type(&req, vk::MemoryPropertyFlags::DEVICE_LOCAL, Fallback::Any)
            .ok_or(Failed::NoMemoryType)?;
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(kind.0);
        // SAFETY: as above.
        let memory = unsafe { d.allocate_memory(&alloc, None) }?;
        // SAFETY: as above; bound once.
        unsafe { d.bind_image_memory(image, memory, 0) }?;
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
        copied?;
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(t.texel.format())
            .subresource_range(range);
        // SAFETY: as above.
        let view = unsafe { d.create_image_view(&view_info, None) }?;
        Ok(Texture {
            image,
            memory,
            view,
        })
    }

    #[allow(unsafe_code)]
    pub(crate) fn drop_buffer(&self, b: Buffer) {
        // SAFETY: no submission uses it: every one is waited for.
        unsafe {
            self.device.destroy_buffer(b.buffer, None);
            self.device.free_memory(b.memory, None);
        }
    }

    #[allow(unsafe_code)]
    pub(crate) fn drop_texture(&self, t: Texture) {
        // SAFETY: as above.
        unsafe {
            self.device.destroy_image_view(t.view, None);
            self.device.destroy_image(t.image, None);
            self.device.free_memory(t.memory, None);
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
        let u_bytes: Vec<u8> = scene.u.iter().flat_map(|f| f.to_le_bytes()).collect();
        let u = self.buffer(&u_bytes, vk::BufferUsageFlags::UNIFORM_BUFFER)?;
        let insts = self.buffer(scene.records, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let curves = self.buffer(scene.curves, vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let effect_u = match effect {
            Some(bytes) => Some(self.buffer(bytes, vk::BufferUsageFlags::UNIFORM_BUFFER)?),
            None => None,
        };
        // Set 2 only for an effect, whose fragment reads it.
        let set_count: usize = if effect_u.is_some() { 3 } else { 2 };
        let mask = self.texture(&scene.mask)?;
        let color = self.texture(&scene.color)?;
        let backgrounds = self.texture(&scene.backgrounds)?;
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::NEAREST)
            .min_filter(vk::Filter::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
        // SAFETY: a device of this struct.
        let sampler = unsafe { d.create_sampler(&sampler_info, None) }?;
        let sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(2),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(2),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(3),
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(u32::try_from(set_count).unwrap_or(2))
            .pool_sizes(&sizes);
        // SAFETY: as above.
        let pool = unsafe { d.create_descriptor_pool(&pool_info, None) }?;
        let all = [pipes.sets[0], pipes.sets[1], pipes.sets[2]];
        let layouts = &all[..set_count];
        let alloc = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(layouts);
        // SAFETY: the pool holds exactly these sets.
        let sets = unsafe { d.allocate_descriptor_sets(&alloc) }?;
        let whole = |b: &Buffer| {
            [vk::DescriptorBufferInfo::default()
                .buffer(b.buffer)
                .range(vk::WHOLE_SIZE)]
        };
        let (insts_info, u_info, curves_info) = (whole(&insts), whole(&u), whole(&curves));
        let image = |t: &Texture| {
            [vk::DescriptorImageInfo::default()
                .sampler(sampler)
                .image_view(t.view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
        };
        let (mask_info, color_info, bg_info) = (image(&mask), image(&color), image(&backgrounds));
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
            write(sets[0], 1, uniform).buffer_info(&u_info),
            write(sets[1], 0, sampled).image_info(&mask_info),
            write(sets[1], 1, sampled).image_info(&color_info),
            write(sets[1], 2, storage).buffer_info(&curves_info),
            write(sets[1], 3, sampled).image_info(&bg_info),
        ];
        if let Some(info) = &effect_info {
            writes.push(write(sets[2], 0, uniform).buffer_info(info));
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
        let attachments = [target_view];
        let fb_info = vk::FramebufferCreateInfo::default()
            .render_pass(pipes.pass())
            .attachments(&attachments)
            .width(t.width.0)
            .height(t.height.0)
            .layers(1);
        // SAFETY: as above.
        let framebuffer = unsafe { d.create_framebuffer(&fb_info, None) }?;
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
        let viewport = [vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: t.width.0 as f32,
            height: t.height.0 as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }];
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
                d.cmd_set_viewport(cb, 0, &viewport);
                d.cmd_set_scissor(cb, 0, &scissor);
                d.cmd_bind_descriptor_sets(
                    cb,
                    vk::PipelineBindPoint::GRAPHICS,
                    layout,
                    0,
                    &sets,
                    &[],
                );
                if let Some(bg_pipe) = bg_pipe {
                    d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, bg_pipe);
                    d.cmd_draw(cb, 4, 1, 0, 0);
                }
                d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, glyph_pipe);
                d.cmd_draw(cb, 4, scene.count.0, 0, 0);
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
            d.destroy_sampler(sampler, None);
        }
        for b in [u, insts, curves].into_iter().chain(effect_u) {
            self.drop_buffer(b);
        }
        for t in [mask, color, backgrounds] {
            self.drop_texture(t);
        }
        drawn
    }
}
