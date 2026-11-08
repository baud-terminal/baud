//! `flash`: WCAG 2.3.1's flashes, counted on the frames the cursor-trail
//! effects render (note AJ, section 1.3 and section 7).
//!
//! **Every frame is the shipped one.** The driver is `baud-app`'s own
//! `trail.rs`, compiled here from its source (`#[path]` below), so the moves
//! that ignite, retarget inside `REIGNITE` or snap are decided by the code
//! the app runs, on the display link's clock as the app hands it
//! (`HostTime`, a frame every `1 / rate` s). The frame is built as the app's
//! cell path builds it -- the styles synced, the glyph map and the overlay
//! walked, the window copied flat (research decision D4), the trail's records
//! after the overlay's -- and drawn by `baud-present-macos`'s Metal pipelines
//! into an offscreen `BGRA8Unorm` target, read back and handed to
//! [`baud_bench::flash::Census`], which counts the flashes. No shader is
//! reimplemented: what is counted is what the GPU wrote.
//!
//! **The cases** (note AJ section 7): every effect in `Effect::ALL` and the
//! trail off, the control that attributes what the cursor's own jumps flash;
//! a single long move, and the cursor jumping back and forth every 100 ms and
//! every 200 ms for three seconds, both inside `REIGNITE` (334 ms); between
//! two cells a 341 x 256-point window's diagonal apart, the longest run one
//! window holds, or a window's width apart on one row; the default 13 pt and
//! larger fonts at 1x and 2x; 60 and 120 frames a second; the cursor colour
//! the theme's default, or white, black or a saturated red by `OSC 12`; the
//! ground the dark or light theme's page, or `#CD0000` or `#1A0000` behind
//! every cell. Every ground is opaque, unless `FLASH_GLASS=1` (below).
//!
//! **Its controls, read before any verdict.** Before the cases at each size,
//! a block of cells about 30% of a window toggled white and black at 4 Hz
//! through the same path must fail, and at 2 Hz pass, or the run stops. Each
//! case prints what the driver did (moves, frames with an effect, distinct
//! ignitions), how many band records were not in the cursor colour asked
//! for, the most device pixels one frame lit beyond the ground and the
//! cursor's two cells, how many pixels on the frame's edge ever changed (a
//! trail the frame clipped), and how many pixel-frames were not opaque; each
//! combination checks the ground and the cursor's own pixels, exactly, on its
//! first frame. Beside the verdict's area it prints that window's largest
//! connected piece and when its second starts.
//!
//! **The cursor flashes too.** A block cursor jumping between two cells
//! every 100 ms flashes each five times a second with no trail at all (the
//! trail-off arm). Those cells are in the frames, so the verdict's area, the
//! screen's, counts them. The `apart` columns are the same count with the
//! cursor's two cells left out ([`baud_bench::flash::Census::exclude`]),
//! the frames untouched: what the trail flashes beyond the cursor's own,
//! and a lower bound on it, since the trail's pixels over those cells go
//! too.
//!
//! **The smears and a third motion** (note AL section 6): the quad and the
//! sweep, which a config can name, run as arms beside the effects, as does
//! the ghost, by `trail.rs`'s rules as the app calls them; and the
//! cursor also cycles three cells on the diagonal every 100 ms, a first, a
//! third and two thirds of the way in equal whole steps.
//!
//! **Two instruments, by environment.** `FLASH_HASH=1` fills the `fnv`
//! column with FNV-1a over every frame's pixels, so two builds can be shown
//! to draw the same frames byte for byte; `-` without it. `FLASH_PATH=cpu`
//! draws each frame on the CPU path instead, [`Renderer::render_at`], which
//! the app takes under `BAUD_GPU=0` or with no Metal device, and where the
//! app draws every effect from its Rust model. This bench asks only the
//! smears and off of it and refuses an effect arm there, and the `path`
//! column says which path drew the line.
//!
//! **The glass.** `FLASH_GLASS=1` draws every ground through the default
//! config's glass: the page and every background a program sets dimmed to
//! `dim_dark` 0.70 (alpha 179) under the dark theme and `dim_light` 0.60
//! (153) under the light, as `half_theme` sets `bg_alpha`. A luminance read
//! off a translucent pixel is not what a screen shows, so each frame read
//! back is laid over a flat white and a flat black desktop, note AS's
//! bound, the premultiplied pixel plus the desktop at one minus its alpha,
//! and each desktop is counted as its own case, the `desktop` column.
//!
//! `FLASH_TEXT=1` fills every cell with an `X` in the default colour once
//! a combination's controls have read the bare ground, so the trail passes
//! over text.
//!
//! **The flash bound's pixel check** (note AL, option O2). Over every frame
//! the driver marks a repeat pass, against the case's first frame and
//! outside the cursor's cells: `repeat` counts those frames, `under_dl` is
//! the largest change in relative luminance where the darker of the two is
//! below 0.80 (where a change can be a general transition at all),
//! `under_raw` the largest change with no condition, `under_over` the
//! pixel-frames that are a general transition (0.10 or more, the darker
//! below 0.80), `under_red` those that are a red transition (u'v' more than
//! 0.2 apart, either a saturated red), and `over_cap` the trail's records
//! that carry more alpha than the renderer's cap.
//!
//! Usage: `flash [effects] [sizes] [rates] [inks] [grounds] [motions]`, each
//! a comma list or `all`: effects by name, `quad`, `sweep`, `ghost` or
//! `off`; sizes
//! such as `13x1,13x2,32x1,32x2,72x1` (points x scale); rates `60,120`; inks
//! `default,white,black,red`; grounds `dark,light,red,darkred`; motions by
//! the names the lines print (`long-diag`, `every-100ms-across`,
//! `tri-100ms`, ...). One tab-separated line a case on stdout, the sizes and
//! progress on stderr. macOS only.

#[cfg(target_os = "macos")]
#[allow(dead_code, unused_imports)]
#[path = "../../baud-app/src/trail.rs"]
mod trail;

