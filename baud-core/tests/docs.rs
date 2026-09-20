#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. That happened to
// the blanking test in `amplification.rs` -- then named
// `blanking_writes_the_whole_row_and_that_is_the_cheaper_choice`, and since
// renamed because the decision it pinned was reversed by measurement -- whose
// assertion was wrong for two commits without anyone finding out. The
// compiler's own dead-code analysis is the check that catches it.
//! The documentation, checked rather than asserted.
//!
//! `tests/coverage.rs` already checks that every **done** row in
//! `docs/vt-coverage.md` names a tested operation, that no row is unfinished,
//! and that the counts quoted elsewhere match the table.
//! `scripts/check-sources.sh` checks that every `[[source: …]]` citation
//! resolves to a hash-pinned document. This file covers what neither did: that
//! the docs' *references to the repository itself* are still true.
//!
//! Every one of these has been wrong here at least once. `docs/vt-coverage.md`
//! spent most of this project's life naming `tests/coverage.rs` as the
//! mechanism that kept it honest, in a file that did not exist; README pointed
//! at `bench/src/payload.rs` after the crate was renamed to `baud-bench`. The
//! failure mode is always the same — the prose stays put while the thing it
//! names moves — and it is exactly the mechanical kind, so a test can hold it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// Every `.md` in the repository, excluding build output.
/// Markdown **and Rust**, because a test can be cited from either.
///
/// It was markdown only, and a doc comment in `vt_properties.rs` claimed
/// a test covered the charset rows while no such function existed. That is the same defect as a `done` row with no test, one
/// file type over, and the scanner could not see it.
///
/// Rust files are read whole rather than only their `///` lines: a name cited
/// in an ordinary `//` comment is just as wrong, and the multi-underscore rule
/// below already keeps this from firing on identifiers.
fn walk(dir: &Path, out: &mut Vec<PathBuf>, want_rust: bool) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if matches!(name, "target" | ".git" | "node_modules" | ".devenv") {
                continue;
            }
            // Pinned copies of other projects' files, hash-locked by
            // `MANIFEST.toml`. Their links point into repositories that are not
            // this one and their identifiers are theirs; a gate over them would
            // demand edits that break the hash. Group J is what made this bite:
            // msdfgen's README links its own `CMakeLists.txt`, and vello's
            // `strip.rs` names its own multi-underscore identifiers, which this
            // file's test-name rule would read as baud tests.
            if p.ends_with("docs/research/sources") {
                continue;
            }
            walk(&p, out, want_rust);
        } else if name.ends_with(".md") || (want_rust && name.ends_with(".rs")) {
            out.push(p);
        }
    }
}

fn markdown_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(&root(), &mut out, false);
    out.sort();
    out
}

/// Markdown **and Rust**, for the test-name check only.
///
/// Links and repository paths are a markdown concern; a `crate::AmbiguousWidth`
/// in a Rust doc comment is not a path and reading it as one produces noise.
/// A cited *test* is different: it is just as wrong in a `///` comment as in a
/// table, and the scanner could not see it.
fn doc_and_code_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(&root(), &mut out, true);
    out.sort();
    out
}

fn rel(p: &Path) -> String {
    p.strip_prefix(root())
        .unwrap_or(p)
        .display()
        .to_string()
        .replace("../", "")
}

/// **Every relative link in every document resolves to a file that exists.**
///
/// Checked mechanically because checking it by eye is how they rot: a link is
/// only ever followed by someone who does not already know where the thing is,
/// which is the person least able to tell that it is wrong.
#[test]
fn every_relative_link_resolves() {
    let mut broken = Vec::new();
    let mut checked = 0usize;
    for f in markdown_files() {
        let Ok(src) = fs::read_to_string(&f) else {
            continue;
        };
        let dir = f.parent().unwrap().to_path_buf();
        // `](target)` — markdown inline links.
        let mut rest = src.as_str();
        while let Some(i) = rest.find("](") {
            rest = &rest[i + 2..];
            let Some(end) = rest.find(')') else { break };
            let target = &rest[..end];
            rest = &rest[end..];
            // External, anchor-only, or mail: not this test's business.
            if target.starts_with("http")
                || target.starts_with('#')
                || target.starts_with("mailto:")
                || target.is_empty()
            {
                continue;
            }
            // Strip any `#anchor`.
            let path = target.split('#').next().unwrap_or(target);
            if path.is_empty() {
                continue;
            }
            checked += 1;
            if !dir.join(path).exists() {
                broken.push(format!("{} -> {target}", rel(&f)));
            }
        }
    }
    assert!(
        checked > 20,
        "only {checked} links checked -- the link scanner stopped matching"
    );
    assert!(
        broken.is_empty(),
        "{} broken link(s):\n  {}",
        broken.len(),
        broken.join("\n  ")
    );
}

