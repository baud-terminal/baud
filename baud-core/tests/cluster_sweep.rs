//! The cluster store's mechanisms, one test each: the hash that spreads, the
//! eight probes, a lookup that reads past a slot a sweep freed, the count of
//! filled slots, a sweep only once every slot is filled and the bound on how
//! often it runs then, the epoch a renderer reads, and the base a cell with
//! continuations reads back from its slot.
//!
//! `cluster_bounds.rs` holds what a reader sees, and fails on the store this
//! replaced. These read the store itself, through the API it gained, so they
//! cannot compile against it; each is red under a perturbation of the
//! mechanism it names instead.

#[path = "support/mod.rs"]
mod support;

use baud_core::{
    Back, CacheGeometry, ClusterEpoch, ClusterId, ClusterLive, ClusterSlot, ClusterStore,
    Codepoint, Content, Depth, NCLUSTER, Owned, PackedCell, Refused, Row, Term, WinSize,
};

fn base(c: u32) -> Content {
    Content::codepoint(Codepoint::new(c).expect("a codepoint"))
}

/// The slot `cps` under `b` takes in an empty store of `NCLUSTER` slots:
/// the one its hash starts at.
fn landing(b: u32, cps: &[u32]) -> usize {
    let mut slots = vec![ClusterSlot::EMPTY; NCLUSTER];
    let mut store = ClusterStore::new(&mut slots);
    let id = store.intern(base(b), cps);
    id.index()
        .checked_sub(1)
        .expect("an empty store takes any cluster")
}

/// The 5,824 one-mark clusters of `cluster_bounds.rs`: an ASCII letter and
/// a combining diacritical mark.
fn one_mark() -> Vec<(u32, u32)> {
    let letters = (u32::from(b'a')..=u32::from(b'z')).chain(u32::from(b'A')..=u32::from(b'Z'));
    let letters: Vec<u32> = letters.collect();
    (0x0300..=0x036F)
        .flat_map(|m| letters.iter().map(move |&b| (b, m)))
        .collect()
}

/// One-mark clusters, each a base and a mark.
type Clusters = Vec<(u32, u32)>;

/// One one-mark cluster starting at each slot, in slot order, so the
/// clusters fill every slot of an empty store, each its own start: a base
/// from the 52 ASCII letters, the 25 Greek small letters U+03B1-03C9 and the
/// 32 Cyrillic small letters U+0430-044F, and one of the 112 combining
/// diacritical marks, 12,208 clusters to cover 1024 slots. The rest, unused,
/// are returned after them.
fn one_per_slot() -> (Clusters, Clusters) {
    let bases = (u32::from(b'a')..=u32::from(b'z'))
        .chain(u32::from(b'A')..=u32::from(b'Z'))
        .chain(0x03B1..=0x03C9)
        .chain(0x0430..=0x044F);
    let bases: Vec<u32> = bases.collect();
    assert_eq!(bases.len(), 109);
    let mut by: Vec<Option<(u32, u32)>> = vec![None; NCLUSTER];
    let mut rest = Vec::new();
    for m in 0x0300..=0x036F {
        for &b in &bases {
            let s = landing(b, &[m]);
            match by[s] {
                None => by[s] = Some((b, m)),
                Some(_) => rest.push((b, m)),
            }
        }
    }
    let fill: Vec<(u32, u32)> = by
        .into_iter()
        .map(|c| c.expect("a cluster starting at every slot"))
        .collect();
    (fill, rest)
}

/// `n` distinct one-mark clusters that all start at one slot, and the slot.
fn same_start(n: usize) -> (usize, Vec<(u32, u32)>) {
    let mut by: Vec<Vec<(u32, u32)>> = vec![Vec::new(); NCLUSTER];
    for (b, m) in one_mark() {
        by[landing(b, &[m])].push((b, m));
    }
    let s = (0..NCLUSTER - 8)
        .find(|&s| by[s].len() >= n)
        .expect("a slot that many clusters start at");
    (s, by[s][..n].to_vec())
}

