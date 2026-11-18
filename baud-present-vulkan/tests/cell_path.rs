//! **The cell path draws the CPU path's frame**: a real terminal and
//! renderer, a screen of coloured text, a run on a set background, an
//! underline and a block cursor, drawn by the scene harness
//! (`scenes::Arms`) on the cell path, the instance path and the CPU path.
//! Held, on both memory arms, in place and staged:
//!
//! - to the instance path's frame of the same screen on the same device
//!   (`Renderer::convert`, [`Gpu::draw`]), exactly, on lavapipe and on RADV;
//! - to the CPU path's premultiplied frame (`Renderer::render_at`), exactly
//!   on the pixels no record blends and within a step a channel on those
//!   one does (`common::assert_frame_of`), on lavapipe and on RADV.
//!
//! After it, the same screen is drawn again four times, the cursor hidden
//! and shown as a blink takes it: once warm, such a frame makes no heap
//! allocation on the drawing thread (`heap::made_by`).
//! Staged, such a frame uploads its `CU` alone, 152 bytes.
//!
//! **Two targets in turn.** A run of frames -- the screen, the cursor
//! hidden, the cursor moved, a cell written, the cursor hidden and shown,
//! another cell written -- is drawn into two targets alternately, each
//! frame into a target that holds an earlier one drawing that target's
//! stale rows and its own change in bands. After each, the target read
//! back is the same frame drawn whole into a new target, exactly, on both
//! memory arms, on lavapipe and on RADV.
//!
//! RADV needs the GPU, so its tests are `#[ignore]`d and run by name from
//! `scripts/linux-vulkan-radv.sh`.
//!
//! The face is JetBrains Mono, read from the repository by path
//! (`scenes::jetbrains_mono`).

#![cfg(target_os = "linux")]

mod common;
mod scenes;

use baud_core::{Owned, Term};
use baud_domain::WinSize;
use baud_font::Face;
use baud_present_vulkan::cells::CellDraw;
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::lend::Arm;
use baud_present_wayland::{Changed, Px};
use baud_render::cell_frame::{CU_NAMES, CU_WORDS};
use baud_render::{Redraw, Renderer, Stack, Theme};
use common::{lavapipe, radv};
use scenes::{Arms, Frames, differing, feed, jetbrains_mono};

/// **Heap allocations made on this thread while counting is on**, for the
/// caret frames' bound: a frame that only blinks the cursor allocates
/// nothing once warm. Per thread, so the harness's other threads count
/// nothing.
mod heap {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static COUNTING: Cell<bool> = const { Cell::new(false) };
        static MADE: Cell<u64> = const { Cell::new(0) };
    }

    pub struct Counting;

    fn note() {
        let on = COUNTING.try_with(Cell::get).unwrap_or(false);
        if on {
            let _ = MADE.try_with(|m| m.set(m.get() + 1));
        }
    }

    // SAFETY: every call is passed to the system allocator unchanged; the
    // count touches only const-initialised thread-locals, which allocate
    // nothing.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            note();
            // SAFETY: the caller's contract, passed on.
            unsafe { System.alloc(l) }
        }
        unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
            note();
            // SAFETY: as above.
            unsafe { System.alloc_zeroed(l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            note();
            // SAFETY: as above.
            unsafe { System.realloc(p, l, n) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            // SAFETY: as above.
            unsafe { System.dealloc(p, l) }
        }
    }

    /// The allocations `f` makes on this thread.
    pub fn made_by<T>(f: impl FnOnce() -> T) -> (T, u64) {
        MADE.with(|m| m.set(0));
        COUNTING.with(|c| c.set(true));
        let out = f();
        COUNTING.with(|c| c.set(false));
        (out, MADE.with(Cell::get))
    }
}

#[global_allocator]
static HEAP: heap::Counting = heap::Counting;

const WS: WinSize = WinSize {
    ws_col: 24,
    ws_row: 5,
};

/// **The screen, each printed character through `ink`**: red and bold
/// green text, a run of yellow on blue, an underline, truecolour text, and a
/// steady block cursor on the `e` of `red`, from the top left of a reset
/// terminal.
fn screen(ink: impl Fn(&str) -> String) -> Vec<u8> {
    format!(
        "\x1bc\x1b[2 q\x1b[31m{}\x1b[1;32m{}\x1b[0m {}\r\n\x1b[44;33m{}\x1b[0m \x1b[4m{}\x1b[0m\r\n\
         \x1b[38;2;200;100;50m{}\x1b[0m\x1b[1;2H",
        ink("red "),
        ink("bold green"),
        ink("plain"),
        ink(" blue bg "),
        ink("underline"),
        ink("true colour"),
    )
    .into_bytes()
}

