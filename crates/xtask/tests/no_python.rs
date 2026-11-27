//! `cargo xtask no-python`: each form planted in a temporary git tree turns
//! the gate red at the line it is on, what the gate has to leave alone
//! stays green, and the repository itself passes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::no_python::{self, Form};
use xtask::third_party::Outcome;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Tree(PathBuf);

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Tree {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("xtask-no-python-{}-{n}", std::process::id()));
        fs::create_dir_all(&d).expect("scratch");
        let t = Self(d);
        t.git(&["init", "-q"]);
        t
    }

    fn git(&self, args: &[&str]) {
        let mut git = Command::new("git");
        git.current_dir(&self.0).args(args);
        for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX"] {
            git.env_remove(v);
        }
        let out = git.output().expect("git");
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    fn add(&self, path: &str, content: &[u8]) -> &Self {
        let full = self.0.join(path);
        fs::create_dir_all(full.parent().expect("a parent")).expect("the parent");
        fs::write(full, content).expect("the file");
        self.git(&["add", "-A"]);
        self
    }

    /// The gate's verdict and report on what is staged.
    fn run(&self) -> (Outcome, String) {
        let mut out = Vec::new();
        let o = no_python::run(&self.0, &mut out).expect("the gate");
        (o, String::from_utf8(out).expect("UTF-8"))
    }
}

/// The gate is red on `content` at `path` and names exactly that line, in
/// that form.
#[track_caller]
fn fires(path: &str, content: &str, line: usize, form: Form) {
    let t = Tree::new();
    t.add(path, content.as_bytes());
    let (o, report) = t.run();
    assert_eq!(o, Outcome::Failed, "{path}: {content}\n{report}");
    let found = no_python::scan(path, content.as_bytes(), &Default::default());
    assert_eq!(found.len(), 1, "{path}: {content}\n{found:?}");
    assert_eq!((found[0].line, found[0].form), (line, form), "{content}");
    assert!(
        report.contains(&format!("FAIL {path}:{line}: ")),
        "{report}"
    );
}

/// The gate is green on `content` at `path`.
#[track_caller]
fn passes(path: &str, content: &str) {
    let t = Tree::new();
    t.add(path, content.as_bytes());
    let (o, report) = t.run();
    assert_eq!(o, Outcome::Passed, "{path}: {content}\n{report}");
}

#[test]
fn a_tracked_python_file_fires_anywhere_and_whatever_it_holds() {
    for path in [
        "a.py",
        "scripts/gen.py",
        "crates/xtask/src/no_python.py",
        "tests/recordings/x.py",
        "docs/research/sources/x.py",
        "a.pyi",
        "b.pyc",
        "scripts/__pycache__/x.txt",
    ] {
        fires(path, "nothing at all\n", 0, Form::File);
    }
}

#[test]
fn an_untracked_python_file_is_not_the_repository_s() {
    let t = Tree::new();
    t.add("kept.txt", b"text\n");
    fs::write(t.0.join("loose.py"), "x = 1\n").expect("the file");
    assert_eq!(t.run().0, Outcome::Passed);
}

#[test]
fn a_shebang_fires_in_any_file() {
    for first in [
        "#!/usr/bin/env python3",
        "#!/usr/bin/env python",
        "#!/usr/bin/python3.13 -u",
        "#!/usr/bin/env -S python3 -u",
        "#!/nix/store/abc-python3-3.13/bin/python3",
    ] {
        let text = format!("{first}\nprint(1)\n");
        fires("scripts/tool", &text, 1, Form::Shebang);
        fires("notes.txt", &text, 1, Form::Shebang);
    }
}

#[test]
fn a_shebang_of_another_program_or_past_line_one_is_none() {
    passes("scripts/tool", "#!/usr/bin/env bash\ntrue\n");
    passes("scripts/tool", "#!/bin/sh\ntrue\n");
    passes("notes.txt", "a line\n#!/usr/bin/env python3\n");
    passes("scripts/tool", "#!/opt/pythonic/bin/sh\ntrue\n");
}

