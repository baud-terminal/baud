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
//! and a screen with a blinking cell whose text blink flips, apart; and
//! the slot arm, the screen on an opaque page with colour 8 a light grey,
//! where the struck link is lent a slot. Only the controls are asserted --
//! that each counter moves on a full frame, that the caret frame blinks
//! the cursor, that a written cell repaints its row alone and draws the
//! pixels a full frame does, that the slot arm's struck link is lent a
//! slot -- and every count is printed, for the rows that bring each one
//! down to assert their own.
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

const NAMES: [&str; 25] = [
    "every_counter_moves_on_a_full_frame",
    "a_caret_frame_blinks_the_cursor_and_counts_what_it_costs",
    "a_caret_frame_drawn_as_stored_copies_and_compares_no_cell",
    "a_caret_frame_compares_no_row_drawn_in_another_order",
    "a_write_outside_every_reordered_paragraph_compares_no_row_drawn_otherwise",
    "a_display_made_again_compares_no_cell_of_the_copy",
    "a_caret_frame_over_a_re_broken_display_lays_out_and_reads_nothing_at_the_re_cut",
    "a_write_outside_the_display_reads_its_rows_once",
    "a_caret_frame_lays_out_no_paragraph",
    "a_caret_frames_layout_allocates_nothing",
    "every_driver_seals_once_a_frame_before_it_builds",
    "a_written_cell_repaints_its_row_alone",
    "a_marked_cell_costs_a_caret_frame_no_allocation",
    "a_ligature_window_costs_a_caret_frame_no_allocation",
    "a_sized_line_costs_a_caret_frame_no_allocation",
    "a_cell_written_on_a_sized_line_converts_it_again",
    "a_converted_row_costs_a_caret_frame_no_allocation",
    "the_pointer_at_rest_costs_a_caret_frame_no_allocation",
    "a_struck_link_lent_a_slot_counts_what_its_caret_frames_cost",
    "a_caret_frame_places_no_link_record",
    "a_caret_frame_blits_no_image_pixel",
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
            "a_caret_frame_drawn_as_stored_copies_and_compares_no_cell" => {
                checks::a_caret_frame_drawn_as_stored_copies_and_compares_no_cell();
            }
            "a_caret_frame_compares_no_row_drawn_in_another_order" => {
                checks::a_caret_frame_compares_no_row_drawn_in_another_order();
            }
            "a_write_outside_every_reordered_paragraph_compares_no_row_drawn_otherwise" => {
                checks::a_write_outside_every_reordered_paragraph_compares_no_row_drawn_otherwise();
            }
            "a_display_made_again_compares_no_cell_of_the_copy" => {
                checks::a_display_made_again_compares_no_cell_of_the_copy();
            }
            "a_caret_frame_over_a_re_broken_display_lays_out_and_reads_nothing_at_the_re_cut" => {
                checks::a_caret_frame_over_a_re_broken_display_lays_out_and_reads_nothing_at_the_re_cut();
            }
            "a_write_outside_the_display_reads_its_rows_once" => {
                checks::a_write_outside_the_display_reads_its_rows_once();
            }
            "a_caret_frame_lays_out_no_paragraph" => checks::a_caret_frame_lays_out_no_paragraph(),
            "a_caret_frames_layout_allocates_nothing" => {
                checks::a_caret_frames_layout_allocates_nothing();
            }
            "every_driver_seals_once_a_frame_before_it_builds" => {
                checks::every_driver_seals_once_a_frame_before_it_builds();
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
            "the_pointer_at_rest_costs_a_caret_frame_no_allocation" => {
                checks::the_pointer_at_rest_costs_a_caret_frame_no_allocation();
            }
            "a_struck_link_lent_a_slot_counts_what_its_caret_frames_cost" => {
                checks::a_struck_link_lent_a_slot_counts_what_its_caret_frames_cost();
            }
            "a_caret_frame_places_no_link_record" => {
                checks::a_caret_frame_places_no_link_record();
            }
            "a_caret_frame_blits_no_image_pixel" => checks::a_caret_frame_blits_no_image_pixel(),
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
    use baud_domain::{
        BytePos, Col, Cols, ImageBudget, PackedCell, ReadLen, Row, Rows, WinSize, Wrap,
    };
    use baud_font::{Face, Px};
    use baud_present_macos::journal::{SetIx, Wheel};
    use baud_present_macos::screens::{Built, Screens};
    use baud_render::cell_frame::{CellInputs, CellParts, build};
    use baud_render::counters::{self, Counter, Counts, Site};
    use baud_render::{
        Blend, Blink, BlinkPhases, Candidate, CursorPhase, Damage, GlyphMap, HoverPaint, Instance,
        Leaving, Lent, LineStyle, LinkHover, LinkLine, LinkSpan, MissingPaint, Overlay,
        OverlayStats, PxSize, Renderer, Resolved, Stack, StyleStamp, Tally, Target, Theme,
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

    /// **The slot arm's page** (FS-C14): C0's theme with the page opaque,
    /// so a colour reads against the page alone and not over the window's
    /// material. It finds no picture drawn in text: only a page dimmed
    /// below opaque does.
    fn opaque() -> Theme {
        Theme {
            bg_alpha: baud_domain::Chan::from_u8(0xFF),
            ..theme()
        }
    }

    /// Colour 8 a light grey (`baud-render/tests/strike_lending.rs`'s
    /// `GRAY`), which reads `baud_render::MISSING_CONTRAST` on the opaque
    /// page.
    const GREY: &[u8] = b"\x1b]4;8;rgb:c0/c0/c0\x1b\\";

    fn renderer<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
        let r = renderer_on(faces, theme());
        assert!(r.theme().finds_pictures(), "the theme finds pictures");
        r
    }

    /// A renderer of `faces` on `theme`.
    fn renderer_on<'f, 'a>(faces: &'f [Face<'a>], theme: Theme) -> Renderer<'f, 'a> {
        Renderer::new(
            Stack::new(faces).expect("a stack of the two faces"),
            PxSize::of(14.0).expect("a size"),
            theme,
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .expect("a renderer")
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

    /// The status line's text.
    const STATUS: &str = "status -> main != done \u{5e9}\u{5dc}\u{5d5}\u{5dd} ";

    /// **The seeds and status line with no right-to-left text**: the
    /// Hebrew and the Arabic seed's letters, and the status line's, in
    /// Latin, so the layout draws no row in another order.
    const LTR_SEEDS: [&str; 6] = [
        SEEDS[0],
        SEEDS[1],
        "shalom shlm olm 2026 world ",
        "marhaba mrhba blalm 42 ",
        SEEDS[4],
        SEEDS[5],
    ];
    const LTR_STATUS: &str = "status -> main != done shlm ";

    /// **The screen**: the status line, a DECDWL row, a DECDHL pair, rows
    /// full from the seeds -- every fourth one wrapped onto the next as one
    /// line -- two links, a kitty image and a Sixel over the text, a
    /// blinking cell where `blinking` asks, and the cursor a blinking block
    /// mid-screen.
    fn fixture(blinking: Blinking) -> Vec<u8> {
        fixture_of(blinking, &SEEDS, STATUS)
    }

    /// [`fixture`] from `seeds` and with `status` on the status line.
    fn fixture_of(blinking: Blinking, seeds: &[&str; 6], status: &str) -> Vec<u8> {
        let cols = usize::from(WS.ws_col);
        // The screen's width, and half of it for a double-width line.
        let (wide, half) = (Cols::of_index(cols), Cols::of_index(cols / 2));
        let mut v = Vec::new();
        v.extend_from_slice(b"\x1b[2$~\x1b[1$}");
        v.extend_from_slice(filled(status, wide).as_bytes());
        v.extend_from_slice(b"\x1b[0$}\x1b[H");
        v.extend_from_slice(b"\x1b#6");
        v.extend_from_slice(filled(seeds[0], half).as_bytes());
        v.extend_from_slice(b"\r\n\x1b#3");
        v.extend_from_slice(filled(seeds[5], half).as_bytes());
        v.extend_from_slice(b"\r\n\x1b#4");
        v.extend_from_slice(filled(seeds[5], half).as_bytes());
        let main = usize::from(WS.ws_row);
        let mut y = 3;
        while y < main {
            v.extend_from_slice(b"\r\n");
            let seed = seeds[y % seeds.len()];
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
                let (link, word, from) = (c.flags.link(), t.styles().one().word_of(c), x);
                while x + 1 < n
                    && cells[x + 1].flags.link() == link
                    && t.styles().one().word_of(cells[x + 1]) == word
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
            // Where the cells compared and copied were, by site.
            for site in Site::ALL {
                let at = |c: Counter| self.counts.at(site, c).value();
                let (compared, copied) = (at(Counter::CellsCompared), at(Counter::CellsCopied));
                if compared + copied > 0 {
                    s.push_str(&format!(
                        "{}:compared={compared},copied={copied} ",
                        site.name()
                    ));
                }
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
    /// frame's writes sealed first (`Term::seal_writes`), as `present`
    /// seals them before its first ask of the display, then the inputs set,
    /// then `render_at` into the frame's pixels under that seal.
    fn cpu_frame(
        r: &mut Renderer<'_, '_>,
        t: &mut Term<'_>,
        inputs: &Inputs,
        target: &mut Target<'_>,
        damage: Damage,
    ) -> baud_render::Frame {
        let seal = t.seal_writes();
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
        r.render_at(t, seal, back, target, damage)
    }

    /// **The CPU path's frame, then the control that it was drawn into a
    /// target of its size**: [`cpu_frame`] makes `present`'s calls alone
    /// (`the_cell_driver_builds_as_present_builds`), and `present` draws
    /// into the target AppKit hands it without asking the renderer its
    /// size, so the harness asks it here, after the frame's own fit.
    fn cpu_frame_sized(
        r: &mut Renderer<'_, '_>,
        t: &mut Term<'_>,
        inputs: &Inputs,
        target: &mut Target<'_>,
        damage: Damage,
    ) -> baud_render::Frame {
        let frame = cpu_frame(r, t, inputs, target, damage);
        let (w, h) = r.pixel_size();
        assert_eq!(
            target.pixels().len(),
            usize::try_from(w.value() * h.value()).unwrap(),
            "the control: the frame is drawn into a target of its size"
        );
        frame
    }

    /// Pixels the size of `r`'s frame.
    struct Pixels {
        px: Vec<u32>,
        w: Px,
        h: Px,
    }

    /// **Pixels the size of `r`'s frame of `t`, `r` fitted to `t` first**:
    /// sized before the renderer's first fit, a frame is drawn into a
    /// target one row short of the status line's, and every compare of it
    /// leaves that row out.
    fn fitted(r: &mut Renderer<'_, '_>, t: &Term<'_>) -> Pixels {
        r.fit(t);
        Pixels::of(r)
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
                styles_mem: core::array::from_fn(|_| {
                    vec![Resolved::default(); t.styles().one().len()]
                }),
                overlay_mem: core::array::from_fn(|_| vec![0u8; cols * (rows + 1)]),
                style_stamps: [StyleStamp::default(); Wheel::SETS],
                lent: core::array::from_fn(|_| Lent::default()),
                candidates: Vec::new(),
                records: Vec::new(),
                grounds: Vec::new(),
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
    /// its order (`the_cell_driver_builds_as_present_builds`), and the
    /// frame's writes are sealed first (`Term::seal_writes`).
    fn cell_frame(
        r: &mut Renderer<'_, '_>,
        t: &mut Term<'_>,
        inputs: &Inputs,
        cell: &mut Cells<'_>,
        at: SetIx,
    ) -> Built {
        let seal = t.seal_writes();
        let t = &*t;
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
            seal,
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
        with_screen_of(&fixture(blinking), Strike::First, f);
    }

    /// Whether a screen's first link is struck.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Strike {
        First,
        None,
    }

    /// The page a screen is drawn on: C0's, dimmed over the window's
    /// material, or the slot arm's, opaque with colour 8 a light grey.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Page {
        Dimmed,
        Opaque,
    }

    /// A screen of `bytes`, its two links set at rest with the first
    /// struck where `strike` says, handed to `f` with the faces' renderer.
    fn with_screen_of(
        bytes: &[u8],
        strike: Strike,
        f: impl FnOnce(&mut Term<'_>, &mut Renderer<'_, '_>),
    ) {
        with_screen_on(Page::Dimmed, bytes, strike, f);
    }

    /// [`with_screen_of`] on `page`: on the opaque page, the renderer on
    /// [`opaque`] and colour 8 set to [`GREY`] before `bytes` are written.
    fn with_screen_on(
        page: Page,
        bytes: &[u8],
        strike: Strike,
        f: impl FnOnce(&mut Term<'_>, &mut Renderer<'_, '_>),
    ) {
        let faces = [
            Face::parse(JETBRAINS).expect("JetBrains Mono"),
            Face::parse(CASCADIA).expect("Cascadia Mono"),
        ];
        let mut r = match page {
            Page::Dimmed => renderer(&faces),
            Page::Opaque => renderer_on(&faces, opaque()),
        };
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
        let mut t = Term::new(owned.storage(), &WS);
        if page == Page::Opaque {
            feed(&mut t, GREY);
        }
        feed(&mut t, bytes);
        let lines = links(&t);
        assert_eq!(lines.len(), 2, "two links on screen");
        r.set_link_lines(&lines, LineStyle::Solid);
        if strike == Strike::First {
            r.set_link_strikes(&lines[..1], &[]);
        }
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
            let mut pixels = fitted(r, t);
            let mut target = pixels.target();
            Cost::take();
            for k in n.before() {
                let inputs = steps(t, k);
                let damage = match k {
                    Nth(0) => Damage::Full,
                    _ => inputs.damage,
                };
                Cost::take();
                let f = cpu_frame_sized(r, t, &inputs, &mut target, damage);
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
                // This screen holds no finished paragraph, so no display is
                // laid out, and a first frame has none kept to ask a row's
                // generation for: the held screen's frames feed both, and
                // their controls assert it
                // (`a_caret_frame_over_a_re_broken_display_lays_out_and_reads_nothing_at_the_re_cut`,
                // `a_write_outside_the_display_reads_its_rows_once`).
                Counter::DisplaysLaid | Counter::GensRead => continue,
                // The CPU path blits the kitty image and the Sixel; the
                // cell path blits no image, which the GPU draws.
                Counter::ImagePixelsBlitted => {
                    assert!(on_cpu > 0, "the CPU path's full frame blits the images");
                    assert_eq!(cell_sum(c), 0, "the cell path blits no image");
                    continue;
                }
                // The kitty image and the Sixel are both over the text, so
                // neither path builds what a picture under it spans:
                // `baud-render/tests/picture_ranges.rs` feeds it.
                Counter::PictureRangesBuilt => {
                    assert_eq!(on_cpu, 0, "no picture is under the text on the CPU path");
                    assert_eq!(
                        cell_sum(c),
                        0,
                        "no picture is under the text on the cell path"
                    );
                    continue;
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
            let mut a = fitted(r, t);
            let style = t.cursor_style();
            cpu_frame_sized(
                r,
                t,
                &Inputs::at(CursorPhase::SHOWN),
                &mut a.target(),
                Damage::Full,
            );
            assert!(r.cursor_drawn(style), "the shown phase draws the cursor");
            let shown = a.px.clone();
            let f = cpu_frame_sized(
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
            let (mut a, mut b) = (fitted(r, t), fitted(r, t));
            let still = Inputs::at(CursorPhase::SHOWN);
            cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Full);
            cpu_frame_sized(r, t, &hover(t, Nth(0)), &mut b.target(), Damage::Full);
            assert_eq!(
                a.px.len(),
                b.px.len(),
                "the control: both frames are drawn at one size"
            );
            assert!(a.px != b.px, "the link under the pointer is drawn");
        });
        with_screen(Blinking::No, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            let hovered = hover(t, Nth(0));
            cell_frame(r, t, &hovered, &mut cells, Nth(0).set());
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
            let (mut a, mut b) = (fitted(r, t), fitted(r, t));
            cpu_frame_sized(r, t, &text_blink(t, Nth(0)), &mut a.target(), Damage::Full);
            cpu_frame_sized(r, t, &text_blink(t, hidden), &mut b.target(), Damage::Full);
            assert_eq!(
                a.px.len(),
                b.px.len(),
                "the control: both frames are drawn at one size"
            );
            assert!(a.px != b.px, "the blinking cell is hidden");
        });
        with_screen(Blinking::Yes, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            let blinking = text_blink(t, Nth(0));
            cell_frame(r, t, &blinking, &mut cells, Nth(0).set());
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

    /// **A caret frame on a screen drawn as stored copies and compares no
    /// cell** (FS-C5), on either path: the fixture with its Hebrew and
    /// Arabic in Latin ([`LTR_SEEDS`]) and no link struck, so no row is
    /// drawn in another order, no display is re-broken, nothing is veiled
    /// and nothing is lent. On every frame after the first ones, the copy
    /// of the screen (`Site::Screens`), the CPU path's convert
    /// (`Site::Convert`) and the sized lines (`Site::Sized`) read no cell
    /// and write none: nothing was written since the seal each was taken
    /// under. The controls: the first frame copies at those sites -- they
    /// are fed -- the CPU caret frame repaints the cursor's row, and the
    /// cell path's is built from the last screen. Named break: every row
    /// taken as written since the seal.
    pub fn a_caret_frame_drawn_as_stored_copies_and_compares_no_cell() {
        let caret = |_: &Term<'_>, k: Nth| Inputs::at(caret_at(k));
        let ltr = fixture_of(Blinking::No, &LTR_SEEDS, LTR_STATUS);
        let (mut cpu, mut cell) = (Vec::new(), Vec::new());
        with_screen_of(&ltr, Strike::None, |t, r| {
            let mut pixels = fitted(r, t);
            let mut target = pixels.target();
            Cost::take();
            for k in Nth::ALL.before() {
                let inputs = caret(t, k);
                let damage = match k {
                    Nth(0) => Damage::Full,
                    _ => inputs.damage,
                };
                Cost::take();
                let f = cpu_frame_sized(r, t, &inputs, &mut target, damage);
                cpu.push((Cost::take(), f.rows_dirty));
            }
        });
        with_screen_of(&ltr, Strike::None, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            Cost::take();
            for k in Nth::ALL.before() {
                let inputs = caret(t, k);
                Cost::take();
                let built = cell_frame(r, t, &inputs, &mut cells, k.set());
                cell.push((Cost::take(), built));
            }
        });
        print("ltr", "cpu", cpu.iter().map(|c| c.0));
        print("ltr", "cell", cell.iter().map(|c| c.0));
        let fed = |c: &Cost, site: Site| c.counts.at(site, Counter::CellsCopied).value() > 0;
        assert!(
            fed(&cpu[0].0, Site::Convert),
            "the control: the CPU path's first frame copies"
        );
        assert!(
            fed(&cell[0].0, Site::Screens),
            "the control: the cell path's first frame copies"
        );
        assert!(
            fed(&cell[0].0, Site::Sized),
            "the control: its sized lines are taken"
        );
        for k in Nth::ALL.before().filter(|k| !k.warming()) {
            let (cpu, rows) = &cpu[k.0];
            let (cell, built) = &cell[k.0];
            assert_eq!(
                rows.value(),
                1,
                "CPU caret frame {}: the cursor's row repaints",
                k.0
            );
            assert_eq!(
                *built,
                Built::Last,
                "cell caret frame {}: the last screen",
                k.0
            );
            for (path, cost) in [("cpu", cpu), ("cell", cell)] {
                for site in [Site::Screens, Site::Convert, Site::Sized] {
                    for c in [Counter::CellsCompared, Counter::CellsCopied] {
                        assert_eq!(
                            cost.counts.at(site, c).value(),
                            0,
                            "{path} caret frame {}: {} at {}",
                            k.0,
                            c.name(),
                            site.name()
                        );
                    }
                }
            }
        }
    }

    /// **The C0 screen with the shell's prompt mark on its last row**
    /// (FS-C3): [`fixture`]'s screen, with `OSC 133 ; A` at row 50, column
    /// 1, before the cursor's last move, so every row above it is finished
    /// and its wrapped pairs are drawn at their words: a display held. At
    /// column 1 the mark's fresh line moves nothing, and from the output it
    /// opens a prompt and erases nothing.
    fn fixture_held() -> Vec<u8> {
        let mut v = fixture(Blinking::No);
        let caret: &[u8] = b"\x1b[30;60H\x1b[1 q\x1b[?25h";
        assert!(v.ends_with(caret), "the fixture ends on the cursor's move");
        v.truncate(v.len() - caret.len());
        v.extend_from_slice(b"\x1b[50;1H\x1b]133;A\x1b\\");
        v.extend_from_slice(caret);
        v
    }

    /// `c`'s count at the re-cut (`Site::Rebreak`) in `cost`.
    fn at_rebreak(cost: &Cost, c: Counter) -> u64 {
        cost.counts.at(Site::Rebreak, c).value()
    }

    /// **H1: a caret frame over a re-broken display lays out nothing and
    /// reads nothing at the re-cut** (FS-C3): on the held screen
    /// ([`fixture_held`]), on both paths, every caret frame after the
    /// first compares and copies no cell at `Site::Rebreak`, lays no
    /// display out and reads no row's generation, since nothing was
    /// written after the seal its layout is kept under. The controls: every
    /// frame holds a display, and the first lays it out. Named break: an
    /// answer that never holds, which lays the display out on every ask.
    pub fn a_caret_frame_over_a_re_broken_display_lays_out_and_reads_nothing_at_the_re_cut() {
        let caret = |k: Nth| Inputs::at(caret_at(k));
        let held = fixture_held();
        let (mut cpu, mut cell) = (Vec::new(), Vec::new());
        with_screen_of(&held, Strike::First, |t, r| {
            let mut pixels = fitted(r, t);
            let mut target = pixels.target();
            Cost::take();
            for k in Nth::ALL.before() {
                let inputs = caret(k);
                let damage = match k {
                    Nth(0) => Damage::Full,
                    _ => inputs.damage,
                };
                Cost::take();
                let _ = cpu_frame_sized(r, t, &inputs, &mut target, damage);
                cpu.push((Cost::take(), r.bidi().display().is_some()));
            }
        });
        with_screen_of(&held, Strike::First, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            Cost::take();
            for k in Nth::ALL.before() {
                let inputs = caret(k);
                Cost::take();
                let _ = cell_frame(r, t, &inputs, &mut cells, k.set());
                cell.push((Cost::take(), r.bidi().display().is_some()));
            }
        });
        print("held", "cpu", cpu.iter().map(|c| c.0));
        print("held", "cell", cell.iter().map(|c| c.0));
        for (path, costs) in [("cpu", &cpu), ("cell", &cell)] {
            assert!(
                at_rebreak(&costs[0].0, Counter::DisplaysLaid) > 0,
                "the control: the {path} path's first frame lays the display out"
            );
            for (k, (cost, held)) in costs.iter().enumerate() {
                assert!(*held, "the control: {path} frame {k} holds a display");
                if k == 0 {
                    continue;
                }
                for c in [
                    Counter::CellsCompared,
                    Counter::CellsCopied,
                    Counter::DisplaysLaid,
                    Counter::GensRead,
                ] {
                    assert_eq!(
                        at_rebreak(cost, c),
                        0,
                        "{path} caret frame {k}: {} at the re-cut",
                        c.name()
                    );
                }
            }
        }
    }

    /// **The status line written, and no row the held screen's display
    /// read**: what [`a_write_outside_the_display_reads_its_rows_once`]
    /// writes between two frames, checked against the seal before it. The
    /// window is the live screen, so its display read its own rows and the
    /// rows above it its first paragraph's walk reached, the one that ended
    /// the walk included, and none below: how many, which is what an ask
    /// after the write reads.
    fn write_outside(t: &mut Term<'_>) -> u64 {
        let back = t.window_back();
        assert_eq!(back, Rows::ZERO, "the window is the live screen");
        let view = t.view(back);
        let (rows, budget) = (
            view.rows().index(),
            baud_render::CONTEXT / view.cols().index(),
        );
        let mut up = 0;
        loop {
            let want = Rows::of_index(up + 1);
            if t.looking_back(want) != want {
                break;
            }
            up += 1;
            if t.view(want).wrapped(Row::ZERO) != Wrap::Continues || up > budget {
                break;
            }
        }
        let seal = t.sealed();
        feed(t, b"\x1b[1$}X\x1b[0$}");
        let gens = t.row_gens(back);
        assert!(
            t.status_gen().is_some_and(|s| s.after(&seal))
                && (0..rows).all(|y| !gens.after(Row::from_index(y), &seal))
                && (1..=up).all(|k| !t.row_gens(Rows::of_index(k)).after(Row::ZERO, &seal)),
            "the isolation: the status line written, and no row the display read"
        );
        u64::try_from(rows + up).expect("a count")
    }

    /// **H2: a write outside the display reads its rows once** (FS-C3): on
    /// the held screen, the status line written between two caret frames
    /// ([`write_outside`]); on both paths, the next frame reads the
    /// generation of each row the display read once and lays nothing out,
    /// and the frame after reads nothing. Named breaks: the layout not kept
    /// again under the seal, which reads every row on every ask of every
    /// frame after; and a driver that asks before it seals, which reads
    /// them once an ask before its seal and once after it.
    pub fn a_write_outside_the_display_reads_its_rows_once() {
        let caret = |k: Nth| Inputs::at(caret_at(k));
        let held = fixture_held();
        let after = [Nth::WARM, Nth(Nth::WARM.0 + 1)];
        let mut got: Vec<(&str, Vec<Cost>, u64)> = Vec::new();
        with_screen_of(&held, Strike::First, |t, r| {
            let mut pixels = fitted(r, t);
            let mut target = pixels.target();
            for k in Nth::WARM.before() {
                let damage = match k {
                    Nth(0) => Damage::Full,
                    _ => Damage::Incremental,
                };
                let _ = cpu_frame_sized(r, t, &caret(k), &mut target, damage);
            }
            let rows = write_outside(t);
            let mut costs = Vec::new();
            for k in after {
                Cost::take();
                let _ = cpu_frame_sized(r, t, &caret(k), &mut target, Damage::Incremental);
                costs.push(Cost::take());
            }
            got.push(("cpu", costs, rows));
        });
        with_screen_of(&held, Strike::First, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            for k in Nth::WARM.before() {
                let _ = cell_frame(r, t, &caret(k), &mut cells, k.set());
            }
            let rows = write_outside(t);
            let mut costs = Vec::new();
            for k in after {
                Cost::take();
                let _ = cell_frame(r, t, &caret(k), &mut cells, k.set());
                costs.push(Cost::take());
            }
            got.push(("cell", costs, rows));
        });
        for (path, costs, rows) in &got {
            print("outside", path, costs.iter().copied());
            let did = |c: &Cost| {
                (
                    at_rebreak(c, Counter::DisplaysLaid),
                    at_rebreak(c, Counter::GensRead),
                )
            };
            assert_eq!(
                did(&costs[0]),
                (0, *rows),
                "{path}: the frame after the write reads each row its display read once"
            );
            assert_eq!(did(&costs[1]), (0, 0), "{path}: the frame after that");
        }
    }

    /// `c`'s count at the bidirectional layout (`Site::Layout`) in `cost`.
    fn at_layout(cost: &Cost, c: Counter) -> u64 {
        cost.counts.at(Site::Layout, c).value()
    }

    /// **H1: a caret frame lays out no paragraph** (FS-C4): on the C0
    /// screen ([`fixture`], its rows stored) and on the held screen
    /// ([`fixture_held`], a display held), on both paths, every caret frame
    /// after the first lays out no row, reads no row's generation and
    /// compares no cell at `Site::Layout`, since nothing was written after
    /// the seal its layout is kept under. The control: every frame draws
    /// some rows in another order, and not all. Named break: an answer
    /// that never holds, which lays out every paragraph on every frame.
    pub fn a_caret_frame_lays_out_no_paragraph() {
        let caret = |k: Nth| Inputs::at(caret_at(k));
        for (screen, bytes) in [("stored", fixture(Blinking::No)), ("held", fixture_held())] {
            let (mut cpu, mut cell) = (Vec::new(), Vec::new());
            with_screen_of(&bytes, Strike::First, |t, r| {
                let mut pixels = fitted(r, t);
                let mut target = pixels.target();
                Cost::take();
                for k in Nth::ALL.before() {
                    let inputs = caret(k);
                    let damage = match k {
                        Nth(0) => Damage::Full,
                        _ => inputs.damage,
                    };
                    Cost::take();
                    let _ = cpu_frame_sized(r, t, &inputs, &mut target, damage);
                    cpu.push((Cost::take(), drawn_otherwise(t, r)));
                }
            });
            with_screen_of(&bytes, Strike::First, |t, r| {
                let mut backing = GlyphMap::backing();
                let mut cells = Cells::new(&mut backing, t);
                Cost::take();
                for k in Nth::ALL.before() {
                    let inputs = caret(k);
                    Cost::take();
                    let _ = cell_frame(r, t, &inputs, &mut cells, k.set());
                    cell.push((Cost::take(), drawn_otherwise(t, r)));
                }
            });
            print(screen, "cpu", cpu.iter().map(|c| c.0));
            print(screen, "cell", cell.iter().map(|c| c.0));
            for (path, costs) in [("cpu", &cpu), ("cell", &cell)] {
                for (k, (cost, (drawn, rows))) in costs.iter().enumerate() {
                    assert!(
                        0 < *drawn && drawn < rows,
                        "the control: {screen} {path} frame {k} draws {drawn} of {rows} rows \
                         in another order"
                    );
                    if k == 0 {
                        continue;
                    }
                    for c in [
                        Counter::BidiRowsLaid,
                        Counter::GensRead,
                        Counter::CellsCompared,
                    ] {
                        assert_eq!(
                            at_layout(cost, c),
                            0,
                            "{screen} {path} caret frame {k}: {} at the layout",
                            c.name()
                        );
                    }
                }
            }
        }
    }

    /// **H2: a caret frame's layout allocates nothing** (FS-C4): on the C0
    /// screen, on both paths, after each caret frame the path's own layout
    /// is asked once more -- the CPU path's `Renderer::lay_out_bidi`, the
    /// cell path's `Renderer::lay_out_bidi_copied`, each with the find
    /// marks and the link underline it draws through the order -- and on
    /// every caret frame after the first that ask allocates nothing and
    /// lays out no row. The frame's own allocations are printed, not
    /// asserted: other rows' work still allocates. Named break: an answer
    /// that never holds.
    pub fn a_caret_frames_layout_allocates_nothing() {
        let caret = |k: Nth| Inputs::at(caret_at(k));
        let bytes = fixture(Blinking::No);
        let mut got: Vec<(&str, Vec<(Cost, Cost)>)> = Vec::new();
        with_screen_of(&bytes, Strike::First, |t, r| {
            let mut pixels = fitted(r, t);
            let mut target = pixels.target();
            let mut costs = Vec::new();
            for k in Nth::ALL.before() {
                let inputs = caret(k);
                let damage = match k {
                    Nth(0) => Damage::Full,
                    _ => inputs.damage,
                };
                Cost::take();
                let _ = cpu_frame_sized(r, t, &inputs, &mut target, damage);
                let frame = Cost::take();
                let back = t.window_back();
                r.lay_out_bidi(t, back);
                costs.push((frame, Cost::take()));
            }
            got.push(("cpu", costs));
        });
        with_screen_of(&bytes, Strike::First, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            let mut costs = Vec::new();
            for k in Nth::ALL.before() {
                Cost::take();
                let _ = cell_frame(r, t, &caret(k), &mut cells, k.set());
                let frame = Cost::take();
                let back = t.window_back();
                let _ = r.lay_out_bidi_copied(t, back);
                costs.push((frame, Cost::take()));
            }
            got.push(("cell", costs));
        });
        for (path, costs) in &got {
            print("frame", path, costs.iter().map(|c| c.0));
            print("layout asked again", path, costs.iter().map(|c| c.1));
            for (k, (_, again)) in costs.iter().enumerate().skip(1) {
                assert_eq!(
                    again.heap.here.value(),
                    0,
                    "{path} caret frame {k}: the layout asked again allocates"
                );
                assert_eq!(
                    at_layout(again, Counter::BidiRowsLaid),
                    0,
                    "{path} caret frame {k}: the layout asked again lays out a row"
                );
            }
        }
    }

    /// The rows of `t`'s window and its status line that `r`'s last layout
    /// drew in another order, and how many rows there are.
    fn drawn_otherwise(t: &Term<'_>, r: &Renderer<'_, '_>) -> (usize, usize) {
        let rows = t.viewport().rows().index() + usize::from(t.status_line().is_some());
        let drawn = (0..rows)
            .filter(|&y| r.bidi().reordered(Row::from_index(y)))
            .count();
        (drawn, rows)
    }

    /// **R1: a caret frame on the C0 screen compares and copies no row,
    /// those drawn in another order among them** (FS-C4): the Hebrew and
    /// the Arabic seeds' rows and the status line are drawn in another
    /// order, and none was laid out again since the last frame drew it
    /// (`Orders::laid_after`), so no compare is asked of their cells. On
    /// the cell path, as the C0 screen is, its first link struck, the copy
    /// of the screen (`Site::Screens`) compares none and copies none, since
    /// the set a caret frame is built into already holds the last frame's
    /// screen; on the CPU path, its first link struck too, the convert
    /// (`Site::Convert`) compares and copies none (FS-C14): the struck
    /// link's lending rewrites only the rows its cells are on, each the
    /// terminal's again at the next frame's start, and a row it lends as
    /// the last frame lent it is the last frame's -- on C0's page, where
    /// no colour reads `[link] missing_color` and nothing is lent, and on
    /// the slot arm's ([`Page::Opaque`]), where a slot is lent the struck
    /// text every frame. On either path, the window and the status line in
    /// drawn order (`Site::Layout`) are the last frame's, permuted again
    /// nowhere; the layout compares no cell on any frame, so its compared
    /// count is a guard that none comes back there, not a check this
    /// screen can fail. The controls: some rows are drawn in another order,
    /// and not all. Named breaks: the drawn-order copy made whole every
    /// frame; the struck links' lending marking every row of the CPU
    /// path's copy as lent. A row drawn in another order answered as laid
    /// out again every frame (`SetKey::dirt`) cannot go red here, since a
    /// caret frame writes nothing and the key keeps the whole copy without
    /// asking a row: its red is
    /// [`a_write_outside_every_reordered_paragraph_compares_no_row_drawn_otherwise`].
    pub fn a_caret_frame_compares_no_row_drawn_in_another_order() {
        let caret = |_: &Term<'_>, k: Nth| Inputs::at(caret_at(k));
        let mut cell_costs = Vec::new();
        with_screen(Blinking::No, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            Cost::take();
            for k in Nth::ALL.before() {
                let inputs = caret(t, k);
                Cost::take();
                cell_frame(r, t, &inputs, &mut cells, k.set());
                cell_costs.push((k, Cost::take(), drawn_otherwise(t, r)));
            }
        });
        let cpu_on = |page: Page| {
            let mut costs = Vec::new();
            with_screen_on(page, &fixture(Blinking::No), Strike::First, |t, r| {
                let mut pixels = fitted(r, t);
                let mut target = pixels.target();
                Cost::take();
                for k in Nth::ALL.before() {
                    let inputs = caret(t, k);
                    let damage = match k {
                        Nth(0) => Damage::Full,
                        _ => inputs.damage,
                    };
                    Cost::take();
                    let _ = cpu_frame_sized(r, t, &inputs, &mut target, damage);
                    costs.push((k, Cost::take(), drawn_otherwise(t, r)));
                }
            });
            costs
        };
        let (cpu_dimmed, cpu_opaque) = (cpu_on(Page::Dimmed), cpu_on(Page::Opaque));
        print("c0", "cell", cell_costs.iter().map(|c| c.1));
        print("c0", "cpu", cpu_dimmed.iter().map(|c| c.1));
        print("c0 slot", "cpu", cpu_opaque.iter().map(|c| c.1));
        for (path, costs, site) in [
            ("cell", &cell_costs, Site::Screens),
            ("cpu", &cpu_dimmed, Site::Convert),
            ("cpu slot", &cpu_opaque, Site::Convert),
        ] {
            for (k, cost, (drawn, every)) in costs.iter().filter(|c| !c.0.warming()) {
                assert!(
                    0 < *drawn && drawn < every,
                    "the control: {path} caret frame {} draws some rows in another order, {drawn} of {every}",
                    k.0
                );
                for (at, c) in [
                    (site, Counter::CellsCompared),
                    (site, Counter::CellsCopied),
                    (Site::Layout, Counter::CellsCompared),
                    (Site::Layout, Counter::CellsCopied),
                ] {
                    assert_eq!(
                        cost.counts.at(at, c).value(),
                        0,
                        "{path} caret frame {}: {} at {}, {drawn} of {every} rows drawn otherwise",
                        k.0,
                        c.name(),
                        at.name()
                    );
                }
            }
        }
    }

    /// **R1's twin with a row written: a write outside every reordered
    /// paragraph compares no row drawn in another order** (FS-C4). R1's
    /// caret frames write nothing, so a copy's key keeps the whole copy
    /// and never asks a row why it may differ; here one cell is written on
    /// the C0 screen's Latin pair of rows 24-25, which holds no row drawn
    /// in another order, so the key asks every row (`Keeps::Rows`). On the
    /// cell path, its first link struck, the copy of the screen
    /// (`Site::Screens`) compares no cell: the written row is copied
    /// without one, and every other row -- those drawn in another order,
    /// and row 25, laid out again with its paragraph in its own order, as
    /// it was -- is kept. On the CPU path, no link struck, the convert
    /// (`Site::Convert`) compares the written row's cells alone. The
    /// controls: some rows are drawn in another order, rows 24 and 25 are
    /// not, no display is held, each path's write frame lays out rows 24
    /// and 25 again (`Renderer::bidi_work`, the rows of each unit laid out
    /// again: `Counter::BidiRowsLaid` counts only the rows a paragraph
    /// that reorders or joins resolves, none of a Latin pair), and the cell
    /// frame is a new screen. Named breaks: a row drawn in another order
    /// answered as laid out again every frame (`SetKey::dirt`); every row
    /// a relay lifted stamped at its end.
    pub fn a_write_outside_every_reordered_paragraph_compares_no_row_drawn_otherwise() {
        const WRITE: &[u8] = b"\x1b7\x1b[25;100HX\x1b8";
        let pair = [Row::from_index(24), Row::from_index(25)];
        let cols = u64::from(WS.ws_col);
        let still = Inputs::at(CursorPhase::SHOWN);
        let controls = |t: &Term<'_>, r: &Renderer<'_, '_>, path: &str| {
            let (drawn, every) = drawn_otherwise(t, r);
            assert!(
                0 < drawn && drawn < every,
                "the control: {path} draws some rows in another order, {drawn} of {every}"
            );
            assert!(
                pair.iter().all(|&y| !r.bidi().reordered(y)),
                "the control: {path} draws rows 24 and 25 in their own order"
            );
            assert!(
                r.bidi().display().is_none(),
                "the control: {path} holds no display"
            );
        };
        with_screen(Blinking::No, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            for k in Nth::WARM.before() {
                cell_frame(r, t, &still, &mut cells, k.set());
            }
            controls(&*t, &*r, "the cell path");
            feed(t, WRITE);
            Cost::take();
            let built = cell_frame(r, t, &still, &mut cells, Nth::WARM.set());
            let cost = Cost::take();
            println!("write outside cell: {}", cost.line());
            assert_eq!(
                r.bidi_work().laid.value(),
                2,
                "the control: the cell path's write frame lays out rows 24 and 25 again"
            );
            assert_eq!(built, Built::New, "the control: a new screen");
            assert_eq!(
                cost.counts
                    .at(Site::Screens, Counter::CellsCompared)
                    .value(),
                0,
                "cell: the copy of the screen compares no cell"
            );
        });
        with_screen_of(&fixture(Blinking::No), Strike::None, |t, r| {
            let mut pixels = fitted(r, t);
            let mut target = pixels.target();
            for k in Nth::WARM.before() {
                let damage = match k {
                    Nth(0) => Damage::Full,
                    _ => Damage::Incremental,
                };
                let _ = cpu_frame_sized(r, t, &still, &mut target, damage);
            }
            controls(&*t, &*r, "the CPU path");
            feed(t, WRITE);
            Cost::take();
            let _ = cpu_frame_sized(r, t, &still, &mut target, Damage::Incremental);
            let cost = Cost::take();
            println!("write outside cpu: {}", cost.line());
            assert_eq!(
                r.bidi_work().laid.value(),
                2,
                "the control: the CPU path's write frame lays out rows 24 and 25 again"
            );
            let compared = cost
                .counts
                .at(Site::Convert, Counter::CellsCompared)
                .value();
            assert!(
                compared <= cols,
                "cpu: the convert compares {compared} cells, the written row {cols}"
            );
        });
    }

    /// **A display made again compares no cell of the copy** (FS-C4): on
    /// the held screen ([`fixture_held`]), cell path, cells written inside
    /// the first pair of rows its display re-breaks lay the display out
    /// again, so no set's copy is kept under the display it was drawn from
    /// and the copy of the screen (`Site::Screens`) compares no cell, each
    /// row copied afresh. The controls: the re-cut lays a display out, a
    /// display is held, and the frame is a new screen. Named break: a
    /// copy's key without the display it was drawn from, which keeps the
    /// copy's rows and compares each, every row of the maps laid out again
    /// under the new display.
    pub fn a_display_made_again_compares_no_cell_of_the_copy() {
        let still = Inputs::at(CursorPhase::SHOWN);
        with_screen_of(&fixture_held(), Strike::First, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            for k in Nth::WARM.before() {
                cell_frame(r, t, &still, &mut cells, k.set());
            }
            feed(t, b"\x1b7\x1b[5;5HQQQQQQQQ\x1b8");
            Cost::take();
            let built = cell_frame(r, t, &still, &mut cells, Nth::WARM.set());
            let cost = Cost::take();
            println!("display made again cell: {}", cost.line());
            assert!(
                at_rebreak(&cost, Counter::DisplaysLaid) > 0,
                "the control: the re-cut lays the display out again"
            );
            assert!(r.bidi().display().is_some(), "the control: a display held");
            assert_eq!(built, Built::New, "the control: a new screen");
            assert_eq!(
                cost.counts
                    .at(Site::Screens, Counter::CellsCompared)
                    .value(),
                0,
                "the copy of the screen compares no cell"
            );
        });
    }

    /// **(c) One cell written mid-screen repaints its row alone**, and
    /// the frame is the one a full frame draws, pixel for pixel; the cell
    /// path takes it as a new screen. The control the caret rows are
    /// checked against: a fix that stops repainting also stops this. The
    /// control on the CPU path: the write frame lays out the written row's
    /// paragraph, rows 24 and 25, again (`Renderer::bidi_work`), so row 25,
    /// laid out again in its own order as it was, is not repainted.
    pub fn a_written_cell_repaints_its_row_alone() {
        with_screen(Blinking::No, |t, r| {
            let (mut a, mut b) = (fitted(r, t), fitted(r, t));
            let still = Inputs::at(CursorPhase::SHOWN);
            cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Full);
            cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Incremental);
            feed(t, b"\x1b7\x1b[25;100HX\x1b8");
            Cost::take();
            let f = cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Incremental);
            let cost = Cost::take();
            println!(
                "written cpu: {} rows_dirty={}",
                cost.line(),
                f.rows_dirty.value()
            );
            assert_eq!(
                r.bidi_work().laid.value(),
                2,
                "the control: the write frame lays out rows 24 and 25 again"
            );
            assert_eq!(f.rows_dirty.value(), 1, "the written row alone repaints");
            cpu_frame_sized(r, t, &still, &mut b.target(), Damage::Full);
            assert_eq!(
                a.px.len(),
                b.px.len(),
                "the control: both frames are drawn at one size"
            );
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
            let mut pixels = fitted(r, t);
            let mut target = pixels.target();
            for k in Nth::ALL.before() {
                let damage = match k {
                    Nth(0) => Damage::Full,
                    _ => Damage::Incremental,
                };
                Cost::take();
                let f = cpu_frame_sized(r, t, &Inputs::at(caret_at(k)), &mut target, damage);
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

    /// **The pointer at rest over a link costs a caret frame no
    /// allocation for its fields** (FS-C16), arm (d): the fields the link
    /// under the pointer is drawn as are kept with the records they were
    /// composed into while nothing they are drawn from moves (`Veiled`), so
    /// on each path each caret frame with the pointer at rest over the
    /// second link drawn as a box allocates exactly what the same frame
    /// does with it drawn as a line, which has no fields, and composes no
    /// veil. The line, not the caret arm (b), is the control because both
    /// looks find the link's runs in the row's order each frame
    /// (`Orders::stored_runs`, two allocations a run, FS-C4's), and the
    /// fields are what this counts. The controls: the box's first frame
    /// composes its fields, and the line's composes none. Named break: the
    /// key never holding, so every frame composes its veils again.
    pub fn the_pointer_at_rest_costs_a_caret_frame_no_allocation() {
        let n = Nth::ALL;
        let at_rest = |paint: HoverPaint| {
            move |t: &Term<'_>, k: Nth| Inputs {
                hover: Some(LinkHover {
                    paint,
                    ..hover_over(t)
                }),
                ..Inputs::at(caret_at(k))
            }
        };
        let mut wrong = Vec::new();
        let (line, boxed) = (
            cpu_costs(Blinking::No, at_rest(HoverPaint::Line), n),
            cpu_costs(Blinking::No, at_rest(HoverPaint::Box), n),
        );
        let caret = cpu_costs(Blinking::No, |_, k| Inputs::at(caret_at(k)), n);
        let (cell_line, cell_box) = (
            cell_costs(Blinking::No, at_rest(HoverPaint::Line), n),
            cell_costs(Blinking::No, at_rest(HoverPaint::Box), n),
        );
        for (k, ((l, _), (b, _))) in cell_line.iter().zip(&cell_box).enumerate() {
            println!(
                "hover cell frame {k}: box allocations={}, line allocations={}",
                b.heap.here.value(),
                l.heap.here.value()
            );
            if !Nth(k).warming() && b.heap.here != l.heap.here {
                wrong.push(format!(
                    "cell caret frame {k}: {} for the box against {} for the line\n  {}\n  {}",
                    b.heap.here.value(),
                    l.heap.here.value(),
                    b.line(),
                    l.line()
                ));
            }
        }
        for (k, ((l, b), c)) in line.iter().zip(&boxed).zip(&caret).enumerate() {
            println!(
                "hover cpu frame {k}: box allocations={}, line allocations={}, \
                 no pointer allocations={}",
                b.heap.here.value(),
                l.heap.here.value(),
                c.heap.here.value()
            );
            if !Nth(k).warming() && b.heap.here != l.heap.here {
                wrong.push(format!(
                    "cpu caret frame {k}: {} for the box against {} for the line\n  {}\n  {}",
                    b.heap.here.value(),
                    l.heap.here.value(),
                    b.line(),
                    l.line()
                ));
            }
        }
        for (paint, first) in [(HoverPaint::Box, true), (HoverPaint::Line, false)] {
            let mut composed = Vec::new();
            with_screen(Blinking::No, |t, r| {
                let mut pixels = fitted(r, t);
                let mut target = pixels.target();
                for k in n.before() {
                    let damage = match k {
                        Nth(0) => Damage::Full,
                        _ => Damage::Incremental,
                    };
                    let before = r.veils_composed().value();
                    cpu_frame_sized(r, t, &at_rest(paint)(t, k), &mut target, damage);
                    composed.push(r.veils_composed().value() - before);
                }
            });
            with_screen(Blinking::No, |t, r| {
                let mut backing = GlyphMap::backing();
                let mut cells = Cells::new(&mut backing, t);
                for k in n.before() {
                    let before = r.veils_composed().value();
                    let inputs = at_rest(paint)(t, k);
                    cell_frame(r, t, &inputs, &mut cells, k.set());
                    let c = r.veils_composed().value() - before;
                    if !k.warming() && c != 0 {
                        wrong.push(format!(
                            "cell caret frame {}: {c} veils composed again for the {paint:?}",
                            k.0
                        ));
                    }
                }
            });
            assert_eq!(
                composed[0] > 0,
                first,
                "the control: the first frame composes the {paint:?}'s fields, if any"
            );
            for (k, c) in composed.iter().enumerate().skip(Nth::WARM.0) {
                if *c != 0 {
                    wrong.push(format!(
                        "cpu caret frame {k}: {c} veils composed again for the {paint:?}"
                    ));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "the pointer at rest cost caret frames more than its runs:\n{}",
            wrong.join("\n")
        );
    }

    /// The cell path's caret frames on the fixture's screen on `page`, its
    /// first link struck where `strike` says, each into the next set round:
    /// each frame's cost, and the slots its set lent and left short.
    fn slot_cell_frames(page: Page, strike: Strike) -> Vec<(Cost, Tally, Tally)> {
        let mut out = Vec::new();
        with_screen_on(page, &fixture(Blinking::No), strike, |t, r| {
            let mut backing = GlyphMap::backing();
            let mut cells = Cells::new(&mut backing, t);
            Cost::take();
            for k in Nth::ALL.before() {
                let inputs = Inputs::at(caret_at(k));
                Cost::take();
                cell_frame(r, t, &inputs, &mut cells, k.set());
                let cost = Cost::take();
                let lent = &cells.lent[k.set().0];
                out.push((cost, lent.lent(), lent.short()));
            }
        });
        out
    }

    /// The CPU path's caret frames on the fixture's screen on `page`, its
    /// first link struck where `strike` says, each read for its cost.
    fn slot_cpu_frames(page: Page, strike: Strike) -> Vec<Cost> {
        let mut out = Vec::new();
        with_screen_on(page, &fixture(Blinking::No), strike, |t, r| {
            let mut pixels = fitted(r, t);
            let mut target = pixels.target();
            Cost::take();
            for k in Nth::ALL.before() {
                let damage = match k {
                    Nth(0) => Damage::Full,
                    _ => Damage::Incremental,
                };
                let inputs = Inputs::at(caret_at(k));
                Cost::take();
                let f = cpu_frame_sized(r, t, &inputs, &mut target, damage);
                let cost = Cost::take();
                out.push(Cost {
                    rows_dirty: Some(f.rows_dirty),
                    ..cost
                });
            }
        });
        out
    }

    /// The CPU path's first frame of the fixture's screen on `page`, its
    /// first link struck and drawn with `missing` as `[link]
    /// missing_color`.
    fn struck_pixels(page: Page, missing: MissingPaint) -> Vec<u32> {
        let mut px = Vec::new();
        with_screen_on(page, &fixture(Blinking::No), Strike::First, |t, r| {
            r.set_link_missing(missing);
            let mut a = fitted(r, t);
            let still = Inputs::at(CursorPhase::SHOWN);
            cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Full);
            px = a.px;
        });
        px
    }

    /// **The slot arm: a struck link lent a slot, and what its caret
    /// frames cost** (FS-C14): the fixture's screen on an opaque page
    /// with colour 8 a light grey ([`opaque`], [`GREY`]), so its struck
    /// link's text reads `[link] missing_color` at
    /// `baud_render::MISSING_CONTRAST` and is drawn in a slot lent to it.
    /// On C0's own page, dimmed over the window's material, no colour
    /// reads, so nothing is lent there and the lent view is never copied.
    /// Each path's caret frames on the slot arm are printed beside the
    /// same frames with no link struck: the cells copied and compared by
    /// site -- the lent view's copy at `Site::Lent` among them -- the link
    /// records placed and the allocations, none asserted.
    ///
    /// The controls, asserted: on the cell path, every frame on the slot
    /// arm lends the struck link a slot, is short of none and copies the
    /// lent view, counted at `Site::Lent`; with no link struck, or on C0's
    /// page, a frame lends nothing and copies nothing there. On the CPU
    /// path, the struck link drawn in `missing_color` is drawn otherwise
    /// than under `"none"` on the opaque page, and alike on C0's: the
    /// colour reads, which is what the CPU path lends its text.
    pub fn a_struck_link_lent_a_slot_counts_what_its_caret_frames_cost() {
        let lent_copied = |c: &Cost| c.counts.at(Site::Lent, Counter::CellsCopied).value();
        let (struck, none, dimmed) = (
            slot_cell_frames(Page::Opaque, Strike::First),
            slot_cell_frames(Page::Opaque, Strike::None),
            slot_cell_frames(Page::Dimmed, Strike::First),
        );
        print("slot", "cell", struck.iter().map(|c| c.0));
        print("slot unstruck", "cell", none.iter().map(|c| c.0));
        for (k, ((s, s_lent, s_short), (n, n_lent, _))) in struck.iter().zip(&none).enumerate() {
            println!(
                "slot cell frame {k}: lent={} short={} lent_copied={}, allocations struck={} \
                 unstruck={}, other_threads struck={} unstruck={}",
                s_lent.value(),
                s_short.value(),
                lent_copied(s),
                s.heap.here.value(),
                n.heap.here.value(),
                s.heap.elsewhere.value(),
                n.heap.elsewhere.value(),
            );
            assert!(
                s_lent.value() > 0 && s_short.value() == 0,
                "the control: cell frame {k} lends the struck link a slot, none short"
            );
            assert!(
                lent_copied(s) > 0,
                "the control: cell frame {k} copies the lent view, counted at {}",
                Site::Lent.name()
            );
            assert!(
                n_lent.value() == 0 && lent_copied(n) == 0,
                "the control: cell frame {k} with no link struck lends nothing"
            );
        }
        for (k, (d, d_lent, _)) in dimmed.iter().enumerate() {
            assert!(
                d_lent.value() == 0 && lent_copied(d) == 0,
                "the control: cell frame {k} on C0's page lends nothing"
            );
        }
        let (struck, none) = (
            slot_cpu_frames(Page::Opaque, Strike::First),
            slot_cpu_frames(Page::Opaque, Strike::None),
        );
        print("slot", "cpu", struck.iter().copied());
        print("slot unstruck", "cpu", none.iter().copied());
        for (k, (s, n)) in struck.iter().zip(&none).enumerate() {
            println!(
                "slot cpu frame {k}: allocations struck={} unstruck={}, other_threads \
                 struck={} unstruck={}",
                s.heap.here.value(),
                n.heap.here.value(),
                s.heap.elsewhere.value(),
                n.heap.elsewhere.value(),
            );
        }
        for (page, reads) in [(Page::Opaque, true), (Page::Dimmed, false)] {
            let grey = struck_pixels(page, MissingPaint::Gray);
            let kept = struck_pixels(page, MissingPaint::Kept);
            assert_eq!(
                grey.len(),
                kept.len(),
                "the control: both frames are drawn at one size"
            );
            assert_eq!(
                grey != kept,
                reads,
                "the control: on the {page:?} page, missing_color reads on the struck link"
            );
        }
    }

    /// **A caret frame places no link record** (FS-C6): the solids of the
    /// lines at rest, the strikes and `hover = "line"` read no cell, and are
    /// kept while nothing they are drawn from moved, so on C0's screen, the
    /// first link struck, each caret frame past the first ones places none
    /// on either path -- with the pointer at rest over the second link,
    /// drawn as a box or as a line, too. Its controls: each arm's first
    /// frame places them, and a CPU frame after the strikes changed with
    /// nothing written places them again and draws what a full frame,
    /// which places them whatever their key, draws. Named break: the
    /// solids never kept.
    pub fn a_caret_frame_places_no_link_record() {
        let n = Nth::ALL;
        let placed = |c: &Cost| {
            c.counts
                .at(Site::Links, Counter::LinkInstancesBuilt)
                .value()
        };
        let at_rest = |paint: Option<HoverPaint>| {
            move |t: &Term<'_>, k: Nth| Inputs {
                hover: paint.map(|paint| LinkHover {
                    paint,
                    ..hover_over(t)
                }),
                ..Inputs::at(caret_at(k))
            }
        };
        let mut wrong = Vec::new();
        for (arm, paint) in [
            ("caret", None),
            ("hover box", Some(HoverPaint::Box)),
            ("hover line", Some(HoverPaint::Line)),
        ] {
            let cpu = cpu_costs(Blinking::No, at_rest(paint), n);
            let cell: Vec<Cost> = cell_costs(Blinking::No, at_rest(paint), n)
                .into_iter()
                .map(|c| c.0)
                .collect();
            for (path, costs) in [("cpu", &cpu), ("cell", &cell)] {
                assert!(
                    placed(&costs[0]) > 0,
                    "the control: the {arm} arm's first {path} frame places the links"
                );
                for (k, c) in costs.iter().enumerate() {
                    println!(
                        "placed {arm} {path} frame {k}: link_instances_built={}",
                        placed(c)
                    );
                    if !Nth(k).warming() && placed(c) != 0 {
                        wrong.push(format!(
                            "{arm} {path} caret frame {k}: {} link records placed\n  {}",
                            placed(c),
                            c.line()
                        ));
                    }
                }
            }
        }
        // The control that a kept placement still moves: both links struck
        // with nothing written, on the CPU path.
        with_screen(Blinking::No, |t, r| {
            let still = Inputs::at(CursorPhase::SHOWN);
            // Fitted before `a` is sized: the fixture shows a status line,
            // whose row the renderer counts only once fitted, and `b` below
            // is sized after the frames have fitted it.
            r.fit(t);
            let mut a = Pixels::of(r);
            for k in Nth::WARM.before() {
                let damage = match k {
                    Nth(0) => Damage::Full,
                    _ => Damage::Incremental,
                };
                cpu_frame_sized(r, t, &still, &mut a.target(), damage);
            }
            let lines = links(t);
            r.set_link_strikes(&lines[..2], &[]);
            Cost::take();
            cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Incremental);
            let cost = Cost::take();
            assert!(
                placed(&cost) > 0,
                "the control: a frame after the strikes changed places the links again"
            );
            let mut b = Pixels::of(r);
            cpu_frame_sized(r, t, &still, &mut b.target(), Damage::Full);
            assert_eq!(
                a.px.len(),
                b.px.len(),
                "the control: both frames are drawn at one size"
            );
            assert!(
                a.px == b.px,
                "the control: the frame after the strikes changed draws what a full frame draws"
            );
        });
        assert!(
            wrong.is_empty(),
            "caret frames placed link records:\n{}",
            wrong.join("\n")
        );
    }

    /// **A sized line costs a caret frame no allocation** (FS-C12): a line
    /// that is not single-width is drawn on the cell path by the records
    /// its last conversion kept, read in place, and the key that says
    /// whether they still hold is compared with the line where it stands,
    /// so the fixture with four more such lines -- rows 10 to 13 made
    /// double-width and a double-height pair before the first frame --
    /// allocates on each caret frame no more than the fixture alone does:
    /// the control that the four draw more than a hundred more records.
    /// Not exactly as much: once the layout allocates nothing on a caret
    /// frame (FS-C4), the fixture alone reads one allocation more than the
    /// four more sized lines. Named break: the line's cells copied into a
    /// key each frame.
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
                assert!(
                    s.heap.here.value() <= b.heap.here.value(),
                    "caret frame {k}: a sized line allocated: {} against {}",
                    s.heap.here.value(),
                    b.heap.here.value()
                );
            }
        }
    }

    /// One cell-path frame into `k`'s set with the cursor shown: how many
    /// sized lines it converted, and the records it draws.
    fn sized_frame(
        r: &mut Renderer<'_, '_>,
        t: &mut Term<'_>,
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

    /// **A caret frame blits no image pixel** (FS-C8): an image over the
    /// text is blitted only into the rows a frame repaints, so on C0's
    /// screen, whose kitty image and Sixel are on no row the caret is on,
    /// each CPU caret frame past the first ones blits none and repaints the
    /// caret's row alone, where every frame blitted both images over the
    /// whole frame. Its controls: the first frame blits the images, and a
    /// cell written on the Sixel's middle row -- the swirl, over three rows
    /// or more -- blits that row's band of the swirl and no more, which a
    /// blit of the whole placement would not. Named break: every image
    /// blitted over the whole frame on every frame.
    pub fn a_caret_frame_blits_no_image_pixel() {
        let blitted = |c: &Cost| {
            c.counts
                .at(Site::Images, Counter::ImagePixelsBlitted)
                .value()
        };
        let caret = |_: &Term<'_>, k: Nth| Inputs::at(caret_at(k));
        let cpu = cpu_costs(Blinking::No, caret, Nth::ALL);
        assert!(
            blitted(&cpu[0]) > 0,
            "the control: the first frame blits the images"
        );
        let mut wrong = Vec::new();
        for (k, c) in cpu.iter().enumerate() {
            let dirty = c.rows_dirty.map(|t| t.value());
            println!(
                "blit cpu frame {k}: image_pixels_blitted={} rows_dirty={dirty:?}",
                blitted(c)
            );
            if !Nth(k).warming() && (blitted(c) != 0 || dirty != Some(1)) {
                wrong.push(format!(
                    "cpu caret frame {k}: {} image pixels blitted, rows_dirty {dirty:?}\n  {}",
                    blitted(c),
                    c.line()
                ));
            }
        }
        // The control: a cell written on the swirl's middle row, away from
        // its columns, blits the band of the swirl on that row alone.
        with_screen(Blinking::No, |t, r| {
            let still = Inputs::at(CursorPhase::SHOWN);
            r.fit(t);
            let mut a = Pixels::of(r);
            cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Full);
            cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Incremental);
            let ch = i64::from(r.metrics().cell_h.value());
            let width = i64::from(r.pixel_size().0.value());
            let swirl = r
                .image_placements()
                .iter()
                .max_by_key(|p| p.h)
                .expect("the premise: the images are placed");
            let (top, h) = (i64::from(swirl.y), i64::from(swirl.h));
            let (first, last) = (top.div_euclid(ch), (top + h - 1).div_euclid(ch));
            assert!(
                last - first >= 2,
                "the premise: the swirl covers three rows or more, {first}..={last}"
            );
            let mid = (first + last) / 2;
            let rows = (top + h).min((mid + 1) * ch) - top.max(mid * ch);
            let cols =
                (i64::from(swirl.x) + i64::from(swirl.w)).min(width) - i64::from(swirl.x).max(0);
            let want = u64::try_from(rows * cols).expect("a count");
            feed(t, format!("\x1b7\x1b[{};1HX\x1b8", mid + 1).as_bytes());
            Cost::take();
            let f = cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Incremental);
            let cost = Cost::take();
            println!(
                "blit written cpu: image_pixels_blitted={} want={want} rows_dirty={}",
                blitted(&cost),
                f.rows_dirty.value()
            );
            assert_eq!(
                f.rows_dirty.value(),
                1,
                "the premise: the written row alone repaints"
            );
            assert!(
                u64::try_from(h * cols).expect("a count") > want,
                "the premise: the swirl is more than the row's band of it"
            );
            assert_eq!(
                blitted(&cost),
                want,
                "the control: the written row's band of the swirl is blitted, and no more"
            );
        });
        assert!(
            wrong.is_empty(),
            "a caret frame blitted an image:\n{}",
            wrong.join("\n")
        );
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
                    cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Full);
                    let before =
                        cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Incremental);
                    event(t, r, &mut a);
                    r.fit(t);
                    let (mut a, mut b) = match a.px.len() == Pixels::of(r).px.len() {
                        true => (a, Pixels::of(r)),
                        false => (Pixels::of(r), Pixels::of(r)),
                    };
                    let inc = cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Incremental);
                    Totals::hold(&inc, r, what);
                    let full = cpu_frame_sized(r, t, &still, &mut b.target(), Damage::Full);
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
                    let afresh =
                        cpu_frame_sized(&mut fresh, t, &still, &mut c.target(), Damage::Full);
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
                    assert_eq!(
                        a.px.len(),
                        b.px.len(),
                        "{what}: the control, both frames are drawn at one size"
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
            cpu_frame_sized(r, t, &still, &mut a.target(), Damage::Incremental);
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

    /// **Every driver seals once a frame, before it builds** (FS-C5): the
    /// cell path's build takes the seal as an argument and seals nothing
    /// itself, so `App::present` seals once, after the link scan (which
    /// keeps the seal it read under) and before `cell_frame::build`, and
    /// hands that seal in; the cell driver here does the same; and
    /// `Renderer::render_at`, which every CPU driver goes through, takes
    /// the seal as the build does and seals nothing itself. A present that
    /// stopped sealing would build every caret frame as a new screen, every
    /// row read as written; one that sealed before the scan would hide a
    /// frame's writes from it.
    ///
    /// **And each seals before its first ask of the display** (FS-C3): the
    /// re-cut keeps its layout under the seal a reader on `&Term` names,
    /// which a write since the frame's last seal is after, so an ask before
    /// the seal reads the rows again, once an ask. `present` and the cell
    /// driver seal before `set_images` and `place_marks`, the CPU driver
    /// here before `place_marks`, and the Linux driver in its `plan`, after
    /// its link scan as on macOS, which `present` runs before its first
    /// `place_marks`.
    pub fn every_driver_seals_once_a_frame_before_it_builds() {
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
        let at = |flat: &str, what: &str| {
            flat.find(what)
                .unwrap_or_else(|| panic!("{what} is in the body: {flat}"))
        };
        assert_eq!(
            present.matches("seal_writes()").count(),
            1,
            "present seals once a frame"
        );
        assert!(
            at(&present, "self.scan_lines()") < at(&present, "seal_writes()")
                && at(&present, "seal_writes()") < at(&present, "cell_frame::build("),
            "present seals after the link scan and before the build"
        );
        assert!(
            present.contains("letseal=self.term.seal_writes();"),
            "present keeps the seal it takes: {present}"
        );
        for ask in [".set_images(", ".place_marks("] {
            assert!(
                at(&present, "seal_writes()") < at(&present, ask),
                "present seals before its first {ask}"
            );
        }
        let driver = body_of(here, "fn cell_frame(", METHOD);
        assert_eq!(
            driver.matches("seal_writes()").count(),
            1,
            "the driver seals once a frame"
        );
        assert!(
            at(&driver, "letseal=t.seal_writes();") < at(&driver, "=build("),
            "the driver seals before it builds"
        );
        for ask in ["r.set_images(", "r.place_marks("] {
            assert!(
                at(&driver, "letseal=t.seal_writes();") < at(&driver, ask),
                "the driver seals before its first {ask}"
            );
        }
        let cpu = body_of(here, "fn cpu_frame(", METHOD);
        assert_eq!(
            cpu.matches("seal_writes()").count(),
            1,
            "the CPU driver seals once"
        );
        assert!(
            at(&cpu, "t.seal_writes()") < at(&cpu, "r.place_marks("),
            "the CPU driver seals before its first ask"
        );
        let linux = read("baud-app/src/linux.rs");
        let shown = body_of(&linux, "fn present(&mut self) {", METHOD);
        let planned = body_of(&linux, "fn plan(&mut self)", METHOD);
        assert_eq!(
            (
                planned.matches("seal_writes()").count(),
                shown.matches("seal_writes()").count()
            ),
            (1, 0),
            "the Linux driver seals once a frame, in its plan"
        );
        assert!(
            at(&shown, "self.plan()") < at(&shown, ".place_marks("),
            "the Linux driver plans, and so seals, before its first ask"
        );
        assert!(
            at(&planned, "self.lines.scan(") < at(&planned, "seal_writes()"),
            "the Linux driver's plan seals after its link scan"
        );
        let library = read("baud-render/src/cell_frame.rs");
        let build = body_of(&library, "pub fn build<P>(", "\n}\n");
        assert!(
            build.contains("term:&implTermRead,seal:Seal,"),
            "the build takes the seal: {build}"
        );
        assert!(
            !build.contains("seal_writes(") && !build.contains("sealed()"),
            "and takes none itself"
        );
        let renderer = read("baud-render/src/renderer.rs");
        let render_at = body_of(&renderer, "pub fn render_at(", METHOD);
        assert!(
            render_at.contains("term:&implTermRead,seal:Seal,"),
            "render_at takes the seal: {render_at}"
        );
        assert!(
            !render_at.contains("seal_writes(") && !render_at.contains("sealed()"),
            "and takes none itself"
        );
    }

    /// **The cell driver builds the frame as `App::present` does**: both
    /// call `baud_render::cell_frame::build` once, every call the driver
    /// makes before it into the renderer is one `present` makes, in
    /// `present`'s order, and so is every call `cpu_frame` makes. And
    /// `present` makes none of the calls `build` makes itself, its read of
    /// the frame's damage and the calls of the `build_with` it hands the
    /// damage to -- but the
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
        // `build` reads the frame's damage and hands it to `build_with`,
        // which builds the frame, so build's calls are both bodies' calls.
        let library = read("baud-render/src/cell_frame.rs");
        let moved: std::collections::BTreeSet<String> =
            ["pub fn build<P>(", "pub fn build_with<P>("]
                .into_iter()
                .flat_map(|name| {
                    calls(
                        &body_of(&library, name, "\n}\n"),
                        &[
                            ("renderer.bidi().", "self.renderer.bidi()."),
                            ("renderer.", "self.renderer."),
                            ("cell.map.", "cell.maps.live."),
                            ("cell.exceptions.", "cell.exceptions."),
                            ("screens.", "screens."),
                        ],
                    )
                })
                .collect();
        for control in [
            "self.renderer.damage",
            "screens.take_below",
            "cell.maps.live.ensure_with",
            "cell.exceptions.ensure_framed",
            "self.renderer.sync_styles",
            "self.renderer.bidi().window_kept",
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
                "self.renderer.badge",
                "self.renderer.overlay_rows",
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
