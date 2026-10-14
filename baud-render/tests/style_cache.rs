//! The resolved rendition table is a cache: each copy is brought up to date
//! from the core's write log (`Renderer::sync_styles`) instead of resolving
//! every slot a frame.
//!
//! The contract is exact against the full resolve it replaced
//! (`Renderer::resolve_styles_into`): at every read of every recording, with
//! the inputs moving under it and copies that fall behind, every live slot of
//! every copy is what a full resolve gives. And it is cheap for the reason it
//! exists: a frame in which nothing was minted resolves no slot.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Blend, Damage, Pass, PxSize, Renderer, Resolved, Stack, StyleStamp, Tally, Theme, VideoFlash,
};
use support::{Canvas, RECORDINGS, WS, font, paint, recording};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn renderer<'f>(faces: &'f [Face<'f>], ws: &WinSize) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap()
}

/// Every live slot of `copy` against a full resolve now.
fn agrees(r: &mut Renderer<'_, '_>, t: &Term<'_>, copy: &[Resolved], what: &str) {
    let mut full = vec![Resolved::default(); t.styles().len()];
    r.resolve_styles_into(t, &mut full);
    for (k, slot) in t.styles().iter().enumerate() {
        if slot.is_some() {
            assert_eq!(copy[k], full[k], "{what}: slot {k} is stale");
        }
    }
}

/// A program that fills a 1,024-slot table and evicts: 2,500 distinct
/// truecolour renditions over one cell, with hits between them.
fn evicting() -> Vec<u8> {
    let mut out = Vec::new();
    let mut n = 0usize;
    'fill: for g in 0u8..=255 {
        for b in 1u8..=255 {
            out.extend(format!("\x1b[38;2;3;{g};{b}mX\x08\x1b[1mY\x08\x1b[0m").as_bytes());
            n += 1;
            if n == 2_500 {
                break 'fill;
            }
        }
    }
    out
}

/// **Three copies, like the app's buffer sets, each synced on its own
/// schedule -- one every read, one every third, one every fiftieth, which
/// laps the log -- with the theme, the video and the palette moving under
/// them, agree with a full resolve at every read.**
#[test]
fn the_cache_equals_a_full_resolve_at_every_read_boundary() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let small = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let mut streams: Vec<(String, WinSize, Vec<u8>)> = RECORDINGS
        .iter()
        .map(|n| (n.to_string(), WS, recording(n)))
        .collect();
    streams.push(("evicting".into(), small, evicting()));
    for (name, ws, bytes) in streams {
        let mut r = renderer(&faces, &ws);
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let len = t.styles().len();
        let mut copies = [
            vec![Resolved::default(); len],
            vec![Resolved::default(); len],
            vec![Resolved::default(); len],
        ];
        let mut stamps = [StyleStamp::default(); 3];
        for (i, piece) in bytes.chunks(4096).enumerate() {
            feed(&mut t, piece);
            // The inputs move: the theme, the video, the palette.
            match i % 13 {
                3 => r.set_theme(Theme::light()),
                6 => r.set_theme(Theme::dark()),
                8 => r.set_flash(VideoFlash::On),
                9 => r.set_flash(VideoFlash::Off),
                11 => feed(&mut t, b"\x1b]4;1;rgb:10/20/30\x07\x1b]11;rgb:01/02/03\x07"),
                12 => feed(&mut t, b"\x1b]104\x07\x1b]111\x07"),
                _ => {}
            }
            for (c, every) in [1usize, 3, 50].into_iter().enumerate() {
                if i % every == 0 {
                    r.sync_styles(&t, &mut copies[c], &mut stamps[c]);
                    agrees(
                        &mut r,
                        &t,
                        &copies[c],
                        &format!("{name}, read {i}, copy {c}"),
                    );
                }
            }
        }
    }
}

