//! **A row's Devanagari words are laid once while the row is the same**
//! (`crates/baud-render/src/kept.rs`): on the cell path a frame over rows that did
//! not change walks none of them for syllables and lays none of their words
//! again, and draws the records a renderer meeting the screen for the first
//! time draws; the cursor and the selection light a kept word without
//! laying it; a scrolled row is found by its cells; and what a word is laid
//! from besides its cells -- the size, the faces, a composition over the
//! row -- lays it again. With BDSM reset, where the layout keeps no answer,
//! a row the same since the last frame is not tested again for whether it
//! may hold a syllable.
//!
//! A debug build also lays every kept word again and requires the layout
//! kept to be the one it lays (`Renderer::word_at`).

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Col, Cols, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_font::var::{Fixed, Variation};
use baud_font::{Face, Tag};
use baud_render::{
    Blend, Caret, Composition, FaceRole, GlyphMap, Highlight, Instance, Overlay, OverlayStats,
    PxSize, Region, Renderer, Resolved, Stack, StyleStamp, Theme,
};

const README: &str = "टर्मिनल हर अक्षर को ग्रिड में रखता है।";
const SECOND: &str = "हिन्दी में शब्द पूरे खींचे जाते हैं";
const THIRD: &str = "नया वाक्य नीचे लिखा गया";

const WS: WinSize = WinSize {
    ws_col: 48,
    ws_row: 8,
};

