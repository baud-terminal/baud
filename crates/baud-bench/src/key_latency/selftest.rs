//! `--selftest`: the pure functions against the on-disk fixtures and a few
//! hand-built cases. No app, no posting.

use super::cli::Child;
use super::parse::{
    Count, Front, INSTRUMENT_REFUSED, Ns, OFFSETS, Post, Record, Seq, parse_front_line,
    parse_keylat_line, parse_keylat_lines, parse_post_line,
};
use super::py;
use super::report::summary_text;
use super::rows::{
    Diag, PerLeg, Row, SHARED_FRAME, Tally, exclude_refused_landings, is_completed, legs_of,
    pair_by_order, shared_frame_count,
};
use super::stats::{
    DEFAULT_WINDOW, Floor, Iqrs, Ms, Round, WarmUp, aa_floor, arm_deltas, gap_bounds, leg_stats,
    merge_diags, refused_runs, rotate, settled_rounds, summarize, warmup_measured_nothing,
    window_for_gap, window_ns,
};
use super::sweep::{STREAM_FLAG, child_env};
use std::path::Path;

/// The directory the committed fixtures are in.
pub const DEFAULT_FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/tests/keylat");

/// The default pairing window, in nanoseconds.
pub fn default_window() -> Ns {
    window_ns(DEFAULT_WINDOW)
}

fn approx(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-6)
}

/// A well-formed record line with every known token, overridable.
pub fn record_line(seq: Seq, event: Ns, over: &[(&str, &str)]) -> String {
    let default = |name: &str| match name {
        "handled" => "+300.0",
        "written" => "+350.0",
        "echo" => "+360.0",
        "fed" => "+365.0",
        "commit" => "+370.0",
        "landed" => "+380.0",
        "shown" => "+385.0",
        "vsync" => "+400.0",
        _ => "+416.7",
    };
    let mut line = format!("keylat: seq={} event={}", seq.0, event.0);
    for name in OFFSETS {
        let v = over
            .iter()
            .find(|(k, _)| *k == name)
            .map_or(default(name), |(_, v)| *v);
        line += &format!(" {name}={v}");
    }
    line
}

fn rec(line: &str) -> Record {
    parse_keylat_line(line)
        .expect("a hand-built record parses")
        .expect("a hand-built line is a record")
}

fn post(seq: i128, ns: i128, ch: &str) -> Post {
    Post {
        seq: Seq(seq),
        ns: Ns(ns),
        ch: ch.to_string(),
    }
}

/// `+<us>` of `t - e`, one decimal.
fn off(e: i128, t: i128) -> String {
    format!("+{:.1}", (t - e) as f64 / 1000.0)
}

/// One commit at `at`, the legs before it inside 40 us.
fn frame_line(seq: i128, e: i128, at: i128) -> String {
    let (commit, landed, shown, vsync, hi) = (
        off(e, at),
        off(e, at + 10_000),
        off(e, at + 15_000),
        off(e, at + 30_000),
        off(e, at + 16_697_000),
    );
    record_line(
        Seq(seq),
        Ns(e),
        &[
            ("handled", "+10.0"),
            ("written", "+20.0"),
            ("echo", "+30.0"),
            ("fed", "+40.0"),
            ("commit", &commit),
            ("landed", &landed),
            ("shown", &shown),
            ("vsync", &vsync),
            ("vsync_hi", &hi),
        ],
    )
}

/// The exclusions by seq, a later row of one seq keeping the first's place,
/// as a dict does.
fn exclusions(rows: &[Row]) -> Vec<(i128, String)> {
    let mut ex: Vec<(i128, String)> = Vec::new();
    for r in rows {
        if let Some(why) = &r.exclude_reason {
            match ex.iter_mut().find(|(k, _)| *k == r.seq.0) {
                Some((_, have)) => *have = why.clone(),
                None => ex.push((r.seq.0, why.clone())),
            }
        }
    }
    ex
}

fn repr_exclusions(ex: &[(i128, String)]) -> String {
    let items: Vec<String> = ex
        .iter()
        .map(|(k, v)| format!("{k}: {}", py::repr_str(v)))
        .collect();
    format!("{{{}}}", items.join(", "))
}

