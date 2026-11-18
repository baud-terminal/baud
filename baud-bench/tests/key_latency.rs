//! `key-latency`'s pure half, one perturbation a branch: each case changes
//! one thing about a well-formed input and asserts what the parser, the
//! pairing or the statistic does about it. The numeric expectations are the
//! Python driver's answers, taken side by side before it was removed.

use baud_bench::key_latency::cli::{self, AaArm, Child, Cli};
use baud_bench::key_latency::parse::*;
use baud_bench::key_latency::py;
use baud_bench::key_latency::report::{aa_text, deltas_text, summary_text};
use baud_bench::key_latency::rows::*;
use baud_bench::key_latency::selftest::{default_window, record_line};
use baud_bench::key_latency::sha256;
use baud_bench::key_latency::stats::*;
use baud_bench::key_latency::sweep::{MUTE_CMD, STREAM_FLAG, child_env, tsv_text};

const LINE: &str = "keylat: seq=1 event=1000000000 handled=+300.0 written=+350.0 pred=- \
                    echo=+360.0 fed=+365.0 commit=+370.0 landed=+380.0 shown=+385.0 vsync=+400.0 \
                    vsync_hi=+416.7 predok=-";

fn err(line: &str) -> String {
    parse_keylat_line(line).expect_err("a parse failure").0
}

fn rec(line: &str) -> Record {
    parse_keylat_line(line)
        .expect("it parses")
        .expect("it is a record")
}

fn post(seq: i128, ns: i128) -> Post {
    Post {
        seq: Seq(seq),
        ns: Ns(ns),
        ch: "a".to_string(),
    }
}

fn one_row(p: Post, r: Record) -> Row {
    pair_by_order(&[p], &[r], default_window())
        .0
        .pop()
        .expect("a pair")
}

fn stats(v: &[f64]) -> LegStats {
    leg_stats(v).expect("values")
}

// -- the record parser --------------------------------------------------

#[test]
fn a_well_formed_record_makes_every_stage_absolute() {
    let r = rec(LINE);
    assert_eq!(r.seq, Seq(1));
    assert_eq!(r.event, Ns(1_000_000_000));
    assert_eq!(r.stamps.handled, Some(Ns(1_000_300_000)));
    assert_eq!(r.stamps.vsync_hi, Some(Ns(1_000_416_700)));
}

#[test]
fn a_missing_known_token_is_a_failure_naming_it() {
    let line = LINE.replace(" vsync_hi=+416.7", "");
    assert!(err(&line).starts_with("missing vsync_hi: keylat: seq=1 "));
}

#[test]
fn every_missing_token_is_named_in_emission_order() {
    let line = LINE
        .replace(" echo=+360.0", "")
        .replace(" event=1000000000", "");
    assert!(err(&line).starts_with("missing event,echo: "));
}

#[test]
fn a_bare_offset_is_a_failure() {
    let line = LINE.replace("handled=+300.0", "handled=300.0");
    assert!(err(&line).starts_with("handled=300.0 is neither +<us> nor -: "));
}

#[test]
fn every_malformed_offset_is_refused() {
    for bad in [
        "+", "+.5", "+5.", "++1", "+1e5", "+1_000.0", "+1.5.2", "x", "",
    ] {
        let line = LINE.replace("written=+350.0", &format!("written={bad}"));
        assert!(
            err(&line).starts_with(&format!("written={bad} is neither")),
            "{bad}"
        );
    }
}

#[test]
fn an_offset_past_any_stamp_is_a_failure_rather_than_a_panic() {
    let huge = format!("handled=+{}", "9".repeat(400));
    let line = LINE.replace("handled=+300.0", &huge);
    assert!(err(&line).contains("is out of range"));
}

#[test]
fn a_seq_or_event_that_is_no_integer_is_a_failure() {
    let line = LINE.replace("seq=1", "seq=x");
    assert!(err(&line).starts_with("seq/event not integers: "));
    let line = LINE.replace("event=1000000000", "event=1.0");
    assert!(err(&line).starts_with("seq/event not integers: "));
}

#[test]
fn seq_and_event_read_as_int_reads_them() {
    assert_eq!(rec(&LINE.replace("seq=1", "seq=+5")).seq, Seq(5));
    assert_eq!(rec(&LINE.replace("seq=1", "seq=-5")).seq, Seq(-5));
    assert_eq!(rec(&LINE.replace("seq=1", "seq=1_0")).seq, Seq(10));
    assert_eq!(rec(&LINE.replace("seq=1", "seq=007")).seq, Seq(7));
    for bad in ["_1", "1_", "1__0", "", "0x10"] {
        let line = LINE.replace("seq=1", &format!("seq={bad}"));
        assert!(err(&line).starts_with("seq/event not integers: "), "{bad}");
    }
}

#[test]
fn an_unknown_token_is_dropped() {
    let r = rec(&format!("{LINE} future=+1.0 note=x"));
    assert_eq!(r, rec(LINE));
}

#[test]
fn a_token_given_twice_is_the_last_one() {
    let r = rec(&format!("{LINE} handled=+1.0"));
    assert_eq!(r.stamps.handled, Some(Ns(1_000_001_000)));
}

#[test]
fn a_token_with_no_value_sign_ends_the_record_reading() {
    assert_eq!(parse_keylat_line("keylat: 3 keys open"), Ok(None));
    assert_eq!(
        parse_keylat_line(&format!("{LINE} noequals")),
        Ok(None),
        "a token with no = makes the whole line no record"
    );
}

#[test]
fn a_line_with_no_seq_is_no_record() {
    assert_eq!(parse_keylat_line("keylat: event=1 handled=+1.0"), Ok(None));
    assert_eq!(parse_keylat_line("startup: first present 1 ms"), Ok(None));
}

