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
//! A fixture is bytes, and may resize between them: `Term::resize` against
//! `ghostty_terminal_resize`, each read at the geometry it leaves.
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

use baud_core::{BytePos, Owned, ReadLen, Resized, Row, Term, WinSize, WrapPending};
use std::process::ExitCode;

/// The version the library built at d9840f3 reports.
///
/// **It carries no commit**: `ghostty_build_info`'s version string reads
/// `0.1.0-dev` with no build metadata, so the library cannot be tied to the
/// pinned `Terminal.zig` from inside. The tie is the checkout's: the
/// library on the box this was written on was built in a clean clone at
/// d9840f3 whose only reflog entry is the clone, three minutes after it.
/// This check catches a library from another release line, not another
/// commit.
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
        pub fn ghostty_terminal_resize(
            t: Terminal,
            cols: u16,
            rows: u16,
            cell_width: u32,
            cell_height: u32,
        ) -> i32;
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

/// A step of a fixture: bytes, fed at the read size under test, or a
/// resize to rows by columns between two reads.
#[derive(Clone)]
enum Step {
    Bytes(Vec<u8>),
    Resize(u16, u16),
}

/// The geometry after `steps`, and the largest any of them asks for.
fn geometry(steps: &[Step]) -> ((u16, u16), (u16, u16)) {
    let (mut now, mut most) = ((ROWS, COLS), (ROWS, COLS));
    for s in steps {
        if let Step::Resize(r, c) = *s {
            now = (r, c);
            most = (most.0.max(r), most.1.max(c));
        }
    }
    (now, most)
}

