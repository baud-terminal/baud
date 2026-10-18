//! A picture placed one cell at a time is placed whole.
//!
//! The text sets no limit on how many placements a terminal keeps: "there
//! can be many placements per image", each "uniquely identified by the
//! pair of the image id and the placement id", and its only quota is on
//! image data (`[[source: kitty-graphics]]`, "Display images on screen",
//! "Image persistence and storage quotas"). A client may cut one image
//! into a placement per cell, each with its own source rectangle, and
//! yazi 26.9.1's driver for a terminal it does not know by name does
//! exactly that: one chunked `a=t`, then a cursor move and an
//! `a=p,i,p,x,y,w,h,c=1,r=1,z=-1,C=1` per cell of the preview, all at
//! `q=2`, and per-cell `a=d,d=i,i,p` to erase. The store held 64
//! placements, so a 44 by 15 preview showed one row and twenty cells of
//! the next and nothing below, refused silently under `q=2`.
//!
//! The commands here are that shape on a synthetic image, fed at the
//! 1024-byte reads a Darwin tty hands over (and others), and every
//! placement must be live, at its cell, with its rectangle.

use baud_core::*;
use baud_domain::{DecodeBudget, DecodeProgress, ImageBudget, ImagePx, KittyId, KittyPlacement};

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

/// The preview's cells: yazi's, at 8x18 cells, for a 352x264 image.
const COLS: u32 = 44;
const ROWS: u32 = 15;
/// Where it is drawn: one-based row and column of its top-left cell.
const TOP: u32 = 3;
const LEFT: u32 = 31;
/// A small cell, so the image is small: 176 by 90 pixels.
const CW: u32 = 4;
const CH: u32 = 6;
const ID: u32 = 2_701_970;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            match k <= q.len() {
                true => out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => out.push(b'='),
            }
        }
    }
    out
}

/// The image's RGB bytes: every pixel distinct within a cell's reach.
fn rgb() -> Vec<u8> {
    let (w, h) = (COLS * CW, ROWS * CH);
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            v.extend_from_slice(&[x % 251, y % 251, (x + y) % 7].map(|c| u8::try_from(c).unwrap()));
        }
    }
    v
}

/// yazi 26.9.1's `KgpOld::encode`: base64 cut into 4096-byte chunks, the
/// first carrying the keys and `m=1` while more follow.
fn transmit() -> Vec<u8> {
    let data = b64(&rgb());
    let chunks: Vec<&[u8]> = data.chunks(4096).collect();
    let mut v = Vec::new();
    for (k, c) in chunks.iter().enumerate() {
        let more = u8::from(k + 1 < chunks.len());
        match k {
            0 => v.extend_from_slice(
                format!(
                    "\x1b_Gq=2,a=t,i={ID},f=24,s={},v={},m={more};",
                    COLS * CW,
                    ROWS * CH
                )
                .as_bytes(),
            ),
            _ => v.extend_from_slice(format!("\x1b_Gm={more};").as_bytes()),
        }
        v.extend_from_slice(c);
        v.extend_from_slice(b"\x1b\\");
    }
    v
}

/// yazi 26.9.1's `KgpOld::place`: a cursor move and a put per cell,
/// placement ids from 1 in row order.
fn place() -> Vec<u8> {
    let mut v = Vec::new();
    for y in 0..ROWS {
        for x in 0..COLS {
            let p = y * COLS + x + 1;
            v.extend_from_slice(
                format!(
                    "\x1b[{};{}H\x1b_Gq=2,a=p,i={ID},p={p},x={},y={},w={CW},h={CH},\
                     c=1,r=1,z=-1,C=1\x1b\\",
                    TOP + y,
                    LEFT + x,
                    x * CW,
                    y * CH,
                )
                .as_bytes(),
            );
        }
    }
    v
}

fn feed_reads(t: &mut Term<'_>, bytes: &[u8], read: ReadLen) {
    for chunk in bytes.chunks(read.index()) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
        assert!(t.reply().is_empty(), "q=2: nothing is answered");
    }
}

