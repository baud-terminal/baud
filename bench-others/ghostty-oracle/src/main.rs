//! **V10's oracle: baud-core against libghostty-vt, cell for cell.**
//!
//! baud's two running oracles, alacritty (`baud-difftest`) and sfte
//! (`baud-sfte-difftest`), have no left and right margins, so neither can
//! judge anything `DECSLRM` does. ghostty can
//! (`[[source: ghostty-d9840f3-terminal]]`), and libghostty-vt exposes the
//! three things this compares, as note V's V10 design asks:
//!
//! * the screen, one cell at a time: blank, a codepoint, or a wide tail;
//! * the cursor's row and column;
//! * the pending-wrap flag, against [`Term::wrap_pending`].
//!
//! **What ghostty does not implement it cannot judge**, so it is not asked:
//! `DECIC` and `DECDC` appear nowhere in its `Terminal.zig` at d9840f3, and
//! a first run that fed them read ghostty's silence as a difference.
//!
//! **Every fixture is fed at every read size, to both terminals**, and each
//! must reach the same state at every size as it does whole: a fixture whose
//! answer depends on where the reads fell is reported before anything is
//! compared across the two.
//!
//! **A difference is either a failure or pinned, and a pinned one must still
//! differ.** [`Verdict::Pinned`] carries the reason -- a step of V10 not yet
//! built, or a place where ghostty departs from the DEC texts -- and when a
//! later change closes it the run fails until the pin is removed, which is
//! how `baud-difftest` keeps its own list honest.
//!
//! Run it with the library from a ghostty checkout at d9840f3:
//!
//!     GHOSTTY_LIB_DIR=/path/to/ghostty/zig-out/lib cargo run --release
//!
//! `--show-pins` prints both states under each pinned difference too.
//!
//! It exits nonzero on any unpinned difference, any pin that no longer
//! differs, any fixture that is not chunk-invariant, or a library that
//! reports another version. It is outside the workspace and gates nothing on its own:
//! the regression gate behind every coverage row is `baud-core`'s own tests.

use baud_core::{BytePos, Owned, ReadLen, Row, Term, WinSize, WrapPending};
use std::process::ExitCode;

/// The version the library built at d9840f3 reports.
///
/// **It carries no commit**: `ghostty_build_info`'s version string reads
/// `0.1.0-dev` with no build metadata, so the library cannot be tied to the
/// pinned `Terminal.zig` from inside. The tie is the checkout's: the
/// library on the box this was written on was built in a clean clone at
/// d9840f3 whose only reflog entry is the clone, three minutes after it
/// (the record, "V10's oracle"). This check catches a library from another
/// release line, not another commit.
const GHOSTTY_VERSION: &str = "0.1.0-dev";

const ROWS: u16 = 8;
const COLS: u16 = 20;

mod ghostty {
    //! The few calls of libghostty-vt's C ABI the comparison needs, from the
    //! headers of the checkout at d9840f3 (`include/ghostty/vt/*.h`).
    use std::ffi::c_void;

    const _: () = assert!(std::mem::size_of::<Point>() == 24);
    const _: () = assert!(std::mem::size_of::<GridRef>() == 24);

    pub type Terminal = *mut c_void;

    /// `GhosttyPoint`: an `int` tag, then a 16-byte union aligned to 8
    /// (`uint64_t _padding[2]`) whose coordinate variant is
    /// `{ uint16_t x; uint32_t y; }` -- so the union starts at byte 8, `x`
    /// is there and `y` at 12. Laid out field by field, because a first
    /// spelling put `x` at 4 and read row 1 for every row.
    #[repr(C)]
    pub struct Point {
        pub tag: i32,
        pub _gap: u32,
        pub x: u16,
        pub _x_pad: u16,
        pub y: u32,
        pub _rest: u64,
    }

    /// `GhosttyGridRef`, a sized struct.
    #[repr(C)]
    pub struct GridRef {
        pub size: usize,
        pub node: *mut c_void,
        pub x: u16,
        pub y: u16,
    }

    /// `GhosttyString`.
    #[repr(C)]
    pub struct GString {
        pub ptr: *const u8,
        pub len: usize,
    }

