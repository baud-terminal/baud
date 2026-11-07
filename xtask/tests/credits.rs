//! `cargo xtask credits`: the committed notices are the generated ones,
//! every include the binary embeds falls under a row or baud's own list,
//! and so does every file a build reads to write the code the binary
//! compiles in; each embedded row's notice and licence is in the file, and
//! every transcribed row's; each CC BY work carries every part of its
//! section 3(a)(1); each permission text is printed once, with every
//! copyright line under it; and the size tripwire fires and says where the
//! bytes went.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use toml::Table;
use xtask::credits::{
    self, Bytes, CC_BY_PARTS, LICENCES, Lists, Mode, NameId, OUT, Own, Source, TRIPWIRE, flat,
    font_names, licence_text, place, place_with, rows, strs,
};
use xtask::third_party::{self, Outcome};

fn root() -> PathBuf {
    credits::root_of(env!("CARGO_MANIFEST_DIR"))
}

fn committed() -> String {
    fs::read_to_string(root().join(OUT)).expect("the committed notices")
}

fn sidecar() -> Table {
    third_party::load(&root(), third_party::SIDECAR).expect("the sidecar")
}

/// **1. The committed file is the generated one, byte for byte**, and the
/// check passes on the tree. Red under: a byte added to the committed file.
#[test]
fn the_committed_file_is_the_generated_one() {
    let made = credits::generate(&root()).unwrap_or_else(|e| panic!("{}", e.join("\n")));
    assert!(
        made == committed(),
        "{OUT} is stale: run cargo xtask credits"
    );
    let mut out = Vec::new();
    let o = credits::run(&root(), Mode::Check, &mut out).expect("the check runs");
    assert_eq!(o, Outcome::Passed, "{}", String::from_utf8_lossy(&out));
}

/// A directory under the system's temporary directory, removed on drop.
struct Tree(PathBuf);

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().expect("a parent")).expect("its directory");
    fs::write(p, text).expect("written");
}

/// A small workspace: an app crate whose `main.rs` is `main`, with a
/// library it links and one it only builds with, and the files they name.
fn small(main: &str) -> Tree {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("xtask-credits-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    put(
        &root,
        "Cargo.toml",
        "[workspace.dependencies]\nlib-a = { path = \"lib-a\" }\nlib-b = { path = \"lib-b\" }\n",
    );
    put(
        &root,
        "baud-app/Cargo.toml",
        "[dependencies]\nlib-a = { workspace = true }\n[build-dependencies]\nlib-b = { workspace = true }\n",
    );
    put(&root, "baud-app/src/main.rs", main);
    put(
        &root,
        "baud-app/src/helpers.rs",
        "const H: &[u8] = include_bytes!(\"../assets/helper-only.bin\");\n",
    );
    put(&root, "lib-a/Cargo.toml", "[dependencies]\n");
    put(
        &root,
        "lib-a/src/lib.rs",
        "pub const A: &[u8] = include_bytes!(\"../a.bin\");\n",
    );
    put(&root, "lib-b/Cargo.toml", "[dependencies]\n");
    put(
        &root,
        "lib-b/src/lib.rs",
        "pub const B: &[u8] = include_bytes!(\"../unrowed.bin\");\n",
    );
    for f in [
        "baud-app/assets/x.bin",
        "baud-app/assets/own.txt",
        "lib-a/a.bin",
    ] {
        put(&root, f, "bytes");
    }
    Tree(root)
}

const SIDE: &str = r#"
[[tree]]
what     = "x"
paths    = ["baud-app/assets/x.bin", "lib-a/a.bin"]
licence  = "MIT"
notice   = "n"
upstream = "u"
status   = "kept"

[[tree]]
what     = "read only by a test"
paths    = ["baud-app/assets/test-only.bin"]
licence  = "MIT"
notice   = "n"
upstream = "u"
status   = "kept"
"#;