/// Every character as written.
fn text(s: &str) -> String {
    s.to_string()
}

/// Every character a space: the screen with no ink.
fn blank(s: &str) -> String {
    " ".repeat(s.chars().count())
}

/// The renderer every frame here is drawn by, on one thread, so the caret
/// frames' count is the drawing thread's alone.
fn renderer<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    let mut r = scenes::renderer(Stack::new(faces).expect("a stack"), Theme::dark(), WS);
    r.set_threads(core::num::NonZeroUsize::MIN);
    r
}

/// **The screen's three frames on `arm`**, after the premises -- the
/// instance path draws ink, and some pixel is a glyph's edge -- and then
/// the caret frames: once warm, none allocates, and staged, each uploads
/// its `CU` alone.
fn the_screen(gpu: &Gpu, arm: Arm) -> Frames {
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, arm, renderer(&faces), &t);
    feed(&mut t, &screen(blank));
    let none = arms.draw(&mut t);
    feed(&mut t, &screen(text));
    let frames = arms.draw(&mut t);
    assert!(
        differing(&frames.instance, &none.instance) > 0,
        "the premise: the instance path draws ink"
    );
    assert!(
        frames.blended.iter().any(|&b| b),
        "the premise: some pixel is a glyph's edge"
    );
    let frame = arms.frame();
    assert!(
        frame.cursor.is_some(),
        "the premise: the frame draws a cursor"
    );
    arms.with_frame(&t, |d, input| {
        let (w, h) = d.renderer.whole_size();
        let target = d
            .gpu
            .target(Px(w.value()), Px(h.value()))
            .expect("a target");
        d.draw(input, &target);
        // The caret frames: the same screen, the cursor hidden and shown as
        // a blink takes it, the last shown, as the first frame was. Once
        // warm, a frame allocates nothing.
        for k in 0..4 {
            let caret = CellDraw {
                cursor: frame.cursor.filter(|_| k % 2 == 1),
                ..*input
            };
            let before = d.mem.uploaded();
            let (_, made) = heap::made_by(|| d.draw(&caret, &target));
            if k >= 2 {
                assert_eq!(made, 0, "{arm:?}: caret frame {k} allocated");
            }
            // Staged, a caret frame uploads its `CU` and nothing else: the
            // set holds the screen, the table and the overlay already.
            let cu_bytes = u64::try_from(4 * CU_WORDS).expect("bytes");
            let sent = d.mem.uploaded().0 - before.0;
            match arm {
                Arm::Staging => assert_eq!(sent, cu_bytes, "caret frame {k} uploaded"),
                Arm::InPlace => assert_eq!(sent, 0, "in place nothing is copied"),
            }
        }
        let cu = d.mem.uniforms(frame.at);
        for (name, want) in [
            ("grid.x", u32::from(WS.ws_col)),
            ("grid.y", u32::from(WS.ws_row)),
            ("cursor_kind", 1),
            ("pan", 0),
            ("slide_rows", 0),
            ("map_slots", 1 << 14),
        ] {
            assert!(CU_NAMES.contains(&name));
            assert_eq!(cu.get(name).map(|w| w.0), Some(want), "CU.{name}");
        }
        d.mem.forget(d.gpu, &target);
        d.gpu.drop_target(target);
    });
    frames
}

/// **The screen held to the CPU path's on `arm`.**
fn against_cpu(gpu: &Gpu, arm: Arm) {
    the_screen(gpu, arm).against_cpu(&format!("the screen, {arm:?}"), scenes::bar(gpu));
}

/// **The screen held to the instance path's on the same device**, pixel
/// for pixel.
fn against_instances(gpu: &Gpu, arm: Arm) {
    the_screen(gpu, arm).cell_is_instance(&format!("the screen, {arm:?}"));
}

#[test]
fn the_cell_path_is_the_cpu_path_in_place() {
    against_cpu(&lavapipe(), Arm::InPlace);
}

#[test]
fn the_cell_path_is_the_cpu_path_staged() {
    against_cpu(&lavapipe(), Arm::Staging);
}

#[test]
fn the_cell_path_is_the_instance_path() {
    let gpu = lavapipe();
    against_instances(&gpu, Arm::InPlace);
    against_instances(&gpu, Arm::Staging);
}

/// **On RADV, against the instance path, exactly, on both arms**: staged,
/// and on the arm the device's heaps allow.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn the_cell_path_is_the_instance_path_on_radv() {
    let gpu = radv();
    against_instances(&gpu, Arm::Staging);
    against_instances(&gpu, gpu.arm(1));
}