/// **The hash spreads at 1024 slots, by as much as the decided construction
/// does.** Two families of real clusters -- bet and dagesh with each of the
/// 51 Hebrew points, and each of the 27 Hebrew letters with qamats -- start
/// in 31 and 27 distinct slots, where a uniform hash averages 49.8 and 26.7.
/// The hash this replaced, a mistyped FNV prime with no finalizer, started
/// each family in one slot; the right prime with no finalizer, in 4 and 2.
///
/// Pinned exactly, because the first family is short of uniform and that is
/// a finding rather than a tolerance: the slot is bits 32-41 of the
/// finalized product (`>> 32`, then `% 1024`), where the top ten bits start
/// it in 51 (an open question).
#[test]
fn two_families_of_clusters_spread_across_the_slots() {
    let points: Vec<u32> = (0x0591..=0x05C7)
        .filter(|c| !matches!(c, 0x05BE | 0x05C0 | 0x05C3 | 0x05C6))
        .collect();
    assert_eq!(points.len(), 51);
    let letters: Vec<u32> = (0x05D0..=0x05EA).collect();
    assert_eq!(letters.len(), 27);
    let families: [Vec<(u32, Vec<u32>)>; 2] = [
        points.iter().map(|&p| (0x05D1, vec![0x05BC, p])).collect(),
        letters.iter().map(|&l| (l, vec![0x05B8])).collect(),
    ];
    let spread: Vec<usize> = families
        .iter()
        .map(|keys| {
            let mut seen = vec![false; NCLUSTER];
            for (b, cps) in keys {
                seen[landing(*b, cps)] = true;
            }
            seen.iter().filter(|s| **s).count()
        })
        .collect();
    assert_eq!(spread, [31, 27], "distinct starting slots, by family");
}

/// **Eight probes**: nine clusters that start at one slot land in the eight
/// slots from it, and the ninth is refused for want of one -- the refusal a
/// sweep can answer. Four probes refused the fifth.
#[test]
fn eight_clusters_that_start_at_one_slot_land_and_a_ninth_is_refused() {
    // Eight, the chosen count, spelled out: a test that read the store's
    // own constant would follow it anywhere.
    let (s, keys) = same_start(9);
    let mut slots = vec![ClusterSlot::EMPTY; NCLUSTER];
    let mut store = ClusterStore::new(&mut slots);
    for (k, (b, m)) in keys[..8].iter().enumerate() {
        let id = store.try_intern(base(*b), &[*m]).expect("a probe free");
        assert_eq!(id.index(), s + k + 1, "probe {k} from slot {s}");
    }
    let (b, m) = keys[8];
    assert_eq!(store.try_intern(base(b), &[m]), Err(Refused::Full));
    // The same cluster again is the slot it has, not a refusal.
    let (b0, m0) = keys[0];
    assert_eq!(
        store.try_intern(base(b0), &[m0]).map(ClusterId::index),
        Ok(s + 1)
    );
}

/// The id a cell naming `id` holds, as its content.
fn named(id: ClusterId) -> PackedCell {
    PackedCell {
        content: Content::cluster(id),
        ..PackedCell::EMPTY
    }
}

/// **A cluster past a slot a sweep freed is found, not interned again.**
/// Two clusters that start at one slot take it and the next; a sweep that
/// sees only the second frees the first. Asked for again, the second is the
/// slot it holds -- a lookup that stopped at the free slot would put it there
/// a second time -- and the first takes the freed slot back.
#[test]
fn a_cluster_past_a_freed_slot_is_found_and_not_copied() {
    let (s, keys) = same_start(2);
    let [(ab, am), (bb, bm)] = [keys[0], keys[1]];
    let mut slots = vec![ClusterSlot::EMPTY; NCLUSTER];
    let mut store = ClusterStore::new(&mut slots);
    let a = store.intern(base(ab), &[am]);
    let b = store.intern(base(bb), &[bm]);
    assert_eq!((a.index(), b.index()), (s + 1, s + 2));
    let mut live = ClusterLive::new();
    live.mark(&[named(b)]);
    assert_eq!(
        store.sweep(&live),
        1,
        "the sweep frees the slot no cell names"
    );
    assert!(
        store.get(Content::cluster(a)).is_none(),
        "and only that one"
    );
    assert_eq!(store.base_of(Content::cluster(b)), base(bb));
    assert_eq!(
        store.intern(base(bb), &[bm]),
        b,
        "found past the freed slot"
    );
    assert_eq!(store.intern(base(ab), &[am]), a, "and the freed slot taken");
}

