//! `cargo xtask loop-head`: `--self-test` as the script printed it, each
//! rule of the normaliser perturbed alone, and the command over a stand-in
//! for objdump that prints a listing, so every exit is reached with no
//! binary involved.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::features_since::Exit;
use xtask::html::Count;
use xtask::loop_head::{self, Instruction, PageOff, Stop, find, functions, normalise, reloads};

const NAME: &str = "ingest2::timed";

fn body(lines: &[&str]) -> Vec<Instruction> {
    let mut at = 0x1000;
    lines
        .iter()
        .map(|t| {
            let line = (at, (*t).to_string());
            at += 4;
            line
        })
        .collect()
}

fn norm(lines: &[&str]) -> Vec<String> {
    normalise(&body(lines), NAME, PageOff::Normalised).expect("a body")
}

fn one(line: &str) -> String {
    norm(&[line]).pop().expect("one line")
}

#[test]
fn the_self_test_holds_and_says_so_as_it_did() {
    assert_eq!(loop_head::self_test(), Ok("self-test: 6 of 6 hold"));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = loop_head::run(&["--self-test"], "objdump", &mut out, &mut err).expect("writers");
    assert_eq!(exit, Exit(0));
    assert_eq!(out, b"self-test: 6 of 6 hold\n");
}

#[test]
fn a_branch_inside_the_function_is_its_offset_from_the_start() {
    assert_eq!(one("b.hs\t0x1000 <ingest2::timed>"), "b.hs\t<+0x0>");
    let lines = norm(&["nop", "b\t0x1010 <ingest2::timed+0x10>"]);
    assert_eq!(lines[1], "b\t<+0x10>");
    // Back past the start.
    assert_eq!(one("cbz\tx0, 0xf00 <ingest2::timed>"), "cbz\tx0, <+-0x100>");
}

#[test]
fn a_call_keeps_its_name_and_a_stub_loses_it() {
    assert_eq!(
        one("bl\t0x100026d40 <<u64 as core::fmt::Display>::fmt>"),
        "bl\t<<u64 as core::fmt::Display>::fmt>"
    );
    assert_eq!(one("bl\t0x100065b80 <_writev+0x100065b80>"), "bl\t<stub>");
}

#[test]
fn a_page_target_is_page_and_an_offset_off_its_register_is_pageoff() {
    let lines = norm(&[
        "adrp\tx8, 0x100067000 <GCC_except_table431+0x4>",
        "add\tx8, x8, #0xa00",
        "ldr\tx26, [x8, #0x2e8]",
        "adr\tw9, 0x100067100 <foo>",
        "str\tx1, [x9, #-0x18]",
    ]);
    assert_eq!(lines[0], "adrp\tx8, PAGE");
    assert_eq!(lines[1], "add\tx8, x8, #PAGEOFF");
    assert_eq!(lines[2], "ldr\tx26, [x8, #PAGEOFF]");
    assert_eq!(lines[3], "adr\tw9, PAGE");
    // The `w` form names the `x` register's offsets too.
    assert_eq!(lines[4], "str\tx1, [x9, #PAGEOFF]");
}

#[test]
fn only_a_register_an_adr_writes_is_rewritten() {
    assert_eq!(one("add\tx8, x9, #0x140"), "add\tx8, x9, #0x140");
    assert_eq!(one("ldr\tx26, [x8, #0x2e8]"), "ldr\tx26, [x8, #0x2e8]");
    // Function-wide: the `adrp` can come after.
    let lines = norm(&["ldr\tx26, [x8, #0x2e8]", "adrp\tx8, 0x100067000 <t>"]);
    assert_eq!(lines[0], "ldr\tx26, [x8, #PAGEOFF]");
}

#[test]
fn no_pageoff_keeps_every_offset_and_still_normalises_the_target() {
    let kept = normalise(
        &body(&["adrp\tx8, 0x100067000 <t>", "add\tx8, x8, #0xa00"]),
        NAME,
        PageOff::Kept,
    )
    .expect("a body");
    assert_eq!(kept, ["adrp\tx8, PAGE", "add\tx8, x8, #0xa00"]);
}

#[test]
fn a_literal_load_is_lit_and_a_load_off_a_register_is_not() {
    assert_eq!(one("ldr\tq1, 0x100000010"), "ldr\tq1, LIT");
    assert_eq!(one("ldrb\tw1, 0x100000010"), "ldrb\tw1, LIT");
    // A target with a name has lost its address by then, as it did.
    assert_eq!(one("ldr\tx0, 0x100000000 <lit>"), "ldr\tx0, <lit>");
    // `str` is no load, so its operand keeps its form.
    assert_eq!(one("str\tx0, 0x100000000"), "str\tx0, 0x100000000");
}

