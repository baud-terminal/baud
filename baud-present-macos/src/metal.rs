//! Metal: the same instance records and background words the CPU
//! compositor paints, drawn by the GPU into the IOSurface the layer shows.
//!
//! The design is `docs/research/notes/j-rendering.md` D2, D3 and D5. Every
//! frame uploads the whole instance array by one copy into a `.shared`
//! buffer from a ring of three -- D2: nobody uploads partial ranges, and on
//! Apple silicon a shared buffer is the GPU's own memory -- and the whole
//! background bitmap into a `cols` by `rows` texture. The two atlas sheets
//! are uploaded only when [`Atlas::generation`] moved. One full-screen pass
//! paints the background, one instanced draw paints the glyphs, and the
//! blend is premultiplied `ONE, ONE_MINUS_SRC_ALPHA`, so the surface holds
//! what Core Animation reads without the CPU path's premultiplying copy.
//!
//! The shaders are Metal Shading Language compiled at first use from the
//! source below, because a `.metallib` needs Xcode's toolchain at build
//! time and the source is forty lines.
//!
//! **This is the one module in the crate that needs `unsafe`.** Every
//! `objc2-metal` call that takes a pointer or an index the framework does
//! not bounds-check is `unsafe fn`, and each call here carries the reason
//! its contract holds. The crate denies `unsafe_code` and this module
//! allows it, so the boundary is one attribute wide.
#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use crate::slots;
use baud_domain::{Cols, Rows};
use baud_font::Px;
use baud_render::{Argb, Atlas, Generation, Instance, Renderer, Side, Tally};
use core::ffi::c_void;
use core::ptr::NonNull;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_io_surface::{IOSurface, IOSurfaceRef};
use objc2_metal::{
    MTLBlendFactor, MTLBuffer, MTLClearColor, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary, MTLLoadAction, MTLOrigin, MTLPixelFormat,
    MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLResourceOptions, MTLSamplerDescriptor,
    MTLSamplerMinMagFilter, MTLSamplerState, MTLSize, MTLStorageMode, MTLStoreAction, MTLTexture,
    MTLTextureDescriptor, MTLTextureUsage,
};
use std::time::{Duration, Instant};

/// How many instance buffers rotate: one the GPU may still be reading, one
/// queued, one being written.
const RING: usize = 3;

/// The smallest instance buffer allocated, so a sparse screen does not
/// reallocate as it fills.
const BUFFER_FLOOR: usize = 64 * 1024;

/// Uniforms as the shaders read them: viewport, cell, grid, mask side and
/// colour side, each a `float2`.
const UNIFORM_FLOATS: usize = 10;

const SHADERS: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct U { float2 viewport; float2 cell; float2 grid; float2 mask; float2 color; };
struct Inst { short2 xy; ushort2 wh; ushort2 uv; uint color; uint kind; };

struct BgOut { float4 pos [[position]]; };

