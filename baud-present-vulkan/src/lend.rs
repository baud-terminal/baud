//! **The cell path's memory, lent to the CPU**: a buffer the vertex stage
//! reads, and the slice `baud_render::cell_frame::build` writes, on one of
//! two arms chosen from the device's heaps.
//!
//! - **In place**: memory that is device-local, host-visible, coherent and
//!   host-cached (unified memory). The slice is the buffer's own mapping,
//!   and a frame copies nothing.
//! - **Staging**: the slice is a host-visible buffer of the same size,
//!   host-cached where the device has such a type, and
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

/// **Lent in place only where the CPU's reads are cached**: the CPU reads
/// the slices back every frame -- the glyph map's probes, a copy's row
/// compare, the overlay's -- and memory the CPU reads is host-cached
/// (`[[source: vma-usage-patterns]]`, Readback).
const PROPS_IN_PLACE: vk::MemoryPropertyFlags = vk::MemoryPropertyFlags::from_raw(
    vk::MemoryPropertyFlags::DEVICE_LOCAL.as_raw()
        | vk::MemoryPropertyFlags::HOST_VISIBLE.as_raw()
        | vk::MemoryPropertyFlags::HOST_COHERENT.as_raw()
        | vk::MemoryPropertyFlags::HOST_CACHED.as_raw(),
);
pub(crate) const PROPS_HOST: vk::MemoryPropertyFlags = vk::MemoryPropertyFlags::from_raw(
    vk::MemoryPropertyFlags::HOST_VISIBLE.as_raw()
        | vk::MemoryPropertyFlags::HOST_COHERENT.as_raw(),
);
const PROPS_HOST_CACHED: vk::MemoryPropertyFlags = vk::MemoryPropertyFlags::from_raw(
    PROPS_HOST.as_raw() | vk::MemoryPropertyFlags::HOST_CACHED.as_raw(),
);