/// What the self-test came to: what it printed, and the exit code.
pub struct Outcome {
    /// Its stdout.
    pub stdout: String,
    /// Its exit code: 0, or 3 when a check failed.
    pub code: i32,
}

/// The self-test over the fixtures in `dir`. An `Err` is a fixture that
/// cannot be read, which is for stderr and exit 1.
pub fn selftest(dir: &str) -> Result<Outcome, String> {
    let kfile = format!("{}/keylat-fixture.txt", dir.trim_end_matches('/'));
    let pfile = format!("{}/keylat-posts-fixture.txt", dir.trim_end_matches('/'));
    let read =
        |p: &str| std::fs::read_to_string(Path::new(p)).map_err(|e| format!("reading {p}: {e}"));
    let (ktext, ptext) = (read(&kfile)?, read(&pfile)?);
    let mut failures: Vec<String> = Vec::new();

    // Parsing filters non-keylat noise, the "N keys open" report line and
    // the `landing refused` line; a record missing a known token is a
    // counted parse failure (seq=10 lacks vsync_hi), not a silent `-`.
    let parsed = parse_keylat_lines(&py::universal_lines(&ktext));
    let (keylats, pfail, refusals, reports) = (
        &parsed.records,
        &parsed.failures,
        parsed.refusals,
        &parsed.reports,
    );
    let posts: Vec<Post> = py::universal_lines(&ptext)
        .into_iter()
        .filter_map(parse_post_line)
        .collect();
    if keylats.len() != 9 {
        failures.push(format!("expected 9 keylat records, got {}", keylats.len()));
    }
    if posts.len() != 10 {
        failures.push(format!("expected 10 posts, got {}", posts.len()));
    }
    if pfail.len() != 1 || !pfail[0].contains("vsync_hi") || !pfail[0].contains("seq=10") {
        failures.push(format!(
            "expected one parse failure naming vsync_hi on seq=10, got {}",
            py::repr_list(pfail)
        ));
    }
    if refusals != Count(1) {
        failures.push(format!("expected 1 landing-refused line, got {refusals}"));
    }
    // The two classes of non-record line, told apart the way a run tells
    // them apart: the open-keys report, and the `refused` lines by why.
    let mut refused_seen = Tally::default();
    for r in reports.iter().filter(|r| r.starts_with(INSTRUMENT_REFUSED)) {
        refused_seen.add(py::strip(&r[INSTRUMENT_REFUSED.len()..]), Count(1));
    }
    let opens: Vec<String> = reports
        .iter()
        .filter(|r| !r.starts_with(INSTRUMENT_REFUSED))
        .cloned()
        .collect();
    if opens != ["keylat: 3 keys open"] {
        failures.push(format!(
            "expected the one open-keys line, got {}",
            py::repr_list(&opens)
        ));
    }
    if !refused_seen.equals(&[("zone", Count(1)), ("occupied", Count(1))]) {
        failures.push(format!(
            "the fixture's refusals must be counted by why: {}",
            refused_seen.repr_counter()
        ));
    }
    // And the per-round counters are summed by why rather than overwritten.
    let one = |refused: &[(&str, usize)]| {
        let mut d = Diag {
            paired: Count(1),
            ..Diag::default()
        };
        for (k, n) in refused {
            d.instrument_refused.add(k, Count(*n));
        }
        Some(d)
    };
    let pooled = merge_diags(&[one(&[("zone", 2), ("hold", 1)]), one(&[("zone", 5)])])
        .expect("two diags merge");
    if !pooled
        .instrument_refused
        .equals(&[("zone", Count(7)), ("hold", Count(1))])
    {
        failures.push(format!(
            "refusals must pool by why: {}",
            pooled.instrument_refused.repr_dict()
        ));
    }
    if pooled.paired != Count(2) {
        failures.push(format!("paired must pool: {}", pooled.repr()));
    }

    let (mut rows, mut diag) = pair_by_order(&posts, keylats, default_window());
    if diag.paired != Count(9)
        || diag.unpaired_posts != Count(1)
        || diag.unpaired_keylats != Count(0)
    {
        failures.push(format!("pairing counts wrong: {}", diag.repr()));
    }
    diag.parse_failures = Some(Count(pfail.len())); // what a run adds beside the pairing
    diag.landing_refused = Some(refusals);
    // The run printed one `landing refused` line, so seq 9 -- committed to
    // the GPU, `landed=-` -- is a refused landing and is excluded rather
    // than pooled with a `shown` from the wrong surface.
    let excluded = exclude_refused_landings(&mut rows, refusals);
    diag.refused_landing_rows = Some(excluded);
    if excluded != Count(1) {
        failures.push(format!("expected 1 refused-landing row, got {excluded}"));
    }

    // Exclusions: seq 6's event precedes its post; seq 7's trails it by
    // 200 ms (>= the 100 ms window); post 8 met the app's seq=88; seq 9's
    // landing was refused.
    let ex = exclusions(&rows);
    let want_ex: Vec<(i128, String)> = [
        (6, "event<post"),
        (7, "event-post>=window"),
        (8, "seq-mismatch"),
        (9, "refused-landing"),
    ]
    .map(|(k, v)| (k, v.to_string()))
    .to_vec();
    let mut sorted_ex = ex.clone();
    sorted_ex.sort();
    if sorted_ex != want_ex {
        failures.push(format!(
            "expected exclusions {}, got {}",
            repr_exclusions(&want_ex),
            repr_exclusions(&ex)
        ));
    }
    if diag.seq_mismatch != Count(1)
        || diag.window != Count(1)
        || diag.event_before_post != Count(1)
    {
        failures.push(format!("diag counts wrong: {}", diag.repr()));
    }

    let summ = summarize(&rows, Some(&diag));
    if summ.posted != Count(10) || summ.paired != Count(9) {
        failures.push(format!(
            "posted/paired: {}/{} != 10/9",
            summ.posted, summ.paired
        ));
    }
    if summ.completed != Count(5) {
        failures.push(format!("expected 5 completed, got {}", summ.completed));
    }
    if summ.excluded != Count(4) {
        failures.push(format!("expected 4 excluded, got {}", summ.excluded));
    }
    if summ.reasons.get("refused-landing") != Count(1) {
        failures.push(format!(
            "refused-landing must be counted by reason: {}",
            summ.reasons.repr_dict()
        ));
    }
    if summ.shared != Count(0) {
        failures.push(format!("expected 0 shared-frame keys, got {}", summ.shared));
    }

    // seq 9 carries `landed=-` and an unknown token: it parses (the token
    // is dropped) and, the run having printed a refusal, is excluded -- so
    // every leg has n=5, from seq 1-5 alone.
    for (name, want) in [
        ("stamp", 0.0),
        ("deliver", 300.0),
        ("write", 50.0),
        ("child", 10.0),
        ("wake", 5.0),
        ("build", 5.0),
        ("gpu", 10.0),
        ("show", 5.0),
        ("app", 85.0),
        ("vsync", 35.0),
        ("vsync_hi", 51.7),
        ("to_shown", 385.0),
        ("total", 420.0),
        ("total_hi", 436.7),
    ] {
        let st = summ.legs.get(name);
        if !st.is_some_and(|s| approx(Some(s.median), want) && s.n == Count(5)) {
            failures.push(format!(
                "leg {name}: {} != median {} n 5",
                st.map_or("None".to_string(), |s| s.repr()),
                py::repr_float(want)
            ));
        }
    }
    // The IQR is inclusive and within the data; never the max-min spread.
    if let Some(st) = summ.legs.get("total") {
        if !(st.min <= st.q1 && st.q1 <= st.median && st.median <= st.q3 && st.q3 <= st.max) {
            failures.push(format!("quartiles out of order: {}", st.repr()));
        }
    }

    hand_built(&mut failures);

    if !failures.is_empty() {
        let mut stdout = "SELFTEST FAILED:\n".to_string();
        for fmsg in &failures {
            stdout += &format!("  - {fmsg}\n");
        }
        return Ok(Outcome { stdout, code: 3 });
    }
    let mut ex_items = ex;
    ex_items.sort();
    let ex_line: Vec<String> = ex_items
        .iter()
        .map(|(k, v)| format!("seq={k}: {v}"))
        .collect();
    let mut o = "SELFTEST OK\n".to_string();
    o += &format!("  fixtures: {kfile}\n");
    o += &format!("            {pfile}\n");
    o += &format!(
        "  parsed {} keylat records, {} posts, {} parse failure ({} on seq=10), {refusals} \
         landing refused\n",
        keylats.len(),
        posts.len(),
        pfail.len(),
        pfail[0].split(':').next().unwrap_or("")
    );
    o += &format!(
        "  pairing: 5 completed, 4 excluded ({})\n",
        ex_line.join(", ")
    );
    o += &summary_text("fixture", &summ, Some(&diag));
    o += "\n  extra cases: strict tokens (missing/bare value -> parse failure, unknown token \
          dropped); `-` legs -> None and not completed;\n  `N keys open`/`landing \
          refused`/noise skipped; negative leg excluded; 2 keys share a frame within 200 ns \
          off non-multiple-\n  of-1000 events (0 with exact equality); refused landings: CPU \
          path untouched, GPU path with a refusal excludes every\n  committed landed=- row, \
          commit=- stays incomplete, an excluded row keeps its reason, refused_runs names \
          arm/round/count;\n  empty warm-up round reported; gap bounds -/+ a quarter and the \
          window follows the MINIMUM gap; arm order rotates;\n  A/A floor ignores a 163 ms \
          outlier; arm deltas: result only above the floor with disjoint IQRs, three \
          not-a-result\n  verdicts; one round flagged warm-up; --child none is the no-echo \
          control.\n";
    Ok(Outcome { stdout: o, code: 0 })
}