#[test]
fn a_script_runs_it_wherever_a_command_starts() {
    for line in [
        "python3 -c 'print(1)'",
        "python3 <<EOF",
        "python3 - <<'EOF'",
        "python3<<EOF",
        "python tool",
        "python3.13 tool",
        "/usr/bin/python3 tool",
        "pip install wheel",
        "pip3 install wheel",
        "pip3 list",
        "cd x && python3 tool",
        "cd x; python3 tool",
        "false || python3 tool",
        "cat x | python3 tool",
        "x=$(python3 tool)",
        "x=`python3 tool`",
        "(python3 tool)",
        "{ python3 tool; }",
        "exec python3 tool",
        "env python3 tool",
        "/usr/bin/env python3 tool",
        "FOO=1 BAR=2 python3 tool",
        "env FOO=1 python3 tool",
        "nice -n 10 python3 tool",
        "sudo pip install wheel",
        "xargs -0 python3 tool",
        "bash -c 'python3 tool'",
        "sh -c \"pip3 install wheel\"",
        "if python3 tool; then true; fi",
        "while python3 tool; do true; done",
        "true && ! python3 tool",
    ] {
        fires(
            "scripts/x.sh",
            &format!("set -e\n{line}\n"),
            2,
            Form::Command,
        );
    }
}

#[test]
fn a_command_on_a_continued_line_is_at_the_line_it_starts_on() {
    fires(
        "scripts/x.sh",
        "true\ncd x \\\n  && python3 tool\n",
        2,
        Form::Command,
    );
}

#[test]
fn a_shell_file_is_known_by_its_name_or_its_shebang() {
    let line = "python3 tool\n";
    fires(
        "scripts/x",
        &format!("#!/usr/bin/env bash\n{line}"),
        2,
        Form::Command,
    );
    fires("scripts/y", &format!("#!/bin/sh\n{line}"), 2, Form::Command);
    fires("Dockerfile", "FROM x\nRUN python3 tool\n", 2, Form::Command);
    fires(
        "x/Dockerfile.linux",
        "FROM x\nRUN pip install y\n",
        2,
        Form::Command,
    );
    fires(
        ".github/workflows/x.yml",
        "steps:\n  - run: python3 tool\n",
        2,
        Form::Command,
    );
    passes("notes.txt", line);
    passes("tests/themes/x.yml", "name: python3 tool\n");
}

#[test]
fn a_package_argument_fires_across_continued_lines() {
    for text in [
        "RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \\\n    --no-install-recommends sway python3 \\\n    && useradd baud\n",
        "apt-get install -y python3-dev\n",
        "apt install python3.13\n",
        "apk add python3\n",
        "brew install python\n",
        "nix-shell -p python3 --run true\n",
        "nix shell nixpkgs#python3 -c true\n",
        "nix-shell -p python3Packages.pillow\n",
    ] {
        fires("scripts/x.sh", text, 1, Form::Package);
    }
    passes(
        "scripts/x.sh",
        "apt-get install -y sway grim && echo python3\napt-get update\n",
    );
}

#[test]
fn a_nix_attribute_fires_and_a_nix_comment_does_not() {
    for line in [
        "  packages = [ pkgs.python3 ];",
        "  x = pkgs.python313Packages.pillow;",
        "  y = python3.withPackages (p: [ ]);",
        "  z = python3Packages.pillow;",
        "  with pkgs; [ python3 ]",
    ] {
        fires("x.nix", &format!("{{\n{line}\n}}\n"), 2, Form::Nix);
    }
    passes(
        "x.nix",
        "{\n  # python happens to be on the host.\n  # Python\n}\n",
    );
    passes("x.nix", "{\n  a = pkgs.libwebp;  # not python3\n}\n");
}

#[test]
fn a_spawn_fires_in_rust_and_the_word_does_not() {
    fires(
        "src/x.rs",
        "fn f() {\n    Command::new(\"python3\").arg(\"x\");\n}\n",
        2,
        Form::Spawn,
    );
    fires(
        "src/x.rs",
        "fn f() {\n    Command::new(\n        \"/usr/bin/python\",\n    );\n}\n",
        2,
        Form::Spawn,
    );
    fires(
        "src/x.rs",
        "const C: &str = \"python3 -c 'print(1)'\";\n",
        1,
        Form::Invocation,
    );
    passes(
        "src/x.rs",
        concat!(
            "//! # Python\n",
            "//! The Python it replaced, and how `python3` was found.\n",
            "fn f() {\n",
            "    println!(\"--- python\\n{}\", 1);\n",
            "    let a = \"lib/python3.14/os.py\";\n",
            "    let b = \"libpython3.14.dylib\";\n",
            "    Command::new(\"pythonic\");\n",
            "}\n",
        ),
    );
}

