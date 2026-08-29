//! What grapheme segmentation costs, per codepoint.
//!
//! `docs/research/notes/g-text-encoding.md` deferred clustering because the
//! cost was **unmeasured**, and this project's rule is that an unmeasured cost
//! is not an argument. This measures the half that can be measured without
//! deciding storage: the UAX #29 boundary machine itself.
//!
//! It deliberately does **not** measure a clustering ingest path, because none
//! exists — `PackedCell` is eight measured bytes and a cluster needs storage it
//! does not have. What this number bounds is whether segmentation could ever be
//! affordable at all; if it were not, the storage question would be moot.

use baud_core2::{AmbiguousWidth, ClusterBreak, Codepoint, Width};
use std::time::Instant;

/// Codepoints per corpus. One constant, not four literals: the audit counts
/// raw integers per crate and it is right to -- four copies of `1 << 20` is
/// four chances to change one of them.
const CORPUS_LEN: usize = 1 << 20;

/// Codepoint streams a terminal actually meets, each stressing a different rule.
fn corpora() -> Vec<(&'static str, Vec<Codepoint>)> {
    let ascii: Vec<Codepoint> = "the quick brown fox jumps over the lazy dog "
        .chars()
        .cycle()
        .take(CORPUS_LEN)
        .map(Codepoint::from_char)
        .collect();
    // Latin text with combining accents: the common European case.
    let combining: Vec<Codepoint> = "e\u{301}a\u{300}o\u{308}u\u{30a}n\u{303} "
        .chars()
        .cycle()
        .take(CORPUS_LEN)
        .map(Codepoint::from_char)
        .collect();
    // CJK: wide, no combining, no clusters longer than one.
    let cjk: Vec<Codepoint> = "中文字符測試用例文本"
        .chars()
        .cycle()
        .take(CORPUS_LEN)
        .map(Codepoint::from_char)
        .collect();
    // The worst case: ZWJ sequences, flags and Indic conjuncts, which are the
    // rules that need lookback.
    let clusters: Vec<Codepoint> =
        "\u{1f469}\u{200d}\u{1f4bb}\u{1f1ef}\u{1f1f5}\u{915}\u{94d}\u{937} "
            .chars()
            .cycle()
            .take(CORPUS_LEN)
            .map(Codepoint::from_char)
            .collect();
    vec![
        ("ascii", ascii),
        ("latin + combining", combining),
        ("CJK", cjk),
        ("emoji ZWJ + flags + conjuncts", clusters),
    ]
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn time(reps: usize, mut f: impl FnMut() -> u64) -> (f64, f64, f64) {
    let mut rates = Vec::with_capacity(reps);
    for _ in 0..reps {
        let t0 = Instant::now();
        let n = f();
        let dt = t0.elapsed().as_secs_f64();
        rates.push(n as f64 / dt / 1e6);
    }
    let lo = rates.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = rates.iter().cloned().fold(0.0, f64::max);
    (median(rates), lo, hi)
}

pub fn run(reps: usize) {
    println!();
    println!("  grapheme segmentation — Mcp/s (million codepoints per second), median of {reps}");
    println!("  corpus                          width only        + UAX #29 breaks    cost");
    let amb = AmbiguousWidth::Narrow;
    for (name, codes) in corpora() {
        let (w, wlo, whi) = time(reps, || {
            let mut acc = 0u64;
            for c in &codes {
                acc += u64::from(Width::of(*c, amb) == Width::WIDE);
            }
            std::hint::black_box(acc);
            codes.len() as u64
        });

        let (b, blo, bhi) = time(reps, || {
            let mut st = ClusterBreak::new();
            let mut acc = 0u64;
            for c in &codes {
                acc += u64::from(st.is_break(*c));
                acc += u64::from(Width::of(*c, amb) == Width::WIDE);
            }
            std::hint::black_box(acc);
            codes.len() as u64
        });

        println!(
            "  {name:30}  {w:7.1} [{wlo:.0}-{whi:.0}]   {b:7.1} [{blo:.0}-{bhi:.0}]   {:+.0}%",
            (b / w - 1.0) * 100.0
        );
    }
}
