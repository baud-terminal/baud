//! kitty's relative placements (`[[source: kitty-graphics]]`, "Relative
//! placements"): a placement made with `P` and `Q` sits at its parent's
//! top-left cell plus `H` columns and `V` rows, moves when the parent
//! moves, goes when the parent goes -- its image with it when that was the
//! image's last placement -- and never moves the cursor. A parent that
//! does not exist is ENOPARENT, a chain that would loop ECYCLE, one past
//! eight parents ETOODEEP, and a virtual placement with a parent EINVAL,
//! each in kitty's reply shape (`[[source: kitty-graphics-c]]`
//! `handle_put_command`, `has_good_ancestry`). A put that makes one, each
//! refusal, the ninth parent and a parent's delete are fed at every read
//! size.

use baud_core::*;
use baud_domain::{
    DecodeBudget, DecodeProgress, ImageBudget, ImagePx, KittyId, KittyPlacement, LineIdx,
};

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

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

fn apc(ctrl: &str) -> Vec<u8> {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    v.extend_from_slice(b"\x1b\\");
    v
}

/// A 2x2 RGB image `id` stored with no placement, its reply silenced.
fn store(id: u32) -> Vec<u8> {
    let mut v = format!("\x1b_Ga=t,f=24,s=2,v=2,i={id},q=2;").into_bytes();
    v.extend(b64(&[9u8; 12]));
    v.extend_from_slice(b"\x1b\\");
    v
}

/// Feed `bytes` in reads of `n` bytes, finishing decodes, keeping replies.
fn feed_by(t: &mut Term<'_>, bytes: &[u8], n: usize) -> Vec<u8> {
    let mut replies = Vec::new();
    for read in bytes.chunks(n.max(1)) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
        while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
        replies.extend_from_slice(t.reply());
        t.clear_reply();
    }
    replies
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    feed_by(t, bytes, bytes.len())
}

fn with<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    f(&mut t)
}

/// Every placement of the image with kitty id `id`, as (row from the first
/// line, column, placement id), sorted.
fn at(t: &Term<'_>, id: u32) -> Vec<(usize, usize, u32)> {
    let Some(img) = t.images().kitty_image(KittyId::of(id)) else {
        return Vec::new();
    };
    let mut v: Vec<_> = t
        .images()
        .placements()
        .filter(|(_, p)| p.image == img)
        .map(|(_, p)| {
            let row = p.line.since(LineIdx::FIRST).map_or(0, |r| r.index());
            (row, p.col.index(), p.kitty.id.raw())
        })
        .collect();
    v.sort();
    v
}

/// Images 1 to 9 stored, and image 1 placed at row 5, column 10 (1-based
/// 6;11) with placement id 1, the cursor kept.
fn parent(t: &mut Term<'_>) {
    for id in 1..=9 {
        assert!(feed(t, &store(id)).is_empty());
    }
    assert!(feed(t, b"\x1b[6;11H").is_empty());
    assert!(feed(t, &apc("a=p,i=1,p=1,C=1,q=2")).is_empty());
    assert_eq!(at(t, 1), vec![(5, 10, 1)], "the premise");
}

/// **A relative placement sits at its parent's top-left cell plus `H`
/// and `V`**, "Positive values move right and down. Negative values move
/// left and up", and the cursor stays wherever it was, `C` or not: "the
/// cursor must not move after a relative position, regardless of the value
/// of the C key". Answered `OK`, at every read size. On the commit before,
/// EINVAL and nothing placed.
#[test]
fn a_relative_placement_sits_at_its_parent_plus_h_and_v() {
    for (keys, want) in [
        ("H=3,V=2", (7, 13)),
        ("H=-4,V=-1", (4, 6)),
        ("", (5, 10)),
        ("C=1,H=1", (5, 11)),
    ] {
        let cmd = apc(&format!("a=p,i=2,p=7,P=1,Q=1,{keys}"));
        for n in 1..=cmd.len() {
            with(|t| {
                parent(t);
                feed(t, b"\x1b[20;3H");
                let before = t.cursor();
                let r = feed_by(t, &cmd, n);
                assert_eq!(
                    String::from_utf8_lossy(&r),
                    "\x1b_Gi=2,p=7;OK\x1b\\",
                    "{keys} by {n}"
                );
                assert_eq!(at(t, 2), vec![(want.0, want.1, 7)], "{keys} by {n}");
                assert_eq!(t.cursor(), before, "{keys} by {n}: the cursor stays");
            });
        }
    }
}

