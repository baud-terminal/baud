//! Metal: the frame the renderer converts, or the raw grid the terminal
//! holds, drawn by the GPU into the IOSurface the layer shows -- without
//! copying what already sits in memory the GPU can read, and without
//! waiting for it.
//!
//! It is built for a machine with one memory. Two paths share the shaders' second
//! half and differ in what the CPU does before the draw:
//!
//! * **Instances.** `Renderer::convert` resolves every dirty row into
//!   twenty-byte records on the CPU; `Gpu::draw` writes them into a
//!   `.shared` ring slot -- the write is the upload, D2's one copy -- and
//!   draws one background pass and one instanced glyph pass.
//! * **Cells.** Nothing is converted. The grid is a shared buffer
//!   ([`Gpu::grid`]) -- in the app, a copy of the window in the set the
//!   frame is built in (research decision D4, [`crate::screens`]); a bench
//!   or a test may bind the core's own ring, which the shader reads under
//!   its rotation with one modulo -- the rendition table is resolved
//!   straight into another ([`Gpu::styles`]), the glyph map is kept in a
//!   third ([`Gpu::map`]), and a vertex shader resolves every cell itself.
//!   `Gpu::draw_cells` uploads nothing; it binds.
//!
//! **Nothing waits.** A draw is committed with a completion handler, and
//! the caller's [`Done::Notify`] closure runs on the GPU's thread when the
//! frame is finished -- the presenter uses it to hand the surface to the
//! main thread and wake the responder. One frame is in flight at a time,
//! and a draw asked for meanwhile answers [`Drawn::Busy`] rather than
//! blocking. The buffers a frame reads are its own set's, which nothing
//! writes until it lands ([`crate::journal::Wheel`]), so the core goes on
//! being fed while the GPU reads. A caller that binds the core's own grid
//! instead must not feed while [`Gpu::in_flight`] says busy: the core
//! rewrites the rows of the live window in place.
//!
//! Two things are copied, the atlas and images. The atlas copies only
//! the texels a new glyph wrote: the renderer's sheets are CPU memory the
//! rasteriser writes, and a texture takes them by rectangle
//! ([`baud_render::Atlas::take_dirty`]). After the first screenful that is
//! nothing at all. An image is copied twice: the renderer snapshots it out
//! of the store with its fill words resolved to the ground
//! ([`baud_render::Renderer::image_uploads`]), and the snapshot is copied
//! into a texture of its own ([`baud_render::ImageTextures`]), once for
//! each change to that image's pixels, frame or ground.
//!
//! The shaders are Metal Shading Language compiled at first use, because a
//! `.metallib` needs Xcode's toolchain at build time and the source is
//! two hundred lines.
//!
//! **Metal's shader cache can fault, and where each build meets it.** A
//! pipeline compiled at run time goes through Metal's compiler file cache,
//! one directory per bundle id. When a cache data file a process has open
//! shrinks under it and the cache cannot reopen its files,
//! `newRenderPipelineStateWithDescriptor` reads a `FILE` through a null
//! pointer and the process dies of `SIGSEGV`; `tests/shader_cache_fault.rs`
//! makes it on demand in a cache of its own.
//!
//! * **Tests and benches** never go through that cache: their pipelines
//!   come from the suite's binary archive, a miss refused (`suite-archive`,
//!   `metal/suite.rs`). Every test binary is unbundled and shares one cache
//!   directory with every unbundled process on the machine, and a test
//!   suite under load is where the fault was met; which process shrank the
//!   files there is inferred, not found. The archive is written once for
//!   each OS build, GPU, library and set of descriptors, by the one process
//!   that holds its lock, and that write is the suite's one pass through
//!   the cache.
//! * **The app** compiles through the cache as it always has, in the
//!   directory of its own bundle id, which only processes with that id
//!   share. A second process with that id is started by `open -n` ("Open
//!   a new instance of the application(s) even if one is already
//!   running", `[[source: macos-man-open-1]]`), or by the bundle's binary
//!   run while baud is open; `baud -e` refuses on macOS rather than start one
//!   (`main.rs`). The fault then needs that process to shrink the cache's
//!   files under the other at that moment, which has been shown only with
//!   a process truncating them on purpose. The app never reads the suite's
//!   archive and never refuses a miss: on a GPU an archive was not made for,
//!   a refused pipeline would draw nothing (`tests/suite_archive.rs` holds
//!   this).
//!
//! **This is the one module in the crate that needs `unsafe`.** Every
//! `objc2-metal` call that takes a pointer or an index the framework does
//! not bounds-check is `unsafe fn`, the shared-memory slices are made from
//! buffer contents, and each site carries the reason its contract holds.
//! The crate denies `unsafe_code` and this module allows it, so the
//! boundary is one attribute wide.
#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use crate::screens::ScreenGen;
use crate::slots;
use baud_core::Term;
use baud_domain::contrast::CurveRow;
use baud_domain::themefile::CellPaint;
use baud_domain::{Col, Cols, CursorStyle, PackedCell, Row, RowRot, Rows, Stride};
use baud_font::Px;
use baud_render::effect::{self, Effect};
use baud_render::{
    Argb, Atlas, BackgroundDirty, CursorLook, Decorations, Dirty, Generation, Ground, Highlight,
    ImageComposite, ImageLayer, ImagePlacement, ImageTextures, Instance, Kind, MAP_BITS, MAP_SLOTS,
    MapEntry, MapGeneration, Mix, Reach, Redraw, Region, Renderer, Replaced, Resolved, RowSet,
    Screen, Side, StyleStamp, Tally, Tex, TexRect,
};
use core::ffi::c_void;
use core::ptr::NonNull;
use dispatch2::DispatchData;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_io_surface::{IOSurface, IOSurfaceRef};
use objc2_metal::{
    MTLBlendFactor, MTLBlitCommandEncoder, MTLBuffer, MTLClearColor, MTLCommandEncoder,
    MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary, MTLLoadAction, MTLOrigin,
    MTLPixelFormat, MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLResourceOptions, MTLSamplerDescriptor,
    MTLSamplerMinMagFilter, MTLSamplerState, MTLScissorRect, MTLSize, MTLStorageMode,
    MTLStoreAction, MTLTexture, MTLTextureDescriptor, MTLTextureUsage, MTLViewport,
};
use std::sync::Arc;
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

/// The cursor's slot of a cell's shapes, its underline or its bar: what a
/// frame whose table carries no decoration draws of the shape pass, one
/// instance. **Must equal `CURSOR_SLOT` in `shaders.metal`**, pinned with
/// the slot count.
pub const CURSOR_SHAPE_SLOT: usize = 3;

/// The shaders, as the compiler and the runtime both read them:
/// `shaders.metal` with the stress mark's table spliced over its
/// `#include`, written by the build (`build/stress.rs`).
/// `scripts/build-metallib.sh` compiles `shaders.metal` with the include
/// beside it to `baud.metallib`; the fallback below compiles this text at
/// launch when that library will not load, since a source compiled at
/// run time has no include path. One text, so the two cannot disagree.
const SHADERS: &str = include_str!(concat!(env!("OUT_DIR"), "/shaders.metal"));

/// The same shaders compiled to AIR, generated and committed by
/// `scripts/build-metallib.sh`. Loading it skips the source-to-AIR front
/// end, which a cold shader cache prices at 277 of the 287 ms a cold
/// launch costs (note P). AIR is portable and a GPU binary is not, so
/// the pipelines below still build per GPU either way.
const METALLIB: &[u8] = include_bytes!("baud.metallib");

type Texture = Retained<ProtocolObject<dyn MTLTexture>>;
type Buffer = Retained<ProtocolObject<dyn MTLBuffer>>;
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
#[derive(Clone, Debug, Default)]
pub struct GpuFrame {
    pub instances: Tally,
    pub upload: Duration,
    pub submit: Duration,
    /// `None` on the asynchronous path: the frame has not finished, so
    /// there is no GPU time to report and a zero would be a lie. It
    /// reaches the caller through [`Done::Notify`] instead.
    pub gpu: Option<Duration>,
    /// What this frame changed of the frame committed before it: the rows
    /// the cursor, its blink animation or its trail moved through, or the
    /// whole. What a presenter records against the surface it went to.
    pub changed: Redraw,
    /// What was drawn into the target: that change and what the target
    /// was missing before it, or the whole.
    pub drawn: Redraw,
    /// **What the wait found**, on a frame waited for: drawn, with the
    /// proof [`Gpu::read`] takes, failed, with why, or unanswered at its
    /// bound. [`Waited::No`] on the asynchronous path, whose answer arrives
    /// through [`Done::Notify`].
    pub waited: Waited,
    /// **The frame, as its deadline wakes carry it**: the window schedules
    /// a wake at each of its bounds with it ([`Deadline::FRAME`],
    /// [`Deadline::GIVE_UP`]). `None` only on a frame never committed.
    pub flying: Option<Flying>,
}

/// Which of the ring's instance buffers is written next.
#[derive(Clone, Copy)]
struct Ring(usize);

/// Shared memory a grid of cells lives in, from [`Gpu::grid`]: what the
/// cell path binds -- in the app, a frame set's copy of the window.
pub struct GridBuffer(Buffer);

/// **Shared memory a surface the core writes lives in**, from
/// [`Gpu::surface`]: the primary ring or the alternate screen, lent to the
/// core through `Owned::storage_over` and held while it is. A type of its
/// own, with no method, so it is never bound: the cell path binds a
/// [`GridBuffer`] (`CellInput::grid`), the copy of the window a frame
/// owns, and the GPU never reads memory the core writes while a frame is
/// in flight.
///
/// A `GridBuffer` is what a frame's input names:
///
/// ```
/// use baud_present_macos::{CellInput, GridBuffer};
/// fn bind<'a>(input: CellInput<'a>, grid: &'a GridBuffer) -> CellInput<'a> {
///     CellInput { grid, ..input }
/// }
/// ```
///
/// and a `SurfaceMemory` is not:
///
/// ```compile_fail,E0308
/// use baud_present_macos::{CellInput, SurfaceMemory};
/// fn bind<'a>(input: CellInput<'a>, grid: &'a SurfaceMemory) -> CellInput<'a> {
///     CellInput { grid, ..input }
/// }
/// ```
pub struct SurfaceMemory {
    _held: Buffer,
}

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

/// What a frame draws of the cell path's shape pass: every cell's slots, the
/// cursor's slot alone (the instance index), or nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ShapeDraw {
    Every,
    Cursor(usize),
    Nothing,
}

/// A trail's own pipeline -- the quad's and the sweep's, or an effect's:
/// built the first frame one is drawn, so a launch pays nothing for a trail
/// it may never show.
enum LatePipe {
    Unbuilt,
    Built(Retained<ProtocolObject<dyn MTLRenderPipelineState>>),
    Failed,
}

