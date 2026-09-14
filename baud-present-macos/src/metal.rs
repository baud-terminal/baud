//! Metal: the frame the renderer converts, or the raw grid the terminal
//! holds, drawn by the GPU into the IOSurface the layer shows -- without
//! copying what already sits in memory the GPU can read, and without
//! waiting for it.
//!
//! The design is `docs/research/notes/j-rendering.md` D2, D3, D5 and D13,
//! on a machine with one memory. Two paths share the shaders' second
//! half and differ in what the CPU does before the draw:
//!
//! * **Instances.** `Renderer::convert` resolves every dirty row into
//!   twenty-byte records on the CPU; `Gpu::draw` writes them into a
//!   `.shared` ring slot -- the write is the upload, D2's one copy -- and
//!   draws one background pass and one instanced glyph pass.
//! * **Cells.** Nothing is converted. The terminal's viewport *lives* in a
//!   shared buffer ([`Gpu::grid`] lends the core its memory), the rendition
//!   table is resolved straight into another ([`Gpu::styles`]), the glyph
//!   map is kept in a third ([`Gpu::map`]), and a vertex shader resolves
//!   every cell itself, undoing the core's row rotation with one modulo so
//!   the core never turns the grid in place either. `Gpu::draw_cells`
//!   uploads nothing; it binds.
//!
//! **Nothing waits.** A draw is committed with a completion handler, and
//! the caller's [`Done::Notify`] closure runs on the GPU's thread when the
//! frame is finished -- the presenter uses it to hand the surface to the
//! main thread and wake the responder. One frame is in flight at a time:
//! while it is, the buffers it reads are the ones the core writes, so the
//! caller keeps incoming bytes queued until [`Gpu::in_flight`] says idle,
//! and a draw asked for meanwhile answers [`Drawn::Busy`] rather than
//! blocking. That single atomic is the whole synchronisation, and it is
//! what makes the zero-copy grid safe: the GPU reads it only inside a
//! window the core does not write.
//!
//! The atlas is the one thing still copied, and only the texels a new
//! glyph wrote: the renderer's sheets are CPU memory the rasteriser
//! writes, and a texture takes them by rectangle
//! ([`baud_render::Atlas::take_dirty`]). After the first screenful that is
//! nothing at all.
//!
//! The shaders are Metal Shading Language compiled at first use, because a
//! `.metallib` needs Xcode's toolchain at build time and the source is
//! two hundred lines.
//!
//! **This is the one module in the crate that needs `unsafe`.** Every
//! `objc2-metal` call that takes a pointer or an index the framework does
//! not bounds-check is `unsafe fn`, the shared-memory slices are made from
//! buffer contents, and each site carries the reason its contract holds.
//! The crate denies `unsafe_code` and this module allows it, so the
//! boundary is one attribute wide.
#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use crate::slots;
use baud_core::Term;
use baud_domain::{Col, Cols, CursorStyle, PackedCell, Row, RowRot, Rows, Stride};
use baud_font::Px;
use baud_render::{
    Atlas, Dirty, Generation, Highlight, Instance, MAP_BITS, MAP_SLOTS, MapEntry, Renderer,
    Replaced, Resolved, Side, Tally, TexRect,
};
use block2::RcBlock;
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
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// How many instance buffers rotate on the instance path: one the GPU may
/// still be reading, one queued, one being written.
const RING: usize = 3;

/// The smallest instance buffer allocated, so a sparse screen does not
/// reallocate as it fills.
const BUFFER_FLOOR: usize = 64 * 1024;

/// Uniforms as the instance shaders read them: viewport, cell, grid, mask
/// side and colour side, each a `float2`.
const UNIFORM_FLOATS: usize = 10;

/// How many shapes a cell may draw in the cell path's shape pass.
const SHAPES_PER_CELL: usize = 4;

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
    uint thickness; uint map_slots; uint map_shift; uint stride; uint rot;
    uint ring;
    uint sel_on; uint sel_from_col; uint sel_from_row; uint sel_to_col;
    uint sel_to_row;
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

