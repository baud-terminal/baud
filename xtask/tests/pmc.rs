//! `cargo xtask pmc`: a report as the script printed it, on the shape
//! `xctrace export` writes, and a perturbation per branch of the reader and
//! of the sum.

use xtask::features_since::Exit;
use xtask::pmc::{self, Sample, aggregate, defined, events, grouped, referred, rows, samples};

fn report(xml: &str) -> (Exit, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = pmc::run_text(xml, &mut out, &mut err).expect("writers");
    (
        exit,
        String::from_utf8(out).expect("utf-8"),
        String::from_utf8(err).expect("utf-8"),
    )
}

/// A row as the export writes the first of a time and a core.
fn first(id: &str, time: &str, core: &str, counters: &str) -> String {
    format!(
        "<row><sample-time id=\"{id}\" fmt=\"00:00\">{time}</sample-time>\
         <thread id=\"900\" fmt=\"x\"><tid id=\"901\" fmt=\"0x1\">1</tid></thread>\
         <core id=\"{id}0\" fmt=\"CPU {core} (P Core)\">{core}</core>\
         <pmc-events fmt=\"y\">{counters}</pmc-events></row>\n"
    )
}

/// A later sample, naming its core by reference to an earlier row's.
fn next(id: &str, time: &str, core_id: &str, counters: &str) -> String {
    format!(
        "<row><sample-time id=\"{id}\" fmt=\"00:00\">{time}</sample-time>\
         <core ref=\"{core_id}0\"/><pmc-events fmt=\"y\">{counters}</pmc-events></row>\n"
    )
}

/// The script's output for this file, under Python 3.13.12, byte for byte.
#[test]
fn a_report_is_the_scripts() {
    let xml = format!(
        "<?xml version=\"1.0\"?>\n<trace-query-result><node>\n{}{}{}</node></trace-query-result>\n",
        first("1", "1000", "4", "0 0 0 0 0 7"),
        next("2", "2000", "1", "1000000 250000 250000 250000 245000 7"),
        next("3", "3000", "1", "2000000 500000 500000 500000 490000 8"),
    );
    let (exit, out, err) = report(&xml);
    assert_eq!((exit, err.as_str()), (Exit(0), ""));
    assert_eq!(
        out,
        "3 counter samples, 6 columns
consecutive same-core wall time: 0.0000 s
  idx0         2,000,000   1000.000 G/s
  idx1           500,000    250.000 G/s
  idx2           500,000    250.000 G/s
  idx3           500,000    250.000 G/s
  idx4           490,000    245.000 G/s
  idx5                 1      0.001 G/s

additive identities (target = sum of 4 others, within 2%):
  idx0 ~= idx1 + idx2 + idx3 + idx4   (1,990,000 vs 2,000,000, -0.50%)
"
    );
}

#[test]
fn the_identities_come_in_combinations_order_and_stop_at_two_percent() {
    let one = |counters: &str| {
        let xml = format!(
            "{}{}",
            first("1", "1", "0", "0 0 0 0 0"),
            next("2", "2", "1", counters)
        );
        report(&xml).1
    };
    // 2.00% exactly is not within it, and one count closer is.
    assert!(!one("1000 245 245 245 245").contains("~="));
    assert!(
        one("1000 246 245 245 245")
            .ends_with("  idx0 ~= idx1 + idx2 + idx3 + idx4   (981 vs 1,000, -1.90%)\n")
    );
    // Above is reported with its sign, and the same limit.
    assert!(!one("1000 255 255 255 255").contains("~="));
    let up = one("1000 254 255 255 255");
    assert!(up.ends_with("(1,019 vs 1,000, +1.90%)\n"), "{up}");
    // A column at zero is no candidate, on either side.
    let xml = format!(
        "{}{}",
        first("1", "1", "0", "0 0 0 0 0 0"),
        next("2", "2", "1", "1000 250 250 250 250 0")
    );
    let zero = report(&xml).1;
    assert!(
        zero.ends_with("idx0 ~= idx1 + idx2 + idx3 + idx4   (1,000 vs 1,000, +0.00%)\n"),
        "{zero}"
    );
    assert!(
        !zero.contains("idx5 ~=") && !zero.contains("+ idx5"),
        "{zero}"
    );
}