#[test]
fn a_comment_goes_and_the_mnemonic_is_split_from_its_operands() {
    assert_eq!(one("mov\tx0, x1 ; =0x1"), "mov\tx0, x1");
    assert_eq!(one("ret"), "ret\t");
    assert_eq!(one("add   x0,  x1, x2"), "add\tx0,  x1, x2");
    let empty = normalise(&body(&[" ;"]), NAME, PageOff::Normalised);
    assert!(matches!(empty, Err(Stop::Crash(_))), "{empty:?}");
    assert_eq!(normalise(&[], NAME, PageOff::Normalised), Ok(Vec::new()));
}

#[test]
fn reloads_count_stack_frame_loads_only() {
    let lines = norm(&[
        "ldr\tx26, [sp, #0x2e8]",
        "ldur\tx1, [x29, #-0x8]",
        "ldp\tx1, x2, [x29, #-0x20]",
        "ldp\tx29, x30, [sp]",
        "ldr\tw3, [x27, #0x10]",
        "str\tx1, [sp, #0x8]",
        "ldr\tx0, 0x100000000",
    ]);
    assert_eq!(reloads(&lines), Count(4));
}

#[test]
fn functions_reads_headers_and_counts_a_name_they_share() {
    let lines = [
        "/tmp/a:\tfile format mach-o arm64",
        "100000000: \tbefore any header",
        "0000000100001000 <a>:",
        "100001000: \tret",
        "   100001004: \tnop",
        "not an instruction",
        "0000000100002000 <b>:",
        "0000000100003000 <a>:",
        "100003000: \tnop",
        "0000000100004000 <a>:",
    ];
    let l = functions(&lines).expect("a listing");
    assert_eq!(l.names, ["a", "b"]);
    assert_eq!(l.bodies["a"].len(), 3);
    assert_eq!(l.bodies["a"][0], (0x1_0000_1000, "ret".to_string()));
    assert_eq!(l.bodies["b"].len(), 0);
    assert_eq!(l.shared["a"], Count(3));
}

#[test]
fn find_fails_on_a_shared_name_and_names_what_ends_alike() {
    let l = functions(&[
        "0000000100001000 <x::f>:",
        "0000000100002000 <x::f>:",
        "0000000100003000 <y::f>:",
        "0000000100004000 <z::g>:",
    ])
    .expect("a listing");
    assert_eq!(
        find(&l, "x::f", "p"),
        Err(Stop::Fail("'x::f' names 2 functions in p".to_string()))
    );
    assert_eq!(
        find(&l, "w::f", "p"),
        Err(Stop::Fail(
            "no function named 'w::f' in p; ending alike: ['x::f', 'y::f']".to_string()
        ))
    );
    assert_eq!(find(&l, "z::g", "p").expect("found").len(), 0);
}

/// A directory with a stand-in objdump: it prints the file it is given and
/// fails on any path naming `fail`.
struct Bin(PathBuf);

static NEXT: AtomicUsize = AtomicUsize::new(0);

impl Bin {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("loop-head-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("the directory");
        let tool = dir.join("objdump");
        fs::write(
            &tool,
            "#!/bin/sh\nfor a; do f=$a; done\ncase \"$f\" in *fail*) echo \"boom\" >&2; exit 3;; esac\nexec cat \"$f\"\n",
        )
        .expect("the stand-in");
        let mut mode = fs::metadata(&tool).expect("its metadata").permissions();
        mode.set_mode(0o755);
        fs::set_permissions(&tool, mode).expect("executable");
        Self(dir)
    }

    fn listing(&self, name: &str, base: &str, lines: &[&str]) -> String {
        let mut text = format!("{base:0>16} <ingest2::timed>:\n");
        for (i, l) in lines.iter().enumerate() {
            text.push_str(&format!("{base}{i}: \t{l}\n"));
        }
        let path = self.0.join(name);
        fs::write(&path, text).expect("the listing");
        path.to_str().expect("utf-8").to_string()
    }

    fn run(&self, args: &[&str]) -> (Exit, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let tool = self.0.join("objdump");
        let tool = tool.to_str().expect("utf-8");
        let exit = loop_head::run(args, tool, &mut out, &mut err).expect("writers");
        (
            exit,
            String::from_utf8(out).expect("utf-8"),
            String::from_utf8(err).expect("utf-8"),
        )
    }
}

impl Drop for Bin {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn identical_names_the_form_it_held_under() {
    let bin = Bin::new();
    let a = bin.listing(
        "a",
        "1000",
        &["adrp\tx8, 0x100067000 <t>", "add\tx8, x8, #0xa00"],
    );
    let b = bin.listing(
        "b",
        "2000",
        &["adrp\tx8, 0x100099000 <u>", "add\tx8, x8, #0x140"],
    );
    let c = bin.listing(
        "c",
        "3000",
        &["adrp\tx8, 0x100099000 <u>", "add\tx8, x8, #0xa00"],
    );

