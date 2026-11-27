//! `frame_gpu`: the CPU compositor against Metal on the same frames.
//!
//! Replays one recording through the core in `read(2)`-sized reads at
//! 200x50 cells, 14 px, and after every read converts the rows that
//! changed and then either paints them (the `cpu` arm, `Renderer::paint`)
//! or uploads and draws the whole frame on the GPU into an offscreen
//! texture, waiting for completion (the `gpu` arm, `Gpu::draw`). A third
//! arm, `cpu2`, is the CPU arm again -- the A/A control every sweep here
//! carries. The fourth, `cells`, converts nothing: the shader reads the
//! terminal's own grid, a shared GPU buffer, under its rotation -- the
//! primary's ring, or the alternate screen's buffer while that is up --
//! the rendition table is resolved into another and the glyph map lives
//! in a third, and the shader resolves every cell (`Gpu::draw_cells`);
//! its `convert` column is the map walk and the style resolve. It draws a
//! smaller program (no ligatures, no marks; see `baud_render::GlyphMap`),
//! and the `marks` column counts the cells whose marks it skipped. The
//! fifth, `async`, is that frame committed with a completion handler and
//! never waited for, so its `draw` column is the CPU's whole cost of the
//! bind and commit, and its `wait` column is the time the bench idled
//! until the previous frame completed before it could feed the next read:
//! reading the live grid, it must not feed while a frame is in flight,
//! which is the freeze research decision D4 removed from the app. The
//! `copied` arm reads a copy of the window instead, and the `app` arm is
//! the frame loop the app runs since D4: the window laid out in the order
//! UAX #9 gives each row, the copy into a set of its own, the presenter's
//! wheel and surfaces, feeding while a frame is in flight.
//! `FRAME_BIDI=explicit` resets BDSM before the app arm's recording, as
//! `frame`'s does.
//! The arms run interleaved per round, a fresh renderer per arm per round,
//! and every round is printed so the reader discards the first. The
//! `cells` arm prints as `waited`, because that is what it is: the cell
//! path with `waitUntilCompleted` after the commit, kept so the bench can
//! read the GPU's own start-to-end time -- a measurement arm, and not how
//! the app runs.
//!
//! What it does not measure: the present. The CPU arm's frame is not
//! copied into a surface and the GPU arms' texture is not an IOSurface;
//! both are the same one memcpy away from the screen, and the `present`
//! column is a separate instrument.
//!
//! `FRAME_CARET=switch|plasma|fire` makes the `FRAME_IDLE` frames the
//! frame an idle cursor draws: the cursor shown as a block at the end of
//! the replay, and each frame alternating two phases of that animation,
//! its glow appended to the records as the app appends it. Then every arm
//! draws them, not only the cell arms, and each arm's line of them reports
//! its rows, its CPU and, on a waited arm, its GPU time a frame.
//!
//! Each GPU arm draws as the presenter does since a frame where only the
//! cursor, its blink animation or its trail moved draws only its rows: into
//! the one target as though it were the next of three surfaces, so each
//! frame draws what the frame three back's surface would lack
//! (`baud_present_macos::redraw::Ages`), and the cell arms skip the glyph
//! map's walk over a screen they walked last (`baud_render::WalkKey`), as
//! the app does. The `FRAME_CARET` line counts the rows drawn.
//!
//! `FRAME_WHOLE=on` redraws the whole target on each of those frames,
//! one in flight and the next as soon as it lands, and the waited arm's
//! line reports the GPU time a frame: a whole frame of the final screen.
//!
//! `FRAME_HASH=check` draws each cell arm's last screen once more, whole
//! and outside the timing, reads it back and requires its hash to equal
//! the `copied` arm's: the check that every arm drew the screen it is
//! timed on. `FRAME_SIZE=COLSxROWS` sets the screen drawn, 200x50 by
//! default.
//!
//! `FRAME_TRAIL=n` prices each cursor trail's frame on the waited arm:
//! after its replay, its idle frames and `FRAME_HASH`'s frame, the final
//! screen drawn in pairs, each frame drawn whole and waited for. A pair is
//! an off side, a frame with no trail, then an on side drawn straight
//! after it, and its difference is the on side's GPU start-to-end less the
//! off side's. First come the control's n pairs, whose on side is two
//! frames with no trail, their GPU times summed: a known extra whole frame
//! of work.
//! Then, for each effect in `Effect::ALL` over `baud_bench::trail`'s fixed
//! jump, n pairs whose on side is one frame with that trail. Each kind of
//! pair has a line, the effects named as the config file names them: its
//! pairs, the off and on sides' median GPU time, and the differences'
//! median and range, each kind's first pair dropped as cold. A frame the
//! GPU did not time counts as zero. Each
//! kind's last frame is read back, outside the timing, and the bench exits
//! non-zero if an effect's is the control's, which has no trail, or if any
//! kept control pair's difference is not above zero.
//!
//! `FRAME_IMAGES=n` prices an image store's epoch move on the waited arm:
//! after everything else it draws, two kitty images of `FRAME_IMAGES_PX`
//! square pixels (256 by default) are placed on the final screen by a
//! frame of their own, in no line, and then n rounds of four frames, each
//! drawn whole and waited for, the images placed (`Renderer::set_images`)
//! before the walk as the app places them: `still`, nothing written; `still2`, the same again, the
//! A/A control; `store`, a third image stored and never placed, which
//! moves the store's epoch; and `edit`, both placed images' pixels
//! rewritten, which uploads both -- what every epoch move uploaded when the
//! caches were dropped whole at one. Each kind has a line: its frames,
//! the image snapshots made a frame (each one a texture upload), and the
//! median and range of its upload column (the CPU's building of the frame,
//! texture uploads included), its GPU start-to-end and its placing, each
//! less `still`'s on the same round; the first round is dropped as cold.
//! The bench exits non-zero unless `still` and `still2` snapshot nothing,
//! `store` moves the epoch and snapshots nothing, and `edit` moves it and
//! snapshots exactly two a frame.
//!
//! Usage: `frame_gpu <recording> [rounds] [full|incremental]`. macOS only.

