//! Caller-provided storage: the arena bytes come in through, and the cells
//! they become.
//!
//! **Nothing here allocates.** The core is `no_std` and `alloc`-free: a caller
//! without an allocator hands in `&'static mut` slices from a linker section.
//!
//! **A caller *with* one gets [`Owned`], behind the `alloc` feature, and it is
//! not `Storage::boxed`.** This file used to claim it was, which was a
//! sentence carried over from `baud-core2` and false here: core2's `Storage`
//! *owned* `[PackedCell; GRID]` and could be boxed zeroed, while this one holds
//! **borrows**, so there is nothing for a `Box<Storage>` to own. [`Owned`]
//! holds the nine allocations and lends them, which is the same convenience in
//! the shape this design admits.

use baud_domain::*;

/// The read buffer, sized to what one `read(2)` returns.
///
/// `N_TTY_BUF_SIZE` caps a tty read at 4095 bytes on Linux
/// (`[[source: linux-n-tty]]`) and Darwin hands back 1024, the capacity of the
/// replica's output clist (`TTYCLSIZE`, `[[source: xnu-tty-h]]`) -- so a larger
/// buffer receives nothing extra. A smaller one is not a correctness
/// question: the parser keeps its state across reads, and
/// `the_screen_does_not_depend_on_where_the_reads_fell` holds the screen equal
/// for reads of 1 to 64 bytes and of 4096.
pub const READ_CHUNK: usize = 4096;

/// The arena the reads land in.
///
/// 1 MiB, from a sweep on `baud-core2`'s ring, where reads advanced through
/// the arena; it peaked there against a 2 MiB L2. Every caller outside the
/// tests places each read at `BytePos::ZERO`, so on this core it is a ceiling
/// on one read rather than a working set, and nothing walks it to re-sweep
/// (`docs/record.md`, "A resize moves no cell, and the first fixed pitch was
/// the wrong one").
pub const ARENA: usize = 1 << 20;

/// The widest and tallest grid addressable. A `CSI 8 ; r ; c t` past these is
/// clamped rather than refused.
pub const MAX_COLS: usize = 2048;
pub const MAX_ROWS: usize = 640;

/// How many characters one run may hold.
///
/// **Not the lane count.** A block holds at most eight two-byte characters,
/// and capping the run there caps every vectored write at the same number --
/// the run-length histogram showed 25% of runs sitting exactly at the block's
/// limit, which is the hardware talking rather than the text. Runs extend past
/// the block, and this is where the decode buffer stops instead.
pub const STRIDE_MAX: usize = 32;

/// Cells in the widest, tallest viewport: the ceiling on the *live* grid.
///
/// Not the allocation behind it, which is `rows * stride` -- and the pitch
/// [`grid_stride`] chooses follows the provisioned width rather than
/// `MAX_COLS` (208 cells for the corpus on a 128-byte line, 2096 for
/// `MAX_COLS` itself), so a caller sizing memory asks [`RowBlock::leading`]
/// with the pitch rather than multiplying these two.
pub const GRID: usize = MAX_ROWS * MAX_COLS;

/// The row pitch a terminal's grid is laid out at, for the cache hierarchy
/// `geom` describes and the width a caller provisions: an odd number of
/// lines so it is a multiple of no power-of-two way at any level and no
/// page, stepped clear of a wrap of every L1 way and no wider. The width
/// rather than `MAX_COLS`, because with the grid as the scrollback ring every
/// line costs one row of the pitch. See [`store_stride`].
///
/// One function, so every allocator of a grid -- [`Owned`], a bench, an
/// embedder placing the viewport in GPU memory -- lays it out at the same
/// pitch the core will address it by.
pub fn grid_stride(geom: &CacheGeometry, cols: Cols) -> Stride {
    store_stride(geom, cols)
}

/// Everything the core writes into, **placed by the caller**.
///
/// Three borrows rather than three arrays, and that is the contract rather
/// than a convenience: the grid is 24 MB at full size, which is too much to
/// return by value and too much to put on a stack. A caller with an allocator
/// hands in `Vec`s, one without hands in `&'static mut` from a linker section,
/// and the core never learns the difference.
///
/// `Term` takes this by value and holds the borrows, so there is one lifetime
/// on the parser rather than a borrow of a borrow.
pub struct Storage<'b> {
    /// Bytes as they arrived. A read writes here and the parser reads here;
    /// nothing copies between.
    pub(crate) arena: &'b mut [u8],
    /// The primary screen's rows and the lines behind them: `used.len()`
    /// physical rows at the pitch, of which the screen is a window.
    pub(crate) vp: &'b mut [PackedCell],
    /// How many of `vp`'s rows are scrollback: the ring is the screen's
    /// capacity plus this.
    pub(crate) depth: Depth,
    /// The pitch `vp` and `alt` are laid out at: row `p` begins at
    /// `p * stride` cells. **Fixed for the life of the storage** and at least
    /// the provisioned width, so a resize is a change of how many cells in
    /// each row are live and moves none of them. Chosen by [`grid_stride`]
    /// from the cache geometry a host detected -- an odd number of lines, so
    /// it aliases no way, stepped clear of a wrap of every L1 way -- or by
    /// [`Stride::of_cells`] in a bench arm reproducing another layout.
    pub(crate) stride: Stride,
    /// How far each physical row stops being uniform. Blanking stops here, so
    /// a mostly-empty row costs a fraction of its width.
    pub(crate) used: &'b mut [RowExtent],
    /// **The cells a three-byte character becomes, indexed by its bytes.**
    ///
    /// `table[b0][b1][b2]` -- 1024 prefixes x 256 entries -- so the write is a
    /// store of a value already in hand rather than a decode, a property
    /// lookup and a construction. Built once by [`Term::new`], which is why it
    /// is caller-provided memory rather than rodata: 2 MiB is more than a
    /// `const fn` should evaluate, and nothing about it is per-terminal.
    ///
    /// Nothing dedupes here and that is the point: a row carrying the codepoint
    /// is unique by construction, because the codepoint *is* the concatenation
    /// of the index bytes. Storing it is how you stop computing it.
    pub(crate) cells3: &'b mut [PackedCell],
    /// The rendition table. A cell holds a [`StyleId`] *index* into this;
    /// **bounded by construction** and open-addressed: a table keyed on what
    /// an application sends needs a bound. It frees the slots no cell names
    /// when it sweeps, and evicts only when a full table's sweep frees none
    /// or is not yet due (`Paint::sweep_styles`). **Slot 0 is reserved for
    /// the default** (`Term::new`), which every blank cell names without
    /// interning it, and is never evicted.
    ///
    /// `Option` rather than a parallel validity array: absent and occupied are
    /// two states of one thing, and a `bool` beside it is the missing enum the
    /// audit exists to catch.
    pub(crate) tmpl: &'b mut [Option<StyleWord>],
    /// Which slot each write to `tmpl` touched, and how many there have been:
    /// what a renderer that caches the resolved table replays instead of
    /// resolving every slot a frame. See [`StyleTable`].
    pub(crate) style_log: StyleLog<'b>,
    /// Tab stops, one per column.
    pub(crate) tabs: &'b mut [bool],
    /// Interned grapheme clusters, indexed **from the cell**.
    ///
    /// See [`ClusterStore`]. The index rides in the cell's `Content`, so it
    /// moves when the cell moves -- which is what makes it survive `RowRot`, a
    /// frame's read and the alternate screen without a parallel structure to
    /// keep in step, and is exactly what `baud-core2`'s position key did not.
    pub(crate) clusters: ClusterStore<'b>,
    /// Everything a control string writes. See [`StrBuf`].
    pub(crate) strs: StrBuf<'b>,
    /// **The alternate screen's own surface**: its cells, its extents and its
    /// rotation, whichever screen is up. `vp`, `used` and the terminal's
    /// rotation are always the primary's, and this is always the alternate
    /// screen's.
    ///
    /// This was once where the primary waited: `?1049` copied the primary out
    /// and back and the grid was handed over, which is also why `?47` --
    /// which switches buffers *without* clearing, so its alternate content
    /// has to survive an exit -- could not be expressed. Since the grid became
    /// the scrollback ring the two screens are two surfaces, and a read that
    /// runs on the alternate screen swaps them inside its `View`
    /// (`View::swap_surface`), so a transition copies nothing and
    /// `Term::set_rots` puts each rotation back where it belongs. So `?47`
    /// is expressible, and it is routed: `?1049` clears this surface on
    /// entry, and `?47` shows it as it was left, rows, marks and rotation.
    ///
    /// The per-character loop still never learns which screen is up:
    /// `View` is built **once per read**, over whichever surface is live.
    pub(crate) alt: AltBuf<'b>,
    /// The primary ring's bookkeeping; see [`Book`].
    pub(crate) book: Book<'b>,
    /// Bytes the terminal owes the host: the answer to a `DA`, a `DSR`, a
    /// `DECRQM` or an `XTVERSION`.
    ///
    /// **Caller-provided like everything else.** This core owns nothing, which
    /// is the property `multibench`'s manifest calls "the point of it" -- so
    /// the reply buffer is a slice the embedder hands over, not an array the
    /// core keeps. Without one the queries were recognised, counted and
    /// deliberately unanswered, which `docs/vt-coverage.md` rates as better
    /// than answering in the wrong shape but worse than answering.
    pub(crate) reply: &'b mut [u8],
    /// Stored images and their placements; see [`ImageStore`]. Bounded,
    /// refuse-and-count, caller-owned like everything else.
    ///
    /// **The pixel arena defaults absent**, the `cells3` opt-in shape: a
    /// caller that wants images provisions it with [`Owned::with_images`], and
    /// without it every image is refused and counted -- a terminal with no
    /// image memory draws none and says how many it dropped, which is the
    /// refuse-and-count contract rather than a silent nothing.
    pub(crate) images: ImageStore<'b>,
}

