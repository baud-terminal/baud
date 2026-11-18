//! **The cell path**: the grid bound as it is stored and every cell resolved
//! in the vertex stage, a frame drawn in four instanced passes over the
//! cells -- backgrounds, the glyphs inside their rows, the shape slots, the
//! glyphs' parts above their rows -- with the overlay's records, the grounds
//! and baud's own overlays drawn between them by the instance path's
//! pipeline, and the hollow cursor after the shapes.
//!
//! [`CellMemory`] is each buffer set's memory, made once: the window's copy
//! (`PackedCell`), the resolved renditions, the overlay a byte a cell, the
//! frame's uniform `CU`, each draw row's first rendition slot, and the
//! records and their uniforms; the glyph map is two buffers shared by the
//! sets, one live, swapped on a rescale. Every descriptor set is made with
//! it and rewritten only where a buffer behind it is replaced, so a frame
//! allocates no set, pool, buffer or sampler.
//!
//! The memory is lent on the arm [`Gpu::lend`] chooses; under staging a
//! frame owes the ranges the shader reads, and the glyph map's entries that
//! differ from those the device buffer was last given.

use crate::chrome::{Chrome, View, scissor};
use crate::damage::{self, BANDS, Stale, TARGETS, TargetIx};
use crate::draw::Buffer;
use crate::frame::{Failed, Gpu, Recorder, Target};
use crate::lend::{Arm, Elems, Lent, Uploaded};
use crate::pipeline::{Blend, Fragment, Fx, Pipelines, Skipped};
use crate::sheets::Sheets;
use ash::vk;
use baud_domain::themefile::CellPaint;
use baud_domain::{Col, Cols, CursorStyle, PackedCell, Row, RowRot, Rows, Stride};
use baud_font::Px as FontPx;
use baud_present_wayland::{Changed, Px, Strip as Damaged};
use baud_render::cell_frame::{CU_NAMES, CU_WORDS};
use baud_render::screens::{SETS, ScreenGen, SetIx};
use baud_render::{
    Argb, CursorDrawn, CursorLook, CursorSide, Decorations, Generation, Highlight, Instance, Kind,
    MAP_BITS, MAP_SLOTS, MapEntry, MapGeneration, Reach, Redraw, Region, Renderer, Resolved,
    RowSet, StyleStamp, Tally,
};

/// The shape pass's slots a cell, `SHAPE_SLOTS` in `cells.glsl`.
pub const SHAPES_PER_CELL: usize = 27;
/// The cursor's slot among them, `CURSOR_SLOT`.
pub const CURSOR_SHAPE_SLOT: usize = 3;

/// One word of `CU`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct Word(pub u32);

/// **`CU`'s 38 words**, word `i` the one `CU_NAMES[i]` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cu(pub [Word; CU_WORDS]);

impl Cu {
    /// The word `name` names, if `CU_NAMES` has it.
    pub fn get(&self, name: &str) -> Option<Word> {
        CU_NAMES
            .iter()
            .position(|n| *n == name)
            .and_then(|i| self.0.get(i).copied())
    }

    /// The words with the cursor's own set to zero: two frames alike in
    /// this differ only in where and how the cursor is drawn.
    fn without_cursor(self) -> Cu {
        let mut bare = self;
        for (w, name) in bare.0.iter_mut().zip(CU_NAMES) {
            if CURSOR_WORDS.contains(&name) {
                *w = Word(0);
            }
        }
        bare
    }

    /// The row the shader draws a cursor on, when it draws one.
    fn cursor_at(&self) -> Option<Row> {
        let word = |name: &str| self.get(name).unwrap_or(Word(0));
        let drawn =
            word("cursor_kind") != CursorKind::None.code() && word("cursor_alpha") != Word(0);
        drawn.then(|| Row::from_index(usize::try_from(word("cursor.y").0).unwrap_or(usize::MAX)))
    }
}

/// `CU`'s words that say where and how the cursor is drawn.
const CURSOR_WORDS: [&str; 7] = [
    "cursor.x",
    "cursor.y",
    "cursor_kind",
    "cursor_color",
    "cursor_alpha",
    "cursor_text",
    "cursor_text_curve",
];

/// Which of the two glyph map buffers is live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapSide {
    First,
    Second,
}

impl MapSide {
    /// Its place in a pair.
    fn of<T>(self, pair: &mut [T; 2]) -> &mut T {
        let [first, second] = pair;
        match self {
            MapSide::First => first,
            MapSide::Second => second,
        }
    }
}

/// **What the cell memory is sized for**: the provisioned grid, the pitch
/// the core lays its rows at, and the slots of the core's rendition table.
#[derive(Clone, Copy, Debug)]
pub struct CellSizes {
    pub cols: Cols,
    pub rows: Rows,
    pub stride: Stride,
    pub styles: Elems,
}

/// **The slices `baud_render::cell_frame::build` writes**, one of each a
/// set, and the two glyph maps' entries, each taken once for
/// `GlyphMap::over`.
pub struct CellSlices {
    pub copies: Vec<&'static mut [PackedCell]>,
    pub styles: Vec<&'static mut [Resolved]>,
    pub overlay: Vec<&'static mut [u8]>,
    pub maps: [Option<&'static mut [MapEntry]>; 2],
}

/// A host-visible buffer and its mapping.
struct Mapped {
    buffer: Buffer,
    bytes: &'static mut [u8],
}

/// How many uniforms an instance draw of the cell path binds: the grid's,
/// and the exit-status strip's, the lights' line's and the grid's rows'.
const INSTANCE_US: usize = 4;
/// The bytes between them, the most `minUniformBufferOffsetAlignment` is.
const U_PITCH: usize = 256;
/// `U`'s bytes, twelve floats.
const U_BYTES: usize = 48;
/// The effect trail's uniform's bytes: `EffectU`'s 36 floats, which the
/// fireworks' `FwU` fits inside.
const FX_U_BYTES: usize = 144;
/// The fewest records a set's buffer holds.
const RECORDS_FLOOR: usize = 256;

/// One buffer set.
struct CellSet {
    grid: Lent<PackedCell>,
    grid_len: Elems,
    styles: Lent<Resolved>,
    styles_len: Elems,
    overlay: Lent<u8>,
    overlay_len: Elems,
    cu: Lent<u32>,
    cu_mem: &'static mut [u32],
    seg_base: Lent<u32>,
    seg_len: Elems,
    /// Set 0 of the cell layout.
    set: vk::DescriptorSet,
    /// Set 0 of the instance layout, one a uniform slot.
    inst: [vk::DescriptorSet; INSTANCE_US],
    records: Mapped,
    us: Mapped,
    /// The effect trail's uniform, `EffectU` or the fireworks' `FwU`, and
    /// set 2 of the instance layout over it.
    fx_u: Mapped,
    fx_set: vk::DescriptorSet,
    map_bound: Option<MapSide>,
    last: Cu,
    /// Under staging, what the device's copies hold: the screen its grid
    /// was last given, the table its renditions were, whether its band
    /// bases have been, and the overlay as it was last given (empty before
    /// the first frame).
    holds: Option<ScreenGen>,
    styled: Option<StyleStamp>,
    based: Given,
    overlay_given: Vec<u8>,
}

/// Whether a staged buffer that never changes has been given its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Given {
    No,
    Yes,
}