/// A count of cells as an index.
fn cells(n: u32) -> usize {
    usize::try_from(n).unwrap()
}

fn with<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(
        ImagePx::of(u16::try_from(CW).unwrap()),
        ImagePx::of(u16::try_from(CH).unwrap()),
    );
    f(&mut t)
}

/// Every placement of the preview, each at its own cell with its own cell
/// of the source.
fn assert_whole(t: &Term<'_>, how: &str) {
    let imgs = t.images();
    assert_eq!(imgs.placements_refused, 0, "{how}: none refused");
    assert_eq!(t.kitty().refused, 0, "{how}: no command refused");
    assert_eq!(
        imgs.placements().count(),
        cells(COLS * ROWS),
        "{how}: one placement a cell"
    );
    let first = imgs.placements().map(|(_, s)| s.line).min().unwrap();
    let mut seen = vec![false; cells(COLS * ROWS)];
    for (_, s) in imgs.placements() {
        let p = s.kitty.id.raw();
        assert!((1..=COLS * ROWS).contains(&p), "{how}: placement id {p}");
        let (x, y) = ((p - 1) % COLS, (p - 1) / COLS);
        assert!(!seen[cells(p - 1)], "{how}: placement {p} once");
        seen[cells(p - 1)] = true;
        assert_eq!(s.col.index(), cells(LEFT - 1 + x), "{how}: {p}'s column");
        assert_eq!(
            s.line.since(first).unwrap().index(),
            cells(y),
            "{how}: {p}'s row"
        );
        let k = s.kitty;
        let got = [k.src_x, k.src_y, k.src_w, k.src_h].map(|v| u32::from(v.raw()));
        assert_eq!(got, [x * CW, y * CH, CW, CH], "{how}: {p}'s rectangle");
        assert_eq!(s.z.raw(), -1, "{how}: {p} under the text");
    }
}

/// **yazi's per-cell preview is placed whole**: 660 placements, at the
/// 1024- and 2048-byte reads the capture arrived in, at one byte, at
/// 4095 and whole. On the commit before, 64 were placed and 596 refused.
#[test]
fn a_picture_placed_one_cell_at_a_time_is_placed_whole() {
    let stream = [transmit(), place()].concat();
    for read in [1024, 2048, 1, 4095, stream.len()] {
        with(|t| {
            feed_reads(t, &stream, ReadLen::from_index(read));
            assert_whole(t, &format!("reads of {read}"));
        });
    }
}

/// **The per-cell erase takes every placement and the image stays**:
/// yazi's `image_erase`, an `a=d,d=i,i,p` per cell, lowercase, so the
/// named image is kept for the next preview, which places whole again.
#[test]
fn a_per_cell_erase_takes_every_placement_and_the_preview_places_again() {
    with(|t| {
        feed_reads(
            t,
            &[transmit(), place()].concat(),
            ReadLen::from_index(1024),
        );
        let mut erase = Vec::new();
        for p in 1..=COLS * ROWS {
            erase.extend_from_slice(format!("\x1b_Gq=2,a=d,d=i,i={ID},p={p}\x1b\\").as_bytes());
        }
        feed_reads(t, &erase, ReadLen::from_index(1024));
        assert_eq!(t.images().placements().count(), 0, "all erased");
        feed_reads(t, &place(), ReadLen::from_index(1024));
        assert_whole(t, "placed again");
    });
}

