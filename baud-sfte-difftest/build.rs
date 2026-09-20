//! Compile sfte into a static library this crate links.
//!
//! **A raw `cc` invocation, not the `cc` crate**, because there is no
//! `[build-dependencies]` anywhere in this workspace -- `baud-xkb` and
//! `baud-wayland` both read pinned sources from `docs/research/sources/` and
//! generate with nothing at all, and that is the house style. What the crate
//! would buy is warning management, and sfte compiles clean under
//! `-Wall -Wextra`.
//!
//! **The pristine header is never compiled.** `docs/research/sources/sfte.h`
//! stays byte-identical to its `MANIFEST.toml` hash; this script copies it to
//! `OUT_DIR` and applies every patch under `patches/` as an exact-anchor
//! replacement, and **a patch whose anchor is found zero or several times fails
//! the build**. A patch that silently fails to apply is the no-op edit
//! `CLAUDE.md` records three times, and the build refusing is the positive
//! control -- there is no `patch(1)` fuzz here on purpose.
//!
//! The shape is `bench-others/multibench/build.rs`'s ghostty arm: find the
//! source, put the artefact in `OUT_DIR`, `rustc-link-lib=static`. It differs
//! in one thing: **a missing header is an error, not a skipped row.** This
//! crate is a workspace member so that `cargo test --workspace` builds it, and
//! an oracle that quietly builds without its subject is the absent-rather-
//! than-red failure that let `baud-difftest` sit unbuildable for its whole
//! life.
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let pristine = manifest.join("../docs/research/sources/sfte.h");
    let shim_c = manifest.join("src/sfte_shim.c");
    let patches = manifest.join("patches");

    println!("cargo:rerun-if-changed={}", pristine.display());
    println!("cargo:rerun-if-changed={}", shim_c.display());
    println!("cargo:rerun-if-changed={}", patches.display());
    println!("cargo:rerun-if-changed=shim");

    let src = fs::read_to_string(&pristine)
        .unwrap_or_else(|e| panic!("sfte.h is not vendored at {}: {e}", pristine.display()));
    let patched = apply_patches(&src, &patches);
    let header = out.join("sfte.h");
    fs::write(&header, patched).unwrap();

    let obj = out.join("sfte_shim.o");
    let mut cc = Command::new("cc");
    cc.args(["-std=c11", "-O2", "-Wall", "-Wextra", "-c"])
        .arg(&shim_c)
        .arg("-o")
        .arg(&obj)
        .arg(format!("-DSFTE_HDR=\"{}\"", header.display()));
    // The two stub headers exist only for hosts without <pty.h> and
    // <sys/timerfd.h>. On Linux the real ones exist and must not be shadowed.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        cc.arg("-I").arg(manifest.join("shim"));
    }
    let status = cc.status().expect("cc not found on PATH");
    assert!(status.success(), "sfte_shim.c failed to compile");

    let lib = out.join("libsfteshim.a");
    let status = Command::new("ar")
        .arg("crs")
        .arg(&lib)
        .arg(&obj)
        .status()
        .expect("ar not found on PATH");
    assert!(status.success(), "ar failed");

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=sfteshim");
}

/// Apply every `patches/*.md` in name order. A patch file carries an `## anchor`
/// fenced block and a `## replacement` fenced block; the anchor must occur
/// exactly once in the source.
fn apply_patches(src: &str, dir: &Path) -> String {
    let mut names: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    // A `ceiling-*` patch is a measurement arm, not a fix: it is applied only
    // when the build environment names it, so the default build -- the one
    // the oracle's tests and the throughput row use -- never carries one.
    let ceiling = env::var("SFTE_CEILING").unwrap_or_default();
    println!("cargo:rerun-if-env-changed=SFTE_CEILING");
    let mut cur = src.to_string();
    for p in names {
        let stem = p.file_stem().and_then(|x| x.to_str()).unwrap_or("");
        if let Some(arm) = stem.strip_prefix("ceiling-") {
            if ceiling != arm {
                continue;
            }
            println!(
                "cargo:warning=sfte ceiling arm `{arm}` applied: this build is wrong by construction"
            );
        }
        let text = fs::read_to_string(&p).unwrap();
        let anchor = fenced(&text, "## anchor", &p);
        let repl = fenced(&text, "## replacement", &p);
        let n = cur.matches(&anchor).count();
        assert!(
            n == 1,
            "{}: anchor found {n} times in sfte.h (must be exactly 1) -- \
             the pinned header no longer matches this patch",
            p.display()
        );
        cur = cur.replacen(&anchor, &repl, 1);
    }
    cur
}

/// The body of the first ``` fence after `heading`, without its trailing
/// newline.
fn fenced(text: &str, heading: &str, p: &Path) -> String {
    let after = text
        .split_once(heading)
        .unwrap_or_else(|| panic!("{}: no {heading} section", p.display()))
        .1;
    let (_, rest) = after
        .split_once("```")
        .unwrap_or_else(|| panic!("{}: no fence after {heading}", p.display()));
    // Drop the language tag on the opening fence line.
    let body = rest.split_once('\n').map(|x| x.1).unwrap_or(rest);
    let (body, _) = body
        .split_once("```")
        .unwrap_or_else(|| panic!("{}: unclosed fence after {heading}", p.display()));
    body.trim_end_matches('\n').to_string()
}