impl LatePipe {
    /// The pipeline `pipe`, built now if it never was.
    fn get(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        library: &ProtocolObject<dyn MTLLibrary>,
        pipe: Pipe,
    ) -> Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
        if let LatePipe::Unbuilt = self {
            *self = match pipeline(device, library, pipe) {
                Some(p) => LatePipe::Built(p),
                None => {
                    let (_, fragment) = pipe.functions();
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

/// **An effect's pipelines, to build on a thread of their own**
/// ([`Gpu::effect_build`]): the device and the library they come from.
pub struct EffectBuild {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    library: Retained<ProtocolObject<dyn MTLLibrary>>,
    effect: Effect,
}

// SAFETY: a device and a library, which Apple documents as usable from any
// thread; an `EffectBuild` crosses to the thread that builds from it once,
// whole, and only `newFunctionWithName` and the pipeline's creation read
// them there, which the device and the library allow concurrently.
unsafe impl Send for EffectBuild {}

/// **What an [`EffectBuild`] built**, for [`Gpu::adopt`]: the effect's
/// pipeline, and for the plasma its two in front of the text, by `screen`
/// and by `multiply`.
pub struct EffectPipes {
    effect: Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>>,
    mixed: [Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>>; 2],
    of: Effect,
}

// SAFETY: pipeline states, which Apple documents as usable from any
// thread; they cross back once, whole, to the `Gpu` that adopts them.
unsafe impl Send for EffectPipes {}

impl EffectBuild {
    /// **The pipelines, built now on this thread**: those a trail
    /// would build on its first frame, from the precompiled library about
    /// 0.05 ms each.
    pub fn build(self) -> EffectPipes {
        let effect = pipeline(&self.device, &self.library, Pipe::effect(self.effect));
        let mixed = match self.effect {
            Effect::Plasma => [Pipe::PlasmaScreen, Pipe::PlasmaMultiply]
                .map(|p| pipeline(&self.device, &self.library, p)),
            _ => [None, None],
        };
        EffectPipes {
            effect,
            mixed,
            of: self.effect,
        }
    }
}

/// The uniform a trail's fragment function reads beyond the records:
/// `quad_fragment`'s corners, hole, frame origin and shape, an effect's
/// ignition, which its fragment function reads, or the fireworks' frame
/// origin and hole.
enum TrailUniform {
    Quad([f32; 16]),
    Effect([f32; effect::UNIFORM_FLOATS]),
    Fireworks([f32; baud_render::fireworks::UNIFORM_FLOATS]),
}

impl TrailUniform {
    fn bytes(&self) -> (NonNull<c_void>, usize) {
        match self {
            TrailUniform::Quad(u) => (NonNull::from(u).cast(), core::mem::size_of_val(u)),
            TrailUniform::Effect(u) => (NonNull::from(u).cast(), core::mem::size_of_val(u)),
            TrailUniform::Fireworks(u) => (NonNull::from(u).cast(), core::mem::size_of_val(u)),
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
fn widen(at: Option<(At, At)>, At(i): At) -> Option<(At, At)> {
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
#[derive(Clone, Copy, PartialEq, Eq)]
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
#[derive(Clone, Copy, PartialEq, Eq)]
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
    /// for dimmed flashing lights.
    blink_faint: Uni,
    /// The side of its cell a bar cursor is drawn on: 1 the right, where
    /// the character under it runs right to left ([`baud_render::CursorSide`]),
    /// else 0. It took the pad that kept the struct a multiple of eight
    /// bytes.
    cursor_side: Uni,
}

const _: () = assert!(core::mem::size_of::<CellUniforms>() == 152);

impl CellUniforms {
    /// These uniforms with the cursor's words cleared: what two frames that
    /// differ only in the cursor have alike.
    fn without_cursor(self) -> CellUniforms {
        CellUniforms {
            cursor_col: Uni(0),
            cursor_row: Uni(0),
            cursor: CursorCode(0),
            cursor_color: Uni(0),
            cursor_alpha: Uni(0),
            cursor_text: Uni(0),
            cursor_text_curve: Uni(0),
            ..self
        }
    }

    /// The row the shader draws a cursor on, when it draws one.
    fn cursor_at(&self) -> Option<Row> {
        let none = CursorKind::None.code();
        (self.cursor != none && self.cursor_alpha != Uni(0))
            .then(|| Row::from_index(usize::try_from(self.cursor_row.0).unwrap_or(usize::MAX)))
    }
}

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
    /// Who draws the block cursor ([`baud_render::Overlay::cursor_drawn`]):
    /// the shader, or the records, over the Devanagari syllable that holds
    /// its cell, the shader then drawing no block.
    pub cursor_drawn: baud_render::CursorDrawn,
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
    /// The stamp of the table `styles` holds (`Renderer::sync_styles`):
    /// a frame whose stamp moved may draw any cell in another colour.
    pub table: StyleStamp,
    /// The glyph map's generation and where its glyphs' ink can lie
    /// ([`baud_render::GlyphMap::generation`], `reach`): a map that changed
    /// may draw any cell anew, and a glyph reaching past its row is drawn
    /// with the rows it reaches.
    pub map_gen: MapGeneration,
    pub reach: Reach,
    /// Which of `records` are the cursor's trail and its blink animation,
    /// from the first to one past the last: the records a cursor-only frame
    /// moves, whose rows are its change. The rest must be the last frame's.
    pub moving: (usize, usize),
    /// **The screen the drawn rows of `grid` hold**, as the caller's record
    /// names it ([`crate::screens::Screens::now`]): a frame handed the
    /// screen the last committed frame was handed shows the same cells in
    /// every row the shader reads, so its change is the cursor's rows and
    /// the moving records'. The caller already knows -- it compared the
    /// screen to decide whether to copy it -- so nothing here compares the
    /// cells again or keeps a copy of them to compare with. A caller that
    /// cannot say passes [`ScreenGen::fresh`], a screen of its own, and
    /// the frame is drawn whole.
    pub screen: ScreenGen,
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
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct CursorCode(u32);

/// What to do when the GPU finishes a frame.
pub enum Done {
    /// Block until it has, for at most the bound: for a bench or a test
    /// that reads the target back ([`Deadline::WAIT`]). Never in a frame
    /// loop. What the wait found is the frame's [`GpuFrame::waited`]: past
    /// the bound, [`Waited::Overdue`], the frame still committed.
    Within(Deadline),
    /// [`Done::Within`], and an answer that comes after the bound runs
    /// this, on the GPU's completion thread: the first frame at launch
    /// ([`Deadline::LAUNCH`]), whose window is open by then and is woken
    /// to draw again.
    WithinThen(Deadline, Late),
    /// Run this on the GPU's completion thread, and return at once. It is
    /// handed the frame's [`Completion`]: drawn, with its [`Landing`] --
    /// its own GPU time and when it ended on the host clock, which exist
    /// only once it has completed -- or failed, with why, which every
    /// caller has to match. Nothing times anything -- the fact arrives
    /// with the event that produced it.
    Notify(Arc<dyn Fn(Completion) + Send + Sync>),
}

/// What a draw did.
#[derive(Clone, Debug)]
pub enum Drawn {
    /// Committed; with [`Done::Within`], completed or past the bound, and
    /// what the wait found in [`GpuFrame::waited`].
    Frame(GpuFrame),
    /// A frame is still in flight and reads the memory this one would
    /// bind, so nothing was done. The caller hears about the completion
    /// through its [`Done::Notify`].
    Busy,
    /// **The GPU is given up** ([`GpuUse::GaveUp`]), so nothing was
    /// committed, now or ever again on this `Gpu`: the caller draws on the
    /// CPU from here. Checked after the frame in flight is counted out,
    /// which a handler does only after it has latched a give-up, so no
    /// frame is committed after the one that gave the GPU up.
    GaveUp(GaveUp),
}

/// **What a device has allocated**, in bytes: Metal's
/// `currentAllocatedSize`, "the current size in bytes of all resources
/// allocated by this device" (`MTLDevice.h`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Allocated(pub usize);

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

/// Whether a texture was just made, holding nothing yet, or kept.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Made {
    New,
    Kept,
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
    /// The fireworks' pipeline: `fireworks_vertex` and `fireworks_fragment`.
    fireworks_pipe: LatePipe,
    effect_pipes: [LatePipe; Effect::COUNT],
    /// **The plasma in front of the text** ([`Renderer::trail_mix`]):
    /// `plasma_fragment` by `screen`, then `plasma_multiply_fragment` by
    /// `multiply`.
    plasma_mixed: [LatePipe; 2],
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
    /// The background rows the instance path has uploaded
    /// ([`Gpu::background_uploaded`]).
    background_rows: Tally,
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
    /// Per-image textures, one an image -- images are not glyph-sized and
    /// do not belong in the shared atlas -- each made from the renderer's
    /// snapshot it names and kept while the renderer keeps that snapshot
    /// ([`ImageTextures`]). An image uploads again when its own pixels are
    /// written, when another of its kitty frames is shown, and when the
    /// word a Sixel's background fill is drawn as changes; another image's
    /// store, edit or free uploads nothing, and an image stored under a
    /// freed image's id is a new snapshot, so it cannot take the old
    /// texture.
    images: ImageTextures<Texture>,
    /// Frames committed and not yet completed -- zero or one -- and what
    /// every completion's handler counted ([`commit`]).
    flight: Flight,
    /// The frames the handlers had counted failed, and late, when the last
    /// draw looked: a count past either is a frame since that is not what
    /// [`Gpu::last`] says was drawn.
    failed_seen: (Tally, Tally),
    /// Whether the last frame committed was a screen switch's kept frame,
    /// whose failure leaves the slide nothing defined to slide over.
    kept: Kept,
    /// What `Gpu::new` cost, step by step.
    build: Build,
    /// **What the last frame committed drew**, beside its rows, on each
    /// path: what the next frame must equal to change only rows.
    last: Last,
    /// The rectangle every scissor of the pass being encoded is held to:
    /// the band of rows a partial frame draws, or none.
    clip: Option<MTLScissorRect>,
    /// **The alternate-screen slide** ([`baud_render::ScreenSlide`]): the
    /// old screen kept at the switch, and while it slides, where a frame
    /// is drawn before the two are composed into the target.
    slide: Option<Sliding>,
    /// **The frame after a slide goes whole**: the surface the last slid
    /// frame went to holds the composition, not the frame its age says,
    /// so the change it is redrawn by when it comes round must be the
    /// whole, though nothing on the screen moved. So does the frame after
    /// a kept one, whose change the renderer takes against the kept frame.
    settle: Settle,
}

/// Whether a frame was a screen switch's kept frame ([`Gpu::draw_kept`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kept {
    No,
    Yes,
}

/// Whether the next frame is the first after a slide ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Settle {
    No,
    Owed,
}

/// A screen switch's two frames on the GPU: `kept`, the old screen drawn
/// whole with its cursor hidden ([`Gpu::draw_kept`]), and `scratch`,
/// which each frame of the slide is drawn into whole and then copied from
/// at the slide's offset, the kept frame under it -- the rows
/// [`baud_render::compose_slide`] copies on the CPU path, as two blits.
struct Sliding {
    kept: Texture,
    scratch: Texture,
    size: (Px, Px),
    at: Option<baud_render::ScreenSlide>,
}

/// The last frame committed, as [`Gpu::draw_since`] or
/// [`Gpu::draw_cells_since`] drew it.
enum Last {
    None,
    Instances(InstanceKey),
    Cells(Box<CellLast>),
}

/// What an instance-path frame draws beside the rows the renderer converted
/// ([`Renderer::redraw`]): two frames alike in this differ only in those.
/// Compared field by field with the frame's own reads, and kept in place.
struct InstanceKey {
    size: (Px, Px),
    uniforms: [u32; UNIFORM_FLOATS],
    clear: Argb,
    strip: Vec<Instance>,
    /// The activity lights' records on their line: a light that changed
    /// draws the frame whole, since the line is outside every row's band.
    lights: Vec<Instance>,
    /// Whether an image was drawn, which the renderer's rows do not follow
    /// on this path; or a trail or blink record past the grid, which no
    /// row holds.
    unrowed: Unrowed,
}

/// Whether a frame drew something no grid row holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unrowed {
    No,
    Yes,
}

/// What a cell-path frame drew: everything it was handed and the
/// uniforms built from it, the cursor's words apart.
struct CellLast {
    size: (Px, Px),
    viewport: (u64, u64, u64, u64),
    uniforms: CellUniforms,
    clear: Argb,
    strip: Vec<Instance>,
    /// The activity lights' records on their line, as [`InstanceKey`]'s.
    lights: Vec<Instance>,
    unrowed: Unrowed,
    stride: Stride,
    rotation: RowRot,
    ring: Rows,
    under: Tally,
    chrome: Tally,
    table: StyleStamp,
    map_gen: MapGeneration,
    /// The atlas the map's rectangles name: a map made again over a new
    /// stack starts its generation over, and the atlas's does not.
    atlas: Generation,
    /// The records outside `moving`, and the rows `moving`'s covered.
    records: Vec<Instance>,
    moving: RowSet,
    /// The grounds, the badge and the cursor guide, which move with the
    /// cursor: the guide on its row, the badge with its block's cell left
    /// out (`Renderer::place_grounds`).
    grounds: Vec<Instance>,
    /// The screen its grid's drawn rows held ([`CellInput::screen`]), and
    /// the overlay's bytes.
    screen: ScreenGen,
    overlay: Vec<u8>,
}

/// Whether a pass blends over what is under it, or replaces it. The
/// background passes replace, so a cell's background is a single composite
/// matching the margin's clear; glyphs and shapes blend over. The plasma in
/// the terminal blends by `screen` or `multiply` ([`Mix`]): premultiplied
/// `cs + cb (1 - cs)` by `One, OneMinusSourceColor`, and `cs (1 - ab) + cb
/// (1 - as + cs)` by `OneMinusDestinationAlpha` and the second source,
/// `plasma_multiply_fragment`'s `1 - as + cs`; the alpha `as + ab (1 - as)`
/// under all three, which is W3C Compositing's `ao`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Blend {
    Over,
    Replace,
    Screen,
    Multiply,
}

/// **Every pipeline baud builds**, one variant each: its vertex and fragment
/// functions and its blend. [`pipeline`] takes one of these and nothing
/// else, so no pipeline is built that this list does not name, and the
/// suite's binary archive (`suite-archive`, `suite_misses`) holds the
/// compiled code of every variant in [`Pipe::ALL`].
///
/// Five are built with the [`Gpu`] ([`Gpu::new`]); an effect's, and the
/// plasma's two in front of the text, when its trail is asked for
/// ([`EffectBuild::build`]); and the rest the first frame that draws them
/// ([`LatePipe`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pipe {
    /// The background pass.
    Bg,
    /// The instanced glyph pass.
    Glyph,
    /// The cell path's backgrounds, and over an image below them.
    CellBg,
    CellBgOver,
    /// The cell path's glyphs.
    CellGlyph,
    /// The cell path's shapes.
    CellShape,
    /// Each effect's trail over the records ([`Pipe::effect`]).
    Fire,
    Plasma,
    Lightning,
    Ice,
    Ufo,
    Orb,
    Liquid,
    /// The plasma in front of the text, by `screen` and by `multiply`.
    PlasmaScreen,
    PlasmaMultiply,
    /// An image placement, over and replacing.
    Image,
    ImageReplace,
    /// The fireworks.
    Fireworks,
    /// The cursor trail's quad.
    Quad,
    /// The window's ground and the set behind the text.
    BgGround,
    BgSet,
    /// The hollow cursor's ring.
    CellRing,
}

impl Pipe {
    /// Every variant, the order the suite's archive is written in.
    pub const ALL: [Pipe; 22] = [
        Pipe::Bg,
        Pipe::Glyph,
        Pipe::CellBg,
        Pipe::CellBgOver,
        Pipe::CellGlyph,
        Pipe::CellShape,
        Pipe::Fire,
        Pipe::Plasma,
        Pipe::Lightning,
        Pipe::Ice,
        Pipe::Ufo,
        Pipe::Orb,
        Pipe::Liquid,
        Pipe::PlasmaScreen,
        Pipe::PlasmaMultiply,
        Pipe::Image,
        Pipe::ImageReplace,
        Pipe::Fireworks,
        Pipe::Quad,
        Pipe::BgGround,
        Pipe::BgSet,
        Pipe::CellRing,
    ];