#[test]
fn the_reader_takes_only_the_spelling_the_export_writes() {
    let r = "<sample-time id=\"7\" fmt=\"a\">55</sample-time>";
    assert_eq!(defined(r, "sample-time"), Some(("7", "55")));
    // The id must be the first attribute, the value digits, and it must close.
    assert_eq!(defined("<a fmt=\"x\" id=\"1\">2</a>", "a"), None);
    assert_eq!(defined("<a id=\"1\">x</a>", "a"), None);
    assert_eq!(defined("<a id=\"1\">2</b>", "a"), None);
    assert_eq!(defined("<a id=\"x\">2</a>", "a"), None);
    assert_eq!(defined("<a id=\"1\"", "a"), None);
    assert_eq!(defined("<ab id=\"1\">2</ab>", "a"), None);
    // The first that reads, past one that does not.
    assert_eq!(
        defined("<a id=\"1\">x</a><a id=\"2\">3</a>", "a"),
        Some(("2", "3"))
    );

    assert_eq!(referred("<core ref=\"7\"/>", "core"), Some("7"));
    assert_eq!(referred("<core ref=\"7\"></core>", "core"), None);
    assert_eq!(referred("<core ref=\"x\"/>", "core"), None);

    assert_eq!(events("<pmc-events>1 2</pmc-events>"), Some(vec![1, 2]));
    assert_eq!(
        events("<pmc-events a=\"b\">  3\u{20}\u{20}4 </pmc-events>"),
        Some(vec![3, 4])
    );
    assert_eq!(events("<pmc-events-more>5</pmc-events>"), Some(vec![5]));
    assert_eq!(events("<pmc-events> </pmc-events>"), Some(Vec::new()));
    assert_eq!(events("<pmc-events></pmc-events>"), None);
    assert_eq!(events("<pmc-events>1\t2</pmc-events>"), None);
    assert_eq!(
        events("<pmc-events>x</pmc-events><pmc-events>1</pmc-events>"),
        Some(vec![1])
    );

    assert_eq!(
        rows("<row>a</row> <row>\nb\n</row><row>open"),
        ["a", "\nb\n"]
    );
    assert_eq!(rows("<row x=\"1\">a</row>"), [""; 0]);
}

#[test]
fn a_row_is_a_sample_only_when_its_time_and_core_resolve() {
    let got = samples(&format!(
        "{}{}{}{}",
        first("1", "10", "0", "1"),
        // Its core's reference resolves, and so does its time's.
        "<row><sample-time ref=\"1\"/><core ref=\"10\"/><pmc-events>2</pmc-events></row>",
        // Neither time nor core is known.
        "<row><sample-time ref=\"8\"/><core ref=\"10\"/><pmc-events>3</pmc-events></row>",
        "<row><sample-time ref=\"1\"/><core ref=\"80\"/><pmc-events>4</pmc-events></row>",
    ))
    .expect("the samples");
    assert_eq!(
        got,
        [
            Sample {
                time: 10,
                core: 0,
                counters: vec![1]
            },
            Sample {
                time: 10,
                core: 0,
                counters: vec![2]
            },
        ]
    );
    // No events, no sample; but the row's definitions still count.
    let got = samples(
        "<row><sample-time id=\"1\">5</sample-time><core id=\"2\">0</core></row>\
         <row><sample-time ref=\"1\"/><core ref=\"2\"/><pmc-events>7</pmc-events></row>",
    )
    .expect("the samples");
    assert_eq!(
        got,
        [Sample {
            time: 5,
            core: 0,
            counters: vec![7]
        }]
    );
}

