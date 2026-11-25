//! **The images programs send under `g`**, kept so a later notification
//! that names only `g` shows the same icon: "implementations can impose a
//! reasonable max size on the icon cache and evict icons in order of last
//! used" (`[[source: kitty-desktop-notifications]]`). The two bounds, a
//! count and a byte total, are the embedder's.

use alloc::vec::Vec;

/// The bytes the cache holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct IconBytes(usize);

/// When an icon was last kept or read: larger is more recent.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
struct IconUse(u64);

/// One icon, its key and when it was last used.
#[derive(Clone, Debug)]
struct IconEntry {
    key: Vec<u8>,
    data: Vec<u8>,
    used: IconUse,
}

/// **A cache of at most `N` icons and `BYTES` bytes**, which forgets the
/// least recently used first. Reading the bytes it holds reads one field.
///
/// ```
/// let mut icons = baud_domain::IconCache::<2, 8>::new();
/// icons.keep(b"a", b"1234");
/// icons.keep(b"b", b"5678");
/// assert_eq!(icons.get(b"a"), Some(&b"1234"[..]));
/// icons.keep(b"c", b"9");
/// assert_eq!(icons.get(b"b"), None);
/// assert_eq!(icons.total(), 5);
/// ```
///
/// A cache holds at least one icon:
///
/// ```compile_fail,E0080
/// let _ = baud_domain::IconCache::<0, 8>::new();
/// ```
#[derive(Clone, Debug)]
pub struct IconCache<const N: usize, const BYTES: usize> {
    kept: [Option<IconEntry>; N],
    total: IconBytes,
    clock: IconUse,
    #[cfg(test)]
    walks: core::cell::Cell<u32>,
}

impl<const N: usize, const BYTES: usize> IconCache<N, BYTES> {
    /// An empty cache.
    pub const fn new() -> Self {
        const { assert!(N > 0, "an icon cache holds at least one icon") };
        IconCache {
            kept: [const { None }; N],
            total: IconBytes(0),
            clock: IconUse(0),
            #[cfg(test)]
            walks: core::cell::Cell::new(0),
        }
    }

    /// Keep `data` under `key`, in place of what was under it, forgetting
    /// the least recently used until it fits. An image larger than `BYTES`
    /// is not kept, and `key` then names nothing.
    pub fn keep(&mut self, key: &[u8], data: &[u8]) {
        if let Some(i) = self.find(key) {
            self.forget(i);
        }
        if data.len() > BYTES {
            return;
        }
        while self.total() + data.len() > BYTES || self.free().is_none() {
            let Some(i) = self.oldest() else { break };
            self.forget(i);
        }
        let Some(i) = self.free() else { return };
        self.clock = IconUse(self.clock.0 + 1);
        self.total = IconBytes(self.total.0 + data.len());
        self.kept[i] = Some(IconEntry {
            key: key.to_vec(),
            data: data.to_vec(),
            used: self.clock,
        });
    }

    /// The image under `key`, now the most recently used.
    pub fn get(&mut self, key: &[u8]) -> Option<&[u8]> {
        let i = self.find(key)?;
        self.clock = IconUse(self.clock.0 + 1);
        let e = self.kept[i].as_mut()?;
        e.used = self.clock;
        Some(&e.data)
    }

    /// The bytes held.
    pub fn total(&self) -> usize {
        self.total.0
    }

    /// How many icons are held.
    pub fn len(&self) -> usize {
        self.entries().count()
    }

    /// Whether no icon is held.
    pub fn is_empty(&self) -> bool {
        self.entries().next().is_none()
    }

    fn entries(&self) -> impl Iterator<Item = (usize, &IconEntry)> {
        #[cfg(test)]
        self.walks.set(self.walks.get() + 1);
        self.kept
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|e| (i, e)))
    }

    fn find(&self, key: &[u8]) -> Option<usize> {
        self.entries().find(|(_, e)| e.key == key).map(|(i, _)| i)
    }

    fn oldest(&self) -> Option<usize> {
        self.entries().min_by_key(|(_, e)| e.used).map(|(i, _)| i)
    }

    fn free(&self) -> Option<usize> {
        self.kept.iter().position(Option::is_none)
    }

    fn forget(&mut self, i: usize) {
        if let Some(e) = self.kept[i].take() {
            self.total = IconBytes(self.total.0 - e.data.len());
        }
    }
}