/// **The cell path's memory**: every set's, the two glyph maps, and the
/// descriptor sets over them.
pub struct CellMemory {
    arm: Arm,
    sets: Vec<CellSet>,
    maps: [Lent<MapEntry>; 2],
    /// Under staging, each map's entries as its device buffer was last
    /// given them: none before the first copy.
    given: [Vec<Option<MapEntry>>; 2],
    pool: vk::DescriptorPool,
    /// Set 1 of both layouts: the sheets and the curves.
    atlas: vk::DescriptorSet,
    atlas_bound: Option<(vk::ImageView, vk::ImageView, vk::Buffer)>,
    /// Images on screen that a frame left undrawn: the cell path does not
    /// draw images yet.
    pub images_skipped: Skipped,
    /// Effect-trail records (`Kind::Quad`, `Band`, `Particle`) a frame
    /// left undrawn: their own pipelines are not the cell path's yet.
    pub trail_skipped: Skipped,
    /// The command buffer and fence every frame is recorded and waited
    /// through, made once.
    recorder: Recorder,
    /// Each target drawn into, its view and framebuffer, made the first
    /// frame it is drawn and kept until [`CellMemory::forget`].
    targets: Vec<Held>,
    /// What each target lacks of the frame drawn last.
    stale: Stale,
    /// What the frame drawn last was handed, for the next to be compared
    /// with; none before the first.
    last: Option<Box<CellLast>>,
    /// The bands of a partial frame, kept for their capacity.
    bands: Vec<(Row, Rows)>,
    /// The rows of the target the frame drawn last drew, and their pixel
    /// strips when it drew rows.
    shown: Redraw,
    strips: Vec<Damaged>,
}

/// A target's image, the view and framebuffer a frame draws it through,
/// and its place among the targets whose stale rows are kept: none past
/// the [`TARGETS`], which is drawn whole.
struct Held {
    image: vk::Image,
    view: vk::ImageView,
    framebuffer: vk::Framebuffer,
    ix: Option<TargetIx>,
}

/// Whether a frame drew something no grid row holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unrowed {
    No,
    Yes,
}

/// **What a cell frame drew**: everything it was handed and the uniforms
/// built from it, for the next frame to be compared with.
struct CellLast {
    size: (Px, Px),
    view: View,
    cu: Cu,
    clear: Argb,
    /// The exit-status marks and the word wrap's marks, as one run.
    strip: Vec<Instance>,
    lights: Vec<Instance>,
    unrowed: Unrowed,
    stride: Stride,
    rotation: RowRot,
    ring: Rows,
    under: Tally,
    chrome: Tally,
    table: StyleStamp,
    map_gen: MapGeneration,
    atlas: Generation,
    /// The records outside the moving ones, and the rows the moving ones
    /// covered.
    records: Vec<Instance>,
    moving: RowSet,
    /// The grounds, which move with the cursor.
    grounds: Vec<Instance>,
    screen: ScreenGen,
    overlay: Vec<u8>,
}

/// **What [`Gpu::draw_cells`] drew**: what the frame changed of the last
/// drawn, and the rows it drew into its target -- those and the rows the
/// target lacked -- each the whole or rows of the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellDrawn {
    pub changed: Redraw,
    pub drawn: Redraw,
}

/// Where a pass of a frame draws: the whole target, or a band of rows under
/// its scissor, with the rows whose glyphs reach it.
#[derive(Clone, Copy)]
enum Clip {
    Whole,
    Band {
        from: Elems,
        to: Elems,
        rect: vk::Rect2D,
    },
}

/// **What a cell frame draws from**, beyond its set's memory: what the app
/// hands Metal's cell path, from `cell_frame::build`'s frame.
pub struct CellDraw<'a> {
    pub set: SetIx,
    pub stride: Stride,
    pub rotation: RowRot,
    /// The rows the shader reads modulo: the copy's.
    pub ring: Rows,
    pub cursor: Option<(Row, Col, CursorStyle)>,
    pub cursor_drawn: CursorDrawn,
    pub highlight: Option<Highlight>,
    /// What draws the cells the overlay masks, and after them the trail,
    /// the glow and baud's own overlays.
    pub records: &'a [Instance],
    /// What lies over the backgrounds and under the glyphs.
    pub grounds: &'a [Instance],
    /// How many of `records`, from the first, lie inside their own rows.
    pub under: Tally,
    /// How many of `records`, at the end, are baud's own overlays.
    pub chrome: Tally,
    /// Smooth scroll's pan; none on Linux, which draws no leaving row.
    pub slide: Option<FontPx>,
    pub table: StyleStamp,
    pub reach: Reach,
    /// The live glyph map's entries and which buffer they are.
    pub map: (&'a [MapEntry], MapSide),
    /// The screen the set's copy holds (`Screens::now`): a staged frame
    /// whose set's device copy holds it already uploads no cell of it.
    pub screen: ScreenGen,
    /// The set's overlay as the frame wrote it, through the slice it was
    /// lent.
    pub overlay: &'a [u8],
    /// The records from `moving.0` to `moving.1` move with the cursor or
    /// the clock -- the trail, the blink animation -- and are compared by
    /// the rows they cover (`cell_frame::CellFrame::moving`).
    pub moving: (Elems, Elems),
    /// The glyph map's generation: a frame read through other entries
    /// draws whole.
    pub map_gen: MapGeneration,
}

/// How the cursor is drawn, as the shaders switch on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CursorKind {
    None,
    Block,
    Underline,
    Bar,
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

    fn code(self) -> Word {
        Word(match self {
            CursorKind::None => 0,
            CursorKind::Block => 1,
            CursorKind::Underline => 2,
            CursorKind::Bar => 3,
            CursorKind::Hollow => 4,
        })
    }

    /// The kind the shader draws: none for a block the records draw.
    fn drawn(renderer: &Renderer<'_, '_>, input: &CellDraw<'_>) -> CursorKind {
        match (
            input.cursor_drawn,
            CursorKind::of(input.cursor, renderer.cursor_look()),
        ) {
            (CursorDrawn::Records, CursorKind::Block) => CursorKind::None,
            (_, k) => k,
        }
    }
}

/// The shader's word for the cell's own text, and for its page.
const PAINT_TEXT: u32 = 1;
const PAINT_PAGE: u32 = 2;

/// A paint as the shader's word: 0 where it is `none`, the paint the shader
/// takes for 0, else the cell's own by name or the colour, opaque.
fn paint_word<T: Copy>(
    p: Option<CellPaint<T>>,
    none: Option<CellPaint<()>>,
    colour: impl Fn(T) -> Argb,
) -> Word {
    let unit = p.map(|p| p.map(|_| ()));
    Word(match p {
        _ if unit == none => 0,
        None => 0,
        Some(CellPaint::Text) => PAINT_TEXT,
        Some(CellPaint::Page) => PAINT_PAGE,
        Some(CellPaint::Colour(c)) => colour(c).0,
    })
}

/// A count as a word, saturating.
fn count(n: Elems) -> Word {
    Word(u32::try_from(n.0).unwrap_or(u32::MAX))
}