/// **On RADV, against the CPU path, on both arms**: unblended pixels
/// exact, blended ones within a step (`Rounding::OnceAtTies`).
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn the_cell_path_is_the_cpu_path_on_radv() {
    let gpu = radv();
    against_cpu(&gpu, Arm::Staging);
    against_cpu(&gpu, gpu.arm(1));
}

/// **The frames, drawn into two targets in turn, each the whole frame**:
/// built into set 0 of memory on `arm`, each drawn into the target its turn
/// names and read back against the same frame drawn whole into a new
/// target. The third frame, the cursor moved off row 0 into a target that
/// last held it there, draws the row the other target's frame hid it on;
/// the sixth draws the cursor's row into a target two frames behind.
fn two_targets(gpu: &Gpu, arm: Arm) {
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &screen(text));
    let mut arms = Arms::new(gpu, arm, renderer(&faces), &t);
    arms.build(&mut t);
    let (w, h) = arms.renderer.whole_size();
    let (w, h) = (Px(w.value()), Px(h.value()));
    let targets = [
        gpu.target(w, h).expect("the first target"),
        gpu.target(w, h).expect("the second target"),
    ];
    let shown = arms
        .frame()
        .cursor
        .expect("the premise: the frame draws a cursor");
    let moved = (baud_domain::Row::from_index(3), shown.1, shown.2);
    // Each frame: what is written first, the cursor, the target, and how
    // it is drawn.
    let steps: [(&[u8], Option<_>, usize, Drawn); 7] = [
        (b"", Some(shown), 0, Drawn::Whole),
        (b"", None, 1, Drawn::Whole),
        (b"", Some(moved), 0, Drawn::Bands),
        (b"\x1b[5;20HX\x1b[1;2H", Some(shown), 1, Drawn::Whole),
        (b"", None, 0, Drawn::Whole),
        (b"", Some(shown), 1, Drawn::Bands),
        (b"\x1b[2;3HY\x1b[1;2H", Some(shown), 0, Drawn::Whole),
    ];
    for (i, &(write, cursor, k, how)) in steps.iter().enumerate() {
        if !write.is_empty() {
            feed(&mut t, write);
            arms.build(&mut t);
        }
        let into = &targets[k];
        arms.with_frame(&t, |d, input| {
            let input = CellDraw { cursor, ..*input };
            let drawn = d.draw(&input, into);
            match (drawn.drawn, d.mem.damage(), how) {
                (Redraw::Rows(_), Changed::Strips(s), Drawn::Bands) => assert!(
                    !s.is_empty() && s.iter().all(|s| s.top.0 < s.bottom.0 && s.bottom.0 <= h.0),
                    "{arm:?}: frame {i}'s strips {s:?}"
                ),
                (Redraw::Whole, Changed::Whole, Drawn::Whole) => {}
                (d, c, _) => panic!("{arm:?}: frame {i} drew {d:?}, damaged {c:?}, not {how:?}"),
            }
            if let (2, Redraw::Rows(rows)) = (i, drawn.drawn) {
                assert!(
                    rows.has(baud_domain::Row::from_index(0)) && rows.has(moved.0),
                    "{arm:?}: the moved cursor's frame drew {rows:?}"
                );
            }
            let got = d.gpu.read(into).expect("the target read back");
            let want = d.whole(&input);
            let apart: Vec<usize> = (0..want.len()).filter(|&p| got[p] != want[p]).collect();
            assert!(
                apart.is_empty(),
                "{arm:?}: frame {i} into target {k}: {} pixels apart from the whole frame, \
                 the first rows {:?}",
                apart.len(),
                apart
                    .iter()
                    .take(6)
                    .map(|&p| p / usize::try_from(w.0).expect("a width"))
                    .collect::<Vec<_>>()
            );
        });
    }
    arms.with_frame(&t, |d, _| {
        for target in targets {
            d.mem.forget(d.gpu, &target);
            d.gpu.drop_target(target);
        }
    });
}

#[test]
fn two_targets_in_turn_are_each_the_whole_frame() {
    let gpu = lavapipe();
    two_targets(&gpu, Arm::InPlace);
    two_targets(&gpu, Arm::Staging);
}

/// **On RADV, the same, on both arms**: staged, and on the arm the
/// device's heaps allow.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn two_targets_in_turn_are_each_the_whole_frame_on_radv() {
    let gpu = radv();
    two_targets(&gpu, Arm::Staging);
    two_targets(&gpu, gpu.arm(1));
}

/// How a frame of [`two_targets`] is drawn into its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drawn {
    Whole,
    Bands,
}
