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
//! the docs' *references to the repository itself* are still true -- and, in
//! `no_attribute_is_stranded_above_a_doc_comment` and
//! `no_doc_is_stranded_on_the_item_before_its_own`, that a doc comment still
//! sits on the item it was written for.
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
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// **The repository's files whose extension is one of `exts`, as git tracks
/// them**: the paths `git ls-files` lists under `repo`, sorted.
///
/// Markdown **and Rust**, because a test can be cited from either, and a
/// name cited in an ordinary `//` comment is as wrong as one in a table.
///
/// Only tracked files are read: a file the checkout carries and git does not
/// (a local note, an excluded draft, build output) is not one of this
/// repository's documents, and a gate over it would fail on one machine and
/// pass on a clean checkout. Without git the checks cannot know which files
/// are the repository's, so this panics rather than read nothing.
fn tracked_files(repo: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let out = git(repo)
        .args(["ls-files", "-z"])
        .output()
        .unwrap_or_else(|err| {
            panic!(
                "git could not run ({err}): the docs checks read the files git \
                 tracks, and need git on PATH"
            )
        });
    assert!(
        out.status.success(),
        "git ls-files failed in {}: {}",
        repo.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let mut files: Vec<PathBuf> = out
        .stdout
        .split(|b| *b == 0)
        .filter_map(|p| std::str::from_utf8(p).ok())
        .filter(|p| !p.is_empty())
        // Pinned copies of other projects' files, hash-locked by
        // `MANIFEST.toml`. Their links point into repositories that are not
        // this one and their identifiers are theirs; a gate over them would
        // demand edits that break the hash.
        .filter(|p| !p.starts_with("docs/research/sources/"))
        .filter(|p| {
            Path::new(p)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| exts.contains(&e))
        })
        .map(|p| repo.join(p))
        .collect();
    files.sort();
    files
}

/// `git -C repo`, with the variables that would point it at another
/// repository taken out.
fn git(repo: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    c
}

/// Every tracked `.md` in the repository.
fn markdown_files() -> Vec<PathBuf> {
    tracked_files(&root(), &["md"])
}

/// Markdown **and Rust**, for the test-name check and the stranded-attribute
/// gate.
///
/// Links and repository paths are a markdown concern; a `crate::AmbiguousWidth`
/// in a Rust doc comment is not a path and reading it as one produces noise.
/// A cited *test* is different: it is just as wrong in a `///` comment as in a
/// table, and the scanner could not see it.
fn doc_and_code_files() -> Vec<PathBuf> {
    tracked_files(&root(), &["md", "rs"])
}

