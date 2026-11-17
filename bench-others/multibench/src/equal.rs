//! **A rate only where baud-core and ghostty end on the same screen.**
//!
//! Before a recording is timed, it is fed once through both, from memory
//! in the reads the rates use and to the same total, and both final
//! screens are read back:
//!
//! * every cell: blank, a wide character's tail, or its base character,
//!   read as `ghostty-oracle` reads them;
//! * every kitty placement anchored on the screen: its cell, its `z`, and
//!   its image's size and pixels;
//! * every virtual placement (`U=1`) either arm names: by its client id,
//!   from ghostty's placements and from the image ids baud-core's
//!   placeholder cells carry, each looked up on both arms.
//!
//! Pixels are read as `0xAARRGGBB` words and hashed. An image in a format
//! this does not read is compared by its size, and the report says how
//! many were. A recording whose screens differ is not timed, prints no
//! rate, and fails the run.
//!
//! The counters printed beside the rates are each arm's own, from this
//! rep; a counter an arm does not report is "n/a".

// Without libghostty-vt there is no second arm to compare, so `check`
// returns nothing and what it would build goes unused.
#![cfg_attr(not(have_ghostty), allow(dead_code))]

#[cfg(have_ghostty)]
use crate::transport::through_memory;
use crate::{COLS, ROWS};

/// How many differing cells the report names.
const SHOWN: usize = 5;

/// One cell's character, as both arms are read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Glyph {
    Blank,
    WideTail,
    Char(u32),
}

impl std::fmt::Display for Glyph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Glyph::Blank => write!(f, "blank"),
            Glyph::WideTail => write!(f, "wide tail"),
            Glyph::Char(cp) => match char::from_u32(*cp).filter(|c| !c.is_control()) {
                Some(c) => write!(f, "U+{cp:04X} {c:?}"),
                None => write!(f, "U+{cp:04X}"),
            },
        }
    }
}

/// An image as an arm reports it: its size, and a hash of its pixels as
/// `0xAARRGGBB` words, `None` where the arm's format is not one read here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Picture {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) pixels: Option<u64>,
}

impl Picture {
    /// The same size, and the same pixels where both were read.
    fn same(self, other: Picture) -> bool {
        self.width == other.width
            && self.height == other.height
            && match (self.pixels, other.pixels) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
    }

    fn unread(self) -> bool {
        self.pixels.is_none()
    }
}

impl std::fmt::Display for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.pixels {
            Some(h) => write!(f, "{}x{} #{h:016x}", self.width, self.height),
            None => write!(f, "{}x{} (pixels not read)", self.width, self.height),
        }
    }
}

/// A placement anchored on the screen: its anchor cell, its `z` and its
/// image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Placed {
    pub(crate) row: usize,
    pub(crate) col: usize,
    pub(crate) z: i32,
    pub(crate) picture: Picture,
}

impl std::fmt::Display for Placed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "row {} col {} z {}: {}",
            self.row, self.col, self.z, self.picture
        )
    }
}

/// A cell the arms disagree on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CellDiff {
    pub(crate) row: usize,
    pub(crate) col: usize,
    pub(crate) baud: Glyph,
    pub(crate) ghostty: Glyph,
}

/// A virtual placement the arms disagree on, by its image's client id:
/// the image each arm shows for it, `None` where it has none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct VirtualDiff {
    pub(crate) image: u32,
    pub(crate) baud: Option<Picture>,
    pub(crate) ghostty: Option<Picture>,
}

/// What one arm shows: the cells row by row, the placements anchored on
/// the screen sorted by cell and `z`, and the image ids of its virtual
/// placements as it names them.
struct Shown {
    cells: Vec<Glyph>,
    placed: Vec<Placed>,
    virtual_ids: Vec<u32>,
}

impl Shown {
    fn sort(mut self) -> Shown {
        self.placed
            .sort_by_key(|p| (p.row, p.col, p.z, p.picture.width, p.picture.height));
        self
    }
}

/// baud-core's own image counters from the check's rep.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct BaudCounts {
    /// Kitty images stored ([`baud_core::Term::kitty`]).
    pub(crate) stored: usize,
    /// Kitty images deleted to make room for a new one.
    pub(crate) evicted: usize,
    /// Images refused, whatever the cause.
    pub(crate) refused: usize,
}

