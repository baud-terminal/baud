use crate::*;

#[repr(C)]
#[cfg_attr(
    feature = "alloc",
    derive(zerocopy::FromZeros, zerocopy::KnownLayout, zerocopy::Immutable)
)]
pub struct Storage {
    pub arena: [u8; ARENA],
    pub runs: [Run; SB * MAXRUN],
    pub nrun: [RunCount; SB],
    pub vp: [PackedCell; GRID],
    pub promoted: [bool; MAX_ROWS],
    pub tmpl: [PackedCell; NSTYLE * MAX_COLS],
    pub tmpl_key: [StyleWord; NSTYLE],
    pub tmpl_valid: [bool; NSTYLE],
    /// Bytes the terminal owes the host.
    ///
    /// A terminal is not write-only: DA, DSR and DECRQM are questions, and an
    /// application that asks one and gets nothing back **hangs** rather than
    /// degrading. The core cannot own a PTY -- it is `no_std` with no I/O -- so
    /// this mirrors the input side: the core fills a buffer, the caller drains
    /// it with [`Term::reply`] and writes it wherever the input came from.
    pub reply: [u8; REPLY],
    /// Tab stops, one flag per column.
    ///
    /// A real table rather than the every-8 arithmetic HT used to do: HTS, TBC,
    /// CHT and CBT are meaningless without one, and `HT` after `CSI 3 g` must
    /// run to the right margin rather than to the next multiple of eight.
    pub tabs: [bool; MAX_COLS],
    /// The OSC payload being accumulated, and the two strings it can become.
    pub osc: [u8; OSC_MAX],
    pub title: [u8; OSC_MAX],
    pub icon: [u8; OSC_MAX],
    /// Palette overrides from `OSC 4`. `ColorWord::DEFAULT` means "not
    /// overridden" -- a zeroed `Storage` therefore starts on the built-in
    /// palette, with no separate `is_set` array to keep in step.
    pub palette: [ColorWord; 256],
    /// Hyperlink URIs, indexed by [`LinkId`]. Slot 0 is "no link", so a zeroed
    /// `Storage` starts with nothing linked and no separate valid array.
    pub links: [[u8; LINK_MAX]; NLINK],
    pub link_len: [ByteLen; NLINK],
    /// The alternate screen buffer, `CSI ? 1049 h`.
    ///
    /// A second full-size grid, which looks expensive and is not: it is 10 MiB
    /// of *virtual* address space, and MEASUREMENTS.md records that untouched
    /// pages of this arena cost ~0.3 MB resident. A terminal that never runs a
    /// full-screen program never faults most of it in.
    ///
    /// Placed last, on principle rather than on a measurement.
    ///
    /// Putting it straight after `vp` pushes every small hot structure --
    /// `promoted`, `tmpl`, `tabs`, `reply` -- 10 MiB further from the grid,
    /// and an erase touches the grid, `promoted` and `nrun` together. Keeping
    /// them adjacent is what "working set <= cache" asks for.
    ///
    /// **The benchmark did not resolve it.** Both placements were measured
    /// same-session: streaming showed no difference either way, and the TUI
    /// cells figures moved by more than the placement could explain while
    /// ghostty moved 2% in the same runs. Ranges overlap or the control drifted,
    /// so there is no result -- this ordering is the reasoned default, not a
    /// measured win, and it should not be quoted as one.
    ///
    /// It has **no scrollback**, which is what makes it cheaper than the
    /// primary screen rather than merely as expensive: no ring, no run table,
    /// no promotion. `[[source: ctlseqs]]` is explicit that switching clears
    /// it, so nothing here needs to survive a switch.
    pub alt: [PackedCell; GRID],
}

impl Storage {
    /// Allocate zeroed on the heap without a stack temporary.
    ///
    /// `Box::new(Storage::zeroed())` builds ~24 MB on the stack before moving
    /// it; LLVM elides that copy sometimes and not others, and a test thread
    /// overflowed on it. This is the only construction path callers should use
    /// — and it means no test or harness hand-rolls `vec![0u8; size_of::<..>()]`
    /// with a raw pointer cast.
    #[cfg(feature = "alloc")]
    pub fn boxed() -> alloc::boxed::Box<Storage> {
        use zerocopy::FromZeros;
        Storage::new_box_zeroed().expect("allocation failed")
    }

    pub const fn zeroed() -> Self {
        const R: Run = Run {
            pos: BytePos::ZERO,
            len: ByteLen::ZERO,
            style: StyleId::DEFAULT,
            encoding: 0,
            flags: 0,
            _pad: [0; 2],
        };
        const C: PackedCell = PackedCell::EMPTY;
        Storage {
            arena: [0; ARENA],
            runs: [R; SB * MAXRUN],
            nrun: [RunCount::ZERO; SB],
            vp: [C; GRID],
            alt: [C; GRID],
            promoted: [false; MAX_ROWS],
            tmpl: [C; NSTYLE * MAX_COLS],
            tmpl_key: [StyleWord::DEFAULT; NSTYLE],
            tmpl_valid: [false; NSTYLE],
            // Zeroed here and populated by Term::reset_tabs, so `Storage` stays
            // a plain zeroable block for `new_box_zeroed`.
            tabs: [false; MAX_COLS],
            osc: [0; OSC_MAX],
            title: [0; OSC_MAX],
            icon: [0; OSC_MAX],
            palette: [ColorWord::DEFAULT; 256],
            links: [[0; LINK_MAX]; NLINK],
            link_len: [ByteLen::ZERO; NLINK],
            reply: [0; REPLY],
        }
    }
}
