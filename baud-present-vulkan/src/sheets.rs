//! **The glyph sheets, one owner**: the mask and colour atlases as device
//! images that persist across frames, and the contrast curves as a buffer
//! written once. Every path that draws a glyph samples these; nothing
//! uploads an atlas per draw.
//!
//! A glyph that lands on a sheet reaches its image as a copy of the
//! rectangle it was written in ([`Sheets::bring`], from
//! `Atlas::take_dirty`), recorded in the next frame's command buffer ahead
//! of its draws, behind a barrier from the fragment reads of the frames
//! before. The rectangle's rows are written once, from the atlas's own
//! texels straight into the copy's staging buffer, row after row with no
//! padding; the colour sheet's words become their little-endian bytes on
//! the way. The atlas never moves or frees a slot within a sheet, so such a
//! copy fills only texels no rectangle has named, and no frame samples a
//! texel a copy writes. A sheet the atlas replaces (grown, or reset) is a
//! new image taken whole, and the old one is destroyed once the frame
//! after it has finished, never copied into.

use crate::draw::{Buffer, HostBytes, Texel, Texels, Texture};
use crate::frame::{Failed, Gpu};
use ash::vk;
use baud_present_wayland::Px;
use baud_render::{Atlas, Replaced, TexRect};
use std::cell::{Cell, RefCell};
use std::mem::MaybeUninit;

/// Which of the two sheets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    /// The mask atlas, a byte a texel.
    Mask,
    /// The colour atlas, `0xAARRGGBB` a texel.
    Color,
}

/// Texels copied to the sheets, counted when a frame records the copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Copied(pub u64);

/// **What the sheets' uploads wrote on the CPU** ([`Sheets::uploads`]):
/// the bytes copied into a vector on the way to staging, and the bytes
/// written to staging. No upload makes a vector, so `into_vec` stays zero;
/// it is what a test holds that to.
#[cfg(feature = "test-hooks")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Uploads {
    pub into_vec: u64,
    pub staged: u64,
}

/// Whether an image has been written since it was made: its first copy
/// takes it from `UNDEFINED`, every later one from the layout it is read in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fresh {
    Yes,
    No,
}

/// A sheet's image, its size and its texel.
struct Sheet {
    texture: Texture,
    width: Px,
    height: Px,
    texel: Texel,
    fresh: Cell<Fresh>,
}

/// **Where a sheet's texels are read from, in place**: bytes as the
/// texel lays them out, or `0xAARRGGBB` words, staged as their
/// little-endian bytes.
#[derive(Clone, Copy)]
enum Plane<'a> {
    Bytes(&'a [u8]),
    Words(&'a [u32]),
}

/// **A rectangle of a plane** `stride` texels wide: its texel, its
/// top-left in the plane and its size.
#[derive(Clone, Copy)]
struct Cut<'a> {
    plane: Plane<'a>,
    texel: Texel,
    stride: Px,
    x: Px,
    y: Px,
    width: Px,
    height: Px,
}

impl<'a> Cut<'a> {
    /// `t`, whole.
    fn whole(t: &Texels<'a>) -> Cut<'a> {
        Cut {
            plane: Plane::Bytes(t.bytes),
            texel: t.texel,
            stride: t.width,
            x: Px(0),
            y: Px(0),
            width: t.width,
            height: t.height,
        }
    }

    /// The rectangle `r` of this one's plane.
    fn at(self, r: TexRect) -> Cut<'a> {
        Cut {
            x: Px(u32::from(r.x.0)),
            y: Px(u32::from(r.y.0)),
            width: Px(u32::from(r.w.0)),
            height: Px(u32::from(r.h.0)),
            ..self
        }
    }

    /// The bytes it stages.
    fn len(&self) -> HostBytes {
        let n = |p: Px| usize::try_from(p.0).unwrap_or(0);
        HostBytes(n(self.width) * n(self.height) * self.texel.size().0)
    }