/// **Every repository path a document names in backticks exists.**
///
/// Documents here cite real paths constantly — `baud-core2/src/parse/fast.rs`,
/// `scripts/check-sources.sh`, `docs/research/notes/g-text-encoding.md` — and
/// those are load-bearing: they are how a reader gets from a claim to the code
/// that implements it. A path that has moved turns the claim into folklore.
///
/// Only strings that unambiguously *look* like repository paths are checked: a
/// backticked token containing `/`, starting with a known top-level directory,
/// and ending in a real file extension. That deliberately misses some and
/// invents none.
#[test]
fn every_repository_path_named_in_the_docs_exists() {
    const ROOTS: &[&str] = &[
        "baud-core/",
        "baud-domain/",
        "baud-bench/",
        "baud-font/",
        "baud-render/",
        "baud-present-macos/",
        "baud-app/",
        "baud-wayland/",
        "baud-xkb/",
        "baud-present-wayland/",
        // **The four that were missing**, and the cost of missing them was
        // not a stale path but a false claim: SECURITY.md cited
        // `baud-isa/src/avx2.rs` -- a file that has never existed under
        // that name here -- to support "unsafe appears in exactly one
        // place", and neither half could be checked because the root was
        // not in this list.
        "baud-isa/",
        "baud-isa-aarch64/",
        "baud-isa-x86-64/",
        "baud-difftest/",
        // The second external oracle, added with the crate: a path under it
        // named in a document is checked, which the root's absence would not.
        "baud-sfte-difftest/",
        "bench-others/",
        "docs/",
        "scripts/",
        ".github/",
        "assets/",
    ];
    const EXTS: &[&str] = &[".rs", ".md", ".sh", ".toml", ".nix", ".py", ".yml", ".yaml"];
    let mut missing = BTreeSet::new();
    let mut checked = 0usize;
    for f in markdown_files() {
        let Ok(src) = fs::read_to_string(&f) else {
            continue;
        };
        for tok in src.split('`').skip(1).step_by(2) {
            let tok = tok.trim();
            if !tok.contains('/') || tok.contains(' ') {
                continue;
            }
            if !ROOTS.iter().any(|r| tok.starts_with(r)) {
                continue;
            }
            if !EXTS.iter().any(|e| tok.ends_with(e)) {
                continue;
            }
            for path in expand_braces(tok) {
                checked += 1;
                if !root().join(&path).exists() {
                    missing.insert(format!("{}: {path}", rel(&f)));
                }
            }
        }
    }
    assert!(
        checked > 15,
        "only {checked} paths checked -- the path scanner stopped matching"
    );
    assert!(
        missing.is_empty(),
        "{} path(s) named in docs that do not exist:\n  {}",
        missing.len(),
        missing.into_iter().collect::<Vec<_>>().join("\n  ")
    );
}

/// Expand shell-style `{a,b,c}` shorthand, which the protocol docs use to name
/// a group of modules in one breath: `baud-core2/src/{osc,mouse,modes}.rs`.
/// Readable, and not a path — so it is expanded rather than banned.
fn expand_braces(tok: &str) -> Vec<String> {
    let (Some(open), Some(close)) = (tok.find('{'), tok.find('}')) else {
        return vec![tok.to_string()];
    };
    if close < open {
        return vec![tok.to_string()];
    }
    let (pre, mid, post) = (&tok[..open], &tok[open + 1..close], &tok[close + 1..]);
    mid.split(',')
        .map(|m| format!("{pre}{}{post}", m.trim()))
        .collect()
}

