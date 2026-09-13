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
use baud_domain::{Col, Cols, CursorStyle, PackedCell, Row, Rows};
use baud_font::Px;
use baud_render::{
    Argb, Atlas, Generation, GlyphMap, Instance, MAP_BITS, MapEntry, MapGeneration, Renderer,
    Resolved, Side, Tally,
};
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

/// The cell path's uniforms, all `uint`: viewport, cell, grid, the two
/// sides, the cursor cell, then the cursor kind, baseline, underline and
/// strike rows, thickness, the map's slot count and the hash shift.
const CELL_UNIFORMS: usize = 17;

/// How many shapes a cell may draw in the cell path's shape pass.
const SHAPES_PER_CELL: usize = 3;

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

// ---- The cell path: the raw grid, resolved per cell in the vertex stage.

struct CU {
    uint2 viewport; uint2 cell; uint2 grid; uint2 sides; uint2 cursor;
    uint cursor_kind; uint baseline; uint underline; uint strike;
    uint thickness; uint map_slots; uint map_shift;
};
struct Style { uint fg; uint bg; uint ul; uint shapes; };
struct Entry { uint key; uint kind; ushort4 rect; short2 at; uint pad; };

struct FlatOut { float4 pos [[position]]; float4 color; };

static float4 unpack(uint c) {
    return float4(float((c >> 16) & 0xffu), float((c >> 8) & 0xffu),
                  float(c & 0xffu), float(c >> 24)) / 255.0;
}

static float4 clip(float2 p, constant CU& u) {
    float2 n = p / float2(u.viewport);
    return float4(n.x * 2.0 - 1.0, 1.0 - n.y * 2.0, 0.0, 1.0);
}

// The cell's colours after the block cursor's swap, which the CPU path
// applies after the rendition rules and before anything is drawn.
static void colors(constant CU& u, device const Style* styles, uint2 cell,
                   uint col, uint row, thread uint& fgw, thread uint& bgw) {
    Style s = styles[cell.y & 0xffffu];
    fgw = s.fg; bgw = s.bg;
    if (u.cursor_kind == 1u && col == u.cursor.x && row == u.cursor.y) {
        uint t = fgw; fgw = bgw; bgw = t | 0xff000000u;
    }
}

vertex FlatOut cell_bg_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const uint2* cells [[buffer(0)]],
                              device const Style* styles [[buffer(1)]],
                              constant CU& u [[buffer(2)]]) {
    uint col = iid % u.grid.x, row = iid / u.grid.x;
    uint fgw, bgw;
    colors(u, styles, cells[iid], col, row, fgw, bgw);
    float2 c = float2(vid & 1u, vid >> 1u);
    float2 p = (float2(col, row) + c) * float2(u.cell);
    FlatOut o;
    o.pos = clip(p, u);
    float4 bg = unpack(bgw);
    o.color = float4(bg.rgb * bg.a, bg.a);
    return o;
}

fragment float4 flat_fragment(FlatOut in [[stage_in]]) { return in.color; }

vertex GOut cell_glyph_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const uint2* cells [[buffer(0)]],
                              device const Style* styles [[buffer(1)]],
                              constant CU& u [[buffer(2)]],
                              device const Entry* entries [[buffer(3)]]) {
    GOut o;
    o.pos = float4(0.0); o.uv = float2(0.0); o.color = float4(0.0); o.kind = 0u;
    uint2 cell = cells[iid];
    uint content = cell.x;
    if (content > 0x10FFFFu) return o;
    uint mask = u.map_slots - 1u;
    uint i = (content * 0x9E3779B1u) >> u.map_shift;
    Entry e; bool found = false;
    for (uint n = 0u; n < u.map_slots; n++) {
        e = entries[i];
        if (e.key == 0xFFFFFFFFu) break;
        if (e.key == content) { found = true; break; }
        i = (i + 1u) & mask;
    }
    if (!found || e.kind > 2u) return o;
    uint col = iid % u.grid.x, row = iid / u.grid.x;
    uint fgw, bgw;
    colors(u, styles, cell, col, row, fgw, bgw);
    float2 c = float2(vid & 1u, vid >> 1u);
    float2 origin = float2(float(col * u.cell.x) + float(e.at.x),
                           float(row * u.cell.y + u.baseline) + float(e.at.y));
    float2 wh = float2(e.rect.zw);
    o.pos = clip(origin + c * wh, u);
    float side = e.kind == 1u ? float(u.sides.y) : float(u.sides.x);
    o.uv = (float2(e.rect.xy) + c * wh) / side;
    o.color = unpack(fgw);
    o.kind = e.kind;
    return o;
}