    /// **Its rows, one after another with no padding, written to `out`**,
    /// [`Cut::len`] bytes; a row the plane does not hold is written clear,
    /// so no staging byte goes to the device unwritten.
    fn write(&self, out: &mut [MaybeUninit<u8>]) {
        let n = |p: Px| usize::try_from(p.0).unwrap_or(0);
        let HostBytes(bpp) = self.texel.size();
        let w = n(self.width);
        let Some(row) = core::num::NonZeroUsize::new(w * bpp) else {
            return;
        };
        for (r, dst) in out.chunks_exact_mut(row.get()).enumerate() {
            let from = (n(self.y) + r) * n(self.stride) + n(self.x);
            match self.plane {
                Plane::Bytes(b) => match b.get(from * bpp..from * bpp + row.get()) {
                    Some(src) => {
                        dst.write_copy_of_slice(src);
                    }
                    None => {
                        dst.fill(MaybeUninit::new(0));
                    }
                },
                Plane::Words(ws) => match ws.get(from..from + w) {
                    Some(src) => {
                        for (d, word) in dst.as_chunks_mut::<4>().0.iter_mut().zip(src) {
                            *d = word.to_le_bytes().map(MaybeUninit::new);
                        }
                    }
                    None => {
                        dst.fill(MaybeUninit::new(0));
                    }
                },
            }
        }
    }
}

/// A copy owed to a sheet: the texels in a staging buffer and where they go.
struct Pending {
    staging: Buffer,
    which: Which,
    at: (Px, Px),
    width: Px,
    height: Px,
}

/// **The two sheets and the curves**, and the copies owed to them.
pub struct Sheets {
    mask: Sheet,
    color: Sheet,
    curves: Buffer,
    pub(crate) sampler: vk::Sampler,
    pending: RefCell<Vec<Pending>>,
    /// Staging buffers whose copies a frame has recorded, freed once it has
    /// finished.
    recorded: RefCell<Vec<Buffer>>,
    /// Images replaced, destroyed once the frame after has finished.
    retired: RefCell<Vec<Texture>>,
    copied: Cell<Copied>,
    #[cfg(feature = "test-hooks")]
    uploads: Cell<Uploads>,
}

impl Gpu {
    /// **The sheets, each one transparent texel, and `curves`**, the
    /// contrast curves (`baud_domain::contrast::CURVES` in a frame), in a
    /// buffer written once.
    #[allow(unsafe_code)]
    pub fn sheets(&self, curves: &[u8]) -> Result<Sheets, Failed> {
        let one = |texel| Texels {
            width: Px(1),
            height: Px(1),
            texel,
            bytes: match texel {
                Texel::Coverage => &[0],
                Texel::Argb => &[0, 0, 0, 0],
            },
        };
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::NEAREST)
            .min_filter(vk::Filter::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
        self.call()?;
        // SAFETY: a device of this struct.
        let sampler = unsafe { self.device.create_sampler(&sampler_info, None) }?;
        self.object_made();
        // A failure part way destroys what the steps before it made.
        let unmake = |made: (Option<Sheet>, Option<Sheet>)| {
            for s in [made.0, made.1].into_iter().flatten() {
                self.drop_texture(s.texture);
            }
            // SAFETY: made above and never used.
            unsafe { self.device.destroy_sampler(sampler, None) };
            self.object_dropped();
        };
        let mask = match self.sheet(Texel::Coverage, Px(1), Px(1)) {
            Ok(s) => s,
            Err(e) => {
                unmake((None, None));
                return Err(e);
            }
        };
        let color = match self.sheet(Texel::Argb, Px(1), Px(1)) {
            Ok(s) => s,
            Err(e) => {
                unmake((Some(mask), None));
                return Err(e);
            }
        };
        let curves = match self.buffer(curves, vk::BufferUsageFlags::STORAGE_BUFFER) {
            Ok(b) => b,
            Err(e) => {
                unmake((Some(mask), Some(color)));
                return Err(e);
            }
        };
        let mut sheets = Sheets {
            mask,
            color,
            curves,
            sampler,
            pending: RefCell::default(),
            recorded: RefCell::default(),
            retired: RefCell::default(),
            copied: Cell::default(),
            #[cfg(feature = "test-hooks")]
            uploads: Cell::default(),
        };
        let copied = sheets
            .copy(self, Which::Mask, (Px(0), Px(0)), &one(Texel::Coverage))
            .and_then(|()| sheets.copy(self, Which::Color, (Px(0), Px(0)), &one(Texel::Argb)));
        match copied {
            Ok(()) => Ok(sheets),
            Err(e) => {
                sheets.destroy(self);
                Err(e)
            }
        }
    }

    fn sheet(&self, texel: Texel, width: Px, height: Px) -> Result<Sheet, Failed> {
        Ok(Sheet {
            texture: self.image(texel, width, height)?,
            width,
            height,
            texel,
            fresh: Cell::new(Fresh::Yes),
        })
    }
}

impl Sheets {
    fn get(&self, which: Which) -> &Sheet {
        match which {
            Which::Mask => &self.mask,
            Which::Color => &self.color,
        }
    }

