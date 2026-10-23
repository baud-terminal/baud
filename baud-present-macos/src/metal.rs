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

use crate::journal::Landing;
use crate::slots;
use baud_core::Term;
use baud_domain::contrast::CurveRow;
use baud_domain::themefile::CellPaint;
use baud_domain::{
    Col, Cols, CursorStyle, HostTime, ImageGen, ImageId, PackedCell, Row, RowRot, Rows, Stride,
};
use baud_font::Px;
use baud_render::effect::{self, Effect};
use baud_render::{
    Argb, Atlas, CursorLook, Dirty, Generation, Ground, Highlight, ImageComposite, ImageLayer,
    ImagePlacement, Instance, Kind, MAP_BITS, MAP_SLOTS, MapEntry, Region, Renderer, Replaced,
    Resolved, Screen, Side, Tally, Tex, TexRect,
};
use block2::RcBlock;
use core::ffi::c_void;
use core::ptr::NonNull;
use dispatch2::DispatchData;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_io_surface::{IOSurface, IOSurfaceRef};
use objc2_metal::{
    MTLBlendFactor, MTLBuffer, MTLClearColor, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary, MTLLoadAction, MTLOrigin, MTLPixelFormat,
    MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLResourceOptions, MTLSamplerDescriptor,
    MTLSamplerMinMagFilter, MTLSamplerState, MTLScissorRect, MTLSize, MTLStorageMode,
    MTLStoreAction, MTLTexture, MTLTextureDescriptor, MTLTextureUsage, MTLViewport,
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
const UNIFORM_FLOATS: usize = 12;

/// How many shapes a cell may draw in the cell path's shape pass: the
/// underline, the strike, the double underline's second line, the cursor,
/// the overline, the double overline's second line, a frame's bottom and
/// its two sides, the stress mark, an end of an encircled run, and
/// sixteen segments for a curly, dotted or dashed underline. **Must equal `SHAPE_SLOTS` in
/// `shaders.metal`**, which is the divisor the vertex stage turns an
/// instance id into a cell and a slot with; `the_shape_slot_
/// count_matches_the_shader` pins the pair.
pub const SHAPES_PER_CELL: usize = 27;

/// The shaders, as the compiler and the runtime both read them.
/// `scripts/build-metallib.sh` compiles this file to `baud.metallib`;
/// the fallback below compiles this same text at launch when that
/// library will not load. One source, so the two cannot disagree.
const SHADERS: &str = include_str!("shaders.metal");

/// The same shaders compiled to AIR, generated and committed by
/// `scripts/build-metallib.sh`. Loading it skips the source-to-AIR front
/// end, which a cold shader cache prices at 277 of the 287 ms a cold
/// launch costs (note P). AIR is portable and a GPU binary is not, so
/// the pipelines below still build per GPU either way.
const METALLIB: &[u8] = include_bytes!("baud.metallib");

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
/// start-to-end where the frame was waited for -- `None` when it was not,
/// because the answer does not exist yet and arrives with the completion.
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuFrame {
    pub instances: Tally,
    pub upload: Duration,
    pub submit: Duration,
    /// `None` on the asynchronous path: the frame has not finished, so
    /// there is no GPU time to report and a zero would be a lie. It
    /// reaches the caller through [`Done::Notify`] instead.
    pub gpu: Option<Duration>,
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

/// A record's place in its buffer: the one the trail's quad is, or where the
/// records end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct At(usize);

/// A trail's own pipeline -- the quad's and the sweep's, or an effect's:
/// built the first frame one is drawn, so a launch pays nothing for a trail
/// it may never show.
enum LatePipe {
    Unbuilt,
    Built(Retained<ProtocolObject<dyn MTLRenderPipelineState>>),
    Failed,
}

impl LatePipe {
    /// The pipeline, built now if it never was: `glyph_vertex` over the
    /// records and `fragment` over them.
    fn get(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        library: &ProtocolObject<dyn MTLLibrary>,
        fragment: &str,
    ) -> Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
        self.get_as(device, library, ("glyph_vertex", fragment), Blend::Over)
    }

    /// [`LatePipe::get`], over another vertex function and blend.
    fn get_as(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        library: &ProtocolObject<dyn MTLLibrary>,
        (vertex, fragment): (&str, &str),
        blend: Blend,
    ) -> Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
        if let LatePipe::Unbuilt = self {
            *self = match pipeline(device, library, vertex, fragment, blend) {
                Some(p) => LatePipe::Built(p),
                None => {
                    eprintln!("baud-present-macos: the {fragment} pipeline would not build");
                    LatePipe::Failed
                }
            };
        }
        match self {
            LatePipe::Built(p) => Some(p.clone()),
            _ => None,
        }
    }
}

/// The uniform a trail's fragment function reads beyond the records:
/// `quad_fragment`'s corners, hole, frame origin and shape, or
/// an effect's ignition, which its fragment function reads.
enum TrailUniform {
    Quad([f32; 16]),
    Effect([f32; effect::UNIFORM_FLOATS]),
}

impl TrailUniform {
    fn bytes(&self) -> (NonNull<c_void>, usize) {
        match self {
            TrailUniform::Quad(u) => (NonNull::from(u).cast(), core::mem::size_of_val(u)),
            TrailUniform::Effect(u) => (NonNull::from(u).cast(), core::mem::size_of_val(u)),
        }
    }
}

/// What a trail's draw needs beyond the glyph pipeline's bindings: its
/// pipeline and its uniform.
struct TrailDraw {
    pipe: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    uniform: TrailUniform,
}

/// The records a trail draws with its own pipeline, a contiguous run of
/// them: the quad's one, or an effect's bands and particles. `None` before one
/// is seen; `at` extends it by record `i`.
fn widen(at: Option<(At, At)>, i: usize) -> Option<(At, At)> {
    Some(match at {
        None => (At(i), At(i + 1)),
        Some((a, _)) => (a, At(i + 1)),
    })
}

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

/// The cell path's uniforms as the shader's `CU` reads them, 38 words, 152
/// bytes, `repr(C)`. Each field says what it counts; `sel_on` to
/// `sel_to_row` are the selection, a flag and its two inclusive corners,
/// and `sel_bg` to `sel_fg_curve` its colours.
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
    /// Smooth scroll: the pixels the screen is shifted down, and the number
    /// of extra rows drawn above the window (0 or 1). Both zero settles the
    /// draw to the plain one.
    pan: Px,
    slide_rows: Uni,
    /// The cursor colour -- a program's `OSC 12`, else the configured one,
    /// else a theme's -- packed `0xFFRRGGBB`, or [`PAINT_PAGE`] for the
    /// cell's page, or 0 for none, the cell's text ([`paint_word`]). Read
    /// after the block-cursor swap and by the bar or underline shape pass.
    cursor_color: Uni,
    /// The blink shapes this frame hides, `BlinkPhases::hidden`:
    /// `Shapes::BLINK` in `SGR 5`'s off phase and `Shapes::RAPID_BLINK` in
    /// `SGR 6`'s, the frame's. The shader hides blinking text itself, since
    /// a resolved slot is the same in every phase.
    blink_hidden: Uni,
    /// How much of a blinking cursor this frame draws, 255 all of it:
    /// `Renderer::cursor_opacity`. The thirtieth word: `CU` is 8-aligned by
    /// its `uint2`s, so its size is a multiple of eight and the bytes bound
    /// must be too -- 29 words were 116 bytes against the shader's 120,
    /// which Metal's validation layer refused, and this word was a pad.
    cursor_alpha: Uni,
    /// The selection's colours, `OSC 17`'s page and `OSC 19`'s text, packed
    /// `0xFFRRGGBB`, or 0 for none -- reverse video. Opaque always
    /// ([`baud_render::Hilite::of`]), so a set colour is never 0, black
    /// included. With each, the contrast curve row a glyph in it takes. Four
    /// words, so the struct stays a multiple of eight bytes.
    sel_bg: Uni,
    sel_fg: Uni,
    sel_bg_curve: Uni,
    sel_fg_curve: Uni,
    /// The text under a block cursor: packed `0xFFRRGGBB`, or
    /// [`PAINT_TEXT`] for the cell's own text, or 0 for none, the cell's
    /// page; and the curve row a glyph in a colour takes. Two words, so the
    /// struct stays a multiple of eight bytes.
    cursor_text: Uni,
    cursor_text_curve: Uni,
    /// The blink shapes this frame draws faint rather than hides,
    /// `BlinkPhases::faint`: each rate's off phase while the system asks
    /// for dimmed flashing lights. With a pad, so the struct stays a
    /// multiple of eight bytes.
    blink_faint: Uni,
    faint_pad: Uni,
}

