//! **What a frame costs when only the cursor's blink moved**, counted
//! headless on both paths the app draws with: the CPU path's `render_at`,
//! and the cell path's frame -- `baud_render::cell_frame::build`, which
//! `App::present`'s cell arm builds it with, the copy of the screen it
//! draws from included, built and never handed to a GPU. A 200x50 screen with every row full:
//! Hebrew and Arabic runs, ligature pairs, combining marks, a DECDWL row and
//! a DECDHL pair, wrapped rows under word-wrap, two link lines with one
//! struck, a kitty image over the text, a Sixel and a status line.
//!
//! Each frame is read for the renderer's counters
//! (`baud_render::counters`, under its `counters` feature, which this
//! crate's dev-dependency turns on: a build without it does not compile)
//! and for the heap allocations made on this thread and on any other while
//! it was built. The arms: (a) the first frames, which build everything;
//! (b) frames where only the cursor's blink phase changes; (c) one cell
//! written mid-screen, then a frame; (d) the pointer at rest over a link;
//! and a screen with a blinking cell whose text blink flips, apart. Only
//! the controls are asserted -- that each counter moves on a full frame,
//! that the caret frame blinks the cursor, that a written cell repaints its
//! row alone and draws the pixels a full frame does -- and every count is
//! printed, for the rows that bring each one down to assert their own.
//!
//! The cell path's driver is pinned to `App::present`: both build the
//! frame through `build`, once, every call the driver makes before it is
//! one `present` makes, in `present`'s order, and `present` makes none of
//! the calls `build` makes. Without it the counts would describe a frame
//! the app no longer draws.
//!
//! The faces are read from the repository by path, never from the system:
//! JetBrains Mono from `baud-app/fonts/jetbrains-mono`, as the app embeds
//! it, and Cascadia Mono's Hebrew and Arabic from
//! `baud-app/fonts/glyphs/languages/hebrew`, under the SIL Open Font
//! Licence 1.1 (`OFL.txt` beside it).
//!
//! `harness = false`, so the process has one thread and an allocation on
//! any other is one the renderer's frame made. It answers nextest's
//! `--list` itself, one name a check.

#[cfg(target_os = "macos")]
#[global_allocator]
static HEAP: heap::Counting = heap::Counting;

const NAMES: [&str; 12] = [
    "every_counter_moves_on_a_full_frame",
    "a_caret_frame_blinks_the_cursor_and_counts_what_it_costs",
    "a_written_cell_repaints_its_row_alone",
    "a_marked_cell_costs_a_caret_frame_no_allocation",
    "a_ligature_window_costs_a_caret_frame_no_allocation",
    "a_sized_line_costs_a_caret_frame_no_allocation",
    "a_cell_written_on_a_sized_line_converts_it_again",
    "a_converted_row_costs_a_caret_frame_no_allocation",
    "a_caret_frame_tallies_the_rows_it_converts",
    "the_frames_totals_follow_every_write_of_a_row",
    "the_cell_driver_builds_as_present_builds",
    "nothing_that_ships_counts",
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        if cfg!(target_os = "macos") && !args.iter().any(|a| a == "--ignored") {
            for name in NAMES {
                println!("{name}: test");
            }
        }
        return;
    }
    let exact = args.iter().any(|a| a == "--exact");
    let filter = args.iter().find(|a| !a.starts_with('-'));
    let chosen = |name: &str| match filter {
        None => true,
        Some(f) if exact => name == f.as_str(),
        Some(f) => name.contains(f.as_str()),
    };
    #[cfg(target_os = "macos")]
    for name in NAMES.into_iter().filter(|n| chosen(n)) {
        match name {
            "every_counter_moves_on_a_full_frame" => checks::every_counter_moves_on_a_full_frame(),
            "a_caret_frame_blinks_the_cursor_and_counts_what_it_costs" => {
                checks::a_caret_frame_blinks_the_cursor_and_counts_what_it_costs();
            }
            "a_written_cell_repaints_its_row_alone" => {
                checks::a_written_cell_repaints_its_row_alone();
            }
            "a_marked_cell_costs_a_caret_frame_no_allocation" => {
                checks::a_marked_cell_costs_a_caret_frame_no_allocation();
            }
            "a_ligature_window_costs_a_caret_frame_no_allocation" => {
                checks::a_ligature_window_costs_a_caret_frame_no_allocation();
            }
            "a_sized_line_costs_a_caret_frame_no_allocation" => {
                checks::a_sized_line_costs_a_caret_frame_no_allocation();
            }
            "a_cell_written_on_a_sized_line_converts_it_again" => {
                checks::a_cell_written_on_a_sized_line_converts_it_again();
            }
            "a_converted_row_costs_a_caret_frame_no_allocation" => {
                checks::a_converted_row_costs_a_caret_frame_no_allocation();
            }
            "a_caret_frame_tallies_the_rows_it_converts" => {
                checks::a_caret_frame_tallies_the_rows_it_converts();
            }
            "the_frames_totals_follow_every_write_of_a_row" => {
                checks::the_frames_totals_follow_every_write_of_a_row();
            }
            "the_cell_driver_builds_as_present_builds" => {
                checks::the_cell_driver_builds_as_present_builds();
            }
            "nothing_that_ships_counts" => checks::nothing_that_ships_counts(),
            _ => unreachable!("every name is dispatched"),
        }
        println!("test {name} ... ok");
    }
    #[cfg(not(target_os = "macos"))]
    let _ = chosen;
}

/// **Every allocation, counted on the thread that made it and in all.**
/// The system's allocator does the work; this counts each `alloc`, and
/// `alloc_zeroed` and `realloc`, which reach it through the trait's own
/// defaults. A `const` thread-local with nothing to drop, so counting
/// allocates nothing.
#[cfg(target_os = "macos")]
mod heap {
    use baud_render::Tally;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct Counting;

    static EVERY_THREAD: AtomicUsize = AtomicUsize::new(0);

    std::thread_local! {
        static THIS_THREAD: Cell<usize> = const { Cell::new(0) };
        static THIS_THREAD_BYTES: Cell<usize> = const { Cell::new(0) };
    }

    // SAFETY: every call is forwarded to the system allocator with the
    // layout and pointer it was given; the counts are plain stores.
    #[allow(unsafe_code)]
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            EVERY_THREAD.fetch_add(1, Ordering::Relaxed);
            let _ = THIS_THREAD.try_with(|c| c.set(c.get().wrapping_add(1)));
            let _ = THIS_THREAD_BYTES.try_with(|c| c.set(c.get().wrapping_add(layout.size())));
            // SAFETY: the caller's contract, passed on.
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: the caller's contract, passed on.
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    /// What was allocated since the last [`take`]: on this thread, its
    /// bytes, and on every other thread.
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    pub struct Heap {
        pub here: Tally,
        pub bytes: Tally,
        pub elsewhere: Tally,
    }

    /// The allocations since the last call, which start again at zero.
    /// Nothing between two calls may print, or it counts.
    pub fn take() -> Heap {
        let here = THIS_THREAD.with(|c| c.replace(0));
        let bytes = THIS_THREAD_BYTES.with(|c| c.replace(0));
        let all = EVERY_THREAD.swap(0, Ordering::Relaxed);
        Heap {
            here: Tally::of(here),
            bytes: Tally::of(bytes),
            elsewhere: Tally::of(all.saturating_sub(here)),
        }
    }
}