    fn get_mut(&mut self, which: Which) -> &mut Sheet {
        match which {
            Which::Mask => &mut self.mask,
            Which::Color => &mut self.color,
        }
    }

    /// **A sheet replaced by `t`, whole**: a new image of `t`'s size, its
    /// texels owed to it, the old image retired.
    pub fn replace(&mut self, gpu: &Gpu, which: Which, t: &Texels<'_>) -> Result<(), Failed> {
        self.renew(gpu, which, Cut::whole(t))
    }

    /// **A sheet replaced by `whole`'s texels**, as [`Sheets::replace`].
    fn renew(&mut self, gpu: &Gpu, which: Which, whole: Cut<'_>) -> Result<(), Failed> {
        let new = gpu.sheet(whole.texel, whole.width, whole.height)?;
        let old = std::mem::replace(self.get_mut(which), new);
        // Copies owed to the old image go with it.
        let (gone, kept): (Vec<_>, Vec<_>) = self
            .pending
            .take()
            .into_iter()
            .partition(|p| p.which == which);
        *self.pending.get_mut() = kept;
        for p in gone {
            gpu.drop_buffer(p.staging);
        }
        self.retired.get_mut().push(old.texture);
        self.stage(gpu, which, (Px(0), Px(0)), &whole)
    }

    /// **`t`'s texels owed to a sheet at `at`**, copied by the next frame.
    pub fn copy(
        &mut self,
        gpu: &Gpu,
        which: Which,
        at: (Px, Px),
        t: &Texels<'_>,
    ) -> Result<(), Failed> {
        self.stage(gpu, which, at, &Cut::whole(t))
    }

    /// **`cut`'s texels owed to a sheet at `at`**, written once, straight
    /// into the copy's staging buffer.
    fn stage(
        &mut self,
        gpu: &Gpu,
        which: Which,
        at: (Px, Px),
        cut: &Cut<'_>,
    ) -> Result<(), Failed> {
        let sheet = self.get(which);
        if cut.texel != sheet.texel
            || at.0.0.saturating_add(cut.width.0) > sheet.width.0
            || at.1.0.saturating_add(cut.height.0) > sheet.height.0
        {
            return Err(Failed::Outside);
        }
        let len = cut.len();
        let staging = gpu.filled(len, vk::BufferUsageFlags::TRANSFER_SRC, |m| cut.write(m))?;
        self.counted(len);
        self.pending.get_mut().push(Pending {
            staging,
            which,
            at,
            width: cut.width,
            height: cut.height,
        });
        Ok(())
    }

    /// **The sheets brought up to `atlas`**: a sheet the atlas replaced, or
    /// one whose side differs, made afresh and taken whole; otherwise the
    /// rectangle written since the last call, and no more.
    pub fn bring(&mut self, gpu: &Gpu, atlas: &mut Atlas) -> Result<(), Failed> {
        let (dirty, mask_replaced, color_replaced) = atlas.take_dirty();
        let side = Px(atlas.side().value());
        let cside = Px(atlas.color_side().value());
        let mask = Cut {
            plane: Plane::Bytes(atlas.pixels()),
            texel: Texel::Coverage,
            stride: side,
            x: Px(0),
            y: Px(0),
            width: side,
            height: side,
        };
        let color = Cut {
            plane: Plane::Words(atlas.argb()),
            texel: Texel::Argb,
            stride: cside,
            x: Px(0),
            y: Px(0),
            width: cside,
            height: cside,
        };
        for (which, whole, replaced, rect) in [
            (Which::Mask, mask, mask_replaced, dirty.mask),
            (Which::Color, color, color_replaced, dirty.color),
        ] {
            let s = self.get(which);
            if replaced == Replaced::Yes || s.width != whole.width || s.height != whole.height {
                self.renew(gpu, which, whole)?;
            } else if let Some(r) = rect {
                self.stage(
                    gpu,
                    which,
                    (Px(u32::from(r.x.0)), Px(u32::from(r.y.0))),
                    &whole.at(r),
                )?;
            }
        }
        Ok(())
    }

    /// The views and buffer set 1 binds: mask, colour, curves.
    pub(crate) fn bound(&self) -> (vk::ImageView, vk::ImageView, vk::Buffer) {
        (
            self.mask.texture.view,
            self.color.texture.view,
            self.curves.buffer,
        )
    }