/// The two arms' final screens, compared.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Compared {
    /// The first [`SHOWN`] differing cells, top to bottom.
    pub(crate) cells: Vec<CellDiff>,
    pub(crate) cells_differing: usize,
    /// baud-core's and ghostty's placements anchored on the screen.
    pub(crate) placed: [Vec<Placed>; 2],
    /// The virtual placements that differ.
    pub(crate) virtuals: Vec<VirtualDiff>,
    /// Virtual placements either arm has, each compared.
    pub(crate) virtuals_compared: usize,
    /// Images compared by size alone, their pixels in a format not read.
    pub(crate) pixels_unread: usize,
    pub(crate) baud: BaudCounts,
}

impl Compared {
    /// Whether the placements anchored on the screen differ.
    pub(crate) fn placements_differ(&self) -> bool {
        let [ours, theirs] = &self.placed;
        ours.len() != theirs.len()
            || ours.iter().zip(theirs).any(|(a, b)| {
                (a.row, a.col, a.z) != (b.row, b.col, b.z) || !a.picture.same(b.picture)
            })
    }

    /// Whether both arms show the same screen.
    pub(crate) fn equal(&self) -> bool {
        self.cells_differing == 0 && !self.placements_differ() && self.virtuals.is_empty()
    }

    /// The report printed under the table for the recording `label`.
    pub(crate) fn report(&self, label: &str) -> Vec<String> {
        let mut out = Vec::new();
        let [ours, theirs] = &self.placed;
        let unread = match self.pixels_unread {
            0 => String::new(),
            n => format!("; {n} images compared by size, their pixels not read"),
        };
        if self.equal() {
            out.push(format!(
                "{label}: baud-core and ghostty show the same screen: {ROWS}x{COLS} cells, \
                 {} placements on the screen, {} virtual placements{unread}",
                ours.len(),
                self.virtuals_compared
            ));
        } else {
            out.push(format!(
                "{label}: SCREENS DIFFER, no rate: baud-core against ghostty{unread}"
            ));
            if self.cells_differing > 0 {
                out.push(format!(
                    "  {} of {} cells differ; the first {}:",
                    self.cells_differing,
                    ROWS * COLS,
                    self.cells.len()
                ));
                for d in &self.cells {
                    out.push(format!(
                        "    row {} col {}: baud-core {}, ghostty {}",
                        d.row, d.col, d.baud, d.ghostty
                    ));
                }
            }
            if self.placements_differ() {
                for (name, list) in [("baud-core", ours), ("ghostty", theirs)] {
                    out.push(format!("  {name}: {} placements on the screen", list.len()));
                    for p in list.iter().take(SHOWN) {
                        out.push(format!("    {p}"));
                    }
                }
            }
            let none = |p: Option<Picture>| p.map_or("none".to_string(), |p| p.to_string());
            for v in &self.virtuals {
                out.push(format!(
                    "  virtual placement of image {}: baud-core {}, ghostty {}",
                    v.image,
                    none(v.baud),
                    none(v.ghostty)
                ));
            }
        }
        out
    }

    /// `arm`'s counters from the check's rep, as printed beside its rates:
    /// "n/a" for each one the arm does not report. baud-core reports no
    /// count of bytes decoded, and libghostty-vt's image storage reports
    /// none of the four.
    pub(crate) fn counters(&self, arm: Arm) -> String {
        match arm {
            Arm::Baud => format!(
                "images stored {}, bytes decoded n/a, evicted {}, refused {}",
                self.baud.stored, self.baud.evicted, self.baud.refused
            ),
            Arm::Ghostty => {
                "images stored n/a, bytes decoded n/a, evicted n/a, refused n/a".to_string()
            }
        }
    }
}

/// The two arms the screen check compares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Arm {
    Baud,
    Ghostty,
}