const MAIN: &str = r##"
const X: &[u8] = include_bytes!("../assets/x.bin");
const O: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/own.txt"));
// include_bytes!("../assets/in-a-comment.bin")
const S: &str = "include_bytes!(\"../assets/in-a-string.bin\") }";
const R: &str = r#"} include_str!("../assets/in-a-raw-string.bin")"#;
const C: char = '{';
fn lifetime<'a>(s: &'a str) -> &'a str { s }
#[cfg(test)]
mod helpers;
#[cfg(test)]
mod tests {
    const SELF: &str = include_str!("main.rs");
    const T: &[u8] = include_bytes!("../assets/test-only.bin");
    #[test]
    fn t() { let _ = '}'; }
}
#[cfg(any(test, target_os = "macos"))]
const BOTH: &[u8] = include_bytes!("../assets/x.bin");
"##;

fn own() -> [Own; 1] {
    [Own {
        path: "baud-app/assets/own.txt",
        from: &[],
        why: "the small tree's own",
    }]
}

fn lists(own: &[Own]) -> Lists<'_> {
    Lists {
        own,
        pending: &[],
        must_ship: &["baud-app/assets/x.bin"],
        must_test: &["baud-app/src/main.rs"],
        computed: &[],
    }
}

/// **2. The scan: an embedded file with no row and no own entry fails.**
/// On a small tree it passes: a row's file, one through the manifest's
/// directory, baud's own, a linked crate's, and only test code reading
/// itself, a test-only module and a file no row is needed for; nothing in a
/// comment, a string or a raw string; a build dependency is not the
/// binary. Then one more include in shipped code, of a file no row names,
/// and it fails naming it. Red under: `gates_on_test` answering no, which
/// makes the test-only module's include shipped.
#[test]
fn an_include_with_no_row_fails() {
    let side: Table = SIDE.parse().expect("the sidecar");
    let own = own();
    let t = small(MAIN);
    let p = place_with(&t.0, &side, &lists(&own)).expect("the scan runs");
    assert!(p.errors.is_empty(), "{:?}", p.errors);
    let shipped: Vec<&str> = p.shipped.iter().map(String::as_str).collect();
    assert_eq!(
        shipped,
        [
            "baud-app/assets/own.txt",
            "baud-app/assets/x.bin",
            "lib-a/a.bin"
        ],
        "the comment, the strings and the build dependency are not includes"
    );
    assert!(p.test.contains("baud-app/assets/test-only.bin"));
    assert!(
        p.test.contains("baud-app/assets/helper-only.bin"),
        "a test-only module"
    );
    let reached: Vec<usize> = p.reached.keys().map(|c| c.0).collect();
    assert_eq!(reached, [0], "a row read only by a test is not embedded");

    let t = small(&format!(
        "{MAIN}\nconst N: &[u8] = include_bytes!(\"../assets/new.bin\");\n"
    ));
    let p = place_with(&t.0, &side, &lists(&own)).expect("the scan runs");
    assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
    assert!(
        p.errors[0].contains("baud-app/assets/new.bin is embedded and falls under no row"),
        "{}",
        p.errors[0]
    );

    // And an own entry nothing names any more is stale.
    let t = small(&MAIN.replace("/assets/own.txt", "/assets/x.bin"));
    let p = place_with(&t.0, &side, &lists(&own)).expect("the scan runs");
    assert!(
        p.errors
            .iter()
            .any(|e| e.contains("no shipped include names it")),
        "{:?}",
        p.errors
    );
}

