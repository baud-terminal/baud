//! `frame`: what a frame costs, split into the conversion and the paint.
//!
//! Replays one recording through the core in `read(2)`-sized reads and
//! paints a frame after every read at 200x50 cells, 14 px, JetBrains
//! Mono (`FRAME_WS` and `FRAME_PX` set others), reporting the per-frame
//! cost split into conversion (style resolution, shaping through the
//! run cache, atlas lookups) and compositing, plus the counters that
//! say two arms did equal work: rows dirtied, instances emitted, cache
//! hits and misses, glyphs rasterised, and the codepoints left out of a
//! cluster no face draws whole because the face that keeps it lacks
//! them (`unmapped`, the last rep's total; `Renderer::marks_unmapped`
//! says what it counts), and the outlines the atlas and the shaper's
//! glyph boxes refused for want of scratch (`refused` and `inkref`,
//! `AtlasStats::refused` and `Renderer::ink_refused`; zero unless a
//! face is read at a variation).
//!
//! Usage: `frame <recording> [reps] [full|incremental]`.
//!
//! Under the row each replay prints the best rep's frame times, `frame p50
//! p95 p99 max`, each frame's scan and render together. With `--features
//! count-allocs` it also prints the allocations a frame makes, all of them
//! and over the rep's second half, where a cold rep has warmed: a counting
//! allocator slows what it counts, so a timed run leaves the feature off.
//!
//! `FRAME_LINKS=on|hover|paths|paths-exist|off` prices a link's line at rest (`baud-app`'s
//! `lined.rs`, compiled here from its source): `on` scans each frame's changed
//! rows and draws what it finds, `hover` adds a pointer resting at the middle
//! of the screen, read every frame and drawn in the box, and `off` is `[link]
//! urls = "off"`, which reads nothing. Each prints, under its row, what the
//! best rep's scans read a frame: rows, cells, candidates and runs added.
//! Without it the replay draws no links, the row of a build before them.
//! `paths` reads printed paths as the app does (`paths.rs`): a frame's changed
//! rows owe a batch, and a collection reads the window's rows for them on the
//! first frame that owes one and then at most one every 60 frames, a batch a
//! second at 60 frames a second, the worker's ceiling; every path asked about
//! is answered gone, so none is drawn. `paths-exist` answers each as there,
//! so they are drawn, and the frames after read their changed rows. No
//! arm asks the file system: there is no worker here, and the answers are
//! the bench's own. `no-app` is `on` with no app answered for `http` and
//! `https`, so every web link the recording prints is struck, its text
//! drawn as a field in `missing_color`, the gray: the strike's frame-path
//! work, the input untouched. Its row's scans count the struck runs.
//! `FRAME_FOLDER=<path>` has the `paths` arms read relative paths and single
//! names against that folder, as a shell reporting it by `OSC 7` would make
//! them: the report is fed before the recording, naming a host the paths
//! are told is this machine's, and the folder is never looked at.
//! `FRAME_FORGET=on` has the `paths` arms forget, after each read, what the
//! read's record says it erased whole (`ReadRecord::erased`), as the app's
//! `Probes::erased` does for a drain of one part: `Erased::of` on the record
//! and the read's lines, placed as `keep::Counts` places a part's, on both
//! screens, then `Paths::forget_erased` for each run; a record that
//! renumbered its lines forgets nothing, as the app's drain skips it. It
//! also takes the record after every read, which `off`, the default and the
//! row of a build before it, does not; both run inside the frame's clock
//! and the scan's. Probes' cuts of the spans it keeps are left out: the
//! bench keeps none. Under the row it prints the best rep's forgets that
//! moved a fact (`Moved::Yes`) and those that moved nothing, the refiles --
//! reads whose forgets left rows to read again (`Reader::refiled`), one a
//! read -- and the records that overflowed and the runs the read's lines
//! did not place, which forget nothing. The bench files no writer, since it
//! has no `Probes`, so its store holds no fact and every forget here moves
//! nothing: the arm prices the record, the placing and the forget's search,
//! and the refile a moving forget owes is priced by no arm of this bench.
//! `FRAME_FORGET=on` with any other `FRAME_LINKS` is refused. The app's
//! source check `only_a_reads_record_ends_a_lines_facts` counts the calls
//! of `forget_erased` in `paths.rs` and `probe.rs` alone, so this call is
//! outside it, and that holds what the check guards: the app's facts end
//! only by a read's record. The bench is not the app; it compiles
//! `paths.rs` on its own, calls it from the same record, and ends no fact
//! the app keeps.
//! `FRAME_READ=<bytes>` paints a frame after each read of that many bytes
//! in the replay above, 4096 unless set: 1024 is what Darwin's tty hands a
//! read. The count of lines retired moves by what each read retired, as the
//! app's driver moves it, so a row's line is the line it shows.
//! `FRAME_STILL=<frames>` paints that many frames after the recording with
//! nothing read: a still screen's frames, the first of them the collection
//! the output owes, the rest what a frame over a screen that does not
//! change costs; the instructions a run retires with it, less those of the
//! run without, over the count, are a still frame's.
//! `FRAME_NAMES=off` reads no single names, `[link.path] forms` with
//! `"name"` taken out; the default reads them, as the app does, against the
//! folder `FRAME_FOLDER` gives.
//!
//! `FRAME_BIDI=explicit` resets BDSM (`CSI 8 l`) before the recording, so
//! each frame draws its rows in stored order; the default leaves it set,
//! as a terminal starts, and each frame lays its rows out in the order
//! UAX #9 gives them.
//!
//! `FRAME_WRAP=off` draws every row as the terminal cut it
//! (`baud_render::WordWrap::Off`, `[screen] wrap_words = "off"`), where the
//! default draws a finished paragraph at its words; each run prints, after
//! its row, the paragraphs the display re-broke summed over the best rep's
//! frames (`Display::counts`), so an arm that re-broke nothing shows it.
//!
//! `FRAME_STRING=1` begins and ends an SDS string (`CSI 1 ]`, `CSI 0 ]`)
//! before the recording, which writes no cell in it: the arm of a
//! terminal that has seen a string, whose frames may read each cell's
//! string (`Term::strings_seen`), against the same recording's.
//!
//! `FRAME_BG_ALPHA=<0-255>` paints over a page dimmed to that alpha, the
//! glass (`[glass] dim_dark = 0.70`, the default, is 179, and
//! `dim_light = 0.60` is 153), where the default is
//! `Theme::dark()`'s opaque page: the compositor blends the two pages by
//! two loops (`baud_render::Page`).
//!
//! `FRAME_BACKDROP=standard` reads the contrast floor's second pass
//! against the see-through standard material's greys, the dark's
//! (`Backdrop::under_window_dark`), which the app draws by default, or
//! the light's under `FRAME_THEME=light`, where the default is the
//! glass's black to white. It matters only below alpha 255.
//!
//! `FRAME_THEME=light` draws in `Theme::light()`'s colours, where the
//! default is `Theme::dark()`'s; with `FRAME_BACKDROP=standard` the floor
//! reads the light material's greys (`Backdrop::under_window_light`), as
//! the app pairs them.
//!
//! `FRAME_PX=<px>` draws the font at that many pixels, 14 by default: 28
//! is a 2x display's 14 points, and with `FRAME_WS=360x90` the 6120 by
//! 3330 pixel frame (6K). Every mode takes it, and a zoom or a rescale
//! starts from it.
//!
//! `FRAME_HASH=on` folds every frame of every rep, the whole buffer after
//! its render, into an FNV-1a hash (`baud_bench::PixelHash`), requires
//! every rep to come to the same one, and prints it under the row, once a
//! run: two builds that print the same hash drew the same pixels in every
//! frame. The hashing runs inside each rep's clock, so a hashed run is a
//! check of its pixels and not a timing. The modes that print no replay
//! row print no hash either; their replay is still hashed and checked.
//!
//! `FRAME_BLINK=none|fade|plasma|fire|ufo` prices instead what an idle blinking
//! cursor's frame costs over the recording's last screen: 2000 incremental
//! frames alternating two phases of that animation -- the hard switch's
//! two, two opacities of a fade, two pulses of the glow, two steps of the
//! fire's burn or the ufo's lift -- reporting the
//! cost, rows and instances a frame. Frames a cycle are `caret.rs`'s
//! `frames_a_cycle_for_each_animation`; the two multiply.
//!
//! `FRAME_TRAIL=none|quad|fire|plasma|lightning|ice|ufo|orb|liquid` prices
//! instead what a cursor trail costs the CPU path a frame: over the
//! recording's last screen, 600 incremental frames 1/120 s apart of a
//! 30-column move down three rows (`FRAME_TRAIL_MOVE` sets another), the
//! trail drawn from its start until it
//! is out and then again -- the quad's chase, or an effect riding it --
//! reporting the cost, rows and trail records a frame. The instructions
//! the whole run retires, `/usr/bin/time -l`, less those of `none`'s, over
//! 600, are the trail's a frame.
//!
//! `FRAME_IMAGE=<keys>|none` prices instead what an image on screen costs a
//! frame: over the recording's last screen a 256x192 kitty image placed at
//! row 6, column 11 with the `a=p` keys given (`c=40`, `z=-1`, ...), or none,
//! then 500 full frames, reporting the cost a frame. The instructions the
//! whole run retires, `/usr/bin/time -l`, less those of `none`'s, over 500,
//! are the image's a frame.
//!
//! `FRAME_GIF=<path>` prices instead an animated GIF's frames on the clock
//! -- `assets/readme/logo-dark.gif`, README's logo film, is the case it is
//! for: over the recording's last screen the GIF shown at the top-left by
//! iTerm2's `File=inline=1` in `FRAME_GIF_MB` MiB of image memory
//! (default 95, the app's), then `FRAME_GIF_FRAMES` (default 640, two of
//! the logo's loops) ticks, each at the next frame's due time and
//! followed by a full frame, reporting the tick -- where a frame kept as
//! what changed is composed into its canvas -- and the frame apart, a
//! frame each. Asserts the picture was placed and every tick moved the
//! frame. It reads only what a build before frames were kept so has too,
//! so one file builds both arms of an A/B; give that arm the memory its
//! whole frames take, 640 for the logo.
//!
//! `FRAME_ICAT=<path>` prices instead a kitty animation's frames on the
//! clock -- kitten icat's stream of the same logo film, made as
//! `tests/recordings/README.md` says `kitty/icat-logo-dark-140.raw` was but
//! of the film itself and not committed, is the case it is for: over the
//! recording's last screen the stream fed from the top-left in reads of
//! 1024 bytes, each followed by every decode step, in `FRAME_ICAT_MB` MiB
//! of image memory (default 95, the app's), then `FRAME_ICAT_FRAMES`
//! (default 640) ticks, each at the next frame's due time and followed by
//! a full frame, reporting the tick and the frame apart, a frame each, and
//! the frames stored and the words they take. Asserts the animation was
//! placed and every tick moved the frame. It reads only what a build before
//! kitty frames were kept as records has too, so one file builds both
//! arms; give that arm the memory its whole frames take, 1280 for the logo.
//!
//! `FRAME_PERCELL=<cols>x<rows>` prices instead a picture placed one cell
//! at a time, yazi 26.9.1's shape (an `a=p,i,p,x,y,w,h,c=1,r=1,z=-1,C=1`
//! per cell): over the recording's last screen, a `cols` by `rows` pixel
//! image placed from the top-left, a source pixel a cell, then a full
//! frame and `FRAME_PERCELL_FRAMES` (default 20) incremental ones over the
//! still screen, reporting the cost a frame, the rows dirtied and an
//! FNV-1a of the last frame's pixels. `FRAME_PERCELL_CHURN=1` re-puts
//! placement 1 with another source pixel before every frame, so each
//! frame has a placement gone and one arrived. `FRAME_PERCELL_OVER=1`
//! places a partly transparent 2x2 image over the text, `c` and `r` the
//! screen's, so every row is repainted every frame, and the picture's
//! placements under it with it. `FRAME_WS=<cols>x<rows>`
//! sets the screen, 200x50 by default.
//!
//! `FRAME_ZOOM=<px>` prices a zoom and a zoom back over the recording's
//! last screen: warm at `FRAME_PX`'s size, then `FRAME_ZOOM_STEPS`
//! rescales to `px` and back by turns, each with the one full frame it
//! forces, reporting each step's time, the glyphs drawn and the bytes a
//! GPU copy of the atlas would take. `FRAME_ZOOM=<px>,<px>,...` walks
//! those sizes instead, and `FRAME_ZOOM_PAINT=off` converts without
//! painting, the CPU's share of a zoom on the GPU path.

