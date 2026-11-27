//! **A letter of a joining script wider than its one cell is drawn by
//! rule D**: full size into the cells its ink reaches when they are blank
//! ("empty or a space whatever its background"), else fitted to its one
//! cell, scaled by its ink, its marks with it, the baseline kept; the last
//! column fits and a wide character is not blank.
//!
//! The faces: baud's built Regular for the grid, and `joining-cases.ttf`
//! (`tools/baud-oracle cases joining`), whose sad is 1300 units isolated and
//! 950 medial against a cell of 600, drawn from boxes so where the ink
//! lies is exact. And, where the README's cache holds it, Noto Sans Arabic
//! 2.013, on which the README's Arabic line drew تضع without the dot of
//! its ض and حرف without its ر.
//!
//! A cell's ink is found by drawing the row with every other cell
//! concealed (`SGR 8`): a concealed cell keeps its letter, so the layout,
//! the joining and rule D read the same row, and draws nothing.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Target, Theme};

#[path = "../../baud-font/tests/sha256/mod.rs"]
mod sha256;

const PX: f32 = 20.0;

const WS: WinSize = WinSize {
    ws_row: 1,
    ws_col: 12,
};

fn read(path: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The pixels a row draws, with the cursor hidden and the characters
/// `shown` says drawn, every other concealed: `prefix` (a mode, say) then
/// each character of `text` in turn.
struct Drawn {
    ink: Vec<bool>,
    width: Px,
    cell: Px,
}

impl Drawn {
    /// The frame's width, in pixels.
    fn across_px(&self) -> usize {
        usize::try_from(self.width.value()).unwrap()
    }

    /// A cell's width, in pixels.
    fn cell_px(&self) -> usize {
        usize::try_from(self.cell.value()).unwrap()
    }

    /// The columns of the pixels drawn, least and most, if any.
    fn across(&self) -> Option<(usize, usize)> {
        let xs = (0..self.ink.len())
            .filter(|&i| self.ink[i])
            .map(|i| i % self.across_px());
        let (lo, hi) = xs.fold((usize::MAX, 0), |(lo, hi), x| (lo.min(x), hi.max(x)));
        (lo <= hi).then_some((lo, hi))
    }

    fn count(&self) -> usize {
        self.ink.iter().filter(|&&p| p).count()
    }

    /// The pixels both draw.
    fn both(&self, other: &Drawn) -> usize {
        self.ink
            .iter()
            .zip(&other.ink)
            .filter(|&(&a, &b)| a && b)
            .count()
    }

    /// Whether every pixel drawn lies in visual cell `c`, a pixel of the
    /// raster's margin either side allowed.
    fn inside(&self, c: usize) -> bool {
        self.across().is_some_and(|(lo, hi)| {
            lo + 1 >= c * self.cell_px() && hi < (c + 1) * self.cell_px() + 1
        })
    }
}

fn draw(
    faces: &[Face<'_>],
    ws: &WinSize,
    prefix: &str,
    text: &str,
    shown: impl Fn(usize) -> bool,
) -> Drawn {
    let mut r = Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(PX).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    // A rendition is written only where it changes: a control between a
    // letter and its mark would part them (GB4).
    let mut bytes = format!("\x1b[?25l{prefix}");
    let mut was = None;
    for (i, c) in text.chars().enumerate() {
        let now = shown(i);
        if was != Some(now) {
            bytes.push_str(if now { "\x1b[28m" } else { "\x1b[8m" });
            was = Some(now);
        }
        bytes.push(c);
    }
    let paint = |r: &mut Renderer<'_, '_>, bytes: &str| {
        let mut owned = Owned::new(ws);
        let mut t = Term::new(owned.storage(), ws);
        t.write_at(BytePos::ZERO, bytes.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        let (w, h) = r.pixel_size();
        let n = usize::try_from(w.value() * h.value()).unwrap();
        let mut pixels = vec![0u32; n];
        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
        let seal = t.seal_writes();
        r.render(&t, seal, &mut target, Damage::Full);
        pixels
    };
    let blank = paint(&mut r, &format!("\x1b[?25l{prefix}"));
    let drawn = paint(&mut r, &bytes);
    Drawn {
        ink: drawn.iter().zip(&blank).map(|(a, b)| a != b).collect(),
        width: r.pixel_size().0,
        cell: r.metrics().cell_w,
    }
}

/// The built Regular and the joining cases.
fn cases() -> (Vec<u8>, Vec<u8>) {
    (
        read("crates/baud-app/fonts/built/Regular.ttf"),
        read("crates/baud-font/tests/var/joining-cases.ttf"),
    )
}

/// BDSM reset: a row drawn as stored, its letters joined as displayed.
const EXPLICIT: &str = "\x1b[8l";
/// BDSM set: a paragraph whose first strong letter is Arabic runs right to
/// left against the right edge.
const IMPLICIT: &str = "\x1b[8h";

const SAD: char = '\u{0635}';
const REH: char = '\u{0631}';
const BEH: char = '\u{0628}';
const FATHA: char = '\u{064E}';

/// An isolated sad, 1300 units, beside a reh: the cell its ink reaches is
/// not blank, so it is fitted to its own, where full size it ran 26 pixels
/// across two cells of 12.
#[test]
fn an_over_wide_letter_beside_a_letter_is_fitted_to_its_cell() {
    let (regular, cases) = cases();
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&cases).unwrap()];
    let text = format!("{SAD}{REH}");
    let sad = draw(&faces, &WS, EXPLICIT, &text, |i| i == 0);
    assert!(sad.inside(0), "the sad in its cell: {:?}", sad.across());
    // Fitted, not cut: its ink still runs the cell's width.
    let (lo, hi) = sad.across().unwrap();
    assert!(hi - lo + 1 >= sad.cell_px() - 1, "{lo}..{hi}");
}

/// Beside blank cells the sad keeps its size, into them; a letter typed in
/// a cell it reaches fits it (rule D's stated trade), and so does a wide
/// character's lead.
#[test]
fn an_over_wide_letter_is_full_size_only_into_blank_cells() {
    let (regular, cases) = cases();
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&cases).unwrap()];
    let full = draw(&faces, &WS, EXPLICIT, &format!("{SAD}  {REH}"), |i| i == 0);
    let (lo, hi) = full.across().unwrap();
    assert!(
        hi - lo + 1 > 2 * full.cell_px(),
        "full size into the two blank cells: {lo}..{hi}"
    );
    let typed = draw(&faces, &WS, EXPLICIT, &format!("{SAD} {REH}"), |i| i == 0);
    assert!(
        typed.inside(0),
        "a letter where it reached: {:?}",
        typed.across()
    );
    let wide = draw(&faces, &WS, EXPLICIT, &format!("{SAD}\u{4E2D}"), |i| i == 0);
    assert!(
        wide.inside(0),
        "a wide lead is not blank: {:?}",
        wide.across()
    );
}

