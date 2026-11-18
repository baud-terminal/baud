//! **One screen, three frames, held to each other**: a terminal fed as
//! bytes and a renderer, drawn on the CPU path (`Renderer::render_at`), on
//! the instance path (`Renderer::convert`, [`Gpu::draw`]) and on the cell
//! path (`baud_render::cell_frame::build`, [`Gpu::draw_cells`]) on one
//! device and one memory arm, each into a target of the whole frame's size
//! and read back. [`Arms::check`] holds them to the cell path's bars:
//!
//! - the cell frame is the instance frame, pixel for pixel, on every
//!   device ([`Frames::cell_is_instance`]);
//! - the cell frame and the instance frame are each the CPU path's
//!   premultiplied frame through `common::assert_frame_stacked`: exact where
//!   no record blends, within a step a channel where one does, the step
//!   rounded term by term on llvmpipe and once, a step off only at a tie,
//!   everywhere else, and on llvmpipe two steps where two or more records
//!   blend over each other, as an effect's stacked records are held; the
//!   lean gated everywhere but on llvmpipe
//!   ([`Frames::against_cpu`]).
//!
//! **A pixel is blended** where a record of the instance path's frame
//! draws it at a source alpha strictly between nothing and all: a mask
//! texel's coverage bent by its record's contrast curve and taken with the
//! record's alpha, a colour texel's alpha, a solid's alpha, and every pixel
//! of an effect's record. Each from the atlas texel the record's pixel
//! reads, as `glyph.vert` maps it, a texel a pixel.
//!
//! **What a scene sets.** The terminal is the scene's own, fed with
//! [`feed`]; the selection, the composition, the predicted echoes and how
//! far the window looks back are [`Arms`]' fields, which the cell frame's
//! build and the renderer are handed each draw; anything else -- the
//! cursor's look and phase, a bell's tint, the other video, the theme -- is
//! set on [`Arms::renderer`], which all three paths draw through, one
//! renderer for the three as one atlas feeds both GPU paths' sheets.
//!
//! **Fonts** are read from the repository by path ([`repo_file`]):
//! JetBrains Mono from `baud-font/tests/fonts`, under the SIL Open Font
//! Licence 1.1 (`JetBrainsMono-LICENSE.txt` beside it), and the faces the
//! app ships from `baud-app/fonts`, each with its licence.
//!
//! What the cell path leaves undrawn, images and effect records, is counted
//! by the cell memory ([`Frames::skipped`]), and a frame that skipped any
//! is not the instance path's: such a scene is not one of these.

#![allow(dead_code)]

use crate::common::{Bar, Rounding, assert_frame_stacked};
use baud_core::Term;
use baud_domain::{BytePos, Codepoint, Col, PackedCell, ReadLen, Row, RowExtent, Rows, WinSize};
use baud_font::Px as FontPx;
use baud_present_vulkan::cells::{CellDraw, CellDrawn, CellMemory, CellSizes, CellSlices, MapSide};
use baud_present_vulkan::draw::{Pass, Records, Scene, Texel, Texels};
use baud_present_vulkan::frame::{Gpu, Target};
use baud_present_vulkan::lend::{Arm, Elems};
use baud_present_vulkan::pipeline::Pipelines;
use baud_present_vulkan::sheets::Sheets;
use baud_present_wayland::Px;
use baud_render::cell_frame::{CellFrame, CellInputs, CellParts, build};
use baud_render::screens::{SETS, Screens, SetIx};
use baud_render::{
    Blend, Candidate, Composition, Damage, GlyphMap, Highlight, Instance, Kind, Leaving, Lent,
    Overlay, PxSize, Renderer, Stack, StyleStamp, Target as CpuTarget, Theme,
};

