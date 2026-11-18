//! **One frame into an exported dmabuf.** An image of the chosen format and
//! modifier, its memory exported as a dmabuf, its one plane's offset and
//! row pitch read back from the driver, and a frame recorded into it:
//! cleared, then released to the compositor's queue family
//! (`VK_QUEUE_FAMILY_FOREIGN_EXT`), as a buffer another process reads must
//! be. Pixels are premultiplied, as `ARGB8888` is on the wire, the same
//! encoding the shared-memory path writes.

use crate::choose::{Choice, Sync};
use crate::vk::{REQUIRED, SEMAPHORE_FD, Vulkan};
use ash::vk;
use baud_present_wayland::Px;
use baud_present_wayland::dmabuf::{Bytes, Format, Plane, Point};
use std::os::fd::{AsFd, FromRawFd, OwnedFd};

/// A premultiplied `0xAARRGGBB` word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Premultiplied(pub u32);

impl Premultiplied {
    /// The clear value: each channel's byte as a UNORM float, in the
    /// image's `B8G8R8A8` order of channels named R, G, B, A.
    fn clear(self) -> vk::ClearColorValue {
        let [b, g, r, a] = self.0.to_le_bytes();
        let f = |c: u8| f32::from(c) / 255.0;
        vk::ClearColorValue {
            float32: [f(r), f(g), f(b), f(a)],
        }
    }
}

/// The device the frames are drawn on, its queue, and how they are synced.
pub struct Gpu {
    pub vulkan: Vulkan,
    pub physical: vk::PhysicalDevice,
    pub device: ash::Device,
    pub(crate) queue: vk::Queue,
    pub(crate) family: QueueFamily,
    pub(crate) pool: vk::CommandPool,
    pub choice: Choice,
    /// How the plasma's `multiply` blends on this device.
    pub multiply: Multiply,
}

/// **How the plasma's `multiply` mix is blended**: by a second source, the
/// factor the backdrop is taken at, where the device has `dualSrcBlend`;
/// otherwise in two passes, the factor multiplied into the backdrop and
/// then the colour added (`effect.rs`), counted where it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Multiply {
    DualSource,
    TwoPass,
}

/// A memory type's index, as `vkAllocateMemory` takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemoryType(pub(crate) u32);

/// What [`Gpu::memory_type`] takes when no type has what is wanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fallback {
    Any,
    None,
}

/// A queue family's index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QueueFamily(pub(crate) u32);

/// A timeline semaphore and its exported fd.
pub struct Timeline {
    semaphore: vk::Semaphore,
    pub fd: OwnedFd,
}

/// An image exported as a dmabuf, with its one plane's layout.
pub struct Target {
    pub(crate) image: vk::Image,
    memory: vk::DeviceMemory,
    pub fd: OwnedFd,
    pub width: Px,
    pub height: Px,
    pub format: Format,
    pub offset: Bytes,
    pub stride: Bytes,
    /// The bytes the plane spans from its offset.
    pub size: Bytes,
}

/// Why the GPU path could not be made.
#[derive(Debug, PartialEq, Eq)]
pub enum Failed {
    NoGraphicsQueue,
    Vulkan(vk::Result),
    NoMemoryType,
    /// A committed shader is not whole SPIR-V words.
    Spirv,
}

impl Target {
    /// **The image as `baud-present-wayland` imports it**: its one plane,
    /// for `Dmabuf::planes`, which `Window::import` makes a `wl_buffer`
    /// of.
    pub fn plane(&self) -> Plane<'_> {
        Plane {
            fd: self.fd.as_fd(),
            offset: self.offset,
            stride: self.stride,
        }
    }
}

impl From<vk::Result> for Failed {
    fn from(r: vk::Result) -> Failed {
        Failed::Vulkan(r)
    }
}

