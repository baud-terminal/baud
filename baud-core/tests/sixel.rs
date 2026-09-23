//! The Sixel parser-hook gate: a `DCS ... q` Sixel is decoded and placed in
//! the image store, and it is **chunk-invariant** -- the same image whatever
//! the read boundaries -- because the body accumulates in `sixel_body` across
//! reads before it decodes. A recording carries a Sixel in ~5 KB, far past a
//! `read(2)`, so the read boundary lands mid-body on every real transfer; a
//! parser that reset there would decode the tail of every image as garbage,
//! and only in production.

use baud_core::*;
use baud_domain::{ImageBudget, ImagePx, RowRot, SixelClaim};

mod support;

/// The committed real capture: a 64x48 ImageMagick Sixel.
const SWIRL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/swirl.raw"
));

/// A budget that holds the 64x48 capture with room.
fn ample() -> ImageBudget {
    ImageBudget::of(64 * 48 + 64, 8192)
}

/// Feed `data` in reads of `chunk` bytes into a term provisioned with `budget`,
/// then inspect it. A closure rather than a returned struct so the borrow of
/// `Owned` stays inside and the assertions read the live term.
fn run(data: &[u8], chunk: usize, budget: ImageBudget, f: impl FnOnce(&Term)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(budget);
    let mut t = Term::new(owned.storage(), &ws);
    let mut at = 0;
    while at < data.len() {
        let n = chunk.min(data.len() - at);
        t.write_at(BytePos::ZERO, &data[at..at + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        at += n;
    }
    f(&t);
}

/// The pixel size of the one image placed, or `None`.
fn placed_dims(t: &Term) -> Option<(u16, u16)> {
    let imgs = t.images();
    let p = imgs.placements().next().map(|(_, p)| *p)?;
    imgs.image(p.image).map(|v| (v.width.raw(), v.height.raw()))
}

#[test]
fn a_sixel_is_decoded_and_placed() {
    run(SWIRL, SWIRL.len(), ample(), |t| {
        assert_eq!(t.images().placements().count(), 1, "one image is placed");
        assert_eq!(placed_dims(t), Some((64, 48)), "the raster size");
        assert_eq!(t.images().images_refused, 0);
        assert_eq!(t.images().placements_refused, 0);
    });
}

#[test]
fn a_sixel_is_chunk_invariant() {
    // The body spans reads at every small chunk size, so a parser that did not
    // accumulate across reads would decode a truncated or empty image. Each
    // read size must give the one 64x48 image, unrefused.
    for chunk in [1usize, 2, 7, 64, 512, 1024, 4096, SWIRL.len()] {
        run(SWIRL, chunk, ample(), |t| {
            assert_eq!(t.images().placements().count(), 1, "chunk {chunk}: one");
            assert_eq!(placed_dims(t), Some((64, 48)), "chunk {chunk}: size");
            assert_eq!(t.images().images_refused, 0, "chunk {chunk}: unrefused");
        });
    }
}

/// Feed `d` to `t` in one read.
fn one(t: &mut Term, d: &[u8]) {
    t.write_at(BytePos::ZERO, d);
    t.feed(BytePos::ZERO, ReadLen::from_index(d.len()));
}

#[test]
fn a_previewer_replacing_images_does_not_accumulate_them() {
    // A file previewer (yazi) draws a Sixel, then to show the next file
    // positions the cursor back at the image's cell and overwrites those cells
    // with spaces before drawing the next Sixel -- it does not use ED. Writing
    // over an image's cells frees it, so the placements stay bounded however
    // many files go by. Without it a new image and slot pile up at the one cell
    // until the 32-slot store fills and every later preview is refused, the
    // stale transparent images stacking into garbage in the meantime -- the
    // scramble a previewer shows in baud.
    let ws = support::CORPUS;
    // Room for 40 copies, so without the fix the slot table (32) fills before
    // the arena does and the failure is refusal, not a body/arena limit.
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(64 * 48 * 40, 8192));
    let mut t = Term::new(owned.storage(), &ws);
    // The embedder's cell size: an image spans cells only once the core knows
    // it, which is what lets an overwrite free the image under it.
    t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
    let spaces = [b' '; 80];
    for _ in 0..40 {
        one(&mut t, b"\x1b[1;1H");
        one(&mut t, &spaces); // overwrite the previous preview's cells
        one(&mut t, b"\x1b[1;1H");
        one(&mut t, SWIRL); // the next preview at the same cell
    }
    assert_eq!(
        t.images().placements().count(),
        1,
        "each preview replaces the last; they do not accumulate"
    );
    assert_eq!(t.images().images_refused, 0, "the store never fills");
}

#[test]
fn writing_over_a_sixels_cells_frees_it_and_writing_elsewhere_does_not() {
    // The mechanism, with its own control. A 64x48 Sixel over 8x16 cells spans
    // 8 columns by 3 rows from its anchor at (0,0). A glyph written inside that
    // rectangle frees the image; a glyph well outside it leaves the image
    // placed -- so the freeing is scoped to the image's cells, not any write.
    let ws = support::CORPUS;
    let budget = ImageBudget::of(64 * 48 + 64, 8192);

    // Inside the rect (row 2, col 3): frees it.
    let mut owned = Owned::new(&ws).with_images(budget);
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
    one(&mut t, b"\x1b[1;1H");
    one(&mut t, SWIRL);
    assert_eq!(t.images().placements().count(), 1, "placed");
    one(&mut t, b"\x1b[3;4HX"); // row 2, col 3 (1-based 3;4) -- inside 3x8
    assert_eq!(
        t.images().placements().count(),
        0,
        "a write inside the image's cells frees it"
    );

    // The control: a write far outside the rect (row 20) leaves it placed.
    let mut owned = Owned::new(&ws).with_images(budget);
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
    one(&mut t, b"\x1b[1;1H");
    one(&mut t, SWIRL);
    one(&mut t, b"\x1b[20;1HX"); // row 19 -- below the 3-row image
    assert_eq!(
        t.images().placements().count(),
        1,
        "a write outside the image's cells leaves it placed"
    );
}

#[test]
fn the_primary_da_advertises_sixel_only_when_opted_in_and_provisioned() {
    // An application reads attribute 4 and stops drawing its text fallback, so
    // the claim is the embedder's to make: off by default whatever the store
    // holds, and on only with image memory behind it -- an unprovisioned baud
    // refuses every Sixel, so claiming it there would be a lie.
    let ws = support::CORPUS;

    let mut provisioned = Owned::new(&ws).with_images(ImageBudget::of(64, 64));
    let mut t = Term::new(provisioned.storage(), &ws);
    one(&mut t, b"\x1b[c");
    assert_eq!(
        t.reply(),
        b"\x1b[?6c",
        "provisioned but not opted in: no claim"
    );
    t.clear_reply();
    t.set_sixel_claim(SixelClaim::Yes);
    one(&mut t, b"\x1b[c");
    assert_eq!(
        t.reply(),
        b"\x1b[?6;4c",
        "opted in and provisioned: VT102 plus Sixel"
    );

    let mut bare = Owned::new(&ws);
    let mut t = Term::new(bare.storage(), &ws);
    t.set_sixel_claim(SixelClaim::Yes);
    one(&mut t, b"\x1b[c");
    assert_eq!(
        t.reply(),
        b"\x1b[?6c",
        "opted in but unprovisioned: no Sixel claim"
    );
}

#[test]
fn an_unprovisioned_store_refuses_and_counts() {
    // A default terminal has no image memory (no body scratch either), so the
    // body length rises past the empty buffer and the Sixel is refused at the
    // body-overflow branch -- recognised and counted, not a wrong image and not
    // a silent drop. (The store-refuse branch is exercised separately, above.)
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    t.write_at(BytePos::ZERO, SWIRL);
    t.feed(BytePos::ZERO, ReadLen::from_index(SWIRL.len()));
    assert_eq!(t.images().placements().count(), 0, "nothing is placed");
    assert_eq!(
        t.images().images_refused,
        1,
        "the one Sixel is counted refused"
    );
}

#[test]
fn a_body_too_large_for_the_scratch_is_refused_whole() {
    // A body scratch smaller than the ~5 KB capture: the length rises past it,
    // so the terminator sees an overflow and refuses the whole image rather
    // than decoding a truncated one.
    run(
        SWIRL,
        SWIRL.len(),
        ImageBudget::of(64 * 48 + 64, 1024),
        |t| {
            assert_eq!(t.images().placements().count(), 0, "not placed");
            assert_eq!(t.images().images_refused, 1, "refused, not decoded short");
        },
    );
}

#[test]
fn a_body_that_fits_but_no_pixel_arena_refuses_at_the_store() {
    // Body scratch provisioned, pixel arena starved: the body decodes-measures
    // fine and reaches `store`, which refuses because no gap holds `w * h`.
    // This is the store-refuse branch, distinct from the body-overflow branch
    // an unprovisioned terminal takes.
    run(SWIRL, SWIRL.len(), ImageBudget::of(0, 8192), |t| {
        assert_eq!(t.images().placements().count(), 0, "not placed");
        assert_eq!(t.images().images_refused, 1, "the store refused it");
    });
}

/// A 1x6 red Sixel, the smallest well-formed image, for placement tests.
const TINY: &[u8] = b"\x1bP0;1;0q#0;2;100;0;0~\x1b\\";

fn feed(t: &mut Term, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// The line at the top of the live window in the image numbering, as a
/// count: what a placement at the cursor's row adds its row to.
fn top(t: &Term) -> usize {
    usize::try_from(t.image_origin(Rows::ZERO).top.value()).unwrap()
}

#[test]
fn the_windows_top_line_matches_the_rotation_deltas() {
    // The window's top line must move exactly with the rotation -- up one for
    // every row retired, down one for every row restored -- or an image
    // anchored by it slides off its row. The claim is by construction (one
    // step per `retire` and `unretire`), which this repo keeps finding wrong,
    // so it is measured on a payload that scrolls past the ring and back.
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let mut net: i64 = 0;
    let mut before = t.rotation();
    let step = |t: &mut Term, bytes: &[u8], before: &mut RowRot, net: &mut i64| {
        feed(t, bytes);
        match t.moved_since(*before) {
            Moved::Retired(n) => *net += i64::try_from(n.index()).unwrap(),
            Moved::Restored(n) => *net -= i64::try_from(n.index()).unwrap(),
        }
        *before = t.rotation();
    };
    let newlines = vec![b'\n'; 80];
    for _ in 0..40 {
        step(&mut t, &newlines, &mut before, &mut net);
    }
    assert!(net > 2000, "the payload scrolled past the ring, not {net}");
    // Twelve reverse indexes at the top take twelve rows back.
    step(&mut t, b"\x1b[1;1H", &mut before, &mut net);
    for _ in 0..12 {
        step(&mut t, b"\x1bM", &mut before, &mut net);
    }
    assert_eq!(
        i64::try_from(top(&t)).unwrap(),
        net,
        "the top line is the net of the retired and restored rows"
    );
}

#[test]
fn two_sixels_past_a_full_ring_get_distinct_anchors() {
    // `hist` caps at `depth`, so two images placed after the ring fills would
    // share an anchor numbered by it. The window's top line does not cap, so
    // they get distinct absolute lines.
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ample());
    let mut t = Term::new(owned.storage(), &ws);
    // Fill the ring (depth 2000) and place the first image.
    feed(&mut t, &vec![b'\n'; 2100]);
    feed(&mut t, TINY);
    // Scroll well past it and place the second.
    feed(&mut t, &vec![b'\n'; 1000]);
    feed(&mut t, TINY);

    let lines: Vec<_> = t.images().placements().map(|(_, p)| p.line).collect();
    assert_eq!(lines.len(), 2, "two images are placed");
    assert!(
        lines[0] != lines[1],
        "two images past a full ring must anchor to distinct lines"
    );
}

#[test]
fn text_after_a_sixel_is_not_consumed() {
    // The `ST` ends the Sixel; the text after it is written normally. A parser
    // that ran the string to the end of the stream would swallow it.
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ample());
    let mut t = Term::new(owned.storage(), &ws);
    let mut bytes = SWIRL.to_vec();
    bytes.extend_from_slice(b"AFTER");
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));

    let row = t.row(Row::from_index(0));
    let text: String = row
        .iter()
        .filter_map(|c| c.content.as_codepoint())
        .filter_map(|p| char::from_u32(p.value()))
        .collect();
    assert_eq!(
        text.trim_end(),
        "AFTER",
        "the text after the Sixel is drawn"
    );
    assert_eq!(
        t.images().placements().count(),
        1,
        "and the image was placed"
    );
}