/// **`CU` for a frame**, from the renderer and what the frame draws from,
/// each word what the macOS cell path's `CellUniforms` takes. The sides are
/// the atlas's, which the sheets are brought up to before a frame reads them.
pub fn cell_uniforms(renderer: &Renderer<'_, '_>, input: &CellDraw<'_>) -> Cu {
    let (cols, rows) = renderer.grid();
    let metrics = renderer.metrics();
    let frame_px = renderer.frame_size();
    let atlas = renderer.atlas();
    let kind = CursorKind::drawn(renderer, input);
    let orders = renderer.bidi();
    let (cursor_row, cursor_col) =
        input
            .cursor
            .map_or((Word(u32::MAX), Word(u32::MAX)), |(r, c, _)| {
                (
                    count(Elems(r.index())),
                    count(Elems(orders.visual_col(r, c).index())),
                )
            });
    let cursor_side = Word(match input.cursor.map(|(r, c, _)| orders.side(r, c)) {
        Some(CursorSide::Right) => 1,
        _ => 0,
    });
    let hilite = renderer.hilite();
    let block = renderer.block_paint();
    let curve_of = |p: Option<CellPaint<(Argb, baud_domain::contrast::CurveRow)>>| match p {
        Some(CellPaint::Colour((_, row))) => Word(u32::from(row.get())),
        _ => Word(0),
    };
    let px = |p: FontPx| Word(p.value());
    let at = |f: fn(&Highlight) -> usize| count(Elems(input.highlight.as_ref().map_or(0, f)));
    let slide_rows = usize::from(input.slide.is_some());
    Cu([
        // viewport
        px(frame_px.0),
        px(frame_px.1),
        // cell
        px(metrics.cell_w),
        px(metrics.cell_h),
        // grid
        count(Elems(cols.index())),
        count(Elems(rows.index())),
        // sides
        Word(atlas.side().value()),
        Word(atlas.color_side().value()),
        // cursor
        cursor_col,
        cursor_row,
        kind.code(),
        px(metrics.baseline),
        px(metrics.underline),
        px(metrics.strike),
        px(metrics.thickness),
        count(Elems(MAP_SLOTS)),
        Word(32 - MAP_BITS),
        count(Elems(input.stride.index())),
        count(Elems(input.rotation.index())),
        count(Elems(input.ring.index())),
        // sel_on: 0 none, 1 a stream, 2 a block.
        Word(match input.highlight.map(|h| h.region) {
            None => 0,
            Some(Region::Stream) => 1,
            Some(Region::Block) => 2,
        }),
        at(|h| h.from.1.index()),
        at(|h| h.from.0.index()),
        at(|h| h.to.1.index()),
        at(|h| h.to.0.index()),
        // pan, slide_rows
        px(input.slide.unwrap_or(FontPx::of(0))),
        count(Elems(slide_rows)),
        // cursor_color: none where it is the cell's text.
        paint_word(block.fill, Some(CellPaint::Text), |c| c),
        Word(renderer.blink().hidden().0),
        Word(u32::from(input.cursor.map_or(0xFF, |(_, _, style)| {
            renderer.cursor_opacity(style).get()
        }))),
        paint_word(hilite.bg, None, |(c, _)| c),
        paint_word(hilite.fg, None, |(c, _)| c),
        curve_of(hilite.bg),
        curve_of(hilite.fg),
        // cursor_text: none where it is the cell's page.
        paint_word(block.text, Some(CellPaint::Page), |(c, _)| c),
        curve_of(block.text),
        Word(renderer.blink().faint().0),
        cursor_side,
    ])
}

impl Gpu {
    /// A host-visible buffer of `bytes`, mapped.
    fn mapped(&self, bytes: Elems, usage: vk::BufferUsageFlags) -> Result<Mapped, Failed> {
        let (buffer, bytes) = self.host_mapped(bytes, usage)?;
        Ok(Mapped { buffer, bytes })
    }
}

