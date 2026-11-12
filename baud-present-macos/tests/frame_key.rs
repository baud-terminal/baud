//! **A GPU frame's key collects nothing to compare.**
//!
//! Each GPU path keeps what its last frame drew beside the rows -- the
//! exit-status strip's records, the word wrap's marks and the activity
//! lights' -- and a frame whose key is alike draws only the rows that
//! changed. The comparison reads the renderer's records where they are and
//! the key keeps them in buffers of its own, so a frame alike the last
//! allocates nothing for it: a `Vec` collected each frame only to be
//! compared is an allocation on every caret frame, which no frame needs.
//!
//! Neither path can be driven here without a Metal device, so this reads
//! `src/metal.rs` as code (comments and strings blanked) and checks, in
//! each frame function, every statement at the top of its body: one that
//! allocates a collection (`.collect`, `.to_vec(`, `vec![`, `Vec::from(`,
//! `Vec::with_capacity(`, `.to_owned(`) is only one of the statements
//! named below, and none of those reads a record. The positive controls:
//! each function is found, reads all three kinds of record, and holds each
//! named statement with an allocation in it, so the search can fire.
//!
//! Perturbation: the strip and the margin's records collected into a
//! `Vec` and compared, as `let strip_now: Vec<Instance> = renderer
//! .strip_records().iter().chain(renderer.margin_records()).copied()
//! .collect();` in `draw_cells_since`, or the instance key built with
//! `strip: ...collect()` and `lights: renderer.lights_records().to_vec()`
//! in `draw_since`; each turns its function's check red.

use std::fs::read_to_string;
use std::path::Path;

mod scan;
use scan::blanked;

/// What allocates a collection, its spaces taken out.
const ALLOCATES: [&str; 6] = [
    ".collect",
    ".to_vec(",
    "vec![",
    "Vec::from(",
    "Vec::with_capacity(",
    ".to_owned(",
];

/// The renderer's records a frame's key holds.
const RECORDS: [&str; 3] = ["strip_records()", "margin_records()", "lights_records()"];

/// `metal.rs` as code, its spaces kept so offsets agree.
fn metal() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/metal.rs");
    let src = read_to_string(path).expect("src/metal.rs");
    String::from_utf8(blanked(&src)).expect("blanking keeps UTF-8")
}

/// The statements at the top of `name`'s body, each with its spaces taken
/// out. A statement ends at a `;` there, or at the `}` that closes a block
/// opened there.
fn statements(code: &str, name: &str) -> Vec<String> {
    let head = format!("fn {name}(");
    let at = code
        .find(&head)
        .unwrap_or_else(|| panic!("`{head}` is in metal.rs"));
    assert_eq!(
        code.matches(&head).count(),
        1,
        "`{head}` is defined once in metal.rs"
    );
    let b = code.as_bytes();
    // The body's brace: the first `{` outside the signature's parentheses.
    let mut paren = 0usize;
    let mut open = None;
    for (i, &c) in b.iter().enumerate().skip(at) {
        match c {
            b'(' | b'[' => paren += 1,
            b')' | b']' => paren -= 1,
            b'{' if paren == 0 => {
                open = Some(i);
                break;
            }
            _ => {}
        }
    }
    let open = open.unwrap_or_else(|| panic!("`{head}` has a body"));
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut from = open + 1;
    for (i, &c) in b.iter().enumerate().skip(open + 1) {
        let end = match c {
            b'(' | b'[' | b'{' => {
                depth += 1;
                false
            }
            b')' | b']' => {
                depth -= 1;
                false
            }
            b'}' if depth == 0 => {
                // The body's own close.
                out.push(code[from..i].to_owned());
                break;
            }
            b'}' => {
                depth -= 1;
                depth == 0
            }
            b';' => depth == 0,
            _ => false,
        };
        if end {
            out.push(code[from..=i].to_owned());
            from = i + 1;
        }
    }
    out.into_iter()
        .map(|s| s.chars().filter(|c| !c.is_whitespace()).collect::<String>())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The check: in `name`, only the statements starting with one of `named`
/// allocate a collection, each of them does (a positive control), and none
/// of them reads a record.
fn collects_only(name: &str, named: &[&str]) {
    let code = metal();
    let body = statements(&code, name);
    for record in RECORDS {
        assert!(
            body.iter().any(|s| s.contains(record)),
            "`{name}` reads `{record}`: the function that keys the records"
        );
    }
    for start in named {
        let found: Vec<&String> = body.iter().filter(|s| s.starts_with(start)).collect();
        assert_eq!(found.len(), 1, "`{name}` holds one `{start}` statement");
        assert!(
            ALLOCATES.iter().any(|a| found[0].contains(a)),
            "`{name}`'s `{start}` statement allocates, so the search can find one: {}",
            found[0]
        );
    }
    for s in &body {
        let allocates = ALLOCATES.iter().find(|a| s.contains(*a));
        let kept = named.iter().find(|n| s.starts_with(*n));
        if let Some(a) = allocates {
            assert!(
                kept.is_some(),
                "`{name}` allocates a collection (`{a}`) in a statement it does not name: {s}"
            );
        }
        if kept.is_some() {
            for record in RECORDS {
                assert!(
                    !s.contains(record),
                    "`{name}` collects `{record}` in an allocating statement: {s}"
                );
            }
        }
    }
}

/// **The instance path's key compares the records where they are.** Its
/// one other collection is the frame's scissor bands, `clips`, which is
/// not a record of the key.
#[test]
fn the_instance_paths_frame_key_collects_nothing() {
    collects_only("draw_since", &["letclips:"]);
}

/// **The cell path's key compares the records where they are**, and keeps
/// them in its last frame's buffers. Its one other collection is `clips`,
/// as the instance path's.
#[test]
fn the_cell_paths_frame_key_collects_nothing() {
    collects_only("draw_cells_since", &["letclips:"]);
}