/// A tree whose linked crate compiles in code its build writes, from a data
/// file and a header its build script names; and a build dependency that
/// does the same, which is not the binary.
fn generating(side_names_data: bool) -> (Tree, Table) {
    let t = small(MAIN);
    put(
        &t.0,
        "lib-a/src/lib.rs",
        "pub const A: &[u8] = include_bytes!(\"../a.bin\");\n\
         mod table { include!(concat!(env!(\"OUT_DIR\"), \"/table.rs\")); }\n",
    );
    put(
        &t.0,
        "lib-a/build.rs",
        "fn main() {\n    println!(\"cargo::rerun-if-changed=keys.h\");\n    \
         let _ = std::fs::read(\"keys.h\");\n}\n",
    );
    put(&t.0, "lib-a/data/names.txt", "a name\n");
    put(&t.0, "lib-a/keys.h", "#define KEY 1\n");
    put(
        &t.0,
        "lib-b/src/lib.rs",
        "mod t { include!(concat!(env!(\"OUT_DIR\"), \"/t.rs\")); }\n",
    );
    put(&t.0, "lib-b/data/unrowed.txt", "x\n");
    let mut side = SIDE.to_owned();
    side.push_str(
        "\n[[tree]]\nwhat = \"the header\"\npaths = [\"lib-a/keys.h\"]\nlicence = \"MIT\"\n\
         notice = \"n\"\nupstream = \"u\"\nstatus = \"kept\"\n",
    );
    if side_names_data {
        side.push_str(
            "\n[[tree]]\nwhat = \"the data\"\npaths = [\"lib-a/data/names.txt\"]\n\
             licence = \"MIT\"\nnotice = \"n\"\nupstream = \"u\"\nstatus = \"kept\"\n",
        );
    }
    let side: Table = side.parse().expect("the sidecar");
    (t, side)
}

/// **7. Code a build writes is followed to what the build read (G1).** A
/// shipped crate's `include!` of a file in `OUT_DIR` reaches every file of
/// its `data/` and every file its build script names, and each must fall
/// under a row: with both rowed the tree passes and both rows are reached;
/// with the data file unrowed it fails naming it. A build dependency's
/// generated code is not followed. Red under: the scan reading
/// `include_bytes!` and `include_str!` only, not `include!`.
#[test]
fn code_a_build_writes_is_followed_to_what_it_read() {
    let own = own();
    let (t, side) = generating(true);
    assert_eq!(
        credits::build_inputs(&t.0, "lib-a").expect("the inputs"),
        ["lib-a/data/names.txt", "lib-a/keys.h"]
    );
    let p = place_with(&t.0, &side, &lists(&own)).expect("the scan runs");
    assert!(p.errors.is_empty(), "{:?}", p.errors);
    let reached: Vec<usize> = p.reached.keys().map(|c| c.0).collect();
    assert_eq!(reached, [0, 2, 3], "the header's row and the data's");
    assert!(
        !p.shipped.iter().any(|s| s.contains("lib-b")),
        "a build dependency's generated code: {:?}",
        p.shipped
    );

    let (t, side) = generating(false);
    let p = place_with(&t.0, &side, &lists(&own)).expect("the scan runs");
    assert_eq!(p.errors.len(), 1, "{:?}", p.errors);
    assert!(
        p.errors[0].contains("lib-a's build reads lib-a/data/names.txt")
            && p.errors[0].contains("falls under no row"),
        "{}",
        p.errors[0]
    );
}

/// **10. Every input a shipped crate's build says it reads is followed.**
/// Each `rerun-if-changed=` a shipped crate's build sources print, read from
/// the sources rather than a build's output, names a file the scan follows,
/// a row's or baud's own, a file under a folder the scan reads whole, or a
/// computed value COMPUTED says what it is. On the tree every print passes,
/// baud-domain's `data/{file}` and baud-xkb's computed path among them; on a
/// small tree a print of a file nothing names fails, and so does a computed
/// one nothing lists. Red under: a `rerun-if-changed` added to
/// baud-domain's build for a file no row names.
#[test]
fn every_input_a_build_says_it_reads_is_followed() {
    let side = sidecar();
    let p = place(&root(), &side).expect("the scan runs");
    assert!(p.errors.is_empty(), "{:?}", p.errors);
    let domain = credits::build_prints(&root(), "baud-domain").expect("the prints");
    assert!(
        domain.iter().any(|pr| pr.printed == "data/{file}"),
        "{domain:?}"
    );
    let xkb = credits::build_prints(&root(), "baud-xkb").expect("the prints");
    assert!(
        xkb.iter()
            .any(|pr| pr.printed == "{}" && pr.arg.as_deref() == Some("path.display()")),
        "{xkb:?}"
    );

    let own = own();
    let (t, side) = generating(true);
    put(
        &t.0,
        "lib-a/build.rs",
        "fn main() {\n    println!(\"cargo::rerun-if-changed=keys.h\");\n    \
         println!(\"cargo::rerun-if-changed=extra/unlisted.txt\");\n    \
         let other = std::path::Path::new(\"x\");\n    \
         println!(\"cargo:rerun-if-changed={}\", other.display());\n}\n",
    );
    put(&t.0, "lib-a/extra/unlisted.txt", "x\n");
    let p = place_with(&t.0, &side, &lists(&own)).expect("the scan runs");
    assert_eq!(p.errors.len(), 2, "{:?}", p.errors);
    assert!(
        p.errors
            .iter()
            .any(|e| e.contains("lib-a/extra/unlisted.txt is no input the scan follows")),
        "{:?}",
        p.errors
    );
    assert!(
        p.errors.iter().any(|e| e.contains("of other.display()")),
        "{:?}",
        p.errors
    );
}