/// **`a=T` with a parent places relative too**, and moves no cursor.
#[test]
fn a_transmission_with_a_parent_is_placed_relative() {
    with(|t| {
        parent(t);
        feed(t, b"\x1b[20;3H");
        let before = t.cursor();
        let mut cmd = b"\x1b_Ga=T,f=24,s=2,v=2,i=20,P=1,H=2,V=3;".to_vec();
        cmd.extend(b64(&[1u8; 12]));
        cmd.extend_from_slice(b"\x1b\\");
        let r = feed(t, &cmd);
        assert_eq!(r, b"\x1b_Gi=20;OK\x1b\\");
        assert_eq!(at(t, 20), vec![(8, 12, 0)]);
        assert_eq!(t.cursor(), before);
    });
}

/// **Off the screen's edge, by what it hangs over**: a parent at column
/// 0 and `H=-2` anchors its child at column 0, two columns over the edge,
/// as the text's "Negative values move left" puts it.
#[test]
fn a_relative_placement_left_of_column_zero_hangs_over_the_edge() {
    with(|t| {
        for id in 1..=2 {
            feed(t, &store(id));
        }
        feed(t, b"\x1b[3;1H");
        feed(t, &apc("a=p,i=1,p=1,C=1,q=2"));
        feed(t, &apc("a=p,i=2,p=1,P=1,Q=1,H=-2,q=2"));
        let img = t.images().kitty_image(KittyId::of(2)).unwrap();
        let (_, p) = t
            .images()
            .placements()
            .find(|(_, p)| p.image == img)
            .expect("placed");
        assert_eq!(p.col.index(), 0);
        assert_eq!(p.kitty.over_cols.index(), 2, "two columns over the edge");
    });
}

/// **With no `Q`, the parent image's oldest placement** (the text names
/// no rule and kitty takes the first of its map).
#[test]
fn with_no_q_the_parent_is_the_oldest_placement() {
    with(|t| {
        parent(t);
        feed(t, b"\x1b[2;2H");
        feed(t, &apc("a=p,i=1,p=2,C=1,q=2"));
        let r = feed(t, &apc("a=p,i=2,P=1,H=1"));
        assert_eq!(r, b"\x1b_Gi=2;OK\x1b\\");
        assert_eq!(at(t, 2), vec![(5, 11, 0)], "against p=1, the older");
    });
}

/// **A re-put keeps a placement's age**: kitty updates the one ref in
/// place (`[[source: kitty-graphics-c]]` `handle_put_command()`), so
/// `p=1`, made first and moved after `p=2` was made, is still the oldest
/// and a child with no `Q` is placed against it, at every read size.
#[test]
fn a_re_put_parent_is_still_the_oldest() {
    let cmd = apc("a=p,i=2,P=1,H=1,V=1");
    for n in 1..=cmd.len() {
        with(|t| {
            parent(t);
            feed(t, b"\x1b[11;11H");
            feed(t, &apc("a=p,i=1,p=2,C=1,q=2"));
            feed(t, b"\x1b[3;3H");
            feed(t, &apc("a=p,i=1,p=1,C=1,q=2"));
            assert_eq!(at(t, 1), vec![(2, 2, 1), (10, 10, 2)], "the premise");
            let r = feed_by(t, &cmd, n);
            assert_eq!(r, b"\x1b_Gi=2;OK\x1b\\", "by {n}");
            assert_eq!(at(t, 2), vec![(3, 3, 0)], "by {n}: against p=1");
        });
    }
}