fn read(path: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The built Regular and its narrower instances, as the app's default
/// stack lays them out.
fn faces(regular: &[u8]) -> (Vec<Face<'_>>, Vec<FaceRole>) {
    let mut faces = vec![Face::parse(regular).unwrap()];
    let mut roles = vec![FaceRole::Fallback];
    for w in baud_render::narrower_widths(&faces[0], &[]) {
        let mut f = Face::parse(regular).unwrap();
        f.set_variations(&[Variation {
            tag: Tag::from_bytes(*b"wdth"),
            value: Fixed::from_f32(w),
        }]);
        faces.push(f);
        roles.push(FaceRole::Narrower);
    }
    (faces, roles)
}

fn renderer<'f, 'a>(faces: &'f [Face<'a>], roles: &[FaceRole], px: f32) -> Renderer<'f, 'a> {
    Renderer::new(
        Stack::with_roles(faces, roles).unwrap(),
        PxSize::of(px).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// What a frame on the cell path draws by records: each record but where
/// on the atlas its glyph is, which a renderer drawing in another order
/// puts elsewhere; and the mask.
type Drawn = (Vec<Instance>, Vec<u8>);

fn placed(records: &[Instance]) -> Vec<Instance> {
    records
        .iter()
        .map(|i| Instance {
            u: baud_render::Tex(0),
            v: baud_render::Tex(0),
            ..*i
        })
        .collect()
}

/// **The cell path's frame as the app draws it**: the window laid out,
/// the rendition table synced, the glyph map walked and the overlay's
/// records made, with the cursor, the selection and the composition given.
struct Cells<'m> {
    map: GlyphMap<'m>,
    overlay: Overlay,
    styles: Vec<Resolved>,
    stamp: StyleStamp,
    candidates: Vec<baud_render::Candidate>,
}

impl<'m> Cells<'m> {
    fn new(backing: &'m mut [baud_render::MapEntry]) -> Cells<'m> {
        Cells {
            map: GlyphMap::over(backing),
            overlay: Overlay::new(),
            styles: Vec::new(),
            stamp: StyleStamp::default(),
            candidates: Vec::new(),
        }
    }

    fn frame(
        &mut self,
        r: &mut Renderer<'_, '_>,
        t: &Term<'_>,
        (highlight, composition): (Option<Highlight>, Option<&Composition>),
    ) -> (Drawn, OverlayStats) {
        if self.styles.len() != t.styles().one().len() {
            self.styles = vec![Resolved::default(); t.styles().one().len()];
            self.stamp = StyleStamp::default();
        }
        r.sync_styles(t, &mut self.styles, &mut self.stamp);
        let joins_seen = r.joins_seen();
        if let Some(how) = r.lay_out_bidi_copied(t, Rows::ZERO) {
            r.rejoin_bidi(t, Rows::ZERO, how, |_| true);
        }
        let view = t.viewport();
        self.map
            .ensure(t, view, &self.styles, r, &mut self.candidates);
        r.join_after_walk(t, Rows::ZERO, joins_seen);
        let cursor = t.cursor_visible().then(|| {
            let (row, col) = t.cursor();
            (row, col, t.cursor_style())
        });
        let (cols, rows) = (view.cols().index(), view.rows().index());
        let mut mask = vec![0u8; cols * (rows + 1)];
        let mut records = Vec::new();
        self.overlay.ensure(
            &baud_render::Scene {
                term: t,
                view,
                styles: baud_render::RowTables::one(&self.styles),
                cursor,
                highlight,
                map: &self.map,
                candidates: &self.candidates,
                pan: baud_font::Px::of(0),
                composition,
            },
            r,
            &mut mask,
            &mut records,
        );
        ((records, mask), self.overlay.stats())
    }
}

/// The same frame drawn by a renderer and a map meeting the screen for the
/// first time: what a frame drawn from kept words must equal.
fn fresh(
    (faces, roles, px): (&[Face<'_>], &[FaceRole], f32),
    t: &Term<'_>,
    lit: (Option<Highlight>, Option<&Composition>),
) -> Drawn {
    let mut r = renderer(faces, roles, px);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing);
    let ((records, mask), _) = cells.frame(&mut r, t, lit);
    (placed(&records), mask)
}

fn screen(t: &mut Term<'_>) {
    feed(
        t,
        &format!("plain ascii\r\n{README}\r\n\u{4E2D}\u{6587}\r\n{SECOND}\r\n{README}"),
    );
}

/// **A frame over the rows it drew last walks none and lays no word
/// again**, and draws what it drew; the block cursor moved onto a word and
/// a selection over one light it without laying it, as a fresh renderer
/// lights it.
#[test]
fn an_unchanged_row_lays_no_word_again() {
    let regular = read("crates/baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let mut r = renderer(&faces, &roles, 28.0);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    screen(&mut t);
    let (first, s) = cells.frame(&mut r, &t, (None, None));
    let laid = r.words_laid();
    assert!(laid.value() >= 15, "the premise: words laid, {laid:?}");
    // The README's two rows hold the same cells, and are one row kept.
    assert_eq!(s.walked.value(), 2, "the Hindi rows walked, each once");
    let (again, s) = cells.frame(&mut r, &t, (None, None));
    assert_eq!(r.words_laid(), laid, "an unchanged screen lays no word");
    assert_eq!(s.walked.value(), 0, "and walks no row");
    assert_eq!(again, first, "and draws what it drew");
    // The block cursor on the second Hindi row's third cell.
    feed(&mut t, "\x1b[4;3H");
    let lit = (None, None);
    let (moved, s) = cells.frame(&mut r, &t, lit);
    assert_eq!(
        r.words_laid(),
        laid,
        "the cursor lights a word, laying none"
    );
    assert_eq!(s.walked.value(), 0);
    assert_eq!(
        (placed(&moved.0), moved.1),
        fresh((&faces, &roles, 28.0), &t, lit)
    );
    // A selection across the README's first word and into its second.
    let h = Highlight::new(
        (Row::from_index(1), Col::at_index(2)),
        (Row::from_index(1), Col::at_index(9)),
        Region::Stream,
    );
    let lit = (Some(h), None);
    let (selected, s) = cells.frame(&mut r, &t, lit);
    assert_eq!(
        r.words_laid(),
        laid,
        "a selection lights words, laying none"
    );
    assert_eq!(s.walked.value(), 0);
    assert_eq!(
        (placed(&selected.0), selected.1),
        fresh((&faces, &roles, 28.0), &t, lit)
    );
}

/// **A scrolled row is found by its cells**: the screen moved up a line
/// lays no word of the rows it moved, and a row written lays its own.
#[test]
fn a_scrolled_row_is_found_by_its_cells() {
    let regular = read("crates/baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let mut r = renderer(&faces, &roles, 28.0);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    screen(&mut t);
    feed(&mut t, "\x1b[8;1H");
    cells.frame(&mut r, &t, (None, None));
    let laid = r.words_laid();
    // Three lines up, the README's first copy off the screen's top.
    feed(&mut t, "\r\n\r\n\r\nascii");
    let (moved, s) = cells.frame(&mut r, &t, (None, None));
    assert_eq!(r.words_laid(), laid, "rows moved lay no word");
    assert_eq!(s.walked.value(), 0, "and are not walked");
    assert_eq!(
        (placed(&moved.0), moved.1),
        fresh((&faces, &roles, 28.0), &t, (None, None))
    );
    // A row written: its words laid, and no other row's.
    feed(&mut t, &format!("\r\n{THIRD}"));
    let (written, s) = cells.frame(&mut r, &t, (None, None));
    assert_eq!(s.walked.value(), 1, "the row written is walked alone");
    let words = THIRD.split(' ').count();
    assert_eq!(
        r.words_laid().value() - laid.value(),
        u64::try_from(words).unwrap(),
        "and its words laid"
    );
    assert_eq!(
        (placed(&written.0), written.1),
        fresh((&faces, &roles, 28.0), &t, (None, None))
    );
}

/// **What a word is laid from besides its row's cells lays it again**:
/// the size the faces are drawn at, a nudge too small to move the cell's
/// geometry among them; the faces; and a composition over the row, whose
/// row is laid as its commit will be. Each frame draws what a fresh
/// renderer draws.
#[test]
fn what_a_word_is_laid_from_lays_it_again() {
    let regular = read("crates/baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let mut r = renderer(&faces, &roles, 28.0);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    screen(&mut t);
    cells.frame(&mut r, &t, (None, None));
    let check = |r: &mut Renderer<'_, '_>, cells: &mut Cells<'_>, t: &Term<'_>, px, what| {
        let laid = r.words_laid();
        let (drawn, _) = cells.frame(r, t, (None, None));
        assert!(r.words_laid().value() > laid.value(), "{what} lays again");
        assert_eq!(
            (placed(&drawn.0), drawn.1),
            fresh((&faces, &roles, px), t, (None, None)),
            "{what}"
        );
    };
    // The size: 28.2 px rounds to the cell 28 px makes.
    let before = r.metrics();
    r.rescale(PxSize::of(28.2).unwrap()).unwrap();
    assert_eq!(r.metrics().cell_w, before.cell_w, "the premise: one cell");
    check(&mut r, &mut cells, &t, 28.2, "a size");
    // The faces: the same faces stacked again.
    r.restack(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(28.2).unwrap(),
    )
    .unwrap();
    cells = {
        drop(cells);
        backing = GlyphMap::backing();
        Cells::new(&mut backing)
    };
    check(&mut r, &mut cells, &t, 28.2, "a restack");
    // A composition on the README's row, at its end.
    feed(&mut t, "\x1b[2;27H");
    let text: Vec<Codepoint> = "कम"
        .chars()
        .map(|c| Codepoint::new(u32::from(c)).unwrap())
        .collect();
    let c = Composition::at_cursor(&t, &text, &[], Caret::End);
    let laid = r.words_laid();
    let (composed, _) = cells.frame(&mut r, &t, (None, Some(&c)));
    assert!(r.words_laid().value() > laid.value(), "a composition's row");
    assert_eq!(
        (placed(&composed.0), composed.1),
        fresh((&faces, &roles, 28.2), &t, (None, Some(&c))),
    );
}

/// **With BDSM reset no row is folded for whether it may hold a
/// syllable**: the layout keeps no answer then, with no letter that takes a
/// joining form seen and with one, and the glyph map's walk of the window,
/// which probes every cell's entry, answers for each row -- the Hindi rows
/// held and walked, and a cluster's row by its marks, and a row that
/// changes to Hindi too.
#[test]
fn with_bdsm_reset_the_map_walk_answers_for_every_row() {
    let regular = read("crates/baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    // The Arabic letters are in a block that holds a Prepend, so their row
    // is walked too, and holds no syllable.
    for (what, joining, held) in [
        ("no join seen", "", 2),
        ("a join seen", "\u{0628}\u{0627}", 3),
    ] {
        let mut r = renderer(&faces, &roles, 28.0);
        let mut backing = GlyphMap::backing();
        let mut cells = Cells::new(&mut backing);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, "\x1b[8l");
        screen(&mut t);
        // A Latin letter carrying a Devanagari sign: a cluster, whose mark
        // the map's entries do not see, read from the store.
        feed(
            &mut t,
            &format!("\r\n{joining}\r\nbox \u{2500}\u{2502}\r\na\u{0901}"),
        );
        let (_, s) = cells.frame(&mut r, &t, (None, None));
        assert_eq!(s.folded.value(), 0, "{what}: no row folded");
        assert_eq!(s.walked.value(), held + 1, "{what}: the Hindi rows walked");
        feed(&mut t, &format!("\x1b[1;1H{THIRD}"));
        let (_, s) = cells.frame(&mut r, &t, (None, None));
        assert_eq!(s.folded.value(), 0, "{what}: none folded");
        assert_eq!(s.walked.value(), 1, "{what}: the row written walked");
    }
}

/// **The CPU and instance paths lay no word of a row they convert again
/// unchanged**: a whole repaint of the same screen converts every row and
/// lays none of their words, and draws the same pixels.
#[test]
fn a_whole_repaint_lays_no_word_again() {
    let regular = read("crates/baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let mut r = renderer(&faces, &roles, 28.0);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    screen(&mut t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let draw = |r: &mut Renderer<'_, '_>, t: &mut Term<'_>| {
        let mut px = vec![0u32; n];
        let mut target = baud_render::Target::new(&mut px, w, w, h).unwrap();
        let seal = t.seal_writes();
        r.render(t, seal, &mut target, baud_render::Damage::Full);
        px
    };
    let first = draw(&mut r, &mut t);
    let laid = r.words_laid();
    assert!(laid.value() >= 15, "the premise: words laid, {laid:?}");
    let again = draw(&mut r, &mut t);
    assert_eq!(r.words_laid(), laid, "a whole repaint lays no word");
    assert!(first == again, "and draws the same pixels");
}

/// **A row UAX #9 reorders is kept by its cells and the order they are
/// drawn in**: a Hebrew phrase before a Hindi one makes a right-to-left
/// paragraph, and a frame over it again walks no row and lays no word, and
/// draws what a fresh renderer draws.
#[test]
fn a_reordered_row_is_kept_by_its_order() {
    let regular = read("crates/baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let mut r = renderer(&faces, &roles, 28.0);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        &format!("\u{05E9}\u{05DC}\u{05D5}\u{05DD} {SECOND}\r\nplain"),
    );
    let (first, s) = cells.frame(&mut r, &t, (None, None));
    assert!(
        r.bidi().reordered(Row::ZERO),
        "the premise: the row reorders"
    );
    assert_eq!(s.walked.value(), 1, "the Hindi row walked");
    let laid = r.words_laid();
    let (again, s) = cells.frame(&mut r, &t, (None, None));
    assert_eq!(s.walked.value(), 0, "and not again");
    assert_eq!(r.words_laid(), laid, "nor its words laid");
    assert_eq!(again, first, "and drawn as it was");
    assert_eq!(
        (placed(&again.0), again.1),
        fresh((&faces, &roles, 28.0), &t, (None, None))
    );
}

/// **A row whose cells stay the same while the order they are drawn in
/// moves is walked and laid again**: a Hebrew line wrapping into a Hindi
/// one makes the paragraph right to left, which draws the Hindi row's
/// blanks first; a Latin letter written over the Hebrew line's first
/// makes it left to right, the Hindi row's cells untouched.
#[test]
fn a_row_drawn_in_another_order_is_laid_again() {
    let regular = read("crates/baud-app/fonts/built/Regular.ttf");
    let (faces, roles) = faces(&regular);
    let mut r = renderer(&faces, &roles, 28.0);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let hebrew: String = "\u{05E9}\u{05DC}\u{05D5}\u{05DD} "
        .chars()
        .cycle()
        .take(usize::from(WS.ws_col))
        .collect();
    feed(&mut t, &format!("{hebrew}{SECOND}\r\nplain"));
    cells.frame(&mut r, &t, (None, None));
    let hindi = Row::from_index(1);
    assert!(
        r.bidi().reordered(hindi),
        "the premise: drawn right to left"
    );
    feed(&mut t, "\x1b[1;1Ha");
    let (drawn, s) = cells.frame(&mut r, &t, (None, None));
    assert!(!r.bidi().reordered(hindi), "and now in its own order");
    assert_eq!(s.walked.value(), 1, "the Hindi row walked again");
    assert_eq!(
        (placed(&drawn.0), drawn.1),
        fresh((&faces, &roles, 28.0), &t, (None, None))
    );
}

/// **A word is laid again where a syllable's rendition names another face
/// though its cells are the same**: the rendition table full of named
/// slots, a word whose second half names the table's last free slot in
/// red, and then that half written again in a bold red rendition that
/// takes the slot once a sweep frees it -- so नल's cells name the same slot
/// and their face is bold. The row is kept by its
/// cells, and its word, kept by its first syllable's face, was painted
/// from the layout laid in the regular face; each word is kept by every
/// cell's face, and the frame draws what a fresh renderer draws.
#[test]
fn a_syllable_whose_rendition_names_another_face_lays_its_word_again() {
    evicted_lays_again("टर्मि\x1b[38;2;200;10;10mनल", 6, "नल");
}

/// **A word is laid again where a cell inside a syllable names another
/// face**, its syllable's first cell's the same: र् and then मि in the
/// table's last free slot, which a bold rendition then takes, so म and ि
/// are bold and र् is not. Each glyph of र्मि is drawn from its own cell's
/// face, so a word kept by its syllables' first cells' faces alone was
/// painted with म and the reph-and-ि glyph regular.
#[test]
fn a_cell_inside_a_syllable_that_names_another_face_lays_its_word_again() {
    evicted_lays_again("र्\x1b[38;2;200;10;10mमि", 3, "मि");
}

/// The rendition table, lent no generations, full of named slots, `text`
/// written at column 2 of row 1 with its tail in the table's last free slot,
/// the cell at column `at` naming it; then the tail overwritten, so nothing
/// names the slot, and written again in a bold rendition bucketing there,
/// which the full table's sweep frees for it. The word lays again, and draws
/// what a fresh renderer draws.
fn evicted_lays_again(text: &str, at: usize, tail: &str) {
    let (regular, bold) = (
        read("crates/baud-app/fonts/built/Regular.ttf"),
        read("crates/baud-app/fonts/built/Bold.ttf"),
    );
    let (mut faces, mut roles) = faces(&regular);
    faces.push(Face::parse(&bold).unwrap());
    roles.push(FaceRole::Style(baud_render::FaceStyle::Bold));
    let rgb = |(r, g, b): (u8, u8, u8)| {
        baud_domain::ColorWord::rgb(
            baud_domain::Chan::from_u8(r),
            baud_domain::Chan::from_u8(g),
            baud_domain::Chan::from_u8(b),
        )
    };
    let mut plane = (0u8..=255).flat_map(|g| (1u8..=255).map(move |b| (5u8, g, b)));
    // 1,022 renditions each on a cell of its own, pushed into the history
    // so each stays named; then the word, its tail in the last free slot.
    let mut before = String::new();
    for i in 0..1_022 {
        let (r, g, b) = plane.next().unwrap();
        before.push_str(&format!("\x1b[38;2;{r};{g};{b}mX"));
        if (i + 1) % 40 == 0 {
            before.push_str("\r\n");
        }
    }
    before.push_str("\x1b[0m");
    before.push_str(&"\n".repeat(10));
    before.push_str(&format!("\x1b[H\x1b[?25l\x1b[2;3H{text}\x1b[0m\x1b[6;1H"));
    let mut owned = Owned::new(&WS).without_frozen();
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &before);
    assert!(
        t.styles().one().slots().iter().all(|s| s.word().is_some()),
        "the premise: the table is full"
    );
    // Where the tail starts: the first cell naming the tail's slot.
    let row = |t: &Term<'_>| t.row(Row::from_index(1)).expect("a row").to_vec();
    let (slot, len) = (row(&t)[at].style, t.styles().one().len());
    let thief = plane
        .find(|&c| {
            let w = baud_domain::StyleWord::DEFAULT
                .with_fg(rgb(c))
                .with_attrs(baud_domain::Attrs::BOLD);
            baud_domain::StyleId::bucket(w, len) == slot.index()
        })
        .unwrap();
    let mut r = renderer(&faces, &roles, 28.0);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing);
    cells.frame(&mut r, &t, (None, None));
    let cells_before = row(&t);
    let from = row(&t)
        .iter()
        .position(|c| c.style == slot)
        .expect("the tail");
    let blanks = " ".repeat(tail.chars().count());
    // Refused until the full table's sweep is due, the refusals counting
    // toward it, so the tail is written again until it takes the slot.
    let again = format!(
        "\x1b[2;{c}H\x1b[1;38;2;{};{};{}m{tail}\x1b[0m",
        thief.0,
        thief.1,
        thief.2,
        c = from + 1,
    );
    feed(
        &mut t,
        &format!(
            "\x1b[2;{c}H\x1b[0m{blanks}{}\x1b[6;1H",
            again.repeat(len / 16 + 2),
            c = from + 1,
        ),
    );
    assert!(t.style_reclaims().sweeps > 0, "the premise: it swept");
    assert!(
        PackedCell::same_cells(&row(&t), &cells_before),
        "the premise: the word's cells are the same"
    );
    assert!(
        t.styles()
            .one()
            .word_of(row(&t)[at])
            .attrs()
            .has(baud_domain::Attrs::BOLD),
        "the premise: {tail}'s slot is bold now"
    );
    let laid = r.words_laid();
    let (drawn, _) = cells.frame(&mut r, &t, (None, None));
    assert!(r.words_laid().value() > laid.value(), "the word laid again");
    assert_eq!(
        (placed(&drawn.0), drawn.1),
        fresh((&faces, &roles, 28.0), &t, (None, None)),
        "drawn as a fresh renderer draws it"
    );
}
