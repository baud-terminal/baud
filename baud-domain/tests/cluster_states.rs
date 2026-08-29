//! How many states does the cluster machine actually reach?
//!
//! Deciding whether `decide`/`advance` can be replaced by a transition table
//! needs the reachable state count, not the nominal one: 15 x 2 x 3 x 3 = 270
//! combinations exist, but most may be unreachable.

use baud_domain::{ClusterBreak, GraphemeClass, Lookback};
use std::collections::HashSet;

#[test]
fn report_reachable_cluster_states() {
    let start = ClusterBreak::new().to_bits();
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
            let mut m = ClusterBreak::from_bits(st);
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

/// Dump the cluster transition table for `scripts/gen-cluster-table.py`.
/// Run with `--ignored` to regenerate.
#[test]
#[ignore]
fn emit_cluster_table() {
    use std::collections::HashMap;
    // BFS from the initial state so ids are stable across runs.
    let start = ClusterBreak::new().to_bits();
    let mut id: HashMap<u16, u8> = HashMap::from([(start, 0)]);
    let mut order: Vec<u16> = vec![start];
    let mut i = 0usize;
    while i < order.len() {
        let st = order[i];
        i += 1;
        for b in 0u16..256 {
            let (cl, lb) = (
                GraphemeClass::FROM_INDEX[(b & 0xf) as usize],
                Lookback::from_bits((b >> 4) as u8),
            );
            if (b & 0xf) >= 14 {
                continue;
            }
            let mut m = ClusterBreak::from_bits(st);
            let _ = m.is_break_by_rules(cl, lb);
            let n = m.to_bits();
            if !id.contains_key(&n) {
                let next = id.len() as u8;
                id.insert(n, next);
                order.push(n);
            }
        }
    }
    assert!(
        order.len() <= 128,
        "state id must fit 7 bits: {}",
        order.len()
    );

    // One byte an entry: bit 7 is the break decision, bits 0..7 the next state.
    let mut table = vec![0u8; order.len() * 256];
    let mut ready = vec![0u8; order.len()];
    for (si, &st) in order.iter().enumerate() {
        ready[si] = u8::from(ClusterBreak::from_bits(st).ordinary_run_ready());
        for b in 0u16..256 {
            if (b & 0xf) >= 14 {
                continue;
            }
            let (cl, lb) = (
                GraphemeClass::FROM_INDEX[(b & 0xf) as usize],
                Lookback::from_bits((b >> 4) as u8),
            );
            let mut m = ClusterBreak::from_bits(st);
            let brk = m.is_break_by_rules(cl, lb);
            table[si * 256 + b as usize] = (u8::from(brk) << 7) | id[&m.to_bits()];
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(&(order.len() as u32).to_le_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&ready);
    for d in ["cluster.bin", "../cluster.bin"] {
        if std::fs::write(d, &out).is_ok() {
            break;
        }
    }
    println!(
        "\n  {} states, table {} bytes, {} of them ordinary-run-ready",
        order.len(),
        table.len(),
        ready.iter().filter(|&&r| r == 1).count()
    );
}

/// The committed transition table must reproduce the rule implementation for
/// every reachable state and every input.
///
/// Runs only **without** `cluster-table`, because with the feature on
/// `is_break_classified` *is* the table and the comparison would be circular.
/// So this checks the artefact against the rules that generated it, which is
/// the same contract `the_merged_table_agrees_with_both_tries` holds for the
/// property table.
#[test]
fn the_cluster_table_agrees_with_the_rules() {
    use baud_domain::cluster_next;
    use std::collections::HashMap;
    let start = ClusterBreak::new().to_bits();
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
            let mut m = ClusterBreak::from_bits(st);
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
            let mut m = ClusterBreak::from_bits(st);
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