impl<const N: usize, const BYTES: usize> Default for IconCache<N, BYTES> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::VecDeque;
    use alloc::format;

    /// The bytes held, walked without counting.
    fn held<const N: usize, const B: usize>(c: &IconCache<N, B>) -> usize {
        c.kept.iter().flatten().map(|e| e.data.len()).sum()
    }

    /// A deterministic stream of small numbers.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, n: u64) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            usize::try_from((self.0 >> 33) % n).expect("small")
        }
    }

    /// **Reading the total walks no entry, and the total is held to the
    /// byte bound after every keep.** Keys repeat, sizes run past the
    /// bound, and reads refresh entries between keeps.
    #[test]
    fn the_icon_cache_holds_its_byte_bound() {
        let mut c = IconCache::<4, 100>::new();
        let mut r = Lcg(7);
        for step in 0..20_000 {
            let key = [u8::try_from(r.next(9)).expect("small")];
            if r.next(3) == 0 {
                let _ = c.get(&key);
            } else {
                c.keep(&key, &alloc::vec![0; r.next(130)]);
            }
            let before = c.walks.get();
            let total = c.total();
            assert_eq!(c.walks.get(), before, "step {step}: the total walked");
            assert!(total <= 100, "step {step}: {total} bytes past the bound");
            assert_eq!(total, held(&c), "step {step}: the total drifted");
            assert!(c.len() <= 4);
        }
    }

    /// A key kept twice holds the later image once; the cache forgets the
    /// least recently used, and a read makes an entry the most recent.
    #[test]
    fn a_key_is_kept_once_and_the_least_recent_goes() {
        let mut c = IconCache::<16, { 4 << 20 }>::new();
        c.keep(b"k1", b"old");
        c.keep(b"k1", b"png");
        assert_eq!((c.len(), c.total()), (1, 3));
        assert_eq!(c.get(b"k1"), Some(&b"png"[..]));
        for i in 0..15 {
            c.keep(format!("o{i}").as_bytes(), b"x");
        }
        assert_eq!(c.len(), 16);
        let _ = c.get(b"k1");
        c.keep(b"o15", b"x");
        assert_eq!(c.get(b"o0"), None, "the least recently used went");
        assert_eq!(c.get(b"k1"), Some(&b"png"[..]), "a read kept it");
        for i in 16..32 {
            c.keep(format!("o{i}").as_bytes(), b"x");
        }
        assert_eq!(c.get(b"k1"), None, "forgotten past the cache's size");
    }

    /// An image past the byte bound is not kept, its key names nothing,
    /// and nothing else is forgotten for it.
    #[test]
    fn an_image_past_the_bound_is_not_kept() {
        let mut c = IconCache::<4, 10>::new();
        c.keep(b"a", b"12345");
        c.keep(b"b", b"old");
        c.keep(b"b", &[0; 11]);
        assert_eq!(c.get(b"b"), None);
        assert_eq!(c.get(b"a"), Some(&b"12345"[..]));
        assert_eq!(c.total(), 5);
        c.keep(b"c", &[0; 10]);
        assert_eq!(
            (c.len(), c.total()),
            (1, 10),
            "a full-size image fits alone"
        );
    }

    /// **Against the list it replaces**: the most recently used last,
    /// the front dropped past the count or the bytes, over 20,000 steps.
    #[test]
    fn it_forgets_what_a_most_recent_last_list_forgets() {
        let mut c = IconCache::<5, 64>::new();
        let mut list: VecDeque<(u8, usize)> = VecDeque::new();
        let mut r = Lcg(11);
        for step in 0..20_000 {
            let k = u8::try_from(r.next(12)).expect("small");
            if r.next(3) == 0 {
                let got = c.get(&[k]).map(<[u8]>::len);
                let want = list.iter().position(|e| e.0 == k).map(|i| {
                    let e = list.remove(i).expect("there");
                    list.push_back(e);
                    e.1
                });
                assert_eq!(got, want, "step {step}: get {k}");
            } else {
                let n = r.next(70);
                c.keep(&[k], &alloc::vec![k; n]);
                list.retain(|e| e.0 != k);
                if n <= 64 {
                    list.push_back((k, n));
                    while list.len() > 5 || list.iter().map(|e| e.1).sum::<usize>() > 64 {
                        list.pop_front();
                    }
                }
            }
            assert_eq!(c.len(), list.len(), "step {step}");
        }
    }
}