#[cfg(target_os = "macos")]
fn main() {
    baud_bench::warm::refuse_while_warm_on_gpu();
    // Imported here rather than at the top of the file: this bench is macOS
    // only, so a top-level `use` is unused on every other target and `-D
    // warnings` makes that an error. A lint for Linux sees it; one on a Mac
    // cannot.
    use baud_core::{Depth, Owned, Screen, Term, grid_stride};
    use baud_domain::PackedCell;
    use baud_domain::{
        BytePos, Cols, FrameGen, ReadLen, Row, RowBlock, RowRot, Rows, SwapEpoch, WinSize,
    };
    use baud_font::{Face, Px};
    use baud_present_macos::journal::{FrameTag, Journal, SetIx, Wheel};
    use baud_present_macos::redraw::{Ages, SURFACES, SurfaceIx};
    use baud_present_macos::screens::{Built, Compare, Screens, Taking};
    use baud_present_macos::{
        CellInput, Completion, Deadline, Done, Drawn, Gpu, InFlight, Presenter, StillBusy,
        SurfaceHandle, SurfaceSpace, Wake,
    };

    /// **A wait for the GPU to go idle, held to its bound**: past
    /// [`Deadline::WAIT`] the arm fails, naming every count.
    fn went_idle(waited: Result<(), StillBusy>) {
        if let Err(e) = waited {
            panic!("{e}");
        }
    }
    use baud_render::effect::Effect;
    use baud_render::{
        Blend, Candidate, CopyOrder, CursorPhase, Damage, GlyphMap, Instance, Leaving, Lend, Lent,
        LinkLine, Overlay, PxSize, Redraw, Renderer, Scene, Stack, StyleStamp, Tally, Target,
        Theme, WalkKey, Words,
    };

    /// **Every `OSC 8` link on the screen, as runs of one rendition**: what
    /// `FRAME_STRIKE=osc8` strikes, read from the recording's own links.
    fn osc8_runs(t: &Term<'_>, view: &baud_domain::Viewport<'_>) -> Vec<LinkLine> {
        let mut out = Vec::new();
        for y in 0..view.rows().index() {
            let row = Row::from_index(y);
            let cells = view.row(row);
            let n = cells.len().min(view.cols().index());
            let mut x = 0;
            while x < n {
                let c = cells[x];
                if c.flags.link().is_none() {
                    x += 1;
                    continue;
                }
                let (link, word, from) = (c.flags.link(), t.styles().one().word_of(c), x);
                while x + 1 < n
                    && cells[x + 1].flags.link() == link
                    && t.styles().one().word_of(cells[x + 1]) == word
                {
                    x += 1;
                }
                out.push(LinkLine {
                    row,
                    from: baud_domain::Col::at_index(from),
                    to: baud_domain::Col::at_index(x),
                    word,
                });
                x += 1;
            }
        }
        out
    }
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    let mut args = baud_bench::args();
    let which = args.next().unwrap_or_else(|| "btop".to_string());
    let rounds: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let damage = match args.next().as_deref() {
        Some("full") => Damage::Full,
        _ => Damage::Incremental,
    };
    let name = match which.as_str() {
        "btop" => "utf8/btop.raw".to_string(),
        "pipes" => "ascii/pipes.raw".to_string(),
        "cat-scroll" | "cat" => "ascii/cat-scroll.raw".to_string(),
        "git-log" => "ascii/git-log.raw".to_string(),
        "less-pager" | "less" => "ascii/less-pager.raw".to_string(),
        "vim" | "vim-unicode" => "utf8/vim-unicode.raw".to_string(),
        "cat-cjk" => "utf8/cat-cjk.raw".to_string(),
        "less-cjk" => "utf8/less-cjk.raw".to_string(),
        "cava" => "utf8/cava.raw".to_string(),
        other => other.to_string(),
    };
    // An absolute path is read as it is, so a payload that is not a
    // recording -- a text-drawn picture placed the way a file manager
    // places it -- can be replayed without joining the corpus.
    let path = if name.starts_with('/') {
        name.clone()
    } else {
        format!(
            "{}/../../tools/tests/recordings/{name}",
            env!("CARGO_MANIFEST_DIR")
        )
    };
    let data = std::fs::read(&path).expect("recording");
    // `FRAME_GLASS`: the window's dim, which `Theme::dark()` leaves at full
    // alpha -- where `[glass] pictures`'s finder never runs, so no default
    // arm can price it. `dim` is the app's default until 2026-10-17 (0.22
    // of 255, every background dimmed, pictures found and kept solid; the
    // default is 0.70 since), kept so its recorded figures re-run; `dim-nopictures`
    // is the same with `pictures = "dim"`, the finder off.
    let glass = std::env::var("FRAME_GLASS").unwrap_or_else(|_| "off".to_string());
    let theme = match glass.as_str() {
        "off" => Theme::dark(),
        "dim" | "dim-nopictures" => Theme {
            bg_alpha: baud_domain::Chan::from_u8(56),
            app_bg: baud_render::AppBg::Dimmed,
            picture_bg: if glass == "dim" {
                baud_render::PictureBg::Opaque
            } else {
                baud_render::PictureBg::Dimmed
            },
            ..Theme::dark()
        },
        other => panic!("FRAME_GLASS={other:?} is not off, dim or dim-nopictures"),
    };
    println!("glass: {glass}");
    let stack_spec = std::env::var("FRAME_FACES").unwrap_or_else(|_| {
        format!(
            "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let files: Vec<(Vec<u8>, u32)> = stack_spec
        .split(',')
        .map(|spec| {
            let (path, index) = spec.split_once('#').unwrap_or((spec, "0"));
            (
                std::fs::read(path).expect("face file"),
                index.parse().unwrap_or(0),
            )
        })
        .collect();
    let faces: Vec<Face<'_>> = files
        .iter()
        .map(|(data, i)| Face::parse_index(data, baud_font::FaceIndex::of(*i)).unwrap())
        .collect();
    // `FRAME_SIZE=400x150`: the screen drawn, 200x50 by default. It sizes
    // the core, the renderer, the target and the copy alike, and the
    // storage too unless `FRAME_GRID` provisions it apart.
    let ws = std::env::var("FRAME_SIZE").map_or(
        WinSize {
            ws_row: 50,
            ws_col: 200,
        },
        |g| {
            let (c, r) = g.split_once('x').expect("FRAME_SIZE is COLSxROWS");
            WinSize {
                ws_row: r.parse().expect("rows"),
                ws_col: c.parse().expect("cols"),
            }
        },
    );
    // `FRAME_GRID=400x150`: provision the storage, and so the rendition
    // table and the GPU's grid, at that size while the screen stays
    // `FRAME_SIZE`.
    // The app provisions `MAX_GRID`, 400x150, which sizes the table to
    // 65,536 slots where the screen alone gives 16,384 -- so without this the
    // bench prices a quarter of the app's style resolve.
    let prov = std::env::var("FRAME_GRID").map_or(WinSize { ..ws }, |g| {
        let (c, r) = g.split_once('x').expect("FRAME_GRID is COLSxROWS");
        WinSize {
            ws_row: r.parse().expect("rows"),
            ws_col: c.parse().expect("cols"),
        }
    });
    // `FRAME_PX`: the size in device pixels, 14 by default; the app on a
    // Retina panel draws 14 pt at 28.
    let px: f32 =
        std::env::var("FRAME_PX").map_or(14.0, |v| v.parse().expect("FRAME_PX is a size"));
    println!("px: {px}");
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(px).unwrap(),
            theme.clone(),
            Blend::Native,
            Cols::of_index(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .unwrap()
    };
    let mut gpu = Gpu::new().expect("a Metal device");
    let probe = fresh();
    let (w, h) = probe.pixel_size();
    let target = gpu.offscreen(w, h).expect("offscreen target");
    let mut pixels = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    println!("memory: {:?}", gpu.unified_memory());
    println!(
        "{:<5} {:<6} {:>7} {:>9} {:>9} {:>9} {:>9} {:>9} {:>8} {:>9} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>9}",
        "round",
        "arm",
        "frames",
        "us/frame",
        "convert",
        "draw",
        "bind",
        "gpu",
        "rows",
        "inst",
        "miss",
        "ptest",
        "cand",
        "win",
        "wcells",
        "wmiss",
        "rec",
        "pict",
        "wait"
    );
    /// One arm's sums over a replay.
    #[derive(Default)]
    struct Sums {
        frames: usize,
        /// The first `FRAME_FIRST` frames of a replay on their own: their
        /// convert, and the map's misses when they ended -- row 8 of note N
        /// is about the first frames of a page, and a replay's median hides
        /// them.
        first_frames: Tally,
        first_convert: Duration,
        first_miss: Tally,
        total: Duration,
        convert: Duration,
        /// The paint on the CPU arm; bind plus commit on the GPU arms.
        draw: Duration,
        bind: Duration,
        gpu: Duration,
        rows: u64,
        inst: u64,
        miss: Tally,
        ptest: Tally,
        cand: Tally,
        win: Tally,
        wcells: Tally,
        wmiss: Tally,
        rec: Tally,
        /// The cells `[glass] pictures`'s finder marked as a picture, which
        /// is what says it ran: zero under `FRAME_GLASS=off`.
        pict: Tally,
        wait: Duration,
        /// Where the cell arms' CPU goes, a frame after a read and a frame
        /// with nothing read (`FRAME_IDLE`), apart; the third is
        /// `FRAME_HASH`'s frame, which is reported in neither.
        parts: [Parts; 3],
    }
    /// One kind of cell-path frame, split by what it does: the glyph-map
    /// walk, the rendition table synced (`Renderer::sync_styles`, the slots
    /// written since the last frame), the overlay walk, the window
    /// copied (the `copied` arm only; the app copies every frame), and the
    /// bind plus commit.
    #[derive(Default, Clone, Copy)]
    struct Parts {
        frames: Tally,
        map: Duration,
        resolve: Duration,
        overlay: Duration,
        copy: Duration,
        draw: Duration,
        /// A waited arm's GPU start-to-end, and the CPU and instance arms'
        /// convert and rows, for the `FRAME_CARET` line.
        gpu: Duration,
        convert: Duration,
        rows: Tally,
        /// The rows the frames drew by, over the frames drawn by rows.
        drawn_rows: Tally,
        partial: Tally,
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Arm {
        Cpu,
        Gpu,
        Cpu2,
        Cells,
        Async,
        /// The cell path with the grid copied into a second shared buffer
        /// before each draw: the arm that says whether reading the core's
        /// grid in place costs the GPU anything.
        Copied,
        /// The cell path committed asynchronously with the surface set
        /// replaced **while a frame is in flight** -- what a live resize or
        /// a zoom does, because `relayout` reaches `present`, whose rebuild
        /// branch fires before the busy check.
        ///
        /// It answers the question the other arms cannot: how many frames
        /// reach the layer carrying an image drawn for surfaces that have
        /// since been freed and rebuilt. Real GPU completions on real
        /// hardware, arriving when the hardware says so; only the rebuild
        /// is scripted, and it is scripted to the shape the app produces.
        Present,
        /// The `async` cell frame plus what the app's smooth-scroll slide
        /// adds: the leaving row copied in its drawn order
        /// (`Renderer::copy_leaving`), the map ensured over that row as
        /// well as the screen and the overlay's walk over it
        /// (`Overlay::ensure_leaving`, `Overlay::leaving`), and the screen
        /// walked every frame. So `slide - async` is the per-frame CPU cost
        /// a slide adds; the +1-row draw ((rows+1)/rows) is not modelled.
        Slide,
        /// **The frame loop as the app runs it**: a [`Presenter`] over its
        /// own GPU, its three surfaces and its wheel of two buffer sets;
        /// each frame built into the set the wheel hands out, the window
        /// copied into that set's own buffer by `Screens::take` -- the
        /// app's code, not a copy of it -- and committed with a completion
        /// handler. Nothing spins before a feed: every read is fed, and a
        /// frame is built only when none is in flight, as `pump` builds one
        /// on the completion's wake. Its line counts the frames committed,
        /// the reads that found one in flight, and the copies made.
        ///
        /// What it does not model: the app's refresh-interval gate, which
        /// holds a frame until a display tick unless a key is owed, so this
        /// arm commits as fast as the GPU lands frames.
        App,
    }
    let arms = [
        Arm::Cpu,
        Arm::Gpu,
        Arm::Cpu2,
        Arm::Cells,
        Arm::Async,
        Arm::Copied,
        Arm::Present,
        Arm::Slide,
        Arm::App,
    ];
    // `FRAME_ARMS=async,cells` runs only the arms named, so a profile or a
    // sweep sees one path.
    let wanted = std::env::var("FRAME_ARMS").ok();
    // `FRAME_FIRST=n` reports the first n frames of every replay apart:
    // their convert a frame and the map misses they took, beside the rest.
    // `FRAME_IDLE=n`: after the replay, n more frames on the cell arms and
    // the app arm over the last screen with nothing read but, under
    // `FRAME_CARET`, one DECSCUSR 1 and `?25h` on the first -- the frame a
    // caret blink or a trail step draws, where nothing in the grid, the
    // table or the palette changed. Reported apart: the app arm's line is
    // its CPU a frame.
    let idle_n: usize = std::env::var("FRAME_IDLE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let phase = |opacity: u8, glow: u8| CursorPhase {
        opacity: baud_domain::Chan::from_u8(opacity),
        glow: baud_domain::Chan::from_u8(glow),
        ..CursorPhase::HIDDEN
    };
    let strike = std::env::var("FRAME_STRIKE").as_deref() == Ok("osc8");
    let caret = std::env::var("FRAME_CARET").ok().map(|k| match k.as_str() {
        "switch" => [CursorPhase::HIDDEN, CursorPhase::SHOWN],
        "plasma" => [phase(0, 128), phase(0, 192)],
        "fire" => [
            CursorPhase::fire(baud_domain::Chan::from_u8(70)),
            CursorPhase::fire(baud_domain::Chan::from_u8(80)),
        ],
        other => panic!("FRAME_CARET={other:?} is not switch, plasma or fire"),
    });
    // `FRAME_WHOLE=on`: each `FRAME_IDLE` frame redraws the whole target,
    // back to back: the GPU time a whole frame of the final screen takes,
    // where a replay's frames draw what each read changed.
    let whole = std::env::var("FRAME_WHOLE").is_ok_and(|v| v == "on");
    // `FRAME_HASH=check`: after each cell arm's replay, one more frame of
    // the final screen drawn whole and waited for, outside the timing, its
    // pixels read back and hashed; every cell arm's hash must equal the
    // `copied` arm's in the same round, or the bench exits non-zero. The
    // copy is the screen by construction, so an arm whose hash differs drew
    // something else -- a live arm reading the primary while the alternate
    // screen is up drew a blank one.
    let hash = std::env::var("FRAME_HASH").is_ok_and(|v| v == "check");
    let mut mismatched = false;
    // FNV-1a over a frame's words.
    let fnv = |pixels: &[u32]| {
        pixels.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &p| {
            (h ^ u64::from(p)).wrapping_mul(0x0100_0000_01b3)
        })
    };
    // `FRAME_TRAIL=n`: n pairs of the control and of each effect, a frame
    // with no trail then the pair's on side, on the waited arm after
    // everything else it draws; see the header.
    let trail_n: usize = std::env::var("FRAME_TRAIL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut trail_blank = false;
    let mut trail_control = false;
    /// **A kind of `FRAME_TRAIL` pair**: the control, whose on side is two
    /// frames with no trail, or an effect, whose on side is one frame with
    /// its trail.
    #[derive(Clone, Copy, PartialEq)]
    enum Pair {
        Control,
        Of(Effect),
    }
    /// **Which frame of a `FRAME_TRAIL` pair**: the off side, the first of
    /// the control's two on frames, or the frame that closes the on side.
    #[derive(Clone, Copy, PartialEq)]
    enum Side {
        Off,
        Lead,
        On,
    }
    // `FRAME_IMAGES=n`: n rounds of the four image frames, on the waited
    // arm after the trail's; see the header.
    let images_n: usize = std::env::var("FRAME_IMAGES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let images_px: usize = std::env::var("FRAME_IMAGES_PX")
        .map_or(256, |v| v.parse().expect("FRAME_IMAGES_PX is a size"));
    let mut images_wrong = false;
    /// **A kind of `FRAME_IMAGES` frame**, in the order each round draws
    /// them.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Pic {
        /// The frame that places the two images, drawn before the rounds
        /// and kept in no line.
        Setup,
        Still,
        Still2,
        Store,
        Edit,
    }
    impl Pic {
        const ALL: [Pic; 4] = [Pic::Still, Pic::Still2, Pic::Store, Pic::Edit];
        fn name(self) -> &'static str {
            match self {
                Pic::Setup => "setup",
                Pic::Still => "still",
                Pic::Still2 => "still2",
                Pic::Store => "store",
                Pic::Edit => "edit",
            }
        }
        /// The snapshots a frame of this kind must make.
        fn snapshots(self) -> Tally {
            match self {
                Pic::Setup | Pic::Edit => Tally::of(2),
                Pic::Still | Pic::Still2 | Pic::Store => Tally::ZERO,
            }
        }
        /// Whether its write must move the image store's epoch.
        fn moves(self) -> bool {
            matches!(self, Pic::Setup | Pic::Store | Pic::Edit)
        }
    }
    /// Whether a `FRAME_IMAGES` frame's write moved the image store's
    /// epoch.
    #[derive(Clone, Copy, PartialEq, Eq, Default)]
    enum Epoch {
        #[default]
        Kept,
        Moved,
    }
    /// **One `FRAME_IMAGES` frame**: its upload column, GPU start-to-end,
    /// placing, the snapshots it made and whether its write moved the
    /// store's epoch.
    #[derive(Clone, Copy, Default)]
    struct Shot {
        upload: Duration,
        gpu: Duration,
        place: Duration,
        snapshots: Tally,
        epoch: Epoch,
    }
    /// A kitty graphics command, its RGBA payload in base64 and sent in
    /// chunks of at most 4096 bytes (`m=1` on all but the last).
    fn kitty(keys: &str, rgba: &[u8]) -> Vec<u8> {
        const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut text = Vec::with_capacity(rgba.len().div_ceil(3) * 4);
        for q in rgba.chunks(3) {
            let mut b = [0u8; 3];
            b[..q.len()].copy_from_slice(q);
            let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
            for k in 0..4 {
                match k <= q.len() {
                    true => text.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                    false => text.push(b'='),
                }
            }
        }
        let chunks: Vec<&[u8]> = text.chunks(4096).collect();
        let mut out = Vec::with_capacity(text.len() + 64 * chunks.len());
        for (i, c) in chunks.iter().enumerate() {
            let more = u8::from(i + 1 < chunks.len());
            match i {
                0 => out.extend_from_slice(format!("\x1b_G{keys},m={more};").as_bytes()),
                _ => out.extend_from_slice(format!("\x1b_Gm={more};").as_bytes()),
            }
            out.extend_from_slice(c);
            out.extend_from_slice(b"\x1b\\");
        }
        out
    }
    // `px` square pixels of one opaque colour, RGBA.
    let square =
        |rgb: [u8; 3]| -> Vec<u8> { [rgb[0], rgb[1], rgb[2], 0xFF].repeat(images_px * images_px) };
    let first_n: usize = std::env::var("FRAME_FIRST")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let arms: Vec<Arm> = arms
        .into_iter()
        .filter(|a| {
            wanted.as_deref().is_none_or(|w| {
                let name = match a {
                    Arm::Cpu => "cpu",
                    Arm::Gpu => "gpu",
                    Arm::Cpu2 => "cpu2",
                    Arm::Cells => "cells",
                    Arm::Async => "async",
                    Arm::Copied => "copied",
                    Arm::Present => "present",
                    Arm::Slide => "slide",
                    Arm::App => "app",
                };
                w.split(',').any(|n| n.trim() == name)
            })
        })
        .collect();
    // The pitch every grid here is laid out at, the storage's and the GPU's,
    // so the copied arm's second buffer is the shape of the first.
    let geom = baud_bench::host_geometry();
    let stride = grid_stride(&geom, Cols::of_index(usize::from(prov.ws_col)));
    // The primary ring is the screen plus the scrollback; the copied arm's
    // buffer holds the screen alone, in screen order, because that is what
    // its copy is: the window, contiguous, under no rotation.
    let rows = Rows::of_index(usize::from(ws.ws_row));
    let ring = Owned::without_surfaces(&prov, &geom, Depth::DEFAULT).ring();
    let (copy_grid, copy_mem) = gpu.grid(rows, stride).expect("a second shared grid");
    let (map_buffer, map_mem) = gpu.map().expect("a shared map");
    let mut map = GlyphMap::over(map_mem);
    let (overlay, overlay_mem) = gpu
        .overlay(Cols::of_index(usize::from(ws.ws_col)), rows)
        .expect("a shared overlay");
    let mut records: Vec<Instance> = Vec::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    // The asynchronous arm's GPU time arrives with the completion, so the
    // handler is where it is taken. A no-op here would report zero for the
    // one arm the app actually runs -- which is exactly what the app did
    // until the completion started carrying the number. The landing also
    // carries *when* the frame ended on the host clock; that is the
    // keystroke instrument's business and this bench, which times frames
    // and not keys, reads only the duration.
    let async_gpu = Arc::new(AtomicU64::new(0));
    let sink = {
        let acc = Arc::clone(&async_gpu);
        // A failed frame has no GPU time; the arm counts it through the
        // GPU's faults instead, and fails on any.
        let f: Arc<dyn Fn(Completion) + Send + Sync> = Arc::new(move |c: Completion| {
            if let Completion::Done(landed) = c {
                acc.fetch_add(
                    u64::try_from(landed.gpu().as_nanos()).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
            }
        });
        f
    };
    // The present arm's state. Shared with every completion, exactly as a
    // presenter shares them with the frames it has in flight.
    let epoch = Arc::new(SwapEpoch::new());
    let journal = Arc::new(Journal::default());
    let mut frame_no = FrameGen::FIRST;
    // How often the surfaces are replaced under a frame in flight. A drag
    // rebuilds on most steps, so every frame is the honest shape; the
    // divisor exists so the arm can also be run at a gentler rate.
    let rebuild_every: usize = std::env::var("FRAME_REBUILD_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    // An arm as its line prints it.
    let label = |a: Arm| match a {
        Arm::Cpu => "cpu",
        Arm::Gpu => "gpu",
        Arm::Cpu2 => "cpu2",
        Arm::Cells => "waited",
        Arm::Async => "async",
        Arm::Copied => "copied",
        Arm::Present => "present",
        Arm::Slide => "slide",
        Arm::App => "app",
    };
    // The app arm's presenter and the memory that outlives a round: the
    // glyph map's buffer, an overlay and a window copy a set, all from the
    // presenter's own GPU, as the app lends them.
    struct Quiet;
    impl Wake for Quiet {
        fn present(&self, _: &SurfaceHandle) {}
        fn wake(&self) {}
    }
    struct AppMemory {
        presenter: Presenter,
        map_buffer: baud_present_macos::MapBuffer,
        map: Option<GlyphMap<'static>>,
        overlays: Vec<baud_present_macos::OverlayBuffer>,
        overlay_mem: Vec<&'static mut [u8]>,
        copies: Vec<baud_present_macos::GridBuffer>,
        copies_mem: Vec<&'static mut [PackedCell]>,
        /// Frames committed through the presenter, every round: the surface
        /// the last went to is the one before the presenter's next.
        commits: Tally,
    }
    // `FRAME_APP_MBS`: the rate the app arm's reads arrive at, in MB/s;
    // 198.6 by default, what the Darwin tty delivers to a reader that does
    // nothing (`transport`'s `null_ceiling`). A replay at memory speed
    // would feed the whole recording under the first frame in flight.
    let app_mbs: f64 = std::env::var("FRAME_APP_MBS")
        .map_or(198.6, |v| v.parse().expect("FRAME_APP_MBS is a rate"));
    let read_every = Duration::from_secs_f64(4096.0 / (app_mbs * 1e6));
    let mut app = arms.contains(&Arm::App).then(|| {
        let presenter = Presenter::with_gpu(w, h, SurfaceSpace::Srgb).expect("a presenter");
        let g = presenter.gpu().expect("the GPU path");
        let (map_buffer, map_mem) = g.map().expect("a shared map");
        let pcols = Cols::of_index(usize::from(prov.ws_col));
        let prows = usize::from(prov.ws_row);
        let (overlays, overlay_mem): (Vec<_>, Vec<_>) = (0..Wheel::SETS)
            .map(|_| g.overlay(pcols, Rows::of_index(prows)).expect("an overlay"))
            .unzip();
        // The provisioned height and the status line's row, as the app's.
        let (copies, copies_mem): (Vec<_>, Vec<_>) = (0..Wheel::SETS)
            .map(|_| g.grid(Rows::of_index(prows + 1), stride).expect("a copy"))
            .unzip();
        AppMemory {
            presenter,
            map_buffer,
            map: Some(GlyphMap::over(map_mem)),
            overlays,
            overlay_mem,
            copies,
            copies_mem,
            commits: Tally::default(),
        }
    });
    let quiet = baud_bench::warm::quiet_begin(baud_bench::warm::Load::CoresAndGpu);
    for round in 0..rounds {
        // The round's final frames, by arm: a hash of the pixels and how
        // many differ from the first, which is the margin's clear.
        let mut hashes: Vec<(Arm, u64, usize)> = Vec::new();
        for &arm in &arms {
            let mut r = fresh();
            if let (Arm::App, Some(m)) = (arm, app.as_mut()) {
                let g = m.presenter.gpu().expect("the GPU path");
                // Kept for the round: the core writes into their memory.
                let (_primary, cells) = g.grid(ring, stride).expect("a shared grid");
                let prows = Rows::of_index(usize::from(prov.ws_row));
                let (_alternate, alt) = g.grid(prows, stride).expect("a shared alternate");
                let mut owned = Owned::without_surfaces(&prov, &geom, Depth::DEFAULT);
                let mut t = Term::new(owned.storage_over(cells, alt), &ws);
                // `FRAME_BIDI=explicit` resets BDSM before the recording, as
                // `frame`'s does.
                if std::env::var("FRAME_BIDI").as_deref() == Ok("explicit") {
                    let bdsm = b"\x1b[8l";
                    t.write_at(BytePos::ZERO, bdsm);
                    t.feed(BytePos::ZERO, ReadLen::from_index(bdsm.len()));
                }
                let (styles, mut styles_mem): (Vec<_>, Vec<_>) = (0..Wheel::SETS)
                    .map(|_| g.styles(&t).expect("shared styles"))
                    .unzip();
                let mut stamps = [StyleStamp::default(); Wheel::SETS];
                let mut map = GlyphMap::over(m.map.take().expect("the map").into_entries());
                let mut exceptions = Overlay::new();
                let mut screens = Screens::new();
                // `FRAME_STRIKE=osc8`: every `OSC 8` link on the screen
                // struck, its text grey through slots each set's table
                // lends, as the app lends them (`baud_render::Lent`).
                let mut lents: Vec<Lent> = (0..Wheel::SETS).map(|_| Lent::default()).collect();
                let mut lent_used = Vec::new();
                let (mut walks, mut idle_walks, mut struck_cells) = (0usize, 0usize, 0usize);
                let (mut windows, mut window_misses) = (Tally::default(), Tally::default());
                let (mut committed, mut busy, mut copies) = (0usize, 0usize, 0usize);
                // Each committed frame's CPU, its copy and its bind plus
                // commit, for medians: the first frame of a round walks a
                // cold map and is reported apart.
                let mut per: Vec<[Duration; 3]> = Vec::new();
                let mut at = 0usize;
                let mut total = None;
                // Whether anything was fed since the last frame was built:
                // the app builds none over a screen nothing touched.
                let mut dirty = false;
                // `FRAME_IDLE`'s frames after the replay, each built over
                // the screen the last one drew with nothing read but, under
                // `FRAME_CARET`, one DECSCUSR 1 and `?25h` on the first, the
                // cursor's blink phase changed, and their CPU a frame.
                let (mut idle_left, mut idling, mut shown) = (idle_n, false, false);
                let mut idle_per: Vec<Duration> = Vec::new();
                // An idle frame's share of the lending, apart, timed only
                // under `FRAME_STRIKE`, so the arm with nothing struck runs
                // the app's path with no clock of the bench's added: the
                // copy put back with its slots, the compare, the strike's
                // lending, and the lent copy the walk and the overlay read.
                let mut idle_parts: Vec<[Duration; 5]> = Vec::new();
                let mut parts = [Duration::ZERO; 5];
                let t0 = Instant::now();
                let mut due = t0;
                loop {
                    if at < data.len() {
                        if Instant::now() >= due {
                            let n = 4096.min(data.len() - at);
                            t.write_at(BytePos::ZERO, &data[at..at + n]);
                            t.feed(BytePos::ZERO, ReadLen::from_index(n));
                            at += n;
                            due += read_every;
                            dirty = true;
                            busy += usize::from(m.presenter.in_flight() == InFlight::Busy);
                        }
                    } else if total.is_none() {
                        // The replay is done: its time is taken, and its
                        // last screen drawn once the frame in flight lands,
                        // untimed, for `FRAME_HASH`.
                        went_idle(m.presenter.idle_within(Deadline::WAIT));
                        total = Some(t0.elapsed());
                    }
                    let fin = total.is_some();
                    if fin && !dirty && idle_left > 0 {
                        went_idle(m.presenter.idle_within(Deadline::WAIT));
                        idle_left -= 1;
                        if let Some(pair) = caret {
                            if !shown {
                                let tail = b"\x1b[1 q\x1b[?25h";
                                t.write_at(BytePos::ZERO, tail);
                                t.feed(BytePos::ZERO, ReadLen::from_index(tail.len()));
                                shown = true;
                            }
                            r.set_cursor_blink(pair[idle_left % 2]);
                        }
                        (dirty, idling) = (true, true);
                    }
                    if fin && !dirty {
                        break;
                    }
                    if !dirty || m.presenter.in_flight() == InFlight::Busy {
                        std::hint::spin_loop();
                        continue;
                    }
                    let tf = Instant::now();
                    let Some(set) = m.presenter.building() else {
                        panic!("one frame in flight and every set held");
                    };
                    dirty = false;
                    let si = set.0;
                    // One write generation closed a frame, before anything
                    // reads, as the app's frame closes it.
                    let seal = t.seal_writes();
                    // The ids this set's copy was lent put back and its slots
                    // given back, as the app's frame does first.
                    let tb = strike.then(Instant::now);
                    lents[si].restore(m.copies_mem[si], t.stride());
                    r.give_back(&mut styles_mem[si][..], &mut stamps[si], &mut lents[si]);
                    parts[0] = tb.map_or(Duration::ZERO, |t| t.elapsed());
                    r.sync_styles(&t, &mut styles_mem[si][..], &mut stamps[si]);
                    // The window laid out in the order each row is drawn
                    // in, once a frame, as the app lays it out.
                    let joins_seen = r.joins_seen();
                    let rejoin = r.lay_out_bidi_copied(&t, Rows::ZERO);
                    let view = t.viewport();
                    let status = t.status_line();
                    let lined = usize::from(status.is_some());
                    let held = screens.holds(set);
                    if strike {
                        r.set_link_strikes(&osc8_runs(&t, &view), &[]);
                    }
                    let tc = Instant::now();
                    screens.compare = Compare::for_rejoin(rejoin);
                    let undo = screens
                        .last_set()
                        .and_then(|l| lents.get(l.0))
                        .map_or(&[][..], Lent::rewrites);
                    let tk = strike.then(Instant::now);
                    // The core's view, every row as stored: none drawn in
                    // another order and no display held.
                    let built = screens.take_lent(
                        &mut m.copies_mem,
                        set,
                        &Taking {
                            term: &t,
                            seal,
                            back: Rows::ZERO,
                            view: &view,
                            status: status.as_ref(),
                            orders: r.bidi(),
                            stride: t.stride(),
                            leaving: Leaving::None,
                        },
                        undo,
                    );
                    // The struck cells lent their grey, and the frame the
                    // last's only under the same lending, as the app has it.
                    // The struck cells are the renderer's kept ones, taken
                    // and put back as `cell_frame::build_with` takes them; the
                    // copy's rows are still collected into a `Vec` each frame
                    // and lent through `Renderer::lend`, not in place as
                    // `build` lends them, so this arm's lend time carries
                    // that `Vec`.
                    let vrows = view.rows().index();
                    parts[1] = tk.map_or(Duration::ZERO, |t| t.elapsed());
                    let tl = strike.then(Instant::now);
                    let struck =
                        r.take_struck(&t, Rows::ZERO, &view, view.rows(), CopyOrder::Stored);
                    parts[4] = tl.map_or(Duration::ZERO, |t| t.elapsed());
                    struck_cells += struck.cells().iter().map(|(_, c)| c.len()).sum::<usize>();
                    let lend = match struck.cells().is_empty() {
                        true => Lend::Nothing,
                        false => {
                            let st = t.stride().index();
                            let cols = view.cols().index();
                            let mut rows: Vec<(&mut [PackedCell], Words<'_>)> = m.copies_mem[si]
                                .chunks_mut(st)
                                .take(vrows + lined)
                                .map(|c| (&mut c[..cols], Words::LIVE))
                                .collect();
                            r.lend(
                                &t,
                                &mut rows,
                                struck.cells(),
                                &mut styles_mem[si][..],
                                &mut stamps[si],
                                &mut lents[si],
                            )
                        }
                    };
                    r.put_struck(struck);
                    parts[2] = tl.map_or(Duration::ZERO, |t| t.elapsed());
                    let tv = strike.then(Instant::now);
                    let lending = lents[si].lending();
                    let built = match built {
                        Built::Last if lending == screens.lending => Built::Last,
                        Built::Last => {
                            screens.built(Built::New);
                            Built::New
                        }
                        Built::New => Built::New,
                    };
                    screens.lending = lending;
                    let view = match lend {
                        Lend::Nothing => view,
                        _ => {
                            lent_used.clear();
                            for y in 0..vrows {
                                let row = Row::from_index(y);
                                let mut e = baud_domain::RowExtent::of(view.cols().index())
                                    .with_rendition(view.rendition(row), view.cols());
                                if view.wrapped(row) == baud_domain::Wrap::Continues {
                                    e = e.wrapping(view.cols().index());
                                }
                                lent_used.push(e);
                            }
                            baud_domain::Viewport::new(
                                &m.copies_mem[si][..],
                                &lent_used,
                                t.stride(),
                                view.cols(),
                                view.rows(),
                                view.rows(),
                                RowRot::NONE,
                                t.styles().one(),
                            )
                        }
                    };
                    parts[3] = tv.map_or(Duration::ZERO, |t| t.elapsed());
                    let dc = tc.elapsed();
                    // The kept joins found again where that compare found
                    // a row changed, as the app finds them.
                    if let Some(how) = rejoin {
                        r.rejoin_bidi(&t, Rows::ZERO, how, |row| screens.changed(row));
                    }
                    let key = WalkKey {
                        table: stamps[si],
                        map: map.generation(),
                        grown: r.atlas_stats().grown,
                        clusters: t.cluster_epoch(),
                        back: Rows::ZERO,
                        cols: view.cols(),
                        rows: view.rows(),
                    };
                    if built == Built::New || screens.walked != Some(key) {
                        walks += 1;
                        idle_walks += usize::from(idling);
                        map.ensure(&t, view, &styles_mem[si][..], &mut r, &mut candidates);
                        screens.walked = Some(WalkKey {
                            map: map.generation(),
                            grown: r.atlas_stats().grown,
                            ..key
                        });
                    }
                    // And again once the walk shaped the first letter that
                    // takes a joining form with BDSM reset, as the app does.
                    r.join_after_walk(&t, Rows::ZERO, joins_seen);
                    let cursor = t.cursor_visible().then(|| {
                        let (row, col) = t.cursor();
                        (row, col, t.cursor_style())
                    });
                    exceptions.ensure(
                        &Scene {
                            term: &t,
                            view,
                            styles: baud_render::RowTables::one(&styles_mem[si][..]),
                            cursor,
                            highlight: None,
                            map: &map,
                            candidates: &candidates,
                            pan: Px::of(0),
                            composition: None,
                        },
                        &mut r,
                        &mut m.overlay_mem[si][..],
                        &mut records,
                    );
                    exceptions.status(
                        &mut r,
                        &t,
                        cursor,
                        (view.cols(), view.rows()),
                        &mut m.overlay_mem[si][..],
                        &mut records,
                    );
                    windows.add(exceptions.stats().windows);
                    window_misses.add(exceptions.stats().window_misses);
                    let input = CellInput {
                        grid: &m.copies[si],
                        styles: &styles[si],
                        map: &m.map_buffer,
                        stride: t.stride(),
                        rotation: RowRot::NONE,
                        ring: Rows::of_index(view.rows().index() + lined),
                        cursor,
                        cursor_drawn: exceptions.cursor_drawn(),
                        highlight: None,
                        overlay: &m.overlays[si],
                        records: &records,
                        grounds: &[],
                        under: exceptions.under(),
                        chrome: Tally::default(),
                        slide: None,
                        set,
                        table: stamps[si],
                        map_gen: map.generation(),
                        reach: map.reach(),
                        moving: (records.len(), records.len()),
                        screen: screens.now(),
                    };
                    let td = Instant::now();
                    let drawn = m.presenter.draw_cells(Quiet, &mut r, &input);
                    let dd = td.elapsed();
                    let df = tf.elapsed();
                    match drawn {
                        Some(Drawn::Frame(_)) => m.commits.bump(),
                        Some(Drawn::Busy) => panic!("built while idle, and refused as busy"),
                        Some(Drawn::GaveUp(why)) => panic!("the GPU was given up: {why:?}"),
                        None => panic!("draw_cells through the presenter"),
                    }
                    if fin {
                        if idling {
                            idle_per.push(df);
                            idle_parts.push(parts);
                        }
                        continue;
                    }
                    committed += 1;
                    copies += usize::from(held != Some(screens.now()));
                    per.push([df, dc, dd]);
                }
                went_idle(m.presenter.idle_within(Deadline::WAIT));
                // A frame the GPU did not draw is no frame to time.
                if let Some(f) = m.presenter.faults() {
                    assert_eq!(f.frames_failed().value(), 0, "{f}");
                }
                m.map = Some(GlyphMap::over(map.into_entries()));
                let total = total.unwrap_or_else(|| t0.elapsed());
                let us = |d: Duration| d.as_secs_f64() * 1e6;
                let median = |k: usize| {
                    let mut v: Vec<Duration> = per.iter().skip(1).map(|f| f[k]).collect();
                    v.sort();
                    v.get(v.len() / 2).copied().map_or(f64::NAN, us)
                };
                let first = |k: usize| per.first().map_or(f64::NAN, |f| us(f[k]));
                println!(
                    "{:<5} {:<6} {:>7} committed, {} reads arrived with one in flight, {} copies, \
{:.1} us of replay a frame; median of the rest cpu {:.1}, copy {:.2}, draw {:.1} us/frame; \
the first cpu {:.1}, copy {:.2}",
                    round,
                    label(arm),
                    committed,
                    busy,
                    copies,
                    us(total) / committed.max(1) as f64,
                    median(0),
                    median(1),
                    median(2),
                    first(0),
                    first(1),
                );
                if !idle_per.is_empty() {
                    let mut v = idle_per.clone();
                    v.sort();
                    println!(
                        "      idle: {} frames over the last screen; median cpu {:.1} us/frame; {} map walks",
                        v.len(),
                        v.get(v.len() / 2).copied().map_or(f64::NAN, us),
                        idle_walks,
                    );
                    let part = |k: usize| {
                        let mut v: Vec<Duration> = idle_parts.iter().map(|p| p[k]).collect();
                        v.sort();
                        v.get(v.len() / 2).copied().map_or(f64::NAN, us)
                    };
                    if strike {
                        println!(
                            "      idle medians: give back {:.1}, compare {:.1}, lend {:.1} (the struck cells {:.1} of it), lent copy {:.1} us",
                            part(0),
                            part(1),
                            part(2),
                            part(4),
                            part(3),
                        );
                    }
                }
                println!(
                    "      walks {walks}, windows {} of which miss {}, struck cells {struck_cells} over the frames",
                    windows.value(),
                    window_misses.value(),
                );
                if hash {
                    // The surface the last frame went to, its rows cut to
                    // the target's width: the presenter's next, one back.
                    let commits = usize::try_from(m.commits.value()).unwrap_or(usize::MAX);
                    let last = (0..(commits + SURFACES - 1) % SURFACES)
                        .fold(SurfaceIx::FIRST, |ix, _| ix.next());
                    let words = m.presenter.surface_pixels(last).expect("the surface");
                    let (wide, high) = (
                        usize::try_from(w.value()).unwrap(),
                        usize::try_from(h.value()).unwrap(),
                    );
                    let pitch = words.len() / high;
                    let px: Vec<u32> = (0..high)
                        .flat_map(|y| words[y * pitch..y * pitch + wide].iter().copied())
                        .collect();
                    let hsh = px.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &p| {
                        (h ^ u64::from(p)).wrapping_mul(0x0100_0000_01b3)
                    });
                    let inked = px.iter().filter(|&&p| p != px[0]).count();
                    hashes.push((arm, hsh, inked));
                }
                continue;
            }
            // The cell arms lend the terminal its viewport from the GPU;
            // the others take the storage's own.
            let cell_arm = matches!(
                arm,
                Arm::Cells | Arm::Async | Arm::Copied | Arm::Present | Arm::Slide
            );
            // Both surfaces' buffers are kept: the live arms bind whichever
            // is up, as the core's rotation and ring follow it.
            let (grid, shared) = if cell_arm {
                let (g, cells) = gpu.grid(ring, stride).expect("a shared grid");
                let prows = Rows::of_index(usize::from(prov.ws_row));
                let (a, alt) = gpu.grid(prows, stride).expect("a shared alternate");
                (Some((g, a)), Some((cells, alt)))
            } else {
                (None, None)
            };
            let mut owned = if cell_arm {
                let owned = Owned::without_surfaces(&prov, &geom, Depth::DEFAULT);
                // `FRAME_IMAGES`' arena: its three images, and a fourth's
                // room for the store frames' replacement of the third.
                match arm == Arm::Cells && images_n > 0 {
                    true => owned
                        .with_images(baud_domain::ImageBudget::of(4 * images_px * images_px + 64)),
                    false => owned,
                }
            } else {
                Owned::with_geometry(&prov, &geom)
            };
            let mut t = match shared {
                Some((cells, alt)) => Term::new(owned.storage_over(cells, alt), &ws),
                None => Term::new(owned.storage(), &ws),
            };
            let (styles, styles_mem) = gpu.styles(&t).expect("shared styles");
            // The resolved table kept as the app keeps a set's: synced from
            // the core's log, so a frame resolves only what was written.
            let mut stamp = StyleStamp::default();
            // A fresh map per arm per round, like the renderer: the entries
            // live in the shared buffer, so the map is remade over it.
            map = GlyphMap::over(map.into_entries());
            let mut exceptions = Overlay::new();
            let mut leaving_row: Vec<PackedCell> = Vec::new();
            let mut s = Sums::default();
            // The surfaces the one target stands for, and what each lacks.
            let mut ages = Ages::new();
            let mut surface = SurfaceIx::FIRST;
            // The screen the last frame was built from, copied into one
            // set as the app's record keeps it: what says a frame's screen
            // is the last's, for the walk and for the GPU's change.
            let mut screens = Screens::new();
            let mut screen =
                vec![PackedCell::EMPTY; stride.index() * (usize::from(prov.ws_row) + 1)];
            let mut at = 0usize;
            let t0 = Instant::now();
            let mut idle_left = if cell_arm || caret.is_some() {
                idle_n
            } else {
                0
            };
            let mut shown = false;
            let mut hash_left = hash && cell_arm;
            // `FRAME_TRAIL`'s frames, in the order drawn: each one's kind of
            // pair, its side and the trail it draws; the control's n pairs
            // first, then each effect's, off and on in turn.
            let trail_plan: Vec<(Pair, Side, Option<Effect>)> = match arm == Arm::Cells {
                true => std::iter::repeat_n(
                    [
                        (Pair::Control, Side::Off, None),
                        (Pair::Control, Side::Lead, None),
                        (Pair::Control, Side::On, None),
                    ],
                    trail_n,
                )
                .flatten()
                .chain(Effect::ALL.into_iter().flat_map(|e| {
                    std::iter::repeat_n(
                        [
                            (Pair::Of(e), Side::Off, None),
                            (Pair::Of(e), Side::On, Some(e)),
                        ],
                        trail_n,
                    )
                    .flatten()
                }))
                .collect(),
                false => Vec::new(),
            };
            let mut trail_at = 0usize;
            // Over each kind of pair, each pair's off and on sides' GPU
            // time, in the order drawn, and its last frame's hash; reserved
            // here, so the frames push without allocating.
            let mut trail_pairs: Vec<(Pair, Vec<Duration>, Vec<Duration>, u64)> =
                match trail_plan.is_empty() {
                    true => Vec::new(),
                    false => std::iter::once(Pair::Control)
                        .chain(Effect::ALL.map(Pair::Of))
                        .map(|p| {
                            (
                                p,
                                Vec::with_capacity(trail_n),
                                Vec::with_capacity(trail_n),
                                0,
                            )
                        })
                        .collect(),
                };
            // The pair being drawn: its off side's GPU time, and the
            // control's first on frame's.
            let mut trail_off = Duration::ZERO;
            let mut trail_lead = Duration::ZERO;
            // `FRAME_IMAGES`' frames, after the trail's: each round's
            // four kinds in turn, each frame's measures kept by kind.
            let images_len = match arm == Arm::Cells && images_n > 0 {
                true => 1 + Pic::ALL.len() * images_n,
                false => 0,
            };
            let mut images_at = 0usize;
            let mut shots: [Vec<Shot>; 4] = core::array::from_fn(|_| Vec::with_capacity(images_n));
            // The frame being drawn's snapshots and epoch before its write,
            // and its placing.
            let mut shot = Shot::default();
            let mut snaps_before = Tally::ZERO;
            let mut total = None;
            while at < data.len()
                || idle_left > 0
                || hash_left
                || trail_at < trail_plan.len()
                || images_at < images_len
            {
                let idle = at >= data.len();
                // `FRAME_HASH`'s frame: the replay and its idle frames done,
                // so the arm's time is taken here, before it.
                let fin = idle && idle_left == 0 && hash_left;
                if fin {
                    hash_left = false;
                    went_idle(gpu.idle_within(Deadline::WAIT));
                    total = Some(t0.elapsed());
                }
                // A `FRAME_TRAIL` frame: the trail it draws, set before the
                // frame is built.
                let lit = (idle && idle_left == 0 && !fin)
                    .then(|| trail_plan.get(trail_at).copied())
                    .flatten();
                // A `FRAME_IMAGES` frame, once the trail's are drawn:
                // its kind, each round's four in turn.
                let pic =
                    (idle && idle_left == 0 && !fin && lit.is_none() && images_at < images_len)
                        .then(|| match images_at {
                            0 => Pic::Setup,
                            i => Pic::ALL[(i - 1) % Pic::ALL.len()],
                        });
                // A priced frame is in none of the arm's columns, and the
                // arm's time is taken before the first.
                let priced = lit.is_some() || pic.is_some();
                if priced && total.is_none() {
                    went_idle(gpu.idle_within(Deadline::WAIT));
                    total = Some(t0.elapsed());
                }
                if pic.is_some() && images_at == 0 {
                    // No trail over the image frames.
                    r.set_trail(None);
                }
                if let Some((_, _, effect)) = lit {
                    let jump = baud_bench::trail::jump(
                        Cols::of_index(usize::from(ws.ws_col)),
                        Rows::of_index(usize::from(ws.ws_row)),
                    );
                    let look = effect.map(|e| baud_bench::trail::look(&r, e, jump));
                    r.set_trail(look);
                }
                if idle && !fin && !priced {
                    idle_left -= 1;
                    if let Some(pair) = caret {
                        if !shown {
                            // A block, shown: a recording may end with the
                            // cursor hidden, as btop's does.
                            let tail = b"\x1b[1 q\x1b[?25h";
                            t.write_at(BytePos::ZERO, tail);
                            t.feed(BytePos::ZERO, ReadLen::from_index(tail.len()));
                            shown = true;
                        }
                        r.set_cursor_blink(pair[idle_left % 2]);
                    }
                }
                let part = match fin || priced {
                    true => 2,
                    false => usize::from(idle),
                };
                if cell_arm {
                    // The bench's stand-in for the frame loop's gate: no
                    // feed while the GPU reads the grid.
                    let tw = Instant::now();
                    went_idle(gpu.idle_within(Deadline::WAIT));
                    if !priced {
                        s.wait += tw.elapsed();
                    }
                }
                if !idle {
                    let n = 4096.min(data.len() - at);
                    t.write_at(BytePos::ZERO, &data[at..at + n]);
                    t.feed(BytePos::ZERO, ReadLen::from_index(n));
                    at += n;
                }
                // A `FRAME_IMAGES` frame's write, outside the timing: the
                // setup frame's two placed images, or what its kind writes,
                // each round in its own colours so every edit and store
                // changes pixels. Fed in `read(2)`-sized pieces, as the
                // replay is.
                if let Some(kind) = pic {
                    let mut bytes = Vec::new();
                    if kind == Pic::Setup {
                        bytes.extend_from_slice(b"\x1b[2;2H");
                        bytes.extend_from_slice(&kitty(
                            &format!("a=T,f=32,s={images_px},v={images_px},i=1,C=1,q=2"),
                            &square([200, 40, 40]),
                        ));
                        bytes.extend_from_slice(b"\x1b[2;100H");
                        bytes.extend_from_slice(&kitty(
                            &format!("a=T,f=32,s={images_px},v={images_px},i=2,C=1,q=2"),
                            &square([40, 40, 200]),
                        ));
                    }
                    let turn = images_at.saturating_sub(1) / Pic::ALL.len();
                    let shade = u8::try_from(turn % 200).unwrap_or(0);
                    match kind {
                        Pic::Setup | Pic::Still | Pic::Still2 => {}
                        Pic::Store => bytes.extend_from_slice(&kitty(
                            &format!("a=t,f=32,s={images_px},v={images_px},i=3,q=2"),
                            &square([shade, 200, 40]),
                        )),
                        Pic::Edit => {
                            for id in [1, 2] {
                                bytes.extend_from_slice(&kitty(
                                    &format!(
                                        "a=f,f=32,s={images_px},v={images_px},x=0,y=0,r=1,i={id},q=2"
                                    ),
                                    &square([shade, 40, 200 - shade]),
                                ));
                            }
                        }
                    }
                    let was = t.images().generation();
                    for piece in bytes.chunks(4096) {
                        t.write_at(BytePos::ZERO, piece);
                        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
                    }
                    shot = Shot {
                        epoch: match t.images().generation() != was {
                            true => Epoch::Moved,
                            false => Epoch::Kept,
                        },
                        ..Shot::default()
                    };
                }
                if cell_arm {
                    // One write generation closed a frame, before anything
                    // reads, as the app's frame closes it.
                    let seal = t.seal_writes();
                    // A `FRAME_IMAGES` frame places the images before the
                    // walk, as the app does on its GPU path.
                    if pic.is_some() {
                        snaps_before = r.image_snapshots();
                        let tp = Instant::now();
                        r.set_images(&t, Rows::ZERO);
                        shot.place = tp.elapsed();
                    }
                    // The styles first, as the app syncs them: the map walk
                    // reads each cell's face style from them.
                    let tr = Instant::now();
                    r.sync_styles(&t, styles_mem, &mut stamp);
                    s.parts[part].resolve += tr.elapsed();
                    let tc = Instant::now();
                    // Faithful to `present`'s slide branch: the leaving row
                    // copied and the map ensured over it first, its walk's
                    // candidates kept for its records, so the screen ensure
                    // below still leaves `candidates` as the screen's -- the
                    // app's order.
                    if arm == Arm::Slide {
                        let cols = t.viewport().cols();
                        leaving_row.resize(cols.index(), PackedCell::EMPTY);
                        r.copy_leaving(&t, Rows::ZERO, &mut leaving_row);
                        let used = [baud_domain::RowExtent::of(cols.index())];
                        let leaving = baud_domain::Viewport::new(
                            &leaving_row,
                            &used,
                            baud_domain::Stride::of_cells(cols.index()),
                            cols,
                            Rows::of_count(1),
                            Rows::of_count(1),
                            RowRot::NONE,
                            t.styles().one(),
                        );
                        exceptions.ensure_leaving(&mut map, &t, leaving, styles_mem, &mut r);
                    }
                    // The walk skipped over the screen it walked last, under
                    // the same key: the app's `Screens`. The slide arm walks
                    // every frame, as a slide does.
                    let vp = t.viewport();
                    let status = t.status_line();
                    let built = screens.take(
                        &mut [&mut screen[..]],
                        SetIx(0),
                        &Taking {
                            term: &t,
                            seal,
                            back: Rows::ZERO,
                            view: &vp,
                            status: status.as_ref(),
                            orders: r.bidi(),
                            stride: t.stride(),
                            leaving: Leaving::None,
                        },
                    );
                    let same = arm != Arm::Slide && built == Built::Last;
                    let key = WalkKey {
                        table: stamp,
                        map: map.generation(),
                        grown: r.atlas_stats().grown,
                        clusters: t.cluster_epoch(),
                        back: Rows::ZERO,
                        cols: vp.cols(),
                        rows: vp.rows(),
                    };
                    if !same || screens.walked != Some(key) {
                        map.ensure(&t, vp, styles_mem, &mut r, &mut candidates);
                        screens.walked = (arm != Arm::Slide).then(|| WalkKey {
                            map: map.generation(),
                            grown: r.atlas_stats().grown,
                            ..key
                        });
                    }
                    s.parts[part].map += tc.elapsed();
                    let cursor = t.cursor_visible().then(|| {
                        let (row, col) = t.cursor();
                        (row, col, t.cursor_style())
                    });
                    let to = Instant::now();
                    exceptions.ensure(
                        &Scene {
                            term: &t,
                            view: t.viewport(),
                            styles: baud_render::RowTables::one(styles_mem),
                            cursor,
                            highlight: None,
                            map: &map,
                            candidates: &candidates,
                            // The bench prices the overlay walk; a settled
                            // frame's records do not slide, and folding a
                            // zero pan costs what folding a real one does.
                            pan: Px::of(0),
                            composition: None,
                        },
                        &mut r,
                        overlay_mem,
                        &mut records,
                    );
                    if arm == Arm::Slide {
                        let vp = t.viewport();
                        exceptions.leaving(
                            &mut r,
                            &t,
                            (&map, baud_render::RowTables::one(styles_mem)),
                            Rows::ZERO,
                            (vp.cols(), vp.rows()),
                            Px::of(0),
                            overlay_mem,
                            &mut records,
                        );
                    }
                    let moving_from = records.len();
                    if lit.is_some() {
                        // The trail's records before the glow, as `present`
                        // appends them.
                        records.extend_from_slice(r.trail());
                    }
                    if caret.is_some() {
                        // The glow over the cursor's cell, after the walk's
                        // records, as `present` appends it.
                        r.set_cursor_glow_at(cursor);
                        records.extend_from_slice(r.cursor_glow());
                    }
                    let moving = (moving_from, records.len());
                    s.parts[part].overlay += to.elapsed();
                    // A priced frame is in none of the arm's columns.
                    if !priced {
                        s.cand.add(Tally::of(candidates.len()));
                        s.win.add(exceptions.stats().windows);
                        s.wcells.add(exceptions.stats().window_cells);
                        s.wmiss.add(exceptions.stats().window_misses);
                        s.rec.add(exceptions.stats().records);
                        s.pict.add(exceptions.stats().pictured);
                    }
                    let tp = Instant::now();
                    if arm == Arm::Copied {
                        let vp = t.viewport();
                        for y in 0..rows.index() {
                            RowBlock::nth(y, t.stride()).of_mut(copy_mem)[..vp.cols().index()]
                                .copy_from_slice(vp.row(Row::from_index(y)));
                        }
                    }
                    s.parts[part].copy += tp.elapsed();
                    let dt = tc.elapsed();
                    s.parts[part].frames.bump();
                    if !idle {
                        s.convert += dt;
                    }
                    if s.frames < first_n && !fin && !priced {
                        s.first_frames.bump();
                        s.first_convert += dt;
                        s.first_miss = map.stats().misses;
                    }
                    let (rotation, ring) = match arm == Arm::Copied {
                        true => (RowRot::NONE, rows),
                        false => (t.rotation(), t.ring()),
                    };
                    // The live grid is the surface that is up: the primary's
                    // ring, or the alternate screen's own buffer after
                    // `?1049h`, which `t.rotation()` and `t.ring()` describe.
                    let (primary, alternate) = grid.as_ref().expect("grid");
                    let input = CellInput {
                        grid: match (arm == Arm::Copied, t.screen()) {
                            (true, _) => &copy_grid,
                            (false, Screen::Normal) => primary,
                            (false, Screen::Alternate) => alternate,
                        },
                        styles: &styles,
                        map: &map_buffer,
                        stride: t.stride(),
                        rotation,
                        ring,
                        cursor,
                        cursor_drawn: exceptions.cursor_drawn(),
                        highlight: None,
                        overlay: &overlay,
                        records: &records,
                        grounds: &[],
                        under: exceptions.under(),
                        chrome: Tally::default(),
                        slide: None,
                        // The bench drives one frame at a time, so a set a
                        // frame would say nothing; the presenter's wheel is
                        // what decides in the app.
                        set: SetIx(0),
                        table: stamp,
                        map_gen: map.generation(),
                        reach: map.reach(),
                        moving,
                        screen: screens.now(),
                    };
                    let done = match arm {
                        _ if fin => Done::Within(Deadline::WAIT),
                        Arm::Async | Arm::Slide => Done::Notify(Arc::clone(&sink)),
                        Arm::Present => {
                            // The identity the frame carries, captured the
                            // way `Presenter::on_done` captures it.
                            frame_no = frame_no.next();
                            let tag = FrameTag {
                                frame: frame_no,
                                swap: epoch.current(),
                                set: None,
                            };
                            let j = Arc::clone(&journal);
                            let e = Arc::clone(&epoch);
                            Done::Notify(Arc::new(move |c| match c {
                                Completion::Done(landed) => {
                                    j.land(tag, &e, landed);
                                }
                                Completion::Failed(_) => j.fail(tag, None),
                            }))
                        }
                        _ => Done::Within(Deadline::WAIT),
                    };
                    let missing = match (idle && whole) || fin || priced {
                        true => Redraw::Whole,
                        false => ages.missing(surface),
                    };
                    let Some(Drawn::Frame(g)) =
                        gpu.draw_cells_since(&mut r, &input, &target, done, missing)
                    else {
                        panic!("draw_cells");
                    };
                    ages.drew(surface, g.changed);
                    surface = surface.next();
                    if fin {
                        let drew = g
                            .waited
                            .landed()
                            .unwrap_or_else(|w| panic!("the last frame: {w}"));
                        gpu.read(&target, drew, &mut pixels).expect("read back");
                        // FNV-1a over the words, and the pixels that are
                        // not the first's: a blank screen is its cursor.
                        let h = fnv(&pixels[..]);
                        let inked = pixels.iter().filter(|&&p| p != pixels[0]).count();
                        hashes.push((arm, h, inked));
                        continue;
                    }
                    if let Some((pair, side, _)) = lit {
                        trail_at += 1;
                        let took = g.gpu.unwrap_or_default();
                        let sums = trail_pairs.iter_mut().find(|k| k.0 == pair);
                        match side {
                            Side::Off => trail_off = took,
                            Side::Lead => trail_lead = took,
                            Side::On => {
                                if let Some(k) = sums {
                                    k.1.push(trail_off);
                                    k.2.push(trail_lead + took);
                                }
                                trail_lead = Duration::ZERO;
                            }
                        }
                        // Each kind's last frame, an on side, read back
                        // outside the timing and after its last pair: what
                        // says the effect drew.
                        if trail_plan.get(trail_at).map(|e| e.0) != Some(pair) {
                            let drew = g
                                .waited
                                .landed()
                                .unwrap_or_else(|w| panic!("a trail frame: {w}"));
                            gpu.read(&target, drew, &mut pixels).expect("read back");
                            if let Some(k) = trail_pairs.iter_mut().find(|k| k.0 == pair) {
                                k.3 = fnv(&pixels[..]);
                            }
                        }
                        continue;
                    }
                    if let Some(kind) = pic {
                        images_at += 1;
                        shot.upload = g.upload;
                        shot.gpu = g.gpu.unwrap_or_default();
                        let made = r.image_snapshots().value() - snaps_before.value();
                        shot.snapshots = Tally::of(usize::try_from(made).unwrap_or(usize::MAX));
                        if let Some(ix) = Pic::ALL.iter().position(|&k| k == kind) {
                            shots[ix].push(shot);
                        }
                        continue;
                    }
                    if let Redraw::Rows(set) = g.drawn {
                        s.parts[part].partial.bump();
                        s.parts[part].drawn_rows.add(Tally::of(set.len().index()));
                    }
                    // `FRAME_REBUILD_EVERY=0` never rebuilds: the control
                    // that says the counter can read zero. Without it a
                    // counter that only ever reported "superseded" would be
                    // making a claim rather than reporting one.
                    if arm == Arm::Present && rebuild_every != 0 && s.frames % rebuild_every == 0 {
                        // **The rebuild, taken while the frame is still in
                        // flight.** This is one line because that is all the
                        // app does: `Presenter::build` makes new surfaces and
                        // bumps the generation, and every frame committed
                        // against the old set is superseded from here.
                        epoch.bump();
                    }
                    s.parts[part].draw += g.upload + g.submit;
                    s.parts[part].gpu += g.gpu.unwrap_or_default();
                    if idle {
                        // An idle frame's GPU time and the idle wait are not
                        // the replay's: they are reported with the parts.
                        async_gpu.swap(0, Ordering::Relaxed);
                        continue;
                    }
                    s.frames += 1;
                    s.draw += g.upload + g.submit;
                    s.bind += g.upload;
                    // A waited frame carries its own GPU time back; an
                    // asynchronous one cannot, because it has not finished
                    // -- its handler has the number and the arm idles until
                    // the completion has run, so what the sink holds here is
                    // the previous frame's. Over a round that costs the last
                    // frame and nothing else.
                    s.gpu += g.gpu.unwrap_or_else(|| {
                        Duration::from_nanos(async_gpu.swap(0, Ordering::Relaxed))
                    });
                    s.inst += g.instances.value();
                    continue;
                }
                let converted = r.convert(&t, Rows::ZERO, damage);
                if idle {
                    // A `FRAME_CARET` frame: reported with the parts.
                    let p = &mut s.parts[part];
                    p.frames.bump();
                    p.convert += converted.convert;
                    p.rows.add(converted.rows_dirty);
                    match arm {
                        Arm::Gpu => {
                            let missing = ages.missing(surface);
                            let Some(Drawn::Frame(g)) = gpu.draw_since(
                                &mut r,
                                &target,
                                Done::Within(Deadline::WAIT),
                                missing,
                            ) else {
                                panic!("draw");
                            };
                            ages.drew(surface, g.changed);
                            surface = surface.next();
                            if let Redraw::Rows(set) = g.drawn {
                                p.partial.bump();
                                p.drawn_rows.add(Tally::of(set.len().index()));
                            }
                            p.draw += g.upload + g.submit;
                            p.gpu += g.gpu.unwrap_or_default();
                        }
                        _ => {
                            let mut target = Target::new(&mut pixels, w, w, h).unwrap();
                            p.draw += r.paint(&mut target, converted).composite;
                        }
                    }
                    continue;
                }
                s.frames += 1;
                s.convert += converted.convert;
                s.rows += converted.rows_dirty.value();
                match arm {
                    Arm::Cpu | Arm::Cpu2 => {
                        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
                        let f = r.paint(&mut target, converted);
                        s.draw += f.composite;
                        s.inst += f.instances.value();
                    }
                    Arm::Gpu => {
                        let missing = ages.missing(surface);
                        let Some(Drawn::Frame(g)) =
                            gpu.draw_since(&mut r, &target, Done::Within(Deadline::WAIT), missing)
                        else {
                            panic!("draw");
                        };
                        ages.drew(surface, g.changed);
                        surface = surface.next();
                        s.draw += g.upload + g.submit;
                        s.bind += g.upload;
                        s.gpu += g.gpu.unwrap_or_default();
                        s.inst += g.instances.value();
                    }
                    Arm::Cells
                    | Arm::Async
                    | Arm::Copied
                    | Arm::Present
                    | Arm::Slide
                    | Arm::App => {
                        unreachable!("the cell arms continue above")
                    }
                }
            }
            went_idle(gpu.idle_within(Deadline::WAIT));
            let faults = gpu.faults();
            assert_eq!(faults.frames_failed().value(), 0, "{faults}");
            s.total = total.unwrap_or_else(|| t0.elapsed());
            if arm == Arm::Present {
                let c = journal.counts();
                println!(
                    "      present: {} landed, {} shown, {} superseded  \
(surfaces replaced under a frame in flight every {} frames)",
                    c.landed, c.shown, c.superseded, rebuild_every
                );
            }
            s.miss = map.stats().misses;
            s.ptest = map.stats().pair_tests;
            let per = |d: Duration| d.as_secs_f64() * 1e6 / s.frames as f64;
            println!(
                "{:<5} {:<6} {:>7} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>8.1} {:>9.1} {:>7} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>9.1}",
                round,
                label(arm),
                s.frames,
                per(s.total),
                per(s.convert),
                per(s.draw),
                per(s.bind),
                per(s.gpu),
                s.rows as f64 / s.frames as f64,
                s.inst as f64 / s.frames as f64,
                s.miss.value(),
                s.ptest.value() as f64 / s.frames as f64,
                s.cand.value() as f64 / s.frames as f64,
                s.win.value() as f64 / s.frames as f64,
                s.wcells.value() as f64 / s.frames as f64,
                s.wmiss.value() as f64 / s.frames as f64,
                s.rec.value() as f64 / s.frames as f64,
                s.pict.value() as f64 / s.frames as f64,
                per(s.wait),
            );
            if r.sized_converted().value() > 0 {
                println!(
                    "      sized rows converted: {}",
                    r.sized_converted().value()
                );
            }
            for (kind, p) in ["after a read", "nothing read"].iter().zip(s.parts) {
                if p.frames.value() == 0 {
                    continue;
                }
                let n = u32::try_from(p.frames.value()).unwrap_or(u32::MAX);
                let us = |d: Duration| (d / n).as_secs_f64() * 1e6;
                if caret.is_some() && kind == &"nothing read" {
                    println!(
                        "      caret: {} frames; {} by rows, {:.2} rows drawn; rows {:.2}, convert {:.1}, map {:.1}, overlay {:.1}, draw {:.1}, gpu {:.1} us/frame",
                        p.frames.value(),
                        p.partial.value(),
                        p.drawn_rows.value() as f64 / p.partial.value().max(1) as f64,
                        p.rows.value() as f64 / f64::from(n),
                        us(p.convert),
                        us(p.map),
                        us(p.overlay),
                        us(p.draw),
                        us(p.gpu),
                    );
                    continue;
                }
                // The GPU's time where the arm waited for it; an asynchronous
                // frame's has not arrived when the frame is counted.
                let gpu = match arm == Arm::Cells {
                    true => format!(", gpu {:.1}", us(p.gpu)),
                    false => String::new(),
                };
                println!(
                    "      {kind}: {} frames; map {:.1}, styles {:.1}, overlay {:.1}, copy {:.1}, draw {:.1}; cpu {:.1}{gpu} us/frame",
                    p.frames.value(),
                    us(p.map),
                    us(p.resolve),
                    us(p.overlay),
                    us(p.copy),
                    us(p.draw),
                    us(p.map + p.resolve + p.overlay + p.copy + p.draw),
                );
            }
            let first_frames = usize::try_from(s.first_frames.value()).unwrap_or(usize::MAX);
            if first_n > 0 && first_frames > 0 && s.frames > first_frames {
                // Ratios without a cast: a `Duration` divides by a `u32`, and a
                // `u32` widens to `f64` by `From`; a count past `u32::MAX` is
                // saturated, which no replay here approaches.
                let n32 = |n| u32::try_from(n).unwrap_or(u32::MAX);
                let us_per = |d: Duration, n| (d / n32(n)).as_secs_f64() * 1e6;
                let per = |a, n| f64::from(n32(a)) / f64::from(n32(n));
                let rest = s.frames - first_frames;
                let first_miss = usize::try_from(s.first_miss.value()).unwrap_or(usize::MAX);
                let rest_miss = usize::try_from(s.miss.value())
                    .unwrap_or(usize::MAX)
                    .saturating_sub(first_miss);
                println!(
                    "      first {} frames: convert {:.1} us/frame, {} map misses ({:.1} a frame); \
the other {}: {:.1} us/frame, {} misses ({:.2} a frame)",
                    first_frames,
                    us_per(s.first_convert, first_frames),
                    first_miss,
                    per(first_miss, first_frames),
                    rest,
                    us_per(s.convert - s.first_convert, rest),
                    rest_miss,
                    per(rest_miss, rest),
                );
            }
            // Each kind of pair: the off and on sides' median GPU time, and
            // each pair's on side less its off side, median and range, the
            // first pair of each kind dropped as cold. An
            // effect whose last frame is the control's drew nothing, and a
            // control pair whose difference is not above zero did not
            // resolve the extra frame it carries.
            let control = trail_pairs
                .first()
                .filter(|k| k.0 == Pair::Control)
                .map(|k| k.3);
            for (pair, offs, ons, h) in &trail_pairs {
                // A kind's first pair follows a read back or a change of
                // kind and runs cold: the pairs after it are kept.
                let warm = usize::from(offs.len() > 1);
                let (offs, ons) = (&offs[warm..], &ons[warm.min(ons.len())..]);
                let sorted = |mut v: Vec<f64>| {
                    v.sort_by(f64::total_cmp);
                    v
                };
                let us = |d: &Duration| d.as_secs_f64() * 1e6;
                let mid = |v: &[f64]| v.get(v.len() / 2).copied().unwrap_or(f64::NAN);
                let off = sorted(offs.iter().map(us).collect());
                let on = sorted(ons.iter().map(us).collect());
                let diffs = sorted(offs.iter().zip(ons).map(|(a, b)| us(b) - us(a)).collect());
                let (lo, hi) = (
                    diffs.first().copied().unwrap_or(f64::NAN),
                    diffs.last().copied().unwrap_or(f64::NAN),
                );
                let (name, flag) = match *pair {
                    Pair::Control => {
                        let unresolved = !diffs.is_empty() && lo <= 0.0;
                        trail_control |= unresolved;
                        ("control", if unresolved { "  NOT ABOVE ZERO" } else { "" })
                    }
                    Pair::Of(e) => {
                        let blank = control == Some(*h);
                        trail_blank |= blank;
                        (
                            baud_bench::trail::name(Some(e)),
                            if blank { "  BLANK" } else { "" },
                        )
                    }
                };
                println!(
                    "      trail: round {round} {name:<9} {} pairs, off {:.1} us, on {:.1} us, on less off {:+.1} [{lo:+.1}..{hi:+.1}] us, {h:016x}{flag}",
                    diffs.len(),
                    mid(&off[..]),
                    mid(&on[..]),
                    mid(&diffs[..]),
                );
            }
            // Each kind of image frame: its snapshots a frame, and its
            // upload column, GPU time and placing, each as its median and
            // that less `still`'s on the same round, median and range; the
            // first round dropped as cold. `still2` less `still` is the
            // A/A range. A kind that snapshots other than it must, or whose
            // write did not move the epoch it must move, is marked.
            if images_len > 0 {
                let warm = usize::from(images_n > 1);
                let sorted = |mut v: Vec<f64>| {
                    v.sort_by(f64::total_cmp);
                    v
                };
                let us = |d: Duration| d.as_secs_f64() * 1e6;
                let mid = |v: &[f64]| v.get(v.len() / 2).copied().unwrap_or(f64::NAN);
                let ends = |v: &[f64]| {
                    (
                        v.first().copied().unwrap_or(f64::NAN),
                        v.last().copied().unwrap_or(f64::NAN),
                    )
                };
                let still = &shots[0][warm.min(shots[0].len())..];
                for (kind, all) in Pic::ALL.iter().zip(&shots) {
                    let kept = &all[warm.min(all.len())..];
                    let frames = u32::try_from(all.len()).unwrap_or(u32::MAX);
                    let snaps: u64 = all.iter().map(|s| s.snapshots.value()).sum();
                    let wrong = all.iter().any(|s| {
                        s.snapshots != kind.snapshots() || (kind.moves() && s.epoch == Epoch::Kept)
                    });
                    images_wrong |= wrong;
                    let column = |f: fn(&Shot) -> Duration| {
                        let own = sorted(kept.iter().map(|s| us(f(s))).collect());
                        let over = sorted(
                            kept.iter()
                                .zip(still)
                                .map(|(s, b)| us(f(s)) - us(f(b)))
                                .collect(),
                        );
                        let (lo, hi) = ends(&over);
                        format!(
                            "{:.1} us ({:+.1} [{lo:+.1}..{hi:+.1}])",
                            mid(&own),
                            mid(&over)
                        )
                    };
                    println!(
                        "      images: round {round} {:<6} {frames} frames, {:.2} snapshots a frame, \
upload {}, gpu {}, place {}{}",
                        kind.name(),
                        f64::from(u32::try_from(snaps).unwrap_or(u32::MAX))
                            / f64::from(frames.max(1)),
                        column(|s| s.upload),
                        column(|s| s.gpu),
                        column(|s| s.place),
                        if wrong { "  WRONG" } else { "" },
                    );
                }
            }
        }
        // Every cell arm's final frame against the copy's, which is the
        // screen by construction; else against the first arm's.
        let reference = hashes
            .iter()
            .find(|(a, ..)| *a == Arm::Copied)
            .or(hashes.first())
            .map(|&(_, h, _)| h);
        for &(a, h, inked) in &hashes {
            let ok = Some(h) == reference;
            mismatched |= !ok;
            println!(
                "      hash: round {round} {:<7} {h:016x}, {inked} pixels not the margin's{}",
                label(a),
                if ok { "" } else { "  MISMATCH" }
            );
        }
    }
    // A run whose own cores or GPU were shared voids and exits non-zero.
    if quiet.end().is_err() {
        std::process::exit(1);
    }
    if mismatched {
        eprintln!("frame_gpu: a cell arm's final frame is not the copy's");
        std::process::exit(1);
    }
    if trail_blank {
        eprintln!("frame_gpu: a trail's frame is the frame with no trail");
        std::process::exit(1);
    }
    if trail_control {
        eprintln!("frame_gpu: a control pair's difference is not above zero");
        std::process::exit(1);
    }
    if images_wrong {
        eprintln!(
            "frame_gpu: an image frame snapshotted other than its kind must, or its write did not move the epoch"
        );
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    baud_bench::warm::refuse_while_warm_on_gpu();
    eprintln!("frame_gpu draws through Metal and runs on macOS only");
}