vertex BgOut bg_vertex(uint vid [[vertex_id]]) {
    float2 c = float2(vid & 1u, vid >> 1u);
    BgOut o;
    o.pos = float4(c * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

fragment float4 bg_fragment(BgOut in [[stage_in]],
                            constant U& u [[buffer(0)]],
                            texture2d<float, access::read> bg [[texture(0)]]) {
    float2 cf = in.pos.xy / u.cell;
    if (cf.x >= u.grid.x || cf.y >= u.grid.y) return float4(0.0);
    float4 c = bg.read(uint2(cf));
    return float4(c.rgb * c.a, c.a);
}

struct GOut { float4 pos [[position]]; float2 uv; float4 color; uint kind [[flat]]; };

vertex GOut glyph_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                         device const Inst* insts [[buffer(0)]],
                         constant U& u [[buffer(1)]]) {
    Inst i = insts[iid];
    float2 c = float2(vid & 1u, vid >> 1u);
    float2 p = float2(i.xy) + c * float2(i.wh);
    float2 n = p / u.viewport;
    GOut o;
    o.pos = float4(n.x * 2.0 - 1.0, 1.0 - n.y * 2.0, 0.0, 1.0);
    float2 side = i.kind == 1u ? u.color : u.mask;
    o.uv = (float2(i.uv) + c * float2(i.wh)) / side;
    uint col = i.color;
    o.color = float4(float((col >> 16) & 0xffu), float((col >> 8) & 0xffu),
                     float(col & 0xffu), float(col >> 24)) / 255.0;
    o.kind = i.kind;
    return o;
}

fragment float4 glyph_fragment(GOut in [[stage_in]],
                               texture2d<float> mask [[texture(0)]],
                               texture2d<float> color [[texture(1)]],
                               sampler s [[sampler(0)]]) {
    if (in.kind == 0u) {
        float a = in.color.a * mask.sample(s, in.uv).r;
        return float4(in.color.rgb * a, a);
    }
    if (in.kind == 1u) {
        float4 c = color.sample(s, in.uv);
        return float4(c.rgb * c.a, c.a);
    }
    return float4(in.color.rgb * in.color.a, in.color.a);
}
"#;

type Texture = Retained<ProtocolObject<dyn MTLTexture>>;
type Buffer = Retained<ProtocolObject<dyn MTLBuffer>>;

/// A texture the GPU draws a frame into.
pub struct Target {
    texture: Texture,
    width: Px,
    height: Px,
}

impl Target {
    pub fn size(&self) -> (Px, Px) {
        (self.width, self.height)
    }
}

/// What one GPU frame cost: building the uploads, the wall time from
/// commit to completion, and the GPU's own start-to-end.
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuFrame {
    pub instances: Tally,
    pub upload: Duration,
    pub submit: Duration,
    pub gpu: Duration,
}

/// Which of the ring's instance buffers is written next.
#[derive(Clone, Copy)]
struct Ring(usize);

/// One atlas sheet on the GPU.
struct Sheet {
    texture: Texture,
    side: Side,
}

/// The background bitmap on the GPU, one texel a cell.
struct Background {
    texture: Texture,
    cols: Cols,
    rows: Rows,
}

pub struct Gpu {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    bg_pipe: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    glyph_pipe: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    sampler: Retained<ProtocolObject<dyn MTLSamplerState>>,
    instances: Vec<Buffer>,
    ring: Ring,
    background: Option<Background>,
    mask: Option<Sheet>,
    color: Option<Sheet>,
    /// The atlas generation the sheets were uploaded at.
    uploaded: Option<Generation>,
}

fn pipeline(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    vertex: &str,
    fragment: &str,
) -> Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
    let vf = library.newFunctionWithName(&NSString::from_str(vertex))?;
    let ff = library.newFunctionWithName(&NSString::from_str(fragment))?;
    let desc = MTLRenderPipelineDescriptor::new();
    desc.setVertexFunction(Some(&vf));
    desc.setFragmentFunction(Some(&ff));
    // SAFETY: attachment 0 exists on every pipeline descriptor; the
    // framework does not bounds-check the index, so only 0 is used.
    let att = unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) };
    att.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    att.setBlendingEnabled(true);
    att.setSourceRGBBlendFactor(MTLBlendFactor::One);
    att.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
    att.setSourceAlphaBlendFactor(MTLBlendFactor::One);
    att.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
    match device.newRenderPipelineStateWithDescriptor_error(&desc) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("baud-present-macos: pipeline {vertex}/{fragment}: {e:?}");
            None
        }
    }
}

/// A 2D texture of `format`, `w` by `h`, for shader reads.
fn texture(
    device: &ProtocolObject<dyn MTLDevice>,
    format: MTLPixelFormat,
    w: Px,
    h: Px,
    usage: MTLTextureUsage,
) -> Option<Texture> {
    // SAFETY: the descriptor constructor takes plain values; a zero side
    // is refused by `newTextureWithDescriptor` returning `None`.
    let desc = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            format,
            slots(w),
            slots(h),
            false,
        )
    };
    desc.setUsage(usage);
    desc.setStorageMode(MTLStorageMode::Shared);
    device.newTextureWithDescriptor(&desc)
}