#[cfg(target_os = "macos")]
mod checks {
    use crate::heap::{self, Heap};
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, Col, Cols, ImageBudget, PackedCell, ReadLen, Row, Rows, WinSize};
    use baud_font::{Face, Px};
    use baud_present_macos::journal::{SetIx, Wheel};
    use baud_present_macos::screens::{Built, Screens};
    use baud_render::cell_frame::{CellInputs, CellParts, build};
    use baud_render::counters::{self, Counter, Counts};
    use baud_render::{
        Blend, Blink, BlinkPhases, Candidate, CursorPhase, Damage, GlyphMap, HoverPaint, Instance,
        Leaving, Lent, LineStyle, LinkHover, LinkLine, LinkSpan, Overlay, OverlayStats, PxSize,
        Renderer, Resolved, Stack, StyleStamp, Tally, Target, Theme,
    };

    const WS: WinSize = WinSize {
        ws_col: 200,
        ws_row: 50,
    };

    /// **A frame's place in an arm**: the first [`Nth::WARM`] build what
    /// the later ones keep -- one a set of the cell path's copies, and one
    /// more -- and after them the blink's phase flips every frame.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Nth(usize);

    impl Nth {
        const WARM: Nth = Nth(Wheel::SETS + 1);
        /// The first frames, then ten whose blink flips.
        const ALL: Nth = Nth(Nth::WARM.0 + 10);

        /// The frames before this one, in order.
        fn before(self) -> impl Iterator<Item = Nth> {
            (0..self.0).map(Nth)
        }

        fn warming(self) -> bool {
            self.0 < Nth::WARM.0
        }

        /// Whether a blink draws its hidden phase: every other frame after
        /// the first ones, starting with the first after them.
        fn hidden(self) -> bool {
            !self.warming() && (self.0 - Nth::WARM.0) % 2 == 0
        }

        /// The cell path's set this frame is built into, round in turn as
        /// the app's wheel hands them out while each frame lands before
        /// the next is built.
        fn set(self) -> SetIx {
            SetIx(self.0 % Wheel::SETS)
        }
    }

    const JETBRAINS: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"
    ));
    const CASCADIA: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-app/fonts/glyphs/languages/hebrew/CascadiaMono.ttf"
    ));
    const SWIRL: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/recordings/sixel/swirl.raw"
    ));

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// A theme that finds pictures drawn in text: the app's dimmed
    /// backgrounds with pictures kept opaque.
    fn theme() -> Theme {
        Theme {
            bg_alpha: baud_domain::Chan::from_u8(56),
            app_bg: baud_render::AppBg::Dimmed,
            picture_bg: baud_render::PictureBg::Opaque,
            ..Theme::dark()
        }
    }

    fn renderer<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
        let r = Renderer::new(
            Stack::new(faces).expect("a stack of the two faces"),
            PxSize::of(14.0).expect("a size"),
            theme(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .expect("a renderer");
        assert!(r.theme().finds_pictures(), "the theme finds pictures");
        r
    }

    /// The columns `s` takes: a combining mark none, every other
    /// character of the fixture one.
    fn width(s: &str) -> Cols {
        Cols::of_index(
            s.chars()
                .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
                .count(),
        )
    }

    /// `seed` repeated and cut to exactly `cols` columns.
    fn filled(seed: &str, cols: Cols) -> String {
        let mut out = String::new();
        let mut used = 0;
        'fill: loop {
            for c in seed.chars() {
                let w = width(c.encode_utf8(&mut [0; 4])).index();
                if used + w > cols.index() {
                    break 'fill;
                }
                out.push(c);
                used += w;
            }
        }
        out
    }

    const SEEDS: [&str; 6] = [
        "fn main() -> Result != None => ok <= x // ",
        "caf\u{65}\u{301} na\u{69}\u{308}ve r\u{65}\u{301}sum\u{65}\u{301} a\u{308}\u{301} ",
        "shalom \u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd} 2026 world ",
        "marhaba \u{645}\u{631}\u{62d}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645} 42 ",
        "\x1b[31mred\x1b[0m \x1b[42mgreen\x1b[0m \x1b[1mbold\x1b[0m plain text -> ",
        "a -> b != c :: d <- e == f ",
    ];

    /// **The screen**: the status line, a DECDWL row, a DECDHL pair, rows
    /// full from the seeds -- every fourth one wrapped onto the next as one
    /// line -- two links, a kitty image and a Sixel over the text, a
    /// blinking cell where `blinking` asks, and the cursor a blinking block
    /// mid-screen.
    fn fixture(blinking: Blinking) -> Vec<u8> {
        let cols = usize::from(WS.ws_col);
        // The screen's width, and half of it for a double-width line.
        let (wide, half) = (Cols::of_index(cols), Cols::of_index(cols / 2));
        let mut v = Vec::new();
        v.extend_from_slice(b"\x1b[2$~\x1b[1$}");
        v.extend_from_slice(
            filled("status -> main != done \u{5e9}\u{5dc}\u{5d5}\u{5dd} ", wide).as_bytes(),
        );
        v.extend_from_slice(b"\x1b[0$}\x1b[H");
        v.extend_from_slice(b"\x1b#6");
        v.extend_from_slice(filled(SEEDS[0], half).as_bytes());
        v.extend_from_slice(b"\r\n\x1b#3");
        v.extend_from_slice(filled(SEEDS[5], half).as_bytes());
        v.extend_from_slice(b"\r\n\x1b#4");
        v.extend_from_slice(filled(SEEDS[5], half).as_bytes());
        let main = usize::from(WS.ws_row);
        let mut y = 3;
        while y < main {
            v.extend_from_slice(b"\r\n");
            let seed = SEEDS[y % SEEDS.len()];
            let text = match (y % 4, y + 1 < main) {
                // A line two rows long, which the terminal cut at the
                // margin and word-wrap draws at its words.
                (0, true) => {
                    y += 1;
                    let mut words = String::new();
                    while width(&words).index() < cols * 2 - 8 {
                        words.push_str("wrapped words of a long line ");
                    }
                    filled(&words, Cols::of_index(cols * 2 - 3))
                }
                _ if seed.contains('\x1b') => {
                    let plain = "red green bold plain text -> ";
                    let mut s = String::new();
                    for _ in 0..cols / width(plain).index() {
                        s.push_str(seed);
                    }
                    s.push_str(&filled(plain, Cols::of_index(cols % width(plain).index())));
                    s
                }
                _ => filled(seed, wide),
            };
            v.extend_from_slice(text.as_bytes());
            y += 1;
        }
        // Two links written over the text, and a blinking cell.
        v.extend_from_slice(
            b"\x1b[12;10H\x1b]8;;https://example.com/one\x1b\\link one\x1b]8;;\x1b\\",
        );
        v.extend_from_slice(
            b"\x1b[14;30H\x1b]8;;https://example.com/two\x1b\\link two\x1b]8;;\x1b\\",
        );
        if blinking == Blinking::Yes {
            v.extend_from_slice(b"\x1b[16;40H\x1b[5mBLINK\x1b[25m");
        }
        // A kitty image over the text, and a Sixel.
        v.extend_from_slice(b"\x1b[8;150H");
        v.extend_from_slice(&kitty_image());
        v.extend_from_slice(b"\x1b[20;120H");
        v.extend_from_slice(SWIRL);
        // The cursor: a blinking block, shown, mid-screen.
        v.extend_from_slice(b"\x1b[30;60H\x1b[1 q\x1b[?25h");
        v
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Blinking {
        No,
        Yes,
    }

    /// A 24x16 RGBA kitty image, `a=T` at the cursor, z 0: over the text.
    fn kitty_image() -> Vec<u8> {
        const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let (w, h) = (24u32, 16u32);
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[
                    u8::try_from(x * 10).unwrap(),
                    u8::try_from(y * 15).unwrap(),
                    200,
                    255,
                ]);
            }
        }
        let mut b64 = Vec::new();
        for q in px.chunks(3) {
            let mut b = [0u8; 3];
            b[..q.len()].copy_from_slice(q);
            let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
            for k in 0..4 {
                match k <= q.len() {
                    true => b64.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                    false => b64.push(b'='),
                }
            }
        }
        let mut v = format!("\x1b_Ga=T,f=32,s={w},v={h};").into_bytes();
        v.extend_from_slice(&b64);
        v.extend_from_slice(b"\x1b\\");
        v
    }

    /// The fixture's links, each a run of one rendition, as the app's scan
    /// finds them at rest.
    fn links(t: &Term<'_>) -> Vec<LinkLine> {
        let view = t.viewport();
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
                let (link, word, from) = (c.flags.link(), t.style_of(c), x);
                while x + 1 < n
                    && cells[x + 1].flags.link() == link
                    && t.style_of(cells[x + 1]) == word
                {
                    x += 1;
                }
                out.push(LinkLine {
                    row,
                    from: Col::at_index(from),
                    to: Col::at_index(x),
                    word,
                });
                x += 1;
            }
        }
        out
    }

    /// What one frame did: the renderer's counts and the heap's.
    #[derive(Clone, Copy, Debug)]
    struct Cost {
        counts: Counts,
        heap: Heap,
        /// The rows the CPU path repainted; none on the cell path.
        rows_dirty: Option<Tally>,
        /// The CPU path's frame's totals of its instances; none on the
        /// cell path.
        totals: Option<Totals>,
    }

    /// **A frame's totals of its instances**, as it reports them: all of
    /// them, those on the colour texture and those solid.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Totals {
        instances: Tally,
        colored: Tally,
        solids: Tally,
    }

    impl Totals {
        fn of(f: &baud_render::Frame) -> Totals {
            Totals {
                instances: f.instances,
                colored: f.colored,
                solids: f.solids,
            }
        }

        /// `r`'s totals counted again from every row's instances, as every
        /// frame counted them before FS-C7 kept them.
        fn recounted(r: &Renderer<'_, '_>) -> Totals {
            let (instances, colored, solids) = r.instances_recounted();
            Totals {
                instances,
                colored,
                solids,
            }
        }

        /// **`f`'s totals are the ones every row's instances count**: the
        /// totals the frame reports -- to `report.rs`, the frame bench and
        /// the golden readers -- are those the walk over every instance
        /// that FS-C7 removed would have reported, on the same frame.
        fn hold(f: &baud_render::Frame, r: &Renderer<'_, '_>, what: &str) -> Totals {
            let kept = Totals::of(f);
            assert_eq!(kept, Totals::recounted(r), "{what}: the kept totals");
            kept
        }
    }

    impl Cost {
        /// Both read and started again: what was done since the last.
        fn take() -> Cost {
            let heap = heap::take();
            Cost {
                counts: counters::take(),
                heap,
                rows_dirty: None,
                totals: None,
            }
        }

        fn line(&self) -> String {
            let mut s = String::new();
            for c in Counter::ALL {
                s.push_str(&format!("{}={} ", c.name(), self.counts.of(c).value()));
            }
            s.push_str(&format!(
                "allocations={} bytes={} other_threads={}",
                self.heap.here.value(),
                self.heap.bytes.value(),
                self.heap.elsewhere.value()
            ));
            if let Some(rows) = self.rows_dirty {
                s.push_str(&format!(" rows_dirty={}", rows.value()));
            }
            if let Some(t) = self.totals {
                s.push_str(&format!(
                    " instances={} colored={} solids={}",
                    t.instances.value(),
                    t.colored.value(),
                    t.solids.value()
                ));
            }
            s
        }
    }

    /// What a frame is drawn with beyond the screen: the cursor's blink
    /// phase, the text's, and the link under the pointer.
    #[derive(Clone)]
    struct Inputs {
        caret: CursorPhase,
        blink: BlinkPhases,
        hover: Option<LinkHover>,
        /// The CPU path's damage after the first frame: full on a text
        /// blink's flip, as the app draws it, since the flip changes no
        /// cell for a row's compare to find.
        damage: Damage,
    }

    impl Inputs {
        fn at(caret: CursorPhase) -> Inputs {
            Inputs {
                caret,
                blink: BlinkPhases::SHOWN,
                hover: None,
                damage: Damage::Incremental,
            }
        }
    }

    /// The CPU path's frame as `App::present` draws it on that path: the
    /// inputs set, then `render_at` into the frame's pixels.
    fn cpu_frame(
        r: &mut Renderer<'_, '_>,
        t: &mut Term<'_>,
        inputs: &Inputs,
        target: &mut Target<'_>,
        damage: Damage,
    ) -> baud_render::Frame {
        let back = t.window_back();
        r.fit(t);
        r.set_highlight(None);
        r.set_link(None);
        r.set_link_hover(inputs.hover.as_ref());
        r.set_tip(None);
        r.set_blink(inputs.blink);
        r.set_cursor_blink(inputs.caret);
        r.splash_yields_to(t, Px::of(0));
        r.set_trail(None);
        r.place_marks(t, back, Leaving::None, Px::of(0));
        r.place_lights(t);
        r.render_at(t, back, target, damage)
    }

    /// Pixels the size of `r`'s frame.
    struct Pixels {
        px: Vec<u32>,
        w: Px,
        h: Px,
    }

    impl Pixels {
        fn of(r: &Renderer<'_, '_>) -> Pixels {
            let (w, h) = r.pixel_size();
            let n = usize::try_from(w.value() * h.value()).unwrap();
            Pixels {
                px: vec![0u32; n],
                w,
                h,
            }
        }

        fn target(&mut self) -> Target<'_> {
            Target::new(&mut self.px, self.w, self.w, self.h).expect("a target")
        }
    }

    /// The cell path's buffers, one set a frame in flight as the app keeps
    /// them, and what it keeps between frames.
    struct Cells<'m> {
        map: GlyphMap<'m>,
        exceptions: Overlay,
        screens: Screens,
        copies_mem: [Vec<PackedCell>; Wheel::SETS],
        styles_mem: [Vec<Resolved>; Wheel::SETS],
        overlay_mem: [Vec<u8>; Wheel::SETS],
        style_stamps: [StyleStamp; Wheel::SETS],
        lent: [Lent; Wheel::SETS],
        candidates: Vec<Candidate>,
        records: Vec<Instance>,
        grounds: Vec<Instance>,
        bidi_cells: Vec<PackedCell>,
        bidi_used: Vec<baud_domain::RowExtent>,
        lent_cells: Vec<PackedCell>,
        lent_used: Vec<baud_domain::RowExtent>,
        used: Vec<baud_domain::RowExtent>,
    }

    impl<'m> Cells<'m> {
        fn new(backing: &'m mut [baud_render::MapEntry], t: &Term<'_>) -> Cells<'m> {
            let stride = t.stride().index();
            let (cols, rows) = (usize::from(WS.ws_col), usize::from(WS.ws_row));
            Cells {
                map: GlyphMap::over(backing),
                exceptions: Overlay::new(),
                screens: Screens::new(),
                copies_mem: core::array::from_fn(|_| vec![PackedCell::EMPTY; (rows + 2) * stride]),
                styles_mem: core::array::from_fn(|_| vec![Resolved::default(); t.styles().len()]),
                overlay_mem: core::array::from_fn(|_| vec![0u8; cols * (rows + 1)]),
                style_stamps: [StyleStamp::default(); Wheel::SETS],
                lent: core::array::from_fn(|_| Lent::default()),
                candidates: Vec::new(),
                records: Vec::new(),
                grounds: Vec::new(),
                bidi_cells: Vec::new(),
                bidi_used: Vec::new(),
                lent_cells: Vec::new(),
                lent_used: Vec::new(),
                used: Vec::new(),
            }
        }
    }

    /// **The cell path's frame as `App::present` builds it**, into `set`:
    /// the inputs `present` sets on the renderer first, then the frame
    /// itself through the function `present` builds it with
    /// (`baud_render::cell_frame::build`). No composition, no prediction,
    /// no slide and no notice. Every call here is one `present` makes, in
    /// its order (`the_cell_driver_builds_as_present_builds`).
    fn cell_frame(
        r: &mut Renderer<'_, '_>,
        t: &Term<'_>,
        inputs: &Inputs,
        cell: &mut Cells<'_>,
        at: SetIx,
    ) -> Built {
        let back = t.window_back();
        r.fit(t);
        r.set_highlight(None);
        r.set_link(None);
        r.set_link_hover(inputs.hover.as_ref());
        r.set_tip(None);
        r.set_blink(inputs.blink);
        r.set_cursor_blink(inputs.caret);
        r.set_images(t, back);
        r.splash_yields_to(t, Px::of(0));
        r.set_trail(None);
        r.place_marks(t, back, Leaving::None, Px::of(0));
        r.place_lights(t);
        let live = t
            .cursor_visible()
            .then(|| t.cursor())
            .map(|(row, col)| (row, col, t.cursor_style()));
        let frame_inputs = CellInputs {
            back,
            predictions: core::iter::empty(),
            live,
            composition: &None,
            highlight: None,
            leaving: Leaving::None,
            pan: Px::of(0),
        };
        let mut copies = cell.copies_mem.each_mut().map(|c| &mut c[..]);
        let mut styles = cell.styles_mem.each_mut().map(|c| &mut c[..]);
        let mut overlay = cell.overlay_mem.each_mut().map(|c| &mut c[..]);
        let frame = build(
            r,
            t,
            &frame_inputs,
            &mut CellParts {
                map: &mut cell.map,
                exceptions: &mut cell.exceptions,
                screens: &mut cell.screens,
                copies_mem: &mut copies,
                styles_mem: &mut styles,
                overlay_mem: &mut overlay,
                style_stamps: &mut cell.style_stamps,
                lent: &mut cell.lent,
                candidates: &mut cell.candidates,
                records: &mut cell.records,
                grounds: &mut cell.grounds,
                bidi_cells: &mut cell.bidi_cells,
                bidi_used: &mut cell.bidi_used,
                lent_cells: &mut cell.lent_cells,
                lent_used: &mut cell.lent_used,
            },
            &mut cell.used,
            || Some(at),
        )
        .expect("the set is free");
        frame.built
    }

    /// A screen of the fixture, its links set at rest with the first one
    /// struck, handed to `f` with the faces' renderer.
    fn with_screen(blinking: Blinking, f: impl FnOnce(&mut Term<'_>, &mut Renderer<'_, '_>)) {
        let faces = [
            Face::parse(JETBRAINS).expect("JetBrains Mono"),
            Face::parse(CASCADIA).expect("Cascadia Mono"),
        ];
        let mut r = renderer(&faces);
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, &fixture(blinking));
        let lines = links(&t);
        assert_eq!(lines.len(), 2, "two links on screen");
        r.set_link_lines(&lines, LineStyle::Solid);
        r.set_link_strikes(&lines[..1], &[]);
        f(&mut t, &mut r);
    }

    /// The pointer at rest over the second link, drawn as a box.
    fn hover_over(t: &Term<'_>) -> LinkHover {
        let line = links(t)[1];
        LinkHover {
            spans: vec![LinkSpan {
                row: line.row,
                from: line.from,
                to: line.to,
            }],
            paint: HoverPaint::Box,
        }
    }

    /// The CPU path's frames for `steps`, each read for its cost: the
    /// first at `Damage::Full`, the rest at the damage each step asks.
    fn cpu_costs(
        blinking: Blinking,
        steps: impl Fn(&Term<'_>, Nth) -> Inputs,
        n: Nth,
    ) -> Vec<Cost> {
        let mut costs = Vec::new();
        with_screen(blinking, |t, r| {
            let mut pixels = Pixels::of(r);
            let mut target = pixels.target();
            Cost::take();
            for k in n.before() {
                let inputs = steps(t, k);
                let damage = match k {
                    Nth(0) => Damage::Full,
                    _ => inputs.damage,
                };
                Cost::take();
                let f = cpu_frame(r, t, &inputs, &mut target, damage);
                let cost = Cost::take();
                costs.push(Cost {
                    rows_dirty: Some(f.rows_dirty),
                    totals: Some(Totals::hold(&f, r, &format!("cpu frame {}", k.0))),
                    ..cost
                });
            }
        });
        costs
    }

    /// The cell path's frames for `steps`, each into the next set round,
    /// read for its cost and what it was built from.
    fn cell_costs(
        blinking: Blinking,
        steps: impl Fn(&Term<'_>, Nth) -> Inputs,
        n: Nth,
    ) -> Vec<(Cost, Built)> {
        let mut costs = Vec::new();
        with_screen(blinking, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            Cost::take();
            for k in n.before() {
                let inputs = steps(t, k);
                Cost::take();
                let built = cell_frame(r, t, &inputs, &mut cells, k.set());
                costs.push((Cost::take(), built));
            }
        });
        costs
    }

    /// The caret's phase on frame `k`: shown, then hidden and shown in
    /// turn after the first frames.
    fn caret_at(k: Nth) -> CursorPhase {
        match k.hidden() {
            true => CursorPhase::HIDDEN,
            false => CursorPhase::SHOWN,
        }
    }

    fn print(arm: &str, path: &str, costs: impl Iterator<Item = Cost>) {
        for (k, c) in costs.enumerate() {
            println!("{arm} {path} frame {k}: {}", c.line());
        }
    }

    /// **Every counter moves on a full frame**, on the path that does
    /// that work, so none can read zero because nothing feeds it: the
    /// CPU path's first frame converts, compares, copies, lays out, scans
    /// for pictures and places the links and the images; the cell path's
    /// first frames do all of that but the conversion of the window, and
    /// build the overlay's records. A counter that stopped being fed reads
    /// zero here and fails.
    pub fn every_counter_moves_on_a_full_frame() {
        let still = |_: &Term<'_>, _| Inputs::at(CursorPhase::SHOWN);
        let cpu = cpu_costs(Blinking::No, still, Nth(1));
        let cell = cell_costs(Blinking::No, still, Nth::WARM);
        print("full", "cpu", cpu.iter().copied());
        print("full", "cell", cell.iter().map(|c| c.0));
        let cell_sum = |c: Counter| -> u64 { cell.iter().map(|f| f.0.counts.of(c).value()).sum() };
        for c in Counter::ALL {
            let on_cpu = cpu[0].counts.of(c).value();
            match c {
                Counter::OverlayCellsBuilt => {
                    assert_eq!(on_cpu, 0, "the CPU path builds no overlay")
                }
                _ => assert!(on_cpu > 0, "the CPU path's full frame moves {}", c.name()),
            }
            assert!(
                cell_sum(c) > 0,
                "the cell path's first frames move {}",
                c.name()
            );
        }
        assert_eq!(
            cell[0].1,
            Built::New,
            "the cell path's first frame is a screen of its own"
        );
    }

    /// **(b) A caret frame blinks the cursor, and what it costs**, and
    /// (d) the pointer at rest over a link, and a screen whose text blinks:
    /// each frame's counts printed per path, master's being the numbers
    /// the rows that bring them down start from. The control that makes
    /// (b) mean something: the cursor is drawn on one phase and not the
    /// other, and the pixels at its cell change with it, a row at a time.
    pub fn a_caret_frame_blinks_the_cursor_and_counts_what_it_costs() {
        let n = Nth::ALL;
        with_screen(Blinking::No, |t, r| {
            let mut a = Pixels::of(r);
            let style = t.cursor_style();
            cpu_frame(
                r,
                t,
                &Inputs::at(CursorPhase::SHOWN),
                &mut a.target(),
                Damage::Full,
            );
            assert!(r.cursor_drawn(style), "the shown phase draws the cursor");
            let shown = a.px.clone();
            let f = cpu_frame(
                r,
                t,
                &Inputs::at(CursorPhase::HIDDEN),
                &mut a.target(),
                Damage::Incremental,
            );
            assert!(!r.cursor_drawn(style), "the hidden phase draws none");
            assert_eq!(f.rows_dirty.value(), 1, "the cursor's row alone repaints");
            assert!(shown != a.px, "the cursor's cell changed with its phase");
        });
        let caret = |_: &Term<'_>, k: Nth| Inputs::at(caret_at(k));
        print(
            "caret",
            "cpu",
            cpu_costs(Blinking::No, caret, n).into_iter(),
        );
        let cell = cell_costs(Blinking::No, caret, n);
        for (k, (_, built)) in cell.iter().enumerate().skip(1) {
            assert_eq!(
                *built,
                Built::Last,
                "caret frame {k} is built from the last screen"
            );
        }
        print("caret", "cell", cell.into_iter().map(|c| c.0));
        let hover = |t: &Term<'_>, k: Nth| Inputs {
            hover: Some(hover_over(t)),
            ..Inputs::at(caret_at(k))
        };
        // The control that makes (d) mean something: the link under the
        // pointer is drawn, on the CPU path in other pixels and on the cell
        // path as a bar.
        with_screen(Blinking::No, |t, r| {
            let (mut a, mut b) = (Pixels::of(r), Pixels::of(r));
            let still = Inputs::at(CursorPhase::SHOWN);
            cpu_frame(r, t, &still, &mut a.target(), Damage::Full);
            cpu_frame(r, t, &hover(t, Nth(0)), &mut b.target(), Damage::Full);
            assert!(a.px != b.px, "the link under the pointer is drawn");
        });
        with_screen(Blinking::No, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            cell_frame(r, t, &hover(t, Nth(0)), &mut cells, Nth(0).set());
            assert!(
                !r.hover_bars().is_empty(),
                "and drawn as a bar on the cell path"
            );
        });
        print(
            "hover",
            "cpu",
            cpu_costs(Blinking::No, hover, n).into_iter(),
        );
        print(
            "hover",
            "cell",
            cell_costs(Blinking::No, hover, n).into_iter().map(|c| c.0),
        );
        let text_blink = |_: &Term<'_>, k: Nth| Inputs {
            blink: match k.hidden() {
                false => BlinkPhases::SHOWN,
                true => BlinkPhases {
                    slow: Blink::Hidden,
                    rapid: Blink::Shown,
                },
            },
            damage: match k.warming() {
                true => Damage::Incremental,
                false => Damage::Full,
            },
            ..Inputs::at(CursorPhase::SHOWN)
        };
        // And for the SGR 5 arm: the blinking cell is on screen, seen by
        // the cell path's walk and drawn in other pixels when hidden.
        let hidden = Nth::WARM;
        assert!(
            hidden.hidden(),
            "the first frame after the first ones hides"
        );
        with_screen(Blinking::Yes, |t, r| {
            let (mut a, mut b) = (Pixels::of(r), Pixels::of(r));
            cpu_frame(r, t, &text_blink(t, Nth(0)), &mut a.target(), Damage::Full);
            cpu_frame(r, t, &text_blink(t, hidden), &mut b.target(), Damage::Full);
            assert!(a.px != b.px, "the blinking cell is hidden");
        });
        with_screen(Blinking::Yes, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            cell_frame(r, t, &text_blink(t, Nth(0)), &mut cells, Nth(0).set());
            assert_eq!(
                r.blink_seen(),
                baud_render::Blinking::SLOW,
                "the cell path's walk sees the blinking cell"
            );
        });
        print(
            "sgr5",
            "cpu",
            cpu_costs(Blinking::Yes, text_blink, n).into_iter(),
        );
        print(
            "sgr5",
            "cell",
            cell_costs(Blinking::Yes, text_blink, n)
                .into_iter()
                .map(|c| c.0),
        );
    }

    /// **(c) One cell written mid-screen repaints its row alone**, and
    /// the frame is the one a full frame draws, pixel for pixel; the cell
    /// path takes it as a new screen. The control the caret rows are
    /// checked against: a fix that stops repainting also stops this.
    pub fn a_written_cell_repaints_its_row_alone() {
        with_screen(Blinking::No, |t, r| {
            let (mut a, mut b) = (Pixels::of(r), Pixels::of(r));
            let still = Inputs::at(CursorPhase::SHOWN);
            cpu_frame(r, t, &still, &mut a.target(), Damage::Full);
            cpu_frame(r, t, &still, &mut a.target(), Damage::Incremental);
            feed(t, b"\x1b7\x1b[25;100HX\x1b8");
            Cost::take();
            let f = cpu_frame(r, t, &still, &mut a.target(), Damage::Incremental);
            let cost = Cost::take();
            println!(
                "written cpu: {} rows_dirty={}",
                cost.line(),
                f.rows_dirty.value()
            );
            assert_eq!(f.rows_dirty.value(), 1, "the written row alone repaints");
            cpu_frame(r, t, &still, &mut b.target(), Damage::Full);
            assert!(
                a.px == b.px,
                "the incremental frame is the full frame's pixels"
            );
        });
        with_screen(Blinking::No, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            let still = Inputs::at(CursorPhase::SHOWN);
            for k in Nth::WARM.before() {
                cell_frame(r, t, &still, &mut cells, k.set());
            }
            let next = Nth::WARM.set();
            feed(t, b"\x1b7\x1b[25;100HX\x1b8");
            Cost::take();
            let built = cell_frame(r, t, &still, &mut cells, next);
            println!("written cell: {}", Cost::take().line());
            assert_eq!(built, Built::New, "a written cell is a new screen");
        });
    }

    /// **A marked cell costs a caret frame no allocation** (FS-C10): the
    /// overlay draws a cell whose cluster it already keeps from the kept
    /// glyphs, by borrow, so the screen with more marked cells -- `e` and U+0301
    /// written over a hundred cells of a row of the fixture's marked text
    /// before the first frame, 53 more of them drawn by records -- allocates
    /// on each caret frame on the cell path exactly what the fixture alone
    /// does: the control that more than forty more are records. Named break:
    /// the kept glyphs cloned back for each cell.
    pub fn a_marked_cell_costs_a_caret_frame_no_allocation() {
        let marked = "e\u{301}".repeat(100);
        let (base, more) = (
            caret_cell_frames(None),
            caret_cell_frames(Some(format!("\x1b7\x1b[8;1H{marked}\x1b8").as_bytes())),
        );
        let cells = |f: &(Cost, OverlayStats)| (f.0, f.1.cells.value());
        let (base, more): (Vec<_>, Vec<_>) = (
            base.iter().map(cells).collect(),
            more.iter().map(cells).collect(),
        );
        for (k, ((b, b_cells), (m, m_cells))) in base.iter().zip(&more).enumerate() {
            println!(
                "marked frame {k}: base allocations={} records={b_cells}, \
                 a hundred more marked allocations={} records={m_cells}",
                b.heap.here.value(),
                m.heap.here.value(),
            );
            assert!(*m_cells > b_cells + 40, "frame {k}: the marked cells drawn");
            if !Nth(k).warming() {
                assert_eq!(
                    m.heap.here.value(),
                    b.heap.here.value(),
                    "caret frame {k}: a marked cell allocated"
                );
            }
        }
    }

    /// **The cell path's caret frames**, each read for its cost and the
    /// overlay's stats, on the fixture with `more` written before the
    /// first frame.
    fn caret_cell_frames(more: Option<&[u8]>) -> Vec<(Cost, OverlayStats)> {
        caret_cell_records(more)
            .into_iter()
            .map(|(cost, stats, _)| (cost, stats))
            .collect()
    }

    /// [`caret_cell_frames`], each with the number of records the frame
    /// draws.
    fn caret_cell_records(more: Option<&[u8]>) -> Vec<(Cost, OverlayStats, usize)> {
        let mut out = Vec::new();
        with_screen(Blinking::No, |t, r| {
            if let Some(more) = more {
                feed(t, more);
            }
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            for k in Nth::ALL.before() {
                let inputs = Inputs::at(caret_at(k));
                Cost::take();
                cell_frame(r, t, &inputs, &mut cells, k.set());
                let cost = Cost::take();
                out.push((cost, cells.exceptions.stats(), cells.records.len()));
            }
        });
        out
    }

    /// The CPU path's caret frames, each read for its cost and the frame's
    /// totals, on the fixture with `more` written before the first frame.
    fn caret_cpu_frames(more: &[u8]) -> Vec<(Cost, Totals)> {
        let mut out = Vec::new();
        with_screen(Blinking::No, |t, r| {
            feed(t, more);
            let mut pixels = Pixels::of(r);
            let mut target = pixels.target();
            for k in Nth::ALL.before() {
                let damage = match k {
                    Nth(0) => Damage::Full,
                    _ => Damage::Incremental,
                };
                Cost::take();
                let f = cpu_frame(r, t, &Inputs::at(caret_at(k)), &mut target, damage);
                let cost = Cost::take();
                out.push((
                    Cost {
                        rows_dirty: Some(f.rows_dirty),
                        ..cost
                    },
                    Totals::of(&f),
                ));
            }
        });
        out
    }

    /// The cell path's caret frames, each read for its cost, how many
    /// sized lines it converted and how many records it draws, on the
    /// fixture with `more` written before the first frame.
    fn caret_sized_frames(more: &[u8]) -> Vec<(Cost, u64, usize)> {
        let mut out = Vec::new();
        with_screen(Blinking::No, |t, r| {
            feed(t, more);
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            for k in Nth::ALL.before() {
                let before = r.sized_converted().value();
                Cost::take();
                cell_frame(r, t, &Inputs::at(caret_at(k)), &mut cells, k.set());
                let cost = Cost::take();
                let converted = r.sized_converted().value() - before;
                out.push((cost, converted, cells.records.len()));
            }
        });
        out
    }

    /// **The rows a converted row is checked on** (FS-C19), each `cols`
    /// columns wide: plain text, text in three renditions, wide
    /// characters, ligature pairs and Hebrew, which the layout reorders.
    fn converted_rows(cols: Cols) -> [(&'static str, String); 5] {
        let styled = "\x1b[31mred\x1b[0m \x1b[42mgreen\x1b[0m \x1b[1mbold\x1b[0m plain text -> ";
        [
            ("plain", filled("plain text ", cols)),
            ("styled", styled.repeat(cols.index() / 29)),
            ("wide", "\u{4e2d}".repeat(cols.index() / 2)),
            ("ligature", filled(SEEDS[5], cols)),
            ("hebrew", filled(SEEDS[2], cols)),
        ]
    }

    /// The bytes that write `text` alone on row 31 and erase row 32, each
    /// line in `size` (`ESC # 5` or `ESC # 6`), then put the cursor on
    /// row `at`.
    fn rows_31_and_32(text: &str, size: &str, at: Row) -> Vec<u8> {
        let at = at.index() + 1;
        format!(
            "\x1b[31;1H\x1b[2K\x1b#{size}{text}\x1b[0m\x1b[32;1H\x1b[2K\x1b#{size}\x1b[{at};20H"
        )
        .into_bytes()
    }

    /// **A converted row costs a caret frame no allocation, whatever it
    /// holds** (FS-C19): a row converts into the renderer's own buffers
    /// (`RowScratch`), its cells' codepoints gathered a run at a time into
    /// one of them, so a caret frame whose cursor stands on a row of plain
    /// text, renditions, wide characters, ligatures or Hebrew -- row 31,
    /// the rest of it erased -- allocates exactly what the same frame does
    /// with the cursor on the erased row below it, row 32: on the CPU
    /// path, which converts the cursor's row every caret frame, and on the
    /// cell path, where both rows are double-width and the cursor's sized
    /// line converts every caret frame. The same screen either way, so
    /// everything but the cursor's row costs the frame the same. The
    /// controls: the row draws more than the erased one on each path, the
    /// CPU path's caret frame repaints a row and the cell path's converts
    /// one sized line. Named breaks: the buffers not put back, so each row
    /// begins its own; each buffer left as it was by `begin`
    /// (`row_scratch::tests::begin_empties_every_buffer`).
    pub fn a_converted_row_costs_a_caret_frame_no_allocation() {
        let cols = Cols::of_index(usize::from(WS.ws_col));
        let half = Cols::of_index(cols.index() / 2);
        // Row 31, the row a converted row is checked on, and the erased row
        // below it.
        let (row_31, row_32) = (Row::from_index(30), Row::from_index(31));
        let blank_cpu = caret_cpu_frames(&rows_31_and_32("", "5", row_31));
        let blank_cell = caret_sized_frames(&rows_31_and_32("", "6", row_31));
        let mut wrong = Vec::new();
        for ((kind, text), (_, sized)) in converted_rows(cols).into_iter().zip(converted_rows(half))
        {
            let on = caret_cpu_frames(&rows_31_and_32(&text, "5", row_31));
            let below = caret_cpu_frames(&rows_31_and_32(&text, "5", row_32));
            for (k, ((o, o_totals), (b, _))) in on.iter().zip(&below).enumerate() {
                println!(
                    "{kind} cpu frame {k}: on it allocations={} instances={} rows_dirty={}, \
                     below it allocations={} rows_dirty={}",
                    o.heap.here.value(),
                    o_totals.instances.value(),
                    o.rows_dirty.map_or(0, |t| t.value()),
                    b.heap.here.value(),
                    b.rows_dirty.map_or(0, |t| t.value()),
                );
                assert!(
                    o_totals.instances.value() > blank_cpu[k].1.instances.value(),
                    "{kind} cpu frame {k}: the row drawn"
                );
                if Nth(k).warming() {
                    continue;
                }
                for (arm, c) in [("on it", o), ("below it", b)] {
                    assert!(
                        c.rows_dirty.is_some_and(|t| t.value() >= 1),
                        "{kind} cpu frame {k} {arm}: the cursor's row repainted"
                    );
                }
                if o.heap.here.value() != b.heap.here.value() {
                    wrong.push(format!(
                        "{kind} cpu frame {k}: {} on the row against {} below it\n  {}\n  {}",
                        o.heap.here.value(),
                        b.heap.here.value(),
                        o.line(),
                        b.line()
                    ));
                }
            }
            let on = caret_sized_frames(&rows_31_and_32(&sized, "6", row_31));
            let below = caret_sized_frames(&rows_31_and_32(&sized, "6", row_32));
            for (k, ((o, o_conv, o_records), (b, b_conv, _))) in on.iter().zip(&below).enumerate() {
                println!(
                    "{kind} cell frame {k}: on it allocations={} sized converted={o_conv} \
                     records={o_records}, below it allocations={} sized converted={b_conv}",
                    o.heap.here.value(),
                    b.heap.here.value(),
                );
                assert!(
                    *o_records > blank_cell[k].2,
                    "{kind} cell frame {k}: the row drawn"
                );
                if Nth(k).warming() {
                    continue;
                }
                assert_eq!(
                    (*o_conv, *b_conv),
                    (1, 1),
                    "{kind} cell frame {k}: the cursor's sized line converted"
                );
                if o.heap.here.value() != b.heap.here.value() {
                    wrong.push(format!(
                        "{kind} cell frame {k}: {} on the row against {} below it\n  {}\n  {}",
                        o.heap.here.value(),
                        b.heap.here.value(),
                        o.line(),
                        b.line()
                    ));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "caret frames on a row allocated otherwise than on the erased row:\n{}",
            wrong.join("\n")
        );
    }

    /// **A sized line costs a caret frame no allocation** (FS-C12): a line
    /// that is not single-width is drawn on the cell path by the records
    /// its last conversion kept, read in place, and the key that says
    /// whether they still hold is compared with the line where it stands,
    /// so the fixture with four more such lines -- rows 10 to 13 made
    /// double-width and a double-height pair before the first frame --
    /// allocates on each caret frame exactly what the fixture alone does:
    /// the control that the four draw more than a hundred more records.
    /// Named break: the line's cells copied into a key each frame.
    pub fn a_sized_line_costs_a_caret_frame_no_allocation() {
        let more = b"\x1b7\x1b[10;1H\x1b#6\x1b[11;1H\x1b#6\x1b[12;1H\x1b#3\x1b[13;1H\x1b#4\x1b8";
        let (base, sized) = (caret_cell_records(None), caret_cell_records(Some(more)));
        for (k, ((b, _, b_records), (s, _, s_records))) in base.iter().zip(&sized).enumerate() {
            println!(
                "sized frame {k}: base allocations={} records={b_records}, \
                 four more sized allocations={} records={s_records}",
                b.heap.here.value(),
                s.heap.here.value(),
            );
            assert!(
                *s_records > b_records + 100,
                "frame {k}: the sized lines drawn"
            );
            if !Nth(k).warming() {
                assert_eq!(
                    s.heap.here.value(),
                    b.heap.here.value(),
                    "caret frame {k}: a sized line allocated"
                );
            }
        }
    }

    /// One cell-path frame into `k`'s set with the cursor shown: how many
    /// sized lines it converted, and the records it draws.
    fn sized_frame(
        r: &mut Renderer<'_, '_>,
        t: &Term<'_>,
        cells: &mut Cells<'_>,
        k: Nth,
    ) -> (u64, Vec<Instance>) {
        let before = r.sized_converted().value();
        cell_frame(r, t, &Inputs::at(CursorPhase::SHOWN), cells, k.set());
        (r.sized_converted().value() - before, cells.records.clone())
    }

    /// **A cell written on a sized line converts the line again**
    /// (FS-C12): a sized line's kept records are reused only while
    /// everything it is drawn from holds, its cells among it, so a cell
    /// written on the fixture's DECDWL row between two frames converts that
    /// line alone again and changes the frame's records, and the frames
    /// either side of it, with nothing written, convert none and draw the
    /// same records: the control that the line's other inputs hold still
    /// from frame to frame. Named break: the line's cells left out of what
    /// its kept records are compared on.
    pub fn a_cell_written_on_a_sized_line_converts_it_again() {
        with_screen(Blinking::No, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            for k in Nth::WARM.before() {
                sized_frame(r, t, &mut cells, k);
            }
            let (steady, before) = sized_frame(r, t, &mut cells, Nth::WARM);
            // An `X` over the `a` of `main` on row 1, the DECDWL line, in
            // the rendition already on screen.
            feed(t, b"\x1b7\x1b[1;5HX\x1b8");
            let (written, after) = sized_frame(r, t, &mut cells, Nth(Nth::WARM.0 + 1));
            let (again, kept) = sized_frame(r, t, &mut cells, Nth(Nth::WARM.0 + 2));
            println!(
                "sized lines converted: steady {steady}, written {written}, \
                 after it {again}"
            );
            assert_eq!(
                steady, 0,
                "the control: a steady frame converts no sized line"
            );
            assert_eq!(written, 1, "the written line alone converted again");
            assert_ne!(after, before, "the written cell drawn");
            assert_eq!(again, 0, "the written line's records kept");
            assert_eq!(kept, after, "the kept records drawn");
        });
    }

    /// **A ligature window costs a caret frame no allocation** (FS-C11):
    /// the overlay gathers a window's cells and their codepoints into
    /// buffers it keeps, so the screen with more windows -- `a->b `
    /// written twenty times over a row of the fixture's ligature text
    /// before the first frame -- allocates on each caret frame on the cell
    /// path exactly what the fixture alone does: the control that the
    /// overlay shapes more windows. Named break: a Vec of codepoints made
    /// for each cell a window gathers.
    pub fn a_ligature_window_costs_a_caret_frame_no_allocation() {
        let ligatures = "a->b ".repeat(20);
        let (base, more) = (
            caret_cell_frames(None),
            caret_cell_frames(Some(format!("\x1b7\x1b[7;1H{ligatures}\x1b8").as_bytes())),
        );
        for (k, ((b, b_stats), (m, m_stats))) in base.iter().zip(&more).enumerate() {
            let (b_windows, m_windows) = (b_stats.windows.value(), m_stats.windows.value());
            println!(
                "ligature frame {k}: base allocations={} windows={b_windows}, \
                 more ligatures allocations={} windows={m_windows}",
                b.heap.here.value(),
                m.heap.here.value(),
            );
            assert!(m_windows > b_windows, "frame {k}: the ligatures drawn");
            if !Nth(k).warming() {
                assert_eq!(
                    m.heap.here.value(),
                    b.heap.here.value(),
                    "caret frame {k}: a ligature window allocated"
                );
            }
        }
    }

    /// **A caret frame tallies only the rows it converts** (FS-C7): the
    /// frame's totals of its instances are kept as each row's are set, so
    /// the CPU path's caret frame tallies the rows it converts and not the
    /// screen -- under a tenth of what the first frame, which converts every
    /// row, tallies, where every frame walked every instance before. Named
    /// break: every row tallied again each frame.
    pub fn a_caret_frame_tallies_the_rows_it_converts() {
        let caret = |_: &Term<'_>, k: Nth| Inputs::at(caret_at(k));
        let costs = cpu_costs(Blinking::No, caret, Nth::ALL);
        let tallied = |c: &Cost| c.counts.of(Counter::InstancesTallied).value();
        let first = tallied(&costs[0]);
        for (k, c) in costs.iter().enumerate().skip(1) {
            println!(
                "tallied cpu frame {k}: {} of the first frame's {first}",
                tallied(c)
            );

            assert!(
                tallied(c) * 10 < first,
                "frame {k}: {} tallied, the first frame {first}",
                tallied(c)
            );
        }
    }

    /// **The frame's totals follow every write of a row**, on each path a
    /// row's instances are written by: an incremental frame reports the
    /// totals a full frame of the same screen reports, and draws its
    /// pixels, and a renderer that has drawn nothing before reports them
    /// too, after (a) a row converted with another mix of instances --
    /// text underlined and struck, more solids -- (b) the leaving line of a
    /// slide converted on row 0's buffers, which are saved and put back
    /// (`Renderer::sized_leaving`, the double-width row a scroll of one
    /// line puts into the history above the window) and (c) a resize, which
    /// empties every row. Named breaks, one a path: the row's tally kept
    /// when its instances are set; the saved row put back untallied; the
    /// totals kept through a reset.
    pub fn the_frames_totals_follow_every_write_of_a_row() {
        let still = Inputs::at(CursorPhase::SHOWN);
        // An incremental frame after `event`, and a full frame of the same
        // screen: their totals and their pixels.
        let check =
            |what: &str, event: &dyn Fn(&mut Term<'_>, &mut Renderer<'_, '_>, &mut Pixels)| {
                with_screen(Blinking::No, |t, r| {
                    r.fit(t);
                    let mut a = Pixels::of(r);
                    cpu_frame(r, t, &still, &mut a.target(), Damage::Full);
                    let before = cpu_frame(r, t, &still, &mut a.target(), Damage::Incremental);
                    event(t, r, &mut a);
                    r.fit(t);
                    let (mut a, mut b) = match a.px.len() == Pixels::of(r).px.len() {
                        true => (a, Pixels::of(r)),
                        false => (Pixels::of(r), Pixels::of(r)),
                    };
                    let inc = cpu_frame(r, t, &still, &mut a.target(), Damage::Incremental);
                    Totals::hold(&inc, r, what);
                    let full = cpu_frame(r, t, &still, &mut b.target(), Damage::Full);
                    Totals::hold(&full, r, what);
                    // The same screen drawn by a renderer that has drawn nothing
                    // before: its totals counted from no earlier tally.
                    let faces = [
                        Face::parse(JETBRAINS).expect("JetBrains Mono"),
                        Face::parse(CASCADIA).expect("Cascadia Mono"),
                    ];
                    let mut fresh = renderer(&faces);
                    let lines = links(t);
                    fresh.set_link_lines(&lines, LineStyle::Solid);
                    fresh.set_link_strikes(&lines[..1], &[]);
                    fresh.fit(t);
                    let mut c = Pixels::of(&fresh);
                    let afresh = cpu_frame(&mut fresh, t, &still, &mut c.target(), Damage::Full);
                    println!(
                        "totals {what}: before {:?}, incremental {:?}, full {:?}, afresh {:?}",
                        Totals::of(&before),
                        Totals::of(&inc),
                        Totals::of(&full),
                        Totals::of(&afresh)
                    );
                    assert_eq!(
                        Totals::of(&inc),
                        Totals::of(&full),
                        "{what}: the incremental frame's totals"
                    );
                    assert_eq!(
                        Totals::of(&full),
                        Totals::of(&afresh),
                        "{what}: the totals a fresh renderer counts"
                    );
                    assert!(a.px == b.px, "{what}: the incremental frame's pixels");
                    assert_ne!(
                        Totals::of(&before),
                        Totals::of(&full),
                        "{what}: the screen's totals moved"
                    );
                });
            };
        check("a row's mix", &|t, _, _| {
            feed(
                t,
                b"\x1b7\x1b[25;10H\x1b[4;9munderlined and struck text\x1b[0m\x1b8",
            );
        });
        check("the leaving line", &|t, r, a| {
            // The double-width row into the history, the window scrolled
            // one line, then its frame drawn before the leaving line is.
            feed(t, b"\x1b7\x1b[50;1H\n\x1b8");
            cpu_frame(r, t, &still, &mut a.target(), Damage::Incremental);
            let mut out = Vec::new();
            r.sized_leaving(t, Rows::ZERO, &mut out);
            assert!(!out.is_empty(), "the leaving line is double width");
        });
        check("a resize", &|t, _, _| {
            let smaller = WinSize {
                ws_col: 160,
                ws_row: 40,
            };
            assert_eq!(t.resize(&smaller), baud_core::Resized::Ok, "resized");
        });
    }

    /// The code of `name`'s body in `src`, its comment lines dropped and
    /// its whitespace removed: from its signature to the first `close`
    /// after it, the brace that closes a function at its indent.
    fn body_of(src: &str, name: &str, close: &str) -> String {
        let at = src
            .find(name)
            .unwrap_or_else(|| panic!("{name} is in its file"));
        let end = at + src[at..].find(close).expect("a body");
        src[at..end]
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .flat_map(str::split_whitespace)
            .collect()
    }

    /// A method's body, closed at four spaces' indent.
    const METHOD: &str = "\n    }\n";

    /// Each call in `flat` whose receiver is one of `prefixes`, as the
    /// receiver `spell` gives that prefix and the method, in order: the
    /// receiver is not the tail of a longer path.
    fn calls(flat: &str, prefixes: &[(&str, &str)]) -> Vec<String> {
        let mut out = Vec::new();
        let bytes = flat.as_bytes();
        let mut i = 0;
        while i < flat.len() {
            let before = i.checked_sub(1).map(|j| bytes[j]);
            let free =
                before.is_none_or(|b| !(b.is_ascii_alphanumeric() || b == b'_' || b == b'.'));
            let hit = prefixes
                .iter()
                .find(|(p, _)| free && flat[i..].starts_with(*p));
            let Some((p, spell)) = hit else {
                i += 1;
                continue;
            };
            let rest = &flat[i + p.len()..];
            let n = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            if n > 0 && rest[n..].starts_with('(') {
                out.push(format!("{spell}{}", &rest[..n]));
            }
            i += p.len() + n;
        }
        out
    }

    /// The receivers a call is read on in the app, spelled as the app
    /// spells them.
    const APP: [(&str, &str); 5] = [
        ("self.renderer.bidi().", "self.renderer.bidi()."),
        ("self.renderer.", "self.renderer."),
        ("cell.maps.live.", "cell.maps.live."),
        ("cell.exceptions.", "cell.exceptions."),
        ("screens.", "screens."),
    ];

    /// **The cell driver builds the frame as `App::present` does**: both
    /// call `baud_render::cell_frame::build` once, every call the driver
    /// makes before it into the renderer is one `present` makes, in
    /// `present`'s order, and so is every call `cpu_frame` makes. And
    /// `present` makes none of the calls `build` makes itself -- but the
    /// map's generation, which the GPU's input names the map by -- so no
    /// part of the frame is built twice or built where the driver does not
    /// reach it; `draw_kept` makes the four calls it makes to redraw a kept
    /// frame, named here until it is built through `build` too. A call
    /// added to a driver that the app does not make, one the app moved or
    /// stopped making, or a moved call made in the app again, fails it.
    pub fn the_cell_driver_builds_as_present_builds() {
        let here = include_str!("caret_frame.rs");
        let read = |p: &str| {
            std::fs::read_to_string(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR")))
                .unwrap_or_else(|e| panic!("{p}: {e}"))
        };
        let app = read("baud-app/src/app.rs");
        let present = body_of(
            &app,
            "fn present(&mut self, screen: &DrawableView, show: Show)",
            METHOD,
        );
        let theirs = calls(&present, &APP);
        for (path, name) in [("cell", "fn cell_frame("), ("cpu", "fn cpu_frame(")] {
            let driver = body_of(here, name, METHOD);
            let ours = calls(
                &driver,
                &[
                    ("r.bidi().", "self.renderer.bidi()."),
                    ("r.", "self.renderer."),
                    ("cell.map.", "cell.maps.live."),
                    ("cell.exceptions.", "cell.exceptions."),
                    ("screens.", "screens."),
                ],
            );
            assert!(
                ours.len() > 10,
                "the {path} driver's calls are read: {ours:?}"
            );
            let mut at = 0;
            for call in &ours {
                match theirs[at..].iter().position(|c| c == call) {
                    Some(i) => at += i + 1,
                    None => panic!(
                        "the {path} driver calls {call} where present does not, in present's order"
                    ),
                }
            }
        }
        let built = |flat: &str| flat.matches("cell_frame::build(").count();
        assert_eq!(built(&present), 1, "present builds the frame once");
        let driver = body_of(here, "fn cell_frame(", METHOD);
        assert_eq!(
            driver.matches("=build(").count(),
            1,
            "and the driver once: {driver}"
        );
        let library = read("baud-render/src/cell_frame.rs");
        let moved: std::collections::BTreeSet<String> = calls(
            &body_of(&library, "pub fn build<P>(", "\n}\n"),
            &[
                ("renderer.bidi().", "self.renderer.bidi()."),
                ("renderer.", "self.renderer."),
                ("cell.map.", "cell.maps.live."),
                ("cell.exceptions.", "cell.exceptions."),
                ("screens.", "screens."),
            ],
        )
        .into_iter()
        .collect();
        for control in [
            "screens.take_lent",
            "cell.maps.live.ensure",
            "cell.exceptions.ensure",
            "self.renderer.sync_styles",
            "self.renderer.bidi().window",
        ] {
            assert!(moved.contains(control), "build's calls are read: {moved:?}");
        }
        let made = |flat: &str| -> std::collections::BTreeSet<String> {
            calls(flat, &APP)
                .into_iter()
                .filter(|c| moved.contains(c))
                .collect()
        };
        assert_eq!(
            made(&present),
            ["cell.maps.live.generation"].map(String::from).into(),
            "present makes none of build's calls"
        );
        assert_eq!(
            made(&body_of(
                &app,
                "fn draw_kept(&mut self) -> KeptDraw",
                METHOD
            )),
            [
                "cell.exceptions.ensure",
                "self.renderer.badge",
                "self.renderer.place_grounds",
                "self.renderer.set_cursor_glow_at",
            ]
            .map(String::from)
            .into(),
            "draw_kept makes the calls it redraws a kept frame with"
        );
    }

    /// **No build that ships turns the counters on**: `baud-app` names no
    /// `counters` feature, the workspace's own `baud-render` requirement
    /// names none, and this crate turns it on in its dev-dependencies
    /// alone. Each file is read where it names `baud-render`, the control
    /// that this read the right files.
    pub fn nothing_that_ships_counts() {
        let read = |p: &str| {
            std::fs::read_to_string(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR")))
                .unwrap_or_else(|e| panic!("{p}: {e}"))
        };
        for p in ["baud-app/Cargo.toml", "Cargo.toml"] {
            let toml = read(p);
            assert!(toml.contains("baud-render"), "{p} names baud-render");
            assert!(!toml.contains("counters"), "{p} turns no counters on");
        }
        let own = read("baud-present-macos/Cargo.toml");
        let mut section = String::new();
        let mut on = Vec::new();
        for line in own.lines() {
            if line.starts_with('[') {
                section = line.to_string();
            }
            if line.contains("counters") && !line.trim_start().starts_with('#') {
                on.push(section.clone());
            }
        }
        assert_eq!(
            on,
            ["[target.'cfg(target_os = \"macos\")'.dev-dependencies]"],
            "this crate turns the counters on in its dev-dependencies alone"
        );
    }
}