// The cell at screen row `row`, column `col`: the grid is read in place,
// under the core's rotation, at its live stride, modulo the ring -- the
// buffer's rows, which on the primary screen are the screen plus the
// scrollback behind it.
static uint2 cell_at(constant CU& u, device const uint2* cells, uint col, uint row) {
    uint prow = (row + u.rot) % u.ring;
    return cells[prow * u.stride + col];
}

// Whether the cell is inside the selection: row-major, both ends inclusive.
static bool selected(constant CU& u, uint col, uint row) {
    if (u.sel_on == 0u) return false;
    bool after_from = row > u.sel_from_row || (row == u.sel_from_row && col >= u.sel_from_col);
    bool before_to = row < u.sel_to_row || (row == u.sel_to_row && col <= u.sel_to_col);
    return after_from && before_to;
}

// The cell's colours after the selection's swap and then the block
// cursor's, which the CPU path applies in the same order after the
// rendition rules and before anything is drawn.
static void colors(constant CU& u, device const Style* styles, uint2 cell,
                   uint col, uint row, thread uint& fgw, thread uint& bgw) {
    Style s = styles[cell.y & 0xffffu];
    fgw = s.fg; bgw = s.bg;
    if (selected(u, col, row)) {
        uint t = fgw; fgw = bgw; bgw = t | 0xff000000u;
    }
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
    colors(u, styles, cell_at(u, cells, col, row), col, row, fgw, bgw);
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
                              device const Entry* entries [[buffer(3)]],
                              device const uchar* overlay [[buffer(4)]]) {
    GOut o;
    o.pos = float4(0.0); o.uv = float2(0.0); o.color = float4(0.0); o.kind = 0u;
    if (overlay[iid] != 0) return o;
    uint col = iid % u.grid.x, row = iid / u.grid.x;
    uint2 cell = cell_at(u, cells, col, row);
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

// Four shapes a cell: its underline, its strike, a second underline line for
// a double underline, and the cursor when it is an underline or a bar. A shape
// not drawn is a zero-area quad.
vertex GOut cell_shape_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const uint2* cells [[buffer(0)]],
                              device const Style* styles [[buffer(1)]],
                              constant CU& u [[buffer(2)]]) {
    GOut o;
    o.pos = float4(0.0); o.uv = float2(0.0); o.color = float4(0.0); o.kind = 2u;
    uint which = iid % 4u, ci = iid / 4u;
    uint col = ci % u.grid.x, row = ci / u.grid.x;
    uint2 cell = cell_at(u, cells, col, row);
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
    } else if (which == 2u) {
        // The second line of a double underline, below the first by a gap and
        // clamped inside the cell.
        if ((s.shapes & 8u) == 0u) return o;
        float th = float(u.thickness);
        float gap = max(th, 1.0) + 1.0;
        float low = float(u.cell.y) - th;
        float y = min(float(u.underline) + gap, max(low, float(u.underline)));
        at = float2(col * u.cell.x, float(row * u.cell.y) + y);
        wh = float2(u.cell.x, u.thickness);
        colw = (s.shapes & 4u) != 0u ? fgw : s.ul;
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
type CommandBuffer = Retained<ProtocolObject<dyn MTLCommandBuffer>>;
type Encoder = Retained<ProtocolObject<dyn MTLRenderCommandEncoder>>;

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
/// commit to completion or to the handler being armed, and the GPU's own
/// start-to-end where the frame was waited for.
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

/// Shared memory the terminal's viewport lives in: what [`Gpu::grid`]
/// lends the core and what the cell path binds. The GPU reads the cells
/// the core writes, in place.
pub struct GridBuffer(Buffer);

/// Shared memory the resolved rendition table lives in.
pub struct StylesBuffer(Buffer);

/// Shared memory the glyph map lives in.
pub struct MapBuffer(Buffer);

/// The overlay: one byte a screen cell, set where the records draw the
/// cell and the map must not.
pub struct OverlayBuffer(Buffer);

/// A count of elements in a shared buffer.
#[derive(Clone, Copy)]
struct Elems(usize);

/// The bytes between one row of a sheet and the next.
#[derive(Clone, Copy)]
struct RowBytes(usize);

/// A shared buffer's length in bytes.
#[derive(Clone, Copy)]
struct Span(usize);

/// A sheet's texel format, and so its bytes a texel.
#[derive(Clone, Copy)]
enum Texel {
    A8,
    Bgra8,
}

/// A count as a uniform word carries it.
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Uni(u32);

impl Uni {
    fn of(n: Elems) -> Uni {
        Uni(u32::try_from(n.0).unwrap_or(u32::MAX))
    }
}

/// The cell path's uniforms as the shader's `CU` reads them: five `uint2`
/// then fifteen `uint`, 100 bytes, `repr(C)`. Every field is a pixel count
/// or a cell count, and says which; the last five are the selection, a
/// flag and its two inclusive corners.
#[repr(C)]
#[derive(Clone, Copy)]
struct CellUniforms {
    viewport_w: Px,
    viewport_h: Px,
    cell_w: Px,
    cell_h: Px,
    cols: Uni,
    rows: Uni,
    mask_side: Px,
    color_side: Px,
    cursor_col: Uni,
    cursor_row: Uni,
    cursor: CursorCode,
    baseline: Px,
    underline: Px,
    strike: Px,
    thickness: Px,
    map_slots: Uni,
    map_shift: Uni,
    stride: Uni,
    rotation: Uni,
    ring: Uni,
    sel_on: Uni,
    sel_from_col: Uni,
    sel_from_row: Uni,
    sel_to_col: Uni,
    sel_to_row: Uni,
}

/// What the cell path draws from: the grid in place, under its rotation
/// and at its stride, modulo its ring, the rendition table resolved, the
/// glyph map, and where the cursor is.
pub struct CellInput<'a> {
    pub grid: &'a GridBuffer,
    pub styles: &'a StylesBuffer,
    pub map: &'a MapBuffer,
    pub stride: Stride,
    pub rotation: RowRot,
    /// The rotation's modulus: the grid buffer's rows, `Term::ring`.
    pub ring: Rows,
    pub cursor: Option<(Row, Col, CursorStyle)>,
    /// The selection on this view, or none: a selected cell's colours swap
    /// before the cursor's own swap, the rule every path applies.
    pub highlight: Option<Highlight>,
    /// One byte a screen cell: set where `records` draw the cell.
    pub overlay: &'a OverlayBuffer,
    /// What draws the cells the overlay masks, in the instance layout.
    pub records: &'a [Instance],
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
#[repr(transparent)]
struct CursorCode(u32);

/// What to do when the GPU finishes a frame.
pub enum Done {
    /// Block until it has: for a bench or a test that reads the target
    /// back. Never in a frame loop.
    Wait,
    /// Run this on the GPU's completion thread, and return at once.
    Notify(Arc<dyn Fn() + Send + Sync>),
}

/// What a draw did.
#[derive(Clone, Copy, Debug)]
pub enum Drawn {
    /// Committed; with [`Done::Wait`], completed.
    Frame(GpuFrame),
    /// A frame is still in flight and reads the memory this one would
    /// bind, so nothing was done. The caller hears about the completion
    /// through its [`Done::Notify`].
    Busy,
}

/// Whether the GPU is reading the shared buffers right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InFlight {
    Idle,
    Busy,
}