fn baud_state(steps: &[Step], read: usize) -> State {
    let ws = WinSize {
        ws_row: ROWS,
        ws_col: COLS,
    };
    let ((rows, cols), (most_rows, most_cols)) = geometry(steps);
    let room = WinSize {
        ws_row: most_rows,
        ws_col: most_cols,
    };
    let mut owned = Owned::new(&room);
    let mut t = Term::new(owned.storage(), &ws);
    for s in steps {
        match s {
            Step::Bytes(bytes) => {
                for piece in bytes.chunks(read.max(1)) {
                    t.write_at(BytePos::ZERO, piece);
                    t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
                }
            }
            Step::Resize(r, c) => {
                let to = WinSize {
                    ws_row: *r,
                    ws_col: *c,
                };
                assert_eq!(t.resize(&to), Resized::Ok, "room for {r}x{c}");
            }
        }
    }
    let screen = (0..rows as usize)
        .map(|y| {
            t.row(Row::from_index(y)).expect("a row on the grid")[..cols as usize]
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

fn ghostty_state(steps: &[Step], read: usize) -> State {
    use ghostty::*;
    let mut t: Terminal = std::ptr::null_mut();
    // SAFETY: `t` is an out-pointer the call fills; NULL selects ghostty's
    // default allocator, as the header documents.
    let rc = unsafe { ghostty_terminal_new(std::ptr::null(), &mut t, COLS, ROWS) };
    assert_eq!(rc, 0, "ghostty_terminal_new failed with {rc}");
    let ((rows, cols), _) = geometry(steps);
    for s in steps {
        match s {
            Step::Bytes(bytes) => {
                for piece in bytes.chunks(read.max(1)) {
                    // SAFETY: `piece` is a live slice and `t` a live terminal.
                    unsafe { ghostty_terminal_vt_write(t, piece.as_ptr(), piece.len()) };
                }
            }
            Step::Resize(r, c) => {
                // SAFETY: `t` is a live terminal; the cell size is any
                // nonzero one, which only pixel reports read.
                let rc = unsafe { ghostty_terminal_resize(t, *c, *r, 10, 20) };
                assert_eq!(rc, 0, "ghostty_terminal_resize failed with {rc}");
            }
        }
    }
    let mut screen = Vec::new();
    for y in 0..rows as u32 {
        let mut row = String::new();
        for x in 0..cols {
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
    steps: Vec<Step>,
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
/// ghostty's `CR` and line starts test the column only.
const CR_ROWS: &str = "ghostty departs: CR on a row outside the top and bottom margins \
                       goes to the left margin (carriageReturn tests the column only, \
                       Terminal.zig 1822-1833), as xterm's CarriageReturn does (cursor.c \
                       345-371); STD 070 CR (p. 5-58) sends it to the first column, \
                       'or outside the top and bottom margins', and NEL (p. 5-64) is \
                       CR then LF, so the line it leaves decides";
/// ghostty's `CUB` without `?45` passes the left margin.
const CUB_MARGIN: &str = "ghostty departs: without ?45 CUB passes the left margin \
                          (cursorLeft's min(count, x), Terminal.zig 1913); STD 070 CUB \
                          note 1 (p. 5-47) stops it there from at or inside it, as \
                          xterm's CursorBack does (cursor.c 148-149)";
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
/// `CHA`/`HPA` under DECOM: the displacement is an open question, and
/// ghostty also moves the row.
const CHA_DISPLACED: &str = "An open question: under DECOM baud counts CHA/HPA from the screen's \
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
/// `?69l` puts the margins back and leaves the cursor, so the `X` after it
/// lands on the cursor's own column on both terminals: right of the old
/// fence with no wrap first, and where baud's `Term::cursor` would clamp it.
const PROBE: &str = "\x1b[?69lX";
/// ghostty's `HT` from right of the right margin.
const HT_PAST_RIGHT: &str = "ghostty departs: HT from beyond the right margin does not move \
                             (horizontalTab loops while x < right, Terminal.zig 2257); STD \
                             070 HT note 1 (p. 5-67) moves it to the next tab stop or the \
                             last column; xterm's TabToNextStop pulls it back to the right \
                             margin (tabs.c 152-156)";
/// ghostty's `HT` on a row outside the scroll area.
const HT_OUTSIDE: &str = "ghostty departs: HT on a row outside the scroll area stops at the \
                          right margin (Terminal.zig 2257); STD 070 HT note 1 (p. 5-67) \
                          moves it to the next tab stop or the last column there; xterm \
                          stops it at the right margin on every row (tabs.c 152-155)";
/// ghostty's `HT` at a pending wrap keeps it.
const HT_WRAP: &str = "ghostty departs: HT at a pending wrap keeps the wrap pending \
                       (horizontalTab moves nothing, Terminal.zig 2257); STD 070 D.6.1 \
                       (pp. D-13, D-14) lists HORIZONTAL_TAB among the controls that \
                       reset the Last Column Flag, and p. C-20 records the PRO 350's \
                       HT not resetting it as a product defect to fix";
/// ghostty's `BS` at the left margin.
const BS_MARGIN: &str = "ghostty departs: without ?45 BS passes the left margin on every \
                         row (cursorLeft's min(count, x), Terminal.zig 1913), and with \
                         ?45 it stops there on every row; STD 070 BS notes 1-2 (p. 5-63) \
                         stop it at the margin inside the scroll area only. xterm stops \
                         it at the margin on every row (CursorBack, cursor.c 148-149)";
/// A wide character that does not fit before the line's end.
const WIDE_LAST: &str = "found, not fixed: a wide character with one column left before \
                         the line's end leaves that column as it was here; ghostty blanks \
                         it before the wrap, a spacer head at the screen's edge \
                         (Terminal.zig 1543-1556) -- at full width and right of the fence \
                         alike";
/// The same with `?7l`.
const NOWRAP_WIDE: &str = "found, not fixed: with ?7l a wide character on the last column is \
                           written here as a lead with no tail; ghostty prints nothing and \
                           leaves the cursor, 'how xterm behaves' (Terminal.zig 1536-1542), \
                           as xterm's dotext writes nothing (charproc.c 7142-7173) -- at \
                           full width and right of the fence alike";
/// A write into the last column with `?7l` sets ghostty's flag.
const NOWRAP_FLAG: &str = "ghostty departs: with ?7l a write into the last column sets its \
                           pending_wrap (Terminal.zig 1574-1578), as xterm's do_wrap is set \
                           (charproc.c 7157-7173); STD 070's algorithm sets the Last Column \
                           Flag only with Auto Wrap on (p. 5-141) -- at full width and right \
                           of the fence alike";
/// Row 3 filled from column 1, the wrap pending at its last column.
const FULL: &str = "\x1b[3;1Habcdefghijklmnopqrst";
/// A wrap pending at the fence's right margin, column 10, kept there by
/// `?69l` putting the margins back.
const KEPT: &str = "\x1b[?69h\x1b[5;10s\x1b[3;5HABCDEF\x1b[?69l";
/// ghostty's `RI` on the top margin keeps its flag.
const RI_TOP: &str = "ghostty departs: RI on the top margin keeps a pending wrap \
                      (reverseIndex's scrollDown saves and restores pending_wrap, \
                      Terminal.zig 2516-2526 and 2619-2626), so the next character \
                      wraps onto the line the scroll moved down; STD 070 D.6.1 (pp. \
                      D-13, D-14) lists REVERSE_INDEX among the controls that reset \
                      the Last Column Flag, and xterm's RevIndex resets do_wrap after \
                      its scroll (cursor.c 319-339)";
/// ghostty's `ICH` and `DCH` keep the side margins outside the scroll area.
const EDIT_OUTSIDE: &str = "ghostty departs: ICH and DCH on a row outside the top and bottom \
                            margins stop at the right margin and ignore a column outside the \
                            side margins (insertBlanks and deleteChars test the column only, \
                            Terminal.zig 3298-3299 and 3390-3391), as xterm's do (util.c \
                            1898-1899, 1998-1999); a VT420 and a VT520 shifted those rows to \
                            the line's end (microsoft/terminal issue 14876's logs), and STD \
                            070's table has both continue to work outside those margins \
                            (p. 5-24)";
/// ghostty's `ICH` right of the side margins ends its flag.
const ICH_IGNORED: &str = "ghostty departs: ICH right of the side margins ends a pending \
                           wrap (insertBlanks resets pending_wrap before its margin test, \
                           Terminal.zig 3284-3291); STD 070 ICH note 2 (p. 5-142) ignores \
                           the control there, and xterm's InsertChar returns before \
                           ResetWrap (util.c 1898)";
/// ghostty's `U+FE0F` widening leaves its flag set.
const FE0F_KEPT: &str = "found in ghostty: a U+FE0F after a resize that kept its flag \
                         widens the base in place but leaves pending_wrap set, the \
                         widen moving the screen's cursor past the tail without \
                         clearing it (Terminal.zig 1293-1406), so the next character \
                         wraps from mid-line; the wait was after the base, which now \
                         ends short of the line's end, as a base written mid-row does";
/// A reset of DECAWM keeps ghostty's flag.
const AWM_RESET: &str = "ghostty departs: ?7l keeps a pending wrap, so after ?7l ?7h the \
                         next character wraps; STD 070 D.6.1 (pp. D-13, D-14) lists \
                         RESET_MODE (AUTO_WRAP_MODE) among the controls that reset the \
                         Last Column Flag, and 070-5's revision item 16 (p. 5-217) adds \
                         the clearing to Reset Auto Wrap Mode's algorithm; xterm keeps \
                         do_wrap too (charproc.c 7603-7605)";

fn fixtures() -> Vec<Fixture> {
    use Verdict::*;
    let f = |name, tail: String, verdict| Fixture {
        name,
        steps: vec![Step::Bytes(on_fill(&tail))],
        verdict,
    };
    // A fixture on a blank screen, its steps as given: the resizes'.
    let r = |name, steps: Vec<Step>| Fixture {
        name,
        steps,
        verdict: Agree,
    };
    let bytes = |s: &str| Step::Bytes(s.as_bytes().to_vec());
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
            Agree,
        ),
        f(
            "full: DL homes the column",
            format!("{TB}\x1b[3;6H\x1b[2M"),
            Agree,
        ),
        f(
            "full: LF after a full row",
            "\x1b[3;1Hxxxxxxxxxxxxxxxxxxxx\nZ".into(),
            Agree,
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
        // Inside the scroll area, left of the left margin, both are
        // ignored (ICH note 2, DCH note 3); outside it the side margins do
        // not bound them, as a VT420 and a VT520 did.
        f(
            "lrm: ICH left of the left margin",
            format!("{LRM}\x1b[1;2H\x1b[2@"),
            Agree,
        ),
        f(
            "lrm: DCH left of the left margin",
            format!("{LRM}\x1b[1;2H\x1b[2P"),
            Agree,
        ),
        // The fill's rows are one letter each, so a marker on each side of
        // the right margin shows where ICH's shift ends.
        f(
            "lrm: ICH above the region runs to the line's end",
            format!("{TB}{LRM}\x1b[1;9Hx\x1b[1;12Hz\x1b[1;7H\x1b[2@"),
            Pinned(EDIT_OUTSIDE),
        ),
        f(
            "lrm: DCH below the region pulls from the line's end",
            format!("{TB}{LRM}\x1b[8;7H\x1b[2P"),
            Pinned(EDIT_OUTSIDE),
        ),
        f(
            "lrm: ICH above the region left of the left margin",
            format!("{TB}{LRM}\x1b[1;2H\x1b[2@"),
            Pinned(EDIT_OUTSIDE),
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
            Agree,
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
        // A cursor moved to right + 1 is placed there, outside the fence,
        // where IL is inert.
        f(
            "lrm: IL at CUP right + 1",
            format!("{TB}{LRM}\x1b[3;11H\x1b[L\x1b[8;1H"),
            Agree,
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
            Agree,
        ),
        f(
            "lrm: DL homes to the left margin",
            format!("{TB}{LRM}\x1b[3;10H\x1b[M"),
            Agree,
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
        // `?6l` at the end homes both, so IL's column is not compared.
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
        // ---- HT and BS: where the cursor starts decides the edge ------------
        // STD 070 HT note 1 (p. 5-67) and BS notes 1-2 (p. 5-63); each ends in
        // PROBE, which reads the cursor's column on both terminals.
        f(
            "htbs: HT inside stops at the right margin",
            format!("{TB}{LRM}\x1b[3;9H\t{PROBE}"),
            Agree,
        ),
        f(
            "htbs: HT on the right margin stays there",
            format!("{TB}{LRM}\x1b[3;10H\t{PROBE}"),
            Agree,
        ),
        f(
            "htbs: HT from left of the margin with no stops",
            format!("{TB}{LRM}\x1b[3g\x1b[3;2H\t{PROBE}"),
            Agree,
        ),
        f(
            "htbs: HT on the top margin row stops at the right margin",
            format!("{TB}{LRM}\x1b[2;9H\t{PROBE}"),
            Agree,
        ),
        f(
            "htbs: HT on the last row with no region stops at the right margin",
            format!("{LRM}\x1b[8;9H\t{PROBE}"),
            Agree,
        ),
        f(
            "htbs: CHT 2 stops at the right margin",
            format!("{TB}{LRM}\x1b[3;6H\x1b[2I{PROBE}"),
            Agree,
        ),
        f(
            "htbs: HT from two columns right of the right margin",
            format!("{TB}{LRM}\x1b[3;12H\t{PROBE}"),
            Pinned(HT_PAST_RIGHT),
        ),
        f(
            "htbs: HT one past the right margin under ?7l",
            format!("{TB}{LRM}\x1b[?7l\x1b[3;11H\t{PROBE}"),
            Pinned(HT_PAST_RIGHT),
        ),
        f(
            "htbs: HT on a row above the region",
            format!("{TB}{LRM}\x1b[1;9H\t{PROBE}"),
            Pinned(HT_OUTSIDE),
        ),
        f(
            "htbs: HT at a pending wrap under side margins",
            format!("{TB}{LRM}\x1b[3;5Habcdef\t{PROBE}"),
            Pinned(HT_WRAP),
        ),
        f(
            "htbs: HT at a pending wrap at full width",
            "\x1b[3;1Hzzzzzzzzzzzzzzzzzzzz\tX".to_string(),
            Pinned(HT_WRAP),
        ),
        f(
            "htbs: BS left of the left margin moves one column",
            format!("{TB}{LRM}\x1b[3;3H\x08{PROBE}"),
            Agree,
        ),
        f(
            "htbs: BS on the first column left of the margin",
            format!("{TB}{LRM}\x1b[3;1H\x08{PROBE}"),
            Agree,
        ),
        f(
            "htbs: BS at the left margin on a row below the region",
            format!("{TB}{LRM}\x1b[8;5H\x08{PROBE}"),
            Agree,
        ),
        f(
            "htbs: ?45 BS left of the left margin moves one column",
            format!("{TB}{LRM}\x1b[?45h\x1b[3;3H\x08{PROBE}"),
            Agree,
        ),
        f(
            "htbs: BS at the left margin inside the region stops",
            format!("{TB}{LRM}\x1b[3;5H\x08{PROBE}"),
            Pinned(BS_MARGIN),
        ),
        f(
            "htbs: BS at the left margin on the top margin row stops",
            format!("{TB}{LRM}\x1b[2;5H\x08{PROBE}"),
            Pinned(BS_MARGIN),
        ),
        f(
            "htbs: ?45 BS at the left margin on a row below the region",
            format!("{TB}{LRM}\x1b[?45h\x1b[8;5H\x08{PROBE}"),
            Pinned(BS_MARGIN),
        ),
        f(
            "htbs: ?45 BS at the left margin inside the region",
            format!("{TB}{LRM}\x1b[?45h\x1b[3;5H\x08{PROBE}"),
            Agree,
        ),
        // ---- side margins: line starts and moves, DECOM reset ---------------
        // STD 070 CR (p. 5-58), NEL (p. 5-64), CUF and CUB (pp. 5-45, 5-47).
        f(
            "lrm: CR inside the fence",
            format!("{LRM}\x1b[3;7HAB\rC"),
            Agree,
        ),
        f(
            "lrm: CR right of the fence",
            format!("{LRM}\x1b[3;15H\r{PROBE}"),
            Agree,
        ),
        f(
            "lrm: CR left of the fence",
            format!("{LRM}\x1b[3;2H\rC"),
            Agree,
        ),
        f(
            "lrm: CR at a pending wrap",
            format!("{LRM}\x1b[3;5HABCDEF\rZ"),
            Agree,
        ),
        f(
            "lrm: CR LF inside the fence",
            format!("{LRM}\x1b[3;7H\r\nC"),
            Agree,
        ),
        f(
            "lrm: CR below the scroll area",
            format!("{TB}{LRM}\x1b[8;7H\rC"),
            Pinned(CR_ROWS),
        ),
        f(
            "lrm: NEL inside the fence",
            format!("{TB}{LRM}\x1b[3;7H\x1bEC"),
            Agree,
        ),
        f(
            "lrm: NEL from above the scroll area",
            format!("{TB}{LRM}\x1b[1;7H\x1bEC"),
            Pinned(CR_ROWS),
        ),
        f(
            "lrm: LF under LNM inside the fence",
            format!("{LRM}\x1b[20h\x1b[3;7H\nC\x1b[20l"),
            Agree,
        ),
        f(
            "lrm: CNL inside the fence",
            format!("{LRM}\x1b[3;7H\x1b[EC"),
            Agree,
        ),
        f(
            "lrm: CPL right of the fence",
            format!("{LRM}\x1b[3;15H\x1b[F{PROBE}"),
            Agree,
        ),
        f(
            "lrm: CNL into the scroll area",
            format!("{TB}{LRM}\x1b[1;7H\x1b[EC"),
            Agree,
        ),
        f(
            "lrm: CUF stops at the right margin",
            format!("{LRM}\x1b[3;8H\x1b[5CX"),
            Agree,
        ),
        f(
            "lrm: CUF from left of the fence",
            format!("{LRM}\x1b[3;2H\x1b[20CX"),
            Agree,
        ),
        f(
            "lrm: CUF beyond the right margin",
            format!("{LRM}\x1b[3;13H\x1b[3C{PROBE}"),
            Agree,
        ),
        f(
            "lrm: CUF at a pending wrap",
            format!("{LRM}\x1b[3;5HABCDEF\x1b[CX"),
            Agree,
        ),
        f(
            "lrm: CUF from one past the right margin, ?7l",
            format!("{LRM}\x1b[?7l\x1b[3;11H\x1b[2C{PROBE}"),
            Agree,
        ),
        f(
            "lrm: CUF from one past the right margin, ?7h",
            format!("{LRM}\x1b[3;11H\x1b[2C{PROBE}"),
            Agree,
        ),
        f(
            "lrm: HPR passes the right margin",
            format!("{LRM}\x1b[3;8H\x1b[5a{PROBE}"),
            Agree,
        ),
        f(
            "lrm: CUB stops at the left margin",
            format!("{LRM}\x1b[3;7H\x1b[5DX"),
            Pinned(CUB_MARGIN),
        ),
        f(
            "lrm: CUB from left of the fence",
            format!("{LRM}\x1b[3;4H\x1b[5DX"),
            Agree,
        ),
        // ---- the corner: a cursor right of the fence, V10's step (d) --------
        // STD 070 p. 5-140 and xterm's dotext: from right of the right margin
        // a write runs to the line's end, then wraps to the left margin.
        f(
            "lrm: a write right of the fence",
            format!("{LRM}\x1b[1;15HXY"),
            Agree,
        ),
        f(
            "corner: a run right of the fence to the edge and on",
            format!("{LRM}\x1b[3;15HABCDEFGHIJKLMNO"),
            Agree,
        ),
        f(
            "corner: a fill to the edge right of the fence",
            format!("{LRM}\x1b[3;15HABCDEF"),
            Agree,
        ),
        f(
            "corner: a fill to the edge, then one more",
            format!("{LRM}\x1b[3;15HABCDEFG"),
            Agree,
        ),
        f(
            "corner: a narrow non-ASCII run right of the fence",
            format!("{LRM}\x1b[3;15H\u{e9}\u{e9}\u{e9}"),
            Agree,
        ),
        f(
            "corner: a wide character on the last column right of the fence",
            format!("{LRM}\x1b[3;20H\u{4e2d}"),
            Pinned(WIDE_LAST),
        ),
        f(
            "full: a wide character on the last column",
            "\x1b[3;20H\u{4e2d}".into(),
            Pinned(WIDE_LAST),
        ),
        f(
            "full: ?7l a wide character on the last column",
            "\x1b[?7l\x1b[3;20H\u{4e2d}".into(),
            Pinned(NOWRAP_WIDE),
        ),
        f(
            "corner: ?7l a wide character on the last column right of the fence",
            format!("{LRM}\x1b[?7l\x1b[3;20H\u{4e2d}"),
            Pinned(NOWRAP_WIDE),
        ),
        f(
            "full: ?7l a write into the last column",
            "\x1b[?7l\x1b[3;18HPQRS".into(),
            Pinned(NOWRAP_FLAG),
        ),
        f(
            "corner: a wide character two before the edge",
            format!("{LRM}\x1b[3;19H\u{4e2d}"),
            Agree,
        ),
        f(
            "corner: CUP right + 1 writes there",
            format!("{LRM}\x1b[3;11HAB"),
            Agree,
        ),
        f(
            "corner: a fill to the margin wraps first",
            format!("{LRM}\x1b[3;5HABCDEFX"),
            Agree,
        ),
        f(
            "corner: ?7l right of the fence overwrites the last column",
            format!("{LRM}\x1b[?7l\x1b[3;18HPQRS"),
            Pinned(NOWRAP_FLAG),
        ),
        f(
            "corner: ?7l CUP right + 1 writes there",
            format!("{LRM}\x1b[?7l\x1b[3;11HPQ"),
            Agree,
        ),
        f(
            "corner: VPA keeps a cursor placed at right + 1",
            format!("{LRM}\x1b[1;11H\x1b[3dQ"),
            Agree,
        ),
        f(
            "corner: CHA to right + 1",
            format!("{LRM}\x1b[3;1H\x1b[11GQ"),
            Agree,
        ),
        f(
            "corner: BS from right + 2 lands placed",
            format!("{LRM}\x1b[3;12H\x08Q"),
            Agree,
        ),
        f(
            "corner: BS from CUP right + 1",
            format!("{LRM}\x1b[3;11H\x08{PROBE}"),
            Agree,
        ),
        f(
            "corner: CUB from right of the fence lands placed",
            format!("{LRM}\x1b[3;13H\x1b[2DQ"),
            Agree,
        ),
        f(
            "corner: HT from CUP right + 1",
            format!("{LRM}\x1b[3;11H\t{PROBE}"),
            Pinned(HT_PAST_RIGHT),
        ),
        f(
            "corner: DECRC at CUP right + 1",
            format!("{LRM}\x1b[3;11H\x1b7\x1b[1;1H\x1b8A"),
            Agree,
        ),
        f(
            "corner: DECRC at a pending wrap under the fence",
            format!("{LRM}\x1b[3;5HABCDEF\x1b7\x1b[1;1H\x1b8X"),
            Agree,
        ),
        f(
            "corner: ICH right of the fence",
            format!("{LRM}\x1b[3;15H\x1b[2@"),
            Agree,
        ),
        f(
            "corner: DCH right of the fence",
            format!("{LRM}\x1b[3;15H\x1b[2P"),
            Agree,
        ),
        f(
            "corner: ?69l leaves a placed cursor's column",
            format!("{LRM}\x1b[3;11H\x1b[?69lQ"),
            Agree,
        ),
        // ---- CUU, CUD, CNL and CPL under top and bottom margins ------------
        // STD 070 CUU and CUD, notes 1 and 2 (pp. 5-41, 5-43): stopped at a
        // margin from at or inside it, DECOM reset; ghostty's `cursorUp` and
        // `cursorDown` (Terminal.zig 1849-1878) are the same test.
        f(
            "tb: CUU inside the region stops at the top margin",
            format!("{TB}\x1b[4;3H\x1b[99AX"),
            Agree,
        ),
        f(
            "tb: CUU on the top margin stays",
            format!("{TB}\x1b[2;3H\x1b[AX"),
            Agree,
        ),
        f(
            "tb: CUU from below the region stops at the top margin",
            format!("{TB}\x1b[8;3H\x1b[99AX"),
            Agree,
        ),
        f(
            "tb: CUU above the region goes to the first line",
            "\x1b[3;6r\x1b[2;3H\x1b[99AX".into(),
            Agree,
        ),
        f(
            "tb: CUD inside the region stops at the bottom margin",
            format!("{TB}\x1b[3;3H\x1b[99BX"),
            Agree,
        ),
        f(
            "tb: CUD on the bottom margin stays",
            format!("{TB}\x1b[6;3H\x1b[BX"),
            Agree,
        ),
        f(
            "tb: CUD from above the region stops at the bottom margin",
            format!("{TB}\x1b[1;3H\x1b[99BX"),
            Agree,
        ),
        f(
            "tb: CUD below the region goes to the last line",
            format!("{TB}\x1b[7;3H\x1b[99BX"),
            Agree,
        ),
        f(
            "tb: CUU and CUD by a count inside the region",
            format!("{TB}\x1b[5;3H\x1b[2AX\x1b[2;3H\x1b[2BX"),
            Agree,
        ),
        f(
            "tb: CNL stops at the bottom margin",
            format!("{TB}\x1b[3;7H\x1b[99EX"),
            Agree,
        ),
        f(
            "tb: CPL stops at the top margin",
            format!("{TB}\x1b[5;7H\x1b[99FX"),
            Agree,
        ),
        f(
            "tb: CNL below the region goes to the last line",
            format!("{TB}\x1b[7;7H\x1b[99EX"),
            Agree,
        ),
        f(
            "tb: CNL at the bottom margin starts at the left margin",
            format!("{TB}{LRM}\x1b[3;7H\x1b[99EX"),
            Agree,
        ),
        f(
            "tb: CPL from below starts at the left margin",
            format!("{TB}{LRM}\x1b[8;7H\x1b[99FX"),
            Agree,
        ),
        // ---- a pending wrap: what ends it, what keeps it ---------------------
        // STD 070 D.6.1 (pp. D-13 and D-14): the controls that move the
        // active position reset the Last Column Flag, DECSC saves it and
        // DECRC restores it. Each control at a full row's wrap has a twin
        // at a wrap `?69l` kept on column 10, which must read the same way.
        f("wrap: ?69l keeps it on the margin", KEPT.into(), Agree),
        f(
            "wrap: ?69l, then a character wraps",
            format!("{KEPT}X"),
            Agree,
        ),
        f(
            "wrap: DECSC and DECRC keep it",
            format!("{FULL}\x1b7\x1b[1;1H\x1b8X"),
            Agree,
        ),
        f(
            "wrap: ?1049 keeps it",
            format!("{FULL}\x1b[?1049h\x1b[?1049lX"),
            Agree,
        ),
        f(
            "wrap: DECRC after ?69l keeps it on the margin",
            format!("{LRM}\x1b[3;5HABCDEF\x1b7\x1b[?69l\x1b[1;1H\x1b8X"),
            Agree,
        ),
        f(
            "wrap: DECRC leaves DECAWM",
            format!("\x1b[?7l\x1b7\x1b[?7h\x1b8{FULL}X"),
            Agree,
        ),
        f("wrap: BS", format!("{FULL}\x08X"), Agree),
        f("wrap: BS, kept", format!("{KEPT}\x08X"), Agree),
        f(
            "wrap: BS at the fence",
            format!("{LRM}\x1b[3;5HABCDEF\x08X"),
            Agree,
        ),
        f("wrap: CUB 2", format!("{FULL}\x1b[2DX"), Agree),
        f("wrap: CUB 2, kept", format!("{KEPT}\x1b[2DX"), Agree),
        f(
            "wrap: CUB 2 at the fence",
            format!("{LRM}\x1b[3;5HABCDEF\x1b[2DX"),
            Agree,
        ),
        f("wrap: ?45 BS", format!("\x1b[?45h{FULL}\x08X"), Agree),
        f("wrap: EL 0", format!("{FULL}\x1b[KX"), Agree),
        f("wrap: EL 0, kept", format!("{KEPT}\x1b[KX"), Agree),
        f(
            "wrap: EL 0 at the fence",
            format!("{LRM}\x1b[3;5HABCDEF\x1b[KX"),
            Agree,
        ),
        f("wrap: ECH", format!("{FULL}\x1b[XX"), Agree),
        f("wrap: ED 0", format!("{FULL}\x1b[JX"), Agree),
        f("wrap: EL 1", format!("{FULL}\x1b[1KX"), Agree),
        f("wrap: EL 2", format!("{FULL}\x1b[2KX"), Agree),
        f("wrap: ED 1", format!("{FULL}\x1b[1JX"), Agree),
        f("wrap: ED 2", format!("{FULL}\x1b[2JX"), Agree),
        f("wrap: ED 2, kept", format!("{KEPT}\x1b[2JX"), Agree),
        f("wrap: CUU", format!("{FULL}\x1b[AX"), Agree),
        f("wrap: CUU, kept", format!("{KEPT}\x1b[AX"), Agree),
        f("wrap: CUD", format!("{FULL}\x1b[BX"), Agree),
        f(
            "wrap: CUF from a kept wrap",
            "\x1b[?69h\x1b[5;8s\x1b[3;5HABCD\x1b[?69l\x1b[CX".into(),
            Agree,
        ),
        f(
            "wrap: SGR and DSR keep it, kept",
            format!("{KEPT}\x1b[1m\x1b[5n\x1b[mX"),
            Agree,
        ),
        f("wrap: LF, kept", format!("{KEPT}\nZ"), Agree),
        f("wrap: ICH", format!("{FULL}\x1b[@X"), Agree),
        f("wrap: DCH", format!("{FULL}\x1b[PX"), Agree),
        // LF, VT, FF, IND and RI end it on its column, and ICH and DCH edit
        // that column, where D.6.1 lists each (p. D-14).
        f("wrap: VT", format!("{FULL}\x0bX"), Agree),
        f("wrap: FF", format!("{FULL}\x0cX"), Agree),
        f("wrap: IND", format!("{FULL}\x1bDX"), Agree),
        f("wrap: RI", format!("{FULL}\x1bMX"), Agree),
        f(
            "wrap: LF on the bottom margin",
            "\x1b[8;1Habcdefghijklmnopqrst\nX".into(),
            Agree,
        ),
        f(
            "wrap: RI on the top margin",
            "\x1b[1;1Habcdefghijklmnopqrst\x1bMX".into(),
            Pinned(RI_TOP),
        ),
        f("wrap: VT, kept", format!("{KEPT}\x0bX"), Agree),
        f("wrap: RI, kept", format!("{KEPT}\x1bMX"), Agree),
        f("wrap: ICH, kept", format!("{KEPT}\x1b[@X"), Agree),
        f("wrap: DCH, kept", format!("{KEPT}\x1b[PX"), Agree),
        f(
            "wrap: ICH at the fence",
            format!("{LRM}\x1b[3;5HABCDEF\x1b[@X"),
            Agree,
        ),
        f(
            "wrap: DCH at the fence",
            format!("{LRM}\x1b[3;5HABCDEF\x1b[PX"),
            Agree,
        ),
        // Right of the side margins both are ignored and the wrap waits on.
        f(
            "wrap: ICH right of the fence",
            format!("{LRM}\x1b[3;15HABCDEF\x1b[@X"),
            Pinned(ICH_IGNORED),
        ),
        f(
            "wrap: DCH right of the fence",
            format!("{LRM}\x1b[3;15HABCDEF\x1b[PX"),
            Agree,
        ),
        // On a row outside the top and bottom margins both act and end it,
        // STD 070's margin table (p. 5-24) giving each "Continues to work
        // outside these margins" for them.
        f(
            "wrap: ICH above the region",
            format!("\x1b[4;7r{FULL}\x1b[@X"),
            Agree,
        ),
        f(
            "wrap: DCH above the region",
            format!("\x1b[4;7r{FULL}\x1b[PX"),
            Agree,
        ),
        f(
            "wrap: ICH below the region",
            format!("\x1b[1;2r{FULL}\x1b[@X"),
            Agree,
        ),
        f(
            "wrap: DCH below the region",
            format!("\x1b[1;2r{FULL}\x1b[PX"),
            Agree,
        ),
        // A reset of DECAWM ends it (D.6.1, 070-5 revision item 16); a set
        // keeps it, and IRM and a SUB with no mark drawn keep it too.
        f(
            "wrap: ?7l then ?7h",
            format!("{FULL}\x1b[?7l\x1b[?7hX"),
            Pinned(AWM_RESET),
        ),
        f("wrap: ?7l", format!("{FULL}\x1b[?7lX"), Pinned(NOWRAP_FLAG)),
        f("wrap: ?7h keeps it", format!("{FULL}\x1b[?7hX"), Agree),
        f("wrap: IRM keeps it", format!("{FULL}\x1b[4hX"), Agree),
        f("wrap: SUB keeps it", format!("{FULL}\x1aX"), Agree),
        // ---- a pending wrap through a resize --------------------------------
        // No text governs a resize; ghostty keeps its flag through every one,
        // and xterm's ScreenResize never touches do_wrap.
        r(
            "resize: the same size keeps it",
            vec![bytes(FULL), Step::Resize(8, 20), bytes("X")],
        ),
        r(
            "resize: taller keeps it",
            vec![bytes(FULL), Step::Resize(10, 20), bytes("X")],
        ),
        r(
            "resize: shorter keeps it",
            vec![bytes(FULL), Step::Resize(6, 20), bytes("X")],
        ),
        r(
            "resize: the primary re-cut onto the last column",
            vec![bytes(FULL), Step::Resize(8, 10), bytes("X")],
        ),
        r(
            "resize: the primary re-cut mid-row",
            vec![bytes(FULL), Step::Resize(8, 15), bytes("X")],
        ),
        r(
            "resize: the primary widened",
            vec![bytes(FULL), Step::Resize(8, 30), bytes("X")],
        ),
        r(
            "resize: the alternate screen narrowed",
            vec![
                bytes(&format!("\x1b[?1049h{FULL}")),
                Step::Resize(8, 15),
                bytes("X"),
            ],
        ),
        r(
            "resize: the alternate screen widened",
            vec![
                bytes(&format!("\x1b[?1049h{FULL}")),
                Step::Resize(8, 30),
                bytes("X"),
            ],
        ),
        r(
            "resize: ?69l's kept wrap, widened",
            vec![bytes(KEPT), Step::Resize(8, 30), bytes("X")],
        ),
        // U+FE0F widens its base with ?2027 set, which ghostty needs and
        // baud does under either state: mid-row, the control, and on the
        // character a re-cut left the wrap waiting after.
        Fixture {
            name: "resize: U+FE0F mid-row widens in place",
            steps: vec![bytes("\x1b[?2027h\x1b[3;1Habcd\u{2764}\u{fe0f}X")],
            verdict: Agree,
        },
        Fixture {
            name: "resize: U+FE0F after the re-cut widens in place",
            steps: vec![
                bytes("\x1b[?2027h\x1b[3;1Habcdefghijklmnopqrs\u{2764}"),
                Step::Resize(8, 15),
                bytes("\u{fe0f}X"),
            ],
            verdict: Pinned(FE0F_KEPT),
        },
        r(
            "resize: a DECSC save re-cut",
            vec![
                bytes(&format!("{FULL}\x1b7\x1b[1;1H")),
                Step::Resize(8, 10),
                bytes("\x1b8X"),
            ],
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
        let whole = fx
            .steps
            .iter()
            .map(|s| match s {
                Step::Bytes(b) => b.len(),
                Step::Resize(..) => 0,
            })
            .max()
            .unwrap_or(0);
        let b = baud_state(&fx.steps, whole);
        let g = ghostty_state(&fx.steps, whole);
        // Chunk invariance first: a state that depends on the reads is a
        // finding in itself, and comparing it across terminals would hide it.
        let mut split = None;
        for read in 1..whole {
            if baud_state(&fx.steps, read) != b {
                split = Some(("baud", read));
                break;
            }
            if ghostty_state(&fx.steps, read) != g {
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
        let a = baud_state(&filled(""), usize::MAX);
        let b = baud_state(&filled("\x1b[1;1HZ"), usize::MAX);
        assert_ne!(a, b);
        let a = ghostty_state(&filled(""), usize::MAX);
        let b = ghostty_state(&filled("\x1b[1;1HZ"), usize::MAX);
        assert_ne!(a, b);
    }

    fn filled(tail: &str) -> Vec<Step> {
        vec![Step::Bytes(on_fill(tail))]
    }

    /// **A resize is a step on both sides**: the geometry read back is the
    /// one it leaves, and a write after it lands on the wider row.
    #[test]
    fn a_resize_is_seen() {
        let steps = [Step::Resize(8, 30), Step::Bytes(b"\x1b[1;25HZ".to_vec())];
        let (b, g) = (
            baud_state(&steps, usize::MAX),
            ghostty_state(&steps, usize::MAX),
        );
        assert_eq!(b.screen[0].len(), 30);
        assert_eq!(&b.screen[0][24..25], "Z");
        assert_eq!(b, g);
    }

    /// **The two read the same fill the same way**: the control every other
    /// comparison stands on.
    #[test]
    fn the_fill_agrees() {
        let steps = filled("");
        assert_eq!(
            baud_state(&steps, usize::MAX),
            ghostty_state(&steps, usize::MAX)
        );
    }

    /// The pending wrap is compared, not assumed: a full row then nothing
    /// sets it on both sides, and a CR clears it on both.
    #[test]
    fn the_pending_wrap_is_compared() {
        let full = filled("");
        assert!(baud_state(&full, usize::MAX).pending);
        assert!(ghostty_state(&full, usize::MAX).pending);
        let cr = filled("\r");
        assert!(!baud_state(&cr, usize::MAX).pending);
        assert!(!ghostty_state(&cr, usize::MAX).pending);
    }
}