/// The alternate screen's surface: its cells, its extents and its rotation.
///
/// **One struct rather than three fields, so they cannot be swapped
/// separately.** `RowExtent` is indexed by *physical* row and `RowRot` is what
/// makes a physical row out of a screen row, so the three are one value; a
/// swap missing either gives a grid whose rows are in the wrong order or
/// whose blanking stops at the other surface's marks. `View::swap_surface`
/// exchanges all three, and the ring, in one call for the same reason, and
/// `Paint` carries the screen that is not up as one `Surface`.
pub struct AltBuf<'b> {
    pub(crate) cells: &'b mut [PackedCell],
    pub(crate) used: &'b mut [RowExtent],
    /// The alternate screen's rotation, into a ring of `used.len()` rows.
    /// Kept while it is not up: `?1049` resets it on entry, because it
    /// clears the screen and blanks rotate to blanks, and `?47` keeps it,
    /// because it shows the rows as they were left and in that order.
    pub(crate) rot: RowRot,
}

/// What the scroll paths keep about the primary ring, a word `Paint`
/// carries: how many rows behind the window are lines that scrolled off,
/// and how many after it were cut by a shorter window and can come back.
/// Grid bookkeeping, so it lives here rather than in `Cold`, whose size is
/// pinned.
pub struct Book<'b> {
    pub(crate) hist: Rows,
    pub(crate) hidden: Rows,
    /// **Which full reset the terminal is at** ([`crate::Term::resets`]),
    /// bumped by `RIS` beside `hist`, which it zeroes. Here rather than in
    /// `Cold`, whose size is pinned at 464 and had no padding left for a
    /// `u32` (its assert refused it), and beside the history counts a reset
    /// is the other half of (note V, V1). `Paint` carries this word by
    /// reference, so the per-byte loop carries nothing more for it.
    pub(crate) resets: crate::term::Resets,
    /// **The absolute line at the top of the primary window.** It moves with
    /// the rotation both ways -- up one for every row the window retires,
    /// down one for every row it takes back -- and a reflow renumbers it with
    /// the rows. So screen row `y` is line `top + y`, a line `n` rows behind
    /// the window is `top - 1 - n`, and the oldest retained is `top - hist`.
    /// An image placement anchors to `top + cursor row`, so it follows its row
    /// through every move the window makes. Unbounded, where `hist` is capped
    /// at the depth, so two images past a full ring get distinct lines.
    pub(crate) top: LineIdx,
    /// The same for the alternate screen, which keeps no history: its line at
    /// the top of the window. Kept only while an image is placed or an SPA
    /// is open -- nothing else reads it -- and restarted each time the
    /// screen is entered.
    pub(crate) alt_top: LineIdx,
    /// **How far the primary's rows hold anything**: the width `Term::new` is
    /// given, raised by a growth to the new width, and at every resize that
    /// leaves it past the live width lowered to the furthest any row of the
    /// ring holds a cell or a mark (`View::held_past_edge`) -- so a screen
    /// narrowed with nothing past the new edge, or re-cut, is back at its
    /// width. Every cell at or past it is as the storage was provisioned, in
    /// every row, while the storage has been lent to this `Term` alone --
    /// `Term::new` does not clear it -- because nothing writes at or past
    /// here but a fill to a mark, which the measure counts, and the re-cut's
    /// and `View::widened`'s blanks; the repair's own copy and clear
    /// (`View::copy_past_edge`, `View::clear_past_edge`) stop short of here.
    /// So a row moved inside the screen carries
    /// everything a later growth can show when it carries its cells to here:
    /// what the region repair copies past a narrower live edge
    /// (`View::copy_past_edge`). Here and not in `Cold`, as the rest of this
    /// word: the grid's bookkeeping. Lowered to the live width, too, when the
    /// last row `past` holds is carried off or cleared (`held`).
    pub(crate) reach: Cols,
    /// How many rows `past` holds, while `reach` lies past the live width:
    /// at zero nothing lies past the edge, and `reach` comes back to the
    /// live width.
    pub(crate) held: Rows,
    /// **Whether each physical row of the primary holds anything past the
    /// live edge**, one a row of the ring: what the passes that carry cells
    /// past a narrower edge read to skip a row with nothing there
    /// (`baud_domain::PastRows`). Set whole at a resize that leaves `reach`
    /// past the live width and read only while it is, so `Term::new` need not
    /// clear it -- by the passes, and by the next resize's measure, which
    /// reads a row they say is clear one cell deep (`View::held_past_edge`).
    /// Here rather than beside `used` because `Paint` carries this word by
    /// reference, and a slice of its own through that struct would be two
    /// more words in the per-byte loop.
    pub(crate) past: &'b mut [PastEdge],
    /// **The top line when the rows the window's bottom swept were last
    /// cleared past the edge** (`View::clear_swept_past_edge`): `top` minus
    /// this is how many lines a full-screen scroll has brought in on rows
    /// whose cells past the edge are a retired line's. Set at every resize
    /// and after each pass that clears them; read only while `reach` lies
    /// past the live width.
    pub(crate) swept: LineIdx,
    /// **The reading of [`crate::Term::history_erasures`] the watch on the
    /// erasures was armed at** ([`crate::Term::watch_erasures`]): what
    /// [`crate::Term::first_erasure`] answers for. Not grid bookkeeping, and
    /// `Paint` never reads it -- only the embedder's two calls touch it --
    /// but here because it filled this word's four bytes of end padding, so
    /// `Book` stayed 80 bytes with every field at base's offset. As a field
    /// of `Term` it moved the twelve fields of `Term` after `utf8` by four
    /// and cost about 0.3% more instructions retired on `cat`; `Cold`'s
    /// three bytes of padding held the kept erasure and not this word too.
    pub(crate) watched_at: crate::term::HistoryErasures,
    /// **The primary's window over the history** (note V, V2): the live
    /// screen, or the absolute line at its top. An absolute line holds its
    /// text as lines retire with no write at all -- `top` moves past it --
    /// so the per-byte path that retires a line touches nothing here. What
    /// moves it is what takes lines back, at the byte: a full-screen scroll
    /// down, `RI` at the top and a prompt redraw's retreat send it to the
    /// live screen when they reach its line or empty the history
    /// ([`Book::window_taken_back`]); `CSI 3 J` and `RIS` send it there; a
    /// resize maps it through the height step and the re-cut. The floor is
    /// applied as it is read ([`Book::window_back`]): a line the ring
    /// recycled reads as the oldest kept, so retiring lines writes nothing
    /// here either. Last in the word, so every field before it keeps its
    /// offset; the word grows from 80 bytes to 96, and `Paint` carries it by
    /// reference. The relayout measured flat in instructions retired on
    /// `cat`, `btop`, `vim` and `less`, none of which reaches the take-back
    /// on the primary (the record, "The terminal keeps the window over the
    /// history").
    pub(crate) window: HistoryWindow,
}