/// One atlas sheet on the GPU.
struct Sheet {
    texture: Texture,
    side: Side,
}

/// The background bitmap on the GPU, one texel a cell: the instance path's.
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
    background: Option<Background>,
    mask: Option<Sheet>,
    color: Option<Sheet>,
    /// The atlas generation the sheets were last brought up to.
    uploaded: Option<Generation>,
    /// Frames committed and not yet completed: zero or one.
    in_flight: Arc<AtomicUsize>,
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

/// A 2D texture of `format`, `w` by `h`.
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
fn upload(texture: &ProtocolObject<dyn MTLTexture>, w: Px, h: Px, bytes: &[u8], texel: Texel) {
    let rows = slots(h);
    if rows == 0 || !bytes.len().is_multiple_of(rows) {
        return;
    }
    let row_bytes = RowBytes(bytes.len() / rows);
    let rect = TexRect {
        x: baud_render::Tex(0),
        y: baud_render::Tex(0),
        w: baud_render::Tex(u16::try_from(w.value()).unwrap_or(u16::MAX)),
        h: baud_render::Tex(u16::try_from(h.value()).unwrap_or(u16::MAX)),
    };
    upload_rect(texture, rect, bytes, row_bytes, texel);
}

/// Copies the texels of `rect` from a sheet whose rows are `row_bytes`
/// apart -- `bytes` starts at the sheet's first byte -- into the same
/// rectangle of the texture. The dirty-rectangle upload.
fn upload_rect(
    texture: &ProtocolObject<dyn MTLTexture>,
    rect: TexRect,
    bytes: &[u8],
    row_bytes: RowBytes,
    texel: Texel,
) {
    let (x, y, w, h) = (
        usize::from(rect.x.0),
        usize::from(rect.y.0),
        usize::from(rect.w.0),
        usize::from(rect.h.0),
    );
    let row_bytes = row_bytes.0;
    if w == 0 || h == 0 || row_bytes == 0 {
        return;
    }
    let bpp = match texel {
        Texel::A8 => 1,
        Texel::Bgra8 => 4,
    };
    let start = y
        .saturating_mul(row_bytes)
        .saturating_add(x.saturating_mul(bpp));
    let need = (h - 1)
        .saturating_mul(row_bytes)
        .saturating_add(w.saturating_mul(bpp));
    let Some(src) = bytes.get(start..) else {
        return;
    };
    if src.len() < need {
        return;
    }
    let region = MTLRegion {
        origin: MTLOrigin { x, y, z: 0 },
        size: MTLSize {
            width: w,
            height: h,
            depth: 1,
        },
    };
    // SAFETY: `src` holds at least `(h - 1) * row_bytes + w * bpp` bytes
    // from the rectangle's first texel, checked above, and the region lies
    // inside the texture the rectangle was cut from, so the copy reads
    // inside the slice and writes inside the texture.
    unsafe {
        texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
            region,
            0,
            NonNull::from(src).cast::<c_void>(),
            row_bytes,
        );
    }
}