#[test]
fn the_first_equals_sign_cuts_a_token() {
    let line = LINE.replace("handled=+300.0", "handled=+1.0=2");
    assert!(err(&line).starts_with("handled=+1.0=2 is neither"));
}

#[test]
fn whitespace_is_what_str_split_splits_on() {
    let spaced = LINE.replace(' ', "\u{1c}");
    assert_eq!(rec(&format!("\u{3000}{spaced}\u{85}")), rec(LINE));
}

#[test]
fn a_tie_at_the_nanosecond_goes_to_even() {
    // 0.0005 us is not representable, so ties come from values that are:
    // 2^-1 ns is 0.0005 us only in decimal. Two offsets whose product lands
    // on .5 exactly hold the rule, which a plain round would break upward.
    let tie = |us: &str| {
        rec(&LINE.replace("handled=+300.0", &format!("handled=+{us}")))
            .stamps
            .handled
            .map(|n| n.0 - 1_000_000_000)
    };
    assert_eq!(tie("0.0025"), Some(2), "2.5 ns goes to 2, not 3");
    assert_eq!(tie("0.0035"), Some(4), "3.5 ns goes to 4");
    assert_eq!(tie("0.25"), Some(250));
}

// -- the lines of a run --------------------------------------------------

#[test]
fn a_run_s_lines_sort_into_their_classes() {
    let good = LINE.to_string();
    let bad = LINE.replace(" vsync_hi=+416.7", "");
    let lines = [
        "startup: first present 1 ms",
        good.as_str(),
        bad.as_str(),
        "keylat: landing refused ended=2 now=1",
        "  keylat: landing refused ended=4 now=3",
        "keylat: refused zone",
        "keylat: 3 keys open",
        "keylat: something else",
    ];
    let p = parse_keylat_lines(&lines);
    assert_eq!(p.records.len(), 1);
    assert_eq!(p.failures.len(), 1);
    assert_eq!(p.refusals, Count(2));
    assert_eq!(p.reports, ["keylat: refused zone", "keylat: 3 keys open"]);
}

#[test]
fn a_post_line_reads_three_fields_or_none() {
    let p = parse_post_line("post 1 2 a").expect("a post");
    assert_eq!((p.seq, p.ns, p.ch.as_str()), (Seq(1), Ns(2), "a"));
    assert!(parse_post_line("  post  3  4  z\u{1c}").is_some());
    for bad in [
        "post 1 2",
        "post 1 2 a b",
        "post x 2 a",
        "post 1 -2 a",
        "post -1 2 a",
        "post 1_0 2 a",
        "Post 1 2 a",
        "post1 2 a",
    ] {
        assert_eq!(parse_post_line(bad), None, "{bad}");
    }
}

#[test]
fn a_front_line_keeps_names_with_spaces() {
    let f = parse_front_line("front 1 a b app 2 c d").expect("a front line");
    assert_eq!((f.window_pid, f.window_name.as_str()), (1, "a b"));
    assert_eq!((f.app_pid, f.app_name.as_str()), (2, "c d"));
}

#[test]
fn a_front_line_s_window_name_ends_at_the_first_app_pid() {
    let f = parse_front_line("front 1 a app 2 b app 3 c").expect("a front line");
    assert_eq!(
        (f.window_name.as_str(), f.app_pid, f.app_name.as_str()),
        ("a", 2, "b app 3 c")
    );
    let f = parse_front_line("front 1 a  app 2x b app 3 c").expect("a front line");
    assert_eq!((f.window_name.as_str(), f.app_pid), ("a  app 2x b", 3));
}

#[test]
fn a_front_line_s_empty_name_needs_two_spaces() {
    let f = parse_front_line("front 1  app 2 x").expect("a front line");
    assert_eq!(f.window_name, "");
    assert_eq!(parse_front_line("front 1 app 2 x"), None);
    let f = parse_front_line("front 1   app 2 x").expect("a front line");
    assert_eq!(f.window_name, "");
}

#[test]
fn a_front_line_takes_negative_pids_and_refuses_the_rest() {
    let f = parse_front_line("front -1 x app -2 y").expect("a front line");
    assert_eq!((f.window_pid, f.app_pid), (-1, -2));
    for bad in [
        "front x a app 2 b",
        "front 1 a app x b",
        "front 1 a app 2",
        "frontx 1 a app 2 b",
        "front 1 x app - 2 b",
    ] {
        assert_eq!(parse_front_line(bad), None, "{bad}");
    }
}

// -- pairing -------------------------------------------------------------

#[test]
fn a_clean_pair_is_kept() {
    let (rows, d) = pair_by_order(&[post(1, 1_000_000_000)], &[rec(LINE)], default_window());
    assert!(!rows[0].excluded());
    assert_eq!(
        (d.paired, d.posted, d.keylats),
        (Count(1), Count(1), Count(1))
    );
    assert_eq!((d.unpaired_posts, d.unpaired_keylats), (Count(0), Count(0)));
}

#[test]
fn seq_mismatch_excludes_first_of_all() {
    // event < post too: the seq is what the reason names.
    let (rows, d) = pair_by_order(&[post(2, 2_000_000_000)], &[rec(LINE)], default_window());
    assert_eq!(rows[0].exclude_reason.as_deref(), Some("seq-mismatch"));
    assert_eq!((d.seq_mismatch, d.event_before_post), (Count(1), Count(0)));
}

#[test]
fn an_event_before_its_post_is_excluded() {
    let (rows, d) = pair_by_order(&[post(1, 1_000_000_001)], &[rec(LINE)], default_window());
    assert_eq!(rows[0].exclude_reason.as_deref(), Some("event<post"));
    assert_eq!(d.event_before_post, Count(1));
}