/// In the last column there is no cell to reach into: the sad fits,
/// drawn as it is fitted beside a letter, a whole number of cells along --
/// where full size it ran past the frame and was cut at its edge.
#[test]
fn an_over_wide_letter_in_the_last_column_fits() {
    let (regular, cases) = cases();
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&cases).unwrap()];
    let last = usize::from(WS.ws_col) - 1;
    let prefix = format!("{EXPLICIT}\x1b[1;{}H", last + 1);
    let sad = draw(&faces, &WS, &prefix, &SAD.to_string(), |_| true);
    let fitted = draw(&faces, &WS, EXPLICIT, &format!("{SAD}{REH}"), |i| i == 0);
    let along = last * sad.cell_px();
    let moved: Vec<usize> = (0..fitted.ink.len())
        .filter(|&i| fitted.ink[i])
        .map(|i| i + along)
        .collect();
    let at: Vec<usize> = (0..sad.ink.len()).filter(|&i| sad.ink[i]).collect();
    assert!(!at.is_empty() && at == moved, "the last column fits");
}

/// On a double-width line (DECDWL) a cell is two columns wide and the
/// sad is drawn at twice its size, 52 pixels: beside a reh it is fitted
/// to its doubled cell.
#[test]
fn an_over_wide_letter_on_a_double_width_line_fits_its_doubled_cell() {
    let (regular, cases) = cases();
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&cases).unwrap()];
    let prefix = format!("{EXPLICIT}\x1b#6");
    let sad = draw(&faces, &WS, &prefix, &format!("{SAD}{REH}"), |i| i == 0);
    let (lo, hi) = sad.across().unwrap();
    let two = 2 * sad.cell_px();
    assert!(hi < two + 1 && hi - lo + 1 >= two - 2, "{lo}..{hi}");
}