/// Where the primary's window over the history stands: the live screen, or
/// with this line at its top. An enum rather than an `Option`, as a two-state
/// field is here. `At(l)` names a line before the window's top line, and
/// one older than the oldest retained line reads as that line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum HistoryWindow {
    Live,
    At(LineIdx),
}

// The word's size, as the window's field says: 80 bytes before it.
const _: () = assert!(core::mem::size_of::<Book<'static>>() == 96);

impl Book<'_> {
    /// How many lines above the primary window's top the window over the
    /// history starts: zero when it is live, and bounded by the history,
    /// which is the floor -- a line the ring recycled reads as the oldest
    /// line kept, as an anchor clamped at every retirement would.
    pub(crate) fn window_back(&self) -> Rows {
        match self.window {
            HistoryWindow::Live => Rows::ZERO,
            HistoryWindow::At(l) => self.top.since(l).map_or(Rows::ZERO, |n| n.min(self.hist)),
        }
    }

    /// Put the window `back` lines above the primary window's top, bounded
    /// by the history: none, or no history, is the live screen.
    pub(crate) fn set_window_back(&mut self, back: Rows) {
        let back = back.min(self.hist);
        self.window = match (back.index(), self.top.minus(back)) {
            (1.., Some(l)) => HistoryWindow::At(l),
            _ => HistoryWindow::Live,
        };
    }

    /// **The window after the primary's top line moved back or the history
    /// shrank**, at the byte that did it: on the live screen once its line
    /// is at or past the top -- the line is on the live screen now -- or
    /// once no history is left behind the window. Live is where it stays as
    /// later lines retire; a window carried by the net movement of a whole
    /// read came back up the history by those lines, so the same bytes left
    /// it live or five lines up by where the reads fell. One compare on the
    /// live window, which is every scroll down nobody is looking back
    /// through.
    #[inline(always)]
    pub(crate) fn window_taken_back(&mut self) {
        if let HistoryWindow::At(l) = self.window {
            if l >= self.top || self.hist.index() == 0 {
                self.window = HistoryWindow::Live;
            }
        }
    }
}

/// Everything a control string writes, in one struct.
///
/// **Six slices rather than six arguments, for the reason `AltBuf` is one
/// argument.** `Storage::new` already takes ten `&mut` slices in a fixed order,
/// which is ten chances to swap two of the same type; six more would be
/// sixteen, and four of these six are `&mut [u8]`. Grouping them means a caller
/// names each one and the compiler checks the name.
///
/// They belong together for a second reason: they are the *result* of a control
/// string, and `pending` is the string itself. A title that outlived its
/// terminator, or a link table lent from one place while the payload it was
/// parsed from was lent from another, is the borrow this shape makes
/// unspellable.
pub struct StrBuf<'b> {
    /// The body being accumulated, reset at the **introducer**.
    ///
    /// Not at the terminator, which is the difference that matters: a string
    /// spans reads, so this outlives a `feed`, and a cancelled string never
    /// reaches a terminator at all. Resetting where the string *starts* is what
    /// makes both cases right, and it is the same failure this repo records for
    /// `pre: Option<Classified>` -- a value carried between two functions,
    /// consumed by a later and different one because the path has more exits
    /// than it looks like it has.
    pub(crate) pending: &'b mut [u8],
    /// `OSC 2`, and `OSC 0` sets this and `icon` together.
    pub(crate) title: &'b mut [u8],
    /// `OSC 1`.
    pub(crate) icon: &'b mut [u8],
    /// Palette overrides from `OSC 4` in the first 256 entries, and the
    /// embedder's own table in the second 256 ([`PALETTE_LEN`]).
    /// `ColorWord::DEFAULT` means "not overridden" in the first half and "not
    /// told" in the second, so a zeroed buffer starts on the renderer's own
    /// palette with no separate validity array to keep in step. The second
    /// half is a copy of the renderer's table, kept only to answer a query:
    /// nothing resolves a rendition through it, so writing it moves no colour
    /// epoch.
    pub(crate) palette: &'b mut [ColorWord],
    /// Hyperlink URIs, `NLINK` of them flat at a stride of `LINK_MAX`.
    ///
    /// Flat rather than `[[u8; LINK_MAX]; NLINK]` because the storage is
    /// caller-provided and a slice cannot carry the inner dimension. The stride
    /// is asserted in `Storage::new` rather than assumed -- a two-dimensional
    /// array flattened into a slice is exactly where a silent stride bug lives.
    pub(crate) links: &'b mut [u8],
    pub(crate) link_len: &'b mut [ByteLen],
    /// `OSC 52` writes: two areas of [`ClipboardBudget::text`] octets, the
    /// copy being collected in the first half and the last one finished in
    /// the second (`clip.rs`). **Empty by default**, the image arena's
    /// opt-in: a storage with no area refuses every write and counts it.
    ///
    /// Here rather than on `Paint` because `Paint` is inlined into the
    /// per-byte loop and carries this struct by reference already: a slice
    /// there is two more words through the loop, the class this repo records
    /// at 3.5-10% (note V, V11 design 1).
    pub(crate) clip_area: &'b mut [u8],
    /// The `OSC 52` string in flight, what was last published, and the
    /// counts (`clip.rs`): here rather than in `Cold`, whose size is pinned
    /// and which the per-glyph write dereferences.
    pub(crate) clip: crate::clip::Clip,
    /// The Sixel in flight, decoded as its body arrives into the image
    /// arena (`SixelStream`): its colour registers, position and claimed
    /// gap. **No body is kept**, which until 2026-10-14 a scratch of
    /// `[images] body_kb` bytes held whole before it decoded, refusing a
    /// longer one. Behind a reference, here for the reason `clip_area` is:
    /// `Paint` carries this struct by one word and only a Sixel reaches it.
    /// Always provisioned, because it is the decoder's state and not image
    /// memory: a terminal with no arena still walks a Sixel and refuses it
    /// for its size against an empty arena.
    pub(crate) sixel: &'b mut SixelStream,
    /// The kitty graphics stream (`kitty.rs`): the chunk being read, the
    /// transmission in flight, the decode deferred and the replies owed.
    /// Behind a reference for the reason `sixel` is, and always provisioned:
    /// it is the protocol's state, and a terminal with no arena still reads
    /// a command and refuses it.
    pub(crate) kitty: &'b mut KittyStream,
    /// Where a kitty PNG waits whole for its decode, its scanlines after it:
    /// [`ImageBudget::scratch`] bytes. Empty by default, the arena's opt-in,
    /// so every PNG is refused for its size until [`Owned::with_images`].
    pub(crate) kitty_scratch: &'b mut [u8],
    /// XTWINOPS's title stack (`CSI 22 t`, `CSI 23 t`): [`TITLE_SLOTS`]
    /// slots of an icon name and a title, [`OSC_MAX`] bytes each,
    /// [`TITLE_STACK`] in all. Here for the reason `clip_area` is: only a
    /// push and a pop reach it.
    pub(crate) titles: &'b mut [u8],
    /// Which halves of which slots hold a title, and their lengths
    /// (`crate::window`).
    pub(crate) title_stack: crate::window::TitleStack,
    /// What the embedder has said about its window and screen
    /// (`Term::set_window`), read by XTWINOPS's reports alone.
    pub(crate) window: baud_domain::WindowFacts,
    /// `CSI 20 t` and `CSI 21 t` refused, and XTSMTITLE's two query modes
    /// with them (`Term::title_reports_refused`).
    pub(crate) title_reports_refused: crate::term::TitleReportsRefused,
    /// XTSMTITLE's set modes (`crate::window`), read where an `OSC 0`, `1`
    /// or `2` is: here for the reason `window` is.
    pub(crate) title_modes: TitleModes,
    /// The encoding the title's and the icon name's bytes are in
    /// (`Term::title_encoding`), decided where each is set.
    pub(crate) title_enc: InputEncoding,
    pub(crate) icon_enc: InputEncoding,
    /// XTPUSHSGR's stack (`crate::stack`): read by a push and a pop alone,
    /// so here rather than in `Cold`, whose size is pinned.
    pub(crate) renditions: crate::stack::RenditionStack,
    /// Which of XTPUSHCOLORS's ten palettes hold one and the stack's
    /// current entry; the palettes are in `palette`.
    pub(crate) colors: crate::stack::ColorStack,
    /// The window operations queued for the embedder, whether they are on
    /// (`Term::set_window_ops`) and the counts of those that did not go
    /// (`Term::window_requests`). Here for the reason `window` is: only an
    /// XTWINOPS operation reaches it.
    pub(crate) window_asks: crate::window::WindowAsks,
    /// The special-colour modes `OSC 6` and `OSC 106` turn on
    /// (`Term::color_modes`), the colours being in `palette`: here rather
    /// than in `Cold`, whose size is pinned, as `clip` is; written by those
    /// two and `RIS`, read by the renderer once a rendition is resolved.
    pub(crate) color_modes: ColorModes,
    /// What the `OSC 133` marks said of the live screen, and of the one
    /// parked (`prompt.rs`): swapped by `?1049`. Here for the reason
    /// `clip_area` is: only a mark and a resize reach them.
    pub(crate) shell: crate::prompt::Shell,
    pub(crate) shell_other: crate::prompt::Shell,
    /// The primary's prompts, where each began and how its command ended
    /// (`baud_domain::PromptLog`): [`Owned`]'s, one entry a ring row, the
    /// most a ring holds, so the log never fills; none through
    /// [`StrBuf::new`], under which every prompt is forgotten as it is kept,
    /// and counted.
    pub(crate) prompts: PromptLog<'b>,
    /// **The rows a re-cut dropped from the oldest end of the history**,
    /// summed over every resize ([`crate::Term::reflow_dropped`]). Here
    /// rather than in `Book`, which the per-byte path carries and whose
    /// size is pinned, because only a resize writes it.
    pub(crate) reflow_dropped: Rows,
}