/// **The arm `props` allow**: in place where some memory type has
/// [`PROPS_IN_PLACE`] and its heap is at least `need` bytes; staging
/// otherwise.
fn arm_of(props: &vk::PhysicalDeviceMemoryProperties, need: vk::DeviceSize) -> Arm {
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

/// **What staging's host buffer is allocated with**: [`PROPS_HOST_CACHED`]
/// where some memory type of `props` has it, [`PROPS_HOST`] otherwise.
fn host_props(props: &vk::PhysicalDeviceMemoryProperties) -> vk::MemoryPropertyFlags {
    let n = usize::try_from(props.memory_type_count).unwrap_or(0);
    match props.memory_types[..n.min(props.memory_types.len())]
        .iter()
        .any(|t| t.property_flags.contains(PROPS_HOST_CACHED))
    {
        true => PROPS_HOST_CACHED,
        false => PROPS_HOST,
    }
}

impl Gpu {
    /// **The arm this device's heaps allow** ([`arm_of`]).
    pub fn arm(&self, need: vk::DeviceSize) -> Arm {
        arm_of(&self.memory(), need)
    }

    /// This device's memory types and heaps.
    #[allow(unsafe_code)]
    fn memory(&self) -> vk::PhysicalDeviceMemoryProperties {
        // SAFETY: a physical device of this instance.
        unsafe {
            self.vulkan
                .instance
                .get_physical_device_memory_properties(self.physical)
        }
    }

    /// **A buffer of `size` bytes in memory with `props`, bound**, and its
    /// mapping where the memory is host-visible. A failure at any step
    /// destroys what the steps before it made.
    #[allow(unsafe_code)]
    pub(crate) fn bound(
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
        self.object_made();
        // SAFETY: the buffer was just made.
        let req = unsafe { d.get_buffer_memory_requirements(buffer) };
        let Some(kind) = self.memory_type(&req, props, Fallback::None) else {
            // SAFETY: unbound and unused.
            unsafe { d.destroy_buffer(buffer, None) };
            self.object_dropped();
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
                // SAFETY: unbound and unused.
                unsafe { d.destroy_buffer(buffer, None) };
                self.object_dropped();
                return Err(e);
            }
        };
        self.object_made();
        let made = Buffer { buffer, memory };
        let mapped = self
            .call()
            // SAFETY: bound once, at offset 0.
            .and_then(|()| unsafe { d.bind_buffer_memory(buffer, memory, 0) }.map_err(Failed::from))
            .and_then(
                |()| match props.contains(vk::MemoryPropertyFlags::HOST_VISIBLE) {
                    true => self.call().and_then(|()| {
                        // SAFETY: a host-visible memory, mapped whole; it stays
                        // mapped until it is freed.
                        unsafe {
                            d.map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                        }
                        .map(|at| Some(at.cast::<u8>()))
                        .map_err(Failed::from)
                    }),
                    false => Ok(None),
                },
            );
        match mapped {
            Ok(at) => Ok((made, at)),
            Err(e) => {
                self.drop_buffer(made);
                Err(e)
            }
        }
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
                let (h, at) = match self.bound(
                    bytes,
                    vk::BufferUsageFlags::TRANSFER_SRC,
                    host_props(&self.memory()),
                ) {
                    Ok(made) => made,
                    Err(e) => {
                        self.drop_buffer(d);
                        return Err(e);
                    }
                };
                (d, Some(h), at)
            }
        };
        let Some(at) = at else {
            self.drop_buffer(device);
            if let Some(h) = host {
                self.drop_buffer(h);
            }
            return Err(Failed::NoMemoryType);
        };
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

    /// Its buffers: the one the stage reads, then the mapped one under
    /// staging.
    pub(crate) fn buffers(&self) -> impl Iterator<Item = &Buffer> {
        std::iter::once(&self.device).chain(&self.host)
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

#[cfg(test)]
mod tests {
    use super::*;
    use vk::MemoryPropertyFlags as F;

    /// A device's memory: each type's flags and its heap's size in GiB.
    fn memory(types: &[(F, u64)]) -> vk::PhysicalDeviceMemoryProperties {
        let mut p = vk::PhysicalDeviceMemoryProperties::default();
        for (i, &(flags, gib)) in types.iter().enumerate() {
            p.memory_types[i] = vk::MemoryType {
                property_flags: flags,
                heap_index: u32::try_from(i).unwrap_or(0),
            };
            p.memory_heaps[i] = vk::MemoryHeap {
                size: gib << 30,
                flags: vk::MemoryHeapFlags::empty(),
            };
        }
        p.memory_type_count = u32::try_from(types.len()).unwrap_or(0);
        p.memory_heap_count = p.memory_type_count;
        p
    }

    /// **A discrete device's mapped VRAM is not where the CPU reads**: its
    /// device-local, host-visible type is uncached, so the cell path
    /// stages, through a host-visible type that is cached; a device whose
    /// device-local memory is host-cached, as unified memory is, lends in
    /// place. The discrete table is a Radeon RX 9070's under RADV.
    #[test]
    fn the_cpu_reads_only_cached_memory() {
        let (local, host) = (F::DEVICE_LOCAL, F::HOST_VISIBLE | F::HOST_COHERENT);
        let discrete = memory(&[
            (local, 16),
            (local, 16),
            (host, 94),
            (local | host, 16),
            (local | host, 16),
            (host | F::HOST_CACHED, 94),
            (host | F::HOST_CACHED, 94),
        ]);
        assert_eq!(arm_of(&discrete, 1 << 20), Arm::Staging);
        assert!(host_props(&discrete) == PROPS_HOST_CACHED, "discrete host");
        let unified = memory(&[(local | host | F::HOST_CACHED, 188)]);
        assert_eq!(arm_of(&unified, 1 << 20), Arm::InPlace);
        let uncached = memory(&[(local, 8), (host, 8)]);
        assert!(host_props(&uncached) == PROPS_HOST, "uncached host");
    }
}