/// Feed `data` in reads of `chunk` bytes into a fresh term whose cell is
/// `cell_h` device pixels tall (`0` leaves it unset, the headless default),
/// then inspect the live term.
fn advanced(data: &[u8], chunk: usize, cell_h: u16, f: impl FnOnce(&Term)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ample());
    let mut t = Term::new(owned.storage(), &ws);
    if cell_h != 0 {
        t.set_cell_size(ImagePx::of(8), ImagePx::of(cell_h));
    }
    let mut at = 0;
    while at < data.len() {
        let n = chunk.min(data.len() - at);
        t.write_at(BytePos::ZERO, &data[at..at + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        at += n;
    }
    f(&t);
}

#[test]
fn a_sixel_advances_the_cursor_below_it() {
    // A 48px image with a 16px cell is three rows tall, so the cursor lands
    // three rows below where it started, its column at the image's left margin.
    advanced(SWIRL, SWIRL.len(), 16, |t| {
        assert_eq!(t.cursor().0.index(), 3, "three rows below");
        assert_eq!(t.cursor().1.index(), 0, "the column is unchanged");
        assert_eq!(top(t), 0, "no scroll: the screen had room");
    });
}

#[test]
fn a_bottom_of_screen_sixel_scrolls_and_the_scroll_count_moves() {
    // The discriminating case. From the last row of a 50-row screen, advancing
    // past a three-row image scrolls three times: the cursor stays on the last
    // row, the monotonic scroll count moves, and the *next* image would anchor
    // to the moved count. A `cy`-assignment advance would push the cursor off
    // the screen and never touch `scrolled` -- this pins the scroll-path route.
    let mut bytes = Vec::from(b"\x1b[50;1H".as_slice());
    bytes.extend_from_slice(SWIRL);
    advanced(&bytes, bytes.len(), 16, |t| {
        assert_eq!(top(t), 3, "three scrolls at the bottom");
        assert_eq!(t.cursor().0.index(), 49, "the cursor stays on the last row");
    });
}

#[test]
fn the_advance_keeps_the_column_where_the_image_started() {
    // CUP to a non-zero column first, so "the column is unchanged" is
    // distinguishable from "the column happened to start at zero": the cursor
    // stays at the image's left margin (column 10), not carriage-returned to
    // column 0. The DEC source for the exact column is unread; this pins the
    // behaviour baud chose so a reader sees a decision, not an accident.
    let mut bytes = Vec::from(b"\x1b[1;11H".as_slice()); // row 1, column 11 -> index 10
    bytes.extend_from_slice(SWIRL);
    advanced(&bytes, bytes.len(), 16, |t| {
        assert_eq!(
            t.cursor().1.index(),
            10,
            "the column stays at the image's left"
        );
        assert_eq!(t.cursor().0.index(), 3, "three rows below");
    });
}

#[test]
fn no_cell_height_leaves_the_cursor_where_the_image_is() {
    // The sentinel control: a caller that never set a cell height advances
    // nothing, which is today's behaviour and what keeps a headless test or
    // bench unchanged.
    advanced(SWIRL, SWIRL.len(), 0, |t| {
        assert_eq!(t.cursor().0.index(), 0, "the cursor is unmoved");
        assert_eq!(top(t), 0);
    });
}

#[test]
fn the_cursor_advance_is_chunk_invariant() {
    // The advance runs once, in `str_end`, over the body accumulated across
    // reads -- so a read boundary landing mid-body cannot change it.
    for chunk in [1usize, 3, 7, 512, SWIRL.len()] {
        advanced(SWIRL, chunk, 16, |t| {
            assert_eq!(t.cursor().0.index(), 3, "chunk {chunk}: three rows below");
            assert_eq!(top(t), 0, "chunk {chunk}");
        });
    }
}

// Sixels differing only in the `Pb`/`P2` background-transparency selector,
// the second `DCS Pa;Pb;Ph` parameter. Each is a 1x6 red image so the only
// variable is P2.
const P2_OPAQUE: &[u8] = b"\x1bP0;0;0q#0;2;100;0;0~\x1b\\"; // P2 = 0, opaque background
const P2_MODE2: &[u8] = b"\x1bP0;2;0q#0;2;100;0;0~\x1b\\"; // P2 = 2, also opaque for a display
const P2_TRANSPARENT: &[u8] = b"\x1bP0;1;0q#0;2;100;0;0~\x1b\\"; // P2 = 1, what baud does
const P2_NONE: &[u8] = b"\x1bPq#0;2;100;0;0~\x1b\\"; // no parameters at all

#[test]
fn an_opaque_p2_is_counted_and_the_image_still_draws() {
    // P2 = 0 asks for an opaque background baud renders transparent: the image
    // still places (honoured as transparent, the more useful default) and the
    // request is counted rather than dropped unseen.
    run(P2_OPAQUE, P2_OPAQUE.len(), ample(), |t| {
        assert_eq!(
            t.images().sixel_p2_ignored,
            1,
            "the opaque request is counted"
        );
        assert_eq!(
            t.images().placements().count(),
            1,
            "and the image still draws"
        );
    });
    run(P2_MODE2, P2_MODE2.len(), ample(), |t| {
        assert_eq!(t.images().sixel_p2_ignored, 1, "P2 = 2 is counted too");
    });
}

#[test]
fn a_transparent_or_absent_p2_is_not_counted() {
    // P2 = 1 is exactly baud's behaviour, and an absent P2 is no explicit
    // request -- neither is a gap, so neither is counted.
    run(P2_TRANSPARENT, P2_TRANSPARENT.len(), ample(), |t| {
        assert_eq!(t.images().sixel_p2_ignored, 0, "P2 = 1 is honoured");
        assert_eq!(t.images().placements().count(), 1);
    });
    run(P2_NONE, P2_NONE.len(), ample(), |t| {
        assert_eq!(t.images().sixel_p2_ignored, 0, "no P2 is no request");
        assert_eq!(t.images().placements().count(), 1);
    });
}

#[test]
fn the_p2_count_is_chunk_invariant() {
    // The parameters feed `csi` across reads (the `Node::Dcs` digit edges), so
    // a read boundary inside `0;0;0q` must not change what P2 reads.
    for chunk in [1usize, 2, 3, 5, P2_OPAQUE.len()] {
        run(P2_OPAQUE, chunk, ample(), |t| {
            assert_eq!(t.images().sixel_p2_ignored, 1, "chunk {chunk}: counted");
            assert_eq!(t.images().placements().count(), 1, "chunk {chunk}: placed");
        });
    }
}

#[test]
fn a_digit_before_the_dollar_still_reaches_decrqss() {
    // The DCS parameters now feed `csi`, but `$` is not a digit, so a digit
    // before it still reaches DECRQSS -- the collision the parameter capture
    // must not cause. `DCS 1 $ q m` requests the SGR setting and must reply,
    // the same reply `DCS $ q m` gives, because the digit is ignored either way
    // (consumed before this change, captured-but-unread by DECRQSS after it).
    let ws = support::CORPUS;
    let reply = |bytes: &[u8]| {
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, bytes);
        t.reply().to_vec()
    };
    let with_digit = reply(b"\x1bP1$qm\x1b\\");
    let without = reply(b"\x1bP$qm\x1b\\");
    assert!(
        !with_digit.is_empty(),
        "the digit-before-dollar form still replies"
    );
    assert_eq!(
        with_digit, without,
        "the digit is ignored, as when it was consumed"
    );
}

#[test]
fn a_refused_opaque_sixel_is_not_counted() {
    // The counter is a decision input for building the fill, so it counts only
    // images that render. A P2 = 0 Sixel whose pixels do not fit (an arena
    // starved to zero) is refused -- nothing drawn -- and must not inflate the
    // count. This is why P2 is read at the terminator, after the store succeeds,
    // not at `SixelStart`.
    run(P2_OPAQUE, P2_OPAQUE.len(), ImageBudget::of(0, 8192), |t| {
        assert_eq!(
            t.images().placements().count(),
            0,
            "refused, nothing placed"
        );
        assert_eq!(t.images().images_refused, 1, "the refusal is counted");
        assert_eq!(
            t.images().sixel_p2_ignored,
            0,
            "but the ignored-P2 count is not, since nothing drew"
        );
    });
}
