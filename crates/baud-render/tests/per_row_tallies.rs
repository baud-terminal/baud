//! **A row's instances are written only where its tally follows** (FS-C7).
//!
//! The renderer's frame totals are kept as each row's instances are set
//! (`src/per_row.rs`), so a write that reached a row without its tally
//! being taken again would leave every later frame reporting stale totals.
//! The type's fields are private, so nothing outside its module writes a
//! row; this reads the module and requires the same of the code inside:
//! every method that writes a row tallies it, or empties the tallies with
//! the rows, and nothing hands a row out to be written elsewhere.

/// `src` with every line comment dropped.
fn code(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The methods of `impl PerRow`, each from its `fn` to the next.
fn methods(code: &str) -> Vec<&str> {
    let at = code
        .find("impl PerRow {")
        .expect("impl PerRow is in per_row.rs");
    let end = at + code[at..].find("\n}\n").expect("impl PerRow closes");
    let block = &code[at..end];
    let starts: Vec<usize> = block.match_indices("fn ").map(|(i, _)| i).collect();
    starts
        .iter()
        .enumerate()
        .map(|(k, &s)| &block[s..starts.get(k + 1).copied().unwrap_or(block.len())])
        .collect()
}

/// Whether `body` writes a row's instances.
fn writes_a_row(body: &str) -> bool {
    let squeezed: String = body.split_whitespace().collect();
    squeezed.contains("&mutself.rows")
        || squeezed.contains("self.rows=")
        || squeezed.match_indices("self.rows[").any(|(i, _)| {
            squeezed[i..]
                .split_once(']')
                .is_some_and(|(_, after)| after.starts_with('=') && !after.starts_with("=="))
        })
}

/// Whether `body` takes the tallies again after its write.
fn tallies(body: &str) -> bool {
    let squeezed: String = body.split_whitespace().collect();
    squeezed.contains("self.tally(")
        || (squeezed.contains("self.kinds=") && squeezed.contains("self.total=Kinds::default()"))
}

#[test]
fn every_write_of_a_row_takes_its_tally_again() {
    let code = code(include_str!("../src/per_row.rs"));
    let methods = methods(&code);
    let writers: Vec<&str> = methods
        .iter()
        .copied()
        .filter(|m| writes_a_row(m))
        .collect();
    for name in ["fn reset(", "fn set(", "fn take("] {
        assert!(
            writers.iter().any(|m| m.starts_with(name)),
            "{name} is read as writing a row: the check can fire"
        );
    }
    for m in &writers {
        let sig = m.lines().next().unwrap_or("");
        assert!(tallies(m), "`{sig}` writes a row without its tally");
    }
}

#[test]
fn no_row_is_handed_out_to_be_written() {
    let code = code(include_str!("../src/per_row.rs"));
    for (form, what) in [
        ("IndexMut", "a row indexed to be written"),
        ("iter_mut", "the rows iterated to be written"),
        ("-> &mut", "a row lent out to be written"),
        ("pub rows", "the rows public"),
        ("pub(crate) rows", "the rows open to the crate"),
        ("pub(crate) kinds", "the tallies open to the crate"),
        ("pub(crate) total", "the totals open to the crate"),
    ] {
        assert!(!code.contains(form), "per_row.rs has {what}: `{form}`");
    }
}