/// The notices and texts of a notices file, by heading: each section's
/// heading, from "1. " to "X. ", one line or two, and its words, flat.
fn notices(text: &str) -> std::collections::BTreeMap<String, String> {
    let ruled = |l: &str| l == "=".repeat(76) || l == "-".repeat(76);
    let heading = |l: &str| {
        l.split_once(". ").is_some_and(|(n, _)| {
            !n.is_empty()
                && (n.bytes().all(|b| b.is_ascii_digit())
                    || n.bytes().all(|b| b.is_ascii_uppercase()))
        })
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut out = std::collections::BTreeMap::new();
    let mut at: Option<(String, Vec<&str>)> = None;
    let mut k = 0;
    while k < lines.len() {
        let end = (k + 2..=k + 3).find(|&j| lines.get(j).is_some_and(|l| ruled(l)));
        if let (true, true, Some(j)) = (
            ruled(lines[k]),
            heading(lines.get(k + 1).copied().unwrap_or("")),
            end,
        ) {
            if let Some((t, body)) = at.take() {
                out.insert(t, flat(&body.join("\n")));
            }
            at = Some((flat(&lines[k + 1..j].join(" ")), Vec::new()));
            k = j + 1;
            continue;
        }
        if let Some((_, body)) = at.as_mut() {
            if !ruled(lines[k]) {
                body.push(lines[k]);
            }
        }
        k += 1;
    }
    if let Some((t, body)) = at {
        out.insert(t, flat(&body.join("\n")));
    }
    out
}

/// **11. THIRD_PARTY.md and the notices file carry one set of notices**:
/// the page the About window's acknowledgements line opens holds, in its
/// first section, every notice and licence text `baud --credits` prints and
/// no other, each compared word for word, layout aside, both ways. They
/// agree because `cargo xtask third-party` writes that section from
/// CREDITS.txt, the file `cargo xtask credits` writes. Red under: a notice
/// dropped from THIRD_PARTY.md.
#[test]
fn third_party_md_and_the_notices_file_carry_one_set_of_notices() {
    let credits = notices(&committed());
    let page = fs::read_to_string(root().join(third_party::OUT)).expect("THIRD_PARTY.md");
    let section = page
        .split_once(&format!("\n{}\n", third_party::NOTICES))
        .map(|(_, rest)| rest)
        .expect("THIRD_PARTY.md's notices section");
    let fenced = section
        .split_once("```text\n")
        .and_then(|(_, r)| r.split_once("\n```\n"))
        .map(|(f, _)| f)
        .expect("its text");
    let md = notices(fenced);
    assert!(credits.len() >= 60, "{} sections", credits.len());
    for (title, body) in &credits {
        match md.get(title) {
            Some(b) => assert!(b == body, "{title}: differs in THIRD_PARTY.md"),
            None => panic!("{title}: not in THIRD_PARTY.md"),
        }
    }
    for title in md.keys() {
        assert!(
            credits.contains_key(title),
            "{title}: in THIRD_PARTY.md alone"
        );
    }
}

/// **8. Every transcribed row is in the file (G2)**: each `[[tree]]` row
/// marked `transcribed = true` names the files it was ported from, and its
/// notice is in the file with its title, those files, every copyright line
/// of its credit and of its texts, and a sentence its licence asks for, as
/// the IJG's does, read from the committed file. Red under: the
/// transcribed rows' notices left out of the file.
#[test]
fn every_transcribed_row_is_in_the_file() {
    let made = committed();
    let flat_text = flat(&made);
    let side = sidecar();
    let mut marked = 0;
    for (i, r) in rows(&side).expect("the rows").iter().enumerate() {
        if r.get("transcribed").and_then(toml::Value::as_bool) != Some(true) {
            continue;
        }
        marked += 1;
        let ported = strs(r, "ported_from");
        assert!(!ported.is_empty(), "row {i}: no ported_from");
        let c = r
            .get("credit")
            .and_then(toml::Value::as_table)
            .unwrap_or_else(|| panic!("row {i}: transcribed with no credit"));
        let title = c
            .get("title")
            .and_then(toml::Value::as_str)
            .expect("a title");
        let head = made
            .lines()
            .find(|s| s.starts_with(char::is_numeric) && flat(s).ends_with(&flat(title)))
            .unwrap_or_else(|| panic!("row {i}: {title} is not in {OUT}"));
        let b = block(&made, head);
        let ported_line: String = b
            .lines()
            .skip_while(|s| !s.starts_with("Ported from: "))
            .take_while(|s| s.starts_with("Ported from: ") || s.starts_with(' '))
            .collect::<Vec<_>>()
            .join(" ");
        for f in &ported {
            assert!(
                ported_line.contains(f.as_str()),
                "row {i}: {f} in {ported_line:?}"
            );
        }
        let mut lines = strs(c, "copyright");
        for t in c
            .get("text")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            lines.extend(strs(t.as_table().expect("a text"), "copyright"));
        }
        if c.get("none").is_none() {
            assert!(!lines.is_empty(), "row {i}: no copyright line");
        }
        for l in &lines {
            assert!(flat(b).contains(&flat(l)), "row {i}: {l}");
        }
        if let Some(st) = c.get("statement").and_then(toml::Value::as_str) {
            assert!(flat_text.contains(&flat(st)), "row {i}: {st}");
        }
    }
    assert!(marked >= 12, "{marked} transcribed rows");
    assert!(
        flat_text.contains(
            "As its licence asks: This software is based in part on the work of the \
             Independent JPEG Group."
        ),
        "the IJG's sentence"
    );
}