/// **Placements freed out of order leave exactly the rest**: the one
/// before the last, the first and every seventh go, the last staying
/// above a freed slot, and each survivor is still found --
/// by the iteration the renderer draws from and by the `(i, p)` a
/// replacing put looks up, which moves it rather than adding one.
#[test]
fn placements_freed_out_of_order_leave_exactly_the_rest() {
    with(|t| {
        feed_reads(
            t,
            &[transmit(), place()].concat(),
            ReadLen::from_index(1024),
        );
        let n = COLS * ROWS;
        let gone = |p: u32| p == n - 1 || p == 1 || p.is_multiple_of(7);
        let mut erase = Vec::new();
        for p in (1..=n).filter(|&p| gone(p)) {
            erase.extend_from_slice(format!("\x1b_Gq=2,a=d,d=i,i={ID},p={p}\x1b\\").as_bytes());
        }
        feed_reads(t, &erase, ReadLen::from_index(1024));
        let mut left: Vec<u32> = t
            .images()
            .placements()
            .map(|(_, s)| s.kitty.id.raw())
            .collect();
        left.sort_unstable();
        let want: Vec<u32> = (1..=n).filter(|&p| !gone(p)).collect();
        assert_eq!(left, want, "exactly the rest");
        // Each survivor replaced in place: the count does not move.
        let before = t.images().placements().count();
        feed_reads(t, &place(), ReadLen::from_index(1024));
        assert_eq!(
            t.images().placements().count(),
            cells(n),
            "the gone come back"
        );
        assert!(before < cells(n));
        let p2 = t
            .images()
            .placements()
            .filter(|(_, s)| s.kitty.id == KittyPlacement::of(2))
            .count();
        assert_eq!(p2, 1, "a replacing put moves its placement");
    });
}

/// **The table holds a placement for every cell of the screen it was
/// built for**, so a per-cell picture covering the whole screen fits, and
/// the one past that is refused, counted, and answered `ENOSPC` when
/// asked; a slot freed in the full table, low in it, is the next put's.
/// A 24x80 screen.
#[test]
fn the_table_holds_a_placement_for_every_cell_of_the_screen() {
    with(|t| {
        let slots = t.images().placement_slots();
        assert!(
            slots >= usize::from(WS.ws_row) * usize::from(WS.ws_col),
            "{slots}"
        );
        feed_reads(t, &transmit(), ReadLen::from_index(1024));
        let mut v = Vec::new();
        for p in 1..=slots {
            v.extend_from_slice(
                format!("\x1b_Gq=2,a=p,i={ID},p={p},w={CW},h={CH},c=1,r=1,C=1\x1b\\").as_bytes(),
            );
        }
        feed_reads(t, &v, ReadLen::from_index(4096));
        assert_eq!(t.images().placements().count(), slots, "every cell");
        assert_eq!(t.images().placements_refused, 0);
        let over = format!("\x1b_Ga=p,i={ID},p={},c=1,r=1,C=1\x1b\\", slots + 1);
        t.write_at(BytePos::ZERO, over.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(over.len()));
        assert_eq!(
            String::from_utf8_lossy(t.reply()),
            format!(
                "\x1b_Gi={ID},p={};ENOSPC:No room for another placement\x1b\\",
                slots + 1
            ),
        );
        assert_eq!(t.images().placements_refused, 1);
        t.clear_reply();
        let again = format!(
            "\x1b_Gq=2,a=d,d=i,i={ID},p=5\x1b\\\x1b_Gq=2,a=p,i={ID},p={},c=1,r=1,C=1\x1b\\",
            slots + 2
        );
        feed_reads(t, again.as_bytes(), ReadLen::from_index(4096));
        assert_eq!(t.images().placements_refused, 1, "the freed slot is taken");
        assert_eq!(t.images().placements().count(), slots, "full again");
    });
}

/// yazi 26.9.1's `image_erase`: a lowercase `a=d,d=i,i,p` per cell.
fn erase() -> Vec<u8> {
    let mut v = Vec::new();
    for p in 1..=COLS * ROWS {
        v.extend_from_slice(format!("\x1b_Gq=2,a=d,d=i,i={ID},p={p}\x1b\\").as_bytes());
    }
    v
}

