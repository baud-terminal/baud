//! Every check of `cargo xtask features --check` fires, on a small tree
//! it passes on, and the page it writes is the page it checks.

mod common;

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::Tree;
use xtask::features::{self, Mode};
use xtask::third_party::Outcome;

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub const F: &str = "docs/data/features.toml";
pub const PAGE: &str = "docs/guide/features.md";

pub const FEATURES: &str = r#"intro = """
What it does.
"""
legend = "No mark: everywhere."
protocols_intro = "The protocols."
footer = "Generated."

[[area]]
id = "text"
title = "Text and `fonts`"
intro = "How text looks."

[[area]]
id = "input"
title = "Input"

[[area]]
id = "spare"
title = "Spare"

[[feature]]
id = "wrap"
area = "text"
name = "Wrapping"
does = "Wraps long lines."
status = "built"
platform = "both"
tests = ["core/tests/wrap.rs::wraps_a_line"]
rows = ["vt:DECAWM"]
measured = ["Wrapping, measured"]
chips = ["VT100"]

[[feature]]
id = "keys"
area = "input"
name = "Keys"
does = "Sends the keys."
status = "partial"
gap = "Some keys."
platform = "macos"
paths = ["core/src/keys.rs"]
rows = ["input:F1"]

[[feature]]
id = "paste"
area = "input"
name = "Paste"
does = "Pastes text."
status = "interim"
gap = "A stopgap."
platform = "linux"
tests = ["core/tests/wrap.rs::pastes"]
rows = ["vt:DECRC"]

[[protocol]]
name = "VT100"
row = "vt:DECAWM"
"#;

pub const VT: &str = "# VT coverage\n\n## Tier 1 — the core\n\n\
| op | status | note |\n|---|---|---|\n| DECAWM | **done** | wraps |\n| DECOM | partial | origin |\n\
| DECSC | | unmarked |\n\n## Tier 1 — the core\n\n| op | status |\n|---|---|\n| DECRC | done |\n";

pub const INPUT: &str = "# Input\n\n### Keys\n\n| key | status |\n|---|---|\n| F1 | done |\n";

pub const MEASURE: &str = "# Measurements\n\n## Wrapping, measured\n\nText.\n";

/// A tree every check passes on, its page written by the generator.
pub fn base() -> Tree {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("xtask-features-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch directory");
    let t = Tree(dir);
    t.put(F, FEATURES);
    t.put("docs/vt-coverage.md", VT);
    t.put("docs/input-coverage.md", INPUT);
    t.put("MEASUREMENTS.md", MEASURE);
    t.put("README.md", "baud. See all 3 features.\n");
    t.put(
        "core/tests/wrap.rs",
        "#[test]\nfn wraps_a_line() {}\n\nfn pastes() {}\n",
    );
    t.put("core/src/keys.rs", "// the keys\n");
    fs::create_dir_all(t.path("docs/guide")).expect("a directory");
    let (o, out) = run(&t, Mode::Write);
    assert_eq!(o, Outcome::Passed, "{out}");
    t
}

/// The gate's verdict on `t`, and what it printed.
pub fn run(t: &Tree, mode: Mode) -> (Outcome, String) {
    let mut out = Vec::new();
    let o = features::run(&t.0, mode, &mut out).expect("a run");
    (o, String::from_utf8(out).expect("UTF-8"))
}

/// The opening line of a failure report.
pub const HEAD: &str = "problem(s) in docs/data/features.toml:\n";

/// One perturbation: what it does, and everything the gate then prints.
pub struct Case {
    pub name: &'static str,
    pub apply: fn(&Tree),
    pub expect: &'static str,
}

const WRAP_TESTS: &str = "tests = [\"core/tests/wrap.rs::wraps_a_line\"]";

pub fn cases() -> Vec<Case> {
    vec![
        // 1. Every feature names its proof; a built one a test, a done row
        // or a measurement.
        Case {
            name: "1 a feature with no proof",
            apply: |t| {
                t.edit(
                    F,
                    "tests = [\"core/tests/wrap.rs::pastes\"]\nrows = [\"vt:DECRC\"]\n",
                    "",
                );
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  feature paste: names no proof\n",
        },
        Case {
            name: "1 a built feature with only a path",
            apply: |t| {
                t.edit(
                    F,
                    &format!(
                        "{WRAP_TESTS}\nrows = [\"vt:DECAWM\"]\nmeasured = [\"Wrapping, measured\"]"
                    ),
                    "paths = [\"core/src/keys.rs\"]",
                );
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  feature wrap: built, and names no test, no done row and no measurement\n",
        },
        Case {
            name: "1 a field left empty",
            apply: |t| t.edit(F, "does = \"Wraps long lines.\"", "does = \"\""),
            expect: "1 problem(s) in docs/data/features.toml:\n  feature wrap: no does\n",
        },
        Case {
            name: "1 an id used twice",
            apply: |t| t.edit(F, "id = \"paste\"", "id = \"keys\""),
            expect: "1 problem(s) in docs/data/features.toml:\n  feature keys: id used twice\n",
        },
        Case {
            name: "1 an area not defined",
            apply: |t| {
                t.edit(
                    F,
                    "area = \"text\"\nname = \"Wrapping\"",
                    "area = \"sound\"\nname = \"Wrapping\"",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  feature wrap: area sound is not defined\n",
        },
        Case {
            name: "1 a status not known",
            apply: |t| t.edit(F, "status = \"interim\"", "status = \"soon\""),
            expect: "1 problem(s) in docs/data/features.toml:\n  feature paste: status soon is not one of ('built', 'partial', 'interim')\n",
        },
        Case {
            name: "1 a platform not known",
            apply: |t| t.edit(F, "platform = \"linux\"", "platform = \"bsd\""),
            expect: "1 problem(s) in docs/data/features.toml:\n  feature paste: platform bsd is not one of ('both', 'macos', 'linux')\n",
        },
        Case {
            name: "1 a partial feature with no gap",
            apply: |t| t.edit(F, "gap = \"Some keys.\"\n", ""),
            expect: "1 problem(s) in docs/data/features.toml:\n  feature keys: a partial feature says what is missing in `gap`\n",
        },
        Case {
            name: "1 a built feature with a gap",
            apply: |t| {
                t.edit(
                    F,
                    "platform = \"both\"",
                    "platform = \"both\"\ngap = \"None.\"",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  feature wrap: a built feature has no gap\n",
        },
        // 2. Every proof path exists, and every measurement is a heading.
        Case {
            name: "2 a proof path gone",
            apply: |t| t.remove("core/src/keys.rs"),
            expect: "1 problem(s) in docs/data/features.toml:\n  feature keys: proof path core/src/keys.rs does not exist\n",
        },
        Case {
            name: "2 a measurement that is not a heading",
            apply: |t| {
                t.edit(
                    "MEASUREMENTS.md",
                    "## Wrapping, measured",
                    "## Wrapping, timed",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  feature wrap: MEASUREMENTS.md has no heading 'Wrapping, measured'\n",
        },
        // 3. Every proof test is a fn in the file it names.
        Case {
            name: "3 a test renamed",
            apply: |t| t.edit("core/tests/wrap.rs", "fn wraps_a_line", "fn wraps_lines"),
            expect: "1 problem(s) in docs/data/features.toml:\n  feature wrap: core/tests/wrap.rs has no fn wraps_a_line\n",
        },
        Case {
            name: "3 a test's file gone",
            apply: |t| {
                t.edit(
                    F,
                    WRAP_TESTS,
                    "tests = [\"core/tests/gone.rs::wraps_a_line\"]",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  feature wrap: test file core/tests/gone.rs does not exist\n",
        },
        Case {
            name: "3 a test not written path::fn",
            apply: |t| {
                t.edit(
                    F,
                    WRAP_TESTS,
                    "tests = [\"core/tests/wrap.rs:wraps_a_line\"]",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  feature wrap: test core/tests/wrap.rs:wraps_a_line is not written <path>::<fn>\n",
        },
        Case {
            name: "3 a fn only after a word character",
            apply: |t| t.edit("core/tests/wrap.rs", "fn pastes", "xfn pastes"),
            expect: "1 problem(s) in docs/data/features.toml:\n  feature paste: core/tests/wrap.rs has no fn pastes\n",
        },
        // 4. Every cited row is a row, and a built feature's rows are done.
        Case {
            name: "4 a row not in the table",
            apply: |t| t.edit(F, "rows = [\"input:F1\"]", "rows = [\"input:F2\"]"),
            expect: "1 problem(s) in docs/data/features.toml:\n  feature keys: no input coverage row F2\n",
        },
        Case {
            name: "4 a built feature's row partial",
            apply: |t| {
                t.edit(
                    F,
                    "rows = [\"vt:DECAWM\"]\nmeasured",
                    "rows = [\"vt:DECAWM\", \"vt:DECOM\"]\nmeasured",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  feature wrap: built, but its row DECOM is partial\n",
        },
        Case {
            name: "4 a built feature's row unmarked",
            apply: |t| {
                t.edit(
                    F,
                    "rows = [\"vt:DECAWM\"]\nmeasured",
                    "rows = [\"vt:DECSC\"]\nmeasured",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  feature wrap: built, but its row DECSC is unmarked\n",
        },
        Case {
            name: "4 a row two rows begin",
            apply: |t| {
                t.edit(
                    "docs/vt-coverage.md",
                    "| DECRC | done |",
                    "| DECAWM | done |",
                )
            },
            expect: "2 problem(s) in docs/data/features.toml:\n  feature wrap: two vt rows begin DECAWM, so the citation is ambiguous\n  feature paste: no vt coverage row DECRC\n",
        },
        Case {
            name: "4 a protocol's row not in the table",
            apply: |t| t.edit(F, "row = \"vt:DECAWM\"", "row = \"vt:DECAWX\""),
            expect: "1 problem(s) in docs/data/features.toml:\n  protocol VT100: no vt row DECAWX\n",
        },
        // 5. Every chip is a defined protocol.
        Case {
            name: "5 a chip not defined",
            apply: |t| t.edit(F, "chips = [\"VT100\"]", "chips = [\"VT100\", \"VT52\"]"),
            expect: "1 problem(s) in docs/data/features.toml:\n  feature wrap: chip VT52 is not a defined protocol\n",
        },
        // 6. Every field is one the gate reads.
        Case {
            name: "6 a top-level field nothing reads",
            apply: |t| {
                t.edit(
                    F,
                    "footer = \"Generated.\"",
                    "footer = \"Generated.\"\nbehind_intro = \"Elsewhere.\"",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  docs/data/features.toml: behind_intro is not a field of the list\n",
        },
        Case {
            name: "6 an area field nothing reads",
            apply: |t| {
                t.edit(
                    F,
                    "intro = \"How text looks.\"",
                    "intro = \"How text looks.\"\ncolour = \"red\"",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  area text: colour is not a field of the list\n",
        },
        Case {
            name: "6 a feature field nothing reads",
            apply: |t| {
                t.edit(
                    F,
                    "rows = [\"input:F1\"]",
                    "rows = [\"input:F1\"]\nroadmap = [\"R-2\"]",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  feature keys: roadmap is not a field of the list\n",
        },
        Case {
            name: "6 a protocol field nothing reads",
            apply: |t| {
                t.edit(
                    F,
                    "row = \"vt:DECAWM\"",
                    "origin = \"DEC\"\nrow = \"vt:DECAWM\"",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml:\n  protocol VT100: origin is not a field of the list\n",
        },
        // 7. README's count is the list's.
        Case {
            name: "7 README counts another number",
            apply: |t| t.put("README.md", "baud. See all 4 features.\n"),
            expect: "1 problem(s) in docs/data/features.toml:\n  README.md: says 'See all 4 features', the list has 3\n",
        },
        Case {
            name: "7 README with no count",
            apply: |t| t.put("README.md", "baud.\n"),
            expect: "1 problem(s) in docs/data/features.toml:\n  README.md: no 'See all N features' link, so its count is unchecked\n",
        },
        // 8. The page is what the generator writes.
        Case {
            name: "8 the page edited",
            apply: |t| t.edit(PAGE, "Wraps long lines.", "Wraps lines."),
            expect: "stale: docs/guide/features.md -- edit the data under docs/data/ and rerun cargo xtask features; a generated page is never edited by hand\n",
        },
        Case {
            name: "8 the page gone",
            apply: |t| t.remove(PAGE),
            expect: "stale: docs/guide/features.md -- edit the data under docs/data/ and rerun cargo xtask features; a generated page is never edited by hand\n",
        },
        Case {
            name: "8 the data edited and the page not",
            apply: |t| t.edit(F, "Sends the keys.", "Sends every key."),
            expect: "stale: docs/guide/features.md -- edit the data under docs/data/ and rerun cargo xtask features; a generated page is never edited by hand\n",
        },
    ]
}

/// The feature page the script wrote for the base tree.
const BASE_PAGE: &str = r#"# Features

What it does.

No mark: everywhere.

[Text and `fonts`](#text-and-fonts) · [Input](#input) · [Spare](#spare)

## Text and `fonts`

How text looks.

- **Wrapping** — Wraps long lines. <sub>[VT100](../vt-coverage.md#tier-1--the-core) · [proof](../../core/tests/wrap.rs) · [measured](../../MEASUREMENTS.md#wrapping-measured) · [reference](../vt-coverage.md#tier-1--the-core)</sub>

## Input

- **Keys** — Sends the keys. *macOS.* **Partly:** Some keys. <sub>[proof](../../core/src/keys.rs) · [reference](../input-coverage.md#keys)</sub>
- **Paste** — Pastes text. *Linux.* **Stopgap:** A stopgap. <sub>[proof](../../core/tests/wrap.rs) · [reference](../vt-coverage.md#tier-1--the-core-1)</sub>

## Spare


## Protocols

The protocols.

| protocol | reference |
|---|---|
| VT100 | [in the coverage table](../vt-coverage.md#tier-1--the-core) |

## How this page stays true

Generated.
"#;

#[test]
fn the_base_tree_passes() {
    let t = base();
    assert_eq!(run(&t, Mode::Check), (Outcome::Passed, String::new()));
}

#[test]
fn the_page_is_what_the_script_wrote() {
    let t = base();
    assert_eq!(t.read(PAGE), BASE_PAGE);
}

/// Each case prints what the script printed on it, line for line, but
/// for the command the stale line names.
#[test]
fn every_check_fires_and_prints_what_the_script_printed() {
    for case in cases() {
        let t = base();
        (case.apply)(&t);
        let (o, out) = run(&t, Mode::Check);
        assert_eq!(o, Outcome::Failed, "{}: the gate passed", case.name);
        assert_eq!(out, case.expect, "{}", case.name);
    }
}

/// The script read a page through Python's universal newlines, so a page
/// checked out with `\r\n` endings passed, and still does.
#[test]
fn a_page_with_crlf_endings_passes() {
    let t = base();
    let p = t.read(PAGE);
    t.put(PAGE, &p.replace('\n', "\r\n"));
    assert_eq!(run(&t, Mode::Check), (Outcome::Passed, String::new()));
}

#[test]
fn writing_refuses_a_tree_that_fails_and_leaves_the_page() {
    let t = base();
    t.edit(F, "does = \"Wraps long lines.\"", "does = \"\"");
    t.edit(PAGE, "Wraps long lines.", "Edited.");
    let (o, _) = run(&t, Mode::Write);
    assert_eq!(o, Outcome::Failed);
    assert!(t.read(PAGE).contains("Edited."));
}

#[test]
fn writing_rewrites_a_stale_page() {
    let t = base();
    t.edit(PAGE, "Wraps long lines.", "Edited.");
    assert_eq!(run(&t, Mode::Write).0, Outcome::Passed);
    assert_eq!(t.read(PAGE), BASE_PAGE);
}

#[test]
fn writing_restores_a_page_gone() {
    let t = base();
    t.remove(PAGE);
    assert_eq!(run(&t, Mode::Write).0, Outcome::Passed);
    assert_eq!(t.read(PAGE), BASE_PAGE);
}

/// Where the script stopped on an exception, the run fails with a line
/// naming it: here the `KeyError` of a feature with no `id`, after the
/// line the script never printed.
#[test]
fn a_feature_with_no_id_stops_on_its_key() {
    let t = base();
    t.edit(F, "id = \"paste\"\n", "");
    assert_eq!(
        run(&t, Mode::Check),
        (Outcome::Failed, "KeyError: 'id'\n".to_string())
    );
}

/// GitHub's anchors as the script made them: Python's `\w` on a `str`
/// is a letter, a number or `_`, so a superscript two and a Roman
/// numeral stay and a combining accent and an em dash go.
#[test]
fn a_slug_keeps_what_python_calls_a_word() {
    assert_eq!(features::slug(" Tier 1 — `the` Core "), "tier-1--the-core");
    assert_eq!(features::slug("x\u{b2} \u{216b}_e\u{301}!"), "x²-ⅻ_e");
}

/// `\bfn` needs a non-word character, in Python's sense, before it, and
/// the search goes on after each name it takes.
#[test]
fn a_fn_is_found_after_a_boundary() {
    let found = features::fns("fn a() {}\nxfn b\n\u{e9}fn c\n.fn d\nfn fn e\n");
    let mut v: Vec<&str> = found.iter().map(String::as_str).collect();
    v.sort_unstable();
    assert_eq!(v, ["a", "d", "fn"]);
}

/// The repository's own page and the data under it, which baud-core's
/// `feature_list` test checked through the script.
#[test]
fn the_repository_passes() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let mut out = Vec::new();
    let o = features::run(std::path::Path::new(root), Mode::Check, &mut out).expect("a run");
    assert_eq!(
        o,
        Outcome::Passed,
        "the feature list has drifted -- fix docs/data/ and run \
         cargo xtask features\n{}",
        String::from_utf8_lossy(&out)
    );
}