    /// **The owed copies recorded into `cb`**, ahead of the frame's draws:
    /// each sheet taken from the fragment reads before (or from nothing, a
    /// new image) to a transfer write, copied, and given back to the
    /// fragment stage. Their staging is freed by [`Sheets::finished`].
    #[allow(unsafe_code)]
    pub(crate) fn record(&self, d: &ash::Device, cb: vk::CommandBuffer) {
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let pending = self.pending.take();
        for which in [Which::Mask, Which::Color] {
            let owed: Vec<&Pending> = pending.iter().filter(|p| p.which == which).collect();
            if owed.is_empty() {
                continue;
            }
            let sheet = self.get(which);
            let image = sheet.texture.image;
            let (from, after) = match sheet.fresh.get() {
                Fresh::Yes => (
                    vk::ImageLayout::UNDEFINED,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                ),
                Fresh::No => (
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                ),
            };
            let to_dst = vk::ImageMemoryBarrier::default()
                .old_layout(from)
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
            // SAFETY: the image and the staging buffers are alive until the
            // frame that records this has finished (`finished`).
            unsafe {
                d.cmd_pipeline_barrier(
                    cb,
                    after,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[to_dst],
                );
                for p in &owed {
                    let region = [vk::BufferImageCopy::default()
                        .image_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .layer_count(1),
                        )
                        .image_offset(vk::Offset3D {
                            x: i32::try_from(p.at.0.0).unwrap_or(i32::MAX),
                            y: i32::try_from(p.at.1.0).unwrap_or(i32::MAX),
                            z: 0,
                        })
                        .image_extent(vk::Extent3D {
                            width: p.width.0,
                            height: p.height.0,
                            depth: 1,
                        })];
                    d.cmd_copy_buffer_to_image(
                        cb,
                        p.staging.buffer,
                        image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &region,
                    );
                }
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
            sheet.fresh.set(Fresh::No);
        }
        let mut copied = self.copied.get();
        let mut recorded = self.recorded.borrow_mut();
        for p in pending {
            copied.0 += u64::from(p.width.0) * u64::from(p.height.0);
            recorded.push(p.staging);
        }
        self.copied.set(copied);
    }

    /// **The frame that recorded the copies has finished**: their staging
    /// and the images replaced before it are freed.
    pub(crate) fn finished(&self, gpu: &Gpu) {
        for b in self.recorded.take() {
            gpu.drop_buffer(b);
        }
        for t in self.retired.take() {
            gpu.drop_texture(t);
        }
    }

    /// Every byte the uploads copied into a vector and wrote to staging.
    #[cfg(feature = "test-hooks")]
    pub fn uploads(&self) -> Uploads {
        self.uploads.get()
    }

    /// `staged` bytes written to staging, counted under `test-hooks`.
    fn counted(&self, staged: HostBytes) {
        #[cfg(feature = "test-hooks")]
        {
            let mut u = self.uploads.get();
            u.staged += u64::try_from(staged.0).unwrap_or(u64::MAX);
            self.uploads.set(u);
        }
        #[cfg(not(feature = "test-hooks"))]
        let _ = staged;
    }

    /// Every texel a frame has copied to a sheet.
    pub fn copied(&self) -> Copied {
        self.copied.get()
    }

    /// Every image and buffer destroyed. The caller has waited for the
    /// device to be idle.
    #[allow(unsafe_code)]
    pub fn destroy(self, gpu: &Gpu) {
        self.finished(gpu);
        for p in self.pending.take() {
            gpu.drop_buffer(p.staging);
        }
        gpu.drop_texture(self.mask.texture);
        gpu.drop_texture(self.color.texture);
        gpu.drop_buffer(self.curves);
        // SAFETY: the device is idle; nothing samples through it.
        unsafe { gpu.device.destroy_sampler(self.sampler, None) };
        gpu.object_dropped();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A row the plane does not hold is staged clear**: a 2x3 coverage
    /// plane cut as 2x4 writes its three rows and a fourth of zeros over
    /// whatever the staging memory held. Named break: the missing row left
    /// as it was, so stale memory went to the device.
    #[test]
    #[allow(unsafe_code)]
    fn a_row_the_plane_lacks_is_staged_clear() {
        let bytes = [1u8, 2, 3, 4, 5, 6];
        let t = Texels {
            width: Px(2),
            height: Px(3),
            texel: Texel::Coverage,
            bytes: &bytes,
        };
        let cut = Cut {
            height: Px(4),
            ..Cut::whole(&t)
        };
        let mut out = [MaybeUninit::new(0xAAu8); 8];
        cut.write(&mut out);
        let got = out.map(|b| {
            // SAFETY: every byte was initialised before the write.
            unsafe { b.assume_init() }
        });
        assert_eq!(got, [1, 2, 3, 4, 5, 6, 0, 0]);
    }
}