/// **Every refusal in kitty's code and shape, at every read size**, and
/// none places anything or moves the cursor: ENOPARENT for a parent image
/// that does not exist, one with no placement, with a `Q` or without, as
/// kitty tests the placements before the `Q`, and a `Q` no placement has;
/// EINVAL for a virtual placement with a parent and for a placement named
/// as its own parent; ECYCLE for a re-put under its own child.
#[test]
fn every_refusal_is_answered_in_kittys_shape() {
    let cases: [(&str, &str, &str); 7] = [
        (
            "",
            "a=p,i=2,P=50",
            "i=2;ENOPARENT:Put command refers to a parent image that does not exist",
        ),
        (
            "",
            "a=p,i=2,P=3",
            "i=2;ENOPARENT:Put command refers to a parent image that has no placements",
        ),
        (
            "",
            "a=p,i=2,P=3,Q=1",
            "i=2;ENOPARENT:Put command refers to a parent image that has no placements",
        ),
        (
            "",
            "a=p,i=2,P=1,Q=9",
            "i=2;ENOPARENT:Put command refers to a parent image placement that does not exist",
        ),
        (
            "",
            "a=p,i=2,U=1,P=1,Q=1",
            "i=2;EINVAL:Put command creating a virtual placement cannot refer to a parent",
        ),
        (
            "",
            "a=p,i=1,p=1,P=1,Q=1",
            "i=1,p=1;EINVAL:Put command refers to itself as its own parent",
        ),
        (
            "a=p,i=2,p=1,P=1,Q=1,q=2",
            "a=p,i=1,p=1,P=2,Q=1",
            "i=1,p=1;ECYCLE:This parent reference creates a cycle",
        ),
    ];
    for (before, cmd, want) in cases {
        let cmd = apc(cmd);
        for n in 1..=cmd.len() {
            with(|t| {
                parent(t);
                if !before.is_empty() {
                    assert!(feed(t, &apc(before)).is_empty());
                }
                feed(t, b"\x1b[20;3H");
                let (cursor, one, two) = (t.cursor(), at(t, 1), at(t, 2));
                let r = feed_by(t, &cmd, n);
                assert_eq!(
                    String::from_utf8_lossy(&r),
                    format!("\x1b_G{want}\x1b\\"),
                    "by {n}"
                );
                assert_eq!((at(t, 1), at(t, 2)), (one, two), "{want}: nothing moved");
                assert_eq!(t.cursor(), cursor, "{want}: the cursor stays");
            });
        }
    }
}

/// **Eight parents are allowed and a ninth is ETOODEEP**, kitty's
/// `PARENT_DEPTH_LIMIT` against the text's "must allow a chain of length
/// at least 8": images 2 to 9 each relative to the one before, then
/// image 10 under image 9.
#[test]
fn eight_parents_are_allowed_and_a_ninth_is_too_deep() {
    with(|t| {
        parent(t);
        feed(t, &store(10));
        for id in 2..=9 {
            let r = feed(t, &apc(&format!("a=p,i={id},p=1,P={},Q=1,H=1", id - 1)));
            assert_eq!(
                String::from_utf8_lossy(&r),
                format!("\x1b_Gi={id},p=1;OK\x1b\\")
            );
        }
        assert_eq!(at(t, 9), vec![(5, 18, 1)], "eight columns along");
        let cmd = apc("a=p,i=10,p=1,P=9,Q=1,H=1");
        for n in 1..=cmd.len() {
            let r = feed_by(t, &cmd, n);
            assert_eq!(
                String::from_utf8_lossy(&r),
                "\x1b_Gi=10,p=1;ETOODEEP:Too many levels of parent references\x1b\\",
                "by {n}"
            );
            assert!(at(t, 10).is_empty());
        }
    });
}

