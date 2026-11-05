//! How many states does the cluster machine actually reach?
//!
//! Deciding whether `decide`/`advance` can be replaced by a transition table
//! needs the reachable state count, not the nominal one: 15 x 2 x 3 x 3 = 270
//! combinations exist, but most may be unreachable.

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

/// The cluster transition table as `scripts/gen-cluster-table.py` reads it:
/// the state count, then one byte per (state, input), then one ready byte per
/// state, then two bit tables of one bit per (state, input): where a join the
/// extended rules make parts under legacy clusters, and where it parts with
/// spacing marks joined ([`ClusterCells`]).
///
/// States are numbered in breadth-first order from the initial one, and the
/// map is only ever looked up, never iterated, so the bytes do not depend on
/// a hash seed.
fn cluster_table_bytes() -> Vec<u8> {
    use std::collections::HashMap;
    let start = ClusterRules::new().to_bits();
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
            let mut m = ClusterRules::from_bits(st);
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
        // From the fields: the rule form has no table id to ask. Until
        // 2026-10-16 one type carried both, and asking its table here
        // said "not ready" of every state from `from_bits`.
        ready[si] = u8::from(matches!(
            ClusterRules::from_bits(st).ordinary_run_blocker(),
            RunBlocker::Ready
        ));
        for b in 0u16..256 {
            if (b & 0xf) >= 14 {
                continue;
            }
            let (cl, lb) = (
                GraphemeClass::FROM_INDEX[(b & 0xf) as usize],
                Lookback::from_bits((b >> 4) as u8),
            );
            let mut m = ClusterRules::from_bits(st);
            let brk = m.is_break_by_rules(cl, lb);
            table[si * 256 + b as usize] = (u8::from(brk) << 7) | id[&m.to_bits()];
        }
    }
    assert_eq!(
        ready.iter().filter(|&&r| r == 1).count(),
        1,
        "exactly one state is ordinary-run-ready"
    );
    let mut out = Vec::new();
    out.extend_from_slice(&(order.len() as u32).to_le_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&ready);
    for cells in [ClusterCells::Legacy, ClusterCells::LegacyMarksJoined] {
        out.extend_from_slice(&parts_bits(&order, cells));
    }
    out
}

/// One bit per (state, input), bit `i & 7` of byte `i >> 3` for `i = state *
/// 256 + input`: the extended rules join there and the variant `cells`
/// breaks, each from the rules ([`ClusterRules::is_break_in`]).
fn parts_bits(order: &[u16], cells: ClusterCells) -> Vec<u8> {
    let mut bits = vec![0u8; order.len() * 256 / 8];
    for (si, &st) in order.iter().enumerate() {
        for b in 0u8..=255 {
            if (b & 0xf) >= 14 {
                continue;
            }
            let (cl, lb) = (
                GraphemeClass::FROM_INDEX[usize::from(b & 0xf)],
                Lookback::from_bits(b >> 4),
            );
            let joins = !ClusterRules::from_bits(st).is_break_by_rules(cl, lb);
            let parts = ClusterRules::from_bits(st).is_break_in(cells, cl, lb);
            if joins && parts {
                let i = si * 256 + usize::from(b);
                bits[i >> 3] |= 1 << (i & 7);
            }
        }
    }
    bits
}

/// Dump the cluster transition table for `scripts/gen-cluster-table.py`.
/// Run with `--ignored` to regenerate.
#[test]
#[ignore]
fn emit_cluster_table() {
    let out = cluster_table_bytes();
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/cluster.bin"), &out).expect("write");
    println!("\n  {} bytes", out.len());
}

/// The committed `cluster.bin` is what the emitter writes, byte for byte,
/// and the emitter writes the same bytes twice in one process, as a guard;
/// determinism rests on the maps being looked up and never iterated.
#[test]
fn the_committed_cluster_bin_is_the_emitters_output() {
    let (a, b) = (cluster_table_bytes(), cluster_table_bytes());
    assert!(a == b, "the emitter is not deterministic");
    let committed: &[u8] = include_bytes!("../cluster.bin");
    assert_eq!(a.len(), committed.len(), "cluster.bin size");
    if let Some(i) = (0..a.len()).find(|&i| a[i] != committed[i]) {
        panic!(
            "cluster.bin differs from the emitter at byte {i}: committed {:#04x}, emitted {:#04x}",
            committed[i], a[i]
        );
    }
}

/// `cluster_data.rs` is what `cluster.bin` formats to, so the chain from the
/// rules to the compiled table is closed: rules to `cluster.bin` above, and
/// `cluster.bin` to `cluster_data.rs` here.
#[test]
fn cluster_data_rs_is_what_cluster_bin_formats_to() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-cluster-table.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the formatter");
    assert!(
        out.status.success(),
        "cluster_data.rs is stale -- rerun scripts/gen-cluster-table.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The committed transition table must reproduce the rule implementation for
/// every reachable state and every input.
///
/// `cluster_next` reads the committed table and `is_break_by_rules` walks
/// the rules, so this checks the artefact against the rules that generated
/// it, which is the same contract `the_merged_table_agrees_with_both_tries`
/// holds for the property table. It does not see the ready byte, which is
/// why `the_committed_cluster_bin_is_the_emitters_output` exists.
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
