//! Differential test against sfte, a second implementation.
//!
//! Feeds identical bytes to `baud-core` and to sfte -- a single-file C11
//! terminal emulator, vendored whole at a pinned commit -- and compares the
//! visible grid, the cursor and the reply bytes **at every 4096-byte read
//! boundary**, not only at the end.
//!
//! **Why every boundary.** A final-screen differential is structurally blind to
//! anything that scrolls off: `git-log.raw`'s three U+2705 sit at 17.5% through
//! the file, and by the last frame they are thousands of lines gone. A probe
//! that compared final screens measured "0 of 10,000 cells differ on six of
//! nine recordings" and that number said nothing about width. This is the
//! probe-that-cannot-fire failure `CLAUDE.md` records seven times, arriving as
//! a comparison point rather than a gate; comparing at each read is the same
//! instrument `baud-render/tests/incremental.rs` uses for the damage path.
//!
//! **Why sfte.** Since `baud-core2` was deleted, `baud-difftest` against
//! alacritty has been the only thing in this repository that can disagree with
//! `baud-core` from outside it. This is the second, and it checks two axes that
//! one does not: the **cursor** and the **replies**.
//!
//! **What it is worth, stated before the design.** sfte's confirmed defect list
//! is long (HT lands one past every stop; `CSI < u` performs SCORC; OSC 52's
//! payload pointer is one byte early; ...), so when the two disagree, baud is
//! not the likely culprit, and a disagreement is pinned with its reason rather
//! than blocking anything. What it is worth is an independent second opinion
//! on the VT core over every recording in the corpus, small enough that a
//! disagreement is diagnosable in minutes.
//!
//! **Nothing is excluded.** Every recording in `tests/recordings/` runs; what
//! differs is pinned at its count with the reason beside it. One sfte defect
//! -- a BEL-terminated OSC wedging its parser until the next `ESC`, the
//! commonest window-title spelling in the wild -- is fixed by a build-time
//! patch (see `patches/`) rather than pinned, and the `BEL-terminated OSC`
//! fixture is the live control that the patch is applied: with it removed,
//! five cells and the cursor diverge. The corpus itself could not show the
//! defect -- `cava.raw`'s one such OSC swallows only its own title -- which
//! is why the fixture exists. The patch lets a BEL end an OSC and nothing
//! else, because an OSC is the one string a BEL ends; the four `BEL in ...`
//! fixtures, a BEL inside a DCS, a DECRQSS, an APC and a PM, are what hold it
//! there.
use baud_core::*;
use std::ffi::CStr;

pub mod ffi;

/// The geometry both sides are measured at, and the single place it is
/// written. sfte's side is set at **compile time** in `sfte_shim.c`, because
/// `sfte_resize` takes pixels and divides by a font's cell width -- which is 0
/// with no font loaded, undefined behaviour that reads 200x50 at -O2 and 1x1 at
/// -O0. `sfte_shim_new` returns NULL at any other geometry, and `Sfte::new`
/// checks the shim's answer against this one.
pub const WS: WinSize = WinSize {
    ws_row: 50,
    ws_col: 200,
};
pub fn cols() -> usize {
    usize::from(WS.ws_col)
}
pub fn rows() -> usize {
    usize::from(WS.ws_row)
}

/// Whether a cell's background is the default or one an SGR set. A coloured
/// run that wraps, or an erase in a colour, must colour the same cells on both
/// sides, and a character-only comparison cannot see a coloured blank.
///
/// **One-directional on sfte's side, and said so.** sfte encodes the default
/// background as the colour 0, so `SGR 48;2;0;0;0` is unrepresentable there
/// and reads as `Default`. The fixtures avoid explicit black.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bg {
    Default,
    Set,
}

/// One cell as both sides are compared. `'\0'` is the second half of a wide
/// character on both sides -- see `Normalised`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: char,
    pub bg: Bg,
}

/// Everything compared at one read boundary.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Screen {
    pub cells: Vec<Cell>,
    pub cursor: (Row, Col),
    pub reply: Vec<u8>,
}