/// **`line` with its code spans taken out**: each run of text between a
/// backtick and the next on the line. A backtick left unpaired keeps the
/// rest of the line, so a span the line does not close hides nothing.
fn outside_code_spans(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('\x60') {
        let Some(close) = rest[open + 1..].find('\x60') else {
            break;
        };
        out.push_str(&rest[..open]);
        rest = &rest[open + 1 + close + 1..];
    }
    out.push_str(rest);
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
        // A fenced code block holds code, not links: a starship format's
        // `[text]($style)` is not one. Nor does a code span on a line, which
        // renders its brackets as they are written: a quoted README's
        // "See `[LICENSE](LICENSE)`" is its text, not a link of this file's.
        let mut fenced = false;
        let prose: String = src
            .lines()
            .filter(|l| {
                // Three backticks, spelt so this file's own backticks pair.
                if l.trim_start().starts_with("\x60\x60\x60") {
                    fenced = !fenced;
                    return false;
                }
                !fenced
            })
            .map(|l| format!("{}\n", outside_code_spans(l)))
            .collect();
        // `](target)` — markdown inline links.
        let mut rest = prose.as_str();
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

/// **`cargo run` runs baud, because `baud` is the workspace's one binary.**
///
/// The only binary, and the default one, is `baud`; the rest is part of the
/// tests. `docs/guide/install.md` tells a
/// reader `cargo run --release` starts the terminal, and that holds only
/// while no other member grows a binary target, `baud-app` keeps
/// `default-run = "baud"`, and the root keeps
/// `default-members = ["baud-app"]` -- without which a bare `cargo run`
/// unifies features across every member and builds a different `baud` from
/// the bundle's. The two differentials were binaries until 2026-10-12 and are
/// test targets now. The one other binary is `xtask`'s, the repository's
/// gates: unpublished, run only through the `cargo xtask` alias, and outside
/// `default-members`, so `cargo run` never reaches it. The member list is
/// read from the root manifest, and the positive controls are its first and
/// last members and `baud-app`'s own `[[bin]]`, which the scan must find.
#[test]
fn baud_is_the_only_binary_and_the_default() {
    let manifest = fs::read_to_string(root().join("Cargo.toml")).expect("Cargo.toml");
    let list = manifest
        .split_once("\nmembers = [")
        .and_then(|(_, rest)| rest.split_once(']'))
        .expect("the root manifest's members list")
        .0;
    let members: Vec<&str> = list
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .flat_map(|l| l.split('"').skip(1).step_by(2))
        .collect();
    assert!(
        members.first() == Some(&"baud-domain")
            && members.last() == Some(&"baud-present-wayland")
            && members.contains(&"baud-app"),
        "the members scan stopped matching: {members:?}"
    );
    assert!(
        manifest
            .lines()
            .any(|l| l.trim() == "default-members = [\"baud-app\"]"),
        "the root is to keep `default-members = [\"baud-app\"]`, so `cargo run` \
         builds the bundle's `baud`"
    );
    let mut bins = Vec::new();
    for m in &members {
        let dir = root().join(m);
        let toml = fs::read_to_string(dir.join("Cargo.toml")).expect("a member's manifest");
        for l in toml.lines().filter(|l| l.trim() == "[[bin]]") {
            bins.push(format!("{m}: {l}"));
        }
        if *m != "baud-app" && dir.join("src/main.rs").exists() {
            bins.push(format!("{m}: src/main.rs"));
        }
        if dir.join("src/bin").exists() {
            bins.push(format!("{m}: src/bin/"));
        }
    }
    assert_eq!(
        bins,
        ["baud-app: [[bin]]", "xtask: src/main.rs"],
        "baud is to be the only binary but the gates' xtask; the rest are \
         tests, benches or examples"
    );
    let app = fs::read_to_string(root().join("baud-app/Cargo.toml")).expect("baud-app");
    assert!(
        app.lines().any(|l| l.trim() == "name = \"baud\"")
            && app.lines().any(|l| l.trim() == "default-run = \"baud\""),
        "baud-app's binary is to be `baud`, and `cargo run` is to run it"
    );
}

/// **Paths a document moved from, and where it went**, for history only.
///
/// A dated record under `docs/research/` names a file by what it was called
/// on the day it was written, and rewriting it would edit evidence. So a
/// path moved on purpose resolves to its new home when, and only when, the
/// document naming it is under `docs/research/`; every other document has
/// to use the new path. Each entry is checked as well: its old path must be
/// gone and its new one present, so a revert or a second move fails here
/// rather than leaving an entry that excuses nothing.
const MOVED: &[(&str, &str)] = &[("baud-core/tests/readme.rs", "baud-core/tests/embedding.rs")];

/// **Every repository path a document names in backticks exists.**
///
/// Documents here cite real paths constantly — `baud-core/src/graph.rs`,
/// `scripts/check-sources.sh`, `docs/vt-coverage.md` — and
/// those are load-bearing: they are how a reader gets from a claim to the code
/// that implements it. A path that has moved turns the claim into folklore.
///
/// Only strings that unambiguously *look* like repository paths are checked: a
/// backticked token containing `/`, starting with a known top-level directory,
/// and ending in a real file extension. That deliberately misses some and
/// invents none.
#[test]
fn every_repository_path_named_in_the_docs_exists() {
    for (old, new) in MOVED {
        assert!(
            !root().join(old).exists() && root().join(new).exists(),
            "MOVED names {old} -> {new}, but the old path is still there or the new one is not"
        );
    }
    let (checked, missing) = named_paths(&root());
    assert!(
        checked.len() > 15,
        "only {} paths checked -- the path scanner stopped matching",
        checked.len()
    );
    assert!(
        missing.is_empty(),
        "{} path(s) named in docs that do not exist:\n  {}",
        missing.len(),
        missing.into_iter().collect::<Vec<_>>().join("\n  ")
    );
}

/// **The repository paths `repo`'s tracked documents name, and those of
/// them that do not exist**, each missing one as `<document>: <path>`.
fn named_paths(repo: &Path) -> (Vec<String>, BTreeSet<String>) {
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
        // The table compiler the build scripts share, added with the crate.
        "baud-tablegen/",
        // The frame handoff, added with the crate.
        "baud-pipeline/",
        "baud-difftest/",
        // The second external oracle, added with the crate: a path under it
        // named in a document is checked, which the root's absence would not.
        "baud-sfte-difftest/",
        "xtask/",
        "bench-others/",
        // The crates that link C engines, outside the workspace, added with
        // `tools/baud-oracle`.
        "tools/",
        "docs/",
        "scripts/",
        ".github/",
        "assets/",
    ];
    const EXTS: &[&str] = &[".rs", ".md", ".sh", ".toml", ".nix", ".py", ".yml", ".yaml"];
    let mut missing = BTreeSet::new();
    let mut checked = Vec::new();
    for f in tracked_files(repo, &["md"]) {
        let Ok(src) = fs::read_to_string(&f) else {
            continue;
        };
        let name = f.strip_prefix(repo).unwrap_or(&f).display().to_string();
        let history = name.starts_with("docs/research/");
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
                let moved = history && MOVED.iter().any(|(old, _)| *old == path);
                if !moved && !repo.join(&path).exists() {
                    missing.insert(format!("{name}: {path}"));
                }
                checked.push(format!("{name}: {path}"));
            }
        }
    }
    (checked, missing)
}

/// **The docs checks read the files git tracks and no other.** A scratch
/// repository holds two documents naming a path that does not exist, one
/// added to the index and one only on disk: the tracked one is reported and
/// the untracked one is not read at all.
#[test]
fn the_docs_checks_read_only_tracked_files() {
    let repo = Path::new(env!("CARGO_TARGET_TMPDIR")).join("docs-tracked-files");
    if repo.exists() {
        fs::remove_dir_all(&repo).expect("clear the scratch repository");
    }
    fs::create_dir_all(&repo).expect("scratch repository");
    let run = |args: &[&str]| {
        let out = git(&repo).args(args).output().expect("git on PATH");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["init", "-q"]);
    fs::write(repo.join("tracked.md"), "See `docs/tracked-gone.md`.\n").expect("tracked.md");
    fs::write(repo.join("untracked.md"), "See `docs/untracked-gone.md`.\n").expect("untracked.md");
    run(&["add", "tracked.md"]);
    assert!(
        repo.join("untracked.md").is_file(),
        "the untracked document is on disk"
    );

    assert_eq!(
        tracked_files(&repo, &["md"]),
        vec![repo.join("tracked.md")],
        "only the tracked document is listed"
    );
    let (checked, missing) = named_paths(&repo);
    assert_eq!(
        checked,
        vec!["tracked.md: docs/tracked-gone.md".to_string()]
    );
    assert_eq!(
        missing.into_iter().collect::<Vec<_>>(),
        vec!["tracked.md: docs/tracked-gone.md".to_string()],
        "the tracked document's missing path is reported"
    );
}