/// **The count of filled slots follows every fill, sweep and reset**, and
/// a store made over slots that hold clusters counts them: what tells a
/// refusal that every slot is used without walking the slots. A cluster the
/// store holds already, and a shape no slot takes, fill nothing.
#[test]
fn the_fill_count_follows_fills_sweeps_and_resets() {
    let (fill, _) = one_per_slot();
    let mut slots = vec![ClusterSlot::EMPTY; NCLUSTER];
    let mut store = ClusterStore::new(&mut slots);
    assert_eq!(store.filled(), 0);
    let ids: Vec<ClusterId> = fill[..10]
        .iter()
        .map(|&(b, m)| store.intern(base(b), &[m]))
        .collect();
    assert_eq!(store.filled(), 10, "ten fills");
    store.intern(base(fill[0].0), &[fill[0].1]);
    assert_eq!(store.filled(), 10, "a cluster the store holds");
    store.intern(base(0x65), &[]);
    assert_eq!(store.filled(), 10, "a shape no slot takes");
    let mut live = ClusterLive::new();
    live.mark(&ids[..4].iter().map(|&id| named(id)).collect::<Vec<_>>());
    assert_eq!(store.sweep(&live), 6);
    assert_eq!(store.filled(), 4, "a sweep empties what it frees");
    let mut store = ClusterStore::new(&mut slots);
    assert_eq!(store.filled(), 4, "a store over held slots counts them");
    store.reset();
    assert_eq!(store.filled(), 0, "a reset empties every slot");
}

/// **No sweep is due while any slot is empty**, however many interns have
/// wanted a slot: a sweep waits until all slots are used. Once the last
/// slot fills, the first refusal sweeps; then a store whose every slot a
/// live cell names, which a sweep leaves full, sweeps at most once every 64
/// interns that wanted a slot (`SLOTS_MAX / 16`), not on every mark.
#[test]
fn a_sweep_is_due_only_when_every_slot_is_filled_and_then_once_every_sixty_four_interns() {
    let (fill, rest) = one_per_slot();
    let mut slots = vec![ClusterSlot::EMPTY; NCLUSTER];
    let mut store = ClusterStore::new(&mut slots);
    assert!(!store.sweep_due(), "an empty store");
    for (k, &(b, m)) in fill[..NCLUSTER - 1].iter().enumerate() {
        let id = store.intern(base(b), &[m]);
        assert_eq!(id.index(), k + 1, "each cluster at its own start");
    }
    assert!(!store.sweep_due(), "1023 slots filled, 1023 interns");
    let (b, m) = fill[NCLUSTER - 1];
    store.intern(base(b), &[m]);
    assert_eq!(store.filled(), NCLUSTER);
    assert!(store.sweep_due(), "every slot filled");
    let mut live = ClusterLive::new();
    let all: Vec<PackedCell> = (1..=NCLUSTER)
        .map(|i| named(ClusterId::from_index(i)))
        .collect();
    live.mark(&all);
    assert_eq!(store.sweep(&live), 0, "every slot named");
    let due = ClusterStore::SLOTS_MAX / 16;
    assert_eq!(due, 64);
    for (k, &(b, m)) in rest[..due].iter().enumerate() {
        assert!(!store.sweep_due(), "due after {k} refusals");
        assert_eq!(store.try_intern(base(b), &[m]), Err(Refused::Full));
    }
    assert!(store.sweep_due(), "due after {due}");
    // A cluster the store holds wants no slot, and counts nothing.
    store.sweep(&live);
    for _ in 0..due {
        store.intern(base(fill[0].0), &[fill[0].1]);
    }
    assert!(!store.sweep_due(), "hits only");
}