/// A `repr(C)` slice over the first `n` elements of a shared buffer's
/// contents, for the life of the process: the buffer is retained forever.
///
/// # Safety
/// `T` must be valid for any bit pattern the caller then writes, and the
/// caller must not read the slice through the GPU while writing it here,
/// which [`Gpu::in_flight`] is the gate for.
unsafe fn lend<T>(buffer: &Buffer, n: Elems) -> Option<&'static mut [T]> {
    let n = n.0;
    if buffer.length() < n.saturating_mul(core::mem::size_of::<T>()) {
        return None;
    }
    let keep: Buffer = buffer.clone();
    core::mem::forget(keep);
    let ptr = buffer.contents().cast::<T>();
    // SAFETY: a shared buffer's contents are page-aligned, at least
    // `n * size_of::<T>()` bytes as checked, and now retained for the life
    // of the process; the caller upholds the rest.
    Some(unsafe { core::slice::from_raw_parts_mut(ptr.as_ptr(), n) })
}

// SAFETY: every field is a Metal object -- device, queue, pipeline states,
// sampler, buffers, textures -- which Apple documents as usable from any
// thread with the caller ordering access, plus an `Arc<AtomicUsize>`; the
// bindings mark the device and queue `Send` and leave buffers and textures
// unmarked. A `Gpu` crosses a thread exactly once, whole, when the probe
// that built it is joined, and nothing touches it on two threads at once.
unsafe impl Send for Gpu {}

impl Gpu {
    /// The system default device with every pipeline compiled; `None` when
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
            cell_bg_pipe,
            cell_glyph_pipe,
            cell_shape_pipe,
            sampler,
            instances,
            ring: Ring(0),
            background: None,
            mask: None,
            color: None,
            uploaded: None,
            in_flight: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Whether the device shares memory with the CPU, which is what makes
    /// reading the grid in place a read rather than a transfer (D2's
    /// discrete arm is unbuilt).
    /// One small render pass, committed at launch and not waited for: the
    /// first *render* pass a process runs costs ~5 ms on this box whatever
    /// it draws, an empty command buffer does not prepay it, and this one
    /// runs while the window is being made rather than inside the first
    /// frame.
    pub fn warm(&self) {
        let Some(scratch) = texture(
            &self.device,
            MTLPixelFormat::BGRA8Unorm,
            Px::of(4),
            Px::of(4),
            MTLTextureUsage::RenderTarget,
        ) else {
            return;
        };
        let pass = MTLRenderPassDescriptor::new();
        let att = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        att.setTexture(Some(&scratch));
        att.setLoadAction(MTLLoadAction::Clear);
        att.setStoreAction(MTLStoreAction::Store);
        let Some(cmd) = self.queue.commandBuffer() else {
            return;
        };
        let Some(enc) = cmd.renderCommandEncoderWithDescriptor(&pass) else {
            return;
        };
        enc.setRenderPipelineState(&self.cell_bg_pipe);
        enc.endEncoding();
        cmd.commit();
    }