/// `baud-app/src/bell.rs`'s `Moment`, the one item `trail.rs` takes from it:
/// the two methods `trail.rs` calls, `after` (a dwell's deadline) and
/// `elapsed_since`, with the bodies of its `HostTime` and `Instant` impls as
/// written there.
#[cfg(target_os = "macos")]
mod bell {
    use baud_domain::HostTime;
    use core::time::Duration;
    use std::time::Instant;

    pub trait Moment: Copy + Ord {
        fn after(self, d: Duration) -> Self;
        fn elapsed_since(self, earlier: Self) -> Duration;
    }

    impl Moment for HostTime {
        fn after(self, d: Duration) -> HostTime {
            self.plus(d)
        }

        fn elapsed_since(self, earlier: HostTime) -> Duration {
            self.since(earlier).unwrap_or(Duration::ZERO)
        }
    }

    impl Moment for Instant {
        fn after(self, d: Duration) -> Instant {
            self + d
        }

        fn elapsed_since(self, earlier: Instant) -> Duration {
            self.saturating_duration_since(earlier)
        }
    }
}

/// `baud-app/src/caret.rs`'s `Owes`, the other item `trail.rs` names.
#[cfg(target_os = "macos")]
mod caret {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Owes {
        Frame,
        Nothing,
    }
}