/// The bytes of a file in the repository, by its path from the root.
pub fn repo_file(path: &str) -> Vec<u8> {
    let at = format!("{}/../{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&at).unwrap_or_else(|e| panic!("{at}: {e}"))
}

/// JetBrains Mono Regular, the face every scene starts from.
pub fn jetbrains_mono() -> Vec<u8> {
    repo_file("baud-font/tests/fonts/JetBrainsMono-Regular.ttf")
}

/// `bytes` written into the terminal's input and parsed.
pub fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// **A renderer over `stack`** at 14 pixels, blending as the platform's
/// text does, sized to `ws`.
pub fn renderer<'f, 'a>(stack: Stack<'f, 'a>, theme: Theme, ws: WinSize) -> Renderer<'f, 'a> {
    Renderer::new(
        stack,
        PxSize::of(14.0).expect("a size"),
        theme,
        Blend::Native,
        baud_domain::Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .expect("a renderer")
}

/// **The bar a frame on `gpu` is held to against the CPU path's**: the
/// blend rounded term by term on llvmpipe and once everywhere else, by
/// its `VkDriverId`.
pub fn bar(gpu: &Gpu) -> Bar {
    let driver = gpu.driver();
    let rounding = match driver {
        ash::vk::DriverId::MESA_LLVMPIPE => Rounding::TermByTerm,
        _ => Rounding::OnceAtTies,
    };
    Bar { rounding, driver }
}

/// How many pixels of `a` and `b` differ.
pub fn differing(a: &[u32], b: &[u32]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// **The three frames of one screen**, premultiplied, with the pixels a
/// record blends and what the cell path left undrawn.
pub struct Frames {
    pub cpu: Vec<u32>,
    pub instance: Vec<u32>,
    pub cell: Vec<u32>,
    pub blended: Vec<bool>,
    /// The blended pixels two or more records blend over each other.
    pub stacked: Vec<bool>,
    pub width: Px,
    /// Images and effect records the cell frame left undrawn.
    pub skipped: Skips,
}

/// What a cell frame left undrawn, as the cell memory counts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Skips {
    pub images: baud_present_vulkan::pipeline::Skipped,
    pub trails: baud_present_vulkan::pipeline::Skipped,
}

impl Frames {
    fn wide(&self) -> usize {
        usize::try_from(self.width.0).expect("a width")
    }

    /// **The cell frame is the instance frame**, pixel for pixel: the two
    /// blend the same glyphs over the same page through the same pipeline
    /// state, so a pixel apart is a rule the cell path resolves otherwise.
    pub fn cell_is_instance(&self, scene: &str) {
        assert_eq!(
            (self.skipped.images.0, self.skipped.trails.0),
            (0, 0),
            "{scene}: the cell path left {:?} undrawn, which the instance path draws",
            self.skipped
        );
        let w = self.wide();
        let apart: Vec<String> = (0..self.instance.len())
            .filter(|&i| self.cell[i] != self.instance[i])
            .map(|i| {
                format!(
                    "({}, {}, {:08x}, {:08x})",
                    i % w,
                    i / w,
                    self.cell[i],
                    self.instance[i]
                )
            })
            .collect();
        assert!(
            apart.is_empty(),
            "{scene}: the cell frame is {} pixels apart from the instance frame, the first \
             (x, y, cell, instance) {}",
            apart.len(),
            apart[..apart.len().min(6)].join(" ")
        );
    }

    /// **The cell frame and the instance frame, each held to the CPU
    /// path's** under `bar`.
    pub fn against_cpu(&self, scene: &str, bar: Bar) {
        for (path, got) in [
            ("the cell frame", &self.cell),
            ("the instance frame", &self.instance),
        ] {
            assert_frame_stacked(
                &format!("{scene}, {path} against the CPU path's"),
                got,
                &self.cpu,
                (&self.blended, &self.stacked),
                bar,
                self.width,
            );
        }
    }
}

/// **The device's own buffers and pipelines the cell and instance frames
/// draw through**, made once for an [`Arms`] and destroyed with it.
struct Held {
    pipes: Pipelines,
    mem: CellMemory,
    sheets: Sheets,
    /// Each set's copy, table and overlay, the second map left lent.
    lent: CellSlices,
    map: GlyphMap<'static>,
}

/// **What a cell frame draws through**, handed to a scene that draws
/// cell frames of its own ([`Arms::with_frame`]).
pub struct Drawing<'s, 'f, 'a> {
    pub gpu: &'s Gpu,
    pub pipes: &'s mut Pipelines,
    pub mem: &'s mut CellMemory,
    pub sheets: &'s mut Sheets,
    pub renderer: &'s mut Renderer<'f, 'a>,
}

impl Drawing<'_, '_, '_> {
    /// `input` drawn into `target`.
    pub fn draw(&mut self, input: &CellDraw<'_>, target: &Target) -> CellDrawn {
        self.gpu
            .draw_cells(
                self.pipes,
                self.mem,
                self.sheets,
                self.renderer,
                input,
                target,
            )
            .expect("a drawn cell frame")
    }

    /// **`input` drawn whole into a new target** of the whole frame's
    /// size, and read back.
    pub fn whole(&mut self, input: &CellDraw<'_>) -> Vec<u32> {
        let (w, h) = self.renderer.whole_size();
        let target = self
            .gpu
            .target(Px(w.value()), Px(h.value()))
            .expect("a target");
        let drawn = self.draw(input, &target);
        assert_eq!(
            drawn.drawn,
            baud_render::Redraw::Whole,
            "a new target is drawn whole"
        );
        let got = self.gpu.read(&target).expect("the cell frame read back");
        self.mem.forget(self.gpu, &target);
        self.gpu.drop_target(target);
        got
    }
}