/// Expand shell-style `{a,b,c}` shorthand, which the protocol docs use to name
/// a group of modules in one breath: `baud-core/src/{write,ops,paint}.rs`.
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
    // Gather every `fn <name>` in the tracked test tree and source, the
    // same files the citations are read from.
    let mut have = BTreeSet::new();
    let rust = tracked_files(&root(), &["rs"]);
    let mut collect = |dir: &Path| {
        for p in rust.iter().filter(|p| p.starts_with(dir)) {
            if let Ok(src) = fs::read_to_string(p) {
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
    };
    // **`baud-core` is in this list because the gate must follow the code.**
    // The new core carries its own tests and the docs cite them; a
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
        // Absent until a document first named a test of its,
        // `the_report_has_nothing_unpinned`, which the gate then reported as a
        // citation to nothing.
        "baud-sfte-difftest",
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
        // Its docs name its pixel tests, such as
        // `multiply_s_second_step_is_its_second_rounding`.
        "baud-present-vulkan",
        "baud-tablegen",
        // Added with the crate: its tests are named as any crate's are.
        "baud-pipeline",
        "xtask",
        // Outside the workspace; its docs name the C functions its extern
        // blocks declare, which this collects as it collects any `fn`.
        "tools/baud-oracle",
        // Outside the workspace with it: the font comparator.
        "tools/baud-fontbuild",
        // And the pictures, whose no-platform-math test the README logo's
        // assets document cites.
        "tools/baud-pictures",
    ] {
        let dir = root().join(d);
        assert!(
            dir.is_dir(),
            "{d} is in the collector's list and does not exist -- the gate \
             would walk nothing for it and report no error"
        );
        collect(&dir);
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
            "a_promoted_run_keeps_the_set_it_was_written_under",
            "a historical name. The test still exists as \
             `a_cell_written_under_ungated_german_keeps_its_glyph_when_g0_is_designated_again`; \
             it was renamed because the run and the promotion its name described \
             were `baud-core2`'s. Note AF read the code before the rename and cites \
             the old name, and its §5(b) names the test by it",
        ),
        (
            "a_body_past_the_scratch_is_counted_against_the_body_with_its_length",
            "a historical name. The test held a Sixel body past its scratch to a \
             body refusal with its length, and was withdrawn on 2026-10-14, when a \
             Sixel began decoding as its body arrived and no scratch was left to \
             run past. It was cited where the refusals by cause were described",
        ),
        (
            "an_abandoned_body_leaves_nothing_and_the_next_sixel_draws_as_alone",
            "a historical name, replaced on 2026-10-19 by \
             `a_body_cut_off_is_placed_from_what_it_received` when a Sixel cut \
             off by CAN, ESC or a C1 began to be placed from what it received; \
             the old name was cited where the streaming decode was built",
        ),
        (
            "an_abandoned_sixel_gives_its_claim_back",
            "a historical name, replaced on 2026-10-19 by \
             `a_sixel_cut_off_keeps_its_claim_and_ris_gives_it_back`, for the \
             same change",
        ),
        (
            "a_gr_byte_in_a_dcs_body_or_under_utf8_is_what_it_was",
            "a historical name, replaced on 2026-10-19 by \
             `a_gr_byte_in_a_decrqss_body_is_its_seven_bit_byte_under_latin1` and \
             `a_gr_prefix_under_utf8_is_what_it_was` when a GR byte in a DECRQSS \
             body began to be read as its 7-bit byte; the old \
             name was cited where the GR prefix build was described",
        ),
        (
            "a_bel_inside_a_sixel_is_payload_and_draws_nothing",
            "a historical name, replaced on 2026-10-19 by \
             `a_bel_inside_a_sixel_rings_and_draws_nothing` when a BEL inside a Sixel began \
             to ring; note AN cites the old name where the BEL \
             was first put to the Sixel",
        ),
        (
            "the_extension_chooses_the_reader",
            "a historical name, replaced on 2026-10-17 by \
             `the_content_chooses_the_reader_and_the_name_only_cross_checks` when \
             a theme file's content began choosing its reader; note AR and its \
             survey of baud name the test the design replaces",
        ),
        (
            "a_body_too_large_for_the_scratch_is_refused_whole",
            "a historical name, withdrawn on 2026-10-14 with the body scratch it ran \
             a Sixel past: a Sixel decodes as it arrives since. It was \
             named among the tests that went with the body refusal",
        ),
        (
            "a_body_overflow_and_an_arena_overflow_are_told_apart",
            "a historical name, withdrawn on 2026-10-14 with the body refusal it told \
             apart from the arena's: a Sixel decodes as it arrives since. It was \
             cited where the refusals by cause were described",
        ),
        (
            "a_body_of_exactly_the_default_scratch_stores",
            "a historical name. The test pinned the 3 MiB body scratch of the \
             interim and was withdrawn on 2026-10-14, the day it landed, when a Sixel \
             began decoding as it arrived; `a_body_of_any_length_stores_at_the_default_budget` \
             replaced it. Note S cites it where it describes the interim",
        ),
        (
            "a_body_past_the_default_scratch_is_said_with_its_length",
            "a historical name, withdrawn with `a_body_of_exactly_the_default_scratch_stores` \
             and replaced by the same test: no body is refused since. Note S \
             cites it where it describes the interim",
        ),
        (
            "the_palette_swaps_black_and_white_and_nothing_else",
            "a historical name. The test pinned xterm's palette swap under \
             `?5` and was withdrawn on 2026-10-14, when the per-cell exchange \
             chosen on 2026-10-13 was built; `every_cell_exchanges_its_colours_explicit_ones_included` \
             replaced it. Note Y cites it where it describes \
             what it pinned then",
        ),
        (
            "an_indexed_underline_colour_swaps_with_the_table",
            "a historical name, withdrawn with xterm's palette swap on \
             2026-10-14; `an_underline_follows_the_text_and_an_sgr_58_colour_stays` \
             replaced it. It was cited where the swap was described",
        ),
        (
            "a_palette_override_travels_with_its_entry",
            "a historical name, withdrawn with xterm's palette swap on \
             2026-10-14; `a_palette_override_is_drawn_where_its_entry_is_named` \
             replaced it. Note V cites it where it describes \
             the swap",
        ),
        (
            "_vte_row_data_get",
            "VTE's C accessor, quoted in note F where it returns NULL past the              row's stored length",
        ),
        (
            "term_cursor_blink_update",
            "foot's C function that arms its cursor blink timer only while the \
             window has focus, quoted in `baud-app/src/caret.rs` \
             as the rule baud's blink follows",
        ),
        (
            "n_tty_receive_buf_common",
            "the Linux N_TTY function that keeps one byte of the read ring free,              quoted in note E and in `SLOT_BYTES`'s doc comment as the line the              4095-byte read bound comes from",
        ),
        (
            "_mm256_movemask_epi8",
            "an x86 intrinsic, named where the AVX2 arm's binary is checked for its              instruction rather than trusting the AVX2 property's output match",
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
            "zwp_linux_buffer_params_v1",
            "a Wayland interface name, the dmabuf parameters object a buffer is made through",
        ),
        (
            "wp_linux_drm_syncobj_manager_v1",
            "a Wayland interface name, the explicit synchronisation manager a window binds",
        ),
        (
            "zwp_linux_buffer_params",
            "a Wayland interface name, cited in note J for the dmabuf swapchain              a pure-protocol client has to build itself",
        ),
        (
            "grid_resize_and_reflow",
            "foot's re-cut, cited in note U's table of where each terminal              re-cuts",
        ),
        (
            "grid_resize_without_reflow",
            "foot's truncating resize for the alternate screen, cited in note U",
        ),
        (
            "stable_row_index_offset",
            "wezterm's field that a rewrap never adjusts, cited in note U as why              its viewport goes stale",
        ),
        (
            "scroll_up_within_margins",
            "wezterm's method, cited where a region narrower than the screen keeps nothing in the history",
        ),
        (
            "selection_on_top_region",
            "foot's check, cited where a selection on the rows a partial scroll left is cancelled",
        ),
        (
            "selection_on_bottom_region",
            "foot's check, cited where a selection on the rows a partial scroll left is cancelled",
        ),
        (
            "_sfte_render_trail",
            "sfte's C function that draws its cursor trail, cited where the sweep trail is its rule",
        ),
        (
            "__builtin_os_log_format",
            "clang's builtin that encodes an os_log buffer, cited in note AI as what Rust has no counterpart for",
        ),
        (
            "__dso_handle",
            "the linker's image-handle symbol os_log's macros pass, cited in note AI's list of what a non-C caller must declare",
        ),
        (
            "__os_log_default",
            "the default os_log object's symbol (OS_LOG_DEFAULT), cited in note AI",
        ),
        (
            "__os_log_impl",
            "the Mach-O spelling of `_os_log_impl` in an object's undefined symbols, cited in note AI's shim listing",
        ),
        (
            "_os_log_create",
            "the Mach-O spelling of `os_log_create`, cited in note AI's symbol listing",
        ),
        (
            "_os_log_impl",
            "os/log.h's \"Internal function that takes compiler generated encoding\", cited in note AI",
        ),
        (
            "_os_log_type_enabled",
            "the Mach-O spelling of `os_log_type_enabled`, cited in note AI's shim listing",
        ),
        (
            "os_log_type_enabled",
            "os/log.h's level check, cited in note AI",
        ),
        ("os_log_type_t", "os/log.h's level type, cited in note AI"),
        (
            "os_log_with_type",
            "os/log.h's logging macro, cited in note AI as the call the shim makes",
        ),
        (
            "get_tcp_inp_list",
            "XNU's TCP pcb walk (tcp_subr.c), cited in note AH where the socket dump's filter is sought",
        ),
        (
            "led_blink_set_oneshot",
            "the Linux LED core's one-shot blink, cited in note AH for the tty trigger's timing",
        ),
        (
            "led_stop_software_blink",
            "the Linux LED core call the netdev trigger makes first and the tty trigger does not, cited in note AH",
        ),
        (
            "master_pty_ops_bsd",
            "one of Linux pty.c's four ops tables, cited in note AH as lacking get_icount and tiocmget",
        ),
        (
            "ri_child_system_time",
            "a field of macOS's rusage_info (sys/resource.h), cited in note AH for reaped children's CPU",
        ),
        (
            "ri_child_user_time",
            "a field of macOS's rusage_info (sys/resource.h), cited in note AH for reaped children's CPU",
        ),
        (
            "sb_update_data_stats",
            "XNU's socket-buffer byte accounting (uipc_socket2.c), cited in note AH",
        ),
        (
            "slave_pty_ops_bsd",
            "one of Linux pty.c's four ops tables, cited in note AH as lacking get_icount and tiocmget",
        ),
        (
            "so_update_tx_data_stats",
            "XNU's per-socket send accounting, cited in note AH",
        ),
        (
            "should_show_zero_state_block",
            "Warp's gate on its zero-state block (AI enabled), cited in note AJ",
        ),
        (
            "show_changelog_after_update",
            "Warp's setting, cited in note AJ: \"Whether the changelog is shown after an update\"",
        ),
        (
            "show_terminal_zero_state_block",
            "Warp's setting for its per-session logo block, cited in note AJ",
        ),
        (
            "hb_ensure_native_direction",
            "HarfBuzz's run-direction rule (hb-ot-shape.cc), cited for baud-font's RunText",
        ),
        (
            "hb_script_get_horizontal_direction",
            "HarfBuzz's per-script direction list (hb-common.cc), cited for Script::direction",
        ),
        (
            "grman_put_cell_image",
            "kitty's graphics.c, how a placeholder run draws its virtual placement, cited in kitty.rs",
        ),
        (
            "screen_render_line_graphics",
            "kitty's screen.c, how a line's placeholders become runs, cited in placeholder.rs",
        ),
        (
            "cell_has_default_bg",
            "kitty's background.slang, which cells count as default under a deep image, cited in color.rs",
        ),
        (
            "a_surface_moved_past_half_its_ring_reaches_the_watch_on_the_alternate_screen",
            "a historical name, replaced on 2026-10-17 by \
             `a_surface_moved_past_half_its_ring_is_counted_on_the_alternate_screen` \
             when the watch began numbering by its count of lines and no longer \
             reached a selection by the distance alone; it was cited where \
             the latch was described",
        ),
        (
            "a_surface_moved_past_half_its_ring_reaches_the_watch_on_the_primary",
            "a historical name, replaced on 2026-10-17 by \
             `a_surface_moved_past_half_its_ring_is_counted_on_the_primary`, for \
             the reason its alternate-screen twin was; it was cited where \
             the latch was described",
        ),
        (
            "selective_erase_matches_plain_erase_when_nothing_is_protected",
            "a historical name, replaced on 2026-10-19 by \
             `selective_erase_writes_spaces_where_plain_erase_empties` when \
             DECSED and DECSEL began writing spaces, as DEC STD 070 says; it \
             was cited where the marked cell's extent was described",
        ),
        (
            "a_copying_scroll_down_reaches_the_regions_rows_and_only_those",
            "a historical name, renamed on 2026-10-20 to `a_copying_scroll_down_carries_the_regions_rows_and_only_those` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "a_copying_scroll_up_reaches_the_regions_rows_and_only_those",
            "a historical name, renamed on 2026-10-20 to `a_copying_scroll_up_carries_the_regions_rows_and_only_those` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "il_reaches_the_rows_from_the_cursor_down",
            "a historical name, renamed on 2026-10-20 to `il_carries_the_rows_from_the_cursor_down` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "dl_reaches_the_rows_from_the_cursor_to_the_regions_bottom",
            "a historical name, renamed on 2026-10-20 to `dl_carries_the_rows_from_the_cursor_to_the_regions_bottom` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "a_top_regions_scroll_reaches_the_rows_under_it",
            "a historical name, renamed on 2026-10-20 to `a_top_regions_scroll_keeps_the_rows_under_it` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "a_top_regions_scroll_reaches_the_rows_under_it_on_the_alternate_screen",
            "a historical name, renamed on 2026-10-20 to `a_top_regions_scroll_keeps_the_rows_under_it_on_the_alternate_screen` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "a_region_retire_drops_a_selection_only_on_the_rows_under_it",
            "a historical name, renamed on 2026-10-20 to `a_region_retire_keeps_a_selection_on_the_rows_under_it` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "a_region_rotation_on_the_alternate_screen_drops_a_selection_under_it",
            "a historical name, renamed on 2026-10-20 to `a_region_rotation_on_the_alternate_screen_keeps_a_selection_under_it` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "a_region_set_after_the_copy_still_drops_a_selection_it_moved",
            "a historical name, renamed on 2026-10-20 to `a_region_set_after_the_copy_still_carries_a_selection_it_moved` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "a_region_set_and_set_back_within_one_read_is_dropped_as_in_separate_reads",
            "a historical name, renamed on 2026-10-20 to `a_region_set_and_set_back_within_one_read_is_judged_as_in_separate_reads` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "a_top_regions_scroll_drops_a_selection_under_it_at_every_read_size",
            "a historical name, renamed on 2026-10-20 to `a_top_regions_scroll_keeps_a_selection_under_it_at_every_read_size` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "rows_moved_in_place_drop_a_selection_on_them_and_only_there",
            "a historical name, renamed on 2026-10-20 to `rows_moved_in_place_carry_a_selection_with_their_text` when a \
             scroll began to carry a selection with its text that day; \
             the old name was cited where it pinned the drop",
        ),
        (
            "pinned_a_cut_row_of_invisible_blanks_is_revealed_as_content",
            "a historical name, replaced on 2026-10-20 by \
             `a_cut_row_of_foreground_only_blanks_is_not_content` when a growth \
             began to ask a cut row for a written character (note V, V9); note \
             V cites the old name where it pins the mark's rule",
        ),
        (
            "pinned_a_cut_row_uniform_in_a_visible_colour_is_left_cut",
            "a historical name, renamed on 2026-10-20 to \
             `a_cut_row_in_a_colour_alone_is_not_content`, for the same change",
        ),
        (
            "pinned_a_band_past_the_old_edge_goes_with_the_erase_of_its_row",
            "a historical name, renamed on 2026-10-20 to \
             `a_band_past_the_old_edge_is_not_content_after_a_width_growth`, for \
             the same change",
        ),
        (
            "the_cpu_path_draws_an_effect_as_its_emitter",
            "a historical name, replaced on 2026-10-23 by \
             `the_cpu_path_draws_every_effect_by_its_light` when the CPU path \
             began to draw every effect; note X cites it where \
             the CPU path drew an effect as its quad",
        ),
        (
            "an_effect_on_the_cpu_path_runs_as_the_quad",
            "a historical name, deleted on 2026-10-23 with the CPU path's quad \
             for an effect; it was cited where the driver ran one so",
        ),
        (
            "a_bel_before_a_dcs_body_is_ignored",
            "a historical name, renamed on 2026-10-23 to \
             `a_bel_before_a_dcs_body_rings_and_the_introducer_goes_on` when a \
             C0 in a DCS introducer began to be executed; the old \
             name was cited where it pinned the BEL",
        ),
        (
            "a_mark_finds_its_base_after_the_cursor_has_been_away",
            "a historical name, renamed on 2026-10-23 to \
             `a_mark_after_the_cursor_has_been_away_takes_its_own_cell` when a \
             mark after a control began to take its own cell (UAX #29 GB4/GB5); \
             the old name was cited where finding the base was described",
        ),
        (
            "alternate_scroll_survives_both_resets",
            "a historical name, renamed on 2026-10-24 to \
             `a_full_reset_sets_alternate_scroll_and_a_soft_one_keeps_it` when \
             `RIS` began to set `?1007` again; the old name was cited \
             where it pinned `?1007` kept by both resets",
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

// A test once held the contributor guide's "Unmeasured" and "Things already
// tried that lost" sections to the coverage table's done rows, so nothing
// listed as absent was also marked done. Those sections were the guide's
// alone, and the guide is not in this tree: `docs/vt-coverage.md` and
// `README.md` carry neither heading, so the check has nothing left to read.

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
    let text = fs::read_to_string(root().join("MEASUREMENTS.md")).expect("MEASUREMENTS.md");

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
    let text = fs::read_to_string(root().join("MEASUREMENTS.md")).expect("MEASUREMENTS.md");

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

/// **Every group the source manifest files a source under is in
/// `docs/README.md`'s index.**
///
/// The index listed four groups missing for weeks after their sources were
/// filed: a list written once by hand, of a manifest every research session
/// adds to, and nothing compared the two. The control is the count: a
/// scanner that stopped matching would find no group, and pass. This test
/// also held the index to a directory of research notes, and those are not
/// in this tree, so the manifest's groups are what is left to hold.
#[test]
fn every_source_group_is_in_the_index() {
    let root = root();
    let index = fs::read_to_string(root.join("docs/README.md")).expect("docs/README.md");

    let manifest = fs::read_to_string(root.join("docs/research/sources/MANIFEST.toml"))
        .expect("MANIFEST.toml");
    let mut groups = BTreeSet::new();
    for line in manifest.lines() {
        let Some(rest) = line.strip_prefix("group") else {
            continue;
        };
        let Some(v) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        groups.insert(v.trim().trim_matches('"').to_string());
    }
    assert!(
        groups.len() > 10,
        "only {} manifest groups found -- the scanner stopped matching",
        groups.len()
    );
    let ungrouped: Vec<_> = groups
        .iter()
        .filter(|g| !index.contains(&format!("\n| {g} |")))
        .collect();

    assert!(
        ungrouped.is_empty(),
        "docs/README.md's research index is behind: manifest groups with no row: {ungrouped:?}"
    );
}

/// The lines of `text` where an attribute other than `cfg` or `cfg_attr` is
/// followed directly by a `///` doc comment, 1-based, at the attribute's
/// first line. A multi-line attribute is followed to the line that closes it.
fn attributes_above_docs(text: &str) -> Vec<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim_start();
        if t.starts_with("#[") && !t.starts_with("#[cfg(") && !t.starts_with("#[cfg_attr(") {
            let start = i;
            let mut depth = 0i64;
            loop {
                let l = lines[i];
                depth += l.matches('[').count() as i64 - l.matches(']').count() as i64;
                if depth <= 0 || i + 1 >= lines.len() {
                    break;
                }
                i += 1;
            }
            let next = lines.get(i + 1).map(|l| l.trim_start()).unwrap_or("");
            if next.starts_with("///") && !next.starts_with("////") {
                out.push(start + 1);
            }
        }
        i += 1;
    }
    out
}

