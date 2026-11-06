//! `cargo xtask features-since` on small workspaces: what its lexer tells
//! apart, what its scan finds, and what a change reaches.
//!
//! Every expected output below was also the output of the script this
//! replaced, but for the usage line and the plain `include!`, which the
//! script never returned on.

mod common;

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use common::Tree;
use xtask::features_since::{At, closing, lex, resolve, split_top, walk};
use xtask::py_path::{basename, dirname, join, normpath, relpath, stem};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Ran {
    out: String,
    code: Option<i32>,
}

/// A git repository holding a workspace, run in by the real command.
struct Repo(Tree);

impl Repo {
    /// The crates, each with what its manifest adds after `[package]`.
    fn new(crates: &[(&str, &str)]) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = env::temp_dir().join(format!("xtask-since-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("the tree");
        let r = Self(Tree(dir));
        let names: Vec<String> = crates.iter().map(|(c, _)| format!("{c:?}")).collect();
        r.put(
            "Cargo.toml",
            &format!(
                "[workspace]\nmembers = [{}]\nexclude = [\"outside\"]\nresolver = \"2\"\n",
                names.join(", ")
            ),
        );
        for (c, extra) in crates {
            r.put(
                &format!("{c}/Cargo.toml"),
                &format!(
                    "[package]\nname = {c:?}\nversion = \"0.1.0\"\nedition = \"2021\"\n{extra}"
                ),
            );
            r.put(&format!("{c}/src/lib.rs"), "");
        }
        r.git(&["init", "-q", "."]);
        r
    }

    fn one() -> Self {
        Self::new(&[("a", "")])
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.0.join(rel)
    }

    fn put(&self, rel: &str, text: &str) {
        self.put_bytes(rel, text.as_bytes());
    }

    fn put_bytes(&self, rel: &str, bytes: &[u8]) {
        let p = self.path(rel);
        fs::create_dir_all(p.parent().expect("a parent")).expect("the directory");
        fs::write(p, bytes).expect("the file");
    }

    fn scrub(c: &mut Command) -> &mut Command {
        for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX"] {
            c.env_remove(v);
        }
        c.env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Self::scrub(
            Command::new("git")
                .args(["-c", "user.name=x", "-c", "user.email=x@example.org"])
                .args(["-c", "gc.auto=0", "-c", "maintenance.auto=false"])
                .args(args)
                .current_dir(&self.0.0),
        )
        .output()
        .expect("git");
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
    }

    fn run_in(&self, dir: &Path, path: Option<&str>, args: &[&str]) -> Ran {
        let bin = env!("CARGO_BIN_EXE_xtask");
        let mut c = Command::new(bin);
        c.arg("features-since").args(args).current_dir(dir);
        Self::scrub(&mut c);
        if let Some(p) = path {
            c.env("PATH", p);
        }
        let out = c.stderr(Stdio::null()).output().expect("xtask");
        Ran {
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            code: out.status.code(),
        }
    }

    fn run(&self, args: &[&str]) -> Ran {
        self.run_in(&self.0.0, None, args)
    }

    fn explain(&self) -> String {
        let r = self.run(&["--explain"]);
        assert_eq!(r.code, Some(0), "{}", r.out);
        r.out
    }

    /// `--explain`, killed and failed if it has not ended in 30 seconds.
    fn explain_within_a_limit(&self) -> String {
        let mut c = Command::new(env!("CARGO_BIN_EXE_xtask"));
        c.args(["features-since", "--explain"])
            .current_dir(&self.0.0)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = Self::scrub(&mut c).spawn().expect("xtask");
        let started = Instant::now();
        while child.try_wait().expect("the child").is_none() {
            if started.elapsed() > Duration::from_secs(30) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the scan did not end");
            }
            thread::sleep(Duration::from_millis(10));
        }
        let out = child.wait_with_output().expect("its output");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn since(&self, rev: &str) -> String {
        let r = self.run(&[rev]);
        assert_eq!(r.code, Some(0), "{}", r.out);
        r.out
    }
}

/// `a` alone with this as its library's source: what `--explain` says after
/// its two header lines.
fn scan_lib(src: &str) -> String {
    let r = Repo::one();
    r.put("a/src/lib.rs", src);
    below_header(&r.explain())
}

fn below_header(explain: &str) -> String {
    explain.split_once("depends on: -\n").map_or_else(
        || panic!("no header: {explain}"),
        |(_, rest)| rest.to_string(),
    )
}

fn read_of(target: &str, by: &str) -> String {
    format!("  reads {target}  <- {by}\n")
}

// ── The lexer ───────────────────────────────────────────────────────────

fn lits(src: &str) -> Vec<String> {
    lex(src).lits
}

fn code(src: &str) -> String {
    lex(src).code
}

#[test]
fn a_line_comment_ends_at_the_newline_it_leaves() {
    let l = lex("a // \"hidden\"\nb");
    assert_eq!(l.code, "a \nb");
    assert!(l.lits.is_empty());
    assert_eq!(code("a // to the end"), "a ");
}

#[test]
fn block_comments_nest_and_leave_a_space() {
    let l = lex("a/* x /* \"y\" */ \"z\" */b");
    assert_eq!(l.code, "a b");
    assert!(l.lits.is_empty());
    assert_eq!(code("a/* /* one close */ to the end"), "a ");
}

#[test]
fn a_comment_in_a_string_is_the_string() {
    assert_eq!(lits("\"// a /* b\" c"), ["// a /* b"]);
    assert_eq!(code("\"// a /* b\" c"), "\u{1}0\u{2} c");
}

#[test]
fn a_string_in_a_comment_does_not_open_one() {
    assert_eq!(lits("// \" \n\"real\""), ["real"]);
}

#[test]
fn literals_are_numbered_in_order() {
    let l = lex("\"a\" r\"b\" \"c\"");
    assert_eq!(l.code, "\u{1}0\u{2} \u{1}1\u{2} \u{1}2\u{2}");
    assert_eq!(l.lits, ["a", "b", "c"]);
}