    pub const POINT_TAG_ACTIVE: i32 = 0;
    pub const DATA_CURSOR_X: i32 = 3;
    pub const DATA_CURSOR_Y: i32 = 4;
    pub const DATA_CURSOR_PENDING_WRAP: i32 = 5;
    pub const CELL_DATA_CODEPOINT: i32 = 1;
    pub const CELL_DATA_WIDE: i32 = 3;
    pub const CELL_WIDE_SPACER_TAIL: i32 = 2;
    pub const BUILD_INFO_VERSION_STRING: i32 = 5;

    unsafe extern "C" {
        pub fn ghostty_terminal_new(
            allocator: *const c_void,
            out: *mut Terminal,
            cols: u16,
            rows: u16,
        ) -> i32;
        pub fn ghostty_terminal_free(t: Terminal);
        pub fn ghostty_terminal_vt_write(t: Terminal, data: *const u8, len: usize);
        pub fn ghostty_terminal_get(t: Terminal, data: i32, out: *mut c_void) -> i32;
        pub fn ghostty_terminal_grid_ref(t: Terminal, point: Point, out: *mut GridRef) -> i32;
        pub fn ghostty_grid_ref_cell(r: *const GridRef, out: *mut u64) -> i32;
        pub fn ghostty_cell_get(cell: u64, data: i32, out: *mut c_void) -> i32;
        pub fn ghostty_build_info(data: i32, out: *mut c_void) -> i32;
    }
}

/// What one terminal holds after a fixture: the screen as one string a row,
/// the cursor, and the pending wrap.
///
/// A cell is `.` when blank, `]` when it is a wide character's tail, and its
/// character otherwise. ghostty's spacer *head* -- the blank it leaves in the
/// last column when a wide character wraps -- is blank here, which is what
/// baud stores there.
#[derive(Clone, PartialEq, Eq, Debug)]
struct State {
    screen: Vec<String>,
    cursor: (usize, usize),
    pending: bool,
}