/// **The three paths over one device and one memory arm**, and the state
/// the cell frame's build keeps from frame to frame, as a driver keeps it:
/// the glyph map, the overlay, each set's screen and resolved table.
pub struct Arms<'g, 'f, 'a> {
    pub gpu: &'g Gpu,
    /// The renderer all three paths draw through: what a scene sets beyond
    /// the fields below is set here.
    pub renderer: Renderer<'f, 'a>,
    /// How far the window looks back into the history.
    pub back: Rows,
    /// The selection, or none.
    pub highlight: Option<Highlight>,
    /// An input method's composition, or none.
    pub composition: Option<Composition>,
    /// Row 9's predicted echoes, each a codepoint at its logical cell.
    pub predictions: Vec<(Row, Col, Codepoint)>,
    /// How the device rounds a blend: term by term on llvmpipe, once
    /// elsewhere.
    pub bar: Bar,
    held: Option<Held>,
    exceptions: Overlay,
    screens: Screens,
    stamps: [StyleStamp; SETS],
    lending: [Lent; SETS],
    candidates: Vec<Candidate>,
    records: Vec<Instance>,
    grounds: Vec<Instance>,
    lent_cells: Vec<PackedCell>,
    lent_used: Vec<RowExtent>,
    used: Vec<RowExtent>,
    frame: Option<CellFrame>,
}

impl<'g, 'f, 'a> Arms<'g, 'f, 'a> {
    /// **The arms for `t`'s geometry**, the cell path's memory on `arm`.
    pub fn new(gpu: &'g Gpu, arm: Arm, renderer: Renderer<'f, 'a>, t: &Term<'_>) -> Self {
        let pipes = Pipelines::new(&gpu.device).expect("the pipelines");
        let (cols, rows) = t.geometry();
        let sizes = CellSizes {
            cols,
            rows,
            stride: t.stride(),
            styles: Elems(t.styles().len()),
        };
        let (mem, mut lent) = CellMemory::new(gpu, &pipes, arm, sizes).expect("the cell memory");
        assert_eq!(mem.arm(), arm, "the memory is on the arm asked for");
        let sheets = gpu
            .sheets(&baud_domain::contrast::CURVES[..])
            .expect("the sheets");
        let map = GlyphMap::over(lent.maps[0].take().expect("the first map"));
        Arms {
            gpu,
            renderer,
            back: Rows::ZERO,
            highlight: None,
            composition: None,
            predictions: Vec::new(),
            bar: bar(gpu),
            held: Some(Held {
                pipes,
                mem,
                sheets,
                lent,
                map,
            }),
            exceptions: Overlay::new(),
            screens: Screens::new(),
            stamps: [StyleStamp::default(); SETS],
            lending: core::array::from_fn(|_| Lent::default()),
            candidates: Vec::new(),
            records: Vec::new(),
            grounds: Vec::new(),
            lent_cells: Vec::new(),
            lent_used: Vec::new(),
            used: Vec::new(),
            frame: None,
        }
    }