#[test]
fn a_readme_runs_it_in_a_fence_a_span_or_a_bare_line() {
    let recipe = "python3 -c \"print(1)\"";
    fires(
        "tests/x/README.md",
        &format!("# Title\n\n```sh\n{recipe}\n```\n"),
        4,
        Form::Invocation,
    );
    fires(
        "README.md",
        "```sh\ncd x && python3 tool\n```\n",
        2,
        Form::Command,
    );
    fires("README.md", "```\npython3 tool\n```\n", 2, Form::Command);
    fires(
        "README.md",
        "~~~bash\npython3 tool\n~~~\n",
        2,
        Form::Command,
    );
    fires(
        "README.md",
        "Do this:\n\n    pip install wheel\n",
        3,
        Form::Invocation,
    );
    fires(
        "README.md",
        &format!("Run {recipe} first.\n"),
        1,
        Form::Invocation,
    );
    fires("README.md", "Run `python3 tool` first.\n", 1, Form::Span);
    fires("README.md", "Then `pip3 list`.\n", 1, Form::Span);
    fires("docs/x.txt", "pip install wheel\n", 1, Form::Invocation);
    fires(
        "README.md",
        "```sh\nunclosed\npython3 tool\n",
        3,
        Form::Command,
    );
}

#[test]
fn a_word_in_prose_is_not_a_use() {
    for text in [
        "# Python\n",
        "A pythonic way, Python 3.14.7 and CPython's `sum`.\n",
        "`python3` is off the path, and `python` names it alone.\n",
        "The Python virtualenv: add `$python` after `$nix_shell`.\n",
        "`KITTY_LIBPYTHON` names `libpython3.14`, found at `lib/python3.14/os.py`.\n",
        "The `[python]` block; the --python flag; pipes.sh -t 4; pipefail; a pipeline.\n",
        "```toml\n[python]\npython = 1\npython3 tool\n```\n",
        "```text\npython3 tool\n```\n",
        "```python3\nimport x\n```\n",
    ] {
        passes("README.md", text);
    }
    for line in [
        "# Python, as a comment",
        "# the python3 that was here",
        "echo python3",
        "set -euo pipefail",
        "pipes.sh -t 4",
        "echo ${python3}",
        "cp a/python3 b",
        "pythonic tool",
        "python-config --libs",
        "pipx run x",
        "pip2to3",
        "libpython3 -c x",
        "$python -c x",
        "a.python3 -c x",
        "true # && python3 tool",
        "# cd x; python3 tool",
    ] {
        passes("scripts/x.sh", &format!("{line}\n"));
    }
}

#[test]
fn a_pinned_source_passes_by_name_and_an_unpinned_one_fires() {
    const PIN: &str = "gen.py.txt";
    let manifest = format!(
        "[sources.gen]\nkey    = \"gen\"\nfile   = \"{PIN}\"\nsha256 = \"0\"\nstatus = \"read\"\n"
    );
    let body = "#!/usr/bin/env python\npip install wheel\n";
    let t = Tree::new();
    t.add("docs/research/sources/MANIFEST.toml", manifest.as_bytes());
    t.add(&format!("docs/research/sources/{PIN}"), body.as_bytes());
    let (o, report) = t.run();
    assert_eq!(o, Outcome::Passed, "{report}");

    // The same bytes under a name the manifest does not hold.
    t.add("docs/research/sources/other.py.txt", body.as_bytes());
    let (o, report) = t.run();
    assert_eq!(o, Outcome::Failed);
    assert!(report.contains("other.py.txt:1: a shebang"), "{report}");
    assert!(!report.contains(PIN), "{report}");
}

#[test]
fn a_pinned_name_elsewhere_is_not_pinned() {
    let manifest = "[sources.gen]\nfile = \"gen.py.txt\"\n";
    let t = Tree::new();
    t.add("docs/research/sources/MANIFEST.toml", manifest.as_bytes());
    t.add("scripts/gen.py.txt", b"#!/usr/bin/env python\n");
    assert_eq!(t.run().0, Outcome::Failed);
}

/// A record's arm, as the record says it: the one line of history the rule
/// exists for, and each way out of the rule.
const HISTORY: &str = "--child stream` ran `python3 -u`; it now runs this";

#[test]
fn a_record_s_past_run_is_history() {
    let record = format!("# M\n\n## A section\n\n**Child.** `{HISTORY} driver.\n");
    passes("MEASUREMENTS.md", &record);
    // The word alone names it, anywhere.
    passes("MEASUREMENTS.md", "## A\n\nA `python3` that exits 99.\n");
}