#[cfg(target_os = "macos")]
fn main() {
    baud_bench::warm::refuse_while_warm();
    // Imported here rather than at the top of the file: this bench is macOS
    // only, so a top-level `use` is unused on every other target and `-D
    // warnings` makes that an error. A lint for Linux sees it; one on a Mac
    // cannot.
    use baud_bench::flash::{Area, Census, Outcome, Rate, Report, Scale};
    use baud_core::{DynamicColor, Owned, Term};
    use baud_domain::{
        BytePos, CacheGeometry, Chan, Col, Cols, Depth, HostTime, PackedCell, ReadLen, Row, RowRot,
        Rows, WinSize,
    };
    use baud_font::{Face, Px};
    use baud_present_macos::journal::SetIx;
    use baud_present_macos::{
        CellInput, Done, Drawn, Gpu, GridBuffer, MapBuffer, OverlayBuffer, StylesBuffer, Target,
    };
    use baud_render::effect::Effect;
    use baud_render::{
        Argb, Blend, Candidate, GlyphMap, Instance, Kind, Overlay, Points, PxSize, Rect, Renderer,
        Resolved, Scene, Shape, Stack, Strength, StyleStamp, Tally, Theme, TrailLook,
    };
    use core::time::Duration;
    use std::collections::BTreeSet;
    use std::time::Instant;

    /// What draws behind the trail.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Ground {
        /// `Theme::dark()`'s page, `#101010`.
        Dark,
        /// `Theme::light()`'s page, `#EFEFEF`.
        Light,
        /// xterm's `SGR 41` colour, `#CD0000`, as a truecolour background:
        /// the light theme's palette is not xterm's.
        Red,
        /// A dark red, `#1A0000`.
        DarkRed,
    }

    /// The cursor colour the trail is drawn in.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Ink {
        /// No `OSC 12`: the theme's default foreground.
        Default,
        White,
        Black,
        Red,
    }

    /// Which trail, or none.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Arm {
        Off,
        Effect(Effect),
        /// The quad or the sweep, the smears a config can name.
        Smear(Shape),
        /// The ghost: a cell-sized box sweeping from the cell the cursor
        /// left, by `trail.rs`'s rules as the app calls them.
        Ghost,
        /// The positive control: a block toggled white and black at this
        /// many flashes a second, no trail.
        Block(u32),
    }

    /// How the cursor moves: when, and between which two cells.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Motion {
        /// One move from the first cell to the second.
        Long(Path),
        /// Back and forth between them, a move every period, for 3 s.
        Retarget(Duration, Path),
        /// Around three cells on the diagonal -- the first, then a third and
        /// two thirds of the way in equal whole steps -- a move every period,
        /// for 3 s: each move's band meets the last one's but not the one
        /// before.
        Tri(Duration),
    }

    /// Which path draws the frames: the app's cell path on the GPU, or its
    /// CPU path.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Drawer {
        Cells,
        Cpu,
    }

    /// The second cell, from the first.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Path {
        /// A window's diagonal away: 341 points across and 256 down, to the
        /// nearest cell.
        Diagonal,
        /// A window's width away on the same row.
        Across,
    }

    impl Ground {
        fn name(self) -> &'static str {
            match self {
                Ground::Dark => "dark",
                Ground::Light => "light",
                Ground::Red => "red",
                Ground::DarkRed => "darkred",
            }
        }

        fn theme(self) -> Theme {
            match self {
                Ground::Light => Theme::light(),
                _ => Theme::dark(),
            }
        }

        /// The ground's bytes, `0xAARRGGBB`.
        fn word(self) -> u32 {
            match self {
                Ground::Dark => 0xFF10_1010,
                Ground::Light => 0xFFEF_EFEF,
                Ground::Red => 0xFFCD_0000,
                Ground::DarkRed => 0xFF1A_0000,
            }
        }

        /// The rendition that paints it: the page by default, or every
        /// cell's background.
        fn sgr(self) -> &'static str {
            match self {
                Ground::Dark | Ground::Light => "\x1b[0m",
                Ground::Red => "\x1b[0;48;2;205;0;0m",
                Ground::DarkRed => "\x1b[0;48;2;26;0;0m",
            }
        }

        /// The rendition, then the screen erased in it.
        fn bytes(self) -> String {
            format!("{}\x1b[2J", self.sgr())
        }
    }

    impl Ink {
        fn name(self) -> &'static str {
            match self {
                Ink::Default => "default",
                Ink::White => "white",
                Ink::Black => "black",
                Ink::Red => "red",
            }
        }

        /// The `OSC 12` colour, or none.
        fn rgb(self) -> Option<(u8, u8, u8)> {
            match self {
                Ink::Default => None,
                Ink::White => Some((0xFF, 0xFF, 0xFF)),
                Ink::Black => Some((0, 0, 0)),
                Ink::Red => Some((0xFF, 0, 0)),
            }
        }

        /// The fill the trail is drawn in under `theme`, opaque: the `OSC 12`
        /// colour or the default foreground.
        fn fill(self, theme: &Theme) -> u32 {
            match self.rgb() {
                Some((r, g, b)) => {
                    0xFF00_0000 | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
                }
                None => theme.fg.0 | 0xFF00_0000,
            }
        }
    }

    impl Arm {
        fn name(self) -> String {
            match self {
                Arm::Off => "off".to_string(),
                Arm::Effect(e) => format!("{e:?}").to_lowercase(),
                Arm::Smear(s) => format!("{s:?}").to_lowercase(),
                Arm::Ghost => "ghost".to_string(),
                Arm::Block(hz) => format!("block-{hz}hz"),
            }
        }
    }

    impl Motion {
        fn name(self) -> String {
            match self {
                Motion::Long(path) => format!("long-{}", path.name()),
                Motion::Retarget(p, path) => format!("every-{}ms-{}", p.as_millis(), path.name()),
                Motion::Tri(p) => format!("tri-{}ms", p.as_millis()),
            }
        }

        fn path(self) -> Path {
            match self {
                Motion::Long(path) | Motion::Retarget(_, path) => path,
                Motion::Tri(_) => Path::Diagonal,
            }
        }
    }

    impl Path {
        fn name(self) -> &'static str {
            match self {
                Path::Diagonal => "diag",
                Path::Across => "across",
            }
        }
    }

    /// The cell path's buffers, the terminal and the renderer: one
    /// combination of size, ground and cursor colour.
    struct World<'f, 'a, 't> {
        gpu: Gpu,
        renderer: Renderer<'f, 'a>,
        term: Term<'t>,
        styles: StylesBuffer,
        styles_mem: &'static mut [Resolved],
        map_buffer: MapBuffer,
        map: GlyphMap<'static>,
        overlay: OverlayBuffer,
        overlay_mem: &'static mut [u8],
        copy: GridBuffer,
        copy_mem: &'static mut [PackedCell],
        exceptions: Overlay,
        records: Vec<Instance>,
        candidates: Vec<Candidate>,
        stamp: StyleStamp,
        target: Target,
        pixels: Vec<u32>,
        drawer: Drawer,
    }

    impl World<'_, '_, '_> {
        fn feed(&mut self, bytes: &[u8]) {
            self.term.write_at(BytePos::ZERO, bytes);
            self.term
                .feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        }

        fn go(&mut self, (row, col): (Row, Col)) {
            let cup = format!("\x1b[{};{}H", row.index() + 1, col.index() + 1);
            self.feed(cup.as_bytes());
        }

        /// The cursor's rectangle, as the app hands it to the driver.
        fn cursor_rect(&self) -> Rect {
            let (r, c) = self.term.cursor();
            self.renderer.cursor_rect(r, c, self.term.cursor_style())
        }

        /// **One frame as the app's cell path builds it**, with `look` as
        /// its trail, drawn and read back into `pixels`.
        fn draw(&mut self, look: Option<TrailLook>) {
            let back = Rows::ZERO;
            self.renderer.set_highlight(None);
            self.renderer.set_scrollbar(None);
            self.renderer.set_screen_fade(None);
            self.renderer.set_trail(look);
            if self.drawer == Drawer::Cpu {
                // The CPU path, whole: every pixel repainted, as a frame
                // with no damage carried over.
                let (fw, fh) = self.renderer.frame_size();
                let mut target =
                    baud_render::Target::new(&mut self.pixels, fw, fw, fh).expect("a CPU target");
                self.renderer.render_at(
                    &mut self.term,
                    back,
                    &mut target,
                    baud_render::Damage::Full,
                );
                return;
            }
            self.renderer.set_images(&self.term, back);
            self.renderer
                .sync_styles(&self.term, self.styles_mem, &mut self.stamp);
            let view = self.term.view(back);
            self.map.ensure(
                &self.term,
                view,
                self.styles_mem,
                &mut self.renderer,
                &mut self.candidates,
            );
            let cursor = (self.term.cursor_visible() && self.term.looking_back(back).index() == 0)
                .then(|| {
                    let (r, c) = self.term.cursor();
                    (r, c, self.term.cursor_style())
                })
                .filter(|&(_, _, style)| self.renderer.cursor_drawn(style));
            let stride = self.term.stride().index();
            let (cols, rows) = (view.cols().index(), view.rows().index());
            for y in 0..rows {
                let src = view.row(Row::from_index(y));
                let at = y * stride;
                self.copy_mem[at..at + cols].copy_from_slice(&src[..cols]);
            }
            self.exceptions.ensure(
                &Scene {
                    term: &self.term,
                    view,
                    styles: self.styles_mem,
                    cursor,
                    highlight: None,
                    map: &self.map,
                    candidates: &self.candidates,
                    pan: Px::of(0),
                    composition: None,
                },
                &mut self.renderer,
                self.overlay_mem,
                &mut self.records,
            );
            let moving_from = self.records.len();
            self.records.extend_from_slice(self.renderer.trail());
            self.records.extend_from_slice(self.renderer.cursor_glow());
            let moving = (moving_from, self.records.len());
            // A `BEL`'s tint, the thumb and the fade are baud's own, drawn
            // over the images.
            let chrome_from = self.records.len();
            self.records.extend_from_slice(self.renderer.bell_tint());
            self.records.extend_from_slice(self.renderer.scrollbar());
            self.records.extend_from_slice(self.renderer.screen_fade());
            let chrome = Tally::of(self.records.len() - chrome_from);
            let input = CellInput {
                set: SetIx(0),
                grid: &self.copy,
                styles: &self.styles,
                map: &self.map_buffer,
                stride: self.term.stride(),
                rotation: RowRot::NONE,
                ring: Rows::of_index(rows),
                cursor,
                cursor_drawn: self.exceptions.cursor_drawn(),
                highlight: None,
                overlay: &self.overlay,
                records: &self.records,
                grounds: &[],
                under: self.exceptions.under(),
                chrome,
                slide: None,
                table: self.stamp,
                map_gen: self.map.generation(),
                reach: self.map.reach(),
                moving,
                // Drawn whole and read back every frame: a screen of its own.
                screen: baud_present_macos::screens::ScreenGen::fresh(),
            };
            let Some(Drawn::Frame(_)) =
                self.gpu
                    .draw_cells(&mut self.renderer, &input, &self.target, Done::Wait)
            else {
                panic!("draw_cells");
            };
            self.gpu.read(&self.target, &mut self.pixels).expect("read");
        }
    }

    /// What a case's run did, beside its count.
    #[derive(Default, Clone)]
    struct Did {
        frames: Tally,
        moves: Tally,
        /// Frames the driver handed an effect.
        vfx: Tally,
        /// Distinct ignitions, by seed.
        ignitions: Tally,
        /// Band records not in the cursor colour asked for.
        off_colour: Tally,
        /// The most pixels one frame lit beyond the ground and the cursor's
        /// two cells.
        lit: Tally,
        /// Pixel-frames on the frame's edge that changed.
        edge: Tally,
        /// Frames the driver marked a repeat pass, and over them the pixel
        /// check: the largest conditioned and raw luminance changes, the
        /// general and the red transitions, and records over the cap.
        repeat: Tally,
        under_dl: f32,
        under_raw: f32,
        under_over: Tally,
        under_red: Tally,
        over_cap: Tally,
    }

    /// The control block's columns and rows: 30% of a window, in cells
    /// `cw` x `ch` device pixels at `s` a point.
    fn block_cells(cw: f64, ch: f64, s: f64) -> (usize, usize) {
        let (cwp, chp) = (cw / s, ch / s);
        let rows = (200.0 / chp).floor().max(1.0);
        let cols = (0.30 * 341.0 * 256.0 / (rows * chp * cwp)).ceil();
        (
            usize::try_from(cols as i64).unwrap(),
            usize::try_from(rows as i64).unwrap(),
        )
    }

    /// One case's line: its size, scale and rate, what it drew, what the
    /// run did and the count.
    fn print_line(
        (pt, scale, rate): (f32, Scale, Rate),
        (ground, ink, arm, motion): (Ground, Ink, Arm, Motion),
        did: &Did,
        rep: &Report,
        t0: Instant,
        (drawer, fnv): (Drawer, Option<u64>),
        (alpha, desk): (Chan, Option<u8>),
    ) {
        let area = |a: Area| format!("{:.0}", a.0);
        let outcome = |o: Outcome| match o {
            Outcome::Passes => "pass",
            Outcome::Fails => "FAIL",
        };
        println!(
            "{pt}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.0}\t{}\t{}\t{}\t{:.4}\t{:.4}\t{}\t{}\t{}\t{}\t{}",
            scale.0,
            rate.0,
            ground.name(),
            ink.name(),
            arm.name(),
            motion.name(),
            did.frames.value(),
            did.moves.value(),
            did.vfx.value(),
            did.ignitions.value(),
            did.off_colour.value(),
            did.lit.value(),
            did.edge.value(),
            rep.translucent.value(),
            rep.general.transitions.0,
            rep.general.flashes.0,
            rep.general.chained.0,
            area(rep.general.over),
            area(rep.general.ever),
            rep.red.transitions.0,
            rep.red.flashes.0,
            rep.red.chained.0,
            area(rep.red.over),
            area(rep.red.ever),
            rep.red_encoded.flashes.0,
            rep.red_encoded.chained.0,
            area(rep.area),
            area(rep.contiguous),
            rep.second,
            area(rep.strict),
            outcome(rep.outcome()),
            outcome(rep.strict_outcome()),
            area(rep.apart),
            area(rep.apart_strict),
            outcome(rep.apart_outcome()),
            outcome(rep.apart_strict_outcome()),
            t0.elapsed().as_secs_f64() * 1e3,
            match drawer {
                Drawer::Cells => "cells",
                Drawer::Cpu => "cpu",
            },
            fnv.map_or_else(|| "-".to_string(), |h| format!("{h:016x}")),
            did.repeat.value(),
            did.under_dl,
            did.under_raw,
            did.under_over.value(),
            did.under_red.value(),
            did.over_cap.value(),
            alpha.get(),
            match desk {
                None => "-",
                Some(0xFF) => "white",
                Some(_) => "black",
            },
        );
    }

    /// A frame as a desktop shows it: the pixels as read back, or under the
    /// glass laid over a flat desktop `d`, the premultiplied pixel plus the
    /// desktop at one minus its alpha, opaque. The CPU path's pixels are
    /// straight and premultiplied first.
    fn on_desk(px: &[u32], d: Option<u8>, drawer: Drawer) -> Vec<u32> {
        let Some(d) = d else {
            return px.to_vec();
        };
        px.iter()
            .map(|&p| {
                let p = match drawer {
                    Drawer::Cells => p,
                    Drawer::Cpu => baud_domain::premultiply(p),
                };
                let [a, r, g, b] = p.to_be_bytes();
                let f = |c: u8| {
                    let v = u32::from(c) + (u32::from(d) * (255 - u32::from(a)) + 127) / 255;
                    u8::try_from(v.min(255)).unwrap_or(u8::MAX)
                };
                u32::from_be_bytes([0xFF, f(r), f(g), f(b)])
            })
            .collect()
    }

    let mut args = baud_bench::args();
    let list = |a: Option<String>| a.filter(|s| s != "all");
    let effects = list(args.next());
    let sizes = list(args.next()).unwrap_or_else(|| "13x1,13x2,32x1,32x2".to_string());
    let rates = list(args.next()).unwrap_or_else(|| "60,120".to_string());
    let inks = list(args.next());
    let grounds = list(args.next());
    let motion_names = list(args.next());
    let picks =
        |s: &Option<String>, name: &str| s.as_ref().is_none_or(|l| l.split(',').any(|x| x == name));
    let arms: Vec<Arm> = core::iter::once(Arm::Off)
        .chain(Effect::ALL.iter().map(|&e| Arm::Effect(e)))
        .chain([
            Arm::Smear(Shape::Quad),
            Arm::Smear(Shape::Sweep),
            Arm::Ghost,
        ])
        .filter(|a| picks(&effects, &a.name()))
        .collect();
    let sizes: Vec<(f32, u32)> = sizes
        .split(',')
        .map(|s| {
            let (pt, x) = s.split_once('x').expect("a size is POINTSxSCALE");
            (pt.parse().expect("points"), x.parse().expect("scale"))
        })
        .collect();
    let rates: Vec<u32> = rates
        .split(',')
        .map(|r| r.parse().expect("a rate"))
        .collect();
    let inks: Vec<Ink> = [Ink::Default, Ink::White, Ink::Black, Ink::Red]
        .into_iter()
        .filter(|i| picks(&inks, i.name()))
        .collect();
    let grounds: Vec<Ground> = [Ground::Dark, Ground::Light, Ground::Red, Ground::DarkRed]
        .into_iter()
        .filter(|g| picks(&grounds, g.name()))
        .collect();
    let every = |ms, path| Motion::Retarget(Duration::from_millis(ms), path);
    let motions = [
        Motion::Long(Path::Diagonal),
        every(100, Path::Diagonal),
        every(200, Path::Diagonal),
        Motion::Long(Path::Across),
        every(100, Path::Across),
        every(200, Path::Across),
        Motion::Tri(Duration::from_millis(100)),
    ];
    let motions: Vec<Motion> = motions
        .into_iter()
        .filter(|m| picks(&motion_names, &m.name()))
        .collect();
    let hashing = std::env::var("FLASH_HASH").is_ok_and(|v| v == "1");
    let text = std::env::var("FLASH_TEXT").is_ok_and(|v| v == "1");
    let glass = std::env::var("FLASH_GLASS").is_ok_and(|v| v == "1");
    // The desktops a glass frame is laid over, or none.
    let desks: Vec<Option<u8>> = match glass {
        true => vec![Some(0xFF), Some(0)],
        false => vec![None],
    };
    // WCAG's curve a byte at a time, and a word's relative luminance, u'v'
    // and saturated redness, as `baud_bench::flash` reads them: the pixel
    // check's.
    let lin: Vec<f32> = (0u8..=255)
        .map(|b| {
            let c = f64::from(b) / 255.0;
            (if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }) as f32
        })
        .collect();
    let see = |p: u32| -> (f32, f32, f32, bool) {
        let [_, r, g, b] = p.to_be_bytes();
        let (lr, lg, lb) = (
            lin[usize::from(r)],
            lin[usize::from(g)],
            lin[usize::from(b)],
        );
        let uv = |lr: f32, lg: f32, lb: f32| {
            let x = 0.412_456_4 * lr + 0.357_576_1 * lg + 0.180_437_5 * lb;
            let y = 0.212_672_9 * lr + 0.715_152_2 * lg + 0.072_175 * lb;
            let z = 0.019_333_9 * lr + 0.119_192 * lg + 0.950_304_1 * lb;
            let d = x + 15.0 * y + 3.0 * z;
            (d > 0.0).then_some((4.0 * x / d, 9.0 * y / d))
        };
        let (u, v) = uv(lr, lg, lb).unwrap_or_else(|| uv(1.0, 1.0, 1.0).unwrap_or((0.0, 0.0)));
        let sum = lr + lg + lb;
        (
            0.2126 * lr + 0.7152 * lg + 0.0722 * lb,
            u,
            v,
            sum > 0.0 && lr / sum >= 0.8,
        )
    };
    let drawer = match std::env::var("FLASH_PATH").as_deref() {
        Ok("cpu") => Drawer::Cpu,
        Ok("cells") | Err(_) => Drawer::Cells,
        Ok(other) => panic!("FLASH_PATH={other:?} is not cells or cpu"),
    };
    if drawer == Drawer::Cpu {
        assert!(
            arms.iter().all(|a| !matches!(a, Arm::Effect(_))),
            "on the CPU path this bench asks only off, quad, sweep and ghost: drop the effect arms"
        );
    }

    let face_data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [Face::parse(&face_data).expect("parse")];

    println!(
        "pt\tscale\trate\tground\tink\tarm\tmotion\tframes\tmoves\tvfx\tignitions\toff_colour\tlit\tedge\ttranslucent\tgen_trans\tgen_flashes\tgen_chained\tgen_over\tgen_ever\tred_trans\tred_flashes\tred_chained\tred_over\tred_ever\tredenc_flashes\tredenc_chained\tarea\tcontiguous\tsecond\tstrict\toutcome\tstrict_outcome\tapart\tapart_strict\tapart_outcome\tapart_strict_outcome\tms\tpath\tfnv\trepeat\tunder_dl\tunder_raw\tunder_over\tunder_red\tover_cap\tglass\tdesktop"
    );

    let started = Instant::now();
    for &(pt, scale) in &sizes {
        let px = PxSize::of(pt * f32::from(u16::try_from(scale).unwrap_or(1))).expect("a size");
        let s = f64::from(scale);
        // The cell, from a probe at the size.
        let probe = Renderer::new(
            Stack::new(&faces).unwrap(),
            px,
            Theme::dark(),
            Blend::Native,
            Cols::of_index(10),
            Rows::of_index(10),
        )
        .unwrap();
        let m = probe.metrics();
        let (cw, ch) = (f64::from(m.cell_w.value()), f64::from(m.cell_h.value()));
        let cell = (
            f32::from(u16::try_from(m.cell_w.value()).unwrap()),
            f32::from(u16::try_from(m.cell_h.value()).unwrap()),
        );
        // The cell's height in points, as the app hands the driver it.
        let height = Points(cell.1 / f32::from(u16::try_from(scale).unwrap_or(1)));
        // The two cells a window's diagonal apart, centre to centre, with a
        // margin on every side for what the effects throw off their path.
        let dc = usize::try_from((341.0 * s / cw).round() as i64).unwrap();
        let dr = usize::try_from((256.0 * s / ch).round() as i64).unwrap();
        let (mc, mr) = (12usize, 6usize);
        let ws = WinSize {
            ws_col: u16::try_from(dc + 2 * mc + 1).unwrap(),
            ws_row: u16::try_from(dr + 2 * mr + 1).unwrap(),
        };
        let a = (Row::from_index(mr), Col::at_index(mc));
        let diagonal = (Row::from_index(mr + dr), Col::at_index(mc + dc));
        let across = (Row::from_index(mr), Col::at_index(mc + dc));
        // The three cells of `tri`: the first, then a third and two thirds of
        // the diagonal in equal whole steps, so every leg is the same move.
        let third = |d: usize| usize::try_from((d as f64 / 3.0).round() as i64).unwrap();
        let (sc, sr) = (third(dc), third(dr));
        let tri = [
            a,
            (Row::from_index(mr + sr), Col::at_index(mc + sc)),
            (Row::from_index(mr + 2 * sr), Col::at_index(mc + 2 * sc)),
        ];
        let (bw, bh) = block_cells(cw, ch, s);
        eprintln!(
            "size {pt} pt x{scale}: cell {cw}x{ch} px, grid {}x{}, path {dc} cols {dr} rows = {:.0} x {:.0} pt; control block {bw}x{bh} cells = {:.0} pt^2",
            ws.ws_col,
            ws.ws_row,
            dc as f64 * cw / s,
            dr as f64 * ch / s,
            (bw * bh) as f64 * cw * ch / (s * s)
        );

        // The positive control first, on the dark ground with no trail, then
        // every combination asked for.
        let mut combos: Vec<(Ground, Ink, Vec<Arm>)> = vec![(
            Ground::Dark,
            Ink::Default,
            vec![Arm::Block(4), Arm::Block(2)],
        )];
        for &g in &grounds {
            for &i in &inks {
                combos.push((g, i, arms.clone()));
            }
        }
        for (ground, ink, case_arms) in combos {
            // One combination: a fresh terminal, renderer and GPU path.
            let mut theme = ground.theme();
            if glass {
                theme.bg_alpha = Chan::from_u8(match ground {
                    Ground::Light => 153,
                    _ => 179,
                });
            }
            let alpha = theme.bg_alpha;
            let fill = ink.fill(&theme);
            let renderer = Renderer::new(
                Stack::new(&faces).unwrap(),
                px,
                theme.clone(),
                Blend::Native,
                Cols::of_index(usize::from(ws.ws_col)),
                Rows::of_index(usize::from(ws.ws_row)),
            )
            .unwrap();
            let gpu = Gpu::new().expect("a Metal device; this bench cannot measure without one");
            let mut owned = Owned::without_surfaces(&ws, &CacheGeometry::UNKNOWN, Depth::of(12));
            let (_grid, cells) = gpu.grid(owned.ring(), owned.stride()).expect("grid");
            let (_alt, alt) = gpu.grid(owned.rows(), owned.stride()).expect("alternate");
            let (copy, copy_mem) = gpu
                .grid(Rows::of_index(usize::from(ws.ws_row) + 1), owned.stride())
                .expect("copy");
            let term = Term::new(owned.storage_over(cells, alt), &ws);
            let (styles, styles_mem) = gpu.styles(&term).expect("styles");
            let (map_buffer, map_mem) = gpu.map().expect("map");
            let (overlay, overlay_mem) = gpu
                .overlay(
                    Cols::of_index(usize::from(ws.ws_col)),
                    Rows::of_index(usize::from(ws.ws_row)),
                )
                .expect("overlay");
            let (fw, fh) = renderer.frame_size();
            let target = gpu.offscreen(fw, fh).expect("target");
            let (w, h) = (
                usize::try_from(fw.value()).unwrap(),
                usize::try_from(fh.value()).unwrap(),
            );
            let mut world = World {
                gpu,
                renderer,
                term,
                styles,
                styles_mem,
                map_buffer,
                map: GlyphMap::over(map_mem),
                overlay,
                overlay_mem,
                copy,
                copy_mem,
                exceptions: Overlay::new(),
                records: Vec::new(),
                candidates: Vec::new(),
                stamp: StyleStamp::default(),
                target,
                pixels: vec![0; w * h],
                drawer,
            };
            // A steady block, the ground, the cursor colour, the first cell.
            world.feed(b"\x1b[2 q");
            world.feed(ground.bytes().as_bytes());
            if let Some((r, g, b)) = ink.rgb() {
                world.feed(format!("\x1b]12;rgb:{r:02x}/{g:02x}/{b:02x}\x07").as_bytes());
            }
            assert_eq!(
                world.term.dynamic_color(DynamicColor::Cursor),
                ink.rgb(),
                "OSC 12 took"
            );
            world.go(a);
            // **The combination's controls**: the ground and the cursor's
            // own pixels, exactly, before any trail.
            world.draw(None);
            // The ground as the target holds it: at the glass's alpha,
            // premultiplied on the GPU and straight on the CPU path.
            let straight = (ground.word() & 0x00FF_FFFF) | (u32::from(alpha.get()) << 24);
            let ground_px = match drawer {
                Drawer::Cells => baud_domain::premultiply(straight),
                Drawer::Cpu => straight,
            };
            let rect = world.cursor_rect();
            let inside = |x: usize, y: usize, r: Rect| {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                fx > r.left && fx < r.right && fy > r.top && fy < r.bottom
            };
            let (mut ground_off, mut cursor_off) = (0usize, 0usize);
            for y in 0..h {
                for x in 0..w {
                    let p = world.pixels[y * w + x];
                    if inside(x, y, rect) {
                        cursor_off += usize::from(p != fill);
                    } else {
                        ground_off += usize::from(p != ground_px);
                    }
                }
            }
            assert_eq!(
                (ground_off, cursor_off),
                (0, 0),
                "{} ground {:08x}, {} cursor {:08x}: pixels off",
                ground.name(),
                ground_px,
                ink.name(),
                fill
            );
            let band = Argb(fill).with_alpha(Chan::from_u8(0xFF));
            // `FLASH_TEXT=1`: every cell an `X` in the default colour over
            // the ground, so the trail passes over text, after the controls
            // have read the bare ground. Not under the control block.
            if text && !case_arms.iter().any(|a| matches!(a, Arm::Block(_))) {
                let mut bytes = ground.sgr().to_string();
                for row in 0..usize::from(ws.ws_row) {
                    bytes.push_str(&format!(
                        "\x1b[{};1H{}",
                        row + 1,
                        "X".repeat(usize::from(ws.ws_col))
                    ));
                }
                world.feed(bytes.as_bytes());
                world.go(a);
            }

            for arm in case_arms {
                let case_motions: Vec<Motion> = match arm {
                    Arm::Block(_) => vec![Motion::Long(Path::Diagonal)],
                    _ => motions.to_vec(),
                };
                for motion in case_motions {
                    for &rate in &rates {
                        let t0 = Instant::now();
                        // The cells the cursor rests at: the first and the
                        // second, or `tri`'s three.
                        let stops: Vec<(Row, Col)> = match motion {
                            Motion::Tri(_) => tri.to_vec(),
                            _ => vec![
                                a,
                                match motion.path() {
                                    Path::Diagonal => diagonal,
                                    Path::Across => across,
                                },
                            ],
                        };
                        let rests: Vec<Rect> = stops
                            .iter()
                            .map(|&s| {
                                world.go(s);
                                world.cursor_rect()
                            })
                            .collect();
                        world.go(a);
                        world.draw(None);
                        let first = world.pixels.clone();
                        let rested = |x: usize, y: usize| rests.iter().any(|&r| inside(x, y, r));
                        // The first frame as each desktop shows it.
                        let firsts: Vec<Vec<u32>> =
                            desks.iter().map(|&d| on_desk(&first, d, drawer)).collect();
                        let first_ls: Vec<Vec<f32>> = firsts
                            .iter()
                            .map(|f| f.iter().map(|&p| see(p).0).collect())
                            .collect();
                        // The schedule: moves from 0.2 s, then time for the
                        // effect to burn out and a full second to close.
                        let (moves, end): (Vec<f64>, f64) = match (arm, motion) {
                            // The control block moves no cursor: it is the
                            // only thing that changes.
                            (Arm::Block(_), _) => (Vec::new(), 2.2),
                            (_, Motion::Long(_)) => (vec![0.2], 2.2),
                            (_, Motion::Retarget(p, _) | Motion::Tri(p)) => {
                                let p = p.as_secs_f64();
                                let n = (3.0 / p).round() as i64;
                                ((0..=n).map(|j| 0.2 + j as f64 * p).collect(), 4.4)
                            }
                        };
                        let mut chase = match arm {
                            Arm::Effect(e) => {
                                Some(trail::Chase::<HostTime>::new(trail::Style::Effect(e)))
                            }
                            Arm::Smear(s) => {
                                Some(trail::Chase::<HostTime>::new(trail::Style::Smear(s)))
                            }
                            _ => None,
                        };
                        let r = f64::from(rate);
                        let frames = (end * r).round() as i64;
                        let mut censuses: Vec<Census> = desks
                            .iter()
                            .map(|_| Census::new(fw, fh, Scale(scale), Rate(rate)))
                            .collect();
                        // The cursor's cells, which it flashes by jumping
                        // between them with or without a trail: left out of the
                        // apart areas, so what is left is what the trail adds.
                        for r in &rests {
                            let px = |v: f32| Px::of(u32::try_from(v.max(0.0) as i64).unwrap_or(0));
                            for census in &mut censuses {
                                census.exclude(
                                    px(r.left),
                                    px(r.top),
                                    px(r.right - r.left),
                                    px(r.bottom - r.top),
                                );
                            }
                        }
                        // The ghost's state as the app keeps it: the cell
                        // the cursor was last seen at, and the sweep in
                        // flight, its two cells and when it started.
                        let mut ghost_last: Option<(Row, Col)> = None;
                        let mut ghost: Option<(trail::GhostSweep, f64)> = None;
                        let mut ghosts = trail::Ledger::<HostTime>::new();
                        let mut did = Did::default();
                        // The pixel check's figures, a desktop each.
                        let mut checks: Vec<Did> = desks.iter().map(|_| Did::default()).collect();
                        let mut seeds: BTreeSet<u32> = BTreeSet::new();
                        let mut next = 0usize;
                        let mut at = 0usize;
                        // FNV-1a over every frame's words, little-endian.
                        let mut fnv: u64 = 0xcbf2_9ce4_8422_2325;
                        let mut block_on = None::<bool>;
                        for k in 0..frames {
                            let now = k as f64 / r;
                            if let Arm::Block(hz) = arm {
                                // The block, white then black, `hz` times a
                                // second, below and right of the cursor.
                                let on = ((now * 2.0 * f64::from(hz)).floor() as i64) % 2 == 1;
                                if block_on != Some(on) {
                                    block_on = Some(on);
                                    let bg = if on { "255;255;255" } else { "0;0;0" };
                                    let mut bytes = format!("\x1b[48;2;{bg}m");
                                    for row in 0..bh {
                                        bytes.push_str(&format!(
                                            "\x1b[{};{}H{}",
                                            mr + 2 + row,
                                            mc + 3,
                                            " ".repeat(bw)
                                        ));
                                    }
                                    bytes.push_str(ground.sgr());
                                    world.feed(bytes.as_bytes());
                                    world.go(a);
                                }
                            }
                            while next < moves.len() && moves[next] <= now + 1e-9 {
                                at = (at + 1) % stops.len();
                                world.go(stops[at]);
                                did.moves.bump();
                                next += 1;
                            }
                            let mut look = chase.as_mut().and_then(|c| {
                                c.frame(
                                    Some(world.cursor_rect()),
                                    HostTime::of_secs(1000.0 + now).expect("a time"),
                                    cell,
                                    height,
                                )
                            });
                            if arm == Arm::Ghost {
                                // `app.rs`'s ghost, on the same clock: a
                                // sweep on any move, weighed on its own
                                // ledger, gone once landed.
                                let tick = 1000.0 + now;
                                let seen = Some(world.term.cursor());
                                let rect = |(r, c): (Row, Col)| {
                                    world.renderer.cursor_rect(
                                        r,
                                        c,
                                        baud_core::CursorStyle::SteadyBlock,
                                    )
                                };
                                let at = HostTime::of_secs(tick).expect("a time");
                                if let Some(sw) = trail::ghost_sweep(
                                    &mut ghosts,
                                    ghost_last,
                                    seen,
                                    at,
                                    rect,
                                    (cell, height),
                                ) {
                                    ghost = Some((sw, tick));
                                }
                                ghost_last = seen;
                                look = ghost.and_then(|(sw, start)| {
                                    let p = trail::ghost_progress(start, tick);
                                    (p < 1.0).then_some(TrailLook::Ghost(baud_render::Trail {
                                        from: sw.from,
                                        to: sw.to,
                                        progress: p.max(0.0),
                                        origin_dy: 0.0,
                                        lift: Px::of(0),
                                        strength: sw.strength,
                                    }))
                                });
                                if look.is_none() {
                                    ghost = None;
                                }
                            }
                            match look {
                                Some(TrailLook::Vfx(v)) => {
                                    did.vfx.bump();
                                    seeds.insert(v.ignition.seed.0);
                                }
                                Some(TrailLook::Smear(_) | TrailLook::Ghost(_)) => did.vfx.bump(),
                                _ => {}
                            }
                            let repeat = match look {
                                Some(TrailLook::Vfx(v)) => v.emitter.strength,
                                Some(TrailLook::Smear(s)) => s.strength,
                                Some(TrailLook::Ghost(t)) => t.strength,
                                None => Strength::Full,
                            } == Strength::UnderThreshold;
                            world.draw(look);
                            for rec in world.renderer.trail() {
                                // The colour, whatever alpha a repeat's cap
                                // left it.
                                if rec.kind == Kind::Band
                                    && rec.color.with_alpha(Chan::from_u8(0xFF)) != band
                                {
                                    did.off_colour.bump();
                                }
                            }
                            if repeat {
                                let k = world.renderer.flash_cap().get();
                                did.repeat.bump();
                                for rec in world.renderer.trail() {
                                    if rec.color.a().get() > k {
                                        did.over_cap.bump();
                                    }
                                }
                            }
                            // What the trail lit: pixels off the case's first
                            // frame outside the cursor's two cells.
                            let (mut lit, mut edge) = (0usize, 0usize);
                            for y in 0..h {
                                for x in 0..w {
                                    let i = y * w + x;
                                    if world.pixels[i] == first[i] {
                                        continue;
                                    }
                                    if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
                                        edge += 1;
                                    }
                                    if rested(x, y) {
                                        continue;
                                    }
                                    lit += 1;
                                }
                            }
                            // Each desktop's frame, and over a repeat frame
                            // the pixel check: against the first frame, by
                            // luminance and by the red rule.
                            let seen: Vec<Vec<u32>> = desks
                                .iter()
                                .map(|&d| on_desk(&world.pixels, d, drawer))
                                .collect();
                            for (j, frame) in seen.iter().enumerate() {
                                if !repeat {
                                    break;
                                }
                                let (first, first_l, check) =
                                    (&firsts[j], &first_ls[j], &mut checks[j]);
                                for (i, (&p, &q)) in frame.iter().zip(first).enumerate() {
                                    if p == q || rested(i % w, i / w) {
                                        continue;
                                    }
                                    let (l, u, v, red) = see(p);
                                    let (_, fu, fv, fred) = see(q);
                                    let dl = (l - first_l[i]).abs();
                                    check.under_raw = check.under_raw.max(dl);
                                    if l.min(first_l[i]) < 0.80 {
                                        check.under_dl = check.under_dl.max(dl);
                                        if dl >= 0.10 {
                                            check.under_over.bump();
                                        }
                                    }
                                    if (red || fred) && (u - fu).hypot(v - fv) > 0.2 {
                                        check.under_red.bump();
                                    }
                                }
                            }
                            if Tally::of(lit).value() > did.lit.value() {
                                did.lit = Tally::of(lit);
                            }
                            did.edge.add(Tally::of(edge));
                            if hashing {
                                for &p in &world.pixels {
                                    for byte in p.to_le_bytes() {
                                        fnv ^= u64::from(byte);
                                        fnv = fnv.wrapping_mul(0x0000_0100_0000_01b3);
                                    }
                                }
                            }
                            for (census, frame) in censuses.iter_mut().zip(&seen) {
                                census.frame(frame);
                            }
                            did.frames.bump();
                        }
                        did.ignitions = Tally::of(seeds.len());
                        if let Arm::Block(_) = arm {
                            // Put the ground back under the block.
                            world.feed(ground.bytes().as_bytes());
                            world.go(a);
                        }
                        for ((census, check), &desk) in
                            censuses.into_iter().zip(&checks).zip(&desks)
                        {
                            let rep = census.finish();
                            let mut did = did.clone();
                            did.under_dl = check.under_dl;
                            did.under_raw = check.under_raw;
                            did.under_over = check.under_over;
                            did.under_red = check.under_red;
                            print_line(
                                (pt, Scale(scale), Rate(rate)),
                                (ground, ink, arm, motion),
                                &did,
                                &rep,
                                t0,
                                (drawer, hashing.then_some(fnv)),
                                (alpha, desk),
                            );
                            if let Arm::Block(hz) = arm {
                                let want = if hz > 3 {
                                    Outcome::Fails
                                } else {
                                    Outcome::Passes
                                };
                                assert_eq!(
                                    rep.outcome(),
                                    want,
                                    "the positive control at {hz} Hz over {desk:?}: {rep:?}"
                                );
                            }
                        }
                    }
                }
            }
            eprintln!(
                "  {} on {}: done, {:.0} s in",
                ink.name(),
                ground.name(),
                started.elapsed().as_secs_f64()
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    baud_bench::warm::refuse_while_warm();
    eprintln!("flash: macOS only -- it draws through Metal");
}
