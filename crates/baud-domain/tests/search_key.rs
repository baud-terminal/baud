//! **A search key is the standard's, at every code point**: the compiled
//! trie of `build/search.rs` and the code that reads it, held to D145's
//! NFD(toCasefold(NFD(X))) (`[[source: unicode17-ch3]]`) computed here
//! straight from the pinned `[[source: unicode-data]]` and
//! `[[source: unicode-casefolding]]`, with no table between.

use baud_domain::{ClusterKey, Codepoint, Needle};
use std::collections::HashMap;

fn source(name: &str) -> String {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/research/sources/");
    std::fs::read_to_string(format!("{root}{name}")).expect("the pinned source")
}

struct Ucd {
    ccc: HashMap<u32, u32>,
    decomp: HashMap<u32, Vec<u32>>,
    fold: HashMap<u32, Vec<u32>>,
}

fn ucd() -> Ucd {
    let (mut ccc, mut decomp, mut fold) = (HashMap::new(), HashMap::new(), HashMap::new());
    for line in source("unicode-unicodedata.txt").lines() {
        let f: Vec<&str> = line.split(';').collect();
        let cp = u32::from_str_radix(f[0], 16).unwrap();
        let class: u32 = f[3].parse().unwrap();
        if class != 0 {
            ccc.insert(cp, class);
        }
        if !f[5].is_empty() && !f[5].starts_with('<') {
            let d = f[5].split(' ').map(|x| u32::from_str_radix(x, 16).unwrap());
            decomp.insert(cp, d.collect());
        }
    }
    for line in source("unicode-casefolding.txt").lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(';').map(str::trim).collect();
        if f[1] == "C" || f[1] == "F" {
            let to = f[2].split(' ').map(|x| u32::from_str_radix(x, 16).unwrap());
            fold.insert(u32::from_str_radix(f[0], 16).unwrap(), to.collect());
        }
    }
    Ucd { ccc, decomp, fold }
}

impl Ucd {
    fn nfd_one(&self, cp: u32, out: &mut Vec<u32>) {
        if (0xAC00..0xAC00 + 11172).contains(&cp) {
            let s = cp - 0xAC00;
            out.push(0x1100 + s / 588);
            out.push(0x1161 + (s % 588) / 28);
            if !s.is_multiple_of(28) {
                out.push(0x11A7 + s % 28);
            }
        } else if let Some(d) = self.decomp.get(&cp) {
            for &x in d {
                self.nfd_one(x, out);
            }
        } else {
            out.push(cp);
        }
    }

    fn order(&self, s: &mut [u32]) {
        // Bubble each mark back past every mark of a higher class.
        for i in 1..s.len() {
            let mut j = i;
            let c = |x: u32| self.ccc.get(&x).copied().unwrap_or(0);
            while j > 0 && c(s[j]) != 0 && c(s[j - 1]) > c(s[j]) {
                s.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    fn nfd(&self, s: &[u32]) -> Vec<u32> {
        let mut out = Vec::new();
        for &cp in s {
            self.nfd_one(cp, &mut out);
        }
        self.order(&mut out);
        out
    }

    fn key(&self, s: &[u32]) -> Vec<u32> {
        let folded: Vec<u32> = self
            .nfd(s)
            .into_iter()
            .flat_map(|c| self.fold.get(&c).cloned().unwrap_or(vec![c]))
            .collect();
        self.nfd(&folded)
    }
}

#[test]
fn every_code_points_key_is_d145s() {
    let u = ucd();
    let mut checked = 0;
    for cp in (0..=0x10FFFF).filter_map(Codepoint::new) {
        let want = u.key(&[cp.value()]);
        let got = ClusterKey::cluster(cp, core::iter::empty());
        assert_eq!(got.as_slice(), &want[..], "U+{:04X}", cp.value());
        checked += 1;
    }
    assert_eq!(checked, 0x110000 - 0x800, "every scalar value");
}

#[test]
fn a_clusters_key_reorders_its_marks_across_the_folding() {
    let u = ucd();
    // Alpha, U+0345 (class 240, folding to iota, class 0) and an acute
    // (230): the first NFD puts the acute first, and the folded iota then
    // stays after it.
    let cases: &[&[u32]] = &[
        &[0x3B1, 0x345, 0x301],
        &[0x61, 0x323, 0x302],
        &[0x41, 0x30A],
        &[0x1E0B, 0x323],
        &[0xD55C, 0x11B7],
    ];
    for &cps in cases {
        let base = Codepoint::new(cps[0]).unwrap();
        let got = ClusterKey::cluster(base, cps[1..].iter().copied());
        assert_eq!(got.as_slice(), &u.key(cps)[..], "{cps:X?}");
        let q: Vec<Codepoint> = cps.iter().map(|&c| Codepoint::new(c).unwrap()).collect();
        assert_eq!(Needle::new(&q).unwrap().key(), &u.key(cps)[..], "{cps:X?}");
    }
}