#[test]
fn an_event_at_the_post_is_kept() {
    assert!(!one_row(post(1, 1_000_000_000), rec(LINE)).excluded());
}

#[test]
fn the_window_excludes_at_its_edge_and_not_below() {
    let at = |p: i128, w: Ns| pair_by_order(&[post(1, p)], &[rec(LINE)], w).0;
    let edge = at(900_000_000, Ns(100_000_000));
    assert_eq!(
        edge[0].exclude_reason.as_deref(),
        Some("event-post>=window")
    );
    let inside = at(900_000_001, Ns(100_000_000));
    assert!(!inside[0].excluded());
    let (_, d) = pair_by_order(&[post(1, 900_000_000)], &[rec(LINE)], Ns(100_000_000));
    assert_eq!(d.window, Count(1));
}

#[test]
fn a_negative_leg_excludes_and_names_every_one() {
    let r = rec(&LINE.replace("landed=+380.0", "landed=+390.0"));
    let (rows, d) = pair_by_order(&[post(1, 1_000_000_000)], &[r], default_window());
    assert_eq!(rows[0].exclude_reason.as_deref(), Some("negative show"));
    assert_eq!(d.negative, Count(1));
    let r = rec(&LINE
        .replace("written=+350.0", "written=+370.0")
        .replace("fed=+365.0", "fed=+359.0"));
    assert_eq!(
        one_row(post(1, 1_000_000_000), r).exclude_reason.as_deref(),
        Some("negative child,wake")
    );
}

#[test]
fn a_negative_stamp_leg_is_the_event_before_post_rule_not_a_negative() {
    let (rows, d) = pair_by_order(&[post(1, 1_000_000_500)], &[rec(LINE)], default_window());
    assert_eq!(d.negative, Count(0));
    assert!(
        *legs_of(&Row {
            exclude_reason: None,
            ..rows[0].clone()
        })
        .get("stamp")
            == Some(-0.5)
    );
    assert!(
        negative_legs(&rows[0]).is_empty(),
        "stamp is not one of them"
    );
}

#[test]
fn a_commit_before_its_echo_is_fed_is_a_negative_build() {
    let line = LINE
        .replace("commit=+370.0", "commit=+355.0")
        .replace("landed=+380.0", "landed=+365.0")
        .replace("shown=+385.0", "shown=+370.0");
    let row = one_row(post(1, 1_000_000_000), rec(&line));
    assert_eq!(row.exclude_reason.as_deref(), Some("negative build"));
}

#[test]
fn unpaired_posts_and_records_are_counted_apart() {
    let (rows, d) = pair_by_order(
        &[post(1, 1_000_000_000), post(2, 1_000_000_000)],
        &[rec(LINE)],
        default_window(),
    );
    assert_eq!(rows.len(), 1);
    assert_eq!((d.unpaired_posts, d.unpaired_keylats), (Count(1), Count(0)));
    let (_, d) = pair_by_order(&[], &[rec(LINE)], default_window());
    assert_eq!((d.unpaired_posts, d.unpaired_keylats), (Count(0), Count(1)));
}

#[test]
fn every_leg_runs_between_its_own_two_stages() {
    let row = one_row(post(1, 999_999_000), rec(LINE));
    let legs = legs_of(&row);
    for (leg, want) in [
        ("stamp", 1.0),
        ("deliver", 301.0),
        ("write", 50.0),
        ("child", 10.0),
        ("wake", 5.0),
        ("build", 5.0),
        ("gpu", 10.0),
        ("show", 5.0),
        ("app", 85.0),
        ("vsync", 15.0),
        ("vsync_hi", 31.7),
        ("to_shown", 386.0),
        ("total", 401.0),
        ("total_hi", 417.7),
    ] {
        assert_eq!(*legs.get(leg), Some(want), "{leg}");
    }
}

#[test]
fn an_excluded_row_has_no_legs() {
    let row = one_row(post(2, 999_999_000), rec(LINE));
    assert!(row.excluded());
    assert!(legs_of(&row).0.iter().all(Option::is_none));
}

#[test]
fn completion_needs_vsync_and_no_exclusion() {
    let row = one_row(post(1, 1_000_000_000), rec(LINE));
    assert!(is_completed(&row));
    let no_vsync = rec(&LINE.replace("vsync=+400.0", "vsync=-"));
    assert!(!is_completed(&one_row(post(1, 1_000_000_000), no_vsync)));
    assert!(!is_completed(&one_row(post(2, 1_000_000_000), rec(LINE))));
}

// -- refused landings ----------------------------------------------------

fn refused_case() -> Vec<Row> {
    let landed_gone = rec(&LINE.replace("landed=+380.0", "landed=-"));
    let no_commit = rec(&LINE
        .replace("commit=+370.0", "commit=-")
        .replace("landed=+380.0", "landed=-"));
    let excluded = rec(&LINE
        .replace("seq=1", "seq=9")
        .replace("landed=+380.0", "landed=-"));
    let keylats = [landed_gone, no_commit, excluded, rec(LINE)];
    let posts = [
        post(1, 1_000_000_000),
        post(1, 1_000_000_000),
        post(1, 1_000_000_000),
        post(1, 1_000_000_000),
    ];
    pair_by_order(&posts, &keylats, default_window()).0
}

#[test]
fn with_no_refusal_line_nothing_is_a_refused_landing() {
    let mut rows = refused_case();
    assert_eq!(exclude_refused_landings(&mut rows, Count(0)), Count(0));
    assert!(!rows[0].excluded());
}