const _: () = assert!(core::mem::size_of::<CellUniforms>() == 152);

/// The shader's word for the cell's own text, and for its page: never an
/// opaque colour, whose alpha byte is `0xFF`.
const PAINT_TEXT: u32 = 1;
const PAINT_PAGE: u32 = 2;

/// A paint as the shader's word: `none` for the paint the shader takes when
/// the word is 0, else the cell's own by name or the colour, opaque.
fn paint_word<T: Copy>(
    p: Option<CellPaint<T>>,
    none: Option<CellPaint<()>>,
    colour: impl Fn(T) -> Argb,
) -> Uni {
    let unit = p.map(|p| p.map(|_| ()));
    Uni(match p {
        _ if unit == none => 0,
        None => 0,
        Some(CellPaint::Text) => PAINT_TEXT,
        Some(CellPaint::Page) => PAINT_PAGE,
        Some(CellPaint::Colour(c)) => colour(c).0,
    })
}

// **The bytes bound are the shader's struct**: `CU` rounds up to its 8-byte
// alignment, so a word count that is odd binds four bytes short of it.
const _: () = assert!(
    core::mem::size_of::<CellUniforms>().is_multiple_of(8),
    "CellUniforms must fill CU's 8-byte alignment"
);

/// What the cell path draws from: the grid in place, under its rotation
/// and at its stride, modulo its ring, the rendition table resolved, the
/// glyph map, and where the cursor is.
pub struct CellInput<'a> {
    /// Which of the caller's per-frame buffer sets `styles` and `overlay`
    /// come from, so the commit can mark it held until the frame lands.
    ///
    /// The synchronous paths ignore it: they wait inline, so the buffers
    /// are free again before they return.
    pub set: crate::journal::SetIx,
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
    /// What lies over the backgrounds and the images under the text and
    /// under the glyphs -- the cursor guide (`Renderer::cursor_guide`) --
    /// drawn after those images and before the glyph pass, as the instance
    /// path draws them first.
    pub grounds: &'a [Instance],
    /// How many of `records`, from the first, lie inside their own rows
    /// ([`baud_render::Overlay::under`]): drawn after the glyph pass and
    /// before the decoration pass, so each row's glyphs are under its
    /// decorations, as the instance path draws a row. The rest are drawn
    /// after the decoration pass.
    pub under: Tally,
    /// How many of `records`, at the end, are baud's own overlays -- the
    /// find field and its caret, the link preview's bar, the scrollbar's
    /// thumb and the screen fade ([`baud_render::Renderer::chrome`]):
    /// drawn after the images over the text, the rest before them.
    pub chrome: Tally,
    /// Smooth scroll: `Some(pan)` shifts the screen down by `pan` pixels and
    /// draws one extra "leaving" row above the window -- ring row -1, the
    /// line just retired, which the shader reaches by the same modulo.
    /// `None` is the settled draw, unchanged.
    pub slide: Option<Px>,
}

/// How the cursor is drawn, as the shader switches on it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CursorKind {
    None,
    Block,
    Underline,
    Bar,
    /// A hollow box round the cell whatever the shape
    /// ([`baud_render::CursorLook::Hollow`]): drawn by `cell_ring_vertex`,
    /// with no swap.
    Hollow,
}

