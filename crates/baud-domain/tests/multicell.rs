//! The multicell words and store: a head names a slot holding a block's
//! text and size, a tail says where its head is, a refused block keeps a
//! head of its own, and a full store sweeps at most once every
//! [`MulticellStore::DUE`] inserts.

use baud_domain::{
    ClusterStore, Content, MulticellId, MulticellLive, MulticellRefused, MulticellSlot,
    MulticellStore, PackedCell, TailAt, TextSize,
};

fn size(meta: &str) -> TextSize {
    TextSize::parse(meta.as_bytes()).unwrap()
}

fn cps(s: &str) -> Vec<u32> {
    s.chars().map(u32::from).collect()
}

fn head(id: MulticellId) -> PackedCell {
    PackedCell {
        content: Content::multicell(id),
        ..PackedCell::EMPTY
    }
}

#[test]
fn a_tail_says_where_its_head_is() {
    for (x, y) in [(1, 0), (48, 0), (0, 1), (48, 6), (3, 2)] {
        let at = TailAt::new(x, y).unwrap();
        let c = Content::tail(at);
        assert_eq!(c.tail_at(), Some(at), "({x}, {y})");
        assert_eq!((at.x(), at.y()), (x, y));
        assert!(c.is_continuation());
        assert!(!c.is_empty() && !c.is_wide_tail());
        assert_eq!(c.multicell_id(), None);
        assert_eq!(c.cluster_id(), None);
        assert_eq!(c.placeholder_marks(), None);
        assert!(!c.is_placeholder_base(), "a tail takes no diacritic");
    }
    // The head itself, and past the largest block: seven by seven cells.
    assert_eq!(TailAt::new(0, 0), None);
    assert_eq!(TailAt::new(49, 0), None);
    assert_eq!(TailAt::new(0, 7), None);
    assert_eq!(TailAt::X_MAX, 48);
    assert_eq!(TailAt::Y_MAX, 6);
}

#[test]
fn a_wide_tail_is_a_continuation_and_nothing_else_is() {
    assert!(Content::WIDE_TAIL.is_continuation());
    assert!(!Content::EMPTY.is_continuation());
    assert!(!Content::DEFECTIVE.is_continuation());
    let a = Content::codepoint(baud_domain::Codepoint::new(0x61).unwrap());
    assert!(!a.is_continuation());
    assert!(!Content::multicell(MulticellId::REFUSED).is_continuation());
}

#[test]
fn a_head_is_neither_a_tail_a_cluster_nor_a_placeholder() {
    let mut slots = vec![MulticellSlot::EMPTY; 4];
    let mut store = MulticellStore::new(&mut slots);
    let id = store.insert(size("w=2"), &cps("ab"));
    for id in [MulticellId::REFUSED, id] {
        let c = Content::multicell(id);
        assert_eq!(c.multicell_id(), Some(id));
        assert_eq!(c.tail_at(), None);
        assert_eq!(c.cluster_id(), None);
        assert_eq!(c.placeholder_marks(), None);
        assert!(!c.is_placeholder_base(), "a head takes no diacritic");
        assert!(!c.is_empty());
        assert_eq!(c.as_codepoint(), None);
    }
    // A head's word is no cluster's slot: the cluster store reads it as
    // itself and its marks as none.
    let mut cl = vec![baud_domain::ClusterSlot::EMPTY; 4];
    let clusters = ClusterStore::new(&mut cl);
    let c = Content::multicell(id);
    assert_eq!(clusters.base_of(c), c);
    assert_eq!(clusters.marks_of(c).count(), 0);
}

#[test]
fn a_block_reads_back_its_text_and_size() {
    let mut slots = vec![MulticellSlot::EMPTY; 8];
    let mut store = MulticellStore::new(&mut slots);
    let id = store.insert(size("w=2"), &cps("ab"));
    assert!(!id.is_refused());
    let c = Content::multicell(id);
    let slot = store.get(c).unwrap();
    assert_eq!(slot.text(), &cps("ab")[..]);
    assert_eq!(slot.size(), size("w=2"));
    assert_eq!(slot.size().width.cells(), Some(2));
    assert_eq!(store.text_of(c), &cps("ab")[..]);
    assert_eq!(store.filled(), 1);
    // The same text at the same size is the same slot.
    assert_eq!(store.insert(size("w=2"), &cps("ab")), id);
    assert_eq!(store.filled(), 1);
    // Another size is another slot.
    let other = store.insert(size("w=3"), &cps("ab"));
    assert_ne!(other, id);
    assert_eq!(store.filled(), 2);
    assert_eq!(store.refused, 0);
}

#[test]
fn text_past_a_slot_is_cut_and_counted() {
    let mut slots = vec![MulticellSlot::EMPTY; 8];
    let mut store = MulticellStore::new(&mut slots);
    let long: Vec<u32> = (0..MulticellSlot::CAPACITY as u32 + 3)
        .map(|i| 0x41 + i)
        .collect();
    let id = store.insert(size("w=7"), &long);
    let kept = store.text_of(Content::multicell(id));
    assert_eq!(kept, &long[..MulticellSlot::CAPACITY]);
    assert_eq!(store.cut, 3);
    assert_eq!(
        store.try_insert(size("w=1"), &[]),
        Err(MulticellRefused::Shape)
    );
}