#[test]
fn a_refusal_line_excludes_only_committed_rows_with_no_landing() {
    let mut rows = refused_case();
    assert_eq!(exclude_refused_landings(&mut rows, Count(1)), Count(1));
    assert_eq!(rows[0].exclude_reason.as_deref(), Some("refused-landing"));
    assert!(!rows[1].excluded(), "commit=- is incomplete, not refused");
    assert_eq!(rows[2].exclude_reason.as_deref(), Some("seq-mismatch"));
    assert!(!rows[3].excluded(), "it landed");
}

// -- shared frames -------------------------------------------------------

fn commit_row(event: i128, commit_us: &str) -> Row {
    let line = record_line(
        Seq(1),
        Ns(event),
        &[
            ("handled", "+0.0"),
            ("written", "+0.0"),
            ("echo", "+0.0"),
            ("fed", "+0.0"),
            ("commit", commit_us),
            ("landed", "-"),
            ("shown", "-"),
            ("vsync", "-"),
            ("vsync_hi", "-"),
        ],
    );
    one_row(post(1, event), rec(&line))
}

#[test]
fn commits_within_the_tolerance_share_a_frame() {
    let rows = [
        commit_row(1_000_000, "+10.0"),
        commit_row(1_000_100, "+10.0"),
    ];
    assert_eq!(shared_frame_count(&rows, SHARED_FRAME), Count(2));
    assert_eq!(shared_frame_count(&rows, Ns(99)), Count(0));
}

#[test]
fn the_tolerance_is_inclusive() {
    let rows = [
        commit_row(1_000_000, "+10.0"),
        commit_row(1_000_200, "+10.0"),
    ];
    assert_eq!(shared_frame_count(&rows, SHARED_FRAME), Count(2));
}

#[test]
fn a_group_runs_from_its_first_commit() {
    let rows = [
        commit_row(1_000_000, "+10.0"),
        commit_row(1_000_150, "+10.0"),
        commit_row(1_000_300, "+10.0"),
    ];
    assert_eq!(shared_frame_count(&rows, SHARED_FRAME), Count(2));
    let chained = [
        commit_row(1_000_000, "+10.0"),
        commit_row(1_000_150, "+10.0"),
        commit_row(1_000_300, "+10.0"),
        commit_row(1_000_450, "+10.0"),
    ];
    assert_eq!(shared_frame_count(&chained, SHARED_FRAME), Count(4));
}

#[test]
fn an_excluded_row_shares_nothing() {
    let mut rows = vec![
        commit_row(1_000_000, "+10.0"),
        commit_row(1_000_100, "+10.0"),
    ];
    rows[1].exclude_reason = Some("seq-mismatch".to_string());
    assert_eq!(shared_frame_count(&rows, SHARED_FRAME), Count(0));
}

// -- the statistics ------------------------------------------------------

#[test]
fn no_values_have_no_statistics() {
    assert_eq!(leg_stats(&[]), None);
}

#[test]
fn one_value_is_every_statistic() {
    let s = stats(&[7.5]);
    assert_eq!(
        (s.median, s.q1, s.q3, s.min, s.max),
        (7.5, 7.5, 7.5, 7.5, 7.5)
    );
    assert_eq!(s.n, Count(1));
}

#[test]
fn an_odd_median_is_the_middle_value() {
    assert_eq!(stats(&[3.0, 1.0, 2.0]).median, 2.0);
}

#[test]
fn an_even_median_is_the_mean_of_the_middle_two() {
    assert_eq!(stats(&[4.0, 1.0, 2.0, 3.0]).median, 2.5);
}

#[test]
fn quartiles_interpolate_inclusively_at_each_remainder() {
    // statistics.quantiles(n=4, method="inclusive"), one case for each value
    // of (n - 1) mod 4.
    let q = |v: &[f64]| (stats(v).q1, stats(v).q3);
    assert_eq!(q(&[1.0, 2.0]), (1.25, 1.75));
    assert_eq!(q(&[1.0, 2.0, 3.0]), (1.5, 2.5));
    assert_eq!(q(&[1.0, 2.0, 3.0, 4.0]), (1.75, 3.25));
    assert_eq!(q(&[1.0, 2.0, 3.0, 4.0, 5.0]), (2.0, 4.0));
}

#[test]
fn quartiles_keep_the_formula_s_order_of_operations() {
    // Python's answers for values whose rounding shows a reordered formula.
    let s = stats(&[0.1, 0.2, 0.3, 0.4]);
    assert_eq!(s.q1, 0.17500000000000002);
    assert_eq!(s.q3, 0.32499999999999996);
}

#[test]
fn min_and_max_ignore_the_input_s_order() {
    let s = stats(&[5.0, -2.0, 9.0, 0.0]);
    assert_eq!((s.min, s.max), (-2.0, 9.0));
}

#[test]
fn a_summary_counts_each_class_of_row() {
    let keep = one_row(post(1, 1_000_000_000), rec(LINE));
    let gone = one_row(post(2, 1_000_000_000), rec(LINE));
    let open = one_row(
        post(1, 1_000_000_000),
        rec(&LINE.replace("vsync=+400.0", "vsync=-")),
    );
    let s = summarize(&[keep, gone, open], None);
    assert_eq!((s.posted, s.paired), (Count(3), Count(3)));
    assert_eq!((s.completed, s.excluded), (Count(1), Count(1)));
    assert!(s.reasons.equals(&[("seq-mismatch", Count(1))]));
    assert_eq!(s.legs.get("write").map(|l| l.n), Some(Count(2)));
    assert_eq!(s.legs.get("vsync").map(|l| l.n), Some(Count(1)));
}

#[test]
fn a_summary_takes_posted_from_the_diag() {
    let keep = one_row(post(1, 1_000_000_000), rec(LINE));
    let d = Diag {
        posted: Count(9),
        ..Diag::default()
    };
    assert_eq!(summarize(&[keep], Some(&d)).posted, Count(9));
}

