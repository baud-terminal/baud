//! **The renderer names no `Term` outside its tests**: every read it makes
//! of a terminal goes through `&impl TermRead`, so no non-test source under
//! `baud-render/src` names the core's `Term` type, in a signature, a field,
//! a path or an import. Comments and test modules are not read.

use std::path::{Path, PathBuf};

/// This crate's `src`.
fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `dir`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// `text` without its test modules -- each `#[cfg(test)]` followed by
/// `mod`, at any indent, to the `}` rustfmt sets alone at that indent to
/// close it, or to its `;` where it is a file of its own -- and without its
/// line comments, doc comments included.
fn non_test(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let indent = &line[..line.len() - line.trim_start().len()];
        if line.trim() == "#[cfg(test)]"
            && lines
                .peek()
                .is_some_and(|n| n.trim_start().starts_with("mod ") && n.starts_with(indent))
        {
            let open = lines.next().unwrap_or_default();
            if !open.trim_end().ends_with(';') {
                let close = format!("{indent}}}");
                for l in lines.by_ref() {
                    if l == close {
                        break;
                    }
                }
            }
            continue;
        }
        let code = line.find("//").map_or(line, |at| &line[..at]);
        kept.push_str(code);
        kept.push('\n');
    }
    kept
}

/// The places `text`'s code names `Term` as a whole word, by line.
fn names(text: &str) -> Vec<String> {
    non_test(text)
        .lines()
        .filter(|l| {
            l.match_indices("Term").any(|(i, _)| {
                let before = l[..i].chars().next_back();
                let after = l[i + 4..].chars().next();
                !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
                    && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
            })
        })
        .map(str::to_string)
        .collect()
}

/// **No non-test source of the renderer names `Term`.**
///
/// Named break: `term: &baud_core::Term<'_>` in place of
/// `term: &impl TermRead` on any renderer function -- this test fails and
/// names the file and the line.
#[test]
fn the_renderer_reads_a_terminal_only_through_term_read() {
    let mut files = Vec::new();
    sources(&src(), &mut files);
    assert!(files.len() > 10, "the scan finds the sources");
    let found: Vec<String> = files
        .iter()
        .flat_map(|p| {
            let text = std::fs::read_to_string(p).expect("a source");
            let name = p.display().to_string();
            names(&text)
                .into_iter()
                .map(move |l| format!("{name}: {}", l.trim()))
        })
        .collect();
    assert!(found.is_empty(), "the renderer names Term: {found:#?}");
}

/// The control: the scan finds `Term` in a signature and an import, and
/// passes `TermRead`, another name ending in `Term`, a comment and a test
/// module, nested or at the top.
#[test]
fn the_scan_finds_a_term_and_passes_the_rest() {
    assert_eq!(names("fn f(term: &Term<'_>) {}\n").len(), 1);
    assert_eq!(names("use baud_core::{Owned, Term};\n").len(), 1);
    assert_eq!(names("fn f(term: &impl TermRead) {}\n").len(), 0);
    assert_eq!(names("struct LiveTerm;\n").len(), 0);
    assert_eq!(names("/// [`Term::view`] draws it\nfn f() {}\n").len(), 0);
    assert_eq!(names("let x = 1; // Term\n").len(), 0);
    let nested = "mod kept {\n    #[cfg(test)]\n    mod tests {\n        use baud_core::Term;\n    }\n    fn f(t: &Term<'_>) {}\n}\n";
    assert_eq!(
        names(nested).len(),
        1,
        "only the code after the test module"
    );
    let top = "#[cfg(test)]\nmod tests {\n    use baud_core::Term;\n}\n";
    assert_eq!(names(top).len(), 0);
    assert_eq!(
        names("#[cfg(test)]\nmod t;\nuse baud_core::Term;\n").len(),
        1
    );
}