    pub fn unified_memory(&self) -> Memory {
        if self.device.hasUnifiedMemory() {
            Memory::Unified
        } else {
            Memory::Discrete
        }
    }

    /// Whether a frame is being read by the GPU. While it is, the shared
    /// buffers a draw binds must not be written: keep incoming bytes
    /// queued, and draw when the completion arrives.
    pub fn in_flight(&self) -> InFlight {
        if self.in_flight.load(Ordering::Acquire) == 0 {
            InFlight::Idle
        } else {
            InFlight::Busy
        }
    }

    fn shared(&self, bytes: Span) -> Option<Buffer> {
        self.device
            .newBufferWithLength_options(bytes.0.max(1), MTLResourceOptions::StorageModeShared)
    }

    /// Memory for a surface of `rows` rows at `stride` cells, in the GPU's
    /// own reach: the buffer the cell path binds, and the slice to lend
    /// the core through `Owned::storage_over` -- the primary ring at
    /// `Owned::ring` rows, the alternate screen at `Owned::rows`. Every
    /// cell starts empty.
    pub fn grid(
        &self,
        rows: Rows,
        stride: Stride,
    ) -> Option<(GridBuffer, &'static mut [PackedCell])> {
        let n = rows.index().saturating_mul(stride.index());
        let buffer = self.shared(Span(n.saturating_mul(core::mem::size_of::<PackedCell>())))?;
        // SAFETY: `PackedCell` is `repr(C)` over plain integers, valid for
        // every bit pattern, and the slice is filled before anything reads
        // it; the in-flight gate keeps the GPU's reads out of the core's
        // writes.
        let cells = unsafe { lend::<PackedCell>(&buffer, Elems(n)) }?;
        cells.fill(PackedCell::EMPTY);
        Some((GridBuffer(buffer), cells))
    }

    /// Memory for the rendition table resolved, one entry a slot of
    /// `term`'s table, for `Renderer::resolve_styles_into` to write.
    pub fn styles(&self, term: &Term<'_>) -> Option<(StylesBuffer, &'static mut [Resolved])> {
        let n = term.styles().len();
        let buffer = self.shared(Span(n.saturating_mul(Resolved::BYTES)))?;
        // SAFETY: `Resolved` is `repr(C)` over four `u32` newtypes, valid
        // for every bit pattern, and written before it is read.
        let styles = unsafe { lend::<Resolved>(&buffer, Elems(n)) }?;
        styles.fill(Resolved::default());
        Some((StylesBuffer(buffer), styles))
    }

    /// Memory for a glyph map, for `GlyphMap::over`.
    /// The overlay for a screen of at most `cols` by `rows`.
    pub fn overlay(&self, cols: Cols, rows: Rows) -> Option<(OverlayBuffer, &'static mut [u8])> {
        let n = cols.index().saturating_mul(rows.index());
        let buffer = self.shared(Span(n.max(BUFFER_FLOOR)))?;
        let bytes = unsafe { lend::<u8>(&buffer, Elems(n)) }?;
        bytes.fill(0);
        Some((OverlayBuffer(buffer), bytes))
    }