impl<'b> StrBuf<'b> {
    /// Lend the eleven, in the one order that is correct.
    ///
    /// A constructor rather than public fields, because `Storage::new` is
    /// public and takes a `StrBuf`: without this an embedder outside the crate
    /// could name the type and not build one, which is an entry point whose
    /// argument is unreachable -- the same shape as a mode recorded and never
    /// read. Eight, past clippy's seven, for the reason `Storage::new` takes
    /// more: each is a buffer the caller owns, and grouping them would only
    /// move the list.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pending: &'b mut [u8],
        title: &'b mut [u8],
        icon: &'b mut [u8],
        palette: &'b mut [ColorWord],
        links: &'b mut [u8],
        link_len: &'b mut [ByteLen],
        clip: &'b mut [u8],
        sixel: &'b mut SixelStream,
        kitty: &'b mut KittyStream,
        kitty_scratch: &'b mut [u8],
        titles: &'b mut [u8],
    ) -> StrBuf<'b> {
        StrBuf {
            pending,
            title,
            icon,
            palette,
            links,
            link_len,
            clip_area: clip,
            clip: crate::clip::Clip::NEW,
            sixel,
            kitty,
            kitty_scratch,
            titles,
            title_stack: crate::window::TitleStack::EMPTY,
            window: baud_domain::WindowFacts::UNKNOWN,
            title_reports_refused: crate::term::TitleReportsRefused::FIRST,
            title_modes: TitleModes::DEFAULT,
            title_enc: InputEncoding::Utf8,
            icon_enc: InputEncoding::Utf8,
            renditions: crate::stack::RenditionStack::EMPTY,
            colors: crate::stack::ColorStack::EMPTY,
            window_asks: crate::window::WindowAsks::NONE,
            color_modes: ColorModes::NONE,
            shell: crate::prompt::Shell::FRESH,
            shell_other: crate::prompt::Shell::FRESH,
            prompts: PromptLog::new(&mut []),
            reflow_dropped: Rows::ZERO,
        }
    }
}

/// The sixteen allocations a `Storage` borrows, owned in one place.
///
/// **A convenience for tests and embedders with an allocator, and nothing the
/// core itself uses.** Every test file that drives this crate otherwise
/// repeats the same twenty-five lines of `vec!` and hands ten `&mut`s to
/// `Storage::new` in the right order -- which is ten chances to swap two
/// arguments of the same type, and the reason the property suite could not be
/// written cheaply before this existed.
///
/// Behind `alloc`, so the default build is still allocation-free and the
/// `no_std` target still compiles with nothing but `core`.
#[cfg(feature = "alloc")]
pub struct Owned {
    stride: Stride,
    depth: Depth,
    arena: alloc::vec::Vec<u8>,
    vp: alloc::vec::Vec<PackedCell>,
    used: alloc::vec::Vec<RowExtent>,
    past: alloc::vec::Vec<PastEdge>,
    cells3: alloc::vec::Vec<PackedCell>,
    tmpl: alloc::vec::Vec<Option<StyleWord>>,
    order: alloc::vec::Vec<StyleId>,
    book: StyleBook,
    tabs: alloc::vec::Vec<bool>,
    reply: alloc::vec::Vec<u8>,
    alt: alloc::vec::Vec<PackedCell>,
    alt_used: alloc::vec::Vec<RowExtent>,
    clusters: alloc::vec::Vec<ClusterSlot>,
    pending: alloc::vec::Vec<u8>,
    title: alloc::vec::Vec<u8>,
    icon: alloc::vec::Vec<u8>,
    titles: alloc::vec::Vec<u8>,
    palette: alloc::vec::Vec<ColorWord>,
    links: alloc::vec::Vec<u8>,
    link_len: alloc::vec::Vec<ByteLen>,
    img_pixels: alloc::vec::Vec<Pixel>,
    img_probe: alloc::vec::Vec<Pixel>,
    img_slots: alloc::vec::Vec<ImageSlot>,
    img_places: alloc::vec::Vec<PlacementSlot>,
    img_index: alloc::vec::Vec<PlacementId>,
    sixel: alloc::boxed::Box<SixelStream>,
    kitty: alloc::boxed::Box<KittyStream>,
    kitty_scratch: alloc::vec::Vec<u8>,
    clip: alloc::vec::Vec<u8>,
    prompts: alloc::vec::Vec<PromptEntry>,
}