impl CursorKind {
    fn of(cursor: Option<(Row, Col, CursorStyle)>, look: CursorLook) -> CursorKind {
        match cursor.map(|(_, _, s)| s) {
            None => CursorKind::None,
            Some(_) if look == CursorLook::Hollow => CursorKind::Hollow,
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
            CursorKind::Hollow => 4,
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
    /// Run this on the GPU's completion thread, and return at once. It is
    /// handed the frame's [`Landing`] -- its own GPU time and when it ended
    /// on the host clock -- because that is when both numbers exist: Metal
    /// stamps `GPUStartTime` and `GPUEndTime` on the command buffer and
    /// they are valid once it has completed. Nothing times anything -- the
    /// fact arrives with the event that produced it.
    Notify(Arc<dyn Fn(Landing) + Send + Sync>),
}

/// A completed frame's landing, from the stamps Metal puts on the command
/// buffer: its own start-to-end, and its end as a host time. Valid only
/// once it has completed, which is why the asynchronous path reads it in
/// the handler rather than at commit.
///
/// Both stamps are `CFTimeInterval` seconds of "the host time"
/// (`MTLCommandBuffer.h`), so the end converts to a [`HostTime`] as it is;
/// a value the conversion refuses lands as [`HostTime::ZERO`], which is the
/// header's own "has not completed" reading rather than a time.
fn landing(cmd: &ProtocolObject<dyn MTLCommandBuffer>) -> Landing {
    let (start, end) = (cmd.GPUStartTime(), cmd.GPUEndTime());
    Landing {
        gpu: Duration::try_from_secs_f64((end - start).max(0.0)).unwrap_or(Duration::ZERO),
        ended: HostTime::of_secs(end).unwrap_or(HostTime::ZERO),
    }
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
    /// The library the pipelines came from, kept for the one built late.
    library: Retained<ProtocolObject<dyn MTLLibrary>>,
    quad_pipe: LatePipe,
    effect_pipes: [LatePipe; Effect::COUNT],
    /// `glyph_vertex` and `image_fragment`: an image placement read
    /// through its tables. Built the first time an image is drawn.
    image_pipe: LatePipe,
    /// `glyph_vertex` and `image_replace_fragment`, replacing: a placement
    /// whose Sixel fill draws as [`baud_render::SixelBg::Cleared`] on the
    /// glass. Built the first time one is drawn.
    image_replace_pipe: LatePipe,
    /// `bg_vertex` and `bg_ground_fragment`, replacing: the instance path's
    /// background pass on a frame with a kitty image below `INT32_MIN/2`,
    /// the non-default backgrounds cleared. Built the first time one is
    /// drawn.
    bg_ground_pipe: LatePipe,
    /// `bg_vertex` and `bg_set_fragment`, blending: the instance path's
    /// non-default backgrounds again, over that image. Built with it.
    bg_set_pipe: LatePipe,
    /// `cell_bg_vertex` and `flat_fragment`, blending: the cell path's
    /// non-default backgrounds again over that image. Built with it.
    cell_bg_over_pipe: LatePipe,
    /// `cell_ring_vertex` and `glyph_fragment`, blending: the cell path's
    /// hollow cursor, after the decorations. Built the first time one is
    /// drawn.
    cell_ring_pipe: LatePipe,
    sampler: Retained<ProtocolObject<dyn MTLSamplerState>>,
    /// The text-contrast curves, `baud_domain::contrast::CURVES`, copied
    /// once: `glyph_fragment` reads a mask's coverage through the row its
    /// glyph carries.
    curves: Buffer,
    instances: Vec<Buffer>,
    ring: Ring,
    background: Option<Background>,
    /// Each cell's [`Ground`], 255 where it is `Set`, for `bg_set_fragment`:
    /// made and written only on a frame with an image below the
    /// backgrounds.
    ground: Option<Background>,
    /// The bytes it is written from, kept so a frame allocates nothing.
    ground_bytes: Vec<u8>,
    grounds: Vec<Ground>,
    mask: Option<Sheet>,
    color: Option<Sheet>,
    /// The atlas generation the sheets were last brought up to.
    uploaded: Option<Generation>,
    /// Per-image textures, valid for `image_gen`. Each image uploads once to
    /// its own texture -- images are not glyph-sized and do not belong in the
    /// shared atlas -- and the cache is cleared whole when the store's epoch
    /// changes, so an image slot reused after a `reset` cannot return the old
    /// texture. Keyed by [`ImageId`] within an epoch, and by the word a
    /// Sixel's background fill was drawn as, so a change of the default
    /// background uploads a filled image again.
    images: Vec<((ImageId, u32), Texture)>,
    image_gen: ImageGen,
    /// Frames committed and not yet completed: zero or one.
    in_flight: Arc<AtomicUsize>,
    /// What `Gpu::new` cost, step by step.
    build: Build,
}

/// Whether a pass blends over what is under it, or replaces it. The
/// background passes replace, so a cell's background is a single composite
/// matching the margin's clear; glyphs and shapes blend over.
enum Blend {
    Over,
    Replace,
}

fn pipeline(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    vertex: &str,
    fragment: &str,
    blend: Blend,
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
    // The background passes replace: a cell's background is written once, the
    // same single composite the margin's clear is, so the two match over the
    // glass. Glyphs and shapes blend over that with premultiplied OVER.
    att.setBlendingEnabled(matches!(blend, Blend::Over));
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

/// A shared buffer holding the text-contrast curves, copied once from the
/// compiled table.
fn curve_buffer(device: &ProtocolObject<dyn MTLDevice>) -> Option<Buffer> {
    let table: &[u8] = &baud_domain::contrast::CURVES;
    let buffer =
        device.newBufferWithLength_options(table.len(), MTLResourceOptions::StorageModeShared)?;
    let ptr = buffer.contents().cast::<u8>();
    // SAFETY: a shared buffer's contents are CPU-addressable for its whole
    // length, which is the table's, and no command has been encoded with it.
    let dst = unsafe { core::slice::from_raw_parts_mut(ptr.as_ptr(), table.len()) };
    dst.copy_from_slice(table);
    Some(buffer)
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

/// What creating the device, compiling the shader library and building the
/// five pipelines cost, in that order -- the three steps whose price the
/// Metal shader cache decides. `newLibraryWithSource` and each pipeline
/// state are served from the system's cache on a warm launch and compiled
/// on a cold one (the first launch after an install or an OS update), and
/// row 1 of note N is the question of what that costs and whether a binary
/// archive the app keeps would change it. Printed under `BAUD_STARTUP`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Build {
    pub device: Duration,
    pub library: Duration,
    pub pipelines: Duration,
    /// What each of the five cost on its own, in the order they are built:
    /// bg, glyph, cell_bg, cell_glyph, cell_shape.
    ///
    /// **Because `pipelines` alone cannot answer row 1's leftover.** Shipping
    /// the `.metallib` took a cold launch's pipeline build from 45.1 ms to
    /// **73.6**, reproduced across every settled launch, and the record
    /// carries that +28.5 as unexplained with one named next step: time the
    /// five separately under both libraries. A cost spread across all five in
    /// proportion to their shaders is work `newLibraryWithSource` was doing
    /// eagerly and `newLibraryWithData` leaves here; a cost in one is
    /// something about that shader.
    pub each: [Duration; 5],
    /// Which of the two the library came from. A measurement that cannot
    /// say this cannot tell a shipped library from a cold compile that
    /// happened to be fast, and the whole of row 1 is the difference.
    pub library_from: Source,
}

/// Where the shader library came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    /// `newLibraryWithData` over the committed `baud.metallib`.
    #[default]
    Library,
    /// `newLibraryWithSource` over [`SHADERS`], because the library would
    /// not load -- a Metal too old for the toolchain that built it, or a
    /// file that is not a library at all.
    Compiled,
}

impl Gpu {
    /// The system default device with every pipeline compiled; `None` when
    /// there is no Metal device or the shaders fail to compile.
    pub fn new() -> Option<Gpu> {
        let t0 = Instant::now();
        let device = MTLCreateSystemDefaultDevice()?;
        let queue = device.newCommandQueue()?;
        let device_made = t0.elapsed();
        let t1 = Instant::now();
        // **The committed library first, the source as the fallback.** A
        // `.metallib` is AIR built by one toolchain, and a Metal runtime
        // older than that toolchain refuses it -- so this cannot be the
        // only route, and a refusal has to cost a compile rather than a
        // launch. Both arms end in the same `MTLLibrary`.
        let data = DispatchData::from_static_bytes(METALLIB);
        let (library, library_from) = match device.newLibraryWithData_error(&data) {
            Ok(l) => (l, Source::Library),
            Err(e) => {
                eprintln!(
                    "baud-present-macos: baud.metallib will not load ({e:?}); compiling the source"
                );
                match device.newLibraryWithSource_options_error(&NSString::from_str(SHADERS), None)
                {
                    Ok(l) => (l, Source::Compiled),
                    Err(e) => {
                        eprintln!("baud-present-macos: shaders: {e:?}");
                        return None;
                    }
                }
            }
        };
        let library_made = t1.elapsed();
        let t2 = Instant::now();
        // Timed one at a time; see `Build::each`.
        let mut each = [Duration::ZERO; 5];
        let p0 = Instant::now();
        let bg_pipe = pipeline(
            &device,
            &library,
            "bg_vertex",
            "bg_fragment",
            Blend::Replace,
        )?;
        each[0] = p0.elapsed();
        let p1 = Instant::now();
        let glyph_pipe = pipeline(
            &device,
            &library,
            "glyph_vertex",
            "glyph_fragment",
            Blend::Over,
        )?;
        each[1] = p1.elapsed();
        let p2 = Instant::now();
        let cell_bg_pipe = pipeline(
            &device,
            &library,
            "cell_bg_vertex",
            "flat_fragment",
            Blend::Replace,
        )?;
        each[2] = p2.elapsed();
        let p3 = Instant::now();
        let cell_glyph_pipe = pipeline(
            &device,
            &library,
            "cell_glyph_vertex",
            "glyph_fragment",
            Blend::Over,
        )?;
        each[3] = p3.elapsed();
        let p4 = Instant::now();
        let cell_shape_pipe = pipeline(
            &device,
            &library,
            "cell_shape_vertex",
            "glyph_fragment",
            Blend::Over,
        )?;
        each[4] = p4.elapsed();
        let build = Build {
            device: device_made,
            library: library_made,
            pipelines: t2.elapsed(),
            each,
            library_from,
        };
        let sd = MTLSamplerDescriptor::new();
        sd.setMinFilter(MTLSamplerMinMagFilter::Nearest);
        sd.setMagFilter(MTLSamplerMinMagFilter::Nearest);
        let sampler = device.newSamplerStateWithDescriptor(&sd)?;
        let curves = curve_buffer(&device)?;
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
            library,
            quad_pipe: LatePipe::Unbuilt,
            effect_pipes: core::array::from_fn(|_| LatePipe::Unbuilt),
            image_pipe: LatePipe::Unbuilt,
            image_replace_pipe: LatePipe::Unbuilt,
            cell_ring_pipe: LatePipe::Unbuilt,
            bg_ground_pipe: LatePipe::Unbuilt,
            bg_set_pipe: LatePipe::Unbuilt,
            cell_bg_over_pipe: LatePipe::Unbuilt,
            sampler,
            curves,
            instances,
            ring: Ring(0),
            background: None,
            ground: None,
            ground_bytes: Vec::new(),
            grounds: Vec::new(),
            mask: None,
            color: None,
            uploaded: None,
            images: Vec::new(),
            image_gen: ImageGen::ZERO,
            in_flight: Arc::new(AtomicUsize::new(0)),
            build,
        })
    }

    /// What building this device cost, for the launch report: the three
    /// steps the shader cache decides the price of.
    pub fn build(&self) -> Build {
        self.build
    }

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