fn baud_state(bytes: &[u8], read: usize) -> State {
    let ws = WinSize {
        ws_row: ROWS,
        ws_col: COLS,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    for piece in bytes.chunks(read.max(1)) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    let screen = (0..ROWS as usize)
        .map(|y| {
            t.row(Row::from_index(y)).expect("a row on the grid")[..COLS as usize]
                .iter()
                .map(|c| match c.content.is_wide_tail() {
                    true => ']',
                    false => match t.base_of(*c).as_codepoint() {
                        Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                        None => '.',
                    },
                })
                .collect()
        })
        .collect();
    let (r, c) = t.cursor();
    State {
        screen,
        cursor: (r.index(), c.index()),
        pending: t.wrap_pending() == WrapPending::Yes,
    }
}

fn ghostty_state(bytes: &[u8], read: usize) -> State {
    use ghostty::*;
    let mut t: Terminal = std::ptr::null_mut();
    // SAFETY: `t` is an out-pointer the call fills; NULL selects ghostty's
    // default allocator, as the header documents.
    let rc = unsafe { ghostty_terminal_new(std::ptr::null(), &mut t, COLS, ROWS) };
    assert_eq!(rc, 0, "ghostty_terminal_new failed with {rc}");
    for piece in bytes.chunks(read.max(1)) {
        // SAFETY: `piece` is a live slice and `t` a live terminal.
        unsafe { ghostty_terminal_vt_write(t, piece.as_ptr(), piece.len()) };
    }
    let mut screen = Vec::new();
    for y in 0..ROWS as u32 {
        let mut row = String::new();
        for x in 0..COLS {
            let mut gref = GridRef {
                size: std::mem::size_of::<GridRef>(),
                node: std::ptr::null_mut(),
                x: 0,
                y: 0,
            };
            let point = Point {
                tag: POINT_TAG_ACTIVE,
                _gap: 0,
                x,
                _x_pad: 0,
                y,
                _rest: 0,
            };
            let (mut cell, mut cp, mut wide) = (0u64, 0u32, 0i32);
            // SAFETY: every out-pointer is a live local of the type the
            // header names for that data kind; the ref is used before the
            // next mutating call, which the header requires.
            unsafe {
                assert_eq!(ghostty_terminal_grid_ref(t, point, &mut gref), 0);
                assert_eq!(ghostty_grid_ref_cell(&gref, &mut cell), 0);
                assert_eq!(
                    ghostty_cell_get(cell, CELL_DATA_CODEPOINT, (&raw mut cp).cast()),
                    0
                );
                assert_eq!(
                    ghostty_cell_get(cell, CELL_DATA_WIDE, (&raw mut wide).cast()),
                    0
                );
            }
            row.push(match (wide, cp) {
                (CELL_WIDE_SPACER_TAIL, _) => ']',
                (_, 0) => '.',
                (_, c) => char::from_u32(c).unwrap_or('?'),
            });
        }
        screen.push(row);
    }
    let (mut cx, mut cy, mut pending) = (0u16, 0u16, false);
    // SAFETY: out-pointers of the types the header names (uint16_t, bool).
    unsafe {
        assert_eq!(
            ghostty_terminal_get(t, DATA_CURSOR_X, (&raw mut cx).cast()),
            0
        );
        assert_eq!(
            ghostty_terminal_get(t, DATA_CURSOR_Y, (&raw mut cy).cast()),
            0
        );
        assert_eq!(
            ghostty_terminal_get(t, DATA_CURSOR_PENDING_WRAP, (&raw mut pending).cast()),
            0
        );
        ghostty_terminal_free(t);
    }
    State {
        screen,
        cursor: (cy as usize, cx as usize),
        pending,
    }
}

/// The version string the linked library reports: `0.1.0-dev`, with no
/// commit in it (`GHOSTTY_VERSION`).
fn ghostty_version() -> String {
    let mut s = ghostty::GString {
        ptr: std::ptr::null(),
        len: 0,
    };
    // SAFETY: a `GhosttyString` out-pointer, as the header names; the bytes
    // are static in the library.
    let rc = unsafe {
        ghostty::ghostty_build_info(ghostty::BUILD_INFO_VERSION_STRING, (&raw mut s).cast())
    };
    assert_eq!(rc, 0, "ghostty_build_info failed with {rc}");
    // SAFETY: `ptr` and `len` describe the library's own static string.
    let bytes = unsafe { std::slice::from_raw_parts(s.ptr, s.len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// What a fixture is expected to show.
#[derive(Clone, Copy)]
enum Verdict {
    /// The two terminals must reach the same state.
    Agree,
    /// They differ, for this reason, and must still differ.
    Pinned(&'static str),
}

struct Fixture {
    name: &'static str,
    bytes: Vec<u8>,
    verdict: Verdict,
}

/// Every row a letter of its own, written by `CUP` so no line feed or
/// pending wrap is involved: row 1 is `a` to its last column, row 8 `h`. A
/// row that moved shows a different letter; a column outside the fence that
/// moved shows one where its row's letter should be.
fn fill() -> Vec<u8> {
    let mut v = Vec::new();
    for r in 0..ROWS as usize {
        v.extend_from_slice(format!("\x1b[{};1H", r + 1).as_bytes());
        v.extend(std::iter::repeat_n(b'a' + r as u8, COLS as usize));
    }
    v
}

/// `?69h`, then side margins at columns 5..10 (1-based), which homes the
/// cursor.
const LRM: &str = "\x1b[?69h\x1b[5;10s";
/// Top and bottom margins at rows 2..6 (1-based), which homes it too.
const TB: &str = "\x1b[2;6r";

fn on_fill(tail: &str) -> Vec<u8> {
    let mut v = fill();
    v.extend_from_slice(tail.as_bytes());
    v
}

/// ghostty's departure from the DEC texts, which baud does not follow.
const INDEX_OUTSIDE: &str = "ghostty departs: IND and RI from outside the side margins move \
                             nothing there; STD 070 p. 5-24 marks them O for L/R, 'Continues \
                             to work outside these margins', and its algorithms test the \
                             line only (xterm departs alike)";
/// IL and DL move the cursor to the left margin: [[source: dec-std-070]] IL
/// and DL note 2, ECMA-48 8.3.67 and 8.3.32's line home position.
const HOME: &str = "IL/DL leave the column where it was; the texts move it to the \
                    left margin (STD 070 IL/DL note 2, ECMA-48 8.3.67/8.3.32)";
/// Note V's V10, step (c).
const STEP_C: &str = "V10 step (c): line starts and moves that honour the fence";
/// Note V's V10, step (d): the corner, and `Term::cursor`'s clamp of a
/// cursor right of the fence to the right margin, which reads it as a
/// pending wrap there.
const STEP_D: &str = "V10 step (d): the corner, a cursor right of the fence, C2-min";
/// ghostty's `CUB` under DECOM goes to column 1, left of the origin.
const CUB_PAST_ORIGIN: &str = "ghostty departs: CUB under DECOM goes past the left margin to \
                               column 1; STD 070 CUB note 1 (p. 5-47) stops it at the Left \
                               Margin, and DECOM (p. 5-31) bars the cursor from left of it";
/// ghostty's `DECRC` under DECOM does not confine what it restores, on
/// either side.
const DECRC_UNCONFINED: &str = "ghostty departs: DECRC under DECOM keeps a restored column \
                                outside the margins, left of the left margin or right of the \
                                right; STD 070's restore_cursor (p. 5-190) clamps it into the \
                                margins when the restored Origin Mode is displaced; xterm's \
                                CursorRestore goes through CursorSet (cursor.c 484-492), which \
                                stops it at the right margin and not at the left (79-84)";
/// `CHA`/`HPA` under DECOM: the displacement is a Jev item, and ghostty also
/// moves the row.
const CHA_DISPLACED: &str = "Jev item: under DECOM baud counts CHA/HPA from the screen's \
                             column 1 and clamps into the margins, option (a'); ghostty \
                             counts from the left margin, option (b), as xterm's CursorSet \
                             does, and also moves the row down by the top margin";
/// A margin past the page: STD 070 ignores the control, ghostty clamps it.
const PAST_PAGE: &str = "ghostty departs: a DECSLRM right margin or DECSTBM bottom margin \
                         past the page is clamped to its last column or line \
                         (setLeftAndRightMargin's and setTopAndBottomMargin's @min, \
                         Terminal.zig 2610 and 2596), as xterm's is (charproc.c \
                         4891-4894 and 4779-4782); STD 070's note 3 (pp. 5-27 and 5-25) \
                         and algorithms say the control is ignored (not executed)";
/// Note V's full-width pending-wrap follow-ons, each a later decision.
const FOLLOW_ON: &str = "note V's full-width follow-ons: baud keeps the sentinel \
                         across LF, ghostty clears its pending_wrap";

fn fixtures() -> Vec<Fixture> {
    use Verdict::*;
    let f = |name, tail: String, verdict| Fixture {
        name,
        bytes: on_fill(&tail),
        verdict,
    };
    vec![
        // ---- full width: the positive controls ------------------------------
        f("full: the fill alone", String::new(), Agree),
        f(
            "full: SU in a region",
            format!("{TB}\x1b[S\x1b[8;1H"),
            Agree,
        ),
        f(
            "full: SD in a region",
            format!("{TB}\x1b[T\x1b[8;1H"),
            Agree,
        ),
        f(
            "full: SU by more than the region",
            format!("{TB}\x1b[9S\x1b[8;1H"),
            Agree,
        ),
        f("full: DECSTBM homes", TB.into(), Agree),
        f(
            "full: IL at column 1",
            format!("{TB}\x1b[3;1H\x1b[L"),
            Agree,
        ),
        f(
            "full: DL at column 1",
            format!("{TB}\x1b[3;1H\x1b[2M"),
            Agree,
        ),
        f(
            "full: LF at the region's bottom",
            format!("{TB}\x1b[6;7H\n"),
            Agree,
        ),
        f(
            "full: RI at the region's top",
            format!("{TB}\x1b[2;7H\x1bM"),
            Agree,
        ),
        f(
            "full: IND below the region",
            format!("{TB}\x1b[8;7H\x1bD"),
            Agree,
        ),
        f(
            "full: IL homes the column",
            format!("{TB}\x1b[3;6H\x1b[L"),
            Pinned(HOME),
        ),
        f(
            "full: LF after a full row",
            "\x1b[3;1Hxxxxxxxxxxxxxxxxxxxx\nZ".into(),
            Pinned(FOLLOW_ON),
        ),
        // ---- side margins: what is built ------------------------------------
        f("lrm: DECSLRM homes", LRM.into(), Agree),
        f(
            "lrm: text wraps within the margins",
            format!("{LRM}\x1b[1;5Hxxxxxxx"),
            Agree,
        ),
        f(
            "lrm: ICH stops at the right margin",
            format!("{LRM}\x1b[1;5H\x1b[2@"),
            Agree,
        ),
        f(
            "lrm: DCH pulls from the right margin",
            format!("{LRM}\x1b[1;5H\x1b[2P"),
            Agree,
        ),
        // ---- side margins: the scrolls --------------------------------------
        f(
            "lrm: SU scrolls the rectangle",
            format!("{TB}{LRM}\x1b[S"),
            Agree,
        ),
        f(
            "lrm: SD scrolls the rectangle",
            format!("{TB}{LRM}\x1b[2T"),
            Agree,
        ),
        f(
            "lrm: SU by more than the region",
            format!("{TB}{LRM}\x1b[9S"),
            Agree,
        ),
        f(
            "lrm: SU with the cursor outside the fence",
            format!("{TB}{LRM}\x1b[1;15H\x1b[S"),
            Pinned(STEP_D),
        ),
        f(
            "lrm: IL inside the rectangle",
            format!("{TB}{LRM}\x1b[3;6H\x1b[L\x1b[8;1H"),
            Agree,
        ),
        f(
            "lrm: DL inside the rectangle",
            format!("{TB}{LRM}\x1b[3;6H\x1b[2M\x1b[8;1H"),
            Agree,
        ),
        f(
            "lrm: IL left of the fence does nothing",
            format!("{TB}{LRM}\x1b[3;2H\x1b[L\x1b[8;1H"),
            Agree,
        ),
        f(
            "lrm: DL right of the fence does nothing",
            format!("{TB}{LRM}\x1b[3;15H\x1b[M\x1b[8;1H"),
            Agree,
        ),
        f(
            "lrm: LF at the bottom margin",
            format!("{TB}{LRM}\x1b[6;7H\n"),
            Agree,
        ),
        f(
            "lrm: IND at the bottom margin",
            format!("{TB}{LRM}\x1b[6;7H\x1bD"),
            Agree,
        ),
        f(
            "lrm: RI at the top margin",
            format!("{TB}{LRM}\x1b[2;7H\x1bM"),
            Agree,
        ),
        f(
            "lrm: IND left of the fence at the bottom margin",
            format!("{TB}{LRM}\x1b[6;2H\x1bD"),
            Pinned(INDEX_OUTSIDE),
        ),
        f(
            "lrm: RI right of the fence at the top margin",
            format!("{TB}{LRM}\x1b[2;15H\x1bM"),
            Pinned(INDEX_OUTSIDE),
        ),
        f(
            "lrm: LF on the whole screen's last row",
            format!("{LRM}\x1b[8;7H\n"),
            Agree,
        ),
        f(
            "lrm: LF at a top-anchored region's bottom",
            format!("{LRM}\x1b[1;6r\x1b[6;7H\n"),
            Agree,
        ),
        f(
            "lrm: margins at the left edge, SU",
            format!("{TB}\x1b[?69h\x1b[1;10s\x1b[S"),
            Agree,
        ),
        f(
            "lrm: margins at the left edge, LF on the whole screen",
            "\x1b[?69h\x1b[1;10s\x1b[8;3H\n".into(),
            Agree,
        ),
        f(
            "lrm: margins at the right edge, SD",
            format!("{TB}\x1b[?69h\x1b[11;20s\x1b[2T"),
            Agree,
        ),
        f(
            "lrm: margins at the right edge, LF on the whole screen",
            "\x1b[?69h\x1b[11;20s\x1b[8;15H\n".into(),
            Agree,
        ),
        // A cursor moved to right + 1 reads as a pending wrap on the right
        // margin, so IL acts from it here; ghostty's is outside and inert.
        // Step (d) ends the conflation, and this goes red when it does.
        f(
            "lrm: IL at CUP right + 1",
            format!("{TB}{LRM}\x1b[3;11H\x1b[L\x1b[8;1H"),
            Pinned(STEP_D),
        ),
        f(
            "lrm: a wrap at the bottom margin scrolls the rectangle",
            format!("{TB}{LRM}\x1b[6;5H0123456789AB"),
            Agree,
        ),
        f(
            "lrm: a wide character across the left margin",
            format!("\x1b[3;4H\u{4e2d}{TB}{LRM}\x1b[S"),
            Agree,
        ),
        f(
            "lrm: a wide character across the right margin",
            format!("\x1b[4;10H\u{4e2d}{TB}{LRM}\x1b[S"),
            Agree,
        ),
        f(
            "lrm: a wide character across the left margin on the top row",
            format!("\x1b[2;4H\u{4e2d}{TB}{LRM}\x1b[S"),
            Agree,
        ),
        f(
            "lrm: a wide character across the right margin on the bottom row, SD",
            format!("\x1b[6;10H\u{4e2d}{TB}{LRM}\x1b[T"),
            Agree,
        ),
        f(
            "lrm: IL homes to the left margin",
            format!("{TB}{LRM}\x1b[3;7H\x1b[L"),
            Pinned(HOME),
        ),
        // ---- DECOM: the origin is the top and left margins -------------------
        f(
            "decom: ?6h homes to the margins' corner",
            format!("{TB}{LRM}\x1b[?6h"),
            Agree,
        ),
        f(
            "decom: CUP 1;1 addresses from the left margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[1;1HXY"),
            Agree,
        ),
        f(
            "decom: CUP stops at the right margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;99HQ"),
            Agree,
        ),
        f(
            "decom: DECSLRM homes to the margins' corner",
            format!("{TB}\x1b[?6h{LRM}"),
            Agree,
        ),
        f(
            "decom: DECSTBM homes to the margins' corner",
            format!("{LRM}\x1b[?6h{TB}"),
            Agree,
        ),
        // `?6l` at the end homes both, so IL's column (HOME) is not compared.
        f(
            "decom: IL from a displaced CUP inside the fence",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;2H\x1b[L\x1b[?6l"),
            Agree,
        ),
        // DECOM's confinement: "cannot be moved ... left of the Left Margin"
        // (STD 070 p. 5-31). Each starts at CUP 2;3, column 6 of the screen.
        f(
            "decom: CR goes to the left margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;3H\rX"),
            Agree,
        ),
        f(
            "decom: CNL goes to the left margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;3H\x1b[EX"),
            Agree,
        ),
        f(
            "decom: CPL goes to the left margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;3H\x1b[FX"),
            Agree,
        ),
        f(
            "decom: NEL goes to the left margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;3H\x1bEX"),
            Agree,
        ),
        f(
            "decom: CUF stops at the right margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;3H\x1b[99CX"),
            Agree,
        ),
        f(
            "decom: HT on the right margin stays there",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;6H\tX"),
            Agree,
        ),
        f(
            "decom: HT, HT stops at the right margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;3H\t\tX"),
            Agree,
        ),
        f(
            "decom: CBT stops at the left margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;3H\x1b[ZX"),
            Agree,
        ),
        f(
            "decom: DECRC confines the restored column",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;1H\x1b7\x1b[8;12s\x1b8X"),
            Pinned(DECRC_UNCONFINED),
        ),
        f(
            "decom: DECRC keeps a pending wrap",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;6HA\x1b7\x1b8B"),
            Agree,
        ),
        f(
            "decom: DECRC clamps a right-margin column a narrower DECSLRM passes",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;6H\x1b7\x1b[3;9s\x1b8X"),
            Pinned(DECRC_UNCONFINED),
        ),
        f(
            "decom: CUB stops at the left margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;3H\x1b[5DX"),
            Pinned(CUB_PAST_ORIGIN),
        ),
        f(
            "decom: CHA left of the margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;3H\x1b[3GX"),
            Pinned(CHA_DISPLACED),
        ),
        f(
            "decom: HPA left of the margin",
            format!("{TB}{LRM}\x1b[?6h\x1b[2;3H\x1b[3`X"),
            Pinned(CHA_DISPLACED),
        ),
        // ---- DECSLRM's defaults: an omitted or 0 Pr is the page's border ----
        // `[[source: vt510-decslrm]]`: "Default: Pr = 80 or 132 (depending on
        // the page width)". Each shows the margins by where `X` lands under
        // DECOM after `CUP 1;99`.
        f(
            "decslrm: a bare CSI s is the page's margins",
            format!("{LRM}\x1b[s\x1b[?6h\x1b[1;99HX"),
            Agree,
        ),
        f(
            "decslrm: CSI 0;0 s is the page's margins",
            format!("{LRM}\x1b[0;0s\x1b[?6h\x1b[1;99HX"),
            Agree,
        ),
        f(
            "decslrm: CSI 5 s runs to the page's right border",
            format!("{LRM}\x1b[5s\x1b[?6h\x1b[1;1HY\x1b[1;99HX"),
            Agree,
        ),
        f(
            "decslrm: CSI 5;0 s runs to the page's right border",
            format!("{LRM}\x1b[5;0s\x1b[?6h\x1b[1;99HX"),
            Agree,
        ),
        // ---- DECSTBM's zero bottom: a 0 Pb is the last line ----------------
        // `[[source: dec-std-070]]` DECSTBM: "default Pb: last line" (p.
        // 5-25), and its algorithm reads a 0 as omitted (p. 5-26).
        // Each shows the region by where `X` lands under DECOM after
        // `CUP 99;1`.
        f(
            "decstbm: CSI 2;0 r runs to the last line",
            "\x1b[4;4H\x1b[2;0rY\x1b[?6h\x1b[99;1HX".to_string(),
            Agree,
        ),
        f(
            "decstbm: CSI 2;0 r replaces a region",
            "\x1b[2;5r\x1b[2;0r\x1b[?6h\x1b[99;1HX".to_string(),
            Agree,
        ),
        f(
            "decstbm: CSI 0;0 r is the whole screen",
            "\x1b[2;5r\x1b[0;0r\x1b[?6h\x1b[99;1HX".to_string(),
            Agree,
        ),
        // ---- a margin past the page: the control is ignored ----------------
        // STD 070 DECSLRM and DECSTBM note 3 (pp. 5-27, 5-25): "this control
        // will be ignored (not executed)". The last column and line are the
        // controls.
        f(
            "decslrm: a right margin past the page leaves 5..10",
            format!("{LRM}\x1b[3;3H\x1b[5;21sY\x1b[?6h\x1b[1;99HX"),
            Pinned(PAST_PAGE),
        ),
        f(
            "decslrm: a right margin on the last column",
            format!("{LRM}\x1b[5;20s\x1b[?6h\x1b[1;99HX"),
            Agree,
        ),
        f(
            "decstbm: a bottom margin past the screen leaves 2..5",
            "\x1b[2;5r\x1b[4;4H\x1b[2;9rY\x1b[?6h\x1b[99;1HX".to_string(),
            Pinned(PAST_PAGE),
        ),
        f(
            "decstbm: a bottom margin on the last line",
            "\x1b[2;5r\x1b[2;8r\x1b[?6h\x1b[99;1HX".to_string(),
            Agree,
        ),
        // ---- side margins: the later steps ----------------------------------
        f(
            "lrm: CR inside the fence",
            format!("{LRM}\x1b[3;7HAB\rC"),
            Pinned(STEP_C),
        ),
        f(
            "lrm: a write right of the fence",
            format!("{LRM}\x1b[1;15HXY"),
            Pinned(STEP_D),
        ),
        // ---- DECALN: origin mode and every margin back ----------------------
        // STD 070 pp. D-19 and D-20 and the VT510's "the margins to the
        // extremes of the page"; ghostty's `decaln` resets all four margins
        // and origin mode too (Terminal.zig 3706-3730).
        f(
            "decaln: resets origin mode",
            format!("{TB}\x1b[?6h\x1b[3;3H\x1b#8\x1b[3;5rX"),
            Agree,
        ),
        f(
            "decaln: resets the side margins",
            format!("{LRM}\x1b#8\x1b[1;9HABC"),
            Agree,
        ),
        f(
            "decaln: under origin mode and both margin pairs",
            format!("{TB}{LRM}\x1b[?6h\x1b[3;7H\x1b#8"),
            Agree,
        ),
    ]
}