#[cfg(feature = "alloc")]
impl Owned {
    /// Sized to `ws`, with the three-byte cell table **absent**.
    ///
    /// An empty `cells3` is how a caller declines it, and declining is the
    /// default: its only reader is behind `cell-table`, and passing the full
    /// 2 MiB otherwise costs 262,144 property-table lookups and 2 MiB of
    /// stores at construction for a table no read will consult.
    pub fn new(ws: &WinSize) -> Owned {
        Owned::with_geometry(ws, &CacheGeometry::UNKNOWN)
    }

    /// Sized to `ws`, at the pitch [`grid_stride`] derives from `geom`.
    ///
    /// [`Owned::new`] passes [`CacheGeometry::UNKNOWN`], which is safe: an
    /// odd number of the smallest line any cache uses, so it aliases no way
    /// on any machine, at the step the search chose for that line. An app
    /// hands in what its host detected -- `baud-isa`'s per-arch crates -- so
    /// the pitch is the one searched for *this* machine's line.
    pub fn with_geometry(ws: &WinSize, geom: &CacheGeometry) -> Owned {
        Owned::with_history(ws, geom, Depth::DEFAULT)
    }

    /// Sized to `ws` at the pitch `geom` and `ws`'s width choose, with
    /// `depth` lines of scrollback behind the screen: [`Depth::NONE`] for a
    /// terminal that keeps none. The width is provisioned as well as the
    /// height -- a resize past it answers `Truncated` -- because every line
    /// of scrollback costs a row of the pitch.
    pub fn with_history(ws: &WinSize, geom: &CacheGeometry, depth: Depth) -> Owned {
        use alloc::vec;
        let (cols, rows) = ws.geometry();
        let stride = grid_stride(geom, cols);
        let span = RowBlock::leading(rows, stride);
        let ring = RowBlock::leading(Rows::of_index(rows.index() + depth.lines()), stride);
        // Sized to the whole primary screen (`span` is `rows * stride`), so
        // a text-drawn image preview -- one distinct rendition per cell --
        // never evicts a rendition a visible cell still references. A resize
        // clamps `cols` to `stride` and `rows` to the provisioned capacity,
        // so the largest screen this storage can display is `span.len()`
        // cells, and the table holds it. The next power of two at or above
        // it (so open addressing's `& (len - 1)` wraps), between the NSTYLE
        // floor and the NSTYLE_MAX ceiling; the ceiling clamp precedes
        // `next_power_of_two()`, which panics past 2^63.
        let styles = span.len().min(NSTYLE_MAX).next_power_of_two().max(NSTYLE);
        // A placement for every cell of the largest screen this storage
        // displays, `span.len()` as the rendition table's is, so a picture
        // placed one cell at a time fits however large it is drawn.
        let places = span.len().max(NPLACEMENT);
        Owned {
            stride,
            depth,
            arena: vec![0u8; ARENA],
            vp: vec![PackedCell::EMPTY; ring.len()],
            used: vec![RowExtent::ZERO; rows.index() + depth.lines()],
            past: vec![PastEdge::Clear; rows.index() + depth.lines()],
            cells3: vec::Vec::new(),
            tmpl: vec![None; styles],
            order: vec![StyleId::DEFAULT; styles],
            book: StyleBook::new(),
            tabs: vec![false; MAX_COLS],
            reply: vec![0u8; REPLY],
            alt: vec![PackedCell::EMPTY; span.len()],
            alt_used: vec![RowExtent::ZERO; rows.index()],
            clusters: vec![ClusterSlot::EMPTY; NCLUSTER],
            pending: vec![0u8; OSC_MAX],
            title: vec![0u8; OSC_MAX],
            icon: vec![0u8; OSC_MAX],
            titles: vec![0u8; TITLE_STACK],
            palette: vec![ColorWord::DEFAULT; PALETTE_LEN],
            links: vec![0u8; NLINK * LINK_MAX],
            link_len: vec![ByteLen::ZERO; NLINK],
            // The slot table is tiny and always allocated, and so is the
            // Sixel decoder's state, its 256 registers the most of it; the
            // placement table and its index come from zeroed pages,
            // resident only where placements land; the pixel arena is the
            // large buffer and defaults absent, provisioned by
            // [`Owned::with_images`].
            img_pixels: vec::Vec::new(),
            img_probe: vec::Vec::new(),
            img_slots: vec![ImageSlot::EMPTY; NIMAGE],
            img_places: PlacementSlot::zeroed_vec(places),
            img_index: PlacementId::zeroed_index(places),
            sixel: alloc::boxed::Box::new(SixelStream::NEW),
            kitty: alloc::boxed::Box::new(KittyStream::NEW),
            kitty_scratch: vec::Vec::new(),
            // The `OSC 52` areas default absent, provisioned by
            // [`Owned::with_clipboard`].
            clip: vec::Vec::new(),
            // One prompt a ring row: a log that never fills.
            prompts: vec![PromptEntry::EMPTY; rows.index() + depth.lines()],
        }
    }

    /// Provision image storage: `pixels` [`Pixel`]s of arena.
    /// [`Owned::new`] provisions none -- the `with_cell_table` opt-in shape --
    /// so a terminal draws no images until a caller that wants them sizes it.
    ///
    /// **The size is the owner's call, between one image and a screenful.** A
    /// 64x48 image is 3072 `Pixel`s (12 KiB); a full 200x50 screen at an
    /// 8x18 cell is ~1.4M (5.6 MiB). It is the only limit on a Sixel's size
    /// besides the 65535 a coordinate holds and the slot table: a body is
    /// decoded as it arrives, and no scratch holds it.
    ///
    /// **A zero-length arena is valid and refuses every image**, which the
    /// gate depends on -- do not add a non-empty assert here. Refuse-and-count
    /// is the contract: `store` returns `None` and increments `images_refused`
    /// and `refusals.arena` when no gap holds the pixels, and an empty arena is
    /// that with no gap at all, which is the correct nothing for a caller that
    /// wants images recognised and dropped rather than drawn.
    ///
    /// **Nothing is written here.** The arena, the kitty query probe and the
    /// PNG scratch come from zeroed memory ([`Pixel::zeroed_vec`],
    /// `vec![0u8; n]`), so a page is committed when an image first writes
    /// it and an idle terminal pays for none of it. Once the image is
    /// freed, or the PNG decoded, its whole pages go back to the system
    /// through the release an embedder lends ([`Term::set_page_release`]);
    /// with none lent they stay committed until an image writes them
    /// again.
    pub fn with_images(mut self, budget: ImageBudget) -> Owned {
        self.img_pixels = Pixel::zeroed_vec(budget.pixels);
        self.img_probe = Pixel::zeroed_vec(budget.probe());
        self.kitty_scratch = alloc::vec![0u8; budget.scratch()];
        self
    }

    /// A prompt log of `entries`, in place of one a ring row: a smaller
    /// log forgets its oldest prompts, counted ([`Term::prompts_forgotten`]).
    pub fn with_prompt_log(mut self, entries: PromptCapacity) -> Owned {
        self.prompts = alloc::vec![PromptEntry::EMPTY; entries.index()];
        self
    }

