//! A sweep's tables, written as the report has always printed them.

use super::parse::Count;
use super::rows::{Diag, LEGS, PerLeg};
use super::stats::{Floor, LegStats, Summary, aa_floor, arm_deltas};

fn fmt_med(st: Option<&LegStats>) -> String {
    match st {
        None => format!("{:>9} {:>5}", "-", ""),
        Some(st) => format!("{:9.1} ({:3})", st.median, st.n.0),
    }
}

fn fmt_iqr(st: Option<&LegStats>) -> String {
    match st {
        None => "-".to_string(),
        Some(st) => format!("[{:.1}-{:.1}]", st.q1, st.q3),
    }
}

fn fmt_range(st: Option<&LegStats>) -> String {
    match st {
        None => "-".to_string(),
        Some(st) => format!("[{:.1}-{:.1}]", st.min, st.max),
    }
}

/// One arm's counts and its table of legs.
pub fn summary_text(name: &str, summ: &Summary, diag: Option<&Diag>) -> String {
    let mut o = format!("\n== arm {name} ==\n");
    o += &format!(
        "posted={} paired={} completed={} excluded={} shared_a_frame={}\n",
        summ.posted, summ.paired, summ.completed, summ.excluded, summ.shared
    );
    if let Some(d) = diag.filter(|d| !d.predict_refused.is_empty()) {
        o += &format!("predict refused: {}\n", d.predict_refused.sorted_line());
    }
    if !summ.reasons.is_empty() {
        o += &format!("excluded by reason: {}\n", summ.reasons.sorted_line());
    }
    if let Some(d) = diag {
        let refused = d.landing_refused.unwrap_or(Count(0));
        o += &format!(
            "unpaired posts={} unpaired keylats={} parse failures={} landings refused={refused}\n",
            d.unpaired_posts,
            d.unpaired_keylats,
            d.parse_failures.unwrap_or(Count(0)),
        );
        if refused != Count(0) {
            o += &format!(
                "NOT QUOTABLE -- {refused} landing(s) refused in the runs pooled here: a GPU \
                 end stamp off the clock; {} row(s) on the GPU path with landed=- excluded as \
                 refused-landing\n",
                summ.reasons.get("refused-landing")
            );
        }
    }
    o += &format!(
        "{:>9}  {:>9} {:>5}  {:<24} range [min-max]\n",
        "leg", "median us", "(n)", "IQR [q1-q3]"
    );
    for (leg, st) in summ.legs.named() {
        let st = st.as_ref();
        o += &format!(
            "{leg:>9}  {}  {:<24} {}\n",
            fmt_med(st),
            fmt_iqr(st),
            fmt_range(st)
        );
    }
    o
}

/// The A/A table: each leg's two medians and the floor they set.
pub fn aa_text(base_name: &str, base: &Summary, aa: &Summary) -> String {
    let floors = aa_floor(&base.legs, &aa.legs);
    let mut o = format!("\n== A/A: {base_name} against aa (the same binary) ==\n");
    o += "floor per leg = max(|median delta|, IQR base, IQR aa); a leg that moves by less \
          than its floor is not a result.\n";
    o += &format!(
        "{:>9}  {base_name:>10}  {:>10}  {:>8}  {:>9}  {:>9}  {:>9}\n",
        "leg", "aa", "|d med|", "IQR base", "IQR aa", "floor us"
    );
    for (i, (name, _, _)) in LEGS.iter().enumerate() {
        let (Some(b), Some(a), Some(f)) = (base.legs.0[i], aa.legs.0[i], floors.0[i]) else {
            o += &format!(
                "{name:>9}  {:>10}  {:>10}  {:>8}  {:>9}  {:>9}  {:>9}\n",
                "-", "-", "-", "-", "-", "-"
            );
            continue;
        };
        o += &format!(
            "{name:>9}  {:10.1}  {:10.1}  {:8.1}  {:9.1}  {:9.1}  {:9.1}\n",
            b.median, a.median, f.dmed, f.iqr_base, f.iqr_aa, f.floor
        );
    }
    o
}

/// The A/B table: each leg's move beside the A/A floor, and its verdict.
pub fn deltas_text(
    base_name: &str,
    arm_name: &str,
    base: &Summary,
    arm: &Summary,
    floors: Option<&PerLeg<Option<Floor>>>,
) -> String {
    let deltas = arm_deltas(&base.legs, &arm.legs, floors);
    let mut o = format!("\n== A/B: {arm_name} - {base_name}, per leg, beside the A/A floor ==\n");
    if floors.is_none() {
        o += "no A/A floor (run with --aa): no row can be a result\n";
    }
    o += "result = |delta| above the floor AND the two arms' IQRs disjoint; anything else is \
          not a result\n";
    let (iqr_base, iqr_arm) = (format!("IQR {base_name}"), format!("IQR {arm_name}"));
    o += &format!(
        "{:>9}  {base_name:>10}  {arm_name:>10}  {:>9}  {:>9}  {iqr_base:>16}  {iqr_arm:>16}  \
         verdict\n",
        "leg", "delta us", "floor us"
    );
    for (i, (name, _, _)) in LEGS.iter().enumerate() {
        let Some(d) = &deltas[i] else {
            o += &format!(
                "{name:>9}  {:>10}  {:>10}  {:>9}  {:>9}  {:>16}  {:>16}  -\n",
                "-", "-", "-", "-", "-", "-"
            );
            continue;
        };
        let (b, a) = (base.legs.0[i].as_ref(), arm.legs.0[i].as_ref());
        let floor = d.floor.map_or("-".to_string(), |f| format!("{f:.1}"));
        o += &format!(
            "{name:>9}  {:10.1}  {:10.1}  {:+9.1}  {floor:>9}  {:>16}  {:>16}  {}\n",
            b.map_or(0.0, |s| s.median),
            a.map_or(0.0, |s| s.median),
            d.delta,
            fmt_iqr(b),
            fmt_iqr(a),
            d.verdict
        );
    }
    o
}