/// **A store whose every slot is filled, half by clusters on cells and half
/// by clusters no cell names, sweeps once and frees exactly the half**, and
/// every cell on the screen reads its base and its mark back. Before the
/// last slot filled, a cluster whose eight slots were taken was refused
/// and nothing swept.
///
/// One cluster starts at each of the 1024 slots. Those at odd slots are
/// written and the screen erased, so no cell names them; those at even
/// slots stay on the screen, all but the last. A cluster whose eight slots
/// hold others is then refused with one slot of the store still empty, the
/// last even cluster takes that slot, and one more cluster, its slots all
/// taken, sweeps. The 512 freed are the odd slots, which the screen's
/// clusters cannot name.
#[test]
fn a_full_store_of_live_and_dead_clusters_frees_only_the_dead() {
    let ws = WinSize {
        ws_row: 20,
        ws_col: 200,
    };
    let per = 100;
    let (fill, rest) = one_per_slot();
    let spell = |cs: &[(u32, u32)]| -> Vec<u8> {
        let mut out = Vec::new();
        for (k, &(b, m)) in cs.iter().enumerate() {
            let s: String = [b, m]
                .iter()
                .map(|&c| char::from_u32(c).expect("a char"))
                .collect();
            out.extend(s.bytes());
            match (k + 1) % per == 0 {
                true => out.extend(b"\r\n"),
                false => out.push(b' '),
            }
        }
        out
    };
    let dead: Vec<(u32, u32)> = fill.iter().copied().skip(1).step_by(2).collect();
    let live: Vec<(u32, u32)> = fill.iter().copied().step_by(2).collect();
    assert_eq!((dead.len(), live.len()), (512, 512));
    let mut st = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::NONE);
    let mut t = Term::new(st.storage(), &ws);
    let mut b = spell(&dead);
    b.extend(b"\x1b[H\x1b[2J");
    b.extend(spell(&live[..511]));
    support::feed(&mut t, &b);
    // A cluster whose eight slots are all filled: the empty slot is the
    // last even one, 1022, and this one's window lies below it.
    let last = NCLUSTER - 2;
    let &(wb, wm) = rest
        .iter()
        .find(|&&(b, m)| {
            let s = landing(b, &[m]);
            s + 8 <= last
        })
        .expect("a cluster starting below the empty slot");
    support::feed(&mut t, &spell(&[(wb, wm)]));
    assert_eq!(t.marks_dropped, 1, "refused for want of a slot");
    assert_eq!(t.cluster_sweeps().sweeps, 0, "and no sweep, 1023 filled");
    support::feed(&mut t, b"\r\n");
    support::feed(&mut t, &spell(&live[511..]));
    assert_eq!(t.cluster_sweeps().sweeps, 0, "the last slot filled");
    let x = rest
        .iter()
        .copied()
        .find(|&c| c != (wb, wm))
        .expect("another cluster");
    support::feed(&mut t, b"\r\n");
    support::feed(&mut t, &spell(&[x]));
    let swept = t.cluster_sweeps();
    assert_eq!(swept.sweeps, 1, "one sweep, every slot filled");
    assert_eq!(swept.freed, 512, "the odd slots, and only those");
    assert_eq!(t.marks_dropped, 1, "the cluster after the sweep is kept");
    // Rows 0-4 hold 500 live clusters, row 5 eleven more and the refused
    // one, row 6 the last live one and row 7 the one that swept.
    let mut got = Vec::new();
    for (r, n) in [(0, per), (1, per), (2, per), (3, per), (4, per), (5, 12)] {
        let row = t.row(Row::from_index(r)).expect("a row");
        got.extend(read(&t, row, n));
    }
    let mut want: Vec<(Content, Vec<u32>)> = live[..511]
        .iter()
        .map(|&(b, m)| (base(b), vec![m]))
        .collect();
    want.push((base(wb), Vec::new()));
    assert_eq!(got, want, "every live cell, and the refused one bare");
    let row = t.row(Row::from_index(6)).expect("a row");
    assert_eq!(read(&t, row, 1), [(base(live[511].0), vec![live[511].1])]);
    let row = t.row(Row::from_index(7)).expect("a row");
    assert_eq!(
        read(&t, row, 1),
        [(base(x.0), vec![x.1])],
        "the cluster that swept"
    );
}

/// **The epoch moves when a slot may have changed hands**: a sweep that
/// freed one and a reset. A sweep that frees nothing leaves it, so a
/// renderer does not repaint a screen whose every slot is still its own.
#[test]
fn the_epoch_moves_on_a_sweep_that_frees_and_on_a_reset() {
    let mut slots = vec![ClusterSlot::EMPTY; NCLUSTER];
    let mut store = ClusterStore::new(&mut slots);
    let e0 = store.epoch();
    assert_eq!(e0, ClusterEpoch::default());
    let id = store.intern(base(0x65), &[0x0301]);
    let mut live = ClusterLive::new();
    live.mark(&[named(id)]);
    assert_eq!(store.sweep(&live), 0);
    assert_eq!(store.epoch(), e0, "a sweep that freed nothing");
    assert_eq!(store.sweep(&ClusterLive::new()), 1);
    let e1 = store.epoch();
    assert_ne!(e1, e0, "a sweep that freed a slot");
    store.reset();
    assert_ne!(store.epoch(), e1, "a reset");
}