/// **A frame in which nothing was minted resolves no slot.** The control:
/// the first sync resolves the table's live slots, and a new rendition
/// resolves exactly one.
#[test]
fn a_steady_frame_resolves_nothing() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, &WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[31mred \x1b[1;32mgreen\x1b[0m plain");
    let mut copy = vec![Resolved::default(); t.styles().len()];
    let mut stamp = StyleStamp::default();
    let first = r.sync_styles(&t, &mut copy, &mut stamp);
    assert!(
        first.resolved.value() > 0,
        "the first sync resolved nothing"
    );
    assert_eq!(first.pass, Pass::Full);
    feed(
        &mut t,
        b" more text in the same renditions \x1b[31mred\x1b[0m",
    );
    let steady = r.sync_styles(&t, &mut copy, &mut stamp);
    assert_eq!(
        steady.resolved,
        Tally::default(),
        "a steady frame resolved slots"
    );
    assert_eq!(steady.pass, Pass::Replay);
    feed(&mut t, b"\x1b[35mmagenta");
    let one = r.sync_styles(&t, &mut copy, &mut stamp);
    assert_eq!(one.resolved.value(), 1, "one new rendition is one slot");
}

/// **An input change resolves the live slots, not the table**: at 16,384
/// slots with a dozen live, a theme change resolves a dozen.
#[test]
fn an_input_change_resolves_only_live_slots() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, &WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    assert_eq!(t.styles().len(), 16_384);
    for c in 31..=37 {
        feed(&mut t, format!("\x1b[{c}mx\x1b[1mx\x1b[0m").as_bytes());
    }
    let live = t.styles().iter().filter(|s| s.is_some()).count();
    let mut copy = vec![Resolved::default(); t.styles().len()];
    let mut stamp = StyleStamp::default();
    r.sync_styles(&t, &mut copy, &mut stamp);
    for step in 0..3 {
        match step {
            0 => r.set_theme(Theme::light()),
            1 => feed(&mut t, b"\x1b]4;2;rgb:ab/cd/ef\x07"),
            _ => r.set_flash(VideoFlash::On),
        }
        let sync = r.sync_styles(&t, &mut copy, &mut stamp);
        assert_eq!(sync.pass, Pass::Full, "step {step}: an input moved");
        assert_eq!(
            sync.resolved.value(),
            u64::try_from(live).unwrap(),
            "step {step}: resolved {} of {live} live slots",
            sync.resolved.value()
        );
        agrees(&mut r, &t, &copy, &format!("step {step}"));
    }
}

/// The digest of an incremental frame and of a fresh full repaint of the same
/// terminal, painted in the same order.
fn incremental_and_full(
    faces: &[Face<'_>],
    ws: &WinSize,
    before: &[u8],
    between: &[u8],
) -> (u64, u64) {
    let mut r = renderer(faces, ws);
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    let mut c = Canvas::for_renderer(&r);
    feed(&mut t, before);
    paint(&mut r, &mut t, &mut c, Damage::Full);
    feed(&mut t, between);
    paint(&mut r, &mut t, &mut c, Damage::Incremental);
    let mut fresh = renderer(faces, ws);
    let mut cf = Canvas::for_renderer(&fresh);
    paint(&mut fresh, &mut t, &mut cf, Damage::Full);
    (c.digest(), cf.digest())
}

/// **Setting a palette entry to black repaints the cells in it**, on an
/// incremental frame whose words did not change. The digest this replaced
/// folded an unset entry as black, so a set-to-black never changed it.
#[test]
fn a_palette_entry_set_to_black_repaints_on_an_incremental_frame() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let (inc, full) = incremental_and_full(
        &faces,
        &WS,
        b"\x1b[31mred text\x1b[0m\x1b[?25l",
        b"\x1b]4;1;rgb:00/00/00\x07",
    );
    assert_eq!(inc, full, "the text drawn in entry 1 kept its old colour");
}

/// **An eviction repaints the rows whose words did not change**: a slot a
/// cell names given to another rendition changes the cell's colour and not
/// its word. A 1,024-slot table filled, then one rendition bucketing to the
/// slot the text on row 0 names.
#[test]
fn an_eviction_repaints_rows_whose_words_did_not_change() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let small = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    // The text's rendition, and where it lands.
    let text = b"\x1b[38;2;200;10;10mvictim\x1b[0m\r\n";
    let slot = {
        let mut owned = Owned::new(&small);
        let mut t = Term::new(owned.storage(), &small);
        feed(&mut t, text);
        t.row(baud_domain::Row::from_index(0))
            .expect("a row on the grid")[0]
            .style
    };
    // Fill the table on row 1, then the thief.
    let mut fill = Vec::new();
    let mut plane = (0u8..=255).flat_map(|g| (1u8..=255).map(move |b| (5u8, g, b)));
    for _ in 0..1_100 {
        let (r, g, b) = plane.next().unwrap();
        fill.extend(format!("\x1b[38;2;{r};{g};{b}mX\x08").as_bytes());
    }
    let thief = plane
        .find(|&(r, g, b)| {
            let w = baud_domain::StyleWord::DEFAULT.with_fg(baud_domain::ColorWord::rgb(
                baud_domain::Chan::from_u8(r),
                baud_domain::Chan::from_u8(g),
                baud_domain::Chan::from_u8(b),
            ));
            baud_domain::StyleId::bucket(w, 1_024) == slot.index()
        })
        .unwrap();
    let mut before = text.to_vec();
    before.extend(&fill);
    before.extend(b"\x1b[?25l");
    let between = format!("\x1b[38;2;{};{};{}mX\x08\x1b[0m", thief.0, thief.1, thief.2);
    let (inc, full) = incremental_and_full(&faces, &small, &before, between.as_bytes());
    assert_eq!(inc, full, "row 0's text kept its evicted colour");
}

/// **The app never resolves the whole table a frame.** It syncs each set's
/// copy from the log; `resolve_styles_into` is the full resolve the tests and
/// the oracle compare against.
#[test]
fn the_app_syncs_and_never_resolves_the_whole_table() {
    let src = std::fs::read_to_string(format!(
        "{}/../baud-app/src/app.rs",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    assert!(src.contains("sync_styles("), "the app does not sync");
    assert!(
        !src.contains("resolve_styles_into("),
        "the app resolves the whole rendition table"
    );
}

/// **A copy resolved against one table is resolved whole against another**,
/// even when the second has taken more writes than the copy had seen: the
/// count alone would replay the second table's log from the first's place
/// and miss everything before it.
#[test]
fn a_copy_of_another_table_is_resolved_whole() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces, &WS);
    let mut copy = vec![Resolved::default(); 16_384];
    let mut stamp = StyleStamp::default();
    {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[31ma\x1b[32mb");
        r.sync_styles(&t, &mut copy, &mut stamp);
    }
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[33mc\x1b[34md\x1b[35me\x1b[36mf\x1b[37mg");
    let sync = r.sync_styles(&t, &mut copy, &mut stamp);
    assert_eq!(sync.pass, Pass::Full, "another table was replayed");
    agrees(&mut r, &t, &copy, "another table");
}