impl Gpu {
    /// A device on `physical` with the extensions `choice` needs, its first
    /// graphics queue, and a command pool for it.
    #[allow(unsafe_code)]
    pub fn new(
        vulkan: Vulkan,
        physical: vk::PhysicalDevice,
        choice: Choice,
    ) -> Result<Gpu, Failed> {
        let inst = &vulkan.instance;
        // SAFETY: a physical device of this instance.
        let families = unsafe { inst.get_physical_device_queue_family_properties(physical) };
        let index = families
            .iter()
            .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .ok_or(Failed::NoGraphicsQueue)?;
        let family = QueueFamily(u32::try_from(index).map_err(|_| Failed::NoGraphicsQueue)?);
        let mut names: Vec<*const core::ffi::c_char> =
            REQUIRED.iter().map(|n| n.as_ptr()).collect();
        if choice.sync == Sync::Explicit {
            names.push(SEMAPHORE_FD.as_ptr());
        }
        let priority = [1.0f32];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family.0)
            .queue_priorities(&priority)];
        let mut v12 = vk::PhysicalDeviceVulkan12Features::default()
            .timeline_semaphore(choice.sync == Sync::Explicit);
        // The plasma's `multiply` blends by a second source where the device
        // offers it, and in two passes where it does not (`Multiply`).
        // SAFETY: a physical device of this instance.
        let offered = unsafe { inst.get_physical_device_features(physical) };
        let multiply = match offered.dual_src_blend {
            vk::TRUE => Multiply::DualSource,
            _ => Multiply::TwoPass,
        };
        let features =
            vk::PhysicalDeviceFeatures::default().dual_src_blend(multiply == Multiply::DualSource);
        let info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_extension_names(&names)
            .enabled_features(&features)
            .push_next(&mut v12);
        // SAFETY: the create-info and everything it points to live through
        // the call; the extensions were checked present (`vk::candidates`).
        let device = unsafe { inst.create_device(physical, &info, None) }?;
        // SAFETY: the queue family and index were just created.
        let queue = unsafe { device.get_device_queue(family.0, 0) };
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(family.0)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        // SAFETY: a device just made.
        let pool = unsafe { device.create_command_pool(&pool_info, None) }?;
        Ok(Gpu {
            vulkan,
            physical,
            device,
            queue,
            family,
            pool,
            choice,
            multiply,
        })
    }

    /// **The device's driver**, as `VkPhysicalDeviceDriverProperties` names
    /// it: how a test tells one implementation's blend from another's.
    #[allow(unsafe_code)]
    pub fn driver(&self) -> vk::DriverId {
        let mut driver = vk::PhysicalDeviceDriverProperties::default();
        let mut props = vk::PhysicalDeviceProperties2::default().push_next(&mut driver);
        // SAFETY: the device's own physical device, and a chain whose
        // members live through the call.
        unsafe {
            self.vulkan
                .instance
                .get_physical_device_properties2(self.physical, &mut props)
        };
        driver.driver_id
    }

    /// **An image of `width` by `height` in the chosen format and
    /// modifier, exported as a dmabuf.** Its memory is dedicated to it, the
    /// one allocation the fd names, and its plane's offset and row pitch
    /// are the driver's, read with `vkGetImageDrmFormatModifierPropertiesEXT`'s
    /// sibling, the memory plane's subresource layout.
    #[allow(unsafe_code)]
    pub fn target(&self, width: Px, height: Px) -> Result<Target, Failed> {
        let format = self.choice.format;
        let modifiers = [format.modifier.0];
        let mut list =
            vk::ImageDrmFormatModifierListCreateInfoEXT::default().drm_format_modifiers(&modifiers);
        let mut external = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::B8G8R8A8_UNORM)
            .extent(vk::Extent3D {
                width: width.0,
                height: height.0,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut list)
            .push_next(&mut external);
        let d = &self.device;
        // SAFETY: a valid create-info whose chain lives through the call.
        let image = unsafe { d.create_image(&info, None) }?;
        // SAFETY: the image was just made.
        let req = unsafe { d.get_image_memory_requirements(image) };
        let device_local = vk::MemoryPropertyFlags::DEVICE_LOCAL;
        let Some(type_index) = self.memory_type(&req, device_local, Fallback::Any) else {
            // SAFETY: the image is unbound and unused.
            unsafe { d.destroy_image(image, None) };
            return Err(Failed::NoMemoryType);
        };
        let mut export = vk::ExportMemoryAllocateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(type_index.0)
            .push_next(&mut export)
            .push_next(&mut dedicated);
        // SAFETY: an allocate-info for this image's requirements.
        let memory = unsafe { d.allocate_memory(&alloc, None) }?;
        // SAFETY: fresh memory of the image's size, bound once at zero.
        unsafe { d.bind_image_memory(image, memory, 0) }?;
        let fd_ext = ash::khr::external_memory_fd::Device::new(&self.vulkan.instance, d);
        let get = vk::MemoryGetFdInfoKHR::default()
            .memory(memory)
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        // SAFETY: memory allocated exportable as a dmabuf.
        let raw = unsafe { fd_ext.get_memory_fd(&get) }?;
        // SAFETY: `vkGetMemoryFdKHR` hands over a new descriptor the caller owns.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let sub =
            vk::ImageSubresource::default().aspect_mask(vk::ImageAspectFlags::MEMORY_PLANE_0_EXT);
        // SAFETY: a modifier image's memory plane 0, which it has.
        let layout = unsafe { d.get_image_subresource_layout(image, sub) };
        let bytes = |v: u64| Bytes(u32::try_from(v).unwrap_or(u32::MAX));
        Ok(Target {
            image,
            memory,
            fd,
            width,
            height,
            format,
            offset: bytes(layout.offset),
            stride: bytes(layout.row_pitch),
            size: bytes(layout.size),
        })
    }

    /// **The first memory type `req` accepts with `want`**, or, with
    /// `Fallback::Any`, the first it accepts at all: device-local for an
    /// image where there is one (a CPU device has none), and host-visible
    /// for a readback, which has no fallback.
    #[allow(unsafe_code)]
    pub(crate) fn memory_type(
        &self,
        req: &vk::MemoryRequirements,
        want: vk::MemoryPropertyFlags,
        fallback: Fallback,
    ) -> Option<MemoryType> {
        // SAFETY: a physical device of this instance.
        let props = unsafe {
            self.vulkan
                .instance
                .get_physical_device_memory_properties(self.physical)
        };
        let n = usize::try_from(props.memory_type_count).unwrap_or(0);
        let types = &props.memory_types[..n.min(props.memory_types.len())];
        let ok = |i: usize| req.memory_type_bits & (1u32 << i) != 0;
        let wanted = types
            .iter()
            .enumerate()
            .position(|(i, t)| ok(i) && t.property_flags.contains(want));
        let pick = match fallback {
            Fallback::Any => wanted.or_else(|| (0..n).find(|&i| ok(i))),
            Fallback::None => wanted,
        }?;
        u32::try_from(pick).ok().map(MemoryType)
    }

    /// **A timeline semaphore exported as an fd**, for one buffer's points:
    /// the acquire point this side signals and the release point the
    /// compositor does, "each buffer should use a separate timeline for
    /// its release points" (`[[source: wp-linux-drm-syncobj-v1]]`). Whether
    /// the fd is a syncobj is [`crate::sys::is_syncobj`]'s to say before it
    /// is sent.
    #[allow(unsafe_code)]
    pub fn timeline(&self) -> Result<Timeline, Failed> {
        let mut export = vk::ExportSemaphoreCreateInfo::default()
            .handle_types(vk::ExternalSemaphoreHandleTypeFlags::OPAQUE_FD);
        let mut ty = vk::SemaphoreTypeCreateInfo::default()
            .semaphore_type(vk::SemaphoreType::TIMELINE)
            .initial_value(0);
        let info = vk::SemaphoreCreateInfo::default()
            .push_next(&mut export)
            .push_next(&mut ty);
        // SAFETY: a create-info whose chain lives through the call, on a
        // device made with timeline semaphores and the semaphore fd
        // extension (`Sync::Explicit`).
        let semaphore = unsafe { self.device.create_semaphore(&info, None) }?;
        let fd_ext =
            ash::khr::external_semaphore_fd::Device::new(&self.vulkan.instance, &self.device);
        let get = vk::SemaphoreGetFdInfoKHR::default()
            .semaphore(semaphore)
            .handle_type(vk::ExternalSemaphoreHandleTypeFlags::OPAQUE_FD);
        // SAFETY: a semaphore made exportable as an opaque fd.
        let raw = unsafe { fd_ext.get_semaphore_fd(&get) }?;
        // SAFETY: the call hands over a new descriptor the caller owns.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        Ok(Timeline { semaphore, fd })
    }

    /// The value `tl` stands at now.
    #[allow(unsafe_code)]
    pub fn reached(&self, tl: &Timeline) -> Result<Point, Failed> {
        // SAFETY: a semaphore of this device.
        let v = unsafe { self.device.get_semaphore_counter_value(tl.semaphore) }?;
        Ok(Point(v))
    }

    /// Destroy `tl`. Its fd is closed when dropped.
    #[allow(unsafe_code)]
    pub fn drop_timeline(&self, tl: Timeline) {
        // SAFETY: no submission waits on it: `clear` waits for its own.
        unsafe { self.device.destroy_semaphore(tl.semaphore, None) };
    }

    /// **Clear `t` to `colour` and release it to the compositor**, signal
    /// `signal`'s point where one is given (the acquire point of an
    /// explicitly synced frame), and wait for the queue on the CPU. The
    /// wait is the implicit path's need, a frame finished before it is
    /// shown; the explicit path keeps it in this first frame and loses it
    /// once each buffer has its own command buffer.
    #[allow(unsafe_code)]
    pub fn clear(
        &self,
        t: &Target,
        colour: Premultiplied,
        signal: Option<(&Timeline, Point)>,
    ) -> Result<(), Failed> {
        let d = &self.device;
        let alloc = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        // SAFETY: the pool is this device's.
        let cb = unsafe { d.allocate_command_buffers(&alloc) }?[0];
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
            .image(t.image)
            .subresource_range(range);
        let release = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .src_queue_family_index(self.family.0)
            .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .image(t.image)
            .subresource_range(range);
        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        // SAFETY: a fresh command buffer recorded once and submitted once,
        // with barriers naming this image and this queue's family.
        unsafe {
            d.begin_command_buffer(cb, &begin)?;
            d.cmd_pipeline_barrier(
                cb,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_dst],
            );
            d.cmd_clear_color_image(
                cb,
                t.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &colour.clear(),
                &[range],
            );
            d.cmd_pipeline_barrier(
                cb,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[release],
            );
            d.end_command_buffer(cb)?;
        }
        let fence_info = vk::FenceCreateInfo::default();
        // SAFETY: a device of this struct.
        let fence = unsafe { d.create_fence(&fence_info, None) }?;
        let cbs = [cb];
        let (sems, values) = match signal {
            Some((tl, p)) => (vec![tl.semaphore], vec![p.0]),
            None => (Vec::new(), Vec::new()),
        };
        let mut timeline =
            vk::TimelineSemaphoreSubmitInfo::default().signal_semaphore_values(&values);
        let mut one = vk::SubmitInfo::default()
            .command_buffers(&cbs)
            .signal_semaphores(&sems);
        if signal.is_some() {
            one = one.push_next(&mut timeline);
        }
        let submit = [one];
        // SAFETY: the buffer is recorded, the fence unsignalled; both are
        // freed only after the wait.
        let done = unsafe {
            d.queue_submit(self.queue, &submit, fence)
                .and_then(|()| d.wait_for_fences(&[fence], true, u64::MAX))
        };
        // SAFETY: the queue has finished with both, or never took them.
        unsafe {
            d.destroy_fence(fence, None);
            d.free_command_buffers(self.pool, &cbs);
        }
        done.map_err(Failed::from)
    }

    /// **Every pixel of `t`, read back by the GPU**: the image taken back
    /// from the foreign queue family, copied into a host-visible buffer a
    /// row of `width` words at a time, and released again. Whatever the
    /// modifier, and wherever the memory is (a device-local dmabuf on a
    /// discrete GPU is not one the CPU can map), the words come out in
    /// rows, premultiplied `0xAARRGGBB`.
    #[allow(unsafe_code)]
    pub fn read(&self, t: &Target) -> Result<Vec<u32>, Failed> {
        let d = &self.device;
        let words = usize::try_from(u64::from(t.width.0) * u64::from(t.height.0)).unwrap_or(0);
        let bytes = u64::from(t.width.0) * u64::from(t.height.0) * 4;
        let info = vk::BufferCreateInfo::default()
            .size(bytes)
            .usage(vk::BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: a valid create-info.
        let buffer = unsafe { d.create_buffer(&info, None) }?;
        // SAFETY: the buffer was just made.
        let req = unsafe { d.get_buffer_memory_requirements(buffer) };
        let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let Some(index) = self.memory_type(&req, host, Fallback::None) else {
            // SAFETY: the buffer is unbound and unused.
            unsafe { d.destroy_buffer(buffer, None) };
            return Err(Failed::NoMemoryType);
        };
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(index.0);
        // SAFETY: an allocate-info for this buffer's requirements; bound once.
        let memory = unsafe { d.allocate_memory(&alloc, None) }?;
        // SAFETY: fresh memory of the buffer's size, bound once at zero.
        unsafe { d.bind_buffer_memory(buffer, memory, 0) }?;
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let acquire = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
            .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .dst_queue_family_index(self.family.0)
            .image(t.image)
            .subresource_range(range);
        let release = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_access_mask(vk::AccessFlags::TRANSFER_READ)
            .src_queue_family_index(self.family.0)
            .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .image(t.image)
            .subresource_range(range);
        let to_host = vk::BufferMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::HOST_READ)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .buffer(buffer)
            .size(vk::WHOLE_SIZE);
        let region = vk::BufferImageCopy::default()
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D {
                width: t.width.0,
                height: t.height.0,
                depth: 1,
            });
        let out = self.submit_and_wait(|cb| {
            // SAFETY: recording into a fresh buffer; the barriers and copy
            // name this image, this buffer and this queue's family.
            unsafe {
                d.cmd_pipeline_barrier(
                    cb,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[acquire],
                );
                d.cmd_copy_image_to_buffer(
                    cb,
                    t.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    buffer,
                    &[region],
                );
                d.cmd_pipeline_barrier(
                    cb,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::HOST | vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[to_host],
                    &[release],
                );
            }
        });
        let read = out.and_then(|()| {
            // SAFETY: host-visible coherent memory the queue has finished
            // writing; mapped whole and unmapped before it is freed.
            unsafe {
                let p = d.map_memory(memory, 0, bytes, vk::MemoryMapFlags::empty())?;
                let v = core::slice::from_raw_parts(p.cast::<u32>(), words).to_vec();
                d.unmap_memory(memory);
                Ok::<_, Failed>(v)
            }
        });
        // SAFETY: the queue has finished with both.
        unsafe {
            d.destroy_buffer(buffer, None);
            d.free_memory(memory, None);
        }
        read
    }

    /// Record with `record` into a fresh command buffer, submit it alone,
    /// and wait for it on the CPU.
    #[allow(unsafe_code)]
    pub(crate) fn submit_and_wait(
        &self,
        record: impl FnOnce(vk::CommandBuffer),
    ) -> Result<(), Failed> {
        let d = &self.device;
        let alloc = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        // SAFETY: the pool is this device's.
        let cb = unsafe { d.allocate_command_buffers(&alloc) }?[0];
        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        // SAFETY: a fresh buffer, begun once.
        unsafe { d.begin_command_buffer(cb, &begin) }?;
        record(cb);
        // SAFETY: the buffer was begun above.
        unsafe { d.end_command_buffer(cb) }?;
        // SAFETY: a device of this struct.
        let fence = unsafe { d.create_fence(&vk::FenceCreateInfo::default(), None) }?;
        let cbs = [cb];
        let submit = [vk::SubmitInfo::default().command_buffers(&cbs)];
        // SAFETY: the buffer is recorded and the fence unsignalled; both are
        // freed only after the wait.
        let done = unsafe {
            d.queue_submit(self.queue, &submit, fence)
                .and_then(|()| d.wait_for_fences(&[fence], true, u64::MAX))
        };
        // SAFETY: the queue has finished with both, or never took them.
        unsafe {
            d.destroy_fence(fence, None);
            d.free_command_buffers(self.pool, &cbs);
        }
        done.map_err(Failed::from)
    }

    /// Destroy `t`: its image and memory. Its fd is closed when dropped.
    #[allow(unsafe_code)]
    pub fn drop_target(&self, t: Target) {
        // SAFETY: no submission still uses the target: `clear` waits.
        unsafe {
            self.device.destroy_image(t.image, None);
            self.device.free_memory(t.memory, None);
        }
    }
}

impl Drop for Gpu {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: every submission waited; the pool and device are ours.
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_device(None);
        }
    }
}