/// **A put and a delete that name a placement find it by its key**: "If
/// you send two placements with the same image id and placement id the
/// second one will replace the first", and a delete with `i` and `p`
/// takes "only the placement with the specified image id and placement
/// id" (`[[source: kitty-graphics]]`, "Display images on screen",
/// "Deleting images"). Each finds its placement by the pair, a few slots
/// examined, not every slot placed before it: on the commit before,
/// yazi's first 660 puts examined 217,470 slots, 660 that replace or
/// erase 435,600, and this test 3,483,480 for its 6,600 lookups. Three rounds of replace, erase and place again, so a lookup that
/// degrades as slots are freed and taken again shows.
#[test]
fn a_put_and_a_delete_find_their_placement_by_its_key() {
    with(|t| {
        let read = ReadLen::from_index(1024);
        feed_reads(t, &transmit(), read);
        let start = t.images().key_probes;
        let puts = cells(COLS * ROWS);
        let mut lookups = 0;
        feed_reads(t, &place(), read);
        lookups += puts;
        for round in 0..3 {
            feed_reads(t, &place(), read);
            assert_whole(t, &format!("replaced, round {round}"));
            feed_reads(t, &erase(), read);
            assert_eq!(t.images().placements().count(), 0, "round {round}");
            feed_reads(t, &place(), read);
            assert_whole(t, &format!("placed again, round {round}"));
            lookups += 3 * puts;
        }
        let spent = t.images().key_probes - start;
        assert!(
            spent <= 3 * lookups,
            "{spent} entries examined for {lookups} lookups"
        );
    });
}

/// **A replacing put takes the new geometry and keeps the image**: the
/// pair names the placement, so the second put of `(i, p)` moves it to
/// its cell and source rectangle, the image is not freed between the two
/// (a lowercase delete of every other placement leaves it named), and the
/// same `p` on another image is another placement.
#[test]
fn a_replacing_put_takes_the_new_geometry() {
    with(|t| {
        let read = ReadLen::from_index(1024);
        feed_reads(t, &[transmit(), place()].concat(), read);
        let moved =
            format!("\x1b[1;1H\x1b_Gq=2,a=p,i={ID},p=1,x=8,y=12,w=4,h=6,c=1,r=1,z=5,C=1\x1b\\");
        feed_reads(t, moved.as_bytes(), read);
        let imgs = t.images();
        assert_eq!(
            imgs.placements().count(),
            cells(COLS * ROWS),
            "replaced, not added"
        );
        assert_eq!(imgs.images_dropped, 0, "the image stays");
        let ones: Vec<_> = imgs
            .placements()
            .filter(|(_, s)| s.kitty.id == KittyPlacement::of(1))
            .map(|(_, s)| *s)
            .collect();
        assert_eq!(ones.len(), 1, "one placement 1");
        let s = ones[0];
        assert_eq!((s.col.index(), s.z.raw()), (0, 5), "at its new cell and z");
        assert_eq!(
            [s.kitty.src_x, s.kitty.src_y].map(|v| v.raw()),
            [8, 12],
            "with its new rectangle"
        );
        let other = format!("\x1b_Gq=2,a=T,i={},f=24,s=1,v=1,p=1,C=1;AAAA\x1b\\", ID + 1);
        feed_reads(t, other.as_bytes(), read);
        assert_eq!(
            t.images().placements().count(),
            cells(COLS * ROWS) + 1,
            "the same p on another image is another placement"
        );
    });
}

/// **Puts and deletes in any order leave each pair found by its key**: a
/// fixed pseudo-random run of 20,000 puts and deletes over 3,000 placement
/// ids, at most 1,920 live in the 24x80 table, against a model of which
/// ids are placed and where. Ids that collide in the index, entries
/// shifted back by a delete and slots taken again are all on its path; a
/// pair the index lost would be placed twice, and one it kept after its
/// delete would take a live placement with it.
#[test]
fn puts_and_deletes_in_any_order_leave_each_pair_found() {
    with(|t| {
        feed_reads(t, &transmit(), ReadLen::from_index(4096));
        let mut model = std::collections::BTreeMap::<u32, u32>::new();
        let mut seed = 0x2545_f491_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let cap = t.images().placement_slots();
        for step in 0..20_000 {
            let pid = next() % 3000 + 1;
            let col = next() % 80;
            let put = next() % 3 != 0;
            let cmd = match put && (model.contains_key(&pid) || model.len() < cap) {
                true => {
                    model.insert(pid, col);
                    format!(
                        "\x1b[1;{}H\x1b_Gq=2,a=p,i={ID},p={pid},c=1,r=1,C=1\x1b\\",
                        col + 1
                    )
                }
                false => {
                    model.remove(&pid);
                    format!("\x1b_Gq=2,a=d,d=i,i={ID},p={pid}\x1b\\")
                }
            };
            feed_reads(t, cmd.as_bytes(), ReadLen::from_index(4096));
            if step % 1000 == 999 || step == 19_999 {
                let mut got: Vec<(u32, u32)> = t
                    .images()
                    .placements()
                    .map(|(_, s)| (s.kitty.id.raw(), u32::try_from(s.col.index()).unwrap()))
                    .collect();
                got.sort_unstable();
                let want: Vec<(u32, u32)> = model.iter().map(|(&p, &c)| (p, c)).collect();
                assert_eq!(got, want, "after step {step}");
            }
        }
        assert_eq!(t.images().placements_refused, 0);
    });
}

