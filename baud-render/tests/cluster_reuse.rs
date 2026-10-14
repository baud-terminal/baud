//! A cluster slot freed and taken by another cluster is repainted.
//!
//! The CPU and instance paths repaint a row only when its cell words
//! changed, against the copy of the last frame they keep. A cell names its
//! cluster by the slot's index, so a cell whose slot was freed -- by a sweep
//! of the dead clusters, or by `RIS` -- and given to another cluster keeps
//! its word while its glyphs change, and a frame that compared words drew
//! the old cluster. The store's epoch (`Term::cluster_epoch`) moves when a
//! slot may have changed hands, and a moved epoch repaints every row, as an
//! evicted rendition does (docs/record.md, "The cluster store holds 1024
//! clusters and sweeps the dead ones").
//!
//! Each test builds the collision on purpose: the slot a cluster lands in is
//! read from an empty `ClusterStore` of the terminal's size, which is the
//! store the terminal interns into.

mod support;

use baud_core::{BytePos, Col, NCLUSTER, Owned, ReadLen, Row, Term};
use baud_domain::{ClusterSlot, ClusterStore, Codepoint, Content};
use baud_font::Face;
use baud_render::Damage;
use support::*;

fn feed(t: &mut Term<'_>, b: &[u8]) {
    t.write_at(BytePos::ZERO, b);
    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
}

fn base(c: char) -> Content {
    Content::codepoint(Codepoint::new(u32::from(c)).expect("a codepoint"))
}

/// The slot index the last of `clusters` lands in, interned in order into
/// an empty store of the terminal's size, as a cell's growing cluster is.
fn lands(clusters: &[(char, Vec<u32>)]) -> Option<usize> {
    let mut slots = vec![ClusterSlot::EMPTY; NCLUSTER];
    let mut store = ClusterStore::new(&mut slots);
    let mut last = None;
    for (b, cps) in clusters {
        last = store.intern(base(*b), cps).index().checked_sub(1);
    }
    last
}

/// A letter and its marks, as bytes.
fn spell(b: char, cps: &[u32]) -> String {
    core::iter::once(b)
        .chain(cps.iter().map(|&m| char::from_u32(m).expect("a mark")))
        .collect()
}

/// The same screen painted incrementally after `before` and then `after`,
/// and whole by a fresh renderer: whether the two agree, and the cell word
/// at the home position after each.
fn frames(before: &[u8], after: &[u8]) -> (bool, [baud_core::PackedCell; 2], Vec<u32>) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("the face")];
    let mut inc = renderer(&faces);
    let mut a = Canvas::for_renderer(&inc);
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    feed(&mut t, before);
    paint(&mut inc, &mut t, &mut a, Damage::Incremental);
    let was = t.row(Row::ZERO).expect("row 0")[0];
    feed(&mut t, after);
    paint(&mut inc, &mut t, &mut a, Damage::Incremental);
    let now = t.row(Row::ZERO).expect("row 0")[0];
    let marks = t.marks(Row::ZERO, Col::ZERO).collect();
    let mut full = renderer(&faces);
    let mut b = Canvas::for_renderer(&full);
    paint(&mut full, &mut t, &mut b, Damage::Full);
    (a.pixels == b.pixels, [was, now], marks)
}

/// **After `RIS`, a cluster that takes the slot `e` + U+0301 held is
/// repainted.** `e` with two of the combining diacritical marks, searched
/// for one whose growing cluster ends in that slot: after the reset the
/// store is empty again, so the first cluster written takes the slot its
/// hash starts at. The cell's word is the same before and after -- the
/// collision this needs, checked -- and its marks are the new cluster's.
/// The store this replaced reused a slot the same way, under the same base
/// (`e` + U+0342 on its hash), and the frame kept the acute.
#[test]
fn a_slot_a_reset_freed_and_another_cluster_took_is_repainted() {
    let first = ('e', vec![0x0301]);
    let k = lands(core::slice::from_ref(&first)).expect("a slot");
    let marks = 0x0300..=0x036F;
    let ones = marks.clone().map(|m| vec![(first.0, vec![m])]);
    let twos = marks.clone().flat_map(|m1| {
        marks
            .clone()
            .map(move |m2| vec![('e', vec![m1]), ('e', vec![m1, m2])])
    });
    let second = ones
        .chain(twos)
        .filter(|c| c.last().is_some_and(|l| *l != first))
        .find(|c| lands(c) == Some(k))
        .expect("a cluster that ends in the slot e + U+0301 took");
    let (b, cps) = second.last().cloned().expect("a cluster");
    let after = format!("\x1bc{}", spell(b, &cps));
    let (same, [was, now], got) = frames(spell(first.0, &first.1).as_bytes(), after.as_bytes());
    assert_eq!(got, cps, "the cell holds the second cluster");
    assert_eq!(was, now, "the collision: one word for both clusters");
    assert!(same, "the frame after the reset is a full repaint's");
}

/// **A slot a sweep freed, taken by the next cluster at the cell that held
/// it, is repainted.** One cluster starting at each of the store's slots
/// fills every one of them: a thousand and twenty-three are written below
/// the home row and erased, and A, at the slot B also starts at, is written
/// at the home position. Then B is written over A: every slot holds a
/// cluster, so the store sweeps, finds no cell naming any, and B takes the
/// slot its hash starts at, A's. The home cell's word is A's word again --
/// checked -- and the cursor ends where it did, after the home cell, so
/// nothing but the word could dirty the row; the frame must draw B. The
/// store this replaced refused B outright: 63 slots, and no sweep.
#[test]
fn a_slot_a_sweep_freed_and_another_cluster_took_is_repainted() {
    let bases = ('a'..='z')
        .chain('A'..='Z')
        .chain('\u{03B1}'..='\u{03C9}')
        .chain('\u{0430}'..='\u{044F}');
    let pool: Vec<(char, u32)> = bases
        .flat_map(|b| (0x0300..=0x036F).map(move |m| (b, m)))
        .collect();
    let mut by: Vec<Vec<(char, u32)>> = vec![Vec::new(); NCLUSTER];
    for &(b, m) in &pool {
        if let Some(s) = lands(&[(b, vec![m])]) {
            by[s].push((b, m));
        }
    }
    let k = (0..NCLUSTER)
        .find(|&k| by[k].len() >= 2)
        .expect("a slot two clusters start at");
    let (a, b) = (by[k][0], by[k][1]);
    // A slot no cluster starts at would stay empty, the store would not
    // fill, and B would be refused: the test says so at B.
    let fillers: Vec<String> = (0..NCLUSTER)
        .filter(|&j| j != k && !by[j].is_empty())
        .map(|j| {
            let (fb, fm) = by[j][0];
            spell(fb, &[fm])
        })
        .collect();
    let before = format!(
        "\x1b[2;1H{}\x1b[2J\x1b[H{}",
        fillers.join(" "),
        spell(a.0, &[a.1])
    );
    let after = format!("\x1b[H{}", spell(b.0, &[b.1]));
    let (same, [was, now], got) = frames(before.as_bytes(), after.as_bytes());
    assert_eq!(got, vec![b.1], "B is stored, in the slot the sweep freed");
    assert_eq!(was, now, "the collision: A's word is B's");
    assert!(same, "the frame after the sweep is a full repaint's");
}
