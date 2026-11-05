//! **`baud-oracle xkb`, `raster` and `shape-helpers`, held to what their
//! Python made**: the xkb fixtures regenerate byte for byte and a changed
//! input fails `--check`; the raster output of one face is the digest
//! freetype-py's script printed for it; and every digest in the shape
//! fixtures' fnv64 column that names a repository file is that file's.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use baud_oracle::repo;

fn oracle(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_baud-oracle"))
        .args(args)
        .output()
        .expect("baud-oracle runs")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a directory");
    for e in std::fs::read_dir(from).expect("a directory") {
        let e = e.expect("an entry");
        let target = to.join(e.file_name());
        if e.file_type().expect("a type").is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), &target).expect("a copy");
        }
    }
}

/// A tree with the xkb fixtures and the two libX11 tables, which `--root`
/// reads; `name` keeps concurrent tests apart.
fn xkb_tree(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("baud-oracle-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(
        &repo().join("baud-xkb/tests/fixtures"),
        &root.join("baud-xkb/tests/fixtures"),
    );
    let sources = root.join("docs/research/sources");
    std::fs::create_dir_all(&sources).expect("a directory");
    for locale in ["en_US.UTF-8", "el_GR.UTF-8"] {
        let f = format!("libx11-1.8.13-compose-{locale}.txt");
        std::fs::copy(
            repo().join("docs/research/sources").join(&f),
            sources.join(&f),
        )
        .expect("a copy");
    }
    root
}

fn check(root: &Path) -> Output {
    oracle(&[
        "xkb",
        "--check",
        "--root",
        root.to_str().expect("a UTF-8 path"),
    ])
}

/// **Every xkb fixture regenerates byte for byte**, the four keymaps and
/// the twelve Compose tables, each table under the environment it was
/// recorded in.
#[test]
fn every_xkb_fixture_regenerates_as_committed() {
    let root = xkb_tree("xkb-identity");
    let out = check(&root);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "16 xkb fixtures, as committed\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// **The positive controls of `--check`**, the perturbations the Python
/// and Rust arms were run on side by side: `a` given the keysym `b` in
/// the US keymap, and a Compose result changed, each fail it and name
/// their file.
#[test]
fn a_changed_keymap_or_compose_file_fails_the_check() {
    for (file, from, to, oracle_file) in [
        (
            "keymap-us.xkb",
            "[               a,               A ]",
            "[               b,               B ]",
            "oracle-us.txt",
        ),
        (
            "compose/edge.Compose",
            ": \"g1\"",
            ": \"g2\"",
            "compose/oracle-edge.txt",
        ),
    ] {
        let root = xkb_tree("xkb-perturbed");
        let p = root.join("baud-xkb/tests/fixtures").join(file);
        let text = std::fs::read_to_string(&p).expect("the fixture");
        assert_eq!(text.matches(from).count(), 1, "{file}: the anchor");
        std::fs::write(&p, text.replacen(from, to, 1)).expect("write");
        let out = check(&root);
        assert!(!out.status.success(), "{file}: the check passed");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.lines()
                .filter(|l| l.contains("differs from the committed file"))
                .count()
                == 1
                && err.contains(oracle_file),
            "{file}: {err}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// FNV-1a over the bytes, as `shape-helpers fnv64` and the shape test
/// compute it, kept here so the test does not take it from the code it
/// checks.
fn fnv64(data: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// **The raster oracle prints what freetype-py's script printed**: JetBrains
/// Mono Regular at the sizes `raster.rs` asks for, 1,975,492 bytes whose
/// digest was taken from the run both arms printed byte for byte; a face
/// index the file does not have fails, and a face with no outlines prints
/// nothing.
#[test]
fn raster_prints_what_the_script_printed() {
    let font = repo().join("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let font = font.to_str().expect("a UTF-8 path");
    let out = oracle(&["raster", font, "0", "12", "14", "16", "24"]);
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), 1_975_492);
    assert_eq!(format!("{:016x}", fnv64(&out.stdout)), "2a032267cdf4b2fa");
    let bad = oracle(&["raster", font, "5", "12"]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(bad.stdout.is_empty());
    let sbix = repo().join("baud-font/tests/fonts/sbix-strikes.ttf");
    let none = oracle(&["raster", sbix.to_str().expect("a UTF-8 path"), "0", "12"]);
    assert!(none.status.success() && none.stdout.is_empty());
}

/// **Every repository face in the shape fixtures hashes to its fnv64
/// column** through `shape-helpers fnv64`; the system faces and the
/// fetched Noto faces are not in the tree, and are skipped.
#[test]
fn every_repository_digest_in_the_shape_fixtures_is_the_helper_s() {
    let dir = repo().join("baud-font/tests/shape");
    let mut checked = std::collections::BTreeMap::new();
    for e in std::fs::read_dir(&dir).expect("the fixtures") {
        let path = e.expect("an entry").path();
        let text = std::fs::read_to_string(&path).expect("a fixture");
        if !text
            .lines()
            .any(|l| l.starts_with("# columns:") && l.contains("fnv64"))
        {
            continue;
        }
        for line in text
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let f: Vec<&str> = line.split('\t').collect();
            let (face, digest) = (f[0], f[2]);
            if face.starts_with('/') || face.starts_with("noto-cache/") {
                continue;
            }
            let want = checked.entry(face.to_string()).or_insert_with(|| {
                let out = oracle(&[
                    "shape-helpers",
                    "fnv64",
                    repo().join(face).to_str().expect("a UTF-8 path"),
                ]);
                assert!(out.status.success(), "{face}");
                String::from_utf8(out.stdout)
                    .expect("hex")
                    .trim()
                    .to_string()
            });
            assert_eq!(want, digest, "{}: {face}", path.display());
        }
    }
    assert!(checked.len() >= 6, "{checked:?}");
    let control = oracle(&["shape-helpers", "fnv64", "/nonexistent"]);
    assert_eq!(control.status.code(), Some(1));
}