/// **A parent's deletion takes its children, down the chain, and an image
/// left with no placement goes with it**: "If its parent is deleted, it
/// is deleted as well. If the image that the relative placement is a
/// placement of, has no more placements, the image is deleted as well."
/// The lowercase delete keeps the parent's own image, as it keeps any;
/// a child's image with a placement elsewhere stays.
#[test]
fn a_parents_deletion_takes_its_children_and_their_images() {
    let cmd = apc("a=d,d=i,i=1");
    for n in 1..=cmd.len() {
        with(|t| {
            parent(t);
            feed(t, &apc("a=p,i=2,p=1,P=1,Q=1,V=1,q=2"));
            feed(t, &apc("a=p,i=3,p=1,P=2,Q=1,V=1,q=2"));
            feed(t, b"\x1b[1;1H");
            feed(t, &apc("a=p,i=4,p=9,C=1,q=2"));
            feed(t, &apc("a=p,i=4,p=1,P=3,Q=1,H=2,q=2"));
            assert_eq!(at(t, 3), vec![(7, 10, 1)], "the premise");
            assert_eq!(at(t, 4).len(), 2, "the premise");
            feed_by(t, &cmd, n);
            let gone = at(t, 1).is_empty() && at(t, 2).is_empty() && at(t, 3).is_empty();
            assert!(gone, "by {n}");
            assert_eq!(
                at(t, 4),
                vec![(0, 0, 9)],
                "by {n}: its other placement stays"
            );
            let imgs = t.images();
            assert!(
                imgs.kitty_image(KittyId::of(1)).is_some(),
                "lowercase keeps it"
            );
            assert!(
                imgs.kitty_image(KittyId::of(2)).is_none(),
                "went with its last"
            );
            assert!(
                imgs.kitty_image(KittyId::of(3)).is_none(),
                "went with its last"
            );
            assert!(
                imgs.kitty_image(KittyId::of(4)).is_some(),
                "placed elsewhere"
            );
        });
    }
    // `ED 2` takes them too.
    with(|t| {
        parent(t);
        feed(t, &apc("a=p,i=2,p=1,P=1,Q=1,V=1,q=2"));
        feed(t, b"\x1b[2J");
        assert!(at(t, 2).is_empty());
        assert!(t.images().kitty_image(KittyId::of(2)).is_none());
    });
}

/// **A parent that moves takes its children**: "When the parent placement
/// moves, the relative placement moves along with it" -- a re-put of the
/// parent's pair elsewhere, a margin scroll that moves the parent and not
/// the rows its child is drawn over, and a reflow.
#[test]
fn a_parent_that_moves_takes_its_children() {
    with(|t| {
        parent(t);
        feed(t, &apc("a=p,i=2,p=1,P=1,Q=1,H=2,V=1,q=2"));
        feed(t, b"\x1b[11;31H");
        feed(t, &apc("a=p,i=1,p=1,C=1,q=2"));
        assert_eq!(at(t, 1), vec![(10, 30, 1)]);
        assert_eq!(at(t, 2), vec![(11, 32, 1)], "re-put: followed");
        // A band of rows 9 to 11 (1-based) scrolled up one: the parent, in
        // it, moves to row 9; the child, below it at row 11, moves too.
        feed(t, b"\x1b[9;11r\x1b[11;1H\n\x1b[r");
        assert_eq!(at(t, 1), vec![(9, 30, 1)], "the premise");
        assert_eq!(at(t, 2), vec![(10, 32, 1)], "scrolled: followed");
    });
    // A reflow: three lines of 70 columns above the parent wrap to two
    // rows each at 40 columns, moving the parent down three rows; the
    // child, on a line of its own below, moves with it.
    with(|t| {
        for id in 1..=2 {
            feed(t, &store(id));
        }
        for _ in 0..3 {
            feed(t, &[b'x'; 70]);
            feed(t, b"\r\n");
        }
        feed(t, b"\x1b[5;11H");
        feed(t, &apc("a=p,i=1,p=1,C=1,q=2"));
        feed(t, &apc("a=p,i=2,p=1,P=1,Q=1,H=2,V=1,q=2"));
        assert_eq!((at(t, 1), at(t, 2)), (vec![(4, 10, 1)], vec![(5, 12, 1)]));
        let _ = t.resize(&WinSize {
            ws_row: 24,
            ws_col: 40,
        });
        assert_eq!(at(t, 1), vec![(7, 10, 1)], "the premise: the parent moved");
        assert_eq!(at(t, 2), vec![(8, 12, 1)], "reflowed: followed");
    });
}