#[test]
fn every_negative_reason_pools_as_one() {
    let a = rec(&LINE.replace("landed=+380.0", "landed=+390.0"));
    let b = rec(&LINE.replace("fed=+365.0", "fed=+359.0"));
    let (rows, _) = pair_by_order(
        &[post(1, 1_000_000_000), post(1, 1_000_000_000)],
        &[a, b],
        default_window(),
    );
    assert!(
        summarize(&rows, None)
            .reasons
            .equals(&[("negative", Count(2))])
    );
}

#[test]
fn diags_sum_and_the_predictor_s_sum_by_gate() {
    let one = |gate: &str, n| {
        let mut d = Diag {
            posted: Count(n),
            landing_refused: Some(Count(n)),
            ..Diag::default()
        };
        d.predict_refused.add(gate, Count(n));
        Some(d)
    };
    let m = merge_diags(&[one("zone", 2), None, one("zone", 3), one("hold", 1)]).expect("a sum");
    assert_eq!(m.posted, Count(6));
    assert_eq!(m.landing_refused, Some(Count(6)));
    assert!(
        m.predict_refused
            .equals(&[("zone", Count(5)), ("hold", Count(1))])
    );
}

#[test]
fn no_diag_at_all_merges_to_none() {
    assert_eq!(merge_diags(&[None, None]), None);
    assert_eq!(merge_diags(&[]), None);
}

// -- the floor and the verdicts ------------------------------------------

fn only(s: LegStats) -> PerLeg<Option<LegStats>> {
    PerLeg::only("total", s)
}

#[test]
fn the_floor_is_the_median_gap_when_that_is_largest() {
    let f = aa_floor(&only(stats(&[1.0, 1.0])), &only(stats(&[9.0, 9.0])));
    assert_eq!(f.get("total").map(|f| f.floor), Some(8.0));
}

#[test]
fn the_floor_is_the_base_iqr_when_that_is_largest() {
    let f = aa_floor(&only(stats(&[0.0, 8.0])), &only(stats(&[4.0, 4.0])));
    assert_eq!(
        f.get("total").map(|f| (f.floor, f.iqr_base)),
        Some((4.0, 4.0))
    );
}

#[test]
fn the_floor_is_the_aa_iqr_when_that_is_largest() {
    let f = aa_floor(&only(stats(&[4.0, 4.0])), &only(stats(&[0.0, 8.0])));
    assert_eq!(
        f.get("total").map(|f| (f.floor, f.iqr_aa)),
        Some((4.0, 4.0))
    );
}

#[test]
fn one_outlier_does_not_own_the_floor() {
    let base = stats(&[10.0, 11.0, 10.0, 12.0, 10.0, 11.0, 163000.0]);
    let aa = stats(&[12.0, 10.0, 13.0, 11.0, 10.0, 12.0, 11.0]);
    let f = aa_floor(&only(base), &only(aa));
    assert!(f.get("total").is_some_and(|f| f.floor < 100.0));
}

#[test]
fn a_leg_either_arm_lacks_has_no_floor() {
    let none = PerLeg::none();
    assert_eq!(*aa_floor(&only(stats(&[1.0])), &none).get("total"), None);
    assert_eq!(*aa_floor(&none, &only(stats(&[1.0]))).get("total"), None);
}

#[test]
fn iqrs_are_disjoint_either_way_round_and_touching_overlaps() {
    let low = stats(&[1.0, 1.0]);
    let high = stats(&[2.0, 2.0]);
    assert_eq!(iqr_disjoint(&low, &high), Iqrs::Disjoint);
    assert_eq!(iqr_disjoint(&high, &low), Iqrs::Disjoint);
    assert_eq!(iqr_disjoint(&low, &low), Iqrs::Overlap);
}

fn floor_of(x: f64) -> PerLeg<Option<Floor>> {
    PerLeg::only(
        "total",
        Floor {
            dmed: 0.0,
            iqr_base: 0.0,
            iqr_aa: 0.0,
            floor: x,
        },
    )
}

fn verdict(arm: &[f64], floors: Option<&PerLeg<Option<Floor>>>) -> Delta {
    let base = only(stats(&[95.0, 98.0, 100.0, 102.0, 105.0]));
    arm_deltas(&base, &only(stats(arm)), floors)
        .into_iter()
        .flatten()
        .next()
        .expect("both arms have the leg")
}

#[test]
fn above_the_floor_and_disjoint_is_a_result() {
    let d = verdict(&[190.0, 195.0, 200.0, 205.0, 210.0], Some(&floor_of(10.0)));
    assert_eq!((d.verdict.as_str(), d.delta), ("result", 100.0));
    assert_eq!(d.iqrs, Iqrs::Disjoint);
}

#[test]
fn inside_the_floor_is_no_result_however_disjoint() {
    let d = verdict(&[100.0, 103.0, 105.0, 107.0, 110.0], Some(&floor_of(10.0)));
    assert_eq!(
        d.verdict,
        "within floor -- not a result (|delta| 5.0 <= floor 10.0)"
    );
    assert_eq!(d.iqrs, Iqrs::Disjoint);
}

#[test]
fn exactly_at_the_floor_is_within_it() {
    let d = verdict(&[190.0, 195.0, 200.0, 205.0, 210.0], Some(&floor_of(100.0)));
    assert!(d.verdict.starts_with("within floor"));
}

#[test]
fn above_the_floor_with_overlapping_iqrs_is_no_result() {
    let d = verdict(&[80.0, 90.0, 150.0, 160.0, 170.0], Some(&floor_of(10.0)));
    assert_eq!(d.verdict, "IQRs overlap -- not a result");
    assert_eq!(d.iqrs, Iqrs::Overlap);
}

