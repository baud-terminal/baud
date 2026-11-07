//! Every check of `cargo xtask features --check` fires, on a small tree
//! it passes on, and the pages it writes are the pages it checks.

mod common;

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::Tree;
use xtask::features::{self, Mode};
use xtask::third_party::Outcome;

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub const F: &str = "docs/data/features.toml";
pub const R: &str = "docs/data/roadmap.toml";
pub const PAGE: &str = "docs/guide/features.md";
pub const ROAD: &str = "docs/roadmap.md";

pub const FEATURES: &str = r#"intro = """
What it does.
"""
legend = "No mark: everywhere."
protocols_intro = "The protocols."
behind_intro = "Where others do more."
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
shipped = ["R-1"]

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
behind = "Others send more."
roadmap = ["R-2"]

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
behind = "Others paste images."

[[missing]]
name = "Ligatures"
does = "Not drawn yet."
area = "text"
roadmap = ["R-3"]

[[unlisted]]
id = "R-4"
why = "Internal."

[[unlisted]]
id = "R-7"
why = "Also internal."

[[protocol]]
name = "VT100"
origin = "DEC"
row = "vt:DECAWM"
"#;

pub const ROADMAP: &str = r#"intro = "What is coming."
footer = "Generated too."

[[item]]
id = "R-1"
area = "text"
status = "done"
date = "2026-01-02"
title = "Wrap"
summary = "Lines wrap."

[[item]]
id = "R-4"
area = "input"
status = "done"
date = "2026-01-03"
title = "Internal thing"
summary = "Done inside."

[[item]]
id = "R-5"
area = "text"
status = "done"
date = "2026-01-01"
title = "Another"
summary = "Also done."

[[item]]
id = "R-7"
area = "input"
status = "done"
date = "2026-01-03"
title = "Also internal"
summary = "Done inside too."

[[item]]
id = "R-2"
area = "input"
status = "in_progress"
title = "More keys"
summary = "Every key."

[[item]]
id = "R-3"
area = "text"
status = "planned"
title = "Ligatures"
summary = "Joined glyphs."

[[item]]
id = "R-6"
area = "text"
status = "in_progress"
title = "Zebra stripes"
summary = "Every other line."
"#;

pub const VT: &str = "# VT coverage\n\n## Tier 1 — the core\n\n\
| op | status | note |\n|---|---|---|\n| DECAWM | **done** | wraps |\n| DECOM | partial | origin |\n\
| DECSC | | unmarked |\n\n## Tier 1 — the core\n\n| op | status |\n|---|---|\n| DECRC | done |\n";

pub const INPUT: &str = "# Input\n\n### Keys\n\n| key | status |\n|---|---|\n| F1 | done |\n";

pub const MEASURE: &str = "# Measurements\n\n## Wrapping, measured\n\nText.\n";

/// A tree every check passes on, its two pages written by the generator.
pub fn base() -> Tree {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("xtask-features-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch directory");
    let t = Tree(dir);
    t.put(F, FEATURES);
    // R-5 is done and shipped by a second feature, so `shipped` has two
    // ids in its order.
    t.edit(F, "shipped = [\"R-1\"]", "shipped = [\"R-1\", \"R-5\"]");
    t.put(R, ROADMAP);
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
pub const HEAD: &str = "problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n";

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
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature paste: names no proof\n",
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
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: built, and names no test, no done row and no measurement\n",
        },
        Case {
            name: "1 a field left empty",
            apply: |t| t.edit(F, "does = \"Wraps long lines.\"", "does = \"\""),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: no does\n",
        },
        Case {
            name: "1 an id used twice",
            apply: |t| t.edit(F, "id = \"paste\"", "id = \"keys\""),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature keys: id used twice\n",
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
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: area sound is not defined\n",
        },
        Case {
            name: "1 a status not known",
            apply: |t| t.edit(F, "status = \"interim\"", "status = \"soon\""),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature paste: status soon is not one of ('built', 'partial', 'interim')\n",
        },
        Case {
            name: "1 a platform not known",
            apply: |t| t.edit(F, "platform = \"linux\"", "platform = \"bsd\""),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature paste: platform bsd is not one of ('both', 'macos', 'linux')\n",
        },
        Case {
            name: "1 a partial feature with no gap",
            apply: |t| t.edit(F, "gap = \"Some keys.\"\n", ""),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature keys: a partial feature says what is missing in `gap`\n",
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
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: a built feature has no gap\n",
        },
        // 2. Every proof path exists, and every measurement is a heading.
        Case {
            name: "2 a proof path gone",
            apply: |t| t.remove("core/src/keys.rs"),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature keys: proof path core/src/keys.rs does not exist\n",
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
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: MEASUREMENTS.md has no heading 'Wrapping, measured'\n",
        },
        // 3. Every proof test is a fn in the file it names.
        Case {
            name: "3 a test renamed",
            apply: |t| t.edit("core/tests/wrap.rs", "fn wraps_a_line", "fn wraps_lines"),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: core/tests/wrap.rs has no fn wraps_a_line\n",
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
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: test file core/tests/gone.rs does not exist\n",
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
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: test core/tests/wrap.rs:wraps_a_line is not written <path>::<fn>\n",
        },
        Case {
            name: "3 a fn only after a word character",
            apply: |t| t.edit("core/tests/wrap.rs", "fn pastes", "xfn pastes"),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature paste: core/tests/wrap.rs has no fn pastes\n",
        },
        // 4. Every cited row is a row, and a built feature's rows are done.
        Case {
            name: "4 a row not in the table",
            apply: |t| t.edit(F, "rows = [\"input:F1\"]", "rows = [\"input:F2\"]"),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature keys: no input coverage row F2\n",
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
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: built, but its row DECOM is partial\n",
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
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: built, but its row DECSC is unmarked\n",
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
            expect: "2 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: two vt rows begin DECAWM, so the citation is ambiguous\n  feature paste: no vt coverage row DECRC\n",
        },
        Case {
            name: "4 a protocol's row not in the table",
            apply: |t| t.edit(F, "row = \"vt:DECAWM\"", "row = \"vt:DECAWX\""),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  protocol VT100: no vt row DECAWX\n",
        },
        // 5. Every chip is a defined protocol.
        Case {
            name: "5 a chip not defined",
            apply: |t| t.edit(F, "chips = [\"VT100\"]", "chips = [\"VT100\", \"VT52\"]"),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature wrap: chip VT52 is not a defined protocol\n",
        },
        // 6. Every roadmap id named exists and is not done.
        Case {
            name: "6 a roadmap id not in the roadmap",
            apply: |t| t.edit(F, "roadmap = [\"R-2\"]", "roadmap = [\"R-9\"]"),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  feature keys: roadmap id R-9 is not in docs/data/roadmap.toml\n",
        },
        Case {
            name: "6 a gap's roadmap id done",
            apply: |t| t.edit(F, "roadmap = [\"R-3\"]", "roadmap = [\"R-1\"]"),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  missing Ligatures: roadmap id R-1 is done, so it closes no gap\n",
        },
        Case {
            name: "6 a gap with no roadmap item",
            apply: |t| t.edit(F, "roadmap = [\"R-3\"]\n", ""),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  missing Ligatures: names no roadmap item\n",
        },
        Case {
            name: "6 a gap's area not defined",
            apply: |t| {
                t.edit(
                    F,
                    "area = \"text\"\nroadmap = [\"R-3\"]",
                    "roadmap = [\"R-3\"]",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  missing Ligatures: area None is not defined\n",
        },
        // 7. Every done item is claimed, or unlisted with a reason.
        Case {
            name: "7 a done item nobody claims",
            apply: |t| t.edit(F, "shipped = [\"R-1\", \"R-5\"]", "shipped = [\"R-1\"]"),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  roadmap R-5: done, and no feature claims it in `shipped` -- a built feature missing from the list, or an `unlisted` entry with its reason\n",
        },
        Case {
            name: "7 a done item claimed and unlisted",
            apply: |t| t.edit(F, "id = \"R-4\"", "id = \"R-1\""),
            expect: "2 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  roadmap R-1: both claimed by a feature and listed as not a feature\n  roadmap R-4: done, and no feature claims it in `shipped` -- a built feature missing from the list, or an `unlisted` entry with its reason\n",
        },
        Case {
            name: "7 an unlisted item with no reason",
            apply: |t| t.edit(F, "why = \"Internal.\"\n", ""),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  unlisted R-4: no reason\n",
        },
        Case {
            name: "7 a shipped id not in the roadmap",
            apply: |t| {
                t.edit(
                    F,
                    "shipped = [\"R-1\", \"R-5\"]",
                    "shipped = [\"R-1\", \"R-5\", \"R-8\"]",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  shipped or unlisted id R-8 is not in docs/data/roadmap.toml\n",
        },
        Case {
            name: "7 a shipped id not done",
            apply: |t| {
                t.edit(
                    F,
                    "shipped = [\"R-1\", \"R-5\"]",
                    "shipped = [\"R-1\", \"R-5\", \"R-3\"]",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  shipped or unlisted id R-3 is not done\n",
        },
        Case {
            name: "7 a roadmap status not known",
            apply: |t| t.edit(R, "status = \"planned\"", "status = \"later\""),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  roadmap R-3: status later is not one of ('in_progress', 'planned', 'done')\n",
        },
        Case {
            name: "7 a roadmap area not defined",
            apply: |t| {
                t.edit(
                    R,
                    "area = \"input\"\nstatus = \"in_progress\"",
                    "area = \"keys\"\nstatus = \"in_progress\"",
                )
            },
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  roadmap R-2: area keys is not defined\n",
        },
        // 8. README's count is the list's.
        Case {
            name: "8 README counts another number",
            apply: |t| t.put("README.md", "baud. See all 4 features.\n"),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  README.md: says 'See all 4 features', the list has 3\n",
        },
        Case {
            name: "8 README with no count",
            apply: |t| t.put("README.md", "baud.\n"),
            expect: "1 problem(s) in docs/data/features.toml / docs/data/roadmap.toml:\n  README.md: no 'See all N features' link, so its count is unchecked\n",
        },
        // 9. Both pages are what the generator writes.
        Case {
            name: "9 the feature page edited",
            apply: |t| t.edit(PAGE, "Wraps long lines.", "Wraps lines."),
            expect: "stale: docs/guide/features.md -- edit the data under docs/data/ and rerun cargo xtask features; a generated page is never edited by hand\n",
        },
        Case {
            name: "9 the roadmap page edited",
            apply: |t| t.edit(ROAD, "Every key.", "Each key."),
            expect: "stale: docs/roadmap.md -- edit the data under docs/data/ and rerun cargo xtask features; a generated page is never edited by hand\n",
        },
        Case {
            name: "9 both pages gone",
            apply: |t| {
                t.remove(PAGE);
                t.remove(ROAD);
            },
            expect: "stale: docs/guide/features.md, docs/roadmap.md -- edit the data under docs/data/ and rerun cargo xtask features; a generated page is never edited by hand\n",
        },
        Case {
            name: "9 the data edited and the pages not",
            apply: |t| t.edit(R, "Lines wrap.", "Lines wrap at the margin."),
            expect: "stale: docs/roadmap.md -- edit the data under docs/data/ and rerun cargo xtask features; a generated page is never edited by hand\n",
        },
    ]
}

/// The feature page the script wrote for the base tree.
const BASE_PAGE: &str = r#"# Features

What it does.

No mark: everywhere.

[Text and `fonts`](#text-and-fonts) · [Input](#input) · [Spare](#spare) · [Where baud is behind](#where-baud-is-behind)

## Text and `fonts`

How text looks.

- **Wrapping** — Wraps long lines. <sub>[VT100](../vt-coverage.md#tier-1--the-core) · [proof](../../core/tests/wrap.rs) · [measured](../../MEASUREMENTS.md#wrapping-measured) · [reference](../vt-coverage.md#tier-1--the-core)</sub>

## Input

- **Keys** — Sends the keys. *macOS.* **Partly:** Some keys. <sub>[proof](../../core/src/keys.rs) · [reference](../input-coverage.md#keys)</sub>
- **Paste** — Pastes text. *Linux.* **For now:** A stopgap. <sub>[proof](../../core/tests/wrap.rs) · [reference](../vt-coverage.md#tier-1--the-core-1)</sub>

## Spare


## Protocols

The protocols.

| protocol | from | reference |
|---|---|---|
| VT100 | DEC | [in the coverage table](../vt-coverage.md#tier-1--the-core) |

## Where baud is behind

Where others do more.

- **Keys** — Others send more. [Roadmap](../roadmap.md#input)
- **Paste** — Others paste images.
- **Ligatures** — Not drawn yet. [Roadmap](../roadmap.md#text-and-fonts)

## How this page stays true

Generated.
"#;

/// The roadmap page the script wrote for the base tree: open items in
/// progress first, then the done ones newest first, equal dates in title
/// order.
const BASE_ROAD: &str = r#"# Roadmap

What is coming.

[Text and `fonts`](#text-and-fonts) · [Input](#input)

## Text and `fonts`

- [ ] **Zebra stripes** — Every other line. *In progress.*
- [ ] **Ligatures** — Joined glyphs.
- [x] **Wrap** — Lines wrap. (2026-01-02)
- [x] **Another** — Also done. (2026-01-01)

## Input

- [ ] **More keys** — Every key. *In progress.*
- [x] **Also internal** — Done inside too. (2026-01-03)
- [x] **Internal thing** — Done inside. (2026-01-03)

## How this page stays true

Generated too.
"#;

#[test]
fn the_base_tree_passes() {
    let t = base();
    assert_eq!(run(&t, Mode::Check), (Outcome::Passed, String::new()));
}

#[test]
fn the_pages_are_what_the_script_wrote() {
    let t = base();
    assert_eq!(t.read(PAGE), BASE_PAGE);
    assert_eq!(t.read(ROAD), BASE_ROAD);
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
fn writing_refuses_a_tree_that_fails_and_leaves_the_pages() {
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
    t.remove(ROAD);
    assert_eq!(run(&t, Mode::Write).0, Outcome::Passed);
    assert_eq!(t.read(PAGE), BASE_PAGE);
    assert_eq!(t.read(ROAD), BASE_ROAD);
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

/// The repository's own pages and the data under them, which baud-core's
/// `feature_list` test checked through the script.
#[test]
fn the_repository_passes() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let mut out = Vec::new();
    let o = features::run(std::path::Path::new(root), Mode::Check, &mut out).expect("a run");
    assert_eq!(
        o,
        Outcome::Passed,
        "the feature list or the roadmap has drifted -- fix docs/data/ and run \
         cargo xtask features\n{}",
        String::from_utf8_lossy(&out)
    );
}