impl Screen {
    /// One number for the whole screen, so a harness that times an arm can
    /// say at the end of every rep whether the two arms drew the same thing
    /// -- the admissibility gate a throughput row needs and sfte cannot
    /// supply from a `cells_written` it does not have. FNV-1a over every
    /// cell's character and background, the cursor and the reply bytes.
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |b: u8| {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        for c in &self.cells {
            for b in u32::from(c.ch).to_le_bytes() {
                mix(b);
            }
            mix(match c.bg {
                Bg::Default => 0,
                Bg::Set => 1,
            });
        }
        for b in self.cursor.0.index().to_le_bytes() {
            mix(b);
        }
        for b in self.cursor.1.index().to_le_bytes() {
            mix(b);
        }
        for &b in &self.reply {
            mix(b);
        }
        h
    }
}

/// How many cells one normalisation touched.
///
/// A newtype rather than a bare `usize`, which is what the audit refused in
/// `baud-difftest` and was right to: these count cells, and a primitive named
/// `blanks` can be added to anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct Cells(usize);

impl Cells {
    pub const NONE: Cells = Cells(0);
    pub fn of(n: usize) -> Cells {
        Cells(n)
    }
    pub fn any(self) -> bool {
        self.0 != 0
    }
    pub fn bump(&mut self) {
        self.0 += 1;
    }
    pub fn get(self) -> usize {
        self.0
    }
}

/// A read boundary's ordinal, or a count of them. The same shape as `Cells`,
/// and for the same reason: a boundary count named by a bare integer can be
/// compared against a cell count, and the two pins below hold one of each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Boundary(usize);

impl Boundary {
    pub const NONE: Boundary = Boundary(0);
    pub fn of(n: usize) -> Boundary {
        Boundary(n)
    }
    pub fn bump(&mut self) {
        self.0 += 1;
    }
    pub fn get(self) -> usize {
        self.0
    }
}

/// How many cells each normalisation touched, over a whole run.
///
/// **Each of these cancels a representation difference that draws nothing, and
/// each is counted**, so one that starts firing somewhere new says so instead
/// of quietly agreeing. All four are derived from sfte's own source, not from
/// assuming the two agree:
///
/// * `tails` -- **the second half of a wide character, keyed on the flag and
///   never on equality.** sfte stamps the *lead rune again* into the tail cell
///   and marks it `_SFTE_ATTR_DUMMY` (sfte_dev.h:2851); baud stores
///   `Content::WIDE_TAIL`, a reserved value above U+10FFFF. A glyph-only read
///   would see the character twice and call it a match, which is more
///   dangerous than alacritty's spacer, which at least stores a space.
/// * `blanks` -- **the blank fold, and it is a blindness.** `sfte_init` memsets
///   the grid to 0 and its clear-cells routine writes `' '` (sfte_dev.h:3290),
///   so after the first erase the two are indistinguishable *within sfte*.
///   Folding sfte's `{0, ' '}` against baud's `{EMPTY, ' '}` gives up exactly the
///   trailing-whitespace and selection fidelity baud's reserved sentinel exists
///   to protect. It is counted on sfte's side only where the cell holds 0.
///
/// The foreground fold (`{0, 0xFFFFFF}` both resolve to the default) and the
/// background's one-directionality are not counters: the comparison does not
/// read the foreground at all, and `Bg` folds by construction.
#[derive(Default)]
pub struct Normalised {
    pub tails: Cells,
    pub blanks: Cells,
}

impl Normalised {
    pub fn new() -> Normalised {
        Normalised {
            tails: Cells::NONE,
            blanks: Cells::NONE,
        }
    }
}

/// `baud-core`, driven a chunk at a time and read at each boundary.
pub struct Ours<'a> {
    t: Term<'a>,
}