fn sample(time: i128, core: i128, counters: &[i128]) -> Sample {
    Sample {
        time,
        core,
        counters: counters.to_vec(),
    }
}

#[test]
fn the_sum_takes_only_consecutive_same_core_forward_deltas() {
    let sum = |s: &[Sample]| aggregate(s).expect("the sum");
    let a = sample(10, 0, &[100]);
    assert_eq!(sum(&[a.clone(), sample(20, 0, &[130])]), (10, vec![30]));
    // A thread that migrated: the delta is not work.
    assert_eq!(sum(&[a.clone(), sample(20, 1, &[130])]), (0, vec![0]));
    // A counter that ran backwards.
    assert_eq!(sum(&[a.clone(), sample(20, 0, &[99])]), (0, vec![0]));
    // No time passed, and time that ran backwards.
    assert_eq!(sum(&[a.clone(), sample(10, 0, &[130])]), (0, vec![0]));
    assert_eq!(sum(&[a.clone(), sample(9, 0, &[130])]), (0, vec![0]));
    // Five milliseconds is the limit, and one past it is out.
    assert_eq!(
        sum(&[a.clone(), sample(5_000_010, 0, &[130])]),
        (5_000_000, vec![30])
    );
    assert_eq!(
        sum(&[a.clone(), sample(5_000_011, 0, &[130])]),
        (0, vec![0])
    );
    // A skipped pair still leaves its second as the next pair's first.
    assert_eq!(
        sum(&[a.clone(), sample(20, 1, &[500]), sample(30, 1, &[510])]),
        (10, vec![10])
    );
    // A wider row is read to the first row's width.
    assert_eq!(
        sum(&[sample(1, 0, &[1, 2]), sample(2, 0, &[2, 3, 9])]),
        (1, vec![1, 1])
    );
    // Past `u64`, as a Python integer is.
    assert_eq!(
        sum(&[
            sample(1, 0, &[18_446_744_073_709_551_615]),
            sample(2, 0, &[18_446_744_073_709_551_616])
        ]),
        (1, vec![1])
    );
    // A narrower row is the script's `IndexError`.
    assert!(aggregate(&[sample(1, 0, &[1, 2, 3]), sample(2, 0, &[2, 3])]).is_err());
}

#[test]
fn what_the_script_raised_on_exits_1_after_what_it_printed() {
    let (exit, out, err) = report("<a/>");
    assert_eq!((exit, out.as_str()), (Exit(1), ""));
    assert!(err.starts_with("pmc: "), "{err}");

    let short = format!(
        "{}{}",
        first("1", "1", "0", "1 2 3"),
        next("2", "2", "1", "2 3")
    );
    let (exit, out, _) = report(&short);
    assert_eq!(exit, Exit(1));
    assert_eq!(out, "2 counter samples, 3 columns\n");

    let (mut o, mut e) = (Vec::new(), Vec::new());
    assert_eq!(pmc::run(&[], &mut o, &mut e).expect("writers"), Exit(1));
    assert_eq!(
        pmc::run(&["/nonexistent/x"], &mut o, &mut e).expect("writers"),
        Exit(1)
    );
}

#[test]
fn one_sample_has_no_deltas_and_a_rate_of_zero() {
    let (exit, out, _) = report(&first("1", "1", "0", "5 6"));
    assert_eq!(exit, Exit(0));
    assert_eq!(
        out,
        "1 counter samples, 2 columns
consecutive same-core wall time: 0.0000 s
  idx0                 0      0.000 G/s
  idx1                 0      0.000 G/s

additive identities (target = sum of 4 others, within 2%):
"
    );
}

#[test]
fn a_thousands_separator_every_three_digits() {
    assert_eq!(grouped(0), "0");
    assert_eq!(grouped(999), "999");
    assert_eq!(grouped(1000), "1,000");
    assert_eq!(grouped(1_234_567), "1,234,567");
    assert_eq!(grouped(-1000), "-1,000");
}