/// **Every test named in a document exists.**
///
/// Documents point at tests by name as evidence — "`no_row_is_unfinished` fails
/// if one does", "`a_marker_after_a_dcs_parameter_is_ignored` pins both".
/// A named test that has been renamed or deleted is a citation to nothing, and
/// it reads exactly like a citation to something.
#[test]
fn every_test_named_in_the_docs_exists() {
    // Gather every `fn <name>` in the test tree and the source.
    let mut have = BTreeSet::new();
    fn collect(dir: &Path, have: &mut BTreeSet<String>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().and_then(|n| n.to_str()) == Some("target") {
                    continue;
                }
                collect(&p, have);
            } else if p.extension().is_some_and(|x| x == "rs") {
                if let Ok(src) = fs::read_to_string(&p) {
                    let mut rest = src.as_str();
                    while let Some(i) = rest.find("fn ") {
                        rest = &rest[i + 3..];
                        let name: String = rest
                            .chars()
                            .take_while(|c| c.is_alphanumeric() || *c == '_')
                            .collect();
                        if !name.is_empty() {
                            have.insert(name);
                        }
                    }
                }
            }
        }
    }
    // **`baud-core` is in this list because the gate must follow the code.**
    // The new core carries its own tests and `CLAUDE.md` cites them; a
    // collector that walked only core2 would report every one of them as a
    // citation to nothing, which is the "instrumentation outlived what fed it"
    // failure this repo already records twice.
    //
    // **And it must follow the code as it is, not as it was.** This list named
    // `baud-core2` until today -- a crate deleted some time ago -- and
    // `collect` returns silently on a directory that is not there, so the gate
    // walked nothing for it and said nothing. It also omitted `baud-difftest`
    // and the three ISA crates, which do carry tests this file cites. A gate
    // scoped to the wrong set of crates is the failure this repository has now
    // recorded three times: `docs.rs`'s own `ROOTS`, `audit_all.sh`'s budgets,
    // and the CI step that named a package with no such target.
    //
    // The directories are checked rather than assumed, so a rename here fails
    // loudly instead of quietly reducing what is collected.
    for d in [
        "baud-core",
        "baud-domain",
        "baud-bench",
        "baud-difftest",
        "baud-isa",
        "baud-isa-aarch64",
        "baud-isa-x86-64",
        "baud-font",
        "baud-render",
        "baud-present-macos",
        "baud-app",
        "baud-wayland",
        "baud-xkb",
        "baud-present-wayland",
    ] {
        let dir = root().join(d);
        assert!(
            dir.is_dir(),
            "{d} is in the collector's list and does not exist -- the gate \
             would walk nothing for it and report no error"
        );
        collect(&dir, &mut have);
    }

    // snake_case identifiers in these docs that are deliberately *not* baud
    // tests. Kept explicit and tiny, as in `coverage.rs`: a growing list here
    // means the check is being worked around rather than a citation fixed.
    const NOT_A_TEST: &[(&str, &str)] = &[
        (
            "sos_pm_apc_string",
            "a williams parser state, named in the Tier 0 table",
        ),
        (
            "treat_east_asian_ambiguous_width_as_wide",
            "wezterm's config option, cited in note G's comparison",
        ),
        (
            "blanking_writes_the_whole_row_and_that_is_the_cheaper_choice",
            "a historical name. The test still exists as              `blanking_stops_at_the_used_extent`; it was renamed when the              decision it pinned -- that a full-width blank is cheaper than              avoiding one -- was reversed by measurement. The old name is kept              in the dead-code header comments because that is the incident              they describe",
        ),
        (
            "_vte_row_data_get",
            "VTE's C accessor, quoted in note F where it returns NULL past the              row's stored length",
        ),
        (
            "n_tty_receive_buf_common",
            "the Linux N_TTY function that keeps one byte of the read ring free,              quoted in note E and in `SLOT_BYTES`'s doc comment as the line the              4095-byte read bound comes from",
        ),
        (
            "_mm256_movemask_epi8",
            "an x86 intrinsic, named where CI checks the binary for its              instruction rather than trusting the AVX2 property's output match",
        ),
        (
            "_mm256_loadu_si256",
            "an x86 intrinsic, named where this file records that              `baud-isa-x86-64` scans 32 bytes where `core::simd` lowers to 16",
        ),
        (
            "swap_buffers_with_damage",
            "alacritty's EGL call, cited in note J where damage is forwarded to              the presentation layer",
        ),
        (
            "zwp_linux_dmabuf_v1",
            "a Wayland interface name, cited in note J for the version Hyprland              advertises",
        ),
        (
            "zwp_linux_buffer_params",
            "a Wayland interface name, cited in note J for the dmabuf swapchain              a pure-protocol client has to build itself",
        ),
    ];

    let mut missing = BTreeSet::new();
    let mut checked = 0usize;
    for f in doc_and_code_files() {
        let Ok(src) = fs::read_to_string(&f) else {
            continue;
        };
        for tok in src.split('`').skip(1).step_by(2) {
            let tok = tok.trim();
            if NOT_A_TEST.iter().any(|(n, _)| *n == tok) {
                continue;
            }
            // A test name: snake_case, several words, no punctuation. The
            // multi-underscore rule is what keeps this from firing on every
            // ordinary identifier.
            if tok.len() < 12
                || tok.matches('_').count() < 3
                || !tok
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                continue;
            }
            checked += 1;
            if !have.contains(tok) {
                missing.insert(format!("{}: {tok}", rel(&f)));
            }
        }
    }
    assert!(
        checked > 10,
        "only {checked} test names checked -- the scanner stopped matching"
    );
    assert!(
        missing.is_empty(),
        "{} test(s) named in docs that do not exist:\n  {}",
        missing.len(),
        missing.into_iter().collect::<Vec<_>>().join("\n  ")
    );
}