    /// The trail pipeline of `effect`.
    pub const fn effect(effect: Effect) -> Pipe {
        match effect {
            Effect::Fire => Pipe::Fire,
            Effect::Plasma => Pipe::Plasma,
            Effect::Lightning => Pipe::Lightning,
            Effect::Ice => Pipe::Ice,
            Effect::Ufo => Pipe::Ufo,
            Effect::Orb => Pipe::Orb,
            Effect::Liquid => Pipe::Liquid,
        }
    }

    /// Its vertex and fragment functions, as `shaders.metal` names them.
    pub const fn functions(self) -> (&'static str, &'static str) {
        match self {
            Pipe::Bg => ("bg_vertex", "bg_fragment"),
            Pipe::Glyph => ("glyph_vertex", "glyph_fragment"),
            Pipe::CellBg | Pipe::CellBgOver => ("cell_bg_vertex", "flat_fragment"),
            Pipe::CellGlyph => ("cell_glyph_vertex", "glyph_fragment"),
            Pipe::CellShape => ("cell_shape_vertex", "glyph_fragment"),
            Pipe::Fire => ("glyph_vertex", Effect::Fire.fragment()),
            Pipe::Plasma => ("glyph_vertex", Effect::Plasma.fragment()),
            Pipe::Lightning => ("glyph_vertex", Effect::Lightning.fragment()),
            Pipe::Ice => ("glyph_vertex", Effect::Ice.fragment()),
            Pipe::Ufo => ("glyph_vertex", Effect::Ufo.fragment()),
            Pipe::Orb => ("glyph_vertex", Effect::Orb.fragment()),
            Pipe::Liquid => ("glyph_vertex", Effect::Liquid.fragment()),
            Pipe::PlasmaScreen => ("glyph_vertex", "plasma_fragment"),
            Pipe::PlasmaMultiply => ("glyph_vertex", "plasma_multiply_fragment"),
            Pipe::Image => ("glyph_vertex", "image_fragment"),
            Pipe::ImageReplace => ("glyph_vertex", "image_replace_fragment"),
            Pipe::Fireworks => ("fireworks_vertex", "fireworks_fragment"),
            Pipe::Quad => ("glyph_vertex", "quad_fragment"),
            Pipe::BgGround => ("bg_vertex", "bg_ground_fragment"),
            Pipe::BgSet => ("bg_vertex", "bg_set_fragment"),
            Pipe::CellRing => ("cell_ring_vertex", "glyph_fragment"),
        }
    }

    /// How it blends over what is under it.
    const fn blend(self) -> Blend {
        match self {
            Pipe::Bg | Pipe::CellBg | Pipe::ImageReplace | Pipe::BgGround => Blend::Replace,
            Pipe::PlasmaScreen => Blend::Screen,
            Pipe::PlasmaMultiply => Blend::Multiply,
            Pipe::Glyph
            | Pipe::CellGlyph
            | Pipe::CellShape
            | Pipe::Fire
            | Pipe::Plasma
            | Pipe::Lightning
            | Pipe::Ice
            | Pipe::Ufo
            | Pipe::Orb
            | Pipe::Liquid
            | Pipe::Image
            | Pipe::Fireworks
            | Pipe::Quad
            | Pipe::BgSet
            | Pipe::CellBgOver
            | Pipe::CellRing => Blend::Over,
        }
    }
}

/// **The pipeline `pipe`.** In a test's or a bench's build
/// (`suite-archive`) its compiled code comes from the suite's binary
/// archive and a miss is refused (`suite.rs`); otherwise it is compiled, or
/// served from the system's shader cache, as the app has always done.
fn pipeline(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    pipe: Pipe,
) -> Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
    let desc = descriptor(library, pipe)?;
    #[cfg(feature = "suite-archive")]
    let built = suite::build(device, library, &desc);
    #[cfg(not(feature = "suite-archive"))]
    let built = device.newRenderPipelineStateWithDescriptor_error(&desc);
    match built {
        Ok(p) => Some(p),
        Err(e) => {
            let (vertex, fragment) = pipe.functions();
            eprintln!("baud-present-macos: pipeline {vertex}/{fragment}: {e:?}");
            None
        }
    }
}

/// **`pipe`'s descriptor**, the one thing both the app's builds and the
/// suite's archive are made from; `None` when the library lacks a function.
fn descriptor(
    library: &ProtocolObject<dyn MTLLibrary>,
    pipe: Pipe,
) -> Option<Retained<MTLRenderPipelineDescriptor>> {
    let (vertex, fragment) = pipe.functions();
    let blend = pipe.blend();
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
    att.setBlendingEnabled(!matches!(blend, Blend::Replace));
    let (source, destination) = match blend {
        Blend::Over | Blend::Replace => (MTLBlendFactor::One, MTLBlendFactor::OneMinusSourceAlpha),
        Blend::Screen => (MTLBlendFactor::One, MTLBlendFactor::OneMinusSourceColor),
        Blend::Multiply => (
            MTLBlendFactor::OneMinusDestinationAlpha,
            MTLBlendFactor::Source1Color,
        ),
    };
    att.setSourceRGBBlendFactor(source);
    att.setDestinationRGBBlendFactor(destination);
    att.setSourceAlphaBlendFactor(MTLBlendFactor::One);
    att.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
    Some(desc)
}

/// **The pipelines the suite's archive cannot give** (`suite-archive`):
/// each [`Pipe`] built from it with a miss refused, and the ones refused.
/// Empty when the archive holds every pipeline baud builds.
#[cfg(feature = "suite-archive")]
pub fn suite_misses() -> Vec<Pipe> {
    let builds = SuiteBuilds::new();
    Pipe::ALL
        .into_iter()
        .filter(|&p| builds.build(p, CodeFrom::Archive).is_err())
        .collect()
}

/// Where a test's pipeline takes its compiled code from (`suite-archive`).
#[cfg(feature = "suite-archive")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeFrom {
    /// The suite's archive, a miss refused: what every test's build does.
    Archive,
    /// The compiler, through the system's shader cache: what the app does,
    /// for a test that shows what the archive spares.
    Compiler,
}

/// **The app's own pipelines, built one at a time for a test**
/// (`suite-archive`): the device and `baud.metallib`, and each [`Pipe`]
/// from the descriptor the app builds it from, its code from where the test
/// says.
#[cfg(feature = "suite-archive")]
pub struct SuiteBuilds {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    library: Retained<ProtocolObject<dyn MTLLibrary>>,
}

#[cfg(feature = "suite-archive")]
impl SuiteBuilds {
    /// The system's device and the committed library.
    pub fn new() -> SuiteBuilds {
        let device = MTLCreateSystemDefaultDevice().expect("a Metal device");
        let library = device
            .newLibraryWithData_error(&DispatchData::from_static_bytes(METALLIB))
            .expect("baud.metallib loads");
        SuiteBuilds { device, library }
    }

    /// Where this machine's archive for these descriptors is kept.
    pub fn archive_path(&self) -> std::path::PathBuf {
        suite::path(&self.device, &self.library)
    }

    /// `pipe`, its code from `from`; the error's text when it is refused.
    pub fn build(&self, pipe: Pipe, from: CodeFrom) -> Result<(), String> {
        let desc = descriptor(&self.library, pipe).ok_or("a function the library lacks")?;
        match from {
            CodeFrom::Archive => suite::build(&self.device, &self.library, &desc)
                .map(drop)
                .map_err(|e| format!("{e:?}")),
            CodeFrom::Compiler => self
                .device
                .newRenderPipelineStateWithDescriptor_error(&desc)
                .map(drop)
                .map_err(|e| format!("{e:?}")),
        }
    }

    /// **An archive of `pipes` alone, written to `path`**: for a test that
    /// needs an archive missing some, to meet a miss at use.
    pub fn write_archive(&self, path: &std::path::Path, pipes: &[Pipe]) {
        suite::write(&self.device, &self.library, path, pipes);
    }
}

#[cfg(feature = "suite-archive")]
impl Default for SuiteBuilds {
    fn default() -> SuiteBuilds {
        SuiteBuilds::new()
    }
}

#[cfg(feature = "suite-archive")]
mod suite;

