//! The Sixel parser-hook gate: a `DCS ... q` Sixel is decoded and placed in
//! the image store, and it is **chunk-invariant** -- the same image whatever
//! the read boundaries -- because the body accumulates in `sixel_body` across
//! reads before it decodes. A recording carries a Sixel in ~5 KB, far past a
//! `read(2)`, so the read boundary lands mid-body on every real transfer; a
//! parser that reset there would decode the tail of every image as garbage,
//! and only in production.

use baud_core::*;
use baud_domain::{ImageBudget, ImagePx, ImageRefusals, RowRot, SixelClaim};

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

/// What a Sixel program left: the placed image's size and pixels, the screen
/// and the cursor.
type Drawn = (
    Option<(u16, u16, Vec<ColorWord>)>,
    Vec<PackedCell>,
    (Row, Col),
);

fn drawn(t: &Term) -> Drawn {
    let imgs = t.images();
    let image = imgs.placements().next().and_then(|(_, p)| {
        imgs.image(p.image)
            .map(|v| (v.width.raw(), v.height.raw(), v.pixels.to_vec()))
    });
    (image, support::screen(t), t.cursor())
}

/// **A BEL inside a Sixel is payload, and draws nothing; one in its
/// parameters is ignored.** Each program must leave what the same program
/// without the BEL leaves -- the image pixel for pixel, the screen, the
/// cursor -- at every read size, and ring nothing.
///
/// `[[source: williams]]` puts `00-17,19,1C-1F` in `dcs passthrough` (line
/// 388) and ignores them in `dcs param` (361); `[[source: ecma48]]` 5.6 ends
/// a control string with ST alone, and `[[source: ctlseqs]]` lets BEL end an
/// OSC (2023-2024) and nothing else. The body BEL is put, and the Sixel
/// handler drops it before the decoder; here it falls between two data
/// bytes, and `a_c0_or_del_inside_a_sixel_number_splits_nothing` holds the
/// places where a decoder that saw it would read a different image. Before,
/// the BEL ended the Sixel: the image was cut at it and the rest of the body
/// was drawn as `~~-~~`.
#[test]
fn a_bel_inside_a_sixel_is_payload_and_draws_nothing() {
    let without: &[u8] = b"\x1bP0;1q#1;2;100;0;0#1~~~~-~~\x1b\\Z";
    let cases: [(&str, &[u8]); 2] = [
        ("in the body", b"\x1bP0;1q#1;2;100;0;0#1~~\x07~~-~~\x1b\\Z"),
        (
            "in the parameters",
            b"\x1bP0;\x071q#1;2;100;0;0#1~~~~-~~\x1b\\Z",
        ),
    ];
    let budget = ImageBudget::of(64 * 48 + 64, 8192);
    let mut want = None;
    run(without, without.len(), budget, |t| want = Some(drawn(t)));
    let want = want.unwrap();
    assert_eq!(
        want.0.as_ref().map(|i| (i.0, i.1)),
        Some((4, 24)),
        "control: the image without the BEL is 4 by 24, two bands at P1 0's 2:1"
    );
    for (name, bytes) in cases {
        for size in 1..=bytes.len() {
            run(bytes, size, budget, |t| {
                assert_eq!(drawn(t), want, "{name}: read size {size}");
                assert_eq!(t.bells, 0, "{name}: no bell, read size {size}");
            });
        }
    }
}

/// The same program under ISO 8859-1: `ESC % @` first, and the 8-bit
/// introducer and terminator, `90` for `ESC P` and `9C` for `ESC \`.
fn latin1(seven: &[u8]) -> Vec<u8> {
    let body = seven
        .strip_prefix(b"\x1bP")
        .and_then(|b| b.strip_suffix(b"\x1b\\Z"))
        .expect("a 7-bit Sixel followed by Z");
    [b"\x1b%@\x90".as_slice(), body, b"\x9cZ"].concat()
}