    /// The overlay the last cell frame was built with: which cells its
    /// records draw.
    pub fn exceptions(&self) -> &Overlay {
        &self.exceptions
    }

    /// The resolved table set 0 holds, as the last cell frame left it.
    pub fn stamp(&self) -> StyleStamp {
        self.stamps[0]
    }

    /// The last cell frame built.
    pub fn frame(&self) -> CellFrame {
        self.frame.expect("a cell frame built")
    }

    /// The cell memory's arm.
    pub fn arm(&self) -> Arm {
        self.held().mem.arm()
    }

    fn held(&self) -> &Held {
        self.held.as_ref().expect("the arms' memory")
    }

    /// **The cell frame of `t` built into set 0**, as a driver builds it:
    /// the frame fitted, the images, marks and lights placed, the writes
    /// sealed, and the selection, composition and echoes handed in.
    pub fn build(&mut self, t: &mut Term<'_>) {
        let held = self.held.as_mut().expect("the arms' memory");
        let r = &mut self.renderer;
        r.fit(t);
        r.set_highlight(self.highlight);
        r.set_composition(self.composition.clone());
        let seal = t.seal_writes();
        r.set_images(t, self.back);
        r.splash_yields_to(t, FontPx::of(0));
        r.place_marks(t, self.back, Leaving::None, FontPx::of(0));
        r.place_lights(t);
        let live_screen = t.looking_back(self.back).index() == 0;
        let style = t.cursor_style();
        let status = t
            .status_line()
            .and_then(|l| l.cursor)
            .map(|c| (Row::from_index(t.geometry().1.index()), c, style));
        let live = status.filter(|_| live_screen).or_else(|| {
            (t.cursor_visible() && live_screen).then(|| {
                let (row, col) = t.cursor();
                (row, col, style)
            })
        });
        let inputs = CellInputs {
            back: self.back,
            predictions: self.predictions.iter().copied(),
            live,
            composition: &self.composition,
            highlight: self.highlight,
            leaving: Leaving::None,
            pan: FontPx::of(0),
        };
        let frame = build(
            r,
            t,
            seal,
            &inputs,
            &mut CellParts {
                map: &mut held.map,
                exceptions: &mut self.exceptions,
                screens: &mut self.screens,
                copies_mem: &mut held.lent.copies,
                styles_mem: &mut held.lent.styles,
                overlay_mem: &mut held.lent.overlay,
                style_stamps: &mut self.stamps,
                lent: &mut self.lending,
                candidates: &mut self.candidates,
                records: &mut self.records,
                grounds: &mut self.grounds,
                lent_cells: &mut self.lent_cells,
                lent_used: &mut self.lent_used,
            },
            &mut self.used,
            || Some(SetIx(0)),
        )
        .expect("set 0 is free");
        self.frame = Some(frame);
    }

    /// **`f` handed the last frame built, as the cell path is handed it**,
    /// and what it draws through.
    pub fn with_frame<R>(
        &mut self,
        t: &Term<'_>,
        f: impl FnOnce(&mut Drawing<'_, 'f, 'a>, &CellDraw<'_>) -> R,
    ) -> R {
        let frame = self.frame.expect("a cell frame built");
        let Held {
            pipes,
            mem,
            sheets,
            lent,
            map,
        } = self.held.as_mut().expect("the arms' memory");
        let input = CellDraw {
            set: frame.at,
            stride: t.stride(),
            rotation: baud_domain::RowRot::NONE,
            ring: frame.ring,
            cursor: frame.cursor,
            cursor_drawn: self.exceptions.cursor_drawn(),
            highlight: self.highlight,
            records: &self.records,
            grounds: &self.grounds,
            under: self.exceptions.under(),
            chrome: frame.chrome,
            slide: None,
            table: self.stamps[frame.at.0],
            reach: map.reach(),
            map: (map.entries(), MapSide::First),
            screen: self.screens.now(),
            overlay: &lent.overlay[frame.at.0][..],
            moving: (Elems(frame.moving.0), Elems(frame.moving.1)),
            map_gen: map.generation(),
        };
        let mut drawing = Drawing {
            gpu: self.gpu,
            pipes,
            mem,
            sheets,
            renderer: &mut self.renderer,
        };
        f(&mut drawing, &input)
    }