/// The 52 ASCII letters, a cluster's base, in `cluster_bounds.rs`'s order.
const LETTERS: &[u8; 52] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

fn cluster(i: usize) -> (u8, u32) {
    (
        LETTERS[i % LETTERS.len()],
        0x0300 + u32::try_from(i / LETTERS.len()).expect("one of 112 marks"),
    )
}

fn lines(from: usize, to: usize, per: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, i) in (from..to).enumerate() {
        let (b, m) = cluster(i);
        out.push(b);
        out.extend(char::from_u32(m).expect("a mark").to_string().bytes());
        match (k + 1) % per == 0 {
            true => out.extend(b"\r\n"),
            false => out.push(b' '),
        }
    }
    out
}

/// Each of `per` clusters on `cells` as base and marks, read back through
/// the store.
fn read(t: &Term<'_>, cells: &[PackedCell], per: usize) -> Vec<(Content, Vec<u32>)> {
    (0..per)
        .map(|k| {
            let c = cells[2 * k];
            (t.base_of(c), t.cluster_marks(c).collect())
        })
        .collect()
}

fn want(from: usize, per: usize) -> Vec<(Content, Vec<u32>)> {
    (from..from + per)
        .map(|i| {
            let (b, m) = cluster(i);
            (base(u32::from(b)), vec![m])
        })
        .collect()
}

/// **Every cell reads its base and its marks back after the sweep**, on
/// the history, on the screen that was not up and above a churning region,
/// and the sweep ran, once all 1024 slots were used, and freed all but the
/// 177 slots cells name: the positive control the base and marks need,
/// since a store that never swept would pass them on a session that fits
/// it. A cell with continuations holds its slot, not its base, so a
/// sweep that freed a slot a cell names would lose the cell's letter with
/// its mark.
#[test]
fn every_cell_reads_its_base_and_marks_back_after_the_sweeps() {
    let ws = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let per = 4;
    let mut st = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::DEFAULT);
    let mut t = Term::new(st.storage(), &ws);
    support::feed(&mut t, &lines(0, 160, per));
    let mut b = b"\x1b[?47h\x1b[H".to_vec();
    b.extend(lines(160, 192, per));
    b.extend(b"\x1b[?47l\x1b[6;10r\x1b[10;1H");
    b.extend(lines(192, 2192, per));
    b.extend(b"\x1b[r\x1b[?47h\x1b[9;10r\x1b[10;1H");
    b.extend(lines(2192, 4192, per));
    b.extend(b"\x1b[r");
    support::feed(&mut t, &b);
    assert_eq!(t.marks_dropped, 2895, "the churn's, while slots were taken");
    let swept = t.cluster_sweeps();
    assert_eq!(
        (swept.sweeps, swept.freed),
        (1, NCLUSTER - 177),
        "the sweep"
    );
    for r in 0..8 {
        let row = t.row(Row::from_index(r)).expect("a row");
        assert_eq!(read(&t, row, per), want(160 + per * r, per), "alt row {r}");
    }
    support::feed(&mut t, b"\x1b[?47l");
    let hist = t.scrollback();
    assert_eq!(hist.len(), 31);
    for k in 0..hist.len() {
        let (cells, _) = hist.line(Back::of(k)).expect("a line");
        assert_eq!(read(&t, cells, per), want(per * (30 - k), per), "line {k}");
    }
    for r in 0..5 {
        let row = t.row(Row::from_index(r)).expect("a row");
        assert_eq!(read(&t, row, per), want(per * (31 + r), per), "row {r}");
    }
}

