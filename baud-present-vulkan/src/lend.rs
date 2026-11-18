//! **The cell path's memory, lent to the CPU**: a buffer the vertex stage
//! reads, and the slice `baud_render::cell_frame::build` writes, on one of
//! two arms chosen from the device's heaps.
//!
//! - **In place**: memory that is device-local, host-visible and coherent
//!   (unified memory, or a BAR the whole of VRAM is mapped through). The
//!   slice is the buffer's own mapping, as Metal's shared buffers are, and
//!   a frame copies nothing.
//! - **Staging**: the slice is a host-visible buffer of the same size, and
//!   the device-local buffer the stage reads takes the ranges a frame owes
//!   ([`Lent::owe`]) by `vkCmdCopyBuffer`, recorded ahead of the frame's
//!   draws behind a transfer-to-vertex barrier. A set is written only when
//!   no frame in flight reads it, so a staging range a frame in flight
//!   copies from is never written under it.

use crate::draw::Buffer;
use crate::frame::{Failed, Fallback, Gpu};
use ash::vk;
use baud_domain::PackedCell;
use baud_render::{MapEntry, Resolved};
use std::marker::PhantomData;
use std::ops::Range;

/// **A type whose every bit pattern is a value**, so a mapping of
/// zeroed or GPU-written memory may be read as a slice of it.
///
/// # Safety
///
/// The implementer is `repr(C)` or `repr(transparent)` over plain
/// integers, with no padding a read could find uninitialised.
#[allow(unsafe_code)]
pub unsafe trait Plain: Copy + 'static {}

// SAFETY: `repr(C)` over a `u32` and two `u16` newtypes, eight bytes.
#[allow(unsafe_code)]
unsafe impl Plain for PackedCell {}
// SAFETY: `repr(C)` over nine `u32` newtypes.
#[allow(unsafe_code)]
unsafe impl Plain for Resolved {}
// SAFETY: `repr(C)` over integer newtypes, twenty-four bytes.
#[allow(unsafe_code)]
unsafe impl Plain for MapEntry {}
// SAFETY: integers.
#[allow(unsafe_code)]
unsafe impl Plain for u8 {}
// SAFETY: as above.
#[allow(unsafe_code)]
unsafe impl Plain for u32 {}

/// Which memory the cell path's buffers live in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arm {
    /// One buffer, device-local and mapped.
    InPlace,
    /// A mapped buffer the CPU writes, and a device-local one the owed
    /// ranges are copied to.
    Staging,
}

/// A count of elements in a lent buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Elems(pub usize);

/// Bytes a frame has copied from staging, counted when it records them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Uploaded(pub u64);

/// **A buffer the cell path binds, and its slice**, from [`Gpu::lend`].
pub struct Lent<T: Plain> {
    /// What the stage reads: the mapped buffer itself in place, the
    /// device-local copy under staging.
    device: Buffer,
    /// Its length in bytes.
    size: vk::DeviceSize,
    /// Under staging, the mapped buffer the slice is.
    host: Option<Buffer>,
    /// The ranges owed to `device`, in bytes, cleared as a frame records
    /// them; its capacity is kept, so a frame owes without allocating.
    owed: Vec<vk::BufferCopy>,
    uploaded: Uploaded,
    _of: PhantomData<T>,
}

const PROPS_IN_PLACE: vk::MemoryPropertyFlags = vk::MemoryPropertyFlags::from_raw(
    vk::MemoryPropertyFlags::DEVICE_LOCAL.as_raw()
        | vk::MemoryPropertyFlags::HOST_VISIBLE.as_raw()
        | vk::MemoryPropertyFlags::HOST_COHERENT.as_raw(),
);
const PROPS_HOST: vk::MemoryPropertyFlags = vk::MemoryPropertyFlags::from_raw(
    vk::MemoryPropertyFlags::HOST_VISIBLE.as_raw()
        | vk::MemoryPropertyFlags::HOST_COHERENT.as_raw(),
);

impl Gpu {
    /// **The arm this device's heaps allow**: in place where some memory
    /// type is device-local, host-visible and coherent, and its heap is
    /// at least `need` bytes; staging otherwise.
    #[allow(unsafe_code)]
    pub fn arm(&self, need: vk::DeviceSize) -> Arm {
        // SAFETY: a physical device of this instance.
        let props = unsafe {
            self.vulkan
                .instance
                .get_physical_device_memory_properties(self.physical)
        };
        let n = usize::try_from(props.memory_type_count).unwrap_or(0);
        let fits = props.memory_types[..n.min(props.memory_types.len())]
            .iter()
            .any(|t| {
                let heap = usize::try_from(t.heap_index)
                    .ok()
                    .and_then(|h| props.memory_heaps.get(h));
                t.property_flags.contains(PROPS_IN_PLACE) && heap.is_some_and(|h| h.size >= need)
            });
        match fits {
            true => Arm::InPlace,
            false => Arm::Staging,
        }
    }