/// A hash of pixels as `0xAARRGGBB` words, FNV-1a over each word's bytes
/// little end first.
fn digest(words: impl Iterator<Item = u32>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for w in words {
        for b in w.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Both arms' screens after the whole replays of `p` that fit in `total`
/// bytes (one at least), compared; `None` when libghostty-vt was not linked
/// and there is nothing to compare against. The replay ends at the
/// recording's end and never inside it: a stream cut inside a control
/// string leaves it open, and what each arm shows of an open string is
/// not a screen either has settled on.
#[cfg(have_ghostty)]
pub(crate) fn check(p: &[u8], total: usize) -> Option<Compared> {
    use crate::ffi::ghostty as g;
    let ws = crate::cores::baud_ws();
    let mut owned = crate::cores::baud_owned(&ws);
    let mut t = crate::cores::baud_new(&mut owned, &ws);
    let term = crate::cores::ghostty_new();
    let whole = (total / p.len().max(1)).max(1) * p.len();
    through_memory(p, whole, |chunk| {
        crate::cores::baud_feed(&mut t, chunk);
        // SAFETY: `term` is a live terminal and `chunk` a live slice.
        unsafe { g::ghostty_terminal_vt_write(term, chunk.as_ptr(), chunk.len()) };
    });
    let ours = baud::shown(&t).sort();
    // SAFETY: `term` is live and nothing writes to it while it is read.
    let theirs = unsafe { ghostty::shown(term) }.sort();

    let mut cells = Vec::new();
    let mut cells_differing = 0;
    for (i, (&a, &b)) in ours.cells.iter().zip(&theirs.cells).enumerate() {
        if a != b {
            cells_differing += 1;
            if cells.len() < SHOWN {
                cells.push(CellDiff {
                    row: i / COLS,
                    col: i % COLS,
                    baud: a,
                    ghostty: b,
                });
            }
        }
    }

    let mut ids: Vec<u32> = ours
        .virtual_ids
        .iter()
        .chain(&theirs.virtual_ids)
        .copied()
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let mut virtuals = Vec::new();
    let mut virtuals_compared = 0;
    let mut pixels_unread = 0;
    for &image in &ids {
        let a = baud::virtual_picture(&t, image);
        // SAFETY: as above.
        let b = unsafe { ghostty::virtual_picture(term, &theirs.virtual_ids, image) };
        if a.is_none() && b.is_none() {
            continue;
        }
        virtuals_compared += 1;
        match (a, b) {
            (Some(x), Some(y)) if x.same(y) => {
                pixels_unread += usize::from(x.unread() || y.unread());
            }
            _ => virtuals.push(VirtualDiff {
                image,
                baud: a,
                ghostty: b,
            }),
        }
    }
    pixels_unread += ours
        .placed
        .iter()
        .chain(&theirs.placed)
        .filter(|p| p.picture.unread())
        .count();

    let images = t.images();
    let baud = BaudCounts {
        stored: t.kitty().images,
        evicted: images.images_evicted,
        refused: images.images_refused,
    };
    // SAFETY: `term` came from `ghostty_new` and is not used after this.
    unsafe { g::ghostty_terminal_free(term) };
    Some(Compared {
        cells,
        cells_differing,
        placed: [ours.placed, theirs.placed],
        virtuals,
        virtuals_compared,
        pixels_unread,
        baud,
    })
}

/// With no libghostty-vt there is no second screen.
#[cfg(not(have_ghostty))]
pub(crate) fn check(_: &[u8], _: usize) -> Option<Compared> {
    None
}

/// baud-core's screen, read through its public API.
#[cfg(have_ghostty)]
mod baud {
    use super::{Glyph, Picture, Placed, Shown, digest};
    use crate::{COLS, ROWS};
    use baud_core::Term;
    use baud_domain::{ImageView, KittyId, KittyPlacement, Row, Rows};

    pub(super) fn picture(v: &ImageView<'_>) -> Picture {
        Picture {
            width: v.width.index(),
            height: v.height.index(),
            pixels: Some(digest(v.pixels.iter().map(|p| p.bits()))),
        }
    }

    /// The cells, the kitty placements anchored on the live screen, and
    /// the image ids the placeholder cells on it name.
    pub(super) fn shown(t: &Term<'_>) -> Shown {
        let mut cells = Vec::with_capacity(ROWS * COLS);
        let mut virtual_ids = Vec::new();
        let view = t.viewport();
        for y in 0..ROWS {
            let r = Row::from_index(y);
            let row = &t.row(r).expect("a row on the grid")[..COLS];
            cells.extend(row.iter().map(|c| match c.content.is_wide_tail() {
                true => Glyph::WideTail,
                false => match t.base_of(*c).as_codepoint() {
                    Some(cp) => Glyph::Char(cp.value()),
                    None => Glyph::Blank,
                },
            }));
            virtual_ids.extend(
                t.placeholder_runs(row, view.styles(r))
                    .map(|run| run.image.raw()),
            );
        }
        let store = t.images();
        let origin = t.image_origin(Rows::ZERO);
        let placed = store
            .placements()
            .filter(|(_, s)| s.screen == origin.screen && store.kitty_names(s.image).is_some())
            .filter_map(|(_, s)| {
                let row = s.line.since(origin.top)?.index();
                let col = s.col.index();
                let image = store.image(s.image)?;
                (row < ROWS && col < COLS).then(|| Placed {
                    row,
                    col,
                    z: s.z.raw(),
                    picture: picture(&image),
                })
            })
            .collect();
        Shown {
            cells,
            placed,
            virtual_ids,
        }
    }

    /// The image a virtual placement of client image `id` shows, if any.
    pub(super) fn virtual_picture(t: &Term<'_>, id: u32) -> Option<Picture> {
        let v = t.kitty_virtual(KittyId::of(id), KittyPlacement::NONE)?;
        t.images().image(v.image).map(|i| picture(&i))
    }
}

/// ghostty's screen, read through libghostty-vt's C API.
#[cfg(have_ghostty)]
mod ghostty {
    use super::{Glyph, Picture, Placed, Shown, digest};
    use crate::ffi::ghostty::*;
    use crate::{COLS, ROWS};
    use std::ffi::c_void;

    /// The active screen's image storage, borrowed until the next write.
    ///
    /// # Safety
    /// `t` is a live terminal.
    unsafe fn graphics(t: Terminal) -> Graphics {
        let mut g: Graphics = std::ptr::null_mut();
        // SAFETY: the out-pointer is a `GhosttyKittyGraphics`, as the header
        // names for this datum.
        let rc =
            unsafe { ghostty_terminal_get(t, TERMINAL_DATA_KITTY_GRAPHICS, (&raw mut g).cast()) };
        assert_eq!(rc, 0, "reading ghostty's image storage failed with {rc}");
        g
    }

    /// One datum of `image` into `out`, of the type the header names.
    ///
    /// # Safety
    /// `image` is live and `out` points at that type.
    unsafe fn image_get<T>(image: Image, data: i32, out: &mut T) -> bool {
        // SAFETY: the caller's.
        unsafe {
            ghostty_kitty_graphics_image_get(image, data, (out as *mut T).cast::<c_void>()) == 0
        }
    }

    /// `image`'s size and pixels, each pixel widened to `0xAARRGGBB`.
    ///
    /// # Safety
    /// `image` is live.
    unsafe fn picture(image: Image) -> Picture {
        let (mut w, mut h, mut format) = (0u32, 0u32, -1i32);
        let (mut ptr, mut len): (*const u8, usize) = (std::ptr::null(), 0);
        // SAFETY: each out-pointer is of the type the header names.
        let read = unsafe {
            image_get(image, IMAGE_DATA_WIDTH, &mut w)
                && image_get(image, IMAGE_DATA_HEIGHT, &mut h)
                && image_get(image, IMAGE_DATA_FORMAT, &mut format)
        };
        assert!(read, "reading a ghostty image's size and format failed");
        // SAFETY: as above.
        let data = unsafe {
            image_get(image, IMAGE_DATA_DATA_PTR, &mut ptr)
                && image_get(image, IMAGE_DATA_DATA_LEN, &mut len)
        };
        let (width, height) = (w as usize, h as usize);
        let per = match format {
            IMAGE_FORMAT_RGB => 3,
            IMAGE_FORMAT_RGBA => 4,
            IMAGE_FORMAT_GRAY_ALPHA => 2,
            IMAGE_FORMAT_GRAY => 1,
            _ => 0,
        };
        let pixels =
            (data && !ptr.is_null() && per > 0 && len == width * height * per).then(|| {
                // SAFETY: the library holds `len` bytes at `ptr` until the
                // terminal is next written.
                let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
                digest(bytes.chunks_exact(per).map(|p| {
                    let (r, g, b, a) = match *p {
                        [r, g, b] => (r, g, b, 0xff),
                        [r, g, b, a] => (r, g, b, a),
                        [v, a] => (v, v, v, a),
                        [v] => (v, v, v, 0xff),
                        _ => unreachable!("a pixel is one to four bytes"),
                    };
                    u32::from_be_bytes([a, r, g, b])
                }))
            });
        Picture {
            width,
            height,
            pixels,
        }
    }

    /// The cells, the placements anchored on the screen, and the client
    /// image ids of the virtual placements.
    ///
    /// # Safety
    /// `t` is a live terminal, not written while this runs.
    pub(super) unsafe fn shown(t: Terminal) -> Shown {
        let mut cells = Vec::with_capacity(ROWS * COLS);
        for y in 0..ROWS as u32 {
            for x in 0..COLS as u16 {
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
                // header names; the ref is used before the next write.
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
                cells.push(match (wide, cp) {
                    (CELL_WIDE_SPACER_TAIL, _) => Glyph::WideTail,
                    (_, 0) => Glyph::Blank,
                    (_, c) => Glyph::Char(c),
                });
            }
        }

        let mut placed = Vec::new();
        let mut virtual_ids = Vec::new();
        // SAFETY: `t` is live; the storage, iterator and images are read
        // before the next write, and the iterator is freed here.
        unsafe {
            let g = graphics(t);
            let mut it: Placements = std::ptr::null_mut();
            assert_eq!(
                ghostty_kitty_graphics_placement_iterator_new(std::ptr::null(), &mut it),
                0
            );
            assert_eq!(
                ghostty_kitty_graphics_get(
                    g,
                    GRAPHICS_DATA_PLACEMENT_ITERATOR,
                    (&raw mut it).cast()
                ),
                0
            );
            while ghostty_kitty_graphics_placement_next(it) {
                let (mut id, mut is_virtual, mut z) = (0u32, false, 0i32);
                assert_eq!(
                    ghostty_kitty_graphics_placement_get(
                        it,
                        PLACEMENT_DATA_IMAGE_ID,
                        (&raw mut id).cast()
                    ),
                    0
                );
                assert_eq!(
                    ghostty_kitty_graphics_placement_get(
                        it,
                        PLACEMENT_DATA_IS_VIRTUAL,
                        (&raw mut is_virtual).cast()
                    ),
                    0
                );
                assert_eq!(
                    ghostty_kitty_graphics_placement_get(it, PLACEMENT_DATA_Z, (&raw mut z).cast()),
                    0
                );
                if is_virtual {
                    virtual_ids.push(id);
                    continue;
                }
                let image = ghostty_kitty_graphics_image(g, id);
                if image.is_null() {
                    continue;
                }
                let (mut col, mut row) = (0i32, 0i32);
                if ghostty_kitty_graphics_placement_viewport_pos(it, image, t, &mut col, &mut row)
                    != 0
                {
                    continue;
                }
                let (Ok(row), Ok(col)) = (usize::try_from(row), usize::try_from(col)) else {
                    continue;
                };
                if row < ROWS && col < COLS {
                    placed.push(Placed {
                        row,
                        col,
                        z,
                        picture: picture(image),
                    });
                }
            }
            ghostty_kitty_graphics_placement_iterator_free(it);
        }
        Shown {
            cells,
            placed,
            virtual_ids,
        }
    }

    /// The image a virtual placement of client image `id` shows: none
    /// unless `virtual_ids`, ghostty's own, holds it.
    ///
    /// # Safety
    /// `t` is a live terminal, not written while this runs.
    pub(super) unsafe fn virtual_picture(
        t: Terminal,
        virtual_ids: &[u32],
        id: u32,
    ) -> Option<Picture> {
        if !virtual_ids.contains(&id) {
            return None;
        }
        // SAFETY: the caller's; the image is read before the next write.
        unsafe {
            let image = ghostty_kitty_graphics_image(graphics(t), id);
            (!image.is_null()).then(|| picture(image))
        }
    }
}

/// **Recordings the two cores are known to leave on different screens**,
/// by file name, and why. Such a recording gets no rate and the run says
/// so, with the reason, instead of failing; one listed here whose screens
/// agree fails the run, so the list cannot outlive its reason.
pub(crate) const UNEQUAL: [(&str, &str); 1] = [(
    "mpv-logo-40x6.raw",
    "alt screen vs primary (APC G end): the capture's last kitty command has no \
     ST; baud-core ends one only at BEL or ESC \\ as kitty does and stays on the \
     alternate screen, ghostty ends it at the next ESC and shows the primary",
)];

/// The reason `label` is listed in [`UNEQUAL`], if it is.
pub(crate) fn known_unequal(label: &str) -> Option<&'static str> {
    UNEQUAL
        .iter()
        .find(|(name, _)| *name == label)
        .map(|(_, why)| *why)
}

#[cfg(test)]
mod tests {
    /// Each recording [`super::UNEQUAL`] lists is one the run can be given,
    /// and only a listed name has a reason.
    #[test]
    fn every_known_unequal_recording_exists() {
        for (name, why) in super::UNEQUAL {
            let found = ["ascii", "kitty", "sixel", "utf8"].iter().any(|dir| {
                std::path::Path::new(&format!(
                    "{}/../../tests/recordings/{dir}/{name}",
                    env!("CARGO_MANIFEST_DIR")
                ))
                .exists()
            });
            assert!(found, "{name} is not a recording");
            assert!(!why.is_empty(), "{name} has no reason");
            assert_eq!(super::known_unequal(name), Some(why));
        }
        assert_eq!(super::known_unequal("grad-80x24-placeholder.raw"), None);
    }

    /// A recording from `tests/recordings/`.
    fn recording(name: &str) -> Vec<u8> {
        let p = format!(
            "{}/../../tests/recordings/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
    }

    /// The decoder hook is the process's: each test holds this while it
    /// sets the hook and runs its check.
    #[cfg(have_ghostty)]
    static HOOK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// One pass of the placeholder recording, its PNG decoded by ghostty
    /// or not, compared.
    #[cfg(have_ghostty)]
    fn placeholder(decoder: crate::ffi::ghostty::PngDecoder) -> super::Compared {
        let _hook = HOOK.lock().unwrap_or_else(|e| e.into_inner());
        crate::ffi::ghostty::set_png_decoder(decoder);
        let p = recording("kitty/grad-80x24-placeholder.raw");
        super::check(&p, p.len()).expect("libghostty-vt is linked")
    }

    /// With no PNG decoder ghostty refuses the `f=100` transmit, so its
    /// placeholder cells name an image it does not have while baud-core's
    /// show the one it decoded. The text is the same on both; the check
    /// fails on the virtual placement, and so the recording gets no rate.
    #[cfg(have_ghostty)]
    #[test]
    fn a_png_ghostty_does_not_decode_fails_the_screen_check() {
        let c = placeholder(crate::ffi::ghostty::PngDecoder::Cleared);
        let report = c.report("grad-80x24-placeholder.raw").join("\n");
        assert!(!c.equal(), "{report}");
        assert_eq!(
            c.cells_differing, 0,
            "the placeholders are text on both arms\n{report}"
        );
        assert!(!c.placements_differ(), "{report}");
        assert_eq!(c.virtuals.len(), 1, "{report}");
        let v = c.virtuals[0];
        assert!(v.baud.is_some() && v.ghostty.is_none(), "{report}");
        assert!(report.contains("SCREENS DIFFER, no rate"), "{report}");
    }

    /// With the decoder both arms store the same image, pixel for pixel,
    /// under the same virtual placement, and the check passes having
    /// compared it.
    #[cfg(have_ghostty)]
    #[test]
    fn a_png_both_arms_decode_passes_the_screen_check() {
        let c = placeholder(crate::ffi::ghostty::PngDecoder::Registered);
        let report = c.report("grad-80x24-placeholder.raw").join("\n");
        assert!(c.equal(), "{report}");
        assert_eq!(c.virtuals_compared, 1, "{report}");
        assert_eq!(c.pixels_unread, 0, "{report}");
        assert!(c.baud.stored > 0, "{report}");
        assert_eq!(
            c.counters(super::Arm::Ghostty),
            "images stored n/a, bytes decoded n/a, evicted n/a, refused n/a"
        );
        assert!(c.counters(super::Arm::Baud).contains("bytes decoded n/a"));
    }

    /// Without the library there is no second arm: the check compares
    /// nothing and says so (`None`), which the run reports as no ghostty
    /// figure, never as screens that agree. A build asked for the library
    /// and not finding it stops in build.rs.
    #[cfg(not(have_ghostty))]
    #[test]
    fn without_libghostty_vt_the_check_compares_nothing() {
        let p = recording("kitty/grad-80x24-placeholder.raw");
        assert!(super::check(&p, p.len()).is_none());
    }
}