    /// The cell frame of `t`, built and drawn whole, with what it left
    /// undrawn.
    pub fn cell_frame(&mut self, t: &mut Term<'_>) -> (Vec<u32>, Skips) {
        self.build(t);
        self.with_frame(t, |d, input| {
            let before = (d.mem.images_skipped, d.mem.trail_skipped);
            let got = d.whole(input);
            let skipped = Skips {
                images: baud_present_vulkan::pipeline::Skipped(d.mem.images_skipped.0 - before.0.0),
                trails: baud_present_vulkan::pipeline::Skipped(d.mem.trail_skipped.0 - before.1.0),
            };
            (got, skipped)
        })
    }

    /// **The instance frame of `t`**: the renderer's records and
    /// backgrounds drawn whole, and the pixels a record of it blends.
    pub fn instance_frame(&mut self, t: &Term<'_>) -> (Vec<u32>, Vec<bool>) {
        let Held { pipes, sheets, .. } = self.held.as_mut().expect("the arms' memory");
        let r = &mut self.renderer;
        r.fit(t);
        r.convert(t, self.back, Damage::Full);
        let records: Vec<u8> = r.instances().flat_map(|i| i.to_bytes()).collect();
        let n = r.instances().count();
        let bg: Vec<u8> = r
            .background()
            .iter()
            .flat_map(|c| c.0.to_le_bytes())
            .collect();
        sheets
            .bring(self.gpu, r.atlas_mut())
            .expect("the sheets brought up");
        let (w, h) = r.whole_size();
        let (cols, rows) = r.grid();
        let m = r.metrics();
        let at = |c: usize| u32::try_from(c).expect("a count");
        let f = |v: u32| v as f32;
        let scene = Scene {
            u: [
                f(w.value()),
                f(h.value()),
                f(m.cell_w.value()),
                f(m.cell_h.value()),
                f(at(cols.index())),
                f(at(rows.index())),
                f(r.atlas().side().value()),
                f(r.atlas().side().value()),
                f(r.atlas().color_side().value()),
                f(r.atlas().color_side().value()),
                0.0,
                0.0,
            ],
            backgrounds: Texels {
                width: Px(at(cols.index())),
                height: Px(at(rows.index())),
                texel: Texel::Argb,
                bytes: &bg,
            },
            sheets,
            records: &records,
            count: Records(at(n)),
        };
        let target = self
            .gpu
            .target(Px(w.value()), Px(h.value()))
            .expect("a target");
        self.gpu
            .draw(pipes, &target, &scene, Pass::Whole)
            .expect("a drawn instance frame");
        let got = self
            .gpu
            .read(&target)
            .expect("the instance frame read back");
        self.gpu.drop_target(target);
        let blended = covers(r, (w.value(), h.value()))
            .into_iter()
            .map(|c| c > 0)
            .collect();
        (got, blended)
    }

    /// **The CPU path's frame of `t`**, the whole frame, premultiplied.
    pub fn cpu_frame(&mut self, t: &mut Term<'_>) -> Vec<u32> {
        let r = &mut self.renderer;
        r.fit(t);
        let (w, h) = r.whole_size();
        let n = usize::try_from(w.value() * h.value()).expect("a size");
        let mut px = vec![0u32; n];
        let mut target = CpuTarget::new(&mut px, w, w, h).expect("a target");
        r.render_at(t, self.back, &mut target, Damage::Full);
        px.into_iter().map(baud_domain::premultiply).collect()
    }