/// **No attribute sits above a doc comment**, because that is what an
/// insertion between a doc and its item leaves when the item had an
/// attribute: `A`'s doc, `A`'s attribute, then the new item's doc and the
/// new item, which now wears `A`'s attribute while `A` has neither.
/// It happened here: `#[inline(never)]` and the comment explaining it
/// landed on the function below the one they were written for; rustc
/// cannot see it, since a doc comment is an attribute and attributes stack
/// in any order.
///
/// `cfg` and `cfg_attr` are exempt: `#[cfg(..)]` above an item's doc is a
/// common and legal order (`baud-app/src/pty.rs` has three), and a stranded
/// `cfg` would fail to compile wherever the item it moved off is used.
///
/// **What this cannot see** is a doc stranded with no attribute: `A`'s doc
/// sitting above `B`'s doc is one long doc on `B` to an attribute check.
/// `no_doc_is_stranded_on_the_item_before_its_own` is the check for that kind.
#[test]
fn no_attribute_is_stranded_above_a_doc_comment() {
    // Positive control: the stranded shape, and two orders that are not.
    let stranded = "    /// A's doc.\n    #[inline(always)]\n    /// B's doc.\n    fn b() {}\n";
    assert_eq!(attributes_above_docs(stranded), [2], "the control");
    let multi = "#[derive(\n    Clone,\n)]\n/// Doc.\nstruct S;\n";
    assert_eq!(attributes_above_docs(multi), [1], "a multi-line attribute");
    let fine = "#[cfg(test)]\n/// Doc.\nfn a() {}\n/// Doc.\n#[inline]\nfn b() {}\n";
    assert!(
        attributes_above_docs(fine).is_empty(),
        "cfg first, doc first"
    );

    let mut found = Vec::new();
    let files: Vec<_> = doc_and_code_files()
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    assert!(files.len() > 200, "only {} Rust files walked", files.len());
    for p in files {
        let text = fs::read_to_string(&p).expect("readable");
        for n in attributes_above_docs(&text) {
            found.push(format!("{}:{n}", rel(&p)));
        }
    }
    assert!(
        found.is_empty(),
        "an attribute above a doc comment -- the doc's item lost it, or it \
         belongs to the item above:\n  {}",
        found.join("\n  ")
    );
}

