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
//!   premultiplied frame through `common::assert_frame_covered`: exact where
//!   no record blends; where one does, on llvmpipe, which rounds term by
//!   term, a step a channel a record that blends it; everywhere else exact
//!   but on a channel whose exact sum lies at a tie on some blend, a step
//!   off there (`common::allowance`, `common::Tie`); the lean gated
//!   everywhere but on llvmpipe ([`Frames::against_cpu`]).
//!
//! **Where a channel's exact sum lies** is found by replaying the
//! records, in the order the instance path draws them, over the frame
//! drawn with none, by the CPU path's blend in premultiplied bytes
//! ([`replay`]); the replay must come out as the CPU path's frame on every
//! pixel no effect draws, or the scene fails ([`Arms::draw`]), so a
//! replay that is wrong cannot put a pixel at a tie it is not at.
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

use crate::common::{Bar, Cover, Rounding, Tie, Ties, assert_frame_covered};
use baud_core::Term;
use baud_domain::{BytePos, Codepoint, Col, PackedCell, ReadLen, Row, RowExtent, Rows, WinSize};
use baud_font::Px as FontPx;
use baud_present_vulkan::cells::{CellDraw, CellDrawn, CellMemory, CellSizes, CellSlices, MapSide};
use baud_present_vulkan::chrome::Chrome;
use baud_present_vulkan::draw::{Framing, Pass, Records, Scene, Texel, Texels};
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
    /// How many records blend each pixel, counted from the scene's records.
    pub covers: Vec<Cover>,
    /// Where each channel's exact sum lies against a half, from the
    /// replay of the scene's records.
    pub ties: Vec<Ties>,
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
            assert_frame_covered(
                &format!("{scene}, {path} against the CPU path's"),
                got,
                &self.cpu,
                (&self.covers, &self.ties),
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
            styles: Elems(t.styles().one().len()),
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
    /// backgrounds drawn whole; and the ground, the same frame drawn with
    /// no record, which no record blends and so is the CPU path's under
    /// them.
    pub fn instance_frame(&mut self, t: &Term<'_>) -> (Vec<u32>, Vec<u32>) {
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
        let frame = r.frame_size();
        let (cols, rows) = r.grid();
        let m = r.metrics();
        let at = |c: usize| u32::try_from(c).expect("a count");
        let f = |v: u32| v as f32;
        let bytes = |run: &[baud_render::Instance]| -> Vec<u8> {
            run.iter().flat_map(|i| i.to_bytes()).collect()
        };
        let strips = [
            bytes(r.strip_records()),
            bytes(r.lights_records()),
            bytes(r.margin_records()),
        ];
        let scene = Scene {
            u: [
                f(frame.0.value()),
                f(frame.1.value()),
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
            framing: Some(Framing {
                chrome: Chrome::of(r, (Px(w.value()), Px(h.value()))),
                strips: [&strips[0], &strips[1], &strips[2]],
            }),
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
        // One record of no alpha, which leaves every pixel as it is.
        let none = crate::common::record((0, 0), (1, 1), (0, 0), (0, 2));
        let ground = Scene {
            records: &none,
            count: Records(1),
            framing: scene.framing.map(|f| Framing {
                chrome: f.chrome,
                strips: [&[], &[], &[]],
            }),
            ..scene
        };
        self.gpu.drop_target(target);
        let target = self
            .gpu
            .target(Px(w.value()), Px(h.value()))
            .expect("a target");
        self.gpu
            .draw(pipes, &target, &ground, Pass::Whole)
            .expect("a drawn ground");
        let under = self.gpu.read(&target).expect("the ground read back");
        self.gpu.drop_target(target);
        (got, under)
    }

    /// **The CPU path's frame of `t`**, the whole frame, premultiplied.
    pub fn cpu_frame(&mut self, t: &mut Term<'_>) -> Vec<u32> {
        let r = &mut self.renderer;
        r.fit(t);
        let (w, h) = r.whole_size();
        let n = usize::try_from(w.value() * h.value()).expect("a size");
        let mut px = vec![0u32; n];
        let mut target = CpuTarget::new(&mut px, w, w, h).expect("a target");
        let seal = t.seal_writes();
        r.render_at(t, seal, self.back, &mut target, Damage::Full);
        px.into_iter().map(baud_domain::premultiply).collect()
    }

    /// **The three frames of `t`**: the cell path's first, then the
    /// instance path's, then the CPU path's.
    pub fn draw(&mut self, t: &mut Term<'_>) -> Frames {
        let (cell, skipped) = self.cell_frame(t);
        let (instance, ground) = self.instance_frame(t);
        let (w, h) = self.renderer.whole_size();
        let Replay {
            covers,
            ties,
            frame,
        } = replay(&self.renderer, (w.value(), h.value()), ground);
        let cpu = self.cpu_frame(t);
        let width = usize::try_from(w.value()).expect("a width");
        let apart: Vec<String> = (0..cpu.len())
            .filter(|&i| !ties[i].effect() && frame[i] != cpu[i])
            .map(|i| {
                format!(
                    "({}, {}, {:08x}, {:08x})",
                    i % width,
                    i / width,
                    frame[i],
                    cpu[i]
                )
            })
            .collect();
        assert!(
            apart.is_empty(),
            "the records replayed over the ground are {} pixels apart from the CPU path's \
             frame, the first (x, y, replayed, cpu) {}: the ties read from the replay are \
             not the CPU path's",
            apart.len(),
            apart[..apart.len().min(6)].join(" ")
        );
        Frames {
            cpu,
            instance,
            cell,
            blended: covers.iter().map(|&c| c > 0).collect(),
            covers: covers.into_iter().map(Cover).collect(),
            ties,
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

    /// **A trail's `fragment` by `blend` built now**, so a run whose frames
    /// are compared with each other draws the trail in every one rather than
    /// only in those after its build finished.
    pub fn build_trail(
        &mut self,
        fragment: baud_present_vulkan::pipeline::Fragment,
        blend: baud_present_vulkan::pipeline::Blend,
    ) {
        let held = self.held.as_mut().expect("the arms' memory");
        held.pipes
            .get(fragment, blend)
            .expect("the trail's pipeline");
    }

    /// **A frame with an effect trail, its cell frame held to the CPU
    /// path's**: the cell path draws the trail's records by the effect's
    /// own stages once their pipelines are built -- the frames until then
    /// leave it undrawn, and are drawn again -- and none is left undrawn.
    /// The instance path draws no effect, so only the cell frame is held.
    pub fn check_trail(&mut self, t: &mut Term<'_>, scene: &str) -> Frames {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while self.cell_frame(t).1.trails.0 > 0 {
            assert!(
                std::time::Instant::now() < until,
                "{scene}: the trail's pipelines never built"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let frames = self.draw(t);
        assert_eq!(
            frames.skipped.trails.0, 0,
            "{scene}: the cell path left the trail's records undrawn"
        );
        assert_frame_covered(
            &format!("{scene}, the cell frame against the CPU path's"),
            &frames.cell,
            &frames.cpu,
            (&frames.covers, &frames.ties),
            self.bar,
            frames.width,
        );
        frames
    }
}

/// **The renderer's last convert replayed over its ground**: how many
/// records blend each pixel, where each channel's exact sums lie, and the
/// frame they leave.
struct Replay {
    covers: Vec<u8>,
    ties: Vec<Ties>,
    /// Premultiplied, as the frames compared are.
    frame: Vec<u32>,
}

/// **The records of the renderer's last convert, replayed in the order
/// the instance path draws them over `ground`**, in a frame `w` by `h` --
/// the grid's records from its pad, and each strip's from its viewport's
/// origin. A pixel is covered by a record that draws it at a source alpha
/// strictly between nothing and all, as `glyph.frag` forms it. Each record
/// is blended as the CPU path blends it (`Blend::over` and
/// `Blend::over_glass`, native) in premultiplied bytes: a source alpha `a`
/// of a mask's coverage bent by its curve and scaled by its colour's alpha
/// ([`baud_domain::premultiply_chan`]), of a colour texel's alpha, of a
/// solid's colour's; each channel `c a + d (255 - a)` over 255, the alpha
/// `255 a + d_a (255 - a)`, rounded once, and its tie read before the
/// rounding ([`Tie::of`]). An effect's record is drawn from floats each path
/// forms on its own, so its pixels and everything later over them are the
/// effect's ([`Tie::Effect`]) and left out of the replay's frame.
fn replay(r: &Renderer<'_, '_>, (w, h): (u32, u32), ground: Vec<u32>) -> Replay {
    let wide = usize::try_from(w).expect("a width");
    let high = usize::try_from(h).expect("a height");
    assert_eq!(ground.len(), wide * high, "the ground is the frame's size");
    let mut out = vec![0u8; wide * high];
    let mut ties = vec![Ties::AWAY; wide * high];
    let mut frame = ground;
    let atlas = r.atlas();
    let side = usize::try_from(atlas.side().value()).expect("a side");
    let cside = usize::try_from(atlas.color_side().value()).expect("a side");
    let (mask, color) = (atlas.pixels(), atlas.argb());
    let origins = Chrome::of(r, (Px(w), Px(h))).origins();
    let whole = |o: [f32; 2]| (o[0].floor() as i32, o[1].floor() as i32);
    let grid: Vec<baud_render::Instance> = r.instances().copied().collect();
    let runs = [
        (&grid[..], origins[0]),
        (r.strip_records(), origins[1]),
        (r.lights_records(), origins[2]),
        (r.margin_records(), origins[3]),
    ];
    for (i, origin) in runs
        .iter()
        .flat_map(|(run, o)| run.iter().map(move |i| (i, *o)))
    {
        let ca = u8::try_from(i.color.0 >> 24).expect("a byte");
        let (ox, oy) = whole(origin);
        let (x0, y0) = (i32::from(i.x.0) + ox, i32::from(i.y.0) + oy);
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
                let at = y * wide + x;
                // The source alpha as `glyph.frag` forms it, and the CPU
                // path's source: its colour and alpha, or none where it
                // leaves the pixel as it is.
                let (a, source) = match i.kind {
                    Kind::Mask => {
                        let cov = mask.get(v * side + u).copied().unwrap_or(0);
                        let bent = i.curve.apply(cov);
                        let cpu = baud_domain::premultiply_chan(
                            baud_domain::Chan::from_u8(bent),
                            baud_domain::Chan::from_u8(ca),
                        )
                        .get();
                        (alpha_byte(ca, bent), (cov != 0).then_some((i.color.0, cpu)))
                    }
                    Kind::Color => {
                        let p = color.get(v * cside + u).copied().unwrap_or(0);
                        let pa = u8::try_from(p >> 24).expect("a byte");
                        (pa, (pa != 0).then_some((p, pa)))
                    }
                    Kind::Solid => (ca, Some((i.color.0, ca))),
                    Kind::Quad | Kind::Band | Kind::Particle => {
                        ties[at] = Ties::EFFECT;
                        (128, None)
                    }
                };
                if a != 0 && a != 255 {
                    out[at] = out[at].saturating_add(1);
                }
                if ties[at].effect() {
                    continue;
                }
                if let Some((src, sa)) = source {
                    let (blended, tie) = blend(src, u32::from(sa), frame[at]);
                    frame[at] = blended;
                    ties[at] = ties[at].or(tie);
                }
            }
        }
    }
    Replay {
        covers: out,
        ties,
        frame,
    }
}

/// **`src`'s colour at source alpha `a` over the premultiplied `dst`**, as
/// the CPU path blends it, premultiplied, with where each channel's exact
/// sum lay: a whole source is itself; otherwise each colour channel `(c a +
/// d (255 - a) + 127) / 255` and the alpha `(255 a + d_a (255 - a) + 127) /
/// 255`.
fn blend(src: u32, a: u32, dst: u32) -> (u32, Ties) {
    if a == 255 {
        return (0xff00_0000 | src & 0x00ff_ffff, Ties::AWAY);
    }
    let mut word = 0;
    let mut ties = [Tie::Away; 4];
    for (k, s) in [0u32, 8, 16, 24].into_iter().enumerate() {
        let c = match s {
            24 => 255,
            _ => (src >> s) & 0xff,
        };
        let n = c * a + ((dst >> s) & 0xff) * (255 - a);
        word |= ((n + 127) / 255) << s;
        ties[k] = Tie::of(n);
    }
    (word, Ties(ties))
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