/// A fatha hung from the sad's anchor at 1100 units moves with its fitted
/// base: drawn where the anchor is drawn, inside the sad's cell, not at
/// the 22 pixels the face places it.
#[test]
fn a_mark_moves_with_its_fitted_base() {
    let (regular, cases) = cases();
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&cases).unwrap()];
    let text = format!("{SAD}{FATHA}{REH}");
    let sad = draw(&faces, &WS, EXPLICIT, &text, |i| i < 2);
    assert!(sad.inside(0), "the sad and its fatha: {:?}", sad.across());
    // The fatha is over the sad's anchor, 1100 of 1300 across the fitted
    // ink: the rows of ink that do not run the sad's width are its, right
    // of the cell's middle.
    let (ink, w) = (&sad.ink, sad.across_px());
    let row = |y: usize| (0..w).filter(move |&x| ink[y * w + x]);
    let fatha: Vec<usize> = (0..ink.len() / w)
        .filter(|&y| (1..sad.cell_px() / 2).contains(&row(y).count()))
        .flat_map(row)
        .collect();
    assert!(!fatha.is_empty(), "the fatha drawn");
    assert!(
        fatha
            .iter()
            .all(|&x| x > sad.cell_px() / 2 && x < sad.cell_px()),
        "the fatha over the anchor: {fatha:?}"
    );
}

/// Joined, each medial sad -- 950 units, 19 pixels -- is fitted between
/// the letters joined to it.
#[test]
fn joined_letters_are_fitted_between_their_neighbours() {
    let (regular, cases) = cases();
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&cases).unwrap()];
    let text = format!("{BEH}{SAD}{SAD}{BEH}");
    for k in [1, 2] {
        let sad = draw(&faces, &WS, EXPLICIT, &text, |i| i == k);
        assert!(sad.inside(k), "medial sad {k}: {:?}", sad.across());
    }
}

/// With BDSM set the cells beside a letter are the ones it is drawn
/// beside: "beh beh reh sad" runs right to left against the right edge, so
/// the sad is drawn fourth from the right with the reh and a beh on its
/// right, where its ink reaches -- fitted, though the cells after it in
/// the text are empty.
#[test]
fn the_cells_beside_a_letter_are_the_ones_drawn_beside_it() {
    let (regular, cases) = cases();
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&cases).unwrap()];
    let text = format!("{BEH}{BEH}{REH}{SAD}");
    let at = usize::from(WS.ws_col) - 4;
    let sad = draw(&faces, &WS, IMPLICIT, &text, |i| i == 3);
    assert!(sad.inside(at), "fitted at column {at}: {:?}", sad.across());
    // The control: with BDSM reset the same text is drawn as stored, the
    // sad at column 3, initial before the reh it joins as displayed, beside
    // the empty cell it reaches, and full size.
    let stored = draw(&faces, &WS, EXPLICIT, &text, |i| i == 3);
    let (lo, hi) = stored.across().unwrap();
    assert!(
        lo / stored.cell_px() == 3 && hi - lo + 1 > stored.cell_px() + 2,
        "{lo}..{hi}"
    );
}

/// Where the README's font cache holds Noto Sans Arabic 2.013 (the pin
/// `tools/scripts/readme-capture.sh`'s cache keeps), its line draws every letter:
/// the ر of حرف no longer under the isolated ف beside it, and the dot of
/// the ض of تضع no longer under the ت's two dots -- each pair of cells
/// shares a few pixels of antialiasing at most, where the ف covered most
/// of the ر and the ض most of the ت.
#[test]
fn the_readme_line_draws_every_letter_on_noto_sans_arabic() {
    const SHA: &str = "bd86ca02f087d7f3c3788ba458fb6b73744c7639ed276b8d870dba6def6c40d0";
    let cache = std::env::var_os("NOTO_CACHE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(|d| std::path::PathBuf::from(d).join("baud-readme"))
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/baud-readme"))
        });
    let Some(noto) = cache
        .map(|d| d.join("NotoSansArabic-Regular.ttf"))
        .and_then(|p| std::fs::read(p).ok())
    else {
        eprintln!("Noto Sans Arabic is not in the README's cache: skipped");
        return;
    };
    assert_eq!(
        sha256::sha256_hex(&noto),
        SHA,
        "the cache's file is not the pinned one"
    );
    let regular = read("crates/baud-app/fonts/built/Regular.ttf");
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&noto).unwrap()];
    let line = "الطرفية تضع كل حرف في الشبكة.";
    let ws = WinSize {
        ws_row: 1,
        ws_col: 32,
    };
    let one = |k: usize| draw(&faces, &ws, IMPLICIT, line, move |i| i == k);
    // ر and ف of حرف, ت and ض of تضع, by their place in the text.
    let pairs: Vec<(usize, usize, usize, usize)> = [(16, 17), (8, 9)]
        .into_iter()
        .map(|(a, b)| {
            let (da, db) = (one(a), one(b));
            (a, b, da.both(&db), da.count().min(db.count()))
        })
        .collect();
    eprintln!("cells, pixels shared, of the fewer drawn: {pairs:?}");
    assert!(
        pairs
            .iter()
            .all(|&(_, _, shared, least)| shared * 10 < least),
        "{pairs:?}"
    );
}