impl CellMemory {
    /// **Every set's memory on `arm`**, sized as the app sizes the macOS
    /// cell path's -- the copy at the provisioned rows and one more at the
    /// core's pitch, the renditions at the table's slots, the overlay a byte
    /// a cell of those rows in whole words, `CU`'s 38 words, a first slot a
    /// draw row -- the two glyph maps of `MAP_SLOTS`, and the descriptor
    /// sets over them; and the slices the frames are built through.
    #[allow(unsafe_code)]
    pub fn new(
        gpu: &Gpu,
        pipes: &Pipelines,
        arm: Arm,
        sizes: CellSizes,
    ) -> Result<(CellMemory, CellSlices), Failed> {
        let d = &gpu.device;
        let storage = vk::BufferUsageFlags::STORAGE_BUFFER;
        let per_set = 1 + INSTANCE_US;
        let n_sets = u32::try_from(SETS * (per_set + 1) + 1).unwrap_or(u32::MAX);
        let n_storage = u32::try_from(SETS * (5 + INSTANCE_US) + 1).unwrap_or(u32::MAX);
        let n_uniform = u32::try_from(SETS * (2 + INSTANCE_US)).unwrap_or(u32::MAX);
        let pool_sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(n_storage),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(n_uniform),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(3),
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(n_sets)
            .pool_sizes(&pool_sizes);
        // SAFETY: a device of `gpu`'s.
        let pool = unsafe { d.create_descriptor_pool(&pool_info, None) }?;
        let atlas_layout = [pipes.sets[1]];
        let alloc = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&atlas_layout);
        // SAFETY: the pool holds it.
        let atlas = unsafe { d.allocate_descriptor_sets(&alloc) }?[0];
        let maps = [
            gpu.lend::<MapEntry>(arm, Elems(MAP_SLOTS), storage)?,
            gpu.lend::<MapEntry>(arm, Elems(MAP_SLOTS), storage)?,
        ];
        let [(map0, map0_mem), (map1, map1_mem)] = maps;
        let mut slices = CellSlices {
            copies: Vec::with_capacity(SETS),
            styles: Vec::with_capacity(SETS),
            overlay: Vec::with_capacity(SETS),
            maps: [Some(map0_mem), Some(map1_mem)],
        };
        let rows1 = sizes.rows.index().saturating_add(1);
        let grid_len = Elems(rows1.saturating_mul(sizes.stride.index()));
        let overlay_bytes = sizes.cols.index().saturating_mul(rows1);
        let overlay_len = Elems(overlay_bytes.div_ceil(4).saturating_mul(4).max(4));
        let mut sets = Vec::with_capacity(SETS);
        for _ in 0..SETS {
            let (grid, copy) = gpu.lend::<PackedCell>(arm, grid_len, storage)?;
            copy.fill(PackedCell::EMPTY);
            let (styles, styles_mem) = gpu.lend::<Resolved>(arm, sizes.styles, storage)?;
            styles_mem.fill(Resolved::default());
            let (overlay, overlay_mem) = gpu.lend::<u8>(arm, overlay_len, storage)?;
            let (cu, cu_mem) =
                gpu.lend::<u32>(arm, Elems(CU_WORDS), vk::BufferUsageFlags::UNIFORM_BUFFER)?;
            let seg_len = Elems(rows1);
            let (seg_base, _zero) = gpu.lend::<u32>(arm, seg_len, storage)?;
            let layouts = [
                pipes.cell_set,
                pipes.sets[0],
                pipes.sets[0],
                pipes.sets[0],
                pipes.sets[0],
                pipes.sets[2],
            ];
            let alloc = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(pool)
                .set_layouts(&layouts);
            // SAFETY: the pool holds them.
            let made = unsafe { d.allocate_descriptor_sets(&alloc) }?;
            let records = gpu.mapped(Elems(RECORDS_FLOOR * Instance::BYTES), storage)?;
            let us = gpu.mapped(
                Elems(INSTANCE_US * U_PITCH),
                vk::BufferUsageFlags::UNIFORM_BUFFER,
            )?;
            let fx_u = gpu.mapped(Elems(FX_U_BYTES), vk::BufferUsageFlags::UNIFORM_BUFFER)?;
            let set = CellSet {
                grid,
                grid_len,
                styles,
                styles_len: sizes.styles,
                overlay,
                overlay_len,
                cu,
                cu_mem,
                seg_base,
                seg_len,
                set: made[0],
                inst: [made[1], made[2], made[3], made[4]],
                records,
                us,
                fx_u,
                fx_set: made[5],
                map_bound: None,
                last: Cu([Word(0); CU_WORDS]),
                holds: None,
                styled: None,
                based: Given::No,
                overlay_given: Vec::new(),
            };
            set.write_own(d);
            set.write_records(d);
            sets.push(set);
            slices.copies.push(copy);
            slices.styles.push(styles_mem);
            slices.overlay.push(&mut overlay_mem[..overlay_bytes]);
        }
        let recorder = gpu.recorder()?;
        Ok((
            CellMemory {
                arm,
                sets,
                maps: [map0, map1],
                given: [vec![None; MAP_SLOTS], vec![None; MAP_SLOTS]],
                pool,
                atlas,
                atlas_bound: None,
                images_skipped: Skipped::default(),
                trail_skipped: Skipped::default(),
                recorder,
                targets: Vec::with_capacity(TARGETS),
                stale: Stale::new(),
                last: None,
                bands: Vec::with_capacity(baud_render::REDRAW_ROWS),
                shown: Redraw::Whole,
                strips: Vec::with_capacity(BANDS),
            },
            slices,
        ))
    }

    /// **Every byte frames have copied from staging** into the cell path's
    /// buffers: grids, renditions, overlays, `CU`, band bases and maps.
    pub fn uploaded(&self) -> Uploaded {
        let sets: u64 = self
            .sets
            .iter()
            .map(|s| {
                s.grid.uploaded().0
                    + s.styles.uploaded().0
                    + s.overlay.uploaded().0
                    + s.cu.uploaded().0
                    + s.seg_base.uploaded().0
            })
            .sum();
        Uploaded(sets + self.maps.iter().map(|m| m.uploaded().0).sum::<u64>())
    }

    /// The arm the memory is on.
    pub fn arm(&self) -> Arm {
        self.arm
    }

    /// The `CU` the last frame drawn from `set` was given.
    pub fn uniforms(&self, set: SetIx) -> Cu {
        self.sets
            .get(set.0)
            .map_or(Cu([Word(0); CU_WORDS]), |s| s.last)
    }

    /// Every buffer and the pool freed. The caller has waited for the
    /// device to be idle and drops the slices first.
    #[allow(unsafe_code)]
    pub fn destroy(self, gpu: &Gpu) {
        for s in self.sets {
            s.grid.destroy(gpu);
            s.styles.destroy(gpu);
            s.overlay.destroy(gpu);
            s.cu.destroy(gpu);
            s.seg_base.destroy(gpu);
            gpu.drop_buffer(s.records.buffer);
            gpu.drop_buffer(s.us.buffer);
            gpu.drop_buffer(s.fx_u.buffer);
        }
        for m in self.maps {
            m.destroy(gpu);
        }
        gpu.drop_recorder(self.recorder);
        // SAFETY: the device is idle; the sets go with the pool, and no
        // frame draws through a target's view or framebuffer again.
        unsafe {
            for h in self.targets {
                gpu.device.destroy_framebuffer(h.framebuffer, None);
                gpu.device.destroy_image_view(h.view, None);
            }
            gpu.device.destroy_descriptor_pool(self.pool, None);
        }
    }

    /// **A target no frame will draw into again**, its view and framebuffer
    /// destroyed: called before the target is. Every frame is waited for, so
    /// none uses them.
    #[allow(unsafe_code)]
    pub fn forget(&mut self, gpu: &Gpu, t: &Target) {
        if let Some(at) = self.targets.iter().position(|h| h.image == t.image) {
            let h = self.targets.swap_remove(at);
            if let Some(k) = h.ix {
                self.stale.forget(k);
            }
            // SAFETY: as above.
            unsafe {
                gpu.device.destroy_framebuffer(h.framebuffer, None);
                gpu.device.destroy_image_view(h.view, None);
            }
        }
    }

    /// **The compositor did not take the frame drawn into `t`**
    /// (`DmabufShown` other than `Yes`): the rows it drew stay owed, and
    /// the next frame into `t` draws them again and names them in its
    /// [`CellMemory::damage`].
    pub fn refused(&mut self, t: &Target) {
        if let Some(k) = self
            .targets
            .iter()
            .find(|h| h.image == t.image)
            .and_then(|h| h.ix)
        {
            self.stale.refused(k);
        }
    }

    /// **A frame drawn on the CPU arm**, which changed `changed` of the
    /// frame before it: neither target holds it, so both lack those rows,
    /// and the next GPU frame into each draws them.
    pub fn drawn_elsewhere(&mut self, changed: Redraw) {
        self.stale.elsewhere(changed);
    }

    /// **What the frame drawn last changed of the frame the compositor
    /// shows, for `present_dmabuf`**: the whole, or the strips of pixel
    /// rows it drew, its target's stale rows among them.
    pub fn damage(&self) -> Changed<'_> {
        match self.shown {
            Redraw::Whole => Changed::Whole,
            Redraw::Rows(_) => Changed::Strips(&self.strips),
        }
    }

    /// **The framebuffer `t` is drawn through**, made the first time and
    /// kept, and its place among the targets: a frame into a target drawn
    /// before makes nothing. A target new to the memory holds no frame.
    #[allow(unsafe_code)]
    fn framebuffer(
        &mut self,
        d: &ash::Device,
        pass: vk::RenderPass,
        t: &Target,
    ) -> Result<(vk::Framebuffer, Option<TargetIx>), Failed> {
        if let Some(h) = self.targets.iter().find(|h| h.image == t.image) {
            return Ok((h.framebuffer, h.ix));
        }
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let view_info = vk::ImageViewCreateInfo::default()
            .image(t.image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(vk::Format::B8G8R8A8_UNORM)
            .subresource_range(range);
        // SAFETY: the target's image, alive until `forget`.
        let view = unsafe { d.create_image_view(&view_info, None) }?;
        let attachments = [view];
        let fb_info = vk::FramebufferCreateInfo::default()
            .render_pass(pass)
            .attachments(&attachments)
            .width(t.width.0)
            .height(t.height.0)
            .layers(1);
        // SAFETY: as above.
        let framebuffer = match unsafe { d.create_framebuffer(&fb_info, None) } {
            Ok(f) => f,
            Err(e) => {
                // SAFETY: unused.
                unsafe { d.destroy_image_view(view, None) };
                return Err(e.into());
            }
        };
        let ix = (0..TARGETS)
            .map(TargetIx)
            .find(|k| self.targets.iter().all(|h| h.ix != Some(*k)));
        if let Some(k) = ix {
            self.stale.forget(k);
        }
        self.targets.push(Held {
            image: t.image,
            view,
            framebuffer,
            ix,
        });
        Ok((framebuffer, ix))
    }

    /// Set 1 written over the sheets' images and curves where they are not
    /// the ones it holds: a sheet the atlas replaced is a new image.
    #[allow(unsafe_code)]
    fn bind_atlas(&mut self, d: &ash::Device, sheets: &Sheets) {
        let now = sheets.bound();
        if self.atlas_bound == Some(now) {
            return;
        }
        let (mask, color, curves) = now;
        let image = |view: vk::ImageView| {
            [vk::DescriptorImageInfo::default()
                .sampler(sheets.sampler)
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
        };
        let (mask_info, color_info) = (image(mask), image(color));
        let curves_info = [vk::DescriptorBufferInfo::default()
            .buffer(curves)
            .range(vk::WHOLE_SIZE)];
        let sampled = vk::DescriptorType::COMBINED_IMAGE_SAMPLER;
        let at = |binding: u32, kind: vk::DescriptorType| {
            vk::WriteDescriptorSet::default()
                .dst_set(self.atlas)
                .dst_binding(binding)
                .descriptor_type(kind)
        };
        // Binding 3, the instance path's backgrounds, no fragment stage of
        // the cell path reads: it names the mask, so the set is whole.
        let writes = [
            at(0, sampled).image_info(&mask_info),
            at(1, sampled).image_info(&color_info),
            at(2, vk::DescriptorType::STORAGE_BUFFER).buffer_info(&curves_info),
            at(3, sampled).image_info(&mask_info),
        ];
        // SAFETY: no frame in flight reads the set: every frame is waited
        // for; every view and buffer named is alive.
        unsafe { d.update_descriptor_sets(&writes, &[]) };
        self.atlas_bound = Some(now);
    }

    /// Binding 2 of `set`'s set 0 written over the live map, where it names
    /// the other.
    #[allow(unsafe_code)]
    fn bind_map(&mut self, d: &ash::Device, set: SetIx, side: MapSide) {
        let buffer = side.of(&mut self.maps).buffer();
        let Some(s) = self.sets.get_mut(set.0) else {
            return;
        };
        if s.map_bound == Some(side) {
            return;
        }
        let info = [vk::DescriptorBufferInfo::default()
            .buffer(buffer)
            .range(vk::WHOLE_SIZE)];
        let write = [vk::WriteDescriptorSet::default()
            .dst_set(s.set)
            .dst_binding(2)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&info)];
        // SAFETY: as in `bind_atlas`.
        unsafe { d.update_descriptor_sets(&write, &[]) };
        s.map_bound = Some(side);
    }

    /// **The live map's entries owed where they differ from those its
    /// device buffer was last given**: every one the first time, the ones
    /// inserted since after. Nothing in place.
    fn owe_map(&mut self, (entries, side): (&[MapEntry], MapSide)) {
        if self.arm == Arm::InPlace {
            return;
        }
        let (lent, given) = (side.of(&mut self.maps), side.of(&mut self.given));
        let mut run: Option<usize> = None;
        for (i, (e, g)) in entries.iter().zip(given.iter_mut()).enumerate() {
            match (*g == Some(*e), run) {
                (false, None) => run = Some(i),
                (true, Some(from)) => {
                    lent.owe(from..i);
                    run = None;
                }
                _ => {}
            }
            *g = Some(*e);
        }
        if let Some(from) = run {
            lent.owe(from..entries.len());
        }
    }
}