/// Where the byte sits, the program, the same program without it, and the
/// image size without it.
type Split = (&'static str, &'static [u8], &'static [u8], (u16, u16));

/// **A C0 or `DEL` inside a Sixel number splits nothing: the Sixel handler
/// drops every C0 it is put before the decoder, and the graph ignores a
/// `DEL`.** Each program must leave what the same program without the byte
/// leaves -- the image pixel for pixel, the screen, the cursor -- at every
/// read size, in both encodings, and ring nothing.
///
/// The decoder reads a number up to its first non-digit. The graph puts a
/// DCS body's `BEL` and its format effectors `08-0D`, `[[source: williams]]`'s
/// `dcs passthrough` (388), so before the handler dropped them `!1 BEL 0~`
/// drew one column where `!10~` draws ten, `#1;2;10 BEL 0;0;0` made register
/// 1 10% red where it is 100%, and `#1 BEL 2` selected register 1 for 12.
/// DEC's Sixel text gives the format effectors no meaning,
/// `[[source: dec-std-070]]` 070-9 §9.1: "These codes shall have no effect
/// on the interpretation of Sixel Control strings"; and its §9.2 lists
/// "7/15 (DELETE) Ignored", as williams' row does, which is where the graph
/// drops it. A `DEL` was stored until 2026-10-10, and this handler dropped
/// it; the format effectors were ignored by the graph, so these cases held
/// before that change too. xterm hands its Sixel parser only `CASE_IGNORE`
/// bytes (`[[source: xterm-reset]]`'s file, 3408-3412), so not the `BEL`,
/// skips `HT LF VT FF CR` as white space, and ends a number on a `BS` and on
/// the `DEL` it hands it (`[[source: xterm-graphics-sixel]]` 541-555),
/// departures that are not followed.
///
/// The controls are the bytes between two tokens -- after the data byte,
/// and between a repeat count and the byte it repeats -- which the decoder
/// skipped before the handler dropped anything, so they pass either way and
/// show the harness is not what changed.
#[test]
fn a_c0_or_del_inside_a_sixel_number_splits_nothing() {
    let cases: [Split; 19] = [
        (
            "BEL inside the repeat count",
            b"\x1bP0;1q#1;2;100;0;0#1!1\x070~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1!10~\x1b\\Z",
            (10, 12),
        ),
        (
            "DEL inside the repeat count",
            b"\x1bP0;1q#1;2;100;0;0#1!1\x7f0~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1!10~\x1b\\Z",
            (10, 12),
        ),
        (
            "BEL inside a colour component",
            b"\x1bP0;1q#1;2;10\x070;0;0#1~~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1~~\x1b\\Z",
            (2, 12),
        ),
        (
            "DEL inside a colour component",
            b"\x1bP0;1q#1;2;10\x7f0;0;0#1~~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1~~\x1b\\Z",
            (2, 12),
        ),
        (
            "BEL inside a register number",
            b"\x1bP0;1q#12;2;0;100;0#1\x072~~\x1b\\Z",
            b"\x1bP0;1q#12;2;0;100;0#12~~\x1b\\Z",
            (2, 12),
        ),
        (
            "DEL inside a register number",
            b"\x1bP0;1q#12;2;0;100;0#1\x7f2~~\x1b\\Z",
            b"\x1bP0;1q#12;2;0;100;0#12~~\x1b\\Z",
            (2, 12),
        ),
        (
            "BEL between # and its number",
            b"\x1bP0;1q#1;2;100;0;0#\x071~~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1~~\x1b\\Z",
            (2, 12),
        ),
        (
            "DEL between # and its number",
            b"\x1bP0;1q#1;2;100;0;0#\x7f1~~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1~~\x1b\\Z",
            (2, 12),
        ),
        (
            "control: BEL between two data bytes",
            b"\x1bP0;1q#1;2;100;0;0#1~\x07~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1~~\x1b\\Z",
            (2, 12),
        ),
        (
            "control: DEL between two data bytes",
            b"\x1bP0;1q#1;2;100;0;0#1~\x7f~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1~~\x1b\\Z",
            (2, 12),
        ),
        (
            "control: BEL between a repeat count and its byte",
            b"\x1bP0;1q#1;2;100;0;0#1!10\x07~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1!10~\x1b\\Z",
            (10, 12),
        ),
        (
            "control: DEL between a repeat count and its byte",
            b"\x1bP0;1q#1;2;100;0;0#1!10\x7f~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1!10~\x1b\\Z",
            (10, 12),
        ),
        (
            "CR inside the repeat count",
            b"\x1bP0;1q#1;2;100;0;0#1!1\r0~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1!10~\x1b\\Z",
            (10, 12),
        ),
        (
            "LF inside a colour component",
            b"\x1bP0;1q#1;2;10\n0;0;0#1~~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1~~\x1b\\Z",
            (2, 12),
        ),
        (
            "HT inside a register number",
            b"\x1bP0;1q#12;2;0;100;0#1\t2~~\x1b\\Z",
            b"\x1bP0;1q#12;2;0;100;0#12~~\x1b\\Z",
            (2, 12),
        ),
        (
            "BS inside the repeat count",
            b"\x1bP0;1q#1;2;100;0;0#1!1\x080~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1!10~\x1b\\Z",
            (10, 12),
        ),
        (
            "VT between # and its number",
            b"\x1bP0;1q#1;2;100;0;0#\x0b1~~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1~~\x1b\\Z",
            (2, 12),
        ),
        (
            "FF inside a colour component",
            b"\x1bP0;1q#1;2;1\x0c00;0;0#1~~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1~~\x1b\\Z",
            (2, 12),
        ),
        (
            "CR LF between two rows",
            b"\x1bP0;1q#1;2;100;0;0#1~~-\r\n~~\x1b\\Z",
            b"\x1bP0;1q#1;2;100;0;0#1~~-~~\x1b\\Z",
            (2, 24),
        ),
    ];
    let budget = ImageBudget::of(64 * 48 + 64, 8192);
    for (name, with, without, size) in cases {
        for (enc, with, without) in [
            ("UTF-8", with.to_vec(), without.to_vec()),
            ("ISO 8859-1", latin1(with), latin1(without)),
        ] {
            let mut want = None;
            run(&without, without.len(), budget, |t| want = Some(drawn(t)));
            let want = want.unwrap();
            assert_eq!(
                want.0.as_ref().map(|i| (i.0, i.1)),
                Some(size),
                "{name}, {enc}: the image without the byte"
            );
            for read in 1..=with.len() {
                run(&with, read, budget, |t| {
                    assert_eq!(drawn(t), want, "{name}, {enc}: read size {read}");
                    assert_eq!(t.bells, 0, "{name}, {enc}: no bell, read size {read}");
                });
            }
        }
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
    assert_eq!(
        t.images().refusals.body,
        1,
        "and counted against the body scratch, whose limit is zero"
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

/// What a reader can see of an image refusal: the store's refusals by cause.
/// Two refusals by different limits must read differently here, or the reader
/// cannot tell which limit to raise. It was every image counter the store
/// exposed, and read `[1, 0, 0, 0, 0]` for both refusals below.
fn refusals(t: &Term) -> ImageRefusals {
    t.images().refusals
}

#[test]
fn a_body_overflow_and_an_arena_overflow_are_told_apart() {
    // The same capture refused twice, by the two limits `[images]` has a knob
    // for: a 1 KiB body scratch under its ~5 KB body, and a zero pixel arena
    // under its 64x48 pixels. Raising `body_kb` fixes the first and
    // `pixels_mb` the second, so a reader must be able to tell them apart.
    let mut body = None;
    let mut arena = None;
    run(
        SWIRL,
        SWIRL.len(),
        ImageBudget::of(64 * 48 + 64, 1024),
        |t| body = Some(refusals(t)),
    );
    run(SWIRL, SWIRL.len(), ImageBudget::of(0, 8192), |t| {
        arena = Some(refusals(t))
    });
    assert_ne!(
        body, arena,
        "a body overflow and an arena overflow read the same: the reader cannot \
         tell which limit bit"
    );
}

/// The capture's raw body: from after the `q` that ends `ESC P 0;1;0 q` to
/// the `ESC \` that ends it -- what the body scratch must hold, 5448 bytes.
fn swirl_body() -> ReadLen {
    let start = SWIRL.iter().position(|&b| b == b'q').unwrap() + 1;
    let end = SWIRL.windows(2).rposition(|w| w == b"\x1b\\").unwrap();
    ReadLen::from_index(end - start)
}

#[test]
fn a_body_past_the_scratch_is_counted_against_the_body_with_its_length() {
    // A 1 KiB scratch under the capture's 5448-byte body, at every read size:
    // the length keeps counting across reads after the scratch stops holding
    // bytes, so the true length is what is kept, not the scratch's. Only the
    // body is blamed -- the arena holds the 64x48 image with room.
    for chunk in [1usize, 7, 1024, SWIRL.len()] {
        run(SWIRL, chunk, ImageBudget::of(64 * 48 + 64, 1024), |t| {
            let r = refusals(t);
            assert_eq!(r.body, 1, "chunk {chunk}: counted against the body");
            assert_eq!(r.body_len, swirl_body(), "chunk {chunk}: its true length");
            assert_eq!(
                (r.empty, r.slot, r.arena),
                (0, 0, 0),
                "chunk {chunk}: no other limit is blamed"
            );
            assert_eq!(t.images().images_refused, r.total(), "chunk {chunk}");
        });
    }
    // The control: a scratch of exactly the body's length holds it.
    let exact = ImageBudget::of(64 * 48 + 64, swirl_body().index());
    run(SWIRL, SWIRL.len(), exact, |t| {
        assert_eq!(refusals(t), ImageRefusals::NONE, "a body that fits");
        assert_eq!(placed_dims(t), Some((64, 48)));
    });
}

#[test]
fn an_image_past_the_arena_is_counted_against_the_arena_with_its_size() {
    // One pixel short of the capture's 64x48: the body fits, the store has a
    // free slot, and no gap holds the pixels. Only the arena is blamed, with
    // the size that did not fit -- and an arena of exactly 64x48 holds it.
    run(
        SWIRL,
        SWIRL.len(),
        ImageBudget::of(64 * 48 - 1, 8192),
        |t| {
            let r = refusals(t);
            assert_eq!(r.arena, 1, "counted against the arena");
            assert_eq!(
                (r.arena_width.raw(), r.arena_height.raw()),
                (64, 48),
                "with the size that did not fit"
            );
            assert_eq!((r.body, r.empty, r.slot), (0, 0, 0), "nothing else");
            assert_eq!(r.body_len, ReadLen::ZERO, "no body length was kept");
            assert_eq!(t.images().images_refused, r.total());
        },
    );
    run(SWIRL, SWIRL.len(), ImageBudget::of(64 * 48, 8192), |t| {
        assert_eq!(refusals(t), ImageRefusals::NONE, "an arena that fits");
        assert_eq!(placed_dims(t), Some((64, 48)));
    });
}

#[test]
fn a_full_slot_table_is_counted_against_the_slots() {
    // 32 one-by-six images, `Owned`'s `NIMAGE`, at one cell: no cell size is
    // set, so nothing frees them and each holds a slot. The arena and the
    // body have room for many more, so the 33rd is refused by the slot table
    // and by nothing else.
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(4096, 8192));
    let mut t = Term::new(owned.storage(), &ws);
    for _ in 0..32 {
        feed(&mut t, TINY);
    }
    assert_eq!(t.images().placements().count(), 32, "all 32 stored");
    assert_eq!(refusals(&t), ImageRefusals::NONE, "and none refused");
    feed(&mut t, TINY);
    let r = refusals(&t);
    assert_eq!(r.slot, 1, "the 33rd has no slot");
    assert_eq!((r.body, r.empty, r.arena), (0, 0, 0), "nothing else");
    assert_eq!(t.images().images_refused, r.total());
}

#[test]
fn a_sixel_with_no_pixels_is_counted_as_empty() {
    // An empty body, and one that defines a colour and paints nothing: no
    // limit refused either and none raised would draw them, so neither is
    // blamed on one.
    for empty in [&b"\x1bPq\x1b\\"[..], b"\x1bPq#0;2;100;0;0\x1b\\"] {
        run(empty, empty.len(), ample(), |t| {
            let r = refusals(t);
            assert_eq!(r.empty, 1, "{empty:?}: counted as empty");
            assert_eq!((r.body, r.slot, r.arena), (0, 0, 0), "{empty:?}");
            assert_eq!(t.images().images_refused, r.total(), "{empty:?}");
        });
    }
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
    // column 0. `[[source: dec-std-070]]` 070-9 §11.3 is the text: "The
    // Horizontal Active position of the ANSI text is not affected by Sixel
    // mode and set exactly to what is was when Sixel mode was entered" (PDF
    // p. 928).
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
const P2_EMPTY: &[u8] = b"\x1bP0;;0q#0;2;100;0;0~\x1b\\"; // P2 present and empty

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
fn a_transparent_p2_is_not_counted() {
    // P2 = 1 is exactly baud's behaviour, so it is no gap and is not counted.
    run(P2_TRANSPARENT, P2_TRANSPARENT.len(), ample(), |t| {
        assert_eq!(t.images().sixel_p2_ignored, 0, "P2 = 1 is honoured");
        assert_eq!(t.images().placements().count(), 1);
    });
}

/// **An absent or empty `P2` is `P2 = 0`, and is counted as 0 is.**
/// `[[source: vt3xx-sixel]]` lists `P2` "0 or 2 (default)", "Pixel positions
/// specified as 0 are set to the current background color", and of an
/// omitted parameter says "If there is no number before the separator, the
/// terminal assumes that parameter is 0"; `[[source: dec-std-070]]` 070-9
/// §7.2 makes 0 the "device default action". So `ESC P q` asks what
/// `ESC P 0;0;0 q` asks. It was not counted until 2026-10-11 while the
/// explicit 0 was, so the number the fill's decision reads missed every
/// `ESC P q` stream, libsixel's among them.
#[test]
fn an_absent_or_empty_p2_is_counted_as_zero_is() {
    for (name, body) in [("absent", P2_NONE), ("empty", P2_EMPTY)] {
        for chunk in [1usize, 2, body.len()] {
            run(body, chunk, ample(), |t| {
                assert_eq!(t.images().sixel_p2_ignored, 1, "{name}, chunk {chunk}");
                assert_eq!(t.images().placements().count(), 1, "{name}: placed");
            });
        }
    }
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

/// A Sixel introducer with GR bytes, beside its GL spelling and whether its
/// `P2` asks for an opaque background.
type Gr = (&'static str, &'static [u8], &'static [u8], usize);

/// 1-by-6 red Sixels whose introducers carry GR bytes, each beside its GL
/// spelling and the count its `P2` makes.
const GR_SIXELS: [Gr; 8] = [
    (
        "B0 BB B1 F1: 0;1 q",
        b"\x1bP\xb0\xbb\xb1\xf1#0;2;100;0;0~\x1b\\Z",
        b"\x1bP0;1q#0;2;100;0;0~\x1b\\Z",
        0,
    ),
    (
        "B0 BB B2 F1: 0;2 q",
        b"\x1bP\xb0\xbb\xb2\xf1#0;2;100;0;0~\x1b\\Z",
        b"\x1bP0;2q#0;2;100;0;0~\x1b\\Z",
        1,
    ),
    (
        "B0 BB B1 B2 F1: 0;12 q",
        b"\x1bP\xb0\xbb\xb1\xb2\xf1#0;2;100;0;0~\x1b\\Z",
        b"\x1bP0;12q#0;2;100;0;0~\x1b\\Z",
        0,
    ),
    (
        "B0 BB 2 B0 F1: 0;20 q, GL then GR",
        b"\x1bP\xb0\xbb2\xb0\xf1#0;2;100;0;0~\x1b\\Z",
        b"\x1bP0;20q#0;2;100;0;0~\x1b\\Z",
        0,
    ),
    (
        "B0 BB B2 0 F1: 0;20 q, GR then GL",
        b"\x1bP\xb0\xbb\xb20\xf1#0;2;100;0;0~\x1b\\Z",
        b"\x1bP0;20q#0;2;100;0;0~\x1b\\Z",
        0,
    ),
    (
        "0 BB B2 q: 0;2 q, GR between GL",
        b"\x1bP0\xbb\xb2q#0;2;100;0;0~\x1b\\Z",
        b"\x1bP0;2q#0;2;100;0;0~\x1b\\Z",
        1,
    ),
    (
        "F1: a bare q",
        b"\x1bP\xf1#0;2;100;0;0~\x1b\\Z",
        b"\x1bPq#0;2;100;0;0~\x1b\\Z",
        1,
    ),
    (
        "B0 BB B2 FF F1: FF ignored",
        b"\x1bP\xb0\xbb\xb2\xff\xf1#0;2;100;0;0~\x1b\\Z",
        b"\x1bP0;2q#0;2;100;0;0~\x1b\\Z",
        1,
    ),
];

/// **Under ISO 8859-1 a GR byte in a Sixel's introducer is read as its 7-bit
/// byte**: `B0-B9` are the digits of `Pa;Pb;Ph`, `BB` is `;`, `F1` is the
/// final `q`, and `FF`, `DEL`'s twin, is ignored where `DEL` is.
/// `[[source: dec-std-070]]` 3.5.4.5 (PDF p. 135): in "the introducer sequence
/// to a Device Control String, the eighth bit will be ignored, and they will
/// be treated as their 7-bit equivalent"; `[[source: williams]]` (462) and
/// `[[source: ecma48]]` §9 read GR as GL there too. The prefix nodes sent the
/// first GR byte to the body, so no image was drawn and its data were
/// discarded.
///
/// The values are what only `sixel_p2_ignored` can see: `P2` of 0 or 2 is
/// counted and 1, 12 or 20 is not, so a GR digit folded as anything but its
/// own value moves the count -- `0;1` read as `0;0` would count one. Each
/// program is held to its GL spelling in all four framings at every read
/// size, the image pixel for pixel, the screen and the cursor; the control is
/// that the GL spelling draws its image and counts what it should.
#[test]
fn a_gr_byte_in_a_sixel_introducer_is_read_as_its_seven_bit_byte_under_latin1() {
    for (name, gr, gl, p2) in GR_SIXELS {
        for ((frame, gr), (_, gl)) in support::latin1_framings(gr)
            .into_iter()
            .zip(support::latin1_framings(gl))
        {
            let mut want = None;
            run(&gl, gl.len(), ample(), |t| {
                assert_eq!(
                    t.images().sixel_p2_ignored,
                    p2,
                    "control: {name}, {frame}, the GL spelling's P2"
                );
                want = Some(drawn(t));
            });
            let want = want.unwrap();
            assert_eq!(
                want.0.as_ref().map(|i| (i.0, i.1)),
                Some((1, 12)),
                "control: {name}, {frame}, the GL spelling draws 1 by 12 at P1 0's 2:1"
            );
            for read in 1..=gr.len() {
                run(&gr, read, ample(), |t| {
                    let at = format!("{name}, {frame}, read size {read}");
                    assert_eq!(drawn(t), want, "{at}");
                    assert_eq!(t.images().sixel_p2_ignored, p2, "{at}: P2");
                });
            }
        }
    }
}

/// **Under UTF-8 the same introducers draw nothing**, the guard beside the
/// test above, green before the change and after it: the GR bytes are a
/// character's there, nothing reads them as a prefix, and the string and its
/// Sixel data are ignored. Only the `Z` after it is drawn, and nothing is
/// refused, so no decoder saw the data.
#[test]
fn a_gr_byte_in_a_sixel_introducer_under_utf8_draws_nothing() {
    let mut z = None;
    run(b"Z", 1, ample(), |t| z = Some(drawn(t)));
    let z = z.unwrap();
    for (name, gr, _, _) in GR_SIXELS {
        for read in 1..=gr.len() {
            run(gr, read, ample(), |t| {
                assert_eq!(drawn(t), z, "UTF-8, {name}, read size {read}");
                assert_eq!(t.images().images_refused, 0, "UTF-8, {name}");
            });
        }
    }
}

/// **A Sixel pixel is drawn at its pixel aspect ratio, where the two DEC
/// texts agree on it.** The envelope's macro parameter `P1` sets it, and a
/// raster command `"Pan;Pad` before any data overrides it.
///
/// `[[source: vt3xx-sixel]]`: `P1` omitted is "2:1 (default)", `0, 1` 2:1,
/// `3, 4` 3:1, `5, 6` 2:1, `7, 8, 9` 1:1, and the raster command "overrides
/// any raster attributes set by the macro parameter". `[[source: dec-std-070]]`
/// 070-9 §7.1 gives `P1` 0, 1 and 5 as 200:100, 3 as 300:100 and 9 as
/// 100:100 (PDF p. 918); §8.1.2 says the raster command "must precede any
/// Sixel, or any sixel positioning command ... or the command will be
/// ignored", and its guideline reads an omitted or zero `Pan` and `Pad` as
/// one (PDF p. 921). So one band is six sixel rows times the ratio. baud drew
/// every image 1:1 before, each band 6 rows tall. The ratios the two tables do
/// not both make whole, `P1` 2, 4, 6, 7 and 8 and a raster `"3;2`, are note
/// AN's item 12, for Jev, and are drawn 1:1 as before.
#[test]
fn a_sixel_pixel_is_drawn_at_its_pixel_aspect_ratio() {
    let cases: [(&str, &[u8], (u16, u16)); 13] = [
        (
            "no P1, no raster: 2:1",
            b"\x1bPq#1;2;100;0;0#1~\x1b\\",
            (1, 12),
        ),
        ("P1 0: 2:1", b"\x1bP0q#1;2;100;0;0#1~\x1b\\", (1, 12)),
        ("P1 1: 2:1", b"\x1bP1q#1;2;100;0;0#1~\x1b\\", (1, 12)),
        ("P1 5: 2:1", b"\x1bP5q#1;2;100;0;0#1~\x1b\\", (1, 12)),
        ("P1 3: 3:1", b"\x1bP3q#1;2;100;0;0#1~\x1b\\", (1, 18)),
        ("P1 9: 1:1", b"\x1bP9q#1;2;100;0;0#1~\x1b\\", (1, 6)),
        (
            "raster 1;1 over P1 0",
            b"\x1bP0q\"1;1#1;2;100;0;0#1~\x1b\\",
            (1, 6),
        ),
        ("raster 3;1", b"\x1bP9q\"3;1#1;2;100;0;0#1~\x1b\\", (1, 18)),
        ("raster 4;2", b"\x1bP9q\"4;2#1;2;100;0;0#1~\x1b\\", (1, 12)),
        (
            "raster ratio omitted",
            b"\x1bPq\";#1;2;100;0;0#1~\x1b\\",
            (1, 6),
        ),
        ("raster 0;0", b"\x1bPq\"0;0#1;2;100;0;0#1~\x1b\\", (1, 6)),
        (
            "raster after data",
            b"\x1bP9q#1;2;100;0;0#1~\"3;1~\x1b\\",
            (2, 6),
        ),
        (
            "raster after a new line",
            b"\x1bP9q#1;2;100;0;0#1~-\"3;1~\x1b\\",
            (1, 12),
        ),
    ];
    for (name, body, dims) in cases {
        for chunk in [1usize, 2, 3, body.len()] {
            run(body, chunk, ample(), |t| {
                assert_eq!(placed_dims(t), Some(dims), "{name}, chunk {chunk}");
            });
        }
    }
}

/// **At 2:1 each sixel row is two pixels tall, and the cursor moves past all
/// of them.** `t` is `0b110101`, rows 1, 0, 1, 0, 1, 1 from the top
/// (`[[source: vt3xx-sixel]]`); at 2:1 each is two device rows. With an
/// 8-pixel cell the 12-row image takes two text rows where 1:1 took one.
#[test]
fn a_two_to_one_sixel_doubles_each_row_and_the_cursor_follows() {
    let body = b"\x1bPq#1;2;100;0;0#1t\x1b\\";
    run(body, body.len(), ample(), |t| {
        let (img, _, _) = drawn(t);
        let (w, h, px) = img.expect("placed");
        assert_eq!((w, h), (1, 12));
        let lit: Vec<bool> = px.iter().map(|c| !c.is_default()).collect();
        let want = [
            true, true, false, false, true, true, false, false, true, true, true, true,
        ];
        assert_eq!(lit, want, "each sixel row is two device rows");
    });
    advanced(body, body.len(), 8, |t| {
        assert_eq!(t.cursor().0.index(), 2, "12 rows at 8 a cell is two rows");
    });
}