use baud_bench::PixelHash;
use baud_core::{Owned, Term};
use baud_domain::Chan;
use baud_domain::{BytePos, Cols, ImageBudget, ImagePx, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Backdrop, Blend, CursorPhase, Damage, PxSize, Renderer, Stack, Target, Theme};
use std::time::Duration;

/// The app's source for the files built here that read it, as
/// `baud-app/src/app_source.rs` lists it: the same files, in the same order.
#[cfg(test)]
#[allow(dead_code)]
mod app_source {
    /// Every listed file's text, joined in the list's order.
    const APP: &str = concat!(include_str!("../../baud-app/src/app.rs"));
    /// Each listed file's text, in the same order.
    const TEXTS: &[&str] = &[include_str!("../../baud-app/src/app.rs")];

    /// The app's whole source.
    pub(crate) const fn app_source() -> &'static str {
        APP
    }

    /// The text a cut from `head` in `src` runs in: the listed file that
    /// holds `head` when `src` is the whole source, else `src` itself.
    pub(crate) fn within<'s>(src: &'s str, head: &str) -> &'s str {
        if src == APP {
            TEXTS
                .iter()
                .copied()
                .find(|text| text.contains(head))
                .unwrap_or(APP)
        } else {
            src
        }
    }
}

#[allow(dead_code, unused_imports)]
#[path = "../../baud-app/src/lined.rs"]
mod lined;
#[allow(dead_code, unused_imports)]
#[path = "../../baud-app/src/link.rs"]
mod link;
// Its tests' `rules_of!` is expanded only in `#[test]` functions, which
// this bench does not compile.
#[allow(dead_code, unused_imports, unused_macros)]
#[path = "../../baud-app/src/paths.rs"]
mod paths;
#[allow(dead_code, unused_imports)]
#[path = "../../baud-app/src/rules.rs"]
mod rules;
// The ssh session a remote link opens through (`link.rs`).
#[allow(dead_code, unused_imports)]
#[path = "../../baud-app/src/ssh.rs"]
mod ssh;
// What a rule's command is started with (`sys::child`).
#[allow(dead_code, unused_imports)]
#[path = "../../baud-app/src/sys.rs"]
mod sys;
// The program `paths.rs`'s tests run, found where this system keeps it.
#[cfg(test)]
#[path = "../../baud-app/src/tool.rs"]
mod tool;

/// `FRAME_LINKS`: what each frame does about links.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Links {
    /// Not set: nothing, the arm a build before them runs.
    Untouched,
    Off,
    On,
    Hover,
    /// `on` with printed paths read as the app reads them: a collection on
    /// the first frame that owes one, then at most one every 60 frames, a
    /// second a batch at 60 frames a second, the worker's ceiling, with
    /// every path answered gone -- no file system is asked -- so none is
    /// drawn.
    Paths,
    /// `paths`, with every path answered as there, so they are drawn.
    PathsExist,
    /// `on`, with no app answered for `http` and `https`: every web link
    /// struck.
    NoApp,
}

/// A switch the environment sets: whether a zoom step paints its frame,
/// whether the replay hashes its frames.
#[derive(Clone, Copy)]
enum Switch {
    On,
    Off,
}