/// The longest first paragraph a stranded doc has been seen to carry before
/// the doc of the item it sits on: `paint.rs`'s `fast_try`, three lines over
/// `scroll_rot`'s. One more for margin; longer reaches shared docs -- a field
/// group's, `term.rs`'s `defaults`/`dynamic` -- that are no stranding.
const SUMMARY_LINES: usize = 4;

/// The item a line declares, as `(indent, name)`: a `fn`, type, `const`,
/// `static`, `trait`, `mod`, or -- indented -- a field (`name: Type,`) or an
/// enum variant. `impl`, `use` and macros are not items here: they never wear
/// a doc, and an `impl` block between two items must not hide one from the
/// other (`style.rs`'s `impl Chan` sits between `Chan` and `Color`).
fn declared_item(line: &str) -> Option<(usize, &str)> {
    const KW: [&str; 9] = [
        "fn", "struct", "enum", "union", "trait", "type", "const", "static", "mod",
    ];
    let t = line.trim_start();
    let indent = line.len() - t.len();
    let toks: Vec<&str> = t.split_whitespace().collect();
    let mut i = 0;
    while let Some(&w) = toks.get(i) {
        let qualifier = matches!(w, "pub" | "async" | "unsafe" | "extern" | "\"C\"")
            || w.starts_with("pub(")
            || (w == "const"
                && toks
                    .get(i + 1)
                    .is_some_and(|n| matches!(*n, "fn" | "unsafe" | "async" | "extern")));
        if !qualifier {
            break;
        }
        i += 1;
    }
    let ident = |s: &str| -> usize {
        s.char_indices()
            .find(|&(_, c)| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(s.len(), |(n, _)| n)
    };
    if toks.get(i).is_some_and(|w| KW.contains(w)) {
        let next = toks.get(i + 1)?;
        let n = ident(next);
        return Some((indent, if n == 0 { "_" } else { &next[..n] }));
    }
    if indent == 0 {
        return None;
    }
    let end = t.trim_end();
    // A field: optional visibility, a lower-case name, `:` but not `::`, and
    // the line ends the field.
    let body = match t.strip_prefix("pub") {
        Some(r) if r.starts_with('(') => r.split_once(')').map_or(t, |(_, r)| r.trim_start()),
        Some(r) if r.starts_with(' ') => r.trim_start(),
        _ => t,
    };
    let n = ident(body);
    let name = &body[..n];
    let after = body[n..].trim_start();
    if n > 0
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
        && after.starts_with(':')
        && !after.starts_with("::")
        && end.ends_with(',')
    {
        return Some((indent, name));
    }
    // A variant: a capitalised name, then `,`, `(`, `{`, `=` or the end.
    let n = ident(t);
    let name = &t[..n];
    let after = t[n..].trim_start();
    if name.starts_with(|c: char| c.is_ascii_uppercase())
        && (after.is_empty() || after.starts_with([',', '(', '{', '=']))
        && (end.ends_with([',', '{', '(']) || end == name)
    {
        return Some((indent, name));
    }
    None
}

/// Where a doc's first paragraph has a sentence end followed, on the next
/// line, by a capitalised sentence, within [`SUMMARY_LINES`]: the seam an
/// insertion leaves between `A`'s doc and `B`'s. `None` once the paragraph ends.
fn summary_seam(doc: &[&str]) -> Option<usize> {
    for k in 1..doc.len().min(SUMMARY_LINES + 1) {
        let (a, b) = (doc[k - 1].trim(), doc[k].trim());
        if a.is_empty() || b.is_empty() {
            return None;
        }
        if a.starts_with(['*', '-', '|', '#']) || a.starts_with("```") {
            continue;
        }
        if a.ends_with('.') && b.starts_with(|c: char| c.is_ascii_uppercase()) {
            return Some(k);
        }
    }
    None
}

/// The lines of `text`, 1-based, of an item with no doc whose sibling just
/// before it -- same indent, same block -- has a doc with a [`summary_seam`].
/// A `#[test]` on either side, `main`, `mod tests` and `const _` are left out:
/// none of them is an item a doc is written for.
fn undocumented_after_a_seamed_doc(text: &str) -> Vec<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    // The doc of the last item at each indent; a test's counts as none.
    let mut prev: std::collections::BTreeMap<usize, Vec<&str>> = Default::default();
    let (mut doc, mut test) = (Vec::new(), false);
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        if t.starts_with("///") && !t.starts_with("////") {
            doc.push(&t[3..]);
        } else if t.starts_with("#[") {
            test |= t.starts_with("#[test");
            let mut depth = 0i64;
            loop {
                let l = lines[i];
                depth += l.matches('[').count() as i64 - l.matches(']').count() as i64;
                if depth <= 0 || i + 1 >= lines.len() {
                    break;
                }
                i += 1;
            }
        } else if !t.is_empty() && !t.starts_with("//") {
            let indent = lines[i].len() - lines[i].trim_start().len();
            prev.retain(|&k, _| k <= indent);
            if let Some((_, name)) = declared_item(lines[i]) {
                if let Some(p) = prev.get(&indent) {
                    if doc.is_empty()
                        && !test
                        && !matches!(name, "main" | "tests" | "_")
                        && summary_seam(p).is_some()
                    {
                        out.push(i + 1);
                    }
                }
                let kept = if test {
                    Vec::new()
                } else {
                    std::mem::take(&mut doc)
                };
                prev.insert(indent, kept);
            }
            doc.clear();
            test = false;
        }
        i += 1;
    }
    out
}

