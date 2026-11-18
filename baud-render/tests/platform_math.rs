//! **baud-render calls no platform math outside `det`** (`src/det.rs`):
//! std's transcendental float functions call the host's libm, whose bits
//! differ between operating systems, so a value built from one can move a
//! pixel's rounding on one system and not the other. Every source of the
//! crate but `det.rs` is read, and a call to any of them fails the check
//! with its file and line.
//!
//! The functions barred are `det::PLATFORM`'s, each as a name, and the
//! rest of std's transcendental ones: the hyperbolic functions and their
//! inverses, the gamma functions, and `powi`, whose precision std leaves
//! unspecified. What stays allowed is IEEE's own and correctly rounded on
//! every machine: `+ - * /`, `sqrt`, `mul_add`, `floor`, `ceil`, `round`,
//! `trunc`, `abs`, `%` and the bit moves.
//!
//! The check reads code, not text: comments and the inside of string and
//! character literals are blanked first, so a comment naming `x.tan()`
//! passes and a call outside one does not. It also reads a name, not a
//! type, so a method of baud's own named after one of std's is a hit as
//! well; none is. Its controls: every source is read, with det's calls
//! counted in their code, and a planted call in each spelling is found
//! while the same spellings in a comment or a literal are not.

use std::path::{Path, PathBuf};

/// The barred functions `det::PLATFORM` does not spell.
const MORE: [&str; 8] = [
    "sinh", "cosh", "asinh", "acosh", "atanh", "powi", "gamma", "ln_gamma",
];

/// Every barred function, as its name.
fn barred() -> Vec<&'static str> {
    baud_render::det::PLATFORM
        .iter()
        .map(|call| call.trim_start_matches('.').trim_end_matches('('))
        .chain(MORE)
        .collect()
}

/// Every `.rs` file under `dir`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a source directory") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `src` with every comment, and the inside of every string and character
/// literal, blanked to spaces, its line breaks kept: the code alone, each
/// line where it was.
fn code(src: &str) -> String {
    let s: Vec<char> = src.chars().collect();
    let peek = |i: usize| s.get(i).copied().unwrap_or('\0');
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < s.len() {
        let (c, next) = (s[i], peek(i + 1));
        let end = if c == '/' && next == '/' {
            // A line comment, a doc comment among them: to the line's end.
            Some((i..s.len()).find(|&j| s[j] == '\n').unwrap_or(s.len()))
        } else if c == '/' && next == '*' {
            // A block comment, which nests.
            let (mut j, mut depth) = (i, 0u32);
            loop {
                if j >= s.len() {
                    break Some(s.len());
                }
                if s[j] == '/' && peek(j + 1) == '*' {
                    depth += 1;
                    j += 2;
                } else if s[j] == '*' && peek(j + 1) == '/' {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        break Some(j);
                    }
                } else {
                    j += 1;
                }
            }
        } else if c == '"' {
            // A string, `b"` and `c"` among them: to the first quote no
            // backslash escapes.
            let mut j = i + 1;
            while j < s.len() && s[j] != '"' {
                j += if s[j] == '\\' { 2 } else { 1 };
            }
            Some((j + 1).min(s.len()))
        } else if c == 'r'
            && matches!(next, '"' | '#')
            && (i == 0 || !word(s[i - 1]) || s[i - 1] == 'b')
        {
            // A raw string, `br"` among them, to its quote and as many
            // `#` as opened it; `r#name` is a raw identifier, and code.
            let hashes = s[i + 1..].iter().take_while(|&&h| h == '#').count();
            let from = i + 2 + hashes;
            (peek(from - 1) == '"').then(|| {
                s[from..]
                    .windows(1 + hashes)
                    .position(|w| w[0] == '"' && w[1..].iter().all(|&h| h == '#'))
                    .map_or(s.len(), |p| from + p + 1 + hashes)
            })
        } else if c == '\'' {
            // A character literal, `b'` among them; a lifetime or a label
            // has no closing quote, and is code.
            if next == '\\' {
                Some(
                    (i + 3..s.len())
                        .find(|&j| s[j] == '\'')
                        .map_or(s.len(), |j| j + 1),
                )
            } else if peek(i + 2) == '\'' {
                Some(i + 3)
            } else {
                None
            }
        } else {
            None
        };
        match end {
            Some(end) => {
                out.extend(
                    s[i..end]
                        .iter()
                        .map(|&k| if k == '\n' { '\n' } else { ' ' }),
                );
                i = end;
            }
            None => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// The barred functions `src`'s code calls, each as `<line>: <name>`: a
/// method call `.name(`, spaces allowed about the dot and before the
/// parenthesis, or the path `f32::name` or `f64::name`, called or passed.
fn platform_calls(src: &str) -> Vec<String> {
    let names = barred();
    let text = code(src);
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let mut start = None;
        for (k, ch) in line.char_indices().chain([(line.len(), ' ')]) {
            let inside = ch.is_alphanumeric() || ch == '_';
            match start {
                None if inside => start = Some(k),
                Some(from) if !inside => {
                    start = None;
                    let name = &line[from..k];
                    let (before, after) = (line[..from].trim_end(), line[k..].trim_start());
                    let method =
                        before.ends_with('.') && !before.ends_with("..") && after.starts_with('(');
                    let path = before.ends_with("f32::") || before.ends_with("f64::");
                    if names.contains(&name) && (method || path) {
                        found.push(format!("{}: {name}", n + 1));
                    }
                }
                _ => {}
            }
        }
    }
    found
}

#[test]
fn baud_render_calls_no_platform_math_outside_det() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    let (mut calls, mut det) = (Vec::new(), 0);
    for f in &files {
        if f.file_name().is_some_and(|n| n == "det.rs") {
            continue;
        }
        let src = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        det += code(&src).matches(".det_").count();
        let name = f.strip_prefix(&root).unwrap_or(f).display().to_string();
        calls.extend(
            platform_calls(&src)
                .into_iter()
                .map(|c| format!("{name}:{c}")),
        );
    }
    assert!(files.len() >= 50, "the sources are read: {}", files.len());
    assert!(det >= 90, "det's calls are read in the code: {det}");
    assert!(
        calls.is_empty(),
        "the platform's math, where det's belongs: {calls:#?}"
    );
}

#[test]
fn the_scan_finds_a_planted_call_and_passes_comments_and_literals() {
    // Each spelling of a call is found, on its line.
    assert_eq!(
        platform_calls("let a = 1;\nlet k = lean.tan();\n"),
        ["2: tan"]
    );
    assert_eq!(
        platform_calls("let s = f32::sin(a);\nlet c = v.map(f64::cos);\nlet p = x . powi (2);\n"),
        ["1: sin", "2: cos", "3: powi"]
    );
    // A character literal holding a quote opens no string, and a lifetime
    // opens no character: the code after each is still read.
    assert_eq!(
        platform_calls("let q = '\"'; let e = x.exp();\nfn f<'a>(x: &'a f32) -> f32 { x.ln() }\n"),
        ["1: exp", "2: ln"]
    );
    // Named but not called: in comments, a string, a raw string, det's own
    // names, a field, a free function and a range.
    assert_eq!(
        platform_calls(concat!(
            "/// lean.tan() leans\n",
            "/* a /* x.sin() */ x.cos() */\n",
            "let s = \".tan(\";\n",
            "let r = r#\"a.cos()\"#;\n",
            "let d = x.det_tan() + x.tangent + x.exp + exp(y) + (0..sin(z)).len();\n",
        )),
        Vec::<String>::new()
    );
}