    /// A buffer of `size` bytes in memory with `props`, bound, and its
    /// mapping where the memory is host-visible.
    #[allow(unsafe_code)]
    fn bound(
        &self,
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
        props: vk::MemoryPropertyFlags,
    ) -> Result<(Buffer, Option<*mut u8>), Failed> {
        let d = &self.device;
        let info = vk::BufferCreateInfo::default()
            .size(size.max(4))
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: a device of this struct.
        let buffer = unsafe { d.create_buffer(&info, None) }?;
        // SAFETY: the buffer was just made.
        let req = unsafe { d.get_buffer_memory_requirements(buffer) };
        let Some(kind) = self.memory_type(&req, props, Fallback::None) else {
            // SAFETY: unbound and unused.
            unsafe { d.destroy_buffer(buffer, None) };
            return Err(Failed::NoMemoryType);
        };
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(kind.0);
        // SAFETY: as above.
        let memory = unsafe { d.allocate_memory(&alloc, None) }?;
        // SAFETY: bound once, at offset 0; a host-visible memory is mapped
        // whole and stays mapped until it is freed.
        let mapped = unsafe {
            d.bind_buffer_memory(buffer, memory, 0)?;
            match props.contains(vk::MemoryPropertyFlags::HOST_VISIBLE) {
                true => Some(
                    d.map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())?
                        .cast::<u8>(),
                ),
                false => None,
            }
        };
        Ok((Buffer { buffer, memory }, mapped))
    }

    /// **`n` elements of `T` for the cell path on `arm`**, bound as
    /// `usage` (and as a transfer's source or destination under staging),
    /// and the slice the CPU writes them through, zeroed.
    #[allow(unsafe_code)]
    pub fn lend<T: Plain>(
        &self,
        arm: Arm,
        n: Elems,
        usage: vk::BufferUsageFlags,
    ) -> Result<(Lent<T>, &'static mut [T]), Failed> {
        let n = n.0;
        let bytes = n
            .checked_mul(size_of::<T>())
            .and_then(|b| vk::DeviceSize::try_from(b).ok())
            .ok_or(Failed::NoMemoryType)?;
        let (device, host, at) = match arm {
            Arm::InPlace => {
                let (b, at) = self.bound(
                    bytes,
                    usage | vk::BufferUsageFlags::TRANSFER_SRC,
                    PROPS_IN_PLACE,
                )?;
                (b, None, at)
            }
            Arm::Staging => {
                let (d, _) = self.bound(
                    bytes,
                    usage | vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                )?;
                let (h, at) =
                    match self.bound(bytes, vk::BufferUsageFlags::TRANSFER_SRC, PROPS_HOST) {
                        Ok(made) => made,
                        Err(e) => {
                            self.drop_buffer(d);
                            return Err(e);
                        }
                    };
                (d, Some(h), at)
            }
        };
        let at = at.ok_or(Failed::NoMemoryType)?;
        // SAFETY: the mapping is `bytes` long, host-coherent, and stays
        // mapped until `Lent::destroy` frees it; it is zeroed before the
        // slice is made, and `T: Plain` makes zero (and anything the CPU
        // writes) a value. The caller writes the slice only while no frame
        // in flight reads the buffer, the set discipline every lent slice
        // is under.
        let slice = unsafe {
            std::ptr::write_bytes(at, 0, usize::try_from(bytes).unwrap_or(0));
            std::slice::from_raw_parts_mut(at.cast::<T>(), n)
        };
        Ok((
            Lent {
                device,
                size: bytes,
                host,
                owed: Vec::with_capacity(64),
                uploaded: Uploaded::default(),
                _of: PhantomData,
            },
            slice,
        ))
    }
}

impl Gpu {
    /// **`bytes` of host-visible, coherent memory as a buffer bound as
    /// `usage`**, and its mapping, zeroed: for what a frame writes whole
    /// and a stage reads where it is, on either arm.
    #[allow(unsafe_code)]
    pub(crate) fn host_mapped(
        &self,
        bytes: Elems,
        usage: vk::BufferUsageFlags,
    ) -> Result<(Buffer, &'static mut [u8]), Failed> {
        let size = vk::DeviceSize::try_from(bytes.0).map_err(|_| Failed::NoMemoryType)?;
        let (buffer, at) = self.bound(size, usage, PROPS_HOST)?;
        let Some(at) = at else {
            self.drop_buffer(buffer);
            return Err(Failed::NoMemoryType);
        };
        // SAFETY: the mapping is `bytes` long, host-coherent, and stays
        // mapped until the buffer is freed; the caller writes it only while
        // no frame in flight reads it.
        let slice = unsafe {
            std::ptr::write_bytes(at, 0, bytes.0);
            std::slice::from_raw_parts_mut(at, bytes.0)
        };
        Ok((buffer, slice))
    }
}