    /// **The three frames of `t`**: the cell path's first, then the
    /// instance path's, then the CPU path's.
    pub fn draw(&mut self, t: &mut Term<'_>) -> Frames {
        let (cell, skipped) = self.cell_frame(t);
        let (instance, blended) = self.instance_frame(t);
        let (w, h) = self.renderer.whole_size();
        let stacked = covers(&self.renderer, (w.value(), h.value()))
            .into_iter()
            .map(|c| c > 1)
            .collect();
        let cpu = self.cpu_frame(t);
        Frames {
            cpu,
            instance,
            cell,
            blended,
            stacked,
            width: Px(w.value()),
            skipped,
        }
    }

    /// **The three frames of `t`, held to the bars**, each failure named
    /// by `scene`.
    pub fn check(&mut self, t: &mut Term<'_>, scene: &str) -> Frames {
        let frames = self.draw(t);
        frames.cell_is_instance(scene);
        frames.against_cpu(scene, self.bar);
        frames
    }
}

/// **How many records of the renderer's last convert blend each pixel**:
/// draw it at a source alpha strictly between nothing and all, as
/// `glyph.frag` forms it, in a frame `w` by `h`.
fn covers(r: &Renderer<'_, '_>, (w, h): (u32, u32)) -> Vec<u8> {
    let wide = usize::try_from(w).expect("a width");
    let high = usize::try_from(h).expect("a height");
    let mut out = vec![0u8; wide * high];
    let atlas = r.atlas();
    let side = usize::try_from(atlas.side().value()).expect("a side");
    let cside = usize::try_from(atlas.color_side().value()).expect("a side");
    let (mask, color) = (atlas.pixels(), atlas.argb());
    for i in r.instances() {
        let ca = u8::try_from(i.color.0 >> 24).expect("a byte");
        let (x0, y0) = (i32::from(i.x.0), i32::from(i.y.0));
        for dy in 0..usize::from(i.h.0) {
            for dx in 0..usize::from(i.w.0) {
                let (Ok(x), Ok(y)) = (
                    usize::try_from(i64::from(x0) + i64::try_from(dx).expect("dx")),
                    usize::try_from(i64::from(y0) + i64::try_from(dy).expect("dy")),
                ) else {
                    continue;
                };
                if x >= wide || y >= high {
                    continue;
                }
                let (u, v) = (usize::from(i.u.0) + dx, usize::from(i.v.0) + dy);
                let a = match i.kind {
                    Kind::Mask => {
                        let cov = mask.get(v * side + u).copied().unwrap_or(0);
                        alpha_byte(ca, i.curve.apply(cov))
                    }
                    Kind::Color => color
                        .get(v * cside + u)
                        .map_or(0, |&p| u8::try_from(p >> 24).expect("a byte")),
                    Kind::Solid => ca,
                    Kind::Quad | Kind::Band | Kind::Particle => 128,
                };
                if a != 0 && a != 255 {
                    out[y * wide + x] = out[y * wide + x].saturating_add(1);
                }
            }
        }
    }
    out
}

/// `alpha_byte` in `glyph.frag`: coverage `cov` in a colour of alpha `ca`,
/// rounded to a byte.
fn alpha_byte(ca: u8, cov: u8) -> u8 {
    match ca {
        255 => cov,
        _ => u8::try_from((u32::from(ca) * u32::from(cov) + 127) / 255).expect("a byte"),
    }
}

impl Drop for Arms<'_, '_, '_> {
    /// The device's buffers and pipelines destroyed, the map and the lent
    /// slices over them dropped first; left to the process while a failed
    /// assertion unwinds, so its message is the one shown.
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        if let Some(Held {
            pipes,
            mem,
            sheets,
            lent,
            map,
        }) = self.held.take()
        {
            drop(map);
            drop(lent);
            mem.destroy(self.gpu);
            sheets.destroy(self.gpu);
            pipes.destroy();
        }
    }
}