/// Copies `bytes` -- exactly `h` rows, so a row is `len / h` bytes --
/// into the whole of a `w` by `h` texture.
fn upload(texture: &ProtocolObject<dyn MTLTexture>, w: Px, h: Px, bytes: &[u8]) {
    let rows = slots(h);
    if rows == 0 || !bytes.len().is_multiple_of(rows) {
        return;
    }
    let row_bytes = bytes.len() / rows;
    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: slots(w),
            height: slots(h),
            depth: 1,
        },
    };
    // SAFETY: `bytes` holds exactly `row_bytes * h` bytes, and the region
    // is the texture's own extent, so the copy reads inside the slice and
    // writes inside the texture; Metal validates the row against the
    // width.
    unsafe {
        texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
            region,
            0,
            NonNull::from(bytes).cast::<c_void>(),
            row_bytes,
        );
    }
}

impl Gpu {
    /// The system default device with both pipelines compiled; `None` when
    /// there is no Metal device or the shaders fail to compile.
    pub fn new() -> Option<Gpu> {
        let device = MTLCreateSystemDefaultDevice()?;
        let queue = device.newCommandQueue()?;
        let library =
            match device.newLibraryWithSource_options_error(&NSString::from_str(SHADERS), None) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("baud-present-macos: shaders: {e:?}");
                    return None;
                }
            };
        let bg_pipe = pipeline(&device, &library, "bg_vertex", "bg_fragment")?;
        let glyph_pipe = pipeline(&device, &library, "glyph_vertex", "glyph_fragment")?;
        let sd = MTLSamplerDescriptor::new();
        sd.setMinFilter(MTLSamplerMinMagFilter::Nearest);
        sd.setMagFilter(MTLSamplerMinMagFilter::Nearest);
        let sampler = device.newSamplerStateWithDescriptor(&sd)?;
        let instances = (0..RING)
            .map(|_| {
                device.newBufferWithLength_options(
                    BUFFER_FLOOR,
                    MTLResourceOptions::StorageModeShared,
                )
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Gpu {
            device,
            queue,
            bg_pipe,
            glyph_pipe,
            sampler,
            instances,
            ring: Ring(0),
            background: None,
            mask: None,
            color: None,
            uploaded: None,
        })
    }

    /// Whether the device shares memory with the CPU, which is what makes
    /// the `.shared` ring the right choice (D2's discrete arm is unbuilt).
    pub fn unified_memory(&self) -> Memory {
        if self.device.hasUnifiedMemory() {
            Memory::Unified
        } else {
            Memory::Discrete
        }
    }

    /// A render target over `surface`, so a frame drawn into it is what the
    /// layer shows.
    pub fn target(&self, surface: &IOSurface, width: Px, height: Px) -> Option<Target> {
        // SAFETY: the descriptor constructor takes plain values.
        let desc = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::BGRA8Unorm,
                slots(width),
                slots(height),
                false,
            )
        };
        desc.setUsage(MTLTextureUsage::RenderTarget);
        desc.setStorageMode(MTLStorageMode::Shared);
        // SAFETY: `IOSurface` the Objective-C class and `IOSurfaceRef` the
        // Core Foundation type are toll-free bridged -- one object under
        // two names -- so a reference to one is a reference to the other
        // for the length of this borrow.
        let r: &IOSurfaceRef = unsafe { NonNull::from(surface).cast::<IOSurfaceRef>().as_ref() };
        let texture = self
            .device
            .newTextureWithDescriptor_iosurface_plane(&desc, r, 0)?;
        Some(Target {
            texture,
            width,
            height,
        })
    }

    /// A render target nothing shows, for measuring the draw alone.
    pub fn offscreen(&self, width: Px, height: Px) -> Option<Target> {
        let texture = texture(
            &self.device,
            MTLPixelFormat::BGRA8Unorm,
            width,
            height,
            MTLTextureUsage::RenderTarget,
        )?;
        Some(Target {
            texture,
            width,
            height,
        })
    }

    /// Makes sure the background texture matches the grid.
    fn background_for(&mut self, cols: Cols, rows: Rows) -> Option<&Background> {
        let stale = self
            .background
            .as_ref()
            .is_none_or(|b| b.cols.index() != cols.index() || b.rows.index() != rows.index());
        if stale {
            let w = Px::of(u32::try_from(cols.index()).ok()?);
            let h = Px::of(u32::try_from(rows.index()).ok()?);
            let texture = texture(
                &self.device,
                MTLPixelFormat::BGRA8Unorm,
                w,
                h,
                MTLTextureUsage::ShaderRead,
            )?;
            self.background = Some(Background {
                texture,
                cols,
                rows,
            });
        }
        self.background.as_ref()
    }

    /// Uploads both atlas sheets when the atlas changed since the last
    /// upload, recreating a texture whose side grew.
    fn sheets_for(&mut self, atlas: &Atlas) -> Option<()> {
        if self.uploaded == Some(atlas.generation()) && self.mask.is_some() && self.color.is_some()
        {
            return Some(());
        }
        let side = atlas.side();
        if self.mask.as_ref().is_none_or(|s| s.side != side) {
            let px = Px::of(side.value());
            let texture = texture(
                &self.device,
                MTLPixelFormat::R8Unorm,
                px,
                px,
                MTLTextureUsage::ShaderRead,
            )?;
            self.mask = Some(Sheet { texture, side });
        }
        let cside = atlas.color_side();
        if self.color.as_ref().is_none_or(|s| s.side != cside) {
            let px = Px::of(cside.value());
            let texture = texture(
                &self.device,
                MTLPixelFormat::BGRA8Unorm,
                px,
                px,
                MTLTextureUsage::ShaderRead,
            )?;
            self.color = Some(Sheet {
                texture,
                side: cside,
            });
        }
        let px = Px::of(side.value());
        upload(&self.mask.as_ref()?.texture, px, px, atlas.pixels());
        let cpx = Px::of(cside.value());
        upload(
            &self.color.as_ref()?.texture,
            cpx,
            cpx,
            word_bytes(atlas.argb()),
        );
        self.uploaded = Some(atlas.generation());
        Some(())
    }

    /// Draws the renderer's current frame into `target`, waiting for the
    /// GPU to finish so the surface may be shown. `None` when a resource
    /// could not be made.
    pub fn draw(&mut self, renderer: &Renderer<'_, '_>, target: &Target) -> Option<GpuFrame> {
        let t0 = Instant::now();
        let (cols, rows) = renderer.grid();
        let metrics = renderer.metrics();
        let count = renderer.instances().count();
        let bytes = count.saturating_mul(Instance::BYTES);
        // The instance ring.
        let slot = self.ring.0;
        if self.instances[slot].length() < bytes {
            let want = bytes.max(BUFFER_FLOOR).next_power_of_two();
            self.instances[slot] = self
                .device
                .newBufferWithLength_options(want, MTLResourceOptions::StorageModeShared)?;
        }
        {
            let buffer = &self.instances[slot];
            let ptr = buffer.contents().cast::<u8>();
            // SAFETY: a shared buffer's contents are CPU-addressable for
            // its whole length, which is at least `bytes` after the check
            // above; the GPU last read this slot two frames ago and has
            // completed, because every draw waits.
            let dst = unsafe { core::slice::from_raw_parts_mut(ptr.as_ptr(), bytes) };
            for (chunk, inst) in dst
                .chunks_exact_mut(Instance::BYTES)
                .zip(renderer.instances())
            {
                chunk.copy_from_slice(&inst.to_bytes());
            }
        }
        self.ring = Ring((slot + 1) % RING);
        // The background bitmap, whole.
        let bg = self.background_for(cols, rows)?;
        let (bw, bh) = (
            Px::of(u32::try_from(bg.cols.index()).ok()?),
            Px::of(u32::try_from(bg.rows.index()).ok()?),
        );
        upload(&bg.texture, bw, bh, argb_bytes(renderer.background()));
        let bg_texture = bg.texture.clone();
        self.sheets_for(renderer.atlas())?;
        let mask = self.mask.as_ref()?.texture.clone();
        let color = self.color.as_ref()?.texture.clone();
        let (tw, th) = target.size();
        let uniforms: [f32; UNIFORM_FLOATS] = [
            px_f32(tw),
            px_f32(th),
            px_f32(metrics.cell_w),
            px_f32(metrics.cell_h),
            px_f32(bw),
            px_f32(bh),
            px_f32(Px::of(self.mask.as_ref()?.side.value())),
            px_f32(Px::of(self.mask.as_ref()?.side.value())),
            px_f32(Px::of(self.color.as_ref()?.side.value())),
            px_f32(Px::of(self.color.as_ref()?.side.value())),
        ];
        let upload_time = t0.elapsed();
        let t1 = Instant::now();
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: attachment 0 exists on every pass descriptor.
        let att = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        att.setTexture(Some(&target.texture));
        att.setLoadAction(MTLLoadAction::Clear);
        att.setStoreAction(MTLStoreAction::Store);
        att.setClearColor(MTLClearColor {
            red: 0.0,
            green: 0.0,
            blue: 0.0,
            alpha: 0.0,
        });
        let cmd = self.queue.commandBuffer()?;
        let enc = cmd.renderCommandEncoderWithDescriptor(&pass)?;
        let ubytes = NonNull::from(&uniforms).cast::<c_void>();
        let ulen = core::mem::size_of::<[f32; UNIFORM_FLOATS]>();
        enc.setRenderPipelineState(&self.bg_pipe);
        // SAFETY: the byte pointers name `uniforms`, which outlives the
        // encoder (Metal copies `setBytes` data at the call); the buffer
        // and texture indices match the shader's `[[buffer(n)]]` and
        // `[[texture(n)]]` attributes; the instance buffer holds `count`
        // records, so an instance id never reads past it.
        unsafe {
            enc.setFragmentBytes_length_atIndex(ubytes, ulen, 0);
            enc.setFragmentTexture_atIndex(Some(&bg_texture), 0);
            enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::TriangleStrip, 0, 4);
            if count > 0 {
                enc.setRenderPipelineState(&self.glyph_pipe);
                enc.setVertexBuffer_offset_atIndex(Some(&self.instances[slot]), 0, 0);
                enc.setVertexBytes_length_atIndex(ubytes, ulen, 1);
                enc.setFragmentTexture_atIndex(Some(&mask), 0);
                enc.setFragmentTexture_atIndex(Some(&color), 1);
                enc.setFragmentSamplerState_atIndex(Some(&self.sampler), 0);
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                    MTLPrimitiveType::TriangleStrip,
                    0,
                    4,
                    count,
                );
            }
        }
        enc.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        let submit = t1.elapsed();
        let gpu = Duration::from_secs_f64((cmd.GPUEndTime() - cmd.GPUStartTime()).max(0.0));
        Some(GpuFrame {
            instances: Tally::of(count),
            upload: upload_time,
            submit,
            gpu,
        })
    }
}

/// Whether the GPU and CPU share one memory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Memory {
    Unified,
    Discrete,
}

/// `0xAARRGGBB` words as the bytes a `BGRA8Unorm` texture reads: on a
/// little-endian machine the word's memory order is already B, G, R, A.
fn word_bytes(words: &[u32]) -> &[u8] {
    // SAFETY: a `u32` slice is `4 * len` initialised bytes with no
    // padding, and a `&[u8]` over the same memory for the same lifetime
    // reads nothing else.
    unsafe {
        core::slice::from_raw_parts(words.as_ptr().cast::<u8>(), words.len().saturating_mul(4))
    }
}

/// The same for the compositor's colour words.
fn argb_bytes(words: &[Argb]) -> &[u8] {
    // SAFETY: `Argb` is `repr(transparent)` over `u32`, so a slice of it
    // is a slice of `u32` of the same length at the same address.
    let words: &[u32] =
        unsafe { core::slice::from_raw_parts(words.as_ptr().cast::<u32>(), words.len()) };
    word_bytes(words)
}

/// A pixel count as the shader's float; the widening is exact below
/// 2^24, and no side or grid here reaches it.
fn px_f32(p: Px) -> f32 {
    p.value() as f32
}