/// **Nothing is listed as absent that the coverage table marks done.**
///
/// This is the class that got past every other check. `CLAUDE.md` listed
/// `OSC 8` under "Unmeasured — do not assert", beside reflow and selection, as
/// something still needing random access into variable-length spans. Hyperlinks
/// are implemented: the link index rides in spare `CellFlags` bits and `NLINK`
/// slots evict on collision. An agent reading that list would conclude they
/// were unbuilt, and might build them a second time.
///
/// No path was broken and no test name was stale, so `every_relative_link_
/// resolves` and friends all passed. The document was simply *wrong about the
/// code*, which is the one thing they cannot see — and it is the exact failure
/// `vt-coverage.md` exists to prevent, one file over.
///
/// The check is narrow on purpose: a mnemonic named in a "not done" section
/// that also has a **done** row in the coverage table is a contradiction
/// between two documents in the same repository, and one of them is wrong.
#[test]
fn nothing_disclaimed_is_also_marked_done() {
    let root = root();
    let coverage = fs::read_to_string(root.join("docs/vt-coverage.md")).expect("vt-coverage.md");

    // Mnemonics with a `done` row.
    let mut done = BTreeSet::new();
    for line in coverage.lines() {
        let l = line.trim();
        if !l.starts_with('|') || !l.contains("| done") {
            continue;
        }
        for tok in l.split(|c: char| !c.is_ascii_alphanumeric()) {
            if tok.len() >= 3
                && tok.len() <= 8
                && tok
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                && tok.chars().any(|c| c.is_ascii_uppercase())
            {
                done.insert(tok.to_string());
            }
        }
    }
    assert!(
        done.len() > 20,
        "only {} done mnemonics parsed -- the parser stopped matching",
        done.len()
    );

    // Sections that claim something is *not* there.
    const DISCLAIMERS: &[&str] = &[
        "## Unmeasured — do not assert",
        "## Things already tried that lost",
    ];
    let mut contradictions = Vec::new();
    for f in ["CLAUDE.md", "docs/vt-coverage.md", "README.md"] {
        let Ok(src) = fs::read_to_string(root.join(f)) else {
            continue;
        };
        for start in DISCLAIMERS {
            let Some(i) = src.find(start) else { continue };
            let rest = &src[i + start.len()..];
            let end = rest.find("\n## ").unwrap_or(rest.len());
            let section = &rest[..end];
            for (n, line) in section.lines().enumerate() {
                // Struck-through entries are records of things that *were*
                // true, and say so.
                if line.contains("~~") {
                    continue;
                }
                for tok in line.split(|c: char| !c.is_ascii_alphanumeric()) {
                    if done.contains(tok) {
                        contradictions.push(format!(
                            "{f}: {start:?} names `{tok}`, which vt-coverage.md marks done\n      {}",
                            line.trim()
                        ));
                        let _ = n;
                    }
                }
            }
        }
    }
    assert!(
        contradictions.is_empty(),
        "{} contradiction(s) between a disclaimer and the coverage table:\n  {}",
        contradictions.len(),
        contradictions.join("\n  ")
    );
}