    /// Provision `OSC 52` writes: two areas of `budget`'s text, the copy
    /// being collected and the last one finished. [`Owned::new`] provisions
    /// none, the [`Owned::with_images`] shape, and a terminal without them
    /// refuses every write and counts it in `clipboard_writes_refused`.
    pub fn with_clipboard(mut self, budget: ClipboardBudget) -> Owned {
        self.clip = alloc::vec![0u8; budget.buffer()];
        self
    }

    /// Sized to `ws` with **neither surface of its own**, for an embedder
    /// that places both where a GPU can read them in place and lends them
    /// through [`Owned::storage_over`]: the primary ring at [`Owned::ring`]
    /// rows and the alternate screen at [`Owned::rows`], both at
    /// [`Owned::stride`]. Everything else is allocated here.
    pub fn without_surfaces(ws: &WinSize, geom: &CacheGeometry, depth: Depth) -> Owned {
        let mut owned = Owned::with_history(ws, geom, depth);
        owned.vp = alloc::vec::Vec::new();
        owned.alt = alloc::vec::Vec::new();
        owned
    }

    /// The pitch this storage lays its grids out at.
    pub fn stride(&self) -> Stride {
        self.stride
    }

    /// Physical rows in the primary ring: the screen's capacity plus the
    /// scrollback.
    pub fn ring(&self) -> Rows {
        Rows::of_index(self.used.len())
    }

    /// The screen's capacity in rows, which is the alternate screen's ring.
    pub fn rows(&self) -> Rows {
        Rows::of_index(self.used.len() - self.depth.lines())
    }

    /// Lend them all to a `Storage` with `vp` and `alt` standing in for the
    /// two surfaces, in memory the caller chose -- shared GPU buffers, so the
    /// grid the core writes is the grid the GPU reads.
    pub fn storage_over<'v>(
        &'v mut self,
        vp: &'v mut [PackedCell],
        alt: &'v mut [PackedCell],
    ) -> Storage<'v> {
        Storage::new(
            &mut self.arena,
            self.stride,
            self.depth,
            vp,
            &mut self.used,
            &mut self.past,
            &mut self.cells3,
            StyleTable {
                slots: &mut self.tmpl,
                order: &mut self.order,
                book: &mut self.book,
            },
            &mut self.tabs,
            &mut self.reply,
            alt,
            &mut self.alt_used,
            &mut self.clusters,
            StrBuf {
                pending: &mut self.pending,
                title: &mut self.title,
                icon: &mut self.icon,
                palette: &mut self.palette,
                links: &mut self.links,
                link_len: &mut self.link_len,
                clip_area: &mut self.clip,
                clip: crate::clip::Clip::NEW,
                sixel: &mut self.sixel,
                kitty: &mut self.kitty,
                kitty_scratch: &mut self.kitty_scratch,
                titles: &mut self.titles,
                title_stack: crate::window::TitleStack::EMPTY,
                window: baud_domain::WindowFacts::UNKNOWN,
                title_reports_refused: crate::term::TitleReportsRefused::FIRST,
                title_modes: TitleModes::DEFAULT,
                title_enc: InputEncoding::Utf8,
                icon_enc: InputEncoding::Utf8,
                renditions: crate::stack::RenditionStack::EMPTY,
                colors: crate::stack::ColorStack::EMPTY,
                window_asks: crate::window::WindowAsks::NONE,
                color_modes: ColorModes::NONE,
                shell: crate::prompt::Shell::FRESH,
                shell_other: crate::prompt::Shell::FRESH,
                prompts: PromptLog::new(&mut self.prompts),
                reflow_dropped: Rows::ZERO,
            },
            ImageStore::new(
                &mut self.img_pixels,
                &mut self.img_slots,
                &mut self.img_places,
                &mut self.img_index,
            )
            .with_probe(&mut self.img_probe),
        )
    }

    /// Lend them all to a `Storage`.
    pub fn storage(&mut self) -> Storage<'_> {
        Storage::new(
            &mut self.arena,
            self.stride,
            self.depth,
            &mut self.vp,
            &mut self.used,
            &mut self.past,
            &mut self.cells3,
            StyleTable {
                slots: &mut self.tmpl,
                order: &mut self.order,
                book: &mut self.book,
            },
            &mut self.tabs,
            &mut self.reply,
            &mut self.alt,
            &mut self.alt_used,
            &mut self.clusters,
            StrBuf {
                pending: &mut self.pending,
                title: &mut self.title,
                icon: &mut self.icon,
                palette: &mut self.palette,
                links: &mut self.links,
                link_len: &mut self.link_len,
                clip_area: &mut self.clip,
                clip: crate::clip::Clip::NEW,
                sixel: &mut self.sixel,
                kitty: &mut self.kitty,
                kitty_scratch: &mut self.kitty_scratch,
                titles: &mut self.titles,
                title_stack: crate::window::TitleStack::EMPTY,
                window: baud_domain::WindowFacts::UNKNOWN,
                title_reports_refused: crate::term::TitleReportsRefused::FIRST,
                title_modes: TitleModes::DEFAULT,
                title_enc: InputEncoding::Utf8,
                icon_enc: InputEncoding::Utf8,
                renditions: crate::stack::RenditionStack::EMPTY,
                colors: crate::stack::ColorStack::EMPTY,
                window_asks: crate::window::WindowAsks::NONE,
                color_modes: ColorModes::NONE,
                shell: crate::prompt::Shell::FRESH,
                shell_other: crate::prompt::Shell::FRESH,
                prompts: PromptLog::new(&mut self.prompts),
                reflow_dropped: Rows::ZERO,
            },
            ImageStore::new(
                &mut self.img_pixels,
                &mut self.img_slots,
                &mut self.img_places,
                &mut self.img_index,
            )
            .with_probe(&mut self.img_probe),
        )
    }
}

#[cfg(feature = "alloc")]
impl Owned {
    /// Sized to `ws`, with the three-byte cell table **present**.
    ///
    /// The one caller that wants it is the table's own equivalence test, which
    /// builds it *without* `cell-table` and reads the slice directly -- so the
    /// write path is the computing one and the table is the thing under test.
    /// `Owned::new` declines it because nothing else reads it: the only read is
    /// in `put_one` behind the feature, and passing 2 MiB otherwise buys
    /// 262,144 property-table lookups and 2 MiB of stores at construction for
    /// a table no read will consult.
    pub fn with_cell_table(ws: &WinSize) -> Owned {
        let mut owned = Owned::new(ws);
        owned.cells3 = alloc::vec![PackedCell::EMPTY; CELLS3];
        owned
    }

    /// The table as `Term::new` built it.
    ///
    /// Borrowed after the `Term` is dropped, which is what lets a test compare
    /// the table against the cells the write path produced from the same bytes.
    pub fn cell_table(&self) -> &[PackedCell] {
        &self.cells3
    }
}

/// How many distinct grapheme clusters can be live at once.
///
/// **1024, the owner's choice of 2026-10-13** ("1024 slots, 8 probes"). The
/// index rides in the cell's `Content`, above U+10FFFF, where it was six
/// bits of `CellFlags` and the store held 63. The 1024 slots are 40,960
/// bytes, and an intern reads at most eight of them
/// (`ClusterStore::PROBES`). A slot no cell names is reclaimed by a sweep
/// once every slot holds a cluster, so a session can pass more clusters
/// than the store holds. It is not a bound on what is on a cell at once:
/// a cluster whose eight slots are taken is refused while others are free.
/// `ClusterStore` refuses and counts, so the answer is never wrong and how
/// often it is missing is a number.
pub const NCLUSTER: usize = ClusterStore::SLOTS_MAX;