    pub fn map(&self) -> Option<(MapBuffer, &'static mut [MapEntry])> {
        let buffer = self.shared(Span(MAP_SLOTS.saturating_mul(MapEntry::BYTES)))?;
        // SAFETY: `MapEntry` is `repr(C)` over integer newtypes, valid for
        // every bit pattern; `GlyphMap::over` empties every slot first.
        let entries = unsafe { lend::<MapEntry>(&buffer, Elems(MAP_SLOTS)) }?;
        Some((MapBuffer(buffer), entries))
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

    /// A render target nothing shows, for measuring the draw alone or
    /// reading it back.
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

    /// Brings the two sheet textures up to the atlas: a replaced sheet is
    /// made afresh at its side and taken whole, an unchanged one is left
    /// alone, and one a glyph landed on takes that rectangle and no more.
    fn sheets_for(&mut self, atlas: &mut Atlas) -> Option<()> {
        let generation = atlas.generation();
        let (dirty, mask_replaced, color_replaced) = atlas.take_dirty();
        let mut whole = Dirty::default();
        let side = atlas.side();
        if self.mask.as_ref().is_none_or(|s| s.side != side) || mask_replaced == Replaced::Yes {
            let px = Px::of(side.value());
            let texture = texture(
                &self.device,
                MTLPixelFormat::R8Unorm,
                px,
                px,
                MTLTextureUsage::ShaderRead,
            )?;
            self.mask = Some(Sheet { texture, side });
            whole.mask = Some(full(side));
        }
        let cside = atlas.color_side();
        if self.color.as_ref().is_none_or(|s| s.side != cside) || color_replaced == Replaced::Yes {
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
            whole.color = Some(full(cside));
        }
        if let Some(r) = whole.mask.or(dirty.mask) {
            upload_rect(
                &self.mask.as_ref()?.texture,
                r,
                atlas.pixels(),
                RowBytes(slots(Px::of(side.value()))),
                Texel::A8,
            );
        }
        if let Some(r) = whole.color.or(dirty.color) {
            upload_rect(
                &self.color.as_ref()?.texture,
                r,
                word_bytes(atlas.argb()),
                RowBytes(slots(Px::of(cside.value())).saturating_mul(4)),
                Texel::Bgra8,
            );
        }
        self.uploaded = Some(generation);
        Some(())
    }

    /// A pass over `target`, cleared to transparent.
    fn encoder(&self, target: &Target) -> Option<(CommandBuffer, Encoder)> {
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
        Some((cmd, enc))
    }

    /// Commits `cmd`: waits, or arms `done` and returns. Counts the frame
    /// in flight either way, and out again when it completes.
    fn commit(
        &self,
        cmd: &ProtocolObject<dyn MTLCommandBuffer>,
        done: Done,
    ) -> (Duration, Duration) {
        let t1 = Instant::now();
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        match done {
            Done::Wait => {
                cmd.commit();
                cmd.waitUntilCompleted();
                self.in_flight.fetch_sub(1, Ordering::AcqRel);
                let gpu = Duration::from_secs_f64((cmd.GPUEndTime() - cmd.GPUStartTime()).max(0.0));
                (t1.elapsed(), gpu)
            }
            Done::Notify(then) => {
                let flying = Arc::clone(&self.in_flight);
                let block =
                    RcBlock::new(move |_: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                        flying.fetch_sub(1, Ordering::AcqRel);
                        then();
                    });
                // SAFETY: the block is a valid `MTLCommandBufferHandler`
                // for the length of the call; Metal copies it, so the
                // `RcBlock` may drop after.
                unsafe { cmd.addCompletedHandler(RcBlock::as_ptr(&block)) };
                cmd.commit();
                (t1.elapsed(), Duration::ZERO)
            }
        }
    }

