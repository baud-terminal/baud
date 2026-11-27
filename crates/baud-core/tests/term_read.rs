//! **`Term` is read through `TermRead`, and every read answers what the
//! inherent read of the same name answers.**
//!
//! A renderer that takes `&impl TermRead` reads the terminal through these
//! methods alone. Each test reads one terminal both ways, through the trait
//! and through `Term`'s own methods, and requires the same result.
mod support;
use baud_core::{Owned, Term};
use baud_domain::{
    Chan, Codepoint, ImageRead, KittyId, KittyPlacement, Needle, PackedCell, PaintRole, Row, Rows,
    TermRead, Video, WinSize,
};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 10,
};

fn implemented<T: TermRead>() {}

#[test]
fn a_term_implements_the_read_trait() {
    implemented::<Term<'static>>();
}

/// The cells of row `y`, read through the trait.
fn row<T: TermRead>(t: &T, y: Row) -> Option<Vec<PackedCell>> {
    t.row(y).map(<[PackedCell]>::to_vec)
}

/// Row `y` of the live screen, read through the trait's viewport.
fn view_row<T: TermRead>(t: &T, y: Row) -> Vec<PackedCell> {
    t.viewport().row(y).to_vec()
}

/// The codepoints attached to `cell`, read through the trait.
fn marks<T: TermRead>(t: &T, cell: PackedCell) -> Vec<u32> {
    t.cluster_marks(cell).collect()
}

/// Every match of `needle` across the history and the screen, read
/// through the trait.
fn found<T: TermRead>(t: &T, needle: &Needle) -> Vec<baud_domain::Found> {
    let lines = t.search_lines();
    let mut all = Vec::new();
    t.find(needle, lines.oldest, lines.end, |f| all.push(f));
    all
}

/// The images' generation, read through the trait's image reads.
fn image_gen<T: TermRead>(t: &T) -> baud_domain::ImageGen {
    t.images().generation()
}

/// One read both ways: through the trait, and `Term`'s own.
macro_rules! same {
    ($t:expr, $read:ident $(, $arg:expr)*) => {
        assert!(
            TermRead::$read($t $(, $arg)*) == Term::$read($t $(, $arg)*),
            concat!("`", stringify!($read), "` through the trait differs"),
        )
    };
}

#[test]
fn every_read_through_the_trait_answers_as_the_inherent_read() {
    let bytes = b"one\r\ntwo e\xcc\x81\r\n\x1b[1;31mred\x1b[m";
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, bytes);
    let t = &t;

    same!(t, geometry);
    same!(t, stride);
    same!(t, cursor);
    same!(t, cursor_visible);
    same!(t, wrap_pending);
    same!(t, window_back);
    same!(t, looking_back, Rows::ZERO);
    same!(t, status_rows);
    same!(t, screen);
    same!(t, screen_epoch);
    same!(t, resets);
    same!(t, history_erasures);
    same!(t, history_wipes);
    same!(t, cluster_epoch);
    same!(t, color_epoch);
    same!(t, style_writes);
    same!(t, palette, Chan::from_u8(1));
    same!(t, cursor_color_in, Video::Normal);
    same!(t, program_paint, PaintRole::Cursor);
    same!(t, reverse_video);
    same!(t, ambiguous_width);
    same!(t, unicode_widths);
    same!(t, vs16_width);
    same!(t, variants_seen);
    same!(t, bidi_support);
    same!(t, copy_direction);
    same!(t, strings_seen);
    same!(t, search_lines);
    same!(t, line_start, t.search_lines().top);
    same!(t, prompt_row);
    same!(t, primary_zone);
    same!(t, kitty_virtual, KittyId::NONE, KittyPlacement::NONE);
    same!(t, kitty_virtuals);
    let kept = t.sealed();
    same!(t, wrote_since, &kept);

    let y = Row::from_index(1);
    assert_eq!(row(t, y).as_deref(), t.row(y));
    assert_eq!(view_row(t, y), t.viewport().row(y));
    let accent = t.row(y).expect("row 1 is on the grid")[4];
    assert_eq!(
        marks(t, accent),
        t.cluster_marks(accent).collect::<Vec<_>>()
    );
    assert!(!marks(t, accent).is_empty(), "the premise: a cluster");
    same!(t, base_of, accent);

    let query: Vec<Codepoint> = "two".chars().map(Codepoint::from_char).collect();
    let needle = Needle::new(&query).expect("a needle");
    let mut inherent = Vec::new();
    let lines = t.search_lines();
    t.find(&needle, lines.oldest, lines.end, |f| inherent.push(f));
    assert_eq!(found(t, &needle), inherent);
    assert_eq!(inherent.len(), 1, "the premise: one match");

    assert_eq!(image_gen(t), t.images().generation());
}