/// **What `FRAME_FORGET=on`'s forgets did over a rep** ([`forget_read`]).
#[derive(Clone, Copy, Default)]
struct Forgets {
    /// Forgets that ended a fact (`Moved::Yes`), and those that moved
    /// nothing.
    moved: baud_render::Tally,
    idle: baud_render::Tally,
    /// Reads whose forgets left rows to read again, one a read.
    refiles: baud_render::Tally,
    /// Records that held more erasures than they have room for, and runs
    /// the read's lines did not place: neither forgets anything.
    overflowed: baud_render::Tally,
    unplaced: baud_render::Tally,
}

/// **`FRAME_FORGET=on`: what the read just fed erased whole, forgotten**,
/// as the app's `Probes::erased` forgets a part's: the read's record taken,
/// its lines placed as `keep::Counts` places them -- `base` the primary's
/// window top and the count that numbered it as the read began, against
/// the top as it ended -- then `Erased::of` on each screen and
/// `Paths::forget_erased` for each run, each counted into `n` by whether
/// it moved the answers' generation. A record that renumbered the lines
/// forgets nothing, as the app's drain skips it, and one that overflowed
/// stops the screens there, as `Probes::erased` does.
fn forget_read(
    t: &mut Term<'_>,
    printed: &mut paths::Paths,
    base: (baud_domain::LineIdx, baud_render::Scrolled),
    n: &mut Forgets,
) {
    let rec = t.take_record();
    if rec.renumbered().is_some() {
        return;
    }
    let lines = paths::CoreLines::of(Some(base)).part(Some(t.history_mark().line()));
    let was = printed.generation();
    for screen in [baud_core::Screen::Normal, baud_core::Screen::Alternate] {
        match paths::Erased::of(&rec, &lines, printed, screen) {
            paths::ErasedLines::Overflowed => {
                n.overflowed.bump();
                break;
            }
            paths::ErasedLines::Placed(erased, unplaced) => {
                for _ in 0..unplaced {
                    n.unplaced.bump();
                }
                for e in erased {
                    let before = printed.generation();
                    printed.forget_erased(e);
                    if printed.generation() == before {
                        n.idle.bump();
                    } else {
                        n.moved.bump();
                    }
                }
            }
        }
    }
    if printed.generation() != was {
        n.refiles.bump();
    }
}

/// **Every allocation counted**, for `count-allocs`: only in a build taken for
/// these counts, never in a timed one.
#[cfg(feature = "count-allocs")]
mod counted {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, Ordering};

    pub static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

    struct Counting;

    // SAFETY: every call is passed to the system's allocator as it came.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            unsafe { System.alloc(l) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            unsafe { System.dealloc(p, l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            unsafe { System.realloc(p, l, n) }
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;
}

/// Allocations so far, none without the counter.
fn allocations() -> u64 {
    #[cfg(feature = "count-allocs")]
    {
        counted::ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(feature = "count-allocs"))]
    {
        0
    }
}

/// **The frame the timer times**: the seal, then the render. Its own
/// symbol, kept out of line, so the counter gate (`scripts/perf-gate.sh`)
/// can scope callgrind to exactly this work; `Renderer::render` and
/// `convert_with` are inlined into the caller and have none.
///
/// The seal is the one a driver takes once a frame, after the scan and
/// before the build: the next frame's scan compares only the rows written
/// after it.
#[inline(never)]
fn timed_frame(
    r: &mut Renderer,
    t: &mut Term<'_>,
    target: &mut Target<'_>,
    damage: Damage,
) -> baud_render::Frame {
    let _ = t.seal_writes();
    r.render(t, target, damage)
}