    /// The instance path: writes the renderer's records into a ring slot,
    /// the background bitmap into its texture, brings the sheets up, and
    /// draws. `None` when a resource could not be made; [`Drawn::Busy`]
    /// while a frame is in flight.
    pub fn draw(
        &mut self,
        renderer: &mut Renderer<'_, '_>,
        target: &Target,
        done: Done,
    ) -> Option<Drawn> {
        if self.in_flight() == InFlight::Busy {
            return Some(Drawn::Busy);
        }
        let t0 = Instant::now();
        let (cols, rows) = renderer.grid();
        let metrics = renderer.metrics();
        let count = renderer.instances().count();
        let bytes = count.saturating_mul(Instance::BYTES);
        let slot = self.ring.0;
        if self.instances[slot].length() < bytes {
            let want = bytes.max(BUFFER_FLOOR).next_power_of_two();
            self.instances[slot] = self.shared(Span(want))?;
        }
        {
            let buffer = &self.instances[slot];
            let ptr = buffer.contents().cast::<u8>();
            // SAFETY: a shared buffer's contents are CPU-addressable for
            // its whole length, at least `bytes` after the check above, and
            // nothing is in flight, so the GPU is not reading it.
            let dst = unsafe { core::slice::from_raw_parts_mut(ptr.as_ptr(), bytes) };
            for (chunk, inst) in dst
                .chunks_exact_mut(Instance::BYTES)
                .zip(renderer.instances())
            {
                chunk.copy_from_slice(&inst.to_bytes());
            }
        }
        self.ring = Ring((slot + 1) % RING);
        let bg = self.background_for(cols, rows)?;
        let (bw, bh) = (
            Px::of(u32::try_from(bg.cols.index()).ok()?),
            Px::of(u32::try_from(bg.rows.index()).ok()?),
        );
        upload(
            &bg.texture,
            bw,
            bh,
            argb_bytes(renderer.background()),
            Texel::Bgra8,
        );
        let bg_texture = bg.texture.clone();
        self.sheets_for(renderer.atlas_mut())?;
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
        let (cmd, enc) = self.encoder(target)?;
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
        let (submit, gpu) = self.commit(&cmd, done);
        Some(Drawn::Frame(GpuFrame {
            instances: Tally::of(count),
            upload: upload_time,
            submit,
            gpu,
        }))
    }