/// **A delete naming a cell finds a relative placement where it is
/// drawn**, the text's "Delete all placements that intersect a specific
/// cell". kitty tests the cursor cell it was put at, which need not be
/// where it is drawn: a departure from "intersect", so the text's reading
/// is built.
#[test]
fn a_cell_delete_finds_a_relative_placement_where_it_is_drawn() {
    with(|t| {
        parent(t);
        feed(t, b"\x1b[20;3H");
        feed(t, &apc("a=p,i=2,p=1,P=1,Q=1,H=5,q=2"));
        feed(t, &apc("a=d,d=p,x=3,y=20"));
        assert_eq!(at(t, 2).len(), 1, "not at the cell it was put at");
        feed(t, &apc("a=d,d=p,x=16,y=6"));
        assert!(at(t, 2).is_empty(), "at the cell it is drawn at");
        assert_eq!(at(t, 1).len(), 1, "the parent is at column 11");
    });
}

/// **Under a virtual parent**: made and answered, moving no cursor, and
/// gone with the virtual placement.
#[test]
fn a_virtual_parent_holds_its_children_until_it_goes() {
    with(|t| {
        parent(t);
        feed(t, &apc("a=p,U=1,i=5,p=3,c=2,r=2,q=2"));
        feed(t, b"\x1b[20;3H");
        let before = t.cursor();
        let r = feed(t, &apc("a=p,i=2,p=1,P=5,Q=3,H=1"));
        assert_eq!(r, b"\x1b_Gi=2,p=1;OK\x1b\\");
        assert_eq!(at(t, 2).len(), 1);
        assert_eq!(t.cursor(), before);
        feed(t, &apc("a=d,d=i,i=5"));
        assert!(at(t, 2).is_empty(), "went with its virtual parent");
        assert!(
            t.kitty_virtual(KittyId::of(5), KittyPlacement::of(3))
                .is_none(),
            "the premise"
        );
    });
}

/// The pinned list's diacritic for `n` (`[[source:
/// kitty-rowcolumn-diacritics]]`), read from the list.
fn d(n: usize) -> char {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/data/placeholder-diacritics.txt"
    );
    let src = std::fs::read_to_string(path).unwrap();
    let line = src
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .nth(n)
        .unwrap();
    char::from_u32(u32::from_str_radix(line.split(';').next().unwrap(), 16).unwrap()).unwrap()
}

/// Image 5's virtual placement 3, two by two, shown by placeholders from
/// 1-based row `y` and column `x`, a line of `size` (`ESC # 6` for double
/// width, else empty) each; then image 2 put under it with `H=1,V=1`
/// from the cursor at 20;3, which is never where it is drawn.
fn under_virtual(t: &mut Term<'_>, y: usize, x: usize, size: &str) {
    parent(t);
    feed(t, &apc("a=p,U=1,i=5,p=3,c=2,r=2,q=2"));
    let mut s = String::new();
    for r in 0..2 {
        s += &format!("\x1b[{};{x}H{size}\x1b[38;5;5;58;5;3m", y + r);
        for c in 0..2 {
            s.push('\u{10EEEE}');
            s.push(d(r));
            s.push(d(c));
        }
        s += "\x1b[0m";
    }
    feed(t, s.as_bytes());
    feed(t, b"\x1b[20;3H");
    feed(t, &apc("a=p,i=2,p=1,P=5,Q=3,H=1,V=1,q=2"));
    assert_eq!(at(t, 2), vec![(19, 2, 1)], "the premise: put at the cursor");
}

