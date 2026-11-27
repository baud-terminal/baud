//! How many states does the cluster machine actually reach, and does the
//! table `build/cluster.rs` compiles answer as the crate's rules do?
//!
//! Deciding whether `decide`/`advance` can be replaced by a transition table
//! needed the reachable state count, not the nominal one: 15 x 2 x 3 x 3 =
//! 270 combinations exist, and 94 are reachable.

use baud_domain::{ClusterCells, ClusterRules, GraphemeClass, Lookback, RunBlocker};
use std::collections::HashSet;

#[test]
fn report_reachable_cluster_states() {
    let start = ClusterRules::new().to_bits();
    let mut seen: HashSet<u16> = HashSet::from([start]);
    let mut queue = vec![start];
    // The transition input is exactly the byte the flat property table returns:
    // four bits of class, four of lookback.
    let inputs: Vec<(GraphemeClass, Lookback)> = (0u8..14)
        .flat_map(|c| {
            (0u8..16).map(move |l| {
                (
                    GraphemeClass::FROM_INDEX[c as usize],
                    Lookback::from_bits(l),
                )
            })
        })
        .collect();
    while let Some(st) = queue.pop() {
        for &(cl, lb) in &inputs {
            let mut m = ClusterRules::from_bits(st);
            let _ = m.is_break_by_rules(cl, lb);
            let next = m.to_bits();
            if seen.insert(next) {
                queue.push(next);
            }
        }
    }
    println!("\n  reachable states: {}", seen.len());
    println!("  transition inputs: {}", inputs.len());
    println!(
        "  table at 2 bytes an entry: {} x {} = {} bytes",
        seen.len(),
        inputs.len(),
        seen.len() * inputs.len() * 2
    );
    println!(
        "  dense over the 9-bit encoding: 512 x 256 x 2 = {} bytes",
        512 * 256 * 2
    );
}

/// The compiled transition table must reproduce the rule implementation for
/// every reachable state and every input.
///
/// `cluster_next` reads the table `build/cluster.rs` compiles from its own
/// implementation of the rules, and `is_break_by_rules` walks the crate's,
/// so this holds the two implementations equal, the same contract
/// `the_merged_table_agrees_with_both_tries` holds for the property table.
/// It does not see the ready state or the parts tables, which is why
/// `the_parts_tables_agree_with_the_rules` exists.
#[test]
fn the_cluster_table_agrees_with_the_rules() {
    use baud_domain::cluster_next;
    use std::collections::HashMap;
    let start = ClusterRules::new().to_bits();
    let mut id: HashMap<u16, u8> = HashMap::from([(start, 0)]);
    let mut order: Vec<u16> = vec![start];
    let mut i = 0usize;
    while i < order.len() {
        let st = order[i];
        i += 1;
        for b in 0u16..256 {
            if (b & 0xf) >= 14 {
                continue;
            }
            let mut m = ClusterRules::from_bits(st);
            let _ = m.is_break_by_rules(
                GraphemeClass::FROM_INDEX[(b & 0xf) as usize],
                Lookback::from_bits((b >> 4) as u8),
            );
            let n = m.to_bits();
            if !id.contains_key(&n) {
                let next = id.len() as u8;
                id.insert(n, next);
                order.push(n);
            }
        }
    }
    let mut checked = 0usize;
    for (si, &st) in order.iter().enumerate() {
        for b in 0u16..256 {
            if (b & 0xf) >= 14 {
                continue;
            }
            let (cl, lb) = (
                GraphemeClass::FROM_INDEX[(b & 0xf) as usize],
                Lookback::from_bits((b >> 4) as u8),
            );
            let mut m = ClusterRules::from_bits(st);
            let want_brk = m.is_break_by_rules(cl, lb);
            let want_next = id[&m.to_bits()];
            let e = cluster_next(si as u8, b as u8);
            assert_eq!(
                e & 0x80 != 0,
                want_brk,
                "break at state {si} input {b:#04x}"
            );
            assert_eq!(e & 0x7f, want_next, "next state at {si} input {b:#04x}");
            checked += 1;
        }
    }
    assert!(checked > 20_000, "only checked {checked}");
}

/// **The two parts tables and the ready state agree with the rules**, at
/// every state the table reaches: the table's machine and the rules' walked
/// side by side from the start of text, each (state, input) parts under a
/// variant exactly where the extended rules join and the variant's own
/// rules ([`ClusterRules::is_break_in`]) break, and the table's ready state
/// is the rules' `prev = Other` with nothing live, the one state
/// `ordinary_run_ready` accepts.
#[test]
fn the_parts_tables_agree_with_the_rules() {
    use baud_domain::ClusterBreak;
    use std::collections::HashMap;
    let start = (ClusterBreak::new(), ClusterRules::new());
    let mut rules_of: HashMap<ClusterBreak, u16> = HashMap::from([(start.0, start.1.to_bits())]);
    let mut queue = vec![start];
    let mut checked = 0usize;
    while let Some((e, r)) = queue.pop() {
        for b in 0u8..=255 {
            if (b & 0xf) >= 14 {
                continue;
            }
            let (cl, lb) = (
                GraphemeClass::FROM_INDEX[usize::from(b & 0xf)],
                Lookback::from_bits(b >> 4),
            );
            let joins = !{
                let mut t = r;
                t.is_break_by_rules(cl, lb)
            };
            for cells in [ClusterCells::Legacy, ClusterCells::LegacyMarksJoined] {
                let parts = joins && {
                    let mut t = r;
                    t.is_break_in(cells, cl, lb)
                };
                assert_eq!(
                    e.parts(cells, cl, lb),
                    parts,
                    "{cells:?} parts at {:?} input {b:#04x}",
                    ClusterRules::from_bits(r.to_bits())
                );
                checked += 1;
            }
            let (mut e2, mut r2) = (e, r);
            assert_eq!(e2.is_break_classified(cl, lb), r2.is_break_by_rules(cl, lb));
            match rules_of.get(&e2) {
                Some(&bits) => assert_eq!(bits, r2.to_bits(), "one table state, two rule states"),
                None => {
                    rules_of.insert(e2, r2.to_bits());
                    queue.push((e2, r2));
                }
            }
        }
    }
    assert_eq!(rules_of.len(), 94, "the reachable state count moved");
    assert!(checked > 40_000, "only checked {checked}");
    let ready: Vec<&ClusterBreak> = rules_of
        .iter()
        .filter(|(_, bits)| {
            matches!(
                ClusterRules::from_bits(**bits).ordinary_run_blocker(),
                RunBlocker::Ready
            )
        })
        .map(|(e, _)| e)
        .collect();
    assert_eq!(ready, [&ClusterBreak::after_ascii_run()], "the ready state");
    assert!(ClusterBreak::after_ascii_run().ordinary_run_ready());
    assert_eq!(
        rules_of[&ClusterBreak::after_ascii_run()],
        ClusterRules::after_ascii_run().to_bits()
    );
}