#[test]
fn a_string_undoes_two_escapes_and_keeps_the_rest() {
    assert_eq!(lits(r#""a\"b\\c\nd\x2e""#), ["a\"b\\c\\nd\\x2e"]);
    assert_eq!(lits("\"a\\\n  b\""), ["a\\\n  b"]);
}

#[test]
fn an_unterminated_string_runs_to_the_end() {
    assert_eq!(lits("\"abc"), ["abc"]);
    assert_eq!(lits("\"abc\\"), ["abc\\"]);
}

#[test]
fn a_raw_string_ends_at_its_own_hashes() {
    assert_eq!(lits(r##"r#"a"b"# c"##), ["a\"b"]);
    assert_eq!(lits(r###"r##"a"#b"## c"###), ["a\"#b"]);
    assert_eq!(lits(r#"r"a\" c"#), ["a\\"]);
    assert_eq!(code(r##"r#"a"# c"##), "\u{1}0\u{2} c");
}

#[test]
fn an_unterminated_raw_string_runs_to_the_end() {
    let l = lex("r#\"a\" b");
    assert_eq!(l.lits, ["a\" b"]);
    assert_eq!(l.code, "\u{1}0\u{2}");
}

#[test]
fn raw_prefixes_are_consumed_and_plain_prefixes_are_not() {
    assert_eq!(code(r#"br"a" cr"b""#), "\u{1}0\u{2} \u{1}1\u{2}");
    assert_eq!(code(r#"b"a" c"b""#), "b\u{1}0\u{2} c\u{1}1\u{2}");
    assert_eq!(lits(r##"br#"a"b"#"##), ["a\"b"]);
}

/// `(?<![A-Za-z0-9_])`: an identifier that ends in `r` opens a plain string
/// at its quote, so a backslash there is an escape, not text.
#[test]
fn a_word_before_r_makes_it_no_raw_string() {
    assert_eq!(lits(r#"bar"a\"b""#), ["a\"b"]);
    assert_eq!(lits(r#"xbr"a\"b""#), ["a\"b"]);
    assert_eq!(lits(r#"_r"a\"b""#), ["a\"b"]);
    assert_eq!(lits(r#"1r"a\"b""#), ["a\"b"]);
}

/// The look-behind's class is ASCII: a letter past it does not stop a raw
/// string opening.
#[test]
fn a_letter_past_ascii_before_r_leaves_it_raw() {
    assert_eq!(lits(r#"ér"a\" c"#), ["a\\"]);
    assert_eq!(code(r#"ér"a\" c"#), "é\u{1}0\u{2} c");
}

#[test]
fn a_literal_before_r_leaves_it_raw() {
    assert_eq!(lits(r#""a"r"b\" c"#), ["a", "b\\"]);
}

#[test]
fn hashes_with_no_quote_open_nothing() {
    assert_eq!(code("r#type r## x"), "r#type r## x");
}

#[test]
fn a_character_literal_may_hold_a_quote() {
    let l = lex("'\"' \"a\"");
    assert_eq!(l.lits, ["a"]);
    assert_eq!(l.code, "' ' \u{1}0\u{2}");
    assert_eq!(code("b'\"' x"), "b' ' x");
}

#[test]
fn an_escaped_character_literal_ends_at_its_closing_quote() {
    assert_eq!(code(r"'\'' x"), "' ' x");
    assert_eq!(code(r"'\\' x"), "' ' x");
    assert_eq!(code(r"'\u{1F600}' x"), "' ' x");
    assert_eq!(lits(r#"'\"' "a""#), ["a"]);
}

#[test]
fn an_escape_with_no_closing_quote_ends_the_text() {
    assert_eq!(code(r"'\n and on"), "' '");
    assert_eq!(code(r"'\"), "' '");
}

/// Two code points on, not two bytes: `'é'` is a literal, and read in bytes
/// its quotes would stay for the next string to be misread after.
#[test]
fn a_character_literal_past_ascii_is_counted_in_code_points() {
    assert_eq!(code("'é' x"), "' ' x");
    assert_eq!(code("'€' x"), "' ' x");
    assert_eq!(code("'😀' x"), "' ' x");
}

#[test]
fn a_lifetime_leaves_its_quote_and_what_follows() {
    assert_eq!(code("fn f<'a>(x: &'a str)"), "fn f<'a>(x: &'a str)");
    let l = lex("&'a \"s\"");
    assert_eq!(l.lits, ["s"]);
    assert_eq!(l.code, "&'a \u{1}0\u{2}");
}

/// The script's rule, a quote two on, reads `'a'` in a lifetime list too.
#[test]
fn a_quote_two_on_is_a_literal_even_in_a_lifetime_list() {
    assert_eq!(code("<'a,'b>"), "<'a,'b>");
    assert_eq!(code("'a'b"), "' 'b");
    assert_eq!(code("''' x"), "' ' x");
}

#[test]
fn closing_counts_the_three_kinds_together_and_stops_at_the_end() {
    assert_eq!(closing("(a[b]{c})d", At(0)), At(8));
    assert_eq!(closing("(a]", At(0)), At(2));
    assert_eq!(closing("(a", At(0)), At(2));
}

#[test]
fn split_top_splits_at_depth_zero_strips_and_drops_the_empty() {
    assert_eq!(
        split_top(" a , (b,c) ,, [d,e]\u{1c}"),
        ["a", "(b,c)", "[d,e]"]
    );
    assert_eq!(split_top(") , a"), [") , a"]);
    assert!(split_top(" ").is_empty());
}

// ── `os.path` ───────────────────────────────────────────────────────────

#[test]
fn normpath_is_posixpath_s() {
    for (p, want) in [
        ("", "."),
        (".", "."),
        ("a/./b", "a/b"),
        ("a//b/", "a/b"),
        ("a/../b", "b"),
        ("../a", "../a"),
        ("a/../..", ".."),
        ("../..", "../.."),
        ("/..", "/"),
        ("/../a", "/a"),
        ("//a", "//a"),
        ("///a", "/a"),
        ("/", "/"),
    ] {
        assert_eq!(normpath(p), want, "{p}");
    }
}

#[test]
fn join_dirname_basename_and_stem_are_posixpath_s() {
    assert_eq!(join("", "a"), "a");
    assert_eq!(join("a", ""), "a/");
    assert_eq!(join("a/", "b"), "a/b");
    assert_eq!(join("a", "/b"), "/b");
    assert_eq!(dirname("a"), "");
    assert_eq!(dirname("a/b"), "a");
    assert_eq!(dirname("a//b"), "a");
    assert_eq!(dirname("/a"), "/");
    assert_eq!(dirname("//a"), "//");
    assert_eq!(basename("a/b"), "b");
    assert_eq!(basename("a/"), "");
    assert_eq!(stem("a.rs"), "a");
    assert_eq!(stem("a.b.rs"), "a.b");
    assert_eq!(stem(".rs"), ".rs");
    assert_eq!(stem("..rs"), "..rs");
    assert_eq!(stem("x.y/z"), "x.y/z");
    assert_eq!(stem("x/.rs"), "x/.rs");
}

#[test]
fn relpath_and_resolve_stay_in_or_leave_the_repository() {
    assert_eq!(relpath("/a/b", "/a"), "b");
    assert_eq!(relpath("/a", "/a"), ".");
    assert_eq!(relpath("/a/c", "/a/b"), "../c");
    assert_eq!(relpath("/ab", "/a"), "../ab");
    assert_eq!(resolve("a/src", "../x"), Some("a/x".to_string()));
    assert_eq!(resolve("a", ".."), Some(String::new()));
    assert_eq!(resolve("a", "../.."), None);
    assert_eq!(resolve("a", "/abs"), None);
    assert_eq!(resolve("", ".."), None);
}

// ── What an include names ───────────────────────────────────────────────

#[test]
fn an_include_is_relative_to_the_file_it_is_written_in() {
    assert_eq!(
        scan_lib("const A: &str = include_str!(\"../../data/x.txt\");\n"),
        read_of("data/x.txt", "a/src/lib.rs (include_str!)")
    );
    assert_eq!(
        scan_lib("const A: &[u8] = include_bytes!(\"../../data/x.bin\");\n"),
        read_of("data/x.bin", "a/src/lib.rs (include_bytes!)")
    );
}

#[test]
fn an_include_in_its_own_crate_is_not_a_read() {
    assert_eq!(scan_lib("const A: &str = include_str!(\"x.txt\");\n"), "");
}

#[test]
fn an_include_leaving_the_repository_is_nothing() {
    assert_eq!(
        scan_lib("const A: &str = include_str!(\"../../../x\");\n"),
        ""
    );
}

#[test]
fn an_include_of_the_root_reads_anything() {
    assert_eq!(
        scan_lib("const A: &str = include_str!(\"../..\");\n"),
        "  reads anything: a/src/lib.rs (include_str!) names the repository root\n"
    );
}

#[test]
fn a_comment_hides_an_include_and_a_string_is_no_call() {
    let src = "// include_str!(\"../../c.txt\")\n/* include_str!(\"../../d.txt\") */\n\
               const S: &str = \"include_str!(\\\"../../e.txt\\\")\";\n";
    assert_eq!(scan_lib(src), "");
}

#[test]
fn a_character_literal_quote_does_not_hide_the_include_after_it() {
    let src = "const Q: char = '\"';\nconst A: &str = include_str!(\"../../x.txt\");\n";
    assert_eq!(
        scan_lib(src),
        read_of("x.txt", "a/src/lib.rs (include_str!)")
    );
}

#[test]
fn a_raw_string_path_is_a_path() {
    assert_eq!(
        scan_lib("const A: &str = include_str!(r#\"../../x.txt\"#);\n"),
        read_of("x.txt", "a/src/lib.rs (include_str!)")
    );
}

#[test]
fn the_manifest_dir_is_the_crate_and_its_slash_is_dropped() {
    assert_eq!(
        scan_lib(
            "const A: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../d/x.txt\"));\n"
        ),
        read_of("d/x.txt", "a/src/lib.rs (include_str!)")
    );
}

#[test]
fn nested_concats_are_flattened() {
    assert_eq!(
        scan_lib(
            "const A: &str = include_str!(concat!(concat!(\"../../\", \"d/\"), \"x.txt\"));\n"
        ),
        read_of("d/x.txt", "a/src/lib.rs (include_str!)")
    );
}

/// A concat that is the only argument opens at any depth; beside another
/// argument, one level, and what is left inside is a piece it cannot read,
/// printed with its literal's placeholder.
#[test]
fn a_concat_opens_all_the_way_alone_and_one_level_beside_another() {
    assert_eq!(
        scan_lib(
            "const A: &str = include_str!(concat!(concat!(concat!(\"../../d/\", \"x.txt\"))));\n"
        ),
        read_of("d/x.txt", "a/src/lib.rs (include_str!)")
    );
    assert_eq!(
        scan_lib(
            "const A: &str = include_str!(concat!(concat!(concat!(\"../../\"), \"d/\"), \"x.txt\"));\n"
        ),
        "  reads anything: a/src/lib.rs (include_str!) spells its path with concat!(\u{1}0\u{2})\n"
    );
}

#[test]
fn out_dir_is_the_build_script_s_output() {
    assert_eq!(
        scan_lib("const A: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/../../x\"));\n"),
        ""
    );
}

#[test]
fn another_env_var_and_a_late_env_read_anything() {
    assert_eq!(
        scan_lib("const A: &str = include_str!(concat!(env!(\"HOME\"), \"/x\"));\n"),
        "  reads anything: a/src/lib.rs (include_str!) spells its path with env!(HOME)\n"
    );
    assert_eq!(
        scan_lib("const A: &str = include_str!(concat!(\"x\", env!(\"OUT_DIR\")));\n"),
        "  reads anything: a/src/lib.rs (include_str!) spells its path with env!(OUT_DIR)\n"
    );
}

/// `' '.join(part.split())[:40]`, in code points.
#[test]
fn a_path_it_cannot_spell_is_named_in_forty_characters() {
    assert_eq!(
        scan_lib(
            "const A: &str = include_str!(PATH  \n .é_ccccccccccccccccccccccccccccccccccccccccc);\n"
        ),
        "  reads anything: a/src/lib.rs (include_str!) spells its path with PATH .é_cccccccccccccccccccccccccccccccc\n"
    );
}

// ── Inside macro_rules! ─────────────────────────────────────────────────

#[test]
fn a_macro_s_include_resolves_from_every_invoking_file() {
    let r = Repo::one();
    let def = "macro_rules! m { () => { include_str!(\"../x.txt\") } }\n";
    r.put("a/src/lib.rs", def);
    r.put("a/src/h.rs", "const B: &str = m!();\n");
    assert_eq!(below_header(&r.explain()), "");
    r.put("a/g.rs", "const C: &str = m!();\n");
    assert_eq!(
        below_header(&r.explain()),
        read_of("x.txt", "a/src/lib.rs (include_str!)")
    );
}

#[test]
fn a_macro_invoked_inside_a_macro_resolves_from_the_outer_invocation() {
    let r = Repo::one();
    r.put(
        "a/src/lib.rs",
        "macro_rules! inner { () => { include_str!(\"../x.txt\") } }\n\
         macro_rules! outer { () => { inner!() } }\n",
    );
    assert_eq!(below_header(&r.explain()), "");
    r.put("a/g.rs", "const C: &str = outer!();\n");
    assert_eq!(
        below_header(&r.explain()),
        read_of("x.txt", "a/src/lib.rs (include_str!)")
    );
}

#[test]
fn a_metavariable_stays_when_every_invocation_is_literals_below() {
    let src = "macro_rules! m { ($p:expr) => { include_str!(concat!(\"../../d/\", $p)) } }\n\
               const A: &str = m!(\"x.txt\");\nconst B: &str = m!(\"y.txt\");\n";
    assert_eq!(scan_lib(src), read_of("d", "a/src/lib.rs (include_str!)"));
}

/// The invocation's literal climbs, so it is also a macro's literal that
/// names a path.
#[test]
fn a_metavariable_that_climbs_reads_anything() {
    let src = "macro_rules! m { ($p:expr) => { include_str!(concat!(\"../../d/\", $p)) } }\n\
               const A: &str = m!(\"../x.txt\");\n";
    assert_eq!(
        scan_lib(src),
        format!(
            "{}{}",
            read_of("x.txt", "a/src/lib.rs (m!)"),
            "  reads anything: a/src/lib.rs (include_str!) spells its path with $p\n"
        )
    );
}

#[test]
fn a_metavariable_given_a_name_reads_anything() {
    let src = "macro_rules! m { ($p:expr) => { include_str!(concat!(\"../../d/\", $p)) } }\n\
               const A: &str = m!(NAME);\n";
    assert_eq!(
        scan_lib(src),
        "  reads anything: a/src/lib.rs (include_str!) spells its path with $p\n"
    );
}

#[test]
fn an_exported_macro_can_be_invoked_from_where_nothing_is_seen() {
    let src = "#[macro_export]\nmacro_rules! m { ($p:expr) => { include_str!(concat!(\"../../d/\", $p)) } }\n\
               const A: &str = m!(\"x.txt\");\n";
    assert_eq!(
        scan_lib(src),
        "  reads anything: a/src/lib.rs (include_str!) spells its path with $p\n"
    );
}

/// `macro_export\b`: a longer word is another attribute.
#[test]
fn a_longer_attribute_name_is_not_macro_export() {
    let src = "#[macro_exporté]\nmacro_rules! m { ($p:expr) => { include_str!(concat!(\"../../d/\", $p)) } }\n\
               const A: &str = m!(\"x.txt\");\n";
    assert_eq!(scan_lib(src), read_of("d", "a/src/lib.rs (include_str!)"));
    let args = src.replace("macro_exporté", "macro_export(local_inner_macros)");
    assert_eq!(
        scan_lib(&args),
        "  reads anything: a/src/lib.rs (include_str!) spells its path with $p\n"
    );
}

#[test]
fn a_metavariable_outside_any_macro_reads_anything() {
    assert_eq!(
        scan_lib("const A: &str = include_str!(concat!(\"x\", $p));\n"),
        "  reads anything: a/src/lib.rs (include_str!) spells its path with $p\n"
    );
}

#[test]
fn a_macro_piece_reads_its_arms() {
    let arms = |value: &str| {
        format!(
            "macro_rules! name {{ () => {{ {value} }} }}\n\
             const A: &str = include_str!(concat!(\"../../d/\", name!()));\n"
        )
    };
    assert_eq!(
        scan_lib(&arms("\"x.txt\"")),
        read_of("d", "a/src/lib.rs (include_str!)")
    );
    assert_eq!(
        scan_lib(&arms("\"../x.txt\"")),
        "  reads anything: a/src/lib.rs (include_str!) spells its path with name!()\n"
    );
    assert_eq!(
        scan_lib("const A: &str = include_str!(concat!(\"d/\", unknown!()));\n"),
        "  reads anything: a/src/lib.rs (include_str!) spells its path with unknown!()\n"
    );
}

#[test]
fn a_literal_climbing_after_an_unknown_piece_reads_anything() {
    assert_eq!(
        scan_lib(
            "macro_rules! m { ($p:expr) => { include_str!(concat!(\"d/\", $p, \"/../x\")) } }\n\
             const A: &str = m!(\"y\");\n"
        ),
        "  reads anything: a/src/lib.rs (include_str!) spells its path with $p\n"
    );
}

// ── `#[path]`, attributes and other macros ──────────────────────────────

#[test]
fn a_path_module_is_a_source_and_its_reads_are_scanned() {
    let r = Repo::one();
    r.put("a/src/lib.rs", "#[path = \"../../b/m.rs\"]\nmod m;\n");
    r.put("b/m.rs", "const A: &str = include_str!(\"data.txt\");\n");
    r.put(
        "b/m/inner.rs",
        "const B: &str = include_str!(\"../../d/y.txt\");\n",
    );
    assert_eq!(
        below_header(&r.explain()),
        format!(
            "{}{}{}",
            read_of("b/data.txt", "b/m.rs (include_str!)"),
            read_of("b/m", "a/src/lib.rs (#[path])"),
            read_of("d/y.txt", "b/m/inner.rs (include_str!)"),
        )
    );
}

#[test]
fn a_path_through_cfg_attr_and_a_spaced_attribute_are_seen() {
    let r = Repo::one();
    r.put(
        "a/src/lib.rs",
        "#[cfg_attr(feature = \"x\", path = \"../../b/m.rs\")]\nmod m;\n# [path = \"../../b/n.rs\"]\nmod n;\n",
    );
    r.put("b/m.rs", "");
    r.put("b/n.rs", "");
    assert_eq!(
        below_header(&r.explain()),
        format!(
            "{}{}",
            read_of("b/m", "a/src/lib.rs (#[path])"),
            read_of("b/n", "a/src/lib.rs (#[path])")
        )
    );
}

#[test]
fn a_path_to_nothing_outside_the_crate_is_still_a_read() {
    assert_eq!(
        scan_lib("#[path = \"../../b/gone.rs\"]\nmod m;\n"),
        read_of("b/gone.rs", "a/src/lib.rs (#[path])")
    );
}

/// A `#[path]` in a non-mod file is also tried from the directory named
/// after that file.
#[test]
fn a_path_is_tried_from_the_directory_beside_the_file() {
    let r = Repo::one();
    r.put("a/src/lib.rs", "");
    r.put("a/src/foo.rs", "#[path = \"../../../b/m.rs\"]\nmod m;\n");
    r.put("b/m.rs", "");
    assert_eq!(
        below_header(&r.explain()),
        read_of("b/m", "a/src/foo.rs (#[path])")
    );
}

#[test]
fn a_mod_rs_module_s_stem_is_its_directory() {
    let r = Repo::one();
    r.put("a/src/lib.rs", "#[path = \"../../b/m/mod.rs\"]\nmod m;\n");
    r.put("b/m/mod.rs", "");
    assert_eq!(
        below_header(&r.explain()),
        read_of("b/m", "a/src/lib.rs (#[path])")
    );
}

#[test]
fn an_attribute_or_macro_literal_with_dot_dot_names_a_path() {
    assert_eq!(
        scan_lib("#[embed(\"../../assets/x\")]\nstruct S;\n"),
        read_of("assets/x", "a/src/lib.rs (attribute)")
    );
    assert_eq!(
        scan_lib("embed_dir!(\"../../assets/x\");\n"),
        read_of("assets/x", "a/src/lib.rs (embed_dir!)")
    );
    assert_eq!(scan_lib("embed_dir!(\"assets/x\");\n"), "");
}

#[test]
fn a_runtime_macro_s_literal_names_nothing() {
    assert_eq!(
        scan_lib("fn f() { println!(\"../../x\"); assert_eq!(1, 1, \"../../y\"); }\n"),
        ""
    );
}

#[test]
fn a_literal_outside_a_macro_or_attribute_names_nothing() {
    assert_eq!(scan_lib("const A: &str = \"../../x\";\n"), "");
}

#[test]
fn a_nested_include_in_a_runtime_macro_is_found_on_its_own() {
    assert_eq!(
        scan_lib("fn f() { println!(\"{}\", include_str!(\"../../x\")); }\n"),
        read_of("x", "a/src/lib.rs (include_str!)")
    );
}

/// `\b` is Python's: a combining mark is no word character there, a letter
/// is, and a digit is. `\s` holds for the information separators.
#[test]
fn a_macro_name_starts_at_python_s_word_boundary() {
    let call = |before: &str| format!("{before}embed!(\"../../x\");\n");
    let found = read_of("x", "a/src/lib.rs (embed!)");
    assert_eq!(scan_lib(&call("")), found);
    assert_eq!(scan_lib(&call("\u{301}")), found);
    assert_eq!(scan_lib(&call("é")), "");
    assert_eq!(scan_lib(&call("1")), "");
    assert_eq!(scan_lib("embed\u{1c}!\u{a0}(\"../../x\");\n"), found);
    assert_eq!(scan_lib("embed != (\"../../x\");\n"), "");
}

#[test]
fn a_call_inside_a_call_is_scanned_on_its_own() {
    assert_eq!(
        scan_lib("embed!(inner!(\"../../x\"));\n"),
        read_of("x", "a/src/lib.rs (embed!)")
    );
    assert_eq!(
        scan_lib("println!(\"{}\", inner!(\"../../x\"));\n"),
        read_of("x", "a/src/lib.rs (inner!)")
    );
}

// ── Files as Python's text layer reads them ─────────────────────────────

#[test]
fn a_lone_carriage_return_ends_a_line_comment() {
    assert_eq!(
        scan_lib("// a\rconst A: &str = include_str!(\"../../x.txt\");\r"),
        read_of("x.txt", "a/src/lib.rs (include_str!)")
    );
}

#[test]
fn a_crlf_in_a_literal_reads_as_a_newline() {
    assert_eq!(lits(&xtask::text::universal("\"a\r\nb\rc\"")), ["a\nb\nc"]);
}

#[test]
fn invalid_utf8_reads_as_replacement_characters() {
    let r = Repo::one();
    r.put_bytes(
        "a/src/lib.rs",
        b"const A: &[u8] = include_bytes!(\"../../d\xff\xc3/x\");\n",
    );
    assert_eq!(
        below_header(&r.explain()),
        read_of("d\u{fffd}\u{fffd}/x", "a/src/lib.rs (include_bytes!)")
    );
}

/// Bytes in a source can spell a placeholder. The script stopped there with
/// `IndexError`, so `--explain` exits 1 and a `<rev>` checks everything.
#[test]
fn a_placeholder_spelled_in_a_source_stops_the_scan() {
    let r = Repo::one();
    r.put("a/src/lib.rs", "embed!(\u{1}9\u{2});\n");
    r.commit("base");
    let e = r.run(&["--explain"]);
    assert_eq!((e.out.as_str(), e.code), ("", Some(1)));
    assert_eq!(
        r.since("HEAD"),
        "all\tthe scope could not be computed (IndexError: list index out of range), \
         so every crate is checked\n"
    );
}

/// `\d` and `int()` take every decimal digit: `\u{663}` is 3, here the
/// fourth literal, and in a run of sets `\u{1d7d9}` is 1.
#[test]
fn a_placeholder_spelled_in_any_digits_names_its_literal() {
    let src = "const A: &str = \"a\";\nconst B: &str = \"../../b\";\n\
               const C: &str = \"c\";\nconst D: &str = \"../../x\";\n";
    assert_eq!(
        scan_lib(&format!("{src}embed!(\u{1}\u{663}\u{2});\n")),
        read_of("x", "a/src/lib.rs (embed!)")
    );
    assert_eq!(
        scan_lib(&format!("{src}embed!(\u{1}\u{1d7d9}\u{2});\n")),
        read_of("b", "a/src/lib.rs (embed!)")
    );
}

// ── Build scripts ───────────────────────────────────────────────────────

fn scan_build(src: &str) -> String {
    let r = Repo::one();
    r.put("a/build.rs", src);
    below_header(&r.explain())
}

#[test]
fn a_build_script_names_every_literal_that_climbs() {
    assert_eq!(
        scan_build("fn main() { let p = \"../d/x.txt\"; }\n"),
        read_of("d/x.txt", "a/build.rs (build script)")
    );
    assert_eq!(scan_build("fn main() { let p = \"d/x.txt\"; }\n"), "");
}

#[test]
fn a_build_script_literal_is_tried_against_the_crate_and_its_directory() {
    let r = Repo::new(&[("a", "build = \"gen/main.rs\"\n")]);
    r.put(
        "a/gen/main.rs",
        "const P: &str = \"../d/from_crate\";\nconst Q: &str = \"../../d/from_script\";\nfn main() {}\n",
    );
    assert_eq!(
        below_header(&r.explain()),
        format!(
            "{}{}",
            read_of("d/from_crate", "a/gen/main.rs (build script)"),
            read_of("d/from_script", "a/gen/main.rs (build script)")
        )
    );
}

#[test]
fn a_literal_that_climbs_outside_a_build_script_names_nothing() {
    let r = Repo::one();
    r.put("a/src/other.rs", "const P: &str = \"../../d/x\";\n");
    assert_eq!(below_header(&r.explain()), "");
}

#[test]
fn a_build_script_that_climbs_reads_anything() {
    let want = "  reads anything: a/build.rs climbs with .parent() or .ancestors()\n";
    assert_eq!(scan_build("fn main() { p.parent(); }\n"), want);
    assert_eq!(scan_build("fn main() { p . ancestors (); }\n"), want);
    assert_eq!(
        scan_build("// p.parent()\nfn main() { let s = \".parent()\"; }\n"),
        ""
    );
    assert_eq!(scan_build("fn main() { p.parents(); }\n"), "");
}

#[test]
fn a_build_script_s_modules_are_build_sources_both_ways() {
    let r = Repo::one();
    r.put("a/build.rs", "mod one;\nmod two;\nfn main() {}\n");
    r.put("a/one.rs", "const P: &str = \"../d/one\";\n");
    r.put("a/two/mod.rs", "const P: &str = \"../../d/two\";\n");
    assert_eq!(
        below_header(&r.explain()),
        format!(
            "{}{}",
            read_of("d/one", "a/one.rs (build script)"),
            read_of("d/two", "a/two/mod.rs (build script)")
        )
    );
}

/// The `mod` pattern consumes the character before the word, so of two with
/// nothing between them the second is not found.
#[test]
fn a_mod_straight_after_a_mod_is_not_found() {
    let r = Repo::one();
    r.put("a/build.rs", "mod one;mod two;\nfn main() {}\n");
    r.put("a/one.rs", "const P: &str = \"../d/one\";\n");
    r.put("a/two.rs", "const P: &str = \"../d/two\";\n");
    assert_eq!(
        below_header(&r.explain()),
        read_of("d/one", "a/one.rs (build script)")
    );
    r.put("a/build.rs", "mod one; mod two;\nfn main() {}\n");
    assert!(r.explain().contains("d/two"));
}

/// A build script is scanned as a source too, so its reason is listed once
/// per scan: the script's list held it twice and its `--explain` printed
/// it twice.
#[test]
fn a_build_script_s_include_it_cannot_spell_is_listed_per_scan() {
    let want = "  reads anything: a/build.rs (include_str!) spells its path with X\n";
    assert_eq!(
        scan_build("const A: &str = include_str!(X);\nfn main() {}\n"),
        format!("{want}{want}")
    );
}

// ── Links and the walk ──────────────────────────────────────────────────

#[test]
fn a_symbolic_link_names_what_it_points_at() {
    let r = Repo::one();
    r.put("shared/x.txt", "");
    symlink("../shared/x.txt", r.path("a/x.txt")).expect("a link");
    symlink("/nowhere/at/all", r.path("a/dead")).expect("a link");
    symlink("../../gone", r.path("a/left")).expect("a link");
    assert_eq!(
        below_header(&r.explain()),
        read_of("shared/x.txt", "a/x.txt (symbolic link)")
    );
}

#[test]
fn a_link_to_a_directory_is_named_and_followed_once() {
    let r = Repo::one();
    r.put(
        "shared/m.rs",
        "const A: &str = include_str!(\"../../d/x\");\n",
    );
    symlink("../shared", r.path("a/lnk")).expect("a link");
    symlink(".", r.path("a/self")).expect("a link");
    assert_eq!(
        below_header(&r.explain()),
        format!(
            "{}{}",
            read_of("d/x", "a/lnk/m.rs (include_str!)"),
            read_of("shared", "a/lnk (symbolic link)")
        )
    );
}

#[test]
fn the_walk_leaves_out_target_and_git_and_lists_in_order() {
    let r = Repo::one();
    r.put(
        "a/target/skip.rs",
        "const A: &str = include_str!(\"../../x\");\n",
    );
    r.put(
        "a/.git/skip.rs",
        "const A: &str = include_str!(\"../../y\");\n",
    );
    r.put("a/z.rs", "");
    r.put("a/b/y.rs", "");
    assert_eq!(below_header(&r.explain()), "");
    let root = r.0.0.to_string_lossy().into_owned();
    let found = walk(&format!("{root}/a"));
    let names: Vec<&str> = found
        .iter()
        .map(|f| f.strip_prefix(&format!("{root}/a/")).expect("under it"))
        .collect();
    assert_eq!(names, ["Cargo.toml", "z.rs", "b/y.rs", "src/lib.rs"]);
}

/// The first file to name a read gives its reason, and the queue is taken
/// from its end: the last file of the walk is scanned first.
#[test]
fn the_last_file_walked_gives_a_read_its_reason() {
    let r = Repo::one();
    let inc = "const A: &str = include_str!(\"../../x\");\n";
    r.put("a/src/one.rs", inc);
    r.put("a/src/two.rs", inc);
    assert_eq!(
        below_header(&r.explain()),
        read_of("x", "a/src/two.rs (include_str!)")
    );
}

// ── Targets and dependencies ────────────────────────────────────────────

#[test]
fn a_target_outside_the_crate_is_a_source() {
    let r = Repo::new(&[("a", "[[bin]]\nname = \"o\"\npath = \"../shared/o.rs\"\n")]);
    r.put(
        "shared/o.rs",
        "const A: &str = include_str!(\"../d/x\");\nfn main() {}\n",
    );
    assert_eq!(
        below_header(&r.explain()),
        format!(
            "{}{}",
            read_of("d/x", "shared/o.rs (include_str!)"),
            read_of("shared/o", "a/Cargo.toml (target o)")
        )
    );
}

#[test]
fn a_path_dependency_outside_the_members_is_a_read() {
    let r = Repo::new(&[("a", "[dependencies]\nout = { path = \"../outside\" }\n")]);
    r.put(
        "outside/Cargo.toml",
        "[package]\nname = \"out\"\nversion = \"0.1.0\"\n",
    );
    r.put("outside/src/lib.rs", "");
    assert_eq!(
        below_header(&r.explain()),
        read_of("outside", "a/Cargo.toml (path dependency)")
    );
}

#[test]
fn explain_lists_members_in_order_with_their_dependencies() {
    let r = Repo::new(&[
        (
            "z",
            "[dependencies]\nb = { path = \"../b\" }\n[dev-dependencies]\na = { path = \"../a\" }\n",
        ),
        ("b", ""),
        ("a", ""),
    ]);
    assert_eq!(
        r.explain(),
        "a (a)\n  depends on: -\nb (b)\n  depends on: -\nz (z)\n  depends on: a b\n"
    );
}

// ── A plain include!, which the script never returned on ────────────────

/// **Departure, by design.** `Member.run` re-queued the file of every
/// `include!` it resolved on every pass and looped while the queue held
/// anything, so a tree with one never returned: `ulimit -t 15` killed it at
/// exit 152, where the same tree with `include_str!` returned. Here the
/// include is read, its own include is followed, and it ends.
#[test]
fn a_plain_include_is_followed_and_the_scan_ends() {
    let r = Repo::one();
    r.put(
        "a/src/lib.rs",
        "const Y: &str = include_str!(Y);\ninclude!(\"../../shared/one.rs\");\n",
    );
    r.put("shared/one.rs", "include!(\"two.rs\");\n");
    r.put(
        "shared/two.rs",
        "const A: &str = include_str!(\"../d/x.txt\");\nconst B: &str = include_str!(X);\n",
    );
    assert_eq!(
        below_header(&r.explain_within_a_limit()),
        format!(
            "{}{}{}{}{}",
            read_of("d/x.txt", "shared/two.rs (include_str!)"),
            read_of("shared/one", "a/src/lib.rs (include!)"),
            read_of("shared/two", "shared/one.rs (include!)"),
            "  reads anything: a/src/lib.rs (include_str!) spells its path with Y\n",
            "  reads anything: shared/two.rs (include_str!) spells its path with X\n"
        )
    );
}

#[test]
fn a_plain_include_of_a_file_in_the_crate_ends() {
    let r = Repo::one();
    r.put(
        "a/src/lib.rs",
        "include!(\"x.rs\");\ninclude!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/x.rs\"));\n",
    );
    r.put("a/src/x.rs", "");
    assert_eq!(below_header(&r.explain_within_a_limit()), "");
}

// ── What a change reaches ───────────────────────────────────────────────

fn chain() -> Repo {
    let r = Repo::new(&[
        ("a", "[dependencies]\nb = { path = \"../b\" }\n"),
        ("b", "[dev-dependencies]\nc = { path = \"../c\" }\n"),
        ("c", ""),
        ("d", ""),
    ]);
    r.put(
        "d/src/lib.rs",
        "const A: &str = include_str!(\"../../shared/x.txt\");\n",
    );
    r.put("shared/x.txt", "");
    r.put(".gitignore", "*.ignored\n");
    r.commit("base");
    r
}

#[test]
fn nothing_changed_reaches_nothing() {
    assert_eq!(chain().since("HEAD"), "");
}

#[test]
fn a_change_reaches_its_crate_and_what_builds_from_it_of_every_kind() {
    let r = chain();
    r.put("c/src/lib.rs", "// edited\n");
    assert_eq!(
        r.since("HEAD"),
        "a\tbuilds from b\nb\tbuilds from c\nc\tchanged: c/src/lib.rs\n"
    );
}

#[test]
fn a_change_since_a_rev_counts_commits_and_the_working_tree() {
    let r = chain();
    let base = r.git(&["rev-parse", "HEAD"]);
    r.put("c/src/lib.rs", "// committed\n");
    r.commit("one");
    assert_eq!(
        r.since(&base),
        "a\tbuilds from b\nb\tbuilds from c\nc\tchanged: c/src/lib.rs\n"
    );
    assert_eq!(r.since("HEAD"), "");
    r.put("d/src/new.rs", "");
    assert_eq!(r.since("HEAD"), "d\tchanged: d/src/new.rs\n");
}

#[test]
fn an_untracked_file_is_a_change_and_an_ignored_one_is_not() {
    let r = chain();
    r.put("c/untracked.txt", "");
    r.put("d/skip.ignored", "");
    assert_eq!(
        r.since("HEAD"),
        "a\tbuilds from b\nb\tbuilds from c\nc\tchanged: c/untracked.txt\n"
    );
}

#[test]
fn a_deleted_file_is_a_change() {
    let r = chain();
    r.put("d/src/extra.rs", "");
    r.commit("extra");
    fs::remove_file(r.path("d/src/extra.rs")).expect("the file");
    assert_eq!(r.since("HEAD"), "d\tchanged: d/src/extra.rs\n");
}

/// Cargo reads a library's root, so its loss stops the scope.
#[test]
fn a_deleted_crate_root_checks_everything() {
    let r = chain();
    fs::remove_file(r.path("d/src/lib.rs")).expect("the file");
    assert!(
        r.since("HEAD")
            .starts_with("all\tthe scope could not be computed (CalledProcessError: ")
    );
}

#[test]
fn a_rename_is_both_its_paths() {
    let r = chain();
    r.put(
        "c/src/gone.rs",
        "fn a_body_long_enough_to_be_a_rename() {}\n",
    );
    r.commit("one");
    fs::rename(r.path("c/src/gone.rs"), r.path("d/src/here.rs")).expect("the move");
    r.git(&["add", "-A"]);
    assert_eq!(
        r.since("HEAD"),
        "a\tbuilds from b\nb\tbuilds from c\nc\tchanged: c/src/gone.rs\nd\tchanged: d/src/here.rs\n"
    );
}

#[test]
fn a_file_read_at_compile_time_reaches_its_reader() {
    let r = chain();
    r.put("shared/x.txt", "edited\n");
    assert_eq!(
        r.since("HEAD"),
        "d\treads shared/x.txt at compile time, in d/src/lib.rs (include_str!)\n"
    );
}

/// `p.startswith(r)`, a string's prefix and not a path's.
#[test]
fn a_read_is_matched_as_a_plain_prefix() {
    let r = chain();
    r.put("shared/x.txt.bak", "");
    assert_eq!(
        r.since("HEAD"),
        "d\treads shared/x.txt.bak at compile time, in d/src/lib.rs (include_str!)\n"
    );
}

#[test]
fn a_change_outside_every_crate_reaches_only_what_may_read_anything() {
    let r = chain();
    r.put("stray.txt", "");
    assert_eq!(r.since("HEAD"), "");
    r.put("c/build.rs", "fn main() { p.parent(); }\n");
    r.commit("climbs");
    r.put("d/src/in_a_member.rs", "");
    assert_eq!(r.since("HEAD"), "d\tchanged: d/src/in_a_member.rs\n");
    fs::remove_file(r.path("d/src/in_a_member.rs")).expect("the file");
    r.put("stray.txt", "edited\n");
    assert_eq!(
        r.since("HEAD"),
        "a\tbuilds from b\nb\tbuilds from c\n\
         c\tstray.txt changed, and c/build.rs climbs with .parent() or .ancestors(), \
         which the scan cannot follow\n"
    );
    r.put("c/edit.txt", "");
    assert_eq!(
        r.since("HEAD"),
        "a\tbuilds from b\nb\tbuilds from c\nc\tchanged: c/edit.txt\n"
    );
}

#[test]
fn a_change_in_a_merge_base_diff_is_found() {
    let r = chain();
    let base = r.git(&["rev-parse", "HEAD"]);
    r.git(&["checkout", "-q", "-b", "side"]);
    r.put("c/src/side.rs", "");
    r.commit("side");
    let side = r.git(&["rev-parse", "HEAD"]);
    r.git(&["checkout", "-q", &base]);
    r.put("d/src/main_line.rs", "");
    r.commit("main line");
    assert_eq!(r.git(&["merge-base", "HEAD", &side]), base);
    assert_eq!(r.since(&base), "d\tchanged: d/src/main_line.rs\n");
    assert_eq!(
        r.since(&side),
        "a\tbuilds from b\nb\tbuilds from c\nc\tchanged: c/src/side.rs\nd\tchanged: d/src/main_line.rs\n"
    );
}

#[test]
fn every_spelling_of_a_commit_is_read() {
    let r = chain();
    r.put("d/src/next.rs", "");
    r.commit("next");
    r.git(&["tag", "light", "HEAD^"]);
    r.git(&["tag", "-a", "-m", "t", "annotated", "HEAD^"]);
    let want = "d\tchanged: d/src/next.rs\n";
    let short = r.git(&["rev-parse", "--short", "HEAD^"]);
    for rev in [
        "HEAD^",
        "HEAD~1",
        "light",
        "annotated",
        "HEAD^^{commit}",
        short.as_str(),
    ] {
        assert_eq!(r.since(rev), want, "{rev}");
    }
}

#[test]
fn a_rev_that_is_no_commit_prints_fail_and_exits_1() {
    let r = chain();
    let blob = r.git(&["rev-parse", "HEAD:Cargo.toml"]);
    for rev in [
        "nonexistent",
        "HEAD^{tree}",
        blob.as_str(),
        "-x",
        "",
        "HEAD..HEAD",
    ] {
        let got = r.run(&[rev]);
        assert_eq!(got.code, Some(1), "{rev}");
        assert_eq!(
            got.out,
            format!("FAIL: --since {rev}: git knows no such commit\n")
        );
    }
}

#[test]
fn a_wrong_command_line_prints_usage_and_exits_2() {
    let r = chain();
    for args in [&[][..], &["HEAD", "HEAD"][..], &["--explain", "x"][..]] {
        let got = r.run(args);
        assert_eq!(got.code, Some(2), "{args:?}");
        assert_eq!(
            got.out,
            "usage: cargo xtask features-since <rev> | --explain\n"
        );
    }
}

#[test]
fn each_file_and_directory_that_reaches_everything_does() {
    for p in [
        "Cargo.lock",
        "rust-toolchain",
        "rust-toolchain.toml",
        ".envrc",
        "devenv.nix",
        "devenv.lock",
        "devenv.yaml",
        "flake.nix",
        "flake.lock",
        "scripts/check-features.sh",
        ".cargo/config.toml",
        "nix/any.nix",
        "xtask/src/features_since.rs",
        "xtask/src/py.rs",
        "xtask/src/py_path.rs",
        "xtask/src/text.rs",
    ] {
        let r = chain();
        r.put(p, "x\n");
        assert_eq!(
            r.since("HEAD"),
            format!("all\t{p} changed, so every crate is checked\n")
        );
    }
    let r = chain();
    r.put(
        "Cargo.toml",
        "[workspace]\nmembers = [\"a\", \"b\", \"c\", \"d\"]\nresolver = \"2\"\n",
    );
    assert_eq!(
        r.since("HEAD"),
        "all\tCargo.toml changed, so every crate is checked\n"
    );
}

/// The first in sorted order names the reason, ahead of any crate's.
#[test]
fn everything_is_checked_before_any_crate() {
    let r = chain();
    r.put("flake.nix", "x\n");
    r.put("a/src/lib.rs", "// edited\n");
    r.put(".envrc", "x\n");
    assert_eq!(
        r.since("HEAD"),
        "all\t.envrc changed, so every crate is checked\n"
    );
}

/// A file near one of the list is not on it.
#[test]
fn a_name_near_the_list_reaches_nothing() {
    let r = chain();
    r.put("sub/Cargo.lock", "");
    r.put(".cargo2/x", "");
    r.put("nixos/x", "");
    assert_eq!(r.since("HEAD"), "");
}

// ── A scope it cannot compute checks everything ─────────────────────────

const ALL_FROM: &str = "all\tthe scope could not be computed (";
const ALL_TO: &str = "), so every crate is checked\n";

#[test]
fn a_manifest_cargo_refuses_checks_everything() {
    let r = chain();
    r.put("d/Cargo.toml", "nonsense");
    assert_eq!(
        r.since("HEAD"),
        format!(
            "{ALL_FROM}CalledProcessError: Command '['cargo', 'metadata', '--no-deps', \
             '--format-version', '1']' returned non-zero exit status 101.{ALL_TO}"
        )
    );
    let e = r.run(&["--explain"]);
    assert_eq!((e.out.as_str(), e.code), ("", Some(1)));
}

#[test]
fn a_source_it_cannot_open_checks_everything() {
    let r = chain();
    r.put("d/src/secret.rs", "");
    let p = r.path("d/src/secret.rs");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).expect("the mode");
    let got = r.since("HEAD");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).expect("the mode");
    assert_eq!(
        got,
        format!(
            "{ALL_FROM}PermissionError: [Errno 13] Permission denied: 'd/src/secret.rs'{ALL_TO}"
        )
    );
}

#[test]
fn cargo_missing_checks_everything() {
    let r = chain();
    let git = String::from_utf8(
        Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .expect("sh")
            .stdout,
    )
    .expect("a path");
    let bare = r.path("bare-path");
    fs::create_dir_all(&bare).expect("the directory");
    symlink(git.trim(), bare.join("git")).expect("a link");
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "-m", "bare"]);
    let got = r.run_in(&r.0.0, Some(&bare.to_string_lossy()), &["HEAD"]);
    assert_eq!(got.code, Some(0));
    assert_eq!(
        got.out,
        format!(
            "{ALL_FROM}FileNotFoundError: [Errno 2] No such file or directory: 'cargo'{ALL_TO}"
        )
    );
}

/// The one reason whose words are not the script's: `json`'s message and
/// serde_json's differ, and the line and the exit do not.
#[test]
fn a_cargo_that_prints_no_json_checks_everything() {
    let r = chain();
    let bare = r.path("bare-path");
    fs::create_dir_all(&bare).expect("the directory");
    let git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("sh")
        .stdout;
    symlink(String::from_utf8_lossy(&git).trim(), bare.join("git")).expect("a link");
    let cargo = bare.join("cargo");
    fs::write(&cargo, "#!/bin/sh\necho nonsense\n").expect("the script");
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).expect("the mode");
    r.commit("bare");
    let got = r.run_in(&r.0.0, Some(&bare.to_string_lossy()), &["HEAD"]);
    assert_eq!(got.code, Some(0));
    assert!(
        got.out
            .starts_with("all\tthe scope could not be computed (JSONDecodeError: ")
    );
    assert!(got.out.ends_with(ALL_TO), "{}", got.out);
}

#[test]
fn a_run_outside_the_root_checks_everything() {
    let r = chain();
    let got = r.run_in(&r.path("a"), None, &["HEAD"]);
    assert_eq!(got.code, Some(0));
    let root = fs::canonicalize(&r.0.0).expect("the root");
    assert_eq!(
        got.out,
        format!(
            "{ALL_FROM}RuntimeError: cargo's workspace root {} is not the checkout{ALL_TO}",
            root.display()
        )
    );
}

// ── A path that is not UTF-8 ────────────────────────────────────────────

/// `chain()` with a commit that holds this path, and a working tree without
/// it: only the object store can hold such a name on every filesystem.
fn with_a_name_of_bytes(name: &[u8]) -> Repo {
    let r = chain();
    let blob = r.git(&["hash-object", "-t", "blob", "shared/x.txt"]);
    r.git(&["hash-object", "-w", "shared/x.txt"]);
    let mut entry = format!("100644,{blob},").into_bytes();
    entry.extend_from_slice(name);
    let run = |args: &[&OsStr]| {
        let st = Repo::scrub(Command::new("git").args(args).current_dir(&r.0.0))
            .status()
            .expect("git");
        assert!(st.success(), "{args:?}");
    };
    run(&[
        OsStr::new("update-index"),
        OsStr::new("--add"),
        OsStr::new("--cacheinfo"),
        OsStr::from_bytes(&entry),
    ]);
    r.git(&["commit", "-q", "-m", "a name of bytes"]);
    run(&[
        OsStr::new("update-index"),
        OsStr::new("--force-remove"),
        OsStr::from_bytes(name),
    ]);
    r
}

#[test]
fn a_path_not_printed_does_not_stop_the_run() {
    let r = with_a_name_of_bytes(b"stray/\xff.txt");
    let got = r.run(&["HEAD"]);
    assert_eq!((got.out.as_str(), got.code), ("", Some(0)));
}

#[test]
fn a_path_that_would_be_printed_exits_1_after_the_lines_before_it() {
    let r = with_a_name_of_bytes(b"c/\xff.txt");
    r.put("a/src/edited.rs", "");
    let got = r.run(&["HEAD"]);
    assert_eq!(
        (got.out.as_str(), got.code),
        ("a\tchanged: a/src/edited.rs\nb\tbuilds from c\n", Some(1))
    );
}

// ── The repository ──────────────────────────────────────────────────────

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the root")
        .to_path_buf()
}

/// The reads its documentation names, found in the real sources, and no
/// crate left to read anything: a line of this kind on this tree would
/// reach it from every change outside the members.
#[test]
fn the_repository_s_reads_are_found() {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["features-since", "--explain"])
        .current_dir(repository())
        .output()
        .expect("xtask");
    assert!(out.status.success());
    let e = String::from_utf8_lossy(&out.stdout);
    assert!(e.contains("baud-bench (baud-bench)\n"), "{e}");
    assert!(
        e.contains("  reads baud-app/src/trail  <- baud-bench/"),
        "{e}"
    );
    assert!(
        e.contains("  reads assets/starship/baud.toml  <- baud-app/"),
        "{e}"
    );
    assert!(!e.contains("reads anything"), "{e}");
}

/// What decides the scope is on the list that reaches every crate: a change
/// to any module this one calls could move a verdict.
#[test]
fn every_module_the_scope_uses_reaches_every_crate() {
    let src =
        fs::read_to_string(repository().join("xtask/src/features_since.rs")).expect("the source");
    let (list, _) = src
        .split_once("const EVERYTHING_DIRS")
        .expect("the list's end");
    for module in ["features_since", "py", "py_path", "text"] {
        assert!(
            list.contains(&format!("\"xtask/src/{module}.rs\"")),
            "{module} is not on the list"
        );
    }
    let uses: Vec<&str> = src
        .lines()
        .filter(|l| l.starts_with("use crate::"))
        .collect();
    assert_eq!(
        uses,
        [
            "use crate::py;",
            "use crate::py_path::{basename, dirname, join, normpath, realpath, relpath, stem};",
            "use crate::text;",
        ]
    );
}