impl CellSet {
    /// Set 0 of the cell layout written over this set's buffers, the map
    /// left to [`CellMemory::bind_map`].
    #[allow(unsafe_code)]
    fn write_own(&self, d: &ash::Device) {
        let whole = |b: vk::Buffer| {
            [vk::DescriptorBufferInfo::default()
                .buffer(b)
                .range(vk::WHOLE_SIZE)]
        };
        let (grid, styles, overlay, cu, seg) = (
            whole(self.grid.buffer()),
            whole(self.styles.buffer()),
            whole(self.overlay.buffer()),
            whole(self.cu.buffer()),
            whole(self.seg_base.buffer()),
        );
        let storage = vk::DescriptorType::STORAGE_BUFFER;
        let at = |binding: u32, kind: vk::DescriptorType| {
            vk::WriteDescriptorSet::default()
                .dst_set(self.set)
                .dst_binding(binding)
                .descriptor_type(kind)
        };
        let writes = [
            at(0, storage).buffer_info(&grid),
            at(1, storage).buffer_info(&styles),
            at(3, storage).buffer_info(&overlay),
            at(4, vk::DescriptorType::UNIFORM_BUFFER).buffer_info(&cu),
            at(5, storage).buffer_info(&seg),
        ];
        // SAFETY: the set is new, and every buffer named alive.
        unsafe { d.update_descriptor_sets(&writes, &[]) };
    }

    /// Set 0 of the instance layout, each of its uniform slots, written
    /// over the records' buffer and its slot.
    #[allow(unsafe_code)]
    fn write_records(&self, d: &ash::Device) {
        let records = [vk::DescriptorBufferInfo::default()
            .buffer(self.records.buffer.buffer)
            .range(vk::WHOLE_SIZE)];
        let us: Vec<[vk::DescriptorBufferInfo; 1]> = (0..INSTANCE_US)
            .map(|k| {
                [vk::DescriptorBufferInfo::default()
                    .buffer(self.us.buffer.buffer)
                    .offset(vk::DeviceSize::try_from(k * U_PITCH).unwrap_or(0))
                    .range(vk::DeviceSize::try_from(U_BYTES).unwrap_or(0))]
            })
            .collect();
        let fx = [vk::DescriptorBufferInfo::default()
            .buffer(self.fx_u.buffer.buffer)
            .range(vk::WHOLE_SIZE)];
        let mut writes = Vec::with_capacity(2 * INSTANCE_US + 1);
        writes.push(
            vk::WriteDescriptorSet::default()
                .dst_set(self.fx_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&fx),
        );
        for (set, u) in self.inst.iter().zip(&us) {
            writes.push(
                vk::WriteDescriptorSet::default()
                    .dst_set(*set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&records),
            );
            writes.push(
                vk::WriteDescriptorSet::default()
                    .dst_set(*set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(u),
            );
        }
        // SAFETY: no frame in flight reads the sets; the buffers are alive.
        unsafe { d.update_descriptor_sets(&writes, &[]) };
    }

    /// **The records' buffer at least `bytes` long**: made anew, at the
    /// next power of two, only when it is shorter.
    fn hold_records(&mut self, gpu: &Gpu, bytes: Elems) -> Result<(), Failed> {
        if self.records.bytes.len() >= bytes.0 {
            return Ok(());
        }
        let want = bytes
            .0
            .max(RECORDS_FLOOR * Instance::BYTES)
            .next_power_of_two();
        let made = gpu.mapped(Elems(want), vk::BufferUsageFlags::STORAGE_BUFFER)?;
        let old = std::mem::replace(&mut self.records, made);
        gpu.drop_buffer(old.buffer);
        self.write_records(&gpu.device);
        Ok(())
    }

    /// Uniform slot `k` written with `u`.
    fn put_u(&mut self, k: Elems, u: &[f32; 12]) {
        let at = k.0 * U_PITCH;
        for (dst, f) in self.us.bytes[at..at + U_BYTES]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(u)
        {
            dst.copy_from_slice(&f.to_le_bytes());
        }
    }
}

/// **What the shape pass draws**: every cell's slots while a slot of the
/// table carries a decoration, else the cursor's slot alone when it is an
/// underline or a bar, else nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShapeDraw {
    Every,
    Cursor(Elems),
    Nothing,
}