// Three shapes a cell: its underline, its strike, and the cursor when it
// is an underline or a bar. A shape not drawn is a zero-area quad.
vertex GOut cell_shape_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const uint2* cells [[buffer(0)]],
                              device const Style* styles [[buffer(1)]],
                              constant CU& u [[buffer(2)]]) {
    GOut o;
    o.pos = float4(0.0); o.uv = float2(0.0); o.color = float4(0.0); o.kind = 2u;
    uint which = iid % 3u, ci = iid / 3u;
    uint col = ci % u.grid.x, row = ci / u.grid.x;
    uint2 cell = cells[ci];
    Style s = styles[cell.y & 0xffffu];
    uint fgw, bgw;
    colors(u, styles, cell, col, row, fgw, bgw);
    float2 at; float2 wh; uint colw;
    float thick = float(max(u.thickness, 2u));
    if (which == 0u) {
        if ((s.shapes & 1u) == 0u) return o;
        at = float2(col * u.cell.x, row * u.cell.y + u.underline);
        wh = float2(u.cell.x, u.thickness);
        colw = (s.shapes & 4u) != 0u ? fgw : s.ul;
    } else if (which == 1u) {
        if ((s.shapes & 2u) == 0u) return o;
        at = float2(col * u.cell.x, row * u.cell.y + u.strike);
        wh = float2(u.cell.x, u.thickness);
        colw = fgw;
    } else {
        if (col != u.cursor.x || row != u.cursor.y || u.cursor_kind < 2u) return o;
        if (u.cursor_kind == 2u) {
            at = float2(col * u.cell.x, float(row * u.cell.y + u.cell.y) - thick);
            wh = float2(u.cell.x, thick);
        } else {
            at = float2(col * u.cell.x, row * u.cell.y);
            wh = float2(thick, u.cell.y);
        }
        colw = fgw;
    }
    float2 c = float2(vid & 1u, vid >> 1u);
    o.pos = clip(at + c * wh, u);
    o.color = unpack(colw);
    return o;
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

/// What the cell path draws from: the grid as the terminal holds it, the
/// rendition table resolved, the glyph map, and where the cursor is.
pub struct CellInput<'a> {
    pub cells: &'a [PackedCell],
    pub styles: &'a [Resolved],
    pub map: &'a GlyphMap,
    pub cursor: Option<(Row, Col, CursorStyle)>,
}

/// How the cursor is drawn, as the shader switches on it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CursorKind {
    None,
    Block,
    Underline,
    Bar,
}

impl CursorKind {
    fn of(cursor: Option<(Row, Col, CursorStyle)>) -> CursorKind {
        match cursor.map(|(_, _, s)| s) {
            None => CursorKind::None,
            Some(CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock) => CursorKind::Block,
            Some(CursorStyle::BlinkingUnderline | CursorStyle::SteadyUnderline) => {
                CursorKind::Underline
            }
            Some(CursorStyle::BlinkingBar | CursorStyle::SteadyBar) => CursorKind::Bar,
        }
    }

    /// The discriminant the shader switches on.
    fn code(self) -> CursorCode {
        CursorCode(match self {
            CursorKind::None => 0,
            CursorKind::Block => 1,
            CursorKind::Underline => 2,
            CursorKind::Bar => 3,
        })
    }
}