impl<'a> Ours<'a> {
    pub fn new(owned: &'a mut Owned) -> Ours<'a> {
        Ours {
            t: Term::new(owned.storage(), &WS),
        }
    }
    pub fn feed(&mut self, chunk: &[u8]) {
        self.t.write_at(BytePos::ZERO, chunk);
        self.t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
    }
    pub fn screen(&mut self) -> Screen {
        let mut cells = Vec::with_capacity(rows() * cols());
        for y in 0..rows() {
            for c in self.t.row(Row::from_index(y)).expect("a row on the grid") {
                let ch = c
                    .content
                    .as_codepoint()
                    .and_then(|p| char::from_u32(p.value()))
                    .unwrap_or(if c.content.is_wide_tail() { '\0' } else { ' ' });
                let bg = if self.t.style_of(*c).bg().is_default() {
                    Bg::Default
                } else {
                    Bg::Set
                };
                cells.push(Cell { ch, bg });
            }
        }
        let cursor = self.t.cursor();
        let reply = self.t.reply().to_vec();
        self.t.clear_reply();
        Screen {
            cells,
            cursor,
            reply,
        }
    }
}

/// sfte, through the shim.
pub struct Sfte {
    pub s: ffi::Shim,
}

impl Sfte {
    pub fn new() -> Sfte {
        // SAFETY: the shim's functions take an opaque handle it allocated and
        // read only through sfte's own accessors; every call below hands back
        // the handle `sfte_shim_new` returned, and `Drop` frees it exactly once.
        let s = unsafe { ffi::sfte_shim_new() };
        assert!(
            !s.is_null(),
            "sfte_shim_new refused: sfte failed to initialise, or came up at a \
             geometry other than 200x50 -- the compile-time geometry did not take"
        );
        let (c, r) = unsafe { (ffi::sfte_shim_cols(s), ffi::sfte_shim_rows(s)) };
        assert!(
            usize::try_from(c) == Ok(cols()) && usize::try_from(r) == Ok(rows()),
            "sfte is {c}x{r}, expected {}x{}",
            cols(),
            rows()
        );
        Sfte { s }
    }
    pub fn feed(&mut self, chunk: &[u8]) {
        unsafe { ffi::sfte_shim_feed(self.s, chunk.as_ptr(), chunk.len()) }
    }
    pub fn screen(&mut self, n: &mut Normalised) -> Screen {
        let mut cells = Vec::with_capacity(rows() * cols());
        for y in 0..rows() {
            let row = i16::try_from(y).unwrap();
            for x in 0..cols() {
                let col = i16::try_from(x).unwrap();
                let (rune, tail, bgd) = unsafe {
                    (
                        ffi::sfte_shim_rune(self.s, col, row),
                        ffi::sfte_shim_is_wide_tail(self.s, col, row),
                        ffi::sfte_shim_bg_is_default(self.s, col, row),
                    )
                };
                let ch = if tail != 0 {
                    n.tails.bump();
                    '\0'
                } else if rune == 0 {
                    n.blanks.bump();
                    ' '
                } else {
                    char::from_u32(rune).unwrap_or('\u{FFFD}')
                };
                let bg = if bgd != 0 { Bg::Default } else { Bg::Set };
                cells.push(Cell { ch, bg });
            }
        }
        let (r, c) = unsafe {
            (
                ffi::sfte_shim_cursor_row(self.s),
                ffi::sfte_shim_cursor_col(self.s),
            )
        };
        let reply = unsafe {
            let len = ffi::sfte_shim_reply_len(self.s);
            let dropped = ffi::sfte_shim_reply_dropped(self.s);
            assert_eq!(dropped, 0, "sfte reply buffer overflowed");
            std::slice::from_raw_parts(ffi::sfte_shim_reply(self.s), len).to_vec()
        };
        unsafe { ffi::sfte_shim_clear_reply(self.s) };
        // Clamped the way `Term::cursor` clamps: sfte's cursor_col can sit
        // one past the last column after a write at the margin.
        Screen {
            cells,
            cursor: (
                Row::from_index(usize::try_from(r).unwrap_or(0)),
                Col::at_index(usize::try_from(c).unwrap_or(0).min(cols() - 1)),
            ),
            reply,
        }
    }
}

impl Default for Sfte {
    fn default() -> Sfte {
        Sfte::new()
    }
}

impl Drop for Sfte {
    fn drop(&mut self) {
        unsafe { ffi::sfte_shim_free(self.s) }
    }
}

/// Set sfte's locale and refuse anything but the table the comparison is
/// written against. **An older host falls back to `C` silently**, where every
/// non-ASCII codepoint is width 1, and a comparison taken there is a different
/// program's.
///
/// **Once per process, under a lock, because `setlocale` is not thread-safe.**
/// It returns a pointer into libc's static storage, and five tests calling it
/// concurrently read each other's writes: the assert below fired once in
/// three runs with the right locale set, which is a race and not a flake.
/// The first caller sets and checks; every later caller gets the stored
/// answer without touching libc again.
pub fn set_locale() -> String {
    static LOCALE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LOCALE
        .get_or_init(|| {
            let got = unsafe { CStr::from_ptr(ffi::sfte_shim_set_locale()) }
                .to_string_lossy()
                .into_owned();
            assert_eq!(
                got, "C.UTF-8",
                "sfte's locale is {got:?}, not C.UTF-8: its wcwidth would answer a \
                 different width table and every count below would be for a different program"
            );
            got
        })
        .clone()
}