mod commit;
#[cfg(feature = "gpu-fault")]
pub use commit::Inject;
#[cfg(all(feature = "gpu-fault", feature = "effect-probe"))]
pub(crate) use commit::rig::{
    arm as rig_arm, held as rig_held, made_count as rig_made, release as rig_release,
};
pub use commit::{
    Came, Cause, CommitId, Completion, Deadline, ErrorNumber, Fault, FaultCode, Faults, Flying,
    GaveUp, GpuUse, InARow, Landed, Landing, Late, Overdue, Refusal, StatusWord, StillBusy, Waited,
};
use commit::{Encoded, Flight, Then};

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
/// contents, read for as long as the borrow of the buffer lasts.
///
/// # Safety
/// `T` must be valid for any bit pattern the buffer holds, and nothing may
/// write the elements while the slice is read.
unsafe fn peek<T>(buffer: &Buffer, n: Elems) -> Option<&[T]> {
    let n = n.0;
    if buffer.length() < n.saturating_mul(core::mem::size_of::<T>()) {
        return None;
    }
    let ptr = buffer.contents().cast::<T>();
    // SAFETY: a shared buffer's contents are page-aligned and at least
    // `n * size_of::<T>()` bytes, as checked; the caller upholds the rest.
    Some(unsafe { core::slice::from_raw_parts(ptr.as_ptr(), n) })
}