/// The **floor** on the rendition table: the smallest it is ever sized to, for
/// a screen too small to need more. The table is sized to the screen by
/// [`style_slots`] -- a text-drawn image preview mints one distinct truecolour
/// rendition per cell, so a table smaller than the screen would evict a
/// rendition an on-screen cell still points at, and that cell would resolve to
/// another's colour. Below capacity the mapping is exact, so a table that holds
/// one full screen never scrambles a preview; the floor keeps a small screen
/// from paying for slots it cannot fill.
pub const NSTYLE: usize = 1024;

/// The **ceiling**, from the cell rather than from a sweep: a [`StyleId`] is a
/// `u16`, so a slot index must fit sixteen bits and the table cannot exceed
/// `2^16` slots however large the screen. A screen with more than this many
/// distinct renditions -- 65,536 cells, a window no real terminal reaches --
/// can still evict, which is recorded rather than paid for. At 20 bytes a
/// `StyleWord` the ceiling is a 1.5 MB table, so only a window that large pays
/// it; a 200x50 screen sizes to 16,384 slots, ~384 KB.
pub const NSTYLE_MAX: usize = 1 << 16;

/// How many `(lead, continuation)` prefixes a three-byte character has.
pub const CELLS3_PREFIXES: usize = 16 * 64;
/// Entries in the three-byte cell table.
pub const CELLS3: usize = CELLS3_PREFIXES * 256;

/// The longest control-string body kept, and the longest title.
///
/// **A truncation is counted, not silent.** A title past this is clipped and
/// `Term::string_truncations` says how often -- the same contract every
/// bounded table here carries. `baud-core2` used the same 512.
pub const OSC_MAX: usize = 512;

/// The title stack's depth: xterm's `MAX_SAVED_TITLES` (`[[source:
/// xterm-ptyx-h]]`), and ctlseqs' direct-access range, "1 through 10".
pub const TITLE_SLOTS: usize = 10;

/// The bytes the title stack takes: [`TITLE_SLOTS`] slots of an icon name
/// and a title, [`OSC_MAX`] each -- 10,240.
pub const TITLE_STACK: usize = TITLE_SLOTS * 2 * OSC_MAX;

/// Entries in the palette slice: the first 256 are the application's `OSC 4`
/// overrides, the second 256 the embedder's own table
/// ([`crate::Term::set_default_palette`]), which an `OSC 4 ; c ; ?` for an
/// entry nothing overrode is answered from, the next five xterm's special
/// colours as `OSC 5` sets them ([`crate::Term::special_color`]), and after
/// them XTPUSHCOLORS's ten palettes, [`COLOR_SLOT`] entries each
/// (`crate::stack`): 3,177 in all, 12,708 bytes.
pub const PALETTE_LEN: usize = COLOR_STACK_PALETTE + SGR_STACK_DEPTH * COLOR_SLOT;

/// Where the special colours end and the colour stack begins.
pub(crate) const COLOR_STACK_PALETTE: usize = SPECIAL_PALETTE + SpecialColor::ALL.len();

/// One palette on the colour stack: what a pop restores, the 256 `OSC 4`
/// overrides, the five special colours and the five dynamic ones -- the
/// "dynamic- and ANSI-palette colors" of `[[source: ctlseqs]]`, as xterm's
/// `ColorSlot` holds its `MAXCOLORS` and its dynamic colours
/// (`[[source: xterm-misc]]`).
pub(crate) const COLOR_SLOT: usize =
    EMBEDDER_PALETTE + SpecialColor::ALL.len() + DynamicColor::COUNT;

/// Where the embedder's half of the palette slice starts.
pub(crate) const EMBEDDER_PALETTE: usize = 256;

/// Where the special colours start in the palette slice.
pub(crate) const SPECIAL_PALETTE: usize = 512;

/// Distinct `OSC 8` hyperlinks live at once.
///
/// **128 because the cell says so.** The link index rides in `CellFlags`,
/// which has seven bits for it, and seven bits is 128 -- 127 usable, since
/// zero is "no link". Bounded because a URI is attacker-supplied content used
/// as a key, and a growable table keyed on that is the content-addressing
/// failure mode this repo already records. **It refuses on collision rather
/// than evicting**, unlike the style table: an evicted style is a wrong colour
/// on the next frame, an evicted link is a wrong *destination* under a cell
/// whose text did not change -- and a later `OSC 8` run that writes no cell
/// at all could have retargeted it. A refused link is counted
/// (`Term::links_refused`) and its cells are plain text.
pub const NLINK: usize = 128;

/// The longest URI kept, and the stride of the flat `links` slice.
pub const LINK_MAX: usize = 256;

/// How many bytes of reply the core will hold before it starts counting
/// losses: **the longest single reply**, which is DECTABSR with a stop at
/// every one of `MAX_COLS` columns, 9,139 bytes (`tab_report_max`), since
/// 2026-10-19. It was 256, which bounded the longest before it, a `DECRQSS`
/// rendition string of at most `DECRQSS_MAX` bytes, const-asserted against
/// this in `dcs.rs`; a DECTABSR at 2048 columns with the default stops is
/// 1,144 bytes, so 256 would have dropped it whole. Cold: written by a
/// report, read by the embedder. Several replies from one read can still
/// exceed it (note AG).
pub const REPLY: usize = match tab_report_max(MAX_COLS) > 256 {
    true => tab_report_max(MAX_COLS),
    false => 256,
};

/// How many images can be live at once. Small: a screen shows a handful, the
/// table is scanned linearly, and it is tiny (a slot is a span and a refcount),
/// so it is always allocated -- unlike the pixel arena, which is the large
/// buffer and defaults absent. The store works with any slot count, so this is
/// [`Owned`]'s provisioning size rather than an invariant, hence `alloc`-gated.
#[cfg(feature = "alloc")]
pub const NIMAGE: usize = 32;

/// **The fewest placements [`Owned`] provisions**; the table is sized to
/// the screen, a placement for every cell of the largest screen the
/// storage displays, and this is its floor for a screen under 64 cells.
///
/// The text sets no limit on placements -- "there can be many placements
/// per image", and its only quota is on image data
/// (`[[source: kitty-graphics]]`) -- and a client may place a picture one
/// cell at a time: yazi 26.9.1 sends an `a=p` per cell of its preview, 660
/// for a 44 by 15 one, and the 64-slot table this replaced placed one row
/// and twenty cells of it (Jev, SCREEN_CELLS, 0.80 and 0.80 then 0.71 and
/// 0.80 after research, under the bar and taken as the lean). The table
/// comes from zeroed memory, and every walk of it stops at its highest live
/// slot, so an idle terminal pays nothing for its size.
#[cfg(feature = "alloc")]
pub const NPLACEMENT: usize = 64;

/// **The rendition table's bookkeeping, in the caller's memory beside the
/// table**: which table it is, how many writes it has taken, and which colour
/// epoch it is in. Caller memory, because the table is: a `Term` lent the same
/// storage again finds the slots it left, and a count that restarted would say
/// they were never written.
#[derive(Debug)]
pub struct StyleBook {
    pub(crate) table: TableSerial,
    pub(crate) writes: StyleWrites,
    pub(crate) colors: ColorEpoch,
    /// The sweep's book: how full the table is and what it has reclaimed.
    pub(crate) sweep: baud_domain::SweepBook,
}

impl StyleBook {
    /// A new table's book: its own serial, and nothing written.
    pub fn new() -> StyleBook {
        StyleBook {
            table: TableSerial::mint(),
            writes: StyleWrites::NEVER,
            colors: ColorEpoch::default(),
            sweep: baud_domain::SweepBook::default(),
        }
    }
}

impl Default for StyleBook {
    fn default() -> StyleBook {
        StyleBook::new()
    }
}