/// The licence-text sections of the file: each one's heading, the copyright
/// lines listed above its text, and its text.
fn texts(made: &str) -> Vec<(String, String, String)> {
    let thin = "-".repeat(76);
    let rule = "=".repeat(76);
    let tail = &made[made
        .find(&format!("{rule}\nLicence texts\n{rule}\n"))
        .expect("the texts")..];
    let parts: Vec<&str> = tail.split(&format!("\n{thin}\n")).collect();
    let mut out = Vec::new();
    for pair in parts[1..].chunks(2) {
        let [head, rest] = pair else {
            panic!("a heading with no text");
        };
        let (under, body) = match rest.strip_prefix("The works under it") {
            Some(r) => r.split_once("\n\n").expect("the list, then the text"),
            None => ("", *rest),
        };
        out.push((head.to_string(), under.to_owned(), body.to_owned()));
    }
    out
}

/// **9. Each permission text once, with every copyright line above it
/// (G3).** No two licence texts in the file hold the same words, whitespace
/// aside; every copyright line of every notice-file text a credit gives is
/// listed above the text it falls under; and some text is shared, listing
/// the copyright lines of more than one notice, as libxkbcommon's and the
/// Wayland files' MIT text is. Red under: texts not merged, each notice's
/// added as its own.
#[test]
fn each_permission_text_is_printed_once_with_every_copyright_line_above_it() {
    let made = committed();
    let all = texts(&made);
    assert!(all.len() >= 20, "{} texts", all.len());
    let mut seen = std::collections::BTreeMap::new();
    for (head, _, body) in &all {
        if let Some(other) = seen.insert(flat(body), head.clone()) {
            panic!("{head} holds the same text as {other}");
        }
    }
    let side = sidecar();
    for r in rows(&side).expect("the rows") {
        let Some(c) = r.get("credit").and_then(toml::Value::as_table) else {
            continue;
        };
        for t in c
            .get("text")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            let t = t.as_table().expect("a text");
            let title = t
                .get("title")
                .and_then(toml::Value::as_str)
                .expect("a title");
            for l in strs(t, "copyright") {
                assert!(
                    all.iter()
                        .any(|(_, under, _)| flat(under).contains(&flat(&l))),
                    "{title}: {l} is listed above no text"
                );
            }
        }
    }
    let shared = all
        .iter()
        .filter(|(_, under, _)| {
            let labels: std::collections::BTreeSet<&str> = under
                .lines()
                .filter_map(|s| s.strip_prefix("  "))
                .filter_map(|s| s.split_whitespace().next())
                .filter(|w| w.ends_with('.') && w.starts_with(char::is_numeric))
                .collect();
            labels.len() > 1
        })
        .count();
    assert!(shared >= 3, "{shared} texts shared by two notices or more");
}