/// A cursor kind as the uniform carries it.
#[derive(Clone, Copy)]
struct CursorCode(u32);

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
    cell_bg_pipe: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    cell_glyph_pipe: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    cell_shape_pipe: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    sampler: Retained<ProtocolObject<dyn MTLSamplerState>>,
    instances: Vec<Buffer>,
    ring: Ring,
    /// The cell path's grid and style rings, and its one map buffer --
    /// one, because every draw waits, so the GPU is never reading it
    /// while the next frame writes.
    cells: Vec<Buffer>,
    styles: Vec<Buffer>,
    map: Buffer,
    map_at: Option<MapGeneration>,
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
        let cell_bg_pipe = pipeline(&device, &library, "cell_bg_vertex", "flat_fragment")?;
        let cell_glyph_pipe = pipeline(&device, &library, "cell_glyph_vertex", "glyph_fragment")?;
        let cell_shape_pipe = pipeline(&device, &library, "cell_shape_vertex", "glyph_fragment")?;
        let sd = MTLSamplerDescriptor::new();
        sd.setMinFilter(MTLSamplerMinMagFilter::Nearest);
        sd.setMagFilter(MTLSamplerMinMagFilter::Nearest);
        let sampler = device.newSamplerStateWithDescriptor(&sd)?;
        let fresh = || {
            device.newBufferWithLength_options(BUFFER_FLOOR, MTLResourceOptions::StorageModeShared)
        };
        let instances = (0..RING).map(|_| fresh()).collect::<Option<Vec<_>>>()?;
        let cells = (0..RING).map(|_| fresh()).collect::<Option<Vec<_>>>()?;
        let styles = (0..RING).map(|_| fresh()).collect::<Option<Vec<_>>>()?;
        let map = fresh()?;
        Some(Gpu {
            device,
            queue,
            bg_pipe,
            glyph_pipe,
            cell_bg_pipe,
            cell_glyph_pipe,
            cell_shape_pipe,
            sampler,
            instances,
            ring: Ring(0),
            cells,
            styles,
            map,
            map_at: None,
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

    /// Makes `buffer` hold `bytes`, reallocating it when too small.
    fn fill(&self, buffer: &mut Buffer, bytes: &[u8]) -> Option<()> {
        if buffer.length() < bytes.len() {
            let want = bytes.len().max(BUFFER_FLOOR).next_power_of_two();
            *buffer = self
                .device
                .newBufferWithLength_options(want, MTLResourceOptions::StorageModeShared)?;
        }
        let ptr = buffer.contents().cast::<u8>();
        // SAFETY: a shared buffer's contents are CPU-addressable for its
        // whole length, at least `bytes.len()` after the check above, and
        // the GPU is not reading it: every draw waits for completion.
        let dst = unsafe { core::slice::from_raw_parts_mut(ptr.as_ptr(), bytes.len()) };
        dst.copy_from_slice(bytes);
        Some(())
    }

    /// Draws the raw grid: the cell path. Uploads the cells and the
    /// resolved styles whole, the glyph map when its generation moved,
    /// then a background pass, a glyph pass and a shape pass, each one
    /// instanced draw over every cell. Waits for completion.
    pub fn draw_cells(
        &mut self,
        renderer: &Renderer<'_, '_>,
        input: &CellInput<'_>,
        target: &Target,
    ) -> Option<GpuFrame> {
        let t0 = Instant::now();
        let (cols, rows) = renderer.grid();
        let count = cols.index().saturating_mul(rows.index());
        if input.cells.len() < count {
            return None;
        }
        let metrics = renderer.metrics();
        let slot = self.ring.0;
        let mut cells = self.cells[slot].clone();
        self.fill(&mut cells, cell_bytes(&input.cells[..count]))?;
        self.cells[slot] = cells;
        let mut styles = self.styles[slot].clone();
        self.fill(&mut styles, resolved_bytes(input.styles))?;
        self.styles[slot] = styles;
        if self.map_at != Some(input.map.generation()) {
            let mut map = self.map.clone();
            self.fill(&mut map, entry_bytes(input.map.entries()))?;
            self.map = map;
            self.map_at = Some(input.map.generation());
        }
        self.ring = Ring((slot + 1) % RING);
        self.sheets_for(renderer.atlas())?;
        let mask = self.mask.as_ref()?.texture.clone();
        let color = self.color.as_ref()?.texture.clone();
        let (tw, th) = target.size();
        let (crow, ccol) = input.cursor.map_or((u32::MAX, u32::MAX), |(r, c, _)| {
            (
                u32::try_from(r.index()).unwrap_or(u32::MAX),
                u32::try_from(c.index()).unwrap_or(u32::MAX),
            )
        });
        let uniforms: [u32; CELL_UNIFORMS] = [
            tw.value(),
            th.value(),
            metrics.cell_w.value(),
            metrics.cell_h.value(),
            u32::try_from(cols.index()).unwrap_or(0),
            u32::try_from(rows.index()).unwrap_or(0),
            self.mask.as_ref()?.side.value(),
            self.color.as_ref()?.side.value(),
            ccol,
            crow,
            CursorKind::of(input.cursor).code().0,
            metrics.baseline.value(),
            metrics.underline.value(),
            metrics.strike.value(),
            metrics.thickness.value(),
            u32::try_from(input.map.entries().len()).unwrap_or(0),
            32 - MAP_BITS,
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
        let ulen = core::mem::size_of::<[u32; CELL_UNIFORMS]>();
        // SAFETY: as in `draw` -- `uniforms` outlives the encoder and Metal
        // copies `setBytes` data at the call; the buffer and texture indices
        // match the shaders' attributes; the cell buffer holds `count`
        // cells, the style buffer every slot a cell can name, and the map
        // buffer every slot the probe can reach.
        unsafe {
            for (pipe, instances) in [
                (&self.cell_bg_pipe, count),
                (&self.cell_glyph_pipe, count),
                (&self.cell_shape_pipe, count.saturating_mul(SHAPES_PER_CELL)),
            ] {
                enc.setRenderPipelineState(pipe);
                enc.setVertexBuffer_offset_atIndex(Some(&self.cells[slot]), 0, 0);
                enc.setVertexBuffer_offset_atIndex(Some(&self.styles[slot]), 0, 1);
                enc.setVertexBytes_length_atIndex(ubytes, ulen, 2);
                enc.setVertexBuffer_offset_atIndex(Some(&self.map), 0, 3);
                enc.setFragmentTexture_atIndex(Some(&mask), 0);
                enc.setFragmentTexture_atIndex(Some(&color), 1);
                enc.setFragmentSamplerState_atIndex(Some(&self.sampler), 0);
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                    MTLPrimitiveType::TriangleStrip,
                    0,
                    4,
                    instances,
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

    /// Reads a target back as `0xAARRGGBB` words, premultiplied, row-major
    /// at the target's width; `None` when `out` is too small.
    pub fn read(&self, target: &Target, out: &mut [u32]) -> Option<()> {
        let (w, h) = target.size();
        let (ws, hs) = (slots(w), slots(h));
        if out.len() < ws.saturating_mul(hs) {
            return None;
        }
        let region = MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: ws,
                height: hs,
                depth: 1,
            },
        };
        // SAFETY: `out` holds at least `w * h` words, checked above, and the
        // region is the texture's own extent at four bytes a texel, so the
        // copy writes inside the slice.
        unsafe {
            target.texture.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                NonNull::from(out).cast::<c_void>(),
                ws.saturating_mul(4),
                region,
                0,
            );
        }
        Some(())
    }
}

/// The grid as bytes: `PackedCell` is `repr(C)` `{u32, u16, u16}`, eight
/// bytes with no padding.
fn cell_bytes(cells: &[PackedCell]) -> &[u8] {
    // SAFETY: `PackedCell` is `repr(C)` over `u32`, `u16`, `u16` -- eight
    // initialised bytes with no padding -- so the slice is `8 * len` bytes.
    unsafe {
        core::slice::from_raw_parts(cells.as_ptr().cast::<u8>(), cells.len().saturating_mul(8))
    }
}

/// The resolved styles as bytes: `Resolved` is `repr(C)` over four `u32`s.
fn resolved_bytes(styles: &[Resolved]) -> &[u8] {
    // SAFETY: `Resolved` is `repr(C)` over four `repr(transparent)` `u32`
    // newtypes, sixteen initialised bytes with no padding.
    unsafe {
        core::slice::from_raw_parts(
            styles.as_ptr().cast::<u8>(),
            styles.len().saturating_mul(Resolved::BYTES),
        )
    }
}

/// The glyph map as bytes: `MapEntry` is `repr(C)`, twenty-four bytes.
fn entry_bytes(entries: &[MapEntry]) -> &[u8] {
    // SAFETY: `MapEntry` is `repr(C)` over `u32, u32, u16 x4, i16 x2, u32`
    // newtypes -- twenty-four initialised bytes with no padding.
    unsafe {
        core::slice::from_raw_parts(
            entries.as_ptr().cast::<u8>(),
            entries.len().saturating_mul(MapEntry::BYTES),
        )
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