/// **A cluster grown one mark at a time keeps its base**: each mark interns
/// the cluster so far under the base its slot holds, not under the slot's
/// word the cell holds once it has a first mark. Three marks on `e`, and
/// every letter of Genesis 1:1, read back as their letters, with their
/// points.
#[test]
fn a_cluster_grown_a_mark_at_a_time_keeps_its_base() {
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    let verse = "בְּרֵאשִׁית בָּרָא אֱלֹהִים אֵת הַשָּׁמַיִם וְאֵת הָאָרֶץ׃";
    support::feed(
        &mut t,
        format!("e\u{0301}\u{0302}\u{0303}\r\n{verse}").as_bytes(),
    );
    let row = t.row(Row::ZERO).expect("a row");
    assert_eq!(t.base_of(row[0]), base(0x65));
    assert_eq!(
        t.cluster_marks(row[0]).collect::<Vec<_>>(),
        [0x301, 0x302, 0x303]
    );
    let mut want: Vec<(Content, Vec<u32>)> = Vec::new();
    for c in verse.chars() {
        let cp = u32::from(c);
        match (0x0591..=0x05C7).contains(&cp) && !matches!(cp, 0x05BE | 0x05C0 | 0x05C3 | 0x05C6) {
            true => want.last_mut().expect("a letter").1.push(cp),
            false => want.push((base(cp), Vec::new())),
        }
    }
    let row = t.row(Row::from_index(1)).expect("a row");
    let got: Vec<(Content, Vec<u32>)> = row[..want.len()]
        .iter()
        .map(|c| (t.base_of(*c), t.cluster_marks(*c).collect()))
        .collect();
    assert_eq!(got, want);
}

/// **A session past the store sweeps once, when the store is full, and
/// frees every slot but the one the screen names**: `cluster_bounds.rs`'s
/// 5,824 distinct one-mark clusters on five rows with no history, where
/// it refuses 3,816 and keeps 2,008.
#[test]
fn a_session_past_the_store_sweeps_once_and_frees_all_but_what_the_screen_names() {
    let ws = WinSize {
        ws_row: 5,
        ws_col: 40,
    };
    let mut st = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::NONE);
    let mut t = Term::new(st.storage(), &ws);
    support::feed(&mut t, &lines(0, LETTERS.len() * 112, 4));
    assert_eq!(t.marks_dropped, 3816);
    let swept = t.cluster_sweeps();
    assert_eq!((swept.sweeps, swept.freed), (1, NCLUSTER - 1));
}

/// **`Term::cluster_epoch` moves on `RIS` and on a sweep that frees**, and
/// not on a frame's worth of ordinary text: what the renderer's damage
/// record reads to repaint a cell whose word a reused slot left unchanged.
#[test]
fn the_terminal_s_epoch_moves_on_a_reset_and_on_a_sweep_that_frees() {
    let ws = WinSize {
        ws_row: 5,
        ws_col: 40,
    };
    let mut st = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::NONE);
    let mut t = Term::new(st.storage(), &ws);
    let e0 = t.cluster_epoch();
    support::feed(&mut t, "e\u{0301} plain text\r\n".as_bytes());
    assert_eq!(t.cluster_epoch(), e0, "a cluster interned");
    support::feed(&mut t, b"\x1bc");
    let e1 = t.cluster_epoch();
    assert_ne!(e1, e0, "RIS");
    // Every slot filled, by clusters the five rows scroll away, and one
    // more, whose slots are all taken: the sweep.
    let (fill, rest) = one_per_slot();
    let spell = |cs: &[(u32, u32)]| -> String {
        cs.iter()
            .flat_map(|&(c, m)| [c, m, 0x20])
            .map(|c| char::from_u32(c).expect("a char"))
            .collect()
    };
    support::feed(&mut t, spell(&fill).as_bytes());
    assert_eq!(t.cluster_epoch(), e1, "no sweep while the store filled");
    support::feed(&mut t, spell(&rest[..1]).as_bytes());
    assert!(t.cluster_sweeps().freed > 0);
    assert_ne!(t.cluster_epoch(), e1, "a sweep that freed");
}

/// **An intern that fills a slot moves the count a renderer reads**, and one
/// that finds its cluster already held does not; the epoch moves for
/// neither.
#[test]
fn an_intern_moves_cluster_writes_and_a_hit_does_not() {
    support::with_corpus(|t| {
        let (w0, e0) = (t.cluster_writes(), t.cluster_epoch());
        support::feed(t, "\u{444}\u{335} ".as_bytes());
        let w1 = t.cluster_writes();
        assert_ne!(w1, w0, "a new cluster is written into a slot");
        support::feed(t, "\u{444}\u{335} ".as_bytes());
        assert_eq!(
            t.cluster_writes(),
            w1,
            "the same cluster again writes nothing"
        );
        assert_eq!(
            t.cluster_epoch(),
            e0,
            "no slot was given to another cluster"
        );
    });
}