/// **A block the store has no slot for keeps a head of its own**, the
/// reserved [`MulticellId::REFUSED`]: it reads as a head with no text, so
/// a write over it erases it as a block and its tails find it.
#[test]
fn a_refused_block_keeps_a_head_with_no_text() {
    let mut slots = vec![MulticellSlot::EMPTY; 1];
    let mut store = MulticellStore::new(&mut slots);
    let first = store.insert(size("w=1"), &cps("a"));
    assert!(!first.is_refused());
    // The only slot is named by a cell, so a sweep frees nothing.
    let mut live = MulticellLive::new();
    live.mark(&[head(first)]);
    assert_eq!(
        store.try_insert(size("w=3"), &cps("x")),
        Err(MulticellRefused::Full)
    );
    assert!(store.sweep_due());
    assert_eq!(store.sweep(&live), 0);
    let id = store.insert(size("w=3"), &cps("x"));
    assert_eq!(id, MulticellId::REFUSED);
    assert!(id.is_refused());
    assert_eq!(store.refused, 1);
    let c = Content::multicell(id);
    assert_eq!(c.multicell_id(), Some(MulticellId::REFUSED));
    assert!(store.get(c).is_none());
    assert!(store.text_of(c).is_empty());
    // Its tails name it as any block's do.
    let tail = Content::tail(TailAt::new(2, 0).unwrap());
    assert_eq!(tail.tail_at().map(|t| t.x()), Some(2));
    // A refused head names no slot, so a sweep never counts it live.
    let mut live = MulticellLive::new();
    live.mark(&[head(id)]);
    assert_eq!(store.sweep(&live), 1);
    assert_eq!(store.filled(), 0);
}

/// **A mark after a block goes into the block's text**: the text and the
/// mark are a new slot at the same size, and the old slot is the sweep's
/// to free. Past [`MulticellSlot::CAPACITY`] the mark is cut and counted;
/// a refused head takes none.
#[test]
fn a_combining_mark_extends_the_block() {
    let mut slots = vec![MulticellSlot::EMPTY; 8];
    let mut store = MulticellStore::new(&mut slots);
    let id = store.insert(size("w=3"), &cps("ab"));
    let with = store.try_extend(id, 0x301).unwrap();
    assert_ne!(with, id);
    assert_eq!(
        store.text_of(Content::multicell(with)),
        &cps("ab\u{301}")[..]
    );
    assert_eq!(
        store.get(Content::multicell(with)).unwrap().size(),
        size("w=3")
    );
    assert_eq!(store.text_of(Content::multicell(id)), &cps("ab")[..]);

    let full: Vec<u32> = (0..MulticellSlot::CAPACITY as u32)
        .map(|i| 0x41 + i)
        .collect();
    let id = store.insert(size("w=7"), &full);
    assert_eq!(store.cut, 0);
    assert_eq!(store.try_extend(id, 0x301), Ok(id));
    assert_eq!(store.cut, 1);
    assert_eq!(store.text_of(Content::multicell(id)), &full[..]);

    assert_eq!(
        store.try_extend(MulticellId::REFUSED, 0x301),
        Ok(MulticellId::REFUSED)
    );
    assert_eq!(store.cut, 2);
}

/// **A flood of distinct blocks into a store whose every slot is named
/// sweeps at most once every [`MulticellStore::DUE`] inserts**, so the walk
/// over the screen and the history a sweep takes is amortised.
#[test]
fn a_full_store_sweeps_at_most_once_every_due_inserts() {
    let mut slots = vec![MulticellSlot::EMPTY; 32];
    let mut store = MulticellStore::new(&mut slots);
    let mut named = Vec::new();
    let n = 4000;
    let mut sweeps = 0;
    for i in 0..n {
        let text = [0x4E00 + i];
        let id = match store.try_insert(size("w=2"), &text) {
            Ok(id) => id,
            Err(MulticellRefused::Full) if store.sweep_due() => {
                let mut live = MulticellLive::new();
                live.mark(&named);
                sweeps += 1;
                store.sweep(&live);
                store.insert(size("w=2"), &text)
            }
            Err(_) => {
                store.refused += 1;
                MulticellId::REFUSED
            }
        };
        named.push(head(id));
    }
    let due = MulticellStore::DUE as usize;
    assert_eq!(store.filled(), 32, "the flood filled the store");
    assert!(sweeps >= 1, "a full store swept");
    assert!(
        sweeps <= n as usize / due + 1,
        "{sweeps} sweeps for {n} inserts at one every {due}"
    );
    assert_eq!(store.sweeps().sweeps, sweeps);
    assert!(store.refused > 0);
}

#[test]
fn a_sweep_frees_what_no_head_names_and_moves_the_epoch() {
    let mut slots = vec![MulticellSlot::EMPTY; 8];
    let mut store = MulticellStore::new(&mut slots);
    let a = store.insert(size("w=1"), &cps("a"));
    let b = store.insert(size("w=1"), &cps("b"));
    let before = store.epoch();
    let mut live = MulticellLive::new();
    live.mark(&[head(a), PackedCell::EMPTY]);
    assert_eq!(store.sweep(&live), 1);
    assert_ne!(store.epoch(), before);
    assert!(store.get(Content::multicell(a)).is_some());
    assert!(store.get(Content::multicell(b)).is_none());
    assert_eq!(store.sweeps().freed, 1);
    let before = store.epoch();
    assert_eq!(store.sweep(&live), 0);
    assert_eq!(
        store.epoch(),
        before,
        "a sweep that frees nothing keeps the epoch"
    );
    store.reset();
    assert_eq!(store.filled(), 0);
    assert!(store.get(Content::multicell(a)).is_none());
}

#[test]
fn a_store_over_filled_slots_counts_them() {
    let mut slots = vec![MulticellSlot::EMPTY; 8];
    let id = {
        let mut store = MulticellStore::new(&mut slots);
        store.insert(size("w=2"), &cps("ab"))
    };
    let store = MulticellStore::new(&mut slots);
    assert_eq!(store.filled(), 1);
    assert_eq!(store.text_of(Content::multicell(id)), &cps("ab")[..]);
}