#[test]
fn history_is_that_file_that_verb_outside_a_fence_and_below_a_heading() {
    let record = format!("## A section\n\n`{HISTORY} driver.\n");
    // Another file.
    fires("README.md", &record, 3, Form::Invocation);
    fires("docs/guide/x.md", &record, 3, Form::Invocation);
    // Before the file's first section.
    fires(
        "MEASUREMENTS.md",
        &format!("# M\n\n`{HISTORY} driver.\n"),
        3,
        Form::Invocation,
    );
    // Another verb, or none.
    for line in [
        "Then run `python3 -u` on it.",
        "It runs `python3 -u`.",
        "Use `python3 -u`.",
        "`python3 -u`.",
        "We then ran the tool, and `python3 -u` after it.",
        "scran `python3 -u` here.",
    ] {
        fires(
            "MEASUREMENTS.md",
            &format!("## A\n\n{line}\n"),
            3,
            Form::Invocation,
        );
    }
    // Without the backquotes.
    fires(
        "MEASUREMENTS.md",
        "## A\n\nIt ran python3 -u then.\n",
        3,
        Form::Invocation,
    );
    // In a fence: a recipe, whatever precedes it.
    fires(
        "MEASUREMENTS.md",
        "## A\n\n```sh\nran `python3 -u`\n```\n",
        4,
        Form::Invocation,
    );
    // A span after it on the same line has no verb of its own.
    fires(
        "MEASUREMENTS.md",
        "## A\n\nIt ran `python3 -u`, and `python3 -c x` next.\n",
        3,
        Form::Invocation,
    );
}

#[test]
fn the_gate_s_own_files_pass_by_exact_path_and_the_same_text_fires_elsewhere() {
    let text = "#!/usr/bin/env python3\n//! python3 -c x\nCommand::new(\"python3\");\n";
    for own in no_python::OWN {
        passes(own, text);
    }
    for other in [
        "crates/xtask/src/other.rs",
        "crates/xtask/tests/other.rs",
        "crates/xtask/src/no_python.rs.bak",
        "x/xtask/src/no_python.rs",
    ] {
        let t = Tree::new();
        t.add(other, text.as_bytes());
        assert_eq!(t.run().0, Outcome::Failed, "{other}");
    }
}

#[test]
fn a_recording_passes_only_under_tests() {
    let line = "`python3 impl_audit.py x` reported 0\n";
    passes("tools/tests/recordings/ascii/git-log.raw", line);
    fires("scripts/x.raw", line, 1, Form::Invocation);
    fires("tests/recordings/x.txt", line, 1, Form::Invocation);
}

#[test]
fn a_binary_file_is_not_read_as_lines() {
    let mut bytes = b"python3 -c x\n".to_vec();
    bytes.push(0);
    let t = Tree::new();
    t.add("x.bin", &bytes);
    assert_eq!(t.run().0, Outcome::Passed);
    // A NUL past git's probe leaves the file text.
    let mut late = b"python3 -c x\n".to_vec();
    late.extend(std::iter::repeat_n(b' ', 8000));
    late.push(0);
    t.add("y.bin", &late);
    assert_eq!(t.run().0, Outcome::Failed);
}

#[test]
fn a_link_and_a_deleted_file_are_not_read() {
    let t = Tree::new();
    t.add("target.txt", b"text\n");
    std::os::unix::fs::symlink("target.txt", t.0.join("link.txt")).expect("the link");
    t.add("gone.txt", b"text\n");
    fs::remove_file(t.0.join("gone.txt")).expect("the delete");
    assert_eq!(t.run().0, Outcome::Passed);
}

fn repository() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("the repository")
}

#[test]
fn the_repository_holds_no_python_and_runs_none() {
    let mut out = Vec::new();
    let o = no_python::run(repository(), &mut out).expect("the gate");
    let report = String::from_utf8(out).expect("UTF-8");
    assert_eq!(o, Outcome::Passed, "{report}");
    // The own files and a pinned source were among what it listed, so the
    // pass is over the repository, not over an empty listing.
    let files = no_python::tracked(repository()).expect("git");
    for own in no_python::OWN {
        assert!(files.iter().any(|f| f == own), "{own} is not tracked");
    }
    let pins = fs::read_to_string(repository().join("docs/research/sources/MANIFEST.toml"))
        .expect("the manifest");
    let pinned = no_python::pinned(&pins);
    let held = "docs/research/sources/iterm2-color-schemes-gen.py.txt";
    assert!(pinned.contains(held), "{held} is not pinned");
    assert!(files.iter().any(|f| f == held), "{held} is not tracked");
}