impl<T: Plain> Lent<T> {
    /// The buffer a descriptor binds.
    pub fn buffer(&self) -> vk::Buffer {
        self.device.buffer
    }

    /// The arm it was made on.
    pub fn arm(&self) -> Arm {
        match self.host {
            Some(_) => Arm::Staging,
            None => Arm::InPlace,
        }
    }

    /// **Elements `range` were written, and the next frame owes them to
    /// the device**: a copy under staging, a range run on from the last
    /// one merged into it; nothing in place, where the stage reads what
    /// the CPU wrote.
    pub fn owe(&mut self, range: Range<usize>) {
        if self.host.is_none() || range.is_empty() {
            return;
        }
        let at = |i: usize| vk::DeviceSize::try_from(i * size_of::<T>()).unwrap_or(0);
        let (from, to) = (at(range.start), at(range.end));
        match self.owed.last_mut() {
            Some(last) if last.src_offset + last.size == from => last.size = to - last.src_offset,
            _ => self.owed.push(
                vk::BufferCopy::default()
                    .src_offset(from)
                    .dst_offset(from)
                    .size(to - from),
            ),
        }
    }

    /// Every byte a frame has copied from staging.
    pub fn uploaded(&self) -> Uploaded {
        self.uploaded
    }

    /// **The owed ranges recorded into `cb`**, ahead of the frame's
    /// draws, and made visible to the vertex stage's reads.
    #[allow(unsafe_code)]
    pub(crate) fn record(&mut self, d: &ash::Device, cb: vk::CommandBuffer) {
        let Some(host) = &self.host else {
            return;
        };
        if self.owed.is_empty() {
            return;
        }
        // A shader read and a uniform read: `CU` is a uniform buffer.
        let reads = vk::AccessFlags::SHADER_READ | vk::AccessFlags::UNIFORM_READ;
        let read = [vk::BufferMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(reads)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .buffer(self.device.buffer)
            .size(vk::WHOLE_SIZE)];
        // SAFETY: both buffers are alive until the frame has finished, and
        // the earlier frames' reads of `device` are ordered before the copy
        // by the barrier ahead of it.
        unsafe {
            let before = [vk::BufferMemoryBarrier::default()
                .src_access_mask(reads)
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(self.device.buffer)
                .size(vk::WHOLE_SIZE)];
            d.cmd_pipeline_barrier(
                cb,
                vk::PipelineStageFlags::VERTEX_SHADER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &before,
                &[],
            );
            d.cmd_copy_buffer(cb, host.buffer, self.device.buffer, &self.owed);
            d.cmd_pipeline_barrier(
                cb,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::VERTEX_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &read,
                &[],
            );
        }
        self.uploaded.0 += self.owed.iter().map(|r| r.size).sum::<u64>();
        self.owed.clear();
    }

    /// The buffers freed. The caller has waited for the device to be idle
    /// and drops the slice first.
    pub fn destroy(self, gpu: &Gpu) {
        gpu.drop_buffer(self.device);
        if let Some(h) = self.host {
            gpu.drop_buffer(h);
        }
    }
}

impl Gpu {
    /// **What `lent` owes, copied now**, and the CPU waits: for a frame
    /// that records nothing else, and a test's.
    pub fn settle<T: Plain>(&self, lent: &mut Lent<T>) -> Result<(), Failed> {
        let d = self.device.clone();
        self.submit_and_wait(|cb| lent.record(&d, cb))
    }

    /// **The bytes `lent`'s stage reads**, copied back to the CPU.
    #[allow(unsafe_code)]
    pub fn read_back<T: Plain>(&self, lent: &Lent<T>) -> Result<Vec<u8>, Failed> {
        let d = &self.device;
        let size = lent.size.max(4);
        let (back, at) = self.bound(size, vk::BufferUsageFlags::TRANSFER_DST, PROPS_HOST)?;
        let region = [vk::BufferCopy::default().size(size)];
        let copied = self.submit_and_wait(|cb| {
            // SAFETY: both buffers outlive the wait.
            unsafe { d.cmd_copy_buffer(cb, lent.device.buffer, back.buffer, &region) };
        });
        let out = copied.map(|()| {
            let n = usize::try_from(size).unwrap_or(0);
            // SAFETY: mapped, `size` long, and the copy into it has finished.
            at.map(|p| unsafe { std::slice::from_raw_parts(p, n) }.to_vec())
                .unwrap_or_default()
        });
        self.drop_buffer(back);
        out
    }
}