/// **Under a virtual parent, a delete by a place finds a relative
/// placement where it is drawn**: at the least row and column of the
/// placeholders showing its parent plus `H` and `V` ("The x position is
/// the minimum of all the placeholder x positions and the y position is
/// the minimum of all the placeholder y positions"), so a cell, a row or a
/// column delete there takes it -- "Delete all placements that intersect
/// a specific cell" -- as a delete at the cursor's cell does with the
/// cursor there, and one at the cell it was put at, or the cursor's cell
/// there, does not; at every read size. A double-width line's
/// placeholders begin at twice their column, where they are drawn.
#[test]
fn under_a_virtual_parent_a_delete_finds_it_where_it_is_drawn() {
    // Placeholders from 11;21, so drawn at 12;22.
    for (cmd, goes) in [
        ("a=d,d=p,x=3,y=20", false),
        ("a=d,d=c", false),
        ("a=d,d=x,x=3", false),
        ("a=d,d=y,y=20", false),
        ("a=d,d=p,x=22,y=12", true),
        ("a=d,d=q,x=22,y=12,z=0", true),
        ("a=d,d=x,x=22", true),
        ("a=d,d=y,y=12", true),
    ] {
        let bytes = apc(cmd);
        for n in 1..=bytes.len() {
            with(|t| {
                under_virtual(t, 11, 21, "");
                feed_by(t, &bytes, n);
                let want = if goes { 0 } else { 1 };
                assert_eq!(at(t, 2).len(), want, "{cmd} by {n}");
            });
        }
    }
    // The cursor on the cell it is drawn at: a cursor delete takes it.
    let bytes = apc("a=d,d=c");
    for n in 1..=bytes.len() {
        with(|t| {
            under_virtual(t, 11, 21, "");
            feed(t, b"\x1b[12;22H");
            feed_by(t, &bytes, n);
            assert!(at(t, 2).is_empty(), "the cursor at 12;22, by {n}");
        });
    }
    // On a double-width line, placeholders at column 11 are drawn from
    // display column 21, the child at 22.
    for (x, goes) in [(12, false), (22, true)] {
        with(|t| {
            under_virtual(t, 11, 11, "\x1b#6");
            feed(t, &apc(&format!("a=d,d=p,x={x},y=12")));
            assert_eq!(at(t, 2).is_empty(), goes, "double width, x={x}");
        });
    }
}

/// **Under a virtual parent none of whose placeholders is on screen, a
/// relative placement is drawn nowhere**, so it is not on the visible
/// screen and a delete of what is visible keeps it, as a cell delete at
/// the cell it was put at does; its parent's going still takes it.
#[test]
fn under_a_virtual_parent_shown_nowhere_it_is_not_visible() {
    let cmd = apc("a=d,d=a");
    for n in 1..=cmd.len() {
        with(|t| {
            under_virtual(t, 11, 21, "");
            // The placeholder cells erased, the text with them.
            feed(t, b"\x1b[11;21H\x1b[2X\x1b[12;21H\x1b[2X");
            feed_by(t, &cmd, n);
            assert_eq!(at(t, 2).len(), 1, "by {n}: drawn nowhere, not visible");
            assert!(at(t, 1).is_empty(), "by {n}: the parent image 1 is visible");
            feed(t, &apc("a=d,d=p,x=3,y=20"));
            assert_eq!(at(t, 2).len(), 1, "by {n}: nor at the cell it was put at");
            feed(t, &apc("a=d,d=i,i=5"));
            assert!(at(t, 2).is_empty(), "by {n}: went with its virtual parent");
        });
    }
    // With its placeholders on screen, it is visible.
    with(|t| {
        under_virtual(t, 11, 21, "");
        feed(t, &cmd);
        assert!(at(t, 2).is_empty(), "drawn, so visible");
    });
}