    let (exit, out, _) = bin.run(&[&a, &b]);
    assert_eq!(exit, Exit(0));
    assert!(
        out.ends_with("IDENTICAL (page offsets normalised; with page offsets kept: DIFFER)\n"),
        "{out}"
    );
    let (exit, out, _) = bin.run(&[&a, &c]);
    assert_eq!(exit, Exit(0));
    assert!(
        out.ends_with("IDENTICAL (page offsets normalised; with page offsets kept: IDENTICAL)\n"),
        "{out}"
    );
    let (exit, out, _) = bin.run(&["--no-pageoff", &a, &c]);
    assert_eq!(exit, Exit(0));
    assert!(out.ends_with("IDENTICAL (page offsets kept)\n"), "{out}");
}

#[test]
fn differ_prints_the_diff_of_the_normalised_bodies_and_exits_1() {
    let bin = Bin::new();
    let a = bin.listing("a", "1000", &["nop", "add\tx8, x9, #0x1", "ret"]);
    let b = bin.listing("b", "2000", &["nop", "add\tx8, x9, #0x2", "ret"]);
    let (exit, out, _) = bin.run(&[&a, &b]);
    assert_eq!(exit, Exit(1));
    let tail = format!(
        "DIFFER (page offsets normalised)\n--- {a}\n+++ {b}\n@@ -1,3 +1,3 @@\n nop\t\n-add\tx8, x9, #0x1\n+add\tx8, x9, #0x2\n ret\t\n"
    );
    assert!(out.ends_with(&tail), "{out}");

    let (exit, out, _) = bin.run(&["--no-pageoff", &a, &b]);
    assert_eq!(exit, Exit(1));
    assert!(out.contains("DIFFER (page offsets kept)\n"), "{out}");
}

#[test]
fn describe_gives_count_loads_and_the_first_sixteen_of_the_digest() {
    let bin = Bin::new();
    let a = bin.listing("a", "1000", &["ldr\tx26, [sp, #0x8]", "ret"]);
    let (exit, out, _) = bin.run(&[&a]);
    assert_eq!(exit, Exit(0));
    let digest = xtask::sha256::hex(b"ldr\tx26, [sp, #0x8]\nret\t");
    assert_eq!(
        out,
        format!(
            "{a}: ingest2::timed: 2 instructions, 1 stack loads, body {}\n",
            &digest[..16]
        )
    );
}

#[test]
fn fn_names_the_function_and_only_the_last_one_counts() {
    let bin = Bin::new();
    let a = bin.listing("a", "1000", &["ret"]);
    let (exit, _, err) = bin.run(&["--fn", "missing", &a]);
    assert_eq!(exit, Exit(2));
    assert!(
        err.starts_with("loop-head: no function named 'missing'"),
        "{err}"
    );
    let (exit, out, _) = bin.run(&["--fn", "missing", "--fn", NAME, &a]);
    assert_eq!(exit, Exit(0));
    assert!(out.contains(": ingest2::timed: 1 instructions"), "{out}");
}

#[test]
fn a_bad_command_line_exits_2_and_the_first_failure_stops_the_second_arm() {
    let bin = Bin::new();
    let a = bin.listing("a", "1000", &["ret"]);
    for args in [
        &[][..],
        &["--fn"],
        &["--fn", "x"],
        &["--no-pageoff"],
        &["a", "b", "c"],
    ] {
        let (exit, out, err) = bin.run(args);
        assert_eq!(exit, Exit(2), "{args:?}");
        assert_eq!(out, "");
        assert!(err.starts_with("loop-head: "), "{err}");
    }
    // `--self-test` beside anything else is a path, as it was.
    let (exit, _, err) = bin.run(&["--self-test", &a]);
    assert_eq!(exit, Exit(2), "{err}");

    let missing = bin.0.join("fail");
    let missing = missing.to_str().expect("utf-8");
    let (exit, out, err) = bin.run(&[&a, missing]);
    assert_eq!(exit, Exit(2));
    assert!(err.contains("objdump failed on"), "{err}");
    assert!(err.ends_with(": boom\n"), "{err}");
    // The first arm was already described.
    assert!(out.contains("1 instructions"), "{out}");
}

#[test]
fn an_objdump_that_cannot_start_or_cannot_be_read_is_exit_1() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = loop_head::run(&["x"], "/nonexistent/objdump", &mut out, &mut err).expect("writers");
    assert_eq!(exit, Exit(1));
    assert!(out.is_empty());

    let bin = Bin::new();
    let bad = bin.0.join("bad");
    fs::write(&bad, b"\xff\n").expect("the bytes");
    let (exit, _, _) = bin.run(&[bad.to_str().expect("utf-8")]);
    assert_eq!(exit, Exit(1));
}