/// **The ladder's store-traffic column is arithmetic, so it is checked.**
///
/// Each rung reports an input rate, a cells-per-byte ratio and the store traffic
/// those imply, and the identity is exact: a cell is eight bytes, so
/// `traffic = rate * 8 * cells_per_byte`. A column of numbers that *follow* from
/// two others is the cheapest kind of claim to get wrong and the cheapest to
/// gate, and this one was wrong in all four rows -- understated by 4.86%,
/// because it multiplied `ingest2`'s mebibytes as though they were decimal
/// megabytes.
///
/// **That is the same slip `ceiling.rs` made independently**, which is what a
/// convention nobody wrote down produces: not one error but the same error
/// twice, years apart, by different hands. The unit pair is now stated in the
/// harness and enforced here.
#[test]
fn the_ladder_arithmetic_holds() {
    const MIB: f64 = (1u64 << 20) as f64;
    let text = fs::read_to_string(root().join("CLAUDE.md")).expect("CLAUDE.md");

    // `| `wrap` | ... | **5,850** | 1.0 | 49.1 GB/s |`
    let mut checked = 0usize;
    for line in text.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if cells.len() < 6 {
            continue;
        }
        let num = |c: &str| -> Option<f64> {
            let t: String = c
                .chars()
                .filter(|ch| ch.is_ascii_digit() || *ch == '.')
                .collect();
            t.parse::<f64>().ok()
        };
        // The traffic cell is the one naming GB/s; the two before it are the
        // rate and the ratio.
        if !cells[5].contains("GB/s") {
            continue;
        }
        let (Some(rate), Some(cpb), Some(traffic)) = (num(cells[3]), num(cells[4]), num(cells[5]))
        else {
            continue;
        };
        // Guard against matching some other five-column table: a rung's rate is
        // thousands of MB/s and its ratio is between one and three.
        if !(1000.0..20000.0).contains(&rate) || !(0.5..4.0).contains(&cpb) {
            continue;
        }
        let want = rate * MIB * 8.0 * cpb / 1e9;
        assert!(
            (want - traffic).abs() / want < 0.02,
            "ladder row `{}`: {rate} MB/s x 8 x {cpb} cells/byte is {want:.1} GB/s, \
             the table says {traffic} -- a 4.86% gap here means the rate was \
             multiplied as decimal when `ingest2` prints mebibytes",
            cells[1]
        );
        checked += 1;
    }
    assert_eq!(
        checked, 4,
        "expected the ladder's four rungs and matched {checked} rows -- the \
         table shape changed and this gate is reading something else"
    );
}

/// **Every row of the cycles-per-byte table must imply the same clock.**
///
/// `cycles/byte = clock / (MB/s * 2^20)`, so each row carries a clock and the
/// seven of them must agree -- they were taken on one box in one run. A
/// throughput edited without its cycle figure, or the reverse, breaks that
/// agreement and nothing else would notice: both columns stay individually
/// plausible.
///
/// **The value is deliberately not pinned.** Asserting 4.20 GHz would make this
/// gate wrong the day the table is honestly re-measured on another machine,
/// which is the opposite of what it is for. What cannot change is that the rows
/// agree with *each other*, whatever box they came from -- so that is the
/// assertion, and the implied clock is reported rather than required.
#[test]
fn the_cycles_per_byte_table_agrees_with_itself() {
    const MIB: f64 = (1u64 << 20) as f64;
    let text = fs::read_to_string(root().join("CLAUDE.md")).expect("CLAUDE.md");

    let mut clocks: Vec<(String, f64)> = Vec::new();
    let mut in_table = false;
    for line in text.lines() {
        if line.starts_with("| payload | MB/s | cycles/byte |") {
            in_table = true;
            continue;
        }
        if in_table {
            if !line.starts_with('|') {
                break;
            }
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            if cells.len() < 5 {
                continue;
            }
            let num = |c: &str| -> Option<f64> {
                let t: String = c
                    .chars()
                    .filter(|ch| ch.is_ascii_digit() || *ch == '.')
                    .collect();
                t.parse::<f64>().ok()
            };
            if let (Some(mbs), Some(cpb)) = (num(cells[2]), num(cells[3])) {
                if mbs > 1.0 && cpb > 0.0 {
                    clocks.push((cells[1].to_string(), mbs * MIB * cpb / 1e9));
                }
            }
        }
    }

    // Positive control: the table has seven payload rows, and a reader that
    // found fewer is reading something else.
    assert_eq!(
        clocks.len(),
        7,
        "expected seven rows in the cycles-per-byte table, found {} -- the \
         table shape changed and this gate is no longer reading it",
        clocks.len()
    );

    let lo = clocks.iter().map(|(_, c)| *c).fold(f64::MAX, f64::min);
    let hi = clocks.iter().map(|(_, c)| *c).fold(0.0, f64::max);
    assert!(
        (hi - lo) / lo < 0.02,
        "the rows imply clocks from {lo:.2} to {hi:.2} GHz, which cannot all be \
         one machine in one run: {:?}",
        clocks
            .iter()
            .map(|(n, c)| format!("{n} {c:.2}"))
            .collect::<Vec<_>>()
    );
}