    /// The cell path: binds the grid, the styles and the map where they
    /// already are, brings the sheets up by the texels new glyphs wrote,
    /// and draws a background pass, a glyph pass and a shape pass, each one
    /// instanced draw over every cell. Nothing is copied. `None` when a
    /// resource could not be made; [`Drawn::Busy`] while a frame is in
    /// flight.
    pub fn draw_cells(
        &mut self,
        renderer: &mut Renderer<'_, '_>,
        input: &CellInput<'_>,
        target: &Target,
        done: Done,
    ) -> Option<Drawn> {
        if self.in_flight() == InFlight::Busy {
            return Some(Drawn::Busy);
        }
        let t0 = Instant::now();
        let (cols, rows) = renderer.grid();
        let count = cols.index().saturating_mul(rows.index());
        let metrics = renderer.metrics();
        self.sheets_for(renderer.atlas_mut())?;
        let mask = self.mask.as_ref()?.texture.clone();
        let color = self.color.as_ref()?.texture.clone();
        let (tw, th) = target.size();
        let (cursor_row, cursor_col) = input
            .cursor
            .map_or((Uni(u32::MAX), Uni(u32::MAX)), |(r, c, _)| {
                (Uni::of(Elems(r.index())), Uni::of(Elems(c.index())))
            });
        let uniforms = CellUniforms {
            viewport_w: tw,
            viewport_h: th,
            cell_w: metrics.cell_w,
            cell_h: metrics.cell_h,
            cols: Uni::of(Elems(cols.index())),
            rows: Uni::of(Elems(rows.index())),
            mask_side: Px::of(self.mask.as_ref()?.side.value()),
            color_side: Px::of(self.color.as_ref()?.side.value()),
            cursor_col,
            cursor_row,
            cursor: CursorKind::of(input.cursor).code(),
            baseline: metrics.baseline,
            underline: metrics.underline,
            strike: metrics.strike,
            thickness: metrics.thickness,
            map_slots: Uni::of(Elems(MAP_SLOTS)),
            map_shift: Uni(32 - MAP_BITS),
            stride: Uni::of(Elems(input.stride.index())),
            rotation: Uni::of(Elems(input.rotation.index())),
            ring: Uni::of(Elems(input.ring.index())),
            sel_on: Uni(u32::from(input.highlight.is_some())),
            sel_from_col: Uni::of(Elems(input.highlight.map_or(0, |h| h.from.1.index()))),
            sel_from_row: Uni::of(Elems(input.highlight.map_or(0, |h| h.from.0.index()))),
            sel_to_col: Uni::of(Elems(input.highlight.map_or(0, |h| h.to.1.index()))),
            sel_to_row: Uni::of(Elems(input.highlight.map_or(0, |h| h.to.0.index()))),
        };
        let upload_time = t0.elapsed();
        let (cmd, enc) = self.encoder(target)?;
        let ubytes = NonNull::from(&uniforms).cast::<c_void>();
        let ulen = core::mem::size_of::<CellUniforms>();
        // SAFETY: as in `draw` -- `uniforms` outlives the encoder and Metal
        // copies `setBytes` data at the call; the buffer and texture indices
        // match the shaders' attributes; the grid holds `ring * stride`
        // cells and the shader reads `(row + rot) % ring * stride + col`
        // with `col < cols <= stride` and `rows <= ring`; the style buffer holds every slot a
        // cell can name; the map buffer holds every slot the probe reaches.
        unsafe {
            for (pipe, instances) in [
                (&self.cell_bg_pipe, count),
                (&self.cell_glyph_pipe, count),
                (&self.cell_shape_pipe, count.saturating_mul(SHAPES_PER_CELL)),
            ] {
                enc.setRenderPipelineState(pipe);
                enc.setVertexBuffer_offset_atIndex(Some(&input.grid.0), 0, 0);
                enc.setVertexBuffer_offset_atIndex(Some(&input.styles.0), 0, 1);
                enc.setVertexBytes_length_atIndex(ubytes, ulen, 2);
                enc.setVertexBuffer_offset_atIndex(Some(&input.map.0), 0, 3);
                enc.setVertexBuffer_offset_atIndex(Some(&input.overlay.0), 0, 4);
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
            // The records: the cells the overlay masked, drawn as the
            // instance path draws, after every cell pass.
            let n = input.records.len();
            if n > 0 {
                let bytes = n.saturating_mul(Instance::BYTES);
                let slot = self.ring.0;
                if self.instances[slot].length() < bytes {
                    let want = bytes.max(BUFFER_FLOOR).next_power_of_two();
                    self.instances[slot] = self.shared(Span(want))?;
                }
                {
                    let buffer = &self.instances[slot];
                    let ptr = buffer.contents().cast::<u8>();
                    let dst = core::slice::from_raw_parts_mut(ptr.as_ptr(), bytes);
                    for (chunk, inst) in dst.chunks_exact_mut(Instance::BYTES).zip(input.records) {
                        chunk.copy_from_slice(&inst.to_bytes());
                    }
                }
                self.ring = Ring((slot + 1) % RING);
                let side_m = px_f32(Px::of(self.mask.as_ref()?.side.value()));
                let side_c = px_f32(Px::of(self.color.as_ref()?.side.value()));
                let u: [f32; UNIFORM_FLOATS] = [
                    px_f32(tw),
                    px_f32(th),
                    px_f32(metrics.cell_w),
                    px_f32(metrics.cell_h),
                    0.0,
                    0.0,
                    side_m,
                    side_m,
                    side_c,
                    side_c,
                ];
                let ubytes = NonNull::from(&u).cast::<c_void>();
                let ulen = core::mem::size_of::<[f32; UNIFORM_FLOATS]>();
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
                    n,
                );
            }
        }
        enc.endEncoding();
        let (submit, gpu) = self.commit(&cmd, done);
        Some(Drawn::Frame(GpuFrame {
            instances: Tally::of(count.saturating_add(input.records.len())),
            upload: upload_time,
            submit,
            gpu,
        }))
    }

    /// Reads a target back as `0xAARRGGBB` words, premultiplied, row-major
    /// at the target's width; `None` when `out` is too small. Only sound
    /// after a [`Done::Wait`] draw.
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

/// Whether the GPU and CPU share one memory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Memory {
    Unified,
    Discrete,
}

/// The whole of a sheet of `side`.
fn full(side: Side) -> TexRect {
    let s = baud_render::Tex(u16::try_from(side.value()).unwrap_or(u16::MAX));
    TexRect {
        x: baud_render::Tex(0),
        y: baud_render::Tex(0),
        w: s,
        h: s,
    }
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
fn argb_bytes(words: &[baud_render::Argb]) -> &[u8] {
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