/// **A reset takes every placement out of the index**: eight rounds of a
/// preview placed, in row order and then backwards, and `RIS`, and every
/// put still finds its pair in a step or so. An index that kept the
/// placements a reset dropped would fill with entries naming slots
/// other placements hold, and every probe would lengthen with it.
#[test]
fn a_reset_takes_every_placement_out_of_the_index() {
    with(|t| {
        let read = ReadLen::from_index(4096);
        let start = t.images().key_probes;
        let forward = place();
        let mut backward = Vec::new();
        for put in forward.split_inclusive(|&b| b == b'\\').rev() {
            backward.extend_from_slice(put);
        }
        for round in 0..8 {
            feed_reads(t, &transmit(), read);
            let order = match round % 2 {
                0 => &forward,
                _ => &backward,
            };
            feed_reads(t, order, read);
            assert_whole(t, &format!("round {round}"));
            feed_reads(t, b"\x1bc", read);
            assert_eq!(t.images().placements().count(), 0, "round {round}");
        }
        let lookups = 8 * cells(COLS * ROWS);
        let spent = t.images().key_probes - start;
        assert!(
            spent <= 3 * lookups,
            "{spent} entries examined for {lookups} lookups"
        );
    });
}

/// **A preview whose rows leave the history goes, and its image stays**:
/// "images must be scrolled along with text" (`[[source:
/// kitty-graphics]]`, "Interaction with other terminal actions"), and
/// kitty removes a placement scrolled off its history and, with its
/// last, the image only when nothing can name it again (`[[source:
/// kitty-graphics-c]]`, `scroll_filter_func` and `modify_refs`). A 30-line
/// history: the 660 placements go, each slot is free, the image `i`
/// names is kept and places whole again, and an image placed with no
/// id or number goes with its placement.
#[test]
fn a_preview_scrolled_out_of_the_history_frees_its_slots_and_keeps_its_image() {
    let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(30))
        .with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(
        ImagePx::of(u16::try_from(CW).unwrap()),
        ImagePx::of(u16::try_from(CH).unwrap()),
    );
    let read = ReadLen::from_index(1024);
    feed_reads(&mut t, &[transmit(), place()].concat(), read);
    feed_reads(&mut t, b"\x1b_Gq=2,a=T,f=24,s=1,v=1,C=1;AAAA\x1b\\", read);
    assert_eq!(t.images().placements().count(), cells(COLS * ROWS) + 1);
    // The bottom row, then the history's 30 lines and the screen's 24
    // scrolled past every placement's row.
    feed_reads(
        &mut t,
        &[b"\x1b[24;1H".as_slice(), &[b'\n'; 60]].concat(),
        read,
    );
    let imgs = t.images();
    assert_eq!(imgs.placements().count(), 0, "every placement went");
    assert_eq!(
        imgs.placements_expired,
        cells(COLS * ROWS) + 1,
        "and is counted"
    );
    assert_eq!(imgs.images_dropped, 1, "the nameless image went with it");
    assert!(
        imgs.kitty_image(KittyId::of(ID)).is_some(),
        "the named image stays"
    );
    feed_reads(&mut t, &place(), read);
    assert_whole(&t, "placed again from the kept image");
}