/// The checks that read no fixture.
fn hand_built(failures: &mut Vec<String>) {
    let mut check = |ok: bool, msg: &str| {
        if !ok {
            failures.push(msg.to_string());
        }
    };
    let window = default_window();

    // A `-` leg parses to None and is left out of its median; a key whose
    // vsync is `-` is not completed.
    let partial = rec(&record_line(
        Seq(9),
        Ns(2_000_000_000),
        &[
            ("landed", "-"),
            ("shown", "-"),
            ("vsync", "-"),
            ("vsync_hi", "-"),
        ],
    ));
    check(
        partial.stamps.landed.is_none() && partial.stamps.vsync.is_none(),
        "missing legs should parse to None",
    );
    let (prow, _) = pair_by_order(&[post(9, 2_000_000_000, "z")], &[partial], window);
    check(
        !is_completed(&prow[0]),
        "a key with vsync=- must not count as completed",
    );
    let plegs = legs_of(&prow[0]);
    check(
        plegs.get("gpu").is_none() && plegs.get("total").is_none(),
        "legs touching a missing stage must be None",
    );
    check(
        approx(*plegs.get("build"), 5.0),
        "build leg of the partial row must be 5.0",
    );

    // Strictness: a record missing a known token, or with a value that is
    // neither +<us> nor -, is a parse failure; unknown tokens are ignored.
    for bad in [
        "keylat: seq=1 event=1 handled=+1.0 written=+2.0 echo=- fed=- commit=- landed=- shown=- \
         vsync=-",
        "keylat: seq=1 event=1 handled=1.0 written=+2.0 echo=- fed=- commit=- landed=- shown=- \
         vsync=- vsync_hi=-",
        "keylat: seq=x event=1 handled=+1.0 written=+2.0 echo=- fed=- commit=- landed=- shown=- \
         vsync=- vsync_hi=-",
    ] {
        check(
            parse_keylat_line(bad).is_err(),
            &format!("must be a parse failure: {bad}"),
        );
    }
    let line = record_line(Seq(4), Ns(4_000_000_000), &[]) + " future=+1.0 note=x";
    check(
        parse_keylat_line(&line).is_ok_and(|r| r.is_some_and(|r| r.seq == Seq(4))),
        "unknown tokens must be tolerated and dropped",
    );
    // A frame committed before its echo was fed is a negative build, and
    // the row is excluded.
    let plain = rec(&record_line(
        Seq(12),
        Ns(12_000_000_000),
        &[
            ("commit", "+355.0"),
            ("landed", "+365.0"),
            ("shown", "+370.0"),
        ],
    ));
    let (nrow, _) = pair_by_order(&[post(12, 12_000_000_000, "q")], &[plain], window);
    check(
        nrow[0]
            .exclude_reason
            .as_deref()
            .is_some_and(|r| r.contains("build")),
        "commit before fed must be a negative build",
    );

    // The `N keys open` report line, the refusal line and unrelated lines
    // are not records.
    for noise in [
        "keylat: 3 keys open",
        "keylat: landing refused ended=1003500390000 now=1003500385000",
        "startup: first present  101.0 ms",
    ] {
        check(
            parse_keylat_line(noise) == Ok(None),
            &format!("must not parse as a record: {noise}"),
        );
    }

    // A negative leg after `stamp` -- here the GPU end after the wake that
    // reported it, show < 0 -- excludes the row and names the leg.
    let neg = rec(&record_line(
        Seq(5),
        Ns(5_000_000_000),
        &[("landed", "+390.0"), ("shown", "+385.0")],
    ));
    let (nrow, ndiag) = pair_by_order(&[post(5, 5_000_000_000, "n")], &[neg], window);
    check(
        nrow[0].exclude_reason.as_deref() == Some("negative show"),
        "expected 'negative show' exclusion",
    );
    check(ndiag.negative == Count(1), "negative count");

    // Shared-frame detection: commits within 200 ns share a frame. The
    // events are NOT multiples of 1000 ns and the true commit is one
    // instant, 5_000_123_456; each key's offset prints at 0.1 us from its
    // own event, so the two reconstruct 36 ns apart -- exact equality would
    // miss them and the case cannot pass by construction.
    let c = 5_000_123_456;
    let (e1, e2, e3) = (5_000_000_017, 5_000_010_053, 5_000_500_000);
    let skeylats = [
        rec(&frame_line(1, e1, c)),
        rec(&frame_line(2, e2, c)),
        rec(&frame_line(3, e3, e3 + 370_000)), // its own frame
    ];
    let commit = |r: &Record| r.stamps.commit.expect("the frame commits").0;
    check(
        commit(&skeylats[0]) != commit(&skeylats[1]),
        "the shared-frame case must not reconstruct equal commits",
    );
    check(
        (commit(&skeylats[0]) - commit(&skeylats[1])).abs() <= SHARED_FRAME.0,
        "the shared-frame case must reconstruct within the tolerance",
    );
    let sposts = [post(1, e1, "a"), post(2, e2, "b"), post(3, e3, "c")];
    let (srows, _) = pair_by_order(&sposts, &skeylats, window);
    check(
        summarize(&srows, None).shared == Count(2),
        "expected 2 keys sharing a frame",
    );
    check(
        shared_frame_count(&srows, Ns(0)) == Count(0),
        "with exact equality the case must NOT be shared (positive control)",
    );

    // Refused landings. The CPU path -- every row `landed=-`, no refusal
    // line -- is untouched and complete; the same rows in a run that DID
    // print a refusal are all refused landings (the positive control); a
    // row that never reached the GPU (`commit=-`) is incomplete, not
    // refused, and an already-excluded row keeps its own reason.
    let at = |i: i128| 7_000_000_000 + i * 1_000_000;
    let cpu_posts: Vec<Post> = [1, 2, 3].map(|i| post(i, at(i), "c")).to_vec();
    let cpu_klines: Vec<Record> = [1, 2, 3]
        .map(|i| rec(&record_line(Seq(i), Ns(at(i)), &[("landed", "-")])))
        .to_vec();
    let (mut cpu_rows, _) = pair_by_order(&cpu_posts, &cpu_klines, window);
    check(
        exclude_refused_landings(&mut cpu_rows, Count(0)) == Count(0),
        "a CPU-path run (no refusal line) must exclude nothing",
    );
    let csum = summarize(&cpu_rows, None);
    check(
        csum.completed == Count(3) && csum.excluded == Count(0) && csum.legs.get("gpu").is_none(),
        "CPU-path rows must complete with no gpu leg",
    );
    let (mut gpu_rows, _) = pair_by_order(&cpu_posts, &cpu_klines, window);
    check(
        exclude_refused_landings(&mut gpu_rows, Count(1)) == Count(3),
        "with a refusal line every committed landed=- row is a refused landing",
    );
    let gsum = summarize(&gpu_rows, None);
    check(
        gsum.completed == Count(0) && gsum.reasons.get("refused-landing") == Count(3),
        "refused-landing rows must not complete",
    );
    check(
        gpu_rows
            .iter()
            .all(|r| r.exclude_reason.as_deref() == Some("refused-landing")),
        "the refused-landing reason must be on every row",
    );
    let mixed = [
        rec(&record_line(
            Seq(1),
            Ns(8_000_000_000),
            &[
                ("commit", "-"),
                ("landed", "-"),
                ("shown", "-"),
                ("vsync", "-"),
                ("vsync_hi", "-"),
            ],
        )),
        rec(&record_line(Seq(2), Ns(8_001_000_000), &[("landed", "-")])),
    ];
    let mposts = [post(1, 8_000_000_000, "m"), post(3, 8_001_000_000, "m")];
    let (mut mrows, _) = pair_by_order(&mposts, &mixed, window);
    check(
        exclude_refused_landings(&mut mrows, Count(2)) == Count(0),
        "a never-committed row and an already-excluded row are not refused landings",
    );
    check(
        !mrows[0].excluded() && !is_completed(&mrows[0]),
        "a row with commit=- is incomplete, neither excluded nor completed",
    );
    check(
        mrows[1].exclude_reason.as_deref() == Some("seq-mismatch"),
        "an excluded row keeps its reason",
    );
    let refused = |k: Option<usize>| {
        Some(Diag {
            landing_refused: k.map(Count),
            ..Diag::default()
        })
    };
    let runs = refused_runs(&[
        ("base".to_string(), vec![refused(Some(0)), refused(Some(2))]),
        ("aa".to_string(), vec![refused(Some(0)), None]),
    ]);
    check(
        runs == [("base".to_string(), Round(2), Count(2))],
        "refused_runs must name the arm, the round and the count",
    );

    // The warm-up round: a discarded round 1 that completed nothing is
    // said; a round 1 that worked is not; a single round is not discarded.
    let (wrow, _) = pair_by_order(
        &[post(1, 9_000_000_000, "w")],
        &[rec(&record_line(Seq(1), Ns(9_000_000_000), &[]))],
        window,
    );
    check(
        warmup_measured_nothing(&[vec![], wrow.clone()]),
        "an empty warm-up round must be reported",
    );
    check(
        !warmup_measured_nothing(&[wrow.clone(), wrow.clone()]),
        "a warm-up round that completed keys must not be reported",
    );
    check(
        !warmup_measured_nothing(&[vec![]]),
        "a single round is used, not discarded; settled_rounds flags it",
    );

    // The gap bounds are gap -/+ a quarter (0 stays 0), and the pairing
    // window follows the MINIMUM jittered gap below 100 ms, floored at 1.
    check(
        gap_bounds(Ms(150)) == (112.5, 187.5) && gap_bounds(Ms(0)) == (0.0, 0.0),
        "gap_bounds",
    );
    for (gap, want) in [(150, 100), (134, 100), (100, 70), (50, 32), (6, 1), (0, 1)] {
        check(
            window_for_gap(Ms(gap)) == Ms(want),
            &format!("window_for_gap({gap}) != {want}"),
        );
    }
    let (wrow, _) = pair_by_order(
        &[post(1, 1_000_000_000, "a")],
        &[rec(&record_line(Seq(1), Ns(1_050_000_000), &[]))],
        Ns(45_000_000),
    );
    check(
        wrow[0].exclude_reason.as_deref() == Some("event-post>=window"),
        "a 50 ms stamp delta must be excluded under a 45 ms window",
    );

    // The arm order rotates: over three rounds each of three arms is first
    // once and `aa` is last only once.
    let arms = ["base", "new", "aa"];
    let orders = [1, 2, 3].map(|r| rotate(&arms, Round(r)));
    let mut firsts: Vec<&str> = orders.iter().map(|o| o[0]).collect();
    firsts.sort_unstable();
    check(
        firsts == ["aa", "base", "new"],
        "rotation must put every arm first once",
    );
    check(
        orders.iter().filter(|o| o[2] == "aa").count() == 1,
        "aa must not always be last",
    );

    // The A/A floor is robust: one 163 ms outlier in a leg of 10 us values
    // must not own it (max-min would be ~163 ms).
    let total = |s| PerLeg::only("total", s);
    let st = |v: &[f64]| leg_stats(v).expect("values");
    let st_b = st(&[10.0, 11.0, 10.0, 12.0, 10.0, 11.0, 163000.0]);
    let st_a = st(&[12.0, 10.0, 13.0, 11.0, 10.0, 12.0, 11.0]);
    let fl = *aa_floor(&total(st_b), &total(st_a)).get("total");
    check(
        fl.is_some_and(|f| f.floor <= 100.0 && f.floor >= f.dmed),
        "the A/A floor must be robust to one outlier",
    );
    check(
        st_b.max - st_b.min >= 100000.0,
        "the outlier control did not carry an outlier",
    );

    // Arm deltas against the floor: `result` needs |delta| above the floor
    // AND disjoint IQRs; a delta inside the floor is not one even with
    // disjoint IQRs, a delta above the floor is not one when the IQRs
    // overlap, and with no floor nothing is.
    let d_base = st(&[95.0, 98.0, 100.0, 102.0, 105.0]); // IQR [98-102]
    let d_res = st(&[190.0, 195.0, 200.0, 205.0, 210.0]); // +100, [195-205]
    let d_in = st(&[100.0, 103.0, 105.0, 107.0, 110.0]); // +5, [103-107]
    let d_ov = st(&[80.0, 90.0, 150.0, 160.0, 170.0]); // +50, [90-160]
    let fl10 = PerLeg::only(
        "total",
        Floor {
            dmed: 0.0,
            iqr_base: 0.0,
            iqr_aa: 0.0,
            floor: 10.0,
        },
    );
    let delta = |arm: &PerLeg<Option<_>>, floors| {
        arm_deltas(&total(d_base), arm, floors)
            .into_iter()
            .flatten()
            .next()
    };
    let dd = delta(&total(d_res), Some(&fl10)).expect("both arms have the leg");
    check(
        dd.verdict == "result" && approx(Some(dd.delta), 100.0) && dd.iqrs == Iqrs::Disjoint,
        "a delta above the floor with disjoint IQRs is a result",
    );
    let dd = delta(&total(d_in), Some(&fl10)).expect("both arms have the leg");
    check(
        dd.verdict.starts_with("within floor -- not a result") && dd.iqrs == Iqrs::Disjoint,
        "a delta inside the floor is not a result even with disjoint IQRs",
    );
    let dd = delta(&total(d_ov), Some(&fl10)).expect("both arms have the leg");
    check(
        dd.verdict == "IQRs overlap -- not a result" && dd.iqrs == Iqrs::Overlap,
        "overlapping IQRs are not a result whatever the delta",
    );
    let dd = delta(&total(d_res), None).expect("both arms have the leg");
    check(
        dd.verdict == "no A/A floor -- not a result",
        "without an A/A floor nothing is a result",
    );
    check(
        delta(&PerLeg::none(), Some(&fl10)).is_none(),
        "a leg one arm lacks has no delta",
    );

    // A single round is warm-up and must be reported as such.
    check(
        settled_rounds(&["x"]) == (&["x"][..], WarmUp::Used),
        "one round must be used and flagged as warm-up",
    );
    check(
        settled_rounds(&["x", "y", "z"]) == (&["y", "z"][..], WarmUp::Discarded),
        "round 1 must be discarded when there are more",
    );

    // The child modes, including the no-echo control.
    let env = |c| child_env(c, "/a b/exe");
    check(
        env(Child::None).as_deref() == Some("stty -echo; sleep 60"),
        "--child none must set the no-echo control",
    );
    check(
        env(Child::Cat).as_deref() == Some("cat") && env(Child::Shell).is_none(),
        "cat/shell child env wrong",
    );
    check(
        env(Child::Stream).is_some_and(|c| c.contains(STREAM_FLAG) && c.starts_with("'/a b/exe'")),
        "--child stream must run this binary's child, quoted",
    );
    let mute = env(Child::Mute).unwrap_or_default();
    check(
        mute.contains("133;A") && mute.contains("133;B") && mute.contains("stty -echo"),
        "--child mute must open a prompt and never echo",
    );

    // The helper's --front line.
    check(
        parse_front_line("front 1390 Safari app 1390 Safari")
            == Some(Front {
                window_pid: 1390,
                window_name: "Safari".to_string(),
                app_pid: 1390,
                app_name: "Safari".to_string(),
            }),
        "front line parse",
    );
}