#[test]
fn with_no_floor_nothing_is_a_result() {
    let d = verdict(&[190.0, 195.0, 200.0, 205.0, 210.0], None);
    assert_eq!(d.verdict, "no A/A floor -- not a result");
    let none = PerLeg::none();
    let d = verdict(&[190.0, 195.0, 200.0, 205.0, 210.0], Some(&none));
    assert_eq!(
        d.verdict, "no A/A floor -- not a result",
        "the leg has none"
    );
}

#[test]
fn a_leg_one_arm_lacks_has_no_delta() {
    let base = only(stats(&[1.0]));
    assert!(
        arm_deltas(&base, &PerLeg::none(), None)
            .iter()
            .all(Option::is_none)
    );
}

// -- rounds, gaps, children ----------------------------------------------

#[test]
fn the_arm_order_rotates_left_a_round_at_a_time() {
    let arms = ["a", "b", "c"];
    assert_eq!(rotate(&arms, Round(1)), ["a", "b", "c"]);
    assert_eq!(rotate(&arms, Round(2)), ["b", "c", "a"]);
    assert_eq!(rotate(&arms, Round(4)), ["a", "b", "c"]);
    assert!(rotate::<&str>(&[], Round(3)).is_empty());
}

#[test]
fn round_one_is_discarded_only_when_there_are_more() {
    assert_eq!(settled_rounds(&[1]), (&[1][..], WarmUp::Used));
    assert_eq!(settled_rounds(&[1, 2, 3]), (&[2, 3][..], WarmUp::Discarded));
}

#[test]
fn an_empty_warm_up_is_reported_and_a_lone_round_is_not() {
    let done = vec![one_row(post(1, 1_000_000_000), rec(LINE))];
    assert!(warmup_measured_nothing(&[vec![], done.clone()]));
    assert!(!warmup_measured_nothing(&[done.clone(), vec![]]));
    assert!(!warmup_measured_nothing(&[vec![]]));
}

#[test]
fn refused_runs_name_arm_round_and_count() {
    let d = |k: Option<usize>| {
        Some(Diag {
            landing_refused: k.map(Count),
            ..Diag::default()
        })
    };
    let runs = refused_runs(&[
        ("base".to_string(), vec![d(Some(0)), d(Some(2))]),
        ("aa".to_string(), vec![None, d(None), d(Some(1))]),
    ]);
    assert_eq!(
        runs,
        [
            ("base".to_string(), Round(2), Count(2)),
            ("aa".to_string(), Round(3), Count(1))
        ]
    );
}

#[test]
fn the_gap_bounds_are_a_quarter_either_side() {
    assert_eq!(gap_bounds(Ms(150)), (112.5, 187.5));
    assert_eq!(gap_bounds(Ms(0)), (0.0, 0.0));
}

#[test]
fn the_window_follows_the_minimum_gap() {
    for (gap, want) in [
        (1000, 100),
        (150, 100),
        (134, 100),
        (133, 94),
        (100, 70),
        (50, 32),
        (7, 1),
        (6, 1),
        (0, 1),
    ] {
        assert_eq!(window_for_gap(Ms(gap)), Ms(want), "gap {gap}");
    }
    assert_eq!(window_ns(Ms(100)), Ns(100_000_000));
}

#[test]
fn the_children_set_what_they_set() {
    let env = |c| child_env(c, "/a b/it's");
    assert_eq!(env(Child::Cat).as_deref(), Some("cat"));
    assert_eq!(env(Child::Shell), None);
    assert_eq!(env(Child::None).as_deref(), Some("stty -echo; sleep 60"));
    assert_eq!(env(Child::Mute).as_deref(), Some(MUTE_CMD));
    assert_eq!(
        env(Child::Stream),
        Some(format!(r"'/a b/it'\''s' {STREAM_FLAG}"))
    );
}

// -- the report ----------------------------------------------------------

fn summary_with(legs: PerLeg<Option<LegStats>>) -> Summary {
    Summary {
        posted: Count(0),
        paired: Count(0),
        completed: Count(0),
        excluded: Count(0),
        reasons: Tally::default(),
        shared: Count(0),
        legs,
    }
}

#[test]
fn the_ab_table_signs_every_delta() {
    let at = |x| summary_with(only(stats(&[x])));
    let row = |base: f64, arm: f64| {
        let text = deltas_text("b", "n", &at(base), &at(arm), None);
        let line = text
            .lines()
            .find(|l| l.starts_with("    total "))
            .expect("the leg's row")
            .to_string();
        line
    };
    assert!(
        row(10.0, 15.0).contains("      +5.0"),
        "{}",
        row(10.0, 15.0)
    );
    assert!(
        row(15.0, 10.0).contains("      -5.0"),
        "{}",
        row(15.0, 10.0)
    );
    assert!(
        row(10.0, 10.0).contains("      +0.0"),
        "{}",
        row(10.0, 10.0)
    );
}

#[test]
fn a_one_decimal_tie_prints_what_the_binary_value_rounds_to() {
    // As `f"{x:.1f}"` does: exact ties to even, the rest by their true value.
    let s = stats(&[0.25]);
    let row = |legs| {
        summary_with(PerLeg(std::array::from_fn(|i| {
            if i == 0 { legs } else { None }
        })))
    };
    let text = summary_text("x", &row(Some(s)), None);
    assert!(text.contains("      0.2 (  1)  [0.2-0.2]"), "{text}");
    let text = summary_text("x", &row(Some(stats(&[0.35]))), None);
    assert!(text.contains("      0.3 (  1)  [0.3-0.3]"), "{text}");
    let text = summary_text("x", &row(Some(stats(&[0.75]))), None);
    assert!(text.contains("      0.8 (  1)  [0.8-0.8]"), "{text}");
    let text = summary_text("x", &row(Some(stats(&[-0.04]))), None);
    assert!(text.contains("     -0.0 (  1)  [-0.0--0.0]"), "{text}");
}