fn show(s: &State) -> String {
    let mut out = String::new();
    for (y, row) in s.screen.iter().enumerate() {
        out.push_str(&format!("    {y} |{row}|\n"));
    }
    out.push_str(&format!(
        "    cursor {:?}, pending {}\n",
        s.cursor, s.pending
    ));
    out
}

fn main() -> ExitCode {
    let version = ghostty_version();
    if version != GHOSTTY_VERSION {
        eprintln!(
            "libghostty-vt reports {version:?}, not {GHOSTTY_VERSION:?}: it was not \
             built from the checkout the pinned Terminal.zig describes"
        );
        return ExitCode::FAILURE;
    }
    println!("libghostty-vt {version}");
    // `--show-pins` prints both states under every pinned difference, which
    // is what a pin's reason has to be checked against.
    let show_pins = std::env::args().any(|a| a == "--show-pins");
    let (mut agree, mut pinned, mut failed) = (0, 0, 0);
    for fx in fixtures() {
        let whole = fx.bytes.len();
        let b = baud_state(&fx.bytes, whole);
        let g = ghostty_state(&fx.bytes, whole);
        // Chunk invariance first: a state that depends on the reads is a
        // finding in itself, and comparing it across terminals would hide it.
        let mut split = None;
        for read in 1..whole {
            if baud_state(&fx.bytes, read) != b {
                split = Some(("baud", read));
                break;
            }
            if ghostty_state(&fx.bytes, read) != g {
                split = Some(("ghostty", read));
                break;
            }
        }
        if let Some((who, read)) = split {
            println!("FAIL  {}: {who} differs at reads of {read}", fx.name);
            failed += 1;
            continue;
        }
        match (fx.verdict, b == g) {
            (Verdict::Agree, true) => {
                println!("ok    {}", fx.name);
                agree += 1;
            }
            (Verdict::Pinned(why), false) => {
                println!("pin   {}: {why}", fx.name);
                if show_pins {
                    println!("  baud\n{}  ghostty\n{}", show(&b), show(&g));
                }
                pinned += 1;
            }
            (Verdict::Agree, false) => {
                println!(
                    "FAIL  {}\n  baud\n{}  ghostty\n{}",
                    fx.name,
                    show(&b),
                    show(&g)
                );
                failed += 1;
            }
            (Verdict::Pinned(why), true) => {
                println!(
                    "FAIL  {}: pinned ({why}) but agrees -- remove the pin",
                    fx.name
                );
                failed += 1;
            }
        }
    }
    println!("{agree} agree, {pinned} pinned, {failed} failed");
    match failed {
        0 => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The probe can fire.** One byte of difference -- a letter written
    /// on one side only -- has to read as a difference, or every `ok` above
    /// is a comparison of nothing.
    #[test]
    fn a_difference_is_seen() {
        let a = baud_state(&on_fill(""), usize::MAX);
        let b = baud_state(&on_fill("\x1b[1;1HZ"), usize::MAX);
        assert_ne!(a, b);
        let a = ghostty_state(&on_fill(""), usize::MAX);
        let b = ghostty_state(&on_fill("\x1b[1;1HZ"), usize::MAX);
        assert_ne!(a, b);
    }

    /// **The two read the same fill the same way**: the control every other
    /// comparison stands on.
    #[test]
    fn the_fill_agrees() {
        let bytes = on_fill("");
        assert_eq!(
            baud_state(&bytes, usize::MAX),
            ghostty_state(&bytes, usize::MAX)
        );
    }

    /// The pending wrap is compared, not assumed: a full row then nothing
    /// sets it on both sides, and a CR clears it on both.
    #[test]
    fn the_pending_wrap_is_compared() {
        let full = on_fill("");
        assert!(baud_state(&full, usize::MAX).pending);
        assert!(ghostty_state(&full, usize::MAX).pending);
        let cr = on_fill("\r");
        assert!(!baud_state(&cr, usize::MAX).pending);
        assert!(!ghostty_state(&cr, usize::MAX).pending);
    }
}