/// **The rendition table and its write log**, lent to [`Storage::new`] as one
/// argument so the three cannot be lent apart.
///
/// The log is a ring of `slots.len()` entries, one a write, naming the slot
/// each write filled, evicted or freed. Until the table sweeps or evicts it
/// fills each slot once, so until it has taken as many writes as it has
/// slots the log holds exactly the filled slots, in the order they were
/// filled. A sweep logs every slot it frees, so a reader that replays the
/// log holds the table still. A renderer resolves each slot once and then
/// only the slots written since (`Term::styles_written`), where it used to
/// resolve all of them every frame: at the app's 65,536 slots that was 159 us
/// of a 190 us idle frame.
pub struct StyleTable<'b> {
    pub slots: &'b mut [Option<StyleWord>],
    /// One entry a slot.
    pub order: &'b mut [StyleId],
    pub book: &'b mut StyleBook,
}

/// The log half of a [`StyleTable`], which the write path carries.
pub(crate) struct StyleLog<'b> {
    pub(crate) order: &'b mut [StyleId],
    pub(crate) book: &'b mut StyleBook,
}

impl StyleLog<'_> {
    /// Records a write to slot `k`.
    pub(crate) fn note(&mut self, k: StyleId) {
        self.book.writes = self.book.writes.next();
        let at = self.book.writes.index_in(self.order.len());
        self.order[at] = k;
    }

    /// A colour a rendition resolves through changed.
    pub(crate) fn colors_moved(&mut self) {
        self.book.colors = self.book.colors.next();
    }
}

impl<'b> Storage<'b> {
    /// How many rows the caller provisioned, which is not the live row count.
    ///
    /// **The number is already in the struct**, so this is a read rather than
    /// a field: `used` has one entry per physical row, and `Storage::new`
    /// asserts `vp` holds that many rows' worth of cells. It matters because
    /// the storage is caller-provided: a `Term` can be resized *down* freely
    /// and can only grow into rows somebody allocated. `Owned::new` sizes to
    /// the geometry it is given, so growing past it is `Resized::Truncated`
    /// rather than a silent clamp -- an embedder that wants headroom
    /// provisions for it, and this is the number it provisions.
    pub fn row_capacity(&self) -> Rows {
        Rows::of_index(self.used.len() - self.depth.lines())
    }

    /// Physical rows in the primary ring: the screen's capacity plus the
    /// scrollback.
    pub fn ring_rows(&self) -> Rows {
        Rows::of_index(self.used.len())
    }

    /// # Panics
    /// If any slice is shorter than the geometry needs. Checked once, here,
    /// rather than on every access -- the parser addresses this memory
    /// millions of times a second and cannot re-establish the invariant.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        arena: &'b mut [u8],
        stride: Stride,
        depth: Depth,
        vp: &'b mut [PackedCell],
        used: &'b mut [RowExtent],
        past: &'b mut [PastEdge],
        cells3: &'b mut [PackedCell],
        styles: StyleTable<'b>,
        tabs: &'b mut [bool],
        reply: &'b mut [u8],
        alt: &'b mut [PackedCell],
        alt_used: &'b mut [RowExtent],
        clusters: &'b mut [ClusterSlot],
        strs: StrBuf<'b>,
        images: ImageStore<'b>,
    ) -> Storage<'b> {
        assert!(arena.len() >= READ_CHUNK, "arena is smaller than one read");
        // **The stride is checked, not assumed.** `links` is `NLINK` URIs flat
        // at `LINK_MAX` each, because caller-provided storage cannot carry the
        // inner dimension of a two-dimensional array. A slice one stride short
        // does not fail: it returns a plausible URI belonging to the next slot.
        assert!(
            strs.links.len() >= NLINK * LINK_MAX,
            "links is {} bytes, needs {} -- {NLINK} URIs at a stride of {LINK_MAX}",
            strs.links.len(),
            NLINK * LINK_MAX
        );
        assert!(
            strs.link_len.len() >= NLINK,
            "link_len is shorter than NLINK"
        );
        assert!(
            strs.pending.len() >= OSC_MAX,
            "pending is shorter than OSC_MAX"
        );
        assert!(strs.title.len() >= OSC_MAX, "title is shorter than OSC_MAX");
        assert!(strs.icon.len() >= OSC_MAX, "icon is shorter than OSC_MAX");
        assert!(
            strs.titles.len() >= TITLE_STACK,
            "titles is shorter than TITLE_STACK"
        );
        assert!(
            strs.palette.len() >= PALETTE_LEN,
            "palette is not PALETTE_LEN entries"
        );
        // `RowBlock` computes the span so the two operands cannot be the
        // wrong way round -- a transposition here is a plausible number and a
        // silently wrong slice.
        assert!(
            used.len() > depth.lines(),
            "the ring holds no screen: {} rows against {} of scrollback",
            used.len(),
            depth.lines()
        );
        let ring = RowBlock::leading(Rows::of_index(used.len()), stride);
        assert!(ring.fits(vp), "the ring is short of its rows");
        // A row's mark shares its word with the wrap and the line rendition,
        // so the pitch must fit what is left of it.
        assert!(
            stride.index() <= RowExtent::MAX_MARK,
            "a pitch of {} cells is past the {} a row's mark holds",
            stride.index(),
            RowExtent::MAX_MARK
        );
        assert!(
            past.len() >= used.len(),
            "the past-edge flags are one per row of the ring"
        );
        let screen = RowBlock::leading(Rows::of_index(used.len() - depth.lines()), stride);
        // **Required, not optional.** An absent buffer would mean the
        // alternate screen is recognised, entered, and then quietly destroys
        // the primary on the way out -- which `docs/vt-coverage.md` rates as
        // worse than not implementing it, because the application is given no
        // way to find out.
        assert!(
            screen.fits(alt),
            "the alternate screen is short of its rows"
        );
        assert!(
            alt_used.len() >= used.len() - depth.lines(),
            "the alternate screen's extents are one per row"
        );
        assert!(
            cells3.len() == CELLS3 || cells3.is_empty(),
            "the three-byte cell table is 1024 x 256 entries, or absent"
        );
        let tmpl = &styles.slots;
        assert!(
            tmpl.len().is_power_of_two() && tmpl.len() >= NSTYLE && tmpl.len() <= NSTYLE_MAX,
            "the rendition table is a power of two between NSTYLE and NSTYLE_MAX slots"
        );
        assert!(
            styles.order.len() == tmpl.len(),
            "the rendition table's log is one entry a slot"
        );
        assert!(
            styles.book.writes == StyleWrites::NEVER
                || tmpl[StyleId::DEFAULT.index()] == Some(StyleWord::DEFAULT),
            "a written rendition table holds the default in slot 0"
        );
        assert!(tabs.len() >= MAX_COLS, "tab stops are one per column");
        assert!(reply.len() >= REPLY, "the reply buffer is REPLY bytes");
        assert!(
            clusters.len() == NCLUSTER,
            "the cluster store is NCLUSTER slots"
        );
        Storage {
            arena,
            stride,
            depth,
            vp,
            used,
            cells3,
            tmpl: styles.slots,
            style_log: StyleLog {
                order: styles.order,
                book: styles.book,
            },
            tabs,
            reply,
            clusters: ClusterStore::new(clusters),
            alt: AltBuf {
                cells: alt,
                used: alt_used,
                rot: RowRot::NONE,
            },
            book: Book {
                hist: Rows::ZERO,
                hidden: Rows::ZERO,
                resets: crate::term::Resets::FIRST,
                top: LineIdx::FIRST,
                alt_top: LineIdx::FIRST,
                reach: Cols::of_index(0),
                held: Rows::ZERO,
                past,
                swept: LineIdx::FIRST,
                watched_at: crate::term::HistoryErasures::FIRST,
                window: HistoryWindow::Live,
            },
            strs,
            images,
        }
    }
}