/// The lines of the part of the file from the line starting `head` to the
/// next blank line followed by a numbered heading.
fn block<'a>(text: &'a str, head: &str) -> &'a str {
    let at = text
        .find(&format!("\n{head}"))
        .unwrap_or_else(|| panic!("no {head:?}"))
        + 1;
    let rest = &text[at..];
    let end = rest[1..].find("\n\n").map_or(rest.len(), |e| e + 1);
    &rest[..end]
}

/// **3. Every embedded row's notice is in the file, each licence named is
/// in it once, and each CC BY work carries each part of its section
/// 3(a)(1)**, read as text. The OFL, MIT and Apache 2.0 are there in full;
/// CC BY 4.0 by its URI, its legal code not; each embedded font's own
/// copyright and trademark notices too. The CC BY-ND work carries the
/// same parts. Red under: a credit's copyright line dropped from the
/// render, or a CC BY part's label; the disclaimer printed for CC BY 4.0
/// alone.
#[test]
fn every_embedded_rows_notice_and_licence_is_in_the_file() {
    let text = committed();
    let flat_text = flat(&text);
    let side = sidecar();
    let placed = place(&root(), &side).expect("the scan runs");
    assert!(placed.errors.is_empty(), "{:?}", placed.errors);
    let rows = rows(&side).expect("the rows");
    let mut named: Vec<String> = Vec::new();
    let mut cc_by: Vec<String> = Vec::new();
    for (row, files) in &placed.reached {
        let r = rows[row.0];
        let c = r
            .get("credit")
            .and_then(toml::Value::as_table)
            .unwrap_or_else(|| panic!("row {} is embedded and has no credit", row.0));
        let title = c
            .get("title")
            .and_then(toml::Value::as_str)
            .expect("a title");
        assert!(flat_text.contains(&flat(title)), "{title}: its notice");
        for line in strs(c, "copyright") {
            assert!(flat_text.contains(&flat(&line)), "{title}: {line}");
        }
        named.extend(strs(c, "licence"));
        for p in c
            .get("part")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            let p = p.as_table().expect("a part");
            let name = p.get("name").and_then(toml::Value::as_str).expect("a name");
            for line in strs(p, "copyright") {
                assert!(flat_text.contains(&flat(&line)), "{name}: {line}");
            }
            let ls = strs(p, "licence");
            let cc = |key: &String| {
                LICENCES
                    .iter()
                    .any(|l| l.key == key.as_str() && l.disclaimer.is_some())
            };
            if ls.iter().any(cc) {
                cc_by.push(name.to_owned());
            }
            named.extend(ls);
        }
        for f in files
            .iter()
            .filter(|f| f.ends_with(".ttf") || f.ends_with(".otf"))
        {
            let bytes = fs::read(root().join(f)).expect("the font");
            for id in [NameId(0), NameId(7)] {
                for n in font_names(&bytes, id) {
                    assert!(flat_text.contains(&flat(&n)), "{f}'s name ID {}: {n}", id.0);
                }
            }
            assert!(
                !font_names(&bytes, NameId(0)).is_empty(),
                "{f}: a copyright notice"
            );
        }
    }
    named.sort();
    named.dedup();
    for key in &named {
        let l = LICENCES
            .iter()
            .find(|l| l.key == key)
            .unwrap_or_else(|| panic!("{key}: no such licence"));
        let heads = text
            .lines()
            .filter(|s| s.get(1..3) == Some(". ") && s.get(3..) == Some(l.title))
            .count();
        assert_eq!(heads, 1, "{key}: its text once");
        let body = licence_text(&root(), l).expect("its text");
        match l.source {
            Source::File { .. } => assert!(text.contains(&body), "{key}: in full"),
            Source::Uri { uri, .. } => assert!(text.contains(uri), "{key}: its URI"),
        }
    }
    for key in ["OFL-1.1", "MIT", "Apache-2.0", "CC-BY-4.0"] {
        assert!(named.iter().any(|n| n == key), "{key} is named");
    }
    assert!(
        text.contains("TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION"),
        "Apache 2.0 in full"
    );
    assert!(
        !text.contains("Section 1 -- Definitions."),
        "CC BY's legal code is not"
    );
    assert_eq!(
        cc_by,
        [
            "Codicons 0.0.45",
            "Font Awesome 6.5.1",
            "Technical Note #5176's CFF Standard Encoding (Appendix B)"
        ],
        "the Creative Commons works"
    );
    for name in &cc_by {
        let head = text
            .lines()
            .find(|s| s.ends_with(name.as_str()) && s.starts_with(char::is_numeric))
            .unwrap_or_else(|| panic!("{name}: its notice"));
        let b = block(&text, head);
        for part in CC_BY_PARTS {
            let line = b
                .lines()
                .find(|s| s.trim_start().starts_with(part))
                .unwrap_or_else(|| panic!("{name}: no {part}\n{b}"));
            assert!(
                line.trim_start()[part.len()..].trim().len() > 3,
                "{name}: {part} is empty"
            );
        }
    }
    assert!(text.starts_with(credits::HEADER), "the header line");
}