/// A `repr(C)` slice over the first `n` elements of a shared buffer's
/// contents, for the life of the process: the buffer is retained forever.
///
/// # Safety
/// `T` must be valid for any bit pattern the caller then writes, and the
/// caller must not write the slice while a committed frame the GPU has not
/// finished reads the buffer: the wheel holds a frame set's until it lands,
/// and [`Gpu::in_flight`] is the gate for one bound otherwise.
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
// thread with the caller ordering access, plus the `Arc`s of atomics a
// `Flight` shares with its handlers; the
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
    /// **73.6**, reproduced across every settled launch, and that +28.5 was
    /// left unexplained with one named next step: time the five separately
    /// under both libraries. A cost spread across all five in proportion to
    /// their shaders is work `newLibraryWithSource` was doing eagerly and
    /// `newLibraryWithData` leaves here; a cost in one is something about
    /// that shader.
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
        let bg_pipe = pipeline(&device, &library, Pipe::Bg)?;
        each[0] = p0.elapsed();
        let p1 = Instant::now();
        let glyph_pipe = pipeline(&device, &library, Pipe::Glyph)?;
        each[1] = p1.elapsed();
        let p2 = Instant::now();
        let cell_bg_pipe = pipeline(&device, &library, Pipe::CellBg)?;
        each[2] = p2.elapsed();
        let p3 = Instant::now();
        let cell_glyph_pipe = pipeline(&device, &library, Pipe::CellGlyph)?;
        each[3] = p3.elapsed();
        let p4 = Instant::now();
        let cell_shape_pipe = pipeline(&device, &library, Pipe::CellShape)?;
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
            fireworks_pipe: LatePipe::Unbuilt,
            effect_pipes: core::array::from_fn(|_| LatePipe::Unbuilt),
            plasma_mixed: [LatePipe::Unbuilt, LatePipe::Unbuilt],
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
            background_rows: Tally::ZERO,
            ground: None,
            ground_bytes: Vec::new(),
            grounds: Vec::new(),
            mask: None,
            color: None,
            uploaded: None,
            images: ImageTextures::default(),
            flight: Flight::new(),
            failed_seen: (Tally::ZERO, Tally::ZERO),
            kept: Kept::No,
            build,
            last: Last::None,
            clip: None,
            slide: None,
            settle: Settle::No,
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
    /// frame. Its status is read and a failure counted
    /// ([`Faults::warm_failed`]); it is not counted in flight, since it
    /// reads nothing a frame writes.
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
        let Some(cmd) = self.flight.encoded(&self.queue) else {
            return;
        };
        let Some(enc) = cmd.render(&pass) else {
            return;
        };
        enc.setRenderPipelineState(&self.cell_bg_pipe);
        enc.endEncoding();
        let _ = cmd.commit(&self.flight, Then::Discard);
    }

    /// **The background rows the instance path has uploaded**: a frame
    /// uploads the rows whose words changed since the frame before
    /// ([`Renderer::take_background_dirty`]), all of them into a texture
    /// just made, and none on a frame that changed none.
    pub fn background_uploaded(&self) -> Tally {
        self.background_rows
    }

    /// What every completion's handler has counted: the frames that
    /// failed, by code, the warm-up pass, the slowest frame, and the
    /// frames past their bounds.
    pub fn faults(&self) -> Faults {
        self.flight.faults()
    }

    /// Whether the window still draws on the GPU, or gave it up.
    pub fn gpu_use(&self) -> GpuUse {
        self.flight.faults().gpu_use()
    }

    /// What the device has allocated, every resource of every `Gpu` on it
    /// counted (`currentAllocatedSize`).
    pub fn allocated(&self) -> Allocated {
        Allocated(self.device.currentAllocatedSize())
    }

    /// **Waits until no frame is in flight, for at most `bound`**,
    /// spinning: for a bench or a test about to write what a frame reads.
    /// Never in a frame loop, which its completion tells. Past the bound,
    /// what was counted, for the caller to fail on.
    pub fn idle_within(&self, bound: Deadline) -> Result<(), StillBusy> {
        self.flight.idle_within(bound)
    }

    /// **The fault seam**: what the completions of the frames committed
    /// from now on report ([`Inject`]). Test builds only.
    #[cfg(feature = "gpu-fault")]
    pub fn inject(&self, inject: Inject) {
        self.flight.set_inject(inject);
    }

    /// How many completions [`Inject::Hold`] holds.
    #[cfg(feature = "gpu-fault")]
    pub fn held(&self) -> Tally {
        self.flight.held()
    }

    /// Delivers every held completion on this thread, as its handler would
    /// have: counted, counted out of flight, answered. How many there were.
    #[cfg(feature = "gpu-fault")]
    pub fn release(&self) -> Tally {
        self.flight.release()
    }

    /// Waits up to `within` for `n` completions to be held; how many are.
    #[cfg(feature = "gpu-fault")]
    pub fn await_held(&self, n: Tally, within: Duration) -> Tally {
        self.flight.await_held(n, within)
    }

    /// Waits up to `within` for `n` completions to have been delivered in
    /// all; how many were.
    #[cfg(feature = "gpu-fault")]
    pub fn await_delivered(&self, n: Tally, within: Duration) -> Tally {
        self.flight.await_delivered(n, within)
    }

    /// **A frame that failed, or landed late, since the last draw looked
    /// is not the last frame**: a failed one's target holds undefined
    /// content, and one past its bound was for every consumer not drawn
    /// until it landed, so the next frame is compared with nothing and
    /// drawn whole, and a kept frame leaves no slide. The handler counts a
    /// failure or a late answer before it counts the frame out, and the
    /// busy check before this acquires the count-out, so neither is missed
    /// here.
    fn take_failures(&mut self) {
        let f = self.flight.faults();
        let seen = (f.frames_failed(), f.late);
        if seen != self.failed_seen {
            self.failed_seen = seen;
            self.forget_last();
        }
    }

    /// The last frame committed did not draw.
    fn forget_last(&mut self) {
        self.last = Last::None;
        if self.kept == Kept::Yes {
            self.slide = None;
        }
    }

    /// Whether the device shares memory with the CPU, which is what makes
    /// a shared buffer the CPU wrote a read for the GPU rather than a
    /// transfer (D2's discrete arm is unbuilt).
    pub fn unified_memory(&self) -> Memory {
        if self.device.hasUnifiedMemory() {
            Memory::Unified
        } else {
            Memory::Discrete
        }
    }

    /// Whether a frame is being read by the GPU. While it is, the buffers
    /// it was committed with must not be written: a frame set's are held
    /// by the wheel until it lands, and a caller that bound the core's own
    /// grid must not feed it until this says idle.
    pub fn in_flight(&self) -> InFlight {
        match self.flight.busy() {
            false => InFlight::Idle,
            true => InFlight::Busy,
        }
    }

    fn shared(&self, bytes: Span) -> Option<Buffer> {
        self.device
            .newBufferWithLength_options(bytes.0.max(1), MTLResourceOptions::StorageModeShared)
    }

    /// Memory for a grid of `rows` rows at `stride` cells, in the GPU's
    /// own reach: the buffer the cell path binds, and the slice its owner
    /// writes -- a frame set's copy of the window. Every cell starts empty.
    pub fn grid(
        &self,
        rows: Rows,
        stride: Stride,
    ) -> Option<(GridBuffer, &'static mut [PackedCell])> {
        let n = rows.index().saturating_mul(stride.index());
        let buffer = self.shared(Span(n.saturating_mul(core::mem::size_of::<PackedCell>())))?;
        // SAFETY: `PackedCell` is `repr(C)` over plain integers, valid for
        // every bit pattern, and the slice is filled before anything reads
        // it; the caller writes it only while no committed frame reads it --
        // a frame set's copy under the wheel, a grid the core is lent behind
        // the in-flight gate.
        let cells = unsafe { lend::<PackedCell>(&buffer, Elems(n)) }?;
        cells.fill(PackedCell::EMPTY);
        Some((GridBuffer(buffer), cells))
    }

    /// **Memory for a surface the core writes**, `rows` rows at `stride`
    /// cells, and the slice to lend it through `Owned::storage_over` --
    /// the primary ring at `Owned::ring` rows, the alternate screen at
    /// `Owned::rows`. Every cell starts empty. The memory is a
    /// [`SurfaceMemory`], which no frame can bind.
    pub fn surface(
        &self,
        rows: Rows,
        stride: Stride,
    ) -> Option<(SurfaceMemory, &'static mut [PackedCell])> {
        let (grid, cells) = self.grid(rows, stride)?;
        Some((SurfaceMemory { _held: grid.0 }, cells))
    }

    /// Memory for the rendition table resolved, one entry a slot of
    /// `term`'s table, for `Renderer::resolve_styles_into` to write.
    pub fn styles(&self, term: &Term<'_>) -> Option<(StylesBuffer, &'static mut [Resolved])> {
        let n = term.styles().one().len();
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

    /// Makes sure the background texture matches the grid, and says
    /// whether it was just made, holding nothing yet.
    fn background_for(&mut self, cols: Cols, rows: Rows) -> Option<(&Background, Made)> {
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
        let made = match stale {
            true => Made::New,
            false => Made::Kept,
        };
        self.background.as_ref().map(|b| (b, made))
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

    /// Ensures every visible image has a GPU texture -- uploading each one
    /// once for each change to its pixels, its frame or its ground, into its
    /// own texture rather than the shared atlas, because an image is not
    /// glyph-sized -- and returns the placements to draw, in the renderer's
    /// order, each with the texture to bind. A texture whose snapshot the
    /// renderer no longer holds is dropped ([`ImageTextures::keep`]).
    /// Called before the encoder, so a `replaceRegion` upload never touches
    /// a bound texture.
    fn resolve_images(&mut self, renderer: &Renderer<'_, '_>) -> Vec<ImageDraw> {
        let ups = renderer.image_uploads();
        self.images.keep(ups);
        let places = renderer.image_placements();
        let mut draws = Vec::with_capacity(places.len());
        let device = &self.device;
        for p in places {
            let Some(up) = ups.iter().find(|u| {
                u.id == p.id && u.born == p.born && u.frame == p.frame && u.ground == p.ground
            }) else {
                continue;
            };
            let made = self.images.of(up, |up| {
                let (w, h) = (Px::of(u32::from(up.w)), Px::of(u32::from(up.h)));
                let tex = texture(
                    device,
                    MTLPixelFormat::BGRA8Unorm,
                    w,
                    h,
                    MTLTextureUsage::ShaderRead,
                )?;
                upload(&tex, w, h, word_bytes(&up.pixels), Texel::Bgra8);
                Some(tex)
            });
            let Some(tex) = made else {
                continue;
            };
            let (xs, ys) = renderer.image_map(p);
            let mut map = Vec::with_capacity(xs.len() + ys.len());
            map.extend_from_slice(xs);
            map.extend_from_slice(ys);
            draws.push(ImageDraw {
                inst: image_instance(p),
                tex,
                layer: p.layer,
                map,
                composite: p.composite,
            });
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
            for (chunk, d) in dst[..records]
                .as_chunks_mut::<{ Instance::BYTES }>()
                .0
                .iter_mut()
                .zip(draws)
            {
                let mut inst = d.inst;
                inst.y = Screen(inst.y.0.saturating_add(pan16));
                chunk.copy_from_slice(&inst.to_bytes());
            }
            for (d, &(xs, _)) in draws.iter().zip(&maps) {
                for (out, v) in dst[xs..xs + d.map.len() * 4]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
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
            .get(&self.device, &self.library, Pipe::Image)?;
        // A replacing placement's pipeline, built only on a frame with one.
        let replace = match draws
            .iter()
            .any(|d| d.layer == layer && d.composite == ImageComposite::Replace)
        {
            true => Some(self.image_replace_pipe.get(
                &self.device,
                &self.library,
                Pipe::ImageReplace,
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

    /// **What builds `effect`'s pipelines off this thread**, for the launch
    /// splash: the device and the library, which [`EffectBuild::build`]
    /// builds the effect's pipeline from on any thread -- and the plasma's
    /// two in front of the text, which the terminal's first move draws by
    /// -- and [`Gpu::adopt`] takes back. So the plasma's first frame waits
    /// on no build and nothing before it waits on the plasma: the window's
    /// first frame and the shell's output are drawn while it builds. None
    /// for an effect this GPU has no slot for.
    pub fn effect_build(&self, effect: Effect) -> Option<EffectBuild> {
        Effect::ALL.contains(&effect).then(|| EffectBuild {
            device: self.device.clone(),
            library: self.library.clone(),
            effect,
        })
    }

    /// **The pipelines an [`EffectBuild`] built**, each taken where it is
    /// not built already: a trail drawn meanwhile built its own.
    pub fn adopt(&mut self, built: EffectPipes) {
        let take =
            |pipe: &mut LatePipe,
             p: Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>>| {
                if let (LatePipe::Unbuilt, Some(p)) = (&*pipe, p) {
                    *pipe = LatePipe::Built(p);
                }
            };
        let [screen, multiply] = built.mixed;
        if let Some(pipe) = Effect::ALL
            .iter()
            .position(|&e| e == built.of)
            .and_then(|at| self.effect_pipes.get_mut(at))
        {
            take(pipe, built.effect);
        }
        take(&mut self.plasma_mixed[0], screen);
        take(&mut self.plasma_mixed[1], multiply);
    }

    /// Whether `effect`'s pipeline is built: what the launch splash's
    /// start reads.
    pub fn effect_built(&self, effect: Effect) -> bool {
        Effect::ALL
            .iter()
            .position(|&e| e == effect)
            .is_some_and(|at| matches!(self.effect_pipes[at], LatePipe::Built(_)))
    }

    /// The plasma's pipeline by `mix`, built now if it never was: `screen`
    /// over `plasma_fragment`, `multiply` over `plasma_multiply_fragment`.
    fn plasma_mixed(
        &mut self,
        mix: Mix,
    ) -> Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
        let (at, pipe) = match mix {
            Mix::Normal => return None,
            Mix::Screen => (0, Pipe::PlasmaScreen),
            Mix::Multiply => (1, Pipe::PlasmaMultiply),
        };
        self.plasma_mixed[at].get(&self.device, &self.library, pipe)
    }

    /// The trail's draw for this frame, if the renderer has one and its
    /// pipeline could be built: `glyph_vertex` over the records, and
    /// `quad_fragment` over a quad's or a sweep's corners, or an effect's own
    /// fragment function over its ignition. Each pipeline is built once, the
    /// first time.
    fn trail_draw(&mut self, renderer: &Renderer<'_, '_>, pad: [f32; 2]) -> Option<TrailDraw> {
        // The fireworks, while they hold the effect's slot: their own
        // vertex function, which hands the fragment function each star's
        // turn and side from the record.
        if let Some(u) = renderer.fireworks_uniform(pad) {
            let pipe = self
                .fireworks_pipe
                .get(&self.device, &self.library, Pipe::Fireworks)?;
            return Some(TrailDraw {
                pipe,
                uniform: TrailUniform::Fireworks(u),
            });
        }
        if let Some(smear) = renderer.smear() {
            let pipe = self
                .quad_pipe
                .get(&self.device, &self.library, Pipe::Quad)?;
            return Some(TrailDraw {
                pipe,
                uniform: TrailUniform::Quad(smear.uniform(pad)),
            });
        }
        let vfx = renderer.vfx()?;
        // The effect's place in `Effect::ALL`, which the array follows; the
        // plasma in front of the text by its own blend, as the CPU path
        // composites it.
        let at = Effect::ALL.iter().position(|&e| e == vfx.effect)?;
        let pipe = match renderer.trail_mix() {
            Mix::Normal => {
                self.effect_pipes[at].get(&self.device, &self.library, Pipe::effect(vfx.effect))?
            }
            mix => self.plasma_mixed(mix)?,
        };
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

    /// **The target a screen switch's kept frame is drawn into**: the
    /// slide's two textures at `size`, the targets' size -- made, or the
    /// last slide's kept when they are that size -- and none sliding yet.
    /// `None` when a texture could not be made.
    fn kept_target(&mut self, size: (Px, Px)) -> Option<Target> {
        let reuse = self.slide.take().filter(|s| s.size == size);
        let (kept, scratch) = match reuse {
            Some(s) => (s.kept, s.scratch),
            None => (
                texture(
                    &self.device,
                    MTLPixelFormat::BGRA8Unorm,
                    size.0,
                    size.1,
                    MTLTextureUsage::RenderTarget,
                )?,
                texture(
                    &self.device,
                    MTLPixelFormat::BGRA8Unorm,
                    size.0,
                    size.1,
                    MTLTextureUsage::RenderTarget,
                )?,
            ),
        };
        let target = Target {
            texture: kept.clone(),
            width: size.0,
            height: size.1,
        };
        self.slide = Some(Sliding {
            kept,
            scratch,
            size,
            at: None,
        });
        Some(target)
    }

    /// **A screen switch's old screen, kept for its slide**: the
    /// renderer's frame drawn whole into a texture of its own at `size`,
    /// the targets' size, after [`Renderer::hide_cursor`] took its cursor
    /// off, as sfte draws its outgoing screen. The surface the next frame
    /// goes to is then drawn whole: the renderer's change is taken against
    /// this frame, which no surface holds. [`Drawn::Busy`] while a frame
    /// is in flight; `None` when a resource could not be made.
    pub fn draw_kept(
        &mut self,
        renderer: &mut Renderer<'_, '_>,
        size: (Px, Px),
        done: Done,
    ) -> Option<Drawn> {
        if self.in_flight() == InFlight::Busy {
            return Some(Drawn::Busy);
        }
        if let GpuUse::GaveUp(why) = self.gpu_use() {
            return Some(Drawn::GaveUp(why));
        }
        // Before the kept target is made: a failed kept frame before this
        // one drops the slide it was kept for.
        self.take_failures();
        let target = self.kept_target(size)?;
        let drawn = self.draw_since(renderer, &target, done, Redraw::Whole);
        self.settle = Settle::Owed;
        self.kept(drawn)
    }

    /// [`Gpu::draw_kept`] on the cell path: `input` is the last frame's,
    /// built again with no cursor, no trail, no blink animation and no
    /// cursor guide.
    pub fn draw_cells_kept(
        &mut self,
        renderer: &mut Renderer<'_, '_>,
        input: &CellInput<'_>,
        size: (Px, Px),
        done: Done,
    ) -> Option<Drawn> {
        if self.in_flight() == InFlight::Busy {
            return Some(Drawn::Busy);
        }
        if let GpuUse::GaveUp(why) = self.gpu_use() {
            return Some(Drawn::GaveUp(why));
        }
        // Before the kept target is made: a failed kept frame before this
        // one drops the slide it was kept for.
        self.take_failures();
        let target = self.kept_target(size)?;
        let drawn = self.draw_cells_since(renderer, input, &target, done, Redraw::Whole);
        self.settle = Settle::Owed;
        self.kept(drawn)
    }

    /// A kept frame committed: the last frame is a kept one, and a kept
    /// frame waited for that failed, or is past its bound, leaves no slide
    /// now.
    fn kept(&mut self, drawn: Option<Drawn>) -> Option<Drawn> {
        if let Some(Drawn::Frame(f)) = &drawn {
            self.kept = Kept::Yes;
            if let Waited::Failed(_) | Waited::Overdue(_) = f.waited {
                self.forget_last();
            }
        }
        drawn
    }

    /// **Where the slide is for the next frame**, or none: `None` ends it
    /// and lets the two textures go. A slide with no kept frame, or one
    /// kept at another size, draws as no slide.
    pub fn set_slide(&mut self, at: Option<baud_render::ScreenSlide>) {
        match at {
            None => {
                if self.slide.as_ref().is_some_and(|s| s.at.is_some()) {
                    self.settle = Settle::Owed;
                }
                self.slide = None;
            }
            Some(at) => {
                if let Some(s) = self.slide.as_mut() {
                    s.at = Some(at);
                }
            }
        }
    }

    /// The slide drawing into a target of `size`: one under way, with its
    /// frame kept at that size.
    fn slide_at(&self, size: (Px, Px)) -> Option<(&Sliding, baud_render::ScreenSlide)> {
        let s = self.slide.as_ref().filter(|s| s.size == size)?;
        Some((s, s.at?))
    }

    /// A frame's change while it slides, and the first after it: the
    /// whole, since every row moves, and then every row moves back.
    fn sliding(&mut self, changed: Redraw) -> Redraw {
        let settle = core::mem::replace(&mut self.settle, Settle::No);
        match (self.slide.as_ref().and_then(|s| s.at), settle) {
            (Some(_), _) | (None, Settle::Owed) => Redraw::Whole,
            (None, Settle::No) => changed,
        }
    }

    /// The scratch target a sliding frame is drawn into before it is
    /// composed into `target`, or none when it does not slide.
    fn scratch_for(&self, target: &Target) -> Option<Target> {
        let (s, _) = self.slide_at(target.size())?;
        Some(Target {
            texture: s.scratch.clone(),
            width: target.width,
            height: target.height,
        })
    }

    /// **A sliding frame composed into `target`**: everything but the
    /// frame's sliding rows -- the margin above and below the whole frame,
    /// `whole` rows tall and centred as [`letterbox`] centres it, and the
    /// activity lights' line, which holds still -- from the scratch, and
    /// each of the rows [`Parts`] gives the grid's frame from the scratch
    /// at the slide's offset where it lies, else from the kept frame --
    /// [`baud_render::slide_source`], the rule the CPU path copies by. Every
    /// copy is whole rows, so the left and right margins come with them.
    fn compose_slide(&self, cmd: &Encoded, target: &Target, whole: Px, parts: Parts) {
        let Some((s, at)) = self.slide_at(target.size()) else {
            return;
        };
        let (tw, th) = (slots(target.width), slots(target.height));
        let whole = slots(whole).min(th);
        let h = slots(parts.rows).min(whole);
        let top = (th - whole) / 2 + slots(parts.top).min(whole - h);
        let o = baud_render::slide_offsets(at, Px::of(u32::try_from(h).unwrap_or(0)));
        let rows = Px::of(u32::try_from(h).unwrap_or(0));
        // Runs of frame rows that read one texture at one offset.
        let mut bands: Vec<(usize, usize, &Texture, usize)> = Vec::new();
        for y in (0..rows.value()).map(Px::of) {
            let Some((frame, r)) = baud_render::slide_source(o, y, rows) else {
                continue;
            };
            let from = match frame {
                baud_render::SlideFrame::Incoming => &s.scratch,
                baud_render::SlideFrame::Outgoing => &s.kept,
            };
            let (y, r) = (slots(y), slots(r));
            match bands.last_mut() {
                Some((dst, n, tex, src))
                    if core::ptr::eq(*tex, from) && *dst + *n == y && *src + *n == r =>
                {
                    *n += 1;
                }
                _ => bands.push((y, 1, from, r)),
            }
        }
        let Some(blit) = cmd.blit() else {
            return;
        };
        let copy = |from: &Texture, src: usize, dst: usize, n: usize| {
            if n == 0 {
                return;
            }
            // SAFETY: both textures are the target's size and the rows
            // `src..src + n` and `dst..dst + n` are inside it; Metal
            // retains what an encoded copy names until the buffer completes.
            unsafe {
                blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                    from,
                    0,
                    0,
                    MTLOrigin { x: 0, y: src, z: 0 },
                    MTLSize { width: tw, height: n, depth: 1 },
                    &target.texture,
                    0,
                    0,
                    MTLOrigin { x: 0, y: dst, z: 0 },
                );
            }
        };
        copy(&s.scratch, 0, 0, top);
        copy(&s.scratch, top + h, top + h, th - top - h);
        for (dst, n, from, src) in bands {
            copy(from, top + src, top + dst, n);
        }
        blit.endEncoding();
    }

    /// A pass over `target`, cleared to `clear` -- the terminal's own
    /// background, so the margin around the grid is the same colour as a
    /// default cell rather than a see-through edge -- with the rasteriser
    /// confined to `viewport`, the grid centred in the surface. A frame
    /// `drawn` by rows loads what the target holds instead of clearing it.
    fn encoder(
        &self,
        target: &Target,
        clear: MTLClearColor,
        viewport: MTLViewport,
        drawn: &Redraw,
    ) -> Option<(Encoded, Encoder)> {
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: attachment 0 exists on every pass descriptor.
        let att = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        att.setTexture(Some(&target.texture));
        att.setLoadAction(match drawn {
            Redraw::Whole => MTLLoadAction::Clear,
            Redraw::Rows(_) => MTLLoadAction::Load,
        });
        att.setStoreAction(MTLStoreAction::Store);
        att.setClearColor(clear);
        let cmd = self.flight.encoded(&self.queue)?;
        let enc = cmd.render(&pass)?;
        enc.setViewport(viewport);
        Some((cmd, enc))
    }

    /// Commits `cmd` ([`Encoded::commit`]): waits, for at most the bound,
    /// or arms `done` and returns. The frame is counted in flight either
    /// way, and out again by its handler. The time is the commit's -- with
    /// the wait, on [`Done::Within`] -- and the wait's finding is the
    /// frame's [`GpuFrame::waited`]; the frame as its deadline wakes carry
    /// it is its [`GpuFrame::flying`].
    fn commit(&self, cmd: Encoded, done: Done) -> (Duration, Waited, Option<Flying>) {
        let t1 = Instant::now();
        let (then, waits) = match done {
            Done::Within(bound) => (Then::Within(bound, None), true),
            Done::WithinThen(bound, late) => (Then::Within(bound, Some(late)), true),
            Done::Notify(then) => (Then::Notify(then), false),
        };
        let committed = cmd.commit(&self.flight, then);
        let flying = committed.flying();
        let waited = match waits {
            true => committed.wait(),
            false => Waited::No,
        };
        (t1.elapsed(), waited, flying)
    }

    /// What a draw that committed `waited` leaves: the last frame, unless
    /// a frame waited for failed or is past its bound, which leaves none;
    /// the frame is not a kept one until [`Gpu::kept`] says so.
    fn committed(&mut self, waited: &Waited) {
        self.kept = Kept::No;
        if let Waited::Failed(_) | Waited::Overdue(_) = waited {
            self.forget_last();
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
        self.draw_since(renderer, target, done, Redraw::Whole)
    }

    /// [`Gpu::draw`] into a target that holds an earlier frame and lacks
    /// `missing` of the last one committed ([`crate::redraw::Ages`]). When
    /// this frame and those changed only rows -- the cursor, its blink
    /// animation, its trail or the cursor guide ([`Renderer::redraw`]), and
    /// nothing an image, the exit-status strip or the frame's size draws --
    /// the target is loaded and only those rows are drawn, each band the
    /// whole pass held to its scissor; otherwise the whole frame is.
    pub fn draw_since(
        &mut self,
        renderer: &mut Renderer<'_, '_>,
        target: &Target,
        done: Done,
        missing: Redraw,
    ) -> Option<Drawn> {
        if self.in_flight() == InFlight::Busy {
            return Some(Drawn::Busy);
        }
        if let GpuUse::GaveUp(why) = self.gpu_use() {
            return Some(Drawn::GaveUp(why));
        }
        self.take_failures();
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
        // The exit-status marks after them, drawn in the strip's own pass,
        // the activity lights after those, in their line's, and the word
        // wrap's marks last, over the grid's rows across the whole width.
        let marks = renderer.strip_records().len();
        let lights = renderer.lights_records().len();
        let wraps = renderer.margin_records().len();
        let bytes = (count + marks + lights + wraps).saturating_mul(Instance::BYTES);
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
                .as_chunks_mut::<{ Instance::BYTES }>()
                .0
                .iter_mut()
                .zip(
                    renderer
                        .instances()
                        .chain(renderer.strip_records())
                        .chain(renderer.lights_records())
                        .chain(renderer.margin_records()),
                )
                .enumerate()
            {
                if matches!(inst.kind, Kind::Quad | Kind::Band | Kind::Particle) {
                    trail_at = widen(trail_at, At(i));
                }
                chunk.copy_from_slice(&inst.to_bytes());
            }
        }
        self.ring = Ring((slot + 1) % RING);
        // Only the background rows that changed since the last frame
        // drawn: all of them into a texture just made.
        let dirty = renderer.take_background_dirty();
        let (bg, made) = self.background_for(cols, rows)?;
        let (bw, bh) = (
            Px::of(u32::try_from(bg.cols.index()).ok()?),
            Px::of(u32::try_from(bg.rows.index()).ok()?),
        );
        let band = match (made, dirty) {
            (Made::New, _) | (_, BackgroundDirty::Whole) => Some((Row::ZERO, bg.rows)),
            (Made::Kept, BackgroundDirty::Clean) => None,
            (Made::Kept, BackgroundDirty::Rows { first, end }) => {
                Some((first, Rows::of_index(end.index().min(bg.rows.index()))))
            }
        };
        let mut uploaded = Rows::ZERO;
        if let Some((first, end)) = band.filter(|(first, end)| first.index() < end.index()) {
            let cols = bg.cols.index();
            let rect = TexRect {
                x: Tex(0),
                y: Tex(u16::try_from(first.index()).ok()?),
                w: Tex(u16::try_from(cols).ok()?),
                h: Tex(u16::try_from(end.index() - first.index()).ok()?),
            };
            upload_rect(
                &bg.texture,
                rect,
                argb_bytes(renderer.background()),
                RowBytes(cols * 4),
                Texel::Bgra8,
            );
            uploaded = Rows::of_index(end.index() - first.index());
        }
        let bg_texture = bg.texture.clone();
        self.background_rows.add(Tally::of(uploaded.index()));
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
                self.bg_ground_pipe
                    .get(&self.device, &self.library, Pipe::BgGround)?,
                self.bg_set_pipe
                    .get(&self.device, &self.library, Pipe::BgSet)?,
            )),
            false => None,
        };
        let boxed = letterbox_whole(target, renderer.whole_size(), Parts::of(renderer));
        let (viewport, pad, strip_vp, line_vp, rows_vp) =
            (boxed.grid, boxed.pad, boxed.strip, boxed.line, boxed.rows);
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
        // Whether the trail's, the blink animation's or the cursor guide's
        // records leave the grid, where no row's band holds them: a guide
        // spans a legacy scroller's gutter too.
        let cell = (metrics.cell_w, metrics.cell_h);
        let grid = (cols, rows);
        let off_grid = renderer
            .trail()
            .iter()
            .chain(renderer.cursor_glow())
            .chain(renderer.cursor_guide())
            .any(|r| crate::redraw::record_rows(r, cell, grid).is_none());
        // What this frame draws beside its rows, read where the renderer
        // holds it and compared with the last frame's key in place: the
        // strip's records and the word wrap's marks as one run, as the key
        // keeps them. Nothing is collected to compare, and the key's own
        // buffers take this frame's records only when they differ.
        let (size, bits, clear_word) = (
            target.size(),
            uniforms.map(f32::to_bits),
            renderer.clear_color(),
        );
        let (strip, margin, lights_now) = (
            renderer.strip_records(),
            renderer.margin_records(),
            renderer.lights_records(),
        );
        let unrowed = match off_grid || !images.is_empty() {
            true => Unrowed::Yes,
            false => Unrowed::No,
        };
        let alike = match &self.last {
            Last::Instances(last) => {
                last.size == size
                    && last.uniforms == bits
                    && last.clear == clear_word
                    && last.strip.iter().eq(strip.iter().chain(margin))
                    && last.lights == lights_now
                    && last.unrowed == unrowed
            }
            _ => false,
        };
        let changed = match alike && unrowed == Unrowed::No {
            true => renderer.redraw(),
            false => Redraw::Whole,
        };
        let changed = self.sliding(changed);
        let drawn = missing.union(changed);
        let clips: Vec<Option<MTLScissorRect>> = match drawn {
            Redraw::Whole => vec![None],
            Redraw::Rows(set) => bands(target, pad, grid_px, metrics.cell_h, set)
                .into_iter()
                .map(|(_, _, r)| Some(r))
                .collect(),
        };
        let upload_time = t0.elapsed();
        let scratch = self.scratch_for(target);
        let (cmd, enc) =
            self.encoder(scratch.as_ref().unwrap_or(target), clear, viewport, &drawn)?;
        let ubytes = NonNull::from(&uniforms).cast::<c_void>();
        let ulen = core::mem::size_of::<[f32; UNIFORM_FLOATS]>();
        for clip in clips {
            self.clip = clip;
            if let Some(c) = clip {
                enc.setScissorRect(c);
            }
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
                enc.setScissorRect(clipped(scissor(target, pad, grid_px), self.clip));
                enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::TriangleStrip, 0, 4);
                enc.setScissorRect(clipped(
                    scissor(target, [0.0, 0.0], target.size()),
                    self.clip,
                ));
                // Images below the non-default backgrounds, then those
                // backgrounds blended over them, as the CPU's `paint_under`.
                if let Some((ground, _, pipe)) = &below {
                    self.draw_images(&enc, &images, staged.as_ref(), ImageLayer::Below, frame_px)?;
                    enc.setRenderPipelineState(pipe);
                    enc.setFragmentBytes_length_atIndex(ubytes, ulen, 0);
                    enc.setFragmentTexture_atIndex(Some(&bg_texture), 0);
                    enc.setFragmentTexture_atIndex(Some(ground), 1);
                    enc.setScissorRect(clipped(scissor(target, pad, grid_px), self.clip));
                    enc.drawPrimitives_vertexStart_vertexCount(
                        MTLPrimitiveType::TriangleStrip,
                        0,
                        4,
                    );
                    enc.setScissorRect(clipped(
                        scissor(target, [0.0, 0.0], target.size()),
                        self.clip,
                    ));
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
                // The strip is outside every band, and a partial frame keeps
                // what the target holds of it: the key says it did not change.
                if self.clip.is_none() {
                    self.draw_strip(
                        &enc,
                        &self.instances[slot],
                        (At(count), At(count + marks)),
                        (target, strip_vp, frame_px.1),
                        metrics,
                        (&mask, &color),
                    )?;
                    self.draw_strip(
                        &enc,
                        &self.instances[slot],
                        (At(count + marks), At(count + marks + lights)),
                        (target, line_vp, renderer.lights_height()),
                        metrics,
                        (&mask, &color),
                    )?;
                    let at = count + marks + lights;
                    self.draw_strip(
                        &enc,
                        &self.instances[slot],
                        (At(at), At(at + wraps)),
                        (target, rows_vp, frame_px.1),
                        metrics,
                        (&mask, &color),
                    )?;
                }
            }
        }
        self.clip = None;
        enc.endEncoding();
        self.compose_slide(&cmd, target, renderer.whole_size().1, Parts::of(renderer));
        let (submit, waited, flying) = self.commit(cmd, done);
        // What the next frame is compared with: this one, in the last
        // key's buffers, written only where it differs from that key.
        let mut key = match core::mem::replace(&mut self.last, Last::None) {
            Last::Instances(last) => last,
            _ => InstanceKey {
                size,
                uniforms: bits,
                clear: clear_word,
                strip: Vec::new(),
                lights: Vec::new(),
                unrowed,
            },
        };
        if !alike {
            key.size = size;
            key.uniforms = bits;
            key.clear = clear_word;
            key.strip.clear();
            key.strip.extend(strip.iter().chain(margin).copied());
            key.lights.clear();
            key.lights.extend_from_slice(lights_now);
            key.unrowed = unrowed;
        }
        self.last = Last::Instances(key);
        self.committed(&waited);
        renderer.take_redraw();
        Some(Drawn::Frame(GpuFrame {
            instances: Tally::of(count + marks + lights + wraps),
            upload: upload_time,
            submit,
            gpu: waited.landed().ok().map(|l| l.landing().gpu()),
            changed,
            drawn,
            waited,
            flying,
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
        self.draw_cells_since(renderer, input, target, done, Redraw::Whole)
    }

    /// [`Gpu::draw_cells`] into a target that holds an earlier frame and
    /// lacks `missing` of the last one committed ([`crate::redraw::Ages`]).
    /// A frame is compared with the last committed on everything it is
    /// handed and every uniform built from it, its grid by the screen the
    /// caller names ([`CellInput::screen`]): when only the cursor's
    /// words, the trail's and the blink animation's records (`moving`) and
    /// the grounds -- the cursor guide and the badge -- differ, its change
    /// is the rows of the old and the new cursor, each with the rows its
    /// glyph can reach ([`CellInput::reach`]), and the rows those records
    /// and the grounds that changed cover.
    /// Then the target is loaded and each band of changed rows drawn alone:
    /// the cell passes over its rows and the rows whose glyphs can reach
    /// it, every record, all held to its scissor.
    pub fn draw_cells_since(
        &mut self,
        renderer: &mut Renderer<'_, '_>,
        input: &CellInput<'_>,
        target: &Target,
        done: Done,
        missing: Redraw,
    ) -> Option<Drawn> {
        if self.in_flight() == InFlight::Busy {
            return Some(Drawn::Busy);
        }
        if let GpuUse::GaveUp(why) = self.gpu_use() {
            return Some(Drawn::GaveUp(why));
        }
        self.take_failures();
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
            true => Some(self.cell_bg_over_pipe.get(
                &self.device,
                &self.library,
                Pipe::CellBgOver,
            )?),
            false => None,
        };
        let boxed = letterbox_whole(target, renderer.whole_size(), Parts::of(renderer));
        let (viewport, pad, strip_vp, line_vp, rows_vp) =
            (boxed.grid, boxed.pad, boxed.strip, boxed.line, boxed.rows);
        let trail = self.trail_draw(renderer, pad);
        // A block the records draw over a Devanagari syllable's span is not
        // the shader's to draw on its cell.
        let kind = match (
            input.cursor_drawn,
            CursorKind::of(input.cursor, renderer.cursor_look()),
        ) {
            (baud_render::CursorDrawn::Records, CursorKind::Block) => CursorKind::None,
            (_, k) => k,
        };
        // The hollow box's pipeline, built the first frame one is drawn.
        let hollow = match kind {
            CursorKind::Hollow => Some(self.cell_ring_pipe.get(
                &self.device,
                &self.library,
                Pipe::CellRing,
            )?),
            _ => None,
        };
        // The cursor is kept by its logical cell and drawn where this
        // frame's order puts it, a bar on the side its character runs from
        // (`Renderer::bidi`, `cursor_px`'s rule).
        let orders = renderer.bidi();
        let (cursor_row, cursor_col) =
            input
                .cursor
                .map_or((Uni(u32::MAX), Uni(u32::MAX)), |(r, c, _)| {
                    (
                        Uni::of(Elems(r.index())),
                        Uni::of(Elems(orders.visual_col(r, c).index())),
                    )
                });
        let cursor_side = Uni(match input.cursor.map(|(r, c, _)| orders.side(r, c)) {
            Some(baud_render::CursorSide::Right) => 1,
            _ => 0,
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
            cursor_side,
            sel_bg: paint_word(hilite.bg, None, |(c, _)| c),
            sel_fg: paint_word(hilite.fg, None, |(c, _)| c),
            sel_bg_curve: Uni(curve_of(hilite.bg)),
            sel_fg_curve: Uni(curve_of(hilite.fg)),
        };
        // **What this frame changed of the last committed**, from what it
        // was handed: the grid's rows by the screen its caller names, and
        // the overlay read where the shader reads it.
        let ncols = cols.index();
        let (ring, stride) = (input.ring.index().max(1), input.stride.index());
        // The shader reads `ring * stride` cells of the grid: a grid shorter
        // than that is refused rather than read past its end.
        if input.grid.0.length()
            < ring
                .saturating_mul(stride)
                .saturating_mul(core::mem::size_of::<PackedCell>())
        {
            return None;
        }
        // SAFETY: the overlay holds at least `ncols * (draw_rows + 1)` bytes
        // or its length, as `overlay` lent it; nothing writes it while a
        // frame is being built into its set, which the caller's wheel keeps.
        let overlay_len = ncols
            .saturating_mul(draw_rows.saturating_add(1))
            .min(input.overlay.0.length());
        let overlay_bytes = unsafe { peek::<u8>(&input.overlay.0, Elems(overlay_len)) }?;
        let n = input.records.len();
        let (ma, mb) = (input.moving.0.min(n), input.moving.1.min(n));
        let (ma, mb) = (ma, mb.max(ma));
        let cell = (metrics.cell_w, metrics.cell_h);
        let grid_cells_rows = (cols, Rows::of_count(draw_rows));
        let moving = input.records[ma..mb]
            .iter()
            .try_fold(RowSet::NONE, |set, r| {
                crate::redraw::record_rows(r, cell, grid_cells_rows).map(|rs| set.union(rs))
            });
        // The word wrap's marks are kept with the strip's, as one run: a
        // change to either draws the frame whole. Both are compared where
        // the renderer holds them, and nothing is collected to compare.
        let (strip, margin) = (renderer.strip_records(), renderer.margin_records());
        let lights_now = renderer.lights_records();
        let unrowed = match moving.is_none() || !images.is_empty() || input.slide.is_some() {
            true => Unrowed::Yes,
            false => Unrowed::No,
        };
        let vp = (
            viewport.originX.to_bits(),
            viewport.originY.to_bits(),
            viewport.width.to_bits(),
            viewport.height.to_bits(),
        );
        let clear_word = renderer.clear_color();
        let atlas_gen = renderer.atlas().generation();
        let alike = |l: &CellLast| {
            l.size == target.size()
                && l.viewport == vp
                && l.uniforms.without_cursor() == uniforms.without_cursor()
                && l.clear == clear_word
                && l.strip.iter().eq(strip.iter().chain(margin))
                && l.lights == lights_now
                && l.unrowed == Unrowed::No
                && unrowed == Unrowed::No
                && l.stride == input.stride
                && l.rotation == input.rotation
                && l.ring == input.ring
                && l.under == input.under
                && l.chrome == input.chrome
                && l.table == input.table
                && l.map_gen == input.map_gen
                && l.atlas == atlas_gen
                && l.records.len() == n - (mb - ma)
                && l.records[..ma] == input.records[..ma]
                && l.records[ma..] == input.records[mb..]
                && l.overlay == overlay_bytes
                && l.screen == input.screen
        };
        // The rows a glyph can reach past its own, up and down.
        let ch = i64::from(metrics.cell_h.value().max(1));
        let base = i64::from(metrics.baseline.value());
        let past = |px: i64| usize::try_from((px.max(0) + ch - 1) / ch).unwrap_or(usize::MAX);
        let up = past(-(base + i64::from(input.reach.top.0)));
        let down = past(base + i64::from(input.reach.bottom.0) - ch);
        let cursor_rows = |at: Option<Row>| match at {
            Some(r) => Redraw::NOTHING.with_span(
                Row::from_index(r.index().saturating_sub(up)),
                Row::from_index(
                    r.index()
                        .saturating_add(down)
                        .saturating_add(1)
                        .min(draw_rows),
                ),
            ),
            None => Redraw::NOTHING,
        };
        // The grounds that changed, the cursor guide moved with the cursor's
        // row or the badge cut round another cell: the rows of every ground
        // from the first that differs to the last, in either frame, and the
        // whole when one of them reaches past the grid -- a guide across a
        // legacy scroller's gutter, which no band holds.
        let ground_rows = |l: &CellLast| {
            let (old, now) = (&l.grounds[..], input.grounds);
            let same = old.iter().zip(now).take_while(|(a, b)| a == b).count();
            let tail = old[same..]
                .iter()
                .rev()
                .zip(now[same..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            old[same..old.len() - tail]
                .iter()
                .chain(&now[same..now.len() - tail])
                .try_fold(RowSet::NONE, |set, r| {
                    crate::redraw::record_rows(r, cell, grid_cells_rows).map(|rs| set.union(rs))
                })
        };
        let changed = match (&self.last, moving) {
            (Last::Cells(l), Some(now)) if alike(l) => match ground_rows(l) {
                Some(grounds) => {
                    let cursor = match l.uniforms == uniforms {
                        true => Redraw::NOTHING,
                        false => cursor_rows(l.uniforms.cursor_at())
                            .union(cursor_rows(uniforms.cursor_at())),
                    };
                    Redraw::Rows(now.union(l.moving).union(grounds)).union(cursor)
                }
                None => Redraw::Whole,
            },
            _ => Redraw::Whole,
        };
        let changed = self.sliding(changed);
        let drawn = missing.union(changed);
        // **What the shape pass draws**: every cell's slots while a slot of
        // the table carries a decoration; else the cursor's slot alone when
        // the cursor is an underline or a bar -- every other instance would
        // find no shape bit and draw nothing; else nothing. The pass's
        // bindings stay, since the hollow cursor draws on them. The cursor's
        // instance is the cell this frame's order draws it in, the column
        // the shader tests (`cursor_col`), not its logical one.
        let shape_draw = match (input.table.decorations(), kind, input.cursor) {
            (Decorations::Present, _, _) => ShapeDraw::Every,
            (Decorations::Absent, CursorKind::Underline | CursorKind::Bar, Some((r, c, _)))
                if r.index() + slide_rows < draw_rows && c.index() < cols.index() =>
            {
                let v = renderer.bidi().visual_col(r, c);
                ShapeDraw::Cursor(
                    ((r.index() + slide_rows) * cols.index() + v.index()) * SHAPES_PER_CELL
                        + CURSOR_SHAPE_SLOT,
                )
            }
            (Decorations::Absent, _, _) => ShapeDraw::Nothing,
        };
        // **Whether a glyph reaches above its row**: with `up` zero no
        // entry the map holds starts above its row's top, and the pass of
        // glyphs' parts above their rows has nothing to draw.
        let above_draws = up > 0;
        let clips: Vec<Option<(usize, usize, MTLScissorRect)>> = match drawn {
            Redraw::Whole => vec![None],
            Redraw::Rows(set) => bands(target, pad, renderer.pixel_size(), metrics.cell_h, set)
                .into_iter()
                .map(|(r, k, rect)| {
                    // The cell passes over the band and the rows whose
                    // glyphs can reach it.
                    let from = r.index().saturating_sub(down);
                    let to = (r.index() + k.index()).saturating_add(up).min(draw_rows);
                    Some((from, to.max(from), rect))
                })
                .collect(),
        };
        let upload_time = t0.elapsed();
        let scratch = self.scratch_for(target);
        let (cmd, enc) =
            self.encoder(scratch.as_ref().unwrap_or(target), clear, viewport, &drawn)?;
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
            // own pass, the activity lights after those, in their line's,
            // and the word wrap's marks last, over the grid's rows across
            // the whole width.
            let marks = renderer.strip_records().len();
            let lights = renderer.lights_records().len();
            let wraps = renderer.margin_records().len();
            let mut trail_at = None;
            let mut records = None;
            if n + marks + lights + wraps > 0 {
                let bytes = (n + marks + lights + wraps).saturating_mul(Instance::BYTES);
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
                        .as_chunks_mut::<{ Instance::BYTES }>()
                        .0
                        .iter_mut()
                        .zip(
                            input
                                .grounds
                                .iter()
                                .chain(input.records)
                                .chain(renderer.strip_records())
                                .chain(renderer.lights_records())
                                .chain(renderer.margin_records()),
                        )
                        .enumerate()
                    {
                        if matches!(inst.kind, Kind::Quad | Kind::Band | Kind::Particle) {
                            trail_at = widen(trail_at, At(i));
                        }
                        chunk.copy_from_slice(&inst.to_bytes());
                    }
                }
                self.ring = Ring((slot + 1) % RING);
                records = Some(slot);
            }
            for clip in clips.iter().copied() {
                self.clip = clip.map(|(_, _, r)| r);
                if let Some((_, _, r)) = clip {
                    enc.setScissorRect(r);
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
                        self.draw_images(
                            &enc,
                            &images,
                            staged.as_ref(),
                            ImageLayer::Under,
                            frame_px,
                        )?;
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
                    match (k, shape_draw, clip) {
                        (2, ShapeDraw::Nothing, _) => {}
                        (3, _, _) if !above_draws => {}
                        // The cursor's line alone, held to the band by its
                        // scissor as the whole pass would be.
                        (2, ShapeDraw::Cursor(at), _) => {
                            enc.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                                MTLPrimitiveType::TriangleStrip,
                                0,
                                4,
                                1,
                                at,
                            );
                        }
                        // A band: the instances of its rows and of the rows that
                        // reach it, which are each row's run of the pass.
                        (_, _, Some((from, to, _))) => {
                            let per = instances / draw_rows.max(1);
                            enc.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                                MTLPrimitiveType::TriangleStrip,
                                0,
                                4,
                                per * (to - from),
                                per * from,
                            );
                        }
                        (_, _, None) => enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                            MTLPrimitiveType::TriangleStrip,
                            0,
                            4,
                            instances,
                        ),
                    }
                    // The hollow cursor over the decorations, on the bindings
                    // the decoration pass left: its four sides.
                    if let (2, Some(hollow)) = (k, &hollow) {
                        enc.setRenderPipelineState(hollow);
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
                    // Outside every band, and kept as the target holds it.
                    if clip.is_none() {
                        self.draw_strip(
                            &enc,
                            &self.instances[slot],
                            (At(n), At(n + marks)),
                            (target, strip_vp, frame_px.1),
                            metrics,
                            (&mask, &color),
                        )?;
                        self.draw_strip(
                            &enc,
                            &self.instances[slot],
                            (At(n + marks), At(n + marks + lights)),
                            (target, line_vp, renderer.lights_height()),
                            metrics,
                            (&mask, &color),
                        )?;
                        let at = n + marks + lights;
                        self.draw_strip(
                            &enc,
                            &self.instances[slot],
                            (At(at), At(at + wraps)),
                            (target, rows_vp, frame_px.1),
                            metrics,
                            (&mask, &color),
                        )?;
                    }
                }
            }
        }
        self.clip = None;
        enc.endEncoding();
        self.compose_slide(&cmd, target, renderer.whole_size().1, Parts::of(renderer));
        let (submit, waited, flying) = self.commit(cmd, done);
        // What the next frame is compared with: this one, its buffers kept
        // where a frame alike leaves them as they are.
        let mut last = match core::mem::replace(&mut self.last, Last::None) {
            Last::Cells(l) => l,
            _ => Box::new(CellLast {
                size: target.size(),
                viewport: vp,
                uniforms,
                clear: clear_word,
                strip: Vec::new(),
                lights: Vec::new(),
                unrowed,
                stride: input.stride,
                rotation: input.rotation,
                ring: input.ring,
                under: input.under,
                chrome: input.chrome,
                table: input.table,
                map_gen: input.map_gen,
                atlas: atlas_gen,
                records: Vec::new(),
                moving: RowSet::NONE,
                grounds: Vec::new(),
                screen: input.screen,
                overlay: Vec::new(),
            }),
        };
        if changed == Redraw::Whole {
            last.strip.clear();
            last.strip.extend(strip.iter().chain(margin).copied());
            last.lights.clear();
            last.lights.extend_from_slice(lights_now);
            last.records.clear();
            last.records.extend_from_slice(&input.records[..ma]);
            last.records.extend_from_slice(&input.records[mb..]);
            last.overlay.clear();
            last.overlay.extend_from_slice(overlay_bytes);
        }
        last.screen = input.screen;
        last.size = target.size();
        last.viewport = vp;
        last.uniforms = uniforms;
        last.clear = clear_word;
        last.unrowed = unrowed;
        last.stride = input.stride;
        last.rotation = input.rotation;
        last.ring = input.ring;
        last.under = input.under;
        last.chrome = input.chrome;
        last.table = input.table;
        last.map_gen = input.map_gen;
        last.atlas = atlas_gen;
        last.moving = moving.unwrap_or(RowSet::NONE);
        last.grounds.clear();
        last.grounds.extend_from_slice(input.grounds);
        self.last = Last::Cells(last);
        self.committed(&waited);
        Some(Drawn::Frame(GpuFrame {
            instances: Tally::of(count.saturating_add(
                input.grounds.len()
                    + input.records.len()
                    + renderer.strip_records().len()
                    + renderer.lights_records().len()
                    + renderer.margin_records().len(),
            )),
            upload: upload_time,
            submit,
            gpu: waited.landed().ok().map(|l| l.landing().gpu()),
            changed,
            drawn,
            waited,
            flying,
        }))
    }

    /// Reads a target back as `0xAARRGGBB` words, premultiplied, row-major
    /// at the target's width; `None` when `out` is too small. It takes the
    /// proof that the frame drawn into it was waited for and drew
    /// ([`GpuFrame::waited`]): a frame that failed leaves the target's
    /// content undefined, and one not waited for may still be drawing.
    pub fn read(&self, target: &Target, _drew: &Landed, out: &mut [u32]) -> Option<()> {
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

/// `r` held to `clip`: their overlap, empty where they do not meet.
fn clipped(r: MTLScissorRect, clip: Option<MTLScissorRect>) -> MTLScissorRect {
    let Some(c) = clip else {
        return r;
    };
    let x0 = r.x.max(c.x);
    let y0 = r.y.max(c.y);
    let x1 = (r.x + r.width).min(c.x + c.width).max(x0);
    let y1 = (r.y + r.height).min(c.y + c.height).max(y0);
    MTLScissorRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

/// The scissor of each run of `rows`, the grid's full width at `pad`, for
/// cells `cell_h` tall: at most [`BANDS`], the closest runs joined past it.
fn bands(
    target: &Target,
    pad: [f32; 2],
    grid: (Px, Px),
    cell_h: Px,
    rows: RowSet,
) -> Vec<(Row, Rows, MTLScissorRect)> {
    let mut runs: Vec<(Row, Rows)> = rows.runs().collect();
    while runs.len() > BANDS {
        // Join the two runs with the fewest rows between them.
        let Some(i) = (0..runs.len() - 1)
            .min_by_key(|&i| runs[i + 1].0.index() - (runs[i].0.index() + runs[i].1.index()))
        else {
            break;
        };
        let (a, b) = (runs[i], runs[i + 1]);
        runs[i] = (a.0, Rows::of_index(b.0.index() + b.1.index() - a.0.index()));
        runs.remove(i + 1);
    }
    let ch = cell_h.value();
    let at = |r: usize| u32::try_from(r).unwrap_or(u32::MAX).saturating_mul(ch);
    runs.into_iter()
        .map(|(r, n)| {
            let top = pad[1] + at(r.index()) as f32;
            let rect = scissor(target, [pad[0], top], (grid.0, Px::of(at(n.index()))));
            (r, n, rect)
        })
        .collect()
}

/// The most bands a partial frame draws, each the whole pass again.
const BANDS: usize = 4;

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

/// **Where the parts of the whole frame are**, in its pixels: the
/// exit-status strip's width at its left, the top and height of the grid's
/// frame -- the grid, the status line and the notice -- and the top and
/// height of the activity lights' line above or below it
/// ([`baud_render::Renderer::whole_size`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Parts {
    margin: Px,
    strip: Px,
    top: Px,
    rows: Px,
    line_top: Px,
    line: Px,
}

impl Parts {
    fn of(r: &Renderer<'_, '_>) -> Parts {
        Parts {
            margin: r.margins(),
            strip: r.strip(),
            top: r.grid_top(),
            rows: r.frame_size().1,
            line_top: r.lights_top(),
            line: r.lights_height(),
        }
    }
}

/// **The whole frame centred**: [`letterbox`] of the margins, the
/// exit-status strip, the grid and the gutter, and the lights' line, then
/// the grid's viewport and pad `parts.margin` and `parts.strip` pixels in
/// from the whole's left and `parts.top` down from its top, the strip's
/// own viewport past the left margin over the grid's rows, the line's
/// across the whole width at its rows, and the grid's rows across the
/// whole width, where the margins' marks are drawn
/// ([`baud_render::Renderer::margin_records`]). With no margin, strip or
/// line, the grid's are [`letterbox`]'s.
fn letterbox_whole(target: &Target, whole: (Px, Px), parts: Parts) -> Letterboxed {
    let (vp, _) = letterbox(target, whole);
    let px = |p: Px| f64::from(p.value());
    let (m, s) = (px(parts.margin), px(parts.strip));
    let rows = MTLViewport {
        originY: vp.originY + px(parts.top),
        height: px(parts.rows),
        ..vp
    };
    let grid = MTLViewport {
        originX: rows.originX + m + s,
        width: (rows.width - 2.0 * m - s).max(0.0),
        ..rows
    };
    let own = MTLViewport {
        originX: rows.originX + m,
        width: s,
        ..rows
    };
    let line = MTLViewport {
        originY: vp.originY + px(parts.line_top),
        height: px(parts.line),
        ..vp
    };
    Letterboxed {
        grid,
        pad: [grid.originX as f32, grid.originY as f32],
        strip: own,
        line,
        rows,
    }
}

/// [`letterbox_whole`]'s viewports: the grid's and its pad, the strip's,
/// the lights' line's, and the grid's rows across the whole width.
struct Letterboxed {
    grid: MTLViewport,
    pad: [f32; 2],
    strip: MTLViewport,
    line: MTLViewport,
    rows: MTLViewport,
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