fn summary_of(rows: &[Row], d: Option<&Diag>) -> String {
    summary_text("arm", &summarize(rows, d), d)
}

#[test]
fn a_diag_adds_the_unpaired_line_and_a_refusal_the_warning() {
    let row = one_row(post(1, 1_000_000_000), rec(LINE));
    assert!(!summary_of(std::slice::from_ref(&row), None).contains("unpaired posts="));
    let mut d = Diag::default();
    let clean = summary_of(std::slice::from_ref(&row), Some(&d));
    assert!(
        clean.contains("unpaired posts=0 unpaired keylats=0 parse failures=0 landings refused=0\n")
    );
    assert!(!clean.contains("NOT QUOTABLE"));
    d.landing_refused = Some(Count(2));
    let warned = summary_of(std::slice::from_ref(&row), Some(&d));
    assert!(warned.contains("NOT QUOTABLE -- 2 landing(s) refused"));
}

#[test]
fn the_predictor_s_lines_sort_by_gate() {
    let row = one_row(post(1, 1_000_000_000), rec(LINE));
    let mut d = Diag::default();
    d.predict_refused.add("zone", Count(1));
    d.predict_refused.add("hold", Count(2));
    let text = summary_of(&[row], Some(&d));
    assert!(text.contains("predict refused: hold=2, zone=1\n"));
}

#[test]
fn an_absent_leg_prints_dashes_in_every_table() {
    let cpu = rec(&LINE.replace("landed=+380.0", "landed=-"));
    let row = one_row(post(1, 1_000_000_000), cpu);
    let s = summarize(&[row], None);
    let text = summary_text("arm", &s, None);
    assert!(
        text.contains("      gpu          -        -                        -\n"),
        "{text}"
    );
    let aa = aa_text("base", &s, &s);
    assert!(
        aa.contains(
            "      gpu           -           -         -          -          -          -\n"
        ),
        "{aa}"
    );
    let ab = deltas_text("base", "new", &s, &s, None);
    assert!(ab.contains("      gpu           -           -          -          -                 -                 -  -\n"), "{ab}");
}

#[test]
fn the_ab_table_says_when_there_is_no_floor() {
    let row = one_row(post(1, 1_000_000_000), rec(LINE));
    let s = summarize(&[row], None);
    assert!(deltas_text("b", "n", &s, &s, None).contains("no A/A floor (run with --aa)"));
    let f = aa_floor(&s.legs, &s.legs);
    assert!(!deltas_text("b", "n", &s, &s, Some(&f)).contains("no A/A floor (run with --aa)"));
}

#[test]
fn a_tsv_says_true_false_and_none_as_python_did() {
    let keep = one_row(post(1, 1_000_000_000), rec(LINE));
    let gone = one_row(post(2, 1_000_000_000), rec(LINE));
    let text = tsv_text(&[keep, gone]);
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some(
            "seq\tapp_seq\tchar\tpost\tevent\texcluded\texclude_reason\thandled\twritten\t\
             echo\tfed\tcommit\tlanded\tshown\tvsync\tvsync_hi"
        )
    );
    assert!(lines.next().is_some_and(|l| l.starts_with(
        "1\t1\ta\t1000000000\t1000000000\tFalse\tNone\t1000300000\t1000350000\t1000360000\t"
    )));
    assert!(lines
        .next()
        .is_some_and(|l| l.starts_with("2\t1\ta\t1000000000\t1000000000\tTrue\tseq-mismatch\t")));
}

// -- what Python wrote -----------------------------------------------------

#[test]
fn a_counter_prints_by_count_and_a_dict_as_it_came() {
    let mut t = Tally::default();
    assert_eq!(t.repr_counter(), "Counter()");
    assert_eq!(t.repr_dict(), "{}");
    t.add("a", Count(1));
    t.add("b", Count(2));
    t.add("c", Count(1));
    assert_eq!(t.repr_counter(), "Counter({'b': 2, 'a': 1, 'c': 1})");
    assert_eq!(t.repr_dict(), "{'a': 1, 'b': 2, 'c': 1}");
}