    /// Whether the device shares memory with the CPU, which is what makes
    /// reading the grid in place a read rather than a transfer (D2's
    /// discrete arm is unbuilt).
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
        // SAFETY: `Resolved` is `repr(C)` over nine `u32` newtypes, valid
        // for every bit pattern, and written before it is read.
        let styles = unsafe { lend::<Resolved>(&buffer, Elems(n)) }?;
        styles.fill(Resolved::default());
        Some((StylesBuffer(buffer), styles))
    }

    /// The overlay for a screen of at most `cols` by `rows`, and one row
    /// more: a smooth scroll's leaving row, which the shader reads after
    /// the screen's (`baud_render::Overlay::leaving`).
    pub fn overlay(&self, cols: Cols, rows: Rows) -> Option<(OverlayBuffer, &'static mut [u8])> {
        let n = cols.index().saturating_mul(rows.index().saturating_add(1));
        let buffer = self.shared(Span(n.max(BUFFER_FLOOR)))?;
        let bytes = unsafe { lend::<u8>(&buffer, Elems(n)) }?;
        bytes.fill(0);
        Some((OverlayBuffer(buffer), bytes))
    }

    /// Memory for a glyph map, for `GlyphMap::over`.
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

    /// Writes every cell's [`Ground`] into the ground texture, made at the
    /// grid's size when it is not: what `bg_set_fragment` reads. `None`
    /// when no texture could be made.
    fn ground_for(&mut self, renderer: &Renderer<'_, '_>) -> Option<Texture> {
        let (cols, rows) = renderer.grid();
        let stale = self
            .ground
            .as_ref()
            .is_none_or(|b| b.cols.index() != cols.index() || b.rows.index() != rows.index());
        let (w, h) = (
            Px::of(u32::try_from(cols.index()).ok()?),
            Px::of(u32::try_from(rows.index()).ok()?),
        );
        if stale {
            let texture = texture(
                &self.device,
                MTLPixelFormat::R8Unorm,
                w,
                h,
                MTLTextureUsage::ShaderRead,
            )?;
            self.ground = Some(Background {
                texture,
                cols,
                rows,
            });
        }
        renderer.grounds(&mut self.grounds);
        self.ground_bytes.clear();
        self.ground_bytes
            .extend(self.grounds.iter().map(|g| match g {
                Ground::Default => 0u8,
                Ground::Set => 0xFF,
            }));
        let ground = self.ground.as_ref()?;
        upload(&ground.texture, w, h, &self.ground_bytes, Texel::A8);
        Some(ground.texture.clone())
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

    /// Ensures every visible image has a GPU texture -- uploading a new one
    /// once, its own texture rather than the shared atlas, because an image is
    /// not glyph-sized -- and returns the placements to draw, in the
    /// renderer's order, each with the texture to bind. The cache is dropped
    /// whole when the store's epoch moves, so a slot reused after a `reset`
    /// cannot return a stale texture. Called before the encoder, so a
    /// `replaceRegion` upload never touches a bound texture.
    fn resolve_images(&mut self, renderer: &Renderer<'_, '_>) -> Vec<ImageDraw> {
        let places = renderer.image_placements();
        let Some(epoch) = places.first().map(|p| p.epoch) else {
            return Vec::new();
        };
        if self.image_gen != epoch {
            self.images.clear();
            self.image_gen = epoch;
        }
        let mut draws = Vec::with_capacity(places.len());
        for p in places {
            let key = (p.id, p.ground.bits());
            if !self.images.iter().any(|(k, _)| *k == key) {
                let Some(up) = renderer
                    .image_uploads()
                    .iter()
                    .find(|u| u.id == p.id && u.epoch == epoch && u.ground == p.ground)
                else {
                    continue;
                };
                let (w, h) = (Px::of(u32::from(up.w)), Px::of(u32::from(up.h)));
                let Some(tex) = texture(
                    &self.device,
                    MTLPixelFormat::BGRA8Unorm,
                    w,
                    h,
                    MTLTextureUsage::ShaderRead,
                ) else {
                    continue;
                };
                upload(&tex, w, h, word_bytes(&up.pixels), Texel::Bgra8);
                // One texture an image: the one drawn in an older fill
                // colour goes, so the cache keeps no texture per colour.
                self.images.retain(|((id, _), _)| *id != p.id);
                self.images.push((key, tex));
            }
            if let Some((_, tex)) = self.images.iter().find(|(k, _)| *k == key) {
                let (xs, ys) = renderer.image_map(p);
                let mut map = Vec::with_capacity(xs.len() + ys.len());
                map.extend_from_slice(xs);
                map.extend_from_slice(ys);
                draws.push(ImageDraw {
                    inst: image_instance(p),
                    tex: tex.clone(),
                    layer: p.layer,
                    map,
                    composite: p.composite,
                });
            }
        }
        draws
    }

    /// **Writes every image draw's record and tables** into the next ring
    /// buffer: the records first, then each draw's tables, the texel x of
    /// its columns and the texel y of its rows, at a 16-byte boundary each.
    /// `pan` shifts every record down by that many pixels -- the
    /// smooth-scroll offset the grid and records already take, so an image
    /// anchored to a sliding line moves with it -- and is zero on the
    /// instance path and on a settled cell frame. One buffer for both
    /// layers, so the draws under the text and over it take one ring slot.
    fn stage_images(&mut self, draws: &[ImageDraw], pan: Px) -> Option<Staged> {
        if draws.is_empty() {
            return None;
        }
        let pan16 = i16::try_from(pan.value()).unwrap_or(i16::MAX);
        let records = draws.len().saturating_mul(Instance::BYTES);
        let align = |n: usize| n.div_ceil(16).saturating_mul(16);
        let mut maps = Vec::with_capacity(draws.len());
        let mut end = align(records);
        for d in draws {
            let xs = end;
            let ys = xs.saturating_add(usize::from(d.inst.w.0).saturating_mul(4));
            maps.push((xs, ys));
            end = align(xs.saturating_add(d.map.len().saturating_mul(4)));
        }
        let slot = self.ring.0;
        if self.instances[slot].length() < end {
            let want = end.max(BUFFER_FLOOR).next_power_of_two();
            self.instances[slot] = self.shared(Span(want))?;
        }
        {
            let buffer = &self.instances[slot];
            let ptr = buffer.contents().cast::<u8>();
            // SAFETY: a shared buffer's contents are CPU-addressable for its
            // whole length, at least `end` after the check above, and nothing
            // is in flight, so the GPU is not reading it.
            let dst = unsafe { core::slice::from_raw_parts_mut(ptr.as_ptr(), end) };
            for (chunk, d) in dst[..records].chunks_exact_mut(Instance::BYTES).zip(draws) {
                let mut inst = d.inst;
                inst.y = Screen(inst.y.0.saturating_add(pan16));
                chunk.copy_from_slice(&inst.to_bytes());
            }
            for (d, &(xs, _)) in draws.iter().zip(&maps) {
                for (out, v) in dst[xs..xs + d.map.len() * 4]
                    .chunks_exact_mut(4)
                    .zip(&d.map)
                {
                    out.copy_from_slice(&v.to_le_bytes());
                }
            }
        }
        self.ring = Ring((slot + 1) % RING);
        Some(Staged {
            slot: Ring(slot),
            maps,
        })
    }

    /// Draws the staged images of `layer`, in order, each a quad from its
    /// own texture through `image_fragment`, which reads each drawn pixel's
    /// texel by its tables -- the same texel the CPU's blit takes -- and
    /// premultiplies it once, as the colour branch of `glyph_fragment` does,
    /// for the same OVER the CPU `blit_image` does. One draw call per image:
    /// its texture at fragment 1, its tables at fragment buffers 0 and 1,
    /// and a uniform whose `color` floats are one, so the quad's uv is the
    /// drawn pixel's position in it.
    ///
    /// Called inside an open encoder; every pass after it binds what it
    /// reads, so the buffers this binds are not left for a glyph to read.
    fn draw_images(
        &mut self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        draws: &[ImageDraw],
        staged: Option<&Staged>,
        layer: ImageLayer,
        frame_px: (Px, Px),
    ) -> Option<()> {
        let Some(staged) = staged else {
            return Some(());
        };
        if !draws.iter().any(|d| d.layer == layer) {
            return Some(());
        }
        let pipe = self
            .image_pipe
            .get(&self.device, &self.library, "image_fragment")?;
        // A replacing placement's pipeline, built only on a frame with one.
        let replace = match draws
            .iter()
            .any(|d| d.layer == layer && d.composite == ImageComposite::Replace)
        {
            true => Some(self.image_replace_pipe.get_as(
                &self.device,
                &self.library,
                ("glyph_vertex", "image_replace_fragment"),
                Blend::Replace,
            )?),
            false => None,
        };
        let buffer = &self.instances[staged.slot.0];
        // Only `viewport` (0,1) and `color` (8,9) are read by `glyph_vertex`;
        // a `color` of one makes the uv the drawn pixel's position.
        let u: [f32; UNIFORM_FLOATS] = [
            px_f32(frame_px.0),
            px_f32(frame_px.1),
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            1.0,
            0.0,
            0.0,
        ];
        let ubytes = NonNull::from(&u).cast::<c_void>();
        let ulen = core::mem::size_of::<[f32; UNIFORM_FLOATS]>();
        // SAFETY: `u` outlives its calls (Metal copies `setVertexBytes` at
        // the call); the buffer holds every draw's record and tables, at the
        // offsets `stage_images` wrote them; the indices match
        // `glyph_vertex` and `image_fragment`, which reads no table entry
        // past the quad's own width and height.
        unsafe {
            for (i, (d, &(xs, ys))) in draws.iter().zip(&staged.maps).enumerate() {
                if d.layer != layer {
                    continue;
                }
                match (d.composite, &replace) {
                    (ImageComposite::Replace, Some(r)) => enc.setRenderPipelineState(r),
                    _ => enc.setRenderPipelineState(&pipe),
                }
                enc.setVertexBuffer_offset_atIndex(
                    Some(buffer),
                    i.saturating_mul(Instance::BYTES),
                    0,
                );
                enc.setVertexBytes_length_atIndex(ubytes, ulen, 1);
                enc.setFragmentTexture_atIndex(Some(&d.tex), 1);
                enc.setFragmentBuffer_offset_atIndex(Some(buffer), xs, 0);
                enc.setFragmentBuffer_offset_atIndex(Some(buffer), ys, 1);
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                    MTLPrimitiveType::TriangleStrip,
                    0,
                    4,
                    1,
                );
            }
        }
        Some(())
    }

    /// The trail's draw for this frame, if the renderer has one and its
    /// pipeline could be built: `glyph_vertex` over the records, and
    /// `quad_fragment` over a quad's or a sweep's corners, or an effect's own
    /// fragment function over its ignition. Each pipeline is built once, the
    /// first time.
    fn trail_draw(&mut self, renderer: &Renderer<'_, '_>, pad: [f32; 2]) -> Option<TrailDraw> {
        if let Some(smear) = renderer.smear() {
            let pipe = self
                .quad_pipe
                .get(&self.device, &self.library, "quad_fragment")?;
            return Some(TrailDraw {
                pipe,
                uniform: TrailUniform::Quad(smear.uniform(pad)),
            });
        }
        let vfx = renderer.vfx()?;
        // The effect's place in `Effect::ALL`, which the array follows.
        let at = Effect::ALL.iter().position(|&e| e == vfx.effect)?;
        let pipe = self.effect_pipes[at].get(&self.device, &self.library, vfx.effect.fragment())?;
        Some(TrailDraw {
            pipe,
            uniform: TrailUniform::Effect(vfx.uniform(pad)),
        })
    }

    /// **The exit-status marks** (records `from..to`), in a pass of their
    /// own over the strip at the whole frame's left: the strip's viewport
    /// and scissor, and uniforms that make its pixels the records' space,
    /// x from the strip's left and y as the grid's rows. The surface's
    /// clear is the strip's ground, as the CPU path fills it.
    ///
    /// # Safety
    ///
    /// The records lie inside `buffer`, as [`Gpu::draw_records`] asks.
    unsafe fn draw_strip(
        &self,
        enc: &Encoder,
        buffer: &Buffer,
        (from, to): (At, At),
        (target, vp, height): (&Target, MTLViewport, Px),
        metrics: baud_render::Metrics,
        (mask, color): (&Texture, &Texture),
    ) -> Option<()> {
        if to.0 <= from.0 || vp.width <= 0.0 {
            return Some(());
        }
        let width = Px::of(u32::try_from(vp.width as i64).ok()?);
        let side_m = px_f32(Px::of(self.mask.as_ref()?.side.value()));
        let side_c = px_f32(Px::of(self.color.as_ref()?.side.value()));
        let origin = [vp.originX as f32, vp.originY as f32];
        let u: [f32; UNIFORM_FLOATS] = [
            px_f32(width),
            px_f32(height),
            px_f32(metrics.cell_w),
            px_f32(metrics.cell_h),
            0.0,
            0.0,
            side_m,
            side_m,
            side_c,
            side_c,
            origin[0],
            origin[1],
        ];
        enc.setViewport(vp);
        enc.setScissorRect(scissor(target, origin, (width, height)));
        // SAFETY: the caller's.
        unsafe { self.draw_records(enc, buffer, from, to, None, None, &u, mask, color) };
        Some(())
    }

    /// **Draws the records `from..end` of `buffer`**: the glyph pipeline for
    /// each run of them, and the trail's pipeline for its records `trail`
    /// -- a quad's one, an effect's bands and particles -- so the trail stays where
    /// the renderer put it in the order: over the cells and their records,
    /// under the scrollbar and the fade. Without a trail draw its records
    /// are left out, not drawn as solids over their bounds.
    ///
    /// # Safety
    ///
    /// `buffer` holds at least `end` records, and the bindings match
    /// `glyph_vertex`, `glyph_fragment` and the trail's fragment function.
    #[allow(clippy::too_many_arguments)]
    unsafe fn draw_records(
        &self,
        enc: &Encoder,
        buffer: &Buffer,
        from: At,
        end: At,
        trail: Option<(At, At)>,
        draw: Option<&TrailDraw>,
        u: &[f32; UNIFORM_FLOATS],
        mask: &Texture,
        color: &Texture,
    ) {
        let ubytes = NonNull::from(u).cast::<c_void>();
        let ulen = core::mem::size_of::<[f32; UNIFORM_FLOATS]>();
        let run = |from: usize, to: usize| {
            if to <= from {
                return;
            }
            // SAFETY: the caller's -- `from..to` lies inside the buffer's
            // `end` records, and the indices match the glyph shaders.
            unsafe {
                enc.setRenderPipelineState(&self.glyph_pipe);
                enc.setVertexBuffer_offset_atIndex(
                    Some(buffer),
                    from.saturating_mul(Instance::BYTES),
                    0,
                );
                enc.setVertexBytes_length_atIndex(ubytes, ulen, 1);
                enc.setFragmentTexture_atIndex(Some(mask), 0);
                enc.setFragmentTexture_atIndex(Some(color), 1);
                enc.setFragmentSamplerState_atIndex(Some(&self.sampler), 0);
                enc.setFragmentBuffer_offset_atIndex(Some(&self.curves), 0, 0);
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                    MTLPrimitiveType::TriangleStrip,
                    0,
                    4,
                    to - from,
                );
            }
        };
        let (At(f), At(n)) = (from, end);
        match (trail, draw) {
            (Some((At(a), At(b))), None) if a < n && a >= f => {
                run(f, a);
                run(b.min(n), n);
            }
            (Some((At(a), At(b))), Some(draw)) if a < n && a >= f => {
                let b = b.min(n);
                run(f, a);
                let (tbytes, tlen) = draw.uniform.bytes();
                // SAFETY: records `a..b` are inside the buffer, and the
                // trail's pipeline reads the vertex bindings the glyph one
                // does plus its fragment function's `buffer(0)`.
                unsafe {
                    enc.setRenderPipelineState(&draw.pipe);
                    enc.setVertexBuffer_offset_atIndex(
                        Some(buffer),
                        a.saturating_mul(Instance::BYTES),
                        0,
                    );
                    enc.setVertexBytes_length_atIndex(ubytes, ulen, 1);
                    enc.setFragmentBytes_length_atIndex(tbytes, tlen, 0);
                    enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                        MTLPrimitiveType::TriangleStrip,
                        0,
                        4,
                        b - a,
                    );
                }
                run(b, n);
            }
            _ => run(f, n),
        }
    }

    /// A pass over `target`, cleared to `clear` -- the terminal's own
    /// background, so the margin around the grid is the same colour as a
    /// default cell rather than a see-through edge -- with the rasteriser
    /// confined to `viewport`, the grid centred in the surface.
    fn encoder(
        &self,
        target: &Target,
        clear: MTLClearColor,
        viewport: MTLViewport,
    ) -> Option<(CommandBuffer, Encoder)> {
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: attachment 0 exists on every pass descriptor.
        let att = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        att.setTexture(Some(&target.texture));
        att.setLoadAction(MTLLoadAction::Clear);
        att.setStoreAction(MTLStoreAction::Store);
        att.setClearColor(clear);
        let cmd = self.queue.commandBuffer()?;
        let enc = cmd.renderCommandEncoderWithDescriptor(&pass)?;
        enc.setViewport(viewport);
        Some((cmd, enc))
    }

    /// Commits `cmd`: waits, or arms `done` and returns. Counts the frame
    /// in flight either way, and out again when it completes.
    fn commit(
        &self,
        cmd: &ProtocolObject<dyn MTLCommandBuffer>,
        done: Done,
    ) -> (Duration, Option<Duration>) {
        let t1 = Instant::now();
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        match done {
            Done::Wait => {
                cmd.commit();
                cmd.waitUntilCompleted();
                self.in_flight.fetch_sub(1, Ordering::AcqRel);
                (t1.elapsed(), Some(landing(cmd).gpu))
            }
            Done::Notify(then) => {
                let flying = Arc::clone(&self.in_flight);
                let block =
                    RcBlock::new(move |done: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                        // SAFETY: Metal hands the handler the command
                        // buffer that completed, valid for the call.
                        let landed = landing(unsafe { done.as_ref() });
                        flying.fetch_sub(1, Ordering::AcqRel);
                        then(landed);
                    });
                // SAFETY: the block is a valid `MTLCommandBufferHandler`
                // for the length of the call; Metal copies it, so the
                // `RcBlock` may drop after.
                unsafe { cmd.addCompletedHandler(RcBlock::as_ptr(&block)) };
                cmd.commit();
                (t1.elapsed(), None)
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
        let grid_px = renderer.pixel_size();
        let frame_px = renderer.frame_size();
        let clear = clear_of(renderer.clear_color());
        let count = renderer.instances().count();
        // The instances before baud's own overlays, which go over the
        // images over the text.
        let chrome = usize::try_from(renderer.chrome().value()).map_or(count, |c| c.min(count));
        let content = count - chrome;
        // The exit-status marks after them, drawn in the strip's own pass.
        let marks = renderer.strip_records().len();
        let bytes = (count + marks).saturating_mul(Instance::BYTES);
        let mut trail_at = None;
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
            for (i, (chunk, inst)) in dst
                .chunks_exact_mut(Instance::BYTES)
                .zip(renderer.instances().chain(renderer.strip_records()))
                .enumerate()
            {
                if matches!(inst.kind, Kind::Quad | Kind::Band | Kind::Particle) {
                    trail_at = widen(trail_at, i);
                }
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
        // Resolve images before the encoder, so a new one's `replaceRegion`
        // upload never touches a bound texture. The instance path does not
        // slide, so no pan.
        let images = self.resolve_images(renderer);
        let staged = self.stage_images(&images, Px::of(0));
        // An image below the backgrounds: the cells' grounds, the pipeline
        // that draws the background pass with the non-default backgrounds
        // cleared, and the one that blends them over the image.
        let below = match images.iter().any(|d| d.layer == ImageLayer::Below) {
            true => Some((
                self.ground_for(renderer)?,
                self.bg_ground_pipe.get_as(
                    &self.device,
                    &self.library,
                    ("bg_vertex", "bg_ground_fragment"),
                    Blend::Replace,
                )?,
                self.bg_set_pipe.get_as(
                    &self.device,
                    &self.library,
                    ("bg_vertex", "bg_set_fragment"),
                    Blend::Over,
                )?,
            )),
            false => None,
        };
        let strip = renderer.strip();
        let (viewport, pad, strip_vp) = letterbox_whole(target, renderer.whole_size(), strip);
        let trail = self.trail_draw(renderer, pad);
        let uniforms: [f32; UNIFORM_FLOATS] = [
            px_f32(frame_px.0),
            px_f32(frame_px.1),
            px_f32(metrics.cell_w),
            px_f32(metrics.cell_h),
            px_f32(bw),
            px_f32(bh),
            px_f32(Px::of(self.mask.as_ref()?.side.value())),
            px_f32(Px::of(self.mask.as_ref()?.side.value())),
            px_f32(Px::of(self.color.as_ref()?.side.value())),
            px_f32(Px::of(self.color.as_ref()?.side.value())),
            pad[0],
            pad[1],
        ];
        let upload_time = t0.elapsed();
        let (cmd, enc) = self.encoder(target, clear, viewport)?;
        let ubytes = NonNull::from(&uniforms).cast::<c_void>();
        let ulen = core::mem::size_of::<[f32; UNIFORM_FLOATS]>();
        match &below {
            Some((ground, pipe, _)) => {
                enc.setRenderPipelineState(pipe);
                // SAFETY: index 1 is `bg_ground_fragment`'s `[[texture(1)]]`.
                unsafe { enc.setFragmentTexture_atIndex(Some(ground), 1) };
            }
            None => enc.setRenderPipelineState(&self.bg_pipe),
        }
        // SAFETY: the byte pointers name `uniforms`, which outlives the
        // encoder (Metal copies `setBytes` data at the call); the buffer
        // and texture indices match the shader's `[[buffer(n)]]` and
        // `[[texture(n)]]` attributes; the instance buffer holds `count`
        // records, so an instance id never reads past it.
        unsafe {
            enc.setFragmentBytes_length_atIndex(ubytes, ulen, 0);
            enc.setFragmentTexture_atIndex(Some(&bg_texture), 0);
            // The background pass replaces what is under it, and it writes
            // nothing a cell does not cover -- so it is held to the grid,
            // and a legacy scroller's gutter keeps the pass's clear colour,
            // the letterbox margin's, rather than the transparent zero the
            // fragment returns past the last column.
            enc.setScissorRect(scissor(target, pad, grid_px));
            enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::TriangleStrip, 0, 4);
            enc.setScissorRect(scissor(target, [0.0, 0.0], target.size()));
            // Images below the non-default backgrounds, then those
            // backgrounds blended over them, as the CPU's `paint_under`.
            if let Some((ground, _, pipe)) = &below {
                self.draw_images(&enc, &images, staged.as_ref(), ImageLayer::Below, frame_px)?;
                enc.setRenderPipelineState(pipe);
                enc.setFragmentBytes_length_atIndex(ubytes, ulen, 0);
                enc.setFragmentTexture_atIndex(Some(&bg_texture), 0);
                enc.setFragmentTexture_atIndex(Some(ground), 1);
                enc.setScissorRect(scissor(target, pad, grid_px));
                enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::TriangleStrip, 0, 4);
                enc.setScissorRect(scissor(target, [0.0, 0.0], target.size()));
            }
            // Images under the text, over the backgrounds, as the CPU
            // compositor blits them into a repainted row.
            self.draw_images(&enc, &images, staged.as_ref(), ImageLayer::Under, frame_px)?;
            if content > 0 {
                self.draw_records(
                    &enc,
                    &self.instances[slot],
                    At(0),
                    At(content),
                    trail_at,
                    trail.as_ref(),
                    &uniforms,
                    &mask,
                    &color,
                );
            }
            // Images over the text after the content, the way the CPU
            // compositor blits them, and baud's own overlays over them.
            self.draw_images(&enc, &images, staged.as_ref(), ImageLayer::Over, frame_px)?;
            if count > content {
                self.draw_records(
                    &enc,
                    &self.instances[slot],
                    At(content),
                    At(count),
                    trail_at,
                    trail.as_ref(),
                    &uniforms,
                    &mask,
                    &color,
                );
            }
            self.draw_strip(
                &enc,
                &self.instances[slot],
                (At(count), At(count + marks)),
                (target, strip_vp, frame_px.1),
                metrics,
                (&mask, &color),
            )?;
        }
        enc.endEncoding();
        let (submit, gpu) = self.commit(&cmd, done);
        Some(Drawn::Frame(GpuFrame {
            instances: Tally::of(count + marks),
            upload: upload_time,
            submit,
            gpu,
        }))
    }

    /// The cell path: binds the grid, the styles and the map where they
    /// already are, brings the sheets up by the texels new glyphs wrote,
    /// and draws a background pass, a glyph pass, a shape pass and a pass
    /// of the glyphs' parts above their rows, each one instanced draw over
    /// every cell, with the records between the glyphs and the shapes. Nothing is copied. `None` when a
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
        // Smooth scroll draws one extra row above the window; `count` is the
        // per-cell draw count over those `draw_rows`.
        let slide_rows = usize::from(input.slide.is_some());
        let draw_rows = rows.index().saturating_add(slide_rows);
        let count = cols.index().saturating_mul(draw_rows);
        let metrics = renderer.metrics();
        let frame_px = renderer.frame_size();
        let clear = clear_of(renderer.clear_color());
        self.sheets_for(renderer.atlas_mut())?;
        let mask = self.mask.as_ref()?.texture.clone();
        let color = self.color.as_ref()?.texture.clone();
        // Resolve images before the encoder, as the instance path does. They
        // slide with the grid under smooth scroll.
        let images = self.resolve_images(renderer);
        let staged = self.stage_images(&images, input.slide.unwrap_or(Px::of(0)));
        // An image below the backgrounds: the pipeline that blends the
        // non-default backgrounds over it.
        let below = match images.iter().any(|d| d.layer == ImageLayer::Below) {
            true => Some(self.cell_bg_over_pipe.get_as(
                &self.device,
                &self.library,
                ("cell_bg_vertex", "flat_fragment"),
                Blend::Over,
            )?),
            false => None,
        };
        let strip = renderer.strip();
        let (viewport, pad, strip_vp) = letterbox_whole(target, renderer.whole_size(), strip);
        let trail = self.trail_draw(renderer, pad);
        let kind = CursorKind::of(input.cursor, renderer.cursor_look());
        // The hollow box's pipeline, built the first frame one is drawn.
        let ring = match kind {
            CursorKind::Hollow => Some(self.cell_ring_pipe.get_as(
                &self.device,
                &self.library,
                ("cell_ring_vertex", "glyph_fragment"),
                Blend::Over,
            )?),
            _ => None,
        };
        let (cursor_row, cursor_col) = input
            .cursor
            .map_or((Uni(u32::MAX), Uni(u32::MAX)), |(r, c, _)| {
                (Uni::of(Elems(r.index())), Uni::of(Elems(c.index())))
            });
        let hilite = renderer.hilite();
        let block = renderer.block_paint();
        let curve_of = |p: Option<CellPaint<(Argb, CurveRow)>>| match p {
            Some(CellPaint::Colour((_, row))) => u32::from(row.get()),
            _ => 0,
        };
        let uniforms = CellUniforms {
            viewport_w: frame_px.0,
            viewport_h: frame_px.1,
            cell_w: metrics.cell_w,
            cell_h: metrics.cell_h,
            cols: Uni::of(Elems(cols.index())),
            rows: Uni::of(Elems(rows.index())),
            mask_side: Px::of(self.mask.as_ref()?.side.value()),
            color_side: Px::of(self.color.as_ref()?.side.value()),
            cursor_col,
            cursor_row,
            cursor: kind.code(),
            baseline: metrics.baseline,
            underline: metrics.underline,
            strike: metrics.strike,
            thickness: metrics.thickness,
            map_slots: Uni::of(Elems(MAP_SLOTS)),
            map_shift: Uni(32 - MAP_BITS),
            stride: Uni::of(Elems(input.stride.index())),
            rotation: Uni::of(Elems(input.rotation.index())),
            ring: Uni::of(Elems(input.ring.index())),
            // 0 none, 1 a stream, 2 a block: the shader's `selected()`.
            sel_on: Uni(match input.highlight.map(|h| h.region) {
                None => 0,
                Some(Region::Stream) => 1,
                Some(Region::Block) => 2,
            }),
            sel_from_col: Uni::of(Elems(input.highlight.map_or(0, |h| h.from.1.index()))),
            sel_from_row: Uni::of(Elems(input.highlight.map_or(0, |h| h.from.0.index()))),
            sel_to_col: Uni::of(Elems(input.highlight.map_or(0, |h| h.to.1.index()))),
            sel_to_row: Uni::of(Elems(input.highlight.map_or(0, |h| h.to.0.index()))),
            pan: input.slide.unwrap_or(Px::of(0)),
            slide_rows: Uni::of(Elems(slide_rows)),
            // What the block is drawn in, as `Renderer::convert_row` reads
            // it: the fill none where it is the cell's text, the text none
            // where it is the cell's page.
            cursor_color: paint_word(block.fill, Some(CellPaint::Text), |c| c),
            cursor_text: paint_word(block.text, Some(CellPaint::Page), |(c, _)| c),
            cursor_text_curve: Uni(match block.text {
                Some(CellPaint::Colour((_, row))) => u32::from(row.get()),
                _ => 0,
            }),
            cursor_alpha: Uni(u32::from(
                input
                    .cursor
                    .map_or(0xFF, |(_, _, style)| renderer.cursor_opacity(style).get()),
            )),
            blink_hidden: Uni(renderer.blink().hidden().0),
            blink_faint: Uni(renderer.blink().faint().0),
            faint_pad: Uni(0),
            sel_bg: paint_word(hilite.bg, None, |(c, _)| c),
            sel_fg: paint_word(hilite.fg, None, |(c, _)| c),
            sel_bg_curve: Uni(curve_of(hilite.bg)),
            sel_fg_curve: Uni(curve_of(hilite.fg)),
        };
        let upload_time = t0.elapsed();
        let (cmd, enc) = self.encoder(target, clear, viewport)?;
        let ubytes = NonNull::from(&uniforms).cast::<c_void>();
        let ulen = core::mem::size_of::<CellUniforms>();
        // SAFETY: as in `draw` -- `uniforms` outlives the encoder and Metal
        // copies `setBytes` data at the call; the buffer and texture indices
        // match the shaders' attributes; the grid holds `ring * stride`
        // cells and the shader reads `(row + rot) % ring * stride + col`
        // with `col < cols <= stride` and `rows <= ring`; the style buffer holds every slot a
        // cell can name; the map buffer holds every slot the probe reaches.
        unsafe {
            // The records, written once and drawn in two places: those
            // inside their rows after the glyph pass, the rest after the
            // decoration pass.
            // The grounds first in the buffer, then the records.
            let g = input.grounds.len();
            let n = g + input.records.len();
            let under = g + usize::try_from(input.under.value()).map_or(n - g, |u| u.min(n - g));
            // The exit-status marks after the records, drawn in the strip's
            // own pass.
            let marks = renderer.strip_records().len();
            let mut trail_at = None;
            let mut records = None;
            if n + marks > 0 {
                let bytes = (n + marks).saturating_mul(Instance::BYTES);
                let slot = self.ring.0;
                if self.instances[slot].length() < bytes {
                    let want = bytes.max(BUFFER_FLOOR).next_power_of_two();
                    self.instances[slot] = self.shared(Span(want))?;
                }
                {
                    let buffer = &self.instances[slot];
                    let ptr = buffer.contents().cast::<u8>();
                    let dst = core::slice::from_raw_parts_mut(ptr.as_ptr(), bytes);
                    for (i, (chunk, inst)) in dst
                        .chunks_exact_mut(Instance::BYTES)
                        .zip(
                            input
                                .grounds
                                .iter()
                                .chain(input.records)
                                .chain(renderer.strip_records()),
                        )
                        .enumerate()
                    {
                        if matches!(inst.kind, Kind::Quad | Kind::Band | Kind::Particle) {
                            trail_at = widen(trail_at, i);
                        }
                        chunk.copy_from_slice(&inst.to_bytes());
                    }
                }
                self.ring = Ring((slot + 1) % RING);
                records = Some(slot);
            }
            let side_m = px_f32(Px::of(self.mask.as_ref()?.side.value()));
            let side_c = px_f32(Px::of(self.color.as_ref()?.side.value()));
            let ru: [f32; UNIFORM_FLOATS] = [
                px_f32(frame_px.0),
                px_f32(frame_px.1),
                px_f32(metrics.cell_w),
                px_f32(metrics.cell_h),
                0.0,
                0.0,
                side_m,
                side_m,
                side_c,
                side_c,
                pad[0],
                pad[1],
            ];
            // The cell passes: the backgrounds, the glyphs inside their
            // rows, the decorations, and then the parts of glyphs above
            // their rows -- each row's glyphs and then its decorations,
            // rows top to bottom, as the instance path draws them, so a
            // glyph reaching up from the row below lies over the
            // decorations of the row it reaches. `cell_glyph_vertex`'s
            // `part`: `PART_INSIDE` and `PART_ABOVE`.
            let inside: u32 = 0;
            let above: u32 = 1;
            let passes = [
                (self.cell_bg_pipe.clone(), count),
                (self.cell_glyph_pipe.clone(), count),
                (
                    self.cell_shape_pipe.clone(),
                    count.saturating_mul(SHAPES_PER_CELL),
                ),
                (self.cell_glyph_pipe.clone(), count),
            ];
            // `cell_bg_vertex`'s `ground_pass`: `GROUND_EVERY`,
            // `GROUND_ONLY_SET` and `GROUND_CLEAR_SET`.
            let every: u32 = 0;
            let only_set: u32 = 1;
            let clear_set: u32 = 2;
            let word = |w: &u32| NonNull::from(w).cast::<c_void>();
            let wlen = core::mem::size_of::<u32>();
            for (k, (pipe, instances)) in passes.iter().enumerate() {
                // Images under the text after the backgrounds, before the
                // glyphs: the CPU compositor's order in a repainted row.
                // Those below the non-default backgrounds first, over a
                // background pass that cleared those cells, and the
                // background pass again over them for those cells alone,
                // blended.
                if k == 1 {
                    if let Some(over) = &below {
                        self.draw_images(
                            &enc,
                            &images,
                            staged.as_ref(),
                            ImageLayer::Below,
                            frame_px,
                        )?;
                        // `draw_images` bound its own buffers at 0 and 1.
                        enc.setRenderPipelineState(over);
                        enc.setVertexBuffer_offset_atIndex(Some(&input.grid.0), 0, 0);
                        enc.setVertexBuffer_offset_atIndex(Some(&input.styles.0), 0, 1);
                        enc.setVertexBytes_length_atIndex(ubytes, ulen, 2);
                        enc.setVertexBuffer_offset_atIndex(Some(&input.overlay.0), 0, 4);
                        enc.setVertexBytes_length_atIndex(word(&only_set), wlen, 5);
                        enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                            MTLPrimitiveType::TriangleStrip,
                            0,
                            4,
                            count,
                        );
                    }
                    self.draw_images(&enc, &images, staged.as_ref(), ImageLayer::Under, frame_px)?;
                    // The grounds over them, under the glyphs.
                    if let Some(slot) = records {
                        self.draw_records(
                            &enc,
                            &self.instances[slot],
                            At(0),
                            At(g),
                            trail_at,
                            trail.as_ref(),
                            &ru,
                            &mask,
                            &color,
                        );
                    }
                }
                // The records inside their rows, between the glyphs and the
                // decorations.
                if k == 2 {
                    if let Some(slot) = records {
                        self.draw_records(
                            &enc,
                            &self.instances[slot],
                            At(g),
                            At(under),
                            trail_at,
                            trail.as_ref(),
                            &ru,
                            &mask,
                            &color,
                        );
                    }
                }
                let instances = *instances;
                enc.setRenderPipelineState(pipe);
                enc.setVertexBuffer_offset_atIndex(Some(&input.grid.0), 0, 0);
                enc.setVertexBuffer_offset_atIndex(Some(&input.styles.0), 0, 1);
                enc.setVertexBytes_length_atIndex(ubytes, ulen, 2);
                enc.setVertexBuffer_offset_atIndex(Some(&input.map.0), 0, 3);
                enc.setVertexBuffer_offset_atIndex(Some(&input.overlay.0), 0, 4);
                let pass = match (k, &below) {
                    (0, Some(_)) => &clear_set,
                    (1, _) => &inside,
                    (3, _) => &above,
                    _ => &every,
                };
                enc.setVertexBytes_length_atIndex(word(pass), wlen, 5);
                enc.setFragmentTexture_atIndex(Some(&mask), 0);
                enc.setFragmentTexture_atIndex(Some(&color), 1);
                enc.setFragmentSamplerState_atIndex(Some(&self.sampler), 0);
                enc.setFragmentBuffer_offset_atIndex(Some(&self.curves), 0, 0);
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                    MTLPrimitiveType::TriangleStrip,
                    0,
                    4,
                    instances,
                );
                // The hollow cursor over the decorations, on the bindings
                // the decoration pass left: its four sides.
                if let (2, Some(ring)) = (k, &ring) {
                    enc.setRenderPipelineState(ring);
                    enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                        MTLPrimitiveType::TriangleStrip,
                        0,
                        4,
                        4,
                    );
                }
            }
            // The rest of the records after every cell pass: the parts of
            // glyphs above their rows, then what the caller appended -- a
            // prediction, a composition, the trail -- drawn as the instance
            // path draws them; then the images over the text, and baud's
            // own overlays over them, the thumb and the fade last.
            let chrome = usize::try_from(input.chrome.value()).map_or(n, |c| c.min(n));
            let content = (n - chrome).max(under);
            if let Some(slot) = records {
                self.draw_records(
                    &enc,
                    &self.instances[slot],
                    At(under),
                    At(content),
                    trail_at,
                    trail.as_ref(),
                    &ru,
                    &mask,
                    &color,
                );
            }
            self.draw_images(&enc, &images, staged.as_ref(), ImageLayer::Over, frame_px)?;
            if let Some(slot) = records {
                self.draw_records(
                    &enc,
                    &self.instances[slot],
                    At(content),
                    At(n),
                    trail_at,
                    trail.as_ref(),
                    &ru,
                    &mask,
                    &color,
                );
                self.draw_strip(
                    &enc,
                    &self.instances[slot],
                    (At(n), At(n + marks)),
                    (target, strip_vp, frame_px.1),
                    metrics,
                    (&mask, &color),
                )?;
            }
        }
        enc.endEncoding();
        let (submit, gpu) = self.commit(&cmd, done);
        Some(Drawn::Frame(GpuFrame {
            instances: Tally::of(count.saturating_add(
                input.grounds.len() + input.records.len() + renderer.strip_records().len(),
            )),
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

/// One image placement as the GPU draws it: its record, its texture, its
/// layer, and its tables, the texel x of each visible drawn column then the
/// texel y of each visible drawn row ([`Renderer::image_map`]).
struct ImageDraw {
    inst: Instance,
    tex: Texture,
    layer: ImageLayer,
    map: Vec<u32>,
    /// Blended over what is under it, or written in its place
    /// ([`ImageComposite`]).
    composite: ImageComposite,
}

/// The ring buffer [`Gpu::stage_images`] wrote, and each draw's tables in
/// it: the byte offsets of its columns' and its rows'.
struct Staged {
    slot: Ring,
    maps: Vec<(usize, usize)>,
}

/// The `Kind::Color` instance for an image placement: the screen rect and
/// size of its visible part, `uv` zero, so with `draw_images`'s uniform the
/// quad's uv is the drawn pixel's position. `color` is unused -- the
/// fragment reads the bound texture.
fn image_instance(p: &ImagePlacement) -> Instance {
    Instance {
        x: Screen(p.x),
        y: Screen(p.y),
        w: Tex(p.w),
        h: Tex(p.h),
        u: Tex(0),
        v: Tex(0),
        color: Argb(0),
        kind: Kind::Color,
        curve: baud_domain::contrast::CurveRow::default(),
    }
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

/// The rectangle of the target from `pad` that is `size` big, clamped to
/// the target, as a scissor. The pad is the letterbox's, which may fall on
/// half a pixel; the first pixel whose centre it covers is its floor.
fn scissor(target: &Target, pad: [f32; 2], size: (Px, Px)) -> MTLScissorRect {
    let (tw, th) = target.size();
    let at = |p: f32| usize::try_from(p.max(0.0).floor() as i64).unwrap_or(0);
    let wide = |v: Px| usize::try_from(v.value()).unwrap_or(usize::MAX);
    let (x, y) = (at(pad[0]).min(wide(tw)), at(pad[1]).min(wide(th)));
    MTLScissorRect {
        x,
        y,
        width: wide(size.0).min(wide(tw) - x),
        height: wide(size.1).min(wide(th) - y),
    }
}

/// The frame centred in the surface: the rasteriser viewport at the pad
/// offset, and that same pad as two floats for the instance background
/// fragment, which indexes cells by the surface pixel it lands on. The
/// frame is the grid and a legacy scroller's gutter beside it.
fn letterbox(target: &Target, grid: (Px, Px)) -> (MTLViewport, [f32; 2]) {
    let (tw, th) = target.size();
    let px = |v: u32| f64::from(v);
    let pad_x = (px(tw.value()) - px(grid.0.value())).max(0.0) / 2.0;
    let pad_y = (px(th.value()) - px(grid.1.value())).max(0.0) / 2.0;
    (
        MTLViewport {
            originX: pad_x,
            originY: pad_y,
            width: px(grid.0.value()),
            height: px(grid.1.value()),
            znear: 0.0,
            zfar: 1.0,
        },
        [pad_x as f32, pad_y as f32],
    )
}

/// **The whole frame centred**: [`letterbox`] of the exit-status strip,
/// the grid and the gutter, then the grid's viewport and pad `strip`
/// pixels in from the whole's left, and the strip's own viewport at that
/// left ([`baud_render::Renderer::whole_size`]). With no strip, the
/// grid's are [`letterbox`]'s.
fn letterbox_whole(
    target: &Target,
    whole: (Px, Px),
    strip: Px,
) -> (MTLViewport, [f32; 2], MTLViewport) {
    let (vp, pad) = letterbox(target, whole);
    let s = f64::from(strip.value());
    let grid = MTLViewport {
        originX: vp.originX + s,
        width: (vp.width - s).max(0.0),
        ..vp
    };
    let own = MTLViewport { width: s, ..vp };
    (grid, [grid.originX as f32, pad[1]], own)
}

/// A straight colour as the render pass's premultiplied clear.
fn clear_of(c: Argb) -> MTLClearColor {
    let [a, r, g, b] = c.0.to_be_bytes();
    let a = f64::from(a) / 255.0;
    MTLClearColor {
        red: f64::from(r) / 255.0 * a,
        green: f64::from(g) / 255.0 * a,
        blue: f64::from(b) / 255.0 * a,
        alpha: a,
    }
}

/// `word_bytes`, for the compositor's colour words.
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