/// **No doc is stranded on the item before its own**, the kind with no
/// attribute: an insertion between `A`'s doc and `A` leaves `A`'s doc on top of
/// the new item's, so the new item reads `A`'s summary and `A` has none.
/// `no_attribute_is_stranded_above_a_doc_comment` sees that only when `A`
/// had an attribute.
///
/// The check reads both halves of the shape: an item with no doc right after a
/// sibling whose doc's first paragraph changes sentence at a line end, within
/// [`SUMMARY_LINES`]. The two docs were each written to stand alone, so the
/// join reads as two summaries -- `style.rs`'s `as_rgb` doc on `as_index`
/// ("... on their own." / "The palette index ..."), and `Color`'s on `Chan`,
/// `LineIdx`'s on `ByteGap`, `csi_param`'s on `csi_entry`, `fast_try`'s on
/// `scroll_rot` and a test's `dynamic` on `Rgb`: the six a reading pass over
/// the workspace missed, all of which this flags. On `4b703f8`, before that
/// pass, it flags fourteen of the forty-nine the pass repaired as well.
///
/// **A doc of the item's own that trips it** separates its summary with a
/// blank `///`, which is also what makes rustdoc's summary line the summary;
/// eleven took one when this landed. **What it cannot see** is a doc whose
/// item was inserted with no doc of its own -- `A`'s doc whole on the new
/// item, no seam -- and a stranding whose `A` later got a doc again.
#[test]
fn no_doc_is_stranded_on_the_item_before_its_own() {
    // Positive controls: the stranded shapes, and the orders that are not.
    let stranded = "impl W {\n    /// The RGB triple.\n    /// The palette index.\n    \
                    pub fn as_index(self) {}\n    pub fn as_rgb(self) {}\n}\n";
    assert_eq!(
        undocumented_after_a_seamed_doc(stranded),
        [5],
        "the control"
    );
    let across_impl = "/// SGR colour.\n/// A channel.\npub struct Chan(u8);\n\n\
                       impl Chan {}\n\n#[derive(Clone)]\npub enum Color {\n    Default,\n}\n";
    assert_eq!(
        undocumented_after_a_seamed_doc(across_impl),
        [8],
        "an impl between the two"
    );
    let field = "struct P {\n    /// Tries, and hits.\n    /// Rotations.\n    \
                 #[cfg(x)]\n    rot: usize,\n    #[cfg(x)]\n    copy: usize,\n}\n";
    assert_eq!(undocumented_after_a_seamed_doc(field), [7], "a field");
    let fine = "/// A summary.\n///\n/// More.\nfn a() {}\nfn b() {}\n\
                /// One doc. Two sentences.\nfn c() {}\n/// Own doc.\nfn d() {}\n\
                /// A test's doc.\n/// Second.\n#[test]\nfn e() {}\nfn f() {}\n";
    assert!(
        undocumented_after_a_seamed_doc(fine).is_empty(),
        "a paragraph break, one line, both documented, after a test: {:?}",
        undocumented_after_a_seamed_doc(fine)
    );

    let mut found = Vec::new();
    let files: Vec<_> = doc_and_code_files()
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    assert!(files.len() > 200, "only {} Rust files walked", files.len());
    for p in files {
        let text = fs::read_to_string(&p).expect("readable");
        for n in undocumented_after_a_seamed_doc(&text) {
            found.push(format!("{}:{n}", rel(&p)));
        }
    }
    assert!(
        found.is_empty(),
        "an item with no doc after a sibling whose doc's first paragraph \
         reads as two -- the first is probably this item's, stranded by an \
         insertion; if the doc is the sibling's own, put a blank `///` after \
         its summary:\n  {}",
        found.join("\n  ")
    );
}