/// **6. The size tripwire**: the file is under it; a file past it fails
/// with its size and its largest sections, the one that grew first, and
/// nothing is cut. If it ever trips, the fix is to raise [`TRIPWIRE`] in a
/// commit that records why the file grew, never to trim a notice or a
/// licence text to fit under it. Every licence text is a section of its
/// own. Red under: the tripwire set below the file's size; a heading read
/// on one line only, so a title that wraps folds into the section before.
#[test]
fn the_size_tripwire_fires_and_says_where() {
    let text = committed();
    assert_eq!(TRIPWIRE, Bytes(112 * 1024));
    assert_eq!(
        credits::tripwire(&text, TRIPWIRE),
        None,
        "{} bytes",
        text.len()
    );
    let rule = "=".repeat(76);
    let big = format!(
        "{text}\n{rule}\n99. A licence pasted whole by mistake\n{rule}\n{}\n",
        "x".repeat(40 * 1024)
    );
    let said = credits::tripwire(&big, TRIPWIRE).expect("it fires");
    assert!(said.contains(&format!("is {} bytes", big.len())), "{said}");
    let first = said.lines().nth(1).expect("the largest section");
    assert!(
        first.ends_with("99. A licence pasted whole by mistake"),
        "{said}"
    );
    assert!(said.contains("never cut"), "{said}");
    // Every licence text is a section of its own, its heading on two lines
    // or one.
    let index: Vec<&str> = text
        .split("\nLicence texts\n")
        .nth(1)
        .expect("the index")
        .lines()
        .take_while(|s| !s.is_empty())
        .filter(|s| !s.starts_with("     "))
        .collect();
    let lettered = credits::sections(&text)
        .into_iter()
        .filter(|(t, _)| {
            t.split_once(". ")
                .is_some_and(|(n, _)| n.bytes().all(|b| b.is_ascii_uppercase()))
        })
        .count();
    assert_eq!(lettered, index.len(), "{index:?}");
}