/// An instance count or first instance, as a draw takes it.
fn n32(n: Elems) -> Instances {
    Instances(u32::try_from(n.0).unwrap_or(u32::MAX))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Instances(u32);

/// A pass's `part` word: the background pass's `ground_pass`, the glyph
/// pass's part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Part(u32);

const GROUND_EVERY: Part = Part(0);
const PART_INSIDE: Part = Part(0);
const PART_ABOVE: Part = Part(1);

impl Gpu {
    /// **A cell frame drawn into `t`**: the sheets brought up to the atlas,
    /// `CU` written into `input.set`'s memory and, under staging, every
    /// range the shader reads owed, all recorded ahead of the render pass;
    /// then the target drawn in the macOS cell path's order -- the
    /// backgrounds; the grounds; the glyphs inside their rows; the records
    /// inside their rows; the shape slots; the hollow cursor; the glyphs
    /// above their rows; the rest of the records; baud's own overlays; and
    /// the exit-status strip, the lights' line and the word wrap's marks in
    /// their own viewports -- and released to the compositor's queue family
    /// after. The CPU waits for it.
    ///
    /// **Whole or in bands.** The frame is compared with the one drawn last
    /// on everything it is handed and every word of `CU`, its grid by the
    /// screen it names: when only the cursor's words, the moving records
    /// and the grounds differ, its change is the rows of the old and the
    /// new cursor, each with the rows its glyph reaches, and the rows those
    /// records and the grounds that changed cover; anything else is the
    /// whole. A target that holds an earlier frame draws its stale rows and
    /// that change: it is taken back from the compositor's queue family as
    /// it was left, and each band of rows ([`damage::bands`]) is drawn under
    /// its scissor -- the cell passes over its rows and the rows whose
    /// glyphs reach it, every record held to the scissor, the strips kept
    /// as the target holds them. A target that holds no frame, and a frame
    /// whose change is the whole, are cleared to the page's colour and
    /// drawn whole. [`CellMemory::damage`] names the rows drawn after.
    ///
    /// Images are not drawn and are counted in
    /// [`CellMemory::images_skipped`]; an effect trail's records are not
    /// drawn and are counted in [`CellMemory::trail_skipped`].
    #[allow(unsafe_code, clippy::too_many_lines)]
    pub fn draw_cells(
        &self,
        pipes: &mut Pipelines,
        mem: &mut CellMemory,
        sheets: &mut Sheets,
        renderer: &mut Renderer<'_, '_>,
        input: &CellDraw<'_>,
        t: &Target,
    ) -> Result<CellDrawn, Failed> {
        let d = &self.device;
        let set = input.set.0;
        if set >= mem.sets.len() {
            return Err(Failed::Outside);
        }
        let (cols, rows) = renderer.grid();
        let slide_rows = usize::from(input.slide.is_some());
        let draw_rows = rows.index().saturating_add(slide_rows);
        let ncols = cols.index();
        let (ring, stride) = (input.ring.index().max(1), input.stride.index());
        // The shader reads `ring * stride` cells of the copy, and the overlay
        // and first slots of every draw row (the status line's and, while
        // sliding, the leaving row's among them): memory shorter is refused
        // rather than read past its end.
        let s = &mem.sets[set];
        if s.grid_len.0 < ring.saturating_mul(stride)
            || s.overlay_len.0 < ncols.saturating_mul(draw_rows)
            || s.seg_len.0 < draw_rows
        {
            return Err(Failed::Outside);
        }
        sheets.bring(self, renderer.atlas_mut())?;
        mem.bind_atlas(d, sheets);
        mem.bind_map(d, input.set, input.map.1);
        mem.owe_map(input.map);
        mem.images_skipped.0 += u64::try_from(renderer.image_placements().len()).unwrap_or(0);
        let cu = cell_uniforms(renderer, input);
        let kind = CursorKind::drawn(renderer, input);
        let metrics = renderer.metrics();
        let frame_px = renderer.frame_size();
        let framed = Chrome::of(renderer, (t.width, t.height));
        let (boxed, clear) = (framed.boxed, framed.clear);
        let side_m = renderer.atlas().side().value() as f32;
        let side_c = renderer.atlas().color_side().value() as f32;
        // The shape pass's draw, and whether a glyph reaches above its row.
        let shape_draw = match (input.table.decorations(), kind, input.cursor) {
            (Decorations::Present, _, _) => ShapeDraw::Every,
            (Decorations::Absent, CursorKind::Underline | CursorKind::Bar, Some((r, c, _)))
                if r.index() + slide_rows < draw_rows && c.index() < ncols =>
            {
                let v = renderer.bidi().visual_col(r, c);
                ShapeDraw::Cursor(Elems(
                    ((r.index() + slide_rows) * ncols + v.index()) * SHAPES_PER_CELL
                        + CURSOR_SHAPE_SLOT,
                ))
            }
            (Decorations::Absent, _, _) => ShapeDraw::Nothing,
        };
        // The rows a glyph reaches past its own, up and down.
        let ch = i64::from(metrics.cell_h.value().max(1));
        let base = i64::from(metrics.baseline.value());
        let past = |px: i64| usize::try_from((px.max(0) + ch - 1) / ch).unwrap_or(usize::MAX);
        let up = past(-(base + i64::from(input.reach.top.0)));
        let down = past(base + i64::from(input.reach.bottom.0) - ch);
        let above_draws = up > 0;
        // The records: the grounds, the records, then the strip's marks,
        // the lights and the word wrap's marks.
        let strip = renderer.strip_records();
        let lights = renderer.lights_records();
        let wraps = renderer.margin_records();
        // **What this frame changed of the one drawn last**, from what it
        // was handed: the grid's rows by the screen it names, and the
        // overlay as the shader reads it.
        let n_records = input.records.len();
        let ma = input.moving.0.0.min(n_records);
        let mb = input.moving.1.0.min(n_records).max(ma);
        let cell_px = (metrics.cell_w.value(), metrics.cell_h.value());
        let grid_cells = (
            u32::try_from(ncols).unwrap_or(u32::MAX),
            u32::try_from(draw_rows).unwrap_or(u32::MAX),
        );
        let moving = damage::records_rows(&input.records[ma..mb], cell_px, grid_cells);
        let unrowed = match moving.is_none()
            || !renderer.image_placements().is_empty()
            || input.slide.is_some()
        {
            true => Unrowed::Yes,
            false => Unrowed::No,
        };
        let overlay_now = &input.overlay[..ncols
            .saturating_mul(draw_rows.saturating_add(1))
            .min(input.overlay.len())];
        let clear_word = renderer.clear_color();
        let atlas_gen = renderer.atlas().generation();
        let size = (t.width, t.height);
        let bare = cu.without_cursor();
        let alike = |l: &CellLast| {
            l.size == size
                && l.view == boxed.grid
                && l.cu.without_cursor() == bare
                && l.clear == clear_word
                && l.strip.iter().eq(strip.iter().chain(wraps))
                && l.lights == lights
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
                && l.records.len() == n_records - (mb - ma)
                && l.records[..ma] == input.records[..ma]
                && l.records[ma..] == input.records[mb..]
                && l.overlay == overlay_now
                && l.screen == input.screen
        };
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
        // The grounds that changed -- the cursor guide moved with the
        // cursor's row, the badge cut round another cell -- the rows of
        // every ground from the first that differs to the last, in either
        // frame, and the whole when one reaches past the grid.
        let ground_rows = |l: &CellLast| {
            let (old, now) = (&l.grounds[..], input.grounds);
            let same = old.iter().zip(now).take_while(|(a, b)| a == b).count();
            let tail = old[same..]
                .iter()
                .rev()
                .zip(now[same..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            damage::records_rows(
                old[same..old.len() - tail]
                    .iter()
                    .chain(&now[same..now.len() - tail]),
                cell_px,
                grid_cells,
            )
        };
        let changed = match (mem.last.as_deref(), moving) {
            (Some(l), Some(now)) if alike(l) => match ground_rows(l) {
                Some(grounds) => {
                    let cursor = match l.cu == cu {
                        true => Redraw::NOTHING,
                        false => cursor_rows(l.cu.cursor_at()).union(cursor_rows(cu.cursor_at())),
                    };
                    Redraw::Rows(now.union(l.moving).union(grounds)).union(cursor)
                }
                None => Redraw::Whole,
            },
            _ => Redraw::Whole,
        };
        let g = input.grounds.len();
        let n = g + input.records.len();
        let all = n + strip.len() + lights.len() + wraps.len();
        let s = &mut mem.sets[set];
        s.hold_records(self, Elems(all.saturating_mul(Instance::BYTES)))?;
        let mut trail: Option<(usize, usize)> = None;
        for (i, (chunk, inst)) in s
            .records
            .bytes
            .as_chunks_mut::<{ Instance::BYTES }>()
            .0
            .iter_mut()
            .zip(
                input
                    .grounds
                    .iter()
                    .chain(input.records)
                    .chain(strip)
                    .chain(lights)
                    .chain(wraps),
            )
            .enumerate()
        {
            if matches!(inst.kind, Kind::Quad | Kind::Band | Kind::Particle) {
                trail = Some(match trail {
                    None => (i, i + 1),
                    Some((a, _)) => (a, i + 1),
                });
            }
            chunk.copy_from_slice(&inst.to_bytes());
        }
        let under = g + usize::try_from(input.under.value()).map_or(n - g, |u| u.min(n - g));
        let chrome = usize::try_from(input.chrome.value()).map_or(n, |c| c.min(n));
        let content = (n - chrome).max(under);
        let ru = [
            frame_px.0.value() as f32,
            frame_px.1.value() as f32,
            metrics.cell_w.value() as f32,
            metrics.cell_h.value() as f32,
            0.0,
            0.0,
            side_m,
            side_m,
            side_c,
            side_c,
            boxed.pad[0],
            boxed.pad[1],
        ];
        s.put_u(Elems(0), &ru);
        let strips = framed.strips(Elems(n), [strip.len(), lights.len(), wraps.len()]);
        let cell_px = (metrics.cell_w.value() as f32, metrics.cell_h.value() as f32);
        for (k, st) in strips.iter().enumerate() {
            s.put_u(Elems(k + 1), &st.u(cell_px, (side_m, side_c)));
        }
        for (w, word) in s.cu_mem.iter_mut().zip(cu.0) {
            *w = word.0;
        }
        s.last = cu;
        // Under staging, what the device's copies lack: the grid when they
        // hold another screen, the renditions another table, the band
        // bases once, and each overlay row that differs from the last
        // given; `CU` always, its 152 bytes. In place, owing is nothing.
        if s.grid.arm() == Arm::Staging {
            if s.holds != Some(input.screen) {
                s.grid.owe(0..ring.saturating_mul(stride));
                s.holds = Some(input.screen);
            }
            if s.styled != Some(input.table) {
                s.styles.owe(0..s.styles_len.0);
                s.styled = Some(input.table);
            }
            if s.based == Given::No {
                s.seg_base.owe(0..s.seg_len.0);
                s.based = Given::Yes;
            }
            let span = ncols
                .saturating_mul(draw_rows + 1)
                .div_ceil(4)
                .saturating_mul(4)
                .min(input.overlay.len());
            let now = &input.overlay[..span];
            if s.overlay_given.len() != span {
                s.overlay.owe(0..span);
                s.overlay_given.clear();
                s.overlay_given.extend_from_slice(now);
            } else {
                for row in 0..span.div_ceil(ncols.max(1)) {
                    let at = row * ncols.max(1)..((row + 1) * ncols.max(1)).min(span);
                    if now[at.clone()] != s.overlay_given[at.clone()] {
                        s.overlay.owe(at.clone());
                        s.overlay_given[at.clone()].copy_from_slice(&now[at]);
                    }
                }
            }
        }
        s.cu.owe(0..CU_WORDS);
        // The pipelines, built before the first frame.
        let bg_pipe = pipes.get(Fragment::CellBg, Blend::Replace)?;
        let glyph_pipe = pipes.get(Fragment::CellGlyph, Blend::Normal)?;
        let shape_pipe = pipes.get(Fragment::CellShape, Blend::Normal)?;
        let records_pipe = pipes.get(Fragment::Glyph, Blend::Normal)?;
        let hollow = match kind {
            CursorKind::Hollow => Some(pipes.get(Fragment::CellRing, Blend::Normal)?),
            _ => None,
        };
        // **The trail's draw**, where the frame has one: the fireworks by
        // their own stages and an effect by its own fragment under the
        // plasma's mix, each with its uniform at set 2 from the grid's pad,
        // as the macOS path draws them; never waited for -- a pipeline still
        // building leaves the trail undrawn, counted, and its build wakes
        // the loop. A smear's quad has no stage here, and is counted too.
        let trail_pipes: Option<Vec<vk::Pipeline>> = {
            let fx_u = &mut mem.sets[set].fx_u.bytes;
            let mut put = |floats: &[f32]| {
                for (at, f) in fx_u.as_chunks_mut::<4>().0.iter_mut().zip(floats) {
                    at.copy_from_slice(&f.to_le_bytes());
                }
            };
            match (renderer.fireworks_uniform(boxed.pad), renderer.vfx()) {
                (Some(u), _) => {
                    put(&u);
                    pipes
                        .ready(Fragment::Fireworks, Blend::Normal)
                        .map(|p| vec![p])
                }
                (None, Some(v)) => {
                    put(&v.uniform(boxed.pad));
                    let (fx, blend) = Fx::mixed(v.effect, renderer.trail_mix());
                    self.passes(fx, blend)
                        .into_iter()
                        .map(|(fx, blend)| pipes.ready(Fragment::Effect(fx), blend))
                        .collect()
                }
                (None, None) => None,
            }
        };
        let (cell_layout, inst_layout) = (pipes.cell_layout(), pipes.layout());
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let (framebuffer, ix) = mem.framebuffer(d, pipes.pass(), t)?;
        // **What the frame draws into `t`**: the rows it lacks and this
        // frame's change; the whole into a target outside the kept ones.
        let drawn = match ix {
            Some(k) => mem.stale.draws(k, changed),
            None => Redraw::Whole,
        };
        // The clips: the whole, or each band under its scissor with the
        // rows whose glyphs reach it, the band's rows down from those that
        // reach down into it to those that reach up into it.
        let mut clips = [Clip::Whole; BANDS];
        let n_clips = match drawn {
            Redraw::Whole => 1,
            Redraw::Rows(set) => {
                damage::bands(set, &mut mem.bands);
                let pad = boxed.pad;
                let grid_w = Px(renderer.pixel_size().0.value());
                let ch_px = metrics.cell_h.value();
                let px_of = |r: usize| u32::try_from(r).unwrap_or(u32::MAX).saturating_mul(ch_px);
                for (clip, &(r, k)) in clips.iter_mut().zip(&mem.bands) {
                    let from = r.index().saturating_sub(down);
                    let to = (r.index() + k.index()).saturating_add(up).min(draw_rows);
                    let top = pad[1] + px_of(r.index()) as f32;
                    *clip = Clip::Band {
                        from: Elems(from),
                        to: Elems(to.max(from)),
                        rect: scissor(
                            (t.width, t.height),
                            [pad[0], top],
                            (grid_w, Px(px_of(k.index()))),
                        ),
                    };
                }
                mem.bands.len().min(BANDS)
            }
        };
        // A whole frame clears every pixel, so what the image held is not
        // kept: from UNDEFINED. A frame in bands keeps it: taken back from
        // the compositor's queue family in the layout it was released in.
        let (from_layout, families) = match drawn {
            Redraw::Whole => (
                vk::ImageLayout::UNDEFINED,
                (vk::QUEUE_FAMILY_IGNORED, vk::QUEUE_FAMILY_IGNORED),
            ),
            Redraw::Rows(_) => (
                vk::ImageLayout::GENERAL,
                (vk::QUEUE_FAMILY_FOREIGN_EXT, self.family.0),
            ),
        };
        let to_colour = vk::ImageMemoryBarrier::default()
            .old_layout(from_layout)
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
        let whole_rect = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: t.width.0,
                height: t.height.0,
            },
        };
        let begin = vk::RenderPassBeginInfo::default()
            .render_pass(pipes.pass())
            .framebuffer(framebuffer)
            .render_area(whole_rect);
        let clear_att = [vk::ClearAttachment {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            color_attachment: 0,
            clear_value: vk::ClearValue {
                color: clear.clear(),
            },
        }];
        let clear_rect = [vk::ClearRect {
            rect: whole_rect,
            base_array_layer: 0,
            layer_count: 1,
        }];
        let atlas = mem.atlas;
        let s = &mem.sets[set];
        let (cell_sets, inst_sets) = ([s.set, atlas], s.inst.map(|i| [i, atlas]));
        let fx_set = [s.fx_set];
        let target_size = (t.width, t.height);
        let mut skipped = 0u64;
        let clips = &clips[..n_clips];
        let submitted = self.submit_reusing(&mem.recorder, |cb| {
            sheets.record(d, cb);
            let s = &mut mem.sets[set];
            s.grid.record(d, cb);
            s.styles.record(d, cb);
            s.overlay.record(d, cb);
            s.cu.record(d, cb);
            s.seg_base.record(d, cb);
            input.map.1.of(&mut mem.maps).record(d, cb);
            // A grid with no width or height draws nothing: Vulkan takes no
            // empty viewport.
            let drawable = boxed.grid.w > 0.0 && boxed.grid.h > 0.0;
            // The cell passes' bindings and part, then one instanced draw.
            let cell = |pipe: vk::Pipeline, part: Part, n: Instances, first: Instances| {
                if !drawable || n.0 == 0 {
                    return;
                }
                // SAFETY: every handle is alive until the wait ends; the
                // sets match the cell layout, the push constant its range.
                unsafe {
                    d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, pipe);
                    d.cmd_bind_descriptor_sets(
                        cb,
                        vk::PipelineBindPoint::GRAPHICS,
                        cell_layout,
                        0,
                        &cell_sets,
                        &[],
                    );
                    d.cmd_push_constants(
                        cb,
                        cell_layout,
                        vk::ShaderStageFlags::VERTEX,
                        0,
                        &part.0.to_le_bytes(),
                    );
                    d.cmd_draw(cb, 4, n.0, 0, first.0);
                }
            };
            // Records `from..to` by the instance pipeline under uniform
            // slot `k`; the trail's among them left undrawn and counted.
            let mut records = |k: usize, from: usize, to: usize| {
                if !drawable {
                    return;
                }
                let run = |f: usize, e: usize| {
                    if e <= f {
                        return;
                    }
                    // SAFETY: as above; the sets match the instance layout,
                    // and the records `f..e` lie inside the buffer.
                    unsafe {
                        d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, records_pipe);
                        d.cmd_bind_descriptor_sets(
                            cb,
                            vk::PipelineBindPoint::GRAPHICS,
                            inst_layout,
                            0,
                            &inst_sets[k],
                            &[],
                        );
                        d.cmd_draw(cb, 4, n32(Elems(e - f)).0, 0, n32(Elems(f)).0);
                    }
                };
                match trail {
                    Some((a, b)) if a < to && a >= from => {
                        let b = b.min(to);
                        run(from, a);
                        match &trail_pipes {
                            // SAFETY: as above; the sets match the instance
                            // layout, set 2 the trail's uniform, and the
                            // records `a..b` lie inside the buffer.
                            Some(passes) => unsafe {
                                for &p in passes {
                                    d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, p);
                                    d.cmd_bind_descriptor_sets(
                                        cb,
                                        vk::PipelineBindPoint::GRAPHICS,
                                        inst_layout,
                                        0,
                                        &inst_sets[k],
                                        &[],
                                    );
                                    d.cmd_bind_descriptor_sets(
                                        cb,
                                        vk::PipelineBindPoint::GRAPHICS,
                                        inst_layout,
                                        2,
                                        &fx_set,
                                        &[],
                                    );
                                    d.cmd_draw(cb, 4, n32(Elems(b - a)).0, 0, n32(Elems(a)).0);
                                }
                            },
                            None => skipped += u64::try_from(b - a).unwrap_or(0),
                        }
                        run(b, to);
                    }
                    _ => run(from, to),
                }
            };
            // SAFETY: the barriers name the target and this queue's family;
            // every handle is alive until the wait ends.
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
                if drawn == Redraw::Whole {
                    d.cmd_clear_attachments(cb, &clear_att, &clear_rect);
                }
            }
            for clip in clips {
                // The rows a pass's instances cover, `per` a row: every
                // draw row, or the band's and those reaching it.
                let (cut, rows) = match *clip {
                    Clip::Whole => (whole_rect, (0, draw_rows)),
                    Clip::Band { from, to, rect } => (rect, (from.0, to.0)),
                };
                let span = |per: usize| {
                    (
                        n32(Elems(per.saturating_mul(rows.1 - rows.0))),
                        n32(Elems(per.saturating_mul(rows.0))),
                    )
                };
                let (cells, first) = span(ncols);
                // SAFETY: as above.
                unsafe {
                    if drawable {
                        d.cmd_set_viewport(cb, 0, &[boxed.grid.vk()]);
                    }
                    d.cmd_set_scissor(cb, 0, &[cut]);
                }
                // The backgrounds.
                cell(bg_pipe, GROUND_EVERY, cells, first);
                // The grounds over them, under the glyphs; the glyphs inside
                // their rows.
                records(0, 0, g);
                cell(glyph_pipe, PART_INSIDE, cells, first);
                // The records inside their rows, then the decorations.
                records(0, g, under);
                match shape_draw {
                    ShapeDraw::Every => {
                        let (n, at) = span(ncols.saturating_mul(SHAPES_PER_CELL));
                        cell(shape_pipe, GROUND_EVERY, n, at);
                    }
                    // The cursor's slot alone, held to a band by its
                    // scissor as the whole pass would be.
                    ShapeDraw::Cursor(at) => cell(shape_pipe, GROUND_EVERY, Instances(1), n32(at)),
                    ShapeDraw::Nothing => {}
                }
                // The hollow cursor over the decorations: its four sides.
                if let Some(hollow) = hollow {
                    cell(hollow, GROUND_EVERY, Instances(4), Instances(0));
                }
                // The parts of glyphs above their rows.
                if above_draws {
                    cell(glyph_pipe, PART_ABOVE, cells, first);
                }
                // The rest of the records, then baud's own overlays.
                records(0, under, content);
                records(0, content, n);
                // The strips, each in its own viewport and scissor: outside
                // every band, and kept as the target holds them.
                if !matches!(clip, Clip::Whole) {
                    continue;
                }
                for (k, st) in strips.iter().enumerate() {
                    if !st.drawn() {
                        continue;
                    }
                    let cut = st.scissor(target_size);
                    // SAFETY: as above.
                    unsafe {
                        d.cmd_set_viewport(cb, 0, &[st.view.vk()]);
                        d.cmd_set_scissor(cb, 0, &[cut]);
                    }
                    records(k + 1, st.from.0, st.to.0);
                }
            }
            // SAFETY: as above.
            unsafe {
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
        mem.trail_skipped.0 += skipped;
        sheets.finished(self);
        // A frame that failed leaves its target holding nothing; the frame
        // drawn last is still the one before it.
        if let Err(e) = submitted {
            if let Some(k) = ix {
                mem.stale.forget(k);
            }
            return Err(e);
        }
        let drawn = match ix {
            Some(k) => mem.stale.drew(k, changed),
            None => {
                mem.stale.elsewhere(changed);
                Redraw::Whole
            }
        };
        // The pixel rows each band drew, for the compositor.
        mem.shown = drawn;
        mem.strips.clear();
        mem.strips.extend(clips.iter().filter_map(|c| match *c {
            Clip::Band { rect, .. } => {
                let top = u32::try_from(rect.offset.y).unwrap_or(0);
                Some(Damaged {
                    top: Px(top),
                    bottom: Px(top.saturating_add(rect.extent.height)),
                })
            }
            Clip::Whole => None,
        }));
        // What the next frame is compared with: this one, its records,
        // marks and overlay kept where a frame alike leaves them as they
        // are.
        let last = mem.last.get_or_insert_with(|| {
            Box::new(CellLast {
                size,
                view: boxed.grid,
                cu,
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
            })
        });
        if changed == Redraw::Whole {
            last.strip.clear();
            last.strip.extend(strip.iter().chain(wraps).copied());
            last.lights.clear();
            last.lights.extend_from_slice(lights);
            last.records.clear();
            last.records.extend_from_slice(&input.records[..ma]);
            last.records.extend_from_slice(&input.records[mb..]);
            last.overlay.clear();
            last.overlay.extend_from_slice(overlay_now);
        }
        last.size = size;
        last.view = boxed.grid;
        last.cu = cu;
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
        last.screen = input.screen;
        Ok(CellDrawn { changed, drawn })
    }
}