#[test]
fn repr_chooses_its_quote_and_escapes() {
    for (s, want) in [
        ("a", "'a'"),
        ("it's", "\"it's\""),
        ("say \"x\"", "'say \"x\"'"),
        ("both ' and \"", r#"'both \' and "'"#),
        ("back\\slash", r"'back\\slash'"),
        ("tab\there", r"'tab\there'"),
        ("\u{e9}", "'\u{e9}'"),
        ("\u{7f}", r"'\x7f'"),
        ("\u{85}", r"'\x85'"),
        ("\u{200b}", r"'\u200b'"),
        ("\u{1f600}", "'\u{1f600}'"),
        ("\u{e000}", r"'\ue000'"),
    ] {
        assert_eq!(py::repr_str(s), want);
    }
    assert_eq!(
        py::repr_list(&["a".to_string(), "b".to_string()]),
        "['a', 'b']"
    );
    assert_eq!(py::repr_list(&[]), "[]");
}

#[test]
fn a_float_prints_as_repr_does() {
    for (x, want) in [
        (0.0, "0.0"),
        (51.7, "51.7"),
        (1e16, "1e+16"),
        (1.5e300, "1.5e+300"),
        (1e-5, "1e-05"),
        (0.0001, "0.0001"),
    ] {
        assert_eq!(py::repr_float(x), want);
    }
}

#[test]
fn g_formats_as_python_does() {
    for (x, want) in [
        (0.0, "0"),
        (-0.0, "-0"),
        (112.5, "112.5"),
        (999999.0, "999999"),
        (999999.5, "1e+06"),
        (1234567.0, "1.23457e+06"),
        (1e-5, "1e-05"),
        (0.0001, "0.0001"),
        (100000.5, "100000"),
    ] {
        assert_eq!(py::format_g(x), want, "{x}");
    }
}

#[test]
fn int_reads_as_python_does() {
    for (s, want) in [
        ("5", Some(5)),
        (" 5 ", Some(5)),
        ("+5", Some(5)),
        ("-5", Some(-5)),
        ("007", Some(7)),
        ("1_000", Some(1000)),
        ("", None),
        ("+", None),
        ("_1", None),
        ("1_", None),
        ("1__0", None),
        ("1.0", None),
        ("0x10", None),
        ("+-1", None),
    ] {
        assert_eq!(py::int(s), want, "{s:?}");
    }
}

#[test]
fn lines_end_where_each_reader_ends_them() {
    assert_eq!(py::universal_lines("a\nb\r\nc\rd"), ["a", "b", "c", "d"]);
    assert_eq!(py::universal_lines("a\u{b}b\n\n"), ["a\u{b}b", ""]);
    assert_eq!(
        py::splitlines("a\u{b}b\u{85}c\u{2028}d\n"),
        ["a", "b", "c", "d"]
    );
    assert!(py::splitlines("").is_empty());
}

#[test]
fn abspath_normalises_without_the_disk() {
    assert_eq!(py::abspath("/a//b/./c/../d/"), "/a/b/d");
    assert_eq!(py::abspath("/.."), "/");
}

#[test]
fn sha256_holds_to_the_fips_vectors() {
    assert_eq!(
        sha256::digest(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256::digest(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256::digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

// -- the command line ------------------------------------------------------

fn parse(args: &[&str]) -> Cli {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    cli::parse(&args)
}

fn sweep_args(args: &[&str]) -> cli::Args {
    match parse(args) {
        Cli::Sweep(a) => a,
        other => panic!("{args:?}: {other:?}"),
    }
}

#[test]
fn the_defaults_are_the_driver_s() {
    let a = sweep_args(&[]);
    assert_eq!((a.rounds, a.keys, a.gap), (5, 30, Ms(150)));
    assert_eq!((a.child, a.aa), (Child::Cat, AaArm::Skip));
    assert_eq!((a.grid.as_str(), a.chars.as_str()), ("120x40", "asdfjkl"));
    assert!(a.arm.is_empty() && a.scratch.is_none());
}

#[test]
fn a_value_follows_the_option_or_its_equals_sign() {
    assert_eq!(sweep_args(&["--rounds", "3"]).rounds, 3);
    assert_eq!(sweep_args(&["--rounds=3"]).rounds, 3);
    assert_eq!(sweep_args(&["--gap", "-1"]).gap, Ms(-1));
    assert_eq!(sweep_args(&["--gap=-1"]).gap, Ms(-1));
    assert_eq!(sweep_args(&["--arm", "-a b"]).arm, ["-a b"]);
    assert_eq!(
        sweep_args(&["--arm=a=b", "--arm", "c=d"]).arm,
        ["a=b", "c=d"]
    );
}

#[test]
fn a_unique_prefix_names_an_option_and_a_shared_one_is_an_error() {
    assert_eq!(sweep_args(&["--ro", "3"]).rounds, 3);
    assert_eq!(sweep_args(&["--r=3"]).rounds, 3);
    assert_eq!(sweep_args(&["--aa"]).aa, AaArm::Run);
    assert!(matches!(parse(&["--a"]), Cli::Usage(_)));
    assert_eq!(parse(&["--sel"]), Cli::Selftest);
}

#[test]
fn every_usage_error_is_one() {
    for args in [
        &["--bogus"][..],
        &["--rounds", "x"],
        &["--rounds", "-1.5"],
        &["--child", "bogus"],
        &["--arm"],
        &["pos"],
        &["--", "pos"],
        &["--"],
        &["--selftest", "--"],
        &["--selftest=1"],
        &["--aa=1"],
        &["-x"],
        &["-"],
        &["--gap", "--", "-1"],
        &["--gap", "--keys"],
        &["--selftest", "--bogus"],
    ] {
        assert!(matches!(parse(args), Cli::Usage(_)), "{args:?}");
    }
}

/// `-h` takes no value: argparse runs it from `-hx`, reading x as more
/// flags, and refuses `-h=x` and `-h-x` with exit 2.
#[test]
fn a_value_glued_to_help_is_refused() {
    for a in ["-h=1", "-h=", "-h-", "-h-x", "-hh=1"] {
        assert!(matches!(parse(&[a]), Cli::Usage(_)), "{a}");
    }
    for a in ["-hx", "-hh", "-h1", "-hx=1"] {
        assert_eq!(parse(&[a]), Cli::Help, "{a}");
    }
}

#[test]
fn help_answers_when_it_is_reached() {
    assert_eq!(parse(&["-h"]), Cli::Help);
    assert_eq!(parse(&["-hx"]), Cli::Help);
    assert_eq!(parse(&["--help", "--gap", "x"]), Cli::Help);
    assert_eq!(parse(&["--bogus", "--help"]), Cli::Help);
    assert!(matches!(parse(&["--gap", "x", "--help"]), Cli::Usage(_)));
}

#[test]
fn every_child_name_is_taken() {
    for (name, child) in [
        ("cat", Child::Cat),
        ("shell", Child::Shell),
        ("none", Child::None),
        ("stream", Child::Stream),
        ("mute", Child::Mute),
    ] {
        assert_eq!(sweep_args(&["--child", name]).child, child);
    }
}
