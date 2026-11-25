//! **`unsafe` is allowed item by item, never a module at a time**: the
//! crate denies `unsafe_code` at its root, and each place that calls into
//! Vulkan or the C library allows it on that one function, `unsafe impl`,
//! `unsafe trait` or `extern` block. An allow on a module, an `impl` block
//! or a whole file would let every item under it call `unsafe` unseen.

#![cfg(target_os = "linux")]

use std::path::Path;

/// **Where `text`, a source file, allows `unsafe_code` wider than one
/// item**: each line naming the allow that is an inner attribute, or whose
/// item is not a function, an `unsafe impl`, an `unsafe trait` or an
/// `extern` block.
fn too_wide(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut wide = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let l = line.trim();
        if !l.contains("allow(unsafe_code)") {
            continue;
        }
        if l.starts_with("#![") {
            wide.push(format!("{}: {l}", i + 1));
            continue;
        }
        let item = lines[i + 1..]
            .iter()
            .map(|n| n.trim())
            .find(|n| !n.starts_with('#') && !n.starts_with("//"))
            .unwrap_or("");
        let bare = item
            .trim_start_matches("pub(crate) ")
            .trim_start_matches("pub ")
            .trim_start_matches("const ");
        let one = [
            "fn ",
            "unsafe fn ",
            "unsafe impl ",
            "unsafe trait ",
            "unsafe extern ",
        ]
        .iter()
        .any(|k| bare.starts_with(k));
        if !one {
            wide.push(format!("{}: {item}", i + 1));
        }
    }
    wide
}

/// **No allow in the crate is wider than one item**, and the root still
/// denies the lint. The controls: an inner allow, an allow on a module and
/// one on an `impl` block are each found, and one on a function is not.
#[test]
fn unsafe_is_allowed_on_one_item_at_a_time() {
    assert_eq!(
        too_wide("#![allow(unsafe_code)]\nfn f() {}").len(),
        1,
        "inner"
    );
    assert_eq!(
        too_wide("#[allow(unsafe_code)]\nmod m;").len(),
        1,
        "a module"
    );
    assert_eq!(
        too_wide("#[allow(unsafe_code)]\nimpl S {").len(),
        1,
        "an impl"
    );
    assert!(too_wide("#[allow(unsafe_code)]\n/// Doc.\npub fn f() {}").is_empty());
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let lib = std::fs::read_to_string(src.join("lib.rs")).expect("lib.rs");
    assert!(lib.contains("#![deny(unsafe_code)]"), "the root denies it");
    let mut read = 0;
    let mut wide = Vec::new();
    for entry in std::fs::read_dir(&src).expect("src") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            read += 1;
            let text = std::fs::read_to_string(&path).expect("a source");
            wide.extend(
                too_wide(&text)
                    .into_iter()
                    .map(|w| format!("{}:{w}", path.display())),
            );
        }
    }
    assert!(read >= 10, "the control: the sources read, {read}");
    assert!(
        wide.is_empty(),
        "allowed wider than one item:\n{}",
        wide.join("\n")
    );
}