fn main() {
    baud_bench::warm::refuse_while_warm();
    // Every renderer map keyed by one fixed seed, so a run repeats its probes
    // and a counter gate reads the same instructions twice
    // (`baud_render::fix_hash_seed`). Before the first map is made.
    assert!(
        baud_render::fix_hash_seed(baud_render::HashSeed(0)),
        "the seed was set before main"
    );
    let mut args = baud_bench::args();
    let which = args.next().unwrap_or_else(|| "btop".to_string());
    let reps: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
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
    let data = std::fs::read(format!(
        "{}/../tests/recordings/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("recording");
    // `FRAME_FACES=path[#index],...` stacks other faces -- on macOS,
    // `/System/Library/Fonts/Menlo.ttc,/System/Library/Fonts/Apple Color
    // Emoji.ttc` is the terminal's real stack and prices the colour path.
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
    let ws = std::env::var("FRAME_WS")
        .ok()
        .and_then(|v| {
            let (c, r) = v.split_once('x')?;
            Some(WinSize {
                ws_row: r.parse().ok()?,
                ws_col: c.parse().ok()?,
            })
        })
        .unwrap_or(WinSize {
            ws_row: 50,
            ws_col: 200,
        });
    // **A fresh renderer per rep.** The run cache and the atlas persist for
    // a session, so a renderer reused across reps reports the second rep
    // with every run already shaped and every glyph already rasterised --
    // the first version of this harness did, and its "best of three" was
    // a warm-cache number three times lower than a real first pass. Each
    // rep starts cold, which is what a recording's first replay costs.
    // `FRAME_THEME`, `FRAME_PX` and `FRAME_HASH` refuse a value they do
    // not name rather than run the default under another arm's name.
    let (base, standard) = match std::env::var("FRAME_THEME").as_deref() {
        Ok("light") => (Theme::light(), Backdrop::under_window_light()),
        Ok("dark") | Err(_) => (Theme::dark(), Backdrop::under_window_dark()),
        Ok(other) => panic!("FRAME_THEME is light or dark, not {other}"),
    };
    let theme = Theme {
        bg_alpha: std::env::var("FRAME_BG_ALPHA")
            .ok()
            .and_then(|s| s.parse().ok())
            .map_or(base.bg_alpha, Chan::from_u8),
        backdrop: match std::env::var("FRAME_BACKDROP").as_deref() {
            Ok("standard") => standard,
            _ => Backdrop::any(),
        },
        ..base
    };
    let font_px: f32 = std::env::var("FRAME_PX").map_or(14.0, |v| {
        v.parse()
            .unwrap_or_else(|_| panic!("FRAME_PX is a size in px, not {v}"))
    });
    let size =
        PxSize::of(font_px).unwrap_or_else(|| panic!("FRAME_PX={font_px} is no size a face takes"));
    let hashing = match std::env::var("FRAME_HASH").as_deref() {
        Ok("on") => Switch::On,
        Ok("off") | Err(_) => Switch::Off,
        Ok(other) => panic!("FRAME_HASH is on or off, not {other}"),
    };
    let wrap = match std::env::var("FRAME_WRAP").as_deref() {
        Ok("off") => baud_render::WordWrap::Off,
        _ => baud_render::WordWrap::On,
    };
    // `FRAME_READ`: the bytes a frame reads, 4096 unless set.
    let read: usize = std::env::var("FRAME_READ").map_or(4096, |v| {
        v.parse()
            .ok()
            .filter(|&n| n > 0)
            .unwrap_or_else(|| panic!("FRAME_READ is a count of bytes, not {v}"))
    });
    // `FRAME_STILL`: frames painted after the recording with nothing read.
    let still: usize = std::env::var("FRAME_STILL").map_or(0, |v| {
        v.parse()
            .unwrap_or_else(|_| panic!("FRAME_STILL is a count of frames, not {v}"))
    });
    let links = match std::env::var("FRAME_LINKS").as_deref() {
        Err(_) => Links::Untouched,
        Ok("off") => Links::Off,
        Ok("on") => Links::On,
        Ok("hover") => Links::Hover,
        Ok("paths") => Links::Paths,
        Ok("paths-exist") => Links::PathsExist,
        Ok("no-app") => Links::NoApp,
        Ok(other) => {
            panic!("FRAME_LINKS is on, hover, paths, paths-exist, no-app or off, not {other}")
        }
    };
    // `FRAME_FORGET`: whether the `paths` arms forget what each read
    // erased whole; off unless asked, the row of a build before it.
    let forget = match std::env::var("FRAME_FORGET").as_deref() {
        Ok("off") | Err(_) => Switch::Off,
        Ok("on") => Switch::On,
        Ok(other) => panic!("FRAME_FORGET is on or off, not {other}"),
    };
    assert!(
        matches!(forget, Switch::Off) || matches!(links, Links::Paths | Links::PathsExist),
        "FRAME_FORGET=on prices the paths arms: FRAME_LINKS=paths or paths-exist"
    );
    let fresh = || {
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            size,
            theme.clone(),
            Blend::Native,
            Cols::of_index(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .unwrap();
        r.set_word_wrap(wrap);
        r
    };
    let mut r = fresh();
    let (w, h) = r.pixel_size();
    let mut pixels = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    println!(
        "{:<12} {:>7} {:>9} {:>9} {:>9} {:>8} {:>9} {:>8} {:>8} {:>8} {:>8} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>8} {:>7} {:>7}",
        "recording",
        "frames",
        "us/frame",
        "convert",
        "paint",
        "rows",
        "inst",
        "hit%",
        "raster",
        "coloured",
        "grown",
        "differs",
        "span",
        "marked",
        "start",
        "shaped",
        "morx",
        "bigram",
        "unmapped",
        "refused",
        "inkref"
    );
    /// The best rep's sums.
    struct Rep {
        frames: usize,
        total: Duration,
        convert: Duration,
        paint: Duration,
        rows: u64,
        inst: u64,
        exc: [u64; 7],
        /// Paragraphs the display re-broke, summed over the frames.
        rebroken: baud_render::Tally,
        /// What the scans read, summed; and the most any frame's did.
        scans: lined::Scan,
        widest: lined::Scan,
        idle_frames: baud_render::Tally,
        /// Rows each frame's scan compared with what it kept
        /// (`Lines::compared`), summed over the frames.
        compared: baud_render::Tally,
        scan_time: Duration,
        reads: baud_render::Tally,
        /// Paths a `paths-exist` collection asked about, answered as there.
        asked: baud_render::Tally,
        /// Rows the collections' own scans read; rows the scan read on the
        /// frame after a batch of answers; and the runs underlined, summed
        /// over the frames, which the renderer draws each frame.
        collect_rows: baud_render::Tally,
        answer_rows: baud_render::Tally,
        marks: baud_render::Tally,
        /// What `FRAME_FORGET=on`'s forgets did, summed over the frames.
        forgets: Forgets,
        /// Each frame's time, and its allocations.
        times: Vec<Duration>,
        allocs: Vec<u64>,
    }
    let mut best: Option<Rep> = None;
    // Each rep's hash of its frames, under `FRAME_HASH=on`.
    let mut hashes = Vec::new();
    for rep in 0..reps {
        if rep > 0 {
            r = fresh();
        }
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        // `FRAME_BIDI=explicit` resets BDSM before the recording, so no
        // frame lays its rows out by UAX #9: the arm without the
        // right-to-left layout, which a terminal starts with since BDSM is
        // set at power-on.
        if std::env::var("FRAME_BIDI").as_deref() == Ok("explicit") {
            let bdsm = b"\x1b[8l";
            t.write_at(BytePos::ZERO, bdsm);
            t.feed(BytePos::ZERO, ReadLen::from_index(bdsm.len()));
        }
        // `FRAME_STRING=1`: a string begun and ended, no cell in it.
        if std::env::var("FRAME_STRING").as_deref() == Ok("1") {
            let sds = b"\x1b[1]\x1b[0]";
            t.write_at(BytePos::ZERO, sds);
            t.feed(BytePos::ZERO, ReadLen::from_index(sds.len()));
            assert!(t.strings_seen(), "the string was begun");
        }
        let mut rep = Rep {
            frames: 0,
            total: Duration::ZERO,
            convert: Duration::ZERO,
            paint: Duration::ZERO,
            rows: 0,
            inst: 0,
            exc: [0; 7],
            rebroken: baud_render::Tally::default(),
            scans: lined::Scan::default(),
            widest: lined::Scan::default(),
            idle_frames: baud_render::Tally::default(),
            compared: baud_render::Tally::default(),
            scan_time: Duration::ZERO,
            reads: baud_render::Tally::default(),
            asked: baud_render::Tally::default(),
            collect_rows: baud_render::Tally::default(),
            answer_rows: baud_render::Tally::default(),
            marks: baud_render::Tally::default(),
            forgets: Forgets::default(),
            times: Vec::with_capacity(data.len() / 4096 + 1),
            allocs: Vec::with_capacity(data.len() / 4096 + 1),
        };
        let mut lines = lined::Lines::default();
        let mut views = lined::ViewLinks::default();
        let mut uris = lined::Uris::default();
        // No home, no names: `~/` and `./` resolve to nothing, as they would
        // before a window knows either; `/` is read as the app reads it.
        let forms = match std::env::var("FRAME_NAMES").as_deref() {
            Ok("off") => paths::Forms {
                name: paths::Turned::Off,
                ..paths::Forms::DEFAULT
            },
            Ok("on") | Err(_) => paths::Forms::DEFAULT,
            Ok(other) => panic!("FRAME_NAMES is on or off, not {other}"),
        };
        let mut printed = paths::Paths::new(None, forms);
        printed.set(match links {
            Links::Paths | Links::PathsExist => paths::Turned::On,
            _ => paths::Turned::Off,
        });
        // `FRAME_FOLDER`: the folder reported before the recording.
        if let Ok(folder) = std::env::var("FRAME_FOLDER") {
            let osc = format!("\x1b]7;file://frame.example{folder}\x07");
            t.write_at(BytePos::ZERO, osc.as_bytes());
            t.feed(BytePos::ZERO, ReadLen::from_index(osc.len()));
            printed.set_names(link::MachineNames::one("frame.example"));
            let _ = printed.observe(
                &t,
                baud_render::Scrolled::START.advanced(Rows::of_count(1 << 40)),
            );
        }
        if links == Links::NoApp {
            let none = [link::Scheme::Https, link::Scheme::Http]
                .into_iter()
                .map(|s| (paths::Program::Scheme(s), paths::Handler::Missing))
                .collect();
            let _ = printed.set_programs(none);
        }
        let mut hover = link::Hover::default();
        // The count of lines retired, moved as the app's driver moves it, by
        // what each read retired, so a row's line is the line it shows.
        let mut scrolled = baud_render::Scrolled::START.advanced(Rows::of_count(1 << 40));
        let middle = (
            baud_domain::Row::from_index(usize::from(ws.ws_row) / 2),
            baud_domain::Col::at_index(usize::from(ws.ws_col) / 2),
        );
        if links == Links::Hover {
            hover.set_at_rest(link::AtRest::Look(baud_render::HoverPaint::Box));
        }
        // The frame of the last collection: a frame that owes one collects
        // at once the first time and after 60 frames since.
        let mut collected: Option<usize> = None;
        // The frame after a batch of answers, whose scan reads what they moved.
        let mut after_answers: Option<usize> = None;
        let mut at = 0usize;
        let mut hash = PixelHash::START;
        let t0 = std::time::Instant::now();
        let mut still_left = still;
        while at < data.len() || still_left > 0 {
            if at >= data.len() {
                still_left -= 1;
            }
            let n = read.min(data.len() - at);
            // `FRAME_FORGET=on`: the primary's window top and the count that
            // numbers it as the read begins, which place the lines its
            // record names; none for a frame with nothing read.
            let began = match forget {
                Switch::On if n > 0 => Some((t.history_mark().line(), scrolled)),
                Switch::On | Switch::Off => None,
            };
            let before = t.rotation();
            if n > 0 {
                t.write_at(BytePos::ZERO, &data[at..at + n]);
                t.feed(BytePos::ZERO, ReadLen::from_index(n));
            }
            if let baud_core::Moved::Retired(k) = t.moved_since(before) {
                scrolled = scrolled.advanced(k);
            }
            at += n;
            let mut target = Target::new(&mut pixels, w, w, h).unwrap();
            let (f0, a0) = (std::time::Instant::now(), allocations());
            match links {
                Links::Untouched => {}
                Links::Off => {
                    lines.off();
                    r.set_link_lines(&[], baud_render::LineStyle::Solid);
                    r.set_link_strikes(&[], &[]);
                }
                Links::On | Links::Hover | Links::Paths | Links::PathsExist | Links::NoApp => {
                    let s0 = std::time::Instant::now();
                    if let Some(base) = began {
                        forget_read(&mut t, &mut printed, base, &mut rep.forgets);
                    }
                    if printed.observe(&t, scrolled) == paths::Moved::Yes {
                        lines.forget();
                    }
                    let s = lines.scan(
                        &t,
                        scrolled,
                        &views.sync(&t, scrolled),
                        &mut paths::Links {
                            uris: &mut uris,
                            urls: paths::Turned::On,
                            paths: &mut printed,
                        },
                    );
                    rep.compared
                        .add(baud_render::Tally::of(lines.compared().index()));
                    let frame = rep.frames;
                    if matches!(links, Links::Paths | Links::PathsExist)
                        && printed.owed() == paths::Owed::Yes
                        && collected.is_none_or(|c| frame >= c + 60)
                    {
                        collected = Some(frame);
                        printed.begin_collect();
                        lines.forget();
                        let c = lines.scan(
                            &t,
                            scrolled,
                            &views.sync(&t, scrolled),
                            &mut paths::Links {
                                uris: &mut uris,
                                urls: paths::Turned::On,
                                paths: &mut printed,
                            },
                        );
                        rep.collect_rows.add(baud_render::Tally::of(c.rows));
                        let asked = printed.end_collect();
                        rep.asked.add(baud_render::Tally::of(asked.len()));
                        let probe = match links {
                            Links::PathsExist => paths::Probe::Exists,
                            _ => paths::Probe::Missing,
                        };
                        let answers = asked.into_iter().map(|c| (c.path, probe));
                        if printed.merge_into(answers, &mut lines) == paths::Moved::Yes {
                            after_answers = Some(rep.frames + 1);
                        }
                    }
                    if links == Links::Hover {
                        let _ = hover.update(
                            &t,
                            Some(middle),
                            scrolled,
                            link::Chord::Other,
                            baud_core::Focus::Gained,
                            &link::MachineNames::NONE,
                        );
                        r.set_link_hover(hover.over());
                        rep.reads = hover.reads();
                    }
                    rep.scan_time += s0.elapsed();
                    r.set_link_lines(lines.lines(), baud_render::LineStyle::Solid);
                    r.set_link_strikes(lines.struck(), &[]);
                    rep.scans.rows += s.rows;
                    rep.scans.cells += s.cells;
                    rep.scans.candidates += s.candidates;
                    rep.scans.spans += s.spans;
                    rep.scans.struck += s.struck;
                    rep.scans.remarks += s.remarks;
                    rep.scans.rereads += s.rereads;
                    if after_answers == Some(frame) {
                        rep.answer_rows.add(baud_render::Tally::of(s.rows));
                    }
                    rep.marks.add(baud_render::Tally::of(lines.lines().len()));
                    rep.widest.rows = rep.widest.rows.max(s.rows);
                    rep.widest.cells = rep.widest.cells.max(s.cells);
                    rep.widest.candidates = rep.widest.candidates.max(s.candidates);
                    rep.widest.spans = rep.widest.spans.max(lines.lines().len());
                    if s == lined::Scan::default() {
                        rep.idle_frames.bump();
                    }
                }
            }
            let f = timed_frame(&mut r, &mut t, &mut target, damage);
            rep.times.push(f0.elapsed());
            rep.allocs.push(allocations() - a0);
            if let Switch::On = hashing {
                hash = hash.frame(target.pixels());
            }
            rep.frames += 1;
            rep.convert += f.convert;
            rep.paint += f.composite;
            rep.rows += f.rows_dirty.value();
            rep.inst += f.instances.value();
            if let Some(d) = r.bidi().display() {
                rep.rebroken.add(d.counts().0);
            }
            let e = f.exceptions;
            for (slot, t) in rep.exc.iter_mut().zip([
                e.differs, e.spanning, e.marked, e.starters, e.shaped, e.morx, e.bigram,
            ]) {
                *slot += t.value();
            }
        }
        rep.total = t0.elapsed();
        hashes.push(hash);
        if best.as_ref().is_none_or(|b| rep.total < b.total) {
            best = Some(rep);
        }
    }
    // A fresh renderer draws over the last rep's buffer, so a rep whose
    // frames differ is a frame that depends on more than the recording.
    let pixels_hash = match hashing {
        Switch::On => Some(PixelHash::agreed(&hashes).unwrap_or_else(|| {
            let each: Vec<String> = hashes.iter().map(PixelHash::to_string).collect();
            panic!("the reps drew different pixels: {}", each.join(" "))
        })),
        Switch::Off => None,
    };
    // `FRAME_RESCALE=<points>` prices a zoom: warm the atlas on the whole
    // recording at `FRAME_PX`'s size, then rescale to the new size and
    // time the one full-damage frame that follows -- the frame a font
    // change forces, in which every visible glyph re-shapes and
    // re-rasterises into an atlas that restarts at its smallest sheet. The
    // grow and raster deltas say whether the cost is the rasterising or
    // the atlas growing under it.
    if let Ok(kind) = std::env::var("FRAME_BLINK") {
        let phase = |opacity: u8, glow: u8| CursorPhase {
            opacity: Chan::from_u8(opacity),
            glow: Chan::from_u8(glow),
            ..CursorPhase::HIDDEN
        };
        let pair = match kind.as_str() {
            "fade" => [phase(128, 0), phase(96, 0)],
            "plasma" => [phase(0, 128), phase(0, 192)],
            "fire" => [
                CursorPhase::fire(Chan::from_u8(70)),
                CursorPhase::fire(Chan::from_u8(80)),
            ],
            "ufo" => [
                CursorPhase::ufo(Chan::from_u8(70)),
                CursorPhase::ufo(Chan::from_u8(80)),
            ],
            _ => [CursorPhase::HIDDEN, CursorPhase::SHOWN],
        };
        let mut rb = fresh();
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let mut tail = data.clone();
        tail.extend_from_slice(b"\x1b[1 q\x1b[?25h");
        t.write_at(BytePos::ZERO, &tail);
        t.feed(BytePos::ZERO, ReadLen::from_index(tail.len()));
        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
        rb.render(&mut t, &mut target, Damage::Full);
        let n = 2000u32;
        let (mut rows, mut inst, mut glow) = (0u64, 0u64, 0u64);
        let (mut conv, mut comp) = (Duration::ZERO, Duration::ZERO);
        let start = std::time::Instant::now();
        for i in 0..n {
            rb.set_cursor_blink(pair[usize::from(i % 2 == 1)]);
            let f = rb.render(&mut t, &mut target, Damage::Incremental);
            rows += f.rows_dirty.value();
            inst += f.instances.value();
            glow += u64::try_from(rb.cursor_glow().len()).unwrap_or(0);
            conv += f.convert;
            comp += f.composite;
        }
        let us = start.elapsed().as_secs_f64() * 1e6 / f64::from(n);
        let per = |d: Duration| d.as_secs_f64() * 1e6 / f64::from(n);
        // `instances` is the rows' converted records; the glow's solids
        // ride beside them and are counted on their own.
        println!(
            "blink {kind} over {which}: {us:.1} us/frame (convert {:.1}, composite {:.1}), {:.2} rows/frame, {:.1} instances/frame, {:.2} glow solids/frame",
            per(conv),
            per(comp),
            rows as f64 / f64::from(n),
            inst as f64 / f64::from(n),
            glow as f64 / f64::from(n),
        );
        return;
    }
    if let Ok(kind) = std::env::var("FRAME_TRAIL") {
        use baud_render::effect::{Effect, Ignition, Seed, Vfx};
        use baud_render::{Pt, Segment, Shape, Smear, Strength, TrailLook};
        let effect = match kind.as_str() {
            "fire" => Some(Effect::Fire),
            "plasma" => Some(Effect::Plasma),
            "lightning" => Some(Effect::Lightning),
            "ice" => Some(Effect::Ice),
            "ufo" => Some(Effect::Ufo),
            "orb" => Some(Effect::Orb),
            "liquid" => Some(Effect::Liquid),
            _ => None,
        };
        let mut rt = fresh();
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        t.write_at(BytePos::ZERO, &data);
        t.feed(BytePos::ZERO, ReadLen::from_index(data.len()));
        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
        rt.render(&mut t, &mut target, Damage::Full);
        let style = baud_core::CursorStyle::SteadyBlock;
        let cell = |row: usize, col: usize| {
            rt.cursor_rect(
                baud_domain::Row::from_index(row),
                baud_domain::Col::at_index(col),
                style,
            )
        };
        // `FRAME_TRAIL_MOVE=<row>,<col>,<row>,<col>` moves elsewhere: the
        // screen's diagonal is `0,0,49,199`.
        let mv: Vec<usize> = std::env::var("FRAME_TRAIL_MOVE")
            .ok()
            .map(|v| v.split(',').filter_map(|n| n.parse().ok()).collect())
            .filter(|v: &Vec<usize>| v.len() == 4)
            .unwrap_or_else(|| vec![10, 40, 13, 70]);
        let (from, to) = (cell(mv[0], mv[1]), cell(mv[2], mv[3]));
        let seg = Segment::new(from.corners(), to);
        let unit = rt.metrics().cell_h.value() as f32;
        let tail = Pt {
            x: (from.left + from.right) * 0.5,
            y: (from.top + from.bottom) * 0.5,
        };
        let ignition = Ignition::ignite(tail, to, Seed(7), unit);
        let life = effect.map_or(seg.lasts(), |e| e.end(&ignition));
        let per = u32::try_from((life * 120.0).ceil().max(1.0) as i64).unwrap_or(1);
        let n = 600u32;
        let (mut rows, mut records) = (0u64, 0u64);
        let start = std::time::Instant::now();
        for i in 0..n {
            let age = (i % per) as f32 / 120.0;
            let emitter = Smear {
                corners: seg.at(age),
                hole: to,
                shape: Shape::Quad,
                strength: Strength::Full,
                opacity: Smear::OPAQUE,
            };
            let look = match (kind.as_str(), effect) {
                ("none", _) => None,
                (_, Some(e)) => Some(TrailLook::Vfx(Vfx {
                    effect: e,
                    emitter,
                    ignition: ignition.at(age),
                    axis: Pt { x: 0.995, y: 0.1 },
                })),
                (_, None) => Some(TrailLook::Smear(emitter)),
            };
            rt.set_trail(look);
            let f = rt.render(&mut t, &mut target, Damage::Incremental);
            rows += f.rows_dirty.value();
            records += u64::try_from(rt.trail().len()).unwrap_or(0);
        }
        let us = start.elapsed().as_secs_f64() * 1e6 / f64::from(n);
        println!(
            "trail {kind} over {which}: {us:.1} us/frame, {:.2} rows/frame, {:.1} records/frame, {per} frames a life",
            rows as f64 / f64::from(n),
            records as f64 / f64::from(n),
        );
        return;
    }
    if let Ok(keys) = std::env::var("FRAME_IMAGE") {
        let mut ri = fresh();
        let m = ri.metrics();
        let cell = |p: baud_font::Px| ImagePx::of(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(256 * 192));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(cell(m.cell_w), cell(m.cell_h));
        let mut tail = data.clone();
        if keys != "none" {
            const B64: &[u8; 64] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut rgba = Vec::new();
            for y in 0..192u32 {
                for x in 0..256u32 {
                    let [_, _, r, g] = (x * 256 + y).to_be_bytes();
                    rgba.extend_from_slice(&[r, g, 160, 255]);
                }
            }
            tail.extend_from_slice(b"\x1b[6;11H\x1b_Ga=T,f=32,s=256,v=192,i=1,q=2,C=1,");
            tail.extend_from_slice(keys.as_bytes());
            tail.push(b';');
            for q in rgba.chunks(3) {
                let mut b = [0u8; 3];
                b[..q.len()].copy_from_slice(q);
                let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
                for k in 0..4 {
                    tail.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap_or(0)]);
                }
            }
            tail.extend_from_slice(b"\x1b\\");
        }
        t.write_at(BytePos::ZERO, &tail);
        t.feed(BytePos::ZERO, ReadLen::from_index(tail.len()));
        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
        ri.render(&mut t, &mut target, Damage::Full);
        let n = 500u32;
        let start = std::time::Instant::now();
        for _ in 0..n {
            ri.render(&mut t, &mut target, Damage::Full);
        }
        let us = start.elapsed().as_secs_f64() * 1e6 / f64::from(n);
        println!(
            "image {keys} over {which}: {us:.1} us/frame, {} placements",
            ri.image_placements().len()
        );
        return;
    }
    if let Ok(path) = std::env::var("FRAME_GIF") {
        let gif = std::fs::read(&path).expect("FRAME_GIF names a GIF");
        let frames: u32 = std::env::var("FRAME_GIF_FRAMES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(640);
        let mut ri = fresh();
        let m = ri.metrics();
        let cell = |p: baud_font::Px| ImagePx::of(u16::try_from(p.value()).unwrap_or(u16::MAX));
        // `baud-app`'s `host::IMAGE_BUDGET` by default.
        let mib: usize = std::env::var("FRAME_GIF_MB")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(95);
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(mib << 18));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(cell(m.cell_w), cell(m.cell_h));
        const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut tail = data.clone();
        tail.extend_from_slice(b"\x1b[H\x1b]1337;File=inline=1:");
        for q in gif.chunks(3) {
            let mut b = [0u8; 3];
            b[..q.len()].copy_from_slice(q);
            let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
            for k in 0..4 {
                match k <= q.len() {
                    true => tail.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap_or(0)]),
                    false => tail.push(b'='),
                }
            }
        }
        tail.push(0x07);
        for c in tail.chunks(4096) {
            t.write_at(BytePos::ZERO, c);
            t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
        }
        let id = t
            .images()
            .placements()
            .next()
            .map(|(_, p)| p.image)
            .expect("FRAME_GIF: the picture was placed");
        let size = t.images().size_of(id).expect("FRAME_GIF: the picture");
        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
        let mut now = baud_domain::HostTime::of_nanos(1_000_000_000);
        let play = baud_domain::ImageMotion::Play;
        t.animate_images(now, play);
        ri.render(&mut t, &mut target, Damage::Full);
        let (mut tick, mut frame) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
        for k in 0..frames {
            now = t.next_image_frame(now).expect("FRAME_GIF: the film runs");
            let a = std::time::Instant::now();
            let moved = t.animate_images(now, play);
            let b = std::time::Instant::now();
            ri.render(&mut t, &mut target, Damage::Full);
            frame += b.elapsed();
            tick += b - a;
            assert_eq!(moved, baud_domain::Animated::Changed, "FRAME_GIF: tick {k}");
        }
        let each = |d: std::time::Duration| d.as_secs_f64() * 1e6 / f64::from(frames);
        println!(
            "gif {path} over {which}: tick {:.1} us/frame, frame {:.1} us/frame, {frames} ticks, {}x{} in {mib} MiB",
            each(tick),
            each(frame),
            size.0.raw(),
            size.1.raw(),
        );
        return;
    }
    if let Ok(path) = std::env::var("FRAME_ICAT") {
        let stream = std::fs::read(&path).expect("FRAME_ICAT names a capture");
        let frames: u32 = std::env::var("FRAME_ICAT_FRAMES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(640);
        let mut ri = fresh();
        let m = ri.metrics();
        let cell = |p: baud_font::Px| ImagePx::of(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let mib: usize = std::env::var("FRAME_ICAT_MB")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(95);
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(mib << 18));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(cell(m.cell_w), cell(m.cell_h));
        let mut tail = data.clone();
        tail.extend_from_slice(b"\x1b[H");
        tail.extend_from_slice(&stream);
        for c in tail.chunks(1024) {
            t.write_at(BytePos::ZERO, c);
            t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
            while t.decode_step(baud_domain::DecodeBudget::ALL) == baud_domain::DecodeProgress::More
            {
            }
            t.clear_reply();
        }
        let id = t
            .images()
            .placements()
            .next()
            .map(|(_, p)| p.image)
            .expect("FRAME_ICAT: the animation was placed");
        let size = t.images().size_of(id).expect("FRAME_ICAT: the animation");
        let (stored, _, _) = t.images().kitty_frames(id).expect("FRAME_ICAT: its frames");
        let words = t.images().stored_words(id).unwrap_or(0);
        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
        let mut now = baud_domain::HostTime::of_nanos(1_000_000_000);
        let play = baud_domain::ImageMotion::Play;
        t.animate_images(now, play);
        ri.render(&mut t, &mut target, Damage::Full);
        let (mut tick, mut frame) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
        for k in 0..frames {
            now = t.next_image_frame(now).expect("FRAME_ICAT: the film runs");
            let a = std::time::Instant::now();
            let moved = t.animate_images(now, play);
            let b = std::time::Instant::now();
            ri.render(&mut t, &mut target, Damage::Full);
            frame += b.elapsed();
            tick += b - a;
            assert_eq!(
                moved,
                baud_domain::Animated::Changed,
                "FRAME_ICAT: tick {k}"
            );
        }
        let each = |d: std::time::Duration| d.as_secs_f64() * 1e6 / f64::from(frames);
        println!(
            "icat {path} over {which}: tick {:.1} us/frame, frame {:.1} us/frame, {frames} ticks, {}x{}, {stored} frames in {words} words, {mib} MiB",
            each(tick),
            each(frame),
            size.0.raw(),
            size.1.raw(),
        );
        return;
    }
    if let Ok(size) = std::env::var("FRAME_PERCELL") {
        let (pc, pr) = size.split_once('x').expect("FRAME_PERCELL=<cols>x<rows>");
        let (pc, pr): (u32, u32) = (pc.parse().unwrap(), pr.parse().unwrap());
        let frames: u32 = std::env::var("FRAME_PERCELL_FRAMES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        let churn = std::env::var("FRAME_PERCELL_CHURN").is_ok_and(|v| v == "1");
        let over = std::env::var("FRAME_PERCELL_OVER").is_ok_and(|v| v == "1");
        let mut ri = fresh();
        let m = ri.metrics();
        let cell = |p: baud_font::Px| ImagePx::of(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let area = usize::try_from(pc * pr).unwrap();
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(area + 64));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(cell(m.cell_w), cell(m.cell_h));
        const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut rgb = Vec::new();
        for y in 0..pr {
            for x in 0..pc {
                let [_, _, a, b] = (x * 7 + y * 13).to_be_bytes();
                rgb.extend_from_slice(&[a, b, 90]);
            }
        }
        let mut tail = data.clone();
        tail.extend_from_slice(format!("\x1b_Gq=2,a=t,i=1,f=24,s={pc},v={pr};").as_bytes());
        for q in rgb.chunks(3) {
            let mut b = [0u8; 3];
            b[..q.len()].copy_from_slice(q);
            let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
            for k in 0..4 {
                tail.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap_or(0)]);
            }
        }
        tail.extend_from_slice(b"\x1b\\");
        for y in 0..pr {
            for x in 0..pc {
                tail.extend_from_slice(
                    format!(
                        "\x1b[{};{}H\x1b_Gq=2,a=p,i=1,p={},x={x},y={y},w=1,h=1,c=1,r=1,z=-1,C=1\x1b\\",
                        y + 1,
                        x + 1,
                        y * pc + x + 1
                    )
                    .as_bytes(),
                );
            }
        }
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            for c in b.chunks(4096) {
                t.write_at(BytePos::ZERO, c);
                t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
            }
        };
        feed(&mut t, &tail);
        if over {
            // Four RGBA pixels at half alpha: `f=32`, base64 of 16 bytes.
            let put = format!(
                "\x1b[1;1H\x1b_Gq=2,a=T,i=2,f=32,s=2,v=2,c={},r={},z=1,C=1;gEAggIBAIICAQCCAgEAggA==\x1b\\",
                ws.ws_col, ws.ws_row
            );
            feed(&mut t, put.as_bytes());
            assert_eq!(
                t.images().placements().count(),
                area + 1,
                "FRAME_PERCELL_OVER: the overlay was placed; leave it a slot"
            );
        }
        let mut rows = 0u64;
        let us = {
            let mut target = Target::new(&mut pixels, w, w, h).unwrap();
            ri.render(&mut t, &mut target, Damage::Full);
            let start = std::time::Instant::now();
            for k in 0..frames {
                if churn {
                    let put = format!(
                        "\x1b[1;1H\x1b_Gq=2,a=p,i=1,p=1,x={},y=0,w=1,h=1,c=1,r=1,z=-1,C=1\x1b\\",
                        (k + 1) % 2
                    );
                    feed(&mut t, put.as_bytes());
                }
                rows += ri
                    .render(&mut t, &mut target, Damage::Incremental)
                    .rows_dirty
                    .value();
            }
            start.elapsed().as_secs_f64() * 1e6 / f64::from(frames)
        };
        let fnv = pixels.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &p| {
            (h ^ u64::from(p)).wrapping_mul(0x0000_0100_0000_01b3)
        });
        println!(
            "percell {pc}x{pr} over {which} at {}x{}: {us:.1} us/frame, {} placements drawn, {} live, {rows} rows dirtied in {frames} frames, pixels {fnv:016x}",
            ws.ws_col,
            ws.ws_row,
            ri.image_placements().len(),
            t.images().placements().count(),
        );
        return;
    }
    if let Ok(pts) = std::env::var("FRAME_RESCALE") {
        let pts: f32 = pts.parse().unwrap_or(24.0);
        let mut rz = fresh();
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let (w0, h0) = rz.pixel_size();
        let mut px0 = vec![0u32; usize::try_from(w0.value() * h0.value()).unwrap()];
        let mut at = 0usize;
        while at < data.len() {
            let n = 4096.min(data.len() - at);
            t.write_at(BytePos::ZERO, &data[at..at + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            at += n;
            let mut target = Target::new(&mut px0, w0, w0, h0).unwrap();
            rz.render(&mut t, &mut target, Damage::Full);
        }
        let a0 = rz.atlas_stats();
        let c0 = rz.cache_stats();
        if rz.rescale(PxSize::of(pts).unwrap()).is_some() {
            let (w1, h1) = rz.pixel_size();
            let mut px1 = vec![0u32; usize::try_from(w1.value() * h1.value()).unwrap()];
            let mut target = Target::new(&mut px1, w1, w1, h1).unwrap();
            let start = std::time::Instant::now();
            let f = rz.render(&mut t, &mut target, Damage::Full);
            let elapsed = start.elapsed();
            let a1 = rz.atlas_stats();
            let c1 = rz.cache_stats();
            println!(
                "rescale {font_px}->{pts}px: frame {} us (convert {} us, paint {} us); raster {}, grown {}, misses {}",
                elapsed.as_micros(),
                f.convert.as_micros(),
                f.composite.as_micros(),
                a1.rasterised.value() - a0.rasterised.value(),
                a1.grown.value() - a0.grown.value(),
                c1.misses.value() - c0.misses.value(),
            );
        }
        return;
    }
    // `FRAME_ZOOM=<px>` prices a zoom and a zoom back: warm the atlas on
    // the whole recording at `FRAME_PX`'s size, then `FRAME_ZOOM_STEPS`
    // (default 4) rescales, to the new size and back by turns, each
    // followed by the one full-damage frame it forces, printing every
    // step's wall time, convert and paint, rasters, sheet sides and the
    // bytes a GPU copy of the sheets would take. `FRAME_ZOOM=<px>,<px>,...`
    // walks those sizes in order instead. Run under `/usr/bin/time -l` at
    // 0, 1, 2 and more steps: the instructions retired at `n` steps less
    // those at `n - 1` are step `n`'s, the first zoom and then the zoom
    // back. `FRAME_ZOOM_PAINT=off` converts each step's frame and paints
    // nothing, as the app's GPU path leaves the paint to the GPU, so the
    // count is the CPU's share of a zoom.
    if let Ok(list) = std::env::var("FRAME_ZOOM") {
        let paint = std::env::var("FRAME_ZOOM_PAINT").map_or(Switch::On, |v| match v.as_str() {
            "off" => Switch::Off,
            _ => Switch::On,
        });
        let walk: Vec<f32> = list.split(',').filter_map(|v| v.parse().ok()).collect();
        let walk = match walk.as_slice() {
            [one] => vec![*one, font_px],
            _ => walk,
        };
        let pts = list;
        let steps: usize = std::env::var("FRAME_ZOOM_STEPS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(walk.len().max(4));
        let mut rz = fresh();
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let (w0, h0) = rz.pixel_size();
        let mut px = vec![0u32; usize::try_from(w0.value() * h0.value()).unwrap()];
        let mut at = 0usize;
        while at < data.len() {
            let n = 4096.min(data.len() - at);
            t.write_at(BytePos::ZERO, &data[at..at + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            at += n;
            let mut target = Target::new(&mut px, w0, w0, h0).unwrap();
            rz.render(&mut t, &mut target, Damage::Full);
        }
        rz.atlas_mut().take_dirty();
        println!(
            "zoom {pts} px over {which}: warm at {font_px} px, sheets {}/{}",
            rz.atlas().side().value(),
            rz.atlas().color_side().value()
        );
        let mut fnv = 0xcbf2_9ce4_8422_2325u64;
        for step in 1..=steps {
            let to = walk[(step - 1) % walk.len()];
            let a0 = rz.atlas_stats();
            // The time is the rescale's and the frame's; the canvas
            // resized between them, and hashed after, is the harness's.
            let start = std::time::Instant::now();
            rz.rescale(PxSize::of(to).unwrap())
                .expect("a size the face takes");
            let rescaled = start.elapsed();
            let (convert, composite, framed) = match paint {
                Switch::On => {
                    let (w1, h1) = rz.pixel_size();
                    px.clear();
                    px.resize(usize::try_from(w1.value() * h1.value()).unwrap(), 0);
                    let mut target = Target::new(&mut px, w1, w1, h1).unwrap();
                    let start = std::time::Instant::now();
                    let f = rz.render(&mut t, &mut target, Damage::Full);
                    (f.convert, f.composite, start.elapsed())
                }
                Switch::Off => {
                    let start = std::time::Instant::now();
                    let c = rz.convert(&t, Rows::ZERO, Damage::Full);
                    (c.convert, Duration::ZERO, start.elapsed())
                }
            };
            let elapsed = rescaled + framed;
            let a1 = rz.atlas_stats();
            // What a GPU copy of the sheets would take for this frame: a
            // replaced sheet whole, else the rectangle written.
            let (dirty, mask_new, color_new) = rz.atlas_mut().take_dirty();
            let texels = |r: Option<baud_render::TexRect>| {
                r.map_or(0, |r| u64::from(r.w.0) * u64::from(r.h.0))
            };
            let side = |s: baud_render::Side| u64::from(s.value()) * u64::from(s.value());
            let upload = match mask_new {
                baud_render::Replaced::Yes => side(rz.atlas().side()),
                baud_render::Replaced::No => texels(dirty.mask),
            } + 4 * match color_new {
                baud_render::Replaced::Yes => side(rz.atlas().color_side()),
                baud_render::Replaced::No => texels(dirty.color),
            };
            if let Switch::On = paint {
                for p in &px {
                    fnv = (fnv ^ u64::from(*p)).wrapping_mul(0x100_0000_01b3);
                }
            }
            println!(
                "  step {step} to {to}px: {} us (convert {} us, paint {} us); raster {}, coloured {}, synthesised {}; sheets {}/{}, upload {upload} bytes",
                elapsed.as_micros(),
                convert.as_micros(),
                composite.as_micros(),
                a1.rasterised.value() - a0.rasterised.value(),
                a1.coloured.value() - a0.coloured.value(),
                a1.synthesised.value() - a0.synthesised.value(),
                rz.atlas().side().value(),
                rz.atlas().color_side().value(),
            );
        }
        println!("  pixels {fnv:016x}");
        return;
    }

    let Rep {
        frames,
        total,
        convert,
        paint,
        rows,
        inst,
        exc,
        rebroken,
        scans,
        widest,
        idle_frames,
        compared,
        scan_time,
        reads,
        asked,
        collect_rows,
        answer_rows,
        marks,
        forgets,
        times,
        allocs,
    } = best.unwrap();
    let a = r.atlas_stats();
    let (raster, coloured, grown) = (a.rasterised.value(), a.coloured.value(), a.grown.value());
    let c = r.cache_stats();
    let hit = 100.0 * c.hits.value() as f64 / (c.hits.value() + c.misses.value()).max(1) as f64;
    println!(
        "{:<12} {:>7} {:>9.1} {:>9.1} {:>9.1} {:>8.1} {:>9.1} {:>7.1}% {:>8} {:>8} {:>8} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>8} {:>7} {:>7}",
        which,
        frames,
        total.as_secs_f64() * 1e6 / frames as f64,
        convert.as_secs_f64() * 1e6 / frames as f64,
        paint.as_secs_f64() * 1e6 / frames as f64,
        rows as f64 / frames as f64,
        inst as f64 / frames as f64,
        hit,
        raster,
        coloured,
        grown,
        exc[0] as f64 / frames as f64,
        exc[1] as f64 / frames as f64,
        exc[2] as f64 / frames as f64,
        exc[3] as f64 / frames as f64,
        exc[4] as f64 / frames as f64,
        exc[5] as f64 / frames as f64,
        exc[6] as f64 / frames as f64,
        r.marks_unmapped().value(),
        a.refused.value(),
        r.ink_refused().value()
    );
    println!("  rebroken {}", rebroken.value());
    if matches!(
        links,
        Links::On | Links::Hover | Links::Paths | Links::PathsExist | Links::NoApp
    ) {
        let per = |n: usize| n as f64 / frames as f64;
        println!(
            "  links scan {:.1} us/frame; rows {:.1} compared {:.1} cells {:.0} candidates {:.2} added {:.2} a frame; most in one frame rows {} cells {} candidates {}, on screen {}; {} frames read nothing; pointer reads {}",
            scan_time.as_secs_f64() * 1e6 / frames as f64,
            per(scans.rows),
            compared.value() as f64 / frames as f64,
            per(scans.cells),
            per(scans.candidates),
            per(scans.spans),
            widest.rows,
            widest.cells,
            widest.candidates,
            widest.spans,
            idle_frames.value(),
            reads.value(),
        );
        if matches!(links, Links::Paths | Links::PathsExist) {
            println!(
                "  paths asked {}, answered by the bench, no file system asked",
                asked.value()
            );
        }
        if let Switch::On = forget {
            println!(
                "  forgets moved {} moved nothing {} refiles {}; records overflowed {}, runs not placed {}",
                forgets.moved.value(),
                forgets.idle.value(),
                forgets.refiles.value(),
                forgets.overflowed.value(),
                forgets.unplaced.value(),
            );
        }
        println!("  struck runs added {}", scans.struck);
        println!(
            "  answers: remarks {} rereads {}; rows read by collections {}, on the frame after a batch {}; runs underlined {:.2} a frame",
            scans.remarks,
            scans.rereads,
            collect_rows.value(),
            answer_rows.value(),
            marks.value() as f64 / frames as f64,
        );
    }
    let mut sorted = times;
    sorted.sort_unstable();
    // The frame `per` hundredths of the way through, to the nearest.
    let at = |per: usize| {
        let k = ((sorted.len() - 1) * per + 50) / 100;
        sorted[k].as_secs_f64() * 1e6
    };
    println!(
        "  frame p50 {:.1} p95 {:.1} p99 {:.1} max {:.1} us",
        at(50),
        at(95),
        at(99),
        at(100)
    );
    if cfg!(feature = "count-allocs") {
        let half = &allocs[allocs.len() / 2..];
        println!(
            "  allocs/frame {:.2}, second half {:.2}",
            allocs.iter().sum::<u64>() as f64 / allocs.len() as f64,
            half.iter().sum::<u64>() as f64 / half.len().max(1) as f64,
        );
    }
    if let Some(h) = pixels_hash {
        println!("  pixels {h}");
    }
}
